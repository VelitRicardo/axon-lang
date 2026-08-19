//! Tracing span helpers for native Rust LLM backends — v1.18.0.
//!
//! Each provider's HTTP loop emits structured `tracing` events around the
//! lifecycle of a single `complete` / `stream` call. The shape is fixed
//! so downstream subscribers (`tracing-subscriber`, OpenTelemetry,
//! Jaeger, log forwarders) can parse fields without string heuristics.
//!
//! # Span layout
//!
//! ```text
//! backend.complete  (provider, model, trace_id)
//!   ├─ event: request_built     (max_tokens, temperature, n_messages, n_tools)
//!   ├─ event: http_send         (url, body_size_bytes)
//!   ├─ event: http_recv         (status_code, body_size_bytes, duration_ms)
//!   ├─ event: retry_scheduled   (attempt, after_ms, reason)         [if retry]
//!   ├─ event: parsed_response   (input_tokens, output_tokens, finish_reason)
//!   └─ event: complete          (total_duration_ms, retry_count, success=true)
//! ```
//!
//! Errors emit `event: error` with category + status + message instead of
//! the `parsed_response` + `complete` pair.
//!
//! # Use
//!
//! ```ignore
//! use axon::backends::observability::{call_span, on_request_built, on_http_send,
//!     on_http_recv, on_retry_scheduled, on_parsed_response, on_complete, on_error};
//!
//! let span = call_span("anthropic", "claude-sonnet-4-5", trace_id);
//! let _enter = span.enter();
//! on_request_built(2048, Some(0.7), 4, 0);
//! on_http_send("https://api.anthropic.com/v1/messages", body_bytes.len());
//! // ... HTTP call ...
//! on_http_recv(200, response_bytes, elapsed_ms);
//! on_parsed_response(input_tokens, output_tokens, "end_turn");
//! on_complete(total_ms, retry_count, true);
//! ```

use tracing::{Span, info_span};

/// Open the top-level span for one `complete` / `stream` invocation.
///
/// Returns a [`Span`] handle the caller enters via `let _enter = span.enter();`
/// for the lifetime of the call. The span carries the canonical
/// `(provider, model, trace_id)` tuple downstream subscribers index by.
///
/// `trace_id` is propagated from the calling context (typically the
/// [`crate::request_tracing`] middleware) so spans for the same flow
/// step correlate.
pub fn call_span(provider: &str, model: &str, trace_id: &str) -> Span {
    info_span!(
        "backend.complete",
        provider = provider,
        model = model,
        trace_id = trace_id,
    )
}

/// Open a span specifically for streaming calls. Identical fields to
/// [`call_span`] but a different name so subscribers can disambiguate
/// stream-vs-request workloads in metrics.
pub fn stream_span(provider: &str, model: &str, trace_id: &str) -> Span {
    info_span!(
        "backend.stream",
        provider = provider,
        model = model,
        trace_id = trace_id,
    )
}

/// Emit `event: request_built` with the canonical request shape fields.
/// Called once per call, immediately after the body JSON is constructed.
pub fn on_request_built(
    max_tokens: Option<u32>,
    temperature: Option<f64>,
    n_messages: usize,
    n_tools: usize,
) {
    tracing::info!(
        event = "request_built",
        max_tokens = max_tokens.map(|v| v as i64).unwrap_or(-1),
        temperature = temperature.unwrap_or(f64::NAN),
        n_messages,
        n_tools,
    );
}

/// Emit `event: http_send` immediately before issuing the HTTP request.
pub fn on_http_send(url: &str, body_size_bytes: usize) {
    tracing::info!(event = "http_send", url, body_size_bytes);
}

/// Emit `event: http_recv` after the HTTP response arrives (success OR
/// status error — caller decides what to do with the result).
pub fn on_http_recv(status_code: u16, body_size_bytes: usize, duration_ms: u64) {
    tracing::info!(
        event = "http_recv",
        status_code,
        body_size_bytes,
        duration_ms,
    );
}

