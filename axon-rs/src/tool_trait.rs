//! §Fase 34.b (v1.29.0) — `Tool` trait + `ToolChunk` closed-catalog
//! struct.
//!
//! The structural foundation for tools-as-stream-producers. Three
//! public surfaces ship here:
//!
//! 1. [`ToolFinishReason`] — closed-catalog enum carried on the
//!    terminator chunk of a tool stream. Distinct from
//!    [`crate::backends::FinishReason`] (LLM-side); tools have a
//!    different vocabulary (no `Length` / `ToolUse` / `SafetyBreach`).
//! 2. [`ToolChunk`] — closed-catalog chunk struct. `delta` + optional
//!    `finish_reason` (elided when None per D4 byte-compat) +
//!    `timestamp_ms`. Cross-stack mirror in Python `axon.tools`.
//! 3. [`Tool`] — async trait with `execute()` + default `stream()` +
//!    default `is_streaming()`. Default `stream()` wraps `execute()`
//!    as a **single-chunk stream** (D9 backwards-compat: every
//!    existing non-streaming tool keeps working byte-equal). Default
//!    `is_streaming()` returns `false`.
//!
//! [`ToolContext`] carries the per-invocation cancellation flag
//! (D5 cancel-into-tool-body propagation) + the request-scoped
//! `trace_id` for audit correlation.
//!
//! # D-letter coverage
//!
//! - **D1** — `Tool` trait surface (`execute` + `stream` +
//!   `is_streaming`); default `stream()` wraps `execute()` as
//!   single-chunk for D9 backwards-compat.
//! - **D6** — `ToolChunk` shape supports per-chunk audit:
//!   `tool_chunks_emitted` counts non-empty deltas;
//!   `tool_output_hash_hex` is computed over the concatenated
//!   deltas. The audit-population path lands in Fase 34.i.
//! - **D9** — default `stream()` makes every existing tool
//!   automatically a single-chunk stream producer; adopters who
//!   haven't migrated see ZERO behavioral change in wire body.
//! - **D10** — cross-stack: Python `axon.tools.Tool` ABC + dataclass
//!   `ToolChunk` mirror this Rust surface byte-identically per
//!   `tests/test_fase34_b_tool_trait_cross_stack.py`.
//!
//! # Pillar trace
//!
//! - **MATHEMATICS** — `Tool::stream(args, ctx)` is the categorical
//!   morphism `Args → Stream<ToolChunk>` the paper §3-§6 defines.
//!   Pre-34 the runtime collapsed this to `Args → String`; post-34
//!   the categorical contract is honored.
//! - **LOGIC** — `ToolFinishReason` is a closed 3-variant catalog
//!   (`Stop` / `Error` / `Cancelled`). Adding a 4th requires a
//!   deliberate sub-fase + cross-stack drift gate update.
//! - **PHILOSOPHY** — adopters declare `effects: <stream:<policy>>`
//!   on a tool; post-34 (34.c+) the declaration becomes the
//!   structural `is_streaming` field on `ToolEntry` and the
//!   dispatcher honors it. The paper's promise is no longer a
//!   compile-time annotation — it's the runtime contract.
//! - **COMPUTING** — `ToolStream` is a `Pin<Box<dyn Stream + Send>>`
//!   so it can cross the spawn boundary of the producer task in
//!   the dispatcher's `pure_shape::run_step` (Fase 34.d).

use crate::cancel_token::CancellationFlag;
use crate::tool_executor::ToolResult;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

// ════════════════════════════════════════════════════════════════════
//  Type aliases
// ════════════════════════════════════════════════════════════════════

/// Boxed pinned tool stream. The trait method `Tool::stream()`
/// returns this type so the dispatcher can store the stream in a
/// uniform variable across all tool implementations.
pub type ToolStream = Pin<Box<dyn Stream<Item = ToolChunk> + Send + 'static>>;

// ════════════════════════════════════════════════════════════════════
//  ToolFinishReason — closed-catalog finish reason for tool streams
// ════════════════════════════════════════════════════════════════════

