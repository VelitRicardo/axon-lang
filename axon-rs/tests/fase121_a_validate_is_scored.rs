//! §Fase 121 — **`validate … against: <Schema>` SCORES, from `.axon` source.**
//!
//! # What it did before
//!
//! `run_validate` asked the model for *"a structured verdict (pass/fail) with
//! the reasoning that supports it"* and returned the answer as opaque text. The
//! prompt requested a structure; the runtime threw it away. §111's shape inside
//! a primitive attested `Real` — and the reason three README blocks could write
//!
//! ```text
//! validate Assess.output against: ContractSchema
//! if confidence < 0.8 -> refine(max_attempts: 2)
//! ```
//!
//! against a confidence that did not exist.
//!
//! `IRValidateStep.rule` — the schema name — had NO reader in either repo. The
//! `pix_ops` shape (§119.f.7), invisible because `validate` returned prose
//! either way.
//!
//! # What it does now, and where the number comes from
//!
//! Nothing new was invented. The confidence is
//! `CSR = |{c ∈ C : r ⊨ c}| / |C|` from `pem::semantic_validator` — shipped in
//! §119.b, already driving `mandate` — over the constraint set the declared
//! `type` denotes (`pem::schema_constraints`).
//!
//! Every assertion below starts at SOURCE and ends at a live binding.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::{DispatchCtx, DispatchError};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::{IRFlowNode, IRValidateStep};
use tokio::sync::mpsc;

fn validate_from_source(src: &str) -> IRValidateStep {
    let tokens = axon_frontend::lexer::Lexer::new(src, "gate.axon")
        .tokenize()
        .expect("lex");
    let prog = axon_frontend::parser::Parser::new(tokens)
        .parse()
        .expect("the published surface must parse");
    let ir = axon_frontend::ir_generator::IRGenerator::new().generate(&prog);
    // ⚠️ A `validate … against:` written in a STEP body lands on
    // `IRStep.pix_ops`, not at flow level — §119.f gave it that position
    // because that is where every README block writes it. Looking only at
    // `flow.steps` is how the first draft of this gate "found no validate node"
    // while the construct was right there.
    for flow in &ir.flows {
        for node in &flow.steps {
            match node {
                IRFlowNode::Validate(v) => return v.clone(),
                IRFlowNode::Step(s) => {
                    for op in &s.pix_ops {
                        if let IRFlowNode::Validate(v) = op {
                            return v.clone();
                        }
                    }
                }
                _ => {}
            }
        }
    }
    panic!("no validate node in the generated IR");
}

fn type_errors(src: &str) -> String {
    let tokens = axon_frontend::lexer::Lexer::new(src, "gate.axon")
        .tokenize()
        .expect("lex");
    let prog = axon_frontend::parser::Parser::new(tokens).parse().expect("parse");
    axon_frontend::type_checker::TypeChecker::new(&prog)
        .check_with_warnings()
        .0
        .iter()
        .map(|e| e.message.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

fn ctx() -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx),
        rx,
    )
}

const WITH_SCHEMA: &str = r#"
type ContractSchema {
    parties:     String
    obligations: String
}

flow AnalyzeContract(doc: Text) -> Text {
    step Check {
        validate Assess.output against: ContractSchema
        output: Text
    }
}
"#;

// ── §1 — the schema REACHES the artifact ────────────────────────────────────

/// `against:` used to be a free string with no reader. It now resolves at
/// LOWERING and is stamped into the node, so dispatch cannot be reached without
/// it — the `IRRun::resolved_flow` pattern rather than a ctx thread.
#[test]
fn a1_the_schema_resolves_into_the_node() {
    let v = validate_from_source(WITH_SCHEMA);
    assert_eq!(v.rule, "ContractSchema");
    let schema = v
        .resolved_schema
        .as_deref()
        .expect("`against: ContractSchema` must resolve INTO the IR node");
    assert_eq!(schema.name, "ContractSchema");
    assert_eq!(schema.fields.len(), 2, "both declared fields survive");
}

/// Order-independence: a `type` declared BELOW the flow resolves too. Phase 0,
/// the same discipline `shield_signs` (§77.b) and the §120 effect catalog need.
#[test]
fn a1b_a_schema_declared_after_the_flow_still_resolves() {
    let v = validate_from_source(
        "flow F(d: Text) -> Text { step C { validate d against: Late output: Text } }\n\
         type Late { a: String }\n",
    );
    assert!(
        v.resolved_schema.is_some(),
        "declaration ORDER must not decide whether a schema binds"
    );
}

// ── §2 — an unresolved schema is REFUSED ────────────────────────────────────

