//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.28.0 — End-to-end axon-dialect byte-compat
//! anchor.
//!
//! The D6 + Q5 invariant: the axon W3C named-events dialect remains
//! a first-class option indefinitely. Three paths reach the axon
//! dialect wire:
//!
//!   1. Type-annotation-only flow + bare `transport: sse` → Q1 Rule 3
//!      defaults to axon dialect.
//!   2. Any flow + explicit `transport: sse(axon)` → Q5 escape valve;
//!      explicit dialect wins.
//!   3. Algebraic-effect flow + explicit `transport: sse(axon)` →
//!      same as path 2; algebraic-effect default flip is overridden
//!      by explicit dialect choice.
//!
//! All three paths MUST emit byte-identical axon wire surface. This
//! test pack pins the equivalence — adopters on the axon dialect see
//! the same `event: axon.token` / `event: axon.complete` shape
//! regardless of how they reached the dialect.

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
        "source_file": "axon_byte_compat.axon",
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

/// Strip the volatile fields (timestamp_ms + latency_ms) from the
/// SSE body so equivalence assertions compare semantic shape, not
/// wall-clock noise.
fn strip_volatile(body: &str) -> String {
    let mut out = String::new();
    for line in body.lines() {
        if let Some(data_pos) = line.find("data: {") {
            let prefix = &line[..data_pos + 6];
            let payload = &line[data_pos + 6..];
            if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(payload) {
                if let Some(obj) = v.as_object_mut() {
                    obj.remove("timestamp_ms");
                    obj.remove("latency_ms");
                }
                out.push_str(prefix);
                out.push_str(&serde_json::to_string(&v).unwrap_or_default());
            } else {
                out.push_str(line);
            }
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

// ─── section 1 — Bare `transport: sse` + type-annotation only → axon ──────

#[tokio::test]
async fn bare_sse_type_annotation_only_resolves_to_axon_dialect() {
    let src = "flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/bare\" execute: Chat transport: sse }";
    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);
    let (status, ct, body) = post_no_accept(app, "/bare").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.contains("text/event-stream"));
    assert!(
        body.contains("event: axon.token"),
        "Q1 Rule 3: type-annotation-only + bare sse → axon dialect; \
         wire MUST carry `event: axon.token`. Body: {body:?}"
    );
    assert!(
        body.contains("event: axon.complete"),
        "axon dialect: terminator is `event: axon.complete` (in-line). \
         Body: {body:?}"
    );
    // Closed-catalog mutex: no openai sentinel.
    assert!(!body.contains("data: [DONE]"));
    assert!(!body.contains("\"object\":\"chat.completion.chunk\""));
}

// ─── section 2 — Explicit `transport: sse(axon)` produces identical wire ──

#[tokio::test]
async fn bare_sse_and_explicit_sse_axon_emit_byte_identical_wire() {
    // Same flow + only the `transport: sse` vs `transport: sse(axon)`
    // grammar differs. The wire body MUST be byte-identical modulo
    // volatile timestamp_ms / latency_ms fields.
    let bare_src = "flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/a\" execute: Chat transport: sse }";
    let explicit_src = "flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/b\" execute: Chat transport: sse(axon) }";
    let app_bare = build_router(server_cfg());
    assert_eq!(deploy(app_bare.clone(), bare_src).await, StatusCode::OK);
    let (_, _, body_bare) = post_no_accept(app_bare, "/a").await;
    let app_explicit = build_router(server_cfg());
    assert_eq!(deploy(app_explicit.clone(), explicit_src).await, StatusCode::OK);
    let (_, _, body_explicit) = post_no_accept(app_explicit, "/b").await;
    let normalized_bare = strip_volatile(&body_bare);
    let normalized_explicit = strip_volatile(&body_explicit);
    assert_eq!(
        normalized_bare, normalized_explicit,
        "D6 + Q5 invariant: bare `transport: sse` (Q1 default to axon) \
         and explicit `transport: sse(axon)` MUST emit byte-identical \
         wire modulo volatile timing fields.\n\
         bare:\n{normalized_bare}\n\
         explicit:\n{normalized_explicit}"
    );
}

// ─── section 3 — Algebraic-effect + explicit sse(axon) overrides Q1 default

#[tokio::test]
async fn algebraic_effect_with_explicit_sse_axon_overrides_openai_default() {
    // Algebraic-effect flow would default to openai per Q1, but
    // explicit `transport: sse(axon)` MUST override (Q5 escape valve).
    let src = "tool chat_token_stream { description: \"S\" \
               effects: <stream:drop_oldest> }\n\
        flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" apply: chat_token_stream output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/override\" execute: Chat transport: sse(axon) }";
    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);
    let (status, ct, body) = post_no_accept(app, "/override").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.contains("text/event-stream"));
    // Q5 invariant: explicit dialect choice always wins.
    assert!(
        body.contains("event: axon.token"),
        "Q5: explicit `sse(axon)` overrides Q1's openai default for \
         algebraic-effect flows. Body: {body:?}"
    );
    assert!(
        body.contains("event: axon.complete"),
        "Q5 + D6: explicit axon dialect emits in-line `axon.complete` \
         terminator. Body: {body:?}"
    );
    // Closed-catalog mutex: no openai shape on explicit-axon wire.
    assert!(
        !body.contains("data: [DONE]"),
        "Q5: explicit sse(axon) MUST NOT emit openai sentinel. Body: {body:?}"
    );
    assert!(
        !body.contains("\"object\":\"chat.completion.chunk\""),
        "Q5: explicit sse(axon) MUST NOT emit openai chunk shape. Body: {body:?}"
    );
}
