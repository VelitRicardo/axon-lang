//! §Fase 33.y.b drift gate — assert the `flow_dispatcher` module's
//! exhaustive surface is structurally locked.
//!
//! # §Fase 33.y.l rewrite — ShimReason retired
//!
//! Pre-33.y.l this drift gate used a `ShimReason` enum mirroring
//! the 45-variant IRFlowNode catalog. After 33.y.j reached 45/45
//! graduation (every IRFlowNode variant has a NAMED async handler),
//! 33.y.l retired the entire `legacy_shim` + `ShimReason` +
//! `NodeOutcome::LegacyShimHandled` infrastructure.
//!
//! The drift gate now uses `flow_plan::ir_flow_node_kind` directly
//! as the single source of truth for kind slugs (eliminates the
//! ShimReason::slug duplication that was a drift-gate-anchored
//! contract pre-33.y.l).
//!
//! # What this drift gate exercises
//!
//! - `dispatch_node` produces a NodeOutcome for every one of the
//!   45 IRFlowNode discriminants (no missing arms, no `_ =>`
//!   catch-all).
//! - `dispatch_node` honors the cancel flag at the entry point of
//!   every per-variant handler.
//! - `DispatchCtx::new` produces a usable context across the crate
//!   boundary.
//! - `DispatchError` variants are reachable from downstream
//!   consumers.
//! - The full 45-variant catalog × outcome-shape partition is
//!   exhaustive: every variant falls into one of {pure_shape_like
//!   / orchestration / strict_empty_completed / algebraic_handler}
//!   buckets — no variant is missing, no variant in multiple
//!   buckets.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::{
    dispatch_node, DispatchCtx, DispatchError, NodeOutcome,
};
use axon::flow_plan::ir_flow_node_kind;
use axon::ir_nodes::*;
use tokio::sync::mpsc;

// ────────────────────────────────────────────────────────────────────
//  Synthetic IRFlowNode factory — covers all 45 variants
// ────────────────────────────────────────────────────────────────────
//
// Each factory returns a minimally-shaped IR value that satisfies the
// enum discriminant; the dispatcher in 33.y.b only matches the
// discriminant, so payload contents are irrelevant. Subsequent
// sub-fases extend each factory with realistic payloads as the
// per-variant handler tests need them.

