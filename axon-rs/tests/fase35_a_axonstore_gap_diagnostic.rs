//! §Fase 118.b.3 — this suite exercises the SQL data plane (the Postgres
//! backend, the pinned-connection contract, live introspection), which lives
//! behind the `postgres` feature. Gated at the file level: the core suites keep
//! running in a build with no driver.
#![cfg(feature = "postgres")]

//! §Fase 35.a (v1.30.0) — Diagnostic anchor for the
//! `axonstore`-as-a-cognitive-data-plane cycle.
//!
//! Captures the **current v1.29.x baseline**: the frontend faithfully
//! lowers an `axonstore { backend: postgresql … }` declaration into a
//! rich [`IRAxonStore`] spec — `backend`, `connection`,
//! `confidence_floor`, `isolation`, `on_breach` are ALL preserved
//! through the lexer → parser → IR-generator chain — yet the Rust
//! runtime's store handlers (`persist` / `retrieve` / `mutate` /
//! `purge`) route unconditionally to an in-memory key-value backing
//! (`ctx.let_bindings` under `__store_<name>_<key>` keys). The
//! `IRAxonStore` spec is **never threaded into dispatch**: the runtime
//! is backend-blind.
//!
//! This is the 4th instance of the systematically-closed defect class
//! — a declarable-but-not-wired capability (cf. SSE Fase 30-34,
//! webhook-HMAC v1.29.1, Dockerfile-clobber v1.20.2).
//!
//! # The reframe — `axonstore` is a cognitive data plane
//!
//! Fase 35 does NOT ship a faithful Postgres-ORM port (that would only
//! EQUAL the market). It reframes `axonstore` as a `Relation` enriched
//! orthogonally in FOUR dimensions, each by JOINING an axon system
//! that already exists:
//!
//! | Pillar | Enrichment | Join point | 35 sub-fase |
//! |---|---|---|---|
//! | **I — Epistemic** | every retrieved tuple born `Untrusted` (⊥) in the ESK lattice; `confidence_floor` enforced at `retrieve` + `persist` | `axon::esk` | 35.g |
//! | **II — Audit-chained** | every `persist`/`mutate`/`purge` appends an HMAC-Merkle delta; verifiable mutation history; `on_breach` honored | `axon::esk::provenance` | 35.h |
//! | **III — Streaming** | `retrieve` is a `Stream<Row>` draining through the Fase 34 `unified_stream_handler` with a `BackpressurePolicy` | `axon::flow_dispatcher::unified_stream` | 35.i |
//! | **IV — Capability-typed** | store ops require a capability the type-checker enforces; data isolation becomes a language guarantee | `axon::auth_scope` | 35.j |
//!
//! # Why anchor BEFORE the lift
//!
//! Each subsequent sub-fase's contract is "invert THIS specific
//! v1.29.x assertion". Without an explicit baseline pin, the cycle's
//! progress becomes unfalsifiable — we'd land 35.j "`retrieve` is a
//! `Stream<Row>`" but couldn't verify the inversion happened. The
//! anchor file is the falsifier — same forensic discipline as the
//! 33.a / 34.a / 33.z.k.a anchors.
//!
//! # Diagnostic discipline
//!
//! Forensic capture with `eprintln!` (visible under
//! `cargo test -- --nocapture`). Assertions are minimal + defensive:
//! the goal is to PIN the current behavior so post-35 regressions
//! surface as anchor-inversion test failures.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::wire_integrations::{
    mutate_store, persist_to_store, purge_from_store, retrieve_from_store,
    run_persist, run_retrieve,
};
use axon::flow_dispatcher::DispatchCtx;
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_generator::IRGenerator;
use axon::ir_nodes::{IRPersistStep, IRRetrieveStep};
use axon::lexer::Lexer;
use axon::parser::Parser;
use tokio::sync::mpsc;

/// Compile a source string through the full frontend chain
/// (lexer → parser → IR generator) into an `IRProgram`.
fn compile_ir(src: &str) -> axon::ir_nodes::IRProgram {
    let tokens = Lexer::new(src, "fase35_anchor.axon")
        .tokenize()
        .expect("lex ok");
    let program = Parser::new(tokens).parse().expect("parse ok");
    IRGenerator::new().generate(&program)
}

