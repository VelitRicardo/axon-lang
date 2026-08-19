//! v2.39.0 — grammar + AST + IR + type-checker for the Remote
//! Hands technician-command surface on `tool` (`target:` / `risk:` / `argv:`).
//! See `docs/cycle/remote_hands_technician_protocol.md` (axon-enterprise).
//!
//! Pinned properties:
//! 1. A full technician `tool` parses into `ToolDefinition` (target/risk/argv).
//! 2. It lowers to `IRToolSpec`; absent technician fields are ELIDED from JSON.
//! 3. **IR-SHA invariance**: a program with no technician tool serialises with
//! no `target`/`risk`/`argv` keys — byte-identical to pre-v2.39.0 IR.
//! 4. A well-formed safe + destructive technician tool → zero diagnostics.
//! 5. **axon-T858** — a `target:`-bound `provider: bash` tool with no `argv:`.
//! 6. **axon-T859** — an argv placeholder not bound to `parameters:`, and a
//!    partial-token placeholder (`"${host}.txt"`).
//! 7. **axon-T860** — a `risk: destructive` tool whose session has no reachable
//!    `branch{approved/denied}`.
//! 8. **axon-T861** — `target:` references an undeclared / non-socket symbol.
//! 9. **axon-T862** — a `risk:` value outside `safe | destructive`.
//! 10. **the design decision** — an unknown field in a `target:`-bound tool is a hard parse
//!     error (while a legacy schema-less tool keeps its lenient skip).

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

fn first_tool(prog: &axon_frontend::ast::Program) -> &axon_frontend::ast::ToolDefinition {
    prog.declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Tool(t) => Some(t),
            _ => None,
        })
        .expect("no tool declaration")
}

fn ir_json(src: &str) -> String {
    let prog = parse(src);
    let ir = IRGenerator::new().generate(&prog);
    serde_json::to_string(&ir).expect("serialize IR")
}

/// The shared session + type + socket scaffolding: a `TechSafe` protocol (no
/// confirmation) and a `TechConfirm` protocol (with the approved/denied branch).
const SCAFFOLD: &str = r#"
type Command { line: String }
type CommandResult { stdout: String, stderr: String, exit_code: Int }
type DenyReason { detail: String }

session TechSafe {
    server: [ send Command, receive CommandResult, end ]
    client: [ receive Command, send CommandResult, end ]
}

session TechConfirm {
    server: [
        send Command,
        select {
            approved: [ receive CommandResult, end ],
            denied:   [ receive DenyReason, end ]
        }
    ]
    client: [
        receive Command,
        branch {
            approved: [ send CommandResult, end ],
            denied:   [ send DenyReason, end ]
        }
    ]
}

socket TechSafeWS { protocol: TechSafe }
socket TechConfirmWS { protocol: TechConfirm }
"#;

/// The canonical well-formed shape: one safe tool over the plain protocol, one
/// destructive tool over the confirm protocol.
fn well_formed() -> String {
    format!(
        "{SCAFFOLD}\n\
         tool Ping {{\n\
         \x20\x20provider: bash\n\
         \x20\x20target: TechSafeWS\n\
         \x20\x20risk: safe\n\
         \x20\x20parameters: {{ count: Int, host: String }}\n\
         \x20\x20argv: [\"ping\", \"-c\", \"${{count}}\", \"${{host}}\"]\n\
         \x20\x20output_type: CommandResult\n\
         }}\n\
         tool DeleteFile {{\n\
         \x20\x20provider: bash\n\
         \x20\x20target: TechConfirmWS\n\
         \x20\x20risk: destructive\n\
         \x20\x20parameters: {{ path: String }}\n\
         \x20\x20argv: [\"rm\", \"${{path}}\"]\n\
         \x20\x20output_type: CommandResult\n\
         }}\n"
    )
}

#[test]
fn technician_tool_parses_into_ast() {
    let prog = parse(&well_formed());
    let ping = first_tool(&prog);
    assert_eq!(ping.target.as_deref(), Some("TechSafeWS"));
    assert_eq!(ping.risk.as_deref(), Some("safe"));
    assert_eq!(ping.argv, vec!["ping", "-c", "${count}", "${host}"]);
}

#[test]
fn well_formed_technician_program_has_no_diagnostics() {
    let errs = check_errors(&well_formed());
    assert!(
        errs.is_empty(),
        "expected zero diagnostics, got: {errs:#?}"
    );
}

#[test]
fn technician_fields_lower_into_ir() {
    let json = ir_json(&well_formed());
    assert!(json.contains("\"target\":\"TechSafeWS\""), "json: {json}");
    assert!(json.contains("\"risk\":\"destructive\""), "json: {json}");
    assert!(json.contains("\"argv\":[\"rm\",\"${path}\"]"), "json: {json}");
}

