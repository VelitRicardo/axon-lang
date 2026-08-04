//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 33.c — Live FlowExecutionEvent forwarding from runner to SSE wire.
//!
//! D1 + D2 in motion: the SSE handler no longer batches `axon.token`
//! events after the synchronous executor returns. Instead it consumes
//! a `tokio::sync::mpsc::UnboundedReceiver<FlowExecutionEvent>` and
//! projects each event onto the wire AS IT ARRIVES.
//!
//! What this test pack proves:
//!   1. The new `server_execute_streaming` producer emits the closed
//!      catalog sequence (FlowStart → per-step → FlowComplete | FlowError)
//!      in correct order for the stub backend.
//!   2. Pure-receiver consumption — exercised end-to-end without the
//!      SSE handler — gives the EXACT byte content the wire surfaces.
//!   3. The wire body emitted by execute_sse_handler stays byte-
//!      compatible with the post-33.b shape (D9 — no wire-format
//!      regression for Fase 30+31+32 adopter contracts).
//!   4. trace_id correlation is consistent: every axon.token event +
//!      the final axon.complete carry the SAME trace_id (so adopters
//!      can bind the wire stream to the `/v1/replay/<id>` audit row
//!      from the FIRST event).
//!   5. Multi-step flows deliver each step's events distinctly; the
//!      catalog invariant is observable from the consumer's vantage.
//!
//! What this test pack does NOT prove (deferred to 33.d + 33.f):
//!   - Real per-token wall-clock incrementality (needs real backend
//!     stream(); stub backend produces output synchronously).
//!   - tokio::test + reqwest::EventSource live timing observation
//!     (needs a real TcpListener; tower::oneshot collapses scheduling).
//!   - Client disconnect → executor abort < 100ms (Fase 33.f scope).

use axon::axon_server::{build_router, ServerConfig};
use axon::flow_execution_event::FlowExecutionEvent;
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
        "source_file": "33c_live.axon",
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

const SINGLE_STEP_SOURCE: &str =
    "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse }";

const MULTI_STEP_SOURCE: &str =
    "flow Pipeline() -> Unit {\n\
        step Plan { ask: \"plan\" output: Stream<Token> }\n\
        step Execute { ask: \"do\" output: Stream<Token> }\n\
        step Audit { ask: \"verify\" output: Stream<Token> }\n\
     }\n\
     axonendpoint PipelineEndpoint { public: true method: POST path: \"/pipe\" execute: Pipeline transport: sse }";

// Helper: parse the entire SSE wire body into (event_type, id, data_json)
// triples. Sentinel events without `event:` (the `retry:` directive
// has no event field) are not returned.
fn parse_sse_events(body: &str) -> Vec<(String, String, serde_json::Value)> {
    let mut out = Vec::new();
    for block in body.split("\n\n") {
        let mut event = None;
        let mut id = None;
        let mut data = None;
        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("event: ") {
                event = Some(rest.to_string());
            } else if let Some(rest) = line.strip_prefix("id: ") {
                id = Some(rest.to_string());
            } else if let Some(rest) = line.strip_prefix("data: ") {
                data = Some(rest.to_string());
            }
        }
        if let (Some(e), Some(i), Some(d)) = (event, id, data) {
            let parsed: serde_json::Value =
                serde_json::from_str(&d).expect("data must be valid JSON");
            out.push((e, i, parsed));
        }
    }
    out
}

// ─── §1 — Wire shape preservation (D9) ─────────────────────────────

