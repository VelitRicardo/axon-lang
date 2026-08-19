//! v2.83.0 — the `mandate` enforcement engine: the loop that makes the
//! cage real.
//!
//! # What this is
//!
//! Everything before this file measured and validated: `pem::semantic_validator`
//! computes `e = 1 − CSR`, `pem::density_matrix` computes `u(t)` and decides
//! `Converged / Refine / Breach`, `axon_frontend::stability` verifies the gain
//! band at compile time conditionally on declared `(D, L)`. This file is the
//! ACTUATOR: it drives a model through that loop and refuses to release any
//! output the constraint set rejects.
//!
//! # The guarantee, stated as what cannot happen
//!
//! *The defensible form of "the instruction cannot be evaded" is not that every
//! model obeys — it is that non-compliance can never be SHIPPED.* Concretely:
//!
//! - An output leaves [`MandateRuntime::enforce`] as `Ok` **only** if the
//!   constraint set accepted it (`|e| < ε`, or full satisfaction when no ε was
//!   declared).
//! - A spent budget is [`MandateBreach::Unconverged`] — never a silent
//!   best-effort return. The `on_violation: coerce` policy is applied by the
//!   DISPATCH layer, visibly, with a runtime warning; the engine itself never
//!   converts a breach into a success.
//! - A mandate whose constraint text lowers to **zero checkable predicates**
//!   is [`MandateRefusal::Unfalsifiable`] — refused before any token is spent,
//! because a mandate that cannot fail enforces nothing (v2.67.0's
//! lease-with-no-subject, v2.83.0's founding refusal).
//! - **Mandated generation is BUFFERED, never streamed raw.** A token that has
//!   reached the wire cannot be unshipped, so a mandate that streamed its
//!   attempts would ship non-compliance mid-sentence and apologise afterwards.
//!   Callers get the final, validated output or an error — nothing in between.
//!
//! # The proof obligations, discharged
//!
//! v2.83.0's compile-time band check is CONDITIONAL on the declared drift
//! bound `D` (`sup|drift(t)| ≤ D`, `paper_mandate.md` section 3). This engine is where
//! the hypothesis meets reality. The plant model is
//!
//! ```text
//! e(t+1) = e(t) + drift(t) − u_eff(t),   u_eff(t) ≥ 0
//! ```
//!
//! so the observable step `Δe = e(t+1) − e(t)` is a LOWER bound on `drift(t)`.
//! When `Δe > D`, the drift provably exceeded the declared bound — no
//! assumption about the model's compliance authority is needed — and the
//! static stability proof is void because its premise is false. That is
//! [`MandateBreach::PremiseViolated`], raised the moment it is observed rather
//! than at budget exhaustion: a theorem with a false hypothesis does not
//! deserve its remaining steps. The test is one-sided and sound: `u_eff ≥ 0`
//! means `Δe ≤ drift`, so it can under-report drift but never over-report it —
//! a `PremiseViolated` is never a false alarm.
//!
//! # Tier 1 and Tier 2 — two actuators, one guarantee
//!
//! - **Tier 2 (universal):** the violated obligations of each attempt are fed
//!   back verbatim as the next attempt's corrective directive — the CSP
//! solver's backtracking step (`prompt_opt` section 5.3: *identify violated `c_j` →
//!   inject as feedback → regenerate*). Works on every backend that can
//!   complete a chat, which is every backend.
//! - **Tier 1 (token-level, where the wire admits it):** every
//!   [`Predicate::Absent`] needle is tokenised (axon-csys BPE) and its tokens
//!   are **banned** — logit bias −100, the OpenAI wire's floor. A ban IS the
//!   thermodynamic cage for a forbidden lexeme: the probability mass of those
//!   tokens is collapsed before sampling, not argued with afterwards. There is
//!   deliberately no scaling by `u(t)` here — an earlier design scaled the
//!   bias with the control effort and that was a free parameter wearing a
//!   theory costume; `Absent` means absent, and −100 is what absent costs.
//!   Only `Absent` constraints yield bans (a `RequiredWith` trigger cannot be
//!   banned — the clause permits it when the disclaimer follows). The
//!   remaining constraint kinds are enforced by the loop itself.
//!
//! `u(t)` is still computed, recorded per step, and reported in the
//! enforcement report — it is the audit trail of the controller, and Lyapunov
//! descent is CHECKED on the recorded trajectory (`density_matrix`), never
//! asserted.
//!
//! # What this file does not know
//!
//! No `DispatchCtx`, no wire events, no policy application. The dispatch layer
//! owns `on_violation` (halt / retry / coerce) because policy is a property of
//! the CALL SITE's contract with the flow, not of the control loop. This
//! separation is what lets the same engine drive the flow-level
//! `mandate X on Y` node and the step-scoped guard identically.

