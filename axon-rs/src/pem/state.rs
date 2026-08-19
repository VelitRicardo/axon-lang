//! [`CognitiveState`] — snapshot-able representation of an agent's
//! mid-conversation posture.
//!
//! Floats encoded as Q32.32 fixed-point
//! -----------------------------------
//!
//! A naive f64 roundtrip through MessagePack / JSON / Postgres
//! `double precision` preserves the *nominal* value but not the
//! *bit pattern*. After three reconnects the density matrix drifts
//! by tens of ulps — small, but enough to shift downstream
//! sampling decisions. We sidestep the class of bug entirely by
//! quantising floats on the way in and de-quantising on the way
//! out: every float becomes a signed 64-bit integer with 32 bits
//! of fractional precision (Q32.32). The worst-case representable
//! error is `2^-32 ≈ 2.3e-10`, well below the noise floor of
//! anything a belief state cares about, and the roundtrip is
//! bit-identical by construction.
//!
//! Callers who need the full f64 dynamic range (rare in PEM —
//! belief entries live in `[0, 1]`) upgrade the representation per-
//! call by using [`MemoryEntry::metadata`] which carries arbitrary
//! JSON.

use chrono::{DateTime, SubsecRound, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 2^32 — the Q32.32 scale factor. Public because adopters may need
/// it when reading raw rows out of a backend that wasn't routed
/// through this crate's codec.
pub const Q32_32_SCALE: f64 = 4_294_967_296.0; // 1u64 << 32

/// Fixed-point wrapper for a single float. Serialises as a plain
/// signed integer to stay compact + exact in any transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FixedPoint(pub i64);

impl FixedPoint {
    /// Quantise an `f64` to Q32.32. Saturates at the representable
    /// range instead of panicking — `f64::INFINITY` becomes
    /// `i64::MAX`.
    pub fn from_f64(v: f64) -> Self {
        let scaled = v * Q32_32_SCALE;
        let clamped = scaled.clamp(i64::MIN as f64, i64::MAX as f64);
        FixedPoint(clamped as i64)
    }

    pub fn to_f64(self) -> f64 {
        (self.0 as f64) / Q32_32_SCALE
    }

    /// Element-wise quantisation for a vector. Convenience wrapper
    /// used by adopters feeding matrix rows into the density matrix.
    pub fn vec_from_f64(v: &[f64]) -> Vec<FixedPoint> {
        v.iter().copied().map(FixedPoint::from_f64).collect()
    }

    pub fn vec_to_f64(v: &[FixedPoint]) -> Vec<f64> {
        v.iter().copied().map(FixedPoint::to_f64).collect()
    }
}

impl From<f64> for FixedPoint {
    fn from(v: f64) -> Self {
        FixedPoint::from_f64(v)
    }
}

impl From<FixedPoint> for f64 {
    fn from(q: FixedPoint) -> Self {
        q.to_f64()
    }
}

/// A single short-term memory entry. Intentionally unopinionated
/// about what adopters store — `payload` is arbitrary JSON, and
/// `symbolic_refs` holds handles to external buffers (audio clip
/// IDs, document checksums) without embedding the bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub key: String,
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbolic_refs: Vec<String>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub stored_at: DateTime<Utc>,
}

/// The full snapshot the backend persists. Canonical shape —
/// adopters serialise this via JSON (the simplest wire format
/// consistent with the 10.g canonicaliser and the 11.c replay
/// tokens); future revisions can swap to MessagePack without
/// changing the structural guarantees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CognitiveState {
    /// Stable identifier of the originating WebSocket session.
    /// Clients presenting a reconnect must prove ownership via the
    /// companion [`crate::pem::continuity_token::ContinuityToken`].
    pub session_id: String,
    /// Tenant slug so multi-tenant deployments route state to the
    /// correct RLS-scoped backend. Also carried forward to the SAR
    /// exporter in 10.l.
    pub tenant_id: String,
    /// Flow-execution identifier — matches `ReplayToken.flow_id`
    /// from 11.c so an auditor can correlate snapshots with the
    /// replay stream.
    pub flow_id: String,
    /// Optional subject / user identifier the state belongs to.
    /// Consumed by the SAR exporter in 10.l; nullable when the flow
    /// ran under a service account.
    #[serde(default)]
    pub subject_user_id: Option<String>,

    /// The agent's probability amplitudes in row-major order. Q32.32
    /// fixed-point means the same matrix round-trips identically
    /// across N reconnects.
    pub density_matrix: Vec<Vec<FixedPoint>>,
    /// Free-form belief state the flow author chose to preserve.
    /// Structured so the replay executor can re-seed an identical
    /// posture after rehydration.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub belief_state: Value,
    /// Short-term memory. Capped in practice by the flow's
    /// `@reconnect_window` TTL; buffers referenced here should be
    /// symbolic, not bytes inline.
    #[serde(default)]
    pub short_term_memory: Vec<MemoryEntry>,

    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub last_updated_at: DateTime<Utc>,
}

impl CognitiveState {
    /// Build a minimal state for a fresh session.
    pub fn new(
        session_id: impl Into<String>,
        tenant_id: impl Into<String>,
        flow_id: impl Into<String>,
    ) -> Self {
        // v1.4.2 — JSON serialisation of `DateTime<Utc>` via
        // chrono's default Serialize impl produces RFC 3339 with
        // millisecond precision, so any sub-millisecond fraction in
        // `Utc::now()` is silently dropped on the way to disk and the
        // restored state compares unequal to the in-memory original.
        // Truncating to ms here aligns the in-memory representation
        // with the on-wire format, making persist→restore a strict
        // identity — the same invariant Q32.32 gives us for floats.
        let now = Utc::now().trunc_subsecs(3);
        CognitiveState {
            session_id: session_id.into(),
            tenant_id: tenant_id.into(),
            flow_id: flow_id.into(),
            subject_user_id: None,
            density_matrix: Vec::new(),
            belief_state: Value::Null,
            short_term_memory: Vec::new(),
            created_at: now,
            last_updated_at: now,
        }
    }

