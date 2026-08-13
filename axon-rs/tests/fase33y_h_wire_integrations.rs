//! §Fase 33.y.h integration tests — wire integrations (π-calc +
//! persistence + multi-agent deliberation).
//!
//! Exercises the 10 graduated wire-integration handlers through
//! `dispatch_node`:
//!
//! - π-calc typed channels: Emit / Publish / Discover (Fase 13)
//! - Persistence: Persist / Retrieve / Mutate / Purge / Transact
//! - Multi-agent blocks: Deliberate / Consensus
//!
//! D-letter coverage:
//! - D1 — 40 of 45 variants graduated (cumulative).
//! - D3 — cancel propagation across each handler.
//! - D7 — every error case routes through DispatchError.
//! - D10 — sync-runner parity: in-memory let_bindings backing
//!   matches the principled persistence + channel discipline.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::wire_integrations::{
    discover_capability, emit_to_channel, mutate_store, persist_to_store,
    publish_capability, purge_from_store, retrieve_from_store,
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

fn emit_node(channel: &str, value: &str) -> IRFlowNode {
    IRFlowNode::Emit(IREmit {
        node_type: "emit",
        source_line: 0,
        source_column: 0,
        channel_ref: channel.into(),
        value_ref: value.into(),
        value_is_channel: false,
        shield_ref: String::new(),
    
        breach_policy: None,
        scan: Vec::new(),
    })
}

fn publish_node(channel: &str, shield: &str) -> IRFlowNode {
    IRFlowNode::Publish(IRPublish {
        node_type: "publish",
        source_line: 0,
        source_column: 0,
        channel_ref: channel.into(),
        shield_ref: shield.into(),
        sign: String::new(),
    })
}

fn discover_node(capability: &str, alias: &str) -> IRFlowNode {
    IRFlowNode::Discover(IRDiscover {
        node_type: "discover",
        source_line: 0,
        source_column: 0,
        capability_ref: capability.into(),
        alias: alias.into(),
    })
}

fn persist_node(store: &str) -> IRFlowNode {
    IRFlowNode::Persist(IRPersistStep {
        node_type: "persist",
            fields: Vec::new(),
        source_line: 0,
        source_column: 0,
        store_name: store.into(),
    })
}

fn retrieve_node(store: &str, where_expr: &str, alias: &str) -> IRFlowNode {
    IRFlowNode::Retrieve(IRRetrieveStep {
        node_type: "retrieve",
        source_line: 0,
        source_column: 0,
        store_name: store.into(),
        where_expr: where_expr.into(),
        alias: alias.into(),
        order_by: String::new(),
        limit_expr: String::new(),
        aggregate: String::new(),
        group_by: String::new(),
        cache: String::new(),
    })
}

fn mutate_node(store: &str, where_expr: &str) -> IRFlowNode {
    IRFlowNode::Mutate(IRMutateStep {
        node_type: "mutate",
            fields: Vec::new(),
        source_line: 0,
        source_column: 0,
        store_name: store.into(),
        where_expr: where_expr.into(),
    })
}

fn purge_node(store: &str, where_expr: &str) -> IRFlowNode {
    IRFlowNode::Purge(IRPurgeStep {
        node_type: "purge",
        source_line: 0,
        source_column: 0,
        store_name: store.into(),
        where_expr: where_expr.into(),
    })
}

fn transact_node() -> IRFlowNode {
    IRFlowNode::Transact(IRTransactBlock {
        node_type: "transact",
        source_line: 0,
        source_column: 0,
    })
}

fn deliberate_node() -> IRFlowNode {
    IRFlowNode::Deliberate(IRDeliberateBlock {
        node_type: "deliberate",
        source_line: 0,
        source_column: 0,
    })
}

fn consensus_node() -> IRFlowNode {
    IRFlowNode::Consensus(IRConsensusBlock {
        node_type: "consensus",
        source_line: 0,
        source_column: 0,
    })
}

// ────────────────────────────────────────────────────────────────────
//  §1 — Public helpers (OSS reference impl)
// ────────────────────────────────────────────────────────────────────

#[test]
fn emit_to_channel_buffer_accumulates() {
    let (mut ctx, _rx) = fresh_ctx();
    emit_to_channel("c", "a", &mut ctx);
    emit_to_channel("c", "b", &mut ctx);
    emit_to_channel("c", "c", &mut ctx);
    assert_eq!(ctx.let_bindings.get("__channel_c").unwrap(), "a\nb\nc");
}

#[test]
fn publish_then_discover_returns_shield_ref() {
    let (mut ctx, _rx) = fresh_ctx();
    publish_capability("ch", "shield_x", &mut ctx);
    assert_eq!(discover_capability("ch", &ctx), "shield_x");
}

#[test]
fn persist_snapshots_user_level_bindings_only() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("a".into(), "1".into());
    ctx.let_bindings.insert("b".into(), "2".into());
    ctx.let_bindings.insert("__internal".into(), "hidden".into());
    let count = persist_to_store("s", &mut ctx);
    assert_eq!(count, 2);
}

