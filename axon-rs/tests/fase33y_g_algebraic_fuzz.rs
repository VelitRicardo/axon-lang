//! §Fase 33.y.g D12 fuzz pack — adversarial coverage of the 6
//! algebraic-effect handler nodes (ShieldApply / OtsApply /
//! MandateApply / ComputeApply / Listen / DaemonStep).
//!
//! Hand-rolled deterministic 64-bit LCG (Knuth/MMIX constants), no
//! external dep.
//!
//! Coverage targets (~1500 LCG iters):
//!
//! - ShieldApply × 200
//! - OtsApply × 200
//! - MandateApply × 200
//! - ComputeApply × 200 (random argument counts 0..6)
//! - Listen × 200
//! - DaemonStep × 200
//! - Cancel propagation × 200 (random handler × pre-cancel)
//! - Composition inside orchestration × 200
//!
//! **Grand total: 1,600 deterministic LCG iters**, runtime <1s.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, DispatchError, NodeOutcome};
use axon::ir_nodes::*;
use tokio::sync::mpsc;

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

fn assert_no_panic(
    label: &str,
    outcome: &Result<NodeOutcome, DispatchError>,
) {
    match outcome {
        Ok(_) => {}
        Err(DispatchError::UpstreamCancelled) => {}
        Err(DispatchError::ChannelClosed) => {}
        // §Fase 111.f — `compute_apply` REFUSES when the compute is undeclared,
        // the arity is wrong, or the term cannot be evaluated (this fuzz ctx
        // attaches no compute catalog and feeds random argument counts, so it
        // hits all three). The refusal is a TOTAL, correct outcome — and it is
        // the entire point of the fase.
        //
        // This fuzz used to pass because `run_compute_apply` returned
        // `Ok("compute:add(5, 7)")` for literally any input, including a random
        // number of arguments to a compute that did not exist. Totality was
        // achieved by never failing — which is exactly how a placeholder string
        // reached a downstream step that expected a number (§111 F10).
        //
        // A handler that cannot fail is not total. It is mute.
        Err(DispatchError::BackendError { name, .. }) if name == "compute" => {}
        // §Fase 119.b — the same doctrine, for the cage: `mandate_apply`
        // REFUSES when the mandate is undeclared (this fuzz ctx attaches no
        // mandate catalog, and every random name is undeclared). This fuzz
        // used to pass because the handler bound the target UNCHANGED under
        // any name at all — the §111 F18 identity. The refusal is the total,
        // correct outcome.
        Err(DispatchError::BackendError { name, .. }) if name.starts_with("mandate:") => {}
        Err(other) => panic!("{label}: unexpected error variant: {other:?}"),
    }
}

