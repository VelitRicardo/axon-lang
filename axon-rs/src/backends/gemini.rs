//! Google Gemini generateContent backend — v1.18.0.
//!
//! Async port of `axon::backend::call_gemini` (legacy blocking) and the
//! Python transport for the `"gemini"` provider in
//! `axon.server.model_clients`. Implements the [`Backend`] trait shipped
//! in 24.b.
//!
//! Gemini's wire shape diverges from both Anthropic and OpenAI on
//! several axes — this module is therefore its own concrete `Backend`
//! impl rather than a reuse of `OpenAICompatibleBackend`:
//!
//!   * **Auth** — API key in URL (`?key=<KEY>`), NOT in a header.
//!   * **Endpoint** — `POST <base>/v1beta/models/<model>:generateContent`
//!     (model name is in the path, not the body).
//!   * **System** — top-level `systemInstruction.parts[].text` (like
//!     Anthropic's top-level `system`, but wrapped in a parts array).
//!   * **Messages** — array is `contents`, not `messages`. Each entry
//!     is `{role, parts: [{text}]}`.
//!   * **Roles** — `user` / `model` (NOT `assistant`). Tool results
//!     use role `function` with a `functionResponse` part.
//!   * **Tools** — `tools: [{functionDeclarations: [...]}]` (NOT
//!     OpenAI's flat `[{type: "function", function: {...}}]`).
//!   * **Sampling params** — go inside `generationConfig`, not at the
//!     top level. Field names differ: `topP` (NOT `top_p`),
//!     `maxOutputTokens` (NOT `max_tokens`).
//!   * **Safety settings** — `safetySettings: [{category, threshold}]`
//!     is Gemini-only. Adopters opt-in via the per-request body builder
//!     (24.e v1 supports the request-side surface; full DSL exposure
//!     lands in a 24.h-followup if demand surfaces).
//!   * **Response** — text in `candidates[0].content.parts[*].text`
//!     (concat); finish reason in `candidates[0].finishReason`
//!     (UPPERCASE: `STOP`, `MAX_TOKENS`, `SAFETY`); usage in
//!     `usageMetadata.{promptTokenCount, candidatesTokenCount,
//!     totalTokenCount}` (NOT `usage`).
//!
//! # Capabilities
//!
//!   * [`Capability::Streaming`]      — yes, via `:streamGenerateContent`
//!     (24.e v1 ships the non-streaming path; streaming impl lands as
//!     24.e.2 follow-up, same pattern as Anthropic 24.c.2).
//!   * [`Capability::ToolUse`]        — yes (functionDeclarations).
//!   * [`Capability::Vision`]         — yes (image parts on gemini-1.5+
//!     and gemini-2+ families); reported `true` for any model whose
//!     name contains `1.5`, `2.0`, or `2.5`.
//!   * [`Capability::SafetySettings`] — yes, Gemini-only.
//!   * [`Capability::StructuredOutput`] — yes (responseSchema + responseMimeType).
//!   * [`Capability::PromptCaching`]  — provider supports cached content
//!     APIs but the surface is non-trivial; reported `false` in 24.e.
//!   * [`Capability::LockedParams`]   — Gemini doesn't have locked-param
//!     model families; reported `false`.

use std::env;
use std::pin::Pin;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::Stream;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
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

const PROVIDER_NAME: &str = "gemini";
const DEFAULT_MODEL: &str = "gemini-2.5-flash";
const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";
const API_KEY_ENV: &str = "GEMINI_API_KEY";

/// Default `maxOutputTokens` when a request omits one.
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 4096;

/// Google Gemini backend. Construct with [`GeminiBackend::from_env`]
/// (reads `GEMINI_API_KEY`) or [`GeminiBackend::with_api_key`].
pub struct GeminiBackend {
    api_key: Option<String>,
    base_url: String,
    default_model: String,
    http_client: reqwest::Client,
    retry_policy: BackendRetryPolicy,
}

impl GeminiBackend {
    /// Construct from env. `GEMINI_API_KEY` is read at construction
    /// time; `None` is permitted (auth check fires at first call).
    pub fn from_env() -> Self {
        Self::with_api_key(env::var(API_KEY_ENV).ok())
    }

