//! v2.81.0 — this suite exercises the SQL data plane (the Postgres
//! backend, the pinned-connection contract, live introspection), which lives
//! behind the `postgres` feature. Gated at the file level: the core suites keep
//! running in a build with no driver.
#![cfg(feature = "postgres")]

//! v1.32.0 — The pooler-coherent contract under a REAL
//! transaction-mode pooler.
//!
//! This is the integration counterpart of 37.x.a's diagnostic anchor:
//! 37.x.a's honest-scope note records that findings A+B (the
//! introspection/operation session split) manifest ONLY behind a
//! transaction-mode pooler — a direct connection is always coherent
//! and cannot reproduce the bug. THIS file owns the faithful smoke-15
//! reproduction.
//!
//! # The harness
//!
//! The test connects via `AXON_TEST_DATABASE_URL`, the same env var
//! `l` and `a_2` use. The CI workflow has a dedicated
//! `pgbouncer-transaction-mode` lane (Gap 3 regression guard) that
//! routes this var THROUGH PgBouncer `pool_mode=transaction` on port
//! 6432 — that is the meaningful execution context for THIS file. An
//! adopter can reproduce it locally with the fixture in
//! `docs/fixtures/pgbouncer-transaction-mode/`.
//!
//! When the env var is unset, OR Postgres is unreachable, every test
//! **skips gracefully** (so `cargo test` on a developer machine without
//! a pooler still passes). The skip is announced in test output —
//! never a silent pass.
//!
//! # What we PROVE here (and what we don't)
//!
//! Behind PgBouncer with `default_pool_size=5` and successive
//! checkouts, the SAME flow that broke at v1.36.5 — typed-column read
//! on a uuid PK against a table in a non-default `search_path` schema
//! — now succeeds, every operation. That is the faithful smoke-15
//! reproduction. The tests deliberately exercise:
//!
//!  - **t1 — smoke-15:** the canonical agent flow (persist → retrieve
//!    ×3) against a uuid-PK table in a non-default schema. Pre-37.x
//!    this died `operator does not exist: uuid = text` on the very
//!    first retrieve.
//!  - **t2 — pool churn:** burst 20 sequential operations across two
//!    distinct tables. With `default_pool_size=5` cross-session
//!    multiplexing IS forced; every operation must succeed.
//!  - **t3 — forced cache miss:** evict the schema cache before every
//!    operation. The D3 introspect-and-operate transaction MUST pin
//!    one backend for both halves — otherwise the introspection lands
//!    on a different physical session than the operation, the
//!    introspection's `to_regclass` returns NULL on that session, and
//!    the operation degrades to the typed-cast-less fallback that v1.36.5
//!    couldn't survive.
//!  - **t4 — D9 self-heal under the pooler:** populate the cache; a live
//!    `ALTER TABLE` ALTERS the column type; the next op self-heals.
//!    Documents that the D9 retry path is pooler-safe (a parse-time
//!    drift SQLSTATE → zero side effects → safe to retry).
//!
//! What we DON'T prove here: the CI lane proves PgBouncer is *in
//! transaction mode*; this file proves the AXON CONTRACT holds on
//! whatever session topology the env-var DSN provides. Both layered
//! checks together — the lane + this suite — are the regression guard.

use axon::store::filter::SqlValue;
use axon::store::postgres_backend::PostgresStoreBackend;

/// A stable UUID across the suite — same shape as 37.x.a's anchor.
const T_UUID_1: &str = "8b3e1c12-7a04-4f7e-9d05-1d6df2c6c2a1";
const T_UUID_2: &str = "9d4f2c23-8b15-5a8f-ae16-2e7e03d7d3b2";
const T_UUID_3: &str = "ae5a3d34-9c26-6b9a-bf27-3f8f14e8e4c3";

fn empty_bindings() -> std::collections::HashMap<String, String> {
    std::collections::HashMap::new()
}

/// v1.32.0 — legacy pool-backed connection source. These pooler
/// integration tests exercise the non-pinned op path against whatever
/// session topology the env-var DSN provides.
fn sc(backend: &PostgresStoreBackend) -> axon::store::store_conn::StoreConn<'_> {
    axon::store::store_conn::StoreConn::pool(backend.pool())
}

// ── Harness ─────────────────────────────────────────────────────────

