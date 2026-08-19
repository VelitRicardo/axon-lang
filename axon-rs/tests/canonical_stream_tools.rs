//! v1.29.0 — Canonical streaming-tool patterns (adopter-agnostic).
//!
//! 8 canonical patterns demonstrating **real** `Tool::stream()`
//! implementations — multi-chunk producers, NOT single-chunk
//! `execute()` wrappers. Each pattern is domain-neutral: it shows
//! a distinct streaming capability any adopter can build on.
//!
//! ## OSS / ENTERPRISE split
//!
//! Verticals (banking / medicine / legal / government) are
//! **exclusive to axon-enterprise** per the project charter — the
//! OSS `axon-lang` crate stays 100% adopter-agnostic. The
//! vertical-grounded instances of these patterns (HIPAA PHI
//! scrubber streamer, FRE 502 privilege-assessment streamer, PCI
//! DSS Req 10 compliance-log streamer, FedRAMP AU-2 audit-trail
//! streamer) ship in axon-enterprise's vertical R&D track via the
//! v1.20.0 catch-up release (v1.29.0). This file ships ONLY the
//! domain-neutral canonical patterns those vertical tools are
//! built on top of.
//!
//! ## The 8 canonical patterns
//!
//! 1. **ChunkedListProcessor** — per-input-item streaming: one
//!    chunk per element of a comma-separated list.
//! 2. **ProgressiveRefinement** — multi-stage refinement: emits
//!    `draft → refined → final`.
//! 3. **PaginatedSource** — pagination streaming: one chunk per
//!    page of an N-page result set.
//! 4. **MultiStagePipeline** — pipeline-stage streaming: one chunk
//!    per stage (`parse → transform → validate → emit`).
//! 5. **ProgressReporter** — progress + result streaming: progress
//!    markers (25/50/75/100) then a final result chunk.
//! 6. **EarlyErrorTool** — mid-stream error surfacing: 2 partial
//!    chunks then a `ToolFinishReason::Error` terminator.
//! 7. **CancelAwareCounter** — cooperative cancel-into-tool-body
//!    (D5): emits `tick-i` chunks, polling `ctx.cancel` between
//!    each; on cancel emits a `Cancelled` terminator.
//! 8. **BurstProducer** — fast burst producer for exercising
//!    backpressure policy at the tool layer.
//!
//! ## What the tests prove
//!
//! - Each pattern's `stream()` produces the **canonical delta
//!   sequence** (a shared cross-stack corpus — the Python mirror
//!   `tests/test_fase34_canonical_stream_tools.py` asserts the
//!   SAME sequences).
//! - Each pattern drains correctly through
//! `unified_stream_handler` (the v1.29.0 convergence point) —
//!   per-chunk `FlowExecutionEvent::StepToken` wire emission +
//!   a well-formed `ToolStreamSummary`.
//! - Backpressure policy enforcement works on real adopter tools.
//! - Cancel + error terminators surface correctly.

#![allow(clippy::needless_return)]

use async_trait::async_trait;
use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::unified_stream::unified_stream_handler;
use axon::flow_execution_event::FlowExecutionEvent;
use axon::stream_effect::BackpressurePolicy;
use axon::tool_executor::ToolResult;
use axon::tool_trait::{Tool, ToolChunk, ToolContext, ToolFinishReason, ToolStream};
use futures::stream::{self, StreamExt};
use tokio::sync::mpsc;

// ════════════════════════════════════════════════════════════════════
//  Pattern 1 — ChunkedListProcessor
// ════════════════════════════════════════════════════════════════════

/// Streams one `item:<x>` chunk per non-empty element of a
/// comma-separated `args` list. Empty input → zero intermediate
/// chunks + a Stop terminator. The canonical per-input-item
/// streaming pattern.
struct ChunkedListProcessor;

fn list_items(args: &str) -> Vec<String> {
    args.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| format!("item:{s}"))
        .collect()
}

