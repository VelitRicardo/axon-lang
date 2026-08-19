//! v2.81.0 — this suite exercises the SQL data plane (the Postgres
//! backend, the pinned-connection contract, live introspection), which lives
//! behind the `postgres` feature. Gated at the file level: the core suites keep
//! running in a build with no driver.
#![cfg(feature = "postgres")]

//! v2.17.0 (Q2) — Real-Postgres integration: `navigate { … where: "<col
//! filter>" }` scopes a `corpus from axonstore` to a SUB-TENANT column, so an
//! adopter multiplexing N end-clients in ONE axon-tenant via a `tenant_id`
//! column does not leak memories across clients.
//!
//! The kivi brief #27 C blocker: RLS scopes by axon-tenant, but kivi packs N
//! clients into the single `kivi` axon-tenant and distinguishes them by a
//! `tenant_id` UUID column. A `navigate` with no column filter sources EVERY
//! client's rows → a cross-client leak that blocks wiring recall to the chat.
//!
//! This test seeds two sub-tenants' rows in the SAME tables (one axon-tenant,
//! no RLS — the column filter is the unit under test) and proves:
//!   * `where: "tenant_id == '${tenant_id}'"` bound to sub-tenant A surfaces
//!     ONLY A's document — B's is absent (the isolation claim);
//!   * the SAME navigate with NO `where:` surfaces BOTH (the negative control —
//!     proving the filter, not some other scoping, is what isolates).
//!
//! # Harness
//!
//! Connects via `AXON_TEST_DATABASE_URL`; skips gracefully when unset/unreachable
//! (mirrors `navigate_pg_integration`). CI provides `postgres:16`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::cognitive::run_navigate;
use axon::flow_dispatcher::{DispatchCtx, NodeOutcome};
use axon::ir_nodes::{IRAxonStore, IRCorpusStoreSource, IRNavigateStep};
use axon::store::postgres_backend::PostgresStoreBackend;
use axon::store::registry::{StoreHandle, StoreRegistry};

const DOC_STORE: &str = "ltm_docs";
const EDGE_STORE: &str = "ltm_edges";
const CORPUS: &str = "LtmGraph";
const TENANT_A: &str = "11111111-1111-1111-1111-111111111111";
const TENANT_B: &str = "22222222-2222-2222-2222-222222222222";

/// v2.83.0 — this suite REFUSES to skip.
///
/// It used to return `None` when `AXON_TEST_DATABASE_URL` was unset, and every
/// test returned early. Combined with the file-level `#![cfg(feature = "postgres")]`
/// that is TWO stacked doors, both shut by default: `cargo test` printed
/// `ok. 0 passed` and the suite had not executed a line in months. That is the
/// exact shape in which the v2.17.0 cross-tenant isolation bug survived — a green
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
        "v2.17.0: {why} — and this suite REFUSES to skip.\n\n\
         You asked for the Postgres suite (`--features postgres`). A silent skip \
         here is how a suite reports `ok` for months without executing: it is the \
         shape in which the v2.17.0 cross-tenant isolation bug survived.\n\n\
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
        .unwrap_or_else(|e| panic!("v2.17.0 fixture SQL failed:\n  {sql}\n  {e}"));
}

/// Two sub-tenants' rows in the SAME tables, no RLS. The `tenant_id` column is
/// the only discriminator — exactly kivi's multiplexed shape.
async fn seed(backend: &PostgresStoreBackend) {
    exec(backend, &format!("DROP TABLE IF EXISTS {DOC_STORE}")).await;
    exec(backend, &format!("DROP TABLE IF EXISTS {EDGE_STORE}")).await;
    exec(
        backend,
        &format!(
            "CREATE TABLE {DOC_STORE} (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, summary TEXT NOT NULL)"
        ),
    )
    .await;
    exec(
        backend,
        &format!(
            "CREATE TABLE {EDGE_STORE} (from_id TEXT, to_id TEXT, tenant_id TEXT NOT NULL, etype TEXT, weight DOUBLE PRECISION)"
        ),
    )
    .await;
    exec(
        backend,
        &format!(
            // v2.81.0 — the summaries carry DISTINCT topics on purpose.
            //
            // `navigate` ranks the frontier by `LexicalGain`, whose marginal
            // gain is "query tokens this candidate's title has that the already-
            // selected docs do NOT cover". With the old fixture — both summaries
            // `"<CLIENT> client memory"` and the query `"recall"` — no title
            // shared a token with the query, so the gain was 0, fell below
            // `epsilon`, and the walk stopped dead at its seed. Identical titles
            // would have failed the same way: BRAVO's marginal gain over ALPHA
            // would be 0 because every token was already covered.
            //
            // Distinct topics + a query spanning both is the only fixture in
            // which the walk CAN cross from one client to the other — which is
            // precisely the crossing section 1 must prove the `where:` filter prevents.
            "INSERT INTO {DOC_STORE} (id, tenant_id, summary) VALUES \
             ('a1','{TENANT_A}','ALPHA client onboarding notes'), \
             ('b1','{TENANT_B}','BRAVO client billing dispute')"
        ),
    )
    .await;
    // v2.81.0 — THE EDGE THIS FIXTURE WAS MISSING, and without which the
    // whole suite proved nothing.
    //
    // `navigate` is a GRAPH WALK, not a `SELECT`: `run_navigate` seeds at the
    // lowest-id document and traverses `edge_store`. This fixture created the
    // edge table and never inserted a row — so the walk could only ever return
    // its own seed (`a1`, ALPHA), and `b1` was unreachable **whether or not the
    // `where:` filter was applied**. That made section 2's negative control impossible
    // to pass and, worse, made section 1's isolation assertion VACUOUS: `!contains
    // ("BRAVO")` held for a reason that had nothing to do with the filter.
    //
    // The edge belongs to sub-tenant A and points at B's document — which is
    // precisely the kivi brief #27 C leak vector: A's own graph reaching into
    // another client's memory. With it seeded:
    // * unfiltered ⇒ docs {a1,b1} + edge ⇒ the walk reaches BRAVO (section 2 control),
    //   * filtered to A ⇒ docs {a1} only ⇒ the edge dangles and BRAVO cannot
    // surface (section 1 isolation) — and now section 1 passes BECAUSE of the filter.
    exec(
        backend,
        &format!(
            // `etype` MUST come from the v2.14.0 closed catalog
            // (cite/elaborate/corroborate/depend/implement/exemplify/contradict/
            // supersede) — `Corpus::from_rows` SILENTLY SKIPS an off-catalog
            // edge, so a typo here reads as "no such edge" and this test would
            // go back to proving nothing.
            "INSERT INTO {EDGE_STORE} (from_id, to_id, tenant_id, etype, weight) VALUES \
             ('a1','b1','{TENANT_A}','elaborate', 1.0)"
        ),
    )
    .await;
}

