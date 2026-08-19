//! Moonshot Kimi backend — v1.18.0.
//!
//! Thin factory + capability override on top of [`OpenAICompatibleBackend`].
//! Moonshot's Kimi family speaks OpenAI-compat wire shape verbatim
//! (Bearer auth, `/v1/chat/completions`, OpenAI tool envelope), so the
//! shared base from 24.d does almost all the work. This module adds
//! Kimi-specific surface:
//!
//!   * [`from_env`] / [`with_api_key`] factories that pin
//!     [`OpenAICompatConfig::kimi`] (base URL `https://api.moonshot.ai`,
//!     default model `moonshot-v1-8k`, env `KIMI_API_KEY`).
//!   * **K2.x locked-param dispatch** — automatic via
//!     [`crate::backends::locked_model`]. Adopters can pass any
//!     sampling parameter (`temperature`, `top_p`, `top_k`, `n`,
//!     `presence_penalty`, `frequency_penalty`); the body builder
//!     silently strips them for `^kimi-k2\.` models so Moonshot's
//!     reasoning models don't return HTTP 400. Verified against the
//!     Moonshot docs (Kivi K2.6 incident, v1.16.2).
//!   * Vision = false. Kimi's mainstream families (K2.x reasoning,
//!     moonshot-v1-* chat) are text-only. If Moonshot ships a
//!     documented vision model in the future, expand this dispatch in
//!     a 24.f-followup; for now reporting `false` is the honest
//!     answer.
//!
//! # Example
//!
//! ```ignore
//! use axon::backends::{kimi, Backend, ChatRequest, Message};
//!
//! let backend = kimi::from_env();
//! let request = ChatRequest {
//!     model: "kimi-k2.6".into(),         // reasoning — sampling params stripped
//!     messages: vec![Message::user("Solve this puzzle...")],
//!     temperature: Some(0.7),            // silently ignored for K2.x
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

const API_KEY_ENV: &str = "KIMI_API_KEY";

/// Moonshot Kimi backend. Composes [`OpenAICompatibleBackend`] with
/// the Kimi preset + a capability override for Vision (false on the
/// Kimi family — text-only).
pub struct KimiBackend {
    inner: OpenAICompatibleBackend,
}

impl KimiBackend {
    /// Construct from env. `KIMI_API_KEY` is read at construction time;
    /// `None` is permitted (auth check fires at first call).
    pub fn from_env() -> Self {
        let mut config = OpenAICompatConfig::kimi();
        config.apply_env_overrides();
        let api_key = env::var(API_KEY_ENV).ok();
        Self {
            inner: OpenAICompatibleBackend::new(config, api_key),
        }
    }

    /// Construct with an explicit API key (or `None`).
    pub fn with_api_key(api_key: Option<String>) -> Self {
        Self {
            inner: OpenAICompatibleBackend::new(OpenAICompatConfig::kimi(), api_key),
        }
    }

    /// v1.18.0 — construct with a per-tenant key + optional explicit
    /// base-URL / chat-path overrides (precedence: explicit > env >
    /// default). `api_key = None` falls back to `KIMI_API_KEY`.
    pub fn with_api_key_and_endpoint(
        api_key: Option<String>,
        base_url: Option<&str>,
        chat_path: Option<&str>,
    ) -> Self {
        let mut config = OpenAICompatConfig::kimi();
        config.apply_env_overrides();
        config.apply_explicit_overrides(base_url, chat_path);
        let api_key = api_key.or_else(|| config.api_key_env.and_then(|e| env::var(e).ok()));
        Self {
            inner: OpenAICompatibleBackend::new(config, api_key),
        }
    }

    /// Override the base URL (test fixtures, regional Moonshot
    /// endpoints if any). Returns `self` for builder chaining.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.inner = self.inner.with_base_url(base_url);
        self
    }

    /// Override the default model. Useful when an adopter pins
    /// `kimi-k2.6` or a specific chat model as the team default.
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

