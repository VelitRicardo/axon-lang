//! v2.88.0 — **the confidence guard: grammar → AST → IR, and every closed
//! catalog watched refusing.**
//!
//! The published form (blocks 1, 16, 18 of the README):
//!
//! ```text
//! validate Assess.output against: ContractSchema
//! if confidence < 0.8 -> refine(max_attempts: 2)
//! ```
//!
//! Design decision this file pins: the guard is a FIELD of the validation it
//! governs (`ValidateStep::guard`), not a sibling statement. As a sibling,
//! "which confidence does this read?" needs a resolution rule, an error for
//! the zero case, and dispatch-side re-pairing — three places to drift. As a
//! field, a guard with no validation is UNREPRESENTABLE, and the pairing
//! survives into the IR untouched.
//!
//! Every position in the form is a closed catalog of ONE (metric, comparison,
//! action, argument). A test per refusal, because a catalog nobody watched
//! refuse is a free string with extra steps.

use axon_frontend::ast::{Declaration, FlowStep, Program};
use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::{IRFlowNode, IRValidateStep};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> Program {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("must parse")
}

fn parse_err(src: &str) -> String {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    match Parser::new(tokens).parse() {
        Ok(_) => panic!("expected a PARSE error"),
        Err(e) => e.message,
    }
}

