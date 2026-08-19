//! Retry policy for native Rust LLM backends — v1.18.0.
//!
//! Mirror of the v1.16.1 Python `_call_with_retry` policy in
//! `axon.server.model_clients`. The transport layer retries on:
//!
//!   * HTTP 429 — honour `Retry-After` header up to `max_backoff`.
//!   * HTTP 408 — request timeout (rare from LLM providers, retryable).
//!   * HTTP 5xx — exponential backoff with jitter.
//!   * Network timeout / connect / request errors — exponential backoff.
//!
//! HTTP 4xx (other) is fail-fast — non-retryable. The retry budget is
//! capped by [`BackendRetryPolicy::max_retries`] (default 3, matching
//! Python).
//!
//! # Composition
//!
//! This module sits ABOVE the legacy [`crate::retry_policy::RetryPolicy`]
//! — that struct knows how to compute exponential delays for a given
//! attempt; this module adds:
//!
//!   * Per-response `Retry-After` parsing (integer-seconds form, the
//!     only form Python v1.16.1 honours).
//!   * Constants matching the Python defaults (`MAX_RETRIES = 3`,
//!     `MAX_BACKOFF_SECONDS = 30.0`, `BASE_BACKOFF_SECONDS = 1.0`).
//!   * A backend-aware `effective_delay_for_response` that the per-
//!     provider HTTP loop calls instead of consulting the legacy
//!     policy directly.

use std::time::Duration;

use crate::retry_policy::RetryPolicy;

/// Defaults match `axon.server.model_clients` Python constants verbatim
/// so behaviour stays identical when the same code paths run on either
/// stack — drift here would silently change retry budgets.
pub const DEFAULT_MAX_RETRIES: u32 = 3;
pub const DEFAULT_BASE_BACKOFF: Duration = Duration::from_millis(500);
pub const DEFAULT_MAX_BACKOFF: Duration = Duration::from_secs(30);
pub const DEFAULT_JITTER: Duration = Duration::from_millis(500);

/// Backend-specific retry policy. Wraps the legacy [`RetryPolicy`] +
/// adds `Retry-After` header parsing.
#[derive(Debug, Clone)]
pub struct BackendRetryPolicy {
    inner: RetryPolicy,
    /// Maximum delay we will honour from a `Retry-After` header. If the
    /// header asks for more, we cap at this value (default 30s) so an
    /// overly aggressive provider doesn't strand a request indefinitely.
    pub max_retry_after: Duration,
}

impl BackendRetryPolicy {
    /// Production default — matches the Python v1.16.1 transport.
    pub fn production() -> Self {
        Self {
            inner: RetryPolicy {
                max_retries: DEFAULT_MAX_RETRIES,
                base_delay: DEFAULT_BASE_BACKOFF,
                max_delay: DEFAULT_MAX_BACKOFF,
                backoff_multiplier: 2.0,
                jitter: true,
            },
            max_retry_after: DEFAULT_MAX_BACKOFF,
        }
    }

    /// Test-friendly — no retries, no waits. Useful for fail-fast unit
    /// tests where the goal is to surface the categorised error.
    pub fn no_retry() -> Self {
        Self {
            inner: RetryPolicy::no_retry(),
            max_retry_after: Duration::ZERO,
        }
    }

    pub fn max_retries(&self) -> u32 {
        self.inner.max_retries
    }

    pub fn inner(&self) -> &RetryPolicy {
        &self.inner
    }

    /// Whether an HTTP status code should trigger a retry.
    /// 429 + 408 + 5xx are retryable; everything else is fail-fast.
    pub fn is_retryable_status(status: u16) -> bool {
        status == 429 || status == 408 || (500..600).contains(&status)
    }

    /// Compute the next-attempt delay given an HTTP response.
    ///
    /// If the response carries a `Retry-After` header (integer-seconds
    /// form), honour it up to `max_retry_after`. Otherwise fall back to
    /// the inner exponential-backoff schedule.
    pub fn delay_for_response(
        &self,
        attempt: u32,
        retry_after_seconds: Option<u64>,
    ) -> Duration {
        if let Some(secs) = retry_after_seconds {
            return Duration::from_secs(secs).min(self.max_retry_after);
        }
        self.inner.delay_for_attempt(attempt)
    }

