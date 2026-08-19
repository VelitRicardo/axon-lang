//! v2.12.0 — the PIX retrieval navigator.
//!
//! A faithful implementation of `docs/papers/paper_pix_formal_research.md`:
//! **embeddings-free** structured retrieval by intentional tree navigation.
//! There is no embedding, no vector store, and no cosine similarity here. A
//! document is a tree `D = (N, E, ρ, κ)`; retrieval is a bounded breadth-first
//! traversal whose branch selection approximates the **conditional mutual
//! information** `I(R; node | Q, path)` between a node and the answer, given the
//! query and the navigational path already taken.
//!
//! # What the paper guarantees, and how this module honours it
//!
//! - **Axiom (section 1.3):** `Relevant(section, q) ⟺ I(R; section | q, path) > ε`.
//!   The branch score is exactly this conditional-MI estimate, supplied by a
//!   [`RelevanceScorer`] (an LLM in production; a deterministic scorer in tests).
//! - **Theorem 2 (monotone entropy reduction):** selecting a node reduces the
//!   conditional entropy of the answer by `I(R; node | …) ≥ 0`. We track the
//!   cumulative information gain along each retrieved path; because every score
//!   is non-negative, the residual entropy `H₀ − gain` is **non-increasing**.
//!   ([`tests::path_gain_is_monotone_nondecreasing`].)
//! - **Convergence corollary:** navigation terminates in at most `d_max` levels.
//!   ([`tests::navigation_terminates_within_d_max`].)
//! - **Theorem 4 (explainability by construction):** every retrieved leaf
//!   carries its reasoning path, and the [`NavResult::trail`] records every
//!   per-level score + the selection threshold. ([`tests::every_leaf_has_a_path`].)
//!
//! The tree-construction (indexing) phase and the LLM-backed scorer are wired in
//! v2.12.0; this module is the algorithm and is fully deterministic + verifiable.

use std::collections::{HashMap, HashSet};

/// Stable identifier of a node within a single [`PixTree`].
pub type NodeId = u32;

/// `ρ(n).location` — spatial metadata locating a node's content in the source
/// document, so a navigated leaf can be resolved back to uncompressed content.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Location {
    pub page_start: u32,
    pub page_end: u32,
    pub offset_start: u32,
    pub offset_end: u32,
}

/// A node of a PIX document tree. `ρ(n) = ⟨title, summary, location, children⟩`
/// (paper Definition 1 / section 2.2).
///
/// Internal nodes carry only the lossy `summary` — a high-salience compression
/// (target ratio `CR ∈ [0.05, 0.15]`) sufficient to decide *whether to explore
/// deeper*, not to answer. Leaves additionally carry uncompressed `content`,
/// the actual answer source.
#[derive(Debug, Clone)]
pub struct PixNode {
    pub id: NodeId,
    pub title: String,
    pub summary: String,
    pub location: Location,
    pub children: Vec<NodeId>,
    /// `Some` for leaves (uncompressed content); `None` for internal nodes.
    pub content: Option<String>,
}

impl PixNode {
    /// A leaf has no children (paper: `κ(n) = ∅`).
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }
}

/// A PIX document tree `D = (N, E, ρ, κ)` (paper Definition 1).
#[derive(Debug, Clone)]
pub struct PixTree {
    nodes: HashMap<NodeId, PixNode>,
    root: NodeId,
}

/// Why a node set fails to form a valid document tree (paper invariants T1–T3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// The declared root id is absent from the node set.
    MissingRoot(NodeId),
    /// A child id is referenced but no such node exists.
    DanglingChild(NodeId),
    /// (T2) A node is the child of more than one parent — not a tree.
    MultipleParents(NodeId),
    /// (T1) Some node is unreachable from the root, or the root is not unique.
    NotConnected(NodeId),
    /// (T3) The child relation contains a cycle (would break termination).
    Cycle(NodeId),
}

