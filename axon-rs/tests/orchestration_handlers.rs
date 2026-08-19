//! v1.24.0 integration tests — orchestration async handlers
//! (Let / Conditional / ForIn / Break / Continue / Return).
//!
//! Cross-cutting coverage:
//!
//! - Each handler's public surface exercised via `dispatch_node`
//!   (not just the per-handler entry point) so the routing arms in
//!   `flow_dispatcher::mod::dispatch_node` stay drift-correct.
//! - Sentinel propagation tested at 2+ levels of nesting (Break
//!   inside ForIn inside Conditional, Return inside nested ForIn).
//! - Cancel propagation across nested orchestration (cancel fired
//!   inside the body fires UpstreamCancelled on the outermost
//!   handler).
//! - Multi-step composition: Let binding consumed by Conditional's
//!   predicate + by ForIn's iterable + by Return's value_expr.
//! - branch_path discipline: orchestration handlers push/pop their
//!   segments; pure-shape handlers inside an orchestration body
//!   inherit the path automatically.
//! - D10 semantic parity: where the sync runner would produce a
//!   specific output, the dispatcher produces the same output.
//!
//! D-letter coverage validated:
//! - D1 — 12 of 45 variants graduated (cumulative with 33.y.c).
//! - D3 — cancel propagation across nested orchestration.
//! - D6 — branch_path threading per-iter / per-branch.
//! - D7 — every error case routes through DispatchError; no panic.
//! - D10 — semantic equivalence with sync runner intuitions.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::orchestration::{
    run_conditional, run_let,
};
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, DispatchError, NodeOutcome};
use axon::ir_nodes::*;
use tokio::sync::mpsc;

// ────────────────────────────────────────────────────────────────────
//  Helpers
// ────────────────────────────────────────────────────────────────────

fn fresh_ctx() -> (
    DispatchCtx,
    mpsc::UnboundedReceiver<axon::flow_execution_event::FlowExecutionEvent>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = DispatchCtx::new(
        "TestFlow",
        "stub",
        "system prompt",
        CancellationFlag::new(),
        tx,
    );
    (ctx, rx)
}

fn let_node(target: &str, value: &str, kind: &str) -> IRFlowNode {
    IRFlowNode::Let(IRLetBinding {
        node_type: "let",
        source_line: 0,
        source_column: 0,
        target: target.into(),
        value: value.into(),
        value_kind: kind.into(),
        value_ast: None,
    })
}

fn break_node() -> IRFlowNode {
    IRFlowNode::Break(IRBreakStep {
        node_type: "break",
        source_line: 0,
        source_column: 0,
    })
}

fn continue_node() -> IRFlowNode {
    IRFlowNode::Continue(IRContinueStep {
        node_type: "continue",
        source_line: 0,
        source_column: 0,
    })
}

fn return_node(value: &str) -> IRFlowNode {
    IRFlowNode::Return(IRReturnStep {
        node_type: "return",
        source_line: 0,
        source_column: 0,
        value_expr: value.into(),
    })
}

fn conditional_node(
    cond_var: &str,
    op: &str,
    rhs: &str,
    then_body: Vec<IRFlowNode>,
    else_body: Vec<IRFlowNode>,
) -> IRFlowNode {
    IRFlowNode::Conditional(IRConditional {
        node_type: "conditional",
        source_line: 0,
        source_column: 0,
        condition: cond_var.into(),
        comparison_op: op.into(),
        comparison_value: rhs.into(),
        then_body,
        else_body,
        conditions: Vec::new(),
        conjunctor: String::new(),
        cond: None,
    })
}

fn for_in_node(variable: &str, iterable: &str, body: Vec<IRFlowNode>) -> IRFlowNode {
    IRFlowNode::ForIn(IRForIn {
        node_type: "for_in",
        source_line: 0,
        source_column: 0,
        variable: variable.into(),
        iterable: iterable.into(),
        body,
    })
}

fn step_node(name: &str, ask: &str) -> IRFlowNode {
    IRFlowNode::Step(IRStep {
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
    })
}

// ────────────────────────────────────────────────────────────────────
// section 1 — Let through dispatch_node entry
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_node_routes_let_to_orchestration_handler() {
    let (mut ctx, _rx) = fresh_ctx();
    let node = let_node("region", "us-east-1", "literal");
    let outcome = dispatch_node(&node, &mut ctx).await.unwrap();
    match outcome {
        NodeOutcome::Completed { output, .. } => assert_eq!(output, "us-east-1"),
        other => panic!("expected Completed, got {other:?}"),
    }
    assert_eq!(ctx.let_bindings.get("region").unwrap(), "us-east-1");
}

