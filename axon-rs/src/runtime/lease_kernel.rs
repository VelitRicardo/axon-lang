//! AXON Runtime — LeaseKernel (v1.1.0)
//!
//! Direct port of `axon/runtime/lease_kernel.py`.
//!
//! τ-decay token manager implementing Decision D2:
//!   * Compile-time: a lease references an `affine` or `linear` resource.
//!     The type checker already rejected `persistent` leases (no τ to decay).
//!   * Runtime: `acquire` emits a revocable `LeaseToken` with explicit τ
//!     (`acquired_at`, `expires_at`). Post-expiry `use` raises
//!     `LeaseExpired` (CT-2 Anchor Breach) unless a permissive policy is set.
//!   * Policy: `on_expire ∈ {anchor_breach, release, extend}`.
//!
//! The kernel is an in-process registry; distributed coordination is
//! deferred to a later phase.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use crate::handlers::base::{HandlerError, LambdaEnvelope};
use crate::ir_nodes::{IRLease, IRResource};

// ═══════════════════════════════════════════════════════════════════
//  DURATION PARSING — "30s" | "5m" | "2h" | "12ms" | "1d"
// ═══════════════════════════════════════════════════════════════════

/// Convert an Axon duration literal into fractional seconds.
///
/// Raises a CT-1 (`HandlerError::callee`) on unparseable input because the
/// parser already validated the syntax — a failure here is a runtime bug.
pub fn parse_duration(text: &str) -> Result<f64, HandlerError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(HandlerError::callee("parse_duration called with empty string"));
    }
    // Split numeric prefix from unit suffix.
    let split = trimmed.find(|c: char| !c.is_ascii_digit()).unwrap_or(trimmed.len());
    if split == 0 {
        return Err(HandlerError::callee(format!(
            "unparseable duration literal: '{text}' (expected <int><ms|s|m|h|d>)"
        )));
    }
    let (num_str, unit) = trimmed.split_at(split);
    let unit = unit.trim_start();
    let value: u64 = num_str.parse().map_err(|_| {
        HandlerError::callee(format!("unparseable duration literal: '{text}'"))
    })?;
    let unit_secs = match unit {
        "ms" => 0.001_f64,
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3600.0,
        "d" => 86400.0,
        _ => {
            return Err(HandlerError::callee(format!(
                "unparseable duration literal: '{text}' (expected <int><ms|s|m|h|d>)"
            )));
        }
    };
    Ok(value as f64 * unit_secs)
}

// ═══════════════════════════════════════════════════════════════════
//  LEASE TOKEN — the τ-decaying affine capability
// ═══════════════════════════════════════════════════════════════════

/// A single-use capability over a resource, valid only while τ is in the
/// `[acquired_at, expires_at)` window. The token is effectively frozen —
/// extension is implemented by minting a new token and revoking the old,
/// preserving the linearity invariant.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LeaseToken {
    pub token_id: String,
    pub lease_name: String,
    pub resource_ref: String,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub on_expire: String,
}

impl LeaseToken {
    /// Current ΛD envelope — c decays to 0.0 when τ expires.
    pub fn envelope(&self, now: DateTime<Utc>) -> LambdaEnvelope {
        if now >= self.expires_at {
            LambdaEnvelope::new(
                0.0,
                now.to_rfc3339(),
                "lease_kernel".into(),
                "observed".into(),
            )
        } else {
            LambdaEnvelope::new(
                1.0,
                self.acquired_at.to_rfc3339(),
                "lease_kernel".into(),
                "axiomatic".into(),
            )
        }
    }

    /// Remaining window in seconds (0 if already expired).
    pub fn remaining_seconds(&self, now: DateTime<Utc>) -> f64 {
        let delta = self.expires_at.signed_duration_since(now);
        let secs = delta.num_milliseconds() as f64 / 1000.0;
        if secs < 0.0 { 0.0 } else { secs }
    }
}

// ═══════════════════════════════════════════════════════════════════
//  LEASE KERNEL
// ═══════════════════════════════════════════════════════════════════

/// Pluggable wall-clock. Defaults to `Utc::now`; tests inject a controllable
/// clock to verify τ-decay without sleeping.
pub type Clock = Box<dyn Fn() -> DateTime<Utc> + Send>;

