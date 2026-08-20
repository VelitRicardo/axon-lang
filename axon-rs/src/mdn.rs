//! v2.12.0 — Multi-Document Navigation (MDN).
//!
//! A faithful implementation of `papers/paper_multi_document.md`: a document
//! corpus as a **labeled directed graph** `C = (D, R, τ, ω, σ)`, navigated by
//! relationship rather than by embedding similarity. This module ships **v2.12.0
//! — the corpus graph + signed Epistemic PageRank (EPR)**, the paper's most
//! distinctive result.
//!
//! # Signed Epistemic PageRank (paper section 2.3, Def 4.2, Theorem 3)
//!
//! The corpus is decomposed by edge polarity into a *trust* subgraph `G⁺`
//! (`cite`, `elaborate`, `corroborate`) and a *distrust* subgraph `G⁻`
//! (`contradict`, `supersede`). On each we compute a PageRank-style stationary
//! distribution by power iteration
//!
//! ```text
//! EPR⁺ = (1-d)·u⁺ + d·(P⁺)ᵀ·EPR⁺      (trust propagation)
//! EPR⁻ = (1-d)·u⁻ + d·(P⁻)ᵀ·EPR⁻      (distrust propagation)
//! ```
//!
//! with row-stochastic `P⁺/P⁻` (dangling rows replaced by uniform, paper P4) and
//! **asymmetric** teleportation (`u⁺ ∝ 1/depth` — primary sources; `u⁻ ∝
//! recency` — corrections arrive late). The net epistemic reputation is
//!
//! ```text
//! EPR(Dᵢ) = EPR⁺(Dᵢ) - λ·EPR⁻(Dᵢ)
//! ```
//!
//! By Perron–Frobenius (Theorem 3) each of `EPR⁺`, `EPR⁻` exists, is unique and
//! strictly positive, and the power iteration converges geometrically. Unlike a
//! standard PageRank, `EPR` is a **signed reputation** — it may be negative for a
//! document more contested than endorsed (paper section 2.3 WARNING), and it sums to
//! `1 - λ`, not 1. The property tests below verify each of these.
//!
//! Embeddings-free throughout (program invariant #1).

use std::collections::{HashMap, HashSet};

/// Stable identifier of a document within a [`Corpus`].
pub type DocId = u32;

/// The relationship a directed edge encodes (paper Def 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeType {
    Cite,
    Elaborate,
    Corroborate,
    Depend,
    Implement,
    Exemplify,
    Contradict,
    Supersede,
}

/// The epistemic polarity of an edge type — which signed subgraph it joins
/// (paper Def 4). `Neutral` edges (`depend`/`implement`/`exemplify`) are
/// structural but propagate neither trust nor distrust, so they sit in neither
/// `G⁺` nor `G⁻`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    Positive,
    Negative,
    Neutral,
}

impl EdgeType {
    /// The signed subgraph membership of this edge type (paper Def 4).
    pub fn polarity(self) -> Polarity {
        match self {
            EdgeType::Cite | EdgeType::Elaborate | EdgeType::Corroborate => Polarity::Positive,
            EdgeType::Contradict | EdgeType::Supersede => Polarity::Negative,
            EdgeType::Depend | EdgeType::Implement | EdgeType::Exemplify => Polarity::Neutral,
        }
    }

    /// v2.14.0 — the relation-type slug (the inverse of [`EdgeType::from_slug`]).
    /// Used by the adaptive write-back to address the edge's row in the store by
    /// its `etype` column value.
    pub fn slug(self) -> &'static str {
        match self {
            EdgeType::Cite => "cite",
            EdgeType::Elaborate => "elaborate",
            EdgeType::Corroborate => "corroborate",
            EdgeType::Depend => "depend",
            EdgeType::Implement => "implement",
            EdgeType::Exemplify => "exemplify",
            EdgeType::Contradict => "contradict",
            EdgeType::Supersede => "supersede",
        }
    }

    /// v2.13.0 — parse a relation-type slug (the closed catalog the frontend
    /// `corpus` grammar uses) into an [`EdgeType`]. `None` for an unknown slug.
    pub fn from_slug(s: &str) -> Option<EdgeType> {
        Some(match s {
            "cite" => EdgeType::Cite,
            "elaborate" => EdgeType::Elaborate,
            "corroborate" => EdgeType::Corroborate,
            "depend" => EdgeType::Depend,
            "implement" => EdgeType::Implement,
            "exemplify" => EdgeType::Exemplify,
            "contradict" => EdgeType::Contradict,
            "supersede" => EdgeType::Supersede,
            _ => return None,
        })
    }
}

/// A labeled, weighted directed edge `(Dᵢ, Dⱼ, τ)` with weight `ω ∈ (0, 1]`.
#[derive(Debug, Clone)]
pub struct Edge {
    pub from: DocId,
    pub to: DocId,
    pub etype: EdgeType,
    /// `ω(r) ∈ (0, 1]` — relationship strength (paper Def 3, G4).
    pub weight: f64,
}

/// A corpus document `Dᵢ` with the structural priors the asymmetric
/// teleportation needs.
#[derive(Debug, Clone)]
pub struct Document {
    pub id: DocId,
    pub title: String,
    /// Dependency depth from the corpus roots — primary sources have small
    /// depth. Drives the trust teleportation prior `u⁺ ∝ 1/(depth+1)`.
    pub depth: u32,
    /// Recency in `[0, 1]` (1 = newest). Drives the distrust teleportation prior
    /// `u⁻ ∝ recency` (corrections/errata arrive late).
    pub recency: f64,
    /// The document's epistemic status `σ(Dᵢ)` (lattice level slug).
    pub epistemic: String,
}

/// A document corpus graph `C = (D, R, τ, ω, σ)` (paper Def 1).
#[derive(Debug, Clone)]
pub struct Corpus {
    docs: HashMap<DocId, Document>,
    edges: Vec<Edge>,
}

/// Why a node/edge set fails to form a valid corpus (paper G2/G4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorpusError {
    /// An edge references a document not in `D` (G2).
    UnknownEndpoint(DocId),
    /// An edge weight is outside `(0, 1]` (G4).
    BadWeight(DocId, DocId),
    /// The corpus has no documents.
    Empty,
    /// v2.13.0 — a `relations:` entry named an unknown edge type (defensive;
    /// the frontend type-checker already rejects these at compile time).
    UnknownRelationType(String),
    /// v2.13.0 — a `relations:` entry referenced an undeclared document.
    UnknownDocumentRef(String),
}

impl Corpus {
    /// Build + validate a corpus: every edge connects corpus members (G2) and
    /// carries a weight `ω ∈ (0, 1]` (G4).
    pub fn new(docs: Vec<Document>, edges: Vec<Edge>) -> Result<Self, CorpusError> {
        if docs.is_empty() {
            return Err(CorpusError::Empty);
        }
        let map: HashMap<DocId, Document> = docs.into_iter().map(|d| (d.id, d)).collect();
        for e in &edges {
            if !map.contains_key(&e.from) {
                return Err(CorpusError::UnknownEndpoint(e.from));
            }
            if !map.contains_key(&e.to) {
                return Err(CorpusError::UnknownEndpoint(e.to));
            }
            if !(e.weight > 0.0 && e.weight <= 1.0) {
                return Err(CorpusError::BadWeight(e.from, e.to));
            }
        }
        Ok(Corpus { docs: map, edges })
    }

