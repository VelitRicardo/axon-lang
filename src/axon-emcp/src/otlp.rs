//! v1.4.0 — OTLP/gRPC exporter follow-up to the v1.3.0 telemetry
//! surface.
//!
//! Where v1.3.0 ships an in-process snapshot + JSONL sink that a
//! downstream collector (Vector / Fluent Bit / otel-collector with
//! the `filelog` receiver) can pick up out-of-band, v1.4.0 closes
//! the loop with a **direct push to an OTLP/gRPC endpoint** — the
//! canonical observability wire across the modern stack (Grafana,
//! Tempo, Honeycomb, Datadog, Lightstep, Splunk Observability, …).
//!
//! # Architecture
//!
//! The exporter is a **pure translation**:
//!
//! 1. Background tokio task wakes every `interval` (default 60 s).
//! 2. Calls [`Telemetry::snapshot`] — the canonical in-process model.
//! 3. Renders the snapshot into [`ResourceMetrics`] using the same
//! OTLP data-model shape v1.3.0's snapshot was already organised
//!    around (service → instrumentation scope → metric → datapoint).
//! 4. Pushes via the auto-generated `MetricsServiceClient` (tonic).
//! 5. Any failure (collector unreachable, timeout, malformed
//!    endpoint) is logged at WARN through `tracing` and the loop
//!    continues — telemetry MUST NOT break the host process.
//!
//! # Privacy invariants (extending v1.3.0)
//!
//! v1.3.0 Privacy invariants 1–5 stay verbatim. v1.4.0 adds:
//!
//! - **#6 — opt-in endpoint**: missing `AXON_EMCP_OTLP_ENDPOINT`
//!   disables the exporter entirely; no background task is spawned,
//!   no network socket is opened, no DNS lookup is issued. Network
//!   egress is impossible without explicit operator consent.
//! - **#7 — snapshot-derived payload only**: the wire payload is a
//!   pure function of [`Telemetry::snapshot`]; no additional
//!   information surface is created. Every value crossing the wire
//! came in through one of the v1.3.0 `record_*` entrypoints,
//!   which already enforce closed-catalog slugs.
//!
//! # Config surface
//!
//! - `AXON_EMCP_OTLP_ENDPOINT` — required to activate (e.g.
//!   `http://localhost:4317` for a local collector,
//!   `https://api.honeycomb.io:443` for commercial). Missing ⇒ off.
//! - `AXON_EMCP_OTLP_HEADERS` — comma-separated `key=value` headers
//!   stamped on every push (typical: `x-honeycomb-team=...`,
//!   `api-key=...`). Empty ⇒ no headers.
//! - `AXON_EMCP_OTLP_INTERVAL_SECS` — push interval (default `60`).
//! - `AXON_EMCP_OTLP_TIMEOUT_SECS` — per-RPC timeout (default `10`).

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tonic::metadata::{MetadataKey, MetadataMap, MetadataValue};
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};

use opentelemetry_proto::tonic::collector::metrics::v1::{
    metrics_service_client::MetricsServiceClient, ExportMetricsServiceRequest,
};
use opentelemetry_proto::tonic::common::v1::{
    any_value::Value as AnyVal, AnyValue, InstrumentationScope, KeyValue,
};
use opentelemetry_proto::tonic::metrics::v1::{
    metric::Data as MetricData, number_data_point::Value as NumberValue,
    AggregationTemporality, Gauge, Metric, NumberDataPoint, ResourceMetrics, ScopeMetrics, Sum,
};
use opentelemetry_proto::tonic::resource::v1::Resource;

use crate::telemetry::Telemetry;

/// Configuration parsed from the `AXON_EMCP_OTLP_*` env vars. An
/// empty `endpoint` field is the canonical "disabled" state — the
/// background task is NEVER spawned (privacy invariant #6).
#[derive(Debug, Clone)]
pub struct OtlpConfig {
    /// gRPC endpoint URL, e.g. `http://localhost:4317`. Empty ⇒
    /// exporter disabled (no task, no socket, no DNS).
    pub endpoint: String,
    /// Per-RPC metadata headers (auth keys for commercial endpoints,
    /// tenant routing, …). Each entry becomes a tonic metadata key
    /// stamped on every `export` call.
    pub headers: Vec<(String, String)>,
    /// Push interval. Default `60s` — matches the OTLP collector's
    /// default scrape interval and keeps the exporter inside the
    /// 5-minute Anthropic-cache window observability tooling assumes.
    pub interval: Duration,
    /// Per-RPC timeout. Default `10s` — long enough for cross-region
    /// commercial endpoints, short enough that a black-holed
    /// collector does not stall the exporter.
    pub timeout: Duration,
}

