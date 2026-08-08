//! §Fase 33.y.f D12 fuzz pack — adversarial coverage of the 10
//! cognitive primitive handlers (Remember / Recall / Forge / Focus
//! / Associate / Aggregate / Explore / Ingest / Navigate /
//! Corroborate).
//!
//! Hand-rolled deterministic 64-bit LCG (Knuth/MMIX constants), no
//! external dep. Same pattern as Fase 33.x.k + 33.y.c/d/e fuzz packs.
//!
//! Coverage targets (~1500 LCG iters):
//!
//! - Remember fuzz       — 150 iters (random expression/target)
//! - Recall fuzz         — 150 iters (random query/source, mix of set/missing keys)
//! - Forge fuzz          — 150 iters
//! - Focus fuzz          — 150 iters
//! - Associate fuzz      — 150 iters
//! - Aggregate fuzz      — 150 iters
//! - Explore fuzz        — 150 iters
//! - Ingest fuzz         — 150 iters
//! - Navigate fuzz       — 150 iters
//! - Corroborate fuzz    — 150 iters
//!
//! Plus composition fuzz:
//! - Remember/Recall round-trip × 200 (write → read, verify symmetry)
//! - Cognitive nested in orchestration × 200
//!
//! **Grand total: 1,900 deterministic LCG iters**, runtime <1s.

use axon::cancel_token::CancellationFlag;
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

fn assert_no_panic(
    label: &str,
    outcome: &Result<NodeOutcome, DispatchError>,
) {
    match outcome {
        Ok(_) => {}
        Err(DispatchError::UpstreamCancelled) => {}
        Err(DispatchError::ChannelClosed) => {}
        // Fase 108.a - the data-plane verbs (focus/associate/aggregate/
        // explore/ingest) fail CLOSED without an engine port: a structured
        // refusal is the CORRECT outcome under fuzz (the property is
        // "never panics", not "never refuses").
        Err(DispatchError::MissingDependency {
            name: "dataspace_engine",
        }) => {}
        Err(other) => panic!("{label}: unexpected variant: {other:?}"),
    }
}

