//! Auth Middleware — role-based authentication gate for AxonServer.
//!
//! Replaces the simple single-token `check_auth` with ApiKeyManager-backed
//! validation that enforces role-based access control on all protected endpoints.
//!
//! Endpoint access levels:
//!   - Public:   no auth required (health, version, rate-limit)
//!   - ReadOnly: any valid key (metrics, list daemons, logs, keys list, session reads)
//!   - Write:    Operator or Admin (deploy, estimate, events, supervisor control, session writes)
//!   - Admin:    Admin only (key management: create, revoke, rotate)
//!
//! When ApiKeyManager is disabled (no auth_token configured), all requests pass.
//! When enabled, Bearer token is validated against the key registry and role checked.

use crate::api_keys::{ApiKeyManager, KeyRole, ValidationResult};
// §Fase 118.b.2 — `http::{HeaderMap, StatusCode}`, not `axum::http::{…}`. Same
// types: axum only re-exports the `http` crate. This module validates API keys
// and maps them to an `AccessLevel` — it needs header types, not a web
// framework, and naming the real source keeps it OUT of the `server` feature
// entirely. The §118.a treatment of `auth_scope.rs`, applied again: a module
// that turns out not to need axum is worth more than a module that is gated.
use http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};

// ── Access levels ───────────────────────────────────────────────────────

/// Required access level for an endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccessLevel {
    /// No authentication required.
    Public,
    /// Any valid key (Admin, Operator, or ReadOnly).
    ReadOnly,
    /// Operator or Admin — write operations.
    Write,
    /// Admin only — key management.
    Admin,
}

impl AccessLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            AccessLevel::Public => "public",
            AccessLevel::ReadOnly => "readonly",
            AccessLevel::Write => "write",
            AccessLevel::Admin => "admin",
        }
    }
}

// ── Auth result ─────────────────────────────────────────────────────────

/// Result of an authentication check.
#[derive(Debug, Clone)]
pub struct AuthResult {
    /// Whether the request is authorized.
    pub authorized: bool,
    /// The key name that was used (if any).
    pub key_name: Option<String>,
    /// The role of the key (if any).
    pub role: Option<KeyRole>,
    /// Per-key rate limit override (if any).
    pub rate_limit: Option<u32>,
}

impl AuthResult {
    fn allowed(v: &ValidationResult) -> Self {
        AuthResult {
            authorized: true,
            key_name: v.key_name.clone(),
            role: v.role,
            rate_limit: v.rate_limit,
        }
    }

    fn public() -> Self {
        AuthResult {
            authorized: true,
            key_name: None,
            role: None,
            rate_limit: None,
        }
    }
}

// ── Token extraction ────────────────────────────────────────────────────

/// Extract Bearer token from Authorization header.
fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

// ── Auth gate ───────────────────────────────────────────────────────────

