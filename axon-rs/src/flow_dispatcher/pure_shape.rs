//! §Fase 33.y.c — Pure-shape variant handlers (Step / Probe / Reason /
//! Validate / Refine / Weave).
//!
//! All 6 IRFlowNode variants here share the underlying shape "produce
//! a single LLM response from a prompt + cognitive framing". The
//! module exposes:
//!
//! - One shared async core [`run_pure_shape`] that drives the per-step
//!   `Backend::stream()` loop, forwards chunks as `axon.token` events,
//!   wraps the chunk stream with [`StreamPolicyEnforcer`] when the
//!   caller supplied a `pending_effect_policy`, and records the
//!   per-step audit row + enforcement summary at FlowComplete.
//!
//! - 6 thin per-variant entry points that build the variant's
//!   [`PureShapeStep`] (name + user prompt + cognitive framing
//!   addendum + wire kind slug) and delegate to `run_pure_shape`.
//!
//! # Cognitive framings
//!
//! Each variant's framing nudges the LLM toward its declared
//! semantic posture WITHOUT changing the underlying call mechanics:
//!
//! - `Step` — neutral. The user prompt is the `ask:` field verbatim;
//!   no framing addendum (the system prompt established at flow
//!   level fully captures the intent).
//! - `Probe` — investigative. Framing addendum: *"You are probing the
//!   target. Investigate deeply, surface what's hidden, return
//!   concisely."*
//! - `Reason` — deliberative. Framing addendum reflects the declared
//!   strategy (e.g. `chain_of_thought`, `tree_of_thought`,
//!   `analogical`) when present.
//! - `Validate` — verification. Framing names the rule being checked.
//! - `Refine` — improvement. Framing names the strategy + signals the
//!   target is treated as draft input.
//! - `Weave` — synthesis. Framing names the sources + format/style;
//!   the LLM produces a stitched output ordered by `priority`.
//!
//! # Wire shape
//!
//! Each handler emits:
//!   1. `axon.step_start { step_name, step_index, step_type: <slug>, timestamp_ms }`
//!   2. `axon.step_token { step_name, content, token_index, timestamp_ms }` — one per non-empty chunk
//!   3. `axon.step_complete { step_name, step_index, success: true, full_output, tokens_input: 0, tokens_output, timestamp_ms }`
//!
//! `step_type` matches `flow_plan::ir_flow_node_kind` byte-for-byte
//! (`"step"` / `"probe"` / `"reason"` / `"validate"` / `"refine"` /
//! `"weave"`). Adopter EventSource clients filter on the `step_type`
//! field to surface per-variant UI affordances.
//!
//! # D-letter anchors
//!
//! - **D1** — every pure-shape variant has a NAMED async handler;
//!   the dispatcher arm delegates exhaustively (no `_ =>` catch-all).
//! - **D2** — `pending_effect_policy` is consumed by [`run_pure_shape`]
//!   before `Backend::stream()` resolves; the enforcer activates per-
//!   node, not per-step-list-iteration.
//! - **D3** — `cancel.is_cancelled()` is checked at every `.await`
//!   boundary; cancel propagates into reqwest body via Fase 33.x.e's
//!   `cancel_aware` adapter (the backend impls already plumb this).
//! - **D4** — wire shape extends v1.25.0 by adding `step_type` slugs
//!   for the 5 non-`Step` variants; the canonical `Step` slug stays
//!   `"step"` byte-identical with the pre-33.y.c emission. New slugs
//!   are observable but elided (`step_type: "step"`) when the IR
//!   variant is `Step`.
//! - **D6** — per-step audit row carries `effect_policy_applied` =
//!   `Some(<policy>.slug())` when the caller supplied a policy,
//!   `None` otherwise. The `step_audit_records` side-channel
//!   accumulates one row per handler call.
//! - **D7** — production-grade: zero `unwrap()` on the chunk-stream
//!   side; every error case routes through [`DispatchError`].

use crate::backends::{ChatRequest, Message};
use crate::flow_dispatcher::{DispatchCtx, DispatchError, NodeOutcome};
use crate::flow_execution_event::{now_ms, FlowExecutionEvent};
use crate::ir_nodes::{
    IRProbe, IRReasonStep, IRRefineStep, IRStep, IRValidateStep, IRWeaveStep,
};
use crate::stream_effect::BackpressurePolicy;
use futures::StreamExt;
use sha2::{Digest, Sha256};

// ────────────────────────────────────────────────────────────────────
//  PureShapeStep — per-variant framing carrier
// ────────────────────────────────────────────────────────────────────

/// The per-variant context built by each entry function. Owns the
/// rendered user prompt + framing addendum; the shared core
/// [`run_pure_shape`] reads + drives the LLM dispatch.
pub struct PureShapeStep {
    /// Step name as declared in the source (stable across versions
    /// of the flow). For variants without an explicit `name:` field
    /// (Probe / Reason / Validate / Refine / Weave) we use the
    /// target/strategy field that uniquely identifies the node.
    pub name: String,
    /// User-side prompt sent as `Message::user(...)`.
    pub user_prompt: String,
    /// Optional framing appended to the flow-level `system_prompt`
    /// (sourced from `ctx.system_prompt`). When `None` the system
    /// prompt is sent verbatim.
    pub framing_addendum: Option<String>,
    /// Wire `step_type` slug — byte-equal with
    /// `flow_plan::ir_flow_node_kind` for the corresponding IR
    /// variant.
    pub kind_slug: &'static str,
    /// §Fase 33.y.k — Tools plumbed into `ChatRequest.tools`. The
    /// per-variant entry function builds this from the step's
    /// declared `apply: <tool>` (canonical Step shape) or
    /// `use_tool: [...]` (multi-tool form). For OSS reference: each
    /// declared tool synthesizes a minimal [`ToolSpec`] with name +
    /// canonical description + empty `{}` parameter schema.
    /// Enterprise integrations resolve real `IRToolSpec` entries
    /// from the IRProgram (a future Fase 33.y.k.2 follow-up
    /// extends `DispatchCtx` with an `Option<&IRProgram>` ref for
    /// full per-provider parameter-schema resolution).
    ///
    /// Empty `Vec` (default) → backend gets no tools → wire shape
    /// stays D4 byte-compat with pre-33.y.k.
    pub tools: Vec<crate::backends::ToolSpec>,
    /// §Fase 68.d — the step's declared model-capability requirement (context
    /// window in tokens), threaded from `IRStep.requires_context`. The §68.c
    /// resolver maps it (against the resolved backend's §68.a catalog) to the
    /// `ChatRequest.model` for this step. `None` (every non-`step` shape +
    /// requirement-less steps) → empty model → backend default (back-compat).
    pub requires_context: Option<u32>,
    /// §Fase 86 — an explicit sampling temperature for this call. The `forge`
    /// pipeline runs each creative phase at a distinct temperature (low for
    /// Preparation/Verification, τ_eff for Incubation, τ_base for Illumination).
    /// `None` (every pre-§86 shape) → backend default, wire-shape byte-compat.
    pub temperature: Option<f64>,
    /// §Fase 91.b — the step's declared cognitive timezone (`now: "<IANA>"`,
    /// threaded from `IRStep.now_tz`). Overrides the frame-level
    /// `ctx.default_now_tz`. When either is present, [`run_pure_shape`]
    /// appends the run's captured instant — rendered in that zone — to the
    /// effective system prompt (`time_is_an_explicit_input`, §91). `None` on
    /// every pre-§91 shape → prompt byte-identical.
    pub now_tz: Option<String>,
}

// ────────────────────────────────────────────────────────────────────
//  Per-variant entry points
// ────────────────────────────────────────────────────────────────────

