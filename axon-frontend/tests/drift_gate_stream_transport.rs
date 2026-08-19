//! v1.21.0 — Cross-stack drift gate (Rust side).
//!
//! D7 ratified 2026-05-10 (extends v1.20.0's D7): byte-identical
//! parse results for the new `transport` + `keepalive` fields across
//! the Python axon-lang frontend and the Rust axon-frontend.
//!
//! The corpus + golden snapshot lives at
//! `tests/fixtures/transport/corpus.json` at the repo root.
//! Each entry pins (source, expected_parse_ok, [expected_transport,
//! expected_keepalive] | expected_error_contains).
//!
//! The Python pack `tests/test_fase30_transport_parser.py`
//! parametrizes over the same JSON. If at any future point the two
//! stacks diverge on what input X produces, exactly one of the two
//! test packs fails — drift caught at PR-review time.

use std::fs;
use std::path::PathBuf;

use axon_frontend::ast::Declaration;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct CorpusEntry {
    name: String,
    source: String,
    expected_parse_ok: bool,
    #[serde(default)]
    expected_transport: String,
    #[serde(default)]
    expected_keepalive: String,
    #[serde(default)]
    expected_error_contains: String,
}

/// Resolve the canonical corpus path. The Rust integration test runs
/// with `CARGO_MANIFEST_DIR = axon-frontend/`; the corpus lives one
/// directory up at `<repo-root>/tests/fixtures/transport/corpus.json`.
fn corpus_path() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .expect("CARGO_MANIFEST_DIR has a parent")
        .join("tests")
        .join("fixtures")
        .join("transport")
        .join("corpus.json")
}

fn load_corpus() -> Vec<CorpusEntry> {
    let path = corpus_path();
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("failed to parse corpus.json: {e}"))
}

/// Parse a single-axonendpoint program and return the AST node, or
/// the parser error message on failure (we don't `panic` on parse
/// errors — that's the negative-test path we want to assert against).
fn parse_axonendpoint(
    source: &str,
) -> Result<(String, String), String> {
    let tokens = Lexer::new(source, "<drift-gate>")
        .tokenize()
        .map_err(|e| format!("lex error: {e:?}"))?;
    let program = Parser::new(tokens).parse().map_err(|e| e.message)?;
    let decl = program
        .declarations
        .first()
        .ok_or_else(|| "no declarations in program".to_string())?;
    match decl {
        Declaration::AxonEndpoint(ae) => {
            Ok((ae.transport.clone(), ae.keepalive.clone()))
        }
        other => Err(format!(
            "expected AxonEndpoint, got {other:?}"
        )),
    }
}

#[test]
fn corpus_loads_and_has_required_fields() {
    let corpus = load_corpus();
    assert!(corpus.len() >= 10, "corpus too small: {}", corpus.len());
    for entry in &corpus {
        assert!(!entry.name.is_empty());
        assert!(!entry.source.is_empty());
        if entry.expected_parse_ok {
            // expected_transport defaults to "" via serde(default);
            // an empty string here would mean the fixture is wrong.
            // The corpus.json must always populate it for positive
            // entries (we check the corpus author wrote it).
            assert!(
                !entry.expected_transport.is_empty(),
                "{}: expected_transport missing for positive entry",
                entry.name
            );
        } else {
            assert!(
                !entry.expected_error_contains.is_empty(),
                "{}: expected_error_contains missing for negative entry",
                entry.name
            );
        }
    }
}

#[test]
fn rust_matches_corpus_positive_entries() {
    let corpus = load_corpus();
    let mut failures: Vec<String> = Vec::new();
    for entry in &corpus {
        if !entry.expected_parse_ok {
            continue;
        }
        match parse_axonendpoint(&entry.source) {
            Ok((transport, keepalive)) => {
                if transport != entry.expected_transport {
                    failures.push(format!(
                        "{}: transport={transport:?} != expected={:?}\n  source: {}",
                        entry.name, entry.expected_transport, entry.source
                    ));
                }
                if keepalive != entry.expected_keepalive {
                    failures.push(format!(
                        "{}: keepalive={keepalive:?} != expected={:?}\n  source: {}",
                        entry.name, entry.expected_keepalive, entry.source
                    ));
                }
            }
            Err(e) => failures.push(format!(
                "{}: expected parse_ok=true but got error: {e}\n  source: {}",
                entry.name, entry.source
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "drift-gate positive-entry mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn rust_matches_corpus_negative_entries() {
    let corpus = load_corpus();
    let mut failures: Vec<String> = Vec::new();
    for entry in &corpus {
        if entry.expected_parse_ok {
            continue;
        }
        match parse_axonendpoint(&entry.source) {
            Ok((t, k)) => failures.push(format!(
                "{}: expected parse error but got transport={t:?} keepalive={k:?}",
                entry.name
            )),
            Err(msg) => {
                if !msg.contains(&entry.expected_error_contains) {
                    failures.push(format!(
                        "{}: error message did not contain {:?}\n  got: {msg}",
                        entry.name, entry.expected_error_contains
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "drift-gate negative-entry mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn defaults_preserve_backwards_compat() {
    // D1: absent transport → "json"; backwards-compat for every
    // v1.20.0 axonendpoint that doesn't touch the new fields.
    let src = "axonendpoint Plain { method: POST execute: F }";
    let (transport, keepalive) =
        parse_axonendpoint(src).expect("parse default");
    assert_eq!(transport, "json");
    assert_eq!(keepalive, "");
}

#[test]
fn all_three_transport_values_accepted() {
    for val in ["json", "sse", "ndjson"] {
        let src = format!("axonendpoint X {{ execute: F transport: {val} }}");
        let (transport, _) = parse_axonendpoint(&src).expect("parse");
        assert_eq!(transport, val);
    }
}

#[test]
fn all_four_keepalive_values_accepted() {
    for val in ["5s", "15s", "30s", "60s"] {
        let src = format!(
            "axonendpoint X {{ execute: F transport: sse keepalive: {val} }}"
        );
        let (_, keepalive) = parse_axonendpoint(&src).expect("parse");
        assert_eq!(keepalive, val);
    }
}

#[test]
fn invalid_transport_typo_emits_smart_suggest() {
    // `sssee` is dist=1 from `sse` → smart-suggest fires byte-identical
    // to the Python side (D7 + v1.20.0 shared smart_suggest module).
    let src = "axonendpoint X { execute: F transport: sssee }";
    let err = parse_axonendpoint(src).expect_err("must error");
    assert!(
        err.contains("Did you mean `sse`?"),
        "smart-suggest hint missing: {err}"
    );
}

#[test]
fn invalid_keepalive_no_unit_rejected() {
    let src = "axonendpoint X { execute: F transport: sse keepalive: 15 }";
    let err = parse_axonendpoint(src).expect_err("must error");
    assert!(
        err.contains("Invalid keepalive"),
        "expected invalid keepalive message, got: {err}"
    );
}
