//! §Fase 119.b — the `mandate` stability judgment: one admissibility predicate,
//! shared by the compiler and the runtime.
//!
//! # Why this lives in the frontend
//!
//! The band arithmetic was born in `axon-rs/src/pem/density_matrix.rs`, next to
//! the controller that first needed it. That is the §118 smell — a general
//! concept parked in the specific module that first needed it — because the
//! *type checker* needs the same judgment: README §XV promises that unstable
//! gains are rejected **at compile time**. The frontend has zero dependencies
//! and `axon-rs` already depends on it, so the mathematics moves here and the
//! runtime consumes it. One admissibility judgment, two call sites; a mandate
//! the compiler admits is exactly a mandate the runtime would admit.
//!
//! # The judgment, and what it is conditional on
//!
//! The two papers bound the control effort `|Kp + Ki + Kd|` in **opposite
//! directions**, and both theorems are **conditional**:
//!
//! - `paper_mandate.md` §3 (Lyapunov): *if* the backend's drift satisfies
//!   `sup|drift(t)| ≤ D`, then the cage holds only when the effort **exceeds
//!   `D`**. Too small, and `V(e) = ½e²` does not decrease.
//! - `paper_mathematical_prompt_optimization.md` §6.3 (Lipschitz): *if* the
//!   refinement map is `L`-Lipschitz, convergence requires the effort **below
//!   `1/L`**. Too large, and the loop oscillates or diverges.
//!
//! Together: `D < |Kp + Ki + Kd| < 1/L`, both endpoints exclusive — at
//! `effort == D` the Lyapunov derivative is not strictly negative; at
//! `effort == 1/L` the contraction factor is exactly 1. Neither gives its
//! theorem the strict inequality it requires.
//!
//! # How a static checker can verify an empirical condition
//!
//! It cannot — and this module does not pretend to. `D` and `L` are **measured
//! properties of a backend**, and the compiler does not know the backend.
//! Fabricating them (a baked-in table of vendor constants) would make the gate
//! vacuous, which is precisely the defect §119.b exists to end. Instead:
//!
//! **The developer declares the bounds** — `stability { D: 0.3, L: 0.4 }` — and
//! the compiler verifies the theorem **conditionally on the declaration**. The
//! declared `(D, L)` travel in the IR as proof obligations: at dispatch, the
//! runtime must discharge them against the measured backend, or refuse. This is
//! the same shape as §51's proof-carrying code (a proof checked against stated
//! axioms, the axioms discharged at the boundary) and §91's `now:` declared
//! time. In sequent form:
//!
//! ```text
//! Γ, sup|drift| ≤ D, Lip(f) ≤ L  ⊢  D < |Kp+Ki+Kd| < 1/L  ⇒  Converge(e, ε, N)
//! ```
//!
//! The compiler checks the turnstile's right side exactly; the hypotheses to
//! the left are the runtime's debt, carried explicitly, never assumed silently.
//!
//! A mandate that declares **no** bounds gets the sign checks only — which the
//! papers show to be necessary but not sufficient — and its IR carries no
//! obligations, so the runtime knows nothing was statically promised. Absence
//! is visible, not defaulted.

use std::fmt;

/// The three gains of `u(t) = Kp·e + Ki·∫e dt + Kd·de/dt`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gains {
    pub kp: f64,
    pub ki: f64,
    pub kd: f64,
}

impl Gains {
    /// The aggregate the two bounds constrain: `|Kp + Ki + Kd|`.
    pub fn effort(&self) -> f64 {
        (self.kp + self.ki + self.kd).abs()
    }
}

