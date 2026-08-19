//! v1.24.0 — Cross-stack drift gate (Rust side) for FlowExecutionEvent.
//!
//! D2 ratificada. The closed 6-variant catalog {FlowStart, StepStart,
//! StepToken, StepComplete, FlowComplete, FlowError} serializes to a
//! JSON shape byte-identical with the Python mirror
//! `axon.runtime.flow_execution_event`.
//!
//! The shared corpus at `tests/fixtures/flow_execution_event/
//! corpus.json` is parametrized by:
//!   - This Rust test pack — asserts serde round-trip of every entry.
//!   - The sibling Python test pack `tests/test_fase33_flow_execution_
//!     event.py` — asserts the same on the Python side.
//!
//! If either stack drifts on a variant, exactly one lane fails.

use std::fs;
use std::path::PathBuf;

use axon::flow_execution_event::{
    now_ms, FlowExecutionEvent,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Corpus {
    description: String,
    d_letter_anchor: String,
    entries: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    name: String,
    variant: String,
    expected_json: Value,
}

fn corpus_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join("flow_execution_event")
        .join("corpus.json")
}

fn load_corpus() -> Corpus {
    let raw =
        fs::read_to_string(corpus_path()).expect("v1.24.0 corpus must exist at the shared path");
    serde_json::from_str(&raw).expect("corpus.json must parse")
}

// ─── Corpus integrity ───────────────────────────────────────────────

#[test]
fn corpus_loads_with_required_shape() {
    let corpus = load_corpus();
    assert!(corpus.d_letter_anchor.starts_with("D2"));
    assert!(!corpus.description.is_empty());
    assert!(
        corpus.entries.len() >= 10,
        "corpus shrank below 10 entries: {}",
        corpus.entries.len()
    );
}

// ─── Drift gate: every corpus entry round-trips through Rust serde ──

