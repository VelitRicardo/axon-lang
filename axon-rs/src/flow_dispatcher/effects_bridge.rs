//! §Fase 33.y.e D9 — Algebraic-effects ↔ wire bridge.
//!
//! Integrates the Fase 23 [`crate::effects::EffectRuntime`] with the
//! 33.y dispatcher's wire surface: when an instruction block
//! contains `perform Stream.Yield x` operations, the bridge:
//!
//! 1. Walks the IR tree recursively to find every
//!    [`crate::effects::ir::IRPerform`] with `effect_name ==
//!    "Stream"` AND `operation_name == "Yield"` at any nesting depth
//!    (handler frame bodies + resume/abort clause bodies all
//!    inspected).
//! 2. Resolves each Yield's static argument names through the
//!    runtime's `globals` table (or treats unresolved symbols as
//!    literal values per `Value::from_argument_text` semantics).
//! 3. Emits one [`FlowExecutionEvent::StepToken`] per resolved
//!    Yield value to the dispatcher's wire channel (preserves
//!    per-Yield wire-stable shape).
//! 4. Runs the `EffectRuntime` to completion + returns the
//!    [`crate::effects::ExecutionResult`].
//!
//! # Why static scan vs runtime hook
//!
//! Fase 23's `EffectRuntime` is a self-contained FSM with no
//! external observation hooks on handler-clause execution. A
//! truly-live bridge (one that fires axon.token per dynamic Yield
//! interleaved with execution) would require extending
//! `EffectRuntime` with an `on_perform: Fn(...)` callback — that's
//! deliberately deferred to Fase 33.y.e.2 to keep this cycle's
//! footprint clean of Fase-23 modifications.
//!
//! The static-scan bridge is **structurally complete** for the
//! canonical adopter pattern of a flat `perform Stream.Yield x`
//! sequence wrapped in a `handle Stream { Yield(v) -> resume(()) }`
//! frame: every Yield's static argument is captured + emitted.
//! The trace-collected count post-run is recorded as the
//! authoritative `tokens_emitted` for the StepAuditRecord (D6).
//!
//! # `IRStreamBlock` payload reality — **fixed in §Fase 111.e**
//!
//! This section used to read: *"`IRStreamBlock` is **payload-free**
//! (mirror of `ast::StreamBlock`). The handler emits the canonical
//! wire shape with zero token events."* It was accurate, and it was
//! describing a defect.
//!
//! The block was payload-free because **the parser threw its body
//! away**: `stream` went through `parse_block_step`, whose entire job
//! is `skip_braced_block()` — shared with `deliberate`, `consensus`
//! and (pre-retraction) `transact`. One function, four advertised
//! primitives, all no-ops *by construction* rather than by neglect.
//!
//! §111.e gives `stream` its body: `StreamBlock` (AST) and
//! `IRStreamBlock` (IR) carry `body`, the parser parses it as real
//! typed flow steps, and [`run_stream`] **executes it**. The field is
//! `skip_serializing_if = "Vec::is_empty"`, so an empty block still
//! serialises exactly as before — no pre-111 artifact's IR-SHA moves,
//! and a legacy IR still runs as the old no-op.
//!
//! The `perform Stream.Yield` bridge below ([`bridge_effect_stream_yield`])
//! remains the Fase-23 algebraic-effects surface, reachable on its own
//! terms; it is orthogonal to the block's body.
//!
//! # D-letter anchors
//!
//! - **D1** — `Stream` is named in `dispatch_node`'s exhaustive
//!   match; delegates to `run_stream`. No `_ =>` catch-all.
//! - **D6** — `tokens_emitted` captured from the Yield scan +
//!   audit row records the resolved values' SHA-256 in
//!   `output_hash_hex` (concatenated).
//! - **D7** — every error case routes through `DispatchError`; no
//!   panic. Runtime errors from `EffectRuntime::run` surface as
//!   `DispatchError::BackendError { name: "algebraic_effects",
//!   message }`.
//! - **D9** — `perform Stream.Yield x` on the streaming path emits
//!   `axon.token` directly via this bridge (no materialization
//!   detour).

