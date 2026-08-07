//! §Fase 119.d — `hibernate`: the flow HALTS, and that is the whole point.
//!
//! # The specification, and where it comes from
//!
//! `hibernate` has no paper. Its requirement is ECONOMIC, stated by the
//! founder: *when the agent hibernates it STOPS — it generates no compute
//! cost and no token cost.* README §"CPS Continuation Points" publishes the
//! mechanics: a deterministic continuation ID via
//! `SHA-256(flow_name ∥ event_name ∥ source_position)`, the executor
//! serializes the execution state **and halts**, and `resume(continuation_id)`
//! continues from the exact IR node.
//!
//! §111 F20 found the shipped behavior: `run_hibernate` returned
//! `"(hibernating …)"` **synchronously and the flow kept walking** — it kept
//! spending while claiming to sleep, which inverts the one guarantee the
//! primitive exists for.
//!
//! # What halting means here
//!
//! The walk loop (not the handler — the handler cannot see its own position
//! in the body) observes [`crate::flow_dispatcher::NodeOutcome::Hibernated`],
//! parks a [`ParkedFlow`] carrying the REMAINING top-level nodes plus the
//! binding snapshot and the resume seed, and **ends the run**. The task dies.
//! No stream is held open, no timer thread spins, no poll loop runs: the
//! parked state is bytes in the lot and nothing else. Expiry is enforced
//! LAZILY — at resume time and at lot access — precisely because a background
//! reaper would be standing compute, which is the cost this primitive
//! promises not to incur.
//!
//! # Resume
//!
//! `emit <Channel>(payload)` is the wake signal: after publishing,
//! `run_emit` calls [`take_resumable`] for the event name; each claimed
//! continuation is re-entered by the caller (the dispatcher owns the walk).
//! A resume AFTER the parked deadline is REFUSED — the timeout is honored by
//! making lateness impossible to ship, not by burning a timer while asleep.
//!
//! # What is in-process and what is not (said out loud)
//!
//! The default lot is in-memory (the `pem::InMemoryBackend` discipline: OSS
//! reference in-process, durable store is the enterprise layer's job —
//! `cognitive_states` already persists PEM snapshots there). [`ParkedFlow`]
//! is `Serialize`, so a durable lot has everything it needs; README's
//! "sleep for months" across process restarts is exactly that catch-up, and
//! the advertised entry names it rather than implying it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use sha2::{Digest, Sha256};

/// The README's published formula, verbatim:
/// `SHA-256(flow_name ∥ event_name ∥ source_position)`.
pub fn continuation_id(flow_name: &str, event_name: &str, line: u32, column: u32) -> String {
    let mut h = Sha256::new();
    h.update(flow_name.as_bytes());
    h.update(event_name.as_bytes());
    h.update(format!("{line}:{column}").as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Everything a continuation needs to re-enter the walk. `Serialize` so a
/// durable lot can persist it; the in-memory lot stores it typed.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ParkedFlow {
    pub continuation_id: String,
    pub flow_name: String,
    pub event_name: String,
    /// Unix ms after which resume is REFUSED. `None` = no timeout declared.
    pub deadline_ms: Option<i64>,
    /// The top-level nodes that come AFTER the hibernate point.
    pub remaining_nodes: Vec<crate::ir_nodes::IRFlowNode>,
    /// The binding snapshot at suspension.
    pub let_bindings: HashMap<String, String>,
    /// Step counter at suspension, so resumed step indices continue the run's
    /// numbering instead of restarting at zero.
    pub step_counter: usize,
    /// What the resumed walk needs to rebuild a DispatchCtx.
    pub backend_name: String,
    pub system_prompt: String,
    pub tenant_id: String,
    pub session_id: String,
    /// The declaration catalogs, carried by Arc so a resumed flow can still
    /// resolve mandates / lambdas / ots / computes (§119.b/c doctrine: an
    /// empty catalog fails closed, so losing these on resume would break
    /// every governed step after the sleep).
    #[serde(skip)]
    pub mandate_specs: Arc<Vec<crate::ir_nodes::IRMandate>>,
    #[serde(skip)]
    pub lambda_data_specs: Arc<Vec<crate::ir_nodes::IRLambdaData>>,
    #[serde(skip)]
    pub ots_specs: Arc<Vec<crate::ir_nodes::IROts>>,
    #[serde(skip)]
    pub compute_specs: Arc<Vec<crate::ir_nodes::IRCompute>>,
}

/// Why a continuation could not be claimed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeRefusal {
    /// No parked flow under this id (never parked, already resumed, or
    /// reaped after expiry).
    Unknown,
    /// The deadline passed. The timeout is honored HERE: lateness cannot be
    /// shipped, and the entry is reaped on refusal.
    Expired { deadline_ms: i64, now_ms: i64 },
}

