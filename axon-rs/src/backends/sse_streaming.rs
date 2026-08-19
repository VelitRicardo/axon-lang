//! v1.24.0 — Shared SSE / JSONL streaming infrastructure.
//!
//! Every native backend that produces a `Backend::stream()` consumes a
//! `reqwest::Response::bytes_stream()` of `Bytes` and decodes it into a
//! provider-specific event sequence. This module is the single source
//! of truth for the BYTE-LEVEL plumbing every backend needs:
//!
//!   1. **Line buffering** — backend bytes arrive in arbitrary chunks
//!      (TCP read sizes, not message boundaries). [`LineBuffer`]
//!      accumulates the byte tail and yields complete LF-delimited lines
//!      as `String`. The tail (incomplete final line) is held until the
//!      next chunk arrives. On stream end any non-empty tail is flushed
//!      as a final line so trailing-newline-less providers (some
//!      Anthropic edge cases) still surface the final event.
//!
//!   2. **SSE event parsing** — [`SseEvent`] captures the W3C-spec
//!      shape `{event, id, data, retry}`. Stateful parser
//!      [`SseEventParser`] turns a sequence of lines into a sequence
//!      of complete events (events terminate on a blank line per W3C).
//!
//!   3. **HTTP body → string-stream wrapper** — [`bytes_stream_to_lines`]
//!      and [`bytes_stream_to_sse_events`] adapt a
//!      `Stream<Item = reqwest::Result<Bytes>>` into the higher-level
//!      shape each backend consumes.
//!
//! The per-provider chunk-to-ChatChunk mapping lives in each backend's
//! module — that's the SEMANTIC layer (e.g. `delta.text` for Anthropic
//! vs `choices[0].delta.content` for OpenAI). The infrastructure here
//! is provider-neutral.
//!
//! ## D-letter trace
//!
//! - **D3** (33.d) — Every native backend implements `Backend::stream()`
//!   natively, sharing this byte-level infrastructure.
//! - **D9** (v1.24.0) — Each impl produces `ChatChunk` instances whose
//!   serialized `delta` content is byte-identical with the network
//!   bytes the provider sent (modulo UTF-8 boundaries the buffer
//!   stitches back together).
//! - **D10** (cancel-safety, v1.24.0) — Both [`LineBuffer`] and
//!   [`SseEventParser`] are non-blocking value types; dropping the
//!   stream consumer drops the buffers without leaking state.

#![allow(dead_code)]

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use std::pin::Pin;
use std::task::{Context, Poll};

use super::error::BackendError;
use crate::cancel_token::CancellationFlag;

// ────────────────────────────────────────────────────────────────────
// v1.24.0 — Cancel-aware stream adapter
// ────────────────────────────────────────────────────────────────────

/// Wrap any `Stream` so it terminates promptly when the supplied
/// [`CancellationFlag`] fires.
///
/// The returned stream's `poll_next` races two futures:
///   - `inner.next()` — the upstream body chunk.
///   - `cancel.cancelled()` — a tokio Notify-backed future that
///     resolves the moment any clone of the flag calls `cancel()`.
///
/// On cancel the stream yields `None` (clean end-of-stream from the
/// consumer's perspective) WITHOUT first awaiting the next chunk.
/// Dropping the returned stream then drops the wrapped `inner`,
/// which for a `reqwest::Response::bytes_stream` aborts the
/// underlying HTTP body — the TCP connection closes (or returns to
/// the pool, depending on H1/H2) without consuming further bytes.
///
/// # Measurable invariant (D3)
///
/// p95 latency from `flag.cancel()` to consumer's next `None`
/// observation MUST be ≤ 100ms under a local-loopback HTTP server
/// emitting one SSE chunk every 1 second. Enforced by
/// `axon-rs/tests/cancel_inside_body.rs`.
///
/// # Pure async — no busy-poll
///
/// The select inside `poll_next` parks the task on a tokio Notify
/// waker; no spinning, no timer polling. Cancel wakes the parked
/// task atomically — the same Notify that powers
/// `CancellationFlag::cancelled()`.
///
/// # Type
///
/// Returns `Pin<Box<dyn Stream<Item = T> + Send + Unpin>>` to
/// match the per-provider [`super::ChatStream`] alias. Adopters
/// that already hold a `Pin<Box<dyn Stream<...>>>` (the canonical
/// `ChatStream` shape) can call this wrapper without re-typing.
pub fn cancel_aware<S, T>(
    stream: S,
    cancel: CancellationFlag,
) -> Pin<Box<dyn Stream<Item = T> + Send>>
where
    S: Stream<Item = T> + Send + Unpin + 'static,
    T: Send + 'static,
{
    Box::pin(futures::stream::unfold(
        (stream, cancel),
        |(mut s, cancel)| async move {
            // Fast path: already cancelled. Avoid a single poll on
            // the inner stream which may still yield a buffered
            // chunk; the contract is "no further deliveries after
            // cancel fires".
            if cancel.is_cancelled() {
                return None;
            }
            // Race the next chunk against the cancel signal.
            // `biased;` ensures we re-check cancel first on every
            // tick — a fired cancel takes priority over a chunk
            // already buffered in the inner stream.
            tokio::select! {
                biased;
                _ = cancel.cancelled() => None,
                item = s.next() => item.map(|x| (x, (s, cancel))),
            }
        },
    ))
}

