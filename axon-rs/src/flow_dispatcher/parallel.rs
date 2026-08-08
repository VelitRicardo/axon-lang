//! §Fase 33.y.e — Parallel variant handler + concurrent dispatch
//! helper.
//!
//! # `IRParallelBlock` payload reality
//!
//! In v1.25.0's AST/IR, `IRParallelBlock` is **payload-free** — it
//! carries only the source-loc metadata; the branches themselves
//! are not decomposed into the IR shape. This is consistent with
//! `ast::ParBlock` which is also payload-free.
//!
//! The Fase 33.y.e cycle ships the PRODUCTION-READY concurrent-
//! dispatch machinery as a PUBLIC helper
//! [`run_branches_concurrently`] that future AST/IR extensions
//! consume; when the IR gains a `branches: Vec<Vec<IRFlowNode>>`
//! field, `run_par` will read it + delegate. Today the handler
//! itself emits the canonical `step_type: "par"` wire shape with
//! zero token events — the wire surface is locked in lockstep
//! with the helper so adopters relying on `axon.step_start` event
//! type filtering already see the variant.
//!
//! # Concurrent dispatch model
//!
//! [`run_branches_concurrently`] takes N branch bodies. For each:
//!
//! 1. Clones the parent [`DispatchCtx`] (snapshot of let_bindings
//!    + branch_path + step_counter + pending_effect_policy; SHARES
//!    the Arc-wrapped side-channels for audit/enforcement/warnings
//!    + mpsc tx + cancel flag).
//! 2. Pushes `"par[<idx>]"` to the branch's `branch_path`.
//! 3. Constructs an async future that calls
//!    [`crate::flow_dispatcher::orchestration::dispatch_body`] (via
//!    a thin wrapper) on the branch body + its cloned ctx.
//!
//! All branch futures are polled concurrently via
//! [`futures::future::join_all`] — they share the current tokio task.
//! The mpsc `tx` is Clone-able so each branch emits to the same
//! consumer; events arrive interleaved ordered by wall-clock
//! arrival.
//!
//! # Sentinel propagation
//!
//! - `NodeOutcome::Return { value }` from any branch → returned
//!   immediately, other branches' outcomes discarded (Return is
//!   flow-terminating; semantics match the sync runner's
//!   "first-Return-wins" discipline). The pending-cancel for the
//!   other branches is NOT fired explicitly — those tasks complete
//!   naturally on their next .await yielding.
//! - `NodeOutcome::Break` / `NodeOutcome::LoopContinue` — Par has
//!   no loop semantics; sentinels are observed but treated as
//!   `Completed` for the merged outcome (the sentinel was emitted
//!   by a nested handler; Par doesn't propagate it further unless
//!   wrapped inside a ForIn).
//! - `NodeOutcome::Completed` — output captured; tokens_emitted
//!   summed across branches.
//! - `NodeOutcome::LegacyShimHandled` — non-graduated child;
//!   treated as `Completed` with empty output.
//!
//! # let_bindings isolation
//!
//! Each branch gets a CLONE of parent's `let_bindings`. Bindings
//! created INSIDE a branch are PRIVATE to that branch (Par parity:
//! parallel branches don't side-effect a shared dictionary). After
//! join, the parent's `let_bindings` is UNCHANGED.
//!
//! # step_counter merge
//!
//! Each branch starts with the parent's `step_counter` value.
//! After join, the parent's `step_counter` advances to the MAX
//! across branches (i.e., the most steps any single branch
//! performed) — this preserves monotonicity for subsequent
//! sequential dispatches.
//!
//! # Cancellation
//!
//! All branches share the parent's `cancel: CancellationFlag`
//! (Arc-backed Clone). A cancel fired during the join terminates
//! every branch's next `.await` boundary; the join completes with
//! all branches surfacing `Err(UpstreamCancelled)`. Per D3, the p95
//! propagation latency is bounded by the slowest branch's next
//! cancel-aware await — typically ≤100ms for in-body reqwest
//! consumption.
//!
//! # D-letter anchors
//!
//! - **D1** — `Par` is named in `dispatch_node`'s exhaustive match;
//!   delegates to `run_par`. No `_ =>` catch-all.
//! - **D2** — Per-branch `pending_effect_policy` honored via the
//!   per-branch ctx clone (parent's policy is consumed by ITS
//!   branch on entry; sibling branches set their own per-branch
//!   policy via `ctx.pending_effect_policy = Some(_)` before
//!   calling the helper, or carry their own via the cloned ctx).
//! - **D3** — Cancel propagation: shared cancel flag → all branches
//!   short-circuit on next `.await`.
//! - **D6** — `branch_path` segments `"par[<idx>]"` thread the
//!   parallelism shape; nested orchestration inside a branch
//!   appends additional segments.
//! - **D10** — Sync-runner parity: when the sync runner gains Par
//!   support (today it's a no-op block marker), the dispatcher's
//!   merged step_counter + audit-row ordering matches the sync
//!   runner's recursive evaluation order.