#[async_trait]
impl Tool for ChunkedListProcessor {
    async fn execute(&self, args: String, _ctx: ToolContext) -> ToolResult {
        ToolResult {
            success: true,
            output: list_items(&args).join("|"),
            tool_name: "ChunkedListProcessor".to_string(),
        }
    }

    async fn stream(&self, args: String, _ctx: ToolContext) -> ToolStream {
        let mut chunks: Vec<ToolChunk> = list_items(&args)
            .into_iter()
            .map(ToolChunk::intermediate)
            .collect();
        chunks.push(ToolChunk::terminator("", ToolFinishReason::Stop));
        Box::pin(stream::iter(chunks))
    }

    fn is_streaming(&self) -> bool {
        true
    }
}

// ════════════════════════════════════════════════════════════════════
//  Pattern 2 — ProgressiveRefinement
// ════════════════════════════════════════════════════════════════════

/// Streams a 3-stage refinement: `draft:<args>`, `refined:<args>`,
/// `final:<args>`. The canonical multi-stage-refinement pattern.
struct ProgressiveRefinement;

fn refinement_stages(args: &str) -> Vec<String> {
    vec![
        format!("draft:{args}"),
        format!("refined:{args}"),
        format!("final:{args}"),
    ]
}

#[async_trait]
impl Tool for ProgressiveRefinement {
    async fn execute(&self, args: String, _ctx: ToolContext) -> ToolResult {
        ToolResult {
            success: true,
            output: refinement_stages(&args).join("|"),
            tool_name: "ProgressiveRefinement".to_string(),
        }
    }

    async fn stream(&self, args: String, _ctx: ToolContext) -> ToolStream {
        let mut chunks: Vec<ToolChunk> = refinement_stages(&args)
            .into_iter()
            .map(ToolChunk::intermediate)
            .collect();
        chunks.push(ToolChunk::terminator("", ToolFinishReason::Stop));
        Box::pin(stream::iter(chunks))
    }

    fn is_streaming(&self) -> bool {
        true
    }
}

// ════════════════════════════════════════════════════════════════════
//  Pattern 3 — PaginatedSource
// ════════════════════════════════════════════════════════════════════

/// Streams one `page-<i>` chunk per page of an N-page result set.
/// `args` is the page count (unparseable → 0 pages). The canonical
/// pagination-streaming pattern.
struct PaginatedSource;

fn pages(args: &str) -> Vec<String> {
    let n: usize = args.trim().parse().unwrap_or(0);
    (0..n).map(|i| format!("page-{i}")).collect()
}

#[async_trait]
impl Tool for PaginatedSource {
    async fn execute(&self, args: String, _ctx: ToolContext) -> ToolResult {
        ToolResult {
            success: true,
            output: pages(&args).join("|"),
            tool_name: "PaginatedSource".to_string(),
        }
    }

    async fn stream(&self, args: String, _ctx: ToolContext) -> ToolStream {
        let mut chunks: Vec<ToolChunk> =
            pages(&args).into_iter().map(ToolChunk::intermediate).collect();
        chunks.push(ToolChunk::terminator("", ToolFinishReason::Stop));
        Box::pin(stream::iter(chunks))
    }

    fn is_streaming(&self) -> bool {
        true
    }
}

// ════════════════════════════════════════════════════════════════════
//  Pattern 4 — MultiStagePipeline
// ════════════════════════════════════════════════════════════════════

/// Streams one chunk per pipeline stage: `parse`, `transform`,
/// `validate`, `emit` — each formatted `stage:<name>:<args>`. The
/// canonical pipeline-stage-streaming pattern.
struct MultiStagePipeline;

const PIPELINE_STAGES: &[&str] = &["parse", "transform", "validate", "emit"];

fn pipeline_chunks(args: &str) -> Vec<String> {
    PIPELINE_STAGES
        .iter()
        .map(|stage| format!("stage:{stage}:{args}"))
        .collect()
}

#[async_trait]
impl Tool for MultiStagePipeline {
    async fn execute(&self, args: String, _ctx: ToolContext) -> ToolResult {
        ToolResult {
            success: true,
            output: pipeline_chunks(&args).join("|"),
            tool_name: "MultiStagePipeline".to_string(),
        }
    }

