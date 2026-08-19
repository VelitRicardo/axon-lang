//! Per-tenant API key resolution via AWS Secrets Manager (M3).
//!
//! Resolution chain for every (tenant_id, provider) pair:
//!   1. In-memory cache (TTL 5 min) — sync-safe via std::sync::RwLock
//!   2. AWS Secrets Manager: `axon/tenants/{tenant_id}/{provider}_api_key`
//!   3. Global env-var fallback (same as single-tenant open-source behavior)
//!
//! The cache uses `std::sync::RwLock` (not tokio's) so it can be read from
//! synchronous call sites inside `resolve_backend_key` without any async overhead.
//!
//! # v1.24.0 — `crate::backend` deprecation
//!
//! This file's env-var-fallback step uses the deprecated
//! `crate::backend::get_api_key` (now a thin shim around the
//! consolidated `crate::backends::get_api_key`). The
//! `#![allow(deprecated)]` silences the warning while the deeper
//! migration progresses under v1.24.0.

#![allow(deprecated)]

use std::{
    collections::HashMap,
    sync::RwLock,
    time::{Duration, Instant},
};

const CACHE_TTL: Duration = Duration::from_secs(300); // 5 minutes

type CacheKey = (String, String); // (tenant_id, provider)
type CacheEntry = (String, Instant); // (api_key, fetched_at)

pub struct TenantSecretsClient {
    cache: RwLock<HashMap<CacheKey, CacheEntry>>,
    /// None when AWS credentials are unavailable (open-source / local dev).
    /// v2.77.0 — the field (and the whole AWS SDK dependency tree) only exists
    /// under the default-on `aws-secrets` feature; without it the resolution
    /// chain is cache → env-var, identical to a creds-less box.
    #[cfg(feature = "aws-secrets")]
    sm_client: Option<aws_sdk_secretsmanager::Client>,
}

impl TenantSecretsClient {
    /// Creates a client with AWS credentials loaded from the environment.
    /// Falls back gracefully to `None` if credentials or region are missing.
    #[cfg(feature = "aws-secrets")]
    pub async fn new() -> Self {
        let sm_client = Self::try_init_sm_client().await;
        if sm_client.is_none() {
            tracing::warn!("tenant_secrets: AWS SM client not available — env-var fallback only");
        } else {
            tracing::info!("tenant_secrets: AWS SM client initialized");
        }
        Self { cache: RwLock::new(HashMap::new()), sm_client }
    }

    /// v2.77.0 — compiled without the `aws-secrets` feature: same constructor
    /// signature, env-var fallback only (the SM leg does not exist).
    #[cfg(not(feature = "aws-secrets"))]
    pub async fn new() -> Self {
        tracing::info!(
            "tenant_secrets: compiled without the `aws-secrets` feature — env-var fallback only"
        );
        Self::new_stub()
    }