fn fresh_ctx() -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = DispatchCtx::new("TestFlow", "stub", "", CancellationFlag::new(), tx);
    (ctx, rx)
}

// ════════════════════════════════════════════════════════════════════
//  §1 — Frontend captures the cognitive-data-plane intent in full
// ════════════════════════════════════════════════════════════════════

#[test]
fn s1_frontend_lowers_postgres_axonstore_into_rich_ir_spec() {
    // The adopter declares a Postgres-backed store with all four
    // cognitive-data-plane knobs: `confidence_floor` (Pillar I),
    // `on_breach` (Pillar II), `isolation` (transactional semantics).
    // v1.29.x baseline: the FRONTEND captures every field faithfully
    // — the gap is downstream, in the runtime.
    let src = "axonstore tenants {\n\
        backend: postgresql\n\
        connection: \"env:DATABASE_URL\"\n\
        confidence_floor: 0.8\n\
        isolation: serializable\n\
        on_breach: rollback\n\
    }";

    let ir = compile_ir(src);

    assert_eq!(
        ir.axonstore_specs.len(),
        1,
        "§1: exactly one axonstore lowered into IRProgram.axonstore_specs"
    );
    let store = &ir.axonstore_specs[0];

    eprintln!(
        "§1 anchor (frontend lowers axonstore → IRAxonStore):\n\
         node_type        = {:?}\n\
         name             = {:?}\n\
         backend          = {:?}  (Pillar substrate — Postgres requested)\n\
         connection       = {:?}\n\
         confidence_floor = {:?}  (Pillar I — epistemic floor)\n\
         isolation        = {:?}  (transactional semantics)\n\
         on_breach        = {:?}  (Pillar II — audit-chain breach policy)\n\
         CONCLUSION: the frontend already carries the full cognitive-\n\
         data-plane intent. The gap is 100% runtime-side.",
        store.node_type,
        store.name,
        store.backend,
        store.connection,
        store.confidence_floor,
        store.isolation,
        store.on_breach,
    );

    assert_eq!(store.node_type, "axonstore");
    assert_eq!(store.name, "tenants");
    assert_eq!(
        store.backend, "postgresql",
        "§1: backend `postgresql` preserved through lexer→parser→IR"
    );
    assert_eq!(store.connection, "env:DATABASE_URL");
    assert_eq!(
        store.confidence_floor,
        Some(0.8),
        "§1 Pillar I: confidence_floor preserved as Option<f64> — \
         35.g will ENFORCE it at retrieve/persist"
    );
    assert_eq!(store.isolation, "serializable");
    assert_eq!(
        store.on_breach, "rollback",
        "§1 Pillar II: on_breach preserved — 35.h will HONOR it on \
         audit-chain breach"
    );
}

// ════════════════════════════════════════════════════════════════════
//  §2 — Dispatcher store helpers are KV / backend-blind
// ════════════════════════════════════════════════════════════════════

#[test]
fn s2_store_helpers_route_to_in_memory_kv_not_sql() {
    // The four store helpers (`persist_to_store` / `retrieve_from_store`
    // / `mutate_store` / `purge_from_store`) take ONLY a `store_name`
    // (+ a `where_expr` key) and a `DispatchCtx`. None of them receives
    // an `IRAxonStore` — so `backend: postgresql` cannot possibly reach
    // them. They unconditionally read/write `ctx.let_bindings` under
    // `__store_<name>_<key>` keys: an in-memory HashMap.
    //
    // POST-35 (35.d-f): a real `AxonStore` runtime resolves the
    // `IRAxonStore` to a `sqlx::PgPool`; these helpers (or their
    // successors) route SQL.
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("tenant_id".into(), "acme-001".into());
    ctx.let_bindings.insert("plan".into(), "enterprise".into());

    let persisted = persist_to_store("tenants", &mut ctx);
    assert_eq!(
        persisted, 2,
        "§2: persist snapshots the 2 user bindings into KV"
    );

    // The KV backing is directly observable — pin its exact shape.
    let kv_key = "__store_tenants_tenant_id";
    let kv_present = ctx.let_bindings.contains_key(kv_key);
    let retrieved = retrieve_from_store("tenants", "tenant_id", &ctx);

    eprintln!(
        "§2 anchor (store helpers = in-memory KV, backend-blind):\n\
         KV key written         = {kv_key:?}  present = {kv_present}\n\
         retrieve(tenants, tenant_id) = {retrieved:?}\n\
         helper signatures carry NO IRAxonStore — `backend: postgresql`\n\
         is structurally unreachable from the dispatch path.\n\
         POST-35: backend resolves to a sqlx::PgPool; SQL replaces KV."
    );

    assert!(
        kv_present,
        "§2: persist writes the `__store_<name>_<key>` KV key — pins \
         the in-memory backing shape that 35.d-f will replace with SQL"
    );
    assert_eq!(
        retrieved, "acme-001",
        "§2: retrieve reads back from the SAME in-memory KV — no SQL"
    );

    // mutate / purge operate on the same KV namespace.
    ctx.let_bindings.insert("tenant_id".into(), "acme-002".into());
    let mutated = mutate_store("tenants", "tenant_id", &mut ctx);
    let purged = purge_from_store("tenants", "plan", &mut ctx);
    eprintln!(
        "§2 anchor (mutate/purge also KV): mutate→{mutated} purge→{purged}"
    );
    assert_eq!(mutated, 1, "§2: mutate updates the KV entry in place");
    assert_eq!(purged, 1, "§2: purge removes the KV entry");
}

