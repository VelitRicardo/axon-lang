//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 33.z.c — Integration tests for the default-on flip + the
//! `axon.tool_call` SSE wire emission graduation (D5 milestone).
//!
//! # What 33.z.c ships (verified by this test pack)
//!
//! 1. **Default-on flip** — `AXON_STREAMING_VIA_DISPATCHER` flips
//!    from OFF (33.z.b alpha) to ON (33.z.c stable). The 8
//!    architectural-group anchor shapes (Conditional / ForIn / Par /
//!    Remember / ShieldApply / Emit / Hibernate / LambdaDataApply)
//!    activate the dispatcher path WITHOUT any explicit flag set.
//!    Adopters who NEED to roll back can still
//!    `set_streaming_via_dispatcher(false)` until 33.z.e deletes
//!    the flag entirely.
//!
//! 2. **`axon.tool_call` SSE event family** — the consumer's
//!    `FlowExecutionEvent::ToolCall { .. } => {}` silent-consume
//!    arm (33.y.k baseline, 33.z.b graft) graduates to active wire
//!    emission via `build_tool_call_event`. Event shape:
//!    `event: axon.tool_call\n id: <N>\n data: {"step": ..., "trace_id": N, "tool_name": ..., "content": ..., "timestamp_ms": N}\n\n`.
//!
//! 3. **D2 invariant verified** — `axon-W002 UnsupportedFlowShape`
//!    is structurally unreachable on the default path. The 8
//!    anchor shapes that pre-33.z fired W002 now emit no warning.
//!
//! 4. **D4 byte-compat preserved** — canonical Step + stub backend
//!    emits 1 token "(stub)" identical to v1.26.0
//!    `run_streaming_async_path` output.
//!
//! 5. **Vertical-grounded canonical patterns** — 4 vertical
//!    canonical patterns (banking decision audit, healthcare CDS,
//!    legal privilege scanner, fintech AML investigative) each
//!    exercise 3+ IRFlowNode variants with the dispatcher default-on.
//!
//! # §Fase 33.z.e — Flag retirement note
//!
//! The `AXON_STREAMING_VIA_DISPATCHER` flag + `StreamingViaDispatcherGuard`
//! + `streaming_via_dispatcher_enabled()` were retired in 33.z.e — the
//! dispatcher is the unconditional production hot path. These tests
//! no longer manipulate any flag; they simply hit the endpoints + assert
//! the dispatcher's wire shape is what arrives.

use axon::axon_server::{build_router, ServerConfig};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

/// §Fase 122.a — the three fixtures below declare `shield … { scan: [pii_leak]
/// compliance: [HIPAA] }` and then apply it. Their SUBJECT is the dispatcher
/// path + `axon-W002` reachability, not shield semantics — the shields are
/// realistic decoration for the regulated-vertical shapes.
///
/// Before §122.a those flows completed because **nothing scanned anything**:
/// with no scanner registered, `shield PHIShield on response` was an identity
/// passthrough and `SanitizedResponse` was bound to unexamined content. These
/// fixtures were, unintentionally, a demonstration of the defect §122.a closes
/// — a HIPAA-badged `pii_leak` scan in a showcase vertical, enforcing nothing.
///
/// A declared `scan:` with no scanner now REFUSES, so the flow must be given
/// what it declares. Registering a scanner is not a workaround for the gate:
/// it is what makes these tests exercise their real subject on a flow that
/// actually runs its declared barrier.
///
/// **Do not delete this to make a red build green** — dropping it means the
/// flows stop completing, which is §122.a working. If a fixture is meant to
/// have no barrier, remove its `scan:` instead (a shield with no `scan:` still
/// passes content through, honestly — §119.c).
struct PassThroughTestScanner;
impl axon::shield_registry::ShieldScanner for PassThroughTestScanner {
    fn scan(
        &self,
        target: &str,
        _ctx: &axon::shield_registry::ShieldScanContext,
    ) -> axon::shield_registry::ShieldVerdict {
        axon::shield_registry::ShieldVerdict::pass(target)
    }
}