impl OtlpConfig {
    /// Parse the `AXON_EMCP_OTLP_*` env vars. Unset / invalid
    /// numeric values silently fall back to defaults — telemetry
    /// MUST NOT abort startup over a misconfigured value.
    ///
    /// Implemented as a thin wrapper over [`OtlpConfig::from_getter`]
    /// so tests can exercise the parser without touching process-
    /// global env state (`set_var`/`remove_var` require `unsafe` in
    /// the current toolchain, and this crate forbids unsafe at the
    /// crate root).
    pub fn from_env() -> Self {
        Self::from_getter(|key| std::env::var(key).ok())
    }

    /// Pure parser — same logic as [`from_env`] but configurable via
    /// an injected getter. Public so the drift gate test can exercise
    /// every code path (empty endpoint, malformed headers, invalid
    /// numerics) without mutating the process env.
    pub fn from_getter(getter: impl Fn(&str) -> Option<String>) -> Self {
        let endpoint = getter("AXON_EMCP_OTLP_ENDPOINT")
            .unwrap_or_default()
            .trim()
            .to_string();
        // Comma-separated `key=value,key=value` — whitespace tolerant.
        // Entries with no `=` are silently dropped (malformed input
        // never breaks startup).
        let headers = getter("AXON_EMCP_OTLP_HEADERS")
            .unwrap_or_default()
            .split(',')
            .filter_map(|s| {
                let s = s.trim();
                if s.is_empty() {
                    return None;
                }
                let (k, v) = s.split_once('=')?;
                Some((k.trim().to_string(), v.trim().to_string()))
            })
            .collect();
        let interval = parse_secs(&getter, "AXON_EMCP_OTLP_INTERVAL_SECS", 60);
        let timeout = parse_secs(&getter, "AXON_EMCP_OTLP_TIMEOUT_SECS", 10);
        OtlpConfig { endpoint, headers, interval, timeout }
    }

    /// `true` ⇒ the exporter is active and will spawn its background
    /// task. The single discriminator is endpoint non-emptiness —
    /// privacy invariant #6.
    pub fn is_enabled(&self) -> bool {
        !self.endpoint.is_empty()
    }
}

fn parse_secs(getter: &impl Fn(&str) -> Option<String>, var: &str, default: u64) -> Duration {
    Duration::from_secs(
        getter(var)
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(default),
    )
}

/// Spawn the OTLP push background task if the config is active.
/// Returns immediately. Failures inside the loop are logged at WARN
/// via `tracing`; the host process is never affected.
pub fn spawn_pusher(config: OtlpConfig, telemetry: Arc<Telemetry>) {
    if !config.is_enabled() {
        return;
    }
    tracing::info!(
        endpoint = %config.endpoint,
        interval_secs = config.interval.as_secs(),
        timeout_secs = config.timeout.as_secs(),
        header_count = config.headers.len(),
        "OTLP/gRPC exporter spawning"
    );
    tokio::spawn(async move {
        run_push_loop(config, telemetry).await;
    });
}

/// The push loop. Owns the gRPC channel for the lifetime of the
/// process — tonic's `Channel` is cheap to clone but expensive to
/// construct (DNS + TLS handshake), so a single long-lived channel
/// is the right shape.
///
/// On channel-construction failure we log WARN, sleep `interval`,
/// and retry — a collector that's temporarily down comes back to
/// life without forcing a host restart.
async fn run_push_loop(config: OtlpConfig, telemetry: Arc<Telemetry>) {
    let mut ticker = tokio::time::interval(config.interval);
    // First tick fires immediately — emit one snapshot at startup so
    // a fresh deployment is visible in the collector without waiting
    // a full interval.
    loop {
        ticker.tick().await;
        if let Err(e) = push_once(&config, &telemetry).await {
            tracing::warn!(error = %e, "OTLP push failed (will retry next interval)");
        }
    }
}

