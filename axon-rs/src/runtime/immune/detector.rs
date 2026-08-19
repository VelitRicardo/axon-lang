//! AXON Runtime — AnomalyDetector (v1.1.0, paper_immune_v2.md section 3 + section 4.1)
//!
//! Direct port of `axon/runtime/immune/detector.py`.
//!
//! KL + Free-Energy anomaly detector. Maintains a baseline empirical
//! distribution via a rolling window and computes `D_KL(baseline || current)`
//! per sample. Pure sensor — takes NO action.

#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};

use sha2::{Digest, Sha256};

use crate::ir_nodes::IRImmune;
use crate::runtime::lease_kernel::parse_duration;

use super::health_report::{HealthReport, make_health_report};

/// Rolling-window histogram over hashable observation values.
pub struct KLDistribution {
    pub window: usize,
    pub samples: VecDeque<String>,
}

impl KLDistribution {
    pub fn new(window: usize) -> Self {
        KLDistribution { window, samples: VecDeque::with_capacity(window) }
    }

    pub fn size(&self) -> usize { self.samples.len() }

    pub fn observe(&mut self, value: impl Into<String>) {
        if self.samples.len() == self.window {
            self.samples.pop_front();
        }
        self.samples.push_back(value.into());
    }

    pub fn observe_many<I, S>(&mut self, values: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for v in values {
            self.observe(v);
        }
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// Probabilities with Laplace smoothing.
    pub fn probabilities(&self, laplace: f64) -> HashMap<String, f64> {
        let mut counts: HashMap<String, i64> = HashMap::new();
        for v in &self.samples {
            *counts.entry(v.clone()).or_insert(0) += 1;
        }
        let n_keys = counts.len().max(1);
        let total: f64 = counts.values().copied().sum::<i64>() as f64 + laplace * n_keys as f64;
        if total == 0.0 {
            return HashMap::new();
        }
        counts
            .into_iter()
            .map(|(k, c)| (k, (c as f64 + laplace) / total))
            .collect()
    }

    /// The Laplace-smoothed probability this distribution assigns to a symbol it
    /// has **never seen**.
    ///
    /// v2.67.0 — **this replaces a bug that made the anomaly detector
    /// structurally blind.**
    ///
    /// `kl_against` used to fall back to `q.values().min()` — the *minimum observed*
    /// probability — for an unseen symbol. Consider the ideal baseline: a stable
    /// system, sampled 4 times, always reporting the same thing. Then `q` has one
    /// key with probability **1.0**, so `min = 1.0`, so a **completely novel event**
    /// is assigned probability 1.0, and
    ///
    /// ```text
    ///   KL contribution = p·ln(p/q) = 1.0 · ln(1.0 / 1.0) = 0
    /// ```
    ///
    /// > **The more consistent the baseline, the blinder the detector.** A perfectly
    /// > stable system — the *best possible* baseline — could never register an
    /// > anomaly at all. That is the exact inverse of the job.
    ///
    /// The correct fallback is the mass Laplace **reserves for unseen events**:
    /// `α / (N + α·(k+1))`, where the `+1` accounts for the new symbol entering the
    /// support. With the baseline above (N=4, k=1, α=1): `1/6 ≈ 0.167`, so a novel
    /// event yields `ln(6) ≈ 1.79` — a real, large divergence, as it must.
    ///
    /// An **empty** distribution has seen nothing, so everything is unseen: it
    /// returns 1.0, and the divergence against it is 0 rather than infinite. The
    /// supervisor never classifies against an untrained baseline anyway (v2.67.0) —
    /// this is belt to that brace.
    pub fn unseen_probability(&self, laplace: f64) -> f64 {
        let mut counts: HashMap<String, i64> = HashMap::new();
        for v in &self.samples {
            *counts.entry(v.clone()).or_insert(0) += 1;
        }
        if counts.is_empty() {
            return 1.0;
        }
        let n: f64 = counts.values().copied().sum::<i64>() as f64;
        let k = counts.len() as f64;
        let total = n + laplace * (k + 1.0);
        if total <= 0.0 {
            return 1.0;
        }
        laplace / total
    }

    /// D_KL(self || other) with Laplace smoothing on both sides.
    ///
    /// A symbol present in `self` but absent from `other` is scored against
    /// [`Self::unseen_probability`] — the mass reserved for novelty — which is the
    /// whole reason an anomaly detector exists.
    pub fn kl_against(&self, other: &KLDistribution, laplace: f64) -> f64 {
        let p = self.probabilities(laplace);
        let q = other.probabilities(laplace);
        if p.is_empty() {
            return 0.0;
        }
        let q_unseen = other.unseen_probability(laplace);
        let mut kl = 0.0;
        for (k, p_k) in &p {
            let q_k = q.get(k).copied().unwrap_or(q_unseen);
            if *p_k > 0.0 && q_k > 0.0 {
                kl += p_k * (p_k / q_k).ln();
            }
        }
        kl.max(0.0)
    }
}

fn signature_of<I, S>(values: I, n: usize) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut h = Sha256::new();
    for v in values {
        h.update(v.as_ref().as_bytes());
        h.update(b"|");
    }
    let full = format!("{:x}", h.finalize());
    full[..n.min(full.len())].to_string()
}

