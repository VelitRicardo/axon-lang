//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.31.0 (D4) — the tool registry wired into the production
//! streaming path; declared tool `provider:` executed.
//!
//! `DispatchCtx::with_tool_registry` was DEAD code — zero production
//! callers. `run_streaming_via_dispatcher` attached only the store
//! registry + side-channels, so `ctx.tool_registry` stayed `None`
//! and the dispatcher's streaming-tool branch in `run_step` (gated
//! on `ctx.tool_registry == Some`) could NEVER fire. A
//! `step S { apply: <tool> }` whose `tool` declared
//! `effects: <stream:<policy>>` silently fell through to the
//! plain-LLM path — the declared provider AND the declared stream
//! effect ignored at execution.
//!
//! 36.i wires the registry: `run_streaming_via_dispatcher` builds a
//! `ToolRegistry` from the compiled IR's tools and attaches it. The
//! streaming-tool branch is now LIVE.
//!
//! Pins:
//!   1. A step applying a `provider: stub_stream`,
//!      `effects: <stream:drop_oldest>` tool reaches the streaming
//!      path — the tool's OWN stream wire (`[stub-stream] …`)
//!      appears, NOT the plain-LLM `(stub)` token.
//!   2. The step's `ask:` is the argument handed to the tool.
//!   3. D9 — a step applying a NON-streaming tool keeps the legacy
//!      plain-LLM path (the registry changes nothing for it).

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

async fn deploy(app: &axum::Router, src: &str) {
    let body = serde_json::json!({ "source": src, "source_file": "test.axon" });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(status, StatusCode::OK, "deploy failed: {json}");
    assert_eq!(json.get("success").and_then(|v| v.as_bool()), Some(true), "{json}");
}

/// Hit a dynamic route asking for SSE; return (status, content_type,
/// full body text).
async fn fetch_sse(app: &axum::Router, path: &str) -> (StatusCode, String, String) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, ct, String::from_utf8_lossy(&bytes).into_owned())
}

// ─── section 1 — a streaming tool reaches its own provider's stream ───────

#[tokio::test]
async fn s1_streaming_tool_step_runs_the_tool_not_the_llm() {
    let app = build_router(server_cfg());
    deploy(
        &app,
        "tool MyStream { provider: stub_stream effects: <stream:drop_oldest> }\n\
         flow Chat() -> Unit { step S { ask: \"hi\" apply: MyStream } }\n\
         axonendpoint E { public: true method: POST path: \"/t\" execute: Chat \
         backend: stub transport: sse }",
    )
    .await;

    let (status, ct, body) = fetch_sse(&app, "/t").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"), "SSE, got {ct}");
    assert!(
        body.contains("[stub-stream]"),
        "36.i D4: a `step` applying a `provider: stub_stream`, \
         `effects: <stream:drop_oldest>` tool must reach the \
         dispatcher's streaming-tool branch and execute the TOOL's \
         stream (`[stub-stream] …`). Pre-36.i `ctx.tool_registry` was \
         `None` so the branch was dead and the step fell through to \
         the plain-LLM path. Body: {body}"
    );
}

// ─── section 2 — the step `ask:` is the argument handed to the tool ───────

#[tokio::test]
async fn s2_step_ask_is_the_tool_argument() {
    let app = build_router(server_cfg());
    deploy(
        &app,
        "tool MyStream { provider: stub_stream effects: <stream:drop_oldest> }\n\
         flow Chat() -> Unit { step S { ask: \"axon-marker-42\" apply: MyStream } }\n\
         axonendpoint E { public: true method: POST path: \"/t\" execute: Chat \
         backend: stub transport: sse }",
    )
    .await;

    let (status, _ct, body) = fetch_sse(&app, "/t").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("axon-marker-42"),
        "36.i D4: the step's `ask:` must be the argument passed to \
         the streaming tool. Body: {body}"
    );
}

// ─── section 3 — D9: a non-streaming tool keeps the legacy path ───────────

#[tokio::test]
async fn s3_non_streaming_tool_keeps_the_legacy_path() {
    // `PlainTool` declares no `<stream:…>` effect — `is_streaming`
    // derives false, so the streaming-tool branch does NOT fire and
    // the step keeps the plain-LLM path. Wiring the registry changes
    // nothing for it (D9 backwards-compat).
    let app = build_router(server_cfg());
    deploy(
        &app,
        "tool PlainTool { description: \"a non-streaming tool\" }\n\
         flow Chat() -> Unit { step S { ask: \"hi\" apply: PlainTool } }\n\
         axonendpoint E { public: true method: POST path: \"/t\" execute: Chat \
         backend: stub transport: sse }",
    )
    .await;

    let (status, ct, body) = fetch_sse(&app, "/t").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));
    assert!(
        !body.contains("[stub-stream]"),
        "36.i D9: a non-streaming tool must NOT reach the streaming- \
         tool branch — the legacy plain-LLM path is preserved. Body: {body}"
    );
    assert!(
        body.contains("(stub)"),
        "36.i D9: the non-streaming `apply:` step keeps the canonical \
         stub-LLM wire shape. Body: {body}"
    );
}
