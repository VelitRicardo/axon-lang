//! v2.81.0 — this suite exercises the SQL data plane (the Postgres
//! backend, the pinned-connection contract, live introspection), which lives
//! behind the `postgres` feature. Gated at the file level: the core suites keep
//! running in a build with no driver.
#![cfg(feature = "postgres")]

//! v1.30.0 — Real-Postgres integration tests (D14, the robustness
//! floor).
//!
//! Every v1.30.0 SQL surface — the substrate (35.c-f), Pillar I
//! (epistemic, 35.g), Pillar III (`Stream<Row>`, 35.i), the type
//! mapping (35.c), Pillar II (audit chain, 35.h) and Pillar IV
//! (capability, 35.j) — exercised **end-to-end against a REAL
//! Postgres**. Not mocks: real rows, a real cursor, real type
//! decoding, real backpressure bounds.
//!
//! # The harness
//!
//! The test connects via the `AXON_TEST_DATABASE_URL` environment
//! variable. When it is unset, OR Postgres is unreachable, every test
//! **skips gracefully** — it logs the skip and returns, so
//! `cargo test` on a developer machine without Postgres still passes.
//! The CI harness (`.github/workflows/axonstore.yml`) provides
//! a `postgres:16` service container and sets the env var, so the
//! tests run for real on every push + PR — the implementation is
//! proven against an actual database before v1.30.0 ships.
//!
//! # Fixtures
//!
//! Each test owns a uniquely-named fixture table, `DROP`s it first
//! (idempotent re-runs) and `CREATE`s it via raw SQL. The test harness
//! is allowed DDL — D12's "no DDL" boundary is about the `axonstore`
//! *runtime*, not the test that builds its own fixtures. Distinct
//! table names keep the parallel test run collision-free.

use axon::cancel_token::CancellationFlag;
use axon::ir_nodes::IRAxonStore;
use axon::store::audit_chain::{ChainVerdict, StoreAuditChain, StoreMutationKind};
use axon::store::capability::check_store_capability;
use axon::store::epistemic::{enforce_retrieve_floor, mark_retrieved};
use axon::store::filter::SqlValue;
use axon::store::postgres_backend::PostgresStoreBackend;
use axon::store::registry::{StoreHandle, StoreRegistry};
use axon::store::row_stream::stream_retrieve;
use axon::stream_effect::BackpressurePolicy;

/// v1.32.0 — empty bindings: these v1.30.0 integration tests use
/// only literal `where` clauses (no `${name}` placeholders).
fn nb() -> std::collections::HashMap<String, String> {
    std::collections::HashMap::new()
}

/// v1.32.0 — legacy pool-backed connection source for the
/// backend ops. The integration tests exercise the v1.38.5 (and
/// earlier) non-pinned path; every op acquires a fresh logical
/// connection from the pool.
fn sc(backend: &PostgresStoreBackend) -> axon::store::store_conn::StoreConn<'_> {
    axon::store::store_conn::StoreConn::pool(backend.pool())
}

// ── Harness ─────────────────────────────────────────────────────────

/// Resolve the test database backend, or `None` to skip the test.
async fn test_backend() -> Option<PostgresStoreBackend> {
    let dsn = match std::env::var("AXON_TEST_DATABASE_URL") {
        Ok(d) if !d.trim().is_empty() => d,
        _ => {
            eprintln!(
                "l_2: AXON_TEST_DATABASE_URL unset — skipping \
                 real-Postgres integration (set it to run; CI always does)"
            );
            return None;
        }
    };
    let backend = match PostgresStoreBackend::connect(&dsn) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("l_2: backend connect failed ({e}) — skipping");
            return None;
        }
    };
    if let Err(e) = backend.ping().await {
        eprintln!("l_2: Postgres unreachable ({e}) — skipping");
        return None;
    }
    Some(backend)
}

/// Bind the backend or `return` from the test (graceful skip).
macro_rules! pg_or_skip {
    () => {
        match test_backend().await {
            Some(b) => b,
            None => return,
        }
    };
}

