//! v1.20.0 — Cross-stack drift gate (Rust side).
//!
//! D7 ratified 2026-05-10: byte-identical error lists across Python
//! and Rust frontends. The drift gate locks this in CI by having
//! both stacks parse the same canonical corpus and assert the same
//! expected counts (error count, suggest-hint count, salvaged
//! declaration count).
//!
//! The corpus + golden snapshot lives in
//! `tests/fixtures/drift_gate/corpus.json` at the repository
//! root. The Python pack `tests/test_fase28_drift_gate.py` reads
//! the same file. If at any future point the two stacks disagree
//! on what input X produces, exactly one of the two test packs
//! fails — the drift is caught at PR review time, not at adopter-
//! bug-report time.
//!
//! When extending: edit the corpus.json + run BOTH stacks against
//! the new entry to confirm parity before committing. The
//! `scripts/regen_drift_corpus.py` helper regenerates
//! expected counts from Python; Rust must agree with whatever the
//! helper writes — if Rust drifts, this test fails.

use std::fs;
use std::path::PathBuf;

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Some fields may not be used by every assertion.
struct CorpusEntry {
    name: String,
    source: String,
    expected_error_count: usize,
    expected_suggest_count: usize,
    expected_decl_count: usize,
}

/// Walk up from the test's source directory to find the repository
/// root. We anchor on the workspace marker (`axon-frontend` sibling
/// + `tests/fixtures/drift_gate/corpus.json` existing).
///
/// `cargo test` runs with `CARGO_MANIFEST_DIR` set to the crate
/// root (`axon-frontend/`); the corpus lives at
/// `<repo-root>/tests/fixtures/...` — exactly one directory up.
fn corpus_path() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let repo_root = PathBuf::from(manifest_dir)
        .parent()
        .expect("CARGO_MANIFEST_DIR has a parent")
        .to_path_buf();
    repo_root
        .join("tests")
        .join("fixtures")
        .join("drift_gate")
        .join("corpus.json")
}

fn load_corpus() -> Vec<CorpusEntry> {
    let path = corpus_path();
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("failed to parse corpus.json: {e}"))
}

fn parse_one(source: &str) -> (usize, usize, usize) {
    let tokens = Lexer::new(source, "<drift-gate>")
        .tokenize()
        .expect("lex");
    let result = Parser::new(tokens).parse_with_recovery();
    let n_err = result.errors.len();
    let n_suggest = result
        .errors
        .iter()
        .filter(|e| e.message.contains("Did you mean"))
        .count();
    let n_decl = result.program.declarations.len();
    (n_err, n_suggest, n_decl)
}

#[test]
fn corpus_loads_and_has_required_fields() {
    let corpus = load_corpus();
    assert!(corpus.len() >= 10, "corpus too small: {}", corpus.len());

    // Spot-check that each entry has a non-empty name.
    for entry in &corpus {
        assert!(!entry.name.is_empty());
    }
}

#[test]
fn rust_matches_corpus_error_counts() {
    let corpus = load_corpus();
    let mut failures: Vec<String> = Vec::new();
    for entry in &corpus {
        let (n_err, _, _) = parse_one(&entry.source);
        if n_err != entry.expected_error_count {
            failures.push(format!(
                "{}: errors={} != expected={}\n  source: {:?}",
                entry.name, n_err, entry.expected_error_count, entry.source
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "drift-gate error-count mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn rust_matches_corpus_suggest_counts() {
    let corpus = load_corpus();
    let mut failures: Vec<String> = Vec::new();
    for entry in &corpus {
        let (_, n_suggest, _) = parse_one(&entry.source);
        if n_suggest != entry.expected_suggest_count {
            failures.push(format!(
                "{}: suggest={} != expected={}\n  source: {:?}",
                entry.name, n_suggest, entry.expected_suggest_count, entry.source
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "drift-gate suggest-count mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn rust_matches_corpus_decl_counts() {
    let corpus = load_corpus();
    let mut failures: Vec<String> = Vec::new();
    for entry in &corpus {
        let (_, _, n_decl) = parse_one(&entry.source);
        if n_decl != entry.expected_decl_count {
            failures.push(format!(
                "{}: decls={} != expected={}\n  source: {:?}",
                entry.name, n_decl, entry.expected_decl_count, entry.source
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "drift-gate decl-count mismatches:\n{}",
        failures.join("\n")
    );
}
