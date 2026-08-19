//! v2.12.0 — papers-as-proofs for the PIX·MDN·Memory trilogy.
//!
//! Each formal guarantee of the three papers becomes an **independent verifier**
//! in the Proof-Carrying-Code spirit (Necula 1997, and AXON's v2.4.0 PCC): a
//! checker that **re-derives the property from a witness without trusting the
//! algorithm that produced it**. The existing v2.4.0 PCC certifies IR-*static*
//! facts (compliance, effect rows, capabilities) by re-deriving from the IR
//! bundle; these guarantees are *algorithmic* (properties of a navigation, an
//! EPR vector, a memory update, a provenance derivation), so the witness is the
//! computation's output and the verifier re-checks the theorem on it.
//!
//! A forged witness is caught because the verifier recomputes the invariant —
//! it never believes the claim. This is the program's invariant #5 ("PCC = the
//! promise made verifiable") realized for the trilogy.

use crate::mdn::{Corpus, EprResult};
use crate::mdn_memory::History;
use crate::mdn_provenance::{is_sound, Annotated};
use crate::pix_navigator::NavResult;

/// The verdict of an independent verifier on one guarantee.
#[derive(Debug, Clone, PartialEq)]
pub struct ProofVerdict {
    /// The property class slug (e.g. `pix_navigation_soundness`).
    pub property: &'static str,
    /// Whether the witness satisfies the property under re-derivation.
    pub verified: bool,
    /// Human-readable reason — the violation when refuted, `"ok"` when verified.
    pub reason: String,
}

impl ProofVerdict {
    fn ok(property: &'static str) -> Self {
        ProofVerdict { property, verified: true, reason: "ok".into() }
    }
    fn refute(property: &'static str, reason: impl Into<String>) -> Self {
        ProofVerdict { property, verified: false, reason: reason.into() }
    }
}

/// **PIX navigation soundness** (paper section 2.5 + Theorems 2/4). Re-derives, from a
/// [`NavResult`], that the navigation honoured its guarantees: every retrieved
/// leaf's reasoning path has length `≤ d_max + 1` (convergence within the depth
/// bound), ends at the leaf it annotates (well-formed path, Theorem 4), and
/// carries a non-negative cumulative information gain (entropy non-increasing,
/// Theorem 2). The total gain is non-negative.
pub fn verify_pix_navigation(result: &NavResult, d_max: usize) -> ProofVerdict {
    const P: &str = "pix_navigation_soundness";
    for leaf in &result.leaves {
        if leaf.path.is_empty() {
            return ProofVerdict::refute(P, format!("leaf {} has an empty path", leaf.id));
        }
        if *leaf.path.last().unwrap() != leaf.id {
            return ProofVerdict::refute(P, format!("leaf {}'s path does not end at it", leaf.id));
        }
        if leaf.path.len() > d_max + 1 {
            return ProofVerdict::refute(
                P,
                format!("leaf {} path length {} exceeds d_max+1 ({})", leaf.id, leaf.path.len(), d_max + 1),
            );
        }
        if leaf.path_gain < 0.0 {
            return ProofVerdict::refute(P, format!("leaf {} has negative path gain", leaf.id));
        }
    }
    if result.total_gain < 0.0 {
        return ProofVerdict::refute(P, "negative total information gain");
    }
    ProofVerdict::ok(P)
}

/// **MDN signed-EPR validity** (paper Theorem 3, Perron–Frobenius). Re-derives,
/// from an [`EprResult`], the defining properties of the signed Epistemic
/// PageRank: `EPR⁺` and `EPR⁻` are each strictly-positive distributions
/// (sum ≈ 1), the net `EPR = EPR⁺ − λ·EPR⁻` is consistent per document, and the
/// net sums to `≈ 1 − λ` (the signed-reputation identity, section 2.3 WARNING).
pub fn verify_epr(result: &EprResult, lambda: f64, tol: f64) -> ProofVerdict {
    const P: &str = "mdn_signed_epr_validity";
    let sum_plus: f64 = result.epr_plus.values().sum();
    let sum_minus: f64 = result.epr_minus.values().sum();
    if (sum_plus - 1.0).abs() > tol {
        return ProofVerdict::refute(P, format!("EPR⁺ sums to {sum_plus}, not 1"));
    }
    if (sum_minus - 1.0).abs() > tol {
        return ProofVerdict::refute(P, format!("EPR⁻ sums to {sum_minus}, not 1"));
    }
    if result.epr_plus.values().any(|&v| v <= 0.0) {
        return ProofVerdict::refute(P, "EPR⁺ is not strictly positive (Perron–Frobenius)");
    }
    if result.epr_minus.values().any(|&v| v <= 0.0) {
        return ProofVerdict::refute(P, "EPR⁻ is not strictly positive (Perron–Frobenius)");
    }
    // Per-document consistency of the net reputation.
    for (id, &net) in &result.epr {
        let p = result.epr_plus.get(id).copied().unwrap_or(0.0);
        let m = result.epr_minus.get(id).copied().unwrap_or(0.0);
        if (net - (p - lambda * m)).abs() > tol {
            return ProofVerdict::refute(P, format!("net EPR for {id} ≠ EPR⁺ − λ·EPR⁻"));
        }
    }
    let sum_net: f64 = result.epr.values().sum();
    if (sum_net - (1.0 - lambda)).abs() > tol {
        return ProofVerdict::refute(P, format!("net EPR sums to {sum_net}, not 1−λ"));
    }
    ProofVerdict::ok(P)
}

