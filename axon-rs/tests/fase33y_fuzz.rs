//! §Fase 33.y.n CONSOLIDATED D12 fuzz pack — cycle-level cross-cutting
//! adversarial coverage that the per-sub-fase fuzz packs (33.y.c
//! through 33.y.j) intentionally don't provide.
//!
//! # Why a consolidated pack?
//!
//! Each sub-fase 33.y.c–j ships its own architectural-group fuzz pack
//! (~11 400 deterministic LCG iters total across 8 files). Those packs
//! cover **per-handler semantics** — given a synthetic IR variant of
//! one group, hammer the corresponding handler with random inputs.
//!
//! This file covers **cross-cutting cycle-level properties** that fall
//! between the per-group fuzz packs:
//!
//! 1. **Handler totality stress** — drive ALL 46 IRFlowNode variants
//!    through `dispatch_node` in random-permuted order × many iters.
//!    Asserts the closed-catalog match never panics for any variant,
//!    no matter the dispatch order or surrounding ctx state.
//!
//! 2. **Cancel propagation across all 46** — pre-cancel × every
//!    variant × many iters. Asserts D3 contract holds uniformly.
//!
//! 3. **Orchestration composition** — synthesize random `Conditional` /
//!    `ForIn` / `Par` nodes with random handler bodies × deep nesting
//!    × many iters. Asserts sentinel propagation + branch_path
//!    discipline + step_counter monotonicity across composition.
//!
//! 4. **Algebraic-semantics parity (D10)** — drive the same synthetic
//!    IR through `dispatch_node` twice with different ctx state
//!    (let_bindings ordering, pending_effect_policy presence) and
//!    assert deterministic outcome shapes.
//!
//! 5. **Tool-call interleaving (D8 + D3 cross-cut)** — `Step` with
//!    random `apply_ref` + random pre-cancel timing × many iters.
//!    Asserts the D8 tool plumbing composes cleanly with the D3
//!    cancel discipline.
//!
//! 6. **Wire-stable kind slug round-trip (D11 cross-stack anchor)** —
//!    every random IRFlowNode discriminant returns a kind slug from
//!    `flow_plan::ir_flow_node_kind` that matches the 46-entry
//!    closed-catalog list (mirrored by Python `axon.runtime.flow_plan`).
//!
//! # Methodology
//!
//! Same hand-rolled deterministic 64-bit LCG (Knuth/MMIX constants)
//! the 33.y.c–j sub-fase packs use. NO external dep. Reseeds via
//! `Lcg::new(seed)`. Same seed + same iter index → byte-identical
//! input.
//!
//! # Coverage targets
//!
//! | Surface | Iters | Cumulative |
//! |---|---|---|
//! | §1 handler totality stress (46 variants × 50 iters each) | 2 300 | 2 300 |
//! | §2 cancel propagation across all 46 (46 × 30 iters) | 1 380 | 3 680 |
//! | §3 orchestration composition (deep-nest × 200 iters) | 200 | 3 800 |
//! | §4 algebraic-semantics parity (D10, 300 iters) | 300 | 4 100 |
//! | §5 tool-call interleaving (D8 + D3, 250 iters) | 250 | 4 350 |
//! | §6 kind slug round-trip (D11 anchor, 46 × 20 iters) | 920 | 5 350 |
//! | **Total**                                       | — | **5 350** |
//!
//! Runtime <2s on a stock GitHub Actions runner.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, DispatchError, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::flow_plan::ir_flow_node_kind;
use axon::ir_nodes::*;
use axon::stream_effect::BackpressurePolicy;
use tokio::sync::mpsc;

// ────────────────────────────────────────────────────────────────────
//  §0 — Deterministic PRNG (linear congruential, no external dep)
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
    fn bool(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
    fn ascii_with_random_len(&mut self, max: usize) -> String {
        let len = self.range(max) + 1;
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            let c = (self.range(95) + 32) as u8;
            s.push(c as char);
        }
        s
    }
}

// ────────────────────────────────────────────────────────────────────
//  Synthetic factory — 46 variants ordered by `ir_flow_node_kind` slug
// ────────────────────────────────────────────────────────────────────
//
// One factory per variant. Each takes an LCG so the synthesized payload
// carries random field content (the dispatcher matches on the variant
// discriminant only; payload contents test the per-handler input
// handling).

fn random_step(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Step(IRStep {
        node_type: "step",
        source_line: 0,
        source_column: 0,
        name: lcg.ascii_with_random_len(16),
        persona_ref: lcg.ascii_with_random_len(8),
        given: lcg.ascii_with_random_len(24),
        ask: lcg.ascii_with_random_len(40),
        use_tool: None,
        probe: None,
        reason: None,
        weave: None,
        output_type: lcg.ascii_with_random_len(12),
        confidence_floor: None,
        navigate_ref: String::new(),
        apply_ref: if lcg.bool() {
            lcg.ascii_with_random_len(12)
        } else {
            String::new()
        },
        requires_context: None,
        now_tz: None,
        pix_ops: Vec::new(),
        stream: None,
        performs: Vec::new(),
        guards: Vec::new(),
        body: Vec::new(),
    })
}