    async fn stream(&self, args: String, _ctx: ToolContext) -> ToolStream {
        let mut chunks: Vec<ToolChunk> = pipeline_chunks(&args)
            .into_iter()
            .map(ToolChunk::intermediate)
            .collect();
        chunks.push(ToolChunk::terminator("", ToolFinishReason::Stop));
        Box::pin(stream::iter(chunks))
    }

    fn is_streaming(&self) -> bool {
        true
    }
}

// ════════════════════════════════════════════════════════════════════
//  Pattern 5 — ProgressReporter
// ════════════════════════════════════════════════════════════════════

/// Streams progress markers (`progress:25` … `progress:100`) then a
/// final `result:<args>` chunk. The canonical progress + result
/// streaming pattern.
struct ProgressReporter;

fn progress_chunks(args: &str) -> Vec<String> {
    vec![
        "progress:25".to_string(),
        "progress:50".to_string(),
        "progress:75".to_string(),
        "progress:100".to_string(),
        format!("result:{args}"),
    ]
}

#[async_trait]
impl Tool for ProgressReporter {
    async fn execute(&self, args: String, _ctx: ToolContext) -> ToolResult {
        ToolResult {
            success: true,
            output: progress_chunks(&args).join("|"),
            tool_name: "ProgressReporter".to_string(),
        }
    }

    async fn stream(&self, args: String, _ctx: ToolContext) -> ToolStream {
        let mut chunks: Vec<ToolChunk> = progress_chunks(&args)
            .into_iter()
            .map(ToolChunk::intermediate)
            .collect();
        chunks.push(ToolChunk::terminator("", ToolFinishReason::Stop));
        Box::pin(stream::iter(chunks))
    }

    fn is_streaming(&self) -> bool {
        true
    }
}

// ════════════════════════════════════════════════════════════════════
//  Pattern 6 — EarlyErrorTool
// ════════════════════════════════════════════════════════════════════

/// Streams 2 partial chunks then surfaces a mid-stream
/// `ToolFinishReason::Error` terminator. The canonical
/// error-mid-stream pattern — proves that an adopter tool can fail
/// PART-WAY through a stream + the dispatcher surfaces it honestly.
struct EarlyErrorTool;

const EARLY_ERROR_MESSAGE: &str = "early-error-tool: simulated mid-stream failure";

fn early_error_partials(args: &str) -> Vec<String> {
    vec![format!("partial:{args}:1"), format!("partial:{args}:2")]
}

#[async_trait]
impl Tool for EarlyErrorTool {
    async fn execute(&self, _args: String, _ctx: ToolContext) -> ToolResult {
        ToolResult {
            success: false,
            output: EARLY_ERROR_MESSAGE.to_string(),
            tool_name: "EarlyErrorTool".to_string(),
        }
    }

    async fn stream(&self, args: String, _ctx: ToolContext) -> ToolStream {
        let mut chunks: Vec<ToolChunk> = early_error_partials(&args)
            .into_iter()
            .map(ToolChunk::intermediate)
            .collect();
        chunks.push(ToolChunk::terminator(
            "",
            ToolFinishReason::Error {
                message: EARLY_ERROR_MESSAGE.to_string(),
            },
        ));
        Box::pin(stream::iter(chunks))
    }

    fn is_streaming(&self) -> bool {
        true
    }
}

// ════════════════════════════════════════════════════════════════════
//  Pattern 7 — CancelAwareCounter
// ════════════════════════════════════════════════════════════════════

/// Streams up to N `tick-<i>` chunks, polling `ctx.cancel` between
/// each via a lazy `futures::stream::unfold`. When cancel fires
/// mid-stream the tool emits a `ToolFinishReason::Cancelled`
/// terminator + ends. The canonical cooperative cancel-into-tool-
/// body pattern (v1.29.0 D5 discipline at the adopter-tool layer).
struct CancelAwareCounter;

