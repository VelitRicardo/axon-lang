//! v1.12.0 — Cross-stack parity gate for `let` runtime binding.
//!
//! Both stacks consume the same input fixture (target + value +
//! value_kind) and produce the same post-bind context shape (a flat
//! `{name: value}` dict). This gate exercises the LetPayload struct
//! end-to-end via deserialization — drift on either side breaks both
//! gates. The matching Python gate lives in
//! tests/test_let_runtime.py::TestCrossStackParity.

use std::fs;
use std::path::PathBuf;

use axon::runner::LetPayload;

#[test]
fn let_binding_payload_round_trips() {
    let base: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "parity"]
        .iter()
        .collect();

    let spec_raw = fs::read_to_string(base.join("let_binding.spec.json"))
        .expect("spec fixture missing");
    let golden_raw = fs::read_to_string(base.join("let_binding.golden.json"))
        .expect("golden missing");

    // Deserialize the spec — LetPayload mirrors what
    // base_backend._compile_let_step writes into the metadata.
    let payload: LetPayload =
        serde_json::from_str(&spec_raw).expect("spec parses as LetPayload");

    // Compute the post-bind context shape: literals bind verbatim,
    // references would resolve at runtime (out of scope for the
    // parity gate which uses a literal fixture).
    let resolved = if payload.value_kind == "reference" {
        // For tests this branch is unused but kept symmetric with
        // the runtime dispatcher.
        payload.value.clone()
    } else {
        payload.value.clone()
    };
    let post_bind = serde_json::json!({ &payload.target: resolved });

    let golden_value: serde_json::Value =
        serde_json::from_str(&golden_raw).expect("golden parses");

    if post_bind != golden_value {
        let actual = serde_json::to_string_pretty(&post_bind).unwrap();
        let expected = serde_json::to_string_pretty(&golden_value).unwrap();
        panic!(
            "v1.12.0 let binding parity broke (Rust):\n\
             --- expected (golden) ---\n{}\n\
             --- actual (rust) ---\n{}",
            expected, actual,
        );
    }
}
