//! §Fase 33.y.g — Algebraic-effect handler nodes.
//!
//! Six variants graduated in 33.y.g:
//!
//! - **`ShieldApply`** (Fase 20) — apply a registered shield to a
//!   target. Per-chunk scrubbing semantics when target is a stream
//!   (foundation for the enterprise PHI scrubber R&D track).
//! - **`OtsApply`** (Fase 14) — apply a One-True-Source transform.
//! - **`MandateApply`** (Fase 16-related) — apply a mandate (a
//!   compliance-bound transformation declared at the language level).
//! - **`ComputeApply`** (Fase 11.f) — invoke a compute capability
//!   with positional arguments; binds result under `output_name`.
//! - **`Listen`** (Fase 13 π-calc) — wait on a typed channel for
//!   an event; binds event payload under `event_alias`.
//! - **`DaemonStep`** (Fase 16 supervisor) — invoke a daemon by
//!   reference (the daemon's per-invocation execution is its own
//!   subsystem).
//!
//! # Architecture: OSS framework + enterprise hooks
//!
//! Per the OSS / ENTERPRISE / SPLIT charter, the actual shield /
//! OTS / mandate / compute capability *implementations* live in
//! `axon_enterprise.shield` (the scanner registry) + `axon_enterprise.
//! ots` (the OTS transformers) + `axon_enterprise.cognitive_states`
//! (the daemon supervisor + Fase 13 channel layer). 33.y.g ships the
//! OSS **framework** — the wire shape + the public `apply_*` helpers
//! that enterprise overrides via future hooks.
//!
//! The OSS-default `apply_*` helpers are **identity passthroughs**:
//! they bind the target verbatim under the output key. This is
//! semantically correct for the OSS reference implementation
//! (adopters with no shield registry see their data unmodified) and
//! provides the integration surface enterprise hooks supersede.
//!
//! # Wire shape
//!
//! Each handler emits:
//!   1. `axon.step_start { step_type: <slug>, ... }`
//!   2. `axon.step_complete { ... }`
//!
//! No StepToken events (these are pure capability-application
//! operations, not LLM dispatches). Output binds to
//! `ctx.let_bindings` under a variant-specific key + returns
//! `NodeOutcome::Completed { output, tokens_emitted: 0,
//! step_index: <reserved> }`.
//!
//! # D-letter anchors
//!
//! - **D1** — every variant has a NAMED async handler; exhaustive
//!   match in `dispatch_node`.
//! - **D3** — cancel checked at every `.await` boundary.
//! - **D7** — every error case routes through `DispatchError`; the
//!   `apply_*` helpers cannot fail in the OSS path (they're
//!   passthroughs); enterprise overrides will surface errors as
//!   `DispatchError::BackendError { name: "shield" / "ots" / ... }`.
//! - **D10** — sync-runner parity: shield/OTS/mandate/compute
//!   capabilities apply identically (identity in OSS); Listen
//!   binds a structured placeholder until the typed-channel layer
//!   wires into the dispatcher in a future sub-fase.

use crate::flow_dispatcher::{DispatchCtx, DispatchError, NodeOutcome};
use crate::flow_execution_event::{now_ms, FlowExecutionEvent};
use crate::ir_nodes::{
    IRComputeApplyStep, IRDaemonStepNode, IRListenStep, IRMandateApplyStep,
    IROtsApplyStep, IRShieldApplyStep,
};

// ────────────────────────────────────────────────────────────────────
//  Public apply_* helpers (enterprise hooks override these)
// ────────────────────────────────────────────────────────────────────

/// Apply a named shield to `target`. OSS default: identity
/// passthrough. This helper is the **no-scanner fallback**: as of
/// §Fase 40.b, [`run_shield_apply`] first consults
/// [`crate::shield_registry`]; this function only runs when no scanner
/// is registered for the shield name. Enterprise vertical crates
/// register HIPAA / legal / fintech scanners via
/// [`crate::shield_registry::register_shield_scanner`].
///
/// # Streaming semantics
///
/// When `target` is the output of a streaming step (i.e. carries
/// per-chunk content), enterprise's per-chunk scrubber applies the
/// shield to each chunk as it arrives. The OSS path applies once
/// to the materialized string — semantically equivalent for the
/// adopter's wire output (the difference is the wire timing, which
/// matters only when shields can BLOCK chunks; OSS shields are
/// passthrough so they never block).
pub fn apply_shield_to_target(shield_name: &str, target: &str, _ctx: &DispatchCtx) -> String {
    // 33.y.g OSS default: identity. Enterprise overrides.
    let _ = shield_name;
    target.to_string()
}

// §Fase 119.c — `apply_ots_to_target` was DELETED here.
//
// It was `target.to_string()` with the name discarded (§111 F18): a
// transformation that transformed nothing under a declared teleology, and
// the "enterprise override" its doc promised had no hook to exist through.
// `run_ots_apply` now consults `crate::ots_registry` (the shield_registry
// shape): a registered transformer transforms, a transformer refusal fails
// the flow with blame, and an UNREGISTERED name REFUSES — because unlike a
// shield (a filter, where absence honestly leaves data untouched), an ots
// output CLAIMS to be the transformed input, and identity under that claim
// fabricates a result.

// §Fase 119.b — `apply_mandate_to_target` was DELETED here.
//
// It was `target.to_string()` with `let _ = mandate_name;` — the mandate's
// name discarded on the first line, documented as an "enterprise override
// hook" that nothing could override into existence (§111 F18, the same
// factory-of-the-lie shape as `invoke_compute_capability`, deleted above).
// `run_mandate_apply` now drives `crate::mandate_engine::MandateRuntime`:
// the target is validated for free, refined in a buffered closed loop when
// it violates, and NOTHING is bound unless the constraint set accepted it
// or the declared `on_violation: coerce` policy visibly released the best
// attempt. There is nothing left for a placeholder to stand in for.

// §Fase 111.f — `invoke_compute_capability` was DELETED here.
//
// It was the factory of the lie. It resolved the arguments and returned
// `format!("compute:{name}({args})")` — a STRING, which `run_compute_apply` bound
// under the step's output name and a downstream step then consumed as if it were
// a number. Its doc comment claimed "OSS default: returns a canonical formatted
// string … Enterprise overrides hook into the real compute runtime". No such hook
// existed, and no override could have helped: `IRCompute` carried no body to run.
//
// `run_compute_apply` now evaluates the declared §70 expression natively via
// `eval_expr`. There is nothing left for a placeholder to stand in for.

/// Listen on a Fase 13 typed channel for an event. OSS default:
/// returns a canonical placeholder `"(awaiting <channel>)"` so
/// adopters observe the wait + bind under `event_alias` until
/// the typed-channel runtime layer wires in (axon_enterprise.
/// channels + future Fase 13 runtime). The placeholder is wire-
/// stable + adopter-diagnostic.
pub fn listen_on_channel(channel: &str, _channel_is_ref: bool, _ctx: &DispatchCtx) -> String {
    format!("(awaiting {channel})")
}

/// Invoke a daemon by reference. OSS default: returns canonical
/// `"daemon:<ref>"` placeholder. Enterprise overrides
/// (axon_enterprise.supervisor) dispatch to the registered daemon
/// supervisor's invocation surface.
pub fn invoke_daemon(daemon_ref: &str, _ctx: &DispatchCtx) -> String {
    format!("daemon:{daemon_ref}")
}