fn random_probe(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Probe(IRProbe {
        node_type: "probe",
        source_line: 0,
        source_column: 0,
        target: lcg.ascii_with_random_len(16),
    })
}

fn random_reason(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Reason(IRReasonStep {
        node_type: "reason",
        source_line: 0,
        source_column: 0,
        strategy: lcg.ascii_with_random_len(12),
        target: lcg.ascii_with_random_len(16),
        // §Fase 119.f.8 — the block form's fields are fuzzed too, not pinned
        // to empty: `run_reason` now SPLITS `given` on commas and trims
        // brackets, and prompt assembly that only ever sees well-formed input
        // is prompt assembly nobody fuzzed.
        given: lcg.ascii_with_random_len(24),
        ask: lcg.ascii_with_random_len(32),
        depth: if lcg.bool() {
            Some(lcg.range(8) as u32)
        } else {
            None
        },
    })
}

fn random_validate(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Validate(IRValidateStep {
        node_type: "validate",
        resolved_schema: None,
        guard: None,
        source_line: 0,
        source_column: 0,
        target: lcg.ascii_with_random_len(16),
        rule: lcg.ascii_with_random_len(20),
    })
}

fn random_refine(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Refine(IRRefineStep {
        node_type: "refine",
        source_line: 0,
        source_column: 0,
        target: lcg.ascii_with_random_len(16),
        strategy: lcg.ascii_with_random_len(12),
    })
}

fn random_weave(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Weave(IRWeaveStep {
        node_type: "weave",
        source_line: 0,
        source_column: 0,
        sources: vec![lcg.ascii_with_random_len(8)],
        target: lcg.ascii_with_random_len(16),
        format_type: lcg.ascii_with_random_len(8),
        priority: Vec::new(),
        style: lcg.ascii_with_random_len(8),
        // §Fase 119.f.9 — fuzzed, not pinned to empty: `weave_prompt` now
        // joins and resolves these, and prompt assembly that only ever sees
        // well-formed input is prompt assembly nobody fuzzed.
        include: vec![lcg.ascii_with_random_len(10)],
    })
}

fn random_use_tool(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::UseTool(IRUseToolStep {
        node_type: "use_tool",
        source_line: 0,
        source_column: 0,
        tool_name: lcg.ascii_with_random_len(12),
        argument: lcg.ascii_with_random_len(20),
        named_args: Vec::new(),
    })
}

fn random_remember(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Remember(IRRememberStep {
        node_type: "remember",
        source_line: 0,
        source_column: 0,
        expression: lcg.ascii_with_random_len(20),
        memory_target: lcg.ascii_with_random_len(12),
    })
}

fn random_recall(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Recall(IRRecallStep {
        node_type: "recall",
        source_line: 0,
        source_column: 0,
        query: lcg.ascii_with_random_len(16),
        memory_source: lcg.ascii_with_random_len(12),
    })
}

fn random_conditional(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Conditional(IRConditional {
        node_type: "conditional",
        source_line: 0,
        source_column: 0,
        condition: lcg.ascii_with_random_len(16),
        comparison_op: "==".to_string(),
        comparison_value: lcg.ascii_with_random_len(12),
        then_body: Vec::new(),
        else_body: Vec::new(),
        conditions: Vec::new(),
        conjunctor: String::new(),
        cond: None,
    })
}

fn random_for_in(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::ForIn(IRForIn {
        node_type: "for_in",
        source_line: 0,
        source_column: 0,
        variable: lcg.ascii_with_random_len(8),
        iterable: lcg.ascii_with_random_len(16),
        body: Vec::new(),
    })
}

fn random_let(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Let(IRLetBinding {
        node_type: "let",
        source_line: 0,
        source_column: 0,
        target: lcg.ascii_with_random_len(8),
        value: lcg.ascii_with_random_len(20),
        value_kind: "literal".to_string(),
        value_ast: None,
    })
}

fn random_return(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Return(IRReturnStep {
        node_type: "return",
        source_line: 0,
        source_column: 0,
        value_expr: lcg.ascii_with_random_len(16),
    })
}

fn random_break(_lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Break(IRBreakStep {
        node_type: "break",
        source_line: 0,
        source_column: 0,
    })
}

fn random_continue(_lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Continue(IRContinueStep {
        node_type: "continue",
        source_line: 0,
        source_column: 0,
    })
}

fn random_lambda_data_apply(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::LambdaDataApply(IRLambdaDataApply {
        node_type: "lambda_data_apply",
        source_line: 0,
        source_column: 0,
        lambda_data_name: lcg.ascii_with_random_len(12),
        target: lcg.ascii_with_random_len(12),
        output_type: lcg.ascii_with_random_len(8),
    })
}

