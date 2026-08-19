//! The `agent` primitive's README promises, made true on the paths an
//! adopter takes — and gated here.
//!
//! An outside reviewer installed 4.0.0 and measured five things the README
//! said the compiler or runtime did for agents and it did not: a missing
//! `max_iterations` passed `axon check`; a typo'd field was skipped in
//! silence; `strategy: custom` with steps parsed clean and was refused at
//! dispatch; `return:` was not in the AST; `max_time` was accepted and not
//! enforced; `run --tool-mode stub` never entered the loop. The compile-time
//! half is gated in `axon-frontend/tests/agent_checker_closes_its_promises.rs`
//! (axon-T1216 … T1220); this file gates the runtime half:
//!
//! - `max_time` is a bound the loop READS (`Spend::affords`), checked before
//!   every deliberation against the instant the run began;
//! - `return: T` over a declared struct type validates the final answer
//!   (`return_schema_defect`), and a non-conforming answer is a STUCK outcome
//!   routed through `on_stuck`, never a free-text success;
//! - the stub backend answers the agent PROTOCOL when there is a tool to
//!   demonstrate (`ACT:` once, then `ANSWER:`), so the loop is exercised —
//!   every parse, grant check, dispatch and bound check is the loop's own;
//! - `axon estimate` charges an agent call at its ceiling
//!   (`max_iterations × per-iteration`), not as one ask;
//! - `axon run --tool-mode stub` runs the loop for an agent written inside a
//!   step body — the README's own `step Research { MarketResearcher(sector) }`
//!   — instead of printing "Execute step: Research" and moving on.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::agent_loop::{return_schema_defect, run_agent};
use axon::flow_dispatcher::{DispatchCtx, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::{IRAgent, IRTypeField};
use std::process::Command;
use tokio::sync::mpsc;

fn agent(strategy: &str, tools: &[&str], max_iterations: Option<i64>, on_stuck: &str) -> IRAgent {
    IRAgent {
        node_type: "agent",
        source_line: 0,
        source_column: 0,
        name: "Researcher".into(),
        goal: "Produce a competitive analysis".into(),
        tools: tools.iter().map(|t| t.to_string()).collect(),
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
    (DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx), rx)
}

fn starts(rx: &mut mpsc::UnboundedReceiver<FlowExecutionEvent>) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let FlowExecutionEvent::StepStart { step_name, .. } = ev {
            out.push(step_name);
        }
    }
    out
}

fn field(name: &str, type_name: &str, optional: bool) -> IRTypeField {
    IRTypeField {
        node_type: "type_field",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        type_name: type_name.into(),
        generic_param: String::new(),
        optional,
    }
}