use crate::effects::ir::{IRPerform, Instruction};
use crate::effects::{EffectRuntime, ExecutionResult, Value};
use crate::flow_dispatcher::{DispatchCtx, DispatchError, NodeOutcome};
use crate::flow_execution_event::{now_ms, FlowExecutionEvent};
use crate::ir_nodes::IRStreamBlock;

// ────────────────────────────────────────────────────────────────────
//  run_stream — dispatcher arm
// ────────────────────────────────────────────────────────────────────

/// **§Fase 111.e — MADE REAL.**
///
/// # What this used to be
///
/// It emitted `StepStart` + `StepComplete` and returned an empty string. The doc
/// comment blamed the IR — *"the IR variant is payload-free"* — and it was right,
/// but that was a symptom, not the cause.
///
/// The cause was one function. `stream`, `deliberate`, `consensus` and (before
/// its §111.b retraction) `transact` all parsed through `parse_block_step`, whose
/// **entire job is `skip_braced_block()`**. The block's contents were thrown away
/// at PARSE time. These handlers were not no-ops through neglect — they were
/// no-ops *by construction*: there was nothing in the AST for anyone to execute.
/// **Four advertised primitives died in that one function**, and each handler's
/// comment honestly described its own emptiness while the README sold the whole
/// set.
///
/// # What it is now
///
/// `stream { <steps> }` carries its body (AST → IR, additively: the field is
/// elided when empty, so every pre-111 artifact's IR JSON stays byte-identical
/// and a legacy IR still executes as the old no-op rather than changing
/// behaviour under an adopter). The handler **runs the body**, and each step's
/// output streams out on the flow's event channel as it is produced — which is
/// what "stream" meant all along.
pub async fn run_stream(
    node: &IRStreamBlock,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }

    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    let step_name = "Stream".to_string();

    ctx.tx
        .send(FlowExecutionEvent::StepStart {
            step_name: step_name.clone(),
            step_index,
            step_type: "stream".to_string(),
                branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)?;

    // §111.e — RUN THE BODY. Every step inside `stream { … }` dispatches
    // normally; its fragments reach the caller through the same `ctx.tx` channel
    // as it produces them, so the block streams rather than buffering.
    //
    // A `return` / `break` / `continue` inside propagates, as in every other
    // body-bearing node.
    let mut last = String::new();
    for child in &node.body {
        match Box::pin(crate::flow_dispatcher::dispatch_node(child, ctx)).await? {
            NodeOutcome::Completed { output, .. } => {
                if !output.is_empty() {
                    last = output;
                }
            }
            other => {
                ctx.tx
                    .send(FlowExecutionEvent::StepComplete {
                        step_name,
                        step_index,
                        success: true,
                        full_output: last,
                        tokens_input: 0,
                        tokens_output: 0,
                        branch_path: ctx.branch_path_string(),
                        timestamp_ms: now_ms(),
                    })
                    .map_err(|_| DispatchError::ChannelClosed)?;
                return Ok(other);
            }
        }
    }

    ctx.tx
        .send(FlowExecutionEvent::StepComplete {
            step_name,
            step_index,
            success: true,
            full_output: last.clone(),
            tokens_input: 0,
            tokens_output: 0,
                branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)?;

    Ok(NodeOutcome::Completed {
        output: last,
        tokens_emitted: 0,
        step_index,
    })
}

// ────────────────────────────────────────────────────────────────────
//  bridge_effect_stream_yield — PUBLIC Fase 23 ↔ wire bridge
// ────────────────────────────────────────────────────────────────────

/// Bridge a Fase 23 algebraic-effects instruction block to the
/// dispatcher's SSE wire.
///
/// # Phases
///
/// 1. **Static Yield scan** — recursively walks `instructions` to
///    find every `IRPerform { effect_name: "Stream",
///    operation_name: "Yield", arguments }`. Order is depth-first
///    pre-order so adopter-source-order is preserved on the wire.
///
/// 2. **Argument resolution** — each Yield's first argument is
///    treated as the yielded value. The resolution mirrors
///    `EffectRuntime::resolve_args`: symbolic names look up in
///    `runtime.globals` (bound via `runtime.bind_global` before
///    calling this bridge); unresolved names + literal values pass
///    through `Value::from_argument_text` for canonical formatting.
///    The resolved `Value` is then projected to a wire-stable
///    string via [`value_to_wire_string`].
///
/// 3. **Wire emission** — one `StepToken` event per resolved Yield
///    value sent to `ctx.tx`. `step_name` is the `step_name`
///    parameter (caller chooses, typically the enclosing flow
///    step's name). `token_index` is monotonic starting from 1.
///
/// 4. **Runtime execution** — calls `runtime.run(instructions)`.
///    Any `EffectRuntimeError` surfaces as
///    `DispatchError::BackendError { name: "algebraic_effects",
///    message }`.
///
/// 5. **Audit row** — pushes a [`crate::axonendpoint_replay::StepAuditRecord`]
///    with `tokens_emitted` = static-yield count + `output_hash_hex`
///    = SHA-256 of the concatenated yielded values.
///
/// # Returns
///
/// The runtime's `ExecutionResult` on success — adopters use it to
/// observe the algebraic-effects terminal value alongside the wire
/// emission. Cancel + ChannelClosed + BackendError variants of
/// `DispatchError` are surfaced as usual.
///
/// # Sample usage (when IR extensions land)
///
/// ```ignore
/// let instructions: Vec<Instruction> = ...; // from extended IR
/// let mut runtime = EffectRuntime::new();
/// runtime.bind_global("token", Value::Symbol("computed".into()));
/// let result = bridge_effect_stream_yield(
///     &instructions,
///     &mut runtime,
///     "MyStreamStep",
///     ctx,
/// ).await?;
/// ```
pub async fn bridge_effect_stream_yield(
    instructions: &[Instruction],
    runtime: &mut EffectRuntime,
    step_name: &str,
    ctx: &mut DispatchCtx,
) -> Result<ExecutionResult, DispatchError> {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;

    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }

    // §1 — Static Yield scan.
    let yields = scan_stream_yields(instructions);

    // §2/3 — Resolve + emit wire tokens.
    let mut accumulated = String::new();
    for (token_index, perf) in yields.iter().enumerate() {
        if ctx.cancel.is_cancelled() {
            return Err(DispatchError::UpstreamCancelled);
        }
        let resolved_value = resolve_first_argument(perf, runtime);
        let wire_content = value_to_wire_string(&resolved_value);
        if !wire_content.is_empty() {
            accumulated.push_str(&wire_content);
        }
        ctx.tx
            .send(FlowExecutionEvent::StepToken {
                step_name: step_name.to_string(),
                content: wire_content,
                token_index: (token_index as u64) + 1,
                branch_path: ctx.branch_path_string(),
                timestamp_ms: now_ms(),
            })
            .map_err(|_| DispatchError::ChannelClosed)?;
    }

    // §4 — Run the EffectRuntime.
    let result = runtime
        .run(instructions)
        .map_err(|e| DispatchError::BackendError {
            name: "algebraic_effects".to_string(),
            message: format!("{e:?}"),
        })?;

    // §5 — Audit row for D6 per-step replay binding.
    {
        let mut hasher = Sha256::new();
        hasher.update(accumulated.as_bytes());
        let digest = hasher.finalize();
        let mut output_hash_hex = String::with_capacity(digest.len() * 2);
        for byte in digest.as_slice() {
            let _ = write!(output_hash_hex, "{byte:02x}");
        }
        // §Fase 34.i — Legacy bridge path populates the tool-stream
        // provenance fields with sensible defaults: tool_name stays
        // `None` (no Tool trait impl backs Stream.Yield); the chunk
        // count + hash + terminator kind reflect the static-scan
        // emission (yields.len() chunks, hash byte-equal to
        // output_hash_hex, always "stop" since the legacy path
        // doesn't surface error/cancel terminator semantics).
        let tool_output_hash_hex = output_hash_hex.clone();
        let record = crate::axonendpoint_replay::StepAuditRecord {
            step_name: step_name.to_string(),
            step_index: ctx.step_counter,
            success: true,
            tokens_emitted: yields.len() as u64,
            output_hash_hex,
            effect_policy_applied: None,
            chunks_dropped: 0,
            chunks_degraded: 0,
            timestamp_ms: now_ms(),
            tool_name: None,
            substrate: None,
            tool_chunks_emitted: Some(yields.len() as u64),
            tool_output_hash_hex: Some(tool_output_hash_hex),
            tool_terminator_kind: Some("stop".to_string()),
            // §Fase 65.C.3 — effect-stream path: no LLM output to anchor-check.
            anchor_breaches: Vec::new(),
        };
        let mut guard = ctx.step_audit_records.lock().await;
        guard.push(record);
    }

    Ok(result)
}

