//! §Fase 119.f.7 — **the step-body statement is DISPATCHED**, not merely parsed.
//!
//! **What was wrong.** §119.f gave the PIX verbs (`navigate` / `drill` / `trail`
//! / `validate`) and the step-scoped invocations (`probe … for […]`,
//! `use_tool … with …`, `par { … }`) a position inside a `step { }` body, an
//! AST slot ([`axon::ast::StepNode::pix_ops`]), an IR field
//! ([`axon::ir_nodes::IRStep::pix_ops`]) — and **no reader in the runtime**.
//! Measured 2026-08-08 on 2.85.0: `.pix_ops` had exactly ONE reader in the whole
//! workspace, the frontend's own grammar test. `pure_shape::run_step` walked
//! `step.guards` (the §119.c/§119.d elevations) and never looked at
//! `step.pix_ops`.
//!
//! Both doc comments asserted the opposite, in these words:
//!
//! > *"They are ELEVATIONS: dispatch runs them before the step generates, so
//! > each `as:` binding is in scope for the step's `ask:`."*
//!
//! That sentence was false for every one of them. Ten README rows left the §117
//! ledger on the strength of programs that compiled and then did nothing — the
//! §111 shape (`warden`/`quant` had a badge, a registry entry, a parser
//! production AND a dispatch arm, and were no-ops) reproduced one fase later,
//! inside the fase built to end it. It is the [`feedback_ask_where_production_calls_it`]
//! question, unasked: *which dispatcher reads this field?*
//!
//! **The law this pins.** A statement written in a step body is the flow-level
//! node run in the step's position — same handler, same wire events, same
//! binding — and it runs BEFORE the step's own generation, so its binding is in
//! scope for the `ask:` that interpolates it. Two observable consequences, one
//! per test below:
//!
//!   1. the op reaches the wire at all (`step_type` slug present);
//!   2. it reaches the wire BEFORE the step's own `step_start` — "elevation" is
//!      an ordering claim, and an ordering claim is only true if something
//!      observes the order.
//!
//! Mutation-verified in both directions (delete the dispatch loop → §1 red;
//! move it after the generation → §2 red).

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::pure_shape::run_step;
use axon::flow_dispatcher::DispatchCtx;
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::{IRFlowNode, IRProbe, IRStep};
use tokio::sync::mpsc;

fn bare_step(name: &str, ask: &str) -> IRStep {
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
        apply_ref: String::new(),
        pix_ops: Vec::new(),
        guards: Vec::new(),
        requires_context: None,
        now_tz: None,
        body: Vec::new(),
    }
}

/// `probe sessions for [...]` written inside the step body — the README shape
/// (blocks 31/33) that §119.f made parse.
fn probe_op(target: &str) -> IRFlowNode {
    IRFlowNode::Probe(IRProbe {
        node_type: "probe",
        source_line: 0,
        source_column: 0,
        target: target.into(),
    })
}

fn ctx() -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx),
        rx,
    )
}

/// Every `step_start` slug reaching the wire, in emission order.
fn step_start_slugs(rx: &mut mpsc::UnboundedReceiver<FlowExecutionEvent>) -> Vec<String> {
    let mut slugs = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let FlowExecutionEvent::StepStart { step_type, .. } = ev {
            slugs.push(step_type);
        }
    }
    slugs
}

// ── §1 — the statement runs ──────────────────────────────────────────────────

#[tokio::test]
async fn s1_a_statement_in_a_step_body_reaches_the_dispatcher() {
    let (mut c, mut rx) = ctx();
    let mut step = bare_step("Initialize", "summarise the baseline");
    step.pix_ops = vec![probe_op("sessions")];

    run_step(&step, &mut c).await.expect("run_step");

    let slugs = step_start_slugs(&mut rx);
    assert!(
        slugs.iter().any(|s| s == "probe"),
        "a `probe` written in a step body must be DISPATCHED, not parsed and \
         dropped. The IR carried it (`IRStep.pix_ops`) and the wire never saw \
         it. Slugs observed: {slugs:?}"
    );
}

