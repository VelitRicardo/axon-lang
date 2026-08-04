//! Request Middleware — automatic request ID, timing, and logging for AxonServer.
//!
//! Provides an axum middleware layer that intercepts every request to:
//!   - Generate a unique sequential request ID (`X-Request-Id` header)
//!   - Time the request duration (start → response)
//!   - Auto-record to the RequestLogger (method, path, status, latency, client key)
//!   - Tag slow requests above a configurable threshold
//!
//! This replaces manual `request_logger.record()` calls in individual handlers
//! and provides consistent observability across all endpoints.
//!
//! Configuration:
//!   - `MiddlewareConfig` — enabled flag, slow request threshold
//!   - `GET /v1/middleware` — view current middleware configuration
//!   - `PUT /v1/middleware` — update middleware settings at runtime

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(feature = "server")]
use std::time::Instant;

// §Fase 118.b.2 — the framework types are needed by exactly one item in this
// module: `request_middleware_fn`, which is an `axum::middleware::from_fn`
// handler and could not be anything else. Everything above it —
// `RequestIdGenerator`, `MiddlewareConfig`, `MiddlewareUpdate`, `apply_update`,
// `RequestMeta`, `MiddlewareStats` — is configuration and counters, and stays in
// every build with its tests.
#[cfg(feature = "server")]
use axum::body::Body;
#[cfg(feature = "server")]
use axum::extract::Request;
#[cfg(feature = "server")]
use axum::middleware::Next;
#[cfg(feature = "server")]
use axum::response::Response;
// `HeaderValue` / `HeaderMap` are `http` crate types that axum re-exports —
// used only by the middleware fn and its test, hence the same gate.
#[cfg(feature = "server")]
use http::HeaderValue;
use serde::{Deserialize, Serialize};

// ── Request ID generator ────────────────────────────────────────────────

/// Atomic sequential request ID generator.
///
/// Produces unique IDs of the form `axr-{counter}` for each request.
/// Counter is monotonically increasing and never resets during server lifetime.
pub struct RequestIdGenerator {
    counter: AtomicU64,
    prefix: String,
}

impl RequestIdGenerator {
    /// Create a new generator with the default prefix "axr".
    pub fn new() -> Self {
        RequestIdGenerator {
            counter: AtomicU64::new(0),
            prefix: "axr".to_string(),
        }
    }

    /// Create a generator with a custom prefix.
    pub fn with_prefix(prefix: &str) -> Self {
        RequestIdGenerator {
            counter: AtomicU64::new(0),
            prefix: prefix.to_string(),
        }
    }

    /// Generate the next request ID.
    pub fn next_id(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("{}-{}", self.prefix, n)
    }

    /// Current counter value (number of IDs generated).
    pub fn count(&self) -> u64 {
        self.counter.load(Ordering::Relaxed)
    }
}

impl Default for RequestIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

// ── Middleware configuration ─────────────────────────────────────────────

/// Configuration for the request middleware layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiddlewareConfig {
    /// Whether the middleware is enabled.
    pub enabled: bool,
    /// Slow request threshold in milliseconds. Requests exceeding this
    /// are tagged in the log. 0 = disabled.
    pub slow_threshold_ms: u64,
    /// Whether to inject X-Request-Id response header.
    pub inject_request_id: bool,
    /// Whether to inject X-Response-Time header (latency in ms).
    pub inject_response_time: bool,
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        MiddlewareConfig {
            enabled: true,
            slow_threshold_ms: 5000,
            inject_request_id: true,
            inject_response_time: true,
        }
    }
}

impl MiddlewareConfig {
    /// Disabled middleware — passes through without recording.
    pub fn disabled() -> Self {
        MiddlewareConfig {
            enabled: false,
            slow_threshold_ms: 0,
            inject_request_id: false,
            inject_response_time: false,
        }
    }
}

// ── Middleware update ────────────────────────────────────────────────────

/// Partial update for middleware configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct MiddlewareUpdate {
    pub enabled: Option<bool>,
    pub slow_threshold_ms: Option<u64>,
    pub inject_request_id: Option<bool>,
    pub inject_response_time: Option<bool>,
}

/// Apply a partial update to a MiddlewareConfig. Returns list of changed fields.
pub fn apply_update(config: &mut MiddlewareConfig, update: &MiddlewareUpdate) -> Vec<String> {
    let mut changes = Vec::new();

    if let Some(enabled) = update.enabled {
        if enabled != config.enabled {
            config.enabled = enabled;
            changes.push("enabled".to_string());
        }
    }
    if let Some(threshold) = update.slow_threshold_ms {
        if threshold != config.slow_threshold_ms {
            config.slow_threshold_ms = threshold;
            changes.push("slow_threshold_ms".to_string());
        }
    }
    if let Some(inject_id) = update.inject_request_id {
        if inject_id != config.inject_request_id {
            config.inject_request_id = inject_id;
            changes.push("inject_request_id".to_string());
        }
    }
    if let Some(inject_time) = update.inject_response_time {
        if inject_time != config.inject_response_time {
            config.inject_response_time = inject_time;
            changes.push("inject_response_time".to_string());
        }
    }

    changes
}

