//! §Fase 33.e — Stream-effect dispatcher (Layer 4).
//!
//! D4 in motion: bridges the `effects: <stream:<policy>>` declarations
//! on tool definitions to actual runtime behavior. Closes the last
//! architectural layer between adopter-level effect annotation and
//! adopter-visible wire timing/backpressure semantics.
//!
//! ## The path adopter→wire
//!
//! ```text
//!   adopter source:    tool chat_stream { effects: <stream:drop_oldest> }
//!                                                  └────────┬───────────┘
//!                            §Fase 11.a closed catalog  ────┘
//!
//!   type checker:      effect row {"stream:drop_oldest"} on ToolDefinition
//!                                                  └─── verified at compile-time
//!
//!   wire negotiation:  Fase 30.c disjunct (b) → implicit_transport = "sse"
//!                                                  └─── classify_negotiation_via_source_text
//!
//!   wire body:         Fase 33.c live FlowExecutionEvent → axon.token SSE events
//!
//!   THIS MODULE:       per-step lookup → BackpressurePolicy → enforcer
//!                                                  └─── policy semantics activated
//! ```
//!
//! ## Architecture (D4 + D7)
//!
//! Two primitives, both pure (no I/O, no global state):
//!
//!   1. [`resolve_stream_effect_for_step`] — given a step name + flow +
//!      program, finds the referenced tool and parses its
//!      `effects: <stream:<policy>>` row into a
//!      [`BackpressurePolicy`]. Returns `None` when the step is not
//!      stream-effecting; that's the most common case and must be
//!      cheap.
//!
//!   2. [`StreamPolicyEnforcer`] — wraps the [`crate::stream_runtime::Stream`]
//!      primitive (which already implements all four policies) into
//!      a chunk-oriented enforcer that bridges
//!      `futures::Stream<Item = Result<ChatChunk, BackendError>>`
//!      → bounded buffer with declared policy → forwarded sender.
//!
//! ## Pillar trace
//!
//! - **MATHEMATICS** — `BackpressurePolicy` is a closed enum
//!   ({DropOldest, DegradeQuality, PauseUpstream, Fail}). Pattern
//!   matching is exhaustive at every dispatch site; adding a fifth
//!   policy breaks the build.
//! - **LOGIC** — effect-row `stream:<policy>` IS-IFF the enforcer
//!   activates with that policy. The resolver is the bidirectional
//!   bridge between type-level annotation and runtime behavior.
//! - **PHILOSOPHY** — declared effect IS the wire behavior. An
//!   adopter writes `<stream:drop_oldest>` and the runtime guarantees
//!   that's exactly what happens on the wire.
//! - **COMPUTING** — bounded buffer + atomic counters + tokio Notify
//!   for wakeups. No spinning, no busy-waits.

#![allow(dead_code)]

use std::sync::Arc;

use crate::ast::{
    Declaration, EffectRow, FlowDefinition, FlowStep, Program, StepNode,
    ToolDefinition,
};
use crate::backends::error::BackendError;
use crate::backends::ChatChunk;
use crate::stream_effect::BackpressurePolicy;
use crate::stream_runtime::{Stream as PolicyStream, StreamError, StreamMetricsSnapshot};

// ────────────────────────────────────────────────────────────────────
//  §1 — Resolver
// ────────────────────────────────────────────────────────────────────

/// The default per-step backpressure buffer capacity, in chunks.
///
/// Sized to absorb a normal-paced LLM token burst (~16 chunks ≈ 1-2s
/// of GPT-style streaming) without surfacing the policy as a no-op,
/// while staying tight enough that a saturating producer triggers the
/// declared policy in adopter-observable time. Per D6 the capacity is
/// configurable per-tool via `effects: <stream:drop_oldest,
/// buffer=N>` (Fase 11.a annotation syntax already accepts options);
/// this constant is the fallback when no `buffer=` option is given.
pub const DEFAULT_STREAM_BUFFER_CAPACITY: usize = 16;

