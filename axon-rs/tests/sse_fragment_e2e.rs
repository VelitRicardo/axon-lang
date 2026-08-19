//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v2.3.0 — End-to-end test of the SSE-as-fragment unification.
//!
//! Spins up a real axum server that returns a Server-Sent-Events
//! response driven by [`axon::session_runtime::drive_sse_producer`],
//! and reads the wire bytes from an HTTP client. The four scenarios
//! verify the section 4.4 identity `S_SSE = Π↓(S_WS)` is honoured on the wire:
//!
//! 1. **W3C SSE framing** — the response Content-Type is
//!    `text/event-stream`, every event uses `event:…\ndata:…\n\n`. Any
//!    standards-compliant SSE consumer (browsers' `EventSource`,
//! `curl --no-buffer`, the v1.24.0 `bytes_stream_to_sse_events`
//!    parser) decodes the wire without axon-specific knowledge.
//! 2. **Session-typed payloads** — each `axon.send` event carries the
//!    JSON `{"payload_type":…,"data":…}` envelope; each `axon.select`
//!    carries `{"label":…}`; the terminating event is `axon.end`.
//! 3. **Polarity preflight** — a non-SSE-projectable schema (e.g.
//!    `!Q.?Ack.end`) yields exactly one `axon.error` event and the
//!    stream closes.
//! 4. **Credit exhaustion at runtime** — the v2.3.0 `n=0` axiom fires
//!    on the SSE carrier just as on the WebSocket carrier: the producer
//!    emits its sustainable prefix, then a single `axon.error` event
//!    with `code: "credit_exhausted"`, then closes.
//!
//! These four together demonstrate D3: the same operational state
//! machine runs over either carrier without divergence, with W3C SSE
//! framing identical to what v1.24.0's existing pipeline emits.

use std::convert::Infallible;
use std::time::Duration;

use axum::{
    response::{
        sse::{Event, Sse},
        IntoResponse,
    },
    routing::get,
    Router,
};
use futures::stream::Stream;
use tokio::net::TcpListener;

use axon::session_runtime::{drive_sse_producer, SessionRuntime};
use axon_frontend::session::SessionType;

// ─── Test scaffolding ────────────────────────────────────────────────────

/// Spawn an axum server with one SSE route on an ephemeral port. The
/// route is `GET /sse` and emits whatever `make_runtime` produces,
/// SSE-driven. Returns the full URL.
async fn spawn_server<F>(make_runtime: F, budget: Option<u64>) -> String
where
    F: Fn() -> SessionType + Send + Sync + 'static,
{
    let make_runtime = std::sync::Arc::new(make_runtime);
    let app = Router::new().route(
        "/sse",
        get(move || {
            let mk = make_runtime.clone();
            async move {
                let schema = (mk)();
                let runtime = SessionRuntime::new(schema, budget);
                sse_response(drive_sse_producer(runtime))
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ephemeral");
    let port = listener.local_addr().expect("local_addr").port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    format!("http://127.0.0.1:{port}/sse")
}

fn sse_response<S>(stream: S) -> impl IntoResponse
where
    S: Stream<Item = Result<Event, Infallible>> + Send + 'static,
{
    Sse::new(stream)
}

/// Pull the full SSE body as text. The carrier closes on its own (the
/// SSE-driver's stream ends after `axon.end` or `axon.error`), so a
/// blocking read is total.
async fn fetch_sse(url: &str) -> (String, String) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .expect("client");
    let resp = client
        .get(url)
        .header("Accept", "text/event-stream")
        .send()
        .await
        .expect("send GET");
    let ctype = resp
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap_or("").to_string())
        .unwrap_or_default();
    let body = resp.text().await.expect("body");
    (ctype, body)
}

/// Split an SSE response body into events. Each event is one
/// `event:`/`data:` block separated by a blank line. Tolerant of
/// `\r\n` vs `\n` line endings (RFC 6455 section 6.1 allows both); the
/// parser used in v1.24.0 (`bytes_stream_to_sse_events`) is the
/// production-grade analogue — here we keep the test self-contained.
fn parse_sse_events(body: &str) -> Vec<(String, String)> {
    let normalised = body.replace("\r\n", "\n");
    let mut events = Vec::new();
    for block in normalised.split("\n\n") {
        let mut name = String::new();
        let mut data = String::new();
        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("event:") {
                name = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("event: ") {
                name = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest.trim_start_matches(' '));
            }
        }
        if !name.is_empty() {
            events.push((name, data));
        }
    }
    events
}

// ─── 1. W3C SSE framing — happy path ─────────────────────────────────────

#[tokio::test]
async fn sse_producer_emits_w3c_compliant_framing_with_content_type() {
    // Schema: !A.!B.end — two sends, one end → exactly 3 events.
    let url = spawn_server(
        || SessionType::send("A", SessionType::send("B", SessionType::End)),
        None,
    )
    .await;
    let (ctype, body) = fetch_sse(&url).await;
    // W3C SSE Content-Type — the carrier identifier any consumer keys off.
    assert!(
        ctype.starts_with("text/event-stream"),
        "Content-Type must be text/event-stream, got: {ctype}"
    );
    let events = parse_sse_events(&body);
    let names: Vec<&str> = events.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["axon.send", "axon.send", "axon.end"]);
    // The wire raw form respects the SSE event-terminator (`\n\n`).
    assert!(
        body.contains("\n\n") || body.contains("\r\n\r\n"),
        "events must be separated by a blank line: {body:?}"
    );
}