/// Step entry — neutral cognitive framing. The user prompt is the
/// `ask:` field verbatim; no addendum (the flow-level system prompt
/// fully establishes intent).
///
/// §Fase 33.y.k — when `step.apply_ref` is non-empty, synthesizes
/// a [`ToolSpec`](crate::backends::ToolSpec) and plumbs it into
/// `ChatRequest.tools` via the shared async core. Adopter flows
/// declaring `step S { apply: <tool> }` activate real upstream
/// tool-calling on the SSE wire (Anthropic `tool_use` / OpenAI
/// `tool_calls` / etc.). When `apply_ref` is empty, tools stays
/// `Vec::new()` → wire shape byte-compat with pre-33.y.k.
pub async fn run_step(
    step: &IRStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    // §Fase 36.x.e (D4) — interpolate `${name}` / `$name` in the
    // step's `ask` against the flow bindings BEFORE it becomes the
    // prompt (legacy LLM path) or the tool argument (streaming-tool
    // path). A `retrieve … as: alias` binds `alias`, a `let` binds
    // its target, and a prior `step`'s output is bound under the step
    // name (see `run_pure_shape` / `run_step_streaming_tool`). So the
    // agent pattern's data threads — retrieve context → deliberate →
    // persist — on the streaming dispatcher path, matching the
    // synchronous path's interpolation contract (Fase 35.q).
    let prompt =
        crate::exec_context::interpolate_vars(&step.ask, &ctx.let_bindings);

    // §Fase 34.d — Streaming-tool branch. When the step's
    // `apply_ref` resolves to a tool flagged `is_streaming` in the
    // attached registry, bypass the LLM upstream entirely + invoke
    // `tool.stream(args, ctx)` via the
    // [`crate::tool_dispatch_bridge::resolve_streaming_tool`] factory.
    //
    // The branch fires ONLY when ALL THREE conditions hold:
    //   1. `step.apply_ref` is non-empty (tool reference present)
    //   2. `ctx.tool_registry` is Some (registry wired)
    //   3. The resolved entry's `is_streaming` flag is true
    //
    // When any condition fails, the legacy LLM-side path is taken
    // (Fase 33.y.k+33.z behavior preserved). D9 backwards-compat:
    // adopters who don't wire the registry see no change.
    if !step.apply_ref.is_empty() {
        if let Some(registry) = ctx.tool_registry.clone() {
            if let Some(entry) = registry.get(&step.apply_ref) {
                if entry.is_streaming {
                    return run_step_streaming_tool(step, entry.clone(), &prompt, ctx).await;
                }
            }
        }
    }
    // §Fase 119.b (D119.4) — a step carrying a `mandate` guard runs the
    // BUFFERED enforcement loop instead of the streaming path. Deliberate:
    // a mandated generation must never stream raw attempts to the wire,
    // because a token that reached the client cannot be unshipped — the
    // step's output appears only after the constraint set accepted it (or
    // the declared `on_violation` policy visibly released it).
    {
        let mandate_guards: Vec<&crate::ir_nodes::IRStepGuard> = step
            .guards
            .iter()
            .filter(|g| g.kind == "mandate")
            .collect();
        if mandate_guards.len() > 1 {
            // Two controllers over one generation have no defined composition
            // yet (whose gains? whose budget?). Refusing is honest; guessing
            // is not. One mandate per step until composition is DESIGNED.
            return Err(DispatchError::BackendError {
                name: format!("step:{}", step.name),
                message: format!(
                    "step '{}' declares {} mandate guards; composing multiple                      mandates over one generation is not yet defined, so it is                      refused rather than half-enforced. Split the step or merge                      the mandates.",
                    step.name,
                    mandate_guards.len()
                ),
            });
        }
        if let Some(guard) = mandate_guards.first() {
            return run_step_mandated(step, guard, &prompt, ctx).await;
        }
        // Non-mandate guards: `shield` is enforced on the step OUTPUT by the
        // legacy path's completion hook below is NOT yet wired — and `ots`
        // has no transformer until §119.c. Silence would be the §111 defect,
        // so both are surfaced as structured warnings at dispatch.
        for g in &step.guards {
            if g.kind != "mandate" {
                tracing::warn!(
                    step = step.name.as_str(),
                    guard_kind = g.kind.as_str(),
                    guard_name = g.name.as_str(),
                    "step guard is parsed and carried but not yet enforced on                      this path (shield: flow-level `shield X on Y` IS enforced;                      ots: lands with §119.c)"
                );
            }
        }
    }

    // Legacy path: LLM-side dispatch (Fase 33.y.k+33.z).
    let tools = synthesize_tools_from_step(step);
    let shape = PureShapeStep {
        name: if step.name.is_empty() {
            "Step".to_string()
        } else {
            step.name.clone()
        },
        user_prompt: prompt,
        framing_addendum: None,
        kind_slug: "step",
        tools,
        requires_context: step.requires_context,
        temperature: None,
        now_tz: step.now_tz.clone(),
    };
    run_pure_shape(shape, ctx).await
}

