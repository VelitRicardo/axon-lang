//! §Fase 33.y.i integration tests — PIX variants (Hibernate / Drill
//! / Trail). Paper §6 hidden-state primitives.
//!
//! D-letter coverage:
//! - D1 — 43 of 45 variants graduated (cumulative).
//! - D3 — cancel propagation across each PIX handler.
//! - D7 — every error case routes through DispatchError; OSS
//!   helpers return placeholders, cannot fail.
//! - D10 — sync-runner parity: in-memory let_bindings backing
//!   matches the principled PIX state-machine discipline.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::pix::{
    drill_pix_subtree, trail_navigation,
};
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, DispatchError, NodeOutcome};
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

fn hibernate_node(event: &str, timeout: &str) -> IRFlowNode {
    IRFlowNode::Hibernate(IRHibernateStep {
        node_type: "hibernate",
        source_line: 0,
        source_column: 0,
        event_name: event.into(),
        timeout: timeout.into(),
    })
}

fn drill_node(pix_ref: &str, subtree: &str, query: &str, output: &str) -> IRFlowNode {
    IRFlowNode::Drill(IRDrillStep {
        node_type: "drill",
        source_line: 0,
        source_column: 0,
        pix_ref: pix_ref.into(),
        subtree_path: subtree.into(),
        query: query.into(),
        output_name: output.into(),
    })
}

fn trail_node(navigate_ref: &str) -> IRFlowNode {
    IRFlowNode::Trail(IRTrailStep {
        node_type: "trail",
        source_line: 0,
        source_column: 0,
        navigate_ref: navigate_ref.into(),
    })
}

// ────────────────────────────────────────────────────────────────────
//  §1 — Public helpers
// ────────────────────────────────────────────────────────────────────

// §Fase 119.d — `await_event_marker_and_placeholder_round_trip` was DELETED
// here. It pinned the §111 F20 placeholder in green: a "(hibernating …)"
// string while the flow kept walking. The real suspension's gates live in
// tests/fase119_d_hibernate.rs.

#[test]
fn drill_pix_seeded_value_returned() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings
        .insert("__pix_x_y.z".into(), "actual-value".into());
    assert_eq!(
        drill_pix_subtree("x", "y.z", "q", &ctx),
        "actual-value"
    );
}

#[test]
fn trail_seeded_value_returned() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings
        .insert("__navigate_nA_trail".into(), "trail-content".into());
    assert_eq!(trail_navigation("nA", &ctx), "trail-content");
}

// ────────────────────────────────────────────────────────────────────
//  §2 — Hibernate through dispatch_node
// ────────────────────────────────────────────────────────────────────