// ════════════════════════════════════════════════════════════════════
//  §3 — `run_persist` / `run_retrieve` handlers route to KV; the IR
//        step nodes carry NO backend handle, and DispatchCtx has no
//        SQL pool. The IRAxonStore spec and the store-op steps are
//        structurally DISCONNECTED.
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s3_run_handlers_route_to_kv_ir_steps_have_no_backend_handle() {
    // `IRPersistStep` is `{node_type, source_line, source_column,
    // store_name}` — no backend, no connection, no pool reference.
    // `IRRetrieveStep` adds `where_expr` + `alias` — still no backend.
    // The handlers receive only these structs + `&mut DispatchCtx`,
    // and `DispatchCtx` has NO `axonstore_runtime` / SQL-pool field
    // (it carries `let_bindings` for KV + `pem_backend` for cognitive
    // state — neither is a relational store).
    //
    // POST-35: the dispatcher resolves `store_name` → the matching
    // `IRAxonStore` in `IRProgram.axonstore_specs` → an `AxonStore`
    // runtime handle. That resolution edge does not exist today.
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("id".into(), "42".into());

    let persist = IRPersistStep {
        node_type: "persist",
            fields: Vec::new(),
        source_line: 0,
        source_column: 0,
        store_name: "entities".into(),
    };
    run_persist(&persist, &mut ctx).await.expect("persist ok");

    let retrieve = IRRetrieveStep {
        node_type: "retrieve",
        source_line: 0,
        source_column: 0,
        store_name: "entities".into(),
        where_expr: "id".into(),
        alias: "found_id".into(),
        order_by: String::new(),
        limit_expr: String::new(),
        aggregate: String::new(),
        group_by: String::new(),
        cache: String::new(),
    };
    run_retrieve(&retrieve, &mut ctx).await.expect("retrieve ok");

    let found = ctx.let_bindings.get("found_id").cloned().unwrap_or_default();
    eprintln!(
        "§3 anchor (run_* handlers route to KV, IR steps backend-blind):\n\
         IRPersistStep fields = {{node_type, source_line, source_column, store_name}}\n\
         IRRetrieveStep adds  = {{where_expr, alias}}  — still NO backend\n\
         run_persist + run_retrieve round-tripped via let_bindings KV.\n\
         retrieve(entities, where id) bound under alias `found_id` = {found:?}\n\
         DispatchCtx has NO SQL-pool field — `IRAxonStore.backend` cannot\n\
         influence dispatch. The spec and the steps are DISCONNECTED.\n\
         POST-35: dispatcher resolves store_name → IRAxonStore → AxonStore."
    );
    assert_eq!(
        found, "42",
        "§3: run_persist→run_retrieve round-trips through in-memory KV; \
         `where_expr` is treated as the entry key, not a SQL predicate"
    );
}

