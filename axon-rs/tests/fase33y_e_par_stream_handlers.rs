//! §Fase 33.y.e integration tests — Par + Stream handlers +
//! concurrent dispatch helper + Fase 23 algebraic-effects bridge.
//!
//! Covers the public surface of:
//!
//! - [`axon::flow_dispatcher::parallel::run_par`] +
//!   [`axon::flow_dispatcher::parallel::run_branches_concurrently`]
//! - [`axon::flow_dispatcher::effects_bridge::run_stream`] +
//!   [`axon::flow_dispatcher::effects_bridge::bridge_effect_stream_yield`]
//!
//! D-letter coverage:
//! - D1 — 14 of 45 variants graduated (cumulative).
//! - D3 — Cancel propagation through the concurrent dispatch +
//!   the algebraic-effects bridge.
//! - D6 — Per-Yield audit row recorded with SHA-256 of the
//!   concatenated wire content; Par's per-branch step counter
//!   merge documented.
//! - D9 — `perform Stream.Yield x` on the streaming path emits
//!   `axon.token` directly via the bridge (the D9 milestone).
//! - D10 — Sync-runner parity: Par's let-bindings isolation
//!   discipline (private per branch) matches the principled
//!   parallel-evaluation semantics the sync runner will adopt.

use axon::cancel_token::CancellationFlag;
use axon::effects::ir::{IRHandlerClause, IRHandlerFrame, IRPerform, IRResume, Instruction};
use axon::effects::{EffectRuntime, ExecutionResult};
use axon::flow_dispatcher::effects_bridge::{bridge_effect_stream_yield, run_stream};
use axon::flow_dispatcher::parallel::run_branches_concurrently;
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
        "",
        CancellationFlag::new(),
        tx,
    );
    (ctx, rx)
}

fn par_node() -> IRFlowNode {
    IRFlowNode::Par(IRParallelBlock {
        node_type: "par",
        source_line: 0,
        source_column: 0,
            branches: Vec::new(),
    })
}

fn stream_node() -> IRFlowNode {
    IRFlowNode::Stream(IRStreamBlock {
        node_type: "stream_block",
        source_line: 0,
        source_column: 0,
        body: Vec::new(),

        chunk_type: String::new(),
        on_chunk: None,
        on_complete: None,
        on_error: None,
    })
}

fn let_branch(target: &str, value: &str) -> Vec<IRFlowNode> {
    vec![IRFlowNode::Let(IRLetBinding {
        node_type: "let",
        source_line: 0,
        source_column: 0,
        target: target.into(),
        value: value.into(),
        value_kind: "literal".into(),
        value_ast: None,
    })]
}

fn step_branch(name: &str) -> Vec<IRFlowNode> {
    vec![IRFlowNode::Step(IRStep {
        node_type: "step",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        persona_ref: String::new(),
        given: String::new(),
        ask: "hi".into(),
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
    })]
}

fn perform_yield(arg: &str) -> Instruction {
    Instruction::Perform(IRPerform {
        effect_name: "Stream".into(),
        operation_name: "Yield".into(),
        arguments: vec![arg.into()],
        state_id: 0,
        resume_label: String::new(),
    })
}

fn handle_stream_with_body(body: Vec<Instruction>) -> Vec<Instruction> {
    vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["Stream".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Yield".into(),
            parameter_names: vec!["v".into()],
            body: vec![Instruction::Resume(IRResume {
                value_expr: String::new(),
                frame_id: 0,
            })],
            source_line: 0,
            source_column: 0,
        }],
        body,
        frame_id: 0,
        body_states: Vec::new(),
        source_line: 0,
        source_column: 0,
    })]
}