use crate::flow_dispatcher::{DispatchCtx, DispatchError, NodeOutcome};
use crate::flow_execution_event::{now_ms, FlowExecutionEvent};
use crate::ir_nodes::{IRFlowNode, IRParallelBlock};

// ────────────────────────────────────────────────────────────────────
//  run_par — dispatcher arm
// ────────────────────────────────────────────────────────────────────

/// Par handler. In v1.25.0 the IR variant is payload-free so this
/// handler emits the canonical wire shape (StepStart with
/// `step_type: "par"` + StepComplete) without dispatching any
/// child bodies. Future IR extensions delegate to
/// [`run_branches_concurrently`] with the extracted branches.
///
/// # Wire shape
///
/// Emits exactly 2 events: StepStart + StepComplete. No StepToken
/// (no LLM dispatch). `step_index` is the reserved index from
/// `ctx.step_counter` (advances by 1).
pub async fn run_par(
    node: &IRParallelBlock,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }

    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    let step_name = "Par".to_string();

    ctx.tx
        .send(FlowExecutionEvent::StepStart {
            step_name: step_name.clone(),
            step_index,
            step_type: "par".to_string(),
                branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)?;

    // §Fase 65 — REAL concurrency. The IR now carries the `par` branches
    // (pre-§65 `IRParallelBlock` was payload-free → this ran as a stub). Fan the
    // branches out concurrently via the established machine; each branch gets a
    // cloned DispatchCtx (sharing the api_key / conversation / anchors / corpora
    // Arcs, with isolated `pinned_conns` so branches don't serialize on one pin).
    // A `par {}` with no branches stays a clean no-op (back-compat).
    let outcome = if node.branches.is_empty() {
        NodeOutcome::Completed {
            output: String::new(),
            tokens_emitted: 0,
            step_index,
        }
    } else {
        run_branches_concurrently(&node.branches, ctx).await?
    };

    let (full_output, tokens_output) = match &outcome {
        NodeOutcome::Completed { output, tokens_emitted, .. } => (output.clone(), *tokens_emitted),
        NodeOutcome::Return { value } => (value.clone(), 0),
        _ => (String::new(), 0),
    };

    ctx.tx
        .send(FlowExecutionEvent::StepComplete {
            step_name,
            step_index,
            success: true,
            full_output: full_output.clone(),
            tokens_input: 0,
            tokens_output,
                branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)?;

    // Propagate a branch's `Return` sentinel (flow-terminating); otherwise the
    // merged completion.
    match outcome {
            // §Fase 119.d — suspending inside a nested construct is refused.
            NodeOutcome::Hibernated { .. } => { return Err(DispatchError::BackendError { name: "hibernate".to_string(), message: "hibernate inside a nested construct (par branch) has no defined continuation shape yet; place it at top-level flow position"
                .to_string() }); }
        NodeOutcome::Return { value } => Ok(NodeOutcome::Return { value }),
        _ => Ok(NodeOutcome::Completed {
            output: full_output,
            tokens_emitted: tokens_output,
            step_index,
        }),
    }
}

// ────────────────────────────────────────────────────────────────────
//  run_branches_concurrently — PUBLIC concurrent dispatch helper
// ────────────────────────────────────────────────────────────────────

/// Run N branch bodies concurrently on the current tokio task via
/// [`futures::future::join_all`] over per-branch DispatchCtx clones.
///
/// # Per-branch ctx isolation
///
/// Each branch gets a CLONE of the parent's DispatchCtx — String/
/// Vec/HashMap fields are deep-cloned; Arc-backed side-channels +
/// CancellationFlag + mpsc `tx` are Clone-cheap (share the
/// underlying resource). The branch's `branch_path` is extended
/// with `"par[<idx>]"`. The branch's `let_bindings` is a snapshot
/// of the parent's bindings; bindings created inside the branch
/// stay private to the branch.
///
/// # Sentinel handling
///
/// First branch to surface `NodeOutcome::Return { value }` wins —
/// the function returns `Return { value }` immediately and the
/// other branches' outcomes are discarded. (Their tasks complete
/// naturally on the next `.await` because they share `join_all`'s
/// future; if cancel is fired the join completes promptly.)
///
/// Break / LoopContinue from a branch are observed but Par has no
/// loop semantics; the branch's outcome is recorded as "completed
/// with no output" rather than propagating the sentinel further.
///
/// # Return shape
///
/// On full success: `NodeOutcome::Completed { output, tokens_emitted,
/// step_index }` where:
/// - `output` is the concatenation of branch outputs joined by `\n`.
/// - `tokens_emitted` is the sum across branches.
/// - `step_index` is the parent's `step_counter` at entry.
///
/// After return, the parent's `step_counter` is advanced to the
/// MAX of all branches' final step_counter values (monotonic
/// post-join semantics).
pub async fn run_branches_concurrently(
    branches: &[Vec<IRFlowNode>],
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    use futures::future::join_all;

    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }

    let entry_step_index = ctx.step_counter;

    // Build per-branch DispatchCtxs. Each branch owns its clone.
    let mut branch_ctxs: Vec<DispatchCtx> = Vec::with_capacity(branches.len());
    for (idx, _body) in branches.iter().enumerate() {
        let mut bc = ctx.clone();
        bc.branch_path.push(format!("par[{idx}]"));
        // Clear `pending_effect_policy` — parent's policy was
        // intended for the Par-block itself (typically none), not
        // for every branch. Branches can set their own per-branch
        // policy by mutating their ctx clone before the helper.
        bc.pending_effect_policy = None;
        // §Fase 37.x.j (D6.a) — Per-branch sub-pin isolation. Each
        // par branch gets a FRESH (independent) `pinned_conns` Arc so
        // branches do NOT serialize on the parent's pin map mutex.
        // When the first store op in a branch runs, the wire-
        // integration handler's take-pin-out logic finds the map
        // empty and lazily acquires a NEW `PoolConnection` for the
        // branch — so each branch's ops on the same store share a
        // single pin within the branch, but DIFFERENT branches use
        // different physical Postgres backends. This preserves
        // par-block concurrency (no false serialization) while
        // still closing the unnamed-statement race WITHIN each
        // branch's linear walk.
        //
        // D6.b (`par(serialized: true)`) would skip this replacement
        // so branches share the parent's Arc<Mutex<>> and serialize
        // on the lock — an honest deferral to a future fase that
        // also lands the parser + AST grammar for `par(serialized:)`.
        bc.pinned_conns = std::sync::Arc::new(std::sync::Mutex::new(
            std::collections::HashMap::new(),
        ));
        branch_ctxs.push(bc);
    }

    // Run all branches concurrently. `join_all` polls them on the
    // current task; each branch's `.await` yields cooperatively
    // when waiting on its child handlers (e.g. Backend::stream()).
    let futures: Vec<_> = branches
        .iter()
        .zip(branch_ctxs.iter_mut())
        .map(|(body, bc)| {
            let body_ref = body.as_slice();
            async move { dispatch_branch_body(body_ref, bc).await }
        })
        .collect();
    let results = join_all(futures).await;

    // Merge results.
    let mut aggregate_output_parts: Vec<String> = Vec::new();
    let mut aggregate_tokens: u64 = 0;
    let mut return_value: Option<String> = None;
    let mut max_step_counter = ctx.step_counter;

    for (bc, outcome) in branch_ctxs.iter().zip(results.into_iter()) {
        max_step_counter = max_step_counter.max(bc.step_counter);
        match outcome {
            // §Fase 119.d — suspending inside a nested construct is refused.
            Ok(NodeOutcome::Hibernated { .. }) => { return Err(DispatchError::BackendError { name: "hibernate".to_string(), message: "hibernate inside a nested construct (par branch) has no defined \n                 continuation shape yet; place it at top-level flow position"
                .to_string() }); }
            Ok(NodeOutcome::Completed {
                output,
                tokens_emitted,
                ..
            }) => {
                if !output.is_empty() {
                    aggregate_output_parts.push(output);
                }
                aggregate_tokens += tokens_emitted;
            }
            Ok(NodeOutcome::Break) | Ok(NodeOutcome::LoopContinue) => {
                // Par has no loop semantics — sentinel observed but
                // treated as completion-with-no-output.
            }
            Ok(NodeOutcome::Return { value }) => {
                // First Return wins (deterministic by branch order
                // for documentation purposes; in practice
                // first-to-complete depends on .await schedule).
                if return_value.is_none() {
                    return_value = Some(value);
                }
            }
            Err(e) => return Err(e),
        }
    }

    // Advance the parent's counter to the post-join max.
    ctx.step_counter = max_step_counter;

    if let Some(value) = return_value {
        Ok(NodeOutcome::Return { value })
    } else {
        Ok(NodeOutcome::Completed {
            output: aggregate_output_parts.join("\n"),
            tokens_emitted: aggregate_tokens,
            step_index: entry_step_index,
        })
    }
}