impl PixTree {
    /// Build a tree from its nodes + declared root, validating the **structural**
    /// invariants:
    ///
    /// - **T1 (unique root, connected):** every node is reachable from `root`.
    /// - **T2 (unique parent):** no node is a child of two parents.
    /// - **T3 (acyclic):** the child relation is a DAG-free tree — guarantees any
    ///   traversal terminates.
    ///
    /// T4 (exhaustive coverage) and T5 (controlled sibling disjunction) are
    /// *semantic* properties of the indexing step (they constrain `content` /
    /// `summary`, not the shape) and are asserted when a tree is indexed, not here.
    pub fn new(nodes: Vec<PixNode>, root: NodeId) -> Result<Self, TreeError> {
        let mut map: HashMap<NodeId, PixNode> = HashMap::with_capacity(nodes.len());
        for n in nodes {
            map.insert(n.id, n);
        }
        if !map.contains_key(&root) {
            return Err(TreeError::MissingRoot(root));
        }

        // Every referenced child must exist, and no child may have two parents
        // (T2). Count in-degrees over the child relation.
        let mut indegree: HashMap<NodeId, u32> = HashMap::new();
        for node in map.values() {
            for &c in &node.children {
                if !map.contains_key(&c) {
                    return Err(TreeError::DanglingChild(c));
                }
                let e = indegree.entry(c).or_insert(0);
                *e += 1;
                if *e > 1 {
                    return Err(TreeError::MultipleParents(c));
                }
            }
        }
        // The root has in-degree 0; every non-root has in-degree exactly 1 (T2)
        // — enforced above for >1; here we ensure the root itself is not a child.
        if indegree.get(&root).copied().unwrap_or(0) != 0 {
            return Err(TreeError::MultipleParents(root));
        }

        // T1 + T3: a BFS from root must reach every node exactly once (connected)
        // without revisiting (acyclic). Revisiting a node ⇒ cycle or shared child.
        let mut seen: HashSet<NodeId> = HashSet::new();
        let mut frontier = vec![root];
        seen.insert(root);
        while let Some(id) = frontier.pop() {
            // Safe: id is in `map` (root checked; children checked for existence).
            for &c in &map[&id].children {
                if !seen.insert(c) {
                    return Err(TreeError::Cycle(c));
                }
                frontier.push(c);
            }
        }
        if seen.len() != map.len() {
            // Some node is unreachable from the root (T1 violated).
            let orphan = map.keys().find(|k| !seen.contains(k)).copied().unwrap_or(root);
            return Err(TreeError::NotConnected(orphan));
        }

        Ok(PixTree { nodes: map, root })
    }

    /// The root node `n₀` (paper T1).
    pub fn root(&self) -> &PixNode {
        &self.nodes[&self.root]
    }

    /// Look up a node by id.
    pub fn node(&self, id: NodeId) -> Option<&PixNode> {
        self.nodes.get(&id)
    }

    /// The children `κ(n)` of a node, in declared order.
    pub fn children_of(&self, id: NodeId) -> Vec<&PixNode> {
        self.nodes
            .get(&id)
            .map(|n| n.children.iter().filter_map(|c| self.nodes.get(c)).collect())
            .unwrap_or_default()
    }

    /// The height `h` of the tree (longest root-to-leaf path length). The paper's
    /// convergence corollary bounds navigation by `min(d_max, height)`.
    pub fn height(&self) -> usize {
        fn depth(t: &PixTree, id: NodeId) -> usize {
            let node = &t.nodes[&id];
            if node.is_leaf() {
                0
            } else {
                1 + node.children.iter().map(|&c| depth(t, c)).max().unwrap_or(0)
            }
        }
        depth(self, self.root)
    }

    /// Number of nodes `|N|`.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the tree has only the root.
    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }
}

/// The conditional-mutual-information estimator `f_LLM(Q, n.summary) ≈
/// I(R; n | Q, path) ∈ [0, 1]` (paper section 2.3).
///
/// A score near `1` means "visiting this node strongly reduces uncertainty about
/// the answer, given where we are"; near `0` means "uninformative". Production
/// uses an LLM that *reasons about whether a summary suggests it contains the
/// answer* — its actual strength, not embedding similarity. Tests inject a
/// deterministic scorer.
pub trait RelevanceScorer {
    /// Score a candidate `node` for `query`, conditioned on the `path` (ids of
    /// the ancestors already traversed). MUST return a value in `[0, 1]`; the
    /// navigator clamps defensively but the entropy guarantee assumes `≥ 0`.
    fn score(&self, query: &str, node: &PixNode, path: &[NodeId]) -> f64;
}

/// Navigation budget — the paper's `(b_max, d_max)` bounded-rationality knobs
/// plus the adaptive-threshold ratio.
#[derive(Debug, Clone)]
pub struct NavConfig {
    /// Maximum branching factor: at most this many children are expanded per
    /// node per level (top-k after thresholding). Paper default 3.
    pub b_max: usize,
    /// Maximum navigation depth — the hard convergence bound. Paper default 4.
    pub d_max: usize,
    /// Adaptive-threshold ratio `ρ ∈ [0, 1]`: a child survives pruning iff its
    /// score `≥ ρ · max_sibling_score`. Higher ⇒ stricter (more specific query).
    pub theta_ratio: f64,
}

impl Default for NavConfig {
    fn default() -> Self {
        NavConfig { b_max: 3, d_max: 4, theta_ratio: 0.5 }
    }
}

/// One navigational decision at a single frontier node (paper Theorem 4 — the
/// reasoning path is recorded by construction, never reconstructed post-hoc).
#[derive(Debug, Clone)]
pub struct NavStep {
    /// 0-based BFS depth at which this decision was taken.
    pub depth: usize,
    /// The node whose children were scored.
    pub from: NodeId,
    /// Every child's conditional-MI score `(child, I)`, in declared order.
    pub scored: Vec<(NodeId, f64)>,
    /// The adaptive threshold `θ` applied at this node.
    pub threshold: f64,
    /// The children selected for expansion (`score ≥ θ`, capped at `b_max`).
    pub selected: Vec<NodeId>,
}

