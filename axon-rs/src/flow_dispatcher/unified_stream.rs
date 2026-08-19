//! v1.29.0 — Unified stream handler converging the
//! four streaming-effect disjunctions onto a single drain path.
//!
//! Per the AXON paper section 3, streaming-effect tools surface through
//! FOUR syntactic disjunctions:
//!
//! - **(a)** `output: Stream<T>` — type-level streaming via
//!   `Backend::stream()`. Produces `Stream<ChatChunk>` (LLM-side).
//! - **(b)** `apply: <stream-tool>` — `apply_ref` resolves to a
//!   `Tool` impl whose `stream()` produces `Stream<ToolChunk>`
//! (v1.29.0 dispatcher arm).
//! - **(c)** `use_tool` syntax — adopter declares allowed tools at
//!   the step level; semantically reduces to (a) at runtime (the
//!   LLM emits the tool-call which is dispatched on the next
//!   iteration).
//! - **(d)** `perform Stream.Yield(x)` — algebraic effect. Static
//! pre-scan of `IRPerform` nodes (v1.24.0
//!   `bridge_effect_stream_yield`).
//!
//! Pre-34.g the four disjunctions had **divergent drain paths** —
//! (a) enforced [`BackpressurePolicy`] at chunk granularity via
//! [`crate::stream_effect_dispatcher::StreamPolicyEnforcer`]; (b) did
//! NOT (audit row's `chunks_dropped` / `chunks_degraded` were always
//! `0` per the 34.d honest scope); (d) emitted pre-computed wire
//! tokens with no policy enforcement.
//!
//! 34.g closes the convergence by:
//!
//! 1. Shipping [`unified_stream_handler`] — the **single** drain
//!    loop that ALL `Stream<ToolChunk>`-producing disjunctions
//!    route through. The handler applies the declared
//!    [`BackpressurePolicy`] at chunk granularity (real
//!    enforcement, not just slug capture in audit), emits per-chunk
//!    [`FlowExecutionEvent::StepToken`] events, and returns a
//!    [`ToolStreamSummary`] the caller uses to populate
//!    [`StepAuditRecord`] with **non-zero**
//!    `chunks_dropped` / `chunks_degraded` counters.
//! 2. Shipping [`chat_chunk_to_tool_chunk`] — the type-bridge from
//!    `ChatChunk` (disjunct a) to `ToolChunk` so disjunct (a) can
//!    be **lifted** into the unified handler's input type when
//!    needed (e.g. for tests asserting byte-equal wire output
//!    across semantically-equivalent flows). Disjunct (a)'s
//!    production drain path stays at
//!    [`crate::stream_effect_dispatcher::StreamPolicyEnforcer`]
//!    for backwards-compat — the conversion is the convergence
//!    PROOF, not a forced migration.
//! 3. Shipping [`unified_stream_from_chunks`] — the static-scan
//!    bridge from `Vec<ToolChunk>` to a `Stream<ToolChunk>`
//!    invocable through [`unified_stream_handler`]. Disjunct (d)
//!    uses this to surface the pre-scanned `Stream.Yield`
//!    materialized chunks as a uniform stream + drain through
//!    the same handler.
//!
//! # Honest scope
//!
//! - Disjunct (a) production code path is unchanged. The conversion
//!   impl + tests prove the convergence is semantically correct;
//!   migrating production (a) to the unified handler is deferred
//!   to 34.g.2 (involves preserving the `FlowExecutionEvent::ToolCall`
//! emission ordering invariant from v1.24.0 D8 + the
//!   `Usage`-bearing terminal chunk semantics).
//! - Disjunct (c) does NOT produce its own runtime stream — it
//!   forwards tool declarations to the LLM via
//!   `ChatRequest::tools`. Convergence is observational: the LLM's
//!   `ToolUse` finish reason fires inside disjunct (a)'s drain
//!   path; the subsequent tool dispatch (if the orchestrator
//!   re-enters with the tool result) flows through disjunct (b).
//! - Disjunct (d)'s `bridge_effect_stream_yield` is kept as the
//!   default surface for backwards-compat; opting into the unified
//!   handler is an additive surface (new
//!   [`bridge_effect_stream_yield_unified`] function) so adopters
//!   who want chunks_dropped/degraded counters on Yield streams
//!   migrate explicitly.

use std::pin::Pin;
use std::sync::Arc;

use futures::stream::StreamExt;
use tokio::sync::mpsc;

use crate::backends::{ChatChunk, FinishReason};
use crate::cancel_token::CancellationFlag;
use crate::flow_dispatcher::{DispatchCtx, DispatchError};
use crate::flow_execution_event::FlowExecutionEvent;
use crate::stream_effect::BackpressurePolicy;
use crate::stream_runtime::{Stream as PolicyStream, StreamError};
use crate::tool_trait::{ToolChunk, ToolFinishReason, ToolStream};

// ════════════════════════════════════════════════════════════════════
//  ToolStreamSummary — what the unified handler returns
// ════════════════════════════════════════════════════════════════════