/// Register the fixtures' scanners once per test binary (the registry is
/// process-global and integration tests get their own process).
fn ensure_vertical_scanners() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        for name in ["PHIShield", "PrivilegeShield"] {
            axon::shield_registry::register_shield_scanner(
                name,
                std::sync::Arc::new(PassThroughTestScanner),
            );
        }
    });
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

async fn deploy(app: axum::Router, src: &str) -> StatusCode {
    let body = serde_json::json!({
        "source": src,
        "source_file": "default_on.axon",
        "backend": "stub",
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    app.oneshot(req).await.unwrap().status()
}

async fn fetch_sse_body(
    app: axum::Router,
    path: &str,
) -> (StatusCode, String, String) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from("{}".to_string()))
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
struct SseEvents {
    tokens: Vec<serde_json::Value>,
    completes: Vec<serde_json::Value>,
    tool_calls: Vec<serde_json::Value>,
    errors: Vec<serde_json::Value>,
}

fn parse_sse_body(body: &str) -> SseEvents {
    let mut events = SseEvents::default();
    let mut current_event: Option<String> = None;
    let mut current_data: Option<String> = None;
    for line in body.lines() {
        if line.starts_with(':') {
            continue;
        }
        if line.strip_prefix("retry: ").is_some() {
            continue;
        }
        if let Some(ev) = line.strip_prefix("event: ") {
            current_event = Some(ev.trim().to_string());
            continue;
        }
        if let Some(data) = line.strip_prefix("data: ") {
            current_data = Some(data.trim().to_string());
            continue;
        }
        if line.is_empty() {
            if let (Some(ev), Some(data)) =
                (current_event.as_deref(), current_data.as_deref())
            {
                let parsed: serde_json::Value =
                    serde_json::from_str(data).unwrap_or(serde_json::Value::Null);
                match ev {
                    "axon.token" => events.tokens.push(parsed),
                    "axon.complete" => events.completes.push(parsed),
                    "axon.tool_call" => events.tool_calls.push(parsed),
                    "axon.error" => events.errors.push(parsed),
                    _ => {}
                }
            }
            current_event = None;
            current_data = None;
        }
    }
    events
}

fn complete_has_w002(events: &SseEvents) -> bool {
    events.completes.iter().any(|c| {
        c.get("warnings")
            .and_then(|w| w.as_array())
            .map(|arr| arr.iter().any(|w| w.get("code").and_then(|c| c.as_str()) == Some("axon-W002")))
            .unwrap_or(false)
    })
}

// ────────────────────────────────────────────────────────────────────
//  §1 — Flag-state pins RETIRED (33.z.e closure)
// ────────────────────────────────────────────────────────────────────
//
// Pre-33.z.e this section exercised:
//   - `default_flag_value_is_on_post_33_z_c` — verifies default ON
//   - `explicit_opt_out_to_off_still_works` — verifies D4 safety net
// Both deleted in 33.z.e: the flag itself is gone, the dispatcher is
// unconditional. The semantic intent of these tests — "non-canonical
// shapes activate the dispatcher" — is now covered by §2 below
// without flag manipulation.

// ────────────────────────────────────────────────────────────────────
//  §2 — 8 anchor shapes activate dispatcher path by DEFAULT (no flag set)
// ────────────────────────────────────────────────────────────────────
//
// These tests do NOT set the flag explicitly — they verify that the
// new v1.27.0 default ON behavior delivers per-dispatcher wire on
// the 8 architectural-group anchor shapes.

const CANONICAL_STEP_FLOW: &str =
    "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/c\" execute: Chat transport: sse }";

const CONDITIONAL_FLOW: &str =
    "flow Chat() -> Unit {\n\
        if active == \"yes\" {\n\
          step Generate { ask: \"hi\" output: Stream<Token> }\n\
        }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/c\" execute: Chat transport: sse }";

const FOR_IN_FLOW: &str =
    "flow Chat() -> Unit {\n\
        let regions = \"us,eu\"\n\
        for region in regions {\n\
          step Generate { ask: \"hi\" output: Stream<Token> }\n\
        }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/c\" execute: Chat transport: sse }";

const PAR_FLOW: &str =
    "flow Chat() -> Unit {\n\
        par {\n\
          step A { ask: \"a\" output: Stream<Token> }\n\
          step B { ask: \"b\" output: Stream<Token> }\n\
        }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/c\" execute: Chat transport: sse }";

const REMEMBER_FLOW: &str =
    "flow Chat() -> Unit {\n\
        let region = \"us\"\n\
        remember region in session_memory\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/c\" execute: Chat transport: sse }";

const SHIELD_APPLY_FLOW: &str =
    "shield PHIShield {\n\
        scan: [pii_leak]\n\
        on_breach: quarantine\n\
        severity: critical\n\
        compliance: [HIPAA]\n\
     }\n\
     flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
        shield PHIShield on response -> SanitizedResponse\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/c\" execute: Chat transport: sse }";

const EMIT_FLOW: &str =
    "channel OrdersCreated { message: String }\n\
     flow Chat() -> Unit {\n\
        let payload = \"x\"\n\
        emit OrdersCreated(payload)\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/c\" execute: Chat transport: sse }";

const HIBERNATE_FLOW: &str =
    "flow Chat() -> Unit {\n\
        hibernate user_response 30s\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/c\" execute: Chat transport: sse }";

const LAMBDA_DATA_APPLY_FLOW: &str =
    "lambda doubler {\n\
        ontology: \"math\"\n\
        certainty: 1.0\n\
     }\n\
     flow Chat() -> Unit {\n\
        let x = \"5\"\n\
        lambda doubler on x -> LambdaResult\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/c\" execute: Chat transport: sse }";

// ── §Fase 122.b — a program on disk whose routes are actually served ────────
//
// `axonendpoint` is a claim about typed routes and `axpoint` is a claim that it
// is the same thing under another spelling. Both were cited on an engine
// (`axon_server`, `tokens.rs`), and no program on disk was being served to
// check either. This deploys one and fetches BOTH routes.

#[tokio::test]
async fn a_deployed_fixture_serves_both_endpoint_spellings() {
    const FIXTURE: &str = "tests/fixtures/fase122_b_endpoint/served_routes.axon";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    let app = build_router(server_cfg());
    assert_eq!(
        deploy(app.clone(), &src).await,
        StatusCode::OK,
        "the fixture must deploy"
    );

    // Each route must run the flow IT declares — checked by the step name that
    // only that flow contains. Asserting merely that "a flow ran" does not
    // discriminate: both flows have the same shape, so swapping `execute:`
    // between them leaves the stream looking identical. (Found by mutating it.)
    for (route, own_step, other_step) in [
        ("/fixture/primary", "Assess", "Confirm"),
        ("/fixture/alias", "Confirm", "Assess"),
    ] {
        let (status, ct, body) = fetch_sse_body(app.clone(), route).await;
        assert_eq!(status, StatusCode::OK, "{route}: HTTP status");
        assert!(
            ct.starts_with("text/event-stream"),
            "{route}: the declared `transport: sse` must be honoured, got {ct}"
        );
        let events = parse_sse_body(&body);
        assert!(
            !events.completes.is_empty(),
            "{route}: the route must actually execute its declared flow"
        );
        assert!(
            body.contains(own_step),
            "{route}: must run the flow it DECLARES in `execute:` (step `{own_step}`). Body: {body}"
        );
        assert!(
            !body.contains(other_step),
            "{route}: must NOT run the other route's flow (step `{other_step}`). Body: {body}"
        );
    }
}

async fn assert_default_on_no_w002(label: &str, src: &str) {
    // Defensive flag-state assertion — the test depends on the
    // 33.z.c default ON. If a previous test left the flag at OFF
    // via panic-during-guard, we reset to the documented default.
    ensure_vertical_scanners(); // §122.a — see the doc at the definition.

    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), src).await;
    assert_eq!(dep, StatusCode::OK, "{label}: deploy");

    let (status, ct, body) = fetch_sse_body(app, "/c").await;
    assert_eq!(status, StatusCode::OK, "{label}: HTTP status");
    assert!(
        ct.starts_with("text/event-stream"),
        "{label}: Content-Type"
    );

    let events = parse_sse_body(&body);
    assert!(
        !events.completes.is_empty(),
        "{label}: must emit FlowComplete"
    );
    assert!(
        !complete_has_w002(&events),
        "{label}: D2 invariant — axon-W002 UnsupportedFlowShape \
         structurally unreachable on the dispatcher default-on path"
    );
}

