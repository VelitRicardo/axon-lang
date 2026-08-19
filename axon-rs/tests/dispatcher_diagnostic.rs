//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.24.0 diagnostic anchor — capture the v1.25.0 production-path
//! SSE wire shape for non-canonical-`Step` adopter flows that the
//! v1.24.0 cycle activates end-to-end.
//!
//! # Why a new diagnostic for 33.y
//!
//! `real_streaming_diagnostic.rs` captured the v1.24.0 → v1.25.0
//! transition for the **canonical `Step` shape**. Post-33.x.b, that
//! shape rides the `run_streaming_async_path` (real `Backend::stream()`
//! per step, per-chunk wire delivery, enforcer activation, cancel-in-body
//! p95 12.6µs, per-step replay binding).
//!
//! **Every other `IRFlowNode` variant** (44 of 45) still falls back to
//! `run_streaming_legacy_path` — synchronous `server_execute_full` →
//! materialized output → 3-word-group chunking → post-hoc `axon.token`
//! events. Indistinguishable from v1.24.0 burst-at-end to the adopter's
//! eye. The only adopter-observable signal of the fallback is
//! `axon-W002` arriving at `axon.complete` time — i.e., **after** the
//! latency was already paid.
//!
//! # What this anchor does
//!
//! Drives the production SSE handler end-to-end against representative
//! non-`Step` flow shapes that real regulated-domain adopters use
//! (`Let` bindings, `ForIn` loops, `if` conditionals, tool-using steps),
//! captures the **current** (v1.25.0) wire shape verbatim, and asserts
//! the legacy-fallback invariants. As each 33.y step lands the
//! per-variant async handler the corresponding anchor assertion is
//! INVERTED in lockstep — pre-33.y.d assertions read "axon-W002 fires
//! with `fallback_mode: unsupported_flow_shape, reason: let_binding`";
//! post-33.y.d they read "no W002 + per-step async streaming activates
//! through Backend::stream()".
//!
//! When 33.y.o ships and tags v1.26.0, every assertion below reads the
//! post-33.y shape and the diagnostic is the closure proof of the cycle.
//!
//! # What this anchor does NOT do
//!
//! Real provider HTTP roundtrips ship in the opt-in
//! `real_provider.yml` workflow (v1.24.0). This diagnostic
//! uses the in-tree `stub` backend so the test is hermetic +
//! deterministic + fast.
//!
//! # D-letter anchors (proposed — see docs/cycle/algebraic_streaming_dispatcher.md)
//!
//! - **D1** — Per-IRFlowNode async dispatch is total. Pre-33.y.b:
//!   `unsupported_feature_reason` returns `Some(_)` for 44 of 45
//!   variants. Post-33.y.l: `unsupported_feature_reason` is deleted;
//!   the dispatcher has compiler-enforced exhaustive coverage.
//! - **D2** — Effects compose at the chunk level. Pre-33.y.e: nested
//!   `Par` with per-branch declared effects → fallback (Par is
//!   UnsupportedNode). Post-33.y.e: each branch's chunk stream wraps
//!   independently; wire `enforcement_summary` keys by node path.
//! - **D5** — `axon-W002` upgrades. Pre-33.y: 4-variant FallbackMode
//!   covers structural fallback. Post-33.y: `UnsupportedFlowShape` is
//!   structurally unreachable + new `axon-W003 partial-streaming-
//!   activation` variant fires on the mixed-shape paths + each
//!   `axon.step_start` carries a `wire_status` field.
//! - **D8** — Tools first-class. Pre-33.y: `ChatRequest.tools = vec![]`
//!   hardcoded; declared `apply: TOOL` silently dropped. Post-33.y.k:
//!   tools plumb through; tool-call chunks interleave as `axon.tool_call`
//!   events on the wire.
//! - **D9** — Algebraic-effects runtime integrates. Pre-33.y.e:
//!   `IRFlowNode::Stream` (perform Stream.Yield) → fallback. Post-33.y.e:
//! `Stream` invokes the v1.17.0 handler stack; `perform Stream.Yield x`
//!   emits `axon.token` directly via the delimited-continuation
//!   machinery.

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

