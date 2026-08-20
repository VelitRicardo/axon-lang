//! v2.42.0 — grammar + AST + IR + static discipline for the `synth`
//! dynamic tool-synthesis policy (`synth <Name> { target:, risk:, language:,
//! sandbox:, review:, max_lines: }`). See `the design plan`.
//!
//! `synth` is the paper's "OTS = Ontological Tool Synthesis" grounded to a real
//! keyword (`ots` already means one-shot media transform — paper section 9.1). It
//! declares the SAFETY ENVELOPE under which a savant may write + run a tool at
//! runtime (Coder/Reviewer `par` → wasm32-wasi → Extism zero-trust). OSS parses
//! + disciplines the policy and ships a DENY-BY-DEFAULT reference; the executor
//! is enterprise (v2.42.0).
//!
//! Pinned properties:
//! 1. A full `synth` parses + lowers; `review` defaults to `required`.
//! 2. IR-SHA invariance: no `synth` ⇒ no `synths` key.
//! 3. **the design decision** — unknown field is a hard parse error.
//! 4. **T879** target required. **T880** risk required + catalog.
//!    **T881** language catalog. **T882** deny-by-default `sandbox: wasm`.
//!    **T883** review catalog + high/critical risk must be reviewed.

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::{ParseError, Parser};
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn try_parse(src: &str) -> Result<axon_frontend::ast::Program, ParseError> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse()
}

fn check_errors(src: &str) -> Vec<String> {
    let prog = parse(src);
    TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

fn has_code(errs: &[String], code: &str) -> bool {
    errs.iter().any(|e| e.contains(code))
}

fn ir_json(src: &str) -> String {
    let prog = parse(src);
    let ir = IRGenerator::new().generate(&prog);
    serde_json::to_string(&ir).expect("serialize IR")
}

fn first_synth(prog: &axon_frontend::ast::Program) -> &axon_frontend::ast::SynthDefinition {
    prog.declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Synth(s) => Some(s),
            _ => None,
        })
        .expect("no synth declaration")
}

const FLOW: &str = "flow Chat() -> Unit { step S { ask: \"hi\" } }\n";

const FULL: &str = r#"
synth Toolsmith {
    target: "parse geospatial datasets",
    risk: medium,
    language: rust,
    sandbox: wasm,
    review: required,
    max_lines: 400
}
"#;

#[test]
fn full_synth_parses_every_field() {
    let prog = parse(FULL);
    let s = first_synth(&prog);
    assert_eq!(s.name, "Toolsmith");
    assert_eq!(s.target, "parse geospatial datasets");
    assert_eq!(s.risk, "medium");
    assert_eq!(s.language, "rust");
    assert_eq!(s.sandbox, "wasm");
    assert_eq!(s.review, "required");
    assert_eq!(s.max_lines, Some(400));
}

#[test]
fn full_synth_lowers_and_review_defaults_required() {
    // Omit `review:` → IR must fill `required` (fail-closed default).
    let src = "synth T { target: \"x\", risk: low, sandbox: wasm }\n";
    let json = ir_json(src);
    assert!(json.contains("\"synths\""));
    assert!(json.contains("\"required\""), "review defaults to required: {json}");
}

#[test]
fn no_synth_leaves_ir_byte_identical() {
    let json = ir_json(FLOW);
    assert!(!json.contains("synths"), "no synth ⇒ no synths key: {json}");
}

#[test]
fn unknown_field_is_a_parse_error() {
    let src = "synth T { target: \"x\", risk: low, sandbox: wasm, bogus: 1 }\n";
    let err = try_parse(src).expect_err("unknown field must fail parse");
    assert!(err.message.contains("bogus"), "{}", err.message);
}

#[test]
fn t879_target_required() {
    let src = "synth T { risk: low, sandbox: wasm }\n";
    assert!(has_code(&check_errors(src), "axon-T879"));
}

#[test]
fn t880_risk_required_and_catalog() {
    let missing = "synth T { target: \"x\", sandbox: wasm }\n";
    assert!(has_code(&check_errors(missing), "axon-T880"), "missing risk");
    let bad = "synth T { target: \"x\", risk: apocalyptic, sandbox: wasm }\n";
    assert!(has_code(&check_errors(bad), "axon-T880"), "unknown risk");
}

#[test]
fn t881_language_catalog() {
    let src = "synth T { target: \"x\", risk: low, language: cobol, sandbox: wasm }\n";
    assert!(has_code(&check_errors(src), "axon-T881"));
}

#[test]
fn t882_sandbox_deny_by_default() {
    let missing = "synth T { target: \"x\", risk: low }\n";
    assert!(has_code(&check_errors(missing), "axon-T882"), "no sandbox");
    let bad = "synth T { target: \"x\", risk: low, sandbox: none }\n";
    assert!(has_code(&check_errors(bad), "axon-T882"), "non-wasm sandbox");
}

#[test]
fn t883_review_catalog_and_high_risk_must_review() {
    let bad_cat = "synth T { target: \"x\", risk: low, sandbox: wasm, review: maybe }\n";
    assert!(has_code(&check_errors(bad_cat), "axon-T883"), "unknown review");

    let high_unreviewed =
        "synth T { target: \"x\", risk: critical, sandbox: wasm, review: none }\n";
    assert!(
        has_code(&check_errors(high_unreviewed), "axon-T883"),
        "critical risk with review:none must fail"
    );
}

#[test]
fn well_formed_synth_is_check_clean() {
    let errs = check_errors(&format!("{FULL}{FLOW}"));
    assert!(
        errs.iter().all(|e| !e.contains("axon-T87")),
        "well-formed synth must raise no v2.42.0 diagnostic: {errs:?}"
    );
}

#[test]
fn low_risk_may_skip_review() {
    // `review: none` is only forbidden at high/critical risk.
    let src = "synth T { target: \"x\", risk: low, sandbox: wasm, review: none }\n";
    assert!(!has_code(&check_errors(src), "axon-T883"));
}
