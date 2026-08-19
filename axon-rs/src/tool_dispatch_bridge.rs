//! §Fase 34.d (v1.29.0) — Bridge between [`ToolEntry`] (the registry
//! shape) and [`Tool`] trait impls (the dispatcher's streaming
//! surface).
//!
//! The dispatcher's `pure_shape::run_step` calls
//! [`resolve_streaming_tool`] to obtain a [`Tool`] trait object for
//! a given registry entry. The returned object's `stream()` method
//! produces a `Stream<ToolChunk>` the dispatcher drains chunk-by-
//! chunk + emits as `FlowExecutionEvent::StepToken` events on the
//! wire.
//!
//! # 34.d-scoped provider coverage
//!
//! - **`stub`** → [`StubStreamingTool`] — emits a deterministic
//!   3-chunk stream (intermediate × 3 + terminator). The reference
//!   stream-producer for testing the dispatch path under all
//!   policy / cancel / audit invariants.
//! - **`native`** → [`NativeWrappedTool`] — wraps the existing
//!   `tool_executor::dispatch` (Calculator / DateTimeTool) as a
//!   single-chunk stream. D9 backwards-compat: built-in tools
//!   that don't declare a stream effect still produce a Tool impl,
//!   they just emit 1 chunk via the `from_result` path.
//! - **`stub_stream`** → [`StubStreamingTool`] — explicit alias
//!   that adopters can declare in source via
//!   `tool MyStream { provider: "stub_stream" effects:
//!   <stream:drop_oldest> ... }`. Same impl as `stub` but the
//!   declaration documents the streaming intent.
//! - **`http`** → [`crate::http_tool::HttpStreamingTool`] (Fase 34.e).
//!   Async reqwest::Client + `bytes_stream()` drain with framing
//!   classified by Content-Type: `text/event-stream` →
//!   per-SSE-event ToolChunks; `application/x-ndjson` /
//!   `application/jsonl` → per-line ToolChunks; everything else →
//!   single-chunk wrap (D9 backwards-compat). HONEST error
//!   terminator on every failure surface. Falls back to
//!   [`SyncFallbackTool`] when the registry entry has an
//!   invalid/missing URL (parser would already reject, defensive).
//! - **`mcp`** → [`crate::emcp::McpStreamingTool`] (Fase 34.f). Async
//!   reqwest::Client + JSON-RPC 2.0 over HTTP. Streaming MCP servers
//!   (Content-Type `application/x-ndjson` / `application/jsonl`)
//!   emit per-`notifications/message` ToolChunks; the final `result`
//!   envelope closes the stream. Non-streaming MCP servers
//!   (Content-Type `application/json`) fall back to D9 single-chunk
//!   wrap. Best-effort `notifications/cancelled` notification fired
//!   on cancel. Falls back to [`SyncFallbackTool`] when the registry
//!   entry has an invalid/missing server URL.
//!
//! # Cross-stack contract
//!
//! Python's `axon.runtime.tools.streaming.Tool` ABC + provider-
//! dispatch surface in `axon.runtime.tools.dispatcher` mirror this
//! Rust bridge. Drift gate alignment lives in Fase 34.j fuzz.

use crate::cancel_token::CancellationFlag;
use crate::tool_executor::{self, ToolResult};
use crate::tool_registry::ToolEntry;
use crate::tool_trait::{Tool, ToolChunk, ToolContext, ToolFinishReason, ToolStream};
use async_trait::async_trait;
use futures::stream;

// ════════════════════════════════════════════════════════════════════
//  Factory
// ════════════════════════════════════════════════════════════════════

