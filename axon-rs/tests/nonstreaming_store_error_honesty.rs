//! v2.15.0 — the non-streaming dispatcher surfaces a store-write failure
//! HONESTLY (named node + cause + log + wire slot), closing the v2.15.0
//! cutover regression.
//!
//! ## The regression this locks
//!
//! The v2.15.0 cutover made the unified dispatcher the DEFAULT non-streaming
//! server engine. But its driver (`collect_via_dispatcher`) handled a node's
//! `DispatchError` with `Err(_) => { success = false; break; }` — SWALLOWED:
//! no `tracing::error!`, no named node, no wire detail. So a `persist` that
//! fails ANY pre-insert gate (a v1.30.0 confidence-floor breach, a registry
//! resolve error, a connection failure — all BEFORE any SQL reaches the DB)
//! presented to the adopter as a SILENT abort: `success:false`, an EMPTY step
//! result for the persist, and ZERO diagnostic. The streaming dispatcher never
//! had this gap — it emits a `FlowError` naming the failing node + the cause
//! (`streaming_via_dispatcher`, v1.32.0/D6). This was the kivi-enterprise
//! 2026-06-24 gap report: a non-streaming `persist into LtmSummaries` aborted
//! with 0 SQL and no error anywhere.
//!
//! ## What this test proves (hermetic — no live Postgres)
//!
//! A `persist` into a postgresql-backed `axonstore` whose `connection` is an
//! UNSET `env:` var fails at registry resolve (`StoreError::MissingEnvVar`) —
//! a deterministic PRE-INSERT failure, the same CLASS as the adopter's
//! (whatever the real gate, the query never reaches the DB). Post-v2.15.0 the
//! non-streaming `ServerRunnerMetrics` carries `error: Some("flow '…' failed
//! at persist into 'LtmSummaries': …")` — the failing node is NAMED and the
//! cause is surfaced. Pre-v2.15.0 this field did not exist and the abort was
//! silent. The test also asserts the flow short-circuits AT the persist (the
//! trailing step never runs), matching the adopter's `steps_executed`
//! truncation.

use axon::runner::execute_server_flow;
use std::collections::HashMap;

/// A postgresql store bound to a GUARANTEED-absent env var, so the persist
/// fails at resolve (`MissingEnvVar`) with no network + no DB — the hermetic
/// stand-in for any pre-insert gate (floor / resolve / connection).
const SOURCE: &str = r#"
axonstore LtmSummaries {
    backend: postgresql
    connection: "env:AXON_FASE65F_ABSENT_DSN"
}

flow HibernateSession() -> String {
    step Summarize { ask: "Summarize the conversation" output: String }
    persist into LtmSummaries {
        summary: "${Summarize}"
    }
    step AfterPersist { ask: "This must NEVER run — the persist aborts first" output: String }
    return Summarize.output
}

axonendpoint HibernateEndpoint { public: true
    method: POST
    path: "/api/memory/hibernate"
    execute: HibernateSession
}
"#;