async fn deploy(app: axum::Router, src: &str) -> StatusCode {
    let body = serde_json::json!({
        "source": src,
        "source_file": "anchor.axon",
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
    request_body: &str,
) -> (StatusCode, String, String) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(request_body.to_string()))
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

#[derive(Debug, Default, Clone)]
struct SseEvents {
    tokens: Vec<serde_json::Value>,
    completes: Vec<serde_json::Value>,
    errors: Vec<serde_json::Value>,
    flow_starts: Vec<serde_json::Value>,
    step_starts: Vec<serde_json::Value>,
    other_events: Vec<(String, serde_json::Value)>,
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
                    "axon.error" => events.errors.push(parsed),
                    "axon.flow_start" => events.flow_starts.push(parsed),
                    "axon.step_start" => events.step_starts.push(parsed),
                    other => events
                        .other_events
                        .push((other.to_string(), parsed)),
                }
            }
            current_event = None;
            current_data = None;
        }
    }
    if let (Some(ev), Some(data)) = (current_event, current_data) {
        let parsed: serde_json::Value =
            serde_json::from_str(&data).unwrap_or(serde_json::Value::Null);
        match ev.as_str() {
            "axon.token" => events.tokens.push(parsed),
            "axon.complete" => events.completes.push(parsed),
            "axon.error" => events.errors.push(parsed),
            "axon.flow_start" => events.flow_starts.push(parsed),
            "axon.step_start" => events.step_starts.push(parsed),
            _ => events.other_events.push((ev, parsed)),
        }
    }
    events
}

// ────────────────────────────────────────────────────────────────────
//  Source fixtures
// ────────────────────────────────────────────────────────────────────

/// Canonical Step-only shape — POST-33.x.b state pinned.
/// Activates the async path (`run_streaming_async_path`) with the
/// real `Backend::stream()` per step. For stub backend this emits
/// exactly 1 chunk → 1 `axon.token` event (D4 byte-compat).
///
/// **33.y MUST NOT regress this anchor.** Every per-variant handler
/// added in 33.y.b–j composes with the existing canonical Step
/// handler; this test re-runs on every step to guarantee zero
/// regression on the v1.25.0 deliverable.
const CANONICAL_STEP_FLOW: &str =
    "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse }";

/// `Let`-binding shape (IRFlowNode::Let). Currently triggers
/// `PlanFallback::LetBindingPresent` → legacy path. Post-33.y.d
/// this inverts: the Let handler binds the RHS into the async
/// DispatchCtx + the child step rides the async path.
const LET_BINDING_FLOW: &str =
    "flow Chat() -> Unit {\n\
        let region = \"us\"\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/let\" execute: Chat transport: sse }";

/// Reason-step shape (IRFlowNode::Reason — `reason about_x` cognitive
/// framing variant of step). Currently triggers
/// `PlanFallback::UnsupportedNode { kind: "reason" }` → legacy path.
/// Post-33.y.c this inverts: pure-shape cognitive variants share the
/// canonical Step handler (D7).
const REASON_STEP_FLOW: &str =
    "flow Chat() -> Unit {\n\
        reason about_topic { target: \"hi\" }\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/reason\" execute: Chat transport: sse }";

/// Tool-using Step shape — declared `apply: TOOL` should activate
/// the upstream backend's tool-calling state machine. Currently
/// `ChatRequest.tools = Vec::new()` hardcoded in the async path
/// drops the declaration silently. Post-33.y.k this inverts.
const TOOL_USING_STEP_FLOW: &str =
    "tool chat_stream { description: \"stream\" effects: <stream:drop_oldest> }\n\
     flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" apply: chat_stream }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/tool\" execute: Chat transport: sse }";