// ────────────────────────────────────────────────────────────────────
//  ShieldApply
// ────────────────────────────────────────────────────────────────────

/// Apply a shield to a target. Wire shape: `step_type: "shield_apply"`.
///
/// Resolution: `target` is resolved through `ctx.let_bindings`
/// (literal if missing). The shield is applied via
/// [`apply_shield_to_target`] (identity in OSS; enterprise
/// overrides). Output binds under `output_type` (when non-empty)
/// or under `<target>_shielded` (canonical fallback) in
/// `ctx.let_bindings`.
pub async fn run_shield_apply(
    node: &IRShieldApplyStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    let step_name = if node.shield_name.is_empty() {
        "ShieldApply".to_string()
    } else {
        node.shield_name.clone()
    };

    emit_step_start(ctx, &step_name, step_index, "shield_apply")?;

    let resolved_target = ctx
        .let_bindings
        .get(&node.target)
        .cloned()
        .unwrap_or_else(|| node.target.clone());

    // §Fase 40.b — consult the shield-scanner registry. A registered
    // scanner (enterprise HIPAA/legal/AML, etc.) returns a verdict:
    // `Pass` binds the (possibly redacted) content; `Reject` surfaces a
    // structured `DispatchError::BackendError` so the SSE/HTTP layer can
    // attribute blame. When NO scanner is registered for this name, the
    // OSS identity passthrough applies (backwards-compatible — adopters
    // with no enterprise layer see their data unmodified).
    let shielded = match crate::shield_registry::lookup_shield_scanner(&node.shield_name) {
        Some(scanner) => {
            let scan_ctx = crate::shield_registry::ShieldScanContext::new(node.shield_name.clone());
            match scanner.scan(&resolved_target, &scan_ctx) {
                crate::shield_registry::ShieldVerdict::Pass(content) => content,
                // §Fase 114.w — a Reject runs the shield's DECLARED `on_breach:`
                // policy (it used to always halt). The policy rides the node
                // (`IRBreachPolicy`, resolved at lowering), so this site and
                // `run_emit`'s σ-gate honor it identically. `Proceed` continues
                // with declared/sanitized content; `Halt` fails closed.
                crate::shield_registry::ShieldVerdict::Reject { code, reason } => {
                    match crate::shield_registry::apply_on_breach(
                        &ctx.tenant_id,
                        &node.shield_name,
                        node.breach_policy.as_ref(),
                        &scanner,
                        &resolved_target,
                        &code,
                        &reason,
                    ) {
                        crate::shield_registry::BreachDisposition::Proceed { content } => content,
                        crate::shield_registry::BreachDisposition::Halt { message } => {
                            return Err(DispatchError::BackendError {
                                name: format!("shield:{}", node.shield_name),
                                message,
                            });
                        }
                    }
                }
            }
        }
        None => apply_shield_to_target(&node.shield_name, &resolved_target, ctx),
    };

    let output_key = if !node.output_type.is_empty() {
        node.output_type.clone()
    } else if !node.target.is_empty() {
        format!("{}_shielded", node.target)
    } else {
        String::new()
    };
    if !output_key.is_empty() {
        ctx.let_bindings.insert(output_key, shielded.clone());
    }

    emit_step_complete(ctx, &step_name, step_index, &shielded, 0)?;

    Ok(NodeOutcome::Completed {
        output: shielded,
        tokens_emitted: 0,
        step_index,
    })
}

// ────────────────────────────────────────────────────────────────────
//  OtsApply
// ────────────────────────────────────────────────────────────────────

/// §Fase 119.c — `ots <Name> on <target>`: registry-dispatched transform.
///
/// Resolution order, every fork fail-closed:
/// 1. the declaration must exist in `ctx.ots_specs` (unknown ⇒ refusal);
/// 2. a transformer must be registered for the name in
///    [`crate::ots_registry`] (unregistered ⇒ refusal — identity under a
///    transformation's name fabricates a result, §111 F18);
/// 3. the transformer's own verdict: `Transformed` binds, `Refused` fails
///    the flow naming the ots and the transformer's reason.
pub async fn run_ots_apply(
    node: &IROtsApplyStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    let step_name = if node.ots_name.is_empty() {
        "OtsApply".to_string()
    } else {
        node.ots_name.clone()
    };
    emit_step_start(ctx, &step_name, step_index, "ots_apply")?;

    let resolved_target = ctx
        .let_bindings
        .get(&node.target)
        .cloned()
        .unwrap_or_else(|| node.target.clone());

    let transformed = transform_ots(&node.ots_name, &resolved_target, ctx)?;

    let output_key = if !node.output_type.is_empty() {
        node.output_type.clone()
    } else if !node.target.is_empty() {
        format!("{}_ots", node.target)
    } else {
        String::new()
    };
    if !output_key.is_empty() {
        ctx.let_bindings.insert(output_key, transformed.clone());
    }

    emit_step_complete(ctx, &step_name, step_index, &transformed, 0)?;

    Ok(NodeOutcome::Completed {
        output: transformed,
        tokens_emitted: 0,
        step_index,
    })
}

/// §Fase 119.c — the shared ots transform: declaration → registry → verdict.
/// Used by the flow-level node above and by the step-scoped `ots` guard.
pub(crate) fn transform_ots(
    ots_name: &str,
    content: &str,
    ctx: &DispatchCtx,
) -> Result<String, DispatchError> {
    let blame = format!("ots:{ots_name}");
    let spec = ctx
        .ots_specs
        .iter()
        .find(|o| o.name == ots_name)
        .ok_or_else(|| DispatchError::BackendError {
            name: blame.clone(),
            message: format!(
                "ots '{ots_name}' is not in the compiled program's ots catalog; \
                 nothing can be synthesized, so nothing is bound."
            ),
        })?;

    match crate::ots_registry::lookup_ots_transformer(ots_name) {
        None => Err(DispatchError::BackendError {
            name: blame,
            message: format!(
                "ots '{ots_name}' has no registered transformer; binding the \
                 input unchanged under a transformation's name would fabricate \
                 a result, so it is refused. Register one via \
                 axon::ots_registry::register_ots_transformer (the in-tree \
                 media pipeline and enterprise verticals register here)."
            ),
        }),
        Some(t) => {
            let tctx = crate::ots_registry::OtsTransformContext {
                ots_name: ots_name.to_string(),
                teleology: spec.teleology.clone(),
                homotopy_search: spec.homotopy_search.clone(),
                loss_function: spec.loss_function.clone(),
            };
            match t.transform(content, &tctx) {
                crate::ots_registry::OtsVerdict::Transformed(out) => Ok(out),
                crate::ots_registry::OtsVerdict::Refused { code, reason } => {
                    Err(DispatchError::BackendError {
                        name: blame,
                        message: format!(
                            "ots '{ots_name}' transformer refused [{code}]: {reason}"
                        ),
                    })
                }
            }
        }
    }
}

pub async fn run_mandate_apply(
    node: &IRMandateApplyStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    let step_name = if node.mandate_name.is_empty() {
        "MandateApply".to_string()
    } else {
        node.mandate_name.clone()
    };
    emit_step_start(ctx, &step_name, step_index, "mandate_apply")?;

    let resolved_target = ctx
        .let_bindings
        .get(&node.target)
        .cloned()
        .unwrap_or_else(|| node.target.clone());

    let mandated = enforce_mandate_over(
        &node.mandate_name,
        Some(resolved_target.as_str()),
        MandateTask::Rewrite {
            target: resolved_target.clone(),
        },
        ctx,
    )
    .await?;

    let output_key = if !node.output_type.is_empty() {
        node.output_type.clone()
    } else if !node.target.is_empty() {
        format!("{}_mandated", node.target)
    } else {
        String::new()
    };
    if !output_key.is_empty() {
        ctx.let_bindings.insert(output_key, mandated.clone());
    }

    emit_step_complete(ctx, &step_name, step_index, &mandated, 0)?;

    Ok(NodeOutcome::Completed {
        output: mandated,
        tokens_emitted: 0,
        step_index,
    })
}