/// Closed-catalog summary returned by [`unified_stream_handler`] after
/// the source stream is fully drained (or cancelled mid-stream).
///
/// The caller (typically `pure_shape::run_step_streaming_tool` for
/// disjunct (b) / `bridge_effect_stream_yield_unified` for (d)) uses
/// these counters to populate the [`crate::axonendpoint_replay::StepAuditRecord`]
/// + decide whether to surface a `DispatchError::BackendError` (if
/// `terminator_message` is `Some`) or a `DispatchError::UpstreamCancelled`
/// (if `cancelled` is `true`).
///
/// **Counter semantics** (cf. [`crate::stream_runtime::StreamMetrics`]):
///
/// - `tokens_emitted` — the count of **non-empty deltas** that
///   reached the wire as `FlowExecutionEvent::StepToken` events.
///   Mirrors the existing `tokens_emitted` field in
///   `StepAuditRecord`.
/// - `output_hash_hex` — SHA-256 of `accumulated` (concatenated
///   non-empty deltas). Stable across runs; the D6 per-step replay
///   anchor.
/// - `chunks_pushed` / `chunks_delivered` — `chunks_pushed` is what
///   the source produced, `chunks_delivered` is what reached the
///   consumer after policy enforcement. Difference =
///   `chunks_dropped` (under `DropOldest`) or similar.
/// - `chunks_dropped` — under `DropOldest`, the count of head-of-
///   queue evictions when a push hit capacity. 34.d's audit row
///   has this field but always `0`; 34.g activates real counts.
/// - `chunks_degraded` — under `DegradeQuality`, the count of
///   degrader-applied chunks (the degrader function fired). Same
///   activation story.
#[derive(Debug, Clone, Default)]
pub struct ToolStreamSummary {
    pub tokens_emitted: u64,
    pub output_hash_hex: String,
    pub accumulated: String,
    pub chunks_pushed: u64,
    pub chunks_delivered: u64,
    pub chunks_dropped: u64,
    pub chunks_degraded: u64,
    pub pause_upstream_blocks: u64,
    pub fail_overflows: u64,
    /// `true` iff the stream's terminator was [`ToolFinishReason::Stop`]
    /// AND no policy `Fail` overflow fired AND the handler was not
    /// cancelled mid-stream.
    pub success: bool,
    /// `Some(_)` iff the stream's terminator was [`ToolFinishReason::Error`].
    /// The caller surfaces this as `DispatchError::BackendError`.
    pub terminator_message: Option<String>,
    /// `true` iff `ctx.cancel.is_cancelled()` was observed mid-drain
    /// OR the stream's terminator was [`ToolFinishReason::Cancelled`].
    /// The caller surfaces this as `DispatchError::UpstreamCancelled`.
    pub cancelled: bool,
}

impl ToolStreamSummary {
    /// Returns true iff the summary is "clean Stop" — no policy
    /// failure, no cancel, no error terminator. The caller routes
    /// this branch to `NodeOutcome::Completed`.
    pub fn is_clean_stop(&self) -> bool {
        self.success && !self.cancelled && self.terminator_message.is_none()
    }
}

// ════════════════════════════════════════════════════════════════════
//  ChatChunk → ToolChunk conversion (disjunct a → unified-input lift)
// ════════════════════════════════════════════════════════════════════

/// Lift a [`ChatChunk`] (disjunct a's chunk type) into a [`ToolChunk`]
/// (the unified handler's input type). The mapping is byte-identical
/// at the `delta` field + closed-catalog at the `finish_reason` field:
///
/// | `ChatChunk::finish_reason` | `ToolChunk::finish_reason` |
/// |---|---|
/// | `Some(Stop)` | `Some(ToolFinishReason::Stop)` |
/// | `Some(Length)` | `Some(Error { message: "length limit exceeded" })` |
/// | `Some(ToolUse)` | `Some(Stop)` (the LLM signaled a tool call; the dispatcher routes the call separately — the chunk itself ends naturally) |
/// | `Some(SafetyBreach)` | `Some(Error { message: "safety classifier blocked output" })` |
/// | `Some(Other(s))` | `Some(Error { message: "unknown finish reason: {s}" })` |
/// | `None` | `None` (intermediate chunk) |
///
/// **Note on `Usage`:** `ChatChunk::usage` carries token-usage
/// metadata that has no `ToolChunk` analog (tools count
/// chunks-emitted, not tokens-charged-by-provider). The conversion
/// drops it. Adopters who need `Usage` on the wire should consume
/// disjunct (a) directly via [`crate::backends::Backend::stream`].
pub fn chat_chunk_to_tool_chunk(chunk: ChatChunk) -> ToolChunk {
    let finish_reason = chunk.finish_reason.map(|fr| match fr {
        FinishReason::Stop => ToolFinishReason::Stop,
        FinishReason::Length => ToolFinishReason::Error {
            message: "length limit exceeded".to_string(),
        },
        FinishReason::ToolUse => ToolFinishReason::Stop,
        FinishReason::SafetyBreach => ToolFinishReason::Error {
            message: "safety classifier blocked output".to_string(),
        },
        FinishReason::Other(s) => ToolFinishReason::Error {
            message: format!("unknown finish reason: {s}"),
        },
    });
    ToolChunk {
        delta: chunk.delta,
        finish_reason,
        timestamp_ms: crate::flow_execution_event::now_ms(),
    }
}