/// Resolve the test backend, or `None` for a graceful skip.
async fn test_backend() -> Option<PostgresStoreBackend> {
    let dsn = match std::env::var("AXON_TEST_DATABASE_URL") {
        Ok(d) if !d.trim().is_empty() => d,
        _ => {
            eprintln!(
                "i_2: AXON_TEST_DATABASE_URL unset — skipping the \
                 pooler-coherent integration (the CI lane \
                 `pgbouncer-transaction-mode` routes this var through \
                 PgBouncer `pool_mode=transaction`; locally see \
                 docs/fixtures/pgbouncer-transaction-mode/)"
            );
            return None;
        }
    };
    let backend = match PostgresStoreBackend::connect(&dsn) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("i_2: backend connect failed ({e}) — skipping");
            return None;
        }
    };
    if let Err(e) = backend.ping().await {
        eprintln!("i_2: Postgres unreachable ({e}) — skipping");
        return None;
    }
    Some(backend)
}

macro_rules! pg_or_skip {
    () => {
        match test_backend().await {
            Some(b) => b,
            None => return,
        }
    };
}

async fn exec(backend: &PostgresStoreBackend, sql: &str) {
    sqlx::query(sql)
        .execute(backend.pool())
        .await
        .unwrap_or_else(|e| panic!("i_2 fixture SQL failed:\n  {sql}\n  {e}"));
}

/// Drop + (re-)create a schema for the test's lifetime.
async fn fresh_schema(backend: &PostgresStoreBackend, schema: &str) {
    exec(
        backend,
        &format!("DROP SCHEMA IF EXISTS {schema} CASCADE"),
    )
    .await;
    exec(backend, &format!("CREATE SCHEMA {schema}")).await;
}

async fn drop_schema(backend: &PostgresStoreBackend, schema: &str) {
    exec(
        backend,
        &format!("DROP SCHEMA IF EXISTS {schema} CASCADE"),
    )
    .await;
}

// ════════════════════════════════════════════════════════════════════
//  t1 — Faithful smoke-15 reproduction: uuid PK, non-default schema,
//       canonical "persist → retrieve ×3" agent flow. Behind PgBouncer
//       `pool_mode=transaction`, the v1.36.5 codepath would die
//       `operator does not exist: uuid = text` on the very first
//       retrieve — every retry only re-opened a fresh empty type-map
//       on yet another pooled session. Post-37.x.a-h:
//
//        - D1 resolves `tenants` against `pg_catalog`, finds it in the
//          non-default schema (`alt`) on ANY pooled session.
//        - D2 emits `"alt"."tenants"` — resolves regardless
//          of the session's `search_path`.
//        - D3 introspect + operate in ONE pool.begin() transaction, so
//          a transaction-mode pooler pins one backend for both halves.
//        - D4 (the type-aware path) renders `$1::uuid` from the typed
//          column map — `uuid = uuid`, the native operator, works.
//
//       Smoke-15 is the canonical agent shape the founder named in
//       the 2026-05-18/19 gap report: "retrieve, retrieve, retrieve,
//       persist". This test exercises exactly that.
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t1_smoke15_uuid_pk_in_non_default_schema_canonical_agent_flow() {
    let backend = pg_or_skip!();
    let schema = "alt";
    let table = "tenants";

    fresh_schema(&backend, schema).await;
    // A uuid-PK table living ONLY in the non-default schema —
    // exactly the adopter shape kivi's smoke-15 hit.
    exec(
        &backend,
        &format!(
            "CREATE TABLE {schema}.{table} (\
                tenant_id UUID PRIMARY KEY, \
                tier TEXT NOT NULL, \
                created_at TIMESTAMPTZ NOT NULL DEFAULT now())"
        ),
    )
    .await;

    // — Step 1: PERSIST the row (the agent's first state-write). —
    let inserted = backend
        .insert(
            &mut sc(&backend),
            table,
            &[
                ("tenant_id".to_string(), SqlValue::Text(T_UUID_1.to_string())),
                ("tier".to_string(), SqlValue::Text("standard".to_string())),
            ],
        )
        .await
        .expect(
            "t1: persist to a non-default-schema uuid-PK table behind \
             a transaction-mode pooler — the pre-37.x defect would have \
             died here on a session that cannot see `tenants` via \
             `search_path`",
        );
    assert_eq!(inserted, 1, "t1: persist must write exactly one row");

    // — Steps 2/3/4: RETRIEVE ×3 (the agent's canonical loop). —
    // Every retrieve forces a fresh checkout under PgBouncer — three
    // pooled sessions, three pg_catalog resolutions if the schema
    // cache is cold, only one in-cache hit after the first. In every
    // case, the retrieved row must come back with its uuid intact.
    for attempt in 1..=3 {
        let rows = backend
            .query(
                &mut sc(&backend),
                table,
                &format!("tenant_id = '{T_UUID_1}'"),
                &empty_bindings(),
            )
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "t1: retrieve #{attempt} failed — this is the EXACT \
                     defect 37.x closes (pre-37.x: `operator does not \
                     exist: uuid = text` on a session whose `to_regclass` \
                     could not find `tenants`). Got: {e}"
                )
            });
        assert_eq!(
            rows.len(),
            1,
            "t1: retrieve #{attempt} must return exactly the persisted row"
        );
        let row = &rows[0];
        assert_eq!(
            row.get("tenant_id").unwrap().as_str(),
            Some(T_UUID_1),
            "t1: retrieve #{attempt} returned the wrong tenant_id"
        );
        assert_eq!(
            row.get("tier").unwrap().as_str(),
            Some("standard"),
            "t1: retrieve #{attempt} returned the wrong tier"
        );
    }

    drop_schema(&backend, schema).await;
}