/// What the actuator is asked to produce on each attempt.
pub(crate) enum MandateTask {
    /// Rewrite pre-existing content until it satisfies the constraint set
    /// (the flow-level `mandate X on <binding>` node).
    Rewrite { target: String },
    /// Generate from a step prompt under the mandate (the step-scoped guard).
    Generate { prompt: String },
}

/// §Fase 119.b — the shared enforcement driver: resolve the declaration,
/// build the runtime (refusing vacuity), drive the buffered loop against the
/// resolved backend, apply the declared `on_violation` policy. Used by the
/// flow-level node above and by the step-guard path in `pure_shape`.
///
/// Fails CLOSED at every fork: unresolved mandate name, unfalsifiable
/// constraint set, inadmissible declaration, backend infrastructure error,
/// and breach under `halt`/`retry`/default — all are flow errors that name
/// the mandate (`mandate:<name>`, the shield-registry blame idiom).
pub(crate) async fn enforce_mandate_over(
    mandate_name: &str,
    initial: Option<&str>,
    task: MandateTask,
    ctx: &mut DispatchCtx,
) -> Result<String, DispatchError> {
    let blame = format!("mandate:{mandate_name}");

    let spec = ctx
        .mandate_specs
        .iter()
        .find(|m| m.name == mandate_name)
        .cloned()
        .ok_or_else(|| DispatchError::BackendError {
            name: blame.clone(),
            message: format!(
                "mandate '{mandate_name}' is not in the compiled program's mandate \
                 catalog; nothing can be enforced, so nothing is released. (The type \
                 checker rejects unknown names statically — an empty catalog here \
                 usually means the deploy path did not attach `mandate_specs`.)"
            ),
        })?;

    let mut runtime = crate::mandate_engine::MandateRuntime::from_spec(&spec).map_err(|r| {
        DispatchError::BackendError {
            name: blame.clone(),
            message: format!("mandate '{mandate_name}' refused at dispatch: {r}"),
        }
    })?;

    // Honest partial coverage is VISIBLE coverage: clauses that steer the
    // prompt but cannot be mechanically checked are surfaced as a runtime
    // warning, not silently counted as enforced.
    // (Not `ctx.runtime_warnings`: that is the CLOSED axon-W002 catalog for
    // streaming fallbacks, and a free-string push there would distort it —
    // the §108/§113 lesson. Structured tracing is the honest channel until a
    // mandate-specific wire surface earns its own catalog entry.)
    let unchecked = runtime.unchecked_clauses().to_vec();
    if !unchecked.is_empty() {
        tracing::warn!(
            mandate = mandate_name,
            clauses = unchecked.join("; ").as_str(),
            "mandate steers but cannot mechanically check {} clause(s)",
            unchecked.len()
        );
    }

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

    // Tier 1 — token bans where the wire admits them; None elsewhere, and
    // the loop (Tier 2) carries the guarantee alone.
    let bias = runtime.logit_bias_bans(backend.default_model());

    let system = {
        let block = runtime.directive_block();
        if ctx.system_prompt.is_empty() {
            block
        } else {
            format!("{}\n\n{}", ctx.system_prompt, block)
        }
    };
    let cancel = ctx.cancel.clone();
    let backend_ref = &backend;
    let system_ref = &system;
    let bias_ref = &bias;
    let task_ref = &task;

    let outcome = runtime
        .enforce(initial, move |directive| {
            let user = match task_ref {
                MandateTask::Rewrite { target } => {
                    if directive.feedback.is_empty() {
                        format!(
                            "Rewrite the following content so it satisfies every mandate \
                             requirement, changing as little as possible:\n\n{target}"
                        )
                    } else {
                        format!(
                            "Rewrite the following content so it satisfies every mandate \
                             requirement, changing as little as possible.\n\nThe previous \
                             attempt violated these obligations:\n{}\n\nContent:\n\n{target}",
                            directive.feedback
                        )
                    }
                }
                MandateTask::Generate { prompt } => {
                    if directive.feedback.is_empty() {
                        prompt.clone()
                    } else {
                        format!(
                            "{prompt}\n\nYour previous attempt violated these mandate \
                             obligations — correct all of them:\n{}",
                            directive.feedback
                        )
                    }
                }
            };
            let request = crate::backends::ChatRequest {
                model: String::new(),
                messages: vec![crate::backends::Message::user(user)],
                system: Some(system_ref.clone()),
                stream: false,
                logit_bias: bias_ref.clone(),
                cancel: cancel.clone(),
                ..Default::default()
            };
            async move {
                backend_ref
                    .complete(request)
                    .await
                    .map(|r| r.content)
                    .map_err(|e| e.to_string())
            }
        })
        .await
        .map_err(|infra| DispatchError::BackendError {
            name: ctx.backend_name.clone(),
            message: format!("mandate '{mandate_name}' aborted on backend failure: {infra}"),
        })?;

    match outcome {
        Ok(report) => Ok(report.output),
        Err(breach) => {
            if spec.on_violation == "coerce" {
                // The developer DECLARED best-effort. It is honored visibly:
                // the residual error is a runtime warning and the breach is
                // in the audit trail — never a silent success.
                if let crate::mandate_engine::MandateBreach::Unconverged {
                    best_error,
                    best_output,
                    ..
                } = &breach
                {
                    tracing::warn!(
                        mandate = mandate_name,
                        residual_error = best_error,
                        "mandate did not converge; `on_violation: coerce` released \
                         the best attempt with its residual error on record"
                    );
                    return Ok(best_output.clone());
                }
                // A PremiseViolated breach is NOT coercible: the declared
                // drift bound is false on this backend, so even the best
                // attempt stands on a voided proof.
            }
            Err(DispatchError::BackendError {
                name: blame,
                message: breach.describe(mandate_name),
            })
        }
    }
}

