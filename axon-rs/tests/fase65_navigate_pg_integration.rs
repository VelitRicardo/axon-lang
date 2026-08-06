//! §Fase 118.b.3 — this suite exercises the SQL data plane (the Postgres
//! backend, the pinned-connection contract, live introspection), which lives
//! behind the `postgres` feature. Gated at the file level: the core suites keep
//! running in a build with no driver.
#![cfg(feature = "postgres")]

//! §Fase 65.A/B — Real-Postgres integration for the STRUCTURAL `navigate`
//! bridge: tenant isolation + real-rows→real-hits, against a real database.
//!
//! This is the load-bearing safeguard the §65 plan flagged. The §65.A/B bridge
//! routes a non-streaming `navigate <corpus from axonstore>` through the
//! dispatcher's real MDN handler (`cognitive::run_navigate`), which reads the
//! backing stores with an EMPTY `where` clause and relies **entirely on RLS**
//! (the §40 GUC `axon.current_tenant` on the flow's pinned connection) to scope
//! the rows. These tests prove that property end-to-end:
//!
//!   * `navigate` over real rows returns the REAL documents (no fabrication,
//!     unlike the legacy LLM fallthrough);
//!   * `navigate` scoped to tenant A returns ONLY tenant A's rows — even though
//!     the read is an unfiltered full scan — because the pinned connection
//!     carries A's RLS GUC;
//!   * two tenants navigating CONCURRENTLY (each on its own pinned, GUC-scoped
//!     connection) never see each other's rows (the §65.A/B risk-matrix
//!     load-bearing test).
//!
//! # Harness
//!
//! Connects via `AXON_TEST_DATABASE_URL`; when unset or unreachable every test
//! skips gracefully (mirrors `fase35_l_postgres_integration`). CI provides a
//! `postgres:16` service container.
//!
//! # RLS fidelity
//!
//! NOTE: run single-threaded — `cargo test --test fase65_navigate_pg_integration
//! -- --test-threads=1`. The suite shares fixture tables + the RLS role by design
//! (it models the SAME corpus seen by DIFFERENT tenants), so parallel DROP/CREATE
//! fixtures would race. The CI lane passes the flag.
//!
//! The fixture tables enable **and FORCE** row-level security (so the table
//! OWNER — the role the tests run as — is ALSO subject to the policy; without
//! FORCE, owners bypass RLS and the isolation claim would be untested). Rows are
//! seeded BEFORE RLS is forced, so the seed inserts are not themselves filtered.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::cognitive::run_navigate;
use axon::flow_dispatcher::{DispatchCtx, NodeOutcome};
use axon::ir_nodes::{IRAxonStore, IRCorpusStoreSource, IRNavigateStep};
use axon::store::postgres_backend::PostgresStoreBackend;
use axon::store::registry::{StoreHandle, StoreRegistry};

const DOC_STORE: &str = "fase65_ltm_summaries";
const EDGE_STORE: &str = "fase65_ltm_edges";
const CORPUS: &str = "LtmGraph";

// ── Harness ─────────────────────────────────────────────────────────

async fn test_backend() -> Option<PostgresStoreBackend> {
    let dsn = match std::env::var("AXON_TEST_DATABASE_URL") {
        Ok(d) if !d.trim().is_empty() => d,
        _ => {
            eprintln!(
                "fase65: AXON_TEST_DATABASE_URL unset — skipping real-Postgres \
                 navigate integration (set it to run; CI always does)"
            );
            return None;
        }
    };
    let backend = match PostgresStoreBackend::connect(&dsn) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("fase65: backend connect failed ({e}) — skipping");
            return None;
        }
    };
    if let Err(e) = backend.ping().await {
        eprintln!("fase65: Postgres unreachable ({e}) — skipping");
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
        .unwrap_or_else(|e| panic!("fase65 fixture SQL failed:\n  {sql}\n  {e}"));
}

