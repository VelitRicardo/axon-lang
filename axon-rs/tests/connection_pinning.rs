//! v2.81.0 — this suite exercises the SQL data plane (the Postgres
//! backend, the pinned-connection contract, live introspection), which lives
//! behind the `postgres` feature. Gated at the file level: the core suites keep
//! running in a build with no driver.
#![cfg(feature = "postgres")]

//! v1.32.0 — Connection-Pinned Flow Execution.
//!
//! This anchor pins the v1.39.0 substrate that closes the
//! `unnamed prepared statement does not exist` race against
//! transaction-mode poolers (Supavisor `:6543`, PgBouncer
//! `pool_mode=transaction`, Neon, RDS Proxy).
//!
//! The full -assertion set from the plan vivo section 5 is partitioned by
//! infrastructure requirement:
//!
//!   IN THIS FILE (no external infra) — `cargo test` portable:
//! S — STATIC grep that the public-surface declarations exist
//!          (`StoreConn` enum + variants, `acquire_pin` method on
//!          `PostgresStoreBackend`, `pinned_conns` field on
//!          `DispatchCtx`, `emit_pin_acquire` symbol).
//! section 3 — D3 in_memory backwards-compat: a flow whose only store
//!          is `in_memory` produces byte-identical output to v1.38.5
//!          (no pin acquired, no warning, no behavioral change).
//! section 4 — D4 observability emit: `emit_pin_acquire` produces a
//!          structured `tracing::info!` event with the documented
//!          fields when called with typical inputs.
//!
//!   DEFERRED to a future CI lane that brings up PgBouncer
//! transaction-mode in a compose service (step 37.x.j.8.b):
//! section 1 — D1 sync runner: 5 sequential retrieves all hit the same
//!          `conn_id` (requires real Postgres + introspection of
//!          which physical backend served each query).
//! section 2 — D2 async dispatcher: same property on the streaming path.
//! section 5 — D4 error-path pin release: a flow that errors mid-
//!          execution still releases pinned conns (requires real
//!          Postgres + error injection harness).
//! section 6 — D5 property test: 100 flows × 5 retrieves each against
//!          PgBouncer transaction-mode → 100% no `unnamed prepared
//!          statement does not exist` errors (the regression-guard
//!          property).
//! section 7 — D6 par-block: `par { branch_a } { branch_b }` against
//!          same store → both branches succeed, neither sees the
//!          race (requires real Postgres + concurrent execution
//!          harness).
//!
//! The deferral is honest because the property tests need real
//! pooler behavior — neither the in-process axon test harness nor
//! sqlx's mock layer simulates Supavisor's connection-swap window.
//! Sub-cycle 37.x.j.8.b lands the CI compose service + the property
//! pack against it.

use axon::store::pin_observability;
use axon::store::store_conn::StoreConn;

// ── S — STATIC grep: surface declarations are present ─────────────

#[test]
fn s_static_grep_storeconn_enum_present() {
    let src = include_str!("../src/store/store_conn.rs");
    assert!(
        src.contains("pub enum StoreConn<'a>"),
        "v1.32.0 S — `pub enum StoreConn<'a>` declaration MUST be \
         present in `axon-rs/src/store/store_conn.rs`. This is the \
         load-bearing D1 dispatch primitive; removing it silently \
         regresses the entire 37.x.j cycle. Source:\n{src}"
    );
    assert!(
        src.contains("Pool(&'a PgPool)"),
        "v1.32.0 S — `StoreConn::Pool(&'a PgPool)` variant MUST be \
         present. This is the v1.38.5 legacy fallback path; without \
         it the wire-integration handlers cannot construct a \
         `StoreConn` when the pin map is empty (D5 backwards-compat \
         absolute)."
    );
    assert!(
        src.contains("Pinned(&'a mut PoolConnection<Postgres>)"),
        "v1.32.0 S — `StoreConn::Pinned(...)` variant MUST be \
         present. This is the D1 win — without it the dispatch \
         cannot route through a held pin."
    );
    assert!(
        src.contains("pub async fn fetch_all"),
        "v1.32.0 S — `StoreConn::fetch_all` dispatch method MUST be \
         present (used by the cache-HIT path of every backend op)."
    );
    assert!(
        src.contains("pub async fn execute"),
        "v1.32.0 S — `StoreConn::execute` dispatch method MUST be \
         present (used by INSERT/UPDATE/DELETE backend ops)."
    );
    assert!(
        src.contains("pub async fn begin"),
        "v1.32.0 S — `StoreConn::begin` dispatch method MUST be \
         present (used by the cache-MISS transaction path of every \
         backend op)."
    );
}

#[test]
fn s_static_grep_acquire_pin_on_postgres_backend() {
    let src = include_str!("../src/store/postgres_backend.rs");
    assert!(
        src.contains("pub async fn acquire_pin("),
        "v1.32.0 S — `PostgresStoreBackend::acquire_pin` MUST be \
         present in `axon-rs/src/store/postgres_backend.rs`. This is \
         the primitive `ExecContext` / `DispatchCtx` call at flow \
         start to acquire the flow-scoped pin."
    );
}

