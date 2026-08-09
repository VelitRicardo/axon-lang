//! §Fase 118.b.3 — this suite exercises the SQL data plane (the Postgres
//! backend, the pinned-connection contract, live introspection), which lives
//! behind the `postgres` feature. Gated at the file level: the core suites keep
//! running in a build with no driver.
#![cfg(feature = "postgres")]

//! §Fase 67.g — Real-Postgres integration: the kivi brief #35 repro END-TO-END.
//! `retrieve sessions { … as: to_hibernate }` then
//! `for s in to_hibernate { persist into <ltm> { tenant_id: "${s.tenant_id}" … } }`
//! writes the RESOLVED column values, not the literal `${s.tenant_id}`.
//!
//! Brief #28/§66.1 fixed `for e in ClassifyEdges.output` — a step's `List<T>`
//! output, a bare JSON ARRAY. The daemon's sweep iterates a DIFFERENT shape: a
//! `retrieve … as:` binds an epistemic ENVELOPE object (`{"taint":…,"rows":[…]}`),
//! which failed `resolve_iterable`'s array check, fell to the comma-split, and
//! shredded the envelope JSON — so every `${s.<col>}` reached Postgres verbatim
//! (`invalid input syntax for type uuid: "${s.tenant_id}"`). §67.g unwraps the
//! envelope's `rows` and iterates them exactly like a step's array output. This
//! test drives the FULL composition — a REAL retrieve building a REAL envelope,
//! then the loop+persist — against a real DB so the gap cannot recur. The target
//! `tenant_id` is a `uuid` column on purpose: an unresolved `${s.tenant_id}`
//! would be rejected by Postgres, exactly as the adopter saw in prod.
//!
//! # Harness
//!
//! Connects via `AXON_TEST_DATABASE_URL`; skips when unset/unreachable. CI
//! provides `postgres:16`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::orchestration::run_for_in;
use axon::flow_dispatcher::wire_integrations::run_retrieve;
use axon::flow_dispatcher::{DispatchCtx, NodeOutcome};
use axon::ir_nodes::{IRAxonStore, IRFlowNode, IRForIn, IRPersistStep, IRRetrieveStep};
use axon::store::postgres_backend::PostgresStoreBackend;
use axon::store::registry::{StoreHandle, StoreRegistry};

const SESSION_STORE: &str = "fase67g_sessions";
const LTM_STORE: &str = "fase67g_ltm";

// Two ACTIVE rows (to hibernate) + one HIBERNATED (must be skipped by the
// `where:`). The tenant_ids are real uuids so the resolved value lands in a
// uuid column cleanly — and an UNRESOLVED `${s.tenant_id}` would be rejected.
const TENANT_A: &str = "11111111-1111-1111-1111-111111111111";
const TENANT_B: &str = "22222222-2222-2222-2222-222222222222";

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
        "fase67.g: {why} — and this suite REFUSES to skip.\n\n\
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
        .unwrap_or_else(|e| panic!("fase67.g fixture SQL failed:\n  {sql}\n  {e}"));
}

