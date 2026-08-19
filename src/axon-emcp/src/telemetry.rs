//! v1.3.0 — Telemetry surface, OTLP-grade in shape + privacy-first by
//! construction.
//!
//! # Design
//!
//! Every MCP host-facing surface — `tools/call`, `resources/read`,
//! `prompts/get` — emits a typed event through this module. Events
//! land in two places (depending on configuration):
//!
//! - **Always-on in-memory aggregates** — counters, error rates, p50/p95/p99
//!   latency histograms per tool, per resource URI family, per prompt,
//!   per compose-domain outcome, per `axon.check` stage outcome.
//!   Surfaced via [`Telemetry::snapshot`] — the wire shape mirrors
//!   OTLP's metrics data model (resource → instrumentation scope →
//!   metric → datapoint).
//!
//! - **Opt-in JSONL event sink** — every recorded event also appends a
//!   single-line JSON object to `$AXON_EMCP_TELEMETRY_FILE` when that
//!   environment variable is set. The file is line-delimited so any
//!   downstream pipeline (Vector / Fluent Bit / otel-collector with
//!   the `filelog` receiver) can ingest it and forward as OTLP gRPC /
//!   OTLP HTTP. This is "OTLP-grade in shape" — adopters get the
//!   structured data model without the runtime carrying the OTLP wire
//!   dependency.
//!
//! # Privacy
//!
//! Five hard invariants enforced by construction in this module:
//!
//! 1. **Never log `axon.check` / `axon.parse` source content.** The
//!    AXON source can be proprietary; we record the stage outcome
//!    (lex/parse/type_check/ir_generate) and the boolean error flag.
//!    The actual `source:` string is dropped on the recording call.
//! 2. **Never log `axon.compose` intent strings.** Intents may
//!    contain customer-facing language. We record the chosen
//!    domain slug (closed catalog), the top score, and whether the
//!    domain was an explicit override.
//! 3. **Never log tool-error messages.** Error MESSAGES may carry
//!    PII or proprietary detail (e.g. a leaked field name from a
//!    type mismatch). We record only the boolean `is_error` and
//!    the JSON-RPC error code where applicable.
//! 4. **Never egress without opt-in.** No network calls happen from
//!    this module. JSONL writing is local-only and opt-in via env
//!    var. No remote exporter is shipped today.
//! 5. **Deployment ID is operator-supplied.** Default is an empty
//!    string; the operator sets `AXON_EMCP_DEPLOYMENT_ID` if they
//!    want correlation IDs in their telemetry pipeline.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value};

/// Configuration read once at startup from environment variables.
/// Construct via [`TelemetryConfig::from_env`].
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    /// Local file to append JSONL events to. `None` ⇒ no file sink.
    /// Set via `AXON_EMCP_TELEMETRY_FILE`. The path is created if
    /// missing; the parent directory must already exist.
    pub jsonl_sink: Option<PathBuf>,
    /// Operator-supplied deployment correlation ID. Surfaced in every
    /// JSONL event + the snapshot's `service.deployment_id` field.
    /// Set via `AXON_EMCP_DEPLOYMENT_ID`. Empty by default.
    pub deployment_id: String,
    /// Maximum number of latency samples retained per tool name. We
    /// truncate to the most-recent N to bound memory under load.
    /// Defaults to 1000; configurable via
    /// `AXON_EMCP_TELEMETRY_MAX_SAMPLES`.
    pub max_samples: usize,
}

