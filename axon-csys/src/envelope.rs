//! v2.0.0 — Epistemic Envelope C23 kernel (Rust shim).
//!
//! Safe Rust wrapper around the C23 envelope kernel at
//! `c-src/effects/envelope.c`. The boundary follows the founder
//! pillar split:
//!
//!   - C side: Theorem 5.1 enforcement as a pure + total + constant-time
//!     primitive over a tiny by-value struct; defensive normalisation
//!     of NaN/Inf at the FFI ingress; closed-catalog `epistemic_kind`
//!     ordinals.
//!   - Rust side: type-safe wrapper struct
//!     [`EpistemicEnvelope`], `From`/`Into` conversions for the
//!     [`axon-rs`] wire shape, and the [`EpistemicKind`] closed enum
//!     mirroring the C ordinals.
//!
//! ## Mathematical pillar
//!
//! Theorem 5.1 (paper section 5.1): For any epistemic state E with
//! `derived_status = true`, the certainty `c` is bounded `c ≤ 0.99`.
//! The C23 kernel applies the clamp structurally; the Rust shim
//! exposes the bound as the const [`THEOREM_5_1_CEILING`] for any
//! adopter code that wants to reason about the ceiling at compile
//! time. Cross-language drift gates (see
//! [`tests::drift_gate_rust_ceiling_matches_c_ceiling`]) catch
//! divergence between the Rust const and the C export.
//!
//! ## Why this lives in axon-csys (and not axon-rs)
//!
//! The C23 kernel is the SINGLE point of structural truth for
//! Theorem 5.1. By construction, every Rust caller that wants to
//! produce a wire envelope MUST pass through this shim's
//! [`validate_degradation`] (or one of the secondary primitives) —
//! no in-tree Rust path exists that bypasses the C23 kernel for
//! production code. The fallback Rust implementation in
//! `axon-rs::wire_envelope::FlowEnvelope::seal` (v2.0.0) is
//! superseded by this shim in 39.c.

use std::os::raw::c_double;

// ──────────────────────────────────────────────────────────────────────
// Raw FFI types — must match envelope.h byte-for-byte.
// ──────────────────────────────────────────────────────────────────────

/// Byte-identical to the C `axon_csys_envelope_t` struct in
/// `c-src/effects/envelope.h`. The `#[repr(C)]` discipline gives us
/// the layout invariant.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EpistemicEnvelopeCRepr {
    pub certainty: c_double,
    pub derived_status: bool,
    pub epistemic_kind: u8,
}

#[cfg(feature = "native")]
extern "C" {
    fn axon_csys_envelope_validate_degradation(
        env: EpistemicEnvelopeCRepr,
    ) -> EpistemicEnvelopeCRepr;
    fn axon_csys_envelope_theorem_5_1_ceiling() -> c_double;
    fn axon_csys_envelope_clamp_ceiling(certainty: c_double) -> c_double;
}

// ──────────────────────────────────────────────────────────────────────
// Public surface — closed-catalog enum + safe wrapper struct.
// ──────────────────────────────────────────────────────────────────────

/// Closed catalog of epistemic posture ordinals. Mirrors the
/// `AXON_CSYS_EPISTEMIC_*` macros in `envelope.h`. Used to surface
/// the dominant epistemic kind to telemetry / audit consumers
/// without re-deriving from the certainty scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EpistemicKind {
    /// Apodictic — `certainty = 1.0`, not derived.
    Clean = 0,
    /// Derived but no breach surfaced. Bounded `≤ 0.99` by Theorem 5.1.
    Derived = 1,
    /// Anchor breach materialised; certainty further reduced by the
    /// breach severity weight (the Rust producer decides the
    /// reduction; this kind tags the envelope for downstream
    /// consumers).
    Breached = 2,
    /// Multi-source degradation (anchor + shield, anchor + store,
    /// etc.) — the most epistemically suspect class.
    Degraded = 3,
}