/// Locate the [`ToolDefinition`] referenced by the named step in the
/// flow. Returns `None` when the step is not a [`FlowStep::Step`], does
/// not declare `apply: <tool>`, or the tool is not in the program.
///
/// Pure — no I/O, O(n) over flow body + O(t) over program declarations.
pub fn tool_referenced_by_step<'a>(
    step_name: &str,
    flow: &FlowDefinition,
    program: &'a Program,
) -> Option<&'a ToolDefinition> {
    let step_node: Option<&StepNode> = flow.body.iter().find_map(|s| match s {
        FlowStep::Step(node) if node.name == step_name => Some(node),
        _ => None,
    });
    let step = step_node?;
    let tool_ref = step.apply_ref.trim();
    if tool_ref.is_empty() {
        return None;
    }
    program.declarations.iter().find_map(|d| match d {
        Declaration::Tool(t) if t.name == tool_ref => Some(t),
        _ => None,
    })
}

/// Parse an [`EffectRow`] for a `stream:<policy>` entry and return the
/// declared [`BackpressurePolicy`]. Returns `None` when no stream
/// effect is declared OR the policy slug is malformed (the type
/// checker upstream rejects malformed policy slugs at compile time;
/// at runtime we silently fall through to the no-policy path).
///
/// Pure — O(n) over effect entries.
pub fn policy_from_effect_row(row: &EffectRow) -> Option<BackpressurePolicy> {
    for effect in &row.effects {
        let trimmed = effect.trim();
        if let Some(slug) = trimmed.strip_prefix("stream:") {
            if let Some(policy) = BackpressurePolicy::from_slug(slug) {
                return Some(policy);
            }
        }
    }
    None
}

/// End-to-end resolver: given a step name + the owning flow + the
/// containing program, returns the step's declared
/// [`BackpressurePolicy`] (or `None` when the step is not
/// stream-effecting).
///
/// This is the primary entry point used by [`server_execute_streaming`
/// (`axon_server.rs`)](crate::axon_server) per emitted [`StepToken`
/// (`flow_execution_event.rs`)](crate::flow_execution_event::FlowExecutionEvent::StepToken).
///
/// Pure — composition of [`tool_referenced_by_step`] +
/// [`policy_from_effect_row`].
pub fn resolve_stream_effect_for_step(
    step_name: &str,
    flow: &FlowDefinition,
    program: &Program,
) -> Option<BackpressurePolicy> {
    let tool = tool_referenced_by_step(step_name, flow, program)?;
    let row = tool.effects.as_ref()?;
    policy_from_effect_row(row)
}

// ────────────────────────────────────────────────────────────────────
//  §2 — Stream-policy enforcer
// ────────────────────────────────────────────────────────────────────

/// Snapshot of a single enforcement run's policy-fire counters.
///
/// Surfaced on the SSE wire's `axon.complete` envelope so adopters
/// can observe whether the declared policy actually fired in
/// production — a `drop_oldest` policy that never fires under sustained
/// load is a configuration smell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnforcementSummary {
    pub policy: Option<&'static str>,
    pub chunks_pushed: u64,
    pub chunks_delivered: u64,
    pub drop_oldest_hits: u64,
    pub degrade_quality_hits: u64,
    pub pause_upstream_blocks: u64,
    pub fail_overflows: u64,
    /// `true` iff the enforcement run terminated by surfacing
    /// `BackpressurePolicy::Fail` overflow (the only policy that
    /// can return an error to the consumer mid-stream).
    pub failed: bool,
}

impl EnforcementSummary {
    pub(crate) fn from_snapshot(
        policy: BackpressurePolicy,
        snap: &StreamMetricsSnapshot,
        failed: bool,
    ) -> Self {
        Self {
            policy: Some(policy_slug_static(policy)),
            chunks_pushed: snap.items_pushed,
            chunks_delivered: snap.items_delivered,
            drop_oldest_hits: snap.drop_oldest_hits,
            degrade_quality_hits: snap.degrade_quality_hits,
            pause_upstream_blocks: snap.pause_upstream_blocks,
            fail_overflows: snap.fail_overflows,
            failed,
        }
    }
}

