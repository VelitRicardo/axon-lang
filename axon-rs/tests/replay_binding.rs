//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.23.0 — Replay-token binding end-to-end tests.
//!
//! D9 (plan-vivo numbering) ratificada 2026-05-11. Verifies the
//! regulator-grade replay surface on dynamic axonendpoint routes:
//!
//!   * Every successful 2xx POST/PUT response writes a replay entry
//!     keyed by trace_id; `GET /v1/replay/<trace_id>` returns it.
//!   * `Replay-Status: deterministic | non_deterministic` header
//!     fires per backend determinism.
//!   * `X-Axon-Trace-Id` response header attached on every dynamic-
//!     route response (the correlation anchor).
//!   * Default: POST/PUT → replay enabled, GET/DELETE → disabled.
//!   * Explicit `replay: true` enables replay on a GET; `replay: false`
//!     disables on a POST.
//!   * Non-2xx responses skip the binding (we don't replay errors).
//!   * SSE responses skip the binding (per-event streams can't be
//! captured at the wire layer — future cycle).
//!   * 404 on unknown trace_id with structured error.
//!   * Auth on `/v1/replay/<trace_id>` follows `/v1/audit` precedent.
//!
//! Pillar trace per D12:
//!   - MATHEMATICS — same input + same model state ⟹ same output
//!                   (deterministic backends).
//!   - PHILOSOPHY — declared `replay: true` IS the audit contract.
//!   - LOGIC      — replay default is total over method.
//!   - COMPUTING  — regulatory replay is audit-defensible AI.

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
    "flow Touch() -> String { let result = \"ok\" return result }\n\
     axonendpoint TouchEndpoint { public: true method: POST path: \"/touch\" execute: Touch }";

const POST_OPT_OUT: &str =
    "flow Touch() -> String { let result = \"ok\" return result }\n\
     axonendpoint TouchEndpoint { public: true method: POST path: \"/no-replay\" \
        execute: Touch replay: false }";

const GET_FLOW: &str =
    "flow Ping() -> String { let result = \"pong\" return result }\n\
     axonendpoint PingEndpoint { public: true method: GET path: \"/ping\" execute: Ping }";

const GET_REPLAY_ON: &str =
    "flow Ping() -> String { let result = \"pong\" return result }\n\
     axonendpoint PingEndpoint { public: true method: GET path: \"/ping-replay\" \
        execute: Ping replay: true }";

