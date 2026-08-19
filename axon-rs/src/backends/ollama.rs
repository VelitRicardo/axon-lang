//! Ollama (local) backend — v1.18.0.
//!
//! Thin factory + capability override on top of [`OpenAICompatibleBackend`].
//! Ollama exposes an OpenAI-compatible chat-completions surface at
//! `http://localhost:11434/v1/chat/completions` with **no
//! authentication** — the daemon listens on localhost only, so the
//! shared base's `api_key_env: None` path applies (no `Authorization`
//! header sent). This module adds Ollama-specific surface:
//!
//!   * [`from_env`] / [`with_api_key`] / [`local`] factories that pin
//!     [`OpenAICompatConfig::ollama`] (base URL `http://localhost:11434`,
//!     default model `llama3.1:8b`, no env var).
//!   * **Vision** = `true` for the documented multimodal families
//!     (Llava, BakLlava, Llama 3.2 Vision, Qwen-VL, MiniCPM-V).
//!     Other models report `false`.
//!   * **No auth** — calling [`from_env`] with `OLLAMA_HOST` set
//!     overrides the base URL (the standard Ollama env convention).
//!     Adopters running a remote / containerised Ollama instance can
//!     also pass an API key (some proxies layer auth in front of the
//!     daemon); the shared base then sends it as a Bearer header,
//!     which Ollama itself ignores but proxies can validate.
//!
//! # Streaming
//!
//! v1.24.0 — Ollama streams via its OpenAI-compatible surface
//! (`POST <base>/v1/chat/completions` with `stream: true`), inheriting
//! the SSE wire format directly from the shared
//! [`OpenAICompatibleBackend::stream`] implementation. Ollama also
//! exposes a native ndjson surface at `/api/chat` for adopters that
//! want the provider-native protocol; that route is a 33.x follow-up
//! and not needed for in-tree adopter consumption since the OpenAI-
//! compat path covers every model the daemon serves.
//!
//! # Example
//!
//! ```ignore
//! use axon::backends::{ollama, Backend, ChatRequest, Message};
//!
//! // Local Ollama daemon at the default port.
//! let backend = ollama::local();
//! let request = ChatRequest {
//!     model: "llama3.1:8b".into(),
//!     messages: vec![Message::user("Hello!")],
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

/// `OLLAMA_HOST` — the standard Ollama env var that adopters set when
/// the daemon listens on a non-default port (or when running inside
/// containers with port forwarding). Read at construction time;
/// applied as a base URL override when present.
const OLLAMA_HOST_ENV: &str = "OLLAMA_HOST";

/// Optional API key env (proxies in front of Ollama may layer auth).
/// Ollama itself ignores Bearer headers but a proxy fronting the
/// daemon can validate them.
const API_KEY_ENV: &str = "OLLAMA_API_KEY";

/// Local-only Ollama daemon backend. Composes
/// [`OpenAICompatibleBackend`] with the Ollama preset + a capability
/// override for Vision (true on documented multimodal families).
pub struct OllamaBackend {
    inner: OpenAICompatibleBackend,
}

impl OllamaBackend {
    /// Construct from env. Honours two env vars:
    ///
    ///   * `OLLAMA_HOST` — base URL override (e.g.
    ///     `http://my-host:11434`). Default: `http://localhost:11434`.
    ///   * `OLLAMA_API_KEY` — optional Bearer token for proxies fronting
    ///     the daemon. Ollama itself doesn't authenticate; the header
    ///     is forwarded by the shared base if set.
    pub fn from_env() -> Self {
        let api_key = env::var(API_KEY_ENV).ok();
        let host = env::var(OLLAMA_HOST_ENV).ok();
        let mut backend = Self::with_api_key(api_key);
        if let Some(host) = host {
            backend = backend.with_base_url(host);
        }
        backend
    }

    /// Construct with an explicit API key (or `None`). Use [`local`] /
    /// [`Self::local`] for the most common case (no key, no host
    /// override).
    pub fn with_api_key(api_key: Option<String>) -> Self {
        Self {
            inner: OpenAICompatibleBackend::new(OpenAICompatConfig::ollama(), api_key),
        }
    }