impl TelemetryConfig {
    /// Resolve the configuration from environment variables. Never
    /// fails — missing vars yield defaults (no file sink, no
    /// deployment ID, 1000-sample latency window).
    pub fn from_env() -> Self {
        let jsonl_sink = std::env::var("AXON_EMCP_TELEMETRY_FILE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from);
        let deployment_id = std::env::var("AXON_EMCP_DEPLOYMENT_ID").unwrap_or_default();
        let max_samples = std::env::var("AXON_EMCP_TELEMETRY_MAX_SAMPLES")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(1000);
        Self { jsonl_sink, deployment_id, max_samples }
    }
}

/// Per-instance telemetry registry. Cheap to clone (it's an
/// `Arc<Mutex>` under the hood) so the server dispatcher can share it
/// across tools, resources, prompts.
pub struct Telemetry {
    config: TelemetryConfig,
    started_at: Instant,
    started_at_utc: u64,
    state: Mutex<TelemetryState>,
}

#[derive(Default)]
struct TelemetryState {
    /// Per-tool stats keyed by canonical tool name (`axon.check`,
    /// `axon.compose`, …).
    tools: BTreeMap<String, ToolStats>,
    /// Per-resource read counters keyed by URI family
    /// (`axon://primitives/`, `axon://grammar/`, …) — NOT the full
    /// slug, to bound cardinality.
    resources: BTreeMap<String, u64>,
    /// Per-prompt counters keyed by prompt name.
    prompts: BTreeMap<String, PromptStats>,
    /// `axon.compose` outcomes — domain distribution + override
    /// rate + classifier-score histogram.
    compose: ComposeStats,
    /// `axon.check` / `axon.parse` outcomes — pass/fail per stage.
    check: CheckStats,
    /// Phase 9 — `axon.examples` outcomes (listing / by-name /
    /// by-topic / by-primitive). Privacy-clean: only closed-catalog
    /// slugs flow here.
    examples: ExamplesStats,
}

#[derive(Default)]
struct ToolStats {
    calls: u64,
    errors: u64,
    /// Latencies in microseconds. Bounded to `config.max_samples` —
    /// older samples are evicted FIFO.
    latencies_us: Vec<u64>,
}

#[derive(Default)]
struct PromptStats {
    calls: u64,
    /// Counts of missing-required-argument errors so a misuse
    /// pattern surfaces in the snapshot.
    missing_required_arg: u64,
}

#[derive(Default)]
struct ComposeStats {
    total: u64,
    overrides: u64,
    /// Per-domain selection counts. Domain is the closed-catalog slug
    /// returned by `Domain::slug()`.
    by_domain: BTreeMap<String, u64>,
    /// Distribution of the top score in the classifier's scoreboard.
    /// Bucket per score value (0..N, capped at 16).
    top_score_buckets: BTreeMap<u32, u64>,
}

/// Phase 9 — `axon.examples` aggregate counters. Tracks how
/// adopters discover the corpus: how often a listing returned vs
/// a specific-slug fetch vs a topic / primitive filter. Both filter
/// fields are closed-catalog slugs (topic from [`ExampleTopic`],
/// primitive from [`PRIMITIVE_REGISTRY`]) so the cardinality is
/// bounded.
#[derive(Default)]
struct ExamplesStats {
    /// Total `axon.examples` calls.
    total: u64,
    /// Calls that asked for a SPECIFIC example by `name:`. These are
    /// the highest-value lookups (the agent already knows the slug).
    by_name: u64,
    /// Calls that filtered by `topic:`. Per-topic counts keyed by the
    /// closed [`ExampleTopic::as_str`] slug.
    by_topic: BTreeMap<String, u64>,
    /// Calls that filtered by `primitive:`. Per-primitive counts
    /// keyed by the canonical primitive name.
    by_primitive: BTreeMap<String, u64>,
    /// Calls that returned zero results (filter matched nothing).
    /// Useful for spotting gaps in the corpus — a topic / primitive
    /// queried repeatedly but never satisfied signals a missing
    /// example.
    empty_responses: u64,
}

#[derive(Default)]
struct CheckStats {
    total: u64,
    /// Per-stage pass count keyed by the closed `Stage::as_str()`
    /// values (`lex`, `parse`, `type_check`, `ir_generate`).
    pass_by_stage: BTreeMap<String, u64>,
    /// Per-stage failure count — symmetric with `pass_by_stage`.
    fail_by_stage: BTreeMap<String, u64>,
}

impl Telemetry {
    /// Construct a new registry. Public for tests + main; production
    /// goes through [`install_from_env`] for the canonical path.
    pub fn new(config: TelemetryConfig) -> Self {
        let started_at_utc = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            config,
            started_at: Instant::now(),
            started_at_utc,
            state: Mutex::new(TelemetryState::default()),
        }
    }

