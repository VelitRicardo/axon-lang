//! §Fase 118.b.2 — tenant IDENTITY, separated from tenant EXTRACTION.
//!
//! **The smell, a fourth time — and this one reached the storage layer.** The
//! task-local that carries the active tenant across a future tree, and the two
//! types that describe a tenant, lived in `tenant.rs` alongside an `axum`
//! middleware. So `storage_postgres.rs` — which calls [`current_tenant_id`] in
//! **31 places** to build the RLS `SET LOCAL axon.current_tenant` of every single
//! query — depended on a web framework to read a `String`. So did `daemon.rs`
//! (3), `resilient_backend.rs` (2) and `rate_limiter.rs` (via [`TenantPlan`]):
//! **42 call sites across the crate**, none of them serving HTTP.
//!
//! The split is along the seam that was always there and never drawn:
//!
//! - **Identity** (here): who the current tenant *is*, and how to bind it. A
//!   task-local, [`TenantPlan`], [`TenantContext`]. Reachable from anywhere,
//!   costs nothing, and is what every consumer above actually wanted.
//! - **Extraction** (`crate::tenant`, behind `server`): how a tenant is
//!   *resolved from an inbound HTTP request* — `X-Tenant-ID`, a JWKS-verified
//!   bearer, the axum middleware that scopes the task-local for the request's
//!   future tree. That is genuinely server code and is gated as such.
//!
//! `crate::tenant` re-exports everything here, so `axon::tenant::current_tenant_id`
//! and `axon::tenant::scope_tenant` keep resolving verbatim under a `server`
//! build — including for `axon-enterprise`, which names both.
//!
//! **This module must never acquire a dependency.**

use serde::{Deserialize, Serialize};

// ── Task-local tenant propagation ─────────────────────────────────────────────

tokio::task_local! {
    /// The active tenant_id for the current async task (Axum request).
    /// Set by `tenant_extractor_middleware` via `.scope()` so every downstream
    /// future — including storage methods — inherits the value automatically.
    pub static CURRENT_TENANT_ID: String;
}

/// Returns the active tenant_id for the current async task.
/// Falls back to `"default"` when called outside a scoped request context
/// (e.g. background tasks, tests, CLI operations).
pub fn current_tenant_id() -> String {
    CURRENT_TENANT_ID
        .try_with(|t| t.clone())
        .unwrap_or_else(|_| "default".to_string())
}

/// Run `fut` with the active-tenant task-local bound to `tenant_id`, so every
/// downstream future — including storage methods that read [`current_tenant_id`]
/// (and thus the RLS `SET LOCAL axon.current_tenant` in
/// `storage_postgres`'s `begin_tenant_tx!`) — observes it automatically.
///
/// This is the **public tenant-scope primitive**.
/// [`crate::tenant::tenant_extractor_middleware`] is the batteries-included path
/// (it resolves the tenant from an `X-Tenant-ID` header or a JWKS-verified
/// bearer); `scope_tenant` is the *unbundled* path for callers that resolve the
/// tenant THEMSELVES — e.g. an authentication layer that verifies its own tokens
/// (a different signature algorithm, a local keyring, an mTLS identity) and just
/// needs to bind the result. Scoping the tenant is thereby decoupled from how it
/// was authenticated, so multi-tenant isolation needs **zero per-handler
/// plumbing**: scope once at the request boundary and every downstream query is
/// tenant-bound by construction.
///
/// §Fase 118.b.2 — and, since the primitive no longer lives in the same module as
/// the HTTP extractor, "resolve the tenant yourself" no longer means "compile a
/// web framework to do it".
///
/// ```no_run
/// # async fn ex() {
/// // An external auth layer resolved `tenant` from its own verified principal:
/// let tenant = "acme".to_string();
/// axon::tenant_context::scope_tenant(tenant, async {
///     // every storage call here runs under SET LOCAL axon.current_tenant='acme'
/// })
/// .await;
/// # }
/// ```
pub async fn scope_tenant<F>(tenant_id: String, fut: F) -> F::Output
where
    F: std::future::Future,
{
    CURRENT_TENANT_ID.scope(tenant_id, fut).await
}

// ── TenantPlan ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TenantPlan {
    Starter,
    Pro,
    Enterprise,
}