use std::collections::HashMap;
use std::fmt;

use crate::ir_nodes::IRMandate;
use crate::pem::density_matrix::{Controller, Gains, MandateSpec, StabilityBand};
use crate::pem::semantic_validator::{ConstraintSet, Predicate, ValidatorError, Verdict};

/// Why a mandate could not even begin to be enforced. All of these fire
/// BEFORE the first token is spent.
#[derive(Debug, Clone, PartialEq)]
pub enum MandateRefusal {
    /// The constraint text lowered to zero checkable predicates. The clauses
    /// that could not be lowered are named — they are what the developer
    /// believed was being enforced.
    Unfalsifiable { clauses: Vec<String> },
    /// The declaration itself is inadmissible (gains outside the declared
    /// band, ε out of range…). The compiler catches this statically for
    /// declared bounds; the runtime re-refuses rather than trusting that
    /// every IR it receives passed through the checker.
    Inadmissible { detail: String },
}

impl fmt::Display for MandateRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MandateRefusal::Unfalsifiable { clauses } => {
                write!(
                    f,
                    "this mandate has no machine-checkable constraint, so no response \
                     could ever violate it — a cage that cannot close holds nothing. \
                     Refused before any token was spent"
                )?;
                if !clauses.is_empty() {
                    write!(f, ". The clauses that could not be lowered: {}", clauses.join("; "))?;
                }
                Ok(())
            }
            MandateRefusal::Inadmissible { detail } => write!(f, "{detail}"),
        }
    }
}

/// How enforcement failed after it began. Either way, **no output ships from
/// here** — the dispatch layer decides what the flow does next, visibly.
#[derive(Debug, Clone, PartialEq)]
pub enum MandateBreach {
    /// The step budget `N` is spent and the last error is outside the band.
    /// Carries the best (lowest-`e`) attempt so a `coerce` policy at the
    /// dispatch layer can apply it — WITH its error, so the caller can never
    /// pretend it converged.
    Unconverged {
        final_error: f64,
        best_error: f64,
        best_output: String,
        steps_taken: u32,
        trajectory: Vec<f64>,
        /// The obligations the final attempt still violated, verbatim.
        outstanding: Vec<String>,
    },
    /// The observed error increase between consecutive attempts exceeded the
    /// DECLARED drift bound `D`. The static stability proof was conditional on
    /// `sup|drift| ≤ D`; the premise is false on this backend, so the proof is
    /// void and continuing would be spending budget on a theorem that no
    /// longer applies.
    PremiseViolated {
        observed_drift: f64,
        declared_bound: f64,
        at_step: u32,
        trajectory: Vec<f64>,
    },
}

impl MandateBreach {
    /// The refusal text the dispatch layer surfaces. Names the numbers.
    pub fn describe(&self, mandate: &str) -> String {
        match self {
            MandateBreach::Unconverged {
                final_error,
                steps_taken,
                outstanding,
                ..
            } => format!(
                "Anchor Breach: mandate '{mandate}' did not converge — after {steps_taken} \
                 attempt(s) the semantic error is {final_error:.4} and the following \
                 obligations remain violated: {}. No best-effort output is released on this \
                 path; non-compliance cannot be shipped.",
                if outstanding.is_empty() {
                    "(none recorded)".to_string()
                } else {
                    outstanding.join("; ")
                }
            ),
            MandateBreach::PremiseViolated {
                observed_drift,
                declared_bound,
                at_step,
                ..
            } => format!(
                "Anchor Breach: mandate '{mandate}' declared a drift bound D = \
                 {declared_bound}, but at attempt {at_step} the error rose by \
                 {observed_drift:.4} in one step — the drift provably exceeds the declared \
                 bound, the premise of the stability proof is false on this backend, and \
                 the compile-time verification no longer covers this run. Re-measure D or \
                 raise the declaration."
            ),
        }
    }
}

