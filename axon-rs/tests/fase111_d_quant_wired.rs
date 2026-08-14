//! §Fase 111.d — `quant` + `yield` MADE REAL: the simulator is wired to the keyword.
//!
//! # What §111 found (F12)
//!
//! `run_quant` was a **pure no-op**: it inserted `__quant_backend` (a key nothing
//! in the tree read), returned an empty output, and **silently skipped every step
//! inside `quant { … }`** — even though `ir_generator` had faithfully lowered
//! them — while `StepComplete` went out on the wire saying the block had
//! finished. `run_yield` stored the carrier expression's raw **text** into
//! `__quant_yield` and collapsed no amplitudes.
//!
//! The simulator was real the whole time (`axon::quant`: `ReferenceSimulator`,
//! `PauliSum`, `StateVector`, with its own fuzz suite) and **no dispatch path
//! could reach it** — `DispatchCtx` had no port, so even enterprise's
//! `Q32Simulator` was reachable only from `POST /api/v1/quant/{name}`.
//!
//! Pins:
//! 1. A `quant` block **measures**: `E = ⟨ψ|M|ψ⟩` over the encoded carrier.
//! 2. The body **runs** (it used to be skipped in silence).
//! 3. The measurement is real arithmetic, not a constant: an all-zero carrier
//!    leaves the register in |0…0⟩ so ⟨Z⟩ = +1 exactly, and a rotated carrier
//!    moves it.
//! 4. No simulator ⇒ `MissingDependency`.
//! 5. **`depth:` is REFUSED** — it declares an L-layer `U(θ)` and the language
//!    carries no θ. Running `U(0)` and calling it the adopter's circuit would
//!    fabricate the physics.
//! 6. No / unresolvable `observable:` ⇒ refusal (E = ⟨ψ|M|ψ⟩ with no M is a
//!    category error, not a weak result).
//! 7. `yield` outside `quant` ⇒ refusal — returning 0.0 would be
//!    indistinguishable from a genuine expectation of zero.
//! 8. A carrier that is not a real vector ⇒ refusal, never a guess.
//! 9. A register above the OSS cap ⇒ `axon-E0783`, never a silent truncation.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, DispatchError, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::*;
use axon::quant::ReferenceSimulator;
use std::sync::Arc;
use tokio::sync::mpsc;

/// `M = Z₀` — a single-qubit Pauli-Z on qubit 0, padded with identities.
fn observable_z(name: &str, qubits: usize) -> IRObservable {
    let mut pauli = String::from("Z");
    for _ in 1..qubits {
        pauli.push('I');
    }
    IRObservable {
        node_type: "observable",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        qubits: Some(qubits as i64),
        terms: vec![IRPauliTerm {
            coefficient: 1.0,
            pauli,
        }],
    }
}

fn quant_node(
    observable: Option<&str>,
    depth: Option<i64>,
    body: Vec<IRFlowNode>,
) -> IRFlowNode {
    IRFlowNode::Quant(IRQuant {
        node_type: "quant",
        source_line: 0,
        source_column: 0,
        encoding: Some("angle".into()),
        observable: observable.map(String::from),
        qubits: None,
        depth,
        bandwidth: None,
        reupload: None,
        effect: "quant_sim".into(),
        body,
    })
}

fn yield_node(expr: &str) -> IRFlowNode {
    IRFlowNode::Yield(IRYield {
        node_type: "yield",
        source_line: 0,
        source_column: 0,
        value_expr: expr.into(),
        value_kind: "reference".into(),
    })
}

fn let_node(target: &str, value: &str) -> IRFlowNode {
    IRFlowNode::Let(IRLetBinding {
        node_type: "let_binding",
        source_line: 0,
        source_column: 0,
        target: target.into(),
        value: value.into(),
        value_kind: "literal".into(),
        value_ast: None,
    })
}

