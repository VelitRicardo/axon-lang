//! AXON Backend — Multi-provider LLM API clients.
//!
//! Supports: Anthropic, OpenAI, Gemini, Kimi, GLM, OpenRouter, Ollama.
//! Uses blocking HTTP (reqwest::blocking) — suitable for CLI.
//!
//! Three API families:
//!   - Anthropic: Claude Messages API (system field + x-api-key header)
//!   - Gemini: Google generateContent (systemInstruction + API key in URL)
//!   - OpenAI-compatible: OpenAI, Kimi, GLM, OpenRouter, Ollama (Bearer auth + chat/completions)
//!
//! API key sourced from environment: <PROVIDER>_API_KEY.
//!
//! # v1.24.0 — Retirement-in-progress
//!
//! This module is the LEGACY synchronous LLM-call surface. The
//! production async path uses [`crate::backends::Backend`] +
//! [`crate::backends::Registry`] instead. As of v1.25.0 (33.x.i):
//!
//!   - [`SUPPORTED_BACKENDS`] is now a `pub use` re-export of the
//!     consolidated [`crate::backends::CANONICAL_PROVIDERS`] —
//!     single source of truth + drift-gated.
//!   - [`get_api_key`] delegates to
//!     [`crate::backends::get_api_key`] for the canonical env-var
//!     resolution (kept here as a thin shim that wraps the result
//!     in the legacy [`BackendError`] shape so existing callers
//!     don't need to change types).
//!   - The synchronous [`call`] / [`call_multi`] / [`call_stream`]
//!     / [`call_multi_stream`] functions are marked
//!     `#[deprecated]`. New code MUST use the async `Backend`
//!     trait via the [`crate::backends::Registry`] surface.
//!
//! The 4 caller files (runner.rs, axon_server.rs,
//! resilient_backend.rs, tenant_secrets.rs) carry a top-level
//! `#![allow(deprecated)]` while the deeper async migration of
//! the synchronous CLI/server paths progresses under a
//! followup step (v1.24.0 — sync→async migration of the
//! 4 callers, separate from this consolidation cycle).

#![allow(deprecated)]

use serde_json::{json, Value};
use std::io::{BufRead, BufReader};

const DEFAULT_MAX_TOKENS: u32 = 4096;

// ── Provider specifications ─────────────────────────────────────────────────

struct ProviderSpec {
    base_url: &'static str,
    default_model: &'static str,
    api_family: ApiFamily,
}

#[derive(Clone, Copy, PartialEq)]
enum ApiFamily {
    Anthropic,
    Gemini,
    OpenAICompatible,
}

fn provider_spec(name: &str) -> Option<ProviderSpec> {
    match name {
        "anthropic" => Some(ProviderSpec {
            base_url: "https://api.anthropic.com",
            default_model: "claude-sonnet-4-20250514",
            api_family: ApiFamily::Anthropic,
        }),
        "openai" => Some(ProviderSpec {
            base_url: "https://api.openai.com",
            default_model: "gpt-4o-mini",
            api_family: ApiFamily::OpenAICompatible,
        }),
        "gemini" => Some(ProviderSpec {
            base_url: "https://generativelanguage.googleapis.com",
            default_model: "gemini-2.0-flash",
            api_family: ApiFamily::Gemini,
        }),
        "kimi" => Some(ProviderSpec {
            base_url: "https://api.moonshot.ai",
            default_model: "moonshot-v1-8k",
            api_family: ApiFamily::OpenAICompatible,
        }),
        "glm" => Some(ProviderSpec {
            base_url: "https://open.bigmodel.cn/api/paas",
            default_model: "glm-4-flash",
            api_family: ApiFamily::OpenAICompatible,
        }),
        "openrouter" => Some(ProviderSpec {
            base_url: "https://openrouter.ai/api",
            default_model: "anthropic/claude-sonnet-4",
            api_family: ApiFamily::OpenAICompatible,
        }),
        "ollama" => Some(ProviderSpec {
            base_url: "http://localhost:11434",
            default_model: "llama3.2",
            api_family: ApiFamily::OpenAICompatible,
        }),
        _ => None,
    }
}

/// v1.24.0 — Re-export the consolidated single source of truth.
///
/// The canonical 7-provider list lives in
/// [`crate::backends::CANONICAL_PROVIDERS`]; this re-export keeps
/// existing call sites compiling without churn while the deeper
/// async migration (v1.24.0) progresses. Drift-gated by
/// `backends::resolver_tests::canonical_providers_equals_legacy_supported_backends`
/// + `tests/mono_file_retirement.rs`.
pub use crate::backends::CANONICAL_PROVIDERS as SUPPORTED_BACKENDS;

// ── Public types ────────────────────────────────────────────────────────────

/// Result of a single model call.
#[derive(Debug)]
pub struct ModelResponse {
    pub text: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub stop_reason: String,
}

