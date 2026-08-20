//! Native Rust LLM backends — v1.18.0.
//!
//! Per-provider async backends consumed by the algebraic-effects
//! runtime (v1.17.0) and, in upcoming v1.19.0+, the general flow
//! executor. Each provider lives in its own submodule:
//!
//! * [`anthropic`] — Claude Messages API (v1.18.0)
//! * [`openai`] — GPT chat/completions (v1.18.0)
//! * [`gemini`] — Google generateContent (v1.18.0)
//! * [`kimi`] — Moonshot K2.x (v1.18.0, locked params)
//! * [`glm`] — Zhipu GLM-4.x (v1.18.0)
//! * [`ollama`] — local LLMs via REST (v1.18.0)
//! * [`openrouter`] — multi-provider gateway (v1.18.0)
//!
//! Shared infrastructure ships in 24.b alongside the trait + Registry:
//!
//!   * [`error`]            — typed transport errors named per failure mode
//!   * [`retry`]            — retry policy + `Retry-After` parsing
//!   * [`observability`]    — tracing span helpers per call lifecycle
//!   * [`locked_model`]     — locked-parameter dispatch (Kimi K2.x / o1 / o3)
//!   * [`tokens`]           — unified `count_tokens` dispatch by model prefix
//!
//! Adopter usage (post-24.k):
//!
//! ```ignore
//! use axon::backends::{Registry, ChatRequest, Message, Role};
//!
//! let registry = Registry::production();
//! let backend = registry.get("anthropic").expect("anthropic registered");
//!
//! let req = ChatRequest {
//!     model: "claude-sonnet-4-5".into(),
//!     messages: vec![Message::user("Hello!")],
//!     ..Default::default()
//! };
//! let response = backend.complete(req).await?;
//! println!("{}", response.content);
//! ```
//!
//! # Architecture decisions (see the design plan)
//!
//! * **D1** — `async_trait` over native async-fn-in-trait so `dyn Backend`
//!   stays object-safe (Registry uses `HashMap<String, Box<dyn Backend>>`).
//! * **D6** — the legacy [`crate::backend`] module stays in place during
//!   24.b–24.i to avoid touching 200+ call sites; in 24.j it becomes a
//!   thin re-export shim that delegates here.
//! * **D7** — Python `axon/backends/*.py` is untouched; flows running on
//!   the Python runtime keep using it.

#![allow(dead_code)]

use std::collections::HashMap;
use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

pub mod anthropic;
pub mod error;
pub mod gemini;
pub mod glm;
pub mod kimi;
pub mod locked_model;
pub mod model_catalog;
pub mod observability;
pub mod ollama;
pub mod openai;
pub mod openai_compat;
pub mod openrouter;
pub mod retry;
pub mod sse_streaming;
/// v1.24.0 — `StubBackend` implementing the [`Backend`] trait so
/// the production async streaming path resolves "stub" through the
/// uniform [`Registry`] surface (no special-cased branches in the
/// runtime). Excluded from the v1.18.0 cross-stack drift gate
/// SHARED_INFRA_MODULES because it is not a real provider.
pub mod stub;
pub mod tokens;
pub(crate) mod transport;

pub use anthropic::AnthropicBackend;
pub use error::{categorise_http, BackendError};
pub use gemini::GeminiBackend;
pub use glm::GLMBackend;
pub use kimi::KimiBackend;
pub use ollama::OllamaBackend;
pub use openai::OpenAIBackend;
pub use openai_compat::{OpenAICompatConfig, OpenAICompatibleBackend};
pub use openrouter::OpenRouterBackend;
pub use stub::{StubBackend, STUB_CONTENT, STUB_DEFAULT_MODEL, STUB_PROVIDER_NAME};

// ────────────────────────────────────────────────────────────────────
//  Request / Response types — the wire shape every backend speaks
// ────────────────────────────────────────────────────────────────────

/// Role of a message in a chat conversation.
///
/// Mirrors the OpenAI ChatML enumeration with one provider-neutral
/// addition (`Tool`) used for tool-call result messages. Per-provider
/// adapters translate this enum to the wire encoding that provider
/// expects (e.g. Anthropic's `system` becomes a top-level field, not a
/// message; Gemini uses `user`/`model`/`function`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

/// One chat message in a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
    /// Optional tool-call identifier when role == Tool. Per-provider
    /// adapters thread this back to the correct tool call ID.
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: Role::User, content: content.into(), tool_call_id: None }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: Role::Assistant, content: content.into(), tool_call_id: None }
    }
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: Role::System, content: content.into(), tool_call_id: None }
    }
}

/// A tool the model may invoke during the response.
///
/// `parameters_json` is the JSON Schema describing the parameter shape;
/// each provider serialises it with its own envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters_json: String,
}

/// Provider-feature discovery enum — used by [`Backend::supports`] so
/// adopters can ask "does this backend support X for this model?"
/// without parsing model strings themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    Streaming,
    ToolUse,
    Vision,
    /// Anthropic prompt caching (cache_control breakpoints).
    PromptCaching,
    /// Gemini safetySettings on the request body.
    SafetySettings,
    /// OpenAI structured outputs (response_format=json_schema).
    StructuredOutput,
    /// Provider hard-codes sampling parameters (Kimi K2.x, o1, o3).
    LockedParams,
}