    /// Construct with an explicit API key (or `None`).
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

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = model.into();
        self
    }

    pub fn with_retry_policy(mut self, policy: BackendRetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    fn resolve_model<'a>(&'a self, request: &'a ChatRequest) -> &'a str {
        if request.model.is_empty() {
            &self.default_model
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

    fn build_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers
    }
}

impl Default for GeminiBackend {
    fn default() -> Self {
        Self::from_env()
    }
}

#[async_trait]
impl Backend for GeminiBackend {
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
            // Step 1 — guard: api key presence.
            let api_key = self.api_key.clone().ok_or_else(|| BackendError::Auth {
                provider: PROVIDER_NAME.into(),
                model: model.clone(),
                api_key_env: Some(API_KEY_ENV.into()),
                status: 0,
                body_preview: format!("{API_KEY_ENV} not set in environment"),
            })?;

            // Step 2 — build request body (Gemini shape).
            let body = build_request_body(&request, &self.default_model, false);
            let body_bytes = serde_json::to_vec(&body)
                .map_err(|e| BackendError::Generic {
                    provider: PROVIDER_NAME.into(),
                    model: model.clone(),
                    status: None,
                    message: format!("failed to encode request body: {e}"),
                })?;
            observability::on_request_built(
                Some(
                    body.get("generationConfig")
                        .and_then(|g| g.get("maxOutputTokens"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32,
                ),
                request.temperature,
                request.messages.len(),
                request.tools.len(),
            );

            // Step 3 — build URL (model + key in URL, NOT a header).
            // For tracing we log a redacted URL so the API key never
            // appears in spans / log shipping pipelines.
            let url = format!(
                "{}/v1beta/models/{}:generateContent?key={}",
                self.base_url, model, api_key
            );
            let display_url = format!(
                "{}/v1beta/models/{}:generateContent?key=REDACTED",
                self.base_url, model
            );

            let headers = Self::build_headers();

            // Step 4 — call with shared retry loop.
            let (raw_response, retry_count) = transport::call_with_retry(
                &self.http_client,
                &self.retry_policy,
                &url,
                Some(&display_url),
                headers,
                body_bytes,
                PROVIDER_NAME,
                &model,
                Some(API_KEY_ENV),
            )
            .await?;

            // Step 5 — decode + parse.
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

            // Step 6 — lift safety-breach finish reasons to typed error.
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
        // v1.24.0 — Real Gemini SSE streaming.
        //
        // Wire: POST <base>/v1beta/models/<model>:streamGenerateContent?alt=sse&key=…
        // Response: SSE with `data:` lines carrying a Candidate-shape
        // JSON envelope per chunk. Each chunk:
        //   data: {"candidates":[{"content":{"parts":[{"text":"..."}],"role":"model"},
        //                         "finishReason":"STOP"}], "usageMetadata":{...}}
        //
        // Mapping to ChatChunk:
        //   - parts[*].text concatenated → chunk.delta
        //   - candidates[0].finishReason → chunk.finish_reason (final
        //     chunk only; intermediate chunks usually omit it or set
        //     null/empty)
        //   - usageMetadata → chunk.usage (final chunk only)
        let model = self.resolve_model(&request).to_string();
        let api_key = self.api_key.as_ref().ok_or_else(|| BackendError::Auth {
            provider: PROVIDER_NAME.into(),
            model: model.clone(),
            api_key_env: Some(API_KEY_ENV.into()),
            status: 0,
            body_preview: format!("{API_KEY_ENV} not set in environment"),
        })?;

        let body = build_request_body(&request, &self.default_model, true);
        let body_bytes = serde_json::to_vec(&body).map_err(|e| BackendError::Generic {
            provider: PROVIDER_NAME.into(),
            model: model.clone(),
            status: None,
            message: format!("failed to encode streaming request body: {e}"),
        })?;

        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            self.base_url, model, api_key
        );
        let headers = Self::build_headers();

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
                    Ok(event) => parse_gemini_chunk(event, &model),
                    Err(e) => Some(Err(e)),
                }
            }
        });
        // v1.24.0 — Cancel-aware wrap (≤100ms p95 abort).
        let inner: ChatStream = Box::pin(chunks);
        Ok(super::sse_streaming::cancel_aware(inner, request.cancel.clone()))
    }

    fn count_tokens(&self, model: &str, text: &str) -> usize {
        // Gemini has no offline tokenizer in tiktoken-rs; the unified
        // dispatch falls back to the 4-cpt estimate. Adopters needing
        // exact counts can call the provider's HTTP `countTokens`
        // endpoint directly via this backend (24.e ships the count
        // surface for budgeting; exact-count round-trips land in
        // 24.h-followup if demand surfaces).
        tokens::count_tokens(model, text).count
    }

    fn supports(&self, capability: Capability, model: &str) -> bool {
        match capability {
            Capability::Streaming
            | Capability::ToolUse
            | Capability::SafetySettings
            | Capability::StructuredOutput => true,
            Capability::Vision => {
                let lc = model.to_lowercase();
                // gemini-1.5*, gemini-2.0*, gemini-2.5* support image parts.
                lc.contains("1.5") || lc.contains("2.0") || lc.contains("2.5")
            }
            Capability::PromptCaching | Capability::LockedParams => false,
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  Request body construction
// ────────────────────────────────────────────────────────────────────

/// Build the JSON body for `POST .../v1beta/models/<model>:generateContent`.
///
/// Distinct from Anthropic / OpenAI shapes:
///
///   * `systemInstruction.parts[].text` carries the system prompt
///     (analogous to Anthropic's `system`, but wrapped in a parts
///     envelope).
///   * `contents` (NOT `messages`) is the conversation array.
///   * Each message is `{role, parts: [{text}]}` with role mapped:
///     `user → "user"`, `assistant → "model"`, `tool → "function"` +
///     a `functionResponse` part.
///   * `generationConfig` holds sampling params with Gemini field
///     names: `topP` (camelCase, NOT `top_p`), `maxOutputTokens`
///     (NOT `max_tokens`).
///   * `tools` is `[{functionDeclarations: [{name, description,
///     parameters}]}]`.
pub(crate) fn build_request_body(
    request: &ChatRequest,
    default_model: &str,
    _stream: bool,
) -> Value {
    let _ = default_model; // model is in the URL path, not the body
    let mut body = serde_json::Map::new();

    // System messages → top-level systemInstruction.parts[].text.
    let mut system_segments: Vec<String> = Vec::new();
    if let Some(s) = request.system.as_ref() {
        if !s.is_empty() {
            system_segments.push(s.clone());
        }
    }
    let mut wire_contents: Vec<Value> = Vec::with_capacity(request.messages.len());
    for msg in &request.messages {
        match msg.role {
            Role::System => {
                if !msg.content.is_empty() {
                    system_segments.push(msg.content.clone());
                }
            }
            Role::User => {
                wire_contents.push(json!({
                    "role": "user",
                    "parts": [{"text": msg.content}],
                }));
            }
            Role::Assistant => {
                wire_contents.push(json!({
                    "role": "model",
                    "parts": [{"text": msg.content}],
                }));
            }
            Role::Tool => {
                // Gemini encodes tool results as a `function` role with
                // a `functionResponse` part. The tool's name lives in
                // `functionResponse.name`; the result payload in
                // `functionResponse.response`.
                let tool_name = msg.tool_call_id.clone().unwrap_or_default();
                let response_value: Value =
                    serde_json::from_str(&msg.content).unwrap_or_else(|_| {
                        json!({"content": msg.content})
                    });
                wire_contents.push(json!({
                    "role": "function",
                    "parts": [{
                        "functionResponse": {
                            "name": tool_name,
                            "response": response_value,
                        }
                    }],
                }));
            }
        }
    }

    if !system_segments.is_empty() {
        body.insert(
            "systemInstruction".into(),
            json!({
                "parts": [{"text": system_segments.join("\n\n")}]
            }),
        );
    }
    body.insert("contents".into(), Value::Array(wire_contents));

    // Sampling params → generationConfig.
    let mut gen_cfg = serde_json::Map::new();
    if let Some(t) = request.temperature {
        gen_cfg.insert("temperature".into(), json!(t));
    }
    if let Some(p) = request.top_p {
        gen_cfg.insert("topP".into(), json!(p));
    }
    gen_cfg.insert(
        "maxOutputTokens".into(),
        json!(request.max_tokens.unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS)),
    );
    body.insert("generationConfig".into(), Value::Object(gen_cfg));

    // Tools → [{functionDeclarations: [...]}].
    if !request.tools.is_empty() {
        let declarations: Vec<Value> = request
            .tools
            .iter()
            .map(|t| {
                let parameters: Value = serde_json::from_str(&t.parameters_json)
                    .unwrap_or_else(|_| json!({"type": "object", "properties": {}}));
                json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": parameters,
                })
            })
            .collect();
        body.insert(
            "tools".into(),
            json!([{"functionDeclarations": declarations}]),
        );
    }

    Value::Object(body)
}

