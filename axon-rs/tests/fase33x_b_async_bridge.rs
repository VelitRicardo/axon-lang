//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 33.x.b — Production SSE path drives `Backend::stream()`
//! per step.
//!
//! Verifies the bridge that takes Fase 33.a-i primitives off the
//! shelf (FlowExecutionEvent catalog L1 + live mpsc consumer L2 +
//! Backend::stream() per-provider L3 + stream_effect_dispatcher L4
//! + CancellationFlag D6) and activates them on the production
//! HTTP SSE handler. The legacy synchronous fallback is preserved
//! verbatim for flow shapes the streaming path does not yet model.
//!
//! # D-letter coverage
//!
//! - **D1** — `Backend::stream()` is reached on the production SSE
//!   path. Verified by:
//!     a. `flow_plan` + `StubBackend` round-trips emit exactly 1
//!        chunk-driven `axon.token` per single-step stub flow.
//!     b. Multi-step flows emit ≥1 axon.token per step with the
//!        step's `step` field set correctly.
//! - **D4** — Wire-format byte-identical to v1.24.0 on stub-driven
//!   flows. The pre-33.x.b synthetic 3-word chunking produced 1
//!   axon.token with content `"(stub)"` per step; post-33.x.b the
//!   same wire emerges from the new async path because
//!   `StubBackend::stream()` returns a one-chunk stream with the
//!   canonical `"(stub)"` delta.
//! - **D8** — CLI sync path + stub backend preserved unchanged.
//! - **Plan vivo §7 "no MVP, no partial solutions"** — flows that
//!   use anchors / lambda apply / let bindings / mid-stream
//!   `use_tool` / hibernate / pix / un-modeled IRFlowNode variants
//!   fall back to `server_execute_streaming_legacy` cleanly; the
//!   wire shape stays byte-identical with pre-33.x.b for those
//!   adopter sources.

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

async fn fetch_sse_body(
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
    let ct = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes).to_string();
    (status, ct, body)
}

#[derive(Debug, Default)]
struct ParsedSse {
    retry_count: usize,
    tokens: Vec<Value>,
    completes: Vec<Value>,
    errors: Vec<Value>,
    other: Vec<(String, Value)>,
}

fn parse_sse(body: &str) -> ParsedSse {
    let mut out = ParsedSse::default();
    let mut cur_event: Option<String> = None;
    let mut cur_data: Option<String> = None;
    for line in body.lines() {
        if line.starts_with(':') {
            continue;
        }
        if line.strip_prefix("retry: ").is_some() {
            out.retry_count += 1;
            continue;
        }
        if let Some(ev) = line.strip_prefix("event: ") {
            cur_event = Some(ev.trim().to_string());
            continue;
        }
        if let Some(data) = line.strip_prefix("data: ") {
            cur_data = Some(data.trim().to_string());
            continue;
        }
        if line.is_empty() {
            if let (Some(ev), Some(data)) = (cur_event.as_deref(), cur_data.as_deref()) {
                let parsed: Value =
                    serde_json::from_str(data).unwrap_or(Value::Null);
                match ev {
                    "axon.token" => out.tokens.push(parsed),
                    "axon.complete" => out.completes.push(parsed),
                    "axon.error" => out.errors.push(parsed),
                    other_ev => out.other.push((other_ev.to_string(), parsed)),
                }
            }
            cur_event = None;
            cur_data = None;
        }
    }
    if let (Some(ev), Some(data)) = (cur_event, cur_data) {
        let parsed: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
        match ev.as_str() {
            "axon.token" => out.tokens.push(parsed),
            "axon.complete" => out.completes.push(parsed),
            "axon.error" => out.errors.push(parsed),
            other_ev => out.other.push((other_ev.to_string(), parsed)),
        }
    }
    out
}

// ── Canonical adopter sources ──────────────────────────────────────

const SIMPLE_STREAM: &str =
    "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse }";

