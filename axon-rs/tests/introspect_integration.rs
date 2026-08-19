//! v2.81.0 — this suite exercises the SQL data plane (the Postgres
//! backend, the pinned-connection contract, live introspection), which lives
//! behind the `postgres` feature. Gated at the file level: the core suites keep
//! running in a build with no driver.
#![cfg(feature = "postgres")]

//! v1.31.0 (D10) — Integration tests for `axon store introspect`
//! against a REAL `postgres:16` service container.
//!
//! Tests gated on `AXON_TEST_DATABASE_URL` — graceful skip when the
//! env var is unset OR Postgres is unreachable, matching the 37.x
//! `l` / `a_3` / `pgbouncer_integration`
//! discipline. The CI lane `pgbouncer-transaction-mode` (in
//! `.github/workflows/axonstore.yml`) sets the var so these
//! tests run on every PR + master push.
//!
//! Five tests covering the load-bearing 38.h paths:
//!
//!   - **t1 — happy-path 15-type-catalog introspection**: a fixture
//!     table covers every Postgres column-type alias in the closed
//!     catalog (uuid + text + int4 + bigint + numeric + bool +
//!     timestamptz + date + jsonb + bytea). The introspection
//!     produces a manifest with all columns + correct types + the
//!     content-hash verifies.
//!
//!   - **t2 — omits unmappable types with honest reason**: a fixture
//!     table mixes catalog columns (`id uuid`) with non-catalog
//!     columns (`label citext`, `loc point`). The introspection emits
//!     the catalog columns AND a sidecar list of OmittedColumn
//!     entries — NEVER silently lossily mapped (`citext` is NOT
//!     written into the manifest as `Text` even though they look
//!     alike at the wire).
//!
//!   - **t3 — constraints round-trip end-to-end**: a fixture table
//!     with PRIMARY KEY + NOT NULL + UNIQUE + DEFAULT round-trips
//!     through the introspection — every constraint flag matches
//!     what the introspector reads off `pg_attribute` /
//!     `pg_constraint`.
//!
//!   - **t4 — auto_increment heuristic recognises SERIAL columns**:
//!     a `SERIAL` PK has its default expression `nextval('…')`
//!     correctly classified as `auto_increment: true` + the
//!     `nextval(…)` expression DROPPED from the manifest (sequence
//!     references are adopter-private).
//!
//!   - **t5 — diff mode flags column-type changes**: introspect a
//!     table, modify it (`ALTER COLUMN … TYPE`), introspect again,
//!     diff against the first manifest, assert the type-change
//!     delta appears verbatim.
//!
//! Each test owns its own fixture table and drops it on exit so a
//! parallel test run is collision-free.

use axon::store::introspect_cli::{introspect_store, introspect_stores};
use axon::store::postgres_backend::PostgresStoreBackend;
use axon::store_introspect::{format_manifest_diff, manifest_diff, ColumnDelta};
use axon::store_schema::StoreColumnType;

/// Resolve the test backend or return `None` to skip — same shape as
/// every prior PG-gated test in the workspace.
async fn pg_or_skip() -> Option<(PostgresStoreBackend, String)> {
    let dsn = match std::env::var("AXON_TEST_DATABASE_URL") {
        Ok(d) if !d.trim().is_empty() => d,
        _ => {
            eprintln!(
                "h_2: AXON_TEST_DATABASE_URL unset — skipping the \
                 introspection-CLI integration (set it to run; CI always does)"
            );
            return None;
        }
    };
    let backend = match PostgresStoreBackend::connect(&dsn) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("h_2: backend connect failed ({e}) — skipping");
            return None;
        }
    };
    if let Err(e) = backend.ping().await {
        eprintln!("h_2: Postgres unreachable ({e}) — skipping");
        return None;
    }
    Some((backend, dsn))
}

async fn exec_raw(backend: &PostgresStoreBackend, sql: &str) {
    sqlx::query(sql)
        .execute(backend.pool())
        .await
        .unwrap_or_else(|e| panic!("h_2 fixture SQL failed:\n  {sql}\n  {e}"));
}