// ────────────────────────────────────────────────────────────────────
//  Line buffering
// ────────────────────────────────────────────────────────────────────

/// Accumulates bytes from a chunk-shaped stream and yields complete
/// LF-delimited lines. CRLF endings are normalized to LF before yield.
///
/// The buffer holds bytes until the next LF arrives; the trailing
/// fragment after the last LF is retained for the next push.
///
/// UTF-8 decoding is best-effort: if a chunk lands mid-codepoint we
/// hold the partial codepoint until the next chunk completes it. Pure
/// invalid UTF-8 surfaces as a replacement character per
/// [`String::from_utf8_lossy`].
#[derive(Debug, Default)]
pub struct LineBuffer {
    /// Bytes accumulated since the last LF. Capacity grows as needed
    /// but never shrinks below the high-water mark of the longest line
    /// observed so far (amortized O(1) per push for well-behaved
    /// providers that don't emit pathological single megaline events).
    tail: Vec<u8>,
}

impl LineBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push `chunk` into the buffer. Returns all complete LF-delimited
    /// lines (CR stripped) that became available; the tail is retained
    /// for the next push.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        for &byte in chunk {
            if byte == b'\n' {
                // Strip trailing CR (CRLF normalization).
                if self.tail.last() == Some(&b'\r') {
                    self.tail.pop();
                }
                out.push(String::from_utf8_lossy(&self.tail).into_owned());
                self.tail.clear();
            } else {
                self.tail.push(byte);
            }
        }
        out
    }

    /// Flush any partial trailing line as the final entry. Callers
    /// invoke this once the upstream byte-stream has ended; providers
    /// that close without a final LF still surface their final event.
    pub fn flush(&mut self) -> Option<String> {
        if self.tail.is_empty() {
            return None;
        }
        // Strip trailing CR if present.
        if self.tail.last() == Some(&b'\r') {
            self.tail.pop();
        }
        let line = String::from_utf8_lossy(&self.tail).into_owned();
        self.tail.clear();
        Some(line)
    }

    /// True if no bytes are pending.
    pub fn is_empty(&self) -> bool {
        self.tail.is_empty()
    }
}

// ────────────────────────────────────────────────────────────────────
//  Server-Sent Events
// ────────────────────────────────────────────────────────────────────

/// One parsed SSE event per W3C spec "event stream interpretation".
///
/// All four fields are optional because the spec allows any subset —
/// a `retry:` directive on its own carries neither data nor event.
/// In practice every provider always sets `data`; most set `event`;
/// some set `id`; only the W3C-compliant `retry:` directive lacks data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub id: Option<String>,
    pub data: Option<String>,
    pub retry_ms: Option<u64>,
}

impl SseEvent {
    /// True iff the event carries no fields. Per W3C an empty event
    /// (consecutive blank lines) is a no-op the dispatcher MUST drop.
    pub fn is_empty(&self) -> bool {
        self.event.is_none()
            && self.id.is_none()
            && self.data.is_none()
            && self.retry_ms.is_none()
    }
}

/// Stateful SSE event parser. Feed it complete LF-delimited lines via
/// [`Self::push_line`]; it emits a complete [`SseEvent`] each time it
/// sees a terminator blank line per W3C spec.
#[derive(Debug, Default)]
pub struct SseEventParser {
    current: SseEvent,
    /// `data:` lines accumulate via newline-join per W3C "data field".
    data_acc: Vec<String>,
}

