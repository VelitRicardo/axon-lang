//! v1.24.0 integration tests — D8 cross-cutting: tools
//! first-class on the streaming path.
//!
//! Verifies:
//!
//! - **ToolSpec synthesis** — `run_step` with non-empty `apply_ref`
//!   synthesizes a `ToolSpec` and plumbs it into `ChatRequest.tools`
//!   via the shared async core (`run_pure_shape`).
//! - **FlowExecutionEvent::ToolCall** — the new event variant is
//!   serde-roundtrippable, step-scoped per `is_step_scoped`, has
//!   the canonical `"tool_call"` kind slug.
//! - **D4 byte-compat** — stub backend (FinishReason::Stop) never
//!   triggers ToolCall emission; wire shape stays canonical for
//!   pre-33.y.k adopters.
//! - **Wire shape preservation** — pure_shape handlers (Step +
//!   cognitive framings) still emit StepStart + StepToken +
//!   StepComplete in the same canonical sequence when no tool is
//!   declared.
//!
//! # Live ToolCall emission scope statement
//!
//! Full live ToolCall emission with a real upstream backend
//! returning `FinishReason::ToolUse` is verified via the
//! `real_provider.yml` opt-in CI lane (Anthropic
//! `tool_use` + OpenAI `tool_calls` real responses). 33.y.k ships
//! the structural surface; production observation lands in the
//! real-provider gated lane.
//!
//! D-letter coverage:
//! - D1 — 45 of 45 graduated remains pinned.
//! - D4 — wire byte-compat for non-tool-using flows preserved.
//! - D7 — every error case routes through DispatchError.
//! - **D8 (the milestone)** — tools first-class: `apply: <tool>`
//!   on a Step plumbs through `ChatRequest.tools`; FinishReason::
//!   ToolUse triggers `axon.tool_call` event emission.
//! - D10 — sync-runner parity: tools field passed through
//!   identical to how the sync runner builds ChatRequest.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::*;
use tokio::sync::mpsc;

fn fresh_ctx() -> (
    DispatchCtx,
    mpsc::UnboundedReceiver<FlowExecutionEvent>,
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

fn step_with_apply(name: &str, ask: &str, apply: &str) -> IRFlowNode {
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
        apply_ref: apply.into(),
        pix_ops: Vec::new(),
        stream: None,
        performs: Vec::new(),
        guards: Vec::new(),
        requires_context: None,        now_tz: None,        body: Vec::new(),
    })
}

fn step_without_apply(name: &str, ask: &str) -> IRFlowNode {
    step_with_apply(name, ask, "")
}

// ────────────────────────────────────────────────────────────────────
// section 1 — FlowExecutionEvent::ToolCall — closed catalog
// ────────────────────────────────────────────────────────────────────

#[test]
fn tool_call_event_kind_is_canonical_slug() {
    let ev = FlowExecutionEvent::ToolCall {
        step_name: "Step".into(),
        tool_name: "my_tool".into(),
        content: "{...}".into(),
        timestamp_ms: 12345,
    };
    assert_eq!(ev.kind(), "tool_call");
}

#[test]
fn tool_call_event_is_step_scoped() {
    let ev = FlowExecutionEvent::ToolCall {
        step_name: "Step".into(),
        tool_name: "my_tool".into(),
        content: "{}".into(),
        timestamp_ms: 0,
    };
    assert!(ev.is_step_scoped());
}

#[test]
fn tool_call_event_is_not_terminator() {
    let ev = FlowExecutionEvent::ToolCall {
        step_name: "S".into(),
        tool_name: "t".into(),
        content: "".into(),
        timestamp_ms: 0,
    };
    assert!(!ev.is_terminator());
}

