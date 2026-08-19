//! v2.54.0 — the active-inference foveation planner.
//!
//! **The real efficiency win is fewer pixels, not cheaper pixels.**
//! Reading a whole 4K page uniformly wastes the budget. Instead the planner keeps
//! a generative belief about where the answer lives and spends the recognizer
//! (v2.54.0) only on the regions with the highest **information scent** — the
//! expected information gain about the caller's target — until the answer is
//! resolved or the declared `budget` (v2.28.0) is exhausted.
//!
//! **Pure and deterministic.** The plan is a total function of the candidate
//! regions + the query + the budget. Every foveation is recorded as a
//! [`FoveationStep`] the runtime sinks to the `ledger` (v2.12.0) — the exact,
//! **replayable** reasoning path: *why* the engine looked where it did.
//!
//! This module does **no** pixel work; it decides *where* the recognizer looks.
//! A coarse first pass supplies [`RegionCandidate`]s (cheap features — ink
//! density, position); the planner orders and bounds the expensive reads.

use crate::extraction::BBox;

/// A candidate region from a cheap coarse pass — the unit the planner reasons
/// over. `relevance` is the generative prior that this region contains the
/// target (from layout heuristics: a "total" lives bottom-right, a "date"
/// top-right, a table has high structured density). `cost` is the pixel budget a
/// deep read of it would spend.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionCandidate {
    /// A stable identifier for the trail (e.g. `"q3.top_right"`).
    pub id: String,
    pub bbox: BBox,
    /// Prior P(this region contains the target) ∈ `[0,1]`.
    pub relevance: f64,
    /// The pixel cost (megapixels ×1000, or any monotone cost unit) of a deep
    /// read — what the budget is spent in.
    pub cost: u64,
}

impl RegionCandidate {
    fn clamped_relevance(&self) -> f64 {
        if self.relevance.is_nan() {
            0.0
        } else {
            self.relevance.clamp(0.0, 1.0)
        }
    }
}

/// The budget a foveation may spend — composes with v2.28.0's linear `budget`. The
/// planner stops when EITHER the confidence target is reached OR the budget is
/// exhausted. At least one visit is always attempted (`min_visits`),
/// so a tiny budget still reads the single most promising region.
#[derive(Debug, Clone, Copy)]
pub struct FoveationBudget {
    /// Total cost the foveation may spend.
    pub max_cost: u64,
    /// Stop early once the cumulative resolved-relevance mass reaches this
    /// (the answer is probably found). `1.0` disables early stop.
    pub target_mass: f64,
    /// Always visit at least this many regions (if any exist), regardless of
    /// budget — so a degenerate budget still makes progress.
    pub min_visits: usize,
}

impl Default for FoveationBudget {
    fn default() -> Self {
        FoveationBudget { max_cost: u64::MAX, target_mass: 0.9, min_visits: 1 }
    }
}

/// The binary Shannon entropy of a probability — the uncertainty a deep read
/// would resolve. Maximal at `p = 0.5`, zero at the extremes. `nats` are fine;
/// only the ordering matters.
fn binary_entropy(p: f64) -> f64 {
    let p = p.clamp(0.0, 1.0);
    if p <= 0.0 || p >= 1.0 {
        return 0.0;
    }
    -(p * p.ln() + (1.0 - p) * (1.0 - p).ln())
}

/// The **information scent** of a region: how much reading it is expected to help
/// answer the query. A region that is both *likely relevant* and *uncertain*
/// scents strongest; a region that is certainly-irrelevant or certainly-known
/// scents zero. `scent = relevance × (H(relevance) + ε·relevance)` — the entropy
/// term drives exploration of ambiguous regions, the small linear term breaks
/// ties toward higher prior relevance (so a confidently-relevant region still
/// outranks a confidently-irrelevant one). Deterministic.
pub fn information_scent(region: &RegionCandidate) -> f64 {
    let r = region.clamped_relevance();
    r * (binary_entropy(r) + 0.1 * r)
}

/// One recorded foveation — a `ledger` trail entry. Says which region
/// was visited, in what order, its scent, and the running resolved mass.
#[derive(Debug, Clone, PartialEq)]
pub struct FoveationStep {
    pub order: usize,
    pub region_id: String,
    pub bbox: BBox,
    pub scent: f64,
    pub cost: u64,
    /// Cumulative relevance mass resolved after this step.
    pub cumulative_mass: f64,
}

/// The outcome of planning: the ordered visits (what the recognizer reads, in
/// order), the regions deliberately skipped (bounded coverage — never silently
/// dropped, so the trail is honest), and the totals.
#[derive(Debug, Clone, PartialEq)]
pub struct FoveationPlan {
    pub visits: Vec<FoveationStep>,
    /// Region ids NOT read, because the budget ran out or the target mass was
    /// reached. Surfaced so a caller (and an auditor) sees what was left unread
    /// (the design decision — no silent truncation).
    pub skipped: Vec<String>,
    pub total_cost: u64,
    pub resolved_mass: f64,
}

