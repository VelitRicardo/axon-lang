//! v2.4.0 — deterministic fuzz for the OSS `quant` reference simulator.
//!
//! A single LCG drives thousands of iterations over the `f64` reference
//! simulator that ships in `axon-lang` (the `n ≤ 10` CPU backend every adopter
//! gets), asserting the *invariants* the primitive promises rather than golden
//! values. Deterministic seeds → a regression surfaces at the exact iteration
//! it diverges, and the run is reproducible across machines.
//!
//! Invariants:
//!   - amplitude-encoding a unit-norm carrier yields `⟨ψ|ψ⟩ ≈ 1`.
//!   - a single-Pauli expectation obeys `|⟨P⟩| ≤ 1` (operator norm 1).
//!   - the fidelity kernel is symmetric, `K(x,x) ≈ 1`, and `K ∈ [0, 1]`.
//!   - the `n ≤ 10` capacity cap is a hard `axon-E0783` error, never a silent
//!     truncation.

use axon::quant::{EncodingScheme, PauliSum, QuantBackend, QuantError, ReferenceSimulator};

struct Lcg(u64);

impl Lcg {
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn unit(&mut self) -> f64 {
        let bits = self.next_u64() >> 11;
        2.0 * (bits as f64 / (1u64 << 53) as f64) - 1.0
    }
    fn range(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// A random unit-norm carrier of length `2^n`.
fn unit_carrier(rng: &mut Lcg, n: usize) -> Vec<f64> {
    let dim = 1usize << n;
    let raw: Vec<f64> = (0..dim).map(|_| rng.unit()).collect();
    let norm: f64 = raw.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm = if norm == 0.0 { 1.0 } else { norm };
    raw.into_iter().map(|x| x / norm).collect()
}

#[test]
fn amplitude_encoding_preserves_unit_norm() {
    let sim = ReferenceSimulator::new();
    let mut rng = Lcg(0x5121_0001);
    for _ in 0..2000 {
        let n = 1 + rng.range(4); // 1..=4 qubits
        let x = unit_carrier(&mut rng, n);
        let sv = sim.encode(&x, EncodingScheme::Amplitude).expect("unit-norm encodes");
        let norm: f64 = sv.amps.iter().map(|c| c.norm_sqr()).sum();
        assert!(
            (norm - 1.0).abs() < 1e-9,
            "‖ψ‖² must stay 1 after amplitude encode, got {norm}"
        );
    }
}

#[test]
fn single_pauli_expectation_is_bounded() {
    let sim = ReferenceSimulator::new();
    let mut rng = Lcg(0x5121_0002);
    let paulis = ['I', 'X', 'Y', 'Z'];
    for _ in 0..2000 {
        let n = 1 + rng.range(4);
        let x = unit_carrier(&mut rng, n);
        let sv = sim.encode(&x, EncodingScheme::Amplitude).unwrap();
        let pstr: String = (0..n).map(|_| paulis[rng.range(4)]).collect();
        let m = sim
            .measure(&sv, &PauliSum { terms: vec![(1.0, pstr.clone())] })
            .unwrap();
        assert!(
            m.abs() <= 1.0 + 1e-9,
            "⟨{pstr}⟩ = {m} violates |⟨P⟩| ≤ 1"
        );
    }
}

#[test]
fn fidelity_kernel_is_symmetric_self_one_and_bounded() {
    let sim = ReferenceSimulator::new();
    let mut rng = Lcg(0x5121_0003);
    for _ in 0..1500 {
        let n = 1 + rng.range(4);
        let a = sim.encode(&unit_carrier(&mut rng, n), EncodingScheme::Amplitude).unwrap();
        let b = sim.encode(&unit_carrier(&mut rng, n), EncodingScheme::Amplitude).unwrap();
        let kab = sim.kernel(&a, &b).unwrap();
        let kba = sim.kernel(&b, &a).unwrap();
        assert!((kab - kba).abs() < 1e-9, "kernel must be symmetric");
        assert!(
            (-1e-9..=1.0 + 1e-9).contains(&kab),
            "fidelity kernel must lie in [0,1], got {kab}"
        );
        let kaa = sim.kernel(&a, &a).unwrap();
        assert!((kaa - 1.0).abs() < 1e-9, "K(x,x) must be 1, got {kaa}");
    }
}

#[test]
fn capacity_cap_is_a_hard_error_never_silent() {
    let sim = ReferenceSimulator::new();
    // n = 11 (D = 2048) exceeds the n ≤ 10 OSS cap → axon-E0783, not a truncation.
    let oversized = vec![0.0; 1 << 11];
    let err = sim
        .encode(&oversized, EncodingScheme::Amplitude)
        .expect_err("n>10 must be rejected");
    assert!(matches!(err, QuantError::CapacityExceeded { .. }));
    assert_eq!(err.code(), "axon-E0783");
}
