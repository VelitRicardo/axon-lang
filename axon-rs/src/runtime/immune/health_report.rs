//! AXON Runtime — HealthReport (v1.1.0, paper_immune_v2.md section 5)
//!
//! Direct port of `axon/runtime/immune/health_report.py`.
//!
//! The typed output of every `immune` sensor. A HealthReport is a ΛD
//! envelope ψ = ⟨T, V, E⟩; immune is a pure sensor — it emits reports,
//! never actions. Temporal decay materialises via `c_at(now)` per section 5.3.

#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::handlers::base::{LambdaEnvelope, now_iso};

/// Epistemic lattice per paper section 5.2.
pub const VALID_LEVELS: &[&str] = &["know", "believe", "speculate", "doubt"];

/// Severity order — higher index = more severe.
pub fn level_order(level: &str) -> i32 {
    match level {
        "know" => 0,
        "believe" => 1,
        "speculate" => 2,
        "doubt" => 3,
        _ => 0,
    }
}

/// Map D_KL magnitude to the epistemic lattice per paper section 5.2.
pub fn level_from_kl(kl: f64) -> String {
    if kl < 0.3 { return "know".into(); }
    if kl < 0.6 { return "believe".into(); }
    if kl < 0.9 { return "speculate".into(); }
    "doubt".into()
}

/// Map D_KL to a certainty c ∈ [0.0, 1.0] using the paper section 5.2 bands.
pub fn certainty_from_kl(kl: f64) -> f64 {
    if kl < 0.0 || kl <= 0.3 {
        return 1.0;
    }
    if kl <= 0.6 {
        return 0.99 - ((kl - 0.3) / 0.3) * (0.99 - 0.85);
    }
    if kl <= 0.9 {
        return 0.85 - ((kl - 0.6) / 0.3) * (0.85 - 0.50);
    }
    let decayed = 0.50 - ((kl - 0.9) / 0.1).min(1.0) * 0.50;
    decayed.max(0.0)
}

/// True iff `observed` is *as severe as or worse than* `threshold`.
pub fn level_at_least(observed: &str, threshold: &str) -> bool {
    level_order(observed) >= level_order(threshold)
}

/// Immutable ΛD envelope emitted by `immune` per paper section 5.
#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub immune_name: String,
    pub kl_divergence: f64,
    pub free_energy: f64,
    pub classification: String,
    pub anomaly_signature: String,
    pub observation_window: Vec<String>,
    pub envelope: LambdaEnvelope,
    pub tau_half_life: f64,
    pub decay: String,
}

impl HealthReport {
    /// Certainty at `now`, applying paper section 5.3 decay.
    pub fn c_at(&self, now: DateTime<Utc>) -> f64 {
        if self.decay == "none" {
            return self.envelope.c;
        }
        let emitted = match DateTime::parse_from_rfc3339(&self.envelope.tau) {
            Ok(t) => t.with_timezone(&Utc),
            Err(_) => return self.envelope.c,
        };
        let elapsed = now.signed_duration_since(emitted).num_milliseconds() as f64 / 1000.0;
        if elapsed <= 0.0 {
            return self.envelope.c;
        }
        if self.decay == "linear" {
            let factor = (1.0 - elapsed / (5.0 * self.tau_half_life)).max(0.0);
            return self.envelope.c * factor;
        }
        // exponential
        let half_life = self.tau_half_life.max(1e-9);
        let factor = 0.5_f64.powf(elapsed / half_life);
        self.envelope.c * factor
    }

    /// Report is considered purged after `purge_multiple × τ` per section 5.3.
    pub fn is_active(&self, now: DateTime<Utc>, purge_multiple: f64) -> bool {
        let emitted = match DateTime::parse_from_rfc3339(&self.envelope.tau) {
            Ok(t) => t.with_timezone(&Utc),
            Err(_) => return true,
        };
        let elapsed = now.signed_duration_since(emitted).num_milliseconds() as f64 / 1000.0;
        elapsed < purge_multiple * self.tau_half_life
    }
}