/// Closed-catalog finish reason carried on the **terminator chunk**
/// of a tool stream. Intermediate chunks have `finish_reason: None`.
///
/// Distinct from [`crate::backends::FinishReason`] (LLM-side
/// vocabulary). Tools have a different conversation model — no
/// max-tokens cliff, no tool-use cascade, no safety filter. The
/// three reachable states for a tool stream are:
///
/// - **`Stop`** — the tool's stream completed cleanly.
/// - **`Error { message }`** — the tool's body raised an error
///   mid-stream (HTTP failure, MCP server error, native panic
///   caught and wrapped, etc.).
/// - **`Cancelled`** — the per-tool-invocation cancellation flag
///   fired and the tool's stream aborted cooperatively (Fase 34.h
///   D5 cancel-into-tool-body discipline).
///
/// Serde tag is `kind`, snake_case payload — adopter SDKs +
/// downstream crates pattern-match cleanly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ToolFinishReason {
    /// Stream completed without error.
    Stop,
    /// Tool execution failed mid-stream. `message` carries
    /// adopter-facing diagnostic detail.
    Error {
        message: String,
    },
    /// Cancellation signal propagated into the tool body (D5).
    Cancelled,
}

impl ToolFinishReason {
    /// Closed-catalog member enumeration for drift-gate pins.
    pub fn all_variants() -> &'static [&'static str] {
        &["stop", "error", "cancelled"]
    }
}

// ════════════════════════════════════════════════════════════════════
//  ToolChunk — closed-catalog tool stream chunk
// ════════════════════════════════════════════════════════════════════

/// Closed-catalog tool stream chunk. Emitted by [`Tool::stream`]
/// per chunk; consumed by the dispatcher's `unified_stream_handler`
/// (Fase 34.g) which drains the stream through `StreamPolicyEnforcer`
/// with the declared backpressure policy + forwards each delivered
/// chunk to `ctx.tx` as `FlowExecutionEvent::StepToken`.
///
/// Field shape:
///
/// - **`delta`** — the chunk's content delta. Adopter-defined
///   semantics; tools producing JSON might emit partial JSON
///   strings, tools producing prose emit token-by-token text.
///   Empty `delta` is permitted (e.g. a terminator-only chunk).
/// - **`finish_reason`** — populated **ONLY** on the terminator
///   chunk. `None` on intermediate chunks (continuation semantics).
///   Serde-elided when None per D4 byte-compat (matches the
///   v1.27.x audit-row optional-field elision pattern).
/// - **`timestamp_ms`** — Unix milliseconds when the chunk was
///   emitted by the tool body. Used by [`StepAuditRecord`] for the
///   per-chunk wall-clock trail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolChunk {
    pub delta: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<ToolFinishReason>,
    pub timestamp_ms: u64,
}