fn store_spec() -> Vec<IRAxonStore> {
    [SESSION_STORE, LTM_STORE]
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

/// `retrieve fase67g_sessions { where: "status == 'ACTIVE'"  as: to_hibernate }`.
fn retrieve_active() -> IRRetrieveStep {
    IRRetrieveStep {
        node_type: "retrieve",
        source_line: 0,
        source_column: 0,
        store_name: SESSION_STORE.to_string(),
        where_expr: "status == 'ACTIVE'".to_string(),
        alias: "to_hibernate".to_string(),
        order_by: "tenant_id asc".to_string(),
        limit_expr: String::new(),
        aggregate: String::new(),
        group_by: String::new(),
        cache: String::new(),
    }
}

/// `for s in to_hibernate { persist into fase67g_ltm { tenant_id: "${s.tenant_id}"
///  session_id_generic: "${s.session_id_generic}" } }`.
fn for_in_persist_ltm() -> IRForIn {
    IRForIn {
        node_type: "for_in",
        source_line: 0,
        source_column: 0,
        variable: "s".into(),
        iterable: "to_hibernate".into(),
        body: vec![IRFlowNode::Persist(IRPersistStep {
            node_type: "persist",
            source_line: 0,
            source_column: 0,
            store_name: LTM_STORE.to_string(),
            fields: vec![
                ("tenant_id".into(), "${s.tenant_id}".into()),
                (
                    "session_id_generic".into(),
                    "${s.session_id_generic}".into(),
                ),
            ],
        })],
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn loop_over_retrieve_envelope_persists_resolved_row_columns() {
    let backend = test_backend().await;

    exec(&backend, &format!("DROP TABLE IF EXISTS {SESSION_STORE}")).await;
    exec(&backend, &format!("DROP TABLE IF EXISTS {LTM_STORE}")).await;
    exec(
        &backend,
        &format!(
            "CREATE TABLE {SESSION_STORE} (tenant_id uuid, session_id_generic TEXT, status TEXT)"
        ),
    )
    .await;
    exec(
        &backend,
        &format!("CREATE TABLE {LTM_STORE} (tenant_id uuid, session_id_generic TEXT)"),
    )
    .await;
    exec(
        &backend,
        &format!(
            "INSERT INTO {SESSION_STORE} VALUES \
             ('{TENANT_A}', 'sess-a', 'ACTIVE'), \
             ('{TENANT_B}', 'sess-b', 'ACTIVE'), \
             ('{TENANT_A}', 'sess-skip', 'HIBERNATED')"
        ),
    )
    .await;

    let registry = Arc::new(StoreRegistry::build(&store_spec()).expect("registry"));
    // §Fase 118.a / D118.2 — keyed on the PORT, not the driver type.
    let pins: Arc<Mutex<HashMap<String, axon::pinned_conn::PinnedConn>>> =
        Arc::new(Mutex::new(HashMap::new()));
    for store in [SESSION_STORE, LTM_STORE] {
        if let StoreHandle::Postgres(b) = registry.resolve(store).expect("resolve") {
            pins.lock()
                .unwrap()
                .insert(store.to_string(), b.acquire_pin().await.expect("pin"));
        }
    }

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new("HibernateSweep", "stub", "", CancellationFlag::new(), tx)
        .with_store_registry(registry.clone())
        .with_pinned_conns(pins);

    // ── Drive a REAL retrieve: it binds `to_hibernate` to the real envelope ──
    match run_retrieve(&retrieve_active(), &mut ctx).await {
        Ok(NodeOutcome::Completed { .. }) => {}
        other => panic!("retrieve failed: {other:?}"),
    }
    // The binding is the epistemic ENVELOPE object, not a bare array — the exact
    // shape that pre-§67.g shredded into garbage.
    let bound = ctx.let_bindings.get("to_hibernate").expect("bound alias");
    assert!(
        bound.contains("\"rows\""),
        "retrieve binds an envelope carrying `rows`: {bound}"
    );

    // ── The sweep loop persists the RESOLVED columns into the uuid LTM table ──
    match run_for_in(&for_in_persist_ltm(), &mut ctx).await {
        Ok(NodeOutcome::Completed { .. }) => {}
        other => panic!("for-in over retrieve envelope failed: {other:?}"),
    }

    let rows: Vec<(uuid::Uuid, String)> = sqlx::query_as(&format!(
        "SELECT tenant_id, session_id_generic FROM {LTM_STORE} ORDER BY tenant_id"
    ))
    .fetch_all(backend.pool())
    .await
    .expect("select ltm");

    // Only the two ACTIVE rows hibernated (the HIBERNATED one was filtered by the
    // `where:`), each carrying its RESOLVED uuid + session id — never a literal.
    assert_eq!(rows.len(), 2, "two ACTIVE rows persisted, the third filtered");
    assert_eq!(rows[0].0.to_string(), TENANT_A);
    assert_eq!(rows[0].1, "sess-a");
    assert_eq!(rows[1].0.to_string(), TENANT_B);
    assert_eq!(rows[1].1, "sess-b");
    for (_, sid) in &rows {
        assert!(
            !sid.contains("${"),
            "REGRESSION: a literal `${{s.<col>}}` reached the DB (kivi #35). got={sid:?}"
        );
    }

    exec(&backend, &format!("DROP TABLE IF EXISTS {SESSION_STORE}")).await;
    exec(&backend, &format!("DROP TABLE IF EXISTS {LTM_STORE}")).await;
}