#[tokio::test]
async fn dispatch_node_routes_let_reference_with_resolved_value() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("source".into(), "value-X".into());
    let node = let_node("dest", "source", "reference");
    dispatch_node(&node, &mut ctx).await.unwrap();
    assert_eq!(ctx.let_bindings.get("dest").unwrap(), "value-X");
}

// ────────────────────────────────────────────────────────────────────
// section 2 — Conditional through dispatch_node (then/else branches)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn conditional_dispatches_then_branch_and_runs_step() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("region".into(), "us".into());
    let cond = conditional_node(
        "region",
        "==",
        "us",
        vec![step_node("ThenStep", "hi")],
        Vec::new(),
    );
    let outcome = dispatch_node(&cond, &mut ctx).await.unwrap();
    match outcome {
        NodeOutcome::Completed { output, .. } => {
            // Inner Step produced "(stub)" for stub backend.
            assert_eq!(output, "(stub)");
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    // The audit row was recorded by the inner Step handler.
    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].step_name, "ThenStep");
}

#[tokio::test]
async fn conditional_dispatches_else_branch() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("region".into(), "us".into());
    let cond = conditional_node(
        "region",
        "!=",
        "us",
        vec![step_node("ThenStep", "no")],
        vec![step_node("ElseStep", "yes")],
    );
    dispatch_node(&cond, &mut ctx).await.unwrap();
    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].step_name, "ElseStep");
}

#[tokio::test]
async fn conditional_with_or_conjunctor_short_circuits_on_first_true() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("a".into(), "1".into());
    ctx.let_bindings.insert("b".into(), "0".into());

    let mut cond = IRConditional {
        node_type: "conditional",
        source_line: 0,
        source_column: 0,
        condition: "a".into(),
        comparison_op: "==".into(),
        comparison_value: "1".into(),
        then_body: vec![let_node("took", "first-or", "literal")],
        else_body: vec![let_node("took", "else", "literal")],
        conditions: vec![("b".into(), "==".into(), "1".into())],
        conjunctor: "or".into(),
        cond: None,
    };
    cond.conjunctor = "or".into();
    run_conditional(&cond, &mut ctx).await.unwrap();
    assert_eq!(ctx.let_bindings.get("took").unwrap(), "first-or");
}

#[tokio::test]
async fn conditional_with_or_conjunctor_takes_second_branch_when_first_false() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("a".into(), "0".into());
    ctx.let_bindings.insert("b".into(), "1".into());

    let cond = IRConditional {
        node_type: "conditional",
        source_line: 0,
        source_column: 0,
        condition: "a".into(),
        comparison_op: "==".into(),
        comparison_value: "1".into(),
        then_body: vec![let_node("took", "either-true", "literal")],
        else_body: vec![let_node("took", "else", "literal")],
        conditions: vec![("b".into(), "==".into(), "1".into())],
        conjunctor: "or".into(),
        cond: None,
    };
    run_conditional(&cond, &mut ctx).await.unwrap();
    assert_eq!(ctx.let_bindings.get("took").unwrap(), "either-true");
}

#[tokio::test]
async fn conditional_numeric_gt_predicate() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("score".into(), "42".into());
    let cond = conditional_node(
        "score",
        ">",
        "10",
        vec![let_node("verdict", "high", "literal")],
        vec![let_node("verdict", "low", "literal")],
    );
    dispatch_node(&cond, &mut ctx).await.unwrap();
    assert_eq!(ctx.let_bindings.get("verdict").unwrap(), "high");
}

// ────────────────────────────────────────────────────────────────────
// section 3 — ForIn through dispatch_node (iteration + sentinels)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn for_in_iterates_3_elements_running_step_each_iter() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("xs".into(), "a,b,c".into());
    let for_in = for_in_node(
        "x",
        "xs",
        vec![step_node("LoopStep", "process x")],
    );
    dispatch_node(&for_in, &mut ctx).await.unwrap();
    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit.len(), 3, "expected 3 step audit rows (one per iter)");
}

#[tokio::test]
async fn for_in_break_inside_body_terminates_at_first_iter() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("xs".into(), "a,b,c".into());
    let for_in = for_in_node(
        "x",
        "xs",
        vec![step_node("S", "hi"), break_node()],
    );
    dispatch_node(&for_in, &mut ctx).await.unwrap();
    let audit = ctx.step_audit_records.lock().await;
    // Only the first iter ran the step before break terminated.
    assert_eq!(audit.len(), 1);
}

