//! Trace Store — in-memory execution trace buffer for AxonServer.
//!
//! Collects execution traces from deployed flow runs and provides
//! queryable access via the server API:
//!   - `GET /v1/traces`       — list/query recent traces
//!   - `GET /v1/traces/:id`   — get a specific trace by ID
//!   - `GET /v1/traces/stats` — aggregate analytics across buffered traces
//!
//! The store is a ring buffer with configurable capacity.
//! Each trace records: flow name, status, steps executed, latency,
//! token usage, anchor results, and a timestamped event log.

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

// ── Trace entry ─────────────────────────────────────────────────────────

/// A single execution trace.
#[derive(Debug, Clone, Serialize)]
pub struct TraceEntry {
    /// Unique trace ID (sequential).
    pub id: u64,
    /// Wall-clock timestamp (Unix seconds).
    pub timestamp: u64,
    /// Flow name that was executed.
    pub flow_name: String,
    /// Execution status.
    pub status: TraceStatus,
    /// Number of steps executed.
    pub steps_executed: usize,
    /// Total latency in milliseconds.
    pub latency_ms: u64,
    /// Input tokens used (if known).
    pub tokens_input: u64,
    /// Output tokens used (if known).
    pub tokens_output: u64,
    /// Anchor checks performed.
    pub anchor_checks: usize,
    /// Anchor breaches detected.
    pub anchor_breaches: usize,
    /// Error count.
    pub errors: usize,
    /// Retry count.
    pub retries: usize,
    /// Source file.
    pub source_file: String,
    /// Backend used (e.g., "anthropic").
    pub backend: String,
    /// Client identifier.
    pub client_key: String,
    /// Ordered event log.
    pub events: Vec<TraceEvent>,
    /// If this trace is a replay, the ID of the original trace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replay_of: Option<u64>,
    /// User-added annotations for debugging and collaboration.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<TraceAnnotation>,
    /// Correlation ID for linking related traces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

/// A user-added annotation on a trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceAnnotation {
    /// Who added the annotation.
    pub author: String,
    /// Free-form note text.
    pub text: String,
    /// Tags for categorization/filtering.
    pub tags: Vec<String>,
    /// Unix timestamp when the annotation was added.
    pub timestamp: u64,
}

/// Execution status for a trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TraceStatus {
    Success,
    Failed,
    Partial,
    Timeout,
}

impl TraceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TraceStatus::Success => "success",
            TraceStatus::Failed => "failed",
            TraceStatus::Partial => "partial",
            TraceStatus::Timeout => "timeout",
        }
    }
}

/// A single event within a trace.
#[derive(Debug, Clone, Serialize)]
pub struct TraceEvent {
    /// Event type (step_start, step_end, anchor_check, model_call, error, etc.).
    pub event_type: String,
    /// Relative timestamp in milliseconds from trace start.
    pub offset_ms: u64,
    /// Step name (if applicable).
    pub step_name: String,
    /// Event detail (free-form).
    pub detail: String,
}

// ── Config ──────────────────────────────────────────────────────────────

/// Trace store configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceStoreConfig {
    /// Maximum traces in the ring buffer.
    pub capacity: usize,
    /// Whether trace recording is enabled.
    pub enabled: bool,
    /// Maximum events per trace (to prevent memory bloat).
    pub max_events_per_trace: usize,
    /// Maximum age of a trace in seconds (0 = no TTL).
    pub max_age_secs: u64,
}

impl Default for TraceStoreConfig {
    fn default() -> Self {
        TraceStoreConfig {
            capacity: 500,
            enabled: true,
            max_events_per_trace: 200,
            max_age_secs: 0,
        }
    }
}

impl TraceStoreConfig {
    pub fn disabled() -> Self {
        TraceStoreConfig {
            capacity: 0,
            enabled: false,
            max_events_per_trace: 0,
            max_age_secs: 0,
        }
    }
}

// ── Store ───────────────────────────────────────────────────────────────

/// In-memory ring buffer for execution traces.
pub struct TraceStore {
    config: TraceStoreConfig,
    entries: VecDeque<TraceEntry>,
    next_id: u64,
    total_recorded: u64,
}

impl TraceStore {
    /// Create a new trace store.
    pub fn new(config: TraceStoreConfig) -> Self {
        TraceStore {
            entries: VecDeque::with_capacity(config.capacity.min(512)),
            config,
            next_id: 1,
            total_recorded: 0,
        }
    }

    /// Record a new trace. Returns the assigned trace ID.
    pub fn record(&mut self, mut trace: TraceEntry) -> u64 {
        if !self.config.enabled {
            return 0;
        }

        let id = self.next_id;
        self.next_id += 1;
        self.total_recorded += 1;

        trace.id = id;
        trace.timestamp = wall_clock_secs();

        // Truncate events if over limit
        if trace.events.len() > self.config.max_events_per_trace {
            trace.events.truncate(self.config.max_events_per_trace);
        }

        // Evict oldest if at capacity
        if self.entries.len() >= self.config.capacity && self.config.capacity > 0 {
            self.entries.pop_front();
        }
        if self.config.capacity > 0 {
            self.entries.push_back(trace);
        }

        id
    }

