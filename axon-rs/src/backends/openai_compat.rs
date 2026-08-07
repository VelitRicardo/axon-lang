//! OpenAI-compatible shared backend — Fase 24.d.
//!
//! Async port of `axon/backends/_openai_compatible.py`. Five providers
//! share this wire shape and only diverge on `(provider_name,
//! base_url, default_model, api_key_env)`:
//!
//!   * OpenAI — `https://api.openai.com`
//!   * Kimi (Moonshot) — `https://api.moonshot.ai`
//!   * GLM (Zhipu) — `https://open.bigmodel.cn/api/paas`
//!   * Ollama (local) — `http://localhost:11434`
//!   * OpenRouter — `https://openrouter.ai/api`
//!
//! All five hit `POST <base>/v1/chat/completions` with `Authorization:
//! Bearer <api_key>` (Ollama lets the key be empty since it's a local
//! daemon) and an OpenAI-shape body. Response parsing is identical
//! across the five (`choices[0].message.content`,
//! `usage.prompt_tokens` / `completion_tokens` / `total_tokens`,
//! plus o1/o3-specific `completion_tokens_details.reasoning_tokens`).
//!
//! # Wire shape
//!
//! ```text
//! POST <base>/v1/chat/completions
//! headers:
//!   authorization: Bearer <api_key>
//!   content-type:  application/json
//! body:
//!   {
//!     "model":        "<resolved>",
//!     "messages":     [{"role": "system|user|assistant|tool", "content": "..."}, ...],
//!     "temperature":  0.7,                     // optional, omitted when locked
//!     "top_p":        0.9,                     // optional, omitted when locked
//!     "max_tokens":   4096,                    // optional
//!     "tools":        [{"type": "function", "function": {...}}, ...],   // optional
//!     "stream":       true                     // optional
//!   }
//! ```
//!
//! Sampling parameters that the model locks (Kimi K2.x, OpenAI o1*,
//! o3*) are stripped before send via [`super::locked_model::apply_sampling_params`]
//! — port of v1.16.2 dispatch.
//!
//! # Response (non-streaming)
//!
//! ```json
//! {
//!   "id": "chatcmpl-...",
//!   "model": "gpt-4o-2024-08-06",
//!   "choices": [{
//!     "index": 0,
//!     "message": {"role": "assistant", "content": "..."},
//!     "finish_reason": "stop"
//!   }],
//!   "usage": {
//!     "prompt_tokens":     12,
//!     "completion_tokens": 4,
//!     "total_tokens":      16,
//!     "completion_tokens_details": {"reasoning_tokens": 0}
//!   }
//! }
//! ```
//!
//! # Composition
//!
//! Subclass-style polymorphism is replaced by composition: each
//! provider exposes a `from_env()` factory that returns an
//! [`OpenAICompatibleBackend`] configured with its own
//! [`OpenAICompatConfig`]. Per-provider files (`openai.rs`, `kimi.rs`,
//! `glm.rs`, `ollama.rs`, `openrouter.rs`) are thin wrappers; the
//! shared [`Backend`] impl lives here.

use std::env;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Value};
use tracing::Instrument;

use super::error::BackendError;
use super::locked_model;
use super::observability;
use super::retry::BackendRetryPolicy;
use super::sse_streaming::sse_event_stream;
use super::tokens;
use super::transport;
use super::{
    Backend, Capability, ChatChunk, ChatRequest, ChatResponse, ChatStream,
    FinishReason, Role, Usage,
};

/// Default `max_tokens` when a request omits one. The OpenAI-compat
/// providers don't strictly require it (unlike Anthropic), but pinning
/// a sane default avoids a surprise unbounded billing event when an
/// adopter forgets to set it.
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Per-provider configuration for an [`OpenAICompatibleBackend`].
///
/// The five OpenAI-compat providers share the wire shape and diverge
/// only on these four fields. Each provider exposes a factory that
/// constructs the right config + reads its env var.
#[derive(Debug, Clone)]
pub struct OpenAICompatConfig {
    /// Short provider name used as the registry key.
    pub provider_name: &'static str,
    /// Base URL — the request URL is `<base><chat_completions_path>`.
    pub base_url: String,
    /// The path appended to `base_url` to form the chat-completions
    /// endpoint. Default `/v1/chat/completions` (the OpenAI convention,
    /// shared by Kimi / OpenRouter / Ollama). Configurable because some
    /// OpenAI-compatible gateways serve a non-`/v1` native path — e.g.
    /// Zhipu/z.ai's `/api/paas/v4/chat/completions` (Fase 24.g.2, Kivi
    /// brief #37).
    pub chat_completions_path: String,
    /// Default model when a request omits one.
    pub default_model: String,
    /// Environment variable that holds the API key. Used in error
    /// messages so adopters can wire credentials correctly.
    pub api_key_env: Option<&'static str>,
    /// Environment variable that overrides `base_url` when set (Fase
    /// 24.g.2). E.g. `GLM_BASE_URL` to point the GLM backend at a
    /// regional / z.ai endpoint instead of the bigmodel.cn default.
    pub base_url_env: Option<&'static str>,
    /// Environment variable that overrides `chat_completions_path` when
    /// set (Fase 24.g.2). E.g. `GLM_CHAT_PATH=/api/paas/v4/chat/completions`.
    pub chat_path_env: Option<&'static str>,
}

/// The OpenAI-convention chat-completions path, shared by every preset
/// as the default (`base_url` + this = the endpoint).
const DEFAULT_CHAT_PATH: &str = "/v1/chat/completions";

impl OpenAICompatConfig {
    /// Provider-canonical `OpenAI` configuration.
    pub fn openai() -> Self {
        Self {
            provider_name: "openai",
            base_url: "https://api.openai.com".into(),
            chat_completions_path: DEFAULT_CHAT_PATH.into(),
            default_model: "gpt-4o-mini".into(),
            api_key_env: Some("OPENAI_API_KEY"),
            base_url_env: Some("OPENAI_BASE_URL"),
            chat_path_env: Some("OPENAI_CHAT_PATH"),
        }
    }

    /// Provider-canonical `Kimi` (Moonshot) configuration.
    pub fn kimi() -> Self {
        Self {
            provider_name: "kimi",
            base_url: "https://api.moonshot.ai".into(),
            chat_completions_path: DEFAULT_CHAT_PATH.into(),
            default_model: "moonshot-v1-8k".into(),
            api_key_env: Some("KIMI_API_KEY"),
            base_url_env: Some("KIMI_BASE_URL"),
            chat_path_env: Some("KIMI_CHAT_PATH"),
        }
    }

