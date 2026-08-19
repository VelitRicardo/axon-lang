//! v2.26.0 — the pure expression engine: the Pratt parser + the zero-drift
//! legacy classifier. A condition that fits the pre-v2.26.0 `(cond, op, value)` +
//! `or` shape keeps `cond = None` (byte-identical IR + eval); a richer condition
//! (`and`, `not`, arithmetic, parens, nesting) lowers to a `cond` expression.

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::{IRExpr, IRConditional, IRFlowNode};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn first_conditional(src: &str) -> IRConditional {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    let ir = IRGenerator::new().generate(&prog);
    for flow in &ir.flows {
        for node in &flow.steps {
            if let IRFlowNode::Conditional(c) = node {
                return c.clone();
            }
        }
    }
    panic!("no conditional in IR");
}

fn wrap(cond_src: &str) -> String {
    format!("flow F() -> String {{\n  if {cond_src} {{ probe x }}\n  return \"ok\"\n}}")
}

// ── Legacy-shaped conditions stay on the legacy path (cond = None) ──────────

#[test]
fn bare_ref_is_legacy() {
    let c = first_conditional(&wrap("ready"));
    assert!(c.cond.is_none(), "a bare ref must keep the legacy path");
    assert_eq!(c.condition, "ready");
    assert_eq!(c.comparison_op, "");
}

#[test]
fn ref_cmp_literal_is_legacy() {
    let c = first_conditional(&wrap("count >= 3"));
    assert!(c.cond.is_none(), "ref-cmp-literal must keep the legacy triple");
    assert_eq!(c.condition, "count");
    assert_eq!(c.comparison_op, ">=");
    assert_eq!(c.comparison_value, "3");
}

#[test]
fn string_eq_is_legacy() {
    let c = first_conditional(&wrap("tier == \"premium\""));
    assert!(c.cond.is_none());
    assert_eq!(c.condition, "tier");
    assert_eq!(c.comparison_op, "==");
    assert_eq!(c.comparison_value, "premium");
}

#[test]
fn or_chain_is_legacy() {
    let c = first_conditional(&wrap("a == \"x\" or b == \"y\""));
    assert!(c.cond.is_none(), "an or-chain of triples must stay legacy");
    assert_eq!(c.condition, "a");
    assert_eq!(c.conjunctor, "or");
    assert_eq!(c.conditions.len(), 1);
    assert_eq!(c.conditions[0], ("b".into(), "==".into(), "y".into()));
}

#[test]
fn dotted_ref_is_legacy() {
    let c = first_conditional(&wrap("User.tier == \"gold\""));
    assert!(c.cond.is_none());
    assert_eq!(c.condition, "User.tier");
}

// ── Richer conditions lower to the expression form (cond = Some) ────────────

fn op_of(e: &IRExpr) -> &str {
    match e {
        IRExpr::Binary { op, .. } => op,
        IRExpr::Unary { op, .. } => op,
        _ => "<not-op>",
    }
}

#[test]
fn and_is_an_expression() {
    let c = first_conditional(&wrap("a and b"));
    let e = c.cond.expect("and must lower to a cond expression");
    assert_eq!(op_of(&e), "and");
}

#[test]
fn not_is_an_expression() {
    let c = first_conditional(&wrap("not ready"));
    let e = c.cond.expect("not must lower to a cond expression");
    assert_eq!(op_of(&e), "not");
}

#[test]
fn arithmetic_comparison_is_an_expression() {
    // `(a + b) * 2 > c` — arithmetic the legacy triple cannot express.
    let c = first_conditional(&wrap("(a + b) * 2 > c"));
    let e = c.cond.expect("arithmetic must lower to a cond expression");
    assert_eq!(op_of(&e), "gt");
}

#[test]
fn precedence_or_binds_looser_than_and() {
    // `a or b and c` must parse as `a or (b and c)`.
    let c = first_conditional(&wrap("a or b and c"));
    let e = c.cond.expect("mixed and/or is a rich expression");
    match &e {
        IRExpr::Binary { op, lhs, rhs } => {
            assert_eq!(op, "or", "top operator must be `or`");
            assert!(matches!(**lhs, IRExpr::Ref { .. }), "lhs of or is `a`");
            assert_eq!(op_of(rhs), "and", "rhs of or must be the `and` subtree");
        }
        _ => panic!("expected a Binary or-expression"),
    }
}

#[test]
fn precedence_mul_binds_tighter_than_add() {
    // `a + b * c == d` → `(a + (b * c)) == d`; top is `eq`, lhs is `add`.
    let c = first_conditional(&wrap("a + b * c == d"));
    let e = c.cond.expect("rich arithmetic");
    match &e {
        IRExpr::Binary { op, lhs, .. } => {
            assert_eq!(op, "eq");
            assert_eq!(op_of(lhs), "add", "lhs must be the add subtree");
            if let IRExpr::Binary { rhs, .. } = &**lhs {
                assert_eq!(op_of(rhs), "mul", "add's rhs must be the mul subtree");
            }
        }
        _ => panic!("expected eq at the top"),
    }
}