// ── Request metadata ────────────────────────────────────────────────────

/// Metadata captured for a single request by the middleware.
#[derive(Debug, Clone, Serialize)]
pub struct RequestMeta {
    /// Unique request ID (e.g., "axr-42").
    pub request_id: String,
    /// HTTP method.
    pub method: String,
    /// Request path.
    pub path: String,
    /// Response status code.
    pub status: u16,
    /// Latency in microseconds.
    pub latency_us: u64,
    /// Latency in milliseconds (convenience).
    pub latency_ms: u64,
    /// Client identifier.
    pub client_key: String,
    /// Whether this was flagged as a slow request.
    pub slow: bool,
}

// ── Middleware state ─────────────────────────────────────────────────────

/// Shared state for the request middleware, held in an Arc for cloning.
pub struct MiddlewareState<S> {
    pub id_generator: RequestIdGenerator,
    pub config: Arc<Mutex<MiddlewareConfig>>,
    pub server_state: Arc<Mutex<S>>,
}

// ── Helper: extract client key from headers ─────────────────────────────

#[cfg(feature = "server")]
fn client_key_from_headers(headers: &http::HeaderMap) -> String {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string())
        .unwrap_or_else(|| "anonymous".to_string())
}

// ── Core middleware function ─────────────────────────────────────────────

/// The request middleware function for use with `axum::middleware::from_fn`.
///
/// Extracts method/path/client, generates request ID, times the request,
/// records to the RequestLogger, and injects response headers.
///
/// This is designed to be used with `axum::middleware::from_fn` in the
/// router setup. The ServerState access is done via the shared state
/// that axum provides.
#[cfg(feature = "server")]
pub async fn request_middleware_fn(
    state: axum::extract::State<Arc<Mutex<crate::axon_server::ServerState>>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let start = Instant::now();

    // Extract request info before passing to handler
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let client_key = client_key_from_headers(request.headers());

    // Read config and generate ID
    let (enabled, slow_threshold_ms, inject_id, inject_time, request_id) = {
        let s = state.lock().unwrap();
        let cfg = &s.middleware_config;
        let id = s.request_id_gen.next_id();
        (cfg.enabled, cfg.slow_threshold_ms, cfg.inject_request_id, cfg.inject_response_time, id)
    };

    // Call the actual handler
    let mut response = next.run(request).await;

    if !enabled {
        return response;
    }

    // Compute latency
    let elapsed = start.elapsed();
    let _latency_us = elapsed.as_micros() as u64;
    let latency_ms = elapsed.as_millis() as u64;
    let status = response.status().as_u16();
    let _slow = slow_threshold_ms > 0 && latency_ms >= slow_threshold_ms;

    // Record to request logger
    {
        let mut s = state.lock().unwrap();
        s.request_logger.record(&method, &path, status, elapsed, &client_key);
    }

    // Inject response headers
    if inject_id {
        if let Ok(val) = HeaderValue::from_str(&request_id) {
            response.headers_mut().insert("x-request-id", val);
        }
    }
    if inject_time {
        if let Ok(val) = HeaderValue::from_str(&format!("{}ms", latency_ms)) {
            response.headers_mut().insert("x-response-time", val);
        }
    }

    response
}

// ── Stats ───────────────────────────────────────────────────────────────

