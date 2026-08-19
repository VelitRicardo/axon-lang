//! AXON Runtime — Lambda Data (ΛD) Apply (v1.10.0 — Rust mirror).
//!
//! Runtime types and helpers for the `lambda apply X to Y` flow-body
//! statement. Mirror of `axon/runtime/lambda_runtime.py` — same
//! semantics, same Theorem 5.1 guard, same JSON shape so cross-stack
//! parity tests can compare byte-for-byte.
//!
//! ψ = ⟨T, V, E⟩ where:
//!   T : String              — Ontology
//!   V : serde_json::Value   — Bound value (string-typed in Rust runner)
//!   E : LambdaTensor        — Epistemic tensor ⟨c, τ_start, τ_end, ρ, δ⟩
//!
//! Vocabulary follows the ΛD formalism:
//!   δ ∈ {raw, derived, inferred, aggregated, transformed}
//!
//! Theorem 5.1 (Epistemic Degradation) is enforced at runtime by
//! `enforce_theorem_5_1`, mirroring the compile-time guard in
//! `axon-frontend::type_checker::check_lambda_data`. Defends against
//! IR-JSON tampering between compile and execute.

use serde::{Deserialize, Serialize};

/// Mirror of `axon-frontend::type_checker::VALID_DERIVATIONS` and the
/// Python runtime's `VALID_DERIVATIONS`. Drift is detected by the
/// cross-stack parity golden in `axon-rs/tests/parity/`.
pub const VALID_DERIVATIONS: [&str; 5] = [
    "raw", "derived", "inferred", "aggregated", "transformed",
];

/// E = ⟨c, τ_start, τ_end, ρ, δ⟩ — the epistemic tensor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LambdaTensor {
    pub c: f64,
    pub tau_start: String,
    pub tau_end: String,
    pub rho: String,
    pub delta: String,
}

/// ψ = ⟨T, V, E⟩ — the epistemic state vector produced by `lambda apply`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LambdaPsi {
    #[serde(rename = "T")]
    pub t: String,
    #[serde(rename = "V")]
    pub v: serde_json::Value,
    #[serde(rename = "E")]
    pub e: LambdaTensor,
    pub spec_name: String,
}

/// Spec snapshot carried through the CompiledStep payload — verbatim
/// copy of the IR `lambda` declaration so the runtime never needs the
/// IR. Same shape as the Python `BaseBackend._compile_lambda_apply_step`
/// metadata (so cross-stack parity is structural not just byte-level).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LambdaApplyPayload {
    pub lambda_data_name: String,
    pub target: String,
    pub output_type: String,
    pub spec_snapshot: SpecSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SpecSnapshot {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub ontology: String,
    #[serde(default)]
    pub certainty: f64,
    #[serde(default)]
    pub temporal_frame_start: String,
    #[serde(default)]
    pub temporal_frame_end: String,
    #[serde(default)]
    pub provenance: String,
    #[serde(default)]
    pub derivation: String,
}

/// Theorem 5.1 violation — raised by `enforce_theorem_5_1` when a spec
/// snapshot reaches the dispatcher with c=1.0 + non-raw derivation,
/// out-of-range certainty, or unknown derivation.
#[derive(Debug, Clone)]
pub struct EpistemicDegradationError {
    pub message: String,
    pub spec_name: String,
}

impl std::fmt::Display for EpistemicDegradationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[L3] EpistemicDegradationError: {}", self.message)
    }
}

impl std::error::Error for EpistemicDegradationError {}

/// Runtime mirror of the Epistemic Degradation Theorem compile-time
/// check. Defends against IR-JSON tampering: a tampered IR could carry
/// a `lambda_data_apply` whose snapshot violates the theorem the
/// front-end rejected. This guard catches that at apply time, before
/// the bad envelope propagates downstream.
pub fn enforce_theorem_5_1(snapshot: &SpecSnapshot) -> Result<(), EpistemicDegradationError> {
    if !(0.0..=1.0).contains(&snapshot.certainty) {
        return Err(EpistemicDegradationError {
            message: format!(
                "lambda '{}' has out-of-range certainty {} (must be in [0.0, 1.0])",
                snapshot.name, snapshot.certainty,
            ),
            spec_name: snapshot.name.clone(),
        });
    }

    if !snapshot.derivation.is_empty()
        && !VALID_DERIVATIONS.contains(&snapshot.derivation.as_str())
    {
        return Err(EpistemicDegradationError {
            message: format!(
                "lambda '{}' has unknown derivation '{}' (valid: {})",
                snapshot.name,
                snapshot.derivation,
                VALID_DERIVATIONS.join(", "),
            ),
            spec_name: snapshot.name.clone(),
        });
    }

    if (snapshot.certainty - 1.0).abs() < f64::EPSILON
        && !snapshot.derivation.is_empty()
        && snapshot.derivation != "raw"
    {
        return Err(EpistemicDegradationError {
            message: format!(
                "Theorem 5.1 violation at apply time: lambda '{}' has \
                 certainty=1.0 with derivation='{}'. Only 'raw' data may \
                 carry absolute certainty (c=1.0).",
                snapshot.name, snapshot.derivation,
            ),
            spec_name: snapshot.name.clone(),
        });
    }

    Ok(())
}

