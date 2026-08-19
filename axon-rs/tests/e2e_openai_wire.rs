//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.28.0 — End-to-end OpenAI-dialect wire emission.
//!
//! The cycle's load-bearing test pack: the canonical Kivi-shape
//! (algebraic-effect flow) POSTed against `/v1/<dynamic-route>` MUST
//! emit OpenAI Chat Completions streaming wire bytes verbatim. The
//! adopter's openai-compatible SDK (litellm, langchain, vercel-ai,
//! instructor, llama_index, etc.) consumes the response without any
//! axon-specific awareness — that is the founder's Q1 ratification:
//!
//! > *"los algebraics effects SSE deben funcionar perfectamente y
//! > cumplir la promesa del paper... una primitiva cognitiva que
//! > supera lo que hoy ofrece el mercado en ese sentido."*
//!
//! This test pack pins the wire bytes against the OpenAI spec
//! (https://platform.openai.com/docs/api-reference/chat/streaming).
//!
//! # What this pack pins
//!
//! 1. **section 1 — algebraic-effect flow defaults to openai dialect** (Q1).
//!    POST to a route declared with `effects: <stream:drop_oldest>`
//!    on the applied tool + no explicit `transport: sse(<dialect>)`
//!    grammar emits OpenAI chunks + `data: [DONE]`.
//!
//! 2. **section 2 — explicit `transport: sse(openai)` always emits openai**.
//!    Even type-annotation-only flows can be served via openai wire
//!    when the adopter declares it. Q5 invariant.
//!
//! 3. **section 3 — Q7 axon_metadata frame precedes [DONE]**. The
//!    enforcement_summary / runtime_warnings / step_audit side-
//!    channels reach the adopter via a custom `data: {"axon_metadata":
//!    {...}}` frame emitted BEFORE the OpenAI `[DONE]` sentinel. The
//!    metadata key is unknown to openai-compat SDKs (they pattern-
//!    match on `choices[*].delta`); strict-validating clients ignore
//!    the frame; SDK-free adopters parsing raw SSE consume it directly.
//!
//! 4. **section 4 — no axon-named events leak on the openai wire**. The
//!    dialect adapters are mutually-exclusive — openai NEVER emits
//!    `event: axon.token` / `axon.complete` / `axon.tool_call` /
//!    `axon.error`.

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
        "source_file": "e2e.axon",
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

// ─── section 1 — Algebraic-effect Kivi-shape emits openai wire bytes ───────

#[tokio::test]
async fn kivi_shape_emits_openai_wire_bytes_end_to_end() {
    // The canonical Kivi-shape: tool with `effects: <stream:<policy>>`
    // applied in a step + axonendpoint with NO explicit
    // `transport: sse(<dialect>)` grammar. Q1 ratification: this
    // resolves to the openai dialect default.
    let src = "tool chat_token_stream { description: \"S\" \
               effects: <stream:drop_oldest> }\n\
        flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" apply: chat_token_stream output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat }";
    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);
    let (status, ct, body) = post_no_accept(app, "/chat").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.contains("text/event-stream"));

    // OpenAI Chat Completions streaming spec verbatim per
    // https://platform.openai.com/docs/api-reference/chat/streaming —
    // every frame carries `object: "chat.completion.chunk"`, role-
    // marker on first chunk, content-delta on token chunks, and the
    // final chunk is terminated with `finish_reason: "stop"` followed
    // by the `data: [DONE]` sentinel.
    assert!(
        body.contains("\"object\":\"chat.completion.chunk\""),
        "OpenAI spec: every chunk MUST carry `object: chat.completion.chunk`. \
         Body: {body:?}"
    );
    assert!(
        body.contains("\"delta\":{\"role\":\"assistant\"}"),
        "OpenAI spec: first chunk emits role-marker `delta: {{\"role\": \"assistant\"}}`. \
         Body: {body:?}"
    );
    // This Kivi-shape flow applies a STREAM-PRODUCER tool
    // (`apply: chat_token_stream`, `effects: <stream:drop_oldest>`), so the
    // content-deltas carry the v1.29.0 tool-stream stub chunks
    // (`[stub-stream] <tool>(…)`), NOT the LLM-backend `(stub)` placeholder
    // — the latter is only emitted when the LLM `StubBackend::stream()`
    // drives the tokens (a flow with no stream-producer tool).
    assert!(
        body.contains("[stub-stream] chat_token_stream("),
        "OpenAI spec: content-delta `delta: {{\"content\": \"...\"}}` MUST carry \
         the stream-producer tool's chunk token. Body: {body:?}"
    );
    assert!(
        body.contains("\"finish_reason\":\"stop\""),
        "OpenAI spec: final chunk MUST carry `finish_reason: \"stop\"`. \
         Body: {body:?}"
    );
    assert!(
        body.contains("data: [DONE]"),
        "OpenAI spec: stream MUST terminate with `data: [DONE]` sentinel. \
         Body: {body:?}"
    );
    // section 4 invariant — no axon-named events leak on openai wire.
    assert!(
        !body.contains("event: axon.token"),
        "openai-dialect wire MUST NOT carry W3C `event: axon.token`. Body: {body:?}"
    );
    assert!(
        !body.contains("event: axon.complete"),
        "openai-dialect wire MUST NOT carry W3C `event: axon.complete`. Body: {body:?}"
    );
}

