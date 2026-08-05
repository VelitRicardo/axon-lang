//! §Fase 118.a / D118.2 — the pinned-connection PORT.
//!
//! # What this fixes
//!
//! The §37.x.j Connection-Pinned Flow Execution Contract pins ONE physical
//! Postgres connection per `axonstore` for a flow's lifetime, so a
//! transaction-mode pooler (Supavisor, PgBouncer `pool_mode=transaction`, Neon,
//! RDS Proxy) cannot swap the backend between two queries that must observe each
//! other. The pins are carried in a map threaded through the executor:
//!
//! ```text
//! pinned_conns: &mut HashMap<String, sqlx::pool::PoolConnection<sqlx::Postgres>>
//! ```
//!
//! That parameter runs through `runner.rs` → `flow_dispatcher` →
//! `streaming_via_dispatcher` — **the cognition path** — which means a concrete
//! database type was embedded in the signatures of a language that speaks
//! through ports for everything else it touches (`StorageBackend`,
//! `QuantBackend`, `BreachSink`, `SynthBackend`). §118 made that urgent; it did
//! not make it wrong. It was already wrong.
//!
//! # Why a newtype and not a trait
//!
//! D118.2 offered three ways out. The measurement that picked this one is that
//! **the executor never calls a method on a pinned connection.** Its entire
//! relationship to a pin is custodial:
//!
//!   1. `remove` it from the map for the duration of one store dispatch,
//!   2. lend a `&mut` to the store layer as a [`StoreConn`],
//!   3. `insert` it back, unconditionally, on success *or* error.
//!
//! Nothing is ever queried through it outside `store/`. A trait would therefore
//! be a vtable with no methods worth dispatching, plus a `Box` allocation in the
//! hot path; `Box<dyn Any>` (option iii) would launder a compile-time guarantee
//! into a runtime downcast, which is against the whole thesis; and `#[cfg]` on
//! the signatures (option ii) would leave the executor with two shapes to keep
//! in sync forever. An opaque newtype gives the executor a name it can hold
//! without knowing what is inside — which is all it ever needed.
//!
//! # Why it lives at the crate root and not in `store/`
//!
//! Because it is the PORT, and a port must be nameable without its
//! implementation. §118 has now found four cases of a general concept parked in
//! the specific module that first needed it (`AXON_VERSION` in the flow
//! executor, `IngestProvenance` in the OOXML reader, `ServerExecutionResult` in
//! the HTTP server, the tenant task-local next to an axum middleware). Putting
//! the pin port inside `store/` would be the fifth.
//!
//! # §118.b.3 — what changes here when `postgres` becomes a feature
//!
//! This module is the seam that makes the gate a small diff instead of a
//! refactor. The intended shape:
//!
//! ```ignore
//! #[cfg(feature = "postgres")]
//! pub struct PinnedConn(sqlx::pool::PoolConnection<sqlx::Postgres>);
//!
//! #[cfg(not(feature = "postgres"))]
//! pub enum PinnedConn {}   // uninhabited
//! ```
//!
//! An uninhabited `PinnedConn` is not a trick — it is the truth stated in the
//! type system. A build without Postgres can hold a `HashMap<String, PinnedConn>`
//! that is *provably always empty*, so **every executor signature stays
//! byte-identical across both profiles** and not one `#[cfg]` enters the
//! cognition path. That is the property option (ii) could never have.

use crate::store::store_conn::StoreConn;

/// A Postgres connection pinned for the lifetime of one flow's use of one
/// `axonstore` (§Fase 37.x.j D1).
///
/// Opaque by construction: the inner handle is private, so the executor can
/// carry a pin without naming — or linking — a database driver. The only way to
/// *use* one is [`PinnedConn::as_store_conn`], which hands the borrow to the
/// store layer where the sqlx types legitimately live.
///
/// §Fase 96 — a pin is not always granted. Under a SESSION-mode pooler or a
/// direct connection every pooled connection is already a coherent session, so
/// `acquire_pin` REFUSES and callers fall through to their per-op
/// `StoreConn::Pool` path. That is the `connections_release_across_cognition`
/// doctrine: holding a scarce connection across a flow's LLM steps starves the
/// pool. The `Option<PinnedConn>` the executor carries is therefore genuinely
/// optional — `None` is a supported, common, correct state, not a failure.
pub struct PinnedConn(sqlx::pool::PoolConnection<sqlx::Postgres>);

