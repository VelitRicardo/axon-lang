//! v2.22.0 — `requires_context:` step grammar round-trips through parse → IR.
//!
//! A step declares its model-capability need (`requires_context: <tokens>`); the
//! field must survive lowering into `IRStep.requires_context` so the v2.22.0
//! resolver can map it to a concrete model. A step WITHOUT it lowers to `None`
//! (back-compat, the design decision) and the IR JSON elides the field (`skip_serializing_if`
//! → no IR-SHA drift). A non-integer value is a parse error at the exact column.

use axon_frontend::ir_nodes::{IRFlowNode, IRStep};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn first_step(src: &str) -> IRStep {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    let ir = axon_frontend::ir_generator::IRGenerator::new().generate(&prog);
    for flow in &ir.flows {
        for node in &flow.steps {
            if let IRFlowNode::Step(s) = node {
                return s.clone();
            }
        }
    }
    panic!("no step found in IR");
}

const FLOW_WITH: &str = r#"
flow Summarize() -> String {
    step Big {
        given: history
        ask: "summarize ${history}"
        requires_context: 16000
        output: String
    }
    return "ok"
}
"#;

const FLOW_WITHOUT: &str = r#"
flow Summarize() -> String {
    step Small {
        given: history
        ask: "summarize ${history}"
        output: String
    }
    return "ok"
}
"#;

#[test]
fn requires_context_lowers_into_the_ir_step() {
    let step = first_step(FLOW_WITH);
    assert_eq!(
        step.requires_context,
        Some(16_000),
        "the declared context requirement must reach IRStep for the v2.22.0 resolver"
    );
}

#[test]
fn absent_requires_context_is_none_and_elided_from_json() {
    let step = first_step(FLOW_WITHOUT);
    assert_eq!(step.requires_context, None, "back-compat: absent → None");
    // The IR JSON must NOT carry the key when None — zero IR-SHA drift for every
    // pre-v2.22.0 step (`skip_serializing_if = Option::is_none`).
    let json = serde_json::to_string(&step).expect("serialize");
    assert!(
        !json.contains("requires_context"),
        "an absent requirement must be elided from the IR JSON, got: {json}"
    );
}

#[test]
fn present_requires_context_is_serialized() {
    let step = first_step(FLOW_WITH);
    let json = serde_json::to_string(&step).expect("serialize");
    assert!(
        json.contains("\"requires_context\":16000"),
        "a declared requirement must ride the IR JSON, got: {json}"
    );
}

#[test]
fn non_integer_requires_context_is_a_parse_error() {
    let src = r#"
flow F() -> String {
    step S { given: h  ask: "x"  requires_context: "big"  output: String }
    return "ok"
}
"#;
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let err = Parser::new(tokens).parse().expect_err("a string value must not parse");
    assert!(
        err.message.contains("requires_context"),
        "the parse error must name the offending field, got: {}",
        err.message
    );
}
