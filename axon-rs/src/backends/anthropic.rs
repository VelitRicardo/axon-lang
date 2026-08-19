//! Anthropic Claude Messages API backend — v1.18.0.
//!
//! Async port of the legacy `axon::backend::call_anthropic` (blocking
//! `reqwest::blocking`) and the Python `axon.server.model_clients`
//! transport for the `"anthropic"` provider. Implements the [`Backend`]
//! trait shipped in 24.b.
//!
//! # Wire shape
//!
//! ```text
//! POST https://api.anthropic.com/v1/messages
//! headers:
//!   x-api-key:          <ANTHROPIC_API_KEY>
//!   anthropic-version:  2023-06-01
//!   content-type:       application/json
//! body:
//!   {
//!     "model":      "claude-sonnet-4-5",
//!     "max_tokens": 4096,
//!     "system":     "<system prompt>",        // optional, top-level
//!     "messages":   [{"role": "user", "content": "..."}, ...],
//!     "temperature": 0.7,                     // optional
//!     "tools":       [...],                   // optional, when ToolSpecs supplied
//!     "stream":      true                     // optional, when streaming
//!   }
//! ```
//!
//! # Response (non-streaming)
//!
//! ```json
//! {
//!   "id": "msg_...",
//!   "model": "claude-sonnet-4-5",
//!   "stop_reason": "end_turn",
//!   "content": [{"type": "text", "text": "..."}],
//!   "usage": {
//!     "input_tokens": 12,
//!     "output_tokens": 34,
//!     "cache_read_input_tokens": 0,
//!     "cache_creation_input_tokens": 0
//!   }
//! }
//! ```
//!
//! # Streaming
//!
//! Server-Sent Events over the same endpoint with `stream: true`. Event
//! types of interest:
//!
//!   * `message_start`         — initial usage hints (input_tokens preview).
//!   * `content_block_delta`   — incremental text fragments (`delta.text`).
//!   * `message_delta`         — final `stop_reason` + `usage.output_tokens`.
//!   * `message_stop`          — end of stream.
//!
//! Each SSE event is parsed into a [`ChatChunk`]. The final chunk
//! carries `finish_reason` + `usage`; intermediate chunks carry only
//! `delta`.
//!
//! # Capabilities
//!
//! * [`Capability::Streaming`]      — yes, via SSE.
//! * [`Capability::ToolUse`]        — yes (Anthropic tool use blocks).
//! * [`Capability::Vision`]         — yes (image content blocks); not
//!   exposed on [`ChatRequest`] in 24.c (lands in 24.h-followup if
//!   demand surfaces).
//! * [`Capability::PromptCaching`]  — yes; opt-in via per-message
//!   `cache_control: {"type": "ephemeral"}` on the underlying JSON
//!   (not in 24.c surface — adopters supply pre-built bodies for now).
//! * [`Capability::SafetySettings`] — provider-side only, not adjustable.

use std::env;
use std::pin::Pin;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::Stream;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use serde_json::{json, Value};
use tracing::Instrument;

use super::error::BackendError;
use super::observability;
use super::retry::BackendRetryPolicy;
use super::tokens;
use super::transport;
use super::{
    Backend, Capability, ChatChunk, ChatRequest, ChatResponse, ChatStream,
    FinishReason, Role, Usage,
};

const PROVIDER_NAME: &str = "anthropic";
const DEFAULT_MODEL: &str = "claude-3-5-haiku-latest";
const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const API_KEY_ENV: &str = "ANTHROPIC_API_KEY";

/// Default `max_tokens` when the request omits one. The API requires
/// `max_tokens` to be set; 4096 is a safe production default that
/// matches the legacy blocking implementation in `backend.rs`.
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Anthropic Claude backend. Construct with [`AnthropicBackend::from_env`]
/// (reads `ANTHROPIC_API_KEY`) or [`AnthropicBackend::with_api_key`].
pub struct AnthropicBackend {
    api_key: Option<String>,
    base_url: String,
    default_model: String,
    http_client: reqwest::Client,
    retry_policy: BackendRetryPolicy,
}

impl AnthropicBackend {
    /// Construct from env. `ANTHROPIC_API_KEY` is read at construction
    /// time; `None` is permitted (allows constructing the backend in
    /// test contexts where the key is supplied per-call elsewhere).
    pub fn from_env() -> Self {
        Self::with_api_key(env::var(API_KEY_ENV).ok())
    }

    /// Construct with an explicit API key (or `None`). Useful for
    /// multi-tenant servers that pin a key per-tenant.
    pub fn with_api_key(api_key: Option<String>) -> Self {
        Self {
            api_key,
            base_url: DEFAULT_BASE_URL.to_string(),
            default_model: DEFAULT_MODEL.to_string(),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .connect_timeout(Duration::from_secs(10))
                .build()
                .expect("reqwest client build"),
            retry_policy: BackendRetryPolicy::production(),
        }
    }

