//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 34.e (v1.29.0) — End-to-end HTTP tool streaming tests.
//!
//! [`axon::http_tool::HttpStreamingTool`] is the first non-stub
//! provider with first-class streaming surface. This pack proves:
//!
//! 1. Framing classification by Content-Type is correct + each
//!    framing mode emits the expected `ToolChunk` sequence:
//!    - `text/event-stream` → per-SSE-event chunks (`data:` field)
//!    - `application/x-ndjson` / `application/jsonl` → per-line chunks
//!    - everything else → single accumulated body chunk (D9 backwards-
//!      compat for non-streaming HTTP endpoints)
//! 2. Every error surface (non-2xx, connect refused, body chunk
//!    error) produces a `ToolFinishReason::Error` terminator —
//!    never a panic, never a silent truncation.
//! 3. Cancellation (`ctx.cancel.cancel()`) propagates: pre-cancel
//!    yields a single `Cancelled` terminator; mid-stream cancel
//!    yields the chunks emitted before the fire + a `Cancelled`
//!    terminator.
//! 4. D9 backwards-compat: `execute()` (the synchronous path)
//!    behaves byte-equal to the legacy [`dispatch_http`].
//! 5. End-to-end composition with the dispatcher arm (Fase 34.d):
//!    streaming HTTP tools emit `StepToken` events on the wire for
//!    each upstream chunk.
//!
//! 18 tests across 5 sections.

#![allow(clippy::needless_return)]

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axon::axonendpoint_replay::StepAuditRecord;
use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::pure_shape::run_step;
use axon::flow_dispatcher::{DispatchCtx, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::http_tool::HttpStreamingTool;
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
//  Test-server scaffolding (same pattern as fase33_d_native_backend_streaming.rs)
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

fn make_tool(url: &str) -> HttpStreamingTool {
    HttpStreamingTool::new(
        "TestTool".to_string(),
        url.to_string(),
        Duration::from_secs(5),
    )
}

fn make_ctx() -> ToolContext {
    ToolContext::new(CancellationFlag::new(), 0x42)
}

async fn drain_tool_stream(tool: &HttpStreamingTool, args: &str) -> Vec<ToolChunk> {
    let ctx = make_ctx();
    let mut s = tool.stream(args.to_string(), ctx).await;
    let mut out = Vec::new();
    while let Some(c) = s.next().await {
        out.push(c);
    }
    out
}

async fn drain_tool_stream_with_cancel(
    tool: &HttpStreamingTool,
    args: &str,
    cancel: CancellationFlag,
) -> Vec<ToolChunk> {
    let ctx = ToolContext::new(cancel, 0x42);
    let mut s = tool.stream(args.to_string(), ctx).await;
    let mut out = Vec::new();
    while let Some(c) = s.next().await {
        out.push(c);
    }
    out
}

// ════════════════════════════════════════════════════════════════════
//  §1 — Framing classification × 5
// ════════════════════════════════════════════════════════════════════

const SSE_BODY: &str = concat!(
    "data: hello\n\n",
    "data: world\n\n",
    "data: !\n\n",
);

async fn sse_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    Ok(static_response(SSE_BODY, "text/event-stream"))
}

#[tokio::test]
async fn s1_sse_upstream_emits_per_event_tool_chunks() {
    let router = Router::new().route("/", post(sse_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);

    let chunks = drain_tool_stream(&tool, "args").await;
    // Expect 3 intermediate (one per `data:` event) + 1 Stop terminator.
    assert_eq!(
        chunks.len(),
        4,
        "SSE 3 events → 3 chunks + terminator, got: {chunks:#?}"
    );
    assert_eq!(chunks[0].delta, "hello");
    assert_eq!(chunks[1].delta, "world");
    assert_eq!(chunks[2].delta, "!");
    assert!(chunks[3].is_terminator());
    assert_eq!(chunks[3].finish_reason, Some(ToolFinishReason::Stop));
}

const NDJSON_BODY: &str = concat!(
    "{\"i\":1,\"text\":\"alpha\"}\n",
    "{\"i\":2,\"text\":\"beta\"}\n",
    "{\"i\":3,\"text\":\"gamma\"}\n",
);

async fn ndjson_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    Ok(static_response(NDJSON_BODY, "application/x-ndjson"))
}