/// One canonical chat request — provider-neutral. Per-provider adapters
/// translate to the wire JSON the provider expects.
#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    /// Empty string → backend uses its `default_model()`.
    pub model: String,
    pub messages: Vec<Message>,
    /// System prompt — Anthropic puts it in a top-level field; OpenAI &
    /// compats prepend a system message to the messages array.
    pub system: Option<String>,
    pub max_tokens: Option<u32>,
    /// Temperature; ignored when the resolved model is locked-params.
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub tools: Vec<ToolSpec>,
    /// `false` → call `complete()`. `true` → call `stream()` and consume
    /// the chunk stream incrementally.
    pub stream: bool,
    /// v2.83.0 (Tier 1) — per-token logit bias, token_id → bias in the
    /// OpenAI wire range [-100, 100]. The mandate engine uses −100 BANS for
    /// `Absent` needles: the probability mass of a forbidden lexeme is
    /// collapsed before sampling. Emitted only on the OpenAI-compat wire;
    /// locked-param models (o1/o3, Kimi K2.x) strip it via
    /// `locked_model::apply_sampling_params`, and providers with no such
    /// knob ignore the field entirely — Tier 2 (the validation loop)
    /// carries the guarantee there.
    pub logit_bias: Option<std::collections::HashMap<u32, i32>>,
    /// Trace ID propagated from the calling flow step. Surfaces in
    /// tracing spans so log lines correlate.
    pub trace_id: Option<String>,
    /// v1.24.0 — Cancellation flag observed INSIDE the reqwest
    /// body. Each per-provider `Backend::stream()` impl wraps its
    /// returned chunk stream with `sse_streaming::cancel_aware`
    /// so the next-chunk poll races a `cancel.cancelled()` future
    /// against the upstream HTTP body — when the flag fires the
    /// stream returns `None` within ≤100ms p95 + the dropped
    /// reqwest Response aborts the upstream HTTP request body.
    ///
    /// `Default` is an uncancelled flag; adopters that don't supply
    /// one get pre-33.x.e semantics (the stream runs to completion).
    /// Cloning is cheap (`Arc`-backed inside).
    pub cancel: crate::cancel_token::CancellationFlag,
}

/// How the model decided to stop generating.
///
/// Maps the provider-specific finish-reason strings to a closed enum
/// callers can `match` on. Unmapped values land in `Other(s)` so the
/// raw string is still recoverable for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishReason {
    /// Natural end of generation (Anthropic `end_turn`, OpenAI `stop`,
    /// Gemini `STOP`).
    Stop,
    /// Hit `max_tokens` budget (Anthropic `max_tokens`, OpenAI `length`,
    /// Gemini `MAX_TOKENS`).
    Length,
    /// Model invoked a tool (Anthropic `tool_use`, OpenAI `tool_calls`).
    ToolUse,
    /// Provider's content filter blocked output (OpenAI `content_filter`,
    /// Gemini `SAFETY`, Anthropic empty + `end_turn`).
    SafetyBreach,
    /// Anything else; carries the raw provider string.
    Other(String),
}

impl FinishReason {
    /// Map a raw provider string into the enum.
    pub fn from_provider(provider: &str, raw: &str) -> Self {
        let lc = raw.to_ascii_lowercase();
        match (provider, lc.as_str()) {
            ("anthropic", "end_turn") => Self::Stop,
            ("anthropic", "max_tokens") => Self::Length,
            ("anthropic", "tool_use") => Self::ToolUse,
            ("anthropic", "stop_sequence") => Self::Stop,
            (_, "stop") => Self::Stop,
            (_, "length") => Self::Length,
            (_, "tool_calls") | (_, "function_call") => Self::ToolUse,
            (_, "content_filter") => Self::SafetyBreach,
            // Gemini uses upper-case slugs.
            (_, "max_tokens") => Self::Length,
            (_, "safety") => Self::SafetyBreach,
            (_, "") => Self::Other(String::new()),
            _ => Self::Other(raw.to_string()),
        }
    }

    /// True iff this finish reason is the provider's safety classifier
    /// blocking output. Used by `BackendError::SafetyBreach` lifting.
    pub fn is_safety_breach(&self) -> bool {
        matches!(self, Self::SafetyBreach)
    }
}

/// Token-usage breakdown returned by the provider. Field naming is
/// canonical (input/output/total); per-provider deltas (cache reads on
/// Anthropic, reasoning tokens on o1/o3) live in dedicated fields so
/// aggregating dashboards across providers stay coherent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    /// Anthropic prompt-cache hit (`cache_read_input_tokens`).
    pub cache_read_tokens: u32,
    /// Anthropic prompt-cache write (`cache_creation_input_tokens`).
    pub cache_creation_tokens: u32,
    /// OpenAI o1/o3 reasoning-token allocation (`reasoning_tokens`).
    pub reasoning_tokens: u32,
}

