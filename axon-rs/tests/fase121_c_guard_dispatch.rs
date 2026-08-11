//! §Fase 121.c — **the confidence guard RUNS: bounded, monotone,
//! non-degrading, from `.axon` source.**
//!
//! The loop under test (in `run_validate`):
//!
//! ```text
//! while best.csr < θ && attempt < max_attempts:
//!     refined  = refine(best_value, feedback = best.violations)
//!     rescored = CSR(refined)
//!     if rescored > best.csr { best = rescored; best_value = refined }
//! ```
//!
//! Three properties, each with the failure it forbids:
//!
//! | property | what it forbids |
//! |---|---|
//! | **bounded** | an unbounded retry burning tokens forever |
//! | **monotone** | adopting a refinement WORSE than the original — the stub answers prose (CSR 0), and a loop without the keep-best rule would replace a 0.67 value with a 0.0 one, signed off by the very guard that exists to raise it |
//! | **early-exit** | model calls spent after the floor is already cleared |
//!
//! Exhaustion below the floor PROCEEDS with an honest record (the anchor-retry
//! precedent): `refine` is a recovery, not a gate — `anchor` is the gate.
//!
//! The `stub` backend answers deterministic prose, which makes the DEGRADING
//! refinement the default case here — exactly the case the monotone rule
//! exists for.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::{pure_shape::run_validate, DispatchCtx};
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
    panic!("no validate node in the generated IR");
}

fn ctx() -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx),
        rx,
    )
}

fn refine_steps(rx: &mut mpsc::UnboundedReceiver<FlowExecutionEvent>) -> Vec<String> {
    let mut names = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let FlowExecutionEvent::StepStart { step_name, .. } = ev {
            if step_name.contains(".refine[") {
                names.push(step_name);
            }
        }
    }
    names
}

/// Block 1's `step Check`, verbatim shape: schema + guarded validation.
/// `ContractSchema` denotes `C_T` = {ParsesAsJson, JsonField{parties},
/// JsonField{obligations}} — three obligations.
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

// ── §1 — MONOTONE: a degrading refinement is never adopted ─────────────────

/// Seed a PARTIAL value (2 of 3 obligations ⇒ CSR = 0.6667, below the 0.8
/// floor). The guard fires; the stub's refinements are prose (CSR 0). A loop
/// without the keep-best rule would adopt them and hand downstream a value
/// strictly worse than the author already had.
#[tokio::test]
async fn c1_a_degrading_refinement_is_never_adopted() {
    let (mut c, mut rx) = ctx();
    let original = r#"{"parties": "Acme and Beta"}"#;
    c.let_bindings
        .insert("Assess.output".to_string(), original.to_string());

    let v = validate_from_source(GUARDED);
    run_validate(&v, &mut c).await.expect("run_validate");

    // Both attempts were SPENT (bounded, and it really tried)…
    let attempts = refine_steps(&mut rx);
    assert_eq!(
        attempts.len(),
        2,
        "max_attempts: 2 with a floor never cleared must spend exactly 2 \
         attempts, got {attempts:?}"
    );

    // …and NONE was adopted: the governed value is still the original, and
    // the confidence is the ORIGINAL's 2/3 — not the prose's 0.
    assert_eq!(
        c.let_bindings.get("Assess.output").map(String::as_str),
        Some(original),
        "the stub refined DOWNWARD (prose, CSR 0); adopting it would have the \
         guard sign off a value worse than the one it was raising"
    );
    let conf: f64 = c.let_bindings.get("Assess.output.confidence").unwrap().parse().unwrap();
    assert!(
        (conf - 2.0 / 3.0).abs() < 1e-3,
        "the bound confidence is the BEST seen (the original's 2/3), got {conf}"
    );
    assert_eq!(
        c.let_bindings.get("Assess.output.passed").map(String::as_str),
        Some("false"),
        "exhaustion below the floor proceeds — with an HONEST record"
    );
    assert!(
        !c.let_bindings.get("Assess.output.violations").unwrap().is_empty(),
        "…including the constraint that is still unmet"
    );
}

// ── §2 — EARLY EXIT: a cleared floor spends nothing ─────────────────────────

#[tokio::test]
async fn c2_a_conforming_value_spends_zero_attempts() {
    let (mut c, mut rx) = ctx();
    c.let_bindings.insert(
        "Assess.output".to_string(),
        r#"{"parties": "Acme", "obligations": "deliver"}"#.to_string(),
    );

    let v = validate_from_source(GUARDED);
    run_validate(&v, &mut c).await.expect("run_validate");

    assert_eq!(
        refine_steps(&mut rx).len(),
        0,
        "CSR 1.0 ≥ 0.8: a guard that fires anyway is burning model calls on a \
         value that already conforms"
    );
    assert_eq!(
        c.let_bindings.get("Assess.output.confidence").map(String::as_str),
        Some("1.0000")
    );
    assert_eq!(
        c.let_bindings.get("Assess.output.passed").map(String::as_str),
        Some("true")
    );
}

// ── §3 — the recovery is OBSERVABLE on the wire ─────────────────────────────

/// Each attempt is a real wire step named `<target>.refine[i]`. An adopter
/// watching the wire sees the WORK, not just a score that moved.
#[tokio::test]
async fn c3_each_attempt_is_a_named_wire_step_in_order() {
    let (mut c, mut rx) = ctx();
    c.let_bindings
        .insert("Assess.output".to_string(), "not json at all".to_string());

    let v = validate_from_source(GUARDED);
    run_validate(&v, &mut c).await.expect("run_validate");

    let names = refine_steps(&mut rx);
    assert_eq!(
        names,
        vec![
            "Assess.output.refine[1]".to_string(),
            "Assess.output.refine[2]".to_string()
        ],
        "numbered, in order, under the target they refine"
    );
}

// ── §4 — a validation WITHOUT a guard loops zero times ─────────────────────

/// The guard is opt-in. A bare `validate … against:` scores once and binds;
/// no recovery is implied that the author did not write.
#[tokio::test]
async fn c4_an_unguarded_validation_never_refines() {
    let (mut c, mut rx) = ctx();
    c.let_bindings
        .insert("Assess.output".to_string(), "prose".to_string());

    let v = validate_from_source(
        "type S { a: String }\n\
         flow F(d: Text) -> Text { step C { validate Assess.output against: S output: Text } }\n",
    );
    assert!(v.guard.is_none());
    run_validate(&v, &mut c).await.expect("run_validate");

    assert_eq!(refine_steps(&mut rx).len(), 0);
    assert_eq!(
        c.let_bindings.get("Assess.output.confidence").map(String::as_str),
        Some("0.0000"),
        "scored honestly — just never retried"
    );
}
