//! v2.46.0 — grammar + AST + IR + type-checker for `credential`
//! (the ephemeral-credential contract) and `mint` (the minting flow verb) —
//! `docs/cycle/ephemeral_visitor_credentials.md`, axon-enterprise repo.
//!
//! Pinned properties:
//! 1. A full `credential` parses into `CredentialDefinition` (ttl + grants).
//! 2. `mint <Credential> as <binding>` parses into `MintStep`.
//! 3. Both lower to IR (`IRCredential` with ttl in SECONDS; `IRMintStep`);
//!    a credential-less program's IR JSON has no `credentials` key
//!    (IR-SHA stability).
//! 4. A well-formed contract + mint produces zero diagnostics.
//! 5. **axon-T893** — empty `grants:`.
//! 6. **axon-T894** — invalid / zero / above-24h-ceiling `ttl:`.
//! 7. **axon-T895** — `mint` references an undeclared (or wrong-kind) symbol.
//! 8. **axon-T896** — a mint binding flows into a `persist` payload.
//! 9. An unknown field in `credential { }` is a HARD PARSE ERROR (v2.38.0
//!    posture); an invalid grant slug is a parse error (the `requires:`
//!    grammar); a `mint` without `as <binding>` is a parse error.

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

fn first_credential(
    prog: &axon_frontend::ast::Program,
) -> &axon_frontend::ast::CredentialDefinition {
    prog.declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Credential(c) => Some(c),
            _ => None,
        })
        .expect("no credential declaration")
}

const WELL_FORMED: &str = "credential WidgetSession {\n\
    ttl:    15m\n\
    grants: [chat.invoke, flow.execute]\n\
}\n\
flow BootstrapWidget() -> Unit {\n\
    mint WidgetSession as tok\n\
    step Compose { ask: \"Compose the widget bootstrap payload for ${tok}.\" }\n\
}\n";

#[test]
fn credential_parses_into_ast() {
    let prog = parse(WELL_FORMED);
    let c = first_credential(&prog);
    assert_eq!(c.name, "WidgetSession");
    assert_eq!(c.ttl, "15m");
    assert_eq!(
        c.grants,
        vec!["chat.invoke".to_string(), "flow.execute".to_string()]
    );
}

#[test]
fn mint_parses_into_ast() {
    let prog = parse(WELL_FORMED);
    let mint = prog
        .declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Flow(f) => f.body.iter().find_map(|s| match s {
                axon_frontend::ast::FlowStep::Mint(m) => Some(m),
                _ => None,
            }),
            _ => None,
        })
        .expect("no mint step");
    assert_eq!(mint.credential_ref, "WidgetSession");
    assert_eq!(mint.binding, "tok");
}

#[test]
fn credential_and_mint_lower_to_ir_with_ttl_in_seconds() {
    let prog = parse(WELL_FORMED);
    let ir = IRGenerator::new().generate(&prog);
    let c = ir.credentials.first().expect("no credential in IR");
    assert_eq!(c.name, "WidgetSession");
    assert_eq!(c.ttl_secs, 900, "15m = 900 seconds");
    assert_eq!(c.grants, vec!["chat.invoke", "flow.execute"]);
    let json = serde_json::to_string(&ir).expect("serialize");
    assert!(json.contains("\"mint\""), "mint node in IR: {json}");
    assert!(json.contains("\"credential_ref\":\"WidgetSession\""), "{json}");
    assert!(json.contains("\"binding\":\"tok\""), "{json}");
}

#[test]
fn credential_less_program_has_no_ir_drift() {
    let src = "flow Chat() -> Unit { step S { ask: \"hi\" } }\n";
    let ir = IRGenerator::new().generate(&parse(src));
    let json = serde_json::to_string(&ir).expect("serialize");
    assert!(
        !json.contains("\"credentials\""),
        "no credential ⇒ no `credentials` key in IR JSON (IR-SHA stability): {json}"
    );
}