/// A complete chat response from a non-streaming `complete()` call.
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    /// Resolved model slug — what the provider actually returned the
    /// response from (may differ from request when an alias was sent).
    pub model_name: String,
    /// Provider name (`"anthropic"`, `"openai"`, etc.).
    pub provider_name: String,
    pub finish_reason: FinishReason,
    pub usage: Usage,
    /// Number of retries that fired before this success. 0 on a clean
    /// first-attempt response.
    pub retry_count: u32,
    /// Trace ID echoed back from the request (or auto-generated if the
    /// request omitted one).
    pub trace_id: String,
}

/// One delta in a streaming response.
///
/// `delta` is the incremental text fragment for this chunk. `finish_reason`
/// + `usage` are populated only on the final chunk so consumers can
/// compute totals without keeping a running tally.
#[derive(Debug, Clone, Default)]
pub struct ChatChunk {
    pub delta: String,
    pub finish_reason: Option<FinishReason>,
    pub usage: Option<Usage>,
}

/// Pinned, boxed stream alias — the concrete return type of
/// [`Backend::stream`]. Adopters consume via `futures::StreamExt`.
pub type ChatStream =
    Pin<Box<dyn Stream<Item = Result<ChatChunk, BackendError>> + Send>>;

// ────────────────────────────────────────────────────────────────────
//  Backend trait — the per-provider contract
// ────────────────────────────────────────────────────────────────────

/// One LLM provider's native Rust client.
///
/// Implementors live in `axon-rs/src/backends/<provider>.rs` and are
/// registered into [`Registry`] at process startup. The trait is
/// object-safe (D1 — uses `async_trait`) so registries can hold
/// `Box<dyn Backend>` for runtime dispatch by name.
#[async_trait]
pub trait Backend: Send + Sync {
    /// Short provider name used as the registry key.
    /// E.g. `"anthropic"`, `"openai"`, `"kimi"`.
    fn name(&self) -> &str;

    /// Default model used when [`ChatRequest::model`] is empty.
    fn default_model(&self) -> &str;

    /// Synchronous-result chat completion (non-streaming).
    async fn complete(&self, request: ChatRequest) -> Result<ChatResponse, BackendError>;

    /// Streaming chat completion. Adopter consumes the returned stream;
    /// per-chunk text arrives in `ChatChunk::delta`, finish reason +
    /// usage in the final chunk.
    async fn stream(&self, request: ChatRequest) -> Result<ChatStream, BackendError>;

    /// Best-effort token count for `text` against a specific model on
    /// this provider. Default impl delegates to the unified
    /// [`tokens::count_tokens`] dispatch; per-provider overrides may
    /// consult the provider's HTTP `count_tokens` endpoint when an
    /// exact answer is required + a network round-trip is acceptable.
    fn count_tokens(&self, model: &str, text: &str) -> usize {
        tokens::count_tokens(model, text).count
    }

    /// Capability discovery — does this backend support `capability`
    /// for the given model? Default returns `false` for everything;
    /// per-provider impls override.
    #[allow(unused_variables)]
    fn supports(&self, capability: Capability, model: &str) -> bool {
        false
    }
}

// ────────────────────────────────────────────────────────────────────
//  Registry — string-keyed dispatch by provider name
// ────────────────────────────────────────────────────────────────────

/// Process-wide registry of registered backends.
///
/// Backends are registered by their canonical short name (the same
/// string the Python `BACKEND_REGISTRY` uses — verified by the
/// v1.18.0 drift gate). Lookup is `O(1)` HashMap.
pub struct Registry {
    backends: HashMap<String, Box<dyn Backend>>,
}

impl Registry {
    /// Empty registry — useful for tests that want to register only
    /// stub backends.
    pub fn empty() -> Self {
        Self { backends: HashMap::new() }
    }

    /// Production registry — populated with all 7 native backends.
    ///
    /// Every backend is constructed via its `from_env()` factory — i.e.
    /// API keys are read at registry-construction time from the
    /// per-provider env vars (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`,
    /// `GEMINI_API_KEY`, `KIMI_API_KEY`, `GLM_API_KEY`, `OPENROUTER_API_KEY`,
    /// plus `OLLAMA_HOST` / `OLLAMA_API_KEY` for the local daemon).
    /// Backends whose env var is missing still construct successfully;
    /// the auth check fires on the first `complete()` call instead.
    ///
    /// The registry's `provider_names()` returns the sorted list of all
    /// 7 keys: `["anthropic", "gemini", "glm", "kimi", "ollama",
    /// "openai", "openrouter"]`. The v1.18.0 drift gate
    /// (`tests/test_fase24_backend_parity.py`) asserts this set
    /// matches Python's `BACKEND_REGISTRY` keys exactly.
    pub fn production() -> Self {
        let mut registry = Self::empty();
        registry.register(Box::new(anthropic::AnthropicBackend::from_env()));
        registry.register(Box::new(gemini::GeminiBackend::from_env()));
        registry.register(Box::new(glm::GLMBackend::from_env()));
        registry.register(Box::new(kimi::KimiBackend::from_env()));
        registry.register(Box::new(ollama::OllamaBackend::from_env()));
        registry.register(Box::new(openai::OpenAIBackend::from_env()));
        registry.register(Box::new(openrouter::OpenRouterBackend::from_env()));
        registry
    }

