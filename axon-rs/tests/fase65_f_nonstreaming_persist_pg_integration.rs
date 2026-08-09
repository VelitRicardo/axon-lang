//! §Fase 118.b.3 — this suite exercises the SQL data plane (the Postgres
//! backend, the pinned-connection contract, live introspection), which lives
//! behind the `postgres` feature. Gated at the file level: the core suites keep
//! running in a build with no driver.
#![cfg(feature = "postgres")]

//! §Fase 65.F — Real-Postgres integration: a NON-streaming `persist` actually
//! WRITES the row (the §49 authoritative-signal safeguard for the kivi
//! 2026-06-24 data-plane gap report).
//!
//! The gap report claimed a non-streaming `persist into LtmSummaries` emitted
//! its provenance but never invoked the store backend — 0 SQL, flow aborts.
//! Static analysis showed the dispatcher's `run_persist` IS wired (not a stub)
//! and the unified driver runs it identically to the SSE path; the real defect
//! was that a PRE-INSERT failure was SILENTLY swallowed (closed by §65.F's
//! honest-error surface — see `fase65_f_nonstreaming_store_error_honesty`).
//!
//! This test is the OTHER half: it proves, against a REAL database, that when
//! nothing fails the non-streaming `persist` genuinely commits the row — so a
//! regression that reverts the write path (or re-stubs it) fails CI. Per the
//! §49 lesson, the adopter-side E2E write is the authoritative signal, not a
//! deploy/provenance check.
//!
//! # Harness
//!
//! Connects via `AXON_TEST_DATABASE_URL`; when unset or unreachable the test
//! skips gracefully (mirrors `fase35_l_postgres_integration` +
//! `fase65_navigate_pg_integration`). CI provides a `postgres:16` service
//! container. The store's `.axon` `connection:` reads the SAME env var, so the
//! runtime resolves to the test DB.

use axon::runner::execute_server_flow;
use axon::store::postgres_backend::PostgresStoreBackend;
use std::collections::HashMap;

/// Store name == table name (D12 — `IRAxonStore` carries no schema; the store
/// name doubles as the SQL table). Distinct from the other fase65 fixtures so
/// the suites never collide on a shared DB.
const TABLE: &str = "fase65f_ltm_summaries";

/// §Fase 119.i.2 — this suite REFUSES to skip.
///
/// It used to return `None` when `AXON_TEST_DATABASE_URL` was unset, and every
/// test returned early. Combined with the file-level `#![cfg(feature = "postgres")]`
/// that is TWO stacked doors, both shut by default: `cargo test` printed
/// `ok. 0 passed` and the suite had not executed a line in months. That is the
/// exact shape in which the §66 cross-tenant isolation bug survived — a green
/// report from a test that never ran.
///
/// Reaching this function means the caller already opted in with
/// `--features postgres`. Asking for the Postgres suite and silently getting
/// nothing is the failure mode; so a missing or unreachable database is now a
/// LOUD panic that says exactly how to provide one.
async fn test_backend() -> PostgresStoreBackend {
    let dsn = std::env::var("AXON_TEST_DATABASE_URL")
        .ok()
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_else(|| panic!("{}", refusal("AXON_TEST_DATABASE_URL is unset")));
    let backend = PostgresStoreBackend::connect(&dsn)
        .unwrap_or_else(|e| panic!("{}", refusal(&format!("backend connect failed: {e}"))));
    if let Err(e) = backend.ping().await {
        panic!("{}", refusal(&format!("Postgres unreachable: {e}")));
    }
    backend
}

/// The one refusal message, so all five pg suites say the same thing.
fn refusal(why: &str) -> String {
    format!(
        "fase65.F: {why} — and this suite REFUSES to skip.\n\n\
         You asked for the Postgres suite (`--features postgres`). A silent skip \
         here is how a suite reports `ok` for months without executing: it is the \
         shape in which the §66 cross-tenant isolation bug survived.\n\n\
         Start a THROWAWAY Postgres and point the variable at it:\n\
         \x20 docker run -d -e POSTGRES_PASSWORD=axon -e POSTGRES_DB=axon \
         -p 55433:5432 postgres:16-alpine\n\
         \x20 AXON_TEST_DATABASE_URL=postgres://postgres:axon@127.0.0.1:55433/axon\n\n\
         NEVER point it at a database you care about: these fixtures DROP TABLE and \
         CREATE ROLE."
    )
}

async fn exec(backend: &PostgresStoreBackend, sql: &str) {
    sqlx::query(sql)
        .execute(backend.pool())
        .await
        .unwrap_or_else(|e| panic!("fase65.F fixture SQL failed:\n  {sql}\n  {e}"));
}

