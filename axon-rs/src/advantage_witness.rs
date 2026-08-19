//! v2.23.0 — The Advantage Witness: a transversal Axon law.
//!
//! Doctrine `axon://logic/no_unwitnessed_advantage`: **no primitive may claim an
//! advantage over a cheaper baseline unless it carries a machine-checkable witness
//! that the advantage is real and exceeds its cost threshold on real data.**
//! Unwitnessed sophistication degrades, honestly, to the baseline.
//!
//! This module is the runtime PROTOCOL. A primitive (quant kernel, a retrieval
//! navigator, a deliberation block) implements [`AdvantageWitness`]: it names the
//! cheaper [`Baseline`] it claims to beat, the closed-catalog [`AdvantageMetric`]
//! that measures the advantage, the cost [`threshold`](AdvantageWitness::threshold)
//! that justifies it, and a `measure(data)` over real data. The default
//! [`verdict`](AdvantageWitness::verdict) computes `holds = measure > threshold` —
//! a `false` verdict is the honest fail-closed signal (the compiler/runtime
//! recommends the baseline; `axon-W007`/`W008`).
//!
//! The metric catalog is CLOSED (the v2.5.0 extension discipline) and mirrored in the
//! frontend (`axon_frontend::type_checker::WITNESS_METRICS`, parity-pinned by
//! `tests/witness_metric_parity.rs`) so `axon check` and the runtime agree
//! on which metrics exist. v1 ships the four the field already uses; quant is the
//! first provider (v2.23.0), retrieval the second (v2.23.0).

/// The CLOSED catalog of advantage metrics — kept byte-identical to
/// `axon_frontend::type_checker::WITNESS_METRICS` (parity-pinned).
pub const WITNESS_METRICS: &[&str] = &[
    "geometric_difference",
    "kernel_target_alignment",
    "ranking_lift",
    "outcome_lift",
];

/// How advantage over a baseline is measured. A closed catalog — adding a metric
/// is a deliberate PR (the v2.5.0 closed-catalog discipline), never an open set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvantageMetric {
    /// quant kernels (v2.23.0): geometric difference `g(K_classical ‖ K_quantum)` —
    /// the report's pre-screening instrument; 0 ⇒ the kernels are interchangeable.
    GeometricDifference,
    /// quant kernels (v2.23.0): centered kernel-target alignment vs the baseline.
    KernelTargetAlignment,
    /// retrieval / navigate (v2.23.0): ranking lift over flat cosine retrieval.
    RankingLift,
    /// deliberation primitives: outcome lift over a single-shot baseline.
    OutcomeLift,
}

impl AdvantageMetric {
    /// The stable slug (the surface `metric:` keyword + the wire form).
    pub fn as_slug(self) -> &'static str {
        match self {
            AdvantageMetric::GeometricDifference => "geometric_difference",
            AdvantageMetric::KernelTargetAlignment => "kernel_target_alignment",
            AdvantageMetric::RankingLift => "ranking_lift",
            AdvantageMetric::OutcomeLift => "outcome_lift",
        }
    }

    /// Parse a slug from the closed catalog. `None` for an unknown metric (the
    /// frontend `axon-E0790` rejects those before this is ever reached).
    pub fn from_slug(s: &str) -> Option<Self> {
        match s {
            "geometric_difference" => Some(AdvantageMetric::GeometricDifference),
            "kernel_target_alignment" => Some(AdvantageMetric::KernelTargetAlignment),
            "ranking_lift" => Some(AdvantageMetric::RankingLift),
            "outcome_lift" => Some(AdvantageMetric::OutcomeLift),
            _ => None,
        }
    }
}

/// The cheaper alternative a primitive claims to beat. Open-ended `Named` carries
/// a domain-specific baseline label (e.g. `cosine`, `flat_retrieval`, `single_shot`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Baseline(pub String);

impl Baseline {
    pub fn label(&self) -> &str {
        &self.0
    }
}

/// The machine-checkable verdict — the proof object the v2.23.0 PCC `AdvantageWitnessed`
/// class carries. An independent verifier re-checks `holds == (measure > threshold)`.
#[derive(Debug, Clone, PartialEq)]
pub struct AdvantageVerdict {
    pub metric: AdvantageMetric,
    pub baseline: Baseline,
    /// The measured advantage (metric-specific units; larger = more advantage).
    pub measure: f64,
    /// The minimum advantage that justifies the cost (ε ≥ 0).
    pub threshold: f64,
    /// `measure > threshold` — the honest gate. `false` ⇒ degrade to the baseline.
    pub holds: bool,
}