/// Return value from `LeaseKernel::use_token`:
///   * `Valid(token)` — the same token is still inside its τ window.
///   * `Extended(new)` — on_expire="extend" minted a fresh token.
///   * `Released` — on_expire="release" silently retired the lease.
#[derive(Debug, Clone)]
pub enum UseOutcome {
    Valid(LeaseToken),
    Extended(LeaseToken),
    Released,
}

/// In-process registry of active leases.
pub struct LeaseKernel {
    tokens: HashMap<String, LeaseToken>,
    revoked: HashSet<String>,
    clock: Clock,
}

impl LeaseKernel {
    pub fn new() -> Self {
        LeaseKernel {
            tokens: HashMap::new(),
            revoked: HashSet::new(),
            clock: Box::new(Utc::now),
        }
    }

    pub fn with_clock(clock: Clock) -> Self {
        LeaseKernel { tokens: HashMap::new(), revoked: HashSet::new(), clock }
    }

    /// Mint a fresh token for a lease against a resource. Rejects persistent
    /// resources (defence in depth — the type-checker already did this).
    pub fn acquire(
        &mut self,
        ir_lease: &IRLease,
        ir_resource: &IRResource,
    ) -> Result<LeaseToken, HandlerError> {
        if ir_resource.lifetime == "persistent" {
            return Err(HandlerError::caller(format!(
                "lease '{}' cannot target persistent resource '{}' — \
                 persistent (!A) is unbounded, it has no τ to decay.",
                ir_lease.name, ir_resource.name
            )));
        }
        if ir_lease.resource_ref != ir_resource.name {
            return Err(HandlerError::callee(format!(
                "acquire called with mismatched resource: lease.resource_ref={:?}, \
                 ir_resource.name={:?}",
                ir_lease.resource_ref, ir_resource.name
            )));
        }
        let seconds = parse_duration(&ir_lease.duration)?;
        let now = (self.clock)();
        let millis = (seconds * 1000.0) as i64;
        let token = LeaseToken {
            token_id: format!("lease-{}", &Uuid::new_v4().simple().to_string()[..12]),
            lease_name: ir_lease.name.clone(),
            resource_ref: ir_resource.name.clone(),
            acquired_at: now,
            expires_at: now + Duration::milliseconds(millis),
            on_expire: ir_lease.on_expire.clone(),
        };
        self.tokens.insert(token.token_id.clone(), token.clone());
        Ok(token)
    }

    /// Verify the token is still valid and apply `on_expire` policy on decay.
    pub fn use_token(&mut self, token: &LeaseToken) -> Result<UseOutcome, HandlerError> {
        if self.revoked.contains(&token.token_id) {
            return Err(HandlerError::caller(format!(
                "lease token '{}' was revoked (lease='{}')",
                token.token_id, token.lease_name
            )));
        }
        if !self.tokens.contains_key(&token.token_id) {
            return Err(HandlerError::caller(format!(
                "unknown lease token '{}' (lease='{}') — did you forget to acquire?",
                token.token_id, token.lease_name
            )));
        }
        let now = (self.clock)();
        if now < token.expires_at {
            return Ok(UseOutcome::Valid(token.clone()));
        }
        match token.on_expire.as_str() {
            "anchor_breach" => Err(HandlerError::lease_expired(format!(
                "lease '{}' on resource '{}' expired at {} \
                 (Anchor Breach — Decision D2, CT-2)",
                token.lease_name, token.resource_ref,
                token.expires_at.to_rfc3339()
            ))),
            "release" => {
                self.tokens.remove(&token.token_id);
                Ok(UseOutcome::Released)
            }
            "extend" => {
                // Preserve the original Δt window; mint a fresh token and
                // revoke the old so linearity of the lease-name mapping holds.
                let duration = token.expires_at.signed_duration_since(token.acquired_at);
                let renewed = LeaseToken {
                    token_id: format!("lease-{}", &Uuid::new_v4().simple().to_string()[..12]),
                    lease_name: token.lease_name.clone(),
                    resource_ref: token.resource_ref.clone(),
                    acquired_at: now,
                    expires_at: now + duration,
                    on_expire: token.on_expire.clone(),
                };
                self.revoked.insert(token.token_id.clone());
                self.tokens.remove(&token.token_id);
                self.tokens.insert(renewed.token_id.clone(), renewed.clone());
                Ok(UseOutcome::Extended(renewed))
            }
            other => Err(HandlerError::callee(format!(
                "unknown on_expire policy '{other}' (token id='{}')",
                token.token_id
            ))),
        }
    }

