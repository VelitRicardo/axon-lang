//! v1.10.0 — Cross-stack parity gate for `lambda apply` runtime.
//!
//! Both stacks (Python `axon.runtime.lambda_runtime.build_psi` + Rust
//! `axon_lang::lambda_runtime::build_psi`) consume the same input
//! shape (a spec snapshot + a target value) and produce the same
//! output shape (ψ = ⟨T, V, E⟩ with formalism-matching JSON keys).
//!
//! This test asserts the Rust output matches the canonical golden in
//! `tests/parity/lambda_apply.golden.json`. The matching Python
//! gate lives in `tests/test_lambda_data_runtime.py::TestCrossStackParity`
//! and reads the same files. Drift on either side breaks both.

use std::fs;
use std::path::PathBuf;

use axon::lambda_runtime::{build_psi, SpecSnapshot};

#[test]
fn lambda_apply_psi_matches_golden() {
    let base: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "parity"]
        .iter()
        .collect();

    let spec_raw = fs::read_to_string(base.join("lambda_apply.spec.json"))
        .expect("spec fixture missing");
    let target_raw = fs::read_to_string(base.join("lambda_apply.target.json"))
        .expect("target fixture missing");
    let golden_raw = fs::read_to_string(base.join("lambda_apply.golden.json"))
        .expect("golden missing");

    let spec: SpecSnapshot = serde_json::from_str(&spec_raw).expect("spec parses");
    let target: serde_json::Value =
        serde_json::from_str(&target_raw).expect("target parses");

    let psi = build_psi(&spec, target).expect("build_psi succeeds for fixture");
    let psi_value = serde_json::to_value(&psi).expect("ψ serialises");

    let golden_value: serde_json::Value =
        serde_json::from_str(&golden_raw).expect("golden parses");

    if psi_value != golden_value {
        let psi_canon = serde_json::to_string_pretty(&psi_value).unwrap();
        let golden_canon = serde_json::to_string_pretty(&golden_value).unwrap();
        panic!(
            "v1.10.0 ψ parity broke (Rust):\n--- expected (golden) ---\n{}\n--- actual (rust) ---\n{}",
            golden_canon, psi_canon,
        );
    }
}