/// Check authentication and authorization for a request.
///
/// - If `api_keys` is disabled, all requests pass (backwards compat).
/// - For `Public` endpoints, always passes.
/// - For `ReadOnly` endpoints, any valid key is sufficient.
/// - For `Write` endpoints, key must have `can_write()` (Operator or Admin).
/// - For `Admin` endpoints, key must have `can_manage_keys()` (Admin only).
///
/// Returns `Ok(AuthResult)` if authorized, `Err(StatusCode)` if not.
pub fn check(
    api_keys: &mut ApiKeyManager,
    headers: &HeaderMap,
    level: AccessLevel,
) -> Result<AuthResult, StatusCode> {
    // Public endpoints always pass
    if level == AccessLevel::Public {
        return Ok(AuthResult::public());
    }

    // If key management is disabled, all requests pass (no auth configured)
    if !api_keys.is_enabled() {
        return Ok(AuthResult::public());
    }

    // Extract Bearer token
    let token = match extract_bearer(headers) {
        Some(t) => t,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    // Validate token
    let validation = api_keys.validate(token);
    if !validation.valid {
        return Err(StatusCode::FORBIDDEN);
    }

    // Check role permissions
    let role = validation.role.unwrap_or(KeyRole::ReadOnly);
    match level {
        AccessLevel::Public => Ok(AuthResult::allowed(&validation)),
        AccessLevel::ReadOnly => {
            // Any valid key can read
            Ok(AuthResult::allowed(&validation))
        }
        AccessLevel::Write => {
            if role.can_write() {
                Ok(AuthResult::allowed(&validation))
            } else {
                Err(StatusCode::FORBIDDEN)
            }
        }
        AccessLevel::Admin => {
            if role.can_manage_keys() {
                Ok(AuthResult::allowed(&validation))
            } else {
                Err(StatusCode::FORBIDDEN)
            }
        }
    }
}

/// Check authentication without recording usage (for peek/status endpoints).
pub fn peek(
    api_keys: &ApiKeyManager,
    headers: &HeaderMap,
    level: AccessLevel,
) -> Result<AuthResult, StatusCode> {
    if level == AccessLevel::Public {
        return Ok(AuthResult::public());
    }

    if !api_keys.is_enabled() {
        return Ok(AuthResult::public());
    }

    let token = match extract_bearer(headers) {
        Some(t) => t,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    let validation = api_keys.peek(token);
    if !validation.valid {
        return Err(StatusCode::FORBIDDEN);
    }

    let role = validation.role.unwrap_or(KeyRole::ReadOnly);
    match level {
        AccessLevel::Public | AccessLevel::ReadOnly => Ok(AuthResult::allowed(&validation)),
        AccessLevel::Write => {
            if role.can_write() {
                Ok(AuthResult::allowed(&validation))
            } else {
                Err(StatusCode::FORBIDDEN)
            }
        }
        AccessLevel::Admin => {
            if role.can_manage_keys() {
                Ok(AuthResult::allowed(&validation))
            } else {
                Err(StatusCode::FORBIDDEN)
            }
        }
    }
}

/// Classify an endpoint path + method into an AccessLevel.
pub fn classify_endpoint(method: &str, path: &str) -> AccessLevel {
    // Public endpoints — no auth required
    if path.starts_with("/v1/health") || path == "/v1/version" || path == "/v1/rate-limit" {
        return AccessLevel::Public;
    }

    // Admin endpoints — key management writes
    if path.starts_with("/v1/keys") && method != "GET" {
        return AccessLevel::Admin;
    }

    // Write endpoints
    match method {
        "POST" | "PUT" | "DELETE" | "PATCH" => AccessLevel::Write,
        _ => AccessLevel::ReadOnly,
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_keys::ApiKeyManager;

    fn make_headers(token: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(t) = token {
            h.insert("authorization", format!("Bearer {t}").parse().unwrap());
        }
        h
    }

    #[test]
    fn public_always_passes() {
        let mut mgr = ApiKeyManager::new(Some("master"));
        let h = make_headers(None);
        let result = check(&mut mgr, &h, AccessLevel::Public).unwrap();
        assert!(result.authorized);
        assert!(result.key_name.is_none());
    }

    #[test]
    fn disabled_manager_allows_all() {
        let mut mgr = ApiKeyManager::new(None);
        let h = make_headers(None);

        assert!(check(&mut mgr, &h, AccessLevel::ReadOnly).is_ok());
        assert!(check(&mut mgr, &h, AccessLevel::Write).is_ok());
        assert!(check(&mut mgr, &h, AccessLevel::Admin).is_ok());
    }

    #[test]
    fn missing_token_returns_unauthorized() {
        let mut mgr = ApiKeyManager::new(Some("master"));
        let h = make_headers(None);

        assert_eq!(check(&mut mgr, &h, AccessLevel::ReadOnly).unwrap_err(), StatusCode::UNAUTHORIZED);
        assert_eq!(check(&mut mgr, &h, AccessLevel::Write).unwrap_err(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn invalid_token_returns_forbidden() {
        let mut mgr = ApiKeyManager::new(Some("master"));
        let h = make_headers(Some("wrong_token"));

        assert_eq!(check(&mut mgr, &h, AccessLevel::ReadOnly).unwrap_err(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn admin_key_has_full_access() {
        let mut mgr = ApiKeyManager::new(Some("admin_tok"));
        let h = make_headers(Some("admin_tok"));

        let r = check(&mut mgr, &h, AccessLevel::ReadOnly).unwrap();
        assert!(r.authorized);
        assert_eq!(r.role, Some(KeyRole::Admin));

        let r = check(&mut mgr, &h, AccessLevel::Write).unwrap();
        assert!(r.authorized);

        let r = check(&mut mgr, &h, AccessLevel::Admin).unwrap();
        assert!(r.authorized);
    }

    #[test]
    fn operator_can_write_not_admin() {
        let mut mgr = ApiKeyManager::new(Some("master"));
        mgr.create_key("op", "op_tok", KeyRole::Operator, None);
        let h = make_headers(Some("op_tok"));

        assert!(check(&mut mgr, &h, AccessLevel::ReadOnly).is_ok());
        assert!(check(&mut mgr, &h, AccessLevel::Write).is_ok());
        assert_eq!(check(&mut mgr, &h, AccessLevel::Admin).unwrap_err(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn readonly_can_only_read() {
        let mut mgr = ApiKeyManager::new(Some("master"));
        mgr.create_key("viewer", "view_tok", KeyRole::ReadOnly, None);
        let h = make_headers(Some("view_tok"));

        assert!(check(&mut mgr, &h, AccessLevel::ReadOnly).is_ok());
        assert_eq!(check(&mut mgr, &h, AccessLevel::Write).unwrap_err(), StatusCode::FORBIDDEN);
        assert_eq!(check(&mut mgr, &h, AccessLevel::Admin).unwrap_err(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn check_records_usage() {
        let mut mgr = ApiKeyManager::new(Some("master"));
        mgr.create_key("svc", "svc_tok", KeyRole::Operator, None);
        let h = make_headers(Some("svc_tok"));

        check(&mut mgr, &h, AccessLevel::ReadOnly).unwrap();
        check(&mut mgr, &h, AccessLevel::ReadOnly).unwrap();
        check(&mut mgr, &h, AccessLevel::Write).unwrap();

        let list = mgr.list();
        let key = list.iter().find(|k| k.name == "svc").unwrap();
        assert_eq!(key.request_count, 3);
        assert!(key.last_used.is_some());
    }

    #[test]
    fn peek_does_not_record_usage() {
        let mut mgr = ApiKeyManager::new(Some("master"));
        mgr.create_key("peeker", "peek_tok", KeyRole::ReadOnly, None);
        let h = make_headers(Some("peek_tok"));

        peek(&mgr, &h, AccessLevel::ReadOnly).unwrap();
        peek(&mgr, &h, AccessLevel::ReadOnly).unwrap();

        let list = mgr.list();
        let key = list.iter().find(|k| k.name == "peeker").unwrap();
        assert_eq!(key.request_count, 0);
        assert!(key.last_used.is_none());
    }

    #[test]
    fn auth_result_carries_rate_limit() {
        let mut mgr = ApiKeyManager::new(Some("master"));
        mgr.create_key("limited", "lim_tok", KeyRole::Operator, Some(50));
        let h = make_headers(Some("lim_tok"));

        let r = check(&mut mgr, &h, AccessLevel::Write).unwrap();
        assert_eq!(r.rate_limit, Some(50));
        assert_eq!(r.key_name, Some("limited".to_string()));
    }

    #[test]
    fn classify_public_endpoints() {
        assert_eq!(classify_endpoint("GET", "/v1/health"), AccessLevel::Public);
        assert_eq!(classify_endpoint("GET", "/v1/health/live"), AccessLevel::Public);
        assert_eq!(classify_endpoint("GET", "/v1/health/ready"), AccessLevel::Public);
        assert_eq!(classify_endpoint("GET", "/v1/version"), AccessLevel::Public);
        assert_eq!(classify_endpoint("GET", "/v1/rate-limit"), AccessLevel::Public);
    }

    #[test]
    fn classify_readonly_endpoints() {
        assert_eq!(classify_endpoint("GET", "/v1/metrics"), AccessLevel::ReadOnly);
        assert_eq!(classify_endpoint("GET", "/v1/daemons"), AccessLevel::ReadOnly);
        assert_eq!(classify_endpoint("GET", "/v1/logs"), AccessLevel::ReadOnly);
        assert_eq!(classify_endpoint("GET", "/v1/keys"), AccessLevel::ReadOnly);
        assert_eq!(classify_endpoint("GET", "/v1/session"), AccessLevel::ReadOnly);
    }

    #[test]
    fn classify_write_endpoints() {
        assert_eq!(classify_endpoint("POST", "/v1/deploy"), AccessLevel::Write);
        assert_eq!(classify_endpoint("POST", "/v1/estimate"), AccessLevel::Write);
        assert_eq!(classify_endpoint("POST", "/v1/events"), AccessLevel::Write);
        assert_eq!(classify_endpoint("DELETE", "/v1/daemons/x"), AccessLevel::Write);
    }

    #[test]
    fn classify_admin_endpoints() {
        assert_eq!(classify_endpoint("POST", "/v1/keys"), AccessLevel::Admin);
        assert_eq!(classify_endpoint("POST", "/v1/keys/revoke"), AccessLevel::Admin);
        assert_eq!(classify_endpoint("POST", "/v1/keys/rotate"), AccessLevel::Admin);
    }

    #[test]
    fn revoked_key_denied() {
        let mut mgr = ApiKeyManager::new(Some("master"));
        mgr.create_key("temp", "temp_tok", KeyRole::Operator, None);
        mgr.revoke("temp_tok");

        let h = make_headers(Some("temp_tok"));
        assert_eq!(check(&mut mgr, &h, AccessLevel::ReadOnly).unwrap_err(), StatusCode::FORBIDDEN);
    }
}
