//! v1.31.0 (D9) — Cross-stack drift gate for the `schema:`
//! declaration (Rust side).
//!
//! Per D9: the Rust frontend is the authoritative type-checker, but
//! the Python frontend must STRUCTURALLY agree on the AST/IR for the
//! same source. This pack walks a shared corpus + asserts the Rust
//! frontend's IR matches the per-entry `expected` block. The Python
//! pack `tests/test_fase38_b_schema_drift.py` reads the same corpus
//! and runs identical assertions; if Python and Rust ever diverge,
//! exactly one of the two packs fails — drift caught at PR review,
//! not at adopter-bug-report time.
//!
//! Corpus path: `<repo-root>/tests/fixtures/schema_drift/corpus.json`.

use std::fs;
use std::path::PathBuf;

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::IRStoreColumnSchema;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Debug)]
struct CorpusFile {
    entries: Vec<CorpusEntry>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)] // `_comment` and other future fields tolerated.
struct CorpusEntry {
    name: String,
    source: String,
    expected: Value,
}

fn corpus_path() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .expect("workspace root")
        .join("tests")
        .join("fixtures")
        .join("schema_drift")
        .join("corpus.json")
}

fn lower_first_axonstore(src: &str) -> Value {
    let tokens = Lexer::new(src, "drift.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    let ir = IRGenerator::new().generate(&program);
    assert_eq!(ir.axonstore_specs.len(), 1, "corpus entry must declare exactly one axonstore");
    let store = &ir.axonstore_specs[0];
    match &store.column_schema {
        None => Value::Null,
        Some(IRStoreColumnSchema::Inline { columns }) => {
            let cols: Vec<Value> = columns
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "name": c.name,
                        "type": c.col_type,
                        "primary_key": c.primary_key,
                        "auto_increment": c.auto_increment,
                        "not_null": c.not_null,
                        "unique": c.unique,
                        "default_value": c.default_value,
                    })
                })
                .collect();
            serde_json::json!({
                "form": "inline",
                "columns": cols,
            })
        }
        Some(IRStoreColumnSchema::ManifestRef { qualified_name }) => serde_json::json!({
            "form": "manifest_ref",
            "qualified_name": qualified_name,
        }),
        Some(IRStoreColumnSchema::EnvVar { var_name }) => serde_json::json!({
            "form": "env_var",
            "var_name": var_name,
        }),
    }
}

#[test]
fn schema_drift_gate_rust_matches_corpus_expectations() {
    let path = corpus_path();
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read corpus {}: {e}", path.display()));
    let corpus: CorpusFile = serde_json::from_str(&text).expect("parse corpus");
    assert!(
        !corpus.entries.is_empty(),
        "corpus must carry at least one entry — pin via a known-good case"
    );
    for entry in &corpus.entries {
        let observed = lower_first_axonstore(&entry.source);
        assert_eq!(
            observed, entry.expected,
            "drift on entry `{}` — Rust observed {observed} but corpus expected {}",
            entry.name, entry.expected
        );
    }
}
