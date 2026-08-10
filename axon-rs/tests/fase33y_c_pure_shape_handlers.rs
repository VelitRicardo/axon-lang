//! §Fase 33.y.c integration tests — pure-shape async handlers
//! (Step / Probe / Reason / Validate / Refine / Weave).
//!
//! These tests exercise the public surface of
//! `axon::flow_dispatcher::pure_shape` end-to-end with the stub
//! backend. Each variant gets:
//!
//! - One happy-path test asserting `NodeOutcome::Completed` + the
//!   3-event wire emission (StepStart / StepToken / StepComplete) +
//!   `step_type` slug matching `flow_plan::ir_flow_node_kind`.
//! - One audit-row test asserting the per-step `StepAuditRecord`
//!   shape (step_name + step_index + success + tokens_emitted +
//!   output_hash_hex SHA-256 + effect_policy_applied None +
//!   chunks_dropped 0 + chunks_degraded 0).
//! - One effect-policy test asserting the enforcement summary fires
//!   when the caller supplies a `pending_effect_policy`.
//! - One cancel-propagation test asserting the handler short-
//!   circuits with `DispatchError::UpstreamCancelled` when the flag
//!   is set pre-dispatch.
//!
//! Plus cross-cutting tests:
//! - Multi-step counter: two consecutive handler invocations on the
//!   same ctx assign step_index 0 then 1.
//! - Pending-policy take-semantics: a policy set BEFORE one handler
//!   is consumed; the NEXT handler observes None (no leak).
//! - SHA-256 output_hash_hex stability: same input → same hash
//!   byte-for-byte (D6 content-addressable contract).
//! - System-prompt composition: framing addendum is appended after
//!   the ctx-level system prompt (newline-separated).
//! - 4-policy enforcer activation: each of DropOldest /
//!   DegradeQuality / PauseUpstream / Fail records a summary with
//!   the correct slug.
//!
//! D-letter coverage:
//! - D1 — every pure-shape variant has a NAMED handler returning
//!   Completed (no `_ =>` fallback).
//! - D2 — pending_effect_policy is consumed pre-dispatch + the
//!   enforcement_summaries side-channel is populated keyed by step
//!   name.
//! - D3 — cancel-flag honored pre-dispatch + after each .await
//!   boundary in the drain loop.
//! - D4 — stub backend wire shape: 1 StepToken with delta `"(stub)"`.
//! - D6 — StepAuditRecord populated with SHA-256 output_hash_hex
//!   per step.
//! - D7 — every error case routes through `DispatchError` (no
//!   panic; the no-panic invariant pinned in 33.y.b drift gate).
//! - D10 — semantic parity with the sync runner for stub backend:
//!   accumulated output equals "(stub)" for all 6 variants.

use axon::axonendpoint_replay::StepAuditRecord;
use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::pure_shape::{
    run_probe, run_reason, run_refine, run_step, run_validate, run_weave,
};
use axon::flow_dispatcher::{DispatchCtx, DispatchError, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::*;
use axon::stream_effect::BackpressurePolicy;
use tokio::sync::mpsc;

// ────────────────────────────────────────────────────────────────────
//  Synthetic IR factories
// ────────────────────────────────────────────────────────────────────

fn step(name: &str, ask: &str) -> IRStep {
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
        stream: None,
        performs: Vec::new(),
        guards: Vec::new(),
        requires_context: None,        now_tz: None,        body: Vec::new(),
    }
}

fn probe(target: &str) -> IRProbe {
    IRProbe {
        node_type: "probe",
        source_line: 0,
        source_column: 0,
        target: target.into(),
    }
}

fn reason(target: &str, strategy: &str) -> IRReasonStep {
    IRReasonStep {
        node_type: "reason",
        source_line: 0,
        source_column: 0,
        strategy: strategy.into(),
        target: target.into(),
        // §Fase 119.f.8 — the block form's fields; the positional
        // `reason <target>` shape this helper builds leaves them empty.
        given: String::new(),
        ask: String::new(),
        depth: None,
    }
}

fn validate(target: &str, rule: &str) -> IRValidateStep {
    IRValidateStep {
        node_type: "validate",
        source_line: 0,
        source_column: 0,
        target: target.into(),
        rule: rule.into(),
    }
}

