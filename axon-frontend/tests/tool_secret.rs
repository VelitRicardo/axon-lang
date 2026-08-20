//! v2.48.0 — the `tool { secret: <key> }` dispatch-injection field
//! (doctrine `rotation_without_revelation`) —
//! `the design plan`, axon-enterprise repo.
//!
//! Pinned properties:
//! 1. `secret:` parses as a dotted config key into `ToolDefinition.secret`.
//! 2. A well-formed tool with `secret:` produces zero diagnostics.
//! 3. **axon-T902** — a non-key-shaped value (the T850 charset mirror:
//!    no `/`, no `:`, no uppercase) is rejected.
//! 4. **axon-T902** — `secret:` on a `target:`-bound technician tool is
//!    rejected (argv dispatch has no request to inject into).
//! 5. IR: `secret` rides `IRToolSpec`; elided when empty (IR-SHA
//! stability for every pre-v2.48.0 tool).

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
    parameters: { nombre: String, email: String }\n\
    output_type: String\n\
}\n";

#[test]
fn tool_secret_parses_as_dotted_key() {
    let prog = parse(WELL_FORMED);
    let t = first_tool(&prog);
    assert_eq!(t.name, "CrmCrearContacto");
    assert_eq!(t.secret, "crm.hubspot");
}

#[test]
fn well_formed_tool_secret_is_clean() {
    let errors = check_errors(WELL_FORMED);
    assert!(errors.is_empty(), "expected zero diagnostics, got: {errors:?}");
}

#[test]
fn t902_uppercase_key_is_rejected() {
    // `Crm.hubspot` parses as a dotted identifier but violates the key
    // charset (lowercase-only head) — the T850 mirror.
    let src = "tool T {\n secret: Crm.hubspot\n}\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T902") && e.contains("not a config key")),
        "expected axon-T902 key-shape, got: {errors:?}"
    );
}

#[test]
fn t902_secret_on_technician_tool_is_rejected() {
    // The socket reference is deliberately unresolved — T861 fires alongside,
    // but the T902 technician exclusion must fire regardless: the law is on
    // the tool's own shape, not on the target's validity.
    let src = "tool Ping {\n\
        secret: crm.hubspot\n\
        target: OpsSocket\n\
        risk: safe\n\
        argv: [\"ping\", \"-c\", \"1\", \"${host}\"]\n\
        parameters: { host: String }\n\
    }\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T902") && e.contains("technician")),
        "expected axon-T902 technician exclusion, got: {errors:?}"
    );
}

#[test]
fn ir_carries_tool_secret_key_only() {
    let prog = parse(WELL_FORMED);
    let ir = IRGenerator::new().generate(&prog);
    let json = serde_json::to_string(&ir).expect("serialize");
    assert!(json.contains("\"secret\":\"crm.hubspot\""), "{json}");
}

#[test]
fn secret_less_tool_ir_has_no_secret_key() {
    let src = "tool Plain {\n parameters: { q: String }\n}\n";
    let ir = IRGenerator::new().generate(&parse(src));
    let json = serde_json::to_string(&ir).expect("serialize");
    assert!(
        !json.contains("\"secret\""),
        "pre-v2.48.0 tools must serialize byte-identically (no `secret` key): {json}"
    );
}