#[tokio::test]
async fn s1_ndjson_upstream_emits_per_line_tool_chunks() {
    let router = Router::new().route("/", post(ndjson_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);

    let chunks = drain_tool_stream(&tool, "args").await;
    assert_eq!(chunks.len(), 4, "3 lines + terminator: {chunks:#?}");
    assert_eq!(chunks[0].delta, "{\"i\":1,\"text\":\"alpha\"}");
    assert_eq!(chunks[1].delta, "{\"i\":2,\"text\":\"beta\"}");
    assert_eq!(chunks[2].delta, "{\"i\":3,\"text\":\"gamma\"}");
    assert!(chunks[3].is_terminator());
    assert_eq!(chunks[3].finish_reason, Some(ToolFinishReason::Stop));
}

async fn jsonl_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    Ok(static_response(NDJSON_BODY, "application/jsonl"))
}

#[tokio::test]
async fn s1_jsonl_content_type_classified_as_ndjson() {
    // `application/jsonl` is the alternative MIME for NDJSON; the
    // classifier MUST treat both identically (per IANA precedent).
    let router = Router::new().route("/", post(jsonl_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);
    let chunks = drain_tool_stream(&tool, "args").await;
    assert_eq!(chunks.len(), 4);
    assert_eq!(chunks[2].delta, "{\"i\":3,\"text\":\"gamma\"}");
}

const JSON_BODY: &str = r#"{"result": "ok", "count": 42}"#;

async fn json_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    Ok(static_response(JSON_BODY, "application/json"))
}

#[tokio::test]
async fn s1_json_upstream_emits_single_accumulated_chunk_d9() {
    // D9 backwards-compat: when upstream is NOT streaming (regular
    // application/json), HttpStreamingTool accumulates the full
    // body + emits 1 ToolChunk (mirrors dispatch_http output).
    let router = Router::new().route("/", post(json_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);
    let chunks = drain_tool_stream(&tool, "args").await;
    assert_eq!(chunks.len(), 2, "single body + terminator: {chunks:#?}");
    assert_eq!(chunks[0].delta, JSON_BODY);
    assert!(chunks[1].is_terminator());
    assert_eq!(chunks[1].finish_reason, Some(ToolFinishReason::Stop));
}

async fn text_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    Ok(static_response("hello plain world", "text/plain"))
}

#[tokio::test]
async fn s1_text_plain_upstream_emits_single_accumulated_chunk() {
    let router = Router::new().route("/", post(text_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);
    let chunks = drain_tool_stream(&tool, "args").await;
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].delta, "hello plain world");
    assert!(chunks[1].is_terminator());
}

// ════════════════════════════════════════════════════════════════════
//  §2 — Error surfaces × 5
// ════════════════════════════════════════════════════════════════════

async fn http_500_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    Ok(Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Body::from("internal server error"))
        .expect("response"))
}

#[tokio::test]
async fn s2_non_2xx_500_emits_error_terminator_with_status_code() {
    let router = Router::new().route("/", post(http_500_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);

    let chunks = drain_tool_stream(&tool, "args").await;
    assert_eq!(chunks.len(), 1);
    match &chunks[0].finish_reason {
        Some(ToolFinishReason::Error { message }) => {
            assert!(message.contains("HTTP 500"), "got {message}");
            assert!(message.contains("internal server error"), "got {message}");
        }
        other => panic!("expected Error finish_reason, got {other:?}"),
    }
}

async fn http_404_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("not found"))
        .expect("response"))
}

