//! §Fase 122.b — `effect` / `handle` / `perform`, driven from the PUBLISHED
//! example rather than from a fixture written for the test.
//!
//! # Why the example and not a fixture
//!
//! `examples/algebraic_effects.axon` is what this repository shows an adopter.
//! A citation that points at it is strictly stronger than one pointing at a
//! file authored to satisfy a gate: if the example rots, the row goes red, and
//! the example is the thing people actually read.
//!
//! `fase117_a_examples_compile` already proves the example COMPILES. It does
//! not run it. §120's dispatch gate runs the machine, but from inline source.
//! This joins the two ends: the published program, dispatched.
//!
//! # What §120 found
//!
//! The Plotkin/Pretnar engine from §23 — 590 lines, 49 tests — had ZERO lexer
//! tokens. There was no grammar. It was invisible to every gate because it was
//! never ADVERTISED, so there was no badge to classify. `effect`, `handle` and
//! `perform` could not be written at all.
//!
//! The property under test is the one nothing else in the language expresses:
//! `study` performs an effect it knows nothing about, and an OUTER handler
//! decides what that effect means — without `study` being changed, recompiled,
//! or aware a handler exists.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::{dispatch_node, DispatchCtx};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::IRFlow;
use tokio::sync::mpsc;

const EXAMPLE: &str = "examples/algebraic_effects.axon";

/// Read the published example from the repo root (this crate lives one level in).
fn example_source() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("axon-rs has a parent directory")
        .join(EXAMPLE);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// SOURCE → type check → IR → the named flow.
fn flow_from_example(name: &str) -> IRFlow {
    let src = example_source();
    let tokens = axon_frontend::lexer::Lexer::new(&src, EXAMPLE)
        .tokenize()
        .expect("the published example must lex");
    let prog = axon_frontend::parser::Parser::new(tokens)
        .parse()
        .expect("the published example must parse");
    let errors = axon_frontend::type_checker::TypeChecker::new(&prog).check();
    assert!(
        errors.is_empty(),
        "the published example must TYPE-CHECK — an adopter reads it as correct: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    let ir = axon_frontend::ir_generator::IRGenerator::new().generate(&prog);
    assert!(
        !ir.effects.is_empty(),
        "the example declares `effect Delivery`; an empty catalog means the \
         declaration no longer reaches the IR"
    );
    ir.flows
        .into_iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no flow named {name} in the published example"))
}

#[tokio::test]
async fn the_published_example_runs_its_handler_over_a_performing_flow() {
    let flow = flow_from_example("stream_findings");

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx);

    for node in &flow.steps {
        dispatch_node(node, &mut ctx)
            .await
            .unwrap_or_else(|e| panic!("the published example must dispatch: {e:?}"));
    }

    // The handler's clause body is an ordinary flow body, so its step runs. If
    // the `in { … }` body were inert — the shape §120's D120.1 rejected — this
    // would be empty and the flow would still have "completed".
    let mut step_types = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let FlowExecutionEvent::StepStart { step_type, .. } = ev {
            step_types.push(step_type);
        }
    }
    assert!(
        !step_types.is_empty(),
        "the handled body must actually execute steps; got {step_types:?}"
    );
}