#[async_trait]
impl Tool for CancelAwareCounter {
    async fn execute(&self, args: String, _ctx: ToolContext) -> ToolResult {
        let n: usize = args.trim().parse().unwrap_or(0);
        ToolResult {
            success: true,
            output: format!("counted {n}"),
            tool_name: "CancelAwareCounter".to_string(),
        }
    }

    async fn stream(&self, args: String, ctx: ToolContext) -> ToolStream {
        let max: usize = args.trim().parse().unwrap_or(0);
        let cancel = ctx.cancel.clone();
        // unfold state: the next tick index. State `max` emits the
        // Stop terminator; state `> max` ends the stream. A cancel
        // observed mid-stream jumps the state to `max + 1` after
        // emitting the Cancelled terminator.
        Box::pin(stream::unfold(0usize, move |i| {
            let cancel = cancel.clone();
            async move {
                if i > max {
                    None
                } else if cancel.is_cancelled() && i < max {
                    // Cooperative cancel: emit Cancelled + end.
                    Some((
                        ToolChunk::terminator("", ToolFinishReason::Cancelled),
                        max + 1,
                    ))
                } else if i == max {
                    Some((
                        ToolChunk::terminator("", ToolFinishReason::Stop),
                        i + 1,
                    ))
                } else {
                    Some((ToolChunk::intermediate(format!("tick-{i}")), i + 1))
                }
            }
        }))
    }

    fn is_streaming(&self) -> bool {
        true
    }
}

// ════════════════════════════════════════════════════════════════════
//  Pattern 8 — BurstProducer
// ════════════════════════════════════════════════════════════════════

/// Streams a burst of N `burst-<i>` chunks rapidly. The canonical
/// fast-producer pattern — used to exercise backpressure policy
/// enforcement at the adopter-tool layer.
struct BurstProducer;

fn burst_chunks(args: &str) -> Vec<String> {
    let n: usize = args.trim().parse().unwrap_or(0);
    (0..n).map(|i| format!("burst-{i}")).collect()
}

#[async_trait]
impl Tool for BurstProducer {
    async fn execute(&self, args: String, _ctx: ToolContext) -> ToolResult {
        ToolResult {
            success: true,
            output: format!("burst of {}", burst_chunks(&args).len()),
            tool_name: "BurstProducer".to_string(),
        }
    }

    async fn stream(&self, args: String, _ctx: ToolContext) -> ToolStream {
        let mut chunks: Vec<ToolChunk> = burst_chunks(&args)
            .into_iter()
            .map(ToolChunk::intermediate)
            .collect();
        chunks.push(ToolChunk::terminator("", ToolFinishReason::Stop));
        Box::pin(stream::iter(chunks))
    }

    fn is_streaming(&self) -> bool {
        true
    }
}

// ════════════════════════════════════════════════════════════════════
//  Cross-stack canonical corpus
// ════════════════════════════════════════════════════════════════════

/// One canonical case: a `(tool, input)` pair + the expected
/// intermediate-delta sequence + the expected terminator kind.
/// The Python mirror `tests/test_fase34_canonical_stream_tools.py`
/// asserts the SAME corpus — drift in either stack fails both.
struct CanonicalCase {
    tool: &'static str,
    input: &'static str,
    expected_deltas: &'static [&'static str],
    terminator_kind: &'static str,
}

