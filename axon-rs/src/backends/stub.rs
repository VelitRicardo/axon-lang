//! v1.24.0 — Production `StubBackend` implementing the
//! [`Backend`] trait so the streaming path dispatches through the
//! v1.18.0 [`Registry`] uniformly (no special-cased "if backend ==
//! \"stub\"" branches scattered across the runtime).
//!
//! # Wire byte-compat with v1.24.0 (D4 invariant)
//!
//! The pre-33.x.b synthetic-chunking path emitted one `axon.token`
//! event with content `"(stub)"` followed by `axon.complete` for a
//! single-step stub flow. To preserve byte-identical wire shape for
//! every test in the repo that drives the SSE path through `stub`,
//! [`StubBackend::stream`] returns a one-chunk stream whose `delta`
//! is the same `"(stub)"` placeholder. Multi-step flows still get
//! one chunk per step (since each step gets its own `stream()` call
//! from the per-step async loop in [`crate::axon_server`]).
//!
//! # Why a real `Backend` impl (not a special-case)
//!
//! D1 (v1.24.0 plan vivo): `Backend::stream()` is the only production
//! path for `output: Stream<T>`. The mono-file `crate::backend` is
//! retired in 33.x.i. To honor D1 today (33.x.b) without retiring the
//! mono-file yet, the streaming path resolves through a
//! [`Registry`] that includes `stub` as a first-class entry. The CLI
//! sync path (per D8) keeps using the mono-file path unchanged.
//!
//! # Drift-gate placement
//!
//! `stub.rs` is excluded from the v1.18.0 cross-stack drift gate
//! (`tests/test_fase24_backend_parity.py` SHARED_INFRA_MODULES set)
//! because it is not a real provider — it has no API key, no real
//! LLM, no Python `BACKEND_REGISTRY` counterpart. The drift gate
//! continues to pin the canonical 7 providers exactly.

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use super::{
    Backend, Capability, ChatChunk, ChatRequest, ChatResponse, ChatStream, FinishReason,
    Usage,
};
use super::error::BackendError;

/// Canonical short name. The streaming-path resolver matches this
/// string before consulting the v1.18.0 [`super::Registry`] for the
/// 7 production providers.
pub const STUB_PROVIDER_NAME: &str = "stub";

/// Canonical default-model slug. Surfaces in the trace span +
/// `axon.complete` `backend` field for adopter diagnostics.
pub const STUB_DEFAULT_MODEL: &str = "stub-model";

/// Canonical single-chunk delta. Preserves byte-compat with the
/// pre-33.x.b synthetic-chunking path which emitted `"(stub)"` as
/// the only `axon.token` content for a single-step stub flow.
pub const STUB_CONTENT: &str = "(stub)";

/// Production stub backend.
///
/// `complete()` returns a single response with content
/// [`STUB_CONTENT`] and `FinishReason::Stop`. `stream()` returns a
/// one-chunk stream with the same delta and a terminal `Usage` of
/// `{input_tokens: 0, output_tokens: 0}` so adopter dashboards
/// reading `axon.complete.tokens_output` see byte-identical values
/// vs v1.24.0 (D4 wire byte-compat).
#[derive(Debug, Default, Clone)]
pub struct StubBackend {
    /// Optional override for the chunk content. Default is
    /// [`STUB_CONTENT`]. Tests that need to verify alternate
    /// content via a stub-driven path (e.g. multi-step wire shape
    /// assertions) override this without touching the production
    /// constant.
    chunk_content: Option<String>,
}

impl StubBackend {
    /// Default constructor — emits `"(stub)"` as the chunk content.
    /// This is the production preset that matches v1.24.0 wire
    /// byte-compat.
    pub fn new() -> Self {
        Self { chunk_content: None }
    }

    /// Override the chunk content. Useful for tests that drive a
    /// multi-step flow through the stub path and need to verify
    /// per-step content distinction in the wire body.
    pub fn with_chunk_content(mut self, content: impl Into<String>) -> Self {
        self.chunk_content = Some(content.into());
        self
    }

    fn effective_chunk(&self) -> &str {
        self.chunk_content.as_deref().unwrap_or(STUB_CONTENT)
    }