    /// v2.13.0 — build an MDN corpus graph from a frontend `corpus`
    /// declaration: the document names (mapped to ids by index) and the typed
    /// weighted `relations:` edges `(etype_slug, from, to, weight)`. The relation
    /// type slug resolves via [`EdgeType::from_slug`]; `from`/`to` resolve against
    /// the declared documents. Returns `Err` on an unknown type or an undeclared
    /// document reference (the frontend type-checker already rejects both at
    /// compile time — this is the runtime mirror).
    pub fn from_declaration(
        documents: &[String],
        relations: &[(String, String, String, f64)],
    ) -> Result<Corpus, CorpusError> {
        if documents.is_empty() {
            return Err(CorpusError::Empty);
        }
        let index: HashMap<&str, DocId> = documents
            .iter()
            .enumerate()
            .map(|(i, d)| (d.as_str(), i as DocId))
            .collect();
        let docs: Vec<Document> = documents
            .iter()
            .enumerate()
            .map(|(i, name)| Document {
                id: i as DocId,
                title: name.clone(),
                depth: 0,
                recency: 0.5,
                epistemic: "believe".to_string(),
            })
            .collect();
        let mut edges = Vec::with_capacity(relations.len());
        for (etype, from, to, weight) in relations {
            let et = EdgeType::from_slug(etype)
                .ok_or_else(|| CorpusError::UnknownRelationType(etype.clone()))?;
            let f = *index
                .get(from.as_str())
                .ok_or_else(|| CorpusError::UnknownDocumentRef(from.clone()))?;
            let t = *index
                .get(to.as_str())
                .ok_or_else(|| CorpusError::UnknownDocumentRef(to.clone()))?;
            edges.push(Edge { from: f, to: t, etype: et, weight: *weight });
        }
        Corpus::new(docs, edges)
    }

    /// v2.14.0 — build a DYNAMIC MDN corpus graph from LIVE store rows (the
    /// v2.14.0 `corpus from axonstore` form). `docs` are `(string_id, title)` rows
    /// from the documents store; `edges` are `(from_id, to_id, etype_slug,
    /// weight)` rows from the edge store. String ids map to internal `DocId`s by
    /// first-seen order.
    ///
    /// Unlike the compile-time [`Corpus::from_declaration`], this path is
    /// RESILIENT to live, growing data: an edge whose endpoint is not a known
    /// document, or whose type is off-catalog, or whose weight is non-finite, is
    /// SKIPPED — a dangling/garbage row (e.g. an edge to a since-deleted summary)
    /// must never break a live navigation. The weight is CLAMPED to `(0, 1]`
    /// (the v2.14.0 G4 invariant is a RUNTIME clamp here, weights being per-row
    /// dynamic, not compile-time literals). Returns `Err(Empty)` only when there
    /// are no documents.
    pub fn from_rows(
        docs: &[(String, String)],
        edges: &[(String, String, String, f64)],
    ) -> Result<Corpus, CorpusError> {
        if docs.is_empty() {
            return Err(CorpusError::Empty);
        }
        let mut index: HashMap<&str, DocId> = HashMap::new();
        let mut documents = Vec::with_capacity(docs.len());
        for (i, (sid, title)) in docs.iter().enumerate() {
            let id = i as DocId;
            // First-seen wins on a duplicate id (a store should enforce a PK,
            // but live data is live data).
            index.entry(sid.as_str()).or_insert(id);
            documents.push(Document {
                id,
                title: title.clone(),
                depth: 0,
                recency: 0.5,
                epistemic: "believe".to_string(),
            });
        }
        let mut built = Vec::new();
        for (from, to, etype, weight) in edges {
            let (Some(&f), Some(&t)) =
                (index.get(from.as_str()), index.get(to.as_str()))
            else {
                continue; // dangling endpoint — skip (resilient to live data)
            };
            let Some(et) = EdgeType::from_slug(etype) else {
                continue; // off-catalog type — skip
            };
            if !weight.is_finite() {
                continue; // garbage weight — skip
            }
            // ω ∈ (0, 1] runtime clamp (v2.14.0 G4 as a runtime invariant).
            let w = weight.clamp(1e-9, 1.0);
            built.push(Edge { from: f, to: t, etype: et, weight: w });
        }
        Corpus::new(documents, built)
    }

    /// `|D|`.
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    pub fn document(&self, id: DocId) -> Option<&Document> {
        self.docs.get(&id)
    }

