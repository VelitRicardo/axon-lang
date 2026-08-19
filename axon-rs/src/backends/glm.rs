//! Zhipu GLM backend — v1.18.0.
//!
//! Thin factory + capability override on top of [`OpenAICompatibleBackend`].
//! Zhipu's GLM family speaks OpenAI-compat wire shape verbatim (Bearer
//! auth, `/v1/chat/completions`, OpenAI tool envelope), so the shared
//! base from 24.d does the heavy lifting. This module adds GLM-specific
//! surface:
//!
//!   * [`from_env`] / [`with_api_key`] factories that pin
//!     [`OpenAICompatConfig::glm`] (base URL
//!     `https://open.bigmodel.cn/api/paas`, default model `glm-4-plus`,
//!     env `GLM_API_KEY`).
//!   * **Vision = true for `glm-4v*` family** — the GLM-4V multimodal
//!     line accepts image content blocks. Standard chat models
//!     (`glm-4-plus`, `glm-4-air`, `glm-4-flash`, etc.) are text-only
//!     and report Vision = false.
//!   * No locked-model dispatch — GLM has no documented locked-param
//!     restrictions (unlike Kimi K2.x / OpenAI o1, o3). Sampling
//!     parameters flow through unchanged.
//!
//! GLM-specific advanced features (`web_search` retrieval opt-in,
//! `tools[].retrieval` envelope for knowledge-base lookups) are NOT
//! exposed in 24.g v1 — adopters supply them via raw body overrides
//! when needed. Full DSL exposure lands in a 24.g-followup if demand
//! surfaces from production usage.
//!
//! # Example
//!
//! ```ignore
//! use axon::backends::{glm, Backend, ChatRequest, Message};
//!
//! let backend = glm::from_env();
//! let request = ChatRequest {
//!     model: "glm-4-plus".into(),
//!     messages: vec![Message::user("Translate to Chinese: Hello, world!")],
//!     temperature: Some(0.7),
//!     ..Default::default()
//! };
//! let response = backend.complete(request).await?;
//! ```

use std::env;
use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use super::error::BackendError;
use super::openai_compat::{OpenAICompatConfig, OpenAICompatibleBackend};
use super::{Backend, Capability, ChatRequest, ChatResponse, ChatStream};

const API_KEY_ENV: &str = "GLM_API_KEY";

/// Zhipu GLM backend. Composes [`OpenAICompatibleBackend`] with the
/// GLM preset + a capability override for Vision (true on the
/// `glm-4v*` multimodal family).
pub struct GLMBackend {
    inner: OpenAICompatibleBackend,
}

impl GLMBackend {
    /// Construct from env. `GLM_API_KEY` is read at construction time
    /// (`None` is permitted — the auth check fires at first call). The
    /// `GLM_BASE_URL` / `GLM_CHAT_PATH` overrides (v1.18.0, Kivi
    /// brief #37) are applied so an adopter on z.ai can redirect the
    /// backend off the bigmodel.cn default without a code change.
    pub fn from_env() -> Self {
        let mut config = OpenAICompatConfig::glm();
        config.apply_env_overrides();
        let api_key = env::var(API_KEY_ENV).ok();
        Self {
            inner: OpenAICompatibleBackend::new(config, api_key),
        }
    }

    /// Construct with an explicit API key (or `None`).
    pub fn with_api_key(api_key: Option<String>) -> Self {
        Self {
            inner: OpenAICompatibleBackend::new(OpenAICompatConfig::glm(), api_key),
        }
    }

    /// v1.18.0 (Kivi brief #37) — construct with a per-tenant key +
    /// optional explicit base-URL / chat-path overrides. Precedence:
    /// **explicit (per-tenant) > `GLM_BASE_URL`/`GLM_CHAT_PATH` env >
    /// bigmodel.cn default.** `api_key = None` falls back to `GLM_API_KEY`.
    /// This is the server/daemon construction path that honours a tenant's
    /// z.ai endpoint (`https://api.z.ai/api/paas/v4` + `/chat/completions`).
    pub fn with_api_key_and_endpoint(
        api_key: Option<String>,
        base_url: Option<&str>,
        chat_path: Option<&str>,
    ) -> Self {
        let mut config = OpenAICompatConfig::glm();
        config.apply_env_overrides();
        config.apply_explicit_overrides(base_url, chat_path);
        let api_key = api_key.or_else(|| config.api_key_env.and_then(|e| env::var(e).ok()));
        Self {
            inner: OpenAICompatibleBackend::new(config, api_key),
        }
    }