/// Error from backend API call.
#[derive(Debug)]
pub struct BackendError {
    pub message: String,
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

// ── Public API ──────────────────────────────────────────────────────────────

/// v1.24.0 — Get the API key for a given backend from
/// environment.
///
/// This is a thin shim around [`crate::backends::get_api_key`]
/// (the consolidated source of truth) that wraps the result in
/// the legacy [`BackendError`] shape so existing callers keep
/// compiling without touching their error-handling code. New
/// code should call [`crate::backends::get_api_key`] directly +
/// use the trait-side error types.
pub fn get_api_key(backend: &str) -> Result<String, BackendError> {
    crate::backends::get_api_key(backend).map_err(|message| BackendError { message })
}

/// Call the LLM API for the given backend (blocking).
/// Dispatches to the correct API family based on provider spec.
///
/// v1.24.0 — DEPRECATED. New code MUST use the async
/// [`crate::backends::Backend`] trait via
/// [`crate::backends::Registry`]. The 4 caller files of this
/// function carry `#![allow(deprecated)]` while the deeper async
/// migration progresses under v1.24.0.
#[deprecated(
    since = "1.25.0",
    note = "use crate::backends::Registry::get(name)?.complete() instead; \
            33.x.i.2 closes the 4-caller sync→async migration"
)]
pub fn call(
    backend: &str,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    max_tokens: Option<u32>,
) -> Result<ModelResponse, BackendError> {
    let spec = provider_spec(backend).ok_or_else(|| BackendError {
        message: format!(
            "Unknown backend '{backend}'. Supported: {}",
            SUPPORTED_BACKENDS.join(", ")
        ),
    })?;

    let start = std::time::Instant::now();
    tracing::info!(
        backend = backend,
        model = spec.default_model,
        max_tokens = max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "llm_call_started"
    );

    let result = match spec.api_family {
        ApiFamily::Anthropic => call_anthropic(&spec, api_key, system_prompt, user_prompt, max_tokens),
        ApiFamily::Gemini => call_gemini(&spec, api_key, system_prompt, user_prompt, max_tokens),
        ApiFamily::OpenAICompatible => call_openai_compat(&spec, api_key, system_prompt, user_prompt, max_tokens),
    };

    let latency_ms = start.elapsed().as_millis() as u64;

    match &result {
        Ok(resp) => {
            tracing::info!(
                backend = backend,
                model = %resp.model,
                latency_ms = latency_ms,
                input_tokens = resp.input_tokens,
                output_tokens = resp.output_tokens,
                stop_reason = %resp.stop_reason,
                "llm_call_completed"
            );
        }
        Err(e) => {
            tracing::error!(
                backend = backend,
                latency_ms = latency_ms,
                error = %e,
                "llm_call_failed"
            );
        }
    }

    result
}

/// Call the LLM API with streaming — text chunks arrive via `on_chunk` callback.
/// Returns the complete `ModelResponse` after the stream ends.
/// Anchor checking and other post-processing run on the accumulated text.
/// v1.24.0 — DEPRECATED single-message streaming call.
/// New code MUST use [`crate::backends::Backend::stream`] instead.
#[deprecated(
    since = "1.25.0",
    note = "use crate::backends::Registry::get(name)?.stream() instead; \
            33.x.i.2 closes the sync→async migration"
)]
pub fn call_stream<F>(
    backend: &str,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    max_tokens: Option<u32>,
    on_chunk: F,
) -> Result<ModelResponse, BackendError>
where
    F: FnMut(&str),
{
    let spec = provider_spec(backend).ok_or_else(|| BackendError {
        message: format!(
            "Unknown backend '{backend}'. Supported: {}",
            SUPPORTED_BACKENDS.join(", ")
        ),
    })?;

    let start = std::time::Instant::now();
    tracing::info!(
        backend = backend,
        model = spec.default_model,
        streaming = true,
        "llm_stream_started"
    );

    let result = match spec.api_family {
        ApiFamily::Anthropic => stream_anthropic(&spec, api_key, system_prompt, user_prompt, max_tokens, on_chunk),
        ApiFamily::Gemini => stream_gemini(&spec, api_key, system_prompt, user_prompt, max_tokens, on_chunk),
        ApiFamily::OpenAICompatible => stream_openai_compat(&spec, api_key, system_prompt, user_prompt, max_tokens, on_chunk),
    };

    let latency_ms = start.elapsed().as_millis() as u64;

    match &result {
        Ok(resp) => {
            tracing::info!(
                backend = backend,
                model = %resp.model,
                latency_ms = latency_ms,
                input_tokens = resp.input_tokens,
                output_tokens = resp.output_tokens,
                "llm_stream_completed"
            );
        }
        Err(e) => {
            tracing::error!(
                backend = backend,
                latency_ms = latency_ms,
                error = %e,
                "llm_stream_failed"
            );
        }
    }

    result
}

/// Call the LLM API with conversation history (blocking).
/// The `messages` slice contains prior user/assistant turns. The current
/// `user_prompt` is appended as the final user message.
/// v1.24.0 — DEPRECATED multi-turn synchronous call.
/// New code MUST use [`crate::backends::Backend::complete`] with
/// a populated `ChatRequest::messages` history.
#[deprecated(
    since = "1.25.0",
    note = "use crate::backends::Registry::get(name)?.complete() with \
            ChatRequest::messages history; 33.x.i.2 closes the migration"
)]
pub fn call_multi(
    backend: &str,
    api_key: &str,
    system_prompt: &str,
    messages: &[crate::conversation::Message],
    user_prompt: &str,
    max_tokens: Option<u32>,
) -> Result<ModelResponse, BackendError> {
    let spec = provider_spec(backend).ok_or_else(|| BackendError {
        message: format!(
            "Unknown backend '{backend}'. Supported: {}",
            SUPPORTED_BACKENDS.join(", ")
        ),
    })?;

    let msgs_json = build_messages_json(&spec, messages, user_prompt);

    let start = std::time::Instant::now();
    let turn_count = messages.len() + 1;
    tracing::info!(
        backend = backend,
        model = spec.default_model,
        turns = turn_count,
        "llm_multi_call_started"
    );

    let result = match spec.api_family {
        ApiFamily::Anthropic => call_anthropic_multi(&spec, api_key, system_prompt, &msgs_json, max_tokens),
        ApiFamily::Gemini => call_gemini_multi(&spec, api_key, system_prompt, &msgs_json, max_tokens),
        ApiFamily::OpenAICompatible => call_openai_multi(&spec, api_key, system_prompt, &msgs_json, max_tokens),
    };

    let latency_ms = start.elapsed().as_millis() as u64;

    match &result {
        Ok(resp) => {
            tracing::info!(
                backend = backend,
                model = %resp.model,
                turns = turn_count,
                latency_ms = latency_ms,
                input_tokens = resp.input_tokens,
                output_tokens = resp.output_tokens,
                "llm_multi_call_completed"
            );
        }
        Err(e) => {
            tracing::error!(
                backend = backend,
                turns = turn_count,
                latency_ms = latency_ms,
                error = %e,
                "llm_multi_call_failed"
            );
        }
    }

    result
}