/// Run a raw SQL statement (fixture DDL / seed). Panics on failure —
/// a broken fixture is a test bug, not a graceful skip.
async fn exec(backend: &PostgresStoreBackend, sql: &str) {
    sqlx::query(sql)
        .execute(backend.pool())
        .await
        .unwrap_or_else(|e| panic!("l_2 fixture SQL failed:\n  {sql}\n  {e}"));
}

/// `DROP` + `CREATE` a fresh fixture table.
async fn fresh_table(backend: &PostgresStoreBackend, table: &str, columns: &str) {
    exec(backend, &format!("DROP TABLE IF EXISTS {table}")).await;
    exec(backend, &format!("CREATE TABLE {table} ({columns})")).await;
}

async fn drop_table(backend: &PostgresStoreBackend, table: &str) {
    exec(backend, &format!("DROP TABLE IF EXISTS {table}")).await;
}

fn text(s: &str) -> SqlValue {
    SqlValue::Text(s.to_string())
}

// ════════════════════════════════════════════════════════════════════
//  Substrate (35.c-f) — real CRUD round-trips
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t1_substrate_insert_then_query_round_trip() {
    let backend = pg_or_skip!();
    let table = "substrate_2";
    fresh_table(&backend, table, "id INTEGER, name TEXT, active BOOLEAN").await;

    let inserted = backend
        .insert(
            &mut sc(&backend),
            table,
            &[
                ("id".into(), SqlValue::Integer(7)),
                ("name".into(), text("alice")),
                ("active".into(), SqlValue::Boolean(true)),
            ],
        )
        .await
        .expect("insert");
    assert_eq!(inserted, 1, "one row persisted");

    let rows = backend.query(&mut sc(&backend), table, "id = 7", &nb()).await.expect("query");
    assert_eq!(rows.len(), 1, "the persisted row is retrievable");
    assert_eq!(rows[0].get("name"), Some(&serde_json::json!("alice")));
    assert_eq!(rows[0].get("active"), Some(&serde_json::json!(true)));

    drop_table(&backend, table).await;
}

#[tokio::test]
async fn t2_substrate_mutate_updates_real_rows() {
    let backend = pg_or_skip!();
    let table = "mutate_2";
    fresh_table(&backend, table, "id INTEGER, status TEXT").await;
    backend
        .insert(&mut sc(&backend), table, &[("id".into(), SqlValue::Integer(1)), ("status".into(), text("draft"))])
        .await
        .expect("seed");

    let affected = backend
        .mutate(&mut sc(&backend), table, "id = 1", &[("status".into(), text("published"))], &nb())
        .await
        .expect("mutate");
    assert_eq!(affected, 1);

    let rows = backend.query(&mut sc(&backend), table, "id = 1", &nb()).await.expect("query");
    assert_eq!(rows[0].get("status"), Some(&serde_json::json!("published")));

    drop_table(&backend, table).await;
}

#[tokio::test]
async fn t3_substrate_purge_deletes_real_rows() {
    let backend = pg_or_skip!();
    let table = "purge_2";
    fresh_table(&backend, table, "id INTEGER").await;
    for i in 1..=5 {
        backend
            .insert(&mut sc(&backend), table, &[("id".into(), SqlValue::Integer(i))])
            .await
            .expect("seed");
    }

    let purged = backend.purge(&mut sc(&backend), table, "id <= 3", &nb()).await.expect("purge");
    assert_eq!(purged, 3, "three rows deleted");
    let survivors = backend.query(&mut sc(&backend), table, "", &nb()).await.expect("query");
    assert_eq!(survivors.len(), 2, "two rows survive");

    drop_table(&backend, table).await;
}

