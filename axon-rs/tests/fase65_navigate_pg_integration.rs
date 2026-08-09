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
//! §Fase 119.i.2 — **no longer single-threaded.** This paragraph used to require
//! `-- --test-threads=1`, because the four tests shared fixture tables. They now
//! carry per-test names (see below), so the suite is correct under cargo's
//! default parallelism and needs no flag. Run it with:
//!
//! ```text
//! cargo test --features postgres --test fase65_navigate_pg_integration
//! ```
//!
//! with `AXON_TEST_DATABASE_URL` pointing at a THROWAWAY Postgres.
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

// §Fase 119.i.2 — the table + role names are PER TEST, not shared constants.
//
// **This was not a latent bug.** The names used to be `const DOC_STORE =
// "fase65_ltm_summaries"` etc., shared by all four tests, and the module header
// above said so plainly: *run single-threaded, the suite shares fixture tables
// by design*. Under `--test-threads=1` it was correct.
//
// What it was, is a REQUIREMENT NOBODY COULD SEE. The suite sits behind two
// stacked doors — `#![cfg(feature = "postgres")]` deletes the file without
// `--features postgres`, and `AXON_TEST_DATABASE_URL` skips every test when
// unset — so `cargo test` reported `ok. 0 passed` and no one ever met the
// precondition. Run the obvious way (`--features postgres` + a DSN, no flag),
// three of the four die with `relation "fase65_ltm_summaries" already exists`:
// one test drops a table another just created. A gate whose correctness depends
// on a flag the default invocation omits will be met exactly as often as someone
// reads the header.
//
// So the fix removes the precondition instead of documenting it harder. Distinct
// names per test make the suite correct under cargo's DEFAULT parallelism; the
// ROLE is namespaced too, because `DO $$ IF NOT EXISTS … CREATE ROLE $$` is
// itself a check-then-act race between concurrent sessions.
//
// A mutex would have been the wrong repair: the tests would still share one
// table, and `t4` — the load-bearing concurrent-tenant leak test — would be
// asserting isolation over state its siblings mutate.
fn doc_store(ns: &str) -> String {
    format!("fase65_{ns}_ltm_summaries")
}
fn edge_store(ns: &str) -> String {
    format!("fase65_{ns}_ltm_edges")
}
fn app_role(ns: &str) -> String {
    format!("fase65_{ns}_app")
}
const CORPUS: &str = "LtmGraph";

// ── Harness ─────────────────────────────────────────────────────────

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
        "fase65: {why} — and this suite REFUSES to skip.\n\n\
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
        .unwrap_or_else(|e| panic!("fase65 fixture SQL failed:\n  {sql}\n  {e}"));
}