// ════════════════════════════════════════════════════════════════════
//  §4 — The four pillars are ABSENT today (pin the v1.29.x state that
//        35.g-j will each invert).
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s4_four_pillars_absent_in_v1_29_baseline() {
    // Pillar I (epistemic): `retrieve_from_store` returns a bare
    //   `String`. There is no ESK trust tag — the retrieved value is
    //   NOT born `Untrusted` (⊥), and `confidence_floor` is never
    //   consulted. The IRAxonStore carries `confidence_floor` (§1)
    //   but the runtime never reads it.
    // Pillar II (audit-chain): persist/mutate/purge mutate the KV in
    //   place. No HMAC-Merkle delta is appended; no mutation history
    //   exists; `on_breach` is inert.
    // Pillar III (streaming): `retrieve` returns ONE materialized
    //   `String`, not a `Stream<Row>`. It does not drain through the
    //   Fase 34 `unified_stream_handler`; no `BackpressurePolicy`.
    // Pillar IV (capability): the helpers + handlers require NO
    //   capability. There is no `auth_scope` check on store access.
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("balance".into(), "1000".into());
    persist_to_store("ledger", &mut ctx);

    // Pillar I — the return type is a plain String (no trust lattice).
    let value: String = retrieve_from_store("ledger", "balance", &ctx);
    let returns_bare_string = value == "1000";

    // Pillar II — no audit-chain key materialises after a mutation.
    ctx.let_bindings.insert("balance".into(), "2000".into());
    mutate_store("ledger", "balance", &mut ctx);
    let audit_chain_keys = ctx
        .let_bindings
        .keys()
        .filter(|k| k.contains("audit") || k.contains("merkle") || k.contains("delta"))
        .count();

    eprintln!(
        "§4 anchor (four pillars ABSENT in v1.29.x):\n\
         Pillar I  (epistemic)  : retrieve → bare String {value:?} — \
         no ESK ⊥ tag, confidence_floor never read.  35.g inverts.\n\
         Pillar II (audit-chain): audit/merkle/delta KV keys = {audit_chain_keys} \
         — no HMAC-Merkle history, on_breach inert.  35.h inverts.\n\
         Pillar III (streaming) : retrieve return type is `String`, not \
         `Stream<Row>` — no unified_stream_handler.  35.i inverts.\n\
         Pillar IV (capability) : store helpers require NO capability — \
         no auth_scope gate.  35.j inverts."
    );

    assert!(
        returns_bare_string,
        "§4 Pillar I: retrieve returns a bare String — no epistemic \
         trust tag. 35.g makes every tuple born Untrusted (⊥)."
    );
    assert_eq!(
        audit_chain_keys, 0,
        "§4 Pillar II: zero audit-chain keys after a mutation — no \
         HMAC-Merkle delta exists. 35.h appends a verifiable delta \
         per persist/mutate/purge."
    );
}

// ════════════════════════════════════════════════════════════════════
//  §5 — Closed-catalog totality pins
// ════════════════════════════════════════════════════════════════════

#[test]
fn s5_store_op_and_pillar_catalogs_are_closed() {
    // The store-op catalog: persist / retrieve / mutate / purge are
    // the FOUR mutation/query primitives over an axonstore (transact
    // is the block wrapper — a future fase per the honest-scope D12
    // statement). Adding a 5th store op is a deliberate language-level
    // decision requiring a paper update + a dedicated sub-fase.
    const STORE_OPS: &[&str] = &["persist", "retrieve", "mutate", "purge"];
    assert_eq!(
        STORE_OPS.len(),
        4,
        "§5: the axonstore op catalog is EXACTLY 4 — persist/retrieve/\
         mutate/purge. Growth is an explicit code change at this site."
    );

    // The pillar catalog: the cognitive-data-plane reframe enriches a
    // Relation in EXACTLY four orthogonal dimensions. A 5th pillar is
    // a founder-level reframe, not an incremental sub-fase.
    const PILLARS: &[&str] = &[
        "I — epistemic (ESK ⊥ trust lattice + confidence_floor)",
        "II — audit-chained (HMAC-Merkle delta + on_breach)",
        "III — streaming (retrieve as Stream<Row> via unified_stream_handler)",
        "IV — capability-typed (auth_scope-enforced store access)",
    ];
    assert_eq!(
        PILLARS.len(),
        4,
        "§5: the cognitive-data-plane reframe is EXACTLY 4 pillars. A \
         5th is a founder-level reframe, not an incremental sub-fase."
    );

    eprintln!(
        "§5 anchor (closed catalogs):\n\
         store ops = {STORE_OPS:?}\n\
         pillars   = {PILLARS:?}"
    );
}