#[tokio::test]
async fn t1_happy_path_15_type_catalog_introspection() {
    let Some((backend, dsn)) = pg_or_skip().await else { return };
    let table = "catalog_2";
    exec_raw(&backend, &format!("DROP TABLE IF EXISTS {table}")).await;
    exec_raw(
        &backend,
        &format!(
            "CREATE TABLE {table} (\
                id UUID PRIMARY KEY, \
                name TEXT NOT NULL, \
                count INTEGER, \
                big_count BIGINT, \
                balance NUMERIC, \
                active BOOLEAN, \
                created_at TIMESTAMPTZ, \
                event_date DATE, \
                payload JSONB, \
                blob BYTEA)"
        ),
    )
    .await;

    let (manifest, omissions) = introspect_store(&dsn, table)
        .await
        .expect("introspect happy-path");
    assert!(omissions.is_empty(), "no unmappable types: {omissions:?}");
    // The store is keyed by `<resolved-schema>.<store_name>`.
    let key = manifest
        .stores
        .keys()
        .find(|k| k.ends_with(&format!(".{table}")))
        .expect("store key matches table name");
    let store = manifest.lookup(key).unwrap();
    // The 10 declared columns survive.
    assert_eq!(store.columns.len(), 10);
    assert_eq!(store.columns.get("id").unwrap().col_type, StoreColumnType::Uuid);
    assert_eq!(store.columns.get("name").unwrap().col_type, StoreColumnType::Text);
    assert_eq!(store.columns.get("count").unwrap().col_type, StoreColumnType::Int);
    assert_eq!(store.columns.get("big_count").unwrap().col_type, StoreColumnType::BigInt);
    assert_eq!(store.columns.get("balance").unwrap().col_type, StoreColumnType::Numeric);
    assert_eq!(store.columns.get("active").unwrap().col_type, StoreColumnType::Bool);
    assert_eq!(
        store.columns.get("created_at").unwrap().col_type,
        StoreColumnType::Timestamptz
    );
    assert_eq!(store.columns.get("event_date").unwrap().col_type, StoreColumnType::Date);
    assert_eq!(store.columns.get("payload").unwrap().col_type, StoreColumnType::Jsonb);
    assert_eq!(store.columns.get("blob").unwrap().col_type, StoreColumnType::Bytea);
    // Constraints survive.
    assert!(store.columns.get("id").unwrap().primary_key);
    assert!(store.columns.get("id").unwrap().not_null);
    assert!(store.columns.get("name").unwrap().not_null);
    // Content-hash verifies (the manifest was refreshed).
    manifest.verify_content_hash().expect("hash verifies after introspect");

    exec_raw(&backend, &format!("DROP TABLE {table}")).await;
}

#[tokio::test]
async fn t2_omits_unmappable_types_with_honest_reason() {
    let Some((backend, dsn)) = pg_or_skip().await else { return };
    let table = "omits_2";
    exec_raw(&backend, &format!("DROP TABLE IF EXISTS {table}")).await;
    // `point` is a builtin Postgres geometric type outside the
    // closed v1.30.0 catalog — perfect omission test. `inet` is
    // another stable-builtin non-catalog type.
    exec_raw(
        &backend,
        &format!(
            "CREATE TABLE {table} (\
                id UUID PRIMARY KEY, \
                location POINT, \
                source_ip INET)"
        ),
    )
    .await;

    let (manifest, omissions) = introspect_store(&dsn, table)
        .await
        .expect("introspect omission case");
    // Two columns must be omitted with reasons; one survives.
    assert_eq!(omissions.len(), 2, "got: {omissions:?}");
    let names: Vec<&str> = omissions.iter().map(|o| o.name.as_str()).collect();
    assert!(names.contains(&"location"));
    assert!(names.contains(&"source_ip"));
    // Reasons name the closed catalog (adopter knows why).
    assert!(omissions[0].reason.contains("closed type catalog"));
    // `tier_enum`-style honest omission — `point`/`inet` are NOT
    // silently mapped to `Text` even though they have a text
    // representation. (Critical D10/D6 guarantee.)
    let key = manifest
        .stores
        .keys()
        .find(|k| k.ends_with(&format!(".{table}")))
        .expect("store key");
    let store = manifest.lookup(key).unwrap();
    assert_eq!(store.columns.len(), 1, "only `id` survives the omission filter");
    assert!(store.columns.contains_key("id"));

    exec_raw(&backend, &format!("DROP TABLE {table}")).await;
}

#[tokio::test]
async fn t3_constraints_round_trip_through_introspection() {
    let Some((backend, dsn)) = pg_or_skip().await else { return };
    let table = "constraints_2";
    exec_raw(&backend, &format!("DROP TABLE IF EXISTS {table}")).await;
    exec_raw(
        &backend,
        &format!(
            "CREATE TABLE {table} (\
                id UUID PRIMARY KEY, \
                email TEXT NOT NULL UNIQUE, \
                tier TEXT NOT NULL DEFAULT 'standard', \
                notes TEXT)"
        ),
    )
    .await;

    let (manifest, _) = introspect_store(&dsn, table).await.expect("introspect");
    let key = manifest
        .stores
        .keys()
        .find(|k| k.ends_with(&format!(".{table}")))
        .expect("store key");
    let store = manifest.lookup(key).unwrap();

    let id = store.columns.get("id").unwrap();
    assert!(id.primary_key, "id is PRIMARY KEY");
    assert!(id.not_null, "PRIMARY KEY implies NOT NULL");

    let email = store.columns.get("email").unwrap();
    assert!(email.not_null);
    assert!(email.unique, "single-column UNIQUE constraint surfaces");

    let tier = store.columns.get("tier").unwrap();
    assert!(tier.not_null);
    assert!(
        tier.default_value.contains("standard"),
        "static default round-trips through the manifest; got: {}",
        tier.default_value
    );
    assert!(!tier.auto_increment, "static text default is NOT auto_increment");

    let notes = store.columns.get("notes").unwrap();
    assert!(!notes.not_null, "nullable column surfaces as not_null=false");
    assert!(!notes.primary_key);

    exec_raw(&backend, &format!("DROP TABLE {table}")).await;
}

