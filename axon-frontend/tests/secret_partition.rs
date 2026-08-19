//! v2.49.0 — the `tool { secret_partition: <param> }` parametric-injection
//! field (doctrine `selection_without_revelation`) —
//! `docs/cycle/secret_partition_2.md`, axon-enterprise repo.
//!
//! Pinned properties:
//! 1. `secret_partition:` parses as a bare parameter reference into
//!    `ToolDefinition.secret_partition`.
//! 2. A well-formed partitioned tool (partition names a required `String`
//!    parameter, alongside a `secret:`) produces zero diagnostics.
//! 3. **axon-T903** — a partition naming a parameter the tool does not
//!    declare is rejected.
//! 4. **axon-T903** — a partition naming a non-`String` (or optional)
//!    parameter is rejected (the segment must be one required key segment).
//! 5. **axon-T903** — a partition with no `secret:` is rejected (nothing to
//!    extend).
//! 6. **axon-T903** — a partition on a `target:`-bound technician tool is
//!    rejected (argv dispatch has no request body to inject into).
//! 7. IR: `secret_partition` rides `IRToolSpec`; elided when empty (IR-SHA
//! stability for every pre-v2.49.0 tool).

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

fn first_tool(prog: &axon_frontend::ast::Program) -> &axon_frontend::ast::ToolDefinition {
    prog.declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Tool(t) => Some(t),
            _ => None,
        })
        .expect("no tool declaration")
}

const WELL_FORMED: &str = "tool CrmCrearContacto {\n\
    secret: crm.hubspot\n\
    secret_partition: tenant_id\n\
    parameters: { tenant_id: String, nombre: String, email: String }\n\
    output_type: String\n\
}\n";

#[test]
fn secret_partition_parses_as_param_ref() {
    let prog = parse(WELL_FORMED);
    let t = first_tool(&prog);
    assert_eq!(t.secret, "crm.hubspot");
    assert_eq!(t.secret_partition, "tenant_id");
}

#[test]
fn well_formed_partition_is_clean() {
    let errors = check_errors(WELL_FORMED);
    assert!(errors.is_empty(), "expected zero diagnostics, got: {errors:?}");
}

#[test]
fn t903_unknown_partition_parameter_is_rejected() {
    let src = "tool T {\n\
        secret: crm.hubspot\n\
        secret_partition: nope\n\
        parameters: { tenant_id: String }\n\
    }\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T903") && e.contains("no parameter named")),
        "expected axon-T903 unknown-parameter, got: {errors:?}"
    );
}

#[test]
fn t903_non_string_partition_parameter_is_rejected() {
    let src = "tool T {\n\
        secret: crm.hubspot\n\
        secret_partition: tenant_id\n\
        parameters: { tenant_id: Int }\n\
    }\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T903") && e.contains("one key segment")),
        "expected axon-T903 non-String, got: {errors:?}"
    );
}

#[test]
fn t903_optional_partition_parameter_is_rejected() {
    let src = "tool T {\n\
        secret: crm.hubspot\n\
        secret_partition: tenant_id\n\
        parameters: { tenant_id: String? }\n\
    }\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T903") && e.contains("one key segment")),
        "expected axon-T903 optional-param, got: {errors:?}"
    );
}

#[test]
fn t903_partition_without_secret_is_rejected() {
    let src = "tool T {\n\
        secret_partition: tenant_id\n\
        parameters: { tenant_id: String }\n\
    }\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T903") && e.contains("no `secret:`")),
        "expected axon-T903 partition-without-secret, got: {errors:?}"
    );
}

#[test]
fn t903_partition_on_technician_tool_is_rejected() {
    // A technician tool dispatches argv over a socket — no request body to
    // inject a partitioned secret into. The socket reference is deliberately
    // unresolved (T861 fires alongside); the T903 technician exclusion must
    // fire regardless, on the tool's own shape.
    let src = "tool Ping {\n\
        secret: crm.hubspot\n\
        secret_partition: host\n\
        target: OpsSocket\n\
        risk: safe\n\
        argv: [\"ping\", \"-c\", \"1\", \"${host}\"]\n\
        parameters: { host: String }\n\
    }\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T903") && e.contains("technician")),
        "expected axon-T903 technician exclusion, got: {errors:?}"
    );
}

#[test]
fn ir_carries_secret_partition() {
    let prog = parse(WELL_FORMED);
    let ir = IRGenerator::new().generate(&prog);
    let json = serde_json::to_string(&ir).expect("serialize");
    assert!(
        json.contains("\"secret_partition\":\"tenant_id\""),
        "{json}"
    );
}

#[test]
fn partition_less_tool_ir_has_no_secret_partition_key() {
    // A v2.48.0 static-key tool (no partition) must serialize byte-identically
    // to pre-v2.49.0 — the `secret_partition` key is elided when empty.
    let src = "tool Plain {\n\
        secret: crm.hubspot\n\
        parameters: { q: String }\n\
    }\n";
    let ir = IRGenerator::new().generate(&parse(src));
    let json = serde_json::to_string(&ir).expect("serialize");
    assert!(
        !json.contains("\"secret_partition\""),
        "pre-v2.49.0 tools must serialize with no `secret_partition` key: {json}"
    );
}