/// Seed both stores from scratch for a fresh test, with RLS FORCED after the
/// rows land. `docs` = (tenant, id, summary); `edges` = (tenant, from, to,
/// etype, weight). All tenants' rows live in the same physical tables —
/// isolation is RLS, not separate tables.
async fn seed(
    backend: &PostgresStoreBackend,
    ns: &str,
    docs: &[(&str, &str, &str)],
    edges: &[(&str, &str, &str, &str, f64)],
) {
    let (doc, edge) = (doc_store(ns), edge_store(ns));
    // Fresh tables, RLS off while seeding.
    exec(backend, &format!("DROP TABLE IF EXISTS {doc}")).await;
    exec(backend, &format!("DROP TABLE IF EXISTS {edge}")).await;
    exec(
        backend,
        &format!(
            "CREATE TABLE {doc} \
             (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, summary TEXT NOT NULL)"
        ),
    )
    .await;
    exec(
        backend,
        &format!(
            "CREATE TABLE {edge} \
             (from_id TEXT NOT NULL, to_id TEXT NOT NULL, tenant_id TEXT NOT NULL, \
              etype TEXT NOT NULL, weight DOUBLE PRECISION NOT NULL)"
        ),
    )
    .await;

    for (tenant, id, summary) in docs {
        exec(
            backend,
            &format!(
                "INSERT INTO {doc} (id, tenant_id, summary) \
                 VALUES ('{id}', '{tenant}', '{summary}')"
            ),
        )
        .await;
    }
    for (tenant, from, to, etype, weight) in edges {
        exec(
            backend,
            &format!(
                "INSERT INTO {edge} (from_id, to_id, tenant_id, etype, weight) \
                 VALUES ('{from}', '{to}', '{tenant}', '{etype}', {weight})"
            ),
        )
        .await;
    }

    // Enable + FORCE RLS, scope each table by the §40 GUC. FORCE makes the
    // owner subject to the policy too — without it the isolation is untested.
    for table in [&doc, &edge] {
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
    // §Fase 119.i.2 — the role is namespaced too. `IF NOT EXISTS … CREATE ROLE`
    // is check-then-act: two concurrent sessions can both see it missing and
    // both try to create it, and the loser gets `role already exists`.
    //
    // Namespacing the role is NOT sufficient, and this is the part that only a
    // repeated run under real concurrency surfaces: `GRANT USAGE ON SCHEMA
    // public` writes the ACL of ONE catalog row — `pg_namespace` for `public` —
    // no matter which role it grants to. Four tests granting concurrently hit
    // `ERROR: tuple concurrently updated`, roughly one run in three. The shared
    // name that remained was never a table; it was the SCHEMA.
    //
    // So the lock covers exactly the shared catalog object and nothing else. The
    // per-test tables, the seeding, the RLS policies and every read stay fully
    // concurrent — which matters, because `t4` exists to prove two tenants can
    // navigate AT THE SAME TIME without leaking. Serialising the whole test
    // would have made that assertion vacuous while turning the suite green: the
    // §119.i trap of hiding a defect by removing the concurrency that reveals it.
    {
        static CATALOG_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _guard = CATALOG_LOCK.lock().await;
        let role = app_role(ns);
        exec(
            backend,
            &format!(
                "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '{role}') \
                 THEN CREATE ROLE {role} NOLOGIN NOBYPASSRLS; END IF; END $$;"
            ),
        )
        .await;
        exec(backend, &format!("GRANT USAGE ON SCHEMA public TO {role}")).await;
    }

    // Per-table grants touch only THIS test's tables — no shared row, no lock.
    let role = app_role(ns);
    for table in [&doc, &edge] {
        exec(backend, &format!("GRANT SELECT ON {table} TO {role}")).await;
    }
}

async fn drop_all(backend: &PostgresStoreBackend, ns: &str) {
    exec(
        backend,
        &format!("DROP TABLE IF EXISTS {}", doc_store(ns)),
    )
    .await;
    exec(
        backend,
        &format!("DROP TABLE IF EXISTS {}", edge_store(ns)),
    )
    .await;
}

fn corpus_source(ns: &str) -> IRCorpusStoreSource {
    IRCorpusStoreSource {
        doc_store: doc_store(ns),
        doc_id: "id".to_string(),
        doc_title: "summary".to_string(),
        edge_store: edge_store(ns),
        edge_from: "from_id".to_string(),
        edge_to: "to_id".to_string(),
        edge_type: "etype".to_string(),
        edge_weight: "weight".to_string(),
    }
}

fn store_specs(ns: &str) -> Vec<IRAxonStore> {
    [doc_store(ns), edge_store(ns)]
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
        depth: None,
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
    ns: &str,
) -> (
    DispatchCtx,
    tokio::sync::mpsc::UnboundedReceiver<axon::flow_execution_event::FlowExecutionEvent>,
) {
    // §Fase 118.a / D118.2 — keyed on the PORT, not the driver type.
    let pins: Arc<Mutex<HashMap<String, axon::pinned_conn::PinnedConn>>> =
        Arc::new(Mutex::new(HashMap::new()));
    for store in [doc_store(ns), edge_store(ns)] {
        let store = store.as_str();
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
            conn.execute(sqlx::query(&format!("SET ROLE {}", app_role(ns))))
                .await
                .expect("set rls role");
        }
        pins.lock().unwrap().insert(store.to_string(), pin);
    }

    let mut sources: HashMap<String, IRCorpusStoreSource> = HashMap::new();
    sources.insert(CORPUS.to_string(), corpus_source(ns));

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let ctx = DispatchCtx::new("RecallLTM", "kimi", "", CancellationFlag::new(), tx)
        .with_store_registry(registry.clone())
        .with_mdn_corpora(Arc::new(HashMap::new()))
        .with_mdn_adaptive(Arc::new(HashSet::new()))
        .with_mdn_store_sources(Arc::new(sources))
        .with_pinned_conns(pins);
    (ctx, rx)
}

