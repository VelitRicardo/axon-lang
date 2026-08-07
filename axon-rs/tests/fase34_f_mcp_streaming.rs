//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 34.f (v1.29.0) — End-to-end MCP tool streaming tests.
//!
//! [`axon::emcp::McpStreamingTool`] is the second non-stub provider
//! with first-class streaming surface (after Fase 34.e's
//! `HttpStreamingTool`). This pack proves:
//!
//! 1. Streaming MCP servers (Content-Type `application/x-ndjson` /
//!    `application/jsonl`) emit per-`notifications/message` /
//!    `notifications/progress` ToolChunks; final `result` envelope
//!    closes the stream with `ToolFinishReason::Stop`.
//! 2. Non-streaming MCP servers (Content-Type `application/json`)
//!    fall back to D9 single-chunk wrap byte-equal to legacy
//!    [`axon::emcp::dispatch_mcp`].
//! 3. JSON-RPC error envelopes (mid-stream or single-response) close
//!    the stream with `ToolFinishReason::Error` carrying the
//!    blame-tagged diagnostic.
//! 4. Network surfaces (connection refused / non-2xx / unparseable
//!    body) → `ToolFinishReason::Error` terminator (no panic).
//! 5. Cancellation propagates: pre-cancel + mid-stream cancel both
//!    yield a `Cancelled` terminator; mid-stream best-effort fires
//!    a `notifications/cancelled` POST (not asserted strictly — it's
//!    fire-and-forget per spec).
//! 6. End-to-end composition with the dispatcher arm (Fase 34.d):
//!    streaming MCP tools emit `StepToken` events on the wire for
//!    each notification + audit row captures policy + tokens.
//!
//! 12 tests across 5 sections.

#![allow(clippy::needless_return)]

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axon::cancel_token::CancellationFlag;
use axon::emcp::McpStreamingTool;
use axon::flow_dispatcher::pure_shape::run_step;
use axon::flow_dispatcher::{DispatchCtx, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::*;
use axon::tool_registry::{ToolEntry, ToolRegistry, ToolSource};
use axon::tool_trait::{Tool, ToolChunk, ToolContext, ToolFinishReason};
use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, Response, StatusCode};
use axum::routing::post;
use axum::Router;
use bytes::Bytes;
use futures::StreamExt;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

// ────────────────────────────────────────────────────────────────────
//  Test-server scaffolding (same pattern as fase34_e_http_streaming.rs)
// ────────────────────────────────────────────────────────────────────

async fn spawn_test_server(router: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind 127.0.0.1:0");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    format!("http://{addr}")
}

fn static_response(body: &'static str, content_type: &'static str) -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .expect("response builder")
}

fn make_tool(url: &str) -> McpStreamingTool {
    McpStreamingTool::new(
        "TestMcpTool".to_string(),
        url.to_string(),
        Duration::from_secs(5),
    )
}

fn make_ctx() -> ToolContext {
    ToolContext::new(CancellationFlag::new(), 0x42)
}

async fn drain_tool_stream(tool: &McpStreamingTool, args: &str) -> Vec<ToolChunk> {
    let ctx = make_ctx();
    let mut s = tool.stream(args.to_string(), ctx).await;
    let mut out = Vec::new();
    while let Some(c) = s.next().await {
        out.push(c);
    }
    out
}

// ════════════════════════════════════════════════════════════════════
//  §1 — Streaming framing × 3
// ════════════════════════════════════════════════════════════════════

const STREAMING_NDJSON_BODY: &str = concat!(
    "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{\"data\":\"partial-1\"}}\n",
    "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{\"data\":\"partial-2\"}}\n",
    "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{\"text\":\"75%\"}}\n",
    "{\"jsonrpc\":\"2.0\",\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"final answer\"}]},\"id\":1}\n",
);

async fn streaming_mcp_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    Ok(static_response(STREAMING_NDJSON_BODY, "application/x-ndjson"))
}