impl ToolChunk {
    /// Current Unix milliseconds. Helper for tool implementations
    /// that don't carry their own clock.
    pub fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Construct an **intermediate** chunk (continuation; no
    /// terminator). `delta` carries the chunk content;
    /// `finish_reason` is `None`; `timestamp_ms` reads the wall
    /// clock.
    pub fn intermediate(delta: impl Into<String>) -> Self {
        ToolChunk {
            delta: delta.into(),
            finish_reason: None,
            timestamp_ms: Self::now_ms(),
        }
    }

    /// Construct a **terminator** chunk carrying the given
    /// finish reason. `delta` may be empty when the terminator
    /// carries no additional content (typical end-of-stream marker).
    pub fn terminator(
        delta: impl Into<String>,
        finish_reason: ToolFinishReason,
    ) -> Self {
        ToolChunk {
            delta: delta.into(),
            finish_reason: Some(finish_reason),
            timestamp_ms: Self::now_ms(),
        }
    }

    /// Convert a synchronous [`ToolResult`] into a single-chunk
    /// stream's lone chunk. Used by the [`Tool::stream`] default
    /// impl to wrap `execute()` results as a one-element stream
    /// (D9 backwards-compat).
    pub fn from_result(result: &ToolResult) -> Self {
        let finish_reason = Some(if result.success {
            ToolFinishReason::Stop
        } else {
            ToolFinishReason::Error {
                message: format!("tool '{}' failed", result.tool_name),
            }
        });
        ToolChunk {
            delta: result.output.clone(),
            finish_reason,
            timestamp_ms: Self::now_ms(),
        }
    }

    /// Whether this chunk is the stream's terminator (carries a
    /// `finish_reason`).
    pub fn is_terminator(&self) -> bool {
        self.finish_reason.is_some()
    }
}

// ════════════════════════════════════════════════════════════════════
//  ToolContext — per-invocation context the dispatcher passes
// ════════════════════════════════════════════════════════════════════

/// Per-tool-invocation context. The dispatcher's
/// `pure_shape::run_step` constructs one of these per stream-tool
/// invocation (Fase 34.d) and passes it through `Tool::stream`.
///
/// Carries:
/// - **`cancel`** — the request-scoped `CancellationFlag`. Tool
///   bodies poll this between chunks for D5 cancel-into-tool-body
///   propagation. HTTP-tool bodies use this to abort the upstream
///   request via `drop(reqwest_response)`; MCP-tool bodies use it
///   to send `$/cancelRequest` JSON-RPC notifications.
/// - **`trace_id`** — the request-scoped UUID (Fase 32.h
///   correlation). Tools can attach this to outbound HTTP requests
///   for distributed trace propagation, or include it in their own
///   audit logs for cross-system correlation.
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub cancel: CancellationFlag,
    pub trace_id: u64,
}

impl ToolContext {
    /// Construct a new `ToolContext` from its components. The
    /// dispatcher is the typical caller (Fase 34.d).
    pub fn new(cancel: CancellationFlag, trace_id: u64) -> Self {
        ToolContext { cancel, trace_id }
    }

    /// Whether cancellation has been signalled. Tool bodies poll
    /// this between chunks for cooperative cancel.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

// ════════════════════════════════════════════════════════════════════
//  Tool — async trait for tool implementations
// ════════════════════════════════════════════════════════════════════

/// Async tool trait. Implementations define `execute()` for
/// synchronous tool invocation (returns a single materialized
/// [`ToolResult`]) AND optionally override `stream()` for
/// per-chunk streaming dispatch.
///
/// # Default `stream()` impl
///
/// The default `stream()` impl invokes `execute()` and wraps the
/// result as a **single-chunk stream** via
/// [`ToolChunk::from_result`]. This is the D9 backwards-compat
/// guarantee — every existing tool that only implements `execute()`
/// automatically becomes a single-chunk stream producer when the
/// dispatcher invokes `stream()` on it. Wire bytes unchanged.
///
/// # When to override `stream()`
///
/// Override `stream()` when the tool's body genuinely produces a
/// stream — for example:
/// - An HTTP tool talking to an SSE upstream (Fase 34.e).
/// - An MCP tool talking to a server that emits partial-response
///   notifications (Fase 34.f).
/// - A native Rust tool computing a sequence of chunks over a
///   long-running computation.
///
/// When overriding `stream()`, also override `is_streaming()` to
/// return `true` — this signals the dispatcher to bypass the LLM
/// upstream and invoke `stream()` directly (Fase 34.d branching).
///
/// # Cross-stack mirror
///
/// Python `axon.tools.Tool` ABC mirrors this surface:
///
/// ```python
/// class Tool:
///     async def execute(self, args: str, ctx: ToolContext) -> ToolResult: ...
///     async def stream(self, args: str, ctx: ToolContext) -> AsyncIterator[ToolChunk]: ...
///     def is_streaming(self) -> bool: ...
/// ```
///
/// Cross-stack drift gate at
/// `tests/test_fase34_b_tool_trait_cross_stack.py` enforces the
/// 1-to-1 method-signature contract (D10).
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// Execute the tool synchronously. Returns a single materialized
    /// [`ToolResult`]. EVERY tool MUST implement this method
    /// (no default — there's no sensible default execute).
    async fn execute(&self, args: String, ctx: ToolContext) -> ToolResult;