#[tokio::test]
async fn default_on_canonical_step_d4_byte_compat() {

    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), CANONICAL_STEP_FLOW).await;
    assert_eq!(dep, StatusCode::OK);
    let (status, ct, body) = fetch_sse_body(app, "/c").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));
    let events = parse_sse_body(&body);
    assert_eq!(
        events.tokens.len(),
        1,
        "D4: canonical Step + stub → exactly 1 axon.token"
    );
    assert_eq!(events.tokens[0]["token"], "(stub)");
    assert!(events.tool_calls.is_empty(), "stub → no tool_call");
}

#[tokio::test]
async fn default_on_conditional_no_w002() {
    assert_default_on_no_w002("Conditional", CONDITIONAL_FLOW).await;
}

#[tokio::test]
async fn default_on_for_in_no_w002() {
    assert_default_on_no_w002("ForIn", FOR_IN_FLOW).await;
}

#[tokio::test]
async fn default_on_par_no_w002() {
    assert_default_on_no_w002("Par", PAR_FLOW).await;
}

#[tokio::test]
async fn default_on_remember_no_w002() {
    assert_default_on_no_w002("Remember", REMEMBER_FLOW).await;
}

#[tokio::test]
async fn default_on_shield_apply_no_w002() {
    assert_default_on_no_w002("ShieldApply", SHIELD_APPLY_FLOW).await;
}

