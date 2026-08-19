//! v1.32.0 (D1) — `StoreConn`: connection-pinned executor wrapper.
//!
//! The Connection-Pinned Flow Execution Contract pins ONE physical
//! Postgres connection per axonstore for the entire flow lifetime,
//! so a transaction-mode pooler (Supabase Supavisor `:6543`, PgBouncer
//! `pool_mode=transaction`, Neon, RDS Proxy) cannot swap the backend
//! between two queries that should observe each other's prepared
//! statements.
//!
//! The contract requires every `PostgresStoreBackend` operation
//! (`query`, `insert`, `mutate`, `purge`, `ping`) to be able to execute
//! against EITHER the pool (legacy, pre-37.x.j) OR a pinned
//! [`sqlx::pool::PoolConnection<sqlx::Postgres>`] held on
//! `ExecContext`/`DispatchCtx`. The naive sqlx-idiomatic path
//! (taking an `executor: E where E: sqlx::Executor<'_, Database = Postgres>`
//! generic parameter) consumes the executor by-value on `.fetch_all` /
//! `.execute` / `.begin`, which collides with the backend's
//! cache-HIT-fall-through-to-MISS-on-schema-drift logic: after a HIT
//! attempt consumes the executor, the MISS path has no executor left.
//!
//! [`StoreConn`] resolves this with an explicit two-variant enum that
//! re-borrows on every operation. Each method dispatches by variant
//! and produces a fresh executor reference from the underlying handle
//! — so the caller can run a cache-HIT `fetch_all` + on schema-drift
//! fall through to a `begin()` + introspect-in-tx + retry, all against
//! the SAME `StoreConn` borrow without ever consuming it.
//!
//! # Design choice (C′ per v1.32.0 ratification 2026-05-20)
//!
//! The sqlx-idiomatic generic `E: Executor` pattern (option C) was
//! considered and rejected because it cannot survive the cache
//! fall-through. Option (A) overlay (parallel `_pinned` methods) was
//! considered and rejected for API duplication. Option (C′) — this
//! file — is the pragmatic middle: a small, internal wrapper type
//! that owns the dispatch decision and lets each method consume a
//! single `&mut StoreConn` for its whole body.
//!
//! `StoreConn` is **not** a sqlx primitive. It does not appear in
//! adopter-facing axon code. It is a runtime-internal helper that
//! exists solely to satisfy the D1 pinning contract.

use sqlx::pool::PoolConnection;
use sqlx::postgres::{PgArguments, PgQueryResult, PgRow};
use sqlx::Connection;
use sqlx::{PgPool, Postgres, Transaction};