    /// Override the base URL (test fixtures, mock servers, regional
    /// endpoints). Returns `self` for builder chaining.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Override the default model. The provider-side default for the
    /// crate is `claude-3-5-haiku-latest`; production deployments
    /// typically override to `claude-sonnet-4-5` or similar.
    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = model.into();
        self
    }

    /// Override the retry policy (e.g. `BackendRetryPolicy::no_retry()`
    /// for fail-fast tests).
    pub fn with_retry_policy(mut self, policy: BackendRetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Build the headers a request to `/v1/messages` requires. Returns
    /// `None` when no API key is configured.
    fn build_headers(&self) -> Option<HeaderMap> {
        let api_key = self.api_key.as_ref()?;
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_str(api_key).ok()?,
        );
        headers.insert(
            HeaderName::from_static("anthropic-version"),
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Some(headers)
    }

    /// Resolve the model the request will use — explicit if non-empty,
    /// else the backend's default.
    fn resolve_model<'a>(&'a self, request: &'a ChatRequest) -> &'a str {
        if request.model.is_empty() {
            &self.default_model
        } else {
            &request.model
        }
    }

    /// Resolve the trace ID — request override if present, else a fresh
    /// uuid4. The result echoes back in [`ChatResponse::trace_id`].
    fn resolve_trace_id(request: &ChatRequest) -> String {
        request
            .trace_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
    }
}

impl Default for AnthropicBackend {
    fn default() -> Self {
        Self::from_env()
    }
}

#[async_trait]
impl Backend for AnthropicBackend {
    fn name(&self) -> &str {
        PROVIDER_NAME
    }

    fn default_model(&self) -> &str {
        &self.default_model
    }

