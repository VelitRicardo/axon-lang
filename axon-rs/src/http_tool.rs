//! HTTP tool provider — executes tool calls as REST requests via reqwest.
//!
//! Tools declared with `provider: http` in .axon files dispatch their
//! argument as the request body to the URL specified in `runtime`.
//!
//! Request format:
//!   POST {runtime_url}
//!   Content-Type: application/json
//!   X-Axon-Tool: {tool_name}
//!
//!   Body: the tool argument (string, sent as JSON-wrapped if not already JSON)
//!
//! Response handling:
//!   - 2xx: response body becomes tool output (success)
//!   - 4xx/5xx: error message with status code (failure)
//!   - Connection error: descriptive error (failure)
//!
//! Timeout: parsed from ToolEntry.timeout field (e.g., "10s", "500ms").
//! Default timeout: 30 seconds.
//!
//! §Fase 34.e (v1.29.0) — Streaming surface via [`HttpStreamingTool`].
//! The async-trait Tool impl drives the upstream HTTP request via
//! `reqwest::Client` (async) + drains the response body chunk-by-chunk.
//! Content-Type drives framing:
//!   - `text/event-stream` → per-W3C-SSE-event ToolChunks
//!   - `application/x-ndjson` / `application/jsonl` → per-line ToolChunks
//!   - Other (raw bytes, JSON, etc.) → single-chunk wrap (D9 backwards-
//!     compat for non-streaming HTTP endpoints)
//! Per-chunk cancel poll honors the D5 ≤100ms budget.

use std::time::Duration;

use crate::tool_executor::ToolResult;
use crate::tool_registry::ToolEntry;

// ── Timeout parsing ───────────────────────────────────────────────────────

/// Parse a timeout string like "10s", "500ms", "2m" into Duration.
/// Returns None for empty or unparseable values.
fn parse_timeout(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    if let Some(secs) = s.strip_suffix("ms") {
        secs.trim().parse::<u64>().ok().map(Duration::from_millis)
    } else if let Some(secs) = s.strip_suffix('s') {
        secs.trim().parse::<u64>().ok().map(Duration::from_secs)
    } else if let Some(mins) = s.strip_suffix('m') {
        mins.trim()
            .parse::<u64>()
            .ok()
            .map(|m| Duration::from_secs(m * 60))
    } else {
        // Try as raw seconds
        s.parse::<u64>().ok().map(Duration::from_secs)
    }
}

/// Public accessor for timeout parsing (used by emcp module).
pub fn parse_timeout_pub(s: &str) -> Option<Duration> {
    parse_timeout(s)
}

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

// ── HTTP dispatch ─────────────────────────────────────────────────────────

/// Execute an HTTP tool call.
///
/// - `entry`: the tool's registry entry (must have provider == "http")
/// - `argument`: the argument string from the use_tool step
///
/// Returns a ToolResult with the HTTP response body on success,
/// or an error description on failure.
pub fn dispatch_http(entry: &ToolEntry, argument: &str) -> ToolResult {
    let url = entry.runtime.trim();

    if url.is_empty() {
        return ToolResult {
            success: false,
            output: format!(
                "HTTP tool '{}': no endpoint URL. Set runtime: \"https://...\" in tool definition.",
                entry.name
            ),
            tool_name: entry.name.clone(),
        };
    }

    // Validate URL scheme
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return ToolResult {
            success: false,
            output: format!(
                "HTTP tool '{}': invalid URL '{}'. Must start with http:// or https://.",
                entry.name, url
            ),
            tool_name: entry.name.clone(),
        };
    }

    let timeout = parse_timeout(&entry.timeout).unwrap_or(DEFAULT_TIMEOUT);

    // Build the request body — wrap as JSON string if not already JSON
    let body = if argument.trim_start().starts_with('{') || argument.trim_start().starts_with('[') {
        argument.to_string()
    } else {
        serde_json::json!({ "input": argument }).to_string()
    };

    // Execute the HTTP request
    match execute_request(url, &entry.name, &body, timeout) {
        Ok(response) => response,
        Err(e) => ToolResult {
            success: false,
            output: format!("HTTP tool '{}': {}", entry.name, e),
            tool_name: entry.name.clone(),
        },
    }
}