const CANONICAL_CORPUS: &[CanonicalCase] = &[
    CanonicalCase {
        tool: "ChunkedListProcessor",
        input: "alpha,beta,gamma",
        expected_deltas: &["item:alpha", "item:beta", "item:gamma"],
        terminator_kind: "stop",
    },
    CanonicalCase {
        tool: "ProgressiveRefinement",
        input: "hello",
        expected_deltas: &["draft:hello", "refined:hello", "final:hello"],
        terminator_kind: "stop",
    },
    CanonicalCase {
        tool: "PaginatedSource",
        input: "4",
        expected_deltas: &["page-0", "page-1", "page-2", "page-3"],
        terminator_kind: "stop",
    },
    CanonicalCase {
        tool: "MultiStagePipeline",
        input: "data",
        expected_deltas: &[
            "stage:parse:data",
            "stage:transform:data",
            "stage:validate:data",
            "stage:emit:data",
        ],
        terminator_kind: "stop",
    },
    CanonicalCase {
        tool: "ProgressReporter",
        input: "output",
        expected_deltas: &[
            "progress:25",
            "progress:50",
            "progress:75",
            "progress:100",
            "result:output",
        ],
        terminator_kind: "stop",
    },
    CanonicalCase {
        tool: "EarlyErrorTool",
        input: "task",
        expected_deltas: &["partial:task:1", "partial:task:2"],
        terminator_kind: "error",
    },
    CanonicalCase {
        tool: "CancelAwareCounter",
        input: "5",
        expected_deltas: &["tick-0", "tick-1", "tick-2", "tick-3", "tick-4"],
        terminator_kind: "stop",
    },
    CanonicalCase {
        tool: "BurstProducer",
        input: "6",
        expected_deltas: &[
            "burst-0", "burst-1", "burst-2", "burst-3", "burst-4", "burst-5",
        ],
        terminator_kind: "stop",
    },
];

// ════════════════════════════════════════════════════════════════════
//  Test scaffolding
// ════════════════════════════════════════════════════════════════════

fn fresh_ctx() -> ToolContext {
    ToolContext::new(CancellationFlag::new(), 0x34)
}

/// Drain a tool's `stream()` directly into (intermediate deltas,
/// terminator kind slug). The terminator's own delta is appended to
/// the intermediate list IFF non-empty (mirrors the dispatcher's
/// "emit non-empty deltas, even on terminator chunks" rule).
async fn drain_pattern(
    tool: &dyn Tool,
    args: &str,
    ctx: ToolContext,
) -> (Vec<String>, String) {
    let mut s = tool.stream(args.to_string(), ctx).await;
    let mut deltas = Vec::new();
    let mut terminator = "none".to_string();
    while let Some(chunk) = s.next().await {
        if !chunk.delta.is_empty() {
            deltas.push(chunk.delta.clone());
        }
        if let Some(reason) = &chunk.finish_reason {
            terminator = match reason {
                ToolFinishReason::Stop => "stop",
                ToolFinishReason::Error { .. } => "error",
                ToolFinishReason::Cancelled => "cancelled",
            }
            .to_string();
            break;
        }
    }
    (deltas, terminator)
}

fn pattern_by_name(name: &str) -> Box<dyn Tool> {
    match name {
        "ChunkedListProcessor" => Box::new(ChunkedListProcessor),
        "ProgressiveRefinement" => Box::new(ProgressiveRefinement),
        "PaginatedSource" => Box::new(PaginatedSource),
        "MultiStagePipeline" => Box::new(MultiStagePipeline),
        "ProgressReporter" => Box::new(ProgressReporter),
        "EarlyErrorTool" => Box::new(EarlyErrorTool),
        "CancelAwareCounter" => Box::new(CancelAwareCounter),
        "BurstProducer" => Box::new(BurstProducer),
        other => panic!("unknown canonical pattern: {other}"),
    }
}

// ════════════════════════════════════════════════════════════════════
// section 1 — Each pattern produces its canonical delta sequence × 8
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s1_chunked_list_processor_emits_per_item() {
    let (deltas, term) =
        drain_pattern(&ChunkedListProcessor, "alpha,beta,gamma", fresh_ctx()).await;
    assert_eq!(deltas, vec!["item:alpha", "item:beta", "item:gamma"]);
    assert_eq!(term, "stop");
}

#[tokio::test]
async fn s1_chunked_list_processor_empty_input_emits_only_terminator() {
    let (deltas, term) = drain_pattern(&ChunkedListProcessor, "", fresh_ctx()).await;
    assert!(deltas.is_empty(), "empty input → zero items");
    assert_eq!(term, "stop");
}

#[tokio::test]
async fn s1_progressive_refinement_emits_three_stages() {
    let (deltas, term) =
        drain_pattern(&ProgressiveRefinement, "hello", fresh_ctx()).await;
    assert_eq!(deltas, vec!["draft:hello", "refined:hello", "final:hello"]);
    assert_eq!(term, "stop");
}

