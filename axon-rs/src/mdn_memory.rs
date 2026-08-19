//! v2.12.0 — Memory-Augmented MDN (paper `paper_memory_augmented_mdn.md`).
//!
//! "Memory is not storage. It is a continuous deformation of the epistemic
//! landscape." This module implements memory as a transformation of the corpus
//! graph itself — **no embeddings, no external vector store**. A
//! memory-augmented corpus is `C* = (D, R, τ, ω, σ, H, μ)`: the MDN corpus plus a
//! history `H` of past interactions and an update operator `μ : (C, H) → C'`.
//!
//! `μ` is deliberately restricted to the **geometry** (`ω`, `σ`) — never the
//! **topology** (`D`, `R`, `τ`) (paper Def 3) — which buys convergence,
//! reversibility, and functoriality. Three orthogonal memory types decompose it:
//!
//! - **Episodic** (Def 5): stored trajectories `Π`; recall by Jaccard node-set
//!   similarity — no embeddings.
//! - **Semantic** (Def 6): edge-weight learning `ω'(r) = clamp(ω(r) + Δ(r|H),
//!   ε, 1)` with `Δ(r|H) = η · Σ_{o: r∈Edges(πₒ)} (sₒ − s̄) · γ^(now−tₒ)`. Edges
//!   on above-average-outcome paths are reinforced; below-average ones weakened.
//! - **Procedural** (Def 7): a navigation `Bias(D')` — Kivi's
//!   `ProceduralBiasVector` — accumulated from D's frequency in successful paths.
//!
//! Faithful to the paper's guarantees (verified by the property tests below):
//! **locality** (Def 4 — only traversed edges change), **empty history is
//! identity** (Prop 2 — `μ(C, ∅) = C`), **convergence** under bounded updates
//! (Theorem 2 — `Σ|Δ_t| ≤ η/(1−γ) < ∞` ⇒ weights are Cauchy in `[ε, 1]`), and
//! **geometry-not-topology** (the document/edge sets are preserved).

use crate::mdn::{Corpus, DocId, Edge};
use std::collections::{HashMap, HashSet};

/// An interaction outcome `oᵢ = (qᵢ, πᵢ, sᵢ, tᵢ)` (paper Def 2). The path `πᵢ`
/// is stored as its document sequence; `Edges(πᵢ)` are the consecutive pairs.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub query: String,
    pub path: Vec<DocId>,
    /// Outcome quality `sₒ ∈ [0, 1]`.
    pub score: f64,
    /// Interaction timestamp `tₒ` (a logical clock; smaller = older).
    pub timestamp: u64,
}

impl Outcome {
    fn node_set(&self) -> HashSet<DocId> {
        self.path.iter().copied().collect()
    }
    fn traverses(&self, from: DocId, to: DocId) -> bool {
        self.path.windows(2).any(|w| w[0] == from && w[1] == to)
    }
}

/// The history structure `H = (Q, Π, O)` (paper Def 2). Episodic memory is
/// write-once: outcomes are appended, never modified.
#[derive(Debug, Clone, Default)]
pub struct History {
    pub outcomes: Vec<Outcome>,
}

impl History {
    pub fn new() -> Self {
        History { outcomes: Vec::new() }
    }

    /// Episodic `record` (Def 5) — append a trajectory.
    pub fn record(&mut self, outcome: Outcome) {
        self.outcomes.push(outcome);
    }

    pub fn is_empty(&self) -> bool {
        self.outcomes.is_empty()
    }

    /// The baseline `s̄` — the running mean of all outcome scores.
    pub fn mean_score(&self) -> f64 {
        if self.outcomes.is_empty() {
            return 0.0;
        }
        self.outcomes.iter().map(|o| o.score).sum::<f64>() / self.outcomes.len() as f64
    }

    /// `Edges(Π)` — the set of edges (consecutive doc pairs) ever traversed.
    pub fn traversed_edges(&self) -> HashSet<(DocId, DocId)> {
        let mut s = HashSet::new();
        for o in &self.outcomes {
            for w in o.path.windows(2) {
                s.insert((w[0], w[1]));
            }
        }
        s
    }
}

