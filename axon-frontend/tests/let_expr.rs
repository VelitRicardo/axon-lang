//! v2.26.0 — `let`-binding values evaluate through the expression engine.
//! A real expression (`price * qty + tax`, `recent.length`) carries a
//! `value_ast` the runtime evaluates; a bare literal / reference / list keeps
//! its pre-v2.26.0 string form (`value_ast = None`, byte-identical).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::{IRExpr, IRFlowNode, IRLetBinding};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn first_let(src: &str) -> IRLetBinding {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    let ir = IRGenerator::new().generate(&prog);
    for flow in &ir.flows {
        for node in &flow.steps {
            if let IRFlowNode::Let(b) = node {
                return b.clone();
            }
        }
    }
    panic!("no let binding in IR");
}

fn flow(let_line: &str) -> String {
    format!("flow F(price: Int, qty: Int, tax: Int) -> String {{\n  {let_line}\n  return \"ok\"\n}}")
}

#[test]
fn arithmetic_let_carries_a_value_ast() {
    let b = first_let(&flow("let total = price * qty + tax"));
    assert_eq!(b.value_kind, "expression");
    let e = b.value_ast.expect("an arithmetic let carries value_ast");
    match &e {
        IRExpr::Binary { op, .. } => assert_eq!(op, "add", "top is the + (mul binds tighter)"),
        other => panic!("expected add Binary, got {other:?}"),
    }
}

#[test]
fn builtin_let_carries_a_value_ast() {
    let b = first_let(&flow("let n = price.is_null"));
    assert_eq!(b.value_kind, "expression");
    assert!(matches!(b.value_ast, Some(IRExpr::Call { .. })));
}

#[test]
fn literal_let_has_no_value_ast() {
    let b = first_let(&flow("let x = 5"));
    assert_eq!(b.value_kind, "literal");
    assert!(b.value_ast.is_none(), "a literal keeps the pre-v2.26.0 string form");
    assert_eq!(b.value, "5");
}

#[test]
fn reference_let_has_no_value_ast() {
    let b = first_let(&flow("let r = price"));
    assert_eq!(b.value_kind, "reference");
    assert!(b.value_ast.is_none());
    assert_eq!(b.value, "price");
}

#[test]
fn list_let_has_no_value_ast() {
    let b = first_let(&flow("let xs = [1, 2, 3]"));
    assert_eq!(b.value_kind, "literal");
    assert!(b.value_ast.is_none(), "a list literal keeps its dedicated path");
}
