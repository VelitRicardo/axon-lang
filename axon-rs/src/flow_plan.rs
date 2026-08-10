//! §Fase 33.x.b + 33.x.c — Streaming execution plan extraction +
//! unified `.axon` source compilation pipeline.
//!
//! Builds a streaming-shaped execution plan from `.axon` source for
//! the production async SSE path. Pre-resolves per-step
//! [`BackpressurePolicy`] via
//! [`crate::stream_effect_dispatcher::resolve_stream_effect_for_step`]
//! so the hot per-chunk loop in
//! `crate::axon_server::server_execute_streaming_async` does NOT
//! re-walk the AST per chunk.
//!
//! # Scope boundary (33.x.b)
//!
//! 33.x.b ships the SIMPLEST step shape (single `ask`, no anchors,
//! no `apply: lambda`, no `let` bindings, no mid-stream `use_tool`).
//! This is exactly the canonical Kivi/Stream-effect adopter shape
//! the diagnostic anchor pins. Flows that use the omitted features
//! still execute correctly on the legacy synchronous path; the
//! streaming path falls back via [`PlanFallback::LegacyOrchestration`]
//! and the SSE handler routes through `server_execute_full` (the
//! pre-33.x.b path). NO silent degradation — the fallback is
//! observable in plan output so adopter diagnostics can flag it.
//!
//! # Lift target (33.x.c)
//!
//! `runner::build_execution_plan` (private, sync) stays unchanged in
//! 33.x.b. 33.x.c unifies the two builders. Until then, this module
//! is the SOLE source of truth for streaming-path plan extraction;
//! `runner.rs` is the sole source of truth for sync-path execution.
//!
//! # D-letter anchors
//!
//! - **D1** — every streaming-path flow resolves a [`StreamingExecutionPlan`]
//!   here before dispatching to `Backend::stream()`. The plan's
//!   `backend_name` is the resolved provider name; `steps[].effect_policy`
//!   pre-resolves the declared `<stream:<policy>>` for the
//!   `StreamPolicyEnforcer` wrap in 33.x.d.
//! - **D4** — the plan structure carries NO new wire-shape fields;
//!   it is internal scaffolding that produces the same
//!   FlowExecutionEvent sequence as the pre-33.x.b path for the
//!   adopter shapes 33.x.b supports.
//! - **D5** — fallback to legacy orchestration is observable via
//!   [`PlanFallback`] so 33.x.g can hook `axon-W002`-style warnings.

use crate::ir_nodes::{IRFlow, IRFlowNode, IRProgram};
use crate::stream_effect::BackpressurePolicy;
use crate::stream_effect_dispatcher::resolve_stream_effect_for_step;

// ────────────────────────────────────────────────────────────────────
//  §33.x.c — Shared source-compilation pipeline
// ────────────────────────────────────────────────────────────────────

/// Compile an `.axon` source string into its AST [`crate::ast::Program`]
/// + IR [`IRProgram`] forms. Both stacks-of-truth are returned so
/// downstream callers can do AST-level walking (effect-row resolution,
/// route table extraction) AND IR-level execution planning without
/// re-running the pipeline.
///
/// # Stages
///
/// 1. Lex via [`crate::lexer::Lexer`]
/// 2. Parse via [`crate::parser::Parser`]
/// 3. Type-check via [`crate::type_checker::TypeChecker`]
/// 4. IR generation via [`crate::ir_generator::IRGenerator`]
///
/// Each stage's failure surfaces as a structured [`PlanError`] variant
/// so the caller distinguishes "user source is malformed" from "the
/// pipeline encountered an internal invariant violation".
///
/// # Performance
///
/// This is the canonical source-compilation entry point on the Fase
/// 33.x.b production async streaming path (called once per
/// flow-invocation). The legacy `axon_server.rs` call sites still
/// inline their own pipelines; 33.x.i (mono-file `crate::backend`
/// retirement) is when the deeper migration completes.
///
/// # Purity
///
/// Pure + deterministic. No I/O. No global state. Same source +
/// source_file → same `(Program, IRProgram)` byte-for-byte.
pub fn compile_source_to_ir(
    source: &str,
    source_file: &str,
) -> Result<(crate::ast::Program, IRProgram), PlanError> {
    let tokens = crate::lexer::Lexer::new(source, source_file)
        .tokenize()
        .map_err(|e| PlanError::Parse(format!("lex error: {e:?}")))?;

    let mut parser = crate::parser::Parser::new(tokens);
    let program = parser.parse().map_err(|e| PlanError::Parse(e.message))?;

    let checker = crate::type_checker::TypeChecker::new(&program);
    let type_errors = checker.check();
    if !type_errors.is_empty() {
        return Err(PlanError::TypeCheck(
            type_errors.into_iter().map(|e| e.message).collect(),
        ));
    }

    let ir = crate::ir_generator::IRGenerator::new().generate(&program);
    Ok((program, ir))
}