#[tokio::test]
async fn s1_paginated_source_emits_n_pages() {
    let (deltas, term) = drain_pattern(&PaginatedSource, "4", fresh_ctx()).await;
    assert_eq!(deltas, vec!["page-0", "page-1", "page-2", "page-3"]);
    assert_eq!(term, "stop");
}

#[tokio::test]
async fn s1_multi_stage_pipeline_emits_four_stages() {
    let (deltas, term) =
        drain_pattern(&MultiStagePipeline, "data", fresh_ctx()).await;
    assert_eq!(
        deltas,
        vec![
            "stage:parse:data",
            "stage:transform:data",
            "stage:validate:data",
            "stage:emit:data"
        ]
    );
    assert_eq!(term, "stop");
}

#[tokio::test]
async fn s1_progress_reporter_emits_progress_then_result() {
    let (deltas, term) =
        drain_pattern(&ProgressReporter, "output", fresh_ctx()).await;
    assert_eq!(
        deltas,
        vec![
            "progress:25",
            "progress:50",
            "progress:75",
            "progress:100",
            "result:output"
        ]
    );
    assert_eq!(term, "stop");
}

#[tokio::test]
async fn s1_early_error_tool_emits_partials_then_error_terminator() {
    let (deltas, term) = drain_pattern(&EarlyErrorTool, "task", fresh_ctx()).await;
    assert_eq!(deltas, vec!["partial:task:1", "partial:task:2"]);
    assert_eq!(term, "error");
}

#[tokio::test]
async fn s1_burst_producer_emits_n_burst_chunks() {
    let (deltas, term) = drain_pattern(&BurstProducer, "6", fresh_ctx()).await;
    assert_eq!(deltas.len(), 6);
    assert_eq!(deltas[0], "burst-0");
    assert_eq!(deltas[5], "burst-5");
    assert_eq!(term, "stop");
}

// ════════════════════════════════════════════════════════════════════
// section 2 — Patterns drain through unified_stream_handler × 4
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s2_progressive_refinement_through_unified_handler_emits_wire_tokens() {
    let cancel = CancellationFlag::new();
    let ctx = ToolContext::new(cancel.clone(), 0);
    let source = ProgressiveRefinement.stream("axon".to_string(), ctx).await;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let summary = unified_stream_handler(source, None, &cancel, &tx, "Refine", "")
        .await
        .expect("ok");

    assert!(summary.is_clean_stop());
    assert_eq!(summary.tokens_emitted, 3);
    let tokens: Vec<String> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|e| match e {
            FlowExecutionEvent::StepToken { content, .. } => Some(content),
            _ => None,
        })
        .collect();
    assert_eq!(tokens, vec!["draft:axon", "refined:axon", "final:axon"]);
}

#[tokio::test]
async fn s2_multi_stage_pipeline_through_unified_handler_hash_is_deterministic() {
    let run = || async {
        let cancel = CancellationFlag::new();
        let ctx = ToolContext::new(cancel.clone(), 0);
        let source = MultiStagePipeline.stream("payload".to_string(), ctx).await;
        let (tx, _rx) = mpsc::unbounded_channel();
        unified_stream_handler(source, None, &cancel, &tx, "Pipe", "")
            .await
            .expect("ok")
    };
    let h1 = run().await.output_hash_hex;
    let h2 = run().await.output_hash_hex;
    assert_eq!(h1, h2, "same pattern + input ⟹ same D6 replay hash");
    assert_eq!(h1.len(), 64);
}

#[tokio::test]
async fn s2_early_error_tool_through_unified_handler_surfaces_terminator_message() {
    let cancel = CancellationFlag::new();
    let ctx = ToolContext::new(cancel.clone(), 0);
    let source = EarlyErrorTool.stream("job".to_string(), ctx).await;
    let (tx, _rx) = mpsc::unbounded_channel();
    let summary = unified_stream_handler(source, None, &cancel, &tx, "Err", "")
        .await
        .expect("ok");
    assert!(!summary.success);
    assert!(!summary.cancelled);
    assert_eq!(
        summary.terminator_message.as_deref(),
        Some(EARLY_ERROR_MESSAGE)
    );
    // The 2 partial chunks reached the wire before the error.
    assert_eq!(summary.tokens_emitted, 2);
}