fn errors(src: &str) -> String {
    let prog = parse(src);
    TypeChecker::new(&prog)
        .check_with_warnings()
        .0
        .iter()
        .map(|e| e.message.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_validate(src: &str) -> IRValidateStep {
    let ir = IRGenerator::new().generate(&parse(src));
    for flow in &ir.flows {
        for node in &flow.steps {
            if let IRFlowNode::Step(s) = node {
                for op in &s.pix_ops {
                    if let IRFlowNode::Validate(v) = op {
                        return v.clone();
                    }
                }
            }
        }
    }
    panic!("no validate in the IR");
}

/// Block 1's `step Check`, verbatim shape, schema declared.
const GUARDED: &str = r#"
type ContractSchema {
    parties:     String
    obligations: String
}

flow F(doc: Text) -> Text {
    step Check {
        validate Assess.output against: ContractSchema
        if confidence < 0.8 -> refine(max_attempts: 2)
        output: Text
    }
}
"#;

// ── section 1 — the published surface parses and the guard rides its validation ────

#[test]
fn b1_the_published_form_parses_and_attaches() {
    let prog = parse(GUARDED);
    let Declaration::Flow(f) = &prog.declarations[1] else { panic!("flow") };
    let FlowStep::Step(s) = &f.body[0] else { panic!("step") };
    let FlowStep::Validate(v) = &s.pix_ops[0] else { panic!("validate") };
    let g = v.guard.as_ref().expect("the guard attaches to ITS validation");
    assert!((g.threshold - 0.8).abs() < 1e-12);
    assert_eq!(g.max_attempts, 2);
}

#[test]
fn b1b_the_guard_reaches_the_ir_paired() {
    let v = first_validate(GUARDED);
    let g = v.guard.expect("the pairing must survive into the IR");
    assert!((g.threshold - 0.8).abs() < 1e-12);
    assert_eq!(g.max_attempts, 2);
    assert!(
        v.resolved_schema.is_some(),
        "…next to the schema the recovery re-scores against"
    );
}

#[test]
fn b1c_the_full_program_type_checks_clean() {
    assert!(errors(GUARDED).is_empty(), "{}", errors(GUARDED));
}

// ── section 2 — every closed catalog REFUSES, naming itself ────────────────────────

#[test]
fn b2_a_metric_outside_the_catalog_is_refused() {
    let msg = parse_err(
        "flow F(d: Text) -> Text { step C { validate d against: S\n\
         if certainty < 0.8 -> refine(max_attempts: 2) output: Text } }\n",
    );
    assert!(msg.contains("confidence"), "must name the one metric: {msg}");
    assert!(
        msg.contains("flow level"),
        "…and point general branching to where it lives: {msg}"
    );
}

/// A guard declares a FLOOR. An inverted comparison would refine the outputs
/// that already conform.
#[test]
fn b2b_an_inverted_comparison_is_refused() {
    let msg = parse_err(
        "flow F(d: Text) -> Text { step C { validate d against: S\n\
         if confidence > 0.8 -> refine(max_attempts: 2) output: Text } }\n",
    );
    assert!(msg.contains("FLOOR"), "must say why `<` is the catalog: {msg}");
}

#[test]
fn b2c_an_action_outside_the_catalog_is_refused() {
    let msg = parse_err(
        "flow F(d: Text) -> Text { step C { validate d against: S\n\
         if confidence < 0.8 -> retry(max_attempts: 2) output: Text } }\n",
    );
    assert!(
        msg.contains("refine") && msg.contains("catalog"),
        "an action outside the catalog would advertise a recovery nothing \
         dispatches: {msg}"
    );
}

#[test]
fn b2d_an_argument_outside_the_catalog_is_refused() {
    let msg = parse_err(
        "flow F(d: Text) -> Text { step C { validate d against: S\n\
         if confidence < 0.8 -> refine(retries: 2) output: Text } }\n",
    );
    assert!(msg.contains("max_attempts"), "{msg}");
}

// ── section 3 — the structural law: no validation, no guard ────────────────────────

#[test]
fn b3_a_guard_with_no_validation_is_refused_at_parse() {
    let msg = parse_err(
        "flow F(d: Text) -> Text { step C { \
         if confidence < 0.8 -> refine(max_attempts: 2) output: Text } }\n",
    );
    assert!(
        msg.contains("preceding `validate"),
        "a guard over a score nobody computed is governance theatre: {msg}"
    );
}

/// A `validate` WITHOUT `against:` computes no score, so it cannot carry a
/// guard either — the message must say both halves.
#[test]
fn b3b_a_schemaless_validate_cannot_carry_the_guard() {
    let msg = parse_err(
        "flow F(d: Text) -> Text { step C { validate d\n\
         if confidence < 0.8 -> refine(max_attempts: 2) output: Text } }\n",
    );
    assert!(msg.contains("against:"), "{msg}");
}

#[test]
fn b3c_a_second_guard_on_one_validation_is_refused() {
    let msg = parse_err(
        "flow F(d: Text) -> Text { step C { validate d against: S\n\
         if confidence < 0.8 -> refine(max_attempts: 2)\n\
         if confidence < 0.5 -> refine(max_attempts: 1) output: Text } }\n",
    );
    assert!(
        msg.contains("already carries"),
        "two floors racing one score: {msg}"
    );
}

// ── section 4 — the value laws (type-checker) ──────────────────────────────────────

/// CSR ∈ [0,1] by construction, so `< 0.0` can NEVER fire — dead governance
/// that reads as live, refused rather than warned.
#[test]
fn b4_a_floor_that_cannot_fire_is_refused() {
    let errs = errors(
        "type S { a: String }\n\
         flow F(d: Text) -> Text { step C { validate d against: S\n\
         if confidence < 0.0 -> refine(max_attempts: 2) output: Text } }\n",
    );
    assert!(errs.contains("axon-T1211"), "{errs}");
    assert!(errs.contains("NEVER"), "…and it must say WHICH way it lies: {errs}");
}

/// `< 1.5` ALWAYS fires — an unconditional retry loop disguised as a
/// conditional.
#[test]
fn b4b_a_floor_that_always_fires_is_refused() {
    let errs = errors(
        "type S { a: String }\n\
         flow F(d: Text) -> Text { step C { validate d against: S\n\
         if confidence < 1.5 -> refine(max_attempts: 2) output: Text } }\n",
    );
    assert!(errs.contains("axon-T1211") && errs.contains("ALWAYS"), "{errs}");
}

/// `max_attempts: 0` promises a recovery and performs none.
#[test]
fn b4c_zero_attempts_is_refused() {
    let errs = errors(
        "type S { a: String }\n\
         flow F(d: Text) -> Text { step C { validate d against: S\n\
         if confidence < 0.8 -> refine(max_attempts: 0) output: Text } }\n",
    );
    assert!(errs.contains("axon-T1212"), "{errs}");
}

/// `< 1.0` is a legal floor — "anything short of perfect conformance gets a
/// recovery attempt" is a meaning, not a lie.
#[test]
fn b4d_a_floor_of_exactly_one_is_legal() {
    let errs = errors(
        "type S { a: String }\n\
         flow F(d: Text) -> Text { step C { validate d against: S\n\
         if confidence < 1.0 -> refine(max_attempts: 2) output: Text } }\n",
    );
    assert!(!errs.contains("axon-T1211"), "{errs}");
}

// ── section 5 — nothing moved for programs without guards ──────────────────────────

#[test]
fn b5_a_guardless_program_adds_nothing_to_the_ir_json() {
    let ir = IRGenerator::new().generate(&parse(
        "type S { a: String }\n\
         flow F(d: Text) -> Text { step C { validate d against: S output: Text } }\n",
    ));
    let json = serde_json::to_string(&ir).expect("json");
    assert!(
        !json.contains("confidence_guard"),
        "the guard field must be ELIDED when absent so no pre-v2.88.0 IR-SHA moves"
    );
}