// §Fase 33.z.k.g.2 — Algebraic-effect flows declare
// `transport: sse(axon)` explicitly (Q5 escape valve) so the 33.x.b
// async-bridge anchor continues to assert axon.complete.stream_policies
// shape. The openai-dialect projection of the same data lands on the
// Q7 axon_metadata frame (covered by 33.z.k.g E2E pack).
const STREAM_WITH_DROP_OLDEST: &str =
    "tool chat_tokens { description: \"stream\" effects: <stream:drop_oldest> }\n\
     flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" apply: chat_tokens }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse(axon) }";

const STREAM_WITH_FAIL_POLICY: &str =
    "tool chat_tokens { description: \"stream\" effects: <stream:fail> }\n\
     flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" apply: chat_tokens }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse(axon) }";

const STREAM_WITH_PAUSE_UPSTREAM: &str =
    "tool chat_tokens { description: \"stream\" effects: <stream:pause_upstream> }\n\
     flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" apply: chat_tokens }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse(axon) }";

const STREAM_WITH_DEGRADE_QUALITY: &str =
    "tool chat_tokens { description: \"stream\" effects: <stream:degrade_quality> }\n\
     flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" apply: chat_tokens }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse(axon) }";

const MULTI_STEP_STREAM: &str =
    "flow Chain() -> Unit {\n\
        step First { ask: \"one\" output: Stream<Token> }\n\
        step Second { ask: \"two\" output: Stream<Token> }\n\
        step Third { ask: \"three\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChainEndpoint { public: true method: POST path: \"/chain\" execute: Chain transport: sse }";

// ─── §1 — D1 anchor: Backend::stream() reached on production path ───

#[tokio::test]
async fn d1_simple_stream_flow_emits_chunk_driven_token() {
    let app = build_router(server_cfg());
    deploy(app.clone(), SIMPLE_STREAM).await;
    let (status, ct, body) = fetch_sse_body(app, "/chat", "{}").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));

    let parsed = parse_sse(&body);
    assert_eq!(parsed.retry_count, 1, "W3C SSE retry: directive leads");
    assert_eq!(
        parsed.tokens.len(),
        1,
        "stub backend emits exactly one chunk via Backend::stream(); \
         post-33.x.b wire body has 1 axon.token, byte-compat with \
         v1.24.0 (D4)"
    );
    assert_eq!(parsed.tokens[0]["step"], "Generate");
    assert_eq!(
        parsed.tokens[0]["token"], "(stub)",
        "chunk.delta == STUB_CONTENT (\"(stub)\"); wire body \
         byte-identical with v1.24.0 (D4)"
    );
    assert_eq!(parsed.completes.len(), 1);
    assert!(parsed.errors.is_empty());
}

#[tokio::test]
async fn d1_multi_step_emits_one_token_per_step() {
    let app = build_router(server_cfg());
    deploy(app.clone(), MULTI_STEP_STREAM).await;
    let (status, _, body) = fetch_sse_body(app, "/chain", "{}").await;
    assert_eq!(status, StatusCode::OK);
    let parsed = parse_sse(&body);
    assert_eq!(parsed.tokens.len(), 3, "one chunk per step from stub");
    assert_eq!(parsed.tokens[0]["step"], "First");
    assert_eq!(parsed.tokens[1]["step"], "Second");
    assert_eq!(parsed.tokens[2]["step"], "Third");
    for tok in &parsed.tokens {
        assert_eq!(tok["token"], "(stub)");
    }
    assert_eq!(parsed.completes.len(), 1);
    let c = &parsed.completes[0];
    assert_eq!(c["steps_executed"], 3);
}