// ─── section 2 — Explicit `transport: sse(openai)` forces openai dialect ───

#[tokio::test]
async fn explicit_transport_sse_openai_emits_openai_wire() {
    // Type-annotation-only flow (no algebraic effect → Q1 default
    // would be axon). Explicit `transport: sse(openai)` overrides
    // the default per the Q5 invariant — adopters can opt INTO
    // openai dialect for any flow.
    let src = "flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse(openai) }";
    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);
    let (status, ct, body) = post_no_accept(app, "/chat").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.contains("text/event-stream"));
    assert!(
        body.contains("\"object\":\"chat.completion.chunk\""),
        "explicit sse(openai): MUST emit OpenAI chunks. Body: {body:?}"
    );
    assert!(
        body.contains("data: [DONE]"),
        "explicit sse(openai): MUST emit `[DONE]` sentinel. Body: {body:?}"
    );
}

// ─── section 3 — Q7 axon_metadata frame precedes the [DONE] sentinel ───────

#[tokio::test]
async fn q7_axon_metadata_frame_emitted_before_done_sentinel() {
    let src = "tool chat_token_stream { description: \"S\" \
               effects: <stream:drop_oldest> }\n\
        flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" apply: chat_token_stream output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/q7\" execute: Chat }";
    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);
    let (_status, _ct, body) = post_no_accept(app, "/q7").await;
    // The axon_metadata frame MUST appear before [DONE] — adopters
    // parsing the stream linearly see the metadata as the last data
    // frame before the terminator sentinel.
    let metadata_pos = body.find("axon_metadata");
    let done_pos = body.find("data: [DONE]");
    assert!(
        metadata_pos.is_some(),
        "Q7: axon_metadata frame MUST be present on openai wire. Body: {body:?}"
    );
    assert!(
        done_pos.is_some(),
        "OpenAI: [DONE] sentinel MUST be present. Body: {body:?}"
    );
    assert!(
        metadata_pos.unwrap() < done_pos.unwrap(),
        "Q7: axon_metadata frame MUST precede [DONE] sentinel \
         (linear-parser invariant). Body: {body:?}"
    );
}

// ─── section 4 — No axon-named events leak on the openai wire ─────────────

#[tokio::test]
async fn openai_wire_has_no_axon_named_events_at_all() {
    let src = "tool chat_token_stream { description: \"S\" \
               effects: <stream:drop_oldest> }\n\
        flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" apply: chat_token_stream output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/mutex\" execute: Chat }";
    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);
    let (_status, _ct, body) = post_no_accept(app, "/mutex").await;
    // The W3C named-event family is exclusive to the axon dialect.
    // Openai dialect MUST NOT interleave them — adapters are
    // mutually-exclusive by closed-catalog construction.
    for needle in &[
        "event: axon.token",
        "event: axon.complete",
        "event: axon.tool_call",
        "event: axon.error",
        "event: axon.start",
        "event: axon.metadata",
    ] {
        assert!(
            !body.contains(needle),
            "openai dialect MUST NOT carry `{needle}` (closed-catalog mutex). \
             Body: {body:?}"
        );
    }
}
