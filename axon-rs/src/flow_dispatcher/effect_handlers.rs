//! v2.87.0 — **the algebraic-effect machine** (Plotkin/Pretnar handlers with
//! one-shot delimited continuations), running on the DISPATCHER.
//!
//! # the design decision — why the machine is here and not in `crate::effects`
//!
//! `crate::effects::EffectRuntime` is a real 590-line FSM with a handler stack,
//! `find_handler` walking innermost-out, clause parameter binding and
//! resume/abort/forward. It was built in v1.17.0 and it works. It is also
//! **not a machine this language can run on**, and the reason is one line:
//!
//! ```text
//! Instruction::Passthrough => { i += 1; }        // effects/runtime.rs
//! ```
//!
//! Its instruction alphabet is `Instruction`, whose catch-all variant
//! (`#[serde(other)]`) is INERT. Lowering `handle SSE { … } in { run generate(…) }`
//! onto it — the shape `the design plan`'s plan originally proposed as step 2 — would
//! deserialise `run generate(…)` to `Passthrough` and execute **nothing**, while
//! the program compiled clean. That is v2.67.0's *motor real, cable muerto* rebuilt
//! deliberately, one level deeper.
//!
//! So v2.87.0 puts the machine where the language's instructions already are: the
//! dispatcher, whose alphabet is `IRFlowNode`. A handler clause is a `Vec<IRFlowNode>`
//! and therefore composes with all 51 primitives — a clause can `emit` on a
//! channel, call a tool, `persist`, run a nested `handle`. That composability is
//! the entire argument of `paper_algebraic_effects_streaming.md`, and nothing in
//! the language could express it before.
//!
//! `crate::effects` keeps its job: the FSM over the JSON `Instruction` IR that
//! `effects_bridge::bridge_effect_stream_yield` drives. Two machines, two
//! alphabets, one documented relationship — not two catalogs of one concept
//! (the v2.83.0 defect).
//!
//! # The semantics, and the two places they diverge from `EffectRuntime`
//!
//! Both divergences are decisions, not oversights, and both were forced by
//! reading `the design plan` section 3.1 rather than the engine:
//!
//! 1. **A clause that falls off its end ABORTS; it is not an error.**
//! `EffectRuntime` returns `NoDischarge` there. But section 3.1 publishes
//!    `Done() -> { websocket.close() }` with the comment *"sin resume — abort
//! implícito"*, and section 3.6 states *"0 resumes: aborto explícito, OK."* Under
//! v2.83.0's doctrine the published surface is the promise, so a clause with no
//!    `resume` aborts the handle with its last output.
//!
//! 2. **An `abort` is routed to the frame it belongs to**, by `frame_id`.
//!    `EffectRuntime` propagates `Aborted` past every enclosing frame to the top
//!    of the run — which contradicts its own doc comment (*"control yields past
//!    the enclosing handle expression"*). With nested handlers that matters:
//!    `handle A { Op() -> { abort() } } in { handle B { … } in { perform Op() } }`
//!    must terminate **A**, not B. Neither behaviour was ever observable, because
//!    nothing reached the engine.
//!
//! # Scope discipline
//!
//! A clause body of the frame at stack index `i` runs with the frame stack
//! **truncated to `i`** — strictly outer frames only. This is what makes:
//!
//!   - `forward` reach the next OUTER handler (D12) with no special casing, and
//!   - a clause that performs the operation it handles escape to an outer frame
//!     instead of dispatching itself, which would be an infinite loop.
//!
//! The type-checker's D9 row computation uses the same rule
//! (`axon_frontend::effect_check`), so static and dynamic scope agree by
//! construction rather than by coincidence.
//!
//! # D-letter anchors
//!
//! - **D1** — all five variants have NAMED handlers, reached from
//!   `dispatch_node`'s exhaustive match. Zero `_ =>` catch-alls.
//! - **D3** — cancel checked on entry to every handler.
//! - **D7** — every failure routes through [`DispatchError`]; no panic, no
//!   silent success. An unresolved effect name, a missing frame and a missing
//!   clause each FAIL CLOSED with the name that was not found.
//! - **D9 (runtime half)** — the type-checker refuses an undischarged `perform`
//!   statically (`axon-T966`). The runtime refusal below is belt-and-braces for
//!   a hand-built IR, and it is a REFUSAL: never a fallback, never a no-op.

