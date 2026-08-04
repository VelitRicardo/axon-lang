//! Rate Limiter — sliding window rate limiting for AxonServer.
//!
//! Implements a sliding window counter algorithm per client key (IP or token).
//! Each window tracks request timestamps; expired entries are pruned on access.
//!
//! Configuration:
//!   - `max_requests` — maximum requests per window (default: 100)
//!   - `window_secs` — window duration in seconds (default: 60)
//!
//! Integration: called from AxonServer handlers before processing requests.
//! Returns `RateLimitResult` with allowed/denied status and remaining quota.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use serde::Serialize;

// ── Configuration ────────────────────────────────────────────────────────

/// Rate limiter configuration.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests allowed per window.
    pub max_requests: u32,
    /// Window duration.
    pub window: Duration,
    /// Whether rate limiting is enabled.
    pub enabled: bool,
}

impl RateLimitConfig {
    /// Default: 100 requests per 60 seconds.
    pub fn default_config() -> Self {
        RateLimitConfig {
            max_requests: 100,
            window: Duration::from_secs(60),
            enabled: true,
        }
    }

    /// Disabled rate limiter.
    pub fn disabled() -> Self {
        RateLimitConfig {
            max_requests: 0,
            window: Duration::from_secs(0),
            enabled: false,
        }
    }
}

// ── Result ───────────────────────────────────────────────────────────────

/// Result of a rate limit check.
#[derive(Debug, Clone, Serialize)]
pub struct RateLimitResult {
    /// Whether the request is allowed.
    pub allowed: bool,
    /// Remaining requests in the current window.
    pub remaining: u32,
    /// Total limit per window.
    pub limit: u32,
    /// Seconds until the window resets (oldest entry expires).
    pub reset_secs: u64,
}

// ── Limiter ──────────────────────────────────────────────────────────────

/// Per-client rate limiter metrics (for Prometheus exposition).
#[derive(Debug, Clone)]
pub struct ClientRateMetric {
    pub client_key: String,
    pub total_requests: u64,
    pub rejected: u64,
    pub current_window_count: u32,
}

/// Per-client request timestamps.
struct ClientBucket {
    timestamps: VecDeque<Instant>,
    total_requests: u64,
    rejected: u64,
}

impl ClientBucket {
    fn new() -> Self {
        ClientBucket {
            timestamps: VecDeque::new(),
            total_requests: 0,
            rejected: 0,
        }
    }

    /// Prune expired timestamps and return current count.
    fn prune_and_count(&mut self, now: Instant, window: Duration) -> u32 {
        let cutoff = now.checked_sub(window).unwrap_or(now);
        while let Some(&front) = self.timestamps.front() {
            if front < cutoff {
                self.timestamps.pop_front();
            } else {
                break;
            }
        }
        self.timestamps.len() as u32
    }

    /// Time until the oldest entry expires (window reset).
    fn reset_time(&self, now: Instant, window: Duration) -> u64 {
        if let Some(&oldest) = self.timestamps.front() {
            let expires_at = oldest + window;
            if expires_at > now {
                return (expires_at - now).as_secs();
            }
        }
        0
    }
}

/// Sliding window rate limiter.
pub struct RateLimiter {
    config: RateLimitConfig,
    buckets: HashMap<String, ClientBucket>,
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration.
    pub fn new(config: RateLimitConfig) -> Self {
        RateLimiter {
            config,
            buckets: HashMap::new(),
        }
    }