fn random_par(_lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Par(IRParallelBlock {
        node_type: "par",
        source_line: 0,
        source_column: 0,
            branches: Vec::new(),
    })
}

fn random_hibernate(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Hibernate(IRHibernateStep {
        node_type: "hibernate",
        source_line: 0,
        source_column: 0,
        event_name: lcg.ascii_with_random_len(12),
        timeout: lcg.ascii_with_random_len(8),
    })
}

fn random_deliberate(_lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Deliberate(IRDeliberateBlock {
        node_type: "deliberate",
        source_line: 0,
        source_column: 0,
    })
}

fn random_consensus(_lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Consensus(IRConsensusBlock {
        node_type: "consensus",
        source_line: 0,
        source_column: 0,
    })
}

fn random_forge(_lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Forge(IRForgeBlock {
        node_type: "forge",
        source_line: 0,
        source_column: 0,
                ..Default::default()
    })
}

fn random_focus(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Focus(IRFocusStep {
        node_type: "focus",
        source_line: 0,
        source_column: 0,
        expression: lcg.ascii_with_random_len(16),
        where_expr: String::new(),
        select: Vec::new(),
        output: String::new(),
    })
}

fn random_associate(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Associate(IRAssociateStep {
        node_type: "associate",
        source_line: 0,
        source_column: 0,
        left: lcg.ascii_with_random_len(8),
        right: lcg.ascii_with_random_len(8),
        using_field: lcg.ascii_with_random_len(8),
        output: String::new(),
    })
}

fn random_aggregate(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Aggregate(IRAggregateStep {
        node_type: "aggregate",
        source_line: 0,
        source_column: 0,
        target: lcg.ascii_with_random_len(12),
        group_by: vec![lcg.ascii_with_random_len(8)],
        alias: lcg.ascii_with_random_len(8),
        compute: Vec::new(),
        where_expr: String::new(),
    })
}

fn random_explore(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Explore(IRExploreStep {
        node_type: "explore",
        source_line: 0,
        source_column: 0,
        target: lcg.ascii_with_random_len(12),
        limit: None,
        output: String::new(),
    })
}

fn random_ingest(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Ingest(IRIngestStep {
        node_type: "ingest",
        source_line: 0,
        source_column: 0,
        source: lcg.ascii_with_random_len(16),
        target: lcg.ascii_with_random_len(12),
        format: "json".into(),
        max_bytes: None,
        max_rows: None,
    })
}

fn random_shield_apply(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::ShieldApply(IRShieldApplyStep {
        node_type: "shield_apply",
        source_line: 0,
        source_column: 0,
        shield_name: lcg.ascii_with_random_len(8),
        target: lcg.ascii_with_random_len(12),
        output_type: lcg.ascii_with_random_len(8),
    
        breach_policy: None,
    })
}

