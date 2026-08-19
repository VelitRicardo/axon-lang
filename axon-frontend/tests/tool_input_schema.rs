//! v2.8.0 — a `tool` can declare a typed INPUT SCHEMA + an OUTPUT type.
//!
//! Brief #22 / W2: pre-v2.8.0 a `tool {}` could only declare
//! `provider/effects/timeout/max_results/filter/runtime/sandbox` — ZERO input
//! parameters — so there was no caller↔tool contract for the type-checker to
//! validate against. v2.8.0 adds `parameters: { k: Type, … }` (the schema) and
//! `output_type: <Type>` (so `${Step.output}` is typed, v2.8.0 D8). This gate
//! pins the parse shape and the v2.8.0 D5 back-compat (a schema-less tool still
//! parses).

use axon_frontend::ast::{Declaration, Program};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn parse(src: &str) -> Program {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn tool<'a>(p: &'a Program, name: &str) -> &'a axon_frontend::ast::ToolDefinition {
    p.declarations
        .iter()
        .find_map(|d| match d {
            Declaration::Tool(t) if t.name == name => Some(t),
            _ => None,
        })
        .unwrap_or_else(|| panic!("tool {name} not declared"))
}

#[test]
fn tool_declares_typed_input_schema_and_output_type() {
    let program = parse(
        r#"
tool WebSearch {
    provider: http
    runtime: tool_server
    parameters: {
        query: String,
        max_results: Int,
        safesearch: Bool
    }
    output_type: SearchResults
}
"#,
    );
    let t = tool(&program, "WebSearch");

    // The schema is the caller↔tool contract (W2).
    assert_eq!(t.parameters.len(), 3, "three declared input parameters");
    assert_eq!(t.parameters[0].name, "query");
    assert_eq!(t.parameters[0].type_expr.name, "String");
    assert_eq!(t.parameters[1].name, "max_results");
    assert_eq!(t.parameters[1].type_expr.name, "Int");
    assert_eq!(t.parameters[2].name, "safesearch");
    assert_eq!(t.parameters[2].type_expr.name, "Bool");

    // The output type makes a tool-step's result typed/referenceable (D8).
    assert_eq!(t.output_type.as_deref(), Some("SearchResults"));

    // The pre-existing fields still parse alongside.
    assert_eq!(t.provider, "http");
    assert_eq!(t.runtime, "tool_server");
}

#[test]
fn schema_supports_generic_and_optional_param_types() {
    // The schema reuses the flow-parameter `TypeExpr` grammar, so generics
    // and `?`-optionals work exactly as in flow signatures.
    let program = parse(
        r#"
tool Enrich {
    provider: http
    parameters: {
        ids: List<String>,
        hint: String?
    }
}
"#,
    );
    let t = tool(&program, "Enrich");
    assert_eq!(t.parameters[0].name, "ids");
    assert_eq!(t.parameters[0].type_expr.name, "List");
    assert_eq!(t.parameters[0].type_expr.generic_param, "String");
    assert_eq!(t.parameters[1].name, "hint");
    assert!(t.parameters[1].type_expr.optional, "`String?` is optional");
    // No output_type declared → None.
    assert_eq!(t.output_type, None);
}

#[test]
fn trailing_comma_and_empty_schema_are_tolerated() {
    let program = parse(
        r#"
tool A {
    provider: http
    parameters: {
        only: String,
    }
}
tool B {
    provider: http
    parameters: {}
}
"#,
    );
    assert_eq!(tool(&program, "A").parameters.len(), 1, "trailing comma ok");
    assert!(tool(&program, "B").parameters.is_empty(), "empty schema ok");
}

#[test]
fn schemaless_tool_still_parses_unchanged_back_compat() {
    // v2.8.0 D5 — a tool that declares no `parameters:` is the legacy shape and
    // must keep parsing with an empty schema (the single-`on <arg>` dispatch
    // form still applies to it).
    let program = parse(
        r#"
tool WebSearch {
    provider: http
    max_results: 5
    timeout: 30s
}
"#,
    );
    let t = tool(&program, "WebSearch");
    assert!(t.parameters.is_empty(), "no schema declared → empty");
    assert_eq!(t.output_type, None);
    assert_eq!(t.provider, "http");
    assert_eq!(t.max_results, Some(5));
}
