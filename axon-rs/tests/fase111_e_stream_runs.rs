//! §Fase 111.e — the `stream` block's body EXECUTES.
//!
//! The frontend half (`axon-frontend/tests/fase111_e_stream_body.rs`) proves the
//! body now survives parsing and reaches the IR. This proves the runtime runs it.
//!
//! Before §111.e, `run_stream` emitted `StepStart` + `StepComplete` and returned
//! an empty string — and its comment was honest about why: *"No body to dispatch
//! — IRStreamBlock is payload-free."* The cause was upstream: `parse_block_step`
//! (`stream` / `deliberate` / `consensus` / `transact`) calls
//! `skip_braced_block()`, so the body never reached the AST. **One function, four
//! advertised primitives.**

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::*;
use tokio::sync::mpsc;

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

fn stream_node(body: Vec<IRFlowNode>) -> IRFlowNode {
    IRFlowNode::Stream(IRStreamBlock {
        node_type: "stream",
        source_line: 0,
        source_column: 0,
        body,
        // §Fase 119.n — this file pins §111.e's `body` form, which stays real.
        // The PUBLISHED surface (`stream<T> { on_chunk … on_complete … }`) is
        // gated separately, from SOURCE, in `fase119_n_stream_dispatch.rs`:
        // this harness builds IR by hand, so it proves the engine and says
        // nothing about whether any published program reaches it — the law-4
        // distinction that made `stream`'s old `Real` attestation worthless.
        chunk_type: String::new(),
        on_chunk: None,
        on_complete: None,
        on_error: None,
    })
}

fn ctx() -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        DispatchCtx::new("SFlow", "stub", "", CancellationFlag::new(), tx),
        rx,
    )
}

/// The line that would have failed for every release since v1.25.0.
#[tokio::test]
async fn the_stream_body_executes() {
    let (mut c, _rx) = ctx();

    dispatch_node(
        &stream_node(vec![let_node("a", "one"), let_node("b", "two")]),
        &mut c,
    )
    .await
    .expect("stream must dispatch");

    assert_eq!(
        c.let_bindings.get("a").map(String::as_str),
        Some("one"),
        "steps inside `stream` were discarded at parse time and never ran — they must execute"
    );
    assert_eq!(c.let_bindings.get("b").map(String::as_str), Some("two"));
}

/// An empty `stream {}` still completes cleanly — the legacy shape (and any
/// legacy IR, which deserialises to an empty body) behaves exactly as before, so
/// no adopter's program changes meaning under them without a recompile.
#[tokio::test]
async fn an_empty_stream_block_is_still_a_clean_no_op() {
    let (mut c, _rx) = ctx();
    let outcome = dispatch_node(&stream_node(vec![]), &mut c)
        .await
        .expect("an empty stream must complete");
    match outcome {
        NodeOutcome::Completed { output, tokens_emitted, .. } => {
            assert!(output.is_empty());
            assert_eq!(tokens_emitted, 0);
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

/// The block's value is its body's — the frame does not swallow what the body
/// produced.
///
/// NOTE on wire events: a `let` is a *structural* binding and correctly emits no
/// `StepStart` of its own, so this asserts the property that actually holds —
/// the stream frame announces itself, and the body's result propagates out
/// through it rather than being discarded (which is precisely what the old
/// handler did: it returned `String::new()` no matter what).
#[tokio::test]
async fn the_stream_frame_announces_itself_and_propagates_the_bodys_result() {
    let (mut c, mut rx) = ctx();

    let outcome = dispatch_node(
        &stream_node(vec![let_node("a", "one"), let_node("b", "two")]),
        &mut c,
    )
    .await
    .expect("stream must dispatch");
    drop(c);

    match outcome {
        NodeOutcome::Completed { output, .. } => assert_eq!(
            output, "two",
            "the block's value must be its body's last result — the old handler returned an \
             empty string no matter what the body did (it had no body to do anything with)"
        ),
        other => panic!("expected Completed, got {other:?}"),
    }

    let mut starts: Vec<String> = Vec::new();
    while let Some(ev) = rx.recv().await {
        if let FlowExecutionEvent::StepStart { step_type, .. } = ev {
            starts.push(step_type);
        }
    }
    assert!(
        starts.contains(&"stream".to_string()),
        "the stream frame must announce itself on the wire; got {starts:?}"
    );
}