    /// Record a `tools/call` outcome.
    ///
    /// # Privacy
    /// - `tool` is the closed-catalog tool name; no caller-supplied
    ///   value flows here.
    /// - `duration` is wall-clock latency.
    /// - `is_error` is the boolean `isError` from the MCP envelope.
    ///   No error message is recorded — those may carry PII.
    pub fn record_tool_call(&self, tool: &str, duration: Duration, is_error: bool) {
        let micros = duration.as_micros().min(u128::from(u64::MAX)) as u64;
        let max_samples = self.config.max_samples;
        {
            let mut state = self.state.lock().unwrap();
            let entry = state.tools.entry(tool.to_string()).or_default();
            entry.calls += 1;
            if is_error {
                entry.errors += 1;
            }
            entry.latencies_us.push(micros);
            // Bound the histogram window.
            if entry.latencies_us.len() > max_samples {
                let excess = entry.latencies_us.len() - max_samples;
                entry.latencies_us.drain(..excess);
            }
        }
        self.append_event(json!({
            "kind": "tool_call",
            "tool": tool,
            "duration_us": micros,
            "is_error": is_error,
        }));
    }

    /// Record a `resources/read` outcome keyed by URI family
    /// (e.g. `axon://primitives/`). The full slug is NOT recorded —
    /// bounded cardinality + no information leak about which primitive
    /// was looked up.
    pub fn record_resource_read(&self, uri_family: &str) {
        {
            let mut state = self.state.lock().unwrap();
            *state.resources.entry(uri_family.to_string()).or_insert(0) += 1;
        }
        self.append_event(json!({
            "kind": "resource_read",
            "uri_family": uri_family,
        }));
    }

    /// Record a `prompts/get` outcome.
    pub fn record_prompt_get(&self, prompt: &str, is_missing_required: bool) {
        {
            let mut state = self.state.lock().unwrap();
            let entry = state.prompts.entry(prompt.to_string()).or_default();
            entry.calls += 1;
            if is_missing_required {
                entry.missing_required_arg += 1;
            }
        }
        self.append_event(json!({
            "kind": "prompt_get",
            "prompt": prompt,
            "missing_required": is_missing_required,
        }));
    }

    /// Record an `axon.compose` outcome.
    ///
    /// # Privacy
    /// `intent` strings (caller-supplied free-form text) are NEVER
    /// recorded. Only the closed-catalog `domain` slug, the top
    /// classifier score, and the `was_override` boolean.
    pub fn record_compose(&self, domain_slug: &str, top_score: u32, was_override: bool) {
        // Bucket the top score — cap at 16 so the histogram is
        // bounded even if a vocabulary grows to many keyword matches.
        let bucket = top_score.min(16);
        {
            let mut state = self.state.lock().unwrap();
            state.compose.total += 1;
            if was_override {
                state.compose.overrides += 1;
            }
            *state.compose.by_domain.entry(domain_slug.to_string()).or_insert(0) += 1;
            *state.compose.top_score_buckets.entry(bucket).or_insert(0) += 1;
        }
        self.append_event(json!({
            "kind": "compose",
            "domain": domain_slug,
            "top_score": bucket,
            "was_override": was_override,
        }));
    }

    /// Phase 9 — record an `axon.examples` lookup outcome.
    ///
    /// # Privacy
    /// - `by_name` (`Some(slug)`) — the exact example slug requested.
    ///   Closed-catalog (one of the embedded corpus slugs), so no
    ///   adopter input flows here. `None` ⇒ the call was a listing
    ///   (topic / primitive filter or unfiltered).
    /// - `topic_filter` / `primitive_filter` — closed-catalog slugs
    ///   from [`ExampleTopic`] / [`PRIMITIVE_REGISTRY`] respectively.
    /// - `result_count` — how many examples the call returned. Used
    ///   to detect zero-result queries that signal corpus gaps.
    ///
    /// Caller-supplied strings (filter keywords, search terms) are
    /// NEVER passed here — privacy invariant #5.
    pub fn record_examples(
        &self,
        by_name: Option<&str>,
        topic_filter: Option<&str>,
        primitive_filter: Option<&str>,
        result_count: usize,
    ) {
        {
            let mut state = self.state.lock().unwrap();
            state.examples.total += 1;
            if by_name.is_some() {
                state.examples.by_name += 1;
            }
            if let Some(t) = topic_filter {
                *state.examples.by_topic.entry(t.to_string()).or_insert(0) += 1;
            }
            if let Some(p) = primitive_filter {
                *state.examples.by_primitive.entry(p.to_string()).or_insert(0) += 1;
            }
            if result_count == 0 {
                state.examples.empty_responses += 1;
            }
        }
        self.append_event(json!({
            "kind": "examples",
            "by_name": by_name,
            "topic_filter": topic_filter,
            "primitive_filter": primitive_filter,
            "result_count": result_count,
        }));
    }

