//! §Fase 119.b — the `mandate` controller: PID, anti-windup, convergence, and
//! the stability band the compiler must check.
//!
//! Named for `density_matrix.py` in `docs/papers/paper_mandate.md`, which places
//! the controller inside the PEM engine. The name is part of the specification;
//! inventing a different one would break the correspondence between the paper
//! and the code that is supposed to implement it.
//!
//! # What this computes
//!
//! Given `e(t)` from [`super::semantic_validator`] — and nothing else — it
//! produces the control effort README §XV publishes:
//!
//! ```text
//! u(t) = Kp·e(t) + Ki·∫e dt + Kd·de/dt
//! ```
//!
//! plus the two things that make the loop terminate rather than spin:
//!
//! - **Anti-windup.** `I_clamped = clamp(∫e, −I_max, I_max)`. Without it, a
//!   mandate the model cannot satisfy accumulates unbounded integral, and the
//!   effort keeps rising after it has stopped helping.
//! - **`Converge(e, ε, N) = ∃t ≤ N : |e(t)| < ε`.** Bounded, and when the bound
//!   is spent the answer is [`Outcome::Breach`] — never a best-effort response.
//!   That is the invariant the whole primitive rests on: **non-compliance can
//!   never be shipped.**
//!
//! # The stability band, and why a sign test is not enough
//!
//! README §XV publishes only `Kp > 0 ∧ Ki ≥ 0 ∧ Kd ≥ 0 ∧ ε > 0 ∧ N ≥ 1`, and a
//! compiler that checked exactly that would admit both failure modes, because
//! the two papers bound the gains **in opposite directions**:
//!
//! - `paper_mandate.md` §3 (Lyapunov). Theorem 1 holds only *"if the configured
//!   control effort exceeds the natural drift"*, with `sup|drift(t)| ≤ D`. Gains
//!   that are too **small** leave the cage open — `V(e) = ½e²` does not decrease
//!   and the argument never applies. The README omits this precondition
//!   entirely, which would have made the static check vacuous.
//! - `paper_mathematical_prompt_optimization.md` §6.3 (Lipschitz). Convergence
//!   requires `|Kp + Ki + Kd| < 1/L`. Gains that are too **large** overshoot;
//!   the loop oscillates or diverges.
//!
//! Together they bound the gains on **both** sides:
//!
//! ```text
//! D < |Kp + Ki + Kd| < 1/L
//! ```
//!
//! So the compile-time rule is a **band, not a sign** — see [`StabilityBand`].
//! And when `D ≥ 1/L` the band is *empty*: no gains whatever can cage that
//! backend, which is a fact worth reporting rather than a configuration to keep
//! tuning ([`GainError::EmptyBand`]).
//!
//! # What is deliberately absent
//!
//! No I/O, no backend, no model. Everything here is a pure function of numbers,
//! so the controller can be tested exhaustively without a network. Turning
//! `u(t)` into a logit bias (Tier 1) or into corrective pressure on the next
//! attempt (Tier 2) is the actuator's job and lives elsewhere.

use std::fmt;

/// The three gains of `u(t) = Kp·e + Ki·∫e + Kd·de/dt`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gains {
    pub kp: f64,
    pub ki: f64,
    pub kd: f64,
}

impl Gains {
    /// The aggregate the Lipschitz condition bounds: `|Kp + Ki + Kd|`.
    pub fn effort(&self) -> f64 {
        (self.kp + self.ki + self.kd).abs()
    }
}

/// Why a mandate's declared gains are not admissible.
///
/// Every variant carries the numbers, because a diagnostic that says "unstable
/// gains" without saying which bound was crossed and by how much cannot be acted
/// on by the developer who wrote them.
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
    /// `ε ≤ 0`: a band no response can enter. Published in README §XV.
    NonPositiveTolerance { epsilon: f64 },
    /// `N < 1`: a budget that permits no attempt.
    NonPositiveStepBudget { max_steps: u32 },
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
                "the control effort |Kp+Ki+Kd| = {effort} does not exceed this \
                 backend's measured drift bound D = {drift_bound}. The Lyapunov \
                 argument for this mandate is conditional on exceeding it \
                 (paper_mandate §3), so with these gains the cage does not hold"
            ),
            GainError::AboveLipschitzCeiling { effort, ceiling } => write!(
                f,
                "the control effort |Kp+Ki+Kd| = {effort} is at or above the \
                 convergence ceiling 1/L = {ceiling}; the refinement loop would \
                 oscillate or diverge rather than settle (prompt_opt §6.3)"
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
            GainError::NonPositiveTolerance { epsilon } => write!(
                f,
                "epsilon is {epsilon}; a convergence band that is not strictly \
                 positive is one no response can ever enter"
            ),
            GainError::NonPositiveStepBudget { max_steps } => write!(
                f,
                "max_steps is {max_steps}; a mandate must permit at least one \
                 refinement attempt"
            ),
        }
    }
}