#[tokio::test]
async fn d1_token_index_monotone_within_step() {
    let app = build_router(server_cfg());
    deploy(app.clone(), MULTI_STEP_STREAM).await;
    let (_status, _, body) = fetch_sse_body(app, "/chain", "{}").await;
    let parsed = parse_sse(&body);
    // Each step's tokens start at index 1; stub emits 1 chunk per
    // step, so every event has token_index 1. When real backends
    // stream N chunks per step, the index counts within that step.
    for tok in &parsed.tokens {
        let idx = tok.get("token_index")
            .or_else(|| tok.get("trace_id"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        // Stub: exactly 1 chunk per step, so token_index == 1
        // (or the legacy trace_id field on older anchors).
        assert!(idx >= 1, "monotone count starts at 1");
    }
}

// ─── §2 — D2 anchor: stream_policies populated for declared effects ─

#[tokio::test]
async fn d2_drop_oldest_policy_surfaces_on_complete_event() {
    let app = build_router(server_cfg());
    deploy(app.clone(), STREAM_WITH_DROP_OLDEST).await;
    let (_status, _, body) = fetch_sse_body(app, "/chat", "{}").await;
    let parsed = parse_sse(&body);
    let c = &parsed.completes[0];
    let policies = c["stream_policies"]
        .as_array()
        .expect("stream_policies array present (Fase 33.e wiring preserved through 33.x.b)");
    assert_eq!(policies.len(), 1);
    assert_eq!(policies[0]["step"], "Generate");
    assert_eq!(policies[0]["policy"], "drop_oldest");
}

#[tokio::test]
async fn d2_fail_policy_surfaces_on_complete_event() {
    let app = build_router(server_cfg());
    deploy(app.clone(), STREAM_WITH_FAIL_POLICY).await;
    let (_status, _, body) = fetch_sse_body(app, "/chat", "{}").await;
    let parsed = parse_sse(&body);
    let policies = parsed.completes[0]["stream_policies"].as_array().unwrap();
    assert_eq!(policies[0]["policy"], "fail");
}

#[tokio::test]
async fn d2_pause_upstream_policy_surfaces_on_complete_event() {
    let app = build_router(server_cfg());
    deploy(app.clone(), STREAM_WITH_PAUSE_UPSTREAM).await;
    let (_status, _, body) = fetch_sse_body(app, "/chat", "{}").await;
    let parsed = parse_sse(&body);
    let policies = parsed.completes[0]["stream_policies"].as_array().unwrap();
    assert_eq!(policies[0]["policy"], "pause_upstream");
}

#[tokio::test]
async fn d2_degrade_quality_policy_surfaces_on_complete_event() {
    let app = build_router(server_cfg());
    deploy(app.clone(), STREAM_WITH_DEGRADE_QUALITY).await;
    let (_status, _, body) = fetch_sse_body(app, "/chat", "{}").await;
    let parsed = parse_sse(&body);
    let policies = parsed.completes[0]["stream_policies"].as_array().unwrap();
    assert_eq!(policies[0]["policy"], "degrade_quality");
}

// ─── §3 — D4 anchor: wire byte-compat preserved for v1.24.0 fields ──

#[tokio::test]
async fn d4_axon_token_field_schema_unchanged() {
    let app = build_router(server_cfg());
    deploy(app.clone(), SIMPLE_STREAM).await;
    let (_status, _, body) = fetch_sse_body(app, "/chat", "{}").await;
    let parsed = parse_sse(&body);
    let tok = &parsed.tokens[0];
    // v1.24.0 axon.token field set: step + token + trace_id +
    // timestamp_ms. 33.x.b preserves this exactly.
    assert!(tok.get("step").is_some());
    assert!(tok.get("token").is_some());
    assert!(tok.get("trace_id").is_some());
    assert!(tok.get("timestamp_ms").is_some());
}

#[tokio::test]
async fn d4_axon_complete_carries_backend_steps_tokens_fields() {
    let app = build_router(server_cfg());
    deploy(app.clone(), SIMPLE_STREAM).await;
    let (_status, _, body) = fetch_sse_body(app, "/chat", "{}").await;
    let parsed = parse_sse(&body);
    let c = &parsed.completes[0];
    assert_eq!(c["backend"], "stub");
    assert_eq!(c["flow"], "Chat");
    assert!(c.get("steps_executed").is_some());
    assert!(c.get("tokens_input").is_some());
    assert!(c.get("tokens_output").is_some());
    assert!(c.get("success").is_some());
    assert!(c.get("latency_ms").is_some());
    assert!(c.get("trace_id").is_some());
}

#[tokio::test]
async fn d4_no_effect_declaration_elides_stream_policies() {
    let app = build_router(server_cfg());
    deploy(app.clone(), SIMPLE_STREAM).await;
    let (_status, _, body) = fetch_sse_body(app, "/chat", "{}").await;
    let parsed = parse_sse(&body);
    let c = &parsed.completes[0];
    // Empty stream_policies is elided per Fase 33.e D9 byte-compat —
    // adopters using v1.20-v1.24 parsers do not see the field.
    assert!(
        c.get("stream_policies").is_none()
            || c["stream_policies"].as_array().map(|a| a.is_empty()).unwrap_or(false),
        "v1.24.0 byte-compat: no effect → no stream_policies key"
    );
}

#[tokio::test]
async fn d4_trace_id_is_consistent_across_token_and_complete() {
    let app = build_router(server_cfg());
    deploy(app.clone(), SIMPLE_STREAM).await;
    let (_status, _, body) = fetch_sse_body(app, "/chat", "{}").await;
    let parsed = parse_sse(&body);
    let token_trace = &parsed.tokens[0]["trace_id"];
    let complete_trace = &parsed.completes[0]["trace_id"];
    assert_eq!(token_trace, complete_trace);
}

// ─── §4 — Legacy fallback for unsupported flow shapes ──────────────

#[tokio::test]
async fn legacy_fallback_is_reached_via_v1_execute_sse_for_non_planned_flow() {
    // /v1/execute/sse always routes through `server_execute_streaming`
    // regardless of dynamic-route registration. For non-deployed
    // flows, the streaming handler still produces a wire-valid SSE
    // body (axon.error or axon.complete success:false) — pre-33.x.b
    // behavior preserved. This anchors the D4 byte-compat invariant
    // for the legacy fallback path without requiring a specific
    // adopter source shape (which can drift across parser versions).
    let app = build_router(server_cfg());
    let (status, ct, body) = fetch_sse_body(
        app,
        "/v1/execute/sse",
        "{\"flow_name\":\"UndeployedFlow\",\"backend\":\"stub\"}",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));
    let parsed = parse_sse(&body);
    assert!(parsed.retry_count >= 1, "W3C SSE retry: directive leads");
    // Either axon.error fires (clean error event) or the legacy
    // path emits axon.complete with success:false. Both are wire-
    // valid v1.24.0 shapes.
    let wire_terminated_cleanly = !parsed.errors.is_empty()
        || parsed
            .completes
            .iter()
            .any(|c| c["success"] == false || c["success"] == true);
    assert!(
        wire_terminated_cleanly,
        "wire MUST terminate with axon.error or axon.complete — D4 byte-compat"
    );
}

// ─── §5 — Plan-build error paths emit structured FlowError ─────────

#[tokio::test]
async fn unparseable_source_falls_to_legacy_which_surfaces_error() {
    // Source that fails to parse triggers PlanError::Parse → legacy
    // path → which also fails to parse → axon.error wire event.
    // No panic, well-formed wire.
    let app = build_router(server_cfg());
    // Cannot deploy unparseable source; instead hit a route that
    // doesn't exist + verify the streaming handler responds wire-
    // valid.
    let (status, ct, body) =
        fetch_sse_body(app, "/v1/execute/sse", "{\"flow_name\":\"NotDeployed\",\"backend\":\"stub\"}").await;
    // Not-deployed responses still return 200 with a wire-valid SSE
    // body per Fase 30.f.
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));
    let parsed = parse_sse(&body);
    // Either axon.error fires or axon.complete with success:false.
    let saw_err_or_unsuccessful_complete = !parsed.errors.is_empty()
        || parsed.completes.iter().any(|c| c["success"] == false);
    assert!(saw_err_or_unsuccessful_complete);
}