#[tokio::test]
async fn s1_streaming_mcp_ndjson_emits_per_notification_then_final_result() {
    let router = Router::new().route("/", post(streaming_mcp_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);
    let chunks = drain_tool_stream(&tool, r#"{"q":"test"}"#).await;

    // Expected sequence:
    //   intermediate("partial-1"), intermediate("partial-2"),
    //   intermediate("75%"), intermediate("final answer"),
    //   terminator("", Stop)
    assert_eq!(chunks.len(), 5, "got chunks: {chunks:#?}");
    assert_eq!(chunks[0].delta, "partial-1");
    assert_eq!(chunks[1].delta, "partial-2");
    assert_eq!(chunks[2].delta, "75%");
    assert_eq!(chunks[3].delta, "final answer");
    assert!(chunks[4].is_terminator());
    assert_eq!(chunks[4].finish_reason, Some(ToolFinishReason::Stop));
}

async fn streaming_mcp_jsonl_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    Ok(static_response(STREAMING_NDJSON_BODY, "application/jsonl"))
}

#[tokio::test]
async fn s1_application_jsonl_content_type_routed_as_ndjson() {
    // `application/jsonl` is an alternative MIME for NDJSON; the
    // classifier MUST treat both identically.
    let router = Router::new().route("/", post(streaming_mcp_jsonl_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);
    let chunks = drain_tool_stream(&tool, "args").await;
    assert_eq!(chunks.len(), 5);
    assert_eq!(chunks[3].delta, "final answer");
    assert_eq!(chunks[4].finish_reason, Some(ToolFinishReason::Stop));
}

const SINGLE_RESPONSE_BODY: &str =
    r#"{"jsonrpc":"2.0","result":{"content":[{"type":"text","text":"single-shot answer"}]},"id":1}"#;

async fn single_response_mcp_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    // Non-streaming MCP server returning a single JSON-RPC response.
    Ok(static_response(SINGLE_RESPONSE_BODY, "application/json"))
}

#[tokio::test]
async fn s1_non_streaming_mcp_server_single_response_d9_fallback() {
    // D9 backwards-compat: MCP servers that don't stream → single
    // chunk + Stop terminator (byte-equal to legacy dispatch_mcp).
    let router = Router::new().route("/", post(single_response_mcp_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);
    let chunks = drain_tool_stream(&tool, "args").await;
    assert_eq!(chunks.len(), 2, "got {chunks:#?}");
    assert_eq!(chunks[0].delta, "single-shot answer");
    assert!(chunks[1].is_terminator());
    assert_eq!(chunks[1].finish_reason, Some(ToolFinishReason::Stop));
}

// ════════════════════════════════════════════════════════════════════
//  §2 — JSON-RPC error surfaces × 3
// ════════════════════════════════════════════════════════════════════

const MID_STREAM_ERROR_BODY: &str = concat!(
    "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{\"data\":\"started\"}}\n",
    "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32000,\"message\":\"internal failure\"},\"id\":1}\n",
);

async fn mid_stream_error_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    Ok(static_response(MID_STREAM_ERROR_BODY, "application/x-ndjson"))
}

#[tokio::test]
async fn s2_mid_stream_jsonrpc_server_error_closes_with_error_terminator_server_blame() {
    let router = Router::new().route("/", post(mid_stream_error_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);
    let chunks = drain_tool_stream(&tool, "args").await;
    // Expected: 1 intermediate("started") + 1 Error terminator
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].delta, "started");
    match &chunks[1].finish_reason {
        Some(ToolFinishReason::Error { message }) => {
            assert!(message.contains("-32000"));
            assert!(message.contains("internal failure"));
            assert!(message.contains("blame=server"));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

const CALLER_ERROR_BODY: &str =
    r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"invalid params"},"id":1}"#;

async fn caller_error_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    Ok(static_response(CALLER_ERROR_BODY, "application/json"))
}