/// §Fase 114 (owed) — the PROCESS-SHARED blocking HTTP client. A fresh
/// `reqwest::blocking::Client` per call threw away reqwest's internal connection
/// pool, so every tool call paid a new TCP + TLS handshake. reqwest is explicitly
/// designed to be built ONCE and reused (the pool lives on the `Client`); the
/// per-call knob — the request timeout — is set per REQUEST instead, so one shared
/// client serves every timeout. `http_tool` carries no per-resource client config
/// (proxy / TLS impersonation is the `scrape` domain), so process-wide is the
/// correct granularity here. Built lazily; `new()` only fails on a broken TLS
/// backend, which would fail every call anyway.
fn shared_blocking_client() -> &'static reqwest::blocking::Client {
    static CLIENT: std::sync::OnceLock<reqwest::blocking::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(reqwest::blocking::Client::new)
}

/// §Fase 114 (owed) — the PROCESS-SHARED async HTTP client (the streaming-tool
/// dual of [`shared_blocking_client`]). Same rationale: build once, reuse the
/// connection pool, set the timeout per request.
fn shared_async_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

/// Perform the actual HTTP POST request.
fn execute_request(
    url: &str,
    tool_name: &str,
    body: &str,
    timeout: Duration,
) -> Result<ToolResult, String> {
    let response = shared_blocking_client()
        .post(url)
        .timeout(timeout)
        .header("Content-Type", "application/json")
        .header("X-Axon-Tool", tool_name)
        .body(body.to_string())
        .send()
        .map_err(|e| {
            if e.is_timeout() {
                format!("request timed out after {}s", timeout.as_secs())
            } else if e.is_connect() {
                format!("connection failed to {url}")
            } else {
                format!("request failed: {e}")
            }
        })?;

    let status = response.status();
    let response_body = response
        .text()
        .map_err(|e| format!("failed to read response body: {e}"))?;

    if status.is_success() {
        Ok(ToolResult {
            success: true,
            output: response_body,
            tool_name: tool_name.to_string(),
        })
    } else {
        Ok(ToolResult {
            success: false,
            output: format!(
                "HTTP {}: {}",
                status.as_u16(),
                if response_body.len() > 200 {
                    format!("{}...", &response_body[..200])
                } else {
                    response_body
                }
            ),
            tool_name: tool_name.to_string(),
        })
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  §Fase 34.e — HttpStreamingTool: async-trait Tool impl with per-chunk
//  streaming wire emission via reqwest::Response::bytes_stream().
// ════════════════════════════════════════════════════════════════════════════

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;

use crate::backends::sse_streaming::{LineBuffer, SseEventParser};
use crate::tool_trait::{Tool, ToolChunk, ToolContext, ToolFinishReason, ToolStream};

/// HTTP tool with first-class streaming surface (Fase 34.e).
///
/// `stream()` drives a `reqwest::Client::post(url)` async request +
/// drains `response.bytes_stream()` chunk-by-chunk. Content-Type
/// header decides framing:
///
/// - **`text/event-stream`** → SSE per W3C spec. Each `data:` field
///   from a complete SSE event emits as a `ToolChunk::intermediate`.
///   `event:` / `id:` / `retry:` fields are dropped (the adopter sees
///   only the data payload — the framing was for HTTP transport).
/// - **`application/x-ndjson`** / **`application/jsonl`** → one
///   `ToolChunk::intermediate` per LF-delimited line. Empty lines
///   are skipped.
/// - **Other** (`application/json`, `text/plain`, raw bytes) →
///   single `ToolChunk::intermediate` with the full body accumulated,
///   then a terminator. This is the D9 backwards-compat path for
///   non-streaming HTTP endpoints — the same response shape
///   [`dispatch_http`] returns synchronously, projected onto the
///   single-chunk streaming surface.
///
/// # Cancel discipline (D5)
///
/// `ctx.cancel` is polled between every `bytes_stream().next().await`
/// boundary. When fired, the stream drops the `reqwest::Response`
/// (closing the connection) + emits a single
/// `ToolFinishReason::Cancelled` terminator chunk. Wall-clock budget
/// is bounded by the network roundtrip latency to the next chunk
/// arrival (typically ≤100ms for SSE streams with regular keepalive).
///
/// # Error discipline
///
/// Every failure surface (URL invalid / client build / connect /
/// timeout / non-2xx status / mid-stream byte error / I/O error)
/// is captured as a `ToolFinishReason::Error { message }` terminator
/// chunk — the consumer never sees a panic or a silently-truncated
/// stream.
pub struct HttpStreamingTool {
    name: String,
    url: String,
    timeout: Duration,
}

impl HttpStreamingTool {
    /// Construct from a registry [`ToolEntry`]. Validates the URL +
    /// extracts the timeout. Returns `Err` with adopter-facing
    /// diagnostic when the URL is missing or has an invalid scheme.
    pub fn from_entry(entry: &ToolEntry) -> Result<Self, String> {
        let url = entry.runtime.trim();
        if url.is_empty() {
            return Err(format!(
                "HTTP tool '{}': no endpoint URL. Set runtime: \"https://...\" in tool definition.",
                entry.name
            ));
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(format!(
                "HTTP tool '{}': invalid URL '{}'. Must start with http:// or https://.",
                entry.name, url
            ));
        }
        let timeout = parse_timeout(&entry.timeout).unwrap_or(DEFAULT_TIMEOUT);
        Ok(Self {
            name: entry.name.clone(),
            url: url.to_string(),
            timeout,
        })
    }

    /// Public new() ctor for tests + adopters who construct directly
    /// without a registry entry.
    pub fn new(name: String, url: String, timeout: Duration) -> Self {
        Self { name, url, timeout }
    }
}

/// Build the request body — wrap as JSON `{ "input": args }` if not
/// already JSON. Same logic as [`dispatch_http`].
fn build_request_body(args: &str) -> String {
    let trimmed = args.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        args.to_string()
    } else {
        serde_json::json!({ "input": args }).to_string()
    }
}

/// Classify an HTTP Content-Type header into the framing mode the
/// streaming tool will use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FramingMode {
    /// W3C Server-Sent Events. Drain via [`LineBuffer`] +
    /// [`SseEventParser`]; emit each event's `data:` field as a
    /// `ToolChunk`.
    Sse,
    /// Newline-delimited JSON. Drain via [`LineBuffer`]; emit each
    /// non-empty line as a `ToolChunk`.
    Ndjson,
    /// Anything else. Accumulate full body + emit as 1 chunk +
    /// terminator. D9-style backwards-compat for non-streaming
    /// HTTP endpoints.
    Single,
}

