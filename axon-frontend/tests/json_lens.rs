//! v2.26.0 — the `Json<T>` shape LENS does field-level checking.
//!
//! 73.a validated the lens TYPE's well-formedness (`T` is a declared
//! struct). 73.e makes the lens WORK: a navigation over a `Json<T>` value
//! is checked against `T`'s declared shape —
//!
//!   * a known field resolves to its declared scalar type, so
//!     `profile.age >= 18` is a well-typed Int comparison and a wrong-typed
//!     use (`not profile.name`) is caught;
//!   * an undeclared field is `axon-T842` (a likely typo);
//!   * a nested struct field continues the lens (`profile.address.city`);
//!   * a bare (open) `Json` is unconstrained — navigation never errors.
//!
//! The runtime is unaffected: the lens is a compile-time EXPECTATION; a
//! declared-but-absent field still degrades to null at runtime, never a
//! crash (doctrine `open_data_is_total` — the compiler may help, the
//! runtime never lies).

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog).check().into_iter().map(|e| e.message).collect()
}

const SHAPES: &str = r#"
    type Address { city: String zip: String }
    type UserEvent { name: String age: Int active: Bool address: Address }
"#;

fn flow(cond: &str) -> String {
    format!(
        "{SHAPES}\nflow F(profile: Json<UserEvent>) -> String {{\n  if {cond} {{ probe x }}\n  return \"ok\"\n}}"
    )
}

// ── A known field resolves to its declared type ──────────────────────────────

#[test]
fn known_field_comparison_type_checks_clean() {
    let errs = errors(&flow("profile.age >= 18"));
    assert!(
        errs.is_empty(),
        "`profile.age >= 18` (age: Int) must be a clean Int comparison: {errs:?}"
    );
}

#[test]
fn lens_field_type_propagates_and_catches_misuse() {
    // `profile.name` resolves to String — `not <String>` is axon-T812.
    let errs = errors(&flow("not profile.name"));
    assert!(
        errs.iter().any(|m| m.contains("axon-T812")),
        "the lens result type (String) must propagate so `not profile.name` is T812: {errs:?}"
    );
}

// ── An undeclared field is axon-T842 ─────────────────────────────────────────

#[test]
fn undeclared_field_is_t842() {
    let errs = errors(&flow("profile.agee >= 18"));
    assert!(
        errs.iter().any(|m| m.contains("axon-T842") && m.contains("agee") && m.contains("UserEvent")),
        "a typo field must be axon-T842 naming the field + the shape: {errs:?}"
    );
}

// ── Nested struct fields continue the lens ───────────────────────────────────

#[test]
fn nested_struct_field_navigation_type_checks_clean() {
    let errs = errors(&flow("profile.address.city == \"Bogotá\""));
    assert!(
        errs.is_empty(),
        "`profile.address.city` (Address.city: String) must be clean: {errs:?}"
    );
}

#[test]
fn nested_undeclared_field_is_t842() {
    let errs = errors(&flow("profile.address.country == \"CO\""));
    assert!(
        errs.iter().any(|m| m.contains("axon-T842") && m.contains("country") && m.contains("Address")),
        "an undeclared nested field must be axon-T842 naming the nested shape: {errs:?}"
    );
}

// ── A bare (open) Json is unconstrained ──────────────────────────────────────

#[test]
fn open_json_navigation_is_permissive() {
    let src = r#"
        flow F(payload: Json) -> String {
            if payload.anything.at.all == "x" { probe p }
            return "ok"
        }
    "#;
    let errs = errors(src);
    assert!(
        !errs.iter().any(|m| m.contains("axon-T842")),
        "open Json navigation must never raise T842: {errs:?}"
    );
}

// ── The lens does not leak across flows / params ─────────────────────────────

#[test]
fn a_non_lens_param_does_not_trigger_field_checking() {
    // `other` is a plain String param — `other.x` is open v2.26.0 navigation,
    // not a lens, so no T842 (and the legacy T814-for-scalars still applies
    // only to KNOWN scalars, which String is not under field access).
    let src = format!(
        "{SHAPES}\nflow F(profile: Json<UserEvent>, other: String) -> String {{\n  if other.x == \"y\" {{ probe p }}\n  return \"ok\"\n}}"
    );
    let errs = errors(&src);
    assert!(
        !errs.iter().any(|m| m.contains("axon-T842")),
        "a non-lens param must not trigger lens field checking: {errs:?}"
    );
}