    /// Check if a request from the given client key is allowed.
    /// If allowed, records the request timestamp.
    pub fn check(&mut self, client_key: &str) -> RateLimitResult {
        if !self.config.enabled {
            return RateLimitResult {
                allowed: true,
                remaining: u32::MAX,
                limit: 0,
                reset_secs: 0,
            };
        }

        let now = Instant::now();
        let bucket = self.buckets
            .entry(client_key.to_string())
            .or_insert_with(ClientBucket::new);

        let count = bucket.prune_and_count(now, self.config.window);
        bucket.total_requests += 1;

        if count >= self.config.max_requests {
            bucket.rejected += 1;
            let reset = bucket.reset_time(now, self.config.window);
            return RateLimitResult {
                allowed: false,
                remaining: 0,
                limit: self.config.max_requests,
                reset_secs: reset,
            };
        }

        // Allow and record
        bucket.timestamps.push_back(now);
        let remaining = self.config.max_requests - count - 1;
        let reset = bucket.reset_time(now, self.config.window);

        RateLimitResult {
            allowed: true,
            remaining,
            limit: self.config.max_requests,
            reset_secs: reset,
        }
    }

    /// Get current stats for a client without recording a request.
    pub fn peek(&mut self, client_key: &str) -> RateLimitResult {
        if !self.config.enabled {
            return RateLimitResult {
                allowed: true,
                remaining: u32::MAX,
                limit: 0,
                reset_secs: 0,
            };
        }

        let now = Instant::now();
        let bucket = self.buckets
            .entry(client_key.to_string())
            .or_insert_with(ClientBucket::new);

        let count = bucket.prune_and_count(now, self.config.window);
        let remaining = self.config.max_requests.saturating_sub(count);
        let reset = bucket.reset_time(now, self.config.window);

        RateLimitResult {
            allowed: remaining > 0,
            remaining,
            limit: self.config.max_requests,
            reset_secs: reset,
        }
    }

    /// Number of tracked client keys.
    pub fn client_count(&self) -> usize {
        self.buckets.len()
    }

    /// Prune all empty buckets (cleanup).
    pub fn cleanup(&mut self) {
        let now = Instant::now();
        let window = self.config.window;
        self.buckets.retain(|_, bucket| {
            bucket.prune_and_count(now, window);
            !bucket.timestamps.is_empty()
        });
    }

    /// Get the configuration.
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Update the configuration at runtime.
    pub fn update_config(&mut self, max_requests: Option<u32>, window_secs: Option<u64>, enabled: Option<bool>) {
        if let Some(max) = max_requests {
            self.config.max_requests = max;
        }
        if let Some(secs) = window_secs {
            self.config.window = Duration::from_secs(secs);
        }
        if let Some(en) = enabled {
            self.config.enabled = en;
        }
    }

    /// Get per-client rate limiter metrics (for Prometheus).
    pub fn client_metrics(&mut self) -> Vec<ClientRateMetric> {
        let now = Instant::now();
        let window = self.config.window;
        self.buckets.iter_mut().map(|(key, bucket)| {
            let current = bucket.prune_and_count(now, window);
            ClientRateMetric {
                client_key: key.clone(),
                total_requests: bucket.total_requests,
                rejected: bucket.rejected,
                current_window_count: current,
            }
        }).collect()
    }
}

// ── Per-tenant rate limiter (M4) ─────────────────────────────────────────

use crate::tenant_context::TenantPlan;

/// Per-plan default quotas for tenant rate limiting.
/// All values are conservative; enterprise is effectively unlimited.
pub struct TenantQuotas {
    /// Maximum requests per minute.
    pub requests_per_min: u32,
    /// Maximum tokens per 24-hour rolling window. u64::MAX = unlimited.
    pub tokens_per_day: u64,
}

impl TenantQuotas {
    pub fn for_plan(plan: &TenantPlan) -> Self {
        match plan {
            TenantPlan::Starter    => Self { requests_per_min: 60,   tokens_per_day: 100_000 },
            TenantPlan::Pro        => Self { requests_per_min: 300,  tokens_per_day: 1_000_000 },
            TenantPlan::Enterprise => Self { requests_per_min: 2000, tokens_per_day: u64::MAX },
        }
    }
}

/// Daily token counter for one tenant.
struct TokenBucket {
    used: u64,
    window_start: std::time::Instant,
}