fn ctx_with_quant(
    observables: Vec<IRObservable>,
) -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = DispatchCtx::new("QFlow", "stub", "", CancellationFlag::new(), tx)
        .with_quant(Arc::new(ReferenceSimulator::new()), Arc::new(observables));
    (ctx, rx)
}

fn ctx_without_quant() -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        DispatchCtx::new("QFlow", "stub", "", CancellationFlag::new(), tx),
        rx,
    )
}

async fn expectation_of(ctx: &mut DispatchCtx, carrier: &str) -> f64 {
    dispatch_node(&let_node("x", carrier), ctx)
        .await
        .expect("bind carrier");
    let outcome = dispatch_node(
        &quant_node(Some("M"), None, vec![yield_node("x")]),
        ctx,
    )
    .await
    .expect("quant must measure");
    let output = match outcome {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("expected Completed, got {other:?}"),
    };
    let v: serde_json::Value = serde_json::from_str(&output).expect("quant binds a JSON result");
    assert_eq!(v["observable"], "M", "the result must say WHAT it measured");
    v["expectation"].as_f64().expect("a real expectation")
}

// ── §Fase 122.b — the same measurement, driven from SOURCE ──────────────────
//
// Every gate in this file hand-builds `IRObservable` and `IRQuant` in Rust,
// which proves the HANDLER. Law 4 asks for the PATH: a program an adopter could
// write, compiled by the real pipeline, dispatched as the compiler lowered it,
// against the same `ReferenceSimulator` the runner mounts.
//
// The observable is the single-qubit Pauli-Z and the carrier is the all-zero
// vector, so the register stays in |0⟩ and ⟨Z⟩ = +1 EXACTLY. The expected value
// is analytic — it cannot be satisfied by a placeholder, and it is not an oracle
// invented for this fixture.