    async fn complete(
        &self,
        request: ChatRequest,
    ) -> Result<ChatResponse, BackendError> {
        let model = self.resolve_model(&request).to_string();
        let trace_id = Self::resolve_trace_id(&request);
        let span = observability::call_span(PROVIDER_NAME, &model, &trace_id);
        let start = Instant::now();

        async move {
            // Step 1 — build the request body.
            let body = build_request_body(&request, &self.default_model, false);
            let body_bytes = serde_json::to_vec(&body)
                .map_err(|e| BackendError::Generic {
                    provider: PROVIDER_NAME.into(),
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
            let headers = self.build_headers().ok_or_else(|| BackendError::Auth {
                provider: PROVIDER_NAME.into(),
                model: model.clone(),
                api_key_env: Some(API_KEY_ENV.into()),
                status: 0,
                body_preview: format!("{API_KEY_ENV} not set in environment"),
            })?;

            let url = format!("{}/v1/messages", self.base_url);

            // Step 3 — call with retry. Shared loop in `transport.rs`
            // so anthropic / openai-compat / gemini converge on a
            // single source of truth for HTTP retry policy.
            let (raw_response, retry_count) = transport::call_with_retry(
                &self.http_client,
                &self.retry_policy,
                &url,
                None, // display_url — real URL has no embedded secrets
                headers,
                body_bytes,
                PROVIDER_NAME,
                &model,
                Some(API_KEY_ENV),
            )
            .await?;

            // Step 4 — decode + extract.
            let json: Value = serde_json::from_slice(&raw_response).map_err(|e| {
                BackendError::Generic {
                    provider: PROVIDER_NAME.into(),
                    model: model.clone(),
                    status: Some(200),
                    message: format!("failed to parse response JSON: {e}"),
                }
            })?;
            let response = parse_response(&json, &model, retry_count, &trace_id);
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
                    provider: PROVIDER_NAME.into(),
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
        // v1.24.0 — Real Anthropic SSE streaming.
        //
        // Wire shape per https://docs.anthropic.com/claude/reference/streaming:
        //   POST /v1/messages  (body { ..., "stream": true })
        //   Content-Type: text/event-stream
        //
        //   event: message_start
        //   data: {"type":"message_start","message":{...,"usage":{"input_tokens":N}}}
        //
        //   event: content_block_start
        //   data: {"type":"content_block_start","index":0,"content_block":{...}}
        //
        //   event: ping        ← Anthropic emits keepalive pings; we drop them.
        //   data: {"type":"ping"}
        //
        //   event: content_block_delta
        //   data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}
        //   ... many of these ...
        //
        //   event: content_block_stop
        //   data: {"type":"content_block_stop","index":0}
        //
        //   event: message_delta
        //   data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},
        //          "usage":{"output_tokens":N}}
        //
        //   event: message_stop
        //   data: {"type":"message_stop"}
        //
        // Mapping to `ChatChunk`:
        //   - `content_block_delta` with `delta.type == "text_delta"`  → chunk with delta=text
        //   - `message_start` → carry usage.input_tokens (no delta)
        //   - `message_delta` → carry finish_reason + cumulative usage
        //   - all others (message_stop, content_block_start/stop, ping) → drop
        let model = self.resolve_model(&request).to_string();

        // Step 1 — build streaming body.
        let body = build_request_body(&request, &self.default_model, true);
        let body_bytes = serde_json::to_vec(&body).map_err(|e| BackendError::Generic {
            provider: PROVIDER_NAME.into(),
            model: model.clone(),
            status: None,
            message: format!("failed to encode streaming request body: {e}"),
        })?;

        // Step 2 — build headers (returns None when no API key).
        let headers = self.build_headers().ok_or_else(|| BackendError::Auth {
            provider: PROVIDER_NAME.into(),
            model: model.clone(),
            api_key_env: Some(API_KEY_ENV.into()),
            status: 0,
            body_preview: format!("{API_KEY_ENV} not set in environment"),
        })?;

        // Step 3 — fire request. Streaming MUST fail-fast on non-200
        // (retrying mid-stream replays partial tokens — semantically
        // wrong).
        let url = format!("{}/v1/messages", self.base_url);
        let response = self
            .http_client
            .post(&url)
            .headers(headers)
            .body(body_bytes)
            .send()
            .await
            .map_err(|e| BackendError::Generic {
                provider: PROVIDER_NAME.into(),
                model: model.clone(),
                status: None,
                message: format!("streaming transport failure: {e}"),
            })?;

        let status = response.status().as_u16();
        if status != 200 {
            let headers_clone = response.headers().clone();
            let body = response.text().await.unwrap_or_default();
            return Err(super::error::categorise_http(
                PROVIDER_NAME,
                &model,
                status,
                &headers_clone,
                &body,
                Some(API_KEY_ENV),
            ));
        }

        // Step 4 — wrap byte-stream + project each Anthropic event to
        // a ChatChunk via the closed event-shape catalog.
        let events = super::sse_streaming::sse_event_stream(
            response,
            PROVIDER_NAME,
            model.clone(),
        );
        let model_owned = model.clone();
        let chunks = futures::StreamExt::filter_map(events, move |event| {
            let model = model_owned.clone();
            async move {
                match event {
                    Ok(event) => parse_anthropic_chunk(event, &model),
                    Err(e) => Some(Err(e)),
                }
            }
        });
        // Step 5 — v1.24.0. Cancel-aware wrap so `next()`
        // returns `None` ≤100ms p95 after `request.cancel.cancel()`.
        let inner: ChatStream = Box::pin(chunks);
        Ok(super::sse_streaming::cancel_aware(inner, request.cancel.clone()))
    }

    fn count_tokens(&self, model: &str, text: &str) -> usize {
        // Claude family uses the offline 4-cpt estimate; an exact
        // tokenizer requires `claude-tokenizer-rs` (not yet a dep) or
        // an HTTP `count_tokens` round-trip. Adopters needing exact
        // counts can call the API directly — surfaced via
        // `Backend::count_tokens` only as the rough budget.
        tokens::count_tokens(model, text).count
    }

    fn supports(&self, capability: Capability, _model: &str) -> bool {
        matches!(
            capability,
            Capability::Streaming
                | Capability::ToolUse
                | Capability::Vision
                | Capability::PromptCaching,
        )
    }
}

// ────────────────────────────────────────────────────────────────────
//  Request body construction
// ────────────────────────────────────────────────────────────────────

/// Build the JSON body for `POST /v1/messages`.
///
/// Mirrors the Python transport in `axon.server.model_clients`:
///
///   * `system` field carries the system prompt (top-level, NOT a
///     message — Anthropic's API differs from OpenAI here).
///   * `messages` array carries user / assistant turns; system messages
///     in the `ChatRequest::messages` are folded into the top-level
///     `system` field if no explicit `system` is set.
///   * `max_tokens` defaults to [`DEFAULT_MAX_TOKENS`] when the request
///     omits one (the API requires it; missing yields HTTP 400).
///   * `temperature` is included only when set on the request.
///   * `tools` is included only when non-empty; each spec serialises to
///     `{name, description, input_schema}` (Anthropic's tool envelope).
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

    // Collect system message(s) from the messages array; Anthropic puts
    // them in a top-level field rather than in the messages list.
    let mut system_segments: Vec<String> = Vec::new();
    if let Some(s) = request.system.as_ref() {
        if !s.is_empty() {
            system_segments.push(s.clone());
        }
    }
    let mut wire_messages: Vec<Value> = Vec::with_capacity(request.messages.len());
    for msg in &request.messages {
        match msg.role {
            Role::System => {
                if !msg.content.is_empty() {
                    system_segments.push(msg.content.clone());
                }
            }
            Role::User | Role::Assistant => {
                wire_messages.push(json!({
                    "role": msg.role.as_str(),
                    "content": msg.content,
                }));
            }
            Role::Tool => {
                // Anthropic encodes tool results as a user message
                // containing a `tool_result` content block.
                wire_messages.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": msg.tool_call_id.clone().unwrap_or_default(),
                        "content": msg.content,
                    }]
                }));
            }
        }
    }

    let mut body = json!({
        "model": model,
        "max_tokens": request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "messages": wire_messages,
    });

    let body_obj = body.as_object_mut().expect("json object");

    if !system_segments.is_empty() {
        body_obj.insert("system".into(), Value::String(system_segments.join("\n\n")));
    }
    if let Some(t) = request.temperature {
        body_obj.insert("temperature".into(), json!(t));
    }
    if let Some(p) = request.top_p {
        body_obj.insert("top_p".into(), json!(p));
    }
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|t| {
                let schema: Value = serde_json::from_str(&t.parameters_json)
                    .unwrap_or_else(|_| json!({"type": "object", "properties": {}}));
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": schema,
                })
            })
            .collect();
        body_obj.insert("tools".into(), Value::Array(tools));
    }
    if stream {
        body_obj.insert("stream".into(), Value::Bool(true));
    }

    body
}

