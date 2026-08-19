//! v2.46.0 — grammar + AST + IR + type-checker for `now:` (declared
//! cognitive time — the cognitive completion of `time_is_an_explicit_input`;
//! `docs/cycle/temporal_cognitive_context.md`, axon-enterprise repo).
//!
//! Pinned properties:
//! 1. A step-level `now: "<IANA-tz>"` parses into `StepNode.now_tz`.
//! 2. A context-level `now:` parses into `ContextDefinition.now_tz`.
//! 3. Both lower to IR (`IRStep.now_tz` / `IRContext.now_tz`).
//! 4. **IR-SHA invariance**: a program with no `now:` serializes with no
//! `now_tz` key anywhere — byte-identical to pre-v2.46.0 IR.
//! 5. A well-formed `now:` (step + context) produces zero diagnostics;
//!    `"UTC"` is valid.
//! 6. **axon-T892** — a `now:` that is not a plausible IANA name (no slash,
//!    leading/trailing slash, empty) — on BOTH surfaces.
//! 7. A non-string `now:` value in a step body is a parse error at the
//!    exact token.

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

fn first_step(prog: &axon_frontend::ast::Program) -> &axon_frontend::ast::StepNode {
    prog.declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Flow(f) => f.body.iter().find_map(|s| match s {
                axon_frontend::ast::FlowStep::Step(st) => Some(st),
                _ => None,
            }),
            _ => None,
        })
        .expect("no step")
}

fn first_context(prog: &axon_frontend::ast::Program) -> &axon_frontend::ast::ContextDefinition {
    prog.declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Context(c) => Some(c),
            _ => None,
        })
        .expect("no context declaration")
}

const STEP_NOW: &str = "flow Plan() -> Unit {\n\
    step Triage {\n\
        now: \"America/Bogota\"\n\
        ask: \"Propose three visit slots this week.\"\n\
    }\n\
}\n";

const CONTEXT_NOW: &str = "context Scheduling {\n\
    depth: standard\n\
    now: \"America/Bogota\"\n\
}\n";

#[test]
fn step_now_parses_into_ast() {
    let prog = parse(STEP_NOW);
    let s = first_step(&prog);
    assert_eq!(s.name, "Triage");
    assert_eq!(s.now_tz.as_deref(), Some("America/Bogota"));
}

#[test]
fn context_now_parses_into_ast() {
    let prog = parse(CONTEXT_NOW);
    let c = first_context(&prog);
    assert_eq!(c.name, "Scheduling");
    assert_eq!(c.now_tz.as_deref(), Some("America/Bogota"));
    assert_eq!(c.depth, "standard");
}

#[test]
fn now_lowers_to_ir_on_both_surfaces() {
    let src = format!("{CONTEXT_NOW}{STEP_NOW}");
    let prog = parse(&src);
    let ir = IRGenerator::new().generate(&prog);

    let ctx = ir.contexts.first().expect("no context in IR");
    assert_eq!(ctx.now_tz.as_deref(), Some("America/Bogota"));

    let json = serde_json::to_string(&ir).expect("serialize");
    // Both surfaces present ⇒ two now_tz occurrences.
    assert_eq!(
        json.matches("\"now_tz\"").count(),
        2,
        "expected step + context now_tz in IR JSON: {json}"
    );
}

#[test]
fn now_less_program_has_no_ir_drift() {
    let src = "context Plain { depth: standard }\n\
               flow Chat() -> Unit { step S { ask: \"hi\" requires_context: 8000 } }\n";
    let prog = parse(src);
    let ir = IRGenerator::new().generate(&prog);
    let json = serde_json::to_string(&ir).expect("serialize");
    assert!(
        !json.contains("\"now_tz\""),
        "no `now:` declaration ⇒ no `now_tz` key anywhere in IR JSON (IR-SHA stability): {json}"
    );
}

#[test]
fn well_formed_now_produces_no_diagnostics() {
    let src = format!("{CONTEXT_NOW}{STEP_NOW}");
    let errors = check_errors(&src);
    let mine: Vec<_> = errors
        .iter()
        .filter(|m| m.contains("now") || m.contains("axon-T892"))
        .collect();
    assert!(mine.is_empty(), "expected clean check, got: {mine:?}");
}

#[test]
fn utc_is_a_valid_zone_on_both_surfaces() {
    let src = "context C { now: \"UTC\" }\n\
               flow F() -> Unit { step S { now: \"UTC\" ask: \"hi\" } }\n";
    let errors = check_errors(src);
    assert!(
        errors.iter().all(|m| !m.contains("axon-T892")),
        "UTC must be accepted: {errors:?}"
    );
}

#[test]
fn t892_step_zone_without_slash_is_an_error() {
    let src = "flow F() -> Unit { step S { now: \"Bogota\" ask: \"hi\" } }";
    let errors = check_errors(src);
    assert!(
        errors.iter().any(|m| m.contains("axon-T892") && m.contains("step 'S'")),
        "expected axon-T892 on step, got: {errors:?}"
    );
}

#[test]
fn t892_context_malformed_zones_are_errors() {
    for bad in ["/America/Bogota", "America/Bogota/", "", "EST5EDT_nope/"] {
        let src = format!("context C {{ now: \"{bad}\" }}");
        let errors = check_errors(&src);
        assert!(
            errors.iter().any(|m| m.contains("axon-T892")),
            "expected axon-T892 for {bad:?}, got: {errors:?}"
        );
    }
}

#[test]
fn step_now_requires_a_string_literal() {
    let src = "flow F() -> Unit { step S { now: true ask: \"hi\" } }";
    let err = try_parse(src).expect_err("non-string now: must be a parse error");
    assert!(
        err.message.contains("`now:` must be an IANA timezone string literal"),
        "unexpected parse error: {}",
        err.message
    );
}