    /// v1.24.0 — Reserve a trace ID without recording the entry yet.
    ///
    /// Wire-streaming sites (`execute_sse_handler`) need a trace ID up
    /// front so the first `axon.token` event can carry it on the wire
    /// — adopters bind the live stream to the eventual replay surface
    /// via this id. The entry is finalized via `record_with_id` once
    /// the executor closes the FlowExecutionEvent channel.
    ///
    /// When the store is disabled the call still returns 0 to match
    /// `record`'s disabled-path semantics.
    pub fn reserve_id(&mut self) -> u64 {
        if !self.config.enabled {
            return 0;
        }
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// v1.24.0 — Persist a trace entry under a previously reserved id.
    ///
    /// Pairs with `reserve_id`: callers reserve early to plumb the id
    /// into wire events, then call here once execution has completed
    /// with final stats. The capacity / eviction / timestamp semantics
    /// match `record` verbatim — only the id-allocation timing differs.
    pub fn record_with_id(&mut self, mut trace: TraceEntry, id: u64) {
        if !self.config.enabled {
            return;
        }
        self.total_recorded += 1;
        trace.id = id;
        trace.timestamp = wall_clock_secs();

        if trace.events.len() > self.config.max_events_per_trace {
            trace.events.truncate(self.config.max_events_per_trace);
        }

        if self.entries.len() >= self.config.capacity && self.config.capacity > 0 {
            self.entries.pop_front();
        }
        if self.config.capacity > 0 {
            self.entries.push_back(trace);
        }
    }

    /// Get a trace by ID.
    pub fn get(&self, id: u64) -> Option<&TraceEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Query recent traces (newest first), optionally filtered.
    pub fn recent(&self, limit: usize, filter: Option<&TraceFilter>) -> Vec<&TraceEntry> {
        self.entries
            .iter()
            .rev()
            .filter(|e| match filter {
                Some(f) => f.matches(e),
                None => true,
            })
            .take(limit)
            .collect()
    }

    /// Number of buffered traces.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total traces recorded (including evicted).
    pub fn total_recorded(&self) -> u64 {
        self.total_recorded
    }

    /// Get a mutable trace by ID (for annotations).
    pub fn get_mut(&mut self, id: u64) -> Option<&mut TraceEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Annotate a trace by ID. Returns true if the trace was found.
    pub fn annotate(&mut self, id: u64, annotation: TraceAnnotation) -> bool {
        match self.get_mut(id) {
            Some(entry) => {
                entry.annotations.push(annotation);
                true
            }
            None => false,
        }
    }

    /// Bulk delete traces by IDs. Returns number actually deleted.
    pub fn bulk_delete(&mut self, ids: &[u64]) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| !ids.contains(&e.id));
        before - self.entries.len()
    }

    /// Bulk annotate traces by IDs. Returns number of traces annotated.
    pub fn bulk_annotate(&mut self, ids: &[u64], annotation: TraceAnnotation) -> usize {
        let mut count = 0;
        for entry in self.entries.iter_mut() {
            if ids.contains(&entry.id) {
                entry.annotations.push(annotation.clone());
                count += 1;
            }
        }
        count
    }

    /// Full-text search across buffered traces.
    ///
    /// Matches the query (case-insensitive substring) against:
    /// flow_name, source_file, backend, client_key, event step_name,
    /// event detail, annotation text, and annotation tags.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&TraceEntry> {
        let q = query.to_lowercase();
        self.entries
            .iter()
            .rev()
            .filter(|e| {
                e.flow_name.to_lowercase().contains(&q)
                    || e.source_file.to_lowercase().contains(&q)
                    || e.backend.to_lowercase().contains(&q)
                    || e.client_key.to_lowercase().contains(&q)
                    || e.events.iter().any(|ev| {
                        ev.step_name.to_lowercase().contains(&q)
                            || ev.detail.to_lowercase().contains(&q)
                    })
                    || e.annotations.iter().any(|a| {
                        a.text.to_lowercase().contains(&q)
                            || a.tags.iter().any(|t| t.to_lowercase().contains(&q))
                    })
            })
            .take(limit)
            .collect()
    }

    /// Evict traces older than `max_age_secs`. Returns number evicted.
    /// No-op if max_age_secs is 0.
    pub fn evict_expired(&mut self) -> usize {
        if self.config.max_age_secs == 0 {
            return 0;
        }
        let now = wall_clock_secs();
        let cutoff = now.saturating_sub(self.config.max_age_secs);
        let before = self.entries.len();
        self.entries.retain(|e| e.timestamp >= cutoff);
        before - self.entries.len()
    }

    /// Update retention policy. Returns previous max_age_secs.
    pub fn set_max_age_secs(&mut self, max_age_secs: u64) -> u64 {
        let prev = self.config.max_age_secs;
        self.config.max_age_secs = max_age_secs;
        prev
    }

    /// Get configuration.
    pub fn config(&self) -> &TraceStoreConfig {
        &self.config
    }

    /// Compute aggregate statistics across buffered traces.
    pub fn stats(&self) -> TraceStoreStats {
        let mut total_latency_ms: u64 = 0;
        let mut max_latency_ms: u64 = 0;
        let mut total_tokens_input: u64 = 0;
        let mut total_tokens_output: u64 = 0;
        let mut total_steps: usize = 0;
        let mut total_anchor_checks: usize = 0;
        let mut total_anchor_breaches: usize = 0;
        let mut total_errors: usize = 0;
        let mut total_retries: usize = 0;
        let mut status_counts = std::collections::HashMap::new();
        let mut flow_counts = std::collections::HashMap::new();

        for e in &self.entries {
            total_latency_ms += e.latency_ms;
            if e.latency_ms > max_latency_ms {
                max_latency_ms = e.latency_ms;
            }
            total_tokens_input += e.tokens_input;
            total_tokens_output += e.tokens_output;
            total_steps += e.steps_executed;
            total_anchor_checks += e.anchor_checks;
            total_anchor_breaches += e.anchor_breaches;
            total_errors += e.errors;
            total_retries += e.retries;
            *status_counts.entry(e.status.as_str().to_string()).or_insert(0u64) += 1;
            *flow_counts.entry(e.flow_name.clone()).or_insert(0u64) += 1;
        }

        let count = self.entries.len() as u64;
        let avg_latency_ms = if count > 0 { total_latency_ms / count } else { 0 };

        let mut top_flows: Vec<(String, u64)> = flow_counts.into_iter().collect();
        top_flows.sort_by(|a, b| b.1.cmp(&a.1));
        top_flows.truncate(10);

        let mut status_breakdown: Vec<(String, u64)> = status_counts.into_iter().collect();
        status_breakdown.sort_by_key(|(k, _)| k.clone());

        TraceStoreStats {
            total_recorded: self.total_recorded,
            buffered: self.entries.len(),
            avg_latency_ms,
            max_latency_ms,
            total_tokens_input,
            total_tokens_output,
            total_steps,
            total_anchor_checks,
            total_anchor_breaches,
            total_errors,
            total_retries,
            top_flows,
            status_breakdown,
        }
    }

    /// Aggregate traces within a time window (seconds from now).
    /// Returns percentiles, error rate, and per-flow stats.
    /// If window_secs is 0, aggregates all buffered traces.
    pub fn aggregate(&self, window_secs: u64) -> TraceAggregate {
        let now = wall_clock_secs();
        let cutoff = if window_secs > 0 { now.saturating_sub(window_secs) } else { 0 };

        let window_entries: Vec<&TraceEntry> = self.entries
            .iter()
            .filter(|e| e.timestamp >= cutoff)
            .collect();

        let count = window_entries.len();
        if count == 0 {
            return TraceAggregate {
                window_secs,
                count: 0,
                error_rate: 0.0,
                avg_latency_ms: 0,
                p50_latency_ms: 0,
                p95_latency_ms: 0,
                p99_latency_ms: 0,
                min_latency_ms: 0,
                max_latency_ms: 0,
                total_tokens: 0,
                avg_steps: 0.0,
                flows: Vec::new(),
            };
        }

        let mut latencies: Vec<u64> = window_entries.iter().map(|e| e.latency_ms).collect();
        latencies.sort();

        let error_count = window_entries.iter().filter(|e| e.errors > 0).count();
        let total_latency: u64 = latencies.iter().sum();
        let total_tokens: u64 = window_entries.iter().map(|e| e.tokens_input + e.tokens_output).sum();
        let total_steps: f64 = window_entries.iter().map(|e| e.steps_executed as f64).sum();

        // Per-flow aggregation
        let mut flow_map: std::collections::HashMap<String, (u64, u64, usize)> = std::collections::HashMap::new();
        for e in &window_entries {
            let entry = flow_map.entry(e.flow_name.clone()).or_insert((0, 0, 0));
            entry.0 += 1; // count
            entry.1 += e.latency_ms; // total latency
            if e.errors > 0 { entry.2 += 1; } // errors
        }
        let mut flows: Vec<FlowAggregate> = flow_map.into_iter().map(|(name, (cnt, lat, errs))| {
            FlowAggregate {
                flow_name: name,
                count: cnt,
                avg_latency_ms: if cnt > 0 { lat / cnt } else { 0 },
                errors: errs as u64,
            }
        }).collect();
        flows.sort_by(|a, b| b.count.cmp(&a.count));

        TraceAggregate {
            window_secs,
            count: count as u64,
            error_rate: error_count as f64 / count as f64,
            avg_latency_ms: total_latency / count as u64,
            p50_latency_ms: percentile(&latencies, 50),
            p95_latency_ms: percentile(&latencies, 95),
            p99_latency_ms: percentile(&latencies, 99),
            min_latency_ms: latencies[0],
            max_latency_ms: latencies[latencies.len() - 1],
            total_tokens,
            avg_steps: total_steps / count as f64,
            flows,
        }
    }

    /// Set correlation ID on a trace. Returns true if found.
    pub fn set_correlation(&mut self, id: u64, correlation_id: &str) -> bool {
        match self.get_mut(id) {
            Some(entry) => {
                entry.correlation_id = Some(correlation_id.to_string());
                true
            }
            None => false,
        }
    }

    /// Find all traces with a given correlation ID.
    pub fn by_correlation(&self, correlation_id: &str) -> Vec<&TraceEntry> {
        self.entries.iter()
            .filter(|e| e.correlation_id.as_deref() == Some(correlation_id))
            .collect()
    }

    /// Clear all buffered traces.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Percentile from a sorted slice (nearest-rank method).
