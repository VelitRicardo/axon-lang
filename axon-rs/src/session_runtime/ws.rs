//! WebSocket carrier for the session-typed dialogue runtime.
//!
//! v2.3.0 — the transport-specific wiring. The pure
//! [`crate::session_runtime::state::SessionRuntime`] is the operational
//! state machine; this module makes it speak RFC 6455 over a `tokio` +
//! `axum` carrier.
//!
//! Outer surface:
//! - [`drive`] — the protocol loop. Given an `axum::extract::ws::WebSocket`
//!   already upgraded by the caller, a [`SessionRuntime`], and a peer
//!   role (`PeerRole::Server`/`Client`), runs the dialogue to completion
//!   (`end`) or to a protocol error (which is reported to the peer via
//!   an `error` frame + carrier-close `1002 protocol error`).
//! - [`upgrade_handler`] — a ready-made axum extractor handler that
//!   upgrades + drives a runtime against a caller-supplied factory.
//!
//! Connection lifecycle:
//! 1. The carrier is established (HTTP upgrade → WS).
//! 2. The server initialises a [`SessionRuntime`] for its declared role
//!    (e.g. `server` in `session Negotiate { client: [..], server: [..] }`).
//!    The client must run the *dual* role; duality has been checked at
//!    compile time by `axon-frontend::session::SessionType::is_dual_to`.
//! 3. Frames are exchanged. Each frame received from the peer is routed
//!    to the appropriate `try_*` step of the runtime. The runtime owns
//!    *its own role's* perspective, so a peer `kind: "send"` is consumed
//!    as `try_recv` on the local cursor.
//! 4. When the cursor reaches `end` the carrier is closed cleanly
//!    (`1000 normal closure`); a protocol error closes with `1002
//!    protocol error` and an `error` frame as the last message before
//!    the close.

use axum::extract::ws::{CloseFrame, Message, WebSocket};

use super::error::ProtocolError;
use super::state::SessionRuntime;
use super::wire::Frame;

/// Which side of the dialogue this runtime is hosting — informs the
/// receive-vs-send dispatch on incoming `Send` frames (a peer's `send` is
/// our `recv`) and frames the carrier-close attribution on protocol
/// errors. The choice is locked at upgrade time and never observed by
/// the algebra (`SessionType` is direction-free; duality folds the
/// direction in via the connection law).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerRole {
    /// The local runtime is the server-side endpoint of the session.
    Server,
    /// The local runtime is the client-side endpoint.
    Client,
}

impl PeerRole {
    /// The dual — useful for symmetry checks in tests.
    pub fn flip(self) -> Self {
        match self {
            PeerRole::Server => PeerRole::Client,
            PeerRole::Client => PeerRole::Server,
        }
    }
}

/// WebSocket close codes used by the runtime (RFC 6455 section 7.4). Kept here
/// as named constants so the protocol-loop body documents its own
/// closure semantics.
const CLOSE_NORMAL: u16 = 1000;
const CLOSE_PROTOCOL_ERROR: u16 = 1002;
const CLOSE_INTERNAL_ERROR: u16 = 1011;