impl TokenBucket {
    fn new() -> Self {
        Self { used: 0, window_start: std::time::Instant::now() }
    }

    /// Reset counter if the 24-hour window has rolled over.
    fn refresh(&mut self) {
        if self.window_start.elapsed() >= Duration::from_secs(86400) {
            self.used = 0;
            self.window_start = std::time::Instant::now();
        }
    }

    fn add(&mut self, tokens: u64) {
        self.refresh();
        self.used = self.used.saturating_add(tokens);
    }

    fn can_consume(&mut self, limit: u64) -> bool {
        self.refresh();
        self.used < limit
    }
}

/// Per-tenant combined rate limiter: request rate + daily token quota.
///
/// Both dimensions are enforced independently:
///   - Request rate: sliding window per minute (same algorithm as `RateLimiter`)
///   - Token quota: rolling 24-hour counter reset at midnight of first request
///
/// Quotas are derived from `TenantPlan` at check time — no DB round-trip needed.
/// An unknown tenant defaults to `Starter` quotas (safest for open-source compat).
pub struct TenantRateLimiter {
    /// Per-tenant request-rate limiters (created lazily).
    request_limiters: HashMap<String, RateLimiter>,
    /// Per-tenant daily token counters (created lazily).
    token_buckets: HashMap<String, TokenBucket>,
}

impl TenantRateLimiter {
    pub fn new() -> Self {
        Self {
            request_limiters: HashMap::new(),
            token_buckets: HashMap::new(),
        }
    }

    /// Check and record one request for a tenant.
    /// Returns `RateLimitResult` — caller should reject if `!result.allowed`.
    pub fn check_request(&mut self, tenant_id: &str, plan: &TenantPlan) -> RateLimitResult {
        let quotas = TenantQuotas::for_plan(plan);
        let limiter = self.request_limiters
            .entry(tenant_id.to_string())
            .or_insert_with(|| {
                RateLimiter::new(RateLimitConfig {
                    max_requests: quotas.requests_per_min,
                    window: Duration::from_secs(60),
                    enabled: true,
                })
            });
        // Update quota if plan changed (e.g. tenant upgraded)
        limiter.update_config(Some(quotas.requests_per_min), None, None);
        limiter.check(tenant_id)
    }

    /// Record tokens consumed by a tenant (called after a successful LLM response).
    pub fn record_tokens(&mut self, tenant_id: &str, tokens: u64) {
        self.token_buckets
            .entry(tenant_id.to_string())
            .or_insert_with(TokenBucket::new)
            .add(tokens);
    }

    /// Check whether a tenant is within their daily token quota.
    /// Does NOT consume tokens — call `record_tokens` after the LLM call.
    pub fn check_token_quota(&mut self, tenant_id: &str, plan: &TenantPlan) -> bool {
        let limit = TenantQuotas::for_plan(plan).tokens_per_day;
        if limit == u64::MAX {
            return true; // Enterprise = unlimited
        }
        self.token_buckets
            .entry(tenant_id.to_string())
            .or_insert_with(TokenBucket::new)
            .can_consume(limit)
    }

    /// Current token usage for a tenant (used, daily_limit).
    pub fn token_usage(&mut self, tenant_id: &str, plan: &TenantPlan) -> (u64, u64) {
        let limit = TenantQuotas::for_plan(plan).tokens_per_day;
        let bucket = self.token_buckets
            .entry(tenant_id.to_string())
            .or_insert_with(TokenBucket::new);
        bucket.refresh();
        (bucket.used, limit)
    }

    /// Number of tracked tenants.
    pub fn tenant_count(&self) -> usize {
        self.request_limiters.len()
    }