    /// Provider-canonical `GLM` (Zhipu) configuration. The bigmodel.cn
    /// default serves `/api/paas/v1/...`; adopters on z.ai override via
    /// `GLM_BASE_URL` + `GLM_CHAT_PATH` (Fase 24.g.2, Kivi brief #37).
    pub fn glm() -> Self {
        Self {
            provider_name: "glm",
            base_url: "https://open.bigmodel.cn/api/paas".into(),
            chat_completions_path: DEFAULT_CHAT_PATH.into(),
            default_model: "glm-4-plus".into(),
            api_key_env: Some("GLM_API_KEY"),
            base_url_env: Some("GLM_BASE_URL"),
            chat_path_env: Some("GLM_CHAT_PATH"),
        }
    }

    /// Provider-canonical `Ollama` (local) configuration. No API key
    /// required — the daemon is local-only.
    pub fn ollama() -> Self {
        Self {
            provider_name: "ollama",
            base_url: "http://localhost:11434".into(),
            chat_completions_path: DEFAULT_CHAT_PATH.into(),
            default_model: "llama3.1:8b".into(),
            api_key_env: None,
            // Ollama has its own `OLLAMA_HOST` base override (handled in
            // `ollama::from_env`), so no generic `*_BASE_URL` env here.
            base_url_env: None,
            chat_path_env: None,
        }
    }

    /// Provider-canonical `OpenRouter` (multi-provider gateway)
    /// configuration. Model names use the `provider/model` slug form
    /// (e.g. `openai/gpt-4o-mini`); the gateway routes accordingly.
    pub fn openrouter() -> Self {
        Self {
            provider_name: "openrouter",
            base_url: "https://openrouter.ai/api".into(),
            chat_completions_path: DEFAULT_CHAT_PATH.into(),
            default_model: "openai/gpt-4o-mini".into(),
            api_key_env: Some("OPENROUTER_API_KEY"),
            base_url_env: Some("OPENROUTER_BASE_URL"),
            chat_path_env: Some("OPENROUTER_CHAT_PATH"),
        }
    }

    /// §Fase 24.g.2 — apply the per-provider `*_BASE_URL` / `*_CHAT_PATH`
    /// environment overrides onto this config (when the vars are set and
    /// non-empty). Called by each provider's `from_env()` — the
    /// deployment path — so the process environment can redirect a
    /// backend at a regional / OpenAI-compatible endpoint without a code
    /// change. The base is trailing-`/`-trimmed and the path is forced to
    /// a leading `/` so `<base><path>` always joins cleanly.
    pub fn apply_env_overrides(&mut self) {
        if let Some(var) = self.base_url_env {
            if let Ok(v) = env::var(var) {
                if !v.trim().is_empty() {
                    self.base_url = normalize_base_url(&v);
                }
            }
        }
        if let Some(var) = self.chat_path_env {
            if let Ok(v) = env::var(var) {
                if !v.trim().is_empty() {
                    self.chat_completions_path = normalize_chat_path(&v);
                }
            }
        }
    }

    /// §Fase 24.g.2 — apply EXPLICIT base-URL / chat-path overrides (each
    /// applied only when `Some` + non-empty). These are the highest-
    /// precedence tier — a per-tenant `llm.<backend>.base_url` /
    /// `.chat_path` secret the enterprise layer threads through the flow
    /// runner. Precedence: **explicit (per-tenant) > env > preset default.**
    pub fn apply_explicit_overrides(&mut self, base_url: Option<&str>, chat_path: Option<&str>) {
        if let Some(b) = base_url {
            if !b.trim().is_empty() {
                self.base_url = normalize_base_url(b);
            }
        }
        if let Some(p) = chat_path {
            if !p.trim().is_empty() {
                self.chat_completions_path = normalize_chat_path(p);
            }
        }
    }
}

/// Trim trailing slashes off a base URL so joining with a leading-`/`
/// path never produces a double slash.
fn normalize_base_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

/// Force a chat path to a single leading `/` (no trailing slash) so
/// `<base><path>` is well-formed regardless of how the adopter wrote it.
fn normalize_chat_path(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if let Some(stripped) = trimmed.strip_prefix('/') {
        format!("/{}", stripped.trim_start_matches('/'))
    } else {
        format!("/{trimmed}")
    }
}

/// Shared OpenAI-compatible backend. Five providers consume this
/// directly via their own [`OpenAICompatConfig`].
pub struct OpenAICompatibleBackend {
    config: OpenAICompatConfig,
    api_key: Option<String>,
    http_client: reqwest::Client,
    retry_policy: BackendRetryPolicy,
}

impl OpenAICompatibleBackend {
    /// Construct from a preset config + an API key (typically read from
    /// env by the per-provider factory). `api_key` may be `None` for
    /// Ollama (local daemon) or for testing fixtures.
    pub fn new(config: OpenAICompatConfig, api_key: Option<String>) -> Self {
        Self {
            config,
            api_key,
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .connect_timeout(Duration::from_secs(10))
                .build()
                .expect("reqwest client build"),
            retry_policy: BackendRetryPolicy::production(),
        }
    }

    /// Read the per-provider env var (when configured) + construct.
    /// Convenience over `new(config, env::var(config.api_key_env).ok())`.
    pub fn from_env(config: OpenAICompatConfig) -> Self {
        let api_key = config
            .api_key_env
            .and_then(|env_name| env::var(env_name).ok());
        Self::new(config, api_key)
    }

    /// Override the base URL (test fixtures, mock servers, regional
    /// endpoints, self-hosted Ollama on a non-default port). The value is
    /// trailing-`/`-trimmed so it joins cleanly with the chat path.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.config.base_url = normalize_base_url(&base_url.into());
        self
    }

    /// Override the chat-completions path appended to the base URL
    /// (§Fase 24.g.2). Default `/v1/chat/completions`; z.ai/Zhipu native
    /// is `/api/paas/v4/chat/completions`.
    pub fn with_chat_path(mut self, path: impl Into<String>) -> Self {
        self.config.chat_completions_path = normalize_chat_path(&path.into());
        self
    }

    /// Override the default model.
    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        self.config.default_model = model.into();
        self
    }

    /// Override the retry policy.
    pub fn with_retry_policy(mut self, policy: BackendRetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Provider-canonical name (read-only — the registry uses it).
    pub fn provider_name(&self) -> &str {
        self.config.provider_name
    }

    fn build_headers(&self) -> Result<HeaderMap, BackendError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        // Ollama (local) accepts requests without auth; other providers
        // require a Bearer token. Surface a typed AuthError early when
        // a non-Ollama provider is configured without a key.
        match (&self.api_key, self.config.api_key_env) {
            (Some(key), _) => {
                let header_value = format!("Bearer {key}");
                let header_value = HeaderValue::from_str(&header_value).map_err(|_| {
                    BackendError::Auth {
                        provider: self.config.provider_name.into(),
                        model: self.config.default_model.clone(),
                        api_key_env: self.config.api_key_env.map(str::to_string),
                        status: 0,
                        body_preview: "API key contains characters that cannot \
                                       be encoded as a header value"
                            .into(),
                    }
                })?;
                headers.insert(AUTHORIZATION, header_value);
            }
            (None, None) => {
                // Local daemon (Ollama) — no auth needed.
            }
            (None, Some(env_name)) => {
                return Err(BackendError::Auth {
                    provider: self.config.provider_name.into(),
                    model: self.config.default_model.clone(),
                    api_key_env: Some(env_name.into()),
                    status: 0,
                    body_preview: format!("{env_name} not set in environment"),
                });
            }
        }
        Ok(headers)
    }

    fn resolve_model<'a>(&'a self, request: &'a ChatRequest) -> &'a str {
        if request.model.is_empty() {
            &self.config.default_model
        } else {
            &request.model
        }
    }

    fn resolve_trace_id(request: &ChatRequest) -> String {
        request
            .trace_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
    }
}