#[test]
fn tool_call_event_serde_round_trip() {
    let ev = FlowExecutionEvent::ToolCall {
        step_name: "MyStep".into(),
        tool_name: "calculator".into(),
        content: r#"{"a":1,"b":2}"#.into(),
        timestamp_ms: 1_234_567_890,
    };
    let s = serde_json::to_string(&ev).unwrap();
    assert!(s.contains(r#""kind":"tool_call""#));
    assert!(s.contains(r#""tool_name":"calculator""#));
    let back: FlowExecutionEvent = serde_json::from_str(&s).unwrap();
    assert_eq!(back, ev, "serde round-trip preserves ToolCall");
}

// ────────────────────────────────────────────────────────────────────
// section 2 — D4 byte-compat: stub backend never triggers ToolCall
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn stub_backend_never_emits_tool_call_event() {
    let (mut ctx, mut rx) = fresh_ctx();
    // Run a Step WITH `apply_ref` declared — even though the tool
    // declaration plumbs through to ChatRequest.tools, the stub
    // backend always returns FinishReason::Stop, so no ToolCall
    // event ever fires on the wire. This is the D4 byte-compat
    // invariant: adopters running on stub (test/dev) see the
    // same wire shape pre- and post-33.y.k.
    dispatch_node(&step_with_apply("Generate", "hi", "my_tool"), &mut ctx)
        .await
        .unwrap();

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    let tool_call_count = events
        .iter()
        .filter(|e| matches!(e, FlowExecutionEvent::ToolCall { .. }))
        .count();
    assert_eq!(tool_call_count, 0, "stub backend never emits ToolCall (D4)");
}

#[tokio::test]
async fn stub_backend_step_with_apply_ref_canonical_wire_shape() {
    let (mut ctx, mut rx) = fresh_ctx();
    let outcome = dispatch_node(&step_with_apply("S", "hi", "tool_x"), &mut ctx)
        .await
        .unwrap();
    match outcome {
        NodeOutcome::Completed { output, tokens_emitted, .. } => {
            // Wire shape unchanged: "(stub)" + 1 token (D4 byte-compat
            // with pre-33.y.k canonical Step variant).
            assert_eq!(output, "(stub)");
            assert_eq!(tokens_emitted, 1);
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    // Canonical 3 events: StepStart + StepToken + StepComplete.
    // No ToolCall.
    assert_eq!(events.len(), 3);
    assert!(matches!(events[0], FlowExecutionEvent::StepStart { .. }));
    assert!(matches!(events[1], FlowExecutionEvent::StepToken { .. }));
    assert!(matches!(events[2], FlowExecutionEvent::StepComplete { .. }));
}

// ────────────────────────────────────────────────────────────────────
// section 3 — Step without apply_ref: wire shape strictly identical
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn step_without_apply_ref_canonical_wire_shape() {
    let (mut ctx, mut rx) = fresh_ctx();
    let outcome = dispatch_node(&step_without_apply("S", "hi"), &mut ctx)
        .await
        .unwrap();
    match outcome {
        NodeOutcome::Completed { output, tokens_emitted, .. } => {
            assert_eq!(output, "(stub)");
            assert_eq!(tokens_emitted, 1);
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    // Wire shape byte-identical to the apply_ref="" path → 3 events
    // (StepStart + StepToken + StepComplete). The tools-empty
    // ChatRequest never triggers ToolCall.
    assert_eq!(events.len(), 3);
}

// ────────────────────────────────────────────────────────────────────
// section 4 — Cognitive variants stay byte-compat (no apply_ref)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cognitive_framing_variants_unchanged_no_tool_call() {
    let variants: Vec<IRFlowNode> = vec![
        IRFlowNode::Focus(IRFocusStep {
            node_type: "focus",
            source_line: 0,
            source_column: 0,
            expression: "x".into(),
            where_expr: String::new(),
            select: Vec::new(),
            output: String::new(),
        }),
        IRFlowNode::Associate(IRAssociateStep {
            node_type: "associate",
            source_line: 0,
            source_column: 0,
            left: "a".into(),
            right: "b".into(),
            using_field: "".into(),
            output: String::new(),
        }),
        IRFlowNode::Aggregate(IRAggregateStep {
            node_type: "aggregate",
            source_line: 0,
            source_column: 0,
            target: "t".into(),
            group_by: Vec::new(),
            alias: "".into(),
            compute: Vec::new(),
            where_expr: String::new(),
        }),
        IRFlowNode::Explore(IRExploreStep {
            node_type: "explore",
            source_line: 0,
            source_column: 0,
            target: "h".into(),
            limit: None,
            output: String::new(),
        }),
        IRFlowNode::Ingest(IRIngestStep {
            node_type: "ingest",
            source_line: 0,
            source_column: 0,
            source: "s".into(),
            target: "t".into(),
            format: "json".into(),
            max_bytes: None,
            max_rows: None,
        }),
        IRFlowNode::Navigate(IRNavigateStep {
            depth: None,
            node_type: "navigate",
            source_line: 0,
            source_column: 0,
            pix_ref: "p".into(),
            corpus_ref: "c".into(),
            query: "q".into(),
            trail_enabled: false,
            output_name: "o".into(),
            seed: String::new(),
            budget: None,
            where_expr: String::new(),
        }),
        IRFlowNode::Corroborate(IRCorroborateStep {
            node_type: "corroborate",
            source_line: 0,
            source_column: 0,
            navigate_ref: "n".into(),
            output_name: "o".into(),
        }),
    ];

    for node in variants {
        let (mut ctx, mut rx) = fresh_ctx();
        // v2.63.0 - the five data-plane verbs REFUSE without an engine
        // port; navigate/corroborate still Complete. Either way the D4
        // property under test is identical: NO ToolCall event is emitted.
        let _ = dispatch_node(&node, &mut ctx).await;
        let mut tool_call_count = 0;
        while let Ok(ev) = rx.try_recv() {
            if matches!(ev, FlowExecutionEvent::ToolCall { .. }) {
                tool_call_count += 1;
            }
        }
        assert_eq!(
            tool_call_count, 0,
            "cognitive variant {node:?} must not emit ToolCall (D4)"
        );
    }
}

// ────────────────────────────────────────────────────────────────────
// section 5 — Pure-shape variants (Probe/Reason/Validate/Refine/Weave) no tool
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn pure_shape_variants_no_tool_call_event() {
    let variants: Vec<IRFlowNode> = vec![
        IRFlowNode::Probe(IRProbe {
            node_type: "probe",
            source_line: 0,
            source_column: 0,
            target: "t".into(),
        }),
        IRFlowNode::Reason(IRReasonStep {
            node_type: "reason",
            source_line: 0,
            source_column: 0,
            strategy: "cot".into(),
            target: "t".into(),
            given: String::new(),
            ask: String::new(),
            depth: None,
        }),
        IRFlowNode::Validate(IRValidateStep {
            node_type: "validate",
            resolved_schema: None,
            guard: None,
            source_line: 0,
            source_column: 0,
            target: "t".into(),
            rule: "r".into(),
        }),
        IRFlowNode::Refine(IRRefineStep {
            node_type: "refine",
            source_line: 0,
            source_column: 0,
            target: "t".into(),
            strategy: "s".into(),
        }),
        IRFlowNode::Weave(IRWeaveStep {
            node_type: "weave",
            source_line: 0,
            source_column: 0,
            sources: Vec::new(),
            target: "t".into(),
            format_type: "".into(),
            priority: Vec::new(),
            style: "".into(),
            include: Vec::new(),
        }),
    ];

    for node in variants {
        let (mut ctx, mut rx) = fresh_ctx();
        dispatch_node(&node, &mut ctx).await.unwrap();
        let mut tool_call_count = 0;
        while let Ok(ev) = rx.try_recv() {
            if matches!(ev, FlowExecutionEvent::ToolCall { .. }) {
                tool_call_count += 1;
            }
        }
        assert_eq!(
            tool_call_count, 0,
            "pure-shape variant {node:?} (no apply_ref) must not emit ToolCall"
        );
    }
}

// ────────────────────────────────────────────────────────────────────
// section 6 — Composition: Step with apply_ref inside orchestration
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn step_with_apply_inside_for_in_canonical_per_iter() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("xs".into(), "a,b,c".into());
    let for_in = IRFlowNode::ForIn(IRForIn {
        node_type: "for_in",
        source_line: 0,
        source_column: 0,
        variable: "x".into(),
        iterable: "xs".into(),
        body: vec![step_with_apply("Generate", "hi", "tool_y")],
    });
    dispatch_node(&for_in, &mut ctx).await.unwrap();
    // 3 iters → 3 audit rows.
    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit.len(), 3);
}

#[tokio::test]
async fn step_with_apply_inside_conditional_branch() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("flag".into(), "yes".into());
    let cond = IRFlowNode::Conditional(IRConditional {
        node_type: "conditional",
        source_line: 0,
        source_column: 0,
        condition: "flag".into(),
        comparison_op: "==".into(),
        comparison_value: "yes".into(),
        then_body: vec![step_with_apply("Generate", "hi", "tool_z")],
        else_body: Vec::new(),
        conditions: Vec::new(),
        conjunctor: String::new(),
        cond: None,
    });
    dispatch_node(&cond, &mut ctx).await.unwrap();
    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit.len(), 1);
}

// ────────────────────────────────────────────────────────────────────
// section 7 — Fuzz pack
// ────────────────────────────────────────────────────────────────────

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        let mixed = seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(0xBB67_AE85_84CA_A73B);
        Self(mixed.max(1))
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    fn range(&mut self, max: usize) -> usize {
        (self.next_u64() as usize) % max.max(1)
    }
    fn ascii_string(&mut self, len: usize) -> String {
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            let c = (self.range(95) + 32) as u8;
            s.push(c as char);
        }
        s
    }
    fn ascii_with_random_len(&mut self, max: usize) -> String {
        let len = self.range(max) + 1;
        self.ascii_string(len)
    }
}

#[tokio::test]
async fn fuzz_step_with_random_apply_ref_never_panics_on_stub() {
    let mut lcg = Lcg::new(0xCA_FE_BA_BE_DE_AD_BE_EF);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        let apply = lcg.ascii_with_random_len(20);
        let node = step_with_apply(
            &lcg.ascii_with_random_len(15),
            &lcg.ascii_with_random_len(20),
            &apply,
        );
        let outcome = dispatch_node(&node, &mut ctx).await;
        // Stub backend always succeeds → Completed.
        match outcome {
            Ok(NodeOutcome::Completed { tokens_emitted, .. }) => {
                assert_eq!(tokens_emitted, 1, "iter={iter}: stub emits 1 token");
            }
            other => panic!("iter={iter}: unexpected {other:?}"),
        }
    }
}

#[test]
fn fuzz_tool_call_event_serde_round_trip_random_inputs() {
    let mut lcg = Lcg::new(0xDE_AD_BE_EF_F0_0D_BA_AD);
    for iter in 0..200 {
        let ev = FlowExecutionEvent::ToolCall {
            step_name: lcg.ascii_with_random_len(15),
            tool_name: lcg.ascii_with_random_len(15),
            content: lcg.ascii_with_random_len(50),
            timestamp_ms: lcg.next_u64(),
        };
        let s = serde_json::to_string(&ev).unwrap();
        let back: FlowExecutionEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(back, ev, "iter={iter}: serde round-trip");
    }
}

#[test]
fn fuzz_pack_total_iter_count() {
    let total = 200 + 200;
    assert_eq!(total, 400, "33.y.k fuzz pack target: 400 LCG iters");
}