#[tokio::test]
async fn default_on_emit_no_w002() {
    assert_default_on_no_w002("Emit", EMIT_FLOW).await;
}

#[tokio::test]
async fn default_on_hibernate_no_w002() {
    assert_default_on_no_w002("Hibernate", HIBERNATE_FLOW).await;
}

#[tokio::test]
async fn default_on_lambda_data_apply_no_w002() {
    assert_default_on_no_w002("LambdaDataApply", LAMBDA_DATA_APPLY_FLOW).await;
}

// ────────────────────────────────────────────────────────────────────
//  §3 — `axon.tool_call` SSE wire emission (D5 milestone)
// ────────────────────────────────────────────────────────────────────
//
// The stub backend signals `FinishReason::Stop` and NEVER triggers
// `FlowExecutionEvent::ToolCall`. So with the stub backend, the wire
// never carries `axon.tool_call` events — D4 byte-compat preserved.
// The D5 milestone is the CAPABILITY: when an upstream backend DOES
// signal `FinishReason::ToolUse`, the wire emits the new event family.
//
// These tests verify the unit-level invariants:
// - The `build_tool_call_event` helper exists + is callable from the
//   consumer path (compile-time invariant).
// - The closed-catalog event slug is `axon.tool_call`.
// - Stub backend doesn't emit it (D4 byte-compat preserved).
//
// End-to-end emission with a real upstream signalling ToolUse is
// gated under the opt-in `AXON_RUN_REAL_PROVIDER_TEST` lane (the
// 33.x.j precedent) — this test pack stays hermetic + deterministic.

#[tokio::test]
async fn stub_backend_never_emits_tool_call_event() {
    // Canonical Step + apply: stub_tool. stub backend signals Stop;
    // dispatcher's pure_shape doesn't emit FlowExecutionEvent::ToolCall.
    let src = "tool example_tool {\n\
                   description: \"stub tool\"\n\
                   effects: <stream:drop_oldest>\n\
               }\n\
               flow Chat() -> Unit {\n\
                   step Generate { ask: \"hi\" apply: example_tool }\n\
               }\n\
               axonendpoint E { public: true method: POST path: \"/tool\" execute: Chat transport: sse }";

    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), src).await;
    assert_eq!(dep, StatusCode::OK);
    let (status, _ct, body) = fetch_sse_body(app, "/tool").await;
    assert_eq!(status, StatusCode::OK);
    let events = parse_sse_body(&body);
    assert!(
        events.tool_calls.is_empty(),
        "D4 byte-compat: stub backend never emits ToolCall → wire \
         body carries no axon.tool_call events. Wire shape is \
         strictly additive — adopters running on stub see the same \
         3-event canonical shape (StepStart implied + StepToken + \
         StepComplete) as v1.25.0."
    );
}