/// Why a mandate's declared control law is not admissible.
///
/// Every variant carries the numbers, because a diagnostic that says "unstable
/// gains" without saying which bound was crossed and by how much cannot be
/// acted on by the developer who wrote them.
#[derive(Debug, Clone, PartialEq)]
pub enum GainError {
    /// `Kp ≤ 0`: no restoring force at all. Published in README §XV.
    NonPositiveProportional { kp: f64 },
    /// `Ki < 0`: the integral term would amplify accumulated error.
    NegativeIntegral { ki: f64 },
    /// `Kd < 0`: the derivative term would amplify acceleration.
    NegativeDerivative { kd: f64 },
    /// Below the drift floor — `paper_mandate.md` §3's precondition.
    BelowDriftFloor { effort: f64, drift_bound: f64 },
    /// Above the Lipschitz ceiling — `prompt_opt` §6.3.
    AboveLipschitzCeiling { effort: f64, ceiling: f64 },
    /// `D ≥ 1/L`: the band itself is empty. Not a bad configuration — an
    /// uncageable backend.
    EmptyBand { drift_bound: f64, ceiling: f64 },
    /// `ε ∉ (0, 1]`. Below zero no response can enter the band; above one the
    /// band admits even total violation, because `e = 1 − CSR` is confined to
    /// `[0, 1]` — the mandate could never fail, which is the vacuous guarantee
    /// this whole judgment exists to refuse.
    ToleranceOutOfRange { epsilon: f64 },
    /// `N < 1`: a budget that permits no attempt.
    NonPositiveStepBudget { max_steps: i64 },
    /// A declared drift bound below zero. `D` is `sup|drift(t)|`, a supremum of
    /// absolute values; a negative declaration is not a loose bound, it is a
    /// category error.
    NegativeDriftBound { drift_bound: f64 },
    /// A declared Lipschitz constant below zero — same category error as above.
    NegativeLipschitz { lipschitz: f64 },
    /// `stability { … }` declared with no proportional gain to check it
    /// against. A premise with no controller is unfalsifiable.
    BoundsWithoutProportionalGain,
}

impl fmt::Display for GainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GainError::NonPositiveProportional { kp } => write!(
                f,
                "Kp is {kp}, but a mandate with no proportional term applies no \
                 restoring force to a violating response"
            ),
            GainError::NegativeIntegral { ki } => write!(
                f,
                "Ki is {ki}; a negative integral gain amplifies accumulated \
                 error instead of correcting it"
            ),
            GainError::NegativeDerivative { kd } => write!(
                f,
                "Kd is {kd}; a negative derivative gain amplifies error \
                 acceleration instead of damping it"
            ),
            GainError::BelowDriftFloor {
                effort,
                drift_bound,
            } => write!(
                f,
                "the control effort |Kp+Ki+Kd| = {effort} does not exceed the \
                 declared drift bound D = {drift_bound}. The Lyapunov argument \
                 for this mandate is conditional on exceeding it \
                 (see paper_mandate), so with these gains the cage does not hold"
            ),
            GainError::AboveLipschitzCeiling { effort, ceiling } => write!(
                f,
                "the control effort |Kp+Ki+Kd| = {effort} is at or above the \
                 convergence ceiling 1/L = {ceiling}; the refinement loop would \
                 oscillate or diverge rather than settle"
            ),
            GainError::EmptyBand {
                drift_bound,
                ceiling,
            } => write!(
                f,
                "no gains can stabilise this mandate on this backend: the drift \
                 floor D = {drift_bound} is at or above the convergence ceiling \
                 1/L = {ceiling}, so the stability band is empty. This is a \
                 property of the backend, not of the declaration"
            ),
            GainError::ToleranceOutOfRange { epsilon } => write!(
                f,
                "epsilon is {epsilon}; the convergence band must lie in (0, 1] \
                 — e = 1 − CSR is confined to [0, 1], so a band wider than 1 \
                 admits even total violation and the mandate could never fail"
            ),
            GainError::NonPositiveStepBudget { max_steps } => write!(
                f,
                "max_steps is {max_steps}; a mandate must permit at least one \
                 refinement attempt"
            ),
            GainError::NegativeDriftBound { drift_bound } => write!(
                f,
                "the declared drift bound D = {drift_bound} is negative, but D \
                 is sup|drift(t)| — a supremum of absolute values cannot be \
                 below zero"
            ),
            GainError::NegativeLipschitz { lipschitz } => write!(
                f,
                "the declared Lipschitz constant L = {lipschitz} is negative, \
                 but L bounds |f(x) − f(y)| / |x − y|, which cannot be below \
                 zero"
            ),
            GainError::BoundsWithoutProportionalGain => write!(
                f,
                "stability bounds are declared but the mandate has no Kp to \
                 check them against; a premise with no controller is \
                 unfalsifiable"
            ),
        }
    }
}

impl std::error::Error for GainError {}

