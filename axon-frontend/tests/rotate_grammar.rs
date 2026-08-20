//! v2.48.0 — grammar + AST + IR + type-checker for the `rotate` flow
//! verb (mediated secret renewal, doctrine `rotation_without_revelation`)
//! `the design plan`, axon-enterprise repo.
//!
//! Pinned properties:
//! 1. `rotate <Store> where "…" with <Tool> as <binding>` parses into
//!    `RotateStep` (all four anchors captured).
//! 2. The `where` filter is optional (whole-class rotation).
//! 3. Missing `with <Tool>` / missing `as <binding>` are HARD parse errors.
//! 4. A well-formed secrets store + tool + rotate produces zero diagnostics.
//! 5. **axon-T898** — `rotate` on an undeclared name / a non-store symbol /
//!    a non-secrets axonstore.
//! 6. **axon-T899** — `rotate … with` an undeclared tool / a non-tool symbol.
//! 7. The optional filter is proven against the SYNTHESIZED metadata
//! schema (v1.31.0): an unknown column is rejected.
//! 8. IR: the `rotate` node carries store_ref/tool_ref/binding; an empty
//!    `where_expr` is elided from the wire (IR-SHA stability).

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

fn first_rotate(
    prog: &axon_frontend::ast::Program,
) -> &axon_frontend::ast::RotateStep {
    prog.declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Flow(f) => f.body.iter().find_map(|s| match s {
                axon_frontend::ast::FlowStep::Rotate(r) => Some(r),
                _ => None,
            }),
            _ => None,
        })
        .expect("no rotate step")
}

const WELL_FORMED: &str = "axonstore CrmTokens {\n\
    backend: secrets\n\
    class: crm\n\
}\n\
tool RefreshCrmToken {\n\
    endpoint: \"/tools/crm/refresh\"\n\
}\n\
flow RotateExpiring() -> Unit {\n\
    rotate CrmTokens where \"expires_at < now() + interval '10 minutes'\" with RefreshCrmToken as result\n\
    step Report { ask: \"Report the rotation outcome: ${result}.\" }\n\
}\n";

// ── 1 + 2: parse shapes ─────────────────────────────────────────────

#[test]
fn rotate_parses_all_four_anchors() {
    let prog = parse(WELL_FORMED);
    let r = first_rotate(&prog);
    assert_eq!(r.store_ref, "CrmTokens");
    assert_eq!(r.where_expr, "expires_at < now() + interval '10 minutes'");
    assert_eq!(r.tool_ref, "RefreshCrmToken");
    assert_eq!(r.binding, "result");
}

#[test]
fn rotate_without_where_is_whole_class() {
    let src = "axonstore CrmTokens {\n backend: secrets\n class: crm\n}\n\
        tool R { endpoint: \"/r\" }\n\
        flow BulkRotate() -> Unit {\n\
            rotate CrmTokens with R as summary\n\
        }\n";
    let prog = parse(src);
    let r = first_rotate(&prog);
    assert!(r.where_expr.is_empty(), "omitted filter = whole class");
    assert_eq!(r.tool_ref, "R");
}

// ── 3: hard parse errors ────────────────────────────────────────────

#[test]
fn rotate_without_with_is_a_parse_error() {
    let src = "flow F() -> Unit { rotate CrmTokens as x }\n";
    let err = try_parse(src).expect_err("expected parse error");
    assert!(
        err.message.contains("with <Tool>"),
        "error must teach the shape: {}",
        err.message
    );
}

#[test]
fn rotate_without_binding_is_a_parse_error() {
    let src = "flow F() -> Unit { rotate CrmTokens with R }\n";
    assert!(
        try_parse(src).is_err(),
        "a rotate with no `as <binding>` must not parse — renewal with no \
         observable outcome"
    );
}

// ── 4: well-formed shape is clean ───────────────────────────────────

#[test]
fn well_formed_rotate_is_clean() {
    let errors = check_errors(WELL_FORMED);
    assert!(errors.is_empty(), "expected zero diagnostics, got: {errors:?}");
}