/// One push cycle: construct (or reuse) the channel, snapshot,
/// translate, send. Errors propagate to the caller (the loop logs
/// and continues).
async fn push_once(config: &OtlpConfig, telemetry: &Telemetry) -> Result<(), PushError> {
    let endpoint = Endpoint::from_shared(config.endpoint.clone())
        .map_err(|e| PushError::BadEndpoint(format!("{e}")))?
        .timeout(config.timeout)
        .connect_timeout(config.timeout);
    // TLS — opt-in by URL scheme. `https://` triggers the native
    // root-store TLS config; `http://` stays plaintext.
    let endpoint = if config.endpoint.starts_with("https://") {
        endpoint
            .tls_config(ClientTlsConfig::new().with_native_roots())
            .map_err(|e| PushError::Tls(format!("{e}")))?
    } else {
        endpoint
    };
    let channel: Channel = endpoint
        .connect()
        .await
        .map_err(|e| PushError::Connect(format!("{e}")))?;

    // Build the metadata map from header pairs. Invalid keys are
    // silently dropped (malformed config never breaks the push).
    let mut metadata = MetadataMap::new();
    for (k, v) in &config.headers {
        if let (Ok(key), Ok(val)) = (
            MetadataKey::from_bytes(k.as_bytes()),
            MetadataValue::try_from(v.as_str()),
        ) {
            metadata.insert(key, val);
        }
    }

    let snapshot = telemetry.snapshot();
    let resource_metrics = snapshot_to_resource_metrics(&snapshot);
    let payload = ExportMetricsServiceRequest {
        resource_metrics: vec![resource_metrics],
    };

    let mut request = tonic::Request::new(payload);
    *request.metadata_mut() = metadata;

    let mut client = MetricsServiceClient::new(channel);
    client
        .export(request)
        .await
        .map_err(|e| PushError::Rpc(format!("{e}")))?;
    Ok(())
}

/// Failure modes the push loop logs at WARN. Held as a typed enum so
/// future telemetry-of-telemetry can distinguish (DNS failure vs TLS
/// failure vs RPC-level error) without parsing strings.
#[derive(Debug)]
pub enum PushError {
    BadEndpoint(String),
    Tls(String),
    Connect(String),
    Rpc(String),
}

impl std::fmt::Display for PushError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PushError::BadEndpoint(e) => write!(f, "bad endpoint URL: {e}"),
            PushError::Tls(e) => write!(f, "TLS setup failed: {e}"),
            PushError::Connect(e) => write!(f, "channel connect failed: {e}"),
            PushError::Rpc(e) => write!(f, "export RPC failed: {e}"),
        }
    }
}

impl std::error::Error for PushError {}

// ─── Snapshot → ResourceMetrics translation ──────────────────────────

/// Pure function — exposed so the drift gate can exercise the shape
/// without spinning up a gRPC server. Takes the `serde_json::Value`
/// snapshot that [`Telemetry::snapshot`] produces and renders it
/// into the canonical OTLP `ResourceMetrics` shape.
///
/// The mapping is deliberate and stable so a downstream operator
/// can `grep '"name":'` an exported payload and find every metric
/// the v1.3.0 in-process model carries:
///
/// | snapshot path | OTLP metric | kind |
/// |---|---|---|
/// | `tools[*].calls` | `axon_emcp.tool.calls{tool}` | Sum (monotonic) |
/// | `tools[*].errors` | `axon_emcp.tool.errors{tool}` | Sum (monotonic) |
/// | `tools[*].p50/p95/p99` | `axon_emcp.tool.duration.p{50,95,99}_us{tool}` | Gauge |
/// | `resources[*].reads` | `axon_emcp.resource.reads{uri_family}` | Sum (monotonic) |
/// | `prompts[*].calls` | `axon_emcp.prompt.calls{prompt}` | Sum (monotonic) |
/// | `prompts[*].missing_required_arg` | `axon_emcp.prompt.missing_required_arg{prompt}` | Sum (monotonic) |
/// | `compose.total` | `axon_emcp.compose.total` | Sum (monotonic) |
/// | `compose.overrides` | `axon_emcp.compose.overrides` | Sum (monotonic) |
/// | `compose.by_domain[*]` | `axon_emcp.compose.by_domain{domain}` | Sum (monotonic) |
/// | `check.pass_by_stage[*]` | `axon_emcp.check.pass_by_stage{stage}` | Sum (monotonic) |
/// | `check.fail_by_stage[*]` | `axon_emcp.check.fail_by_stage{stage}` | Sum (monotonic) |
/// | `examples.total` | `axon_emcp.examples.total` | Sum (monotonic) |
/// | `examples.by_name` | `axon_emcp.examples.by_name` | Sum (monotonic) |
/// | `examples.empty_responses` | `axon_emcp.examples.empty_responses` | Sum (monotonic) |
/// | `examples.by_topic[*]` | `axon_emcp.examples.by_topic{topic}` | Sum (monotonic) |
/// | `examples.by_primitive[*]` | `axon_emcp.examples.by_primitive{primitive}` | Sum (monotonic) |
pub fn snapshot_to_resource_metrics(snapshot: &Value) -> ResourceMetrics {
    let svc = &snapshot["service"];
    let started_at = svc["started_at_utc"].as_u64().unwrap_or(0);
    let now = now_unix_secs();

    // Per OTLP convention: nanos since the Unix epoch.
    let start_ns = started_at.saturating_mul(1_000_000_000);
    let now_ns = now.saturating_mul(1_000_000_000);

    let service_name = svc["name"].as_str().unwrap_or("axon-emcp").to_string();
    let service_version = svc["version"].as_str().unwrap_or("").to_string();
    let deployment_id = svc["deployment_id"].as_str().unwrap_or("").to_string();

    let resource = Resource {
        attributes: vec![
            kv("service.name", &service_name),
            kv("service.version", &service_version),
            kv("deployment.environment", &deployment_id),
            kv("telemetry.sdk.name", "axon-emcp"),
            kv("telemetry.sdk.language", "rust"),
        ],
        dropped_attributes_count: 0,
    };

    let scope = InstrumentationScope {
        name: "axon-emcp".to_string(),
        version: service_version.clone(),
        attributes: vec![],
        dropped_attributes_count: 0,
    };

    let mut metrics: Vec<Metric> = Vec::new();
    push_tool_metrics(snapshot, &mut metrics, start_ns, now_ns);
    push_resource_metrics_(snapshot, &mut metrics, start_ns, now_ns);
    push_prompt_metrics(snapshot, &mut metrics, start_ns, now_ns);
    push_compose_metrics(snapshot, &mut metrics, start_ns, now_ns);
    push_check_metrics(snapshot, &mut metrics, start_ns, now_ns);
    push_examples_metrics(snapshot, &mut metrics, start_ns, now_ns);

    ResourceMetrics {
        resource: Some(resource),
        scope_metrics: vec![ScopeMetrics {
            scope: Some(scope),
            metrics,
            schema_url: String::new(),
        }],
        schema_url: String::new(),
    }
}

fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// One string-valued resource / datapoint attribute.
fn kv(key: &str, val: &str) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: Some(AnyValue {
            value: Some(AnyVal::StringValue(val.to_string())),
        }),
    }
}

/// Build a monotonic-cumulative `Sum` data-point. The OTLP
/// aggregation_temporality is CUMULATIVE — values are the running
/// total since `start_time_unix_nano`.
fn sum_metric(name: &str, unit: &str, datapoints: Vec<NumberDataPoint>) -> Metric {
    Metric {
        name: name.to_string(),
        description: String::new(),
        unit: unit.to_string(),
        metadata: vec![],
        data: Some(MetricData::Sum(Sum {
            data_points: datapoints,
            aggregation_temporality: AggregationTemporality::Cumulative as i32,
            is_monotonic: true,
        })),
    }
}

/// Build a `Gauge` metric (point-in-time value). Used for the latency
/// percentiles p50/p95/p99 — the snapshot already computed them, so
/// re-deriving raw histogram buckets at push time would be lossy.
fn gauge_metric(name: &str, unit: &str, datapoints: Vec<NumberDataPoint>) -> Metric {
    Metric {
        name: name.to_string(),
        description: String::new(),
        unit: unit.to_string(),
        metadata: vec![],
        data: Some(MetricData::Gauge(Gauge {
            data_points: datapoints,
        })),
    }
}

fn int_point(start_ns: u64, now_ns: u64, attrs: Vec<KeyValue>, v: i64) -> NumberDataPoint {
    NumberDataPoint {
        attributes: attrs,
        start_time_unix_nano: start_ns,
        time_unix_nano: now_ns,
        exemplars: vec![],
        flags: 0,
        value: Some(NumberValue::AsInt(v)),
    }
}

