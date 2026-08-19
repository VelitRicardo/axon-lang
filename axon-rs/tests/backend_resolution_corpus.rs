//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.31.0 (D9) — backwards-compatibility corpus.
//!
//! D9 is absolute: the ONLY behavior change v1.31.0 introduces is
//! the intended fix (an endpoint that silently ran `stub` because a
//! provider key IS present now runs that provider; an endpoint with
//! genuinely no backend flips from a silent no-op to a loud 503).
//! Every pre-36 working setup must keep working byte-for-byte.
//!
//! This corpus consolidates the D9 contract into one pinned pack:
//!
//!   1. `/v1/execute` with an explicit `backend` is byte-unchanged.
//!   2. `ExecuteRequest.backend` still defaults to `stub`.
//!   3. `DeployRequest.backend` defaults to `auto` (was `anthropic`
//!      pre-36 — but the dynamic-route path never consulted it, so
//!      the change is regression-free).
//!   4. A pre-36 `.axon` (axonendpoint with no `backend:` field
//!      anywhere) parses + type-checks with zero errors.
//!   5. An `axonendpoint` with no `backend:` still deploys — the
//!      deploy-time check is non-blocking.
//!   6. Such an endpoint, with a backend resolvable in the
//!      environment, runs it with zero source change.

use axon::axon_server::{build_router, DeployRequest, ExecuteRequest, ServerConfig};
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

async fn post_json(
    app: &axum::Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    (status, json)
}

// ─── section 1 — /v1/execute with an explicit backend is unchanged ────────

#[tokio::test]
async fn d9_v1_execute_explicit_backend_unchanged() {
    let app = build_router(server_cfg());
    let (ds, dj) = post_json(
        &app,
        "/v1/deploy",
        serde_json::json!({
            "source": "flow Chat() -> Unit { step S { ask: \"hi\" } }",
            "source_file": "t.axon",
        }),
    )
    .await;
    assert_eq!(ds, StatusCode::OK, "{dj}");

    let (status, json) =
        post_json(&app, "/v1/execute", serde_json::json!({"flow": "Chat", "backend": "stub"}))
            .await;
    assert_eq!(status, StatusCode::OK);
    // v2.0.0 FlowEnvelope wire: a real step ran (no top-level `success`).
    assert!(
        json["step_audit"]["steps_executed"].as_u64().unwrap_or(0) >= 1,
        "36.l D9: `/v1/execute` with an explicit backend must run a real \
         step. Body: {json}"
    );
    assert_eq!(json["execution_metrics"]["backend"], "stub");
}

// ─── section 2 — request-struct serde defaults are unchanged-or-honest ────

#[test]
fn d9_execute_request_backend_still_defaults_to_stub() {
    let req: ExecuteRequest =
        serde_json::from_str(r#"{"flow":"Chat"}"#).expect("deserialize");
    assert_eq!(
        req.backend, "stub",
        "36.l D9: `ExecuteRequest.backend` default is UNCHANGED — \
         `/v1/execute` without a backend still defaults to `stub`"
    );
}

#[test]
fn d9_deploy_request_backend_defaults_to_auto() {
    let req: DeployRequest =
        serde_json::from_str(r#"{"source":"flow F() -> Unit {}"}"#).expect("deserialize");
    assert_eq!(
        req.backend, "auto",
        "36.l D9: `DeployRequest.backend` now defaults to `auto` (was \
         `anthropic`). Regression-free — the dynamic-route execution \
         path never consulted this field pre-36."
    );
}

// ─── section 3 — a pre-36 .axon parses + type-checks clean ────────────────

#[test]
fn d9_pre_36_axon_compiles_with_zero_errors() {
    // No `backend:` field anywhere — the v1.33.0 adopter program.
    let src = "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
               axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat }";
    let tokens = axon::lexer::Lexer::new(src, "t.axon").tokenize().expect("lex");
    let prog = axon::parser::Parser::new(tokens).parse().expect("parse");
    let errors = axon::type_checker::TypeChecker::new(&prog).check();
    assert!(
        errors.is_empty(),
        "36.l D9: a pre-36 axonendpoint (no `backend:`) must compile \
         with ZERO errors — the field is optional. Errors: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ─── section 4 — an undeclared-backend endpoint still deploys ─────────────

#[tokio::test]
async fn d9_undeclared_backend_endpoint_still_deploys() {
    let app = build_router(server_cfg());
    let (status, json) = post_json(
        &app,
        "/v1/deploy",
        serde_json::json!({
            "source": "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
                       axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat }",
            "source_file": "t.axon",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["success"], true,
        "36.l D9: an axonendpoint with no `backend:` must still deploy \
         — the deploy-time resolution check (36.k) is NON-blocking. \
         Body: {json}"
    );
}

// ─── section 5 — the intended fix: a server default "just works" ──────────

#[tokio::test]
async fn d9_endpoint_runs_resolved_backend_with_zero_source_change() {
    // The intended D9 behavior change: an endpoint that declares no
    // `backend:` resolves down the ladder. With a server default set
    // (`axon serve --backend stub`), the undeclared endpoint runs it
    // — zero source change, no silent no-op, an honest `success:
    // true`. (A `stub` server default is used so the test stays
    // deterministic; in production this would be a real provider.)
    let cfg = ServerConfig {
        default_backend: Some("stub".to_string()),
        ..server_cfg()
    };
    let app = build_router(cfg);

    let (ds, _dj) = post_json(
        &app,
        "/v1/deploy",
        serde_json::json!({
            "source": "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
                       axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat }",
            "source_file": "t.axon",
        }),
    )
    .await;
    assert_eq!(ds, StatusCode::OK);

    let (status, json) = post_json(&app, "/chat", serde_json::json!({})).await;
    assert_eq!(status, StatusCode::OK, "Body: {json}");
    // v2.0.0 FlowEnvelope wire: resolution is visible via backend_resolution
    // and a real step ran (steps_executed under step_audit).
    assert_eq!(
        json["backend_resolution"]["backend"], "stub",
        "36.l D9: an undeclared-backend endpoint resolves its backend \
         down the ladder and runs — never a silent no-op. Body: {json}"
    );
    assert!(json["step_audit"]["steps_executed"].as_u64().unwrap_or(0) >= 1);
}