// ════════════════════════════════════════════════════════════════════
//  Pillar I (35.g) — confidence_floor filters REAL rows
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t4_pillar_i_confidence_floor_filters_real_rows() {
    let backend = pg_or_skip!();
    let table = "epistemic_2";
    // The reserved `_confidence` column carries each row's stored
    // epistemic confidence (NUMERIC — precision-safe).
    fresh_table(&backend, table, "id INTEGER, _confidence NUMERIC").await;
    // Seed five rows spanning the [0, 1] confidence band.
    for (id, conf) in [(1, "0.95"), (2, "0.50"), (3, "0.80"), (4, "0.10"), (5, "0.99")] {
        exec(
            &backend,
            &format!("INSERT INTO {table} (id, _confidence) VALUES ({id}, {conf})"),
        )
        .await;
    }

    let outcome =
        stream_retrieve(&backend, &mut sc(&backend), table, "", "", "", "", "", BackpressurePolicy::DegradeQuality, 1000, &CancellationFlag::new(), &nb())
            .await
            .expect("stream_retrieve");
    assert_eq!(outcome.rows.len(), 5, "all five rows off the cursor");

    // A confidence_floor of 0.8 keeps only 0.95 / 0.80 / 0.99 — the
    // 0.50 and 0.10 rows are below the floor.
    let floored = enforce_retrieve_floor(mark_retrieved(outcome.rows), Some(0.8));
    assert_eq!(floored.trusted.len(), 3, "three rows at or above the floor");
    assert_eq!(floored.below_floor.len(), 2, "two sub-floor rows filtered");

    drop_table(&backend, table).await;
}

// ════════════════════════════════════════════════════════════════════
//  Pillar III (35.i) — Stream<Row> backpressure over a REAL cursor
// ════════════════════════════════════════════════════════════════════

/// Seed `n` rows into a single-`id`-column table.
async fn seed_n(backend: &PostgresStoreBackend, table: &str, n: i64) {
    fresh_table(backend, table, "id INTEGER").await;
    // One multi-row INSERT keeps the fixture fast.
    let values: Vec<String> = (1..=n).map(|i| format!("({i})")).collect();
    exec(
        backend,
        &format!("INSERT INTO {table} (id) VALUES {}", values.join(", ")),
    )
    .await;
}

#[tokio::test]
async fn t5_pillar_iii_drop_oldest_bounds_a_real_cursor() {
    let backend = pg_or_skip!();
    let table = "dropoldest";
    seed_n(&backend, table, 50).await;

    let outcome = stream_retrieve(
        &backend, &mut sc(&backend), table, "", "", "", "", "", BackpressurePolicy::DropOldest, 10, &CancellationFlag::new(),
        &nb(),
    )
    .await
    .expect("stream_retrieve");
    assert_eq!(outcome.rows.len(), 10, "drop_oldest bounds to the window");
    assert_eq!(outcome.total_seen, 50, "the cursor yielded all 50");
    assert_eq!(outcome.dropped, 40, "40 older rows dropped");

    drop_table(&backend, table).await;
}

#[tokio::test]
async fn t6_pillar_iii_pause_upstream_truncates_a_real_cursor() {
    let backend = pg_or_skip!();
    let table = "pause";
    seed_n(&backend, table, 50).await;

    let outcome = stream_retrieve(
        &backend, &mut sc(&backend), table, "", "", "", "", "", BackpressurePolicy::PauseUpstream, 10, &CancellationFlag::new(),
        &nb(),
    )
    .await
    .expect("stream_retrieve");
    assert_eq!(outcome.rows.len(), 10, "pause_upstream stops at the bound");
    assert!(outcome.truncated, "more rows existed past the bound");

    drop_table(&backend, table).await;
}

#[tokio::test]
async fn t7_pillar_iii_fail_errors_past_the_bound() {
    let backend = pg_or_skip!();
    let table = "fail_2";
    seed_n(&backend, table, 50).await;

    let result = stream_retrieve(
        &backend, &mut sc(&backend), table, "", "", "", "", "", BackpressurePolicy::Fail, 10, &CancellationFlag::new(),
        &nb(),
    )
    .await;
    assert!(result.is_err(), "fail policy errors on an over-bound result");

    drop_table(&backend, table).await;
}