/// What the engine hands the actuator for the next attempt.
#[derive(Debug, Clone, PartialEq)]
pub struct RefinementDirective {
    /// 1-based attempt number about to run.
    pub attempt: u32,
    /// The violated obligations of the previous attempt, rendered as the
    /// corrective feedback block (empty on the first attempt).
    pub feedback: String,
    /// `u(t)` from the controller after the previous attempt, when a control
    /// law is declared. Audit signal; Tier 2 semantics do not scale with it.
    pub effort: Option<f64>,
}

/// The successful outcome: a validated output plus the full audit trail.
#[derive(Debug, Clone, PartialEq)]
pub struct MandateEnforcement {
    /// The output that satisfied the constraint set. The ONLY field callers
    /// should bind into the flow.
    pub output: String,
    /// Attempts actually spent (0 = the initial content was already
    /// compliant and no model was consulted).
    pub attempts: u32,
    /// `e` per evaluation, in order (includes the initial-content evaluation
    /// when one happened).
    pub trajectory: Vec<f64>,
    /// Whether `V(e) = ½e²` decreased at every step — CHECKED on the recorded
    /// trajectory. Reported, not enforced: one non-monotone step is normal
    /// for a stochastic plant; convergence within `N` is what is enforced.
    pub lyapunov_descended: bool,
    /// Clauses of the constraint text that steer the prompt but could not be
    /// lowered to checkable predicates. Non-empty means partial mechanical
    /// coverage, and callers surface it rather than implying a full cage.
    pub unchecked_clauses: Vec<String>,
}

/// The per-application enforcement state: one [`MandateRuntime`] per
/// `mandate X on Y` site, built from the declaration in the IR.
pub struct MandateRuntime {
    pub name: String,
    constraints: ConstraintSet,
    /// Present iff the declaration carries a control law (`Kp` declared).
    controller: Option<Controller>,
    /// Declared ε. `None` = strictest reading: full satisfaction required.
    epsilon: Option<f64>,
    /// Declared N. `None` = strictest reading: one attempt.
    budget: u32,
    /// Declared drift bound `D` — the proof obligation to discharge.
    declared_drift: Option<f64>,
}

impl MandateRuntime {
    /// Build the runtime from the IR declaration, refusing everything that
    /// would make enforcement vacuous or unsound. Zero tokens spent here.
    pub fn from_spec(spec: &IRMandate) -> Result<Self, MandateRefusal> {
        let clauses = ConstraintSet::lower(&spec.constraint);
        let constraints = ConstraintSet::new(clauses).map_err(|e| match e {
            ValidatorError::NoCheckableConstraints { clauses } => {
                MandateRefusal::Unfalsifiable { clauses }
            }
            other => MandateRefusal::Inadmissible {
                detail: other.to_string(),
            },
        })?;

        // The control law is optional in the grammar; absence means a pure
        // convergence loop. What is NOT done: inventing gains. A fabricated
        // Kp would put a number nobody declared into an audit trail that
        // claims to be a measurement.
        let controller = match spec.kp {
            None => None,
            Some(kp) => {
                let band = match (spec.drift_bound, spec.lipschitz) {
                    (None, None) => StabilityBand::uncharacterised(),
                    (d, l) => StabilityBand::new(d.unwrap_or(0.0), l.unwrap_or(0.0))
                        .map_err(|e| MandateRefusal::Inadmissible {
                            detail: e.to_string(),
                        })?,
                };
                let mandate_spec = MandateSpec {
                    gains: Gains {
                        kp,
                        // Absent term = zero term in the control law — the
                        // v2.83.0 rule, same arithmetic as the compiler.
                        ki: spec.ki.unwrap_or(0.0),
                        kd: spec.kd.unwrap_or(0.0),
                    },
                    // The runtime controller requires a concrete ε/N; when the
                    // declaration omits them the STRICTEST values apply (full
                    // satisfaction, one attempt) — see `epsilon`/`budget`
                    // below. For the controller's own convergence test we pass
                    // the declared ε or the strictest representable band.
                    epsilon: spec.tolerance.unwrap_or(f64::MIN_POSITIVE),
                    max_steps: spec.max_steps.map(|n| n.max(1) as u32).unwrap_or(1),
                };
                Some(
                    Controller::new(mandate_spec, &band).map_err(|e| {
                        MandateRefusal::Inadmissible {
                            detail: e.to_string(),
                        }
                    })?,
                )
            }
        };

        Ok(MandateRuntime {
            name: spec.name.clone(),
            constraints,
            controller,
            epsilon: spec.tolerance,
            // Absence = strictest budget, never a loose default. One
            // generation happens anyway; what `None` must never become is
            // "unlimited" or an invented constant.
            budget: spec.max_steps.map(|n| n.max(1) as u32).unwrap_or(1),
            declared_drift: spec.drift_bound,
        })
    }

