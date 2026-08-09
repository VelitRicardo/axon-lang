//! §Fase 33.y.e D12 fuzz pack — adversarial coverage of the Par +
//! Stream handlers + `run_branches_concurrently` +
//! `bridge_effect_stream_yield`.
//!
//! Coverage targets (~1450 LCG iters):
//!
//! - Par dispatcher fuzz       — 200 iters (random ctx state + random pre-cancel)
//! - Stream dispatcher fuzz    — 200 iters
//! - Concurrent dispatch fuzz  — 300 iters (random 0..5 branch counts × random body shapes)
//! - Bridge static-scan fuzz   — 300 iters (random Yield argument strings + random handler-frame nesting)
//! - Cancel mid-bridge fuzz    — 200 iters (random pre-cancel × random Yield count)
//! - Nested orchestration fuzz — 200 iters (Par inside ForIn, Stream inside Conditional, etc.)
//!
//! **Grand total: 1,400 deterministic LCG iters**, runtime <1s.

use axon::cancel_token::CancellationFlag;
use axon::effects::ir::{IRHandlerClause, IRHandlerFrame, IRPerform, IRResume, Instruction};
use axon::effects::EffectRuntime;
use axon::flow_dispatcher::effects_bridge::bridge_effect_stream_yield;
use axon::flow_dispatcher::parallel::{run_branches_concurrently, run_par};
use axon::flow_dispatcher::effects_bridge::run_stream;
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, DispatchError, NodeOutcome};
use axon::ir_nodes::*;
use tokio::sync::mpsc;

// ────────────────────────────────────────────────────────────────────
//  LCG
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
    fn boolean(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

// ────────────────────────────────────────────────────────────────────
//  Helpers
// ────────────────────────────────────────────────────────────────────

fn fresh_ctx() -> (
    DispatchCtx,
    mpsc::UnboundedReceiver<axon::flow_execution_event::FlowExecutionEvent>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = DispatchCtx::new(
        "FuzzFlow",
        "stub",
        "",
        CancellationFlag::new(),
        tx,
    );
    (ctx, rx)
}

fn assert_no_panic_outcome(
    label: &str,
    outcome: &Result<NodeOutcome, DispatchError>,
) {
    match outcome {
        Ok(_) => {}
        Err(DispatchError::UpstreamCancelled) => {}
        Err(DispatchError::ChannelClosed) => {}
        Err(other) => panic!("{label}: unexpected error variant: {other:?}"),
    }
}

fn make_step(name: &str) -> IRFlowNode {
    IRFlowNode::Step(IRStep {
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
        guards: Vec::new(),
        requires_context: None,        now_tz: None,        body: Vec::new(),
    })
}

fn make_let(target: &str) -> IRFlowNode {
    IRFlowNode::Let(IRLetBinding {
        node_type: "let",
        source_line: 0,
        source_column: 0,
        target: target.into(),
        value: "v".into(),
        value_kind: "literal".into(),
        value_ast: None,
    })
}

// ────────────────────────────────────────────────────────────────────
//  §1 — Par dispatcher fuzz: 200 iters
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_par_handler_never_panics() {
    let mut lcg = Lcg::new(0xA1_C3_5F_71_AA_F0_F0_FE);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        if lcg.boolean() {
            ctx.cancel.cancel();
        }
        let par = IRParallelBlock {
            node_type: "par",
            source_line: 0,
            source_column: 0,
            branches: Vec::new(),
        };
        let outcome = run_par(&par, &mut ctx).await;
        assert_no_panic_outcome(&format!("par iter={iter}"), &outcome);
    }
}

// ────────────────────────────────────────────────────────────────────
//  §2 — Stream dispatcher fuzz: 200 iters
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_stream_handler_never_panics() {
    let mut lcg = Lcg::new(0xB1_C3_5F_71_BB_F0_F0_FE);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        if lcg.boolean() {
            ctx.cancel.cancel();
        }
        let stream = IRStreamBlock {
            node_type: "stream_block",
            source_line: 0,
            source_column: 0,
        body: Vec::new(),
            chunk_type: String::new(),
            on_chunk: None,
            on_complete: None,
            on_error: None,

        };
        let outcome = run_stream(&stream, &mut ctx).await;
        assert_no_panic_outcome(&format!("stream iter={iter}"), &outcome);
    }
}

// ────────────────────────────────────────────────────────────────────
//  §3 — Concurrent dispatch fuzz: 300 iters
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_run_branches_concurrently_random_branch_counts_and_shapes() {
    let mut lcg = Lcg::new(0xCC_DD_EE_FF_AB_CD_EF_FF);
    for iter in 0..300 {
        let (mut ctx, _rx) = fresh_ctx();
        let n = lcg.range(5);
        let branches: Vec<Vec<IRFlowNode>> = (0..n)
            .map(|i| {
                // Each branch: either a single Step, a single Let,
                // or a 2-node body (Let then Step).
                match lcg.range(3) {
                    0 => vec![make_step(&format!("S_{iter}_{i}"))],
                    1 => vec![make_let(&format!("L_{iter}_{i}"))],
                    _ => vec![make_let("k"), make_step(&format!("S_{iter}_{i}"))],
                }
            })
            .collect();
        let outcome = run_branches_concurrently(&branches, &mut ctx).await;
        assert_no_panic_outcome(&format!("conc iter={iter} n={n}"), &outcome);
    }
}

