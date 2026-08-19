//! v2.28.0 — the RateLease budget kernel.
//!
//! The runtime for `budget { rate/max … on Tool(X) }` (v2.28.0). A [`RateLease`] is
//! the **refilling generalization** of the [`lease_kernel`](crate::runtime::lease_kernel)'s
//! τ-decay affine `LeaseToken`: where a `LeaseToken` is single-use and DECAYS to
//! nothing, a `RateLease` is N-use and REFILLS — but the linearity invariant is
//! the same, *a consumed token is gone until it is refilled*. This is what makes
//! "no more than N external effects per period" a real linear contract rather
//! than an advisory counter (the v2.28.0 doctrine `effects_are_linear`).
//!
//! Two quota kinds, both PURE functions of `(lease state, now)`:
//!
//!   * `rate:` → a **token bucket** of capacity `limit`, refilling continuously
//!     at `limit / period` tokens per second (so it permits a burst up to
//! `limit`, then a steady rate). The v2.28.0 default daemon starts full.
//!   * `max:`  → a **fixed tumbling window**: at most `limit` consumptions per
//!     `period`; the window rolls (counter resets) once `period` has elapsed
//!     since it opened. No intra-window refill — a hard cap.
//!
//! Refill/roll is LAZY: every `try_acquire` brings the lease current from the
//! elapsed wall-clock, so the decision never depends on a background tick's
//! granularity. [`RateLeaseKernel::tick`] is housekeeping (keeps `available`
//! queries fresh + reaps), the refilling analogue of the lease kernel's `sweep`
//! / the reconcile loop's periodic pass.

#![allow(dead_code)]

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};

use crate::ir_nodes::IRBudgetQuota;

// ═══════════════════════════════════════════════════════════════════
//  PERIOD — the closed catalog (mirrors axon-T832)
// ═══════════════════════════════════════════════════════════════════

/// A budget quota's renewal/window period. Closed catalog — the type checker
/// (`axon-T832`) already rejected anything else at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetPeriod {
    Second,
    Minute,
    Hour,
    Day,
}

