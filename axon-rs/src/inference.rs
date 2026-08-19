//! v2.42.0 — the `InferenceBackend` port + the OSS reference active-inference
//! engine (classical, honest).
//!
//! This is the control loop of a `savant`: it turns the mandate's epistemic gap
//! into a stream of self-generated actions by minimising **Expected Free Energy
//! (EFE)** over candidate policies (paper section 3). The reference here is a small,
//! exact, *classical* implementation:
//!
//!   - The belief state is a probability vector `q` over hypotheses (a classical
//!     mixed state / Bayesian model average — **NOT** quantum superposition, and
//! claiming NO computational advantage, per the paper section 3.3 revision and the
//! transversal `no_unwitnessed_advantage` law, v2.23.0). The enterprise engine
//! (v2.42.0) may mount a density-matrix representation behind this same trait,
//!     but any convergence-advantage claim there must carry a `witness`.
//!   - Perception = Bayesian belief update (the discrete analogue of minimising
//!     Variational Free Energy).
//!   - Planning = ranking policies by EFE, which decomposes exactly into an
//!     **epistemic value** (expected information gain — drives exploration) and a
//!     **pragmatic value** (expected cost against preferences — drives
//!     exploitation). This decomposition is the whole point: it resolves
//!     explore/exploit as a single arithmetic, not a hand-tuned schedule.
//!
//! Everything here is exact `f64` probability arithmetic with unit tests — no
//! magic, no unverified advantage.

/// Shannon entropy `H(p) = -Σ pᵢ ln pᵢ` (nats). Zero-probability terms contribute
/// zero (the `0·ln 0 = 0` convention).
pub fn shannon_entropy(p: &[f64]) -> f64 {
    p.iter()
        .filter(|&&x| x > 0.0)
        .map(|&x| -x * x.ln())
        .sum()
}

/// KL divergence `D(q‖p) = Σ qᵢ ln(qᵢ/pᵢ)` (nats). Terms with `qᵢ = 0` contribute
/// zero; a `pᵢ = 0` where `qᵢ > 0` yields `+∞` (an impossible-under-`p` belief).
pub fn kl_divergence(q: &[f64], p: &[f64]) -> f64 {
    q.iter()
        .zip(p.iter())
        .filter(|(&qi, _)| qi > 0.0)
        .map(|(&qi, &pi)| {
            if pi <= 0.0 {
                f64::INFINITY
            } else {
                qi * (qi / pi).ln()
            }
        })
        .sum()
}

/// Normalise a non-negative vector to sum 1. A zero vector maps to uniform.
pub fn normalize(v: &[f64]) -> Vec<f64> {
    let s: f64 = v.iter().sum();
    if s <= 0.0 {
        let u = 1.0 / v.len() as f64;
        vec![u; v.len()]
    } else {
        v.iter().map(|&x| x / s).collect()
    }
}

/// The EFE of a policy, decomposed. `total = pragmatic − epistemic` is what the
/// agent MINIMISES: it minimises expected cost while maximising information gain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Efe {
    /// Expected information gain `D(posterior‖prior)` — higher ⇒ more exploratory
    /// value (the agent *wants* this, so it enters `total` with a minus sign).
    pub epistemic_value: f64,
    /// Expected cost of the predicted outcome against preferences (cross-entropy
    /// to preferred outcomes) — lower is better.
    pub pragmatic_value: f64,
    /// `pragmatic_value − epistemic_value`. The agent selects the policy with the
    /// smallest `total`.
    pub total: f64,
}

/// A candidate policy the savant could pursue (e.g. "run this simulation",
/// "scrape that repo"), with its predicted consequences.
#[derive(Debug, Clone)]
pub struct Policy {
    pub name: String,
    /// The belief the agent predicts it would hold AFTER acting under this policy
    /// (a probability vector over hypotheses).
    pub predicted_posterior: Vec<f64>,
    /// The outcome distribution this policy predicts, over observable outcomes.
    pub predicted_outcome: Vec<f64>,
}

/// The active-inference port (charter split R1). Enterprise mounts a
/// density-matrix / QuIDD engine (v2.42.0) behind this trait, witness-gated.
pub trait InferenceBackend {
    /// Bayesian belief update: `posteriorᵢ ∝ priorᵢ · likelihoodᵢ`.
    fn update_belief(&self, prior: &[f64], likelihood: &[f64]) -> Vec<f64>;
    /// The EFE of a policy given the current `prior` and the log-preferences over
    /// outcomes (`ln C`, the agent's goal).
    fn expected_free_energy(&self, prior: &[f64], policy: &Policy, log_prefs: &[f64]) -> Efe;
    /// Index of the minimum-EFE policy (the action the agent takes next).
    /// Returns `None` for an empty policy set.
    fn select_policy(&self, prior: &[f64], policies: &[Policy], log_prefs: &[f64]) -> Option<usize>;
}

/// The OSS reference: exact classical probability arithmetic.
pub struct ReferenceInference;