impl std::fmt::Display for ResumeRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResumeRefusal::Unknown => write!(
                f,
                "no parked continuation under this id (never parked, already \
                 resumed, or reaped after expiry)"
            ),
            ResumeRefusal::Expired { deadline_ms, now_ms } => write!(
                f,
                "hibernation expired: the deadline was {deadline_ms} and it is \
                 now {now_ms}; a late resume would execute a continuation whose \
                 timeout already fired, so it is refused and the entry reaped"
            ),
        }
    }
}

/// The terminal record a resumed run leaves behind, inspectable by tests and
/// operators (the original client's stream ended at the halt, so the result
/// must land SOMEWHERE visible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumedOutcome {
    Completed { output: String },
    Failed { error: String },
}

#[derive(Default)]
struct LotInner {
    parked: HashMap<String, ParkedFlow>,
    /// event_name → continuation ids awaiting it.
    by_event: HashMap<String, Vec<String>>,
    /// Terminal results of resumed runs.
    outcomes: HashMap<String, ResumedOutcome>,
}

/// The in-process parking lot. One per process (the shield_registry
/// discipline); a durable implementation replaces the storage, not the
/// contract.
pub struct ParkingLot {
    inner: Mutex<LotInner>,
}

static LOT: OnceLock<ParkingLot> = OnceLock::new();

/// The process-global lot.
pub fn parking_lot() -> &'static ParkingLot {
    LOT.get_or_init(|| ParkingLot {
        inner: Mutex::new(LotInner::default()),
    })
}

impl ParkingLot {
    /// Park a continuation. Replaces an existing entry under the same id —
    /// the id is deterministic by construction (same flow, same event, same
    /// source position), so re-parking IS the same suspension point.
    pub fn park(&self, flow: ParkedFlow) {
        let mut g = self.inner.lock().expect("parking lot poisoned");
        g.by_event
            .entry(flow.event_name.clone())
            .or_default()
            .push(flow.continuation_id.clone());
        g.parked.insert(flow.continuation_id.clone(), flow);
    }

    /// Claim one continuation by id. Expiry enforced here, lazily.
    pub fn claim(&self, id: &str, now_ms: i64) -> Result<ParkedFlow, ResumeRefusal> {
        let mut g = self.inner.lock().expect("parking lot poisoned");
        let Some(p) = g.parked.get(id) else {
            return Err(ResumeRefusal::Unknown);
        };
        if let Some(d) = p.deadline_ms {
            if now_ms > d {
                let ev = p.event_name.clone();
                g.parked.remove(id);
                if let Some(v) = g.by_event.get_mut(&ev) {
                    v.retain(|x| x != id);
                }
                return Err(ResumeRefusal::Expired {
                    deadline_ms: d,
                    now_ms,
                });
            }
        }
        let p = g.parked.remove(id).expect("checked above");
        if let Some(v) = g.by_event.get_mut(&p.event_name) {
            v.retain(|x| x != id);
        }
        Ok(p)
    }

    /// Claim every unexpired continuation awaiting `event_name`. Expired
    /// entries encountered on the way are reaped (the lazy timeout firing).
    pub fn take_resumable(&self, event_name: &str, now_ms: i64) -> Vec<ParkedFlow> {
        let ids: Vec<String> = {
            let g = self.inner.lock().expect("parking lot poisoned");
            g.by_event.get(event_name).cloned().unwrap_or_default()
        };
        let mut out = Vec::new();
        for id in ids {
            if let Ok(p) = self.claim(&id, now_ms) {
                out.push(p);
            }
        }
        out
    }

    /// Record the terminal outcome of a resumed run.
    pub fn record_outcome(&self, id: &str, outcome: ResumedOutcome) {
        let mut g = self.inner.lock().expect("parking lot poisoned");
        g.outcomes.insert(id.to_string(), outcome);
    }

    /// Inspect a resumed run's terminal outcome.
    pub fn outcome(&self, id: &str) -> Option<ResumedOutcome> {
        let g = self.inner.lock().expect("parking lot poisoned");
        g.outcomes.get(id).cloned()
    }

    /// Whether a continuation is currently parked.
    pub fn is_parked(&self, id: &str) -> bool {
        let g = self.inner.lock().expect("parking lot poisoned");
        g.parked.contains_key(id)
    }
}