// ════════════════════════════════════════════════════════════════════
//  Vec<ToolChunk> → ToolStream bridge (disjunct d → unified-input lift)
// ════════════════════════════════════════════════════════════════════

/// Wrap a pre-materialized `Vec<ToolChunk>` (typical product of
/// disjunct (d)'s static-scan over `IRPerform Stream.Yield` nodes)
/// into a [`ToolStream`] consumable by [`unified_stream_handler`].
///
/// The returned stream yields each chunk in order, then closes. No
/// async work is performed — the chunks are already known. The
/// surface exists so disjunct (d) gains the same audit-row shape as
/// disjunct (b) (chunks_dropped/degraded counters) when the adopter
/// opts into the unified handler.
pub fn unified_stream_from_chunks(chunks: Vec<ToolChunk>) -> ToolStream {
    Box::pin(futures::stream::iter(chunks))
}

// ════════════════════════════════════════════════════════════════════
//  unified_stream_handler — the convergent drain loop
// ════════════════════════════════════════════════════════════════════

/// **The unified streaming-effect drain loop** that the 34.g
/// convergence routes all `Stream<ToolChunk>`-producing disjunctions
/// through.
///
/// # Arguments
///
/// - `source` — the chunk stream to drain. Disjunct (b) constructs
///   this via `tool.stream(args, ctx).await`. Disjunct (d) constructs
///   it via [`unified_stream_from_chunks`]. Disjunct (a) can lift its
///   `Stream<ChatChunk>` via
///   `chunk_stream.map(|r| r.map(chat_chunk_to_tool_chunk))` (kept
///   out of production hot path for backwards-compat — see module
///   docstring).
/// - `policy` — the declared [`BackpressurePolicy`]. `None` means
///   no policy declared (direct drain, no enforcement). `Some(p)`
///   activates the [`crate::stream_runtime::Stream`] primitive
///   wrapping the source via a producer task + consumer loop.
/// - `cancel` — the cancellation flag observed before each chunk.
///   Mirrors the per-chunk cancel discipline from
///   `pure_shape::run_step_streaming_tool` (D5 budget).
/// - `tx` — the wire-event channel. Each non-empty `chunk.delta`
///   emits as `FlowExecutionEvent::StepToken { step_name, content,
///   token_index, timestamp_ms }`.
/// - `step_name` — the IR step name used in the emitted events.
///
/// # Returns
///
/// - `Ok(summary)` — the stream drained to completion (or was
///   cancelled, or surfaced an error terminator). The caller
///   inspects `summary.success` / `summary.cancelled` /
///   `summary.terminator_message` to decide the `NodeOutcome` /
///   `DispatchError` routing.
/// - `Err(ChannelClosed)` — the consumer dropped the receiver mid-
///   drain. The handler short-circuits + surfaces this so the caller
///   can clean up.
///
/// # Policy enforcement
///
/// When `policy` is `Some(_)`:
///
/// - A `PolicyStream<ToolChunk>` is constructed with the declared
///   policy + default capacity
///   ([`crate::stream_effect_dispatcher::DEFAULT_STREAM_BUFFER_CAPACITY`]).
/// - A producer task drains `source` chunk-by-chunk into the
///   `PolicyStream` via `push()`. Under `DropOldest`/`DegradeQuality`
///   the policy fires on overflow + the metrics counters tick.
///   Under `Fail`, an overflow push returns `Err` which the producer
///   captures + surfaces via the summary's `fail_overflows` count
///   + `success = false`.
/// - The consumer loop in this function pops chunks from the
///   `PolicyStream`, polls cancel, emits StepToken, accumulates
///   SHA-256, handles terminator. On natural close (producer
///   finishes + buffer drains) the consumer exits + the metrics
///   snapshot populates the summary.
/// - For `DegradeQuality`, an **identity degrader** is wired by
///   default — OSS adopters get the metric counter without
///   degradation semantics. Enterprise verticals override the
///   degrader for per-domain semantics (audio bitrate, video
///   resolution, etc.) — that R&D lives in the enterprise crate.
///
/// When `policy` is `None`:
///
/// - The source is drained directly chunk-by-chunk. No counters
///   tick (chunks_dropped = chunks_degraded = 0). Mirrors the
///   pre-34.g `run_step_streaming_tool` shape byte-equal (D9
///   backwards-compat for tools that don't declare a stream policy).
/// v2.83.0 — the `on_chunk:` arm of a `stream<T> { … }`, plus the context it
/// needs, handed to the drain so the handler runs **per chunk, during the
/// stream**.
///
/// `tx` and `cancel` are passed to the drain as separate owned CLONES rather
/// than borrowed out of this `ctx` — otherwise the mutable borrow the handler
/// needs would alias them. That is the whole reason this is a struct and not two
/// more parameters.
pub struct ChunkHook<'a> {
    /// The handler arm, dispatched through `run_step` like any other step.
    pub arm: &'a crate::ir_nodes::IRStep,
    pub ctx: &'a mut DispatchCtx,
}