impl std::error::Error for GainError {}

/// The admissible interval for `|Kp + Ki + Kd|`: `(D, 1/L)`.
///
/// Both endpoints are **exclusive**. At `effort == D` the control effort merely
/// matches the drift and the Lyapunov derivative is not strictly negative; at
/// `effort == 1/L` the contraction factor is exactly 1 and the map is no longer
/// a contraction. Neither endpoint gives the strict inequality its theorem
/// requires, so neither is admitted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StabilityBand {
    /// `D` — `sup|drift(t)|` for the backend, measured, not assumed.
    pub floor: f64,
    /// `1/L` — the reciprocal Lipschitz constant of the refinement map.
    pub ceiling: f64,
}

impl StabilityBand {
    /// Build the band from a measured drift bound and Lipschitz constant.
    ///
    /// Returns [`GainError::EmptyBand`] when `D ≥ 1/L` — the backend admits no
    /// stable mandate at all.
    pub fn new(drift_bound: f64, lipschitz: f64) -> Result<Self, GainError> {
        let ceiling = if lipschitz <= 0.0 {
            // L = 0 means the refinement map does not expand error at all;
            // there is no upper bound to respect.
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
    /// Used when a backend has not been characterised. It admits any gains that
    /// pass the sign tests — **which is exactly the vacuous check §119.b exists
    /// to replace**, so callers must treat an uncharacterised backend as a gap
    /// to close, not as a pass.
    pub fn uncharacterised() -> Self {
        StabilityBand {
            floor: 0.0,
            ceiling: f64::INFINITY,
        }
    }

    /// Whether this band came from real measurements.
    pub fn is_characterised(&self) -> bool {
        self.floor > 0.0 && self.ceiling.is_finite()
    }

    /// The full compile-time check: signs, then the band.
    ///
    /// This is the function `axon check` must call. Signs first, because
    /// `Kp = -5` should be reported as a negative gain rather than as a band
    /// violation — `effort()` takes an absolute value and would otherwise
    /// describe it in terms the developer cannot map back to what they wrote.
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

/// What the controller says to do after observing one `e(t)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// `|e| < ε`. The response satisfies the mandate; ship it.
    Converged,
    /// Not yet inside the band, and budget remains. Refine with `effort`.
    Refine,
    /// The step budget `N` is spent and `|e| ≥ ε`.
    ///
    /// **The flow fails.** No best-effort response is returned — that is the
    /// whole of "the instruction cannot be evaded": not that every model obeys,
    /// but that disobedience cannot be shipped.
    Breach,
}

/// One observation of the loop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Control {
    /// `u(t)` — the control effort. Tier 1 turns this into a logit bias; Tier 2
    /// turns it into corrective pressure on the next attempt.
    pub effort: f64,
    /// The error that produced it.
    pub error: f64,
    /// `V(e) = ½e²`, the Lyapunov function of `paper_mandate.md` §3. Recorded so
    /// the descent claim can be **checked on the trajectory** rather than
    /// asserted in a comment.
    pub lyapunov: f64,
    /// 1-based step index.
    pub step: u32,
    pub outcome: Outcome,
}

/// Everything the controller needs, validated at construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MandateSpec {
    pub gains: Gains,
    /// `ε` — the convergence band.
    pub epsilon: f64,
    /// `N` — the step budget.
    pub max_steps: u32,
}

impl MandateSpec {
    /// Validate the whole declaration against a band.
    pub fn validate(&self, band: &StabilityBand) -> Result<(), GainError> {
        if self.epsilon <= 0.0 {
            return Err(GainError::NonPositiveTolerance {
                epsilon: self.epsilon,
            });
        }
        if self.max_steps < 1 {
            return Err(GainError::NonPositiveStepBudget {
                max_steps: self.max_steps,
            });
        }
        band.admits(&self.gains)
    }
}

/// The closed-loop controller. One instance per mandate application.
#[derive(Debug, Clone)]
pub struct Controller {
    spec: MandateSpec,
    integral: f64,
    i_max: f64,
    last_error: Option<f64>,
    step: u32,
    trajectory: Vec<f64>,
}

