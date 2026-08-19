//! v2.39.0 — drift gate for the Remote Hands technician-command doc.
//!
//! The canonical programs published in `knowledge/primitives/tool.md`
//! (`target:` / `risk:` / `argv:`) must round-trip through the same
//! `axon-frontend` pipeline the `axon` CLI uses — the "published grammar
//! MUST compile" discipline, applied to the highest-stakes primitive.
//!
//! Mirrors `upstream_canonical_programs.rs`.

use axon_emcp::compiler_pipeline::{run, Outcome};

fn must_compile(label: &str, source: &str) {
    match run(source, label) {
        Outcome::Ok { .. } => {}
        Outcome::Err {
            stage,
            errors,
            warnings,
        } => panic!(
            "{label}: expected well-formed program, got {stage:?} failure:\n\
             errors   = {errors:#?}\n\
             warnings = {warnings:#?}\n\
             source   = {source}"
        ),
    }
}

/// The published `tool.md` Remote Hands example: a destructive technician
/// command whose bound session carries the approved/denied confirm branch.
#[test]
fn technician_command_doc_example_compiles() {
    let src = r#"
type Command { line: String }
type CommandResult { stdout: String, stderr: String, exit_code: Int }
type DenyReason { detail: String }

session TechConfirm {
    server: [ send Command,
              select { approved: [receive CommandResult, end],
                       denied:   [receive DenyReason, end] } ]
    client: [ receive Command,
              branch { approved: [send CommandResult, end],
                       denied:   [send DenyReason, end] } ]
}
socket TechConfirmWS { protocol: TechConfirm }

tool DeleteFile {
    provider: bash
    target: TechConfirmWS
    risk: destructive
    parameters: { path: String }
    argv: ["rm", "${path}"]
    output_type: CommandResult
}
"#;
    must_compile("technician/destructive", src);
}

/// A `risk: safe` technician command over a plain (no-branch) session — the
/// safe path needs no confirmation branch.
#[test]
fn safe_technician_command_compiles() {
    let src = r#"
type Command { line: String }
type CommandResult { stdout: String, stderr: String, exit_code: Int }

session TechSafe {
    server: [ send Command, receive CommandResult, end ]
    client: [ receive Command, send CommandResult, end ]
}
socket TechSafeWS { protocol: TechSafe }

tool Ping {
    provider: bash
    target: TechSafeWS
    risk: safe
    parameters: { count: Int, host: String }
    argv: ["ping", "-c", "${count}", "${host}"]
    output_type: CommandResult
}
"#;
    must_compile("technician/safe", src);
}