    /// v1.24.0 — Production registry PLUS the `stub` backend.
    ///
    /// Used by the server streaming path so dispatch through the
    /// uniform `Registry` surface includes the stub. The 7 canonical
    /// production backends are unchanged; `stub` is added as an 8th
    /// entry. The v1.18.0 cross-stack drift gate continues to pin
    /// the 7 canonical entries exactly via filesystem enumeration of
    /// `axon-rs/src/backends/*.rs` minus the `SHARED_INFRA_MODULES`
    /// set (which includes `stub`).
    ///
    /// Adopters who call [`Registry::production()`] directly do not
    /// see the stub — it surfaces only on the streaming-path
    /// dispatcher, where its content matches the legacy synchronous
    /// stub-mode output byte-for-byte (D4 wire byte-compat).
    pub fn production_with_stub() -> Self {
        let mut registry = Self::production();
        registry.register(Box::new(stub::StubBackend::new()));
        registry
    }
}

/// v1.24.0 — Owned-backend resolver for the streaming dispatch
/// path.
///
/// Returns `Some(Box<dyn Backend>)` for the 7 canonical production
/// providers plus `"stub"`. Returns `None` for any other name (e.g.
/// `"auto"` after upstream resolution failed, or an unknown name
/// the adopter supplied).
///
/// The dispatch set MUST match [`Registry::production_with_stub`]
/// exactly — adding a backend here without adding it to the
/// registry (or vice versa) is caught by the
/// `resolve_streaming_backend_dispatch_set_matches_production_with_stub`
/// drift test below.
///
/// Each lookup constructs a fresh backend via `from_env()` so the
/// returned `Box` owns its own reqwest client + retry policy.
/// Async tasks own their backend for the duration of one flow
/// (the trait is `Send + Sync` but not `Clone`, so per-task
/// ownership keeps the dispatch path simple).
pub fn resolve_streaming_backend(name: &str) -> Option<Box<dyn Backend>> {
    match name {
        "anthropic" => Some(Box::new(anthropic::AnthropicBackend::from_env())),
        "openai" => Some(Box::new(openai::OpenAIBackend::from_env())),
        "gemini" => Some(Box::new(gemini::GeminiBackend::from_env())),
        "kimi" => Some(Box::new(kimi::KimiBackend::from_env())),
        "glm" => Some(Box::new(glm::GLMBackend::from_env())),
        "ollama" => Some(Box::new(ollama::OllamaBackend::from_env())),
        "openrouter" => Some(Box::new(openrouter::OpenRouterBackend::from_env())),
        "stub" => Some(Box::new(stub::StubBackend::new())),
        _ => None,
    }
}

/// v2.15.0 — like [`resolve_streaming_backend`] but pins an EXPLICIT API key
/// (the per-tenant key resolved from the tenant secrets manager) onto the
/// backend via each provider's `with_api_key`, instead of reading the provider
/// env var through `from_env()`. `None` ⇒ defer to `resolve_streaming_backend`
/// (the pre-v2.15.0 env behavior). This is what lets the dispatcher's LLM path
/// honor per-tenant keys — before v2.15.0 the streaming/SSE path could ONLY use
/// the process env key, so multi-tenant SSE either shared one key or broke.
pub fn resolve_streaming_backend_with_key(
    name: &str,
    api_key: Option<&str>,
) -> Option<Box<dyn Backend>> {
    let Some(key) = api_key else {
        return resolve_streaming_backend(name);
    };
    let k = || Some(key.to_string());
    match name {
        "anthropic" => Some(Box::new(anthropic::AnthropicBackend::with_api_key(k()))),
        "openai" => Some(Box::new(openai::OpenAIBackend::with_api_key(k()))),
        "gemini" => Some(Box::new(gemini::GeminiBackend::with_api_key(k()))),
        "kimi" => Some(Box::new(kimi::KimiBackend::with_api_key(k()))),
        "glm" => Some(Box::new(glm::GLMBackend::with_api_key(k()))),
        "ollama" => Some(Box::new(ollama::OllamaBackend::with_api_key(k()))),
        "openrouter" => Some(Box::new(openrouter::OpenRouterBackend::with_api_key(k()))),
        // The stub backend ignores keys.
        "stub" => Some(Box::new(stub::StubBackend::new())),
        _ => None,
    }
}

