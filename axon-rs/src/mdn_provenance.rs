//! v2.12.0 — MDN provenance logic (paper `paper_multi_document.md` section 3.2).
//!
//! A faithful implementation of the **provenance calculus**: every derived claim
//! carries the corpus path(s) that justify it. A *provenance-annotated formula*
//! `φ@Π` pairs a proposition `φ` with a non-empty set `Π` of valid acyclic paths
//! through the corpus graph (paper Def 10). New annotations are derived only via
//! the three composition rules, each with its side conditions:
//!
//! - **P-CITE** — `φ@(D₀), ψ@(D₁), (D₀,D₁,cite)∈R ⊢ (φ→ψ)@(D₀,cite,D₁)`.
//! - **P-EXTEND** — `φ@π₁, (Dₖ,D_{k+1},t)∈R ⊢ φ@(π₁·t·D_{k+1})`, acyclicity preserved.
//! - **P-CORROBORATE** — `φ@{π₁}, φ@{π₂}` sharing only the origin `⊢ φ@{π₁,π₂}`
//!   (independent lines of evidence; the confidence boost is a separate
//! quantitative concern handled by EPR section 2.3 — the rule is purely logical).
//!
//! **Theorem 5 (Provenance Soundness):** every `π ∈ Π` of a derived annotation is
//! a valid acyclic path in the corpus. [`is_sound`] is the independent checker.
//!
//! The S4_MDN modal logic (section 3.1) and its soundness/completeness are meta-logical
//! and land as a PCC property class in v2.12.0; this module is the concrete,
//! runtime-relevant calculus over corpus paths.

use crate::mdn::{Corpus, DocId, EdgeType};
use std::collections::HashSet;

/// A provenance path `π = (D₀, r₁, D₁, …, rₖ, Dₖ)` (paper Def 7/10): an acyclic
/// alternating sequence of documents and the typed edges between them. `edges`
/// has length `nodes.len() - 1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    pub nodes: Vec<DocId>,
    pub edges: Vec<EdgeType>,
}

impl Path {
    /// The single-node path `(D)` — an atomic provenance annotation.
    pub fn single(d: DocId) -> Self {
        Path { nodes: vec![d], edges: Vec::new() }
    }

    /// The origin `D₀`.
    pub fn origin(&self) -> DocId {
        self.nodes[0]
    }

    /// The endpoint `Dₖ`.
    pub fn endpoint(&self) -> DocId {
        *self.nodes.last().expect("a path has at least one node")
    }

    /// `nodes(π)` as a set.
    pub fn node_set(&self) -> HashSet<DocId> {
        self.nodes.iter().copied().collect()
    }

    /// Whether the path is acyclic (no revisited node) and well-formed
    /// (`|edges| = |nodes| - 1`).
    pub fn is_acyclic(&self) -> bool {
        self.edges.len() + 1 == self.nodes.len()
            && self.node_set().len() == self.nodes.len()
    }
}

/// A provenance-annotated formula `φ@Π` (paper Def 10): a proposition witnessed
/// by a non-empty set of paths. Multiple paths represent independent
/// corroborating evidence for the same `φ`.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotated {
    pub phi: String,
    pub provenance: Vec<Path>,
}

impl Annotated {
    /// The atomic annotation `φ@(D)` — `φ` asserted by a single document.
    pub fn atom(phi: impl Into<String>, d: DocId) -> Self {
        Annotated { phi: phi.into(), provenance: vec![Path::single(d)] }
    }
}

/// Whether a `cite` edge `D₀ → D₁` exists in the corpus.
fn has_cite(corpus: &Corpus, from: DocId, to: DocId) -> bool {
    corpus
        .edges()
        .iter()
        .any(|e| e.from == from && e.to == to && e.etype == EdgeType::Cite)
}

/// The type of the first edge `from → to`, if any.
fn edge_type_between(corpus: &Corpus, from: DocId, to: DocId) -> Option<EdgeType> {
    corpus
        .edges()
        .iter()
        .find(|e| e.from == from && e.to == to)
        .map(|e| e.etype)
}

/// **P-CITE** (paper section 3.2). From `φ@(D₀)`, `ψ@(D₁)`, and a citation `D₀ → D₁`,
/// derive `(φ → ψ)@(D₀, cite, D₁)`. Both premises must be atomic single-document
/// annotations. Returns `None` if either premise is non-atomic or no cite edge
/// joins them.
pub fn p_cite(corpus: &Corpus, phi_at_d0: &Annotated, psi_at_d1: &Annotated) -> Option<Annotated> {
    let d0_path = single_atom(phi_at_d0)?;
    let d1_path = single_atom(psi_at_d1)?;
    let d0 = d0_path.origin();
    let d1 = d1_path.origin();
    if !has_cite(corpus, d0, d1) {
        return None;
    }
    Some(Annotated {
        phi: format!("({} → {})", phi_at_d0.phi, psi_at_d1.phi),
        provenance: vec![Path { nodes: vec![d0, d1], edges: vec![EdgeType::Cite] }],
    })
}