    /// Creates a stub client for use before async init (sync contexts, tests).
    /// Has no SM client — only env-var fallback operates.
    pub fn new_stub() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            #[cfg(feature = "aws-secrets")]
            sm_client: None,
        }
    }

    /// Replaces the SM client after async initialization completes.
    /// Called from `run_serve()` after the tokio runtime is ready.
    #[cfg(feature = "aws-secrets")]
    pub fn set_sm_client(&mut self, client: aws_sdk_secretsmanager::Client) {
        self.sm_client = Some(client);
    }

    // ── Sync API (for resolve_backend_key and other sync call sites) ──────────

    /// Returns a cached API key if it exists and has not expired.
    /// This is the fast path — no I/O, no await required.
    pub fn get_cached(&self, tenant_id: &str, provider: &str) -> Option<String> {
        let cache = self.cache.read().ok()?;
        let key = (tenant_id.to_string(), provider.to_string());
        if let Some((api_key, fetched_at)) = cache.get(&key) {
            if fetched_at.elapsed() < CACHE_TTL {
                return Some(api_key.clone());
            }
        }
        None
    }

    // ── Async API (for async handlers) ────────────────────────────────────────

    /// Full resolution: cache → AWS SM → env-var fallback.
    /// Populates the cache on a successful SM fetch.
    pub async fn get_api_key(
        &self,
        tenant_id: &str,
        provider: &str,
    ) -> Result<String, String> {
        // 1. Cache hit
        if let Some(key) = self.get_cached(tenant_id, provider) {
            return Ok(key);
        }

        // 2. AWS Secrets Manager (only exists under `aws-secrets`, v2.77.0)
        if let Some(api_key) = self.get_from_sm(tenant_id, provider).await {
            return Ok(api_key);
        }

        // 3. Global env-var fallback
        crate::backend::get_api_key(provider).map_err(|e| e.message)
    }

    /// The AWS SM leg of the resolution chain: fetch, cache, log. `None` on any
    /// miss/failure (the chain falls through to the env-var fallback).
    #[cfg(feature = "aws-secrets")]
    async fn get_from_sm(&self, tenant_id: &str, provider: &str) -> Option<String> {
        let sm = self.sm_client.as_ref()?;
        let secret_id = Self::secret_path(tenant_id, provider);
        match sm.get_secret_value().secret_id(&secret_id).send().await {
            Ok(resp) => {
                if let Some(value) = resp.secret_string() {
                    let api_key = value.trim().to_string();
                    if !api_key.is_empty() {
                        self.store_cache(tenant_id, provider, &api_key);
                        tracing::debug!(
                            tenant_id, provider, secret_id,
                            "tenant_secret_resolved_from_sm"
                        );
                        return Some(api_key);
                    }
                }
                None
            }
            Err(e) => {
                // Log but do not hard-fail — fall through to env var
                tracing::warn!(
                    tenant_id, provider, secret_id, error = %e,
                    "tenant_secret_sm_lookup_failed"
                );
                None
            }
        }
    }

    /// v2.77.0 — without the feature the SM leg is a no-op: the chain is
    /// cache → env-var, byte-identical to a creds-less deployment.
    #[cfg(not(feature = "aws-secrets"))]
    async fn get_from_sm(&self, _tenant_id: &str, _provider: &str) -> Option<String> {
        None
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    #[cfg(feature = "aws-secrets")]
    fn secret_path(tenant_id: &str, provider: &str) -> String {
        format!("axon/tenants/{tenant_id}/{provider}_api_key")
    }

    // (`test` too: the cache unit tests exercise store_cache without the SM leg.)
    #[cfg(any(feature = "aws-secrets", test))]
    fn store_cache(&self, tenant_id: &str, provider: &str, api_key: &str) {
        if let Ok(mut cache) = self.cache.write() {
            cache.insert(
                (tenant_id.to_string(), provider.to_string()),
                (api_key.to_string(), Instant::now()),
            );
        }
    }

    #[cfg(feature = "aws-secrets")]
    async fn try_init_sm_client() -> Option<aws_sdk_secretsmanager::Client> {
        // aws_config::load_from_env() returns a config even without credentials;
        // the actual credential error surfaces on first API call. We guard here
        // by checking that AWS_DEFAULT_REGION or AWS_REGION is set — a necessary
        // (though not sufficient) proxy for a configured AWS environment.
        let region = std::env::var("AWS_DEFAULT_REGION")
            .or_else(|_| std::env::var("AWS_REGION"))
            .ok()?;
        if region.is_empty() {
            return None;
        }
        let config = aws_config::load_from_env().await;
        Some(aws_sdk_secretsmanager::Client::new(&config))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub_cache_miss() {
        let client = TenantSecretsClient::new_stub();
        assert_eq!(client.get_cached("acme", "anthropic"), None);
    }

    #[test]
    fn test_cache_hit_within_ttl() {
        let client = TenantSecretsClient::new_stub();
        client.store_cache("acme", "openai", "sk-test-key");
        let result = client.get_cached("acme", "openai");
        assert_eq!(result, Some("sk-test-key".to_string()));
    }

    #[test]
    fn test_cache_miss_different_tenant() {
        let client = TenantSecretsClient::new_stub();
        client.store_cache("tenant-a", "openai", "sk-a");
        assert_eq!(client.get_cached("tenant-b", "openai"), None);
    }

    #[test]
    fn test_cache_miss_different_provider() {
        let client = TenantSecretsClient::new_stub();
        client.store_cache("acme", "openai", "sk-openai");
        assert_eq!(client.get_cached("acme", "anthropic"), None);
    }

    // v2.81.0 — `secret_path` is `#[cfg(feature = "aws-secrets")]` (it names
    // an AWS Secrets Manager path and has no meaning without the SM leg), so
    // this test must carry the same gate. Without it `cargo test
    // --no-default-features` fails to COMPILE — which is how v2.77.0 shipped:
    // its opt-out was verified with `--lib --bins`, and the TEST target was
    // never built without the feature. v2.81.0 makes this configuration the
    // DEFAULT, so the gap had to close first.
    #[cfg(feature = "aws-secrets")]
    #[test]
    fn test_secret_path_format() {
        assert_eq!(
            TenantSecretsClient::secret_path("acme-corp", "anthropic"),
            "axon/tenants/acme-corp/anthropic_api_key"
        );
        assert_eq!(
            TenantSecretsClient::secret_path("example-tenant", "openai"),
            "axon/tenants/example-tenant/openai_api_key"
        );
    }

    #[tokio::test]
    async fn test_get_api_key_stub_falls_back_to_env() {
        // Without SM client, should fall through to env-var lookup.
        // If ANTHROPIC_API_KEY is not set this will return Err — that's expected.
        let client = TenantSecretsClient::new_stub();
        let result = client.get_api_key("acme", "anthropic").await;
        // We only assert no panic; actual result depends on env.
        let _ = result;
    }
}
