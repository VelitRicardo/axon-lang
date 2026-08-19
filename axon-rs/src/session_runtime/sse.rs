//! v2.3.0 — Server-Sent Events carrier for the **single-polarity**
//! fragment of a session type. The paper's section 4.4 identity
//! `S_SSE = Π↓(S_WS)` makes SSE not a *parallel* protocol but the
//! downstream **projection** of the WebSocket dialogue: when a session
//! type satisfies [`SessionType::projects_to_sse`] (only `End` / `Send`
//! / internal-`Select` / `Rec` / `Var` — no client input, no offered
//! choice), the same [`SessionRuntime`] that runs over a WebSocket can
//! be driven onto an SSE response stream byte-for-byte compatible with
//! v1.24.0's W3C SSE framing.
//!
//! The wire shape: one SSE event per operational step. The event names
//! are namespaced under `axon.` (mirroring v1.24.0's `axon.token` /
//! `axon.complete` cohort), and the `data:` payload is the same JSON
//! envelope `Frame` uses on a WebSocket (minus the `v` key, which is
//! redundant inside an SSE event whose `Content-Type` already names the
//! protocol). The W3C SSE framing rules (`event:` line, `data:` line,
//! `\n\n` event-terminator) are honoured exactly — any compliant SSE
//! consumer (browsers' `EventSource`, `curl --no-buffer`, the v1.24.0
//! `bytes_stream_to_sse_events` parser) decodes the wire without any
//! axon-specific knowledge.
//!
//! The driver is **lazy**: a `Stream<Item = Result<Event, Infallible>>`
//! is built once and yields one event per `poll_next` — back-pressure
//! flows through the tokio runtime onto the underlying TCP socket
//! exactly as v1.24.0's SSE handlers already do.

use std::convert::Infallible;

use axum::response::sse::Event;
use futures::stream::{self, Stream};
use serde_json::json;

use super::error::ProtocolError;
use super::state::SessionRuntime;
use super::wire::Frame;
use super::ws::{apply_outgoing, next_outgoing_frame, PeerRole};

// ─── Closed catalog of SSE event names — namespaced under `axon.` ─────────
//
// Stable identifiers so SSE consumers can filter / dispatch without
// re-parsing JSON. Mirror the section 4.4 mapping `Π↓(send)` / `Π↓(select)` /
// `Π↓(end)` exactly; `axon.error` is the out-of-band carrier for
// protocol-error close signals (the SSE analogue of WebSocket's
// `1002 protocol error` close-frame reason).

const SSE_EVENT_SEND: &str = "axon.send";
const SSE_EVENT_SELECT: &str = "axon.select";
const SSE_EVENT_END: &str = "axon.end";
const SSE_EVENT_ERROR: &str = "axon.error";

/// Convert a [`Frame`] into an SSE [`Event`]. The mapping is closed and
/// total — every wire frame has exactly one SSE projection. The version
/// tag is dropped (SSE's `Content-Type: text/event-stream` already
/// disambiguates the protocol; the data payload mirrors the WebSocket
/// envelope's inner shape).
pub(super) fn frame_to_sse_event(frame: &Frame) -> Event {
    match frame {
        Frame::Send { payload_type, data } => Event::default()
            .event(SSE_EVENT_SEND)
            .data(json!({ "payload_type": payload_type, "data": data }).to_string()),
        Frame::Select { label } => Event::default()
            .event(SSE_EVENT_SELECT)
            .data(json!({ "label": label }).to_string()),
        Frame::End => Event::default()
            .event(SSE_EVENT_END)
            .data("{}".to_string()),
        Frame::Error { code, detail } => Event::default()
            .event(SSE_EVENT_ERROR)
            .data(json!({ "code": code, "detail": detail }).to_string()),
    }
}