#[tokio::test]
async fn s2_single_response_jsonrpc_caller_error_closes_with_blame_caller() {
    let router = Router::new().route("/", post(caller_error_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);
    let chunks = drain_tool_stream(&tool, "args").await;
    assert_eq!(chunks.len(), 1);
    match &chunks[0].finish_reason {
        Some(ToolFinishReason::Error { message }) => {
            assert!(message.contains("-32602"));
            assert!(message.contains("invalid params"));
            assert!(message.contains("blame=caller"));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

async fn unparseable_body_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    Ok(static_response("this is not JSON-RPC", "application/json"))
}

#[tokio::test]
async fn s2_unparseable_single_response_body_emits_error_terminator() {
    let router = Router::new().route("/", post(unparseable_body_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);
    let chunks = drain_tool_stream(&tool, "args").await;
    assert_eq!(chunks.len(), 1);
    match &chunks[0].finish_reason {
        Some(ToolFinishReason::Error { message }) => {
            assert!(
                message.contains("unparseable") || message.contains("JSON-RPC"),
                "got: {message}"
            );
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

// ════════════════════════════════════════════════════════════════════
//  §3 — Network surfaces × 2
// ════════════════════════════════════════════════════════════════════

async fn http_500_mcp_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    Ok(Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Body::from("MCP gateway error"))
        .expect("response"))
}

#[tokio::test]
async fn s3_non_2xx_status_emits_error_terminator_with_status_and_body() {
    let router = Router::new().route("/", post(http_500_mcp_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);
    let chunks = drain_tool_stream(&tool, "args").await;
    assert_eq!(chunks.len(), 1);
    match &chunks[0].finish_reason {
        Some(ToolFinishReason::Error { message }) => {
            assert!(message.contains("HTTP 500"));
            assert!(message.contains("MCP gateway error"));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[tokio::test]
async fn s3_connection_refused_emits_error_terminator() {
    let tool = McpStreamingTool::new(
        "Unreachable".to_string(),
        "http://127.0.0.1:1/mcp".to_string(),
        Duration::from_secs(1),
    );
    let chunks = drain_tool_stream(&tool, "args").await;
    assert_eq!(chunks.len(), 1);
    match &chunks[0].finish_reason {
        Some(ToolFinishReason::Error { message }) => {
            assert!(
                message.contains("cannot connect")
                    || message.contains("MCP request failed")
                    || message.contains("timed out"),
                "expected connect/timeout diagnostic, got {message}"
            );
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

// ════════════════════════════════════════════════════════════════════
//  §4 — Cancel propagation × 2
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s4_pre_cancel_emits_only_cancelled_terminator() {
    let router = Router::new().route("/", post(streaming_mcp_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);

    let cancel = CancellationFlag::new();
    cancel.cancel(); // Fire BEFORE invoking stream().
    let ctx = ToolContext::new(cancel, 0x42);
    let mut stream = tool.stream("args".to_string(), ctx).await;
    let mut chunks = Vec::new();
    while let Some(c) = stream.next().await {
        chunks.push(c);
    }
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].is_terminator());
    assert_eq!(chunks[0].finish_reason, Some(ToolFinishReason::Cancelled));
}

#[tokio::test]
async fn s4_cancel_mid_stream_yields_partial_chunks_plus_cancelled_terminator() {
    // Slow streaming MCP server (150ms between lines × 10 lines).
    // Read 2 chunks, fire cancel; remaining chunks NOT delivered +
    // stream terminates with `Cancelled`.
    fn slow_streaming_handler() -> Router {
        async fn handler(_req: Request) -> Response<Body> {
            let body_stream = futures::stream::unfold(0usize, |i| async move {
                if i >= 10 {
                    None
                } else {
                    let payload = format!(
                        "{{\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\
                         \"params\":{{\"data\":\"chunk-{i}\"}}}}\n"
                    );
                    let yielded = Ok::<_, Infallible>(Bytes::from(payload));
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    Some((yielded, i + 1))
                }
            });
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/x-ndjson")
                .body(Body::from_stream(body_stream))
                .expect("response")
        }
        Router::new().route("/", post(handler))
    }
    let base = spawn_test_server(slow_streaming_handler()).await;
    let tool = make_tool(&base);

    let cancel = CancellationFlag::new();
    let ctx = ToolContext::new(cancel.clone(), 0);
    let mut stream = tool.stream("args".to_string(), ctx).await;

    let mut collected = Vec::new();
    for _ in 0..2 {
        if let Some(c) = stream.next().await {
            collected.push(c);
        }
    }
    cancel.cancel();
    while let Some(c) = stream.next().await {
        collected.push(c);
    }

    assert!(collected.len() >= 2, "got {} chunks", collected.len());
    let last = collected.last().expect("non-empty");
    assert!(last.is_terminator(), "last MUST be terminator: {last:?}");
    assert_eq!(last.finish_reason, Some(ToolFinishReason::Cancelled));
    let intermediate_count =
        collected.iter().filter(|c| !c.is_terminator()).count();
    assert!(
        intermediate_count < 10,
        "cancel should short-circuit before all 10 chunks: got {intermediate_count}"
    );
}

// ════════════════════════════════════════════════════════════════════
//  §5 — End-to-end composition with dispatcher arm × 2
// ════════════════════════════════════════════════════════════════════

fn step_with_apply(name: &str, ask: &str, apply_ref: &str) -> IRStep {
    IRStep {
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
        apply_ref: apply_ref.into(),
        guards: Vec::new(),
        requires_context: None,        now_tz: None,        body: Vec::new(),
    }
}

fn registry_with_mcp(name: &str, runtime: &str, effect_row: Vec<&str>) -> Arc<ToolRegistry> {
    let mut reg = ToolRegistry::new();
    reg.register(ToolEntry {
        name: name.into(),
        provider: "mcp".into(),
        timeout: "5s".into(),
        runtime: runtime.into(),
        resource_ref: String::new(),
        capacity: None,
        sandbox: None,
        max_results: None,
        output_schema: String::new(),
        effect_row: effect_row.into_iter().map(String::from).collect(),
        parameters: Vec::new(),
        secret: String::new(),
        secret_partition: String::new(),
        source: ToolSource::Program,
        is_streaming: true,
        scrape: None,
    });
    Arc::new(reg)
}

fn fresh_dispatch_ctx(
    registry: Arc<ToolRegistry>,
) -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = DispatchCtx::new(
        "TestFlow",
        "stub",
        "you are a test agent",
        CancellationFlag::new(),
        tx,
    )
    .with_tool_registry(registry);
    (ctx, rx)
}

#[tokio::test]
async fn s5_end_to_end_streaming_mcp_emits_per_notification_step_token_events() {
    let router = Router::new().route("/", post(streaming_mcp_handler));
    let base = spawn_test_server(router).await;

    let registry = registry_with_mcp("McpStreamer", &base, vec!["stream:drop_oldest"]);
    let (mut ctx, mut rx) = fresh_dispatch_ctx(registry);
    let s = step_with_apply("McpFetch", "args", "McpStreamer");
    let outcome = run_step(&s, &mut ctx).await;
    assert!(matches!(outcome, Ok(NodeOutcome::Completed { .. })));

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    let step_tokens: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            FlowExecutionEvent::StepToken { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect();
    // partial-1, partial-2, 75% (progress), final answer
    assert_eq!(
        step_tokens,
        vec![
            "partial-1".to_string(),
            "partial-2".to_string(),
            "75%".to_string(),
            "final answer".to_string(),
        ]
    );
    assert!(matches!(events.first(), Some(FlowExecutionEvent::StepStart { .. })));
    assert!(matches!(events.last(), Some(FlowExecutionEvent::StepComplete { .. })));
}

#[tokio::test]
async fn s5_end_to_end_audit_row_captures_tokens_and_policy_for_mcp_stream() {
    let router = Router::new().route("/", post(streaming_mcp_handler));
    let base = spawn_test_server(router).await;

    let registry = registry_with_mcp("AuditedMcp", &base, vec!["stream:pause_upstream"]);
    let (mut ctx, _rx) = fresh_dispatch_ctx(registry);
    let s = step_with_apply("McpFetch", "args", "AuditedMcp");
    let _ = run_step(&s, &mut ctx).await;

    let audit = ctx.step_audit_records.lock().await.clone();
    assert_eq!(audit.len(), 1);
    let row = &audit[0];
    assert_eq!(row.tokens_emitted, 4, "4 notification/result deltas");
    assert!(row.success);
    assert_eq!(
        row.effect_policy_applied.as_deref(),
        Some("pause_upstream")
    );
    // 34.f honest scope: chunks_dropped/degraded stay 0 until 34.g.
    assert_eq!(row.chunks_dropped, 0);
    assert_eq!(row.chunks_degraded, 0);
}