#[test]
fn retrieve_returns_persisted_value() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("key".into(), "value".into());
    persist_to_store("s", &mut ctx);
    assert_eq!(retrieve_from_store("s", "key", &ctx), "value");
}

#[test]
fn mutate_updates_then_purge_removes() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("c".into(), "1".into());
    persist_to_store("s", &mut ctx);
    ctx.let_bindings.insert("c".into(), "2".into());
    assert_eq!(mutate_store("s", "c", &mut ctx), 1);
    assert_eq!(retrieve_from_store("s", "c", &ctx), "2");
    assert_eq!(purge_from_store("s", "c", &mut ctx), 1);
    assert_eq!(retrieve_from_store("s", "c", &ctx), "");
}

// ────────────────────────────────────────────────────────────────────
//  §2 — Emit through dispatch_node
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_node_routes_emit_with_literal_value() {
    let (mut ctx, mut rx) = fresh_ctx();
    dispatch_node(&emit_node("outbox", "hello"), &mut ctx)
        .await
        .unwrap();
    assert_eq!(ctx.let_bindings.get("__channel_outbox").unwrap(), "hello");
    let first = rx.try_recv().unwrap();
    match first {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "emit");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

#[tokio::test]
async fn emit_resolves_value_through_let_bindings() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("payload".into(), "real-value".into());
    dispatch_node(&emit_node("ch", "payload"), &mut ctx).await.unwrap();
    assert_eq!(ctx.let_bindings.get("__channel_ch").unwrap(), "real-value");
}

// ────────────────────────────────────────────────────────────────────
//  §3 — Publish + Discover through dispatch_node
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn publish_then_discover_chain() {
    let (mut ctx, _rx) = fresh_ctx();
    dispatch_node(&publish_node("secure_chan", "hipaa_shield"), &mut ctx)
        .await
        .unwrap();
    dispatch_node(&discover_node("secure_chan", "found"), &mut ctx)
        .await
        .unwrap();
    assert_eq!(ctx.let_bindings.get("found").unwrap(), "hipaa_shield");
}

#[tokio::test]
async fn discover_missing_capability_binds_empty() {
    let (mut ctx, _rx) = fresh_ctx();
    dispatch_node(&discover_node("never_published", "alias"), &mut ctx)
        .await
        .unwrap();
    assert_eq!(ctx.let_bindings.get("alias").unwrap(), "");
}

// ────────────────────────────────────────────────────────────────────
//  §4 — Persist through dispatch_node
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn persist_snapshots_and_retrieve_returns_value() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("user_id".into(), "42".into());
    ctx.let_bindings.insert("status".into(), "active".into());

    dispatch_node(&persist_node("users"), &mut ctx).await.unwrap();
    dispatch_node(&retrieve_node("users", "user_id", "retrieved_id"), &mut ctx)
        .await
        .unwrap();
    assert_eq!(ctx.let_bindings.get("retrieved_id").unwrap(), "42");
}

#[tokio::test]
async fn persist_returns_entry_count_in_output() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("a".into(), "1".into());
    ctx.let_bindings.insert("b".into(), "2".into());
    let outcome = dispatch_node(&persist_node("s"), &mut ctx).await.unwrap();
    match outcome {
        NodeOutcome::Completed { output, .. } => {
            assert!(output.contains("persisted 2 entries"));
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

// ────────────────────────────────────────────────────────────────────
//  §5 — Mutate + Purge through dispatch_node
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn mutate_chain_changes_persisted_value() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("ver".into(), "1.0".into());
    dispatch_node(&persist_node("releases"), &mut ctx).await.unwrap();

    ctx.let_bindings.insert("ver".into(), "2.0".into());
    dispatch_node(&mutate_node("releases", "ver"), &mut ctx).await.unwrap();

    // Re-retrieve from store
    dispatch_node(&retrieve_node("releases", "ver", "latest"), &mut ctx)
        .await
        .unwrap();
    assert_eq!(ctx.let_bindings.get("latest").unwrap(), "2.0");
}

#[tokio::test]
async fn purge_removes_persisted_entry() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("tmp".into(), "scratch".into());
    dispatch_node(&persist_node("workspace"), &mut ctx).await.unwrap();

    dispatch_node(&purge_node("workspace", "tmp"), &mut ctx).await.unwrap();

    dispatch_node(&retrieve_node("workspace", "tmp", "retrieved"), &mut ctx)
        .await
        .unwrap();
    assert_eq!(ctx.let_bindings.get("retrieved").unwrap(), "");
}