/// A retrieved leaf with its reasoning path + the information accumulated along
/// it. `path_gain = Σ I(R; nᵢ | Q, n₀…nᵢ₋₁)` over the selected ancestors — the
/// total entropy reduction the path achieved (paper Theorem 2).
#[derive(Debug, Clone)]
pub struct RetrievedLeaf {
    pub id: NodeId,
    pub path: Vec<NodeId>,
    pub content: String,
    pub path_gain: f64,
}

/// The result of a navigation: the retrieved leaves, the full decision trail
/// (explainability), and the total information gain.
#[derive(Debug, Clone)]
pub struct NavResult {
    pub leaves: Vec<RetrievedLeaf>,
    pub trail: Vec<NavStep>,
    /// `Σ` of all selected-node scores across the navigation — the cumulative
    /// `I(R; ·)`; the residual answer entropy is `H₀ − total_gain`.
    pub total_gain: f64,
}

/// `PIX-Navigate(Q, D, b_max, d_max)` — paper section 2.5.
///
/// Bounded breadth-first search with adaptive LLM-heuristic pruning. At each
/// level every frontier node's children are scored by `scorer`; children with
/// `score ≥ θ` (where `θ = theta_ratio · max_sibling_score`) survive, capped at
/// `b_max` by score (top-k). Leaves are collected with their reasoning path.
///
/// Guarantees (see module docs + tests): terminates in `≤ d_max` levels; every
/// retrieved leaf carries a reasoning path; the per-path cumulative information
/// gain is non-negative and monotone non-decreasing (⟺ residual entropy
/// non-increasing, Theorem 2).
pub fn pix_navigate(
    tree: &PixTree,
    query: &str,
    cfg: &NavConfig,
    scorer: &dyn RelevanceScorer,
) -> NavResult {
    let mut trail: Vec<NavStep> = Vec::new();
    let mut leaves: Vec<RetrievedLeaf> = Vec::new();
    let mut total_gain = 0.0_f64;

    // Frontier carries each live node together with the path + accumulated gain
    // that reached it, so a collected leaf inherits the right reasoning path.
    struct Frontier {
        id: NodeId,
        path: Vec<NodeId>,
        gain: f64,
    }

    let root = tree.root();
    // The root itself is the entry; if it is already a leaf, it is the answer.
    if root.is_leaf() {
        leaves.push(RetrievedLeaf {
            id: root.id,
            path: vec![root.id],
            content: root.content.clone().unwrap_or_default(),
            path_gain: 0.0,
        });
        return NavResult { leaves, trail, total_gain };
    }

    let mut frontier = vec![Frontier { id: root.id, path: vec![root.id], gain: 0.0 }];

    for depth in 0..cfg.d_max {
        if frontier.is_empty() {
            break;
        }
        let mut next: Vec<Frontier> = Vec::new();

        for f in &frontier {
            let node = &tree.nodes[&f.id];
            if node.is_leaf() {
                // A leaf reached before d_max — collect it (paper alg. line 5-6).
                leaves.push(RetrievedLeaf {
                    id: node.id,
                    path: f.path.clone(),
                    content: node.content.clone().unwrap_or_default(),
                    path_gain: f.gain,
                });
                continue;
            }

            // Score every child (paper alg. line 8): f_LLM ≈ I(R; child | Q, path).
            let scored: Vec<(NodeId, f64)> = node
                .children
                .iter()
                .map(|&c| {
                    let s = scorer.score(query, &tree.nodes[&c], &f.path).clamp(0.0, 1.0);
                    (c, s)
                })
                .collect();

            // Adaptive threshold θ = ρ · max score (paper alg. line 9).
            let max_score = scored.iter().map(|(_, s)| *s).fold(0.0_f64, f64::max);
            let threshold = cfg.theta_ratio * max_score;

            // Survivors: score ≥ θ, then top-k by score (paper alg. line 10).
            let mut survivors: Vec<(NodeId, f64)> =
                scored.iter().copied().filter(|(_, s)| *s >= threshold).collect();
            survivors.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            survivors.truncate(cfg.b_max);

            let selected: Vec<NodeId> = survivors.iter().map(|(c, _)| *c).collect();

            trail.push(NavStep {
                depth,
                from: node.id,
                scored: scored.clone(),
                threshold,
                selected: selected.clone(),
            });

            for (c, s) in survivors {
                total_gain += s;
                let mut path = f.path.clone();
                path.push(c);
                next.push(Frontier { id: c, path, gain: f.gain + s });
            }
        }

        frontier = next;
    }

    // Any nodes still on the frontier when d_max is hit are surfaced as
    // best-effort leaves (satisficing — paper section 3.2 bounded rationality), so a
    // query that bottoms out at the depth bound still returns its best path.
    for f in frontier {
        let node = &tree.nodes[&f.id];
        leaves.push(RetrievedLeaf {
            id: node.id,
            path: f.path,
            content: node
                .content
                .clone()
                .unwrap_or_else(|| node.summary.clone()),
            path_gain: f.gain,
        });
    }

    NavResult { leaves, trail, total_gain }
}