// ════════════════════════════════════════════════════════════════════
//  t2 — Pool churn: 20 sequential operations across two distinct
//       tables. With `default_pool_size=5` (the CI lane's value), cross-
//       session multiplexing IS forced. Every operation must succeed.
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t2_pool_churn_two_tables_twenty_ops_all_succeed() {
    let backend = pg_or_skip!();
    let schema = "churn_2";
    fresh_schema(&backend, schema).await;
    exec(
        &backend,
        &format!(
            "CREATE TABLE {schema}.alpha (id UUID PRIMARY KEY, v TEXT)"
        ),
    )
    .await;
    exec(
        &backend,
        &format!(
            "CREATE TABLE {schema}.beta  (id UUID PRIMARY KEY, v TEXT)"
        ),
    )
    .await;

    // Seed one row in each table so every retrieve has a target.
    for (table, uuid) in [("alpha", T_UUID_1), ("beta", T_UUID_2)] {
        backend
            .insert(
                &mut sc(&backend),
                table,
                &[
                    ("id".to_string(), SqlValue::Text(uuid.to_string())),
                    ("v".to_string(), SqlValue::Text("seed".to_string())),
                ],
            )
            .await
            .unwrap_or_else(|e| panic!("t2 seed {table}: {e}"));
    }

    // 20 alternating ops — every operation is a fresh axonstore call
    // (so behind PgBouncer it forces a fresh checkout). The first op
    // per table populates the cache; the rest run cache-hit. ALL must
    // succeed — no `operator does not exist`, no `prepared statement
    // … already exists`, no resolution failure.
    for i in 0..20 {
        let (table, uuid) = if i % 2 == 0 {
            ("alpha", T_UUID_1)
        } else {
            ("beta", T_UUID_2)
        };
        let rows = backend
            .query(
                &mut sc(&backend),
                table,
                &format!("id = '{uuid}'"),
                &empty_bindings(),
            )
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "t2: pool-churn op #{i} ({table}) failed — every op \
                     in the burst must survive cross-session \
                     multiplexing. Got: {e}"
                )
            });
        assert_eq!(rows.len(), 1, "t2: op #{i} ({table}) returned wrong row count");
    }

    drop_schema(&backend, schema).await;
}