/// Emit `event: retry_scheduled` after a retryable error decides to
/// retry. `after_ms` is the delay before the next attempt; `reason` is a
/// short label (`"429"` / `"503"` / `"timeout"` / `"connect"`).
pub fn on_retry_scheduled(attempt: u32, after_ms: u64, reason: &str) {
    tracing::warn!(
        event = "retry_scheduled",
        attempt,
        after_ms,
        reason,
    );
}

/// Emit `event: parsed_response` after a successful 200 OK is decoded.
pub fn on_parsed_response(input_tokens: u32, output_tokens: u32, finish_reason: &str) {
    tracing::info!(
        event = "parsed_response",
        input_tokens,
        output_tokens,
        finish_reason,
    );
}

/// Emit `event: complete` as the final span event on success.
pub fn on_complete(total_duration_ms: u64, retry_count: u32, success: bool) {
    tracing::info!(
        event = "complete",
        total_duration_ms,
        retry_count,
        success,
    );
}

/// Emit `event: error` as the final span event on failure. `category`
/// matches `BackendError::category()`; `status` is the HTTP status code
/// when applicable, `None` for transport-layer failures.
pub fn on_error(category: &str, status: Option<u16>, message: &str) {
    tracing::error!(
        event = "error",
        category,
        status = status.map(|v| v as i64).unwrap_or(-1),
        message,
    );
}

// ────────────────────────────────────────────────────────────────────
//  Tests — verify the span / event helpers don't panic and produce
//  fields the subscriber crate can pick up. Subscribers are not wired
//  inside the unit tests (no real assertion of emitted events here);
//  end-to-end tracing assertions live in 24.j cross-backend tests.
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Instrument;

    /// v2.81.0 — install a subscriber for the assertion's duration.
    ///
    /// `Span::metadata()` returns `None` for a DISABLED span, and a span is
    /// disabled when no subscriber is interested. These two tests asserted
    /// `Some(...)` while installing nothing — so they were reading AMBIENT
    /// state, and passed only because `sqlx` (unconditional until v2.81.0)
    /// pulled `tracing` machinery that left spans enabled. Making `postgres`
    /// optional removed `sqlx` from the lean build and the assertions went red,
    /// having never actually tested what they claimed.
    ///
    /// The fix is not to gate them behind `postgres` — the span names are not a
    /// database property. It is to stop depending on what some other crate
    /// happens to switch on. Output goes to `io::sink` so the suite stays quiet.
    fn with_spans_enabled<T>(f: impl FnOnce() -> T) -> T {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(std::io::sink)
            .finish();
        tracing::subscriber::with_default(subscriber, f)
    }

    #[test]
    fn call_span_constructs_with_canonical_fields() {
        with_spans_enabled(|| {
            let span = call_span("anthropic", "claude-x", "trace-1");
            let _enter = span.enter();
            // The metadata target is the module path; the span name is fixed.
            assert_eq!(span.metadata().map(|m| m.name()), Some("backend.complete"));
        });
    }

    #[test]
    fn stream_span_distinct_from_call_span() {
        with_spans_enabled(|| {
            let s = stream_span("openai", "gpt-x", "trace-2");
            assert_eq!(s.metadata().map(|m| m.name()), Some("backend.stream"));
        });
    }

    #[tokio::test]
    async fn helpers_emit_without_panicking_inside_a_span() {
        let span = call_span("kimi", "kimi-k2.6", "trace-3");
        async {
            on_request_built(Some(2048), Some(0.7), 4, 0);
            on_http_send("https://api.moonshot.ai/v1/chat/completions", 1024);
            on_http_recv(200, 2048, 150);
            on_parsed_response(120, 80, "stop");
            on_complete(150, 0, true);
        }
        .instrument(span)
        .await;
    }

    #[tokio::test]
    async fn retry_and_error_helpers_emit() {
        let span = call_span("openai", "gpt-x", "trace-4");
        async {
            on_retry_scheduled(0, 1000, "429");
            on_error("rate_limit", Some(429), "rate limited");
        }
        .instrument(span)
        .await;
    }

    #[test]
    fn request_built_handles_unset_temperature() {
        // f64::NAN is the sentinel for "not supplied"; the helper should
        // not panic when temperature is None.
        on_request_built(None, None, 0, 0);
    }
}
