//! v2.8.0 — the typed tool schema + structured keyword args survive into
//! the IR (and serialize losslessly).
//!
//! W1 was "the tool arg collapses to one opaque string at the IR level". v2.8.0
//! carries `IRToolSpec.parameters` (+ `output_type`) and `IRUseToolStep
//! .named_args` so the runtime (v2.8.0) can build a structured request body
//! and resolve the output type. The serialization check brings forward the
//! v2.8.0 lossless-compilation invariant in basic form.

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::IRFlowNode;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn ir(src: &str) -> axon_frontend::ir_nodes::IRProgram {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    IRGenerator::new().generate(&prog)
}

const PROGRAM: &str = r#"
tool WebSearch {
    provider: http
    runtime: tool_server
    parameters: { query: String, max_results: Int, tags: List<String>, hint: String? }
    output_type: SearchResults
}
flow Research(q: String) -> Any {
    use WebSearch(query = "${q}", max_results = 5)
}
"#;

#[test]
fn ir_tool_spec_carries_typed_parameters_and_output_type() {
    let ir = ir(PROGRAM);
    let tool = ir
        .tools
        .iter()
        .find(|t| t.name == "WebSearch")
        .expect("WebSearch in IR tools");

    assert_eq!(tool.parameters.len(), 4);
    assert_eq!(tool.parameters[0].name, "query");
    assert_eq!(tool.parameters[0].type_name, "String");
    assert!(!tool.parameters[0].optional);
    assert_eq!(tool.parameters[1].name, "max_results");
    assert_eq!(tool.parameters[1].type_name, "Int");
    // Generic type flattens losslessly into the base type string.
    assert_eq!(tool.parameters[2].name, "tags");
    assert_eq!(tool.parameters[2].type_name, "List<String>");
    // `String?` → base `String`, optionality on the flag (no double-encoding).
    assert_eq!(tool.parameters[3].name, "hint");
    assert_eq!(tool.parameters[3].type_name, "String");
    assert!(tool.parameters[3].optional);

    assert_eq!(tool.output_type.as_deref(), Some("SearchResults"));

    // The v1.23.0 validation hints stay independent and empty (distinct axis).
    assert!(tool.input_schema.is_empty());
    assert!(tool.output_schema.is_empty());
}

#[test]
fn ir_use_tool_carries_named_args() {
    let ir = ir(PROGRAM);
    let flow = ir.flows.iter().find(|f| f.name == "Research").expect("flow");
    let use_tool = flow
        .steps
        .iter()
        .find_map(|s| if let IRFlowNode::UseTool(u) = s { Some(u) } else { None })
        .expect("flow carries an IR use_tool node");

    assert_eq!(use_tool.tool_name, "WebSearch");
    // W1 closed: the structured args survive (not collapsed to one string).
    assert_eq!(use_tool.named_args.len(), 2);
    assert_eq!(use_tool.named_args[0].name, "query");
    assert_eq!(use_tool.named_args[0].value, "${q}");
    assert_eq!(use_tool.named_args[1].name, "max_results");
    assert_eq!(use_tool.named_args[1].value, "5");
    // Named form leaves the legacy positional `argument` empty.
    assert_eq!(use_tool.argument, "");
}

#[test]
fn legacy_positional_use_carries_argument_and_no_named_args() {
    // v2.8.0 D5 — the v2.7.0 single-`on <arg>` form lowers to the legacy
    // `argument` with an empty `named_args`.
    let ir = ir(r#"
tool WebSearch { provider: http }
flow R(query: String) -> Any { use WebSearch on "${query}" }
"#);
    let flow = ir.flows.iter().find(|f| f.name == "R").unwrap();
    let u = flow
        .steps
        .iter()
        .find_map(|s| if let IRFlowNode::UseTool(u) = s { Some(u) } else { None })
        .unwrap();
    assert_eq!(u.argument, "${query}");
    assert!(u.named_args.is_empty());
}

#[test]
fn ir_serializes_losslessly_parameters_output_type_named_args() {
    // v2.8.0 invariant (basic) — the structured tool data must survive JSON-IR
    // serialization with no information loss.
    let ir = ir(PROGRAM);
    let tool = ir.tools.iter().find(|t| t.name == "WebSearch").unwrap();
    let tool_json = serde_json::to_value(tool).expect("serialize IRToolSpec");

    assert_eq!(tool_json["parameters"][0]["name"], "query");
    assert_eq!(tool_json["parameters"][0]["type_name"], "String");
    assert_eq!(tool_json["parameters"][2]["type_name"], "List<String>");
    assert_eq!(tool_json["parameters"][3]["optional"], true);
    assert_eq!(tool_json["output_type"], "SearchResults");

    let flow = ir.flows.iter().find(|f| f.name == "Research").unwrap();
    let use_tool = flow
        .steps
        .iter()
        .find_map(|s| if let IRFlowNode::UseTool(u) = s { Some(u) } else { None })
        .unwrap();
    let ut_json = serde_json::to_value(use_tool).expect("serialize IRUseToolStep");
    assert_eq!(ut_json["named_args"][0]["name"], "query");
    assert_eq!(ut_json["named_args"][0]["value"], "${q}");
    assert_eq!(ut_json["named_args"][1]["value"], "5");
}