/// v1.18.0 (Kivi brief #37) — like [`resolve_streaming_backend_with_key`]
/// but also threads an EXPLICIT per-tenant base-URL / chat-path override onto
/// the OpenAI-compatible providers (openai, kimi, glm, ollama, openrouter).
///
/// Precedence inside each provider is **explicit (per-tenant secret) > process
/// env (`<PROVIDER>_BASE_URL` / `_CHAT_PATH`) > preset default** — the same
/// shape as the per-tenant `tool.base_url` override, applied to the LLM
/// endpoint. This is what lets a tenant point the `glm` backend at z.ai's
/// `https://api.z.ai/api/paas/v4` + `/chat/completions` instead of the
/// bigmodel.cn default.
///
/// `base_url`/`chat_path = None` ⇒ identical behaviour to
/// `resolve_streaming_backend_with_key` for the OpenAI-compat providers
/// (env/default), so existing call sites that pass `None` are unaffected.
/// Providers without an OpenAI-compat config (anthropic, gemini, stub) ignore
/// the endpoint override and fall back to the key-only resolver.
pub fn resolve_streaming_backend_with_key_and_endpoint(
    name: &str,
    api_key: Option<&str>,
    base_url: Option<&str>,
    chat_path: Option<&str>,
) -> Option<Box<dyn Backend>> {
    let key = || api_key.map(|s| s.to_string());
    match name {
        "openai" => Some(Box::new(openai::OpenAIBackend::with_api_key_and_endpoint(
            key(),
            base_url,
            chat_path,
        ))),
        "kimi" => Some(Box::new(kimi::KimiBackend::with_api_key_and_endpoint(
            key(),
            base_url,
            chat_path,
        ))),
        "glm" => Some(Box::new(glm::GLMBackend::with_api_key_and_endpoint(
            key(),
            base_url,
            chat_path,
        ))),
        "ollama" => Some(Box::new(ollama::OllamaBackend::with_api_key_and_endpoint(
            key(),
            base_url,
            chat_path,
        ))),
        "openrouter" => Some(Box::new(
            openrouter::OpenRouterBackend::with_api_key_and_endpoint(key(), base_url, chat_path),
        )),
        // anthropic / gemini / stub have no OpenAI-compat base; the endpoint
        // override does not apply — defer to the key-only resolver.
        _ => resolve_streaming_backend_with_key(name, api_key),
    }
}

/// Names recognised by [`resolve_streaming_backend`]. Sorted.
/// Pinned by the drift test below.
pub const STREAMING_BACKEND_NAMES: &[&str] = &[
    "anthropic",
    "gemini",
    "glm",
    "kimi",
    "ollama",
    "openai",
    "openrouter",
    "stub",
];

/// v1.24.0 — Canonical 7-provider set surfaced to adopters.
///
/// Identical to [`STREAMING_BACKEND_NAMES`] minus `"stub"` (which is
/// a test/internal backend, not an adopter-facing provider). This is
/// the SINGLE SOURCE OF TRUTH for "which providers does axon
/// support natively"; the legacy
/// [`crate::backend::SUPPORTED_BACKENDS`] mono-file constant is now
/// a `pub use` re-export of this.
///
/// Drift-gated by `resolver_tests::canonical_providers_equals_legacy_supported`
/// (asserts byte-equality with the legacy constant) and
/// `tests/mono_file_retirement.rs` (asserts the same plus
/// the count + canonical-vs-stub-removed invariant).
pub const CANONICAL_PROVIDERS: &[&str] = &[
    "anthropic",
    "gemini",
    "glm",
    "kimi",
    "ollama",
    "openai",
    "openrouter",
];

/// v1.31.0 — Canonical providers with a usable API key present in
/// the environment, in `CANONICAL_PROVIDERS` priority order.
///
/// Feeds the `env_available` rung of the Backend Resolution Contract
/// (D1 / D6): when the operator-tuned `backend_registry` is empty,
/// `auto` resolution picks the first canonical provider whose
/// `<PROVIDER>_API_KEY` is set — so a server started with one provider
/// key "just works" without a `PUT /v1/backends` registration dance.
///
/// `ollama` (whose key is optional for the local daemon) is included
/// ONLY when `OLLAMA_API_KEY` is explicitly set to a non-empty value —
/// a local-ollama deployment declares `backend: ollama` explicitly (or
/// sets the var) rather than the resolver probing a network port.
/// `stub` is not a canonical provider, so it is never returned here.
pub fn env_available_backends() -> Vec<String> {
    CANONICAL_PROVIDERS
        .iter()
        .filter(|p| get_api_key(p).map(|k| !k.is_empty()).unwrap_or(false))
        .map(|p| p.to_string())
        .collect()
}

/// v1.24.0 — Canonical API-key env-var resolution.
///
/// Same semantics as the legacy `crate::backend::get_api_key`:
///   - For known providers, reads `<PROVIDER>_API_KEY` from the
///     environment.
///   - For `"ollama"`, missing key is permitted (local daemon).
///   - For all other providers, returns an error with adopter-
///     actionable hint when the env var is unset.
///   - For unknown provider names, returns an error listing
///     [`CANONICAL_PROVIDERS`].
///
/// This is the SINGLE SOURCE OF TRUTH for the legacy
/// `crate::backend::get_api_key` shim. The legacy shim wraps this
/// with the legacy `crate::backend::BackendError` struct shape;
/// callers using the trait `Backend` surface read keys via their
/// per-provider `from_env` factory instead.
pub fn get_api_key(provider: &str) -> Result<String, String> {
    let env_var = match provider {
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "gemini" => "GEMINI_API_KEY",
        "kimi" => "KIMI_API_KEY",
        "glm" => "GLM_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "ollama" => "OLLAMA_API_KEY", // local: missing key permitted
        _ => {
            return Err(format!(
                "Unknown backend '{provider}'. Supported: {}",
                CANONICAL_PROVIDERS.join(", ")
            ));
        }
    };
    if provider == "ollama" {
        return Ok(std::env::var(env_var).unwrap_or_default());
    }
    std::env::var(env_var).map_err(|_| {
        format!(
            "{env_var} not set. Required for backend '{provider}'.\n\
             hint: export {env_var}=<your-api-key>"
        )
    })
}

