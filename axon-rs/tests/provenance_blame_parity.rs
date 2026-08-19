//! v2.15.0 — observability parity gate: the unified dispatcher engine
//! emits the SAME `provenance_events` + `blame_attribution` + `anchor_breaches`
//! the legacy executor did.
//!
//! The v2.15.0 cutover made the dispatcher the DEFAULT non-streaming server
//! engine, but left three envelope fields empty (a documented regression with a
//! kill-switch). v2.15.0 closes that gap:
//!
//!   - `provenance_events` — a PURE IR walk (execution_units → closed-catalog
//!     slugs), execution-independent → byte-identical across engines.
//!   - `anchor_breaches` — projected from the dispatcher's per-step audit
//!     records (`StepAuditRecord.anchor_breaches`).
//!   - `blame_attribution` — the same AnchorBreach attribution shape the legacy
//!     `derive_blame_from_report` produced.
//!
//! This gate drives ONE flow through BOTH engines (toggled by the
//! `AXON_LEGACY_EXECUTOR` kill-switch) under the in-tree `stub` backend. The
//! flow declares a `RequiresCitation` anchor (name-keyed; breaches the stub
//! output `(stub)` deterministically — no brackets/DOI/URL) so the blame
//! assertion is NON-trivial, and a `remember`/`recall` pair so
//! `provenance_events` is non-empty.
//!
//! ## What parity means here (an HONEST asymmetry)
//!
//! `provenance_events` is byte-IDENTICAL across engines (pure IR walk). But
//! `anchor_breaches`/`blame` are NOT strictly equal on the stub backend, and
//! that is the dispatcher being MORE faithful, not regressed: the legacy stub
//! path never evaluated anchors (reports 0 breaches / no blame), whereas the
//! unified dispatcher evaluates them on EVERY backend (v2.15.0). Retiring the
//! legacy executor therefore LOSES no observability and GAINS correct anchor
//! evaluation on stub. We assert the dispatcher surfaces the breach the legacy
//! dropped (`dispatcher >= legacy`) rather than encoding the legacy deficiency
//! as a strict-equality contract.
//!
//! Serial by construction (mutates a process-global env var); this file holds a
//! single test so there is no intra-binary race.

use axon::runner::{execute_server_flow, ServerRunnerMetrics};
use std::collections::HashMap;

const SOURCE: &str = r#"
anchor RequiresCitation {
    description: "Every claim must carry a citation"
}

flow DueDiligence() -> Unit {
    step CollectKyc { ask: "Collect KYC identity attributes" output: Stream<Token> }
    let owner = "Acme Holdings Ltd"
    remember owner in cdd_case
    recall owner from cdd_case
    step Finalize { ask: "Finalize CDD profile" output: Stream<Token> }
}

axonendpoint DueDiligenceEndpoint { public: true
    method: POST
    path: "/test/due-diligence"
    execute: DueDiligence
    transport: sse
}
"#;

fn run() -> ServerRunnerMetrics {
    let (_program, ir) = axon::flow_plan::compile_source_to_ir(SOURCE, "e3.axon")
        .expect("compile");
    execute_server_flow(
        &ir,
        "DueDiligence",
        "stub",
        "", // v2.49.0 — tenant scope (empty = pre-fix behavior)
        "e3.axon",
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
    .expect("run")
}

#[test]
fn dispatcher_engine_emits_provenance_blame_anchors_at_legacy_parity() {
    // ── Engine A: the unified dispatcher (the post-v2.15.0 DEFAULT). ──
    std::env::remove_var("AXON_LEGACY_EXECUTOR");
    let dispatcher = run();

    // ── Engine B: the legacy executor (kill-switch ON). ──
    std::env::set_var("AXON_LEGACY_EXECUTOR", "1");
    let legacy = run();
    std::env::remove_var("AXON_LEGACY_EXECUTOR");

    // ── section 1 — provenance is actually POPULATED (not trivially equal-empty). ──
    assert!(
        dispatcher
            .provenance_events
            .iter()
            .any(|e| e.starts_with("memory:remember")),
        "the dispatcher must emit the remember provenance slug, got {:?}",
        dispatcher.provenance_events
    );
    assert!(
        dispatcher
            .provenance_events
            .iter()
            .any(|e| e.starts_with("memory:recall")),
        "the dispatcher must emit the recall provenance slug, got {:?}",
        dispatcher.provenance_events
    );

    // ── section 2 — the anchor actually BREACHED (non-trivial blame). ──
    assert!(
        dispatcher.anchor_breaches >= 1,
        "RequiresCitation must breach the stub output on the dispatcher path, \
         got {}",
        dispatcher.anchor_breaches
    );
    let blame = dispatcher
        .blame_attribution
        .as_ref()
        .expect("an anchor breach must surface blame on the dispatcher path");
    assert_eq!(
        blame.kind,
        axon::wire_envelope::BlameKind::AnchorBreach,
        "the breach must attribute as AnchorBreach"
    );

    // ── section 3 — provenance is byte-IDENTICAL across engines (pure IR walk). ──
    assert_eq!(
        dispatcher.provenance_events, legacy.provenance_events,
        "provenance_events must be byte-identical across engines"
    );

    // ── section 4 — the dispatcher surfaces AT LEAST the breaches the legacy did. ──
    // On the stub backend the legacy path evaluates NO anchors (reports 0) —
    // the dispatcher catches the breach the legacy silently dropped. Retiring
    // the legacy loses nothing; strict equality would encode the legacy bug.
    assert!(
        dispatcher.anchor_breaches >= legacy.anchor_breaches,
        "the unified dispatcher must surface at least the breaches the legacy \
         did (dispatcher={}, legacy={})",
        dispatcher.anchor_breaches,
        legacy.anchor_breaches
    );
}