/// Seed both stores from scratch for a fresh test, with RLS FORCED after the
/// rows land. `docs` = (tenant, id, summary); `edges` = (tenant, from, to,
/// etype, weight). All tenants' rows live in the same physical tables —
/// isolation is RLS, not separate tables.
async fn seed(
    backend: &PostgresStoreBackend,
    docs: &[(&str, &str, &str)],
    edges: &[(&str, &str, &str, &str, f64)],
) {
    // Fresh tables, RLS off while seeding.
    exec(backend, &format!("DROP TABLE IF EXISTS {DOC_STORE}")).await;
    exec(backend, &format!("DROP TABLE IF EXISTS {EDGE_STORE}")).await;
    exec(
        backend,
        &format!(
            "CREATE TABLE {DOC_STORE} \
             (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, summary TEXT NOT NULL)"
        ),
    )
    .await;
    exec(
        backend,
        &format!(
            "CREATE TABLE {EDGE_STORE} \
             (from_id TEXT NOT NULL, to_id TEXT NOT NULL, tenant_id TEXT NOT NULL, \
              etype TEXT NOT NULL, weight DOUBLE PRECISION NOT NULL)"
        ),
    )
    .await;

    for (tenant, id, summary) in docs {
        exec(
            backend,
            &format!(
                "INSERT INTO {DOC_STORE} (id, tenant_id, summary) \
                 VALUES ('{id}', '{tenant}', '{summary}')"
            ),
        )
        .await;
    }
    for (tenant, from, to, etype, weight) in edges {
        exec(
            backend,
            &format!(
                "INSERT INTO {EDGE_STORE} (from_id, to_id, tenant_id, etype, weight) \
                 VALUES ('{from}', '{to}', '{tenant}', '{etype}', {weight})"
            ),
        )
        .await;
    }

    // Enable + FORCE RLS, scope each table by the §40 GUC. FORCE makes the
    // owner subject to the policy too — without it the isolation is untested.
    for table in [DOC_STORE, EDGE_STORE] {
        exec(backend, &format!("ALTER TABLE {table} ENABLE ROW LEVEL SECURITY")).await;
        exec(backend, &format!("ALTER TABLE {table} FORCE ROW LEVEL SECURITY")).await;
        exec(backend, &format!("DROP POLICY IF EXISTS {table}_rls ON {table}")).await;
        exec(
            backend,
            &format!(
                "CREATE POLICY {table}_rls ON {table} USING \
                 (tenant_id = current_setting('axon.current_tenant', true))"
            ),
        )
        .await;
    }

    // A NON-superuser, NOBYPASSRLS role for the read path. The default
    // `postgres`-image login role is a SUPERUSER, which BYPASSES RLS entirely
    // (even with FORCE) — so the reads must `SET ROLE` to this role for the
    // policy to actually apply. This models production, where the runtime
    // connects as an unprivileged, RLS-bound application role.
    exec(
        backend,
        "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'fase65_app') \
         THEN CREATE ROLE fase65_app NOLOGIN NOBYPASSRLS; END IF; END $$;",
    )
    .await;
    exec(backend, "GRANT USAGE ON SCHEMA public TO fase65_app").await;
    for table in [DOC_STORE, EDGE_STORE] {
        exec(backend, &format!("GRANT SELECT ON {table} TO fase65_app")).await;
    }
}

async fn drop_all(backend: &PostgresStoreBackend) {
    exec(backend, &format!("DROP TABLE IF EXISTS {DOC_STORE}")).await;
    exec(backend, &format!("DROP TABLE IF EXISTS {EDGE_STORE}")).await;
}

fn corpus_source() -> IRCorpusStoreSource {
    IRCorpusStoreSource {
        doc_store: DOC_STORE.to_string(),
        doc_id: "id".to_string(),
        doc_title: "summary".to_string(),
        edge_store: EDGE_STORE.to_string(),
        edge_from: "from_id".to_string(),
        edge_to: "to_id".to_string(),
        edge_type: "etype".to_string(),
        edge_weight: "weight".to_string(),
    }
}

fn store_specs() -> Vec<IRAxonStore> {
    [DOC_STORE, EDGE_STORE]
        .iter()
        .map(|name| IRAxonStore {
            node_type: "axonstore",
            source_line: 0,
            source_column: 0,
            name: name.to_string(),
            backend: "postgresql".to_string(),
            connection: "env:AXON_TEST_DATABASE_URL".to_string(),
            confidence_floor: None,
            isolation: String::new(),
            on_breach: String::new(),
            capability: String::new(),
            class: String::new(),
            column_schema: None,
            resource_ref: String::new(),
        })
        .collect()
}