fn classify_framing(content_type: &str) -> FramingMode {
    let lc = content_type.to_ascii_lowercase();
    if lc.contains("text/event-stream") {
        FramingMode::Sse
    } else if lc.contains("application/x-ndjson") || lc.contains("application/jsonl") {
        FramingMode::Ndjson
    } else {
        FramingMode::Single
    }
}

#[async_trait]
impl Tool for HttpStreamingTool {
    async fn execute(&self, args: String, _ctx: ToolContext) -> ToolResult {
        // Synchronous path — adopters calling execute() directly get
        // the legacy [`dispatch_http`] behavior verbatim. The
        // streaming path drives `stream()` exclusively.
        //
        // [`dispatch_http`] uses `reqwest::blocking::Client` (it
        // existed pre-async-trait). Calling blocking-reqwest from
        // inside a tokio runtime panics; we wrap the call with
        // `spawn_blocking` so the synchronous client runs on tokio's
        // blocking pool. Output is byte-equal to dispatch_http (D9).
        let entry = ToolEntry {
            name: self.name.clone(),
            provider: "http".to_string(),
            timeout: format!("{}s", self.timeout.as_secs()),
            runtime: self.url.clone(),
            resource_ref: String::new(),
            substrate: None,
            capacity: None,
            sandbox: None,
            max_results: None,
            output_schema: String::new(),
            effect_row: Vec::new(),
            // §Fase 58.f.2 — reconstructed entry for the legacy sync
            // delegate; no typed input schema needed on this path.
            parameters: Vec::new(),
            secret: String::new(),
            secret_partition: String::new(),
            source: crate::tool_registry::ToolSource::Program,
            is_streaming: false,
            scrape: None,
        };
        match tokio::task::spawn_blocking(move || dispatch_http(&entry, &args)).await {
            Ok(result) => result,
            Err(e) => ToolResult {
                success: false,
                output: format!("HTTP tool '{}': blocking task join failed: {e}", self.name),
                tool_name: self.name.clone(),
            },
        }
    }