#[tokio::test]
async fn t4_auto_increment_heuristic_recognises_serial_columns() {
    let Some((backend, dsn)) = pg_or_skip().await else { return };
    let table = "serial";
    exec_raw(&backend, &format!("DROP TABLE IF EXISTS {table}")).await;
    exec_raw(
        &backend,
        &format!(
            "CREATE TABLE {table} (\
                id SERIAL PRIMARY KEY, \
                event TEXT NOT NULL)"
        ),
    )
    .await;

    let (manifest, _) = introspect_store(&dsn, table).await.expect("introspect");
    let key = manifest
        .stores
        .keys()
        .find(|k| k.ends_with(&format!(".{table}")))
        .expect("store key");
    let id = manifest.lookup(key).unwrap().columns.get("id").unwrap();
    assert_eq!(id.col_type, StoreColumnType::Int);
    assert!(id.auto_increment, "SERIAL must classify as auto_increment");
    assert!(
        id.default_value.is_empty(),
        "auto_increment columns DROP the nextval(...) default — sequence \
         names are adopter-private. Got: {}",
        id.default_value
    );
    assert!(id.primary_key);
    assert!(id.not_null);

    exec_raw(&backend, &format!("DROP TABLE {table}")).await;
}

#[tokio::test]
async fn t5_diff_mode_flags_column_type_changes() {
    let Some((backend, dsn)) = pg_or_skip().await else { return };
    let table = "diff_2";
    exec_raw(&backend, &format!("DROP TABLE IF EXISTS {table}")).await;
    exec_raw(
        &backend,
        &format!("CREATE TABLE {table} (id INTEGER PRIMARY KEY, name TEXT)"),
    )
    .await;

    // First introspection — the baseline manifest.
    let (baseline, _) = introspect_store(&dsn, table).await.expect("baseline");

    // Live ALTER — change `id` from int4 to int8 (BigInt).
    exec_raw(
        &backend,
        &format!("ALTER TABLE {table} ALTER COLUMN id TYPE BIGINT"),
    )
    .await;

    // Second introspection — captures the drift.
    let (after, _) = introspect_store(&dsn, table).await.expect("after-alter");

    let diff = manifest_diff(&baseline, &after);
    assert!(!diff.is_empty(), "ALTER must produce a diff");

    let key = baseline
        .stores
        .keys()
        .find(|k| k.ends_with(&format!(".{table}")))
        .expect("baseline store key");
    let deltas = diff.per_store.get(key).expect("per-store delta");
    let type_changes: Vec<&ColumnDelta> = deltas
        .iter()
        .filter(|d| matches!(d, ColumnDelta::TypeChanged { .. }))
        .collect();
    assert_eq!(type_changes.len(), 1, "exactly one type change");
    match type_changes[0] {
        ColumnDelta::TypeChanged { column, old_type, new_type } => {
            assert_eq!(column, "id");
            assert_eq!(*old_type, StoreColumnType::Int);
            assert_eq!(*new_type, StoreColumnType::BigInt);
        }
        _ => unreachable!(),
    }

    // The human-readable summary mentions the type change explicitly.
    let summary = format_manifest_diff(&diff);
    assert!(summary.contains("column `id` type: Int → BigInt"));

    exec_raw(&backend, &format!("DROP TABLE {table}")).await;
}

#[tokio::test]
async fn t6_multi_store_introspect_merges_into_one_manifest() {
    // Bonus test — `introspect_stores` aggregates multiple stores into
    // one manifest with a single content_hash. Exercises the
    // `axon store introspect a b c` CLI shape.
    let Some((backend, dsn)) = pg_or_skip().await else { return };
    let table_a = "a_2";
    let table_b = "b_2";
    for table in [table_a, table_b] {
        exec_raw(&backend, &format!("DROP TABLE IF EXISTS {table}")).await;
        exec_raw(
            &backend,
            &format!("CREATE TABLE {table} (id UUID PRIMARY KEY)"),
        )
        .await;
    }

    let (manifest, omissions) = introspect_stores(
        &dsn,
        &[table_a.to_string(), table_b.to_string()],
    )
    .await
    .expect("multi-store introspect");

    assert!(omissions.is_empty());
    assert!(manifest
        .stores
        .keys()
        .any(|k| k.ends_with(&format!(".{table_a}"))));
    assert!(manifest
        .stores
        .keys()
        .any(|k| k.ends_with(&format!(".{table_b}"))));
    manifest.verify_content_hash().expect("merged hash verifies");

    for table in [table_a, table_b] {
        exec_raw(&backend, &format!("DROP TABLE {table}")).await;
    }
}