impl SseEventParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push one complete line. Returns `Some(event)` iff the line
    /// terminated an event (blank line); `None` while still
    /// accumulating fields.
    ///
    /// Per W3C "event stream interpretation":
    ///   - `event: <name>`     → set event name
    ///   - `id: <id>`          → set last event id
    ///   - `data: <line>`      → append (with newline) to data buffer
    ///   - `retry: <ms>`       → set retry hint
    ///   - `<empty line>`      → dispatch accumulated event
    ///   - `: <comment>`       → ignore (comment line)
    ///   - other               → ignore (unknown field)
    pub fn push_line(&mut self, line: &str) -> Option<SseEvent> {
        // Blank line = event terminator. Per W3C the event dispatches
        // only if non-empty (consecutive blank lines = no-op).
        if line.is_empty() {
            // Materialise accumulated data lines if any.
            if !self.data_acc.is_empty() {
                self.current.data = Some(self.data_acc.join("\n"));
                self.data_acc.clear();
            }
            let event = std::mem::take(&mut self.current);
            return if event.is_empty() { None } else { Some(event) };
        }

        // Comment line: starts with `:`.
        if line.starts_with(':') {
            return None;
        }

        // Field line: `<field>:<space?><value>`. The W3C spec allows
        // the colon-only form (empty value) so we treat missing-colon
        // as a single-token field with empty value.
        let (field, raw_value) = match line.find(':') {
            Some(idx) => (&line[..idx], &line[idx + 1..]),
            None => (line, ""),
        };
        // Strip the optional single space after the colon per W3C.
        let value = raw_value.strip_prefix(' ').unwrap_or(raw_value);

        match field {
            "event" => self.current.event = Some(value.to_string()),
            "id" => self.current.id = Some(value.to_string()),
            "data" => self.data_acc.push(value.to_string()),
            "retry" => {
                if let Ok(ms) = value.parse::<u64>() {
                    self.current.retry_ms = Some(ms);
                }
            }
            _ => {
                // Unknown field — silently ignore per W3C spec.
            }
        }
        None
    }

    /// Flush any pending event (no terminator blank line observed).
    /// Most providers do close the final event with a blank line so
    /// this is usually a no-op; it covers the edge case of a network
    /// drop mid-event or a non-compliant terminator-omitting server.
    pub fn flush(&mut self) -> Option<SseEvent> {
        if !self.data_acc.is_empty() {
            self.current.data = Some(self.data_acc.join("\n"));
            self.data_acc.clear();
        }
        let event = std::mem::take(&mut self.current);
        if event.is_empty() {
            None
        } else {
            Some(event)
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  Stream adapter: bytes → lines
// ────────────────────────────────────────────────────────────────────

/// Adapter that pulls from a `Stream<Item = reqwest::Result<Bytes>>`
/// and yields complete LF-delimited lines as `Result<String,
/// BackendError>`. Provider-neutral.
pub struct LineStream<S> {
    inner: S,
    buffer: LineBuffer,
    /// Lines extracted from the most-recent chunk that haven't been
    /// yielded yet. `poll_next` drains this before pulling the next
    /// chunk.
    pending: std::collections::VecDeque<String>,
    /// `true` once the inner stream has ended and the buffer tail
    /// (if any) has been flushed.
    done: bool,
    /// Provider name + model — surfaced in error contexts so an HTTP
    /// 502 from the upstream provider surfaces as a typed BackendError
    /// with full diagnostic context.
    provider: String,
    model: String,
}

impl<S> LineStream<S> {
    pub fn new(inner: S, provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            inner,
            buffer: LineBuffer::new(),
            pending: std::collections::VecDeque::new(),
            done: false,
            provider: provider.into(),
            model: model.into(),
        }
    }
}

impl<S> Stream for LineStream<S>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    type Item = Result<String, BackendError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            // Drain pending lines first.
            if let Some(line) = self.pending.pop_front() {
                return Poll::Ready(Some(Ok(line)));
            }
            if self.done {
                return Poll::Ready(None);
            }
            match self.inner.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    let lines = self.buffer.push(&chunk);
                    self.pending.extend(lines);
                    // Loop back to drain.
                }
                Poll::Ready(Some(Err(e))) => {
                    self.done = true;
                    return Poll::Ready(Some(Err(BackendError::Generic {
                        provider: self.provider.clone(),
                        model: self.model.clone(),
                        status: None,
                        message: format!("stream transport error: {e}"),
                    })));
                }
                Poll::Ready(None) => {
                    // Upstream done — flush any trailing fragment.
                    if let Some(tail) = self.buffer.flush() {
                        self.pending.push_back(tail);
                    }
                    self.done = true;
                    // Loop back to drain final pending.
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  Stream adapter: bytes → SSE events
// ────────────────────────────────────────────────────────────────────

/// Adapter that pulls from a `Stream<Item = reqwest::Result<Bytes>>`
/// and yields complete SSE events. Provider-neutral.
pub struct SseEventStream<S> {
    line_stream: LineStream<S>,
    parser: SseEventParser,
    /// `true` once the line stream has ended and the parser tail has
    /// been flushed.
    done: bool,
    /// `true` once we've yielded the parser's flush result (so we
    /// don't loop forever after final-flush).
    flushed: bool,
}

impl<S> SseEventStream<S> {
    pub fn new(
        inner: S,
        provider: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            line_stream: LineStream::new(inner, provider, model),
            parser: SseEventParser::new(),
            done: false,
            flushed: false,
        }
    }
}