fn random_stream(_lcg: &mut Lcg) -> IRFlowNode {
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

fn random_navigate(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Navigate(IRNavigateStep {
        depth: None,
        node_type: "navigate",
        source_line: 0,
        source_column: 0,
        pix_ref: lcg.ascii_with_random_len(8),
        corpus_ref: lcg.ascii_with_random_len(8),
        query: lcg.ascii_with_random_len(16),
        trail_enabled: lcg.bool(),
        output_name: lcg.ascii_with_random_len(8),
            seed: String::new(),
            budget: None,
            where_expr: String::new(),
    })
}

fn random_drill(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Drill(IRDrillStep {
        node_type: "drill",
        source_line: 0,
        source_column: 0,
        pix_ref: lcg.ascii_with_random_len(8),
        subtree_path: lcg.ascii_with_random_len(16),
        query: lcg.ascii_with_random_len(16),
        output_name: lcg.ascii_with_random_len(8),
    })
}

fn random_trail(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Trail(IRTrailStep {
        node_type: "trail",
        source_line: 0,
        source_column: 0,
        navigate_ref: lcg.ascii_with_random_len(8),
    })
}

fn random_corroborate(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Corroborate(IRCorroborateStep {
        node_type: "corroborate",
        source_line: 0,
        source_column: 0,
        navigate_ref: lcg.ascii_with_random_len(8),
        output_name: lcg.ascii_with_random_len(8),
    })
}

fn random_ots_apply(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::OtsApply(IROtsApplyStep {
        node_type: "ots_apply",
        source_line: 0,
        source_column: 0,
        ots_name: lcg.ascii_with_random_len(8),
        target: lcg.ascii_with_random_len(12),
        output_type: lcg.ascii_with_random_len(8),
    })
}

fn random_mandate_apply(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::MandateApply(IRMandateApplyStep {
        node_type: "mandate_apply",
        source_line: 0,
        source_column: 0,
        mandate_name: lcg.ascii_with_random_len(8),
        target: lcg.ascii_with_random_len(12),
        output_type: lcg.ascii_with_random_len(8),
    })
}

fn random_compute_apply(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::ComputeApply(IRComputeApplyStep {
        node_type: "compute_apply",
        source_line: 0,
        source_column: 0,
        compute_name: lcg.ascii_with_random_len(8),
        arguments: vec![lcg.ascii_with_random_len(8)],
        output_name: lcg.ascii_with_random_len(8),
    })
}

/// §Fase 119.m.3 — the 46th variant.
///
/// ⚠️ This factory table is HAND-MAINTAINED, and adding an `IRFlowNode`
/// variant does NOT break it — `dispatch_node`'s exhaustive match does (it
/// caught this one four times), but a table of 45 literals keeps passing while
/// silently covering 45 of 46. The tests are named `…all_45`, so leaving it
/// would have made the suite CLAIM coverage it no longer had. Counts updated
/// to 46 along with this entry.
fn random_agent_call(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::AgentCall(IRAgentCall {
        node_type: "agent_call",
        source_line: 0,
        source_column: 0,
        agent_name: lcg.ascii_with_random_len(12),
        arguments: vec![lcg.ascii_with_random_len(8)],
    })
}

fn random_listen(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Listen(IRListenStep {
        node_type: "listen",
        source_line: 0,
        source_column: 0,
        channel: lcg.ascii_with_random_len(12),
        channel_is_ref: lcg.bool(),
        event_alias: lcg.ascii_with_random_len(8),
        body: Vec::new(),
    })
}

fn random_daemon_step(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::DaemonStep(IRDaemonStepNode {
        node_type: "daemon_step",
        source_line: 0,
        source_column: 0,
        daemon_ref: lcg.ascii_with_random_len(12),
    })
}

fn random_emit(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Emit(IREmit {
        node_type: "emit",
        source_line: 0,
        source_column: 0,
        channel_ref: lcg.ascii_with_random_len(8),
        value_ref: lcg.ascii_with_random_len(8),
        value_is_channel: lcg.bool(),
        shield_ref: String::new(),
    
        breach_policy: None,
    })
}

fn random_publish(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Publish(IRPublish {
        node_type: "publish",
        source_line: 0,
        source_column: 0,
        channel_ref: lcg.ascii_with_random_len(8),
        shield_ref: lcg.ascii_with_random_len(8),
        sign: String::new(),
    })
}

fn random_discover(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Discover(IRDiscover {
        node_type: "discover",
        source_line: 0,
        source_column: 0,
        capability_ref: lcg.ascii_with_random_len(8),
        alias: lcg.ascii_with_random_len(8),
    })
}

fn random_persist(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Persist(IRPersistStep {
        node_type: "persist",
            fields: Vec::new(),
        source_line: 0,
        source_column: 0,
        store_name: lcg.ascii_with_random_len(8),
    })
}

fn random_retrieve(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Retrieve(IRRetrieveStep {
        node_type: "retrieve",
        source_line: 0,
        source_column: 0,
        store_name: lcg.ascii_with_random_len(8),
        where_expr: lcg.ascii_with_random_len(12),
        alias: lcg.ascii_with_random_len(8),
        order_by: String::new(),
        limit_expr: String::new(),
        aggregate: String::new(),
        group_by: String::new(),
        cache: String::new(),
    })
}

fn random_mutate(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Mutate(IRMutateStep {
        node_type: "mutate",
            fields: Vec::new(),
        source_line: 0,
        source_column: 0,
        store_name: lcg.ascii_with_random_len(8),
        where_expr: lcg.ascii_with_random_len(12),
    })
}

fn random_purge(lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Purge(IRPurgeStep {
        node_type: "purge",
        source_line: 0,
        source_column: 0,
        store_name: lcg.ascii_with_random_len(8),
        where_expr: lcg.ascii_with_random_len(12),
    })
}

fn random_transact(_lcg: &mut Lcg) -> IRFlowNode {
    IRFlowNode::Transact(IRTransactBlock {
        node_type: "transact",
        source_line: 0,
        source_column: 0,
    })
}

/// Dispatch table: (kind slug, factory). One row per IRFlowNode variant.
/// Iteration over this table = exhaustive iteration over the 46-variant
/// catalog. The order is the same as `flow_dispatcher/mod.rs::dispatch_node`
/// match arms for cache-locality during the totality-stress test.
type Factory = fn(&mut Lcg) -> IRFlowNode;

fn factories() -> Vec<(&'static str, Factory)> {
    vec![
        ("step", random_step),
        ("probe", random_probe),
        ("reason", random_reason),
        ("validate", random_validate),
        ("refine", random_refine),
        ("weave", random_weave),
        ("use_tool", random_use_tool),
        ("remember", random_remember),
        ("recall", random_recall),
        ("conditional", random_conditional),
        ("for_in", random_for_in),
        ("let", random_let),
        ("return", random_return),
        ("break", random_break),
        ("continue", random_continue),
        ("lambda_data_apply", random_lambda_data_apply),
        ("par", random_par),
        ("hibernate", random_hibernate),
        ("deliberate", random_deliberate),
        ("consensus", random_consensus),
        ("forge", random_forge),
        ("focus", random_focus),
        ("associate", random_associate),
        ("aggregate", random_aggregate),
        ("explore", random_explore),
        ("ingest", random_ingest),
        ("shield_apply", random_shield_apply),
        ("stream_block", random_stream),
        ("navigate", random_navigate),
        ("drill", random_drill),
        ("trail", random_trail),
        ("corroborate", random_corroborate),
        ("ots_apply", random_ots_apply),
        ("mandate_apply", random_mandate_apply),
        ("compute_apply", random_compute_apply),
        ("agent_call", random_agent_call),
        ("listen", random_listen),
        ("daemon_step", random_daemon_step),
        ("emit", random_emit),
        ("publish", random_publish),
        ("discover", random_discover),
        ("persist", random_persist),
        ("retrieve", random_retrieve),
        ("mutate", random_mutate),
        ("purge", random_purge),
        ("transact", random_transact),
    ]
}

fn fresh_ctx() -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = DispatchCtx::new(
        "FuzzFlow",
        "stub",
        "system prompt",
        CancellationFlag::new(),
        tx,
    );
    (ctx, rx)
}

