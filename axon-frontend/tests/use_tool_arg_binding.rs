//! v2.7.0 — the `use <Tool> on <arg>` argument binds a request
//! parameter into the tool dispatch.
//!
//! ## Finding (verified 2026-06-03)
//!
//! The original v2.7.0 validation reported that the `on <arg>` grammar
//! "takes a bare ident (no `${param}`)". That was incomplete: the
//! argument is consumed by `Parser::consume_any_ident_or_kw`, which
//! ALREADY accepts a `StringLit`. So the canonical binding form —
//! `use <Tool> on "${param}"` — already parses, capturing the literal
//! `${param}` interpolation text, which the runtime
//! (`runner.rs` → `ExecContext::interpolate`) resolves against the
//! request-bound flow parameters (`request_binding::bind_request`).
//!
//! ## The contract this locks
//!
//!   * `on "${query}"` / `on "$query"` — STRING LITERAL interpolation →
//!     binds the request param `query` at dispatch.
//!   * `on query` — BARE identifier → a literal argument (no `$` ⇒ no
//!     interpolation). Unchanged.
//!   * `on ${query}` — UNQUOTED interpolation → not a form. Axon keeps
//!     interpolation inside string literals everywhere; the lexer now
//!     guides a bare `$` toward quoting rather than emitting an opaque
//!     "unexpected character" error.

use axon_frontend::ast::{Declaration, FlowStep, Program};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn parse(src: &str) -> Result<Program, axon_frontend::parser::ParseError> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse()
}

fn use_tool_argument(src: &str) -> String {
    let program = parse(src).expect("parse");
    let flow = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::Flow(f) = d { Some(f) } else { None })
        .expect("flow");
    flow.body
        .iter()
        .find_map(|s| if let FlowStep::UseTool(u) = s { Some(u) } else { None })
        .expect("FlowStep::UseTool")
        // v2.8.0 — the v2.7.0 positional arg is now `UseArgs::LegacyPositional`.
        .args
        .legacy_argument()
}

const HEAD: &str = "axonendpoint E { method: POST path: \"/f\" execute: F }";

// ─── section 1 — interpolating string-literal argument (the binding form) ──

#[test]
fn braced_interpolation_string_literal_captures_dollar_brace_query() {
    let src = format!(
        "flow F(query: String) -> Unit {{\n\
         use Search on \"${{query}}\"\n\
         }}\n{HEAD}"
    );
    assert_eq!(
        use_tool_argument(&src),
        "${query}",
        "the `on \"${{query}}\"` argument MUST capture the verbatim interpolation \
         text so the runtime resolves it against the bound request param"
    );
}

#[test]
fn dollar_name_interpolation_string_literal_is_captured() {
    let src = format!(
        "flow F(query: String) -> Unit {{\n\
         use Search on \"$query\"\n\
         }}\n{HEAD}"
    );
    assert_eq!(use_tool_argument(&src), "$query");
}

// ─── section 2 — bare identifier stays a literal (backwards-compat) ────────

#[test]
fn bare_identifier_argument_is_a_literal() {
    let src = format!(
        "flow F(query: String) -> Unit {{\n\
         use Search on query\n\
         }}\n{HEAD}"
    );
    assert_eq!(
        use_tool_argument(&src),
        "query",
        "a bare ident carries no `$` ⇒ the runtime passes it through verbatim"
    );
}

// ─── section 3 — unquoted `$` is rejected with a guiding diagnostic ────────

#[test]
fn unquoted_dollar_interpolation_is_a_guiding_lex_error() {
    let src = format!(
        "flow F(query: String) -> Unit {{\n\
         use Search on ${{query}}\n\
         }}\n{HEAD}"
    );
    let tokens = Lexer::new(&src, "<test>").tokenize();
    let err = tokens.expect_err("a bare `$` outside a string literal must not lex");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("Interpolation") && msg.contains("string literal"),
        "the lexer must guide the author to quote the interpolation; got: {msg}"
    );
}
