//! v2.44.0 — the `axonendpoint public:` authorization-coverage opt-out.
//!
//! Doctrine `every_boundary_is_guarded` (sister of v2.33.0 `authority_is_declared`).
//! The boundary-coverage audit found Modo 1: an `axonendpoint` with no
//! `requires:`/`shield:`/`compliance:` silently dispatches to any authenticated
//! same-tenant caller ("Empty vec means 'no auth gate' — back-compat"). v2.44.0
//! closes that by making the un-covered state an EXPLICIT, auditable
//! `public: true` — and (in v2.44.0) a hard error when neither coverage nor
//! `public` is declared.
//!
//! 89.a lands the GRAMMAR only (the field + its parse + its IR lowering). The
//! coverage RULE that reads it (`axon-T890`) is v2.44.0; this file pins that the
//! field round-trips and defaults correctly, with zero effect on existing
//! programs.
//!
//! Pins:
//!   1. `public: true` is captured into the AST.
//!   2. `public: false` is captured (explicit opt-in to coverage).
//!   3. An omitted `public:` defaults to `false` (back-compat — the field is
//! inert until v2.44.0 reads it).
//!   4. The field lowers into the IR (`IRAxonEndpoint.public`).
//! 5. `public: false` (the default) ELIDES from the IR-JSON — a pre-v2.44.0
//! snapshot stays byte-identical (zero IR-SHA drift, the v2.33.0 discipline).
//!   6. `public: true` IS serialized in the IR-JSON.
//!   7. A non-bool `public:` value is a parse error.

use axon_frontend::ast::{Declaration, Program};
use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::{ParseError, Parser};

fn parse(src: &str) -> Result<Program, ParseError> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse()
}

fn endpoint<'a>(prog: &'a Program, name: &str) -> &'a axon_frontend::ast::AxonEndpointDefinition {
    for decl in &prog.declarations {
        if let Declaration::AxonEndpoint(ae) = decl {
            if ae.name == name {
                return ae;
            }
        }
    }
    panic!("axonendpoint {name} not found");
}

/// A minimal, type-correct program: one flow + one endpoint that references
/// it. The `public:` field is interpolated so each test can vary it.
fn program_with_public(public_field: &str) -> String {
    format!(
        "flow Chat() -> Unit {{ step S {{ ask: \"hi\" }} }}\n\
         axonendpoint E {{ method: POST path: \"/chat\" execute: Chat {public_field} }}"
    )
}

fn ir_json(src: &str) -> String {
    let prog = parse(src).expect("parse");
    let ir = IRGenerator::new().generate(&prog);
    serde_json::to_string(&ir).expect("serialize IR")
}

// ─── section 1/section 2 — the field is captured ─────────────────────────────────

#[test]
fn s1_public_true_is_captured() {
    let prog = parse(&program_with_public("public: true")).expect("parse");
    assert!(
        endpoint(&prog, "E").public,
        "89.a: `public: true` must be captured into the AST"
    );
}

#[test]
fn s2_public_false_is_captured() {
    let prog = parse(&program_with_public("public: false")).expect("parse");
    assert!(
        !endpoint(&prog, "E").public,
        "89.a: `public: false` must be captured (explicit opt-in to coverage)"
    );
}

// ─── section 3 — omission defaults to false (back-compat) ─────────────────

#[test]
fn s3_omitted_public_defaults_false() {
    let prog = parse(&program_with_public("")).expect("parse");
    assert!(
        !endpoint(&prog, "E").public,
        "89.a: an omitted `public:` must default to false — inert until v2.44.0"
    );
}

// ─── section 4 — the field lowers into the IR ─────────────────────────────

#[test]
fn s4_public_lowers_into_ir() {
    let prog = parse(&program_with_public("public: true")).expect("parse");
    let ir = IRGenerator::new().generate(&prog);
    let ep = ir
        .endpoints
        .iter()
        .find(|e| e.name == "E")
        .expect("endpoint in IR");
    assert!(ep.public, "89.a: `public: true` must lower into IRAxonEndpoint");
}

// ─── section 5/section 6 — IR-JSON serialization discipline ──────────────────────

#[test]
fn s5_public_false_elides_from_ir_json() {
    let json = ir_json(&program_with_public("")); // default false
    assert!(
        !json.contains("\"public\""),
        "89.a: the default `public: false` must ELIDE from IR-JSON (zero IR-SHA \
         drift for pre-v2.44.0 programs). Got: {json}"
    );
}

#[test]
fn s6_public_true_is_serialized() {
    let json = ir_json(&program_with_public("public: true"));
    assert!(
        json.contains("\"public\":true"),
        "89.a: `public: true` must be serialized into the IR-JSON. Got: {json}"
    );
}

// ─── section 7 — a non-bool value is a parse error ────────────────────────

#[test]
fn s7_non_bool_public_is_a_parse_error() {
    let err = parse(&program_with_public("public: maybe"));
    assert!(
        err.is_err(),
        "89.a: `public:` accepts only a bool literal — a non-bool must not parse"
    );
}