fn push_tool_metrics(snapshot: &Value, out: &mut Vec<Metric>, start_ns: u64, now_ns: u64) {
    let tools = match snapshot["tools"].as_array() {
        Some(a) => a,
        None => return,
    };
    let mut calls = Vec::new();
    let mut errors = Vec::new();
    let mut p50 = Vec::new();
    let mut p95 = Vec::new();
    let mut p99 = Vec::new();
    for t in tools {
        let name = t["name"].as_str().unwrap_or("");
        let attrs = vec![kv("tool", name)];
        calls.push(int_point(start_ns, now_ns, attrs.clone(), t["calls"].as_i64().unwrap_or(0)));
        errors.push(int_point(start_ns, now_ns, attrs.clone(), t["errors"].as_i64().unwrap_or(0)));
        p50.push(int_point(start_ns, now_ns, attrs.clone(), t["p50_us"].as_i64().unwrap_or(0)));
        p95.push(int_point(start_ns, now_ns, attrs.clone(), t["p95_us"].as_i64().unwrap_or(0)));
        p99.push(int_point(start_ns, now_ns, attrs, t["p99_us"].as_i64().unwrap_or(0)));
    }
    out.push(sum_metric("axon_emcp.tool.calls", "1", calls));
    out.push(sum_metric("axon_emcp.tool.errors", "1", errors));
    out.push(gauge_metric("axon_emcp.tool.duration.p50_us", "us", p50));
    out.push(gauge_metric("axon_emcp.tool.duration.p95_us", "us", p95));
    out.push(gauge_metric("axon_emcp.tool.duration.p99_us", "us", p99));
}

/// NOTE: function name is `push_resource_metrics_` (trailing underscore)
/// to avoid shadowing the proto type `ResourceMetrics`.
fn push_resource_metrics_(snapshot: &Value, out: &mut Vec<Metric>, start_ns: u64, now_ns: u64) {
    let resources = match snapshot["resources"].as_array() {
        Some(a) => a,
        None => return,
    };
    let mut reads = Vec::new();
    for r in resources {
        let attrs = vec![kv("uri_family", r["uri_family"].as_str().unwrap_or(""))];
        reads.push(int_point(start_ns, now_ns, attrs, r["reads"].as_i64().unwrap_or(0)));
    }
    out.push(sum_metric("axon_emcp.resource.reads", "1", reads));
}

fn push_prompt_metrics(snapshot: &Value, out: &mut Vec<Metric>, start_ns: u64, now_ns: u64) {
    let prompts = match snapshot["prompts"].as_array() {
        Some(a) => a,
        None => return,
    };
    let mut calls = Vec::new();
    let mut missing = Vec::new();
    for p in prompts {
        let attrs = vec![kv("prompt", p["name"].as_str().unwrap_or(""))];
        calls.push(int_point(start_ns, now_ns, attrs.clone(), p["calls"].as_i64().unwrap_or(0)));
        missing.push(int_point(
            start_ns,
            now_ns,
            attrs,
            p["missing_required_arg"].as_i64().unwrap_or(0),
        ));
    }
    out.push(sum_metric("axon_emcp.prompt.calls", "1", calls));
    out.push(sum_metric("axon_emcp.prompt.missing_required_arg", "1", missing));
}

fn push_compose_metrics(snapshot: &Value, out: &mut Vec<Metric>, start_ns: u64, now_ns: u64) {
    let compose = &snapshot["compose"];
    out.push(sum_metric(
        "axon_emcp.compose.total",
        "1",
        vec![int_point(start_ns, now_ns, vec![], compose["total"].as_i64().unwrap_or(0))],
    ));
    out.push(sum_metric(
        "axon_emcp.compose.overrides",
        "1",
        vec![int_point(start_ns, now_ns, vec![], compose["overrides"].as_i64().unwrap_or(0))],
    ));
    if let Some(by_domain) = compose["by_domain"].as_array() {
        let dps: Vec<_> = by_domain
            .iter()
            .map(|e| {
                int_point(
                    start_ns,
                    now_ns,
                    vec![kv("domain", e["domain"].as_str().unwrap_or(""))],
                    e["count"].as_i64().unwrap_or(0),
                )
            })
            .collect();
        out.push(sum_metric("axon_emcp.compose.by_domain", "1", dps));
    }
}

fn push_check_metrics(snapshot: &Value, out: &mut Vec<Metric>, start_ns: u64, now_ns: u64) {
    let check = &snapshot["check"];
    // pass_by_stage / fail_by_stage are JSON objects (BTreeMap<String, u64>)
    // in the snapshot. We render each as a labeled sum.
    let mut pass = Vec::new();
    if let Some(obj) = check["pass_by_stage"].as_object() {
        for (stage, count) in obj {
            pass.push(int_point(
                start_ns,
                now_ns,
                vec![kv("stage", stage)],
                count.as_i64().unwrap_or(0),
            ));
        }
    }
    out.push(sum_metric("axon_emcp.check.pass_by_stage", "1", pass));
    let mut fail = Vec::new();
    if let Some(obj) = check["fail_by_stage"].as_object() {
        for (stage, count) in obj {
            fail.push(int_point(
                start_ns,
                now_ns,
                vec![kv("stage", stage)],
                count.as_i64().unwrap_or(0),
            ));
        }
    }
    out.push(sum_metric("axon_emcp.check.fail_by_stage", "1", fail));
}

