//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.23.0 — Auth-scope (capability subset) gate end-to-end tests.
//!
//! D8 ratificada 2026-05-11. When the axonendpoint declares
//! `requires: [cap1, cap2, ...]`, the runtime verifies the bearer's
//! `capabilities` JWT claim contains every declared slug (AND
//! semantics — `declared ⊆ have`). Missing capability → 403
//! Forbidden with structured `{error: "missing_capability",
//! missing, required, have, endpoint, method, path, hint,
//! d_letter: "D8"}`.
//!
//! Pillar trace per D12:
//!   - PHILOSOPHY — the access contract IS the source declaration;
//!                   auditors read source + KNOW which endpoints
//!                   require which capabilities.
//!   - LOGIC      — subset predicate `declared ⊆ have` is precise +
//!                   total over the two sets.
//!   - COMPUTING  — D9 backwards-compat absolute: empty requires
//!                   list short-circuits to Allow; /v1/execute legacy
//!                   path unaffected; existing axonendpoints without
//!                   `requires:` keep current behavior.
//!
//! Coverage:
//!   - Single-capability allow + deny.
//!   - Multi-capability AND semantics (every slug must be present).
//!   - No bearer → 403 on protected endpoint.
//!   - D9 backwards-compat: no `requires:` declared → no gate.
//!   - Auth fires BEFORE idempotency cache lookup (info-leak hardening).
//!   - Diagnostic payload shape locked (adopter client contract).
//!   - Cross-stack with Python parser (slug grammar shared).

use axon::axon_server::{build_router, ServerConfig};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
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

async fn deploy(app: axum::Router, src: &str) {
    let body = serde_json::json!({
        "source": src,
        "source_file": "test.axon",
        "backend": "stub",
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(status, StatusCode::OK, "deploy failed: {json}");
    assert_eq!(
        json.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "deploy success=false: {json}"
    );
}

/// Mint an unverified JWT carrying the given `capabilities` claim.
/// Used purely for testing the OSS unverified-decode path; production
/// deployments layer signature verification on top via the existing
/// `tenant_extractor_middleware`.
fn jwt_with_caps(caps: &[&str]) -> String {
    let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\",\"typ\":\"JWT\"}");
    let payload_json = serde_json::json!({"capabilities": caps});
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload_json).unwrap());
    format!("{header}.{payload}.")
}