// ── 5: axon-T898 target law ─────────────────────────────────────────

#[test]
fn t898_rotate_on_undeclared_store_is_rejected() {
    let src = "tool R { endpoint: \"/r\" }\n\
        flow F() -> Unit { rotate Ghost with R as x }\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T898") && e.contains("not declared")),
        "expected axon-T898 undeclared, got: {errors:?}"
    );
}

#[test]
fn t898_rotate_on_non_secrets_store_is_rejected() {
    let src = "axonstore Sessions {\n backend: postgresql\n}\n\
        tool R { endpoint: \"/r\" }\n\
        flow F() -> Unit { rotate Sessions with R as x }\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T898") && e.contains("backend is not")),
        "expected axon-T898 non-secrets, got: {errors:?}"
    );
}

#[test]
fn t898_rotate_on_non_store_symbol_is_rejected() {
    let src = "tool R { endpoint: \"/r\" }\n\
        flow F() -> Unit { rotate R with R as x }\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T898") && e.contains("it is a")),
        "expected axon-T898 wrong-kind, got: {errors:?}"
    );
}

// ── 6: axon-T899 tool law ───────────────────────────────────────────

#[test]
fn t899_rotate_with_undeclared_tool_is_rejected() {
    let src = "axonstore CrmTokens {\n backend: secrets\n class: crm\n}\n\
        flow F() -> Unit { rotate CrmTokens with Ghost as x }\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T899") && e.contains("undeclared tool")),
        "expected axon-T899 undeclared, got: {errors:?}"
    );
}

#[test]
fn t899_rotate_with_non_tool_symbol_is_rejected() {
    let src = "axonstore CrmTokens {\n backend: secrets\n class: crm\n}\n\
        flow F() -> Unit { rotate CrmTokens with CrmTokens as x }\n";
    let errors = check_errors(src);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axon-T899") && e.contains("not a tool")),
        "expected axon-T899 wrong-kind, got: {errors:?}"
    );
}

// ── 7: the filter is proven against the synthesized schema ──────────

#[test]
fn unknown_column_in_rotate_filter_is_rejected() {
    let src = "axonstore CrmTokens {\n backend: secrets\n class: crm\n}\n\
        tool R { endpoint: \"/r\" }\n\
        flow F() -> Unit {\n\
            rotate CrmTokens where \"refresh_token = 'x'\" with R as out\n\
        }\n";
    let errors = check_errors(src);
    assert!(
        errors.iter().any(|e| e.contains("refresh_token")),
        "expected a v1.31.0 unknown-column error naming `refresh_token` — the \
         secret VALUE has no column by design — got: {errors:?}"
    );
}

// ── 8: IR shape + wire stability ────────────────────────────────────

#[test]
fn ir_carries_the_rotate_node() {
    let prog = parse(WELL_FORMED);
    let ir = IRGenerator::new().generate(&prog);
    let json = serde_json::to_string(&ir).expect("serialize");
    assert!(json.contains("\"rotate\""), "{json}");
    assert!(json.contains("\"store_ref\":\"CrmTokens\""), "{json}");
    assert!(json.contains("\"tool_ref\":\"RefreshCrmToken\""), "{json}");
    assert!(json.contains("\"binding\":\"result\""), "{json}");
    assert!(
        json.contains("\"where_expr\":\"expires_at < now() + interval '10 minutes'\""),
        "{json}"
    );
}

#[test]
fn empty_where_is_elided_from_the_wire() {
    let src = "axonstore CrmTokens {\n backend: secrets\n class: crm\n}\n\
        tool R { endpoint: \"/r\" }\n\
        flow F() -> Unit { rotate CrmTokens with R as x }\n";
    let ir = IRGenerator::new().generate(&parse(src));
    let json = serde_json::to_string(&ir).expect("serialize");
    assert!(
        !json.contains("\"where_expr\""),
        "empty filter must be elided (IR-SHA stability): {json}"
    );
}
