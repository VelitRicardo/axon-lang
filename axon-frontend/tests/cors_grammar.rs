//! v2.38.0 — grammar + AST + IR + type-checker for `cors`
//! (the named, referenced browser-origin policy —
//! `docs/cycle/cors_first_class_endpoint_property.md`, axon-enterprise repo).
//!
//! Pinned properties:
//! 1. A full `cors` parses into `CorsDefinition` (every field).
//! 2. `axonendpoint.cors: <Name>` parses into `cors_ref`.
//! 3. It lowers to `IRCors`; absent optionals are ELIDED from the JSON.
//! 4. **IR-SHA invariance**: a program with no `cors` serializes with no
//! `cors_policies` key and no `cors_ref` key — byte-identical to pre-v2.38.0
//! IR (v2.33.0 discipline).
//! 5. A well-formed cors + axonendpoint produces zero diagnostics.
//! 6. **axon-T853** — any-origin `allow_origins` combined with
//!    `allow_credentials: true` (the CORS spec's forbidden pairing).
//! 7. **axon-T854** — an invalid origin glob (multiple wildcards, or a
//!    wildcard outside the leading host label).
//! 8. **axon-T855** — an `allow_methods` value outside the closed catalog.
//! 9. **axon-T856** — `axonendpoint.cors:` references an undeclared symbol.
//! 10. **axon-T857** — two axonendpoints sharing a `path:` disagree on `cors:`.
//! 11. **the design decision** — an unknown field in a `cors { }` block is a hard parse
//!     error (not `shield`'s lenient record-and-skip).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::{ParseError, Parser};
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn try_parse(src: &str) -> Result<axon_frontend::ast::Program, ParseError> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse()
}

fn check_errors(src: &str) -> Vec<String> {
    let prog = parse(src);
    TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

fn first_cors(prog: &axon_frontend::ast::Program) -> &axon_frontend::ast::CorsDefinition {
    prog.declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Cors(c) => Some(c),
            _ => None,
        })
        .expect("no cors declaration")
}

/// Minimal, type-correct flow + endpoint boilerplate (mirrors the
/// `axonendpoint_backend_field.rs` precedent's shape).
const FLOW: &str = "flow Chat() -> Unit { step S { ask: \"hi\" } }\n";

/// The canonical well-formed shape: one cors policy, one endpoint
/// referencing it, no violations.
fn well_formed() -> String {
    format!(
        "{FLOW}\
         cors PublicWebCors {{\n\
         \x20\x20\x20\x20allow_origins: [\"https://app.example.com\", \"https://*.kivi.io\"]\n\
         \x20\x20\x20\x20allow_methods: [GET, POST]\n\
         \x20\x20\x20\x20allow_headers: [\"Content-Type\", \"Authorization\"]\n\
         \x20\x20\x20\x20allow_credentials: true\n\
         \x20\x20\x20\x20max_age: 3600s\n\
         \x20\x20\x20\x20expose_headers: [\"X-Request-Id\"]\n\
         }}\n\
         axonendpoint ChatEndpoint {{ method: POST path: \"/api/chat\" execute: Chat cors: PublicWebCors }}"
    )
}

#[test]
fn cors_parses_into_ast() {
    let src = well_formed();
    let prog = parse(&src);
    let c = first_cors(&prog);

    assert_eq!(c.name, "PublicWebCors");
    assert_eq!(
        c.allow_origins,
        vec!["https://app.example.com".to_string(), "https://*.kivi.io".to_string()]
    );
    assert_eq!(c.allow_methods, vec!["GET".to_string(), "POST".to_string()]);
    assert_eq!(
        c.allow_headers,
        vec!["Content-Type".to_string(), "Authorization".to_string()]
    );
    assert!(c.allow_credentials);
    assert_eq!(c.max_age.as_deref(), Some("3600s"));
    assert_eq!(c.expose_headers, vec!["X-Request-Id".to_string()]);
}

#[test]
fn axonendpoint_cors_ref_parses() {
    let src = well_formed();
    let prog = parse(&src);
    let ep = prog
        .declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::AxonEndpoint(e) => Some(e),
            _ => None,
        })
        .expect("no axonendpoint");
    assert_eq!(ep.cors_ref, "PublicWebCors");
}

#[test]
fn cors_lowers_to_ir_with_elided_optionals() {
    let src = well_formed();
    let prog = parse(&src);
    let ir = IRGenerator::new().generate(&prog);
    let c = ir.cors_policies.first().expect("no cors in IR");

    assert_eq!(c.node_type, "cors");
    assert_eq!(c.name, "PublicWebCors");
    assert_eq!(c.max_age.as_deref(), Some("3600s"));

    let ep = ir.endpoints.first().expect("no endpoint in IR");
    assert_eq!(ep.cors_ref, "PublicWebCors");

    // A declaration that omits expose_headers/max_age elides them.
    let minimal = parse("cors Minimal { allow_origins: [\"https://a.example.com\"] }");
    let ir2 = IRGenerator::new().generate(&minimal);
    let c2 = ir2.cors_policies.first().expect("no cors in IR");
    let json = serde_json::to_string(c2).expect("serialize");
    assert!(!json.contains("\"max_age\""), "absent max_age must elide: {json}");
    assert!(!json.contains("\"expose_headers\""), "absent (empty) expose_headers must elide: {json}");
}

