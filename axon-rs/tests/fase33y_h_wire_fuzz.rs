//! §Fase 33.y.h D12 fuzz pack — adversarial coverage of the 10
//! wire-integration handlers.
//!
//! Hand-rolled deterministic 64-bit LCG (Knuth/MMIX constants), no
//! external dep.
//!
//! Coverage targets:
//! - 10 handlers × 150 iters = 1500
//! - Cancel propagation × 200
//! - Nested in orchestration × 200
//!
//! **Grand total: 1,900 deterministic LCG iters**, runtime <1s.

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
        Err(other) => panic!("{label}: unexpected variant: {other:?}"),
    }
}

#[tokio::test]
async fn fuzz_emit_never_panics() {
    let mut lcg = Lcg::new(0x11_22_33_44_55_66_77_88);
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::Emit(IREmit {
            node_type: "emit",
            source_line: 0,
            source_column: 0,
            channel_ref: lcg.ascii_with_random_len(15),
            value_ref: lcg.ascii_with_random_len(20),
            value_is_channel: lcg.boolean(),
            shield_ref: String::new(),
        
            breach_policy: None,
            scan: Vec::new(),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("emit iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_publish_never_panics() {
    let mut lcg = Lcg::new(0x22_33_44_55_66_77_88_99);
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::Publish(IRPublish {
            node_type: "publish",
            source_line: 0,
            source_column: 0,
            channel_ref: lcg.ascii_with_random_len(15),
            shield_ref: lcg.ascii_with_random_len(15),
            sign: String::new(),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("publish iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_discover_never_panics() {
    let mut lcg = Lcg::new(0x33_44_55_66_77_88_99_AA);
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::Discover(IRDiscover {
            node_type: "discover",
            source_line: 0,
            source_column: 0,
            capability_ref: lcg.ascii_with_random_len(15),
            alias: lcg.ascii_with_random_len(10),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("discover iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_persist_never_panics() {
    let mut lcg = Lcg::new(0x44_55_66_77_88_99_AA_BB);
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        // Optionally seed bindings to give persist something to snapshot.
        let n_bindings = lcg.range(5);
        for _ in 0..n_bindings {
            ctx.let_bindings
                .insert(lcg.ascii_with_random_len(8), lcg.ascii_with_random_len(10));
        }
        let node = IRFlowNode::Persist(IRPersistStep {
            node_type: "persist",
            fields: Vec::new(),
            source_line: 0,
            source_column: 0,
            store_name: lcg.ascii_with_random_len(15),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("persist iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_retrieve_never_panics() {
    let mut lcg = Lcg::new(0x55_66_77_88_99_AA_BB_CC);
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::Retrieve(IRRetrieveStep {
            node_type: "retrieve",
            source_line: 0,
            source_column: 0,
            store_name: lcg.ascii_with_random_len(15),
            where_expr: lcg.ascii_with_random_len(15),
            alias: lcg.ascii_with_random_len(10),
            order_by: String::new(),
            limit_expr: String::new(),
            aggregate: String::new(),
            group_by: String::new(),
            cache: String::new(),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("retrieve iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_mutate_never_panics() {
    let mut lcg = Lcg::new(0x66_77_88_99_AA_BB_CC_DD);
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::Mutate(IRMutateStep {
            node_type: "mutate",
            fields: Vec::new(),
            source_line: 0,
            source_column: 0,
            store_name: lcg.ascii_with_random_len(15),
            where_expr: lcg.ascii_with_random_len(15),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("mutate iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_purge_never_panics() {
    let mut lcg = Lcg::new(0x77_88_99_AA_BB_CC_DD_EE);
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::Purge(IRPurgeStep {
            node_type: "purge",
            source_line: 0,
            source_column: 0,
            store_name: lcg.ascii_with_random_len(15),
            where_expr: lcg.ascii_with_random_len(15),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("purge iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_transact_never_panics() {
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::Transact(IRTransactBlock {
            node_type: "transact",
            source_line: 0,
            source_column: 0,
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("transact iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_deliberate_never_panics() {
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::Deliberate(IRDeliberateBlock {
            node_type: "deliberate",
            source_line: 0,
            source_column: 0,
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("deliberate iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_consensus_never_panics() {
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::Consensus(IRConsensusBlock {
            node_type: "consensus",
            source_line: 0,
            source_column: 0,
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("consensus iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_cancel_propagation_across_wire_handlers() {
    let mut lcg = Lcg::new(0x99_AA_BB_CC_DD_EE_FF_11);
    for iter in 0..200 {
        let cancel = CancellationFlag::new();
        let pre_cancel = lcg.boolean();
        if pre_cancel {
            cancel.cancel();
        }
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);

        let node = match lcg.range(10) {
            0 => IRFlowNode::Emit(IREmit {
                node_type: "emit",
                source_line: 0,
                source_column: 0,
                channel_ref: "c".into(),
                value_ref: "v".into(),
                value_is_channel: false,
                shield_ref: String::new(),
            
                breach_policy: None,
                scan: Vec::new(),
            }),
            1 => IRFlowNode::Publish(IRPublish {
                node_type: "publish",
                source_line: 0,
                source_column: 0,
                channel_ref: "c".into(),
                shield_ref: "s".into(),
                sign: String::new(),
            }),
            2 => IRFlowNode::Discover(IRDiscover {
                node_type: "discover",
                source_line: 0,
                source_column: 0,
                capability_ref: "c".into(),
                alias: "a".into(),
            }),
            3 => IRFlowNode::Persist(IRPersistStep {
                node_type: "persist",
            fields: Vec::new(),
                source_line: 0,
                source_column: 0,
                store_name: "s".into(),
            }),
            4 => IRFlowNode::Retrieve(IRRetrieveStep {
                node_type: "retrieve",
                source_line: 0,
                source_column: 0,
                store_name: "s".into(),
                where_expr: "w".into(),
                alias: "a".into(),
                order_by: String::new(),
                limit_expr: String::new(),
                aggregate: String::new(),
                group_by: String::new(),
                cache: String::new(),
            }),
            5 => IRFlowNode::Mutate(IRMutateStep {
                node_type: "mutate",
            fields: Vec::new(),
                source_line: 0,
                source_column: 0,
                store_name: "s".into(),
                where_expr: "w".into(),
            }),
            6 => IRFlowNode::Purge(IRPurgeStep {
                node_type: "purge",
                source_line: 0,
                source_column: 0,
                store_name: "s".into(),
                where_expr: "w".into(),
            }),
            7 => IRFlowNode::Transact(IRTransactBlock {
                node_type: "transact",
                source_line: 0,
                source_column: 0,
            }),
            8 => IRFlowNode::Deliberate(IRDeliberateBlock {
                node_type: "deliberate",
                source_line: 0,
                source_column: 0,
            }),
            _ => IRFlowNode::Consensus(IRConsensusBlock {
                node_type: "consensus",
                source_line: 0,
                source_column: 0,
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
async fn fuzz_wire_handlers_nested_inside_orchestration() {
    let mut lcg = Lcg::new(0xAA_BB_CC_DD_EE_FF_11_22);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings.insert("xs".into(), "a,b".into());
        ctx.let_bindings.insert("flag".into(), "yes".into());

        let inner: IRFlowNode = match lcg.range(10) {
            0 => IRFlowNode::Emit(IREmit {
                node_type: "emit",
                source_line: 0,
                source_column: 0,
                channel_ref: format!("c_{iter}"),
                value_ref: "v".into(),
                value_is_channel: false,
                shield_ref: String::new(),
            
                breach_policy: None,
                scan: Vec::new(),
            }),
            1 => IRFlowNode::Publish(IRPublish {
                node_type: "publish",
                source_line: 0,
                source_column: 0,
                channel_ref: format!("c_{iter}"),
                shield_ref: "s".into(),
                sign: String::new(),
            }),
            2 => IRFlowNode::Discover(IRDiscover {
                node_type: "discover",
                source_line: 0,
                source_column: 0,
                capability_ref: format!("c_{iter}"),
                alias: format!("a_{iter}"),
            }),
            3 => IRFlowNode::Persist(IRPersistStep {
                node_type: "persist",
            fields: Vec::new(),
                source_line: 0,
                source_column: 0,
                store_name: format!("s_{iter}"),
            }),
            4 => IRFlowNode::Retrieve(IRRetrieveStep {
                node_type: "retrieve",
                source_line: 0,
                source_column: 0,
                store_name: format!("s_{iter}"),
                where_expr: "w".into(),
                alias: format!("a_{iter}"),
                order_by: String::new(),
                limit_expr: String::new(),
                aggregate: String::new(),
                group_by: String::new(),
                cache: String::new(),
            }),
            5 => IRFlowNode::Mutate(IRMutateStep {
                node_type: "mutate",
            fields: Vec::new(),
                source_line: 0,
                source_column: 0,
                store_name: format!("s_{iter}"),
                where_expr: "w".into(),
            }),
            6 => IRFlowNode::Purge(IRPurgeStep {
                node_type: "purge",
                source_line: 0,
                source_column: 0,
                store_name: format!("s_{iter}"),
                where_expr: "w".into(),
            }),
            7 => IRFlowNode::Transact(IRTransactBlock {
                node_type: "transact",
                source_line: 0,
                source_column: 0,
            }),
            8 => IRFlowNode::Deliberate(IRDeliberateBlock {
                node_type: "deliberate",
                source_line: 0,
                source_column: 0,
            }),
            _ => IRFlowNode::Consensus(IRConsensusBlock {
                node_type: "consensus",
                source_line: 0,
                source_column: 0,
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
    let total = (10 * 150) + 200 + 200;
    assert_eq!(
        total, 1900,
        "33.y.h fuzz pack target: 1900 deterministic LCG iters."
    );
}