/// The binding the statement produces must exist afterwards — that is what the
/// step's `ask:` interpolates against.
#[tokio::test]
async fn s1b_the_statement_binds_its_result() {
    let (mut c, _rx) = ctx();
    let mut step = bare_step("Initialize", "summarise the baseline");
    step.pix_ops = vec![probe_op("sessions")];

    run_step(&step, &mut c).await.expect("run_step");

    assert!(
        c.let_bindings.contains_key("sessions"),
        "the elevation's output must be bound so `ask: \"… {{sessions}} …\"` can \
         interpolate it. Bindings: {:?}",
        c.let_bindings.keys().collect::<Vec<_>>()
    );
}

// ── §2 — it runs BEFORE the step generates ───────────────────────────────────

#[tokio::test]
async fn s2_the_statement_is_an_elevation_it_precedes_the_generation() {
    let (mut c, mut rx) = ctx();
    let mut step = bare_step("Initialize", "summarise ${sessions}");
    step.pix_ops = vec![probe_op("sessions")];

    run_step(&step, &mut c).await.expect("run_step");

    let slugs = step_start_slugs(&mut rx);
    let probe_at = slugs.iter().position(|s| s == "probe");
    let step_at = slugs.iter().position(|s| s == "step");
    assert!(
        matches!((probe_at, step_at), (Some(p), Some(s)) if p < s),
        "\"dispatch runs them BEFORE the step generates\" is an ORDERING claim: \
         the elevation's `step_start` must precede the step's own, or the \
         binding is not in scope for the `ask:` that names it. Slugs: {slugs:?}"
    );
}

// ── §2b — the elevation fails CLOSED ─────────────────────────────────────────

/// A statement that cannot run must stop the step, not be stepped over. The
/// §112 kernel defect was exactly the opposite reflex — "if the evidence is
/// missing, substitute the belief" — and a step that generates over evidence
/// its own body said to gather and could not is that defect with a new spelling.
///
/// The observable case is the CLOSED CATALOG's own refusal: a step body admits
/// seven statements, and `refine` is not one of them (the parser cannot place
/// it there). A generic dispatcher would have executed it — the catalog names
/// the variant and refuses, and the refusal must abort the step rather than
/// warn past it.
#[tokio::test]
async fn s2b_an_out_of_catalog_statement_is_refused_and_the_step_does_not_generate() {
    use axon::ir_nodes::IRRefineStep;

    let (mut c, mut rx) = ctx();
    let mut step = bare_step("Charge", "confirm ${PaymentResult}");
    step.pix_ops = vec![IRFlowNode::Refine(IRRefineStep {
        node_type: "refine",
        source_line: 0,
        source_column: 0,
        target: "draft".into(),
        strategy: "tighten".into(),
    })];

    let outcome = run_step(&step, &mut c).await;

    let err = outcome.err().unwrap_or_else(|| {
        panic!(
            "a statement outside the step-body catalog must be REFUSED — executing \
             grammar the parser cannot produce is how a dispatch table grows a \
             catalog nobody decided (§111)"
        )
    });
    let msg = format!("{err:?}");
    assert!(
        msg.contains("refine"),
        "the refusal must NAME the variant it refused, or the omission is \
         indistinguishable from silence. Got: {msg}"
    );
    let slugs = step_start_slugs(&mut rx);
    assert!(
        !slugs.iter().any(|s| s == "step"),
        "the step must NOT have generated after its elevation was refused — the \
         `?` on the dispatch is what makes the elevation fail CLOSED. Slugs: {slugs:?}"
    );
}

// ── §3 — a step with no statements is unchanged ──────────────────────────────

/// The dispatch loop must be invisible to every pre-§119.f program: no extra
/// wire events, no extra bindings. (A gate that only ever fires forward is how
/// a "fix" silently reshapes the wire for existing adopters.)
#[tokio::test]
async fn s3_a_step_without_statements_emits_exactly_one_step_start() {
    let (mut c, mut rx) = ctx();
    let step = bare_step("Plain", "just ask");

    run_step(&step, &mut c).await.expect("run_step");

    let slugs = step_start_slugs(&mut rx);
    assert_eq!(
        slugs,
        vec!["step".to_string()],
        "a step with an empty `pix_ops` must emit exactly what it emitted \
         before §119.f.7"
    );
}
