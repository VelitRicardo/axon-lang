//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 33.b — Anchor regression test for the stub-backend
//! steps_executed:0 hollow-wire bug closed by Layer 1.
//!
//! Pre-fix `execute_server_flow` called `execute_stub` but never
//! populated the ReportBuilder for the stub path (CLI path did the
//! population at the call site but the server path didn't). Result:
//! every dynamic-route SSE response over the stub backend served
//! `steps_executed: 0` and `step:""`/`token:""` in the axon.token
//! event — the hollow shape the adopter trail surfaced.
//!
//! Post-fix the server path mirrors the CLI path: after execute_stub
//! returns, the report is populated with one StepReport per step,
//! `result: "(stub)"` placeholder. The SSE wire now shows
//! `steps_executed >= 1` and the step name + sentinel token content.
//!
//! This file's purpose:
//!   1. Lock the fix as a regression anchor — if anyone refactors
//!      `execute_server_flow` and accidentally re-drops the stub
//!      report population, this test fires immediately.
//!   2. Document the new wire shape (Layer 1 closed) so subsequent
//!      Fase 33 sub-fases (33.c live forwarding, 33.d real backend
//!      streaming, 33.e effect-handler dispatch) have a clear before/
//!      after baseline.

use axon::axon_server::{build_router, ServerConfig};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn server_cfg(strict: bool) -> ServerConfig {
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
        strict_type_driven_transport: strict,
        default_backend: None,
        schemas_dir: None,
    }
}

async fn deploy(app: axum::Router, src: &str) {
    let body = serde_json::json!({
        "source": src,
        "source_file": "33b_anchor.axon",
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

const STUB_FLOW_SOURCE: &str =
    "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat }";

// ─── §1 — /v1/execute (JSON) path reports steps_executed >= 1 ──────

#[tokio::test]
async fn v1_execute_stub_backend_reports_nonzero_steps_executed() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STUB_FLOW_SOURCE).await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({"flow": "Chat", "backend": "stub"}).to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    // Pre-fix this was 0 — the report was empty for stub path.
    // v2.0.0 FlowEnvelope wire contract: step metrics are nested under
    // the `step_audit` slot (no top-level `steps_executed`/`step_names`).
    let step_audit = &payload["step_audit"];
    let steps = step_audit["steps_executed"].as_u64().unwrap_or(99);
    assert!(
        steps >= 1,
        "steps_executed must be >= 1 for the stub path (got {steps}); regression of Fase 33.b Layer 1 fix"
    );
    let step_names = step_audit["step_names"].as_array().expect("step_names array");
    assert!(
        !step_names.is_empty(),
        "step_names must be non-empty for the stub path; regression of Fase 33.b Layer 1 fix"
    );
    assert_eq!(
        step_names[0].as_str().unwrap_or(""),
        "Generate",
        "step_names[0] MUST carry the source-declared step name"
    );
}

// ─── §2 — SSE wire body now shows non-hollow step + non-zero count ──

#[tokio::test]
async fn dynamic_route_sse_wire_body_carries_step_name_and_count() {
    let app = build_router(server_cfg(true));
    deploy(app.clone(), STUB_FLOW_SOURCE).await;
    let req = Request::builder()
        .method("POST")
        .uri("/chat")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(ct.starts_with("text/event-stream"));
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes);

    // Anchor #1: the wire body must contain at least one axon.token
    // event carrying the step name "Generate" (not the empty
    // sentinel that pre-33.b yielded).
    assert!(
        body.contains(r#""step":"Generate""#),
        "Wire body MUST carry step:'Generate' in at least one axon.token \
         event after the Fase 33.b Layer 1 fix. Body:\n{body}"
    );
    // Anchor #2: the wire body must contain a non-empty token field
    // matching the stub placeholder. Real backends (Fase 33.d) will
    // replace `(stub)` with actual chunk content.
    assert!(
        body.contains(r#""token":"(stub)""#),
        "Wire body MUST carry token:'(stub)' for stub-backend dispatch \
         after the Fase 33.b Layer 1 fix. Body:\n{body}"
    );
    // Anchor #3: the axon.complete envelope reports steps_executed >= 1.
    assert!(
        !body.contains(r#""steps_executed":0"#),
        "axon.complete envelope MUST NOT report steps_executed:0 for a \
         flow that has steps; regression of Fase 33.b Layer 1 fix. \
         Body:\n{body}"
    );
    assert!(
        body.contains(r#""steps_executed":1"#),
        "axon.complete envelope MUST report steps_executed:1 for the 1-step \
         canonical Kivi shape. Body:\n{body}"
    );
}

// ─── §3 — Multi-step flow: each step shows up in the wire ──────────

const MULTI_STEP_SOURCE: &str =
    "flow Pipeline() -> Unit {\n\
        step Plan { ask: \"plan\" output: Stream<Token> }\n\
        step Execute { ask: \"do\" output: Stream<Token> }\n\
        step Audit { ask: \"verify\" output: Stream<Token> }\n\
     }\n\
     axonendpoint PipelineEndpoint { public: true method: POST path: \"/pipe\" execute: Pipeline transport: sse }";

#[tokio::test]
async fn multi_step_flow_each_step_visible_in_wire() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), MULTI_STEP_SOURCE).await;
    let req = Request::builder()
        .method("POST")
        .uri("/pipe")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes);

    for step_name in &["Plan", "Execute", "Audit"] {
        assert!(
            body.contains(&format!(r#""step":"{step_name}""#)),
            "Wire body MUST contain step:'{step_name}' in at least one event. Body:\n{body}"
        );
    }
    assert!(
        body.contains(r#""steps_executed":3"#),
        "Multi-step flow MUST report steps_executed:3. Body:\n{body}"
    );
}

// ─── §4 — Audit trail records the trace ────────────────────────────

#[tokio::test]
async fn audit_log_records_execute_trace_even_for_stub_path() {
    // Pre-33.b the stub path had steps_executed:0 in the audit log
    // detail too. Post-fix the trace record reflects the actual
    // step count + names — auditors get the real number even on
    // stub deployments.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STUB_FLOW_SOURCE).await;
    let _exec = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/execute")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"flow": "Chat", "backend": "stub"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let audit_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/audit")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = audit_resp.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    let entries = payload["entries"].as_array().expect("entries");
    let execute_entry = entries
        .iter()
        .find(|e| e["action"] == "execute" && e["target"] == "Chat")
        .expect("audit log must contain the execute entry");
    assert!(
        execute_entry["success"]
            .as_bool()
            .expect("success bool"),
        "execute trace must report success=true for the stub path"
    );
}