/// Factory: derive epistemic level + certainty from KL per paper section 5.2.
pub fn make_health_report(
    immune_name: impl Into<String>,
    kl_divergence: f64,
    observation_window: Vec<String>,
    signature: impl Into<String>,
    tau_half_life: f64,
    decay: impl Into<String>,
    provenance: impl Into<String>,
) -> HealthReport {
    let level = level_from_kl(kl_divergence);
    let c = certainty_from_kl(kl_divergence);
    let envelope = LambdaEnvelope::new(c, now_iso(), provenance.into(), "inferred".into());
    HealthReport {
        immune_name: immune_name.into(),
        kl_divergence,
        free_energy: kl_divergence,
        classification: level,
        anomaly_signature: signature.into(),
        observation_window,
        envelope,
        tau_half_life,
        decay: decay.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_bands_match_paper_table() {
        assert_eq!(level_from_kl(0.0), "know");
        assert_eq!(level_from_kl(0.29), "know");
        assert_eq!(level_from_kl(0.45), "believe");
        assert_eq!(level_from_kl(0.75), "speculate");
        assert_eq!(level_from_kl(1.5), "doubt");
    }

    #[test]
    fn certainty_monotone_non_increasing() {
        let kls = [0.0, 0.1, 0.29, 0.3, 0.5, 0.6, 0.75, 0.9, 1.0, 2.0];
        let mut prev = f64::INFINITY;
        for k in kls {
            let c = certainty_from_kl(k);
            assert!(c <= prev + 1e-9, "c={c} at kl={k} exceeded prev={prev}");
            prev = c;
        }
    }

    #[test]
    fn level_at_least_covers_ordering() {
        assert!(level_at_least("doubt", "speculate"));
        assert!(level_at_least("speculate", "believe"));
        assert!(!level_at_least("know", "doubt"));
        assert!(level_at_least("believe", "believe"));
    }

    #[test]
    fn make_health_report_derives_level_from_kl() {
        let r = make_health_report(
            "I", 0.75, vec!["Health".into()], "sig", 300.0, "exponential", "immune:I",
        );
        assert_eq!(r.classification, "speculate");
        assert!(r.envelope.c < 1.0);
    }

    #[test]
    fn c_at_no_decay_returns_initial_c() {
        let mut r = make_health_report(
            "I", 0.0, vec![], "sig", 300.0, "none", "immune:I",
        );
        r.envelope.tau = "2026-04-20T12:00:00+00:00".into();
        let now: DateTime<Utc> = "2026-04-20T13:00:00Z".parse().unwrap();
        assert_eq!(r.c_at(now), r.envelope.c);
    }

    #[test]
    fn c_at_exponential_halves_after_tau() {
        let mut r = make_health_report(
            "I", 0.0, vec![], "sig", 60.0, "exponential", "immune:I",
        );
        r.envelope.tau = "2026-04-20T12:00:00+00:00".into();
        let initial = r.envelope.c;
        let tau_later: DateTime<Utc> = "2026-04-20T12:01:00Z".parse().unwrap();
        let c = r.c_at(tau_later);
        assert!((c - initial * 0.5).abs() < 1e-6);
    }

    #[test]
    fn is_active_purges_after_five_tau() {
        let mut r = make_health_report(
            "I", 0.0, vec![], "sig", 60.0, "exponential", "immune:I",
        );
        r.envelope.tau = "2026-04-20T12:00:00+00:00".into();
        let after_4tau: DateTime<Utc> = "2026-04-20T12:04:00Z".parse().unwrap();
        let after_6tau: DateTime<Utc> = "2026-04-20T12:06:00Z".parse().unwrap();
        assert!(r.is_active(after_4tau, 5.0));
        assert!(!r.is_active(after_6tau, 5.0));
    }
}
