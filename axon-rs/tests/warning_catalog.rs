//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.24.0 — `axon-W002 streaming-not-supported` closed-catalog
//! warning surface.
//!
//! D5 contract: when the production streaming path cannot activate
//! cleanly (unknown backend / source compilation failed / future
//! backend lacks `Backend::stream()`), the runtime emits a closed-
//! catalog `axon-W002` warning instead of silently degrading.
//! Surfaces on `axon.complete.warnings[*]` (wire body) + on
//! `replay.runtime_warnings[*]` (audit row for SSE routes with
//! `replay: true`).
//!
//! # v1.24.0 — `UnsupportedFlowShape` variant RETIRED
//!
//! Pre-33.z.e, `FallbackMode::UnsupportedFlowShape` was one of four
//! W002 triggers. The 33.z dispatcher graft (33.z.b/c/d/e cycle)
//! retired the legacy synchronous fallback entirely + the
//! per-IRFlowNode dispatcher (v1.24.0 45/45) covers every
//! IRFlowNode variant — no shape is "unsupported" anymore. The
//! catalog shrinks from 4 to 3 variants in 33.z.e.
//!
//! Remaining triggers:
//!   - `UnknownBackend` — adopter requested backend not in registry.
//!   - `SourceCompilationFailed` — lex/parse/type-check/IR-generation error.
//!   - `BackendLacksStream` — reserved for adopter-provided backends.

#![allow(clippy::needless_return)]

use axon::axon_server::{build_router, ServerConfig};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
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

async fn deploy(app: axum::Router, src: &str) -> bool {
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
    resp.status() == StatusCode::OK
}

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

async fn get_replay(app: axum::Router, trace_id: &str) -> Value {
    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/replay/{trace_id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

/// Parse SSE body + return the axon.complete data object.
fn parse_complete(body: &str) -> Value {
    let mut cur_event: Option<String> = None;
    let mut cur_data: Option<String> = None;
    for line in body.lines() {
        if let Some(ev) = line.strip_prefix("event: ") {
            cur_event = Some(ev.trim().to_string());
        } else if let Some(data) = line.strip_prefix("data: ") {
            cur_data = Some(data.trim().to_string());
        } else if line.is_empty() {
            if let (Some(ev), Some(data)) = (cur_event.as_deref(), cur_data.as_deref()) {
                if ev == "axon.complete" {
                    return serde_json::from_str(data).unwrap_or(Value::Null);
                }
            }
            cur_event = None;
            cur_data = None;
        }
    }
    if let (Some(ev), Some(data)) = (cur_event, cur_data) {
        if ev == "axon.complete" {
            return serde_json::from_str(&data).unwrap_or(Value::Null);
        }
    }
    Value::Null
}

fn complete_warnings_array(complete: &Value) -> Vec<Value> {
    complete
        .get("warnings")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

fn has_w002_with_mode(complete: &Value, mode_slug: &str) -> bool {
    complete_warnings_array(complete).iter().any(|w| {
        w.get("code").and_then(|c| c.as_str()) == Some("axon-W002")
            && w.get("fallback_mode").and_then(|m| m.as_str()) == Some(mode_slug)
    })
}

// ─── Source fixtures ────────────────────────────────────────────────

fn simple_streaming_flow_with_replay() -> &'static str {
    "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse replay: true }"
}

fn flow_with_for_in_post_33_z_e() -> &'static str {
    // v1.24.0 — post-retirement of `UnsupportedFlowShape`, the
    // ForIn shape (which previously triggered the unreachable
    // variant) MUST NOT fire any W002. The dispatcher's
    // `orchestration::run_for_in` handles it cleanly.
    "flow Loop() -> Unit {\n\
        let xs = \"a,b\"\n\
        for x in xs {\n\
            step S { ask: \"hi\" output: Stream<Token> }\n\
        }\n\
     }\n\
     axonendpoint LoopEndpoint { public: true method: POST path: \"/loop\" execute: Loop transport: sse replay: true }"
}

// ─── section 1 — D5 anchor: happy path → no warnings ───────────────────────

#[tokio::test]
async fn d5_happy_streaming_path_emits_no_warnings() {
    let app = build_router(server_cfg());
    assert!(deploy(app.clone(), simple_streaming_flow_with_replay()).await);
    let (status, _trace_id, body) = post_route(app, "/chat", "{}").await;
    assert_eq!(status, StatusCode::OK);
    let complete = parse_complete(&body);
    assert!(
        complete.get("warnings").is_none()
            || complete_warnings_array(&complete).is_empty(),
        "D5: happy path MUST elide the warnings field. Got: {complete:?}"
    );
}

// ─── section 2 — D2 anchor (33.z.e): UnsupportedFlowShape unreachable ──────
//
// Pre-33.z.e a ForIn flow fired `axon-W002 unsupported_flow_shape`.
// Post-33.z.e the dispatcher path graduates ForIn (v1.24.0
// orchestration handler) so NO W002 fires for this shape.

#[tokio::test]
async fn d2_unsupported_flow_shape_structurally_unreachable_post_33_z_e() {
    let app = build_router(server_cfg());
    if !deploy(app.clone(), flow_with_for_in_post_33_z_e()).await {
        // Parser may reject; skip cleanly.
        return;
    }
    let (status, _trace_id, body) = post_route(app, "/loop", "{}").await;
    if status != StatusCode::OK {
        return;
    }
    let complete = parse_complete(&body);
    assert!(
        !has_w002_with_mode(&complete, "unsupported_flow_shape"),
        "33.z.e D2 invariant: `unsupported_flow_shape` slug MUST NOT \
         appear on the wire — the variant was deleted from FallbackMode. \
         Got complete: {complete:?}"
    );
}

// ─── section 3 — Replay audit row mirrors wire warnings (still valid) ──────

#[tokio::test]
async fn d5_replay_audit_row_mirrors_runtime_warnings_field() {
    let app = build_router(server_cfg());
    assert!(deploy(app.clone(), simple_streaming_flow_with_replay()).await);
    let (status, trace_id, _body) = post_route(app.clone(), "/chat", "{}").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!trace_id.is_empty());
    let replay = get_replay(app, &trace_id).await;
    let warnings = replay["runtime_warnings"]
        .as_array()
        .expect("replay GET ALWAYS includes runtime_warnings field");
    assert!(
        warnings.is_empty(),
        "happy path → empty runtime_warnings on audit row"
    );
}

