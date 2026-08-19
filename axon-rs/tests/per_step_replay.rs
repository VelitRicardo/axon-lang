//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.24.0 — Per-step audit trace in `/v1/replay/<trace_id>`.
//!
//! D6 in motion: for SSE routes whose axonendpoint declared
//! `replay: true`, the replay log now records ONE entry with a
//! populated `step_audit: Vec<StepAuditRecord>` — auditors retrieve
//! the per-step sequence (step_name + tokens_emitted + output_hash +
//! effect_policy_applied + chunks_dropped + chunks_degraded +
//! timestamp_ms) rather than just the final response.
//!
//! # Required for regulated verticals
//!
//! - **Banking** (PCI DSS Req 10) — per-step output_hash chain
//! - **Government** (FedRAMP AU-2) — FOIA per-step reasoning capture
//! - **Legal** (FRE 502) — per-step privilege-assessment for appeals
//! - **Medicine** (21 CFR Part 11 section 11.10) — per-step CDS provenance
//!
//! Per-token chain signature (each `axon.token` cryptographically
//! chained) stays deferred to v1.29.0.
//!
//! # Test surface
//!
//! - SSE route with `replay: true` → GET /v1/replay/<uuid> returns
//!   step_audit with N entries for N-step flow.
//! - SSE route with `replay: false` declaration → no replay entry
//!   recorded (404 on GET).
//! - SSE route without explicit `replay:` on POST method → replay
//! enabled by v1.23.0 default (POST default-on).
//! - GET method default → no replay (32.h default).
//! - /v1/execute/sse direct hit (no axonendpoint binding) → no
//!   replay entry (33.x.f doesn't add a binding for this path).
//! - step_audit field shape: each entry has all 8 declared fields.
//! - output_hash_hex is SHA-256 of accumulated chunk delta.
//! - effect_policy_applied surfaces declared `<stream:<policy>>`.
//! - Multi-step flow: records arrive in step-execution order.

#![allow(clippy::needless_return)]

use axon::axon_server::{build_router, ServerConfig};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

fn hex_of(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

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
    assert_eq!(resp.status(), StatusCode::OK);
}

/// POST a route + return (status, X-Axon-Trace-Id header, body).
async fn post_route(
    app: axum::Router,
    path: &str,
    body: &str,
) -> (StatusCode, String, String) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let trace_id = resp
        .headers()
        .get("x-axon-trace-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes).to_string();
    (status, trace_id, body)
}

async fn get_replay(
    app: axum::Router,
    trace_id: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/replay/{trace_id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes).to_string();
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    (status, v)
}

// Canonical adopter sources
fn single_step_with_replay_true() -> String {
    "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse replay: true }"
        .to_string()
}

fn single_step_with_drop_oldest_effect() -> String {
    "tool tk { description: \"stream\" effects: <stream:drop_oldest> }\n\
     flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" apply: tk }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse replay: true }"
        .to_string()
}

fn multi_step_with_replay_true() -> String {
    "flow Chain() -> Unit {\n\
        step First { ask: \"one\" output: Stream<Token> }\n\
        step Second { ask: \"two\" output: Stream<Token> }\n\
        step Third { ask: \"three\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChainEndpoint { public: true method: POST path: \"/chain\" execute: Chain transport: sse replay: true }"
        .to_string()
}

fn single_step_with_replay_false() -> String {
    "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse replay: false }"
        .to_string()
}

fn single_step_post_default_replay() -> String {
    // POST without explicit `replay:` → 32.h default = enabled.
    "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse }"
        .to_string()
}

// ─── section 1 — D6 anchor: per-step records on SSE replay ─────────────────

