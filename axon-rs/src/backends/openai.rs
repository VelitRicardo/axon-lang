//! OpenAI Chat Completions backend — v1.18.0.
//!
//! Thin factory + capability override on top of [`OpenAICompatibleBackend`].
//! The OpenAI provider is the canonical OpenAI-compat shape; everything
//! about the wire (Bearer auth, `/v1/chat/completions`, OpenAI tool
//! envelope) lives in the shared base.
//!
//! What this module adds on top of the shared base:
//!
//!   * [`from_env`] / [`with_api_key`] factories that pin
//!     [`OpenAICompatConfig::openai`] (base URL, default model, env var).
//!   * Vision support discovery — `gpt-4o*` models accept image content
//!     blocks; older models don't. The shared base conservatively
//!     reports `Capability::Vision = false`; OpenAI's adapter overrides
//!     to `true` for the gpt-4o family.
//!   * o1 / o3 reasoning models work transparently — the locked-model
//!     dispatch in the body builder strips `temperature` / `top_p` /
//!     `presence_penalty` / `frequency_penalty` / `logprobs` /
//!     `logit_bias` for those families, so adopters can pass any
//!     sampling params they like and they're silently filtered out.
//!     `Capability::LockedParams` returns `true` for the resolved model
//!     when this filtering would fire.
//!
//! # Example
//!
//! ```ignore
//! use axon::backends::{openai, Backend, ChatRequest, Message};
//!
//! let backend = openai::from_env();
//! let request = ChatRequest {
//!     model: "gpt-4o-mini".into(),
//!     messages: vec![Message::user("Hello!")],
//!     temperature: Some(0.7),
//!     ..Default::default()
//! };
//! let response = backend.complete(request).await?;
//! println!("{}", response.content);
//! ```

use std::env;
use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use super::error::BackendError;
use super::openai_compat::{OpenAICompatConfig, OpenAICompatibleBackend};
use super::{Backend, Capability, ChatRequest, ChatResponse, ChatStream};

const API_KEY_ENV: &str = "OPENAI_API_KEY";

/// OpenAI Chat Completions backend. Composes [`OpenAICompatibleBackend`]
/// with the OpenAI preset + a capability override for `Vision` on the
/// gpt-4o family.
pub struct OpenAIBackend {
    inner: OpenAICompatibleBackend,
}

impl OpenAIBackend {
    /// Construct from env. `OPENAI_API_KEY` is read at construction time
    /// (`None` is permitted — auth check fires at first call). The
    /// `OPENAI_BASE_URL` / `OPENAI_CHAT_PATH` overrides (v1.18.0)
    /// redirect the backend at an Azure / proxy OpenAI-compatible
    /// endpoint without a code change.
    pub fn from_env() -> Self {
        let mut config = OpenAICompatConfig::openai();
        config.apply_env_overrides();
        let api_key = env::var(API_KEY_ENV).ok();
        Self {
            inner: OpenAICompatibleBackend::new(config, api_key),
        }
    }

    /// Construct with an explicit API key (or `None`).
    pub fn with_api_key(api_key: Option<String>) -> Self {
        Self {
            inner: OpenAICompatibleBackend::new(OpenAICompatConfig::openai(), api_key),
        }
    }

    /// v1.18.0 — construct with a per-tenant key + optional explicit
    /// base-URL / chat-path overrides (precedence: explicit > env >
    /// default). `api_key = None` falls back to `OPENAI_API_KEY`.
    pub fn with_api_key_and_endpoint(
        api_key: Option<String>,
        base_url: Option<&str>,
        chat_path: Option<&str>,
    ) -> Self {
        let mut config = OpenAICompatConfig::openai();
        config.apply_env_overrides();
        config.apply_explicit_overrides(base_url, chat_path);
        let api_key = api_key.or_else(|| config.api_key_env.and_then(|e| env::var(e).ok()));
        Self {
            inner: OpenAICompatibleBackend::new(config, api_key),
        }
    }

    /// Override the base URL (test fixtures, mock servers, Azure
    /// OpenAI-compatible deployments). Returns `self` for builder
    /// chaining.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.inner = self.inner.with_base_url(base_url);
        self
    }

    /// Override the default model.
    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        self.inner = self.inner.with_default_model(model);
        self
    }

    /// Borrow the underlying [`OpenAICompatibleBackend`] (for testing
    /// fixtures that need access to the composed inner state).
    pub fn inner(&self) -> &OpenAICompatibleBackend {
        &self.inner
    }
}