    /// Stream the tool's output as a sequence of [`ToolChunk`]s.
    ///
    /// **Default impl**: invokes `execute()` and wraps the result
    /// as a single-chunk stream via [`ToolChunk::from_result`]. This
    /// is the D9 backwards-compat guarantee — every existing
    /// non-streaming tool automatically becomes a single-chunk
    /// stream producer.
    ///
    /// **Overriding** `stream()` lets the tool's body emit multiple
    /// chunks. Implementations that override `stream()` SHOULD
    /// also override `is_streaming()` to return `true` so the
    /// dispatcher routes through the streaming path (Fase 34.d).
    async fn stream(&self, args: String, ctx: ToolContext) -> ToolStream {
        let result = self.execute(args, ctx).await;
        Box::pin(futures::stream::once(async move {
            ToolChunk::from_result(&result)
        }))
    }

    /// Whether this tool is a stream producer. Default: `false`.
    ///
    /// The dispatcher's `pure_shape::run_step` (Fase 34.d) reads
    /// this flag to decide whether to route through the streaming
    /// path (`stream()`) or the synchronous path (`execute()`).
    /// Tools that override `stream()` to emit multiple chunks
    /// SHOULD override `is_streaming()` to return `true`.
    ///
    /// Tool implementations registered via the registry (Fase 34.c)
    /// get this flag automatically derived from their declared
    /// `effect_row` — presence of `<stream:<policy>>` sets it to
    /// `true`. Adopters writing direct Rust impls of `Tool`
    /// override this method when their tool body streams.
    fn is_streaming(&self) -> bool {
        false
    }
}