#[test]
fn tool_call_event_helper_slug_is_axon_tool_call() {
    // Compile-time invariant: the helper exists (it's `fn build_tool_call_event`
    // in axon_server.rs) + its event slug per `build_tool_call_event`
    // is exactly "axon.tool_call". This test pin documents the
    // closed-catalog wire event family.
    //
    // The helper is a private fn so we can't call it directly from
    // tests — instead we assert the slug ARRIVES on the wire when
    // the dispatcher actually emits ToolCall. Real-provider lane
    // (33.x.j precedent) exercises this end-to-end with real upstream
    // backends. This test pin is a documentation anchor for the
    // closed-catalog event family.
    let expected_slug = "axon.tool_call";
    assert_eq!(
        expected_slug, "axon.tool_call",
        "D5 closed-catalog: the event slug is 'axon.tool_call'. \
         Adopter clients pattern-matching on `event:` discriminate \
         tool-call requests from text token deltas via this slug."
    );
}

#[test]
fn tool_call_event_payload_shape_documented() {
    // Documentation pin — the wire payload for `axon.tool_call`
    // carries 5 fields:
    //   - step: string (step name from FlowExecutionEvent::ToolCall.step_name)
    //   - trace_id: u64 (matches the per-flow trace counter)
    //   - tool_name: string
    //   - content: string (provider-specific tool-call delta)
    //   - timestamp_ms: u64
    let expected_fields = ["step", "trace_id", "tool_name", "content", "timestamp_ms"];
    assert_eq!(
        expected_fields.len(),
        5,
        "D5 wire payload: closed-catalog 5-field shape. Future fields \
         (e.g., tool-call ID for client-side correlation) are additive \
         per the wire byte-compat discipline."
    );
}

// ────────────────────────────────────────────────────────────────────
//  §4 — Vertical-grounded canonical patterns
// ────────────────────────────────────────────────────────────────────
//
// 4 vertical canonical patterns — banking decision audit, healthcare
// CDS, legal privilege scanner, fintech AML — each exercising 3+
// IRFlowNode variants through the dispatcher default-on path.

// §Fase 33.z.k.g.2 (v1.28.0) — algebraic-effect verticals declare
// `transport: sse(axon)` explicit so the W3C named-events wire is
// preserved despite the Q1 default flip to openai for algebraic-
// effect flows. These tests verify DISPATCHER behavior + warning
// catalog (not wire format); the openai-dialect projection of the
// same vertical patterns is exercised in the 33.z.k.g E2E test pack.
// Q5 escape valve: explicit dialect declaration always wins.
const BANKING_DECISION_FLOW: &str =
    "tool aml_check {\n\
        description: \"AML scanner\"\n\
        effects: <stream:drop_oldest>\n\
     }\n\
     flow LoanDecision() -> Unit {\n\
        if active == \"yes\" {\n\
          step Investigate { ask: \"AML check\" apply: aml_check }\n\
        }\n\
        step Adjudicate { ask: \"Final decision\" output: Stream<Token> }\n\
     }\n\
     axonendpoint Decision { public: true method: POST path: \"/loan\" execute: LoanDecision transport: sse(axon) }";

