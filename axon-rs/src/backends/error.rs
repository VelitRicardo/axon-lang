//! Typed transport errors for native Rust LLM backends — v1.18.0.
//!
//! Mirror of the v1.16.1 named subclasses on the Python side
//! (`axon.runtime.runtime_errors`):
//!
//!   * [`BackendError::RateLimit`]    — HTTP 429, retries exhausted.
//!   * [`BackendError::Auth`]         — HTTP 401 / 403, fail-fast.
//!   * [`BackendError::ContextLength`]— HTTP 400 with a context-overrun shape.
//!   * [`BackendError::SafetyBreach`] — provider's content filter fired.
//!   * [`BackendError::ModelNotFound`]— HTTP 404 / 400 with model-not-found shape.
//!   * [`BackendError::Generic`]      — unmapped 4xx / 5xx / transport.
//!
//! Adopters can `match` on these variants without parsing message strings.
//! For retry-policy decisions the legacy
//! [`crate::backend_error::BackendErrorKind`] taxonomy is still consulted —
//! the new typed variants carry a [`Self::kind`] accessor that translates,
//! so existing `resilient_backend` / `circuit_breaker` infra keeps working.

use std::fmt;

use crate::backend_error::BackendErrorKind;

/// Typed transport error from a native Rust backend.
///
/// Each variant carries enough context to render a useful message for the
/// adopter without sacrificing programmatic dispatch — the message body
/// repeats the provider name + model name + status code so log-only
/// consumers don't lose information.
#[derive(Debug, Clone)]
pub enum BackendError {
    /// Provider rate limit hit (HTTP 429); retries exhausted or unavailable.
    RateLimit {
        provider: String,
        model: String,
        retry_after_seconds: Option<u64>,
        body_preview: String,
    },
    /// Provider rejected the request as unauthenticated (HTTP 401 / 403).
    Auth {
        provider: String,
        model: String,
        api_key_env: Option<String>,
        status: u16,
        body_preview: String,
    },
    /// Compiled prompt exceeds the model's context window (HTTP 400 with a
    /// `context_length_exceeded` / `maximum context` / `too long` shape).
    ContextLength {
        provider: String,
        model: String,
        body_preview: String,
    },
    /// Provider's content filter blocked the request or response.
    SafetyBreach {
        provider: String,
        model: String,
        finish_reason: String,
        body_preview: String,
    },
    /// Provider does not recognise the requested model identifier.
    ModelNotFound {
        provider: String,
        model: String,
        status: u16,
        body_preview: String,
    },
    /// Unmapped HTTP error or transport-layer failure.
    Generic {
        provider: String,
        model: String,
        status: Option<u16>,
        message: String,
    },
}

impl BackendError {
    /// Provider name reported by the error (always set).
    pub fn provider(&self) -> &str {
        match self {
            Self::RateLimit { provider, .. }
            | Self::Auth { provider, .. }
            | Self::ContextLength { provider, .. }
            | Self::SafetyBreach { provider, .. }
            | Self::ModelNotFound { provider, .. }
            | Self::Generic { provider, .. } => provider,
        }
    }

    /// Resolved model name reported by the error (always set; may be the
    /// provider's default if the request omitted one).
    pub fn model(&self) -> &str {
        match self {
            Self::RateLimit { model, .. }
            | Self::Auth { model, .. }
            | Self::ContextLength { model, .. }
            | Self::SafetyBreach { model, .. }
            | Self::ModelNotFound { model, .. }
            | Self::Generic { model, .. } => model,
        }
    }

    /// Translate into the legacy [`BackendErrorKind`] taxonomy so that
    /// existing infra in `resilient_backend.rs` / `circuit_breaker.rs` /
    /// `retry_policy.rs` continues to drive retry / CB decisions without
    /// changes during the v1.18.0 transition (D6 — dual presence).
    pub fn kind(&self) -> BackendErrorKind {
        match self {
            Self::RateLimit { retry_after_seconds, .. } => BackendErrorKind::RateLimit {
                retry_after: retry_after_seconds.map(std::time::Duration::from_secs),
            },
            Self::Auth { .. } => BackendErrorKind::AuthError,
            Self::ContextLength { .. } => BackendErrorKind::Unknown, // 400 — fail-fast, not retryable
            Self::SafetyBreach { .. } => BackendErrorKind::Unknown, // not retryable
            Self::ModelNotFound { .. } => BackendErrorKind::Unknown, // 404 — fail-fast
            Self::Generic { status, .. } => match status {
                Some(s) if (500..600).contains(s) => BackendErrorKind::ServerError { status: *s },
                Some(429) => BackendErrorKind::RateLimit { retry_after: None },
                Some(401) | Some(403) => BackendErrorKind::AuthError,
                Some(408) => BackendErrorKind::Timeout,
                Some(_) => BackendErrorKind::Unknown,
                None => BackendErrorKind::NetworkError,
            },
        }
    }