/// Construct ψ from a spec snapshot and a resolved target value (here
/// represented as a JSON value to keep the Rust runner uniform with the
/// string-typed ExecContext).
pub fn build_psi(
    snapshot: &SpecSnapshot,
    target_value: serde_json::Value,
) -> Result<LambdaPsi, EpistemicDegradationError> {
    enforce_theorem_5_1(snapshot)?;

    let delta = if snapshot.derivation.is_empty() {
        "raw".to_string()
    } else {
        snapshot.derivation.clone()
    };

    Ok(LambdaPsi {
        t: snapshot.ontology.clone(),
        v: target_value,
        e: LambdaTensor {
            c: snapshot.certainty,
            tau_start: snapshot.temporal_frame_start.clone(),
            tau_end: snapshot.temporal_frame_end.clone(),
            rho: snapshot.provenance.clone(),
            delta,
        },
        spec_name: snapshot.name.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(certainty: f64, derivation: &str) -> SpecSnapshot {
        SpecSnapshot {
            name: "S".to_string(),
            ontology: "measurement.temp".to_string(),
            certainty,
            temporal_frame_start: "2026-01-01T00:00:00Z".to_string(),
            temporal_frame_end: "2026-12-31T23:59:59Z".to_string(),
            provenance: "Sensor-A".to_string(),
            derivation: derivation.to_string(),
        }
    }

    #[test]
    fn t51_raw_one_legal() {
        assert!(enforce_theorem_5_1(&snap(1.0, "raw")).is_ok());
    }

    #[test]
    fn t51_inferred_below_one_legal() {
        assert!(enforce_theorem_5_1(&snap(0.7, "inferred")).is_ok());
    }

    #[test]
    fn t51_inferred_with_one_rejected() {
        let err = enforce_theorem_5_1(&snap(1.0, "inferred")).unwrap_err();
        assert!(err.message.contains("Theorem 5.1"));
        assert_eq!(err.spec_name, "S");
    }

    #[test]
    fn t51_aggregated_with_one_rejected() {
        assert!(enforce_theorem_5_1(&snap(1.0, "aggregated")).is_err());
    }

    #[test]
    fn t51_out_of_range_rejected() {
        assert!(enforce_theorem_5_1(&snap(1.5, "raw")).is_err());
        assert!(enforce_theorem_5_1(&snap(-0.1, "raw")).is_err());
    }

    #[test]
    fn t51_unknown_derivation_rejected() {
        let err = enforce_theorem_5_1(&snap(0.5, "unicornified")).unwrap_err();
        assert!(err.message.contains("unicornified"));
    }

    #[test]
    fn t51_empty_derivation_passes() {
        // Compile-time guard treats empty derivation as legacy/observed
        // and skips Theorem 5.1; runtime mirrors that.
        assert!(enforce_theorem_5_1(&snap(1.0, "")).is_ok());
    }

    #[test]
    fn build_psi_carries_full_tensor() {
        let psi = build_psi(&snap(0.9, "raw"), serde_json::json!(23.5)).unwrap();
        assert_eq!(psi.t, "measurement.temp");
        assert_eq!(psi.v, serde_json::json!(23.5));
        assert!((psi.e.c - 0.9).abs() < f64::EPSILON);
        assert_eq!(psi.e.delta, "raw");
        assert_eq!(psi.e.rho, "Sensor-A");
        assert_eq!(psi.spec_name, "S");
    }

    #[test]
    fn build_psi_serialises_with_formal_keys() {
        let psi = build_psi(&snap(1.0, "raw"), serde_json::json!("payload")).unwrap();
        let json = serde_json::to_value(&psi).unwrap();
        assert!(json.get("T").is_some());
        assert!(json.get("V").is_some());
        assert!(json.get("E").is_some());
        assert!(json.get("spec_name").is_some());
    }

    #[test]
    fn build_psi_json_round_trip() {
        let psi = build_psi(&snap(0.8, "aggregated"), serde_json::json!(42)).unwrap();
        let s = serde_json::to_string(&psi).unwrap();
        let back: LambdaPsi = serde_json::from_str(&s).unwrap();
        assert_eq!(back, psi);
    }

    #[test]
    fn build_psi_t51_violation_propagates() {
        assert!(build_psi(&snap(1.0, "inferred"), serde_json::json!(1)).is_err());
    }

    #[test]
    fn build_psi_empty_derivation_defaults_to_raw() {
        let psi = build_psi(&snap(1.0, ""), serde_json::json!(1)).unwrap();
        assert_eq!(psi.e.delta, "raw");
    }
}
