//! v2.8.0 — the founder's lossless-compilation invariant, drift-gated
//! on the CANONICAL example.
//!
//! The v2.8.0 structured tool data — a tool's `parameters:` schema +
//! `output_type:`, and a `use <Tool>(k = v, …)` call's `named_args` — MUST
//! survive serialization into the JSON-IR with **zero information loss**.
//! This is the artifact the PCC proof bundle binds to (v2.8.0): if a key
//! were dropped or renamed in the JSON-IR, an independent verifier could
//! not re-derive the tool-call-soundness property from the artifact.
//!
//! v2.8.0 pins the invariant on an inline program; THIS gate pins it on the
//! living `examples/tool_dispatch_structured.axon`, serializing the WHOLE
//! `IRProgram` (not just the tool node), so a regression in the canonical
//! reference program — or in program-level IR serialization — fails here.

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

const EXAMPLE_PATH: &str = "../examples/tool_dispatch_structured.axon";

fn canonical_ir_json() -> serde_json::Value {
    let src = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/tool_dispatch_structured.axon not found — run from axon-frontend/");
    let tokens = Lexer::new(&src, "tool_dispatch_structured.axon")
        .tokenize()
        .expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    let ir = IRGenerator::new().generate(&program);
    serde_json::to_value(&ir).expect("serialize the whole IRProgram to JSON-IR")
}

#[test]
fn tool_schema_survives_into_program_json_ir_losslessly() {
    let json = canonical_ir_json();
    let tool = json["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .find(|t| t["name"] == "CrmRadar")
        .expect("CrmRadar tool in JSON-IR");

    // The typed input schema — names, types, optionality — all present.
    let params = tool["parameters"].as_array().expect("parameters array");
    let pairs: Vec<(&str, &str, bool)> = params
        .iter()
        .map(|p| {
            (
                p["name"].as_str().unwrap(),
                p["type_name"].as_str().unwrap(),
                p["optional"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("company", "String", false),
            ("max_results", "Int", false),
            ("active", "Bool", false),
        ],
        "the typed parameters schema must survive into the JSON-IR verbatim"
    );

    // The declared output type (D8) survives.
    assert_eq!(
        tool["output_type"], "CrmReport",
        "output_type must survive into the JSON-IR"
    );
}

#[test]
fn use_tool_named_args_survive_into_program_json_ir_losslessly() {
    let json = canonical_ir_json();
    let flow = json["flows"]
        .as_array()
        .expect("flows array")
        .iter()
        .find(|f| f["name"] == "ScanCrm")
        .expect("ScanCrm flow in JSON-IR");
    let use_tool = flow["steps"]
        .as_array()
        .expect("steps array")
        .iter()
        .find(|s| s["tool_name"] == "CrmRadar" && s["named_args"].is_array())
        .expect("the flow's use_tool node in JSON-IR");

    let named: Vec<(&str, &str)> = use_tool["named_args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| (a["name"].as_str().unwrap(), a["value"].as_str().unwrap()))
        .collect();
    assert_eq!(
        named,
        vec![("company", "company"), ("max_results", "5"), ("active", "true")],
        "the structured keyword args must survive into the JSON-IR verbatim"
    );
}