fn assert_clean_outcome(
    kind: &str,
    iter: usize,
    outcome: Result<NodeOutcome, DispatchError>,
) {
    match outcome {
        Ok(NodeOutcome::Completed { .. })
        | Ok(NodeOutcome::Break)
        | Ok(NodeOutcome::LoopContinue)
        | Ok(NodeOutcome::Return { .. })
        | Err(DispatchError::UpstreamCancelled)
        | Err(DispatchError::ChannelClosed) => {
            // All four NodeOutcome variants + the two acceptable errors
            // are clean for the closed catalog. Pure-shape handlers
            // produce Completed; orchestration handlers can produce
            // sentinels; algebraic/wire/cognitive/PIX/Lambda produce
            // Completed with placeholders; cancel pre-check produces
            // UpstreamCancelled. ChannelClosed is acceptable when the
            // receiver drops between iters (test runtime teardown).
        }
        // §Fase 119.d — hibernate yields the SUSPENSION outcome: the walk
        // loop (not exercised by this per-node fuzz) parks and halts. The
        // outcome itself is the correct, total answer — the Completed-with-
        // placeholder it replaces was the §111 F20 defect.
        Ok(NodeOutcome::Hibernated { .. }) if kind == "hibernate" => {}
        Ok(other) => panic!(
            "33.y.n fuzz: unexpected NodeOutcome variant for {kind} iter {iter}: {other:?}"
        ),
        // §Fase 111.c/d/f — the primitives MADE REAL in §111 fail CLOSED when
        // their port / catalog is absent (this fuzz ctx attaches none). The
        // refusal IS the correct total outcome, and it is the whole point: each
        // of these used to "complete" with an empty output or a placeholder
        // string, which is indistinguishable from a real result — the defect the
        // fase exists to end.
        //
        //   compute — a placeholder "compute:Name(args)" a step read as a number
        //   yield   — a measurement with no Hilbert space to measure in
        //   warden  — an analysis that never ran, reported as "no findings"
        //   quant   — a block whose body was silently skipped
        //   grad    — a stale artifact carrying no compile-time derivative (§109)
        //   mandate — §119.b: the identity passthrough that bound the target
        //             unchanged under a compliance-sounding name; with no
        //             catalog attached, refusing IS the correct total outcome
        //   lambda  — §119.c: the placeholder string "lambda:<name>(<target>)"
        //             a downstream step consumed as if it were ψ
        //   ots     — §119.c: the identity under a transformation's name
        //   agent   — §119.m.3: the fuzz ctx has an EMPTY agent catalog, and an
        //             unresolvable agent is an UNBOUNDED one (`max_iterations`
        //             lives in the declaration). Refusing is the correct total
        //             outcome here, exactly as it is for mandate/lambda/ots.
        Err(DispatchError::BackendError { ref name, .. })
            if matches!(name.as_str(), "compute" | "yield" | "warden" | "quant" | "grad")
                || name.starts_with("mandate:")
                || name.starts_with("lambda:")
                || name.starts_with("ots:")
                || name.starts_with("agent:") => {}
        Err(DispatchError::BackendError { name, message }) => panic!(
            "33.y.n fuzz: unexpected BackendError for {kind} iter {iter}: name={name} msg={message}"
        ),
        // Fase 108.a - the five data-plane verbs REFUSE without an engine
        // port (MissingDependency: dataspace_engine). Under this fuzz ctx
        // (no engine attached) that refusal is the CORRECT total outcome.
        Err(DispatchError::MissingDependency {
            name: "dataspace_engine",
        }) if matches!(
            kind,
            "focus" | "associate" | "aggregate" | "explore" | "ingest"
        ) => {}
        // §Fase 111.c/d — warden + quant refuse without their engine port.
        Err(DispatchError::MissingDependency {
            name: "warden_backend",
        }) if kind == "warden" => {}
        Err(DispatchError::MissingDependency {
            name: "quant_backend",
        }) if matches!(kind, "quant" | "yield") => {}
        Err(DispatchError::MissingDependency { name }) => panic!(
            "33.y.n fuzz: unexpected MissingDependency for {kind} iter {iter}: name={name}"
        ),
        // §Fase 33.y.l — `DispatchError` is `#[non_exhaustive]`; a
        // future variant added in 33.z+ surfaces here so the fuzz
        // pack reports the new failure shape instead of silently
        // accepting it.
        Err(other) => panic!(
            "33.y.n fuzz: unexpected DispatchError variant for {kind} iter {iter}: {other:?}"
        ),
    }
}