#[test]
fn rust_round_trip_matches_corpus_for_every_entry() {
    let corpus = load_corpus();
    let mut failures = Vec::new();
    for entry in &corpus.entries {
        // Step 1: deserialize the expected JSON into a Rust enum.
        let parsed: Result<FlowExecutionEvent, _> =
            serde_json::from_value(entry.expected_json.clone());
        let event = match parsed {
            Ok(e) => e,
            Err(err) => {
                failures.push(format!(
                    "[{}] from_json failed: {err} (json: {})",
                    entry.name,
                    serde_json::to_string(&entry.expected_json).unwrap_or_default()
                ));
                continue;
            }
        };

        // Step 2: kind discriminator MUST match the corpus variant.
        let actual_kind = event.kind();
        if actual_kind != entry.variant {
            failures.push(format!(
                "[{}] kind drift: parser produced '{actual_kind}', corpus expected '{}'",
                entry.name, entry.variant
            ));
            continue;
        }

        // Step 3: re-serialize and compare byte-identical with corpus.
        let re_serialized = serde_json::to_value(&event).unwrap();
        if re_serialized != entry.expected_json {
            failures.push(format!(
                "[{}] re-serialization drift:\n  rust: {}\n  corpus: {}",
                entry.name,
                serde_json::to_string(&re_serialized).unwrap_or_default(),
                serde_json::to_string(&entry.expected_json).unwrap_or_default()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "Rust↔corpus drift in {} entries:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ─── Catalog closure: every variant constructible + serializable ────

#[test]
fn every_variant_constructible_and_serializable() {
    let variants = vec![
        FlowExecutionEvent::FlowStart {
            flow_name: "F".to_string(),
            backend: "stub".to_string(),
            timestamp_ms: 1,
        },
        FlowExecutionEvent::StepStart {
            step_name: "S".to_string(),
            step_index: 0,
            step_type: "step".to_string(),
            branch_path: String::new(),
            timestamp_ms: 2,
        },
        FlowExecutionEvent::StepToken {
            step_name: "S".to_string(),
            content: "x".to_string(),
            token_index: 1,
            branch_path: String::new(),
            timestamp_ms: 3,
        },
        FlowExecutionEvent::StepComplete {
            step_name: "S".to_string(),
            step_index: 0,
            success: true,
            full_output: "x".to_string(),
            tokens_input: 0,
            tokens_output: 1,
            branch_path: String::new(),
            timestamp_ms: 4,
        },
        FlowExecutionEvent::FlowComplete {
            flow_name: "F".to_string(),
            backend: "stub".to_string(),
            success: true,
            steps_executed: 1,
            tokens_input: 0,
            tokens_output: 1,
            latency_ms: 50,
            timestamp_ms: 5,
        },
        FlowExecutionEvent::FlowError {
            flow_name: "F".to_string(),
            error: "x".to_string(),
            timestamp_ms: 6,
        },
    ];
    for v in &variants {
        let s = serde_json::to_string(v).expect("variant must serialize");
        assert!(s.contains(r#""kind":"#));
    }
    assert_eq!(variants.len(), 6, "catalog must be 6 variants exactly");
}

// ─── Total predicates: terminator + step_scoped ─────────────────────

#[test]
fn terminator_predicate_partitions_variants_total() {
    let variants = vec![
        FlowExecutionEvent::FlowStart {
            flow_name: "F".to_string(),
            backend: "stub".to_string(),
            timestamp_ms: 0,
        },
        FlowExecutionEvent::StepStart {
            step_name: "S".to_string(),
            step_index: 0,
            step_type: "step".to_string(),
            branch_path: String::new(),
            timestamp_ms: 0,
        },
        FlowExecutionEvent::StepToken {
            step_name: "S".to_string(),
            content: "x".to_string(),
            token_index: 1,
            branch_path: String::new(),
            timestamp_ms: 0,
        },
        FlowExecutionEvent::StepComplete {
            step_name: "S".to_string(),
            step_index: 0,
            success: true,
            full_output: "x".to_string(),
            tokens_input: 0,
            tokens_output: 1,
            branch_path: String::new(),
            timestamp_ms: 0,
        },
        FlowExecutionEvent::FlowComplete {
            flow_name: "F".to_string(),
            backend: "stub".to_string(),
            success: true,
            steps_executed: 1,
            tokens_input: 0,
            tokens_output: 1,
            latency_ms: 50,
            timestamp_ms: 0,
        },
        FlowExecutionEvent::FlowError {
            flow_name: "F".to_string(),
            error: "x".to_string(),
            timestamp_ms: 0,
        },
    ];
    let terminators = variants.iter().filter(|v| v.is_terminator()).count();
    let non_terminators = variants.iter().filter(|v| !v.is_terminator()).count();
    assert_eq!(terminators, 2);
    assert_eq!(non_terminators, 4);
    assert_eq!(terminators + non_terminators, variants.len());
}

#[test]
fn step_scoped_predicate_partitions_variants_total() {
    let variants = vec![
        FlowExecutionEvent::FlowStart {
            flow_name: "F".to_string(),
            backend: "stub".to_string(),
            timestamp_ms: 0,
        },
        FlowExecutionEvent::StepStart {
            step_name: "S".to_string(),
            step_index: 0,
            step_type: "step".to_string(),
            branch_path: String::new(),
            timestamp_ms: 0,
        },
        FlowExecutionEvent::StepToken {
            step_name: "S".to_string(),
            content: "x".to_string(),
            token_index: 1,
            branch_path: String::new(),
            timestamp_ms: 0,
        },
        FlowExecutionEvent::StepComplete {
            step_name: "S".to_string(),
            step_index: 0,
            success: true,
            full_output: "x".to_string(),
            tokens_input: 0,
            tokens_output: 1,
            branch_path: String::new(),
            timestamp_ms: 0,
        },
        FlowExecutionEvent::FlowComplete {
            flow_name: "F".to_string(),
            backend: "stub".to_string(),
            success: true,
            steps_executed: 1,
            tokens_input: 0,
            tokens_output: 1,
            latency_ms: 50,
            timestamp_ms: 0,
        },
        FlowExecutionEvent::FlowError {
            flow_name: "F".to_string(),
            error: "x".to_string(),
            timestamp_ms: 0,
        },
    ];
    let step_scoped = variants.iter().filter(|v| v.is_step_scoped()).count();
    let non_step = variants.iter().filter(|v| !v.is_step_scoped()).count();
    assert_eq!(step_scoped, 3); // StepStart + StepToken + StepComplete
    assert_eq!(non_step, 3); // FlowStart + FlowComplete + FlowError
}

#[test]
fn now_ms_is_strictly_monotonic_under_clock() {
    let a = now_ms();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let b = now_ms();
    assert!(b > a, "now_ms must be monotonic on this platform");
}

// ─── Unknown variant rejected (mirror of Python ValueError) ─────────

#[test]
fn unknown_kind_rejected_by_serde() {
    let json = serde_json::json!({
        "kind": "bogus",
        "timestamp_ms": 0,
    });
    let result: Result<FlowExecutionEvent, _> = serde_json::from_value(json);
    assert!(
        result.is_err(),
        "serde MUST reject unknown 'kind' discriminator value"
    );
}