// ── max_time ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn max_time_is_a_bound_the_loop_reads() {
    // `0ms` has already elapsed when the first check runs: zero deliberations,
    // the stuck outcome names `max_time`. Deterministic — no clock races.
    let (mut c, mut rx) = ctx();
    let mut a = agent("react", &[], Some(50), "retry");
    a.max_time = "0ms".into();
    let out = run_agent(&a, "x", &mut c).await.expect("a timed-out run completes via on_stuck");
    assert_eq!(starts(&mut rx).len(), 0, "the bound is checked BEFORE the first spend");
    match out {
        NodeOutcome::Completed { output, .. } => assert!(
            output.contains("max_time"),
            "the diagnostic must name max_time, not max_iterations. Got: {output}"
        ),
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn max_time_short_of_elapsed_does_not_bite() {
    // The converse: a generous ceiling lets the iteration bound decide.
    let (mut c, mut rx) = ctx();
    let mut a = agent("react", &[], Some(3), "retry");
    a.max_time = "1h".into();
    let out = run_agent(&a, "x", &mut c).await.expect("completes");
    assert_eq!(starts(&mut rx).len(), 3);
    match out {
        NodeOutcome::Completed { output, .. } => {
            assert!(output.contains("max_iterations"), "got: {output}")
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn an_unreadable_max_time_is_refused_before_any_spend() {
    let (mut c, mut rx) = ctx();
    let mut a = agent("react", &[], Some(3), "retry");
    a.max_time = "soon".into();
    let err = run_agent(&a, "x", &mut c).await.expect_err("refused");
    assert!(format!("{err:?}").contains("not a duration"), "got: {err:?}");
    assert_eq!(starts(&mut rx).len(), 0);
}

// ── return: T ────────────────────────────────────────────────────────────────

#[test]
fn return_schema_defect_is_structural_and_total() {
    let schema = vec![
        field("total", "Integer", false),
        field("note", "String", true),
        field("items", "List", false),
    ];
    assert_eq!(
        return_schema_defect(&schema, r#"{"total": 3, "items": []}"#),
        None,
        "required present, optional absent, shapes right ⇒ inhabits"
    );
    assert_eq!(
        return_schema_defect(&schema, "```json\n{\"total\": 3, \"items\": [1]}\n```"),
        None,
        "one code fence is tolerated — models fence JSON"
    );
    assert!(
        return_schema_defect(&schema, r#"{"items": []}"#)
            .unwrap()
            .contains("`total` is missing"),
        "a missing required field is the defect named"
    );
    assert!(
        return_schema_defect(&schema, r#"{"total": "three", "items": []}"#)
            .unwrap()
            .contains("declared `Integer`"),
        "a wrong shape names the field and the declared type"
    );
    assert!(
        return_schema_defect(&schema, "Here is your report: it went well.")
            .unwrap()
            .contains("not a JSON object"),
        "prose is not an inhabitant"
    );
}

#[tokio::test]
async fn a_non_conforming_answer_is_stuck_on_return_never_a_typed_success() {
    // Tool-bearing, so the stub terminates the protocol with `ANSWER: (stub)`
    // — which is not a JSON object of type Report. The run is STUCK on the
    // `return` bound and `on_stuck: retry` hands back the partial, labelled.
    let (mut c, mut rx) = ctx();
    let mut a = agent("react", &["WebSearch"], Some(5), "retry");
    a.return_type = "Report".into();
    a.return_schema = vec![field("total", "Integer", false)];
    let out = run_agent(&a, "x", &mut c).await.expect("completes via on_stuck");
    let names = starts(&mut rx);
    assert_eq!(
        names.iter().filter(|n| n.ends_with(":react")).count(),
        2,
        "ACT, then ANSWER (the tool dispatch in between is its own step). Got: {names:?}"
    );
    match out {
        NodeOutcome::Completed { output, .. } => {
            assert!(output.contains("[retry]"), "routed through on_stuck. Got: {output}");
            assert!(
                output.contains("return: Report") && output.contains("not a JSON object"),
                "the partial is labelled with the return defect. Got: {output}"
            );
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn with_no_declared_schema_the_answer_is_free_text_as_before() {
    let (mut c, _rx) = ctx();
    let a = agent("react", &["WebSearch"], Some(5), "escalate");
    let out = run_agent(&a, "x", &mut c).await.expect("answers");
    match out {
        NodeOutcome::Completed { output, .. } => assert_eq!(output, "(stub)"),
        other => panic!("expected Completed, got {other:?}"),
    }
}

// ── the protocol-aware stub ──────────────────────────────────────────────────

#[tokio::test]
async fn the_stub_demonstrates_one_dispatch_and_the_loop_terminates_on_its_own_answer() {
    let (mut c, mut rx) = ctx();
    let a = agent("react", &["WebSearch", "PDFExtractor"], Some(10), "escalate");
    let out = run_agent(&a, "ev market", &mut c).await.expect("answers");
    let names = starts(&mut rx);
    // Two deliberations (ACT, ANSWER) plus the tool dispatch the ACT caused —
    // the dispatch is a step of its own in the wire events.
    assert!(
        names.iter().filter(|n| n.ends_with(":react")).count() == 2,
        "ACT then ANSWER: exactly two deliberations. Got: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("WebSearch")),
        "the FIRST granted tool was dispatched (the loop's grant check, not the stub's). Got: {names:?}"
    );
    match out {
        NodeOutcome::Completed { output, .. } => assert_eq!(output, "(stub)"),
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn a_tool_less_agent_is_still_ended_only_by_its_bound() {
    // The stub demonstrates nothing it cannot: no tool ⇒ `(stub)` ⇒ the bound.
    let (mut c, mut rx) = ctx();
    let out = run_agent(&agent("react", &[], Some(3), "retry"), "x", &mut c)
        .await
        .expect("completes");
    assert_eq!(starts(&mut rx).len(), 3);
    match out {
        NodeOutcome::Completed { output, .. } => assert!(output.contains("max_iterations")),
        other => panic!("expected Completed, got {other:?}"),
    }
}

// ── the estimator ────────────────────────────────────────────────────────────

const ESTIMATE_SOURCE: &str = r#"
tool WebSearch { provider: http timeout: 10s }

agent Researcher {
    goal: "find things"
    tools: [WebSearch]
    strategy: react
    max_iterations: 6
    on_stuck: retry
}

flow F(q: String) -> String {
    step Research {
        Researcher(q)
        ask: "Summarise ${q}"
    }
}
"#;

#[test]
fn estimate_charges_an_agent_at_its_ceiling_not_as_one_ask() {
    let (_p, ir) = axon::flow_plan::compile_source_to_ir(ESTIMATE_SOURCE, "est.axon").expect("compiles");
    let pricing = axon::cost_estimator::PricingModel::default_sonnet();
    let report = axon::cost_estimator::estimate_program(&ir, &pricing);
    // Floor: the Ask (800+400) + one MultiAgent call (2000+1000) — the agent
    // INSIDE the step body is counted, which it was not before.
    assert_eq!(report.total_tokens, 800 + 400 + 2000 + 1000, "floor counts the nested agent call");
    // Ceiling: five more iterations at the agent's per-iteration estimate.
    assert_eq!(
        report.ceiling_total_tokens,
        report.total_tokens + 5 * 3000,
        "ceiling = floor + (max_iterations − 1) × per-iteration"
    );
    assert!(report.ceiling_cost_usd > report.estimated_cost_usd);
    let text = axon::cost_estimator::format_text(&report);
    assert!(text.contains("Ceiling (agents at max_iterations)"), "the text output shows both: {text}");
}

// ── both doors — the attestation gate (Law 4: source in, dispatch out) ──────

fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/agent/react_agent_in_step.axon")
}

#[test]
fn the_unified_server_engine_runs_the_agent_written_inside_a_step() {
    let source = std::fs::read_to_string(fixture_path()).expect("fixture on disk");
    let (_p, ir) = axon::flow_plan::compile_source_to_ir(&source, "react_agent_in_step.axon")
        .expect("the fixture compiles clean");
    let metrics = axon::runner::execute_server_flow(
        &ir,
        "F",
        "stub",
        "",
        "react_agent_in_step.axon",
        None,
        Some(&serde_json::json!({"q": "2+2"})),
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("the run completes");
    assert!(metrics.success, "error: {:?}; steps: {:?}", metrics.error, metrics.step_names);
    assert!(
        metrics.step_names.iter().any(|n| n == "Calc:react"),
        "the agent's deliberations are steps of the run — the loop RAN on the server engine. Got: {:?}",
        metrics.step_names
    );
    assert!(
        metrics.step_names.iter().any(|n| n.contains("Calculator")),
        "…and the stub's ACT dispatched the granted tool through the loop's own grant check. Got: {:?}",
        metrics.step_names
    );
}

#[test]
fn run_tool_mode_stub_enters_the_loop_for_an_agent_written_inside_a_step() {
    let out = Command::new(env!("CARGO_BIN_EXE_axon"))
        .args(["run", fixture_path().to_str().unwrap(), "--backend", "stub"])
        .output()
        .expect("spawn axon");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "stdout: {stdout}
stderr: {stderr}");
    assert!(
        stdout.contains("agent Calc →"),
        "the stub walker must RUN the agent (and say so), not print the step and move on.
{stdout}"
    );
}