    /// The clauses the mandate steers but cannot mechanically check.
    pub fn unchecked_clauses(&self) -> &[String] {
        self.constraints.unchecked()
    }

    /// The system-prompt block that injects the mandate into the model's
    /// context. Every clause — checkable or not — steers the generation; only
    /// the checkable ones move `e`.
    pub fn directive_block(&self) -> String {
        let mut lines = vec![format!(
            "You are operating under mandate '{}'. Every requirement below is \
             mandatory; output violating any of them will be rejected and \
             regenerated, so satisfy all of them in one pass.",
            self.name
        )];
        for c in self.constraints.constraints() {
            lines.push(format!("- {}", c.source));
        }
        for u in self.constraints.unchecked() {
            lines.push(format!("- {u}"));
        }
        lines.join("\n")
    }

    /// Tier 1: the token bans for this mandate's `Absent` needles, for
    /// backends whose wire accepts a logit bias map.
    ///
    /// −100 is the OpenAI wire's ban value. Both the bare needle and its
    /// leading-space form are banned, because BPE tokenises " word" and
    /// "word" differently and the mid-sentence form is the one that actually
    /// occurs. This is deliberately NOT scaled by `u(t)`: `Absent` means
    /// absent, and a partial discouragement of a forbidden lexeme would be a
    /// cage with a published gap.
    ///
    /// Honest limits, stated: a ban suppresses these token SEQUENCES, not the
    /// concept — paraphrases and unusual casings can still express the
    /// violation, which is exactly why the loop re-validates every attempt.
    /// Tier 1 accelerates convergence; the guarantee comes from the loop.
    pub fn logit_bias_bans(&self, model: &str) -> Option<HashMap<u32, i32>> {
        let tokenizer = tokenizer_for(model)?;
        let mut bans: HashMap<u32, i32> = HashMap::new();
        for c in self.constraints.constraints() {
            if let Predicate::Absent { needle } = &c.predicate {
                for form in [needle.clone(), format!(" {needle}")] {
                    if let Ok(ids) = tokenizer.encode_ordinary(&form) {
                        for id in ids {
                            bans.insert(id, -100);
                        }
                    }
                }
            }
        }
        if bans.is_empty() {
            None
        } else {
            Some(bans)
        }
    }

    /// Whether this verdict is inside the mandate's acceptance region.
    fn accepted(&self, verdict: &Verdict) -> bool {
        match self.epsilon {
            Some(eps) => verdict.within(eps),
            // No ε declared → the strictest reading: every constraint holds.
            None => verdict.is_satisfied(),
        }
    }