async fn drop_all(backend: &PostgresStoreBackend) {
    exec(backend, &format!("DROP TABLE IF EXISTS {DOC_STORE}")).await;
    exec(backend, &format!("DROP TABLE IF EXISTS {EDGE_STORE}")).await;
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

fn navigate_node(where_expr: &str) -> IRNavigateStep {
    IRNavigateStep {
        depth: None,
        node_type: "navigate",
        source_line: 0,
        source_column: 0,
        pix_ref: CORPUS.to_string(),
        corpus_ref: CORPUS.to_string(),
        // v2.81.0 — spans BOTH clients' topics. A query matching nothing (the
        // old `"recall"`) gives every candidate a marginal gain of 0, so the walk
        // never leaves its seed and the negative control below cannot fire.
        query: "onboarding billing".to_string(),
        trail_enabled: false,
        output_name: "hits".to_string(),
        seed: String::new(),
        budget: Some(5),
        where_expr: where_expr.to_string(),
    }
}

async fn navigate(registry: &Arc<StoreRegistry>, where_expr: &str, tenant_id: &str) -> String {
    // v2.81.0 / the design decision — keyed on the PORT, not the driver type.
    let pins: Arc<Mutex<HashMap<String, axon::pinned_conn::PinnedConn>>> =
        Arc::new(Mutex::new(HashMap::new()));
    for store in [DOC_STORE, EDGE_STORE] {
        if let StoreHandle::Postgres(b) = registry.resolve(store).expect("resolve") {
            let pin = b.acquire_pin().await.expect("pin");
            pins.lock().unwrap().insert(store.to_string(), pin);
        }
    }
    let mut sources: HashMap<String, IRCorpusStoreSource> = HashMap::new();
    sources.insert(CORPUS.to_string(), corpus_source());

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new("RecallLTM", "kimi", "", CancellationFlag::new(), tx)
        .with_store_registry(registry.clone())
        .with_mdn_corpora(Arc::new(HashMap::new()))
        .with_mdn_adaptive(Arc::new(HashSet::new()))
        .with_mdn_store_sources(Arc::new(sources))
        .with_pinned_conns(pins);
    // The `${tenant_id}` the where-filter resolves against (v1.32.0).
    ctx.let_bindings
        .insert("tenant_id".to_string(), tenant_id.to_string());

    match run_navigate(&navigate_node(where_expr), &mut ctx)
        .await
        .expect("navigate ok")
    {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn navigate_where_scopes_the_mdn_graph_to_one_sub_tenant() {
    let backend = test_backend().await;
    seed(&backend).await;
    let registry = Arc::new(StoreRegistry::build(&store_specs()).expect("registry"));

    // ── section 1 — column-scoped to A: ONLY A's memory, B's absent ─────────────
    let a_hits = navigate(&registry, "tenant_id == '${tenant_id}'", TENANT_A).await;
    assert!(
        a_hits.contains("ALPHA"),
        "section 1: A's own memory must be navigable. hits={a_hits:?}"
    );
    assert!(
        !a_hits.contains("BRAVO"),
        "section 1 LEAK: sub-tenant A's `navigate` surfaced sub-tenant B's memory — the \
         column-scope `where:` is not filtering the sourced rows. hits={a_hits:?}"
    );

    // ── section 2 — negative control: NO where: ⇒ BOTH leak (proves the filter) ─
    let all_hits = navigate(&registry, "", TENANT_A).await;
    assert!(
        all_hits.contains("ALPHA") && all_hits.contains("BRAVO"),
        "section 2: without a `where:` the navigate sources EVERY sub-tenant's rows — \
         this control proves section 1's isolation comes from the filter, not some \
         other scoping. hits={all_hits:?}"
    );

    drop_all(&backend).await;
}