    /// Remove stale tenant entries to prevent unbounded growth.
    pub fn cleanup(&mut self) {
        let now = std::time::Instant::now();
        // Remove tenants whose request windows are all expired (24h+)
        self.token_buckets.retain(|_, b| {
            b.window_start.elapsed() < Duration::from_secs(86400 * 2)
        });
        // Also prune the request limiters
        for limiter in self.request_limiters.values_mut() {
            limiter.cleanup();
        }
        let _ = now; // suppress unused warning
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_config(max: u32, window_ms: u64) -> RateLimitConfig {
        RateLimitConfig {
            max_requests: max,
            window: Duration::from_millis(window_ms),
            enabled: true,
        }
    }

    #[test]
    fn allows_within_limit() {
        let mut limiter = RateLimiter::new(fast_config(5, 1000));
        for i in 0..5 {
            let result = limiter.check("client_a");
            assert!(result.allowed, "request {} should be allowed", i);
            assert_eq!(result.remaining, 4 - i as u32);
            assert_eq!(result.limit, 5);
        }
    }

    #[test]
    fn denies_over_limit() {
        let mut limiter = RateLimiter::new(fast_config(3, 60_000));
        for _ in 0..3 {
            assert!(limiter.check("client_a").allowed);
        }
        let result = limiter.check("client_a");
        assert!(!result.allowed);
        assert_eq!(result.remaining, 0);
    }

    #[test]
    fn separate_clients_independent() {
        let mut limiter = RateLimiter::new(fast_config(2, 60_000));
        assert!(limiter.check("alice").allowed);
        assert!(limiter.check("alice").allowed);
        assert!(!limiter.check("alice").allowed);

        // Bob should still be able to make requests
        assert!(limiter.check("bob").allowed);
        assert!(limiter.check("bob").allowed);
        assert!(!limiter.check("bob").allowed);
    }

    #[test]
    fn window_expiry_allows_again() {
        let mut limiter = RateLimiter::new(fast_config(2, 1)); // 1ms window
        assert!(limiter.check("client").allowed);
        assert!(limiter.check("client").allowed);
        assert!(!limiter.check("client").allowed);

        // Wait for window to expire
        std::thread::sleep(Duration::from_millis(5));
        assert!(limiter.check("client").allowed);
    }

    #[test]
    fn disabled_always_allows() {
        let mut limiter = RateLimiter::new(RateLimitConfig::disabled());
        for _ in 0..1000 {
            let result = limiter.check("anyone");
            assert!(result.allowed);
            assert_eq!(result.remaining, u32::MAX);
        }
    }

    #[test]
    fn peek_does_not_consume() {
        let mut limiter = RateLimiter::new(fast_config(3, 60_000));
        limiter.check("client"); // consume 1

        let peek1 = limiter.peek("client");
        assert!(peek1.allowed);
        assert_eq!(peek1.remaining, 2);

        let peek2 = limiter.peek("client");
        assert_eq!(peek2.remaining, 2); // unchanged
    }

    #[test]
    fn client_count_tracks_unique() {
        let mut limiter = RateLimiter::new(fast_config(10, 60_000));
        assert_eq!(limiter.client_count(), 0);

        limiter.check("a");
        assert_eq!(limiter.client_count(), 1);

        limiter.check("b");
        assert_eq!(limiter.client_count(), 2);

        limiter.check("a"); // same client
        assert_eq!(limiter.client_count(), 2);
    }

    #[test]
    fn cleanup_removes_expired() {
        let mut limiter = RateLimiter::new(fast_config(5, 1)); // 1ms window
        limiter.check("temp");
        assert_eq!(limiter.client_count(), 1);

        std::thread::sleep(Duration::from_millis(5));
        limiter.cleanup();
        assert_eq!(limiter.client_count(), 0);
    }

    #[test]
    fn reset_secs_positive_when_active() {
        let mut limiter = RateLimiter::new(fast_config(5, 60_000)); // 60s window
        let result = limiter.check("client");
        assert!(result.allowed);
        // Reset should be close to 60 (but timing is imprecise)
        assert!(result.reset_secs <= 60);
    }

    #[test]
    fn result_serializes_to_json() {
        let result = RateLimitResult {
            allowed: true,
            remaining: 42,
            limit: 100,
            reset_secs: 30,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"allowed\":true"));
        assert!(json.contains("\"remaining\":42"));
        assert!(json.contains("\"limit\":100"));
    }