    /// Snapshot this state to opaque bytes suitable for a backend
    /// to store. JSON for 11.d — simple, debuggable, lets the
    /// Postgres adapter query into `state.density_matrix[...]`
    /// when auditors need it.
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("CognitiveState is always serialisable")
    }

    /// Reconstruct a state from bytes previously produced by
    /// [`encode`]. Returns an error on any decode failure — adopters
    /// surface the mismatch as a deliberate "cold start" rather
    /// than silently corrupting the session.
    pub fn decode(bytes: &[u8]) -> Result<Self, StateDecodeError> {
        serde_json::from_slice(bytes)
            .map_err(|e| StateDecodeError(e.to_string()))
    }
}

#[derive(Debug)]
pub struct StateDecodeError(pub String);

impl std::fmt::Display for StateDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cognitive state decode failed: {}", self.0)
    }
}

impl std::error::Error for StateDecodeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn fixed_ts() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap()
    }

    // ── FixedPoint ───────────────────────────────────────────────

    #[test]
    fn fixed_point_roundtrip_preserves_bit_pattern_across_n_hops() {
        let originals = [0.0_f64, 1e-5, 0.25, 0.5, 0.9999, 1.0];
        for v in originals {
            let q = FixedPoint::from_f64(v);
            let back = q.to_f64();
            // After N quantise/dequantise cycles, the value is
            // stable at the first cycle.
            let q2 = FixedPoint::from_f64(back);
            let back2 = q2.to_f64();
            assert_eq!(q, q2, "value {v} drifts between cycles");
            assert_eq!(back, back2);
        }
    }

    #[test]
    fn fixed_point_saturates_on_infinity() {
        let q_pos = FixedPoint::from_f64(f64::INFINITY);
        let q_neg = FixedPoint::from_f64(f64::NEG_INFINITY);
        assert_eq!(q_pos.0, i64::MAX);
        assert_eq!(q_neg.0, i64::MIN);
    }

    #[test]
    fn fixed_point_vec_helpers() {
        let v = vec![0.1, 0.25, 0.5];
        let q = FixedPoint::vec_from_f64(&v);
        let back = FixedPoint::vec_to_f64(&q);
        // Each element round-trips; the first cycle fixes the
        // representation so the second is identity.
        let q2 = FixedPoint::vec_from_f64(&back);
        assert_eq!(q, q2);
    }

    #[test]
    fn fixed_point_representable_precision_is_about_2e_minus_10() {
        // Two f64 values that differ by less than 2e-10 round-trip
        // to the same FixedPoint — this is the documented precision
        // ceiling; the test asserts it hasn't silently widened.
        let a = 0.5_f64;
        let b = a + 1e-11;
        assert_eq!(FixedPoint::from_f64(a), FixedPoint::from_f64(b));
    }

    // ── CognitiveState ────────────────────────────────────────────

    #[test]
    fn encode_decode_roundtrip() {
        let mut s = CognitiveState::new("sess-1", "alpha", "flow-1");
        s.created_at = fixed_ts();
        s.last_updated_at = fixed_ts();
        s.density_matrix = vec![FixedPoint::vec_from_f64(&[0.1, 0.9])];
        s.belief_state = json!({"confidence": 0.73});
        s.short_term_memory.push(MemoryEntry {
            key: "last_user_msg".into(),
            payload: json!({"text": "hi"}),
            symbolic_refs: vec!["audio-buf-17".into()],
            stored_at: fixed_ts(),
        });

        let bytes = s.encode();
        let decoded = CognitiveState::decode(&bytes).expect("decode");
        assert_eq!(decoded, s);
    }

    #[test]
    fn density_matrix_roundtrips_bit_identical_across_multiple_cycles() {
        let mut s = CognitiveState::new("sess", "alpha", "f");
        let original = vec![vec![0.1, 0.5, 0.9], vec![0.2, 0.3, 0.8]];
        s.density_matrix = original
            .iter()
            .map(|row| FixedPoint::vec_from_f64(row))
            .collect();

        // Three encode/decode cycles.
        let mut current = s.clone();
        for _ in 0..3 {
            let bytes = current.encode();
            current = CognitiveState::decode(&bytes).expect("decode");
        }
        assert_eq!(current.density_matrix, s.density_matrix);
    }

    #[test]
    fn decode_rejects_garbage() {
        let err = CognitiveState::decode(b"not json").unwrap_err();
        assert!(err.0.contains("decode failed") || !err.0.is_empty());
    }

    #[test]
    fn optional_fields_default_cleanly() {
        let minimal = r#"{
            "session_id": "sess",
            "tenant_id": "alpha",
            "flow_id": "f",
            "density_matrix": [],
            "created_at": 1700000000000,
            "last_updated_at": 1700000000000
        }"#;
        let decoded = CognitiveState::decode(minimal.as_bytes()).unwrap();
        assert_eq!(decoded.subject_user_id, None);
        assert_eq!(decoded.belief_state, Value::Null);
        assert!(decoded.short_term_memory.is_empty());
    }
}
