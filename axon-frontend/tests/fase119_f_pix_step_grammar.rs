//! §Fase 119.f — the pix/corpus family: PIX verbs as step-body statements.
//!
//! §117 built the README ledger and predicted this exact family: *"ONE
//! `pix`/`corpus` header-grammar decision clears fifteen blocks."* The
//! decision turned out to be the D119.4 shape a second time — the README
//! writes the PIX verbs INSIDE the step they feed, with a braceless field
//! list, and the parser accepted them only at flow level and only braced:
//!
//! ```text
//! step Search {
//!     navigate ContractIndex
//!         query: question          ← a BINDING, not a literal
//!         trail: enabled           ← not the literal `true`
//!         depth: 3
//!         as: relevant_sections    ← the binding the step's `ask:` uses
//! }
//! ```
//!
//! Each verb is an **elevation**: it produces a binding before the step
//! generates, exactly like §119.c's `lambda`/`ots` guards. Reusing the
//! flow-level node types (no new AST shapes) is the D119.4 doctrine — one
//! concept, two positions.
//!
//! The braceless list needs no delimiter because the field names are a
//! CLOSED catalog: "the next token is one of these and is followed by a
//! colon" is an unambiguous continuation test, and a name outside the
//! catalog ends the verb and belongs to the enclosing step.

use axon_frontend::ast::{Declaration, FlowStep};
use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn first_step(program: &axon_frontend::ast::Program) -> &axon_frontend::ast::StepNode {
    program
        .declarations
        .iter()
        .find_map(|d| match d {
            Declaration::Flow(f) => f.body.iter().find_map(|s| match s {
                FlowStep::Step(st) => Some(st),
                _ => None,
            }),
            _ => None,
        })
        .expect("a step")
}

/// README block 17's shape, verbatim in structure.
const NAVIGATE_STEP: &str = r#"
pix ContractIndex {
    source: "contracts/master.md"
    depth: 4
    branching: 3
}

flow AnalyzeContract(question: String) -> LegalAnalysis {
    step Search {
        navigate ContractIndex
            query: question
            trail: enabled
            depth: 3
            as: relevant_sections
        output: LegalAnalysis
    }
}
"#;

#[test]
fn the_braceless_navigate_parses_every_field_the_readme_writes() {
    let program = parse(NAVIGATE_STEP);
    let step = first_step(&program);
    assert_eq!(step.pix_ops.len(), 1, "one PIX op on the step");
    match &step.pix_ops[0] {
        FlowStep::Navigate(n) => {
            assert_eq!(n.pix_name, "ContractIndex");
            assert_eq!(
                n.query_expr, "question",
                "`query: question` is a BINDING reference — the parser used to \
                 demand a string literal, which is why every published navigate \
                 failed on its own second line"
            );
            assert!(n.trail_enabled, "`trail: enabled` means on, like `true`");
            assert_eq!(n.depth, Some(3), "the per-navigation depth override");
            assert_eq!(n.output_name, "relevant_sections");
        }
        other => panic!("expected Navigate, got {other:?}"),
    }
    // The step's own fields still parse AROUND the braceless list — the
    // closed-catalog terminator working.
    assert_eq!(step.output_type, "LegalAnalysis");
}

