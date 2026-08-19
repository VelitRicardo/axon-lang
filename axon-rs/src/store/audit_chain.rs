//! v1.30.0 — Pillar II: audit-chained mutations.
//!
//! Every `persist` / `mutate` / `purge` against an `axonstore` appends
//! a delta to a **tamper-evident HMAC-Merkle mutation chain**. The
//! chain's complete history is independently verifiable: given the
//! chain and its HMAC key, any alteration of any past delta is
//! detectable. Regulatory replay (PCI DSS Req 10, FedRAMP AU-2,
//! 21 CFR Part 11 section 11.10) becomes a language primitive — not an
//! event-sourcing framework the adopter bolts on.
//!
//! # Joins v1.2.0 / `esk::provenance`
//!
//! The chain mechanism is not reinvented here — it is
//! [`crate::esk::provenance::ProvenanceChain`], the HMAC-SHA256
//! Merkle-linked signed-entry chain v1.2.0 already ships. This
//! module is the thin **store-domain layer** over it: a store mutation
//! is the chained payload; `on_breach` is the policy fired on a
//! detected tamper.
//!
//! # `on_breach` — the closed catalog
//!
//! An `axonstore` may declare `on_breach: { log | raise | rollback }`
//! (the catalog the type-checker's `VALID_STORE_ON_BREACH` already
//! enforces — section 7 question #3 resolved to this set, not a new one).
//! When chain verification detects tampering, the resolved
//! [`OnBreachPolicy`] decides the [`BreachOutcome`].
//!
//! # OSS / ENTERPRISE seam (section 6 — 35.h is SPLIT)
//!
//! This module is the **OSS mechanism**: the HMAC-Merkle chain with
//! `sha2`/`hmac` crypto + the `on_breach` policy. The chain is
//! in-process for the flow's lifetime. The **enterprise** layer
//! overrides with the FIPS-validated crypto link, the mmap
//! tamper-evident append-only kernel (persistent, cross-process), and
//! the court-admissible evidence packager. The seam is the
//! [`crate::esk::provenance::Signer`] trait + the in-memory chain:
//! enterprise swaps the signer for the FIPS link and the storage for
//! the mmap kernel; this module's verification logic is unchanged.
//!
//! Pure + total — no I/O.

use std::fmt;

use serde_json::{json, Value};

use crate::esk::provenance::{HmacSigner, ProvenanceChain, SignedEntry};
use crate::flow_execution_event::now_ms;

// ════════════════════════════════════════════════════════════════════
//  Store mutation kinds
// ════════════════════════════════════════════════════════════════════

/// The mutating store operations. `retrieve` is absent by design — it
/// reads, it does not mutate, so it appends no audit delta (D9 scopes
/// the chain to `persist` / `mutate` / `purge`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreMutationKind {
    /// `persist into S` — an insert.
    Persist,
    /// `mutate S where φ` — an update.
    Mutate,
    /// `purge from S where φ` — a delete.
    Purge,
}

impl StoreMutationKind {
    /// The canonical lowercase spelling — the value hashed into the
    /// chain delta.
    pub fn as_str(self) -> &'static str {
        match self {
            StoreMutationKind::Persist => "persist",
            StoreMutationKind::Mutate => "mutate",
            StoreMutationKind::Purge => "purge",
        }
    }
}

impl fmt::Display for StoreMutationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ════════════════════════════════════════════════════════════════════
//  on_breach policy (closed catalog — VALID_STORE_ON_BREACH)
// ════════════════════════════════════════════════════════════════════

/// The policy a store's `on_breach:` declaration resolves to. The
/// catalog is `{ log, raise, rollback }` — the set the frontend
/// type-checker's `VALID_STORE_ON_BREACH` already enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnBreachPolicy {
    /// Record the breach and continue (the default for an undeclared
    /// `on_breach`).
    Log,
    /// Surface the breach as an error — fail loud.
    Raise,
    /// Surface the breach AND signal that the mutation history must be
    /// rolled back to the last verified-intact delta.
    Rollback,
}

impl OnBreachPolicy {
    /// The canonical lowercase spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            OnBreachPolicy::Log => "log",
            OnBreachPolicy::Raise => "raise",
            OnBreachPolicy::Rollback => "rollback",
        }
    }
}

