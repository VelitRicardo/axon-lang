//! v2.23.0 — `quant(… reupload: L)`: the data re-uploading grammar. `L ≥ 2`
//! interleaves the data encoding L times (the only provable escape from the
//! amplitude+Pauli quadratic bound). `reupload < 1` is `axon-E0784`.

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::{IRFlowNode, IRQuant};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn first_quant(src: &str) -> IRQuant {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    let ir = IRGenerator::new().generate(&prog);
    for flow in &ir.flows {
        for node in &flow.steps {
            if let IRFlowNode::Quant(q) = node {
                return q.clone();
            }
        }
    }
    panic!("no quant block");
}

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog).check().into_iter().map(|e| e.message).collect()
}

#[test]
fn reupload_lowers_into_the_ir_quant() {
    let q = first_quant(
        r#"
flow F() -> String {
    quant(encoding: angle, reupload: 3, qubits: 2) { yield s }
    return "ok"
}
"#,
    );
    assert_eq!(q.reupload, Some(3));
}

#[test]
fn absent_reupload_is_none_and_elided() {
    let q = first_quant(
        r#"
flow F() -> String {
    quant(encoding: amplitude, qubits: 2) { yield s }
    return "ok"
}
"#,
    );
    assert_eq!(q.reupload, None);
    let json = serde_json::to_string(&q).expect("serialize");
    assert!(!json.contains("reupload"), "absent reupload must be elided: {json}");
}

#[test]
fn reupload_zero_is_e0784() {
    let errs = errors(
        r#"
flow F() -> String {
    quant(encoding: angle, reupload: 0) { yield s }
    return "ok"
}
"#,
    );
    assert!(
        errs.iter().any(|e| e.contains("axon-E0784") && e.contains("reupload")),
        "reupload: 0 must be E0784; got {errs:?}"
    );
}
