//! OpenRouter (multi-provider gateway) backend — v1.18.0.
//!
//! Thin factory + slug-aware capability override on top of
//! [`OpenAICompatibleBackend`]. OpenRouter speaks OpenAI-compat verbatim
//! (Bearer auth, `/v1/chat/completions`, OpenAI tool envelope) and
//! routes requests to underlying providers based on the
//! `provider/model` slug form (e.g. `openai/gpt-4o-mini`,
//! `anthropic/claude-sonnet-4-5`, `google/gemini-2.5-flash`,
//! `moonshot/kimi-k2.6`).
//!
//! What this module adds on top of the shared base:
//!
//!   * [`from_env`] / [`with_api_key`] factories that pin
//!     [`OpenAICompatConfig::openrouter`] (base URL
//!     `https://openrouter.ai/api`, default model
//!     `openai/gpt-4o-mini`, env `OPENROUTER_API_KEY`).
//!   * **Slug-aware Vision dispatch** — `Capability::Vision` consults
//!     the underlying model name (e.g. `openai/gpt-4o-mini` → true
//!     because gpt-4o family supports vision; `meta/llama-3.1-70b` →
//!     false because Llama 3.1 is text-only). The dispatch matches
//!     the per-provider rules from `openai.rs` / `gemini.rs` /
//!     `anthropic.rs` / `glm.rs` / `ollama.rs` — adopters get
//!     consistent `supports()` answers regardless of whether they
//!     route through OpenRouter or call the provider directly.
//!   * **Locked-model dispatch works on slug form** — the v1.16.2
//!     `apply_sampling_params` machinery normalises slug-form names
//!     (strips `provider/` prefix) before pattern matching, so
//!     `openai/o1-mini` correctly strips the locked params. Confirmed
//!     by the parametric test `body_strips_locked_params_for_slug_form`.
//!   * **count_tokens slug-aware** — overridden to strip the
//!     `provider/` prefix before delegating to the unified
//!     [`tokens::count_tokens`] dispatch, so an adopter passing
//!     `openai/gpt-4o-mini` gets the exact `o200k_base` count rather
//!     than the 4-cpt fallback.
//!
//! # Example
//!
//! ```ignore
//! use axon::backends::{openrouter, Backend, ChatRequest, Message};
//!
//! let backend = openrouter::from_env();
//! let request = ChatRequest {
//!     model: "anthropic/claude-sonnet-4-5".into(),
//!     messages: vec![Message::user("Translate to Chinese: Hello, world!")],
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
use super::tokens;
use super::{Backend, Capability, ChatRequest, ChatResponse, ChatStream};

const API_KEY_ENV: &str = "OPENROUTER_API_KEY";

/// OpenRouter multi-provider gateway. Composes
/// [`OpenAICompatibleBackend`] with the OpenRouter preset + slug-aware
/// capability + tokens overrides.
pub struct OpenRouterBackend {
    inner: OpenAICompatibleBackend,
}

impl OpenRouterBackend {
    /// Construct from env. `OPENROUTER_API_KEY` is read at construction
    /// time; `None` is permitted (auth check fires at first call).
    pub fn from_env() -> Self {
        let mut config = OpenAICompatConfig::openrouter();
        config.apply_env_overrides();
        let api_key = env::var(API_KEY_ENV).ok();
        Self {
            inner: OpenAICompatibleBackend::new(config, api_key),
        }
    }

    /// Construct with an explicit API key (or `None`).
    pub fn with_api_key(api_key: Option<String>) -> Self {
        Self {
            inner: OpenAICompatibleBackend::new(
                OpenAICompatConfig::openrouter(),
                api_key,
            ),
        }
    }

    /// v1.18.0 — construct with a per-tenant key + optional explicit
    /// base-URL / chat-path overrides (precedence: explicit > env >
    /// default). `api_key = None` falls back to `OPENROUTER_API_KEY`.
    pub fn with_api_key_and_endpoint(
        api_key: Option<String>,
        base_url: Option<&str>,
        chat_path: Option<&str>,
    ) -> Self {
        let mut config = OpenAICompatConfig::openrouter();
        config.apply_env_overrides();
        config.apply_explicit_overrides(base_url, chat_path);
        let api_key = api_key.or_else(|| config.api_key_env.and_then(|e| env::var(e).ok()));
        Self {
            inner: OpenAICompatibleBackend::new(config, api_key),
        }
    }

    /// Override the base URL (test fixtures, regional OpenRouter
    /// endpoints).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.inner = self.inner.with_base_url(base_url);
        self
    }

    /// Override the default slug. Useful when an adopter pins a
    /// specific model (e.g. `anthropic/claude-haiku-4-5` for cheap
    /// drafting).
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