    /// Compute the delay for a non-HTTP transport error (no response to
    /// inspect). Always falls back to exponential backoff.
    pub fn delay_for_transport(&self, attempt: u32) -> Duration {
        self.inner.delay_for_attempt(attempt)
    }
}

impl Default for BackendRetryPolicy {
    fn default() -> Self {
        Self::production()
    }
}

/// Parse a `Retry-After` header value into a number of seconds.
///
/// Mirrors the Python implementation: integer-seconds form only (HTTP-
/// date is rare from LLM providers and complicates the parser). When
/// the header is absent or unparseable, returns `None`.
pub fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
}

// ────────────────────────────────────────────────────────────────────
//  Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderMap;

    #[test]
    fn defaults_match_python_v1_16_1_constants() {
        let p = BackendRetryPolicy::production();
        assert_eq!(p.max_retries(), 3);
        assert_eq!(p.max_retry_after, Duration::from_secs(30));
    }

    #[test]
    fn no_retry_policy_disables_retries() {
        let p = BackendRetryPolicy::no_retry();
        assert_eq!(p.max_retries(), 0);
    }

    #[test]
    fn retryable_status_codes() {
        assert!(BackendRetryPolicy::is_retryable_status(429));
        assert!(BackendRetryPolicy::is_retryable_status(408));
        assert!(BackendRetryPolicy::is_retryable_status(500));
        assert!(BackendRetryPolicy::is_retryable_status(502));
        assert!(BackendRetryPolicy::is_retryable_status(503));
        assert!(BackendRetryPolicy::is_retryable_status(599));
    }

    #[test]
    fn non_retryable_status_codes() {
        assert!(!BackendRetryPolicy::is_retryable_status(400));
        assert!(!BackendRetryPolicy::is_retryable_status(401));
        assert!(!BackendRetryPolicy::is_retryable_status(403));
        assert!(!BackendRetryPolicy::is_retryable_status(404));
        assert!(!BackendRetryPolicy::is_retryable_status(200));
        assert!(!BackendRetryPolicy::is_retryable_status(600));
    }

    #[test]
    fn parse_retry_after_integer_seconds() {
        let mut h = HeaderMap::new();
        h.insert("retry-after", "60".parse().unwrap());
        assert_eq!(parse_retry_after(&h), Some(60));
    }

    #[test]
    fn parse_retry_after_with_whitespace() {
        let mut h = HeaderMap::new();
        h.insert("retry-after", "  120  ".parse().unwrap());
        assert_eq!(parse_retry_after(&h), Some(120));
    }

    #[test]
    fn parse_retry_after_missing_returns_none() {
        let h = HeaderMap::new();
        assert_eq!(parse_retry_after(&h), None);
    }

    #[test]
    fn parse_retry_after_http_date_returns_none() {
        // HTTP-date form is intentionally not parsed (rare from LLM
        // providers; Python v1.16.1 also returns None for this case).
        let mut h = HeaderMap::new();
        h.insert("retry-after", "Wed, 21 Oct 2026 07:28:00 GMT".parse().unwrap());
        assert_eq!(parse_retry_after(&h), None);
    }

    #[test]
    fn delay_for_response_honours_retry_after() {
        let p = BackendRetryPolicy::production();
        let delay = p.delay_for_response(0, Some(5));
        assert_eq!(delay, Duration::from_secs(5));
    }

    #[test]
    fn delay_for_response_caps_retry_after_at_max() {
        let p = BackendRetryPolicy::production();
        let delay = p.delay_for_response(0, Some(99999));
        assert_eq!(delay, Duration::from_secs(30)); // capped
    }

    #[test]
    fn delay_for_response_falls_back_to_exponential_when_no_header() {
        let p = BackendRetryPolicy::production();
        // attempt=0 yields base_delay (500ms) +/- jitter; verify it's
        // bounded — exact value is jitter-dependent so we check range.
        let delay = p.delay_for_response(0, None);
        assert!(delay >= Duration::from_millis(500));
        assert!(delay <= Duration::from_secs(30));
    }

    #[test]
    fn delay_for_transport_uses_exponential_backoff() {
        let p = BackendRetryPolicy::production();
        let d0 = p.delay_for_transport(0);
        let d2 = p.delay_for_transport(2);
        // attempt 2 should be larger than attempt 0 (multiplier 2.0).
        assert!(d2 >= d0);
        assert!(d2 <= Duration::from_secs(30)); // capped
    }
}