/// Call the LLM API with conversation history and streaming.
/// v1.24.0 — DEPRECATED multi-turn streaming call.
/// New code MUST use [`crate::backends::Backend::stream`] with a
/// populated `ChatRequest::messages` history.
#[deprecated(
    since = "1.25.0",
    note = "use crate::backends::Registry::get(name)?.stream() with \
            ChatRequest::messages history; 33.x.i.2 closes the migration"
)]
pub fn call_multi_stream<F>(
    backend: &str,
    api_key: &str,
    system_prompt: &str,
    messages: &[crate::conversation::Message],
    user_prompt: &str,
    max_tokens: Option<u32>,
    on_chunk: F,
) -> Result<ModelResponse, BackendError>
where
    F: FnMut(&str),
{
    let spec = provider_spec(backend).ok_or_else(|| BackendError {
        message: format!(
            "Unknown backend '{backend}'. Supported: {}",
            SUPPORTED_BACKENDS.join(", ")
        ),
    })?;

    let msgs_json = build_messages_json(&spec, messages, user_prompt);

    let start = std::time::Instant::now();
    let turn_count = messages.len() + 1;
    tracing::info!(
        backend = backend,
        model = spec.default_model,
        turns = turn_count,
        streaming = true,
        "llm_multi_stream_started"
    );

    let result = match spec.api_family {
        ApiFamily::Anthropic => stream_anthropic_multi(&spec, api_key, system_prompt, &msgs_json, max_tokens, on_chunk),
        ApiFamily::Gemini => stream_gemini_multi(&spec, api_key, system_prompt, &msgs_json, max_tokens, on_chunk),
        ApiFamily::OpenAICompatible => stream_openai_multi(&spec, api_key, system_prompt, &msgs_json, max_tokens, on_chunk),
    };

    let latency_ms = start.elapsed().as_millis() as u64;

    match &result {
        Ok(resp) => {
            tracing::info!(
                backend = backend,
                model = %resp.model,
                turns = turn_count,
                latency_ms = latency_ms,
                input_tokens = resp.input_tokens,
                output_tokens = resp.output_tokens,
                "llm_multi_stream_completed"
            );
        }
        Err(e) => {
            tracing::error!(
                backend = backend,
                turns = turn_count,
                latency_ms = latency_ms,
                error = %e,
                "llm_multi_stream_failed"
            );
        }
    }

    result
}

/// Build the messages JSON array from conversation history + current user prompt.
/// Format depends on API family:
///   - Anthropic/OpenAI: `[{role, content}, ...]`
///   - Gemini: `[{role, parts: [{text}]}, ...]` with "model" instead of "assistant"
fn build_messages_json(
    spec: &ProviderSpec,
    messages: &[crate::conversation::Message],
    user_prompt: &str,
) -> Vec<Value> {
    let mut msgs: Vec<Value> = Vec::with_capacity(messages.len() + 1);

    if spec.api_family == ApiFamily::Gemini {
        for m in messages {
            let role = if m.role == "assistant" { "model" } else { &m.role };
            msgs.push(json!({"role": role, "parts": [{"text": &m.content}]}));
        }
        msgs.push(json!({"role": "user", "parts": [{"text": user_prompt}]}));
    } else {
        for m in messages {
            msgs.push(json!({"role": &m.role, "content": &m.content}));
        }
        msgs.push(json!({"role": "user", "content": user_prompt}));
    }

    msgs
}

// ── SSE line parser ────────────────────────────────────────────────────────

/// Parse SSE event stream from a reader, extracting text chunks.
/// Calls `extract_text` on each JSON data line to get the text delta.
/// Returns (accumulated_text, model, input_tokens, output_tokens, stop_reason).
fn parse_sse_stream<R, F, E>(
    reader: R,
    mut on_chunk: F,
    extract_text: E,
) -> Result<(String, String, u64, u64, String), BackendError>
where
    R: std::io::Read,
    F: FnMut(&str),
    E: Fn(&Value) -> SseExtract,
{
    let buf = BufReader::new(reader);
    let mut full_text = String::new();
    let mut model = String::new();
    let mut input_tokens: u64 = 0;
    let mut output_tokens: u64 = 0;
    let mut stop_reason = "unknown".to_string();

    for line in buf.lines() {
        let line = line.map_err(|e| BackendError {
            message: format!("Stream read error: {e}"),
        })?;

        let line = line.trim_end();

        // SSE format: "data: {...}"
        if let Some(data) = line.strip_prefix("data: ") {
            if data == "[DONE]" {
                break;
            }
            if let Ok(json) = serde_json::from_str::<Value>(data) {
                match extract_text(&json) {
                    SseExtract::Text(text) => {
                        on_chunk(&text);
                        full_text.push_str(&text);
                    }
                    SseExtract::Meta { m, it, ot, sr } => {
                        if !m.is_empty() { model = m; }
                        if it > 0 { input_tokens = it; }
                        if ot > 0 { output_tokens = ot; }
                        if !sr.is_empty() { stop_reason = sr; }
                    }
                    SseExtract::None => {}
                }
            }
        }
    }

    Ok((full_text, model, input_tokens, output_tokens, stop_reason))
}