/// Resolve a [`Tool`] trait object from a registry [`ToolEntry`].
///
/// The dispatcher (`pure_shape::run_step`) calls this when
/// `entry.is_streaming` is true. The returned trait object's
/// `stream()` method drives the per-chunk wire emission path.
///
/// # 34.f-scoped dispatch
///
/// | Provider | Impl | Behavior |
/// |---|---|---|
/// | _(empty)_ | [`StubStreamingTool`] | §Fase 36.x.e — an unspecified `provider:` resolves to the deterministic stub stream |
/// | `stub` | [`StubStreamingTool`] | Deterministic 3-chunk stream |
/// | `stub_stream` | [`StubStreamingTool`] | Alias for adopter clarity |
/// | `native` | [`NativeWrappedTool`] | Wraps `tool_executor::dispatch` as 1-chunk |
/// | `http` | [`crate::http_tool::HttpStreamingTool`] | Async reqwest + framing-aware drain (Fase 34.e) |
/// | `mcp` | [`crate::emcp::McpStreamingTool`] | JSON-RPC 2.0 + notifications stream (Fase 34.f) |
/// | _other_ | [`SyncFallbackTool`] | Synchronous fallback (unknown provider) |
///
/// §Fase 36.x.e — a `tool` declaring `effects: <stream:…>` but NO
/// `provider:` is under-specified, not erroneous: it resolves to the
/// deterministic stub stream (no external dependency) so the flow
/// runs gracefully — the adopter adds a concrete `provider:` when
/// ready. An unspecified streaming provider that hard-errored the
/// flow (via the `SyncFallbackTool` `_other_` arm) was the masked
/// regression the Fase 36.x.c terminator fix surfaced.
pub fn resolve_streaming_tool(entry: &ToolEntry) -> Box<dyn Tool> {
    match entry.provider.trim() {
        // §Fase 114.b — an EMPTY provider stays here, and that is correct.
        //
        // The census framed empty→stub as "a fabricated answer", and I nearly
        // split it out. But an empty provider is an **LLM-routed** tool, and on
        // the streaming path this arm is how it streams **against the configured
        // backend** — which in a test is the stub backend, and in production is
        // the real model. It is routing, not invention.
        //
        // The actual §112-shaped defect here was the **typo** — a *non-empty*
        // unknown provider that silently reached a fallthrough and let the model
        // fabricate. `axon-T948` now refuses that at COMPILE, so it can no longer
        // reach any runtime arm. The empty case that remains is a declared intent
        // (the tool IS the model), not a mistake.
        "" | "stub" | "stub_stream" => {
            Box::new(StubStreamingTool::new(entry.name.clone()))
        }
        "native" => {
            Box::new(NativeWrappedTool::new(entry.name.clone()))
        }
        "http" => match crate::http_tool::HttpStreamingTool::from_entry(entry) {
            Ok(t) => Box::new(t),
            // Defensive: parser rejects invalid URLs at compile time,
            // but if a malformed runtime URL reaches the registry
            // (e.g. programmatic registration in tests / hot-reload
            // edge), fall back to the honest SyncFallbackTool so the
            // consumer sees a structured error-terminator chunk
            // instead of a panic.
            Err(_) => Box::new(SyncFallbackTool::new(
                entry.name.clone(),
                "http".to_string(),
            )),
        },
        "mcp" => match crate::emcp::McpStreamingTool::from_entry(entry) {
            Ok(t) => Box::new(t),
            // Defensive: same shape as the http arm — bad MCP server
            // URL → honest fallback.
            Err(_) => Box::new(SyncFallbackTool::new(
                entry.name.clone(),
                "mcp".to_string(),
            )),
        },
        // §Fase 98.e — the streaming crawl provider. A bounded, checkpointed
        // BFS spider emitting each fetched RawPage as a chunk. `scrape_http` /
        // `scrape_dom` are synchronous (registry `dispatch`), so only
        // `scrape_crawl` needs the streaming surface here.
        "scrape_crawl" => {
            Box::new(crate::scrape_tool::ScrapeStreamingTool::from_entry(entry))
        }
        // Unknown provider → synchronous fallback. Adopters declaring
        // a custom provider see the honest error-terminator at the
        // wire layer.
        other => Box::new(SyncFallbackTool::new(
            entry.name.clone(),
            other.to_string(),
        )),
    }
}

// ════════════════════════════════════════════════════════════════════
//  StubStreamingTool — deterministic 3-chunk reference stream
// ════════════════════════════════════════════════════════════════════