    /// Explicitly revoke a token. Idempotent.
    pub fn release(&mut self, token: &LeaseToken) {
        self.revoked.insert(token.token_id.clone());
        self.tokens.remove(&token.token_id);
    }

    /// Purge tokens whose τ has elapsed. Returns the removed tokens.
    pub fn sweep(&mut self) -> Vec<LeaseToken> {
        let now = (self.clock)();
        let expired: Vec<LeaseToken> = self
            .tokens
            .values()
            .filter(|t| now >= t.expires_at)
            .cloned()
            .collect();
        for t in &expired {
            self.tokens.remove(&t.token_id);
        }
        expired
    }

    /// Snapshot of currently-valid tokens.
    pub fn active(&self) -> Vec<LeaseToken> {
        let now = (self.clock)();
        self.tokens.values().filter(|t| now < t.expires_at).cloned().collect()
    }

    pub fn contains(&self, token_id: &str) -> bool {
        self.tokens.contains_key(token_id)
    }
}

impl Default for LeaseKernel {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::base::HandlerErrorKind;
    use std::cell::Cell;
    use std::sync::{Arc, Mutex};

    fn mk_resource(name: &str, lifetime: &str) -> IRResource {
        IRResource {
            node_type: "resource",
            source_line: 1,
            source_column: 1,
            name: name.into(),
            kind: "postgres".into(),
            endpoint: String::new(),
            capacity: None,
            lifetime: lifetime.into(),
            certainty_floor: None,
            shield_ref: String::new(),
            within: String::new(),
        }
    }

    fn mk_lease(name: &str, resource_ref: &str, duration: &str, on_expire: &str) -> IRLease {
        IRLease {
            node_type: "lease",
            source_line: 1,
            source_column: 1,
            name: name.into(),
            resource_ref: resource_ref.into(),
            duration: duration.into(),
            acquire: "on_start".into(),
            on_expire: on_expire.into(),
        }
    }

    /// A controllable clock for τ-decay tests. Uses interior mutability
    /// to advance time without re-acquiring the kernel.
    fn mock_clock() -> (Clock, Arc<Mutex<DateTime<Utc>>>) {
        let start: DateTime<Utc> = "2026-04-20T12:00:00Z".parse().unwrap();
        let state = Arc::new(Mutex::new(start));
        let c = state.clone();
        let clock: Clock = Box::new(move || *c.lock().unwrap());
        (clock, state)
    }