    /// Whether the transport layer should retry this error before giving
    /// up. Consults [`Self::kind`] + the legacy `is_retryable` predicate
    /// so the policy stays in lockstep with `resilient_backend`.
    pub fn is_retryable(&self) -> bool {
        self.kind().is_retryable()
    }

    /// Stable category label for logs / metrics. Preserved across variants
    /// so that operators can filter by error class without enum-matching.
    pub fn category(&self) -> &'static str {
        match self {
            Self::RateLimit { .. } => "rate_limit",
            Self::Auth { .. } => "auth_error",
            Self::ContextLength { .. } => "context_length_exceeded",
            Self::SafetyBreach { .. } => "safety_breach",
            Self::ModelNotFound { .. } => "model_not_found",
            Self::Generic { .. } => "model_call_error",
        }
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RateLimit {
                provider,
                model,
                retry_after_seconds,
                body_preview,
            } => {
                let retry_after_part = retry_after_seconds
                    .map(|s| format!(", retry_after={}s", s))
                    .unwrap_or_default();
                write!(
                    f,
                    "Rate limit on provider {provider:?} (model={model:?}, status=429{retry_after_part}). \
                     Retries exhausted. Body: {body_preview}"
                )
            }
            Self::Auth {
                provider,
                model,
                api_key_env,
                status,
                body_preview,
            } => {
                let env_hint = api_key_env
                    .as_ref()
                    .map(|env| format!(" (env var: {env})"))
                    .unwrap_or_default();
                write!(
                    f,
                    "Authentication failed on provider {provider:?}{env_hint}, \
                     status={status}. Verify the API key is set, valid, and has \
                     access to model {model:?}. Body: {body_preview}"
                )
            }
            Self::ContextLength {
                provider,
                model,
                body_preview,
            } => write!(
                f,
                "Prompt exceeds context window of model {model:?} on provider \
                 {provider:?} (status=400). Body: {body_preview}"
            ),
            Self::SafetyBreach {
                provider,
                model,
                finish_reason,
                body_preview,
            } => write!(
                f,
                "Provider {provider:?} content filter blocked the request \
                 (model={model:?}, finish_reason={finish_reason:?}). Body: {body_preview}"
            ),
            Self::ModelNotFound {
                provider,
                model,
                status,
                body_preview,
            } => write!(
                f,
                "Model {model:?} not found at provider {provider:?} (status={status}). \
                 Either the slug is mistyped or the model was deprecated. Body: {body_preview}"
            ),
            Self::Generic {
                provider,
                model,
                status,
                message,
            } => {
                let status_part = status
                    .map(|s| format!("HTTP {s}"))
                    .unwrap_or_else(|| "transport error".to_string());
                write!(
                    f,
                    "Provider {provider:?} returned {status_part} for model \
                     {model:?}. {message}"
                )
            }
        }
    }
}

impl std::error::Error for BackendError {}