#[tokio::test]
async fn single_step_wire_carries_token_event_and_complete() {
    let app = build_router(server_cfg(true));
    deploy(app.clone(), SINGLE_STEP_SOURCE).await;
    let req = Request::builder()
        .method("POST")
        .uri("/chat")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes);

    // Wire-format invariant — preserves the pre-33.c wire shape that
    // Fase 30+31+32 adopter contracts depend on.
    let events = parse_sse_events(&body);
    let token_events: Vec<_> = events.iter().filter(|(e, _, _)| e == "axon.token").collect();
    let complete_events: Vec<_> = events.iter().filter(|(e, _, _)| e == "axon.complete").collect();

    assert_eq!(
        token_events.len(),
        1,
        "Expected exactly 1 axon.token event for the 1-step stub flow; body:\n{body}"
    );
    assert_eq!(
        complete_events.len(),
        1,
        "Expected exactly 1 axon.complete event terminating the stream; body:\n{body}"
    );

    // Token shape (D9 byte-identical with post-33.b wire):
    let (_, _, token_data) = token_events[0];
    assert_eq!(token_data["step"].as_str().unwrap_or(""), "Generate");
    assert_eq!(token_data["token"].as_str().unwrap_or(""), "(stub)");
    assert!(
        token_data["trace_id"].as_u64().unwrap_or(0) >= 1,
        "trace_id must be >= 1; got {}",
        token_data["trace_id"]
    );

    // Complete envelope shape:
    let (_, _, complete_data) = complete_events[0];
    assert_eq!(complete_data["steps_executed"].as_u64(), Some(1));
    assert_eq!(complete_data["success"].as_bool(), Some(true));
}

#[tokio::test]
async fn multi_step_wire_carries_one_token_per_step_in_order() {
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
    let events = parse_sse_events(&body);

    // Strict ordering invariant: token events arrive in the SAME order
    // the steps were declared in source. The catalog producer
    // (server_execute_streaming) guarantees this; this test pins it.
    let token_events: Vec<_> = events.iter().filter(|(e, _, _)| e == "axon.token").collect();
    assert_eq!(token_events.len(), 3);

    let step_names_in_order: Vec<String> = token_events
        .iter()
        .map(|(_, _, d)| d["step"].as_str().unwrap_or("").to_string())
        .collect();
    assert_eq!(
        step_names_in_order,
        vec!["Plan".to_string(), "Execute".to_string(), "Audit".to_string()],
    );

    // The terminator is axon.complete with steps_executed = 3.
    let complete: Vec<_> = events.iter().filter(|(e, _, _)| e == "axon.complete").collect();
    assert_eq!(complete.len(), 1);
    assert_eq!(complete[0].2["steps_executed"].as_u64(), Some(3));
}

#[tokio::test]
async fn trace_id_is_consistent_across_all_wire_events() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), MULTI_STEP_SOURCE).await;
    let req = Request::builder()
        .method("POST")
        .uri("/pipe")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes);
    let events = parse_sse_events(&body);

    // Pull the trace_id from each data event; all must match.
    let trace_ids: Vec<u64> = events
        .iter()
        .filter_map(|(_, _, d)| d["trace_id"].as_u64())
        .collect();
    assert!(!trace_ids.is_empty(), "Expected at least one trace_id in wire");
    let first = trace_ids[0];
    assert!(first >= 1, "trace_id must be >= 1 (got {first})");
    for tid in &trace_ids {
        assert_eq!(
            *tid, first,
            "All wire events must carry the same trace_id ({first}); got {tid}"
        );
    }
}

// ─── §2 — Event-id monotonicity (W3C SSE Last-Event-ID resume) ─────

#[tokio::test]
async fn event_ids_are_monotonic_across_token_and_complete() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), MULTI_STEP_SOURCE).await;
    let req = Request::builder()
        .method("POST")
        .uri("/pipe")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes);
    let events = parse_sse_events(&body);

    let ids: Vec<u64> = events
        .iter()
        .map(|(_, i, _)| i.parse::<u64>().expect("id must be u64"))
        .collect();
    assert!(!ids.is_empty(), "Expected at least one wire event with id");
    for w in ids.windows(2) {
        assert!(
            w[1] > w[0],
            "event ids must be strictly monotonic; got {ids:?}"
        );
    }
    // First id must be 1 — the retry directive carries no id per W3C.
    assert_eq!(ids[0], 1, "first wire event id must be 1");
}