/// The admissible interval for `|Kp + Ki + Kd|`: `(D, 1/L)`.
///
/// Both endpoints are **exclusive** — see the module docs for why each theorem
/// demands its strict inequality.
///
/// Provenance is deliberately not part of this type: the arithmetic is the same
/// whether `(D, L)` were declared by the developer (the compile-time path,
/// where they are hypotheses) or measured from trajectories (the runtime path,
/// where they are facts). What a green `admits` MEANS differs by construction
/// site, and each construction site says so.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StabilityBand {
    /// `D` — `sup|drift(t)|`.
    pub floor: f64,
    /// `1/L` — the reciprocal Lipschitz constant of the refinement map.
    pub ceiling: f64,
}

impl StabilityBand {
    /// Build the band from a drift bound and a Lipschitz constant.
    ///
    /// Returns [`GainError::EmptyBand`] when `D ≥ 1/L` — no gains at all can
    /// stabilise that configuration. `L = 0` (a constant refinement map, which
    /// expands nothing) yields an unbounded ceiling.
    pub fn new(drift_bound: f64, lipschitz: f64) -> Result<Self, GainError> {
        let ceiling = if lipschitz <= 0.0 {
            f64::INFINITY
        } else {
            1.0 / lipschitz
        };
        let floor = drift_bound.max(0.0);
        if floor >= ceiling {
            return Err(GainError::EmptyBand {
                drift_bound: floor,
                ceiling,
            });
        }
        Ok(StabilityBand { floor, ceiling })
    }

    /// The band for a backend with no measured drift and no measured expansion.
    ///
    /// It admits any gains that pass the sign tests — **which is exactly the
    /// vacuous check §119.b exists to replace**, so callers must treat an
    /// uncharacterised backend as a gap to close, not as a pass.
    pub fn uncharacterised() -> Self {
        StabilityBand {
            floor: 0.0,
            ceiling: f64::INFINITY,
        }
    }

    /// Whether this band constrains anything beyond the sign tests.
    pub fn is_characterised(&self) -> bool {
        self.floor > 0.0 && self.ceiling.is_finite()
    }

    /// The full admissibility check: signs, then the band.
    ///
    /// Signs first, because `Kp = -5` should be reported as a negative gain
    /// rather than as a band violation — `effort()` takes an absolute value and
    /// would otherwise describe it in terms the developer cannot map back to
    /// what they wrote.
    pub fn admits(&self, gains: &Gains) -> Result<(), GainError> {
        if gains.kp <= 0.0 {
            return Err(GainError::NonPositiveProportional { kp: gains.kp });
        }
        if gains.ki < 0.0 {
            return Err(GainError::NegativeIntegral { ki: gains.ki });
        }
        if gains.kd < 0.0 {
            return Err(GainError::NegativeDerivative { kd: gains.kd });
        }
        let effort = gains.effort();
        if effort <= self.floor {
            return Err(GainError::BelowDriftFloor {
                effort,
                drift_bound: self.floor,
            });
        }
        if effort >= self.ceiling {
            return Err(GainError::AboveLipschitzCeiling {
                effort,
                ceiling: self.ceiling,
            });
        }
        Ok(())
    }
}

/// Everything a `mandate` declaration states about its control law, exactly as
/// declared — every field optional, because absence and zero are different
/// statements and only the declaration site knows which was meant.
///
/// This is the input to [`MandateStatics::admissibility_errors`], the single
/// judgment the type checker applies. The runtime applies [`StabilityBand`]
/// directly, with the same arithmetic, once the bounds are facts instead of
/// hypotheses.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MandateStatics {
    pub kp: Option<f64>,
    pub ki: Option<f64>,
    pub kd: Option<f64>,
    /// `ε` — must lie in `(0, 1]` when declared.
    pub epsilon: Option<f64>,
    /// `N` — must be `≥ 1` when declared.
    pub max_steps: Option<i64>,
    /// Declared `D`. A hypothesis, not a fact — it travels to the runtime as a
    /// proof obligation.
    pub drift_bound: Option<f64>,
    /// Declared `L`. Same standing as `drift_bound`.
    pub lipschitz: Option<f64>,
}