#[tokio::test]
async fn s2_progress_reporter_through_unified_handler_accumulates_full_output() {
    let cancel = CancellationFlag::new();
    let ctx = ToolContext::new(cancel.clone(), 0);
    let source = ProgressReporter.stream("done".to_string(), ctx).await;
    let (tx, _rx) = mpsc::unbounded_channel();
    let summary = unified_stream_handler(source, None, &cancel, &tx, "Prog", "")
        .await
        .expect("ok");
    assert_eq!(summary.tokens_emitted, 5);
    assert_eq!(
        summary.accumulated,
        "progress:25progress:50progress:75progress:100result:done"
    );
}

// ════════════════════════════════════════════════════════════════════
// section 3 — Backpressure policy enforcement on a real adopter tool × 4
// ════════════════════════════════════════════════════════════════════

async fn burst_through_policy(
    burst: usize,
    policy: BackpressurePolicy,
) -> axon::flow_dispatcher::unified_stream::ToolStreamSummary {
    let cancel = CancellationFlag::new();
    let ctx = ToolContext::new(cancel.clone(), 0);
    let source = BurstProducer.stream(burst.to_string(), ctx).await;
    let (tx, _rx) = mpsc::unbounded_channel();
    unified_stream_handler(source, Some(policy), &cancel, &tx, "Burst", "")
        .await
        .expect("ok")
}

#[tokio::test]
async fn s3_burst_producer_under_drop_oldest_never_fails() {
    let summary = burst_through_policy(300, BackpressurePolicy::DropOldest).await;
    assert!(summary.success);
    assert!(summary.chunks_delivered <= summary.chunks_pushed);
}

#[tokio::test]
async fn s3_burst_producer_under_degrade_quality_conserves_chunks() {
    let summary = burst_through_policy(300, BackpressurePolicy::DegradeQuality).await;
    assert!(summary.success);
    assert_eq!(
        summary.chunks_degraded + summary.chunks_delivered,
        summary.chunks_pushed,
        "degraded + delivered must equal pushed"
    );
}

#[tokio::test]
async fn s3_burst_producer_under_pause_upstream_never_drops() {
    let summary = burst_through_policy(80, BackpressurePolicy::PauseUpstream).await;
    assert!(summary.success);
    assert_eq!(summary.chunks_delivered, summary.chunks_pushed);
}

#[tokio::test]
async fn s3_burst_producer_under_fail_policy_never_panics() {
    // With a live consumer the Fail policy typically completes; the
    // invariant is no-panic + delivered ≤ pushed.
    let summary = burst_through_policy(120, BackpressurePolicy::Fail).await;
    assert!(summary.chunks_delivered <= summary.chunks_pushed);
}

// ════════════════════════════════════════════════════════════════════
// section 4 — Cancel + composition × 3
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s4_cancel_aware_counter_runs_to_completion_without_cancel() {
    let (deltas, term) = drain_pattern(&CancelAwareCounter, "5", fresh_ctx()).await;
    assert_eq!(deltas, vec!["tick-0", "tick-1", "tick-2", "tick-3", "tick-4"]);
    assert_eq!(term, "stop");
}

