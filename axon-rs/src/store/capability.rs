//! v1.30.0 — Pillar IV: capability-typed store access.
//!
//! An `axonstore` may declare a `capability:` slug. Access to that
//! store — `retrieve` / `persist` / `mutate` / `purge` — then requires
//! the caller to hold that capability. Data isolation stops being an
//! app-code `if tenant_id == …` the developer must remember; it
//! becomes a **language guarantee**.
//!
//! # Two enforcement layers (D11)
//!
//! 1. **Compile-time** (`axon-frontend` type-checker): an
//!    `axonendpoint` executing a flow that accesses a capability-gated
//!    store must GRANT that capability in its `requires:` list. A
//!    program that would let an under-privileged endpoint reach a
//!    gated store does not type-check.
//!
//! 2. **Runtime re-check** (this module): the streaming dispatcher's
//!    store handlers re-verify, against the capabilities the request
//!    actually carries, that the gated store may be touched —
//!    defense-in-depth behind the static guarantee.
//!
//! # OSS / ENTERPRISE seam (section 6 — 35.j is SPLIT)
//!
//! This module + the type-checker enforcement are the **OSS
//! mechanism** — a capability is a slug, the check is set membership.
//! The **enterprise** layer owns the multitenant *operations*:
//! per-tenant capability provisioning, tenant-scoped connection
//! routing, per-tenant audit-chain segregation. The seam is the
//! `held` capability set: this module checks it; enterprise tooling
//! provisions it per tenant.
//!
//! Pure + total — no I/O.

use std::fmt;

/// A store access denied for lack of the required capability. Carries
/// the full picture — the store, the capability it demands, and what
/// the caller actually holds — so the denial is auditable without
/// server-log diving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDenied {
    /// The store whose access was denied.
    pub store: String,
    /// The capability slug the store requires.
    pub required: String,
    /// The capabilities the caller actually holds.
    pub held: Vec<String>,
}

impl fmt::Display for CapabilityDenied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "access to axonstore `{}` denied: it requires capability \
             `{}`, which the caller does not hold (held: {:?})",
            self.store, self.required, self.held
        )
    }
}

impl std::error::Error for CapabilityDenied {}

/// Check whether a caller holding `held` capabilities may access the
/// store `store_name`, which is gated by `required`.
///
/// - `required` empty — the store declares no capability gate → `Ok`.
/// - `required` ∈ `held` — the caller holds it → `Ok`.
/// - otherwise → `Err(CapabilityDenied)`.
///
/// Total: every input maps to exactly one outcome.
pub fn check_store_capability(
    store_name: &str,
    required: &str,
    held: &[String],
) -> Result<(), CapabilityDenied> {
    if required.is_empty() || held.iter().any(|h| h == required) {
        Ok(())
    } else {
        Err(CapabilityDenied {
            store: store_name.to_string(),
            required: required.to_string(),
            held: held.to_vec(),
        })
    }
}

// ════════════════════════════════════════════════════════════════════
//  Unit tests
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn slugs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn ungated_store_is_always_allowed() {
        // Empty `required` — no capability declared on the store.
        assert!(check_store_capability("cache", "", &[]).is_ok());
        assert!(check_store_capability("cache", "", &slugs(&["x"])).is_ok());
    }

    #[test]
    fn held_capability_allows_access() {
        let held = slugs(&["tenant.read", "audit.write"]);
        assert!(check_store_capability("tenants", "tenant.read", &held).is_ok());
    }

    #[test]
    fn missing_capability_is_denied() {
        let held = slugs(&["audit.write"]);
        match check_store_capability("tenants", "tenant.read", &held) {
            Err(denied) => {
                assert_eq!(denied.store, "tenants");
                assert_eq!(denied.required, "tenant.read");
                assert_eq!(denied.held, held);
            }
            Ok(()) => panic!("expected a capability denial"),
        }
    }

    #[test]
    fn empty_held_set_denies_a_gated_store() {
        assert!(check_store_capability("tenants", "tenant.read", &[]).is_err());
    }

    #[test]
    fn capability_match_is_exact_not_prefix() {
        // `tenant.read` must not satisfy a `tenant` requirement, nor
        // vice versa — capability slugs are matched whole.
        let held = slugs(&["tenant"]);
        assert!(check_store_capability("s", "tenant.read", &held).is_err());
        let held2 = slugs(&["tenant.read"]);
        assert!(check_store_capability("s", "tenant", &held2).is_err());
    }

    #[test]
    fn capability_denied_display_is_informative() {
        let denied = CapabilityDenied {
            store: "tenants".into(),
            required: "tenant.read".into(),
            held: slugs(&["audit.write"]),
        };
        let msg = denied.to_string();
        assert!(msg.contains("tenants"));
        assert!(msg.contains("tenant.read"));
        assert!(!msg.is_empty());
    }
}