#[test]
fn nonstreaming_persist_failure_is_surfaced_not_swallowed() {
    // The unified dispatcher is the DEFAULT non-streaming engine; make sure the
    // kill-switch is off so we exercise it (not the legacy executor).
    std::env::remove_var("AXON_LEGACY_EXECUTOR");
    // The store's connection points HERE; keep it unset so resolve fails
    // deterministically — a hermetic pre-insert failure, no DB required.
    std::env::remove_var("AXON_FASE65F_ABSENT_DSN");

    let (_program, ir) =
        axon::flow_plan::compile_source_to_ir(SOURCE, "f_2.axon").expect("compile");

    let metrics = execute_server_flow(
        &ir,
        "HibernateSession",
        "stub",
        "", // v2.49.0 — tenant scope (empty = pre-fix behavior)
        "f_2.axon",
        None,
        None,
        &HashMap::new(),
        &HashMap::new(),
        None,
        None, // v1.18.0 — llm_base_url
        None, // v1.18.0 — llm_chat_path
            None, // v2.28.0 — budget (test: unbudgeted)
            None, // v2.69.0 — channel semaphores (test: none)
            None, // v2.69.0 — tool leases (test: none)
        None, // v2.31.0 — event_outbox (test: in-process emit)
        None, // v2.46.0 — credential minter (test: none)
        None, // v2.48.0 — secret custody (test: none)
        None, // v2.63.0 dataspace_engine (tests: fail closed)
        None, // v2.56.0 scrape_overrides
)
    .expect("the server runner returns Ok (a flow-level failure is reported in the metrics, not as an Err)");

    // ── section 1 — the failure is REPORTED, not swallowed ──────────────────────
    assert!(
        !metrics.success,
        "section 1: a failed persist must mark the flow unsuccessful"
    );
    let detail = metrics.error.as_deref().unwrap_or_else(|| {
        panic!(
            "section 1 REGRESSION: the non-streaming persist failure was SWALLOWED — \
             `ServerRunnerMetrics.error` is None. This is exactly the v2.15.0 \
             silent-abort the kivi gap report hit. metrics: success={}, \
             steps={:?}",
            metrics.success, metrics.step_names
        )
    });

    // ── section 2 — the diagnostic NAMES the failing node (parity with streaming) ─
    assert!(
        detail.contains("persist into 'LtmSummaries'"),
        "section 2: the honest detail must name the failing node like the streaming \
         dispatcher's FlowError does (v1.32.0/D6). Got: {detail:?}"
    );
    assert!(
        detail.contains("HibernateSession"),
        "section 2: the detail names the flow. Got: {detail:?}"
    );

    // ── section 3 — the flow SHORT-CIRCUITS at the persist ──────────────────────
    // The trailing `AfterPersist` step must never execute — matching the
    // adopter's `steps_executed` truncation (their abort stopped at the
    // persist; ClassifyEdges/return never ran).
    assert!(
        !metrics.step_names.iter().any(|n| n == "AfterPersist"),
        "section 3: the step AFTER the failing persist must not run — the flow aborts \
         at the persist. step_names={:?}",
        metrics.step_names
    );
}

/// The clean path stays byte-identical: a flow with NO store op (or a
/// succeeding one) carries `error: None`. Guards against a false-positive
/// where v2.15.0 leaks a spurious error onto every non-streaming response.
#[test]
fn nonstreaming_clean_flow_carries_no_error() {
    std::env::remove_var("AXON_LEGACY_EXECUTOR");

    const CLEAN: &str = r#"
flow Greet() -> String {
    step Hello { ask: "say hi" output: String }
    return Hello.output
}

axonendpoint GreetEndpoint { public: true
    method: POST
    path: "/greet"
    execute: Greet
}
"#;

    let (_program, ir) =
        axon::flow_plan::compile_source_to_ir(CLEAN, "clean_2.axon").expect("compile");

    let metrics = execute_server_flow(
        &ir,
        "Greet",
        "stub",
        "", // v2.49.0 — tenant scope (empty = pre-fix behavior)
        "clean_2.axon",
        None,
        None,
        &HashMap::new(),
        &HashMap::new(),
        None,
        None, // v1.18.0 — llm_base_url
        None, // v1.18.0 — llm_chat_path
            None, // v2.28.0 — budget (test: unbudgeted)
            None, // v2.69.0 — channel semaphores (test: none)
            None, // v2.69.0 — tool leases (test: none)
        None, // v2.31.0 — event_outbox (test: in-process emit)
        None, // v2.46.0 — credential minter (test: none)
        None, // v2.48.0 — secret custody (test: none)
        None, // v2.63.0 dataspace_engine (tests: fail closed)
        None, // v2.56.0 scrape_overrides
)
    .expect("run");

    assert!(metrics.success, "clean flow succeeds");
    assert!(
        metrics.error.is_none(),
        "the v2.15.0 error slot is None on the clean path (no spurious error). \
         Got: {:?}",
        metrics.error
    );
}