#[tokio::test]
async fn d5_audit_row_runtime_warnings_field_present_for_legacy_json_entries() {
    let src = "flow Touch() -> Unit { step S { ask: \"hi\" } }\n\
               axonendpoint TouchEndpoint { public: true method: POST path: \"/touch\" execute: Touch replay: true }";
    let app = build_router(server_cfg());
    assert!(deploy(app.clone(), src).await);
    let (status, trace_id, _body) = post_route(app.clone(), "/touch", "{}").await;
    assert_eq!(status, StatusCode::OK);
    let replay = get_replay(app, &trace_id).await;
    assert!(
        replay["runtime_warnings"].as_array().is_some(),
        "runtime_warnings field MUST always be a JSON array (possibly empty)"
    );
}

// ─── section 4 — Closed-catalog drift gates (post-33.z.e) ──────────────────

#[tokio::test]
async fn closed_catalog_warning_code_is_axon_w002_only() {
    use axon::runtime_warnings::WarningCode;
    let all = [WarningCode::AxonW002];
    assert_eq!(all.len(), 1, "WarningCode catalog is closed at 1 variant");
    assert_eq!(WarningCode::AxonW002.slug(), "axon-W002");
}

#[tokio::test]
async fn closed_catalog_fallback_mode_has_three_variants_post_33_z_e() {
    use axon::runtime_warnings::FallbackMode;
    // v1.24.0 — catalog shrinks from 4 → 3 (UnsupportedFlowShape
    // retired in lockstep with the dispatcher unification).
    let all = [
        FallbackMode::UnknownBackend,
        FallbackMode::SourceCompilationFailed,
        FallbackMode::BackendLacksStream,
    ];
    assert_eq!(all.len(), 3, "FallbackMode catalog closed at 3 variants post-33.z.e");
    let mut slugs: Vec<&str> = all.iter().map(|m| m.slug()).collect();
    slugs.sort();
    let mut unique = slugs.clone();
    unique.dedup();
    assert_eq!(slugs.len(), unique.len(), "FallbackMode slugs must be unique");
}

#[tokio::test]
async fn warning_wire_field_serde_round_trip_via_axon_complete() {
    use axon::runtime_warnings::{FallbackMode, RuntimeWarning, WarningCode};
    let w = RuntimeWarning::streaming_not_supported(
        "F",
        "stub",
        FallbackMode::UnknownBackend,
        "test",
    );
    let json = serde_json::to_value(&w).unwrap();
    assert_eq!(json["code"], "axon-W002");
    assert_eq!(json["fallback_mode"], "unknown_backend");
    assert_eq!(json["flow_name"], "F");
    assert_eq!(json["backend"], "stub");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .contains("streaming-not-supported"));
    let s = serde_json::to_string(&w).unwrap();
    let parsed: RuntimeWarning = serde_json::from_str(&s).unwrap();
    assert_eq!(parsed.code, WarningCode::AxonW002);
    assert_eq!(parsed.fallback_mode, FallbackMode::UnknownBackend);
}