fn percentile(sorted: &[u64], pct: usize) -> u64 {
    if sorted.is_empty() { return 0; }
    let idx = (pct * sorted.len() + 99) / 100;
    sorted[idx.min(sorted.len()) - 1]
}

/// Aggregate result for a time window.
#[derive(Debug, Clone, Serialize)]
pub struct TraceAggregate {
    pub window_secs: u64,
    pub count: u64,
    pub error_rate: f64,
    pub avg_latency_ms: u64,
    pub p50_latency_ms: u64,
    pub p95_latency_ms: u64,
    pub p99_latency_ms: u64,
    pub min_latency_ms: u64,
    pub max_latency_ms: u64,
    pub total_tokens: u64,
    pub avg_steps: f64,
    pub flows: Vec<FlowAggregate>,
}

/// Per-flow aggregate within a window.
#[derive(Debug, Clone, Serialize)]
pub struct FlowAggregate {
    pub flow_name: String,
    pub count: u64,
    pub avg_latency_ms: u64,
    pub errors: u64,
}

// ── Filter ──────────────────────────────────────────────────────────────

/// Filter for querying traces.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TraceFilter {
    /// Filter by flow name.
    pub flow_name: Option<String>,
    /// Filter by status.
    pub status: Option<String>,
    /// Filter by client key.
    pub client_key: Option<String>,
    /// Only traces with latency >= this (ms).
    pub min_latency_ms: Option<u64>,
    /// Only traces with errors > 0.
    pub has_errors: Option<bool>,
    /// Only traces with this annotation tag.
    pub tag: Option<String>,
}