/// What a single SSE data line yields.
enum SseExtract {
    /// A text chunk to append.
    Text(String),
    /// Metadata update (model, input_tokens, output_tokens, stop_reason).
    Meta { m: String, it: u64, ot: u64, sr: String },
    /// Nothing useful in this line.
    None,
}

// ── Streaming: Anthropic ───────────────────────────────────────────────────

fn stream_anthropic<F>(
    spec: &ProviderSpec,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    max_tokens: Option<u32>,
    on_chunk: F,
) -> Result<ModelResponse, BackendError>
where
    F: FnMut(&str),
{
    let url = format!("{}/v1/messages", spec.base_url);
    let body = json!({
        "model": spec.default_model,
        "max_tokens": max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "stream": true,
        "system": system_prompt,
        "messages": [{"role": "user", "content": user_prompt}]
    });

    let response = http_post_stream(
        &url,
        &[
            ("x-api-key", api_key),
            ("anthropic-version", "2023-06-01"),
            ("content-type", "application/json"),
        ],
        &body,
    )?;

    let (text, model, input_tokens, output_tokens, stop_reason) =
        parse_sse_stream(response, on_chunk, |json| {
            let event_type = json["type"].as_str().unwrap_or("");
            match event_type {
                "content_block_delta" => {
                    if let Some(text) = json["delta"]["text"].as_str() {
                        SseExtract::Text(text.to_string())
                    } else {
                        SseExtract::None
                    }
                }
                "message_start" => {
                    let m = json["message"]["model"].as_str().unwrap_or("").to_string();
                    let it = json["message"]["usage"]["input_tokens"].as_u64().unwrap_or(0);
                    SseExtract::Meta { m, it, ot: 0, sr: String::new() }
                }
                "message_delta" => {
                    let ot = json["usage"]["output_tokens"].as_u64().unwrap_or(0);
                    let sr = json["delta"]["stop_reason"].as_str().unwrap_or("").to_string();
                    SseExtract::Meta { m: String::new(), it: 0, ot, sr }
                }
                _ => SseExtract::None,
            }
        })?;

    let model = if model.is_empty() { spec.default_model.to_string() } else { model };

    Ok(ModelResponse { text, model, input_tokens, output_tokens, stop_reason })
}

// ── Streaming: Gemini ──────────────────────────────────────────────────────

fn stream_gemini<F>(
    spec: &ProviderSpec,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    _max_tokens: Option<u32>,
    mut on_chunk: F,
) -> Result<ModelResponse, BackendError>
where
    F: FnMut(&str),
{
    // Gemini streaming uses streamGenerateContent endpoint
    let url = format!(
        "{}/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
        spec.base_url, spec.default_model, api_key
    );
    let body = json!({
        "systemInstruction": {"parts": [{"text": system_prompt}]},
        "contents": [{"parts": [{"text": user_prompt}]}]
    });

    let response = http_post_stream(
        &url,
        &[("content-type", "application/json")],
        &body,
    )?;

    let (text, _, input_tokens, output_tokens, stop_reason) =
        parse_sse_stream(response, &mut on_chunk, |json| {
            // Gemini SSE: each event is a generateContent response chunk
            if let Some(text) = json["candidates"]
                .as_array()
                .and_then(|c| c.first())
                .and_then(|c| c["content"]["parts"].as_array())
                .and_then(|p| p.first())
                .and_then(|p| p["text"].as_str())
            {
                return SseExtract::Text(text.to_string());
            }

            // Check for usage metadata in final chunk
            let it = json["usageMetadata"]["promptTokenCount"].as_u64().unwrap_or(0);
            let ot = json["usageMetadata"]["candidatesTokenCount"].as_u64().unwrap_or(0);
            let sr = json["candidates"]
                .as_array()
                .and_then(|c| c.first())
                .and_then(|c| c["finishReason"].as_str())
                .unwrap_or("")
                .to_string();

            if it > 0 || ot > 0 || !sr.is_empty() {
                SseExtract::Meta { m: String::new(), it, ot, sr }
            } else {
                SseExtract::None
            }
        })?;

    Ok(ModelResponse {
        text,
        model: spec.default_model.to_string(),
        input_tokens,
        output_tokens,
        stop_reason: if stop_reason == "unknown" { "STOP".to_string() } else { stop_reason },
    })
}

// ── Streaming: OpenAI-compatible ───────────────────────────────────────────

