//! Resilient Backend — production-grade LLM call wrapper with retry, circuit breaker, and fallback.
//!
//! Composes multiple resilience patterns into a single call path:
//!   1. **Circuit Breaker**: Fail fast if provider is known to be down (isolated per tenant)
//!   2. **Timeout**: Configurable connect + read timeout per provider
//!   3. **Retry with Backoff**: Exponential backoff with jitter for transient errors
//!   4. **Fallback Chain**: If primary provider fails, try secondary/tertiary
//!
//! Circuit breakers are keyed by `(tenant_id, provider)` so one tenant's failures
//! cannot open another tenant's circuit — complete blast-radius isolation (M4).
//!
//! Designed for production SaaS workloads where LLM availability is critical.
//! All state transitions and retry attempts are logged via `tracing`.
//!
//! # v1.24.0 — `crate::backend` deprecation
//!
//! This file wraps the deprecated synchronous `crate::backend`
//! surface with resilience patterns. The `#![allow(deprecated)]`
//! below silences the deprecation warnings while the deeper async
//! migration progresses under followup step v1.24.0.
//! The async equivalent (`backends::Backend::complete()` with
//! retry/circuit-breaker layers) is the migration target.

#![allow(deprecated)]

use std::collections::HashMap;
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use crate::backend::{self, BackendError, ModelResponse};
use crate::backend_error::BackendErrorKind;
use crate::circuit_breaker::CircuitBreaker;
use crate::retry_policy::RetryPolicy;

/// Per-provider resilience configuration.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// Provider name (e.g., "anthropic", "openai").
    pub name: String,
    /// Connection timeout.
    pub connect_timeout: Duration,
    /// Read/response timeout (includes LLM thinking time).
    pub read_timeout: Duration,
    /// Retry policy for this provider.
    pub retry_policy: RetryPolicy,
}

impl ProviderConfig {
    pub fn new(name: &str) -> Self {
        ProviderConfig {
            name: name.to_string(),
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(120),
            retry_policy: RetryPolicy::default(),
        }
    }
}

/// Resilient backend wrapping the raw LLM call layer with production hardening.
///
/// Circuit breakers are keyed by `(tenant_id, provider)` and created lazily on
/// first access — one tenant's failures cannot trip another tenant's circuit.
pub struct ResilientBackend {
    /// Per-(tenant_id, provider) circuit breakers — created lazily on demand.
    /// RwLock: concurrent reads (per-tenant lookups), exclusive writes only on
    /// first encounter of a new (tenant, provider) pair.
    circuit_breakers: RwLock<HashMap<(String, String), Mutex<CircuitBreaker>>>,
    /// Per-provider configuration (shared across all tenants).
    provider_configs: HashMap<String, ProviderConfig>,
    /// Fallback chains: primary_provider → [fallback_1, fallback_2, ...]
    fallback_chains: HashMap<String, Vec<String>>,
}

impl ResilientBackend {
    /// Create a new resilient backend with default configs for all supported providers.
    /// Circuit breakers are created lazily on first (tenant, provider) access.
    pub fn new() -> Self {
        let mut provider_configs = HashMap::new();
        for &name in backend::SUPPORTED_BACKENDS {
            provider_configs.insert(name.to_string(), ProviderConfig::new(name));
        }
        ResilientBackend {
            circuit_breakers: RwLock::new(HashMap::new()),
            provider_configs,
            fallback_chains: HashMap::new(),
        }
    }

    /// Configure a specific provider's resilience settings.
    pub fn configure_provider(&mut self, config: ProviderConfig) {
        self.provider_configs.insert(config.name.clone(), config);
    }

    /// Set a fallback chain for a provider.
    pub fn set_fallback_chain(&mut self, primary: &str, fallbacks: Vec<String>) {
        self.fallback_chains.insert(primary.to_string(), fallbacks);
    }

