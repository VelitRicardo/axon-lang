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
pub mod introspect_cli;
pub mod postgres_backend;
pub mod registry;
pub mod pin_observability;
pub mod row_stream;
pub mod store_conn;
