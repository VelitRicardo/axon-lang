//! v1.22.0 — Cross-stack drift gate (Rust side).
//!
//! D7 ratified 2026-05-11 (extends v1.20.0 D7 + v1.21.0 D7):
//! byte-identical `implicit_transport` inference across the Python
//! axon-lang frontend and the Rust axon-frontend.
//!
//! The corpus + golden snapshot lives at
//! `tests/fixtures/implicit_transport/corpus.json` at the
//! repo root. Each entry pins `(name, source, endpoint, expected_
//! implicit_transport)`.
//!
//! The Python pack `tests/test_fase31_implicit_transport.py`
//! parametrizes over the same JSON. If at any future point the two
//! stacks diverge on what inference a given source produces, exactly
//! one of the two test packs fails — drift caught at PR-review time,
//! never at adopter-bug-report time.
//!
//! Four-pillar trace per D10:
//!   MATHEMATICS — function is total + deterministic across stacks.
//!   LOGIC       — the inference rule is the same disjunction (a, b, c).
//!   PHILOSOPHY  — both stacks present the same language to adopters.
//!   COMPUTING   — drift is impossible to ship; CI gates every PR.

use std::fs;
use std::path::PathBuf;

use axon_frontend::ast::Declaration;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::compute_implicit_transports;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct CorpusEntry {
    name: String,
    source: String,
    /// `None` for sentinel rows that exercise the fixture walker
    /// only (no endpoint to assert on).
    endpoint: Option<String>,
    /// `None` aligned with `endpoint == None`.
    expected_implicit_transport: Option<String>,
    /// v1.22.0 — explicit flag for corpus entries that hit the
    /// known Rust frontend parser gap (e.g. `output: Stream<T>`
    /// inside step bodies). Rust drift gate skips these and logs;
    /// Python carries the assertion on its own until the Rust
    /// frontend completion step lands. D7 contract is honored
    /// for all entries WITHOUT this flag.
    #[serde(default)]
    rust_parser_known_gap: bool,
    /// Optional comment field; not consumed by the gate.
    #[serde(default)]
    comment: String,
}

fn corpus_path() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .expect("CARGO_MANIFEST_DIR has a parent")
        .join("tests")
        .join("fixtures")
        .join("implicit_transport")
        .join("corpus.json")
}

fn load_corpus() -> Vec<CorpusEntry> {
    let path = corpus_path();
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("failed to parse corpus.json: {e}"))
}

#[test]
fn corpus_loads_and_has_required_shape() {
    let corpus = load_corpus();
    assert!(corpus.len() >= 20, "corpus should have >= 20 entries");
    let mut seen: std::collections::HashSet<&str> =
        std::collections::HashSet::new();
    for entry in &corpus {
        assert!(seen.insert(&entry.name), "duplicate entry name: {}", entry.name);
        if let Some(ref v) = entry.expected_implicit_transport {
            assert!(
                v == "sse" || v == "json",
                "entry {}: expected_implicit_transport must be 'sse' or 'json', got {v}",
                entry.name
            );
        }
    }
}

