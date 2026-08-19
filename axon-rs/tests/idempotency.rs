//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.23.0 — Idempotency-Key end-to-end tests.
//!
//! D7 ratificada 2026-05-11. Verifies the Stripe-compatible
//! Idempotency-Key surface on dynamic axonendpoint routes:
//!
//!   * First POST with key → execute + cache the response.
//!   * Repeat POST with same key + same body → byte-identical cached
//!     response + `Idempotency-Status: replayed` header.
//!   * Repeat POST with same key + different body → 422 with
//!     `idempotency_key_reused_with_different_request`.
//!   * POST without key → normal execute (no caching).
//!   * Cross-tenant isolation: two clients can use the same key
//!     value without colliding.
//!   * Cross-path isolation: same client + same key on different
//!     paths don't collide.
//!   * GET / DELETE with key → header ignored (HTTP-spec idempotent).
//!   * Backwards-compat: existing /v1/execute path unaffected.
//!
//! Pillar trace per D12:
//!   - MATHEMATICS — `same_key + same_body ⟹ same_response` invariant
//!                   verified by byte-for-byte equality of replays.
//!   - LOGIC      — every truth-table cell exercised once + the
//!                   422 conflict branch verified.
//!   - PHILOSOPHY — Stripe / Plaid clients work unchanged when
//!                   pointed at axon endpoints (industry standard
//!                   honored verbatim).
//!   - COMPUTING  — D9 backwards-compat absolute: requests without
//!                   the header are unaffected; non-POST/PUT
//!                   endpoints unaffected; /v1/execute unaffected.

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

const POST_FLOW: &str =
    "flow Charge() -> String { let result = \"ok\" return result }\n\
     axonendpoint ChargeEndpoint { public: true method: POST path: \"/charge\" execute: Charge }";

const GET_FLOW: &str =
    "flow Ping() -> String { let result = \"pong\" return result }\n\
     axonendpoint PingEndpoint { public: true method: GET path: \"/ping\" execute: Ping }";

const SECOND_PATH_FLOW: &str =
    "flow Charge() -> String { let result = \"ok\" return result }\n\
     flow Refund() -> String { let result = \"refunded\" return result }\n\
     axonendpoint ChargeEndpoint { public: true method: POST path: \"/charge\" execute: Charge }\n\
     axonendpoint RefundEndpoint { public: true method: POST path: \"/refund\" execute: Refund }";

fn post_with(
    path: &str,
    body: &serde_json::Value,
    idempotency_key: Option<&str>,
    auth: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json");
    if let Some(k) = idempotency_key {
        builder = builder.header("idempotency-key", k);
    }
    if let Some(a) = auth {
        builder = builder.header("authorization", a);
    }
    builder
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

// ─── First request caches; replay returns cached body ────────────────

#[tokio::test]
async fn first_request_caches_and_replay_returns_cached_body() {
    let app = build_router(server_cfg());
    deploy(app.clone(), POST_FLOW).await;
    let body = serde_json::json!({"amount": 100});

    let req1 = post_with("/charge", &body, Some("idem-001"), Some("Bearer tA"));
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    let status1 = resp1.status();
    let replayed_hdr_1 = resp1.headers().get("idempotency-status").cloned();
    let bytes1 = resp1.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(status1, StatusCode::OK);
    assert!(
        replayed_hdr_1.is_none(),
        "first response must NOT carry Idempotency-Status header"
    );

    let req2 = post_with("/charge", &body, Some("idem-001"), Some("Bearer tA"));
    let resp2 = app.oneshot(req2).await.unwrap();
    let status2 = resp2.status();
    let replayed_hdr_2 = resp2
        .headers()
        .get("idempotency-status")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes2 = resp2.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(status2, StatusCode::OK);
    assert_eq!(
        replayed_hdr_2.as_deref(),
        Some("replayed"),
        "replay response must carry Idempotency-Status: replayed"
    );
    assert_eq!(
        bytes1, bytes2,
        "D7 invariant: same key + same body ⟹ byte-identical response"
    );
}

// ─── Conflict — same key, different body ────────────────────────────

#[tokio::test]
async fn same_key_different_body_returns_422_conflict() {
    let app = build_router(server_cfg());
    deploy(app.clone(), POST_FLOW).await;

    let req1 = post_with(
        "/charge",
        &serde_json::json!({"amount": 100}),
        Some("idem-conflict"),
        Some("Bearer tA"),
    );
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);

    let req2 = post_with(
        "/charge",
        &serde_json::json!({"amount": 999}),
        Some("idem-conflict"),
        Some("Bearer tA"),
    );
    let resp2 = app.oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let bytes = resp2.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        payload["error"],
        "idempotency_key_reused_with_different_request"
    );
    assert_eq!(payload["d_letter"], "D7");
    assert_eq!(payload["idempotency_key"], "idem-conflict");
    // The 8-byte hash prefix surfaces in the diagnostic for adopter
    // correlation but is not the full hash (defense-in-depth).
    let prefix = payload["cached_body_hash_prefix"].as_str().unwrap();
    assert_eq!(prefix.len(), 16);
}

