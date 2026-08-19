//! v1.24.0 integration tests — cognitive primitives (v1.4.0
//! neuro-symbolic).
//!
//! Exercises the 10 cognitive variant handlers through the public
//! `dispatch_node` surface + the PEM async bridge for
//! Remember/Recall.
//!
//! D-letter coverage:
//! - D1 — 24 of 45 variants graduated (cumulative).
//! - D3 — cancel propagation across each handler.
//! - D6 — pure-shape-routed handlers push StepAuditRecord (via
//!   shared core); Remember/Recall/Forge do not (they're not
//!   wire-LLM steps).
//! - D7 — every error case routes through DispatchError; no panic.
//! - D10 — sync-runner parity: Remember binds + Recall reads via
//!   let_bindings identically; PEM write-through is transparent.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, DispatchError, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::*;
use axon::pem::{InMemoryBackend, PersistenceBackend};
use std::sync::Arc;
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

fn remember(expression: &str, memory_target: &str) -> IRFlowNode {
    IRFlowNode::Remember(IRRememberStep {
        node_type: "remember",
        source_line: 0,
        source_column: 0,
        expression: expression.into(),
        memory_target: memory_target.into(),
    })
}

fn recall(query: &str, memory_source: &str) -> IRFlowNode {
    IRFlowNode::Recall(IRRecallStep {
        node_type: "recall",
        source_line: 0,
        source_column: 0,
        query: query.into(),
        memory_source: memory_source.into(),
    })
}

fn forge() -> IRFlowNode {
    IRFlowNode::Forge(IRForgeBlock {
        node_type: "forge",
        source_line: 0,
        source_column: 0,
                ..Default::default()
    })
}

fn focus(expression: &str) -> IRFlowNode {
    IRFlowNode::Focus(IRFocusStep {
        node_type: "focus",
        source_line: 0,
        source_column: 0,
        expression: expression.into(),
        where_expr: String::new(),
        select: Vec::new(),
        output: String::new(),
    })
}

fn associate(left: &str, right: &str, using_field: &str) -> IRFlowNode {
    IRFlowNode::Associate(IRAssociateStep {
        node_type: "associate",
        source_line: 0,
        source_column: 0,
        left: left.into(),
        right: right.into(),
        using_field: using_field.into(),
        output: String::new(),
    })
}

fn aggregate(target: &str, group_by: Vec<&str>, alias: &str) -> IRFlowNode {
    IRFlowNode::Aggregate(IRAggregateStep {
        node_type: "aggregate",
        source_line: 0,
        source_column: 0,
        target: target.into(),
        group_by: group_by.into_iter().map(String::from).collect(),
        alias: alias.into(),
        compute: Vec::new(),
        where_expr: String::new(),
    })
}

fn explore(target: &str, limit: Option<i64>) -> IRFlowNode {
    IRFlowNode::Explore(IRExploreStep {
        node_type: "explore",
        source_line: 0,
        source_column: 0,
        target: target.into(),
        limit,
        output: String::new(),
    })
}

fn ingest(source: &str, target: &str) -> IRFlowNode {
    IRFlowNode::Ingest(IRIngestStep {
        node_type: "ingest",
        source_line: 0,
        source_column: 0,
        source: source.into(),
        target: target.into(),
        format: "json".into(),
        max_bytes: None,
        max_rows: None,
    })
}

fn navigate(pix_ref: &str, corpus_ref: &str, query: &str, output_name: &str) -> IRFlowNode {
    IRFlowNode::Navigate(IRNavigateStep {
        depth: None,
        node_type: "navigate",
        source_line: 0,
        source_column: 0,
        pix_ref: pix_ref.into(),
        corpus_ref: corpus_ref.into(),
        query: query.into(),
        trail_enabled: true,
        output_name: output_name.into(),
            seed: String::new(),
            budget: None,
            where_expr: String::new(),
    })
}

fn corroborate(navigate_ref: &str, output_name: &str) -> IRFlowNode {
    IRFlowNode::Corroborate(IRCorroborateStep {
        node_type: "corroborate",
        source_line: 0,
        source_column: 0,
        navigate_ref: navigate_ref.into(),
        output_name: output_name.into(),
    })
}