    /// All documents (for the v2.12.0 memory endofunctor, which rebuilds the corpus
    /// with updated edge weights — geometry, not topology).
    pub fn documents(&self) -> Vec<&Document> {
        self.docs.values().collect()
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// Document ids in ascending order — a deterministic index ordering for the
    /// EPR vectors.
    fn ordered_ids(&self) -> Vec<DocId> {
        let mut ids: Vec<DocId> = self.docs.keys().copied().collect();
        ids.sort_unstable();
        ids
    }
}

/// Parameters of the signed Epistemic PageRank (paper Def 4.2).
#[derive(Debug, Clone)]
pub struct EprParams {
    /// Damping `d ∈ (0, 1)` — must be strict for Perron–Frobenius (Theorem 3).
    pub damping: f64,
    /// Distrust penalty `λ ∈ [0, 1]` weighting `EPR⁻` in the net `EPR`.
    pub lambda: f64,
    /// Convergence tolerance on the L1 change between iterations.
    pub tolerance: f64,
    /// Iteration cap (a safety bound; convergence is geometric).
    pub max_iter: usize,
}

impl Default for EprParams {
    fn default() -> Self {
        EprParams { damping: 0.85, lambda: 0.5, tolerance: 1e-10, max_iter: 200 }
    }
}

/// The result of an EPR computation (paper Def 4.2).
#[derive(Debug, Clone)]
pub struct EprResult {
    /// Net signed reputation `EPR = EPR⁺ - λ·EPR⁻` per document. May be negative.
    pub epr: HashMap<DocId, f64>,
    /// Trust distribution `EPR⁺` (sums to 1, strictly positive).
    pub epr_plus: HashMap<DocId, f64>,
    /// Distrust distribution `EPR⁻` (sums to 1, strictly positive).
    pub epr_minus: HashMap<DocId, f64>,
    /// Iterations the trust channel took to converge.
    pub iterations: usize,
}

/// Build a row-stochastic transition matrix for the edges of a given polarity,
/// in `ordered_ids` index space. `mat[j]` is the (target-index, prob) list for
/// source index `j`; a source with no out-edges of this polarity is *dangling*
/// and is flagged (its row is treated as uniform `1/n` — paper P4).
fn build_transition(
    corpus: &Corpus,
    ids: &[DocId],
    polarity: Polarity,
) -> (Vec<Vec<(usize, f64)>>, Vec<bool>) {
    let index: HashMap<DocId, usize> = ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let n = ids.len();
    // Accumulate out-weights per source over edges of the requested polarity.
    let mut out: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    let mut row_sum: Vec<f64> = vec![0.0; n];
    for e in &corpus.edges {
        if e.etype.polarity() != polarity {
            continue;
        }
        let (j, i) = (index[&e.from], index[&e.to]);
        out[j].push((i, e.weight));
        row_sum[j] += e.weight;
    }
    // Normalize each non-dangling row to sum 1 (row-stochastic, P1-P3).
    let mut dangling = vec![false; n];
    for j in 0..n {
        if row_sum[j] > 0.0 {
            for (_, w) in out[j].iter_mut() {
                *w /= row_sum[j];
            }
        } else {
            dangling[j] = true; // P4 — handled as uniform during iteration.
        }
    }
    (out, dangling)
}

/// One power-iteration fixed point `x = (1-d)·u + d·Pᵀ·x` with dangling mass
/// redistributed uniformly (paper P4). Returns `(x, iterations)`. `u` must be a
/// strictly-positive distribution (Perron–Frobenius requirement).
fn power_iterate(
    transition: &[Vec<(usize, f64)>],
    dangling: &[bool],
    u: &[f64],
    params: &EprParams,
) -> (Vec<f64>, usize) {
    let n = u.len();
    if n == 0 {
        return (Vec::new(), 0);
    }
    let d = params.damping;
    let mut x = vec![1.0 / n as f64; n];
    let mut iters = 0;
    for _ in 0..params.max_iter {
        iters += 1;
        // Dangling mass: a dangling source distributes its rank uniformly.
        let dangling_mass: f64 =
            (0..n).filter(|&j| dangling[j]).map(|j| x[j]).sum::<f64>() * d / n as f64;
        let mut next = vec![0.0_f64; n];
        for i in 0..n {
            next[i] = (1.0 - d) * u[i] + dangling_mass;
        }
        // d · (P)ᵀ · x : each source j pushes its rank along its out-edges.
        for (j, row) in transition.iter().enumerate() {
            if row.is_empty() {
                continue;
            }
            let xj = x[j] * d;
            for &(i, p) in row {
                next[i] += xj * p;
            }
        }
        let delta: f64 = (0..n).map(|i| (next[i] - x[i]).abs()).sum();
        x = next;
        if delta < params.tolerance {
            break;
        }
    }
    (x, iters)
}

/// Normalize a non-negative weight vector to a strictly-positive distribution on
/// the simplex (Perron–Frobenius needs `u > 0`). An all-zero input falls back to
/// uniform; otherwise a tiny ε floor keeps every entry strictly positive.
fn teleportation(weights: &[f64]) -> Vec<f64> {
    let n = weights.len();
    if n == 0 {
        return Vec::new();
    }
    const EPS: f64 = 1e-9;
    let floored: Vec<f64> = weights.iter().map(|&w| w.max(0.0) + EPS).collect();
    let sum: f64 = floored.iter().sum();
    floored.iter().map(|&w| w / sum).collect()
}

/// Compute the signed **Epistemic PageRank** of a corpus (paper Def 4.2,
/// Theorem 3). Embeddings-free.
pub fn epistemic_pagerank(corpus: &Corpus, params: &EprParams) -> EprResult {
    let ids = corpus.ordered_ids();
    let n = ids.len();

    // Asymmetric teleportation: trust ∝ 1/(depth+1); distrust ∝ recency.
    let u_plus: Vec<f64> = teleportation(
        &ids.iter()
            .map(|id| 1.0 / (corpus.docs[id].depth as f64 + 1.0))
            .collect::<Vec<_>>(),
    );
    let u_minus: Vec<f64> = teleportation(
        &ids.iter().map(|id| corpus.docs[id].recency).collect::<Vec<_>>(),
    );

    let (p_plus, dangle_plus) = build_transition(corpus, &ids, Polarity::Positive);
    let (p_minus, dangle_minus) = build_transition(corpus, &ids, Polarity::Negative);

    let (epr_plus_v, iters) = power_iterate(&p_plus, &dangle_plus, &u_plus, params);
    let (epr_minus_v, _) = power_iterate(&p_minus, &dangle_minus, &u_minus, params);

    let mut epr_plus = HashMap::with_capacity(n);
    let mut epr_minus = HashMap::with_capacity(n);
    let mut epr = HashMap::with_capacity(n);
    for (k, &id) in ids.iter().enumerate() {
        let p = epr_plus_v[k];
        let m = epr_minus_v[k];
        epr_plus.insert(id, p);
        epr_minus.insert(id, m);
        epr.insert(id, p - params.lambda * m);
    }

    EprResult { epr, epr_plus, epr_minus, iterations: iters }
}

// ── v2.12.0 — ε-informative navigation + submodular greedy ──────────────

/// The marginal information gain `I(A; d | Q, S)` of visiting candidate `d` given
/// the query and the already-selected set `S` (paper section 2.2, Cor 2.1). MUST be
/// **monotone submodular** for the `(1 − 1/e)` greedy guarantee — adding `d`
/// helps less as `S` grows (diminishing returns). An LLM estimates it in
/// production; a deterministic coverage gain drives the property tests.
pub trait MarginalGain {
    fn gain(&self, query: &str, candidate: DocId, selected: &[DocId]) -> f64;
}

/// Greedy maximization of a monotone submodular set function under the
/// cardinality constraint `|S| ≤ k` (Nemhauser–Wolsey–Fisher 1978): repeatedly
/// add the unselected element of maximum marginal gain, stopping at `k` or when
/// no positive-gain element remains.
///
/// Guarantee: `f(S_greedy) ≥ (1 − 1/e) · f(S_OPT) ≈ 0.632 · f(S_OPT)` — tight
/// unless P = NP (paper section 2.2 NOTE). Candidate order ties break by `DocId` for
/// determinism. Returns the selected ids in selection order.
pub fn greedy_submodular_select(
    ground: &[DocId],
    k: usize,
    query: &str,
    gain: &dyn MarginalGain,
) -> Vec<DocId> {
    let mut selected: Vec<DocId> = Vec::new();
    while selected.len() < k {
        let mut best: Option<(DocId, f64)> = None;
        for &d in ground {
            if selected.contains(&d) {
                continue;
            }
            let g = gain.gain(query, d, &selected);
            match best {
                Some((_, bg)) if g <= bg => {}
                _ => best = Some((d, g)),
            }
        }
        match best {
            Some((d, g)) if g > 0.0 => selected.push(d),
            _ => break,
        }
    }
    selected
}

/// Budget for bounded reachability navigation (paper Theorem 1).
#[derive(Debug, Clone)]
pub struct NavBudget {
    /// Maximum documents to retrieve `|S| ≤ max_docs`.
    pub max_docs: usize,
    /// Minimum per-step information gain `ε > 0` — the ε-informative floor
    /// (paper Def 3). Navigation stops rather than select an uninformative doc.
    pub epsilon: f64,
}

impl Default for NavBudget {
    fn default() -> Self {
        NavBudget { max_docs: 5, epsilon: 1e-6 }
    }
}

/// The result of a corpus navigation.
#[derive(Debug, Clone)]
pub struct MdnNavResult {
    /// The selected documents, in selection order (the seed first).
    pub selected: Vec<DocId>,
    /// `(doc, marginal_gain)` per selection — the explainable reasoning trail.
    pub trail: Vec<(DocId, f64)>,
    /// `Σ` marginal gains — the total information `f(S) = I(A; S | Q)` acquired.
    pub total_gain: f64,
}

/// ε-informative greedy navigation over the corpus graph (paper section 2.2, Cor 2.1 +
/// Def 3). From a seed document, repeatedly select the **reachable** (out-edge
/// adjacent), unvisited document with maximum marginal information gain, while
/// the gain stays `≥ ε` (ε-informative — never visit an uninformative doc) and
/// the budget holds. The greedy step is `(1 − 1/e)`-optimal by submodularity;
/// the per-step `≥ ε` floor gives the convergence bound `k ≤ ⌈H₀/ε⌉`
/// (Theorem 2).
pub fn navigate_corpus(
    corpus: &Corpus,
    query: &str,
    seed: DocId,
    budget: &NavBudget,
    gain: &dyn MarginalGain,
) -> MdnNavResult {
    let mut selected: Vec<DocId> = Vec::new();
    let mut trail: Vec<(DocId, f64)> = Vec::new();
    let mut total_gain = 0.0;

    if corpus.docs.contains_key(&seed) {
        selected.push(seed);
        trail.push((seed, 0.0));
    }

    while selected.len() < budget.max_docs {
        // Reachable frontier: unvisited out-neighbours of any selected doc.
        let mut frontier: Vec<DocId> = Vec::new();
        for e in &corpus.edges {
            if selected.contains(&e.from)
                && !selected.contains(&e.to)
                && !frontier.contains(&e.to)
            {
                frontier.push(e.to);
            }
        }
        if frontier.is_empty() {
            break;
        }
        frontier.sort_unstable(); // deterministic tie-break

        // Greedy: the reachable doc of maximum marginal gain.
        let mut best: Option<(DocId, f64)> = None;
        for &d in &frontier {
            let g = gain.gain(query, d, &selected);
            match best {
                Some((_, bg)) if g <= bg => {}
                _ => best = Some((d, g)),
            }
        }
        match best {
            // ε-informative: only select if the gain clears the floor.
            Some((d, g)) if g >= budget.epsilon => {
                selected.push(d);
                trail.push((d, g));
                total_gain += g;
            }
            _ => break,
        }
    }

    MdnNavResult { selected, trail, total_gain }
}

// ── v2.12.0 — contradiction / balance + Jeffreys cost + shortest path ───

/// The contradiction relation (paper Def 11): the document pairs `(Dᵢ, Dⱼ)`
/// joined by a `contradict` edge — `Dᵢ` disputes a claim in `Dⱼ`.
pub fn contradictions(corpus: &Corpus) -> Vec<(DocId, DocId)> {
    corpus
        .edges
        .iter()
        .filter(|e| e.etype == EdgeType::Contradict)
        .map(|e| (e.from, e.to))
        .collect()
}

/// Structural balance (Harary 1953; paper section 2.3 NOTE). A signed corpus is
/// **balanced** iff its documents 2-color so every positive (trust) edge is
/// intra-color and every negative (distrust) edge is inter-color — equivalently,
/// every cycle has an even number of negative edges. Balanced ⇒ clean epistemic
/// consensus; **unbalanced ⇒ genuine controversy** requiring contradiction
/// resolution. Neutral edges are ignored; edges are taken undirected for balance.
pub fn is_balanced(corpus: &Corpus) -> bool {
    // Signed undirected adjacency: (neighbour, positive?).
    let mut adj: HashMap<DocId, Vec<(DocId, bool)>> = HashMap::new();
    for e in &corpus.edges {
        let pos = match e.etype.polarity() {
            Polarity::Positive => true,
            Polarity::Negative => false,
            Polarity::Neutral => continue,
        };
        adj.entry(e.from).or_default().push((e.to, pos));
        adj.entry(e.to).or_default().push((e.from, pos));
    }

    let mut color: HashMap<DocId, bool> = HashMap::new();
    for &start in corpus.docs.keys() {
        if color.contains_key(&start) {
            continue;
        }
        color.insert(start, true);
        let mut stack = vec![start];
        while let Some(u) = stack.pop() {
            let cu = color[&u];
            if let Some(neighbours) = adj.get(&u) {
                for &(v, pos) in neighbours {
                    // + edge ⇒ same colour; − edge ⇒ opposite.
                    let required = if pos { cu } else { !cu };
                    match color.get(&v) {
                        Some(&cv) => {
                            if cv != required {
                                return false; // a conflicting constraint ⇒ unbalanced.
                            }
                        }
                        None => {
                            color.insert(v, required);
                            stack.push(v);
                        }
                    }
                }
            }
        }
    }
    true
}

/// Jeffreys divergence `J(p, q) = KL(p‖q) + KL(q‖p)` — the symmetrized KL over
/// two topic distributions (paper Def 5). A pseudo-metric: `J ≥ 0`, `J(p,p)=0`,
/// symmetric. A small ε in the denominators keeps it finite when a coordinate is
/// zero in one distribution but not the other.
pub fn jeffreys_divergence(p: &[f64], q: &[f64]) -> f64 {
    const EPS: f64 = 1e-12;
    let n = p.len().max(q.len());
    let mut j = 0.0;
    for i in 0..n {
        let pi = p.get(i).copied().unwrap_or(0.0).max(0.0);
        let qi = q.get(i).copied().unwrap_or(0.0).max(0.0);
        if pi > 0.0 {
            j += pi * (pi / (qi + EPS)).ln();
        }
        if qi > 0.0 {
            j += qi * (qi / (pi + EPS)).ln();
        }
    }
    j.max(0.0)
}

/// The type-dependent cost coefficient `α_τ` (paper Def 5.1): following a
/// citation is cheap; navigating a contradiction is expensive.
pub fn type_cost_coefficient(etype: EdgeType) -> f64 {
    match etype {
        EdgeType::Corroborate => 0.5,
        EdgeType::Elaborate => 0.7,
        EdgeType::Cite => 1.0,
        EdgeType::Supersede => 2.0,
        EdgeType::Contradict => 3.0,
        // Neutral structural edges: unit cost.
        EdgeType::Depend | EdgeType::Implement | EdgeType::Exemplify => 1.0,
    }
}

/// The type-weighted divergence cost of an edge: `c = α_τ · J(Dᵢ, Dⱼ)` (paper
/// Def 5.1). `dist` provides each endpoint's topic distribution.
pub fn edge_cost(edge: &Edge, dist: &HashMap<DocId, Vec<f64>>) -> f64 {
    let empty = Vec::new();
    let p = dist.get(&edge.from).unwrap_or(&empty);
    let q = dist.get(&edge.to).unwrap_or(&empty);
    type_cost_coefficient(edge.etype) * jeffreys_divergence(p, q)
}

/// The optimal navigation path `π* = argmin_π Σ α_τ·J(Dᵢ,Dⱼ)` (paper section 2.4) — the
/// minimum-cost weighted path via Dijkstra (a graph shortest path, not a
/// geodesic). Edge costs are non-negative (`α_τ > 0`, `J ≥ 0`), so Dijkstra is
/// exact. Returns `(path, total_cost)` or `None` if `to` is unreachable.
pub fn shortest_cost_path(
    corpus: &Corpus,
    dist: &HashMap<DocId, Vec<f64>>,
    from: DocId,
    to: DocId,
) -> Option<(Vec<DocId>, f64)> {
    use std::cmp::Ordering;
    use std::collections::BinaryHeap;

    if !corpus.docs.contains_key(&from) || !corpus.docs.contains_key(&to) {
        return None;
    }

    // Out-edge adjacency with precomputed costs.
    let mut adj: HashMap<DocId, Vec<(DocId, f64)>> = HashMap::new();
    for e in &corpus.edges {
        adj.entry(e.from).or_default().push((e.to, edge_cost(e, dist)));
    }

    // Min-heap state ordered by ascending cost (Reverse via the Ord impl).
    struct State {
        cost: f64,
        node: DocId,
    }
    impl PartialEq for State {
        fn eq(&self, o: &Self) -> bool {
            self.cost == o.cost && self.node == o.node
        }
    }
    impl Eq for State {}
    impl Ord for State {
        fn cmp(&self, o: &Self) -> Ordering {
            // Reverse on cost (min-heap); tie-break on node for determinism.
            o.cost
                .partial_cmp(&self.cost)
                .unwrap_or(Ordering::Equal)
                .then_with(|| o.node.cmp(&self.node))
        }
    }
    impl PartialOrd for State {
        fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
            Some(self.cmp(o))
        }
    }