fn navigate_node() -> IRNavigateStep {
    IRNavigateStep {
        node_type: "navigate",
        source_line: 0,
        source_column: 0,
        pix_ref: CORPUS.to_string(),
        corpus_ref: CORPUS.to_string(),
        query: "recall".to_string(),
        trail_enabled: false,
        output_name: "hits".to_string(),
        seed: String::new(),
        budget: Some(5),
        where_expr: String::new(),
    }
}

/// Build a `DispatchCtx` whose pinned connections are scoped to `tenant` via
/// the RLS GUC — exactly what the flow's pinned, tenant-scoped connection
/// carries in production. This is the precise condition the §65.A/B bridge
/// reproduces: `navigate`'s reads run over THESE pinned connections.
async fn ctx_for_tenant(
    registry: &Arc<StoreRegistry>,
    tenant: &str,
) -> (
    DispatchCtx,
    tokio::sync::mpsc::UnboundedReceiver<axon::flow_execution_event::FlowExecutionEvent>,
) {
    // §Fase 118.a / D118.2 — keyed on the PORT, not the driver type.
    let pins: Arc<Mutex<HashMap<String, axon::pinned_conn::PinnedConn>>> =
        Arc::new(Mutex::new(HashMap::new()));
    for store in [DOC_STORE, EDGE_STORE] {
        let backend = match registry.resolve(store).expect("resolve store") {
            StoreHandle::Postgres(b) => b,
            StoreHandle::InMemory | StoreHandle::Secrets { .. } => {
                panic!("expected a postgresql store for {store}")
            }
        };
        let mut pin = backend.acquire_pin().await.expect("acquire pin");
        // Set the §40 tenant GUC (as the privileged owner), THEN drop to the
        // unprivileged RLS-bound role so the policy actually filters the reads.
        //
        // §Fase 118.a / D118.2 — both statements go through the port's
        // `as_store_conn()` rather than `&mut *pin`. This is the case that
        // proves the port did not lose anything that mattered: these are
        // SESSION-level statements, and they are only correct because the pin is
        // a stable session — run them on the pool and the GUC lands on a
        // connection the later reads may never see again. `StoreConn` re-borrows
        // per query, so both run on the same physical backend and the pin is
        // still owned afterwards.
        {
            let mut conn = pin.as_store_conn();
            conn.execute(
                sqlx::query("SELECT set_config('axon.current_tenant', $1, false)").bind(tenant),
            )
            .await
            .expect("set tenant GUC");
            conn.execute(sqlx::query("SET ROLE fase65_app"))
                .await
                .expect("set rls role");
        }
        pins.lock().unwrap().insert(store.to_string(), pin);
    }

    let mut sources: HashMap<String, IRCorpusStoreSource> = HashMap::new();
    sources.insert(CORPUS.to_string(), corpus_source());

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let ctx = DispatchCtx::new("RecallLTM", "kimi", "", CancellationFlag::new(), tx)
        .with_store_registry(registry.clone())
        .with_mdn_corpora(Arc::new(HashMap::new()))
        .with_mdn_adaptive(Arc::new(HashSet::new()))
        .with_mdn_store_sources(Arc::new(sources))
        .with_pinned_conns(pins);
    (ctx, rx)
}

async fn navigate_for(registry: &Arc<StoreRegistry>, tenant: &str) -> String {
    // Keep `_rx` alive across the navigate: the handler emits step events on the
    // channel, and a dropped receiver closes it → `ChannelClosed`. (The §65.A/B
    // bridge keeps its own `_rx` binding alive in `dispatch_structural` exactly
    // for this reason.)
    let (mut ctx, _rx) = ctx_for_tenant(registry, tenant).await;
    match run_navigate(&navigate_node(), &mut ctx).await.expect("navigate ok") {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("expected Completed, got {other:?}"),
    }
}

// ════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════