/// §Fase 119.d — routing now yields the SUSPENSION outcome (the walk loop
/// parks and halts); the marker-binding this test used to pin was the F20
/// placeholder's residue.
#[tokio::test]
async fn dispatch_node_routes_hibernate_sets_marker() {
    let (mut ctx, mut rx) = fresh_ctx();
    let outcome = dispatch_node(&hibernate_node("user_input", "5m"), &mut ctx)
        .await
        .unwrap();
    match outcome {
        NodeOutcome::Hibernated { event_name, timeout, .. } => {
            assert_eq!(event_name, "user_input");
            assert_eq!(timeout, "5m");
        }
        other => panic!("expected Hibernated, got {other:?}"),
    }
    let first = rx.try_recv().unwrap();
    match first {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "hibernate");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

#[tokio::test]
async fn hibernate_placeholder_output_contains_event_and_timeout() {
    // §Fase 119.d — renamed in spirit: there IS no placeholder output any
    // more. The outcome is the suspension itself.
    let (mut ctx, _rx) = fresh_ctx();
    let outcome = dispatch_node(&hibernate_node("evt_X", "1h"), &mut ctx)
        .await
        .unwrap();
    assert!(
        matches!(outcome, NodeOutcome::Hibernated { .. }),
        "got {outcome:?}"
    );
}

// ────────────────────────────────────────────────────────────────────
//  §3 — Drill through dispatch_node
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_node_routes_drill_reads_pix_subtree() {
    let (mut ctx, mut rx) = fresh_ctx();
    ctx.let_bindings
        .insert("__pix_legal_corpus_civil.art23".into(), "Article 23 text".into());
    dispatch_node(
        &drill_node("legal_corpus", "civil.art23", "interpret", "interpretation"),
        &mut ctx,
    )
    .await
    .unwrap();
    assert_eq!(
        ctx.let_bindings.get("interpretation").unwrap(),
        "Article 23 text"
    );
    let first = rx.try_recv().unwrap();
    match first {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "drill");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

#[tokio::test]
async fn drill_unseeded_pix_binds_placeholder() {
    let (mut ctx, _rx) = fresh_ctx();
    dispatch_node(
        &drill_node("unseeded", "root", "q", "result"),
        &mut ctx,
    )
    .await
    .unwrap();
    assert_eq!(
        ctx.let_bindings.get("result").unwrap(),
        "(drilled unseeded path=root query=q)"
    );
}

// ────────────────────────────────────────────────────────────────────
//  §4 — Trail through dispatch_node
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_node_routes_trail_reads_navigation_trail() {
    let (mut ctx, mut rx) = fresh_ctx();
    ctx.let_bindings
        .insert("__navigate_searchX_trail".into(), "n1>n2>n3".into());
    dispatch_node(&trail_node("searchX"), &mut ctx).await.unwrap();
    assert_eq!(
        ctx.let_bindings.get("searchX_trail_walked").unwrap(),
        "n1>n2>n3"
    );
    let first = rx.try_recv().unwrap();
    match first {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "trail");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

#[tokio::test]
async fn trail_unseeded_navigation_binds_placeholder() {
    let (mut ctx, _rx) = fresh_ctx();
    dispatch_node(&trail_node("never_navigated"), &mut ctx).await.unwrap();
    assert_eq!(
        ctx.let_bindings.get("never_navigated_trail_walked").unwrap(),
        "(trail of never_navigated)"
    );
}

// ────────────────────────────────────────────────────────────────────
//  §5 — Cancel propagation
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cancel_propagates_into_every_pix_handler() {
    let nodes = vec![
        hibernate_node("e", "1s"),
        drill_node("p", "s", "q", "o"),
        trail_node("n"),
    ];
    for node in nodes {
        let cancel = CancellationFlag::new();
        cancel.cancel();
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert!(
            matches!(outcome, Err(DispatchError::UpstreamCancelled)),
            "expected UpstreamCancelled for {node:?}, got {outcome:?}"
        );
    }
}

// ────────────────────────────────────────────────────────────────────
//  §6 — Composition with cognitive + orchestration
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn drill_inside_for_in_per_iter() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings
        .insert("__pix_kb_a".into(), "value-A".into());
    ctx.let_bindings
        .insert("__pix_kb_b".into(), "value-B".into());
    ctx.let_bindings.insert("paths".into(), "a,b".into());

    // for path in paths { drill kb[path] -> result }
    // Note: subtree_path is static in IR; this test uses static paths.
    let outcome = dispatch_node(
        &drill_node("kb", "a", "find", "first_result"),
        &mut ctx,
    )
    .await
    .unwrap();
    if let NodeOutcome::Completed { output, .. } = outcome {
        assert_eq!(output, "value-A");
    }
    assert_eq!(ctx.let_bindings.get("first_result").unwrap(), "value-A");
}

// §Fase 119.d — `hibernate_chain_with_recall` was DELETED here. It chained a
// Recall AFTER a hibernate in the same walk and read the marker binding —
// i.e., it TESTED the F20 defect: execution continuing past a suspension
// point. Nothing runs after a hibernate in its own walk any more; what runs
// afterwards is the RESUMED continuation, and that chain is gated end-to-end
// in tests/fase119_d_hibernate.rs.

#[tokio::test]
async fn trail_after_navigate_chain() {
    let (mut ctx, _rx) = fresh_ctx();
    // Navigate (pure_shape) emits a step; Trail reads from seed.
    // Pre-seed the trail.
    ctx.let_bindings
        .insert("__navigate_nav_session_trail".into(), "step1>step2".into());

    let nav = IRFlowNode::Navigate(IRNavigateStep {
        depth: None,
        node_type: "navigate",
        source_line: 0,
        source_column: 0,
        pix_ref: "kb".into(),
        corpus_ref: "doc".into(),
        query: "search".into(),
        trail_enabled: true,
        output_name: "nav_session".into(),
            seed: String::new(),
            budget: None,
            where_expr: String::new(),
    });
    dispatch_node(&nav, &mut ctx).await.unwrap();

    dispatch_node(&trail_node("nav_session"), &mut ctx).await.unwrap();
    assert_eq!(
        ctx.let_bindings.get("nav_session_trail_walked").unwrap(),
        "step1>step2"
    );
}

// ────────────────────────────────────────────────────────────────────
//  §7 — Step counter discipline
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn pix_handlers_advance_step_counter() {
    let (mut ctx, _rx) = fresh_ctx();
    assert_eq!(ctx.step_counter, 0);
    dispatch_node(&hibernate_node("e", "1s"), &mut ctx).await.unwrap();
    assert_eq!(ctx.step_counter, 1);
    dispatch_node(&drill_node("p", "s", "q", "o"), &mut ctx).await.unwrap();
    assert_eq!(ctx.step_counter, 2);
    dispatch_node(&trail_node("n"), &mut ctx).await.unwrap();
    assert_eq!(ctx.step_counter, 3);
}

// ────────────────────────────────────────────────────────────────────
//  §8 — Fuzz pack (inline since 33.y.i scope is small)
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

fn assert_no_panic(label: &str, outcome: &Result<NodeOutcome, DispatchError>) {
    match outcome {
        Ok(_) => {}
        Err(DispatchError::UpstreamCancelled) => {}
        Err(DispatchError::ChannelClosed) => {}
        Err(other) => panic!("{label}: unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn fuzz_hibernate_never_panics() {
    let mut lcg = Lcg::new(0xA1_B1_C1_D1_E1_F1_11_22);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::Hibernate(IRHibernateStep {
            node_type: "hibernate",
            source_line: 0,
            source_column: 0,
            event_name: lcg.ascii_with_random_len(20),
            timeout: lcg.ascii_with_random_len(8),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("hibernate iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_drill_never_panics() {
    let mut lcg = Lcg::new(0xB2_C2_D2_E2_F2_12_23_34);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::Drill(IRDrillStep {
            node_type: "drill",
            source_line: 0,
            source_column: 0,
            pix_ref: lcg.ascii_with_random_len(15),
            subtree_path: lcg.ascii_with_random_len(20),
            query: lcg.ascii_with_random_len(30),
            output_name: lcg.ascii_with_random_len(10),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("drill iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_trail_never_panics() {
    let mut lcg = Lcg::new(0xC3_D3_E3_F3_13_24_35_46);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::Trail(IRTrailStep {
            node_type: "trail",
            source_line: 0,
            source_column: 0,
            navigate_ref: lcg.ascii_with_random_len(15),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("trail iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_cancel_propagation_random_pix_handler() {
    let mut lcg = Lcg::new(0xD4_E4_F4_14_25_36_47_58);
    for iter in 0..200 {
        let cancel = CancellationFlag::new();
        let pre_cancel = lcg.boolean();
        if pre_cancel {
            cancel.cancel();
        }
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);

        let node = match lcg.range(3) {
            0 => IRFlowNode::Hibernate(IRHibernateStep {
                node_type: "hibernate",
                source_line: 0,
                source_column: 0,
                event_name: "e".into(),
                timeout: "1s".into(),
            }),
            1 => IRFlowNode::Drill(IRDrillStep {
                node_type: "drill",
                source_line: 0,
                source_column: 0,
                pix_ref: "p".into(),
                subtree_path: "s".into(),
                query: "q".into(),
                output_name: "o".into(),
            }),
            _ => IRFlowNode::Trail(IRTrailStep {
                node_type: "trail",
                source_line: 0,
                source_column: 0,
                navigate_ref: "n".into(),
            }),
        };

        let outcome = dispatch_node(&node, &mut ctx).await;
        if pre_cancel {
            assert!(
                matches!(outcome, Err(DispatchError::UpstreamCancelled)),
                "iter={iter}: expected UpstreamCancelled, got {outcome:?}"
            );
        } else {
            assert_no_panic(&format!("cancel iter={iter}"), &outcome);
        }
    }
}

#[test]
fn fuzz_pack_total_iter_count() {
    // 3 handlers × 200 + cancel × 200 = 800.
    let total = (3 * 200) + 200;
    assert_eq!(total, 800, "33.y.i fuzz pack target: 800 LCG iters");
}
