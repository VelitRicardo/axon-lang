//! v2.81.0 — this suite exercises the SQL data plane (the Postgres
//! backend, the pinned-connection contract, live introspection), which lives
//! behind the `postgres` feature. Gated at the file level: the core suites keep
//! running in a build with no driver.
#![cfg(feature = "postgres")]

//! v2.17.0 — Real-Postgres integration: the kivi brief #28 repro END-TO-END.
//! `for e in ClassifyEdges.output { persist into <edges> { to_id: "${e.to_id}" } }`
//! writes the RESOLVED field value (the real uuid), not the literal `${e.to_id}`.
//!
//! v2.17.0 shipped the dotted `${e.field}` interpolation + JSON-array iteration, but
//! its test used `for e in <plain binding>`. The adopter's CANONICAL form is
//! `for e in ClassifyEdges.output` — a `Step.output` reference. That reference
//! did not resolve (the step binds under the bare name `ClassifyEdges`, but
//! `resolve_iterable` did exact `get("ClassifyEdges.output")` → miss → literal),
//! so the whole feature was inert in production: `${e.to_id}` reached Postgres
//! verbatim → `invalid input syntax for type uuid: "${e.to_id}"`. v2.17.0 resolves
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

const EDGE_STORE: &str = "gate_66_1_edges";

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
        "v2.17.0.1: {why} — and this suite REFUSES to skip.\n\n\
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
        .unwrap_or_else(|e| panic!("v2.17.0.1 fixture SQL failed:\n  {sql}\n  {e}"));
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

/// `for e in ClassifyEdges.output { persist into gate_66_1_edges { from_id:
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
    let backend = test_backend().await;
    exec(&backend, &format!("DROP TABLE IF EXISTS {EDGE_STORE}")).await;
    exec(
        &backend,
        &format!("CREATE TABLE {EDGE_STORE} (from_id TEXT, to_id TEXT, etype TEXT)"),
    )
    .await;

    let registry = Arc::new(StoreRegistry::build(&store_spec()).expect("registry"));
    // v2.81.0 / the design decision — the pin map is keyed on the PORT
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