/// Middleware statistics snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct MiddlewareStats {
    /// Total requests processed by the middleware.
    pub total_requests: u64,
    /// Current configuration.
    pub config: MiddlewareConfig,
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_id_generator_sequential() {
        let gen = RequestIdGenerator::new();
        assert_eq!(gen.next_id(), "axr-0");
        assert_eq!(gen.next_id(), "axr-1");
        assert_eq!(gen.next_id(), "axr-2");
        assert_eq!(gen.count(), 3);
    }

    #[test]
    fn request_id_generator_custom_prefix() {
        let gen = RequestIdGenerator::with_prefix("req");
        assert_eq!(gen.next_id(), "req-0");
        assert_eq!(gen.next_id(), "req-1");
    }

    #[test]
    fn request_id_generator_default() {
        let gen = RequestIdGenerator::default();
        assert_eq!(gen.next_id(), "axr-0");
    }

    #[test]
    fn default_config() {
        let cfg = MiddlewareConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.slow_threshold_ms, 5000);
        assert!(cfg.inject_request_id);
        assert!(cfg.inject_response_time);
    }

    #[test]
    fn disabled_config() {
        let cfg = MiddlewareConfig::disabled();
        assert!(!cfg.enabled);
        assert_eq!(cfg.slow_threshold_ms, 0);
        assert!(!cfg.inject_request_id);
        assert!(!cfg.inject_response_time);
    }

    #[test]
    fn config_serializable() {
        let cfg = MiddlewareConfig::default();
        let json = serde_json::to_value(&cfg).unwrap();
        assert_eq!(json["enabled"], true);
        assert_eq!(json["slow_threshold_ms"], 5000);
        assert_eq!(json["inject_request_id"], true);
        assert_eq!(json["inject_response_time"], true);
    }

    #[test]
    fn config_deserializable() {
        let json = serde_json::json!({
            "enabled": false,
            "slow_threshold_ms": 1000,
            "inject_request_id": false,
            "inject_response_time": true,
        });
        let cfg: MiddlewareConfig = serde_json::from_value(json).unwrap();
        assert!(!cfg.enabled);
        assert_eq!(cfg.slow_threshold_ms, 1000);
        assert!(!cfg.inject_request_id);
        assert!(cfg.inject_response_time);
    }

    #[test]
    fn apply_update_changes_tracked() {
        let mut cfg = MiddlewareConfig::default();
        let update = MiddlewareUpdate {
            enabled: None,
            slow_threshold_ms: Some(2000),
            inject_request_id: Some(false),
            inject_response_time: None,
        };
        let changes = apply_update(&mut cfg, &update);
        assert_eq!(changes.len(), 2);
        assert!(changes.contains(&"slow_threshold_ms".to_string()));
        assert!(changes.contains(&"inject_request_id".to_string()));
        assert_eq!(cfg.slow_threshold_ms, 2000);
        assert!(!cfg.inject_request_id);
    }

    #[test]
    fn apply_update_no_op_when_same() {
        let mut cfg = MiddlewareConfig::default();
        let update = MiddlewareUpdate {
            enabled: Some(true),
            slow_threshold_ms: Some(5000),
            inject_request_id: Some(true),
            inject_response_time: Some(true),
        };
        let changes = apply_update(&mut cfg, &update);
        assert!(changes.is_empty());
    }

    #[test]
    fn apply_update_all_fields() {
        let mut cfg = MiddlewareConfig::default();
        let update = MiddlewareUpdate {
            enabled: Some(false),
            slow_threshold_ms: Some(100),
            inject_request_id: Some(false),
            inject_response_time: Some(false),
        };
        let changes = apply_update(&mut cfg, &update);
        assert_eq!(changes.len(), 4);
        assert!(!cfg.enabled);
        assert_eq!(cfg.slow_threshold_ms, 100);
        assert!(!cfg.inject_request_id);
        assert!(!cfg.inject_response_time);
    }

    #[test]
    fn request_meta_serializable() {
        let meta = RequestMeta {
            request_id: "axr-42".to_string(),
            method: "POST".to_string(),
            path: "/v1/deploy".to_string(),
            status: 200,
            latency_us: 1500,
            latency_ms: 1,
            client_key: "token_abc".to_string(),
            slow: false,
        };
        let json = serde_json::to_value(&meta).unwrap();
        assert_eq!(json["request_id"], "axr-42");
        assert_eq!(json["method"], "POST");
        assert_eq!(json["path"], "/v1/deploy");
        assert_eq!(json["status"], 200);
        assert_eq!(json["latency_us"], 1500);
        assert_eq!(json["slow"], false);
    }

    #[test]
    fn request_meta_slow_flag() {
        let meta = RequestMeta {
            request_id: "axr-99".to_string(),
            method: "GET".to_string(),
            path: "/v1/health".to_string(),
            status: 200,
            latency_us: 6_000_000,
            latency_ms: 6000,
            client_key: "anonymous".to_string(),
            slow: true,
        };
        let json = serde_json::to_value(&meta).unwrap();
        assert_eq!(json["slow"], true);
        assert_eq!(json["latency_ms"], 6000);
    }

    #[test]
    fn middleware_stats_serializable() {
        let stats = MiddlewareStats {
            total_requests: 42,
            config: MiddlewareConfig::default(),
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["total_requests"], 42);
        assert_eq!(json["config"]["enabled"], true);
    }

    // §Fase 118.b.2 — follows `client_key_from_headers` behind the `server`
    // feature. THIS is the §117.a trap, caught exactly where the plan said it
    // would be: an inline test calling a gated function. `--lib --bins` is
    // perfectly happy with it; `--all-targets` is what fails.
    #[cfg(feature = "server")]
    #[test]
    fn client_key_extraction() {
        let mut headers = http::HeaderMap::new();
        assert_eq!(client_key_from_headers(&headers), "anonymous");

        headers.insert("authorization", HeaderValue::from_static("Bearer token123"));
        assert_eq!(client_key_from_headers(&headers), "Bearer token123");
    }
}