#[tokio::test]
async fn t8_pillar_iii_cancel_stops_the_real_drain() {
    let backend = pg_or_skip!();
    let table = "cancel_2";
    seed_n(&backend, table, 50).await;

    let cancel = CancellationFlag::new();
    cancel.cancel(); // pre-cancelled
    let outcome = stream_retrieve(
        &backend, &mut sc(&backend), table, "", "", "", "", "", BackpressurePolicy::DegradeQuality, 1000, &cancel,
        &nb(),
    )
    .await
    .expect("stream_retrieve");
    assert!(outcome.cancelled, "a pre-cancelled drain reports cancelled");
    assert!(outcome.rows.is_empty(), "no row consumed under a fired cancel");

    drop_table(&backend, table).await;
}

// ════════════════════════════════════════════════════════════════════
//  Type mapping (35.c) — UUID / TIMESTAMPTZ / NUMERIC / JSONB / BYTEA
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t9_type_mapping_is_json_safe_for_every_supported_type() {
    let backend = pg_or_skip!();
    let table = "types_2";
    fresh_table(
        &backend,
        table,
        "u UUID, ts TIMESTAMPTZ, n NUMERIC, j JSONB, b BYTEA, i INTEGER, f DOUBLE PRECISION",
    )
    .await;
    exec(
        &backend,
        &format!(
            "INSERT INTO {table} (u, ts, n, j, b, i, f) VALUES \
             (gen_random_uuid(), now(), 1234.5678, '{{\"k\":1}}'::jsonb, \
             '\\xDEADBEEF'::bytea, 42, 2.5)"
        ),
    )
    .await;

    let rows = backend.query(&mut sc(&backend), table, "", &nb()).await.expect("query");
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    // The kivi-reported Python pain — UUID / TIMESTAMPTZ / NUMERIC are
    // pre-mapped to JSON-safe values; no monkey-patched encoder needed.
    assert!(r.get("u").unwrap().is_string(), "UUID → string");
    assert!(r.get("ts").unwrap().is_string(), "TIMESTAMPTZ → RFC3339 string");
    assert!(r.get("n").unwrap().is_string(), "NUMERIC → precision-safe string");
    assert!(r.get("j").unwrap().is_object(), "JSONB → JSON object");
    assert!(r.get("b").unwrap().is_string(), "BYTEA → base64 string");
    assert!(r.get("i").unwrap().is_i64(), "INTEGER → JSON number");
    assert!(r.get("f").unwrap().is_f64(), "DOUBLE PRECISION → JSON number");
    assert_eq!(r.get("n").unwrap().as_str(), Some("1234.5678"));

    drop_table(&backend, table).await;
}

// ════════════════════════════════════════════════════════════════════
//  Pillar II (35.h) — audit chain over REAL mutations
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t10_pillar_ii_audit_chain_records_real_mutations() {
    let backend = pg_or_skip!();
    let table = "audit_2";
    fresh_table(&backend, table, "id INTEGER, v TEXT").await;

    // Drive three real mutations + chain each as it happens.
    let mut chain = StoreAuditChain::new();

    let n = backend
        .insert(&mut sc(&backend), table, &[("id".into(), SqlValue::Integer(1)), ("v".into(), text("a"))])
        .await
        .expect("insert");
    chain.record(StoreMutationKind::Persist, table, &format!("{n} row(s)"));

    let n = backend
        .mutate(&mut sc(&backend), table, "id = 1", &[("v".into(), text("b"))], &nb())
        .await
        .expect("mutate");
    chain.record(StoreMutationKind::Mutate, table, &format!("{n} row(s)"));

    let n = backend.purge(&mut sc(&backend), table, "id = 1", &nb()).await.expect("purge");
    chain.record(StoreMutationKind::Purge, table, &format!("{n} row(s)"));

    // The mutation history of three REAL operations verifies Intact.
    assert_eq!(chain.len(), 3);
    assert_eq!(
        chain.verify(),
        ChainVerdict::Intact,
        "the audit chain of three real mutations verifies tamper-free"
    );

    drop_table(&backend, table).await;
}

