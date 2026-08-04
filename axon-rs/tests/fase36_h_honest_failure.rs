//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 36.h (D5) — no silent stub, honest failure.
//!
//! The Backend Resolution Contract is total: when every ladder rung
//! is empty `resolve_route_backend` returns `Err(NoBackendAvailable)`.
//! 36.h closes the last seam — the `Err` arm of `dynamic_endpoint_
//! handler` no longer falls back to `"auto"` (which dead-ended at the
//! no-op `stub`). It now fails LOUDLY with a structured diagnostic:
//!
//!   - JSON route → HTTP 503 + `{error: "no_backend_available", …}`.
//!   - SSE route  → HTTP 200 `text/event-stream` + a dialect-correct
//!     `axon.error` event.
//!
//! `stub` remains reachable ONLY by an explicit, written
//! `backend: stub` — D5 forbids a SILENT degradation, not an opt-in.
//!
//! Pins:
//!   1. `honest_backend_failure_response` — JSON wire → 503 with the
//!      structured body; the `NoBackendAvailable` message names every
//!      fix.
//!   2. …SSE wire → 200 `text/event-stream` with a dialect-correct
//!      error event (axon → `axon.error`; openai → `[DONE]`).
//!   3. End-to-end: an undeclared route on a server with nothing to
//!      resolve fails with the structured 503 (env-key-free CI).
//!   4. A route that explicitly declares `backend: stub` NEVER hits
//!      the honest-failure path — explicit opt-in is legal (D5).

use axon::axon_server::{
    build_router, collect_axonendpoint_routes, honest_backend_failure_response,
    DynamicEndpointRoute, DynamicRouteWire, ServerConfig,
};
use axon::backend_resolution::NoBackendAvailable;
use axon::backends::env_available_backends;
use axon::type_checker::compute_implicit_transports;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use std::collections::HashMap;
use tower::ServiceExt;

fn one_route(src: &str) -> DynamicEndpointRoute {
    let tokens = axon::lexer::Lexer::new(src, "<test>").tokenize().unwrap();
    let mut prog = axon::parser::Parser::new(tokens).parse().unwrap();
    compute_implicit_transports(&mut prog);
    let table: HashMap<_, _> = collect_axonendpoint_routes(&prog, src, "<test>").unwrap();
    table.into_values().next().expect("one route expected")
}

const ROUTE_SRC: &str = "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
    axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat }";

// ─── §1 — JSON honest failure is a structured 503 ──────────────────

#[tokio::test]
async fn s1_json_honest_failure_is_a_structured_503() {
    let route = one_route(ROUTE_SRC);
    let resp = honest_backend_failure_response(
        DynamicRouteWire::Json,
        &route,
        &NoBackendAvailable,
        "trace-abc",
        "axon",
    );
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "36.h D5: the JSON honest failure must be HTTP 503"
    );
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["error"], "no_backend_available");
    assert_eq!(json["success"], false);
    assert_eq!(json["d_letter"], "D5");
    assert_eq!(json["endpoint"], "E");
    assert_eq!(json["flow"], "Chat");
    assert_eq!(json["trace_id"], "trace-abc");
    let msg = json["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("backend:") && msg.contains("API_KEY") && msg.contains("stub"),
        "36.h D5: the message must name every fix (declare backend:, \
         set a provider key, opt into stub). Got: {msg}"
    );
}

// ─── §2 — SSE honest failure is a dialect-correct error event ──────

#[tokio::test]
async fn s2_sse_honest_failure_axon_dialect_is_an_axon_error_event() {
    let route = one_route(ROUTE_SRC);
    let resp = honest_backend_failure_response(
        DynamicRouteWire::Sse,
        &route,
        &NoBackendAvailable,
        "trace-sse",
        "axon",
    );
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "36.h D5: the SSE honest failure is an in-band event — HTTP 200"
    );
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.starts_with("text/event-stream"),
        "36.h D5: the SSE honest failure must be `text/event-stream`, got {ct}"
    );
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("axon.error"),
        "36.h D5: the axon dialect must emit an `axon.error` event. Body: {text}"
    );
    assert!(
        text.contains("no execution backend available"),
        "36.h D5: the error event must carry the honest diagnostic. Body: {text}"
    );
}

#[tokio::test]
async fn s2_sse_honest_failure_openai_dialect_is_wire_valid() {
    let route = one_route(ROUTE_SRC);
    let resp = honest_backend_failure_response(
        DynamicRouteWire::Sse,
        &route,
        &NoBackendAvailable,
        "trace-oai",
        "openai",
    );
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("data:") && text.contains("[DONE]"),
        "36.h D5: the openai dialect honest failure must terminate \
         with the `data: [DONE]` sentinel. Body: {text}"
    );
}

// ─── integration helpers ───────────────────────────────────────────

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

// ─── §3 — end-to-end honest 503 for an unresolvable route ──────────

#[tokio::test]
async fn s3_undeclared_route_with_nothing_fails_honestly() {
    // The honest-failure path is reached only when EVERY ladder rung
    // is empty — including the environment scan. This test is the
    // end-to-end proof under the standard CI condition (no provider
    // API keys exported). A developer running locally WITH a key
    // exported would see the route resolve to that provider (rung
    // 4b) — the honest-failure path correctly NOT taken — so the
    // strict assertion is gated on the precondition.
    if !env_available_backends().is_empty() {
        eprintln!(
            "36.h s3: skipped strict assertion — provider API key(s) \
             present in the environment ({:?}); the route resolves via \
             rung 4b. Run with no *_API_KEY set for the strict path.",
            env_available_backends()
        );
        return;
    }

    let app = build_router(server_cfg()); // no server default, empty registry
    deploy(
        &app,
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat }",
    )
    .await;

    let req = Request::builder()
        .method("POST")
        .uri("/chat")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "36.h D5: an `axonendpoint` with no resolvable backend must \
         fail with HTTP 503 — never silently run the no-op `stub`"
    );
    assert!(
        resp.headers().get("x-axon-trace-id").is_some(),
        "36.h: the honest failure must still carry the trace correlation header"
    );
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["error"], "no_backend_available");
    assert_eq!(json["d_letter"], "D5");
}

// ─── §4 — explicit `backend: stub` never hits honest failure (D5) ──

#[tokio::test]
async fn s4_explicit_stub_is_a_legal_optin_never_honest_failure() {
    // D5 forbids a SILENT stub, not an explicit written opt-in. A
    // route declaring `backend: stub` resolves on rung 2 and executes
    // — no 503. Fully deterministic (no env dependency).
    let app = build_router(server_cfg());
    deploy(
        &app,
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat backend: stub }",
    )
    .await;

    let req = Request::builder()
        .method("POST")
        .uri("/chat")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "36.h D5: an explicit `backend: stub` is a legal opt-in — it \
         must execute, not 503"
    );
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["execution_metrics"]["backend"], "stub");
    assert!(json["step_audit"]["steps_executed"].as_u64().unwrap_or(0) >= 1);
}