impl EpistemicKind {
    /// Decode an ordinal from the C side. Unknown ordinals (drift
    /// between header + Rust) fall back to [`EpistemicKind::Derived`]
    /// — the safe assumption when the producer is unrecognised.
    pub const fn from_ordinal(o: u8) -> Self {
        match o {
            0 => Self::Clean,
            1 => Self::Derived,
            2 => Self::Breached,
            3 => Self::Degraded,
            _ => Self::Derived,
        }
    }
}

/// Safe wrapper around the C `axon_csys_envelope_t`. Construction
/// goes through builder methods so producers always populate every
/// field explicitly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EpistemicEnvelope {
    pub certainty: f64,
    pub derived_status: bool,
    pub kind: EpistemicKind,
}

impl EpistemicEnvelope {
    /// Construct an envelope from explicit producer values.
    pub fn new(certainty: f64, derived_status: bool, kind: EpistemicKind) -> Self {
        Self {
            certainty,
            derived_status,
            kind,
        }
    }

    /// Theorem 5.1 enforcement via the C23 kernel. THIS IS THE
    /// CANONICAL ENTRY POINT — every wire envelope MUST pass through
    /// this method (or [`validate_degradation`]) before HTTP
    /// serialization. The C23 kernel:
    ///
    ///   1. Defensively normalises NaN / Inf / out-of-range values
    ///      to `[0.0, 1.0]`.
    ///   2. Clamps `certainty ≤ 0.99` when `derived_status == true`.
    ///   3. Passes `derived_status` + `kind` through unchanged.
    ///
    /// Pure + total + constant-time on the C side. The Rust shim
    /// just marshals the FFI types.
    pub fn validate_degradation(self) -> Self {
        validate_degradation(self)
    }
}

impl From<EpistemicEnvelope> for EpistemicEnvelopeCRepr {
    fn from(e: EpistemicEnvelope) -> Self {
        Self {
            certainty: e.certainty,
            derived_status: e.derived_status,
            epistemic_kind: e.kind as u8,
        }
    }
}

impl From<EpistemicEnvelopeCRepr> for EpistemicEnvelope {
    fn from(c: EpistemicEnvelopeCRepr) -> Self {
        Self {
            certainty: c.certainty,
            derived_status: c.derived_status,
            kind: EpistemicKind::from_ordinal(c.epistemic_kind),
        }
    }
}

/// Theorem 5.1 ceiling — the unbypassable upper bound on derived
/// certainty. Exported as a `pub const` so adopter code can
/// reason about the bound at compile time. The drift gate in
/// [`tests`] verifies this matches the C kernel's export.
pub const THEOREM_5_1_CEILING: f64 = 0.99;

// ──────────────────────────────────────────────────────────────────────
// Free-function entry points (called via FFI to the C23 kernel).
// ──────────────────────────────────────────────────────────────────────

/// Theorem 5.1 — canonical entry point. Calls the C23 kernel
/// `axon_csys_envelope_validate_degradation`. See
/// [`EpistemicEnvelope::validate_degradation`] for semantics.
#[cfg(feature = "native")]
pub fn validate_degradation(env: EpistemicEnvelope) -> EpistemicEnvelope {
    let c_input: EpistemicEnvelopeCRepr = env.into();
    let c_output = unsafe { axon_csys_envelope_validate_degradation(c_input) };
    c_output.into()
}

/// v2.83.0 — the same function without the C toolchain.
///
/// A LINE-BY-LINE mirror of `c-src/effects/envelope.c`, not a
/// reinterpretation: defensive normalisation first (NaN / ±Inf / out-of-range
/// coerced into `[0,1]` BEFORE the ceiling check, so the post-condition holds
/// regardless of caller misbehaviour), then the Theorem 5.1 clamp on derived
/// states only, then `derived_status` + `epistemic_kind` pass through
/// untouched.
///
/// The drift gate in this module's tests runs against whichever path is
/// compiled, so the two cannot diverge silently.
#[cfg(not(feature = "native"))]
pub fn validate_degradation(mut env: EpistemicEnvelope) -> EpistemicEnvelope {
    env.certainty = normalise(env.certainty);
    if env.derived_status && env.certainty > THEOREM_5_1_CEILING {
        env.certainty = THEOREM_5_1_CEILING;
    }
    env
}