/// Drive a session-typed dialogue over Server-Sent Events.
///
/// **Preconditions** (checked once, up front — a violation surfaces as an
/// immediate `axon.error` SSE event):
/// - `runtime`'s schema must satisfy
///   [`axon_frontend::session::SessionType::projects_to_sse`] — the SSE
///   fragment requires a single-polarity (producer-only) protocol.
///
/// **Output**: a stream of [`Event`]s yielding one event per producer
/// step (`Send` → `axon.send`, `Select` → `axon.select`, `End` →
/// `axon.end`) followed by stream closure. On a runtime error
/// (canonically: credit exhaustion at runtime, v2.3.0 "no rule at
/// n=0" axiom) the stream emits one final `axon.error` event carrying
/// the [`ProtocolError::code`] and then ends.
///
/// The stream is `Send + 'static` so axum's `Sse::new(stream)` accepts
/// it directly as a handler return value.
pub fn drive_sse_producer(
    runtime: SessionRuntime,
) -> impl Stream<Item = Result<Event, Infallible>> + Send + 'static {
    // The walker state: the runtime + a `done` flag. The flag flips to
    // `true` after the terminating `End` (or `Error`) event so the
    // stream poll returns `None` next.
    let init = WalkerState { runtime, done: false };
    let preflight_error = preflight_polarity_check(&init.runtime);
    let init_with_preflight = (init, preflight_error);
    stream::unfold(init_with_preflight, |(mut state, preflight)| async move {
        if state.done {
            return None;
        }
        // First poll: if the schema isn't in the SSE fragment, emit one
        // `axon.error` event and shut the stream down.
        if let Some(err) = preflight {
            state.done = true;
            return Some((
                Ok(error_event(&err)),
                (state, None),
            ));
        }
        match step_runtime(&mut state.runtime) {
            StepOutcome::Event(event, becomes_done) => {
                state.done = becomes_done;
                Some((Ok(event), (state, None)))
            }
            StepOutcome::Error(err) => {
                state.done = true;
                Some((Ok(error_event(&err)), (state, None)))
            }
            StepOutcome::Done => None,
        }
    })
}

#[derive(Debug)]
struct WalkerState {
    runtime: SessionRuntime,
    done: bool,
}

#[derive(Debug)]
enum StepOutcome {
    /// One SSE event was produced; `bool` is true iff this was the
    /// terminating `axon.end` (stream closes after).
    Event(Event, bool),
    /// A runtime ProtocolError fired (e.g., credit exhaustion) — emit a
    /// final `axon.error` event then close the stream.
    Error(ProtocolError),
    /// The runtime has nothing more to emit at this carrier. Stream
    /// closes immediately. (Reached only if the cursor is in a
    /// non-producer state, which the preflight check rules out — kept
    /// as a defence-in-depth terminator.)
    Done,
}

/// Verify the runtime is in the SSE producer fragment **before** the
/// first event is emitted; if not, the driver short-circuits with a
/// single `axon.error` event so the consumer is told *why* the stream
/// closed. Static analysis (`check_socket`) catches this at compile
/// time when the protocol is declared with a `socket`; this is the
/// runtime safety net for direct API users.
fn preflight_polarity_check(runtime: &SessionRuntime) -> Option<ProtocolError> {
    if runtime.schema().projects_to_sse() {
        None
    } else {
        Some(ProtocolError::UnexpectedFrame {
            // The cursor is whatever wrong-polarity head the schema has —
            // we report the canonical SSE-fragment expectation versus
            // what was passed.
            cursor_kind: "non-sse-polarity-schema",
            frame_kind: "sse-projection-requested",
        })
    }
}

/// Pull the next outgoing frame from the runtime, advance the cursor,
/// and report the SSE projection.
///
/// `End` is treated specially because [`next_outgoing_frame`] returns
/// `None` for that cursor (the WS driver handles termination at the
/// outer loop). For SSE we emit one explicit `axon.end` event so the
/// consumer sees the dialogue's closure on the wire rather than just
/// the TCP-level end-of-stream — symmetric with the v2.3.0 WS carrier's
/// `Frame::End` then close-`1000`.
fn step_runtime(runtime: &mut SessionRuntime) -> StepOutcome {
    if runtime.is_complete() {
        // Terminal `axon.end` event — the SSE counterpart of the WS
        // driver's final `Frame::End` then `1000 normal closure`.
        return StepOutcome::Event(frame_to_sse_event(&Frame::End), true);
    }
    let Some(frame) = next_outgoing_frame(runtime) else {
        // Cursor is at `Recv` / `Branch` / `Var` on a non-producer
        // protocol. The pre-flight should have caught this; reaching
        // here is a defence-in-depth terminator.
        return StepOutcome::Done;
    };
    // `apply_outgoing` is an `async fn` for the WS carrier (which can
    // observe network back-pressure on send); for our purposes here it
    // is pure state-machine advancement, so block on its future
    // synchronously. This keeps the stream `Send + 'static` without
    // requiring a tokio runtime context just to step the cursor.
    if let Err(e) = futures::executor::block_on(apply_outgoing(runtime, &frame, PeerRole::Server)) {
        return StepOutcome::Error(e);
    }
    StepOutcome::Event(frame_to_sse_event(&frame), false)
}