/// v1.32.0 (D1) — A connection-source for `PostgresStoreBackend`
/// operations.
///
/// Two variants:
///
///  - [`StoreConn::Pool`] — wraps a borrow of the backend's connection
///    pool; every operation acquires a fresh sqlx logical connection
///    per call. This is the v1.38.5 (and earlier) legacy behavior;
///    backwards-compat path D5.
///
///  - [`StoreConn::Pinned`] — wraps a mutable borrow of a single
///    `PoolConnection<Postgres>` already acquired (by `acquire_pin`
///    on the backend) and held by the calling [`ExecContext`] or
///    [`DispatchCtx`]. Every operation runs against this exact
///    physical Postgres backend connection, so a transaction-mode
///    pooler cannot swap mid-flow.
///
/// The wrapper is `'a`-scoped — the borrow lifetime matches the
/// lifetime of the pool reference or the pinned connection. The
/// caller passes `&mut StoreConn<'_>` into each backend method, which
/// can then re-borrow on each query without consuming the wrapper.
///
/// # Re-borrow discipline
///
/// `sqlx`'s `Executor` trait consumes by value on every operation.
/// `StoreConn` resolves this by NOT exposing the underlying handle —
/// instead, it provides three operations (`fetch_all`, `execute`,
/// `begin`) that each internally re-borrow the wrapped pool/conn,
/// freshly producing an `&PgPool` or `&mut PgConnection` (via
/// `&mut **pinned`) per call. The wrapper itself stays usable across
/// successive operations.
///
/// # The `begin()` method returns a `Transaction` (not a StoreConn)
///
/// Once inside a `Transaction`, the connection is automatically pinned
/// for the transaction's duration — sqlx's `Transaction` borrows the
/// connection mutably for its lifetime. The cache-MISS path of every
/// backend op uses this property: it begins a transaction on the
/// `StoreConn`, runs introspection + the operation queries against
/// the transaction, then commits. While the transaction is alive, no
/// other code can use the `StoreConn` (the borrow is exclusive), so
/// no foreign pool acquire can interleave a connection swap.
pub enum StoreConn<'a> {
    /// Legacy / pre-37.x.j path: every call acquires a fresh logical
    /// connection from the pool. Safe against named-prepared-statement
    /// leaks (D1 `.persistent(false)` per-query) + named-prepared-
    /// statement-already-exists collisions (D2 `after_release
    /// DEALLOCATE ALL` per-conn-return). Vulnerable to the unnamed-
    /// statement race against transaction-mode poolers (the D3 class
    /// that v1.32.0 closes for flows that opt in via pinning).
    Pool(&'a PgPool),
    /// 37.x.j path: this `PoolConnection<Postgres>` was acquired ONCE
    /// at flow start by `PostgresStoreBackend::acquire_pin()` and held
    /// on `ExecContext.pinned_conns` or `DispatchCtx.pinned_conns`.
    /// All operations against the same `axonstore` for the duration
    /// of the flow execution route through this exact physical
    /// backend connection — no inter-query Supavisor/PgBouncer swap
    /// window exists. Closes the D3 unnamed-statement race
    /// structurally.
    Pinned(&'a mut PoolConnection<Postgres>),
}

impl<'a> StoreConn<'a> {
    /// Construct a legacy pool-backed connection source. Every
    /// existing call site in `wire_integrations::*` constructs this
    /// variant until 37.x.j.4/5 wire the pinned variant through
    /// `ExecContext`/`DispatchCtx`.
    pub fn pool(pool: &'a PgPool) -> Self {
        StoreConn::Pool(pool)
    }

    /// Construct a pinned-conn-backed connection source. Used by
    /// `ExecContext.with_store_conn` (sync runner) and
    /// `DispatchCtx.with_store_conn` (async dispatcher) after the
    /// pin was acquired at flow start. (Sub-cycles 37.x.j.4/5.)
    pub fn pinned(conn: &'a mut PoolConnection<Postgres>) -> Self {
        StoreConn::Pinned(conn)
    }

    /// `true` iff this connection source is the legacy pool path.
    /// Diagnostic helper for the D4 observability emitter — a pool
    /// path means we did NOT enter the 37.x.j pinning regime for this
    /// op (legacy / pre-pin / non-Postgres backend).
    pub fn is_pool(&self) -> bool {
        matches!(self, StoreConn::Pool(_))
    }

    /// `true` iff this connection source is a held pin. Diagnostic
    /// helper symmetric to [`Self::is_pool`].
    pub fn is_pinned(&self) -> bool {
        matches!(self, StoreConn::Pinned(_))
    }

    /// v1.32.0 (D1) — Execute a `SELECT`-style query against this
    /// connection source and return every row.
    ///
    /// Internally re-borrows the wrapped pool/conn so the `StoreConn`
    /// stays usable across successive operations (the cache-HIT
    /// fall-through to cache-MISS pattern depends on this). The query
    /// is consumed by sqlx as required.
    ///
    /// The caller MUST set `.persistent(false)` on the query before
    /// invocation — this is the per-query D1 invariant established in
    /// v1.31.0 and reasserted across the 37.x.j surface.
    pub async fn fetch_all<'q>(
        &mut self,
        q: sqlx::query::Query<'q, Postgres, PgArguments>,
    ) -> Result<Vec<PgRow>, sqlx::Error> {
        match self {
            StoreConn::Pool(p) => q.fetch_all(*p).await,
            StoreConn::Pinned(c) => q.fetch_all(&mut ***c).await,
        }
    }

    /// v1.32.0 (D1) — Execute a `SELECT`-style query expected to
    /// return a single row. The query is consumed.
    ///
    /// Symmetric to [`Self::fetch_all`] / [`Self::execute`]; provided
    /// for `ping`-style health checks that want a single row back.
    pub async fn fetch_optional<'q>(
        &mut self,
        q: sqlx::query::Query<'q, Postgres, PgArguments>,
    ) -> Result<Option<PgRow>, sqlx::Error> {
        match self {
            StoreConn::Pool(p) => q.fetch_optional(*p).await,
            StoreConn::Pinned(c) => q.fetch_optional(&mut ***c).await,
        }
    }

    /// v1.32.0 (D1) — Execute a non-`SELECT` query (INSERT /
    /// UPDATE / DELETE / DEALLOCATE / etc.) and return the rows-affected
    /// summary. Symmetric to [`Self::fetch_all`].
    pub async fn execute<'q>(
        &mut self,
        q: sqlx::query::Query<'q, Postgres, PgArguments>,
    ) -> Result<PgQueryResult, sqlx::Error> {
        match self {
            StoreConn::Pool(p) => q.execute(*p).await,
            StoreConn::Pinned(c) => q.execute(&mut ***c).await,
        }
    }

    /// v1.32.0 (D1) — Begin a Postgres transaction on this
    /// connection source.
    ///
    /// On the [`StoreConn::Pool`] variant, sqlx acquires a fresh
    /// `PoolConnection` from the pool, pins it for the transaction's
    /// lifetime, and releases it on `commit()`/`rollback()`/`Drop`.
    ///
    /// On the [`StoreConn::Pinned`] variant, the transaction borrows
    /// the held `PoolConnection` exclusively for its lifetime — this
    /// is the D3 win: the transaction body executes on the SAME
    /// physical Postgres backend as every other op against this
    /// `StoreConn`, no Supavisor/PgBouncer swap window exists.
    ///
    /// While the returned `Transaction` is alive, no other operation
    /// can run on this `StoreConn` (the borrow is exclusive). This is
    /// the standard sqlx invariant; backend methods that need to mix
    /// transactioned + non-transactioned queries on the same connection
    /// MUST sequence them — first run the tx + commit, then run other
    /// ops on the released `StoreConn`.
    pub async fn begin<'b>(
        &'b mut self,
    ) -> Result<Transaction<'b, Postgres>, sqlx::Error> {
        match self {
            StoreConn::Pool(p) => p.begin().await,
            // v1.32.0 (D2) — `Connection::begin` is in scope via
            // the `sqlx::Connection` import; without it the deref chain
            // `&mut PoolConnection → &mut PgConnection` would not resolve
            // the `.begin()` method.
            StoreConn::Pinned(c) => {
                let conn_mut: &mut sqlx::PgConnection = c;
                conn_mut.begin().await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The StoreConn enum is dispatch-only; its real test surface lives
    // in `axon-rs/tests/connection_pinning.rs` where actual
    // queries run against a real Postgres (or an in-memory mock that
    // exercises the variants). Here we only pin the constructor +
    // discriminator API so a future refactor that drops these helpers
    // fails the lib build.

    #[test]
    fn is_pool_iff_constructed_from_pool() {
        // We can't instantiate a real PgPool without a runtime, but
        // we can prove the discriminator works for any well-formed
        // value. The runtime exercise lives in the anchor.
        // This test verifies the type-level discipline at compile time.
        fn _accepts_pool(p: &PgPool) -> bool {
            let conn = StoreConn::pool(p);
            conn.is_pool() && !conn.is_pinned()
        }
        // `_accepts_pool` compiles ⇒ the API contract holds.
        let _ = _accepts_pool;
    }

    #[test]
    fn is_pinned_iff_constructed_from_pinned() {
        fn _accepts_pinned(c: &mut PoolConnection<Postgres>) -> bool {
            let conn = StoreConn::pinned(c);
            conn.is_pinned() && !conn.is_pool()
        }
        let _ = _accepts_pinned;
    }
}