    /// Drive the actuator until the constraint set accepts an output, the
    /// budget is spent, or a declared premise is observed false.
    ///
    /// `initial` is pre-existing content (the `on <target>` binding). It is
    /// evaluated for free first — an already-compliant target costs zero
    /// tokens and zero attempts, and enforcement reports `attempts: 0`.
    ///
    /// The actuator receives the [`RefinementDirective`] and returns the next
    /// attempt's full output, or an error string (backend failure), which
    /// aborts enforcement — a backend that cannot answer is not a breach, it
    /// is an infrastructure error and is reported as such.
    pub async fn enforce<F, Fut>(
        &mut self,
        initial: Option<&str>,
        mut actuator: F,
    ) -> Result<Result<MandateEnforcement, MandateBreach>, String>
    where
        F: FnMut(RefinementDirective) -> Fut,
        Fut: std::future::Future<Output = Result<String, String>>,
    {
        let mut trajectory: Vec<f64> = Vec::new();
        let mut best: Option<(f64, String)> = None;
        let mut last_verdict: Option<Verdict> = None;

        // t = 0 — the free evaluation of pre-existing content.
        if let Some(content) = initial {
            if !content.is_empty() {
                let verdict = self.constraints.evaluate(content);
                trajectory.push(verdict.error);
                if self.accepted(&verdict) {
                    return Ok(Ok(MandateEnforcement {
                        output: content.to_string(),
                        attempts: 0,
                        lyapunov_descended: true,
                        trajectory,
                        unchecked_clauses: self.constraints.unchecked().to_vec(),
                    }));
                }
                if let Some(c) = self.controller.as_mut() {
                    c.observe(verdict.error);
                }
                best = Some((verdict.error, content.to_string()));
                last_verdict = Some(verdict);
            }
        }

        for attempt in 1..=self.budget {
            let directive = RefinementDirective {
                attempt,
                feedback: last_verdict
                    .as_ref()
                    .map(|v| v.feedback())
                    .unwrap_or_default(),
                effort: None, // filled below when a controller exists
            };
            // The controller's u(t) from the PREVIOUS observation is the
            // audit signal for THIS attempt's pressure.
            let directive = RefinementDirective {
                effort: self.last_effort(),
                ..directive
            };

            let output = actuator(directive).await?;
            let verdict = self.constraints.evaluate(&output);
            let error = verdict.error;

            // ── The proof obligation, discharged or violated ────────────
            // Δe > D ⇒ drift > D (since u_eff ≥ 0 implies Δe ≤ drift): the
            // declared premise is false and the static proof is void. Raised
            // immediately — a theorem with a false hypothesis does not get
            // to keep spending the budget its conclusion promised.
            if let (Some(d), Some(prev)) = (self.declared_drift, trajectory.last().copied()) {
                let delta = error - prev;
                if delta > d + 1e-9 {
                    trajectory.push(error);
                    return Ok(Err(MandateBreach::PremiseViolated {
                        observed_drift: delta,
                        declared_bound: d,
                        at_step: attempt,
                        trajectory,
                    }));
                }
            }

            trajectory.push(error);
            if let Some(c) = self.controller.as_mut() {
                c.observe(error);
            }
            match &best {
                Some((b, _)) if *b <= error => {}
                _ => best = Some((error, output.clone())),
            }

            if self.accepted(&verdict) {
                let lyapunov = self
                    .controller
                    .as_ref()
                    .map(|c| c.lyapunov_descends())
                    .unwrap_or_else(|| trajectory.windows(2).all(|w| w[1] < w[0]));
                return Ok(Ok(MandateEnforcement {
                    output,
                    attempts: attempt,
                    trajectory,
                    lyapunov_descended: lyapunov,
                    unchecked_clauses: self.constraints.unchecked().to_vec(),
                }));
            }
            last_verdict = Some(verdict);
        }

        let (best_error, best_output) = best.unwrap_or((1.0, String::new()));
        let outstanding = last_verdict
            .map(|v| v.violated.iter().map(|x| x.obligation.clone()).collect())
            .unwrap_or_default();
        Ok(Err(MandateBreach::Unconverged {
            final_error: trajectory.last().copied().unwrap_or(1.0),
            best_error,
            best_output,
            steps_taken: self.budget,
            trajectory,
            outstanding,
        }))
    }

    fn last_effort(&self) -> Option<f64> {
        // The controller does not expose its last Control; recompute from
        // the recorded trajectory is not possible without duplicating state,
        // so the effort reported to the directive is the trajectory-driven
        // signal only when a controller exists AND has observed something.
        self.controller
            .as_ref()
            .and_then(|c| c.trajectory().last().copied())
    }
}