#[async_trait]
impl Backend for OpenAICompatibleBackend {
    fn name(&self) -> &str {
        self.config.provider_name
    }

    fn default_model(&self) -> &str {
        &self.config.default_model
    }

    async fn complete(
        &self,
        request: ChatRequest,
    ) -> Result<ChatResponse, BackendError> {
        let model = self.resolve_model(&request).to_string();
        let trace_id = Self::resolve_trace_id(&request);
        let span =
            observability::call_span(self.config.provider_name, &model, &trace_id);
        let start = Instant::now();
        let provider = self.config.provider_name;

        async move {
            // Step 1 — build the request body (with locked-model dispatch).
            let body = build_request_body(&request, &self.config.default_model, false);
            let body_bytes = serde_json::to_vec(&body)
                .map_err(|e| BackendError::Generic {
                    provider: provider.into(),
                    model: model.clone(),
                    status: None,
                    message: format!("failed to encode request body: {e}"),
                })?;
            observability::on_request_built(
                Some(body.get("max_tokens").and_then(Value::as_u64).unwrap_or(0) as u32),
                request.temperature,
                request.messages.len(),
                request.tools.len(),
            );

            // Step 2 — build headers.
            let headers = self.build_headers()?;

            let url = format!(
                "{}{}",
                self.config.base_url, self.config.chat_completions_path
            );

            // Step 3 — call with shared retry loop.
            let (raw_response, retry_count) = transport::call_with_retry(
                &self.http_client,
                &self.retry_policy,
                &url,
                None, // display_url — real URL has no embedded secrets (Bearer auth in header)
                headers,
                body_bytes,
                provider,
                &model,
                self.config.api_key_env,
            )
            .await?;

            // Step 4 — decode + extract.
            let json: Value = serde_json::from_slice(&raw_response).map_err(|e| {
                BackendError::Generic {
                    provider: provider.into(),
                    model: model.clone(),
                    status: Some(200),
                    message: format!("failed to parse response JSON: {e}"),
                }
            })?;
            let response = parse_response(&json, provider, &model, retry_count, &trace_id);
            observability::on_parsed_response(
                response.usage.input_tokens,
                response.usage.output_tokens,
                finish_reason_label(&response.finish_reason),
            );
            observability::on_complete(
                start.elapsed().as_millis() as u64,
                retry_count,
                true,
            );

            // Step 5 — lift safety-breach finish reasons to typed error.
            if response.finish_reason.is_safety_breach() {
                return Err(BackendError::SafetyBreach {
                    provider: provider.into(),
                    model: response.model_name.clone(),
                    finish_reason: finish_reason_label(&response.finish_reason).into(),
                    body_preview: response.content.chars().take(200).collect(),
                });
            }

            Ok(response)
        }
        .instrument(span)
        .await
    }

    async fn stream(
        &self,
        request: ChatRequest,
    ) -> Result<ChatStream, BackendError> {
        // §Fase 33.d — Real OpenAI-compatible SSE streaming.
        //
        // Wire shape per OpenAI streaming docs (shared by Kimi, GLM,
        // Ollama-as-openai-compat, OpenRouter):
        //   POST /v1/chat/completions  (body { ..., "stream": true })
        //   Content-Type: text/event-stream
        //   data: {"choices":[{"delta":{"content":"..."},"finish_reason":null}]}
        //   data: {"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{...}}
        //   data: [DONE]
        //
        // Each `data:` line is a JSON object except the final
        // `data: [DONE]` sentinel which closes the stream.
        let model = self.resolve_model(&request).to_string();
        let provider = self.config.provider_name;

        // Step 1 — build streaming body (stream=true).
        let body = build_request_body(&request, &self.config.default_model, true);
        let body_bytes = serde_json::to_vec(&body).map_err(|e| BackendError::Generic {
            provider: provider.into(),
            model: model.clone(),
            status: None,
            message: format!("failed to encode streaming request body: {e}"),
        })?;

        // Step 2 — build headers.
        let headers = self.build_headers()?;

        // Step 3 — fire the request. We do NOT use the shared retry
        // loop because retrying mid-stream is semantically wrong (an
        // adopter already saw partial tokens; a retry would replay
        // them). Streaming MUST fail-fast on any non-200 status.
        let url = format!(
            "{}{}",
            self.config.base_url, self.config.chat_completions_path
        );
        let response = self
            .http_client
            .post(&url)
            .headers(headers)
            .body(body_bytes)
            .send()
            .await
            .map_err(|e| BackendError::Generic {
                provider: provider.into(),
                model: model.clone(),
                status: None,
                message: format!("streaming transport failure: {e}"),
            })?;

        let status = response.status().as_u16();
        if status != 200 {
            let headers_clone = response.headers().clone();
            let body = response.text().await.unwrap_or_default();
            return Err(super::error::categorise_http(
                provider,
                &model,
                status,
                &headers_clone,
                &body,
                self.config.api_key_env,
            ));
        }

        // Step 4 — wrap the byte-stream in an SSE event iterator and
        // project each event onto a ChatChunk. The chunk parser is
        // pure + total over the closed event-shape catalog.
        let provider_owned = provider.to_string();
        let model_owned = model.clone();
        let events = sse_event_stream(response, provider_owned.clone(), model_owned.clone());
        let chunks = futures::StreamExt::filter_map(events, move |event| {
            let provider = provider_owned.clone();
            let model = model_owned.clone();
            async move {
                match event {
                    Ok(event) => parse_openai_compat_chunk(event, &provider, &model),
                    Err(e) => Some(Err(e)),
                }
            }
        });
        // Step 5 — §Fase 33.x.e. Wrap with the cancel-aware adapter
        // so the consumer's `next()` returns `None` within ≤100ms
        // p95 after `request.cancel.cancel()` fires. The dropped
        // wrapper releases the reqwest body, aborting the HTTP
        // request mid-stream — no wasted token quota.
        let inner: ChatStream = Box::pin(chunks);
        Ok(super::sse_streaming::cancel_aware(inner, request.cancel.clone()))
    }

