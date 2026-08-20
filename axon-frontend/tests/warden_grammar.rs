//! v2.43.0 — grammar + AST + IR for the `warden` adversarial-analysis
//! flow-body block + the `scope` authorization-policy declaration. See
//! `the design plan` (axon-enterprise repo).
//!
//! Pinned properties (surface only — the v2.43.0 checker owns semantics):
//! 1. A full `scope` parses into `ScopeDefinition` (targets/depth/approver).
//! 2. A `warden(<target>) within <Scope> { body }` parses into `WardenBlock`.
//! 3. **Fail-closed by grammar** — a `warden` with no `within <Scope>` clause is
//!    a HARD parse error (a scopeless warden cannot be written).
//! 4. Both lower to IR (`IRScope` top-level, `IRWarden` as an `IRFlowNode`).
//! 5. **IR-SHA invariance** — a program with no `scope` serialises with no
//!    `scopes` key.
//! 6. **the design decision** — an unknown `scope` field is a hard parse error.
//! 7. The `warden` body (nested flow steps) is preserved.

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::{ParseError, Parser};
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

fn has_code(errs: &[String], code: &str) -> bool {
    errs.iter().any(|e| e.contains(code))
}

fn try_parse(src: &str) -> Result<axon_frontend::ast::Program, ParseError> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse()
}

fn ir_json(src: &str) -> String {
    let prog = parse(src);
    let ir = IRGenerator::new().generate(&prog);
    serde_json::to_string(&ir).expect("serialize IR")
}

fn first_scope(prog: &axon_frontend::ast::Program) -> &axon_frontend::ast::ScopeDefinition {
    prog.declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Scope(s) => Some(s),
            _ => None,
        })
        .expect("no scope declaration")
}

/// Pull the first `warden` block out of the first flow's steps.
fn first_warden(prog: &axon_frontend::ast::Program) -> &axon_frontend::ast::WardenBlock {
    for d in &prog.declarations {
        if let axon_frontend::ast::Declaration::Flow(f) = d {
            for step in &f.body {
                if let axon_frontend::ast::FlowStep::Warden(w) = step {
                    return w;
                }
            }
        }
    }
    panic!("no warden block");
}

const SCOPE: &str = r#"
scope InternalAudit {
    targets: [ "svc://payments-core", "svc://ledger" ]
    depth: static_artifact
    approver: requires "security.lead"
}
"#;

const FULL: &str = r#"
scope InternalAudit {
    targets: [ "svc://payments-core" ]
    depth: static_artifact
    approver: requires "security.lead"
}
flow Audit() -> Unit {
    warden(payments_core) within InternalAudit {
        step S { ask: "analyse" }
    }
}
"#;

// ── scope (top-level declaration) ────────────────────────────────────────────

#[test]
fn full_scope_parses_every_field() {
    let prog = parse(SCOPE);
    let s = first_scope(&prog);
    assert_eq!(s.name, "InternalAudit");
    assert_eq!(s.targets.len(), 2);
    assert_eq!(s.targets[0], "svc://payments-core");
    assert_eq!(s.targets[1], "svc://ledger");
    assert_eq!(s.depth, "static_artifact");
    assert_eq!(s.approver, "security.lead");
}

#[test]
fn scope_lowers_to_ir() {
    let json = ir_json(SCOPE);
    assert!(json.contains("\"scopes\""), "scopes key present: {json}");
    assert!(json.contains("\"InternalAudit\""));
    assert!(json.contains("static_artifact"));
    assert!(json.contains("security.lead"));
}

#[test]
fn no_scope_leaves_ir_byte_identical() {
    let json = ir_json("flow Chat() -> Unit { step S { ask: \"hi\" } }\n");
    assert!(!json.contains("scopes"), "no scope ⇒ no scopes key: {json}");
}

#[test]
fn unknown_scope_field_is_a_parse_error() {
    let src = "scope S { targets: [ \"a\" ] depth: static_artifact nonsense: 3 }\n";
    let err = try_parse(src).expect_err("unknown scope field must fail parse");
    assert!(err.message.contains("nonsense"), "{}", err.message);
}