/// Parameters of the memory update (paper Def 6).
#[derive(Debug, Clone)]
pub struct MemoryParams {
    /// Learning rate `η ∈ (0, 1)`.
    pub eta: f64,
    /// Temporal decay `γ ∈ (0, 1)` — `decay(t) = γ^(now−t)`.
    pub gamma: f64,
    /// The current logical time, for `decay`.
    pub now: u64,
    /// Minimum edge weight `ε > 0` — prevents weight collapse to 0 (which would
    /// delete the edge, violating geometry-not-topology).
    pub epsilon: f64,
}

impl Default for MemoryParams {
    fn default() -> Self {
        MemoryParams { eta: 0.1, gamma: 0.95, now: 0, epsilon: 0.001 }
    }
}

fn decay(params: &MemoryParams, t: u64) -> f64 {
    let dt = params.now.saturating_sub(t) as f64;
    params.gamma.powf(dt)
}

/// The semantic learning signal `Δ(r | H)` for one edge (paper Def 6):
/// `η · Σ_{o: r∈Edges(πₒ)} (sₒ − s̄) · γ^(now−tₒ)`.
pub fn semantic_delta(edge: &Edge, history: &History, params: &MemoryParams) -> f64 {
    let s_bar = history.mean_score();
    params.eta
        * history
            .outcomes
            .iter()
            .filter(|o| o.traverses(edge.from, edge.to))
            .map(|o| (o.score - s_bar) * decay(params, o.timestamp))
            .sum::<f64>()
}

/// The memory update operator `μ : (C, H) → C'` (paper Def 3) — the **semantic**
/// channel. Returns a NEW corpus whose edge weights are
/// `ω'(r) = clamp(ω(r) + Δ(r|H), ε, 1)`, **only for traversed edges** (locality,
/// Def 4); the document set, edge set, and types are untouched (geometry, not
/// topology). This is the endofunctor `Mem : Corp → Corp` on objects.
pub fn apply_memory(corpus: &Corpus, history: &History, params: &MemoryParams) -> Corpus {
    let traversed = history.traversed_edges();
    let new_edges: Vec<Edge> = corpus
        .edges()
        .iter()
        .map(|e| {
            let mut weight = e.weight;
            if traversed.contains(&(e.from, e.to)) {
                let delta = semantic_delta(e, history, params);
                weight = (e.weight + delta).clamp(params.epsilon, 1.0);
            }
            Edge { from: e.from, to: e.to, etype: e.etype, weight }
        })
        .collect();
    let docs = corpus.documents().into_iter().cloned().collect();
    // clamp keeps ω ∈ [ε, 1] ⊆ (0, 1] ⇒ G4 holds; topology unchanged ⇒ G2 holds.
    Corpus::new(docs, new_edges).expect("memory preserves the corpus invariants")
}

/// Procedural memory `Bias(D')` (paper Def 7) — Kivi's `ProceduralBiasVector`.
/// `Bias(D') = Σ_{o: D'∈Nodes(πₒ)} sₒ · γ^(now−tₒ) / Z`, normalized over the
/// documents seen in history. Documents frequent in high-scoring interactions
/// accumulate higher bias.
pub fn procedural_bias(history: &History, params: &MemoryParams) -> HashMap<DocId, f64> {
    let mut raw: HashMap<DocId, f64> = HashMap::new();
    for o in &history.outcomes {
        let contribution = o.score * decay(params, o.timestamp);
        for d in o.node_set() {
            *raw.entry(d).or_insert(0.0) += contribution;
        }
    }
    let z: f64 = raw.values().sum();
    if z > 0.0 {
        for v in raw.values_mut() {
            *v /= z;
        }
    }
    raw
}

/// Episodic recall (paper Def 5) — past trajectories *structurally* similar to a
/// reference path, by **Jaccard on node sets** (no embeddings):
/// `similarity(π₁, π₂) = |Nodes(π₁) ∩ Nodes(π₂)| / |Nodes(π₁) ∪ Nodes(π₂)|`.
/// Returns the outcomes whose path scores `≥ threshold` against `reference`.
pub fn recall_similar<'a>(
    history: &'a History,
    reference: &[DocId],
    threshold: f64,
) -> Vec<&'a Outcome> {
    let ref_set: HashSet<DocId> = reference.iter().copied().collect();
    history
        .outcomes
        .iter()
        .filter(|o| path_jaccard(&ref_set, &o.node_set()) >= threshold)
        .collect()
}