/// The anti-windup clamp: `I_max = Kp / Ki`, so that `Ki·I_max ≤ Kp·1`.
///
/// **The integral term can never exceed what the proportional term contributes
/// at full error.** That is the bound worth enforcing, because the failure
/// anti-windup exists to prevent is precisely the integral growing until it,
/// rather than the present error, decides the effort — at which point the loop
/// is steering by history and overshoots when the error finally drops.
///
/// An earlier draft scaled the clamp with `ε` and floored it at `1.0`. The floor
/// dominated for every `ε < 0.1` — which is every tight mandate, including
/// README §XV's own `ε = 0.05` — so the clamp was a constant wearing a
/// rationale about scaling. Deriving it from the gains removes the free
/// parameter entirely: there is nothing left to tune, and nothing to get wrong.
///
/// With `Ki = 0` there is no integral term, so the clamp is irrelevant and the
/// accumulator is pinned at zero rather than dividing by zero.
fn integral_clamp(gains: &Gains) -> f64 {
    if gains.ki > 0.0 {
        gains.kp / gains.ki
    } else {
        0.0
    }
}

impl Controller {
    /// Build a controller, rejecting a declaration the band does not admit.
    pub fn new(spec: MandateSpec, band: &StabilityBand) -> Result<Self, GainError> {
        spec.validate(band)?;
        Ok(Controller {
            spec,
            integral: 0.0,
            i_max: integral_clamp(&spec.gains),
            last_error: None,
            step: 0,
            trajectory: Vec::new(),
        })
    }

    /// Feed one `e(t)` and get the control action.
    ///
    /// `e` is expected in `[0, 1]` (the range [`super::semantic_validator`]
    /// produces) but nothing here requires it; the arithmetic is total.
    pub fn observe(&mut self, error: f64) -> Control {
        self.step += 1;
        self.trajectory.push(error);

        // ∫e, then the anti-windup clamp. Clamping the ACCUMULATOR (not just
        // the term) is what stops a permanently-unsatisfiable mandate from
        // carrying a huge integral into a step where the error has finally
        // dropped — the classic windup overshoot.
        self.integral = (self.integral + error).clamp(-self.i_max, self.i_max);

        // de/dt across unit steps; zero on the first observation, where no
        // derivative exists. Seeding it with `error` instead would invent a
        // step-change from zero that never happened.
        let derivative = match self.last_error {
            Some(prev) => error - prev,
            None => 0.0,
        };
        self.last_error = Some(error);

        let effort = self.spec.gains.kp * error
            + self.spec.gains.ki * self.integral
            + self.spec.gains.kd * derivative;

        let outcome = if error.abs() < self.spec.epsilon {
            Outcome::Converged
        } else if self.step >= self.spec.max_steps {
            Outcome::Breach
        } else {
            Outcome::Refine
        };

        Control {
            effort,
            error,
            lyapunov: 0.5 * error * error,
            step: self.step,
            outcome,
        }
    }

    /// The observed error trajectory, oldest first.
    pub fn trajectory(&self) -> &[f64] {
        &self.trajectory
    }

    /// Whether `V(e) = ½e²` decreased at every step of the observed trajectory.
    ///
    /// This is the Lyapunov claim, **checked** rather than asserted. It is
    /// reported, not enforced: a single non-decreasing step is normal for a
    /// stochastic plant, and refusing on it would make the primitive unusable.
    /// What is enforced is convergence within `N` — see [`Outcome::Breach`].
    pub fn lyapunov_descends(&self) -> bool {
        self.trajectory
            .windows(2)
            .all(|w| 0.5 * w[1] * w[1] < 0.5 * w[0] * w[0])
    }

    pub fn steps_taken(&self) -> u32 {
        self.step
    }

