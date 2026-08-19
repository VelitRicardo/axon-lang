//! v1.28.0 — Tests for the effective-dialect resolver.
//!
//! Pure 2-input function `resolve_effective_dialect(transport_dialect,
//! has_algebraic_stream_effect) -> String`. Closed-catalog output: one
//! of `"axon"`, `"openai"`, `"anthropic"`. Never empty under valid input.
//!
//! Truth table (6 cells exhaustive):
//!
//! | transport_dialect | algebraic | output     | rule                |
//! |-------------------|-----------|------------|---------------------|
//! | "axon"            | *         | "axon"     | Rule 1 (explicit)   |
//! | "openai"          | *         | "openai"   | Rule 1 (explicit)   |
//! | "anthropic"       | *         | "anthropic"| Rule 1 (explicit)   |
//! | ""                | true      | "openai"   | Rule 2 (Q1 default) |
//! | ""                | false     | "axon"     | Rule 3 (Q1 default) |

use axon_frontend::type_checker::resolve_effective_dialect;

// ─── section 1 — Explicit dialect wins (Rule 1) ────────────────────────────

#[test]
fn s1_explicit_axon_wins_regardless_of_algebraic_predicate() {
    assert_eq!(resolve_effective_dialect("axon", true), "axon");
    assert_eq!(resolve_effective_dialect("axon", false), "axon");
}

#[test]
fn s1_explicit_openai_wins_regardless_of_algebraic_predicate() {
    assert_eq!(resolve_effective_dialect("openai", true), "openai");
    assert_eq!(resolve_effective_dialect("openai", false), "openai");
}

#[test]
fn s1_explicit_kimi_wins_regardless_of_algebraic_predicate() {
    assert_eq!(resolve_effective_dialect("kimi", true), "kimi");
    assert_eq!(resolve_effective_dialect("kimi", false), "kimi");
}

#[test]
fn s1_explicit_glm_wins_regardless_of_algebraic_predicate() {
    assert_eq!(resolve_effective_dialect("glm", true), "glm");
    assert_eq!(resolve_effective_dialect("glm", false), "glm");
}

#[test]
fn s1_explicit_anthropic_wins_regardless_of_algebraic_predicate() {
    assert_eq!(resolve_effective_dialect("anthropic", true), "anthropic");
    assert_eq!(resolve_effective_dialect("anthropic", false), "anthropic");
}

// ─── section 2 — Q1 algebraic-effect default → openai (Rule 2) ─────────────

#[test]
fn s2_q1_algebraic_default_openai() {
    assert_eq!(
        resolve_effective_dialect("", true),
        "openai",
        "33.z.k.c Q1: tool with `effects: <stream:<policy>>` defaults \
         to openai dialect — LLM-streaming ecosystem expectation"
    );
}

// ─── section 3 — Q1 type-annotation default → axon (Rule 3) ────────────────

#[test]
fn s3_q1_type_annotation_default_axon() {
    assert_eq!(
        resolve_effective_dialect("", false),
        "axon",
        "33.z.k.c Q1: type-annotation-only stream flow (no tool effect) \
         defaults to axon dialect — W3C named-events baseline"
    );
}

// ─── section 4 — Total function: every output is in closed catalog ─────────

#[test]
fn s4_total_function_closed_catalog() {
    use axon_frontend::parser::AXONENDPOINT_TRANSPORT_DIALECTS;
    let catalog: std::collections::HashSet<&str> =
        AXONENDPOINT_TRANSPORT_DIALECTS.iter().copied().collect();
    for explicit in ["", "axon", "openai", "kimi", "glm", "anthropic"] {
        for algebraic in [false, true] {
            let result = resolve_effective_dialect(explicit, algebraic);
            assert!(
                catalog.contains(result.as_str()),
                "resolve_effective_dialect({explicit:?}, {algebraic}) \
                 = {result:?} — NOT in closed catalog {catalog:?}"
            );
        }
    }
}

// ─── section 5 — Defensive: empty dialect + no algebraic → axon ────────────

#[test]
fn s5_defensive_no_signal_returns_axon() {
    // Caller violated the precondition (wire is supposed to be SSE
    // but neither dialect nor algebraic signal present). Returns
    // axon defensively rather than panicking.
    assert_eq!(resolve_effective_dialect("", false), "axon");
}