/// Parse the grammar's duration literal (`30s`, `5m`, `24h`, `7d`) to ms.
/// Unknown/empty → `None` (no deadline), never a fabricated default.
pub fn timeout_to_ms(timeout: &str) -> Option<i64> {
    let t = timeout.trim();
    if t.is_empty() {
        return None;
    }
    let (num, unit) = t.split_at(t.len() - 1);
    let n: i64 = num.parse().ok()?;
    match unit {
        "s" => Some(n * 1_000),
        "m" => Some(n * 60_000),
        "h" => Some(n * 3_600_000),
        "d" => Some(n * 86_400_000),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parked(id: &str, event: &str, deadline_ms: Option<i64>) -> ParkedFlow {
        ParkedFlow {
            continuation_id: id.into(),
            flow_name: "F".into(),
            event_name: event.into(),
            deadline_ms,
            remaining_nodes: Vec::new(),
            let_bindings: HashMap::new(),
            step_counter: 3,
            backend_name: "stub".into(),
            system_prompt: String::new(),
            tenant_id: String::new(),
            session_id: "s".into(),
            mandate_specs: Arc::new(Vec::new()),
            lambda_data_specs: Arc::new(Vec::new()),
            ots_specs: Arc::new(Vec::new()),
            compute_specs: Arc::new(Vec::new()),
        }
    }

    #[test]
    fn the_continuation_id_is_the_published_formula() {
        let a = continuation_id("Flow", "event", 10, 5);
        let b = continuation_id("Flow", "event", 10, 5);
        assert_eq!(a, b, "deterministic");
        assert_eq!(a.len(), 64, "SHA-256 hex");
        assert_ne!(a, continuation_id("Flow", "event", 10, 6), "position-sensitive");
        assert_ne!(a, continuation_id("Flow", "other", 10, 5), "event-sensitive");
    }

    #[test]
    fn park_claim_roundtrip_and_double_claim_refused() {
        let lot = ParkingLot { inner: Mutex::new(LotInner::default()) };
        lot.park(parked("id1", "ev", None));
        assert!(lot.is_parked("id1"));
        let p = lot.claim("id1", 1_000).expect("claimable");
        assert_eq!(p.step_counter, 3);
        assert_eq!(
            lot.claim("id1", 1_000).unwrap_err(),
            ResumeRefusal::Unknown,
            "a continuation resumes exactly once"
        );
    }

    #[test]
    fn a_late_resume_is_refused_and_the_entry_reaped() {
        let lot = ParkingLot { inner: Mutex::new(LotInner::default()) };
        lot.park(parked("id2", "ev", Some(5_000)));
        match lot.claim("id2", 6_000) {
            Err(ResumeRefusal::Expired { deadline_ms, now_ms }) => {
                assert_eq!((deadline_ms, now_ms), (5_000, 6_000));
            }
            other => panic!("expected Expired, got {other:?}"),
        }
        assert!(!lot.is_parked("id2"), "reaped on refusal");
        assert_eq!(lot.claim("id2", 4_000).unwrap_err(), ResumeRefusal::Unknown);
    }

    #[test]
    fn take_resumable_claims_matching_and_reaps_expired() {
        let lot = ParkingLot { inner: Mutex::new(LotInner::default()) };
        lot.park(parked("live", "quarterly", Some(10_000)));
        lot.park(parked("stale", "quarterly", Some(1_000)));
        lot.park(parked("other", "different_event", None));
        let got = lot.take_resumable("quarterly", 5_000);
        assert_eq!(got.len(), 1, "only the unexpired matching one");
        assert_eq!(got[0].continuation_id, "live");
        assert!(!lot.is_parked("stale"), "expired entry reaped in passing");
        assert!(lot.is_parked("other"), "unrelated event untouched");
    }

    #[test]
    fn timeout_literals_parse_and_garbage_is_none_never_a_default() {
        assert_eq!(timeout_to_ms("30s"), Some(30_000));
        assert_eq!(timeout_to_ms("5m"), Some(300_000));
        assert_eq!(timeout_to_ms("24h"), Some(86_400_000));
        assert_eq!(timeout_to_ms("7d"), Some(604_800_000));
        assert_eq!(timeout_to_ms(""), None);
        assert_eq!(timeout_to_ms("soon"), None, "garbage must not invent a deadline");
    }

    #[test]
    fn outcomes_are_recorded_and_inspectable() {
        let lot = ParkingLot { inner: Mutex::new(LotInner::default()) };
        lot.record_outcome("idX", ResumedOutcome::Completed { output: "done".into() });
        assert_eq!(
            lot.outcome("idX"),
            Some(ResumedOutcome::Completed { output: "done".into() })
        );
        assert_eq!(lot.outcome("nope"), None);
    }
}
