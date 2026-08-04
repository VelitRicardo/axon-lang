//! CORS Middleware — Cross-Origin Resource Sharing configuration for AxonServer.
//!
//! Provides configurable CORS policies so web clients (browsers) can
//! interact with the AxonServer API from different origins.
//!
//! Configuration:
//!   - `CorsConfig` — allowed origins, methods, headers, max age, credentials
//!   - `build_cors_layer()` — converts CorsConfig into a tower-http CorsLayer
//!
//! Defaults are permissive for development (allow all origins).
//! Production deployments should restrict `allowed_origins` to specific domains.
//!
//! Runtime adjustment via:
//!   - `GET /v1/cors` — view current CORS configuration
//!   - `PUT /v1/cors` — update CORS configuration (requires server restart to take effect)

use serde::{Deserialize, Serialize};
// §Fase 118.b.2 — the CONFIG half of this module (`CorsConfig`, `CorsUpdate`,
// `apply_update` — §83's "CORS as an endpoint property") is pure data and stays
// in every build; only `build_cors_layer`, which materialises a `tower-http`
// `CorsLayer`, needs the `server` feature. `HeaderName`/`Method` come from the
// `http` crate directly rather than through `axum::http::…` — same types, one
// fewer coupling.
#[cfg(feature = "server")]
use http::{HeaderName, Method};
#[cfg(feature = "server")]
use tower_http::cors::{CorsLayer, Any};

// ── Configuration ───────────────────────────────────────────────────────

/// CORS configuration for the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsConfig {
    /// Allowed origins. Empty or ["*"] means allow all.
    pub allowed_origins: Vec<String>,
    /// Allowed HTTP methods.
    pub allowed_methods: Vec<String>,
    /// Allowed request headers.
    pub allowed_headers: Vec<String>,
    /// Whether to allow credentials (cookies, auth headers).
    pub allow_credentials: bool,
    /// Max age for preflight cache (seconds). 0 = no cache.
    pub max_age_secs: u64,
    /// Whether CORS is enabled at all.
    pub enabled: bool,
}

impl Default for CorsConfig {
    fn default() -> Self {
        CorsConfig {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
                "OPTIONS".to_string(),
            ],
            allowed_headers: vec![
                "Content-Type".to_string(),
                "Authorization".to_string(),
                "X-Axon-Signature".to_string(),
            ],
            allow_credentials: false,
            max_age_secs: 3600,
            enabled: true,
        }
    }
}

impl CorsConfig {
    /// Create a restrictive config that only allows specific origins.
    pub fn restricted(origins: Vec<String>) -> Self {
        CorsConfig {
            allowed_origins: origins,
            allow_credentials: true,
            ..Default::default()
        }
    }

    /// Check if a wildcard ("*") is in allowed origins.
    pub fn is_permissive(&self) -> bool {
        self.allowed_origins.iter().any(|o| o == "*")
    }
}

// ── Layer builder ───────────────────────────────────────────────────────

/// Build a tower-http CorsLayer from a CorsConfig.
///
/// If CORS is disabled, returns a permissive no-op layer (allows everything).
/// This ensures the middleware is always present in the stack for consistency.
///
/// §Fase 118.b.2 — the one function in this module that needs the `server`
/// feature: a `CorsLayer` is a `tower-http` service, and there is no router to
/// mount it on without one. The §83 declaration (`cors <Name>` on an
/// `axonendpoint`) still type-checks in a `cli` build — `axon check` validates
/// the policy; only serving it needs the layer.
#[cfg(feature = "server")]
pub fn build_cors_layer(config: &CorsConfig) -> CorsLayer {
    if !config.enabled {
        // Disabled: allow everything (no restrictions)
        return CorsLayer::permissive();
    }

    let mut layer = CorsLayer::new();

    // Origins
    if config.is_permissive() {
        layer = layer.allow_origin(Any);
    } else {
        let origins: Vec<http::HeaderValue> = config.allowed_origins.iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        layer = layer.allow_origin(origins);
    }

    // Methods
    let methods: Vec<Method> = config.allowed_methods.iter()
        .filter_map(|m| m.parse().ok())
        .collect();
    layer = layer.allow_methods(methods);

    // Headers
    let headers: Vec<HeaderName> = config.allowed_headers.iter()
        .filter_map(|h| h.parse().ok())
        .collect();
    layer = layer.allow_headers(headers);

    // Max age
    if config.max_age_secs > 0 {
        layer = layer.max_age(std::time::Duration::from_secs(config.max_age_secs));
    }

    layer
}

// ── Update struct ───────────────────────────────────────────────────────

/// Partial update for CORS configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct CorsUpdate {
    pub allowed_origins: Option<Vec<String>>,
    pub allowed_methods: Option<Vec<String>>,
    pub allowed_headers: Option<Vec<String>>,
    pub allow_credentials: Option<bool>,
    pub max_age_secs: Option<u64>,
    pub enabled: Option<bool>,
}