#[tokio::test]
async fn for_in_continue_skips_remaining_body_keeps_iterating() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("xs".into(), "a,b".into());
    let for_in = for_in_node(
        "x",
        "xs",
        vec![continue_node(), step_node("AfterContinue", "x")],
    );
    dispatch_node(&for_in, &mut ctx).await.unwrap();
    let audit = ctx.step_audit_records.lock().await;
    // continue fired before reaching AfterContinue in every iter →
    // no step audit rows recorded.
    assert_eq!(audit.len(), 0);
}

#[tokio::test]
async fn for_in_return_propagates_through_loop_and_dispatch() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("xs".into(), "a,b,c".into());
    let for_in = for_in_node("x", "xs", vec![return_node("computed")]);
    let outcome = dispatch_node(&for_in, &mut ctx).await.unwrap();
    match outcome {
        NodeOutcome::Return { value } => assert_eq!(value, "computed"),
        other => panic!("expected Return propagation, got {other:?}"),
    }
}

#[tokio::test]
async fn for_in_iterable_is_literal_csv_when_not_a_binding() {
    let (mut ctx, _rx) = fresh_ctx();
    let for_in = for_in_node("x", "alpha,beta,gamma", vec![step_node("S", "hi")]);
    dispatch_node(&for_in, &mut ctx).await.unwrap();
    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit.len(), 3);
}

#[tokio::test]
async fn for_in_zero_iterations_when_iterable_empty() {
    let (mut ctx, _rx) = fresh_ctx();
    let for_in = for_in_node("x", "", vec![step_node("S", "hi")]);
    dispatch_node(&for_in, &mut ctx).await.unwrap();
    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit.len(), 0);
}

// ────────────────────────────────────────────────────────────────────
// section 4 — Break/Continue/Return sentinels through dispatch_node
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_node_break_returns_break_sentinel() {
    let (mut ctx, _rx) = fresh_ctx();
    let outcome = dispatch_node(&break_node(), &mut ctx).await.unwrap();
    assert!(matches!(outcome, NodeOutcome::Break));
}

#[tokio::test]
async fn dispatch_node_continue_returns_loop_continue_sentinel() {
    let (mut ctx, _rx) = fresh_ctx();
    let outcome = dispatch_node(&continue_node(), &mut ctx).await.unwrap();
    assert!(matches!(outcome, NodeOutcome::LoopContinue));
}

#[tokio::test]
async fn dispatch_node_return_with_literal_value() {
    let (mut ctx, _rx) = fresh_ctx();
    let outcome = dispatch_node(&return_node("final"), &mut ctx).await.unwrap();
    match outcome {
        NodeOutcome::Return { value } => assert_eq!(value, "final"),
        other => panic!("expected Return, got {other:?}"),
    }
}

#[tokio::test]
async fn dispatch_node_return_with_let_resolved_value() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("answer".into(), "42".into());
    let outcome = dispatch_node(&return_node("answer"), &mut ctx).await.unwrap();
    match outcome {
        NodeOutcome::Return { value } => assert_eq!(value, "42"),
        other => panic!("expected Return, got {other:?}"),
    }
}

// ────────────────────────────────────────────────────────────────────
// section 5 — Nested orchestration composition
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn break_inside_for_in_inside_conditional() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("active".into(), "yes".into());

    // if active == "yes" { for x in [a,b,c] { S; break } }
    let outer = conditional_node(
        "active",
        "==",
        "yes",
        vec![for_in_node(
            "x",
            "a,b,c",
            vec![step_node("Inner", "hi"), break_node()],
        )],
        Vec::new(),
    );
    dispatch_node(&outer, &mut ctx).await.unwrap();
    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit.len(), 1, "break terminated the loop after 1 iter");
}

#[tokio::test]
async fn return_inside_nested_for_in_propagates_to_outer_dispatch() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("xs".into(), "a,b".into());
    ctx.let_bindings.insert("ys".into(), "1,2".into());

    // for x in [a,b] { for y in [1,2] { return "early" } }
    let outer = for_in_node(
        "x",
        "xs",
        vec![for_in_node(
            "y",
            "ys",
            vec![return_node("early")],
        )],
    );
    let outcome = dispatch_node(&outer, &mut ctx).await.unwrap();
    match outcome {
        NodeOutcome::Return { value } => assert_eq!(value, "early"),
        other => panic!("expected Return propagated through 2 ForIn levels, got {other:?}"),
    }
}

