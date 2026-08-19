//! v2.23.0 — quant as the FIRST Advantage-Witness instance.
//!
//! The adopter deep-research verdict, made executable in our own code: for the
//! amplitude-encoding + fixed-Pauli path, the quantum kernel is *classically
//! reproducible*, so it carries **no** advantage over a classical baseline.
//!
//! Two facts the report proved about our exact simulator (`quant.rs`):
//!   1. amplitude-fidelity ≡ cosine — `⟨ψ(x)|ψ(y)⟩ = xᵀy` for unit vectors
//!      (Schuld & Killoran), so our `kernel = |⟨ψ_a|ψ_b⟩|²` equals `(xᵀy)²`,
//!      the **classical degree-2 polynomial kernel**;
//!   2. a fixed Pauli observable on real amplitudes is a degree-≤2 quadratic form,
//!      and a data-INDEPENDENT `evolve` does not escape it (Theorem 4.1).
//!
//! So the [`QuantKernelWitness`] measures the **`geometric_difference` (reference
//! form)**: how much the quantum fidelity Gram differs from the classical `(xᵀy)²`
//! Gram it provably equals. For amplitude+Pauli it is **0** — the witness FAILS,
//! and the honest verdict recommends the classical baseline (`axon-W007`). This is
//! the small-`n` OSS reference; the enterprise evaluates the full Huang et al. `g`
//! at real-embedding scale (`crates/saas-quant`, shipped v2.4.0). The escape that
//! *can* make the witness pass — data re-uploading — is v2.23.0.

use crate::advantage_witness::{AdvantageMetric, AdvantageWitness, Baseline};
use crate::quant::{EncodingScheme, QuantBackend, ReferenceSimulator};

/// A witness that a quant amplitude kernel beats the classical cosine baseline.
/// Per the theorem above, for amplitude+Pauli it never does (the measure is ~0).
pub struct QuantKernelWitness {
    /// The minimum geometric difference that would justify the quantum cost.
    pub threshold: f64,
}

impl QuantKernelWitness {
    pub fn new(threshold: f64) -> Self {
        Self { threshold }
    }
}

/// The witness is evaluated on a real dataset: a set of unit-norm vectors (e.g.
/// L2-normalised embeddings). The OSS reference is capped by the simulator's
/// `n ≤ 10` qubit bound; the enterprise lifts it.
impl AdvantageWitness<Vec<Vec<f64>>> for QuantKernelWitness {
    fn baseline(&self) -> Baseline {
        Baseline("cosine".to_string())
    }
    fn metric(&self) -> AdvantageMetric {
        AdvantageMetric::GeometricDifference
    }
    fn threshold(&self) -> f64 {
        self.threshold
    }

    /// The reference `geometric_difference`: the normalised Frobenius distance
    /// between the quantum fidelity Gram `K_q` (from the actual simulator) and the
    /// classical polynomial Gram `K_c[i][j] = (xᵢ·xⱼ)²`. `0` ⇒ the quantum kernel
    /// is reproduced bit-for-bit by a classical formula ⇒ no advantage.
    fn measure(&self, data: &Vec<Vec<f64>>) -> f64 {
        let sim = ReferenceSimulator::new();
        let mut num = 0.0_f64;
        let mut den = 0.0_f64;
        for xi in data {
            for xj in data {
                let kq = match (
                    sim.encode(xi, EncodingScheme::Amplitude),
                    sim.encode(xj, EncodingScheme::Amplitude),
                ) {
                    (Ok(a), Ok(b)) => sim.kernel(&a, &b).unwrap_or(0.0),
                    // A non-unit / over-cap input cannot be amplitude-encoded; it
                    // contributes nothing (the witness data must be valid carriers).
                    _ => continue,
                };
                let dot: f64 = xi.iter().zip(xj).map(|(a, b)| a * b).sum();
                let kc = dot * dot; // the classical (xᵀy)² polynomial kernel
                num += (kq - kc).powi(2);
                den += kc * kc;
            }
        }
        if den == 0.0 {
            0.0
        } else {
            (num / den).sqrt()
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(v: &[f64]) -> Vec<f64> {
        let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        v.iter().map(|x| x / n).collect()
    }

    #[test]
    fn quantum_fidelity_kernel_equals_the_classical_cosine_squared_theorem() {
        // The report's core identity, in our own code: for amplitude-encoded unit
        // vectors, `|⟨ψ_a|ψ_b⟩|² == (xᵀy)²`. Bit-checked over several pairs.
        let sim = ReferenceSimulator::new();
        let pairs = [
            (unit(&[0.6, 0.8]), unit(&[1.0, 0.0])),
            (unit(&[1.0, 2.0, 3.0, 4.0]), unit(&[4.0, 3.0, 2.0, 1.0])),
            (unit(&[0.3, 0.3, 0.3, 0.9]), unit(&[0.9, 0.1, 0.1, 0.1])),
        ];
        for (x, y) in &pairs {
            let a = sim.encode(x, EncodingScheme::Amplitude).unwrap();
            let b = sim.encode(y, EncodingScheme::Amplitude).unwrap();
            let kq = sim.kernel(&a, &b).unwrap();
            let dot: f64 = x.iter().zip(y).map(|(p, q)| p * q).sum();
            let kc = dot * dot;
            assert!(
                (kq - kc).abs() < 1e-12,
                "amplitude fidelity must equal (xᵀy)²: kq={kq}, (xᵀy)²={kc}"
            );
        }
    }

    #[test]
    fn amplitude_pauli_witness_fails_closed_no_advantage_over_cosine() {
        // The verdict the report reached, executable: the geometric difference of
        // the quantum kernel vs the classical (xᵀy)² Gram is ~0 → holds = false.
        let data = vec![
            unit(&[0.6, 0.8]),
            unit(&[1.0, 0.0]),
            unit(&[1.0, 1.0]),
            unit(&[0.2, 0.9]),
        ];
        let w = QuantKernelWitness::new(0.05);
        let v = w.verdict(&data);
        assert!(v.measure < 1e-9, "amplitude+Pauli geometric difference must be ~0, got {}", v.measure);
        assert!(!v.holds, "the witness must FAIL — no advantage over cosine");
        assert_eq!(v.baseline.label(), "cosine");
        assert!(v.summary().contains("NO measurable advantage"));
    }
}
