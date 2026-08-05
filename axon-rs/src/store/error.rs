//! §Fase 118.b.3 — the `axonstore` ERROR CATALOG, in a module that links no driver.
//!
//! **The fifth instance of the smell, and §118 said to expect it.** `StoreError`
//! lived in `store/postgres_backend.rs` — so `runner.rs`, `wire_integrations.rs`,
//! `registry.rs` and the §113 lease kernel all imported the general error type of
//! every store operation *from the Postgres driver module*, and gating that module
//! took the error catalog with it.
//!
//! It was never a Postgres type. The enum and its five impls contain **zero**
//! `sqlx` references, and its variants are mostly about governance rather than
//! SQL: `LeaseExpired` is §113's CT-2 Anchor Breach (a τ-decaying affine
//! capability used after expiry), `Filter` delegates to the §35.b WHERE algebra,
//! `Epistemic` to §35.g's Pillar I confidence floor. `postgres_backend` was
//! simply the first module that needed somewhere to put a failure.
//!
//! `postgres_backend` re-exports it, so every existing call site — including
//! `axon-enterprise`'s — keeps resolving verbatim.
//!
//! **This module must never acquire a dependency.**

use super::epistemic::EpistemicError;
use super::filter::FilterError;
use std::fmt;

/// Every way an `axonstore` operation can fail. The backend is total: it returns
/// one of these or a result — never a panic, never a silent empty result masking
/// a failure.
#[derive(Debug, Clone, PartialEq)]
pub enum StoreError {
    /// §Fase 113.d — **the CT-2 Anchor Breach.** A store operation was attempted
    /// against a resource whose `lease` has expired.
    ///
    /// The README has always promised this: a `lease` is a τ-decaying affine
    /// capability, and *post-expiry USE* is an Anchor Breach. The kernel that
    /// raises it was complete years ago. What did not exist was **a moment at
    /// which a resource could be used** — a flow could not `use` a `resource`, so
    /// the breach had nowhere to fire. §113 made the store operation that moment.
    LeaseExpired {
        store: String,
        resource: String,
        lease: String,
        detail: String,
    },
    /// `connection` was empty or whitespace-only.
    EmptyConnection,
    /// `connection` was the bare prefix `env:` with no variable name.
    EmptyEnvVarName,
    /// `connection: "env:VAR"` and `VAR` is unset (or not UTF-8).
    MissingEnvVar { var: String },
    /// The resolved DSN is malformed — `connect_lazy` rejected it.
    PoolInit { dsn_masked: String, source: String },
    /// A table or column identifier failed the `[A-Za-z_]\w*` / 63-byte
    /// safety check (D4 — no untrusted identifier reaches SQL).
    InvalidIdentifier { kind: &'static str, name: String },
    /// `insert` / `mutate` was called with no column data.
    EmptyData { op: &'static str },
    /// The `where` expression did not compile (delegates to 35.b).
    Filter(FilterError),
    /// A `confidence_floor` violation — a sub-floor or un-elevated
    /// `persist` (delegates to 35.g's Pillar I epistemic data plane).
    Epistemic(EpistemicError),
    /// A live connection could not be acquired / the ping failed.
    Connect { source: String },
    /// A SQL statement failed at execution time.
    Query { op: &'static str, source: String },
    /// A retrieved column has a type outside the supported catalog
    /// ([`classify_pg_type`]). Honest scope, not a silent miss.
    UnsupportedColumnType { column: String, pg_type: String },
    /// A retrieved column of a supported type failed to decode.
    Decode { column: String, pg_type: String, source: String },
    /// §Fase 37.x.b (D1) — the table named by a store operation could
    /// not be resolved to a relation in ANY schema of the database.
    TableNotResolved { table: String },
    /// §Fase 37.x.b (D1) — the table name resolves to a relation in
    /// more than one schema and the connection's `search_path` does not
    /// disambiguate it. Carries the schemas found, sorted.
    AmbiguousTable { table: String, schemas: Vec<String> },
    /// §Fase 37.x.f (D9) — a store SQL statement failed with a
    /// schema-drift SQLSTATE: the cached schema no longer matches the
    /// live table (an `ALTER TABLE` ran since the cache was populated).
    /// `42P01` undefined_table, `42703` undefined_column, `42804`
    /// datatype_mismatch (a stale write cast), `42883` undefined
    /// operator (a stale read cast). Triggers the D9 self-heal — the
    /// `(dsn, table)` cache entry is evicted and the operation retried
    /// once against fresh introspection. Safe: every one is a
    /// parse/plan-time rejection, so the failed statement had ZERO side
    /// effects (a retried `persist`/`mutate` cannot double-write).
    SchemaDrift { op: &'static str, sqlstate: String, source: String },
    /// §Fase 38.f (D3) — `axon-T806`. A `postgresql` store declared
    /// `schema: env:VAR` and the named env var is unset at deploy
    /// time. Never falls back silently — the deploy fails, the
    /// operator either exports the var or fixes the declaration.
    MissingPerTenantSchemaEnv { store: String, var: String },
    /// §Fase 38.f (D8 strengthening) — `axon-T807`. A declared column
    /// schema and the live introspected columns disagree at deploy
    /// time. Carries a human-readable drift summary (which columns
    /// are missing on the live DB, which have a type mismatch). The
    /// remedy is named in the message: run `axon store introspect
    /// <store>` to refresh the manifest, run the missing migration,
    /// or fix the declaration.
    DeclaredVsLiveDrift { store: String, drift: String },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::LeaseExpired {
                store,
                resource,
                lease,
                detail,
            } => write!(
                f,
                "CT-2 ANCHOR BREACH — axonstore `{store}` was used, but the `lease {lease}` over \
                 its resource `{resource}` is no longer held: {detail}. A lease is a τ-decaying \
                 affine capability: using the resource after expiry is the breach, and this is \
                 the moment it fires. (Until §Fase 113 a flow could not USE a resource at all, \
                 so this guarantee was structurally impossible to violate — and therefore \
                 structurally impossible to keep.)"
            ),
            StoreError::EmptyConnection => write!(
                f,
                "axonstore `connection` is empty — expected a DSN or an \
                 `env:VARNAME` reference"
            ),
            StoreError::EmptyEnvVarName => write!(
                f,
                "axonstore `connection` is the bare prefix `env:` with no \
                 variable name"
            ),
            StoreError::MissingEnvVar { var } => write!(
                f,
                "axonstore `connection: \"env:{var}\"` — environment \
                 variable `{var}` is not set (or not valid UTF-8)"
            ),
            StoreError::PoolInit { dsn_masked, source } => write!(
                f,
                "axonstore connection pool could not be initialised for \
                 `{dsn_masked}`: {source}"
            ),
            StoreError::InvalidIdentifier { kind, name } => write!(
                f,
                "unsafe {kind} identifier `{name}` — must match \
                 [A-Za-z_][A-Za-z0-9_]* and be ≤ 63 bytes"
            ),
            StoreError::EmptyData { op } => write!(
                f,
                "axonstore `{op}` was given no column data"
            ),
            StoreError::Filter(e) => write!(f, "where-expression: {e}"),
            StoreError::Epistemic(e) => write!(f, "{e}"),
            StoreError::Connect { source } => {
                write!(f, "axonstore could not reach the database: {source}")
            }
            StoreError::Query { op, source } => {
                write!(f, "axonstore `{op}` SQL failed: {source}")
            }
            StoreError::UnsupportedColumnType { column, pg_type } => write!(
                f,
                "column `{column}` has Postgres type `{pg_type}`, outside \
                 the v1.30.0 supported catalog"
            ),
            StoreError::Decode { column, pg_type, source } => write!(
                f,
                "column `{column}` (`{pg_type}`) failed to decode: {source}"
            ),
            StoreError::TableNotResolved { table } => write!(
                f,
                "axonstore could not resolve table `{table}` to a \
                 relation in any schema of the database — verify the \
                 table exists in the target database (a deploy-time \
                 migration is the usual remedy) and that the configured \
                 credentials can SELECT from it; the introspection scans \
                 `pg_catalog` independent of `search_path`, so the table \
                 is genuinely absent on every schema this role can see"
            ),
            StoreError::AmbiguousTable { table, schemas } => write!(
                f,
                "axonstore table `{table}` is ambiguous — it exists in \
                 {} schemas ({}) and the connection's `search_path` does \
                 not disambiguate it; either narrow the role's \
                 `search_path` so exactly one of the resolving schemas \
                 is visible, or declare the target schema explicitly on \
                 the `axonstore` (the Fase 38 `schema:` declaration, \
                 incl. `schema: env:VAR` per-tenant)",
                schemas.len(),
                schemas.join(", "),
            ),
            StoreError::SchemaDrift { op, sqlstate, source } => write!(
                f,
                "axonstore `{op}` hit live schema drift (SQLSTATE \
                 {sqlstate}) — the cached schema is stale: {source}"
            ),
            StoreError::MissingPerTenantSchemaEnv { store, var } => write!(
                f,
                "axon-T806 axonstore `{store}` declares `schema: env:{var}` \
                 but environment variable `{var}` is not set at deploy \
                 time. The per-tenant schema namespace is required to \
                 resolve the store's column manifest entry. Either \
                 export `{var}` with the SQL schema name (e.g. \
                 `tenant_42`), or declare the schema differently \
                 (inline `schema {{ … }}` block, or manifest reference \
                 `schema: \"qualified.name\"`). Never a silent fallback."
            ),
            StoreError::DeclaredVsLiveDrift { store, drift } => write!(
                f,
                "axon-T807 axonstore `{store}` declared column schema \
                 disagrees with the live database: {drift}. The deploy \
                 fails fail-closed (D8 strengthening). Remedy: run `axon \
                 store introspect {store}` to refresh the manifest, run \
                 the missing migration on the database, or fix the \
                 declared `schema:` block to match the live shape."
            ),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoreError::Filter(e) => Some(e),
            StoreError::Epistemic(e) => Some(e),
            _ => None,
        }
    }
}

impl StoreError {
    /// §Fase 37.x.f (D9) — `true` iff this is a schema-drift failure
    /// ([`StoreError::SchemaDrift`]) — the signal that triggers the
    /// `(dsn, table)` cache self-heal (evict + retry once).
    pub fn is_schema_drift(&self) -> bool {
        matches!(self, StoreError::SchemaDrift { .. })
    }
}

impl From<FilterError> for StoreError {
    fn from(e: FilterError) -> Self {
        StoreError::Filter(e)
    }
}

impl From<EpistemicError> for StoreError {
    fn from(e: EpistemicError) -> Self {
        StoreError::Epistemic(e)
    }
}
