//! v2.26.0 — static type-checking of pure expressions in `if` conditions.
//! `axon-T810` (non-numeric arithmetic), `axon-T811` (incompatible comparison),
//! `axon-T812` (non-boolean to `and`/`or`/`not`). Only RICH conditions
//! (`cond = Some`) are checked; the legacy triple shape is unaffected (zero
//! drift). A reference of unknown static type is permissive (no false positive).

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

/// Wrap a condition in a flow (optionally with typed params).
fn flow(params: &str, cond: &str) -> String {
    format!("flow F({params}) -> String {{\n  if {cond} {{ probe x }}\n  return \"ok\"\n}}")
}

fn has(errs: &[String], code: &str) -> bool {
    errs.iter().any(|e| e.contains(code))
}

// ── T810 — non-numeric operand to an arithmetic operator ────────────────────

#[test]
fn string_literal_in_arithmetic_is_t810() {
    let errs = errors(&flow("", "\"hello\" + 3 > 0"));
    assert!(has(&errs, "axon-T810"), "string + int must be T810: {errs:?}");
}

#[test]
fn negate_string_is_t810() {
    let errs = errors(&flow("", "-\"x\" > 0"));
    assert!(has(&errs, "axon-T810"), "negating a string must be T810: {errs:?}");
}

#[test]
fn param_typed_arithmetic_mismatch_is_t810() {
    // `count` is Int; `count * "x"` mixes Int and String.
    let errs = errors(&flow("count: Int", "count * \"x\" > 0"));
    assert!(has(&errs, "axon-T810"), "Int * String must be T810: {errs:?}");
}

// ── T811 — incompatible comparison ──────────────────────────────────────────

#[test]
fn order_string_against_int_is_t811() {
    let errs = errors(&flow("", "\"a\" < 3"));
    assert!(has(&errs, "axon-T811"), "String < Int must be T811: {errs:?}");
}

#[test]
fn equality_across_incompatible_types_is_t811() {
    let errs = errors(&flow("", "5 == \"x\""));
    assert!(has(&errs, "axon-T811"), "Int == String must be T811: {errs:?}");
}

// ── T812 — non-boolean operand to a boolean operator ────────────────────────

#[test]
fn integer_in_and_is_t812() {
    let errs = errors(&flow("", "true and 5"));
    assert!(has(&errs, "axon-T812"), "`true and 5` must be T812: {errs:?}");
}

#[test]
fn not_on_integer_is_t812() {
    let errs = errors(&flow("", "not 5"));
    assert!(has(&errs, "axon-T812"), "`not 5` must be T812: {errs:?}");
}

// ── No false positives ──────────────────────────────────────────────────────

#[test]
fn unknown_refs_are_permissive() {
    // `a` / `b` are not in scope → Unknown → no type error.
    let errs = errors(&flow("", "a and b"));
    assert!(
        !errs.iter().any(|e| e.contains("axon-T81")),
        "unknown refs must not error: {errs:?}"
    );
}

#[test]
fn well_typed_param_arithmetic_is_clean() {
    // `count + 1 > 0` over an Int param — well typed.
    let errs = errors(&flow("count: Int", "count + 1 > 0"));
    assert!(
        !errs.iter().any(|e| e.contains("axon-T81")),
        "well-typed arithmetic must be clean: {errs:?}"
    );
}

#[test]
fn legacy_condition_is_not_type_checked() {
    // `count == "x"` is a legacy ref-cmp-literal shape (cond = None) → not
    // type-checked (zero drift). No T81x even though it would be incompatible.
    let errs = errors(&flow("count: Int", "count == \"x\""));
    assert!(
        !errs.iter().any(|e| e.contains("axon-T81")),
        "legacy conditions stay unchecked: {errs:?}"
    );
}

#[test]
fn well_typed_boolean_combo_is_clean() {
    let errs = errors(&flow("active: Bool", "active and not active"));
    assert!(
        !errs.iter().any(|e| e.contains("axon-T81")),
        "bool and not bool must be clean: {errs:?}"
    );
}