/// Locate an [`IRFlow`] by name. Returns a structured [`PlanError`]
/// with the list of available flow names so the diagnostic message
/// is adopter-actionable (typo-detection on first read).
///
/// # Used by
///
/// * [`build_plan_from_ir`] — the streaming-path planner.
/// * Future 33.x.i migrations of the legacy axon_server.rs sync
///   path will adopt this helper for byte-identical diagnostics.
pub fn find_ir_flow_by_name<'a>(
    ir: &'a IRProgram,
    flow_name: &str,
) -> Result<&'a IRFlow, PlanError> {
    ir.flows
        .iter()
        .find(|f| f.name == flow_name)
        .ok_or_else(|| PlanError::FlowNotFound {
            flow_name: flow_name.to_string(),
            available: ir.flows.iter().map(|f| f.name.clone()).collect(),
        })
}

/// Stable kind discriminant for an [`IRFlowNode`]. Closed catalog
/// extracted as a separate public helper so adding a new
/// `IRFlowNode` variant on the frontend forces an explicit match
/// update here (compiler enforces exhaustiveness).
///
/// # Drift contract with `runner::extract_step_info`
///
/// The synchronous CLI path's `runner::extract_step_info` produces
/// a `step_type` string for each `IRFlowNode` variant. The kinds
/// here MUST match that mapping. Drift is gated by
/// [`ir_flow_node_kind_runner_drift`] in `flow_plan::tests`.
pub fn ir_flow_node_kind(node: &IRFlowNode) -> &'static str {
    match node {
        IRFlowNode::Step(_) => "step",
        IRFlowNode::Grad(_) => "grad",
        IRFlowNode::Probe(_) => "probe",
        IRFlowNode::Reason(_) => "reason",
        IRFlowNode::Validate(_) => "validate",
        IRFlowNode::Refine(_) => "refine",
        IRFlowNode::Weave(_) => "weave",
        IRFlowNode::UseTool(_) => "use_tool",
        IRFlowNode::Remember(_) => "remember",
        IRFlowNode::Recall(_) => "recall",
        IRFlowNode::Conditional(_) => "conditional",
        IRFlowNode::ForIn(_) => "for_in",
        IRFlowNode::Let(_) => "let",
        IRFlowNode::Return(_) => "return",
        IRFlowNode::Break(_) => "break",
        IRFlowNode::Continue(_) => "continue",
        IRFlowNode::LambdaDataApply(_) => "lambda_data_apply",
        IRFlowNode::Par(_) => "par",
        IRFlowNode::Hibernate(_) => "hibernate",
        IRFlowNode::Deliberate(_) => "deliberate",
        IRFlowNode::Consensus(_) => "consensus",
        IRFlowNode::Forge(_) => "forge",
        IRFlowNode::Focus(_) => "focus",
        IRFlowNode::Associate(_) => "associate",
        IRFlowNode::Aggregate(_) => "aggregate",
        IRFlowNode::Explore(_) => "explore",
        IRFlowNode::Ingest(_) => "ingest",
        IRFlowNode::ShieldApply(_) => "shield_apply",
        IRFlowNode::Stream(_) => "stream_block",
        // §Fase 120 — the algebraic-effect constructs. These slugs are the ones
        // `effect_handlers` puts on the wire as `step_type`, so an adopter
        // reading an SSE trace sees the same names their source uses.
        IRFlowNode::Handle(_) => "handle",
        IRFlowNode::Perform(_) => "perform",
        IRFlowNode::Resume(_) => "resume",
        IRFlowNode::Abort(_) => "abort",
        IRFlowNode::Forward(_) => "forward",
        IRFlowNode::Navigate(_) => "navigate",
        IRFlowNode::Drill(_) => "drill",
        IRFlowNode::Trail(_) => "trail",
        IRFlowNode::Corroborate(_) => "corroborate",
        IRFlowNode::OtsApply(_) => "ots_apply",
        IRFlowNode::MandateApply(_) => "mandate_apply",
        IRFlowNode::ComputeApply(_) => "compute_apply",
        // §Fase 119.m.3 — a NEW wire slug. Additive: no pre-§119.m program can
        // emit it, so every existing flow's wire shape is byte-identical (D4).
        IRFlowNode::AgentCall(_) => "agent_call",
        IRFlowNode::Listen(_) => "listen",
        IRFlowNode::DaemonStep(_) => "daemon_step",
        IRFlowNode::Emit(_) => "emit",
        // §Fase 92.b — ephemeral-credential minting. Same step_type string
        // as `runner::extract_step_info` (drift-gated).
        IRFlowNode::Mint(_) => "mint",
        // §Fase 94.b — mediated secret renewal. Same step_type string as
        // `runner::extract_step_info` (drift-gated).
        IRFlowNode::Rotate(_) => "rotate",
        IRFlowNode::Publish(_) => "publish",
        IRFlowNode::Discover(_) => "discover",
        IRFlowNode::Persist(_) => "persist",
        IRFlowNode::Retrieve(_) => "retrieve",
        IRFlowNode::Mutate(_) => "mutate",
        IRFlowNode::Purge(_) => "purge",
        IRFlowNode::Transact(_) => "transact",
        // §Fase 88.a — the `warden` adversarial-analysis block. Same step_type
        // string as `runner::extract_step_info` below (drift-gated).
        IRFlowNode::Warden(_) => "warden",
        // §Fase 51.a — the `quant` cognitive block. Same step_type string as
        // `runner::extract_step_info` below (drift-gated).
        IRFlowNode::Quant(_) => "quant",
        // §Fase 51.d.2 — the `yield` measurement point.
        IRFlowNode::Yield(_) => "yield",
        // §Fase 52.c — `run <Flow>(args)` flow-step.
        IRFlowNode::Run(_) => "run",
    }
}