fn post_with(
    path: &str,
    body: &serde_json::Value,
    bearer: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json");
    if let Some(b) = bearer {
        builder = builder.header("authorization", format!("Bearer {b}"));
    }
    builder
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

const ADMIN_FLOW: &str =
    "flow Touch() -> String { let result = \"ok\" return result }\n\
     axonendpoint AdminEndpoint { method: POST path: \"/admin/policy\" \
        execute: Touch requires: [admin] }";

const MULTI_CAP_FLOW: &str =
    "flow Touch() -> String { let result = \"ok\" return result }\n\
     axonendpoint LegalEndpoint { method: POST path: \"/legal/discovery\" \
        execute: Touch requires: [legal.read, legal.write] }";

const PUBLIC_FLOW: &str =
    "flow Touch() -> String { let result = \"ok\" return result }\n\
     axonendpoint Pub { public: true method: POST path: \"/pub\" execute: Touch }";

// ─── D9 backwards-compat: no `requires:` declared ────────────────────

#[tokio::test]
async fn no_requires_declared_passes_without_bearer_d9() {
    let app = build_router(server_cfg());
    deploy(app.clone(), PUBLIC_FLOW).await;
    let resp = app
        .oneshot(post_with("/pub", &serde_json::json!({}), None))
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_ne!(p["error"], "missing_capability");
}

// ─── Single capability — allow path ─────────────────────────────────

#[tokio::test]
async fn bearer_with_required_capability_passes() {
    let app = build_router(server_cfg());
    deploy(app.clone(), ADMIN_FLOW).await;
    let token = jwt_with_caps(&["admin"]);
    let resp = app
        .oneshot(post_with(
            "/admin/policy",
            &serde_json::json!({}),
            Some(&token),
        ))
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_ne!(p["error"], "missing_capability");
}

// ─── Single capability — deny path ──────────────────────────────────

#[tokio::test]
async fn bearer_without_required_capability_returns_403() {
    let app = build_router(server_cfg());
    deploy(app.clone(), ADMIN_FLOW).await;
    // Bearer has unrelated capabilities — must be denied.
    let token = jwt_with_caps(&["legal.read"]);
    let resp = app
        .oneshot(post_with(
            "/admin/policy",
            &serde_json::json!({}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(p["error"], "missing_capability");
    assert_eq!(p["d_letter"], "D8");
}

// ─── No bearer → 403 ────────────────────────────────────────────────

#[tokio::test]
async fn no_bearer_on_protected_endpoint_returns_403() {
    let app = build_router(server_cfg());
    deploy(app.clone(), ADMIN_FLOW).await;
    let resp = app
        .oneshot(post_with(
            "/admin/policy",
            &serde_json::json!({}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(p["error"], "missing_capability");
    let missing = p["missing"].as_array().unwrap();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0], "admin");
}

// ─── Multi-capability AND semantics ─────────────────────────────────

#[tokio::test]
async fn multi_capability_requires_all_slugs() {
    let app = build_router(server_cfg());
    deploy(app.clone(), MULTI_CAP_FLOW).await;

    // Only one of two capabilities → 403, missing the other.
    let partial = jwt_with_caps(&["legal.read"]);
    let resp = app
        .clone()
        .oneshot(post_with(
            "/legal/discovery",
            &serde_json::json!({}),
            Some(&partial),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let missing: Vec<String> = p["missing"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(missing, vec!["legal.write".to_string()]);

    // Both capabilities → pass.
    let full = jwt_with_caps(&["legal.read", "legal.write"]);
    let resp = app
        .oneshot(post_with(
            "/legal/discovery",
            &serde_json::json!({}),
            Some(&full),
        ))
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_ne!(p["error"], "missing_capability");
}

// ─── Diagnostic payload shape locked (adopter contract) ─────────────

#[tokio::test]
async fn deny_payload_shape_is_locked() {
    let app = build_router(server_cfg());
    deploy(app.clone(), MULTI_CAP_FLOW).await;
    let token = jwt_with_caps(&["other.cap"]);
    let resp = app
        .oneshot(post_with(
            "/legal/discovery",
            &serde_json::json!({}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    // Locked-shape contract — adopter clients can rely on these keys.
    assert_eq!(p["error"], "missing_capability");
    assert!(p["missing"].is_array());
    assert!(p["required"].is_array());
    assert!(p["have"].is_array());
    assert!(p["endpoint"].is_string());
    assert!(p["method"].is_string());
    assert!(p["path"].is_string());
    assert!(p["hint"].is_string());
    assert_eq!(p["d_letter"], "D8");
    // `required` mirrors declaration order; `have` reflects bearer.
    assert_eq!(p["required"][0], "legal.read");
    assert_eq!(p["required"][1], "legal.write");
    assert_eq!(p["have"][0], "other.cap");
}

// ─── Auth fires BEFORE idempotency cache lookup ─────────────────────

#[tokio::test]
async fn auth_gate_fires_before_idempotency_lookup() {
    // Information-leak hardening: an unauthorized client cannot
    // distinguish "key cached" from "key absent" because the auth
    // gate rejects them before the cache is consulted.
    let app = build_router(server_cfg());
    deploy(app.clone(), ADMIN_FLOW).await;

    // First, an AUTHORIZED request populates the cache.
    let admin_token = jwt_with_caps(&["admin"]);
    let mut req1 = post_with(
        "/admin/policy",
        &serde_json::json!({"op": "x"}),
        Some(&admin_token),
    );
    req1.headers_mut()
        .insert("idempotency-key", "shared-key".parse().unwrap());
    let _ = app.clone().oneshot(req1).await.unwrap();

    // Now an UNAUTHORIZED request with the same Idempotency-Key must
    // return 403, NOT a 200 cache hit. This proves the auth gate
    // fires BEFORE the cache lookup — the cache is invisible to
    // unauthorized callers.
    let mut req2 = post_with(
        "/admin/policy",
        &serde_json::json!({"op": "x"}),
        None, // no bearer
    );
    req2.headers_mut()
        .insert("idempotency-key", "shared-key".parse().unwrap());
    let resp2 = app.oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::FORBIDDEN);
    // And the response must NOT carry the cache-replay header.
    assert!(resp2.headers().get("idempotency-status").is_none());
}

// ─── Parser rejects invalid slug at deploy time (D8 grammar) ────────

#[tokio::test]
async fn invalid_capability_slug_rejected_at_parse_time() {
    let app = build_router(server_cfg());
    // Uppercase slug → parser rejects.
    let src = "flow Touch() -> String { let result = \"ok\" return result }\n\
               axonendpoint BadCap { method: POST path: \"/x\" execute: Touch \
                  requires: [Admin] }";
    let body = serde_json::json!({
        "source": src,
        "source_file": "test.axon",
        "backend": "stub",
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    // Deploy must FAIL on invalid slug — parser/type-checker rejects.
    let success = json.get("success").and_then(|v| v.as_bool());
    assert_eq!(
        success,
        Some(false),
        "deploy must reject invalid slug 'Admin' at parse time, got success={success:?}"
    );
}

// ─── /v1/execute legacy is unaffected ───────────────────────────────

#[tokio::test]
async fn v1_execute_legacy_unaffected_by_requires_gate() {
    // D10 backwards-compat: /v1/execute doesn't go through the
    // dynamic-route handler, so the auth gate doesn't fire there.
    // Legacy adopters continue to work even when their flows are
    // referenced by axonendpoints declaring `requires:`.
    let app = build_router(server_cfg());
    deploy(app.clone(), ADMIN_FLOW).await;
    let body = serde_json::json!({"flow": "Touch", "backend": "stub"});
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_ne!(p["error"], "missing_capability");
}

// ─── Body validation fires BEFORE auth (404 also before auth) ───────

#[tokio::test]
async fn body_validation_fires_before_auth_for_efficiency() {
    // Composition order: 32.c (body) → 32.g (auth) → 32.f (idem) →
    // dispatch → 32.d (output). Malformed body → 400 even if the
    // bearer is missing capabilities (cheaper reject; the request is
    // structurally invalid before any auth decision matters).
    let app = build_router(server_cfg());
    let src = "type Req { amount: Integer }\n\
               flow Touch() -> String { let result = \"ok\" return result }\n\
               axonendpoint Gated { method: POST path: \"/gated\" execute: Touch \
                  body: Req requires: [admin] }";
    deploy(app.clone(), src).await;

    // Malformed body + no bearer — body validation fires first.
    let resp = app
        .oneshot(post_with(
            "/gated",
            &serde_json::json!({"amount": "not-int"}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(p["error"], "body_schema_violation");
}