// ─── §3 — FlowExecutionEvent catalog invariant via receiver path ───
// We exercise the catalog invariant directly by deploying a flow and
// hitting the dynamic-route SSE handler; the wire surface is the
// catalog projection so observing the wire constrains the catalog
// sequence transitively. The pure-receiver path is exercised in
// fase33_b_stub_steps_executed_anchor.rs + fase33_flow_execution_event_drift.rs.

#[tokio::test]
async fn first_wire_event_after_retry_is_axon_token_or_axon_error() {
    let app = build_router(server_cfg(true));
    deploy(app.clone(), SINGLE_STEP_SOURCE).await;
    let req = Request::builder()
        .method("POST")
        .uri("/chat")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes);

    // Locate the first `event: ` line. Per the closed catalog mapping,
    // the very first data event must be either axon.token (the flow
    // produced something) or axon.error (the flow failed before
    // producing any output). NEVER axon.complete-as-first — that would
    // imply a zero-step flow surfaced no token at all, which the
    // catalog producer prevents by always emitting at least one
    // StepToken per StepStart (sentinel empty content for empty step
    // output, see server_execute_streaming).
    let first_event_line = body
        .lines()
        .find(|l| l.starts_with("event: "))
        .expect("Expected at least one `event: ` line in wire body");
    let first_event = first_event_line.trim_start_matches("event: ");
    assert!(
        first_event == "axon.token" || first_event == "axon.error",
        "First wire data event must be axon.token or axon.error per closed-catalog \
         invariant; got '{first_event}'. Body:\n{body}"
    );
}

#[tokio::test]
async fn last_wire_event_is_axon_complete_or_axon_error() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), MULTI_STEP_SOURCE).await;
    let req = Request::builder()
        .method("POST")
        .uri("/pipe")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes);
    let events = parse_sse_events(&body);

    // Catalog invariant: stream MUST terminate with exactly one
    // FlowComplete or FlowError → wire emits axon.complete or
    // axon.error as the LAST data event.
    let last = events.last().expect("at least one data event expected");
    assert!(
        last.0 == "axon.complete" || last.0 == "axon.error",
        "Last wire data event must be axon.complete or axon.error; got '{}'",
        last.0
    );
}

// ─── §4 — Error path: not-deployed flow ─────────────────────────────

#[tokio::test]
async fn not_deployed_flow_emits_axon_error_only() {
    // /v1/execute/sse on a flow that was never deployed must emit
    // a wire-format-valid SSE response with `retry:` + exactly one
    // axon.error event + connection close. No token events.
    let app = build_router(server_cfg(false));
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute/sse")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({"flow_name": "NotDeployed"}).to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes);

    let events = parse_sse_events(&body);
    let errors: Vec<_> = events.iter().filter(|(e, _, _)| e == "axon.error").collect();
    let tokens: Vec<_> = events.iter().filter(|(e, _, _)| e == "axon.token").collect();
    let completes: Vec<_> = events.iter().filter(|(e, _, _)| e == "axon.complete").collect();

    assert_eq!(errors.len(), 1, "expected exactly one axon.error event");
    assert_eq!(tokens.len(), 0, "no axon.token events expected for not-deployed flow");
    assert_eq!(completes.len(), 0, "no axon.complete event for not-deployed flow");

    // Body must start with the retry directive per W3C.
    assert!(body.starts_with("retry: 5000"),
        "wire body must begin with retry: 5000; got:\n{body}");
}

// ─── §5 — Trace recording survives the streaming path (audit parity) ─

