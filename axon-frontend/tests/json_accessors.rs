//! v2.26.0 — the honest coercion accessors (`.as_int` / `.as_float` /
//! `.as_string` / `.as_bool`) join the closed builtin catalog, alongside
//! the v2.26.0 builtins (`.length` / `.count` / `.contains` / `.is_empty` /
//! `.is_null`) now lifted to `Json`. This step pins the FRONTEND
//! surface: the accessors parse as builtin calls (not field paths),
//! type-check to their asserted scalar result type, and take zero
//! arguments (`axon-T813` otherwise). The runtime fail-closing semantics
//! (a type mismatch ⇒ null) are pinned in `axon-rs` orchestration tests.
//!
//! Doctrine `open_data_is_total`: the accessor is the boundary where the
//! program DECLARES the type it expects; the runtime keeps the claim
//! honest (null on mismatch), never a panic.

use axon_frontend::ast::Builtin;
use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::{IRConditional, IRExpr, IRFlowNode};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog).check().into_iter().map(|e| e.message).collect()
}

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

fn flow(cond: &str) -> String {
    format!("flow F(payload: Json) -> String {{\n  if {cond} {{ probe x }}\n  return \"ok\"\n}}")
}

// ── The accessors are in the closed catalog ──────────────────────────────────

#[test]
fn accessors_resolve_as_builtins() {
    assert_eq!(Builtin::from_name("as_int"), Some(Builtin::AsInt));
    assert_eq!(Builtin::from_name("as_float"), Some(Builtin::AsFloat));
    assert_eq!(Builtin::from_name("as_string"), Some(Builtin::AsString));
    assert_eq!(Builtin::from_name("as_bool"), Some(Builtin::AsBool));
    // each takes zero extra arguments
    for b in [Builtin::AsInt, Builtin::AsFloat, Builtin::AsString, Builtin::AsBool] {
        assert_eq!(b.extra_arity(), 0, "{} is a zero-arg accessor", b.surface());
    }
}

// ── They lower to a builtin Call, not a field path ───────────────────────────

#[test]
fn accessor_lowers_to_a_builtin_call() {
    // `payload.age.as_int >= 18` — the lhs is a Call(as_int). Per v2.26.0 the
    // plain dotted base `payload.age` stays a flat Ref (zero drift); the
    // accessor wraps it as the receiver argument.
    let c = first_conditional(&flow("payload.age.as_int >= 18"));
    let e = c.cond.expect("a rich condition");
    match &e {
        IRExpr::Binary { op, lhs, .. } => {
            assert_eq!(op, "ge");
            match &**lhs {
                IRExpr::Call { builtin, args } => {
                    assert_eq!(builtin, "as_int");
                    assert_eq!(args.len(), 1, "receiver only — zero extra args");
                    // The receiver is the (flat-Ref) navigation base.
                    assert!(
                        matches!(&args[0], IRExpr::Ref { .. } | IRExpr::Field { .. }),
                        "receiver is the navigation base, got {:?}",
                        args[0]
                    );
                }
                other => panic!("lhs must be a Call(as_int), got {other:?}"),
            }
        }
        other => panic!("expected ge Binary, got {other:?}"),
    }
}

// ── Type-checking: result type + clean usage ─────────────────────────────────

#[test]
fn as_int_comparison_type_checks_clean() {
    // The accessor asserts Int → `>= 18` is a well-typed numeric comparison.
    let errs = errors(&flow("payload.age.as_int >= 18"));
    assert!(
        errs.is_empty(),
        "an as_int comparison must type-check clean: {errs:?}"
    );
}

#[test]
fn as_string_and_as_bool_type_check_clean() {
    let errs = errors(&flow("payload.name.as_string == \"axon\""));
    assert!(errs.is_empty(), "as_string equality clean: {errs:?}");
    let errs2 = errors(&flow("payload.active.as_bool"));
    assert!(errs2.is_empty(), "as_bool guard clean: {errs2:?}");
}

// ── Arity: the accessors take no arguments ───────────────────────────────────

#[test]
fn accessor_with_an_argument_is_t813() {
    let errs = errors(&flow("payload.age.as_int(5) >= 1"));
    assert!(
        errs.iter().any(|m| m.contains("axon-T813") && m.contains("as_int")),
        "an accessor takes zero args — `.as_int(5)` must be axon-T813: {errs:?}"
    );
}

// ── v2.26.0 builtins still type-check over a Json receiver ───────────────────────

#[test]
fn length_and_contains_over_json_type_check_clean() {
    let errs = errors(&flow("payload.items.length >= 1"));
    assert!(errs.is_empty(), "`.length` over Json clean: {errs:?}");
    let errs2 = errors(&flow("payload.tags.contains(\"vip\")"));
    assert!(errs2.is_empty(), "`.contains` over Json clean: {errs2:?}");
    let errs3 = errors(&flow("payload.profile.is_null"));
    assert!(errs3.is_empty(), "`.is_null` over Json clean: {errs3:?}");
}