/// `drill` (paper section 5.3) — explicit descent into a named subtree. Navigates the
/// subtree rooted at `subtree_root` for `query`, reusing [`pix_navigate`]'s
/// guarantees within that subtree. Returns `None` if the id is unknown.
pub fn pix_drill(
    tree: &PixTree,
    subtree_root: NodeId,
    query: &str,
    cfg: &NavConfig,
    scorer: &dyn RelevanceScorer,
) -> Option<NavResult> {
    if !tree.nodes.contains_key(&subtree_root) {
        return None;
    }
    // Re-root a shallow view at `subtree_root` (the subtree is already a valid
    // tree by T1–T3 of the parent), then navigate it.
    let subtree = PixTree { nodes: tree.nodes.clone(), root: subtree_root };
    Some(pix_navigate(&subtree, query, cfg, scorer))
}

/// `trail` (paper Theorem 4) — render a navigation's reasoning path as an
/// ordered, human-readable breadcrumb of `title` choices with their scores, so
/// "why was this retrieved?" is answerable by construction.
pub fn pix_trail(tree: &PixTree, result: &NavResult) -> Vec<String> {
    result
        .trail
        .iter()
        .map(|step| {
            let from_title = tree
                .nodes
                .get(&step.from)
                .map(|n| n.title.as_str())
                .unwrap_or("?");
            let picks: Vec<String> = step
                .selected
                .iter()
                .map(|id| {
                    let title = tree.nodes.get(id).map(|n| n.title.as_str()).unwrap_or("?");
                    let score = step
                        .scored
                        .iter()
                        .find(|(c, _)| c == id)
                        .map(|(_, s)| *s)
                        .unwrap_or(0.0);
                    format!("{title} (I={score:.2})")
                })
                .collect();
            format!("@{} {from_title} → [{}]", step.depth, picks.join(", "))
        })
        .collect()
}

// ── v2.12.0 — indexing + the reference scorer ──────────────────────────

/// Why an outline could not be indexed into a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexError {
    /// The source had no content.
    Empty,
    /// The source had no `#`-prefixed headings to form a hierarchy.
    NoHeadings,
}

/// v2.12.0 — build a [`PixTree`] from a markdown-heading outline.
///
/// Deterministic and **embeddings-free**: each `#`-prefixed heading becomes a
/// node; a deeper heading is a child of the nearest shallower one; the body text
/// between a heading and the next becomes the node's `summary` (navigation
/// salience) and, for leaves, its uncompressed `content` (the answer source). A
/// synthetic root wraps the top-level headings, so a multi-section document is a
/// single tree (paper T1 unique root).
///
/// This is the structural indexer — the OSS reference. An LLM-summarising indexer
/// (paper section 2.2, `CR ∈ [0.05,0.15]`) is the production enhancement; both yield a
/// `PixTree` the navigator consumes identically.
pub fn index_markdown(text: &str) -> Result<PixTree, IndexError> {
    if text.trim().is_empty() {
        return Err(IndexError::Empty);
    }

    // Collect (level, title, body) sections in document order.
    struct Section {
        level: usize,
        title: String,
        body: String,
    }
    let mut sections: Vec<Section> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let hashes = trimmed.chars().take_while(|&c| c == '#').count();
        if hashes > 0 && trimmed[hashes..].starts_with(' ') {
            sections.push(Section {
                level: hashes,
                title: trimmed[hashes..].trim().to_string(),
                body: String::new(),
            });
        } else if let Some(s) = sections.last_mut() {
            if !line.trim().is_empty() {
                if !s.body.is_empty() {
                    s.body.push(' ');
                }
                s.body.push_str(line.trim());
            }
        }
        // Preamble before the first heading is ignored (the synthetic root holds it).
    }
    if sections.is_empty() {
        return Err(IndexError::NoHeadings);
    }

    // Synthetic root (id 0) + one node per section. Attach each section to the
    // nearest preceding ancestor with a strictly smaller level (stack walk).
    let mut nodes: Vec<PixNode> = vec![PixNode {
        id: 0,
        title: "root".to_string(),
        summary: "document root".to_string(),
        location: Location::default(),
        children: vec![],
        content: None,
    }];
    // stack of (level, node-index-in-`nodes`).
    let mut stack: Vec<(usize, usize)> = vec![(0, 0)];

    for (i, sec) in sections.iter().enumerate() {
        let id = (i + 1) as NodeId;
        while let Some(&(lvl, _)) = stack.last() {
            if lvl >= sec.level && stack.len() > 1 {
                stack.pop();
            } else {
                break;
            }
        }
        let parent_idx = stack.last().map(|&(_, idx)| idx).unwrap_or(0);
        nodes[parent_idx].children.push(id);

        let snippet: String = sec.body.chars().take(160).collect();
        nodes.push(PixNode {
            id,
            title: sec.title.clone(),
            summary: if snippet.is_empty() {
                sec.title.clone()
            } else {
                format!("{} — {}", sec.title, snippet)
            },
            location: Location::default(),
            children: vec![],
            // Provisional; internal nodes get `content: None` in the fix-up below.
            content: Some(sec.body.clone()),
        });
        stack.push((sec.level, nodes.len() - 1));
    }

    // Fix-up: a node with children is internal (paper — leaves carry content,
    // internal nodes carry only the navigational summary).
    let child_bearers: HashSet<NodeId> = nodes
        .iter()
        .filter(|n| !n.children.is_empty())
        .map(|n| n.id)
        .collect();
    for n in nodes.iter_mut() {
        if child_bearers.contains(&n.id) {
            n.content = None;
        }
    }

    PixTree::new(nodes, 0).map_err(|_| IndexError::NoHeadings)
}