/// Fresh table, no RLS (the WRITE is the claim under test, not isolation —
/// that's `fase65_navigate_pg_integration`'s job). Columns match the persist
/// field block below.
async fn fresh_table(backend: &PostgresStoreBackend) {
    exec(backend, &format!("DROP TABLE IF EXISTS {TABLE}")).await;
    exec(
        backend,
        &format!(
            "CREATE TABLE {TABLE} \
             (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, summary TEXT NOT NULL)"
        ),
    )
    .await;
}

async fn row_count(backend: &PostgresStoreBackend, id: &str) -> i64 {
    let row: (i64,) = sqlx::query_as(&format!("SELECT count(*) FROM {TABLE} WHERE id = $1"))
        .bind(id)
        .fetch_one(backend.pool())
        .await
        .expect("count query");
    row.0
}

async fn summary_of(backend: &PostgresStoreBackend, id: &str) -> String {
    let row: (String,) = sqlx::query_as(&format!("SELECT summary FROM {TABLE} WHERE id = $1"))
        .bind(id)
        .fetch_one(backend.pool())
        .await
        .expect("summary query");
    row.0
}

/// The persist's `connection:` reads `env:AXON_TEST_DATABASE_URL` — the same
/// DSN the harness connected with — so the runtime resolves to the test DB.
const SOURCE: &str = r#"
axonstore fase65f_ltm_summaries {
    backend: postgresql
    connection: "env:AXON_TEST_DATABASE_URL"
}

flow HibernateSession(tenant_id: String, summary_id: String) -> String {
    step Summarize { ask: "Summarize the conversation" output: String }
    persist into fase65f_ltm_summaries {
        id: "${summary_id}"
        tenant_id: "${tenant_id}"
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

#[tokio::test(flavor = "multi_thread")]
async fn nonstreaming_persist_commits_the_row() {
    let backend = test_backend().await;
    std::env::remove_var("AXON_LEGACY_EXECUTOR"); // exercise the unified engine

    fresh_table(&backend).await;

    let summary_id = "fase65f-summary-001";
    let tenant_id = "tenant-acme";

    // Pre-condition: the row does NOT exist (the gap report's `count(*) = 0`
    // BEFORE).
    assert_eq!(
        row_count(&backend, summary_id).await,
        0,
        "pre: the summary row must not exist yet"
    );

    let (_program, ir) =
        axon::flow_plan::compile_source_to_ir(SOURCE, "fase65_f_pg.axon").expect("compile");

    let body = serde_json::json!({
        "tenant_id": tenant_id,
        "summary_id": summary_id,
    });

    // The runner does its store I/O on its own internal runtime via
    // `block_on_store`; run it on a blocking thread so we don't nest runtimes.
    let metrics = tokio::task::spawn_blocking(move || {
        execute_server_flow(
            &ir,
            "HibernateSession",
            "stub", // the LLM step degrades to "(stub)"; the persist is real SQL
            "", // §Fase 95.f — tenant scope (empty = pre-fix behavior)
            "fase65_f_pg.axon",
            None,
            Some(&body),
            &HashMap::new(),
            &HashMap::new(),
            None,
            None, // §Fase 24.g.2 — llm_base_url
            None, // §Fase 24.g.2 — llm_chat_path
            None, // §Fase 72.c — budget (test: unbudgeted)
            None, // §Fase 114.e — channel semaphores (test: none)
            None, // §Fase 114.f — tool leases (test: none)
            None, // §Fase 74.f — event_outbox (test: in-process emit)
            None, // §Fase 92.c — credential minter (test: none)
            None, // §Fase 94.d — secret custody (test: none)
                None, // §Fase 108.b dataspace_engine (tests: fail closed)
                None, // §Fase 102 scrape_overrides
)
    })
    .await
    .expect("join")
    .expect("server runner Ok");

    // ── §1 — the flow SUCCEEDS and reaches `return` (no silent abort) ────
    assert!(
        metrics.success,
        "§1: the non-streaming flow must succeed. error={:?} step_names={:?}",
        metrics.error, metrics.step_names
    );
    assert!(
        metrics.error.is_none(),
        "§1: no honest-error on the happy path. Got: {:?}",
        metrics.error
    );

    // ── §2 — the row is ACTUALLY in the database (0 → 1) ─────────────────
    // This is the load-bearing assertion: the gap report's symptom was
    // `count(*) = 0` after the call. Post-§65.F it is 1.
    assert_eq!(
        row_count(&backend, summary_id).await,
        1,
        "§2 REGRESSION: the non-streaming `persist` did not write the row — \
         this is the kivi data-plane gap. The dispatcher's run_persist must \
         invoke the Postgres backend on the non-streaming path."
    );
    // The interpolated step-output value landed (the `${Summarize}` column).
    assert_eq!(
        summary_of(&backend, summary_id).await,
        "(stub)",
        "§2: the persisted summary is the (stub) step output, interpolated \
         into the field block"
    );

    drop_table(&backend).await;
}

async fn drop_table(backend: &PostgresStoreBackend) {
    exec(backend, &format!("DROP TABLE IF EXISTS {TABLE}")).await;
}