    /// The anti-windup bound in force, `Kp / Ki`.
    pub fn integral_clamp(&self) -> f64 {
        self.i_max
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gains(kp: f64, ki: f64, kd: f64) -> Gains {
        Gains { kp, ki, kd }
    }

    fn spec(kp: f64, ki: f64, kd: f64, epsilon: f64, max_steps: u32) -> MandateSpec {
        MandateSpec {
            gains: gains(kp, ki, kd),
            epsilon,
            max_steps,
        }
    }

    fn open_controller(s: MandateSpec) -> Controller {
        Controller::new(s, &StabilityBand::uncharacterised()).expect("admissible")
    }

    // ---- the control law itself ----

    #[test]
    fn the_first_observation_has_no_derivative_term() {
        // Kd is large; if the first step invented de/dt = e − 0, the effort
        // would be 1.0*0.4 + 100.0*0.4 rather than 1.0*0.4.
        let mut c = open_controller(spec(1.0, 0.0, 100.0, 0.01, 5));
        let out = c.observe(0.4);
        assert!(
            (out.effort - 0.4).abs() < 1e-12,
            "first-step effort {} should be Kp·e alone",
            out.effort
        );
    }

    #[test]
    fn effort_is_the_published_sum_of_three_terms() {
        let mut c = open_controller(spec(2.0, 0.5, 0.25, 0.01, 10));
        c.observe(0.8); // integral = 0.8, derivative = 0
        let out = c.observe(0.6);
        // integral = 1.4, derivative = -0.2
        let expected = 2.0 * 0.6 + 0.5 * 1.4 + 0.25 * -0.2;
        assert!(
            (out.effort - expected).abs() < 1e-12,
            "got {}, expected {expected}",
            out.effort
        );
    }

    #[test]
    fn the_integral_accumulator_is_clamped_not_merely_its_term() {
        // A mandate the model never satisfies: e = 1.0 forever.
        let mut c = open_controller(spec(1.0, 1.0, 0.0, 0.05, 100));
        for _ in 0..80 {
            c.observe(1.0);
        }
        let i_max = c.integral_clamp();
        assert!(
            c.integral <= i_max + 1e-12,
            "integral wound up to {} past the clamp {i_max}",
            c.integral
        );
        // The decisive consequence: when the error finally collapses, the
        // effort must collapse with it rather than carrying 80 steps of
        // accumulated pressure into a response that is already correct.
        let out = c.observe(0.0);
        assert!(
            out.effort <= i_max + 1e-12,
            "windup survived the clamp: effort {} after the error hit zero",
            out.effort
        );
    }

    #[test]
    fn lyapunov_is_half_e_squared() {
        let mut c = open_controller(spec(1.0, 0.0, 0.0, 0.01, 5));
        assert!((c.observe(0.4).lyapunov - 0.08).abs() < 1e-12);
    }

    // ---- Converge(e, ε, N): the fail-closed invariant ----

    #[test]
    fn entering_the_band_converges() {
        let mut c = open_controller(spec(1.0, 0.0, 0.0, 0.1, 5));
        assert_eq!(c.observe(0.5).outcome, Outcome::Refine);
        assert_eq!(c.observe(0.05).outcome, Outcome::Converged);
    }

    #[test]
    fn the_band_is_strict_so_e_equal_to_epsilon_is_not_convergence() {
        // README §XV: `∃t ≤ N : |e(t)| < ε`.
        let mut c = open_controller(spec(1.0, 0.0, 0.0, 0.1, 5));
        assert_eq!(c.observe(0.1).outcome, Outcome::Refine);
    }

    #[test]
    fn exhausting_the_budget_breaches_it_never_returns_best_effort() {
        let mut c = open_controller(spec(1.0, 0.0, 0.0, 0.01, 3));
        assert_eq!(c.observe(0.9).outcome, Outcome::Refine);
        assert_eq!(c.observe(0.8).outcome, Outcome::Refine);
        let last = c.observe(0.7);
        assert_eq!(
            last.outcome,
            Outcome::Breach,
            "a spent budget with e outside the band is a breach; anything else \
             ships a response that violates the mandate"
        );
    }

    #[test]
    fn converging_exactly_on_the_last_permitted_step_is_convergence_not_breach() {
        // The off-by-one that would turn a satisfied mandate into a failure.
        let mut c = open_controller(spec(1.0, 0.0, 0.0, 0.1, 2));
        assert_eq!(c.observe(0.9).outcome, Outcome::Refine);
        assert_eq!(c.observe(0.01).outcome, Outcome::Converged);
    }

    #[test]
    fn a_single_step_budget_still_permits_one_attempt() {
        let mut c = open_controller(spec(1.0, 0.0, 0.0, 0.1, 1));
        assert_eq!(c.observe(0.01).outcome, Outcome::Converged);
        let mut c = open_controller(spec(1.0, 0.0, 0.0, 0.1, 1));
        assert_eq!(c.observe(0.9).outcome, Outcome::Breach);
    }

    // ---- the stability band: a BAND, not a sign ----

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
        // D = 3.0, 1/L = 2.0. No gains at all can cage this backend, and that
        // is a property of the backend rather than a declaration to keep tuning.
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
        // effort() takes an absolute value, so Kp = -5 would otherwise be
        // described to the developer in terms of a bound they never wrote.
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
             pass as a real check is exactly the vacuous gate §119.b replaces"
        );
        assert!(StabilityBand::new(0.5, 0.5).unwrap().is_characterised());
    }

    #[test]
    fn epsilon_and_step_budget_are_rejected_at_the_declaration() {
        let band = StabilityBand::uncharacterised();
        assert_eq!(
            spec(1.0, 0.0, 0.0, 0.0, 5).validate(&band),
            Err(GainError::NonPositiveTolerance { epsilon: 0.0 })
        );
        assert_eq!(
            spec(1.0, 0.0, 0.0, -0.1, 5).validate(&band),
            Err(GainError::NonPositiveTolerance { epsilon: -0.1 })
        );
        assert_eq!(
            spec(1.0, 0.0, 0.0, 0.1, 0).validate(&band),
            Err(GainError::NonPositiveStepBudget { max_steps: 0 })
        );
    }

    #[test]
    fn a_controller_cannot_be_built_from_a_declaration_the_band_refuses() {
        let band = StabilityBand::new(0.8, 0.5).unwrap();
        assert!(Controller::new(spec(0.3, 0.1, 0.05, 0.1, 5), &band).is_err());
    }

    #[test]
    fn every_error_variant_names_the_numbers_the_developer_wrote() {
        let band = StabilityBand::new(0.8, 0.5).unwrap();
        let msg = band.admits(&gains(0.3, 0.1, 0.05)).unwrap_err().to_string();
        assert!(msg.contains("0.45") && msg.contains("0.8"), "{msg}");
        assert!(
            msg.contains("paper_mandate"),
            "the refusal must cite the precondition it enforces: {msg}"
        );
    }

    // ---- Vía B: the paper's simulation, as a gate ----

    /// A declared plant: error drifts up by `drift` each step and is pulled down
    /// by the control effort with authority `authority`.
    ///
    /// **This validates the CONTROLLER against a stated plant. It does not
    /// validate the plant against a real model** — that is what Tier 1 and
    /// Tier 2 must do, on measured trajectories. Stating the boundary here
    /// rather than letting a green test imply more than it shows.
    fn simulate(spec: MandateSpec, drift: f64, authority: f64, controlled: bool) -> Vec<f64> {
        let mut c = Controller::new(spec, &StabilityBand::uncharacterised()).unwrap();
        let mut e: f64 = 1.0;
        let mut trace = vec![e];
        for _ in 0..spec.max_steps {
            let out = c.observe(e);
            let u = if controlled { out.effort } else { 0.0 };
            e = (e + drift - authority * u).clamp(0.0, 1.0);
            trace.push(e);
            if controlled && out.outcome == Outcome::Converged {
                break;
            }
        }
        trace
    }

    #[test]
    fn via_b_the_controlled_trajectory_settles_where_the_free_one_does_not() {
        let s = spec(1.2, 0.15, 0.1, 0.05, 40);
        let free = simulate(s, 0.02, 0.35, false);
        let controlled = simulate(s, 0.02, 0.35, true);

        let free_final = *free.last().unwrap();
        let controlled_final = *controlled.last().unwrap();

        assert!(
            free_final >= 0.99,
            "the uncontrolled baseline must NOT settle; got {free_final}. If it \
             does, the simulation proves nothing about the controller"
        );
        assert!(
            controlled_final < s.epsilon,
            "the controlled trajectory must enter the band; got {controlled_final}"
        );
        assert!(
            controlled.len() < free.len(),
            "convergence must terminate the loop early: controlled {} steps vs \
             free {}",
            controlled.len(),
            free.len()
        );
    }

    #[test]
    fn via_b_lyapunov_descent_is_checked_on_the_trajectory_not_assumed() {
        let s = spec(1.2, 0.15, 0.1, 0.05, 40);
        let mut c = Controller::new(s, &StabilityBand::uncharacterised()).unwrap();
        for e in [1.0, 0.7, 0.45, 0.2, 0.04] {
            c.observe(e);
        }
        assert!(c.lyapunov_descends(), "V(e) = ½e² decreased at every step");

        let mut bumpy = Controller::new(s, &StabilityBand::uncharacterised()).unwrap();
        for e in [1.0, 0.7, 0.75, 0.2] {
            bumpy.observe(e);
        }
        assert!(
            !bumpy.lyapunov_descends(),
            "a non-monotone trajectory must be REPORTED as non-descending; a \
             predicate that returns true regardless proves nothing"
        );
    }

    #[test]
    fn the_trajectory_is_recorded_in_order() {
        let mut c = open_controller(spec(1.0, 0.0, 0.0, 0.01, 10));
        for e in [0.9, 0.5, 0.2] {
            c.observe(e);
        }
        assert_eq!(c.trajectory(), &[0.9, 0.5, 0.2]);
        assert_eq!(c.steps_taken(), 3);
    }
}
