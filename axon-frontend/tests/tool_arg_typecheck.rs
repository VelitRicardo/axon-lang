//! v2.8.0 — the type-checker validates a `use Tool(k = v, …)` call against
//! the tool's declared input schema (W2). A malformed invocation is CALLER
//! blame (CT-2) at compile time, BEFORE any HTTP dispatch.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

fn has(errs: &[String], needle: &str) -> bool {
    errs.iter().any(|e| e.contains(needle))
}

// A tool whose schema has two required params + one optional.
const TOOL: &str = r#"
tool WebSearch {
    provider: http
    parameters: { query: String, max_results: Int, safesearch: Bool? }
}
"#;

#[test]
fn valid_call_typechecks_clean() {
    let errs = errors(&format!(
        "{TOOL}\nflow F(q: String) -> Any {{ use WebSearch(query = \"${{q}}\", max_results = 5) }}"
    ));
    assert!(errs.is_empty(), "a well-formed call must type-check clean: {errs:?}");
}

#[test]
fn unknown_tool_is_caller_blame() {
    let errs = errors("flow F() -> Any { use NoSuchTool(x = 1) }");
    assert!(has(&errs, "Unknown tool 'NoSuchTool'"), "got: {errs:?}");
}

#[test]
fn unknown_parameter_is_caller_blame() {
    let errs = errors(&format!(
        "{TOOL}\nflow F(q: String) -> Any {{ use WebSearch(query = \"${{q}}\", max_results = 5, bogus = 1) }}"
    ));
    assert!(
        has(&errs, "Tool 'WebSearch' has no parameter 'bogus'"),
        "got: {errs:?}"
    );
}

#[test]
fn missing_required_parameter_is_caller_blame() {
    // `max_results` (required) omitted.
    let errs = errors(&format!(
        "{TOOL}\nflow F(q: String) -> Any {{ use WebSearch(query = \"${{q}}\") }}"
    ));
    assert!(
        has(&errs, "Missing required argument 'max_results'"),
        "got: {errs:?}"
    );
}

#[test]
fn optional_parameter_may_be_omitted() {
    // `safesearch` is `Bool?` (optional) — omitting it is fine.
    let errs = errors(&format!(
        "{TOOL}\nflow F(q: String) -> Any {{ use WebSearch(query = \"${{q}}\", max_results = 5) }}"
    ));
    assert!(
        !has(&errs, "safesearch"),
        "optional param must not be required: {errs:?}"
    );
}

#[test]
fn duplicate_argument_is_caller_blame() {
    let errs = errors(&format!(
        "{TOOL}\nflow F(q: String) -> Any {{ use WebSearch(query = \"${{q}}\", query = \"x\", max_results = 5) }}"
    ));
    assert!(
        has(&errs, "Duplicate argument 'query'"),
        "got: {errs:?}"
    );
}

#[test]
fn literal_type_mismatch_is_caught() {
    // `max_results: Int` given a Bool literal.
    let errs = errors(&format!(
        "{TOOL}\nflow F(q: String) -> Any {{ use WebSearch(query = \"${{q}}\", max_results = true) }}"
    ));
    assert!(
        has(&errs, "Type mismatch for parameter 'max_results'"),
        "got: {errs:?}"
    );
}

#[test]
fn int_coerces_into_float_param() {
    let src = r#"
tool T { provider: http parameters: { ratio: Float } }
flow F() -> Any { use T(ratio = 5) }
"#;
    assert!(
        !has(&errors(src), "Type mismatch"),
        "an Int literal must coerce into a Float parameter"
    );
}

#[test]
fn ambiguous_reference_value_skips_typecheck_no_false_positive() {
    // A bare identifier value is ambiguous (string-literal vs reference); the
    // checker must NOT emit a spurious type mismatch (soundness).
    let src = r#"
tool T { provider: http parameters: { from: String } }
flow F(prev: String) -> Any { use T(from = prev) }
"#;
    assert!(
        !has(&errors(src), "Type mismatch"),
        "ambiguous reference value must not false-positive"
    );
}

#[test]
fn legacy_positional_form_skips_schema_validation_back_compat() {
    // v2.8.0 D5 — `use Tool on "${arg}"` is untyped; even against a schema'd tool
    // it must not trigger missing-required / unknown-param errors.
    let errs = errors(&format!(
        "{TOOL}\nflow F(q: String) -> Any {{ use WebSearch on \"${{q}}\" }}"
    ));
    assert!(
        !has(&errs, "Missing required") && !has(&errs, "has no parameter"),
        "legacy positional form must skip schema validation: {errs:?}"
    );
}

#[test]
fn schemaless_tool_with_named_args_is_not_validated() {
    // A tool with no `parameters:` has no contract — named args pass through.
    let src = r#"
tool Bare { provider: http }
flow F() -> Any { use Bare(anything = 1, goes = 2) }
"#;
    let errs = errors(src);
    assert!(
        !has(&errs, "has no parameter") && !has(&errs, "Missing required"),
        "schema-less tool must not be schema-validated: {errs:?}"
    );
}
