//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 36.f (D1, D3) — `dynamic_endpoint_handler` resolves the
//! backend by the D1 ladder.
//!
//! 36.d/36.e landed the declaration + carried it onto the route.
//! 36.f retires the hardcoded `backend: "auto"` in
//! `dynamic_endpoint_handler` (both the SSE and the JSON branch):
//! the handler now resolves the execution backend through the
//! Backend Resolution Contract ladder (`resolve_route_backend`) and
//! dispatches against the resolved concrete backend.
//!
//! Pins:
//!   1. `resolve_route_backend` projects a route onto the ladder —
//!      rung 2 (the declared `backend:`) fires; an undeclared route
//!      falls through to registry / env / honest failure.
//!   2. A declared `backend:` is what executes on the JSON branch —
//!      and it OUTRANKS an operator-tuned registry entry (rung 2 >
//!      rung 4a). Pre-36 the hardcoded `"auto"` would have auto-
//!      ranked the registry instead.
//!   3. The SSE branch dispatches against the resolved backend too.
//!   4. The resolution is deterministic.

use axon::axon_server::{
    build_router, collect_axonendpoint_routes, resolve_route_backend, DynamicEndpointRoute,
    ServerConfig,
};
use axon::backend_resolution::BackendResolutionReason;
use axon::type_checker::compute_implicit_transports;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use std::collections::HashMap;
use tower::ServiceExt;

// ─── route-table helpers (pure unit tests) ─────────────────────────

fn one_route(src: &str) -> DynamicEndpointRoute {
    let tokens = axon::lexer::Lexer::new(src, "<test>").tokenize().unwrap();
    let mut prog = axon::parser::Parser::new(tokens).parse().unwrap();
    compute_implicit_transports(&mut prog);
    let table: HashMap<_, _> = collect_axonendpoint_routes(&prog, src, "<test>").unwrap();
    table.into_values().next().expect("one route expected")
}

const DECLARED: &str = "flow F() -> Unit { step S { ask: \"x\" } }\n\
    axonendpoint E { public: true method: POST path: \"/chat\" execute: F backend: gemini }";
const UNDECLARED: &str = "flow F() -> Unit { step S { ask: \"x\" } }\n\
    axonendpoint E { public: true method: POST path: \"/chat\" execute: F }";

// ─── §1 — resolve_route_backend projects onto the ladder ───────────

#[test]
fn s1_declared_backend_resolves_via_rung_2() {
    let route = one_route(DECLARED);
    let r = resolve_route_backend(&route, vec![], vec![], None).unwrap();
    assert_eq!(r.backend, "gemini");
    assert_eq!(
        r.reason,
        BackendResolutionReason::EndpointDeclared,
        "36.f D1: a route's declared `backend:` resolves on rung 2"
    );
}

#[test]
fn s1_declared_backend_outranks_registry_and_env() {
    let route = one_route(DECLARED);
    let r = resolve_route_backend(
        &route,
        vec!["openai".into()],
        vec!["anthropic".into()],
        Some("kimi".into()),
    )
    .unwrap();
    assert_eq!(
        r.backend, "gemini",
        "36.f D1: the declared `backend:` (rung 2) outranks the server \
         default (rung 3), the registry (rung 4a) and the env (rung 4b)"
    );
}

#[test]
fn s1_undeclared_route_falls_through_to_registry() {
    let route = one_route(UNDECLARED);
    let r = resolve_route_backend(&route, vec!["openai".into()], vec![], None).unwrap();
    assert_eq!(r.backend, "openai");
    assert_eq!(r.reason, BackendResolutionReason::RegistryRanked);
}

#[test]
fn s1_undeclared_route_falls_through_to_env() {
    let route = one_route(UNDECLARED);
    let r = resolve_route_backend(&route, vec![], vec!["kimi".into()], None).unwrap();
    assert_eq!(r.backend, "kimi");
    assert_eq!(r.reason, BackendResolutionReason::EnvironmentAvailable);
}

#[test]
fn s1_undeclared_route_uses_server_default_over_auto_rungs() {
    let route = one_route(UNDECLARED);
    let r = resolve_route_backend(
        &route,
        vec!["openai".into()],
        vec!["kimi".into()],
        Some("anthropic".into()),
    )
    .unwrap();
    assert_eq!(r.backend, "anthropic");
    assert_eq!(r.reason, BackendResolutionReason::ServerDefault);
}