#[tokio::test]
async fn mutate_missing_entry_outputs_zero_count() {
    let (mut ctx, _rx) = fresh_ctx();
    let outcome = dispatch_node(&mutate_node("nonexistent", "k"), &mut ctx)
        .await
        .unwrap();
    match outcome {
        NodeOutcome::Completed { output, .. } => {
            assert!(output.contains("mutated 0 entries"));
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

// ────────────────────────────────────────────────────────────────────
//  §6 — Transact through dispatch_node
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn transact_sets_active_marker() {
    let (mut ctx, mut rx) = fresh_ctx();
    dispatch_node(&transact_node(), &mut ctx).await.unwrap();
    assert_eq!(ctx.let_bindings.get("__txn_active").unwrap(), "true");
    let first = rx.try_recv().unwrap();
    match first {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "transact");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

// ────────────────────────────────────────────────────────────────────
//  §7 — Deliberate + Consensus through dispatch_node
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_node_routes_deliberate() {
    let (mut ctx, mut rx) = fresh_ctx();
    let outcome = dispatch_node(&deliberate_node(), &mut ctx).await.unwrap();
    match outcome {
        NodeOutcome::Completed { output, tokens_emitted, .. } => {
            assert_eq!(output, "");
            assert_eq!(tokens_emitted, 0);
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    let first = rx.try_recv().unwrap();
    match first {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "deliberate");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

#[tokio::test]
async fn dispatch_node_routes_consensus() {
    let (mut ctx, mut rx) = fresh_ctx();
    let outcome = dispatch_node(&consensus_node(), &mut ctx).await.unwrap();
    match outcome {
        NodeOutcome::Completed { output, tokens_emitted, .. } => {
            assert_eq!(output, "");
            assert_eq!(tokens_emitted, 0);
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    let first = rx.try_recv().unwrap();
    match first {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "consensus");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

// ────────────────────────────────────────────────────────────────────
//  §8 — Cancel propagation
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cancel_propagates_into_every_wire_handler() {
    let nodes: Vec<IRFlowNode> = vec![
        emit_node("c", "v"),
        publish_node("c", "s"),
        discover_node("c", "a"),
        persist_node("s"),
        retrieve_node("s", "w", "a"),
        mutate_node("s", "w"),
        purge_node("s", "w"),
        transact_node(),
        deliberate_node(),
        consensus_node(),
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
//  §9 — Composition with cognitive + orchestration
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn emit_inside_for_in_per_iter_accumulates_buffer() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("xs".into(), "alpha,beta,gamma".into());
    let for_in = IRFlowNode::ForIn(IRForIn {
        node_type: "for_in",
        source_line: 0,
        source_column: 0,
        variable: "current".into(),
        iterable: "xs".into(),
        body: vec![emit_node("audit_log", "current")],
    });
    dispatch_node(&for_in, &mut ctx).await.unwrap();
    let buffer = ctx.let_bindings.get("__channel_audit_log").unwrap();
    // 3 iters → 3 emissions newline-joined.
    let parts: Vec<&str> = buffer.split('\n').collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts, vec!["alpha", "beta", "gamma"]);
}

#[tokio::test]
async fn persist_chain_with_cognitive_remember() {
    let (mut ctx, _rx) = fresh_ctx();
    // Use Remember to set up state
    dispatch_node(
        &IRFlowNode::Remember(IRRememberStep {
            node_type: "remember",
            source_line: 0,
            source_column: 0,
            expression: "snapshot-data".into(),
            memory_target: "snap".into(),
        }),
        &mut ctx,
    )
    .await
    .unwrap();

    // Now persist
    dispatch_node(&persist_node("session_snaps"), &mut ctx).await.unwrap();

    // And retrieve
    dispatch_node(&retrieve_node("session_snaps", "snap", "loaded"), &mut ctx)
        .await
        .unwrap();
    assert_eq!(ctx.let_bindings.get("loaded").unwrap(), "snapshot-data");
}

#[tokio::test]
async fn transact_then_persist_inside_block() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("k".into(), "v".into());
    dispatch_node(&transact_node(), &mut ctx).await.unwrap();
    dispatch_node(&persist_node("tx_store"), &mut ctx).await.unwrap();
    // txn_active should be true; persistence completed inside tx.
    assert_eq!(ctx.let_bindings.get("__txn_active").unwrap(), "true");
    assert_eq!(ctx.let_bindings.get("__store_tx_store_k").unwrap(), "v");
}

// ────────────────────────────────────────────────────────────────────
//  §10 — Step counter discipline
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn all_wire_handlers_advance_step_counter() {
    let (mut ctx, _rx) = fresh_ctx();
    let nodes = vec![
        emit_node("c", "v"),
        publish_node("c", "s"),
        discover_node("c", "a"),
        persist_node("s"),
        retrieve_node("s", "w", "a"),
        mutate_node("s", "w"),
        purge_node("s", "w"),
        transact_node(),
        deliberate_node(),
        consensus_node(),
    ];
    for (i, node) in nodes.iter().enumerate() {
        dispatch_node(node, &mut ctx).await.unwrap();
        assert_eq!(ctx.step_counter, i + 1, "after {} handlers, counter={}", i + 1, ctx.step_counter);
    }
    assert_eq!(ctx.step_counter, 10);
}