    async fn stream(&self, args: String, ctx: ToolContext) -> ToolStream {
        let url = self.url.clone();
        let name = self.name.clone();
        let timeout = self.timeout;
        let cancel = ctx.cancel.clone();
        let body = build_request_body(&args);

        // mpsc + spawn pattern: the background task drives the HTTP
        // request + drains chunks into the channel; the returned
        // stream wraps the receiver. This gives us real per-chunk
        // streaming (chunks reach the dispatcher AS they arrive from
        // upstream) without requiring async-stream macro.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ToolChunk>();

        tokio::spawn(async move {
            // Helper: send the terminator + drop tx so the consumer's
            // stream ends cleanly. Returning `Err` from a sub-step
            // sends an Error-terminator; reaching the natural end of
            // the body sends a Stop-terminator.
            let send_terminator = |reason: ToolFinishReason| {
                let _ = tx.send(ToolChunk::terminator("", reason));
            };

            // Pre-flight cancel check.
            if cancel.is_cancelled() {
                send_terminator(ToolFinishReason::Cancelled);
                return;
            }

            // 1. The PROCESS-SHARED async client (§Fase 114 owed — see
            //    `shared_blocking_client`: pooling was thrown away by a
            //    per-call `Client::builder().build()`). Timeout is per REQUEST.
            let client = shared_async_client();

            // 2. Issue request.
            let response = match client
                .post(&url)
                .timeout(timeout)
                .header("Content-Type", "application/json")
                .header("X-Axon-Tool", &name)
                .body(body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let message = if e.is_timeout() {
                        format!(
                            "HTTP tool '{name}': request timed out after {}s",
                            timeout.as_secs()
                        )
                    } else if e.is_connect() {
                        format!("HTTP tool '{name}': connection failed to {url}")
                    } else {
                        format!("HTTP tool '{name}': request failed: {e}")
                    };
                    send_terminator(ToolFinishReason::Error { message });
                    return;
                }
            };

            // 3. Non-2xx → error terminator with status code +
            //    truncated body. Mirrors dispatch_http's diagnostic
            //    shape.
            let status = response.status();
            if !status.is_success() {
                let body_text = response.text().await.unwrap_or_default();
                let truncated = if body_text.len() > 200 {
                    format!("{}...", &body_text[..200])
                } else {
                    body_text
                };
                send_terminator(ToolFinishReason::Error {
                    message: format!("HTTP {}: {}", status.as_u16(), truncated),
                });
                return;
            }

            // 4. Read Content-Type header → classify framing.
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let framing = classify_framing(&content_type);

            // 5. Drain the body byte-stream per framing mode.
            let mut byte_stream = response.bytes_stream();
            let drain_result = match framing {
                FramingMode::Sse => {
                    drain_sse(&mut byte_stream, &cancel, &tx).await
                }
                FramingMode::Ndjson => {
                    drain_ndjson(&mut byte_stream, &cancel, &tx).await
                }
                FramingMode::Single => {
                    drain_single(&mut byte_stream, &cancel, &tx).await
                }
            };

            match drain_result {
                DrainOutcome::Completed => send_terminator(ToolFinishReason::Stop),
                DrainOutcome::Cancelled => send_terminator(ToolFinishReason::Cancelled),
                DrainOutcome::Error(message) => {
                    send_terminator(ToolFinishReason::Error { message })
                }
            }
        });

        // Wrap the receiver as a Stream. Each `recv().await` yields
        // a ToolChunk + holds the channel open until the producer
        // task drops `tx`.
        Box::pin(futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|chunk| (chunk, rx))
        }))
    }

    fn is_streaming(&self) -> bool {
        true
    }
}

/// Per-framing-mode drain outcome. Drives the terminator decision
/// in the spawned task without leaking implementation details.
enum DrainOutcome {
    Completed,
    Cancelled,
    Error(String),
}

/// Drain SSE framing. Reuses the battle-tested
/// [`crate::backends::sse_streaming::LineBuffer`] +
/// [`crate::backends::sse_streaming::SseEventParser`] from Fase 33.d
/// so every adopter-emitted SSE shape (CRLF normalization, CR strip,
/// multi-line data field, comment lines) is honored verbatim.
async fn drain_sse<S>(
    byte_stream: &mut S,
    cancel: &crate::cancel_token::CancellationFlag,
    tx: &tokio::sync::mpsc::UnboundedSender<ToolChunk>,
) -> DrainOutcome
where
    S: futures::Stream<Item = reqwest::Result<Bytes>> + Unpin + Send,
{
    let mut line_buf = LineBuffer::new();
    let mut sse_parser = SseEventParser::new();
    loop {
        if cancel.is_cancelled() {
            return DrainOutcome::Cancelled;
        }
        match byte_stream.next().await {
            None => break,
            Some(Err(e)) => {
                return DrainOutcome::Error(format!("SSE stream chunk error: {e}"))
            }
            Some(Ok(bytes)) => {
                let lines = line_buf.push(&bytes);
                for line in lines {
                    if let Some(event) = sse_parser.push_line(&line) {
                        if let Some(data) = event.data {
                            if tx
                                .send(ToolChunk::intermediate(data))
                                .is_err()
                            {
                                return DrainOutcome::Cancelled;
                            }
                        }
                    }
                }
            }
        }
    }
    // Flush trailing line (events without a final blank-line
    // terminator) — push it into the parser; if it completes an
    // event, emit it.
    if let Some(line) = line_buf.flush() {
        if let Some(event) = sse_parser.push_line(&line) {
            if let Some(data) = event.data {
                let _ = tx.send(ToolChunk::intermediate(data));
            }
        }
    }
    DrainOutcome::Completed
}