    fn count_tokens(&self, model: &str, text: &str) -> usize {
        tokens::count_tokens(model, text).count
    }

    fn supports(&self, capability: Capability, model: &str) -> bool {
        match capability {
            // Streaming + ToolUse + StructuredOutput supported by all
            // five OpenAI-compat providers (varies in fidelity but the
            // wire-level surface is uniform).
            Capability::Streaming | Capability::ToolUse | Capability::StructuredOutput => true,
            // Vision: OpenAI gpt-4o family + many OpenRouter models +
            // some Ollama models (llava). Conservative default false;
            // per-provider overrides can return true.
            Capability::Vision => false,
            // Anthropic-only.
            Capability::PromptCaching => false,
            // Gemini-only — provider-side safety knobs.
            Capability::SafetySettings => false,
            // Locked-params: true iff the resolved model has at least
            // one entry in the locked registry (Kimi K2.x, o1, o3).
            Capability::LockedParams => {
                !locked_model::locked_params_for_model(model).is_empty()
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  Request body construction
// ────────────────────────────────────────────────────────────────────

/// Build the JSON body for `POST /v1/chat/completions` (OpenAI shape).
///
/// Differences from the Anthropic body:
///
///   * System messages stay in the `messages` array (NOT lifted to a
///     top-level `system` field — that's Anthropic-specific).
///   * `tools` envelope is `{type: "function", function: {name,
///     description, parameters}}` (NOT Anthropic's `{name, description,
///     input_schema}`).
///   * Sampling params (`temperature`, `top_p`) are dispatched through
///     [`super::locked_model::apply_sampling_params`] — the body is
///     stripped of any locked field BEFORE send so reasoning models
///     (Kimi K2.x, o1, o3) don't return HTTP 400.
pub(crate) fn build_request_body(
    request: &ChatRequest,
    default_model: &str,
    stream: bool,
) -> Value {
    let model = if request.model.is_empty() {
        default_model
    } else {
        &request.model
    };

    let mut wire_messages: Vec<Value> = Vec::with_capacity(request.messages.len() + 1);

    // Optional `request.system` becomes a leading system message
    // (NOT a top-level field — that's Anthropic's convention).
    if let Some(s) = request.system.as_ref() {
        if !s.is_empty() {
            wire_messages.push(json!({"role": "system", "content": s}));
        }
    }

    for msg in &request.messages {
        match msg.role {
            Role::Tool => {
                wire_messages.push(json!({
                    "role": "tool",
                    "content": msg.content,
                    "tool_call_id": msg.tool_call_id.clone().unwrap_or_default(),
                }));
            }
            _ => {
                wire_messages.push(json!({
                    "role": msg.role.as_str(),
                    "content": msg.content,
                }));
            }
        }
    }

    let mut body = json!({
        "model": model,
        "messages": wire_messages,
        "max_tokens": request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
    });

    let body_obj = body.as_object_mut().expect("json object");

    // Sampling params — these may be stripped by the locked-model
    // dispatch below if the model rejects them.
    if let Some(t) = request.temperature {
        body_obj.insert("temperature".into(), json!(t));
    }
    if let Some(p) = request.top_p {
        body_obj.insert("top_p".into(), json!(p));
    }
    // §Fase 119.b (Tier 1) — the mandate engine's token bans. OpenAI's wire
    // wants string keys. Inserted BEFORE the locked-model strip below, which
    // already lists `logit_bias` for o1/o3 — a locked model silently loses
    // the bias and the mandate loop (Tier 2) carries the guarantee alone.
    if let Some(bias) = &request.logit_bias {
        let wire: serde_json::Map<String, Value> = bias
            .iter()
            .map(|(id, b)| (id.to_string(), json!(b)))
            .collect();
        body_obj.insert("logit_bias".into(), Value::Object(wire));
    }

    // OpenAI tool envelope.
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|t| {
                let parameters: Value = serde_json::from_str(&t.parameters_json)
                    .unwrap_or_else(|_| json!({"type": "object", "properties": {}}));
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": parameters,
                    }
                })
            })
            .collect();
        body_obj.insert("tools".into(), Value::Array(tools));
    }

    if stream {
        body_obj.insert("stream".into(), Value::Bool(true));
    }

    // Locked-model dispatch — strip any sampling param the resolved
    // model rejects (Kimi K2.x, o1, o3). Mirror of v1.16.2.
    let _removed = locked_model::apply_sampling_params(&mut body, model);

    body
}

// ────────────────────────────────────────────────────────────────────
//  Response parsing
// ────────────────────────────────────────────────────────────────────