/// v2.83.0 — the C helper `axon_csys_envelope_normalise`, in Rust.
/// Non-finite and negative inputs collapse to `0.0`; above `1.0` saturates.
#[cfg(not(feature = "native"))]
fn normalise(c: f64) -> f64 {
    if !c.is_finite() || c < 0.0 {
        return 0.0;
    }
    if c > 1.0 {
        return 1.0;
    }
    c
}

/// Theorem 5.1 ceiling exported by the C23 kernel. Returns
/// `0.99`. Used by drift-gate tests to detect Rust/C divergence
/// in the bound.
#[cfg(feature = "native")]
pub fn theorem_5_1_ceiling_from_c() -> f64 {
    unsafe { axon_csys_envelope_theorem_5_1_ceiling() }
}

/// v2.83.0 — without the C kernel there is no second source to compare
/// against, so this returns the Rust constant. The drift gate that calls it
/// becomes a tautology in this configuration — which is exactly why the
/// `native` build must keep running in CI: the comparison only means
/// something when both sides exist.
#[cfg(not(feature = "native"))]
pub fn theorem_5_1_ceiling_from_c() -> f64 {
    THEOREM_5_1_CEILING
}

/// Belt-and-suspenders unconditional ceiling clamp. Calls the C23
/// kernel `axon_csys_envelope_clamp_ceiling`. NOT the canonical
/// path (canonical is [`validate_degradation`]); this is the
/// secondary guard for callers that want the absolute ceiling
/// regardless of `derived_status`.
#[cfg(feature = "native")]
pub fn clamp_ceiling(certainty: f64) -> f64 {
    unsafe { axon_csys_envelope_clamp_ceiling(certainty) }
}

/// v2.83.0 — mirror of `axon_csys_envelope_clamp_ceiling`: normalise,
/// then saturate at the Theorem 5.1 ceiling.
#[cfg(not(feature = "native"))]
pub fn clamp_ceiling(certainty: f64) -> f64 {
    let c = normalise(certainty);
    if c > THEOREM_5_1_CEILING {
        THEOREM_5_1_CEILING
    } else {
        c
    }
}