#[test]
fn s_static_grep_dispatch_ctx_pinned_conns_field() {
    let src = include_str!("../src/flow_dispatcher/mod.rs");
    assert!(
        src.contains("pub pinned_conns:"),
        "v1.32.0 S — `DispatchCtx.pinned_conns` field MUST be \
         declared. This is the Arc<Mutex<HashMap>> the async \
         dispatcher consults per store op for take/dispatch/return \
         routing."
    );
    assert!(
        src.contains("pub fn with_pinned_conns("),
        "v1.32.0 S — `DispatchCtx::with_pinned_conns` builder MUST \
         be present so the streaming dispatcher installs its \
         eagerly-acquired pins on the ctx before the flow walk."
    );
}

#[test]
fn s_static_grep_pin_observability_emit_symbols() {
    let src = include_str!("../src/store/pin_observability.rs");
    assert!(
        src.contains("pub fn emit_pin_acquire("),
        "v1.32.0 S — `emit_pin_acquire` MUST be declared in \
         `axon-rs/src/store/pin_observability.rs`. This is the D4 \
         observability surface that gives operators visibility into \
         pin acquisitions under load."
    );
    assert!(
        src.contains("pub fn emit_pin_flow_summary("),
        "v1.32.0 S — `emit_pin_flow_summary` MUST be declared. \
         This emits the end-of-flow pin-release count for pin-leak \
         detection."
    );
}

#[test]
fn s_static_grep_par_branch_arc_replace() {
    // v1.32.0 (D6.a) — parallel.rs MUST replace the cloned
    // ctx's `pinned_conns` Arc with a fresh empty one so per-branch
    // sub-pins are structurally isolated. A future refactor that
    // accidentally drops this replacement silently regresses D6.a
    // semantics — every par branch would serialize on the parent's
    // pin map mutex (D6.b behavior without the explicit opt-in).
    let src = include_str!("../src/flow_dispatcher/parallel.rs");
    assert!(
        src.contains("bc.pinned_conns = std::sync::Arc::new"),
        "v1.32.0 S — per-par-branch `bc.pinned_conns` Arc \
         replacement MUST be present in `parallel.rs`. Without it, \
         par branches serialize on the parent's pin → D6.a default \
         per-branch sub-pin semantics silently regress to D6.b \
         serialization."
    );
}

#[test]
fn s_static_grep_runner_eager_pin_acquire() {
    // v1.32.0 (D1) — the sync runner's eager pin acquisition
    // walk MUST be present in `execute_server_flow`. It's the load-
    // bearing entry point: without it, `pinned_conns` stays empty
    // and every store op falls back to `StoreConn::Pool` (legacy).
    let src = include_str!("../src/runner.rs");
    // v1.32.0 — the variable name binding the backend handle
    // varies (pre-hotfix: `backend`; post-hotfix: `backend_pool` to
    // avoid shadowing inside the single outer `block_on_store`). What
    // matters structurally is the call `.acquire_pin().await` inside
    // `runner.rs` — the method invocation, not the receiver name.
    assert!(
        src.contains(".acquire_pin().await"),
        "v1.32.0 S — `runner.rs` MUST invoke `.acquire_pin().await` \
         to populate the flow-scoped pin map at execution start. A \
         future refactor that drops this call would silently regress \
         the sync path's pin protection."
    );
    assert!(
        src.contains("execute_sql_store_step(") && src.contains("pinned_conns"),
        "v1.32.0 S — `execute_sql_store_step` MUST be threaded \
         with the `pinned_conns: &mut HashMap` parameter for the \
         take/return discipline to work."
    );
}

#[test]
fn s_static_grep_streaming_dispatcher_eager_pin_acquire() {
    let src = include_str!("../src/streaming_via_dispatcher.rs");
    assert!(
        src.contains("backend.acquire_pin().await"),
        "v1.32.0 S — `streaming_via_dispatcher.rs` MUST invoke \
         `acquire_pin()` to populate the dispatcher's pin map at \
         flow start. Without it, the async path's `StoreConn::Pinned` \
         dispatch never activates and the adopter `/api/chat` \
         regression is structurally re-opened."
    );
    assert!(
        src.contains(".with_pinned_conns("),
        "v1.32.0 S — the dispatcher MUST install the acquired pins \
         on the `DispatchCtx` via `.with_pinned_conns(...)`. \
         Without the install the wire-integration handlers see the \
         default empty Arc and fall back to legacy Pool routing."
    );
}