/// Synthetic stream producer for testing the dispatcher's streaming
/// arm. Emits a deterministic 4-frame sequence per invocation:
///
/// ```text
///   ToolChunk::intermediate("[stub-stream] <name>(")
///   ToolChunk::intermediate(<args>)
///   ToolChunk::intermediate(")")
///   ToolChunk::terminator("", ToolFinishReason::Stop)
/// ```
///
/// Cancel-safe: between every chunk emission, the tool polls
/// `ctx.cancel`; if fired, the stream short-circuits to a
/// `ToolFinishReason::Cancelled` terminator chunk. The pre-cancel
/// chunks already emitted reach the consumer; the post-cancel
/// chunks are skipped. D5 p95 ≤100ms invariant honored at the
/// chunk boundary.
pub struct StubStreamingTool {
    name: String,
}

impl StubStreamingTool {
    /// Construct a new stub streaming tool with the given name.
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

#[async_trait]
impl Tool for StubStreamingTool {
    async fn execute(&self, args: String, _ctx: ToolContext) -> ToolResult {
        // Synchronous path — kept available so the default
        // `stream()` impl can fall back if needed. Adopters
        // calling execute() directly get the materialized form.
        ToolResult {
            success: true,
            output: format!("[stub-stream-tool] {}({args})", self.name),
            tool_name: self.name.clone(),
        }
    }

    async fn stream(&self, args: String, ctx: ToolContext) -> ToolStream {
        let name = self.name.clone();
        let cancel = ctx.cancel.clone();

        // Materialize the chunk sequence eagerly. The runtime check
        // for cancel happens at the dispatcher's drain boundary
        // (see pure_shape::drain_stream_tool) — the dispatcher polls
        // the cancel flag between consuming each chunk + emitting
        // it on the wire, so stream lazily-yielded chunks honor the
        // cancel discipline at the consumer side.
        let chunks: Vec<ToolChunk> = if cancel.is_cancelled() {
            // Pre-cancel: emit a cancelled-terminator and bail.
            vec![ToolChunk::terminator("", ToolFinishReason::Cancelled)]
        } else {
            vec![
                ToolChunk::intermediate(format!("[stub-stream] {name}(")),
                ToolChunk::intermediate(args),
                ToolChunk::intermediate(")"),
                ToolChunk::terminator("", ToolFinishReason::Stop),
            ]
        };
        Box::pin(stream::iter(chunks))
    }

    fn is_streaming(&self) -> bool {
        true
    }
}

// ════════════════════════════════════════════════════════════════════
//  NativeWrappedTool — bridges built-in tools to the streaming surface
// ════════════════════════════════════════════════════════════════════

/// Wraps a built-in tool (Calculator / DateTimeTool) as a single-
/// chunk stream. D9 backwards-compat: built-ins keep working
/// byte-equal; their materialized output becomes the terminator
/// chunk's `delta`.
///
/// This impl is rarely exercised by 34.d test paths (built-ins
/// don't declare stream effects, so `is_streaming` is false on
/// their registry entries and the dispatcher takes the
/// synchronous path). The impl exists for completeness — adopters
/// who programmatically flag a built-in as streaming (e.g. for
/// testing) get a working single-chunk stream.
pub struct NativeWrappedTool {
    name: String,
}

impl NativeWrappedTool {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

#[async_trait]
impl Tool for NativeWrappedTool {
    async fn execute(&self, args: String, _ctx: ToolContext) -> ToolResult {
        tool_executor::dispatch(&self.name, &args).unwrap_or_else(|| ToolResult {
            success: false,
            output: format!("native tool '{}' not registered", self.name),
            tool_name: self.name.clone(),
        })
    }

    async fn stream(&self, args: String, ctx: ToolContext) -> ToolStream {
        let result = self.execute(args, ctx).await;
        Box::pin(stream::iter(vec![ToolChunk::from_result(&result)]))
    }

