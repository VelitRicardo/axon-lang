//! v2.7.0 — a `use` nested inside a `step { }` body is a parse error,
//! not a silent degrade.
//!
//! ## The soundness hole (validated 2026-06-03, Kivi brief #17 Q4)
//!
//! The canonical tool dispatch is the FLOW-level step `use <Tool> on <arg>`
//! (`FlowStep::UseTool` → `IRFlowNode::UseTool` → `runner.rs` `registry.dispatch`).
//! A `use` written INSIDE a `step { }` body used to be grouped with the
//! `probe|reason|weave|stream` sub-constructs and skipped structurally by
//! `Parser::skip_flow_step_structural`. The AST node was dropped before it
//! ever existed, so:
//!   * the type-checker's `check_flow_steps` could not see it (no node to
//!     visit — the `_ => {}` catch-all is downstream of an already-gone node),
//!   * the step fell through to the unconstrained LLM backend with NO
//!     diagnostic, and
//!   * the linear resource the tool would provision was never accounted for
//!     (use_tool soundness — the Theorem 5.1 uncertainty was "washed").
//!
//! ## The fix
//!
//! The parser — the only stage that still sees the token — now rejects the
//! nested form and redirects to the three canonical forms: the flow-level
//! `use <Tool> on <arg>` step, the in-step `apply: <Tool>` binding, and the
//! step-header `step <name> use <Persona> { … }` persona attachment. The
//! `probe|reason|weave|stream` sub-constructs are UNCHANGED — they still skip
//! structurally.

use axon_frontend::ast::{Declaration, FlowStep, Program};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn parse(source: &str) -> Result<Program, axon_frontend::parser::ParseError> {
    let tokens = Lexer::new(source, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse()
}

fn parse_ok(source: &str) -> Program {
    parse(source).expect("parse")
}

// ─── section 1 — the nested form is now a parse error ──────────────────────

#[test]
fn nested_use_in_step_body_is_a_parse_error() {
    let src = "flow F() -> Unit {\n\
               step Generate { ask: \"x\" use Search on query }\n\
               }\n\
               axonendpoint E { method: POST path: \"/f\" execute: F }";
    let err = parse(src).expect_err(
        "a `use` nested inside a step body MUST be rejected — pre-v2.7.0 it was \
         silently skipped and the step degraded to an unconstrained LLM call",
    );
    let msg = err.to_string();
    assert!(
        msg.contains("`use` is not valid inside a `step"),
        "diagnostic must name the offending construct; got: {msg}"
    );
}

#[test]
fn nested_use_diagnostic_names_the_tool_and_canonical_forms() {
    let src = "flow F() -> Unit {\n\
               step Generate { ask: \"x\" use Search on query }\n\
               }\n\
               axonendpoint E { method: POST path: \"/f\" execute: F }";
    let msg = parse(src).expect_err("nested use rejected").to_string();
    // The peeked tool name appears in both redirects.
    assert!(msg.contains("use Search on <arg>"), "flow-level redirect; got: {msg}");
    assert!(msg.contains("apply: Search"), "in-step redirect; got: {msg}");
    assert!(msg.contains("step <name> use <Persona>"), "persona redirect; got: {msg}");
}

#[test]
fn nested_use_without_a_following_ident_falls_back_to_placeholder() {
    // Defensive: even if the tool name token is missing/empty, the parser
    // must still produce the diagnostic (with the `<Tool>` placeholder)
    // rather than panic on the peek.
    let src = "flow F() -> Unit {\n\
               step Generate { ask: \"x\" use }\n\
               }\n\
               axonendpoint E { method: POST path: \"/f\" execute: F }";
    let msg = parse(src).expect_err("nested bare use rejected").to_string();
    assert!(
        msg.contains("`use` is not valid inside a `step"),
        "got: {msg}"
    );
}

// ─── section 2 — the canonical FLOW-level form still parses ────────────────

#[test]
fn flow_level_use_tool_still_parses_to_use_tool_node() {
    let src = "flow F() -> Unit {\n\
               use Search on query\n\
               }\n\
               axonendpoint E { method: POST path: \"/f\" execute: F }";
    let program = parse_ok(src);
    let flow = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::Flow(f) = d { Some(f) } else { None })
        .expect("flow F");
    let use_tool = flow
        .body
        .iter()
        .find_map(|s| if let FlowStep::UseTool(u) = s { Some(u) } else { None })
        .expect("the flow-level `use Search on query` must parse to FlowStep::UseTool");
    assert_eq!(use_tool.tool_name, "Search");
    // v2.8.0 — the positional arg now lives in `UseArgs::LegacyPositional`.
    assert_eq!(use_tool.args.legacy_argument(), "query");
}

// ─── section 3 — the step-header persona form is unaffected ────────────────

#[test]
fn step_header_use_persona_is_unaffected() {
    let src = "flow F() -> Unit {\n\
               step Generate use Analyst { ask: \"x\" }\n\
               }\n\
               axonendpoint E { method: POST path: \"/f\" execute: F }";
    let program = parse_ok(src);
    let flow = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::Flow(f) = d { Some(f) } else { None })
        .expect("flow F");
    let step = flow
        .body
        .iter()
        .find_map(|s| if let FlowStep::Step(st) = s { Some(st) } else { None })
        .expect("step Generate");
    assert_eq!(
        step.persona_ref, "Analyst",
        "the `use <Persona>` in the step HEADER binds the persona and must not \
         be confused with the rejected body-level `use`"
    );
}

// ─── section 4 — probe / reason / weave / stream still skip structurally ───

#[test]
fn sibling_subconstructs_in_step_body_still_skip_cleanly() {
    // The surgical fix split ONLY `use` out of the structural-skip arm; the
    // four siblings must still parse without error (they remain skipped).
    for kw in ["probe Foo", "reason Bar", "weave Baz", "stream Qux"] {
        let src = format!(
            "flow F() -> Unit {{\n\
             step Generate {{ ask: \"x\" {kw} }}\n\
             }}\n\
             axonendpoint E {{ method: POST path: \"/f\" execute: F }}"
        );
        assert!(
            parse(&src).is_ok(),
            "`{kw}` nested in a step body must still skip structurally (unchanged by v2.7.0)"
        );
    }
}