impl TenantPlan {
    pub fn from_str(s: &str) -> Self {
        match s {
            "pro" => Self::Pro,
            "enterprise" => Self::Enterprise,
            _ => Self::Starter,
        }
    }
}

impl std::fmt::Display for TenantPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Starter => write!(f, "starter"),
            Self::Pro => write!(f, "pro"),
            Self::Enterprise => write!(f, "enterprise"),
        }
    }
}

// ── TenantContext ─────────────────────────────────────────────────────────────

/// Resolved tenant identity, available in every request handler as an
/// Axum `Extension<TenantContext>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantContext {
    pub tenant_id: String,
    pub plan: TenantPlan,
}

impl TenantContext {
    pub fn new(tenant_id: impl Into<String>, plan: TenantPlan) -> Self {
        Self { tenant_id: tenant_id.into(), plan }
    }

    /// The default / open-source single-tenant context.
    pub fn default_tenant() -> Self {
        Self { tenant_id: "default".to_string(), plan: TenantPlan::Enterprise }
    }

    pub fn is_default(&self) -> bool {
        self.tenant_id == "default"
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tenant_plan_from_str() {
        assert_eq!(TenantPlan::from_str("starter"), TenantPlan::Starter);
        assert_eq!(TenantPlan::from_str("pro"), TenantPlan::Pro);
        assert_eq!(TenantPlan::from_str("enterprise"), TenantPlan::Enterprise);
        assert_eq!(TenantPlan::from_str("unknown"), TenantPlan::Starter);
    }

    #[test]
    fn test_tenant_plan_display() {
        assert_eq!(TenantPlan::Starter.to_string(), "starter");
        assert_eq!(TenantPlan::Pro.to_string(), "pro");
        assert_eq!(TenantPlan::Enterprise.to_string(), "enterprise");
    }

    #[test]
    fn test_default_tenant() {
        let ctx = TenantContext::default_tenant();
        assert_eq!(ctx.tenant_id, "default");
        assert!(ctx.is_default());
        assert_eq!(ctx.plan, TenantPlan::Enterprise);
    }

    #[test]
    fn test_tenant_context_new() {
        let ctx = TenantContext::new("acme", TenantPlan::Pro);
        assert_eq!(ctx.tenant_id, "acme");
        assert_eq!(ctx.plan, TenantPlan::Pro);
        assert!(!ctx.is_default());
    }

    // ── Task-local tests ──────────────────────────────────────────────────────

    #[test]
    fn test_current_tenant_id_default_outside_scope() {
        // Outside any scope, must return "default" — never panic
        assert_eq!(current_tenant_id(), "default");
    }

    #[tokio::test]
    async fn test_current_tenant_id_inside_scope() {
        let result = CURRENT_TENANT_ID
            .scope("example-tenant".to_string(), async { current_tenant_id() })
            .await;
        assert_eq!(result, "example-tenant");
    }

    #[tokio::test]
    async fn test_current_tenant_id_nested_scope() {
        let outer = CURRENT_TENANT_ID
            .scope("tenant-a".to_string(), async {
                let inner = CURRENT_TENANT_ID
                    .scope("tenant-b".to_string(), async { current_tenant_id() })
                    .await;
                (current_tenant_id(), inner)
            })
            .await;
        assert_eq!(outer.0, "tenant-a");
        assert_eq!(outer.1, "tenant-b");
    }

    #[tokio::test]
    async fn test_scope_tenant_public_primitive_binds_and_nests() {
        // The public primitive binds the same task-local current_tenant_id reads.
        let got = scope_tenant("acme".to_string(), async { current_tenant_id() }).await;
        assert_eq!(got, "acme");
        // Returns its future's output (not just ()), so callers can scope a
        // whole request pipeline and forward the response.
        let doubled = scope_tenant("x".to_string(), async { 21 * 2 }).await;
        assert_eq!(doubled, 42);
        // Nesting restores the outer tenant after the inner scope ends.
        let (inner, outer) = scope_tenant("outer".to_string(), async {
            let inner = scope_tenant("inner".to_string(), async { current_tenant_id() }).await;
            (inner, current_tenant_id())
        })
        .await;
        assert_eq!((inner.as_str(), outer.as_str()), ("inner", "outer"));
        // Outside any scope it's still the safe default.
        assert_eq!(current_tenant_id(), "default");
    }
}