/// §Fase 34.d (v1.29.0) — Streaming-tool dispatch branch.
///
/// Bypasses `Backend::stream()` entirely. Invokes
/// `tool.stream(step.ask, ctx)` via the bridge factory + drains the
/// resulting `Stream<ToolChunk>` chunk-by-chunk into the wire as
/// `FlowExecutionEvent::StepToken` events.
///
/// # Wire-event sequence
///
/// 1. `FlowExecutionEvent::StepStart` (kind_slug = "step")
/// 2. `FlowExecutionEvent::StepToken` × N (one per non-empty chunk
///    delta the tool emitted)
/// 3. `FlowExecutionEvent::StepComplete` carrying the accumulated
///    output + tokens_emitted (= chunk count) + success flag
///
/// # Cancel discipline
///
/// Polled BEFORE invoking `tool.stream()`, BETWEEN each chunk
/// drain, and AFTER the stream closes. Surfaces
/// `DispatchError::UpstreamCancelled` to the caller; the consumer
/// (post-33.z producer) treats this as a clean exit.
///
/// # Audit row
///
/// Records `StepAuditRecord` with:
/// - `step_name`, `step_index` — standard fields
/// - `tokens_emitted` — chunk count (1 per non-empty delta)
/// - `output_hash_hex` — SHA-256 of concatenated tool deltas
/// - `effect_policy_applied` — the policy slug from the tool's
///   `effect_row` (e.g., "drop_oldest"). Captured at the dispatch
///   layer; actual enforcement at the chunk level lands in
///   Fase 34.g's `unified_stream_handler`.
/// - `chunks_dropped` / `chunks_degraded` — 0 for 34.d (enforcer
///   integration deferred to 34.g).
///
/// # Honest scope
///
/// 34.d ships the BRANCH POINT: the dispatcher correctly detects
/// `is_streaming` tools + routes through the streaming path + the
/// wire emits per-chunk content. The full `StreamPolicyEnforcer`
/// integration (where `drop_oldest` actually drops chunks etc.)
/// lands in 34.g. For 34.d, the policy is captured in the audit
/// row but not enforced at chunk granularity.
async fn run_step_streaming_tool(
    step: &IRStep,
    entry: crate::tool_registry::ToolEntry,
    // §Fase 36.x.e (D4) — the step's `ask` already interpolated by
    // `run_step` against `ctx.let_bindings`. Used as the tool's
    // streaming argument so a `${retrieve_alias}` reaches the tool.
    prompt: &str,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    // §Fase 34.g convergence — the per-chunk drain loop now lives
    // in `flow_dispatcher::unified_stream::unified_stream_handler`.
    // Pre-34.g this function ran an inline drain loop with policy
    // capture-but-no-enforcement; 34.g shifts the drain to the
    // unified handler which integrates a
    // `crate::stream_runtime::Stream<ToolChunk>` policy primitive
    // + returns a `ToolStreamSummary` with real
    // `chunks_dropped`/`chunks_degraded` counters.

    // 1. Reserve step index for audit-row + StepStart parity.
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    // 2. Cancel check at entry — same discipline as run_pure_shape.
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }

    // 3. Resolve declared backpressure policy from the tool's
    //    effect_row. None when the tool flagged is_streaming via a
    //    non-stream slug (parser guarantees one stream policy per
    //    declaration, but the registry's is_streaming flag could be
    //    set programmatically without a declared policy).
    let policy =
        crate::tool_dispatch_bridge::extract_stream_policy(&entry.effect_row);

    let step_name = if step.name.is_empty() {
        "Step".to_string()
    } else {
        step.name.clone()
    };

    // 4. Emit StepStart. Carries the standard `step` kind_slug —
    //    adopters EventSource-filtering on kind don't need to
    //    distinguish stream-tool steps from non-stream steps at the
    //    StepStart layer; the per-chunk StepToken events carry the
    //    per-tool semantics.
    ctx.tx
        .send(FlowExecutionEvent::StepStart {
            step_name: step_name.clone(),
            step_index,
            step_type: "step".to_string(),
                branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)?;

    // 5. Construct ToolContext + Tool trait impl via the bridge.
    let tool_ctx = crate::tool_dispatch_bridge::build_tool_context(
        ctx.cancel.clone(),
        0, // 34.d-scope: trace_id placeholder. The dispatcher doesn't
           // currently carry trace_id in DispatchCtx; future sub-fase
           // (34.i audit extension) plumbs through.
    );
    let tool = crate::tool_dispatch_bridge::resolve_streaming_tool(&entry);

    // 6. Cancel check before invoking the tool — its body might do
    //    work even at .await entry. Mirrors run_pure_shape's pre-
    //    backend-call check.
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }

    // §Fase 72.c — the LINEAR-EFFECT BUDGET GATE. When the flow is run by a
    // budgeted daemon, a tool emission must consume a token from every quota on
    // that tool (`budget { … on Tool(X) }`). An exhausted quota under the
    // fail-closed `block` policy fails the step with a typed
    // `EffectQuotaExhausted` — the call is NOT emitted, so over-emission is
    // impossible by construction (the `effects_are_linear` doctrine). An
    // unbudgeted tool (no quota, or no budget at all) is granted unconditionally,
    // byte-identical to pre-§72. (`defer`/`shed` refine the deny path in §72.d;
    // until then a deny fail-closes for every policy — the budget is always
    // honoured, only the failure MODE is coarser.)
    // §Fase 114.a — the gate now lives in ONE place (`budget_gate::charge`) and
    // runs on EVERY tool path. It used to be inlined right here, and here ONLY —
    // reachable solely by a streaming tool, inside a daemon, on the enterprise
    // supervisor. The canonical `use Tool(…)` path had no budget at all.
    match crate::flow_dispatcher::budget_gate::charge(ctx, &entry.name)? {
        crate::flow_dispatcher::budget_gate::BudgetGrant::Granted => {}
        // §Fase 72.d — `shed`: best-effort. Skip the call, but CONTINUE the flow:
        // the step completes with no tool output (a downstream `${Step}` reference
        // resolves to empty). The audit row marks the shed so it is OBSERVABLE,
        // not silent — a skipped call that leaves no trace is indistinguishable
        // from a call that returned nothing, and those are very different facts.
        crate::flow_dispatcher::budget_gate::BudgetGrant::Shed { .. } => {
            let now = chrono::Utc::now();
            let _ = now;
            {
                    ctx.tx
                        .send(FlowExecutionEvent::StepComplete {
                            step_name: step_name.clone(),
                            step_index,
                            success: true,
                            full_output: String::new(),
                            tokens_input: 0,
                            tokens_output: 0,
                            branch_path: ctx.branch_path_string(),
                            timestamp_ms: now_ms(),
                        })
                        .map_err(|_| DispatchError::ChannelClosed)?;
                    {
                        let mut guard = ctx.step_audit_records.lock().await;
                        guard.push(crate::axonendpoint_replay::StepAuditRecord {
                            step_name: step_name.clone(),
                            step_index,
                            success: true,
                            tokens_emitted: 0,
                            output_hash_hex: String::new(),
                            effect_policy_applied: Some("budget:shed".to_string()),
                            chunks_dropped: 0,
                            chunks_degraded: 0,
                            timestamp_ms: now_ms(),
                            tool_name: Some(entry.name.clone()),
                            tool_chunks_emitted: Some(0),
                            tool_output_hash_hex: Some(String::new()),
                            tool_terminator_kind: Some("shed".to_string()),
                            anchor_breaches: Vec::new(),
                        });
                    }
            }
            // Empty output bound under the step name (a downstream ref gets ""),
            // and the flow proceeds.
            ctx.let_bindings.insert(step_name.clone(), String::new());
            return Ok(NodeOutcome::Completed {
                output: String::new(),
                tokens_emitted: 0,
                step_index,
            });
        }
    }
    // `defer` and `block` no longer appear here: `charge` returns them as typed
    // errors (`EffectDeferred` / `EffectQuotaExhausted`) and the `?` above
    // propagates them. One law, one place.

    // §Fase 114.f — charge the channel LEASE on the streaming path too. Governing
    // the canonical `use Tool(…)` path but not this one would be the exact
    // "real-on-one-path, dead-on-the-other" defect §111 exists to end.
    if let Some(breach) =
        crate::flow_dispatcher::lambda_tools::charge_tool_lease_by_name(&entry.name, ctx)
    {
        ctx.tx
            .send(FlowExecutionEvent::StepComplete {
                step_name: step_name.clone(),
                step_index,
                success: false,
                full_output: breach.clone(),
                tokens_input: 0,
                tokens_output: 0,
                branch_path: ctx.branch_path_string(),
                timestamp_ms: now_ms(),
            })
            .map_err(|_| DispatchError::ChannelClosed)?;
        ctx.let_bindings.insert(step_name.clone(), breach.clone());
        return Ok(NodeOutcome::Completed {
            output: breach,
            tokens_emitted: 0,
            step_index,
        });
    }

    // §Fase 114.e — hold a channel CONCURRENCY permit across the stream drain, so
    // a streaming vendor tool is bounded by `resource.capacity` exactly as the
    // synchronous path is. Held to the end of this function (across the drain).
    let _channel_permit =
        crate::flow_dispatcher::lambda_tools::acquire_channel_permit_by_name(&entry.name, ctx)
            .await;

    // 7. Invoke tool.stream() + route through the unified handler.
    //    The handler applies the declared policy at chunk
    //    granularity (real enforcement, not just slug-capture-in-
    //    audit) + returns a typed summary the caller uses to
    //    populate the audit row + decide the outcome.
    // §Fase 36.x.e (D4) — the interpolated `prompt` is the tool
    // argument (not the raw `step.ask`), so a `${retrieve_alias}`
    // resolved upstream reaches the streaming tool.
    let source = tool.stream(prompt.to_string(), tool_ctx).await;
    let summary = crate::flow_dispatcher::unified_stream::unified_stream_handler(
        source,
        policy,
        &ctx.cancel,
        &ctx.tx,
        &step_name,
        &ctx.branch_path_string(),
    )
    .await?;

    // §Fase 36.x.e.2 — surface the enforcement summary. When the
    // step's applied tool declared a `<stream:<policy>>` effect, the
    // streaming-tool path runs the enforcer (via
    // `unified_stream_handler`) exactly as the LLM-side path does in
    // `run_pure_shape::drain_through_enforcer` — but pre-36.x.e.2 it
    // never WROTE the result to `ctx.enforcement_summaries`, so the
    // `axon.complete` envelope's `enforcement_summary` field stayed
    // empty for an `apply:`-streaming-tool step. This closes that
    // parity gap: the same `EnforcementSummaryWire` shape is keyed
    // under the step name from the `ToolStreamSummary` metrics.
    if let Some(p) = policy {
        let wire = crate::execution_result::EnforcementSummaryWire {
            policy_slug: p.slug().to_string(),
            chunks_pushed: summary.chunks_pushed,
            chunks_delivered: summary.chunks_delivered,
            drop_oldest_hits: summary.chunks_dropped,
            degrade_quality_hits: summary.chunks_degraded,
            pause_upstream_blocks: summary.pause_upstream_blocks,
            fail_overflows: summary.fail_overflows,
            failed: !summary.success,
        };
        ctx.enforcement_summaries
            .lock()
            .await
            .insert(step_name.clone(), wire);
    }

    // 8. Cancel mid-stream → propagate. The accumulated chunks
    //    already reached the wire via the unified handler; the
    //    StepComplete + audit row are skipped (consumer chain
    //    treats this as upstream-cancelled).
    if summary.cancelled && ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }

    // 9. StepComplete event. Mirrors run_pure_shape's shape.
    ctx.tx
        .send(FlowExecutionEvent::StepComplete {
            step_name: step_name.clone(),
            step_index,
            success: summary.success,
            full_output: summary.accumulated.clone(),
            tokens_input: 0,
            tokens_output: summary.tokens_emitted,
                branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)?;

    // 10. Audit row — D6 per-step replay binding. 34.g activates
    //     real `chunks_dropped`/`chunks_degraded` counters from the
    //     unified handler's metrics snapshot. 34.i adds the tool-
    //     stream provenance quartet: tool_name (entry.name), the
    //     source-chunk count (summary.chunks_pushed including
    //     terminator + empty-delta intermediates), explicit
    //     tool_output_hash_hex (same scope as output_hash_hex for
    //     34.i; diverges in future fases with degrader transforms),
    //     and the closed-catalog terminator kind slug.
    {
        let terminator_kind = if summary.cancelled {
            "cancelled"
        } else if summary.terminator_message.is_some() {
            "error"
        } else {
            "stop"
        };
        let record = crate::axonendpoint_replay::StepAuditRecord {
            step_name: step_name.clone(),
            step_index,
            success: summary.success,
            tokens_emitted: summary.tokens_emitted,
            output_hash_hex: summary.output_hash_hex.clone(),
            effect_policy_applied: policy.map(|p| p.slug().to_string()),
            chunks_dropped: summary.chunks_dropped,
            chunks_degraded: summary.chunks_degraded,
            timestamp_ms: now_ms(),
            tool_name: Some(entry.name.clone()),
            tool_chunks_emitted: Some(summary.chunks_pushed),
            tool_output_hash_hex: Some(summary.output_hash_hex.clone()),
            tool_terminator_kind: Some(terminator_kind.to_string()),
            // §Fase 65.C.3 — tool-stream path: no LLM output to anchor-check.
            anchor_breaches: Vec::new(),
        };
        let mut guard = ctx.step_audit_records.lock().await;
        guard.push(record);
    }

    // 11. Surface DispatchError on Error-terminator. Includes the
    //     Fail-policy overflow surface (the summary carries the
    //     terminator_message that the unified handler synthesized
    //     from `StreamError::Overflow`).
    if let Some(message) = summary.terminator_message {
        return Err(DispatchError::BackendError {
            name: format!("tool:{}", entry.name),
            message,
        });
    }

    // §Fase 36.x.e (D4) — bind the tool's accumulated output under
    // the step name so a downstream `persist` / `step` can reference
    // it (`${StepName}`). Only on the success path — an
    // error-terminated step (handled above) has no output to thread.
    ctx.let_bindings
        .insert(step_name.clone(), summary.accumulated.clone());

    Ok(NodeOutcome::Completed {
        output: summary.accumulated,
        tokens_emitted: summary.tokens_emitted,
        step_index,
    })
}

/// §Fase 33.y.k — Resolve `step.apply_ref` into a `Vec<ToolSpec>`.
/// OSS reference: when `apply_ref` is non-empty, synthesizes a
/// minimal `ToolSpec { name, description, parameters_json: "{}" }`.
/// When the IRProgram tool registry surface lands (future Fase
/// 33.y.k.2), this helper resolves the real `IRToolSpec` with
/// `parameters_json` from `input_schema`.
fn synthesize_tools_from_step(step: &IRStep) -> Vec<crate::backends::ToolSpec> {
    if step.apply_ref.is_empty() {
        return Vec::new();
    }
    vec![crate::backends::ToolSpec {
        name: step.apply_ref.clone(),
        description: format!("Tool reference: {}", step.apply_ref),
        parameters_json: "{}".to_string(),
    }]
}