impl TraceFilter {
    pub fn matches(&self, entry: &TraceEntry) -> bool {
        if let Some(ref name) = self.flow_name {
            if entry.flow_name != *name {
                return false;
            }
        }
        if let Some(ref status) = self.status {
            if entry.status.as_str() != status.as_str() {
                return false;
            }
        }
        if let Some(ref key) = self.client_key {
            if entry.client_key != *key {
                return false;
            }
        }
        if let Some(min_lat) = self.min_latency_ms {
            if entry.latency_ms < min_lat {
                return false;
            }
        }
        if let Some(has_err) = self.has_errors {
            if has_err && entry.errors == 0 {
                return false;
            }
            if !has_err && entry.errors > 0 {
                return false;
            }
        }
        if let Some(ref tag) = self.tag {
            let has_tag = entry.annotations.iter().any(|a| a.tags.contains(tag));
            if !has_tag {
                return false;
            }
        }
        true
    }
}

// ── Stats ───────────────────────────────────────────────────────────────

/// Aggregate statistics across buffered traces.
#[derive(Debug, Clone, Serialize)]
pub struct TraceStoreStats {
    pub total_recorded: u64,
    pub buffered: usize,
    pub avg_latency_ms: u64,
    pub max_latency_ms: u64,
    pub total_tokens_input: u64,
    pub total_tokens_output: u64,
    pub total_steps: usize,
    pub total_anchor_checks: usize,
    pub total_anchor_breaches: usize,
    pub total_errors: usize,
    pub total_retries: usize,
    pub top_flows: Vec<(String, u64)>,
    pub status_breakdown: Vec<(String, u64)>,
}

// ── Export formats ─────────────────────────────────────────────────────

/// Supported export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// JSON Lines — one JSON object per trace, OpenTelemetry-like span structure.
    JsonLines,
    /// CSV — tabular rows with header.
    Csv,
    /// Prometheus exposition — aggregate metrics from buffered traces.
    Prometheus,
}

impl ExportFormat {
    /// Parse format string (case-insensitive). Default: JsonLines.
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "csv" => ExportFormat::Csv,
            "prometheus" | "prom" => ExportFormat::Prometheus,
            _ => ExportFormat::JsonLines,
        }
    }

    /// MIME content type for the format.
    pub fn content_type(&self) -> &'static str {
        match self {
            ExportFormat::JsonLines => "application/x-ndjson",
            ExportFormat::Csv => "text/csv",
            ExportFormat::Prometheus => "text/plain; version=0.0.4; charset=utf-8",
        }
    }
}

/// An OpenTelemetry-like span representation for trace export.
#[derive(Debug, Clone, Serialize)]
pub struct TraceSpan {
    /// Trace ID (matches TraceEntry.id).
    pub trace_id: String,
    /// Span name (flow name).
    pub name: String,
    /// Start time (Unix seconds).
    pub start_time_unix_secs: u64,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Status code: "ok", "error", "partial", "timeout".
    pub status: String,
    /// Resource attributes.
    pub resource: TraceSpanResource,
    /// Span attributes (key-value metrics).
    pub attributes: TraceSpanAttributes,
    /// Events (sub-spans).
    pub events: Vec<TraceSpanEvent>,
}

/// Resource metadata for a trace span.
#[derive(Debug, Clone, Serialize)]
pub struct TraceSpanResource {
    pub service_name: String,
    pub service_version: String,
    pub source_file: String,
    pub backend: String,
    pub client_key: String,
}

/// Numeric attributes for a trace span.
#[derive(Debug, Clone, Serialize)]
pub struct TraceSpanAttributes {
    pub steps_executed: usize,
    pub tokens_input: u64,
    pub tokens_output: u64,
    pub tokens_total: u64,
    pub anchor_checks: usize,
    pub anchor_breaches: usize,
    pub errors: usize,
    pub retries: usize,
}

/// An event within a trace span (maps from TraceEvent).
#[derive(Debug, Clone, Serialize)]
pub struct TraceSpanEvent {
    pub name: String,
    pub offset_ms: u64,
    pub attributes: std::collections::HashMap<String, String>,
}

/// Convert a TraceEntry to an OpenTelemetry-like span.
pub fn entry_to_span(entry: &TraceEntry) -> TraceSpan {
    let events = entry
        .events
        .iter()
        .map(|e| {
            let mut attrs = std::collections::HashMap::new();
            if !e.step_name.is_empty() {
                attrs.insert("step".to_string(), e.step_name.clone());
            }
            if !e.detail.is_empty() {
                attrs.insert("detail".to_string(), e.detail.clone());
            }
            TraceSpanEvent {
                name: e.event_type.clone(),
                offset_ms: e.offset_ms,
                attributes: attrs,
            }
        })
        .collect();

    TraceSpan {
        trace_id: format!("axt-{}", entry.id),
        name: entry.flow_name.clone(),
        start_time_unix_secs: entry.timestamp,
        duration_ms: entry.latency_ms,
        status: entry.status.as_str().to_string(),
        resource: TraceSpanResource {
            service_name: "axon-server".to_string(),
            service_version: crate::version::AXON_VERSION.to_string(),
            source_file: entry.source_file.clone(),
            backend: entry.backend.clone(),
            client_key: entry.client_key.clone(),
        },
        attributes: TraceSpanAttributes {
            steps_executed: entry.steps_executed,
            tokens_input: entry.tokens_input,
            tokens_output: entry.tokens_output,
            tokens_total: entry.tokens_input + entry.tokens_output,
            anchor_checks: entry.anchor_checks,
            anchor_breaches: entry.anchor_breaches,
            errors: entry.errors,
            retries: entry.retries,
        },
        events,
    }
}