/// v2.12.0 — a deterministic, embeddings-free reference [`RelevanceScorer`]:
/// the **lexical information scent** of a node for a query (paper section 3.3 — the
/// navigator follows the scent of summaries). The score is the fraction of query
/// terms present in the node's `title + summary`, floored at `epsilon` so every
/// branch stays navigable (mirrors the ε-floor of an LLM scorer).
///
/// This is *not* embedding similarity: it is exact lexical overlap over the
/// compressed navigational summaries. The production scorer is LLM-backed (it
/// reasons about whether a summary suggests it contains the answer); this OSS
/// reference is fully deterministic so the navigation is reproducible.
pub struct LexicalScorer {
    pub epsilon: f64,
}

impl Default for LexicalScorer {
    fn default() -> Self {
        LexicalScorer { epsilon: 0.05 }
    }
}

fn tokenize(s: &str) -> HashSet<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 2)
        .map(|w| w.to_lowercase())
        .collect()
}

impl RelevanceScorer for LexicalScorer {
    fn score(&self, query: &str, node: &PixNode, _path: &[NodeId]) -> f64 {
        let q = tokenize(query);
        if q.is_empty() {
            return self.epsilon;
        }
        let mut text = tokenize(&node.title);
        text.extend(tokenize(&node.summary));
        let hits = q.iter().filter(|t| text.contains(*t)).count();
        let coverage = hits as f64 / q.len() as f64;
        coverage.max(self.epsilon).min(1.0)
    }
}

/// v2.12.0 — build the scoring prompt for a node against a query.
/// Deterministic; the LLM is asked to *reason about the summary*, not to match
/// embeddings (paper section 2.3 — "reasoning about whether a section's summary
/// suggests it contains the answer").
pub fn build_score_prompt(query: &str, node: &PixNode) -> String {
    format!(
        "You are navigating a document by relevance, reading ONLY section \
         summaries (not full text). Rate from 0 to 100 how likely this section — \
         or one of its subsections — CONTAINS THE ANSWER to the query. Reply with \
         ONLY the integer.\n\nQuery: {query}\nSection title: {}\nSection summary: \
         {}\n\nScore (0-100):",
        node.title, node.summary
    )
}

/// v2.12.0 — parse an LLM score response to `[0, 1]`. Extracts the first
/// number; a value `> 1` is read as a 0–100 percentage, otherwise as a 0–1
/// fraction; both are clamped. A response with no number scores `0.0` (the LLM
/// declined — treat as uninformative). Deterministic.
pub fn parse_score(response: &str) -> f64 {
    let mut num = String::new();
    for ch in response.chars() {
        if ch.is_ascii_digit() || (ch == '.' && !num.contains('.')) {
            num.push(ch);
        } else if !num.is_empty() {
            break;
        }
    }
    let n: f64 = num.trim_matches('.').parse().unwrap_or(0.0);
    let frac = if n > 1.0 { n / 100.0 } else { n };
    frac.clamp(0.0, 1.0)
}

/// v2.12.0 — the PRODUCTION [`RelevanceScorer`]: an LLM estimates
/// `f_LLM(Q, summary) ≈ I(R; node | Q, path)` by reasoning about whether a
/// section's summary suggests it holds the answer (paper section 2.3).
///
/// The completion call is INJECTED (`complete`): it is async + backend-specific,
/// while the navigator's `score` is synchronous, so the dispatch handler runs
/// navigation under `spawn_blocking` with a blocking-call closure. The prompt +
/// parser are deterministic and unit-tested; [`LexicalScorer`] remains the
/// deterministic OSS reference the default handler uses.
pub struct LlmRelevanceScorer<F>
where
    F: Fn(&str) -> String,
{
    pub complete: F,
}

impl<F> RelevanceScorer for LlmRelevanceScorer<F>
where
    F: Fn(&str) -> String,
{
    fn score(&self, query: &str, node: &PixNode, _path: &[NodeId]) -> f64 {
        parse_score(&(self.complete)(&build_score_prompt(query, node)))
    }
}