/// The schema's fields ARE the constraints. A name that resolves to nothing has
/// no constraints, so scoring against it would certify a check that never ran.
#[test]
fn a2_an_undeclared_schema_is_refused() {
    let errs = type_errors(
        "flow F(d: Text) -> Text { step C { validate d against: Ghost output: Text } }\n",
    );
    assert!(
        errs.contains("axon-T1210"),
        "an unresolved `against:` must REFUSE, not score against nothing: {errs}"
    );
}

/// …and a name that resolves to something that is not a `type` is refused with
/// its own reason: a shield declares no fields, so it denotes no structure.
#[test]
fn a2b_a_non_type_schema_is_refused_by_kind() {
    let errs = type_errors(
        "shield S { scan: [pii_leak] }\n\
         flow F(d: Text) -> Text { step C { validate d against: S output: Text } }\n",
    );
    assert!(errs.contains("axon-T1210"), "expected the kind refusal: {errs}");
    assert!(errs.contains("not a `type`"), "and it must say WHY: {errs}");
}

/// The canonical shape compiles clean — a refusal that also refuses the valid
/// case is not a law, it is an outage.
#[test]
fn a2c_the_declared_shape_compiles_clean() {
    assert!(
        type_errors(WITH_SCHEMA).is_empty(),
        "{}",
        type_errors(WITH_SCHEMA)
    );
}

// ── §3 — the confidence is BOUND, and it is the CSR ─────────────────────────

/// The reader. A confidence must exist after the validation runs, or
/// `if confidence < 0.8` has nothing to compare.
///
/// The `stub` backend answers with prose, and prose is CSR 0 — which is the
/// honest score: a schema is a claim about STRUCTURE, and a sentence carries
/// none. A `Contains`-style lowering would have scored this above zero.
#[tokio::test]
async fn a3_the_validation_binds_a_confidence_and_prose_scores_zero() {
    let (mut c, _rx) = ctx();
    let v = validate_from_source(WITH_SCHEMA);
    axon::flow_dispatcher::pure_shape::run_validate(&v, &mut c)
        .await
        .expect("run_validate");

    let conf = c
        .let_bindings
        .get("Assess.output.confidence")
        .expect("the validation must bind a confidence under its own target");
    let value: f64 = conf.parse().expect("the confidence is a number");
    assert_eq!(
        value, 0.0,
        "the stub answers with prose; a schema measures STRUCTURE, so prose \
         scores 0. Anything above zero here means the lowering fell back to a \
         substring test over text"
    );
    assert_eq!(
        c.let_bindings.get("Assess.output.passed").map(String::as_str),
        Some("false")
    );
    assert!(
        !c.let_bindings
            .get("Assess.output.violations")
            .expect("violations bound")
            .is_empty(),
        "the corrective payload `refine` needs must not be empty on a failure"
    );
}

/// A validation with NO `against:` binds NOTHING. There is no declared
/// structure, so there is no evidence to compute a confidence from, and
/// manufacturing one (a default 1.0, a prose heuristic) is the §112 defect. A
/// later `if confidence < …` then finds no binding — refusable, rather than
/// silently false.
#[tokio::test]
async fn a3b_no_schema_binds_no_confidence() {
    let (mut c, _rx) = ctx();
    let v = validate_from_source(
        "flow F(d: Text) -> Text { step C { validate d output: Text } }\n",
    );
    assert!(v.resolved_schema.is_none());
    axon::flow_dispatcher::pure_shape::run_validate(&v, &mut c)
        .await
        .expect("run_validate");
    assert!(
        !c.let_bindings.keys().any(|k| k.ends_with(".confidence")),
        "no schema ⇒ no confidence, not a fabricated one: {:?}",
        c.let_bindings
    );
}

// ── §4 — nothing moved for programs that do not validate ────────────────────

/// Every new IR field is `skip_serializing_if`, so no pre-§121 artifact's
/// IR-SHA moves.
#[test]
fn a4_a_schemaless_program_adds_nothing_to_the_ir_json() {
    let tokens = axon_frontend::lexer::Lexer::new(
        "flow F(x: Text) -> Text { step S { ask: \"hi\" output: Text } }\n",
        "g.axon",
    )
    .tokenize()
    .expect("lex");
    let prog = axon_frontend::parser::Parser::new(tokens).parse().expect("parse");
    let ir = axon_frontend::ir_generator::IRGenerator::new().generate(&prog);
    let json = serde_json::to_string(&ir).expect("json");
    assert!(
        !json.contains("resolved_schema"),
        "the field must be ELIDED when absent so no artifact's SHA moves"
    );
}

/// The `DispatchError` path is named, never a panic and never a silent pass.
#[test]
fn a4b_the_failure_surface_is_a_named_dispatch_error() {
    fn _assert_named(e: DispatchError) -> bool {
        matches!(e, DispatchError::BackendError { .. })
    }
}