/// Drain NDJSON framing. Each LF-delimited line emits as a
/// `ToolChunk::intermediate`. Empty lines are skipped per
/// `application/x-ndjson` spec.
async fn drain_ndjson<S>(
    byte_stream: &mut S,
    cancel: &crate::cancel_token::CancellationFlag,
    tx: &tokio::sync::mpsc::UnboundedSender<ToolChunk>,
) -> DrainOutcome
where
    S: futures::Stream<Item = reqwest::Result<Bytes>> + Unpin + Send,
{
    let mut line_buf = LineBuffer::new();
    loop {
        if cancel.is_cancelled() {
            return DrainOutcome::Cancelled;
        }
        match byte_stream.next().await {
            None => break,
            Some(Err(e)) => {
                return DrainOutcome::Error(format!("NDJSON stream chunk error: {e}"))
            }
            Some(Ok(bytes)) => {
                let lines = line_buf.push(&bytes);
                for line in lines {
                    if !line.is_empty()
                        && tx.send(ToolChunk::intermediate(line)).is_err()
                    {
                        return DrainOutcome::Cancelled;
                    }
                }
            }
        }
    }
    if let Some(line) = line_buf.flush() {
        if !line.is_empty() {
            let _ = tx.send(ToolChunk::intermediate(line));
        }
    }
    DrainOutcome::Completed
}

