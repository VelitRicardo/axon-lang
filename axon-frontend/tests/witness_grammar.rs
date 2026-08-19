//! v2.23.0 — the `witness` grammar: a well-formed Advantage Witness parses +
//! lowers into `IRWitness` (rides the IR to the deploy/runtime evaluator); a
//! malformed one is `axon-E0790` at `axon check`. The advantage VALUE is NOT
//! computed here (it needs real data) — this cycle ships the LAW + the surface.

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn ir(src: &str) -> axon_frontend::ir_nodes::IRProgram {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    IRGenerator::new().generate(&prog)
}

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog).check().into_iter().map(|e| e.message).collect()
}

const WELL_FORMED: &str = r#"
witness SeedKernelBeatsCosine {
    claim:     SeedKernel
    against:   cosine
    metric:    geometric_difference
    threshold: 0.05
    data:      mdn_embeddings
}
"#;

#[test]
fn well_formed_witness_lowers_into_ir() {
    let prog = ir(WELL_FORMED);
    assert_eq!(prog.witnesses.len(), 1);
    let w = &prog.witnesses[0];
    assert_eq!(w.name, "SeedKernelBeatsCosine");
    assert_eq!(w.claim, "SeedKernel");
    assert_eq!(w.baseline, "cosine");
    assert_eq!(w.metric, "geometric_difference");
    assert_eq!(w.threshold, 0.05);
    assert_eq!(w.data, "mdn_embeddings");
}

#[test]
fn well_formed_witness_type_checks_clean() {
    let errs = errors(WELL_FORMED);
    assert!(
        !errs.iter().any(|e| e.contains("axon-E0790")),
        "a well-formed witness must type-check clean, got: {errs:?}"
    );
}

#[test]
fn witness_rides_the_ir_json_but_a_witnessless_program_is_byte_identical() {
    // Present → serialized (the enterprise evaluator reads it).
    let prog = ir(WELL_FORMED);
    let json = serde_json::to_string(&prog).expect("serialize");
    assert!(json.contains("\"witnesses\""), "a witness must ride the IR JSON");
    // Absent → elided (zero IR-SHA drift for every pre-v2.23.0 program).
    let empty = ir("flow F() -> String { return \"ok\" }");
    let ejson = serde_json::to_string(&empty).expect("serialize");
    assert!(
        !ejson.contains("witnesses"),
        "a witness-less program must elide the field, got: {ejson}"
    );
}

#[test]
fn unknown_metric_is_e0790() {
    let errs = errors(
        r#"
witness W { claim: K  against: cosine  metric: quantum_magic  threshold: 0.1  data: d }
"#,
    );
    assert!(
        errs.iter().any(|e| e.contains("axon-E0790") && e.contains("quantum_magic")),
        "an unknown metric must be E0790; got {errs:?}"
    );
}

#[test]
fn missing_data_is_e0790_no_advantage_in_the_abstract() {
    let errs = errors(
        r#"
witness W { claim: K  against: cosine  metric: geometric_difference  threshold: 0.1 }
"#,
    );
    assert!(
        errs.iter().any(|e| e.contains("axon-E0790") && e.contains("data")),
        "a witness without `data:` must be E0790 (no advantage in the abstract); got {errs:?}"
    );
}

#[test]
fn missing_baseline_and_claim_are_e0790() {
    let errs = errors(
        r#"
witness W { metric: geometric_difference  threshold: 0.1  data: d }
"#,
    );
    assert!(errs.iter().any(|e| e.contains("axon-E0790") && e.contains("claim")));
    assert!(errs.iter().any(|e| e.contains("axon-E0790") && e.contains("against")));
}