    /// What this stub answers to `request`.
    ///
    /// For every ordinary prompt it is [`STUB_CONTENT`] — byte-compat with the
    /// v1.24.0 wire. For an AGENT deliberation it answers the protocol the
    /// agent loop speaks, recognised by the framing constants the loop itself
    /// publishes (`agent_loop::REACT_FRAMING` …): ReAct gets `ACT: <first
    /// granted tool>` on the first iteration (no observations yet) and
    /// `ANSWER: (stub)` afterwards — when the agent grants no tool at all it
    /// stays `(stub)`, so the declared bound terminates the run; a Reflexion
    /// critique gets `ACCEPT`. The
    /// loop still does every parse, grant check, tool dispatch and bound
    /// check itself — the stub only stops being an answer that can never
    /// terminate the protocol, so `axon run --tool-mode stub` shows the
    /// iterations and the dispatch an adopter came to see.
    pub fn answer_for(&self, request: &ChatRequest) -> String {
        if self.chunk_content.is_some() {
            return self.effective_chunk().to_string();
        }
        let system = request.system.as_deref().unwrap_or("");
        let user = request
            .messages
            .last()
            .map(|m| m.content.as_str())
            .unwrap_or("");
        if system.contains(crate::flow_dispatcher::agent_loop::REFLEXION_CRITIQUE_FRAMING) {
            return "ACCEPT".to_string();
        }
        if system.contains(crate::flow_dispatcher::agent_loop::REACT_FRAMING) {
            let first_iteration = user
                .contains(crate::flow_dispatcher::agent_loop::NO_OBSERVATIONS_YET);
            let first_tool = user
                .lines()
                .find_map(|l| l.strip_prefix(crate::flow_dispatcher::agent_loop::TOOLS_LINE_PREFIX))
                .and_then(|rest| rest.split(',').next())
                .map(|t| t.trim())
                .filter(|t| !t.is_empty() && !t.starts_with('('));
            // With nothing to dispatch the stub has nothing to demonstrate and
            // cannot honestly claim an answer: it stays the unstructured
            // `(stub)`, the protocol violation the loop records, and the
            // declared bound is what terminates the run — which is exactly
            // the property the bound tests exercise.
            return match (first_iteration, first_tool) {
                (true, Some(tool)) => format!("ACT: {tool}"),
                (false, Some(_)) => format!("ANSWER: {STUB_CONTENT}"),
                (_, None) => STUB_CONTENT.to_string(),
            };
        }
        self.effective_chunk().to_string()
    }
}

#[async_trait]
impl Backend for StubBackend {
    fn name(&self) -> &str {
        STUB_PROVIDER_NAME
    }

    fn default_model(&self) -> &str {
        STUB_DEFAULT_MODEL
    }

    async fn complete(&self, request: ChatRequest) -> Result<ChatResponse, BackendError> {
        let content = self.answer_for(&request);
        let trace_id = request.trace_id.unwrap_or_else(|| "stub-trace".to_string());
        Ok(ChatResponse {
            content,
            model_name: STUB_DEFAULT_MODEL.to_string(),
            provider_name: STUB_PROVIDER_NAME.to_string(),
            finish_reason: FinishReason::Stop,
            usage: Usage::default(),
            retry_count: 0,
            trace_id,
        })
    }

    async fn stream(&self, request: ChatRequest) -> Result<ChatStream, BackendError> {
        // One-chunk stream with the chunk content as delta + a
        // terminal envelope carrying FinishReason::Stop and a zero
        // Usage so the downstream `axon.complete` event reports
        // `tokens_output: 0` byte-identically with v1.24.0.
        //
        // v1.24.0 — Even though stub emits a single chunk that
        // would deliver fast, the cancel-aware wrap preserves the
        // contract: if cancel fires before the chunk reaches the
        // consumer (rare in stub, common in real backends) the
        // stream terminates promptly. Uniform behavior across the
        // 8-backend dispatch keeps downstream tests authoritative.
        let chunk = ChatChunk {
            delta: self.answer_for(&request),
            finish_reason: Some(FinishReason::Stop),
            usage: Some(Usage::default()),
        };
        let inner: Pin<Box<dyn Stream<Item = Result<ChatChunk, BackendError>> + Send>> =
            Box::pin(futures::stream::iter(vec![Ok(chunk)]));
        Ok(super::sse_streaming::cancel_aware(inner, request.cancel.clone()))
    }