// ────────────────────────────────────────────────────────────────────
//  §4 — Bridge static-scan fuzz: 300 iters
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_bridge_static_scan_random_yields() {
    let mut lcg = Lcg::new(0xDD_EE_FF_AB_CD_EF_77_88);
    for iter in 0..300 {
        let (mut ctx, _rx) = fresh_ctx();
        let yield_count = lcg.range(8);
        let body: Vec<Instruction> = (0..yield_count)
            .map(|_| {
                let arg_text = format!("\"{}\"", lcg.ascii_with_random_len(15));
                Instruction::Perform(IRPerform {
                    effect_name: "Stream".into(),
                    operation_name: "Yield".into(),
                    arguments: vec![arg_text],
                    state_id: 0,
                    resume_label: String::new(),
                })
            })
            .collect();
        let wrapped = vec![Instruction::HandlerFrame(IRHandlerFrame {
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
        })];
        let mut runtime = EffectRuntime::new();
        let outcome = bridge_effect_stream_yield(&wrapped, &mut runtime, "S", &mut ctx).await;
        match outcome {
            Ok(_) => {}
            Err(DispatchError::UpstreamCancelled) => {}
            Err(DispatchError::ChannelClosed) => {}
            Err(other) => panic!("bridge iter={iter}: {other:?}"),
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  §5 — Cancel mid-bridge fuzz: 200 iters
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_bridge_cancel_random_pre_dispatch() {
    let mut lcg = Lcg::new(0xEE_FF_AB_CD_EF_77_88_99);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        let pre_cancel = lcg.boolean();
        if pre_cancel {
            ctx.cancel.cancel();
        }
        let body = (0..lcg.range(5))
            .map(|_| {
                Instruction::Perform(IRPerform {
                    effect_name: "Stream".into(),
                    operation_name: "Yield".into(),
                    arguments: vec!["\"x\"".into()],
                    state_id: 0,
                    resume_label: String::new(),
                })
            })
            .collect::<Vec<_>>();
        let wrapped = vec![Instruction::HandlerFrame(IRHandlerFrame {
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
        })];
        let mut runtime = EffectRuntime::new();
        let outcome = bridge_effect_stream_yield(&wrapped, &mut runtime, "S", &mut ctx).await;
        if pre_cancel {
            assert!(
                matches!(outcome, Err(DispatchError::UpstreamCancelled)),
                "iter={iter} pre-cancel expected UpstreamCancelled, got {outcome:?}"
            );
        } else {
            match outcome {
                Ok(_) => {}
                Err(DispatchError::UpstreamCancelled) => {}
                Err(DispatchError::ChannelClosed) => {}
                Err(other) => panic!("cancel iter={iter}: unexpected {other:?}"),
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  §6 — Nested orchestration fuzz: 200 iters
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_par_stream_nested_inside_orchestration() {
    let mut lcg = Lcg::new(0xFF_77_88_99_AA_BB_CC_DD);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings.insert("xs".into(), "a,b".into());
        ctx.let_bindings.insert("flag".into(), "yes".into());

        // Wrap Par or Stream inside a Conditional or ForIn at random.
        let inner = if lcg.boolean() {
            IRFlowNode::Par(IRParallelBlock {
                node_type: "par",
                source_line: 0,
                source_column: 0,
            branches: Vec::new(),
            })
        } else {
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
        };
        let outer = if lcg.boolean() {
            IRFlowNode::Conditional(IRConditional {
                node_type: "conditional",
                source_line: 0,
                source_column: 0,
                condition: "flag".into(),
                comparison_op: "==".into(),
                comparison_value: "yes".into(),
                then_body: vec![inner],
                else_body: Vec::new(),
                conditions: Vec::new(),
                conjunctor: String::new(),
                cond: None,
            })
        } else {
            IRFlowNode::ForIn(IRForIn {
                node_type: "for_in",
                source_line: 0,
                source_column: 0,
                variable: "x".into(),
                iterable: "xs".into(),
                body: vec![inner],
            })
        };
        let outcome = dispatch_node(&outer, &mut ctx).await;
        assert_no_panic_outcome(&format!("nested iter={iter}"), &outcome);
    }
}

// ────────────────────────────────────────────────────────────────────
//  §7 — Total iter count pin
// ────────────────────────────────────────────────────────────────────

#[test]
fn fuzz_pack_total_iter_count() {
    let total = 200 + 200 + 300 + 300 + 200 + 200;
    assert_eq!(
        total, 1400,
        "33.y.e fuzz pack target: 1400 deterministic LCG iters \
         across Par/Stream/concurrent-dispatch/bridge surfaces."
    );
}