use std::sync::Arc;

use crate::flow_dispatcher::{DispatchCtx, DispatchError, NodeOutcome};
use crate::flow_execution_event::{now_ms, FlowExecutionEvent};
use crate::ir_nodes::{IREffectAbort, IREffectForward, IREffectHandle, IREffectPerform, IREffectResume};

/// One live handler frame. `Arc` so the clause bodies are shared rather than
/// re-cloned on every nested dispatch.
pub type EffectFrame = Arc<IREffectHandle>;

// ════════════════════════════════════════════════════════════════════
//  handle — push the frame, run the body, pop
// ════════════════════════════════════════════════════════════════════

/// `handle E { clauses } in { body }`.
pub async fn run_handle(
    node: &IREffectHandle,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;
    let step_name = format!("handle:{}", node.effect_names.join(","));
    emit_start(ctx, &step_name, step_index, "handle")?;

    ctx.effect_frames.push(Arc::new(node.clone()));
    let result = crate::flow_dispatcher::orchestration::dispatch_body_public(&node.body, ctx).await;
    ctx.effect_frames.pop();

    let outcome = match result? {
        // The abort belongs to THIS frame — the handle expression yields the
        // aborted value and its remaining body does not run. An abort raised
        // for an OUTER frame passes straight through: routing it here would let
        // an inner handler swallow an outer one's exit.
        NodeOutcome::EffectAborted { frame_id, value } if frame_id == node.frame_id => {
            NodeOutcome::Completed {
                output: value,
                tokens_emitted: 0,
                step_index,
            }
        }
        other => other,
    };

    if let NodeOutcome::Completed { output, .. } = &outcome {
        emit_complete(ctx, &step_name, step_index, output, 0)?;
    }
    Ok(outcome)
}

// ════════════════════════════════════════════════════════════════════
//  perform — find the clause, bind, run, interpret the discharge
// ════════════════════════════════════════════════════════════════════

/// `perform E.Op(args)`.
pub async fn run_perform(
    node: &IREffectPerform,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    // An empty `effect_name` means the frontend could not resolve the site to
    // exactly one declared effect (the design decision ambiguity, or an undeclared
    // operation). The type-checker already refused that program; a hand-built
    // IR that reaches here FAILS CLOSED. Searching the stack by operation name
    // alone would be the tempting fallback and it is precisely wrong — it would
    // route the effect to whichever frame happened to be innermost, which is
    // the silent mis-delivery the ambiguity error exists to prevent.
    if node.effect_name.is_empty() {
        return Err(DispatchError::BackendError {
            name: "algebraic_effects".to_string(),
            message: format!(
                "perform '{}' reached dispatch with no resolved effect. The compiler \
                 refuses this statically (axon-T964 / axon-T965); an IR that carries it \
                 anyway is not dispatched by operation name, because that would deliver \
                 the effect to a handler the author never named.",
                node.operation_name
            ),
        });
    }

    let step_name = format!("perform:{}.{}", node.effect_name, node.operation_name);
    emit_start(ctx, &step_name, step_index, "perform")?;

    let args: Vec<String> = node
        .arguments
        .iter()
        .map(|a| resolve_argument(a, ctx))
        .collect();

    let outcome = dispatch_operation(
        &node.effect_name,
        &node.operation_name,
        &args,
        ctx.effect_frames.len(),
        ctx,
    )
    .await?;

    if let NodeOutcome::Completed { output, .. } = &outcome {
        emit_complete(ctx, &step_name, step_index, output, 0)?;
    }
    Ok(outcome)
}

/// `forward E.Op(args)` (D12) — propagate to the next OUTER frame.
///
/// It is NOT dispatched here. A `forward` is a clause-body TERMINATOR: it
/// returns a sentinel that the `perform` which invoked this clause catches, and
/// that `perform` resumes the search from strictly outside the frame it just
/// left. Doing the outward search here would be a second implementation of the
/// same walk — and the two would drift the first time one of them changed.
pub async fn run_forward(
    node: &IREffectForward,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    if node.effect_name.is_empty() {
        return Err(DispatchError::BackendError {
            name: "algebraic_effects".to_string(),
            message: format!(
                "forward '{}' reached dispatch with no resolved effect (axon-T964 / \
                 axon-T965 refuse this statically).",
                node.operation_name
            ),
        });
    }
    Ok(NodeOutcome::EffectForwarded {
        effect: node.effect_name.clone(),
        operation: node.operation_name.clone(),
        arguments: node
            .arguments
            .iter()
            .map(|a| resolve_argument(a, ctx))
            .collect(),
    })
}