#[test]
fn the_field_catalog_terminates_the_list_without_a_delimiter() {
    // `output:` is NOT a navigate field in this position: it belongs to the
    // step. If the terminator were wrong, the navigate would swallow it and
    // `step.output_type` would be empty.
    let step_src = NAVIGATE_STEP.replace("as: relevant_sections", "as: sections");
    let program = parse(&step_src);
    let step = first_step(&program);
    assert_eq!(step.output_type, "LegalAnalysis");
    match &step.pix_ops[0] {
        FlowStep::Navigate(n) => assert_eq!(n.output_name, "sections"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_braced_form_still_parses_identically() {
    // Backwards compatibility is a theorem, not a hope: the flow-level braced
    // form predates this and must be unchanged.
    let program = parse(
        r#"
pix P { source: "a.md" }

flow F(q: String) -> R {
    navigate P { query: "literal" trail: true as: out }
    step S { ask: "x" output: R }
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
    let nav = flow
        .body
        .iter()
        .find_map(|s| match s {
            FlowStep::Navigate(n) => Some(n),
            _ => None,
        })
        .expect("flow-level navigate");
    assert_eq!(nav.query_expr, "literal");
    assert!(nav.trail_enabled);
    assert_eq!(nav.output_name, "out");
}

#[test]
fn drill_takes_a_title_string_and_a_dotted_binding_path() {
    // README writes BOTH: `into "Liabilities"` and `into findings.top_region`.
    for (into, want) in [
        ("\"Liabilities\"", "Liabilities"),
        ("findings.top_region", "findings.top_region"),
    ] {
        let program = parse(&format!(
            r#"
pix P {{ source: "a.md" }}

flow F(q: String) -> R {{
    step D {{
        drill P
            into {into}
            query: q
            as: detail
        output: R
    }}
}}
"#
        ));
        let step = first_step(&program);
        match &step.pix_ops[0] {
            FlowStep::Drill(d) => {
                assert_eq!(d.subtree_path, want, "for `into {into}`");
                assert_eq!(d.output_name, "detail");
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn trail_and_validate_are_step_body_statements_too() {
    let program = parse(
        r#"
flow F(q: String) -> R {
    step V {
        trail sections
        validate guideline against: ClinicalSchema
        output: R
    }
}
"#,
    );
    let step = first_step(&program);
    assert_eq!(step.pix_ops.len(), 2);
    match (&step.pix_ops[0], &step.pix_ops[1]) {
        (FlowStep::Trail(t), FlowStep::Validate(v)) => {
            assert_eq!(t.navigate_ref, "sections");
            assert_eq!(v.target, "guideline");
            assert_eq!(v.rule, "ClinicalSchema", "the `against:` clause");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_navigate_field_form_is_untouched_by_the_statement_form() {
    // `navigate: <Ref>` (the FIELD) and `navigate <Ref> …` (the STATEMENT)
    // are told apart by the token after the keyword. Both must still work.
    let program = parse(
        r#"
flow F(q: String) -> R {
    step S {
        navigate: SomeIndex
        ask: "x"
        output: R
    }
}
"#,
    );
    let step = first_step(&program);
    assert_eq!(step.navigate_ref, "SomeIndex");
    assert!(step.pix_ops.is_empty(), "the field form makes no PIX op");
}

#[test]
fn the_depth_override_reaches_the_ir_so_something_can_consume_it() {
    // A declared field consumed by nothing is the §111 defect. `depth:`
    // reaches the IR here and `NavConfig.d_max` at dispatch (axon-rs).
    let program = parse(NAVIGATE_STEP);
    let errors = TypeChecker::new(&program).check();
    assert!(errors.is_empty(), "{errors:?}");
    let ir = IRGenerator::new().generate(&program);
    let json = serde_json::to_string(&ir).expect("serialise");
    assert!(
        json.contains("\"depth\":3"),
        "the override must survive into the IR: {}",
        &json[..json.len().min(400)]
    );
}

#[test]
fn an_absent_depth_is_elided_so_pre_119f_programs_keep_their_ir_sha() {
    let program = parse(
        r#"
pix P { source: "a.md" }

flow F(q: String) -> R {
    navigate P { query: "x" as: out }
    step S { ask: "y" output: R }
}
"#,
    );
    let ir = IRGenerator::new().generate(&program);
    // Look at the NAVIGATE node, not the whole program: the `pix`
    // DECLARATION legitimately carries its own `depth:`.
    let nav = ir
        .flows
        .iter()
        .flat_map(|f| f.steps.iter())
        .find_map(|n| match n {
            axon_frontend::ir_nodes::IRFlowNode::Navigate(x) => Some(x),
            _ => None,
        })
        .expect("flow-level navigate in the IR");
    let json = serde_json::to_string(nav).expect("serialise");
    assert!(
        !json.contains("\"depth\""),
        "an absent override must be elided, not serialised as null: {json}"
    );
}