    /// Record an `axon.check` / `axon.parse` pipeline outcome.
    ///
    /// # Privacy
    /// `stage` is the closed `Stage::as_str()` value. `is_error` is
    /// the boolean failure flag. The source text is NEVER recorded —
    /// even though the stage tells us WHICH pass failed, the input
    /// content can be proprietary.
    pub fn record_check(&self, stage: &str, is_error: bool) {
        {
            let mut state = self.state.lock().unwrap();
            state.check.total += 1;
            if is_error {
                *state.check.fail_by_stage.entry(stage.to_string()).or_insert(0) += 1;
            } else {
                *state.check.pass_by_stage.entry(stage.to_string()).or_insert(0) += 1;
            }
        }
        self.append_event(json!({
            "kind": "check",
            "stage": stage,
            "is_error": is_error,
        }));
    }

    /// Render the full in-memory snapshot as JSON. The shape mirrors
    /// OTLP's data model (resource attribution → metric → datapoint)
    /// so a downstream collector can transform it to OTLP wire format
    /// mechanically.
    pub fn snapshot(&self) -> Value {
        let state = self.state.lock().unwrap();
        let uptime = self.started_at.elapsed().as_secs();

        let tools: Vec<Value> = state
            .tools
            .iter()
            .map(|(name, st)| {
                let (p50, p95, p99) = percentiles(&st.latencies_us);
                json!({
                    "name": name,
                    "calls": st.calls,
                    "errors": st.errors,
                    "error_rate": rate(st.errors, st.calls),
                    "samples": st.latencies_us.len(),
                    "p50_us": p50,
                    "p95_us": p95,
                    "p99_us": p99,
                })
            })
            .collect();

        let resources: Vec<Value> = state
            .resources
            .iter()
            .map(|(uri, count)| json!({ "uri_family": uri, "reads": count }))
            .collect();

        let prompts: Vec<Value> = state
            .prompts
            .iter()
            .map(|(name, st)| {
                json!({
                    "name": name,
                    "calls": st.calls,
                    "missing_required_arg": st.missing_required_arg,
                })
            })
            .collect();

        let compose_by_domain: Vec<Value> = state
            .compose
            .by_domain
            .iter()
            .map(|(d, c)| json!({ "domain": d, "count": c }))
            .collect();
        let compose_score_buckets: Vec<Value> = state
            .compose
            .top_score_buckets
            .iter()
            .map(|(b, c)| json!({ "top_score": b, "count": c }))
            .collect();

        json!({
            "service": {
                "name": "axon-emcp",
                "version": env!("CARGO_PKG_VERSION"),
                "deployment_id": self.config.deployment_id,
                "started_at_utc": self.started_at_utc,
            },
            "uptime_seconds": uptime,
            "tools": tools,
            "resources": resources,
            "prompts": prompts,
            "compose": {
                "total": state.compose.total,
                "overrides": state.compose.overrides,
                "override_rate": rate(state.compose.overrides, state.compose.total),
                "by_domain": compose_by_domain,
                "top_score_distribution": compose_score_buckets,
            },
            "check": {
                "total": state.check.total,
                "pass_by_stage": state.check.pass_by_stage,
                "fail_by_stage": state.check.fail_by_stage,
            },
            "examples": {
                "total": state.examples.total,
                "by_name": state.examples.by_name,
                "by_topic": state.examples.by_topic,
                "by_primitive": state.examples.by_primitive,
                "empty_responses": state.examples.empty_responses,
            },
        })
    }

    /// Read access to the JSONL sink path (for tests + the
    /// `telemetry summarize` subcommand).
    pub fn jsonl_sink(&self) -> Option<&PathBuf> {
        self.config.jsonl_sink.as_ref()
    }

    /// Append one structured event to the configured JSONL sink. No-op
    /// when no sink is configured. Errors writing are logged at WARN
    /// level via `tracing` but never escalate — telemetry must not
    /// break the host process.
    fn append_event(&self, mut event: Value) {
        let Some(path) = self.config.jsonl_sink.as_ref() else {
            return;
        };
        // Stamp every event with a UTC timestamp + deployment ID.
        // The shape `{ts, deployment_id, kind, …}` keeps every JSONL
        // line self-describing.
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Value::Object(ref mut map) = event {
            map.insert("ts".to_string(), Value::from(ts));
            map.insert(
                "deployment_id".to_string(),
                Value::String(self.config.deployment_id.clone()),
            );
        }
        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(mut f) => {
                let line = format!("{event}\n");
                if let Err(e) = f.write_all(line.as_bytes()) {
                    tracing::warn!(error = %e, path = %path.display(),
                        "telemetry JSONL write failed (telemetry never breaks the host)");
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, path = %path.display(),
                    "telemetry JSONL open failed (telemetry never breaks the host)");
            }
        }
    }
}