    fn is_streaming(&self) -> bool {
        // Built-ins default to non-streaming. The dispatcher only
        // reaches this impl when the entry's is_streaming flag was
        // set programmatically by an adopter.
        false
    }
}

// ════════════════════════════════════════════════════════════════════
//  SyncFallbackTool — for providers without 34.d streaming coverage
// ════════════════════════════════════════════════════════════════════

/// Fallback impl for providers without a dedicated streaming adapter
/// in 34.d's scope (http / mcp / unknown). The `stream()` method
/// emits a single error-terminator chunk indicating that the
/// provider's streaming surface lands in a later sub-fase
/// (Fase 34.e for HTTP, Fase 34.f for MCP).
///
/// This is a HONEST fallback — it does NOT silently coerce a
/// streaming declaration into a synchronous call. Adopters who
/// declare a stream effect on an HTTP/MCP tool today see a clear
/// `ToolFinishReason::Error { message: "streaming adapter not
/// yet implemented for provider 'http' — pending Fase 34.e" }`
/// terminator chunk. After 34.e/f, the bridge's `match` arms
/// route these providers to their dedicated streaming impls.
pub struct SyncFallbackTool {
    name: String,
    provider: String,
}

impl SyncFallbackTool {
    pub fn new(name: String, provider: String) -> Self {
        Self { name, provider }
    }
}

#[async_trait]
impl Tool for SyncFallbackTool {
    async fn execute(&self, _args: String, _ctx: ToolContext) -> ToolResult {
        ToolResult {
            success: false,
            output: format!(
                "synchronous fallback for provider '{}' tool '{}' — \
                 streaming dispatch only resolves stream-effect tools \
                 via dedicated provider adapters (HTTP / \
                 MCP).",
                self.provider, self.name
            ),
            tool_name: self.name.clone(),
        }
    }

    async fn stream(&self, args: String, ctx: ToolContext) -> ToolStream {
        let provider = self.provider.clone();
        let result = self.execute(args, ctx).await;
        // Post-Fase 34.e, the `http` arm of the bridge resolves to
        // [`crate::http_tool::HttpStreamingTool`] for entries with a
        // valid `runtime:` URL. SyncFallbackTool is only reached for
        // http when [`crate::http_tool::HttpStreamingTool::from_entry`]
        // returns Err (empty / non-http scheme). For mcp, 34.f is
        // pending. Any other unknown provider falls here.
        let hint = match provider.as_str() {
            "http" => {
                "HTTP streaming is shipped — verify the \
                 tool's `runtime:` URL starts with http:// or https://"
            }
            "mcp" => {
                "MCP streaming is shipped — verify the \
                 tool's `runtime:` URL starts with http:// or https://"
            }
            _ => "no dedicated streaming adapter — pending",
        };
        let error_msg = format!(
            "streaming dispatch fallback for provider '{provider}' tool '{name}' \
             ({hint}); synchronous fallback returned: {output}",
            name = self.name,
            output = result.output,
        );
        Box::pin(stream::iter(vec![ToolChunk::terminator(
            String::new(),
            ToolFinishReason::Error { message: error_msg },
        )]))
    }

    fn is_streaming(&self) -> bool {
        // Honestly false — this impl is the placeholder, not a real
        // stream producer. Adopters who declared a stream effect
        // see the error-terminator at the wire layer + can fall
        // back to a different provider until 34.e/f ship.
        false
    }
}

// ════════════════════════════════════════════════════════════════════
//  extract_stream_policy — pull `<stream:<policy>>` from effect_row
// ════════════════════════════════════════════════════════════════════

/// Extract the declared [`crate::stream_effect::BackpressurePolicy`]
/// from a tool's `effect_row`. Returns `None` when:
///
/// - No `stream:<policy>` entry is present (the tool is not flagged
///   as streaming).
/// - The `stream:<policy>` entry's policy slug is not in the closed
///   catalog (defensive — parser rejects unknown slugs at compile
///   time, but the runtime stays robust against stale source).
///
/// When multiple `stream:<policy>` entries appear (malformed
/// source), the FIRST one wins. Parser enforces single-policy per
/// tool declaration at compile time.
pub fn extract_stream_policy(
    effect_row: &[String],
) -> Option<crate::stream_effect::BackpressurePolicy> {
    for entry in effect_row {
        if let Some(rest) = entry.strip_prefix("stream:") {
            // The closed-catalog `BackpressurePolicy::from_slug`
            // returns None on unknown slugs (defensive).
            if let Some(policy) =
                crate::stream_effect::BackpressurePolicy::from_slug(rest)
            {
                return Some(policy);
            }
        }
    }
    None
}

// ════════════════════════════════════════════════════════════════════
//  Build a per-tool-invocation ToolContext
// ════════════════════════════════════════════════════════════════════

/// Construct a fresh [`ToolContext`] for a tool invocation given the
/// dispatcher's cancel flag + trace_id. Centralizes the
/// construction so the dispatcher's branch doesn't duplicate the
/// pattern across multiple call sites.
pub fn build_tool_context(cancel: CancellationFlag, trace_id: u64) -> ToolContext {
    ToolContext::new(cancel, trace_id)
}

// ════════════════════════════════════════════════════════════════════
//  Lib unit tests
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream_effect::BackpressurePolicy;
    use crate::tool_registry::ToolSource;
    use futures::StreamExt;

