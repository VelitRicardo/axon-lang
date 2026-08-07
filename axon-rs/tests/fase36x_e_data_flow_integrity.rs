//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 36.x.e (D4) — data-flow integrity on the streaming
//! dispatcher path.
//!
//! The agent pattern is a DATA pipeline: retrieve context →
//! deliberate (the step) → persist the result. The data must
//! THREAD between the nodes. The 36.x.e audit found the streaming
//! dispatcher path diverged from the synchronous path's
//! interpolation contract (Fase 35.q) on two legs:
//!
//!   Leg A — a `retrieve … as: alias` (or a prior `step`) binds a
//!     value, but `run_step` sent `step.ask` VERBATIM — `${alias}`
//!     never resolved into the step's prompt / tool argument.
//!   Leg B — a `step`'s output was returned as `NodeOutcome`, but
//!     never bound into `ctx.let_bindings` — so a downstream
//!     `persist` / `step` could not reference it as `${StepName}`.
//!
//! 36.x.e fixed both: `run_step` interpolates `ask` against
//! `ctx.let_bindings`; `run_pure_shape` + `run_step_streaming_tool`
//! bind the step output under the step name. This pack pins it.

use axon::axon_server::{build_router, ServerConfig};
use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::pure_shape::run_step;
use axon::flow_dispatcher::DispatchCtx;
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::IRStep;
use axon::tool_registry::{ToolEntry, ToolRegistry, ToolSource};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use std::sync::Arc;
use tokio::sync::mpsc;
use tower::ServiceExt;

// ─── unit-test scaffolding (mirrors fase34_d_dispatcher_arm.rs) ─────

fn step_with_apply(name: &str, ask: &str, apply_ref: &str) -> IRStep {
    IRStep {
        node_type: "step",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        persona_ref: String::new(),
        given: String::new(),
        ask: ask.into(),
        use_tool: None,
        probe: None,
        reason: None,
        weave: None,
        output_type: String::new(),
        confidence_floor: None,
        navigate_ref: String::new(),
        apply_ref: apply_ref.into(),
        guards: Vec::new(),
        requires_context: None,        now_tz: None,        body: Vec::new(),
    }
}

/// A `provider: stub_stream` tool — `StubStreamingTool` echoes its
/// argument verbatim (`[stub-stream] <name>(<args>)`), so the
/// interpolated prompt is observable on the wire.
fn echo_tool(name: &str) -> ToolEntry {
    ToolEntry {
        name: name.into(),
        provider: "stub_stream".into(),
        timeout: String::new(),
        runtime: String::new(),
        resource_ref: String::new(),
        substrate: None,
        capacity: None,
        sandbox: None,
        max_results: None,
        output_schema: String::new(),
        effect_row: vec!["stream:drop_oldest".to_string()],
        parameters: Vec::new(),
        secret: String::new(),
        secret_partition: String::new(),
        source: ToolSource::Program,
        is_streaming: true,
        scrape: None,
    }
}

fn ctx_with_echo() -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut reg = ToolRegistry::new();
    reg.register(echo_tool("Echo"));
    let ctx = DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx)
        .with_tool_registry(Arc::new(reg));
    (ctx, rx)
}

fn drain_tokens(rx: &mut mpsc::UnboundedReceiver<FlowExecutionEvent>) -> String {
    let mut out = String::new();
    while let Ok(ev) = rx.try_recv() {
        if let FlowExecutionEvent::StepToken { content, .. } = ev {
            out.push_str(&content);
        }
    }
    out
}

// ─── §1 — Leg A: `run_step` interpolates `${alias}` in `ask` ───────

#[tokio::test]
async fn s1_step_ask_interpolates_a_bound_alias() {
    let (mut ctx, mut rx) = ctx_with_echo();
    // A `retrieve … as: history` would have bound this.
    ctx.let_bindings
        .insert("history".to_string(), "RETRIEVED_HX".to_string());

    let step = step_with_apply("Deliberate", "context=${history}", "Echo");
    run_step(&step, &mut ctx).await.expect("run_step");

    let wire = drain_tokens(&mut rx);
    assert!(
        wire.contains("context=RETRIEVED_HX"),
        "36.x.e D4 Leg A: `${{history}}` in the step's `ask` must \
         interpolate to the bound value before it becomes the tool \
         argument. Wire: {wire}"
    );
    assert!(
        !wire.contains("${history}"),
        "36.x.e D4 Leg A: the literal `${{history}}` must NOT survive \
         to the wire — it must be substituted. Wire: {wire}"
    );
}

// ─── §2 — Leg B: a step's output is bound under its name ───────────

#[tokio::test]
async fn s2_step_output_is_bound_under_the_step_name() {
    let (mut ctx, _rx) = ctx_with_echo();
    let step = step_with_apply("Generate", "deliberate", "Echo");
    run_step(&step, &mut ctx).await.expect("run_step");

    let bound = ctx.let_bindings.get("Generate");
    assert!(
        bound.is_some_and(|v| !v.is_empty() && v.contains("deliberate")),
        "36.x.e D4 Leg B: the step's output must be bound into \
         `ctx.let_bindings` under the step name `Generate` so a \
         downstream `persist`/`step` can reference `${{Generate}}`. \
         Got: {bound:?}"
    );
}

// ─── §3 — composition: a step reads a prior step's output ──────────

#[tokio::test]
async fn s3_downstream_step_interpolates_a_prior_steps_output() {
    let (mut ctx, mut rx) = ctx_with_echo();

    // Step A — produces output, bound under `First` (Leg B).
    let step_a = step_with_apply("First", "alpha", "Echo");
    run_step(&step_a, &mut ctx).await.expect("step A");
    let _ = drain_tokens(&mut rx); // clear A's wire events

    // Step B — its `ask` references `${First}` (Leg A).
    let step_b = step_with_apply("Second", "prev=${First}", "Echo");
    run_step(&step_b, &mut ctx).await.expect("step B");
    let wire_b = drain_tokens(&mut rx);

    assert!(
        wire_b.contains("alpha"),
        "36.x.e D4: step A's output must thread into step B's \
         `${{First}}` interpolation — the agent pattern's data \
         pipeline (retrieve → deliberate → persist) composes on the \
         streaming dispatcher path. Step B wire: {wire_b}"
    );
}

// ─── §4 — no-regression: the canonical agent flow still streams ────

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

#[tokio::test]
async fn s4_agent_flow_with_interpolation_streams_clean() {
    let app = build_router(server_cfg());
    let src = "axonstore mem { backend: in_memory }\n\
        flow ChatFlow() -> Unit {\n\
            retrieve mem { where: \"kind = 'history'\" as: history }\n\
            step Generate { ask: \"given ${history} answer\" output: Stream<Token> }\n\
            persist into mem { kind: \"reply\" content: \"${Generate}\" }\n\
        }\n\
        axonendpoint ChatE { public: true method: POST path: \"/chat\" execute: ChatFlow \
        backend: stub transport: sse }";
    let dep = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::json!({ "source": src }).to_string()))
        .unwrap();
    let dresp = app.clone().oneshot(dep).await.unwrap();
    assert_eq!(dresp.status(), StatusCode::OK, "deploy");

    let req = Request::builder()
        .method("POST")
        .uri("/chat")
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let wire = String::from_utf8_lossy(&bytes);
    assert!(
        wire.contains("axon.complete") && !wire.contains("axon.error"),
        "36.x.e D5: a mixed flow whose `step` interpolates a retrieve \
         alias and whose `persist` interpolates the step output must \
         stream cleanly — the interpolation + output-binding wiring \
         introduces no regression. Wire:\n{wire}"
    );
}