/// Jaccard similarity of two document sets (paper Def 5).
pub fn path_jaccard(a: &HashSet<DocId>, b: &HashSet<DocId>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let inter = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        inter / union
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdn::{epistemic_pagerank, Document, EdgeType, EprParams};

    fn doc(id: DocId) -> Document {
        Document { id, title: format!("D{id}"), depth: id, recency: 0.5, epistemic: "believe".into() }
    }
    fn edge(from: DocId, to: DocId, w: f64) -> Edge {
        Edge { from, to, etype: EdgeType::Cite, weight: w }
    }
    fn outcome(path: &[DocId], score: f64, t: u64) -> Outcome {
        Outcome { query: "q".into(), path: path.to_vec(), score, timestamp: t }
    }

    /// 1→2, 1→3, 2→4 ; weights all 0.5.
    fn corpus() -> Corpus {
        Corpus::new(
            vec![doc(1), doc(2), doc(3), doc(4)],
            vec![edge(1, 2, 0.5), edge(1, 3, 0.5), edge(2, 4, 0.5)],
        )
        .unwrap()
    }

    fn weight_of(c: &Corpus, from: DocId, to: DocId) -> f64 {
        c.edges().iter().find(|e| e.from == from && e.to == to).unwrap().weight
    }

    // ── Empty history is identity (Prop 2) ───────────────────────────────────

    #[test]
    fn empty_history_is_the_identity() {
        let c = corpus();
        let updated = apply_memory(&c, &History::new(), &MemoryParams::default());
        for e in c.edges() {
            assert!((weight_of(&updated, e.from, e.to) - e.weight).abs() < 1e-12);
        }
    }

    // ── Locality (Def 4) ─────────────────────────────────────────────────────

    #[test]
    fn only_traversed_edges_change() {
        let c = corpus();
        let mut h = History::new();
        // A high path 1→2→4 and a low path 1→2 give a baseline s̄=0.5 with a
        // net signal on 2→4 (only in the high path). Neither touches edge 1→3.
        h.record(outcome(&[1, 2, 4], 1.0, 0));
        h.record(outcome(&[1, 2], 0.0, 0));
        let updated = apply_memory(&c, &h, &MemoryParams::default());
        // Untraversed 1→3 is frozen (locality, Def 4).
        assert!((weight_of(&updated, 1, 3) - 0.5).abs() < 1e-12, "untraversed edge frozen");
        // Traversed 2→4 (only in the above-average path) is reinforced.
        assert!(weight_of(&updated, 2, 4) > 0.5, "traversed above-average edge moved");
    }

    // ── Reinforcement direction (Def 6) ──────────────────────────────────────

    #[test]
    fn above_average_paths_reinforce_below_average_weaken() {
        let c = corpus();
        let mut h = History::new();
        // 1→2 on a high-score path; 1→3 on a low-score path. s̄ = 0.5.
        h.record(outcome(&[1, 2], 1.0, 0));
        h.record(outcome(&[1, 3], 0.0, 0));
        let updated = apply_memory(&c, &h, &MemoryParams::default());
        assert!(weight_of(&updated, 1, 2) > 0.5, "above-average edge reinforced");
        assert!(weight_of(&updated, 1, 3) < 0.5, "below-average edge weakened");
    }

    // ── Weight clamping / convergence (Theorem 2) ────────────────────────────

    #[test]
    fn weights_stay_clamped_and_bounded_updates_converge() {
        let c = corpus();
        let params = MemoryParams { eta: 0.1, gamma: 0.95, now: 50, epsilon: 0.001 };
        // 1→2 consistently on high-score paths, 1→3 on low ones ⇒ s̄=0.5 and a
        // persistent positive signal on 1→2 (it beats the baseline every time).
        let mut h = History::new();
        for t in 0..50 {
            h.record(outcome(&[1, 2], 1.0, t));
            h.record(outcome(&[1, 3], 0.0, t));
        }
        // Repeatedly apply memory to the updated corpus; the weight must stay in
        // [ε, 1] and converge (consecutive applications agree).
        let mut cur = c.clone();
        for _ in 0..30 {
            cur = apply_memory(&cur, &h, &params);
            let w = weight_of(&cur, 1, 2);
            assert!(w >= params.epsilon && w <= 1.0, "weight stays in [ε,1]: {w}");
        }
        let a = weight_of(&cur, 1, 2);
        let next = apply_memory(&cur, &h, &params);
        let b = weight_of(&next, 1, 2);
        assert!((a - b).abs() < 1e-9, "converged — consecutive applications agree");
        assert!((a - 1.0).abs() < 1e-6, "saturates at the upper clamp under persistent reinforcement");
    }

    #[test]
    fn single_step_update_magnitude_is_bounded_by_eta_over_one_minus_gamma() {
        // Σ_t γ^t · |s−s̄| ≤ 1/(1−γ) ⇒ |Δ| ≤ η/(1−γ) (paper Theorem 2 NOTE).
        let params = MemoryParams { eta: 0.1, gamma: 0.95, now: 50, epsilon: 0.001 };
        let mut h = History::new();
        for t in 0..50 {
            h.record(outcome(&[1, 2], 1.0, t));
        }
        let e = edge(1, 2, 0.5);
        let delta = semantic_delta(&e, &h, &params);
        let bound = params.eta / (1.0 - params.gamma);
        assert!(delta.abs() <= bound + 1e-9, "|Δ|={} ≤ η/(1−γ)={}", delta.abs(), bound);
    }

    // ── Procedural bias (Def 7) — Kivi's ProceduralBiasVector ─────────────────

    #[test]
    fn procedural_bias_favors_documents_in_high_scoring_paths() {
        let mut h = History::new();
        h.record(outcome(&[1, 2], 1.0, 0)); // D1,D2 in a winning path
        h.record(outcome(&[3], 0.1, 0)); // D3 in a poor path
        let bias = procedural_bias(&h, &MemoryParams::default());
        assert!(bias[&2] > bias[&3], "doc in the winning path has higher bias");
        // Normalized to a distribution.
        let total: f64 = bias.values().sum();
        assert!((total - 1.0).abs() < 1e-9, "bias is normalized");
    }

    // ── Episodic recall (Def 5) ──────────────────────────────────────────────

    #[test]
    fn jaccard_and_episodic_recall() {
        let a: HashSet<DocId> = [1, 2, 3].into_iter().collect();
        let b: HashSet<DocId> = [2, 3, 4].into_iter().collect();
        assert!((path_jaccard(&a, &a) - 1.0).abs() < 1e-12, "identical → 1");
        assert!((path_jaccard(&a, &b) - 0.5).abs() < 1e-12, "|∩|=2,|∪|=4 → 0.5");
        let disjoint: HashSet<DocId> = [9].into_iter().collect();
        assert!(path_jaccard(&a, &disjoint) < 1e-12, "disjoint → 0");

        let mut h = History::new();
        h.record(outcome(&[1, 2, 3], 0.9, 0)); // similar to [1,2,4]
        h.record(outcome(&[7, 8], 0.9, 0)); // dissimilar
        let hits = recall_similar(&h, &[1, 2, 4], 0.4);
        assert_eq!(hits.len(), 1, "only the structurally similar trajectory recalled");
        assert_eq!(hits[0].path, vec![1, 2, 3]);
    }

    // ── Strict generalization (Theorem 3): memory shifts EPR ─────────────────

    #[test]
    fn memory_reshapes_epistemic_pagerank() {
        // After memory reinforces 1→2 (a winning path), D2's trust EPR rises vs
        // the static corpus — memory-augmented MDN strictly generalizes static.
        let c = corpus();
        let base = epistemic_pagerank(&c, &EprParams::default());
        let mut h = History::new();
        // 1→2 wins, 1→3 loses ⇒ s̄=0.5 and 1→2 is reinforced.
        for t in 0..5 {
            h.record(outcome(&[1, 2], 1.0, t));
            h.record(outcome(&[1, 3], 0.0, t));
        }
        let cstar = apply_memory(&c, &h, &MemoryParams::default());
        let after = epistemic_pagerank(&cstar, &EprParams::default());
        assert!(
            after.epr_plus[&2] > base.epr_plus[&2],
            "memory raised D2's trust EPR ({} → {})",
            base.epr_plus[&2],
            after.epr_plus[&2]
        );
    }
}