#[tokio::test]
async fn d6_sse_route_with_replay_true_returns_step_audit_per_step() {
    let app = build_router(server_cfg());
    deploy(app.clone(), &single_step_with_replay_true()).await;
    let (status, trace_id, _body) = post_route(app.clone(), "/chat", "{}").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!trace_id.is_empty(), "X-Axon-Trace-Id header MUST be present");

    let (get_status, replay) = get_replay(app, &trace_id).await;
    assert_eq!(get_status, StatusCode::OK);
    let step_audit = replay["step_audit"]
        .as_array()
        .expect("step_audit MUST be an array (33.x.f)");
    assert_eq!(step_audit.len(), 1, "single-step flow → exactly 1 step_audit entry");

    let entry = &step_audit[0];
    assert_eq!(entry["step_name"], "Generate");
    assert_eq!(entry["step_index"].as_u64(), Some(0));
    assert_eq!(entry["success"], true);
    assert_eq!(entry["tokens_emitted"].as_u64(), Some(1));
    assert!(
        entry["output_hash_hex"].as_str().unwrap_or("").len() == 64,
        "SHA-256 hex MUST be 64 chars; got {}",
        entry["output_hash_hex"].as_str().unwrap_or("").len()
    );
    assert_eq!(entry["effect_policy_applied"], Value::Null);
    assert_eq!(entry["chunks_dropped"].as_u64(), Some(0));
    assert_eq!(entry["chunks_degraded"].as_u64(), Some(0));
    assert!(entry["timestamp_ms"].as_u64().unwrap_or(0) > 0);
}

#[tokio::test]
async fn d6_multi_step_flow_records_per_step_in_order() {
    let app = build_router(server_cfg());
    deploy(app.clone(), &multi_step_with_replay_true()).await;
    let (_status, trace_id, _) = post_route(app.clone(), "/chain", "{}").await;

    let (get_status, replay) = get_replay(app, &trace_id).await;
    assert_eq!(get_status, StatusCode::OK);
    let step_audit = replay["step_audit"].as_array().unwrap();
    assert_eq!(step_audit.len(), 3);
    assert_eq!(step_audit[0]["step_name"], "First");
    assert_eq!(step_audit[1]["step_name"], "Second");
    assert_eq!(step_audit[2]["step_name"], "Third");
    assert_eq!(step_audit[0]["step_index"].as_u64(), Some(0));
    assert_eq!(step_audit[1]["step_index"].as_u64(), Some(1));
    assert_eq!(step_audit[2]["step_index"].as_u64(), Some(2));
    // timestamps monotone within the flow
    let ts0 = step_audit[0]["timestamp_ms"].as_u64().unwrap();
    let ts1 = step_audit[1]["timestamp_ms"].as_u64().unwrap();
    let ts2 = step_audit[2]["timestamp_ms"].as_u64().unwrap();
    assert!(ts0 <= ts1 && ts1 <= ts2);
}

#[tokio::test]
async fn d6_declared_effect_surfaces_in_step_audit() {
    let app = build_router(server_cfg());
    deploy(app.clone(), &single_step_with_drop_oldest_effect()).await;
    let (_status, trace_id, _) = post_route(app.clone(), "/chat", "{}").await;
    let (_get_status, replay) = get_replay(app, &trace_id).await;
    let step_audit = replay["step_audit"].as_array().unwrap();
    let entry = &step_audit[0];
    assert_eq!(entry["effect_policy_applied"], "drop_oldest");
    // For stub backend (1 chunk), no policy fires.
    assert_eq!(entry["chunks_dropped"].as_u64(), Some(0));
    assert_eq!(entry["chunks_degraded"].as_u64(), Some(0));
}

// ─── section 2 — output_hash_hex correctness ───────────────────────────────

#[tokio::test]
async fn output_hash_hex_is_sha256_of_step_output() {
    let app = build_router(server_cfg());
    deploy(app.clone(), &single_step_with_replay_true()).await;
    let (_status, trace_id, _) = post_route(app.clone(), "/chat", "{}").await;
    let (_get_status, replay) = get_replay(app, &trace_id).await;
    let entry = &replay["step_audit"][0];
    let hash = entry["output_hash_hex"].as_str().unwrap();
    // Stub backend delivers exactly "(stub)" as the accumulated
    // output for the single chunk. SHA-256 over those bytes:
    let mut hasher = Sha256::new();
    hasher.update(b"(stub)");
    let expected_hex = hex_of(&hasher.finalize());
    assert_eq!(hash, expected_hex);
}

// ─── section 3 — D6 anchor: replay: false → no entry ──────────────────────

#[tokio::test]
async fn d6_replay_false_explicit_yields_no_replay_entry() {
    let app = build_router(server_cfg());
    deploy(app.clone(), &single_step_with_replay_false()).await;
    let (_status, trace_id, _) = post_route(app.clone(), "/chat", "{}").await;
    assert!(!trace_id.is_empty(), "X-Axon-Trace-Id header MUST still be present");
    let (get_status, _) = get_replay(app, &trace_id).await;
    assert_eq!(
        get_status,
        StatusCode::NOT_FOUND,
        "explicit replay: false MUST NOT record an entry"
    );
}

