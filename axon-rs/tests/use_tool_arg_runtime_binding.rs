//! v2.7.0 — runtime contract: a request parameter bound by name
//! reaches a `use <Tool> on "${param}"` dispatch argument.
//!
//! This reproduces, in isolation, the exact two lines the synchronous
//! runner executes for a `use_tool` step (`runner.rs`):
//!
//! ```ignore
//! let raw_arg = step.tool_argument.as_deref().unwrap_or("");
//! let arg = ctx.interpolate(raw_arg);
//! ```
//!
//! where `raw_arg` is the parser-captured `${query}` (see the frontend
//! `use_tool_arg_binding` test) and `ctx` holds the request
//! bindings produced by `request_binding::bind_request`. The two halves
//! are each covered by their own unit tests; this test locks the
//! COMPOSITION — the v2.7.0 acceptance criterion (Kivi brief #17 Q1):
//! a request param actually flows into the tool argument.

use axon::exec_context::ExecContext;
use axon::ir_nodes::{IRFlow, IRParameter};
use axon::request_binding::bind_request_body;

fn param(name: &str) -> IRParameter {
    IRParameter {
        node_type: "parameter",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        type_name: "String".into(),
        generic_param: String::new(),
        optional: false,
    }
}

fn flow_with_params(names: &[&str]) -> IRFlow {
    IRFlow {
        node_type: "flow",
        source_line: 0,
        source_column: 0,
        name: "F".into(),
        parameters: names.iter().map(|n| param(n)).collect(),
        return_type_name: "Unit".into(),
        return_type_generic: String::new(),
        return_type_optional: false,
        steps: Vec::new(),
        edges: Vec::new(),
        execution_levels: Vec::new(),
    }
}

/// Mirror the runner's `use_tool` interception: seed the context with
/// the request bindings, then interpolate the captured tool argument.
fn dispatch_arg(flow: &IRFlow, body: serde_json::Value, captured_tool_arg: &str) -> String {
    let mut ctx = ExecContext::new(&flow.name, "P", 0);
    for (k, v) in bind_request_body(flow, Some(&body)) {
        ctx.set(&k, &v);
    }
    ctx.interpolate(captured_tool_arg)
}

#[test]
fn braced_param_argument_resolves_to_the_bound_request_value() {
    let flow = flow_with_params(&["query"]);
    let body = serde_json::json!({ "query": "rust lifetimes" });
    // `${query}` is exactly what the parser captures for `on "${query}"`.
    assert_eq!(
        dispatch_arg(&flow, body, "${query}"),
        "rust lifetimes",
        "the request param `query` MUST reach the tool dispatch argument"
    );
}

#[test]
fn dollar_name_param_argument_also_resolves() {
    let flow = flow_with_params(&["query"]);
    let body = serde_json::json!({ "query": "borrow checker" });
    assert_eq!(dispatch_arg(&flow, body, "$query"), "borrow checker");
}

#[test]
fn bare_literal_argument_is_passed_through_verbatim() {
    let flow = flow_with_params(&["query"]);
    let body = serde_json::json!({ "query": "ignored" });
    // A bare `on query` captures the literal "query" — no `$` ⇒ the
    // runtime does NOT interpolate; the tool receives the literal text.
    assert_eq!(
        dispatch_arg(&flow, body, "query"),
        "query",
        "a literal argument is never silently treated as a variable reference"
    );
}

#[test]
fn an_unbound_param_reference_is_left_literal_not_emptied() {
    // D4 (request binding): an undeclared/unbound `${typo}` stays literal,
    // never silently empties — so a misspelled binding is observable.
    let flow = flow_with_params(&["query"]);
    let body = serde_json::json!({ "query": "present" });
    assert_eq!(dispatch_arg(&flow, body, "${tpyo}"), "${tpyo}");
}

#[test]
fn argument_embedding_a_param_in_surrounding_text_interpolates_in_place() {
    let flow = flow_with_params(&["query"]);
    let body = serde_json::json!({ "query": "axon" });
    assert_eq!(
        dispatch_arg(&flow, body, "search:${query}:end"),
        "search:axon:end"
    );
}