// ════════════════════════════════════════════════════════════════════
//  Lib unit tests — 15 cells
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    // ─── Cell 1-4: ToolChunk serde round-trip ──────────────────────

    #[test]
    fn cell_01_toolchunk_serde_round_trip_intermediate() {
        let chunk = ToolChunk::intermediate("Hola");
        let json = serde_json::to_string(&chunk).unwrap();
        // finish_reason is None → must be ELIDED from JSON (D4
        // byte-compat: matches the optional-field elision pattern
        // v1.27.x audit rows use).
        assert!(
            !json.contains("finish_reason"),
            "intermediate chunk's None finish_reason MUST be elided. \
             Got JSON: {json}"
        );
        let back: ToolChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(back.delta, "Hola");
        assert_eq!(back.finish_reason, None);
        assert_eq!(back.timestamp_ms, chunk.timestamp_ms);
    }

    #[test]
    fn cell_02_toolchunk_serde_round_trip_terminator_stop() {
        let chunk = ToolChunk::terminator("", ToolFinishReason::Stop);
        let json = serde_json::to_string(&chunk).unwrap();
        assert!(json.contains("\"finish_reason\""));
        assert!(json.contains("\"kind\":\"stop\""));
        let back: ToolChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(back.finish_reason, Some(ToolFinishReason::Stop));
    }

    #[test]
    fn cell_03_toolchunk_serde_round_trip_terminator_error() {
        let chunk = ToolChunk::terminator(
            "",
            ToolFinishReason::Error {
                message: "upstream timeout".to_string(),
            },
        );
        let json = serde_json::to_string(&chunk).unwrap();
        assert!(json.contains("\"kind\":\"error\""));
        assert!(json.contains("upstream timeout"));
        let back: ToolChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.finish_reason,
            Some(ToolFinishReason::Error {
                message: "upstream timeout".to_string()
            })
        );
    }

    #[test]
    fn cell_04_toolchunk_serde_round_trip_terminator_cancelled() {
        let chunk = ToolChunk::terminator("", ToolFinishReason::Cancelled);
        let json = serde_json::to_string(&chunk).unwrap();
        assert!(json.contains("\"kind\":\"cancelled\""));
        let back: ToolChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(back.finish_reason, Some(ToolFinishReason::Cancelled));
    }

    // ─── Cell 5-6: ToolChunk constructors ──────────────────────────

    #[test]
    fn cell_05_toolchunk_intermediate_constructor() {
        let chunk = ToolChunk::intermediate("partial");
        assert_eq!(chunk.delta, "partial");
        assert_eq!(chunk.finish_reason, None);
        assert!(chunk.timestamp_ms > 0);
        assert!(!chunk.is_terminator());
    }

    #[test]
    fn cell_06_toolchunk_terminator_constructor() {
        let chunk = ToolChunk::terminator("final", ToolFinishReason::Stop);
        assert_eq!(chunk.delta, "final");
        assert_eq!(chunk.finish_reason, Some(ToolFinishReason::Stop));
        assert!(chunk.is_terminator());
    }

    // ─── Cell 7-8: ToolChunk::from_result conversion ───────────────

    #[test]
    fn cell_07_toolchunk_from_result_success_maps_to_stop() {
        let result = ToolResult {
            success: true,
            output: "42".to_string(),
            tool_name: "Calculator".to_string(),
        };
        let chunk = ToolChunk::from_result(&result);
        assert_eq!(chunk.delta, "42");
        assert_eq!(chunk.finish_reason, Some(ToolFinishReason::Stop));
        assert!(chunk.is_terminator());
    }

    #[test]
    fn cell_08_toolchunk_from_result_failure_maps_to_error() {
        let result = ToolResult {
            success: false,
            output: "division by zero".to_string(),
            tool_name: "Calculator".to_string(),
        };
        let chunk = ToolChunk::from_result(&result);
        assert_eq!(chunk.delta, "division by zero");
        match chunk.finish_reason {
            Some(ToolFinishReason::Error { ref message }) => {
                assert!(message.contains("Calculator"));
            }
            other => panic!("expected Error finish_reason, got {other:?}"),
        }
    }

    // ─── Cell 9: ToolFinishReason closed-catalog totality ─────────

    #[test]
    fn cell_09_toolfinishreason_all_variants_pinned() {
        let variants = ToolFinishReason::all_variants();
        assert_eq!(
            variants,
            &["stop", "error", "cancelled"],
            "closed-catalog: ToolFinishReason has EXACTLY 3 \
             reachable states (stop / error / cancelled). Adding a 4th \
             requires a deliberate change + cross-stack drift gate \
             update + adopter docs update."
        );
        // Compile-time enumeration pin — every variant MUST be \
        // matched here so a future variant addition fires this pin.
        match ToolFinishReason::Stop {
            ToolFinishReason::Stop => {}
            ToolFinishReason::Error { .. } => unreachable!(),
            ToolFinishReason::Cancelled => unreachable!(),
        }
    }

    // ─── Cell 10: Tool trait default is_streaming returns false ───

    struct SyncTool;

    #[async_trait::async_trait]
    impl Tool for SyncTool {
        async fn execute(&self, args: String, _ctx: ToolContext) -> ToolResult {
            ToolResult {
                success: true,
                output: format!("sync({args})"),
                tool_name: "SyncTool".to_string(),
            }
        }
        // No override of stream() or is_streaming() — uses defaults.
    }

    #[test]
    fn cell_10_tool_trait_default_is_streaming_is_false() {
        let tool = SyncTool;
        assert_eq!(
            tool.is_streaming(),
            false,
            "33.b D1 + D9 default: tools that don't declare a stream \
             effect have is_streaming() == false. Backwards-compat: \
             every existing tool stays out of the streaming dispatch \
             path."
        );
    }

    // ─── Cell 11: Default stream() wraps execute() as 1-chunk ─────

    #[tokio::test]
    async fn cell_11_tool_trait_default_stream_wraps_execute_one_chunk() {
        let tool = SyncTool;
        let cancel = CancellationFlag::new();
        let ctx = ToolContext::new(cancel, 0xDEAD_BEEF);
        let mut stream = tool.stream("hello".to_string(), ctx).await;

        let first = stream.next().await.expect("at least one chunk");
        assert_eq!(first.delta, "sync(hello)");
        assert_eq!(first.finish_reason, Some(ToolFinishReason::Stop));
        assert!(first.is_terminator());

        // D9 single-chunk guarantee: exactly ONE chunk for a tool
        // that uses the default stream() impl.
        let second = stream.next().await;
        assert!(
            second.is_none(),
            "33.b D9: default stream() emits EXACTLY 1 chunk. \
             Got a second chunk: {second:?}"
        );
    }

    // ─── Cell 12: Override is_streaming() returns true ─────────────

    struct StreamingTool;

    #[async_trait::async_trait]
    impl Tool for StreamingTool {
        async fn execute(&self, _args: String, _ctx: ToolContext) -> ToolResult {
            ToolResult {
                success: true,
                output: "materialized fallback".to_string(),
                tool_name: "StreamingTool".to_string(),
            }
        }

        async fn stream(&self, _args: String, _ctx: ToolContext) -> ToolStream {
            // Emit 3 intermediate chunks + 1 terminator = 4 total.
            let chunks = vec![
                ToolChunk::intermediate("alpha "),
                ToolChunk::intermediate("beta "),
                ToolChunk::intermediate("gamma"),
                ToolChunk::terminator("", ToolFinishReason::Stop),
            ];
            Box::pin(futures::stream::iter(chunks))
        }

        fn is_streaming(&self) -> bool {
            true
        }
    }

    #[test]
    fn cell_12_tool_trait_override_is_streaming_returns_true() {
        let tool = StreamingTool;
        assert!(
            tool.is_streaming(),
            "33.b D1: tools that override stream() to emit multiple \
             chunks SHOULD override is_streaming() to return true"
        );
    }

    // ─── Cell 13: Override stream() emits multiple chunks ─────────

    #[tokio::test]
    async fn cell_13_tool_trait_override_stream_emits_multiple_chunks() {
        let tool = StreamingTool;
        let cancel = CancellationFlag::new();
        let ctx = ToolContext::new(cancel, 0xDEAD_BEEF);
        let mut stream = tool.stream("".to_string(), ctx).await;

        let mut collected: Vec<ToolChunk> = Vec::new();
        while let Some(chunk) = stream.next().await {
            collected.push(chunk);
        }
        assert_eq!(
            collected.len(),
            4,
            "33.b: StreamingTool override emits exactly 4 chunks \
             (3 intermediate + 1 terminator). Got {} chunks.",
            collected.len()
        );
        assert_eq!(collected[0].delta, "alpha ");
        assert_eq!(collected[1].delta, "beta ");
        assert_eq!(collected[2].delta, "gamma");
        assert_eq!(collected[3].delta, "");
        // Only the last chunk is a terminator.
        assert!(!collected[0].is_terminator());
        assert!(!collected[1].is_terminator());
        assert!(!collected[2].is_terminator());
        assert!(collected[3].is_terminator());
        assert_eq!(collected[3].finish_reason, Some(ToolFinishReason::Stop));
    }

    // ─── Cell 14: D4 byte-compat — finish_reason elided when None ─

    #[test]
    fn cell_14_toolchunk_d4_byte_compat_finish_reason_elided() {
        let chunk = ToolChunk {
            delta: "x".to_string(),
            finish_reason: None,
            timestamp_ms: 42,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        // D4 byte-compat: None fields MUST be elided. The serialized
        // form is `{"delta":"x","timestamp_ms":42}` — adopter parsers
        // ignoring unknown fields see no observable change when a
        // future axon-lang minor adds new optional fields.
        assert_eq!(
            json, r#"{"delta":"x","timestamp_ms":42}"#,
            "33.b D4: serialized JSON MUST elide None finish_reason. \
             Got: {json}"
        );
    }

    // ─── Cell 15: ToolContext constructor + field access ──────────

    #[test]
    fn cell_15_toolcontext_constructor_and_field_access() {
        let cancel = CancellationFlag::new();
        let ctx = ToolContext::new(cancel.clone(), 0xCAFE_BABE);
        assert_eq!(ctx.trace_id, 0xCAFE_BABE);
        assert!(!ctx.is_cancelled());
        // After firing the flag, the context observes it.
        cancel.cancel();
        assert!(ctx.is_cancelled());
    }
}