    fn supports(&self, capability: Capability, _model: &str) -> bool {
        // Stub honors streaming as a first-class capability; tool-use,
        // vision, prompt-caching etc. stay false because the stub
        // doesn't model them. Closed-catalog match keeps adding a new
        // Capability variant force a deliberate update here.
        match capability {
            Capability::Streaming => true,
            Capability::ToolUse
            | Capability::Vision
            | Capability::PromptCaching
            | Capability::SafetySettings
            | Capability::StructuredOutput
            | Capability::LockedParams => false,
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[test]
    fn name_is_canonical_stub() {
        let s = StubBackend::new();
        assert_eq!(s.name(), STUB_PROVIDER_NAME);
        assert_eq!(s.name(), "stub");
    }

    #[test]
    fn default_model_is_canonical_slug() {
        let s = StubBackend::new();
        assert_eq!(s.default_model(), STUB_DEFAULT_MODEL);
        assert_eq!(s.default_model(), "stub-model");
    }

    #[tokio::test]
    async fn complete_returns_canonical_stub_content() {
        let s = StubBackend::new();
        let req = ChatRequest::default();
        let resp = s.complete(req).await.expect("stub complete never fails");
        assert_eq!(resp.content, STUB_CONTENT);
        assert_eq!(resp.content, "(stub)");
        assert_eq!(resp.model_name, "stub-model");
        assert_eq!(resp.provider_name, "stub");
        assert_eq!(resp.finish_reason, FinishReason::Stop);
        assert_eq!(resp.retry_count, 0);
        assert_eq!(resp.usage, Usage::default());
    }

    #[tokio::test]
    async fn complete_echoes_trace_id_when_provided() {
        let s = StubBackend::new();
        let req = ChatRequest {
            trace_id: Some("flow-42".to_string()),
            ..Default::default()
        };
        let resp = s.complete(req).await.unwrap();
        assert_eq!(resp.trace_id, "flow-42");
    }

    #[tokio::test]
    async fn complete_synthesizes_trace_id_when_absent() {
        let s = StubBackend::new();
        let req = ChatRequest::default();
        let resp = s.complete(req).await.unwrap();
        assert!(!resp.trace_id.is_empty());
    }

    #[tokio::test]
    async fn stream_emits_exactly_one_chunk_with_stub_content() {
        let s = StubBackend::new();
        let req = ChatRequest { stream: true, ..Default::default() };
        let mut chunks: Vec<ChatChunk> = Vec::new();
        let mut stream = s.stream(req).await.expect("stub stream never fails");
        while let Some(item) = stream.next().await {
            chunks.push(item.expect("stub never errors"));
        }
        assert_eq!(chunks.len(), 1, "v1.24.0 byte-compat: exactly one chunk");
        let chunk = &chunks[0];
        assert_eq!(chunk.delta, STUB_CONTENT);
        assert_eq!(chunk.finish_reason, Some(FinishReason::Stop));
        assert_eq!(chunk.usage, Some(Usage::default()));
    }

    #[tokio::test]
    async fn stream_with_chunk_content_override_emits_custom_delta() {
        let s = StubBackend::new().with_chunk_content("hello world");
        let req = ChatRequest::default();
        let mut stream = s.stream(req).await.unwrap();
        let chunk = stream.next().await.unwrap().unwrap();
        assert_eq!(chunk.delta, "hello world");
        assert!(stream.next().await.is_none(), "single-chunk semantics preserved");
    }

    #[test]
    fn supports_streaming_capability() {
        let s = StubBackend::new();
        assert!(s.supports(Capability::Streaming, "any-model"));
    }

    #[test]
    fn supports_false_for_non_streaming_capabilities() {
        let s = StubBackend::new();
        for cap in [
            Capability::ToolUse,
            Capability::Vision,
            Capability::PromptCaching,
            Capability::SafetySettings,
            Capability::StructuredOutput,
            Capability::LockedParams,
        ] {
            assert!(!s.supports(cap, "any-model"), "{:?}", cap);
        }
    }

    #[tokio::test]
    async fn stream_chunk_delta_byte_compat_with_v1_24_0_wire() {
        // The v1.24.0 production-path wire emits:
        //   data: {"step":"Generate","timestamp_ms":...,"token":"(stub)","trace_id":1}
        // Post-33.x.b the same wire body is produced by:
        //   stub.stream() → 1 chunk with delta="(stub)" → 1 StepToken → 1 axon.token
        // This test pins the delta byte-for-byte.
        let s = StubBackend::new();
        let req = ChatRequest::default();
        let chunk = s
            .stream(req)
            .await
            .unwrap()
            .next()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(chunk.delta.as_bytes(), b"(stub)");
    }

    #[test]
    fn stub_is_clone_send_sync() {
        fn assert_traits<T: Clone + Send + Sync>() {}
        assert_traits::<StubBackend>();
    }

    #[tokio::test]
    async fn dyn_backend_dispatch_through_box() {
        // Verify object-safety — StubBackend can live in Box<dyn Backend>
        // alongside the 7 production providers in a single Registry.
        let b: Box<dyn Backend> = Box::new(StubBackend::new());
        let req = ChatRequest::default();
        let resp = b.complete(req).await.unwrap();
        assert_eq!(resp.content, "(stub)");
    }
}