#[test]
fn approver_without_requires_sugar_parses() {
    let src = "scope S { targets: [ \"a\" ] depth: static_artifact approver: \"sec.lead\" }\n";
    let prog = parse(src);
    assert_eq!(first_scope(&prog).approver, "sec.lead");
}

// ── warden (flow-body block) ─────────────────────────────────────────────────

#[test]
fn full_warden_parses_target_scope_and_body() {
    let prog = parse(FULL);
    let w = first_warden(&prog);
    assert_eq!(w.target, "payments_core");
    assert_eq!(w.scope_ref, "InternalAudit");
    assert_eq!(w.body.len(), 1, "the nested step is preserved");
}

#[test]
fn warden_lowers_to_ir_flow_node() {
    let json = ir_json(FULL);
    assert!(json.contains("\"warden\""), "warden node present: {json}");
    assert!(json.contains("\"InternalAudit\""));
    assert!(json.contains("payments_core"));
}

#[test]
fn warden_without_within_is_a_hard_parse_error() {
    // Fail-closed by grammar: a scopeless warden cannot be written.
    let src = r#"
flow Audit() -> Unit {
    warden(target) {
        step S { ask: "x" }
    }
}
"#;
    assert!(
        try_parse(src).is_err(),
        "a warden with no `within <Scope>` must fail to parse"
    );
}

#[test]
fn warden_with_within_but_empty_body_parses() {
    let src = r#"
flow Audit() -> Unit {
    warden(t) within S { }
}
"#;
    let prog = parse(src);
    let w = first_warden(&prog);
    assert_eq!(w.scope_ref, "S");
    assert!(w.body.is_empty());
}

// ── v2.43.0 — check_scope (own-field discipline) ───────────────────────────────

#[test]
fn well_formed_scope_is_check_clean() {
    let errs = check_errors(SCOPE);
    assert!(
        errs.iter().all(|e| !e.contains("axon-T88")),
        "well-formed scope must raise no v2.43.0 diagnostic: {errs:?}"
    );
}

#[test]
fn t884_empty_targets_allowlist() {
    let src = "scope S { targets: [ ] depth: static_artifact approver: \"sec.lead\" }\n";
    assert!(has_code(&check_errors(src), "axon-T884"));
}

#[test]
fn t885_unknown_depth() {
    let src = "scope S { targets: [ \"a\" ] depth: kernel_rootkit approver: \"sec.lead\" }\n";
    assert!(has_code(&check_errors(src), "axon-T885"));
}

#[test]
fn t885_valid_depths_are_clean() {
    for depth in ["static_artifact", "memory_dump", "live_network"] {
        let src = format!("scope S {{ targets: [ \"a\" ] depth: {depth} approver: \"x\" }}\n");
        assert!(!has_code(&check_errors(&src), "axon-T885"), "depth={depth}");
    }
}

#[test]
fn t886_missing_approver() {
    let src = "scope S { targets: [ \"a\" ] depth: static_artifact }\n";
    assert!(has_code(&check_errors(src), "axon-T886"));
}

// ── v2.43.0 — check_warden (authorization binding) ─────────────────────────────

#[test]
fn well_formed_warden_resolves_clean() {
    // FULL: warden within a DECLARED scope → no v2.43.0 diagnostic.
    let errs = check_errors(FULL);
    assert!(
        errs.iter().all(|e| !e.contains("axon-T88")),
        "warden within a declared scope must be v2.43.0-clean: {errs:?}"
    );
}

#[test]
fn t887_undefined_scope() {
    let src = r#"
flow Audit() -> Unit {
    warden(t) within Ghost { }
}
"#;
    assert!(has_code(&check_errors(src), "axon-T887"), "undefined scope");
}

#[test]
fn t887_within_resolves_to_non_scope() {
    // `M` is a memory, not a scope.
    let src = r#"
memory M { store: persistent }
flow Audit() -> Unit {
    warden(t) within M { }
}
"#;
    assert!(has_code(&check_errors(src), "axon-T887"), "wrong-kind scope ref");
}