// ════════════════════════════════════════════════════════════════════
//  Pillar IV (35.j) — capability gate over a REAL resolved backend
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t11_pillar_iv_capability_gates_a_real_postgresql_store() {
    let backend = pg_or_skip!();
    let dsn = std::env::var("AXON_TEST_DATABASE_URL").unwrap();
    let _ = &backend; // the skip-guard already proved Postgres reachable

    // A registry with a capability-gated postgresql store.
    let registry = StoreRegistry::build(&[IRAxonStore {
        node_type: "axonstore",
        source_line: 0,
        source_column: 0,
        name: "vault".to_string(),
        backend: "postgresql".to_string(),
        connection: dsn,
        confidence_floor: None,
        isolation: String::new(),
        on_breach: String::new(),
        capability: "vault.read".to_string(),
        class: String::new(),
        column_schema: None,
        resource_ref: String::new(),
    }])
    .expect("registry build");

    // The store resolves to a real Postgres backend.
    let handle = registry.resolve("vault").expect("resolve");
    assert!(matches!(handle, StoreHandle::Postgres(_)));

    // The capability gate denies a caller without `vault.read`...
    let required = registry.spec("vault").unwrap().capability.clone();
    assert!(
        check_store_capability("vault", &required, &["other.cap".to_string()])
            .is_err(),
        "a caller missing vault.read is denied"
    );
    // ...and admits one that holds it.
    assert!(
        check_store_capability("vault", &required, &["vault.read".to_string()])
            .is_ok(),
        "a caller holding vault.read is admitted"
    );
}

// ════════════════════════════════════════════════════════════════════
//  D4 — injection resistance, proven against a real database
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t12_d4_a_malicious_where_value_cannot_drop_a_real_table() {
    let backend = pg_or_skip!();
    let table = "injection_2";
    fresh_table(&backend, table, "id INTEGER, name TEXT").await;
    backend
        .insert(&mut sc(&backend), table, &[("id".into(), SqlValue::Integer(1)), ("name".into(), text("safe"))])
        .await
        .expect("seed");

    // A classic injection payload, fully inside a quoted string value.
    // build_pg_where parameterizes it — Postgres treats it as a literal
    // string to compare, NOT as SQL. The table survives.
    let malicious = format!("name = '; DROP TABLE {table}; --'");
    let rows = backend.query(&mut sc(&backend), table, &malicious, &nb()).await.expect("query runs");
    assert!(rows.is_empty(), "no row literally named the payload");

    // The table is still here — the injection was inert.
    let survivors = backend.query(&mut sc(&backend), table, "", &nb()).await.expect("table intact");
    assert_eq!(survivors.len(), 1, "the injection did NOT drop the table");

    drop_table(&backend, table).await;
}

// ════════════════════════════════════════════════════════════════════
// v1.36.2 — typed-column write + read against a real database
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t13_typed_column_write_and_read_round_trip() {
    let backend = pg_or_skip!();
    let table = "typed_2";
    // A table whose key is `uuid` and a counter is `integer` — the
    // shape of any adopter with an existing Postgres schema.
    fresh_table(&backend, table, "tid uuid, kind text, n integer").await;
    let uuid = "83d078e1-b372-42ba-9572-ff8dc521386e";

    // v1.36.2 — `persist`: the text-bound values are cast to the
    // introspected column types (`$N::uuid`, `$N::int4`), so the row
    // writes into the `uuid` + `integer` columns. Pre-1.36.2 this
    // failed: `column "tid" is of type uuid but expression is of
    // type text`.
    backend
        .insert(
            &mut sc(&backend),
            table,
            &[
                ("tid".into(), text(uuid)),
                ("kind".into(), text("greeting")),
                ("n".into(), text("7")),
            ],
        )
        .await
        .expect("v1.36.2 — persist into a uuid + integer column");

    // section 1.36.1 — `retrieve` filtering on the `uuid` column: the column
    // is cast to text in the `where` clause, the text value matches.
    let rows = backend
        .query(&mut sc(&backend), table, &format!("tid = '{uuid}'"), &nb())
        .await
        .expect("retrieve by uuid");
    assert_eq!(rows.len(), 1, "the row written to the uuid column round-trips");

    // v1.36.2 — `mutate` the `integer` column: the SET value is cast.
    let updated = backend
        .mutate(
            &mut sc(&backend),
            table,
            &format!("tid = '{uuid}'"),
            &[("n".into(), text("99"))],
            &nb(),
        )
        .await
        .expect("v1.36.2 — mutate a typed column");
    assert_eq!(updated, 1, "mutate updated the row via the typed where + set");

    drop_table(&backend, table).await;
}