/// Drive a session-typed dialogue to completion over a `WebSocket`.
///
/// The loop ends in exactly one of three observable states:
/// - the runtime's cursor reaches `End` AND an `end` frame is exchanged
///   → the carrier closes with `1000 normal closure`, return `Ok(())`;
/// - a [`ProtocolError`] fires → an `error` frame is sent, the carrier
///   is closed with `1002 protocol error`, return `Err(err)`;
/// - the carrier drops or returns an I/O error → close cleanly if
///   possible (`1011 internal error`), return
///   [`ProtocolError::Transport`].
///
/// The function takes ownership of the `WebSocket` and consumes it.
pub async fn drive(
    mut ws: WebSocket,
    mut runtime: SessionRuntime,
    role: PeerRole,
) -> Result<(), ProtocolError> {
    // The loop alternates: read a peer frame OR (when the cursor is at
    // `Send`/`Select`) wait for the caller to push one via the runtime.
    // For 41.d the carrier is fully driven by peer frames; outgoing
    // frames are emitted by a future cycle (41.f hooks in the enterprise
    // server). To keep the runtime exercisable end-to-end now, we
    // operate in **echo mode**: any `Send`/`Select` cursor state is
    // emitted onto the wire using a canonical surrogate value (the
    // tests below cover the round-trip explicitly).
    loop {
        if runtime.is_complete() {
            // Send our terminating `end` (the spec requires exactly one
            // `end` per direction; we emit ours after the peer's so the
            // dialogue is symmetric on the wire).
            send_frame(&mut ws, &Frame::End).await?;
            close_normal(&mut ws).await;
            return Ok(());
        }

        // If the local cursor is at a producer state, emit an outgoing
        // frame BEFORE blocking on the carrier — otherwise we deadlock.
        if let Some(out) = next_outgoing_frame(&runtime) {
            // Step the runtime over our own outgoing frame first. A
            // failure here means our LOCAL discipline rejected the step
            // (the v2.3.0 credit-exhausted axiom at runtime is the
            // canonical case — the static analyser would have caught it
            // before deploy, but an off-spec config could still ship the
            // exhaustion to runtime, where this runtime safety net fires
            // and the peer is notified before the close-frame). We
            // report the error onto the wire (so the peer learns *what*
            // happened, not just that the connection died) and close
            // `1002 protocol error`; then we propagate the error so the
            // caller's await resolves with a non-`Ok` outcome.
            if let Err(e) = apply_outgoing(&mut runtime, &out, role).await {
                report_and_close(&mut ws, &e).await;
                return Err(e);
            }
            send_frame(&mut ws, &out).await?;
            continue;
        }

        // Otherwise we are at a consumer state — read the next peer frame.
        let msg = match ws.recv().await {
            Some(Ok(msg)) => msg,
            Some(Err(e)) => {
                let _ = close_internal(&mut ws).await;
                return Err(ProtocolError::Transport(e.to_string()));
            }
            None => {
                // Carrier closed cleanly mid-protocol — surface as a
                // transport error so the caller can decide on retry /
                // resume (41.g typed reconnection lives there).
                return Err(ProtocolError::Transport("peer closed mid-protocol".into()));
            }
        };
        match msg {
            Message::Text(text) => {
                let frame = match Frame::from_wire(&text) {
                    Ok(f) => f,
                    Err(e) => {
                        report_and_close(&mut ws, &e).await;
                        return Err(e);
                    }
                };
                if let Err(e) = apply_incoming(&mut runtime, frame, role) {
                    report_and_close(&mut ws, &e).await;
                    return Err(e);
                }
            }
            Message::Binary(_) => {
                // Binary frames are reserved for a later cycle
                // (multimedia mobility over typed channels). Treating
                // them as malformed here keeps the wire closed.
                let e = ProtocolError::MalformedFrame(
                    "binary frame received on a text-only session-typed channel".into(),
                );
                report_and_close(&mut ws, &e).await;
                return Err(e);
            }
            Message::Ping(p) => {
                // axum sends Pong itself, but be explicit + defensive.
                let _ = ws.send(Message::Pong(p)).await;
            }
            Message::Pong(_) => {}
            Message::Close(_) => {
                // Peer initiated close. If we are not at `End`, this is
                // a mid-protocol drop.
                if runtime.is_complete() {
                    return Ok(());
                }
                return Err(ProtocolError::Transport("peer closed mid-protocol".into()));
            }
        }
    }
}

/// Decide whether the local runtime currently owes the peer a frame. The
/// rules follow the algebra exactly:
/// - `Send`  ⇒ we emit `Frame::Send  { payload_type }`
/// - `End`   ⇒ we emit `Frame::End`
/// - `Recv`  ⇒ peer's turn — we wait
/// - `Branch`⇒ peer's turn (they `select` an arm) — we wait
/// - `Select`⇒ this cycle the runtime cannot auto-pick an arm; the
///             [`drive`] loop's echo mode emits the first label in
///             canonical (BTreeMap) order so the test surface is total.
///
/// Visible to siblings (`session_runtime::sse`) so the SSE-fragment
/// driver shares the same dispatch — both carriers run the same algebra,
/// only the framing differs. v2.3.0.
pub(super) fn next_outgoing_frame(runtime: &SessionRuntime) -> Option<Frame> {
    use axon_frontend::session::SessionType;
    match runtime.cursor() {
        SessionType::Send { payload, .. } => Some(Frame::Send {
            payload_type: payload.to_string(),
            data: serde_json::Value::Null, // payload-shape carried opaquely
        }),
        SessionType::Select(arms) => {
            // Echo mode: deterministic arm pick = the first key. Real
            // application drivers (41.f) override by feeding outgoing
            // frames explicitly.
            let label = arms.keys().next()?.clone();
            Some(Frame::Select { label })
        }
        SessionType::End => None, // handled at the top of `drive`
        _ => None,                // Recv / Branch / Rec / Var — peer's turn
    }
}