/// Helper: classify an HTTP status + response body into the right typed
/// variant. Mirrors `_categorise_http_error` from `axon.server.model_clients`
/// (Python v1.16.1).
pub fn categorise_http(
    provider: &str,
    model: &str,
    status: u16,
    headers: &reqwest::header::HeaderMap,
    body: &str,
    api_key_env: Option<&str>,
) -> BackendError {
    let body_preview: String = body.chars().take(200).collect();
    let body_lower = body.to_lowercase();

    if status == 429 {
        let retry_after = headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok());
        return BackendError::RateLimit {
            provider: provider.to_string(),
            model: model.to_string(),
            retry_after_seconds: retry_after,
            body_preview,
        };
    }

    if status == 401 || status == 403 {
        return BackendError::Auth {
            provider: provider.to_string(),
            model: model.to_string(),
            api_key_env: api_key_env.map(str::to_string),
            status,
            body_preview,
        };
    }

    if status == 404 {
        return BackendError::ModelNotFound {
            provider: provider.to_string(),
            model: model.to_string(),
            status,
            body_preview,
        };
    }

    if status == 400 {
        // Context-overrun shape: providers vary in wording. Substring match
        // mirrors the Python implementation in v1.16.1.
        if body_lower.contains("context_length")
            || body_lower.contains("context length")
            || body_lower.contains("maximum context")
            || body_lower.contains("too long")
        {
            return BackendError::ContextLength {
                provider: provider.to_string(),
                model: model.to_string(),
                body_preview,
            };
        }
        if body_lower.contains("model_not_found")
            || body_lower.contains("model not found")
            || body_lower.contains("no such model")
        {
            return BackendError::ModelNotFound {
                provider: provider.to_string(),
                model: model.to_string(),
                status,
                body_preview,
            };
        }
    }

    BackendError::Generic {
        provider: provider.to_string(),
        model: model.to_string(),
        status: Some(status),
        message: format!("Body: {body_preview}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderMap;

    fn empty_headers() -> HeaderMap {
        HeaderMap::new()
    }

    #[test]
    fn ratelimit_carries_retry_after() {
        let mut h = HeaderMap::new();
        h.insert("retry-after", "60".parse().unwrap());
        let err = categorise_http("anthropic", "claude-x", 429, &h, "rate-limited", None);
        assert!(matches!(err, BackendError::RateLimit { retry_after_seconds: Some(60), .. }));
        assert_eq!(err.category(), "rate_limit");
        assert!(err.is_retryable());
    }

    #[test]
    fn ratelimit_without_header_is_still_classified() {
        let err = categorise_http("openai", "gpt-x", 429, &empty_headers(), "no body", None);
        match err {
            BackendError::RateLimit { retry_after_seconds, .. } => {
                assert!(retry_after_seconds.is_none());
            }
            _ => panic!("expected RateLimit"),
        }
    }

    #[test]
    fn auth_401_with_env_hint() {
        let err = categorise_http(
            "kimi",
            "kimi-k2.6",
            401,
            &empty_headers(),
            "unauthorized",
            Some("AXON_KIMI_API_KEY"),
        );
        match err {
            BackendError::Auth { api_key_env, status, .. } => {
                assert_eq!(api_key_env.as_deref(), Some("AXON_KIMI_API_KEY"));
                assert_eq!(status, 401);
            }
            _ => panic!("expected Auth"),
        }
    }

    #[test]
    fn auth_403_also_classified_as_auth() {
        let err = categorise_http("openai", "gpt-x", 403, &empty_headers(), "", None);
        assert!(matches!(err, BackendError::Auth { status: 403, .. }));
    }

    #[test]
    fn model_not_found_404() {
        let err = categorise_http("openai", "gpt-3.999", 404, &empty_headers(), "", None);
        assert!(matches!(err, BackendError::ModelNotFound { .. }));
        assert!(!err.is_retryable()); // fail-fast
    }

    #[test]
    fn context_length_400_with_oai_marker() {
        let body = r#"{"error":{"code":"context_length_exceeded","message":"prompt too long"}}"#;
        let err = categorise_http("openai", "gpt-x", 400, &empty_headers(), body, None);
        assert!(matches!(err, BackendError::ContextLength { .. }));
    }

    #[test]
    fn context_length_400_with_anthropic_marker() {
        let body = "the prompt is too long for this model's maximum context";
        let err = categorise_http("anthropic", "claude-x", 400, &empty_headers(), body, None);
        assert!(matches!(err, BackendError::ContextLength { .. }));
    }

    #[test]
    fn model_not_found_400_with_marker() {
        let body = r#"{"error":{"code":"model_not_found"}}"#;
        let err = categorise_http("openai", "gpt-y", 400, &empty_headers(), body, None);
        assert!(matches!(err, BackendError::ModelNotFound { status: 400, .. }));
    }

    #[test]
    fn generic_500_is_retryable() {
        let err = categorise_http("openai", "gpt-x", 500, &empty_headers(), "boom", None);
        assert!(matches!(err, BackendError::Generic { status: Some(500), .. }));
        assert!(err.is_retryable());
    }

    #[test]
    fn generic_502_is_retryable() {
        let err = categorise_http("openai", "gpt-x", 502, &empty_headers(), "", None);
        assert!(err.is_retryable());
    }

    #[test]
    fn generic_400_unmapped_is_not_retryable() {
        let err = categorise_http("openai", "gpt-x", 400, &empty_headers(), "weird", None);
        assert!(matches!(err, BackendError::Generic { .. }));
        assert!(!err.is_retryable());
    }

    #[test]
    fn provider_and_model_accessors() {
        let err = categorise_http("kimi", "kimi-k2.6", 429, &empty_headers(), "", None);
        assert_eq!(err.provider(), "kimi");
        assert_eq!(err.model(), "kimi-k2.6");
    }

    #[test]
    fn body_preview_truncated_to_200_chars() {
        let body = "x".repeat(500);
        let err = categorise_http("openai", "gpt-x", 500, &empty_headers(), &body, None);
        match err {
            BackendError::Generic { message, .. } => {
                // message format: "Body: <preview>" where preview is 200 chars max.
                assert!(message.starts_with("Body: "));
                let preview = &message["Body: ".len()..];
                assert_eq!(preview.len(), 200);
            }
            _ => panic!("expected Generic"),
        }
    }

    #[test]
    fn display_includes_provider_and_status() {
        let err = categorise_http("anthropic", "claude-x", 429, &empty_headers(), "tx", None);
        let s = format!("{err}");
        assert!(s.contains("anthropic"));
        assert!(s.contains("claude-x"));
        assert!(s.contains("429"));
    }

    #[test]
    fn safety_breach_constructed_directly() {
        // SafetyBreach is not constructed by HTTP categorisation — it's
        // emitted by the per-provider response parsers when a finish
        // reason indicates filter blocking. Verify the variant is
        // well-formed.
        let err = BackendError::SafetyBreach {
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            finish_reason: "content_filter".to_string(),
            body_preview: "{}".to_string(),
        };
        assert_eq!(err.category(), "safety_breach");
        assert!(!err.is_retryable());
        let msg = format!("{err}");
        assert!(msg.contains("content_filter"));
    }
}