    #[test]
    fn default_config_values() {
        let cfg = RateLimitConfig::default_config();
        assert_eq!(cfg.max_requests, 100);
        assert_eq!(cfg.window, Duration::from_secs(60));
        assert!(cfg.enabled);
    }

    #[test]
    fn single_request_limit() {
        let mut limiter = RateLimiter::new(fast_config(1, 60_000));
        assert!(limiter.check("client").allowed);
        assert!(!limiter.check("client").allowed);
    }

    #[test]
    fn remaining_decrements_correctly() {
        let mut limiter = RateLimiter::new(fast_config(5, 60_000));
        assert_eq!(limiter.check("c").remaining, 4);
        assert_eq!(limiter.check("c").remaining, 3);
        assert_eq!(limiter.check("c").remaining, 2);
        assert_eq!(limiter.check("c").remaining, 1);
        assert_eq!(limiter.check("c").remaining, 0);
        // Next should be denied
        let denied = limiter.check("c");
        assert!(!denied.allowed);
        assert_eq!(denied.remaining, 0);
    }

    // ── TenantRateLimiter tests ───────────────────────────────────────────

    #[test]
    fn tenant_limiter_starter_quota() {
        let quotas = TenantQuotas::for_plan(&TenantPlan::Starter);
        assert_eq!(quotas.requests_per_min, 60);
        assert_eq!(quotas.tokens_per_day, 100_000);
    }

    #[test]
    fn tenant_limiter_pro_quota() {
        let quotas = TenantQuotas::for_plan(&TenantPlan::Pro);
        assert_eq!(quotas.requests_per_min, 300);
        assert_eq!(quotas.tokens_per_day, 1_000_000);
    }

    #[test]
    fn tenant_limiter_enterprise_unlimited_tokens() {
        let quotas = TenantQuotas::for_plan(&TenantPlan::Enterprise);
        assert_eq!(quotas.tokens_per_day, u64::MAX);
    }

    #[test]
    fn tenant_limiter_check_request_allowed() {
        let mut trl = TenantRateLimiter::new();
        let result = trl.check_request("acme", &TenantPlan::Pro);
        assert!(result.allowed);
    }

    #[test]
    fn tenant_limiter_two_tenants_independent() {
        let mut trl = TenantRateLimiter::new();
        // Exhaust starter quota for tenant-a (60 req/min = 60 requests)
        // We use fast_config equivalent via Starter but override to small limit
        // Instead, just verify they start independent
        let r_a = trl.check_request("tenant-a", &TenantPlan::Starter);
        let r_b = trl.check_request("tenant-b", &TenantPlan::Starter);
        assert!(r_a.allowed);
        assert!(r_b.allowed);
        assert_eq!(trl.tenant_count(), 2);
    }

    #[test]
    fn tenant_limiter_token_tracking() {
        let mut trl = TenantRateLimiter::new();
        trl.record_tokens("acme", 50_000);
        let (used, limit) = trl.token_usage("acme", &TenantPlan::Starter);
        assert_eq!(used, 50_000);
        assert_eq!(limit, 100_000);
    }

    #[test]
    fn tenant_limiter_token_quota_check() {
        let mut trl = TenantRateLimiter::new();
        // Under quota
        assert!(trl.check_token_quota("acme", &TenantPlan::Starter));
        // Exhaust quota
        trl.record_tokens("acme", 100_001);
        assert!(!trl.check_token_quota("acme", &TenantPlan::Starter));
    }

    #[test]
    fn tenant_limiter_enterprise_token_quota_always_ok() {
        let mut trl = TenantRateLimiter::new();
        trl.record_tokens("big-corp", u64::MAX / 2);
        assert!(trl.check_token_quota("big-corp", &TenantPlan::Enterprise));
    }
}