/// Walk a branch body via recursive dispatch — moved out of
/// `orchestration::dispatch_body` because it's not crate-public
/// there. Mirrors that helper's discipline byte-for-byte:
/// per-node branch_path push/pop, sentinel propagation, last-output
/// capture.
async fn dispatch_branch_body(
    body: &[IRFlowNode],
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    let mut last_output = String::new();
    let mut total_tokens: u64 = 0;
    let entry_step_index = ctx.step_counter;

    for (i, child) in body.iter().enumerate() {
        if ctx.cancel.is_cancelled() {
            return Err(DispatchError::UpstreamCancelled);
        }
        ctx.branch_path.push(format!("step[{i}]"));
        let outcome = Box::pin(crate::flow_dispatcher::dispatch_node(child, ctx)).await;
        ctx.branch_path.pop();
        match outcome? {
            // §Fase 119.d — suspending inside a nested construct is refused.
            NodeOutcome::Hibernated { .. } => { return Err(DispatchError::BackendError { name: "hibernate".to_string(), message: "hibernate inside a nested construct (par branch) has no defined \n                 continuation shape yet; place it at top-level flow position"
                .to_string() }); }
            NodeOutcome::Completed {
                output,
                tokens_emitted,
                ..
            } => {
                if !output.is_empty() {
                    last_output = output;
                }
                total_tokens += tokens_emitted;
            }
            NodeOutcome::Break => return Ok(NodeOutcome::Break),
            NodeOutcome::LoopContinue => return Ok(NodeOutcome::LoopContinue),
            NodeOutcome::Return { value } => {
                return Ok(NodeOutcome::Return { value });
            }
        }
    }

    Ok(NodeOutcome::Completed {
        output: last_output,
        tokens_emitted: total_tokens,
        step_index: entry_step_index,
    })
}