/// Resolve an `IRAxonStore.on_breach` string to a policy. Trimmed +
/// case-insensitive. Empty → `Log` (the gentle default). An unknown
/// value also resolves to `Log` defensively — the type-checker
/// rejects unknowns at compile time, so this arm is totality-only.
pub fn resolve_on_breach(on_breach: &str) -> OnBreachPolicy {
    match on_breach.trim().to_ascii_lowercase().as_str() {
        "raise" => OnBreachPolicy::Raise,
        "rollback" => OnBreachPolicy::Rollback,
        _ => OnBreachPolicy::Log,
    }
}

// ════════════════════════════════════════════════════════════════════
//  Verification verdict + breach outcome
// ════════════════════════════════════════════════════════════════════

/// The result of verifying a store's mutation chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainVerdict {
    /// Every delta re-hashes + re-links correctly — the history is
    /// authentic.
    Intact,
    /// At least one delta fails verification — the history has been
    /// tampered with.
    Tampered,
}

/// What an `on_breach` policy produces for a given [`ChainVerdict`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreachOutcome {
    /// The chain verified intact — nothing to do.
    Clean,
    /// Tamper detected; `on_breach: log` — recorded, execution
    /// continues.
    Logged { detail: String },
    /// Tamper detected; `on_breach: raise` — the caller must surface
    /// this as an error.
    Raised { detail: String },
    /// Tamper detected; `on_breach: rollback` — the caller must surface
    /// this AND roll the mutation history back.
    RolledBack { detail: String },
}

impl BreachOutcome {
    /// `true` iff the outcome demands the caller halt / fail (raise or
    /// rollback). `Clean` and `Logged` are non-halting.
    pub fn is_halting(&self) -> bool {
        matches!(self, BreachOutcome::Raised { .. } | BreachOutcome::RolledBack { .. })
    }
}

/// Apply a store's `on_breach` policy to a verification verdict.
/// Total — every `(verdict, policy)` pair maps to one outcome.
pub fn apply_on_breach(
    store_name: &str,
    verdict: ChainVerdict,
    policy: OnBreachPolicy,
) -> BreachOutcome {
    if verdict == ChainVerdict::Intact {
        return BreachOutcome::Clean;
    }
    let detail = format!(
        "axonstore `{store_name}` mutation chain failed verification — \
         tamper detected"
    );
    match policy {
        OnBreachPolicy::Log => BreachOutcome::Logged { detail },
        OnBreachPolicy::Raise => BreachOutcome::Raised { detail },
        OnBreachPolicy::Rollback => BreachOutcome::RolledBack { detail },
    }
}

// ════════════════════════════════════════════════════════════════════
//  StoreAuditChain
// ════════════════════════════════════════════════════════════════════

/// The tamper-evident mutation chain for a flow's `axonstore`
/// activity. Wraps an HMAC-SHA256 [`ProvenanceChain`] and retains each
/// delta payload so the chain can be independently re-verified.
pub struct StoreAuditChain {
    chain: ProvenanceChain<HmacSigner>,
    /// The delta payloads, in append order — re-supplied to
    /// `ProvenanceChain::verify`.
    payloads: Vec<Value>,
}

impl StoreAuditChain {
    /// A fresh chain keyed by a cryptographically-random HMAC key.
    /// The key lives as long as the chain — sufficient for in-process
    /// verification (the OSS scope). The enterprise layer supplies a
    /// stable, FIPS-validated key for cross-process replay.
    pub fn new() -> Self {
        StoreAuditChain {
            chain: ProvenanceChain::new(HmacSigner::random()),
            payloads: Vec::new(),
        }
    }

    /// A chain keyed by an explicit HMAC key — for deterministic tests
    /// and for adopters threading a stable key.
    pub fn with_key(key: Vec<u8>) -> Self {
        StoreAuditChain {
            chain: ProvenanceChain::new(HmacSigner::new(key)),
            payloads: Vec::new(),
        }
    }

    /// Append one mutation delta to the chain. The delta hashes a
    /// monotonic `seq` (so two identical mutations stay distinct), the
    /// operation, the store name, a result summary, and a timestamp.
    pub fn record(
        &mut self,
        kind: StoreMutationKind,
        store: &str,
        summary: &str,
    ) -> SignedEntry {
        let payload = json!({
            "seq": self.payloads.len(),
            "op": kind.as_str(),
            "store": store,
            "summary": summary,
            "timestamp_ms": now_ms(),
        });
        let entry = self.chain.append(&payload);
        self.payloads.push(payload);
        entry
    }