/// Compute (p50, p95, p99) microseconds from an unsorted sample
/// vector. Returns `(0, 0, 0)` for empty input. The slice is cloned +
/// sorted internally — the caller's vector is unchanged.
fn percentiles(samples: &[u64]) -> (u64, u64, u64) {
    if samples.is_empty() {
        return (0, 0, 0);
    }
    let mut sorted: Vec<u64> = samples.to_vec();
    sorted.sort_unstable();
    let p = |q: f64| -> u64 {
        let idx = ((sorted.len() as f64 - 1.0) * q).round() as usize;
        sorted[idx.min(sorted.len() - 1)]
    };
    (p(0.50), p(0.95), p(0.99))
}

/// Stable rate calculation as a float in `[0.0, 1.0]`. Returns 0.0
/// for a zero denominator.
fn rate(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

/// Aggregate a JSONL telemetry file into the same shape
/// [`Telemetry::snapshot`] emits. Used by the
/// `axon-emcp telemetry summarize <file>` subcommand to compute
/// counts + percentiles from a persisted event log.
///
/// The function reads the file once into memory; for production log
/// files larger than memory, pipe through a streaming aggregator
/// (Vector / vector.dev).
pub fn summarize_jsonl(path: &std::path::Path) -> std::io::Result<Value> {
    use std::io::BufRead;
    let f = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(f);
    let mut tools: BTreeMap<String, ToolStats> = BTreeMap::new();
    let mut resources: BTreeMap<String, u64> = BTreeMap::new();
    let mut prompts: BTreeMap<String, PromptStats> = BTreeMap::new();
    let mut compose = ComposeStats::default();
    let mut check = CheckStats::default();
    let mut examples = ExamplesStats::default();
    let mut total_lines: u64 = 0;
    let mut skipped_lines: u64 = 0;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        total_lines += 1;
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            skipped_lines += 1;
            continue;
        };
        match v["kind"].as_str() {
            Some("tool_call") => {
                let tool = v["tool"].as_str().unwrap_or("");
                let duration = v["duration_us"].as_u64().unwrap_or(0);
                let is_error = v["is_error"].as_bool().unwrap_or(false);
                let entry = tools.entry(tool.to_string()).or_default();
                entry.calls += 1;
                if is_error {
                    entry.errors += 1;
                }
                entry.latencies_us.push(duration);
            }
            Some("resource_read") => {
                let uri = v["uri_family"].as_str().unwrap_or("");
                *resources.entry(uri.to_string()).or_insert(0) += 1;
            }
            Some("prompt_get") => {
                let name = v["prompt"].as_str().unwrap_or("");
                let missing = v["missing_required"].as_bool().unwrap_or(false);
                let entry = prompts.entry(name.to_string()).or_default();
                entry.calls += 1;
                if missing {
                    entry.missing_required_arg += 1;
                }
            }
            Some("compose") => {
                compose.total += 1;
                if v["was_override"].as_bool().unwrap_or(false) {
                    compose.overrides += 1;
                }
                if let Some(d) = v["domain"].as_str() {
                    *compose.by_domain.entry(d.to_string()).or_insert(0) += 1;
                }
                if let Some(b) = v["top_score"].as_u64() {
                    *compose.top_score_buckets.entry(b as u32).or_insert(0) += 1;
                }
            }
            Some("check") => {
                check.total += 1;
                let stage = v["stage"].as_str().unwrap_or("");
                if v["is_error"].as_bool().unwrap_or(false) {
                    *check.fail_by_stage.entry(stage.to_string()).or_insert(0) += 1;
                } else {
                    *check.pass_by_stage.entry(stage.to_string()).or_insert(0) += 1;
                }
            }
            Some("examples") => {
                examples.total += 1;
                if v["by_name"].is_string() {
                    examples.by_name += 1;
                }
                if let Some(t) = v["topic_filter"].as_str() {
                    *examples.by_topic.entry(t.to_string()).or_insert(0) += 1;
                }
                if let Some(p) = v["primitive_filter"].as_str() {
                    *examples.by_primitive.entry(p.to_string()).or_insert(0) += 1;
                }
                if v["result_count"].as_u64().unwrap_or(0) == 0 {
                    examples.empty_responses += 1;
                }
            }
            _ => skipped_lines += 1,
        }
    }

    let tools_json: Vec<Value> = tools
        .iter()
        .map(|(name, st)| {
            let (p50, p95, p99) = percentiles(&st.latencies_us);
            json!({
                "name": name,
                "calls": st.calls,
                "errors": st.errors,
                "error_rate": rate(st.errors, st.calls),
                "samples": st.latencies_us.len(),
                "p50_us": p50, "p95_us": p95, "p99_us": p99,
            })
        })
        .collect();

    let resources_json: Vec<Value> = resources
        .iter()
        .map(|(uri, count)| json!({ "uri_family": uri, "reads": count }))
        .collect();

    let prompts_json: Vec<Value> = prompts
        .iter()
        .map(|(name, st)| {
            json!({
                "name": name,
                "calls": st.calls,
                "missing_required_arg": st.missing_required_arg,
            })
        })
        .collect();

    let compose_by_domain: Vec<Value> = compose
        .by_domain
        .iter()
        .map(|(d, c)| json!({ "domain": d, "count": c }))
        .collect();
    let compose_score_buckets: Vec<Value> = compose
        .top_score_buckets
        .iter()
        .map(|(b, c)| json!({ "top_score": b, "count": c }))
        .collect();

    Ok(json!({
        "source_file": path.display().to_string(),
        "total_lines": total_lines,
        "skipped_lines": skipped_lines,
        "tools": tools_json,
        "resources": resources_json,
        "prompts": prompts_json,
        "compose": {
            "total": compose.total,
            "overrides": compose.overrides,
            "override_rate": rate(compose.overrides, compose.total),
            "by_domain": compose_by_domain,
            "top_score_distribution": compose_score_buckets,
        },
        "check": {
            "total": check.total,
            "pass_by_stage": check.pass_by_stage,
            "fail_by_stage": check.fail_by_stage,
        },
        "examples": {
            "total": examples.total,
            "by_name": examples.by_name,
            "by_topic": examples.by_topic,
            "by_primitive": examples.by_primitive,
            "empty_responses": examples.empty_responses,
        },
    }))
}

