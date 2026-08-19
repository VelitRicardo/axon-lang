//! §Fase 119.m.1 — **`agent` runs, and `strategy:` is a CONTROL policy.**
//!
//! **What was wrong.** `agent` was advertised in the README's badge wall,
//! type-checked against two closed catalogs (`VALID_AGENT_STRATEGIES`,
//! `VALID_ON_STUCK_POLICIES`), lowered to `IRAgent` with eleven fields — and
//! executed by NOTHING. Measured 2026-08-08: no reader of `ir.agents` outside
//! attestation and PCC; no reader of `strategy` / `max_iterations` /
//! `on_stuck` / `max_cost` anywhere in the dispatcher. `advertised.rs` had it
//! `Unaudited`, which is the worst of the states: nobody had looked.
//!
//! The README makes a load-bearing promise about it in plain words —
//! *"Budget cap of $5.00 prevents runaway API costs — the compiler guarantees
//! termination"* — and this suite is where that sentence is kept or refused.
//!
//! **D119.5, ratified 2026-08-08: `strategy:` is a CONTROL policy, not a
//! prompt flavour.** The three strategies are three different loops with three
//! different termination proofs, and §3 below is what makes that claim
//! falsifiable rather than decorative: if all three collapsed to one loop, the
//! step-count signatures would be identical and those tests would fail.
//!
//! Every bound is checked BEFORE the spend (the §114.a lesson: a budget
//! charged after the call bounds nothing), so the stub backend — which always
//! answers `"(stub)"` and therefore never terminates a `react` loop on its own
//! — is the ideal adversary: it drives every strategy into its bound.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::agent_loop::{
    granted_tool, parse_agent_move, run_agent, AgentBounds, AgentMove,
};
use axon::flow_dispatcher::{DispatchCtx, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::IRAgent;
use tokio::sync::mpsc;

fn agent(strategy: &str, max_iterations: Option<i64>, on_stuck: &str) -> IRAgent {
    IRAgent {
        node_type: "agent",
        source_line: 0,
        source_column: 0,
        name: "Researcher".into(),
        goal: "Produce a competitive analysis".into(),
        // Tool-less on purpose: the stub backend answers the agent protocol
        // when there is a tool to demonstrate (`ACT:` once, then `ANSWER:`),
        // and stays the unstructured `(stub)` when there is none — so a
        // tool-less agent is the one whose run only the BOUND can end, which
        // is the property m2.* and m5.* exercise. The tool-bearing shape has
        // its own suite (agent_protocol_stub.rs).
        tools: Vec::new(),
        memory_ref: String::new(),
        strategy: strategy.into(),
        on_stuck: on_stuck.into(),
        shield_ref: String::new(),
        max_iterations,
        max_tokens: None,
        max_cost: None,
        max_time: String::new(),
        return_type: String::new(),
        return_schema: Vec::new(),
        body: Vec::new(),
    }
}

fn ctx() -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx),
        rx,
    )
}

/// Every `step_start` the run emitted, in order — the observable trace of the
/// control loop's shape.
fn starts(rx: &mut mpsc::UnboundedReceiver<FlowExecutionEvent>) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let FlowExecutionEvent::StepStart {
            step_name,
            step_type,
            ..
        } = ev
        {
            assert_eq!(step_type, "agent", "agent iterations carry the agent slug");
            out.push(step_name);
        }
    }
    out
}

// ── §1 — the termination bound is REQUIRED, and refused before any spend ────

#[tokio::test]
async fn m1_1_an_agent_without_max_iterations_is_refused() {
    // There is no honest default. Any number this runtime invents is a
    // spending decision made on the adopter's behalf, and "unbounded" is that
    // decision at its worst.
    let (mut c, mut rx) = ctx();
    let err = run_agent(&agent("react", None, "escalate"), "electric vehicles", &mut c)
        .await
        .expect_err("an unbounded agent must be refused");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("max_iterations"),
        "the refusal must name the missing bound. Got: {msg}"
    );
    assert!(
        starts(&mut rx).is_empty(),
        "the refusal must land BEFORE the first token is spent"
    );
}