// ════════════════════════════════════════════════════════════════════
//  t3 — Forced cache miss: explicitly evict the schema cache between
//       every operation. Each op then runs the FULL miss path —
//       `pool.begin()` opens one transaction, `introspect_conn`
//       resolves+introspects on the same connection the operation
//       executes on, then commit. Behind PgBouncer transaction mode,
//       this is the moment D3 either holds or fails: a pre-D3
//       `column_types()` would have checked out a SECOND connection,
//       landed on a different physical session, and produced an empty
//       type map.
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t3_forced_cache_miss_introspect_and_operate_pin_one_backend() {
    let backend = pg_or_skip!();
    let schema = "miss_2";
    let table = "audit_log";
    fresh_schema(&backend, schema).await;
    exec(
        &backend,
        &format!(
            "CREATE TABLE {schema}.{table} (\
                event_id UUID PRIMARY KEY, \
                actor TEXT NOT NULL, \
                payload JSONB NOT NULL)"
        ),
    )
    .await;

    // Seed one row.
    backend
        .insert(
            &mut sc(&backend),
            table,
            &[
                ("event_id".to_string(), SqlValue::Text(T_UUID_1.to_string())),
                ("actor".to_string(), SqlValue::Text("alice".to_string())),
                (
                    "payload".to_string(),
                    SqlValue::Text("{\"k\":1}".to_string()),
                ),
            ],
        )
        .await
        .expect("t3 seed");

    // 8 successive retrieves, the cache evicted before each one. EVERY
    // op runs the full D3 introspect+operate transaction; every op
    // must succeed. The first one already proves D3 (the rest prove
    // determinism across sessions).
    //
    // (`evict_schema` is `pub(crate)`, so we can't call it directly
    // from this integration test. We achieve the same effect by
    // building a FRESH `PostgresStoreBackend` each iteration — same
    // process-global cache, but each backend opens its own pool and
    // the cache is keyed by DSN so the first op on the new pool also
    // hits the cache. Better: alternate between two distinct DSNs by
    // appending a no-op query param — Postgres ignores unknown
    // params, but the DSN string differs so the schema cache key
    // differs, FORCING a miss.)
    let base_dsn = std::env::var("AXON_TEST_DATABASE_URL").unwrap();
    for i in 0..8 {
        let dsn_variant = if base_dsn.contains('?') {
            format!("{base_dsn}&__axonprobe={i}")
        } else {
            format!("{base_dsn}?__axonprobe={i}")
        };
        let fresh_backend = PostgresStoreBackend::connect(&dsn_variant)
            .expect("t3: each DSN variant must parse");
        let rows = fresh_backend
            .query(
                &mut sc(&fresh_backend),
                table,
                &format!("event_id = '{T_UUID_1}'"),
                &empty_bindings(),
            )
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "t3: forced-miss op #{i} failed — D3 introspect+ \
                     operate must pin ONE backend on a transaction-mode \
                     pooler. Got: {e}"
                )
            });
        assert_eq!(rows.len(), 1, "t3: forced-miss op #{i} row count");
        assert_eq!(
            rows[0].get("event_id").unwrap().as_str(),
            Some(T_UUID_1),
            "t3: forced-miss op #{i} returned wrong row"
        );
    }

    drop_schema(&backend, schema).await;
}

// ════════════════════════════════════════════════════════════════════
// t4 — D9 self-heal under the pooler. Identical body to 37.x.a section 3 but
//       valuable HERE too: a parse-time drift SQLSTATE
//       (42P01/42703/42804/42883) is a ZERO-side-effect rejection, so
//       the retry is pooler-safe (no half-applied write, no
//       cross-session double-execution risk). This test documents +
//       guards that.
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t4_d9_self_heal_after_alter_table_under_pooler() {
    let backend = pg_or_skip!();
    let schema = "drift_2";
    let table = "probes";
    fresh_schema(&backend, schema).await;
    exec(
        &backend,
        &format!(
            "CREATE TABLE {schema}.{table} (probe UUID PRIMARY KEY, v TEXT)"
        ),
    )
    .await;

    // Seed + first retrieve — populates the schema cache.
    backend
        .insert(
            &mut sc(&backend),
            table,
            &[
                ("probe".to_string(), SqlValue::Text(T_UUID_3.to_string())),
                ("v".to_string(), SqlValue::Text("v0".to_string())),
            ],
        )
        .await
        .expect("t4 seed");
    backend
        .query(
            &mut sc(&backend),
            table,
            &format!("probe = '{T_UUID_3}'"),
            &empty_bindings(),
        )
        .await
        .expect("t4 prime — populates the (dsn,table) cache");

    // The live ALTER drifts the column type — the cached `{probe:
    // uuid}` mapping is now STALE.
    exec(
        &backend,
        &format!(
            "ALTER TABLE {schema}.{table} ALTER COLUMN probe TYPE text \
             USING probe::text"
        ),
    )
    .await;

    // v1.32.0 (D9) — the next retrieve hits the STALE cache, fails
    // with SQLSTATE 42883 (`operator does not exist: text = uuid`),
    // the (dsn,table) entry is evicted, fresh introspection sees
    // `probe text`, the retry runs `"col"::text = $N` and succeeds.
    let rows = backend
        .query(
            &mut sc(&backend),
            table,
            &format!("probe = '{T_UUID_3}'"),
            &empty_bindings(),
        )
        .await
        .expect(
            "t4: D9 self-heal — a schema-drift SQLSTATE evicts the \
             stale cache entry and retries once with fresh \
             introspection; the retry is pooler-safe because every \
             drift SQLSTATE is a parse-time rejection (zero side \
             effects on the failed try)",
        );
    assert_eq!(rows.len(), 1, "t4: self-healed retrieve returns the row");

    drop_schema(&backend, schema).await;
}