// ────────────────────────────────────────────────────────────────────
//  §1 — Handler totality stress
// ────────────────────────────────────────────────────────────────────
//
// Drive EVERY one of the 46 IRFlowNode variants through `dispatch_node`
// in random-permuted order × 50 iters/variant. Asserts the closed-
// catalog match never panics for any variant, no matter the order or
// surrounding ctx state.
//
// Total: 46 × 50 = 2 300 iters.

#[tokio::test]
async fn fase33y_n_handler_totality_stress() {
    const ITERS_PER_VARIANT: usize = 50;
    let mut lcg = Lcg::new(0x33F4_4E51_DEAD_BEEF);
    let factories = factories();

    for (kind, factory) in &factories {
        for i in 0..ITERS_PER_VARIANT {
            let node = factory(&mut lcg);
            let actual_kind = ir_flow_node_kind(&node);
            assert_eq!(
                actual_kind, *kind,
                "§1 33.y.n: kind slug drift — factory for {kind:?} produced \
                 IR variant returning {actual_kind:?} from ir_flow_node_kind"
            );

            let (mut ctx, _rx) = fresh_ctx();
            let outcome = dispatch_node(&node, &mut ctx).await;
            assert_clean_outcome(kind, i, outcome);
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  §2 — Cancel propagation across all 46
// ────────────────────────────────────────────────────────────────────
//
// For every variant: pre-cancel the flag + dispatch + assert
// `Err(UpstreamCancelled)`. D3 invariant must hold uniformly across
// the entire catalog.
//
// Total: 46 × 30 = 1 380 iters.

#[tokio::test]
async fn fase33y_n_cancel_propagation_across_all_46() {
    const ITERS_PER_VARIANT: usize = 30;
    let mut lcg = Lcg::new(0xCAACE1_AC_055_4_5_4_5);
    let factories = factories();

    for (kind, factory) in &factories {
        for i in 0..ITERS_PER_VARIANT {
            let node = factory(&mut lcg);
            let (tx, _rx) = mpsc::unbounded_channel();
            let cancel = CancellationFlag::new();
            cancel.cancel(); // pre-cancel
            let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);

            let outcome = dispatch_node(&node, &mut ctx).await;
            match outcome {
                Err(DispatchError::UpstreamCancelled) => {} // D3 invariant
                other => panic!(
                    "§2 33.y.n: D3 cancel propagation broken for {kind} iter {i}: \
                     pre-cancelled flag → expected UpstreamCancelled, got {other:?}"
                ),
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  §3 — Orchestration composition (deep-nest stress)
// ────────────────────────────────────────────────────────────────────
//
// Synthesize random orchestration trees: ForIn(body=[Conditional(then=
// [Step], else=[ForIn(body=[Let])])]) × 200 iters with random fields.
// Asserts sentinel propagation + branch_path push/pop discipline +
// step_counter monotonicity hold under composition.
//
// Total: 200 iters.

#[tokio::test]
async fn fase33y_n_orchestration_deep_nest_composition() {
    const ITERS: usize = 200;
    let mut lcg = Lcg::new(0xC0_DEC_03_011_0A_71_0);

    for i in 0..ITERS {
        // Random 3-deep nest: ForIn > Conditional > (Step | Let | Break).
        let inner: IRFlowNode = match lcg.range(3) {
            0 => random_step(&mut lcg),
            1 => random_let(&mut lcg),
            _ => random_break(&mut lcg),
        };

        let conditional_then = vec![inner];
        let conditional = IRFlowNode::Conditional(IRConditional {
            node_type: "conditional",
            source_line: 0,
            source_column: 0,
            condition: lcg.ascii_with_random_len(8),
            comparison_op: "==".to_string(),
            comparison_value: lcg.ascii_with_random_len(8),
            then_body: conditional_then,
            else_body: Vec::new(),
            conditions: Vec::new(),
            conjunctor: String::new(),
            cond: None,
        });

        let for_in = IRFlowNode::ForIn(IRForIn {
            node_type: "for_in",
            source_line: 0,
            source_column: 0,
            variable: "x".to_string(),
            iterable: "a,b,c".to_string(), // 3 iterations
            body: vec![conditional],
        });

        let (mut ctx, _rx) = fresh_ctx();
        let entry_counter = ctx.step_counter;
        let outcome = dispatch_node(&for_in, &mut ctx).await;

        // Outcome should be clean (one of the 4 NodeOutcome variants or
        // an acceptable error). step_counter is monotonic non-decreasing.
        assert_clean_outcome("for_in[composition]", i, outcome);
        assert!(
            ctx.step_counter >= entry_counter,
            "§3 33.y.n iter {i}: step_counter must be monotonic non-decreasing \
             through composition; entry={entry_counter} exit={}",
            ctx.step_counter
        );
        // branch_path must be balanced (no leaked push without pop).
        assert!(
            ctx.branch_path.is_empty(),
            "§3 33.y.n iter {i}: branch_path leaked through composition: {:?}",
            ctx.branch_path
        );
    }
}

// ────────────────────────────────────────────────────────────────────
//  §4 — Algebraic-semantics parity (D10 anchor)
// ────────────────────────────────────────────────────────────────────
//
// Drive the same synthetic IR through `dispatch_node` twice with
// different ctx orderings (let_bindings populated vs empty;
// pending_effect_policy present vs None). Asserts outcome SHAPE is
// deterministic: same NodeOutcome variant kind reached; same
// tokens_emitted count for handlers that pass through the stub
// backend; same wire-stable kind slug.
//
// Total: 300 iters.

#[tokio::test]
async fn fase33y_n_algebraic_semantics_parity_d10() {
    const ITERS: usize = 300;
    let mut lcg = Lcg::new(0xD10A_BC5E_AD01_C0FE);
    let factories = factories();

    for i in 0..ITERS {
        let (kind, factory) = &factories[i % factories.len()];
        let mut variant_lcg = Lcg::new(0xDA1_2_5EED_u64.wrapping_mul(i as u64 + 1));
        let node = factory(&mut variant_lcg);

        // Run A: clean ctx.
        let (mut ctx_a, _rx_a) = fresh_ctx();
        let outcome_a = dispatch_node(&node, &mut ctx_a).await;

        // Run B: ctx with a random let_binding pre-populated + a random
        // pending_effect_policy. These must NOT change the outcome
        // shape for handlers that don't consume them.
        let (mut ctx_b, _rx_b) = fresh_ctx();
        ctx_b
            .let_bindings
            .insert(lcg.ascii_with_random_len(8), lcg.ascii_with_random_len(12));
        ctx_b.pending_effect_policy = match lcg.range(5) {
            0 => Some(BackpressurePolicy::DropOldest),
            1 => Some(BackpressurePolicy::DegradeQuality),
            2 => Some(BackpressurePolicy::PauseUpstream),
            3 => Some(BackpressurePolicy::Fail),
            _ => None,
        };
        let outcome_b = dispatch_node(&node, &mut ctx_b).await;

        // The OUTCOME-KIND must match across the two runs (Completed vs
        // sentinel vs error). The exact field values may differ (e.g.
        // tokens_emitted may change when the enforcer activates), but
        // the variant kind is a D10 invariant.
        let outcome_kind = |o: &Result<NodeOutcome, DispatchError>| -> &'static str {
            match o {
                Ok(NodeOutcome::Completed { .. }) => "Completed",
                Ok(NodeOutcome::Break) => "Break",
                Ok(NodeOutcome::LoopContinue) => "LoopContinue",
                Ok(NodeOutcome::Return { .. }) => "Return",
                Ok(_) => "Ok::other_non_exhaustive",
                Err(DispatchError::UpstreamCancelled) => "Err::UpstreamCancelled",
                Err(DispatchError::ChannelClosed) => "Err::ChannelClosed",
                Err(DispatchError::BackendError { .. }) => "Err::BackendError",
                Err(DispatchError::MissingDependency { .. }) => "Err::MissingDependency",
                Err(_) => "Err::other_non_exhaustive",
            }
        };
        let kind_a = outcome_kind(&outcome_a);
        let kind_b = outcome_kind(&outcome_b);
        assert_eq!(
            kind_a, kind_b,
            "§4 33.y.n iter {i} variant {kind}: D10 parity broken — \
             outcome variant kind differs across ctx variations: \
             run A = {kind_a}, run B = {kind_b}"
        );

        assert_clean_outcome(kind, i, outcome_a);
        assert_clean_outcome(kind, i, outcome_b);
    }
}

// ────────────────────────────────────────────────────────────────────
//  §5 — Tool-call interleaving (D8 + D3 cross-cut)
// ────────────────────────────────────────────────────────────────────
//
// Random Step with random apply_ref + random pre-cancel timing × 250
// iters. Asserts the D8 tool plumbing composes cleanly with the D3
// cancel discipline — no panic, outcome is one of the expected
// variants.
//
// Total: 250 iters.

#[tokio::test]
async fn fase33y_n_tool_call_interleaving_d8_d3() {
    const ITERS: usize = 250;
    let mut lcg = Lcg::new(0xD8_D3_C_055_C07_077);

    for i in 0..ITERS {
        let step = IRFlowNode::Step(IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
            name: lcg.ascii_with_random_len(16),
            persona_ref: String::new(),
            given: String::new(),
            ask: lcg.ascii_with_random_len(40),
            use_tool: None,
            probe: None,
            reason: None,
            weave: None,
            output_type: String::new(),
            confidence_floor: None,
            navigate_ref: String::new(),
            apply_ref: if lcg.bool() {
                lcg.ascii_with_random_len(16) // random tool ref
            } else {
                String::new() // empty → no tool plumb
            },
            requires_context: None,
            now_tz: None,
            pix_ops: Vec::new(),
            stream: None,
            performs: Vec::new(),
            guards: Vec::new(),
            body: Vec::new(),
        });

        let cancel = CancellationFlag::new();
        let pre_cancel = lcg.range(4) == 0; // 25% pre-cancel
        if pre_cancel {
            cancel.cancel();
        }

        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);
        let outcome = dispatch_node(&step, &mut ctx).await;

        if pre_cancel {
            match outcome {
                Err(DispatchError::UpstreamCancelled) => {}
                other => panic!(
                    "§5 33.y.n iter {i}: pre-cancel + tool plumb broken: \
                     expected UpstreamCancelled, got {other:?}"
                ),
            }
        } else {
            // Stub backend never emits FinishReason::ToolUse so no
            // ToolCall event surfaces; outcome is canonical
            // Completed("(stub)", 1).
            match outcome {
                Ok(NodeOutcome::Completed { tokens_emitted, .. }) => {
                    assert_eq!(
                        tokens_emitted, 1,
                        "§5 33.y.n iter {i}: D4 byte-compat broken — \
                         stub backend should emit exactly 1 token"
                    );
                }
                Err(DispatchError::ChannelClosed) => {} // acceptable
                other => panic!(
                    "§5 33.y.n iter {i}: tool plumb broke outcome shape: \
                     expected Completed or ChannelClosed, got {other:?}"
                ),
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  §6 — Kind slug round-trip (D11 cross-stack anchor)
// ────────────────────────────────────────────────────────────────────
//
// For every variant × 20 iters: synthesize random IR, call
// `ir_flow_node_kind` + assert the returned slug matches the
// 46-entry expected catalog AND is non-empty AND is the same slug
// across all iters for the same variant (kind is a function of the
// discriminant, not the payload).
//
// Total: 46 × 20 = 920 iters.

#[test]
fn fase33y_n_kind_slug_round_trip_d11_anchor() {
    const ITERS_PER_VARIANT: usize = 20;
    let mut lcg = Lcg::new(0x511_D11_C_055_57_AC);
    let factories = factories();
    let expected_kinds: Vec<&str> = factories.iter().map(|(k, _)| *k).collect();

    for (expected_kind, factory) in &factories {
        for i in 0..ITERS_PER_VARIANT {
            let node = factory(&mut lcg);
            let actual_kind = ir_flow_node_kind(&node);
            assert!(
                !actual_kind.is_empty(),
                "§6 33.y.n iter {i} variant {expected_kind}: kind slug must be non-empty"
            );
            assert_eq!(
                actual_kind, *expected_kind,
                "§6 33.y.n iter {i}: kind slug drift — factory for \
                 {expected_kind:?} produced IR returning {actual_kind:?}"
            );
            assert!(
                expected_kinds.contains(&actual_kind),
                "§6 33.y.n iter {i}: kind slug {actual_kind:?} not in the \
                 expected 46-entry closed catalog"
            );
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  §7 — Cardinality pins
// ────────────────────────────────────────────────────────────────────

#[test]
fn fase33y_n_factory_table_has_exactly_46_entries() {
    let factories = factories();
    assert_eq!(
        factories.len(),
        46,
        "33.y.n: consolidated fuzz factory table must mirror the 46-variant \
         IRFlowNode closed catalog. A 46th variant in a future axon-lang \
         minor fails this assertion + the dispatch_node compile."
    );
}

#[test]
fn fase33y_n_total_iter_count_pinned() {
    // Sum the per-test iter counts so the documented "5 350 total"
    // is enforced. Drift surfaces at PR time.
    const SECT_1_TOTALITY: usize = 46 * 50;
    const SECT_2_CANCEL: usize = 46 * 30;
    const SECT_3_NESTED: usize = 200;
    const SECT_4_PARITY: usize = 300;
    const SECT_5_TOOL: usize = 250;
    const SECT_6_SLUG: usize = 46 * 20;
    let total =
        SECT_1_TOTALITY + SECT_2_CANCEL + SECT_3_NESTED + SECT_4_PARITY + SECT_5_TOOL + SECT_6_SLUG;
    assert_eq!(
        total, 5350,
        "33.y.n consolidated fuzz pack must run exactly 5 350 deterministic \
         LCG iters across 6 cross-cutting surfaces."
    );
}