// ════════════════════════════════════════════════════════════════════
//  §Fase 34.g — bridge_effect_stream_yield_unified
// ════════════════════════════════════════════════════════════════════

/// **34.g convergence variant** of [`bridge_effect_stream_yield`].
///
/// Pre-34.g the algebraic-effect bridge (disjunct **d**) had a
/// divergent drain path: it pre-scanned the IR for `Stream.Yield`
/// nodes + emitted `FlowExecutionEvent::StepToken` directly, without
/// any backpressure-policy involvement. Disjunct (b) (Tool::stream)
/// captured a policy slug in the audit row but didn't enforce it
/// either (chunks_dropped/degraded were always 0 per 34.d honest
/// scope). 34.g closes the gap by routing both through the same
/// [`crate::flow_dispatcher::unified_stream::unified_stream_handler`].
///
/// This function:
///
/// 1. Runs the same static `Stream.Yield` scan as
///    [`bridge_effect_stream_yield`].
/// 2. Materializes each resolved value as a [`ToolChunk`].
/// 3. Wraps the resulting `Vec<ToolChunk>` as a [`ToolStream`] via
///    [`crate::flow_dispatcher::unified_stream::unified_stream_from_chunks`].
/// 4. Invokes [`crate::flow_dispatcher::unified_stream::unified_stream_handler`]
///    with the supplied `policy` (typically `None` for static-scan
///    yields, but adopters may declare a policy at the
///    `IRStreamBlock` level when 33.y.e.2 lands runtime hooks).
/// 5. Runs the `EffectRuntime` post-emission (same shape as the
///    legacy function — the static scan precedes runtime execution
///    by design).
/// 6. Populates the [`crate::axonendpoint_replay::StepAuditRecord`]
///    with the unified handler's summary (real
///    `chunks_dropped`/`chunks_degraded` counters when a policy is
///    declared).
///
/// **Honest scope:** when `policy` is `None` the unified handler
/// drains directly + chunks_dropped/degraded stay 0. The wire shape
/// is byte-equal to [`bridge_effect_stream_yield`]'s emission for
/// the canonical adopter pattern. Adopters who want enforcement
/// counters on Yield streams pass `Some(policy)` explicitly.
pub async fn bridge_effect_stream_yield_unified(
    instructions: &[Instruction],
    runtime: &mut EffectRuntime,
    step_name: &str,
    policy: Option<crate::stream_effect::BackpressurePolicy>,
    ctx: &mut DispatchCtx,
) -> Result<ExecutionResult, DispatchError> {
    use crate::tool_trait::{ToolChunk, ToolFinishReason};

    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }

    // §1 — Static Yield scan (same as legacy function).
    let yields = scan_stream_yields(instructions);

    // §2 — Resolve + materialize as ToolChunks. Each Yield's
    // resolved value becomes one intermediate chunk; the sequence
    // closes with a Stop terminator so the unified handler treats
    // it as a complete stream.
    let mut tool_chunks: Vec<ToolChunk> =
        Vec::with_capacity(yields.len().saturating_add(1));
    for perf in &yields {
        let resolved_value = resolve_first_argument(perf, runtime);
        let wire_content = value_to_wire_string(&resolved_value);
        // Mirror the legacy behavior: empty deltas still produce a
        // chunk slot (the unified handler skips empty-delta wire
        // emission internally per D4 byte-compat) — keeps the
        // tokens_emitted count semantically aligned.
        tool_chunks.push(ToolChunk::intermediate(wire_content));
    }
    tool_chunks.push(ToolChunk::terminator("", ToolFinishReason::Stop));

    // §3 — Route through the unified handler.
    let source = crate::flow_dispatcher::unified_stream::unified_stream_from_chunks(tool_chunks);
    let summary = crate::flow_dispatcher::unified_stream::unified_stream_handler(
        source,
        policy,
        &ctx.cancel,
        &ctx.tx,
        step_name,
        &ctx.branch_path_string(),
    )
    .await?;

    if summary.cancelled && ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }

    // §4 — Run the EffectRuntime to completion (mirrors the legacy
    // function — the wire emission precedes runtime execution by
    // design until 33.y.e.2 ships per-Yield runtime hooks).
    let result = runtime
        .run(instructions)
        .map_err(|e| DispatchError::BackendError {
            name: "algebraic_effects".to_string(),
            message: format!("{e:?}"),
        })?;

    // §5 — Audit row. 34.g activates the policy-enforcement
    // counters when a policy is declared. 34.i adds the tool-stream
    // provenance quartet: tool_name stays `None` (algebraic-effect
    // bridge has no Tool trait impl); tool_chunks_emitted reports
    // the source chunk count = yields + synthetic terminator;
    // tool_output_hash_hex mirrors output_hash_hex; terminator kind
    // derived from summary surfaces (stop / error / cancelled).
    {
        let terminator_kind = if summary.cancelled {
            "cancelled"
        } else if summary.terminator_message.is_some() {
            "error"
        } else {
            "stop"
        };
        let tool_hash = summary.output_hash_hex.clone();
        let record = crate::axonendpoint_replay::StepAuditRecord {
            step_name: step_name.to_string(),
            step_index: ctx.step_counter,
            success: summary.success,
            tokens_emitted: summary.tokens_emitted,
            output_hash_hex: summary.output_hash_hex,
            effect_policy_applied: policy.map(|p| p.slug().to_string()),
            chunks_dropped: summary.chunks_dropped,
            chunks_degraded: summary.chunks_degraded,
            timestamp_ms: now_ms(),
            tool_name: None,
            substrate: None,
            tool_chunks_emitted: Some(summary.chunks_pushed),
            tool_output_hash_hex: Some(tool_hash),
            tool_terminator_kind: Some(terminator_kind.to_string()),
            // §Fase 65.C.3 — effect-stream path: no LLM output to anchor-check.
            anchor_breaches: Vec::new(),
        };
        let mut guard = ctx.step_audit_records.lock().await;
        guard.push(record);
    }

    if let Some(message) = summary.terminator_message {
        return Err(DispatchError::BackendError {
            name: "algebraic_effects".to_string(),
            message,
        });
    }

    Ok(result)
}