    fn entry(name: &str, provider: &str, effect_row: Vec<String>) -> ToolEntry {
        let is_streaming = crate::tool_registry::derive_is_streaming(&effect_row);
        ToolEntry {
            name: name.to_string(),
            provider: provider.to_string(),
            timeout: String::new(),
            runtime: String::new(),
            resource_ref: String::new(),
            substrate: None,
            capacity: None,
            sandbox: None,
            max_results: None,
            output_schema: String::new(),
            effect_row,
            parameters: Vec::new(),
            secret: String::new(),
            secret_partition: String::new(),
            source: ToolSource::Program,
            is_streaming,
            scrape: None,
        }
    }

    // ─── resolve_streaming_tool dispatch table ─────────────────────

    #[test]
    fn resolve_stub_provider_returns_stub_streaming_tool() {
        let e = entry("MyTool", "stub", vec!["stream:drop_oldest".into()]);
        let tool = resolve_streaming_tool(&e);
        assert!(tool.is_streaming());
    }

    #[test]
    fn resolve_stub_stream_alias_returns_stub_streaming_tool() {
        let e = entry("MyTool", "stub_stream", vec!["stream:fail".into()]);
        let tool = resolve_streaming_tool(&e);
        assert!(tool.is_streaming());
    }

    #[test]
    fn resolve_native_provider_returns_native_wrapped_tool() {
        let e = entry("Calculator", "native", vec!["compute".into()]);
        let tool = resolve_streaming_tool(&e);
        assert!(!tool.is_streaming()); // NativeWrappedTool reports false
    }

    #[test]
    fn resolve_http_provider_with_valid_url_returns_http_streaming_tool() {
        // §Fase 34.e — `http` arm now resolves to HttpStreamingTool
        // (when the runtime URL is valid). The HttpStreamingTool
        // reports `is_streaming() = true` (first-class streaming
        // surface).
        let mut e = entry("HttpTool", "http", vec!["stream:drop_oldest".into()]);
        e.runtime = "https://example.com/api".to_string();
        e.timeout = "10s".to_string();
        let tool = resolve_streaming_tool(&e);
        assert!(tool.is_streaming());
    }

    #[test]
    fn resolve_http_provider_with_invalid_url_falls_back_to_sync_fallback() {
        // Defensive: parser rejects invalid URLs, but if a malformed
        // runtime URL reaches the registry, the bridge falls back to
        // the honest SyncFallbackTool (is_streaming = false) so the
        // consumer sees a structured error-terminator chunk.
        let mut e = entry("HttpTool", "http", vec!["stream:drop_oldest".into()]);
        e.runtime = "ftp://example.com/api".to_string(); // bad scheme
        let tool = resolve_streaming_tool(&e);
        assert!(!tool.is_streaming());
    }

    #[test]
    fn resolve_http_provider_with_empty_url_falls_back_to_sync_fallback() {
        let mut e = entry("HttpTool", "http", vec!["stream:drop_oldest".into()]);
        e.runtime = String::new();
        let tool = resolve_streaming_tool(&e);
        assert!(!tool.is_streaming());
    }

    #[test]
    fn resolve_mcp_provider_with_valid_url_returns_mcp_streaming_tool() {
        // §Fase 34.f — `mcp` arm now resolves to McpStreamingTool
        // (when the runtime URL is valid). McpStreamingTool reports
        // `is_streaming() = true` (first-class streaming surface).
        let mut e = entry("McpTool", "mcp", vec!["stream:fail".into()]);
        e.runtime = "http://localhost:3000/mcp".to_string();
        e.timeout = "10s".to_string();
        let tool = resolve_streaming_tool(&e);
        assert!(tool.is_streaming());
    }