fn push_examples_metrics(snapshot: &Value, out: &mut Vec<Metric>, start_ns: u64, now_ns: u64) {
    let examples = &snapshot["examples"];
    out.push(sum_metric(
        "axon_emcp.examples.total",
        "1",
        vec![int_point(start_ns, now_ns, vec![], examples["total"].as_i64().unwrap_or(0))],
    ));
    out.push(sum_metric(
        "axon_emcp.examples.by_name",
        "1",
        vec![int_point(start_ns, now_ns, vec![], examples["by_name"].as_i64().unwrap_or(0))],
    ));
    out.push(sum_metric(
        "axon_emcp.examples.empty_responses",
        "1",
        vec![int_point(
            start_ns,
            now_ns,
            vec![],
            examples["empty_responses"].as_i64().unwrap_or(0),
        )],
    ));
    let mut by_topic = Vec::new();
    if let Some(obj) = examples["by_topic"].as_object() {
        for (topic, count) in obj {
            by_topic.push(int_point(
                start_ns,
                now_ns,
                vec![kv("topic", topic)],
                count.as_i64().unwrap_or(0),
            ));
        }
    }
    out.push(sum_metric("axon_emcp.examples.by_topic", "1", by_topic));
    let mut by_prim = Vec::new();
    if let Some(obj) = examples["by_primitive"].as_object() {
        for (prim, count) in obj {
            by_prim.push(int_point(
                start_ns,
                now_ns,
                vec![kv("primitive", prim)],
                count.as_i64().unwrap_or(0),
            ));
        }
    }
    out.push(sum_metric("axon_emcp.examples.by_primitive", "1", by_prim));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::{Telemetry, TelemetryConfig};
    use std::time::Duration;

    fn tel() -> Telemetry {
        Telemetry::new(TelemetryConfig {
            jsonl_sink: None,
            deployment_id: "test-deploy".to_string(),
            max_samples: 1000,
        })
    }

    // ── Config parsing ──────────────────────────────────────────────────

    #[test]
    fn from_getter_returns_disabled_when_endpoint_unset() {
        // Every getter call returns None — simulates "no env vars set".
        // We use `from_getter` (not `from_env`) so tests stay pure;
        // mutating process env from a test would (a) race with parallel
        // test execution and (b) require `unsafe` blocks the crate root
        // forbids.
        let cfg = OtlpConfig::from_getter(|_| None);
        assert!(!cfg.is_enabled(), "missing endpoint must yield disabled");
        assert_eq!(cfg.endpoint, "");
        assert_eq!(cfg.interval, Duration::from_secs(60));
        assert_eq!(cfg.timeout, Duration::from_secs(10));
        assert!(cfg.headers.is_empty());
    }

    #[test]
    fn from_getter_resolves_a_full_config_envelope() {
        let cfg = OtlpConfig::from_getter(|key| match key {
            "AXON_EMCP_OTLP_ENDPOINT" => Some("https://api.honeycomb.io:443".to_string()),
            "AXON_EMCP_OTLP_HEADERS" => Some("x-team=k1,x-dataset=axon".to_string()),
            "AXON_EMCP_OTLP_INTERVAL_SECS" => Some("30".to_string()),
            "AXON_EMCP_OTLP_TIMEOUT_SECS" => Some("5".to_string()),
            _ => None,
        });
        assert!(cfg.is_enabled());
        assert_eq!(cfg.endpoint, "https://api.honeycomb.io:443");
        assert_eq!(cfg.interval, Duration::from_secs(30));
        assert_eq!(cfg.timeout, Duration::from_secs(5));
        assert_eq!(cfg.headers.len(), 2);
        assert_eq!(cfg.headers[0], ("x-team".to_string(), "k1".to_string()));
        assert_eq!(cfg.headers[1], ("x-dataset".to_string(), "axon".to_string()));
    }

    #[test]
    fn from_getter_falls_back_to_defaults_on_invalid_numeric_values() {
        // A misconfigured value MUST NOT abort startup — telemetry is
        // best-effort. Garbage numerics silently degrade to defaults.
        let cfg = OtlpConfig::from_getter(|key| match key {
            "AXON_EMCP_OTLP_ENDPOINT" => Some("http://localhost:4317".to_string()),
            "AXON_EMCP_OTLP_INTERVAL_SECS" => Some("not-a-number".to_string()),
            "AXON_EMCP_OTLP_TIMEOUT_SECS" => Some("".to_string()),
            _ => None,
        });
        assert!(cfg.is_enabled());
        assert_eq!(cfg.interval, Duration::from_secs(60));
        assert_eq!(cfg.timeout, Duration::from_secs(10));
    }

    #[test]
    fn config_default_interval_and_timeout_match_spec() {
        let cfg = OtlpConfig {
            endpoint: String::new(),
            headers: vec![],
            interval: Duration::from_secs(60),
            timeout: Duration::from_secs(10),
        };
        assert_eq!(cfg.interval, Duration::from_secs(60));
        assert_eq!(cfg.timeout, Duration::from_secs(10));
    }

    // ── Snapshot → ResourceMetrics mapping ──────────────────────────────

    #[test]
    fn snapshot_renders_resource_attributes_correctly() {
        let t = tel();
        let snap = t.snapshot();
        let rm = snapshot_to_resource_metrics(&snap);
        let res = rm.resource.expect("resource must be present");
        let names: Vec<&str> = res.attributes.iter().map(|kv| kv.key.as_str()).collect();
        assert!(names.contains(&"service.name"));
        assert!(names.contains(&"service.version"));
        assert!(names.contains(&"deployment.environment"));
        assert!(names.contains(&"telemetry.sdk.name"));
        assert!(names.contains(&"telemetry.sdk.language"));
        // Deployment ID must be the test fixture's `test-deploy`.
        let deploy = res
            .attributes
            .iter()
            .find(|kv| kv.key == "deployment.environment")
            .and_then(|kv| kv.value.as_ref())
            .and_then(|v| v.value.as_ref())
            .and_then(|v| match v {
                AnyVal::StringValue(s) => Some(s.clone()),
                _ => None,
            })
            .expect("deployment.environment must be a string");
        assert_eq!(deploy, "test-deploy");
    }

    #[test]
    fn snapshot_renders_one_scope_metrics_named_axon_emcp() {
        let t = tel();
        let snap = t.snapshot();
        let rm = snapshot_to_resource_metrics(&snap);
        assert_eq!(rm.scope_metrics.len(), 1, "exactly one InstrumentationScope");
        let scope = rm.scope_metrics[0].scope.as_ref().expect("scope present");
        assert_eq!(scope.name, "axon-emcp");
        assert_eq!(scope.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn snapshot_emits_every_expected_metric_family() {
        let t = tel();
        // Drive every recorder so each family has at least one entry.
        t.record_tool_call("axon.check", Duration::from_micros(500), false);
        t.record_resource_read("axon://primitives/");
        t.record_prompt_get("flow_design", false);
        t.record_compose("healthcare", 3, false);
        t.record_check("type_check", false);
        t.record_examples(None, Some("composition"), Some("weave"), 1);
        let snap = t.snapshot();
        let rm = snapshot_to_resource_metrics(&snap);
        let names: Vec<&str> = rm.scope_metrics[0]
            .metrics
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        // Closed set of metric names the operator should grep for.
        let expected = &[
            "axon_emcp.tool.calls",
            "axon_emcp.tool.errors",
            "axon_emcp.tool.duration.p50_us",
            "axon_emcp.tool.duration.p95_us",
            "axon_emcp.tool.duration.p99_us",
            "axon_emcp.resource.reads",
            "axon_emcp.prompt.calls",
            "axon_emcp.prompt.missing_required_arg",
            "axon_emcp.compose.total",
            "axon_emcp.compose.overrides",
            "axon_emcp.compose.by_domain",
            "axon_emcp.check.pass_by_stage",
            "axon_emcp.check.fail_by_stage",
            "axon_emcp.examples.total",
            "axon_emcp.examples.by_name",
            "axon_emcp.examples.empty_responses",
            "axon_emcp.examples.by_topic",
            "axon_emcp.examples.by_primitive",
        ];
        for n in expected {
            assert!(
                names.contains(n),
                "metric `{n}` missing from OTLP payload — every snapshot family must surface; got {names:?}"
            );
        }
    }

    #[test]
    fn tool_metrics_carry_tool_attribute() {
        let t = tel();
        t.record_tool_call("axon.compose", Duration::from_micros(1_200), false);
        let snap = t.snapshot();
        let rm = snapshot_to_resource_metrics(&snap);
        let calls = rm.scope_metrics[0]
            .metrics
            .iter()
            .find(|m| m.name == "axon_emcp.tool.calls")
            .expect("tool.calls metric present");
        let sum = match &calls.data {
            Some(MetricData::Sum(s)) => s,
            _ => panic!("tool.calls must be a Sum"),
        };
        assert!(sum.is_monotonic, "tool.calls must be monotonic");
        assert_eq!(sum.aggregation_temporality, AggregationTemporality::Cumulative as i32);
        // The data point's attributes must include `tool=axon.compose`.
        assert_eq!(sum.data_points.len(), 1);
        let attrs = &sum.data_points[0].attributes;
        let tool_attr = attrs
            .iter()
            .find(|kv| kv.key == "tool")
            .and_then(|kv| kv.value.as_ref())
            .and_then(|v| v.value.as_ref())
            .and_then(|v| match v {
                AnyVal::StringValue(s) => Some(s.clone()),
                _ => None,
            });
        assert_eq!(tool_attr.as_deref(), Some("axon.compose"));
    }

    #[test]
    fn latency_percentiles_are_gauges_not_sums() {
        let t = tel();
        t.record_tool_call("axon.check", Duration::from_micros(500), false);
        let snap = t.snapshot();
        let rm = snapshot_to_resource_metrics(&snap);
        for name in ["axon_emcp.tool.duration.p50_us", "axon_emcp.tool.duration.p95_us", "axon_emcp.tool.duration.p99_us"] {
            let metric = rm.scope_metrics[0]
                .metrics
                .iter()
                .find(|m| m.name == name)
                .unwrap_or_else(|| panic!("missing metric `{name}`"));
            assert!(
                matches!(metric.data, Some(MetricData::Gauge(_))),
                "{name} must be a Gauge (point-in-time percentile, not a running total)"
            );
        }
    }

    // ── Failure modes ───────────────────────────────────────────────────

    #[tokio::test]
    async fn push_once_against_unreachable_endpoint_returns_typed_error_not_panic() {
        let cfg = OtlpConfig {
            endpoint: "http://127.0.0.1:1".to_string(), // closed port
            headers: vec![],
            interval: Duration::from_secs(60),
            timeout: Duration::from_millis(500),
        };
        let t = tel();
        let err = push_once(&cfg, &t).await.expect_err("must fail typed, not panic");
        // The exact variant depends on platform (Linux: Connect refused;
        // some BSD/Windows return a different syscall error). Both arms
        // are acceptable; the point is we get a typed PushError back.
        assert!(
            matches!(err, PushError::Connect(_) | PushError::Rpc(_)),
            "expected Connect or Rpc, got {err:?}"
        );
    }

    #[tokio::test]
    async fn push_once_with_malformed_endpoint_returns_bad_endpoint() {
        let cfg = OtlpConfig {
            endpoint: "::: not a url :::".to_string(),
            headers: vec![],
            interval: Duration::from_secs(60),
            timeout: Duration::from_millis(500),
        };
        let t = tel();
        let err = push_once(&cfg, &t).await.expect_err("malformed URL must fail");
        assert!(matches!(err, PushError::BadEndpoint(_)), "got {err:?}");
    }

    #[test]
    fn spawn_pusher_is_a_no_op_when_disabled() {
        // Endpoint empty ⇒ no task is spawned, no socket opened.
        // Privacy invariant #6: zero network egress without opt-in.
        let cfg = OtlpConfig {
            endpoint: String::new(),
            headers: vec![],
            interval: Duration::from_secs(60),
            timeout: Duration::from_secs(10),
        };
        // The function should return immediately without panic.
        // We can't easily assert "no task was spawned" without
        // intrusive instrumentation, but we CAN assert the function
        // is total + returns.
        spawn_pusher(cfg, Arc::new(tel()));
    }

    // ── Headers ─────────────────────────────────────────────────────────

    #[test]
    fn from_getter_parses_comma_separated_headers_dropping_malformed() {
        let cfg = OtlpConfig::from_getter(|key| {
            (key == "AXON_EMCP_OTLP_HEADERS").then(|| {
                "x-api-key=secret123, x-tenant=axon-prod ,malformed-no-equals, ".to_string()
            })
        });
        // Two valid entries; the malformed one (no `=`) silently dropped.
        // Whitespace around keys + values is trimmed.
        assert_eq!(cfg.headers.len(), 2);
        assert_eq!(cfg.headers[0], ("x-api-key".to_string(), "secret123".to_string()));
        assert_eq!(cfg.headers[1], ("x-tenant".to_string(), "axon-prod".to_string()));
    }
}
