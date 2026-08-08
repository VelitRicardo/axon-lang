//! §Fase 118.b.3 — this suite exercises the SQL data plane (the Postgres
//! backend, the pinned-connection contract, live introspection), which lives
//! behind the `postgres` feature. Gated at the file level: the core suites keep
//! running in a build with no driver.
#![cfg(feature = "postgres")]

//! §Fase 66 (Q2) — Real-Postgres integration: `navigate { … where: "<col
//! filter>" }` scopes a `corpus from axonstore` to a SUB-TENANT column, so an
//! adopter multiplexing N end-clients in ONE axon-tenant via a `tenant_id`
//! column does not leak memories across clients.
//!
//! The kivi brief #27 §C blocker: RLS scopes by axon-tenant, but kivi packs N
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
//! (mirrors `fase65_navigate_pg_integration`). CI provides `postgres:16`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::cognitive::run_navigate;
use axon::flow_dispatcher::{DispatchCtx, NodeOutcome};
use axon::ir_nodes::{IRAxonStore, IRCorpusStoreSource, IRNavigateStep};
use axon::store::postgres_backend::PostgresStoreBackend;
use axon::store::registry::{StoreHandle, StoreRegistry};

const DOC_STORE: &str = "fase66_ltm_docs";
const EDGE_STORE: &str = "fase66_ltm_edges";
const CORPUS: &str = "LtmGraph";
const TENANT_A: &str = "11111111-1111-1111-1111-111111111111";
const TENANT_B: &str = "22222222-2222-2222-2222-222222222222";

async fn test_backend() -> Option<PostgresStoreBackend> {
    let dsn = match std::env::var("AXON_TEST_DATABASE_URL") {
        Ok(d) if !d.trim().is_empty() => d,
        _ => {
            eprintln!("fase66: AXON_TEST_DATABASE_URL unset — skipping navigate where-scope test");
            return None;
        }
    };
    let backend = match PostgresStoreBackend::connect(&dsn) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("fase66: backend connect failed ({e}) — skipping");
            return None;
        }
    };
    if backend.ping().await.is_err() {
        eprintln!("fase66: Postgres unreachable — skipping");
        return None;
    }
    Some(backend)
}

async fn exec(backend: &PostgresStoreBackend, sql: &str) {
    sqlx::query(sql)
        .execute(backend.pool())
        .await
        .unwrap_or_else(|e| panic!("fase66 fixture SQL failed:\n  {sql}\n  {e}"));
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
            // §Fase 118.a — the summaries carry DISTINCT topics on purpose.
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
            // precisely the crossing §1 must prove the `where:` filter prevents.
            "INSERT INTO {DOC_STORE} (id, tenant_id, summary) VALUES \
             ('a1','{TENANT_A}','ALPHA client onboarding notes'), \
             ('b1','{TENANT_B}','BRAVO client billing dispute')"
        ),
    )
    .await;
    // §Fase 118.a — THE EDGE THIS FIXTURE WAS MISSING, and without which the
    // whole suite proved nothing.
    //
    // `navigate` is a GRAPH WALK, not a `SELECT`: `run_navigate` seeds at the
    // lowest-id document and traverses `edge_store`. This fixture created the
    // edge table and never inserted a row — so the walk could only ever return
    // its own seed (`a1`, ALPHA), and `b1` was unreachable **whether or not the
    // `where:` filter was applied**. That made §2's negative control impossible
    // to pass and, worse, made §1's isolation assertion VACUOUS: `!contains
    // ("BRAVO")` held for a reason that had nothing to do with the filter.
    //
    // The edge belongs to sub-tenant A and points at B's document — which is
    // precisely the kivi brief #27 §C leak vector: A's own graph reaching into
    // another client's memory. With it seeded:
    //   * unfiltered ⇒ docs {a1,b1} + edge ⇒ the walk reaches BRAVO (§2 control),
    //   * filtered to A ⇒ docs {a1} only ⇒ the edge dangles and BRAVO cannot
    //     surface (§1 isolation) — and now §1 passes BECAUSE of the filter.
    exec(
        backend,
        &format!(
            // `etype` MUST come from the §64 closed catalog
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
        // §Fase 118.a — spans BOTH clients' topics. A query matching nothing (the
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
    // §Fase 118.a / D118.2 — keyed on the PORT, not the driver type.
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
    // The `${tenant_id}` the where-filter resolves against (§37.d).
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
    let backend = match test_backend().await {
        Some(b) => b,
        None => return,
    };
    seed(&backend).await;
    let registry = Arc::new(StoreRegistry::build(&store_specs()).expect("registry"));

    // ── §1 — column-scoped to A: ONLY A's memory, B's absent ─────────────
    let a_hits = navigate(&registry, "tenant_id == '${tenant_id}'", TENANT_A).await;
    assert!(
        a_hits.contains("ALPHA"),
        "§1: A's own memory must be navigable. hits={a_hits:?}"
    );
    assert!(
        !a_hits.contains("BRAVO"),
        "§1 LEAK: sub-tenant A's `navigate` surfaced sub-tenant B's memory — the \
         column-scope `where:` is not filtering the sourced rows. hits={a_hits:?}"
    );

    // ── §2 — negative control: NO where: ⇒ BOTH leak (proves the filter) ─
    let all_hits = navigate(&registry, "", TENANT_A).await;
    assert!(
        all_hits.contains("ALPHA") && all_hits.contains("BRAVO"),
        "§2: without a `where:` the navigate sources EVERY sub-tenant's rows — \
         this control proves §1's isolation comes from the filter, not some \
         other scoping. hits={all_hits:?}"
    );

    drop_all(&backend).await;
}