// ─── 2. Session-typed payloads inside the data: lines ────────────────────

#[tokio::test]
async fn session_typed_payloads_ride_inside_sse_data_field() {
    let url = spawn_server(
        || SessionType::send("Token", SessionType::send("Token", SessionType::End)),
        None,
    )
    .await;
    let (_, body) = fetch_sse(&url).await;
    let events = parse_sse_events(&body);
    // Two send events both carrying the `Token` payload type.
    let sends: Vec<&(String, String)> = events.iter().filter(|(n, _)| n == "axon.send").collect();
    assert_eq!(sends.len(), 2, "expected 2 axon.send events: {events:?}");
    for (_, data) in &sends {
        // The data: line is the JSON envelope — payload_type is `Token`.
        let v: serde_json::Value = serde_json::from_str(data).expect("data is JSON");
        assert_eq!(v["payload_type"], "Token");
        // data is opaque here (`null` in the producer-emitted echo), but
        // its presence as a key is part of the section 4.4 envelope contract.
        assert!(v.get("data").is_some(), "envelope must carry `data`: {v}");
    }
}

// ─── 3. Polarity preflight: non-SSE schema short-circuits ────────────────

#[tokio::test]
async fn non_sse_polarity_schema_returns_one_error_event_then_closes() {
    // !Q.?Ack.end is NOT in the producer fragment (it expects a recv).
    let url = spawn_server(
        || SessionType::send("Q", SessionType::recv("Ack", SessionType::End)),
        None,
    )
    .await;
    let (_, body) = fetch_sse(&url).await;
    let events = parse_sse_events(&body);
    assert_eq!(events.len(), 1, "exactly one event for preflight reject: {events:?}");
    let (name, data) = &events[0];
    assert_eq!(name, "axon.error");
    let v: serde_json::Value = serde_json::from_str(data).expect("data is JSON");
    // The error envelope mirrors the WebSocket carrier's Frame::Error
    // shape: `code` + `detail`. (D3 byte-compat between the carriers.)
    assert!(v.get("code").is_some(), "error envelope carries `code`: {v}");
    assert!(v.get("detail").is_some(), "error envelope carries `detail`: {v}");
}

// ─── 4. Credit exhaustion at runtime — the v2.3.0 axiom on SSE ────────────

#[tokio::test]
async fn credit_exhaustion_emits_send_then_error_then_closes() {
    // !A.!B.end with budget=1 — the second send hits n=0.
    let url = spawn_server(
        || SessionType::send("A", SessionType::send("B", SessionType::End)),
        Some(1),
    )
    .await;
    let (_, body) = fetch_sse(&url).await;
    let events = parse_sse_events(&body);
    let names: Vec<&str> = events.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        vec!["axon.send", "axon.error"],
        "expected sustainable prefix then runtime error: {events:?}"
    );
    let (_, err_data) = &events[1];
    let v: serde_json::Value = serde_json::from_str(err_data).expect("error data is JSON");
    assert_eq!(v["code"], "credit_exhausted");
    assert!(
        v["detail"].as_str().unwrap_or("").contains("budget = 1"),
        "detail names the budget: {v}"
    );
}