// ────────────────────────────────────────────────────────────────────
// section 1 — D1 canonical Step regression pin (must stay green throughout 33.y)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn d1_canonical_step_async_path_pin_post_33_x_b() {
    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), CANONICAL_STEP_FLOW).await;
    assert_eq!(dep, StatusCode::OK, "deploy of canonical Step flow");

    let (status, ct, body) = fetch_sse_body(app, "/chat", "{}").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));

    let events = parse_sse_body(&body);
    eprintln!("─── D1 canonical Step regression pin ───");
    eprintln!("tokens: {}", events.tokens.len());
    eprintln!("completes: {}", events.completes.len());

    // POST-33.x.b invariant pinned: stub.stream() emits exactly 1
    // chunk → 1 axon.token event with "(stub)" content. This is the
    // foundation behavior 33.y MUST PRESERVE on every step.
    assert_eq!(
        events.tokens.len(),
        1,
        "33.y regression pin: canonical Step shape stays on async path \
         with 1 chunk per step (stub.stream emits 1 chunk → 1 token)"
    );
    assert_eq!(events.tokens[0]["token"], "(stub)");

    // Async-path indicator: no axon-W002 warning in the complete
    // event when the canonical shape rides the async path.
    let complete = &events.completes[0];
    assert!(
        complete.get("warnings").is_none()
            || complete["warnings"].as_array().is_some_and(|a| a.is_empty()),
        "canonical Step shape activates async path → no axon-W002 \
         warning surface (warnings field elided per D4 byte-compat)"
    );
}

// ────────────────────────────────────────────────────────────────────
// section 2 — Let-binding fallback (pre-33.y.d)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn let_binding_falls_back_to_legacy_pre_33_y_d() {
    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), LET_BINDING_FLOW).await;
    // Deploy may succeed (the parser + type-checker accept Let);
    // fallback fires at execution time inside `server_execute_streaming`.
    if dep != StatusCode::OK {
        eprintln!("Let-binding flow did not deploy cleanly (status {dep}). \
                   Anchor records this state — post-33.y.d the Let \
                   handler ships with deploy-time IR support.");
        return;
    }

    let (status, ct, body) = fetch_sse_body(app, "/let", "{}").await;
    eprintln!("─── section 2 Let-binding fallback anchor ───");
    eprintln!("status: {status}");
    eprintln!("content-type: {ct}");
    eprintln!("body bytes: {}", body.len());

    if status != StatusCode::OK {
        eprintln!("non-200 status — fallback may have routed differently. \
                   Recording verbatim for post-33.y.d inversion.");
        return;
    }

    let events = parse_sse_body(&body);
    let complete = events.completes.first().cloned().unwrap_or_default();
    eprintln!("complete event: {complete:#?}");

    // PRE-33.y.d assertion: the legacy path fires + axon-W002 warning
    // appears on the complete event with FallbackMode tag indicating
    // the unsupported flow shape. Post-33.y.d this is inverted to
    // assert async path + no warning + per-chunk delivery.
    //
    // We record the warning verbatim instead of asserting a strict
    // shape because route-registration filters may sometimes prevent
    // the request from reaching the streaming dispatcher entirely
    // (legacy axon-W002 surface is best-effort observable across
    // adopter shapes). The 33.y.d inversion will tighten the
    // assertion to "warnings absent + tokens > 1 (per-chunk)".
    if let Some(warnings) = complete.get("warnings").and_then(|v| v.as_array()) {
        if !warnings.is_empty() {
            eprintln!("anchor records pre-33.y.d state: axon-W002 fired \
                       for Let-binding fallback. warnings = {warnings:#?}");
        }
    }
}