fn stream_openai_compat<F>(
    spec: &ProviderSpec,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    max_tokens: Option<u32>,
    on_chunk: F,
) -> Result<ModelResponse, BackendError>
where
    F: FnMut(&str),
{
    let url = format!("{}/v1/chat/completions", spec.base_url);
    let body = json!({
        "model": spec.default_model,
        "max_tokens": max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "temperature": 0,
        "stream": true,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt}
        ]
    });

    let mut headers: Vec<(&str, &str)> = vec![("content-type", "application/json")];
    let auth_header;
    if !api_key.is_empty() {
        auth_header = format!("Bearer {api_key}");
        headers.push(("authorization", &auth_header));
    }

    let response = http_post_stream(&url, &headers, &body)?;

    let (text, model, input_tokens, output_tokens, stop_reason) =
        parse_sse_stream(response, on_chunk, |json| {
            // OpenAI streaming: choices[0].delta.content
            if let Some(text) = json["choices"]
                .as_array()
                .and_then(|c| c.first())
                .and_then(|c| c["delta"]["content"].as_str())
            {
                if !text.is_empty() {
                    return SseExtract::Text(text.to_string());
                }
            }

            // Model name from first chunk
            let m = json["model"].as_str().unwrap_or("").to_string();

            // Usage in final chunk (OpenAI includes it with stream_options)
            let it = json["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
            let ot = json["usage"]["completion_tokens"].as_u64().unwrap_or(0);

            // Stop reason
            let sr = json["choices"]
                .as_array()
                .and_then(|c| c.first())
                .and_then(|c| c["finish_reason"].as_str())
                .unwrap_or("")
                .to_string();

            if !m.is_empty() || it > 0 || ot > 0 || !sr.is_empty() {
                SseExtract::Meta { m, it, ot, sr }
            } else {
                SseExtract::None
            }
        })?;

    let model = if model.is_empty() { spec.default_model.to_string() } else { model };

    Ok(ModelResponse { text, model, input_tokens, output_tokens, stop_reason })
}

// ── Anthropic Messages API ──────────────────────────────────────────────────

