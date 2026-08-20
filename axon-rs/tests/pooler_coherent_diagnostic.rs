//! v2.81.0 — this suite exercises the SQL data plane (the Postgres
//! backend, the pinned-connection contract, live introspection), which lives
//! behind the `postgres` feature. Gated at the file level: the core suites keep
//! running in a build with no driver.
#![cfg(feature = "postgres")]

//! v1.32.0 — Diagnostic anchor for the Pooler-Coherent Store cycle.
//!
//! Pins the **post-1.36.5 broken state** of the `axonstore` Postgres
//! backend so every later 37.x step inverts a specific -assertion.
//! Same forensic discipline as the 33.a / 34.a / 35.a / 37.a anchors:
//! capture the current behaviour with `eprintln!`, assert it minimally,
//! and let a post-fix regression surface as an anchor-inversion failure.
//!
//! # The four findings (see `the design plan` section 2)
//!
//!  - **A** — `column_types()` introspects on its own `&self.pool`
//!    checkout, separate from the operation's checkout.
//!  - **B** — that introspection resolves the table via `to_regclass`,
//!    which honours the connection's ambient `search_path`.
//!  - **C** — when introspection yields nothing, `build_pg_where`
//!    emits a bare `"col" = $N` — the form that fails `operator does
//!    not exist: uuid = text` against a typed column.
//!  - The schema cache has **no invalidation** — a live `ALTER TABLE`
//!    leaves a `(dsn, table)` entry stale until a process restart.
//!
//! # What this anchor pins — and what it honestly cannot
//!
//! Findings A+B compose into a defect that manifests ONLY behind a
//! transaction-mode pooler: two pool checkouts land on two physical
//! sessions whose `search_path` differs, so introspection resolves the
//! table on one session and the operation runs on another. On a DIRECT
//! connection every checkout shares one DSN-configured `search_path` —
//! introspection and operation are always coherent, and the bug cannot
//! be reproduced. The faithful smoke-15 reproduction therefore needs a
//! real transaction pooler and belongs to **37.x.i**'s PgBouncer
//! harness; it is not forced into a non-deterministic test here.
//!
//! This file pins what IS deterministic now:
//!
//! - section 1 — D4 equality fallback, infra-free: an unknown-type equality
//!    filter renders `"col"::text = $N`.    → inverted by 37.x.e (D4).
//! - section 2 — schema-anchored operation SQL, infra-free: with a resolved
//!    schema the operation SQL is `"schema"."table"`.
//!                                          → inverted by 37.x.c (D2).
//! - section 3 — the self-healing schema cache, real-Postgres: a live
//!    `ALTER TABLE` drift triggers the D9 evict + retry, and the
//!    operation succeeds.                  → inverted by 37.x.f (D9).
//! - section 4 — multi-schema resolution, real-Postgres: a table present in
//!    two schemas resolves silently by `search_path` order, with no
//!    ambiguity signal.                  → 37.x.b / D1 + D5 invert/keep.
//! - section 5 — totality, infra-free: ALL FOUR pure SQL builders emit the
//!    schema-qualified relation when a schema resolved.
//!                                          → inverted by 37.x.c (D2).

use std::collections::HashMap;

use axon::store::filter::{build_pg_where, SqlValue};
use axon::store::postgres_backend::{
    build_delete_sql, build_insert_sql, build_select_sql, build_update_sql,
    PostgresStoreBackend,
};

/// A real `tenant_id` of an adopter Postgres schema (the smoke-15
/// value) — a `uuid` primary key, the column class this cycle exists
/// to make work.
const ADOPTER_UUID: &str = "83d078e1-b372-42ba-9572-ff8dc521386e";

/// An empty bindings / column-types map — the unknown-schema state.
fn empty() -> HashMap<String, String> {
    HashMap::new()
}

/// v1.32.0 — legacy pool-backed connection source for the
/// backend ops exercised by the real-Postgres anchors (section 3 + section 4).
fn sc(backend: &PostgresStoreBackend) -> axon::store::store_conn::StoreConn<'_> {
    axon::store::store_conn::StoreConn::pool(backend.pool())
}

// ════════════════════════════════════════════════════════════════════
// section 1 — D4 equality type-agnostic fallback (infra-free). 37.x.e (D4)
//        INVERTED this anchor: an unknown-type equality filter renders
//        `"col"::text = $N` — the cast that makes it work against ANY
//        column type without introspection.
// ════════════════════════════════════════════════════════════════════

