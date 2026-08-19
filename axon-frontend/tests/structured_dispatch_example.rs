//! v2.8.0 — the canonical `examples/tool_dispatch_structured.axon` is a
//! living contract for the structured `use <Tool>(k = v, …)` primitive
//! (v2.8.0: typed tool input schema + keyword-arg dispatch).
//!
//! Sibling of `tool_dispatch_example` (which pins the legacy
//! single-arg `use <Tool> on <arg>` form). This gate parses + type-checks
//! the structured example and asserts its shape, so the reference program
//! cannot silently regress (a renamed `parameters:` field, a broken
//! keyword-arg dispatch, a non-flow-level `use`) without a red test.

use axon_frontend::ast::{Declaration, FlowStep, Program, UseArgs};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

const EXAMPLE_PATH: &str = "../examples/tool_dispatch_structured.axon";

fn load() -> Program {
    let src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "examples/tool_dispatch_structured.axon not found — run tests from axon-frontend/",
    );
    let tokens = Lexer::new(&src, "tool_dispatch_structured.axon")
        .tokenize()
        .expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

#[test]
fn structured_example_parses_and_type_checks_clean() {
    let program = load();
    let errors = TypeChecker::new(&program).check();
    assert!(
        errors.is_empty(),
        "the canonical structured tool-dispatch example must type-check clean; errors: {errors:?}"
    );
}

#[test]
fn declares_crmradar_with_typed_input_schema_and_output_type() {
    let program = load();
    let tool = program
        .declarations
        .iter()
        .find_map(|d| match d {
            Declaration::Tool(t) if t.name == "CrmRadar" => Some(t),
            _ => None,
        })
        .expect("the example must declare `tool CrmRadar`");

    // v2.8.0 — the typed input schema (the call contract).
    let param_names: Vec<&str> = tool.parameters.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        param_names,
        vec!["company", "max_results", "active"],
        "CrmRadar must declare the 3-field typed input schema (v2.8.0)"
    );
    // v2.8.0 — the declared output type (D8).
    assert_eq!(
        tool.output_type.as_deref(),
        Some("CrmReport"),
        "CrmRadar must declare output_type: CrmReport (v2.8.0 D8)"
    );
}

#[test]
fn dispatches_crmradar_flow_level_with_named_keyword_args() {
    let program = load();
    let flow = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::Flow(f) = d { Some(f) } else { None })
        .expect("flow ScanCrm");

    let use_tool = flow
        .body
        .iter()
        .find_map(|s| if let FlowStep::UseTool(u) = s { Some(u) } else { None })
        .expect(
            "the flow MUST carry a FLOW-LEVEL FlowStep::UseTool — a `use` nested in a \
             step body is a v2.7.0 parse error",
        );

    assert_eq!(use_tool.tool_name, "CrmRadar");
    // v2.8.0 — the structured keyword-arg form (NOT the legacy positional).
    match &use_tool.args {
        UseArgs::Named(pairs) => {
            let names: Vec<&str> = pairs.iter().map(|(k, _, _)| k.as_str()).collect();
            assert_eq!(
                names,
                vec!["company", "max_results", "active"],
                "the dispatch must carry the three named keyword args (v2.8.0)"
            );
        }
        UseArgs::LegacyPositional(s) => panic!(
            "the structured example MUST use the keyword-arg form `use CrmRadar(k = v, …)`, \
             not the legacy positional form (got: {s:?})"
        ),
    }
}

#[test]
fn flow_parameter_company_matches_the_request_body_field() {
    // The v1.32.0 request-binding contract: the flow parameter `company`
    // binds by name from the `CrmRequest.company` body field, and is
    // forwarded as the `company =` keyword arg. If any side is renamed
    // without the others, the binding silently breaks — this keeps them
    // in lock-step.
    let program = load();
    let flow = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::Flow(f) = d { Some(f) } else { None })
        .expect("flow ScanCrm");
    assert!(
        flow.parameters.iter().any(|p| p.name == "company"),
        "flow ScanCrm must declare the `company` parameter the tool arg binds"
    );
}