#[cfg(test)]
mod resolver_tests {
    use super::*;

    #[test]
    fn resolve_streaming_backend_returns_none_for_unknown_name() {
        assert!(resolve_streaming_backend("does-not-exist").is_none());
        assert!(resolve_streaming_backend("").is_none());
        assert!(resolve_streaming_backend("auto").is_none());
    }

    #[test]
    fn resolve_streaming_backend_returns_some_for_each_streaming_name() {
        for name in STREAMING_BACKEND_NAMES {
            let backend = resolve_streaming_backend(name)
                .unwrap_or_else(|| panic!("resolver should return Some for {name:?}"));
            assert_eq!(backend.name(), *name);
        }
    }

    #[test]
    fn resolve_with_key_pins_each_provider_and_falls_back_on_none() {
        // v2.15.0 — with an explicit key, every streaming provider resolves
        // (the per-tenant key path), and the backend identity is unchanged.
        for name in STREAMING_BACKEND_NAMES {
            let b = resolve_streaming_backend_with_key(name, Some("sk-tenant-test"))
                .unwrap_or_else(|| panic!("with-key resolver should return Some for {name:?}"));
            assert_eq!(b.name(), *name, "identity preserved for {name:?}");
        }
        // `None` defers to the env path (same Some/None shape as the base fn).
        assert!(resolve_streaming_backend_with_key("anthropic", None).is_some());
        assert!(resolve_streaming_backend_with_key("does-not-exist", Some("k")).is_none());
        assert!(resolve_streaming_backend_with_key("does-not-exist", None).is_none());
    }

    #[test]
    fn resolve_streaming_backend_dispatch_set_matches_production_with_stub() {
        let registry = Registry::production_with_stub();
        let registry_names = registry.provider_names();
        let mut resolver_names: Vec<String> =
            STREAMING_BACKEND_NAMES.iter().map(|s| s.to_string()).collect();
        resolver_names.sort();
        assert_eq!(
            registry_names, resolver_names,
            "resolve_streaming_backend() and Registry::production_with_stub() \
             must dispatch the same set of backends — drift here breaks the \
             D1 contract that Backend::stream() is the only production path \
             for Stream<T>"
        );
    }

    #[test]
    fn streaming_backend_names_pins_eight_entries() {
        // 7 canonical providers + stub. Adding a ninth requires
        // updating both the resolver match and the
        // `Registry::production_with_stub()` constructor — and
        // re-running the drift test above.
        assert_eq!(STREAMING_BACKEND_NAMES.len(), 8);
    }

    #[test]
    fn streaming_backend_names_are_sorted() {
        let mut sorted = STREAMING_BACKEND_NAMES.to_vec();
        sorted.sort();
        assert_eq!(sorted.as_slice(), STREAMING_BACKEND_NAMES);
    }

    #[test]
    fn canonical_providers_equals_legacy_supported_backends() {
        // v1.24.0 drift gate: the new
        // `crate::backends::CANONICAL_PROVIDERS` (consolidated
        // single source of truth) MUST equal the legacy
        // `crate::backend::SUPPORTED_BACKENDS` byte-for-byte. The
        // legacy constant is now a `pub use` re-export of the new
        // one, so byte-equality is by-construction; this test pins
        // the invariant + catches accidental drift if someone
        // re-introduces a separate const.
        assert_eq!(
            CANONICAL_PROVIDERS,
            crate::backend::SUPPORTED_BACKENDS,
            "33.x.i drift: canonical providers must equal legacy SUPPORTED_BACKENDS"
        );
    }

    #[test]
    fn canonical_providers_is_streaming_minus_stub() {
        // v1.24.0 invariant: the canonical 7-provider set
        // equals the 8-entry streaming dispatch set with `stub`
        // removed. Drift here surfaces if a new provider is added
        // to one set but not the other.
        let mut canonical_sorted: Vec<&str> = CANONICAL_PROVIDERS.to_vec();
        canonical_sorted.sort();
        let streaming_without_stub: Vec<&str> = STREAMING_BACKEND_NAMES
            .iter()
            .copied()
            .filter(|n| *n != "stub")
            .collect();
        assert_eq!(canonical_sorted, streaming_without_stub);
    }

    #[test]
    fn get_api_key_unknown_provider_returns_error() {
        let err = get_api_key("does-not-exist").unwrap_err();
        assert!(err.contains("Unknown backend"));
        assert!(err.contains("Supported:"));
    }

