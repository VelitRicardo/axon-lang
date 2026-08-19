//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.31.0 (D7) — server default backend.
//!
//! Rung 3 of the Backend Resolution Contract: `axon serve --backend
//! <name>` / `AXON_DEFAULT_BACKEND` / `ServerConfig.default_backend`.
//! When an `axonendpoint` declares no `backend:` of its own, the
//! server-wide default is consulted before the environment-available
//! `auto` rungs — letting an operator pin a fleet-wide default
//! without editing a single `.axon`.
//!
//! Pins:
//!   1. `validate_server_default_backend` accepts `None` + every
//!      closed-catalog entry; rejects an unknown name with a
//!      diagnostic that lists the catalog.
//!   2. The server default feeds rung 3 end-to-end — an undeclared
//!      route on a server with `default_backend: Some(_)` resolves to
//!      that backend, OUTRANKING the operator-tuned registry (rung 4a).
//!   3. A route's own declared `backend:` (rung 2) still outranks the
//!      server default (rung 3).

use axon::axon_server::{build_router, validate_server_default_backend, ServerConfig};
use axon::parser::AXONENDPOINT_BACKEND_VALUES;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

// ─── section 1 — validate_server_default_backend (pure) ───────────────────

#[test]
fn s1_none_is_always_valid() {
    assert!(
        validate_server_default_backend(&None).is_ok(),
        "36.g D7: `None` ≡ no server default — always valid"
    );
}

#[test]
fn s2_every_catalog_entry_is_valid() {
    for &b in AXONENDPOINT_BACKEND_VALUES {
        assert!(
            validate_server_default_backend(&Some(b.to_string())).is_ok(),
            "36.g D7: catalog entry `{b}` must be a valid server default"
        );
    }
}

#[test]
fn s3_unknown_backend_is_rejected_with_a_listing() {
    let err = validate_server_default_backend(&Some("gpt-9-ultra".into()))
        .expect_err("36.g D7: an unknown server default must be rejected");
    assert!(
        err.contains("gpt-9-ultra"),
        "36.g: the diagnostic must name the offending value. Got: {err}"
    );
    assert!(
        err.contains("anthropic") && err.contains("stub"),
        "36.g: the diagnostic must list the valid catalog. Got: {err}"
    );
    assert!(
        err.contains("--backend") || err.contains("AXON_DEFAULT_BACKEND"),
        "36.g: the diagnostic should name the surfaces. Got: {err}"
    );
}

// ─── integration: deploy + hit ─────────────────────────────────────

fn server_cfg(default_backend: Option<&str>) -> ServerConfig {
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
        default_backend: default_backend.map(|s| s.to_string()),
        schemas_dir: None,
    }
}

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

async fn hit_json(app: &axum::Router, path: &str) -> serde_json::Value {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "{path} must dispatch");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ─── section 4 — server default feeds rung 3, outranking the registry ─────

#[tokio::test]
async fn s4_server_default_feeds_rung_3_over_the_registry() {
    // `default_backend: stub` is the server-wide default (rung 3).
    // The operator-tuned registry holds `anthropic` (rung 4a). An
    // UNDECLARED route must resolve to the server default — rung 3
    // is consulted BEFORE the `auto` rungs.
    let app = build_router(server_cfg(Some("stub")));
    register_backend(&app, "anthropic").await;

    deploy(
        &app,
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat }",
    )
    .await;

    let json = hit_json(&app, "/chat").await;
    assert_eq!(
        json["execution_metrics"]["backend"], "stub",
        "36.g D7: an undeclared route must resolve to the server \
         default `stub` (rung 3) — outranking the `anthropic` registry \
         entry (rung 4a). Body: {json}"
    );
    assert!(json["step_audit"]["steps_executed"].as_u64().unwrap_or(0) >= 1);
}

// ─── section 5 — the route declaration (rung 2) outranks the server default

#[tokio::test]
async fn s5_route_declaration_outranks_server_default() {
    // Server default is `openai` (rung 3); the route declares
    // `backend: stub` (rung 2). Rung 2 must win — and because it
    // does, the test stays deterministic (no `openai` network call).
    let app = build_router(server_cfg(Some("openai")));

    deploy(
        &app,
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat backend: stub }",
    )
    .await;

    let json = hit_json(&app, "/chat").await;
    assert_eq!(
        json["execution_metrics"]["backend"], "stub",
        "36.g D7: the route's declared `backend: stub` (rung 2) must \
         outrank the server default `openai` (rung 3). Body: {json}"
    );
    assert!(json["step_audit"]["steps_executed"].as_u64().unwrap_or(0) >= 1);
}

// ─── section 6 — no server default: undeclared route still dispatches ─────

#[tokio::test]
async fn s6_no_server_default_undeclared_route_still_dispatches() {
    // `default_backend: None` — rung 3 is transparent. The route
    // declares `backend: stub` so the test stays deterministic.
    let app = build_router(server_cfg(None));
    deploy(
        &app,
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/plain\" execute: Chat backend: stub }",
    )
    .await;
    let json = hit_json(&app, "/plain").await;
    assert_eq!(json["execution_metrics"]["backend"], "stub");
    assert!(json["step_audit"]["steps_executed"].as_u64().unwrap_or(0) >= 1);
}