    /// Construct a local-only backend pointing at `http://localhost:11434`
    /// with no authentication. Equivalent to `with_api_key(None)`.
    pub fn local() -> Self {
        Self::with_api_key(None)
    }

    /// v1.18.0 — construct with a per-tenant key + optional explicit
    /// base-URL / chat-path overrides. Precedence: **explicit (per-tenant)
    /// > `OLLAMA_HOST` env > localhost default.** Lets a tenant point at a
    /// remote / containerised Ollama without a code change.
    pub fn with_api_key_and_endpoint(
        api_key: Option<String>,
        base_url: Option<&str>,
        chat_path: Option<&str>,
    ) -> Self {
        let mut config = OpenAICompatConfig::ollama();
        // env tier — Ollama's own `OLLAMA_HOST` base override.
        let host = env::var(OLLAMA_HOST_ENV).ok();
        config.apply_explicit_overrides(host.as_deref(), None);
        // per-tenant tier (wins over env).
        config.apply_explicit_overrides(base_url, chat_path);
        let api_key = api_key.or_else(|| env::var(API_KEY_ENV).ok());
        Self {
            inner: OpenAICompatibleBackend::new(config, api_key),
        }
    }

    /// Override the base URL (test fixtures, remote Ollama instances,
    /// containerised deployments). Returns `self` for builder chaining.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.inner = self.inner.with_base_url(base_url);
        self
    }

    /// Override the default model. Useful when a host has a different
    /// model pulled (e.g. `qwen2.5:14b`, `mistral-small:24b`).
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

impl Default for OllamaBackend {
    fn default() -> Self {
        Self::from_env()
    }
}

#[async_trait]
impl Backend for OllamaBackend {
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
        // v1.24.0 — Ollama streams via its OpenAI-compatible
        // /v1/chat/completions surface; the shared
        // OpenAICompatibleBackend::stream impl handles the SSE wire
        // shape. Ollama-native ndjson (/api/chat) is a 33.x follow-up
        // that isn't needed for in-tree adopter consumption — every
        // model the daemon serves is reachable via OpenAI-compat SSE.
        self.inner.stream(request).await
    }

    fn count_tokens(&self, model: &str, text: &str) -> usize {
        // The unified dispatch uses the 4-cpt offline estimate for
        // Ollama models (no offline tokenizer in tiktoken-rs covers
        // Llama / Mistral / Qwen / Phi). Adopters needing exact counts
        // can call the Ollama HTTP `/api/tokenize` endpoint directly;
        // future revisions of this method may delegate there once an
        // async trait method is acceptable on the public surface.
        self.inner.count_tokens(model, text)
    }

    fn supports(&self, capability: Capability, model: &str) -> bool {
        match capability {
            // Multimodal Ollama families: Llava (1.5/1.6/Next), BakLlava,
            // Llama 3.2 Vision, Qwen2-VL / Qwen2.5-VL, MiniCPM-V.
            // Conservative case-insensitive substring match — adopters
            // pull model names like `llava:7b`, `llama3.2-vision:11b`,
            // `qwen2-vl:7b`. New multimodal families can be added by
            // extending the match list.
            Capability::Vision => is_known_multimodal(model),
            // Ollama has no documented locked-param families.
            Capability::LockedParams => false,
            // Streaming / ToolUse / StructuredOutput delegate to base.
            // Note: tool calling support varies by model in Ollama
            // (works on Llama 3.1+, Mistral Small, etc.; not every
            // local model supports it). Reporting `true` here matches
            // the shared base's behaviour and aligns with the OpenAI-
            // compat wire surface — the daemon returns an error if the
            // pulled model can't tool-call, which the existing typed
            // error path surfaces cleanly.
            other => self.inner.supports(other, model),
        }
    }
}

/// True iff `model` matches one of the documented Ollama multimodal
/// families. Case-insensitive substring match — Ollama tags models by
/// their pull slug (e.g. `llava:7b`, `llama3.2-vision:11b`).
fn is_known_multimodal(model: &str) -> bool {
    let lc = model.to_lowercase();
    lc.contains("llava")
        || lc.contains("bakllava")
        || lc.contains("llama3.2-vision")
        || lc.contains("llama-3.2-vision")
        || lc.contains("qwen2-vl")
        || lc.contains("qwen2.5-vl")
        || lc.contains("minicpm-v")
}