impl Default for OpenRouterBackend {
    fn default() -> Self {
        Self::from_env()
    }
}

#[async_trait]
impl Backend for OpenRouterBackend {
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
        // Strip the `provider/` prefix and delegate to the unified
        // dispatch, so `openai/gpt-4o-mini` gets the exact
        // `o200k_base` count and `moonshot/kimi-k2.6` gets the
        // exact `cl100k_base` count instead of the 4-cpt fallback
        // that an unrecognised slug would receive.
        let underlying = strip_provider_prefix(model);
        tokens::count_tokens(underlying, text).count
    }

    fn supports(&self, capability: Capability, model: &str) -> bool {
        match capability {
            Capability::Vision => slug_supports_vision(model),
            // LockedParams = true iff the underlying slug matches a
            // locked-model family (Kimi K2.x, OpenAI o1, OpenAI o3).
            // The shared base already consults
            // `locked_model::locked_params_for_model`, which 24.i
            // updated to normalise slug-form names — so this returns
            // the correct answer for `openai/o1-mini`,
            // `moonshot/kimi-k2.6`, etc.
            other => self.inner.supports(other, model),
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  Slug helpers
// ────────────────────────────────────────────────────────────────────

/// Strip the `provider/` prefix from a slug-form model identifier.
/// `openai/gpt-4o-mini` → `gpt-4o-mini`. Returns the input unchanged
/// when there's no `/`.
fn strip_provider_prefix(model: &str) -> &str {
    model.split_once('/').map(|(_, rest)| rest).unwrap_or(model)
}

/// Determine Vision support for an OpenRouter slug. Mirrors the
/// per-provider rules from `openai.rs` / `gemini.rs` / `anthropic.rs`
/// / `glm.rs` / `ollama.rs` so adopters get consistent `supports()`
/// answers regardless of routing.
fn slug_supports_vision(model: &str) -> bool {
    let lc = model.to_lowercase();
    let (provider, name) = match lc.split_once('/') {
        Some((p, n)) => (p, n),
        // Bare model name (no slug) — treat conservatively, default false.
        None => return false,
    };
    match provider {
        // OpenAI: gpt-4o family supports vision; o1 / o3 are text-only.
        "openai" => name.starts_with("gpt-4o"),
        // Anthropic: every Claude 3+ family supports vision (claude-3,
        // claude-3.5, claude-4, claude-haiku-4-5, claude-sonnet-4-5).
        "anthropic" => name.starts_with("claude-"),
        // Google Gemini: 1.5 / 2.0 / 2.5 families are multimodal.
        "google" => name.contains("1.5") || name.contains("2.0") || name.contains("2.5"),
        // Mistral / Meta / Qwen / Microsoft / DeepSeek: only specific
        // multimodal SKUs. Match on documented family names.
        "meta" | "meta-llama" => name.contains("llama-3.2-vision") || name.contains("llava"),
        "qwen" => name.contains("vl"),
        "microsoft" => name.contains("phi-3.5-vision") || name.contains("phi-4-vision"),
        // GLM: 4v family is multimodal.
        "zhipu" | "glm" | "z-ai" => name.starts_with("glm-4v"),
        // Mistral: pixtral family supports vision.
        "mistralai" | "mistral" => name.contains("pixtral"),
        // Conservative default — explicit per-family list above.
        _ => false,
    }
}

// ────────────────────────────────────────────────────────────────────
//  Module-level factories
// ────────────────────────────────────────────────────────────────────

/// Construct an OpenRouter backend using the `OPENROUTER_API_KEY` env var.
pub fn from_env() -> OpenRouterBackend {
    OpenRouterBackend::from_env()
}

/// Construct an OpenRouter backend with an explicit API key (or `None`).
pub fn with_api_key(api_key: Option<String>) -> OpenRouterBackend {
    OpenRouterBackend::with_api_key(api_key)
}

#[allow(dead_code)]
type OpenRouterChatStream =
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
    fn from_env_constructs_openrouter_backend() {
        let b = OpenRouterBackend::from_env();
        assert_eq!(b.name(), "openrouter");
        assert_eq!(b.default_model(), "openai/gpt-4o-mini");
    }

    #[test]
    fn module_factory_from_env_works() {
        let b = from_env();
        assert_eq!(b.name(), "openrouter");
    }

    #[test]
    fn module_factory_with_api_key_explicit() {
        let b = with_api_key(Some("sk-or-v1-test".into()));
        assert_eq!(b.name(), "openrouter");
    }

    #[test]
    fn with_default_model_overrides() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()))
            .with_default_model("anthropic/claude-haiku-4-5");
        assert_eq!(b.default_model(), "anthropic/claude-haiku-4-5");
    }

    #[test]
    fn with_base_url_overrides_for_test_fixtures() {
        let _b = OpenRouterBackend::with_api_key(Some("k".into()))
            .with_base_url("http://localhost:9999");
    }

    #[test]
    fn inner_accessor_returns_compat_backend() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        assert_eq!(b.inner().name(), "openrouter");
    }

    #[test]
    fn default_constructs_via_from_env() {
        let b = OpenRouterBackend::default();
        assert_eq!(b.name(), "openrouter");
    }

    // ── Slug helpers ────────────────────────────────────────────────

    #[test]
    fn strip_provider_prefix_returns_model_only() {
        assert_eq!(strip_provider_prefix("openai/gpt-4o-mini"), "gpt-4o-mini");
        assert_eq!(
            strip_provider_prefix("anthropic/claude-sonnet-4-5"),
            "claude-sonnet-4-5"
        );
        assert_eq!(strip_provider_prefix("moonshot/kimi-k2.6"), "kimi-k2.6");
    }

    #[test]
    fn strip_provider_prefix_idempotent_for_bare_names() {
        assert_eq!(strip_provider_prefix("gpt-4o-mini"), "gpt-4o-mini");
        assert_eq!(strip_provider_prefix(""), "");
    }

    // ── Capability: Vision dispatch by underlying model ─────────────

    #[test]
    fn supports_vision_for_openai_gpt_4o_slug() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        assert!(b.supports(Capability::Vision, "openai/gpt-4o-mini"));
        assert!(b.supports(Capability::Vision, "openai/gpt-4o-2024-08-06"));
    }

    #[test]
    fn does_not_support_vision_for_openai_o1_o3_slugs() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        assert!(!b.supports(Capability::Vision, "openai/o1-mini"));
        assert!(!b.supports(Capability::Vision, "openai/o3"));
        assert!(!b.supports(Capability::Vision, "openai/o3-mini"));
    }

    #[test]
    fn supports_vision_for_anthropic_claude_slugs() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        assert!(b.supports(Capability::Vision, "anthropic/claude-sonnet-4-5"));
        assert!(b.supports(Capability::Vision, "anthropic/claude-haiku-4-5"));
        assert!(b.supports(Capability::Vision, "anthropic/claude-3-5-sonnet"));
    }

    #[test]
    fn supports_vision_for_google_gemini_15_20_25_slugs() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        assert!(b.supports(Capability::Vision, "google/gemini-1.5-pro"));
        assert!(b.supports(Capability::Vision, "google/gemini-2.0-flash"));
        assert!(b.supports(Capability::Vision, "google/gemini-2.5-pro"));
        assert!(b.supports(Capability::Vision, "google/gemini-2.5-flash"));
    }

    #[test]
    fn does_not_support_vision_for_legacy_gemini_pro() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        assert!(!b.supports(Capability::Vision, "google/gemini-pro"));
        assert!(!b.supports(Capability::Vision, "google/gemini-1.0-pro"));
    }

    #[test]
    fn supports_vision_for_meta_llama_vision_and_llava() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        assert!(b.supports(Capability::Vision, "meta-llama/llama-3.2-vision-11b"));
        assert!(b.supports(Capability::Vision, "meta-llama/llava-llama-3"));
    }

    #[test]
    fn does_not_support_vision_for_text_only_meta_llama() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        assert!(!b.supports(Capability::Vision, "meta-llama/llama-3.1-70b-instruct"));
        assert!(!b.supports(Capability::Vision, "meta-llama/llama-3.3-70b-instruct"));
    }

    #[test]
    fn supports_vision_for_qwen_vl_slugs() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        assert!(b.supports(Capability::Vision, "qwen/qwen2-vl-7b-instruct"));
        assert!(b.supports(Capability::Vision, "qwen/qwen2.5-vl-72b-instruct"));
    }

    #[test]
    fn supports_vision_for_mistral_pixtral() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        assert!(b.supports(Capability::Vision, "mistralai/pixtral-12b-2409"));
    }

    #[test]
    fn does_not_support_vision_for_text_only_mistral() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        assert!(!b.supports(Capability::Vision, "mistralai/mistral-large"));
    }

    #[test]
    fn does_not_support_vision_for_bare_model_name() {
        // A bare model without a slug is conservative-default false —
        // OpenRouter API expects the slug form so this case is unusual
        // anyway, but the dispatch must not crash.
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        assert!(!b.supports(Capability::Vision, "gpt-4o-mini"));
    }

    #[test]
    fn does_not_support_vision_for_unknown_provider() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        assert!(!b.supports(Capability::Vision, "newprovider/exotic-model-7b"));
    }

    // ── LockedParams via slug normalisation in shared base ──────────

    #[test]
    fn supports_lockedparams_for_openai_o1_o3_slugs() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        assert!(b.supports(Capability::LockedParams, "openai/o1-mini"));
        assert!(b.supports(Capability::LockedParams, "openai/o3"));
        assert!(b.supports(Capability::LockedParams, "openai/o3-mini"));
    }

    #[test]
    fn supports_lockedparams_for_moonshot_kimi_k2_slug() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        assert!(b.supports(Capability::LockedParams, "moonshot/kimi-k2.6"));
        assert!(b.supports(Capability::LockedParams, "moonshot/kimi-k2.8"));
    }

    #[test]
    fn does_not_support_lockedparams_for_chat_slugs() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        assert!(!b.supports(Capability::LockedParams, "openai/gpt-4o-mini"));
        assert!(!b.supports(Capability::LockedParams, "anthropic/claude-sonnet-4-5"));
        assert!(!b.supports(Capability::LockedParams, "google/gemini-2.5-pro"));
    }

    // ── Body builder strips locked params on slug form ──────────────

    #[test]
    fn body_strips_locked_params_for_slug_form_o1() {
        // The Kivi v1.16.2 incident, but routed through OpenRouter:
        // adopter sends `openai/o1-mini` with temperature; the shared
        // base must strip it before forwarding to OpenRouter (which
        // forwards to OpenAI, which would otherwise return HTTP 400).
        let mut req = req_with(vec![Message::user("hi")]);
        req.model = "openai/o1-mini".into();
        req.temperature = Some(0.7);
        let body = build_request_body(&req, "openai/gpt-4o-mini", false);
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn body_strips_locked_params_for_slug_form_kimi_k2() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.model = "moonshot/kimi-k2.6".into();
        req.temperature = Some(0.5);
        req.top_p = Some(0.9);
        let body = build_request_body(&req, "openai/gpt-4o-mini", false);
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
    }

    #[test]
    fn body_keeps_sampling_params_for_unlocked_slug() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.model = "openai/gpt-4o-mini".into();
        req.temperature = Some(0.5);
        let body = build_request_body(&req, "openai/gpt-4o-mini", false);
        assert_eq!(body["temperature"], 0.5);
    }

    // ── count_tokens slug-aware ─────────────────────────────────────

    #[test]
    fn count_tokens_uses_o200k_for_openai_gpt_4o_slug() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        // `openai/gpt-4o-mini` → strip → `gpt-4o-mini` → o200k_base.
        let n = b.count_tokens("openai/gpt-4o-mini", "hello world");
        // Exact tokenizer reports 1-5 tokens for "hello world".
        assert!(n > 0);
        assert!(n <= 5);
    }

    #[test]
    fn count_tokens_uses_cl100k_for_moonshot_slug() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        // `moonshot/kimi-k2.6` → strip → `kimi-k2.6` → cl100k_base.
        let n = b.count_tokens("moonshot/kimi-k2.6", "hello world");
        assert!(n > 0);
    }

    #[test]
    fn count_tokens_uses_estimate_for_anthropic_slug() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        // Claude has no offline tokenizer in tiktoken-rs → estimate.
        // 8 chars → 2 tokens.
        assert_eq!(b.count_tokens("anthropic/claude-sonnet-4-5", "ABCDEFGH"), 2);
    }

    // ── Streaming surface ───────────────────────────────────────────

    #[tokio::test]
    async fn stream_delegates_to_base_real_sse_implementation() {
        // v1.24.0 — OpenRouter delegates to OpenAI-compat which now
        // ships a real SSE streamer; unreachable-port test exercises
        // the transport-error path.
        let b = OpenRouterBackend::with_api_key(Some("k".into()))
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
        let b =
            OpenRouterBackend::with_api_key(None).with_base_url("http://127.0.0.1:0");
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

    // ── Standard caps via base ──────────────────────────────────────

    #[test]
    fn supports_streaming_tooluse_structured_via_base() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        let any = "openai/gpt-4o-mini";
        assert!(b.supports(Capability::Streaming, any));
        assert!(b.supports(Capability::ToolUse, any));
        assert!(b.supports(Capability::StructuredOutput, any));
    }

    #[test]
    fn does_not_support_anthropic_or_gemini_only_caps() {
        let b = OpenRouterBackend::with_api_key(Some("k".into()));
        let any = "openai/gpt-4o-mini";
        assert!(!b.supports(Capability::PromptCaching, any));
        assert!(!b.supports(Capability::SafetySettings, any));
    }
}