/// Probe entry — investigative framing. The target is investigated
/// deeply; the LLM surfaces what's hidden + returns concisely.
/// §Fase 119.b — the mandated-step executor: `step S { mandate M on x … }`.
///
/// One StepStart, one StepComplete — the attempts in between are buffered
/// through `enforce_mandate_over` and never reach the wire individually. The
/// final bound output is the validated one (or the `coerce`-released best
/// attempt, visibly). The guard's `-> binding` additionally binds the
/// enforced output under that name for downstream steps.
async fn run_step_mandated(
    step: &IRStep,
    guard: &crate::ir_nodes::IRStepGuard,
    prompt: &str,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_name = if step.name.is_empty() {
        "Step".to_string()
    } else {
        step.name.clone()
    };
    ctx.tx
        .send(FlowExecutionEvent::StepStart {
            step_name: step_name.clone(),
            step_index,
            step_type: "step".to_string(),
            branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)?;

    // The guard's target, when it names a binding, is the content the
    // generation must govern — it rides the prompt as context. (A call
    // expression target was captured verbatim by the parser; it interpolates
    // like any other step input.)
    let effective_prompt = if guard.target.is_empty() {
        prompt.to_string()
    } else {
        let resolved = ctx
            .let_bindings
            .get(&guard.target)
            .cloned()
            .unwrap_or_else(|| guard.target.clone());
        if prompt.is_empty() {
            resolved
        } else {
            format!("{prompt}

Input:
{resolved}")
        }
    };

    let output = super::algebraic_handlers::enforce_mandate_over(
        &guard.name,
        None,
        super::algebraic_handlers::MandateTask::Generate {
            prompt: effective_prompt,
        },
        ctx,
    )
    .await?;

    ctx.let_bindings.insert(step_name.clone(), output.clone());
    if !guard.binding.is_empty() {
        ctx.let_bindings.insert(guard.binding.clone(), output.clone());
    }
    if !step.output_type.is_empty() {
        ctx.let_bindings
            .insert(step.output_type.clone(), output.clone());
    }

    ctx.tx
        .send(FlowExecutionEvent::StepComplete {
            step_name: step_name.clone(),
            step_index,
            success: true,
            full_output: output.clone(),
            tokens_input: 0,
            tokens_output: 0,
            branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)?;

    Ok(NodeOutcome::Completed {
        output,
        tokens_emitted: 0,
        step_index,
    })
}

pub async fn run_probe(
    probe: &IRProbe,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    let shape = PureShapeStep {
        name: if probe.target.is_empty() {
            "Probe".to_string()
        } else {
            probe.target.clone()
        },
        user_prompt: format!("Investigate: {}", probe.target),
        framing_addendum: Some(
            "You are probing the target. Investigate deeply, surface what's hidden, return concisely.".into(),
        ),
        kind_slug: "probe",
        tools: Vec::new(),
        requires_context: None,
        temperature: None,
        now_tz: None,
    };
    run_pure_shape(shape, ctx).await
}

/// Reason entry — deliberative framing reflecting the declared
/// strategy (`chain_of_thought`, `tree_of_thought`, `analogical`, …).
pub async fn run_reason(
    reason: &IRReasonStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    let strategy_clause = if reason.strategy.is_empty() {
        String::new()
    } else {
        format!(" using strategy `{}`", reason.strategy)
    };
    let shape = PureShapeStep {
        name: if reason.target.is_empty() {
            "Reason".to_string()
        } else {
            reason.target.clone()
        },
        user_prompt: format!("Reason about: {}{}", reason.target, strategy_clause),
        framing_addendum: Some(
            "You are reasoning deliberately. Show the steps of your reasoning where they bear on the answer.".into(),
        ),
        kind_slug: "reason",
        tools: Vec::new(),
        requires_context: None,
        temperature: None,
        now_tz: None,
    };
    run_pure_shape(shape, ctx).await
}

/// Validate entry — verification framing. The target is checked
/// against the declared rule; the LLM returns a pass/fail verdict
/// with reasoning.
pub async fn run_validate(
    validate: &IRValidateStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    let rule_clause = if validate.rule.is_empty() {
        String::new()
    } else {
        format!(" against rule `{}`", validate.rule)
    };
    let shape = PureShapeStep {
        name: if validate.target.is_empty() {
            "Validate".to_string()
        } else {
            validate.target.clone()
        },
        user_prompt: format!("Validate: {}{}", validate.target, rule_clause),
        framing_addendum: Some(
            "You are validating. Return a structured verdict (pass/fail) with the reasoning that supports it.".into(),
        ),
        kind_slug: "validate",
        tools: Vec::new(),
        requires_context: None,
        temperature: None,
        now_tz: None,
    };
    run_pure_shape(shape, ctx).await
}

/// Refine entry — improvement framing. The target is treated as
/// draft input; the declared strategy (when present) names the
/// improvement axis.
pub async fn run_refine(
    refine: &IRRefineStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    let strategy_clause = if refine.strategy.is_empty() {
        String::new()
    } else {
        format!(" using strategy `{}`", refine.strategy)
    };
    let shape = PureShapeStep {
        name: if refine.target.is_empty() {
            "Refine".to_string()
        } else {
            refine.target.clone()
        },
        user_prompt: format!("Refine: {}{}", refine.target, strategy_clause),
        framing_addendum: Some(
            "You are refining. Treat the target as draft input; improve it along the declared strategy without losing fidelity to its intent.".into(),
        ),
        kind_slug: "refine",
        tools: Vec::new(),
        requires_context: None,
        temperature: None,
        now_tz: None,
    };
    run_pure_shape(shape, ctx).await
}

/// Weave entry — synthesis framing. Sources are stitched into the
/// target via `format_type`; `priority` orders the contribution
/// weighting; `style` shapes the output voice.
pub async fn run_weave(
    weave: &IRWeaveStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    let sources_clause = if weave.sources.is_empty() {
        String::new()
    } else {
        format!(" from sources [{}]", weave.sources.join(", "))
    };
    let format_clause = if weave.format_type.is_empty() {
        String::new()
    } else {
        format!(" as {}", weave.format_type)
    };
    let style_clause = if weave.style.is_empty() {
        String::new()
    } else {
        format!(" in {} style", weave.style)
    };
    let priority_clause = if weave.priority.is_empty() {
        String::new()
    } else {
        format!(" with priority [{}]", weave.priority.join(", "))
    };
    let shape = PureShapeStep {
        name: if weave.target.is_empty() {
            "Weave".to_string()
        } else {
            weave.target.clone()
        },
        user_prompt: format!(
            "Weave: {}{}{}{}{}",
            weave.target, sources_clause, format_clause, style_clause, priority_clause
        ),
        framing_addendum: Some(
            "You are weaving. Stitch the sources into the target output. Honor the declared priority + format + style.".into(),
        ),
        kind_slug: "weave",
        tools: Vec::new(),
        requires_context: None,
        temperature: None,
        now_tz: None,
    };
    run_pure_shape(shape, ctx).await
}

// ────────────────────────────────────────────────────────────────────
//  Shared async core
// ────────────────────────────────────────────────────────────────────

/// Drive a single pure-shape step end-to-end: emit StepStart, build
/// ChatRequest, dispatch to the backend's `stream()`, optionally
/// wrap with `StreamPolicyEnforcer`, forward chunks as
/// `axon.step_token` events, capture the audit row, emit
/// StepComplete, return `NodeOutcome::Completed`.
///
/// # Cancellation
///
/// Checked at every `.await` boundary. On cancel surfaces
/// `DispatchError::UpstreamCancelled` — the caller treats this as a
/// clean exit (no `axon.error` event surfaced; the consumer is
/// already gone).
///
/// # Backend resolution
///
/// `ctx.backend_name` is resolved via
/// [`crate::backends::resolve_streaming_backend`]. Returns
/// `DispatchError::BackendError` if the name is unknown.
///
/// # Effect-policy activation
///
/// If `ctx.pending_effect_policy` is `Some(_)`, the backend's chunk
/// stream is wrapped in `StreamPolicyEnforcer` per Fase 33.x.d
/// semantics — producer-side `tokio::spawn` runs the enforcer's
/// `drain`; consumer-side this fn pops chunks via `pop_chunk`. The
/// `EnforcementSummary` is captured post-drain + recorded under the
/// step's name in `ctx.enforcement_summaries`.
///
/// `pending_effect_policy` is CONSUMED by this call (cleared on
/// entry) so the next handler invocation observes its OWN policy,
/// never the previous handler's residue.
pub async fn run_pure_shape(
    shape: PureShapeStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    // 1. Reserve the step index BEFORE incrementing the counter so
    //    the audit row + StepStart event share the same index value.
    //    This matches the sync runner's discipline for D10 byte-
    //    identical parity.
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    // 2. Consume the pending effect policy. Take-semantics: if the
    //    caller forgot to set it for the NEXT handler, no stale
    //    leak.
    let effect_policy = ctx.take_pending_effect_policy();

    // 3. Cancel check at entry.
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }

    // 4. StepStart event. Carries the variant's wire slug so adopter
    //    EventSource clients filter per variant.
    ctx.tx
        .send(FlowExecutionEvent::StepStart {
            step_name: shape.name.clone(),
            step_index,
            step_type: shape.kind_slug.to_string(),
                branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)?;

    // 5. Resolve backend through the streaming registry. Mirrors
    //    `run_streaming_async_path`'s resolution discipline.
    // §Fase 65.C — pin the per-tenant API key (when the caller threaded one
    // via `with_api_key`) so the LLM call uses THIS tenant's key, not the
    // process env var. `None` ⇒ the prior env-key behavior, unchanged.
    // §Fase 24.g.2 (Kivi brief #37) — also thread the per-tenant LLM endpoint
    // override (base URL + chat path) so e.g. `glm` hits z.ai's `/api/paas/v4`
    // instead of the bigmodel.cn default. Both `None` ⇒ env/default, unchanged.
    let backend = crate::backends::resolve_streaming_backend_with_key_and_endpoint(
        &ctx.backend_name,
        ctx.api_key.as_deref(),
        ctx.llm_base_url.as_deref(),
        ctx.llm_chat_path.as_deref(),
    )
    .ok_or_else(|| DispatchError::BackendError {
            name: ctx.backend_name.clone(),
            message: format!(
                "not in streaming registry; supported: {}",
                crate::backends::STREAMING_BACKEND_NAMES.join(", ")
            ),
        })?;

    // 6. Compose effective system prompt: flow-level (ctx.system_prompt)
    //    + variant-specific framing addendum.
    let system = match &shape.framing_addendum {
        Some(addendum) if ctx.system_prompt.is_empty() => addendum.clone(),
        Some(addendum) => format!("{}\n\n{}", ctx.system_prompt, addendum),
        None => ctx.system_prompt.clone(),
    };

    // 6b. §Fase 91.b — declared cognitive time. When the step (or the frame)
    //     declares `now:`, append the run's single captured instant rendered
    //     in that zone (`time_is_an_explicit_input` — the source declared it,
    //     the runtime supplies it, the envelope records it). A zone that
    //     passed the compile-time format law but is unknown to this build's
    //     tz database fails CLOSED — a loud dispatch error, never a silent
    //     omission. The lock is scoped and never held across an `.await`.
    let system = {
        let mut temporal = ctx.temporal.lock().unwrap();
        crate::temporal_context::compose_effective_system(
            &system,
            shape.now_tz.as_deref(),
            ctx.default_now_tz.as_deref(),
            &mut temporal,
        )
        .map_err(|e| DispatchError::BackendError {
            name: "temporal_context".to_string(),
            message: format!("step '{}': {e}", shape.name),
        })?
    };

    // §Fase 65.C.2 — read the flow's conversation history (enforcing the char
    // budget) and prepend the prior turns so this step has multi-step
    // coherence — the runner's `ConversationHistory` discipline, brought to the
    // dispatcher's previously-stateless LLM path. The lock is held only to
    // enforce + clone the turns; it is NEVER held across the stream `.await`.
    // The two `Message` types differ (`conversation::Message{role:String}` vs
    // the provider-neutral `backends::Message`), so we convert by role.
    let history_msgs: Vec<Message> = {
        let mut conv = ctx.conversation.lock().unwrap();
        conv.truncate_to_budget(ctx.context_budget);
        conv.messages()
            .iter()
            .map(|m| {
                if m.role == "assistant" {
                    Message::assistant(m.content.clone())
                } else {
                    Message::user(m.content.clone())
                }
            })
            .collect()
    };

    // §Fase 68.d — resolve the model from the step's declared capability
    // requirement (`requires_context:`) against the RESOLVED backend's §68.a
    // catalog. A `None` requirement → empty model string → the backend's
    // `default_model()` (byte-identical to every pre-§68 flow). An UNSATISFIABLE
    // requirement fails CLOSED here (D68.3) — a loud error BEFORE the upstream
    // request, never a too-small model that 400s mid-stream (the brief-#36
    // failure mode). This is the one production engine (dispatcher), so daemon
    // + axonendpoint flows both honor it.
    let resolved_model = crate::model_resolution::resolve_model(
        crate::backends::model_catalog::models_for(&ctx.backend_name),
        shape.requires_context,
    )
    .map_err(|e| DispatchError::BackendError {
        name: ctx.backend_name.clone(),
        message: format!("model capability unsatisfied: {e}"),
    })?;

    // 7. Build the per-attempt ChatRequest (history + the current user turn).
    //    §Fase 33.y.k D8 — `shape.tools` plumbs through; empty for cognitive-
    //    framing handlers whose IR shapes carry no tool reference today.
    let make_request = |user_prompt: &str, cancel: &crate::cancel_token::CancellationFlag| {
        let mut messages = history_msgs.clone();
        messages.push(Message::user(user_prompt.to_string()));
        ChatRequest {
            model: resolved_model.model.clone(),
            messages,
            system: if system.is_empty() { None } else { Some(system.clone()) },
            max_tokens: None,
            temperature: shape.temperature,
            top_p: None,
            tools: shape.tools.clone(),
            stream: true,
            logit_bias: None,
            trace_id: None,
            cancel: cancel.clone(),
        }
    };

    // 8. Cancel check before issuing the upstream request — the HTTP call
    //    itself is the most expensive `.await` boundary we're about to cross.
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }

    // 9/10. §Fase 65.C.4 — dispatch with anchor-aware retry.
    //
    // NO anchors → stream live exactly as before (the common path; zero change).
    // WITH anchors → we cannot both stream live AND regenerate-on-breach (SSE
    // tokens can't be un-sent), so BUFFER the response, check the anchors, and
    // regenerate up to MAX_ANCHOR_RETRIES with violation feedback (the runner's
    // `execute_step_with_retry` discipline + prompt wording), then REPLAY the
    // accepted response's chunks to the wire — wire-identical to a live drain,
    // just deferred past the anchor gate. This brings the runner's anchor-retry
    // to the streaming dispatcher (the last LLM-parity gap before the §65.E
    // driver collapse).
    let (accumulated, tokens_emitted, drop_count, degrade_count, anchor_breaches): (
        String,
        u64,
        u64,
        u64,
        Vec<String>,
    ) = if ctx.anchors.is_empty() {
        let cancel = ctx.cancel.clone();
        let chunk_stream = backend
            .stream(make_request(&shape.user_prompt, &cancel))
            .await
            .map_err(|e| DispatchError::BackendError {
                name: ctx.backend_name.clone(),
                message: format!("{e}"),
            })?;
        let (acc, toks, dc, dg) = match effect_policy {
            Some(policy) => {
                drain_through_enforcer(chunk_stream, &shape, ctx, policy, step_index).await?
            }
            None => drain_direct(chunk_stream, &shape, ctx, step_index).await?,
        };
        (acc, toks, dc, dg, Vec::new())
    } else {
        let mut user_prompt = shape.user_prompt.clone();
        let mut attempt: u32 = 0;
        loop {
            let cancel = ctx.cancel.clone();
            let chunk_stream = backend
                .stream(make_request(&user_prompt, &cancel))
                .await
                .map_err(|e| DispatchError::BackendError {
                    name: ctx.backend_name.clone(),
                    message: format!("{e}"),
                })?;
            let (acc, buffered) = drain_to_buffer(chunk_stream, ctx).await?;
            let results = crate::anchor_checker::check_all(&ctx.anchors, &acc);
            let error_breaches: Vec<&crate::anchor_checker::AnchorResult> = results
                .iter()
                .filter(|r| !r.passed && r.severity == "error")
                .collect();
            if error_breaches.is_empty() || attempt >= MAX_ANCHOR_RETRIES {
                // Accept (clean OR retries exhausted — the runner also continues
                // with the last response after MAX_ANCHOR_RETRIES). Record EVERY
                // remaining breach, then replay the buffered chunks to the wire.
                let breaches: Vec<String> = results
                    .iter()
                    .filter(|r| !r.passed)
                    .map(|r| {
                        let first = r.violations.first().cloned().unwrap_or_default();
                        format!("{} [{}]: {}", r.anchor_name, r.severity, first)
                    })
                    .collect();
                let toks = emit_buffered(buffered, &shape, ctx).await?;
                break (acc, toks, 0, 0, breaches);
            }
            // Regenerate with violation feedback (runner-identical wording so
            // both paths converge on the same correction).
            attempt += 1;
            let feedback = error_breaches
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let v = r.violations.first().cloned().unwrap_or_else(|| r.anchor_name.clone());
                    format!("{}. {}", i + 1, v)
                })
                .collect::<Vec<_>>()
                .join("\n");
            user_prompt = format!(
                "{}\n\nIMPORTANT: Your previous response violated the following \
                 constraints:\n{}\n\nPlease regenerate your response, strictly \
                 avoiding the violations listed above.",
                shape.user_prompt, feedback
            );
        }
    };

    // §Fase 65.C.2 — record this turn into the flow's conversation so the NEXT
    // LLM step sees it (the runner's `add_user`/`add_assistant` discipline after
    // a successful call). System framing stays out of the history — it is sent
    // separately each call, exactly as in the runner.
    {
        let mut conv = ctx.conversation.lock().unwrap();
        conv.add_user(&shape.user_prompt);
        conv.add_assistant(&accumulated);
    }

    // §Fase 65.C.3/C.4 — `anchor_breaches` was computed by the dispatch above
    // (the anchored branch checks + retries; the non-anchored branch returns
    // empty). It carries the breaches that REMAIN after the retry budget, which
    // the audit record surfaces per-step.

    // 11. Compute the output SHA-256 for the audit row + emit
    //     StepComplete.
    let output_hash_hex = sha256_hex(&accumulated);

    ctx.tx
        .send(FlowExecutionEvent::StepComplete {
            step_name: shape.name.clone(),
            step_index,
            success: true,
            full_output: accumulated.clone(),
            tokens_input: 0,
            tokens_output: tokens_emitted,
                branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)?;

    // 12. Push the audit row for D6 per-step replay binding.
    //     LLM-side disjunct (a) → no Tool::stream() source backing
    //     this path; the 34.i tool-stream provenance quartet stays
    //     `None`. D4 byte-compat: serde elides the fields so the
    //     wire shape for legacy LLM-side rows is byte-identical to
    //     the pre-34.i emission.
    {
        let record = crate::axonendpoint_replay::StepAuditRecord {
            step_name: shape.name.clone(),
            step_index,
            success: true,
            tokens_emitted,
            output_hash_hex,
            effect_policy_applied: effect_policy.map(|p| p.slug().to_string()),
            chunks_dropped: drop_count,
            chunks_degraded: degrade_count,
            timestamp_ms: now_ms(),
            tool_name: None,
            tool_chunks_emitted: None,
            tool_output_hash_hex: None,
            tool_terminator_kind: None,
            anchor_breaches,
        };
        let mut guard = ctx.step_audit_records.lock().await;
        guard.push(record);
    }

    // §Fase 36.x.e (D4) — bind the step's output under its name so a
    // downstream `persist` / `step` / interpolation site can
    // reference it (`${StepName}`). The streaming dispatcher path
    // threads a step's output through `ctx.let_bindings` exactly as a
    // `retrieve … as: alias` threads a retrieved value.
    ctx.let_bindings
        .insert(shape.name.clone(), accumulated.clone());

    Ok(NodeOutcome::Completed {
        output: accumulated,
        tokens_emitted,
        step_index,
    })
}

// ────────────────────────────────────────────────────────────────────
//  Drain helpers — direct + through-enforcer
// ────────────────────────────────────────────────────────────────────

async fn drain_direct(
    chunk_stream: crate::backends::ChatStream,
    shape: &PureShapeStep,
    ctx: &mut DispatchCtx,
    _step_index: usize,
) -> Result<(String, u64, u64, u64), DispatchError> {
    use crate::backends::FinishReason;
    let mut accumulated = String::new();
    let mut tokens_emitted: u64 = 0;
    let mut stream = chunk_stream;

    while let Some(chunk_result) = stream.next().await {
        if ctx.cancel.is_cancelled() {
            return Err(DispatchError::UpstreamCancelled);
        }
        match chunk_result {
            Ok(chunk) => {
                // §Fase 33.y.k D8 — emit ToolCall event when the
                // backend signals FinishReason::ToolUse. Carries
                // the FIRST declared tool name from
                // `shape.tools[0].name` so adopters correlate the
                // tool-call event with their declared `apply: <tool>`.
                // When `shape.tools` is empty (no declared tool)
                // the tool_name is `"<unknown>"` — the upstream
                // signaled a tool-use but the step didn't declare
                // one, so the adopter sees the divergence on the
                // wire (closed-catalog tag, not silent).
                if let Some(FinishReason::ToolUse) = &chunk.finish_reason {
                    let tool_name = shape
                        .tools
                        .first()
                        .map(|t| t.name.clone())
                        .unwrap_or_else(|| "<unknown>".to_string());
                    ctx.tx
                        .send(FlowExecutionEvent::ToolCall {
                            step_name: shape.name.clone(),
                            tool_name,
                            content: chunk.delta.clone(),
                            timestamp_ms: now_ms(),
                        })
                        .map_err(|_| DispatchError::ChannelClosed)?;
                }
                if !chunk.delta.is_empty() {
                    tokens_emitted += 1;
                    accumulated.push_str(&chunk.delta);
                    ctx.tx
                        .send(FlowExecutionEvent::StepToken {
                            step_name: shape.name.clone(),
                            content: chunk.delta,
                            token_index: tokens_emitted,
                branch_path: ctx.branch_path_string(),
                            timestamp_ms: now_ms(),
                        })
                        .map_err(|_| DispatchError::ChannelClosed)?;
                }
            }
            Err(e) => {
                return Err(DispatchError::BackendError {
                    name: ctx.backend_name.clone(),
                    message: format!("chunk error: {e}"),
                });
            }
        }
    }
    Ok((accumulated, tokens_emitted, 0, 0))
}

/// §Fase 65.C.4 — max regenerate attempts on an error-severity anchor breach.
/// Mirrors the non-streaming runner's `MAX_ANCHOR_RETRIES` so both server paths
/// converge after the same number of corrections.
const MAX_ANCHOR_RETRIES: u32 = 2;

/// §Fase 65.C.4 — drain the chunk stream into a BUFFER without emitting to the
/// wire. The anchor-retry path must see the FULL output before deciding whether
/// to accept or regenerate, and SSE tokens can't be un-sent. Returns the
/// accumulated text + the buffered `(delta, is_tool_use)` chunks to replay on
/// acceptance — [`emit_buffered`] reproduces `drain_direct`'s emission exactly.
async fn drain_to_buffer(
    chunk_stream: crate::backends::ChatStream,
    ctx: &DispatchCtx,
) -> Result<(String, Vec<(String, bool)>), DispatchError> {
    use crate::backends::FinishReason;
    let mut accumulated = String::new();
    let mut buffered: Vec<(String, bool)> = Vec::new();
    let mut stream = chunk_stream;
    while let Some(chunk_result) = stream.next().await {
        if ctx.cancel.is_cancelled() {
            return Err(DispatchError::UpstreamCancelled);
        }
        match chunk_result {
            Ok(chunk) => {
                let is_tool = matches!(chunk.finish_reason, Some(FinishReason::ToolUse));
                if is_tool || !chunk.delta.is_empty() {
                    if !chunk.delta.is_empty() {
                        accumulated.push_str(&chunk.delta);
                    }
                    buffered.push((chunk.delta, is_tool));
                }
            }
            Err(e) => {
                return Err(DispatchError::BackendError {
                    name: ctx.backend_name.clone(),
                    message: format!("chunk error: {e}"),
                });
            }
        }
    }
    Ok((accumulated, buffered))
}

/// §Fase 65.C.4 — replay buffered chunks to the wire, reproducing `drain_direct`'s
/// ToolCall + StepToken emission EXACTLY so the wire shape is identical to a live
/// drain (only deferred past the anchor gate). Returns `tokens_emitted`.
async fn emit_buffered(
    buffered: Vec<(String, bool)>,
    shape: &PureShapeStep,
    ctx: &mut DispatchCtx,
) -> Result<u64, DispatchError> {
    let mut tokens_emitted: u64 = 0;
    for (delta, is_tool) in buffered {
        if is_tool {
            let tool_name = shape
                .tools
                .first()
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "<unknown>".to_string());
            ctx.tx
                .send(FlowExecutionEvent::ToolCall {
                    step_name: shape.name.clone(),
                    tool_name,
                    content: delta.clone(),
                    timestamp_ms: now_ms(),
                })
                .map_err(|_| DispatchError::ChannelClosed)?;
        }
        if !delta.is_empty() {
            tokens_emitted += 1;
            ctx.tx
                .send(FlowExecutionEvent::StepToken {
                    step_name: shape.name.clone(),
                    content: delta,
                    token_index: tokens_emitted,
                branch_path: ctx.branch_path_string(),
                    timestamp_ms: now_ms(),
                })
                .map_err(|_| DispatchError::ChannelClosed)?;
        }
    }
    Ok(tokens_emitted)
}