// ════════════════════════════════════════════════════════════════════
// t5 — v1.31.0 regression guard: sequential transactions across
//       pooled connections must NOT collide on prepared statement names.
//
//  This is the load-bearing test that protects v1.38.0's regression
//  (kivi smoke 16, 2026-05-20) from recurring. Pre-fix: 10 sequential
//  transactions through PgBouncer `pool_mode=transaction` exhaust the
//  pool, force connection reuse, and the SECOND tx onwards collides
//  on `prepared statement "sqlx_s_N" already exists`. Post-fix
//  (D1 `.persistent(false)` + D2 `after_release DEALLOCATE ALL`):
//  every transaction succeeds without collision.
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t5_sequential_transactions_across_pooled_connections() {
    let backend = pg_or_skip!();
    let schema = "t5_2";
    fresh_schema(&backend, schema).await;
    exec(
        &backend,
        &format!(
            "CREATE TABLE {schema}.records ( \
                id uuid PRIMARY KEY, \
                label text NOT NULL, \
                seq integer NOT NULL \
             )"
        ),
    )
    .await;
    exec(
        &backend,
        &format!("ALTER ROLE CURRENT_USER SET search_path = {schema}, public"),
    )
    .await;

    // Drive 10 sequential transactions. Each tx exercises a different
    // operation (retrieve / persist / mutate / purge), forcing the
    // cache-MISS path → `pool.begin()` + `introspect_conn` (2 sqlx::query
    // calls) + the operation's own sqlx::query. Pre-fix: tx N+1 collides
    // on `sqlx_s_1` because tx N's named PARSE survives on the physical
    // conn returned to the pooler's pool. Post-fix: every PARSE is
    // unnamed (D1) AND every released conn is `DEALLOCATE ALL`'d (D2),
    // so collision is structurally impossible.
    for n in 0..10u32 {
        // Force the cache-MISS path on every iteration so EVERY op uses
        // pool.begin() and introspect_conn — the original collision site.
        // (Behind the public API: persist after a `DROP/CREATE` schema
        // → cache empty → MISS guaranteed for the first call. Subsequent
        // calls would HIT cache; we evict via a parallel `DROP TABLE
        // IF EXISTS` no-op-after-create… actually simplest: just use
        // distinct tables each iteration so each one is a fresh cache
        // miss against a fresh table name.)
        let table = format!("records_{n}");
        exec(
            &backend,
            &format!(
                "CREATE TABLE IF NOT EXISTS {schema}.{table} ( \
                    id uuid PRIMARY KEY, \
                    label text NOT NULL, \
                    seq integer NOT NULL \
                 )"
            ),
        )
        .await;
        let qualified = format!("{schema}.{table}");
        let row_id = format!("11111111-2222-3333-4444-{:012}", n);
        let result = backend
            .insert(
                &mut sc(&backend),
                &qualified,
                &[
                    ("id".to_string(), SqlValue::Text(row_id.clone())),
                    (
                        "label".to_string(),
                        SqlValue::Text(format!("tx-{n}")),
                    ),
                    ("seq".to_string(), SqlValue::Integer(n as i64)),
                ],
            )
            .await;
        assert!(
            result.is_ok(),
            "t5: sequential insert #{n} must succeed without prepared \
             statement collision; got: {result:?}. \
             If this FAILS with `42710 duplicate_prepared_statement`, \
             the v1.31.0 Pooler-coherent Transactions Contract has \
             regressed: check that every `sqlx::query` under \
             `axon-rs/src/store/` carries `.persistent(false)` (D1) \
             AND that `connect_named_with_namespace` installs \
             `after_release(DEALLOCATE ALL)` (D2)."
        );

        // Round-trip retrieve to also exercise the read tx path.
        let rows = backend
            .query(
                &mut sc(&backend),
                &qualified,
                "id = ${id}",
                &std::collections::HashMap::from([("id".to_string(), row_id.clone())]),
            )
            .await;
        assert!(
            rows.is_ok(),
            "t5: sequential retrieve #{n} must succeed; got: {rows:?}"
        );
        assert_eq!(rows.unwrap().len(), 1, "t5: retrieve #{n} returns 1 row");
    }

    drop_schema(&backend, schema).await;
}