    let mut best: HashMap<DocId, f64> = HashMap::new();
    let mut prev: HashMap<DocId, DocId> = HashMap::new();
    let mut heap = BinaryHeap::new();
    best.insert(from, 0.0);
    heap.push(State { cost: 0.0, node: from });

    while let Some(State { cost, node }) = heap.pop() {
        if node == to {
            // Reconstruct the path.
            let mut path = vec![to];
            let mut cur = to;
            while let Some(&p) = prev.get(&cur) {
                path.push(p);
                cur = p;
            }
            path.reverse();
            return Some((path, cost));
        }
        if cost > *best.get(&node).unwrap_or(&f64::INFINITY) {
            continue; // a stale heap entry.
        }
        if let Some(neighbours) = adj.get(&node) {
            for &(next, w) in neighbours {
                let nc = cost + w;
                if nc < *best.get(&next).unwrap_or(&f64::INFINITY) {
                    best.insert(next, nc);
                    prev.insert(next, node);
                    heap.push(State { cost: nc, node: next });
                }
            }
        }
    }
    None
}

// ── v2.13.0 — deterministic reference MarginalGain over document titles ───

fn title_tokens(s: &str) -> HashSet<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 2)
        .map(|w| w.to_lowercase())
        .collect()
}

/// v2.13.0 — a deterministic, embeddings-free reference [`MarginalGain`]:
/// query-term **coverage** over document titles. The marginal gain of a
/// candidate is the number of query terms its title covers that the
/// already-selected set does not — monotone submodular (diminishing returns) by
/// construction, so the navigation's `(1 − 1/e)` greedy guarantee holds. The
/// production gain is LLM-estimated; this OSS reference keeps MDN navigation
/// reproducible (the same role `LexicalScorer` plays for PIX).
pub struct LexicalGain<'a> {
    corpus: &'a Corpus,
}