// ════════════════════════════════════════════════════════════════════
// v1.36.5 — introspection resolves a table outside current_schema()
// ════════════════════════════════════════════════════════════════════

/// An adopter's table can live in a schema reachable via `search_path`
/// yet NOT be `current_schema()` — a legacy schema sitting behind
/// `public`. Pre-v1.36.5 `column_types()` keyed introspection on
/// `table_schema = current_schema()` (only the FIRST `search_path`
/// entry), so such a table was invisible: every typed-column filter
/// fell back to a bare `$N` and failed with the adopter-reported
/// `operator does not exist: uuid = text`. v1.36.5 resolves the table
/// via `to_regclass`, honouring the full `search_path` — the same
/// resolution the store's own `SELECT * FROM "t"` performs.
#[tokio::test]
async fn t14_introspection_resolves_a_table_outside_current_schema() {
    let base = match std::env::var("AXON_TEST_DATABASE_URL") {
        Ok(d) if !d.trim().is_empty() => d,
        _ => {
            eprintln!("l_2 t14: AXON_TEST_DATABASE_URL unset — skipping");
            return;
        }
    };
    // A backend whose search_path is `public, <legacy>` — current_schema()
    // is `public`, but the fixture table lives in the legacy schema.
    let schema = "axon_legacy_probe";
    let sep = if base.contains('?') { '&' } else { '?' };
    let dsn =
        format!("{base}{sep}options=-c%20search_path%3Dpublic%2C{schema}");
    let backend = match PostgresStoreBackend::connect(&dsn) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("l_2 t14: connect failed ({e}) — skipping");
            return;
        }
    };
    if backend.ping().await.is_err() {
        eprintln!("l_2 t14: Postgres unreachable — skipping");
        return;
    }

    let table = "t14_legacy_widgets";
    let qualified = format!("{schema}.{table}");
    exec(&backend, &format!("CREATE SCHEMA IF NOT EXISTS {schema}")).await;
    exec(&backend, &format!("DROP TABLE IF EXISTS {qualified}")).await;
    exec(
        &backend,
        &format!("CREATE TABLE {qualified} (tid uuid, label text)"),
    )
    .await;

    // The bug needs the table reachable via `search_path` WITHOUT being
    // `current_schema()`. If the harness did not honour the `options`
    // search_path (some poolers drop it), the unqualified name will not
    // resolve — skip, since search-path resolution is orthogonal to
    // pooling and the direct-connection lane covers it unconditionally.
    let unqualified_resolves: Option<String> =
        sqlx::query_scalar(&format!("SELECT to_regclass('\"{table}\"')::text"))
            .fetch_one(backend.pool())
            .await
            .unwrap_or(None);
    if unqualified_resolves.is_none() {
        eprintln!(
            "l_2 t14: search_path option not honoured here — skipping \
             (covered by the direct-connection lane)"
        );
        exec(&backend, &format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).await;
        return;
    }

    let uuid = "83d078e1-b372-42ba-9572-ff8dc521386e";

    // persist — the write path introspects via `to_regclass`, resolves
    // the legacy schema and casts `$N::uuid` / `$N::text`.
    backend
        .insert(
            &mut sc(&backend),
            table,
            &[("tid".into(), text(uuid)), ("label".into(), text("probe"))],
        )
        .await
        .expect("v1.36.5 — persist resolves a table outside current_schema()");

    // retrieve filtering on the `uuid` column — pre-v1.36.5 this is the
    // adopter's exact failure (`operator does not exist: uuid = text`).
    let rows = backend
        .query(&mut sc(&backend), table, &format!("tid = '{uuid}'"), &nb())
        .await
        .expect(
            "v1.36.5 — retrieve resolves a uuid column outside current_schema()",
        );
    assert_eq!(rows.len(), 1, "the row in the legacy schema round-trips");
    assert_eq!(rows[0].get("label"), Some(&serde_json::json!("probe")));

    exec(&backend, &format!("DROP TABLE IF EXISTS {qualified}")).await;
    exec(&backend, &format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).await;
}