/// Plan a foveation over the candidate regions under a budget. Greedy by
/// information scent (the active-inference policy): read the most informative
/// region first, accumulate resolved relevance, stop when the target mass is
/// reached or the budget is exhausted — but always make `min_visits` progress.
///
/// Deterministic: candidates are ranked by `(scent desc, id asc)`, a total
/// order, so the same inputs always yield the same plan.
pub fn plan_foveation(candidates: &[RegionCandidate], budget: &FoveationBudget) -> FoveationPlan {
    let mut ranked: Vec<(f64, &RegionCandidate)> =
        candidates.iter().map(|c| (information_scent(c), c)).collect();
    // Total order: higher scent first; ties broken by id for determinism.
    ranked.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.id.cmp(&b.1.id))
    });

    let mut visits = Vec::new();
    let mut skipped = Vec::new();
    let mut spent = 0u64;
    let mut mass = 0.0f64;
    for (order, (scent, c)) in ranked.into_iter().enumerate() {
        let done_mass = mass >= budget.target_mass;
        let would_overspend = spent.saturating_add(c.cost) > budget.max_cost;
        let met_min = visits.len() >= budget.min_visits;
        // Stop reading once we've satisfied the minimum AND (target reached or
        // this read would overspend). Everything after is skipped, on the trail.
        if met_min && (done_mass || would_overspend) {
            skipped.push(c.id.clone());
            continue;
        }
        spent = spent.saturating_add(c.cost);
        mass = (mass + c.clamped_relevance()).min(1.0);
        visits.push(FoveationStep {
            order,
            region_id: c.id.clone(),
            bbox: c.bbox,
            scent,
            cost: c.cost,
            cumulative_mass: mass,
        });
    }

    FoveationPlan { visits, skipped, total_cost: spent, resolved_mass: mass }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(id: &str, relevance: f64, cost: u64) -> RegionCandidate {
        RegionCandidate {
            id: id.into(),
            bbox: BBox { x: 0.0, y: 0.0, w: 0.1, h: 0.1 },
            relevance,
            cost,
        }
    }

    #[test]
    fn scent_is_highest_for_relevant_and_uncertain() {
        // A confidently-irrelevant region scents ~0; a likely-but-uncertain one
        // scents high; a certain region has no entropy to resolve.
        let irrelevant = information_scent(&region("a", 0.02, 1));
        let uncertain = information_scent(&region("b", 0.6, 1));
        let certain = information_scent(&region("c", 1.0, 1));
        assert!(uncertain > irrelevant, "{uncertain} !> {irrelevant}");
        assert!(uncertain > certain, "a resolved region has nothing to gain");
    }

    #[test]
    fn foveation_reads_high_scent_first() {
        let regions = vec![
            region("footer", 0.05, 10),
            region("total_bottom_right", 0.7, 10),
            region("header", 0.2, 10),
        ];
        let plan = plan_foveation(&regions, &FoveationBudget::default());
        assert_eq!(plan.visits[0].region_id, "total_bottom_right", "most informative first");
    }

    #[test]
    fn budget_bounds_the_read_and_records_skips() {
        // Budget for ~2 regions of cost 10 each; the third is skipped, not
        // silently dropped.
        let regions = vec![
            region("r1", 0.5, 10),
            region("r2", 0.45, 10),
            region("r3", 0.4, 10),
        ];
        let budget = FoveationBudget { max_cost: 20, target_mass: 1.0, min_visits: 1 };
        let plan = plan_foveation(&regions, &budget);
        assert_eq!(plan.visits.len(), 2, "budget caps at two reads");
        assert_eq!(plan.skipped, vec!["r3".to_string()]);
        assert_eq!(plan.total_cost, 20);
    }

    #[test]
    fn early_stop_when_target_mass_reached() {
        // A single high-relevance region resolves the target; the rest are
        // skipped even with budget to spare (fewer pixels, the design decision).
        let regions = vec![
            region("answer", 0.95, 10),
            region("elsewhere1", 0.3, 10),
            region("elsewhere2", 0.3, 10),
        ];
        let budget = FoveationBudget { max_cost: u64::MAX, target_mass: 0.9, min_visits: 1 };
        let plan = plan_foveation(&regions, &budget);
        assert_eq!(plan.visits.len(), 1, "target reached after one read");
        assert_eq!(plan.skipped.len(), 2);
        assert!(plan.resolved_mass >= 0.9);
    }

    #[test]
    fn min_visits_makes_progress_under_a_degenerate_budget() {
        // Even a zero budget reads the single most promising region.
        let regions = vec![region("a", 0.8, 999), region("b", 0.1, 1)];
        let budget = FoveationBudget { max_cost: 0, target_mass: 0.9, min_visits: 1 };
        let plan = plan_foveation(&regions, &budget);
        assert_eq!(plan.visits.len(), 1);
        assert_eq!(plan.visits[0].region_id, "a");
    }

    #[test]
    fn plan_is_deterministic_including_ties() {
        // Equal scents → ranked by id, so the plan is reproducible.
        let regions = vec![region("z", 0.5, 1), region("a", 0.5, 1), region("m", 0.5, 1)];
        let budget = FoveationBudget { max_cost: u64::MAX, target_mass: 2.0, min_visits: 3 };
        let p1 = plan_foveation(&regions, &budget);
        let p2 = plan_foveation(&regions, &budget);
        assert_eq!(p1, p2);
        let ids: Vec<&str> = p1.visits.iter().map(|v| v.region_id.as_str()).collect();
        assert_eq!(ids, vec!["a", "m", "z"], "ties broken by id, deterministically");
    }

    #[test]
    fn trail_records_cumulative_mass_for_the_ledger() {
        let regions = vec![region("r1", 0.4, 5), region("r2", 0.3, 5)];
        let budget = FoveationBudget { max_cost: u64::MAX, target_mass: 2.0, min_visits: 2 };
        let plan = plan_foveation(&regions, &budget);
        assert!((plan.visits[0].cumulative_mass - 0.4).abs() < 1e-9);
        assert!((plan.visits[1].cumulative_mass - 0.7).abs() < 1e-9);
    }
}