    /// The chain head — the Merkle root over every delta so far. A
    /// tamper-evident fingerprint of the flow's complete mutation
    /// history. `GENESIS_HASH` for an empty chain.
    pub fn head(&self) -> String {
        self.chain.head()
    }

    /// The number of mutation deltas recorded.
    pub fn len(&self) -> usize {
        self.payloads.len()
    }

    /// `true` iff no mutation has been recorded.
    pub fn is_empty(&self) -> bool {
        self.payloads.is_empty()
    }

    /// Independently verify the chain — re-hash every delta and check
    /// every Merkle link + HMAC signature.
    pub fn verify(&self) -> ChainVerdict {
        if self.chain.verify(&self.payloads) {
            ChainVerdict::Intact
        } else {
            ChainVerdict::Tampered
        }
    }

    /// The recorded delta payloads, in append order.
    pub fn payloads(&self) -> &[Value] {
        &self.payloads
    }

    /// Verify the chain and apply a store's `on_breach` policy to the
    /// verdict in one step.
    pub fn audit(&self, store_name: &str, policy: OnBreachPolicy) -> BreachOutcome {
        apply_on_breach(store_name, self.verify(), policy)
    }
}

impl Default for StoreAuditChain {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for StoreAuditChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never expose payloads or the key — just the shape.
        f.debug_struct("StoreAuditChain")
            .field("deltas", &self.payloads.len())
            .field("head", &self.head())
            .finish()
    }
}