// ════════════════════════════════════════════════════════════════════
// v1.32.0 — one coherent introspect-and-operate session
// ════════════════════════════════════════════════════════════════════

/// v1.32.0 (D3) — the first operation on a table is a cache MISS:
/// the schema introspection and the operation execute inside ONE
/// transaction. Every subsequent operation is a cache HIT and runs
/// without a transaction. This exercises BOTH paths against a real
/// database and asserts they produce identical, correct results.
#[tokio::test]
async fn t15_coherent_session_miss_then_hit() {
    let backend = pg_or_skip!();
    let table = "coherent_2";
    fresh_table(&backend, table, "id integer, label text").await;

    // First op — a cache MISS: resolve + INSERT in one transaction.
    let inserted = backend
        .insert(
            &mut sc(&backend),
            table,
            &[
                ("id".into(), SqlValue::Integer(1)),
                ("label".into(), text("miss")),
            ],
        )
        .await
        .expect("v1.32.0 — the miss-path insert resolves + writes in one txn");
    assert_eq!(inserted, 1);

    // Second op — a cache HIT: runs without a transaction, and SEES the
    // miss-path write (it was committed when the miss-path txn closed).
    let rows = backend
        .query(&mut sc(&backend), table, "id = 1", &nb())
        .await
        .expect("v1.32.0 — the hit-path query needs no transaction");
    assert_eq!(
        rows.len(),
        1,
        "the miss-path write is visible to the hit-path read"
    );
    assert_eq!(rows[0].get("label"), Some(&serde_json::json!("miss")));

    // A third op — still a HIT — mutates; the result is correct.
    let updated = backend
        .mutate(&mut sc(&backend), table, "id = 1", &[("label".into(), text("hit"))], &nb())
        .await
        .expect("v1.32.0 — the hit-path mutate");
    assert_eq!(updated, 1);
    let rows = backend.query(&mut sc(&backend), table, "id = 1", &nb()).await.expect("query");
    assert_eq!(rows[0].get("label"), Some(&serde_json::json!("hit")));

    drop_table(&backend, table).await;
}

// ════════════════════════════════════════════════════════════════════
// v1.32.0 — D9 self-healing schema cache
// ════════════════════════════════════════════════════════════════════

/// v1.32.0 (D9) — a live `ALTER TABLE` between two writes drifts
/// the cache; the second write hits the stale entry, fails with a
/// schema-drift SQLSTATE, and SELF-HEALS (evict + retry once against
/// fresh introspection). The retry is safe — a drift SQLSTATE is a
/// parse/plan-time rejection, so the failed `INSERT` wrote ZERO rows:
/// this proves it by asserting EXACTLY ONE row was added, not two.
#[tokio::test]
async fn t16_d9_write_self_heals_without_double_writing() {
    let backend = pg_or_skip!();
    let table = "selfheal";
    fresh_table(&backend, table, "id integer").await;

    // First write — a cache MISS — caches `{id: int4}`.
    backend
        .insert(&mut sc(&backend), table, &[("id".into(), SqlValue::Integer(1))])
        .await
        .expect("v1.32.0 — the seed insert");

    // A live migration: `id` becomes `text`. The cached `{id: int4}` is
    // now stale; the existing row's `id` becomes the text `'1'`.
    exec(
        &backend,
        &format!("ALTER TABLE {table} ALTER COLUMN id TYPE text USING id::text"),
    )
    .await;

    // The next write hits the STALE cache → `INSERT … VALUES ($1::int4)`
    // into the now-`text` column → SQLSTATE 42804 → D9 evicts + retries
    // once with fresh introspection (`id` is now `text`) → succeeds.
    let inserted = backend
        .insert(&mut sc(&backend), table, &[("id".into(), text("3"))])
        .await
        .expect("v1.32.0 (D9) — the write SELF-HEALS after the drift");
    assert_eq!(inserted, 1, "the self-healed insert reports one row");

    // The retry-safety proof: the drifted first attempt wrote ZERO rows
    // (a parse/plan-time rejection), so the table holds EXACTLY the
    // seed row + the one self-healed row — no double-write.
    let rows = backend.query(&mut sc(&backend), table, "", &nb()).await.expect("count");
    assert_eq!(
        rows.len(),
        2,
        "v1.32.0 — exactly one row was added by the self-healed write; \
         a double-write would leave three"
    );

    drop_table(&backend, table).await;
}