fn call_anthropic(
    spec: &ProviderSpec,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    max_tokens: Option<u32>,
) -> Result<ModelResponse, BackendError> {
    let url = format!("{}/v1/messages", spec.base_url);
    let body = json!({
        "model": spec.default_model,
        "max_tokens": max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "system": system_prompt,
        "messages": [{"role": "user", "content": user_prompt}]
    });

    let response = http_post(
        &url,
        &[
            ("x-api-key", api_key),
            ("anthropic-version", "2023-06-01"),
            ("content-type", "application/json"),
        ],
        &body,
    )?;

    let text = response["content"]
        .as_array()
        .and_then(|blocks| {
            blocks.iter()
                .filter_map(|b| {
                    if b["type"].as_str() == Some("text") {
                        b["text"].as_str().map(|s| s.to_string())
                    } else { None }
                })
                .reduce(|a, b| format!("{a}\n{b}"))
        })
        .unwrap_or_default();

    Ok(ModelResponse {
        text,
        model: response["model"].as_str().unwrap_or(spec.default_model).to_string(),
        input_tokens: response["usage"]["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: response["usage"]["output_tokens"].as_u64().unwrap_or(0),
        stop_reason: response["stop_reason"].as_str().unwrap_or("unknown").to_string(),
    })
}

// ── Gemini generateContent API ──────────────────────────────────────────────

fn call_gemini(
    spec: &ProviderSpec,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    _max_tokens: Option<u32>,
) -> Result<ModelResponse, BackendError> {
    let url = format!(
        "{}/v1beta/models/{}:generateContent?key={}",
        spec.base_url, spec.default_model, api_key
    );
    let body = json!({
        "systemInstruction": {"parts": [{"text": system_prompt}]},
        "contents": [{"parts": [{"text": user_prompt}]}]
    });

    let response = http_post(
        &url,
        &[("content-type", "application/json")],
        &body,
    )?;

    // Extract text from candidates[0].content.parts[0].text
    let text = response["candidates"]
        .as_array()
        .and_then(|cands| cands.first())
        .and_then(|c| c["content"]["parts"].as_array())
        .and_then(|parts| parts.first())
        .and_then(|p| p["text"].as_str())
        .unwrap_or_default()
        .to_string();

    // Gemini usage is in usageMetadata
    let input_tokens = response["usageMetadata"]["promptTokenCount"].as_u64().unwrap_or(0);
    let output_tokens = response["usageMetadata"]["candidatesTokenCount"].as_u64().unwrap_or(0);
    let stop_reason = response["candidates"]
        .as_array()
        .and_then(|c| c.first())
        .and_then(|c| c["finishReason"].as_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(ModelResponse {
        text,
        model: spec.default_model.to_string(),
        input_tokens,
        output_tokens,
        stop_reason,
    })
}

// ── OpenAI-compatible chat/completions API ──────────────────────────────────
// Covers: OpenAI, Kimi, GLM, OpenRouter, Ollama

fn call_openai_compat(
    spec: &ProviderSpec,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    max_tokens: Option<u32>,
) -> Result<ModelResponse, BackendError> {
    let url = format!("{}/v1/chat/completions", spec.base_url);
    let body = json!({
        "model": spec.default_model,
        "max_tokens": max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "temperature": 0,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt}
        ]
    });

    let mut headers: Vec<(&str, &str)> = vec![("content-type", "application/json")];
    let auth_header;
    if !api_key.is_empty() {
        auth_header = format!("Bearer {api_key}");
        headers.push(("authorization", &auth_header));
    }

    let response = http_post(&url, &headers, &body)?;

    // Extract text from choices[0].message.content
    let text = response["choices"]
        .as_array()
        .and_then(|choices| choices.first())
        .and_then(|c| c["message"]["content"].as_str())
        .unwrap_or_default()
        .to_string();

    let model = response["model"].as_str().unwrap_or(spec.default_model).to_string();
    let input_tokens = response["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
    let output_tokens = response["usage"]["completion_tokens"].as_u64().unwrap_or(0);
    let stop_reason = response["choices"]
        .as_array()
        .and_then(|c| c.first())
        .and_then(|c| c["finish_reason"].as_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(ModelResponse {
        text,
        model,
        input_tokens,
        output_tokens,
        stop_reason,
    })
}

// ── HTTP helper ─────────────────────────────────────────────────────────────

/// HTTP POST returning the raw response for streaming reads.
fn http_post_stream(
    url: &str,
    headers: &[(&str, &str)],
    body: &Value,
) -> Result<reqwest::blocking::Response, BackendError> {
    let client = reqwest::blocking::Client::new();
    let mut request = client.post(url);

    for (key, val) in headers {
        request = request.header(*key, *val);
    }

    tracing::debug!(url = url, "http_post_stream_sending");

    let response = request
        .json(body)
        .send()
        .map_err(|e| {
            tracing::error!(url = url, error = %e, "http_post_stream_network_error");
            BackendError {
                message: format!("HTTP request failed: {e}"),
            }
        })?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().unwrap_or_default();
        tracing::error!(url = url, status = status.as_u16(), "http_post_stream_api_error");
        return Err(BackendError {
            message: format!("API error ({status}): {error_text}"),
        });
    }

    Ok(response)
}

// ── Multi-turn: Anthropic ──────────────────────────────────────────────────

fn call_anthropic_multi(
    spec: &ProviderSpec,
    api_key: &str,
    system_prompt: &str,
    messages: &[Value],
    max_tokens: Option<u32>,
) -> Result<ModelResponse, BackendError> {
    let url = format!("{}/v1/messages", spec.base_url);
    let body = json!({
        "model": spec.default_model,
        "max_tokens": max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "system": system_prompt,
        "messages": messages
    });

    let response = http_post(
        &url,
        &[
            ("x-api-key", api_key),
            ("anthropic-version", "2023-06-01"),
            ("content-type", "application/json"),
        ],
        &body,
    )?;

    let text = response["content"]
        .as_array()
        .and_then(|blocks| {
            blocks.iter()
                .filter_map(|b| {
                    if b["type"].as_str() == Some("text") {
                        b["text"].as_str().map(|s| s.to_string())
                    } else { None }
                })
                .reduce(|a, b| format!("{a}\n{b}"))
        })
        .unwrap_or_default();

    Ok(ModelResponse {
        text,
        model: response["model"].as_str().unwrap_or(spec.default_model).to_string(),
        input_tokens: response["usage"]["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: response["usage"]["output_tokens"].as_u64().unwrap_or(0),
        stop_reason: response["stop_reason"].as_str().unwrap_or("unknown").to_string(),
    })
}

fn stream_anthropic_multi<F>(
    spec: &ProviderSpec,
    api_key: &str,
    system_prompt: &str,
    messages: &[Value],
    max_tokens: Option<u32>,
    on_chunk: F,
) -> Result<ModelResponse, BackendError>
where
    F: FnMut(&str),
{
    let url = format!("{}/v1/messages", spec.base_url);
    let body = json!({
        "model": spec.default_model,
        "max_tokens": max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "stream": true,
        "system": system_prompt,
        "messages": messages
    });

    let response = http_post_stream(
        &url,
        &[
            ("x-api-key", api_key),
            ("anthropic-version", "2023-06-01"),
            ("content-type", "application/json"),
        ],
        &body,
    )?;

    let (text, model, input_tokens, output_tokens, stop_reason) =
        parse_sse_stream(response, on_chunk, |json| {
            let event_type = json["type"].as_str().unwrap_or("");
            match event_type {
                "content_block_delta" => {
                    if let Some(text) = json["delta"]["text"].as_str() {
                        SseExtract::Text(text.to_string())
                    } else {
                        SseExtract::None
                    }
                }
                "message_start" => {
                    let m = json["message"]["model"].as_str().unwrap_or("").to_string();
                    let it = json["message"]["usage"]["input_tokens"].as_u64().unwrap_or(0);
                    SseExtract::Meta { m, it, ot: 0, sr: String::new() }
                }
                "message_delta" => {
                    let ot = json["usage"]["output_tokens"].as_u64().unwrap_or(0);
                    let sr = json["delta"]["stop_reason"].as_str().unwrap_or("").to_string();
                    SseExtract::Meta { m: String::new(), it: 0, ot, sr }
                }
                _ => SseExtract::None,
            }
        })?;

    let model = if model.is_empty() { spec.default_model.to_string() } else { model };
    Ok(ModelResponse { text, model, input_tokens, output_tokens, stop_reason })
}

// ── Multi-turn: Gemini ────────────────────────────────────────────────────

fn call_gemini_multi(
    spec: &ProviderSpec,
    api_key: &str,
    system_prompt: &str,
    contents: &[Value],
    _max_tokens: Option<u32>,
) -> Result<ModelResponse, BackendError> {
    let url = format!(
        "{}/v1beta/models/{}:generateContent?key={}",
        spec.base_url, spec.default_model, api_key
    );
    let body = json!({
        "systemInstruction": {"parts": [{"text": system_prompt}]},
        "contents": contents
    });

    let response = http_post(
        &url,
        &[("content-type", "application/json")],
        &body,
    )?;

    let text = response["candidates"]
        .as_array()
        .and_then(|cands| cands.first())
        .and_then(|c| c["content"]["parts"].as_array())
        .and_then(|parts| parts.first())
        .and_then(|p| p["text"].as_str())
        .unwrap_or_default()
        .to_string();

    let input_tokens = response["usageMetadata"]["promptTokenCount"].as_u64().unwrap_or(0);
    let output_tokens = response["usageMetadata"]["candidatesTokenCount"].as_u64().unwrap_or(0);
    let stop_reason = response["candidates"]
        .as_array()
        .and_then(|c| c.first())
        .and_then(|c| c["finishReason"].as_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(ModelResponse {
        text,
        model: spec.default_model.to_string(),
        input_tokens,
        output_tokens,
        stop_reason,
    })
}

fn stream_gemini_multi<F>(
    spec: &ProviderSpec,
    api_key: &str,
    system_prompt: &str,
    contents: &[Value],
    _max_tokens: Option<u32>,
    mut on_chunk: F,
) -> Result<ModelResponse, BackendError>
where
    F: FnMut(&str),
{
    let url = format!(
        "{}/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
        spec.base_url, spec.default_model, api_key
    );
    let body = json!({
        "systemInstruction": {"parts": [{"text": system_prompt}]},
        "contents": contents
    });

    let response = http_post_stream(
        &url,
        &[("content-type", "application/json")],
        &body,
    )?;

    let (text, _model, input_tokens, output_tokens, stop_reason) =
        parse_sse_stream(response, &mut on_chunk, |json| {
            if let Some(text) = json["candidates"]
                .as_array()
                .and_then(|c| c.first())
                .and_then(|c| c["content"]["parts"].as_array())
                .and_then(|parts| parts.first())
                .and_then(|p| p["text"].as_str())
            {
                SseExtract::Text(text.to_string())
            } else if let Some(usage) = json.get("usageMetadata") {
                SseExtract::Meta {
                    m: String::new(),
                    it: usage["promptTokenCount"].as_u64().unwrap_or(0),
                    ot: usage["candidatesTokenCount"].as_u64().unwrap_or(0),
                    sr: json["candidates"]
                        .as_array()
                        .and_then(|c| c.first())
                        .and_then(|c| c["finishReason"].as_str())
                        .unwrap_or("")
                        .to_string(),
                }
            } else {
                SseExtract::None
            }
        })?;

    Ok(ModelResponse {
        text,
        model: spec.default_model.to_string(),
        input_tokens,
        output_tokens,
        stop_reason,
    })
}

// ── Multi-turn: OpenAI-compatible ─────────────────────────────────────────

fn call_openai_multi(
    spec: &ProviderSpec,
    api_key: &str,
    system_prompt: &str,
    messages: &[Value],
    max_tokens: Option<u32>,
) -> Result<ModelResponse, BackendError> {
    let url = format!("{}/v1/chat/completions", spec.base_url);

    // Prepend system message to the conversation messages
    let mut all_msgs = vec![json!({"role": "system", "content": system_prompt})];
    all_msgs.extend_from_slice(messages);

    let body = json!({
        "model": spec.default_model,
        "max_tokens": max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "temperature": 0,
        "messages": all_msgs
    });

    let response = http_post(
        &url,
        &[
            ("Authorization", &format!("Bearer {api_key}")),
            ("content-type", "application/json"),
        ],
        &body,
    )?;

    let text = response["choices"]
        .as_array()
        .and_then(|c| c.first())
        .and_then(|c| c["message"]["content"].as_str())
        .unwrap_or_default()
        .to_string();

    let model = response["model"].as_str().unwrap_or(spec.default_model).to_string();
    let input_tokens = response["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
    let output_tokens = response["usage"]["completion_tokens"].as_u64().unwrap_or(0);
    let stop_reason = response["choices"]
        .as_array()
        .and_then(|c| c.first())
        .and_then(|c| c["finish_reason"].as_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(ModelResponse { text, model, input_tokens, output_tokens, stop_reason })
}

fn stream_openai_multi<F>(
    spec: &ProviderSpec,
    api_key: &str,
    system_prompt: &str,
    messages: &[Value],
    max_tokens: Option<u32>,
    on_chunk: F,
) -> Result<ModelResponse, BackendError>
where
    F: FnMut(&str),
{
    let url = format!("{}/v1/chat/completions", spec.base_url);

    let mut all_msgs = vec![json!({"role": "system", "content": system_prompt})];
    all_msgs.extend_from_slice(messages);

    let body = json!({
        "model": spec.default_model,
        "max_tokens": max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "temperature": 0,
        "stream": true,
        "messages": all_msgs
    });

    let response = http_post_stream(
        &url,
        &[
            ("Authorization", &format!("Bearer {api_key}")),
            ("content-type", "application/json"),
        ],
        &body,
    )?;

    let (text, model, input_tokens, output_tokens, stop_reason) =
        parse_sse_stream(response, on_chunk, |json| {
            if let Some(delta) = json["choices"]
                .as_array()
                .and_then(|c| c.first())
                .and_then(|c| c["delta"]["content"].as_str())
            {
                SseExtract::Text(delta.to_string())
            } else {
                let m = json["model"].as_str().unwrap_or("").to_string();
                let sr = json["choices"]
                    .as_array()
                    .and_then(|c| c.first())
                    .and_then(|c| c["finish_reason"].as_str())
                    .unwrap_or("")
                    .to_string();
                if !m.is_empty() || !sr.is_empty() {
                    SseExtract::Meta { m, it: 0, ot: 0, sr }
                } else {
                    SseExtract::None
                }
            }
        })?;

    let model = if model.is_empty() { spec.default_model.to_string() } else { model };
    Ok(ModelResponse { text, model, input_tokens, output_tokens, stop_reason })
}

fn http_post(url: &str, headers: &[(&str, &str)], body: &Value) -> Result<Value, BackendError> {
    let client = reqwest::blocking::Client::new();
    let mut request = client.post(url);

    for (key, val) in headers {
        request = request.header(*key, *val);
    }

    tracing::debug!(url = url, "http_post_sending");

    let response = request
        .json(body)
        .send()
        .map_err(|e| {
            tracing::error!(url = url, error = %e, "http_post_network_error");
            BackendError {
                message: format!("HTTP request failed: {e}"),
            }
        })?;

    let status = response.status();
    let response_text = response.text().map_err(|e| BackendError {
        message: format!("Failed to read response: {e}"),
    })?;

    if !status.is_success() {
        tracing::error!(url = url, status = status.as_u16(), "http_post_api_error");
        return Err(BackendError {
            message: format!("API error ({status}): {response_text}"),
        });
    }

    serde_json::from_str(&response_text).map_err(|e| BackendError {
        message: format!("Failed to parse response JSON: {e}"),
    })
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_parse_anthropic_stream() {
        // Simulate Anthropic SSE stream
        let stream = b"\
data: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-sonnet-4-20250514\",\"usage\":{\"input_tokens\":42}}}\n\
\n\
data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"Hello\"}}\n\
\n\
data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\" world\"}}\n\
\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":10}}\n\
\n\
";
        let reader = std::io::Cursor::new(stream);
        let mut chunks: Vec<String> = Vec::new();

        let (text, model, it, ot, sr) = parse_sse_stream(
            reader,
            |chunk| chunks.push(chunk.to_string()),
            |json| {
                let event_type = json["type"].as_str().unwrap_or("");
                match event_type {
                    "content_block_delta" => {
                        if let Some(t) = json["delta"]["text"].as_str() {
                            SseExtract::Text(t.to_string())
                        } else { SseExtract::None }
                    }
                    "message_start" => {
                        let m = json["message"]["model"].as_str().unwrap_or("").to_string();
                        let it = json["message"]["usage"]["input_tokens"].as_u64().unwrap_or(0);
                        SseExtract::Meta { m, it, ot: 0, sr: String::new() }
                    }
                    "message_delta" => {
                        let ot = json["usage"]["output_tokens"].as_u64().unwrap_or(0);
                        let sr = json["delta"]["stop_reason"].as_str().unwrap_or("").to_string();
                        SseExtract::Meta { m: String::new(), it: 0, ot, sr }
                    }
                    _ => SseExtract::None,
                }
            },
        ).unwrap();

        assert_eq!(text, "Hello world");
        assert_eq!(chunks, vec!["Hello", " world"]);
        assert_eq!(model, "claude-sonnet-4-20250514");
        assert_eq!(it, 42);
        assert_eq!(ot, 10);
        assert_eq!(sr, "end_turn");
    }

    #[test]
    fn sse_parse_openai_stream() {
        // Simulate OpenAI SSE stream (realistic: first chunk has model + role, then content chunks)
        let stream = b"\
data: {\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"content\":\" there\"}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\
\n\
data: [DONE]\n\
";
        let reader = std::io::Cursor::new(stream);
        let mut chunks: Vec<String> = Vec::new();

        let (text, model, it, ot, sr) = parse_sse_stream(
            reader,
            |chunk| chunks.push(chunk.to_string()),
            |json| {
                if let Some(t) = json["choices"].as_array()
                    .and_then(|c| c.first())
                    .and_then(|c| c["delta"]["content"].as_str())
                {
                    if !t.is_empty() { return SseExtract::Text(t.to_string()); }
                }
                let m = json["model"].as_str().unwrap_or("").to_string();
                let it = json["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
                let ot = json["usage"]["completion_tokens"].as_u64().unwrap_or(0);
                let sr = json["choices"].as_array()
                    .and_then(|c| c.first())
                    .and_then(|c| c["finish_reason"].as_str())
                    .unwrap_or("").to_string();
                if !m.is_empty() || it > 0 || ot > 0 || !sr.is_empty() {
                    SseExtract::Meta { m, it, ot, sr }
                } else { SseExtract::None }
            },
        ).unwrap();

        assert_eq!(text, "Hi there");
        assert_eq!(chunks, vec!["Hi", " there"]);
        assert_eq!(model, "gpt-4o-mini");
        assert_eq!(it, 5);
        assert_eq!(ot, 2);
        assert_eq!(sr, "stop");
    }

    #[test]
    fn sse_parse_empty_stream() {
        let stream = b"data: [DONE]\n";
        let reader = std::io::Cursor::new(stream);
        let mut chunk_count = 0;

        let (text, _, _, _, _) = parse_sse_stream(
            reader,
            |_| chunk_count += 1,
            |_| SseExtract::None,
        ).unwrap();

        assert_eq!(text, "");
        assert_eq!(chunk_count, 0);
    }

    #[test]
    fn sse_parse_ignores_non_data_lines() {
        let stream = b"\
: comment line\n\
event: ping\n\
data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"ok\"}}\n\
\n\
retry: 1000\n\
";
        let reader = std::io::Cursor::new(stream);
        let mut chunks: Vec<String> = Vec::new();

        let (text, _, _, _, _) = parse_sse_stream(
            reader,
            |chunk| chunks.push(chunk.to_string()),
            |json| {
                if let Some(t) = json["delta"]["text"].as_str() {
                    SseExtract::Text(t.to_string())
                } else { SseExtract::None }
            },
        ).unwrap();

        assert_eq!(text, "ok");
        assert_eq!(chunks, vec!["ok"]);
    }
}