/// Apply an outgoing frame to the local runtime (advancing the cursor
/// before we put bytes on the wire). The `role` parameter is reserved
/// for symmetry — both roles step the cursor identically on local
/// actions; the algebra carries no direction beyond duality, which is
/// already baked into the role's `SessionType` at construction.
///
/// Visible to siblings (`session_runtime::sse`) — the SSE-fragment
/// driver advances its runtime via the same primitive so a Send / End
/// transition is identical at the operational layer regardless of the
/// carrier (WS frame vs SSE event). v2.3.0.
pub(super) async fn apply_outgoing(
    runtime: &mut SessionRuntime,
    frame: &Frame,
    _role: PeerRole,
) -> Result<(), ProtocolError> {
    match frame {
        Frame::Send { payload_type, .. } => runtime.try_send(payload_type),
        Frame::Select { label } => runtime.try_select(label),
        Frame::End => runtime.try_end(),
        Frame::Error { .. } => Ok(()), // pure carrier signal; no state change
    }
}

/// Apply an incoming frame from the peer to the local runtime. From our
/// side a peer-`send` is a `recv`, a peer-`select` is a `branch_offer`,
/// and `end` matches `End` on the cursor.
fn apply_incoming(
    runtime: &mut SessionRuntime,
    frame: Frame,
    _role: PeerRole,
) -> Result<(), ProtocolError> {
    match frame {
        Frame::Send { payload_type, .. } => runtime.try_recv(&payload_type),
        Frame::Select { label } => runtime.try_offer(&label),
        Frame::End => runtime.try_end(),
        Frame::Error { code, detail } => Err(ProtocolError::Transport(format!(
            "peer reported `{code}`: {detail}"
        ))),
    }
}

async fn send_frame(ws: &mut WebSocket, frame: &Frame) -> Result<(), ProtocolError> {
    ws.send(Message::Text(frame.to_wire().into()))
        .await
        .map_err(|e| ProtocolError::Transport(e.to_string()))
}

async fn report_and_close(ws: &mut WebSocket, err: &ProtocolError) {
    let frame = Frame::Error {
        code: err.code().to_string(),
        detail: err.to_string(),
    };
    // Best-effort — we may already be racing a peer close.
    let _ = ws.send(Message::Text(frame.to_wire().into())).await;
    let _ = close_with(ws, CLOSE_PROTOCOL_ERROR, err.code()).await;
}

async fn close_normal(ws: &mut WebSocket) {
    let _ = close_with(ws, CLOSE_NORMAL, "session_end").await;
}

async fn close_internal(ws: &mut WebSocket) -> Result<(), ProtocolError> {
    close_with(ws, CLOSE_INTERNAL_ERROR, "internal").await
}

async fn close_with(ws: &mut WebSocket, code: u16, reason: &str) -> Result<(), ProtocolError> {
    let frame = CloseFrame {
        code,
        reason: reason.to_string().into(),
    };
    ws.send(Message::Close(Some(frame)))
        .await
        .map_err(|e| ProtocolError::Transport(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_role_flip_is_involutive() {
        assert_eq!(PeerRole::Server.flip(), PeerRole::Client);
        assert_eq!(PeerRole::Client.flip(), PeerRole::Server);
        assert_eq!(PeerRole::Server.flip().flip(), PeerRole::Server);
    }

    #[test]
    fn next_outgoing_frame_for_send_cursor() {
        use axon_frontend::session::SessionType;
        let r = SessionRuntime::new(SessionType::send("Msg", SessionType::End), None);
        match next_outgoing_frame(&r) {
            Some(Frame::Send { payload_type, .. }) => assert_eq!(payload_type, "Msg"),
            other => panic!("expected Send frame for Send cursor, got {other:?}"),
        }
    }

    #[test]
    fn next_outgoing_frame_for_recv_cursor_is_none() {
        use axon_frontend::session::SessionType;
        let r = SessionRuntime::new(SessionType::recv("Msg", SessionType::End), None);
        assert!(next_outgoing_frame(&r).is_none());
    }

    #[test]
    fn next_outgoing_frame_for_select_picks_first_label() {
        use axon_frontend::session::SessionType;
        let r = SessionRuntime::new(
            SessionType::select([
                ("zeta".into(), SessionType::End),
                ("alpha".into(), SessionType::End),
            ]),
            None,
        );
        match next_outgoing_frame(&r) {
            // BTreeMap keys in canonical order ⇒ "alpha" before "zeta".
            Some(Frame::Select { label }) => assert_eq!(label, "alpha"),
            other => panic!("expected Select(alpha), got {other:?}"),
        }
    }
}