impl AdvantageVerdict {
    /// A human + audit-facing one-liner.
    pub fn summary(&self) -> String {
        if self.holds {
            format!(
                "advantage WITNESSED: {} = {:.6} > threshold {:.6} over baseline '{}'",
                self.metric.as_slug(),
                self.measure,
                self.threshold,
                self.baseline.label()
            )
        } else {
            format!(
                "NO measurable advantage: {} = {:.6} ≤ threshold {:.6} — degrade to baseline '{}'",
                self.metric.as_slug(),
                self.measure,
                self.threshold,
                self.baseline.label()
            )
        }
    }
}

/// The transversal protocol. ANY primitive that adds cost over a baseline can
/// implement it; the default `verdict` enforces the law uniformly.
///
/// `D` is the primitive's real-data type (embeddings + labels for a quant kernel,
/// a query/relevance set for retrieval, …) — the witness is always evaluated on
/// REAL data (you cannot claim advantage in the abstract, the design decision).
pub trait AdvantageWitness<D> {
    /// The cheaper alternative this construct claims to beat.
    fn baseline(&self) -> Baseline;
    /// Which closed-catalog metric measures the advantage.
    fn metric(&self) -> AdvantageMetric;
    /// The minimum advantage that justifies the cost (ε ≥ 0).
    fn threshold(&self) -> f64;
    /// Measure the advantage over `data` (metric-specific; larger = more advantage).
    fn measure(&self, data: &D) -> f64;

    /// Compute the verdict — the uniform fail-closed gate. Provided; primitives
    /// only supply the four pieces above. `holds` is `measure > threshold`.
    fn verdict(&self, data: &D) -> AdvantageVerdict {
        let measure = self.measure(data);
        let threshold = self.threshold();
        AdvantageVerdict {
            metric: self.metric(),
            baseline: self.baseline(),
            measure,
            threshold,
            holds: measure > threshold,
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal witness over a toy `f64` "advantage" datum — exercises the
    /// transversal default `verdict` without any domain (proves the protocol is
    /// primitive-agnostic).
    struct ToyWitness {
        baseline: &'static str,
        metric: AdvantageMetric,
        threshold: f64,
    }
    impl AdvantageWitness<f64> for ToyWitness {
        fn baseline(&self) -> Baseline {
            Baseline(self.baseline.to_string())
        }
        fn metric(&self) -> AdvantageMetric {
            self.metric
        }
        fn threshold(&self) -> f64 {
            self.threshold
        }
        fn measure(&self, data: &f64) -> f64 {
            *data
        }
    }

    #[test]
    fn verdict_holds_when_measure_exceeds_threshold() {
        let w = ToyWitness {
            baseline: "cosine",
            metric: AdvantageMetric::GeometricDifference,
            threshold: 0.1,
        };
        let v = w.verdict(&0.42);
        assert!(v.holds);
        assert_eq!(v.measure, 0.42);
        assert_eq!(v.baseline.label(), "cosine");
        assert!(v.summary().contains("WITNESSED"));
    }

    #[test]
    fn verdict_fails_closed_when_measure_at_or_below_threshold() {
        let w = ToyWitness {
            baseline: "cosine",
            metric: AdvantageMetric::GeometricDifference,
            threshold: 0.1,
        };
        // Exactly at threshold is NOT advantage (strict >).
        assert!(!w.verdict(&0.1).holds);
        let v = w.verdict(&0.02);
        assert!(!v.holds);
        assert!(v.summary().contains("NO measurable advantage"));
        assert!(v.summary().contains("degrade to baseline 'cosine'"));
    }

    #[test]
    fn metric_slug_round_trips_for_the_closed_catalog() {
        for slug in WITNESS_METRICS {
            let m = AdvantageMetric::from_slug(slug).expect("known metric");
            assert_eq!(m.as_slug(), *slug);
        }
        assert_eq!(AdvantageMetric::from_slug("not_a_metric"), None);
    }
}
