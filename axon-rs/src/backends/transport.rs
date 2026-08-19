//! Shared HTTP transport with retry — v1.18.0.
//!
//! Single source of truth for the `complete()` HTTP loop every native
//! Rust backend uses. Mirrors `_call_with_retry` from the Python
//! `axon.server.model_clients` (v1.16.1) — same status policy
//! (429 + 408 + 5xx retryable, others fail-fast), same `Retry-After`
//! honouring (integer-seconds form), same exponential backoff with
//! jitter via [`BackendRetryPolicy`].
//!
//! Lifted to a dedicated module in 24.d so the Anthropic / OpenAI /
//! Gemini / OpenAI-compat impls all share a single retry loop instead
//! of duplicating it. The function is provider-agnostic — the only
//! provider-specific behaviour is the body / headers / URL the caller
//! has already built before invoking it.

use std::time::Instant;

use reqwest::header::HeaderMap;

use super::error::{categorise_http, BackendError};
use super::observability;
use super::retry::{parse_retry_after, BackendRetryPolicy};

/// POST `body` to `url` with retry-on-retryable-status policy. Returns
/// the raw response body bytes + the retry count (0 = clean first
/// attempt) on success.
///
/// Mirrors the Python `_call_with_retry` from
/// `axon.server.model_clients` — identical status policy, identical
/// `Retry-After` honouring, identical backoff schedule. The single
/// source of truth lives here so per-provider backends only customise
/// what they actually need to (URL / headers / body / response
/// parsing).
///
/// `display_url` controls what URL appears in tracing spans — pass
/// `Some("…?key=REDACTED")` when the actual URL embeds an API key
/// (Gemini), `None` to log the real URL (Anthropic / OpenAI / compat
/// providers that put auth in headers).
///
/// Telemetry: emits the canonical observability events
/// (`http_send` / `http_recv` / `retry_scheduled` / `error`) inside the
/// caller's tracing span. The span itself is the caller's
/// responsibility — the Anthropic / OpenAI / Gemini complete() methods
/// open it and pass control here.
pub(crate) async fn call_with_retry(
    http: &reqwest::Client,
    policy: &BackendRetryPolicy,
    url: &str,
    display_url: Option<&str>,
    headers: HeaderMap,
    body: Vec<u8>,
    provider: &str,
    model: &str,
    api_key_env: Option<&str>,
) -> Result<(Vec<u8>, u32), BackendError> {
    let max_retries = policy.max_retries();
    let mut last_status: Option<u16> = None;
    let log_url = display_url.unwrap_or(url);

    for attempt in 0..=max_retries {
        let send_start = Instant::now();
        observability::on_http_send(log_url, body.len());

        let result = http
            .post(url)
            .headers(headers.clone())
            .body(body.clone())
            .send()
            .await;

        match result {
            Ok(response) => {
                let status = response.status().as_u16();
                let response_headers = response.headers().clone();
                let response_bytes = response
                    .bytes()
                    .await
                    .map_err(|e| BackendError::Generic {
                        provider: provider.into(),
                        model: model.into(),
                        status: Some(status),
                        message: format!("failed to read response bytes: {e}"),
                    })?;
                observability::on_http_recv(
                    status,
                    response_bytes.len(),
                    send_start.elapsed().as_millis() as u64,
                );
                last_status = Some(status);

                if status == 200 {
                    return Ok((response_bytes.to_vec(), attempt));
                }

                // Retryable status?
                if BackendRetryPolicy::is_retryable_status(status) && attempt < max_retries {
                    let retry_after = parse_retry_after(&response_headers);
                    let delay = policy.delay_for_response(attempt, retry_after);
                    observability::on_retry_scheduled(
                        attempt,
                        delay.as_millis() as u64,
                        &status.to_string(),
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }

                // Fail-fast or budget exhausted — categorise + return.
                let body_str = String::from_utf8_lossy(&response_bytes).to_string();
                let err = categorise_http(
                    provider,
                    model,
                    status,
                    &response_headers,
                    &body_str,
                    api_key_env,
                );
                observability::on_error(err.category(), Some(status), &err.to_string());
                return Err(err);
            }
            Err(e) if attempt < max_retries => {
                // Transport-layer failure — retry with exponential backoff.
                let reason = if e.is_timeout() {
                    "timeout"
                } else if e.is_connect() {
                    "connect"
                } else {
                    "transport"
                };
                let delay = policy.delay_for_transport(attempt);
                observability::on_retry_scheduled(
                    attempt,
                    delay.as_millis() as u64,
                    reason,
                );
                tokio::time::sleep(delay).await;
                continue;
            }
            Err(e) => {
                let err = BackendError::Generic {
                    provider: provider.into(),
                    model: model.into(),
                    status: None,
                    message: format!("transport failure after {} attempts: {e}", attempt + 1),
                };
                observability::on_error(err.category(), None, &err.to_string());
                return Err(err);
            }
        }
    }

    // Defensive — loop above always returns.
    Err(BackendError::Generic {
        provider: provider.into(),
        model: model.into(),
        status: last_status,
        message: format!("retry budget exhausted ({max_retries} retries)"),
    })
}

#[cfg(test)]
mod tests {
    //! Integration coverage for `call_with_retry` lives in the per-
    //! provider modules (each backend exercises its own success +
    //! retry + categorisation paths against their fixtures). Live
    //! HTTP smoke tests are creds-gated `#[ignore]` and execute via
    //! `cargo test -- --ignored` against real provider endpoints.
    //!
    //! Pure-unit tests for the retry math live in [`super::retry`];
    //! pure-unit tests for status categorisation live in
    //! [`super::error`]. Both are exercised independently — the
    //! `transport` glue is pure I/O orchestration, hence integration-
    //! covered rather than unit-covered.
}