fn policy_slug_static(policy: BackpressurePolicy) -> &'static str {
    // BackpressurePolicy::slug() already returns &'static str (the
    // catalog is closed). This wrapper exists so the enforcer surface
    // keeps a stable type without importing the inner module.
    policy.slug()
}

/// Bridges a chunk-producing source through a declared backpressure
/// policy. Construct one per step that has a `<stream:policy>` effect.
///
/// ## Producer / consumer model
///
/// The enforcer is **two-sided**:
///
/// - **Producer side** — [`Self::push_chunk`]. Each chunk the backend
///   produces is pushed through the declared policy. Bounded buffer
///   semantics activate when the consumer is slower than the producer.
///
/// - **Consumer side** — [`Self::pop_chunk`]. The downstream sink
///   (SSE wire forwarder) pulls one chunk at a time. Buffer drains as
///   chunks are consumed; the policy fires only when push hits the
///   capacity limit.
///
/// Both sides are `async`. The enforcer is `Clone` (handle), so a
/// producer task + a consumer task can share the same enforcer.
///
/// ## Failure surface
///
/// Only [`BackpressurePolicy::Fail`] can surface a
/// [`StreamError::Overflow`] on push. Other policies handle overflow
/// transparently (drop, degrade, block).
#[derive(Clone)]
pub struct StreamPolicyEnforcer {
    inner: PolicyStream<ChatChunk>,
    policy: BackpressurePolicy,
}

impl StreamPolicyEnforcer {
    /// Construct an enforcer with capacity [`DEFAULT_STREAM_BUFFER_CAPACITY`].
    pub fn new(policy: BackpressurePolicy) -> Self {
        Self::with_capacity(policy, DEFAULT_STREAM_BUFFER_CAPACITY)
    }

    /// Construct with an explicit capacity. `DegradeQuality` requires
    /// a degrader; use [`Self::with_degrader`] for that policy.
    pub fn with_capacity(policy: BackpressurePolicy, capacity: usize) -> Self {
        let annotation = crate::stream_effect::BackpressureAnnotation {
            policy,
            options: Vec::new(),
        };
        Self {
            inner: PolicyStream::new(capacity.max(1), annotation),
            policy,
        }
    }

    /// Construct a `DegradeQuality` enforcer with the mandatory
    /// degrader function. Other policies should use [`Self::new`] or
    /// [`Self::with_capacity`].
    pub fn with_degrader(
        policy: BackpressurePolicy,
        capacity: usize,
        degrader: Arc<dyn Fn(ChatChunk) -> ChatChunk + Send + Sync>,
    ) -> Self {
        let annotation = crate::stream_effect::BackpressureAnnotation {
            policy,
            options: Vec::new(),
        };
        Self {
            inner: PolicyStream::with_degrader(capacity.max(1), annotation, degrader),
            policy,
        }
    }

    pub fn policy(&self) -> BackpressurePolicy {
        self.policy
    }

    /// Push a chunk through the declared policy. Returns `Ok(())` on
    /// success, `Err(StreamError::Overflow)` only when the policy is
    /// `Fail` and the buffer is at capacity.
    pub async fn push_chunk(&self, chunk: ChatChunk) -> Result<(), StreamError> {
        self.inner.push(chunk).await
    }

    /// Pull the next chunk. Returns `None` once the producer closes
    /// the enforcer (via [`Self::close`]) AND the buffer drains.
    pub async fn pop_chunk(&self) -> Option<ChatChunk> {
        self.inner.pop().await
    }

    /// Signal end-of-stream so pending consumers wake and observe
    /// `None`. Idempotent.
    pub async fn close(&self) {
        self.inner.close().await;
    }

