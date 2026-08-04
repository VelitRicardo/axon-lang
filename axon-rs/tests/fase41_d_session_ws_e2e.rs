//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 41.d — End-to-end test of the session-typed WebSocket runtime.
//!
//! Spins up a real axum server on a TCP-bound port, upgrades a connection
//! to WebSocket via `axum::extract::ws::WebSocketUpgrade`, hands the
//! upgraded socket to [`axon::session_runtime::drive`] with a configured
//! [`SessionRuntime`], and exchanges frames from a `tokio-tungstenite`
//! client. The four scenarios cover the four behaviours that 41.d must
//! enforce on a live carrier:
//!
//! 1. **Happy path** — a well-formed dual dialogue runs to completion;
//!    the carrier closes cleanly with `1000 normal closure`.
//! 2. **Payload mismatch** — the client sends a frame whose declared
//!    payload type diverges from the server cursor; the server emits an
//!    `error` frame with code `payload_mismatch` and closes `1002`.
//! 3. **Unexpected frame kind** — the client sends `select` where the
//!    server expects `recv` (`Send`); the server rejects with
//!    `unexpected_frame`.
//! 4. **Credit exhaustion** — the protocol allows two sends but the
//!    server runtime is initialised with `budget = 1`; the second send
//!    triggers the §41.c "no rule at n=0" axiom at runtime
//!    (`credit_exhausted`).

use std::time::Duration;

use axum::{
    extract::WebSocketUpgrade,
    response::Response,
    routing::any,
    Router,
};
use futures::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::{frame::coding::CloseCode, Message as TgMessage},
};

use axon::session_runtime::{drive, Frame, PeerRole, SessionRuntime};
use axon_frontend::session::SessionType;

// ─── Test scaffolding ──────────────────────────────────────────────────────

/// Spawn an axum server with one WS route on an OS-assigned ephemeral
/// port. Returns the `ws://…/ws` URL the client should dial. The server
/// is `tokio::spawn`-ed; the test panics on bind failure so a missing
/// loopback or a fluke port-exhaustion is loud, not silent.
async fn spawn_server<F>(make_runtime: F, budget: Option<u64>) -> String
where
    F: Fn() -> SessionType + Send + Sync + 'static,
{
    let make_runtime = std::sync::Arc::new(make_runtime);
    let app = Router::new().route(
        "/ws",
        any(move |ws: WebSocketUpgrade| {
            let mk = make_runtime.clone();
            async move {
                let schema = (mk)();
                let runtime = SessionRuntime::new(schema, budget);
                let resp: Response = ws.on_upgrade(move |socket| async move {
                    // The driver swallows the result — we observe success
                    // and failure via the wire (close code + error frame).
                    let _ = drive(socket, runtime, PeerRole::Server).await;
                });
                resp
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ephemeral");
    let port = listener.local_addr().expect("local_addr").port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    // Give the server a brief moment to start listening. axum::serve is
    // ready synchronously after bind, but on Windows the accept queue
    // sometimes needs a tick — keep this tight.
    tokio::time::sleep(Duration::from_millis(20)).await;
    format!("ws://127.0.0.1:{port}/ws")
}

/// Read the next text frame from the carrier and parse it as a `Frame`.
async fn next_frame<S>(ws: &mut S) -> Frame
where
    S: StreamExt<Item = Result<TgMessage, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let msg = match tokio::time::timeout(Duration::from_secs(2), ws.next()).await {
        Ok(Some(Ok(m))) => m,
        Ok(Some(Err(e))) => panic!("ws recv error: {e}"),
        Ok(None) => panic!("ws closed before frame arrived"),
        Err(_) => panic!("ws recv timeout"),
    };
    match msg {
        TgMessage::Text(t) => Frame::from_wire(&t).expect("frame parses"),
        TgMessage::Close(c) => panic!("got close before frame: {c:?}"),
        other => panic!("expected text frame, got: {other:?}"),
    }
}

/// Pull the next message, expecting a Close — return the (code, reason).
async fn expect_close<S>(ws: &mut S) -> (u16, String)
where
    S: StreamExt<Item = Result<TgMessage, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let msg = match tokio::time::timeout(Duration::from_secs(2), ws.next()).await {
            Ok(Some(Ok(m))) => m,
            Ok(Some(Err(e))) => panic!("ws recv error: {e}"),
            Ok(None) => panic!("ws closed silently — no close frame"),
            Err(_) => panic!("ws recv timeout"),
        };
        if let TgMessage::Close(Some(cf)) = msg {
            return (cf.code.into(), cf.reason.to_string());
        }
        if let TgMessage::Close(None) = msg {
            return (1005, String::new());
        }
        // Any non-close frame here means the server is still on a step
        // before closing; let the test peel them off explicitly with
        // `next_frame`. If we are inside `expect_close`, the caller has
        // declared "I'm done" — fail loud.
        panic!("expected close, got non-close frame: {msg:?}");
    }
}

async fn send_frame<S>(ws: &mut S, f: Frame)
where
    S: SinkExt<TgMessage, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    // §Fase 117.c — tungstenite 0.26+ carries text payloads as `Utf8Bytes`.
    ws.send(TgMessage::Text(f.to_wire().into())).await.expect("send");
}

// ─── 1. Happy path — well-formed dialogue runs to completion ──────────────