/// Compile a fixture through the real pipeline — INCLUDING the type checker.
///
/// §Fase 122.b: skipping the checker would let a fixture break a compile-time
/// law without any gate noticing. A program that does not type-check is not one
/// an adopter could deploy, so it cannot stand as a Law 4 citation.
fn compile_fixture(rel: &str) -> IRProgram {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let tokens = axon_frontend::lexer::Lexer::new(&src, rel)
        .tokenize()
        .expect("fixture must lex");
    let prog = axon_frontend::parser::Parser::new(tokens)
        .parse()
        .expect("fixture must parse");
    let errors = axon_frontend::type_checker::TypeChecker::new(&prog).check();
    assert!(
        errors.is_empty(),
        "the fixture must TYPE-CHECK — an adopter could not deploy it otherwise: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    axon_frontend::ir_generator::IRGenerator::new().generate(&prog)
}

#[tokio::test]
async fn a_compiled_program_reaches_the_simulator_and_is_measured() {
    const FIXTURE: &str = "tests/fixtures/fase122_b_quant/ground_state_measurement.axon";
    let ir = compile_fixture(FIXTURE);

    // The observable catalog the COMPILER produced from `observable GroundEnergy`.
    assert!(
        !ir.observables.is_empty(),
        "the fixture declares `observable GroundEnergy`; an empty catalog means \
         the declaration no longer reaches the IR"
    );
    let (mut ctx, _rx) = ctx_with_quant(ir.observables.clone());

    let steps: Vec<IRFlowNode> = ir
        .flows
        .iter()
        .flat_map(|f| f.steps.iter().cloned())
        .collect();
    assert!(
        steps.iter().any(|n| matches!(n, IRFlowNode::Quant(_))),
        "the fixture's `quant(...)` block must lower to an IRQuant node"
    );

    let mut measured = None;
    for node in &steps {
        let outcome = dispatch_node(node, &mut ctx)
            .await
            .expect("every node in the compiled fixture must dispatch");
        if matches!(node, IRFlowNode::Quant(_)) {
            measured = match outcome {
                NodeOutcome::Completed { output, .. } => Some(output),
                other => panic!("expected Completed from quant, got {other:?}"),
            };
        }
    }

    let output = measured.expect("the quant node must have produced a result");
    let v: serde_json::Value = serde_json::from_str(&output).expect("quant binds a JSON result");

    // It must say WHAT it measured, and the name came from the declaration.
    assert_eq!(v["observable"], "GroundEnergy");

    let e = v["expectation"].as_f64().expect("a real expectation");
    assert!(
        (e - 1.0).abs() < 1e-9,
        "|0⟩ under Z must give ⟨Z⟩ = +1 exactly; got {e}"
    );
}

// ── 1-3. The flagship: a quant block that actually measures ─────────────────

/// An all-zero carrier under angle encoding applies no rotation, so the register
/// stays in |0…0⟩ and ⟨Z₀⟩ = +1 **exactly**. This is a real computation with an
/// analytically-known answer — it cannot be faked by a placeholder.
#[tokio::test]
async fn quant_measures_the_ground_state_exactly() {
    let (mut ctx, _rx) = ctx_with_quant(vec![observable_z("M", 2)]);
    let e = expectation_of(&mut ctx, "[0.0, 0.0]").await;
    assert!(
        (e - 1.0).abs() < 1e-9,
        "|0…0⟩ under Z₀ must give ⟨Z⟩ = +1 exactly; got {e}"
    );
}

/// …and a rotated carrier MOVES it. A constant would pass the test above; only a
/// real measurement passes both.
#[tokio::test]
async fn a_rotated_carrier_changes_the_expectation() {
    let (mut ctx, _rx) = ctx_with_quant(vec![observable_z("M", 2)]);
    let ground = expectation_of(&mut ctx, "[0.0, 0.0]").await;
    let rotated = expectation_of(&mut ctx, "[1.2, 0.0]").await;

    assert!(
        (ground - rotated).abs() > 1e-6,
        "rotating qubit 0 must change ⟨Z₀⟩ — a handler that returns a constant would pass the \
         ground-state test alone. ground={ground}, rotated={rotated}"
    );
    assert!(
        (-1.0..=1.0).contains(&rotated),
        "⟨Z⟩ is bounded by the spectrum of Z: |⟨Z⟩| ≤ 1. Got {rotated}"
    );
}

/// The body used to be SKIPPED — every step inside `quant { … }` silently
/// dropped while the wire reported completion.
#[tokio::test]
async fn the_quant_body_runs() {
    let (mut ctx, _rx) = ctx_with_quant(vec![observable_z("M", 1)]);
    ctx.let_bindings.insert("x".into(), "[0.0]".into());

    dispatch_node(
        &quant_node(
            Some("M"),
            None,
            vec![let_node("body_ran", "yes"), yield_node("x")],
        ),
        &mut ctx,
    )
    .await
    .expect("quant must run");

    assert_eq!(
        ctx.let_bindings.get("body_ran").map(String::as_str),
        Some("yes"),
        "steps inside a `quant` block were silently skipped — they must execute"
    );
}

// ── 4-9. Every joint fails CLOSED ───────────────────────────────────────────

#[tokio::test]
async fn no_simulator_refuses() {
    let (mut ctx, _rx) = ctx_without_quant();
    let err = dispatch_node(&quant_node(Some("M"), None, vec![]), &mut ctx)
        .await
        .expect_err("no simulator must refuse");
    match err {
        DispatchError::MissingDependency { name } => assert_eq!(name, "quant_backend"),
        other => panic!("expected MissingDependency{{quant_backend}}, got {other:?}"),
    }
}

/// **The honest refusal.** `depth: L` declares an L-layer variational circuit
/// `U(θ)` — and the language carries **no θ**: `IRQuant` has no parameter vector,
/// and a `RotationLayer` needs real angles. §51 shipped the knob and never
/// shipped its parameter source.
///
/// Running `U(0)` and calling it the adopter's circuit would be fabricating the
/// physics — the exact class of defect §111 exists to end.
#[tokio::test]
async fn declared_depth_refuses_because_the_language_carries_no_theta() {
    let (mut ctx, _rx) = ctx_with_quant(vec![observable_z("M", 1)]);
    ctx.let_bindings.insert("x".into(), "[0.0]".into());

    let err = dispatch_node(&quant_node(Some("M"), Some(3), vec![yield_node("x")]), &mut ctx)
        .await
        .expect_err("a declared depth with no θ must refuse");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("no θ") || msg.contains("no \\u{3b8}") || msg.contains("parameter surface"),
        "the diagnostic must name the missing parameter source, not just reject the knob; got {msg}"
    );
    assert!(
        msg.contains("fabricate"),
        "it must say WHY we refuse rather than running U(0): that would fabricate the physics; \
         got {msg}"
    );
}

