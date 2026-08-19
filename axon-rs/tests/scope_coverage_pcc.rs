//! v2.77.0 — the PCC `ScopeCoverageSoundness` witness: the
//! deploy-time proof twin of the `axon-T956` compile-time law.
//!
//! A covered program generates a verifying proof; a hand-edited IR that strips
//! the granting credential (or adds an unauthorized tool-use) is REFUTED before
//! it mounts — the checker RE-DERIVES scope coverage independently.

use axon::pcc::{
    check_proof, derive_scope_coverage_soundness_witness,
    generate_scope_coverage_soundness_proofs, CheckOutcome, PropertyClass, Witness,
};

const VERSION: &str = "2.8.0-test";

/// A covered program: a tool declaring a required scope, used in a flow, with a
/// `credential` granting that scope.
const COVERED: &str = r#"
tool ReportReader {
  requires: [reports.read]
  parameters: { id: String }
  output_type: String
}

credential ReportAuth { ttl: 1h grants: [reports.read] }

type Out { summary: String }

flow ReadReport(id: String) -> Out {
  use ReportReader(id = "${id}")
  step S { ask: "summarize" output: Out }
}
"#;

fn ir_of(src: &str) -> axon::ir_nodes::IRProgram {
    let (_program, ir) =
        axon::flow_plan::compile_source_to_ir(src, "test.axon").expect("compile to IR");
    ir
}

#[test]
fn a_covered_program_generates_one_proof_that_verifies() {
    let ir = ir_of(COVERED);
    let proofs = generate_scope_coverage_soundness_proofs(&ir, VERSION);
    assert_eq!(proofs.len(), 1, "one scope-declaring tool ⇒ one whole-program proof");
    assert_eq!(proofs[0].property, PropertyClass::ScopeCoverageSoundness);
    assert_eq!(
        check_proof(&proofs[0], &ir),
        CheckOutcome::Verified,
        "the covered program must be certified sound by the independent checker"
    );
}

#[test]
fn the_witness_carries_the_scoped_tools_the_granted_set_and_no_uncovered_use() {
    let ir = ir_of(COVERED);
    let proofs = generate_scope_coverage_soundness_proofs(&ir, VERSION);
    let Witness::ScopeCoverageSoundness(w) = &proofs[0].witness else {
        panic!("expected a ScopeCoverageSoundness witness");
    };
    assert_eq!(w.scoped_tools, vec![("ReportReader".to_string(), vec!["reports.read".to_string()])]);
    assert!(w.granted.contains(&"reports.read".to_string()));
    assert!(w.uncovered_uses.is_empty(), "a verifying proof has no uncovered use");
}

#[test]
fn stripping_the_granting_credential_is_detected_by_re_derivation() {
    let ir = ir_of(COVERED);
    let proof = generate_scope_coverage_soundness_proofs(&ir, VERSION).remove(0);

    // The attack: a hand-edited stored IR with the granting credential removed —
    // the tool-use now requires a scope the (empty) granted set does not cover.
    let mut tampered = ir.clone();
    tampered.credentials.clear();

    // (a) The re-derivation independently surfaces the uncovered use.
    let actual = derive_scope_coverage_soundness_witness(&tampered)
        .expect("the scope-declaring tool still makes this a contract");
    assert_eq!(
        actual.uncovered_uses,
        vec![(
            "ReadReport".to_string(),
            "ReportReader".to_string(),
            "reports.read".to_string()
        )],
        "the stripped-credential tamper must surface as an uncovered use"
    );

    // (b) The original proof no longer certifies the tampered artifact — the
    // digest binding alone refutes it (the witness is bound to the artifact it
    // was minted for), defense in depth over the re-derivation above.
    assert_eq!(
        check_proof(&proof, &tampered),
        CheckOutcome::DigestMismatch,
        "the proof is bound to the original artifact; a tampered IR is refuted"
    );
}

#[test]
fn a_program_with_no_scope_declaring_tool_yields_no_proof() {
    // No `requires:` anywhere ⇒ no contract ⇒ no proof (the "no contract, no
    // proof" posture — a verifier cannot be handed a proof of a property the
    // program never engages).
    let src = r#"
tool Plain { parameters: { id: String } output_type: String }
type Out { x: String }
flow F(id: String) -> Out {
  use Plain(id = "${id}")
  step S { ask: "x" output: Out }
}
"#;
    let ir = ir_of(src);
    assert!(generate_scope_coverage_soundness_proofs(&ir, VERSION).is_empty());
    assert!(derive_scope_coverage_soundness_witness(&ir).is_none());
}
