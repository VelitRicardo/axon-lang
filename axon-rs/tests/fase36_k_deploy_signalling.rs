//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 36.k (D10, D11) — deploy-time signalling + `steps_executed`
//! honesty.
//!
//! D10 — `deploy_handler` runs the D1 ladder for every collected
//! route at deploy time; a route whose backend cannot be resolved is
//! surfaced in the deploy response `warnings` array. The adopter
//! learns at deploy, not at the first production 503.
//!
//! D11 — `steps_executed` honesty: the gap report's symptom was a
//! deployed `axonendpoint` returning `steps_executed: 0` while the
//! flow silently ran the no-op `stub`. Post-36 a stub-backed route
//! runs a REAL step (`steps_executed >= 1`, `success: true` — the
//! 33.b hollow-wire fix), and a route with no resolvable backend
//! fails with a structured 503 (36.h) rather than a hollow
//! `success:false, steps_executed:0` body. A misleading clean `0`
//! can no longer occur.
//!
//! Pins:
//!   1. A resolvable route deploys with an empty `warnings` array.
//!   2. An unresolvable route deploys (non-blocking) with a
//!      `no_resolvable_backend` deploy warning.
//!   3. A stub-backed route executes a real step — `success: true`,
//!      `steps_executed >= 1` (never the misleading `0`).
//!   4. The honest failure is a structured 503, never a hollow
//!      `success:false, steps_executed:0` body.

use axon::axon_server::{build_router, ServerConfig};
use axon::backends::env_available_backends;
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

async fn deploy(app: &axum::Router, src: &str) -> serde_json::Value {
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
    json
}

async fn hit(app: &axum::Router, path: &str) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    (status, json)
}

// ─── §1 — a resolvable route deploys with no warnings ──────────────

#[tokio::test]
async fn s1_resolvable_route_deploys_with_empty_warnings() {
    let app = build_router(server_cfg());
    let resp = deploy(
        &app,
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat backend: stub }",
    )
    .await;
    assert_eq!(resp["success"], true);
    let warnings = resp["warnings"].as_array().expect("warnings array");
    assert!(
        warnings.is_empty(),
        "36.k D10: a route with a declared `backend: stub` resolves — \
         the deploy must carry no backend warnings. Got: {warnings:?}"
    );
}

// ─── §2 — an unresolvable route deploys with a deploy warning ──────

#[tokio::test]
async fn s2_unresolvable_route_surfaces_a_deploy_warning() {
    // Reached only when the env carries no provider keys (standard
    // CI) — otherwise the undeclared route resolves via rung 4b.
    if !env_available_backends().is_empty() {
        eprintln!(
            "36.k s2: skipped — provider API key(s) present ({:?}); the \
             undeclared route resolves and raises no deploy warning.",
            env_available_backends()
        );
        return;
    }
    let app = build_router(server_cfg());
    let resp = deploy(
        &app,
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat }",
    )
    .await;
    assert_eq!(
        resp["success"], true,
        "36.k D10: the deploy-time check is NON-BLOCKING — the deploy \
         still succeeds"
    );
    let warnings = resp["warnings"].as_array().expect("warnings array");
    assert_eq!(warnings.len(), 1, "one unresolvable route: {warnings:?}");
    let w = &warnings[0];
    assert_eq!(w["code"], "no_resolvable_backend");
    assert_eq!(w["d_letter"], "D10");
    assert_eq!(w["endpoint"], "E");
    assert_eq!(w["method"], "POST");
    assert_eq!(w["path"], "/chat");
    let msg = w["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("503") && msg.contains("backend:"),
        "36.k: the deploy warning must name the consequence + fixes. \
         Got: {msg}"
    );
}

// ─── §3 — D11: a stub-backed route runs a REAL step ────────────────

#[tokio::test]
async fn s3_stub_route_executes_a_real_step_never_a_misleading_zero() {
    let app = build_router(server_cfg());
    deploy(
        &app,
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat backend: stub }",
    )
    .await;
    let (status, json) = hit(&app, "/chat").await;
    assert_eq!(status, StatusCode::OK);
    let steps = json["step_audit"]["steps_executed"].as_u64().unwrap_or(0);
    assert!(
        steps >= 1,
        "36.k D11: the gap report's symptom was `steps_executed: 0` \
         on a deployed endpoint. A stub-backed route now runs a REAL \
         step — `steps_executed` reflects reality, never a misleading \
         clean `0`. Body: {json}"
    );
}

// ─── §4 — D11: honest failure, never a hollow steps:0 body ─────────

#[tokio::test]
async fn s4_honest_failure_is_structured_never_a_hollow_zero() {
    if !env_available_backends().is_empty() {
        eprintln!(
            "36.k s4: skipped — provider API key(s) present ({:?}).",
            env_available_backends()
        );
        return;
    }
    let app = build_router(server_cfg());
    deploy(
        &app,
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat }",
    )
    .await;
    let (status, json) = hit(&app, "/chat").await;
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "36.k D11: an unresolvable route fails with a structured 503 — \
         NOT a hollow `success:false, steps_executed:0` body. Body: {json}"
    );
    assert_eq!(json["error"], "no_backend_available");
    assert!(
        json.get("steps_executed").is_none(),
        "36.k D11: the honest failure carries a structured diagnostic, \
         not a misleading `steps_executed` counter. Body: {json}"
    );
}