#[tokio::test]
async fn missing_observable_refuses() {
    let (mut ctx, _rx) = ctx_with_quant(vec![]);
    let err = dispatch_node(&quant_node(None, None, vec![]), &mut ctx)
        .await
        .expect_err("no observable must refuse");
    assert!(format!("{err:?}").contains("needs an M"));
}

#[tokio::test]
async fn unresolvable_observable_refuses() {
    let (mut ctx, _rx) = ctx_with_quant(vec![observable_z("M", 1)]);
    let err = dispatch_node(&quant_node(Some("Ghost"), None, vec![]), &mut ctx)
        .await
        .expect_err("an unresolvable observable must refuse");
    assert!(format!("{err:?}").contains("nothing to measure"));
}

/// A measurement with no state to measure is a **category error**. Returning
/// `0.0` would be indistinguishable from a genuine expectation of zero — the
/// same "silence looks like a result" defect that made the old `warden` an
/// anti-feature.
#[tokio::test]
async fn yield_outside_a_quant_block_refuses() {
    let (mut ctx, _rx) = ctx_with_quant(vec![observable_z("M", 1)]);
    ctx.let_bindings.insert("x".into(), "[0.0]".into());

    let err = dispatch_node(&yield_node("x"), &mut ctx)
        .await
        .expect_err("a bare yield must refuse");
    let msg = format!("{err:?}");
    assert!(msg.contains("outside a `quant"), "got {msg}");
    assert!(
        msg.contains("indistinguishable"),
        "the diagnostic must name the failure mode it prevents; got {msg}"
    );
}

#[tokio::test]
async fn a_non_vector_carrier_refuses_rather_than_guessing() {
    let (mut ctx, _rx) = ctx_with_quant(vec![observable_z("M", 1)]);
    ctx.let_bindings
        .insert("x".into(), "the quarterly report".into());

    let err = dispatch_node(&quant_node(Some("M"), None, vec![yield_node("x")]), &mut ctx)
        .await
        .expect_err("a prose carrier must refuse");
    assert!(format!("{err:?}").contains("does not resolve to a real vector"));
}

/// The OSS reference simulator is capped. A register above the cap fails closed
/// with the stable diagnostic `axon-E0783` — **never a silent truncation**,
/// which would quietly measure a different system than the one declared.
#[tokio::test]
async fn a_register_above_the_oss_cap_refuses_with_its_diagnostic_code() {
    let (mut ctx, _rx) = ctx_with_quant(vec![observable_z("M", 64)]);
    // Angle encoding: d features → d qubits. 64 ≫ the OSS cap.
    let big: Vec<String> = (0..64).map(|_| "0.1".to_string()).collect();
    ctx.let_bindings
        .insert("x".into(), format!("[{}]", big.join(", ")));

    let err = dispatch_node(&quant_node(Some("M"), None, vec![yield_node("x")]), &mut ctx)
        .await
        .expect_err("an over-capacity register must refuse");
    assert!(
        format!("{err:?}").contains("axon-E0783"),
        "the refusal must carry the stable capacity diagnostic; got {err:?}"
    );
}
