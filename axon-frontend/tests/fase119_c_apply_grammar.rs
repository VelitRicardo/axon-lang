//! §Fase 119.c — the grammar that let README's lambda/ots blocks compile.
//!
//! Three independent repairs, each pinned here:
//!
//! 1. `[T]` in type position is `List<T>` sugar — README flow signatures
//!    have always written `readings: [SensorReading]` (blocks 44-45), and
//!    §39.a's own comments name `List<T>` as the canonical carrier, so the
//!    sugar lowers to EXACTLY that shape and nothing downstream learns a new
//!    one.
//! 2. `loss_function:` accepts the identifier form the README always wrote
//!    (`SemanticPreservation`, `L2`) alongside the string form.
//! 3. `lambda X on y -> b` is a step-body statement (blocks 46-47's exact
//!    position) — the D119.4 statement position extended to the fourth
//!    member of the apply family. Unlike the mandate guard it is an
//!    ELEVATION: dispatch runs it BEFORE the step's generation.

use axon_frontend::ast::{Declaration, FlowStep};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

#[test]
fn bracket_type_sugar_lowers_to_exactly_list_of_t() {
    let program = parse(
        r#"
flow ProcessSensorData(readings: [SensorReading]) -> AnalysisReport {
    step S {
        ask: "q"
        output: AnalysisReport
    }
}
"#,
    );
    let flow = program
        .declarations
        .iter()
        .find_map(|d| match d {
            Declaration::Flow(f) => Some(f),
            _ => None,
        })
        .expect("flow");
    let p = &flow.parameters[0];
    assert_eq!(p.name, "readings");
    assert_eq!(
        (p.type_expr.name.as_str(), p.type_expr.generic_param.as_str()),
        ("List", "SensorReading"),
        "[T] must lower to the SAME shape List<T> produces — a different \
         shape would fork every downstream consumer"
    );
    assert!(!p.type_expr.optional);
}

#[test]
fn bracket_sugar_composes_with_nesting_and_optionality() {
    let program = parse(
        r#"
flow F(xs: [List<Row>], maybe: [Row]?) -> U {
    step S {
        ask: "q"
        output: U
    }
}
"#,
    );
    let flow = program
        .declarations
        .iter()
        .find_map(|d| match d {
            Declaration::Flow(f) => Some(f),
            _ => None,
        })
        .expect("flow");
    assert_eq!(flow.parameters[0].type_expr.name, "List");
    assert_eq!(flow.parameters[0].type_expr.generic_param, "List<Row>");
    assert!(flow.parameters[1].type_expr.optional, "[Row]? keeps the ?");
}

#[test]
fn loss_function_accepts_the_identifier_form_the_readme_writes() {
    for (src_val, want) in [
        ("SemanticPreservation", "SemanticPreservation"),
        ("L2", "L2"),
        ("\"L_accuracy + 0.1 * L_complexity\"", "L_accuracy + 0.1 * L_complexity"),
    ] {
        let program = parse(&format!(
            r#"
ots T<A, B> {{
    teleology: "t"
    loss_function: {src_val}
    homotopy_search: shallow
}}
"#
        ));
        let ots = program
            .declarations
            .iter()
            .find_map(|d| match d {
                Declaration::Ots(o) => Some(o),
                _ => None,
            })
            .expect("ots");
        assert_eq!(ots.loss_function, want, "for {src_val}");
    }
}

#[test]
fn a_lambda_inside_a_step_body_parses_as_an_elevation_guard() {
    let program = parse(
        r#"
lambda RawQuote {
    ontology: "finance.quote"
    certainty: 0.9
    provenance: "MarketFeed"
    derivation: raw
}

flow AssessRisk(ticker: String) -> RiskAssessment {
    step Price {
        lambda RawQuote on ticker -> verified_quote
        output: QuoteData
    }
}
"#,
    );
    let flow = program
        .declarations
        .iter()
        .find_map(|d| match d {
            Declaration::Flow(f) => Some(f),
            _ => None,
        })
        .expect("flow");
    let step = flow
        .body
        .iter()
        .find_map(|s| match s {
            FlowStep::Step(st) => Some(st),
            _ => None,
        })
        .expect("step");
    let g = &step.guards[0];
    assert_eq!(
        (g.kind.as_str(), g.name.as_str(), g.target.as_str(), g.binding.as_str()),
        ("lambda", "RawQuote", "ticker", "verified_quote")
    );
}

#[test]
fn a_lambda_guard_naming_a_ghost_is_a_compile_error() {
    let program = parse(
        r#"
flow F(x: T) -> U {
    step S {
        lambda DoesNotExist on x -> y
        output: U
    }
}
"#,
    );
    let errors: Vec<String> = TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect();
    assert!(
        errors
            .iter()
            .any(|m| m.contains("Undefined lambda 'DoesNotExist'")),
        "{errors:?}"
    );
}

#[test]
fn a_lambda_guard_resolves_against_the_lambda_data_symbol_kind() {
    // The guard keyword is `lambda`; the symbol table's kind is
    // `lambda_data`. A declared lambda must NOT be reported as wrong-kind.
    let program = parse(
        r#"
lambda Good {
    ontology: "o"
    certainty: 0.5
    provenance: "p"
    derivation: raw
}

flow F(x: T) -> U {
    step S {
        lambda Good on x -> y
        output: U
    }
}
"#,
    );
    let errors: Vec<String> = TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect();
    assert!(
        !errors.iter().any(|m| m.contains("Good")),
        "a declared lambda must resolve cleanly: {errors:?}"
    );
}