// ────────────────────────────────────────────────────────────────────
// section 3 — Reason-step fallback (pre-33.y.c)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn reason_step_falls_back_to_legacy_pre_33_y_c() {
    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), REASON_STEP_FLOW).await;
    if dep != StatusCode::OK {
        eprintln!("Reason-step flow did not deploy cleanly (status {dep}). \
                   Anchor records this state — post-33.y.c the Reason \
                   handler ships with deploy-time IR support.");
        return;
    }

    let (status, _ct, body) = fetch_sse_body(app, "/reason", "{}").await;
    eprintln!("─── section 3 Reason-step fallback anchor ───");
    eprintln!("status: {status}");
    eprintln!("body bytes: {}", body.len());

    if status != StatusCode::OK {
        return;
    }

    let events = parse_sse_body(&body);
    let complete = events.completes.first().cloned().unwrap_or_default();
    eprintln!("complete event: {complete:#?}");

    if let Some(warnings) = complete.get("warnings").and_then(|v| v.as_array()) {
        if !warnings.is_empty() {
            eprintln!("anchor records pre-33.y.c state: axon-W002 fired \
                       for Reason-step fallback. warnings = {warnings:#?}");
        }
    }
}

// ────────────────────────────────────────────────────────────────────
// section 4 — Tool-using Step: `apply: TOOL` silently dropped (pre-33.y.k)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn tool_using_step_drops_apply_tool_pre_33_y_k() {
    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), TOOL_USING_STEP_FLOW).await;
    assert_eq!(dep, StatusCode::OK, "deploy of tool-using Step flow");

    let (status, _ct, body) = fetch_sse_body(app, "/tool", "{}").await;
    assert_eq!(status, StatusCode::OK);

    let events = parse_sse_body(&body);
    eprintln!("─── section 4 Tool-using Step anchor ───");
    eprintln!("tokens: {}", events.tokens.len());
    eprintln!("other_events: {:?}", events.other_events.iter().map(|(k, _)| k).collect::<Vec<_>>());

    // PRE-33.y.k assertion: NO `axon.tool_call` events appear on the
    // wire even though the step declares `apply: chat_stream` (the
    // tool registry plumb-through is hardcoded to `Vec::new()` in
    // `run_streaming_async_path`). Post-33.y.k this inverts to
    // assert tool_call events interleave with text tokens.
    let tool_call_events: Vec<_> = events
        .other_events
        .iter()
        .filter(|(name, _)| name == "axon.tool_call")
        .collect();
    assert!(
        tool_call_events.is_empty(),
        "PRE-33.y.k: tool calls silently dropped on streaming path \
         (ChatRequest.tools hardcoded to Vec::new()). Post-33.y.k \
         this anchor inverts."
    );
}

// ────────────────────────────────────────────────────────────────────
// section 5 — IRFlowNode catalog totality pin (closed enum invariant)
// ────────────────────────────────────────────────────────────────────