#[tokio::test]
async fn happy_path_runs_to_completion_and_closes_normally() {
    // Protocol (server side): ?Msg.!Ack.end. The client is the dual:
    // !Msg.?Ack.end. We drive the client by hand.
    let url = spawn_server(
        || SessionType::recv("Msg", SessionType::send("Ack", SessionType::End)),
        None,
    )
    .await;
    let (mut ws, _resp) = connect_async(&url).await.expect("connect");

    // Client sends `Msg` → server consumes (try_recv("Msg")).
    send_frame(
        &mut ws,
        Frame::Send {
            payload_type: "Msg".into(),
            data: serde_json::Value::Null,
        },
    )
    .await;

    // Server's cursor is now `!Ack.end` — it emits `Send Ack` on its turn.
    let ack = next_frame(&mut ws).await;
    match ack {
        Frame::Send { payload_type, .. } => assert_eq!(payload_type, "Ack"),
        other => panic!("expected Send(Ack), got {other:?}"),
    }

    // Server's cursor is now `End` — it emits the terminating `End` frame
    // before closing the carrier.
    assert_eq!(next_frame(&mut ws).await, Frame::End);

    // Carrier closes cleanly with code 1000 (normal closure).
    let (code, reason) = expect_close(&mut ws).await;
    assert_eq!(code, u16::from(CloseCode::Normal));
    assert_eq!(reason, "session_end");
}

// ─── 2. Payload mismatch is rejected with close 1002 ──────────────────────

#[tokio::test]
async fn payload_mismatch_is_rejected_with_protocol_error() {
    let url = spawn_server(
        || SessionType::recv("Msg", SessionType::send("Ack", SessionType::End)),
        None,
    )
    .await;
    let (mut ws, _resp) = connect_async(&url).await.expect("connect");

    // Client sends a Send frame with a payload type the server's cursor
    // does NOT expect (`Msg` is required, `Bogus` arrives instead).
    send_frame(
        &mut ws,
        Frame::Send {
            payload_type: "Bogus".into(),
            data: serde_json::Value::Null,
        },
    )
    .await;

    // Server sends a typed `Error` frame announcing the violation.
    let err = next_frame(&mut ws).await;
    match err {
        Frame::Error { code, detail } => {
            assert_eq!(code, "payload_mismatch");
            assert!(detail.contains("Msg"), "detail must name the expected payload: {detail}");
            assert!(detail.contains("Bogus"), "detail must name the offending payload: {detail}");
        }
        other => panic!("expected Error frame, got {other:?}"),
    }

    // …then closes the carrier with 1002 protocol error.
    let (code, reason) = expect_close(&mut ws).await;
    assert_eq!(code, u16::from(CloseCode::Protocol));
    assert_eq!(reason, "payload_mismatch");
}

// ─── 3. Unexpected frame kind is rejected ─────────────────────────────────

#[tokio::test]
async fn unexpected_frame_kind_is_rejected() {
    let url = spawn_server(
        || SessionType::recv("Msg", SessionType::send("Ack", SessionType::End)),
        None,
    )
    .await;
    let (mut ws, _resp) = connect_async(&url).await.expect("connect");

    // Cursor is `?Msg.…` — but the client sends a `select` (a choice
    // step). There is no transition for `select` on a `Recv` cursor.
    send_frame(&mut ws, Frame::Select { label: "ask".into() }).await;

    let err = next_frame(&mut ws).await;
    match err {
        Frame::Error { code, .. } => assert_eq!(code, "unexpected_frame"),
        other => panic!("expected Error, got {other:?}"),
    }
    let (code, reason) = expect_close(&mut ws).await;
    assert_eq!(code, u16::from(CloseCode::Protocol));
    assert_eq!(reason, "unexpected_frame");
}

// ─── 4. Credit exhaustion — runtime witness of the §41.c n=0 axiom ────────

#[tokio::test]
async fn credit_exhaustion_is_rejected_at_runtime() {
    // Server schema accepts two sends, but the runtime is initialised
    // with `budget = 1` — the second `recv` (peer-`send`) cannot refill
    // because there is no intervening recv on the server side. So the
    // server's `!Ack` step would fail credit-exhausted, but we want to
    // test the *consumer* side. Setup: server side is `!A.!B.end` with
    // budget = 1; the second send the SERVER makes hits n=0.
    let url = spawn_server(
        || SessionType::send("A", SessionType::send("B", SessionType::End)),
        Some(1),
    )
    .await;
    let (mut ws, _resp) = connect_async(&url).await.expect("connect");

    // First server-emitted Send consumes the credit (budget → 0). Wire
    // delivers it to us.
    let first = next_frame(&mut ws).await;
    assert!(matches!(first, Frame::Send { ref payload_type, .. } if payload_type == "A"));

    // Server's NEXT step is `!B`, but budget = 0 → CreditExhausted; the
    // server should emit an Error frame instead of the `B` send.
    let err = next_frame(&mut ws).await;
    match err {
        Frame::Error { code, detail } => {
            assert_eq!(code, "credit_exhausted");
            assert!(detail.contains("B"), "detail names the blocked payload: {detail}");
            assert!(detail.contains("budget = 1"), "detail names the budget: {detail}");
        }
        other => panic!("expected Error(credit_exhausted), got {other:?}"),
    }
    let (code, reason) = expect_close(&mut ws).await;
    assert_eq!(code, u16::from(CloseCode::Protocol));
    assert_eq!(reason, "credit_exhausted");
}