    #[test]
    fn parse_duration_handles_all_units() {
        assert!((parse_duration("500ms").unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(parse_duration("30s").unwrap(), 30.0);
        assert_eq!(parse_duration("5m").unwrap(), 300.0);
        assert_eq!(parse_duration("2h").unwrap(), 7200.0);
        assert_eq!(parse_duration("1d").unwrap(), 86400.0);
    }

    #[test]
    fn parse_duration_rejects_garbage() {
        assert_eq!(parse_duration("").unwrap_err().kind, HandlerErrorKind::Callee);
        assert_eq!(parse_duration("30y").unwrap_err().kind, HandlerErrorKind::Callee);
        assert_eq!(parse_duration("forever").unwrap_err().kind, HandlerErrorKind::Callee);
    }

    #[test]
    fn acquire_mints_valid_token() {
        let mut k = LeaseKernel::new();
        let r = mk_resource("Db", "linear");
        let l = mk_lease("L", "Db", "30s", "anchor_breach");
        let tok = k.acquire(&l, &r).unwrap();
        assert!(tok.token_id.starts_with("lease-"));
        assert_eq!(tok.lease_name, "L");
        assert_eq!(tok.resource_ref, "Db");
        assert!(k.contains(&tok.token_id));
    }

    #[test]
    fn acquire_rejects_persistent_resource() {
        let mut k = LeaseKernel::new();
        let r = mk_resource("Shared", "persistent");
        let l = mk_lease("L", "Shared", "30s", "anchor_breach");
        let err = k.acquire(&l, &r).unwrap_err();
        assert_eq!(err.kind, HandlerErrorKind::Caller);
    }

    #[test]
    fn use_before_expiry_returns_same_token() {
        let mut k = LeaseKernel::new();
        let r = mk_resource("Db", "affine");
        let l = mk_lease("L", "Db", "30s", "anchor_breach");
        let tok = k.acquire(&l, &r).unwrap();
        match k.use_token(&tok).unwrap() {
            UseOutcome::Valid(t) => assert_eq!(t.token_id, tok.token_id),
            other => panic!("expected Valid, got {other:?}"),
        }
    }

    #[test]
    fn anchor_breach_policy_raises_after_tau_decay() {
        let (clock, state) = mock_clock();
        let mut k = LeaseKernel::with_clock(clock);
        let r = mk_resource("Db", "linear");
        let l = mk_lease("L", "Db", "1s", "anchor_breach");
        let tok = k.acquire(&l, &r).unwrap();
        // Advance past expiry.
        *state.lock().unwrap() += Duration::seconds(2);
        let err = k.use_token(&tok).unwrap_err();
        assert_eq!(err.kind, HandlerErrorKind::LeaseExpired);
        assert_eq!(err.blame, "CT-2");
    }

    #[test]
    fn release_policy_silently_retires_after_decay() {
        let (clock, state) = mock_clock();
        let mut k = LeaseKernel::with_clock(clock);
        let r = mk_resource("Db", "linear");
        let l = mk_lease("L", "Db", "1s", "release");
        let tok = k.acquire(&l, &r).unwrap();
        *state.lock().unwrap() += Duration::seconds(2);
        let outcome = k.use_token(&tok).unwrap();
        assert!(matches!(outcome, UseOutcome::Released));
        assert!(!k.contains(&tok.token_id));
    }

    #[test]
    fn extend_policy_mints_fresh_token_and_revokes_old() {
        let (clock, state) = mock_clock();
        let mut k = LeaseKernel::with_clock(clock);
        let r = mk_resource("Db", "linear");
        let l = mk_lease("L", "Db", "1s", "extend");
        let tok = k.acquire(&l, &r).unwrap();
        let first_id = tok.token_id.clone();
        *state.lock().unwrap() += Duration::seconds(2);
        let outcome = k.use_token(&tok).unwrap();
        match outcome {
            UseOutcome::Extended(new_tok) => {
                assert_ne!(new_tok.token_id, first_id);
                assert_eq!(new_tok.lease_name, "L");
                // Old token is revoked, so using it again should error.
                let err = k.use_token(&tok).unwrap_err();
                assert_eq!(err.kind, HandlerErrorKind::Caller);
            }
            other => panic!("expected Extended, got {other:?}"),
        }
    }

    #[test]
    fn release_is_idempotent() {
        let mut k = LeaseKernel::new();
        let r = mk_resource("Db", "linear");
        let l = mk_lease("L", "Db", "30s", "release");
        let tok = k.acquire(&l, &r).unwrap();
        k.release(&tok);
        k.release(&tok); // second call must not panic
        assert!(!k.contains(&tok.token_id));
    }

    #[test]
    fn sweep_removes_only_expired_tokens() {
        let (clock, state) = mock_clock();
        let mut k = LeaseKernel::with_clock(clock);
        let r = mk_resource("Db", "affine");
        let l_short = mk_lease("S", "Db", "1s", "release");
        let l_long = mk_lease("L", "Db", "1h", "release");
        let s = k.acquire(&l_short, &r).unwrap();
        let _l = k.acquire(&l_long, &r).unwrap();
        *state.lock().unwrap() += Duration::seconds(2);
        let expired = k.sweep();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].token_id, s.token_id);
        assert_eq!(k.active().len(), 1);
    }

    #[test]
    fn envelope_decays_to_zero_after_expiry() {
        let start: DateTime<Utc> = "2026-04-20T12:00:00Z".parse().unwrap();
        let tok = LeaseToken {
            token_id: "lease-x".into(),
            lease_name: "L".into(),
            resource_ref: "Db".into(),
            acquired_at: start,
            expires_at: start + Duration::seconds(30),
            on_expire: "anchor_breach".into(),
        };
        assert_eq!(tok.envelope(start).c, 1.0);
        assert_eq!(tok.envelope(start + Duration::seconds(60)).c, 0.0);
    }

    // Keeps the `Cell` import alive if future tests need it.
    #[allow(dead_code)]
    fn _unused_cell_probe() { let _ = Cell::new(0u32); }
}