impl Default for KimiBackend {
    fn default() -> Self {
        Self::from_env()
    }
}

#[async_trait]
impl Backend for KimiBackend {
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
            // Kimi's mainstream families (K2.x reasoning, moonshot-v1-*
            // chat) are text-only. If Moonshot ships a documented vision
            // model in the future, expand this dispatch — for 24.f v1
            // the conservative answer is `false`.
            Capability::Vision => false,
            // LockedParams = true for `^kimi-k2\.` automatically — the
            // shared base consults `locked_model::locked_params_for_model`.
            // Streaming / ToolUse / StructuredOutput delegate to base
            // (all true). PromptCaching / SafetySettings = false.
            other => self.inner.supports(other, model),
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  Module-level factories
// ────────────────────────────────────────────────────────────────────

/// Construct a Kimi backend using the `KIMI_API_KEY` env var.
pub fn from_env() -> KimiBackend {
    KimiBackend::from_env()
}

/// Construct a Kimi backend with an explicit API key (or `None`).
pub fn with_api_key(api_key: Option<String>) -> KimiBackend {
    KimiBackend::with_api_key(api_key)
}

#[allow(dead_code)]
type KimiChatStream =
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
    fn from_env_constructs_kimi_backend() {
        let b = KimiBackend::from_env();
        assert_eq!(b.name(), "kimi");
        assert_eq!(b.default_model(), "moonshot-v1-8k");
    }

    #[test]
    fn module_factory_from_env_works() {
        let b = from_env();
        assert_eq!(b.name(), "kimi");
    }

    #[test]
    fn module_factory_with_api_key_explicit() {
        let b = with_api_key(Some("sk-moonshot-test".into()));
        assert_eq!(b.name(), "kimi");
    }

    #[test]
    fn with_default_model_overrides() {
        let b = KimiBackend::with_api_key(Some("k".into()))
            .with_default_model("kimi-k2.6");
        assert_eq!(b.default_model(), "kimi-k2.6");
    }

    #[test]
    fn with_base_url_overrides_for_test_fixtures() {
        let _b = KimiBackend::with_api_key(Some("k".into()))
            .with_base_url("http://localhost:9999");
        // Verified indirectly by the auth-error test below: base URL
        // must point at unreachable endpoint to exercise auth check.
    }

    #[test]
    fn inner_accessor_returns_compat_backend() {
        let b = KimiBackend::with_api_key(Some("k".into()));
        assert_eq!(b.inner().name(), "kimi");
    }

    // ── Capability discovery — Kimi-specific overrides ──────────────

    #[test]
    fn does_not_support_vision_on_any_kimi_model() {
        let b = KimiBackend::with_api_key(Some("k".into()));
        assert!(!b.supports(Capability::Vision, "moonshot-v1-8k"));
        assert!(!b.supports(Capability::Vision, "moonshot-v1-32k"));
        assert!(!b.supports(Capability::Vision, "kimi-k2.6"));
        assert!(!b.supports(Capability::Vision, "kimi-k2.8"));
    }

    #[test]
    fn supports_lockedparams_for_kimi_k2_via_shared_base() {
        let b = KimiBackend::with_api_key(Some("k".into()));
        assert!(b.supports(Capability::LockedParams, "kimi-k2.6"));
        assert!(b.supports(Capability::LockedParams, "kimi-k2.8"));
    }

    #[test]
    fn does_not_support_lockedparams_for_moonshot_v1_chat_models() {
        let b = KimiBackend::with_api_key(Some("k".into()));
        assert!(!b.supports(Capability::LockedParams, "moonshot-v1-8k"));
        assert!(!b.supports(Capability::LockedParams, "moonshot-v1-32k"));
        assert!(!b.supports(Capability::LockedParams, "moonshot-v1-128k"));
    }