/// Apply a partial update to a CorsConfig. Returns list of changed fields.
pub fn apply_update(config: &mut CorsConfig, update: &CorsUpdate) -> Vec<String> {
    let mut changes = Vec::new();

    if let Some(ref origins) = update.allowed_origins {
        if *origins != config.allowed_origins {
            config.allowed_origins = origins.clone();
            changes.push("allowed_origins".to_string());
        }
    }
    if let Some(ref methods) = update.allowed_methods {
        if *methods != config.allowed_methods {
            config.allowed_methods = methods.clone();
            changes.push("allowed_methods".to_string());
        }
    }
    if let Some(ref headers) = update.allowed_headers {
        if *headers != config.allowed_headers {
            config.allowed_headers = headers.clone();
            changes.push("allowed_headers".to_string());
        }
    }
    if let Some(creds) = update.allow_credentials {
        if creds != config.allow_credentials {
            config.allow_credentials = creds;
            changes.push("allow_credentials".to_string());
        }
    }
    if let Some(age) = update.max_age_secs {
        if age != config.max_age_secs {
            config.max_age_secs = age;
            changes.push("max_age_secs".to_string());
        }
    }
    if let Some(enabled) = update.enabled {
        if enabled != config.enabled {
            config.enabled = enabled;
            changes.push("enabled".to_string());
        }
    }

    changes
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = CorsConfig::default();
        assert!(config.enabled);
        assert!(config.is_permissive());
        assert_eq!(config.allowed_origins, vec!["*"]);
        assert_eq!(config.allowed_methods.len(), 5);
        assert_eq!(config.allowed_headers.len(), 3);
        assert!(!config.allow_credentials);
        assert_eq!(config.max_age_secs, 3600);
    }

    #[test]
    fn restricted_config() {
        let config = CorsConfig::restricted(vec!["https://app.example.com".into()]);
        assert!(!config.is_permissive());
        assert!(config.allow_credentials);
        assert_eq!(config.allowed_origins, vec!["https://app.example.com"]);
    }

    #[test]
    fn config_serializable() {
        let config = CorsConfig::default();
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["enabled"], true);
        assert_eq!(json["allowed_origins"][0], "*");
        assert_eq!(json["max_age_secs"], 3600);
        assert!(json["allowed_methods"].is_array());
        assert!(json["allowed_headers"].is_array());
    }

    #[test]
    fn config_deserializable() {
        let json = serde_json::json!({
            "allowed_origins": ["https://example.com"],
            "allowed_methods": ["GET", "POST"],
            "allowed_headers": ["Content-Type"],
            "allow_credentials": true,
            "max_age_secs": 600,
            "enabled": true,
        });
        let config: CorsConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.allowed_origins, vec!["https://example.com"]);
        assert_eq!(config.allowed_methods.len(), 2);
        assert!(config.allow_credentials);
        assert_eq!(config.max_age_secs, 600);
    }

    // §Fase 118.b.2 — the three layer-builder tests follow their subject behind
    // the `server` feature. §117.a's lesson, paid for once already: an inline
    // test that calls a gated function is exactly what makes
    // `--no-default-features` fail to compile while `--lib --bins` says it is
    // fine. Verified here with `--all-targets`.
    #[cfg(feature = "server")]
    #[test]
    fn build_layer_permissive() {
        let config = CorsConfig::default();
        let _layer = build_cors_layer(&config);
        // Just verify it builds without panic
    }

    #[cfg(feature = "server")]
    #[test]
    fn build_layer_restricted() {
        let config = CorsConfig::restricted(vec!["https://app.example.com".into()]);
        let _layer = build_cors_layer(&config);
    }

    #[cfg(feature = "server")]
    #[test]
    fn build_layer_disabled() {
        let mut config = CorsConfig::default();
        config.enabled = false;
        let _layer = build_cors_layer(&config);
    }

    #[test]
    fn apply_update_changes_tracked() {
        let mut config = CorsConfig::default();
        let update = CorsUpdate {
            allowed_origins: Some(vec!["https://example.com".into()]),
            max_age_secs: Some(600),
            enabled: None,
            allowed_methods: None,
            allowed_headers: None,
            allow_credentials: None,
        };

        let changes = apply_update(&mut config, &update);
        assert_eq!(changes.len(), 2);
        assert!(changes.contains(&"allowed_origins".to_string()));
        assert!(changes.contains(&"max_age_secs".to_string()));
        assert_eq!(config.allowed_origins, vec!["https://example.com"]);
        assert_eq!(config.max_age_secs, 600);
    }

    #[test]
    fn apply_update_no_op_when_same() {
        let mut config = CorsConfig::default();
        let update = CorsUpdate {
            allowed_origins: Some(vec!["*".into()]),
            max_age_secs: Some(3600),
            enabled: Some(true),
            allowed_methods: None,
            allowed_headers: None,
            allow_credentials: None,
        };

        let changes = apply_update(&mut config, &update);
        assert!(changes.is_empty());
    }

    #[test]
    fn apply_update_all_fields() {
        let mut config = CorsConfig::default();
        let update = CorsUpdate {
            allowed_origins: Some(vec!["https://a.com".into(), "https://b.com".into()]),
            allowed_methods: Some(vec!["GET".into()]),
            allowed_headers: Some(vec!["Authorization".into()]),
            allow_credentials: Some(true),
            max_age_secs: Some(120),
            enabled: Some(false),
        };

        let changes = apply_update(&mut config, &update);
        assert_eq!(changes.len(), 6);
        assert!(!config.enabled);
        assert!(config.allow_credentials);
        assert_eq!(config.allowed_origins.len(), 2);
        assert_eq!(config.allowed_methods.len(), 1);
        assert_eq!(config.allowed_headers.len(), 1);
        assert_eq!(config.max_age_secs, 120);
    }

    #[test]
    fn is_permissive_true_with_wildcard() {
        let config = CorsConfig::default();
        assert!(config.is_permissive());
    }

    #[test]
    fn is_permissive_false_without_wildcard() {
        let config = CorsConfig::restricted(vec!["https://example.com".into()]);
        assert!(!config.is_permissive());
    }
}