// ────────────────────────────────────────────────────────────────────
//  Module-level factories
// ────────────────────────────────────────────────────────────────────

/// Construct an Ollama backend reading `OLLAMA_HOST` + `OLLAMA_API_KEY`
/// from env.
pub fn from_env() -> OllamaBackend {
    OllamaBackend::from_env()
}

/// Construct a local-only Ollama backend at `http://localhost:11434`
/// with no authentication. Equivalent to `OllamaBackend::local()`.
pub fn local() -> OllamaBackend {
    OllamaBackend::local()
}

/// Construct an Ollama backend with an explicit API key (for proxy
/// deployments fronting the daemon).
pub fn with_api_key(api_key: Option<String>) -> OllamaBackend {
    OllamaBackend::with_api_key(api_key)
}

#[allow(dead_code)]
type OllamaChatStream =
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
    fn local_constructs_ollama_backend() {
        let b = OllamaBackend::local();
        assert_eq!(b.name(), "ollama");
        assert_eq!(b.default_model(), "llama3.1:8b");
    }

    #[test]
    fn module_factory_local_works() {
        let b = local();
        assert_eq!(b.name(), "ollama");
    }

    #[test]
    fn module_factory_from_env_works() {
        let b = from_env();
        assert_eq!(b.name(), "ollama");
    }

    #[test]
    fn module_factory_with_api_key_explicit() {
        let b = with_api_key(Some("proxy-token".into()));
        assert_eq!(b.name(), "ollama");
    }

    #[test]
    fn with_default_model_overrides() {
        let b = OllamaBackend::local().with_default_model("qwen2.5:14b");
        assert_eq!(b.default_model(), "qwen2.5:14b");
    }

    #[test]
    fn with_base_url_overrides_for_test_fixtures() {
        let _b = OllamaBackend::local().with_base_url("http://remote-host:11435");
    }

    #[test]
    fn inner_accessor_returns_compat_backend() {
        let b = OllamaBackend::local();
        assert_eq!(b.inner().name(), "ollama");
    }

    // ── Capability: Vision dispatch ─────────────────────────────────

    #[test]
    fn supports_vision_for_llava_family() {
        let b = OllamaBackend::local();
        assert!(b.supports(Capability::Vision, "llava"));
        assert!(b.supports(Capability::Vision, "llava:7b"));
        assert!(b.supports(Capability::Vision, "llava:13b"));
        assert!(b.supports(Capability::Vision, "llava-llama3:8b"));
    }

    #[test]
    fn supports_vision_for_bakllava() {
        let b = OllamaBackend::local();
        assert!(b.supports(Capability::Vision, "bakllava"));
        assert!(b.supports(Capability::Vision, "bakllava:7b"));
    }

    #[test]
    fn supports_vision_for_llama_3_2_vision() {
        let b = OllamaBackend::local();
        assert!(b.supports(Capability::Vision, "llama3.2-vision:11b"));
        assert!(b.supports(Capability::Vision, "llama3.2-vision:90b"));
        // Hyphenated alias (some pull slugs use llama-3.2-vision).
        assert!(b.supports(Capability::Vision, "llama-3.2-vision"));
    }

    #[test]
    fn supports_vision_for_qwen_vl() {
        let b = OllamaBackend::local();
        assert!(b.supports(Capability::Vision, "qwen2-vl:7b"));
        assert!(b.supports(Capability::Vision, "qwen2.5-vl:7b"));
    }

    #[test]
    fn supports_vision_for_minicpm_v() {
        let b = OllamaBackend::local();
        assert!(b.supports(Capability::Vision, "minicpm-v:8b"));
    }

    #[test]
    fn does_not_support_vision_for_text_only_models() {
        let b = OllamaBackend::local();
        assert!(!b.supports(Capability::Vision, "llama3.1:8b"));
        assert!(!b.supports(Capability::Vision, "llama3.1:70b"));
        assert!(!b.supports(Capability::Vision, "mistral-small:24b"));
        assert!(!b.supports(Capability::Vision, "qwen2.5:14b"));
        assert!(!b.supports(Capability::Vision, "phi-4"));
        assert!(!b.supports(Capability::Vision, "deepseek-r1:32b"));
    }

    #[test]
    fn vision_is_case_insensitive() {
        let b = OllamaBackend::local();
        assert!(b.supports(Capability::Vision, "LLaVA:7b"));
        assert!(b.supports(Capability::Vision, "LLAMA3.2-VISION:11b"));
    }

    // ── Capability: standard dispatch ───────────────────────────────

    #[test]
    fn supports_streaming_tooluse_structured_via_base() {
        let b = OllamaBackend::local();
        let any_model = "llama3.1:8b";
        assert!(b.supports(Capability::Streaming, any_model));
        assert!(b.supports(Capability::ToolUse, any_model));
        assert!(b.supports(Capability::StructuredOutput, any_model));
    }

    #[test]
    fn does_not_support_anthropic_or_gemini_only_caps() {
        let b = OllamaBackend::local();
        let any_model = "llama3.1:8b";
        assert!(!b.supports(Capability::PromptCaching, any_model));
        assert!(!b.supports(Capability::SafetySettings, any_model));
        assert!(!b.supports(Capability::LockedParams, any_model));
    }

    // ── count_tokens delegates to estimate ──────────────────────────

    #[test]
    fn count_tokens_uses_estimate_for_ollama_models() {
        let b = OllamaBackend::local();
        // No offline tokenizer for Llama / Mistral / Qwen in tiktoken-rs;
        // dispatch returns the 4-cpt estimate. 8 chars → 2 tokens.
        assert_eq!(b.count_tokens("llama3.1:8b", "ABCDEFGH"), 2);
    }

    // ── Streaming surface ───────────────────────────────────────────

    #[tokio::test]
    async fn stream_delegates_to_base_real_sse_implementation() {
        // v1.24.0 — Ollama streams via OpenAI-compat SSE; unreachable
        // port exercises the transport-error path.
        let b = OllamaBackend::local().with_base_url("http://127.0.0.1:1");
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

    // ── No-auth path: no Auth error when pointing at unreachable URL ───

    #[tokio::test]
    async fn complete_without_api_key_does_not_return_auth_error() {
        // Ollama is local-only — no API key required. The complete()
        // path must NOT short-circuit with an Auth error; it should
        // get to the HTTP layer (which fails because we point at an
        // unreachable port), surfacing as a transport-layer Generic.
        let b = OllamaBackend::local()
            .with_base_url("http://127.0.0.1:0")
            .with_default_model("llama3.1:8b");
        // Override retry policy to fail-fast for the test.
        let inner = b.inner();
        let _ = inner; // exercised via the call below
        let err = b
            .complete(ChatRequest {
                messages: vec![Message::user("hi")],
                ..Default::default()
            })
            .await
            .unwrap_err();
        // Critical assertion: NOT an Auth error.
        assert!(
            !matches!(err, BackendError::Auth { .. }),
            "Ollama must not require an API key; got Auth error: {err:?}"
        );
    }

    // ── Default trait works ─────────────────────────────────────────

    #[test]
    fn default_constructs_via_from_env() {
        let b = OllamaBackend::default();
        assert_eq!(b.name(), "ollama");
    }

    // ── is_known_multimodal helper coverage ─────────────────────────

    #[test]
    fn is_known_multimodal_recognises_all_documented_families() {
        for model in &[
            "llava",
            "llava:7b",
            "bakllava:7b",
            "llama3.2-vision:11b",
            "llama-3.2-vision",
            "qwen2-vl:7b",
            "qwen2.5-vl:7b",
            "minicpm-v:8b",
        ] {
            assert!(is_known_multimodal(model), "{model} should be multimodal");
        }
    }

    #[test]
    fn is_known_multimodal_rejects_text_only_families() {
        for model in &[
            "llama3.1:8b",
            "mistral:7b",
            "qwen2.5:14b",
            "phi-4",
            "deepseek-r1:32b",
            "gemma3:12b",
        ] {
            assert!(!is_known_multimodal(model), "{model} should not be multimodal");
        }
    }
}