#[tokio::test]
async fn fuzz_shield_apply_never_panics() {
    let mut lcg = Lcg::new(0xA1_B2_C3_D4_E5_F6_77_88);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::ShieldApply(IRShieldApplyStep {
            node_type: "shield_apply",
            source_line: 0,
            source_column: 0,
            shield_name: lcg.ascii_with_random_len(15),
            target: lcg.ascii_with_random_len(20),
            output_type: lcg.ascii_with_random_len(15),
        
            breach_policy: None,
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("shield iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_ots_apply_never_panics() {
    let mut lcg = Lcg::new(0xB2_C3_D4_E5_F6_77_88_99);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::OtsApply(IROtsApplyStep {
            node_type: "ots_apply",
            source_line: 0,
            source_column: 0,
            ots_name: lcg.ascii_with_random_len(15),
            target: lcg.ascii_with_random_len(20),
            output_type: lcg.ascii_with_random_len(15),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("ots iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_mandate_apply_never_panics() {
    let mut lcg = Lcg::new(0xC3_D4_E5_F6_77_88_99_AA);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::MandateApply(IRMandateApplyStep {
            node_type: "mandate_apply",
            source_line: 0,
            source_column: 0,
            mandate_name: lcg.ascii_with_random_len(15),
            target: lcg.ascii_with_random_len(20),
            output_type: lcg.ascii_with_random_len(15),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("mandate iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_compute_apply_never_panics_random_arg_counts() {
    let mut lcg = Lcg::new(0xD4_E5_F6_77_88_99_AA_BB);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        let n_args = lcg.range(6);
        let arguments: Vec<String> = (0..n_args)
            .map(|_| lcg.ascii_with_random_len(10))
            .collect();
        let output_name = lcg.ascii_with_random_len(10);
        let node = IRFlowNode::ComputeApply(IRComputeApplyStep {
            node_type: "compute_apply",
            source_line: 0,
            source_column: 0,
            compute_name: lcg.ascii_with_random_len(15),
            arguments,
            output_name: output_name.clone(),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("compute iter={iter}"), &outcome);
        if outcome.is_ok() && !output_name.is_empty() {
            assert!(
                ctx.let_bindings.contains_key(&output_name),
                "iter={iter}: output_name binding must be set"
            );
        }
    }
}

#[tokio::test]
async fn fuzz_listen_never_panics() {
    let mut lcg = Lcg::new(0xE5_F6_77_88_99_AA_BB_CC);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::Listen(IRListenStep {
            node_type: "listen",
            source_line: 0,
            source_column: 0,
            channel: lcg.ascii_with_random_len(15),
            channel_is_ref: lcg.boolean(),
            event_alias: lcg.ascii_with_random_len(10),
            body: Vec::new(),        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("listen iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_daemon_step_never_panics() {
    let mut lcg = Lcg::new(0xF6_77_88_99_AA_BB_CC_DD);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::DaemonStep(IRDaemonStepNode {
            node_type: "daemon_step",
            source_line: 0,
            source_column: 0,
            daemon_ref: lcg.ascii_with_random_len(15),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("daemon iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_cancel_propagation_across_algebraic_handlers() {
    let mut lcg = Lcg::new(0x77_88_99_AA_BB_CC_DD_EE);
    for iter in 0..200 {
        let cancel = CancellationFlag::new();
        let pre_cancel = lcg.boolean();
        if pre_cancel {
            cancel.cancel();
        }
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);

        let node = match lcg.range(6) {
            0 => IRFlowNode::ShieldApply(IRShieldApplyStep {
                node_type: "shield_apply",
                source_line: 0,
                source_column: 0,
                shield_name: "s".into(),
                target: "t".into(),
                output_type: "o".into(),
            
                breach_policy: None,
            }),
            1 => IRFlowNode::OtsApply(IROtsApplyStep {
                node_type: "ots_apply",
                source_line: 0,
                source_column: 0,
                ots_name: "s".into(),
                target: "t".into(),
                output_type: "o".into(),
            }),
            2 => IRFlowNode::MandateApply(IRMandateApplyStep {
                node_type: "mandate_apply",
                source_line: 0,
                source_column: 0,
                mandate_name: "s".into(),
                target: "t".into(),
                output_type: "o".into(),
            }),
            3 => IRFlowNode::ComputeApply(IRComputeApplyStep {
                node_type: "compute_apply",
                source_line: 0,
                source_column: 0,
                compute_name: "s".into(),
                arguments: vec!["a".into()],
                output_name: "o".into(),
            }),
            4 => IRFlowNode::Listen(IRListenStep {
                node_type: "listen",
                source_line: 0,
                source_column: 0,
                channel: "c".into(),
                channel_is_ref: false,
                event_alias: "e".into(),
                body: Vec::new(),            }),
            _ => IRFlowNode::DaemonStep(IRDaemonStepNode {
                node_type: "daemon_step",
                source_line: 0,
                source_column: 0,
                daemon_ref: "d".into(),
            }),
        };

        let outcome = dispatch_node(&node, &mut ctx).await;
        if pre_cancel {
            assert!(
                matches!(outcome, Err(DispatchError::UpstreamCancelled)),
                "iter={iter} pre-cancel: expected UpstreamCancelled, got {outcome:?}"
            );
        } else {
            assert_no_panic(&format!("cancel iter={iter}"), &outcome);
        }
    }
}

#[tokio::test]
async fn fuzz_algebraic_nested_inside_orchestration() {
    let mut lcg = Lcg::new(0x88_99_AA_BB_CC_DD_EE_FF);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings.insert("xs".into(), "a,b".into());
        ctx.let_bindings.insert("flag".into(), "yes".into());

        let inner = match lcg.range(6) {
            0 => IRFlowNode::ShieldApply(IRShieldApplyStep {
                node_type: "shield_apply",
                source_line: 0,
                source_column: 0,
                shield_name: "h".into(),
                target: "input".into(),
                output_type: format!("out_{iter}"),
            
                breach_policy: None,
            }),
            1 => IRFlowNode::OtsApply(IROtsApplyStep {
                node_type: "ots_apply",
                source_line: 0,
                source_column: 0,
                ots_name: "o".into(),
                target: "input".into(),
                output_type: format!("out_{iter}"),
            }),
            2 => IRFlowNode::MandateApply(IRMandateApplyStep {
                node_type: "mandate_apply",
                source_line: 0,
                source_column: 0,
                mandate_name: "m".into(),
                target: "input".into(),
                output_type: format!("out_{iter}"),
            }),
            3 => IRFlowNode::ComputeApply(IRComputeApplyStep {
                node_type: "compute_apply",
                source_line: 0,
                source_column: 0,
                compute_name: "c".into(),
                arguments: vec![],
                output_name: format!("out_{iter}"),
            }),
            4 => IRFlowNode::Listen(IRListenStep {
                node_type: "listen",
                source_line: 0,
                source_column: 0,
                channel: "ch".into(),
                channel_is_ref: false,
                event_alias: format!("alias_{iter}"),
                body: Vec::new(),            }),
            _ => IRFlowNode::DaemonStep(IRDaemonStepNode {
                node_type: "daemon_step",
                source_line: 0,
                source_column: 0,
                daemon_ref: format!("d_{iter}"),
            }),
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
        assert_no_panic(&format!("nested iter={iter}"), &outcome);
    }
}

#[test]
fn fuzz_pack_total_iter_count() {
    let total = (6 * 200) + 200 + 200;
    assert_eq!(
        total, 1600,
        "33.y.g fuzz pack target: 1600 deterministic LCG iters."
    );
}