/// `resume(v)` — invoke the captured one-shot continuation (D2).
pub async fn run_resume(
    node: &IREffectResume,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    Ok(NodeOutcome::EffectResumed {
        value: resolve_argument(&node.value_expr, ctx),
    })
}

/// `abort(v)` — leave the enclosing handle without resuming.
///
/// The frame it belongs to is the INNERMOST live frame: an `abort` written in a
/// clause of frame F runs with the stack truncated to F's outer frames, so F
/// itself is not visible — which is why the id is taken from the frame the
/// dispatching `perform` selected, threaded on `ctx.effect_aborting_frame`.
pub async fn run_abort(
    node: &IREffectAbort,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    Ok(NodeOutcome::EffectAborted {
        frame_id: ctx.effect_clause_frame.unwrap_or(u32::MAX),
        value: resolve_argument(&node.value_expr, ctx),
    })
}

// ════════════════════════════════════════════════════════════════════
//  The shared dispatch: one operation against the frame stack
// ════════════════════════════════════════════════════════════════════

/// Find the innermost frame below `search_from` that handles `effect`, run its
/// clause for `operation`, and interpret the discharge.
///
/// `search_from` is EXCLUSIVE: `ctx.effect_frames.len()` for a `perform` (the
/// whole stack is visible), and the source frame's index for a `forward` (so the
/// frame being left is bypassed, along with everything nested inside it).
///
/// Loops rather than recurses on `forward`, so a chain of decorating handlers
/// cannot blow the stack — and because an `async fn` that recursed here would
/// need boxing at every level for no benefit.
async fn dispatch_operation(
    effect: &str,
    operation: &str,
    args: &[String],
    search_from: usize,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    let mut effect = effect.to_string();
    let mut operation = operation.to_string();
    let mut args: Vec<String> = args.to_vec();
    let mut search_from = search_from;

    loop {
        let Some(frame_idx) = find_frame(ctx, &effect, search_from) else {
            // D9's runtime half. A REFUSAL, never a fallback: an effect with no
            // handler has nowhere to yield to, and completing quietly would put
            // the program in a state its author never wrote.
            return Err(DispatchError::BackendError {
                name: "algebraic_effects".to_string(),
                message: format!(
                    "unhandled effect '{effect}.{operation}': no enclosing `handle {effect}` \
                     is in scope at this point. The compiler refuses this statically \
                     (axon-T966, the design plan D9 — there is no runtime fallback); reaching it \
                     here means the IR was not produced by `axon check`."
                ),
            });
        };

        let frame = ctx.effect_frames[frame_idx].clone();
        let Some(clause) = frame
            .clauses
            .iter()
            .find(|c| c.operation_name == operation)
        else {
            return Err(DispatchError::BackendError {
                name: "algebraic_effects".to_string(),
                message: format!(
                    "handler for '{effect}' declares no clause for operation '{operation}' \
                     (it handles: {}). The compiler refuses this statically (axon-T962 / \
                     axon-T965).",
                    frame
                        .clauses
                        .iter()
                        .map(|c| c.operation_name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        };

        // Bind the clause parameters positionally, saving whatever they shadow.
        // The restore below is unconditional — a clause must not leak a binding
        // into the continuation it resumes, or the perform site would silently
        // see the handler's locals.
        let saved: Vec<(String, Option<String>)> = clause
            .parameter_names
            .iter()
            .map(|n| (n.clone(), ctx.let_bindings.get(n).cloned()))
            .collect();
        for (name, value) in clause.parameter_names.iter().zip(args.iter()) {
            ctx.let_bindings.insert(name.clone(), value.clone());
        }

        // Scope discipline: the clause body sees strictly OUTER frames. This is
        // what makes `forward` reach the next handler out, and what stops a
        // clause that performs its own operation from dispatching itself.
        let parked: Vec<EffectFrame> = ctx.effect_frames.split_off(frame_idx);
        let prior_clause_frame = ctx.effect_clause_frame.replace(frame.frame_id);

        let body_result =
            crate::flow_dispatcher::orchestration::dispatch_body_public(&clause.body, ctx).await;

        ctx.effect_clause_frame = prior_clause_frame;
        ctx.effect_frames.extend(parked);
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

        match body_result? {
            // `resume(v)` — the perform site receives `v` and its surrounding
            // block continues. The continuation is consumed here, exactly once
            // (D2): there is nothing to clone, because the continuation IS the
            // caller's remaining node list.
            NodeOutcome::EffectResumed { value } => {
                return Ok(NodeOutcome::Completed {
                    output: value,
                    tokens_emitted: 0,
                    step_index: ctx.step_counter.saturating_sub(1),
                });
            }
            // `abort(v)` — propagate to the frame that owns it. `run_handle`
            // converts it when the id matches; every other construct passes it
            // through, so an inner handler cannot swallow an outer one's exit.
            NodeOutcome::EffectAborted { frame_id, value } => {
                return Ok(NodeOutcome::EffectAborted { frame_id, value });
            }
            // `forward` — resume the search strictly outside the frame we just
            // left, with whatever operation the clause named.
            NodeOutcome::EffectForwarded {
                effect: fwd_effect,
                operation: fwd_operation,
                arguments,
            } => {
                effect = fwd_effect;
                operation = fwd_operation;
                args = arguments;
                search_from = frame_idx;
                continue;
            }
            // The clause ran off its end without resuming. section 3.1 publishes
            // exactly this (`Done() -> { websocket.close() }`, *"sin resume —
            // abort implícito"*), so it ABORTS the handle with the clause's last
            // output rather than raising `NoDischarge` the way the v1.17.0 engine
            // does. The continuation is simply dropped — nothing was allocated
            // to release.
            NodeOutcome::Completed { output, .. } => {
                return Ok(NodeOutcome::EffectAborted {
                    frame_id: frame.frame_id,
                    value: output,
                });
            }
            // A `return` / `break` / `continue` / hibernation inside a clause
            // body has no defined meaning: the clause is not a loop body and not
            // a flow tail, and inventing one would silently reshape control
            // flow. Refused, naming the construct.
            other => {
                return Err(DispatchError::BackendError {
                    name: "algebraic_effects".to_string(),
                    message: format!(
                        "handler clause '{operation}' ended with {other:?}, which has no \
                         defined meaning inside a handler: a clause discharges via \
                         `resume`, `abort` or `forward`, or runs off its end (an implicit \
                         abort). Refused rather than reshaped into one of those."
                    ),
                });
            }
        }
    }
}

/// Walk the frame stack outward from `start_exclusive - 1` toward 0, returning
/// the first frame that names `effect`.
fn find_frame(ctx: &DispatchCtx, effect: &str, start_exclusive: usize) -> Option<usize> {
    let start = start_exclusive.min(ctx.effect_frames.len());
    (0..start)
        .rev()
        .find(|&i| ctx.effect_frames[i].effect_names.iter().any(|e| e == effect))
}

/// Resolve one argument / value expression against the live bindings.
///
/// A quoted argument is a LITERAL (v2.10.0's classification, preserved by v2.83.0's
/// parser); anything else is a reference the runtime looks up, so
/// `perform Emit(Gen.output)` hands the handler the step's VALUE, not the name
/// `Gen.output`. Putting the name on the wire is the specific defect that made
/// `perform` belong on `IRStep::performs` rather than `pix_ops`.
fn resolve_argument(expr: &str, ctx: &DispatchCtx) -> String {
    if expr.is_empty() {
        return String::new();
    }
    match expr.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Some(literal) => literal.to_string(),
        None => crate::exec_context::resolve_value_reference(expr, &ctx.let_bindings),
    }
}

// ── wire events ─────────────────────────────────────────────────────

fn emit_start(
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

fn emit_complete(
    ctx: &mut DispatchCtx,
    step_name: &str,
    step_index: usize,
    output: &str,
    tokens_emitted: u64,
) -> Result<(), DispatchError> {
    ctx.tx
        .send(FlowExecutionEvent::StepComplete {
            step_name: step_name.to_string(),
            step_index,
            success: true,
            full_output: output.to_string(),
            tokens_input: 0,
            tokens_output: tokens_emitted,
            branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)
}