// ────────────────────────────────────────────────────────────────────
//  Unit tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel_token::CancellationFlag;
    use crate::ir_nodes::*;
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

    fn let_branch(target: &str, value: &str) -> Vec<IRFlowNode> {
        vec![IRFlowNode::Let(IRLetBinding {
            node_type: "let",
            source_line: 0,
            source_column: 0,
            target: target.into(),
            value: value.into(),
            value_kind: "literal".into(),
            value_ast: None,
        })]
    }

    #[tokio::test]
    async fn run_par_emits_canonical_wire_shape() {
        let (mut ctx, mut rx) = fresh_ctx();
        let par = IRParallelBlock {
            node_type: "par",
            source_line: 0,
            source_column: 0,
            branches: Vec::new(),
        };
        let outcome = run_par(&par, &mut ctx).await.unwrap();
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
        // Wire events: StepStart + StepComplete (no StepToken).
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        assert_eq!(events.len(), 2);
        match &events[0] {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "par");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
        match &events[1] {
            FlowExecutionEvent::StepComplete { tokens_output, .. } => {
                assert_eq!(*tokens_output, 0);
            }
            e => panic!("expected StepComplete, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn run_par_cancel_short_circuits() {
        let cancel = CancellationFlag::new();
        cancel.cancel();
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);
        let par = IRParallelBlock {
            node_type: "par",
            source_line: 0,
            source_column: 0,
            branches: Vec::new(),
        };
        assert!(matches!(
            run_par(&par, &mut ctx).await,
            Err(DispatchError::UpstreamCancelled)
        ));
    }

    /// §Fase 65 — `run_par` now runs the IR's branches CONCURRENTLY (real
    /// effect), not a stub. Two branches → both execute, their outputs merge,
    /// and the `par` wrapper stays on the wire (StepComplete carries the merge).
    #[tokio::test]
    async fn run_par_with_branches_runs_them_concurrently() {
        let (mut ctx, mut rx) = fresh_ctx();
        let par = IRParallelBlock {
            node_type: "par",
            source_line: 0,
            source_column: 0,
            branches: vec![let_branch("a", "A-value"), let_branch("b", "B-value")],
        };
        let outcome = run_par(&par, &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Completed { output, .. } => {
                assert!(
                    output.contains("A-value") && output.contains("B-value"),
                    "both branches ran + merged: {output:?}"
                );
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        assert!(
            events.iter().any(|e| matches!(
                e,
                FlowExecutionEvent::StepStart { step_type, .. } if step_type == "par"
            )),
            "the `par` wrapper stays on the wire"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                FlowExecutionEvent::StepComplete { full_output, .. } if full_output.contains("A-value")
            )),
            "StepComplete carries the merged branch output"
        );
    }

    /// §Fase 65 (Multiplexed SSE) — the concurrent `par` branches emit their
    /// step events INTERLEAVED on the one stream, each carrying its `branch_path`
    /// (`"par[0]"` / `"par[1]"`) so the SSE consumer DEMUXES the concurrent
    /// streams instead of guessing by arrival order. This is the honest
    /// out-of-order emission (the founder ruling) made usable.
    #[tokio::test]
    async fn par_branch_events_carry_the_multiplex_key() {
        use crate::ir_nodes::IRStep;
        let mk_step = |name: &str| {
            vec![IRFlowNode::Step(IRStep {
                node_type: "step",
                source_line: 0,
                source_column: 0,
                name: name.into(),
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
                guards: Vec::new(),
                requires_context: None,                now_tz: None,                body: Vec::new(),
            })]
        };
        let (mut ctx, mut rx) = fresh_ctx();
        let par = IRParallelBlock {
            node_type: "par",
            source_line: 0,
            source_column: 0,
            branches: vec![mk_step("A"), mk_step("B")],
        };
        run_par(&par, &mut ctx).await.unwrap();

        let mut keys = std::collections::HashSet::new();
        while let Ok(ev) = rx.try_recv() {
            if let FlowExecutionEvent::StepStart { branch_path, .. } = ev {
                if !branch_path.is_empty() {
                    keys.insert(branch_path);
                }
            }
        }
        // Each branch's events are keyed by its `par[<idx>]` path (the nested
        // `.step[0]` segment is the position WITHIN the branch body). The
        // `par[0]` vs `par[1]` prefix is what the consumer demuxes on.
        assert!(
            keys.iter().any(|k| k.starts_with("par[0]")),
            "branch 0 carries its demux key: {keys:?}"
        );
        assert!(
            keys.iter().any(|k| k.starts_with("par[1]")),
            "branch 1 carries its demux key: {keys:?}"
        );
    }

    // ── run_branches_concurrently ─────────────────────────────────────

    #[tokio::test]
    async fn run_branches_concurrently_two_let_branches() {
        let (mut ctx, _rx) = fresh_ctx();
        let branches = vec![let_branch("a", "A-value"), let_branch("b", "B-value")];

        let outcome = run_branches_concurrently(&branches, &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Completed {
                output,
                tokens_emitted,
                ..
            } => {
                // Each Let branch's "output" is the bound value.
                // Aggregate is "\n"-joined non-empty parts.
                assert!(
                    output.contains("A-value") && output.contains("B-value"),
                    "expected both branch outputs aggregated, got {output:?}"
                );
                assert_eq!(tokens_emitted, 0);
            }
            other => panic!("expected Completed, got {other:?}"),
        }

        // Parent's let_bindings should NOT contain "a" or "b"
        // (branches are scoped — D10 parity).
        assert!(!ctx.let_bindings.contains_key("a"));
        assert!(!ctx.let_bindings.contains_key("b"));
    }

    #[tokio::test]
    async fn run_branches_concurrently_zero_branches_returns_completed() {
        let (mut ctx, _rx) = fresh_ctx();
        let outcome = run_branches_concurrently(&[], &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Completed {
                output,
                tokens_emitted,
                ..
            } => {
                assert_eq!(output, "");
                assert_eq!(tokens_emitted, 0);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_branches_concurrently_propagates_return_sentinel() {
        let (mut ctx, _rx) = fresh_ctx();
        let branches = vec![
            let_branch("a", "side"),
            vec![IRFlowNode::Return(IRReturnStep {
                node_type: "return",
                source_line: 0,
                source_column: 0,
                value_expr: "computed-from-branch-1".into(),
            })],
        ];
        let outcome = run_branches_concurrently(&branches, &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Return { value } => {
                assert_eq!(value, "computed-from-branch-1");
            }
            other => panic!("expected Return propagation, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_branches_concurrently_cancel_propagation() {
        let cancel = CancellationFlag::new();
        cancel.cancel();
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);
        let branches = vec![let_branch("a", "v")];
        assert!(matches!(
            run_branches_concurrently(&branches, &mut ctx).await,
            Err(DispatchError::UpstreamCancelled)
        ));
    }

    #[tokio::test]
    async fn run_branches_concurrently_step_counter_merges_max() {
        let (mut ctx, _rx) = fresh_ctx();
        // Branch A: 2 Steps (counter advances 2).
        // Branch B: 1 Step.
        // After join, parent's counter should be max = 2.
        let branches = vec![
            vec![
                IRFlowNode::Step(make_step("A1")),
                IRFlowNode::Step(make_step("A2")),
            ],
            vec![IRFlowNode::Step(make_step("B1"))],
        ];
        run_branches_concurrently(&branches, &mut ctx).await.unwrap();
        assert_eq!(
            ctx.step_counter, 2,
            "parent counter merges to max(2, 1) = 2 post-join"
        );
    }

    fn make_step(name: &str) -> IRStep {
        IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
            name: name.into(),
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
            guards: Vec::new(),
            requires_context: None,            now_tz: None,            body: Vec::new(),
        }
    }

    #[tokio::test]
    async fn run_branches_concurrently_branch_path_isolated_per_branch() {
        // Use a "tracer" pure-shape inside each branch + observe
        // its audit row's step_name reflects branch-path-aware
        // dispatch. After return, parent's branch_path is empty
        // (each branch's pop was applied to its own clone).
        let (mut ctx, _rx) = fresh_ctx();
        ctx.branch_path.push("outer".into());
        let branches = vec![
            vec![IRFlowNode::Step(make_step("InA"))],
            vec![IRFlowNode::Step(make_step("InB"))],
        ];
        run_branches_concurrently(&branches, &mut ctx).await.unwrap();
        // Parent retains its branch_path verbatim.
        assert_eq!(ctx.branch_path, vec!["outer".to_string()]);
        // Audit rows recorded by inner Step handlers — 2 rows.
        let audit = ctx.step_audit_records.lock().await;
        assert_eq!(audit.len(), 2);
    }
}