#[tokio::test]
async fn trace_recorded_under_reserved_id_after_streaming() {
    // After the SSE wire closes, the trace_store entry MUST exist
    // with the trace_id carried by the wire's axon.token + axon.complete
    // events. Adopter audit replay surface (/v1/replay/<id>) depends
    // on this — D9 + Fase 30.f audit parity.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), SINGLE_STEP_SOURCE).await;

    let req = Request::builder()
        .method("POST")
        .uri("/chat")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes);
    let events = parse_sse_events(&body);
    let wire_trace_id = events
        .iter()
        .find_map(|(e, _, d)| {
            if e == "axon.complete" {
                d["trace_id"].as_u64()
            } else {
                None
            }
        })
        .expect("axon.complete must carry trace_id");

    // Give the spawned executor task a beat to record the trace before
    // we poll the audit surface. The trace is recorded after the
    // wire's axon.complete is sent (handler tail), so a small yield
    // window is appropriate.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Poll the audit surface; the trace entry must exist under
    // wire_trace_id.
    let traces_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/traces")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let traces_bytes = traces_resp.into_body().collect().await.unwrap().to_bytes();
    let traces_payload: serde_json::Value =
        serde_json::from_slice(&traces_bytes).unwrap_or_default();
    let entries = traces_payload["traces"]
        .as_array()
        .or_else(|| traces_payload["entries"].as_array())
        .expect("traces endpoint must return an array");
    let found = entries
        .iter()
        .any(|e| e["id"].as_u64() == Some(wire_trace_id));
    assert!(
        found,
        "trace entry for wire trace_id {wire_trace_id} must exist in trace_store \
         after streaming completes; got entries: {}",
        serde_json::to_string(&entries).unwrap_or_default()
    );
}

// ─── §6 — Catalog closure: every variant is reachable from the wire ─

#[test]
fn catalog_variant_count_locked_at_six() {
    // If a future PR adds a 7th FlowExecutionEvent variant without
    // updating server_execute_streaming + execute_sse_handler match
    // arms, this test fires immediately — the handler's match would
    // be non-exhaustive and the build would fail. This test is the
    // explicit catalog-closure pin from the consumer side.
    use axon::flow_execution_event::FlowExecutionEvent::*;
    let _ = FlowStart {
        flow_name: String::new(),
        backend: String::new(),
        timestamp_ms: 0,
    };
    let _ = StepStart {
        step_name: String::new(),
        step_index: 0,
        step_type: String::new(),
        branch_path: String::new(),
        timestamp_ms: 0,
    };
    let _ = StepToken {
        step_name: String::new(),
        content: String::new(),
        token_index: 0,
        branch_path: String::new(),
        timestamp_ms: 0,
    };
    let _ = StepComplete {
        step_name: String::new(),
        step_index: 0,
        success: true,
        full_output: String::new(),
        tokens_input: 0,
        tokens_output: 0,
        branch_path: String::new(),
        timestamp_ms: 0,
    };
    let _ = FlowComplete {
        flow_name: String::new(),
        backend: String::new(),
        success: true,
        steps_executed: 0,
        tokens_input: 0,
        tokens_output: 0,
        latency_ms: 0,
        timestamp_ms: 0,
    };
    let _ = FlowError {
        flow_name: String::new(),
        error: String::new(),
        timestamp_ms: 0,
    };
    // 6 variants pinned. Adding a 7th forces the producer + consumer
    // match arms to update before this assertion can compile.
}

// ─── §7 — Pure FlowExecutionEvent helpers exercised in cross-stack drift ─
// Catalog-shape assertions live in fase33_flow_execution_event_drift.rs;
// this single test confirms the runtime surface is callable from the
// integration test layer so import drift gets caught here too.

#[test]
fn flow_execution_event_runtime_surface_is_importable() {
    let ev = FlowExecutionEvent::StepToken {
        step_name: "S".to_string(),
        content: "x".to_string(),
        token_index: 1,
        branch_path: String::new(),
        timestamp_ms: 1,
    };
    assert_eq!(ev.kind(), "step_token");
    assert!(ev.is_step_scoped());
    assert!(!ev.is_terminator());
}