/// Drain single-chunk framing. Accumulate the full body + emit as
/// 1 `ToolChunk::intermediate` (terminator follows from the caller).
/// D9 backwards-compat for non-streaming HTTP endpoints.
async fn drain_single<S>(
    byte_stream: &mut S,
    cancel: &crate::cancel_token::CancellationFlag,
    tx: &tokio::sync::mpsc::UnboundedSender<ToolChunk>,
) -> DrainOutcome
where
    S: futures::Stream<Item = reqwest::Result<Bytes>> + Unpin + Send,
{
    let mut acc: Vec<u8> = Vec::new();
    loop {
        if cancel.is_cancelled() {
            return DrainOutcome::Cancelled;
        }
        match byte_stream.next().await {
            None => break,
            Some(Err(e)) => {
                return DrainOutcome::Error(format!("HTTP body chunk error: {e}"))
            }
            Some(Ok(bytes)) => acc.extend_from_slice(&bytes),
        }
    }
    let body_text = String::from_utf8_lossy(&acc).into_owned();
    if !body_text.is_empty()
        && tx
            .send(ToolChunk::intermediate(body_text))
            .is_err()
    {
        return DrainOutcome::Cancelled;
    }
    DrainOutcome::Completed
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_registry::{ToolEntry, ToolSource};

    /// §Fase 114 (owed) — **the HTTP client is POOLED: every call reuses ONE
    /// client, not a fresh one per request.** reqwest's connection pool lives on
    /// the `Client`; a fresh `Client::builder().build()` per call discarded it, so
    /// every tool call paid a new TCP + TLS handshake. This pins the fix directly:
    /// the shared accessor returns the SAME instance across calls — one client,
    /// one pool. (Connection reuse from a shared client is reqwest's documented
    /// behavior; what this crate owns is *not rebuilding the client per call*.)
    #[test]
    fn the_blocking_http_client_is_a_reused_singleton() {
        let a = shared_blocking_client();
        let b = shared_blocking_client();
        assert!(
            std::ptr::eq(a, b),
            "every call must reuse ONE blocking client (its connection pool) — not build a fresh \
             one per request"
        );
    }

    #[test]
    fn the_async_http_client_is_a_reused_singleton() {
        let a = shared_async_client();
        let b = shared_async_client();
        assert!(
            std::ptr::eq(a, b),
            "every call must reuse ONE async client (its connection pool)"
        );
    }

    fn make_http_entry(name: &str, url: &str, timeout: &str) -> ToolEntry {
        ToolEntry {
            name: name.to_string(),
            provider: "http".to_string(),
            timeout: timeout.to_string(),
            runtime: url.to_string(),
            resource_ref: String::new(),
            substrate: None,
            capacity: None,
            sandbox: None,
            max_results: None,
            output_schema: "JSON".to_string(),
            effect_row: vec!["network".to_string()],
            parameters: Vec::new(),
            secret: String::new(),
            secret_partition: String::new(),
            source: ToolSource::Program,
            // §Fase 34.c — HTTP tools default to non-streaming; effect_row
            // carries `network` but no `stream:` prefix. HTTP streaming
            // (SSE-aware adapter consuming upstream SSE) lands in Fase 34.e.
            is_streaming: false,
            scrape: None,
        }
    }

    // ── Timeout parsing ───────────────────────────────────────────

    #[test]
    fn parse_timeout_seconds() {
        assert_eq!(parse_timeout("10s"), Some(Duration::from_secs(10)));
        assert_eq!(parse_timeout("30s"), Some(Duration::from_secs(30)));
    }

    #[test]
    fn parse_timeout_milliseconds() {
        assert_eq!(parse_timeout("500ms"), Some(Duration::from_millis(500)));
        assert_eq!(parse_timeout("100ms"), Some(Duration::from_millis(100)));
    }

    #[test]
    fn parse_timeout_minutes() {
        assert_eq!(parse_timeout("2m"), Some(Duration::from_secs(120)));
    }

    #[test]
    fn parse_timeout_raw_number() {
        assert_eq!(parse_timeout("15"), Some(Duration::from_secs(15)));
    }

    #[test]
    fn parse_timeout_empty() {
        assert_eq!(parse_timeout(""), None);
        assert_eq!(parse_timeout("  "), None);
    }

    #[test]
    fn parse_timeout_invalid() {
        assert_eq!(parse_timeout("abc"), None);
        assert_eq!(parse_timeout("10x"), None);
    }

    // ── URL validation ────────────────────────────────────────────

    #[test]
    fn dispatch_empty_url_fails() {
        let entry = make_http_entry("DataAPI", "", "10s");
        let result = dispatch_http(&entry, "test query");
        assert!(!result.success);
        assert!(result.output.contains("no endpoint URL"));
    }

    #[test]
    fn dispatch_invalid_url_scheme_fails() {
        let entry = make_http_entry("DataAPI", "ftp://example.com", "10s");
        let result = dispatch_http(&entry, "test query");
        assert!(!result.success);
        assert!(result.output.contains("invalid URL"));
        assert!(result.output.contains("http://"));
    }

    // ── Connection errors (no server) ─────────────────────────────

    #[test]
    fn dispatch_connection_refused() {
        // Port 1 is almost certainly not listening
        let entry = make_http_entry("TestTool", "http://127.0.0.1:1/api", "2s");
        let result = dispatch_http(&entry, "test");
        assert!(!result.success);
        assert!(
            result.output.contains("connection failed")
                || result.output.contains("request failed")
                || result.output.contains("timed out"),
            "unexpected error: {}",
            result.output
        );
    }

    // ── Body wrapping ─────────────────────────────────────────────

    #[test]
    fn json_body_passthrough() {
        // If argument is already JSON, it should be sent as-is
        let arg = r#"{"query": "test"}"#;
        let body = if arg.trim_start().starts_with('{') {
            arg.to_string()
        } else {
            serde_json::json!({ "input": arg }).to_string()
        };
        assert_eq!(body, r#"{"query": "test"}"#);
    }

    #[test]
    fn plain_text_wrapped() {
        // If argument is plain text, it should be wrapped
        let arg = "search for cats";
        let body = if arg.trim_start().starts_with('{') || arg.trim_start().starts_with('[') {
            arg.to_string()
        } else {
            serde_json::json!({ "input": arg }).to_string()
        };
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["input"], "search for cats");
    }

    #[test]
    fn array_body_passthrough() {
        let arg = r#"[1, 2, 3]"#;
        let body = if arg.trim_start().starts_with('{') || arg.trim_start().starts_with('[') {
            arg.to_string()
        } else {
            serde_json::json!({ "input": arg }).to_string()
        };
        assert_eq!(body, "[1, 2, 3]");
    }
}
