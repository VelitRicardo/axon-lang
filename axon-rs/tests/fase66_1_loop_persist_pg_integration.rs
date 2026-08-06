//! §Fase 118.b.3 — this suite exercises the SQL data plane (the Postgres
//! backend, the pinned-connection contract, live introspection), which lives
//! behind the `postgres` feature. Gated at the file level: the core suites keep
//! running in a build with no driver.
#![cfg(feature = "postgres")]

//! §Fase 66.1 — Real-Postgres integration: the kivi brief #28 repro END-TO-END.
//! `for e in ClassifyEdges.output { persist into <edges> { to_id: "${e.to_id}" } }`
//! writes the RESOLVED field value (the real uuid), not the literal `${e.to_id}`.
//!
//! §66 shipped the dotted `${e.field}` interpolation + JSON-array iteration, but
//! its test used `for e in <plain binding>`. The adopter's CANONICAL form is
//! `for e in ClassifyEdges.output` — a `Step.output` reference. That reference
//! did not resolve (the step binds under the bare name `ClassifyEdges`, but
//! `resolve_iterable` did exact `get("ClassifyEdges.output")` → miss → literal),
//! so the whole feature was inert in production: `${e.to_id}` reached Postgres
//! verbatim → `invalid input syntax for type uuid: "${e.to_id}"`. §66.1 resolves
//! the iterable reference (the `.output` maps to the step-name key). This test
//! drives the FULL composition against a real DB so the gap cannot recur.
//!
//! # Harness
//!
//! Connects via `AXON_TEST_DATABASE_URL`; skips when unset/unreachable. CI
//! provides `postgres:16`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::orchestration::run_for_in;
use axon::flow_dispatcher::{DispatchCtx, NodeOutcome};
use axon::ir_nodes::{IRAxonStore, IRFlowNode, IRForIn, IRPersistStep};
use axon::store::postgres_backend::PostgresStoreBackend;
use axon::store::registry::{StoreHandle, StoreRegistry};

const EDGE_STORE: &str = "fase66_1_edges";

async fn test_backend() -> Option<PostgresStoreBackend> {
    let dsn = match std::env::var("AXON_TEST_DATABASE_URL") {
        Ok(d) if !d.trim().is_empty() => d,
        _ => {
            eprintln!("fase66.1: AXON_TEST_DATABASE_URL unset — skipping loop-persist E2E");
            return None;
        }
    };
    let backend = PostgresStoreBackend::connect(&dsn).ok()?;
    if backend.ping().await.is_err() {
        eprintln!("fase66.1: Postgres unreachable — skipping");
        return None;
    }
    Some(backend)
}

async fn exec(backend: &PostgresStoreBackend, sql: &str) {
    sqlx::query(sql)
        .execute(backend.pool())
        .await
        .unwrap_or_else(|e| panic!("fase66.1 fixture SQL failed:\n  {sql}\n  {e}"));
}

fn store_spec() -> Vec<IRAxonStore> {
    vec![IRAxonStore {
        node_type: "axonstore",
        source_line: 0,
        source_column: 0,
        name: EDGE_STORE.to_string(),
        backend: "postgresql".to_string(),
        connection: "env:AXON_TEST_DATABASE_URL".to_string(),
        confidence_floor: None,
        isolation: String::new(),
        on_breach: String::new(),
        capability: String::new(),
        class: String::new(),
        column_schema: None,
        resource_ref: String::new(),
    }]
}

/// `for e in ClassifyEdges.output { persist into fase66_1_edges { from_id:
/// "${summary_id}"  to_id: "${e.to_id}"  etype: "${e.etype}" } }`.
fn for_in_persist_edges() -> IRForIn {
    IRForIn {
        node_type: "for_in",
        source_line: 0,
        source_column: 0,
        variable: "e".into(),
        iterable: "ClassifyEdges.output".into(),
        body: vec![IRFlowNode::Persist(IRPersistStep {
            node_type: "persist",
            source_line: 0,
            source_column: 0,
            store_name: EDGE_STORE.to_string(),
            fields: vec![
                ("from_id".into(), "${summary_id}".into()),
                ("to_id".into(), "${e.to_id}".into()),
                ("etype".into(), "${e.etype}".into()),
            ],
        })],
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn loop_over_step_output_persists_resolved_field_values() {
    let backend = match test_backend().await {
        Some(b) => b,
        None => return,
    };
    exec(&backend, &format!("DROP TABLE IF EXISTS {EDGE_STORE}")).await;
    exec(
        &backend,
        &format!("CREATE TABLE {EDGE_STORE} (from_id TEXT, to_id TEXT, etype TEXT)"),
    )
    .await;

    let registry = Arc::new(StoreRegistry::build(&store_spec()).expect("registry"));
    // §Fase 118.a / D118.2 — the pin map is keyed on the PORT
    // (`axon::pinned_conn::PinnedConn`), not on `sqlx::pool::PoolConnection`.
    // That this test had to change is the point: it was reaching for the driver
    // type to describe something the executor only ever carries.
    let pins: Arc<Mutex<HashMap<String, axon::pinned_conn::PinnedConn>>> =
        Arc::new(Mutex::new(HashMap::new()));
    if let StoreHandle::Postgres(b) = registry.resolve(EDGE_STORE).expect("resolve") {
        pins.lock()
            .unwrap()
            .insert(EDGE_STORE.to_string(), b.acquire_pin().await.expect("pin"));
    }

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new("Hibernate", "stub", "", CancellationFlag::new(), tx)
        .with_store_registry(registry.clone())
        .with_pinned_conns(pins);
    // The step's output (a List<EdgeDecision>) is bound under its BARE NAME, a
    // JSON array — exactly what ClassifyEdges emits.
    ctx.let_bindings.insert(
        "ClassifyEdges".to_string(),
        r#"[{"to_id":"11111111-1111-1111-1111-111111111111","etype":"supersede"},
            {"to_id":"22222222-2222-2222-2222-222222222222","etype":"cite"}]"#
            .to_string(),
    );
    ctx.let_bindings
        .insert("summary_id".to_string(), "aaaa0000-0000-0000-0000-000000000000".to_string());

    match run_for_in(&for_in_persist_edges(), &mut ctx).await {
        Ok(NodeOutcome::Completed { .. }) => {}
        other => panic!("for-in persist failed: {other:?}"),
    }

    // ── The rows carry the RESOLVED uuids, never the literal `${e.to_id}` ──
    let rows: Vec<(String, String)> =
        sqlx::query_as(&format!("SELECT to_id, etype FROM {EDGE_STORE} ORDER BY to_id"))
            .fetch_all(backend.pool())
            .await
            .expect("select edges");
    assert_eq!(rows.len(), 2, "both loop elements persisted a row");
    assert_eq!(rows[0].0, "11111111-1111-1111-1111-111111111111");
    assert_eq!(rows[0].1, "supersede");
    assert_eq!(rows[1].0, "22222222-2222-2222-2222-222222222222");
    assert_eq!(rows[1].1, "cite");
    for (to_id, _) in &rows {
        assert!(
            !to_id.contains("${"),
            "REGRESSION: the literal `${{e.to_id}}` reached the DB (kivi #28). got={to_id:?}"
        );
    }

    exec(&backend, &format!("DROP TABLE IF EXISTS {EDGE_STORE}")).await;
}