fn error_event(err: &ProtocolError) -> Event {
    let frame = Frame::Error {
        code: err.code().to_string(),
        detail: err.to_string(),
    };
    frame_to_sse_event(&frame)
}

#[cfg(test)]
mod tests {
    //! Unit tests cover the stream's **sequence** + **termination**;
    //! the actual W3C SSE wire-byte shape is verified end-to-end in
    //! `tests/sse_fragment_e2e.rs` against a real axum server
    //! (axum's [`Event`] does not expose its inner fields publicly, so
    //! a unit-only "looks like SSE" assertion would have to reach into
    //! private state — the E2E test is the proper place for wire
    //! verification).
    use super::*;
    use axon_frontend::session::SessionType;
    use futures::StreamExt;

    #[tokio::test]
    async fn producer_fragment_stream_emits_one_event_per_step_then_closes() {
        // !A.end — one send, one end ⇒ 2 events.
        let schema = SessionType::send("A", SessionType::End);
        let mut stream = Box::pin(drive_sse_producer(SessionRuntime::new(schema, None)));
        assert!(stream.next().await.expect("first event").is_ok());
        assert!(stream.next().await.expect("second event").is_ok());
        assert!(stream.next().await.is_none(), "stream should close after End");
    }

    #[tokio::test]
    async fn recursive_token_stream_emits_indefinitely() {
        // rec X. !Token.X — the canonical SSE token stream. We pull a
        // bounded prefix to confirm it doesn't terminate early.
        let schema = SessionType::rec(
            "X",
            SessionType::send("Token", SessionType::var("X")),
        );
        let mut stream = Box::pin(drive_sse_producer(SessionRuntime::new(schema, None)));
        for i in 0..16 {
            assert!(
                stream.next().await.expect("token #").is_ok(),
                "token #{i} should arrive"
            );
        }
        // …and it would keep going indefinitely; we cut the test off.
    }

    #[tokio::test]
    async fn non_sse_polarity_short_circuits_with_one_error_event() {
        // !Q.?Ack.end is NOT in the producer fragment (it expects a recv).
        // The driver must emit exactly one event (the preflight error)
        // and close — no partial step, no leaked send.
        let schema = SessionType::send("Q", SessionType::recv("Ack", SessionType::End));
        let mut stream = Box::pin(drive_sse_producer(SessionRuntime::new(schema, None)));
        assert!(stream.next().await.expect("error event").is_ok());
        assert!(stream.next().await.is_none(), "stream should close after preflight error");
    }

    #[tokio::test]
    async fn credit_exhaustion_at_runtime_emits_one_event_then_one_error_then_closes() {
        // !A.!B.end with budget=1 — second send hits n=0 at runtime.
        let schema = SessionType::send("A", SessionType::send("B", SessionType::End));
        let mut stream = Box::pin(drive_sse_producer(SessionRuntime::new(schema, Some(1))));
        assert!(stream.next().await.expect("send A").is_ok());
        assert!(stream.next().await.expect("error after credit exhaustion").is_ok());
        assert!(stream.next().await.is_none(), "stream should close after error");
    }

    #[test]
    fn frame_to_sse_event_is_total_over_the_closed_frame_catalog() {
        // Every variant produces *some* Event — totality of the wire
        // map. (Inner-byte inspection is the E2E's job.)
        let cases = vec![
            Frame::Send { payload_type: "T".into(), data: serde_json::json!(null) },
            Frame::Select { label: "a".into() },
            Frame::End,
            Frame::Error { code: "c".into(), detail: "d".into() },
        ];
        for c in cases {
            let _ = frame_to_sse_event(&c); // doesn't panic
        }
    }
}