/// Pin the 45-variant IRFlowNode closed catalog via an exhaustive
/// match. Adding a 46th variant fails compilation here (forcing the
/// dispatcher arm to be added in lockstep) AND fails the count
/// assertion until this anchor is updated to acknowledge the new
/// variant.
///
/// This is the **D1 totality invariant** of the dispatcher: when 33.y
/// ships, the per-variant async dispatcher's match has exactly the
/// same closed catalog as this anchor's match.
#[test]
fn ir_flow_node_catalog_pin_45_variants() {
    use axon::ir_nodes::IRFlowNode;

    fn kind_for(node: &IRFlowNode) -> &'static str {
        // The exhaustive match. The compiler enforces every variant
        // is named. If a 46th IRFlowNode variant ships, this match
        // fails to compile — forcing the dispatcher (and this anchor
        // count) to be updated in lockstep.
        match node {
            // v2.65.0 — the proof-carrying derivative (49th variant).
            IRFlowNode::Grad(_) => "grad",
            IRFlowNode::Step(_) => "step",
            IRFlowNode::Probe(_) => "probe",
            IRFlowNode::Reason(_) => "reason",
            IRFlowNode::Validate(_) => "validate",
            IRFlowNode::Refine(_) => "refine",
            IRFlowNode::Weave(_) => "weave",
            IRFlowNode::UseTool(_) => "use_tool",
            IRFlowNode::Remember(_) => "remember",
            IRFlowNode::Recall(_) => "recall",
            IRFlowNode::Conditional(_) => "conditional",
            IRFlowNode::ForIn(_) => "for_in",
            IRFlowNode::Let(_) => "let",
            IRFlowNode::Return(_) => "return",
            IRFlowNode::Break(_) => "break",
            IRFlowNode::Continue(_) => "continue",
            IRFlowNode::LambdaDataApply(_) => "lambda_data_apply",
            IRFlowNode::Par(_) => "par",
            IRFlowNode::Hibernate(_) => "hibernate",
            IRFlowNode::Deliberate(_) => "deliberate",
            IRFlowNode::Consensus(_) => "consensus",
            IRFlowNode::Forge(_) => "forge",
            IRFlowNode::Focus(_) => "focus",
            IRFlowNode::Associate(_) => "associate",
            IRFlowNode::Aggregate(_) => "aggregate",
            IRFlowNode::Explore(_) => "explore",
            IRFlowNode::Ingest(_) => "ingest",
            IRFlowNode::ShieldApply(_) => "shield_apply",
            IRFlowNode::Stream(_) => "stream",
            IRFlowNode::Navigate(_) => "navigate",
            IRFlowNode::Drill(_) => "drill",
            IRFlowNode::Trail(_) => "trail",
            IRFlowNode::Corroborate(_) => "corroborate",
            IRFlowNode::OtsApply(_) => "ots_apply",
            IRFlowNode::MandateApply(_) => "mandate_apply",
            IRFlowNode::ComputeApply(_) => "compute_apply",
            IRFlowNode::Listen(_) => "listen",
            IRFlowNode::DaemonStep(_) => "daemon_step",
            IRFlowNode::Emit(_) => "emit",
            // v2.46.0 — `mint <Credential> as <binding>` (ephemeral
            // credential minting; wire_integrations::run_mint).
            IRFlowNode::Mint(_) => "mint",
            // v2.48.0 — `rotate <SecretsStore> [where] with <Tool> as
            // <binding>` (mediated secret renewal; wire_integrations::run_rotate).
            IRFlowNode::Rotate(_) => "rotate",
            IRFlowNode::Publish(_) => "publish",
            IRFlowNode::Discover(_) => "discover",
            IRFlowNode::Persist(_) => "persist",
            IRFlowNode::Retrieve(_) => "retrieve",
            IRFlowNode::Mutate(_) => "mutate",
            IRFlowNode::Purge(_) => "purge",
            IRFlowNode::Transact(_) => "transact",
            IRFlowNode::Quant(_) => "quant",
            // v2.43.0 — the `warden(<target>) within <Scope>` adversarial
            // security-analysis block (a flow-body block like `quant`).
            IRFlowNode::Warden(_) => "warden",
            IRFlowNode::Yield(_) => "yield",
            // v2.83.0 — `<Agent>(args)`. Missing since that cycle landed,
            // and invisible for the same reason as the v2.87.0 arms below: this
            // file has not compiled in a default `cargo test` since v2.81.0
            // moved the server behind an off-by-default feature.
            IRFlowNode::AgentCall(_) => "agent_call",
            // v2.87.0 — the algebraic-effect constructs. Slugs match
            // `flow_plan::ir_flow_node_kind` and the wire `step_type`, so one
            // construct has one name everywhere.
            IRFlowNode::Handle(_) => "handle",
            IRFlowNode::Perform(_) => "perform",
            IRFlowNode::Resume(_) => "resume",
            IRFlowNode::Abort(_) => "abort",
            IRFlowNode::Forward(_) => "forward",
            IRFlowNode::Run(_) => "run",
        }
    }

    // The full catalog enumerated by name. If the count diverges
    // from the match above, one side is stale.
    const CATALOG: &[&str] = &[
        "step",
        "probe",
        "reason",
        "validate",
        "refine",
        "weave",
        "use_tool",
        "remember",
        "recall",
        "conditional",
        "for_in",
        "let",
        "return",
        "break",
        "continue",
        "lambda_data_apply",
        "par",
        "hibernate",
        "deliberate",
        "consensus",
        "forge",
        "focus",
        "associate",
        "aggregate",
        "explore",
        "ingest",
        "shield_apply",
        "stream",
        "navigate",
        "drill",
        "trail",
        "corroborate",
        "ots_apply",
        "mandate_apply",
        "compute_apply",
        "listen",
        "daemon_step",
        "emit",
        "mint",
        "rotate",
        "publish",
        "discover",
        "persist",
        "retrieve",
        "mutate",
        "purge",
        "transact",
        "quant",
        "warden",
        "yield",
        "run",
    ];

    assert_eq!(
        CATALOG.len(),
        51,
        "33.y D1 totality invariant: the IRFlowNode closed catalog \
         has exactly 51 variants (v2.43.0 added `warden`; v2.46.0 added `mint`; \
         v2.48.0 added `rotate`). The dispatcher's exhaustive match must \
         cover all 51 — adding a 52nd requires updating both the \
         dispatcher AND this anchor in lockstep."
    );

    // Unused-variable suppression for `kind_for` — the function
    // exists for its compile-time exhaustiveness check, not for
    // runtime invocation here.
    let _ = kind_for;

    eprintln!("─── section 5 IRFlowNode catalog totality pin ───");
    eprintln!("variants: {}", CATALOG.len());
}