/// §Fase 65.A — `navigate` returns the REAL documents from real rows, ordered
/// by the MDN traversal — never fabricated. The opposite of the legacy LLM
/// fallthrough that invented ids/titles.
#[tokio::test]
async fn t1_navigate_returns_real_hits_from_real_rows() {
    let backend = pg_or_skip!();
    seed(
        &backend,
        &[
            ("acme", "a1", "ACME consultative selling playbook"),
            ("acme", "a2", "ACME discovery-call methodology"),
        ],
        &[("acme", "a1", "a2", "elaborate", 0.9)],
    )
    .await;
    let registry = Arc::new(StoreRegistry::build(&store_specs()).expect("registry"));

    let out = navigate_for(&registry, "acme").await;

    assert!(out.contains("ACME"), "real seeded summaries returned: {out:?}");
    // Every line is one of the two REAL summaries — nothing invented.
    for line in out.lines().filter(|l| !l.is_empty()) {
        assert!(
            line.contains("ACME consultative selling playbook")
                || line.contains("ACME discovery-call methodology"),
            "line is a real seeded doc, not fabricated: {line:?}"
        );
    }
    drop_all(&backend).await;
}

/// §Fase 65.A — the anti-hallucination guarantee against a REAL empty corpus:
/// zero rows visible to the tenant ⇒ empty result, never invented hits.
#[tokio::test]
async fn t2_empty_corpus_binds_empty_not_hallucinated() {
    let backend = pg_or_skip!();
    // Seed ONLY for `other`; tenant `ghost` sees nothing under RLS.
    seed(
        &backend,
        &[("other", "o1", "OTHER tenant private note")],
        &[],
    )
    .await;
    let registry = Arc::new(StoreRegistry::build(&store_specs()).expect("registry"));

    let out = navigate_for(&registry, "ghost").await;

    assert_eq!(out, "", "an empty (RLS-scoped) corpus binds empty, never fabricates");
    drop_all(&backend).await;
}

/// §Fase 64.B / §65.A — tenant isolation: `navigate` reads with an EMPTY where
/// clause and relies on RLS. Scoped to A it must return ONLY A's rows; scoped
/// to B only B's — no cross-tenant leak — even sequentially over the same
/// shared tables.
#[tokio::test]
async fn t3_navigate_is_tenant_scoped_sequentially() {
    let backend = pg_or_skip!();
    seed(
        &backend,
        &[
            ("alpha", "x1", "ALPHA secret revenue figures"),
            ("beta", "y1", "BETA confidential roadmap"),
        ],
        &[],
    )
    .await;
    let registry = Arc::new(StoreRegistry::build(&store_specs()).expect("registry"));

    let a = navigate_for(&registry, "alpha").await;
    assert!(a.contains("ALPHA"), "alpha sees its own row: {a:?}");
    assert!(!a.contains("BETA"), "alpha must NOT see beta's row: {a:?}");

    let b = navigate_for(&registry, "beta").await;
    assert!(b.contains("BETA"), "beta sees its own row: {b:?}");
    assert!(!b.contains("ALPHA"), "beta must NOT see alpha's row: {b:?}");

    drop_all(&backend).await;
}

/// §Fase 65.A/B — THE load-bearing test: two tenants navigating CONCURRENTLY,
/// each on its own pinned, GUC-scoped connection, must never see each other's
/// rows. This is the cross-tenant leak the risk matrix flagged as severity
/// HIGH — a fresh pool acquire or a mis-shared GUC would surface here.
#[tokio::test]
async fn t4_navigate_concurrent_two_tenants_no_leak() {
    let backend = pg_or_skip!();
    seed(
        &backend,
        &[
            ("alpha", "x1", "ALPHA secret revenue figures"),
            ("alpha", "x2", "ALPHA growth strategy"),
            ("beta", "y1", "BETA confidential roadmap"),
            ("beta", "y2", "BETA pricing model"),
        ],
        &[
            ("alpha", "x1", "x2", "elaborate", 0.9),
            ("beta", "y1", "y2", "elaborate", 0.9),
        ],
    )
    .await;
    let registry = Arc::new(StoreRegistry::build(&store_specs()).expect("registry"));

    // Drive both concurrently — each gets its own GUC-scoped pinned conns.
    let (a, b) = tokio::join!(
        navigate_for(&registry, "alpha"),
        navigate_for(&registry, "beta"),
    );

    assert!(a.contains("ALPHA"), "alpha sees its own rows: {a:?}");
    assert!(!a.contains("BETA"), "NO leak: alpha never sees beta: {a:?}");
    assert!(b.contains("BETA"), "beta sees its own rows: {b:?}");
    assert!(!b.contains("ALPHA"), "NO leak: beta never sees alpha: {b:?}");

    drop_all(&backend).await;
}