#[test]
fn s1_unknown_type_equality_filter_casts_the_column_to_text() {
    // The canonical request-bound agent filter: `${tenant_id}` from the
    // request body (v1.32.0) reaches the `where:` clause as a bind
    // value. The column's type is unknown — `column_types` is empty.
    // v1.32.0 (D4): for an EQUALITY comparison `build_pg_where` casts
    // the COLUMN to `text` — `"id"::text = $1` — so a `text`-bound
    // value compares `text = text` against a `uuid` column (or any
    // type). The pre-37.x.e bare `"id" = $1` was the `uuid = text`
    // failure this inverts.
    let bindings =
        HashMap::from([("tenant_id".to_string(), ADOPTER_UUID.to_string())]);

    let (clause, params) =
        build_pg_where("id == '${tenant_id}'", 0, &bindings, &empty())
            .expect("the where-expression compiles");

    eprintln!(
        "section 1 (37.x.e — D4 equality type-agnostic fallback):\n\
         where-expr     = id == '${{tenant_id}}'\n\
         column_types   = {{}}  (empty — introspection found nothing)\n\
         rendered WHERE = {clause:?}\n\
         bound params   = {params:?}\n\
         CONCLUSION: the equality renders `\"id\"::text = $1` — the\n\
         column is cast to `text`, so a `text`-bound value matches a\n\
         `uuid` (or any-type) column. The D4 degraded backstop."
    );

    assert_eq!(
        clause, "\"id\"::text = $1",
        "section 1 (D4): an unknown-type equality casts the COLUMN to `text` — \
         `text = text` works against any column type"
    );
    assert_eq!(
        params,
        vec![SqlValue::Text(ADOPTER_UUID.to_string())],
        "section 1: the request-bound value is carried as a `text` parameter"
    );
}

// ════════════════════════════════════════════════════════════════════
// section 2 — Schema-anchored operation SQL (infra-free). 37.x.c (D2)
//        INVERTED this anchor: with a resolved schema the operation SQL
//        is emitted SCHEMA-QUALIFIED — `"public"."tenants"` — so it no
//        longer rides the ambient `search_path`. A `None` schema keeps
//        the bare pre-37.x form (D5).
// ════════════════════════════════════════════════════════════════════

#[test]
fn s2_operation_sql_is_schema_qualified_when_resolved() {
    // v1.32.0 (D2) — given a resolved schema, the SELECT and INSERT name
    // the relation SCHEMA-QUALIFIED; `None` keeps the bare table. The
    // SELECT's WHERE carries the v1.32.0 D4 `::text` equality cast; the
    // INSERT's `$1` stays cast-less (the write-side type cast is D1+D8).
    let (qualified, _) = build_select_sql(
        "tenants",
        Some("public"),
        "id == '${t}'",
        &empty(),
        &empty(),
    )
    .expect("build_select_sql");
    let (bare, _) =
        build_select_sql("tenants", None, "id == '${t}'", &empty(), &empty())
            .expect("build_select_sql");
    let (insert_sql, _) = build_insert_sql(
        "chat_history",
        Some("public"),
        &[("tenant_id".to_string(), SqlValue::Text(ADOPTER_UUID.to_string()))],
        &empty(),
    )
    .expect("build_insert_sql");

    eprintln!(
        "section 2 (37.x.c — D2 schema-anchored operation SQL):\n\
         SELECT (schema resolved) = {qualified:?}\n\
         SELECT (schema absent)   = {bare:?}\n\
         INSERT (schema resolved) = {insert_sql:?}\n\
         CONCLUSION: a resolved schema renders `\"public\".\"tenants\"` —\n\
         the operation no longer rides the ambient `search_path`. A\n\
         `None` schema (resolution failed) keeps the bare form (D5)."
    );

    assert_eq!(
        qualified,
        "SELECT * FROM \"public\".\"tenants\" WHERE \"id\"::text = $1",
        "section 2 (D2): a resolved schema → the SELECT is schema-qualified \
         (the WHERE carries the v1.32.0 D4 equality `::text` cast)"
    );
    assert_eq!(
        bare, "SELECT * FROM \"tenants\" WHERE \"id\"::text = $1",
        "section 2 (D5): a `None` schema → the bare TABLE, pre-37.x form"
    );
    assert_eq!(
        insert_sql,
        "INSERT INTO \"public\".\"chat_history\" (\"tenant_id\") VALUES ($1)",
        "section 2 (D2): the INSERT is schema-qualified too; the cast-less `$1` \
         on an unknown-type column stays pinned for D1+D8 (37.x.g)"
    );
}

// ════════════════════════════════════════════════════════════════════
// section 5 — Totality (infra-free). 37.x.c (D2) INVERTED this anchor: ALL
//        FOUR pure SQL builders now emit the SCHEMA-QUALIFIED relation
//        when a schema resolved — D2 flipped every one, not three.
// ════════════════════════════════════════════════════════════════════

