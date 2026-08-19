//! v2.83.0 — **a SUBJECT is a dotted reference.**
//!
//! Every statement in the language has two kinds of operand, and until this
//! cycle one function parsed both:
//!
//!   - a **NAME** — the declaration being applied (`compute CalculatePremium`,
//!     `mandate SECFormat`). Always bare; a dotted name refers to nothing.
//!   - a **SUBJECT** — what the statement acts ON (`validate Assess.output`,
//!     `probe student.recent_interactions`, `compute X on Analyze.risk_factor`).
//!
//! Subject positions called `consume_any_ident_or_kw`, which stops at the dot —
//! so `Assess.output`, the canonical way one step names another's result, was
//! rejected in the position that needs it most. It already parsed inside
//! `given:`, inside `use_tool … with k: v`, and in `navigate_ref`; the helper
//! (`parse_dotted_identifier`) had existed since v2.12.0.
//!
//! Measured 2026-08-08: **five** README blocks fail on this as their FIRST
//! error and three more need it further in. It is the widest single lever left
//! in the ledger, and the fix is mechanical — which is why it went unnoticed:
//! nothing about it looks like language work.
//!
//! Literals are subjects too (`compute X on Profile.tenure, 1.2, "USD"`), and a
//! string literal KEEPS ITS QUOTES through the AST so dispatch can tell a
//! literal from a binding name. Flattening them would reintroduce the exact
//! ambiguity v2.10.0's literal/reference classification exists to remove.

use axon_frontend::ast::{Declaration, FlowStep};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn parse(src: &str) -> Result<axon_frontend::ast::Program, axon_frontend::parser::ParseError> {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    Parser::new(tokens).parse()
}

fn first_flow_body(program: &axon_frontend::ast::Program) -> &Vec<FlowStep> {
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            return &f.body;
        }
    }
    panic!("no flow");
}

// ── section 1 — `compute X on <dotted>, <literal>` ──────────────────────────────────

#[test]
fn f10_1_compute_arguments_are_dotted_references_and_literals() {
    // README block 48's application line, verbatim.
    let src = r#"
flow ProcessApplication(client: Document) -> PolicyReport {
    compute CalculatePremium on Analyze.risk_factor, 1.2, 5.0 -> premium
}
"#;
    let program = parse(src).expect("the README's own `compute` application must parse");
    for s in first_flow_body(&program) {
        if let FlowStep::ComputeApply(c) = s {
            assert_eq!(c.compute_name, "CalculatePremium", "the NAME stays bare");
            assert_eq!(
                c.arguments,
                vec![
                    "Analyze.risk_factor".to_string(),
                    "1.2".to_string(),
                    "5.0".to_string()
                ],
                "the dotted reference must survive whole, and numeric literals \
                 are subjects too"
            );
            assert_eq!(c.output_name, "premium");
            return;
        }
    }
    panic!("no ComputeApply node");
}

#[test]
fn f10_2_a_string_literal_argument_keeps_its_quotes() {
    // README block 53: `compute NormalizeCurrency on tx.amount, tx.currency, "USD" -> normalized`.
    // The quotes are the v2.10.0 classification made durable: without them dispatch
    // would look `USD` up as a binding name.
    let src = r#"
flow F(tx: Tx) -> R {
    compute NormalizeCurrency on tx.amount, tx.currency, "USD" -> normalized
}
"#;
    let program = parse(src).expect("parse");
    for s in first_flow_body(&program) {
        if let FlowStep::ComputeApply(c) = s {
            assert_eq!(
                c.arguments,
                vec![
                    "tx.amount".to_string(),
                    "tx.currency".to_string(),
                    "\"USD\"".to_string()
                ]
            );
            return;
        }
    }
    panic!("no ComputeApply node");
}

// ── section 2 — the step-body statements ────────────────────────────────────────────

fn step_ops(program: &axon_frontend::ast::Program) -> Vec<&FlowStep> {
    let mut out = Vec::new();
    for s in first_flow_body(program) {
        if let FlowStep::Step(st) = s {
            for op in &st.pix_ops {
                out.push(op);
            }
        }
    }
    out
}

#[test]
fn f10_3_validate_takes_a_dotted_subject() {
    // README blocks 1 and 16: `validate Assess.output against: ContractSchema`.
    let src = r#"
flow F(a: String) -> R {
    step Check {
        validate Assess.output against: ContractSchema
        output: R
    }
}
"#;
    let program = parse(src).expect("parse");
    for op in step_ops(&program) {
        if let FlowStep::Validate(v) = op {
            assert_eq!(v.target, "Assess.output");
            assert_eq!(v.rule, "ContractSchema");
            return;
        }
    }
    panic!("no Validate node");
}

#[test]
fn f10_4_probe_takes_a_dotted_subject() {
    // README psyche: `probe student.recent_interactions for [...]`.
    let src = r#"
flow F(student: StudentProfile) -> R {
    step Assess {
        probe student.recent_interactions for [comprehension_signals, error_patterns]
        output: R
    }
}
"#;
    let program = parse(src).expect("parse");
    for op in step_ops(&program) {
        if let FlowStep::Probe(p) = op {
            assert_eq!(p.target, "student.recent_interactions");
            assert_eq!(p.fields.len(), 2);
            return;
        }
    }
    panic!("no Probe node");
}

#[test]
fn f10_5_a_guards_on_target_takes_a_dotted_subject() {
    let src = r#"
flow F(a: String) -> R {
    step Verify {
        shield ComplianceShield on Charge.output -> verified
        output: R
    }
}
"#;
    let program = parse(src).expect("parse");
    for s in first_flow_body(&program) {
        if let FlowStep::Step(st) = s {
            let g = st.guards.first().expect("one guard");
            assert_eq!(g.name, "ComplianceShield", "the NAME stays bare");
            assert_eq!(g.target, "Charge.output", "the SUBJECT is dotted");
            assert_eq!(g.binding, "verified");
            return;
        }
    }
    panic!("no step");
}

// ── section 3 — the NAME position is NOT dotted ─────────────────────────────────────

/// The distinction is the point: widening the subject must not widen the name.
/// `compute A.B on x` would name a declaration that cannot exist, and accepting
/// it would turn a typo into a silent lookup failure at dispatch.
#[test]
fn f10_6_the_bare_forms_are_unchanged() {
    let src = r#"
flow F(a: String) -> R {
    compute Score on a -> s
    step Check {
        validate guideline against: ClinicalSchema
        output: R
    }
}
"#;
    let program = parse(src).expect("the pre-v2.83.0 bare forms must still parse");
    let mut saw_compute = false;
    for s in first_flow_body(&program) {
        if let FlowStep::ComputeApply(c) = s {
            assert_eq!(c.compute_name, "Score");
            assert_eq!(c.arguments, vec!["a".to_string()]);
            saw_compute = true;
        }
    }
    assert!(saw_compute, "the bare compute application must still parse");
}