/// **P-EXTEND** (paper section 3.2). Extend `φ@π₁` along an edge `(Dₖ, D_{k+1}, t) ∈ R`
/// to `φ@(π₁ · t · D_{k+1})`, where `Dₖ` is the endpoint of `π₁`. The extension
/// requires `D_{k+1} ∉ nodes(π₁)` (acyclicity preserved). `φ` is unchanged. The
/// premise must carry a single path.
pub fn p_extend(corpus: &Corpus, phi: &Annotated, next: DocId) -> Option<Annotated> {
    if phi.provenance.len() != 1 {
        return None;
    }
    let pi = &phi.provenance[0];
    let dk = pi.endpoint();
    let t = edge_type_between(corpus, dk, next)?;
    if pi.node_set().contains(&next) {
        return None; // would create a cycle.
    }
    let mut nodes = pi.nodes.clone();
    let mut edges = pi.edges.clone();
    nodes.push(next);
    edges.push(t);
    Some(Annotated { phi: phi.phi.clone(), provenance: vec![Path { nodes, edges }] })
}

/// **P-CORROBORATE** (paper section 3.2). Combine `φ@{π₁}` and `φ@{π₂}` — same `φ`,
/// paths sharing **only the origin** `D₀` — into `φ@{π₁, π₂}`. This is a purely
/// logical combination of independent evidence chains; the confidence boost is
/// handled separately by the EPR scoring (section 2.3), not here. Returns `None` if the
/// formulas differ, the premises are non-singleton, the origins differ, or the
/// paths share an intermediate node (not independent).
pub fn p_corroborate(phi_a: &Annotated, phi_b: &Annotated) -> Option<Annotated> {
    if phi_a.phi != phi_b.phi {
        return None;
    }
    if phi_a.provenance.len() != 1 || phi_b.provenance.len() != 1 {
        return None;
    }
    let p1 = &phi_a.provenance[0];
    let p2 = &phi_b.provenance[0];
    if p1.origin() != p2.origin() {
        return None;
    }
    let shared: HashSet<DocId> = p1.node_set().intersection(&p2.node_set()).copied().collect();
    if shared.len() != 1 {
        return None; // they must share ONLY the origin.
    }
    Some(Annotated { phi: phi_a.phi.clone(), provenance: vec![p1.clone(), p2.clone()] })
}

/// **Theorem 5 — Provenance Soundness.** A provenance annotation is sound iff `Π`
/// is non-empty and every path in it is acyclic and corpus-valid: each node
/// exists, and each consecutive pair is joined by an edge of the recorded type.
/// This is the *independent* checker — it re-verifies a derivation without
/// trusting how it was produced (the same spirit as PCC).
pub fn is_sound(corpus: &Corpus, ann: &Annotated) -> bool {
    if ann.provenance.is_empty() {
        return false;
    }
    ann.provenance.iter().all(|p| path_is_valid(corpus, p))
}

fn path_is_valid(corpus: &Corpus, p: &Path) -> bool {
    if !p.is_acyclic() {
        return false;
    }
    if p.nodes.iter().any(|n| corpus.document(*n).is_none()) {
        return false;
    }
    // Each recorded edge must exist between consecutive nodes with that type.
    for (i, &etype) in p.edges.iter().enumerate() {
        let (from, to) = (p.nodes[i], p.nodes[i + 1]);
        let exists = corpus
            .edges()
            .iter()
            .any(|e| e.from == from && e.to == to && e.etype == etype);
        if !exists {
            return false;
        }
    }
    true
}