impl BudgetPeriod {
    /// Parse the closed catalog spelling. `None` for an unknown period (the
    /// type-checker guarantees this does not happen for compiled programs).
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "second" => BudgetPeriod::Second,
            "minute" => BudgetPeriod::Minute,
            "hour" => BudgetPeriod::Hour,
            "day" => BudgetPeriod::Day,
            _ => return None,
        })
    }

    /// The period length in seconds.
    pub fn as_secs(self) -> f64 {
        match self {
            BudgetPeriod::Second => 1.0,
            BudgetPeriod::Minute => 60.0,
            BudgetPeriod::Hour => 3600.0,
            BudgetPeriod::Day => 86400.0,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
//  ACQUIRE OUTCOME
// ═══════════════════════════════════════════════════════════════════

/// The result of attempting to consume one token from a [`RateLease`]. A pure
/// function of the lease's state + `now`.
#[derive(Debug, Clone, PartialEq)]
pub enum AcquireOutcome {
    /// A token was consumed — the budgeted effect MAY proceed.
    Granted,
    /// The quota is exhausted. `retry_at` is the earliest instant a token will
    /// be available again (the input to `on_exhausted: defer`'s reschedule).
    Denied { retry_at: DateTime<Utc> },
}

impl AcquireOutcome {
    pub fn is_granted(&self) -> bool {
        matches!(self, AcquireOutcome::Granted)
    }
}

// ═══════════════════════════════════════════════════════════════════
//  RATE LEASE
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
enum RateState {
    /// `rate:` — a refilling token bucket. `tokens` is fractional (continuous
    /// refill); `last_refill` is the watermark elapsed-time is measured from.
    Bucket { tokens: f64, last_refill: DateTime<Utc> },
    /// `max:` — a fixed tumbling window. `consumed` resets when the window rolls.
    Window { window_start: DateTime<Utc>, consumed: i64 },
}

/// One quota's live state: a refilling bucket (`rate:`) or a fixed window
/// (`max:`). Construct via [`RateLease::rate`] / [`RateLease::max`] /
/// [`RateLease::from_quota`]. Consume via [`RateLease::try_acquire`].
#[derive(Debug, Clone)]
pub struct RateLease {
    /// The declared tool this quota governs (`on Tool(effect)`).
    pub effect: String,
    /// The token allowance per period (> 0).
    pub limit: i64,
    /// The renewal/window period.
    pub period: BudgetPeriod,
    state: RateState,
}

impl RateLease {
    /// A `rate:` quota — a token bucket starting FULL (`limit` tokens), refilling
    /// `limit` tokens per `period`.
    pub fn rate(effect: impl Into<String>, limit: i64, period: BudgetPeriod, now: DateTime<Utc>) -> Self {
        RateLease {
            effect: effect.into(),
            limit,
            period,
            state: RateState::Bucket { tokens: limit.max(0) as f64, last_refill: now },
        }
    }

    /// A `max:` quota — a fixed window of `period`, starting empty (0 consumed).
    pub fn max(effect: impl Into<String>, limit: i64, period: BudgetPeriod, now: DateTime<Utc>) -> Self {
        RateLease {
            effect: effect.into(),
            limit,
            period,
            state: RateState::Window { window_start: now, consumed: 0 },
        }
    }

    /// Build a lease from a compiled [`IRBudgetQuota`]. `None` if the period is
    /// not in the closed catalog (the type checker prevents this for compiled
    /// programs; the caller fail-closes defensively).
    pub fn from_quota(q: &IRBudgetQuota, now: DateTime<Utc>) -> Option<Self> {
        let period = BudgetPeriod::parse(&q.period)?;
        Some(match q.kind.as_str() {
            "max" => RateLease::max(q.effect.clone(), q.limit, period, now),
            // "rate" (and defensively any other kind) → a refilling bucket.
            _ => RateLease::rate(q.effect.clone(), q.limit, period, now),
        })
    }

    /// The refill-per-second rate for a bucket (`limit / period_secs`).
    fn refill_per_sec(&self) -> f64 {
        self.limit.max(0) as f64 / self.period.as_secs()
    }

    /// Bring the lease current as of `now` (refill the bucket / roll the window).
    /// Idempotent at a fixed `now`; pure given the prior state.
    pub fn refill(&mut self, now: DateTime<Utc>) {
        let rate = self.refill_per_sec();
        let capacity = self.limit.max(0) as f64;
        let period_secs = self.period.as_secs();
        match &mut self.state {
            RateState::Bucket { tokens, last_refill } => {
                let elapsed = (now - *last_refill).num_milliseconds() as f64 / 1000.0;
                if elapsed > 0.0 {
                    *tokens = (*tokens + elapsed * rate).min(capacity);
                    *last_refill = now;
                }
            }
            RateState::Window { window_start, consumed } => {
                let elapsed = (now - *window_start).num_milliseconds() as f64 / 1000.0;
                if elapsed >= period_secs {
                    *window_start = now;
                    *consumed = 0;
                }
            }
        }
    }

    /// Attempt to consume one token as of `now`. Refills/rolls first, then either
    /// consumes (→ [`AcquireOutcome::Granted`]) or denies with the next-available
    /// instant. PURE: same `(state, now)` ⇒ same outcome + same post-state.
    pub fn try_acquire(&mut self, now: DateTime<Utc>) -> AcquireOutcome {
        self.refill(now);
        let rate = self.refill_per_sec();
        let period_secs = self.period.as_secs();
        match &mut self.state {
            RateState::Bucket { tokens, .. } => {
                if *tokens >= 1.0 {
                    *tokens -= 1.0;
                    AcquireOutcome::Granted
                } else {
                    // Time until the bucket accrues the missing fraction of a token.
                    let deficit = 1.0 - *tokens;
                    let wait_secs = if rate > 0.0 { deficit / rate } else { f64::INFINITY };
                    let retry_at = now + secs_to_duration(wait_secs);
                    AcquireOutcome::Denied { retry_at }
                }
            }
            RateState::Window { window_start, consumed } => {
                if *consumed < self.limit {
                    *consumed += 1;
                    AcquireOutcome::Granted
                } else {
                    let retry_at = *window_start + secs_to_duration(period_secs);
                    AcquireOutcome::Denied { retry_at }
                }
            }
        }
    }

    /// The number of tokens currently available (after refilling to `now`).
    /// Whole tokens for a window; fractional for a bucket.
    pub fn available(&self, now: DateTime<Utc>) -> f64 {
        let mut probe = self.clone();
        probe.refill(now);
        match probe.state {
            RateState::Bucket { tokens, .. } => tokens,
            RateState::Window { consumed, .. } => (self.limit - consumed).max(0) as f64,
        }
    }

    /// Whether a token is available at `now` WITHOUT consuming it. `Granted` if a
    /// call would succeed; `Denied{retry_at}` otherwise. Used by the multi-quota
    /// gate to test all-or-none before committing any consumption.
    pub fn peek(&self, now: DateTime<Utc>) -> AcquireOutcome {
        let mut probe = self.clone();
        probe.try_acquire(now)
    }

    /// v2.28.0 — capture this lease's live STATE as a serializable snapshot
    /// (epoch-millis, no chrono in the wire form). The enterprise daemon
    /// supervisor persists it so a `max` window / `rate` bucket is cumulative
    /// ACROSS ticks (a daily cap spans the day's ticks). The v2.4.0 fire-once claim
    /// serializes a daemon's ticks, so load → run → save needs no lock.
    pub fn snapshot(&self) -> RateLeaseSnapshot {
        match &self.state {
            RateState::Bucket { tokens, last_refill } => RateLeaseSnapshot {
                kind: "rate".to_string(),
                tokens: *tokens,
                last_refill_ms: last_refill.timestamp_millis(),
                window_start_ms: 0,
                consumed: 0,
            },
            RateState::Window { window_start, consumed } => RateLeaseSnapshot {
                kind: "max".to_string(),
                tokens: 0.0,
                last_refill_ms: 0,
                window_start_ms: window_start.timestamp_millis(),
                consumed: *consumed,
            },
        }
    }

    /// v2.28.0 — restore this lease's STATE from a snapshot (the inverse of
    /// [`snapshot`](Self::snapshot)). A kind mismatch (a `rate` lease restored
    /// from a `max` snapshot — e.g. the budget grammar changed between ticks) is
    /// IGNORED, leaving the freshly-built state (fail-safe: a re-budgeted daemon
    /// starts clean rather than mis-restoring).
    pub fn restore(&mut self, snap: &RateLeaseSnapshot) {
        match (&mut self.state, snap.kind.as_str()) {
            (RateState::Bucket { tokens, last_refill }, "rate") => {
                *tokens = snap.tokens.min(self.limit.max(0) as f64);
                if let Some(t) = DateTime::from_timestamp_millis(snap.last_refill_ms) {
                    *last_refill = t;
                }
            }
            (RateState::Window { window_start, consumed }, "max") => {
                *consumed = snap.consumed.clamp(0, self.limit.max(0));
                if let Some(t) = DateTime::from_timestamp_millis(snap.window_start_ms) {
                    *window_start = t;
                }
            }
            _ => { /* kind mismatch ⇒ keep the fresh state */ }
        }
    }
}

/// v2.28.0 — a [`RateLease`]'s persistable state (epoch-millis wire form). The
/// enterprise supervisor stores one per quota subject key so budgets are
/// cumulative across a daemon's ticks. `kind` discriminates which fields are
/// live (`rate` ⇒ `tokens`/`last_refill_ms`; `max` ⇒ `window_start_ms`/`consumed`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RateLeaseSnapshot {
    pub kind: String,
    pub tokens: f64,
    pub last_refill_ms: i64,
    pub window_start_ms: i64,
    pub consumed: i64,
}

/// Convert fractional seconds to a chrono [`Duration`] (millisecond precision;
/// non-finite / negative values clamp to zero).
fn secs_to_duration(secs: f64) -> Duration {
    if !secs.is_finite() || secs <= 0.0 {
        return Duration::zero();
    }
    // Cap at ~100 days to avoid i64-millis overflow on an infinite-ish wait.
    let capped = secs.min(8_640_000.0);
    Duration::milliseconds((capped * 1000.0) as i64)
}

// ═══════════════════════════════════════════════════════════════════
//  RATE LEASE KERNEL — the in-process registry (OSS single-replica)
// ═══════════════════════════════════════════════════════════════════

/// An in-process registry of [`RateLease`]s, keyed by an opaque subject string
/// (the v2.28.0 dispatch gate composes the key from the budget's scope + the
/// effect + the quota kind, e.g. `"daemon:Outbound:Tool(TelnyxCall):rate"`). This
/// is the OSS single-replica reference; the v2.28.0 enterprise layer binds the
/// per-tenant Redis `RateLimiter` for multi-replica enforcement.
#[derive(Default)]
pub struct RateLeaseKernel {
    leases: HashMap<String, RateLease>,
}

impl RateLeaseKernel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) the lease for `key`.
    pub fn register(&mut self, key: impl Into<String>, lease: RateLease) {
        self.leases.insert(key.into(), lease);
    }

    /// Whether a lease is registered for `key`.
    pub fn contains(&self, key: &str) -> bool {
        self.leases.contains_key(key)
    }

    /// Attempt to consume one token from the lease at `key`. A `key` with no
    /// registered lease is **unbudgeted** ⇒ always [`AcquireOutcome::Granted`]
    /// (an effect with no declared quota is not rate-limited).
    pub fn try_acquire(&mut self, key: &str, now: DateTime<Utc>) -> AcquireOutcome {
        match self.leases.get_mut(key) {
            Some(lease) => lease.try_acquire(now),
            None => AcquireOutcome::Granted,
        }
    }

    /// Attempt to consume one token from EVERY lease in `keys`, **all-or-none**:
    /// consumes from all only if all have a token (so a per-hour `rate` and a
    /// per-day `max` on the same tool both gate the call without a partial
    /// consumption when one is exhausted). Returns [`AcquireOutcome::Denied`] with
    /// the LATEST `retry_at` among the exhausted leases (the binding constraint —
    /// you must wait for the slowest). An empty `keys` (an unbudgeted effect) is
    /// granted. Unknown keys are skipped (treated as unbudgeted).
    pub fn try_acquire_all(&mut self, keys: &[String], now: DateTime<Utc>) -> AcquireOutcome {
        // Phase 1 — peek every lease; collect the binding retry_at on any denial.
        let mut latest_retry: Option<DateTime<Utc>> = None;
        for key in keys {
            if let Some(lease) = self.leases.get(key) {
                if let AcquireOutcome::Denied { retry_at } = lease.peek(now) {
                    latest_retry = Some(match latest_retry {
                        Some(prev) if prev >= retry_at => prev,
                        _ => retry_at,
                    });
                }
            }
        }
        if let Some(retry_at) = latest_retry {
            return AcquireOutcome::Denied { retry_at };
        }
        // Phase 2 — all available ⇒ consume from each (cannot deny now).
        for key in keys {
            if let Some(lease) = self.leases.get_mut(key) {
                let _ = lease.try_acquire(now);
            }
        }
        AcquireOutcome::Granted
    }

    /// Tokens currently available at `key` (`None` if unregistered).
    pub fn available(&self, key: &str, now: DateTime<Utc>) -> Option<f64> {
        self.leases.get(key).map(|l| l.available(now))
    }

    /// Housekeeping pass — refill/roll every registered lease to `now` so
    /// `available` snapshots are fresh. The refilling analogue of the lease
    /// kernel's `sweep`. Acquisition does not depend on this (refill is lazy).
    pub fn tick(&mut self, now: DateTime<Utc>) {
        for lease in self.leases.values_mut() {
            lease.refill(now);
        }
    }

    /// v2.28.0 — snapshot every lease's state as `(key, snapshot)` pairs for
    /// persistence. Deterministic order (sorted by key).
    pub fn snapshot(&self) -> Vec<(String, RateLeaseSnapshot)> {
        let mut out: Vec<(String, RateLeaseSnapshot)> =
            self.leases.iter().map(|(k, l)| (k.clone(), l.snapshot())).collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// v2.28.0 — restore lease states from `(key, snapshot)` pairs. Keys with
    /// no registered lease are skipped (a quota dropped from a re-budgeted daemon).
    pub fn restore(&mut self, snaps: &[(String, RateLeaseSnapshot)]) {
        for (key, snap) in snaps {
            if let Some(lease) = self.leases.get_mut(key) {
                lease.restore(snap);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// BUDGET GATE — the v2.28.0 dispatch-site decision over a daemon's budget
// ═══════════════════════════════════════════════════════════════════

/// The dispatch gate's verdict for one budgeted effect emission.
#[derive(Debug, Clone, PartialEq)]
pub enum GateDecision {
    /// A token was consumed from every quota on the effect — the call proceeds.
    Allow,
    /// At least one quota is exhausted. The caller applies `on_exhausted`:
    /// `block` (fail the step), `defer` (reschedule to `retry_at`, v2.28.0), or
    /// `shed` (skip the call, v2.28.0).
    Deny {
        retry_at: DateTime<Utc>,
        on_exhausted: String,
    },
}

/// v2.28.0 — a daemon's compiled `budget { … }` as a runnable gate. Holds one
/// [`RateLease`] per quota (keyed by effect + kind), the `on_exhausted` policy,
/// and an effect→keys index so the dispatch site can gate a tool emission by
/// name. Built once when a budgeted daemon starts running its flow; the OSS
/// reference is single-process (the v2.28.0 enterprise layer swaps the in-process
/// kernel for the per-tenant Redis `RateLimiter` behind the same `gate` shape).
pub struct BudgetGate {
    kernel: RateLeaseKernel,
    on_exhausted: String,
    /// effect (tool name) → the subject keys of its quotas.
    by_effect: HashMap<String, Vec<String>>,
}

impl BudgetGate {
    /// Build a gate from a compiled [`crate::ir_nodes::IRBudget`]. `scope` is an
    /// opaque prefix (e.g. the daemon name) that namespaces the subject keys.
    /// An invalid-period quota (the type checker prevents this) is skipped.
    pub fn from_ir(budget: &crate::ir_nodes::IRBudget, scope: &str, now: DateTime<Utc>) -> Self {
        let mut kernel = RateLeaseKernel::new();
        let mut by_effect: HashMap<String, Vec<String>> = HashMap::new();
        for (i, quota) in budget.quotas.iter().enumerate() {
            let Some(lease) = RateLease::from_quota(quota, now) else {
                continue;
            };
            let key = format!("{scope}:Tool({}):{}:{i}", quota.effect, quota.kind);
            kernel.register(key.clone(), lease);
            by_effect.entry(quota.effect.clone()).or_default().push(key);
        }
        BudgetGate {
            kernel,
            on_exhausted: if budget.on_exhausted.is_empty() {
                "block".to_string()
            } else {
                budget.on_exhausted.clone()
            },
            by_effect,
        }
    }

    /// v2.69.0 — fold another gate into this one.
    ///
    /// A program may declare several top-level `budget`s. They compose into one
    /// gate, and the composition **may only ever tighten**:
    ///
    /// - **Quotas accumulate.** Subject keys are namespaced by scope, so they
    ///   cannot collide. Two budgets over the *same* tool means **both** must
    ///   grant — `gate()` is all-or-none over an effect's quotas. You cannot
    ///   satisfy one quota by ignoring another.
    /// - **The STRICTEST `on_exhausted` wins** (`block` > `defer` > `shed`).
    ///
    /// That second rule is the load-bearing one. If a lax budget could soften a
    /// strict one, then **adding a budget could widen what the program is allowed
    /// to do** — and a quota whose presence increases your permissions is not a
    /// quota. Merging must never be a way to buy leniency.
    pub fn merged_with(mut self, other: BudgetGate) -> BudgetGate {
        for (key, lease) in other.kernel.leases {
            self.kernel.leases.insert(key, lease);
        }
        for (effect, keys) in other.by_effect {
            self.by_effect.entry(effect).or_default().extend(keys);
        }
        // Strictest wins. `block` fails closed; `defer` reschedules; `shed` skips.
        let strictness = |p: &str| match p {
            "block" => 2,
            "defer" => 1,
            _ => 0, // `shed` — and any unknown policy, which must never be the winner
        };
        if strictness(&other.on_exhausted) > strictness(&self.on_exhausted) {
            self.on_exhausted = other.on_exhausted;
        }
        self
    }

    /// Gate one emission of `effect` (a tool name) at `now`. An effect with no
    /// quota is [`GateDecision::Allow`] (unbudgeted). Otherwise all of its quotas
    /// must grant (all-or-none); on exhaustion the daemon's `on_exhausted` policy
    /// rides on the [`GateDecision::Deny`].
    pub fn gate(&mut self, effect: &str, now: DateTime<Utc>) -> GateDecision {
        let Some(keys) = self.by_effect.get(effect) else {
            return GateDecision::Allow;
        };
        let keys = keys.clone();
        match self.kernel.try_acquire_all(&keys, now) {
            AcquireOutcome::Granted => GateDecision::Allow,
            AcquireOutcome::Denied { retry_at } => GateDecision::Deny {
                retry_at,
                on_exhausted: self.on_exhausted.clone(),
            },
        }
    }

    /// The exhaustion policy (`block` | `defer` | `shed`).
    pub fn on_exhausted(&self) -> &str {
        &self.on_exhausted
    }

    /// Whether `effect` has any quota under this budget.
    pub fn governs(&self, effect: &str) -> bool {
        self.by_effect.contains_key(effect)
    }

    /// v2.28.0 — snapshot the gate's cumulative state for persistence (the
    /// enterprise supervisor saves this after a tick + restores it before the
    /// next, so a `max: 50 per day` spans the day's ticks).
    pub fn snapshot(&self) -> Vec<(String, RateLeaseSnapshot)> {
        self.kernel.snapshot()
    }

    /// v2.28.0 — restore the gate's state from a prior [`snapshot`](Self::snapshot).
    pub fn restore(&mut self, snaps: &[(String, RateLeaseSnapshot)]) {
        self.kernel.restore(snaps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> DateTime<Utc> {
        "2026-06-29T00:00:00Z".parse().unwrap()
    }

    fn quota(kind: &str, limit: i64, period: &str, effect: &str) -> IRBudgetQuota {
        IRBudgetQuota {
            kind: kind.into(),
            limit,
            period: period.into(),
            effect: effect.into(),
        }
    }

    fn ir_budget(quotas: Vec<IRBudgetQuota>, on_exhausted: &str) -> crate::ir_nodes::IRBudget {
        crate::ir_nodes::IRBudget {
            node_type: "budget",
            source_line: 1,
            source_column: 1,
            name: String::new(),
            quotas,
            on_exhausted: on_exhausted.into(),
        }
    }

    // ── BudgetPeriod ─────────────────────────────────────────────────────

    #[test]
    fn period_parses_and_maps_to_seconds() {
        assert_eq!(BudgetPeriod::parse("hour"), Some(BudgetPeriod::Hour));
        assert_eq!(BudgetPeriod::parse("fortnight"), None);
        assert_eq!(BudgetPeriod::Second.as_secs(), 1.0);
        assert_eq!(BudgetPeriod::Minute.as_secs(), 60.0);
        assert_eq!(BudgetPeriod::Hour.as_secs(), 3600.0);
        assert_eq!(BudgetPeriod::Day.as_secs(), 86400.0);
    }

    // ── rate: token bucket ───────────────────────────────────────────────

    #[test]
    fn bucket_starts_full_and_grants_up_to_capacity() {
        let now = t0();
        let mut l = RateLease::rate("Telnyx", 3, BudgetPeriod::Hour, now);
        // A burst of 3 succeeds (starts full)...
        assert!(l.try_acquire(now).is_granted());
        assert!(l.try_acquire(now).is_granted());
        assert!(l.try_acquire(now).is_granted());
        // ...the 4th is denied at the same instant (bucket empty).
        match l.try_acquire(now) {
            AcquireOutcome::Denied { retry_at } => {
                // 3/hour ⇒ one token every 1200s; deficit is a full token.
                assert_eq!(retry_at, now + Duration::seconds(1200));
            }
            other => panic!("expected Denied, got {other:?}"),
        }
    }

    #[test]
    fn bucket_refills_over_time() {
        let now = t0();
        let mut l = RateLease::rate("Telnyx", 2, BudgetPeriod::Hour, now);
        // Drain both tokens.
        assert!(l.try_acquire(now).is_granted());
        assert!(l.try_acquire(now).is_granted());
        assert!(!l.try_acquire(now).is_granted());
        // 2/hour ⇒ 1 token per 1800s. After 1800s, exactly one token is back.
        let later = now + Duration::seconds(1800);
        assert!(l.try_acquire(later).is_granted());
        assert!(!l.try_acquire(later).is_granted());
    }

    #[test]
    fn bucket_refill_is_capped_at_capacity() {
        let now = t0();
        let mut l = RateLease::rate("Telnyx", 5, BudgetPeriod::Minute, now);
        // Drain one, then wait a full day — refill must NOT exceed capacity.
        assert!(l.try_acquire(now).is_granted());
        let way_later = now + Duration::days(1);
        // Only `limit` (5) grants available, not days' worth.
        for _ in 0..5 {
            assert!(l.try_acquire(way_later).is_granted());
        }
        assert!(!l.try_acquire(way_later).is_granted(), "capped at capacity");
    }

    // ── max: fixed window ────────────────────────────────────────────────

    #[test]
    fn window_caps_at_limit_then_rolls() {
        let now = t0();
        let mut l = RateLease::max("Telnyx", 50, BudgetPeriod::Day, now);
        // 50 calls in the window succeed.
        for _ in 0..50 {
            assert!(l.try_acquire(now).is_granted());
        }
        // The 51st is denied; retry at the window roll (24h later).
        match l.try_acquire(now) {
            AcquireOutcome::Denied { retry_at } => {
                assert_eq!(retry_at, now + Duration::seconds(86400));
            }
            other => panic!("expected Denied, got {other:?}"),
        }
        // Just before the roll → still denied.
        assert!(!l.try_acquire(now + Duration::seconds(86399)).is_granted());
        // After the roll → the window resets, calls succeed again.
        let next_day = now + Duration::seconds(86400);
        assert!(l.try_acquire(next_day).is_granted());
        assert_eq!(l.available(next_day), 49.0);
    }

    #[test]
    fn window_has_no_intra_window_refill() {
        let now = t0();
        let mut l = RateLease::max("Telnyx", 2, BudgetPeriod::Hour, now);
        assert!(l.try_acquire(now).is_granted());
        assert!(l.try_acquire(now).is_granted());
        // Halfway through the window, still capped (no continuous refill).
        assert!(!l.try_acquire(now + Duration::seconds(1800)).is_granted());
    }

    // ── from_quota + available ───────────────────────────────────────────

    #[test]
    fn from_quota_builds_the_right_kind() {
        let now = t0();
        let rate_q = IRBudgetQuota {
            kind: "rate".into(),
            limit: 8,
            period: "hour".into(),
            effect: "Telnyx".into(),
        };
        let max_q = IRBudgetQuota {
            kind: "max".into(),
            limit: 50,
            period: "day".into(),
            effect: "Telnyx".into(),
        };
        let rate = RateLease::from_quota(&rate_q, now).unwrap();
        assert_eq!(rate.available(now), 8.0, "a rate bucket starts full");
        let maxl = RateLease::from_quota(&max_q, now).unwrap();
        assert_eq!(maxl.available(now), 50.0, "a max window starts with the full allowance");
        // An invalid period fails closed to None.
        let bad = IRBudgetQuota { period: "fortnight".into(), ..rate_q };
        assert!(RateLease::from_quota(&bad, now).is_none());
    }

    // ── kernel ───────────────────────────────────────────────────────────

    #[test]
    fn kernel_unregistered_key_is_unbudgeted() {
        let mut k = RateLeaseKernel::new();
        // No lease for the key ⇒ an effect with no quota is never limited.
        assert!(k.try_acquire("daemon:X:Tool(Y):rate", t0()).is_granted());
        assert_eq!(k.available("daemon:X:Tool(Y):rate", t0()), None);
    }

    #[test]
    fn kernel_enforces_a_registered_lease() {
        let now = t0();
        let mut k = RateLeaseKernel::new();
        k.register("d:Out:Tool(Telnyx):rate", RateLease::rate("Telnyx", 1, BudgetPeriod::Hour, now));
        assert!(k.try_acquire("d:Out:Tool(Telnyx):rate", now).is_granted());
        assert!(!k.try_acquire("d:Out:Tool(Telnyx):rate", now).is_granted());
        // After a full hour, the single token is back.
        let later = now + Duration::seconds(3600);
        assert!(k.try_acquire("d:Out:Tool(Telnyx):rate", later).is_granted());
    }

    #[test]
    fn kernel_tick_refreshes_available_without_consuming() {
        let now = t0();
        let mut k = RateLeaseKernel::new();
        k.register("k", RateLease::rate("E", 4, BudgetPeriod::Minute, now));
        // Drain to empty.
        for _ in 0..4 {
            assert!(k.try_acquire("k", now).is_granted());
        }
        assert_eq!(k.available("k", now), Some(0.0));
        // 30s = half a minute ⇒ 4/min * 30s = 2 tokens refilled by tick.
        let later = now + Duration::seconds(30);
        k.tick(later);
        assert_eq!(k.available("k", later), Some(2.0));
    }

    // ── try_acquire_all — the all-or-none multi-quota gate ───────────────

    #[test]
    fn acquire_all_is_all_or_none() {
        let now = t0();
        let mut k = RateLeaseKernel::new();
        // rate: 5/hour (plenty) + max: 1/day (the binding constraint).
        k.register("r", RateLease::rate("E", 5, BudgetPeriod::Hour, now));
        k.register("m", RateLease::max("E", 1, BudgetPeriod::Day, now));
        let keys = vec!["r".to_string(), "m".to_string()];
        // First call: both grant.
        assert!(k.try_acquire_all(&keys, now).is_granted());
        // Second: max is exhausted → DENIED, and the rate token must NOT have
        // been consumed (all-or-none) — 4 still available on the bucket.
        match k.try_acquire_all(&keys, now) {
            AcquireOutcome::Denied { retry_at } => {
                assert_eq!(retry_at, now + Duration::seconds(86400), "binding = the daily max");
            }
            other => panic!("expected Denied, got {other:?}"),
        }
        assert_eq!(k.available("r", now), Some(4.0), "rate token not consumed on denial");
    }

    #[test]
    fn acquire_all_empty_keys_is_granted() {
        let mut k = RateLeaseKernel::new();
        assert!(k.try_acquire_all(&[], t0()).is_granted());
    }

    // ── BudgetGate ───────────────────────────────────────────────────────

    #[test]
    fn gate_allows_unbudgeted_effects() {
        let now = t0();
        let b = ir_budget(vec![quota("rate", 1, "hour", "Telnyx")], "block");
        let mut gate = BudgetGate::from_ir(&b, "daemon:Out", now);
        // A tool with no quota is unbudgeted → always allowed.
        assert_eq!(gate.gate("SomeOtherTool", now), GateDecision::Allow);
        assert!(!gate.governs("SomeOtherTool"));
        assert!(gate.governs("Telnyx"));
    }

    #[test]
    fn gate_enforces_then_denies_with_policy() {
        let now = t0();
        let b = ir_budget(
            vec![
                quota("rate", 2, "hour", "Telnyx"),
                quota("max", 3, "day", "Telnyx"),
            ],
            "defer",
        );
        let mut gate = BudgetGate::from_ir(&b, "daemon:Out", now);
        // The rate bucket (2/hour) is the tighter constraint at t0.
        assert_eq!(gate.gate("Telnyx", now), GateDecision::Allow);
        assert_eq!(gate.gate("Telnyx", now), GateDecision::Allow);
        match gate.gate("Telnyx", now) {
            GateDecision::Deny { on_exhausted, retry_at } => {
                assert_eq!(on_exhausted, "defer");
                // 2/hour ⇒ next token in 1800s.
                assert_eq!(retry_at, now + Duration::seconds(1800));
            }
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn gate_omitted_policy_is_block() {
        let now = t0();
        let b = ir_budget(vec![quota("rate", 1, "hour", "E")], "");
        let gate = BudgetGate::from_ir(&b, "d", now);
        assert_eq!(gate.on_exhausted(), "block");
    }

    // ── v2.28.0 — snapshot / restore (cumulative across ticks) ─────────

    #[test]
    fn snapshot_restore_carries_max_window_across_ticks() {
        let now = t0();
        let b = ir_budget(vec![quota("max", 3, "day", "Telnyx")], "block");
        // Tick 1: a fresh gate consumes 2 of 3.
        let mut g1 = BudgetGate::from_ir(&b, "d", now);
        assert_eq!(g1.gate("Telnyx", now), GateDecision::Allow);
        assert_eq!(g1.gate("Telnyx", now), GateDecision::Allow);
        let snap = g1.snapshot();

        // Tick 2 (a later minute, possibly a different replica): a fresh gate
        // RESTORED from the snapshot has consumed=2 → only 1 left, NOT a full 3.
        let mut g2 = BudgetGate::from_ir(&b, "d", now + Duration::minutes(5));
        g2.restore(&snap);
        assert_eq!(g2.gate("Telnyx", now + Duration::minutes(5)), GateDecision::Allow);
        // The 4th overall consumption (2 in tick 1 + 2 here) is denied — the
        // daily cap is honoured ACROSS ticks.
        match g2.gate("Telnyx", now + Duration::minutes(5)) {
            GateDecision::Deny { .. } => {}
            other => panic!("expected the daily cap to hold across ticks, got {other:?}"),
        }
    }

    #[test]
    fn snapshot_round_trips_a_bucket() {
        let now = t0();
        let mut l = RateLease::rate("E", 8, BudgetPeriod::Hour, now);
        l.try_acquire(now); // tokens: 8 → 7
        let snap = l.snapshot();
        assert_eq!(snap.kind, "rate");
        let mut l2 = RateLease::rate("E", 8, BudgetPeriod::Hour, now);
        l2.restore(&snap);
        assert_eq!(l2.available(now), 7.0, "restored bucket carries the consumed token");
    }
}