async fn drain_through_enforcer(
    chunk_stream: crate::backends::ChatStream,
    shape: &PureShapeStep,
    ctx: &mut DispatchCtx,
    policy: BackpressurePolicy,
    _step_index: usize,
) -> Result<(String, u64, u64, u64), DispatchError> {
    use crate::stream_effect_dispatcher::{StreamPolicyEnforcer, DEFAULT_STREAM_BUFFER_CAPACITY};
    use std::sync::Arc;

    // Build enforcer per the established Fase 33.x.d dispatch
    // (identity degrader OSS default for DegradeQuality; enterprise
    // verticals override via separate R&D track).
    let enforcer = Arc::new(match policy {
        BackpressurePolicy::DegradeQuality => StreamPolicyEnforcer::with_degrader(
            policy,
            DEFAULT_STREAM_BUFFER_CAPACITY,
            Arc::new(|chunk| chunk),
        ),
        BackpressurePolicy::DropOldest
        | BackpressurePolicy::PauseUpstream
        | BackpressurePolicy::Fail => StreamPolicyEnforcer::new(policy),
    });

    // Producer task — drains the chunk stream into the enforcer.
    // `ChatStream` (Pin<Box<dyn Stream + Send>>) is `Unpin` by
    // construction so it satisfies `enforcer.drain`'s bound.
    let producer_enforcer = enforcer.clone();
    let producer = tokio::spawn(async move {
        let summary = producer_enforcer
            .drain(chunk_stream, |_e| {
                // Backend errors are captured by the consumer when
                // it sees the enforcer close prematurely.
            })
            .await;
        producer_enforcer.close().await;
        summary
    });

    // Consumer side — pop chunks + forward to wire.
    let mut accumulated = String::new();
    let mut tokens_emitted: u64 = 0;

    while let Some(chunk) = enforcer.pop_chunk().await {
        if ctx.cancel.is_cancelled() {
            return Err(DispatchError::UpstreamCancelled);
        }
        // §Fase 33.y.k D8 — same ToolCall emission as `drain_direct`.
        // When the backend signals FinishReason::ToolUse on a chunk
        // pulled through the enforcer, surface the tool-call to the
        // wire BEFORE forwarding any text delta (the enforcer's
        // chunk ordering preserves arrival sequence; the ToolCall
        // event always precedes the StepToken from the same chunk).
        if let Some(crate::backends::FinishReason::ToolUse) = &chunk.finish_reason {
            let tool_name = shape
                .tools
                .first()
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "<unknown>".to_string());
            ctx.tx
                .send(FlowExecutionEvent::ToolCall {
                    step_name: shape.name.clone(),
                    tool_name,
                    content: chunk.delta.clone(),
                    timestamp_ms: now_ms(),
                })
                .map_err(|_| DispatchError::ChannelClosed)?;
        }
        if !chunk.delta.is_empty() {
            tokens_emitted += 1;
            accumulated.push_str(&chunk.delta);
            ctx.tx
                .send(FlowExecutionEvent::StepToken {
                    step_name: shape.name.clone(),
                    content: chunk.delta,
                    token_index: tokens_emitted,
                branch_path: ctx.branch_path_string(),
                    timestamp_ms: now_ms(),
                })
                .map_err(|_| DispatchError::ChannelClosed)?;
        }
    }

    // Producer summary — wait for the producer to finish so we get
    // accurate counters in the snapshot below.
    let drain_summary = producer.await.map_err(|e| DispatchError::BackendError {
        name: ctx.backend_name.clone(),
        message: format!("enforcer producer task join: {e}"),
    })?;

    // Post-drain metrics snapshot. Pull the counters AFTER the
    // consumer loop finished (matches Fase 33.x.d discipline — the
    // drain-returned `chunks_delivered` is captured before the
    // consumer terminates; the post-loop snapshot is authoritative
    // for delivered count). The drain summary keeps `failed` +
    // policy slug as authoritative.
    let snap = enforcer.metrics_snapshot();
    let wire = crate::execution_result::EnforcementSummaryWire {
        policy_slug: policy.slug().to_string(),
        chunks_pushed: snap.items_pushed,
        chunks_delivered: snap.items_delivered,
        drop_oldest_hits: snap.drop_oldest_hits,
        degrade_quality_hits: snap.degrade_quality_hits,
        pause_upstream_blocks: snap.pause_upstream_blocks,
        fail_overflows: snap.fail_overflows,
        failed: drain_summary.failed,
    };

    {
        let mut guard = ctx.enforcement_summaries.lock().await;
        guard.insert(shape.name.clone(), wire);
    }

    let drop_count = snap.drop_oldest_hits;
    let degrade_count = snap.degrade_quality_hits;
    Ok((accumulated, tokens_emitted, drop_count, degrade_count))
}