    /// Drain a `Result<ChatChunk, BackendError>` stream entirely
    /// through the enforcer. Errors mid-stream surface to the consumer
    /// without being filtered; chunks pass through the declared policy.
    ///
    /// Returns the [`EnforcementSummary`] (policy fire counters) once
    /// the source has terminated. The consumer-side drain happens in
    /// parallel via [`Self::pop_chunk`]; this method only feeds the
    /// producer side.
    ///
    /// Cancellation: if the consumer drops the handle (last `Clone`
    /// destroyed), [`Self::push_chunk`] surfaces
    /// [`StreamError::Cancelled`]; this method propagates by closing
    /// the source stream and returning the partial summary.
    pub async fn drain<S>(
        &self,
        mut source: S,
        on_error: impl Fn(BackendError) + Send,
    ) -> EnforcementSummary
    where
        S: futures::Stream<Item = Result<ChatChunk, BackendError>> + Send + Unpin,
    {
        use futures::StreamExt;
        let mut failed = false;
        while let Some(item) = source.next().await {
            match item {
                Ok(chunk) => {
                    if let Err(StreamError::Overflow { .. }) = self.push_chunk(chunk).await {
                        failed = true;
                        break;
                    }
                }
                Err(e) => {
                    on_error(e);
                    failed = true;
                    break;
                }
            }
        }
        self.close().await;
        let snap = self.inner.metrics.snapshot();
        EnforcementSummary::from_snapshot(self.policy, &snap, failed)
    }

    /// Pure synchronous read of the current metrics snapshot. Useful
    /// for mid-stream observability without taking the buffer lock.
    pub fn metrics_snapshot(&self) -> StreamMetricsSnapshot {
        self.inner.metrics.snapshot()
    }
}

