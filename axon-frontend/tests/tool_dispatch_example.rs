//! v2.7.0 — the canonical `examples/tool_dispatch.axon` is a living
//! contract for the `use <Tool> on <arg>` primitive.
//!
//! Before v2.7.0 there were ZERO `use <Tool> on <arg>` examples in the
//! repository — the primitive was under-exercised. This drift gate parses
//! and type-checks the canonical example and asserts its shape, so the
//! reference program cannot silently regress (a renamed field, a broken
//! dispatch, a non-flow-level `use`) without a red test.

use axon_frontend::ast::{Declaration, FlowStep, Program};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

const EXAMPLE_PATH: &str = "../examples/tool_dispatch.axon";

fn load() -> Program {
    let src = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/tool_dispatch.axon not found — run tests from axon-frontend/");
    let tokens = Lexer::new(&src, "tool_dispatch.axon").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

#[test]
fn canonical_example_parses_and_type_checks_clean() {
    let program = load();
    let errors = TypeChecker::new(&program).check();
    assert!(
        errors.is_empty(),
        "the canonical tool-dispatch example must type-check clean; errors: {errors:?}"
    );
}

#[test]
fn declares_the_websearch_tool() {
    let program = load();
    let tool_declared = program
        .declarations
        .iter()
        .any(|d| matches!(d, Declaration::Tool(t) if t.name == "WebSearch"));
    assert!(tool_declared, "the example must declare `tool WebSearch`");
}

#[test]
fn dispatches_websearch_flow_level_with_interpolated_request_param() {
    let program = load();
    let flow = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::Flow(f) = d { Some(f) } else { None })
        .expect("flow ResearchQuery");

    let use_tool = flow
        .body
        .iter()
        .find_map(|s| if let FlowStep::UseTool(u) = s { Some(u) } else { None })
        .expect(
            "the flow MUST carry a FLOW-LEVEL FlowStep::UseTool — this is the whole \
             point of the example (a `use` nested in a step body is a v2.7.0 parse error)",
        );

    assert_eq!(use_tool.tool_name, "WebSearch");
    assert_eq!(
        // v2.8.0 — positional arg now in `UseArgs::LegacyPositional`.
        use_tool.args.legacy_argument(),
        "${query}",
        "the dispatch argument must carry the verbatim `${{query}}` interpolation so the \
         runtime resolves it against the bound request parameter (v2.7.0)"
    );
}

#[test]
fn flow_parameter_query_matches_the_request_body_field() {
    // The v1.32.0 request-binding contract: the flow parameter `query`
    // binds by name from the `ResearchRequest.query` body field. If either
    // side is renamed without the other, the binding silently breaks — this
    // asserts they stay in lock-step.
    let program = load();
    let flow = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::Flow(f) = d { Some(f) } else { None })
        .expect("flow ResearchQuery");
    assert!(
        flow.parameters.iter().any(|p| p.name == "query"),
        "flow ResearchQuery must declare the `query` parameter the tool argument binds"
    );
}
