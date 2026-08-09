//! §Fase 119.m.3 (runtime half) — the agent call DISPATCHES, and fails closed.
//!
//! The grammar half is `axon-frontend/tests/fase119_m3_agent_call_grammar.rs`.
//! This is the other end: the call resolves its declaration from
//! `DispatchCtx::agent_specs`, and REFUSES when it cannot.
//!
//! **Why the refusal is the load-bearing test.** `max_iterations` lives in the
//! declaration. An unresolved agent is therefore an UNBOUNDED one, and a
//! runtime that ran it anyway would spend against a ceiling nobody wrote — the
//! §111.f compute doctrine ("an empty catalog fails closed") applied to the one
//! construct in the language that spends in a loop.
//!
//! It also pins the seam §119.f.7 was about: a catalog the executor reads and
//! no entry point fills is an executor that never runs. `with_agents` is called
//! by `runner.rs` (the CLI/sync path), by `streaming_via_dispatcher` (the SSE
//! twin — the half `feedback_ask_where_production_calls_it` records as the one
//! everybody forgets) and by the §119.d resume path.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, DispatchError, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::{IRAgent, IRAgentCall, IRFlowNode};
use std::sync::Arc;
use tokio::sync::mpsc;

fn call(agent_name: &str, args: &[&str]) -> IRFlowNode {
    IRFlowNode::AgentCall(IRAgentCall {
        node_type: "agent_call",
        source_line: 0,
        source_column: 0,
        agent_name: agent_name.into(),
        arguments: args.iter().map(|a| a.to_string()).collect(),
    })
}

fn declared_agent(name: &str, max_iterations: Option<i64>) -> IRAgent {
    IRAgent {
        node_type: "agent",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        goal: "Produce a competitive analysis".into(),
        tools: Vec::new(),
        memory_ref: String::new(),
        strategy: "react".into(),
        on_stuck: "retry".into(),
        shield_ref: String::new(),
        max_iterations,
        max_tokens: None,
        max_cost: None,
        max_time: String::new(),
    }
}

fn ctx() -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx),
        rx,
    )
}

// ── §1 — fail closed ────────────────────────────────────────────────────────

#[tokio::test]
async fn m3d_1_an_undeclared_agent_is_refused_not_run() {
    let (mut c, mut rx) = ctx();
    let err = dispatch_node(&call("GhostAgent", &["x"]), &mut c)
        .await
        .expect_err("an unresolvable agent must be refused");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("GhostAgent"),
        "the refusal must name the agent. Got: {msg}"
    );
    assert!(
        msg.contains("max_iterations") || msg.contains("unbounded"),
        "and must say WHY it matters: the declaration is where the bound \
         lives. Got: {msg}"
    );
    let mut events = 0;
    while rx.try_recv().is_ok() {
        events += 1;
    }
    assert_eq!(events, 0, "nothing was spent before the refusal");
}

#[tokio::test]
async fn m3d_2_an_empty_catalog_fails_closed() {
    // The §111.f doctrine, stated as a test: a catalog nobody filled must run
    // nothing. If this ever passes by RUNNING, the seam has come unwired and
    // every agent in every program is silently unbounded.
    let (mut c, _rx) = ctx();
    assert!(c.agent_specs.is_empty(), "default catalog is empty");
    assert!(dispatch_node(&call("Anything", &[]), &mut c).await.is_err());
}

// ── §2 — a declared agent runs ─────────────────────────────────────────────

#[tokio::test]
async fn m3d_3_a_declared_agent_runs_and_binds_under_its_name() {
    let (mut c, mut rx) = ctx();
    c = c.with_agents(Arc::new(vec![declared_agent("Researcher", Some(2))]));

    let outcome = dispatch_node(&call("Researcher", &["ev market"]), &mut c)
        .await
        .expect("a declared agent runs");
    assert!(matches!(outcome, NodeOutcome::Completed { .. }));
    assert!(
        c.let_bindings.contains_key("Researcher"),
        "the answer binds under the agent's name"
    );

    let mut agent_steps = 0;
    while let Ok(ev) = rx.try_recv() {
        if let FlowExecutionEvent::StepStart { step_type, .. } = ev {
            if step_type == "agent" {
                agent_steps += 1;
            }
        }
    }
    assert_eq!(
        agent_steps, 2,
        "and the declared `max_iterations: 2` bounded the loop — this is the \
         number that proves the DECLARATION reached the executor, not a default"
    );
}

#[tokio::test]
async fn m3d_4_the_declarations_bound_is_what_binds_not_a_default() {
    // Same call, different declaration: the loop length must follow the
    // declaration. A hardcoded default would make both runs equal.
    for (bound, expected) in [(1i64, 1usize), (3, 3)] {
        let (mut c, mut rx) = ctx();
        c = c.with_agents(Arc::new(vec![declared_agent("R", Some(bound))]));
        dispatch_node(&call("R", &[]), &mut c).await.expect("runs");
        let mut n = 0;
        while let Ok(ev) = rx.try_recv() {
            if let FlowExecutionEvent::StepStart { step_type, .. } = ev {
                if step_type == "agent" {
                    n += 1;
                }
            }
        }
        assert_eq!(n, expected, "bound {bound} must produce {expected} iterations");
    }
}

// ── §3 — arguments carry VALUES ────────────────────────────────────────────

#[tokio::test]
async fn m3d_5_a_dotted_argument_is_resolved_to_its_value() {
    // `TrendAnalyzer(Gather.output)` must hand the agent the prior step's
    // VALUE. Passing the string `"Gather.output"` would be the §119.f.8 defect
    // (`reason`'s `given:`) repeated in the argument position.
    let (mut c, _rx) = ctx();
    c = c.with_agents(Arc::new(vec![declared_agent("Analyst", Some(1))]));
    c.let_bindings
        .insert("Gather.output".to_string(), "REVENUE: 4.2B".to_string());

    // The agent's own prompt is not observable through the stub, so the proof
    // is the binding the loop leaves behind plus the absence of a refusal —
    // the resolution itself is pinned by `exec_context::resolve_value_reference`
    // and by §119.f.8's suite. What THIS asserts is that the argument path is
    // wired to it at all.
    let out = dispatch_node(&call("Analyst", &["Gather.output"]), &mut c).await;
    assert!(out.is_ok(), "a dotted argument must dispatch: {out:?}");
}

// ── §4 — cancellation is honoured at entry (D3) ────────────────────────────

/// Found by §33.y.n's cancel-propagation fuzz on the first run: the handler
/// resolved the catalog BEFORE checking cancel, so a cancelled run reported a
/// missing agent instead of a cancellation — a wrong diagnosis for the operator
/// and a broken invariant for the dispatcher. Pinned here so it stays fixed.
#[tokio::test]
async fn m3d_6_cancel_is_checked_before_catalog_resolution() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationFlag::new();
    cancel.cancel();
    let mut c = DispatchCtx::new("F", "stub", "", cancel, tx);

    match dispatch_node(&call("GhostAgent", &[]), &mut c).await {
        Err(DispatchError::UpstreamCancelled) => {}
        other => panic!(
            "a pre-cancelled run must report UpstreamCancelled, not a catalog \
             miss. Got: {other:?}"
        ),
    }
}
