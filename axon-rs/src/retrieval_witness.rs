//! v2.23.0 — the SECOND Advantage-Witness instance: retrieval / navigate.
//!
//! This is the transversality proof. The v2.23.0 `AdvantageWitness` protocol is NOT
//! quant-shaped: the *same* trait, the *same* fail-closed verdict, a *different*
//! metric. A sophisticated retrieval (e.g. the v2.12.0 MDN signed-EPR navigator) claims
//! to beat **flat cosine retrieval**; the witness measures the **`ranking_lift`** —
//! does the sophisticated ranking surface relevant items higher than the baseline,
//! on the adopter's REAL labelled queries? If not, it degrades, honestly, to flat
//! retrieval (`axon-W008`).
//!
//! The metric here is **mean reciprocal-rank lift**: per query, `1/rank` of the
//! first relevant item under each ranking; the advantage is the mean difference
//! (candidate − baseline). `≤ threshold` ⇒ no measurable lift ⇒ use flat retrieval.

use crate::advantage_witness::{AdvantageMetric, AdvantageWitness, Baseline};

/// One query's candidates: `(candidate_score, baseline_score, is_relevant)` per
/// item. The candidate score is the sophisticated ranker's; the baseline score is
/// flat cosine retrieval's. Labels (`is_relevant`) come from the adopter's data.
#[derive(Debug, Clone)]
pub struct RankingQuery {
    pub items: Vec<RankedItem>,
}

#[derive(Debug, Clone, Copy)]
pub struct RankedItem {
    pub candidate_score: f64,
    pub baseline_score: f64,
    pub relevant: bool,
}

/// Reciprocal rank of the first relevant item when the candidates are sorted by
/// `score` (descending). `0.0` when no relevant item exists. Ties keep input order
/// (deterministic — no clock/RNG).
fn reciprocal_rank(items: &[RankedItem], score: impl Fn(&RankedItem) -> f64) -> f64 {
    let mut idx: Vec<usize> = (0..items.len()).collect();
    // Stable sort by score desc (partial_cmp is fine — scores are finite).
    idx.sort_by(|&a, &b| {
        score(&items[b])
            .partial_cmp(&score(&items[a]))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (rank0, &i) in idx.iter().enumerate() {
        if items[i].relevant {
            return 1.0 / (rank0 as f64 + 1.0);
        }
    }
    0.0
}

/// A witness that a sophisticated retrieval beats flat cosine retrieval.
pub struct RankingLiftWitness {
    pub threshold: f64,
}

impl RankingLiftWitness {
    pub fn new(threshold: f64) -> Self {
        Self { threshold }
    }
}

impl AdvantageWitness<Vec<RankingQuery>> for RankingLiftWitness {
    fn baseline(&self) -> Baseline {
        Baseline("flat_retrieval".to_string())
    }
    fn metric(&self) -> AdvantageMetric {
        AdvantageMetric::RankingLift
    }
    fn threshold(&self) -> f64 {
        self.threshold
    }
    /// Mean reciprocal-rank lift of the candidate ranking over the baseline.
    fn measure(&self, data: &Vec<RankingQuery>) -> f64 {
        if data.is_empty() {
            return 0.0;
        }
        let total: f64 = data
            .iter()
            .map(|q| {
                let cand = reciprocal_rank(&q.items, |it| it.candidate_score);
                let base = reciprocal_rank(&q.items, |it| it.baseline_score);
                cand - base
            })
            .sum();
        total / data.len() as f64
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn item(c: f64, b: f64, rel: bool) -> RankedItem {
        RankedItem { candidate_score: c, baseline_score: b, relevant: rel }
    }

    #[test]
    fn a_retrieval_identical_to_cosine_fails_closed_no_lift() {
        // The transversality test: a non-quant primitive, the SAME protocol. A
        // candidate ranking equal to the baseline has zero lift → degrade to flat.
        let data = vec![RankingQuery {
            items: vec![item(0.9, 0.9, false), item(0.5, 0.5, true), item(0.1, 0.1, false)],
        }];
        let w = RankingLiftWitness::new(0.01);
        let v = w.verdict(&data);
        assert_eq!(v.measure, 0.0);
        assert!(!v.holds, "no lift over cosine ⇒ the witness must FAIL");
        assert_eq!(v.baseline.label(), "flat_retrieval");
        assert_eq!(v.metric.as_slug(), "ranking_lift");
    }

    #[test]
    fn a_retrieval_that_ranks_relevant_items_higher_is_witnessed() {
        // Candidate puts the relevant item first (RR=1); baseline puts it third
        // (RR=1/3). Lift = 2/3 > threshold ⇒ holds.
        let data = vec![RankingQuery {
            items: vec![
                item(0.9, 0.2, true),  // relevant: candidate ranks #1, baseline #3
                item(0.5, 0.9, false),
                item(0.1, 0.5, false),
            ],
        }];
        let w = RankingLiftWitness::new(0.05);
        let v = w.verdict(&data);
        assert!((v.measure - (1.0 - 1.0 / 3.0)).abs() < 1e-9, "lift={}", v.measure);
        assert!(v.holds, "a genuine ranking lift must be WITNESSED");
        assert!(v.summary().contains("WITNESSED"));
    }

    #[test]
    fn lift_below_threshold_still_fails_closed() {
        // A tiny lift that does not clear the cost threshold degrades to baseline.
        let data = vec![
            RankingQuery { items: vec![item(0.6, 0.5, true), item(0.5, 0.6, false)] },
            RankingQuery { items: vec![item(0.5, 0.5, false), item(0.4, 0.4, true)] },
        ];
        // Query 1: candidate RR=1, baseline RR=1/2 → +0.5; query 2: 0. Mean=0.25.
        let w = RankingLiftWitness::new(0.5); // demand a big lift
        assert!(!w.verdict(&data).holds, "0.25 lift ≤ 0.5 threshold ⇒ fail closed");
        let w2 = RankingLiftWitness::new(0.1);
        assert!(w2.verdict(&data).holds, "0.25 lift > 0.1 threshold ⇒ witnessed");
    }
}