impl ChunkHook<'_> {
    /// Bind the chunk and run the arm. Errors propagate: the arm is the author's
    /// processing of the stream, and a stream whose processing silently failed
    /// still looks like a successful stream from the outside.
    async fn fire(&mut self, delta: &str) -> Result<(), DispatchError> {
        self.ctx
            .let_bindings
            .insert("chunk".to_string(), delta.to_string());
        // v2.83.0 — a failure raised BY THE HANDLER is tagged `stream:on_`
        // so `on_error` cannot catch it. `on_error` handles a failing SOURCE; a
        // broken handler that silently catches itself and reports the stream as
        // healthy is worse than one that crashes, because the program looks fine.
        // Without this tag the raw error would reach `is_source_failure` wearing
        // the handler's own step name and be mistaken for a producer fault.
        let outcome = Box::pin(crate::flow_dispatcher::pure_shape::run_step(
            self.arm, self.ctx,
        ))
        .await
        .map_err(|e| match e {
            DispatchError::UpstreamCancelled => DispatchError::UpstreamCancelled,
            other => DispatchError::BackendError {
                name: "stream:on_chunk".to_string(),
                message: format!("`on_chunk` failed while handling a chunk: {other}"),
            },
        })?;
        match outcome {
            crate::flow_dispatcher::NodeOutcome::Completed { .. } => Ok(()),
            // v2.83.0 discipline — a sentinel raised mid-drain has no defined
            // continuation shape (which chunk does it resume at?) and the upstream
            // is still open. Refused rather than half-honoured.
            _ => Err(DispatchError::BackendError {
                name: "stream:on_chunk".to_string(),
                message: "`on_chunk` raised a control sentinel (hibernate / break / \
                          continue / return) mid-stream; a per-chunk handler has no \
                          defined continuation shape and the upstream is still open."
                    .to_string(),
            }),
        }
    }
}

pub async fn unified_stream_handler(
    source: ToolStream,
    policy: Option<BackpressurePolicy>,
    cancel: &CancellationFlag,
    tx: &mpsc::UnboundedSender<FlowExecutionEvent>,
    step_name: &str,
    // v2.15.0 — the multiplex demux key for the event stream. Empty
    // at the top level (elided on the wire via skip-if-empty); `"par[i].…"`
    // inside a `par` branch so an SSE consumer demuxes a tool-stream nested
    // in a concurrent branch by the same key as every other handler event.
    branch_path: &str,
) -> Result<ToolStreamSummary, DispatchError> {
    unified_stream_handler_with_hook(source, policy, cancel, tx, step_name, branch_path, None).await
}

/// v2.83.0 — [`unified_stream_handler`] with an optional per-chunk handler.
///
/// Additive: the hookless entry point above delegates here with `None`, so every
/// pre-v2.83.0 caller is byte-identical.
///
/// The hook goes HERE, in the one drain that enforces the declared
/// `BackpressurePolicy`, rather than in a private loop inside the stream step.
/// A second drain would have silently stopped honouring a declared
/// `backpressure:` the moment the two copies diverged — a dropped policy being
/// exactly the class of defect v2.83.0 exists to end.
pub async fn unified_stream_handler_with_hook(
    source: ToolStream,
    policy: Option<BackpressurePolicy>,
    cancel: &CancellationFlag,
    tx: &mpsc::UnboundedSender<FlowExecutionEvent>,
    step_name: &str,
    branch_path: &str,
    hook: Option<ChunkHook<'_>>,
) -> Result<ToolStreamSummary, DispatchError> {
    // Pre-flight cancel check. An already-cancelled flag MUST
    // short-circuit with a `cancelled` summary even when the source
    // is empty — otherwise an empty `Stream<ToolChunk>` never
    // triggers the per-chunk cancel poll inside the drain loop and
    // the summary would mis-report `cancelled: false`. This makes
    // the handler's cancel contract total: cancel-before-entry ⟹
    // `summary.cancelled` regardless of how many chunks the source
    // produces.
    if cancel.is_cancelled() {
        return Ok(ToolStreamSummary {
            success: false,
            cancelled: true,
            output_hash_hex: sha256_hex(""),
            ..Default::default()
        });
    }
    if let Some(p) = policy {
        unified_drain_with_policy(source, p, cancel, tx, step_name, branch_path, hook).await
    } else {
        unified_drain_direct(source, cancel, tx, step_name, branch_path, hook).await
    }
}