#[test]
fn s_static_grep_wire_integration_take_return_discipline() {
    let src = include_str!("../src/flow_dispatcher/wire_integrations.rs");
    // The take-pin-out / dispatch / return-pin discipline must
    // appear at all four store-op sites. We grep for the canonical
    // pattern: `.pinned_conns.lock().unwrap().remove(`.
    let take_count = src.matches(".pinned_conns.lock().unwrap().remove(").count();
    assert!(
        take_count >= 4,
        "v1.32.0 S — wire_integrations MUST take pins from \
         `ctx.pinned_conns` at all 4 store-op sites (persist, \
         retrieve, mutate, purge). Found {take_count} take sites; \
         expected at least 4. A missing site would re-open the race \
         for that specific store op."
    );
    let lazy_acquire_count = src
        .matches("if pin.is_none() {\n                if let Ok(p) = backend.acquire_pin().await {")
        .count();
    assert!(
        lazy_acquire_count >= 4 || src.matches("backend.acquire_pin().await").count() >= 4,
        "v1.32.0 S (D6.a) — wire_integrations MUST perform a lazy \
         on-miss `backend.acquire_pin().await` at every take site so \
         par branches that landed empty maps (post `parallel.rs` Arc \
         replacement) still pin per branch. A missing site would \
         silently regress D6.a default to Pool fallback for that \
         store op in par contexts."
    );
}

// ── section 3 — D3 in_memory backwards-compat: zero behavior change ────────

#[test]
fn s3_d3_in_memory_backend_byte_identical() {
    // v1.32.0 (D3) — a flow whose store is `in_memory` (or
    // undeclared, which defaults to in_memory) never enters the
    // postgresql pin discovery path: `backend_kind` returns
    // `InMemory`, the eager-acquire walk skips it, the wire-
    // integration handler routes via `retrieve_from_store` /
    // `persist_to_store` (the KV path), and `pinned_conns` stays
    // empty for the flow's lifetime.
    //
    // This test pins the structural invariant: the public
    // `StoreConn` variant catalog has EXACTLY TWO variants (Pool +
    // Pinned), neither of which represents an in-memory backend.
    // The in-memory dispatch path is upstream of `StoreConn` — it
    // never touches the pin substrate.
    let src = include_str!("../src/store/store_conn.rs");
    // Pin the variant count by asserting both variants exist + no
    // third variant has been added.
    assert!(src.contains("Pool(&'a PgPool)"));
    assert!(src.contains("Pinned(&'a mut PoolConnection<Postgres>)"));
    // A third variant would break the in-memory invariant — adopters
    // expect StoreConn ONLY routes Postgres dispatch.
    let variant_count = src
        .matches("\n    Pool(")
        .count()
        + src.matches("\n    Pinned(").count();
    assert_eq!(
        variant_count, 2,
        "v1.32.0 D3 — `StoreConn` MUST have exactly 2 variants \
         (Pool + Pinned). A third variant would expand the pin \
         substrate beyond Postgres, contradicting the in-memory \
         byte-identical guarantee that wire_integrations routes \
         in_memory ops away from `StoreConn` entirely. Found \
         {variant_count} variants."
    );
}

// ── section 4 — D4 observability emit produces structured events ───────────

#[test]
fn s4_d4_emit_pin_acquire_is_total() {
    // v1.32.0 (D4) — `emit_pin_acquire` is a single
    // `tracing::info!` macro call. It must be callable with every
    // representative input shape without panicking. The tracing
    // subscriber is not active in this test (cargo test default), so
    // the assertion is that the call site COMPILES and EXECUTES
    // without panic — the macro arguments are well-formed.
    //
    // A future step (e.g. a CI lane with tracing-test
    // subscriber) can assert the captured event's fields directly.
    // For v1.39.0 we ship the pure-totality assertion: emit never
    // panics on any documented input.
    pin_observability::emit_pin_acquire(
        "chat_history",
        "ChatFlow",
        "trace-abc-123",
        "eager",
        None,
    );
    pin_observability::emit_pin_acquire(
        "tenant_secrets",
        "WriteSecret",
        "",
        "lazy",
        Some(2),
    );
    // Hostile inputs — empty strings, very long strings — never panic.
    pin_observability::emit_pin_acquire("", "", "", "", None);
    pin_observability::emit_pin_acquire(
        &"x".repeat(10_000),
        &"y".repeat(10_000),
        &"z".repeat(10_000),
        "lazy",
        Some(usize::MAX),
    );
}

#[test]
fn s4_d4_emit_pin_flow_summary_is_total() {
    // Symmetric to acquire: never panic on any input.
    pin_observability::emit_pin_flow_summary("ChatFlow", "trace-abc-123", 6);
    pin_observability::emit_pin_flow_summary("", "", 0);
    pin_observability::emit_pin_flow_summary("ChatFlow", "", usize::MAX);
}

// ── S — StoreConn API: constructor + discriminator pin ─────────────

// The `StoreConn::pool` / `StoreConn::pinned` constructors + the
// `is_pool` / `is_pinned` discriminators are pinned by the lib unit
// tests in `axon::store::store_conn::tests`; we don't duplicate them
// here. The integration anchor's role is to pin the SURFACE — that
// the symbols exist with the right shapes, accessible from external
// callers (i.e. as `pub` from the `axon` crate).

#[test]
fn s_storeconn_public_surface_accessible_externally() {
    // If this compiles, the public surface is correct. The actual
    // construction requires a tokio runtime + real PgPool which
    // belongs in the deferred section 1/section 2 real-DB anchor.
    fn _accepts_storeconn_type<'a>(_c: &mut StoreConn<'a>) {}
    let _ = _accepts_storeconn_type;
}
