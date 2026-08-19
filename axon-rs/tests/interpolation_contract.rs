//! v1.30.0 — the interpolation-contract drift gate (Rust side).
//!
//! axon has ONE interpolation syntax — `${name}` / `$name`. This test
//! reads the SAME corpus the Python runtime's contract test reads
//! (`tests/fixtures/interpolation_contract.json`, repo root) and
//! asserts the canonical Rust `interpolate_vars` produces byte-
//! identical output. Together with the Python test
//! (`tests/test_interpolation_contract.py`) it pins the two runtimes:
//! a single `.axon` interpolates the same on either one. If either
//! runtime drifts from the documented contract, this gate fails.

use std::collections::HashMap;

use axon::exec_context::interpolate_vars;
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    input: String,
    vars: HashMap<String, String>,
    expected: String,
}

#[test]
fn interpolation_contract_corpus_is_byte_identical() {
    // The corpus lives at the repo root, shared with the Python test.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/fixtures/interpolation_contract.json"
    );
    let raw = std::fs::read_to_string(path)
        .expect("read tests/fixtures/interpolation_contract.json");
    let cases: Vec<Case> =
        serde_json::from_str(&raw).expect("parse interpolation corpus");
    assert!(!cases.is_empty(), "interpolation contract corpus is empty");

    for case in &cases {
        let got = interpolate_vars(&case.input, &case.vars);
        assert_eq!(
            got, case.expected,
            "case `{}`: interpolate_vars({:?}, {:?}) = {:?}, expected {:?}",
            case.name, case.input, case.vars, got, case.expected
        );
    }
}

#[test]
fn unknown_variables_are_left_literal() {
    // The conservative rule, shared with the Python runtime — an
    // unknown name is never blanked, it stays verbatim.
    let empty: HashMap<String, String> = HashMap::new();
    assert_eq!(interpolate_vars("${a} $b", &empty), "${a} $b");
}

#[test]
fn double_brace_is_not_the_contract() {
    // `{{name}}` was never axon interpolation — the Rust runtime never
    // accepted it; it stays literal here.
    let vars: HashMap<String, String> =
        [("name".to_string(), "x".to_string())].into_iter().collect();
    assert_eq!(interpolate_vars("{{name}}", &vars), "{{name}}");
}