impl MandateStatics {
    /// Every admissibility violation in this declaration, in one pass.
    ///
    /// All violations are reported, not just the first: a developer who wrote
    /// three defects should not need three compiles to hear about them. The
    /// band check runs only when the sign checks are clean — `admits` would
    /// re-derive the sign errors and the developer would see each one twice.
    pub fn admissibility_errors(&self) -> Vec<GainError> {
        let mut errors = Vec::new();

        if let Some(v) = self.kp {
            if v <= 0.0 {
                errors.push(GainError::NonPositiveProportional { kp: v });
            }
        }
        if let Some(v) = self.ki {
            if v < 0.0 {
                errors.push(GainError::NegativeIntegral { ki: v });
            }
        }
        if let Some(v) = self.kd {
            if v < 0.0 {
                errors.push(GainError::NegativeDerivative { kd: v });
            }
        }
        if let Some(v) = self.epsilon {
            if v <= 0.0 || v > 1.0 {
                errors.push(GainError::ToleranceOutOfRange { epsilon: v });
            }
        }
        if let Some(v) = self.max_steps {
            if v < 1 {
                errors.push(GainError::NonPositiveStepBudget { max_steps: v });
            }
        }
        if let Some(d) = self.drift_bound {
            if d < 0.0 {
                errors.push(GainError::NegativeDriftBound { drift_bound: d });
            }
        }
        if let Some(l) = self.lipschitz {
            if l < 0.0 {
                errors.push(GainError::NegativeLipschitz { lipschitz: l });
            }
        }

        let declares_bounds = self.drift_bound.is_some() || self.lipschitz.is_some();
        if declares_bounds {
            match self.kp {
                None => errors.push(GainError::BoundsWithoutProportionalGain),
                Some(_) if errors.is_empty() => {
                    // Signs and ranges are clean; the band is now meaningful.
                    // An absent Ki/Kd is a zero TERM in the control law — the
                    // sum simply has no such contribution — while remaining
                    // absent for every question about what was declared.
                    let gains = Gains {
                        kp: self.kp.unwrap_or(0.0),
                        ki: self.ki.unwrap_or(0.0),
                        kd: self.kd.unwrap_or(0.0),
                    };
                    match StabilityBand::new(
                        self.drift_bound.unwrap_or(0.0),
                        self.lipschitz.unwrap_or(0.0),
                    ) {
                        Err(e) => errors.push(e),
                        Ok(band) => {
                            if let Err(e) = band.admits(&gains) {
                                errors.push(e);
                            }
                        }
                    }
                }
                Some(_) => {
                    // Sign/range defects already reported; checking the band
                    // against numbers known to be malformed would only produce
                    // a second diagnostic for the same keystroke.
                }
            }
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gains(kp: f64, ki: f64, kd: f64) -> Gains {
        Gains { kp, ki, kd }
    }

    // ---- the band: a BAND, not a sign ----

    #[test]
    fn the_band_rejects_gains_that_are_too_small_to_beat_the_drift() {
        // Every sign test in README §XV passes here; only the paper's
        // precondition catches it.
        let band = StabilityBand::new(0.8, 0.5).expect("D=0.8 < 1/L=2.0");
        let weak = gains(0.3, 0.1, 0.05); // effort 0.45 ≤ D = 0.8
        assert!(weak.kp > 0.0 && weak.ki >= 0.0 && weak.kd >= 0.0);
        match band.admits(&weak) {
            Err(GainError::BelowDriftFloor {
                effort,
                drift_bound,
            }) => {
                assert!((effort - 0.45).abs() < 1e-12);
                assert_eq!(drift_bound, 0.8);
            }
            other => panic!("expected BelowDriftFloor, got {other:?}"),
        }
    }

    #[test]
    fn the_band_rejects_gains_that_are_too_large_to_converge() {
        let band = StabilityBand::new(0.1, 0.5).expect("D=0.1 < 1/L=2.0");
        match band.admits(&gains(2.0, 0.3, 0.1)) {
            // effort 2.4 ≥ 1/L = 2.0
            Err(GainError::AboveLipschitzCeiling { effort, ceiling }) => {
                assert!((effort - 2.4).abs() < 1e-12);
                assert_eq!(ceiling, 2.0);
            }
            other => panic!("expected AboveLipschitzCeiling, got {other:?}"),
        }
    }

    #[test]
    fn the_band_admits_gains_between_the_two_bounds() {
        let band = StabilityBand::new(0.5, 0.5).expect("D=0.5 < 1/L=2.0");
        assert_eq!(band.admits(&gains(1.0, 0.2, 0.05)), Ok(())); // effort 1.25
    }

    #[test]
    fn both_endpoints_are_exclusive_because_both_theorems_are_strict() {
        let band = StabilityBand::new(1.0, 0.5).expect("D=1.0 < 1/L=2.0");
        assert!(
            band.admits(&gains(1.0, 0.0, 0.0)).is_err(),
            "effort exactly at D gives a non-strict Lyapunov derivative"
        );
        assert!(
            band.admits(&gains(2.0, 0.0, 0.0)).is_err(),
            "effort exactly at 1/L gives a contraction factor of exactly 1"
        );
    }

    #[test]
    fn a_backend_whose_drift_exceeds_the_ceiling_has_an_empty_band() {
        match StabilityBand::new(3.0, 0.5) {
            Err(GainError::EmptyBand {
                drift_bound,
                ceiling,
            }) => {
                assert_eq!((drift_bound, ceiling), (3.0, 2.0));
            }
            other => panic!("expected EmptyBand, got {other:?}"),
        }
    }

    #[test]
    fn the_sign_tests_are_reported_as_signs_not_as_band_violations() {
        let band = StabilityBand::new(0.1, 0.5).unwrap();
        assert_eq!(
            band.admits(&gains(-5.0, 0.0, 0.0)),
            Err(GainError::NonPositiveProportional { kp: -5.0 })
        );
        assert_eq!(
            band.admits(&gains(1.0, -0.2, 0.0)),
            Err(GainError::NegativeIntegral { ki: -0.2 })
        );
        assert_eq!(
            band.admits(&gains(1.0, 0.0, -0.3)),
            Err(GainError::NegativeDerivative { kd: -0.3 })
        );
    }

    #[test]
    fn an_uncharacterised_band_says_so_rather_than_pretending_to_have_checked() {
        let band = StabilityBand::uncharacterised();
        assert!(
            !band.is_characterised(),
            "an uncharacterised backend must be visible as a gap; treating this \
             pass as a real check is exactly the vacuous gate this judgment replaces"
        );
        assert!(StabilityBand::new(0.5, 0.5).unwrap().is_characterised());
    }

    #[test]
    fn every_band_violation_names_the_numbers_the_developer_wrote() {
        let band = StabilityBand::new(0.8, 0.5).unwrap();
        let msg = band.admits(&gains(0.3, 0.1, 0.05)).unwrap_err().to_string();
        assert!(msg.contains("0.45") && msg.contains("0.8"), "{msg}");
        assert!(
            msg.contains("paper_mandate"),
            "the refusal must cite the precondition it enforces: {msg}"
        );
    }

    // ---- the whole-declaration judgment ----

    fn statics() -> MandateStatics {
        MandateStatics {
            kp: Some(2.0),
            ki: Some(0.3),
            kd: Some(0.1),
            epsilon: Some(0.05),
            max_steps: Some(8),
            drift_bound: Some(0.5),
            lipschitz: Some(0.4), // ceiling 2.5; effort 2.4 ∈ (0.5, 2.5) ✓
        }
    }

    #[test]
    fn a_fully_declared_admissible_mandate_produces_no_errors() {
        assert_eq!(statics().admissibility_errors(), vec![]);
    }

    #[test]
    fn declared_bounds_turn_readme_legal_gains_into_a_compile_error() {
        // THE test of the whole sub-fase: gains that pass every published sign
        // check are refused once the developer states the drift they must beat.
        let s = MandateStatics {
            kp: Some(0.3),
            ki: Some(0.1),
            kd: Some(0.05),
            drift_bound: Some(0.8),
            lipschitz: Some(0.5),
            ..statics()
        };
        match s.admissibility_errors()[..] {
            [GainError::BelowDriftFloor {
                effort,
                drift_bound,
            }] => {
                assert!((effort - 0.45).abs() < 1e-12, "effort {effort}");
                assert_eq!(drift_bound, 0.8);
            }
            ref other => panic!("expected exactly BelowDriftFloor, got {other:?}"),
        }
    }

    #[test]
    fn without_declared_bounds_only_the_sign_checks_apply() {
        // Backwards compatibility: every mandate written before §119.b.3
        // declares no bounds and must keep compiling.
        let s = MandateStatics {
            drift_bound: None,
            lipschitz: None,
            ..statics()
        };
        assert_eq!(s.admissibility_errors(), vec![]);
    }

    #[test]
    fn epsilon_above_one_is_refused_because_e_cannot_exceed_one() {
        let s = MandateStatics {
            epsilon: Some(1.5),
            ..statics()
        };
        assert_eq!(
            s.admissibility_errors(),
            vec![GainError::ToleranceOutOfRange { epsilon: 1.5 }]
        );
        // ε = 1 is the edge that is NOT vacuous: it rejects exactly total
        // violation, so it stays admissible.
        let edge = MandateStatics {
            epsilon: Some(1.0),
            ..statics()
        };
        assert_eq!(edge.admissibility_errors(), vec![]);
    }

    #[test]
    fn bounds_without_a_proportional_gain_are_a_premise_with_no_theorem() {
        let s = MandateStatics {
            kp: None,
            ki: None,
            kd: None,
            drift_bound: Some(0.5),
            lipschitz: Some(0.4),
            ..statics()
        };
        assert_eq!(
            s.admissibility_errors(),
            vec![GainError::BoundsWithoutProportionalGain]
        );
    }

    #[test]
    fn negative_declared_bounds_are_category_errors_not_loose_bounds() {
        let s = MandateStatics {
            drift_bound: Some(-0.5),
            ..statics()
        };
        assert_eq!(
            s.admissibility_errors(),
            vec![GainError::NegativeDriftBound { drift_bound: -0.5 }]
        );
        let s = MandateStatics {
            lipschitz: Some(-2.0),
            ..statics()
        };
        assert_eq!(
            s.admissibility_errors(),
            vec![GainError::NegativeLipschitz { lipschitz: -2.0 }]
        );
    }

    #[test]
    fn an_absent_gain_is_a_zero_term_in_the_effort_sum() {
        // Kp alone: effort = 2.0. Band (0.5, 2.5) admits it. If absence were
        // treated as anything but a zero term, this would misreport.
        let s = MandateStatics {
            ki: None,
            kd: None,
            ..statics()
        };
        assert_eq!(s.admissibility_errors(), vec![]);
    }

    #[test]
    fn a_declared_empty_band_is_reported_on_the_declaration() {
        let s = MandateStatics {
            drift_bound: Some(3.0),
            lipschitz: Some(0.5),
            ..statics()
        };
        assert_eq!(
            s.admissibility_errors(),
            vec![GainError::EmptyBand {
                drift_bound: 3.0,
                ceiling: 2.0
            }]
        );
    }

    #[test]
    fn sign_defects_suppress_the_band_check_to_avoid_double_reporting() {
        let s = MandateStatics {
            kp: Some(-1.0),
            drift_bound: Some(0.8),
            lipschitz: Some(0.5),
            ..statics()
        };
        let errors = s.admissibility_errors();
        assert_eq!(
            errors,
            vec![GainError::NonPositiveProportional { kp: -1.0 }],
            "one keystroke, one diagnostic — not a sign error AND a band error"
        );
    }

    #[test]
    fn all_independent_defects_are_reported_in_one_pass() {
        let s = MandateStatics {
            kp: Some(-1.0),
            ki: Some(-0.2),
            epsilon: Some(2.0),
            max_steps: Some(0),
            ..MandateStatics::default()
        };
        let errors = s.admissibility_errors();
        assert_eq!(
            errors.len(),
            4,
            "a developer with four defects should not need four compiles: {errors:?}"
        );
    }

    #[test]
    fn a_floor_only_declaration_checks_only_the_floor() {
        // D declared, L not: the ceiling is unbounded, per the papers being
        // independent conditions. Effort 2.4 > D 0.5 ✓.
        let s = MandateStatics {
            lipschitz: None,
            ..statics()
        };
        assert_eq!(s.admissibility_errors(), vec![]);
        // And the floor still bites on its own.
        let weak = MandateStatics {
            kp: Some(0.3),
            ki: None,
            kd: None,
            lipschitz: None,
            ..statics()
        };
        assert!(matches!(
            weak.admissibility_errors()[..],
            [GainError::BelowDriftFloor { .. }]
        ));
    }
}