// ────────────────────────────────────────────────────────────────────
// section 1 — Remember through dispatch_node
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_node_routes_remember_to_cognitive_handler() {
    let (mut ctx, _rx) = fresh_ctx();
    let outcome = dispatch_node(&remember("hello-world", "greeting"), &mut ctx)
        .await
        .unwrap();
    match outcome {
        NodeOutcome::Completed { output, tokens_emitted, .. } => {
            assert_eq!(output, "hello-world");
            assert_eq!(tokens_emitted, 0);
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    assert_eq!(ctx.let_bindings.get("greeting").unwrap(), "hello-world");
}

#[tokio::test]
async fn remember_chain_resolves_expression_through_prior_bindings() {
    let (mut ctx, _rx) = fresh_ctx();
    // Bind "source" via Remember
    dispatch_node(&remember("alpha", "source"), &mut ctx).await.unwrap();
    // Remember "source" → "snapshot" should resolve through let_bindings.
    dispatch_node(&remember("source", "snapshot"), &mut ctx).await.unwrap();
    assert_eq!(ctx.let_bindings.get("snapshot").unwrap(), "alpha");
}

// ────────────────────────────────────────────────────────────────────
// section 2 — Recall through dispatch_node
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_node_routes_recall_to_cognitive_handler() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("region".into(), "us-east-1".into());
    let outcome = dispatch_node(&recall("current_region", "region"), &mut ctx)
        .await
        .unwrap();
    match outcome {
        NodeOutcome::Completed { output, .. } => {
            assert_eq!(output, "us-east-1");
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    assert_eq!(
        ctx.let_bindings.get("current_region").unwrap(),
        "us-east-1"
    );
}

#[tokio::test]
async fn recall_missing_key_yields_empty_string() {
    let (mut ctx, _rx) = fresh_ctx();
    let outcome = dispatch_node(&recall("x", "never_set"), &mut ctx)
        .await
        .unwrap();
    match outcome {
        NodeOutcome::Completed { output, .. } => assert_eq!(output, ""),
        other => panic!("expected Completed, got {other:?}"),
    }
    assert_eq!(ctx.let_bindings.get("x").unwrap(), "");
}

// ────────────────────────────────────────────────────────────────────
// section 3 — PEM integration: persist + restore round-trip
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn pem_round_trip_remember_then_recall_via_backend() {
    let backend: Arc<dyn PersistenceBackend> = Arc::new(InMemoryBackend::default());
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new(
        "F",
        "stub",
        "",
        CancellationFlag::new(),
        tx,
    )
    .with_pem(backend.clone())
    .with_session_id("session-XYZ");

    // Remember
    dispatch_node(&remember("persisted-content", "pem_key"), &mut ctx)
        .await
        .unwrap();

    // Verify PEM has the entry
    let state = backend.restore("session-XYZ").await.unwrap();
    assert_eq!(state.short_term_memory.len(), 1);
    assert_eq!(state.short_term_memory[0].key, "pem_key");

    // Drop let_bindings to simulate a fresh ctx (cold start)
    ctx.let_bindings.clear();

    // Recall should pull from PEM
    dispatch_node(&recall("retrieved", "pem_key"), &mut ctx)
        .await
        .unwrap();
    assert_eq!(
        ctx.let_bindings.get("retrieved").unwrap(),
        "persisted-content"
    );
}

#[tokio::test]
async fn pem_multiple_remember_calls_accumulate_in_short_term_memory() {
    let backend: Arc<dyn PersistenceBackend> = Arc::new(InMemoryBackend::default());
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new(
        "F",
        "stub",
        "",
        CancellationFlag::new(),
        tx,
    )
    .with_pem(backend.clone())
    .with_session_id("acc-session");

    dispatch_node(&remember("first", "k1"), &mut ctx).await.unwrap();
    dispatch_node(&remember("second", "k2"), &mut ctx).await.unwrap();
    dispatch_node(&remember("third", "k3"), &mut ctx).await.unwrap();

    let state = backend.restore("acc-session").await.unwrap();
    assert_eq!(state.short_term_memory.len(), 3);
    let keys: Vec<&str> = state.short_term_memory.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(keys, vec!["k1", "k2", "k3"]);
}

#[tokio::test]
async fn pem_recall_finds_latest_entry_when_key_repeated() {
    let backend: Arc<dyn PersistenceBackend> = Arc::new(InMemoryBackend::default());
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new(
        "F",
        "stub",
        "",
        CancellationFlag::new(),
        tx,
    )
    .with_pem(backend.clone())
    .with_session_id("rev-session");

    dispatch_node(&remember("v1", "evolving"), &mut ctx).await.unwrap();
    dispatch_node(&remember("v2", "evolving"), &mut ctx).await.unwrap();
    dispatch_node(&remember("v3", "evolving"), &mut ctx).await.unwrap();

    ctx.let_bindings.clear();
    dispatch_node(&recall("latest", "evolving"), &mut ctx).await.unwrap();
    assert_eq!(
        ctx.let_bindings.get("latest").unwrap(),
        "v3",
        "Recall finds the LATEST entry (reverse-iter discipline)"
    );
}

// ────────────────────────────────────────────────────────────────────
// section 4 — Forge canonical wire shape
// ────────────────────────────────────────────────────────────────────

/// v2.41.0 — `forge` is no longer a 1-step no-op; it routes to the real
/// Directed Creative Synthesis pipeline (Preparation → Incubation → Illumination
/// → Verification), so it emits MANY sub-step events and returns either a
/// Completed synthesis or a fail-closed structured error. This test asserts it
/// routes and runs (emits phase events); the forge-logic assertions live in the
/// `forge` + `cognitive::run_forge` unit tests.
#[tokio::test]
async fn dispatch_node_routes_forge_to_cognitive_handler() {
    let (mut ctx, mut rx) = fresh_ctx();
    // The pipeline may accept (Ok) or fail closed (Err) with the stub backend;
    // either is valid routing — we only assert it reached the handler.
    let _ = dispatch_node(&forge(), &mut ctx).await;
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    assert!(
        !events.is_empty(),
        "forge must route to a handler and emit at least one phase event"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            FlowExecutionEvent::StepStart { step_type, .. } if step_type == "forge"
        )),
        "forge pipeline must emit a `forge` step event"
    );
}

// ────────────────────────────────────────────────────────────────────
// section 5 — Cognitive-framing handlers each emit 1 token (stub backend)
// ────────────────────────────────────────────────────────────────────

async fn assert_pure_shape_like(node: IRFlowNode, expected_slug: &str) {
    let (mut ctx, mut rx) = fresh_ctx();
    let outcome = dispatch_node(&node, &mut ctx).await.unwrap();
    match outcome {
        NodeOutcome::Completed { output, tokens_emitted, .. } => {
            assert_eq!(output, "(stub)");
            assert_eq!(tokens_emitted, 1);
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    let first_ev = rx.try_recv().unwrap();
    match first_ev {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, expected_slug);
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
    // Audit row recorded by run_pure_shape's shared core.
    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit.len(), 1);
}

/// v2.63.0 — the five data-plane verbs REFUSE without an engine
/// port: `Err(MissingDependency { name: "dataspace_engine" })` after
/// the attributable StepStart. Pre-108.a they routed through
/// `run_pure_shape` and NARRATED their result — the hallucination the
/// honesty floor ended. No audit row: the refusal happens before any
/// LLM dispatch.
async fn assert_data_plane_refusal(node: IRFlowNode, expected_slug: &str) {
    let (mut ctx, mut rx) = fresh_ctx();
    let err = dispatch_node(&node, &mut ctx).await.unwrap_err();
    assert!(
        matches!(
            err,
            axon::flow_dispatcher::DispatchError::MissingDependency {
                name: "dataspace_engine"
            }
        ),
        "{expected_slug}: got {err:?}"
    );
    match rx.try_recv().unwrap() {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, expected_slug);
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
    assert_eq!(ctx.step_counter, 1, "the refusal still claims its step index");
    let audit = ctx.step_audit_records.lock().await;
    assert_eq!(audit.len(), 0, "no LLM dispatch => no audit row");
}

#[tokio::test]
async fn focus_refuses_without_dataspace_engine() {
    assert_data_plane_refusal(focus("key_insight"), "focus").await;
}

#[tokio::test]
async fn associate_refuses_without_dataspace_engine() {
    assert_data_plane_refusal(associate("A", "B", "id"), "associate").await;
}

#[tokio::test]
async fn aggregate_refuses_without_dataspace_engine() {
    assert_data_plane_refusal(aggregate("events", vec!["region"], "by_region"), "aggregate")
        .await;
}

#[tokio::test]
async fn explore_refuses_without_dataspace_engine() {
    assert_data_plane_refusal(explore("hypothesis_space", Some(5)), "explore").await;
}

#[tokio::test]
async fn ingest_refuses_without_dataspace_engine() {
    assert_data_plane_refusal(ingest("external_api", "raw"), "ingest").await;
}

#[tokio::test]
async fn navigate_emits_step_token_via_pure_shape_core() {
    assert_pure_shape_like(
        navigate("main_pix", "law_corpus", "interpret_clause", "nav_result"),
        "navigate",
    )
    .await;
}

#[tokio::test]
async fn corroborate_emits_step_token_via_pure_shape_core() {
    assert_pure_shape_like(corroborate("nav_result", "validated"), "corroborate").await;
}

// ────────────────────────────────────────────────────────────────────
// section 6 — Cancel propagation across cognitive handlers
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cancel_propagates_into_every_cognitive_handler() {
    let nodes: Vec<IRFlowNode> = vec![
        remember("x", "y"),
        recall("q", "k"),
        forge(),
        focus("x"),
        associate("a", "b", ""),
        aggregate("t", vec![], ""),
        explore("t", None),
        ingest("s", "t"),
        navigate("p", "c", "q", "o"),
        corroborate("n", "o"),
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
// section 7 — Composition with orchestration handlers (33.y.d)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn remember_inside_for_in_per_iter() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("xs".into(), "alpha,beta,gamma".into());

    let for_in = IRFlowNode::ForIn(IRForIn {
        node_type: "for_in",
        source_line: 0,
        source_column: 0,
        variable: "item".into(),
        iterable: "xs".into(),
        body: vec![IRFlowNode::Remember(IRRememberStep {
            node_type: "remember",
            source_line: 0,
            source_column: 0,
            expression: "item".into(),
            memory_target: "last_seen".into(),
        })],
    });
    dispatch_node(&for_in, &mut ctx).await.unwrap();
    // After 3 iterations, last_seen holds the final value.
    assert_eq!(ctx.let_bindings.get("last_seen").unwrap(), "gamma");
}

#[tokio::test]
async fn conditional_with_recall_resolves_via_let_bindings() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("role".into(), "admin".into());

    let cond = IRFlowNode::Conditional(IRConditional {
        node_type: "conditional",
        source_line: 0,
        source_column: 0,
        condition: "role".into(),
        comparison_op: "==".into(),
        comparison_value: "admin".into(),
        then_body: vec![
            IRFlowNode::Recall(IRRecallStep {
                node_type: "recall",
                source_line: 0,
                source_column: 0,
                query: "perms".into(),
                memory_source: "role".into(),
            }),
        ],
        else_body: Vec::new(),
        conditions: Vec::new(),
        conjunctor: String::new(),
        cond: None,
    });
    dispatch_node(&cond, &mut ctx).await.unwrap();
    assert_eq!(ctx.let_bindings.get("perms").unwrap(), "admin");
}

#[tokio::test]
async fn remember_recall_chain_through_multiple_steps() {
    let (mut ctx, _rx) = fresh_ctx();
    dispatch_node(&remember("input-value", "raw"), &mut ctx).await.unwrap();
    dispatch_node(&recall("processed", "raw"), &mut ctx).await.unwrap();
    dispatch_node(&remember("processed", "final"), &mut ctx).await.unwrap();
    assert_eq!(ctx.let_bindings.get("raw").unwrap(), "input-value");
    assert_eq!(ctx.let_bindings.get("processed").unwrap(), "input-value");
    assert_eq!(ctx.let_bindings.get("final").unwrap(), "input-value");
}

// ────────────────────────────────────────────────────────────────────
// section 8 — Step counter discipline across cognitive handlers
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cognitive_handlers_all_advance_step_counter_by_one() {
    let (mut ctx, _rx) = fresh_ctx();
    assert_eq!(ctx.step_counter, 0);
    dispatch_node(&remember("x", "y"), &mut ctx).await.unwrap();
    assert_eq!(ctx.step_counter, 1);
    dispatch_node(&recall("a", "b"), &mut ctx).await.unwrap();
    assert_eq!(ctx.step_counter, 2);
    // v2.41.0 — `forge` is now a composite multi-phase pipeline, so it
    // advances the counter by MORE than 1 (one per LLM phase + the forge
    // wrapper). It is no longer a "advance by exactly 1" cognitive handler;
    // excluded from this simple-handler contract.
    let before_focus = ctx.step_counter;
    // v2.63.0 - focus REFUSES without an engine, but still claims
    // its step index first (the refusal is attributable).
    dispatch_node(&focus("x"), &mut ctx).await.unwrap_err();
    assert_eq!(ctx.step_counter, before_focus + 1);
}