impl<S> Stream for SseEventStream<S>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    type Item = Result<SseEvent, BackendError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if self.done {
                return Poll::Ready(None);
            }
            match self.line_stream.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(line))) => {
                    if let Some(event) = self.parser.push_line(&line) {
                        return Poll::Ready(Some(Ok(event)));
                    }
                    // No event yet; loop to pull next line.
                }
                Poll::Ready(Some(Err(e))) => {
                    self.done = true;
                    return Poll::Ready(Some(Err(e)));
                }
                Poll::Ready(None) => {
                    if !self.flushed {
                        self.flushed = true;
                        if let Some(event) = self.parser.flush() {
                            return Poll::Ready(Some(Ok(event)));
                        }
                    }
                    self.done = true;
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  Convenience constructors
// ────────────────────────────────────────────────────────────────────

/// Wrap a `reqwest::Response::bytes_stream()` in a line stream.
/// Convenience over `LineStream::new(response.bytes_stream(), ...)`.
pub fn line_stream(
    response: reqwest::Response,
    provider: impl Into<String>,
    model: impl Into<String>,
) -> LineStream<impl Stream<Item = Result<Bytes, reqwest::Error>> + Unpin> {
    LineStream::new(Box::pin(response.bytes_stream()), provider, model)
}

/// Wrap a `reqwest::Response::bytes_stream()` in an SSE event stream.
/// Convenience over `SseEventStream::new(response.bytes_stream(), ...)`.
pub fn sse_event_stream(
    response: reqwest::Response,
    provider: impl Into<String>,
    model: impl Into<String>,
) -> SseEventStream<impl Stream<Item = Result<Bytes, reqwest::Error>> + Unpin> {
    SseEventStream::new(Box::pin(response.bytes_stream()), provider, model)
}

// ────────────────────────────────────────────────────────────────────
//  Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── LineBuffer ──────────────────────────────────────────────────

    #[test]
    fn line_buffer_yields_complete_lf_lines() {
        let mut buf = LineBuffer::new();
        let lines = buf.push(b"hello\nworld\n");
        assert_eq!(lines, vec!["hello", "world"]);
        assert!(buf.is_empty());
    }

    #[test]
    fn line_buffer_holds_partial_line_until_lf() {
        let mut buf = LineBuffer::new();
        let lines = buf.push(b"hello");
        assert!(lines.is_empty());
        assert!(!buf.is_empty());
        let lines = buf.push(b" world\n");
        assert_eq!(lines, vec!["hello world"]);
    }

    #[test]
    fn line_buffer_normalizes_crlf() {
        let mut buf = LineBuffer::new();
        let lines = buf.push(b"hello\r\nworld\r\n");
        assert_eq!(lines, vec!["hello", "world"]);
    }

    #[test]
    fn line_buffer_splits_chunk_across_pushes() {
        let mut buf = LineBuffer::new();
        let lines = buf.push(b"hel");
        assert!(lines.is_empty());
        let lines = buf.push(b"lo\nwor");
        assert_eq!(lines, vec!["hello"]);
        let lines = buf.push(b"ld\n");
        assert_eq!(lines, vec!["world"]);
    }

    #[test]
    fn line_buffer_flush_returns_trailing_fragment() {
        let mut buf = LineBuffer::new();
        let _ = buf.push(b"complete\nincomplete");
        let tail = buf.flush();
        assert_eq!(tail, Some("incomplete".to_string()));
        assert!(buf.is_empty());
    }

    #[test]
    fn line_buffer_flush_on_empty_returns_none() {
        let mut buf = LineBuffer::new();
        assert_eq!(buf.flush(), None);
    }

    #[test]
    fn line_buffer_empty_chunk_is_noop() {
        let mut buf = LineBuffer::new();
        let lines = buf.push(b"");
        assert!(lines.is_empty());
        assert!(buf.is_empty());
    }

    #[test]
    fn line_buffer_handles_consecutive_lf() {
        let mut buf = LineBuffer::new();
        let lines = buf.push(b"a\n\nb\n");
        assert_eq!(lines, vec!["a", "", "b"]);
    }

    // ── SseEventParser ─────────────────────────────────────────────

    #[test]
    fn sse_parser_data_only_event() {
        let mut p = SseEventParser::new();
        assert!(p.push_line("data: hello").is_none());
        let ev = p.push_line("").expect("event dispatched on blank");
        assert_eq!(ev.data, Some("hello".to_string()));
        assert!(ev.event.is_none());
    }

    #[test]
    fn sse_parser_full_event_shape() {
        let mut p = SseEventParser::new();
        assert!(p.push_line("event: axon.token").is_none());
        assert!(p.push_line("id: 42").is_none());
        assert!(p.push_line("data: hello").is_none());
        let ev = p.push_line("").expect("dispatched");
        assert_eq!(ev.event, Some("axon.token".to_string()));
        assert_eq!(ev.id, Some("42".to_string()));
        assert_eq!(ev.data, Some("hello".to_string()));
    }

    #[test]
    fn sse_parser_multi_line_data_joins_with_lf() {
        let mut p = SseEventParser::new();
        p.push_line("data: line1");
        p.push_line("data: line2");
        p.push_line("data: line3");
        let ev = p.push_line("").expect("dispatched");
        assert_eq!(ev.data, Some("line1\nline2\nline3".to_string()));
    }

    #[test]
    fn sse_parser_retry_directive_parsed_to_u64() {
        let mut p = SseEventParser::new();
        p.push_line("retry: 5000");
        let ev = p.push_line("").expect("dispatched");
        assert_eq!(ev.retry_ms, Some(5000));
    }

    #[test]
    fn sse_parser_retry_invalid_value_silently_ignored() {
        let mut p = SseEventParser::new();
        p.push_line("retry: not-a-number");
        p.push_line("data: x");
        let ev = p.push_line("").expect("dispatched");
        assert_eq!(ev.retry_ms, None);
        assert_eq!(ev.data, Some("x".to_string()));
    }

    #[test]
    fn sse_parser_comment_lines_ignored() {
        let mut p = SseEventParser::new();
        p.push_line(": this is a comment");
        p.push_line("data: visible");
        let ev = p.push_line("").expect("dispatched");
        assert_eq!(ev.data, Some("visible".to_string()));
    }

    #[test]
    fn sse_parser_unknown_field_ignored() {
        let mut p = SseEventParser::new();
        p.push_line("bogus: ignored");
        p.push_line("data: visible");
        let ev = p.push_line("").expect("dispatched");
        assert_eq!(ev.data, Some("visible".to_string()));
    }

    #[test]
    fn sse_parser_consecutive_blank_lines_dont_dispatch_empty() {
        let mut p = SseEventParser::new();
        assert!(p.push_line("").is_none());
        assert!(p.push_line("").is_none());
        p.push_line("data: x");
        let ev = p.push_line("").expect("dispatched");
        assert_eq!(ev.data, Some("x".to_string()));
    }

    #[test]
    fn sse_parser_field_without_space_after_colon() {
        // Per W3C the single space after `:` is optional.
        let mut p = SseEventParser::new();
        p.push_line("data:nospace");
        let ev = p.push_line("").expect("dispatched");
        assert_eq!(ev.data, Some("nospace".to_string()));
    }

    #[test]
    fn sse_parser_field_without_colon_still_parsed_as_empty_value() {
        // Per W3C `<field>` with no colon = `<field>:` with empty value.
        let mut p = SseEventParser::new();
        p.push_line("data");
        let ev = p.push_line("").expect("dispatched");
        assert_eq!(ev.data, Some(String::new()));
    }

    #[test]
    fn sse_parser_flush_yields_pending_event_on_eof() {
        let mut p = SseEventParser::new();
        p.push_line("data: trailing");
        // No blank line yet; flush surfaces the pending event.
        let ev = p.flush().expect("flush yields pending");
        assert_eq!(ev.data, Some("trailing".to_string()));
    }

    #[test]
    fn sse_parser_flush_on_clean_state_returns_none() {
        let mut p = SseEventParser::new();
        assert!(p.flush().is_none());
    }

    #[test]
    fn sse_event_is_empty_predicate_total() {
        let empty = SseEvent::default();
        assert!(empty.is_empty());
        let non_empty = SseEvent {
            data: Some("x".into()),
            ..Default::default()
        };
        assert!(!non_empty.is_empty());
    }

    // ── LineStream + SseEventStream (end-to-end) ───────────────────

    use futures::stream;

    fn fake_chunk_stream(
        chunks: Vec<&'static [u8]>,
    ) -> impl Stream<Item = Result<Bytes, reqwest::Error>> + Unpin {
        Box::pin(stream::iter(
            chunks.into_iter().map(|c| Ok(Bytes::from_static(c))),
        ))
    }

    #[tokio::test]
    async fn line_stream_yields_complete_lines_across_chunk_boundaries() {
        let inner = fake_chunk_stream(vec![b"hel", b"lo\nwor", b"ld\n"]);
        let stream = LineStream::new(inner, "test", "test-model");
        let lines: Vec<String> = stream
            .map(|r| r.unwrap())
            .collect()
            .await;
        assert_eq!(lines, vec!["hello".to_string(), "world".to_string()]);
    }

    #[tokio::test]
    async fn line_stream_flushes_trailing_fragment_on_eof() {
        let inner = fake_chunk_stream(vec![b"a\nb"]);
        let stream = LineStream::new(inner, "test", "test-model");
        let lines: Vec<String> = stream
            .map(|r| r.unwrap())
            .collect()
            .await;
        assert_eq!(lines, vec!["a".to_string(), "b".to_string()]);
    }

    #[tokio::test]
    async fn sse_event_stream_parses_canonical_openai_data_format() {
        let inner = fake_chunk_stream(vec![
            b"data: {\"chunk\":1}\n",
            b"\n",
            b"data: {\"chunk\":2}\n",
            b"\n",
        ]);
        let stream = SseEventStream::new(inner, "openai", "gpt-4o-mini");
        let events: Vec<SseEvent> = stream
            .map(|r| r.unwrap())
            .collect()
            .await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, Some(r#"{"chunk":1}"#.to_string()));
        assert_eq!(events[1].data, Some(r#"{"chunk":2}"#.to_string()));
    }

    #[tokio::test]
    async fn sse_event_stream_parses_anthropic_event_data_pairs() {
        let inner = fake_chunk_stream(vec![
            b"event: message_start\n",
            b"data: {\"type\":\"message_start\"}\n",
            b"\n",
            b"event: content_block_delta\n",
            b"data: {\"delta\":{\"text\":\"hi\"}}\n",
            b"\n",
        ]);
        let stream = SseEventStream::new(inner, "anthropic", "claude-x");
        let events: Vec<SseEvent> = stream
            .map(|r| r.unwrap())
            .collect()
            .await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event.as_deref(), Some("message_start"));
        assert_eq!(events[1].event.as_deref(), Some("content_block_delta"));
        assert!(events[1].data.as_ref().unwrap().contains("hi"));
    }

    #[tokio::test]
    async fn sse_event_stream_yields_final_event_without_trailing_blank() {
        // Provider closes connection without a final blank line; flush
        // path surfaces the last event.
        let inner = fake_chunk_stream(vec![
            b"data: one\n\n",
            b"data: two\n",
        ]);
        let stream = SseEventStream::new(inner, "test", "test-model");
        let events: Vec<SseEvent> = stream
            .map(|r| r.unwrap())
            .collect()
            .await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].data, Some("two".to_string()));
    }
}