impl Default for OpenAIBackend {
    fn default() -> Self {
        Self::from_env()
    }
}

#[async_trait]
impl Backend for OpenAIBackend {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn default_model(&self) -> &str {
        self.inner.default_model()
    }

    async fn complete(&self, request: ChatRequest) -> Result<ChatResponse, BackendError> {
        self.inner.complete(request).await
    }

    async fn stream(&self, request: ChatRequest) -> Result<ChatStream, BackendError> {
        self.inner.stream(request).await
    }

    fn count_tokens(&self, model: &str, text: &str) -> usize {
        self.inner.count_tokens(model, text)
    }

    fn supports(&self, capability: Capability, model: &str) -> bool {
        match capability {
            // OpenAI gpt-4o family supports image content blocks.
            // Older models (gpt-3.5, gpt-4 turbo) and reasoning models
            // (o1*, o3*) do not. Conservative match: only gpt-4o* gets
            // a true here.
            Capability::Vision => model.to_lowercase().starts_with("gpt-4o"),
            // Everything else delegates to the shared base — Streaming,
            // ToolUse, StructuredOutput, LockedParams (for o1/o3) all
            // return whatever the base reports.
            other => self.inner.supports(other, model),
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  Module-level factories — adopter-friendly entry points
// ────────────────────────────────────────────────────────────────────

/// Construct an OpenAI backend using the `OPENAI_API_KEY` env var.
///
/// Convenience over `OpenAIBackend::from_env()` — adopter writes
/// `let b = backends::openai::from_env();`.
pub fn from_env() -> OpenAIBackend {
    OpenAIBackend::from_env()
}

/// Construct an OpenAI backend with an explicit API key (or `None`).
pub fn with_api_key(api_key: Option<String>) -> OpenAIBackend {
    OpenAIBackend::with_api_key(api_key)
}

#[allow(dead_code)]
type OpenAIChatStream =
    Pin<Box<dyn Stream<Item = Result<crate::backends::ChatChunk, BackendError>> + Send>>;

// ────────────────────────────────────────────────────────────────────
//  Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::Message;

    // ── Construction ────────────────────────────────────────────────

    #[test]
    fn from_env_constructs_openai_backend() {
        let b = OpenAIBackend::from_env();
        assert_eq!(b.name(), "openai");
        assert_eq!(b.default_model(), "gpt-4o-mini");
    }

    #[test]
    fn module_factory_from_env_works() {
        let b = from_env();
        assert_eq!(b.name(), "openai");
    }

    #[test]
    fn module_factory_with_api_key_explicit() {
        let b = with_api_key(Some("sk-test".into()));
        assert_eq!(b.name(), "openai");
    }

    #[test]
    fn with_base_url_overrides() {
        let b = OpenAIBackend::with_api_key(Some("k".into()))
            .with_base_url("http://localhost:1234");
        // Verify by going through inner accessor.
        // (No public getter for base_url — exercise via complete()
        // failure path in dedicated test below.)
        let _ = b;
    }

    #[test]
    fn with_default_model_overrides() {
        let b = OpenAIBackend::with_api_key(Some("k".into()))
            .with_default_model("o1-mini");
        assert_eq!(b.default_model(), "o1-mini");
    }

    // ── Capability discovery — OpenAI-specific overrides ────────────

    #[test]
    fn supports_vision_for_gpt_4o_family() {
        let b = OpenAIBackend::with_api_key(Some("k".into()));
        assert!(b.supports(Capability::Vision, "gpt-4o"));
        assert!(b.supports(Capability::Vision, "gpt-4o-mini"));
        assert!(b.supports(Capability::Vision, "gpt-4o-2024-08-06"));
    }

    #[test]
    fn does_not_support_vision_for_older_models() {
        let b = OpenAIBackend::with_api_key(Some("k".into()));
        assert!(!b.supports(Capability::Vision, "gpt-3.5-turbo"));
        assert!(!b.supports(Capability::Vision, "gpt-4"));
        assert!(!b.supports(Capability::Vision, "gpt-4-turbo"));
    }

    #[test]
    fn does_not_support_vision_for_reasoning_models() {
        // o1 / o3 are text-only reasoning models.
        let b = OpenAIBackend::with_api_key(Some("k".into()));
        assert!(!b.supports(Capability::Vision, "o1"));
        assert!(!b.supports(Capability::Vision, "o1-mini"));
        assert!(!b.supports(Capability::Vision, "o3-mini"));
    }

    #[test]
    fn vision_is_case_insensitive() {
        let b = OpenAIBackend::with_api_key(Some("k".into()));
        assert!(b.supports(Capability::Vision, "GPT-4o-mini"));
    }

    // ── Locked-params reaches o1/o3 via shared base ─────────────────

    #[test]
    fn supports_lockedparams_for_o1_o3() {
        let b = OpenAIBackend::with_api_key(Some("k".into()));
        assert!(b.supports(Capability::LockedParams, "o1"));
        assert!(b.supports(Capability::LockedParams, "o1-mini"));
        assert!(b.supports(Capability::LockedParams, "o1-preview"));
        assert!(b.supports(Capability::LockedParams, "o3"));
        assert!(b.supports(Capability::LockedParams, "o3-mini"));
    }

    #[test]
    fn does_not_support_lockedparams_for_chat_models() {
        let b = OpenAIBackend::with_api_key(Some("k".into()));
        assert!(!b.supports(Capability::LockedParams, "gpt-4o-mini"));
        assert!(!b.supports(Capability::LockedParams, "gpt-3.5-turbo"));
        assert!(!b.supports(Capability::LockedParams, "gpt-4"));
    }

    // ── Capabilities passed through to base ─────────────────────────

    #[test]
    fn supports_streaming_tooluse_structured_via_base() {
        let b = OpenAIBackend::with_api_key(Some("k".into()));
        assert!(b.supports(Capability::Streaming, "gpt-4o-mini"));
        assert!(b.supports(Capability::ToolUse, "gpt-4o-mini"));
        assert!(b.supports(Capability::StructuredOutput, "gpt-4o-mini"));
    }

    #[test]
    fn does_not_support_anthropic_or_gemini_only_caps() {
        let b = OpenAIBackend::with_api_key(Some("k".into()));
        assert!(!b.supports(Capability::PromptCaching, "gpt-4o-mini"));
        assert!(!b.supports(Capability::SafetySettings, "gpt-4o-mini"));
    }

    // ── count_tokens delegates to unified dispatch ──────────────────

    #[test]
    fn count_tokens_uses_o200k_for_gpt_4o() {
        let b = OpenAIBackend::with_api_key(Some("k".into()));
        let n = b.count_tokens("gpt-4o-mini", "hello world");
        // Exact tokenizer reports a small nonzero count.
        assert!(n > 0);
        assert!(n <= 5);
    }

    #[test]
    fn count_tokens_uses_o200k_for_o1() {
        let b = OpenAIBackend::with_api_key(Some("k".into()));
        let n = b.count_tokens("o1-mini", "hello world");
        assert!(n > 0);
    }

    // ── complete() — early failure paths ────────────────────────────

    #[tokio::test]
    async fn complete_without_api_key_returns_auth_error() {
        let b = OpenAIBackend::with_api_key(None).with_base_url("http://127.0.0.1:0");
        let err = b
            .complete(ChatRequest {
                messages: vec![Message::user("hi")],
                ..Default::default()
            })
            .await
            .unwrap_err();
        match err {
            BackendError::Auth { api_key_env, .. } => {
                assert_eq!(api_key_env.as_deref(), Some(API_KEY_ENV));
            }
            other => panic!("expected Auth, got {other:?}"),
        }
    }

    // ── Streaming surface ───────────────────────────────────────────

    #[tokio::test]
    async fn stream_delegates_to_base_real_sse_implementation() {
        // v1.24.0 — OpenAI-compat now implements SSE streaming
        // natively. Without a reachable server this test exercises the
        // transport-error path (early failure before any chunk).
        let b = OpenAIBackend::with_api_key(Some("k".into()))
            .with_base_url("http://127.0.0.1:1");
        match b.stream(ChatRequest::default()).await {
            Err(BackendError::Generic { ref message, .. }) => {
                // Transport failure on unreachable port; message
                // contains the connect-error string from reqwest.
                assert!(
                    message.contains("streaming transport failure")
                        || message.contains("transport"),
                    "unexpected message: {message}"
                );
            }
            Err(other) => panic!("expected Generic, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    // ── Inner accessor exists for test fixtures ─────────────────────

    #[test]
    fn inner_accessor_returns_compat_backend() {
        let b = OpenAIBackend::with_api_key(Some("k".into()));
        assert_eq!(b.inner().name(), "openai");
    }
}