/// Continuous anomaly detector for one `immune` declaration.
pub struct AnomalyDetector {
    pub ir: IRImmune,
    pub baseline: KLDistribution,
    pub current: KLDistribution,
}

impl AnomalyDetector {
    pub fn new(ir: IRImmune) -> Self {
        let w = (ir.window.max(1)) as usize;
        let baseline = KLDistribution::new(w);
        let current = KLDistribution::new((w / 2).max(1));
        AnomalyDetector { ir, baseline, current }
    }

    pub fn train<I, S>(&mut self, samples: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.baseline.observe_many(samples);
    }

    /// Commit a sample and return its HealthReport.
    pub fn observe(&mut self, sample: impl Into<String>) -> HealthReport {
        self.current.observe(sample);
        self.report_for_current()
    }

    pub fn observe_many<I, S>(&mut self, samples: I) -> HealthReport
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.current.observe_many(samples);
        self.report_for_current()
    }

    /// KL divergence the detector WOULD report without mutating state.
    pub fn score(&self, sample: &str) -> f64 {
        let mut snapshot = KLDistribution::new(self.current.window);
        for v in &self.current.samples {
            snapshot.samples.push_back(v.clone());
        }
        if snapshot.samples.len() == snapshot.window {
            snapshot.samples.pop_front();
        }
        snapshot.samples.push_back(sample.into());
        snapshot.kl_against(&self.baseline, 1.0)
    }

    pub fn reset_current(&mut self) {
        self.current.clear();
    }

    /// Per-window classifier: reset → observe batch → report.
    pub fn classify_batch<I, S>(&mut self, samples: I) -> HealthReport
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.reset_current();
        self.observe_many(samples)
    }

    fn report_for_current(&self) -> HealthReport {
        let raw_kl = self.current.kl_against(&self.baseline, 1.0);
        let sens_raw = self.ir.sensitivity.unwrap_or(0.5);
        let sens = sens_raw.clamp(0.01, 1.0);
        let adjusted_kl = if sens < 1.0 {
            (raw_kl / (1.0 - sens)).min(2.0)
        } else {
            (raw_kl * 10.0).min(2.0)
        };
        // Signature over the last ≤ 8 samples, mirroring Python's [-8:] slice.
        let recent: Vec<String> = self
            .current
            .samples
            .iter()
            .rev()
            .take(8)
            .rev()
            .cloned()
            .collect();
        let sig = signature_of(&recent, 8);
        make_health_report(
            self.ir.name.clone(),
            adjusted_kl,
            self.ir.watch.clone(),
            sig,
            self.tau_seconds(),
            if self.ir.decay.is_empty() { "exponential".into() } else { self.ir.decay.clone() },
            format!("immune:{}", self.ir.name),
        )
    }

    fn tau_seconds(&self) -> f64 {
        if self.ir.tau.is_empty() {
            return 300.0;
        }
        parse_duration(&self.ir.tau).unwrap_or(300.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// v2.67.0 — **the regression guard for the bug that made this detector
    /// structurally blind.**
    ///
    /// The IDEAL baseline — a stable system that always reports the same thing — is
    /// exactly the case the old fallback got catastrophically wrong. `q.min()` was
    /// `1.0`, so a completely novel event scored `ln(1/1) = 0`: **no divergence, no
    /// anomaly, ever.** The better your baseline, the blinder the sensor.
    ///
    /// It was never caught because the kernel had **zero production instantiations**
    /// (v2.67.0 F14). Nobody ever ran it. Wiring it up (v2.67.0) exposed this on the
    /// first real tick.
    #[test]
    fn a_novel_symbol_diverges_from_a_perfectly_homogeneous_baseline() {
        let mut baseline = KLDistribution::new(8);
        baseline.observe_many(["ok:0.98", "ok:0.98", "ok:0.98", "ok:0.98"]);

        let mut current = KLDistribution::new(4);
        current.observe("ok:0.12"); // never seen before

        let kl = current.kl_against(&baseline, 1.0);
        assert!(
            kl > 1.0,
            "a completely novel symbol must diverge SHARPLY from a homogeneous baseline.              The old code returned 0.0 here — the more consistent your infrastructure, the              blinder its immune system. Got kl={kl}"
        );
        // ln(1 / (1/6)) = ln 6 ≈ 1.79
        assert!((kl - 6f64.ln()).abs() < 1e-9, "kl={kl}");
    }

    /// The other half: an unchanged world must NOT diverge. A monitor that cries
    /// wolf is as useless as one that never cries.
    #[test]
    fn an_unchanged_world_does_not_diverge() {
        let mut baseline = KLDistribution::new(8);
        baseline.observe_many(["ok:0.9", "ok:0.9", "ok:0.9", "ok:0.9"]);
        let mut current = KLDistribution::new(4);
        current.observe("ok:0.9");
        assert_eq!(current.kl_against(&baseline, 1.0), 0.0);
    }

    fn mk_ir(name: &str, sensitivity: Option<f64>, window: i64, tau: &str, decay: &str) -> IRImmune {
        IRImmune {
            node_type: "immune",
            source_line: 1,
            source_column: 1,
            name: name.into(),
            watch: vec!["Health".into()],
            sensitivity,
            baseline: "learned".into(),
            window,
            scope: "tenant".into(),
            tau: tau.into(),
            decay: decay.into(),
        }
    }

    #[test]
    fn kl_against_empty_is_zero() {
        let a = KLDistribution::new(10);
        let b = KLDistribution::new(10);
        assert_eq!(a.kl_against(&b, 1.0), 0.0);
    }

    #[test]
    fn kl_against_identical_distributions_is_near_zero() {
        let mut a = KLDistribution::new(10);
        let mut b = KLDistribution::new(10);
        a.observe_many(vec!["x", "x", "y"]);
        b.observe_many(vec!["x", "x", "y"]);
        assert!(a.kl_against(&b, 1.0).abs() < 1e-6);
    }

    #[test]
    fn kl_grows_when_distributions_diverge() {
        // Each distribution needs ≥2 keys so Laplace smoothing doesn't
        // collapse every probability to 1.0 (which would null-out the KL
        // with any q_floor).
        let mut a = KLDistribution::new(100);
        let mut b = KLDistribution::new(100);
        a.observe_many(vec!["x", "x", "x", "x", "x", "x", "x", "x", "x", "y"]); // skewed to x
        b.observe_many(vec!["y", "y", "y", "y", "y", "y", "y", "y", "y", "x"]); // skewed to y
        assert!(a.kl_against(&b, 1.0) > 0.0);
    }

    #[test]
    fn detector_train_then_observe_classifies() {
        let mut d = AnomalyDetector::new(mk_ir("I", Some(0.5), 100, "60s", "exponential"));
        d.train(vec!["ok"; 50]);
        let report = d.observe("ok");
        assert_eq!(report.immune_name, "I");
        assert_eq!(report.classification, "know"); // baseline match
    }

    #[test]
    fn detector_reports_drift_when_batch_differs() {
        let mut d = AnomalyDetector::new(mk_ir("I", Some(0.5), 100, "60s", "exponential"));
        // Baseline dominated by "ok" but with a rare "alert" seed so the
        // Laplace-smoothed distribution has two keys.
        let mut baseline: Vec<&str> = vec!["ok"; 49];
        baseline.push("alert");
        d.train(baseline);
        // Current batch dominated by "alert" (the inverse distribution).
        let mut batch: Vec<&str> = vec!["alert"; 29];
        batch.push("ok");
        let report = d.classify_batch(batch);
        assert!(
            report.kl_divergence > 0.0,
            "drifted batch should produce non-zero KL, got {:?}",
            report.kl_divergence
        );
    }

    #[test]
    fn detector_score_does_not_mutate_state() {
        let mut d = AnomalyDetector::new(mk_ir("I", Some(0.5), 100, "60s", "exponential"));
        d.train(vec!["ok"; 20]);
        let before = d.current.size();
        let _ = d.score("alert");
        assert_eq!(d.current.size(), before);
    }

    #[test]
    fn detector_reset_current_leaves_baseline_intact() {
        let mut d = AnomalyDetector::new(mk_ir("I", Some(0.5), 100, "60s", "exponential"));
        d.train(vec!["ok"; 30]);
        d.observe("alert");
        d.reset_current();
        assert_eq!(d.current.size(), 0);
        assert_eq!(d.baseline.size(), 30);
    }

    #[test]
    fn detector_falls_back_to_300s_tau_when_parse_fails() {
        let d = AnomalyDetector::new(mk_ir("I", Some(0.5), 10, "broken", "exponential"));
        assert_eq!(d.tau_seconds(), 300.0);
    }
}