fn step_node() -> IRFlowNode {
    IRFlowNode::Step(IRStep {
        node_type: "step",
        source_line: 0,
        source_column: 0,
        name: String::new(),
        persona_ref: String::new(),
        given: String::new(),
        ask: String::new(),
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

fn grad_node() -> IRFlowNode {
    // §Fase 109 — synthetic shape: NO compile-time derivative (a stale
    // artifact's shape). The handler must fail CLOSED on it.
    IRFlowNode::Grad(IRGradStep {
        node_type: "grad",
        source_line: 0,
        source_column: 0,
        target: "e".into(),
        wrt: vec!["x".into()],
        output: "g".into(),
        original: None,
        derivatives: Vec::new(),
    })
}

fn probe_node() -> IRFlowNode {
    IRFlowNode::Probe(IRProbe {
        node_type: "probe",
        source_line: 0,
        source_column: 0,
        target: String::new(),
    })
}

fn reason_node() -> IRFlowNode {
    IRFlowNode::Reason(IRReasonStep {
        node_type: "reason",
        source_line: 0,
        source_column: 0,
        strategy: String::new(),
        target: String::new(),
        given: String::new(),
        ask: String::new(),
        depth: None,
    })
}

fn validate_node() -> IRFlowNode {
    IRFlowNode::Validate(IRValidateStep {
        node_type: "validate",
        resolved_schema: None,
        guard: None,
        source_line: 0,
        source_column: 0,
        target: String::new(),
        rule: String::new(),
    })
}

fn refine_node() -> IRFlowNode {
    IRFlowNode::Refine(IRRefineStep {
        node_type: "refine",
        source_line: 0,
        source_column: 0,
        target: String::new(),
        strategy: String::new(),
    })
}

fn weave_node() -> IRFlowNode {
    IRFlowNode::Weave(IRWeaveStep {
        node_type: "weave",
        source_line: 0,
        source_column: 0,
        sources: Vec::new(),
        target: String::new(),
        format_type: String::new(),
        priority: Vec::new(),
        style: String::new(),
        include: Vec::new(),
    })
}

fn use_tool_node() -> IRFlowNode {
    IRFlowNode::UseTool(IRUseToolStep {
        node_type: "use_tool",
        source_line: 0,
        source_column: 0,
        tool_name: String::new(),
        argument: String::new(),
        named_args: Vec::new(),
    })
}

fn remember_node() -> IRFlowNode {
    IRFlowNode::Remember(IRRememberStep {
        node_type: "remember",
        source_line: 0,
        source_column: 0,
        expression: String::new(),
        memory_target: String::new(),
    })
}

fn recall_node() -> IRFlowNode {
    IRFlowNode::Recall(IRRecallStep {
        node_type: "recall",
        source_line: 0,
        source_column: 0,
        query: String::new(),
        memory_source: String::new(),
    })
}

fn conditional_node() -> IRFlowNode {
    IRFlowNode::Conditional(IRConditional {
        node_type: "conditional",
        source_line: 0,
        source_column: 0,
        condition: String::new(),
        comparison_op: String::new(),
        comparison_value: String::new(),
        then_body: Vec::new(),
        else_body: Vec::new(),
        conditions: Vec::new(),
        conjunctor: String::new(),
        cond: None,
    })
}

fn for_in_node() -> IRFlowNode {
    IRFlowNode::ForIn(IRForIn {
        node_type: "for_in",
        source_line: 0,
        source_column: 0,
        variable: String::new(),
        iterable: String::new(),
        body: Vec::new(),
    })
}

fn let_node() -> IRFlowNode {
    IRFlowNode::Let(IRLetBinding {
        node_type: "let",
        source_line: 0,
        source_column: 0,
        target: String::new(),
        value: String::new(),
        value_kind: String::new(),
        value_ast: None,
    })
}

fn return_node() -> IRFlowNode {
    IRFlowNode::Return(IRReturnStep {
        node_type: "return",
        source_line: 0,
        source_column: 0,
        value_expr: String::new(),
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

fn lambda_data_apply_node() -> IRFlowNode {
    // §Fase 119.c — the empty name used to complete via the placeholder
    // string; the handler now elevates for real, so the routing fixture
    // names the ΛD that `fresh_ctx` declares in its catalog.
    IRFlowNode::LambdaDataApply(IRLambdaDataApply {
        node_type: "lambda_data_apply",
        source_line: 0,
        source_column: 0,
        lambda_data_name: "RoutedLambda".into(),
        target: "x".into(),
        output_type: String::new(),
    })
}

fn par_node() -> IRFlowNode {
    IRFlowNode::Par(IRParallelBlock {
        node_type: "par",
        source_line: 0,
        source_column: 0,
            branches: Vec::new(),
    })
}

fn hibernate_node() -> IRFlowNode {
    IRFlowNode::Hibernate(IRHibernateStep {
        node_type: "hibernate",
        source_line: 0,
        source_column: 0,
        event_name: "user_input_evt".into(),
        timeout: String::new(),
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

fn forge_node() -> IRFlowNode {
    IRFlowNode::Forge(IRForgeBlock {
        node_type: "forge",
        source_line: 0,
        source_column: 0,
                ..Default::default()
    })
}

fn focus_node() -> IRFlowNode {
    IRFlowNode::Focus(IRFocusStep {
        node_type: "focus",
        source_line: 0,
        source_column: 0,
        expression: String::new(),
        where_expr: String::new(),
        select: Vec::new(),
        output: String::new(),
    })
}

fn associate_node() -> IRFlowNode {
    IRFlowNode::Associate(IRAssociateStep {
        node_type: "associate",
        source_line: 0,
        source_column: 0,
        left: String::new(),
        right: String::new(),
        using_field: String::new(),
        output: String::new(),
    })
}

fn aggregate_node() -> IRFlowNode {
    IRFlowNode::Aggregate(IRAggregateStep {
        node_type: "aggregate",
        source_line: 0,
        source_column: 0,
        target: String::new(),
        group_by: Vec::new(),
        alias: String::new(),
        compute: Vec::new(),
        where_expr: String::new(),
    })
}

fn explore_node() -> IRFlowNode {
    IRFlowNode::Explore(IRExploreStep {
        node_type: "explore",
        source_line: 0,
        source_column: 0,
        target: String::new(),
        limit: None,
        output: String::new(),
    })
}

fn ingest_node() -> IRFlowNode {
    IRFlowNode::Ingest(IRIngestStep {
        node_type: "ingest",
        source_line: 0,
        source_column: 0,
        source: String::new(),
        target: String::new(),
        format: "json".into(),
        max_bytes: None,
        max_rows: None,
    })
}

fn shield_apply_node() -> IRFlowNode {
    IRFlowNode::ShieldApply(IRShieldApplyStep {
        node_type: "shield_apply",
        source_line: 0,
        source_column: 0,
        shield_name: String::new(),
        target: String::new(),
        output_type: String::new(),
    
        breach_policy: None,
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

fn navigate_node() -> IRFlowNode {
    IRFlowNode::Navigate(IRNavigateStep {
        depth: None,
        node_type: "navigate",
        source_line: 0,
        source_column: 0,
        pix_ref: String::new(),
        corpus_ref: String::new(),
        query: String::new(),
        trail_enabled: false,
        output_name: String::new(),
            seed: String::new(),
            budget: None,
            where_expr: String::new(),
    })
}

fn drill_node() -> IRFlowNode {
    IRFlowNode::Drill(IRDrillStep {
        node_type: "drill",
        source_line: 0,
        source_column: 0,
        pix_ref: String::new(),
        subtree_path: String::new(),
        query: String::new(),
        output_name: String::new(),
    })
}

fn trail_node() -> IRFlowNode {
    IRFlowNode::Trail(IRTrailStep {
        node_type: "trail",
        source_line: 0,
        source_column: 0,
        navigate_ref: String::new(),
    })
}

fn corroborate_node() -> IRFlowNode {
    IRFlowNode::Corroborate(IRCorroborateStep {
        node_type: "corroborate",
        source_line: 0,
        source_column: 0,
        navigate_ref: String::new(),
        output_name: String::new(),
    })
}

fn ots_apply_node() -> IRFlowNode {
    // §Fase 119.c — same story as the lambda fixture: the identity
    // passthrough is gone, so the fixture names the declared, REGISTERED
    // ots that `fresh_ctx` sets up.
    IRFlowNode::OtsApply(IROtsApplyStep {
        node_type: "ots_apply",
        source_line: 0,
        source_column: 0,
        ots_name: "RoutedOts".into(),
        target: "x".into(),
        output_type: String::new(),
    })
}

fn mandate_apply_node() -> IRFlowNode {
    // §Fase 119.b — this fixture used to carry an EMPTY mandate name and
    // still complete, because the handler was an identity passthrough. The
    // handler now enforces: an unresolvable mandate REFUSES (which itself
    // proves routing), so the routing fixture names a mandate that the
    // fixture ctx (see `fresh_ctx`) carries in its catalog and that the stub
    // backend's fixed "(stub)" reply satisfies on the free pre-check's first
    // attempt.
    IRFlowNode::MandateApply(IRMandateApplyStep {
        node_type: "mandate_apply",
        source_line: 0,
        source_column: 0,
        mandate_name: "RoutedMandate".into(),
        target: "stub content present".into(),
        output_type: String::new(),
    })
}

fn compute_apply_node() -> IRFlowNode {
    IRFlowNode::ComputeApply(IRComputeApplyStep {
        node_type: "compute_apply",
        source_line: 0,
        source_column: 0,
        compute_name: String::new(),
        arguments: Vec::new(),
        output_name: String::new(),
    })
}

fn listen_node() -> IRFlowNode {
    IRFlowNode::Listen(IRListenStep {
        node_type: "listen",
        source_line: 0,
        source_column: 0,
        channel: String::new(),
        channel_is_ref: false,
        event_alias: String::new(),
        body: Vec::new(),    })
}

fn daemon_step_node() -> IRFlowNode {
    IRFlowNode::DaemonStep(IRDaemonStepNode {
        node_type: "daemon_step",
        source_line: 0,
        source_column: 0,
        daemon_ref: String::new(),
    })
}

fn emit_node() -> IRFlowNode {
    IRFlowNode::Emit(IREmit {
        node_type: "emit",
        source_line: 0,
        source_column: 0,
        channel_ref: String::new(),
        value_ref: String::new(),
        value_is_channel: false,
        shield_ref: String::new(),
    
        breach_policy: None,
    })
}

fn publish_node() -> IRFlowNode {
    IRFlowNode::Publish(IRPublish {
        node_type: "publish",
        source_line: 0,
        source_column: 0,
        channel_ref: String::new(),
        shield_ref: String::new(),
        sign: String::new(),
    })
}

fn discover_node() -> IRFlowNode {
    IRFlowNode::Discover(IRDiscover {
        node_type: "discover",
        source_line: 0,
        source_column: 0,
        capability_ref: String::new(),
        alias: String::new(),
    })
}

fn persist_node() -> IRFlowNode {
    IRFlowNode::Persist(IRPersistStep {
        node_type: "persist",
            fields: Vec::new(),
        source_line: 0,
        source_column: 0,
        store_name: String::new(),
    })
}

fn retrieve_node() -> IRFlowNode {
    IRFlowNode::Retrieve(IRRetrieveStep {
        node_type: "retrieve",
        source_line: 0,
        source_column: 0,
        store_name: String::new(),
        where_expr: String::new(),
        alias: String::new(),
        order_by: String::new(),
        limit_expr: String::new(),
        aggregate: String::new(),
        group_by: String::new(),
        cache: String::new(),
    })
}

fn mutate_node() -> IRFlowNode {
    IRFlowNode::Mutate(IRMutateStep {
        node_type: "mutate",
            fields: Vec::new(),
        source_line: 0,
        source_column: 0,
        store_name: String::new(),
        where_expr: String::new(),
    })
}

fn purge_node() -> IRFlowNode {
    IRFlowNode::Purge(IRPurgeStep {
        node_type: "purge",
        source_line: 0,
        source_column: 0,
        store_name: String::new(),
        where_expr: String::new(),
    })
}

fn transact_node() -> IRFlowNode {
    IRFlowNode::Transact(IRTransactBlock {
        node_type: "transact",
        source_line: 0,
        source_column: 0,
    })
}

// §Fase 88.a — the `warden` adversarial security-analysis block.
fn warden_node() -> IRFlowNode {
    IRFlowNode::Warden(axon::ir_nodes::IRWarden {
        node_type: "warden",
        source_line: 0,
        source_column: 0,
        target: String::new(),
        scope_ref: String::new(),
        body: Vec::new(),
    })
}

// §Fase 51.a — the `quant` cognitive block (Hilbert-space projection).
fn quant_node() -> IRFlowNode {
    IRFlowNode::Quant(IRQuant {
        node_type: "quant",
        source_line: 0,
        source_column: 0,
        encoding: None,
        observable: None,
        qubits: None,
        depth: None,
        bandwidth: None,
        reupload: None,
        effect: "quant_sim".to_string(),
        body: Vec::new(),
    })
}

// §Fase 51.d.2 — the `yield` measurement point.
fn yield_node() -> IRFlowNode {
    IRFlowNode::Yield(IRYield {
        node_type: "yield",
        source_line: 0,
        source_column: 0,
        value_expr: String::new(),
        value_kind: "reference".to_string(),
    })
}

/// The full IRFlowNode catalog the 33.y.b drift gate exercises
/// end-to-end through `dispatch_node`. Each entry: a factory
/// producing a synthetic IR variant + the wire-stable slug
/// `flow_plan::ir_flow_node_kind` returns for that IR variant.
///
/// # §Fase 33.y.l — `ShimReason` retired
///
/// Pre-33.y.l this returned `Vec<(IRFlowNode, ShimReason, &'static str)>`
/// because non-graduated variants routed through `legacy_shim`. After
/// 33.y.j reached 45/45 graduation, ShimReason became dead weight; the
/// kind slug (single source of truth via `ir_flow_node_kind`) is all
/// the drift gate needs.
fn all_48_pairs() -> Vec<(IRFlowNode, &'static str)> {
    vec![
        (step_node(), "step"),
        (probe_node(), "probe"),
        (reason_node(), "reason"),
        (validate_node(), "validate"),
        (refine_node(), "refine"),
        (weave_node(), "weave"),
        (use_tool_node(), "use_tool"),
        (remember_node(), "remember"),
        (recall_node(), "recall"),
        (conditional_node(), "conditional"),
        (for_in_node(), "for_in"),
        (let_node(), "let"),
        (return_node(), "return"),
        (break_node(), "break"),
        (continue_node(), "continue"),
        (lambda_data_apply_node(), "lambda_data_apply"),
        (par_node(), "par"),
        (hibernate_node(), "hibernate"),
        (deliberate_node(), "deliberate"),
        (consensus_node(), "consensus"),
        (forge_node(), "forge"),
        (grad_node(), "grad"),
        (focus_node(), "focus"),
        (associate_node(), "associate"),
        (aggregate_node(), "aggregate"),
        (explore_node(), "explore"),
        (ingest_node(), "ingest"),
        (shield_apply_node(), "shield_apply"),
        (stream_node(), "stream_block"),
        (navigate_node(), "navigate"),
        (drill_node(), "drill"),
        (trail_node(), "trail"),
        (corroborate_node(), "corroborate"),
        (ots_apply_node(), "ots_apply"),
        (mandate_apply_node(), "mandate_apply"),
        (compute_apply_node(), "compute_apply"),
        (listen_node(), "listen"),
        (daemon_step_node(), "daemon_step"),
        (emit_node(), "emit"),
        (publish_node(), "publish"),
        (discover_node(), "discover"),
        (persist_node(), "persist"),
        (retrieve_node(), "retrieve"),
        (mutate_node(), "mutate"),
        (purge_node(), "purge"),
        (transact_node(), "transact"),
        // §Fase 51.a — the 46th variant.
        (quant_node(), "quant"),
        // §Fase 51.d.2 — the 47th variant.
        (yield_node(), "yield"),
        // §Fase 88.a — the 48th variant.
        (warden_node(), "warden"),
    ]
}

/// Construct a fresh DispatchCtx + return the receiver so callers
/// keep it alive (graduated handlers in 33.y.c+ send events to tx;
/// a dropped receiver returns DispatchError::ChannelClosed).
fn fresh_ctx() -> (DispatchCtx, mpsc::UnboundedReceiver<axon::flow_execution_event::FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new(
        "TestFlow",
        "stub",
        "system prompt",
        CancellationFlag::new(),
        tx,
    );
    // §Fase 119.b — the mandate the routing fixture applies. Its constraint
    // is satisfied by the fixture node's literal target, so the routing test
    // observes Completed through the REAL enforcement path (free pre-check,
    // zero attempts) instead of through a passthrough that no longer exists.
    // §Fase 119.c — the ΛD + ots the routing fixtures resolve.
    ctx.lambda_data_specs = std::sync::Arc::new(vec![axon::ir_nodes::IRLambdaData {
        node_type: "lambda_data",
        source_line: 0,
        source_column: 0,
        name: "RoutedLambda".into(),
        ontology: "test.ontology".into(),
        certainty: 0.8,
        temporal_frame_start: String::new(),
        temporal_frame_end: String::new(),
        provenance: "test".into(),
        derivation: "derived".into(),
    }]);
    ctx.ots_specs = std::sync::Arc::new(vec![axon::ir_nodes::IROts {
        node_type: "ots",
        source_line: 0,
        source_column: 0,
        name: "RoutedOts".into(),
        teleology: "route".into(),
        homotopy_search: "shallow".into(),
        loss_function: "L2".into(),
    }]);
    struct RoutedIdentityUpper;
    impl axon::ots_registry::OtsTransformer for RoutedIdentityUpper {
        fn transform(
            &self,
            content: &str,
            _ctx: &axon::ots_registry::OtsTransformContext,
        ) -> axon::ots_registry::OtsVerdict {
            axon::ots_registry::OtsVerdict::Transformed(content.to_uppercase())
        }
    }
    axon::ots_registry::register_ots_transformer(
        "RoutedOts",
        std::sync::Arc::new(RoutedIdentityUpper),
    );
    ctx.mandate_specs = std::sync::Arc::new(vec![axon::ir_nodes::IRMandate {
        node_type: "mandate",
        source_line: 0,
        source_column: 0,
        name: "RoutedMandate".into(),
        constraint: "must contain \"stub\"".into(),
        kp: Some(1.0),
        ki: None,
        kd: None,
        tolerance: Some(0.05),
        max_steps: Some(1),
        drift_bound: None,
        lipschitz: None,
        on_violation: "halt".into(),
    }]);
    (ctx, rx)
}

// ────────────────────────────────────────────────────────────────────
//  §1 — Cardinality + arm coverage
// ────────────────────────────────────────────────────────────────────

#[test]
fn cartesian_product_has_exactly_48_entries() {
    // (§Fase 109 grew the catalog to 49 — the fn name is historical.)
    assert_eq!(
        all_48_pairs().len(),
        49,
        "33.y.b drift gate: the IRFlowNode catalog must cover all 49 \
         variants (§Fase 51.a `quant` + §Fase 51.d.2 `yield`). Adding a 48th \
         IRFlowNode variant fails the dispatch_node compile (forcing a new arm) \
         AND requires updating this drift gate factory + pair list."
    );
}

// ────────────────────────────────────────────────────────────────────
//  §2 — Every variant routes to its labeled handler
// ────────────────────────────────────────────────────────────────────

/// 33.y.l: All 45 IRFlowNode variants have a NAMED async handler in
/// `dispatch_node`. The `legacy_shim` + `ShimReason` +
/// `NodeOutcome::LegacyShimHandled` infrastructure is retired.
///
/// Sub-catalogs below partition the 45 variants by the outcome shape
/// they produce in this drift gate's synthetic-payload tests.
/// Slug strings (wire-stable, single source of truth via
/// `flow_plan::ir_flow_node_kind`) replace the retired ShimReason
/// labels.
const GRADUATED_VARIANTS: &[&str] = &[
    // 33.y.c — pure-shape (6)
    "step", "probe", "reason", "validate", "refine", "weave",
    // 33.y.d — orchestration (6)
    "let", "conditional", "for_in", "break", "continue", "return",
    // 33.y.e — parallel + algebraic (2)
    "par", "stream_block",
    // 33.y.f — cognitive primitives (10)
    "remember", "recall", "forge", "focus", "associate", "aggregate",
    "explore", "ingest", "navigate", "corroborate",
    // 33.y.g — algebraic-effect handlers (6)
    "shield_apply", "ots_apply", "mandate_apply", "compute_apply",
    "listen", "daemon_step",
    // 33.y.h — wire integrations (10)
    "emit", "publish", "discover", "persist", "retrieve", "mutate",
    "purge", "transact", "deliberate", "consensus",
    // 33.y.i — PIX variants (3)
    "hibernate", "drill", "trail",
    // 33.y.j — Lambda + UseTool (45/45 at the close of §33.y)
    "lambda_data_apply", "use_tool",
    // §Fase 51.a — the `quant` cognitive block (46th).
    "quant",
    // §Fase 109 — `grad`, the proof-carrying derivative (49th).
    "grad",
    // §Fase 51.d.2 — the `yield` measurement point (47th).
    "yield",
    // §Fase 88.a — the `warden` adversarial-analysis block (48th).
    "warden",
];

/// Pure-shape graduated variants (33.y.c) — strict "(stub)" + 1 token.
const PURE_SHAPE_GRADUATED: &[&str] =
    &["step", "probe", "reason", "validate", "refine", "weave"];

/// Orchestration graduated variants (33.y.d) — return varied outcome
/// shapes depending on the synthetic IR payload: Let → Completed
/// with the bound value; Conditional/ForIn → Completed (empty body
/// in the synthetic factory); Break → Break sentinel; Continue →
/// LoopContinue sentinel; Return → Return sentinel.
const ORCHESTRATION_GRADUATED: &[&str] =
    &["let", "conditional", "for_in", "break", "continue", "return"];

/// Parallel + algebraic graduated variants (33.y.e) — both
/// `IRParallelBlock` and `IRStreamBlock` are payload-free in
/// v1.25.0; handlers emit canonical wire shape (StepStart +
/// StepComplete with the corresponding step_type slug) +
/// return Completed { output: "", tokens_emitted: 0 }.
const PARALLEL_ALGEBRAIC_GRADUATED: &[&str] = &["par", "stream_block"];

/// Cognitive primitive graduated variants (33.y.f).
///
/// - **Remember/Recall/Forge**: emit wire shape; for the synthetic
///   factory inputs in this drift gate, Remember binds the literal
///   expression as output; Recall returns empty (no prior memory);
///   Forge returns empty (payload-free). All 0 tokens.
/// - **Focus/Associate/Aggregate/Explore/Ingest/Navigate/Corroborate**:
///   route through `pure_shape::run_pure_shape` which calls the
///   stub backend → 1 chunk of "(stub)" → 1 token.
// §Fase 86 — `forge` stays in the graduated set (it routes to a real handler),
// but it is no longer a strict-empty no-op: it runs the Directed Creative
// Synthesis pipeline and, with the stub's identical output, fails closed. The
// `every_ir_flow_node_routes_to_its_labeled_handler` assertion special-cases it
// BEFORE the strict-empty arm.
const COGNITIVE_PEM_BOUND_GRADUATED: &[&str] = &["remember", "recall", "forge"];

const COGNITIVE_FRAMING_GRADUATED: &[&str] = &["navigate", "corroborate"];

/// §Fase 108.a — the five DATA-PLANE verbs. They are relational
/// operations over a declared `dataspace`; an LLM fallthrough would
/// HALLUCINATE their results (the mint/rotate argument verbatim), so
/// with no engine port attached (this gate's `fresh_ctx`) each MUST
/// refuse: `Err(MissingDependency { name: "dataspace_engine" })`,
/// AFTER emitting its attributable StepStart. The real handlers land
/// in §108.c (ingest) / §108.d (the query verbs).
const DATA_PLANE_GRADUATED: &[&str] =
    &["focus", "associate", "aggregate", "explore", "ingest"];

/// §Fase 109 — `grad` evaluates a COMPILE-TIME derivative. The synthetic
/// factory carries none (a stale artifact's shape), so the handler must
/// fail CLOSED with the structured `grad` error — the refusal IS the
/// routing proof (the mint/rotate/§108.a posture).
const GRAD_GRADUATED: &[&str] = &["grad"];

/// Algebraic-effect handler graduated variants (33.y.g) — apply
/// capability + invoke / listen / daemon. All emit wire shape +
/// bind output under a variant-specific key in `let_bindings`.
/// For zero-payload synthetic inputs in the drift gate, outputs
/// are sensible defaults (empty/placeholder).
const ALGEBRAIC_HANDLERS_GRADUATED: &[&str] = &[
    "shield_apply", "ots_apply", "mandate_apply", "listen",
    "daemon_step",
];

/// §Fase 111.f — `compute_apply` REFUSES without a declared compute. It used to
/// sit in ALGEBRAIC_HANDLERS_GRADUATED and "complete" by binding the literal
/// string "compute:Name(args)" — which a downstream step then read as a number.
/// The refusal IS the routing proof (the §108.a / mint / rotate posture).
const COMPUTE_FAILCLOSED: &[&str] = &["compute_apply"];

/// Wire-integration graduated variants (33.y.h) — π-calc typed
/// channels + persistence primitives + multi-agent deliberation.
/// All emit wire shape + various placeholder/persisted outputs;
/// tokens_emitted=0 across the board (no LLM dispatch).
const WIRE_INTEGRATIONS_GRADUATED: &[&str] = &[
    "emit", "publish", "discover", "persist", "retrieve", "mutate", "purge",
    "transact", "deliberate", "consensus",
];

/// PIX graduated variants (33.y.i) — Hibernate / Drill / Trail.
/// All emit canonical wire shape + bind placeholder/seeded value
/// under variant-specific keys. Tokens=0 across the board.
const PIX_GRADUATED: &[&str] = &["hibernate", "drill", "trail"];

/// Lambda + UseTool graduated variants (33.y.j) — the FINAL 2.
/// LambdaDataApply uses public helper `apply_lambda_data` (Fase 15
/// CPS dispatcher); UseTool uses `invoke_tool` (Fase 22 tool
/// registry). Both emit canonical wire shape + bind result under
/// variant-specific key. Tokens=0.
const LAMBDA_TOOLS_GRADUATED: &[&str] = &["lambda_data_apply", "use_tool"];

/// §Fase 51.a — the `quant` cognitive block. SURFACE only: `run_quant`
/// emits the canonical `step_type: "quant"` wire shape and returns
/// `Completed { output: "", tokens_emitted: 0 }` (no Hilbert execution
/// in the OSS runtime). Routed via the `algebraic_handler` bucket (it IS
/// an algebraic-effect block, D9) which asserts only `tokens_emitted == 0`.
/// §Fase 51.d.2 adds `yield` (the measurement point) to the same surface-only
/// bucket — `run_yield` likewise returns `Completed { tokens_emitted: 0 }`.
/// §Fase 111.d — `quant` and `yield` are no longer surface-only either. They
/// used to "complete" with an empty output while the block's body was SILENTLY
/// SKIPPED and no amplitude was ever collapsed.
///
/// Both now REFUSE without their port, and the refusal IS the routing proof:
///   `quant` — MissingDependency { quant_backend }
///   `yield` — BackendError { yield }: a measurement outside a `quant` block has
///             no Hilbert space to collapse into, and returning 0.0 would be
///             indistinguishable from a genuine expectation of zero.
const QUANT_GRADUATED: &[&str] = &[];

/// §Fase 111.d — `quant` + `yield`, now fail-closed. Kept as their own counted
/// sub-catalog so the exhaustive-partition check still covers every variant: a
/// primitive that moves from "completes empty" to "refuses" must not silently
/// fall OUT of the census — that is how a no-op hides.
const QUANT_FAILCLOSED: &[&str] = &["quant", "yield"];

/// §Fase 111.c — `warden` is no longer surface-only. It used to sit in
/// QUANT_GRADUATED and "complete" with an empty output — which is exactly the
/// defect §111 found: an analysis that never ran was indistinguishable from an
/// analysis that found nothing.
///
/// It now REFUSES without its engine port, and the refusal IS the routing proof
/// (the §108.a / mint / rotate posture). A security primitive must never report
/// a clean bill of health for a target it never opened.
const WARDEN_GRADUATED: &[&str] = &["warden"];

#[tokio::test]
async fn every_ir_flow_node_routes_to_its_labeled_handler() {
    let pairs = all_48_pairs();
    for (node, expected_kind) in pairs {
        // The kind slug from `ir_flow_node_kind` is the single source
        // of truth (eliminates the ShimReason::slug duplication that
        // existed pre-33.y.l). Drift gate pair list ↔ flow_plan slug
        // are byte-equal by construction.
        let actual_kind = ir_flow_node_kind(&node);
        assert_eq!(
            actual_kind, expected_kind,
            "drift gate pair list disagrees with flow_plan::ir_flow_node_kind",
        );

        let (mut ctx, _rx) = fresh_ctx();
        let outcome = dispatch_node(&node, &mut ctx).await;

        let pure_shape = PURE_SHAPE_GRADUATED.contains(&expected_kind);
        let orchestration = ORCHESTRATION_GRADUATED.contains(&expected_kind);
        let parallel_algebraic =
            PARALLEL_ALGEBRAIC_GRADUATED.contains(&expected_kind);
        let cognitive_pem = COGNITIVE_PEM_BOUND_GRADUATED.contains(&expected_kind);
        let cognitive_framing = COGNITIVE_FRAMING_GRADUATED.contains(&expected_kind);
        let algebraic_handler = ALGEBRAIC_HANDLERS_GRADUATED.contains(&expected_kind)
            || WIRE_INTEGRATIONS_GRADUATED.contains(&expected_kind)
            || PIX_GRADUATED.contains(&expected_kind)
            || LAMBDA_TOOLS_GRADUATED.contains(&expected_kind)
            // §Fase 51.a — quant routes here (Completed, 0 tokens).
            || QUANT_GRADUATED.contains(&expected_kind);
        let _ = QUANT_FAILCLOSED; // handled by its own fail-closed arms below
        let _ = COMPUTE_FAILCLOSED;
        let _ = WARDEN_GRADUATED; // handled by its own fail-closed arm below

        // Cognitive-framing handlers behave like pure-shape (1 token).
        let pure_shape_like = pure_shape || cognitive_framing;
        // Parallel/algebraic + cognitive-PEM-bound return Completed
        // with output="" (truly payload-free). Algebraic handlers +
        // wire integrations return Completed with placeholder output
        // (compute:(...), (awaiting ...), daemon:..., "persisted N
        // entries to `s`", etc.) so they need a separate arm that
        // doesn't assert output equality.
        let strict_empty_completed = parallel_algebraic || cognitive_pem;

        // §Fase 86 — `forge` runs the real creative pipeline; with the stub's
        // identical output it correctly fails closed (novelty floor breached).
        // That IS the node routing to its handler and enforcing its guarantee.
        if expected_kind == "forge" {
            // Routing check only: `forge` reaches its handler and runs the real
            // pipeline. Either it clears the (tiny-input) novelty floor and
            // Completes, or it fails closed with a structured `forge:` error —
            // both are valid routing; the forge-logic assertions live in the
            // `forge` + `cognitive::run_forge` unit tests.
            match outcome {
                Ok(NodeOutcome::Completed { .. }) => {}
                Err(axon::flow_dispatcher::DispatchError::BackendError { name, .. })
                    if name == "forge" => {}
                other => panic!("forge routed to an unexpected outcome: {other:?}"),
            }
            continue;
        }

        // §Fase 109 — grad with no compile-time derivative (the synthetic
        // stale shape) fails CLOSED with the structured `grad` error.
        if GRAD_GRADUATED.contains(&expected_kind) {
            match outcome {
                Err(axon::flow_dispatcher::DispatchError::BackendError { ref name, .. })
                    if name == "grad" => {}
                other => panic!(
                    "grad must fail CLOSED on a stale shape (BackendError name=grad),                      got {other:?}",
                ),
            }
            continue;
        }

        // §Fase 111.f — `compute_apply` refuses without a declared compute.
        if COMPUTE_FAILCLOSED.contains(&expected_kind) {
            match outcome {
                Err(axon::flow_dispatcher::DispatchError::BackendError { ref name, .. })
                    if name == "compute" => {}
                other => panic!(
                    "compute_apply must fail CLOSED with no declared compute — binding a                      placeholder string is what §111 F10 was, got {other:?}",
                ),
            }
            continue;
        }

        // §Fase 111.d — `quant` refuses without its simulator; `yield` refuses
        // outside a quant frame. Both used to "complete" empty.
        if expected_kind == "quant" {
            match outcome {
                Err(axon::flow_dispatcher::DispatchError::MissingDependency {
                    name: "quant_backend",
                }) => {}
                other => panic!("quant must fail CLOSED without its simulator, got {other:?}"),
            }
            continue;
        }
        if expected_kind == "yield" {
            match outcome {
                Err(axon::flow_dispatcher::DispatchError::BackendError { ref name, .. })
                    if name == "yield" => {}
                other => panic!(
                    "a bare `yield` (no enclosing quant frame) must fail CLOSED, got {other:?}",
                ),
            }
            continue;
        }

        // §Fase 111.c — `warden` REFUSES without its engine port. Pre-111 it
        // "completed" with an empty output, so a reader could not tell an audit
        // that found nothing from an audit that never ran.
        if WARDEN_GRADUATED.contains(&expected_kind) {
            match outcome {
                Err(axon::flow_dispatcher::DispatchError::MissingDependency {
                    name: "warden_backend",
                }) => {}
                other => panic!(
                    "warden must fail CLOSED without its engine                      (MissingDependency: warden_backend), got {other:?}",
                ),
            }
            continue;
        }

        // §Fase 108.a — the data-plane verbs REFUSE without an engine port
        // (this gate's ctx attaches none). The refusal IS the routing proof:
        // pre-108.a these five prompted the stub backend and "completed"
        // with narrated output — the hallucination the honesty floor ended.
        if DATA_PLANE_GRADUATED.contains(&expected_kind) {
            match outcome {
                Err(axon::flow_dispatcher::DispatchError::MissingDependency {
                    name: "dataspace_engine",
                }) => {}
                other => panic!(
                    "data-plane variant {expected_kind:?} must fail CLOSED without \
                     the engine (MissingDependency: dataspace_engine), got {other:?}",
                ),
            }
            continue;
        }

        match (outcome, pure_shape_like, orchestration, strict_empty_completed, algebraic_handler) {
            // §Fase 119.d — hibernate routes to its handler and yields the
            // SUSPENSION outcome (the walk loop parks + halts). Reaching
            // Hibernated IS proof of routing; the old Completed-with-
            // placeholder was the F20 defect this arm replaces.
            (Ok(NodeOutcome::Hibernated { ref event_name, .. }), _, _, _, _)
                if expected_kind == "hibernate" =>
            {
                assert_eq!(event_name, "user_input_evt");
            }
            // Pure-shape: stub-backend produces "(stub)" with 1 token.
            (Ok(NodeOutcome::Completed { output, tokens_emitted, .. }), true, _, _, _) => {
                assert_eq!(
                    output, "(stub)",
                    "pure-shape variant {expected_kind:?} stub output should be '(stub)' (D4)",
                );
                assert_eq!(
                    tokens_emitted, 1,
                    "pure-shape variant {expected_kind:?} stub emits 1 token (D4 byte-compat)",
                );
            }
            // Orchestration: Break/Continue/Return are sentinel-only;
            // Let/Conditional/ForIn return Completed with payload-
            // dependent output (zero-payload synthetics → empty body
            // → empty output).
            (Ok(NodeOutcome::Break), _, true, _, _) if expected_kind == "break" => {}
            (Ok(NodeOutcome::LoopContinue), _, true, _, _)
                if expected_kind == "continue" => {}
            (Ok(NodeOutcome::Return { .. }), _, true, _, _)
                if expected_kind == "return" => {}
            (Ok(NodeOutcome::Completed { .. }), _, true, _, _)
                if matches!(expected_kind, "let" | "conditional" | "for_in") => {}
            // Strict empty (Par + Stream + Remember + Recall + Forge):
            // Completed with output="", tokens=0.
            (
                Ok(NodeOutcome::Completed {
                    output,
                    tokens_emitted,
                    ..
                }),
                _,
                _,
                true,
                _,
            ) => {
                assert_eq!(
                    output, "",
                    "33.y.e/f strict-empty variant {expected_kind:?} → empty output",
                );
                assert_eq!(
                    tokens_emitted, 0,
                    "33.y.e/f strict-empty variant {expected_kind:?} → 0 tokens",
                );
            }
            // 33.y.g/h/i/j algebraic + wire + PIX + lambda/tool handlers:
            // Completed with placeholder outputs. tokens_emitted=0 always.
            (
                Ok(NodeOutcome::Completed { tokens_emitted, .. }),
                _,
                _,
                _,
                true,
            ) => {
                assert_eq!(
                    tokens_emitted, 0,
                    "algebraic handler variant {expected_kind:?} → 0 tokens",
                );
            }
            (Ok(other), true, _, _, _) => panic!(
                "pure-shape variant {expected_kind:?} expected Completed (stub), got {other:?}",
            ),
            (Ok(other), _, true, _, _) => panic!(
                "orchestration variant {expected_kind:?} unexpected outcome: {other:?}",
            ),
            (Ok(other), _, _, true, _) => panic!(
                "strict-empty variant {expected_kind:?} expected Completed (empty), got {other:?}",
            ),
            (Ok(other), _, _, _, true) => panic!(
                "algebraic handler variant {expected_kind:?} expected Completed, got {other:?}",
            ),
            (Ok(other), false, false, false, false) => panic!(
                "33.y.l invariant: variant {expected_kind:?} is in no graduated \
                 sub-catalog — 45/45 graduation contract broken. Outcome: {other:?}",
            ),
            (Err(e), _, _, _, _) => panic!(
                "dispatch_node returned Err for {expected_kind:?}: {e:?}",
            ),
        }
    }
}

/// Pin the size of the graduated-variants set. Across 33.y.c–j this
/// set GREW variant-by-variant; 33.y.j sealed it at 45 / 45 and
/// 33.y.l retired `legacy_shim` + `ShimReason` +
/// `NodeOutcome::LegacyShimHandled` in lockstep. From 33.y.l onward
/// the invariant is structural: any new IRFlowNode variant fails the
/// `dispatch_node` compile (exhaustive match) AND requires growing
/// this set + a new entry in one of the sub-catalogs below.
#[test]
fn graduated_variants_set_size_pinned_48_of_48() {
    assert_eq!(
        GRADUATED_VARIANTS.len(),
        49,
        "48 / 48 graduated. All IRFlowNode variants have a NAMED \
         async handler in dispatch_node; `legacy_shim` retired in \
         33.y.l. Composition: 6 pure-shape (33.y.c) + 6 orchestration \
         (33.y.d) + 2 parallel/algebraic (33.y.e) + 10 cognitive \
         (33.y.f) + 6 algebraic handlers (33.y.g) + 10 wire \
         integrations (33.y.h) + 3 PIX (33.y.i) + 2 Lambda+UseTool \
         (33.y.j) + 3 quant+yield+warden (§51.a/§51.d.2/§88.a) + 1 grad (§109) = 49 variants total."
    );
    assert_eq!(PURE_SHAPE_GRADUATED.len(), 6);
    assert_eq!(ORCHESTRATION_GRADUATED.len(), 6);
    assert_eq!(PARALLEL_ALGEBRAIC_GRADUATED.len(), 2);
    assert_eq!(COGNITIVE_PEM_BOUND_GRADUATED.len(), 3);
    // §Fase 108.a — the five data-plane verbs left the cognitive-framing
    // (pure-shape/LLM) bucket for the fail-closed DATA_PLANE bucket.
    assert_eq!(COGNITIVE_FRAMING_GRADUATED.len(), 2);
    assert_eq!(DATA_PLANE_GRADUATED.len(), 5);
    assert_eq!(GRAD_GRADUATED.len(), 1);
    assert_eq!(ALGEBRAIC_HANDLERS_GRADUATED.len(), 5);
    assert_eq!(COMPUTE_FAILCLOSED.len(), 1);
    assert_eq!(WIRE_INTEGRATIONS_GRADUATED.len(), 10);
    assert_eq!(PIX_GRADUATED.len(), 3);
    assert_eq!(LAMBDA_TOOLS_GRADUATED.len(), 2);
    assert_eq!(QUANT_GRADUATED.len(), 0);
    assert_eq!(QUANT_FAILCLOSED.len(), 2);
    assert_eq!(WARDEN_GRADUATED.len(), 1);

    // Sum check — the partition is exhaustive (no variant in
    // multiple groups; no variant missing).
    let total = PURE_SHAPE_GRADUATED.len()
        + ORCHESTRATION_GRADUATED.len()
        + PARALLEL_ALGEBRAIC_GRADUATED.len()
        + COGNITIVE_PEM_BOUND_GRADUATED.len()
        + COGNITIVE_FRAMING_GRADUATED.len()
        + DATA_PLANE_GRADUATED.len()
        + ALGEBRAIC_HANDLERS_GRADUATED.len()
        + COMPUTE_FAILCLOSED.len()
        + WIRE_INTEGRATIONS_GRADUATED.len()
        + PIX_GRADUATED.len()
        + LAMBDA_TOOLS_GRADUATED.len()
        + QUANT_GRADUATED.len()
        + QUANT_FAILCLOSED.len()
        + WARDEN_GRADUATED.len()
        + GRAD_GRADUATED.len();
    assert_eq!(
        total, 49,
        "partition sum check: all 12 sub-catalogs must cover exactly 49 variants"
    );
}

// ────────────────────────────────────────────────────────────────────
//  §3 — Cancel propagation through dispatch_node entry
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_node_honors_cancel_flag_at_entry() {
    let pairs = all_48_pairs();
    for (node, kind) in pairs {
        let (tx, _rx) = mpsc::unbounded_channel();
        let cancel = CancellationFlag::new();
        cancel.cancel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);
        let outcome = dispatch_node(&node, &mut ctx).await;
        match outcome {
            Err(DispatchError::UpstreamCancelled) => {} // expected
            other => panic!(
                "expected UpstreamCancelled for {kind} with cancel set, got {other:?}"
            ),
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  §4 — flow_plan::ir_flow_node_kind covers every variant
// ────────────────────────────────────────────────────────────────────
//
// Pre-33.y.l this section asserted byte-equality between
// `ShimReason::slug()` and `ir_flow_node_kind`. With ShimReason
// retired, `ir_flow_node_kind` is the single source of truth — the
// only invariant left is that it returns a non-empty slug for every
// of the 45 variants. Drift between this drift gate's pair list and
// `ir_flow_node_kind` is exercised inline by
// `every_ir_flow_node_routes_to_its_labeled_handler`.

#[test]
fn flow_plan_kind_returns_non_empty_slug_for_all_45_variants() {
    let pairs = all_48_pairs();
    let mut seen = std::collections::HashSet::new();
    for (node, expected_kind) in pairs {
        let kind = ir_flow_node_kind(&node);
        assert!(
            !kind.is_empty(),
            "flow_plan::ir_flow_node_kind returned empty slug for the IR \
             variant expected as {expected_kind:?}",
        );
        assert_eq!(
            kind, expected_kind,
            "drift gate pair list disagrees with flow_plan::ir_flow_node_kind",
        );
        assert!(
            seen.insert(kind),
            "duplicate slug {kind:?} — flow_plan::ir_flow_node_kind must \
             be 1-to-1 with IRFlowNode variants",
        );
    }
    assert_eq!(seen.len(), 49, "slugs cover all 49 variants exactly once");
}

// ────────────────────────────────────────────────────────────────────
//  §5 — DispatchCtx surface usable across crate boundary
// ────────────────────────────────────────────────────────────────────

#[test]
fn dispatch_ctx_branch_path_round_trip() {
    let (mut ctx, _rx) = fresh_ctx();
    assert_eq!(ctx.branch_path_string(), "");
    ctx.branch_path.push("par[0]".to_string());
    assert_eq!(ctx.branch_path_string(), "par[0]");
    ctx.branch_path.push("step[3]".to_string());
    assert_eq!(ctx.branch_path_string(), "par[0].step[3]");
    ctx.branch_path.pop();
    assert_eq!(ctx.branch_path_string(), "par[0]");
}

#[test]
fn dispatch_ctx_step_counter_writable() {
    let (mut ctx, _rx) = fresh_ctx();
    assert_eq!(ctx.step_counter, 0);
    ctx.step_counter += 1;
    assert_eq!(ctx.step_counter, 1);
}

// ────────────────────────────────────────────────────────────────────
//  §6 — DispatchError variants are reachable from the public surface
// ────────────────────────────────────────────────────────────────────

#[test]
fn dispatch_error_variants_constructible_from_public_surface() {
    let _be = DispatchError::BackendError {
        name: "anthropic".to_string(),
        message: "rate limited".to_string(),
    };
    let _uc = DispatchError::UpstreamCancelled;
    // 33.y.l: `DispatchError::LegacyShimFailed` retired along with
    // `legacy_shim` + `ShimReason` + `NodeOutcome::LegacyShimHandled`.
    let _md = DispatchError::MissingDependency { name: "pem_async" };
    let _cc = DispatchError::ChannelClosed;
    // If this compiles, the variants are all reachable.
}

// ────────────────────────────────────────────────────────────────────
//  §7 — D7 invariant: zero `unimplemented!()` / `todo!()` markers
// ────────────────────────────────────────────────────────────────────
//
// This test verifies (at the type level) that calling dispatch_node
// on every IRFlowNode variant does NOT panic. If a future maintainer
// drops an `unimplemented!()` into an arm, this test surfaces the
// panic at PR time, not at adopter runtime.

#[tokio::test]
async fn dispatch_node_does_not_panic_for_any_variant() {
    let pairs = all_48_pairs();
    for (node, kind) in pairs {
        let (mut ctx, _rx) = fresh_ctx();
        // If this `await` panics for any variant we've shipped a
        // `unimplemented!()` / `todo!()` / `panic!()` into an arm.
        // The D7 mandate forbids this.
        let outcome = dispatch_node(&node, &mut ctx).await;
        // We only assert it didn't panic — the specific outcome
        // shape is exercised by §2 above.
        assert!(
            outcome.is_ok() || outcome.is_err(),
            "outcome for {kind} is somehow neither Ok nor Err — \
             impossible per Rust's type system but asserted for sanity"
        );
    }
}