/// **Memory locality + geometry-not-topology** (paper Def 3/4). Re-derives, from
/// a `(before, after)` corpus pair and the `history`, that the memory update
/// preserved topology (same documents, same edges + types) and changed ONLY the
/// weights of traversed edges (`Δω(r) ≠ 0 ⇒ r ∈ Edges(Π)`).
pub fn verify_memory_locality(before: &Corpus, after: &Corpus, history: &History) -> ProofVerdict {
    const P: &str = "memory_locality";
    if before.len() != after.len() {
        return ProofVerdict::refute(P, "document set changed (topology not preserved)");
    }
    // Index both edge multisets by (from, to, type) → weight.
    use std::collections::HashMap;
    let key = |e: &crate::mdn::Edge| (e.from, e.to, e.etype);
    let before_w: HashMap<_, f64> = before.edges().iter().map(|e| (key(e), e.weight)).collect();
    let after_w: HashMap<_, f64> = after.edges().iter().map(|e| (key(e), e.weight)).collect();
    if before_w.len() != after_w.len() || before_w.keys().any(|k| !after_w.contains_key(k)) {
        return ProofVerdict::refute(P, "edge set / types changed (topology not preserved)");
    }
    let traversed = history.traversed_edges();
    for (k, &wb) in &before_w {
        let wa = after_w[k];
        if (wa - wb).abs() > 1e-12 && !traversed.contains(&(k.0, k.1)) {
            return ProofVerdict::refute(
                P,
                format!("edge {:?}→{:?} weight changed but was never traversed (locality)", k.0, k.1),
            );
        }
    }
    ProofVerdict::ok(P)
}