impl InferenceBackend for ReferenceInference {
    fn update_belief(&self, prior: &[f64], likelihood: &[f64]) -> Vec<f64> {
        let unnorm: Vec<f64> = prior
            .iter()
            .zip(likelihood.iter())
            .map(|(&pr, &li)| pr * li)
            .collect();
        normalize(&unnorm)
    }

    fn expected_free_energy(&self, prior: &[f64], policy: &Policy, log_prefs: &[f64]) -> Efe {
        // Epistemic value: information gained = how far the predicted posterior
        // moves from the prior. Large for policies that resolve uncertainty.
        let epistemic_value = kl_divergence(&policy.predicted_posterior, prior);

        // Pragmatic value: expected cost of the predicted outcome against the
        // agent's preferences — the cross-entropy `−Σ oᵢ · ln Cᵢ`. Low when the
        // policy is predicted to land in preferred outcomes.
        let pragmatic_value: f64 = policy
            .predicted_outcome
            .iter()
            .zip(log_prefs.iter())
            .map(|(&o, &lc)| -o * lc)
            .sum();

        Efe {
            epistemic_value,
            pragmatic_value,
            total: pragmatic_value - epistemic_value,
        }
    }

    fn select_policy(&self, prior: &[f64], policies: &[Policy], log_prefs: &[f64]) -> Option<usize> {
        policies
            .iter()
            .enumerate()
            .map(|(i, p)| (i, self.expected_free_energy(prior, p, log_prefs).total))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn entropy_of_uniform_and_certain() {
        assert!(approx(shannon_entropy(&[0.5, 0.5]), 2.0_f64.ln()));
        assert!(approx(shannon_entropy(&[1.0, 0.0]), 0.0));
    }

    #[test]
    fn kl_zero_iff_equal() {
        assert!(approx(kl_divergence(&[0.5, 0.5], &[0.5, 0.5]), 0.0));
        assert!(kl_divergence(&[0.9, 0.1], &[0.5, 0.5]) > 0.0);
    }

    #[test]
    fn bayesian_update_sharpens_belief() {
        let eng = ReferenceInference;
        let prior = vec![0.5, 0.5];
        // Evidence strongly favouring hypothesis 0.
        let post = eng.update_belief(&prior, &[0.9, 0.1]);
        assert!(post[0] > prior[0]);
        assert!(approx(post.iter().sum::<f64>(), 1.0));
    }

    #[test]
    fn efe_decomposes_and_total_is_pragmatic_minus_epistemic() {
        let eng = ReferenceInference;
        let prior = vec![0.5, 0.5];
        let policy = Policy {
            name: "p".into(),
            predicted_posterior: vec![0.95, 0.05],
            predicted_outcome: vec![0.8, 0.2],
        };
        let log_prefs = vec![0.0_f64.ln().max(-10.0), (0.5_f64).ln()];
        let efe = eng.expected_free_energy(&prior, &policy, &log_prefs);
        assert!(approx(efe.total, efe.pragmatic_value - efe.epistemic_value));
        assert!(efe.epistemic_value > 0.0, "policy resolves uncertainty");
    }

    #[test]
    fn explore_when_prefs_flat_exploit_when_info_equal() {
        let eng = ReferenceInference;
        let prior = vec![0.5, 0.5];

        // A resolves uncertainty (big info gain), neutral outcome.
        let explore = Policy {
            name: "explore".into(),
            predicted_posterior: vec![0.99, 0.01],
            predicted_outcome: vec![0.5, 0.5],
        };
        // B learns nothing (posterior = prior), lands the preferred outcome.
        let exploit = Policy {
            name: "exploit".into(),
            predicted_posterior: vec![0.5, 0.5],
            predicted_outcome: vec![0.05, 0.95],
        };

        // Flat preferences ⇒ pragmatic values equal ⇒ information gain decides:
        // the explorer wins.
        let flat = vec![(0.5_f64).ln(), (0.5_f64).ln()];
        assert_eq!(
            eng.select_policy(&prior, &[explore.clone(), exploit.clone()], &flat),
            Some(0)
        );

        // Two policies with EQUAL info gain but different outcomes ⇒ the
        // preference for outcome 1 decides: the goal-seeker wins.
        let a = Policy {
            name: "a".into(),
            predicted_posterior: vec![0.7, 0.3],
            predicted_outcome: vec![0.9, 0.1],
        };
        let b = Policy {
            name: "b".into(),
            predicted_posterior: vec![0.7, 0.3],
            predicted_outcome: vec![0.1, 0.9],
        };
        let prefer_outcome1 = vec![(0.1_f64).ln(), (0.9_f64).ln()];
        assert_eq!(eng.select_policy(&prior, &[a, b], &prefer_outcome1), Some(1));
    }

    #[test]
    fn select_policy_empty_is_none() {
        let eng = ReferenceInference;
        assert_eq!(eng.select_policy(&[0.5, 0.5], &[], &[0.0, 0.0]), None);
    }
}
