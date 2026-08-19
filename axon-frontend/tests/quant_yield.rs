//! v2.4.0 — the `yield <expr>` measurement point (D9).
//!
//! `yield` collapses a `quant` block's evolved amplitudes back to classical
//! silicon. It parses inside a quant body (token + AST/IR node) and is only
//! well-formed there — the checker rejects `yield` outside a quant block
//! (`axon-E0787`). The actual amplitude collapse + one-shot continuation is the
//! v2.4.0 reference simulator / enterprise backend (surface-only here).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::{IRFlowNode, IRProgram};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn ir_of(src: &str) -> IRProgram {
    let tokens = Lexer::new(src, "y.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    IRGenerator::new().generate(&program)
}

fn errors_of(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "y.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program).check().into_iter().map(|e| e.message).collect()
}

fn has(errs: &[String], code: &str) -> bool {
    errs.iter().any(|e| e.contains(code))
}

// ── Parse + lower ────────────────────────────────────────────────────────────

#[test]
fn yield_inside_quant_parses_and_lowers() {
    let src = "flow F(audio: String) -> String {\n\
                  quant {\n\
                     let surrogate = audio\n\
                     yield surrogate\n\
                  }\n\
                  return audio\n\
               }";
    let ir = ir_of(src);
    let quant = ir
        .flows
        .iter()
        .find(|f| f.name == "F")
        .expect("flow")
        .steps
        .iter()
        .find_map(|n| match n {
            IRFlowNode::Quant(q) => Some(q),
            _ => None,
        })
        .expect("quant");
    // The body holds [Let, Yield]; the yield carries the measured reference.
    assert_eq!(quant.body.len(), 2);
    match &quant.body[1] {
        IRFlowNode::Yield(y) => {
            assert_eq!(y.value_expr, "surrogate");
            assert_eq!(y.value_kind, "reference");
        }
        other => panic!("expected a Yield node, got {other:?}"),
    }
}

#[test]
fn yield_inside_quant_typechecks_clean() {
    let src = "flow F(audio: String) -> String {\n\
                  quant {\n\
                     let surrogate = audio\n\
                     yield surrogate\n\
                  }\n\
                  return audio\n\
               }";
    assert!(!has(&errors_of(src), "axon-E0787"), "yield inside quant is well-formed");
}

#[test]
fn yield_nested_in_for_inside_quant_is_allowed() {
    let src = "flow F(items: String, audio: String) -> String {\n\
                  quant {\n\
                     for e in items {\n\
                        yield audio\n\
                     }\n\
                  }\n\
                  return audio\n\
               }";
    assert!(!has(&errors_of(src), "axon-E0787"), "yield nested inside quant (via for) is allowed");
}

// ── yield outside quant is rejected (E0787) ──────────────────────────────────

#[test]
fn yield_at_flow_top_level_is_rejected() {
    let src = "flow F(audio: String) -> String {\n\
                  yield audio\n\
                  return audio\n\
               }";
    let errs = errors_of(src);
    assert!(has(&errs, "axon-E0787"), "yield outside quant must raise E0787: {errs:?}");
}

#[test]
fn yield_in_a_plain_for_outside_quant_is_rejected() {
    let src = "flow F(items: String) -> String {\n\
                  for e in items {\n\
                     yield e\n\
                  }\n\
                  return items\n\
               }";
    assert!(has(&errors_of(src), "axon-E0787"), "yield in a non-quant for-body must raise E0787");
}

// ── lowering catalog: yield is a distinct IRFlowNode kind ────────────────────

#[test]
fn yield_lowers_to_its_own_node_kind() {
    // Two yields (top-level rejected by checker, but parse/lower still works);
    // here we lower a quant-scoped yield and confirm the node kind.
    let src = "flow F(audio: String) -> String {\n\
                  quant { yield audio }\n\
                  return audio\n\
               }";
    let ir = ir_of(src);
    let f = ir.flows.iter().find(|f| f.name == "F").unwrap();
    let q = f.steps.iter().find_map(|n| match n { IRFlowNode::Quant(q) => Some(q), _ => None }).unwrap();
    assert!(matches!(q.body[0], IRFlowNode::Yield(_)), "the bare quant-body yield lowers to IRFlowNode::Yield");
}