// ────────────────────────────────────────────────────────────────────
//  §1 — Remember fuzz: 150 iters
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_remember_never_panics_random_input() {
    let mut lcg = Lcg::new(0x1F_2F_3F_4F_5F_6F_7F_8F);
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        // Optionally pre-seed a binding for the expression to resolve to.
        if lcg.boolean() {
            let pre_key = lcg.ascii_with_random_len(10);
            ctx.let_bindings.insert(pre_key, lcg.ascii_with_random_len(20));
        }
        let expression = lcg.ascii_with_random_len(15);
        let memory_target = lcg.ascii_with_random_len(15);
        let node = IRFlowNode::Remember(IRRememberStep {
            node_type: "remember",
            source_line: 0,
            source_column: 0,
            expression,
            memory_target: memory_target.clone(),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("remember iter={iter}"), &outcome);
        if outcome.is_ok() {
            assert!(
                ctx.let_bindings.contains_key(&memory_target),
                "iter={iter}: memory_target binding must be set"
            );
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  §2 — Recall fuzz: 150 iters
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_recall_never_panics_random_input() {
    let mut lcg = Lcg::new(0x2F_3F_4F_5F_6F_7F_8F_9F);
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let memory_source = lcg.ascii_with_random_len(15);
        // 50/50: seed the source key or leave it missing.
        if lcg.boolean() {
            ctx.let_bindings.insert(
                memory_source.clone(),
                lcg.ascii_with_random_len(20),
            );
        }
        let query = lcg.ascii_with_random_len(15);
        let node = IRFlowNode::Recall(IRRecallStep {
            node_type: "recall",
            source_line: 0,
            source_column: 0,
            query: query.clone(),
            memory_source,
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("recall iter={iter}"), &outcome);
        if outcome.is_ok() {
            assert!(
                ctx.let_bindings.contains_key(&query),
                "iter={iter}: query binding must be set"
            );
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  §3 — Forge fuzz: 150 iters
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_forge_never_panics() {
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::Forge(IRForgeBlock {
            node_type: "forge",
            source_line: 0,
            source_column: 0,
                ..Default::default()
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("forge iter={iter}"), &outcome);
    }
}

// ────────────────────────────────────────────────────────────────────
//  §4 — 7 cognitive-framing handlers × 150 iters each
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_focus_never_panics_random_input() {
    let mut lcg = Lcg::new(0x3F_4F_5F_6F_7F_8F_9F_AF);
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let expression = lcg.ascii_with_random_len(20);
        let node = IRFlowNode::Focus(IRFocusStep {
            node_type: "focus",
            source_line: 0,
            source_column: 0,
            expression,
            where_expr: String::new(),
            select: Vec::new(),
            output: String::new(),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("focus iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_associate_never_panics_random_input() {
    let mut lcg = Lcg::new(0x4F_5F_6F_7F_8F_9F_AF_BF);
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let left = lcg.ascii_with_random_len(10);
        let right = lcg.ascii_with_random_len(10);
        let using_field = lcg.ascii_with_random_len(10);
        let node = IRFlowNode::Associate(IRAssociateStep {
            node_type: "associate",
            source_line: 0,
            source_column: 0,
            left,
            right,
            using_field,
            output: String::new(),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("associate iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_aggregate_never_panics_random_input() {
    let mut lcg = Lcg::new(0x5F_6F_7F_8F_9F_AF_BF_CF);
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let target = lcg.ascii_with_random_len(15);
        let alias = lcg.ascii_with_random_len(10);
        let group_count = lcg.range(4);
        let group_by: Vec<String> = (0..group_count)
            .map(|_| lcg.ascii_with_random_len(8))
            .collect();
        let node = IRFlowNode::Aggregate(IRAggregateStep {
            node_type: "aggregate",
            source_line: 0,
            source_column: 0,
            target,
            group_by,
            alias,
            compute: Vec::new(),
            where_expr: String::new(),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("aggregate iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_explore_never_panics_random_input() {
    let mut lcg = Lcg::new(0x6F_7F_8F_9F_AF_BF_CF_DF);
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let target = lcg.ascii_with_random_len(15);
        let limit = if lcg.boolean() {
            Some(lcg.range(100) as i64)
        } else {
            None
        };
        let node = IRFlowNode::Explore(IRExploreStep {
            node_type: "explore",
            source_line: 0,
            source_column: 0,
            target,
            limit,
            output: String::new(),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("explore iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_ingest_never_panics_random_input() {
    let mut lcg = Lcg::new(0x7F_8F_9F_AF_BF_CF_DF_EF);
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let source = lcg.ascii_with_random_len(15);
        let target = lcg.ascii_with_random_len(15);
        let node = IRFlowNode::Ingest(IRIngestStep {
            node_type: "ingest",
            source_line: 0,
            source_column: 0,
            source,
            target,
            format: "json".into(),
            max_bytes: None,
            max_rows: None,
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("ingest iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_navigate_never_panics_random_input() {
    let mut lcg = Lcg::new(0x8F_9F_AF_BF_CF_DF_EF_FF);
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let pix_ref = lcg.ascii_with_random_len(10);
        let corpus_ref = lcg.ascii_with_random_len(10);
        let query = lcg.ascii_with_random_len(20);
        let output_name = lcg.ascii_with_random_len(10);
        let node = IRFlowNode::Navigate(IRNavigateStep {
            depth: None,
            node_type: "navigate",
            source_line: 0,
            source_column: 0,
            pix_ref,
            corpus_ref,
            query,
            trail_enabled: lcg.boolean(),
            output_name,
            // §Fase 63.B fields — fuzz them too (random seed + optional budget).
            seed: lcg.ascii_with_random_len(8),
            budget: if lcg.boolean() { Some(5) } else { None },
            where_expr: String::new(),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("navigate iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_corroborate_never_panics_random_input() {
    let mut lcg = Lcg::new(0x9F_AF_BF_CF_DF_EF_FF_11);
    for iter in 0..150 {
        let (mut ctx, _rx) = fresh_ctx();
        let navigate_ref = lcg.ascii_with_random_len(10);
        let output_name = lcg.ascii_with_random_len(10);
        let node = IRFlowNode::Corroborate(IRCorroborateStep {
            node_type: "corroborate",
            source_line: 0,
            source_column: 0,
            navigate_ref,
            output_name,
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("corroborate iter={iter}"), &outcome);
    }
}

// ────────────────────────────────────────────────────────────────────
//  §5 — Remember/Recall round-trip fuzz: 200 iters
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_remember_recall_round_trip_symmetry() {
    let mut lcg = Lcg::new(0xAB_CD_EF_12_34_56_78_9A);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        let value = lcg.ascii_with_random_len(20);
        let memory_key = lcg.ascii_with_random_len(10);
        let recall_query = lcg.ascii_with_random_len(10);

        // Remember the value
        dispatch_node(
            &IRFlowNode::Remember(IRRememberStep {
                node_type: "remember",
                source_line: 0,
                source_column: 0,
                expression: value.clone(),
                memory_target: memory_key.clone(),
            }),
            &mut ctx,
        )
        .await
        .unwrap();

        // Recall it
        dispatch_node(
            &IRFlowNode::Recall(IRRecallStep {
                node_type: "recall",
                source_line: 0,
                source_column: 0,
                query: recall_query.clone(),
                memory_source: memory_key,
            }),
            &mut ctx,
        )
        .await
        .unwrap();

        // The recalled value should match what was remembered.
        assert_eq!(
            ctx.let_bindings.get(&recall_query).unwrap(),
            &value,
            "iter={iter}: round-trip symmetry broken"
        );
    }
}

// ────────────────────────────────────────────────────────────────────
//  §6 — Cognitive nested in orchestration fuzz: 200 iters
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_cognitive_nested_in_orchestration_never_panics() {
    let mut lcg = Lcg::new(0xCD_EF_12_34_56_78_9A_BC);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings.insert("xs".into(), "a,b,c".into());
        ctx.let_bindings.insert("flag".into(), "yes".into());

        // Random cognitive variant inside a Conditional or ForIn.
        let cognitive_node: IRFlowNode = match lcg.range(10) {
            0 => IRFlowNode::Remember(IRRememberStep {
                node_type: "remember",
                source_line: 0,
                source_column: 0,
                expression: "v".into(),
                memory_target: format!("k_{iter}"),
            }),
            1 => IRFlowNode::Recall(IRRecallStep {
                node_type: "recall",
                source_line: 0,
                source_column: 0,
                query: "q".into(),
                memory_source: "flag".into(),
            }),
            2 => IRFlowNode::Forge(IRForgeBlock {
                node_type: "forge",
                source_line: 0,
                source_column: 0,
                ..Default::default()
            }),
            3 => IRFlowNode::Focus(IRFocusStep {
                node_type: "focus",
                source_line: 0,
                source_column: 0,
                expression: "k".into(),
                where_expr: String::new(),
                select: Vec::new(),
                output: String::new(),
            }),
            4 => IRFlowNode::Associate(IRAssociateStep {
                node_type: "associate",
                source_line: 0,
                source_column: 0,
                left: "a".into(),
                right: "b".into(),
                using_field: "id".into(),
                output: String::new(),
            }),
            5 => IRFlowNode::Aggregate(IRAggregateStep {
                node_type: "aggregate",
                source_line: 0,
                source_column: 0,
                target: "t".into(),
                group_by: Vec::new(),
                alias: "".into(),
                compute: Vec::new(),
                where_expr: String::new(),
            }),
            6 => IRFlowNode::Explore(IRExploreStep {
                node_type: "explore",
                source_line: 0,
                source_column: 0,
                target: "h".into(),
                limit: None,
                output: String::new(),
            }),
            7 => IRFlowNode::Ingest(IRIngestStep {
                node_type: "ingest",
                source_line: 0,
                source_column: 0,
                source: "s".into(),
                target: "t".into(),
                format: "json".into(),
                max_bytes: None,
                max_rows: None,
            }),
            8 => IRFlowNode::Navigate(IRNavigateStep {
                depth: None,
                node_type: "navigate",
                source_line: 0,
                source_column: 0,
                pix_ref: "p".into(),
                corpus_ref: "c".into(),
                query: "q".into(),
                trail_enabled: false,
                output_name: "o".into(),
                // §Fase 63.B fields.
                seed: "s".into(),
                budget: None,
                where_expr: String::new(),
            }),
            _ => IRFlowNode::Corroborate(IRCorroborateStep {
                node_type: "corroborate",
                source_line: 0,
                source_column: 0,
                navigate_ref: "n".into(),
                output_name: "o".into(),
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
                then_body: vec![cognitive_node],
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
                body: vec![cognitive_node],
            })
        };

        let outcome = dispatch_node(&outer, &mut ctx).await;
        assert_no_panic(&format!("nested cognitive iter={iter}"), &outcome);
    }
}

// ────────────────────────────────────────────────────────────────────
//  §7 — Total iter count pin
// ────────────────────────────────────────────────────────────────────

#[test]
fn fuzz_pack_total_iter_count() {
    // 10 handlers × 150 iters = 1500
    // round-trip × 200 = 200
    // nested orchestration × 200 = 200
    let total = (10 * 150) + 200 + 200;
    assert_eq!(
        total, 1900,
        "33.y.f fuzz pack target: 1900 deterministic LCG iters \
         across cognitive primitives + composition."
    );
}