/// A premise that must be an atomic single-document annotation `φ@(D)`.
fn single_atom(ann: &Annotated) -> Option<&Path> {
    match ann.provenance.as_slice() {
        [p] if p.nodes.len() == 1 => Some(p),
        _ => None,
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdn::{Document, Edge};

    fn doc(id: DocId) -> Document {
        Document { id, title: format!("D{id}"), depth: 0, recency: 0.5, epistemic: "believe".into() }
    }
    fn edge(from: DocId, to: DocId, etype: EdgeType) -> Edge {
        Edge { from, to, etype, weight: 0.9 }
    }

    /// 1 -cite-> 2 -cite-> 3 ; 1 -cite-> 4 ; 4 -cite-> 3 (a diamond to 3).
    fn corpus() -> Corpus {
        Corpus::new(
            vec![doc(1), doc(2), doc(3), doc(4)],
            vec![
                edge(1, 2, EdgeType::Cite),
                edge(2, 3, EdgeType::Cite),
                edge(1, 4, EdgeType::Cite),
                edge(4, 3, EdgeType::Cite),
            ],
        )
        .unwrap()
    }

    #[test]
    fn path_acyclicity_is_detected() {
        let good = Path { nodes: vec![1, 2, 3], edges: vec![EdgeType::Cite, EdgeType::Cite] };
        assert!(good.is_acyclic());
        let cyclic = Path { nodes: vec![1, 2, 1], edges: vec![EdgeType::Cite, EdgeType::Cite] };
        assert!(!cyclic.is_acyclic());
        let malformed = Path { nodes: vec![1, 2, 3], edges: vec![EdgeType::Cite] };
        assert!(!malformed.is_acyclic(), "edges must be nodes-1");
    }

    #[test]
    fn p_cite_derives_an_implication_with_the_citation_path() {
        let c = corpus();
        let phi = Annotated::atom("rain", 1);
        let psi = Annotated::atom("wet", 2);
        let derived = p_cite(&c, &phi, &psi).expect("cite edge 1→2 exists");
        assert_eq!(derived.phi, "(rain → wet)");
        assert_eq!(derived.provenance[0].nodes, vec![1, 2]);
        assert!(is_sound(&c, &derived), "the derived annotation is sound");
    }

    #[test]
    fn p_cite_rejects_when_no_citation_joins_the_documents() {
        let c = corpus();
        // No cite 2→1.
        assert!(p_cite(&c, &Annotated::atom("a", 2), &Annotated::atom("b", 1)).is_none());
    }

    #[test]
    fn p_extend_grows_the_path_and_preserves_acyclicity() {
        let c = corpus();
        // φ@(1) → extend along 1→2 → extend along 2→3.
        let a1 = Annotated::atom("claim", 1);
        let a2 = p_extend(&c, &a1, 2).expect("edge 1→2");
        assert_eq!(a2.provenance[0].nodes, vec![1, 2]);
        let a3 = p_extend(&c, &a2, 3).expect("edge 2→3");
        assert_eq!(a3.provenance[0].nodes, vec![1, 2, 3]);
        assert!(is_sound(&c, &a3));
    }

    #[test]
    fn p_extend_refuses_a_cycle() {
        let c = Corpus::new(
            vec![doc(1), doc(2)],
            vec![edge(1, 2, EdgeType::Cite), edge(2, 1, EdgeType::Cite)],
        )
        .unwrap();
        let a = p_extend(&c, &Annotated::atom("x", 1), 2).unwrap(); // 1→2
        // Extending 1→2 back to 1 would revisit node 1 → refused.
        assert!(p_extend(&c, &a, 1).is_none(), "acyclicity preserved");
    }

    #[test]
    fn p_corroborate_combines_independent_evidence_sharing_only_the_origin() {
        let c = corpus();
        // Two independent paths 1→2→3 and 1→4→3 for the SAME φ. They share only
        // the origin (1) ... but BOTH end at 3, so node 3 is shared too → NOT
        // independent by the rule. Build paths that share only the origin.
        let p_left = Annotated {
            phi: "thesis".into(),
            provenance: vec![Path { nodes: vec![1, 2], edges: vec![EdgeType::Cite] }],
        };
        let p_right = Annotated {
            phi: "thesis".into(),
            provenance: vec![Path { nodes: vec![1, 4], edges: vec![EdgeType::Cite] }],
        };
        let both = p_corroborate(&p_left, &p_right).expect("share only origin 1");
        assert_eq!(both.provenance.len(), 2, "two independent evidence chains");
        assert!(is_sound(&c, &both));
    }

    #[test]
    fn p_corroborate_rejects_non_independent_or_mismatched() {
        let c = corpus();
        // Paths 1→2→3 and 1→4→3 share BOTH origin 1 and endpoint 3 → not
        // independent (intersection {1,3} ≠ {1}).
        let left = Annotated {
            phi: "t".into(),
            provenance: vec![Path { nodes: vec![1, 2, 3], edges: vec![EdgeType::Cite, EdgeType::Cite] }],
        };
        let right = Annotated {
            phi: "t".into(),
            provenance: vec![Path { nodes: vec![1, 4, 3], edges: vec![EdgeType::Cite, EdgeType::Cite] }],
        };
        assert!(p_corroborate(&left, &right).is_none(), "share node 3 → not independent");
        // Different formulas cannot corroborate.
        let _ = c;
        assert!(
            p_corroborate(&Annotated::atom("a", 1), &Annotated::atom("b", 1)).is_none()
        );
    }

    #[test]
    fn is_sound_rejects_a_forged_edge() {
        let c = corpus();
        // A hand-built annotation claiming a 1→3 edge that doesn't exist.
        let forged = Annotated {
            phi: "x".into(),
            provenance: vec![Path { nodes: vec![1, 3], edges: vec![EdgeType::Cite] }],
        };
        assert!(!is_sound(&c, &forged), "no 1→3 cite edge exists");
        // Empty provenance is unsound (Π must be non-empty).
        assert!(!is_sound(&c, &Annotated { phi: "x".into(), provenance: vec![] }));
    }
}
