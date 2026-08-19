//! v2.26.0 — the closed-catalog builtins (`.length`, `.count`, `.is_empty`,
//! `.is_null`, `.contains`, `.starts_with`, `.ends_with`). Grammar (postfix
//! parse → IR Call), type rules (T813 arity / T814 receiver+arg), and the
//! zero-drift property: a non-builtin dotted name stays a plain `Ref`.

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::{IRExpr, IRConditional, IRFlowNode};
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

// ── Grammar: postfix `.builtin` lowers to an IRExpr::Call ────────────────────

#[test]
fn length_lowers_to_a_call() {
    // The adopter's throttle: `recent.length >= limit`.
    let c = first_conditional(&flow("", "recent.length >= limit"));
    let e = c.cond.expect("a builtin condition is rich");
    match &e {
        IRExpr::Binary { op, lhs, .. } => {
            assert_eq!(op, "ge");
            match &**lhs {
                IRExpr::Call { builtin, args } => {
                    assert_eq!(builtin, "length");
                    assert_eq!(args.len(), 1, "receiver only");
                    assert!(matches!(args[0], IRExpr::Ref { .. }));
                }
                other => panic!("lhs must be a length Call, got {other:?}"),
            }
        }
        other => panic!("expected ge Binary, got {other:?}"),
    }
}

#[test]
fn starts_with_lowers_to_a_call_with_arg() {
    let c = first_conditional(&flow("", "name.starts_with(\"Dr\")"));
    let e = c.cond.expect("rich");
    match &e {
        IRExpr::Call { builtin, args } => {
            assert_eq!(builtin, "starts_with");
            assert_eq!(args.len(), 2, "receiver + 1 arg");
        }
        other => panic!("expected a starts_with Call, got {other:?}"),
    }
}

#[test]
fn non_builtin_dotted_name_stays_a_ref() {
    // ZERO DRIFT: `user.status` is not a builtin → a plain dotted Ref
    // (a legacy ref-cmp-literal shape → cond = None).
    let c = first_conditional(&flow("", "user.status == \"active\""));
    assert!(c.cond.is_none(), "non-builtin dotted ref stays the legacy path");
    assert_eq!(c.condition, "user.status");
}

// ── Type rules: T813 arity, T814 receiver / arg ─────────────────────────────

fn has(errs: &[String], code: &str) -> bool {
    errs.iter().any(|e| e.contains(code))
}

#[test]
fn length_result_is_int_and_clean_in_comparison() {
    // `recent.length >= limit` — length → Int, compared to an unknown ref → clean.
    let errs = errors(&flow("", "recent.length >= limit"));
    assert!(!errs.iter().any(|e| e.contains("axon-T81")), "clean: {errs:?}");
}

#[test]
fn length_on_a_number_is_t814() {
    // `.length` on an Int-typed param is a type error (a number is neither a
    // collection nor a string).
    let errs = errors(&flow("n: Int", "n.length > 0"));
    assert!(has(&errs, "axon-T814"), "`.length` on Int must be T814: {errs:?}");
}

#[test]
fn length_with_an_argument_is_t813_arity() {
    let errs = errors(&flow("", "recent.length(3) > 0"));
    assert!(has(&errs, "axon-T813"), "`.length(3)` wrong arity: {errs:?}");
}

#[test]
fn starts_with_non_string_arg_is_t814() {
    let errs = errors(&flow("name: String", "name.starts_with(3)"));
    assert!(has(&errs, "axon-T814"), "`.starts_with(3)` arg must be string: {errs:?}");
}

#[test]
fn contains_clean_on_unknown_receiver() {
    let errs = errors(&flow("", "items.contains(\"x\")"));
    assert!(!errs.iter().any(|e| e.contains("axon-T81")), "permissive: {errs:?}");
}