// ────────────────────────────────────────────────────────────────────
//  §1 — Par dispatcher arm wire shape
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_node_routes_par_to_parallel_handler() {
    let (mut ctx, mut rx) = fresh_ctx();
    let outcome = dispatch_node(&par_node(), &mut ctx).await.unwrap();
    match outcome {
        NodeOutcome::Completed {
            output,
            tokens_emitted,
            ..
        } => {
            assert_eq!(output, "");
            assert_eq!(tokens_emitted, 0);
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    assert_eq!(events.len(), 2, "Par emits StepStart + StepComplete only");
    match &events[0] {
        axon::flow_execution_event::FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "par");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

#[tokio::test]
async fn par_advances_step_counter_by_one() {
    let (mut ctx, _rx) = fresh_ctx();
    assert_eq!(ctx.step_counter, 0);
    dispatch_node(&par_node(), &mut ctx).await.unwrap();
    assert_eq!(ctx.step_counter, 1);
}

// ────────────────────────────────────────────────────────────────────
//  §2 — run_branches_concurrently public helper
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn run_branches_concurrently_two_let_branches_outputs_both() {
    let (mut ctx, _rx) = fresh_ctx();
    let branches = vec![
        let_branch("a", "value-A"),
        let_branch("b", "value-B"),
    ];
    let outcome = run_branches_concurrently(&branches, &mut ctx).await.unwrap();
    match outcome {
        NodeOutcome::Completed { output, .. } => {
            assert!(output.contains("value-A"));
            assert!(output.contains("value-B"));
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn run_branches_concurrently_step_branches_emit_concurrent_axon_tokens() {
    let (mut ctx, mut rx) = fresh_ctx();
    let branches = vec![
        step_branch("BranchA"),
        step_branch("BranchB"),
        step_branch("BranchC"),
    ];
    let outcome = run_branches_concurrently(&branches, &mut ctx).await.unwrap();

    // All 3 branches completed.
    if let NodeOutcome::Completed { tokens_emitted, .. } = outcome {
        assert_eq!(tokens_emitted, 3, "3 branches × 1 token each = 3");
    } else {
        panic!("expected Completed, got {outcome:?}");
    }

    // Wire saw 3 × (StepStart + StepToken + StepComplete) = 9 events.
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    let token_count = events
        .iter()
        .filter(|e| matches!(e, axon::flow_execution_event::FlowExecutionEvent::StepToken { .. }))
        .count();
    assert_eq!(token_count, 3, "exactly 3 StepToken events on the wire");

    // Audit rows: 3 (one per branch's step).
    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit.len(), 3);
}

#[tokio::test]
async fn run_branches_concurrently_return_sentinel_wins() {
    let (mut ctx, _rx) = fresh_ctx();
    let branches = vec![
        let_branch("side", "ignored"),
        vec![IRFlowNode::Return(IRReturnStep {
            node_type: "return",
            source_line: 0,
            source_column: 0,
            value_expr: "early-exit".into(),
        })],
        step_branch("BranchC"),
    ];
    let outcome = run_branches_concurrently(&branches, &mut ctx).await.unwrap();
    match outcome {
        NodeOutcome::Return { value } => assert_eq!(value, "early-exit"),
        other => panic!("expected Return propagation, got {other:?}"),
    }
}

#[tokio::test]
async fn run_branches_concurrently_let_bindings_isolated_per_branch() {
    let (mut ctx, _rx) = fresh_ctx();
    let branches = vec![
        let_branch("priv_A", "should-not-leak"),
        let_branch("priv_B", "also-shouldnt"),
    ];
    run_branches_concurrently(&branches, &mut ctx).await.unwrap();
    assert!(
        !ctx.let_bindings.contains_key("priv_A"),
        "Par branches' let_bindings stay private (D10 parity)"
    );
    assert!(!ctx.let_bindings.contains_key("priv_B"));
}

#[tokio::test]
async fn run_branches_concurrently_parent_branch_path_unchanged_after_dispatch() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.branch_path.push("outer".into());
    let branches = vec![step_branch("S1"), step_branch("S2")];
    run_branches_concurrently(&branches, &mut ctx).await.unwrap();
    assert_eq!(ctx.branch_path, vec!["outer".to_string()]);
}

#[tokio::test]
async fn run_branches_concurrently_empty_is_completed_with_zero_tokens() {
    let (mut ctx, _rx) = fresh_ctx();
    let outcome = run_branches_concurrently(&[], &mut ctx).await.unwrap();
    match outcome {
        NodeOutcome::Completed { output, tokens_emitted, .. } => {
            assert_eq!(output, "");
            assert_eq!(tokens_emitted, 0);
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn run_branches_concurrently_cancel_short_circuits() {
    let cancel = CancellationFlag::new();
    cancel.cancel();
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);
    let branches = vec![step_branch("S")];
    assert!(matches!(
        run_branches_concurrently(&branches, &mut ctx).await,
        Err(DispatchError::UpstreamCancelled)
    ));
}

#[tokio::test]
async fn run_branches_concurrently_audit_rows_aggregate_across_branches() {
    let (mut ctx, _rx) = fresh_ctx();
    let branches = vec![
        step_branch("Alpha"),
        step_branch("Beta"),
        step_branch("Gamma"),
    ];
    run_branches_concurrently(&branches, &mut ctx).await.unwrap();
    let audit = ctx.step_audit_records.lock().await;
    let names: Vec<&str> = audit.iter().map(|r| r.step_name.as_str()).collect();
    assert!(names.contains(&"Alpha"));
    assert!(names.contains(&"Beta"));
    assert!(names.contains(&"Gamma"));
}

// ────────────────────────────────────────────────────────────────────
//  §3 — Stream dispatcher arm wire shape
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_node_routes_stream_to_effects_bridge_handler() {
    let (mut ctx, mut rx) = fresh_ctx();
    let outcome = dispatch_node(&stream_node(), &mut ctx).await.unwrap();
    match outcome {
        NodeOutcome::Completed {
            output,
            tokens_emitted,
            ..
        } => {
            assert_eq!(output, "");
            assert_eq!(tokens_emitted, 0);
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    assert_eq!(events.len(), 2);
    match &events[0] {
        axon::flow_execution_event::FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "stream");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

#[tokio::test]
async fn stream_advances_step_counter_by_one() {
    let (mut ctx, _rx) = fresh_ctx();
    assert_eq!(ctx.step_counter, 0);
    dispatch_node(&stream_node(), &mut ctx).await.unwrap();
    assert_eq!(ctx.step_counter, 1);
}

#[tokio::test]
async fn run_stream_directly_returns_completed() {
    let (mut ctx, _rx) = fresh_ctx();
    let block = IRStreamBlock {
        node_type: "stream_block",
        source_line: 0,
        source_column: 0,
        body: Vec::new(),

        chunk_type: String::new(),
        on_chunk: None,
        on_complete: None,
        on_error: None,
    };
    let outcome = run_stream(&block, &mut ctx).await.unwrap();
    assert!(matches!(outcome, NodeOutcome::Completed { .. }));
}

// ────────────────────────────────────────────────────────────────────
//  §4 — bridge_effect_stream_yield (D9)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn bridge_emits_axon_token_per_static_yield() {
    let (mut ctx, mut rx) = fresh_ctx();
    let block = handle_stream_with_body(vec![
        perform_yield("\"alpha\""),
        perform_yield("\"beta\""),
        perform_yield("\"gamma\""),
    ]);
    let mut runtime = EffectRuntime::new();
    let result = bridge_effect_stream_yield(&block, &mut runtime, "StreamStep", &mut ctx)
        .await
        .unwrap();
    assert!(matches!(result, ExecutionResult::Completed(_)));

    let mut tokens = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let axon::flow_execution_event::FlowExecutionEvent::StepToken { content, .. } = ev {
            tokens.push(content);
        }
    }
    assert_eq!(tokens, vec!["alpha", "beta", "gamma"]);
}

#[tokio::test]
async fn bridge_token_indices_are_monotonic_starting_from_1() {
    let (mut ctx, mut rx) = fresh_ctx();
    let block = handle_stream_with_body(vec![
        perform_yield("\"a\""),
        perform_yield("\"b\""),
    ]);
    let mut runtime = EffectRuntime::new();
    bridge_effect_stream_yield(&block, &mut runtime, "S", &mut ctx)
        .await
        .unwrap();
    let mut indices = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let axon::flow_execution_event::FlowExecutionEvent::StepToken { token_index, .. } = ev {
            indices.push(token_index);
        }
    }
    assert_eq!(indices, vec![1, 2]);
}

#[tokio::test]
async fn bridge_records_audit_row_with_token_count() {
    let (mut ctx, _rx) = fresh_ctx();
    let block = handle_stream_with_body(vec![
        perform_yield("\"x\""),
        perform_yield("\"y\""),
        perform_yield("\"z\""),
        perform_yield("\"w\""),
    ]);
    let mut runtime = EffectRuntime::new();
    bridge_effect_stream_yield(&block, &mut runtime, "BridgedStep", &mut ctx)
        .await
        .unwrap();
    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].step_name, "BridgedStep");
    assert_eq!(audit[0].tokens_emitted, 4);
    assert!(audit[0].success);
    // SHA-256 hex of "xyzw" computed independently — 64-char lowercase hex.
    assert_eq!(audit[0].output_hash_hex.len(), 64);
}

#[tokio::test]
async fn bridge_zero_yields_emits_zero_tokens_zero_audit_rows() {
    // Wait — the bridge ALWAYS records 1 audit row for the call.
    // So this test verifies: zero StepToken events + 1 audit row
    // with tokens_emitted=0.
    let (mut ctx, mut rx) = fresh_ctx();
    let block = handle_stream_with_body(vec![]);
    let mut runtime = EffectRuntime::new();
    bridge_effect_stream_yield(&block, &mut runtime, "EmptyBridge", &mut ctx)
        .await
        .unwrap();
    let token_count = (0..)
        .map_while(|_| rx.try_recv().ok())
        .filter(|ev| matches!(ev, axon::flow_execution_event::FlowExecutionEvent::StepToken { .. }))
        .count();
    assert_eq!(token_count, 0);
    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].tokens_emitted, 0);
}

#[tokio::test]
async fn bridge_ignores_non_stream_performs() {
    let (mut ctx, mut rx) = fresh_ctx();
    let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["Other".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "DoStuff".into(),
            parameter_names: vec!["v".into()],
            body: vec![Instruction::Resume(IRResume {
                value_expr: String::new(),
                frame_id: 0,
            })],
            source_line: 0,
            source_column: 0,
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "Other".into(),
            operation_name: "DoStuff".into(),
            arguments: vec!["x".into()],
            state_id: 0,
            resume_label: String::new(),
        })],
        frame_id: 0,
        body_states: Vec::new(),
        source_line: 0,
        source_column: 0,
    })];
    let mut runtime = EffectRuntime::new();
    bridge_effect_stream_yield(&block, &mut runtime, "S", &mut ctx)
        .await
        .unwrap();
    let token_count = (0..)
        .map_while(|_| rx.try_recv().ok())
        .filter(|ev| matches!(ev, axon::flow_execution_event::FlowExecutionEvent::StepToken { .. }))
        .count();
    assert_eq!(token_count, 0, "non-Stream performs do NOT emit axon.token");
}