    /// Override the base URL (test fixtures, regional Zhipu endpoints,
    /// proxy deployments). Returns `self` for builder chaining.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.inner = self.inner.with_base_url(base_url);
        self
    }

    /// Override the default model (e.g. `glm-4-air` for cost-sensitive
    /// workloads, `glm-4v-plus` for vision).
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

impl Default for GLMBackend {
    fn default() -> Self {
        Self::from_env()
    }
}

#[async_trait]
impl Backend for GLMBackend {
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
            // GLM-4V family is multimodal. Anything matching `glm-4v*`
            // accepts image content blocks; standard chat models
            // (`glm-4`, `glm-4-plus`, `glm-4-air`, `glm-4-flash`,
            // `glm-3-turbo`, etc.) are text-only.
            Capability::Vision => model.to_lowercase().starts_with("glm-4v"),
            // GLM has no documented locked-param families (unlike Kimi
            // K2.x or OpenAI o1/o3). The shared base already returns
            // `false` for unknown models — confirmed here for
            // explicitness.
            Capability::LockedParams => false,
            // Streaming / ToolUse / StructuredOutput delegate to base
            // (all true). PromptCaching / SafetySettings = false.
            other => self.inner.supports(other, model),
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  Module-level factories
// ────────────────────────────────────────────────────────────────────

/// Construct a GLM backend using the `GLM_API_KEY` env var.
pub fn from_env() -> GLMBackend {
    GLMBackend::from_env()
}

/// Construct a GLM backend with an explicit API key (or `None`).
pub fn with_api_key(api_key: Option<String>) -> GLMBackend {
    GLMBackend::with_api_key(api_key)
}

#[allow(dead_code)]
type GLMChatStream =
    Pin<Box<dyn Stream<Item = Result<crate::backends::ChatChunk, BackendError>> + Send>>;

// ────────────────────────────────────────────────────────────────────
//  Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::openai_compat::build_request_body;
    use crate::backends::Message;

