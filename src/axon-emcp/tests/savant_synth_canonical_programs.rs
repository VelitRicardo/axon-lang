//! v2.42.0 — drift gate for the `savant` + `synth` docs.
//!
//! The canonical programs published in `knowledge/primitives/savant.md` and
//! `knowledge/primitives/synth.md` must round-trip through the same
//! `axon-frontend` pipeline the `axon` CLI uses — the "published grammar MUST
//! compile" discipline. A doc example that does not compile is a lie the corpus
//! must never ship.

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

/// The published savant.md example: a fully-specified governed research loop
/// (domain, cognition, a resolvable memory backend, a bounded budget, a typed
/// mandate).
#[test]
fn savant_doc_example_compiles() {
    let src = r#"
type FormalReport { summary: String }
memory ResearchStore { store: persistent }

savant DeepTechAnalyst {
    domain: "Quantum Computing Error Correction"
    cognition {
        depth: hyper
        entropic_threshold: 0.001
        divergence: high
    }
    memory {
        backend: ResearchStore
        corpus_graph: true
        isolation_level: strict
    }
    budget {
        max_iterations: 50000
    }
    mandate resolve_decoherence {
        objective: "Synthesise 2024-2026 topological-code papers and propose 3 architectures."
        output: FormalReport
    }
}
"#;
    must_compile("savant/canonical", src);
}

/// A minimal savant: domain + mandatory budget + one mandate (the smallest
/// governed loop that compiles).
#[test]
fn minimal_savant_compiles() {
    let src = r#"
type Report { summary: String }
savant Scout {
    domain: "emerging battery chemistries"
    budget { max_iterations: 1000 }
    mandate survey { objective: "map the field" output: Report }
}
"#;
    must_compile("savant/minimal", src);
}

/// The published synth.md example: a medium-risk, reviewed, WASM-sandboxed
/// Rust tool-synthesis policy.
#[test]
fn synth_doc_example_compiles() {
    let src = r#"
synth Toolsmith {
    target: "parse geospatial datasets the corpus references"
    risk: medium
    language: rust
    sandbox: wasm
    review: required
    max_lines: 400
}
"#;
    must_compile("synth/canonical", src);
}