/// Export traces as JSON Lines (one JSON object per line).
pub fn export_jsonl(entries: &[&TraceEntry]) -> String {
    let mut out = String::new();
    for entry in entries {
        let span = entry_to_span(entry);
        if let Ok(line) = serde_json::to_string(&span) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

/// Export traces as CSV with header row.
pub fn export_csv(entries: &[&TraceEntry]) -> String {
    let mut out = String::new();
    out.push_str("trace_id,timestamp,flow_name,status,steps_executed,latency_ms,tokens_input,tokens_output,anchor_checks,anchor_breaches,errors,retries,source_file,backend,client_key,event_count\n");
    for entry in entries {
        out.push_str(&format!(
            "axt-{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            entry.id,
            entry.timestamp,
            entry.flow_name,
            entry.status.as_str(),
            entry.steps_executed,
            entry.latency_ms,
            entry.tokens_input,
            entry.tokens_output,
            entry.anchor_checks,
            entry.anchor_breaches,
            entry.errors,
            entry.retries,
            entry.source_file,
            entry.backend,
            entry.client_key,
            entry.events.len(),
        ));
    }
    out
}

/// Export aggregate metrics from traces as Prometheus exposition format.
pub fn export_prometheus(entries: &[&TraceEntry]) -> String {
    let count = entries.len() as u64;
    let mut total_latency: u64 = 0;
    let mut max_latency: u64 = 0;
    let mut total_tokens_in: u64 = 0;
    let mut total_tokens_out: u64 = 0;
    let mut total_steps: u64 = 0;
    let mut total_errors: u64 = 0;
    let mut total_retries: u64 = 0;
    let mut total_anchor_checks: u64 = 0;
    let mut total_anchor_breaches: u64 = 0;
    let mut status_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    for e in entries {
        total_latency += e.latency_ms;
        if e.latency_ms > max_latency {
            max_latency = e.latency_ms;
        }
        total_tokens_in += e.tokens_input;
        total_tokens_out += e.tokens_output;
        total_steps += e.steps_executed as u64;
        total_errors += e.errors as u64;
        total_retries += e.retries as u64;
        total_anchor_checks += e.anchor_checks as u64;
        total_anchor_breaches += e.anchor_breaches as u64;
        *status_counts.entry(e.status.as_str().to_string()).or_insert(0) += 1;
    }

    let avg_latency = if count > 0 { total_latency / count } else { 0 };

    let mut out = String::new();

    out.push_str("# HELP axon_trace_export_count Number of traces in this export.\n");
    out.push_str("# TYPE axon_trace_export_count gauge\n");
    out.push_str(&format!("axon_trace_export_count {}\n\n", count));

    out.push_str("# HELP axon_trace_export_latency_avg_ms Average latency across exported traces.\n");
    out.push_str("# TYPE axon_trace_export_latency_avg_ms gauge\n");
    out.push_str(&format!("axon_trace_export_latency_avg_ms {}\n\n", avg_latency));

    out.push_str("# HELP axon_trace_export_latency_max_ms Maximum latency across exported traces.\n");
    out.push_str("# TYPE axon_trace_export_latency_max_ms gauge\n");
    out.push_str(&format!("axon_trace_export_latency_max_ms {}\n\n", max_latency));

    out.push_str("# HELP axon_trace_export_tokens_total Total tokens in exported traces.\n");
    out.push_str("# TYPE axon_trace_export_tokens_total counter\n");
    out.push_str(&format!("axon_trace_export_tokens_total{{type=\"input\"}} {}\n", total_tokens_in));
    out.push_str(&format!("axon_trace_export_tokens_total{{type=\"output\"}} {}\n\n", total_tokens_out));

    out.push_str("# HELP axon_trace_export_steps_total Total steps executed in exported traces.\n");
    out.push_str("# TYPE axon_trace_export_steps_total counter\n");
    out.push_str(&format!("axon_trace_export_steps_total {}\n\n", total_steps));

    out.push_str("# HELP axon_trace_export_errors_total Total errors in exported traces.\n");
    out.push_str("# TYPE axon_trace_export_errors_total counter\n");
    out.push_str(&format!("axon_trace_export_errors_total {}\n\n", total_errors));

    out.push_str("# HELP axon_trace_export_retries_total Total retries in exported traces.\n");
    out.push_str("# TYPE axon_trace_export_retries_total counter\n");
    out.push_str(&format!("axon_trace_export_retries_total {}\n\n", total_retries));

    out.push_str("# HELP axon_trace_export_anchor_checks_total Total anchor checks in exported traces.\n");
    out.push_str("# TYPE axon_trace_export_anchor_checks_total counter\n");
    out.push_str(&format!("axon_trace_export_anchor_checks_total {}\n\n", total_anchor_checks));

    out.push_str("# HELP axon_trace_export_anchor_breaches_total Total anchor breaches in exported traces.\n");
    out.push_str("# TYPE axon_trace_export_anchor_breaches_total counter\n");
    out.push_str(&format!("axon_trace_export_anchor_breaches_total {}\n\n", total_anchor_breaches));

    if !status_counts.is_empty() {
        out.push_str("# HELP axon_trace_export_by_status Count of exported traces by status.\n");
        out.push_str("# TYPE axon_trace_export_by_status gauge\n");
        let mut sorted: Vec<_> = status_counts.into_iter().collect();
        sorted.sort_by_key(|(k, _)| k.clone());
        for (status, n) in sorted {
            out.push_str(&format!("axon_trace_export_by_status{{status=\"{}\"}} {}\n", status, n));
        }
        out.push('\n');
    }

    out
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn wall_clock_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Build a trace entry (convenience constructor for server use).
pub fn build_trace(
    flow_name: &str,
    source_file: &str,
    backend: &str,
    client_key: &str,
    status: TraceStatus,
    steps: usize,
    latency_ms: u64,
) -> TraceEntry {
    TraceEntry {
        id: 0, // set by store
        timestamp: 0, // set by store
        flow_name: flow_name.to_string(),
        status,
        steps_executed: steps,
        latency_ms,
        tokens_input: 0,
        tokens_output: 0,
        anchor_checks: 0,
        anchor_breaches: 0,
        errors: 0,
        retries: 0,
        source_file: source_file.to_string(),
        backend: backend.to_string(),
        client_key: client_key.to_string(),
        events: Vec::new(),
        replay_of: None,
        annotations: Vec::new(),
        correlation_id: None,
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_trace(name: &str, status: TraceStatus) -> TraceEntry {
        let mut t = build_trace(name, "test.axon", "anthropic", "token_a", status, 3, 150);
        t.tokens_input = 100;
        t.tokens_output = 50;
        t.anchor_checks = 2;
        t.events.push(TraceEvent {
            event_type: "step_start".into(),
            offset_ms: 0,
            step_name: "step1".into(),
            detail: "starting".into(),
        });
        t
    }

    #[test]
    fn record_and_retrieve() {
        let mut store = TraceStore::new(TraceStoreConfig::default());
        let id = store.record(sample_trace("FlowA", TraceStatus::Success));
        assert_eq!(id, 1);
        assert_eq!(store.len(), 1);

        let entry = store.get(id).unwrap();
        assert_eq!(entry.flow_name, "FlowA");
        assert_eq!(entry.status, TraceStatus::Success);
        assert_eq!(entry.steps_executed, 3);
        assert!(entry.timestamp > 0);
    }

    #[test]
    fn ring_buffer_eviction() {
        let config = TraceStoreConfig { capacity: 3, enabled: true, max_events_per_trace: 100, max_age_secs: 0 };
        let mut store = TraceStore::new(config);

        for i in 0..5 {
            store.record(sample_trace(&format!("Flow{}", i), TraceStatus::Success));
        }

        assert_eq!(store.len(), 3);
        assert_eq!(store.total_recorded(), 5);

        let recent = store.recent(10, None);
        assert_eq!(recent[0].flow_name, "Flow4");
        assert_eq!(recent[2].flow_name, "Flow2");
    }

    #[test]
    fn disabled_store() {
        let mut store = TraceStore::new(TraceStoreConfig::disabled());
        let id = store.record(sample_trace("X", TraceStatus::Success));
        assert_eq!(id, 0);
        assert_eq!(store.len(), 0);
        assert_eq!(store.total_recorded(), 0);
    }

    // v1.24.0 — reserve_id / record_with_id

    #[test]
    fn reserve_id_monotonic_and_consumes_next_id() {
        let mut store = TraceStore::new(TraceStoreConfig::default());
        let id1 = store.reserve_id();
        let id2 = store.reserve_id();
        let id3 = store.reserve_id();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
        // No entries persisted by reserve_id — it only allocates the
        // sequence number.
        assert_eq!(store.len(), 0);
        assert_eq!(store.total_recorded(), 0);
    }

    #[test]
    fn reserve_id_disabled_store_returns_zero() {
        let mut store = TraceStore::new(TraceStoreConfig::disabled());
        let id = store.reserve_id();
        assert_eq!(id, 0);
    }

    #[test]
    fn record_with_id_persists_under_reserved_id() {
        let mut store = TraceStore::new(TraceStoreConfig::default());
        let id = store.reserve_id();
        store.record_with_id(sample_trace("ReservedFlow", TraceStatus::Success), id);
        let entry = store.get(id).expect("entry must exist under reserved id");
        assert_eq!(entry.id, id);
        assert_eq!(entry.flow_name, "ReservedFlow");
        assert_eq!(store.len(), 1);
        assert_eq!(store.total_recorded(), 1);
    }

    #[test]
    fn record_with_id_does_not_advance_next_id() {
        let mut store = TraceStore::new(TraceStoreConfig::default());
        let reserved = store.reserve_id();
        store.record_with_id(sample_trace("X", TraceStatus::Success), reserved);
        // record() after record_with_id() should produce a NEW id
        // (the next sequence number), not reuse the reserved id.
        let next = store.record(sample_trace("Y", TraceStatus::Success));
        assert!(
            next > reserved,
            "record after record_with_id must produce a strictly greater id"
        );
    }

    #[test]
    fn record_with_id_disabled_store_is_noop() {
        let mut store = TraceStore::new(TraceStoreConfig::disabled());
        let id = store.reserve_id();
        store.record_with_id(sample_trace("X", TraceStatus::Success), id);
        assert_eq!(store.len(), 0);
        assert_eq!(store.total_recorded(), 0);
    }

    #[test]
    fn reserve_then_record_preserves_audit_correlation() {
        // Live-streaming pattern from execute_sse_handler: reserve up
        // front so wire events carry the trace_id, then record the
        // final entry once execution completes.
        let mut store = TraceStore::new(TraceStoreConfig::default());
        let wire_trace_id = store.reserve_id();

        // …flow executes; wire emits axon.token + axon.complete with
        // trace_id = wire_trace_id…

        let mut entry = sample_trace("LiveStreaming", TraceStatus::Success);
        entry.steps_executed = 5;
        entry.tokens_output = 42;
        store.record_with_id(entry, wire_trace_id);

        let recovered = store.get(wire_trace_id).expect("audit lookup must succeed");
        assert_eq!(recovered.id, wire_trace_id);
        assert_eq!(recovered.steps_executed, 5);
        assert_eq!(recovered.tokens_output, 42);
    }

    #[test]
    fn filter_by_flow_name() {
        let mut store = TraceStore::new(TraceStoreConfig::default());
        store.record(sample_trace("Alpha", TraceStatus::Success));
        store.record(sample_trace("Beta", TraceStatus::Success));
        store.record(sample_trace("Alpha", TraceStatus::Failed));

        let filter = TraceFilter { flow_name: Some("Alpha".into()), ..Default::default() };
        let result = store.recent(10, Some(&filter));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn filter_by_status() {
        let mut store = TraceStore::new(TraceStoreConfig::default());
        store.record(sample_trace("A", TraceStatus::Success));
        store.record(sample_trace("B", TraceStatus::Failed));
        store.record(sample_trace("C", TraceStatus::Success));

        let filter = TraceFilter { status: Some("failed".into()), ..Default::default() };
        let result = store.recent(10, Some(&filter));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].flow_name, "B");
    }

    #[test]
    fn filter_by_has_errors() {
        let mut store = TraceStore::new(TraceStoreConfig::default());

        let mut t1 = sample_trace("A", TraceStatus::Success);
        t1.errors = 0;
        store.record(t1);

        let mut t2 = sample_trace("B", TraceStatus::Failed);
        t2.errors = 2;
        store.record(t2);

        let filter = TraceFilter { has_errors: Some(true), ..Default::default() };
        let result = store.recent(10, Some(&filter));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].flow_name, "B");
    }

    #[test]
    fn stats_computation() {
        let mut store = TraceStore::new(TraceStoreConfig::default());

        let mut t1 = sample_trace("Alpha", TraceStatus::Success);
        t1.latency_ms = 100;
        t1.tokens_input = 200;
        t1.tokens_output = 100;
        store.record(t1);

        let mut t2 = sample_trace("Alpha", TraceStatus::Success);
        t2.latency_ms = 300;
        t2.tokens_input = 400;
        t2.tokens_output = 200;
        t2.errors = 1;
        store.record(t2);

        let mut t3 = sample_trace("Beta", TraceStatus::Failed);
        t3.latency_ms = 500;
        t3.errors = 2;
        store.record(t3);

        let stats = store.stats();
        assert_eq!(stats.total_recorded, 3);
        assert_eq!(stats.buffered, 3);
        assert_eq!(stats.avg_latency_ms, 300); // (100+300+500)/3
        assert_eq!(stats.max_latency_ms, 500);
        assert_eq!(stats.total_tokens_input, 700); // 200+400+100
        assert_eq!(stats.total_errors, 3);
        assert_eq!(stats.top_flows[0].0, "Alpha");
        assert_eq!(stats.top_flows[0].1, 2);
    }

    #[test]
    fn stats_empty_store() {
        let store = TraceStore::new(TraceStoreConfig::default());
        let stats = store.stats();
        assert_eq!(stats.total_recorded, 0);
        assert_eq!(stats.avg_latency_ms, 0);
        assert_eq!(stats.max_latency_ms, 0);
    }

    #[test]
    fn trace_status_serde() {
        assert_eq!(TraceStatus::Success.as_str(), "success");
        assert_eq!(TraceStatus::Failed.as_str(), "failed");
        assert_eq!(TraceStatus::Partial.as_str(), "partial");
        assert_eq!(TraceStatus::Timeout.as_str(), "timeout");

        let json = serde_json::to_value(TraceStatus::Success).unwrap();
        assert_eq!(json, "success");
    }

    #[test]
    fn trace_entry_serializable() {
        let t = sample_trace("TestFlow", TraceStatus::Success);
        let json = serde_json::to_value(&t).unwrap();
        assert_eq!(json["flow_name"], "TestFlow");
        assert_eq!(json["status"], "success");
        assert_eq!(json["steps_executed"], 3);
        assert!(json["events"].is_array());
    }

    #[test]
    fn stats_serializable() {
        let store = TraceStore::new(TraceStoreConfig::default());
        let stats = store.stats();
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["total_recorded"], 0);
        assert_eq!(json["buffered"], 0);
        assert!(json["top_flows"].is_array());
    }

    #[test]
    fn config_serializable() {
        let cfg = TraceStoreConfig::default();
        let json = serde_json::to_value(&cfg).unwrap();
        assert_eq!(json["capacity"], 500);
        assert_eq!(json["enabled"], true);
        assert_eq!(json["max_events_per_trace"], 200);
    }

    #[test]
    fn event_truncation() {
        let config = TraceStoreConfig { capacity: 10, enabled: true, max_events_per_trace: 3, max_age_secs: 0 };
        let mut store = TraceStore::new(config);

        let mut t = sample_trace("X", TraceStatus::Success);
        for i in 0..10 {
            t.events.push(TraceEvent {
                event_type: "test".into(),
                offset_ms: i,
                step_name: "s".into(),
                detail: "d".into(),
            });
        }
        let id = store.record(t);
        let entry = store.get(id).unwrap();
        assert_eq!(entry.events.len(), 3);
    }

    #[test]
    fn clear_preserves_total() {
        let mut store = TraceStore::new(TraceStoreConfig::default());
        store.record(sample_trace("A", TraceStatus::Success));
        store.record(sample_trace("B", TraceStatus::Success));
        assert_eq!(store.len(), 2);

        store.clear();
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());
        assert_eq!(store.total_recorded(), 2);
    }

    // ── Export tests ────────────────────────────────────────────────────

    #[test]
    fn export_format_parsing() {
        assert_eq!(ExportFormat::from_str("jsonl"), ExportFormat::JsonLines);
        assert_eq!(ExportFormat::from_str("JSONL"), ExportFormat::JsonLines);
        assert_eq!(ExportFormat::from_str("json"), ExportFormat::JsonLines);
        assert_eq!(ExportFormat::from_str("csv"), ExportFormat::Csv);
        assert_eq!(ExportFormat::from_str("CSV"), ExportFormat::Csv);
        assert_eq!(ExportFormat::from_str("prometheus"), ExportFormat::Prometheus);
        assert_eq!(ExportFormat::from_str("prom"), ExportFormat::Prometheus);
        assert_eq!(ExportFormat::from_str("unknown"), ExportFormat::JsonLines);
    }

    #[test]
    fn export_format_content_type() {
        assert_eq!(ExportFormat::JsonLines.content_type(), "application/x-ndjson");
        assert_eq!(ExportFormat::Csv.content_type(), "text/csv");
        assert!(ExportFormat::Prometheus.content_type().starts_with("text/plain"));
    }

    #[test]
    fn entry_to_span_conversion() {
        let mut store = TraceStore::new(TraceStoreConfig::default());
        let id = store.record(sample_trace("FlowX", TraceStatus::Success));
        let entry = store.get(id).unwrap();

        let span = entry_to_span(entry);
        assert_eq!(span.trace_id, format!("axt-{}", id));
        assert_eq!(span.name, "FlowX");
        assert_eq!(span.status, "success");
        assert_eq!(span.duration_ms, 150);
        assert_eq!(span.resource.service_name, "axon-server");
        assert_eq!(span.resource.backend, "anthropic");
        assert_eq!(span.resource.client_key, "token_a");
        assert_eq!(span.attributes.steps_executed, 3);
        assert_eq!(span.attributes.tokens_input, 100);
        assert_eq!(span.attributes.tokens_output, 50);
        assert_eq!(span.attributes.tokens_total, 150);
        assert_eq!(span.attributes.anchor_checks, 2);
        assert_eq!(span.events.len(), 1);
        assert_eq!(span.events[0].name, "step_start");
    }

    #[test]
    fn export_jsonl_format() {
        let mut store = TraceStore::new(TraceStoreConfig::default());
        store.record(sample_trace("A", TraceStatus::Success));
        store.record(sample_trace("B", TraceStatus::Failed));
        let entries = store.recent(10, None);

        let jsonl = export_jsonl(&entries);
        let lines: Vec<&str> = jsonl.lines().collect();
        assert_eq!(lines.len(), 2);

        // Each line is valid JSON
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["name"], "B"); // newest first
        assert_eq!(first["status"], "failed");
        assert!(first["trace_id"].as_str().unwrap().starts_with("axt-"));
        assert!(first["resource"]["service_name"].as_str().unwrap() == "axon-server");

        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["name"], "A");
        assert_eq!(second["status"], "success");
    }

    #[test]
    fn export_csv_format() {
        let mut store = TraceStore::new(TraceStoreConfig::default());
        store.record(sample_trace("FlowA", TraceStatus::Success));
        store.record(sample_trace("FlowB", TraceStatus::Failed));
        let entries = store.recent(10, None);

        let csv = export_csv(&entries);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 3); // header + 2 rows

        // Header
        assert!(lines[0].starts_with("trace_id,"));
        assert!(lines[0].contains("flow_name"));
        assert!(lines[0].contains("latency_ms"));
        assert!(lines[0].contains("event_count"));

        // Data rows
        assert!(lines[1].contains("FlowB")); // newest first
        assert!(lines[1].contains("failed"));
        assert!(lines[2].contains("FlowA"));
        assert!(lines[2].contains("success"));
    }

    #[test]
    fn export_prometheus_format() {
        let mut store = TraceStore::new(TraceStoreConfig::default());
        let mut t1 = sample_trace("A", TraceStatus::Success);
        t1.latency_ms = 200;
        t1.errors = 0;
        store.record(t1);
        let mut t2 = sample_trace("B", TraceStatus::Failed);
        t2.latency_ms = 400;
        t2.errors = 2;
        store.record(t2);
        let entries = store.recent(10, None);

        let prom = export_prometheus(&entries);
        assert!(prom.contains("axon_trace_export_count 2"));
        assert!(prom.contains("axon_trace_export_latency_avg_ms 300")); // (200+400)/2
        assert!(prom.contains("axon_trace_export_latency_max_ms 400"));
        assert!(prom.contains("axon_trace_export_errors_total 2"));
        assert!(prom.contains("axon_trace_export_by_status{status=\"success\"} 1"));
        assert!(prom.contains("axon_trace_export_by_status{status=\"failed\"} 1"));
        assert!(prom.contains("# HELP axon_trace_export_count"));
        assert!(prom.contains("# TYPE axon_trace_export_count gauge"));
    }

    #[test]
    fn export_empty_traces() {
        let entries: Vec<&TraceEntry> = vec![];
        let jsonl = export_jsonl(&entries);
        assert!(jsonl.is_empty());

        let csv = export_csv(&entries);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 1); // header only

        let prom = export_prometheus(&entries);
        assert!(prom.contains("axon_trace_export_count 0"));
        assert!(prom.contains("axon_trace_export_latency_avg_ms 0"));
    }

    #[test]
    fn span_serializable() {
        let mut store = TraceStore::new(TraceStoreConfig::default());
        store.record(sample_trace("Test", TraceStatus::Success));
        let entry = store.get(1).unwrap();
        let span = entry_to_span(entry);
        let json = serde_json::to_value(&span).unwrap();
        assert!(json["trace_id"].is_string());
        assert!(json["resource"].is_object());
        assert!(json["attributes"].is_object());
        assert!(json["events"].is_array());
    }
}
