//! v2.48.0 — grammar + AST + IR + type-checker for the `backend:
//! secrets` metadata store (doctrine `rotation_without_revelation`) —
//! `the design plan`, axon-enterprise repo.
//!
//! Pinned properties:
//! 1. `axonstore X { backend: secrets  class: crm }` parses; AST carries
//!    `backend == "secrets"` + `class == "crm"`.
//! 2. A well-formed secrets store + a `retrieve` with a v2.21.0 time-aware
//!    `where:` over the SYNTHESIZED metadata schema produces zero
//!    diagnostics.
//! 3. **axon-T900** — missing `class:`; invalid class shape; explicit
//!    `schema` block; adopter-storage fields (`connection:` …); `class:`
//!    on a non-secrets backend.
//! 4. **axon-T897** — `persist` / `mutate` / `purge` against a secrets
//!    store are unrepresentable.
//! 5. The v1.31.0 proof runs against the synthesized schema: an unknown
//!    column in `where:` is rejected.
//! 6. IR: `class` rides the store node; the FIXED metadata schema is
//!    synthesized into `column_schema` (self-describing artifact); a
//!    non-secrets store's IR JSON has no `class` key (IR-SHA stability).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn check_errors(src: &str) -> Vec<String> {
    let prog = parse(src);
    TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

fn first_store(
    prog: &axon_frontend::ast::Program,
) -> &axon_frontend::ast::AxonStoreDefinition {
    prog.declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::AxonStore(s) => Some(s),
            _ => None,
        })
        .expect("no axonstore declaration")
}

const WELL_FORMED: &str = "axonstore CrmTokens {\n\
    backend: secrets\n\
    class: crm\n\
}\n\
flow ListExpiring() -> Unit {\n\
    retrieve CrmTokens { where: \"expires_at < now() + interval '10 minutes'\" as: expiring }\n\
    step Report { ask: \"Summarize which connections near expiry: ${expiring}.\" }\n\
}\n";

// ── 1 + 2: the well-formed shape ────────────────────────────────────

#[test]
fn secrets_store_parses_with_class() {
    let prog = parse(WELL_FORMED);
    let s = first_store(&prog);
    assert_eq!(s.name, "CrmTokens");
    assert_eq!(s.backend, "secrets");
    assert_eq!(s.class, "crm");
    assert!(s.column_schema.is_none(), "schema is synthesized, never parsed");
}

#[test]
fn well_formed_secrets_store_and_time_aware_retrieve_are_clean() {
    let errors = check_errors(WELL_FORMED);
    assert!(errors.is_empty(), "expected zero diagnostics, got: {errors:?}");
}

#[test]
fn dotted_class_is_accepted() {
    let src = "axonstore OauthTokens {\n backend: secrets\n class: crm.oauth\n}\n";
    let errors = check_errors(src);
    assert!(errors.is_empty(), "dotted class must be valid: {errors:?}");
}

// ── 3: axon-T900 placement laws ─────────────────────────────────────

#[test]
fn t900_missing_class_is_rejected() {
    let src = "axonstore Tokens {\n backend: secrets\n}\n";
    let errors = check_errors(src);
    assert!(
        errors.iter().any(|e| e.contains("axon-T900") && e.contains("without a")),
        "expected axon-T900 missing-class, got: {errors:?}"
    );
}

#[test]
fn t900_invalid_class_shape_is_rejected() {
    let src = "axonstore Tokens {\n backend: secrets\n class: Crm\n}\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T900") && e.contains("invalid secret class")),
        "expected axon-T900 invalid-class, got: {errors:?}"
    );
}

#[test]
fn t900_explicit_schema_on_secrets_store_is_rejected() {
    let src = "axonstore Tokens {\n\
        backend: secrets\n\
        class: crm\n\
        schema { key: Text }\n\
    }\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T900") && e.contains("synthesized by the compiler")),
        "expected axon-T900 explicit-schema, got: {errors:?}"
    );
}

#[test]
fn t900_adopter_storage_fields_on_secrets_store_are_rejected() {
    let src = "axonstore Tokens {\n\
        backend: secrets\n\
        class: crm\n\
        connection: \"postgres://…\"\n\
    }\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T900") && e.contains("adopter-storage fields")),
        "expected axon-T900 adopter-storage-fields, got: {errors:?}"
    );
}

#[test]
fn t900_class_on_non_secrets_backend_is_rejected() {
    let src = "axonstore Sessions {\n backend: postgresql\n class: crm\n}\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T900") && e.contains("no meaning elsewhere")),
        "expected axon-T900 class-misplacement, got: {errors:?}"
    );
}

// ── 4: axon-T897 read-only law ──────────────────────────────────────

#[test]
fn t897_persist_into_secrets_store_is_rejected() {
    let src = "axonstore CrmTokens {\n backend: secrets\n class: crm\n}\n\
        flow Leak(v: String) -> Unit {\n\
            persist into CrmTokens { key: \"crm.hubspot\" }\n\
        }\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T897") && e.contains("`persist`")),
        "expected axon-T897 on persist, got: {errors:?}"
    );
}

#[test]
fn t897_mutate_and_purge_against_secrets_store_are_rejected() {
    let src = "axonstore CrmTokens {\n backend: secrets\n class: crm\n}\n\
        flow Tamper() -> Unit {\n\
            mutate CrmTokens { version: \"9\", where: \"key = 'crm.hubspot'\" }\n\
            purge CrmTokens { where: \"key = 'crm.hubspot'\" }\n\
        }\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T897") && e.contains("`mutate`")),
        "expected axon-T897 on mutate, got: {errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T897") && e.contains("`purge`")),
        "expected axon-T897 on purge, got: {errors:?}"
    );
}

// ── 5: the v1.31.0 proof runs against the synthesized schema ──────────

#[test]
fn unknown_column_in_where_is_rejected_against_synthesized_schema() {
    let src = "axonstore CrmTokens {\n backend: secrets\n class: crm\n}\n\
        flow Bad() -> Unit {\n\
            retrieve CrmTokens { where: \"access_token = 'x'\" as: rows }\n\
            step S { ask: \"${rows}\" }\n\
        }\n";
    let errors = check_errors(src);
    assert!(
        errors.iter().any(|e| e.contains("access_token")),
        "expected a v1.31.0 unknown-column error naming `access_token` — the \
         secret VALUE has no column by design — got: {errors:?}"
    );
}

// ── 6: IR — class + synthesized schema + wire stability ─────────────

#[test]
fn ir_carries_class_and_synthesized_metadata_schema() {
    let prog = parse(WELL_FORMED);
    let ir = IRGenerator::new().generate(&prog);
    let json = serde_json::to_string(&ir).expect("serialize");
    assert!(json.contains("\"class\":\"crm\""), "{json}");
    assert!(json.contains("\"form\":\"inline\""), "{json}");
    for col in ["\"key\"", "\"version\"", "\"created_at\"", "\"expires_at\""] {
        assert!(
            json.contains(&format!("\"name\":{col}")),
            "synthesized column {col} missing from IR: {json}"
        );
    }
    assert!(
        !json.contains("value"),
        "no `value` column may ever appear in a secrets store IR: {json}"
    );
}

#[test]
fn non_secrets_store_ir_has_no_class_key() {
    let src = "axonstore Sessions {\n backend: postgresql\n connection: \"env:DB\"\n}\n";
    let ir = IRGenerator::new().generate(&parse(src));
    let json = serde_json::to_string(&ir).expect("serialize");
    assert!(
        !json.contains("\"class\""),
        "pre-v2.48.0 stores must serialize byte-identically (no `class` key): {json}"
    );
}
