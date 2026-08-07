//! §Fase 119 (D119.4) — `mandate X on Y` / `shield X on Y -> b` / `ots X on Y`
//! are step-body statements.
//!
//! # The decision this gate pins
//!
//! README §XV has always written the governance application INSIDE the step it
//! governs — next to the `output:` it constrains — and the parser accepted the
//! same form only at flow level. That one gap was the real reason README
//! blocks 40–42 never compiled: not `pid` (fixed in §119.b.1), and not mandate
//! being unimplemented. Block 41 failed identically on `shield`, one line
//! before its `mandate` was ever reached.
//!
//! The published position is also the better semantics: a mandate inside a
//! step is scoped to THIS step's generation; the flow-level form is a bare
//! statement whose subject must be inferred. So the compiler moved — the
//! fase's standing rule — and the family shares one AST shape with the
//! flow-level `*ApplyStep` family it mirrors.
//!
//! # What the guards are NOT yet
//!
//! Parsed, carried in the IR, and name-resolved — not yet enforced. Runtime
//! enforcement of a step-scoped mandate is §119.b Tier 1/Tier 2, which
//! consumes `IRStep.guards` together with the `(D, L)` obligations of
//! §119.b.3. Until that lands, `mandate` stays in `KNOWN_DEBT`: a parsed cage
//! is not a cage, and this header says so rather than letting a green gate
//! imply otherwise.

use axon_frontend::ast::{Declaration, FlowStep};
use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn check_msgs(src: &str) -> Vec<String> {
    let program = parse(src);
    TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

/// README block 40's shape, verbatim in structure: declaration + a flow whose
/// step applies the mandate to a binding.
const BLOCK_40_SHAPE: &str = r#"
mandate SECCompliance {
    constraint: "no forward-looking statements without safe harbor language"
    pid { Kp: 2.0, Ki: 0.3, Kd: 0.1 }
    epsilon: 0.05
    max_steps: 8
    on_violation: halt
}

know {
    flow GenerateQuarterlyReport(data: FinancialData) -> SECReport {
        step Draft {
            mandate SECCompliance on data
            output: SECReport
        }
    }
}
"#;

fn first_flow_steps(program: &axon_frontend::ast::Program) -> &[FlowStep] {
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            return &f.body;
        }
    }
    panic!("no flow in program");
}

#[test]
fn a_mandate_inside_a_step_body_parses_and_lands_on_that_step() {
    // Unwrapped variant: `know { }`-nested flows are not top-level
    // declarations in the AST (the IR test below covers the wrapped shape,
    // which is what README block 40 actually writes).
    let program = parse(
        r#"
mandate SECCompliance {
    constraint: "no forward-looking statements without safe harbor language"
    pid { Kp: 2.0, Ki: 0.3, Kd: 0.1 }
    epsilon: 0.05
    max_steps: 8
    on_violation: halt
}

flow GenerateQuarterlyReport(data: FinancialData) -> SECReport {
    step Draft {
        mandate SECCompliance on data
        output: SECReport
    }
}
"#,
    );
    let steps = first_flow_steps(&program);
    let step = steps
        .iter()
        .find_map(|s| match s {
            FlowStep::Step(st) => Some(st),
            _ => None,
        })
        .expect("the flow has a step");
    assert_eq!(step.guards.len(), 1);
    let g = &step.guards[0];
    assert_eq!(
        (g.kind.as_str(), g.name.as_str(), g.target.as_str()),
        ("mandate", "SECCompliance", "data")
    );
    assert!(g.binding.is_empty());
    // And the rest of the step body still parsed around it.
    assert_eq!(step.output_type, "SECReport");
}

#[test]
fn a_shield_guard_with_a_binding_parses_like_readme_block_41() {
    let program = parse(
        r#"
shield PatientShield {
    scan: [pii_leak, hallucination]
    strategy: dual_llm
    on_breach: halt
}

flow GenerateDiagnosis(symptoms: PatientData) -> ClinicalReport {
    step Sanitize {
        shield PatientShield on symptoms -> clean_data
        output: CleanData
    }
}
"#,
    );
    let steps = first_flow_steps(&program);
    let step = steps
        .iter()
        .find_map(|s| match s {
            FlowStep::Step(st) => Some(st),
            _ => None,
        })
        .expect("step");
    let g = &step.guards[0];
    assert_eq!(
        (g.kind.as_str(), g.name.as_str(), g.target.as_str(), g.binding.as_str()),
        ("shield", "PatientShield", "symptoms", "clean_data")
    );
}