#[test]
fn s5_all_four_sql_builders_qualify_with_a_resolved_schema() {
    // The store-op SQL surface is exactly four pure builders. v1.32.0
    // (D2) — each emits `"schema"."t"` given a resolved schema; this
    // guards the totality so a regression cannot fix three and miss the
    // fourth.
    let data = [("v".to_string(), SqlValue::Integer(1))];
    let sqls: Vec<(&str, String)> = vec![
        (
            "build_select_sql",
            build_select_sql("t", Some("app"), "", &empty(), &empty())
                .unwrap()
                .0,
        ),
        (
            "build_delete_sql",
            build_delete_sql("t", Some("app"), "", &empty(), &empty())
                .unwrap()
                .0,
        ),
        (
            "build_insert_sql",
            build_insert_sql("t", Some("app"), &data, &empty()).unwrap().0,
        ),
        (
            "build_update_sql",
            build_update_sql("t", Some("app"), "", &data, &empty(), &empty())
                .unwrap()
                .0,
        ),
    ];

    for (name, sql) in &sqls {
        eprintln!("section 5 ({name}): {sql:?}");
        assert!(
            sql.contains("\"app\".\"t\""),
            "section 5 (D2): `{name}` must emit the schema-qualified \
             `\"app\".\"t\"` — D2 flips ALL FOUR builders"
        );
    }
    assert_eq!(sqls.len(), 4, "section 5: the store-op SQL surface is 4 builders");
}

// ════════════════════════════════════════════════════════════════════
// Real-Postgres harness — section 3 + section 4 (graceful skip without a database)
// ════════════════════════════════════════════════════════════════════

/// Resolve the test database backend, or `None` to skip — identical
/// discipline to `l`'s harness: unset env var OR an unreachable
/// Postgres skips gracefully so `cargo test` passes on a bare machine.
async fn test_backend() -> Option<PostgresStoreBackend> {
    let dsn = match std::env::var("AXON_TEST_DATABASE_URL") {
        Ok(d) if !d.trim().is_empty() => d,
        _ => {
            eprintln!(
                "a_2: AXON_TEST_DATABASE_URL unset — skipping the \
                 real-Postgres anchors (set it to run; CI always does)"
            );
            return None;
        }
    };
    let backend = match PostgresStoreBackend::connect(&dsn) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("a_2: backend connect failed ({e}) — skipping");
            return None;
        }
    };
    if let Err(e) = backend.ping().await {
        eprintln!("a_2: Postgres unreachable ({e}) — skipping");
        return None;
    }
    Some(backend)
}

/// Run a raw SQL statement (fixture DDL). Panics on failure — a broken
/// fixture is a test bug, not a graceful skip.
async fn exec(backend: &PostgresStoreBackend, sql: &str) {
    sqlx::query(sql)
        .execute(backend.pool())
        .await
        .unwrap_or_else(|e| panic!("a_2 fixture SQL failed:\n  {sql}\n  {e}"));
}

// ════════════════════════════════════════════════════════════════════
// section 3 — The self-healing schema cache (real-Postgres). 37.x.f (D9)
//        INVERTED this anchor: a live `ALTER TABLE` no longer poisons
//        the cache — the next operation hits the stale entry, fails
//        with a schema-drift SQLSTATE, EVICTS the entry, and retries
//        once against fresh introspection — and succeeds.
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s3_schema_cache_self_heals_after_a_live_alter_table() {
    let backend = match test_backend().await {
        Some(b) => b,
        None => return,
    };
    let table = "stale_2";

    exec(&backend, &format!("DROP TABLE IF EXISTS {table}")).await;
    exec(&backend, &format!("CREATE TABLE {table} (probe uuid, label text)")).await;

    // Seed + a first retrieve: this populates the process-global schema
    // cache for `(dsn, table)` with `{probe: uuid, label: text}` and
    // the typed-column filter works (`"probe" = $1::uuid`).
    backend
        .insert(
            &mut sc(&backend),
            table,
            &[
                ("probe".to_string(), SqlValue::Text(ADOPTER_UUID.to_string())),
                ("label".to_string(), SqlValue::Text("v1".to_string())),
            ],
        )
        .await
        .expect("section 3: seed insert");
    let before = backend
        .query(&mut sc(&backend), table, &format!("probe = '{ADOPTER_UUID}'"), &empty())
        .await;
    assert!(
        before.is_ok(),
        "section 3 precondition: the typed-column filter works before the \
         ALTER (and the cache is now populated)"
    );

    // A live schema migration: `probe` becomes `text`. The process
    // schema cache still holds the STALE `{probe: uuid}` mapping.
    exec(
        &backend,
        &format!("ALTER TABLE {table} ALTER COLUMN probe TYPE text USING probe::text"),
    )
    .await;

    // v1.32.0 (D9) — the next retrieve hits the STALE cache → casts
    // `$1::uuid` against the now-`text` column → `operator does not
    // exist: text = uuid` (SQLSTATE 42883) → the self-heal evicts the
    // `(dsn, table)` entry and retries once with fresh introspection
    // (`probe` is now `text`) → the retry succeeds.
    let after = backend
        .query(&mut sc(&backend), table, &format!("probe = '{ADOPTER_UUID}'"), &empty())
        .await;
    eprintln!(
        "section 3 (37.x.f — D9 self-healing schema cache):\n\
         after-ALTER query result = {after:?}\n\
         CONCLUSION: the stale `{{probe: uuid}}` cast failed `text =\n\
         uuid`; D9 evicted the `(dsn, table)` entry and retried with\n\
         fresh introspection — the operation self-healed."
    );
    let rows = after.expect(
        "v1.32.0 (D9): the operation SELF-HEALS — a schema-drift \
         SQLSTATE evicts the stale cache entry and retries once",
    );
    assert_eq!(rows.len(), 1, "section 3: the self-healed retrieve returns the row");

    exec(&backend, &format!("DROP TABLE IF EXISTS {table}")).await;
}