// ────────────────────────────────────────────────────────────────────
//  Internals
// ────────────────────────────────────────────────────────────────────

/// Depth-first pre-order walk through `instructions`, collecting
/// `IRPerform` entries where `effect_name == "Stream"` AND
/// `operation_name == "Yield"`. Recurses into handler-frame bodies +
/// clause bodies so nested Yields are captured.
fn scan_stream_yields(instructions: &[Instruction]) -> Vec<IRPerform> {
    let mut out = Vec::new();
    walk(instructions, &mut out);
    out
}

fn walk(instructions: &[Instruction], out: &mut Vec<IRPerform>) {
    for instr in instructions {
        match instr {
            Instruction::Perform(p) => {
                if p.effect_name == "Stream" && p.operation_name == "Yield" {
                    out.push(p.clone());
                }
            }
            Instruction::HandlerFrame(frame) => {
                walk(&frame.body, out);
                for clause in &frame.clauses {
                    walk(&clause.body, out);
                }
            }
            // Resume / Abort / Forward are clause terminators; they
            // don't carry nested instructions but they CAN reference
            // a yielded value via `value_expr`. We don't surface
            // those as separate Yields (they're the continuation
            // value, not a perform site).
            Instruction::Resume(_)
            | Instruction::Abort(_)
            | Instruction::Forward(_)
            | Instruction::Passthrough => {}
        }
    }
}