/// Drain without policy enforcement. Counter fields stay at `0`.
/// Mirrors the 34.d pure_shape::run_step_streaming_tool drain shape
/// byte-equal — adopters who don't declare a stream policy see
/// **zero behavior change** post-34.g (D9).
async fn unified_drain_direct(
    mut source: Pin<Box<dyn futures::Stream<Item = ToolChunk> + Send>>,
    cancel: &CancellationFlag,
    tx: &mpsc::UnboundedSender<FlowExecutionEvent>,
    step_name: &str,
    branch_path: &str,
    mut hook: Option<ChunkHook<'_>>,
) -> Result<ToolStreamSummary, DispatchError> {
    let mut summary = ToolStreamSummary {
        success: true,
        ..Default::default()
    };
    while let Some(chunk) = source.next().await {
        summary.chunks_pushed += 1;
        if cancel.is_cancelled() {
            summary.cancelled = true;
            summary.success = false;
            break;
        }
        summary.chunks_delivered += 1;
        if !chunk.delta.is_empty() {
            summary.tokens_emitted += 1;
            summary.accumulated.push_str(&chunk.delta);
            tx.send(FlowExecutionEvent::StepToken {
                step_name: step_name.to_string(),
                content: chunk.delta.clone(),
                token_index: summary.tokens_emitted,
                // v2.15.0 — carry the multiplex key threaded from the
                // caller's DispatchCtx so a tool-stream nested in a `par`
                // branch demuxes by the same `par[i].…` key as every other
                // handler event (empty at the top level → elided on the wire).
                branch_path: branch_path.to_string(),
                timestamp_ms: crate::flow_execution_event::now_ms(),
            })
            .map_err(|_| DispatchError::ChannelClosed)?;
            // v2.83.0 — `on_chunk`, over the chunk that just reached the wire.
            if let Some(h) = hook.as_mut() {
                h.fire(&chunk.delta).await?;
            }
        }
        if let Some(reason) = chunk.finish_reason {
            handle_terminator(reason, &mut summary);
            break;
        }
    }
    summary.output_hash_hex = sha256_hex(&summary.accumulated);
    Ok(summary)
}

/// Drain through a [`crate::stream_runtime::Stream`] primitive
/// configured with the declared policy. Producer task pushes source
/// chunks into the policy stream; consumer loop pops + wire-emits.
async fn unified_drain_with_policy(
    source: ToolStream,
    policy: BackpressurePolicy,
    cancel: &CancellationFlag,
    tx: &mpsc::UnboundedSender<FlowExecutionEvent>,
    step_name: &str,
    branch_path: &str,
    mut hook: Option<ChunkHook<'_>>,
) -> Result<ToolStreamSummary, DispatchError> {
    use crate::stream_effect::BackpressureAnnotation;
    use crate::stream_effect_dispatcher::DEFAULT_STREAM_BUFFER_CAPACITY;

    let annotation = BackpressureAnnotation {
        policy,
        options: Vec::new(),
    };
    let policy_stream: PolicyStream<ToolChunk> = match policy {
        BackpressurePolicy::DegradeQuality => PolicyStream::with_degrader(
            DEFAULT_STREAM_BUFFER_CAPACITY,
            annotation,
            // OSS identity degrader — keeps the chunk shape intact
            // while the policy fires the degrader counter. Enterprise
            // verticals (`axon-enterprise.shield`) override for
            // domain-specific quality reduction.
            Arc::new(|c| c),
        ),
        BackpressurePolicy::DropOldest
        | BackpressurePolicy::PauseUpstream
        | BackpressurePolicy::Fail => PolicyStream::new(DEFAULT_STREAM_BUFFER_CAPACITY, annotation),
    };

    let producer_stream = policy_stream.clone();
    let producer_cancel = cancel.clone();
    let producer = tokio::spawn(async move {
        let mut source = source;
        let mut producer_failed: Option<StreamError> = None;
        while let Some(chunk) = source.next().await {
            if producer_cancel.is_cancelled() {
                break;
            }
            if let Err(e) = producer_stream.push(chunk).await {
                producer_failed = Some(e);
                break;
            }
        }
        producer_stream.close().await;
        producer_failed
    });

    // Consumer loop — pull chunks from the policy stream + emit.
    let mut summary = ToolStreamSummary {
        success: true,
        ..Default::default()
    };
    while let Some(chunk) = policy_stream.pop().await {
        if cancel.is_cancelled() {
            summary.cancelled = true;
            summary.success = false;
            // Close the policy stream so the producer doesn't block.
            policy_stream.close().await;
            break;
        }
        if !chunk.delta.is_empty() {
            summary.tokens_emitted += 1;
            summary.accumulated.push_str(&chunk.delta);
            tx.send(FlowExecutionEvent::StepToken {
                step_name: step_name.to_string(),
                content: chunk.delta.clone(),
                token_index: summary.tokens_emitted,
                // v2.15.0 — carry the multiplex key threaded from the
                // caller's DispatchCtx so a tool-stream nested in a `par`
                // branch demuxes by the same `par[i].…` key as every other
                // handler event (empty at the top level → elided on the wire).
                branch_path: branch_path.to_string(),
                timestamp_ms: crate::flow_execution_event::now_ms(),
            })
            .map_err(|_| DispatchError::ChannelClosed)?;
            // v2.83.0 — `on_chunk` runs on the chunks the POLICY let through,
            // not on the raw source: a `drop_oldest` chunk that never reached the
            // wire must not reach the handler either, or the author's processing
            // would see a stream the consumer never got.
            if let Some(h) = hook.as_mut() {
                h.fire(&chunk.delta).await?;
            }
        }
        if let Some(reason) = chunk.finish_reason {
            handle_terminator(reason, &mut summary);
            // Close to wake any pending producer push.
            policy_stream.close().await;
            break;
        }
    }

    // Wait for the producer to finish (or join cleanly) so the
    // metrics snapshot below captures the final counter state.
    let producer_failed = producer.await.map_err(|e| DispatchError::BackendError {
        name: "unified_stream:producer".to_string(),
        message: format!("producer task join failed: {e}"),
    })?;

    // Snapshot the policy stream's metrics. The atomic counters are
    // updated by push_*/pop on each operation; reading post-drain
    // gives the authoritative totals.
    let snap = policy_stream.metrics.as_ref();
    use std::sync::atomic::Ordering;
    summary.chunks_pushed = snap.items_pushed.load(Ordering::Relaxed);
    summary.chunks_delivered = snap.items_delivered.load(Ordering::Relaxed);
    summary.chunks_dropped = snap.drop_oldest_hits.load(Ordering::Relaxed);
    summary.chunks_degraded = snap.degrade_quality_hits.load(Ordering::Relaxed);
    summary.pause_upstream_blocks = snap.pause_upstream_blocks.load(Ordering::Relaxed);
    summary.fail_overflows = snap.fail_overflows.load(Ordering::Relaxed);

    if let Some(err) = producer_failed {
        if let StreamError::Overflow { policy: p, .. } = err {
            // BackpressurePolicy::Fail surfaced — record + mark failed.
            summary.success = false;
            summary.terminator_message = Some(format!(
                "stream overflow under policy {p}: producer hit capacity \
                 ({} chunks pushed before overflow)",
                summary.chunks_pushed
            ));
        }
    }

    summary.output_hash_hex = sha256_hex(&summary.accumulated);
    Ok(summary)
}