// ════════════════════════════════════════════════════════════════════
// section 4 — Multi-schema resolution (real-Postgres): a table present in
//        two schemas resolves silently by `search_path` order.
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s4_multi_schema_table_resolves_silently_by_search_path_order() {
    let base = match std::env::var("AXON_TEST_DATABASE_URL") {
        Ok(d) if !d.trim().is_empty() => d,
        _ => {
            eprintln!("a_2 s4: AXON_TEST_DATABASE_URL unset — skipping");
            return;
        }
    };
    let schema_a = "axon_f37xa_amb_a";
    let schema_b = "axon_f37xa_amb_b";
    let table = "dup_widgets";

    // Connect with a `search_path` that lists BOTH schemas, A before B.
    let sep = if base.contains('?') { '&' } else { '?' };
    let dsn = format!(
        "{base}{sep}options=-c%20search_path%3D{schema_a}%2C{schema_b}"
    );
    let backend = match PostgresStoreBackend::connect(&dsn) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("a_2 s4: connect failed ({e}) — skipping");
            return;
        }
    };
    if backend.ping().await.is_err() {
        eprintln!("a_2 s4: Postgres unreachable — skipping");
        return;
    }

    // The SAME table name in two schemas — the topology of a
    // schema-per-tenant adopter, or a legacy + working-schema overlap.
    for (schema, which) in [(schema_a, "from_schema_a"), (schema_b, "from_schema_b")]
    {
        exec(&backend, &format!("CREATE SCHEMA IF NOT EXISTS {schema}")).await;
        exec(&backend, &format!("DROP TABLE IF EXISTS {schema}.{table}")).await;
        exec(&backend, &format!("CREATE TABLE {schema}.{table} (which text)")).await;
        exec(
            &backend,
            &format!("INSERT INTO {schema}.{table} (which) VALUES ('{which}')"),
        )
        .await;
    }

    // Skip if the host/pooler dropped the `search_path` option (so the
    // unqualified name cannot resolve at all) — orthogonal to this pin.
    let resolves: Option<String> =
        sqlx::query_scalar(&format!("SELECT to_regclass('\"{table}\"')::text"))
            .fetch_one(backend.pool())
            .await
            .unwrap_or(None);
    if resolves.is_none() {
        eprintln!("a_2 s4: search_path option not honoured — skipping");
        for s in [schema_a, schema_b] {
            exec(&backend, &format!("DROP SCHEMA IF EXISTS {s} CASCADE")).await;
        }
        return;
    }

    // An unqualified `retrieve` resolves SILENTLY to the first schema on
    // the `search_path` — no ambiguity signal, no diagnostic.
    let rows = backend
        .query(&mut sc(&backend), table, "", &empty())
        .await
        .expect("section 4: the unqualified retrieve resolves");
    let picked = rows.first().and_then(|r| r.get("which")).cloned();

    eprintln!(
        "section 4 anchor (multi-schema → silent search_path-order resolution):\n\
         `{table}` exists in BOTH {schema_a} and {schema_b}\n\
         retrieve resolved {} row(s); picked `which` = {picked:?}\n\
         CONCLUSION: resolution is SILENT — `to_regclass` takes the\n\
         first `search_path` match, no ambiguity diagnostic. 37.x.b\n\
         (D1) keeps this resolvable case working (D5) and adds an\n\
         honest ambiguity error for the genuinely-unresolvable variant.",
        rows.len()
    );
    assert_eq!(
        picked,
        Some(serde_json::json!("from_schema_a")),
        "section 4: today a multi-schema table resolves silently to the FIRST \
         `search_path` schema — no ambiguity error. 37.x.b (D1) keeps \
         this (D5) but makes the unresolvable-ambiguous case honest."
    );

    for s in [schema_a, schema_b] {
        exec(&backend, &format!("DROP SCHEMA IF EXISTS {s} CASCADE")).await;
    }
}
