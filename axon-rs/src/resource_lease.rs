//! v2.69.0 — **`lease` over a vendor: the CT-2 Anchor Breach fires on a tool
//! call, not just a store op.**
//!
//! v2.67.0 gave `lease` its first use-site: a store operation is a *use* of the
//! resource, so a post-expiry store op is the breach. v2.69.0 made a `tool` name
//! a resource too — so a **tool call** is also a use, and a post-expiry vendor call
//! must breach the same way.
//!
//! This is the shared lease guard, keyed by **resource** (a lease is over a
//! resource, not over the thing that uses it). `StoreRegistry` holds its own guard
//! for store-held leases (v2.67.0, unchanged); this one is held on `ServerState` for
//! tool-held leases.
//!
//! # Why one guard never charges another's lease
//!
//! `axon-T945` (extended in v2.69.0 to count tool holders) guarantees a leased —
//! i.e. `affine` or `linear` — resource has **exactly one holder**: a store XOR a
//! tool, never both. So a given leased resource is charged by exactly one path. The
//! two guards are disjoint by construction, not by luck.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::ir_nodes::{IRLease, IRResource};
use crate::runtime::lease_kernel::{Clock, LeaseKernel, LeaseToken, UseOutcome};

/// A CT-2 Anchor Breach: a resource was used after its lease expired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseBreach {
    pub resource: String,
    pub lease: String,
    pub detail: String,
}

impl std::fmt::Display for LeaseBreach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CT-2 ANCHOR BREACH — resource '{}' was used, but `lease {}` over it is no longer \
             held: {}. A lease is a τ-decaying affine capability: using the resource after \
             expiry is the breach.",
            self.resource, self.lease, self.detail
        )
    }
}

/// Why a lease could not be acquired at build.
#[derive(Debug, Clone)]
pub struct LeaseAcquireError {
    pub lease: String,
    pub resource: String,
    pub detail: String,
}

/// The leases held over a program's resources, keyed by resource name.
pub struct ResourceLeaseGuard {
    kernel: Mutex<LeaseKernel>,
    tokens: Mutex<HashMap<String, LeaseToken>>,
}

impl std::fmt::Debug for ResourceLeaseGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never dump token internals; report only which resources are governed.
        let held: Vec<String> = self
            .tokens
            .lock()
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default();
        f.debug_struct("ResourceLeaseGuard").field("resources", &held).finish()
    }
}

impl ResourceLeaseGuard {
    /// Acquire the program's leases. `acquire: on_start` takes the token now, so it
    /// exists BEFORE any use — which is what makes a post-expiry use a detectable
    /// event. Returns `Ok(None)` when the program declares no leases (costs
    /// nothing).
    pub fn from_ir(
        leases: &[IRLease],
        resources: &[IRResource],
    ) -> Result<Option<Self>, LeaseAcquireError> {
        Self::from_ir_with_clock(leases, resources, Box::new(chrono::Utc::now))
    }

    /// As [`Self::from_ir`], with an injectable clock (τ-decay is only testable if
    /// time can be moved — a gate that slept an hour would never run).
    pub fn from_ir_with_clock(
        leases: &[IRLease],
        resources: &[IRResource],
        clock: Clock,
    ) -> Result<Option<Self>, LeaseAcquireError> {
        if leases.is_empty() {
            return Ok(None);
        }
        let mut kernel = LeaseKernel::with_clock(clock);
        let mut tokens = HashMap::new();
        for l in leases {
            let Some(res) = resources.iter().find(|r| r.name == l.resource_ref) else {
                return Err(LeaseAcquireError {
                    lease: l.name.clone(),
                    resource: l.resource_ref.clone(),
                    detail: "the lease names a resource the program does not declare".into(),
                });
            };
            let token = kernel.acquire(l, res).map_err(|e| LeaseAcquireError {
                lease: l.name.clone(),
                resource: l.resource_ref.clone(),
                detail: e.message.clone(),
            })?;
            tokens.insert(l.resource_ref.clone(), token);
        }
        Ok(Some(ResourceLeaseGuard {
            kernel: Mutex::new(kernel),
            tokens: Mutex::new(tokens),
        }))
    }

    /// Charge a **use** of `resource_name` against its lease.
    ///
    /// `Ok(())` ⇒ no lease governs this resource, or the lease is live (or was
    /// renewed under `on_expire: extend`). `Err` ⇒ the CT-2 Anchor Breach — the
    /// use must not proceed.
    pub fn charge(&self, resource_name: &str) -> Result<(), LeaseBreach> {
        let token = {
            let tokens = self.tokens.lock().unwrap_or_else(|p| p.into_inner());
            match tokens.get(resource_name) {
                Some(t) => t.clone(),
                None => return Ok(()), // no lease over this resource
            }
        };
        let mut kernel = self.kernel.lock().unwrap_or_else(|p| p.into_inner());
        match kernel.use_token(&token) {
            Ok(UseOutcome::Valid(_)) => Ok(()),
            Ok(UseOutcome::Extended(renewed)) => {
                // `on_expire: extend` — the window rolls forward. Record the new
                // token, or the next use would present a revoked one.
                self.tokens
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(resource_name.to_string(), renewed);
                Ok(())
            }
            Ok(UseOutcome::Released) => {
                self.tokens
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(resource_name);
                Err(LeaseBreach {
                    resource: resource_name.to_string(),
                    lease: token.lease_name.clone(),
                    detail: "lease released at expiry (on_expire: release) — the capability is no \
                             longer held"
                        .into(),
                })
            }
            Err(e) => Err(LeaseBreach {
                resource: resource_name.to_string(),
                lease: token.lease_name.clone(),
                detail: e.message,
            }),
        }
    }
}
