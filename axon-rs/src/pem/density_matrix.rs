//! v2.83.0 — the `mandate` controller: PID, anti-windup, convergence, and
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
//! produces the control effort README XV publishes:
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
//! README XV publishes only `Kp > 0 ∧ Ki ≥ 0 ∧ Kd ≥ 0 ∧ ε > 0 ∧ N ≥ 1`, and a
//! compiler that checked exactly that would admit both failure modes, because
//! the two papers bound the gains **in opposite directions**:
//!
//! - `paper_mandate.md` section 3 (Lyapunov). Theorem 1 holds only *"if the configured
//!   control effort exceeds the natural drift"*, with `sup|drift(t)| ≤ D`. Gains
//!   that are too **small** leave the cage open — `V(e) = ½e²` does not decrease
//!   and the argument never applies. The README omits this precondition
//!   entirely, which would have made the static check vacuous.
//! - `paper_mathematical_prompt_optimization.md` section 6.3 (Lipschitz). Convergence
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

// v2.83.0 — the band arithmetic moved to `axon_frontend::stability`.
//
// It was born here, next to the controller that first needed it — which is the
// v2.81.0 smell: the TYPE CHECKER needs the same judgment (README XV promises
// compile-time rejection of unstable gains), the frontend has zero deps, and
// this crate already depends on it. One admissibility judgment, two call
// sites; a mandate the compiler admits is exactly a mandate this controller
// would admit. The re-export keeps `axon::pem::StabilityBand` working.
pub use axon_frontend::stability::{GainError, Gains, StabilityBand};

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
    /// `V(e) = ½e²`, the Lyapunov function of `paper_mandate.md` section 3. Recorded so
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
        // ε ∈ (0, 1], NOT merely ε > 0: e = 1 − CSR is confined to [0, 1], so a
        // band wider than 1 admits even total violation — a mandate that could
        // never fail. Same rule as the compile-time judgment, same reason.
        if self.epsilon <= 0.0 || self.epsilon > 1.0 {
            return Err(GainError::ToleranceOutOfRange {
                epsilon: self.epsilon,
            });
        }
        if self.max_steps < 1 {
            return Err(GainError::NonPositiveStepBudget {
                max_steps: i64::from(self.max_steps),
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
/// README XV's own `ε = 0.05` — so the clamp was a constant wearing a
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
        // README XV: `∃t ≤ N: |e(t)| < ε`.
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
    fn epsilon_and_step_budget_are_rejected_at_the_declaration() {
        let band = StabilityBand::uncharacterised();
        assert_eq!(
            spec(1.0, 0.0, 0.0, 0.0, 5).validate(&band),
            Err(GainError::ToleranceOutOfRange { epsilon: 0.0 })
        );
        assert_eq!(
            spec(1.0, 0.0, 0.0, -0.1, 5).validate(&band),
            Err(GainError::ToleranceOutOfRange { epsilon: -0.1 })
        );
        assert_eq!(
            spec(1.0, 0.0, 0.0, 1.5, 5).validate(&band),
            Err(GainError::ToleranceOutOfRange { epsilon: 1.5 }),
            "ε > 1 is vacuous: e = 1 − CSR cannot exceed 1, so the mandate              could never fail"
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