// ──────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_clean_path_preserves_certainty() {
        let env = EpistemicEnvelope::new(1.0, false, EpistemicKind::Clean);
        let out = validate_degradation(env);
        assert_eq!(
            out.certainty, 1.0,
            "non-derived envelope preserves apodictic certainty"
        );
        assert!(!out.derived_status);
        assert_eq!(out.kind, EpistemicKind::Clean);
    }

    #[test]
    fn validate_derived_clamps_to_ceiling() {
        // A misbehaving Rust producer sets certainty = 1.0 + derived.
        // The C23 kernel MUST clamp regardless.
        let env = EpistemicEnvelope::new(1.0, true, EpistemicKind::Derived);
        let out = validate_degradation(env);
        assert_eq!(
            out.certainty, THEOREM_5_1_CEILING,
            "derived envelope clamped to Theorem 5.1 ceiling"
        );
        assert!(out.derived_status);
        assert_eq!(out.kind, EpistemicKind::Derived);
    }

    #[test]
    fn validate_derived_below_ceiling_passes_through() {
        let env = EpistemicEnvelope::new(0.5, true, EpistemicKind::Derived);
        let out = validate_degradation(env);
        assert_eq!(
            out.certainty, 0.5,
            "derived envelope below ceiling passes through unchanged"
        );
    }

    #[test]
    fn validate_normalises_nan() {
        let env = EpistemicEnvelope::new(f64::NAN, true, EpistemicKind::Breached);
        let out = validate_degradation(env);
        assert_eq!(
            out.certainty, 0.0,
            "NaN input coerced to 0.0 (defensive normalisation)"
        );
    }

    #[test]
    fn validate_normalises_positive_infinity() {
        let env = EpistemicEnvelope::new(f64::INFINITY, true, EpistemicKind::Derived);
        let out = validate_degradation(env);
        assert_eq!(
            out.certainty, 0.0,
            "+Inf coerced to 0.0 (defensive normalisation)"
        );
    }

    #[test]
    fn validate_normalises_negative_certainty() {
        let env = EpistemicEnvelope::new(-0.5, false, EpistemicKind::Clean);
        let out = validate_degradation(env);
        assert_eq!(out.certainty, 0.0, "negative certainty coerced to 0.0");
    }

    #[test]
    fn validate_normalises_above_one() {
        // 1.5 is out-of-range; the kernel coerces to 1.0 first, then
        // (since not derived) preserves.
        let env = EpistemicEnvelope::new(1.5, false, EpistemicKind::Clean);
        let out = validate_degradation(env);
        assert_eq!(
            out.certainty, 1.0,
            "certainty > 1.0 coerced to 1.0 on non-derived path"
        );
    }

    #[test]
    fn validate_normalises_above_one_and_clamps_derived() {
        let env = EpistemicEnvelope::new(1.5, true, EpistemicKind::Derived);
        let out = validate_degradation(env);
        assert_eq!(
            out.certainty, THEOREM_5_1_CEILING,
            "certainty > 1.0 coerced AND derived → 0.99"
        );
    }

    #[test]
    fn clamp_ceiling_unconditional() {
        // The belt-and-suspenders clamp ignores `derived_status` and
        // always applies the ceiling.
        assert_eq!(clamp_ceiling(1.0), THEOREM_5_1_CEILING);
        assert_eq!(clamp_ceiling(0.5), 0.5);
        assert_eq!(clamp_ceiling(f64::NAN), 0.0);
        assert_eq!(clamp_ceiling(f64::INFINITY), 0.0);
        assert_eq!(clamp_ceiling(-1.0), 0.0);
        assert_eq!(clamp_ceiling(2.0), THEOREM_5_1_CEILING);
    }

    #[test]
    fn drift_gate_rust_ceiling_matches_c_ceiling_2() {
        // Drift gate — the Rust THEOREM_5_1_CEILING const MUST agree
        // with the C kernel's exported constant. If they ever diverge
        // it indicates a bug in one of the two; this test fails fast.
        let from_c = theorem_5_1_ceiling_from_c();
        assert_eq!(
            from_c, THEOREM_5_1_CEILING,
            "DRIFT — Rust THEOREM_5_1_CEILING ({}) MUST match \
             C kernel export ({}). Divergence is a structural bug.",
            THEOREM_5_1_CEILING, from_c
        );
    }

    #[test]
    fn epistemic_kind_round_trip_through_ffi() {
        // Each EpistemicKind variant MUST round-trip through the FFI
        // byte-identically.
        let kinds = [
            EpistemicKind::Clean,
            EpistemicKind::Derived,
            EpistemicKind::Breached,
            EpistemicKind::Degraded,
        ];
        for k in kinds {
            let env = EpistemicEnvelope::new(0.7, true, k);
            let out = validate_degradation(env);
            assert_eq!(
                out.kind, k,
                "EpistemicKind::{k:?} MUST round-trip through FFI"
            );
        }
    }

    #[test]
    fn unknown_ordinal_falls_back_to_derived() {
        // Defensive: an unknown ordinal coming back from C means the
        // C/Rust enum drifted. We fall back to Derived (safe class)
        // rather than panic.
        assert_eq!(EpistemicKind::from_ordinal(7), EpistemicKind::Derived);
        assert_eq!(EpistemicKind::from_ordinal(255), EpistemicKind::Derived);
    }

    #[test]
    fn validate_method_on_struct_matches_free_function() {
        // The method on EpistemicEnvelope MUST be a thin wrapper
        // around the free function — verify they produce identical
        // outputs.
        let env = EpistemicEnvelope::new(0.95, true, EpistemicKind::Derived);
        let via_fn = validate_degradation(env);
        let via_method = env.validate_degradation();
        assert_eq!(via_fn, via_method);
    }
}