pub async fn run_compute_apply(
    node: &IRComputeApplyStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    let step_name = if node.compute_name.is_empty() {
        "ComputeApply".to_string()
    } else {
        node.compute_name.clone()
    };
    emit_step_start(ctx, &step_name, step_index, "compute_apply")?;

    // (1) Resolve the declared function.
    let spec = ctx
        .compute_specs
        .iter()
        .find(|c| c.name == node.compute_name)
        .cloned()
        .ok_or_else(|| DispatchError::BackendError {
            name: "compute".to_string(),
            message: format!(
                "`compute {}` does not resolve to a declared compute — nothing to execute",
                node.compute_name
            ),
        })?;

    // (2) A compute with no body cannot compute. REFUSE rather than bind a
    //     placeholder string that a downstream step will read as a number.
    let body = spec.body.clone().ok_or_else(|| DispatchError::BackendError {
        name: "compute".to_string(),
        message: format!(
            "`compute {}` declares no body — it cannot compute anything. Give it one: \
             `compute {}(x: Number) -> Number {{ x * 2 }}`. Binding a placeholder here is how \
             the pre-§111 runtime handed a downstream step the text \"compute:{}(…)\" where it \
             expected a number (§111 F10)",
            spec.name, spec.name, spec.name
        ),
    })?;

    // (3) Arity. A silent mismatch would evaluate the body against a stale or
    //     missing binding and still produce *a* number — the worst outcome.
    if node.arguments.len() != spec.parameters.len() {
        return Err(DispatchError::BackendError {
            name: "compute".to_string(),
            message: format!(
                "`compute {} on …` was applied to {} argument(s) but declares {} parameter(s) ({}). \
                 A silent arity mismatch would still yield a number — just the wrong one",
                spec.name,
                node.arguments.len(),
                spec.parameters.len(),
                spec.parameters
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }

    // (4) Bind the arguments into the call frame: each declared parameter takes
    //     the value of its positional argument, resolved from the caller's
    //     bindings. Saved and restored, so a parameter name cannot leak out of
    //     the compute and shadow the caller's own binding.
    let mut saved: Vec<(String, Option<String>)> = Vec::new();
    for (param, arg) in spec.parameters.iter().zip(&node.arguments) {
        // §Fase 119.f.10 — a QUOTED argument is a literal, not a name to look
        // up. §119.f.10 let `compute X on a.b, "USD"` parse; the parser keeps
        // the quotes so this distinction survives to dispatch, because
        // `resolve_value_reference` would otherwise treat `USD` as a binding
        // name and silently return whatever a binding of that name held. That
        // is the §60 literal/reference classification, and flattening it here
        // would reintroduce exactly the ambiguity §60 exists to remove.
        let value = match arg.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            Some(literal) => literal.to_string(),
            None => crate::exec_context::resolve_value_reference(arg, &ctx.let_bindings),
        };
        saved.push((param.name.clone(), ctx.let_bindings.get(&param.name).cloned()));
        ctx.let_bindings.insert(param.name.clone(), value);
    }

    // (5) Evaluate — natively, deterministically, with no model in the loop.
    let evaluated = crate::flow_dispatcher::orchestration::eval_expr(&body, ctx);

    // Restore the frame before doing anything else with the result.
    for (name, prior) in saved {
        match prior {
            Some(v) => {
                ctx.let_bindings.insert(name, v);
            }
            None => {
                ctx.let_bindings.remove(&name);
            }
        }
    }

    use crate::flow_dispatcher::orchestration::EVal;
    let result = match evaluated {
        Some(EVal::Int(i)) => i.to_string(),
        Some(EVal::Float(f)) => f.to_string(),
        Some(EVal::Bool(b)) => b.to_string(),
        Some(EVal::Str(s)) => s,
        // The §70 evaluator fails CLOSED on division by zero, an unresolvable
        // reference, a type mismatch — every one of which the old handler would
        // have papered over with a plausible-looking string.
        _ => {
            return Err(DispatchError::BackendError {
                name: "compute".to_string(),
                message: format!(
                    "`compute {}` did not evaluate — the expression could not be reduced to a \
                     value (an unresolvable reference, a type mismatch, or a division by zero). \
                     Refusing: a compute that cannot compute must not return something that LOOKS \
                     like a result",
                    spec.name
                ),
            })
        }
    };

    if !node.output_name.is_empty() {
        ctx.let_bindings
            .insert(node.output_name.clone(), result.clone());
    }

    emit_step_complete(ctx, &step_name, step_index, &result, 0)?;

    Ok(NodeOutcome::Completed {
        output: result,
        tokens_emitted: 0,
        step_index,
    })
}

// ────────────────────────────────────────────────────────────────────
//  Listen
// ────────────────────────────────────────────────────────────────────

/// Listen on a typed channel. Wire shape: `step_type: "listen"`.
/// The placeholder `"(awaiting <channel>)"` binds under
/// `event_alias` until the Fase 13 typed-channel runtime wires
/// into the dispatcher in a future sub-fase.
pub async fn run_listen(
    node: &IRListenStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    let step_name = if node.event_alias.is_empty() {
        "Listen".to_string()
    } else {
        node.event_alias.clone()
    };
    emit_step_start(ctx, &step_name, step_index, "listen")?;

    let placeholder = listen_on_channel(&node.channel, node.channel_is_ref, ctx);
    if !node.event_alias.is_empty() {
        ctx.let_bindings
            .insert(node.event_alias.clone(), placeholder.clone());
    }

    emit_step_complete(ctx, &step_name, step_index, &placeholder, 0)?;

    Ok(NodeOutcome::Completed {
        output: placeholder,
        tokens_emitted: 0,
        step_index,
    })
}

/// §Fase 52.c — `run <Flow>(args)` flow-step: invoke a declared flow from a
/// body (a daemon `listen` handler — the brief #32 Q3). The LANGUAGE surface
/// (parse / type-check / IR / this dispatcher arm) lands here; the REAL
/// recursive flow dispatch — looking up `flow_name` in the program and
/// executing its steps under the daemon's identity — is wired by the §52.c
/// daemon executor (it needs the flow registry + the recursion guard, which
/// this leaf dispatcher does not own). Until then this binds the invocation
/// outcome under `output_to` (if any) so downstream steps resolve, mirroring
/// the surface-handler pattern §51 used for `quant`/`yield`.
pub async fn run_run(
    node: &axon_frontend::ir_nodes::IRRun,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    emit_step_start(ctx, &node.flow_name, step_index, "run")?;
    let outcome = format!("(invoking flow {})", node.flow_name);
    if !node.output_to.is_empty() {
        ctx.let_bindings
            .insert(node.output_to.clone(), outcome.clone());
    }
    emit_step_complete(ctx, &node.flow_name, step_index, &outcome, 0)?;

    Ok(NodeOutcome::Completed {
        output: outcome,
        tokens_emitted: 0,
        step_index,
    })
}

// ────────────────────────────────────────────────────────────────────
//  DaemonStep
// ────────────────────────────────────────────────────────────────────

/// Invoke a daemon by reference. Wire shape:
/// `step_type: "daemon_step"`. The placeholder `"daemon:<ref>"`
/// binds under `<daemon_ref>_invoked` until the Fase 16
/// supervisor invocation surface wires into the dispatcher in a
/// future sub-fase.
pub async fn run_daemon_step(
    node: &IRDaemonStepNode,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    let step_name = if node.daemon_ref.is_empty() {
        "DaemonStep".to_string()
    } else {
        node.daemon_ref.clone()
    };
    emit_step_start(ctx, &step_name, step_index, "daemon_step")?;

    let invoked = invoke_daemon(&node.daemon_ref, ctx);
    if !node.daemon_ref.is_empty() {
        ctx.let_bindings
            .insert(format!("{}_invoked", node.daemon_ref), invoked.clone());
    }

    emit_step_complete(ctx, &step_name, step_index, &invoked, 0)?;

    Ok(NodeOutcome::Completed {
        output: invoked,
        tokens_emitted: 0,
        step_index,
    })
}

// ────────────────────────────────────────────────────────────────────
//  Wire-event helpers (shared)
// ────────────────────────────────────────────────────────────────────

fn emit_step_start(
    ctx: &mut DispatchCtx,
    step_name: &str,
    step_index: usize,
    step_type: &str,
) -> Result<(), DispatchError> {
    ctx.tx
        .send(FlowExecutionEvent::StepStart {
            step_name: step_name.to_string(),
            step_index,
            step_type: step_type.to_string(),
                branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)
}

fn emit_step_complete(
    ctx: &mut DispatchCtx,
    step_name: &str,
    step_index: usize,
    full_output: &str,
    tokens_output: u64,
) -> Result<(), DispatchError> {
    ctx.tx
        .send(FlowExecutionEvent::StepComplete {
            step_name: step_name.to_string(),
            step_index,
            success: true,
            full_output: full_output.to_string(),
            tokens_input: 0,
            tokens_output,
                branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)
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
            "",
            CancellationFlag::new(),
            tx,
        );
        (ctx, rx)
    }

    // ── apply_* helpers (OSS identity passthrough) ───────────────────

    #[test]
    fn apply_shield_oss_default_is_identity() {
        let (ctx, _rx) = fresh_ctx();
        assert_eq!(apply_shield_to_target("hipaa", "patient data", &ctx), "patient data");
    }

    #[test]
    fn listen_returns_canonical_awaiting_placeholder() {
        let (ctx, _rx) = fresh_ctx();
        assert_eq!(listen_on_channel("event_bus", true, &ctx), "(awaiting event_bus)");
    }

    #[test]
    fn invoke_daemon_returns_canonical_invocation_placeholder() {
        let (ctx, _rx) = fresh_ctx();
        assert_eq!(invoke_daemon("supervisor", &ctx), "daemon:supervisor");
    }

    // ── ShieldApply ──────────────────────────────────────────────────

    #[tokio::test]
    async fn run_shield_apply_binds_output_under_output_type() {
        let (mut ctx, mut rx) = fresh_ctx();
        ctx.let_bindings.insert("input_text".into(), "sensitive".into());
        let node = IRShieldApplyStep {
            node_type: "shield_apply",
            source_line: 0,
            source_column: 0,
            shield_name: "hipaa".into(),
            target: "input_text".into(),
            output_type: "scrubbed".into(),
        
            breach_policy: None,
        };
        let outcome = run_shield_apply(&node, &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Completed { output, tokens_emitted, .. } => {
                assert_eq!(output, "sensitive");
                assert_eq!(tokens_emitted, 0);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        assert_eq!(ctx.let_bindings.get("scrubbed").unwrap(), "sensitive");
        // Wire emits StepStart "shield_apply" + StepComplete.
        let first = rx.try_recv().unwrap();
        match first {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "shield_apply");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn run_shield_apply_canonical_fallback_output_key() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings.insert("doc".into(), "content".into());
        let node = IRShieldApplyStep {
            node_type: "shield_apply",
            source_line: 0,
            source_column: 0,
            shield_name: "hipaa".into(),
            target: "doc".into(),
            output_type: String::new(),
        
            breach_policy: None,
        };
        run_shield_apply(&node, &mut ctx).await.unwrap();
        assert_eq!(ctx.let_bindings.get("doc_shielded").unwrap(), "content");
    }

    // ── ShieldApply × §Fase 40.b registry hook ───────────────────────
    // Unique shield names + cleanup so these don't collide with the
    // "hipaa" identity tests above under parallel execution.

    struct RedactScanner;
    impl crate::shield_registry::ShieldScanner for RedactScanner {
        fn scan(
            &self,
            _target: &str,
            _ctx: &crate::shield_registry::ShieldScanContext,
        ) -> crate::shield_registry::ShieldVerdict {
            crate::shield_registry::ShieldVerdict::pass("[REDACTED]")
        }
    }

    struct BlockScanner;
    impl crate::shield_registry::ShieldScanner for BlockScanner {
        fn scan(
            &self,
            _target: &str,
            _ctx: &crate::shield_registry::ShieldScanContext,
        ) -> crate::shield_registry::ShieldVerdict {
            crate::shield_registry::ShieldVerdict::reject("phi.unredacted", "PHI present")
        }
    }

    #[tokio::test]
    async fn run_shield_apply_routes_through_registered_scanner() {
        const NAME: &str = "t40b_redact";
        crate::shield_registry::register_shield_scanner(NAME, std::sync::Arc::new(RedactScanner));

        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings.insert("note".into(), "SSN 123-45-6789".into());
        let node = IRShieldApplyStep {
            node_type: "shield_apply",
            source_line: 0,
            source_column: 0,
            shield_name: NAME.into(),
            target: "note".into(),
            output_type: "scrubbed".into(),
        
            breach_policy: None,
        };
        let outcome = run_shield_apply(&node, &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Completed { output, .. } => assert_eq!(output, "[REDACTED]"),
            other => panic!("expected Completed, got {other:?}"),
        }
        assert_eq!(ctx.let_bindings.get("scrubbed").unwrap(), "[REDACTED]");

        crate::shield_registry::unregister_shield_scanner(NAME);
    }

    #[tokio::test]
    async fn run_shield_apply_rejecting_scanner_surfaces_backend_error() {
        const NAME: &str = "t40b_block";
        crate::shield_registry::register_shield_scanner(NAME, std::sync::Arc::new(BlockScanner));

        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings.insert("note".into(), "raw phi".into());
        let node = IRShieldApplyStep {
            node_type: "shield_apply",
            source_line: 0,
            source_column: 0,
            shield_name: NAME.into(),
            target: "note".into(),
            output_type: "scrubbed".into(),
        
            breach_policy: None,
        };
        let err = run_shield_apply(&node, &mut ctx).await.unwrap_err();
        match err {
            DispatchError::BackendError { name, message } => {
                assert_eq!(name, format!("shield:{NAME}"));
                assert!(message.contains("phi.unredacted"), "blame code in message");
                assert!(message.contains("PHI present"), "reason in message");
            }
            other => panic!("expected BackendError, got {other:?}"),
        }
        // A rejected shield must NOT bind output.
        assert!(ctx.let_bindings.get("scrubbed").is_none());

        crate::shield_registry::unregister_shield_scanner(NAME);
    }

    #[tokio::test]
    async fn run_shield_apply_unregistered_name_is_identity() {
        // No scanner registered under this unique name → OSS identity.
        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings.insert("doc".into(), "untouched".into());
        let node = IRShieldApplyStep {
            node_type: "shield_apply",
            source_line: 0,
            source_column: 0,
            shield_name: "t40b_never_registered".into(),
            target: "doc".into(),
            output_type: "out".into(),
        
            breach_policy: None,
        };
        let outcome = run_shield_apply(&node, &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Completed { output, .. } => assert_eq!(output, "untouched"),
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    // ── OtsApply ─────────────────────────────────────────────────────

    /// §Fase 119.c — **this test used to BE the §111 F18 bug for ots.**
    ///
    /// It asserted, in green, that `ots g711_mulaw on raw_audio` binds
    /// `"samples"` UNCHANGED under `pcm` — a µ-law transcoder whose transform
    /// was the identity function, tested and passing. It now routes a
    /// declared, REGISTERED ots through the real path and asserts the wire
    /// shape; the fail-closed inversions live in
    /// `an_unregistered_ots_refuses_never_identity` and
    /// `an_undeclared_ots_fails_closed_before_the_registry`.
    #[tokio::test]
    async fn run_ots_apply_binds_output() {
        let (mut ctx, mut rx) = fresh_ctx();
        ctx.ots_specs = std::sync::Arc::new(vec![ots_spec("g711_mulaw")]);
        crate::ots_registry::register_ots_transformer(
            "g711_mulaw",
            std::sync::Arc::new(ReverseTransformer),
        );
        ctx.let_bindings.insert("raw_audio".into(), "samples".into());
        let node = IROtsApplyStep {
            node_type: "ots_apply",
            source_line: 0,
            source_column: 0,
            ots_name: "g711_mulaw".into(),
            target: "raw_audio".into(),
            output_type: "pcm".into(),
        };
        run_ots_apply(&node, &mut ctx).await.unwrap();
        crate::ots_registry::unregister_ots_transformer("g711_mulaw");
        assert_eq!(
            ctx.let_bindings.get("pcm").unwrap(),
            "selpmas",
            "the bound value is the TRANSFORM's output, never the input"
        );
        let first = rx.try_recv().unwrap();
        match first {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "ots_apply");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    // ── MandateApply ─────────────────────────────────────────────────

    /// §Fase 119.b — **this test used to BE the §111 F18 bug.**
    ///
    /// It asserted, in green, that `mandate gdpr_erasure on user_record`
    /// binds `"user_record"` UNCHANGED under `erased` — a mandate named
    /// "gdpr_erasure" whose enforcement was the identity function, tested
    /// and passing. It now asserts the wire shape on a mandate that
    /// actually resolves and enforces (and the fail-closed behavior of the
    /// old fixture lives in
    /// `an_unresolved_mandate_name_fails_closed_never_identity`).
    #[tokio::test]
    async fn run_mandate_apply_binds_output() {
        let (mut ctx, mut rx) = fresh_ctx();
        ctx.mandate_specs = std::sync::Arc::new(vec![mandate_spec(
            "gdpr_erasure",
            "must not contain \"ssn\"",
            2,
            "halt",
        )]);
        ctx.let_bindings
            .insert("user_record".into(), "name and city only".into());
        let node = IRMandateApplyStep {
            node_type: "mandate_apply",
            source_line: 0,
            source_column: 0,
            mandate_name: "gdpr_erasure".into(),
            target: "user_record".into(),
            output_type: "erased".into(),
        };
        run_mandate_apply(&node, &mut ctx).await.unwrap();
        assert_eq!(
            ctx.let_bindings.get("erased").unwrap(),
            "name and city only",
            "a COMPLIANT target binds; a violating one would have been rewritten"
        );
        let first = rx.try_recv().unwrap();
        match first {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "mandate_apply");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    // ── ComputeApply ─────────────────────────────────────────────────

    /// §Fase 111.f — **this test used to BE the bug.**
    ///
    /// It asserted, in green, for years:
    ///
    /// ```text
    /// assert_eq!(ctx.let_bindings.get("sum").unwrap(), "compute:add(5, 7)");
    /// ```
    ///
    /// A test demanding that the sum of 5 and 7 be the **string**
    /// `"compute:add(5, 7)"` — and a downstream step would then consume that text
    /// where it expected a number, while the README promised "native Fast-Path
    /// execution bypassing the LLM" with an O(n) guarantee.
    ///
    /// The placeholder was not an oversight the tests missed. **The tests pinned
    /// it.** That is worth staring at: a suite can lock a lie in place as firmly
    /// as it locks a truth.
    ///
    /// It now asserts the opposite — an undeclared compute REFUSES rather than
    /// binding something that looks like a result.
    #[tokio::test]
    async fn run_compute_apply_refuses_an_undeclared_compute() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings.insert("x".into(), "5".into());
        ctx.let_bindings.insert("y".into(), "7".into());
        let node = IRComputeApplyStep {
            node_type: "compute_apply",
            source_line: 0,
            source_column: 0,
            compute_name: "add".into(),
            arguments: vec!["x".into(), "y".into()],
            output_name: "sum".into(),
        };
        let err = run_compute_apply(&node, &mut ctx)
            .await
            .expect_err("an undeclared compute must refuse, not bind a placeholder string");
        assert!(format!("{err:?}").contains("does not resolve to a declared compute"));
        assert!(
            ctx.let_bindings.get("sum").is_none(),
            "nothing may be bound for a compute that did not compute"
        );
    }

    /// …and a DECLARED compute really adds. `5 + 7 = 12`, a number, zero tokens.
    #[tokio::test]
    async fn run_compute_apply_evaluates_the_declared_expression_natively() {
        use crate::ir_nodes::{IRCompute, IRExpr, IRParameter};

        let (mut ctx, mut rx) = fresh_ctx();
        ctx.let_bindings.insert("x".into(), "5".into());
        ctx.let_bindings.insert("y".into(), "7".into());

        let p = |n: &str| IRParameter {
            node_type: "parameter",
            source_line: 0,
            source_column: 0,
            name: n.into(),
            type_name: "Number".into(),
            generic_param: String::new(),
            optional: false,
        };
        ctx.compute_specs = std::sync::Arc::new(vec![IRCompute {
            node_type: "compute",
            source_line: 0,
            source_column: 0,
            name: "add".into(),
            shield_ref: String::new(),
            parameters: vec![p("a"), p("b")],
            return_type: "Number".into(),
            body: Some(IRExpr::Binary {
                op: "add".into(),
                lhs: Box::new(IRExpr::Ref { path: "a".into() }),
                rhs: Box::new(IRExpr::Ref { path: "b".into() }),
            }),
        }]);

        let node = IRComputeApplyStep {
            node_type: "compute_apply",
            source_line: 0,
            source_column: 0,
            compute_name: "add".into(),
            arguments: vec!["x".into(), "y".into()],
            output_name: "sum".into(),
        };
        let outcome = run_compute_apply(&node, &mut ctx).await.unwrap();

        assert_eq!(
            ctx.let_bindings.get("sum").unwrap(),
            "12",
            "5 + 7 = 12 — a NUMBER. This is the line the old test got wrong"
        );
        match outcome {
            NodeOutcome::Completed { tokens_emitted, .. } => assert_eq!(
                tokens_emitted, 0,
                "`compute` bypasses the LLM — that is the entire primitive"
            ),
            e => panic!("expected Completed, got {e:?}"),
        }

        match rx.try_recv().unwrap() {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "compute_apply");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    // ── Listen ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn run_listen_binds_placeholder_under_event_alias() {
        let (mut ctx, mut rx) = fresh_ctx();
        let node = IRListenStep {
            node_type: "listen",
            source_line: 0,
            source_column: 0,
            channel: "user_events".into(),
            channel_is_ref: true,
            event_alias: "evt".into(),
            body: Vec::new(),
        };
        run_listen(&node, &mut ctx).await.unwrap();
        assert_eq!(
            ctx.let_bindings.get("evt").unwrap(),
            "(awaiting user_events)"
        );
        let first = rx.try_recv().unwrap();
        match first {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "listen");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    // ── DaemonStep ───────────────────────────────────────────────────

    #[tokio::test]
    async fn run_daemon_step_binds_invocation_placeholder() {
        let (mut ctx, mut rx) = fresh_ctx();
        let node = IRDaemonStepNode {
            node_type: "daemon_step",
            source_line: 0,
            source_column: 0,
            daemon_ref: "audit_supervisor".into(),
        };
        run_daemon_step(&node, &mut ctx).await.unwrap();
        assert_eq!(
            ctx.let_bindings.get("audit_supervisor_invoked").unwrap(),
            "daemon:audit_supervisor"
        );
        let first = rx.try_recv().unwrap();
        match first {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "daemon_step");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    // ── Cancel guards ────────────────────────────────────────────────

    #[tokio::test]
    async fn every_handler_short_circuits_on_cancel() {
        let cancel = CancellationFlag::new();
        cancel.cancel();
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);

        assert!(matches!(
            run_shield_apply(
                &IRShieldApplyStep {
                    node_type: "shield_apply",
                    source_line: 0,
                    source_column: 0,
                    shield_name: "x".into(),
                    target: "y".into(),
                    output_type: "z".into(),
                
                    breach_policy: None,
                },
                &mut ctx,
            )
            .await,
            Err(DispatchError::UpstreamCancelled)
        ));

        assert!(matches!(
            run_ots_apply(
                &IROtsApplyStep {
                    node_type: "ots_apply",
                    source_line: 0,
                    source_column: 0,
                    ots_name: "x".into(),
                    target: "y".into(),
                    output_type: "z".into(),
                },
                &mut ctx,
            )
            .await,
            Err(DispatchError::UpstreamCancelled)
        ));

        assert!(matches!(
            run_mandate_apply(
                &IRMandateApplyStep {
                    node_type: "mandate_apply",
                    source_line: 0,
                    source_column: 0,
                    mandate_name: "x".into(),
                    target: "y".into(),
                    output_type: "z".into(),
                },
                &mut ctx,
            )
            .await,
            Err(DispatchError::UpstreamCancelled)
        ));

        assert!(matches!(
            run_compute_apply(
                &IRComputeApplyStep {
                    node_type: "compute_apply",
                    source_line: 0,
                    source_column: 0,
                    compute_name: "x".into(),
                    arguments: vec![],
                    output_name: "y".into(),
                },
                &mut ctx,
            )
            .await,
            Err(DispatchError::UpstreamCancelled)
        ));

        assert!(matches!(
            run_listen(
                &IRListenStep {
                    node_type: "listen",
                    source_line: 0,
                    source_column: 0,
                    channel: "x".into(),
                    channel_is_ref: false,
                    event_alias: "y".into(),
                    body: Vec::new(),
                },
                &mut ctx,
            )
            .await,
            Err(DispatchError::UpstreamCancelled)
        ));

        assert!(matches!(
            run_daemon_step(
                &IRDaemonStepNode {
                    node_type: "daemon_step",
                    source_line: 0,
                    source_column: 0,
                    daemon_ref: "x".into(),
                },
                &mut ctx,
            )
            .await,
            Err(DispatchError::UpstreamCancelled)
        ));
    }

    // ── §Fase 119.b — MandateApply: the loop, through real dispatch ──

    fn mandate_spec(
        name: &str,
        constraint: &str,
        max_steps: i64,
        on_violation: &str,
    ) -> crate::ir_nodes::IRMandate {
        crate::ir_nodes::IRMandate {
            node_type: "mandate",
            source_line: 1,
            source_column: 1,
            name: name.into(),
            constraint: constraint.into(),
            kp: Some(2.0),
            ki: Some(0.3),
            kd: Some(0.1),
            tolerance: Some(0.05),
            max_steps: Some(max_steps),
            drift_bound: None,
            lipschitz: None,
            on_violation: on_violation.into(),
        }
    }

    fn mandate_node(name: &str, target: &str, output: &str) -> IRMandateApplyStep {
        IRMandateApplyStep {
            node_type: "mandate_apply",
            source_line: 0,
            source_column: 0,
            mandate_name: name.into(),
            target: target.into(),
            output_type: output.into(),
        }
    }

    #[tokio::test]
    async fn a_compliant_target_passes_through_with_zero_attempts() {
        // The stub backend would return "(stub)"; if the output equals the
        // ORIGINAL target, the free pre-check accepted it and no model was
        // consulted.
        let (mut ctx, _rx) = fresh_ctx();
        ctx.mandate_specs = std::sync::Arc::new(vec![mandate_spec(
            "NoGuarantees",
            "must not contain \"guaranteed\"",
            3,
            "halt",
        )]);
        ctx.let_bindings
            .insert("draft".into(), "historical results only".into());
        let out = run_mandate_apply(&mandate_node("NoGuarantees", "draft", "vetted"), &mut ctx)
            .await
            .expect("compliant target passes");
        match out {
            NodeOutcome::Completed { output, .. } => {
                assert_eq!(output, "historical results only");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            ctx.let_bindings.get("vetted").map(String::as_str),
            Some("historical results only")
        );
    }

    #[tokio::test]
    async fn a_violating_target_is_rewritten_through_the_loop_until_accepted() {
        // The stub backend's fixed reply "(stub)" satisfies `must contain
        // "stub"`, so the loop converges on attempt 1 — and the BOUND value
        // is the model's rewrite, not the violating original.
        let (mut ctx, _rx) = fresh_ctx();
        ctx.mandate_specs = std::sync::Arc::new(vec![mandate_spec(
            "MustSayStub",
            "must contain \"stub\"",
            3,
            "halt",
        )]);
        ctx.let_bindings.insert("draft".into(), "noncompliant".into());
        let out = run_mandate_apply(&mandate_node("MustSayStub", "draft", "vetted"), &mut ctx)
            .await
            .expect("loop converges via the stub");
        match out {
            NodeOutcome::Completed { output, .. } => assert_eq!(output, "(stub)"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn an_unconverged_mandate_under_halt_fails_the_flow_with_the_breach() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.mandate_specs = std::sync::Arc::new(vec![mandate_spec(
            "Impossible",
            "must contain \"unicorn\"",
            2,
            "halt",
        )]);
        ctx.let_bindings.insert("draft".into(), "plain".into());
        let err = run_mandate_apply(&mandate_node("Impossible", "draft", ""), &mut ctx)
            .await
            .err()
            .expect("must fail closed");
        match err {
            DispatchError::BackendError { name, message } => {
                assert_eq!(name, "mandate:Impossible");
                assert!(message.contains("Anchor Breach"), "{message}");
                assert!(
                    message.contains("non-compliance cannot be shipped"),
                    "{message}"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn coerce_releases_the_best_attempt_visibly_instead_of_failing() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.mandate_specs = std::sync::Arc::new(vec![mandate_spec(
            "Impossible",
            "must contain \"unicorn\"; must contain \"stub\"",
            2,
            "coerce",
        )]);
        ctx.let_bindings.insert("draft".into(), "plain".into());
        let out = run_mandate_apply(&mandate_node("Impossible", "draft", "released"), &mut ctx)
            .await
            .expect("coerce is a DECLARED policy, honored");
        match out {
            // Best attempt: "(stub)" satisfies 1 of 2 (e = 0.5) vs the
            // target's 0 of 2 (e = 1.0).
            NodeOutcome::Completed { output, .. } => assert_eq!(output, "(stub)"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn an_unfalsifiable_mandate_is_refused_at_dispatch_before_any_token() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.mandate_specs = std::sync::Arc::new(vec![mandate_spec(
            "Vibes",
            "output must be insightful and warm",
            3,
            "halt",
        )]);
        ctx.let_bindings.insert("draft".into(), "text".into());
        let err = run_mandate_apply(&mandate_node("Vibes", "draft", ""), &mut ctx)
            .await
            .err()
            .expect("unfalsifiable is refused");
        match err {
            DispatchError::BackendError { name, message } => {
                assert_eq!(name, "mandate:Vibes");
                assert!(message.contains("refused at dispatch"), "{message}");
                assert!(
                    message.contains("insightful"),
                    "the refusal names the clauses: {message}"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn an_unresolved_mandate_name_fails_closed_never_identity() {
        // THE §111 F18 regression test: this exact call used to bind the
        // target unchanged. An empty catalog must now refuse.
        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings.insert("draft".into(), "user record".into());
        let err = run_mandate_apply(&mandate_node("gdpr_erasure", "draft", ""), &mut ctx)
            .await
            .err()
            .expect("no catalog, no passthrough");
        match err {
            DispatchError::BackendError { name, message } => {
                assert_eq!(name, "mandate:gdpr_erasure");
                assert!(message.contains("nothing is released"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    // ── §Fase 119.b (D119.4) — the step-scoped guard ─────────────────

    fn guarded_step(guard_name: &str) -> crate::ir_nodes::IRStep {
        crate::ir_nodes::IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
            name: "Draft".into(),
            persona_ref: String::new(),
            given: String::new(),
            ask: "write the section".into(),
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
            pix_ops: Vec::new(),
            guards: vec![crate::ir_nodes::IRStepGuard {
                kind: "mandate".into(),
                name: guard_name.into(),
                target: String::new(),
                binding: "vetted".into(),
            }],
            body: Vec::new(),
        }
    }

    #[tokio::test]
    async fn a_step_with_a_mandate_guard_generates_through_the_buffered_loop() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.mandate_specs = std::sync::Arc::new(vec![mandate_spec(
            "MustSayStub",
            "must contain \"stub\"",
            3,
            "halt",
        )]);
        let step = guarded_step("MustSayStub");
        let out = super::super::pure_shape::run_step(&step, &mut ctx)
            .await
            .expect("guarded generation converges via the stub");
        match out {
            NodeOutcome::Completed { output, .. } => assert_eq!(output, "(stub)"),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            ctx.let_bindings.get("vetted").map(String::as_str),
            Some("(stub)"),
            "the guard binding carries the ENFORCED output"
        );
    }

    #[tokio::test]
    async fn a_step_guard_breach_fails_the_step_not_just_the_binding() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.mandate_specs = std::sync::Arc::new(vec![mandate_spec(
            "Impossible",
            "must contain \"unicorn\"",
            2,
            "halt",
        )]);
        let step = guarded_step("Impossible");
        let err = super::super::pure_shape::run_step(&step, &mut ctx)
            .await
            .err()
            .expect("the step fails closed");
        match err {
            DispatchError::BackendError { name, .. } => {
                assert_eq!(name, "mandate:Impossible");
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn two_mandate_guards_on_one_step_are_refused_not_half_enforced() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.mandate_specs = std::sync::Arc::new(vec![
            mandate_spec("A", "must contain \"stub\"", 2, "halt"),
            mandate_spec("B", "must not contain \"x\"", 2, "halt"),
        ]);
        let mut step = guarded_step("A");
        step.guards.push(crate::ir_nodes::IRStepGuard {
            kind: "mandate".into(),
            name: "B".into(),
            target: String::new(),
            binding: String::new(),
        });
        let err = super::super::pure_shape::run_step(&step, &mut ctx)
            .await
            .err()
            .expect("undefined composition is refused");
        match err {
            DispatchError::BackendError { message, .. } => {
                assert!(message.contains("not yet defined"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    // ── §Fase 119.c — OtsApply: registry or refusal ──────────────────

    fn ots_spec(name: &str) -> crate::ir_nodes::IROts {
        crate::ir_nodes::IROts {
            node_type: "ots",
            source_line: 0,
            source_column: 0,
            name: name.into(),
            teleology: "normalize the payload".into(),
            homotopy_search: "shallow".into(),
            loss_function: "L2".into(),
        }
    }

    fn ots_node(name: &str, target: &str, output: &str) -> IROtsApplyStep {
        IROtsApplyStep {
            node_type: "ots_apply",
            source_line: 0,
            source_column: 0,
            ots_name: name.into(),
            target: target.into(),
            output_type: output.into(),
        }
    }

    struct ReverseTransformer;
    impl crate::ots_registry::OtsTransformer for ReverseTransformer {
        fn transform(
            &self,
            content: &str,
            ctx: &crate::ots_registry::OtsTransformContext,
        ) -> crate::ots_registry::OtsVerdict {
            // The context must carry the DECLARATION — a transformer that
            // cannot see the teleology cannot serve it.
            assert!(!ctx.teleology.is_empty());
            crate::ots_registry::OtsVerdict::Transformed(
                content.chars().rev().collect::<String>(),
            )
        }
    }

    struct AlwaysRefuses;
    impl crate::ots_registry::OtsTransformer for AlwaysRefuses {
        fn transform(
            &self,
            _content: &str,
            _ctx: &crate::ots_registry::OtsTransformContext,
        ) -> crate::ots_registry::OtsVerdict {
            crate::ots_registry::OtsVerdict::Refused {
                code: "E-OTS-SHAPE".into(),
                reason: "input is not the declared shape".into(),
            }
        }
    }

    #[tokio::test]
    async fn a_registered_transformer_transforms_and_binds() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.ots_specs = std::sync::Arc::new(vec![ots_spec("Reverser119c")]);
        crate::ots_registry::register_ots_transformer(
            "Reverser119c",
            std::sync::Arc::new(ReverseTransformer),
        );
        ctx.let_bindings.insert("raw".into(), "abc".into());
        let out = run_ots_apply(&ots_node("Reverser119c", "raw", "normalized"), &mut ctx)
            .await
            .expect("registered transformer runs");
        crate::ots_registry::unregister_ots_transformer("Reverser119c");
        match out {
            NodeOutcome::Completed { output, .. } => assert_eq!(output, "cba"),
            other => panic!("{other:?}"),
        }
        assert_eq!(ctx.let_bindings.get("normalized").map(String::as_str), Some("cba"));
    }

    #[tokio::test]
    async fn an_unregistered_ots_refuses_never_identity() {
        // THE §111 F18 regression test for ots: this exact call used to bind
        // the target unchanged.
        let (mut ctx, _rx) = fresh_ctx();
        ctx.ots_specs = std::sync::Arc::new(vec![ots_spec("NoTransformer119c")]);
        ctx.let_bindings.insert("raw".into(), "abc".into());
        let err = run_ots_apply(&ots_node("NoTransformer119c", "raw", "out"), &mut ctx)
            .await
            .err()
            .expect("unregistered must refuse");
        match err {
            DispatchError::BackendError { name, message } => {
                assert_eq!(name, "ots:NoTransformer119c");
                assert!(message.contains("fabricate"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn an_undeclared_ots_fails_closed_before_the_registry() {
        let (mut ctx, _rx) = fresh_ctx();
        let err = run_ots_apply(&ots_node("ghost", "raw", ""), &mut ctx)
            .await
            .err()
            .expect("undeclared must refuse");
        match err {
            DispatchError::BackendError { name, message } => {
                assert_eq!(name, "ots:ghost");
                assert!(message.contains("nothing is bound"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_transformer_refusal_fails_the_flow_with_its_reason() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.ots_specs = std::sync::Arc::new(vec![ots_spec("Strict119c")]);
        crate::ots_registry::register_ots_transformer(
            "Strict119c",
            std::sync::Arc::new(AlwaysRefuses),
        );
        ctx.let_bindings.insert("raw".into(), "abc".into());
        let err = run_ots_apply(&ots_node("Strict119c", "raw", ""), &mut ctx)
            .await
            .err()
            .expect("transformer refusal propagates");
        crate::ots_registry::unregister_ots_transformer("Strict119c");
        match err {
            DispatchError::BackendError { message, .. } => {
                assert!(message.contains("E-OTS-SHAPE"), "{message}");
                assert!(message.contains("not the declared shape"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }
}