fn refine(target: &str, strategy: &str) -> IRRefineStep {
    IRRefineStep {
        node_type: "refine",
        source_line: 0,
        source_column: 0,
        target: target.into(),
        strategy: strategy.into(),
    }
}

fn weave(target: &str, sources: Vec<&str>, format_type: &str, style: &str) -> IRWeaveStep {
    IRWeaveStep {
        node_type: "weave",
        source_line: 0,
        source_column: 0,
        sources: sources.into_iter().map(String::from).collect(),
        target: target.into(),
        format_type: format_type.into(),
        priority: Vec::new(),
        style: style.into(),
        // §Fase 119.f.9 — `include:` (the parts the synthesis must contain);
        // this helper builds the pre-§119.f.9 shape, so it stays empty.
        include: Vec::new(),
    }
}

fn fresh_ctx() -> (
    DispatchCtx,
    mpsc::UnboundedReceiver<FlowExecutionEvent>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = DispatchCtx::new(
        "TestFlow",
        "stub",
        "you are a test agent",
        CancellationFlag::new(),
        tx,
    );
    (ctx, rx)
}

fn drain_events(rx: &mut mpsc::UnboundedReceiver<FlowExecutionEvent>) -> Vec<FlowExecutionEvent> {
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    events
}

fn expect_completed(outcome: Result<NodeOutcome, DispatchError>) -> (String, u64, usize) {
    match outcome {
        Ok(NodeOutcome::Completed {
            output,
            tokens_emitted,
            step_index,
        }) => (output, tokens_emitted, step_index),
        other => panic!("expected Completed, got {other:?}"),
    }
}