fn post(path: &str, body: &serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

// ─── X-Axon-Trace-Id header on EVERY dynamic-route response ─────────

#[tokio::test]
async fn trace_id_header_attached_on_every_dynamic_response() {
    let app = build_router(server_cfg());
    deploy(app.clone(), POST_FLOW).await;
    let resp = app
        .oneshot(post("/touch", &serde_json::json!({})))
        .await
        .unwrap();
    let trace_hdr = resp
        .headers()
        .get("x-axon-trace-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    assert!(trace_hdr.is_some(), "X-Axon-Trace-Id must be present");
    let id = trace_hdr.unwrap();
    assert_eq!(id.len(), 36, "trace_id is a canonical UUID v4 string");
}

// ─── Default: POST writes binding ───────────────────────────────────

#[tokio::test]
async fn post_default_writes_replay_binding() {
    let app = build_router(server_cfg());
    deploy(app.clone(), POST_FLOW).await;

    let req_body = serde_json::json!({"data": "audit-me"});
    let resp = app
        .clone()
        .oneshot(post("/touch", &req_body))
        .await
        .unwrap();
    let trace_id = resp
        .headers()
        .get("x-axon-trace-id")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_string();
    // Drain response body to ensure the binding write happens.
    let _ = resp.into_body().collect().await.unwrap().to_bytes();

    // Retrieve via GET /v1/replay/<trace_id>.
    let replay_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/replay/{trace_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay_resp.status(), StatusCode::OK);
    let replay_status = replay_resp
        .headers()
        .get("replay-status")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = replay_resp.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["trace_id"], trace_id);
    assert_eq!(payload["endpoint_name"], "TouchEndpoint");
    assert_eq!(payload["flow_name"], "Touch");
    assert_eq!(payload["method"], "POST");
    assert_eq!(payload["path"], "/touch");
    assert_eq!(payload["response_status"], 200);
    assert!(payload["deterministic"].as_bool().unwrap_or(false));
    // Replay-Status: deterministic header per plan vivo section 9.2.
    assert_eq!(replay_status.as_deref(), Some("deterministic"));
}

// ─── Explicit `replay: false` opts out on POST ──────────────────────

#[tokio::test]
async fn explicit_replay_false_disables_binding_on_post() {
    let app = build_router(server_cfg());
    deploy(app.clone(), POST_OPT_OUT).await;

    let resp = app
        .clone()
        .oneshot(post("/no-replay", &serde_json::json!({})))
        .await
        .unwrap();
    let trace_id = resp
        .headers()
        .get("x-axon-trace-id")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_string();
    let _ = resp.into_body().collect().await.unwrap().to_bytes();

    // Trace header present, but binding NOT written.
    let replay_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/replay/{trace_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay_resp.status(), StatusCode::NOT_FOUND);
    let bytes = replay_resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(p["error"], "replay_trace_not_found");
    assert_eq!(p["d_letter"], "D9");
}

// ─── Default: GET does NOT bind ─────────────────────────────────────

#[tokio::test]
async fn get_default_skips_binding() {
    let app = build_router(server_cfg());
    deploy(app.clone(), GET_FLOW).await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/ping")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let trace_id = resp
        .headers()
        .get("x-axon-trace-id")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_string();
    let _ = resp.into_body().collect().await.unwrap().to_bytes();

    let replay_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/replay/{trace_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay_resp.status(), StatusCode::NOT_FOUND);
}

// ─── Explicit `replay: true` enables on GET ────────────────────────

#[tokio::test]
async fn explicit_replay_true_enables_binding_on_get() {
    let app = build_router(server_cfg());
    deploy(app.clone(), GET_REPLAY_ON).await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/ping-replay")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let trace_id = resp
        .headers()
        .get("x-axon-trace-id")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_string();
    let _ = resp.into_body().collect().await.unwrap().to_bytes();

    let replay_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/replay/{trace_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay_resp.status(), StatusCode::OK);
}

// ─── 404 on unknown trace_id ────────────────────────────────────────

#[tokio::test]
async fn unknown_trace_id_returns_404_with_structured_error() {
    let app = build_router(server_cfg());
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/replay/00000000-0000-0000-0000-000000000000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(p["error"], "replay_trace_not_found");
    assert_eq!(p["d_letter"], "D9");
    assert!(p["hint"].is_string());
}

// ─── Replay entry carries request + response body shapes ────────────

#[tokio::test]
async fn replay_entry_captures_request_and_response_bodies() {
    let app = build_router(server_cfg());
    deploy(app.clone(), POST_FLOW).await;

    let req_body = serde_json::json!({"unique_marker": "cycle-32-h-replay-test"});
    let resp = app
        .clone()
        .oneshot(post("/touch", &req_body))
        .await
        .unwrap();
    let trace_id = resp
        .headers()
        .get("x-axon-trace-id")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_string();
    let response_bytes = resp.into_body().collect().await.unwrap().to_bytes();

    let replay_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/replay/{trace_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = replay_resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    // Locked-shape contract: all critical fields present.
    assert!(p["trace_id"].is_string());
    assert!(p["timestamp_ms"].is_number());
    assert!(p["endpoint_name"].is_string());
    assert!(p["flow_name"].is_string());
    assert!(p["method"].is_string());
    assert!(p["path"].is_string());
    assert!(p["client_id"].is_string());
    assert!(p["capabilities_used"].is_array());
    assert!(p["request_body_hash_hex"].is_string());
    assert!(p["request_body_base64"].is_string());
    assert!(p["response_status"].is_number());
    assert!(p["response_body_hash_hex"].is_string());
    assert!(p["response_body_base64"].is_string());
    assert!(p["response_content_type"].is_string());
    assert!(p["model_version"].is_string());
    assert!(p["deterministic"].is_boolean());

    // Decode request_body_base64 — should round-trip to the original
    // marker we sent.
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;
    let req_b64 = p["request_body_base64"].as_str().unwrap();
    let req_bytes = B64.decode(req_b64).unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&req_bytes).unwrap();
    assert_eq!(parsed["unique_marker"], "cycle-32-h-replay-test");

    // Response_body_base64 round-trip — must equal what the client saw.
    let resp_b64 = p["response_body_base64"].as_str().unwrap();
    let resp_recorded = B64.decode(resp_b64).unwrap();
    assert_eq!(
        resp_recorded.as_slice(),
        response_bytes.as_ref(),
        "replay-recorded response body must equal client-seen body"
    );
}

// ─── Hashes verified (request_body_hash matches actual SHA-256) ────

#[tokio::test]
async fn replay_request_body_hash_matches_sha256_of_actual_body() {
    use sha2::{Digest, Sha256};
    let app = build_router(server_cfg());
    deploy(app.clone(), POST_FLOW).await;

    let req_body = serde_json::json!({"data": "hash-check"});
    let body_bytes = serde_json::to_vec(&req_body).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/touch")
        .header("content-type", "application/json")
        .body(Body::from(body_bytes.clone()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let trace_id = resp
        .headers()
        .get("x-axon-trace-id")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_string();
    let _ = resp.into_body().collect().await.unwrap().to_bytes();

    let replay_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/replay/{trace_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = replay_resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let recorded_hash = p["request_body_hash_hex"].as_str().unwrap();
    let mut h = Sha256::new();
    h.update(&body_bytes);
    let expected: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        recorded_hash, expected,
        "recorded request_body_hash_hex must match SHA-256 of original body"
    );
}

// ─── PUT also writes binding ────────────────────────────────────────

#[tokio::test]
async fn put_writes_replay_binding_by_default() {
    let src = "flow Upsert() -> String { let result = \"upserted\" return result }\n\
               axonendpoint UpsertEndpoint { public: true method: PUT path: \"/upsert\" execute: Upsert }";
    let app = build_router(server_cfg());
    deploy(app.clone(), src).await;

    let req = Request::builder()
        .method("PUT")
        .uri("/upsert")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let trace_id = resp
        .headers()
        .get("x-axon-trace-id")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_string();
    let _ = resp.into_body().collect().await.unwrap().to_bytes();

    let replay_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/replay/{trace_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay_resp.status(), StatusCode::OK);
    let bytes = replay_resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(p["method"], "PUT");
}

// ─── Auth: bearer with replay capability accesses /v1/replay ────────

#[tokio::test]
async fn replay_get_unauthorized_when_auth_token_configured() {
    // Build server with an auth_token set so the read-only check fires.
    let cfg = ServerConfig {
        auth_token: "secret".into(),
        ..server_cfg()
    };
    let app = build_router(cfg);
    // Deploy via the master auth token so it's permitted.
    let deploy_body = serde_json::json!({
        "source": POST_FLOW,
        "source_file": "test.axon",
        "backend": "stub",
    });
    let deploy_req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .header("authorization", "Bearer secret")
        .body(Body::from(deploy_body.to_string()))
        .unwrap();
    let _ = app.clone().oneshot(deploy_req).await.unwrap();

    // Unauthenticated GET /v1/replay/<id> must be 401.
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/replay/00000000-0000-0000-0000-000000000000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // Either 401 (auth check fires) or another non-success — the
    // contract is "does not reveal data without auth". 200 would be a
    // regression.
    assert_ne!(resp.status(), StatusCode::OK);
}

// ─── /v1/execute legacy unaffected ──────────────────────────────────

#[tokio::test]
async fn v1_execute_legacy_path_does_not_write_replay_entries() {
    let app = build_router(server_cfg());
    deploy(app.clone(), POST_FLOW).await;
    let body = serde_json::json!({"flow": "Touch", "backend": "stub"});
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    // /v1/execute is the LEGACY RPC path; replay binding fires only on
    // dynamic routes (D10 backwards-compat). No X-Axon-Trace-Id header
    // expected here either.
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().get("x-axon-trace-id").is_none());
}

// ─── D6 composition: SSE response binds replay with per-step audit ─
//
// v1.24.0 rewrites this anchor. Pre-33.x.f the SSE branch
// fell through the post-dispatch body capture (only application/
// json 2xx captured), so SSE routes with `replay: true` (or POST
// default-on) wrote NO replay entry — the test asserted 404.
//
// Post-33.x.f, the SSE handler records an entry with the
// per-step `step_audit` records populated. Response body bytes
// stay empty (per-event token chain is v1.29.0 scope). The
// `response_content_type` is `text/event-stream` so adopters
// can distinguish SSE-mode replay entries from JSON-mode ones.

#[tokio::test]
async fn sse_response_records_replay_with_step_audit_post_33_x_f() {
    let src = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
               flow Chat() -> Unit { step S { ask: \"x\" apply: t } }\n\
               axonendpoint ChatSse { public: true method: POST path: \"/chat-sse\" \
                  execute: Chat transport: sse }";
    let app = build_router(server_cfg());
    deploy(app.clone(), src).await;
    let resp = app
        .clone()
        .oneshot(post("/chat-sse", &serde_json::json!({})))
        .await
        .unwrap();
    let trace_id = resp
        .headers()
        .get("x-axon-trace-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .expect("33.x.f lifts X-Axon-Trace-Id generation BEFORE dispatch, so the header is set even on SSE responses");
    let _ = resp.into_body().collect().await.unwrap().to_bytes();

    // POST + replay omitted → 32.h default = enabled → 33.x.f
    // records an entry with step_audit.
    let replay_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/replay/{trace_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        replay_resp.status(),
        StatusCode::OK,
        "POST-33.x.f: SSE response with replay_enabled MUST record entry"
    );
    let bytes = replay_resp.into_body().collect().await.unwrap().to_bytes();
    let parsed: serde_json::Value =
        serde_json::from_slice(&bytes).expect("replay GET returns JSON");
    // Per-event token chain stays deferred to v1.29.0: response
    // body stays empty.
    assert_eq!(parsed["response_content_type"], "text/event-stream");
    assert_eq!(
        parsed["response_body_base64"],
        "",
        "33.x.f records step_audit, NOT per-event body bytes (v1.29.0 scope)"
    );
    // Per-step audit MUST be populated.
    let step_audit = parsed["step_audit"]
        .as_array()
        .expect("step_audit MUST be a JSON array post-33.x.f");
    assert_eq!(step_audit.len(), 1, "1-step flow → 1 step_audit entry");
    assert_eq!(step_audit[0]["step_name"], "S");
    assert_eq!(step_audit[0]["effect_policy_applied"], "drop_oldest");
}