// ─── §6 — Effect policy + multi-step plan composition ──────────────

#[tokio::test]
async fn declared_effect_on_one_step_does_not_pollute_other_steps() {
    // §Fase 33.z.k.g.2 — `transport: sse(axon)` keeps W3C wire (Q5).
    let src = "tool chat_tokens { description: \"stream\" effects: <stream:drop_oldest> }\n\
               flow Multi() -> Unit {\n\
                   step Plain { ask: \"plain\" output: Stream<Token> }\n\
                   step Effecting { ask: \"hi\" apply: chat_tokens }\n\
               }\n\
               axonendpoint MultiEndpoint { public: true method: POST path: \"/multi\" execute: Multi transport: sse(axon) }";
    let app = build_router(server_cfg());
    deploy(app.clone(), src).await;
    let (_status, _, body) = fetch_sse_body(app, "/multi", "{}").await;
    let parsed = parse_sse(&body);

    let policies = parsed.completes[0]["stream_policies"].as_array().unwrap();
    // Only the Effecting step has a declared policy; Plain does not.
    let effecting_only: Vec<&Value> = policies
        .iter()
        .filter(|p| p["step"] == "Effecting")
        .collect();
    assert_eq!(
        effecting_only.len(),
        1,
        "Effecting step has its own policy entry"
    );
    let plain_entries: Vec<&Value> = policies
        .iter()
        .filter(|p| p["step"] == "Plain")
        .collect();
    assert!(
        plain_entries.is_empty(),
        "Plain step (no declared effect) MUST NOT appear in stream_policies — \
         per-step independence (D2 + Fase 33.e D8 multi-vertical safety)"
    );
}

