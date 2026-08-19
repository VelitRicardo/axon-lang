//! v2.15.0 follow-up — does a `persist into <store>` RESOLVE when `<store>`
//! also backs a v2.14.0 `corpus … from axonstore` (the kivi 2026-06-24 REOPENED gap)?
//!
//! The adopter's reopened report hypothesizes that a `persist into LtmSummaries`
//! aborts BECAUSE `LtmSummaries` is the `documents:` store of a
//! `corpus LtmGraph from axonstore { documents: LtmSummaries(id, summary) … }`
//! — i.e. that corpus-backing diverts the store into a special write path that
//! fails before the INSERT.
//!
//! This test reproduces that exact topology HERMETICALLY (no DB): a declared
//! postgresql `axonstore` that ALSO backs an adaptive `corpus from axonstore`,
//! with a non-streaming flow that `persist`s into it. The store's `connection`
//! is an UNSET `env:` var, so the persist's backend resolve fails
//! deterministically at `resolve_dsn` — the same CLASS of pre-INSERT failure
//! the adopter sees.
//!
//! The two outcomes discriminate the hypothesis:
//!   * If the persist RESOLVES to Postgres (then errors on the missing env, with
//!     `error: Some("…persist into 'LtmSummaries'…")`) → corpus-backing does NOT
//! break persist resolution; the store is in the registry; v2.15.0 names the
//!     failure. The adopter's `error: null` is then purely the ENTERPRISE
//!     envelope dropping `ServerRunnerMetrics.error` (the real gap).
//!   * If the persist falls to the in-memory KV no-op (`success: true`,
//!     `error: None`) → the corpus lowering dropped the store from
//!     `axonstore_specs` (a real bug to fix upstream).

use axon::runner::execute_server_flow;
use std::collections::HashMap;

const SOURCE: &str = r#"
axonstore LtmSummaries {
    backend: postgresql
    connection: "env:AXON_FASE65F2_ABSENT_DSN"
    schema {
        id:      Uuid primary_key
        summary: Text not_null
    }
}
axonstore LtmEdges {
    backend: postgresql
    connection: "env:AXON_FASE65F2_ABSENT_DSN"
    schema {
        from_id: Uuid
        to_id:   Uuid
        etype:   Text
        weight:  Float
    }
}

corpus LtmGraph from axonstore {
    documents: LtmSummaries( id, summary )
    relations: LtmEdges( from_id, to_id, etype, weight )
    adaptive: true
}

flow HibernateSession(summary_id: String) -> String {
    step Summarize { ask: "summarize" output: String }
    persist into LtmSummaries {
        id: "${summary_id}"
        summary: "${Summarize}"
    }
    return Summarize.output
}

axonendpoint HibernateEndpoint { public: true
    method: POST
    path: "/api/memory/hibernate"
    execute: HibernateSession
}
"#;

#[test]
fn persist_into_a_corpus_backed_store_still_resolves_and_names_its_failure() {
    std::env::remove_var("AXON_LEGACY_EXECUTOR");
    std::env::remove_var("AXON_FASE65F2_ABSENT_DSN");

    let (_program, ir) =
        axon::flow_plan::compile_source_to_ir(SOURCE, "f2.axon").expect("compile");

    // ── section 1 — the corpus-backing store IS still a top-level axonstore ─────
    // The v2.14.0 lowering adds an `IRCorpusStoreSource` to the corpus spec but
    // must NOT remove the store from `axonstore_specs` — otherwise persist
    // could never resolve it.
    assert!(
        ir.axonstore_specs.iter().any(|s| s.name == "LtmSummaries"),
        "section 1 REGRESSION: a store that backs a `corpus from axonstore` vanished \
         from axonstore_specs — persist could never resolve it. specs={:?}",
        ir.axonstore_specs.iter().map(|s| &s.name).collect::<Vec<_>>()
    );

    let body = serde_json::json!({ "summary_id": "v2.15.0-001" });
    let metrics = execute_server_flow(
        &ir,
        "HibernateSession",
        "stub",
        "", // v2.49.0 — tenant scope (empty = pre-fix behavior)
        "f2.axon",
        None,
        Some(&body),
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
    .expect("server runner Ok");

    // ── section 2 — the persist RESOLVED to Postgres and FAILED honestly ────────
    // (vs. an in-memory no-op that would report `success: true, error: None`).
    // A `Some` error naming the node proves: (a) corpus-backing did NOT divert
    // the store into a no-op path, and (b) v2.15.0's honesty fires in
    // collect_via_dispatcher. If THIS passes but the adopter still sees
    // `error: null`, the gap is the enterprise envelope dropping `metrics.error`.
    assert!(
        !metrics.success,
        "section 2: a persist whose backend can't resolve must fail the flow, not \
         silently succeed as an in-memory no-op (which would mean the corpus \
         store was missing from the registry). step_names={:?}",
        metrics.step_names
    );
    let detail = metrics.error.as_deref().unwrap_or_else(|| {
        panic!(
            "section 2: the corpus-backed persist failure was SWALLOWED in the runner \
             (error=None) — collect_via_dispatcher must name it. success={}, \
             step_names={:?}",
            metrics.success, metrics.step_names
        )
    });
    assert!(
        detail.contains("persist into 'LtmSummaries'"),
        "section 2: the honest detail names the failing corpus-backed persist node. \
         Got: {detail:?}"
    );
}
