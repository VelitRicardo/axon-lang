//! v2.26.0 — `Json` as a first-class type + the `Json<T>` shape lens
//! at the catalog level (the type-system catalog + the `axonstore`
//! column catalog). This step establishes that:
//!
//!   * a bare `Json` is a recognized, open, first-class value type —
//!     it never raises a spurious "unknown type" or `axon-T81x`;
//!   * `Json<T>` parses as a refined lens both as a type annotation
//!     (flow param / return / `type` field) and as an `axonstore`
//!     column, and is WELL-FORMED only when `T` names a declared struct
//!     `type` — otherwise `axon-T840`;
//!   * a `<T>` shape lens may refine ONLY a `Json` / `Jsonb` column —
//!     applying it to any other column type is `axon-T841`.
//!
//! Runtime navigation + the honest accessors + the field-level lens
//! checking are later steps (73.b / 73.c / 73.e); this is the
//! frontend catalog floor only. Doctrine: `open_data_is_total` — open
//! navigation is always total; the lens is a compile-time expectation
//! the compiler checks, never an enforced runtime guarantee.

use axon_frontend::epistemic;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

/// Type-check error messages (lex + parse must succeed).
fn tc_errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog).check().into_iter().map(|e| e.message).collect()
}

/// Parse result — `Ok(())` on a clean parse, `Err(message)` otherwise.
/// Used for the `axon-T841` structural lens rule, which is a PARSE-time
/// diagnostic (it needs no symbol table).
fn parse_result(src: &str) -> Result<(), String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    match Parser::new(tokens).parse() {
        Ok(_) => Ok(()),
        Err(e) => Err(e.message),
    }
}

// ── `Json` is a first-class builtin type ─────────────────────────────────────

#[test]
fn json_is_a_recognized_builtin_type() {
    assert!(
        epistemic::builtin_types().contains("Json"),
        "Json must be a recognized first-class builtin type"
    );
}

#[test]
fn bare_json_param_type_checks_clean() {
    let errs = tc_errors("flow F(payload: Json) -> String {\n  return \"ok\"\n}");
    assert!(
        errs.is_empty(),
        "a bare `Json` param is the open default and must type-check clean: {errs:?}"
    );
}

// ── `Json<T>` lens as a type annotation ──────────────────────────────────────

#[test]
fn json_lens_with_declared_struct_is_well_formed() {
    let src = r#"
        type UserEvent { name: String age: Int }
        flow F(profile: Json<UserEvent>) -> String { return "ok" }
    "#;
    let errs = tc_errors(src);
    assert!(
        !errs.iter().any(|m| m.contains("axon-T840")),
        "Json<UserEvent> with UserEvent a declared `type` is well-formed: {errs:?}"
    );
}

#[test]
fn json_lens_with_undeclared_shape_is_t840() {
    let errs = tc_errors("flow F(profile: Json<Nope>) -> String { return \"ok\" }");
    assert!(
        errs.iter().any(|m| m.contains("axon-T840") && m.contains("Nope")),
        "Json<Nope> where Nope is undeclared must be axon-T840 naming `Nope`: {errs:?}"
    );
}

#[test]
fn json_lens_over_a_non_type_symbol_is_t840() {
    // `Greet` is a flow, not a struct `type` — an illegitimate lens shape.
    let src = r#"
        flow Greet() -> String { return "hi" }
        flow F(p: Json<Greet>) -> String { return "ok" }
    "#;
    let errs = tc_errors(src);
    assert!(
        errs.iter().any(|m| m.contains("axon-T840") && m.contains("flow")),
        "Json<Greet> over a flow symbol must be axon-T840 naming the wrong kind: {errs:?}"
    );
}

#[test]
fn json_lens_over_a_builtin_scalar_is_t840() {
    // A builtin scalar is not a struct whose fields are a shape.
    let errs = tc_errors("flow F(p: Json<String>) -> String { return \"ok\" }");
    assert!(
        errs.iter().any(|m| m.contains("axon-T840")),
        "Json<String> (a non-struct builtin) must be axon-T840: {errs:?}"
    );
}

#[test]
fn json_lens_in_a_type_field_is_validated() {
    let src = r#"
        type Wrap { p: Json<Nope> }
    "#;
    let errs = tc_errors(src);
    assert!(
        errs.iter().any(|m| m.contains("axon-T840") && m.contains("Nope")),
        "a `Json<Nope>` field inside a `type` must be axon-T840: {errs:?}"
    );
}

// ── `Json` / `Jsonb` columns in the store catalog ────────────────────────────

#[test]
fn bare_json_and_jsonb_columns_type_check_clean() {
    let src = r#"
        axonstore Events {
            backend: postgresql
            connection: "env:DB"
            schema {
                id:      Uuid primary_key
                payload: Json
                raw:     Jsonb
            }
        }
    "#;
    let errs = tc_errors(src);
    assert!(
        !errs.iter().any(|m| m.contains("axon-T84")),
        "bare Json/Jsonb columns are open + valid: {errs:?}"
    );
}

#[test]
fn json_lens_column_with_declared_struct_is_well_formed() {
    let src = r#"
        type UserEvent { name: String }
        axonstore Events {
            backend: postgresql
            connection: "env:DB"
            schema {
                id:      Uuid primary_key
                profile: Json<UserEvent>
            }
        }
    "#;
    let errs = tc_errors(src);
    assert!(
        !errs.iter().any(|m| m.contains("axon-T840")),
        "a Json<UserEvent> column with a declared struct is well-formed: {errs:?}"
    );
}

#[test]
fn json_lens_column_with_undeclared_shape_is_t840() {
    let src = r#"
        axonstore Events {
            backend: postgresql
            connection: "env:DB"
            schema {
                id:      Uuid primary_key
                profile: Json<Nope>
            }
        }
    "#;
    let errs = tc_errors(src);
    assert!(
        errs.iter().any(|m| m.contains("axon-T840") && m.contains("Nope")),
        "a Json<Nope> column must be axon-T840: {errs:?}"
    );
}

// ── `axon-T841` — a `<T>` lens only refines a Json/Jsonb column ───────────────

#[test]
fn shape_lens_on_a_non_json_column_is_t841() {
    let src = r#"
        type UserEvent { name: String }
        axonstore Events {
            backend: postgresql
            connection: "env:DB"
            schema {
                id:   Uuid primary_key
                name: Text<UserEvent>
            }
        }
    "#;
    match parse_result(src) {
        Err(msg) => assert!(
            msg.contains("axon-T841"),
            "a `<T>` lens on a Text column must be a parse-time axon-T841: {msg}"
        ),
        Ok(()) => panic!("expected axon-T841 parse error for `Text<UserEvent>`"),
    }
}

#[test]
fn shape_lens_on_a_jsonb_column_parses() {
    let src = r#"
        type UserEvent { name: String }
        axonstore Events {
            backend: postgresql
            connection: "env:DB"
            schema {
                id:  Uuid primary_key
                raw: Jsonb<UserEvent>
            }
        }
    "#;
    assert!(
        parse_result(src).is_ok(),
        "a `Jsonb<T>` lens is structurally valid and must parse"
    );
}