    #[test]
    fn get_api_key_ollama_permits_missing_key() {
        // Ollama is a local daemon — missing key is allowed.
        // Save+restore to avoid test-isolation issues.
        let prev = std::env::var("OLLAMA_API_KEY").ok();
        std::env::remove_var("OLLAMA_API_KEY");
        let result = get_api_key("ollama");
        assert!(
            result.is_ok(),
            "ollama MUST permit missing API key for local daemon"
        );
        assert_eq!(result.unwrap(), "");
        if let Some(v) = prev {
            std::env::set_var("OLLAMA_API_KEY", v);
        }
    }

    #[tokio::test]
    async fn resolved_stub_streams_one_canonical_chunk() {
        let backend = resolve_streaming_backend("stub").expect("stub resolves");
        let req = ChatRequest::default();
        let mut stream = backend.stream(req).await.expect("stub streams");
        use futures::StreamExt;
        let chunk = stream.next().await.expect("one chunk").expect("ok");
        assert_eq!(chunk.delta, stub::STUB_CONTENT);
        assert!(stream.next().await.is_none(), "single-chunk semantics");
    }
}

impl Registry {
    /// Internal marker reserved for future expansion of the
    /// streaming-resolver dispatch surface. Currently a no-op; kept
    /// as a public-crate anchor so future v1.24.0 steps can
    /// extend the dispatch table without re-opening the parent impl
    /// block. Untyped const is a zero-cost marker in monomorphisation.
    #[doc(hidden)]
    pub(crate) const __FASE_33X_B_RESOLVER_BOUNDARY: () = ();

    /// Register `backend` under the key `backend.name()`. Replaces any
    /// existing entry with the same name (last-write-wins).
    pub fn register(&mut self, backend: Box<dyn Backend>) {
        self.backends.insert(backend.name().to_string(), backend);
    }

    /// Look up a backend by name. Returns `None` if not registered.
    pub fn get(&self, name: &str) -> Option<&dyn Backend> {
        self.backends.get(name).map(|b| b.as_ref())
    }

    /// All registered provider names, sorted alphabetically. Used by
    /// the cross-stack drift gate (v1.18.0) to verify the Rust set
    /// equals the Python `BACKEND_REGISTRY` set.
    pub fn provider_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.backends.keys().cloned().collect();
        names.sort();
        names
    }

    pub fn len(&self) -> usize {
        self.backends.len()
    }

    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::production()
    }
}