#[tokio::test]
async fn s2_non_2xx_404_emits_error_terminator_with_body_excerpt() {
    let router = Router::new().route("/", post(http_404_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);
    let chunks = drain_tool_stream(&tool, "args").await;
    assert_eq!(chunks.len(), 1);
    match &chunks[0].finish_reason {
        Some(ToolFinishReason::Error { message }) => {
            assert!(message.contains("HTTP 404"));
            assert!(message.contains("not found"));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

async fn http_429_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    // 200-char body to verify the 200-char truncation discipline
    // (mirrors dispatch_http behavior).
    let body = "x".repeat(300);
    Ok(Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .body(Body::from(body))
        .expect("response"))
}

#[tokio::test]
async fn s2_non_2xx_429_emits_error_with_truncated_body() {
    let router = Router::new().route("/", post(http_429_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);
    let chunks = drain_tool_stream(&tool, "args").await;
    assert_eq!(chunks.len(), 1);
    match &chunks[0].finish_reason {
        Some(ToolFinishReason::Error { message }) => {
            assert!(message.contains("HTTP 429"));
            assert!(
                message.contains("..."),
                "300-char body should be truncated with ellipsis: {message}"
            );
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[tokio::test]
async fn s2_connection_refused_emits_error_terminator() {
    // Port 1 is conventionally closed.
    let tool = HttpStreamingTool::new(
        "Unreachable".to_string(),
        "http://127.0.0.1:1/api".to_string(),
        Duration::from_secs(1),
    );
    let chunks = drain_tool_stream(&tool, "args").await;
    assert_eq!(chunks.len(), 1);
    match &chunks[0].finish_reason {
        Some(ToolFinishReason::Error { message }) => {
            assert!(
                message.contains("connection failed")
                    || message.contains("request failed")
                    || message.contains("timed out"),
                "expected connection/timeout diagnostic, got {message}"
            );
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

async fn empty_200_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    Ok(static_response("", "application/json"))
}

#[tokio::test]
async fn s2_empty_body_200_emits_only_stop_terminator_no_intermediate() {
    // Empty 200 response — natural stream end. Single-framing mode
    // skips empty-body emission + emits the Stop terminator directly.
    let router = Router::new().route("/", post(empty_200_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);
    let chunks = drain_tool_stream(&tool, "args").await;
    assert_eq!(chunks.len(), 1, "empty body → terminator only: {chunks:#?}");
    assert!(chunks[0].is_terminator());
    assert_eq!(chunks[0].finish_reason, Some(ToolFinishReason::Stop));
}

// ════════════════════════════════════════════════════════════════════
//  §3 — Cancel propagation × 3
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s3_pre_cancel_emits_only_cancelled_terminator() {
    let router = Router::new().route("/", post(sse_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);

    let cancel = CancellationFlag::new();
    cancel.cancel(); // Fire BEFORE invoking stream().
    let chunks = drain_tool_stream_with_cancel(&tool, "args", cancel).await;
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].is_terminator());
    assert_eq!(chunks[0].finish_reason, Some(ToolFinishReason::Cancelled));
}

/// Slow streaming handler — emits N NDJSON lines with `delay_ms`
/// between chunks. Used by the cancel-mid-stream tests so the
/// dispatcher can observe + cancel between chunks.
///
/// Built on `futures::stream::unfold` since `async-stream` isn't a
/// workspace dep + `axum::body::Body::from_stream` accepts any
/// `Stream<Item = Result<Bytes, _>>`.
fn slow_ndjson_handler(lines: Vec<String>, delay_ms: u64) -> Router {
    async fn handler(
        axum::extract::State(state): axum::extract::State<Arc<(Vec<String>, u64)>>,
        _req: Request,
    ) -> Response<Body> {
        let lines = state.0.clone();
        let delay_ms = state.1;
        let body_stream =
            futures::stream::unfold((lines.into_iter(), delay_ms), move |(mut it, d)| async move {
                match it.next() {
                    None => None,
                    Some(line) => {
                        let mut payload = line;
                        payload.push('\n');
                        let yielded =
                            Ok::<_, Infallible>(Bytes::from(payload));
                        // Sleep AFTER yielding so the consumer gets
                        // the current chunk before the next-chunk
                        // delay kicks in.
                        tokio::time::sleep(Duration::from_millis(d)).await;
                        Some((yielded, (it, d)))
                    }
                }
            });
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/x-ndjson")
            .body(Body::from_stream(body_stream))
            .expect("response")
    }
    Router::new()
        .route("/", post(handler))
        .with_state(Arc::new((lines, delay_ms)))
}

#[tokio::test]
async fn s3_cancel_mid_stream_ndjson_yields_partial_chunks_plus_cancelled_terminator() {
    // Set up a slow NDJSON server (200ms between lines × 10 lines).
    // Start the tool stream, consume 2 chunks, fire cancel; the
    // remaining chunks are NOT delivered + the stream terminates
    // with `Cancelled`.
    let lines: Vec<String> = (0..10).map(|i| format!("{{\"i\":{i}}}")).collect();
    let router = slow_ndjson_handler(lines, 150);
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);

    let cancel = CancellationFlag::new();
    let ctx = ToolContext::new(cancel.clone(), 0);
    let mut stream = tool.stream("args".to_string(), ctx).await;

    let mut collected = Vec::new();
    // Read 2 chunks.
    for _ in 0..2 {
        if let Some(c) = stream.next().await {
            collected.push(c);
        }
    }
    // Fire cancel.
    cancel.cancel();
    // Drain the rest.
    while let Some(c) = stream.next().await {
        collected.push(c);
    }

    // We expect at least 2 intermediate chunks (the 2 we explicitly
    // read) + a Cancelled terminator. The exact count of pre-cancel
    // intermediate chunks varies by scheduling — assert the
    // invariants robustly.
    assert!(collected.len() >= 2, "got {} chunks", collected.len());
    let last = collected.last().expect("non-empty");
    assert!(last.is_terminator(), "last chunk must be terminator: {last:?}");
    assert_eq!(last.finish_reason, Some(ToolFinishReason::Cancelled));
    // We MUST NOT have emitted all 10 lines (cancel should short-circuit).
    let intermediate_count =
        collected.iter().filter(|c| !c.is_terminator()).count();
    assert!(
        intermediate_count < 10,
        "cancel should short-circuit before all 10 lines: got {intermediate_count}"
    );
}

#[tokio::test]
async fn s3_cancel_mid_stream_sse_yields_partial_chunks_plus_cancelled_terminator() {
    // Same shape as the NDJSON case, but with SSE framing.
    async fn handler(_req: Request) -> Response<Body> {
        let body_stream = futures::stream::unfold(0usize, |i| async move {
            if i >= 10 {
                None
            } else {
                let payload = format!("data: chunk-{i}\n\n");
                let yielded = Ok::<_, Infallible>(Bytes::from(payload));
                tokio::time::sleep(Duration::from_millis(150)).await;
                Some((yielded, i + 1))
            }
        });
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(body_stream))
            .expect("response")
    }
    let router = Router::new().route("/", post(handler));
    let base = spawn_test_server(router).await;
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

    assert!(collected.len() >= 2);
    let last = collected.last().expect("non-empty");
    assert!(last.is_terminator());
    assert_eq!(last.finish_reason, Some(ToolFinishReason::Cancelled));
    let intermediate_count =
        collected.iter().filter(|c| !c.is_terminator()).count();
    assert!(intermediate_count < 10);
}

// ════════════════════════════════════════════════════════════════════
//  §4 — D9 + composition × 2
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s4_execute_synchronous_path_preserves_legacy_dispatch_http() {
    // D9 backwards-compat: HttpStreamingTool.execute() is the
    // synchronous path; adopters calling Tool::execute() directly
    // (without going through stream()) get the legacy [`dispatch_http`]
    // behavior byte-equal.
    let router = Router::new().route("/", post(json_handler));
    let base = spawn_test_server(router).await;
    let tool = make_tool(&base);
    let result = tool.execute("input".to_string(), make_ctx()).await;
    assert!(result.success);
    assert_eq!(result.output, JSON_BODY);
    assert_eq!(result.tool_name, "TestTool");
}

#[tokio::test]
async fn s4_from_entry_rejects_invalid_url_scheme() {
    let bad = ToolEntry {
        name: "Bad".into(),
        provider: "http".into(),
        timeout: "10s".into(),
        runtime: "ftp://example.com/".into(),
        resource_ref: String::new(),
        capacity: None, // wrong scheme
        sandbox: None,
        max_results: None,
        output_schema: String::new(),
        effect_row: vec!["stream:drop_oldest".into()],
        parameters: Vec::new(),
        secret: String::new(),
        secret_partition: String::new(),
        source: ToolSource::Program,
        is_streaming: true,
        scrape: None,
    };
    let result = HttpStreamingTool::from_entry(&bad);
    assert!(result.is_err());
    let msg = result.err().unwrap();
    assert!(msg.contains("invalid URL"));
}

// ════════════════════════════════════════════════════════════════════
//  §5 — End-to-end composition with the dispatcher arm (Fase 34.d) × 3
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

fn registry_with_http(name: &str, runtime: &str, effect_row: Vec<&str>) -> Arc<ToolRegistry> {
    let mut reg = ToolRegistry::new();
    reg.register(ToolEntry {
        name: name.into(),
        provider: "http".into(),
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
async fn s5_end_to_end_sse_upstream_emits_per_chunk_step_token_events() {
    // E2E: register an http tool that points at an SSE mock; drive
    // the dispatcher's streaming arm; verify each upstream SSE event
    // produces a StepToken event on the wire.
    let router = Router::new().route("/", post(sse_handler));
    let base = spawn_test_server(router).await;

    let registry = registry_with_http("SseTool", &base, vec!["stream:drop_oldest"]);
    let (mut ctx, mut rx) = fresh_dispatch_ctx(registry);
    let s = step_with_apply("Fetch", "args", "SseTool");
    let outcome = run_step(&s, &mut ctx).await;
    assert!(matches!(outcome, Ok(NodeOutcome::Completed { .. })));

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    // StepStart + 3× StepToken (SSE events: hello/world/!) + StepComplete = 5
    let step_tokens: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            FlowExecutionEvent::StepToken { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(step_tokens, vec!["hello", "world", "!"]);
    assert!(matches!(events.first(), Some(FlowExecutionEvent::StepStart { .. })));
    assert!(matches!(events.last(), Some(FlowExecutionEvent::StepComplete { .. })));
}

#[tokio::test]
async fn s5_end_to_end_ndjson_upstream_emits_per_line_step_token_events() {
    let router = Router::new().route("/", post(ndjson_handler));
    let base = spawn_test_server(router).await;

    let registry = registry_with_http("NdjsonTool", &base, vec!["stream:pause_upstream"]);
    let (mut ctx, mut rx) = fresh_dispatch_ctx(registry);
    let s = step_with_apply("Fetch", "args", "NdjsonTool");
    let _ = run_step(&s, &mut ctx).await;

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
    assert_eq!(step_tokens.len(), 3);
    assert!(step_tokens[0].contains("alpha"));
    assert!(step_tokens[1].contains("beta"));
    assert!(step_tokens[2].contains("gamma"));
}

async fn drain_audit_records(ctx: &DispatchCtx) -> Vec<StepAuditRecord> {
    let g = ctx.step_audit_records.lock().await;
    g.clone()
}

#[tokio::test]
async fn s5_end_to_end_audit_row_captures_tokens_and_policy_for_http_stream() {
    let router = Router::new().route("/", post(sse_handler));
    let base = spawn_test_server(router).await;

    let registry = registry_with_http("AuditedHttp", &base, vec!["stream:drop_oldest"]);
    let (mut ctx, _rx) = fresh_dispatch_ctx(registry);
    let s = step_with_apply("Fetch", "args", "AuditedHttp");
    let _ = run_step(&s, &mut ctx).await;

    let audit = drain_audit_records(&ctx).await;
    assert_eq!(audit.len(), 1);
    let row = &audit[0];
    assert_eq!(row.tokens_emitted, 3, "3 SSE events");
    assert!(row.success);
    assert_eq!(row.effect_policy_applied.as_deref(), Some("drop_oldest"));
    // 34.e honest scope: chunks_dropped/degraded stay 0 until 34.g.
    assert_eq!(row.chunks_dropped, 0);
    assert_eq!(row.chunks_degraded, 0);
}
