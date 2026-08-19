//! v2.8.0 — `use Tool(k = v, …)` keyword-arg invocation + the `UseArgs`
//! closed catalog.
//!
//! Brief #22 / W1: pre-v2.8.0 a tool received a single opaque `argument: String`,
//! so multi-field structured args were impossible. v2.8.0 replaces that field
//! with `UseArgs { LegacyPositional, Named }` — the v2.7.0 single-`on <arg>`
//! form (D5 back-compat) and the canonical `use Tool(query = …, max_results =
//! …)` keyword form (D2). This gate pins both forms parse to the right variant
//! and coexist.

use axon_frontend::ast::{Declaration, FlowStep, Program, UseArgs};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn parse(src: &str) -> Program {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn first_use_tool(p: &Program, flow_name: &str) -> UseArgs {
    let flow = p
        .declarations
        .iter()
        .find_map(|d| match d {
            Declaration::Flow(f) if f.name == flow_name => Some(f),
            _ => None,
        })
        .unwrap_or_else(|| panic!("flow {flow_name} not found"));
    flow.body
        .iter()
        .find_map(|s| if let FlowStep::UseTool(u) = s { Some(u) } else { None })
        .expect("flow must carry a FlowStep::UseTool")
        .args
        .clone()
}

#[test]
fn named_keyword_args_parse_to_useargs_named() {
    let program = parse(
        r#"
tool WebSearch {
    provider: http
    parameters: { query: String, max_results: Int, safesearch: Bool }
}
flow Research(q: String) -> Any {
    use WebSearch(query = "${q}", max_results = 5, safesearch = true)
}
"#,
    );
    match first_use_tool(&program, "Research") {
        UseArgs::Named(pairs) => {
            assert_eq!(pairs.len(), 3, "three keyword args");
            // v2.10.0 — each entry carries its value_kind. All three are
            // literals (string / int / bool).
            assert_eq!(
                pairs[0],
                ("query".to_string(), "${q}".to_string(), "literal".to_string())
            );
            assert_eq!(
                pairs[1],
                ("max_results".to_string(), "5".to_string(), "literal".to_string())
            );
            assert_eq!(
                pairs[2],
                ("safesearch".to_string(), "true".to_string(), "literal".to_string())
            );
        }
        other => panic!("expected UseArgs::Named, got {other:?}"),
    }
}

#[test]
fn legacy_on_arg_still_parses_to_legacy_positional_back_compat() {
    // v2.8.0 D5 — the v2.7.0 single-`on <arg>` form must keep parsing to
    // `LegacyPositional` with the verbatim interpolation string.
    let program = parse(
        r#"
tool WebSearch { provider: http }
flow Research(query: String) -> Any {
    use WebSearch on "${query}"
}
"#,
    );
    assert_eq!(
        first_use_tool(&program, "Research"),
        UseArgs::LegacyPositional("${query}".to_string())
    );
}

#[test]
fn bare_use_with_no_args_is_empty_legacy_positional() {
    let program = parse(
        r#"
tool Ping { provider: http }
flow F() -> Any {
    use Ping
}
"#,
    );
    assert_eq!(
        first_use_tool(&program, "F"),
        UseArgs::LegacyPositional(String::new())
    );
}

#[test]
fn named_args_tolerate_trailing_comma_and_empty_parens() {
    let program = parse(
        r#"
tool T { provider: http parameters: { a: String } }
flow WithTrailing() -> Any { use T(a = "x",) }
flow WithEmpty() -> Any { use T() }
"#,
    );
    match first_use_tool(&program, "WithTrailing") {
        UseArgs::Named(p) => assert_eq!(p.len(), 1, "trailing comma ok"),
        other => panic!("expected Named, got {other:?}"),
    }
    match first_use_tool(&program, "WithEmpty") {
        UseArgs::Named(p) => assert!(p.is_empty(), "empty parens → no args"),
        other => panic!("expected Named (empty), got {other:?}"),
    }
}

#[test]
fn named_arg_values_capture_dotted_references_and_literals() {
    let program = parse(
        r#"
tool Enrich { provider: http parameters: { from: String, limit: Int } }
flow F(prev: String) -> Any {
    use Enrich(from = prev, limit = 10)
}
"#,
    );
    match first_use_tool(&program, "F") {
        UseArgs::Named(pairs) => {
            // v2.10.0 — `from = prev` is a REFERENCE (bare identifier);
            // `limit = 10` is a literal.
            assert_eq!(
                pairs[0],
                ("from".to_string(), "prev".to_string(), "reference".to_string())
            );
            assert_eq!(
                pairs[1],
                ("limit".to_string(), "10".to_string(), "literal".to_string())
            );
        }
        other => panic!("expected Named, got {other:?}"),
    }
}