// ─── No key → no caching ────────────────────────────────────────────

#[tokio::test]
async fn no_idempotency_key_skips_cache_and_replay_header() {
    let app = build_router(server_cfg());
    deploy(app.clone(), POST_FLOW).await;
    let body = serde_json::json!({"amount": 50});

    let req1 = post_with("/charge", &body, None, Some("Bearer tA"));
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    assert!(resp1.headers().get("idempotency-status").is_none());

    // Without the header, the second request executes again from scratch.
    let req2 = post_with("/charge", &body, None, Some("Bearer tA"));
    let resp2 = app.oneshot(req2).await.unwrap();
    assert!(resp2.headers().get("idempotency-status").is_none());
}

// ─── Cross-tenant isolation ─────────────────────────────────────────

#[tokio::test]
async fn cross_tenant_same_key_is_not_a_collision() {
    let app = build_router(server_cfg());
    deploy(app.clone(), POST_FLOW).await;

    // Tenant A caches a response under key "shared".
    let req_a = post_with(
        "/charge",
        &serde_json::json!({"amount": 1}),
        Some("shared"),
        Some("Bearer tA"),
    );
    let resp_a = app.clone().oneshot(req_a).await.unwrap();
    assert_eq!(resp_a.status(), StatusCode::OK);

    // Tenant B uses the SAME key — must be a miss (cross-tenant isolation).
    let req_b = post_with(
        "/charge",
        &serde_json::json!({"amount": 1}),
        Some("shared"),
        Some("Bearer tB"),
    );
    let resp_b = app.oneshot(req_b).await.unwrap();
    let replayed = resp_b.headers().get("idempotency-status");
    assert!(
        replayed.is_none(),
        "Tenant B's first request must NOT see Tenant A's cached response"
    );
}

// ─── Cross-path isolation ────────────────────────────────────────────

#[tokio::test]
async fn cross_path_same_key_is_not_a_collision() {
    let app = build_router(server_cfg());
    deploy(app.clone(), SECOND_PATH_FLOW).await;

    let req_charge = post_with(
        "/charge",
        &serde_json::json!({"a": 1}),
        Some("path-key"),
        Some("Bearer tA"),
    );
    let resp_charge = app.clone().oneshot(req_charge).await.unwrap();
    assert_eq!(resp_charge.status(), StatusCode::OK);

    // Same client, same key, different path → must be a miss.
    let req_refund = post_with(
        "/refund",
        &serde_json::json!({"a": 1}),
        Some("path-key"),
        Some("Bearer tA"),
    );
    let resp_refund = app.oneshot(req_refund).await.unwrap();
    let replayed = resp_refund.headers().get("idempotency-status");
    assert!(
        replayed.is_none(),
        "/refund must NOT replay /charge's cached response under the same key"
    );
}

// ─── Header on GET is logged + ignored ──────────────────────────────

#[tokio::test]
async fn idempotency_key_on_get_is_ignored() {
    let app = build_router(server_cfg());
    deploy(app.clone(), GET_FLOW).await;

    let req = Request::builder()
        .method("GET")
        .uri("/ping")
        .header("idempotency-key", "key-on-get")
        .header("authorization", "Bearer tA")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    // GET is natively idempotent — no replay header attached, no caching.
    assert!(resp.headers().get("idempotency-status").is_none());
}

// ─── /v1/execute legacy path is unaffected ──────────────────────────

#[tokio::test]
async fn v1_execute_legacy_unaffected_by_idempotency_gate() {
    let app = build_router(server_cfg());
    deploy(app.clone(), POST_FLOW).await;

    // /v1/execute with the header — the LEGACY handler doesn't know
    // about Idempotency-Key (D10 backwards-compat). No replay header,
    // no 422; the request executes normally.
    let body = serde_json::json!({"flow": "Charge", "backend": "stub"});
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .header("idempotency-key", "legacy-key")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().get("idempotency-status").is_none());
}

// ─── Three repeat replays all return same body ──────────────────────

#[tokio::test]
async fn multiple_replays_all_return_same_body() {
    // The invariant must hold ACROSS multiple replays, not just one —
    // adopters retrying repeatedly on a flaky network should get the
    // same response every time.
    let app = build_router(server_cfg());
    deploy(app.clone(), POST_FLOW).await;
    let body = serde_json::json!({"amount": 250});

    let mut bodies = Vec::new();
    for _ in 0..4 {
        let req = post_with("/charge", &body, Some("retry-key"), Some("Bearer tA"));
        let resp = app.clone().oneshot(req).await.unwrap();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        bodies.push(bytes);
    }
    // First execution + 3 replays — all four bodies byte-identical.
    for i in 1..bodies.len() {
        assert_eq!(
            bodies[0], bodies[i],
            "replay #{i} must be byte-identical to the original"
        );
    }
}