#[test]
fn well_formed_produces_no_diagnostics() {
    let errors = check_errors(WELL_FORMED);
    let mine: Vec<_> = errors
        .iter()
        .filter(|m| m.contains("credential") || m.contains("axon-T89"))
        .collect();
    assert!(mine.is_empty(), "expected clean check, got: {mine:?}");
}

#[test]
fn t893_empty_grants_is_an_error() {
    let src = "credential Dead { ttl: 15m }";
    let errors = check_errors(src);
    assert!(
        errors.iter().any(|m| m.contains("axon-T893")),
        "expected axon-T893, got: {errors:?}"
    );
}

#[test]
fn t894_ttl_laws() {
    // Zero-length TTL.
    let errors = check_errors("credential Z { ttl: 0s grants: [chat.invoke] }");
    assert!(errors.iter().any(|m| m.contains("axon-T894")), "{errors:?}");
    // Above the 24h ceiling.
    let errors = check_errors("credential Long { ttl: 2d grants: [chat.invoke] }");
    assert!(
        errors.iter().any(|m| m.contains("axon-T894") && m.contains("24h")),
        "{errors:?}"
    );
    // Exactly 24h is admitted.
    let errors = check_errors("credential Day { ttl: 24h grants: [chat.invoke] }");
    assert!(
        errors.iter().all(|m| !m.contains("axon-T894")),
        "24h must pass: {errors:?}"
    );
    // Missing ttl (empty string) is an error.
    let errors = check_errors("credential NoTtl { grants: [chat.invoke] }");
    assert!(errors.iter().any(|m| m.contains("axon-T894")), "{errors:?}");
}

#[test]
fn t895_undeclared_or_wrong_kind_reference() {
    let src = "flow F() -> Unit { mint Ghost as tok\n step S { ask: \"hi\" } }";
    let errors = check_errors(src);
    assert!(
        errors.iter().any(|m| m.contains("axon-T895") && m.contains("Ghost")),
        "expected axon-T895 (undeclared), got: {errors:?}"
    );
    // Wrong kind: a cors declaration is not a credential.
    let src = "cors NotACred { allow_origins: [\"*\"] }\n\
               flow F() -> Unit { mint NotACred as tok\n step S { ask: \"hi\" } }";
    let errors = check_errors(src);
    assert!(
        errors.iter().any(|m| m.contains("axon-T895") && m.contains("not a credential")),
        "expected axon-T895 (wrong kind), got: {errors:?}"
    );
}

#[test]
fn t896_mint_binding_never_enters_a_store() {
    let src = "credential WidgetSession { ttl: 15m grants: [chat.invoke] }\n\
               axonstore Tokens { backend: postgresql schema { value: Text } }\n\
               flow Leak() -> Unit {\n\
                   mint WidgetSession as tok\n\
                   persist into Tokens { value: \"${tok}\" }\n\
               }\n";
    let errors = check_errors(src);
    assert!(
        errors.iter().any(|m| m.contains("axon-T896")),
        "expected axon-T896, got: {errors:?}"
    );
}

#[test]
fn unknown_credential_field_is_a_hard_parse_error() {
    let err = try_parse("credential Bad { ttl: 15m grantz: [chat.invoke] }")
        .expect_err("typo'd field must be a hard parse error");
    assert!(
        err.message.contains("unknown credential field `grantz`"),
        "unexpected parse error: {}",
        err.message
    );
}

#[test]
fn invalid_grant_slug_is_a_parse_error() {
    let err = try_parse("credential Bad { ttl: 15m grants: [chat.invoke, Tenant.Write] }")
        .expect_err("colon/uppercase slug must be a parse error");
    assert!(
        err.message.contains("Invalid capability slug"),
        "unexpected parse error: {}",
        err.message
    );
}

#[test]
fn mint_without_binding_is_a_parse_error() {
    let src = "credential C { ttl: 15m grants: [chat.invoke] }\n\
               flow F() -> Unit { mint C\n step S { ask: \"hi\" } }";
    assert!(
        try_parse(src).is_err(),
        "`mint C` without `as <binding>` must not parse"
    );
}