#[test]
fn ir_sha_invariance_no_technician_fields_elided() {
    // A plain tool (no technician fields) must serialise with NONE of the
    // three new keys — byte-identical to the pre-v2.39.0 IR.
    let src = "tool WebSearch { provider: http max_results: 5 timeout: 10s }";
    let json = ir_json(src);
    assert!(!json.contains("\"target\""), "target leaked: {json}");
    assert!(!json.contains("\"risk\""), "risk leaked: {json}");
    assert!(!json.contains("\"argv\""), "argv leaked: {json}");
}

#[test]
fn t858_target_bound_bash_without_argv() {
    let src = format!(
        "{SCAFFOLD}\n\
         tool Broken {{ provider: bash target: TechSafeWS risk: safe }}\n"
    );
    let errs = check_errors(&src);
    assert!(
        errs.iter().any(|m| m.contains("axon-T858")),
        "expected T858, got: {errs:#?}"
    );
}

#[test]
fn t859_unbound_and_partial_placeholders() {
    // Unbound placeholder ${bogus}.
    let unbound = format!(
        "{SCAFFOLD}\n\
         tool T {{ provider: bash target: TechSafeWS risk: safe \
         parameters: {{ host: String }} argv: [\"ping\", \"${{bogus}}\"] }}\n"
    );
    assert!(
        check_errors(&unbound).iter().any(|m| m.contains("axon-T859")),
        "expected T859 for unbound placeholder"
    );

    // Partial (fused) placeholder ${host}.txt.
    let partial = format!(
        "{SCAFFOLD}\n\
         tool T {{ provider: bash target: TechSafeWS risk: safe \
         parameters: {{ host: String }} argv: [\"cat\", \"${{host}}.txt\"] }}\n"
    );
    assert!(
        check_errors(&partial).iter().any(|m| m.contains("axon-T859")),
        "expected T859 for partial-token placeholder"
    );
}

#[test]
fn t860_destructive_without_confirm_branch() {
    // A destructive tool bound to the SAFE protocol (no approved/denied branch).
    let src = format!(
        "{SCAFFOLD}\n\
         tool DeleteFile {{ provider: bash target: TechSafeWS risk: destructive \
         parameters: {{ path: String }} argv: [\"rm\", \"${{path}}\"] }}\n"
    );
    let errs = check_errors(&src);
    assert!(
        errs.iter().any(|m| m.contains("axon-T860")),
        "expected T860, got: {errs:#?}"
    );
}

#[test]
fn t860_destructive_with_confirm_branch_is_clean() {
    // The destructive tool in `well_formed()` is bound to TechConfirmWS — no T860.
    let errs = check_errors(&well_formed());
    assert!(
        errs.iter().all(|m| !m.contains("axon-T860")),
        "unexpected T860: {errs:#?}"
    );
}

#[test]
fn t861_target_not_a_socket() {
    // `target:` names a session, not a socket.
    let src = format!(
        "{SCAFFOLD}\n\
         tool T {{ provider: bash target: TechSafe risk: safe \
         parameters: {{ host: String }} argv: [\"ping\", \"${{host}}\"] }}\n"
    );
    assert!(
        check_errors(&src).iter().any(|m| m.contains("axon-T861")),
        "expected T861 for non-socket target"
    );

    // `target:` names nothing at all.
    let undef = format!(
        "{SCAFFOLD}\n\
         tool T {{ provider: bash target: NopeWS risk: safe \
         parameters: {{ host: String }} argv: [\"ping\", \"${{host}}\"] }}\n"
    );
    assert!(
        check_errors(&undef).iter().any(|m| m.contains("axon-T861")),
        "expected T861 for undefined target"
    );
}

#[test]
fn t862_unknown_risk_class() {
    let src = format!(
        "{SCAFFOLD}\n\
         tool T {{ provider: bash target: TechSafeWS risk: dangerous \
         parameters: {{ host: String }} argv: [\"ping\", \"${{host}}\"] }}\n"
    );
    assert!(
        check_errors(&src).iter().any(|m| m.contains("axon-T862")),
        "expected T862 for unknown risk class"
    );
}

#[test]
fn d84_13_unknown_field_in_target_bound_tool_is_parse_error() {
    let src = format!(
        "{SCAFFOLD}\n\
         tool T {{ provider: bash target: TechSafeWS risks: safe \
         parameters: {{ host: String }} argv: [\"ping\", \"${{host}}\"] }}\n"
    );
    let err = try_parse(&src).expect_err("expected a hard parse error for unknown field");
    assert!(
        err.message.contains("risks") && err.message.contains("strict field checking"),
        "unexpected error: {}",
        err.message
    );
}

#[test]
fn legacy_tool_keeps_lenient_unknown_field_skip() {
    // A NON-technician tool (no target) keeps its pre-v2.39.0 lenient skip — an
    // unknown field is silently ignored, never a parse error (zero regression).
    let src = "tool WebSearch { provider: http banana: 3 max_results: 5 }";
    let prog = try_parse(src).expect("legacy tool must still parse with an unknown field");
    let t = first_tool(&prog);
    assert_eq!(t.provider, "http");
    assert!(t.target.is_none());
}