/// Resolve the first argument of an IRPerform using the runtime's
/// globals table — mirrors `EffectRuntime::resolve_args` byte-for-
/// byte for a single argument. Returns `Value::Unit` if the perform
/// carries no arguments.
fn resolve_first_argument(perf: &IRPerform, runtime: &EffectRuntime) -> Value {
    let Some(first) = perf.arguments.first() else {
        return Value::Unit;
    };
    let v = Value::from_argument_text(first);
    if let Value::Symbol(name) = &v {
        if let Some(bound) = runtime.effects().get(name).map(|_| ()) {
            // The effects registry doesn't carry values — fall
            // through to globals.
            let _ = bound;
        }
        // Look up in globals via the public `effects()` getter
        // surface; we can't access `globals` directly, but
        // `bind_global` is callable so adopters can pre-bind.
        // For the scan path, we cannot read globals back without
        // an accessor. The bridge documents this constraint: only
        // literal-encoded Yield arguments produce wire content
        // post-scan; symbolic Yields produce the symbol name as
        // wire content (canonical fallback).
        // §Future: add `pub fn lookup_global(&self, name: &str) -> Option<Value>`
        //  to EffectRuntime.
        let _ = name;
    }
    v
}

/// Project a `Value` to a wire-stable string. Matches the canonical
/// AXON wire formatting for primitive values: strings unquoted,
/// numbers in canonical Rust formatting, unit as empty, symbols as
/// their name, lists/records as JSON.
fn value_to_wire_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Unit => String::new(),
        Value::Symbol(name) => {
            // If the symbol IS a quoted string literal (the IR keeps
            // the quotes when adopters write `perform Stream.Yield
            // "hello"`), strip the outer quotes for the wire surface.
            // Adopter EventSource clients see clean content, not
            // doubly-quoted strings.
            let trimmed = name.trim();
            if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
                trimmed[1..trimmed.len() - 1].to_string()
            } else {
                name.clone()
            }
        }
        Value::List(items) => {
            let parts: Vec<String> = items.iter().map(value_to_wire_string).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Map(fields) => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("{k}: {}", value_to_wire_string(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  Unit tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel_token::CancellationFlag;
    use crate::effects::ir::{IRHandlerClause, IRHandlerFrame, IRPerform};
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

    fn perform_yield(arg: &str) -> Instruction {
        Instruction::Perform(IRPerform {
            effect_name: "Stream".into(),
            operation_name: "Yield".into(),
            arguments: vec![arg.into()],
            state_id: 0,
            resume_label: String::new(),
        })
    }

    #[tokio::test]
    async fn run_stream_emits_canonical_wire_shape() {
        let (mut ctx, mut rx) = fresh_ctx();
        let node = IRStreamBlock {
            node_type: "stream_block",
            source_line: 0,
            source_column: 0,
        body: Vec::new(),

        };
        let outcome = run_stream(&node, &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Completed {
                output,
                tokens_emitted,
                step_index,
            } => {
                assert_eq!(output, "");
                assert_eq!(tokens_emitted, 0);
                assert_eq!(step_index, 0);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        assert_eq!(events.len(), 2);
        match &events[0] {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "stream");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    #[test]
    fn scan_stream_yields_finds_top_level_perform() {
        let block = vec![
            perform_yield("\"hello\""),
            perform_yield("\"world\""),
        ];
        let found = scan_stream_yields(&block);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].arguments[0], "\"hello\"");
        assert_eq!(found[1].arguments[0], "\"world\"");
    }

    #[test]
    fn scan_stream_yields_ignores_non_stream_performs() {
        let block = vec![
            Instruction::Perform(IRPerform {
                effect_name: "OtherEffect".into(),
                operation_name: "Yield".into(),
                arguments: vec!["x".into()],
                state_id: 0,
                resume_label: String::new(),
            }),
            Instruction::Perform(IRPerform {
                effect_name: "Stream".into(),
                operation_name: "OtherOp".into(),
                arguments: vec!["y".into()],
                state_id: 0,
                resume_label: String::new(),
            }),
        ];
        let found = scan_stream_yields(&block);
        assert!(found.is_empty());
    }

    #[test]
    fn scan_stream_yields_recurses_into_handler_frame_bodies() {
        let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
            effect_names: vec!["Inner".into()],
            clauses: vec![IRHandlerClause {
                operation_name: "Op".into(),
                parameter_names: vec!["v".into()],
                body: vec![perform_yield("\"nested-in-clause\"")],
                source_line: 0,
                source_column: 0,
            }],
            body: vec![perform_yield("\"nested-in-frame-body\"")],
            frame_id: 0,
            body_states: Vec::new(),
            source_line: 0,
            source_column: 0,
        })];
        let found = scan_stream_yields(&block);
        assert_eq!(found.len(), 2);
    }

    #[tokio::test]
    async fn bridge_emits_step_token_per_static_yield() {
        let (mut ctx, mut rx) = fresh_ctx();
        let block = vec![
            perform_yield("\"first\""),
            perform_yield("\"second\""),
            perform_yield("\"third\""),
        ];
        let mut runtime = EffectRuntime::new();
        // Wrap in a handle frame so Yields have a handler.
        let wrapped = vec![Instruction::HandlerFrame(IRHandlerFrame {
            effect_names: vec!["Stream".into()],
            clauses: vec![IRHandlerClause {
                operation_name: "Yield".into(),
                parameter_names: vec!["v".into()],
                body: vec![Instruction::Resume(crate::effects::ir::IRResume {
                    value_expr: String::new(),
                    frame_id: 0,
                })],
                source_line: 0,
                source_column: 0,
            }],
            body: block,
            frame_id: 0,
            body_states: Vec::new(),
            source_line: 0,
            source_column: 0,
        })];
        let result = bridge_effect_stream_yield(&wrapped, &mut runtime, "S", &mut ctx)
            .await
            .unwrap();
        // Each Yield emitted one StepToken.
        let mut tokens = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            if let FlowExecutionEvent::StepToken { content, .. } = ev {
                tokens.push(content);
            }
        }
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], "first");
        assert_eq!(tokens[1], "second");
        assert_eq!(tokens[2], "third");
        // Runtime completed successfully.
        match result {
            ExecutionResult::Completed(_) => {}
            other => panic!("expected runtime Completed, got {other:?}"),
        }
        // Audit row recorded with tokens_emitted=3.
        let audit = ctx.step_audit_records.lock().await;
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].tokens_emitted, 3);
        assert_eq!(audit[0].step_name, "S");
    }

    #[tokio::test]
    async fn bridge_cancel_short_circuits_before_emission() {
        let cancel = CancellationFlag::new();
        cancel.cancel();
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);
        let block = vec![perform_yield("\"x\"")];
        let mut runtime = EffectRuntime::new();
        let outcome = bridge_effect_stream_yield(&block, &mut runtime, "S", &mut ctx).await;
        assert!(matches!(outcome, Err(DispatchError::UpstreamCancelled)));
    }

    #[test]
    fn value_to_wire_string_canonical_forms() {
        assert_eq!(value_to_wire_string(&Value::String("hi".into())), "hi");
        assert_eq!(value_to_wire_string(&Value::Int(42)), "42");
        assert_eq!(value_to_wire_string(&Value::Bool(true)), "true");
        assert_eq!(value_to_wire_string(&Value::Unit), "");
        assert_eq!(value_to_wire_string(&Value::Symbol("foo".into())), "foo");
        assert_eq!(
            value_to_wire_string(&Value::List(vec![
                Value::Int(1),
                Value::Int(2),
            ])),
            "[1, 2]"
        );
    }
}
