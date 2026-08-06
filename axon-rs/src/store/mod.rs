//! §Fase 35 — the `axonstore` cognitive data plane runtime.
//!
//! `axonstore` is reframed in Fase 35 from an ignored declaration into
//! a load-bearing runtime primitive: a persistent relation that is
//! epistemically typed, audit-chained by construction, streamable, and
//! capability-secured (the plan vivo's four pillars).
//!
//! This module is built **Rust-canonical** per the 0-Python strategic
//! direction — the Python `axon/runtime/store_backends/` modules are
//! the historical reference this cycle learns from, frozen.
//!
//! # Sub-module map (sequenced per the plan vivo §5)
//!
//! - [`filter`] — 35.b — the parameterized `where`-expression filter
//!   compiler. SQL-injection-proof by construction (D4).
//! - `postgres_backend` — 35.c — the `sqlx::PgPool` SQL substrate.
//! - `registry` — 35.d — closed-catalog `store_name` → backend dispatch.
//! - `epistemic` — 35.g — Pillar I, the ESK trust lattice join.
//! - `audit_chain` — 35.h — Pillar II, the HMAC-Merkle mutation chain.
//! - `row_stream` — 35.i — Pillar III, `retrieve` as a `Stream<Row>`.

pub mod audit_chain;
pub mod capability;
pub mod epistemic;
/// §Fase 118.b.3 — the axonstore error catalog, dependency-free. See the module
/// docs: it lived in `postgres_backend` and the driver gate took it hostage.
pub mod error;
/// §Fase 118.b.3 — the row shape + pool sizing, dependency-free. See the docs.
pub mod row;
/// §Fase 118.b.3 — the §96 pooler-mode decision, dependency-free.
pub mod pooler_mode;
pub mod filter;
#[cfg(feature = "postgres")]
pub mod introspect_cli;
#[cfg(feature = "postgres")]
pub mod postgres_backend;

/// §Fase 118.b.3 — the absent-driver stand-in.
///
/// Without `postgres` there is no backend, and this states it in the type
/// system: [`PostgresStoreBackend`](postgres_backend::PostgresStoreBackend) is
/// **uninhabited**. That is what keeps the gate out of the cognition path.
/// `StoreHandle::Postgres(PostgresStoreBackend)` stays a declared variant in
/// every profile — so `StoreHandle` needs no `#[cfg]`, its `is_postgres()` still
/// answers, and `axon check` still type-checks a flow that declares
/// `backend: postgresql`. What changes is that the variant can never be
/// CONSTRUCTED, which makes every SQL-dispatch arm unreachable and lets the
/// compiler prove the remaining match is exhaustive.
///
/// The same trick as [`crate::pinned_conn::PinnedConn`], one level up: say the
/// impossible thing is impossible, rather than duplicating every signature that
/// mentions it (D118.2's rejected option (ii)).
#[cfg(not(feature = "postgres"))]
pub mod postgres_backend {
    /// An `axonstore` Postgres backend — unconstructible in this build.
    ///
    /// The derives are free on an uninhabited type (every impl body is
    /// unreachable) and they matter: `StoreHandle` derives `Debug` + `Clone`, so
    /// without them the CATALOG would need gating rather than just this variant.
    #[derive(Debug, Clone)]
    pub enum PostgresStoreBackend {}

    /// Resolve an `axonstore`'s `connection:` to a DSN — the REFUSAL.
    ///
    /// The registry calls this before it would build a pool. Without the driver
    /// there is nothing to connect to, so it refuses here, in writing, naming the
    /// exact reinstall command — rather than letting the store resolve and fail
    /// somewhere less legible. §111 doctrine: the advertised surface stays
    /// advertised and says how to get the implementation back.
    pub fn resolve_dsn(_connection: &str) -> Result<String, super::error::StoreError> {
        Err(super::error::StoreError::Connect {
            source: "this build was compiled without the `postgres` feature, so no                      PostgreSQL driver is linked and a `backend: postgresql` axonstore                      cannot be opened. Reinstall with: cargo install axon-lang --features postgres                      (`axon check` type-checks the declaration in every build; only                      executing against a live database needs the driver.)"
                .to_string(),
        })
    }
}
pub mod registry;
pub mod pin_observability;
#[cfg(feature = "postgres")]
pub mod row_stream;
#[cfg(feature = "postgres")]
pub mod store_conn;