impl PinnedConn {
    /// Wrap a freshly-acquired pooled connection.
    ///
    /// Called only by `store::postgres_backend::acquire_pin`, which owns the
    /// §96 decision about whether a pin should exist at all.
    pub fn new(conn: sqlx::pool::PoolConnection<sqlx::Postgres>) -> Self {
        PinnedConn(conn)
    }

    /// Borrow this pin as a [`StoreConn`] for one store operation.
    ///
    /// This is the port's whole surface. The returned `StoreConn::Pinned`
    /// re-borrows per query (see `store_conn.rs` on why sqlx's by-value
    /// `Executor` forced that design), so the caller can run a cache-HIT
    /// `fetch_all`, fall through to a `begin()` + introspect + retry, and still
    /// hand the pin back to the map afterwards.
    pub fn as_store_conn(&mut self) -> StoreConn<'_> {
        StoreConn::pinned(&mut self.0)
    }
}

impl std::fmt::Debug for PinnedConn {
    /// Deliberately says nothing about the connection.
    ///
    /// A pin's `Debug` reaching a log line is how a DSN — credentials included —
    /// escapes into an audit trail that §Fase 106 promises is safe to hand a
    /// regulator. The executor logs store names, never handles.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PinnedConn(<postgres connection>)")
    }
}

#[cfg(test)]
mod tests {
    /// The source of this module ABOVE the test module — i.e. the code that
    /// actually ships.
    ///
    /// **This helper exists because the first draft of these tests failed on
    /// itself.** `as_store_conn_is_the_only_accessor` scans for the literal
    /// `"fn inner("`; that literal is written *in this file*, inside the very
    /// array the test iterates, so `include_str!` handed the assertion its own
    /// source and it reported a leak that does not exist.
    ///
    /// That is exactly the §118.a analyser defect — where a doc comment naming
    /// `crate::runner::AXON_VERSION` while *explaining the fix* manufactured the
    /// dependency edge it was describing — and §118.b.2 hit the same shape a
    /// third time, in test classification, when seven files were miscounted
    /// because a grep matched `axum` inside comments. A source-scanning gate
    /// must exclude the region where it names its own patterns, or it measures
    /// itself.
    /// Comments are stripped too, so prose that *names* an escape hatch while
    /// explaining why it is forbidden cannot be mistaken for one.
    fn shipped_source() -> String {
        include_str!("pinned_conn.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("the file always has content before the test module")
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The port's load-bearing property is a NEGATIVE one: the executor must not
    /// be able to reach the driver through it. That cannot be asserted at
    /// runtime, so it is asserted structurally — this module's source declares
    /// exactly one field, and it is private.
    #[test]
    fn the_inner_handle_is_private() {
        let src = shipped_source();
        assert!(
            src.contains("pub struct PinnedConn(sqlx::pool::PoolConnection<sqlx::Postgres>);"),
            "§118 D118.2 — the inner handle must stay a PRIVATE tuple field. \
             Making it `pub` would re-open the leak this port exists to close: \
             the executor could then name `sqlx::Postgres` again through the \
             newtype, and the §118.b.3 uninhabited-variant trick would stop \
             compiling because callers would depend on the field."
        );
    }

    /// `as_store_conn` is the ONLY way out. If a second accessor appears —
    /// `inner()`, `into_inner()`, `AsMut`, `Deref` — the executor regains the
    /// ability to hold an sqlx type and the port becomes decoration.
    #[test]
    fn as_store_conn_is_the_only_accessor() {
        let src = shipped_source();

        // Match the CONCEPT, not one spelling of it. The first version of this
        // list held `"impl Deref"`, and an injected
        // `impl std::ops::Deref for PinnedConn` sailed straight through — a gate
        // that does not catch the thing it names is decoration. Verified by
        // injecting each of these and watching the test go red.
        let escapes = ["Deref", "AsMut", "AsRef", "BorrowMut", "into_inner", "fn inner"];
        for e in escapes {
            assert!(
                !src.contains(e),
                "§118 D118.2 — `{e}` would hand the raw `PoolConnection` back to \
                 callers and re-open the leak this port exists to close. The \
                 port's surface is `new` + `as_store_conn`, deliberately."
            );
        }

        // And positively: exactly two public functions, by name.
        let pub_fns: Vec<&str> = src
            .lines()
            .filter(|l| l.trim_start().starts_with("pub fn "))
            .collect();
        assert_eq!(
            pub_fns.len(),
            2,
            "§118 D118.2 — `PinnedConn` must expose exactly `new` and \
             `as_store_conn`. A third public function is how a port becomes a \
             leak again, one convenience at a time. Found: {pub_fns:?}"
        );
    }
}
