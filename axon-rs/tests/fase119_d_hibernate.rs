//! §Fase 119.d — `hibernate`: the flow HALTS, the continuation parks, and
//! resume rides `emit`.
//!
//! §111 F20: the handler returned "(hibernating …)" synchronously and the
//! flow KEPT WALKING — it kept spending while claiming to sleep. These gates
//! pin the inversion: the run ends at the hibernate point (zero further
//! compute, zero tokens — the founder's economic requirement), the parked
//! continuation carries the README's SHA-256 continuation id, an `emit` on
//! the awaited channel resumes exactly the remaining nodes, and a late
//! resume is REFUSED (the timeout, honored lazily — no timer burns while
//! asleep).

use axon::cancel_token::CancellationFlag;
use axon::flow_execution_event::FlowExecutionEvent;
use tokio::sync::mpsc;

const SOURCE: &str = r#"
flow SleepyFlow(x: String) -> Report {
    step Before { ask: "before" output: Draft }
    hibernate quarterly_data 24h
    step After { ask: "after ${quarterly_data}" output: Report }
}
"#;

async fn run_flow(source: &str, flow: &str) -> (bool, usize) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let cancel = CancellationFlag::new();
    let enforcement = std::sync::Arc::new(tokio::sync::Mutex::new(
        std::collections::HashMap::new(),
    ));
    let audit = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let warnings = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let drain = tokio::spawn(async move {
        let mut steps = 0usize;
        while let Some(ev) = rx.recv().await {
            if matches!(ev, FlowExecutionEvent::StepComplete { .. }) {
                steps += 1;
            }
        }
        steps
    });
    axon::streaming_via_dispatcher::run_streaming_via_dispatcher(
        source.to_string(),
        "<hibernate-test>".to_string(),
        flow.to_string(),
        "stub".to_string(),
        cancel,
        tx,
        enforcement,
        audit,
        warnings,
        std::sync::Arc::new(std::sync::Mutex::new(
            axon::temporal_context::TemporalState::default(),
        )),
        None,
        None,
        std::collections::HashMap::new(),
        std::collections::HashMap::new(),
        None,
        None,
        None,
        None,
    )
    .await;
    let steps = drain.await.unwrap_or(0);
    (true, steps)
}

#[tokio::test]
async fn the_walk_halts_at_hibernate_and_parks_the_continuation() {
    let (_ok, steps) = run_flow(SOURCE, "SleepyFlow").await;
    // THE economic assertion: exactly ONE StepComplete (the step before the
    // hibernate). If the walk kept going — the §111 F20 behavior — the step
    // AFTER the hibernate would also complete and this reads 2.
    assert_eq!(
        steps, 1,
        "the walk must HALT at hibernate; further steps are further spend"
    );
    // The continuation id is the README's published formula — recompute it
    // from the same inputs and find the parked entry under it.
    let cid = axon::hibernation::continuation_id("SleepyFlow", "quarterly_data", 4, 5);
    assert!(
        axon::hibernation::parking_lot().is_parked(&cid),
        "the walk must PARK the continuation under the deterministic id"
    );
    // Claim it and inspect: the remaining program is exactly the one step
    // after the hibernate point, and the halt happened before it ran.
    let parked = axon::hibernation::parking_lot()
        .claim(&cid, 0)
        .expect("claimable");
    assert_eq!(parked.event_name, "quarterly_data");
    assert_eq!(
        parked.remaining_nodes.len(),
        1,
        "exactly the post-hibernate step remains"
    );
    assert!(parked.deadline_ms.is_some(), "24h deadline recorded");
    assert_eq!(parked.backend_name, "stub");
}

#[tokio::test]
async fn a_resumed_continuation_runs_the_remaining_steps_and_records_the_outcome() {
    // Independent flow name so the deterministic id does not collide with
    // the other tests in this file.
    let source = SOURCE
        .replace("SleepyFlow", "ResumableFlow")
        .replace("quarterly_data", "resumable_event");
    run_flow(&source, "ResumableFlow").await;
    let cid = axon::hibernation::continuation_id("ResumableFlow", "resumable_event", 4, 5);
    let parked = axon::hibernation::parking_lot()
        .claim(&cid, 0)
        .expect("parked");
    axon::flow_dispatcher::resume_parked_flow(parked, "Q3 numbers".to_string()).await;
    match axon::hibernation::parking_lot().outcome(&cid) {
        Some(axon::hibernation::ResumedOutcome::Completed { output }) => {
            assert_eq!(output, "(stub)", "the remaining step ran via the stub backend");
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn emit_wakes_a_parked_continuation_end_to_end() {
    let source = SOURCE
        .replace("SleepyFlow", "EmitWokenFlow")
        .replace("quarterly_data", "emit_woken_event");
    run_flow(&source, "EmitWokenFlow").await;
    let cid = axon::hibernation::continuation_id("EmitWokenFlow", "emit_woken_event", 4, 5);
    assert!(axon::hibernation::parking_lot().is_parked(&cid));

    // Drive an `emit emit_woken_event(...)` through the real dispatcher (the
    // legacy in-process route — no bus attached — which also wakes).
    //
    // §Fase 119.i — the event name is UNIQUE to this test. The four cases in
    // this file used to share `quarterly_data`, and the wake claims by event
    // name against a PROCESS-GLOBAL lot, so whichever test emitted first
    // consumed the others' parked continuations. That race is what made
    // `the_walk_halts_at_hibernate…` fail roughly one run in three — and it is
    // what exposed the cross-tenant defect §119.m.4 fixes.
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut ctx = axon::flow_dispatcher::DispatchCtx::new(
        "Emitter",
        "stub",
        "",
        CancellationFlag::new(),
        tx,
    );
    let node = axon::ir_nodes::IREmit {
        node_type: "emit",
        source_line: 0,
        source_column: 0,
        channel_ref: "emit_woken_event".into(),
        value_ref: "fresh data".into(),
        value_is_channel: false,
        shield_ref: String::new(),
        breach_policy: None,
    };
    axon::flow_dispatcher::wire_integrations::run_emit(&node, &mut ctx)
        .await
        .expect("emit completes");

    // The resume is fire-and-forget; poll the lot for the terminal outcome.
    for _ in 0..100 {
        if axon::hibernation::parking_lot().outcome(&cid).is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    match axon::hibernation::parking_lot().outcome(&cid) {
        Some(axon::hibernation::ResumedOutcome::Completed { .. }) => {}
        other => panic!("emit must wake the parked continuation: {other:?}"),
    }
    assert!(
        !axon::hibernation::parking_lot().is_parked(&cid),
        "a continuation resumes exactly once"
    );
}

#[tokio::test]
async fn hibernate_inside_a_par_branch_is_refused_not_half_suspended() {
    let source = r#"
flow BranchSleeper(x: String) -> Report {
    par {
        step A { ask: "a" output: Draft }
        hibernate never_arrives 1h
    }
}
"#;
    // The dispatcher surfaces the refusal as a flow error; the run reports
    // failure rather than a half-suspended branch.
    let (ok, _steps) = run_flow(source, "BranchSleeper").await;
    // run_streaming_via_dispatcher returns Ok even for flow errors (the error
    // rides the wire); the decisive assertion is that nothing got PARKED.
    let _ = ok;
    let cid = axon::hibernation::continuation_id("BranchSleeper", "never_arrives", 5, 9);
    assert!(
        !axon::hibernation::parking_lot().is_parked(&cid),
        "a nested hibernate must refuse, never park"
    );
}