// ────────────────────────────────────────────────────────────────────
//  Response parsing
// ────────────────────────────────────────────────────────────────────

/// Parse a successful 200 OK response body into a [`ChatResponse`].
pub(crate) fn parse_response(
    payload: &Value,
    requested_model: &str,
    retry_count: u32,
    trace_id: &str,
) -> ChatResponse {
    // `content` is an array of blocks; concatenate every `text`-type
    // block in order. Tool-use blocks are not surfaced through `content`
    // here (24.c.2 streaming may surface them differently); for now
    // their presence is reported via `finish_reason: ToolUse`.
    let content_text = extract_content_text(payload);

    let stop_reason_raw = payload
        .get("stop_reason")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let finish_reason = FinishReason::from_provider(PROVIDER_NAME, stop_reason_raw);

    let model_name = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(requested_model)
        .to_string();

    let usage = extract_usage(payload);

    ChatResponse {
        content: content_text,
        model_name,
        provider_name: PROVIDER_NAME.into(),
        finish_reason,
        usage,
        retry_count,
        trace_id: trace_id.to_string(),
    }
}

/// Concatenate the text from every `type: "text"` content block.
pub(crate) fn extract_content_text(payload: &Value) -> String {
    payload
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| {
                    if b.get("type").and_then(Value::as_str) == Some("text") {
                        b.get("text").and_then(Value::as_str).map(str::to_string)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// Extract the [`Usage`] breakdown from the response. Anthropic uses
/// `input_tokens` / `output_tokens` + the cache-related fields.
pub(crate) fn extract_usage(payload: &Value) -> Usage {
    let usage = payload.get("usage");
    let read_field = |name: &str| -> u32 {
        usage
            .and_then(|u| u.get(name))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32
    };
    let input = read_field("input_tokens");
    let output = read_field("output_tokens");
    Usage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: input + output,
        cache_read_tokens: read_field("cache_read_input_tokens"),
        cache_creation_tokens: read_field("cache_creation_input_tokens"),
        reasoning_tokens: 0,
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

#[allow(dead_code)]
type AnthropicChatStream =
    Pin<Box<dyn Stream<Item = Result<ChatChunk, BackendError>> + Send>>;

// ────────────────────────────────────────────────────────────────────
// v1.24.0 — SSE chunk parsing
// ────────────────────────────────────────────────────────────────────

/// Parse one Anthropic SSE event into an optional `ChatChunk`.
///
/// Returns:
///   * `None` — the event is a non-text-delta type (ping, message_stop,
///     content_block_start/stop) the consumer can safely ignore.
///   * `Some(Ok(chunk))` — a usable chunk (text delta, usage hint, or
///     terminal stop_reason carrier).
///   * `Some(Err(...))` — JSON parse failure surfaces as a typed
///     transport error.
///
/// Event-type → chunk mapping (closed catalog):
///   - `content_block_delta` w/ `delta.type == "text_delta"` → chunk
///     with `delta = delta.text`, no finish_reason yet.
///   - `content_block_delta` w/ other delta types (input_json_delta
///     for tool use, etc.) → empty-delta chunk; tool-use streaming
/// surface lands in v1.24.0.
///   - `message_start` → empty-delta chunk carrying initial usage
///     `{input_tokens: N}` so adopters can show "tokens budgeted"
///     before any text arrives.
///   - `message_delta` → empty-delta chunk carrying `finish_reason` +
///     final usage `{output_tokens: N}`.
///   - `message_stop` / `content_block_start` / `content_block_stop`
///     / `ping` → dropped silently (no observable wire change).
///   - Unknown event type → dropped (forward-compat).
pub(crate) fn parse_anthropic_chunk(
    event: super::sse_streaming::SseEvent,
    model: &str,
) -> Option<Result<ChatChunk, BackendError>> {
    let event_type = event.event.as_deref().unwrap_or("");
    let data = event.data?;

    // Empty data: no observable change.
    if data.trim().is_empty() {
        return None;
    }

    let payload: Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(e) => {
            return Some(Err(BackendError::Generic {
                provider: PROVIDER_NAME.into(),
                model: model.into(),
                status: Some(200),
                message: format!("failed to parse Anthropic streaming JSON chunk: {e}"),
            }));
        }
    };

    match event_type {
        "content_block_delta" => {
            // delta.type may be "text_delta" / "input_json_delta" / ...
            let delta_obj = payload.get("delta");
            let delta_text = delta_obj
                .and_then(|d| d.get("text"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Some(Ok(ChatChunk {
                delta: delta_text,
                finish_reason: None,
                usage: None,
            }))
        }
        "message_start" => {
            // Carry initial input_tokens budget so adopters can show
            // "0 / N tokens" before any text arrives.
            let usage = payload
                .get("message")
                .and_then(|m| m.get("usage"))
                .map(|u| Usage {
                    input_tokens: u
                        .get("input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32,
                    output_tokens: u
                        .get("output_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32,
                    total_tokens: 0,
                    cache_read_tokens: u
                        .get("cache_read_input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32,
                    cache_creation_tokens: u
                        .get("cache_creation_input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32,
                    reasoning_tokens: 0,
                });
            Some(Ok(ChatChunk {
                delta: String::new(),
                finish_reason: None,
                usage,
            }))
        }
        "message_delta" => {
            // Final stop_reason + cumulative output_tokens.
            let stop_reason = payload
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(Value::as_str);
            let finish_reason = stop_reason
                .map(|raw| FinishReason::from_provider(PROVIDER_NAME, raw));
            let usage = payload.get("usage").map(|u| Usage {
                input_tokens: 0,
                output_tokens: u
                    .get("output_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
                total_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                reasoning_tokens: 0,
            });
            Some(Ok(ChatChunk {
                delta: String::new(),
                finish_reason,
                usage,
            }))
        }
        // Dropped silently — these events are not observable on the
        // ChatChunk wire (the consumer can derive equivalent state
        // from message_start + content_block_delta + message_delta).
        "message_stop" | "content_block_start" | "content_block_stop" | "ping" => None,
        // Unknown event type — forward-compat: drop silently. Per
        // Anthropic's streaming spec evolution clauses, new event
        // types may be introduced; existing event types' semantics
        // never change.
        _ => None,
    }
}

// ────────────────────────────────────────────────────────────────────
//  Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::Message;
    use crate::backends::ToolSpec;
    use serde_json::json;

    fn req_with(messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            messages,
            ..Default::default()
        }
    }

    // ── Construction ────────────────────────────────────────────────

    #[test]
    fn from_env_constructs_with_default_model() {
        let b = AnthropicBackend::from_env();
        assert_eq!(b.name(), "anthropic");
        assert_eq!(b.default_model(), DEFAULT_MODEL);
    }

    #[test]
    fn with_api_key_explicit_overrides_env() {
        let b = AnthropicBackend::with_api_key(Some("sk-ant-test".into()));
        assert!(b.api_key.is_some());
    }

    #[test]
    fn with_default_model_overrides() {
        let b = AnthropicBackend::with_api_key(Some("k".into()))
            .with_default_model("claude-sonnet-4-5");
        assert_eq!(b.default_model(), "claude-sonnet-4-5");
    }

    #[test]
    fn with_base_url_overrides_for_test_fixtures() {
        let b = AnthropicBackend::with_api_key(Some("k".into()))
            .with_base_url("http://localhost:1234");
        assert_eq!(b.base_url, "http://localhost:1234");
    }

    // ── Capability discovery ────────────────────────────────────────

    #[test]
    fn supports_streaming_tool_use_vision_caching() {
        let b = AnthropicBackend::with_api_key(Some("k".into()));
        let any_model = "claude-sonnet-4-5";
        assert!(b.supports(Capability::Streaming, any_model));
        assert!(b.supports(Capability::ToolUse, any_model));
        assert!(b.supports(Capability::Vision, any_model));
        assert!(b.supports(Capability::PromptCaching, any_model));
    }

    #[test]
    fn does_not_support_safetysettings_or_lockedparams() {
        let b = AnthropicBackend::with_api_key(Some("k".into()));
        assert!(!b.supports(Capability::SafetySettings, "claude-x"));
        assert!(!b.supports(Capability::LockedParams, "claude-x"));
        assert!(!b.supports(Capability::StructuredOutput, "claude-x"));
    }

    // ── Headers ─────────────────────────────────────────────────────

    #[test]
    fn build_headers_includes_required_anthropic_keys() {
        let b = AnthropicBackend::with_api_key(Some("sk-ant-x".into()));
        let h = b.build_headers().expect("headers");
        assert_eq!(h.get("x-api-key").unwrap(), "sk-ant-x");
        assert_eq!(h.get("anthropic-version").unwrap(), ANTHROPIC_VERSION);
        assert_eq!(h.get("content-type").unwrap(), "application/json");
    }

    #[test]
    fn build_headers_returns_none_when_api_key_missing() {
        let b = AnthropicBackend::with_api_key(None);
        assert!(b.build_headers().is_none());
    }

    // ── Request body shape ──────────────────────────────────────────

    #[test]
    fn body_includes_model_max_tokens_messages() {
        let req = req_with(vec![Message::user("hi")]);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        assert_eq!(body["model"], DEFAULT_MODEL);
        assert_eq!(body["max_tokens"], DEFAULT_MAX_TOKENS);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "hi");
    }

    #[test]
    fn body_uses_explicit_model_when_set() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.model = "claude-sonnet-4-5".into();
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        assert_eq!(body["model"], "claude-sonnet-4-5");
    }

    #[test]
    fn body_falls_back_to_default_model_when_request_blank() {
        let req = req_with(vec![Message::user("hi")]);
        let body = build_request_body(&req, "claude-haiku-4-5", false);
        assert_eq!(body["model"], "claude-haiku-4-5");
    }

    #[test]
    fn body_lifts_system_field_to_top_level() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.system = Some("You are helpful.".into());
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        assert_eq!(body["system"], "You are helpful.");
        // System should NOT appear in the messages array.
        let msgs = body["messages"].as_array().unwrap();
        for m in msgs {
            assert_ne!(m["role"], "system");
        }
    }

    #[test]
    fn body_folds_system_role_messages_into_top_level_system() {
        let req = req_with(vec![
            Message::system("fold-me"),
            Message::user("hi"),
        ]);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        assert_eq!(body["system"], "fold-me");
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1); // system folded out
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn body_combines_explicit_system_and_role_messages() {
        let mut req = req_with(vec![
            Message::system("from-message"),
            Message::user("hi"),
        ]);
        req.system = Some("from-field".into());
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        // Joined with double newline.
        assert_eq!(body["system"], "from-field\n\nfrom-message");
    }

    #[test]
    fn body_includes_temperature_when_set() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.temperature = Some(0.7);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        assert_eq!(body["temperature"], 0.7);
    }

    #[test]
    fn body_omits_temperature_when_unset() {
        let req = req_with(vec![Message::user("hi")]);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn body_includes_top_p_when_set() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.top_p = Some(0.9);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        assert_eq!(body["top_p"], 0.9);
    }

    #[test]
    fn body_serialises_tool_specs_with_input_schema() {
        let mut req = req_with(vec![Message::user("call a tool")]);
        req.tools = vec![ToolSpec {
            name: "get_weather".into(),
            description: "fetch the current weather".into(),
            parameters_json: r#"{"type":"object","properties":{"city":{"type":"string"}}}"#
                .into(),
        }];
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "get_weather");
        assert_eq!(tools[0]["description"], "fetch the current weather");
        assert_eq!(tools[0]["input_schema"]["type"], "object");
    }

    #[test]
    fn body_omits_tools_when_empty() {
        let req = req_with(vec![Message::user("no tools")]);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn body_includes_stream_flag_when_streaming() {
        let req = req_with(vec![Message::user("hi")]);
        let body = build_request_body(&req, DEFAULT_MODEL, true);
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn body_uses_explicit_max_tokens_when_set() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.max_tokens = Some(8192);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        assert_eq!(body["max_tokens"], 8192);
    }

    #[test]
    fn body_encodes_tool_role_as_tool_result_block() {
        let req = req_with(vec![
            Message::user("call it"),
            Message {
                role: Role::Tool,
                content: "result-payload".into(),
                tool_call_id: Some("toolu_x".into()),
            },
        ]);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        // The tool result is wrapped as a user message with a
        // tool_result content block.
        let tr = &msgs[1];
        assert_eq!(tr["role"], "user");
        let block = &tr["content"][0];
        assert_eq!(block["type"], "tool_result");
        assert_eq!(block["tool_use_id"], "toolu_x");
        assert_eq!(block["content"], "result-payload");
    }

    // ── Response parsing ────────────────────────────────────────────

    #[test]
    fn parse_response_extracts_text_content() {
        let payload = json!({
            "model": "claude-sonnet-4-5",
            "stop_reason": "end_turn",
            "content": [{"type": "text", "text": "Hello, world!"}],
            "usage": {"input_tokens": 12, "output_tokens": 4},
        });
        let resp = parse_response(&payload, "claude-sonnet-4-5", 0, "trace-1");
        assert_eq!(resp.content, "Hello, world!");
        assert_eq!(resp.model_name, "claude-sonnet-4-5");
        assert_eq!(resp.provider_name, "anthropic");
        assert_eq!(resp.finish_reason, FinishReason::Stop);
        assert_eq!(resp.retry_count, 0);
        assert_eq!(resp.trace_id, "trace-1");
    }

    #[test]
    fn parse_response_concatenates_multiple_text_blocks() {
        let payload = json!({
            "stop_reason": "end_turn",
            "content": [
                {"type": "text", "text": "First."},
                {"type": "text", "text": "Second."},
            ],
            "usage": {"input_tokens": 0, "output_tokens": 0},
        });
        let resp = parse_response(&payload, "claude-x", 0, "t");
        assert_eq!(resp.content, "First.\nSecond.");
    }

    #[test]
    fn parse_response_skips_non_text_blocks() {
        let payload = json!({
            "stop_reason": "tool_use",
            "content": [
                {"type": "text", "text": "About to call a tool."},
                {"type": "tool_use", "id": "toolu_x", "name": "get_weather", "input": {}},
            ],
            "usage": {"input_tokens": 0, "output_tokens": 0},
        });
        let resp = parse_response(&payload, "claude-x", 0, "t");
        assert_eq!(resp.content, "About to call a tool.");
        assert_eq!(resp.finish_reason, FinishReason::ToolUse);
    }

    #[test]
    fn parse_response_extracts_usage_with_cache_fields() {
        let payload = json!({
            "stop_reason": "end_turn",
            "content": [{"type": "text", "text": "ok"}],
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_read_input_tokens": 80,
                "cache_creation_input_tokens": 20,
            },
        });
        let resp = parse_response(&payload, "claude-x", 0, "t");
        assert_eq!(resp.usage.input_tokens, 100);
        assert_eq!(resp.usage.output_tokens, 50);
        assert_eq!(resp.usage.total_tokens, 150);
        assert_eq!(resp.usage.cache_read_tokens, 80);
        assert_eq!(resp.usage.cache_creation_tokens, 20);
    }

    #[test]
    fn parse_response_handles_empty_content_array() {
        let payload = json!({
            "stop_reason": "end_turn",
            "content": [],
            "usage": {"input_tokens": 0, "output_tokens": 0},
        });
        let resp = parse_response(&payload, "claude-x", 0, "t");
        assert_eq!(resp.content, "");
    }

    #[test]
    fn parse_response_uses_payload_model_when_present() {
        let payload = json!({
            "model": "claude-sonnet-4-5-20251022",
            "stop_reason": "end_turn",
            "content": [{"type": "text", "text": "hi"}],
            "usage": {"input_tokens": 1, "output_tokens": 1},
        });
        let resp = parse_response(&payload, "claude-haiku-4-5", 0, "t");
        // Payload model overrides the requested one (provider may
        // resolve aliases / pin a specific version).
        assert_eq!(resp.model_name, "claude-sonnet-4-5-20251022");
    }

    #[test]
    fn parse_response_max_tokens_finish_reason() {
        let payload = json!({
            "stop_reason": "max_tokens",
            "content": [{"type": "text", "text": "truncated..."}],
            "usage": {"input_tokens": 1, "output_tokens": 1},
        });
        let resp = parse_response(&payload, "claude-x", 0, "t");
        assert_eq!(resp.finish_reason, FinishReason::Length);
    }

    #[test]
    fn parse_response_unknown_stop_reason_preserved_as_other() {
        let payload = json!({
            "stop_reason": "weird_provider_signal",
            "content": [{"type": "text", "text": "ok"}],
            "usage": {"input_tokens": 1, "output_tokens": 1},
        });
        let resp = parse_response(&payload, "claude-x", 0, "t");
        assert_eq!(
            resp.finish_reason,
            FinishReason::Other("weird_provider_signal".into())
        );
    }

    #[test]
    fn parse_response_retry_count_propagates() {
        let payload = json!({
            "stop_reason": "end_turn",
            "content": [{"type": "text", "text": "retried"}],
            "usage": {"input_tokens": 1, "output_tokens": 1},
        });
        let resp = parse_response(&payload, "claude-x", 2, "t");
        assert_eq!(resp.retry_count, 2);
    }

    // ── count_tokens / Backend trait ────────────────────────────────

    #[test]
    fn count_tokens_uses_estimate_for_claude_models() {
        let b = AnthropicBackend::with_api_key(Some("k".into()));
        // 8 chars → 2 tokens via the offline estimate.
        assert_eq!(b.count_tokens("claude-sonnet-4-5", "ABCDEFGH"), 2);
    }

    // ── Streaming surface ───────────────────────────────────────────

    #[tokio::test]
    async fn stream_real_anthropic_sse_implementation_transport_path() {
        // v1.24.0 — Anthropic now ships a real SSE streamer.
        // Unreachable-port test exercises the transport-error path.
        let b = AnthropicBackend::with_api_key(Some("k".into()))
            .with_base_url("http://127.0.0.1:1");
        match b.stream(ChatRequest::default()).await {
            Err(BackendError::Generic { ref message, .. }) => {
                assert!(
                    message.contains("streaming transport failure"),
                    "unexpected message: {message}",
                );
            }
            Err(other) => panic!("expected Generic, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[tokio::test]
    async fn stream_without_api_key_returns_auth_error() {
        let b = AnthropicBackend::with_api_key(None)
            .with_base_url("http://127.0.0.1:1");
        match b.stream(ChatRequest::default()).await {
            Err(BackendError::Auth { provider, .. }) => {
                assert_eq!(provider, "anthropic");
            }
            Err(other) => panic!("expected Auth, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    // ── v1.24.0 — Anthropic SSE chunk parsing (pure-unit) ────────

    use super::parse_anthropic_chunk;
    use super::super::sse_streaming::SseEvent;

    fn anthropic_event(event_type: &str, data: &str) -> SseEvent {
        SseEvent {
            event: Some(event_type.to_string()),
            data: Some(data.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn parse_anthropic_content_block_delta_extracts_text() {
        let ev = anthropic_event(
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,
                "delta":{"type":"text_delta","text":"Hello"}}"#,
        );
        let chunk = parse_anthropic_chunk(ev, "claude-x")
            .expect("yields chunk")
            .expect("valid JSON");
        assert_eq!(chunk.delta, "Hello");
        assert!(chunk.finish_reason.is_none());
    }

    #[test]
    fn parse_anthropic_message_start_carries_input_token_budget() {
        let ev = anthropic_event(
            "message_start",
            r#"{"type":"message_start","message":{"id":"msg_x","usage":{"input_tokens":42,"output_tokens":0}}}"#,
        );
        let chunk = parse_anthropic_chunk(ev, "claude-x")
            .expect("yields chunk")
            .expect("valid JSON");
        assert_eq!(chunk.delta, "");
        assert!(chunk.finish_reason.is_none());
        let usage = chunk.usage.expect("message_start carries usage");
        assert_eq!(usage.input_tokens, 42);
    }

    #[test]
    fn parse_anthropic_message_delta_carries_stop_reason_and_output_tokens() {
        let ev = anthropic_event(
            "message_delta",
            r#"{"type":"message_delta",
                "delta":{"stop_reason":"end_turn","stop_sequence":null},
                "usage":{"output_tokens":17}}"#,
        );
        let chunk = parse_anthropic_chunk(ev, "claude-x")
            .expect("yields chunk")
            .expect("valid JSON");
        assert_eq!(chunk.delta, "");
        assert_eq!(chunk.finish_reason, Some(FinishReason::Stop));
        let usage = chunk.usage.expect("message_delta carries usage");
        assert_eq!(usage.output_tokens, 17);
    }

    #[test]
    fn parse_anthropic_message_delta_maps_max_tokens_to_length() {
        let ev = anthropic_event(
            "message_delta",
            r#"{"delta":{"stop_reason":"max_tokens"},"usage":{"output_tokens":4096}}"#,
        );
        let chunk = parse_anthropic_chunk(ev, "claude-x")
            .expect("yields chunk")
            .expect("valid JSON");
        assert_eq!(chunk.finish_reason, Some(FinishReason::Length));
    }

    #[test]
    fn parse_anthropic_ping_dropped() {
        let ev = anthropic_event("ping", r#"{"type":"ping"}"#);
        assert!(parse_anthropic_chunk(ev, "claude-x").is_none());
    }

    #[test]
    fn parse_anthropic_message_stop_dropped() {
        let ev = anthropic_event("message_stop", r#"{"type":"message_stop"}"#);
        assert!(parse_anthropic_chunk(ev, "claude-x").is_none());
    }

    #[test]
    fn parse_anthropic_content_block_start_dropped() {
        let ev = anthropic_event(
            "content_block_start",
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        );
        assert!(parse_anthropic_chunk(ev, "claude-x").is_none());
    }

    #[test]
    fn parse_anthropic_unknown_event_type_dropped_for_forward_compat() {
        let ev = anthropic_event("future_event_type_42", r#"{"foo":"bar"}"#);
        assert!(parse_anthropic_chunk(ev, "claude-x").is_none());
    }

    #[test]
    fn parse_anthropic_invalid_json_surfaces_as_error() {
        let ev = anthropic_event("content_block_delta", "not-json{");
        let result = parse_anthropic_chunk(ev, "claude-x")
            .expect("yields error");
        match result {
            Err(BackendError::Generic { message, .. }) => {
                assert!(message.contains("failed to parse Anthropic streaming JSON"));
            }
            other => panic!("expected Generic error, got {other:?}"),
        }
    }

    #[test]
    fn parse_anthropic_input_json_delta_yields_empty_text_chunk() {
        // Tool-use streams emit input_json_delta — for 33.d we emit
        // an empty-delta chunk; the tool-use streaming surface lands
        // in v1.24.0.
        let ev = anthropic_event(
            "content_block_delta",
            r#"{"delta":{"type":"input_json_delta","partial_json":"{\"loc\":"}}"#,
        );
        let chunk = parse_anthropic_chunk(ev, "claude-x")
            .expect("yields chunk")
            .expect("valid JSON");
        assert_eq!(chunk.delta, "");
    }

    #[test]
    fn parse_anthropic_message_start_with_cache_fields() {
        let ev = anthropic_event(
            "message_start",
            r#"{"message":{"usage":{"input_tokens":10,"cache_read_input_tokens":80,"cache_creation_input_tokens":20}}}"#,
        );
        let chunk = parse_anthropic_chunk(ev, "claude-x")
            .expect("yields chunk")
            .expect("valid JSON");
        let usage = chunk.usage.expect("usage present");
        assert_eq!(usage.cache_read_tokens, 80);
        assert_eq!(usage.cache_creation_tokens, 20);
    }

    // ── complete() — early failure paths (no real HTTP) ─────────────

    #[tokio::test]
    async fn complete_without_api_key_returns_auth_error() {
        // No key, no env override → build_headers returns None →
        // typed AuthError surfaces immediately (no HTTP attempted).
        let b = AnthropicBackend::with_api_key(None);
        // Override the base URL to something the test won't actually
        // reach; the auth check fires before the HTTP call.
        let b = b.with_base_url("http://127.0.0.1:0");
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
            _ => panic!("expected Auth error, got {err:?}"),
        }
    }
}