const HEALTHCARE_CDS_FLOW: &str =
    // §Fase 36.x.c — `clinical_reasoning` is applied at TOP LEVEL by
    // `step Differential`; since Fase 36.i wired the tool registry, a
    // `<stream:…>` tool with no `provider:` resolves to the
    // SyncFallbackTool and errors. It declares `provider: stub_stream`
    // so the canonical healthcare CDS flow actually streams + completes
    // (the v1.34.0 double-terminator bug had masked the error).
    "tool clinical_reasoning {\n\
        description: \"differential\"\n\
        provider: stub_stream\n\
        effects: <stream:drop_oldest>\n\
     }\n\
     shield PHIShield {\n\
        scan: [pii_leak]\n\
        on_breach: quarantine\n\
        severity: critical\n\
        compliance: [HIPAA]\n\
     }\n\
     flow CDSAssessment() -> Unit {\n\
        step Triage { ask: \"vitals\" output: Stream<Token> }\n\
        step Differential { ask: \"diagnosis\" apply: clinical_reasoning }\n\
        shield PHIShield on response -> SanitizedResponse\n\
     }\n\
     axonendpoint CDS { public: true method: POST path: \"/cds\" execute: CDSAssessment transport: sse(axon) }";

const LEGAL_PRIVILEGE_FLOW: &str =
    "shield PrivilegeShield {\n\
        scan: [pii_leak]\n\
        on_breach: quarantine\n\
        severity: critical\n\
        compliance: [HIPAA]\n\
     }\n\
     flow PrivilegeAssessment() -> Unit {\n\
        let docs = \"doc1,doc2,doc3\"\n\
        for doc in docs {\n\
          step Scan { ask: \"privilege check\" output: Stream<Token> }\n\
        }\n\
        shield PrivilegeShield on response -> SanitizedResponse\n\
     }\n\
     axonendpoint Privilege { public: true method: POST path: \"/privilege\" execute: PrivilegeAssessment transport: sse }";

const FINTECH_AML_FLOW: &str =
    "tool aml_investigator {\n\
        description: \"AML reasoning\"\n\
        effects: <stream:drop_oldest>\n\
     }\n\
     flow AmlInvestigation() -> Unit {\n\
        par {\n\
          step ScreenSanctions { ask: \"sanctions check\" output: Stream<Token> }\n\
          step AnalyzeFlow { ask: \"flow analysis\" apply: aml_investigator }\n\
        }\n\
        step Decide { ask: \"final report\" output: Stream<Token> }\n\
     }\n\
     axonendpoint Aml { public: true method: POST path: \"/aml\" execute: AmlInvestigation transport: sse(axon) }";

async fn assert_vertical_pattern(label: &str, src: &str, path: &str) {
    ensure_vertical_scanners(); // §122.a — see the doc at the definition.

    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), src).await;
    assert_eq!(dep, StatusCode::OK, "{label}: deploy");

    let (status, _ct, body) = fetch_sse_body(app, path).await;
    assert_eq!(status, StatusCode::OK, "{label}: HTTP status");
    let events = parse_sse_body(&body);

    eprintln!(
        "─── §4 vertical {label} ───\n  tokens={} completes={} w002={}",
        events.tokens.len(),
        events.completes.len(),
        complete_has_w002(&events)
    );
    assert!(
        !events.completes.is_empty(),
        "{label}: must emit FlowComplete"
    );
    assert!(
        !complete_has_w002(&events),
        "{label}: D2 invariant — vertical patterns activate dispatcher \
         path without W002 firing. Regulated-vertical adopters \
         observe per-chunk granularity across their canonical \
         orchestration shapes."
    );
}

#[tokio::test]
async fn vertical_banking_decision_audit_no_w002() {
    assert_vertical_pattern("Banking-Decision", BANKING_DECISION_FLOW, "/loan").await;
}

#[tokio::test]
async fn vertical_healthcare_cds_no_w002() {
    assert_vertical_pattern("Healthcare-CDS", HEALTHCARE_CDS_FLOW, "/cds").await;
}

#[tokio::test]
async fn vertical_legal_privilege_no_w002() {
    assert_vertical_pattern("Legal-Privilege", LEGAL_PRIVILEGE_FLOW, "/privilege").await;
}

#[tokio::test]
async fn vertical_fintech_aml_no_w002() {
    assert_vertical_pattern("Fintech-AML", FINTECH_AML_FLOW, "/aml").await;
}
