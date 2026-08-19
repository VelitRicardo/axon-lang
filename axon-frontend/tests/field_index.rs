//! v2.26.0 — field access (`.field` on a non-reference base) + index access
//! (`base[i]`): the structured nodes the JSONB SQL lowering (deferred v2.26.0)
//! consumes. A plain dotted path on a reference stays a flat `Ref` (zero drift).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::{IRConditional, IRExpr, IRFlowNode};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

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
    panic!("no conditional");
}

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog).check().into_iter().map(|e| e.message).collect()
}

fn flow(params: &str, cond: &str) -> String {
    format!("flow F({params}) -> String {{\n  if {cond} {{ probe x }}\n  return \"ok\"\n}}")
}

// ── Index access ────────────────────────────────────────────────────────────

#[test]
fn index_lowers_to_an_index_node() {
    let c = first_conditional(&flow("", "items[0] == \"x\""));
    let e = c.cond.expect("indexing is a rich condition");
    match &e {
        IRExpr::Binary { op, lhs, .. } => {
            assert_eq!(op, "eq");
            assert!(matches!(**lhs, IRExpr::Index { .. }), "lhs must be an Index node");
        }
        other => panic!("expected eq Binary, got {other:?}"),
    }
}

// ── Field access on a non-reference base (chained after an index) ────────────

#[test]
fn field_on_index_lowers_to_field_of_index() {
    let c = first_conditional(&flow("", "items[0].name == \"x\""));
    let e = c.cond.expect("rich");
    match &e {
        IRExpr::Binary { lhs, .. } => match &**lhs {
            IRExpr::Field { base, field } => {
                assert_eq!(field, "name");
                assert!(matches!(**base, IRExpr::Index { .. }), "base of field is the index");
            }
            other => panic!("lhs must be Field(Index), got {other:?}"),
        },
        other => panic!("expected Binary, got {other:?}"),
    }
}

// ── Zero drift: a plain dotted ref stays a flat Ref (legacy) ─────────────────

#[test]
fn plain_dotted_ref_stays_a_ref() {
    let c = first_conditional(&flow("", "user.tier == \"gold\""));
    assert!(c.cond.is_none(), "a plain dotted path is a legacy Ref");
    assert_eq!(c.condition, "user.tier");
}

// ── Type rules ──────────────────────────────────────────────────────────────

#[test]
fn indexing_a_number_is_t814() {
    let errs = errors(&flow("n: Int", "n[0] == 1"));
    assert!(
        errs.iter().any(|e| e.contains("axon-T814")),
        "indexing an Int must be T814: {errs:?}"
    );
}

#[test]
fn index_on_unknown_is_permissive() {
    let errs = errors(&flow("", "items[0] == \"x\""));
    assert!(
        !errs.iter().any(|e| e.contains("axon-T81")),
        "indexing an unknown is permissive: {errs:?}"
    );
}