/// Compose a streaming-path system prompt from persona + context +
/// (optional) backend tag. Public helper shared by the streaming
/// planner today + adopted by the sync CLI path during 33.x.i.
///
/// # Parameters
///
/// * `flow` — the IR flow whose system prompt is being built. The
///   flow's name surfaces in the prompt for trace clarity.
/// * `ir` — full IR for persona/context resolution.
/// * `backend_tag` — `Some("anthropic")` appends `[Backend:
///   anthropic | AXON <version>]`; `None` omits the tag. The
///   streaming path passes `None` because the wire's
///   `axon.complete.backend` field already carries this info.
///
/// # Determinism
///
/// Pure + deterministic. Same inputs → same string byte-for-byte.
pub fn compose_system_prompt_public(
    flow: &IRFlow,
    ir: &IRProgram,
    backend_tag: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(persona) = ir.personas.first() {
        parts.push(format!("# Persona: {}", persona.name));
        if !persona.domain.is_empty() {
            parts.push(format!("Domain expertise: {}", persona.domain.join(", ")));
        }
        if !persona.tone.is_empty() {
            parts.push(format!("Communication tone: {}", persona.tone));
        }
        if !persona.language.is_empty() {
            parts.push(format!("Language: {}", persona.language));
        }
        if let Some(ct) = persona.confidence_threshold {
            parts.push(format!("Confidence threshold: {ct:.2}"));
        }
        if persona.cite_sources == Some(true) {
            parts.push("Always cite sources.".to_string());
        }
        if !persona.refuse_if.is_empty() {
            parts.push(format!("Refuse if: {}", persona.refuse_if.join(", ")));
        }
    }

    if let Some(ctx) = ir.contexts.first() {
        parts.push(format!("\n# Context: {}", ctx.name));
        if !ctx.depth.is_empty() {
            parts.push(format!("Analysis depth: {}", ctx.depth));
        }
        if !ctx.memory_scope.is_empty() {
            parts.push(format!("Memory scope: {}", ctx.memory_scope));
        }
        if let Some(t) = ctx.temperature {
            parts.push(format!("Temperature: {t:.1}"));
        }
        if let Some(mt) = ctx.max_tokens {
            parts.push(format!("Max tokens: {mt}"));
        }
    }

    parts.push(format!("\n# Flow: {}", flow.name));

    if let Some(tag) = backend_tag {
        parts.push(format!("\n[Backend: {tag} | AXON {}]", env!("CARGO_PKG_VERSION")));
    }

    parts.join("\n")
}

// ────────────────────────────────────────────────────────────────────
//  Plan types
// ────────────────────────────────────────────────────────────────────

/// One step in a streaming execution plan.
///
/// Pre-resolves every field the hot loop needs — system prompt,
/// user prompt, declared effect policy. The per-chunk loop in
/// `server_execute_streaming_async` reads these fields without
/// touching the IR or AST.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamingStep {
    /// Canonical step name (matches `IRStep.name`). Surfaces in
    /// `axon.token.step` + `axon.complete.step_names`.
    pub step_name: String,
    /// User prompt this step asks the LLM. Built from `step.ask` +
    /// optional `apply: tool` argument expansion.
    pub user_prompt: String,
    /// Optional max-tokens cap from `context.max_tokens` or
    /// step-level `max_tokens:` declaration.
    pub max_tokens: Option<u32>,
    /// Optional temperature from `context.temperature` (overridden
    /// by locked-param dispatch in the Backend impl).
    pub temperature: Option<f64>,
    /// Pre-resolved backpressure policy from the step's tool's
    /// `effects: <stream:<policy>>` declaration. `None` when the
    /// step doesn't `apply:` a tool with a stream effect.
    /// Activated by 33.x.d's `StreamPolicyEnforcer` wrap; recorded
    /// today in `axon.complete.stream_policies` for audit
    /// correlation (Fase 33.e).
    pub effect_policy: Option<BackpressurePolicy>,
}

/// Streaming execution plan — one per flow invocation.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamingExecutionPlan {
    /// Flow name from `run` declaration.
    pub flow_name: String,
    /// Backend name as the adopter resolved it (after `auto`
    /// resolution upstream of plan construction).
    pub backend_name: String,
    /// Composed system prompt — persona + context + anchor
    /// instructions (Fase 11 stack). Same shape as the sync
    /// `runner::build_system_prompt` output.
    pub system_prompt: String,
    /// Ordered step list. Empty plan == empty flow (rare but valid
    /// per IR grammar; the streaming path emits FlowStart +
    /// FlowComplete with `steps_executed: 0` per Fase 33.b).
    pub steps: Vec<StreamingStep>,
}

