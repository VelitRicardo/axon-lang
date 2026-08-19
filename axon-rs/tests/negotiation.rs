//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.21.0 — Content-negotiation fallback conformance tests.
//!
//! Exercises the decision matrix on `POST /v1/execute`:
//!
//! | axonendpoint.transport | flow has stream-eff | Accept SSE | Response |
//! |---|---|---|---|
//! | "sse" / "ndjson"      | —      | any   | SSE  (D5 force) |
//! | "json" (explicit)     | —      | any   | JSON (D5 force) |
//! | absent                | no     | any   | JSON (default)  |
//! | absent                | yes    | no    | JSON            |
//! | absent                | yes    | yes   | SSE  (D4 nego)  |
//!
//! Hermetic (stub backend, zero network).

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

/// Deploy a flow + return the axum app. The deploy_handler returns
/// HTTP 200 even for parse/typecheck failures (with `success: false`
/// in the body), so we have to inspect the body to confirm true
/// deployment success — otherwise downstream /v1/execute calls
/// return "flow not deployed" and the test misdiagnoses the wrapper.
async fn deploy(
    app: axum::Router,
    source: &str,
) -> axum::Router {
    let body = serde_json::json!({
        "source": source,
        "source_file": "test.axon",
        "backend": "stub",
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "deploy HTTP status");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        json["success"], serde_json::Value::Bool(true),
        "deploy must succeed; got: {json}"
    );
    app
}

/// POST /v1/execute with the given flow name and optional Accept header.
/// Returns (status, content_type, body bytes).
async fn execute_with_accept(
    app: axum::Router,
    flow: &str,
    accept: Option<&str>,
) -> (StatusCode, String, Vec<u8>) {
    let mut req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json");
    if let Some(a) = accept {
        req = req.header("accept", a);
    }
    let body = serde_json::json!({
        "flow": flow,
        "backend": "stub",
    });
    let resp = app
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, ct, bytes)
}

// ──────────────────────────────────────────────────────────────────────
// Row 1: transport: sse declared → SSE regardless of Accept (D5)
// ──────────────────────────────────────────────────────────────────────

// Note on test fixture sources: the Rust frontend has known parser
// gaps vs Python on `output: Stream<T>` and `use <tool>` inside step
// bodies. Those gaps prevent `deploy_handler` from registering the
// flow. The tests therefore use shapes that DO deploy through Rust:
//   - For D5 force-promote: declare a plain flow + axonendpoint with
//     `transport: sse` (axonendpoint parses fine; D5 decision comes
//     from the AST walk of the axonendpoint declaration).
//   - For D4 fallback: declare a tool with `effects: <stream:...>`
//     + a flow. The Rust StepNode can't carry use_tool inside step
//     bodies, but the source-text predicate catches the `stream:`
//     substring (defensive complement; bridges the parser gap).

#[tokio::test]
async fn declared_sse_forces_sse_with_accept_json() {
    let app = build_router(server_cfg());
    let app = deploy(
        app,
        "flow F() { step S { ask: \"hi\" } }\n\
         axonendpoint Live { public: true method: POST path: \"/l\" execute: F transport: sse }\n",
    )
    .await;

    let (status, ct, _body) =
        execute_with_accept(app, "F", Some("application/json")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        ct.starts_with("text/event-stream"),
        "D5: declared transport: sse must force SSE even when client \
         requested JSON — got {ct}"
    );
}

#[tokio::test]
async fn declared_sse_forces_sse_without_accept_header() {
    let app = build_router(server_cfg());
    let app = deploy(
        app,
        "flow F() { step S { ask: \"hi\" } }\n\
         axonendpoint Live { public: true method: POST path: \"/l\" execute: F transport: sse }\n",
    )
    .await;

    let (status, ct, _body) = execute_with_accept(app, "F", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        ct.starts_with("text/event-stream"),
        "D5 force-promote applies even with no Accept header — got {ct}"
    );
}

#[tokio::test]
async fn declared_ndjson_also_forces_streaming_branch() {
    // D2: ndjson is included in the streaming-transport set even
    // though wire emission stays placeholder for a future cycle.
    // For 30.e the negotiation decision is the same as sse.
    let app = build_router(server_cfg());
    let app = deploy(
        app,
        "flow F() { step S { ask: \"hi\" } }\n\
         axonendpoint Live { public: true method: POST path: \"/l\" execute: F transport: ndjson }\n",
    )
    .await;
    let (status, ct, _) = execute_with_accept(app, "F", None).await;
    assert_eq!(status, StatusCode::OK);
    // Implementation maps ndjson → the SSE handler (D2 namespace
    // reserved). What matters is the content-type IS NOT
    // application/json — JSON path was not taken.
    assert!(
        ct.starts_with("text/event-stream") || ct.starts_with("application/x-ndjson"),
        "ndjson decl should route to streaming branch, got {ct}"
    );
}

// ──────────────────────────────────────────────────────────────────────
// Row 2: transport: json explicit → JSON regardless of Accept (D5)
// ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn explicit_json_transport_suppresses_sse_even_with_accept_sse() {
    // Adopter declared `transport: json` — even though source has
    // streaming evidence (a tool with `stream:drop_oldest` effect)
    // AND the client asked for text/event-stream, D5 says explicit
    // declaration wins. Server returns JSON.
    let app = build_router(server_cfg());
    let app = deploy(
        app,
        "tool Streamer { provider: stub effects: <stream:drop_oldest> }\n\
         flow F() { step S { ask: \"hi\" } }\n\
         axonendpoint BatchOnly { public: true method: POST path: \"/b\" execute: F transport: json }\n",
    )
    .await;

    let (status, ct, _body) =
        execute_with_accept(app, "F", Some("text/event-stream")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        ct.starts_with("application/json"),
        "D5: explicit transport: json must suppress auto-promotion — got {ct}"
    );
}