#[tokio::test]
async fn d6_post_default_replay_enabled_records_entry() {
    // 32.h default: POST + replay omitted → enabled.
    let app = build_router(server_cfg());
    deploy(app.clone(), &single_step_post_default_replay()).await;
    let (_status, trace_id, _) = post_route(app.clone(), "/chat", "{}").await;
    let (get_status, replay) = get_replay(app, &trace_id).await;
    assert_eq!(get_status, StatusCode::OK);
    assert!(replay["step_audit"].as_array().unwrap().len() == 1);
}

// ─── section 4 — /v1/execute/sse direct hit → no replay entry ─────────────

#[tokio::test]
async fn legacy_v1_execute_sse_path_does_not_record_replay() {
    let app = build_router(server_cfg());
    deploy(app.clone(), &single_step_with_replay_true()).await;
    // Hit /v1/execute/sse directly (bypasses dynamic-route dispatch).
    let body = serde_json::json!({
        "flow_name": "Chat",
        "backend": "stub",
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute/sse")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let trace_hdr = resp
        .headers()
        .get("x-axon-trace-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    // /v1/execute/sse doesn't set X-Axon-Trace-Id (it's a
    // dispatch-handler-level header). Even if present, no replay
    // entry is recorded (legacy path).
    if let Some(tid) = trace_hdr {
        let (get_status, _) = get_replay(app, &tid).await;
        assert_eq!(
            get_status,
            StatusCode::NOT_FOUND,
            "direct /v1/execute/sse path MUST NOT record a replay entry"
        );
    }
}

// ─── section 5 — Wire shape: step_audit array fields ──────────────────────

#[tokio::test]
async fn step_audit_entry_has_all_eight_canonical_fields() {
    let app = build_router(server_cfg());
    deploy(app.clone(), &single_step_with_replay_true()).await;
    let (_status, trace_id, _) = post_route(app.clone(), "/chat", "{}").await;
    let (_get_status, replay) = get_replay(app, &trace_id).await;
    let entry = &replay["step_audit"][0];
    for field in &[
        "step_name",
        "step_index",
        "success",
        "tokens_emitted",
        "output_hash_hex",
        "effect_policy_applied",
        "chunks_dropped",
        "chunks_degraded",
        "timestamp_ms",
    ] {
        assert!(
            entry.get(*field).is_some(),
            "step_audit entry MUST carry field {field:?}; got: {entry:?}"
        );
    }
}

// ─── section 6 — Replay entry top-level fields ─────────────────────────────

#[tokio::test]
async fn replay_get_payload_carries_endpoint_metadata_for_sse_route() {
    let app = build_router(server_cfg());
    deploy(app.clone(), &single_step_with_replay_true()).await;
    let (_status, trace_id, _) = post_route(app.clone(), "/chat", "{}").await;
    let (_get_status, replay) = get_replay(app, &trace_id).await;
    assert_eq!(replay["trace_id"], trace_id);
    assert_eq!(replay["endpoint_name"], "ChatEndpoint");
    assert_eq!(replay["flow_name"], "Chat");
    assert_eq!(replay["method"], "POST");
    assert_eq!(replay["path"], "/chat");
    assert_eq!(replay["response_status"].as_u64(), Some(200));
    assert_eq!(replay["response_content_type"], "text/event-stream");
    assert!(replay["model_version"]
        .as_str()
        .unwrap_or("")
        .starts_with("axon.runtime.dynamic_route.sse"));
}

// ─── section 7 — D6 anchor in real_streaming_diagnostic rewrite ───
//
// This anchor lived in `real_streaming_diagnostic.rs::d6_*`
// and asserted ≤1 row for SSE replay. The 33.x.f commit rewrites
// it to assert N rows for N-step flow with replay: true. The new
// shape is verified by the tests above; the legacy diagnostic is
// updated separately so the closure-proof diff is observable.

#[tokio::test]
async fn d6_anchor_complements_diagnostic_rewrite() {
    let app = build_router(server_cfg());
    deploy(app.clone(), &multi_step_with_replay_true()).await;
    let (_status, trace_id, _) = post_route(app.clone(), "/chain", "{}").await;
    let (_get_status, replay) = get_replay(app, &trace_id).await;
    let n = replay["step_audit"].as_array().unwrap().len();
    // 3-step flow → 3 audit records (not ≤1).
    assert_eq!(n, 3);
}