// ────────────────────────────────────────────────────────────────────
//  Tests — trait + types + Registry
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    /// Test-only stub that lets us exercise the Registry + trait without
    /// hitting a real provider.
    struct StubBackend {
        name: String,
    }

    #[async_trait]
    impl Backend for StubBackend {
        fn name(&self) -> &str {
            &self.name
        }
        fn default_model(&self) -> &str {
            "stub-model"
        }
        async fn complete(
            &self,
            _request: ChatRequest,
        ) -> Result<ChatResponse, BackendError> {
            Ok(ChatResponse {
                content: "stubbed".into(),
                model_name: "stub-model".into(),
                provider_name: self.name.clone(),
                finish_reason: FinishReason::Stop,
                usage: Usage::default(),
                retry_count: 0,
                trace_id: "stub".into(),
            })
        }
        async fn stream(
            &self,
            _request: ChatRequest,
        ) -> Result<ChatStream, BackendError> {
            let chunks = vec![
                Ok(ChatChunk { delta: "hi ".into(), ..Default::default() }),
                Ok(ChatChunk {
                    delta: "world".into(),
                    finish_reason: Some(FinishReason::Stop),
                    usage: Some(Usage { input_tokens: 1, output_tokens: 2, total_tokens: 3, ..Default::default() }),
                }),
            ];
            Ok(Box::pin(futures::stream::iter(chunks)))
        }
        fn supports(&self, capability: Capability, _model: &str) -> bool {
            matches!(capability, Capability::Streaming)
        }
    }

    fn stub(name: &str) -> Box<dyn Backend> {
        Box::new(StubBackend { name: name.to_string() })
    }

    #[test]
    fn role_round_trips_via_as_str() {
        for r in [Role::System, Role::User, Role::Assistant, Role::Tool] {
            assert!(!r.as_str().is_empty());
        }
        assert_eq!(Role::User.as_str(), "user");
    }

    #[test]
    fn message_helpers_set_role() {
        assert_eq!(Message::user("a").role, Role::User);
        assert_eq!(Message::assistant("b").role, Role::Assistant);
        assert_eq!(Message::system("c").role, Role::System);
    }

    #[test]
    fn chat_request_default_is_empty() {
        let r = ChatRequest::default();
        assert!(r.model.is_empty());
        assert!(r.messages.is_empty());
        assert!(r.tools.is_empty());
        assert!(!r.stream);
    }

    #[test]
    fn finish_reason_anthropic_mapping() {
        assert_eq!(FinishReason::from_provider("anthropic", "end_turn"), FinishReason::Stop);
        assert_eq!(FinishReason::from_provider("anthropic", "max_tokens"), FinishReason::Length);
        assert_eq!(FinishReason::from_provider("anthropic", "tool_use"), FinishReason::ToolUse);
        assert_eq!(FinishReason::from_provider("anthropic", "stop_sequence"), FinishReason::Stop);
    }

    #[test]
    fn finish_reason_openai_mapping() {
        assert_eq!(FinishReason::from_provider("openai", "stop"), FinishReason::Stop);
        assert_eq!(FinishReason::from_provider("openai", "length"), FinishReason::Length);
        assert_eq!(FinishReason::from_provider("openai", "tool_calls"), FinishReason::ToolUse);
        assert_eq!(FinishReason::from_provider("openai", "content_filter"), FinishReason::SafetyBreach);
    }

    #[test]
    fn finish_reason_gemini_mapping_uppercase() {
        // Gemini emits SAFETY / MAX_TOKENS / STOP — case-folded.
        assert_eq!(FinishReason::from_provider("gemini", "STOP"), FinishReason::Stop);
        assert_eq!(FinishReason::from_provider("gemini", "MAX_TOKENS"), FinishReason::Length);
        assert_eq!(FinishReason::from_provider("gemini", "SAFETY"), FinishReason::SafetyBreach);
    }

    #[test]
    fn finish_reason_unknown_preserves_raw() {
        let r = FinishReason::from_provider("openai", "weird_signal");
        assert_eq!(r, FinishReason::Other("weird_signal".into()));
    }

    #[test]
    fn finish_reason_safety_breach_predicate() {
        assert!(FinishReason::SafetyBreach.is_safety_breach());
        assert!(!FinishReason::Stop.is_safety_breach());
        assert!(!FinishReason::Other("anything".into()).is_safety_breach());
    }

    #[test]
    fn registry_empty_then_register() {
        let mut r = Registry::empty();
        assert_eq!(r.len(), 0);
        r.register(stub("anthropic"));
        assert_eq!(r.len(), 1);
        assert!(r.get("anthropic").is_some());
        assert!(r.get("openai").is_none());
    }

    #[test]
    fn registry_provider_names_sorted() {
        let mut r = Registry::empty();
        r.register(stub("openai"));
        r.register(stub("anthropic"));
        r.register(stub("gemini"));
        assert_eq!(
            r.provider_names(),
            vec!["anthropic".to_string(), "gemini".to_string(), "openai".to_string()]
        );
    }

    #[test]
    fn registry_replace_on_duplicate_register() {
        let mut r = Registry::empty();
        r.register(stub("anthropic"));
        r.register(stub("anthropic"));
        assert_eq!(r.len(), 1); // last-write-wins
    }

    #[tokio::test]
    async fn stub_complete_returns_response() {
        let b = StubBackend { name: "stub".into() };
        let resp = b.complete(ChatRequest::default()).await.unwrap();
        assert_eq!(resp.content, "stubbed");
        assert_eq!(resp.provider_name, "stub");
        assert_eq!(resp.finish_reason, FinishReason::Stop);
    }

    #[tokio::test]
    async fn stub_stream_yields_chunks() {
        let b = StubBackend { name: "stub".into() };
        let stream = b.stream(ChatRequest::default()).await.unwrap();
        let chunks: Vec<_> = stream.collect().await;
        assert_eq!(chunks.len(), 2);
        let first = chunks[0].as_ref().unwrap();
        assert_eq!(first.delta, "hi ");
        assert!(first.finish_reason.is_none());
        let last = chunks[1].as_ref().unwrap();
        assert_eq!(last.delta, "world");
        assert!(matches!(last.finish_reason, Some(FinishReason::Stop)));
        let usage = last.usage.as_ref().unwrap();
        assert_eq!(usage.total_tokens, 3);
    }

    #[tokio::test]
    async fn registry_dispatches_to_correct_backend() {
        let mut r = Registry::empty();
        r.register(stub("anthropic"));
        r.register(stub("openai"));
        let b = r.get("openai").expect("openai registered");
        let resp = b.complete(ChatRequest::default()).await.unwrap();
        assert_eq!(resp.provider_name, "openai");
    }

    #[test]
    fn supports_capability_default_false() {
        struct DefaultBackend;
        #[async_trait]
        impl Backend for DefaultBackend {
            fn name(&self) -> &str {
                "default"
            }
            fn default_model(&self) -> &str {
                ""
            }
            async fn complete(
                &self,
                _r: ChatRequest,
            ) -> Result<ChatResponse, BackendError> {
                unreachable!()
            }
            async fn stream(
                &self,
                _r: ChatRequest,
            ) -> Result<ChatStream, BackendError> {
                unreachable!()
            }
        }
        let b = DefaultBackend;
        assert!(!b.supports(Capability::Streaming, "anything"));
        assert!(!b.supports(Capability::ToolUse, "anything"));
    }

    #[test]
    fn count_tokens_default_uses_unified_dispatch() {
        let b = StubBackend { name: "stub".into() };
        // The stub doesn't override count_tokens, so the trait default
        // delegates to tokens::count_tokens — same model dispatch as
        // the standalone function.
        let n = b.count_tokens("gpt-4o-mini", "hello world");
        assert!(n > 0);
    }
}