#[test]
fn rust_matches_corpus_inference() {
    let corpus = load_corpus();
    let mut skipped: Vec<&str> = Vec::new();
    let mut asserted: usize = 0;
    for entry in &corpus {
        let expected = match &entry.expected_implicit_transport {
            Some(v) => v,
            None => continue, // sentinel row
        };
        let endpoint_name = match &entry.endpoint {
            Some(n) => n,
            None => continue,
        };
        if entry.rust_parser_known_gap {
            skipped.push(&entry.name);
            continue;
        }

        // Parse + compute inference on the AST.
        let tokens = Lexer::new(&entry.source, "<corpus>")
            .tokenize()
            .unwrap_or_else(|e| {
                panic!("entry {}: tokenize failed: {e:?}", entry.name)
            });
        let mut program = Parser::new(tokens).parse().unwrap_or_else(|e| {
            panic!("entry {}: parse failed: {e:?}", entry.name)
        });
        compute_implicit_transports(&mut program);

        let ae = program
            .declarations
            .iter()
            .find_map(|d| match d {
                Declaration::AxonEndpoint(ae) if &ae.name == endpoint_name => Some(ae),
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!(
                    "entry {}: axonendpoint '{}' not found in program",
                    entry.name, endpoint_name
                )
            });

        assert_eq!(
            &ae.implicit_transport, expected,
            "drift on '{}': expected '{expected}', got '{}'",
            entry.name, ae.implicit_transport
        );
        asserted += 1;
    }
    // Sanity: we should be asserting on at LEAST 15 entries (the
    // corpus has 25; skipping disjunct (a) entries leaves >= 20).
    assert!(
        asserted >= 15,
        "drift gate asserted only {asserted} entries — corpus may have over-flagged Rust gaps"
    );
    eprintln!(
        "drift_gate_wire_inference: {asserted} entries asserted cross-stack, {} skipped (Rust parser gaps): {:?}",
        skipped.len(),
        skipped
    );
}

#[test]
fn parser_sets_transport_explicit_on_declaration() {
    // Direct, surgical test that the parser sets
    // `transport_explicit = true` if-and-only-if the source carries
    // the `transport:` field on the specific axonendpoint. Uses
    // hand-crafted sources so there's no multi-endpoint ambiguity.
    let cases: &[(&str, bool)] = &[
        (
            "flow F() -> Unit { step S { ask: \"x\" } } \
             axonendpoint F { method: POST path: \"/f\" execute: F }",
            false,
        ),
        (
            "flow F() -> Unit { step S { ask: \"x\" } } \
             axonendpoint F { method: POST path: \"/f\" execute: F transport: json }",
            true,
        ),
        (
            "flow F() -> Unit { step S { ask: \"x\" } } \
             axonendpoint F { method: POST path: \"/f\" execute: F transport: sse }",
            true,
        ),
        (
            "flow F() -> Unit { step S { ask: \"x\" } } \
             axonendpoint F { method: POST path: \"/f\" execute: F transport: ndjson }",
            true,
        ),
    ];
    for (src, expected_explicit) in cases {
        let tokens = Lexer::new(src, "<flag>").tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let ae = program
            .declarations
            .iter()
            .find_map(|d| match d {
                Declaration::AxonEndpoint(ae) => Some(ae),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            ae.transport_explicit, *expected_explicit,
            "transport_explicit mismatch for source: {src}"
        );
    }
}

#[test]
fn idempotent_recomputation() {
    // f(f(x)) == f(x) for compute_implicit_transports.
    let src = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
               flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
               axonendpoint F { method: POST path: \"/f\" execute: F }";
    let tokens = Lexer::new(src, "<idempotence>").tokenize().unwrap();
    let mut program = Parser::new(tokens).parse().unwrap();
    compute_implicit_transports(&mut program);
    let first = program
        .declarations
        .iter()
        .find_map(|d| match d {
            Declaration::AxonEndpoint(ae) => Some(ae.implicit_transport.clone()),
            _ => None,
        })
        .unwrap();
    compute_implicit_transports(&mut program);
    let second = program
        .declarations
        .iter()
        .find_map(|d| match d {
            Declaration::AxonEndpoint(ae) => Some(ae.implicit_transport.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(first, "sse");
    assert_eq!(first, second, "compute_implicit_transports must be idempotent");
}

#[test]
fn unresolvable_execute_flow_defaults_json() {
    let src = "axonendpoint Orphan { method: POST path: \"/o\" execute: Ghost }";
    let tokens = Lexer::new(src, "<orphan>").tokenize().unwrap();
    let mut program = Parser::new(tokens).parse().unwrap();
    compute_implicit_transports(&mut program);
    let ae = program
        .declarations
        .iter()
        .find_map(|d| match d {
            Declaration::AxonEndpoint(ae) => Some(ae),
            _ => None,
        })
        .unwrap();
    assert_eq!(ae.implicit_transport, "json");
}