#[test]
fn cors_less_program_has_no_ir_drift() {
    let src = format!("{FLOW}axonendpoint E {{ method: GET path: \"/no-cors\" execute: Chat }}");
    let prog = parse(&src);
    let ir = IRGenerator::new().generate(&prog);
    let json = serde_json::to_string(&ir).expect("serialize");
    assert!(
        !json.contains("\"cors_policies\""),
        "no cors declaration ⇒ no `cors_policies` key in IR JSON (IR-SHA stability): {json}"
    );
    assert!(
        !json.contains("\"cors_ref\""),
        "no cors_ref set ⇒ elided from IR JSON (IR-SHA stability): {json}"
    );
}

#[test]
fn well_formed_cors_produces_no_diagnostics() {
    let errors = check_errors(&well_formed());
    let mine: Vec<_> = errors
        .iter()
        .filter(|m| m.contains("cors") || m.contains("axon-T85"))
        .collect();
    assert!(mine.is_empty(), "expected clean check, got: {mine:?}");
}

#[test]
fn t853_wildcard_origin_with_credentials_is_an_error() {
    let src = "cors Bad { allow_origins: [\"*\"] allow_credentials: true }";
    let errors = check_errors(src);
    assert!(
        errors.iter().any(|m| m.contains("axon-T853")),
        "expected axon-T853, got: {errors:?}"
    );
}

#[test]
fn any_origin_without_credentials_is_fine() {
    let src = "cors Ok { allow_origins: [\"*\"] }";
    let errors = check_errors(src);
    assert!(
        errors.iter().all(|m| !m.contains("axon-T853")),
        "wildcard alone (no credentials) must not trigger T853: {errors:?}"
    );
}

#[test]
fn t854_invalid_origin_glob_shapes_are_errors() {
    for bad_origin in [
        "https://api.*.example.com", // wildcard mid-host
        "https://example.*",         // wildcard trailing, not leading label
        "https://*.*.example.com",   // two wildcards
        "*.example.com",             // no scheme
    ] {
        let src = format!("cors Bad {{ allow_origins: [\"{bad_origin}\"] }}");
        let errors = check_errors(&src);
        assert!(
            errors.iter().any(|m| m.contains("axon-T854")),
            "origin '{bad_origin}' expected axon-T854, got: {errors:?}"
        );
    }
}

#[test]
fn valid_origin_globs_produce_no_t854() {
    let src = "cors Ok { allow_origins: [\"https://exact.example.com\", \"https://*.kivi.io\", \"*\"] }";
    let errors = check_errors(src);
    assert!(
        errors.iter().all(|m| !m.contains("axon-T854")),
        "valid globs must not trigger T854: {errors:?}"
    );
}

#[test]
fn t855_unknown_method_is_an_error() {
    let src = "cors Bad { allow_methods: [GET, TRACE] }";
    let errors = check_errors(src);
    assert!(
        errors.iter().any(|m| m.contains("axon-T855")),
        "expected axon-T855, got: {errors:?}"
    );
}

#[test]
fn t856_undefined_cors_reference_is_an_error() {
    let src = format!(
        "{FLOW}axonendpoint E {{ method: GET path: \"/x\" execute: Chat cors: DoesNotExist }}"
    );
    let errors = check_errors(&src);
    assert!(
        errors.iter().any(|m| m.contains("axon-T856")),
        "expected axon-T856, got: {errors:?}"
    );
}

#[test]
fn t857_cross_method_path_conflict_is_an_error() {
    let src = format!(
        "{FLOW}\
         cors A {{ allow_origins: [\"https://a.example.com\"] }}\n\
         cors B {{ allow_origins: [\"https://b.example.com\"] }}\n\
         axonendpoint Get1 {{ method: GET path: \"/shared\" execute: Chat cors: A }}\n\
         axonendpoint Post1 {{ method: POST path: \"/shared\" execute: Chat cors: B }}"
    );
    let errors = check_errors(&src);
    assert!(
        errors.iter().any(|m| m.contains("axon-T857")),
        "expected axon-T857, got: {errors:?}"
    );
}

#[test]
fn t857_same_cors_on_shared_path_is_fine() {
    let src = format!(
        "{FLOW}\
         cors A {{ allow_origins: [\"https://a.example.com\"] }}\n\
         axonendpoint Get1 {{ method: GET path: \"/shared\" execute: Chat cors: A }}\n\
         axonendpoint Post1 {{ method: POST path: \"/shared\" execute: Chat cors: A }}"
    );
    let errors = check_errors(&src);
    assert!(
        errors.iter().all(|m| !m.contains("axon-T857")),
        "identical cors on a shared path must not conflict: {errors:?}"
    );
}

#[test]
fn t857_both_unset_on_shared_path_is_fine() {
    let src = format!(
        "{FLOW}\
         axonendpoint Get1 {{ method: GET path: \"/shared\" execute: Chat }}\n\
         axonendpoint Post1 {{ method: POST path: \"/shared\" execute: Chat }}"
    );
    let errors = check_errors(&src);
    assert!(
        errors.iter().all(|m| !m.contains("axon-T857")),
        "two cors-less endpoints on a shared path must not conflict: {errors:?}"
    );
}

#[test]
fn d83_7_unknown_cors_field_is_a_hard_parse_error() {
    let src = "cors Bad { allow_origins: [\"https://a.example.com\"] totally_made_up_field: true }";
    let err = try_parse(src).expect_err("unknown cors field must be a hard parse error, not a warning");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("totally_made_up_field") || msg.to_lowercase().contains("unknown"),
        "error should name the bad field: {msg}"
    );
}