/// v2.12.0 — resolve a node by a dotted path of (case-insensitive) titles
/// from the root, e.g. `["liability", "limitation"]`. Used by `drill` to locate
/// the named subtree. Returns the deepest node whose title chain matches; `None`
/// if the first segment is not a child of the root.
pub fn find_by_title_path(tree: &PixTree, titles: &[&str]) -> Option<NodeId> {
    let mut current = tree.root().id;
    for want in titles {
        let want_lc = want.trim().to_lowercase();
        if want_lc.is_empty() {
            continue;
        }
        let next = tree
            .children_of(current)
            .into_iter()
            .find(|n| n.title.to_lowercase() == want_lc)?;
        current = next.id;
    }
    if current == tree.root().id {
        None
    } else {
        Some(current)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: NodeId, title: &str, content: &str) -> PixNode {
        PixNode {
            id,
            title: title.into(),
            summary: format!("summary of {title}"),
            location: Location::default(),
            children: vec![],
            content: Some(content.into()),
        }
    }

    fn internal(id: NodeId, title: &str, children: Vec<NodeId>) -> PixNode {
        PixNode {
            id,
            title: title.into(),
            summary: format!("summary of {title}"),
            location: Location::default(),
            children,
            content: None,
        }
    }

    /// A small balanced tree:
    ///        0 (root)
    ///       / \
    ///      1   2
    ///     /\   /\
    ///    3 4  5 6   (leaves)
    fn sample_tree() -> PixTree {
        PixTree::new(
            vec![
                internal(0, "root", vec![1, 2]),
                internal(1, "left", vec![3, 4]),
                internal(2, "right", vec![5, 6]),
                leaf(3, "L3", "content-3"),
                leaf(4, "L4", "content-4"),
                leaf(5, "L5", "content-5"),
                leaf(6, "L6", "content-6"),
            ],
            0,
        )
        .expect("valid tree")
    }

    /// Scores nodes by how well their title matches a keyword in the query.
    /// Deterministic ⇒ the navigation is fully reproducible in tests.
    struct KeywordScorer;
    impl RelevanceScorer for KeywordScorer {
        fn score(&self, query: &str, node: &PixNode, _path: &[NodeId]) -> f64 {
            // The query names target leaf titles; a node on the way to a target
            // scores 1.0, everything else 0.1 (never 0, so the tree stays
            // navigable — mirrors the ε-floor of an LLM scorer).
            if query.contains(&node.title) {
                1.0
            } else if node.title == "left" && query.contains("L3") {
                1.0
            } else if node.title == "left" && query.contains("L4") {
                1.0
            } else if node.title == "right" && (query.contains("L5") || query.contains("L6")) {
                1.0
            } else {
                0.1
            }
        }
    }

    // ── Tree invariants (T1–T3) ──────────────────────────────────────────────

    #[test]
    fn valid_tree_builds() {
        let t = sample_tree();
        assert_eq!(t.len(), 7);
        assert_eq!(t.height(), 2);
        assert_eq!(t.root().title, "root");
    }

    #[test]
    fn missing_root_rejected() {
        let e = PixTree::new(vec![leaf(1, "a", "x")], 99).unwrap_err();
        assert_eq!(e, TreeError::MissingRoot(99));
    }

    #[test]
    fn dangling_child_rejected() {
        let e = PixTree::new(vec![internal(0, "r", vec![7])], 0).unwrap_err();
        assert_eq!(e, TreeError::DanglingChild(7));
    }

    #[test]
    fn shared_child_rejected_t2() {
        // Node 3 is a child of both 1 and 2 — not a tree.
        let e = PixTree::new(
            vec![
                internal(0, "r", vec![1, 2]),
                internal(1, "a", vec![3]),
                internal(2, "b", vec![3]),
                leaf(3, "c", "x"),
            ],
            0,
        )
        .unwrap_err();
        assert_eq!(e, TreeError::MultipleParents(3));
    }

    #[test]
    fn cycle_rejected_t3() {
        // 0 → 1 → 0 is a cycle.
        let e = PixTree::new(
            vec![internal(0, "r", vec![1]), internal(1, "a", vec![0])],
            0,
        )
        .unwrap_err();
        // 0 is revisited during the reachability walk.
        assert!(matches!(e, TreeError::Cycle(_) | TreeError::MultipleParents(0)));
    }

    #[test]
    fn disconnected_rejected_t1() {
        // Node 9 exists but is unreachable from root 0.
        let e = PixTree::new(
            vec![internal(0, "r", vec![1]), leaf(1, "a", "x"), leaf(9, "orphan", "y")],
            0,
        )
        .unwrap_err();
        assert_eq!(e, TreeError::NotConnected(9));
    }

    // ── Navigation guarantees ────────────────────────────────────────────────

    #[test]
    fn navigates_to_the_targeted_leaf() {
        let t = sample_tree();
        let r = pix_navigate(&t, "find L5", &NavConfig::default(), &KeywordScorer);
        // L5 is under "right"; the navigator should reach it.
        assert!(r.leaves.iter().any(|l| l.id == 5), "expected to retrieve L5: {:?}", r.leaves);
        let l5 = r.leaves.iter().find(|l| l.id == 5).unwrap();
        assert_eq!(l5.content, "content-5");
        assert_eq!(l5.path, vec![0, 2, 5], "reasoning path root→right→L5");
    }

    #[test]
    fn navigation_terminates_within_d_max() {
        let t = sample_tree();
        let cfg = NavConfig { b_max: 3, d_max: 2, theta_ratio: 0.5 };
        let r = pix_navigate(&t, "find L6", &cfg, &KeywordScorer);
        // Every trail step is at a depth strictly below d_max.
        assert!(r.trail.iter().all(|s| s.depth < cfg.d_max));
        // No retrieved path is longer than d_max + 1 (root + d_max edges).
        assert!(r.leaves.iter().all(|l| l.path.len() <= cfg.d_max + 1));
    }

    #[test]
    fn every_leaf_has_a_path() {
        // Theorem 4 — explainability by construction.
        let t = sample_tree();
        let r = pix_navigate(&t, "find L3", &NavConfig::default(), &KeywordScorer);
        assert!(!r.leaves.is_empty());
        for l in &r.leaves {
            assert_eq!(l.path.first(), Some(&0), "path starts at root");
            assert_eq!(l.path.last(), Some(&l.id), "path ends at the leaf");
        }
    }

    #[test]
    fn path_gain_is_monotone_nondecreasing() {
        // Theorem 2 — residual entropy H₀ − gain is non-increasing because the
        // per-path cumulative information gain only ever grows (scores ≥ 0).
        let t = sample_tree();
        let r = pix_navigate(&t, "find L4", &NavConfig::default(), &KeywordScorer);
        // Reconstruct the partial gains along the trail for the retrieved path
        // and assert they never decrease.
        for l in &r.leaves {
            // Walk the trail steps whose `from` is on the path, summing the score
            // of the selected next node; partial sums must be non-decreasing.
            let mut running = 0.0_f64;
            for w in l.path.windows(2) {
                let (from, to) = (w[0], w[1]);
                if let Some(step) = r.trail.iter().find(|s| s.from == from) {
                    let s = step.scored.iter().find(|(c, _)| *c == to).map(|(_, s)| *s).unwrap_or(0.0);
                    assert!(s >= 0.0, "every information score is non-negative");
                    let prev = running;
                    running += s;
                    assert!(running >= prev, "cumulative gain is monotone non-decreasing");
                }
            }
            // The leaf's recorded path_gain equals the reconstructed running sum.
            assert!((l.path_gain - running).abs() < 1e-9, "path_gain matches the trail");
        }
        // Total gain is the non-negative sum of all selected scores.
        assert!(r.total_gain >= 0.0);
    }

    #[test]
    fn branching_is_capped_at_b_max() {
        // A wide root with b_max = 2 must select at most 2 children per node.
        let t = PixTree::new(
            vec![
                internal(0, "root", vec![1, 2, 3, 4]),
                leaf(1, "a", "x"),
                leaf(2, "b", "x"),
                leaf(3, "c", "x"),
                leaf(4, "d", "x"),
            ],
            0,
        )
        .unwrap();
        let cfg = NavConfig { b_max: 2, d_max: 4, theta_ratio: 0.0 };
        let r = pix_navigate(&t, "anything", &cfg, &KeywordScorer);
        for step in &r.trail {
            assert!(step.selected.len() <= cfg.b_max, "b_max respected");
        }
    }

    #[test]
    fn drill_navigates_a_subtree() {
        let t = sample_tree();
        // Drill into the "right" subtree (id 2) looking for L6.
        let r = pix_drill(&t, 2, "find L6", &NavConfig::default(), &KeywordScorer).unwrap();
        assert!(r.leaves.iter().any(|l| l.id == 6));
        // The path is rooted at the subtree root, not the document root.
        let l6 = r.leaves.iter().find(|l| l.id == 6).unwrap();
        assert_eq!(l6.path.first(), Some(&2));
    }

    #[test]
    fn drill_unknown_subtree_is_none() {
        let t = sample_tree();
        assert!(pix_drill(&t, 999, "q", &NavConfig::default(), &KeywordScorer).is_none());
    }

    #[test]
    fn trail_renders_reasoning_path() {
        let t = sample_tree();
        let r = pix_navigate(&t, "find L5", &NavConfig::default(), &KeywordScorer);
        let trail = pix_trail(&t, &r);
        assert!(!trail.is_empty());
        // The first decision is taken at the root.
        assert!(trail[0].contains("root"), "trail starts at root: {trail:?}");
        // Scores are surfaced (explainability).
        assert!(trail.iter().any(|s| s.contains("I=")));
    }

    #[test]
    fn root_leaf_is_returned_directly() {
        let t = PixTree::new(vec![leaf(0, "only", "the-answer")], 0).unwrap();
        let r = pix_navigate(&t, "q", &NavConfig::default(), &KeywordScorer);
        assert_eq!(r.leaves.len(), 1);
        assert_eq!(r.leaves[0].content, "the-answer");
        assert!(r.trail.is_empty());
    }

    // ── v2.12.0 indexing + lexical scorer ────────────────────────────────────

    const DOC: &str = r#"
# Liability
General liability terms.
## Indemnification
The seller indemnifies the buyer against third-party claims.
## Limitation
Liability is capped at the contract value.
# Termination
## Notice
Either party may terminate with thirty days written notice.
"#;

    #[test]
    fn index_markdown_builds_a_valid_tree() {
        let t = index_markdown(DOC).expect("indexable");
        // root + Liability + Indemnification + Limitation + Termination + Notice = 6
        assert_eq!(t.len(), 6);
        // Liability (H1) hangs off root and has two H2 children.
        let liability = t
            .children_of(t.root().id)
            .into_iter()
            .find(|n| n.title == "Liability")
            .expect("Liability under root");
        assert_eq!(t.children_of(liability.id).len(), 2);
        // Internal nodes carry no content; leaves do.
        assert!(liability.content.is_none(), "internal node has no content");
        let indemn = t
            .children_of(liability.id)
            .into_iter()
            .find(|n| n.title == "Indemnification")
            .unwrap();
        assert!(indemn.content.as_deref().unwrap().contains("indemnifies"));
    }

    #[test]
    fn index_rejects_empty_and_headingless() {
        assert_eq!(index_markdown("   ").unwrap_err(), IndexError::Empty);
        assert_eq!(
            index_markdown("just prose, no headings").unwrap_err(),
            IndexError::NoHeadings
        );
    }

    #[test]
    fn lexical_scorer_is_embeddings_free_and_floored() {
        let s = LexicalScorer::default();
        let n = leaf(1, "Indemnification", "the seller indemnifies the buyer");
        // Query term present in title ⇒ high score.
        assert!(s.score("indemnification clause", &n, &[]) > 0.4);
        // No overlap ⇒ floored at epsilon, never zero (keeps the tree navigable).
        assert!((s.score("quantum chromodynamics", &n, &[]) - s.epsilon).abs() < 1e-9);
    }

    #[test]
    fn end_to_end_index_then_navigate_retrieves_the_right_section() {
        // The whole point: a real document, indexed, navigated embeddings-free
        // to the section that answers the query — with a reasoning path.
        let tree = index_markdown(DOC).unwrap();
        let r = pix_navigate(
            &tree,
            "what is the cap on liability limitation",
            &NavConfig::default(),
            &LexicalScorer::default(),
        );
        // The "Limitation" leaf (capped liability) must be among the results.
        let got: Vec<&str> = r.leaves.iter().map(|l| l.content.as_str()).collect();
        assert!(
            got.iter().any(|c| c.contains("capped at the contract value")),
            "expected the Limitation section, got {got:?}"
        );
        // And it came with an explainable trail.
        let trail = pix_trail(&tree, &r);
        assert!(trail.iter().any(|s| s.contains("Liability")));
    }

    #[test]
    fn find_by_title_path_locates_a_subtree() {
        let tree = index_markdown(DOC).unwrap();
        // "Liability" → "Limitation" resolves to the Limitation node.
        let id = find_by_title_path(&tree, &["liability", "limitation"]).unwrap();
        assert_eq!(tree.node(id).unwrap().title, "Limitation");
        // A single segment resolves the H1.
        let id2 = find_by_title_path(&tree, &["termination"]).unwrap();
        assert_eq!(tree.node(id2).unwrap().title, "Termination");
        // An unknown path is None.
        assert!(find_by_title_path(&tree, &["nonexistent"]).is_none());
    }

    // ── v2.12.0 LLM scorer (prompt + parse + trait impl) ─────────────────────

    #[test]
    fn parse_score_normalises_to_unit_interval() {
        assert!((parse_score("85") - 0.85).abs() < 1e-9); // 0–100 percentage
        assert!((parse_score("100") - 1.0).abs() < 1e-9);
        assert!((parse_score("0") - 0.0).abs() < 1e-9);
        assert!((parse_score("0.7") - 0.7).abs() < 1e-9); // already a fraction
        assert!((parse_score("Score: 42 / 100") - 0.42).abs() < 1e-9); // first number
        assert!((parse_score("the answer is likely here") - 0.0).abs() < 1e-9); // no number
        assert!((parse_score("250") - 1.0).abs() < 1e-9); // clamped
    }

    #[test]
    fn build_score_prompt_carries_query_and_summary() {
        let n = PixNode {
            id: 1,
            title: "Limitation".into(),
            summary: "liability is capped at contract value".into(),
            location: Location::default(),
            children: vec![],
            content: Some("full text".into()),
        };
        let p = build_score_prompt("what is the cap", &n);
        assert!(p.contains("what is the cap"), "carries the query");
        assert!(p.contains("Limitation"), "carries the title");
        assert!(p.contains("liability is capped at contract value"), "carries the summary");
        assert!(p.contains("ONLY the integer"), "instructs a bare-integer reply");
    }

    #[test]
    fn llm_scorer_uses_the_injected_completion() {
        // A mock "LLM" that returns 90 for the answer section, 5 otherwise —
        // proving the production scorer is a drop-in RelevanceScorer.
        let scorer = LlmRelevanceScorer {
            complete: |prompt: &str| {
                if prompt.contains("Limitation") { "90".to_string() } else { "5".to_string() }
            },
        };
        let hit = leaf(1, "Limitation", "capped");
        let miss = leaf(2, "Preamble", "intro");
        assert!((scorer.score("cap", &hit, &[]) - 0.90).abs() < 1e-9);
        assert!((scorer.score("cap", &miss, &[]) - 0.05).abs() < 1e-9);

        // It drives the navigator exactly like the lexical scorer.
        let tree = index_markdown(DOC).unwrap();
        let r = pix_navigate(&tree, "limitation cap", &NavConfig::default(), &scorer);
        assert!(r.leaves.iter().any(|l| l.content.contains("capped at the contract value")));
    }
}
