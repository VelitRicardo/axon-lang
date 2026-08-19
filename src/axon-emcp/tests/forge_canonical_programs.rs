//! v2.41.0 — drift gate for the real `forge` Directed Creative Synthesis doc.
//!
//! The canonical program published in `knowledge/primitives/forge.md` (and the
//! README IV) must round-trip through the same `axon-frontend` pipeline the
//! `axon` CLI uses — the "published grammar MUST compile" discipline, applied to
//! the primitive that was fiction until v2.41.0.

use axon_emcp::compiler_pipeline::{run, Outcome};

fn must_compile(label: &str, source: &str) {
    match run(source, label) {
        Outcome::Ok { .. } => {}
        Outcome::Err {
            stage,
            errors,
            warnings,
        } => panic!(
            "{label}: expected well-formed program, got {stage:?} failure:\n\
             errors   = {errors:#?}\n\
             warnings = {warnings:#?}\n\
             source   = {source}"
        ),
    }
}

/// The published forge.md / README IV example: a transformational synthesis
/// with a novelty target, a GoldenRatio anchor, depth 4 and 7 branches.
#[test]
fn forge_doc_example_compiles() {
    let src = r#"
anchor GoldenRatio {
    require: aesthetic_harmony
    confidence_floor: 0.70
}

flow CreateVisualConcept(brief: String) -> Visual {
    forge Artwork(seed: "aurora borealis over ancient ruins") -> Visual {
        mode:        transformational
        novelty:     0.85
        constraints: GoldenRatio
        depth:       4
        branches:    7
    }
}
"#;
    must_compile("forge/canonical", src);
}

/// A minimal forge (defaults for mode/depth/branches, no anchor) compiles — the
/// novelty floor is the only gate.
#[test]
fn minimal_forge_compiles() {
    let src = r#"
flow Riff() -> Concept {
    forge Idea(seed: "a city that remembers its own history") -> Concept {
        novelty: 0.6
    }
}
"#;
    must_compile("forge/minimal", src);
}