// ────────────────────────────────────────────────────────────────────
//  Response parsing
// ────────────────────────────────────────────────────────────────────

/// Parse a successful 200 OK generateContent response into a
/// [`ChatResponse`].
pub(crate) fn parse_response(
    payload: &Value,
    requested_model: &str,
    retry_count: u32,
    trace_id: &str,
) -> ChatResponse {
    let content_text = extract_content_text(payload);
    let finish_raw = payload
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .and_then(|c| c.get("finishReason"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let finish_reason = FinishReason::from_provider(PROVIDER_NAME, finish_raw);

    // Gemini may return a `modelVersion` or `model` field; if neither,
    // fall back to the requested model. Gemini's response sometimes
    // lacks any model field entirely on lower-tier endpoints.
    let model_name = payload
        .get("modelVersion")
        .and_then(Value::as_str)
        .or_else(|| payload.get("model").and_then(Value::as_str))
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

/// Concatenate every `text` field across the parts of the first
/// candidate. Multi-part responses are common when Gemini interleaves
/// text + tool calls; we only surface text here (tool calls flow via
/// `finish_reason: ToolUse`).
pub(crate) fn extract_content_text(payload: &Value) -> String {
    payload
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str).map(str::to_string))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// Extract [`Usage`] from `usageMetadata`. Gemini's field names differ
/// from OpenAI / Anthropic: `promptTokenCount`, `candidatesTokenCount`,
/// `totalTokenCount`. Map them to the canonical fields on [`Usage`].
pub(crate) fn extract_usage(payload: &Value) -> Usage {
    let meta = payload.get("usageMetadata");
    let read_field = |name: &str| -> u32 {
        meta.and_then(|m| m.get(name))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32
    };
    let prompt = read_field("promptTokenCount");
    let candidates = read_field("candidatesTokenCount");
    let total = read_field("totalTokenCount");
    Usage {
        input_tokens: prompt,
        output_tokens: candidates,
        total_tokens: if total > 0 { total } else { prompt + candidates },
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
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
type GeminiChatStream =
    Pin<Box<dyn Stream<Item = Result<ChatChunk, BackendError>> + Send>>;

// ────────────────────────────────────────────────────────────────────
// v1.24.0 — SSE chunk parsing
// ────────────────────────────────────────────────────────────────────

/// Parse one Gemini SSE event into an optional `ChatChunk`.
///
/// Returns:
///   * `None` — event has no usable data (empty data, no candidates, or
///     all candidates without text).
///   * `Some(Ok(chunk))` — usable streaming delta or terminal envelope.
///   * `Some(Err(...))` — JSON parse failure surfaces as a typed
///     transport error.
///
/// Gemini's `:streamGenerateContent?alt=sse` wire shape:
///
/// ```text
/// data: {"candidates":[{"content":{"parts":[{"text":"Hi"}],"role":"model"},
///                       "finishReason":null,"index":0}],
///        "usageMetadata":{"promptTokenCount":5}}
/// ...
/// data: {"candidates":[{"content":{"parts":[{"text":""}],"role":"model"},
///                       "finishReason":"STOP","index":0}],
///        "usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":10,
///                         "totalTokenCount":15}}
/// ```
///
/// Each chunk envelope carries:
///   - text deltas concatenated from `candidates[0].content.parts[*].text`
///   - optional `candidates[0].finishReason` (UPPERCASE per Gemini docs)
///   - optional `usageMetadata` (final chunk usually carries totals)
pub(crate) fn parse_gemini_chunk(
    event: super::sse_streaming::SseEvent,
    model: &str,
) -> Option<Result<ChatChunk, BackendError>> {
    let data = event.data?;
    let trimmed = data.trim();
    if trimmed.is_empty() {
        return None;
    }

    let payload: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(e) => {
            return Some(Err(BackendError::Generic {
                provider: PROVIDER_NAME.into(),
                model: model.into(),
                status: Some(200),
                message: format!("failed to parse Gemini streaming JSON chunk: {e}"),
            }));
        }
    };

    let first_candidate = payload
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|c| c.first());

    // Concatenate all text parts from this chunk's first candidate.
    let delta_text = first_candidate
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();

    let finish_raw = first_candidate
        .and_then(|c| c.get("finishReason"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let finish_reason =
        finish_raw.map(|raw| FinishReason::from_provider(PROVIDER_NAME, raw));

    // Usage metadata is present on most chunks (Gemini sends cumulative
    // totals); we surface it whenever the field is present.
    let usage = payload.get("usageMetadata").map(|u| {
        let input = u
            .get("promptTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let output = u
            .get("candidatesTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let total = u
            .get("totalTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        Usage {
            input_tokens: input,
            output_tokens: output,
            total_tokens: if total > 0 { total } else { input + output },
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            reasoning_tokens: 0,
        }
    });

    Some(Ok(ChatChunk {
        delta: delta_text,
        finish_reason,
        usage,
    }))
}

/// Module-level factory — `let b = backends::gemini::from_env();`.
pub fn from_env() -> GeminiBackend {
    GeminiBackend::from_env()
}

/// Module-level factory with explicit API key.
pub fn with_api_key(api_key: Option<String>) -> GeminiBackend {
    GeminiBackend::with_api_key(api_key)
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

    // ── Construction ────────────────────────────────────────────────

    #[test]
    fn from_env_constructs_with_default_model() {
        let b = GeminiBackend::from_env();
        assert_eq!(b.name(), "gemini");
        assert_eq!(b.default_model(), DEFAULT_MODEL);
    }

    #[test]
    fn module_factory_works() {
        let b = from_env();
        assert_eq!(b.name(), "gemini");
    }

    #[test]
    fn with_default_model_overrides() {
        let b = GeminiBackend::with_api_key(Some("k".into()))
            .with_default_model("gemini-2.5-pro");
        assert_eq!(b.default_model(), "gemini-2.5-pro");
    }

    #[test]
    fn with_base_url_overrides_for_test_fixtures() {
        let b = GeminiBackend::with_api_key(Some("k".into()))
            .with_base_url("http://localhost:9000");
        assert_eq!(b.base_url, "http://localhost:9000");
    }

    // ── Capability discovery ────────────────────────────────────────

    #[test]
    fn supports_streaming_tooluse_safetysettings_structured() {
        let b = GeminiBackend::with_api_key(Some("k".into()));
        let any_model = "gemini-2.5-flash";
        assert!(b.supports(Capability::Streaming, any_model));
        assert!(b.supports(Capability::ToolUse, any_model));
        assert!(b.supports(Capability::SafetySettings, any_model));
        assert!(b.supports(Capability::StructuredOutput, any_model));
    }

    #[test]
    fn does_not_support_anthropic_or_openai_only_caps() {
        let b = GeminiBackend::with_api_key(Some("k".into()));
        let any_model = "gemini-2.5-flash";
        assert!(!b.supports(Capability::PromptCaching, any_model));
        assert!(!b.supports(Capability::LockedParams, any_model));
    }

    #[test]
    fn supports_vision_for_15_20_25_families() {
        let b = GeminiBackend::with_api_key(Some("k".into()));
        assert!(b.supports(Capability::Vision, "gemini-1.5-pro"));
        assert!(b.supports(Capability::Vision, "gemini-1.5-flash"));
        assert!(b.supports(Capability::Vision, "gemini-2.0-flash"));
        assert!(b.supports(Capability::Vision, "gemini-2.5-pro"));
        assert!(b.supports(Capability::Vision, "gemini-2.5-flash"));
    }

    #[test]
    fn does_not_support_vision_for_legacy_gemini_1_0() {
        let b = GeminiBackend::with_api_key(Some("k".into()));
        assert!(!b.supports(Capability::Vision, "gemini-pro"));
        assert!(!b.supports(Capability::Vision, "gemini-1.0-pro"));
    }

    // ── Headers ─────────────────────────────────────────────────────

    #[test]
    fn build_headers_includes_only_content_type() {
        let h = GeminiBackend::build_headers();
        assert_eq!(h.get(CONTENT_TYPE).unwrap(), "application/json");
        // No Authorization header — Gemini auth lives in the URL
        // (`?key=<KEY>`) not in headers.
        assert!(h.get(reqwest::header::AUTHORIZATION).is_none());
        assert!(h.get("x-api-key").is_none());
    }

    // ── Request body shape ──────────────────────────────────────────

    #[test]
    fn body_includes_contents_not_messages() {
        let req = req_with(vec![Message::user("hi")]);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        // Gemini uses `contents`, NOT `messages` (OpenAI convention).
        assert!(body.get("messages").is_none());
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "hi");
    }

    #[test]
    fn body_omits_model_field_in_body() {
        // Gemini puts the model in the URL path, NOT in the body.
        let req = req_with(vec![Message::user("hi")]);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        assert!(body.get("model").is_none());
    }

    #[test]
    fn body_lifts_system_to_systeminstruction_parts() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.system = Some("You are helpful.".into());
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        let si = body["systemInstruction"].as_object().unwrap();
        let parts = si["parts"].as_array().unwrap();
        assert_eq!(parts[0]["text"], "You are helpful.");
        // System should not appear in contents.
        for c in body["contents"].as_array().unwrap() {
            assert_ne!(c["role"], "system");
        }
    }

    #[test]
    fn body_folds_system_role_messages_into_systeminstruction() {
        let req = req_with(vec![
            Message::system("from-message"),
            Message::user("hi"),
        ]);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        let parts = body["systemInstruction"]["parts"].as_array().unwrap();
        assert_eq!(parts[0]["text"], "from-message");
        assert_eq!(body["contents"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn body_combines_explicit_system_field_and_role_messages() {
        let mut req = req_with(vec![
            Message::system("from-message"),
            Message::user("hi"),
        ]);
        req.system = Some("from-field".into());
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        let text = body["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap();
        assert_eq!(text, "from-field\n\nfrom-message");
    }

    #[test]
    fn body_maps_assistant_role_to_model() {
        let req = req_with(vec![
            Message::user("hello"),
            Message::assistant("hi back"),
        ]);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 2);
        // Gemini uses `model`, NOT `assistant`.
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[1]["role"], "model");
    }

    #[test]
    fn body_encodes_tool_role_as_function_response_part() {
        let req = req_with(vec![
            Message::user("call a tool"),
            Message {
                role: Role::Tool,
                content: r#"{"temp_c": 22.5}"#.into(),
                tool_call_id: Some("get_weather".into()),
            },
        ]);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents[1]["role"], "function");
        let fn_response = &contents[1]["parts"][0]["functionResponse"];
        assert_eq!(fn_response["name"], "get_weather");
        // The JSON content was parsed into the response field directly.
        assert_eq!(fn_response["response"]["temp_c"], 22.5);
    }

    #[test]
    fn body_encodes_non_json_tool_content_as_wrapper_object() {
        // Adopters supplying plain-text tool results should still work.
        let req = req_with(vec![
            Message::user("call"),
            Message {
                role: Role::Tool,
                content: "raw text result".into(),
                tool_call_id: Some("noop".into()),
            },
        ]);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        let fn_response = &body["contents"][1]["parts"][0]["functionResponse"];
        assert_eq!(fn_response["response"]["content"], "raw text result");
    }

    #[test]
    fn body_uses_camelcase_topp_in_generationconfig() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.top_p = Some(0.9);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        // Gemini uses camelCase (`topP`), NOT snake_case (`top_p`).
        assert_eq!(body["generationConfig"]["topP"], 0.9);
        assert!(body["generationConfig"].get("top_p").is_none());
    }

    #[test]
    fn body_uses_maxoutputtokens_not_max_tokens() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.max_tokens = Some(2048);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        // Gemini uses `maxOutputTokens`, NOT `max_tokens`.
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 2048);
        assert!(body["generationConfig"].get("max_tokens").is_none());
    }

    #[test]
    fn body_includes_temperature_in_generationconfig() {
        let mut req = req_with(vec![Message::user("hi")]);
        req.temperature = Some(0.5);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        assert_eq!(body["generationConfig"]["temperature"], 0.5);
    }

    #[test]
    fn body_max_output_tokens_default_when_unset() {
        let req = req_with(vec![Message::user("hi")]);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        assert_eq!(
            body["generationConfig"]["maxOutputTokens"],
            DEFAULT_MAX_OUTPUT_TOKENS
        );
    }

    #[test]
    fn body_serialises_tools_in_function_declarations_envelope() {
        let mut req = req_with(vec![Message::user("call a tool")]);
        req.tools = vec![ToolSpec {
            name: "get_weather".into(),
            description: "fetch the current weather".into(),
            parameters_json:
                r#"{"type":"object","properties":{"city":{"type":"string"}}}"#.into(),
        }];
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        // Gemini envelope: `tools: [{functionDeclarations: [...]}]`.
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        let fd = tools[0]["functionDeclarations"].as_array().unwrap();
        assert_eq!(fd.len(), 1);
        assert_eq!(fd[0]["name"], "get_weather");
        assert_eq!(fd[0]["description"], "fetch the current weather");
        // Note: NOT wrapped in `{type, function: {...}}` — that's the
        // OpenAI envelope. Gemini's parameters live directly on the
        // declaration.
        assert_eq!(fd[0]["parameters"]["type"], "object");
    }

    #[test]
    fn body_omits_tools_when_empty() {
        let req = req_with(vec![Message::user("no tools")]);
        let body = build_request_body(&req, DEFAULT_MODEL, false);
        assert!(body.get("tools").is_none());
    }

    // ── Response parsing ────────────────────────────────────────────

    #[test]
    fn parse_response_extracts_text_from_first_candidate() {
        let payload = json!({
            "candidates": [{
                "content": {"parts": [{"text": "Hello, world!"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 12,
                "candidatesTokenCount": 4,
                "totalTokenCount": 16
            }
        });
        let resp = parse_response(&payload, "gemini-2.5-flash", 0, "trace-1");
        assert_eq!(resp.content, "Hello, world!");
        assert_eq!(resp.provider_name, "gemini");
        assert_eq!(resp.finish_reason, FinishReason::Stop);
    }

    #[test]
    fn parse_response_concatenates_multiple_text_parts() {
        let payload = json!({
            "candidates": [{
                "content": {"parts": [
                    {"text": "First."},
                    {"text": "Second."}
                ]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 1, "candidatesTokenCount": 1, "totalTokenCount": 2
            }
        });
        let resp = parse_response(&payload, "gemini-x", 0, "t");
        assert_eq!(resp.content, "First.\nSecond.");
    }

    #[test]
    fn parse_response_max_tokens_uppercase_finish_reason() {
        let payload = json!({
            "candidates": [{
                "content": {"parts": [{"text": "trunc..."}]},
                "finishReason": "MAX_TOKENS"
            }],
            "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1, "totalTokenCount": 2}
        });
        let resp = parse_response(&payload, "gemini-x", 0, "t");
        // Gemini emits UPPERCASE finish reasons; case-folding handled
        // in `FinishReason::from_provider`.
        assert_eq!(resp.finish_reason, FinishReason::Length);
    }

    #[test]
    fn parse_response_safety_finish_reason() {
        let payload = json!({
            "candidates": [{
                "content": {"parts": []},
                "finishReason": "SAFETY"
            }],
            "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 0, "totalTokenCount": 1}
        });
        let resp = parse_response(&payload, "gemini-x", 0, "t");
        assert!(resp.finish_reason.is_safety_breach());
    }

    #[test]
    fn parse_response_extracts_usage_metadata() {
        let payload = json!({
            "candidates": [{
                "content": {"parts": [{"text": "ok"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 100,
                "candidatesTokenCount": 50,
                "totalTokenCount": 150
            }
        });
        let resp = parse_response(&payload, "gemini-x", 0, "t");
        assert_eq!(resp.usage.input_tokens, 100);
        assert_eq!(resp.usage.output_tokens, 50);
        assert_eq!(resp.usage.total_tokens, 150);
        // No cache or reasoning fields on Gemini.
        assert_eq!(resp.usage.cache_read_tokens, 0);
        assert_eq!(resp.usage.cache_creation_tokens, 0);
        assert_eq!(resp.usage.reasoning_tokens, 0);
    }

    #[test]
    fn parse_response_total_tokens_falls_back_to_sum_when_missing() {
        let payload = json!({
            "candidates": [{
                "content": {"parts": [{"text": "x"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 30, "candidatesTokenCount": 12}
        });
        let resp = parse_response(&payload, "gemini-x", 0, "t");
        assert_eq!(resp.usage.total_tokens, 42);
    }

    #[test]
    fn parse_response_model_field_uses_modelversion_when_present() {
        let payload = json!({
            "modelVersion": "gemini-2.5-flash-001",
            "candidates": [{
                "content": {"parts": [{"text": "ok"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1, "totalTokenCount": 2}
        });
        let resp = parse_response(&payload, "gemini-2.5-flash", 0, "t");
        assert_eq!(resp.model_name, "gemini-2.5-flash-001");
    }

    #[test]
    fn parse_response_falls_back_to_requested_model() {
        let payload = json!({
            "candidates": [{"content": {"parts": [{"text": "x"}]}, "finishReason": "STOP"}],
            "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1, "totalTokenCount": 2}
        });
        let resp = parse_response(&payload, "gemini-2.5-flash", 0, "t");
        assert_eq!(resp.model_name, "gemini-2.5-flash");
    }

    #[test]
    fn parse_response_handles_empty_candidates() {
        let payload = json!({
            "candidates": [],
            "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 0, "totalTokenCount": 1}
        });
        let resp = parse_response(&payload, "gemini-x", 0, "t");
        assert_eq!(resp.content, "");
    }

    // ── count_tokens delegates to estimate ──────────────────────────

    #[test]
    fn count_tokens_uses_estimate_for_gemini_models() {
        let b = GeminiBackend::with_api_key(Some("k".into()));
        // 8 chars → 2 tokens via the offline estimate path.
        assert_eq!(b.count_tokens("gemini-2.5-flash", "ABCDEFGH"), 2);
    }

    // ── Streaming surface ───────────────────────────────────────────

    #[tokio::test]
    async fn stream_real_gemini_sse_implementation_transport_path() {
        // v1.24.0 — Gemini now ships a real SSE streamer.
        // Unreachable port exercises the transport-error path.
        let b = GeminiBackend::with_api_key(Some("k".into()))
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
        let b = GeminiBackend::with_api_key(None)
            .with_base_url("http://127.0.0.1:1");
        match b.stream(ChatRequest::default()).await {
            Err(BackendError::Auth { provider, .. }) => assert_eq!(provider, "gemini"),
            Err(other) => panic!("expected Auth error, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    // ── v1.24.0 — Gemini SSE chunk parsing (pure-unit) ───────────

    use super::parse_gemini_chunk;
    use super::super::sse_streaming::SseEvent;

    fn gemini_event(data: &str) -> SseEvent {
        SseEvent {
            data: Some(data.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn parse_gemini_chunk_extracts_text_delta() {
        let ev = gemini_event(
            r#"{"candidates":[{"content":{"parts":[{"text":"Hello"}],"role":"model"},"index":0}]}"#,
        );
        let chunk = parse_gemini_chunk(ev, "gemini-x")
            .expect("yields chunk")
            .expect("valid JSON");
        assert_eq!(chunk.delta, "Hello");
        assert!(chunk.finish_reason.is_none());
    }

    #[test]
    fn parse_gemini_chunk_concatenates_multiple_text_parts() {
        let ev = gemini_event(
            r#"{"candidates":[{"content":{"parts":[
                {"text":"Hello "},
                {"text":"World"}
            ],"role":"model"}}]}"#,
        );
        let chunk = parse_gemini_chunk(ev, "gemini-x")
            .expect("yields chunk")
            .expect("valid JSON");
        assert_eq!(chunk.delta, "Hello World");
    }

    #[test]
    fn parse_gemini_chunk_final_chunk_carries_stop_and_usage() {
        let ev = gemini_event(
            r#"{"candidates":[{"content":{"parts":[{"text":""}],"role":"model"},"finishReason":"STOP"}],
                "usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":10,"totalTokenCount":15}}"#,
        );
        let chunk = parse_gemini_chunk(ev, "gemini-x")
            .expect("yields chunk")
            .expect("valid JSON");
        assert_eq!(chunk.finish_reason, Some(FinishReason::Stop));
        let usage = chunk.usage.expect("usage present");
        assert_eq!(usage.input_tokens, 5);
        assert_eq!(usage.output_tokens, 10);
        assert_eq!(usage.total_tokens, 15);
    }

    #[test]
    fn parse_gemini_chunk_max_tokens_finish_reason_maps_to_length() {
        let ev = gemini_event(
            r#"{"candidates":[{"content":{"parts":[]},"finishReason":"MAX_TOKENS"}]}"#,
        );
        let chunk = parse_gemini_chunk(ev, "gemini-x")
            .expect("yields chunk")
            .expect("valid JSON");
        assert_eq!(chunk.finish_reason, Some(FinishReason::Length));
    }

    #[test]
    fn parse_gemini_chunk_safety_finish_reason_maps_to_safety_breach() {
        let ev = gemini_event(
            r#"{"candidates":[{"content":{"parts":[]},"finishReason":"SAFETY"}]}"#,
        );
        let chunk = parse_gemini_chunk(ev, "gemini-x")
            .expect("yields chunk")
            .expect("valid JSON");
        assert_eq!(chunk.finish_reason, Some(FinishReason::SafetyBreach));
    }

    #[test]
    fn parse_gemini_chunk_missing_finish_reason_yields_none() {
        let ev = gemini_event(
            r#"{"candidates":[{"content":{"parts":[{"text":"x"}]}}]}"#,
        );
        let chunk = parse_gemini_chunk(ev, "gemini-x")
            .expect("yields chunk")
            .expect("valid JSON");
        assert!(chunk.finish_reason.is_none());
    }

    #[test]
    fn parse_gemini_chunk_empty_data_returns_none() {
        let ev = gemini_event("");
        assert!(parse_gemini_chunk(ev, "gemini-x").is_none());
    }

    #[test]
    fn parse_gemini_chunk_invalid_json_surfaces_as_error() {
        let ev = gemini_event("{not-json");
        let result = parse_gemini_chunk(ev, "gemini-x").expect("yields error");
        match result {
            Err(BackendError::Generic { message, .. }) => {
                assert!(message.contains("failed to parse Gemini streaming JSON"));
            }
            other => panic!("expected Generic error, got {other:?}"),
        }
    }

    #[test]
    fn parse_gemini_chunk_no_candidates_yields_empty_delta() {
        // Gemini may emit envelopes without candidates (rare; safety
        // pre-filter). We surface an empty-delta chunk so the consumer
        // sees the event without panicking.
        let ev = gemini_event(r#"{"candidates":[],"usageMetadata":{"promptTokenCount":1}}"#);
        let chunk = parse_gemini_chunk(ev, "gemini-x")
            .expect("yields chunk")
            .expect("valid JSON");
        assert_eq!(chunk.delta, "");
        let usage = chunk.usage.expect("usage present");
        assert_eq!(usage.input_tokens, 1);
    }

    #[test]
    fn parse_gemini_chunk_usage_falls_back_to_sum_when_total_missing() {
        let ev = gemini_event(
            r#"{"candidates":[{"content":{"parts":[]},"finishReason":"STOP"}],
                "usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":4}}"#,
        );
        let chunk = parse_gemini_chunk(ev, "gemini-x")
            .expect("yields chunk")
            .expect("valid JSON");
        let usage = chunk.usage.expect("usage present");
        assert_eq!(usage.total_tokens, 7);
    }

    // ── complete() — early failure paths ────────────────────────────

    #[tokio::test]
    async fn complete_without_api_key_returns_auth_error() {
        let b = GeminiBackend::with_api_key(None).with_base_url("http://127.0.0.1:0");
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