    /// Make a resilient LLM call with retry, per-tenant circuit breaker, and fallback.
    ///
    /// # v2.89.0 — `tenant_id` is a parameter, and used to be ambient
    ///
    /// This doc used to read *"Tenant is derived automatically from
    /// `current_tenant_id()` — the active Axum request's task-local"*. That
    /// sentence was a trap with the pin pulled, and the shape of the trap is
    /// worth naming because it is not obvious from the code.
    ///
    /// `call` is a **synchronous `fn` doing blocking network I/O**. This
    /// codebase has one rule for that — Brief #63: blocking work goes on
    /// `spawn_blocking`, or it hangs the server. So the first caller to wire
    /// this up correctly, by following the rule the codebase enforces
    /// everywhere else, would put it on the far side of a task boundary, where
    /// the task-local does not exist and `current_tenant_id()` returns the
    /// `"default"` fallback. Every tenant would then share one circuit breaker
    /// — collapsing precisely the per-`(tenant, provider)` isolation this type
    /// exists for — and nothing would say so.
    ///
    /// v2.89.0's census found `call` has ZERO production callers today: the
    /// field is built on `ServerState` and never read. So this is not a defect
    /// being fixed, it is a defect being **disarmed before it is reachable** —
    /// the alternative was leaving a doc comment that instructs the next author
    /// to walk into it. `call_single_provider` already took the tenant
    /// explicitly; the wrapper is now consistent with the thing it wraps.
    pub fn call(
        &self,
        tenant_id: &str,
        provider: &str,
        api_key: &str,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: Option<u32>,
    ) -> Result<ModelResponse, BackendError> {
        match self.call_single_provider(tenant_id, provider, api_key, system_prompt, user_prompt, max_tokens) {
            Ok(resp) => Ok(resp),
            Err(primary_err) => {
                if let Some(fallbacks) = self.fallback_chains.get(provider) {
                    for fallback in fallbacks {
                        tracing::warn!(
                            tenant_id = %tenant_id,
                            primary = provider,
                            fallback = %fallback,
                            primary_error = %primary_err,
                            "resilient_backend_trying_fallback"
                        );
                        let fallback_key = match backend::get_api_key(fallback) {
                            Ok(k) => k,
                            Err(_) => continue,
                        };
                        match self.call_single_provider(
                            &tenant_id, fallback, &fallback_key, system_prompt, user_prompt, max_tokens,
                        ) {
                            Ok(resp) => {
                                tracing::info!(
                                    tenant_id = %tenant_id,
                                    primary = provider,
                                    fallback = %fallback,
                                    "resilient_backend_fallback_succeeded"
                                );
                                return Ok(resp);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    tenant_id = %tenant_id,
                                    fallback = %fallback,
                                    error = %e,
                                    "resilient_backend_fallback_failed"
                                );
                            }
                        }
                    }
                }
                Err(primary_err)
            }
        }
    }

    /// Call a single provider with the tenant-isolated circuit breaker + retry.
    fn call_single_provider(
        &self,
        tenant_id: &str,
        provider: &str,
        api_key: &str,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: Option<u32>,
    ) -> Result<ModelResponse, BackendError> {
        let cb_key = (tenant_id.to_string(), provider.to_string());

        // Lazy-init: insert a new CB only on first encounter of this (tenant, provider) pair.
        // Check with a read lock first; upgrade to write only when needed.
        {
            let map = self.circuit_breakers.read().unwrap();
            if !map.contains_key(&cb_key) {
                drop(map);
                let mut map = self.circuit_breakers.write().unwrap();
                map.entry(cb_key.clone())
                    .or_insert_with(|| Mutex::new(CircuitBreaker::with_defaults(provider)));
            }
        }

        // Check circuit breaker state
        {
            let map = self.circuit_breakers.read().unwrap();
            let cb_mutex = map.get(&cb_key).unwrap();
            let mut cb = cb_mutex.lock().unwrap();
            if !cb.can_execute() {
                tracing::warn!(
                    tenant_id, provider,
                    state = %cb.state(),
                    "resilient_backend_circuit_open"
                );
                return Err(BackendError {
                    message: format!(
                        "Circuit breaker open for provider '{provider}' (tenant '{tenant_id}') — calls rejected"
                    ),
                });
            }
        }

        let retry_policy = self.provider_configs
            .get(provider)
            .map(|c| c.retry_policy.clone())
            .unwrap_or_default();

        let mut last_error = None;
        for attempt in 0..=retry_policy.max_retries {
            if attempt > 0 {
                let error_kind = BackendErrorKind::Unknown;
                let delay = retry_policy.effective_delay(attempt - 1, &error_kind);
                tracing::info!(
                    tenant_id, provider, attempt,
                    delay_ms = delay.as_millis() as u64,
                    "resilient_backend_retrying"
                );
                std::thread::sleep(delay);
            }

            match backend::call(provider, api_key, system_prompt, user_prompt, max_tokens) {
                Ok(resp) => {
                    let map = self.circuit_breakers.read().unwrap();
                    if let Some(cb_mutex) = map.get(&cb_key) {
                        cb_mutex.lock().unwrap().record_success();
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    let error_kind = classify_backend_error(&e);
                    tracing::warn!(
                        tenant_id, provider, attempt,
                        error = %e,
                        error_kind = error_kind.category(),
                        retryable = error_kind.is_retryable(),
                        "resilient_backend_call_failed"
                    );
                    {
                        let map = self.circuit_breakers.read().unwrap();
                        if let Some(cb_mutex) = map.get(&cb_key) {
                            cb_mutex.lock().unwrap().record_failure();
                        }
                    }
                    if !retry_policy.should_retry(attempt, &error_kind) {
                        return Err(e);
                    }
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| BackendError {
            message: format!(
                "All {} retry attempts exhausted for provider '{provider}' (tenant '{tenant_id}')",
                retry_policy.max_retries
            ),
        }))
    }

    /// Get the circuit breaker state for a (tenant, provider) pair.
    pub fn circuit_state(
        &self,
        tenant_id: &str,
        provider: &str,
    ) -> Option<crate::circuit_breaker::CircuitState> {
        let map = self.circuit_breakers.read().unwrap();
        map.get(&(tenant_id.to_string(), provider.to_string())).map(|cb| {
            cb.lock().unwrap().state()
        })
    }

    /// Reset the circuit breaker for a (tenant, provider) pair.
    pub fn reset_circuit(&self, tenant_id: &str, provider: &str) {
        let map = self.circuit_breakers.read().unwrap();
        if let Some(cb_mutex) = map.get(&(tenant_id.to_string(), provider.to_string())) {
            cb_mutex.lock().unwrap().reset();
            tracing::info!(tenant_id, provider, "circuit_breaker_manually_reset");
        }
    }

    /// List all active (tenant, provider) circuit breaker states.
    pub fn all_circuit_states(&self) -> Vec<(String, String, crate::circuit_breaker::CircuitState)> {
        let map = self.circuit_breakers.read().unwrap();
        map.iter().map(|((tid, prov), cb)| {
            (tid.clone(), prov.clone(), cb.lock().unwrap().state())
        }).collect()
    }
}

/// Classify a BackendError into a BackendErrorKind by inspecting the message.
fn classify_backend_error(e: &BackendError) -> BackendErrorKind {
    let msg = e.message.to_lowercase();

    if msg.contains("timeout") || msg.contains("timed out") {
        BackendErrorKind::Timeout
    } else if msg.contains("429") || msg.contains("rate limit") || msg.contains("too many requests") {
        BackendErrorKind::RateLimit { retry_after: None }
    } else if msg.contains("401") || msg.contains("403") || msg.contains("unauthorized") || msg.contains("forbidden") {
        BackendErrorKind::AuthError
    } else if msg.contains("api error (5") {
        // Match "API error (500)", "API error (502)", etc.
        let status = msg.split("api error (")
            .nth(1)
            .and_then(|s| s.split(')').next())
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(500);
        BackendErrorKind::ServerError { status }
    } else if msg.contains("connection refused") || msg.contains("dns") || msg.contains("http request failed") {
        BackendErrorKind::NetworkError
    } else if msg.contains("stream") && (msg.contains("error") || msg.contains("dropped")) {
        BackendErrorKind::StreamDropped
    } else if msg.contains("parse") || msg.contains("json") {
        BackendErrorKind::InvalidResponse
    } else if msg.contains("unknown backend") {
        BackendErrorKind::ProviderUnavailable
    } else {
        BackendErrorKind::Unknown
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_starts_empty_no_circuits() {
        let rb = ResilientBackend::new();
        // Lazy init — map is empty until first call
        assert!(rb.circuit_breakers.read().unwrap().is_empty());
    }

    #[test]
    fn test_circuit_state_returns_none_before_first_call() {
        let rb = ResilientBackend::new();
        // No circuit breaker exists until a call is made for this (tenant, provider)
        assert_eq!(rb.circuit_state("acme", "anthropic"), None);
    }

    #[test]
    fn test_circuit_state_closed_after_lazy_init() {
        let rb = ResilientBackend::new();
        // Force lazy init by manually inserting
        {
            let mut map = rb.circuit_breakers.write().unwrap();
            map.entry(("acme".to_string(), "anthropic".to_string()))
                .or_insert_with(|| Mutex::new(CircuitBreaker::with_defaults("anthropic")));
        }
        assert_eq!(
            rb.circuit_state("acme", "anthropic"),
            Some(crate::circuit_breaker::CircuitState::Closed)
        );
    }

    #[test]
    fn test_reset_circuit_per_tenant() {
        let rb = ResilientBackend::new();
        // Force open for tenant "acme"
        {
            let mut map = rb.circuit_breakers.write().unwrap();
            let cb = map.entry(("acme".to_string(), "openai".to_string()))
                .or_insert_with(|| Mutex::new(CircuitBreaker::with_defaults("openai")));
            for _ in 0..5 {
                cb.lock().unwrap().record_failure();
            }
            assert_eq!(cb.lock().unwrap().state(), crate::circuit_breaker::CircuitState::Open);
        }
        rb.reset_circuit("acme", "openai");
        assert_eq!(
            rb.circuit_state("acme", "openai"),
            Some(crate::circuit_breaker::CircuitState::Closed)
        );
    }

    #[test]
    fn test_tenant_isolation_circuits_independent() {
        let rb = ResilientBackend::new();
        // Open circuit for tenant-a / anthropic
        {
            let mut map = rb.circuit_breakers.write().unwrap();
            let cb = map.entry(("tenant-a".to_string(), "anthropic".to_string()))
                .or_insert_with(|| Mutex::new(CircuitBreaker::with_defaults("anthropic")));
            for _ in 0..5 {
                cb.lock().unwrap().record_failure();
            }
        }
        // tenant-b / anthropic circuit should not exist (not open)
        assert_eq!(rb.circuit_state("tenant-b", "anthropic"), None);
        // tenant-a / anthropic should be open
        assert_eq!(
            rb.circuit_state("tenant-a", "anthropic"),
            Some(crate::circuit_breaker::CircuitState::Open)
        );
    }

    #[test]
    fn test_all_circuit_states() {
        let rb = ResilientBackend::new();
        {
            let mut map = rb.circuit_breakers.write().unwrap();
            map.insert(("t1".to_string(), "anthropic".to_string()),
                Mutex::new(CircuitBreaker::with_defaults("anthropic")));
            map.insert(("t2".to_string(), "openai".to_string()),
                Mutex::new(CircuitBreaker::with_defaults("openai")));
        }
        let states = rb.all_circuit_states();
        assert_eq!(states.len(), 2);
    }

    #[test]
    fn test_classify_timeout() {
        let e = BackendError { message: "HTTP request failed: operation timed out".into() };
        assert!(matches!(classify_backend_error(&e), BackendErrorKind::Timeout));
    }

    #[test]
    fn test_classify_rate_limit() {
        let e = BackendError { message: "API error (429): Too Many Requests".into() };
        assert!(matches!(classify_backend_error(&e), BackendErrorKind::RateLimit { .. }));
    }

    #[test]
    fn test_classify_auth() {
        let e = BackendError { message: "API error (401): Unauthorized".into() };
        assert!(matches!(classify_backend_error(&e), BackendErrorKind::AuthError));
    }

    #[test]
    fn test_classify_server_error() {
        let e = BackendError { message: "API error (503): Service Unavailable".into() };
        assert!(matches!(classify_backend_error(&e), BackendErrorKind::ServerError { status: 503 }));
    }

    #[test]
    fn test_classify_network() {
        let e = BackendError { message: "HTTP request failed: connection refused".into() };
        assert!(matches!(classify_backend_error(&e), BackendErrorKind::NetworkError));
    }

    #[test]
    fn test_classify_unknown_backend() {
        let e = BackendError { message: "Unknown backend 'foo'".into() };
        assert!(matches!(classify_backend_error(&e), BackendErrorKind::ProviderUnavailable));
    }

    #[test]
    fn test_set_fallback_chain() {
        let mut rb = ResilientBackend::new();
        rb.set_fallback_chain("anthropic", vec!["openrouter".into(), "ollama".into()]);
        assert_eq!(
            rb.fallback_chains.get("anthropic"),
            Some(&vec!["openrouter".to_string(), "ollama".to_string()])
        );
    }
}