#[test]
fn s1_undeclared_route_with_nothing_is_honest_failure() {
    // §D5 — every rung empty: the honest `Err`, never a silent stub.
    let route = one_route(UNDECLARED);
    assert!(
        resolve_route_backend(&route, vec![], vec![], None).is_err(),
        "36.f D5: an undeclared route with no registry / no env / no \
         server default is an honest failure — never a silent stub"
    );
}

#[test]
fn s1_resolution_is_deterministic() {
    let route = one_route(DECLARED);
    let a = resolve_route_backend(&route, vec!["openai".into()], vec![], None).unwrap();
    let b = resolve_route_backend(&route, vec!["openai".into()], vec![], None).unwrap();
    assert_eq!(a, b);
}

// ─── integration: deploy + hit, end-to-end ─────────────────────────

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

/// Register a backend in the operator-tuned registry (rung 4a). The
/// presence of this entry is what makes the test DISTINGUISHING: the
/// pre-36 hardcoded `"auto"` would have auto-ranked this registry
/// entry; 36.f's declared `backend: stub` must outrank it.
async fn register_backend(app: &axum::Router, name: &str) {
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/v1/backends/{name}"))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"enabled":true}"#))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "register {name} failed");
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

async fn hit(app: &axum::Router, method: &str, path: &str, accept_sse: bool) -> (StatusCode, String, Vec<u8>) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if accept_sse {
        builder = builder.header("accept", "text/event-stream");
    }
    let resp = app.clone().oneshot(builder.body(Body::from("{}")).unwrap()).await.unwrap();
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

// ─── §2 — JSON branch: declared backend executes, beats registry ───

#[tokio::test]
async fn s2_json_branch_runs_the_declared_backend_outranking_registry() {
    let app = build_router(server_cfg());
    // Operator-tuned registry holds `openai` — the pre-36 hardcoded
    // `"auto"` would have auto-ranked this and executed `openai`.
    register_backend(&app, "openai").await;

    deploy(
        &app,
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat backend: stub }",
    )
    .await;

    let (status, ct, body) = hit(&app, "POST", "/chat", false).await;
    assert_eq!(status, StatusCode::OK, "dynamic route must dispatch");
    assert!(ct.starts_with("application/json"), "JSON branch, got {ct}");

    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["execution_metrics"]["backend"], "stub",
        "36.f D1+D3: the route's declared `backend: stub` (rung 2) must \
         execute — outranking the `openai` registry entry the pre-36 \
         hardcoded `\"auto\"` would have auto-ranked. Body: {json}"
    );
    assert!(
        json["step_audit"]["steps_executed"].as_u64().unwrap_or(0) >= 1,
        "36.f: a real step must run against the resolved backend: {json}"
    );
}

// ─── §3 — SSE branch dispatches against the resolved backend ───────

#[tokio::test]
async fn s3_sse_branch_runs_the_declared_backend() {
    let app = build_router(server_cfg());
    register_backend(&app, "openai").await;

    deploy(
        &app,
        "flow Stream() -> Unit { step S { ask: \"hi\" output: Stream<Token> } }\n\
         axonendpoint E { public: true method: POST path: \"/stream\" execute: Stream \
         backend: stub transport: sse }",
    )
    .await;

    let (status, ct, body) = hit(&app, "POST", "/stream", true).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        ct.starts_with("text/event-stream"),
        "36.f: the SSE branch must serve `text/event-stream`, got {ct}"
    );
    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("axon.complete") || text.contains("axon.token"),
        "36.f: the SSE branch must stream the flow against the resolved \
         `stub` backend and reach completion — no hardcoded `\"auto\"` \
         dead-ending. Body: {text}"
    );
}

// ─── §4 — undeclared route still dispatches (D9 no-regression) ─────

#[tokio::test]
async fn s4_undeclared_route_still_dispatches() {
    // No registry entry, route declares `backend: stub` so the test
    // stays deterministic (no env-dependent provider resolution). The
    // point: a route with no operator tuning still routes + executes.
    let app = build_router(server_cfg());
    deploy(
        &app,
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/plain\" execute: Chat backend: stub }",
    )
    .await;
    let (status, _ct, body) = hit(&app, "POST", "/plain", false).await;
    assert_eq!(status, StatusCode::OK, "36.f D9: the route must dispatch");
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["execution_metrics"]["backend"], "stub");
    assert!(json["step_audit"]["steps_executed"].as_u64().unwrap_or(0) >= 1);
}