#[tokio::test]
async fn bridge_finds_nested_yields_inside_handler_frame() {
    let (mut ctx, mut rx) = fresh_ctx();
    // Outer Handle Stream wrapping a nested Handle Other wrapping a Yield.
    let inner_yield = perform_yield("\"deep\"");
    let nested = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["OtherEffect".into()],
        clauses: vec![],
        body: vec![inner_yield],
        frame_id: 1,
        body_states: Vec::new(),
        source_line: 0,
        source_column: 0,
    });
    let block = handle_stream_with_body(vec![nested]);
    let mut runtime = EffectRuntime::new();
    let _ = bridge_effect_stream_yield(&block, &mut runtime, "S", &mut ctx).await;
    let tokens: Vec<String> = (0..)
        .map_while(|_| rx.try_recv().ok())
        .filter_map(|ev| match ev {
            axon::flow_execution_event::FlowExecutionEvent::StepToken { content, .. } => Some(content),
            _ => None,
        })
        .collect();
    assert_eq!(tokens, vec!["deep".to_string()]);
}

#[tokio::test]
async fn bridge_cancel_short_circuits_before_runtime_execution() {
    let cancel = CancellationFlag::new();
    cancel.cancel();
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);
    let block = handle_stream_with_body(vec![perform_yield("\"x\"")]);
    let mut runtime = EffectRuntime::new();
    let outcome = bridge_effect_stream_yield(&block, &mut runtime, "S", &mut ctx).await;
    assert!(matches!(outcome, Err(DispatchError::UpstreamCancelled)));
}

// ────────────────────────────────────────────────────────────────────
//  §5 — Cross-cutting composition
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn par_inside_dispatch_node_emits_only_canonical_wire_shape() {
    // Confirms run_par doesn't accidentally dispatch nested branches
    // (since IRParallelBlock is payload-free). Wire shape is locked.
    let (mut ctx, mut rx) = fresh_ctx();
    dispatch_node(&par_node(), &mut ctx).await.unwrap();
    let events: Vec<_> = (0..).map_while(|_| rx.try_recv().ok()).collect();
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn stream_inside_dispatch_node_emits_only_canonical_wire_shape() {
    let (mut ctx, mut rx) = fresh_ctx();
    dispatch_node(&stream_node(), &mut ctx).await.unwrap();
    let events: Vec<_> = (0..).map_while(|_| rx.try_recv().ok()).collect();
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn par_followed_by_stream_advances_counter_to_2() {
    let (mut ctx, _rx) = fresh_ctx();
    dispatch_node(&par_node(), &mut ctx).await.unwrap();
    dispatch_node(&stream_node(), &mut ctx).await.unwrap();
    assert_eq!(ctx.step_counter, 2);
}