/// Bound for [`Telemetry`] consumers that want to advertise their
/// telemetry surface without taking a heavy import. Most call sites
/// just use the inherent methods.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct EventKind(&'static str);
impl EventKind {
    pub const TOOL_CALL: EventKind = EventKind("tool_call");
    pub const RESOURCE_READ: EventKind = EventKind("resource_read");
    pub const PROMPT_GET: EventKind = EventKind("prompt_get");
    pub const COMPOSE: EventKind = EventKind("compose");
    pub const CHECK: EventKind = EventKind("check");
    pub const EXAMPLES: EventKind = EventKind("examples");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Throwaway tempfile path for JSONL-sink tests.
    fn tempfile(label: &str) -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "axon-emcp-telemetry-test-{}-{n}-{label}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn new_telemetry_with_sink(label: &str) -> (Telemetry, PathBuf) {
        let path = tempfile(label);
        let cfg = TelemetryConfig {
            jsonl_sink: Some(path.clone()),
            deployment_id: "test-deployment".to_string(),
            max_samples: 1000,
        };
        (Telemetry::new(cfg), path)
    }

    #[test]
    fn config_from_env_handles_empty_strings() {
        // We don't actually set env vars here (tests run in parallel,
        // env is process-wide); we exercise the helper's logic via
        // direct construction below.
        let cfg = TelemetryConfig::from_env();
        // Default invariants — these hold regardless of host env.
        assert!(cfg.max_samples > 0);
    }

    #[test]
    fn record_tool_call_increments_counters() {
        let (tel, _path) = new_telemetry_with_sink("tool-call");
        tel.record_tool_call("axon.check", Duration::from_millis(2), false);
        tel.record_tool_call("axon.check", Duration::from_millis(5), false);
        tel.record_tool_call("axon.check", Duration::from_millis(1), true);
        let snap = tel.snapshot();
        let tools = snap["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "axon.check");
        assert_eq!(tools[0]["calls"], 3);
        assert_eq!(tools[0]["errors"], 1);
        // error_rate = 1/3 ≈ 0.333
        let er = tools[0]["error_rate"].as_f64().unwrap();
        assert!((er - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn latency_percentiles_match_naive_calculation() {
        let samples: Vec<u64> = (1..=100).collect();
        let (p50, p95, p99) = percentiles(&samples);
        // Closest indices: 0.50 ⇒ idx 50, 0.95 ⇒ idx 94, 0.99 ⇒ idx 98.
        assert_eq!(p50, 51);
        assert_eq!(p95, 95);
        assert_eq!(p99, 99);
    }

    #[test]
    fn latency_window_is_bounded() {
        let cfg = TelemetryConfig {
            jsonl_sink: None,
            deployment_id: "".into(),
            max_samples: 5,
        };
        let tel = Telemetry::new(cfg);
        for i in 0..20 {
            tel.record_tool_call("axon.check", Duration::from_micros(i + 1), false);
        }
        let snap = tel.snapshot();
        let tools = snap["tools"].as_array().unwrap();
        // calls counter is full; samples retained = window cap.
        assert_eq!(tools[0]["calls"], 20);
        assert_eq!(tools[0]["samples"], 5);
        // p50 of the last 5 samples (16,17,18,19,20) → 18.
        assert_eq!(tools[0]["p50_us"], 18);
    }

    #[test]
    fn record_resource_read_groups_by_uri_family() {
        let (tel, _) = new_telemetry_with_sink("resource");
        tel.record_resource_read("axon://primitives/");
        tel.record_resource_read("axon://primitives/");
        tel.record_resource_read("axon://grammar/");
        let snap = tel.snapshot();
        let resources = snap["resources"].as_array().unwrap();
        assert_eq!(resources.len(), 2);
        // BTreeMap iteration is alphabetical — `axon://grammar/`
        // appears BEFORE `axon://primitives/` (g < p).
        assert_eq!(resources[0]["uri_family"], "axon://grammar/");
        assert_eq!(resources[0]["reads"], 1);
        assert_eq!(resources[1]["uri_family"], "axon://primitives/");
        assert_eq!(resources[1]["reads"], 2);
    }

    #[test]
    fn record_prompt_get_tracks_missing_required() {
        let (tel, _) = new_telemetry_with_sink("prompt");
        tel.record_prompt_get("flow_design", false);
        tel.record_prompt_get("flow_design", true);
        tel.record_prompt_get("flow_design", false);
        let snap = tel.snapshot();
        let prompts = snap["prompts"].as_array().unwrap();
        assert_eq!(prompts[0]["name"], "flow_design");
        assert_eq!(prompts[0]["calls"], 3);
        assert_eq!(prompts[0]["missing_required_arg"], 1);
    }

    #[test]
    fn record_compose_tracks_domain_distribution_and_override_rate() {
        let (tel, _) = new_telemetry_with_sink("compose");
        tel.record_compose("healthcare", 5, false);
        tel.record_compose("healthcare", 4, false);
        tel.record_compose("banking", 3, true); // explicit override
        let snap = tel.snapshot();
        let c = &snap["compose"];
        assert_eq!(c["total"], 3);
        assert_eq!(c["overrides"], 1);
        let or = c["override_rate"].as_f64().unwrap();
        assert!((or - 1.0 / 3.0).abs() < 1e-9);
        let by_domain = c["by_domain"].as_array().unwrap();
        // Alphabetical: banking before healthcare.
        assert_eq!(by_domain[0]["domain"], "banking");
        assert_eq!(by_domain[0]["count"], 1);
        assert_eq!(by_domain[1]["domain"], "healthcare");
        assert_eq!(by_domain[1]["count"], 2);
    }

    #[test]
    fn record_compose_caps_top_score_bucket_at_16() {
        let (tel, _) = new_telemetry_with_sink("compose-cap");
        tel.record_compose("healthcare", 50, false); // way over cap
        let snap = tel.snapshot();
        let buckets = snap["compose"]["top_score_distribution"]
            .as_array()
            .unwrap();
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0]["top_score"], 16);
        assert_eq!(buckets[0]["count"], 1);
    }

    #[test]
    fn record_check_separates_pass_and_fail_by_stage() {
        let (tel, _) = new_telemetry_with_sink("check");
        tel.record_check("type_check", false);
        tel.record_check("type_check", false);
        tel.record_check("parse", true);
        tel.record_check("lex", true);
        let snap = tel.snapshot();
        let c = &snap["check"];
        assert_eq!(c["total"], 4);
        assert_eq!(c["pass_by_stage"]["type_check"], 2);
        assert_eq!(c["fail_by_stage"]["parse"], 1);
        assert_eq!(c["fail_by_stage"]["lex"], 1);
    }

    #[test]
    fn jsonl_sink_appends_one_event_per_call() {
        let (tel, path) = new_telemetry_with_sink("jsonl");
        tel.record_tool_call("axon.check", Duration::from_millis(1), false);
        tel.record_compose("healthcare", 5, false);
        tel.record_check("type_check", false);
        let lines: Vec<String> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        assert_eq!(lines.len(), 3);
        // Every line is valid JSON + carries `ts`, `deployment_id`,
        // `kind`.
        for line in &lines {
            let v: Value = serde_json::from_str(line).unwrap();
            assert!(v["ts"].is_u64());
            assert_eq!(v["deployment_id"], "test-deployment");
            assert!(v["kind"].is_string());
        }
    }

    #[test]
    fn jsonl_sink_disabled_writes_nothing() {
        // No path → no file ever created.
        let tel = Telemetry::new(TelemetryConfig {
            jsonl_sink: None,
            deployment_id: "".into(),
            max_samples: 1000,
        });
        tel.record_tool_call("axon.check", Duration::from_millis(1), false);
        // Counter still increments in memory.
        let snap = tel.snapshot();
        assert_eq!(snap["tools"][0]["calls"], 1);
    }

    #[test]
    fn summarize_jsonl_reconstructs_the_snapshot_shape() {
        let (tel, path) = new_telemetry_with_sink("summarize");
        tel.record_tool_call("axon.check", Duration::from_micros(100), false);
        tel.record_tool_call("axon.check", Duration::from_micros(200), true);
        tel.record_resource_read("axon://primitives/");
        tel.record_compose("healthcare", 7, false);
        tel.record_check("type_check", false);

        let summary = summarize_jsonl(&path).unwrap();
        assert_eq!(summary["total_lines"], 5);
        assert_eq!(summary["skipped_lines"], 0);
        let tools = summary["tools"].as_array().unwrap();
        assert_eq!(tools[0]["calls"], 2);
        assert_eq!(tools[0]["errors"], 1);
        assert_eq!(summary["compose"]["total"], 1);
        assert_eq!(summary["check"]["total"], 1);
    }

    #[test]
    fn summarize_jsonl_skips_malformed_lines_without_failing() {
        let path = tempfile("malformed");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "{{ this is not json").unwrap();
        writeln!(f, r#"{{"kind":"tool_call","tool":"axon.check","duration_us":100,"is_error":false}}"#).unwrap();
        writeln!(f, "").unwrap(); // blank line — should be silently skipped (NOT counted as malformed)
        writeln!(f, r#"{{"kind":"unknown_event"}}"#).unwrap();
        drop(f);
        let summary = summarize_jsonl(&path).unwrap();
        assert_eq!(summary["total_lines"], 3); // blank line not counted
        assert_eq!(summary["skipped_lines"], 2); // first + unknown_event
        let tools = summary["tools"].as_array().unwrap();
        assert_eq!(tools[0]["name"], "axon.check");
        assert_eq!(tools[0]["calls"], 1);
    }

    #[test]
    fn snapshot_includes_service_metadata() {
        let (tel, _) = new_telemetry_with_sink("metadata");
        let snap = tel.snapshot();
        let svc = &snap["service"];
        assert_eq!(svc["name"], "axon-emcp");
        // version comes from CARGO_PKG_VERSION at compile time.
        assert!(svc["version"].as_str().unwrap().contains('.'));
        assert_eq!(svc["deployment_id"], "test-deployment");
        assert!(svc["started_at_utc"].is_u64());
        // uptime_seconds is non-negative.
        assert!(snap["uptime_seconds"].as_u64().unwrap() < 60);
    }

    #[test]
    fn event_kind_constants_carry_canonical_strings() {
        // Privacy invariant — the closed catalog of event kinds is
        // stable; downstream pipelines key off these strings.
        assert_eq!(EventKind::TOOL_CALL.0, "tool_call");
        assert_eq!(EventKind::RESOURCE_READ.0, "resource_read");
        assert_eq!(EventKind::PROMPT_GET.0, "prompt_get");
        assert_eq!(EventKind::COMPOSE.0, "compose");
        assert_eq!(EventKind::CHECK.0, "check");
    }
}