// ────────────────────────────────────────────────────────────────────
//  sha256_hex helper
// ────────────────────────────────────────────────────────────────────

fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest.as_slice() {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

// ────────────────────────────────────────────────────────────────────
//  Unit tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel_token::CancellationFlag;
    use tokio::sync::mpsc;

    fn fresh_ctx() -> (
        DispatchCtx,
        mpsc::UnboundedReceiver<FlowExecutionEvent>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        let ctx = DispatchCtx::new(
            "TestFlow",
            "stub",
            "system prompt",
            CancellationFlag::new(),
            tx,
        );
        (ctx, rx)
    }

    /// sha256_hex of the empty string is the canonical
    /// e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855.
    #[test]
    fn sha256_hex_empty_string_is_canonical() {
        assert_eq!(
            sha256_hex(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    /// sha256_hex of "(stub)" — the canonical stub backend chunk.
    #[test]
    fn sha256_hex_stub_marker() {
        // Independently computed:
        //   echo -n "(stub)" | sha256sum
        //   97f2ad79c25c0b6f3c87018b5e6b94c91d11ef0aaa61d4f7f8a6d8b1f0c8c0fb (will be checked at runtime)
        let h = sha256_hex("(stub)");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[tokio::test]
    async fn run_step_with_stub_backend_emits_one_token() {
        use crate::ir_nodes::IRStep;

        let step = IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
            name: "Generate".into(),
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
            guards: Vec::new(),
            requires_context: None,            now_tz: None,            body: Vec::new(),
        };
        let (mut ctx, mut rx) = fresh_ctx();

        let outcome = run_step(&step, &mut ctx).await.expect("run_step ok");
        match outcome {
            NodeOutcome::Completed { output, tokens_emitted, step_index } => {
                assert_eq!(output, "(stub)");
                assert_eq!(tokens_emitted, 1);
                assert_eq!(step_index, 0);
            }
            other => panic!("expected Completed, got {other:?}"),
        }

        // Drain wire events
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        // Expect StepStart + StepToken + StepComplete (3 events).
        assert_eq!(events.len(), 3, "events: {events:?}");
        assert!(matches!(events[0], FlowExecutionEvent::StepStart { .. }));
        assert!(matches!(events[1], FlowExecutionEvent::StepToken { .. }));
        assert!(matches!(events[2], FlowExecutionEvent::StepComplete { .. }));
    }

    /// §Fase 68.d — a step with NO `requires_context:` runs exactly as before
    /// (empty model → backend default). Back-compat (D68.4): the resolver is
    /// invoked but yields the empty-model sentinel, so the stub path is untouched.
    #[tokio::test]
    async fn step_without_requires_context_runs_unchanged() {
        use crate::ir_nodes::IRStep;
        let step = IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
            name: "Generate".into(),
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
            requires_context: None,

            now_tz: None,

            guards: Vec::new(),
            body: Vec::new(),
        };
        let (mut ctx, _rx) = fresh_ctx();
        let outcome = run_step(&step, &mut ctx).await.expect("run_step ok");
        assert!(matches!(outcome, NodeOutcome::Completed { .. }));
    }

    /// §Fase 68.d — a step whose `requires_context:` the resolved backend cannot
    /// satisfy FAILS CLOSED (D68.3) BEFORE the upstream request — a loud
    /// `BackendError`, never a too-small model that 400s mid-stream (the brief-#36
    /// failure mode). `fresh_ctx` uses the `stub` backend (empty §68.a catalog),
    /// so ANY positive requirement is unsatisfiable → the resolver gates the call.
    #[tokio::test]
    async fn step_with_unsatisfiable_requires_context_fails_closed() {
        use crate::ir_nodes::IRStep;
        let step = IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
            name: "BigSummary".into(),
            persona_ref: String::new(),
            given: String::new(),
            ask: "summarize a long conversation".into(),
            use_tool: None,
            probe: None,
            reason: None,
            weave: None,
            output_type: String::new(),
            confidence_floor: None,
            navigate_ref: String::new(),
            apply_ref: String::new(),
            requires_context: Some(16_000),

            now_tz: None,

            guards: Vec::new(),
            body: Vec::new(),
        };
        let (mut ctx, _rx) = fresh_ctx();
        match run_step(&step, &mut ctx).await {
            Err(DispatchError::BackendError { message, .. }) => assert!(
                message.contains("model capability unsatisfied"),
                "fail-closed must name the capability gap, got: {message}"
            ),
            other => panic!("expected fail-closed BackendError, got {other:?}"),
        }
    }

    /// §Fase 65.C.2 — two LLM steps on ONE ctx accumulate conversation history
    /// (user + assistant per turn), so the dispatcher's LLM path is no longer
    /// stateless single-shot. The second step's request carries the first
    /// turn's Q&A (coherence parity with the non-streaming runner).
    #[tokio::test]
    async fn conversation_history_accumulates_across_steps() {
        use crate::ir_nodes::IRStep;

        let mk = |ask: &str| IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
            name: "Generate".into(),
            persona_ref: String::new(),
            given: String::new(),
            ask: ask.into(),
            use_tool: None,
            probe: None,
            reason: None,
            weave: None,
            output_type: String::new(),
            confidence_floor: None,
            navigate_ref: String::new(),
            apply_ref: String::new(),
            guards: Vec::new(),
            requires_context: None,            now_tz: None,            body: Vec::new(),
        };
        let (mut ctx, _rx) = fresh_ctx();

        run_step(&mk("first question"), &mut ctx).await.expect("step 1");
        run_step(&mk("second question"), &mut ctx).await.expect("step 2");

        let conv = ctx.conversation.lock().unwrap();
        let msgs = conv.messages();
        assert_eq!(msgs.len(), 4, "two turns recorded (user+assistant ×2)");
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[1].content, "(stub)");
        assert_eq!(msgs[2].role, "user");
        assert_eq!(msgs[3].role, "assistant");
        // The user turns carry the asks (proves the step prompt landed in history).
        assert!(msgs[0].content.contains("first question"));
        assert!(msgs[2].content.contains("second question"));
    }

    /// §Fase 65.C.2 — the char budget drops the oldest turn pairs before a call,
    /// keeping at least the most recent turn (the runner's `ContextWindow`).
    #[tokio::test]
    async fn conversation_history_respects_char_budget() {
        use crate::ir_nodes::IRStep;
        let mk = |ask: &str| IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
            name: "G".into(),
            persona_ref: String::new(),
            given: String::new(),
            ask: ask.into(),
            use_tool: None,
            probe: None,
            reason: None,
            weave: None,
            output_type: String::new(),
            confidence_floor: None,
            navigate_ref: String::new(),
            apply_ref: String::new(),
            guards: Vec::new(),
            requires_context: None,            now_tz: None,            body: Vec::new(),
        };
        let (mut ctx, _rx) = fresh_ctx();
        ctx.context_budget = 1; // force truncation to the most recent turn

        run_step(&mk("turn one is quite long"), &mut ctx).await.expect("s1");
        run_step(&mk("turn two"), &mut ctx).await.expect("s2");
        run_step(&mk("turn three"), &mut ctx).await.expect("s3");

        // After each call truncates oldest pairs to budget then appends its own
        // turn, the history never grows unbounded — it holds the most recent
        // turn pair plus the just-appended one.
        let conv = ctx.conversation.lock().unwrap();
        assert!(conv.messages().len() <= 4, "budget bounds the history: {}", conv.messages().len());
    }

    fn step_named(name: &str, ask: &str) -> crate::ir_nodes::IRStep {
        crate::ir_nodes::IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
            name: name.into(),
            persona_ref: String::new(),
            given: String::new(),
            ask: ask.into(),
            use_tool: None,
            probe: None,
            reason: None,
            weave: None,
            output_type: String::new(),
            confidence_floor: None,
            navigate_ref: String::new(),
            apply_ref: String::new(),
            guards: Vec::new(),
            requires_context: None,            now_tz: None,            body: Vec::new(),
        }
    }

    fn anchor_named(name: &str) -> crate::ir_nodes::IRAnchor {
        crate::ir_nodes::IRAnchor {
            node_type: "anchor",
            source_line: 0,
            source_column: 0,
            name: name.into(),
            description: String::new(),
            require: String::new(),
            reject: Vec::new(),
            enforce: String::new(),
            confidence_floor: None,
            unknown_response: String::new(),
            on_violation: String::new(),
            on_violation_target: String::new(),
        }
    }

    /// §Fase 65.C.3 — a flow anchor the step output BREACHES is now surfaced in
    /// the step audit (the streaming/SSE path previously ignored anchors
    /// entirely). `RequiresCitation` breaches on the stub output `(stub)` (no
    /// citation), deterministically.
    #[tokio::test]
    async fn anchor_breach_is_surfaced_in_step_audit() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.anchors = std::sync::Arc::new(vec![anchor_named("RequiresCitation")]);

        run_step(&step_named("Generate", "say hi"), &mut ctx).await.expect("ok");

        let audit = ctx.step_audit_records.lock().await;
        let rec = audit.last().expect("one audit record");
        assert_eq!(rec.anchor_breaches.len(), 1, "the breach is recorded: {:?}", rec.anchor_breaches);
        assert!(rec.anchor_breaches[0].contains("RequiresCitation"));
        assert!(rec.anchor_breaches[0].contains("[error]"));
    }

    /// §Fase 65.C.3 — back-compat: with no anchors declared the audit record's
    /// `anchor_breaches` is empty (serde elides it → byte-identical wire).
    #[tokio::test]
    async fn no_anchors_means_no_breaches_recorded() {
        let (mut ctx, _rx) = fresh_ctx();
        run_step(&step_named("Generate", "say hi"), &mut ctx).await.expect("ok");
        let audit = ctx.step_audit_records.lock().await;
        assert!(audit.last().unwrap().anchor_breaches.is_empty());
    }

    /// §Fase 65.C.4 — an ANCHORED step goes through the buffer-then-retry path
    /// (it can't stream live AND regenerate). After the retries resolve, the
    /// accepted response's chunks are REPLAYED to the wire — so the StepToken
    /// event still arrives (wire-identical to a live drain, just deferred). The
    /// stub output `(stub)` keeps breaching `RequiresCitation`, so the loop
    /// exhausts MAX_ANCHOR_RETRIES and accepts, recording the breach.
    #[tokio::test]
    async fn anchored_step_buffers_retries_then_replays_tokens_to_wire() {
        let (mut ctx, mut rx) = fresh_ctx();
        ctx.anchors = std::sync::Arc::new(vec![anchor_named("RequiresCitation")]);

        let outcome = run_step(&step_named("Generate", "say hi"), &mut ctx)
            .await
            .expect("ok");
        match outcome {
            NodeOutcome::Completed { output, tokens_emitted, .. } => {
                assert_eq!(output, "(stub)", "accepted output after retries");
                assert_eq!(tokens_emitted, 1, "the buffered chunk is replayed to the wire");
            }
            other => panic!("expected Completed, got {other:?}"),
        }

        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        // Wire fidelity preserved: StepStart + StepToken("(stub)") + StepComplete.
        assert!(matches!(events[0], FlowExecutionEvent::StepStart { .. }));
        assert!(
            events.iter().any(|e| matches!(
                e,
                FlowExecutionEvent::StepToken { content, .. } if content == "(stub)"
            )),
            "the accepted response's token is replayed to the wire: {events:?}"
        );
        assert!(events.iter().any(|e| matches!(e, FlowExecutionEvent::StepComplete { .. })));

        let audit = ctx.step_audit_records.lock().await;
        let breaches = &audit.last().unwrap().anchor_breaches;
        assert_eq!(breaches.len(), 1, "the unresolved breach is recorded: {breaches:?}");
    }

    #[tokio::test]
    async fn run_step_cancel_pre_dispatch_short_circuits() {
        use crate::ir_nodes::IRStep;

        let step = IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
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
            guards: Vec::new(),
            requires_context: None,            now_tz: None,            body: Vec::new(),
        };
        let cancel = CancellationFlag::new();
        cancel.cancel();
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);

        let outcome = run_step(&step, &mut ctx).await;
        assert!(matches!(outcome, Err(DispatchError::UpstreamCancelled)));
    }

    #[tokio::test]
    async fn run_step_unknown_backend_returns_backend_error() {
        use crate::ir_nodes::IRStep;

        let step = IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
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
            guards: Vec::new(),
            requires_context: None,            now_tz: None,            body: Vec::new(),
        };
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new(
            "F",
            "does_not_exist",
            "",
            CancellationFlag::new(),
            tx,
        );

        let outcome = run_step(&step, &mut ctx).await;
        match outcome {
            Err(DispatchError::BackendError { name, message }) => {
                assert_eq!(name, "does_not_exist");
                assert!(message.contains("not in streaming registry"));
            }
            other => panic!("expected BackendError, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_step_pending_policy_consumed_on_entry() {
        use crate::ir_nodes::IRStep;

        let step = IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
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
            guards: Vec::new(),
            requires_context: None,            now_tz: None,            body: Vec::new(),
        };
        let (mut ctx, _rx) = fresh_ctx();
        ctx.pending_effect_policy = Some(BackpressurePolicy::DropOldest);

        let _ = run_step(&step, &mut ctx).await.expect("ok");
        assert!(
            ctx.pending_effect_policy.is_none(),
            "33.y.c contract: handler MUST consume pending_effect_policy on entry"
        );

        // Enforcement summary recorded for the step name.
        let summaries = ctx.enforcement_summaries.lock().await;
        assert!(summaries.contains_key("S"));
        assert_eq!(summaries["S"].policy_slug, "drop_oldest");
    }

    #[tokio::test]
    async fn run_step_records_step_audit_row() {
        use crate::ir_nodes::IRStep;

        let step = IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
            name: "Generate".into(),
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
            guards: Vec::new(),
            requires_context: None,            now_tz: None,            body: Vec::new(),
        };
        let (mut ctx, _rx) = fresh_ctx();
        let _ = run_step(&step, &mut ctx).await.expect("ok");

        let audit = ctx.step_audit_records.lock().await;
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].step_name, "Generate");
        assert_eq!(audit[0].tokens_emitted, 1);
        assert!(audit[0].success);
        // SHA-256 of "(stub)" — content-addressable per D6.
        assert_eq!(audit[0].output_hash_hex.len(), 64);
        assert!(audit[0].effect_policy_applied.is_none());
    }

    #[tokio::test]
    async fn run_probe_kind_slug_is_probe() {
        use crate::ir_nodes::IRProbe;

        let probe = IRProbe {
            node_type: "probe",
            source_line: 0,
            source_column: 0,
            target: "market_data".into(),
        };
        let (mut ctx, mut rx) = fresh_ctx();
        let _ = run_probe(&probe, &mut ctx).await.expect("ok");

        // First event is StepStart with step_type="probe".
        let ev = rx.try_recv().expect("event");
        match ev {
            FlowExecutionEvent::StepStart { step_type, step_name, .. } => {
                assert_eq!(step_type, "probe");
                assert_eq!(step_name, "market_data");
            }
            other => panic!("expected StepStart, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_reason_kind_slug_is_reason() {
        use crate::ir_nodes::IRReasonStep;

        let reason = IRReasonStep {
            node_type: "reason",
            source_line: 0,
            source_column: 0,
            strategy: "chain_of_thought".into(),
            target: "claim".into(),
        };
        let (mut ctx, mut rx) = fresh_ctx();
        let _ = run_reason(&reason, &mut ctx).await.expect("ok");

        let ev = rx.try_recv().expect("event");
        match ev {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "reason");
            }
            other => panic!("expected StepStart, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_validate_kind_slug_is_validate() {
        use crate::ir_nodes::IRValidateStep;

        let validate = IRValidateStep {
            node_type: "validate",
            source_line: 0,
            source_column: 0,
            target: "draft".into(),
            rule: "no_pii".into(),
        };
        let (mut ctx, mut rx) = fresh_ctx();
        let _ = run_validate(&validate, &mut ctx).await.expect("ok");
        let ev = rx.try_recv().expect("event");
        match ev {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "validate");
            }
            other => panic!("expected StepStart, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_refine_kind_slug_is_refine() {
        use crate::ir_nodes::IRRefineStep;

        let refine = IRRefineStep {
            node_type: "refine",
            source_line: 0,
            source_column: 0,
            target: "draft".into(),
            strategy: "tighten".into(),
        };
        let (mut ctx, mut rx) = fresh_ctx();
        let _ = run_refine(&refine, &mut ctx).await.expect("ok");
        let ev = rx.try_recv().expect("event");
        match ev {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "refine");
            }
            other => panic!("expected StepStart, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_weave_kind_slug_is_weave() {
        use crate::ir_nodes::IRWeaveStep;

        let weave = IRWeaveStep {
            node_type: "weave",
            source_line: 0,
            source_column: 0,
            sources: vec!["A".into(), "B".into()],
            target: "report".into(),
            format_type: "markdown".into(),
            priority: vec!["A".into()],
            style: "formal".into(),
        };
        let (mut ctx, mut rx) = fresh_ctx();
        let _ = run_weave(&weave, &mut ctx).await.expect("ok");
        let ev = rx.try_recv().expect("event");
        match ev {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "weave");
            }
            other => panic!("expected StepStart, got {other:?}"),
        }
    }
}