impl<'a> LexicalGain<'a> {
    pub fn new(corpus: &'a Corpus) -> Self {
        LexicalGain { corpus }
    }
}

impl MarginalGain for LexicalGain<'_> {
    fn gain(&self, query: &str, candidate: DocId, selected: &[DocId]) -> f64 {
        let q = title_tokens(query);
        if q.is_empty() {
            return 0.0;
        }
        let mut covered: HashSet<String> = HashSet::new();
        for d in selected {
            if let Some(doc) = self.corpus.document(*d) {
                covered.extend(title_tokens(&doc.title));
            }
        }
        let cand = self
            .corpus
            .document(candidate)
            .map(|d| title_tokens(&d.title))
            .unwrap_or_default();
        q.iter().filter(|t| cand.contains(*t) && !covered.contains(*t)).count() as f64
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── v2.14.0 — Corpus::from_rows (dynamic, store-sourced graph) ──────

    #[test]
    fn from_rows_builds_the_live_graph() {
        let docs = vec![
            ("uuid-a".to_string(), "Session A summary".to_string()),
            ("uuid-b".to_string(), "Session B summary".to_string()),
            ("uuid-c".to_string(), "Session C summary".to_string()),
        ];
        let edges = vec![
            ("uuid-b".to_string(), "uuid-a".to_string(), "cite".to_string(), 0.9),
            ("uuid-c".to_string(), "uuid-a".to_string(), "contradict".to_string(), 0.7),
        ];
        let c = Corpus::from_rows(&docs, &edges).expect("builds");
        assert_eq!(c.len(), 3, "all three rows became documents");
        // titles preserved; ids interned by first-seen order.
        let titles: Vec<_> = {
            let mut t: Vec<_> = c.documents().iter().map(|d| d.title.clone()).collect();
            t.sort();
            t
        };
        assert_eq!(titles, vec!["Session A summary", "Session B summary", "Session C summary"]);
    }

    #[test]
    fn from_rows_skips_dangling_edges_and_unknown_types() {
        let docs = vec![("a".to_string(), "A".to_string()), ("b".to_string(), "B".to_string())];
        let edges = vec![
            ("b".to_string(), "a".to_string(), "cite".to_string(), 0.5), // good
            ("b".to_string(), "ghost".to_string(), "cite".to_string(), 0.5), // dangling → skip
            ("a".to_string(), "b".to_string(), "hugs".to_string(), 0.5), // off-catalog → skip
        ];
        let c = Corpus::from_rows(&docs, &edges).expect("builds");
        // Only the one valid edge survives — a garbage row must not break the graph.
        assert_eq!(c.edges().len(), 1, "dangling + off-catalog edges dropped");
    }

    #[test]
    fn from_rows_clamps_weight_into_unit_interval() {
        let docs = vec![("a".to_string(), "A".to_string()), ("b".to_string(), "B".to_string())];
        let edges = vec![
            ("b".to_string(), "a".to_string(), "cite".to_string(), 5.0),  // > 1 → 1.0
            ("a".to_string(), "b".to_string(), "cite".to_string(), 0.0),  // ≤ 0 → tiny+
            ("a".to_string(), "b".to_string(), "cite".to_string(), f64::NAN), // garbage → skip
        ];
        let c = Corpus::from_rows(&docs, &edges).expect("builds (G4 is a runtime clamp here)");
        let ws: Vec<f64> = c.edges().iter().map(|e| e.weight).collect();
        assert_eq!(ws.len(), 2, "the NaN-weight edge is skipped");
        assert!(ws.iter().all(|&w| w > 0.0 && w <= 1.0), "every weight clamped into (0,1]: {ws:?}");
    }

    #[test]
    fn from_rows_empty_documents_is_empty_error() {
        assert!(matches!(Corpus::from_rows(&[], &[]), Err(CorpusError::Empty)));
    }

    #[test]
    fn edge_type_slug_round_trips() {
        // v2.14.0 — slug() is the inverse of from_slug(), used to address an
        // edge's store row by its `etype` value in the write-back.
        for s in [
            "cite", "elaborate", "corroborate", "depend", "implement", "exemplify",
            "contradict", "supersede",
        ] {
            assert_eq!(EdgeType::from_slug(s).unwrap().slug(), s);
        }
    }

    fn doc(id: DocId, depth: u32, recency: f64) -> Document {
        Document {
            id,
            title: format!("D{id}"),
            depth,
            recency,
            epistemic: "believe".into(),
        }
    }

    fn edge(from: DocId, to: DocId, etype: EdgeType, weight: f64) -> Edge {
        Edge { from, to, etype, weight }
    }

    fn sum(m: &HashMap<DocId, f64>) -> f64 {
        m.values().sum()
    }

    // ── Polarity / construction ──────────────────────────────────────────────

    #[test]
    fn edge_polarity_matches_the_paper_decomposition() {
        assert_eq!(EdgeType::Cite.polarity(), Polarity::Positive);
        assert_eq!(EdgeType::Corroborate.polarity(), Polarity::Positive);
        assert_eq!(EdgeType::Contradict.polarity(), Polarity::Negative);
        assert_eq!(EdgeType::Supersede.polarity(), Polarity::Negative);
        assert_eq!(EdgeType::Depend.polarity(), Polarity::Neutral);
    }

    #[test]
    fn corpus_rejects_unknown_endpoint_and_bad_weight() {
        let docs = vec![doc(1, 0, 0.5), doc(2, 1, 0.5)];
        assert_eq!(
            Corpus::new(docs.clone(), vec![edge(1, 9, EdgeType::Cite, 0.5)]).unwrap_err(),
            CorpusError::UnknownEndpoint(9)
        );
        assert_eq!(
            Corpus::new(docs, vec![edge(1, 2, EdgeType::Cite, 1.5)]).unwrap_err(),
            CorpusError::BadWeight(1, 2)
        );
        assert_eq!(Corpus::new(vec![], vec![]).unwrap_err(), CorpusError::Empty);
    }

    // ── v2.13.0 — build an MDN graph from a `corpus` declaration ───────────────

    #[test]
    fn edge_type_from_slug_roundtrips() {
        assert_eq!(EdgeType::from_slug("cite"), Some(EdgeType::Cite));
        assert_eq!(EdgeType::from_slug("supersede"), Some(EdgeType::Supersede));
        assert_eq!(EdgeType::from_slug("corroborate"), Some(EdgeType::Corroborate));
        assert!(EdgeType::from_slug("nope").is_none());
    }

    #[test]
    fn from_declaration_builds_an_mdn_graph() {
        let docs = vec!["sess_a".to_string(), "sess_b".to_string(), "sess_c".to_string()];
        let rels = vec![
            ("cite".to_string(), "sess_b".to_string(), "sess_a".to_string(), 0.9),
            ("contradict".to_string(), "sess_c".to_string(), "sess_a".to_string(), 0.7),
        ];
        let c = Corpus::from_declaration(&docs, &rels).unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(c.edges().len(), 2);
        // The built graph drives the signed EPR: the cited sess_a (id 0) outranks
        // the uncited sess_c (id 2).
        let r = epistemic_pagerank(&c, &EprParams::default());
        assert!(r.epr_plus[&0] > r.epr_plus[&2]);
    }

    #[test]
    fn from_declaration_rejects_unknown_type_and_doc() {
        let docs = vec!["a".to_string(), "b".to_string()];
        assert_eq!(
            Corpus::from_declaration(&docs, &[("hug".into(), "a".into(), "b".into(), 0.5)])
                .unwrap_err(),
            CorpusError::UnknownRelationType("hug".into())
        );
        assert_eq!(
            Corpus::from_declaration(&docs, &[("cite".into(), "a".into(), "z".into(), 0.5)])
                .unwrap_err(),
            CorpusError::UnknownDocumentRef("z".into())
        );
    }

    // ── Signed EPR — the Perron–Frobenius guarantees ─────────────────────────

    #[test]
    fn epr_plus_and_minus_are_strictly_positive_distributions() {
        // A small citation chain + one contradiction.
        let docs = vec![doc(1, 0, 0.1), doc(2, 1, 0.3), doc(3, 2, 0.9)];
        let edges = vec![
            edge(2, 1, EdgeType::Cite, 0.9),
            edge(3, 1, EdgeType::Cite, 0.8),
            edge(3, 2, EdgeType::Contradict, 0.7),
        ];
        let c = Corpus::new(docs, edges).unwrap();
        let r = epistemic_pagerank(&c, &EprParams::default());

        // Each channel is a distribution (sums to 1) and strictly positive
        // (Perron–Frobenius).
        assert!((sum(&r.epr_plus) - 1.0).abs() < 1e-6, "EPR⁺ sums to 1");
        assert!((sum(&r.epr_minus) - 1.0).abs() < 1e-6, "EPR⁻ sums to 1");
        assert!(r.epr_plus.values().all(|&v| v > 0.0), "EPR⁺ strictly positive");
        assert!(r.epr_minus.values().all(|&v| v > 0.0), "EPR⁻ strictly positive");
    }

    #[test]
    fn net_epr_sums_to_one_minus_lambda() {
        let docs = vec![doc(1, 0, 0.1), doc(2, 1, 0.5)];
        let edges = vec![edge(2, 1, EdgeType::Cite, 0.9)];
        let c = Corpus::new(docs, edges).unwrap();
        let params = EprParams { lambda: 0.5, ..EprParams::default() };
        let r = epistemic_pagerank(&c, &params);
        // EPR = EPR⁺ - λ·EPR⁻ ⇒ Σ EPR = 1 - λ (paper section 2.3 WARNING).
        assert!((sum(&r.epr) - (1.0 - params.lambda)).abs() < 1e-6);
    }

    #[test]
    fn a_heavily_cited_document_outranks_an_uncited_one() {
        // D1 is cited by D2 and D3; D4 is cited by nobody.
        let docs = vec![doc(1, 0, 0.1), doc(2, 1, 0.5), doc(3, 1, 0.5), doc(4, 1, 0.5)];
        let edges = vec![
            edge(2, 1, EdgeType::Cite, 0.9),
            edge(3, 1, EdgeType::Cite, 0.9),
        ];
        let c = Corpus::new(docs, edges).unwrap();
        let r = epistemic_pagerank(&c, &EprParams::default());
        assert!(
            r.epr_plus[&1] > r.epr_plus[&4],
            "the cited D1 ({}) must outrank the uncited D4 ({})",
            r.epr_plus[&1],
            r.epr_plus[&4]
        );
    }

    #[test]
    fn a_contested_document_can_have_negative_net_epr() {
        // D1 is lightly cited but heavily contradicted ⇒ more contested than
        // endorsed ⇒ negative net EPR (paper section 2.3 WARNING — a meaningful signal
        // a probability distribution cannot express).
        let docs = vec![doc(1, 0, 0.9), doc(2, 1, 0.1), doc(3, 1, 0.1), doc(4, 1, 0.1)];
        let edges = vec![
            edge(2, 1, EdgeType::Cite, 0.1),
            edge(2, 1, EdgeType::Contradict, 1.0),
            edge(3, 1, EdgeType::Contradict, 1.0),
            edge(4, 1, EdgeType::Contradict, 1.0),
        ];
        let c = Corpus::new(docs, edges).unwrap();
        let params = EprParams { lambda: 1.0, ..EprParams::default() };
        let r = epistemic_pagerank(&c, &params);
        assert!(
            r.epr[&1] < 0.0,
            "a heavily-contradicted, lightly-cited doc has negative EPR: {}",
            r.epr[&1]
        );
    }

    #[test]
    fn power_iteration_converges_well_within_the_cap() {
        let docs = vec![doc(1, 0, 0.1), doc(2, 1, 0.5), doc(3, 2, 0.9)];
        let edges = vec![
            edge(2, 1, EdgeType::Cite, 0.9),
            edge(3, 2, EdgeType::Cite, 0.8),
        ];
        let c = Corpus::new(docs, edges).unwrap();
        let params = EprParams::default();
        let r = epistemic_pagerank(&c, &params);
        assert!(r.iterations < params.max_iter, "converged before the cap (geometric)");
        assert!(r.iterations > 0);
    }

    #[test]
    fn dangling_nodes_keep_epr_a_proper_distribution() {
        // D1 has NO outgoing positive edges (dangling in G⁺). The uniform-row
        // treatment (P4) must keep EPR⁺ summing to 1.
        let docs = vec![doc(1, 0, 0.1), doc(2, 1, 0.5)];
        let edges = vec![edge(2, 1, EdgeType::Cite, 0.9)];
        let c = Corpus::new(docs, edges).unwrap();
        let r = epistemic_pagerank(&c, &EprParams::default());
        assert!((sum(&r.epr_plus) - 1.0).abs() < 1e-6, "dangling handled, still a distribution");
    }

    #[test]
    fn single_document_corpus_is_well_defined() {
        let c = Corpus::new(vec![doc(1, 0, 0.5)], vec![]).unwrap();
        let r = epistemic_pagerank(&c, &EprParams::default());
        assert!((r.epr_plus[&1] - 1.0).abs() < 1e-9, "the lone doc holds all trust mass");
    }

    // ── v2.12.0 — submodular greedy + ε-informative navigation ───────────────

    use std::collections::HashSet;

    /// A monotone submodular coverage gain: each doc "covers" a topic set; the
    /// marginal gain of adding `d` is the number of NEW topics it covers given
    /// the already-selected set. Classic submodular (diminishing returns).
    struct CoverageGain {
        topics: HashMap<DocId, Vec<u32>>,
    }
    impl MarginalGain for CoverageGain {
        fn gain(&self, _q: &str, candidate: DocId, selected: &[DocId]) -> f64 {
            let mut covered: HashSet<u32> = HashSet::new();
            for d in selected {
                if let Some(ts) = self.topics.get(d) {
                    covered.extend(ts.iter().copied());
                }
            }
            self.topics
                .get(&candidate)
                .map(|ts| ts.iter().filter(|t| !covered.contains(t)).count() as f64)
                .unwrap_or(0.0)
        }
    }
    fn coverage(topics: &HashMap<DocId, Vec<u32>>, set: &[DocId]) -> f64 {
        let mut u: HashSet<u32> = HashSet::new();
        for d in set {
            if let Some(ts) = topics.get(d) {
                u.extend(ts.iter().copied());
            }
        }
        u.len() as f64
    }
    fn combinations(items: &[DocId], k: usize) -> Vec<Vec<DocId>> {
        if k == 0 {
            return vec![vec![]];
        }
        if items.len() < k {
            return vec![];
        }
        let mut out = Vec::new();
        for mut rest in combinations(&items[1..], k - 1) {
            let mut v = vec![items[0]];
            v.append(&mut rest);
            out.push(v);
        }
        out.extend(combinations(&items[1..], k));
        out
    }

    #[test]
    fn greedy_submodular_achieves_the_one_minus_one_over_e_bound() {
        let topics = HashMap::from([
            (1, vec![1, 2, 3]),
            (2, vec![3, 4]),
            (3, vec![1, 2]),
            (4, vec![5, 6, 7]),
            (5, vec![7, 8]),
        ]);
        let g = CoverageGain { topics: topics.clone() };
        let ground = vec![1, 2, 3, 4, 5];
        let k = 2;
        let greedy = greedy_submodular_select(&ground, k, "", &g);
        let f_greedy = coverage(&topics, &greedy);
        // Brute-force the optimal k-subset.
        let opt = combinations(&ground, k)
            .iter()
            .map(|c| coverage(&topics, c))
            .fold(0.0_f64, f64::max);
        let bound = (1.0 - 1.0 / std::f64::consts::E) * opt;
        assert!(
            f_greedy >= bound - 1e-9,
            "greedy f={f_greedy} must be ≥ (1-1/e)·OPT={bound} (OPT={opt})"
        );
    }

    #[test]
    fn greedy_marginal_gains_are_non_increasing() {
        // Diminishing returns: under a monotone submodular f, the greedy marginal
        // gain sequence is non-increasing (paper Cor 2.1(a)).
        let topics = HashMap::from([
            (1, vec![1, 2, 3, 4]),
            (2, vec![3, 4, 5]),
            (3, vec![5, 6]),
            (4, vec![6, 7]),
        ]);
        let g = CoverageGain { topics };
        let ground = vec![1, 2, 3, 4];
        let sel = greedy_submodular_select(&ground, 4, "", &g);
        let mut prev = f64::INFINITY;
        let mut acc: Vec<DocId> = Vec::new();
        for d in sel {
            let m = g.gain("", d, &acc);
            assert!(m <= prev + 1e-9, "marginal gains must be non-increasing (got {m} after {prev})");
            prev = m;
            acc.push(d);
        }
    }

    #[test]
    fn navigate_corpus_is_epsilon_informative_and_only_visits_reachable() {
        // 1 → 2 → 3 (a citation chain); 4 is isolated/unreachable from the seed.
        let docs = vec![doc(1, 0, 0.1), doc(2, 1, 0.1), doc(3, 2, 0.1), doc(4, 1, 0.1)];
        let edges = vec![
            edge(1, 2, EdgeType::Cite, 0.9),
            edge(2, 3, EdgeType::Cite, 0.9),
        ];
        let c = Corpus::new(docs, edges).unwrap();
        let topics = HashMap::from([(1, vec![1]), (2, vec![2]), (3, vec![3]), (4, vec![4])]);
        let g = CoverageGain { topics };
        let budget = NavBudget { max_docs: 5, epsilon: 0.5 };
        let r = navigate_corpus(&c, "", 1, &budget, &g);

        // Only reachable docs are visited — D4 is never reached from the seed.
        assert!(!r.selected.contains(&4), "unreachable D4 must not be selected");
        assert_eq!(r.selected, vec![1, 2, 3], "follows the citation chain");
        // ε-informative: every non-seed selection cleared the ε floor.
        for &(d, gain) in &r.trail {
            if d != 1 {
                assert!(gain >= budget.epsilon, "selected {d} below ε floor: {gain}");
            }
        }
        // Bounded: at most max_docs.
        assert!(r.selected.len() <= budget.max_docs);
    }

    #[test]
    fn navigate_stops_when_no_neighbor_clears_epsilon() {
        // The neighbour covers a topic already held by the seed ⇒ marginal gain 0
        // < ε ⇒ navigation stops (ε-informative — no uninformative visit).
        let docs = vec![doc(1, 0, 0.1), doc(2, 1, 0.1)];
        let edges = vec![edge(1, 2, EdgeType::Cite, 0.9)];
        let c = Corpus::new(docs, edges).unwrap();
        let topics = HashMap::from([(1, vec![7]), (2, vec![7])]); // same topic
        let g = CoverageGain { topics };
        let r = navigate_corpus(&c, "", 1, &NavBudget { max_docs: 5, epsilon: 0.5 }, &g);
        assert_eq!(r.selected, vec![1], "the uninformative neighbour is not visited");
    }

    // ── v2.12.0 — contradiction / balance + Jeffreys cost + shortest path ────

    #[test]
    fn contradictions_lists_the_contradict_edges() {
        let docs = vec![doc(1, 0, 0.1), doc(2, 1, 0.1), doc(3, 1, 0.1)];
        let edges = vec![
            edge(1, 2, EdgeType::Cite, 0.9),
            edge(2, 3, EdgeType::Contradict, 0.8),
        ];
        let c = Corpus::new(docs, edges).unwrap();
        assert_eq!(contradictions(&c), vec![(2, 3)]);
    }

    #[test]
    fn balance_theory_even_negatives_balanced_odd_unbalanced() {
        let docs = vec![doc(1, 0, 0.1), doc(2, 1, 0.1), doc(3, 1, 0.1)];
        // All-positive triangle: balanced (clean consensus).
        let all_pos = Corpus::new(
            docs.clone(),
            vec![
                edge(1, 2, EdgeType::Cite, 0.9),
                edge(2, 3, EdgeType::Cite, 0.9),
                edge(1, 3, EdgeType::Cite, 0.9),
            ],
        )
        .unwrap();
        assert!(is_balanced(&all_pos), "all-positive cycle is balanced");

        // ONE negative edge (odd) → unbalanced (genuine controversy).
        let one_neg = Corpus::new(
            docs.clone(),
            vec![
                edge(1, 2, EdgeType::Cite, 0.9),
                edge(2, 3, EdgeType::Cite, 0.9),
                edge(1, 3, EdgeType::Contradict, 0.9),
            ],
        )
        .unwrap();
        assert!(!is_balanced(&one_neg), "one negative in the cycle ⇒ unbalanced");

        // TWO negative edges (even) → balanced again.
        let two_neg = Corpus::new(
            docs,
            vec![
                edge(1, 2, EdgeType::Contradict, 0.9),
                edge(2, 3, EdgeType::Contradict, 0.9),
                edge(1, 3, EdgeType::Cite, 0.9),
            ],
        )
        .unwrap();
        assert!(is_balanced(&two_neg), "two negatives (even) ⇒ balanced");
    }

    #[test]
    fn jeffreys_is_a_symmetric_nonnegative_pseudometric() {
        let p = vec![0.7, 0.3];
        let q = vec![0.3, 0.7];
        assert!(jeffreys_divergence(&p, &p) < 1e-9, "J(p,p) = 0");
        let jpq = jeffreys_divergence(&p, &q);
        let jqp = jeffreys_divergence(&q, &p);
        assert!((jpq - jqp).abs() < 1e-12, "J is symmetric");
        assert!(jpq > 0.0, "J(p,q) > 0 for distinct distributions");
    }

    #[test]
    fn type_cost_coefficients_rank_citations_cheap_contradictions_expensive() {
        assert!(
            type_cost_coefficient(EdgeType::Corroborate)
                < type_cost_coefficient(EdgeType::Cite)
        );
        assert!(
            type_cost_coefficient(EdgeType::Cite) < type_cost_coefficient(EdgeType::Supersede)
        );
        assert!(
            type_cost_coefficient(EdgeType::Supersede)
                < type_cost_coefficient(EdgeType::Contradict)
        );
        assert_eq!(type_cost_coefficient(EdgeType::Contradict), 3.0);
    }

    #[test]
    fn shortest_path_prefers_cheap_citations_over_an_expensive_contradiction() {
        // 1 →(contradict, α=3) 3   vs   1 →(cite) 2 →(cite) 3.
        let docs = vec![doc(1, 0, 0.1), doc(2, 1, 0.1), doc(3, 2, 0.1)];
        let edges = vec![
            edge(1, 3, EdgeType::Contradict, 0.9),
            edge(1, 2, EdgeType::Cite, 0.9),
            edge(2, 3, EdgeType::Cite, 0.9),
        ];
        let c = Corpus::new(docs, edges).unwrap();
        // All-positive distributions (finite J).
        let dist = HashMap::from([
            (1, vec![0.9, 0.1]),
            (2, vec![0.85, 0.15]),
            (3, vec![0.1, 0.9]),
        ]);
        let (path, cost) = shortest_cost_path(&c, &dist, 1, 3).unwrap();
        assert_eq!(path, vec![1, 2, 3], "Dijkstra takes the cheap two-cite route");
        // The direct contradiction edge costs strictly more.
        let direct = 3.0 * jeffreys_divergence(&dist[&1], &dist[&3]);
        assert!(cost < direct, "indirect cost {cost} < direct contradiction cost {direct}");
    }

    #[test]
    fn shortest_path_none_when_unreachable() {
        let docs = vec![doc(1, 0, 0.1), doc(2, 1, 0.1)];
        let c = Corpus::new(docs, vec![]).unwrap(); // no edges
        let dist = HashMap::from([(1, vec![1.0]), (2, vec![1.0])]);
        assert!(shortest_cost_path(&c, &dist, 1, 2).is_none());
    }

    // ── v2.13.0 — LexicalGain reference scorer drives MDN navigation ───────────

    fn titled(id: DocId, title: &str) -> Document {
        Document { id, title: title.into(), depth: 0, recency: 0.1, epistemic: "believe".into() }
    }

    #[test]
    fn lexical_gain_navigates_to_the_query_relevant_document() {
        // Seed cites a liability doc and a termination doc; the query about
        // liability must steer the navigation to the liability doc, not the other.
        let docs = vec![
            titled(0, "intro overview"),
            titled(1, "liability limitation cap"),
            titled(2, "termination notice"),
        ];
        let edges = vec![
            Edge { from: 0, to: 1, etype: EdgeType::Cite, weight: 0.9 },
            Edge { from: 0, to: 2, etype: EdgeType::Cite, weight: 0.9 },
        ];
        let c = Corpus::new(docs, edges).unwrap();
        let gain = LexicalGain::new(&c);
        let r = navigate_corpus(&c, "liability cap", 0, &NavBudget { max_docs: 3, epsilon: 0.5 }, &gain);
        assert!(r.selected.contains(&1), "navigated to the liability doc: {:?}", r.selected);
        assert!(!r.selected.contains(&2), "the uninformative termination doc was not visited");
    }

    #[test]
    fn lexical_gain_is_zero_for_irrelevant_titles() {
        let c = Corpus::new(vec![titled(0, "alpha"), titled(1, "beta")], vec![]).unwrap();
        let g = LexicalGain::new(&c);
        assert_eq!(g.gain("zzz qqq", 1, &[]), 0.0);
        assert_eq!(g.gain("beta", 1, &[]), 1.0);
        // Already covered by a selected doc with the same title ⇒ no new gain.
        assert_eq!(g.gain("beta", 1, &[1]), 0.0);
    }
}