#[tokio::test]
async fn let_chain_then_conditional_then_for_in() {
    let (mut ctx, _rx) = fresh_ctx();

    // let region = "us"
    run_let(
        &IRLetBinding {
            node_type: "let",
            source_line: 0,
            source_column: 0,
            target: "region".into(),
            value: "us".into(),
            value_kind: "literal".into(),
            value_ast: None,
        },
        &mut ctx,
    )
    .await
    .unwrap();

    // let xs = "a,b,c"
    run_let(
        &IRLetBinding {
            node_type: "let",
            source_line: 0,
            source_column: 0,
            target: "xs".into(),
            value: "a,b,c".into(),
            value_kind: "literal".into(),
            value_ast: None,
        },
        &mut ctx,
    )
    .await
    .unwrap();

    // if region == "us" { for x in xs { S } }
    let outer = conditional_node(
        "region",
        "==",
        "us",
        vec![for_in_node(
            "x",
            "xs",
            vec![step_node("ChainedStep", "hi")],
        )],
        Vec::new(),
    );
    dispatch_node(&outer, &mut ctx).await.unwrap();
    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit.len(), 3);
}

// ────────────────────────────────────────────────────────────────────
// section 6 — Cancel propagation through nested orchestration
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cancel_propagates_into_for_in_body_dispatch() {
    let cancel = CancellationFlag::new();
    cancel.cancel();
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);
    ctx.let_bindings.insert("xs".into(), "a,b,c".into());

    let for_in = for_in_node("x", "xs", vec![step_node("S", "hi")]);
    let outcome = dispatch_node(&for_in, &mut ctx).await;
    assert!(matches!(outcome, Err(DispatchError::UpstreamCancelled)));
}

#[tokio::test]
async fn cancel_propagates_into_conditional_body() {
    let cancel = CancellationFlag::new();
    cancel.cancel();
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);
    ctx.let_bindings.insert("flag".into(), "yes".into());

    let cond = conditional_node(
        "flag",
        "==",
        "yes",
        vec![step_node("S", "hi")],
        Vec::new(),
    );
    let outcome = dispatch_node(&cond, &mut ctx).await;
    assert!(matches!(outcome, Err(DispatchError::UpstreamCancelled)));
}

// ────────────────────────────────────────────────────────────────────
// section 7 — Branch path threading
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn conditional_pushes_and_pops_branch_path() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("flag".into(), "yes".into());
    let cond = conditional_node(
        "flag",
        "==",
        "yes",
        vec![let_node("inner", "true-branch", "literal")],
        Vec::new(),
    );
    dispatch_node(&cond, &mut ctx).await.unwrap();
    // After successful dispatch the branch_path is empty again.
    assert!(ctx.branch_path.is_empty());
}

#[tokio::test]
async fn for_in_pushes_and_pops_per_iter_branch_path() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("xs".into(), "a,b,c".into());
    let for_in = for_in_node("x", "xs", vec![let_node("inner", "v", "literal")]);
    dispatch_node(&for_in, &mut ctx).await.unwrap();
    assert!(ctx.branch_path.is_empty());
}

// ────────────────────────────────────────────────────────────────────
// section 8 — Step counter monotonicity across orchestration
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn step_counter_advances_only_for_pure_shape_inside_orchestration() {
    let (mut ctx, _rx) = fresh_ctx();
    assert_eq!(ctx.step_counter, 0);

    // Let does NOT advance.
    dispatch_node(&let_node("k", "v", "literal"), &mut ctx)
        .await
        .unwrap();
    assert_eq!(ctx.step_counter, 0);

    // Conditional (taken branch dispatches a Step) DOES advance for
    // the inner Step.
    ctx.let_bindings.insert("flag".into(), "yes".into());
    let cond = conditional_node(
        "flag",
        "==",
        "yes",
        vec![step_node("Inner", "hi")],
        Vec::new(),
    );
    dispatch_node(&cond, &mut ctx).await.unwrap();
    assert_eq!(
        ctx.step_counter, 1,
        "Conditional itself does not advance counter, but the inner \
         Step did → counter = 1"
    );

    // ForIn with 3 iters of a Step → counter advances 3 more.
    ctx.let_bindings.insert("xs".into(), "a,b,c".into());
    let for_in = for_in_node("x", "xs", vec![step_node("Iter", "hi")]);
    dispatch_node(&for_in, &mut ctx).await.unwrap();
    assert_eq!(ctx.step_counter, 4);
}
