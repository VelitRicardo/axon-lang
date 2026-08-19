//! v2.8.0 — Proof-Carrying Code for tool calls, drift-gated on the
//! CANONICAL example.
//!
//! The v2.8.0 type-checker's CT-2 caller-blame check is now an
//! INDEPENDENTLY-VERIFIABLE proof: the tool's `parameters:` schema rides
//! the proof bundle, and the checker re-derives — from the artifact alone,
//! never trusting the compiler — that every `use <Tool>(k = v, …)` call
//! satisfies it. This gate pins the property on the living
//! `examples/tool_dispatch_structured.axon`: its one structured call to a
//! schema-full tool generates exactly one tool-call-soundness proof that
//! the independent checker VERIFIES; a regression (a renamed schema, a
//! broken derivation) fails here.

use axon::pcc::{
    check_proof, generate_all_proofs, generate_tool_call_soundness_proofs, CheckOutcome,
    PropertyClass, Witness,
};

const EXAMPLE_PATH: &str = "../examples/tool_dispatch_structured.axon";
const VERSION: &str = "2.8.0-test";

fn canonical_ir() -> axon::ir_nodes::IRProgram {
    let src = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/tool_dispatch_structured.axon not found — run from axon-rs/");
    let (_program, ir) =
        axon::flow_plan::compile_source_to_ir(&src, "tool_dispatch_structured.axon")
            .expect("compile the canonical example to IR");
    ir
}

#[test]
fn canonical_example_generates_one_tool_call_proof_that_verifies() {
    let ir = canonical_ir();
    let proofs = generate_tool_call_soundness_proofs(&ir, VERSION);
    assert_eq!(
        proofs.len(),
        1,
        "the canonical example has exactly one structured `use Tool(k=v)` call \
         to a schema-full tool => one tool-call-soundness proof"
    );
    assert_eq!(
        check_proof(&proofs[0], &ir),
        CheckOutcome::Verified,
        "the canonical structured call must be certified sound by the independent checker"
    );
}

#[test]
fn canonical_proof_witness_carries_the_schema_and_call() {
    let ir = canonical_ir();
    let proofs = generate_tool_call_soundness_proofs(&ir, VERSION);
    let Witness::ToolCallSoundness(w) = &proofs[0].witness else {
        panic!("expected a ToolCallSoundness witness");
    };
    assert_eq!(w.flow_name, "ScanCrm");
    assert_eq!(w.tool_name, "CrmRadar");
    assert!(w.schema_present, "the tool declares a non-empty parameters schema");
    // The schema rides the proof (the verifier re-derives from it).
    assert_eq!(
        w.declared_params,
        vec![
            "active".to_string(),
            "company".to_string(),
            "max_results".to_string()
        ]
    );
    // A sound call: no defects.
    assert!(w.unknown_args.is_empty());
    assert!(w.duplicate_args.is_empty());
    assert!(w.missing_required.is_empty());
    assert!(w.type_mismatches.is_empty());
}

#[test]
fn tampering_the_canonical_artifact_digest_is_rejected() {
    // A proof minted for the canonical example must not verify against a
    // mutated artifact (digest binding, the design decision). Add a tool → new digest.
    let ir = canonical_ir();
    let proofs = generate_tool_call_soundness_proofs(&ir, VERSION);

    let mut ir_b = canonical_ir();
    ir_b.tools.push(axon::ir_nodes::IRToolSpec {
        node_type: "tool",
        source_line: 1,
        source_column: 1,
        name: "Extra".to_string(),
        provider: "native".to_string(),
        max_results: None,
        filter_expr: String::new(),
        timeout: String::new(),
        runtime: String::new(),
        resource_ref: String::new(),
        sandbox: None,
        input_schema: Vec::new(),
        output_schema: String::new(),
        parameters: Vec::new(),
        output_type: None,
        requires: Vec::new(),
        secret: String::new(),
        secret_partition: String::new(),
        effect_row: Vec::new(),
        target: None,
        risk: None,
        argv: Vec::new(),
        cache: String::new(),
        scrape: None,
    });
    assert_eq!(check_proof(&proofs[0], &ir_b), CheckOutcome::DigestMismatch);
}

#[test]
fn canonical_full_bundle_includes_tool_call_soundness_and_is_deployable() {
    let ir = canonical_ir();
    let bundle = generate_all_proofs(&ir, VERSION);
    assert!(
        bundle
            .iter()
            .any(|p| p.property == PropertyClass::ToolCallSoundness),
        "the full proof bundle must include the tool-call-soundness class"
    );
    for proof in &bundle {
        assert_eq!(
            check_proof(proof, &ir),
            CheckOutcome::Verified,
            "the canonical example is clean — every proof must verify"
        );
    }
}