async fn navigate_for(registry: &Arc<StoreRegistry>, tenant: &str, ns: &str) -> String {
    // Keep `_rx` alive across the navigate: the handler emits step events on the
    // channel, and a dropped receiver closes it → `ChannelClosed`. (The §65.A/B
    // bridge keeps its own `_rx` binding alive in `dispatch_structural` exactly
    // for this reason.)
    let (mut ctx, _rx) = ctx_for_tenant(registry, tenant, ns).await;
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
    const NS: &str = "t1";
    let backend = test_backend().await;
    seed(
        &backend,
        NS,
        &[
            ("acme", "a1", "ACME consultative selling playbook"),
            ("acme", "a2", "ACME discovery-call methodology"),
        ],
        &[("acme", "a1", "a2", "elaborate", 0.9)],
    )
    .await;
    let registry = Arc::new(StoreRegistry::build(&store_specs(NS)).expect("registry"));

    let out = navigate_for(&registry, "acme", NS).await;

    assert!(out.contains("ACME"), "real seeded summaries returned: {out:?}");
    // Every line is one of the two REAL summaries — nothing invented.
    for line in out.lines().filter(|l| !l.is_empty()) {
        assert!(
            line.contains("ACME consultative selling playbook")
                || line.contains("ACME discovery-call methodology"),
            "line is a real seeded doc, not fabricated: {line:?}"
        );
    }
    drop_all(&backend, NS).await;
}

/// §Fase 65.A — the anti-hallucination guarantee against a REAL empty corpus:
/// zero rows visible to the tenant ⇒ empty result, never invented hits.
#[tokio::test]
async fn t2_empty_corpus_binds_empty_not_hallucinated() {
    const NS: &str = "t2";
    let backend = test_backend().await;
    // Seed ONLY for `other`; tenant `ghost` sees nothing under RLS.
    seed(
        &backend,
        NS,
        &[("other", "o1", "OTHER tenant private note")],
        &[],
    )
    .await;
    let registry = Arc::new(StoreRegistry::build(&store_specs(NS)).expect("registry"));

    let out = navigate_for(&registry, "ghost", NS).await;

    assert_eq!(out, "", "an empty (RLS-scoped) corpus binds empty, never fabricates");
    drop_all(&backend, NS).await;
}

/// §Fase 64.B / §65.A — tenant isolation: `navigate` reads with an EMPTY where
/// clause and relies on RLS. Scoped to A it must return ONLY A's rows; scoped
/// to B only B's — no cross-tenant leak — even sequentially over the same
/// shared tables.
#[tokio::test]
async fn t3_navigate_is_tenant_scoped_sequentially() {
    const NS: &str = "t3";
    let backend = test_backend().await;
    seed(
        &backend,
        NS,
        &[
            ("alpha", "x1", "ALPHA secret revenue figures"),
            ("beta", "y1", "BETA confidential roadmap"),
        ],
        &[],
    )
    .await;
    let registry = Arc::new(StoreRegistry::build(&store_specs(NS)).expect("registry"));

    let a = navigate_for(&registry, "alpha", NS).await;
    assert!(a.contains("ALPHA"), "alpha sees its own row: {a:?}");
    assert!(!a.contains("BETA"), "alpha must NOT see beta's row: {a:?}");

    let b = navigate_for(&registry, "beta", NS).await;
    assert!(b.contains("BETA"), "beta sees its own row: {b:?}");
    assert!(!b.contains("ALPHA"), "beta must NOT see alpha's row: {b:?}");

    drop_all(&backend, NS).await;
}

/// §Fase 65.A/B — THE load-bearing test: two tenants navigating CONCURRENTLY,
/// each on its own pinned, GUC-scoped connection, must never see each other's
/// rows. This is the cross-tenant leak the risk matrix flagged as severity
/// HIGH — a fresh pool acquire or a mis-shared GUC would surface here.
#[tokio::test]
async fn t4_navigate_concurrent_two_tenants_no_leak() {
    const NS: &str = "t4";
    let backend = test_backend().await;
    seed(
        &backend,
        NS,
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
    let registry = Arc::new(StoreRegistry::build(&store_specs(NS)).expect("registry"));

    // Drive both concurrently — each gets its own GUC-scoped pinned conns.
    let (a, b) = tokio::join!(
        navigate_for(&registry, "alpha", NS),
        navigate_for(&registry, "beta", NS),
    );

    assert!(a.contains("ALPHA"), "alpha sees its own rows: {a:?}");
    assert!(!a.contains("BETA"), "NO leak: alpha never sees beta: {a:?}");
    assert!(b.contains("BETA"), "beta sees its own rows: {b:?}");
    assert!(!b.contains("ALPHA"), "NO leak: beta never sees alpha: {b:?}");

    drop_all(&backend, NS).await;
}