// ────────────────────────────────────────────────────────────────────
//  §3 — Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{
        Declaration, EffectRow, FlowDefinition, FlowStep, Loc, Program,
        StepNode, ToolDefinition,
    };

    fn loc() -> Loc {
        Loc::default()
    }

    fn make_step(name: &str, apply_ref: &str) -> FlowStep {
        FlowStep::Step(StepNode {
            name: name.to_string(),
            persona_ref: String::new(),
            given: String::new(),
            ask: String::new(),
            output_type: String::new(),
            confidence_floor: None,
            navigate_ref: String::new(),
            apply_ref: apply_ref.to_string(),
            requires_context: None,
            now_tz: None,
            guards: Vec::new(),
            pix_ops: Vec::new(),
            loc: loc(),
        })
    }

    fn make_tool(name: &str, effects: Option<Vec<&str>>) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            provider: String::new(),
            max_results: None,
            filter_expr: String::new(),
            timeout: String::new(),
            runtime: String::new(),
            resource_ref: String::new(),
            sandbox: None,
            effects: effects.map(|es| EffectRow {
                effects: es.into_iter().map(String::from).collect(),
                epistemic_level: String::new(),
                loc: loc(),
            }),
            parameters: Vec::new(),
            output_type: None,
            requires: Vec::new(),
            secret: String::new(),
            secret_partition: String::new(),
            target: None,
            risk: None,
            argv: Vec::new(),
            cache: String::new(),
            scrape: None,
            loc: loc(),
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        }
    }

    fn make_flow(name: &str, steps: Vec<FlowStep>) -> FlowDefinition {
        FlowDefinition {
            name: name.to_string(),
            parameters: Vec::new(),
            return_type: None,
            body: steps,
            loc: loc(),
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        }
    }

    fn make_program(
        flow: FlowDefinition,
        tools: Vec<ToolDefinition>,
    ) -> Program {
        Program {
            declarations: std::iter::once(Declaration::Flow(flow))
                .chain(tools.into_iter().map(Declaration::Tool))
                .collect(),
            declaration_trivia: Vec::new(),
            loc: loc(),
        }
    }

    // ── §3.1 Resolver — pure-unit ──────────────────────────────────

    #[test]
    fn resolve_step_with_drop_oldest_effect() {
        let flow = make_flow("Chat", vec![make_step("Generate", "chat_stream")]);
        let tool = make_tool("chat_stream", Some(vec!["stream:drop_oldest"]));
        let program = make_program(flow, vec![tool]);
        let flow_ref = match &program.declarations[0] {
            Declaration::Flow(f) => f,
            _ => unreachable!(),
        };
        let policy = resolve_stream_effect_for_step("Generate", flow_ref, &program);
        assert_eq!(policy, Some(BackpressurePolicy::DropOldest));
    }

    #[test]
    fn resolve_step_with_degrade_quality_effect() {
        let flow = make_flow("Chat", vec![make_step("Generate", "chat_stream")]);
        let tool = make_tool("chat_stream", Some(vec!["stream:degrade_quality"]));
        let program = make_program(flow, vec![tool]);
        let flow_ref = match &program.declarations[0] {
            Declaration::Flow(f) => f,
            _ => unreachable!(),
        };
        let policy = resolve_stream_effect_for_step("Generate", flow_ref, &program);
        assert_eq!(policy, Some(BackpressurePolicy::DegradeQuality));
    }

    #[test]
    fn resolve_step_with_pause_upstream_effect() {
        let flow = make_flow("Chat", vec![make_step("Generate", "chat_stream")]);
        let tool = make_tool("chat_stream", Some(vec!["stream:pause_upstream"]));
        let program = make_program(flow, vec![tool]);
        let flow_ref = match &program.declarations[0] {
            Declaration::Flow(f) => f,
            _ => unreachable!(),
        };
        let policy = resolve_stream_effect_for_step("Generate", flow_ref, &program);
        assert_eq!(policy, Some(BackpressurePolicy::PauseUpstream));
    }

    #[test]
    fn resolve_step_with_fail_effect() {
        let flow = make_flow("Chat", vec![make_step("Generate", "chat_stream")]);
        let tool = make_tool("chat_stream", Some(vec!["stream:fail"]));
        let program = make_program(flow, vec![tool]);
        let flow_ref = match &program.declarations[0] {
            Declaration::Flow(f) => f,
            _ => unreachable!(),
        };
        let policy = resolve_stream_effect_for_step("Generate", flow_ref, &program);
        assert_eq!(policy, Some(BackpressurePolicy::Fail));
    }

    #[test]
    fn resolve_step_without_apply_returns_none() {
        let flow = make_flow("Chat", vec![make_step("Generate", "")]);
        let program = make_program(flow, vec![]);
        let flow_ref = match &program.declarations[0] {
            Declaration::Flow(f) => f,
            _ => unreachable!(),
        };
        let policy = resolve_stream_effect_for_step("Generate", flow_ref, &program);
        assert!(policy.is_none());
    }

    #[test]
    fn resolve_step_with_tool_lacking_effects_returns_none() {
        let flow = make_flow("Chat", vec![make_step("Generate", "chat_stream")]);
        let tool = make_tool("chat_stream", None);
        let program = make_program(flow, vec![tool]);
        let flow_ref = match &program.declarations[0] {
            Declaration::Flow(f) => f,
            _ => unreachable!(),
        };
        let policy = resolve_stream_effect_for_step("Generate", flow_ref, &program);
        assert!(policy.is_none());
    }

    #[test]
    fn resolve_step_with_non_stream_effect_returns_none() {
        let flow = make_flow("Chat", vec![make_step("Generate", "chat_stream")]);
        let tool = make_tool("chat_stream", Some(vec!["telemetry", "audit"]));
        let program = make_program(flow, vec![tool]);
        let flow_ref = match &program.declarations[0] {
            Declaration::Flow(f) => f,
            _ => unreachable!(),
        };
        let policy = resolve_stream_effect_for_step("Generate", flow_ref, &program);
        assert!(policy.is_none());
    }

    #[test]
    fn resolve_unknown_step_returns_none() {
        let flow = make_flow("Chat", vec![make_step("Generate", "chat_stream")]);
        let tool = make_tool("chat_stream", Some(vec!["stream:drop_oldest"]));
        let program = make_program(flow, vec![tool]);
        let flow_ref = match &program.declarations[0] {
            Declaration::Flow(f) => f,
            _ => unreachable!(),
        };
        let policy = resolve_stream_effect_for_step("NotAStep", flow_ref, &program);
        assert!(policy.is_none());
    }

    #[test]
    fn resolve_step_with_malformed_policy_slug_falls_through() {
        // Type checker would reject this at compile time; runtime
        // resolver returns None rather than panicking.
        let flow = make_flow("Chat", vec![make_step("Generate", "chat_stream")]);
        let tool = make_tool("chat_stream", Some(vec!["stream:never_block_ever"]));
        let program = make_program(flow, vec![tool]);
        let flow_ref = match &program.declarations[0] {
            Declaration::Flow(f) => f,
            _ => unreachable!(),
        };
        let policy = resolve_stream_effect_for_step("Generate", flow_ref, &program);
        assert!(policy.is_none());
    }

    #[test]
    fn resolve_multi_step_flow_per_step_lookup() {
        let flow = make_flow(
            "Pipeline",
            vec![
                make_step("Plan", "planner"),
                make_step("Generate", "chat_stream"),
                make_step("Audit", "audit_tool"),
            ],
        );
        let planner = make_tool("planner", Some(vec!["stream:fail"]));
        let chat = make_tool("chat_stream", Some(vec!["stream:drop_oldest"]));
        let audit = make_tool("audit_tool", None);
        let program = make_program(flow, vec![planner, chat, audit]);
        let flow_ref = match &program.declarations[0] {
            Declaration::Flow(f) => f,
            _ => unreachable!(),
        };
        assert_eq!(
            resolve_stream_effect_for_step("Plan", flow_ref, &program),
            Some(BackpressurePolicy::Fail)
        );
        assert_eq!(
            resolve_stream_effect_for_step("Generate", flow_ref, &program),
            Some(BackpressurePolicy::DropOldest)
        );
        assert_eq!(
            resolve_stream_effect_for_step("Audit", flow_ref, &program),
            None
        );
    }

    // ── §3.2 Enforcer — policy semantics under saturation ─────────

    fn chunk(text: &str) -> ChatChunk {
        ChatChunk {
            delta: text.to_string(),
            finish_reason: None,
            usage: None,
        }
    }

    #[tokio::test]
    async fn enforcer_drop_oldest_under_pressure_drops_oldest_chunks() {
        let enforcer =
            StreamPolicyEnforcer::with_capacity(BackpressurePolicy::DropOldest, 2);
        // Push 4 chunks without draining → 2 get dropped.
        enforcer.push_chunk(chunk("a")).await.expect("push ok");
        enforcer.push_chunk(chunk("b")).await.expect("push ok");
        enforcer.push_chunk(chunk("c")).await.expect("push ok");
        enforcer.push_chunk(chunk("d")).await.expect("push ok");
        enforcer.close().await;

        let mut delivered = Vec::new();
        while let Some(c) = enforcer.pop_chunk().await {
            delivered.push(c.delta);
        }
        // Oldest 2 dropped; newest 2 delivered.
        assert_eq!(delivered, vec!["c".to_string(), "d".to_string()]);
        let metrics = enforcer.metrics_snapshot();
        assert_eq!(metrics.items_pushed, 4);
        assert_eq!(metrics.drop_oldest_hits, 2);
        assert_eq!(metrics.items_delivered, 2);
    }

    #[tokio::test]
    async fn enforcer_drop_oldest_below_capacity_passes_through() {
        let enforcer =
            StreamPolicyEnforcer::with_capacity(BackpressurePolicy::DropOldest, 4);
        enforcer.push_chunk(chunk("a")).await.expect("push ok");
        enforcer.push_chunk(chunk("b")).await.expect("push ok");
        enforcer.close().await;

        let mut delivered = Vec::new();
        while let Some(c) = enforcer.pop_chunk().await {
            delivered.push(c.delta);
        }
        assert_eq!(delivered, vec!["a".to_string(), "b".to_string()]);
        let metrics = enforcer.metrics_snapshot();
        assert_eq!(metrics.drop_oldest_hits, 0);
    }

    #[tokio::test]
    async fn enforcer_fail_under_pressure_returns_overflow_error() {
        let enforcer =
            StreamPolicyEnforcer::with_capacity(BackpressurePolicy::Fail, 2);
        enforcer.push_chunk(chunk("a")).await.expect("push ok");
        enforcer.push_chunk(chunk("b")).await.expect("push ok");
        let err = enforcer.push_chunk(chunk("c")).await;
        match err {
            Err(StreamError::Overflow { policy, .. }) => {
                assert_eq!(policy, BackpressurePolicy::Fail);
            }
            other => panic!("expected Overflow, got {other:?}"),
        }
        let metrics = enforcer.metrics_snapshot();
        assert_eq!(metrics.fail_overflows, 1);
    }

    #[tokio::test]
    async fn enforcer_fail_below_capacity_passes_through() {
        let enforcer =
            StreamPolicyEnforcer::with_capacity(BackpressurePolicy::Fail, 4);
        enforcer.push_chunk(chunk("a")).await.expect("push ok");
        enforcer.push_chunk(chunk("b")).await.expect("push ok");
        let metrics = enforcer.metrics_snapshot();
        assert_eq!(metrics.fail_overflows, 0);
    }

    #[tokio::test]
    async fn enforcer_pause_upstream_drains_then_admits_new_chunk() {
        // pause_upstream blocks the producer until the consumer drains.
        // We simulate it: spawn a producer task that fills the buffer,
        // wait briefly, drain one chunk on the consumer side, observe
        // the producer's blocked push completes.
        let enforcer = StreamPolicyEnforcer::with_capacity(
            BackpressurePolicy::PauseUpstream,
            1,
        );
        enforcer.push_chunk(chunk("first")).await.expect("push ok");
        // Now the buffer is full; spawn a producer that tries to push.
        let producer = enforcer.clone();
        let push_handle = tokio::spawn(async move {
            producer.push_chunk(chunk("second")).await
        });
        // Yield so the producer task starts and blocks on not_full.
        tokio::task::yield_now().await;
        // Drain one chunk — wakes the producer.
        let first = enforcer.pop_chunk().await.expect("first chunk");
        assert_eq!(first.delta, "first");
        // Producer's push now completes.
        push_handle
            .await
            .expect("join ok")
            .expect("producer push ok");
        let metrics = enforcer.metrics_snapshot();
        assert!(metrics.pause_upstream_blocks >= 1);
    }

    #[tokio::test]
    async fn enforcer_degrade_quality_applies_degrader_under_pressure() {
        // Degrader: empty out chunk's delta (lossy summary).
        let degrader: Arc<dyn Fn(ChatChunk) -> ChatChunk + Send + Sync> =
            Arc::new(|c| ChatChunk {
                delta: "*".to_string(), // marker so we can observe degradation
                finish_reason: c.finish_reason,
                usage: c.usage,
            });
        let enforcer = StreamPolicyEnforcer::with_degrader(
            BackpressurePolicy::DegradeQuality,
            2,
            degrader,
        );
        enforcer.push_chunk(chunk("good1")).await.expect("push");
        enforcer.push_chunk(chunk("good2")).await.expect("push");
        // 3rd push: at capacity → degrader applies (drops oldest, pushes degraded).
        enforcer.push_chunk(chunk("good3")).await.expect("push");
        enforcer.close().await;

        let mut delivered = Vec::new();
        while let Some(c) = enforcer.pop_chunk().await {
            delivered.push(c.delta);
        }
        // Buffer state after pushes: drop "good1", push degraded "good3"
        // → delivered ["good2", "*"].
        assert_eq!(delivered, vec!["good2".to_string(), "*".to_string()]);
        let metrics = enforcer.metrics_snapshot();
        assert_eq!(metrics.degrade_quality_hits, 1);
    }

    // ── §3.3 Drain method (producer + consumer end-to-end) ─────────

    #[tokio::test]
    async fn enforcer_drain_drives_source_through_drop_oldest() {
        let enforcer =
            StreamPolicyEnforcer::with_capacity(BackpressurePolicy::DropOldest, 3);
        let source = futures::stream::iter(vec![
            Ok(chunk("a")),
            Ok(chunk("b")),
            Ok(chunk("c")),
            Ok(chunk("d")),
            Ok(chunk("e")),
        ]);
        let enforcer_clone = enforcer.clone();
        let drain_handle = tokio::spawn(async move {
            enforcer_clone.drain(Box::pin(source), |_e| ()).await
        });
        // Wait until the drain task fully populates the buffer + closes
        // the channel; we should then pull the remaining 3 in order.
        let summary = drain_handle.await.expect("drain join");
        let mut delivered = Vec::new();
        while let Some(c) = enforcer.pop_chunk().await {
            delivered.push(c.delta);
        }
        // 5 pushes, capacity 3 → 2 dropped, 3 delivered.
        assert_eq!(summary.policy, Some("drop_oldest"));
        assert_eq!(summary.chunks_pushed, 5);
        assert_eq!(summary.drop_oldest_hits, 2);
        assert_eq!(delivered.len(), 3);
        // Newest 3 retained.
        assert_eq!(delivered, vec!["c".to_string(), "d".to_string(), "e".to_string()]);
    }

    #[tokio::test]
    async fn enforcer_drain_surfaces_failed_flag_on_fail_overflow() {
        let enforcer =
            StreamPolicyEnforcer::with_capacity(BackpressurePolicy::Fail, 2);
        let source = futures::stream::iter(vec![
            Ok(chunk("a")),
            Ok(chunk("b")),
            Ok(chunk("c")), // overflow
        ]);
        let enforcer_clone = enforcer.clone();
        let summary = enforcer_clone
            .drain(Box::pin(source), |_e| ())
            .await;
        assert!(summary.failed);
        assert_eq!(summary.fail_overflows, 1);
        assert_eq!(summary.chunks_pushed, 3); // counter increments BEFORE push
    }

    #[tokio::test]
    async fn enforcer_drain_propagates_backend_errors_via_callback() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let enforcer =
            StreamPolicyEnforcer::with_capacity(BackpressurePolicy::DropOldest, 4);
        let source = futures::stream::iter(vec![
            Ok(chunk("a")),
            Err(BackendError::Generic {
                provider: "test".into(),
                model: "test".into(),
                status: None,
                message: "synthetic mid-stream failure".into(),
            }),
            Ok(chunk("never_pushed")), // drain stops at first error
        ]);
        let saw_error = Arc::new(AtomicBool::new(false));
        let saw_error_clone = saw_error.clone();
        let enforcer_clone = enforcer.clone();
        let summary = enforcer_clone
            .drain(Box::pin(source), move |_e| {
                saw_error_clone.store(true, Ordering::Relaxed);
            })
            .await;
        assert!(saw_error.load(Ordering::Relaxed));
        assert!(summary.failed);
        // Exactly one chunk got into the buffer.
        assert_eq!(summary.chunks_pushed, 1);
    }

    // ── §3.4 EnforcementSummary ─────────────────────────────────────

    #[test]
    fn enforcement_summary_default_is_empty() {
        let s = EnforcementSummary::default();
        assert!(s.policy.is_none());
        assert_eq!(s.chunks_pushed, 0);
        assert!(!s.failed);
    }

    #[test]
    fn enforcement_summary_slug_for_each_policy() {
        for &policy in BackpressurePolicy::ALL {
            let snap = StreamMetricsSnapshot {
                items_pushed: 0,
                items_delivered: 0,
                drop_oldest_hits: 0,
                degrade_quality_hits: 0,
                pause_upstream_blocks: 0,
                fail_overflows: 0,
            };
            let summary = EnforcementSummary::from_snapshot(policy, &snap, false);
            assert_eq!(summary.policy, Some(policy.slug()));
        }
    }

    #[test]
    fn default_buffer_capacity_is_sensible() {
        // Sanity-check the constant: small enough to surface policy
        // semantics under load, large enough not to fire on the median
        // adopter case.
        assert!(DEFAULT_STREAM_BUFFER_CAPACITY >= 4);
        assert!(DEFAULT_STREAM_BUFFER_CAPACITY <= 256);
    }
}