#[tokio::test]
async fn s4_cancel_aware_counter_honors_mid_stream_cancel() {
    // Drain the counter manually, firing cancel after 2 ticks. The
    // tool's per-tick cancel poll observes the flag + emits a
    // Cancelled terminator instead of continuing to tick-49.
    let cancel = CancellationFlag::new();
    let ctx = ToolContext::new(cancel.clone(), 0);
    let mut s = CancelAwareCounter.stream("50".to_string(), ctx).await;
    let mut deltas = Vec::new();
    let mut terminator = "none".to_string();
    let mut pulled = 0;
    while let Some(chunk) = s.next().await {
        if !chunk.delta.is_empty() {
            deltas.push(chunk.delta.clone());
        }
        pulled += 1;
        if pulled == 2 {
            cancel.cancel(); // fire after 2 ticks
        }
        if let Some(reason) = &chunk.finish_reason {
            terminator = match reason {
                ToolFinishReason::Stop => "stop",
                ToolFinishReason::Error { .. } => "error",
                ToolFinishReason::Cancelled => "cancelled",
            }
            .to_string();
            break;
        }
    }
    assert_eq!(terminator, "cancelled", "mid-stream cancel ⟹ Cancelled terminator");
    // We pulled 2 ticks + 1 terminator; the counter short-circuited
    // well before tick-49.
    assert!(deltas.len() < 50, "cancel short-circuited the count");
    assert!(deltas.iter().all(|d| d.starts_with("tick-")));
}

#[tokio::test]
async fn s4_two_patterns_share_one_unified_handler_step_sequence() {
    // Compose two different patterns through the unified handler in
    // sequence — proves the handler is reusable across heterogeneous
    // adopter tools within one flow.
    let cancel = CancellationFlag::new();
    let (tx, mut rx) = mpsc::unbounded_channel();

    let s1 = PaginatedSource
        .stream("2".to_string(), ToolContext::new(cancel.clone(), 0))
        .await;
    let sum1 = unified_stream_handler(s1, None, &cancel, &tx, "First", "")
        .await
        .expect("ok");
    let s2 = ProgressiveRefinement
        .stream("z".to_string(), ToolContext::new(cancel.clone(), 0))
        .await;
    let sum2 = unified_stream_handler(s2, None, &cancel, &tx, "Second", "")
        .await
        .expect("ok");

    assert_eq!(sum1.tokens_emitted, 2);
    assert_eq!(sum2.tokens_emitted, 3);
    let tokens: Vec<String> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|e| match e {
            FlowExecutionEvent::StepToken { content, .. } => Some(content),
            _ => None,
        })
        .collect();
    assert_eq!(
        tokens,
        vec!["page-0", "page-1", "draft:z", "refined:z", "final:z"]
    );
}

// ════════════════════════════════════════════════════════════════════
// section 5 — Cross-stack canonical corpus pin × 2
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s5_every_canonical_pattern_matches_the_shared_corpus() {
    // The single source of cross-stack truth: every pattern in
    // CANONICAL_CORPUS, when streamed with its input, MUST produce
    // exactly the recorded delta sequence + terminator kind. The
    // Python mirror asserts the SAME corpus — drift in either stack
    // fails both gates.
    for case in CANONICAL_CORPUS {
        let tool = pattern_by_name(case.tool);
        let (deltas, term) =
            drain_pattern(tool.as_ref(), case.input, fresh_ctx()).await;
        assert_eq!(
            deltas,
            case.expected_deltas
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
            "corpus drift on pattern {} input {:?}",
            case.tool,
            case.input
        );
        assert_eq!(
            term, case.terminator_kind,
            "terminator drift on pattern {}",
            case.tool
        );
    }
}

#[test]
fn s5_corpus_covers_all_eight_patterns_and_three_terminator_kinds() {
    assert_eq!(CANONICAL_CORPUS.len(), 8, "corpus must cover all 8 patterns");
    let names: Vec<&str> = CANONICAL_CORPUS.iter().map(|c| c.tool).collect();
    for expected in [
        "ChunkedListProcessor",
        "ProgressiveRefinement",
        "PaginatedSource",
        "MultiStagePipeline",
        "ProgressReporter",
        "EarlyErrorTool",
        "CancelAwareCounter",
        "BurstProducer",
    ] {
        assert!(names.contains(&expected), "corpus missing {expected}");
    }
    // The corpus exercises stop + error terminators (cancelled is
    // covered by the section 4 mid-stream cancel test — it's timing-driven,
    // not a static-corpus case).
    let kinds: Vec<&str> =
        CANONICAL_CORPUS.iter().map(|c| c.terminator_kind).collect();
    assert!(kinds.contains(&"stop"));
    assert!(kinds.contains(&"error"));
}