#[test]
fn a_call_expression_target_is_captured_verbatim_like_readme_block_42() {
    let program = parse(
        r#"
mandate LegalPrecision {
    constraint: "c"
    pid { Kp: 1.5, Ki: 0.4, Kd: 0.15 }
    epsilon: 0.08
    max_steps: 10
    on_violation: coerce
}

flow DraftContract(terms: NegotiationTerms) -> ContractDocument {
    step Generate {
        mandate LegalPrecision on ContractDrafter(terms)
        output: ContractDocument
    }
}
"#,
    );
    let steps = first_flow_steps(&program);
    let step = steps
        .iter()
        .find_map(|s| match s {
            FlowStep::Step(st) => Some(st),
            _ => None,
        })
        .expect("step");
    let g = &step.guards[0];
    assert_eq!(g.name, "LegalPrecision");
    assert!(
        g.target.starts_with("ContractDrafter(") && g.target.ends_with(')'),
        "the call target must survive verbatim, got {:?}",
        g.target
    );
    assert!(
        g.target.contains("terms"),
        "the argument must not be dropped: {:?}",
        g.target
    );
}

#[test]
fn guards_travel_in_the_ir_scoped_to_their_step() {
    let program = parse(BLOCK_40_SHAPE);
    let errors = TypeChecker::new(&program).check();
    assert!(errors.is_empty(), "{errors:?}");
    let ir = IRGenerator::new().generate(&program);
    let flow = ir.flows.iter().find(|f| f.name == "GenerateQuarterlyReport");
    let flow = flow.expect("flow reaches the IR");
    let step = flow
        .steps
        .iter()
        .find_map(|n| match n {
            axon_frontend::ir_nodes::IRFlowNode::Step(s) => Some(s),
            _ => None,
        })
        .expect("IR step");
    assert_eq!(step.guards.len(), 1, "dispatch cannot enforce a guard the IR does not carry");
    assert_eq!(step.guards[0].name, "SECCompliance");
    assert_eq!(step.guards[0].kind, "mandate");
}

#[test]
fn a_stepless_program_serialises_without_a_guards_key() {
    // The §68.b/§91.a discipline: pre-§119 programs' IR JSON stays
    // byte-identical. An empty guards vec must be elided, not serialised as [].
    let program = parse(
        r#"
flow F(x: T) -> U {
    step S {
        ask: "q"
        output: U
    }
}
"#,
    );
    let ir = IRGenerator::new().generate(&program);
    let json = serde_json::to_string(&ir).expect("serialise");
    assert!(
        !json.contains("\"guards\""),
        "an empty guards key would shift every pre-§119 IR-SHA"
    );
}

#[test]
fn a_guard_naming_a_ghost_is_a_compile_error() {
    let msgs = check_msgs(
        r#"
flow F(x: T) -> U {
    step S {
        mandate DoesNotExist on x
        output: U
    }
}
"#,
    );
    assert!(
        msgs.iter()
            .any(|m| m.contains("Undefined mandate 'DoesNotExist'") && m.contains("step 'S'")),
        "a cage around nothing must be refused: {msgs:?}"
    );
}

#[test]
fn a_guard_naming_the_wrong_kind_is_a_compile_error() {
    let msgs = check_msgs(
        r#"
shield RealShield {
    scan: [pii_leak]
    strategy: dual_llm
    on_breach: halt
}

flow F(x: T) -> U {
    step S {
        mandate RealShield on x
        output: U
    }
}
"#,
    );
    assert!(
        msgs.iter()
            .any(|m| m.contains("is a shield, not a mandate")),
        "kinds must not be interchangeable: {msgs:?}"
    );
}

#[test]
fn the_old_error_message_still_fires_for_tokens_that_are_not_guards() {
    // The step body did not become a lawless zone: only the three governance
    // applications gained statement position.
    let tokens = Lexer::new(
        r#"
flow F(x: T) -> U {
    step S {
        for item in x { }
        output: U
    }
}
"#,
        "<test>",
    )
    .tokenize()
    .expect("lex");
    let err = Parser::new(tokens).parse().expect_err("`for` must still be refused");
    assert!(
        err.message.contains("Unexpected token in step body"),
        "{}",
        err.message
    );
}