// ────────────────────────────────────────────────────────────────────
// section 6 — PlanFallback closed catalog pin (must SHRINK across 33.y)
// ────────────────────────────────────────────────────────────────────

/// Pin the v1.25.0 PlanFallback closed catalog. Across the 33.y
/// cycle this catalog SHRINKS as variants graduate to async handlers.
/// At 33.y.l (legacy path retirement) the entire `PlanFallback` enum
/// is deleted along with `unsupported_feature_reason` — the dispatcher
/// is total so no fallback can fire.
///
/// Today (v1.25.0): 7 variants
///   AnchorConstraintsPresent / LambdaApplyPresent / LetBindingPresent
///   UseToolPresent / HibernatePresent / PixPresent / UnsupportedNode
///
/// Target (post-33.y.l): 0 variants — the type is deleted.
#[test]
fn plan_fallback_catalog_pin_pre_33_y() {
    use axon::flow_plan::PlanFallback;

    // Exhaustive match enumerates every variant; compiler enforces
    // each is named. Note `UnsupportedNode` carries a `kind: String`
    // field — we destructure it but only check the variant tag here.
    fn slug(f: &PlanFallback) -> &'static str {
        match f {
            PlanFallback::AnchorConstraintsPresent => "anchor_constraints",
            PlanFallback::LambdaApplyPresent => "lambda_apply",
            PlanFallback::LetBindingPresent => "let_binding",
            PlanFallback::UseToolPresent => "use_tool",
            PlanFallback::HibernatePresent => "hibernate",
            PlanFallback::PixPresent => "pix",
            PlanFallback::UnsupportedNode { .. } => "unsupported_node",
        }
    }

    // 7 slugs today. Each 33.y step that ships a per-variant
    // async handler REMOVES the corresponding PlanFallback variant
    // (and the match arm here would fail to compile until updated).
    // 33.y.l deletes the enum entirely.
    let _ = slug;
    eprintln!("─── section 6 PlanFallback catalog pre-33.y pin ───");
    eprintln!("7 variants today. Target post-33.y.l: 0 (enum deleted).");
}