// ────────────────────────────────────────────────────────────────────
//  §1 — Per-variant happy-path tests (6 tests)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn step_happy_path_emits_step_start_token_complete() {
    let (mut ctx, mut rx) = fresh_ctx();
    let s = step("Generate", "hi");
    let (output, tokens, idx) = expect_completed(run_step(&s, &mut ctx).await);
    assert_eq!(output, "(stub)");
    assert_eq!(tokens, 1);
    assert_eq!(idx, 0);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 3);
    match &events[0] {
        FlowExecutionEvent::StepStart {
            step_name,
            step_index,
            step_type,
            ..
        } => {
            assert_eq!(step_name, "Generate");
            assert_eq!(*step_index, 0);
            assert_eq!(step_type, "step");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
    match &events[1] {
        FlowExecutionEvent::StepToken {
            step_name,
            content,
            token_index,
            ..
        } => {
            assert_eq!(step_name, "Generate");
            assert_eq!(content, "(stub)");
            assert_eq!(*token_index, 1);
        }
        e => panic!("expected StepToken, got {e:?}"),
    }
    match &events[2] {
        FlowExecutionEvent::StepComplete {
            step_name,
            success,
            full_output,
            tokens_output,
            ..
        } => {
            assert_eq!(step_name, "Generate");
            assert!(*success);
            assert_eq!(full_output, "(stub)");
            assert_eq!(*tokens_output, 1);
        }
        e => panic!("expected StepComplete, got {e:?}"),
    }
}

#[tokio::test]
async fn probe_happy_path_step_type_is_probe() {
    let (mut ctx, mut rx) = fresh_ctx();
    let p = probe("market_data");
    let (output, _, _) = expect_completed(run_probe(&p, &mut ctx).await);
    assert_eq!(output, "(stub)");

    let events = drain_events(&mut rx);
    match &events[0] {
        FlowExecutionEvent::StepStart {
            step_name,
            step_type,
            ..
        } => {
            assert_eq!(step_type, "probe");
            assert_eq!(step_name, "market_data");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

#[tokio::test]
async fn reason_happy_path_step_type_is_reason() {
    let (mut ctx, mut rx) = fresh_ctx();
    let r = reason("claim", "chain_of_thought");
    expect_completed(run_reason(&r, &mut ctx).await);

    let events = drain_events(&mut rx);
    match &events[0] {
        FlowExecutionEvent::StepStart {
            step_name,
            step_type,
            ..
        } => {
            assert_eq!(step_type, "reason");
            assert_eq!(step_name, "claim");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

#[tokio::test]
async fn validate_happy_path_step_type_is_validate() {
    let (mut ctx, mut rx) = fresh_ctx();
    let v = validate("draft", "no_pii");
    expect_completed(run_validate(&v, &mut ctx).await);

    let events = drain_events(&mut rx);
    match &events[0] {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "validate");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

#[tokio::test]
async fn refine_happy_path_step_type_is_refine() {
    let (mut ctx, mut rx) = fresh_ctx();
    let r = refine("draft", "tighten");
    expect_completed(run_refine(&r, &mut ctx).await);

    let events = drain_events(&mut rx);
    match &events[0] {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "refine");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

#[tokio::test]
async fn weave_happy_path_step_type_is_weave() {
    let (mut ctx, mut rx) = fresh_ctx();
    let w = weave("report", vec!["A", "B"], "markdown", "formal");
    expect_completed(run_weave(&w, &mut ctx).await);

    let events = drain_events(&mut rx);
    match &events[0] {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "weave");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

// ────────────────────────────────────────────────────────────────────
//  §2 — Per-variant audit-row tests (6 tests)
// ────────────────────────────────────────────────────────────────────

async fn assert_audit_row_canonical(ctx: &DispatchCtx, step_name: &str) {
    let guard = ctx.step_audit_records.lock().await;
    assert_eq!(guard.len(), 1, "expected exactly 1 audit row, got {}", guard.len());
    let row: &StepAuditRecord = &guard[0];
    assert_eq!(row.step_name, step_name);
    assert_eq!(row.step_index, 0);
    assert!(row.success);
    assert_eq!(row.tokens_emitted, 1);
    assert_eq!(row.output_hash_hex.len(), 64);
    assert!(
        row.output_hash_hex
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    assert!(row.effect_policy_applied.is_none());
    assert_eq!(row.chunks_dropped, 0);
    assert_eq!(row.chunks_degraded, 0);
}

#[tokio::test]
async fn step_audit_row_recorded() {
    let (mut ctx, _rx) = fresh_ctx();
    run_step(&step("S1", "hi"), &mut ctx).await.unwrap();
    assert_audit_row_canonical(&ctx, "S1").await;
}

#[tokio::test]
async fn probe_audit_row_recorded() {
    let (mut ctx, _rx) = fresh_ctx();
    run_probe(&probe("data"), &mut ctx).await.unwrap();
    assert_audit_row_canonical(&ctx, "data").await;
}

#[tokio::test]
async fn reason_audit_row_recorded() {
    let (mut ctx, _rx) = fresh_ctx();
    run_reason(&reason("claim", "cot"), &mut ctx).await.unwrap();
    assert_audit_row_canonical(&ctx, "claim").await;
}

#[tokio::test]
async fn validate_audit_row_recorded() {
    let (mut ctx, _rx) = fresh_ctx();
    run_validate(&validate("doc", "rule"), &mut ctx).await.unwrap();
    assert_audit_row_canonical(&ctx, "doc").await;
}

#[tokio::test]
async fn refine_audit_row_recorded() {
    let (mut ctx, _rx) = fresh_ctx();
    run_refine(&refine("doc", "tighten"), &mut ctx).await.unwrap();
    assert_audit_row_canonical(&ctx, "doc").await;
}

#[tokio::test]
async fn weave_audit_row_recorded() {
    let (mut ctx, _rx) = fresh_ctx();
    run_weave(&weave("out", vec!["a"], "md", "neutral"), &mut ctx)
        .await
        .unwrap();
    assert_audit_row_canonical(&ctx, "out").await;
}

// ────────────────────────────────────────────────────────────────────
//  §3 — Effect-policy activation (D2) — 4 policies × 1 handler family
// ────────────────────────────────────────────────────────────────────

async fn run_step_with_policy(policy: BackpressurePolicy) -> DispatchCtx {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.pending_effect_policy = Some(policy);
    run_step(&step("S", "hi"), &mut ctx).await.expect("ok");
    ctx
}

#[tokio::test]
async fn enforcer_activates_for_drop_oldest_policy() {
    let ctx = run_step_with_policy(BackpressurePolicy::DropOldest).await;
    let summaries = ctx.enforcement_summaries.lock().await;
    let summary = summaries.get("S").expect("summary for step 'S'");
    assert_eq!(summary.policy_slug, "drop_oldest");
    // For stub backend (1 chunk), counters reflect the single
    // chunk pushed + delivered + zero policy fires.
    assert_eq!(summary.chunks_pushed, 1);
    assert_eq!(summary.chunks_delivered, 1);
    assert_eq!(summary.drop_oldest_hits, 0);
    assert_eq!(summary.degrade_quality_hits, 0);
    assert_eq!(summary.pause_upstream_blocks, 0);
    assert_eq!(summary.fail_overflows, 0);
    assert!(!summary.failed);

    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit[0].effect_policy_applied, Some("drop_oldest".into()));
}

#[tokio::test]
async fn enforcer_activates_for_degrade_quality_policy() {
    let ctx = run_step_with_policy(BackpressurePolicy::DegradeQuality).await;
    let summaries = ctx.enforcement_summaries.lock().await;
    let summary = summaries.get("S").expect("summary");
    assert_eq!(summary.policy_slug, "degrade_quality");
    assert!(!summary.failed);

    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit[0].effect_policy_applied, Some("degrade_quality".into()));
}

#[tokio::test]
async fn enforcer_activates_for_pause_upstream_policy() {
    let ctx = run_step_with_policy(BackpressurePolicy::PauseUpstream).await;
    let summaries = ctx.enforcement_summaries.lock().await;
    let summary = summaries.get("S").expect("summary");
    assert_eq!(summary.policy_slug, "pause_upstream");
    assert!(!summary.failed);
}

#[tokio::test]
async fn enforcer_activates_for_fail_policy() {
    let ctx = run_step_with_policy(BackpressurePolicy::Fail).await;
    let summaries = ctx.enforcement_summaries.lock().await;
    let summary = summaries.get("S").expect("summary");
    assert_eq!(summary.policy_slug, "fail");
    assert!(!summary.failed); // stub emits 1 chunk; cap not exceeded
}

// ────────────────────────────────────────────────────────────────────
//  §4 — Cancel propagation (D3) — each variant short-circuits
// ────────────────────────────────────────────────────────────────────

async fn assert_cancel_short_circuits<F, Fut>(invoke: F)
where
    F: FnOnce(DispatchCtx) -> Fut,
    Fut: std::future::Future<Output = Result<NodeOutcome, DispatchError>>,
{
    let cancel = CancellationFlag::new();
    cancel.cancel();
    let (tx, _rx) = mpsc::unbounded_channel();
    let ctx = DispatchCtx::new("F", "stub", "", cancel, tx);
    let outcome = invoke(ctx).await;
    assert!(
        matches!(outcome, Err(DispatchError::UpstreamCancelled)),
        "expected UpstreamCancelled, got {outcome:?}"
    );
}

#[tokio::test]
async fn step_cancel_short_circuits() {
    assert_cancel_short_circuits(|mut ctx| async move {
        run_step(&step("S", "hi"), &mut ctx).await
    })
    .await;
}

#[tokio::test]
async fn probe_cancel_short_circuits() {
    assert_cancel_short_circuits(|mut ctx| async move {
        run_probe(&probe("t"), &mut ctx).await
    })
    .await;
}

#[tokio::test]
async fn reason_cancel_short_circuits() {
    assert_cancel_short_circuits(|mut ctx| async move {
        run_reason(&reason("t", "cot"), &mut ctx).await
    })
    .await;
}

#[tokio::test]
async fn validate_cancel_short_circuits() {
    assert_cancel_short_circuits(|mut ctx| async move {
        run_validate(&validate("t", "r"), &mut ctx).await
    })
    .await;
}

#[tokio::test]
async fn refine_cancel_short_circuits() {
    assert_cancel_short_circuits(|mut ctx| async move {
        run_refine(&refine("t", "s"), &mut ctx).await
    })
    .await;
}

#[tokio::test]
async fn weave_cancel_short_circuits() {
    assert_cancel_short_circuits(|mut ctx| async move {
        run_weave(&weave("t", vec![], "md", "neutral"), &mut ctx).await
    })
    .await;
}

// ────────────────────────────────────────────────────────────────────
//  §5 — Cross-cutting semantics (5 tests)
// ────────────────────────────────────────────────────────────────────

/// Multi-step counter discipline: two consecutive handler invocations
/// on the same ctx reserve step_index 0 then 1 (matches sync runner
/// for D10 parity).
#[tokio::test]
async fn multi_step_invocations_advance_step_counter() {
    let (mut ctx, _rx) = fresh_ctx();
    let (_, _, idx0) = expect_completed(run_step(&step("A", "hi"), &mut ctx).await);
    let (_, _, idx1) = expect_completed(run_probe(&probe("B"), &mut ctx).await);
    assert_eq!(idx0, 0);
    assert_eq!(idx1, 1);
    assert_eq!(ctx.step_counter, 2);

    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit.len(), 2);
    assert_eq!(audit[0].step_name, "A");
    assert_eq!(audit[0].step_index, 0);
    assert_eq!(audit[1].step_name, "B");
    assert_eq!(audit[1].step_index, 1);
}

/// `pending_effect_policy` is consumed on handler entry; the NEXT
/// handler observes None (no policy leak).
#[tokio::test]
async fn pending_effect_policy_take_semantics_no_leak() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.pending_effect_policy = Some(BackpressurePolicy::DropOldest);

    run_step(&step("first", "hi"), &mut ctx).await.unwrap();
    assert!(
        ctx.pending_effect_policy.is_none(),
        "first handler MUST consume the policy"
    );

    run_step(&step("second", "hi"), &mut ctx).await.unwrap();
    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit.len(), 2);
    assert_eq!(
        audit[0].effect_policy_applied,
        Some("drop_oldest".into()),
        "first handler observes the policy"
    );
    assert_eq!(
        audit[1].effect_policy_applied, None,
        "second handler observes None (policy was consumed by first)"
    );
}

/// SHA-256 output_hash_hex stability — D6 content-addressable
/// contract: same input → same hash byte-for-byte.
#[tokio::test]
async fn output_hash_hex_is_deterministic_for_stub_backend() {
    let (mut ctx_a, _rx_a) = fresh_ctx();
    let (mut ctx_b, _rx_b) = fresh_ctx();
    run_step(&step("X", "hi"), &mut ctx_a).await.unwrap();
    run_step(&step("X", "hi"), &mut ctx_b).await.unwrap();

    let a = ctx_a.step_audit_records.lock().await;
    let b = ctx_b.step_audit_records.lock().await;
    assert_eq!(
        a[0].output_hash_hex, b[0].output_hash_hex,
        "D6: same '(stub)' input must produce byte-identical SHA-256"
    );
}

/// D10 semantic parity: all 6 variants produce the same accumulated
/// output for stub backend ("(stub)"). Only wire framing differs.
#[tokio::test]
async fn all_six_variants_produce_identical_stub_output() {
    let outputs = {
        let mut out = Vec::new();
        let (mut c, _r) = fresh_ctx();
        out.push(expect_completed(run_step(&step("a", "hi"), &mut c).await).0);
        let (mut c, _r) = fresh_ctx();
        out.push(expect_completed(run_probe(&probe("a"), &mut c).await).0);
        let (mut c, _r) = fresh_ctx();
        out.push(expect_completed(run_reason(&reason("a", ""), &mut c).await).0);
        let (mut c, _r) = fresh_ctx();
        out.push(expect_completed(run_validate(&validate("a", ""), &mut c).await).0);
        let (mut c, _r) = fresh_ctx();
        out.push(expect_completed(run_refine(&refine("a", ""), &mut c).await).0);
        let (mut c, _r) = fresh_ctx();
        out.push(expect_completed(run_weave(&weave("a", vec![], "", ""), &mut c).await).0);
        out
    };
    for output in &outputs {
        assert_eq!(output, "(stub)", "D10: stub output is uniform across variants");
    }
}

/// Unknown backend produces a structured `DispatchError::BackendError`
/// with the unknown name + a hint listing supported backends. D7
/// (named error catalog) contract.
#[tokio::test]
async fn unknown_backend_returns_backend_error_with_hint() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new(
        "F",
        "made_up_provider",
        "",
        CancellationFlag::new(),
        tx,
    );
    let outcome = run_step(&step("S", "hi"), &mut ctx).await;
    match outcome {
        Err(DispatchError::BackendError { name, message }) => {
            assert_eq!(name, "made_up_provider");
            assert!(
                message.contains("not in streaming registry"),
                "message: {message}"
            );
            assert!(
                message.contains("stub")
                    && message.contains("anthropic")
                    && message.contains("openai"),
                "hint should list canonical backends: {message}"
            );
        }
        other => panic!("expected BackendError, got {other:?}"),
    }
}
