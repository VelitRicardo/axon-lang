/// Tenant request EXTRACTION for Axon Enterprise multi-tenancy.
///
/// Resolves the active tenant from every inbound HTTP request via:
///   1. `X-Tenant-ID` header  (direct, service-to-service calls)
///   2. `Authorization: Bearer <jwt>` — **verified** when
/// `AXON_JWT_JWKS_URL` is configured (v1.4.0). Falls back to
///      unverified payload extraction when the verifier is not
///      configured (OSS / single-tenant installs).
///   3. Fallback → `"default"` (single-tenant / open-source installs)
///
/// The resolved `TenantContext` is injected as:
///   - An Axum request extension (`Extension<TenantContext>`) for handlers
///   - A tokio task-local (`CURRENT_TENANT_ID`) for storage methods, so every
///     `PostgresBackend` call picks up the tenant automatically without requiring
///     any changes to existing handlers.
///
/// v2.81.0 — tenant IDENTITY (the task-local, [`TenantPlan`],
/// [`TenantContext`], [`current_tenant_id`], [`scope_tenant`]) moved to
/// [`crate::tenant_context`], a leaf module with no dependencies, and is
/// re-exported below so every call site keeps resolving. What remains here is
/// the part that is genuinely about an inbound HTTP request: JWKS verification,
/// header parsing, and the `axum` middleware. That is why this module is behind
/// the `server` feature and `tenant_context` is not — `storage_postgres` reads
/// the active tenant in 58 places to build its RLS `SET LOCAL`, and it should
/// never have needed a web framework to do it.
use std::sync::Arc;

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Extension,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::Value;
use tokio::sync::OnceCell;

use crate::jwt_verifier::{JwtVerifier, JwtVerifierConfig};
use crate::tenant_context::CURRENT_TENANT_ID;

// v2.81.0 — the identity half, re-exported verbatim. `axon::tenant::
// current_tenant_id`, `axon::tenant::scope_tenant`, `axon::tenant::TenantPlan`
// and `axon::tenant::TenantContext` all keep resolving under a `server` build,
// including for `axon-enterprise`, which names them.
pub use crate::tenant_context::{
    current_tenant_id, scope_tenant, TenantContext, TenantPlan,
};

// ── JWT verifier singleton (v1.4.0) ──────────────────────────────────────

/// Lazily-initialised verifier. `None` means "no `AXON_JWT_JWKS_URL`
/// configured" — handlers fall through to the legacy unverified
/// extraction path. In production the OPS team MUST set
/// `AXON_JWT_JWKS_URL` so this returns `Some` and every bearer token
/// is signature-verified.
static JWT_VERIFIER: OnceCell<Option<Arc<JwtVerifier>>> = OnceCell::const_new();

async fn jwt_verifier() -> Option<Arc<JwtVerifier>> {
    JWT_VERIFIER
        .get_or_init(|| async {
            JwtVerifierConfig::from_env().map(|cfg| Arc::new(JwtVerifier::new(cfg)))
        })
        .await
        .clone()
}

// ── JWT claim extraction ──────────────────────────────────────────────────────

/// Extracts `tenant_id` from a JWT payload without signature verification.
/// Signature verification is the responsibility of the auth middleware layer.
fn tenant_id_from_jwt(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.splitn(3, '.').collect();
    if parts.len() < 2 {
        return None;
    }
    let payload_bytes = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let claims: Value = serde_json::from_slice(&payload_bytes).ok()?;
    claims.get("tenant_id")?.as_str().map(|s| s.to_string())
}

/// Extracts tenant_id from `Authorization: Bearer <token>` header
/// without signature verification. Used as a fallback only when no
/// `AXON_JWT_JWKS_URL` is configured (OSS / single-tenant installs).
fn tenant_id_from_bearer_unverified(headers: &HeaderMap) -> Option<String> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    tenant_id_from_jwt(token)
}

fn bearer_token<'a>(headers: &'a HeaderMap) -> Option<&'a str> {
    headers.get("authorization")?.to_str().ok()?.strip_prefix("Bearer ")
}

/// Result of a verified-bearer extraction. Extends `TenantContext`
/// with the other claims (roles, sub, jti) surfaced by `JwtVerifier`.
fn plan_from_claim(raw: Option<&str>) -> TenantPlan {
    raw.map(TenantPlan::from_str).unwrap_or(TenantPlan::Enterprise)
}

// ── Axum middleware ───────────────────────────────────────────────────────────