// ════════════════════════════════════════════════════════════════════
// v1.32.0 — deploy-time eager schema verification (D8)
// ════════════════════════════════════════════════════════════════════

/// v1.32.0 (D8) — `verify_postgres_schemas` resolves a REAL table
/// at "deploy" → `verified`; a non-existent table on the SAME reachable
/// database → `missing` → `has_fatal()` → the deploy would FAIL.
#[tokio::test]
async fn t17_d8_deploy_verification_verified_and_missing() {
    let backend = pg_or_skip!();
    let dsn = std::env::var("AXON_TEST_DATABASE_URL").unwrap();

    let real_table = "real_2";
    fresh_table(&backend, real_table, "id integer").await;

    // A registry with one store on a REAL table + one on a table that
    // does not exist — both on the reachable test database.
    let registry = StoreRegistry::build(&[
        IRAxonStore {
            node_type: "axonstore",
            source_line: 0,
            source_column: 0,
            name: real_table.to_string(),
            backend: "postgresql".to_string(),
            connection: dsn.clone(),
            confidence_floor: None,
            isolation: String::new(),
            on_breach: String::new(),
            capability: String::new(),
            class: String::new(),
            column_schema: None,
            resource_ref: String::new(),
        },
        IRAxonStore {
            node_type: "axonstore",
            source_line: 0,
            source_column: 0,
            name: "ghost_table".to_string(),
            backend: "postgresql".to_string(),
            connection: dsn.clone(),
            confidence_floor: None,
            isolation: String::new(),
            on_breach: String::new(),
            capability: String::new(),
            class: String::new(),
            column_schema: None,
            resource_ref: String::new(),
        },
    ])
    .expect("registry build");

    let report = registry.verify_postgres_schemas().await;
    assert!(
        report.verified.contains(&real_table.to_string()),
        "v1.32.0 — the real table verified at deploy"
    );
    assert_eq!(
        report.missing.len(),
        1,
        "v1.32.0 — the ghost table is a fatal `missing` entry"
    );
    assert_eq!(report.missing[0].0, "ghost_table");
    assert!(
        report.has_fatal(),
        "v1.32.0 (D8) — a missing table on a reachable store FAILS the deploy"
    );

    // v1.32.0 (D6) — the `missing` diagnostic carries the masked DSN
    // so a deploy-log operator can SEE which physical database resolved.
    // The credential MUST be masked, never leaked.
    let diag = &report.missing[0].1;
    assert!(
        diag.contains("database:"),
        "v1.32.0 (D6) — the missing diagnostic must surface the \
         physical-database context, got: {diag}"
    );
    assert!(
        diag.contains("***") || !diag.contains('@'),
        "v1.32.0 (D6) — when the DSN carries credentials they MUST be \
         masked (`***`), got: {diag}"
    );
    // The actionable hint from `TableNotResolved`'s enriched Display is
    // also threaded through unchanged.
    assert!(
        diag.contains("pg_catalog"),
        "v1.32.0 (D6) — the actionable hint (pg_catalog scan, so \
         `search_path` is not the culprit) must survive composition \
         with the masked DSN, got: {diag}"
    );

    drop_table(&backend, real_table).await;
}
