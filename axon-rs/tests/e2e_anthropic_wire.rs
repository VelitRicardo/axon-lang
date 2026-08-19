//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.28.0 — End-to-end Anthropic-dialect wire
//! emission.
//!
//! Adopters whose adopter-SDK ecosystem hard-codes the Anthropic
//! Messages streaming spec
//! (https://docs.anthropic.com/en/api/messages-streaming) declare
//! `transport: sse(anthropic)` on their axonendpoint. The wire bytes
//! match Anthropic's spec verbatim — structured W3C SSE with named
//! events: `message_start` / `content_block_start` /
//! `content_block_delta` / `content_block_stop` / `message_delta` /
//! `message_stop` (terminator). The Q1 default flips to anthropic
//! ONLY when explicitly declared — algebraic-effect default is
//! openai, type-annotation default is axon.
//!
//! # What this pack pins
//!
//! 1. **section 1 — explicit `transport: sse(anthropic)` emits Anthropic
//!    Messages streaming wire**. Every named event present per spec,
//!    in the canonical order.
//!
//! 2. **section 2 — Q7 axon.metadata frame precedes message_stop**.
//!    Anthropic's equivalent of OpenAI's `axon_metadata` — the
//!    side-channel data lands on a `event: axon.metadata` frame
//!    emitted from `flush_terminator()`, BEFORE `event: message_stop`.
//!
//! 3. **section 3 — no openai-style framing leaks on the anthropic wire**.
//!    `data: [DONE]` MUST NOT appear; `chat.completion.chunk` MUST NOT
//!    appear. Closed-catalog mutex per Q3.

use axon::axon_server::{build_router, ServerConfig};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn server_cfg() -> ServerConfig {
    ServerConfig {
        host: "127.0.0.1".into(),
        port: 0,
        channel: "memory".into(),
        auth_token: String::new(),
        log_level: "INFO".into(),
        log_format: "json".into(),
        log_file: None,
        database_url: None,
        config_path: None,
        strict_type_driven_transport: false,
        default_backend: None,
        schemas_dir: None,
    }
}

async fn deploy(app: axum::Router, src: &str) -> StatusCode {
    let body = serde_json::json!({
        "source": src,
        "source_file": "anthropic_e2e.axon",
        "backend": "stub",
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    app.oneshot(req).await.unwrap().status()
}

async fn post_no_accept(app: axum::Router, path: &str) -> (StatusCode, String, String) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let ct = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes).to_string();
    (status, ct, body)
}

// ─── section 1 — Explicit sse(anthropic) emits Anthropic Messages wire ─────

#[tokio::test]
async fn explicit_sse_anthropic_emits_messages_streaming_wire() {
    let src = "flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/anthropic\" execute: Chat transport: sse(anthropic) }";
    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);
    let (status, ct, body) = post_no_accept(app, "/anthropic").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.contains("text/event-stream"));

    // Anthropic Messages streaming spec verbatim:
    // - `message_start` announces msg id + role: assistant
    // - `content_block_start` opens text block (index 0)
    // - `content_block_delta` carries token text_delta
    // - `content_block_stop` closes the text block
    // - `message_delta` reports stop_reason + usage
    // - `message_stop` is the terminator
    for needle in &[
        "event: message_start",
        "event: content_block_start",
        "event: content_block_delta",
        "event: content_block_stop",
        "event: message_delta",
        "event: message_stop",
    ] {
        assert!(
            body.contains(needle),
            "Anthropic spec: wire MUST carry `{needle}`. Body: {body:?}"
        );
    }
    // Content block delta type: text_delta carries the actual token.
    assert!(
        body.contains("\"type\":\"text_delta\""),
        "Anthropic spec: content_block_delta MUST carry `type: \"text_delta\"`. \
         Body: {body:?}"
    );
    assert!(
        body.contains("\"text\":\"(stub)\""),
        "Anthropic spec: text_delta MUST carry token in `text` field. Body: {body:?}"
    );
}

// ─── section 2 — Q7 axon.metadata frame precedes message_stop ──────────────

#[tokio::test]
async fn q7_axon_metadata_frame_precedes_message_stop() {
    let src = "tool chat_token_stream { description: \"S\" \
               effects: <stream:drop_oldest> }\n\
        flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" apply: chat_token_stream output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/anthropic-q7\" execute: Chat transport: sse(anthropic) }";
    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);
    let (_status, _ct, body) = post_no_accept(app, "/anthropic-q7").await;
    let metadata_pos = body.find("event: axon.metadata");
    let stop_pos = body.find("event: message_stop");
    assert!(
        metadata_pos.is_some(),
        "Q7: anthropic wire MUST carry `event: axon.metadata` frame. \
         Body: {body:?}"
    );
    assert!(
        stop_pos.is_some(),
        "Anthropic spec: `event: message_stop` terminator MUST be present. \
         Body: {body:?}"
    );
    assert!(
        metadata_pos.unwrap() < stop_pos.unwrap(),
        "Q7: axon.metadata frame MUST precede message_stop (linear-parser \
         invariant). Body: {body:?}"
    );
}

// ─── section 3 — No openai-style framing leaks on the anthropic wire ──────

#[tokio::test]
async fn anthropic_wire_has_no_openai_style_framing() {
    let src = "flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/anthropic-mutex\" execute: Chat transport: sse(anthropic) }";
    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);
    let (_status, _ct, body) = post_no_accept(app, "/anthropic-mutex").await;
    // Closed-catalog mutex: anthropic wire MUST NOT carry openai
    // sentinels or chunk shape.
    assert!(
        !body.contains("data: [DONE]"),
        "Anthropic dialect MUST NOT carry openai `[DONE]` sentinel. \
         Body: {body:?}"
    );
    assert!(
        !body.contains("\"object\":\"chat.completion.chunk\""),
        "Anthropic dialect MUST NOT carry openai `chat.completion.chunk`. \
         Body: {body:?}"
    );
    assert!(
        !body.contains("event: axon.token"),
        "Anthropic dialect MUST NOT carry axon-W3C `event: axon.token`. \
         Body: {body:?}"
    );
}