/// Axum middleware that resolves the active tenant and:
///   1. Injects `TenantContext` into request extensions (for handlers)
///   2. Scopes `CURRENT_TENANT_ID` task-local for the request's future tree
///      (for storage methods — zero handler changes needed)
///
/// Resolution order:
///   1. `X-Tenant-ID` header
///   2. `tenant_id` claim in `Authorization: Bearer <jwt>`
///   3. Fallback: `TenantContext::default_tenant()`
pub async fn tenant_extractor_middleware(
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let headers = req.headers().clone();
    let verifier = jwt_verifier().await;

    // ── 1. Verified bearer path (v1.4.0) ─────────────────────────────
    //
    // When a verifier is configured we prefer the verified claims over the
    // `X-Tenant-ID` header: a header can be forged by a compromised
    // intermediary, but the JWT signature cannot be forged without the
    // issuer's private key.
    if let Some(v) = verifier.clone() {
        if let Some(token) = bearer_token(&headers) {
            match v.verify(token).await {
                Ok(claims) => {
                    let ctx = TenantContext::new(
                        claims.tenant_id.clone(),
                        plan_from_claim(claims.plan.as_deref()),
                    );
                    tracing::debug!(
                        tenant_id = %ctx.tenant_id,
                        plan = %ctx.plan,
                        sub = claims.sub.as_deref().unwrap_or(""),
                        "tenant resolved via verified JWT"
                    );
                    let tenant_id = ctx.tenant_id.clone();
                    req.extensions_mut().insert(ctx);
                    return CURRENT_TENANT_ID.scope(tenant_id, next.run(req)).await;
                }
                Err(err) => {
                    // Enforcing deployments reject the request; lax
                    // deployments fall through to the legacy path with a
                    // warn log so operators notice the failure.
                    if v.config().enforce {
                        tracing::warn!(
                            error = %err,
                            "rejecting request: JWT verification failed"
                        );
                        return (
                            StatusCode::UNAUTHORIZED,
                            "invalid bearer token",
                        )
                            .into_response();
                    }
                    tracing::warn!(
                        error = %err,
                        "JWT verification failed — falling back to legacy path"
                    );
                }
            }
        } else if v.config().enforce {
            // Enforcing mode + no bearer + no X-Tenant-ID → reject.
            let has_xtenant = headers
                .get("x-tenant-id")
                .and_then(|v| v.to_str().ok())
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            if !has_xtenant {
                return (
                    StatusCode::UNAUTHORIZED,
                    "authorization required",
                )
                    .into_response();
            }
        }
    }

    // ── 2. Legacy path: X-Tenant-ID header or unverified JWT payload ────
    let ctx = if let Some(tid) = headers
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
    {
        TenantContext::new(tid, TenantPlan::Enterprise)
    } else if let Some(tid) = tenant_id_from_bearer_unverified(&headers) {
        TenantContext::new(tid, TenantPlan::Enterprise)
    } else {
        TenantContext::default_tenant()
    };

    tracing::debug!(
        tenant_id = %ctx.tenant_id,
        plan = %ctx.plan,
        "tenant resolved (legacy path)"
    );

    let tenant_id = ctx.tenant_id.clone();
    req.extensions_mut().insert(ctx);

    // Drive the rest of the request pipeline with CURRENT_TENANT_ID scoped to
    // this tenant. All storage calls downstream read it via current_tenant_id().
    CURRENT_TENANT_ID.scope(tenant_id, next.run(req)).await
}

// ── Helper for handlers ───────────────────────────────────────────────────────

/// Extract `TenantContext` from request extensions.
pub fn require_tenant(
    ext: Option<Extension<TenantContext>>,
) -> Result<TenantContext, Response> {
    ext.map(|Extension(ctx)| ctx)
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "TenantContext missing — tenant_extractor_middleware not wired",
            )
                .into_response()
        })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    fn make_jwt(payload_json: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"HS256","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(payload_json);
        format!("{}.{}.fakesig", header, payload)
    }

    // v2.81.0 — the TenantPlan / TenantContext / task-local tests moved
    // with their subjects to `tenant_context.rs`. What stays here is what this
    // module still owns: extracting a tenant from an inbound HTTP request.

    #[test]
    fn test_tenant_id_from_jwt_valid() {
        let jwt = make_jwt(r#"{"sub":"user123","tenant_id":"acme-corp"}"#);
        assert_eq!(tenant_id_from_jwt(&jwt), Some("acme-corp".to_string()));
    }

    #[test]
    fn test_tenant_id_from_jwt_missing_claim() {
        let jwt = make_jwt(r#"{"sub":"user123"}"#);
        assert_eq!(tenant_id_from_jwt(&jwt), None);
    }

    #[test]
    fn test_tenant_id_from_jwt_malformed() {
        assert_eq!(tenant_id_from_jwt("not.a.jwt.at.all"), None);
        assert_eq!(tenant_id_from_jwt("onlyone"), None);
    }

    #[test]
    fn test_tenant_id_from_bearer_valid() {
        let jwt = make_jwt(r#"{"tenant_id":"example-tenant"}"#);
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {}", jwt).parse().unwrap(),
        );
        assert_eq!(tenant_id_from_bearer_unverified(&headers), Some("example-tenant".to_string()));
    }

    #[test]
    fn test_tenant_id_from_bearer_missing() {
        let headers = HeaderMap::new();
        assert_eq!(tenant_id_from_bearer_unverified(&headers), None);
    }

}