/// Parse a successful 200 OK chat-completions response into a
/// [`ChatResponse`]. Provider-neutral — `provider_name` parameterises
/// the [`FinishReason`] lookup so `content_filter` / `length` / `stop`
/// map correctly.
pub(crate) fn parse_response(
    payload: &Value,
    provider_name: &str,
    requested_model: &str,
    retry_count: u32,
    trace_id: &str,
) -> ChatResponse {
    let content_text = extract_content_text(payload);

    let finish_raw = payload
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .and_then(|c| c.get("finish_reason"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let finish_reason = FinishReason::from_provider(provider_name, finish_raw);

    let model_name = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(requested_model)
        .to_string();

    let usage = extract_usage(payload);

    ChatResponse {
        content: content_text,
        model_name,
        provider_name: provider_name.into(),
        finish_reason,
        usage,
        retry_count,
        trace_id: trace_id.to_string(),
    }
}

/// Pull the assistant text out of `choices[0].message.content`.
pub(crate) fn extract_content_text(payload: &Value) -> String {
    payload
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Extract [`Usage`] from an OpenAI-shape `usage` object. Handles the
/// `completion_tokens_details.reasoning_tokens` field surfaced by o1
/// and o3 models — those reasoning tokens count toward billing but
/// don't appear in the output text, so an aggregator dashboard that
/// ignores them undercounts cost.
pub(crate) fn extract_usage(payload: &Value) -> Usage {
    let usage = payload.get("usage");
    let read_field = |name: &str| -> u32 {
        usage
            .and_then(|u| u.get(name))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32
    };

    let prompt = read_field("prompt_tokens");
    let completion = read_field("completion_tokens");
    let total = read_field("total_tokens");

    let reasoning = usage
        .and_then(|u| u.get("completion_tokens_details"))
        .and_then(|d| d.get("reasoning_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;

    Usage {
        input_tokens: prompt,
        output_tokens: completion,
        total_tokens: if total > 0 { total } else { prompt + completion },
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        reasoning_tokens: reasoning,
    }
}

// ────────────────────────────────────────────────────────────────────
//  Helpers
// ────────────────────────────────────────────────────────────────────

fn finish_reason_label(reason: &FinishReason) -> &'static str {
    match reason {
        FinishReason::Stop => "stop",
        FinishReason::Length => "length",
        FinishReason::ToolUse => "tool_use",
        FinishReason::SafetyBreach => "safety_breach",
        FinishReason::Other(_) => "other",
    }
}

// ────────────────────────────────────────────────────────────────────
//  §Fase 33.d — SSE chunk parsing
// ────────────────────────────────────────────────────────────────────

/// Parse one OpenAI-compatible SSE event into an optional `ChatChunk`.
///
/// Returns:
///   * `None` — event carries no `data:` field, or the `[DONE]`
///     sentinel (which closes the stream silently).
///   * `Some(Ok(chunk))` — a usable streaming delta.
///   * `Some(Err(...))` — JSON in the event body was unparseable; this
///     surfaces to the caller's stream as a typed transport error.
///
/// Per OpenAI streaming docs (`docs/api/streaming`) and verified
/// against Moonshot Kimi / Zhipu GLM / OpenRouter SSE samples:
///
/// ```text
/// data: {"choices":[{"index":0,"delta":{"content":"hi"},"finish_reason":null}]}
/// data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}
/// data: [DONE]
/// ```
pub(crate) fn parse_openai_compat_chunk(
    event: super::sse_streaming::SseEvent,
    provider: &str,
    model: &str,
) -> Option<Result<ChatChunk, BackendError>> {
    let data = event.data?;
    let trimmed = data.trim();

    // [DONE] sentinel — close stream silently per OpenAI spec.
    if trimmed == "[DONE]" {
        return None;
    }

    // Parse the JSON envelope.
    let payload: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(e) => {
            return Some(Err(BackendError::Generic {
                provider: provider.into(),
                model: model.into(),
                status: Some(200),
                message: format!("failed to parse streaming JSON chunk: {e}"),
            }));
        }
    };

    // Extract delta text from `choices[0].delta.content`. May be absent
    // on chunks that carry only a finish_reason / role-only delta — in
    // that case we still emit a ChatChunk with empty delta + the
    // finish_reason populated.
    let choices = payload.get("choices").and_then(Value::as_array);
    let first_choice = choices.and_then(|c| c.first());
    let delta_text = first_choice
        .and_then(|c| c.get("delta"))
        .and_then(|d| d.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let finish_raw = first_choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(Value::as_str);
    let finish_reason =
        finish_raw.map(|raw| FinishReason::from_provider(provider, raw));

    // Usage is only present on the terminal chunk for most providers
    // (some send usage on every chunk — both shapes are valid).
    let usage = if payload.get("usage").is_some() {
        Some(extract_usage(&payload))
    } else {
        None
    };

    Some(Ok(ChatChunk {
        delta: delta_text,
        finish_reason,
        usage,
    }))
}

// ────────────────────────────────────────────────────────────────────
//  Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::{Message, ToolSpec};
    use serde_json::json;

    fn req_with(messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            messages,
            ..Default::default()
        }
    }

    // ── Config presets ──────────────────────────────────────────────

    #[test]
    fn config_presets_have_distinct_provider_names() {
        let names: Vec<_> = vec![
            OpenAICompatConfig::openai().provider_name,
            OpenAICompatConfig::kimi().provider_name,
            OpenAICompatConfig::glm().provider_name,
            OpenAICompatConfig::ollama().provider_name,
            OpenAICompatConfig::openrouter().provider_name,
        ];
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), 5);
    }

    #[test]
    fn ollama_config_has_no_api_key_env() {
        let c = OpenAICompatConfig::ollama();
        assert!(c.api_key_env.is_none());
        assert!(c.base_url.starts_with("http://localhost"));
    }

    #[test]
    fn kimi_config_points_to_moonshot_endpoint() {
        let c = OpenAICompatConfig::kimi();
        assert!(c.base_url.contains("moonshot"));
        assert_eq!(c.api_key_env, Some("KIMI_API_KEY"));
    }

    // ── Construction + builder API ──────────────────────────────────

    #[test]
    fn new_constructs_with_explicit_key() {
        let b = OpenAICompatibleBackend::new(
            OpenAICompatConfig::openai(),
            Some("sk-test".into()),
        );
        assert_eq!(b.name(), "openai");
        assert_eq!(b.default_model(), "gpt-4o-mini");
    }

    #[test]
    fn from_env_reads_per_provider_var() {
        // Env var almost certainly unset; just exercise the path.
        let _b = OpenAICompatibleBackend::from_env(OpenAICompatConfig::openai());
    }

    #[test]
    fn with_default_model_overrides() {
        let b = OpenAICompatibleBackend::new(OpenAICompatConfig::openai(), Some("k".into()))
            .with_default_model("o1-mini");
        assert_eq!(b.default_model(), "o1-mini");
    }

    #[test]
    fn with_base_url_overrides_for_test_fixtures() {
        let b = OpenAICompatibleBackend::new(OpenAICompatConfig::openai(), Some("k".into()))
            .with_base_url("http://127.0.0.1:9999");
        assert!(b.config.base_url.starts_with("http://127.0.0.1"));
    }

    // ── §Fase 24.g.2 — configurable base-URL + chat path (Kivi #37) ──

    #[test]
    fn every_preset_defaults_to_the_v1_chat_completions_path() {
        // No behavior change for the existing providers: the default
        // path stays `/v1/chat/completions`.
        for c in [
            OpenAICompatConfig::openai(),
            OpenAICompatConfig::kimi(),
            OpenAICompatConfig::glm(),
            OpenAICompatConfig::ollama(),
            OpenAICompatConfig::openrouter(),
        ] {
            assert_eq!(c.chat_completions_path, "/v1/chat/completions");
        }
    }

    #[test]
    fn glm_default_endpoint_is_bigmodel_v1() {
        // The unconfigured GLM backend joins to the bigmodel.cn default
        // (this is the URL that produced Kivi's Spring 404 on a z.ai key).
        let c = OpenAICompatConfig::glm();
        assert_eq!(
            format!("{}{}", c.base_url, c.chat_completions_path),
            "https://open.bigmodel.cn/api/paas/v1/chat/completions"
        );
        assert_eq!(c.base_url_env, Some("GLM_BASE_URL"));
        assert_eq!(c.chat_path_env, Some("GLM_CHAT_PATH"));
    }

    #[test]
    fn with_base_url_trims_trailing_slash() {
        let b = OpenAICompatibleBackend::new(OpenAICompatConfig::glm(), Some("k".into()))
            .with_base_url("https://api.z.ai/api/paas/v4/");
        assert_eq!(b.config.base_url, "https://api.z.ai/api/paas/v4");
    }

    #[test]
    fn with_chat_path_forces_leading_slash_and_trims_trailing() {
        let b = OpenAICompatibleBackend::new(OpenAICompatConfig::glm(), Some("k".into()))
            .with_chat_path("chat/completions/");
        assert_eq!(b.config.chat_completions_path, "/chat/completions");
    }

    #[test]
    fn zai_scenario_joins_to_the_native_paas_v4_endpoint() {
        // Kivi brief #37: z.ai serves `/api/paas/v4/chat/completions`
        // (no `/v1`). With base + chat-path overrides the backend hits
        // exactly the endpoint their key answered on.
        let b = OpenAICompatibleBackend::new(OpenAICompatConfig::glm(), Some("k".into()))
            .with_base_url("https://api.z.ai/api/paas/v4")
            .with_chat_path("/chat/completions");
        assert_eq!(
            format!("{}{}", b.config.base_url, b.config.chat_completions_path),
            "https://api.z.ai/api/paas/v4/chat/completions"
        );
    }

    #[test]
    fn apply_env_overrides_redirects_base_and_path() {
        // GLM_BASE_URL / GLM_CHAT_PATH are unique to this test in the
        // suite, so the process-wide env mutation does not race a reader.
        std::env::set_var("GLM_BASE_URL", "https://api.z.ai/api/paas/v4/");
        std::env::set_var("GLM_CHAT_PATH", "chat/completions");
        let mut c = OpenAICompatConfig::glm();
        c.apply_env_overrides();
        std::env::remove_var("GLM_BASE_URL");
        std::env::remove_var("GLM_CHAT_PATH");
        assert_eq!(
            format!("{}{}", c.base_url, c.chat_completions_path),
            "https://api.z.ai/api/paas/v4/chat/completions"
        );
    }

    #[test]
    fn explicit_override_wins_over_env_and_default() {
        // Precedence: explicit (per-tenant) > env > preset default.
        std::env::set_var("GLM_BASE_URL", "https://env.example/api");
        let mut c = OpenAICompatConfig::glm();
        c.apply_env_overrides(); // base now the env value
        assert_eq!(c.base_url, "https://env.example/api");
        c.apply_explicit_overrides(Some("https://api.z.ai/api/paas/v4"), Some("/chat/completions"));
        std::env::remove_var("GLM_BASE_URL");
        // explicit base wins; chat path set from explicit (env left it default).
        assert_eq!(
            format!("{}{}", c.base_url, c.chat_completions_path),
            "https://api.z.ai/api/paas/v4/chat/completions"
        );
    }

    #[test]
    fn apply_explicit_overrides_ignores_none_and_empty() {
        let mut c = OpenAICompatConfig::glm();
        c.apply_explicit_overrides(None, None);
        c.apply_explicit_overrides(Some("   "), Some(""));
        // Untouched — still the bigmodel.cn default.
        assert_eq!(
            format!("{}{}", c.base_url, c.chat_completions_path),
            "https://open.bigmodel.cn/api/paas/v1/chat/completions"
        );
    }

    #[test]
    fn apply_env_overrides_is_a_noop_when_unset() {
        // Defensive: the override is only applied when the var is set +
        // non-empty, so the default endpoint survives an unset env.
        std::env::remove_var("OPENROUTER_BASE_URL");
        std::env::remove_var("OPENROUTER_CHAT_PATH");
        let mut c = OpenAICompatConfig::openrouter();
        c.apply_env_overrides();
        assert_eq!(c.base_url, "https://openrouter.ai/api");
        assert_eq!(c.chat_completions_path, "/v1/chat/completions");
    }

    // ── Headers ─────────────────────────────────────────────────────

    #[test]
    fn build_headers_includes_bearer_auth() {
        let b = OpenAICompatibleBackend::new(
            OpenAICompatConfig::openai(),
            Some("sk-test".into()),
        );
        let h = b.build_headers().expect("headers");
        let auth = h.get(AUTHORIZATION).unwrap().to_str().unwrap();
        assert_eq!(auth, "Bearer sk-test");
        assert_eq!(h.get(CONTENT_TYPE).unwrap(), "application/json");
    }

    #[test]
    fn build_headers_omits_auth_for_ollama() {
        let b = OpenAICompatibleBackend::new(OpenAICompatConfig::ollama(), None);
        let h = b.build_headers().expect("headers");
        assert!(h.get(AUTHORIZATION).is_none());
    }

    #[test]
    fn build_headers_returns_auth_error_when_key_missing_for_keyed_provider() {
        let b = OpenAICompatibleBackend::new(OpenAICompatConfig::openai(), None);
        match b.build_headers() {
            Err(BackendError::Auth { api_key_env, .. }) => {
                assert_eq!(api_key_env.as_deref(), Some("OPENAI_API_KEY"));
            }
            other => panic!("expected Auth error, got {other:?}"),
        }
    }

    // ── Request body shape ──────────────────────────────────────────

    #[test]
    fn body_includes_model_messages_max_tokens() {
        let req = req_with(vec![Message::user("hi")]);
        let body = build_request_body(&req, "gpt-4o-mini", false);
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["max_tokens"], DEFAULT_MAX_TOKENS);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "hi");
    }

    #[test]
    fn body_keeps_system_in_messages_array_unlike_anthropic() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.system = Some("You are helpful.".into());
        let body = build_request_body(&req, "gpt-4o-mini", false);
        // OpenAI puts the system prompt as a regular message; no
        // top-level `system` field (that's Anthropic-specific).
        assert!(body.get("system").is_none());
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "You are helpful.");
        assert_eq!(msgs[1]["role"], "user");
    }

    #[test]
    fn body_serialises_tool_specs_in_openai_envelope() {
        let mut req = req_with(vec![Message::user("call a tool")]);
        req.tools = vec![ToolSpec {
            name: "get_weather".into(),
            description: "fetch the current weather".into(),
            parameters_json:
                r#"{"type":"object","properties":{"city":{"type":"string"}}}"#.into(),
        }];
        let body = build_request_body(&req, "gpt-4o-mini", false);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        // OpenAI envelope: `{"type": "function", "function": {...}}` —
        // distinct from Anthropic's `{"name", "description", "input_schema"}`.
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "get_weather");
        assert_eq!(tools[0]["function"]["description"], "fetch the current weather");
        assert_eq!(tools[0]["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn body_encodes_tool_role_with_tool_call_id() {
        let req = req_with(vec![
            Message::user("call it"),
            Message {
                role: Role::Tool,
                content: "result-payload".into(),
                tool_call_id: Some("call_abc".into()),
            },
        ]);
        let body = build_request_body(&req, "gpt-4o-mini", false);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        let tool = &msgs[1];
        assert_eq!(tool["role"], "tool");
        assert_eq!(tool["content"], "result-payload");
        assert_eq!(tool["tool_call_id"], "call_abc");
    }

    #[test]
    fn body_strips_locked_params_for_kimi_k2() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.model = "kimi-k2.6".into();
        req.temperature = Some(0.5);
        req.top_p = Some(0.9);
        let body = build_request_body(&req, "moonshot-v1-8k", false);
        // Kimi K2.x rejects temperature + top_p — they must be stripped.
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
    }

    /// §Fase 119.b (Tier 1) — the mandate engine's token bans reach the wire
    /// as string-keyed `logit_bias`, and a locked model strips them (the loop
    /// then carries the guarantee alone).
    #[test]
    fn body_carries_logit_bias_with_string_keys_for_unlocked_models() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.model = "gpt-4o-mini".into();
        req.logit_bias = Some([(50256u32, -100i32)].into_iter().collect());
        let body = build_request_body(&req, "gpt-4o-mini", false);
        let bias = body.get("logit_bias").expect("bias on the wire");
        assert_eq!(bias["50256"], -100, "OpenAI wants string token ids");
    }

    #[test]
    fn body_strips_logit_bias_for_locked_o1_family() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.model = "o1-mini".into();
        req.logit_bias = Some([(50256u32, -100i32)].into_iter().collect());
        let body = build_request_body(&req, "gpt-4o-mini", false);
        assert!(
            body.get("logit_bias").is_none(),
            "o1 rejects logit_bias; sending it would 400 the whole mandate loop"
        );
    }

    #[test]
    fn body_strips_locked_params_for_o1_family() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.model = "o1-mini".into();
        req.temperature = Some(0.7);
        let body = build_request_body(&req, "gpt-4o-mini", false);
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn body_keeps_sampling_params_for_unlocked_models() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.model = "gpt-4o-mini".into();
        req.temperature = Some(0.5);
        req.top_p = Some(0.9);
        let body = build_request_body(&req, "gpt-4o-mini", false);
        assert_eq!(body["temperature"], 0.5);
        assert_eq!(body["top_p"], 0.9);
    }

    #[test]
    fn body_includes_stream_flag_when_streaming() {
        let req = req_with(vec![Message::user("hi")]);
        let body = build_request_body(&req, "gpt-4o-mini", true);
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn body_uses_explicit_max_tokens_when_set() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.max_tokens = Some(2048);
        let body = build_request_body(&req, "gpt-4o-mini", false);
        assert_eq!(body["max_tokens"], 2048);
    }

    // ── Response parsing ────────────────────────────────────────────

    #[test]
    fn parse_response_extracts_assistant_content() {
        let payload = json!({
            "id": "chatcmpl-x",
            "model": "gpt-4o-mini-2024-07-18",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 12, "completion_tokens": 4, "total_tokens": 16},
        });
        let resp = parse_response(&payload, "openai", "gpt-4o-mini", 0, "trace-1");
        assert_eq!(resp.content, "Hello!");
        assert_eq!(resp.model_name, "gpt-4o-mini-2024-07-18");
        assert_eq!(resp.provider_name, "openai");
        assert_eq!(resp.finish_reason, FinishReason::Stop);
    }

    #[test]
    fn parse_response_length_finish_reason() {
        let payload = json!({
            "model": "gpt-4o-mini",
            "choices": [{"message": {"content": "..."}, "finish_reason": "length"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
        });
        let resp = parse_response(&payload, "openai", "gpt-4o-mini", 0, "t");
        assert_eq!(resp.finish_reason, FinishReason::Length);
    }

    #[test]
    fn parse_response_tool_calls_finish_reason() {
        let payload = json!({
            "model": "gpt-4o-mini",
            "choices": [{"message": {"content": ""}, "finish_reason": "tool_calls"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
        });
        let resp = parse_response(&payload, "openai", "gpt-4o-mini", 0, "t");
        assert_eq!(resp.finish_reason, FinishReason::ToolUse);
    }

    #[test]
    fn parse_response_content_filter_finish_reason() {
        let payload = json!({
            "model": "gpt-4o",
            "choices": [{"message": {"content": ""}, "finish_reason": "content_filter"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 0, "total_tokens": 1},
        });
        let resp = parse_response(&payload, "openai", "gpt-4o", 0, "t");
        assert!(resp.finish_reason.is_safety_breach());
    }

    #[test]
    fn parse_response_usage_with_o1_reasoning_tokens() {
        let payload = json!({
            "model": "o1-mini",
            "choices": [{"message": {"content": "answer"}, "finish_reason": "stop"}],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 200,
                "total_tokens": 300,
                "completion_tokens_details": {"reasoning_tokens": 150}
            },
        });
        let resp = parse_response(&payload, "openai", "o1-mini", 0, "t");
        assert_eq!(resp.usage.input_tokens, 100);
        assert_eq!(resp.usage.output_tokens, 200);
        assert_eq!(resp.usage.total_tokens, 300);
        assert_eq!(resp.usage.reasoning_tokens, 150);
    }

    #[test]
    fn parse_response_falls_back_to_request_model_when_payload_missing() {
        let payload = json!({
            "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
        });
        let resp = parse_response(&payload, "openai", "gpt-4o-mini", 0, "t");
        assert_eq!(resp.model_name, "gpt-4o-mini");
    }

    #[test]
    fn parse_response_handles_empty_choices() {
        let payload = json!({
            "model": "gpt-4o-mini",
            "choices": [],
            "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0},
        });
        let resp = parse_response(&payload, "openai", "gpt-4o-mini", 0, "t");
        assert_eq!(resp.content, "");
    }

    #[test]
    fn parse_response_total_tokens_falls_back_to_sum() {
        // Some providers (esp. older OpenAI-compat shims) omit
        // `total_tokens`; the parser should compute it from prompt +
        // completion when missing or zero.
        let payload = json!({
            "model": "gpt-4o-mini",
            "choices": [{"message": {"content": "x"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 30, "completion_tokens": 12},
        });
        let resp = parse_response(&payload, "openai", "gpt-4o-mini", 0, "t");
        assert_eq!(resp.usage.total_tokens, 42);
    }

    // ── Capability discovery ────────────────────────────────────────

    #[test]
    fn supports_streaming_tooluse_structuredoutput() {
        let b = OpenAICompatibleBackend::new(OpenAICompatConfig::openai(), Some("k".into()));
        let any_model = "gpt-4o-mini";
        assert!(b.supports(Capability::Streaming, any_model));
        assert!(b.supports(Capability::ToolUse, any_model));
        assert!(b.supports(Capability::StructuredOutput, any_model));
    }

    #[test]
    fn does_not_support_promptcaching_or_safetysettings() {
        let b = OpenAICompatibleBackend::new(OpenAICompatConfig::openai(), Some("k".into()));
        assert!(!b.supports(Capability::PromptCaching, "gpt-4o-mini"));
        assert!(!b.supports(Capability::SafetySettings, "gpt-4o-mini"));
    }

    #[test]
    fn supports_lockedparams_iff_model_is_in_locked_registry() {
        let b = OpenAICompatibleBackend::new(OpenAICompatConfig::openai(), Some("k".into()));
        assert!(b.supports(Capability::LockedParams, "o1-mini"));
        assert!(b.supports(Capability::LockedParams, "o3-mini"));
        assert!(b.supports(Capability::LockedParams, "kimi-k2.6"));
        assert!(!b.supports(Capability::LockedParams, "gpt-4o-mini"));
        assert!(!b.supports(Capability::LockedParams, "moonshot-v1-8k"));
    }

    // ── §Fase 33.d — SSE chunk parsing (pure-unit, network-free) ────

    use super::parse_openai_compat_chunk;
    use super::super::sse_streaming::SseEvent;

    fn sse_event_with_data(data: &str) -> SseEvent {
        SseEvent {
            data: Some(data.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn parse_chunk_extracts_delta_content() {
        let ev = sse_event_with_data(
            r#"{"choices":[{"index":0,"delta":{"content":"hello"},"finish_reason":null}]}"#,
        );
        let chunk = parse_openai_compat_chunk(ev, "openai", "gpt-4o-mini")
            .expect("data event yields chunk")
            .expect("valid JSON");
        assert_eq!(chunk.delta, "hello");
        assert!(chunk.finish_reason.is_none());
        assert!(chunk.usage.is_none());
    }

    #[test]
    fn parse_chunk_done_sentinel_returns_none() {
        let ev = sse_event_with_data("[DONE]");
        assert!(parse_openai_compat_chunk(ev, "openai", "gpt-4o-mini").is_none());
    }

    #[test]
    fn parse_chunk_done_sentinel_with_surrounding_whitespace_recognized() {
        // Some providers emit `data:[DONE]\n\n`; the SSE parser drops
        // the leading space already, but if a provider sends extra
        // whitespace inside the value we still recognize it.
        let ev = sse_event_with_data("  [DONE]  ");
        assert!(parse_openai_compat_chunk(ev, "openai", "gpt-4o-mini").is_none());
    }

    #[test]
    fn parse_chunk_final_chunk_carries_finish_reason_and_usage() {
        let ev = sse_event_with_data(
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":3,"completion_tokens":5,"total_tokens":8}}"#,
        );
        let chunk = parse_openai_compat_chunk(ev, "openai", "gpt-4o-mini")
            .expect("yields chunk")
            .expect("valid JSON");
        assert_eq!(chunk.delta, "");
        assert_eq!(chunk.finish_reason, Some(FinishReason::Stop));
        let usage = chunk.usage.expect("usage on terminal chunk");
        assert_eq!(usage.input_tokens, 3);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.total_tokens, 8);
    }

    #[test]
    fn parse_chunk_role_only_delta_yields_empty_delta() {
        // First chunk often carries only `delta.role="assistant"`.
        let ev = sse_event_with_data(
            r#"{"choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#,
        );
        let chunk = parse_openai_compat_chunk(ev, "openai", "gpt-4o-mini")
            .expect("yields chunk")
            .expect("valid JSON");
        assert_eq!(chunk.delta, "");
        assert!(chunk.finish_reason.is_none());
    }

    #[test]
    fn parse_chunk_invalid_json_surfaces_as_error() {
        let ev = sse_event_with_data("not-valid-json{");
        let result = parse_openai_compat_chunk(ev, "openai", "gpt-4o-mini")
            .expect("yields error");
        match result {
            Err(BackendError::Generic { message, .. }) => {
                assert!(message.contains("failed to parse streaming JSON chunk"));
            }
            other => panic!("expected Generic error, got {other:?}"),
        }
    }

    #[test]
    fn parse_chunk_event_without_data_returns_none() {
        // Comment-only / event-name-only events have no data payload
        // and must drop silently.
        let ev = SseEvent {
            event: Some("ping".into()),
            ..Default::default()
        };
        assert!(parse_openai_compat_chunk(ev, "openai", "gpt-4o-mini").is_none());
    }

    #[test]
    fn parse_chunk_kimi_locked_model_finish_reason_mapping() {
        // Kimi uses OpenAI-shape SSE; FinishReason mapping is provider-
        // agnostic here. Verifies the dispatch.
        let ev = sse_event_with_data(
            r#"{"choices":[{"delta":{},"finish_reason":"length"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#,
        );
        let chunk = parse_openai_compat_chunk(ev, "kimi", "moonshot-v1-8k")
            .expect("yields chunk")
            .expect("valid JSON");
        assert_eq!(chunk.finish_reason, Some(FinishReason::Length));
    }

    // ── complete() — early failure paths ────────────────────────────

    #[tokio::test]
    async fn complete_without_api_key_returns_auth_error() {
        let b = OpenAICompatibleBackend::new(OpenAICompatConfig::openai(), None)
            .with_base_url("http://127.0.0.1:0");
        let err = b
            .complete(ChatRequest {
                messages: vec![Message::user("hi")],
                ..Default::default()
            })
            .await
            .unwrap_err();
        match err {
            BackendError::Auth { api_key_env, .. } => {
                assert_eq!(api_key_env.as_deref(), Some("OPENAI_API_KEY"));
            }
            other => panic!("expected Auth, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn complete_for_ollama_does_not_require_api_key() {
        // Ollama has no API key env. complete() with empty key must
        // proceed past the auth check and only fail at the HTTP layer
        // (which we simulate by pointing at an unreachable port).
        let b = OpenAICompatibleBackend::new(OpenAICompatConfig::ollama(), None)
            .with_base_url("http://127.0.0.1:0")
            .with_retry_policy(BackendRetryPolicy::no_retry());
        let err = b
            .complete(ChatRequest {
                messages: vec![Message::user("hi")],
                ..Default::default()
            })
            .await
            .unwrap_err();
        // Should be a transport-layer Generic error, NOT an Auth error.
        assert!(matches!(err, BackendError::Generic { .. }));
    }
}