// ──────────────────────────────────────────────────────────────────────
// Rows 3+4: absent transport, flow has no stream-effect → JSON
// ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn non_streaming_flow_with_accept_sse_returns_json() {
    // Flow has neither Stream<T> output nor a tool with stream
    // effect. Even though the client asked for SSE, the server has
    // no stream-effect proof → JSON.
    let app = build_router(server_cfg());
    let app = deploy(
        app,
        "flow F() { step S { ask: \"hi\" } }\n",
    )
    .await;

    let (status, ct, _) =
        execute_with_accept(app, "F", Some("text/event-stream")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        ct.starts_with("application/json"),
        "D4: no stream-effect proof → JSON regardless of Accept — got {ct}"
    );
}

#[tokio::test]
async fn non_streaming_flow_with_no_accept_returns_json() {
    let app = build_router(server_cfg());
    let app = deploy(app, "flow F() { step S { ask: \"hi\" } }\n").await;
    let (status, ct, _) = execute_with_accept(app, "F", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("application/json"));
}

// ──────────────────────────────────────────────────────────────────────
// Row 5: absent transport + flow has stream-effect + Accept SSE → SSE (D4)
// ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn stream_effect_via_tool_effect_with_accept_sse_promotes() {
    // Source contains a tool with `effects: <stream:drop_oldest>`.
    // The defensive source-text predicate detects the streaming
    // evidence even though the Rust AST walk can't see the use_tool
    // (StepNode lacks the field; tracked as a Rust-frontend gap).
    // Client asks for SSE → D4 promotes.
    let app = build_router(server_cfg());
    let app = deploy(
        app,
        "tool Streamer { provider: stub effects: <stream:drop_oldest> }\n\
         flow F() { step S { ask: \"hi\" } }\n",
    )
    .await;

    let (status, ct, _) =
        execute_with_accept(app, "F", Some("text/event-stream")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        ct.starts_with("text/event-stream"),
        "D4 fallback: stream:drop_oldest substring + Accept SSE must \
         enable promotion — got {ct}"
    );
}

#[tokio::test]
async fn stream_effect_via_each_of_4_policies_promotes() {
    // The 4 ratified backpressure policies (v1.4.0) each trigger
    // promotion via the source-text predicate.
    for policy in ["drop_oldest", "degrade_quality", "pause_upstream", "fail"] {
        let app = build_router(server_cfg());
        let src = format!(
            "tool T {{ provider: stub effects: <stream:{policy}> }}\n\
             flow F() {{ step S {{ ask: \"hi\" }} }}\n"
        );
        let app = deploy(app, &src).await;
        let (status, ct, _) =
            execute_with_accept(app, "F", Some("text/event-stream")).await;
        assert_eq!(status, StatusCode::OK, "policy={policy}");
        assert!(
            ct.starts_with("text/event-stream"),
            "D4 promotion failed for policy={policy} — got {ct}"
        );
    }
}

#[tokio::test]
async fn stream_effect_flow_without_accept_stays_json() {
    // D4 requires Accept. Without it, even a streaming flow returns JSON.
    let app = build_router(server_cfg());
    let app = deploy(
        app,
        "tool Streamer { provider: stub effects: <stream:drop_oldest> }\n\
         flow F() { step S { ask: \"hi\" } }\n",
    )
    .await;

    let (status, ct, _) = execute_with_accept(app, "F", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        ct.starts_with("application/json"),
        "D4 fallback requires Accept: text/event-stream — got {ct}"
    );
}

// ──────────────────────────────────────────────────────────────────────
// Backwards-compat — D9: every v1.20.0 client unchanged
// ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn v1_20_0_client_no_accept_header_gets_json_verbatim() {
    let app = build_router(server_cfg());
    let app = deploy(app, "flow F() { step S { ask: \"hi\" } }\n").await;

    // Adopter on v1.20.0 sends no Accept; expects JSON shape with
    // success/trace_id/etc.
    let (status, ct, body) = execute_with_accept(app, "F", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("application/json"));
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    // v2.0.0 wire (v2.0.0): the no-Accept client gets a valid JSON
    // FlowEnvelope body (ψ-vector), which carries `trace_id` rather than
    // the legacy top-level `success` flag.
    assert!(json.get("trace_id").is_some());
}

#[tokio::test]
async fn non_deployed_flow_falls_through_to_legacy_handler() {
    // No deploy → legacy execute_handler surfaces its existing
    // not-deployed error shape unchanged.
    let app = build_router(server_cfg());
    let (status, ct, _body) =
        execute_with_accept(app, "ghost", Some("text/event-stream")).await;
    // Either JSON (legacy not-deployed shape) OR SSE (if the
    // wrapper forwards to the legacy handler unchanged).
    // Critical: NOT a 5xx.
    assert!(
        status == StatusCode::OK || status == StatusCode::NOT_FOUND,
        "got {status}, ct={ct}"
    );
}

// ──────────────────────────────────────────────────────────────────────
// Wire-shape integrity — promoted responses match the 30.d wire format
// ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn promoted_sse_response_carries_retry_directive_and_complete_event() {
    let app = build_router(server_cfg());
    let app = deploy(
        app,
        "flow F() { step S { ask: \"hi\" } }\n\
         axonendpoint Live { public: true method: POST path: \"/l\" execute: F transport: sse }\n",
    )
    .await;

    let (_, ct, bytes) =
        execute_with_accept(app, "F", Some("text/event-stream")).await;
    assert!(ct.starts_with("text/event-stream"));
    let text = String::from_utf8(bytes).expect("utf-8");
    assert!(text.contains("retry: 5000"));
    assert!(text.contains("event: axon.complete"));
}