// ─── PUT also caches (per Stripe semantics) ─────────────────────────

#[tokio::test]
async fn put_method_also_caches() {
    let src = "flow Upsert() -> String { let result = \"upserted\" return result }\n\
               axonendpoint UpsertEndpoint { public: true method: PUT path: \"/upsert\" execute: Upsert }";
    let app = build_router(server_cfg());
    deploy(app.clone(), src).await;

    let make_req = |key: &str| -> Request<Body> {
        Request::builder()
            .method("PUT")
            .uri("/upsert")
            .header("content-type", "application/json")
            .header("idempotency-key", key)
            .header("authorization", "Bearer tA")
            .body(Body::from(serde_json::json!({"v": 1}).to_string()))
            .unwrap()
    };

    let resp1 = app.clone().oneshot(make_req("put-key")).await.unwrap();
    let bytes1 = resp1.into_body().collect().await.unwrap().to_bytes();

    let resp2 = app.oneshot(make_req("put-key")).await.unwrap();
    let replayed = resp2
        .headers()
        .get("idempotency-status")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes2 = resp2.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(replayed.as_deref(), Some("replayed"));
    assert_eq!(bytes1, bytes2);
}

// ─── Conflict diagnostic shape — adopter-actionable ─────────────────

#[tokio::test]
async fn conflict_payload_shape_is_locked() {
    let app = build_router(server_cfg());
    deploy(app.clone(), POST_FLOW).await;

    let _ = app
        .clone()
        .oneshot(post_with(
            "/charge",
            &serde_json::json!({"amount": 1}),
            Some("conflict-shape"),
            Some("Bearer tA"),
        ))
        .await
        .unwrap();

    let resp = app
        .oneshot(post_with(
            "/charge",
            &serde_json::json!({"amount": 2}),
            Some("conflict-shape"),
            Some("Bearer tA"),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    // Locked-shape contract: adopter clients can rely on these keys.
    assert!(p["error"].is_string());
    assert!(p["idempotency_key"].is_string());
    assert!(p["endpoint"].is_string());
    assert!(p["method"].is_string());
    assert!(p["path"].is_string());
    assert!(p["cached_body_hash_prefix"].is_string());
    assert!(p["hint"].is_string());
    assert_eq!(p["d_letter"], "D7");
}

// ─── Body validation gate fires BEFORE idempotency ──────────────────

#[tokio::test]
async fn body_schema_violation_fires_before_idempotency_caching() {
    // 32.c body validation must run BEFORE 32.f caching — a malformed
    // body 400's out of the request pipeline, so no entry gets cached.
    // If a subsequent request with a corrected body arrives with the
    // same key, it should EXECUTE (not replay the cached error).
    let src = "type Req { amount: Integer }\n\
               flow Charge() -> String { let result = \"ok\" return result }\n\
               axonendpoint ChargeEndpoint { public: true method: POST path: \"/charge2\" body: Req execute: Charge }";
    let app = build_router(server_cfg());
    deploy(app.clone(), src).await;

    // Malformed body → 400 body_schema_violation.
    let req1 = post_with(
        "/charge2",
        &serde_json::json!({"amount": "not-int"}),
        Some("compose-key"),
        Some("Bearer tA"),
    );
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::BAD_REQUEST);
    let bytes1 = resp1.into_body().collect().await.unwrap().to_bytes();
    let p1: serde_json::Value = serde_json::from_slice(&bytes1).unwrap();
    assert_eq!(p1["error"], "body_schema_violation");

    // Same key, NEW well-formed body → must execute (not replay the 400).
    let req2 = post_with(
        "/charge2",
        &serde_json::json!({"amount": 42}),
        Some("compose-key"),
        Some("Bearer tA"),
    );
    let resp2 = app.oneshot(req2).await.unwrap();
    // The runtime treats the well-formed second request as a fresh
    // execution because the malformed first request never reached the
    // idempotency cache write step. Status is whatever the flow
    // produces; what matters is it's NOT 400 + body_schema_violation
    // AND NOT 422 conflict (we don't conflate a rejected request body
    // with the cache key's "original" body).
    let status2 = resp2.status();
    let bytes2 = resp2.into_body().collect().await.unwrap().to_bytes();
    let p2: serde_json::Value = serde_json::from_slice(&bytes2).unwrap_or_default();
    assert_ne!(p2["error"], "body_schema_violation");
    assert_ne!(p2["error"], "idempotency_key_reused_with_different_request");
    assert!(
        status2.is_success(),
        "well-formed second request should succeed (got status {status2}, body {p2})"
    );
}