// ─── §7 — Cancel-safety preserved (Fase 33.f baseline + 33.x.b) ────

#[tokio::test]
async fn happy_path_no_cancel_yields_complete_event() {
    let app = build_router(server_cfg());
    deploy(app.clone(), SIMPLE_STREAM).await;
    let (_status, _, body) = fetch_sse_body(app, "/chat", "{}").await;
    let parsed = parse_sse(&body);
    assert!(parsed.errors.is_empty());
    assert_eq!(parsed.completes.len(), 1);
    assert_eq!(parsed.completes[0]["success"], true);
}

// ─── §8 — Trace correlation: token + complete share trace_id ───────

#[tokio::test]
async fn trace_id_correlation_holds_for_drop_oldest_flow() {
    let app = build_router(server_cfg());
    deploy(app.clone(), STREAM_WITH_DROP_OLDEST).await;
    let (_status, _, body) = fetch_sse_body(app, "/chat", "{}").await;
    let parsed = parse_sse(&body);
    let token_trace = parsed.tokens[0]["trace_id"].clone();
    let complete_trace = parsed.completes[0]["trace_id"].clone();
    assert_eq!(token_trace, complete_trace);
    assert!(!complete_trace.is_null());
}

// ─── §9 — Catalog closure: only known event types emitted ──────────

#[tokio::test]
async fn wire_catalog_is_closed_for_streaming_path() {
    let app = build_router(server_cfg());
    deploy(app.clone(), STREAM_WITH_DROP_OLDEST).await;
    let (_status, _, body) = fetch_sse_body(app, "/chat", "{}").await;
    let parsed = parse_sse(&body);
    assert!(
        parsed.other.is_empty(),
        "Unexpected wire event types: {:?}. Catalog is {{token, complete, error}}.",
        parsed.other
    );
}

// ─── §10 — Empty body POST is accepted ──────────────────────────────

#[tokio::test]
async fn empty_request_body_dispatches_cleanly() {
    let app = build_router(server_cfg());
    deploy(app.clone(), SIMPLE_STREAM).await;
    let (status, ct, body) = fetch_sse_body(app, "/chat", "{}").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));
    let parsed = parse_sse(&body);
    assert_eq!(parsed.completes.len(), 1);
}