    fn req_with(messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            model: String::new(),
            messages,
            ..Default::default()
        }
    }

    // ── Construction ────────────────────────────────────────────────

    #[test]
    fn from_env_constructs_glm_backend() {
        let b = GLMBackend::from_env();
        assert_eq!(b.name(), "glm");
        assert_eq!(b.default_model(), "glm-4-plus");
    }

    #[test]
    fn module_factory_from_env_works() {
        let b = from_env();
        assert_eq!(b.name(), "glm");
    }

    #[test]
    fn module_factory_with_api_key_explicit() {
        let b = with_api_key(Some("zhipu-test-key".into()));
        assert_eq!(b.name(), "glm");
    }

    #[test]
    fn with_default_model_overrides() {
        let b = GLMBackend::with_api_key(Some("k".into()))
            .with_default_model("glm-4-air");
        assert_eq!(b.default_model(), "glm-4-air");
    }

    #[test]
    fn with_base_url_overrides_for_test_fixtures() {
        let _b = GLMBackend::with_api_key(Some("k".into()))
            .with_base_url("http://localhost:9999");
    }

    #[test]
    fn inner_accessor_returns_compat_backend() {
        let b = GLMBackend::with_api_key(Some("k".into()));
        assert_eq!(b.inner().name(), "glm");
    }

    #[test]
    fn default_constructs_via_from_env() {
        let b = GLMBackend::default();
        assert_eq!(b.name(), "glm");
    }

    // ── Capability discovery — GLM-specific overrides ───────────────

    #[test]
    fn supports_vision_for_glm_4v_family() {
        let b = GLMBackend::with_api_key(Some("k".into()));
        assert!(b.supports(Capability::Vision, "glm-4v"));
        assert!(b.supports(Capability::Vision, "glm-4v-plus"));
        assert!(b.supports(Capability::Vision, "glm-4v-flash"));
    }

    #[test]
    fn does_not_support_vision_for_chat_only_models() {
        let b = GLMBackend::with_api_key(Some("k".into()));
        assert!(!b.supports(Capability::Vision, "glm-4-plus"));
        assert!(!b.supports(Capability::Vision, "glm-4-air"));
        assert!(!b.supports(Capability::Vision, "glm-4-flash"));
        assert!(!b.supports(Capability::Vision, "glm-3-turbo"));
    }

    #[test]
    fn vision_is_case_insensitive() {
        let b = GLMBackend::with_api_key(Some("k".into()));
        assert!(b.supports(Capability::Vision, "GLM-4v-plus"));
    }

    #[test]
    fn does_not_support_lockedparams_for_any_glm_model() {
        let b = GLMBackend::with_api_key(Some("k".into()));
        // GLM has no documented locked-param families.
        assert!(!b.supports(Capability::LockedParams, "glm-4-plus"));
        assert!(!b.supports(Capability::LockedParams, "glm-4-air"));
        assert!(!b.supports(Capability::LockedParams, "glm-4v-plus"));
    }

    #[test]
    fn supports_streaming_tooluse_structured_via_base() {
        let b = GLMBackend::with_api_key(Some("k".into()));
        let any_model = "glm-4-plus";
        assert!(b.supports(Capability::Streaming, any_model));
        assert!(b.supports(Capability::ToolUse, any_model));
        assert!(b.supports(Capability::StructuredOutput, any_model));
    }

    #[test]
    fn does_not_support_anthropic_or_gemini_only_caps() {
        let b = GLMBackend::with_api_key(Some("k".into()));
        let any_model = "glm-4-plus";
        assert!(!b.supports(Capability::PromptCaching, any_model));
        assert!(!b.supports(Capability::SafetySettings, any_model));
    }

    // ── Sampling params — no locked-model dispatch on GLM ───────────

    #[test]
    fn body_keeps_sampling_params_for_glm_models() {
        // GLM has no locked-param families. Verify temperature + top_p
        // pass through unchanged for every documented GLM model.
        for model in &["glm-4-plus", "glm-4-air", "glm-4-flash", "glm-4v-plus"] {
            let mut req = req_with(vec![Message::user("hi")]);
            req.model = (*model).into();
            req.temperature = Some(0.5);
            req.top_p = Some(0.9);
            let body = build_request_body(&req, "glm-4-plus", false);
            assert_eq!(body["temperature"], 0.5, "model {model} should keep temperature");
            assert_eq!(body["top_p"], 0.9, "model {model} should keep top_p");
        }
    }

    // ── count_tokens delegates to cl100k_base ───────────────────────

    #[test]
    fn count_tokens_uses_cl100k_for_glm_models() {
        let b = GLMBackend::with_api_key(Some("k".into()));
        // GLM uses an OpenAI-compatible BPE — exact (not estimate).
        let n = b.count_tokens("glm-4-plus", "hello world");
        assert!(n > 0);
        assert!(n <= 5);
    }

    #[test]
    fn count_tokens_uses_cl100k_for_glm_4v() {
        let b = GLMBackend::with_api_key(Some("k".into()));
        let n = b.count_tokens("glm-4v-plus", "hello world");
        assert!(n > 0);
    }

    // ── Streaming surface ───────────────────────────────────────────

    #[tokio::test]
    async fn stream_delegates_to_base_real_sse_implementation() {
        // v1.24.0 — GLM delegates to OpenAI-compat which now ships
        // a real SSE streamer; unreachable-port test exercises the
        // transport-error path.
        let b = GLMBackend::with_api_key(Some("k".into()))
            .with_base_url("http://127.0.0.1:1");
        match b.stream(ChatRequest::default()).await {
            Err(BackendError::Generic { ref message, .. }) => {
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

    // ── complete() — early failure paths ────────────────────────────

    #[tokio::test]
    async fn complete_without_api_key_returns_auth_error() {
        let b = GLMBackend::with_api_key(None).with_base_url("http://127.0.0.1:0");
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
}