#[tokio::test]
async fn m1_2_a_zero_bound_is_refused_too() {
    let (mut c, _rx) = ctx();
    assert!(
        run_agent(&agent("react", Some(0), "escalate"), "x", &mut c)
            .await
            .is_err(),
        "`max_iterations: 0` is a declaration with no execution"
    );
}

#[tokio::test]
async fn m1_3_bounds_resolve_from_the_declaration() {
    let mut a = agent("react", Some(15), "forge");
    a.max_tokens = Some(50_000);
    a.max_cost = Some(2.50);
    let b = AgentBounds::resolve(&a).expect("resolves");
    assert_eq!(b.max_iterations, 15);
    assert_eq!(b.max_tokens, Some(50_000));
    assert_eq!(b.max_cost, Some(2.5));
}

// ── §2 — the loop actually terminates at the declared bound ─────────────────

/// The load-bearing test. The stub backend answers `"(stub)"` forever, which
/// is never a ReAct `ANSWER:` — so nothing but the bound can stop this loop.
/// If `max_iterations` were unread, this test would hang rather than fail,
/// which is why the assertion is on the exact iteration count and not merely
/// on "it returned".
#[tokio::test]
async fn m2_1_react_halts_at_exactly_max_iterations() {
    let (mut c, mut rx) = ctx();
    // `retry` returns the partial state instead of erroring, so the run's
    // shape is observable rather than swallowed by a refusal.
    let out = run_agent(&agent("react", Some(4), "retry"), "ev market", &mut c)
        .await
        .expect("bounded run completes");

    let names = starts(&mut rx);
    assert_eq!(
        names.len(),
        4,
        "the loop must run EXACTLY the declared number of iterations, no more \
         and no fewer. Got: {names:?}"
    );
    match out {
        NodeOutcome::Completed { output, .. } => assert!(
            output.contains("[retry]") && output.contains("max_iterations"),
            "the stuck outcome must name the bound that bit. Got: {output}"
        ),
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn m2_2_the_token_ceiling_bites_before_the_iteration_ceiling() {
    // Each stub deliberation emits 1 token, so a 2-token ceiling must stop a
    // 50-iteration agent at 2. This is what proves `max_tokens` is CONSUMED
    // and not merely carried.
    let (mut c, mut rx) = ctx();
    let mut a = agent("react", Some(50), "retry");
    a.max_tokens = Some(2);
    let out = run_agent(&a, "x", &mut c).await.expect("completes");

    assert_eq!(
        starts(&mut rx).len(),
        2,
        "the run must stop at the TOKEN ceiling, well short of 50 iterations"
    );
    match out {
        NodeOutcome::Completed { output, .. } => assert!(
            output.contains("max_tokens"),
            "the diagnostic must name max_tokens, not max_iterations. Got: {output}"
        ),
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn m2_3_the_cost_ceiling_is_priced_and_bites() {
    // §72's affine discipline applied to an agent run: the ceiling is checked
    // BEFORE each iteration, so it can never be exceeded — only reached.
    let (mut c, mut rx) = ctx();
    let mut a = agent("react", Some(50), "retry");
    // One stub token costs 15/1e6 USD at the OSS reference output price. The
    // ceiling is set STRICTLY BETWEEN one and two tokens' worth (15e-6 < 25e-6
    // < 30e-6) rather than exactly on a token boundary: an equality test
    // against a float the implementation computes a different way is a test of
    // IEEE-754 rounding, not of the budget.
    a.max_cost = Some(25.0 / 1_000_000.0);
    let out = run_agent(&a, "x", &mut c).await.expect("completes");

    assert_eq!(starts(&mut rx).len(), 2, "stopped at the cost ceiling");
    match out {
        NodeOutcome::Completed { output, .. } => {
            assert!(output.contains("max_cost"), "got: {output}")
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

// ── §3 — the three strategies are three DIFFERENT loops ────────────────────

/// D119.5 made falsifiable. If `strategy:` were a prompt flavour, all three
/// would emit the same trace under the same bound. They do not: each has a
/// distinct step-name signature, because each is a distinct control policy.
#[tokio::test]
async fn m3_1_each_strategy_has_its_own_observable_control_shape() {
    // react — every iteration is one `:react` deliberation.
    let (mut c, mut rx) = ctx();
    run_agent(&agent("react", Some(3), "retry"), "x", &mut c)
        .await
        .expect("ok");
    let react = starts(&mut rx);
    assert!(
        react.iter().all(|n| n.ends_with(":react")),
        "react is a single interleaved loop. Got: {react:?}"
    );

    // plan_and_execute — ONE plan, then executions. The plan is never
    // regenerated; that is this strategy's termination proof.
    let (mut c, mut rx) = ctx();
    run_agent(&agent("plan_and_execute", Some(3), "retry"), "x", &mut c)
        .await
        .expect("ok");
    let pae = starts(&mut rx);
    assert_eq!(
        pae.iter().filter(|n| n.ends_with(":plan")).count(),
        1,
        "the plan is produced EXACTLY once — a strategy that re-plans has no \
         finite list to walk. Got: {pae:?}"
    );
    assert!(
        pae.iter().any(|n| n.ends_with(":execute")),
        "and then it executes the plan. Got: {pae:?}"
    );

    // reflexion — attempt, then critique/revise cycles.
    let (mut c, mut rx) = ctx();
    run_agent(&agent("reflexion", Some(4), "retry"), "x", &mut c)
        .await
        .expect("ok");
    let refl = starts(&mut rx);
    assert_eq!(
        refl.iter().filter(|n| n.ends_with(":attempt")).count(),
        1,
        "one first attempt. Got: {refl:?}"
    );
    assert!(
        refl.iter().any(|n| n.ends_with(":critique")),
        "reflexion's defining move is the SELF-CRITIQUE — without it this is \
         just react under another name. Got: {refl:?}"
    );

    // And the three traces are pairwise different, which is the claim.
    assert_ne!(react, pae);
    assert_ne!(react, refl);
    assert_ne!(pae, refl);
}

// ── §4 — containment: an agent may only reach the tools it was granted ─────

/// The one place where model output is untrusted input. A tool named outside
/// the agent's declared `tools:` must NOT be dispatched — an agent's tool
/// grant is an attenuation, and an attenuation that widens at dispatch is not
/// an attenuation.
///
/// Asserted against the PURE predicate rather than through the loop, on
/// purpose: the stub backend never emits an `ACT:`, so a test that drove the
/// loop would assert containment it never exercised — green because nothing
/// happened. That is the shape of gate this fase spent three sub-fases
/// removing, and writing another one here would be indefensible.
#[test]
fn m4_1_containment_the_grant_is_an_attenuation() {
    let mut a = agent("react", Some(5), "retry");
    a.tools = vec!["WebSearch".into(), "PDFExtractor".into()];

    assert!(
        granted_tool(&a, "WebSearch").is_some(),
        "a granted tool resolves"
    );
    assert!(
        granted_tool(&a, "websearch").is_some(),
        "the match is case-insensitive — a model's casing is not a security boundary"
    );
    assert!(
        granted_tool(&a, "ShellExec").is_none(),
        "a tool OUTSIDE the declared grant must not resolve, however the model \
         spelled it"
    );
    assert!(
        granted_tool(&a, "").is_none(),
        "an empty name resolves to nothing"
    );

    // And the grant is checked against the DECLARATION, not the global
    // registry: an agent that declares no tools can reach none, even though
    // the process may have many registered.
    let mut none = agent("react", Some(5), "retry");
    none.tools = Vec::new();
    assert!(granted_tool(&none, "WebSearch").is_none());
}

/// The protocol parse is where untrusted output becomes a control decision.
#[test]
fn m4_2_an_unstructured_reply_is_a_violation_never_an_answer() {
    assert_eq!(
        parse_agent_move("ANSWER: 42"),
        AgentMove::Answer("42".into())
    );
    assert_eq!(
        parse_agent_move("act: WebSearch"),
        AgentMove::Act("WebSearch".into()),
        "prefix matching is case-insensitive"
    );
    // The load-bearing one: loose prose must NOT be read as an answer.
    // Accepting it would silently substitute "this is probably the answer" for
    // "the agent completed its protocol", and the caller cannot tell the two
    // apart.
    assert_eq!(
        parse_agent_move("Sure! Here is what I found about the EV market."),
        AgentMove::Unstructured
    );
    assert_eq!(parse_agent_move("(stub)"), AgentMove::Unstructured);
    // An `ACT:` with no tool name is not an act.
    assert_eq!(parse_agent_move("ACT:"), AgentMove::Unstructured);
}

// ── §5 — on_stuck: every catalog member decides, or refuses by name ────────

#[tokio::test]
async fn m5_1_escalate_raises_agent_stuck_error_with_context() {
    // README §agent, verbatim: escalate "raises `AgentStuckError` with full
    // context, so the human reviews exactly where the agent got stuck".
    let (mut c, _rx) = ctx();
    let err = run_agent(&agent("react", Some(2), "escalate"), "x", &mut c)
        .await
        .expect_err("escalate must raise");
    let msg = format!("{err:?}");
    for needle in ["AgentStuckError", "max_iterations", "2 iteration"] {
        assert!(msg.contains(needle), "missing `{needle}` in: {msg}");
    }
}

#[tokio::test]
async fn m5_2_an_undeclared_policy_fails_closed() {
    // Silence must not buy a partial answer that reads as a finished one.
    let (mut c, _rx) = ctx();
    let err = run_agent(&agent("react", Some(2), ""), "x", &mut c)
        .await
        .expect_err("no declared policy must fail closed");
    assert!(format!("{err:?}").contains("AgentStuckError"));
}

#[tokio::test]
async fn m5_3_hibernate_is_refused_by_name_not_silently_escalated() {
    // It is in the frontend's catalog and genuinely not implementable here:
    // §119.d parks a flow walk's remaining nodes, and an agent loop has no
    // such list. Refusing by name is the difference between a known gap and a
    // silent substitution.
    let (mut c, _rx) = ctx();
    let err = run_agent(&agent("react", Some(2), "hibernate"), "x", &mut c)
        .await
        .expect_err("hibernate must be refused");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("hibernate") && msg.contains("continuation"),
        "the refusal must say WHY, not just that. Got: {msg}"
    );
}

#[tokio::test]
async fn m5_4_forge_recovery_runs_the_real_forge() {
    // §86's forge, seeded with the stuck context — not a placeholder string.
    let (mut c, mut rx) = ctx();
    let out = run_agent(&agent("react", Some(2), "forge"), "x", &mut c)
        .await
        .expect("forge recovery completes");
    assert!(matches!(out, NodeOutcome::Completed { .. }));
    // The forge handler emits its own wire events with the `forge` slug, so
    // `starts` (which asserts the agent slug) must not be used here; drain
    // and look for the forge step instead.
    let mut saw_forge = false;
    while let Ok(ev) = rx.try_recv() {
        if let FlowExecutionEvent::StepStart { step_type, .. } = ev {
            if step_type == "forge" {
                saw_forge = true;
            }
        }
    }
    assert!(
        saw_forge,
        "`on_stuck: forge` must dispatch the REAL §86 forge, not a message \
         that says it did"
    );
}

// ── §6 — custom: the step sequence inside the agent block IS the policy ────

fn agent_step(name: &str, ask: &str) -> axon::ir_nodes::IRFlowNode {
    axon::ir_nodes::IRFlowNode::Step(axon::ir_nodes::IRStep {
        node_type: "step",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        persona_ref: String::new(),
        given: String::new(),
        ask: ask.into(),
        use_tool: None,
        probe: None,
        reason: None,
        weave: None,
        output_type: String::new(),
        confidence_floor: None,
        navigate_ref: String::new(),
        apply_ref: String::new(),
        requires_context: None,
        now_tz: None,
        guards: Vec::new(),
        pix_ops: Vec::new(),
        stream: None,
        performs: Vec::new(),
        body: Vec::new(),
    })
}

/// Like [`starts`], but for custom-agent steps — which go through the
/// ordinary step handler and therefore carry the `step` slug, not `agent`.
fn step_starts(rx: &mut mpsc::UnboundedReceiver<FlowExecutionEvent>) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let FlowExecutionEvent::StepStart { step_name, .. } = ev {
            out.push(step_name);
        }
    }
    out
}

#[tokio::test]
async fn m6_1_custom_with_no_body_is_refused_naming_the_rule() {
    // An EMPTY custom policy has nothing to run. The checker refuses it as
    // axon-T1217; the loop refuses it too (IR that bypassed `axon check`).
    let (mut c, _rx) = ctx();
    let err = run_agent(&agent("custom", Some(5), "escalate"), "x", &mut c)
        .await
        .expect_err("an empty custom body must be refused");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("T1217") && msg.contains("step"),
        "the refusal must name the rule and the missing step sequence. Got: {msg}"
    );
}

#[tokio::test]
async fn m6_2_custom_walks_its_own_steps_once_each_through_the_step_handler() {
    // README: "the agent follows a user-defined step sequence (Greet →
    // Configure → Train), not a generic loop". Each step is ONE deliberation
    // through the ordinary step handler, so it shows up as a StepStart under
    // the step's own name, and `max_iterations` bounds the count.
    let (mut c, mut rx) = ctx();
    let mut a = agent("custom", Some(5), "escalate");
    a.body = vec![
        agent_step("Greet", "Welcome the user"),
        agent_step("Configure", "Recommend a configuration"),
        agent_step("Train", "Generate a tutorial"),
    ];
    let out = run_agent(&a, "acme industries", &mut c)
        .await
        .expect("a bodied custom agent runs");
    let names = step_starts(&mut rx);
    assert_eq!(
        names,
        vec!["Greet", "Configure", "Train"],
        "the policy is the written sequence, in order, once. Got: {names:?}"
    );
    match out {
        NodeOutcome::Completed { output, .. } => {
            assert_eq!(output, "(stub)", "the result is the last step's output");
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    assert_eq!(
        c.let_bindings.get("Researcher").map(String::as_str),
        Some("(stub)"),
        "bound under the agent's name like every other strategy"
    );
}

#[tokio::test]
async fn m6_3_custom_is_bounded_by_max_iterations_like_every_strategy() {
    let (mut c, mut rx) = ctx();
    let mut a = agent("custom", Some(2), "retry");
    a.body = vec![
        agent_step("Greet", "a"),
        agent_step("Configure", "b"),
        agent_step("Train", "c"),
    ];
    let out = run_agent(&a, "x", &mut c).await.expect("bounded run completes");
    assert_eq!(step_starts(&mut rx).len(), 2, "two of three steps, then the bound bites");
    match out {
        NodeOutcome::Completed { output, .. } => assert!(
            output.contains("[retry]") && output.contains("max_iterations"),
            "the stuck outcome names the bound. Got: {output}"
        ),
        other => panic!("expected Completed, got {other:?}"),
    }
}

// ── §7 — the agent binds its result under its own name ────────────────────

#[tokio::test]
async fn m7_1_the_result_is_bound_under_the_agent_name() {
    let (mut c, _rx) = ctx();
    run_agent(&agent("react", Some(2), "retry"), "x", &mut c)
        .await
        .expect("ok");
    assert!(
        c.let_bindings.contains_key("Researcher"),
        "the binding discipline every other handler follows. Bindings: {:?}",
        c.let_bindings.keys().collect::<Vec<_>>()
    );
}