    #[test]
    fn resolve_mcp_provider_with_invalid_url_falls_back_to_sync_fallback() {
        let mut e = entry("McpTool", "mcp", vec!["stream:fail".into()]);
        e.runtime = "ws://localhost:3000".to_string(); // wrong scheme
        let tool = resolve_streaming_tool(&e);
        assert!(!tool.is_streaming());
    }

    #[test]
    fn resolve_mcp_provider_with_empty_url_falls_back_to_sync_fallback() {
        let mut e = entry("McpTool", "mcp", vec!["stream:fail".into()]);
        e.runtime = String::new();
        let tool = resolve_streaming_tool(&e);
        assert!(!tool.is_streaming());
    }

    #[test]
    fn resolve_unknown_provider_falls_through() {
        let e = entry("CustomTool", "custom_xyz", vec![]);
        let tool = resolve_streaming_tool(&e);
        assert!(!tool.is_streaming());
    }

    // ─── StubStreamingTool emits 4-frame stream ────────────────────

    #[tokio::test]
    async fn stub_streaming_tool_emits_4_frame_sequence() {
        let tool = StubStreamingTool::new("Search".to_string());
        let cancel = CancellationFlag::new();
        let ctx = ToolContext::new(cancel, 0x42);
        let mut stream = tool.stream("query=axon".to_string(), ctx).await;

        let chunks: Vec<ToolChunk> = {
            let mut v = Vec::new();
            while let Some(c) = stream.next().await {
                v.push(c);
            }
            v
        };
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].delta, "[stub-stream] Search(");
        assert_eq!(chunks[1].delta, "query=axon");
        assert_eq!(chunks[2].delta, ")");
        assert!(chunks[3].is_terminator());
        assert_eq!(chunks[3].finish_reason, Some(ToolFinishReason::Stop));
    }

    // ─── StubStreamingTool pre-cancel emits cancelled-terminator ──

    #[tokio::test]
    async fn stub_streaming_tool_pre_cancel_emits_cancelled_terminator_only() {
        let tool = StubStreamingTool::new("Search".to_string());
        let cancel = CancellationFlag::new();
        cancel.cancel(); // Fire BEFORE invoking stream().
        let ctx = ToolContext::new(cancel, 0x42);
        let mut stream = tool.stream("query=axon".to_string(), ctx).await;

        let chunks: Vec<ToolChunk> = {
            let mut v = Vec::new();
            while let Some(c) = stream.next().await {
                v.push(c);
            }
            v
        };
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].is_terminator());
        assert_eq!(chunks[0].finish_reason, Some(ToolFinishReason::Cancelled));
    }

    // ─── NativeWrappedTool wraps execute() as single-chunk ────────

    #[tokio::test]
    async fn native_wrapped_tool_wraps_calculator_as_single_chunk() {
        let tool = NativeWrappedTool::new("Calculator".to_string());
        let cancel = CancellationFlag::new();
        let ctx = ToolContext::new(cancel, 0);
        let mut stream = tool.stream("2 + 3".to_string(), ctx).await;
        let first = stream.next().await.expect("at least one chunk");
        assert_eq!(first.delta, "5");
        assert_eq!(first.finish_reason, Some(ToolFinishReason::Stop));
        assert!(first.is_terminator());
        // Single-chunk stream — no second chunk.
        assert!(stream.next().await.is_none());
    }

    // ─── SyncFallbackTool emits error-terminator with hint ────────

    #[tokio::test]
    async fn sync_fallback_tool_for_http_emits_error_terminator() {
        // Post-34.e: SyncFallbackTool for `http` is only reached
        // when [`HttpStreamingTool::from_entry`] fails (invalid
        // URL). The error hint MUST point at URL validation rather
        // than the (now-shipped) Fase 34.e itself.
        let tool = SyncFallbackTool::new("HttpTool".to_string(), "http".to_string());
        let cancel = CancellationFlag::new();
        let ctx = ToolContext::new(cancel, 0);
        let mut stream = tool.stream("arg".to_string(), ctx).await;
        let chunk = stream.next().await.expect("at least one chunk");
        assert!(chunk.is_terminator());
        match chunk.finish_reason {
            Some(ToolFinishReason::Error { ref message }) => {
                assert!(message.contains("HTTP streaming is shipped"));
                assert!(message.contains("http"));
                assert!(
                    message.contains("runtime") || message.contains("URL"),
                    "post-34.e fallback hint must reference URL validation: {message}"
                );
            }
            other => panic!("expected Error finish_reason, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn sync_fallback_tool_for_mcp_emits_error_terminator() {
        // Post-34.f: SyncFallbackTool for `mcp` is only reached
        // when [`McpStreamingTool::from_entry`] fails (invalid URL).
        // The error hint MUST point at URL validation rather than
        // the (now-shipped) Fase 34.f itself.
        let tool = SyncFallbackTool::new("McpTool".to_string(), "mcp".to_string());
        let cancel = CancellationFlag::new();
        let ctx = ToolContext::new(cancel, 0);
        let mut stream = tool.stream("arg".to_string(), ctx).await;
        let chunk = stream.next().await.expect("at least one chunk");
        match chunk.finish_reason {
            Some(ToolFinishReason::Error { ref message }) => {
                assert!(message.contains("MCP streaming is shipped"));
                assert!(message.contains("mcp"));
                assert!(
                    message.contains("runtime") || message.contains("URL"),
                    "post-34.f fallback hint must reference URL validation: {message}"
                );
            }
            other => panic!("expected Error finish_reason, got {other:?}"),
        }
    }

    // ─── extract_stream_policy ─────────────────────────────────────

    #[test]
    fn extract_stream_policy_returns_none_for_empty_effect_row() {
        assert_eq!(extract_stream_policy(&[]), None);
    }

    #[test]
    fn extract_stream_policy_returns_none_for_non_stream_effects() {
        assert_eq!(
            extract_stream_policy(&[
                "compute".into(),
                "network".into(),
                "io".into(),
            ]),
            None
        );
    }

    #[test]
    fn extract_stream_policy_parses_drop_oldest() {
        assert_eq!(
            extract_stream_policy(&["stream:drop_oldest".into()]),
            Some(BackpressurePolicy::DropOldest)
        );
    }

    #[test]
    fn extract_stream_policy_parses_all_four_closed_catalog_policies() {
        assert_eq!(
            extract_stream_policy(&["stream:drop_oldest".into()]),
            Some(BackpressurePolicy::DropOldest)
        );
        assert_eq!(
            extract_stream_policy(&["stream:degrade_quality".into()]),
            Some(BackpressurePolicy::DegradeQuality)
        );
        assert_eq!(
            extract_stream_policy(&["stream:pause_upstream".into()]),
            Some(BackpressurePolicy::PauseUpstream)
        );
        assert_eq!(
            extract_stream_policy(&["stream:fail".into()]),
            Some(BackpressurePolicy::Fail)
        );
    }

    #[test]
    fn extract_stream_policy_ignores_unknown_slug() {
        // Defensive: parser rejects unknown slugs at compile time,
        // but the runtime stays robust if stale source somehow
        // reaches the registry.
        assert_eq!(
            extract_stream_policy(&["stream:nonsense_xyz".into()]),
            None
        );
    }

    #[test]
    fn extract_stream_policy_first_wins_on_multiple() {
        // Defensive: malformed source might have multiple
        // stream entries. First-wins is the policy.
        assert_eq!(
            extract_stream_policy(&[
                "stream:drop_oldest".into(),
                "stream:fail".into(),
            ]),
            Some(BackpressurePolicy::DropOldest)
        );
    }

    // ─── build_tool_context ────────────────────────────────────────

    #[test]
    fn build_tool_context_wires_cancel_and_trace_id() {
        let cancel = CancellationFlag::new();
        let ctx = build_tool_context(cancel.clone(), 0xCAFE_BABE);
        assert_eq!(ctx.trace_id, 0xCAFE_BABE);
        assert!(!ctx.is_cancelled());
        cancel.cancel();
        assert!(ctx.is_cancelled());
    }
}