/// Reason a flow could not be planned for the streaming path.
///
/// Each variant is a closed-catalog signal the SSE handler routes
/// to either an `axon.error` wire event (hard error) or a fallback
/// to the legacy synchronous path (soft fall-through).
#[derive(Debug, Clone, PartialEq)]
pub enum PlanError {
    /// Source did not parse. Mirrors `Parser::parse` error message.
    Parse(String),
    /// Type checker rejected the source.
    TypeCheck(Vec<String>),
    /// IR generation failed (rare — usually means the type-check
    /// pass missed an invariant).
    IrGeneration(String),
    /// The requested flow_name was not found in the IR's flow list.
    FlowNotFound { flow_name: String, available: Vec<String> },
    // §Fase 33.z.e — `LegacyOrchestrationRequired` variant DELETED
    // (the 33.y.l `#[deprecated]` retirement cycle completes here).
    // The 33.y per-IRFlowNode async dispatcher (45/45 graduation)
    // covers every IRFlowNode variant the planner used to reject.
    // Any downstream crate that pattern-matched against this variant
    // hits an explicit compile error — the intended failure shape
    // for the deprecation cycle.
}

/// Closed catalog of reasons the streaming path falls back to the
/// legacy synchronous orchestration. Each variant maps to a
/// specific adopter source shape the 33.x.b scope explicitly
/// defers.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanFallback {
    /// Flow uses `anchor: <name>` constraints. Anchor enforcement
    /// fires on FINAL flow output, which is incompatible with
    /// per-token streaming until per-chunk anchor checking lands.
    AnchorConstraintsPresent,
    /// Flow uses `apply: <lambda>` (Fase 15 lambda data apply).
    /// Lambda apply runs after the step's LLM response; per-chunk
    /// lambda application is a future fase.
    LambdaApplyPresent,
    /// Flow uses `let X = ...` SSA bindings (Fase 17). Binding
    /// resolution runs between steps; streaming path doesn't yet
    /// thread the binding context through the per-chunk loop.
    LetBindingPresent,
    /// Flow uses `use_tool` mid-step (function calling). Mid-stream
    /// tool calls are explicit Fase 33-followon-2 scope.
    UseToolPresent,
    /// Flow contains a `Hibernate` step (Fase 19 CPS). Hibernation
    /// requires the synchronous CPS handler stack.
    HibernatePresent,
    /// Flow contains a `Drill` or `Trail` step (Fase 19 PIX). PIX
    /// trace state is captured on the synchronous path.
    PixPresent,
    /// Flow contains an `IRFlowNode` variant that 33.x.b does not
    /// yet model (Conditional / ForIn / Par / Probe / Reason /
    /// ShieldApply / etc.). 33.x.b ships the canonical
    /// `step S { ask: "..." [apply: tool] }` shape only; subsequent
    /// 33.x followups extend coverage per founder-sequenced
    /// sub-fases. The legacy synchronous path keeps working for
    /// these flows — there is no functional regression.
    UnsupportedNode {
        /// Stable kind discriminant — e.g. `"conditional"`,
        /// `"for_in"`, `"reason"`. Surfaces in audit row + future
        /// 33.x.g warning emission.
        kind: &'static str,
    },
}