/// Model-prefix → axon-csys tokenizer, for the families whose wire accepts a
/// logit-bias map. Mirrors the routing in `backends::tokens` (cl100k/o200k);
/// families with no bias wire (or no exact tokenizer) return `None`, which
/// means Tier 1 silently contributes nothing and Tier 2 carries the mandate —
/// the honest degradation, since the loop is the guarantee.
fn tokenizer_for(model: &str) -> Option<&'static axon_csys::tokens::Tokenizer> {
    let m = model.to_ascii_lowercase();
    if m.starts_with("gpt-4o") || m.starts_with("o1") || m.starts_with("o3") || m.starts_with("chatgpt") {
        axon_csys::tokens::o200k_base().ok()
    } else if m.starts_with("gpt-") || m.starts_with("kimi") || m.starts_with("moonshot") || m.starts_with("glm") {
        axon_csys::tokens::cl100k_base().ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(constraint: &str) -> IRMandate {
        IRMandate {
            node_type: "mandate",
            source_line: 1,
            source_column: 1,
            name: "TestMandate".into(),
            constraint: constraint.into(),
            kp: Some(2.0),
            ki: Some(0.3),
            kd: Some(0.1),
            tolerance: Some(0.05),
            max_steps: Some(4),
            drift_bound: None,
            lipschitz: None,
            on_violation: "halt".into(),
        }
    }

    /// An actuator scripted with a fixed sequence of outputs.
    fn scripted(
        outputs: Vec<&'static str>,
    ) -> impl FnMut(RefinementDirective) -> std::future::Ready<Result<String, String>> {
        let mut queue: std::collections::VecDeque<&'static str> = outputs.into();
        move |_d| {
            std::future::ready(Ok(queue
                .pop_front()
                .expect("actuator called more times than scripted")
                .to_string()))
        }
    }

    /// v2.83.0 — Tier 1 without the C23 tokeniser.
    ///
    /// `logit_bias_bans` needs BPE ranks to name the tokens it wants
    /// banned, so a toolchain-free build cannot emit them. The design
    /// already accounts for this — `tokenizer_for` returns `None` and
    /// Tier 2's CSP feedback carries the mandate, because the LOOP is the
    /// guarantee and Tier 1 is only an accelerator. This test pins that
    /// the fallback is EMPTY rather than wrong: a build that emitted bans
    /// against invented ranks would be biasing tokens nobody chose.
    #[cfg(not(feature = "csys-native"))]
    #[test]
    fn without_the_c_kernel_tier_one_contributes_nothing_and_tier_two_still_holds() {
        let rt = MandateRuntime::from_spec(&spec(
            "must not contain \"guaranteed\"; must contain \"disclaimer\"",
        ))
        .expect("a falsifiable mandate with admissible gains");

        assert!(
            rt.logit_bias_bans("gpt-4o-mini").is_none(),
            "no tokeniser means no ranks, and the absence must be reported as `None`. An empty \
             map would read as \"nothing needed banning\" — indistinguishable from a mandate with \
             no Absent clauses, and a wire layer would send it as a real instruction."
        );
        assert!(
            super::tokenizer_for("gpt-4o-mini").is_none(),
            "the whole reason Tier 1 is empty: cl100k/o200k cannot be constructed without the \
             C23 kernel"
        );

        // Tier 2 is untouched, and it is where the guarantee lives. The
        // semantic validator is pure Rust, so the mandate still carries the
        // constraint it was written to enforce — the loop just lost one
        // accelerator, not its teeth.
        assert!(
            rt.directive_block().contains("guaranteed"),
            "the CSP feedback that carries the mandate in a toolchain-free build must still name \
             the constraint"
        );
    }

    // ── the refusals: zero tokens spent ─────────────────────────────

    #[test]
    fn an_unfalsifiable_mandate_is_refused_before_any_token() {
        let err = MandateRuntime::from_spec(&spec("output must be insightful and professional"))
            .err()
            .expect("must refuse");
        match &err {
            MandateRefusal::Unfalsifiable { clauses } => {
                assert!(!clauses.is_empty(), "the refusal names what it could not lower");
            }
            other => panic!("expected Unfalsifiable, got {other:?}"),
        }
        assert!(err.to_string().contains("cannot close holds nothing"), "{err}");
    }

    #[test]
    fn an_inadmissible_declaration_is_refused_at_runtime_too() {
        // The compiler catches this statically; the runtime must not TRUST
        // that every IR it receives went through the checker.
        let mut s = spec("must contain \"x\"");
        s.drift_bound = Some(0.8);
        s.lipschitz = Some(0.5); // ceiling 2.0; effort 2.4 ≥ 2.0 → inadmissible
        let err = MandateRuntime::from_spec(&s).err().expect("must refuse");
        assert!(matches!(err, MandateRefusal::Inadmissible { .. }), "{err:?}");
    }

    // ── the loop: converge, breach, never best-effort ───────────────

    #[tokio::test]
    async fn a_compliant_first_attempt_converges() {
        let mut rt = MandateRuntime::from_spec(&spec("must contain \"safe harbor\"")).unwrap();
        let result = rt
            .enforce(None, scripted(vec!["includes safe harbor language"]))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.attempts, 1);
        assert_eq!(result.trajectory, vec![0.0]);
    }

    #[tokio::test]
    async fn feedback_names_the_violated_obligation_on_the_second_attempt() {
        let mut rt = MandateRuntime::from_spec(&spec("must contain \"safe harbor\"")).unwrap();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen2 = seen.clone();
        let mut outputs =
            std::collections::VecDeque::from(vec!["non-compliant text", "now with safe harbor"]);
        let result = rt
            .enforce(None, move |d: RefinementDirective| {
                seen2.lock().unwrap().push(d.feedback.clone());
                std::future::ready(Ok(outputs.pop_front().unwrap().to_string()))
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.attempts, 2);
        let feedbacks = seen.lock().unwrap();
        assert_eq!(feedbacks[0], "", "first attempt has nothing to correct");
        assert!(
            feedbacks[1].contains("must contain \"safe harbor\""),
            "the CSP backtracking step injects the violated clause verbatim: {:?}",
            feedbacks[1]
        );
    }

    #[tokio::test]
    async fn a_spent_budget_is_a_breach_never_a_silent_best_effort() {
        let mut s = spec("must contain \"safe harbor\"");
        s.max_steps = Some(2);
        let mut rt = MandateRuntime::from_spec(&s).unwrap();
        let breach = rt
            .enforce(None, scripted(vec!["nope", "still nope"]))
            .await
            .unwrap()
            .err()
            .expect("must breach");
        match &breach {
            MandateBreach::Unconverged {
                steps_taken,
                final_error,
                outstanding,
                ..
            } => {
                assert_eq!(*steps_taken, 2);
                assert_eq!(*final_error, 1.0);
                assert!(outstanding[0].contains("safe harbor"));
            }
            other => panic!("expected Unconverged, got {other:?}"),
        }
        let msg = breach.describe("TestMandate");
        assert!(msg.contains("Anchor Breach"), "{msg}");
        assert!(msg.contains("non-compliance cannot be shipped"), "{msg}");
    }

    #[tokio::test]
    async fn the_breach_carries_the_best_attempt_with_its_error_for_coerce() {
        let mut s = spec("must contain \"alpha\"; must contain \"beta\"");
        s.max_steps = Some(2);
        let mut rt = MandateRuntime::from_spec(&s).unwrap();
        // Attempt 1 satisfies one of two (e=0.5); attempt 2 satisfies none.
        let breach = rt
            .enforce(None, scripted(vec!["has alpha only", "neither"]))
            .await
            .unwrap()
            .err()
            .unwrap();
        match breach {
            MandateBreach::Unconverged {
                best_error,
                best_output,
                ..
            } => {
                assert_eq!(best_error, 0.5);
                assert_eq!(best_output, "has alpha only");
            }
            other => panic!("{other:?}"),
        }
    }

    // ── the free pre-check: an already-compliant target costs nothing ──

    #[tokio::test]
    async fn a_compliant_initial_target_spends_zero_attempts() {
        let mut rt = MandateRuntime::from_spec(&spec("must not contain \"guarantee\"")).unwrap();
        let result = rt
            .enforce(
                Some("historical results only"),
                scripted(vec![]), // panics if ever called
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.attempts, 0);
        assert_eq!(result.output, "historical results only");
    }

    #[tokio::test]
    async fn a_non_compliant_initial_target_enters_the_loop_with_feedback() {
        let mut rt = MandateRuntime::from_spec(&spec("must not contain \"guaranteed\"")).unwrap();
        let result = rt
            .enforce(
                Some("returns are guaranteed"),
                scripted(vec!["returns are historical"]),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.attempts, 1);
        assert_eq!(result.trajectory, vec![1.0, 0.0], "t0 evaluation + one attempt");
    }

    // ── the proof obligation ────────────────────────────────────────

    #[tokio::test]
    async fn an_observed_drift_above_the_declared_bound_voids_the_proof_immediately() {
        let mut s = spec("must contain \"alpha\"; must contain \"beta\"; must contain \"gamma\"; must contain \"delta\"");
        s.kp = Some(0.6); // effort 1.0 ∈ (D=0.25, 1/L=2.0)
        s.ki = Some(0.3);
        s.kd = Some(0.1);
        s.drift_bound = Some(0.25);
        s.lipschitz = Some(0.5);
        s.max_steps = Some(4);
        let mut rt = MandateRuntime::from_spec(&s).unwrap();
        // e: 0.25 (3 of 4) → 0.75 (1 of 4): Δe = 0.5 > D = 0.25.
        let breach = rt
            .enforce(
                None,
                scripted(vec!["alpha beta gamma", "alpha only here"]),
            )
            .await
            .unwrap()
            .err()
            .expect("premise violated");
        match &breach {
            MandateBreach::PremiseViolated {
                observed_drift,
                declared_bound,
                at_step,
                ..
            } => {
                assert!((observed_drift - 0.5).abs() < 1e-9);
                assert_eq!(*declared_bound, 0.25);
                assert_eq!(*at_step, 2);
            }
            other => panic!("expected PremiseViolated, got {other:?}"),
        }
        assert!(
            breach.describe("M").contains("premise of the stability proof is false"),
            "the breach names WHAT was violated, not just that something was"
        );
    }

    #[tokio::test]
    async fn without_a_declared_bound_no_premise_check_fires() {
        // Nothing was statically promised, so nothing is discharged — the
        // same rise that voids a declared premise is just a bad attempt here.
        let mut s = spec("must contain \"alpha\"; must contain \"beta\"; must contain \"gamma\"; must contain \"delta\"");
        s.max_steps = Some(3);
        let mut rt = MandateRuntime::from_spec(&s).unwrap();
        let result = rt
            .enforce(
                None,
                scripted(vec![
                    "alpha beta gamma",
                    "alpha only here",
                    "alpha beta gamma delta",
                ]),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.attempts, 3);
    }

    // ── strictest-reading defaults: absence is never loose ─────────

    #[tokio::test]
    async fn no_declared_epsilon_requires_full_satisfaction() {
        let mut s = spec("must contain \"alpha\"; must contain \"beta\"");
        s.tolerance = None;
        s.max_steps = Some(1);
        let mut rt = MandateRuntime::from_spec(&s).unwrap();
        // e = 0.5 would pass any ε > 0.5; with no ε it must NOT pass.
        let breach = rt
            .enforce(None, scripted(vec!["alpha only"]))
            .await
            .unwrap()
            .err();
        assert!(breach.is_some(), "half-satisfied is not satisfied when no ε was declared");
    }

    #[tokio::test]
    async fn no_declared_budget_means_one_attempt() {
        let mut s = spec("must contain \"x\"");
        s.max_steps = None;
        let mut rt = MandateRuntime::from_spec(&s).unwrap();
        let breach = rt.enforce(None, scripted(vec!["nope"])).await.unwrap().err();
        assert!(breach.is_some(), "absence of N is one attempt, never unlimited");
    }

    // ── the directive block and Tier 1 ──────────────────────────────

    #[test]
    fn the_directive_block_carries_every_clause_checkable_or_not() {
        let rt = MandateRuntime::from_spec(&spec(
            "must contain \"safe harbor\"; output must be professional in tone",
        ))
        .unwrap();
        let block = rt.directive_block();
        assert!(block.contains("mandate 'TestMandate'"));
        assert!(block.contains("must contain \"safe harbor\""));
        assert!(
            block.contains("output must be professional in tone"),
            "unlowerable clauses still STEER the prompt: {block}"
        );
    }

    #[cfg(feature = "csys-native")]
    #[test]
    fn absent_needles_are_banned_at_minus_one_hundred_for_tokenised_families() {
        let rt = MandateRuntime::from_spec(&spec(
            "must not contain \"guaranteed\"; must contain \"disclaimer\"",
        ))
        .unwrap();
        let bans = rt.logit_bias_bans("gpt-4o-mini").expect("o200k family has a tokenizer");
        assert!(!bans.is_empty());
        assert!(
            bans.values().all(|b| *b == -100),
            "Absent means absent — a ban, not a discouragement"
        );
        // The Contains needle must NOT be banned — banning "disclaimer"
        // would make the mandate unsatisfiable by its own actuator.
        let tok = super::tokenizer_for("gpt-4o-mini").unwrap();
        let disclaimer_ids = tok.encode_ordinary("disclaimer").unwrap();
        let guaranteed_ids = tok.encode_ordinary("guaranteed").unwrap();
        assert!(guaranteed_ids.iter().all(|id| bans.contains_key(id)));
        assert!(
            !disclaimer_ids.iter().all(|id| bans.contains_key(id)),
            "a required needle's tokens must not be banned"
        );
    }

    #[test]
    fn families_without_a_bias_wire_get_no_bans_and_the_loop_carries_it() {
        let rt = MandateRuntime::from_spec(&spec("must not contain \"guaranteed\"")).unwrap();
        assert!(rt.logit_bias_bans("claude-sonnet-5").is_none());
        assert!(rt.logit_bias_bans("gemini-2.0").is_none());
    }

    // ── backend failure is infrastructure, not a breach ─────────────

    #[tokio::test]
    async fn an_actuator_error_aborts_enforcement_as_infrastructure() {
        let mut rt = MandateRuntime::from_spec(&spec("must contain \"x\"")).unwrap();
        let err = rt
            .enforce(None, |_d| {
                std::future::ready(Err("connection refused".to_string()))
            })
            .await
            .err()
            .expect("infrastructure error");
        assert!(err.contains("connection refused"));
    }
}