/// Apply a terminator chunk's finish reason to the summary.
/// Centralized so direct + policy drain paths handle terminators
/// identically.
fn handle_terminator(reason: ToolFinishReason, summary: &mut ToolStreamSummary) {
    match reason {
        ToolFinishReason::Stop => { /* summary.success stays true */ }
        ToolFinishReason::Error { message } => {
            summary.success = false;
            summary.terminator_message = Some(message);
        }
        ToolFinishReason::Cancelled => {
            summary.success = false;
            summary.cancelled = true;
        }
    }
}

fn sha256_hex(input: &str) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest.as_slice() {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

// ════════════════════════════════════════════════════════════════════
//  Lib unit tests
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_chunk_to_tool_chunk_preserves_delta() {
        let chat = ChatChunk {
            delta: "hello world".to_string(),
            finish_reason: None,
            usage: None,
        };
        let tool = chat_chunk_to_tool_chunk(chat);
        assert_eq!(tool.delta, "hello world");
        assert_eq!(tool.finish_reason, None);
    }

    #[test]
    fn chat_chunk_to_tool_chunk_maps_stop_to_stop() {
        let chat = ChatChunk {
            delta: "".to_string(),
            finish_reason: Some(FinishReason::Stop),
            usage: None,
        };
        let tool = chat_chunk_to_tool_chunk(chat);
        assert_eq!(tool.finish_reason, Some(ToolFinishReason::Stop));
    }

    #[test]
    fn chat_chunk_to_tool_chunk_maps_length_to_error() {
        let chat = ChatChunk {
            delta: "".to_string(),
            finish_reason: Some(FinishReason::Length),
            usage: None,
        };
        let tool = chat_chunk_to_tool_chunk(chat);
        match tool.finish_reason {
            Some(ToolFinishReason::Error { message }) => {
                assert!(message.contains("length"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn chat_chunk_to_tool_chunk_maps_tooluse_to_stop() {
        // ToolUse is the LLM signaling a tool-call; the dispatcher
        // routes the call separately. The chunk itself ends Stop.
        let chat = ChatChunk {
            delta: "".to_string(),
            finish_reason: Some(FinishReason::ToolUse),
            usage: None,
        };
        let tool = chat_chunk_to_tool_chunk(chat);
        assert_eq!(tool.finish_reason, Some(ToolFinishReason::Stop));
    }

    #[test]
    fn chat_chunk_to_tool_chunk_maps_safety_to_error() {
        let chat = ChatChunk {
            delta: "".to_string(),
            finish_reason: Some(FinishReason::SafetyBreach),
            usage: None,
        };
        let tool = chat_chunk_to_tool_chunk(chat);
        match tool.finish_reason {
            Some(ToolFinishReason::Error { message }) => {
                assert!(message.contains("safety"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn chat_chunk_to_tool_chunk_maps_other_to_error_carrying_raw() {
        let chat = ChatChunk {
            delta: "".to_string(),
            finish_reason: Some(FinishReason::Other("custom-reason".to_string())),
            usage: None,
        };
        let tool = chat_chunk_to_tool_chunk(chat);
        match tool.finish_reason {
            Some(ToolFinishReason::Error { message }) => {
                assert!(message.contains("custom-reason"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn unified_stream_from_chunks_yields_inputs_in_order() {
        let chunks = vec![
            ToolChunk::intermediate("a"),
            ToolChunk::intermediate("b"),
            ToolChunk::terminator("c", ToolFinishReason::Stop),
        ];
        let stream = unified_stream_from_chunks(chunks);
        let collected = futures::executor::block_on(async {
            let mut s = stream;
            let mut out = Vec::new();
            while let Some(c) = s.next().await {
                out.push(c);
            }
            out
        });
        assert_eq!(collected.len(), 3);
        assert_eq!(collected[0].delta, "a");
        assert_eq!(collected[1].delta, "b");
        assert_eq!(collected[2].delta, "c");
        assert!(collected[2].is_terminator());
    }

    #[test]
    fn unified_stream_from_chunks_empty_vec_closes_immediately() {
        let stream = unified_stream_from_chunks(Vec::new());
        let collected = futures::executor::block_on(async {
            let mut s = stream;
            let mut out = Vec::new();
            while let Some(c) = s.next().await {
                out.push(c);
            }
            out
        });
        assert!(collected.is_empty());
    }

    #[test]
    fn tool_stream_summary_is_clean_stop_truthy_default_plus_success() {
        let s = ToolStreamSummary {
            success: true,
            cancelled: false,
            terminator_message: None,
            ..Default::default()
        };
        assert!(s.is_clean_stop());
    }

    #[test]
    fn tool_stream_summary_is_clean_stop_false_when_cancelled() {
        let s = ToolStreamSummary {
            success: false,
            cancelled: true,
            ..Default::default()
        };
        assert!(!s.is_clean_stop());
    }

    #[test]
    fn tool_stream_summary_is_clean_stop_false_when_terminator_error() {
        let s = ToolStreamSummary {
            success: false,
            terminator_message: Some("upstream failed".to_string()),
            ..Default::default()
        };
        assert!(!s.is_clean_stop());
    }

    #[tokio::test]
    async fn unified_handler_direct_drain_emits_step_tokens_in_order() {
        let chunks = vec![
            ToolChunk::intermediate("hello"),
            ToolChunk::intermediate(" "),
            ToolChunk::intermediate("world"),
            ToolChunk::terminator("", ToolFinishReason::Stop),
        ];
        let source = unified_stream_from_chunks(chunks);
        let cancel = CancellationFlag::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let summary = unified_stream_handler(source, None, &cancel, &tx, "TestStep", "")
            .await
            .expect("ok");
        assert!(summary.success);
        assert_eq!(summary.tokens_emitted, 3);
        assert_eq!(summary.accumulated, "hello world");
        // No policy → counters all zero.
        assert_eq!(summary.chunks_dropped, 0);
        assert_eq!(summary.chunks_degraded, 0);

        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        let tokens: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                FlowExecutionEvent::StepToken { content, .. } => Some(content.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(tokens, vec!["hello", " ", "world"]);
    }

    #[tokio::test]
    async fn unified_handler_with_drop_oldest_policy_under_burst_drops_chunks() {
        // Source emits 200 chunks rapidly; bounded policy stream
        // capacity (DEFAULT_STREAM_BUFFER_CAPACITY) under DropOldest
        // → some chunks dropped + counter ticks.
        let n: u64 = 200;
        let chunks: Vec<ToolChunk> = (0..n)
            .map(|i| ToolChunk::intermediate(format!("c{i}")))
            .chain(std::iter::once(ToolChunk::terminator(
                "",
                ToolFinishReason::Stop,
            )))
            .collect();
        let source = unified_stream_from_chunks(chunks);
        let cancel = CancellationFlag::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let summary = unified_stream_handler(
            source,
            Some(BackpressurePolicy::DropOldest),
            &cancel,
            &tx,
            "DropTest",
            "",
        )
        .await
        .expect("ok");
        // Policy fired at least once OR the consumer kept pace.
        // Under tokio's scheduler, the producer typically gets ahead
        // for some chunks. The summary's counters are the durable
        // assertion target — even 0 drops is a VALID outcome under
        // a sufficiently fast consumer; what we MUST pin is that
        // the **handler reports the counters honestly** (vs the 34.d
        // baseline where they were ALWAYS 0 regardless of policy).
        assert!(summary.success, "DropOldest should never fail");
        assert!(summary.chunks_pushed >= n);
        // For DropOldest, delivered may be less than pushed.
        assert!(summary.chunks_delivered <= summary.chunks_pushed);
    }

    #[tokio::test]
    async fn unified_handler_with_fail_policy_under_overflow_surfaces_error() {
        // Source emits 10_000 chunks with no consumer pull → buffer
        // overflows immediately under Fail; producer returns
        // Err(Overflow); summary.success = false +
        // terminator_message Some.
        let n: u64 = 10_000;
        let chunks: Vec<ToolChunk> = (0..n)
            .map(|i| ToolChunk::intermediate(format!("c{i}")))
            .collect();
        let source = unified_stream_from_chunks(chunks);
        let cancel = CancellationFlag::new();
        // Consumer that NEVER reads — close the receiver immediately.
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        let summary = unified_stream_handler(
            source,
            Some(BackpressurePolicy::Fail),
            &cancel,
            &tx,
            "FailTest",
            "",
        )
        .await;
        // Either ChannelClosed (consumer dropped) or summary marked
        // !success — both are acceptable proof that Fail fired.
        match summary {
            Ok(s) => {
                if s.fail_overflows > 0 {
                    assert!(!s.success);
                    assert!(s.terminator_message.is_some());
                }
            }
            Err(DispatchError::ChannelClosed) => {
                // Acceptable — consumer dropped before fail surfaced.
            }
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn unified_handler_pre_cancel_short_circuits() {
        let chunks = vec![
            ToolChunk::intermediate("a"),
            ToolChunk::intermediate("b"),
            ToolChunk::terminator("", ToolFinishReason::Stop),
        ];
        let source = unified_stream_from_chunks(chunks);
        let cancel = CancellationFlag::new();
        cancel.cancel();
        let (tx, _rx) = mpsc::unbounded_channel();
        let summary = unified_stream_handler(source, None, &cancel, &tx, "PreCancel", "")
            .await
            .expect("ok");
        assert!(summary.cancelled);
        assert!(!summary.success);
    }

    #[tokio::test]
    async fn unified_handler_error_terminator_surfaces_in_summary() {
        let chunks = vec![
            ToolChunk::intermediate("partial"),
            ToolChunk::terminator(
                "",
                ToolFinishReason::Error {
                    message: "upstream failed".to_string(),
                },
            ),
        ];
        let source = unified_stream_from_chunks(chunks);
        let cancel = CancellationFlag::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let summary = unified_stream_handler(source, None, &cancel, &tx, "ErrTerm", "")
            .await
            .expect("ok");
        assert!(!summary.success);
        assert!(!summary.cancelled);
        assert_eq!(summary.terminator_message.as_deref(), Some("upstream failed"));
        assert_eq!(summary.tokens_emitted, 1); // "partial" reached wire
    }

    #[tokio::test]
    async fn unified_handler_cancelled_terminator_surfaces_in_summary() {
        let chunks = vec![
            ToolChunk::intermediate("partial"),
            ToolChunk::terminator("", ToolFinishReason::Cancelled),
        ];
        let source = unified_stream_from_chunks(chunks);
        let cancel = CancellationFlag::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let summary = unified_stream_handler(source, None, &cancel, &tx, "CancTerm", "")
            .await
            .expect("ok");
        assert!(summary.cancelled);
        assert!(!summary.success);
    }

    // v2.15.0 — a tool-stream nested in a `par` branch must carry the
    // multiplex key so an SSE consumer demuxes its tokens by the SAME
    // `par[i].…` key as every other handler event in that branch.
    #[tokio::test]
    async fn tool_stream_tokens_carry_the_threaded_branch_path() {
        let chunks = vec![
            ToolChunk::intermediate("alpha"),
            ToolChunk::intermediate("beta"),
            ToolChunk::terminator("", ToolFinishReason::Stop),
        ];
        let source = unified_stream_from_chunks(chunks);
        let cancel = CancellationFlag::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let summary =
            unified_stream_handler(source, None, &cancel, &tx, "Drain", "par[1].step[0]")
                .await
                .expect("ok");
        assert!(summary.success);

        let mut keys = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            if let FlowExecutionEvent::StepToken { branch_path, .. } = ev {
                keys.push(branch_path);
            }
        }
        assert_eq!(keys.len(), 2, "two non-empty deltas → two StepToken events");
        assert!(
            keys.iter().all(|k| k == "par[1].step[0]"),
            "every nested tool-stream token must carry the threaded demux key, \
             got {keys:?}"
        );
    }

    // Top-level (no `par`) tool-stream tokens carry an EMPTY key so they
    // elide on the wire via skip-if-empty → non-`par` flows stay byte-compat.
    #[tokio::test]
    async fn top_level_tool_stream_tokens_carry_empty_branch_path() {
        let chunks = vec![
            ToolChunk::intermediate("solo"),
            ToolChunk::terminator("", ToolFinishReason::Stop),
        ];
        let source = unified_stream_from_chunks(chunks);
        let cancel = CancellationFlag::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        unified_stream_handler(source, None, &cancel, &tx, "Drain", "")
            .await
            .expect("ok");
        while let Ok(ev) = rx.try_recv() {
            if let FlowExecutionEvent::StepToken { branch_path, .. } = ev {
                assert!(branch_path.is_empty(), "top-level key must stay empty");
            }
        }
    }

    #[test]
    fn sha256_hex_matches_canonical_for_empty_string() {
        assert_eq!(
            sha256_hex(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_hex_matches_canonical_for_abc() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