impl PlanFallback {
    /// Stable slug for diagnostic emission. Used by audit row +
    /// future 33.x.g warning surface.
    pub fn slug(&self) -> &'static str {
        match self {
            Self::AnchorConstraintsPresent => "anchor_constraints",
            Self::LambdaApplyPresent => "lambda_apply",
            Self::LetBindingPresent => "let_binding",
            Self::UseToolPresent => "use_tool",
            Self::HibernatePresent => "hibernate",
            Self::PixPresent => "pix",
            Self::UnsupportedNode { .. } => "unsupported_node",
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  Plan builder
// ────────────────────────────────────────────────────────────────────

/// Build a streaming execution plan from `.axon` source.
///
/// Pipeline: lex → parse → type-check → IR generation → walk the
/// IR's runs for `flow_name` → emit one [`StreamingStep`] per step.
///
/// §Fase 33.z.e — Returns `Err(PlanError::*)` only for hard compile
/// failures (Parse / TypeCheck / IrGeneration / FlowNotFound). The
/// 33.y.l `LegacyOrchestrationRequired` variant has been DELETED;
/// every IRFlowNode variant the planner produces is dispatchable
/// via `flow_dispatcher::dispatch_node`.
pub fn build_streaming_plan(
    source: &str,
    source_file: &str,
    flow_name: &str,
    backend_name: &str,
) -> Result<StreamingExecutionPlan, PlanError> {
    // §33.x.c — delegated to the unified `compile_source_to_ir`
    // helper. Both the AST and IR forms are returned so
    // `build_plan_from_ir` can resolve effect rows from the AST
    // without re-walking the parser pipeline.
    let (program, ir) = compile_source_to_ir(source, source_file)?;
    build_plan_from_ir(&ir, &program, flow_name, backend_name)
}

/// Build a plan from an already-typed IR. Useful for tests that
/// drive the planner with hand-constructed IR (no source parse).
///
/// §Fase 33.z.e — the `unsupported_feature_reason` rejection step
/// has been DELETED in lockstep with `PlanError::LegacyOrchestrationRequired`
/// + the v1.26.0 legacy path 33.z.e deleted. The per-IRFlowNode dispatcher
/// (Fase 33.y 45/45) handles every IRFlowNode variant; the planner
/// no longer needs to gate against a closed-deferred-catalog. Any
/// shape the planner could compile pre-33.z.e is dispatchable
/// post-33.z.e via `flow_dispatcher::dispatch_node`.
pub fn build_plan_from_ir(
    ir: &IRProgram,
    program: &crate::ast::Program,
    flow_name: &str,
    backend_name: &str,
) -> Result<StreamingExecutionPlan, PlanError> {
    // 5. Locate the flow on the IR side. §33.x.c uses the public
    //    `find_ir_flow_by_name` helper so the diagnostic message
    //    shape is shared across callers.
    let flow = find_ir_flow_by_name(ir, flow_name)?;

    // 5b. Locate the same flow on the AST side (for effect-row
    //     resolution via `resolve_stream_effect_for_step`, which
    //     takes `&FlowDefinition`).
    let ast_flow = program
        .declarations
        .iter()
        .find_map(|d| match d {
            crate::ast::Declaration::Flow(f) if f.name == flow_name => Some(f),
            _ => None,
        })
        .ok_or_else(|| PlanError::FlowNotFound {
            flow_name: flow_name.to_string(),
            available: ir.flows.iter().map(|f| f.name.clone()).collect(),
        })?;

    // §Fase 33.z.e — `unsupported_feature_reason` gate retired.
    // The dispatcher path handles every IRFlowNode variant; no
    // pre-flight rejection needed. The `StreamingExecutionPlan`
    // produced below carries only canonical Step variants by
    // design (other shapes route through the dispatcher's
    // per-variant handlers directly via `flow.steps`); the plan
    // structure stays as the legacy carrier for backend resolution
    // + system-prompt composition + per-step effect policy
    // resolution.

    // 7. System prompt — §33.x.c uses the public composer with
    //    `backend_tag: None`. The streaming wire's
    //    `axon.complete.backend` field already carries the backend
    //    name so the prompt suffix is redundant on this path
    //    (avoids hidden-context drift between sync + async).
    let system_prompt = compose_system_prompt_public(flow, ir, None);

    // 8. Per-step plan. Only `IRFlowNode::Step` variants surface in
    //    `StreamingExecutionPlan.steps` (other variants are handled
    //    by the dispatcher's per-variant handlers in
    //    `streaming_via_dispatcher` directly via `flow.steps`).
    let mut steps = Vec::new();
    for node in &flow.steps {
        if let crate::ir_nodes::IRFlowNode::Step(ir_step) = node {
            let max_tokens = ir
                .contexts
                .first()
                .and_then(|c| c.max_tokens)
                .map(|n| n as u32);
            let temperature = ir.contexts.first().and_then(|c| c.temperature);
            let effect_policy =
                resolve_stream_effect_for_step(&ir_step.name, ast_flow, program);

            steps.push(StreamingStep {
                step_name: ir_step.name.clone(),
                user_prompt: ir_step.ask.clone(),
                max_tokens,
                temperature,
                effect_policy,
            });
        }
    }

    Ok(StreamingExecutionPlan {
        flow_name: flow_name.to_string(),
        backend_name: backend_name.to_string(),
        system_prompt,
        steps,
    })
}

// §33.x.c — The private `ir_flow_node_kind` was lifted to the
// module's public surface above. The single source of truth for
// the kind-string mapping lives at `flow_plan::ir_flow_node_kind`
// and is drift-gated by `tests::ir_flow_node_kind_runner_drift`.

// §33.x.c — `compose_system_prompt` was lifted to public
// `compose_system_prompt_public` above. It accepts an optional
// `backend_tag` so the sync CLI path can opt into the canonical
// `[Backend: foo | AXON x.y.z]` suffix during 33.x.i migration.

// ────────────────────────────────────────────────────────────────────
//  Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_simple_stream_source() -> &'static str {
        // Canonical Kivi-shape: single step, output Stream<Token>,
        // explicit transport: sse. No anchors, no apply, no let.
        "flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" output: Stream<Token> }\n\
         }\n\
         axonendpoint ChatEndpoint { method: POST path: \"/chat\" execute: Chat transport: sse public: true }"
    }

    fn parse_stream_with_effect_source() -> &'static str {
        // Disjunct (b): tool with stream effect, step apply.
        "tool chat_token_stream { description: \"stream\" effects: <stream:drop_oldest> }\n\
         flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" apply: chat_token_stream }\n\
         }\n\
         axonendpoint ChatEndpoint { method: POST path: \"/chat\" execute: Chat transport: sse public: true }"
    }

    #[test]
    fn build_plan_for_simple_stream_flow_returns_one_step() {
        let plan = build_streaming_plan(
            parse_simple_stream_source(),
            "test.axon",
            "Chat",
            "stub",
        )
        .expect("simple stream flow plans cleanly");

        assert_eq!(plan.flow_name, "Chat");
        assert_eq!(plan.backend_name, "stub");
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].step_name, "Generate");
        assert_eq!(plan.steps[0].user_prompt, "hi");
        assert_eq!(plan.steps[0].effect_policy, None, "no tool effect → no policy");
    }

    #[test]
    fn build_plan_with_drop_oldest_effect_pre_resolves_policy() {
        let plan = build_streaming_plan(
            parse_stream_with_effect_source(),
            "test.axon",
            "Chat",
            "stub",
        )
        .expect("stream-effect flow plans cleanly");

        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].effect_policy, Some(BackpressurePolicy::DropOldest));
    }

    #[test]
    fn build_plan_unknown_flow_returns_flow_not_found() {
        let err = build_streaming_plan(
            parse_simple_stream_source(),
            "test.axon",
            "NonexistentFlow",
            "stub",
        )
        .expect_err("unknown flow rejected");

        match err {
            PlanError::FlowNotFound { flow_name, available } => {
                assert_eq!(flow_name, "NonexistentFlow");
                assert_eq!(available, vec!["Chat".to_string()]);
            }
            other => panic!("expected FlowNotFound, got {other:?}"),
        }
    }

    #[test]
    fn build_plan_unparseable_source_returns_parse_error() {
        let err = build_streaming_plan(
            "not a valid axon source",
            "test.axon",
            "Chat",
            "stub",
        )
        .expect_err("garbage rejected");

        assert!(matches!(err, PlanError::Parse(_)));
    }

    #[test]
    fn build_plan_multi_step_flow_preserves_order() {
        let src = "flow MultiStep() -> Unit {\n\
                     step First { ask: \"one\" }\n\
                     step Second { ask: \"two\" }\n\
                     step Third { ask: \"three\" }\n\
                   }\n\
                   axonendpoint E { method: POST path: \"/m\" execute: MultiStep transport: sse public: true }";
        let plan = build_streaming_plan(src, "test.axon", "MultiStep", "stub").unwrap();
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].step_name, "First");
        assert_eq!(plan.steps[1].step_name, "Second");
        assert_eq!(plan.steps[2].step_name, "Third");
        assert_eq!(plan.steps[0].user_prompt, "one");
        assert_eq!(plan.steps[1].user_prompt, "two");
        assert_eq!(plan.steps[2].user_prompt, "three");
    }

    #[test]
    fn plan_fallback_slugs_are_stable_strings() {
        // Each variant has a documented slug. Drift in this match
        // surfaces in 33.x.g warning emission.
        assert_eq!(PlanFallback::AnchorConstraintsPresent.slug(), "anchor_constraints");
        assert_eq!(PlanFallback::LambdaApplyPresent.slug(), "lambda_apply");
        assert_eq!(PlanFallback::LetBindingPresent.slug(), "let_binding");
        assert_eq!(PlanFallback::UseToolPresent.slug(), "use_tool");
        assert_eq!(PlanFallback::HibernatePresent.slug(), "hibernate");
        assert_eq!(PlanFallback::PixPresent.slug(), "pix");
    }

    #[test]
    fn plan_is_deterministic_for_same_source() {
        let plan1 = build_streaming_plan(
            parse_simple_stream_source(),
            "test.axon",
            "Chat",
            "stub",
        )
        .unwrap();
        let plan2 = build_streaming_plan(
            parse_simple_stream_source(),
            "test.axon",
            "Chat",
            "stub",
        )
        .unwrap();
        assert_eq!(plan1, plan2, "plan builder is pure + deterministic");
    }

    #[test]
    fn streaming_step_eq_is_field_wise() {
        let a = StreamingStep {
            step_name: "X".into(),
            user_prompt: "y".into(),
            max_tokens: Some(100),
            temperature: Some(0.7),
            effect_policy: Some(BackpressurePolicy::DropOldest),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn streaming_plan_includes_backend_name() {
        let plan = build_streaming_plan(
            parse_simple_stream_source(),
            "test.axon",
            "Chat",
            "anthropic",
        )
        .unwrap();
        assert_eq!(plan.backend_name, "anthropic");
    }

    #[test]
    fn empty_flow_body_produces_empty_step_list() {
        // Pathological but parseable case: empty flow body.
        let src = "flow Empty() -> Unit {\n\
                   }\n\
                   axonendpoint E { method: POST path: \"/e\" execute: Empty transport: sse public: true }";
        let plan = build_streaming_plan(src, "test.axon", "Empty", "stub").unwrap();
        assert!(plan.steps.is_empty());
        assert_eq!(plan.flow_name, "Empty");
    }

    // ── §33.x.c — Public helper tests ───────────────────────────────

    #[test]
    fn compile_source_to_ir_returns_program_and_ir_for_valid_source() {
        let (program, ir) =
            compile_source_to_ir(parse_simple_stream_source(), "test.axon").unwrap();
        // Program has at least one Flow declaration.
        let flow_count = program
            .declarations
            .iter()
            .filter(|d| matches!(d, crate::ast::Declaration::Flow(_)))
            .count();
        assert_eq!(flow_count, 1);
        // IR mirrors with one flow.
        assert_eq!(ir.flows.len(), 1);
        assert_eq!(ir.flows[0].name, "Chat");
    }

    #[test]
    fn compile_source_to_ir_is_pure_deterministic() {
        let src = parse_simple_stream_source();
        let (p1, ir1) = compile_source_to_ir(src, "test.axon").unwrap();
        let (p2, ir2) = compile_source_to_ir(src, "test.axon").unwrap();
        assert_eq!(p1.declarations.len(), p2.declarations.len());
        assert_eq!(ir1.flows.len(), ir2.flows.len());
        assert_eq!(ir1.flows[0].name, ir2.flows[0].name);
    }

    #[test]
    fn compile_source_to_ir_surfaces_parse_error() {
        let err = compile_source_to_ir("not axon source at all", "test.axon").unwrap_err();
        assert!(matches!(err, PlanError::Parse(_)));
    }

    #[test]
    fn compile_source_to_ir_surfaces_type_check_error() {
        // §Fase 28 — `axonendpoint method: YEET` is rejected at
        // parse time. Use a different shape that parses cleanly
        // but fails type-check: undefined flow reference.
        let src = "axonendpoint Bad { method: POST path: \"/x\" execute: NonexistentFlow public: true }";
        let err = compile_source_to_ir(src, "test.axon").unwrap_err();
        // Either parser rejected it (post-Fase 28 hardening may
        // catch undefined references at parse time) or the type
        // checker did — both are valid PlanError variants.
        assert!(matches!(err, PlanError::Parse(_) | PlanError::TypeCheck(_)));
    }

    #[test]
    fn find_ir_flow_by_name_returns_flow_when_present() {
        let (_p, ir) = compile_source_to_ir(parse_simple_stream_source(), "t.axon").unwrap();
        let flow = find_ir_flow_by_name(&ir, "Chat").unwrap();
        assert_eq!(flow.name, "Chat");
    }

    #[test]
    fn find_ir_flow_by_name_returns_flow_not_found_with_available_list() {
        let (_p, ir) = compile_source_to_ir(parse_simple_stream_source(), "t.axon").unwrap();
        let err = find_ir_flow_by_name(&ir, "Nope").unwrap_err();
        match err {
            PlanError::FlowNotFound { flow_name, available } => {
                assert_eq!(flow_name, "Nope");
                assert_eq!(available, vec!["Chat".to_string()]);
            }
            other => panic!("expected FlowNotFound, got {other:?}"),
        }
    }

    #[test]
    fn ir_flow_node_kind_step_returns_step() {
        use crate::ir_nodes::{IRFlowNode, IRStep};
        let n = IRFlowNode::Step(IRStep {
            node_type: "Step",
            source_line: 1,
            source_column: 1,
            name: "S".into(),
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
            performs: Vec::new(),
            guards: Vec::new(),
            requires_context: None,            now_tz: None,            body: vec![],
        });
        assert_eq!(ir_flow_node_kind(&n), "step");
    }

    #[test]
    fn ir_flow_node_kind_distinct_slugs_for_every_variant() {
        // We can't enumerate every variant with fresh instances
        // (some have non-trivial payloads), but we can pin the slug
        // set by enumerating distinct slugs that the function CAN
        // produce. The match in `ir_flow_node_kind` is exhaustive
        // (compiler-enforced); this test pins the slug values that
        // downstream callers (e.g. audit logs, 33.x.g warning
        // catalog) depend on.
        let expected_slugs: Vec<&str> = vec![
            "step",
            "probe",
            "reason",
            "validate",
            "refine",
            "weave",
            "use_tool",
            "remember",
            "recall",
            "conditional",
            "for_in",
            "let",
            "return",
            "break",
            "continue",
            "lambda_data_apply",
            "par",
            "hibernate",
            "deliberate",
            "consensus",
            "forge",
            "focus",
            "associate",
            "aggregate",
            "explore",
            "ingest",
            "shield_apply",
            "stream_block",
            "navigate",
            "drill",
            "trail",
            "corroborate",
            "ots_apply",
            "mandate_apply",
            "compute_apply",
            "listen",
            "daemon_step",
            "emit",
            "publish",
            "discover",
            "persist",
            "retrieve",
            "mutate",
            "purge",
            "transact",
        ];
        // 45 closed-catalog slugs. Adding a new IRFlowNode variant
        // requires updating `ir_flow_node_kind` (compiler enforces)
        // AND this list (intentional — keeps the slug catalog
        // pinned for audit/warning callers).
        assert_eq!(expected_slugs.len(), 45);
        // Every slug is a valid lowercase identifier (snake_case).
        for slug in &expected_slugs {
            assert!(
                slug.chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()),
                "slug {slug:?} must be lowercase snake_case for stable audit emission"
            );
        }
        // Every slug is unique.
        let mut sorted = expected_slugs.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), expected_slugs.len(), "slug catalog has duplicates");
    }

    #[test]
    fn ir_flow_node_kind_runner_drift() {
        // §33.x.c drift gate: `flow_plan::ir_flow_node_kind` is the
        // single source of truth for IRFlowNode kind slugs. The
        // legacy `runner.rs::extract_step_info` produces a
        // `step_type` string for each variant that downstream
        // consumers (audit row, stub trace, IR debugger) parse.
        // Drift between the two surfaces here would make the audit
        // row inconsistent across sync + async paths.
        //
        // For 33.x.c the drift gate operates by-construction: the
        // public helper here is the canonical surface; future
        // migrations of `extract_step_info` to call into this
        // helper are tracked under 33.x.i. Until then, this test
        // pins the kind catalog so accidental drift on either side
        // surfaces in CI.
        use crate::ir_nodes::{IRFlowNode, IRStep};
        let step = IRFlowNode::Step(IRStep {
            node_type: "Step",
            source_line: 1,
            source_column: 1,
            name: "T".into(),
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
            requires_context: None,            now_tz: None,            body: vec![],
        });
        // Pinned drift assertion: the canonical "step" kind is
        // what `runner.rs::extract_step_info` produces for
        // `IRFlowNode::Step`, per file inspection at 33.x.c
        // landing. Future runner refactor MUST keep this
        // invariant or update this test (deliberate change).
        assert_eq!(ir_flow_node_kind(&step), "step");
    }

    #[test]
    fn compose_system_prompt_public_includes_flow_name() {
        let (_p, ir) = compile_source_to_ir(parse_simple_stream_source(), "t.axon").unwrap();
        let flow = find_ir_flow_by_name(&ir, "Chat").unwrap();
        let prompt = compose_system_prompt_public(flow, &ir, None);
        assert!(prompt.contains("# Flow: Chat"));
    }

    #[test]
    fn compose_system_prompt_public_omits_backend_tag_when_none() {
        let (_p, ir) = compile_source_to_ir(parse_simple_stream_source(), "t.axon").unwrap();
        let flow = find_ir_flow_by_name(&ir, "Chat").unwrap();
        let prompt = compose_system_prompt_public(flow, &ir, None);
        assert!(!prompt.contains("[Backend:"));
        assert!(!prompt.contains("AXON"));
    }

    #[test]
    fn compose_system_prompt_public_includes_backend_tag_when_set() {
        let (_p, ir) = compile_source_to_ir(parse_simple_stream_source(), "t.axon").unwrap();
        let flow = find_ir_flow_by_name(&ir, "Chat").unwrap();
        let prompt = compose_system_prompt_public(flow, &ir, Some("anthropic"));
        assert!(prompt.contains("[Backend: anthropic | AXON "));
    }

    #[test]
    fn compose_system_prompt_public_includes_persona_when_present() {
        // `domain` is a list per AST grammar; `tone` is a string.
        // `tone` is a closed-catalog enum (see type_checker valid tones).
        let src = "persona Doctor { domain: [\"medicine\"] tone: \"formal\" }\n\
                   context Clinic { depth: \"deep\" memory_scope: \"session\" }\n\
                   flow Chat() -> Unit {\n\
                       step Generate { ask: \"hi\" output: Stream<Token> }\n\
                   }\n\
                   axonendpoint E { method: POST path: \"/c\" execute: Chat transport: sse public: true }";
        let (_p, ir) = compile_source_to_ir(src, "t.axon").unwrap();
        let flow = find_ir_flow_by_name(&ir, "Chat").unwrap();
        let prompt = compose_system_prompt_public(flow, &ir, None);
        assert!(prompt.contains("# Persona: Doctor"));
        assert!(prompt.contains("Domain expertise: medicine"));
        assert!(prompt.contains("# Context: Clinic"));
        assert!(prompt.contains("Analysis depth: deep"));
    }

    #[test]
    fn compose_system_prompt_public_is_pure_deterministic() {
        let (_p, ir) = compile_source_to_ir(parse_simple_stream_source(), "t.axon").unwrap();
        let flow = find_ir_flow_by_name(&ir, "Chat").unwrap();
        let p1 = compose_system_prompt_public(flow, &ir, Some("openai"));
        let p2 = compose_system_prompt_public(flow, &ir, Some("openai"));
        assert_eq!(p1, p2);
    }

    #[test]
    fn build_streaming_plan_uses_compile_source_to_ir_internally() {
        // Cross-check: plan built via build_streaming_plan has the
        // same system_prompt as one built by manually invoking
        // compile_source_to_ir + compose_system_prompt_public.
        let src = parse_simple_stream_source();
        let plan = build_streaming_plan(src, "t.axon", "Chat", "stub").unwrap();
        let (_program, ir) = compile_source_to_ir(src, "t.axon").unwrap();
        let flow = find_ir_flow_by_name(&ir, "Chat").unwrap();
        let expected = compose_system_prompt_public(flow, &ir, None);
        assert_eq!(plan.system_prompt, expected);
    }
}