// ════════════════════════════════════════════════════════════════════
//  Unit tests
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── mutation kinds ───────────────────────────────────────────────

    #[test]
    fn mutation_kinds_render_canonically() {
        assert_eq!(StoreMutationKind::Persist.as_str(), "persist");
        assert_eq!(StoreMutationKind::Mutate.as_str(), "mutate");
        assert_eq!(StoreMutationKind::Purge.as_str(), "purge");
    }

    // ── on_breach resolution ─────────────────────────────────────────

    #[test]
    fn on_breach_resolves_the_closed_catalog() {
        assert_eq!(resolve_on_breach("log"), OnBreachPolicy::Log);
        assert_eq!(resolve_on_breach("raise"), OnBreachPolicy::Raise);
        assert_eq!(resolve_on_breach("rollback"), OnBreachPolicy::Rollback);
    }

    #[test]
    fn on_breach_is_trimmed_and_case_insensitive() {
        assert_eq!(resolve_on_breach("  RAISE  "), OnBreachPolicy::Raise);
        assert_eq!(resolve_on_breach("Rollback"), OnBreachPolicy::Rollback);
    }

    #[test]
    fn on_breach_empty_defaults_to_log() {
        assert_eq!(resolve_on_breach(""), OnBreachPolicy::Log);
        assert_eq!(resolve_on_breach("   "), OnBreachPolicy::Log);
    }

    // ── chain append + head ──────────────────────────────────────────

    #[test]
    fn empty_chain_head_is_genesis() {
        let chain = StoreAuditChain::with_key(vec![1, 2, 3, 4]);
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
        assert_eq!(chain.head(), crate::esk::provenance::GENESIS_HASH);
    }

    #[test]
    fn recording_a_mutation_advances_the_chain() {
        let mut chain = StoreAuditChain::with_key(vec![9; 32]);
        let genesis = chain.head();
        chain.record(StoreMutationKind::Persist, "tenants", "1 row");
        assert_eq!(chain.len(), 1);
        assert_ne!(chain.head(), genesis, "the head must advance on append");
    }

    #[test]
    fn each_delta_links_to_the_previous() {
        let mut chain = StoreAuditChain::with_key(vec![7; 32]);
        let e0 = chain.record(StoreMutationKind::Persist, "s", "a");
        let e1 = chain.record(StoreMutationKind::Mutate, "s", "b");
        let e2 = chain.record(StoreMutationKind::Purge, "s", "c");
        // Each entry's previous_hash is the prior entry's chain_hash.
        assert_eq!(e1.previous_hash, e0.chain_hash);
        assert_eq!(e2.previous_hash, e1.chain_hash);
        assert_eq!(e0.index, 0);
        assert_eq!(e2.index, 2);
    }

    // ── verification ─────────────────────────────────────────────────

    #[test]
    fn an_untampered_chain_verifies_intact() {
        let mut chain = StoreAuditChain::with_key(vec![3; 32]);
        chain.record(StoreMutationKind::Persist, "ledger", "100");
        chain.record(StoreMutationKind::Mutate, "ledger", "200");
        chain.record(StoreMutationKind::Purge, "ledger", "0");
        assert_eq!(chain.verify(), ChainVerdict::Intact);
    }

    #[test]
    fn an_empty_chain_verifies_intact() {
        let chain = StoreAuditChain::with_key(vec![1; 32]);
        assert_eq!(chain.verify(), ChainVerdict::Intact);
    }

    #[test]
    fn a_tampered_delta_breaks_verification() {
        let mut chain = StoreAuditChain::with_key(vec![5; 32]);
        chain.record(StoreMutationKind::Persist, "ledger", "100");
        chain.record(StoreMutationKind::Mutate, "ledger", "200");
        chain.record(StoreMutationKind::Purge, "ledger", "0");
        // Forge a retained payload — alter the recorded summary of the
        // middle delta. The chain hashes no longer match.
        chain.payloads[1]["summary"] = json!("999999");
        assert_eq!(chain.verify(), ChainVerdict::Tampered);
    }

    #[test]
    fn tampering_with_the_operation_is_detected() {
        let mut chain = StoreAuditChain::with_key(vec![6; 32]);
        chain.record(StoreMutationKind::Purge, "ledger", "drop");
        // Rewrite a `purge` delta to look like a `persist`.
        chain.payloads[0]["op"] = json!("persist");
        assert_eq!(chain.verify(), ChainVerdict::Tampered);
    }

    // ── on_breach application ────────────────────────────────────────

    #[test]
    fn intact_chain_yields_a_clean_outcome_for_every_policy() {
        for policy in [
            OnBreachPolicy::Log,
            OnBreachPolicy::Raise,
            OnBreachPolicy::Rollback,
        ] {
            assert_eq!(
                apply_on_breach("s", ChainVerdict::Intact, policy),
                BreachOutcome::Clean
            );
        }
    }

    #[test]
    fn tampered_chain_fires_the_declared_policy() {
        assert!(matches!(
            apply_on_breach("s", ChainVerdict::Tampered, OnBreachPolicy::Log),
            BreachOutcome::Logged { .. }
        ));
        assert!(matches!(
            apply_on_breach("s", ChainVerdict::Tampered, OnBreachPolicy::Raise),
            BreachOutcome::Raised { .. }
        ));
        assert!(matches!(
            apply_on_breach("s", ChainVerdict::Tampered, OnBreachPolicy::Rollback),
            BreachOutcome::RolledBack { .. }
        ));
    }

    #[test]
    fn only_raise_and_rollback_are_halting() {
        assert!(!BreachOutcome::Clean.is_halting());
        assert!(!BreachOutcome::Logged { detail: "x".into() }.is_halting());
        assert!(BreachOutcome::Raised { detail: "x".into() }.is_halting());
        assert!(BreachOutcome::RolledBack { detail: "x".into() }.is_halting());
    }

    #[test]
    fn audit_combines_verify_and_policy() {
        let mut chain = StoreAuditChain::with_key(vec![8; 32]);
        chain.record(StoreMutationKind::Persist, "s", "ok");
        // Intact → Clean regardless of policy.
        assert_eq!(
            chain.audit("s", OnBreachPolicy::Raise),
            BreachOutcome::Clean
        );
        // Tamper, then audit under `raise` → a halting outcome.
        chain.payloads[0]["summary"] = json!("forged");
        assert!(chain.audit("s", OnBreachPolicy::Raise).is_halting());
    }

    // ── Debug never leaks payloads ───────────────────────────────────

    #[test]
    fn debug_does_not_leak_delta_payloads() {
        let mut chain = StoreAuditChain::with_key(vec![2; 32]);
        chain.record(StoreMutationKind::Persist, "secret_store", "sensitive");
        let debug = format!("{chain:?}");
        assert!(!debug.contains("sensitive"));
        assert!(debug.contains("deltas"));
    }
}