/// **Provenance soundness** (paper Theorem 5). Wraps the independent
/// [`is_sound`] checker as a verdict: every path in the annotation's `Π` is
/// acyclic and corpus-valid (each node exists; each recorded edge exists with
/// its type), with `Π` non-empty.
pub fn verify_provenance(corpus: &Corpus, annotated: &Annotated) -> ProofVerdict {
    const P: &str = "provenance_soundness";
    if is_sound(corpus, annotated) {
        ProofVerdict::ok(P)
    } else {
        ProofVerdict::refute(P, "a provenance path is acyclic-invalid or corpus-invalid")
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdn::{epistemic_pagerank, Document, Edge, EdgeType, EprParams};
    use crate::mdn_memory::{apply_memory, MemoryParams, Outcome};
    use crate::mdn_provenance::{p_cite, Annotated, Path};
    use crate::pix_navigator::{index_markdown, pix_navigate, LexicalScorer, NavConfig, RetrievedLeaf};

    // ── PIX navigation soundness ─────────────────────────────────────────────

    #[test]
    fn verifies_a_real_navigation_and_refutes_a_forged_one() {
        let doc = "# A\n## B\nthe answer is in B.\n# C\n## D\nunrelated.";
        let tree = index_markdown(doc).unwrap();
        let cfg = NavConfig::default();
        let r = pix_navigate(&tree, "answer in B", &cfg, &LexicalScorer::default());
        assert!(verify_pix_navigation(&r, cfg.d_max).verified, "an honest navigation verifies");

        // Forge a result with a leaf path longer than the bound.
        let forged = NavResult {
            leaves: vec![RetrievedLeaf { id: 9, path: vec![0, 1, 2, 3, 4, 5, 6, 9], content: "x".into(), path_gain: 0.1 }],
            trail: vec![],
            total_gain: 0.1,
        };
        let v = verify_pix_navigation(&forged, 4);
        assert!(!v.verified && v.reason.contains("exceeds d_max"), "{v:?}");
    }

    #[test]
    fn refutes_a_navigation_with_negative_gain() {
        let forged = NavResult {
            leaves: vec![RetrievedLeaf { id: 1, path: vec![0, 1], content: "x".into(), path_gain: -0.5 }],
            trail: vec![],
            total_gain: -0.5,
        };
        assert!(!verify_pix_navigation(&forged, 4).verified);
    }

    // ── MDN signed-EPR validity ──────────────────────────────────────────────

    fn small_corpus() -> Corpus {
        Corpus::new(
            vec![
                Document { id: 1, title: "D1".into(), depth: 0, recency: 0.1, epistemic: "believe".into() },
                Document { id: 2, title: "D2".into(), depth: 1, recency: 0.5, epistemic: "believe".into() },
            ],
            vec![Edge { from: 2, to: 1, etype: EdgeType::Cite, weight: 0.9 }],
        )
        .unwrap()
    }

    #[test]
    fn verifies_a_real_epr_and_refutes_a_tampered_one() {
        let c = small_corpus();
        let params = EprParams { lambda: 0.5, ..EprParams::default() };
        let r = epistemic_pagerank(&c, &params);
        assert!(verify_epr(&r, params.lambda, 1e-6).verified, "an honest EPR verifies");

        // Tamper: inflate a net EPR so it no longer equals EPR⁺ − λ·EPR⁻.
        let mut tampered = r.clone();
        if let Some(v) = tampered.epr.get_mut(&1) {
            *v += 0.5;
        }
        let v = verify_epr(&tampered, params.lambda, 1e-6);
        assert!(!v.verified, "a tampered net EPR is caught: {v:?}");
    }

    // ── Memory locality ──────────────────────────────────────────────────────

    #[test]
    fn verifies_a_local_update_and_refutes_a_nonlocal_one() {
        let c = Corpus::new(
            vec![
                Document { id: 1, title: "D1".into(), depth: 0, recency: 0.1, epistemic: "believe".into() },
                Document { id: 2, title: "D2".into(), depth: 1, recency: 0.1, epistemic: "believe".into() },
                Document { id: 3, title: "D3".into(), depth: 1, recency: 0.1, epistemic: "believe".into() },
            ],
            vec![
                Edge { from: 1, to: 2, etype: EdgeType::Cite, weight: 0.5 },
                Edge { from: 1, to: 3, etype: EdgeType::Cite, weight: 0.5 },
            ],
        )
        .unwrap();
        let mut h = History::new();
        h.record(Outcome { query: "q".into(), path: vec![1, 2], score: 1.0, timestamp: 0 });
        h.record(Outcome { query: "q".into(), path: vec![1, 3], score: 0.0, timestamp: 0 });
        let after = apply_memory(&c, &h, &MemoryParams::default());
        assert!(verify_memory_locality(&c, &after, &h).verified, "an honest update is local");

        // Forge a non-local update: change edge 1→3 weight while the history that
        // we hand the verifier only traversed 1→2.
        let forged = Corpus::new(
            c.documents().into_iter().cloned().collect(),
            vec![
                Edge { from: 1, to: 2, etype: EdgeType::Cite, weight: 0.6 },
                Edge { from: 1, to: 3, etype: EdgeType::Cite, weight: 0.9 }, // tampered
            ],
        )
        .unwrap();
        let mut only_12 = History::new();
        only_12.record(Outcome { query: "q".into(), path: vec![1, 2], score: 1.0, timestamp: 0 });
        let v = verify_memory_locality(&c, &forged, &only_12);
        assert!(!v.verified && v.reason.contains("never traversed"), "{v:?}");
    }

    // ── Provenance soundness ─────────────────────────────────────────────────

    #[test]
    fn verifies_a_derived_annotation_and_refutes_a_forged_path() {
        let c = Corpus::new(
            vec![
                Document { id: 1, title: "D1".into(), depth: 0, recency: 0.1, epistemic: "believe".into() },
                Document { id: 2, title: "D2".into(), depth: 1, recency: 0.1, epistemic: "believe".into() },
            ],
            vec![Edge { from: 1, to: 2, etype: EdgeType::Cite, weight: 0.9 }],
        )
        .unwrap();
        let derived = p_cite(&c, &Annotated::atom("a", 1), &Annotated::atom("b", 2)).unwrap();
        assert!(verify_provenance(&c, &derived).verified, "a derived annotation is sound");

        // Forge an annotation claiming a non-existent 2→1 edge.
        let forged = Annotated {
            phi: "x".into(),
            provenance: vec![Path { nodes: vec![2, 1], edges: vec![EdgeType::Cite] }],
        };
        assert!(!verify_provenance(&c, &forged).verified, "a forged edge is caught");
    }
}