    #[test]
    fn supports_streaming_tooluse_structured_via_base() {
        let b = KimiBackend::with_api_key(Some("k".into()));
        let any_model = "moonshot-v1-8k";
        assert!(b.supports(Capability::Streaming, any_model));
        assert!(b.supports(Capability::ToolUse, any_model));
        assert!(b.supports(Capability::StructuredOutput, any_model));
    }

    #[test]
    fn does_not_support_anthropic_or_gemini_only_caps() {
        let b = KimiBackend::with_api_key(Some("k".into()));
        let any_model = "moonshot-v1-8k";
        assert!(!b.supports(Capability::PromptCaching, any_model));
        assert!(!b.supports(Capability::SafetySettings, any_model));
    }

    // ── K2.x locked-param dispatch (the v1.16.2 incident port) ──────

    #[test]
    fn body_strips_temperature_for_kimi_k2_6() {
        // The v1.16.2 incident: Moonshot K2.6 returns HTTP 400 if any
        // sampling parameter is sent. The shared base's body builder
        // routes through `locked_model::apply_sampling_params`, which
        // strips the locked fields. This test verifies the regression
        // is permanently closed in the Rust path.
        let mut req = req_with(vec![Message::user("hi")]);
        req.model = "kimi-k2.6".into();
        req.temperature = Some(0.5);
        req.top_p = Some(0.9);
        let body = build_request_body(&req, "moonshot-v1-8k", false);
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
    }

    #[test]
    fn body_strips_full_locked_set_for_kimi_k2_8() {
        // K2.x locks 6 sampling params: temperature, top_p, top_k, n,
        // presence_penalty, frequency_penalty. `top_k` / `n` /
        // `presence_penalty` / `frequency_penalty` aren't on the
        // ChatRequest surface yet (24.h-followup), so we verify
        // temperature + top_p — the two adopters routinely set.
        let mut req = req_with(vec![Message::user("hi")]);
        req.model = "kimi-k2.8".into();
        req.temperature = Some(1.0);
        req.top_p = Some(0.95);
        let body = build_request_body(&req, "moonshot-v1-8k", false);
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
    }

    #[test]
    fn body_keeps_sampling_params_for_moonshot_v1_chat() {
        // Pre-K2 Moonshot chat models accept sampling params freely.
        // The locked-model dispatch must NOT strip them for these
        // models — they're not in the locked registry.
        let mut req = req_with(vec![Message::user("hi")]);
        req.model = "moonshot-v1-8k".into();
        req.temperature = Some(0.3);
        req.top_p = Some(0.85);
        let body = build_request_body(&req, "moonshot-v1-8k", false);
        assert_eq!(body["temperature"], 0.3);
        assert_eq!(body["top_p"], 0.85);
    }

    // ── count_tokens delegates to cl100k_base ───────────────────────

    #[test]
    fn count_tokens_uses_cl100k_for_moonshot_models() {
        let b = KimiBackend::with_api_key(Some("k".into()));
        // Moonshot uses an OpenAI-compatible BPE — exact (not estimate).
        let n = b.count_tokens("moonshot-v1-8k", "hello world");
        assert!(n > 0);
        assert!(n <= 5);
    }

    #[test]
    fn count_tokens_uses_cl100k_for_kimi_k2() {
        let b = KimiBackend::with_api_key(Some("k".into()));
        let n = b.count_tokens("kimi-k2.6", "hello world");
        assert!(n > 0);
    }

    // ── Streaming surface ───────────────────────────────────────────

    #[tokio::test]
    async fn stream_delegates_to_base_real_sse_implementation() {
        // v1.24.0 — base OpenAI-compat now implements SSE streaming
        // natively; this delegate path inherits it. Without a reachable
        // server the test exercises the transport-error path.
        let b = KimiBackend::with_api_key(Some("k".into()))
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
        let b = KimiBackend::with_api_key(None).with_base_url("http://127.0.0.1:0");
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

    // ── Default trait works ─────────────────────────────────────────

    #[test]
    fn default_constructs_via_from_env() {
        let b = KimiBackend::default();
        assert_eq!(b.name(), "kimi");
    }
}
