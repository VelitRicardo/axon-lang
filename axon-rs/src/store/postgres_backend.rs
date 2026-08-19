//! §Fase 35.c (v1.30.0) — `PostgresStoreBackend`, the SQL substrate of
//! the `axonstore` cognitive data plane.
//!
//! This module makes `axonstore { backend: postgresql }` real: the four
//! store operations — `query` (retrieve), `insert` (persist), `mutate`,
//! `purge` — execute parameterized SQL against a `sqlx::PgPool` instead
//! of the key-value path. It is the substrate the four pillars (35.g-j)
//! enrich.
//!
//! # D6 — connection resolution
//!
//! [`resolve_dsn`] honors `connection: "env:VAR"` (resolve the named
//! environment variable) and a literal DSN. A missing env var is a
//! named [`StoreError::MissingEnvVar`] — never a panic, never a silent
//! fallback to the key-value store.
//!
//! # D7 — pooling + honest typed failure surface
//!
//! [`PostgresStoreBackend::connect`] builds ONE lazy, bounded
//! `sqlx::PgPool` (`connect_lazy_with` — no connection is opened until
//! the first operation). Every failure path — empty connection, missing
//! env var, malformed DSN, connect failure, SQL error, an unsupported
//! column type, a decode failure — surfaces as a typed [`StoreError`].
//! No panic; no silent empty result masking a failed query.
//!
//! # Gap 3 (v1.36.3) — transaction-mode pooler safety
//!
//! The pool's `PgConnectOptions` set `statement_cache_capacity(0)`
//! unconditionally. sqlx otherwise caches server-side prepared
//! statements under generated names (`sqlx_s_1`, …); behind a
//! transaction-mode pooler — PgBouncer `pool_mode=transaction`,
//! Supabase Supavisor (`:6543`), Neon, RDS Proxy — successive
//! operations land on different physical sessions, so a name minted on
//! one collides on the next (`prepared statement "sqlx_s_1" already
//! exists`). Capacity 0 routes every query through the *unnamed*
//! prepared statement — collision-free by construction, harmless on a
//! direct/session-mode connection. An axonstore DSN is pooler-agnostic
//! with no knob to misconfigure. Each connection also carries an
//! `application_name` of `axon-store/<store>` so every session is
//! attributable to its declaration in `pg_stat_activity`, pooler logs
//! and DBA dashboards.
//!
//! # §Fase 37.x.b — search-path-independent table resolution
//!
//! [`introspect_conn`] resolves a store table to
//! its schema + column types against `pg_catalog` — NOT via the
//! ambient `search_path`, which a transaction-mode pooler does not
//! preserve across checkouts. `to_regclass` is the search-path-correct
//! primary; a `pg_catalog` scan keyed on `relname` is the
//! search-path-independent fallback. An unresolved or cross-schema-
//! ambiguous table is a typed [`StoreError`].
//!
//! # D4 — injection-proof, identifiers included
//!
//! Values flow through 35.b's [`build_pg_where`] as `$N` bind
//! placeholders. The *identifier* surface — table names and
//! `insert`/`mutate` column names, which ARE interpolated into SQL
//! text — is validated against [`filter::is_safe_identifier`]
//! (`[A-Za-z_]\w*`, ≤ 63 bytes) before being double-quoted. No
//! untrusted identifier reaches SQL.
//!
//! # Architecture — pure builders + thin async execution
//!
//! SQL construction ([`build_select_sql`], [`build_insert_sql`],
//! [`build_update_sql`], [`build_delete_sql`]) is **pure and total** —
//! no I/O — and therefore exhaustively unit-tested here without a
//! database. The async methods are thin: build → bind → execute. The
//! row-decode path and live execution are proven against a real
//! Postgres in 35.l (the integration harness).
//!
//! # Honest scope (D12)
//!
//! No DDL: `IRAxonStore` carries no column schema, so v1.30.0 operates
//! against existing tables (no `CREATE TABLE` / `migrate` / index). Each
//! operation is a single-statement autocommit; the multi-statement
//! `transact { … }` block is a documented future fase. The supported
//! column-type catalog is [`classify_pg_type`]; a column outside it is
//! a clear [`StoreError::UnsupportedColumnType`], not a silent miss.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use serde_json::Value as JsonValue;
use sqlx::postgres::{PgArguments, PgConnectOptions, PgPoolOptions, PgRow};
use sqlx::query::Query;
use sqlx::{Column, PgConnection, PgPool, Postgres, Row, TypeInfo};

use crate::store::epistemic::EpistemicError;
use crate::store::filter::{self, build_pg_where, FilterError, SqlValue};

// §Fase 118.b.3 — the §96 pooler-mode decision MOVED to `store::pooler_mode`.
// It reads `AXON_DB_POOLER_MODE` and compares two strings; it is about
// DEPLOYMENT TOPOLOGY, not about a driver, and `runner`'s eager-pin loops ask it
// before they ask anything of Postgres. Re-exported so call sites keep resolving.
pub(crate) use super::pooler_mode::connection_pinning_enabled;

/// Upper bound on pooled connections per backend (D7 — bounded).
// §Fase 118.b.3 — MOVED to `store::row`. Its own doc comment said it was made
// `pub` "so the registry uses THIS constant rather than a copy of the number — a
// second copy of a fact is how the islands happened". Gating the driver would have
// forced exactly that copy back into existence.
pub use super::row::MAX_POOL_CONNECTIONS;
/// How long to wait to acquire a pooled connection before failing.
const ACQUIRE_TIMEOUT_SECS: u64 = 5;
/// How long an idle pooled connection is kept before being reaped.
const IDLE_TIMEOUT_SECS: u64 = 300;

// ════════════════════════════════════════════════════════════════════
//  Error catalog (typed, total — D7)
// ════════════════════════════════════════════════════════════════════

// §Fase 118.b.3 — the error catalog MOVED to `store::error`, a leaf module with
// no driver. It is the fifth instance of the §118 smell: a general concept —
// including §113's `LeaseExpired` anchor breach and the §35.b/§35.g
// delegations — parked in the module that first needed it, so gating the
// driver took the error type of every store operation with it.
// Re-exported so every call site keeps resolving.
pub use super::error::StoreError;

// ════════════════════════════════════════════════════════════════════
//  D6 — connection resolution
// ════════════════════════════════════════════════════════════════════

/// Resolve an `axonstore` `connection` string into a concrete DSN.
///
/// - `"env:VAR"` → the value of environment variable `VAR`.
/// - any other non-empty value → a literal DSN, returned verbatim.
///
/// Leading/trailing whitespace is trimmed. An empty connection, a bare
/// `env:`, or a missing environment variable is a typed [`StoreError`]
/// — never a panic, never a silent fallback.
pub fn resolve_dsn(connection: &str) -> Result<String, StoreError> {
    let conn = connection.trim();
    if conn.is_empty() {
        return Err(StoreError::EmptyConnection);
    }
    match conn.strip_prefix("env:") {
        Some(var) => {
            let var = var.trim();
            if var.is_empty() {
                return Err(StoreError::EmptyEnvVarName);
            }
            std::env::var(var).map_err(|_| StoreError::MissingEnvVar {
                var: var.to_string(),
            })
        }
        None => Ok(conn.to_string()),
    }
}

/// Mask the password segment of a DSN for safe logging / `Debug`.
fn mask_dsn(dsn: &str) -> String {
    if let Some(at) = dsn.find('@') {
        if let Some(colon) = dsn[..at].rfind(':') {
            return format!("{}***{}", &dsn[..=colon], &dsn[at..]);
        }
    }
    dsn.to_string()
}

/// §Fase 38.h — public alias of [`mask_dsn`] so the introspection
/// CLI (`store::introspect_cli`) can render error messages with
/// masked credentials without re-implementing the routine.
pub fn mask_dsn_pub(dsn: &str) -> String {
    mask_dsn(dsn)
}

/// The `application_name` stamped on an axonstore's Postgres
/// connections (Gap 3 bonus, v1.36.3).
///
/// `axon-store/<store_name>` makes every session attributable to its
/// `axonstore` declaration in `pg_stat_activity`, pooler logs and DBA
/// dashboards; a bare `axon-store` when no store name is available.
///
/// Total and bounded: Postgres caps `application_name` at
/// `NAMEDATALEN - 1` (63 bytes) and *silently truncates* a longer
/// value. This caps it ourselves on a UTF-8 char boundary so the
/// stamped name is exactly what a DBA sees — never a server-mangled
/// suffix.
// Kept for the unit tests pinning the v1.36.3 truncation shape (production
// callers use the namespace-aware variant below).
#[cfg_attr(not(test), allow(dead_code))]
fn application_name_for(store_name: &str) -> String {
    application_name_for_with_namespace(store_name, None)
}

/// §Fase 38.f (D3) — `application_name` stamping that optionally
/// carries a resolved per-tenant schema namespace (Gap-3 inheritance):
///
///   * `application_name_for_with_namespace("claims", None)` →
///     `"axon-store/claims"` (the existing v1.36.3 shape — preserved
///     byte-for-byte for non-namespace stores).
///   * `application_name_for_with_namespace("claims", Some("tenant_42"))`
///     → `"axon-store/claims/tenant_42"`.
///
/// A DBA reading `pg_stat_activity` or pooler logs sees both the
/// `axonstore` declaration AND the tenant namespace at a glance —
/// triaging a multi-tenant slow query stops requiring a join through
/// adopter telemetry.
///
/// Total + bounded: caps the result at `NAMEDATALEN - 1` (63 bytes)
/// on a UTF-8 char boundary, as v1.36.3 already does, so the stamped
/// name is exactly what Postgres records.
pub(crate) fn application_name_for_with_namespace(
    store_name: &str,
    namespace: Option<&str>,
) -> String {
    const MAX: usize = 63;
    let base = if store_name.is_empty() {
        "axon-store".to_string()
    } else {
        format!("axon-store/{store_name}")
    };
    let full = match namespace {
        Some(ns) if !ns.is_empty() => format!("{base}/{ns}"),
        _ => base,
    };
    if full.len() <= MAX {
        return full;
    }
    let mut cut = MAX;
    while cut > 0 && !full.is_char_boundary(cut) {
        cut -= 1;
    }
    full[..cut].to_string()
}

/// Validate a table / column identifier, mapping a failure to a typed
/// [`StoreError::InvalidIdentifier`] (D4).
fn check_identifier(name: &str, kind: &'static str) -> Result<(), StoreError> {
    if filter::is_safe_identifier(name) {
        Ok(())
    } else {
        Err(StoreError::InvalidIdentifier {
            kind,
            name: name.to_string(),
        })
    }
}

/// §Fase 37.x.c (D2) — render the SCHEMA-QUALIFIED relation reference
/// for an operation's SQL: `"schema"."table"` when the schema resolved
/// to a safe identifier, the bare `"table"` otherwise.
///
/// A schema-qualified reference resolves on ANY session regardless of
/// the ambient `search_path` — the D2 guarantee. The schema name is
/// discovered from `pg_catalog` (37.x.b's `resolve_table`); it is
/// validated with [`filter::is_safe_identifier`] before being
/// double-quoted (D4 — no untrusted identifier reaches SQL), exactly
/// as the table name is. When the schema is absent (`None` — the
/// resolution failed) or is not a safe identifier (an exotic quoted
/// schema name `pg_catalog` could yield), the reference falls back to
/// the bare `"table"` — never an unsafe splice, never a false error;
/// `search_path` then resolves it as in the pre-37.x behaviour. The
/// `table` is assumed already [`check_identifier`]-validated.
fn qualified_relation(schema: Option<&str>, table: &str) -> String {
    match schema {
        Some(s) if filter::is_safe_identifier(s) => {
            format!("\"{s}\".\"{table}\"")
        }
        _ => format!("\"{table}\""),
    }
}

// ════════════════════════════════════════════════════════════════════
//  Pure SQL builders (no I/O — exhaustively unit-tested)
// ════════════════════════════════════════════════════════════════════

/// Build a parameterized `SELECT * FROM "schema"."table" WHERE …`
/// statement.
///
/// §Fase 37.x.c (D2) — `schema` is the table's resolved schema (from
/// [`introspect_conn`]); when `Some` and a safe
/// identifier the relation is emitted SCHEMA-QUALIFIED so it resolves
/// on any session regardless of the ambient `search_path`. `None`
/// renders the bare `"table"` (the pre-37.x form — D5).
/// §Fase 37.d (D3) — `bindings` resolves `${name}` placeholders in the
/// `where` expression to `$N` bind parameters (never string-spliced).
/// §v1.36.4 — `column_types` (the `column → udt_name` map) lets
/// [`build_pg_where`] cast each `where`-clause value to its column's
/// Postgres type. Pass an empty map when the schema is unknown — the
/// filter then renders bare `$N` placeholders.
pub fn build_select_sql(
    table: &str,
    schema: Option<&str>,
    where_expr: &str,
    bindings: &std::collections::HashMap<String, String>,
    column_types: &std::collections::HashMap<String, String>,
) -> Result<(String, Vec<SqlValue>), StoreError> {
    check_identifier(table, "table")?;
    let (clause, params) = build_pg_where(where_expr, 0, bindings, column_types)?;
    let relation = qualified_relation(schema, table);
    Ok((format!("SELECT * FROM {relation} WHERE {clause}"), params))
}

/// §Fase 76.d — [`build_select_sql`] with a caller-supplied STRUCTURAL
/// select list (the aggregate SELECT: quoted group columns + the
/// labeled aggregate expression). The list MUST come from
/// [`crate::store::filter::render_aggregate_select`] — a closed-catalog
/// renderer over identifiers that passed `is_safe_identifier` at parse —
/// never adopter text, so the D4 injection invariant holds on this
/// surface exactly as on the `WHERE` clause.
pub fn build_aggregate_select_sql(
    table: &str,
    schema: Option<&str>,
    select_list: &str,
    where_expr: &str,
    bindings: &std::collections::HashMap<String, String>,
    column_types: &std::collections::HashMap<String, String>,
) -> Result<(String, Vec<SqlValue>), StoreError> {
    check_identifier(table, "table")?;
    let (clause, params) = build_pg_where(where_expr, 0, bindings, column_types)?;
    let relation = qualified_relation(schema, table);
    Ok((
        format!("SELECT {select_list} FROM {relation} WHERE {clause}"),
        params,
    ))
}

/// Build a parameterized `DELETE FROM "schema"."table" WHERE …`
/// statement.
///
/// §Fase 37.x.c (D2) — `schema` schema-qualifies the relation (see
/// [`build_select_sql`]). §v1.36.4 — `column_types` drives the
/// `where`-clause value cast.
pub fn build_delete_sql(
    table: &str,
    schema: Option<&str>,
    where_expr: &str,
    bindings: &std::collections::HashMap<String, String>,
    column_types: &std::collections::HashMap<String, String>,
) -> Result<(String, Vec<SqlValue>), StoreError> {
    check_identifier(table, "table")?;
    let (clause, params) = build_pg_where(where_expr, 0, bindings, column_types)?;
    let relation = qualified_relation(schema, table);
    Ok((format!("DELETE FROM {relation} WHERE {clause}"), params))
}

/// §Fase 37.x.b (D1) — a store table resolved against `pg_catalog`,
/// independent of the ambient `search_path`. The product of
/// [`introspect_conn`].
#[derive(Debug, Clone)]
pub(crate) struct ResolvedTable {
    /// The schema the table resolves to (e.g. `public`). §37.x.c (D2)
    /// emits the schema-qualified `"schema"."table"` so an operation
    /// stops depending on the connection's `search_path`.
    pub schema: String,
    /// The `column → udt_name` map driving the `$N::<type>` cast on
    /// both the write side (`build_insert_sql` / `build_update_sql`)
    /// and the read side (`build_pg_where`).
    pub column_types: std::collections::HashMap<String, String>,
}

/// §Fase 37.x.f (D9) — capacity bound on the schema cache. A many-
/// table / many-DSN / multi-tenant adopter cannot grow it unbounded; at
/// the bound the OLDEST entry (smallest insertion sequence) is evicted.
/// 10k matches the idempotency / replay store bound.
const SCHEMA_CACHE_CAPACITY: usize = 10_000;

/// §Fase 37.x.f (D9) — the process-global schema cache:
/// `(dsn, table) → ResolvedTable`, capacity-bounded + self-healing.
///
/// A table's schema + column types are stable for a process lifetime,
/// so one resolution per `(connection, table)` suffices — but the table
/// CAN drift (a live `ALTER TABLE`). D9 makes the cache self-heal: an
/// operation that fails with a schema-drift SQLSTATE evicts the
/// `(dsn, table)` entry ([`PostgresStoreBackend::evict_schema`]) and is
/// retried once against fresh introspection. The cache is also
/// capacity-bounded ([`SCHEMA_CACHE_CAPACITY`]) so it cannot grow
/// without limit. Only a successfully-resolved, non-empty entry is
/// cached (the §v1.36.5 don't-cache-failures rule, preserved).
struct SchemaCache {
    /// `(dsn, table)` → the resolution + its insertion sequence.
    entries: std::collections::HashMap<
        (String, String),
        (std::sync::Arc<ResolvedTable>, u64),
    >,
    /// Monotonic insertion counter — drives oldest-first eviction.
    next_seq: u64,
    /// The capacity bound. A field (not a hard-coded constant) so the
    /// eviction logic is unit-testable with a small bound.
    capacity: usize,
}

impl SchemaCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            next_seq: 0,
            capacity,
        }
    }

    /// The cached resolution for `key`, or `None` on a miss.
    fn get(
        &self,
        key: &(String, String),
    ) -> Option<std::sync::Arc<ResolvedTable>> {
        self.entries.get(key).map(|(arc, _)| std::sync::Arc::clone(arc))
    }

    /// Insert (or refresh) a resolution. §D9 — at capacity the oldest
    /// entry (smallest sequence) is evicted first; a linear scan,
    /// acceptable at the 10k bound (the idempotency store's approach).
    fn insert(
        &mut self,
        key: (String, String),
        resolved: std::sync::Arc<ResolvedTable>,
    ) {
        if self.entries.len() >= self.capacity
            && !self.entries.contains_key(&key)
        {
            // Linear scan for the smallest insertion sequence.
            let oldest = self
                .entries
                .iter()
                .min_by_key(|item| (item.1).1)
                .map(|item| item.0.clone());
            if let Some(oldest) = oldest {
                self.entries.remove(&oldest);
            }
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.entries.insert(key, (resolved, seq));
    }

    /// §D9 — drop `key` so the next operation re-introspects.
    fn evict(&mut self, key: &(String, String)) {
        self.entries.remove(key);
    }
}

static SCHEMA_CACHE: std::sync::LazyLock<std::sync::Mutex<SchemaCache>> =
    std::sync::LazyLock::new(|| {
        std::sync::Mutex::new(SchemaCache::new(SCHEMA_CACHE_CAPACITY))
    });

/// §Fase 37.x.b (D1) — the pure resolution core: group a flat
/// `(schema, column, udt)` introspection result by schema and decide.
///
/// - 0 schemas → [`StoreError::TableNotResolved`].
/// - exactly 1 schema → `Ok((schema, column → udt map))`.
/// - 2+ schemas → [`StoreError::AmbiguousTable`] (the schemas sorted).
///
/// Pure + total — exhaustively unit-tested without a database. Both the
/// search-path-correct primary resolution and the search-path-
/// independent `pg_catalog` fallback feed their rows through this one
/// function, so the resolution verdict is computed identically.
/// `pub` so 37.x.i's property/fuzz pack can drive it across arbitrary
/// schema topologies — same exposure rationale as [`build_pg_where`] /
/// [`build_select_sql`] / [`classify_pg_type`] (pure totals worth
/// exhaustive external test).
pub fn resolve_from_rows(
    table: &str,
    rows: Vec<(String, String, String)>,
) -> Result<(String, std::collections::HashMap<String, String>), StoreError> {
    let mut by_schema: std::collections::BTreeMap<
        String,
        std::collections::HashMap<String, String>,
    > = std::collections::BTreeMap::new();
    for (schema, column, udt) in rows {
        by_schema.entry(schema).or_default().insert(column, udt);
    }
    match by_schema.len() {
        0 => Err(StoreError::TableNotResolved {
            table: table.to_string(),
        }),
        // A `BTreeMap` of length 1 — `into_iter().next()` is total.
        1 => Ok(by_schema.into_iter().next().unwrap()),
        // `BTreeMap` keys iterate sorted — a deterministic schema list.
        _ => Err(StoreError::AmbiguousTable {
            table: table.to_string(),
            schemas: by_schema.into_keys().collect(),
        }),
    }
}

/// §Fase 37.x.b — decode a `pg_catalog` introspection result into the
/// flat `(schema, column, udt)` triples [`resolve_from_rows`] groups. A
/// row missing any field is skipped (defensive — the resolution
/// queries always project all three).
fn collect_triples(rows: &[PgRow]) -> Vec<(String, String, String)> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let schema: String = row.try_get("schema_name").unwrap_or_default();
        let column: String = row.try_get("column_name").unwrap_or_default();
        let udt: String = row.try_get("type_name").unwrap_or_default();
        if !schema.is_empty() && !column.is_empty() && !udt.is_empty() {
            out.push((schema, column, udt));
        }
    }
    out
}

/// §Fase 37.x.f (D9) — `true` iff `code` is a schema-drift SQLSTATE: a
/// store SQL statement that fails with one has hit a STALE cache.
///
///  - `42P01` undefined_table — the table was dropped / renamed / had
///    its schema changed since the resolution was cached.
///  - `42703` undefined_column — a column was dropped / renamed.
///  - `42804` datatype_mismatch — a stale WRITE cast (`$N::<old>` into
///    a column whose type changed).
///  - `42883` undefined_function — a stale READ cast (`"col" = $N::<old>`
///    whose operator no longer exists, e.g. `text = uuid`).
///
/// Every one is a PARSE / PLAN-time rejection — the statement never
/// executed, so the failed operation had ZERO side effects and the D9
/// retry cannot double-write. `pub` so 37.x.i's property/fuzz pack
/// drives it across all ASCII inputs (the closed-set membership test
/// must be total + never panic).
pub fn is_schema_drift_sqlstate(code: &str) -> bool {
    matches!(code, "42P01" | "42703" | "42804" | "42883")
}

/// §Fase 37.x.f (D9) — classify a failed store SQL statement: a
/// schema-drift SQLSTATE ([`is_schema_drift_sqlstate`]) yields
/// [`StoreError::SchemaDrift`] (which triggers the cache self-heal);
/// anything else is a plain [`StoreError::Query`]. `pub(crate)` so the
/// `row_stream` cursor maps its errors through the same classifier.
pub(crate) fn classify_sql_error(
    op: &'static str,
    err: sqlx::Error,
) -> StoreError {
    let sqlstate = err
        .as_database_error()
        .and_then(|db| db.code())
        .map(|c| c.into_owned());
    match sqlstate {
        Some(code) if is_schema_drift_sqlstate(&code) => {
            StoreError::SchemaDrift {
                op,
                sqlstate: code,
                source: err.to_string(),
            }
        }
        _ => StoreError::Query { op, source: err.to_string() },
    }
}

/// §Fase 37.x.b/d (D1/D3) — the two-stage `pg_catalog` table resolution
/// run on a CALLER-PROVIDED connection, so it shares the operation's
/// transaction (D3 — one coherent introspect-and-operate session).
///
///  1. **Primary — search-path-correct.** `to_regclass($1)` (the
///     double-quoted table name) resolves the table exactly as an
///     unqualified `SELECT * FROM "table"` would; the same query
///     introspects its columns.
///  2. **Fallback — search-path-INDEPENDENT.** When `to_regclass`
///     yields NULL (the table is not on this session's `search_path`),
///     a `pg_class` + `pg_namespace` scan keyed on `relname` finds the
///     table in ANY user schema (system schemas excluded; only real
///     relations — `relkind` table / view / matview / partitioned /
///     foreign).
///
/// Exactly one schema resolves the table; zero is
/// [`StoreError::TableNotResolved`], two or more is
/// [`StoreError::AmbiguousTable`]. `pub(crate)` so `row_stream`'s
/// cursor drain runs it inside the cursor's own transaction.
pub(crate) async fn introspect_conn(
    conn: &mut PgConnection,
    table: &str,
) -> Result<std::sync::Arc<ResolvedTable>, StoreError> {
    // — Stage 1: primary, search-path-correct via `to_regclass`. —
    // §Fase 38.x.a (D1) — `.persistent(false)` issues an UNNAMED PARSE
    // (empty name `""`), which Postgres auto-discards/replaces on the
    // next unnamed PARSE — structurally collision-free behind every
    // transaction-mode pooler. Setting `statement_cache_capacity(0)`
    // on the pool's `PgConnectOptions` is necessary but NOT sufficient;
    // sqlx's named PARSE protocol (`sqlx_s_N`) leaks across logical
    // sessions when the physical conn behind the pooler is reused.
    let primary = sqlx::query(
        "SELECT n.nspname AS schema_name, a.attname AS column_name, \
         t.typname AS type_name \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid \
         JOIN pg_catalog.pg_type t ON t.oid = a.atttypid \
         WHERE c.oid = to_regclass($1) \
           AND a.attnum > 0 AND NOT a.attisdropped",
    )
    .persistent(false)
    .bind(format!("\"{table}\""))
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| StoreError::Query {
        op: "resolve",
        source: e.to_string(),
    })?;

    let resolution: Result<(String, std::collections::HashMap<String, String>), StoreError> = {
        let primary_rows = collect_triples(&primary);
        if !primary_rows.is_empty() {
            // `to_regclass` resolved — one relation, one schema.
            resolve_from_rows(table, primary_rows)
        } else {
            // — Stage 2: fallback, search-path-INDEPENDENT scan. —
            let scan = sqlx::query(
                "SELECT n.nspname AS schema_name, \
                 a.attname AS column_name, t.typname AS type_name \
                 FROM pg_catalog.pg_class c \
                 JOIN pg_catalog.pg_namespace n \
                   ON n.oid = c.relnamespace \
                 JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid \
                 JOIN pg_catalog.pg_type t ON t.oid = a.atttypid \
                 WHERE c.relname = $1 \
                   AND c.relkind IN ('r', 'v', 'm', 'p', 'f') \
                   AND left(n.nspname, 3) <> 'pg_' \
                   AND n.nspname <> 'information_schema' \
                   AND a.attnum > 0 AND NOT a.attisdropped",
            )
            .persistent(false)
            .bind(table)
            .fetch_all(&mut *conn)
            .await
            .map_err(|e| StoreError::Query {
                op: "resolve",
                source: e.to_string(),
            })?;
            resolve_from_rows(table, collect_triples(&scan))
        }
    };
    // §Fase 37.x.h (D6) — every resolution failure logs as a structured
    // `tracing::error!` so an adopter's operator can SEE it in production
    // logs / journald, not only in the propagated `StoreError`. The
    // Display hint (the `TableNotResolved` / `AmbiguousTable` arms above)
    // is the actionable line; the structured fields here are the index
    // for log search.
    match resolution {
        Ok((schema, column_types)) => {
            Ok(std::sync::Arc::new(ResolvedTable { schema, column_types }))
        }
        Err(err) => {
            match &err {
                StoreError::TableNotResolved { table } => {
                    tracing::error!(
                        target: "axon::store::resolve",
                        store_table = %table,
                        kind = "table_not_resolved",
                        d_letter = "D6",
                        "axonstore could not resolve `{table}` on any \
                         schema visible to this role — see StoreError \
                         Display for the actionable remedy"
                    );
                }
                StoreError::AmbiguousTable { table, schemas } => {
                    tracing::error!(
                        target: "axon::store::resolve",
                        store_table = %table,
                        kind = "ambiguous_table",
                        schemas = %schemas.join(","),
                        d_letter = "D6",
                        "axonstore `{table}` resolved in {n} schemas — \
                         declare the target schema or narrow \
                         `search_path`",
                        n = schemas.len(),
                    );
                }
                other => {
                    tracing::error!(
                        target: "axon::store::resolve",
                        store_table = %table,
                        kind = "resolve_failed",
                        d_letter = "D6",
                        "axonstore resolution of `{table}` failed: \
                         {other}"
                    );
                }
            }
            Err(err)
        }
    }
}

/// §v1.36.2 — the `::<type>` cast suffix for a `$N` value placeholder.
///
/// axon's runtime carries no column schema (D12), so a `text`-bound
/// value cannot reach a `uuid` / `int` / `timestamptz` column: Postgres
/// has no cross-type operator. The cure is to cast the VALUE to the
/// column's type — `$N::uuid` is a valid explicit cast over the bound
/// parameter (`'83d0…'::uuid` parses the text). v1.36.2 applies it to
/// every WRITE placeholder (`INSERT` values, `UPDATE … SET`); §v1.36.4
/// applies the identical cure to the read side via [`build_pg_where`]
/// (`"col" {op} $N::<type>`). The column's Postgres type name comes
/// from a cached `to_regclass` + `pg_catalog` introspection
/// ([`introspect_conn`]).
///
/// Empty when the column type is unknown (introspection missed the
/// column, or ran against a table outside `current_schema()`) or the
/// type name is not a safe identifier — the builder then emits a bare
/// `$N`: a `text` column still works, a typed column fails LOUDLY (no
/// regression, no silent-wrong write).
fn write_cast(
    column_types: &std::collections::HashMap<String, String>,
    column: &str,
) -> String {
    match column_types.get(column) {
        Some(udt) if filter::is_safe_identifier(udt) => format!("::{udt}"),
        _ => String::new(),
    }
}

/// Build a parameterized `INSERT INTO "table" (…) VALUES (…)`.
///
/// A `NULL` data value renders as the inline `NULL` keyword (a fixed
/// SQL token, injection-safe) and consumes no `$N` placeholder — the
/// same discipline 35.b applies to `NULL` in a `where` clause. Postgres
/// infers the column type for an inline `NULL`.
///
/// §v1.36.2 — each `$N` value placeholder is cast to its column's
/// introspected type (`column_types`) so a `text`-bound value writes
/// into a `uuid` / `int` / `timestamptz` column. An empty
/// `column_types` map emits bare `$N` (the pre-1.36.2 behaviour).
/// §Fase 37.x.c (D2) — `schema` schema-qualifies the relation
/// (`INSERT INTO "schema"."table"`); `None` renders the bare `"table"`.
pub fn build_insert_sql(
    table: &str,
    schema: Option<&str>,
    data: &[(String, SqlValue)],
    column_types: &std::collections::HashMap<String, String>,
) -> Result<(String, Vec<SqlValue>), StoreError> {
    check_identifier(table, "table")?;
    if data.is_empty() {
        return Err(StoreError::EmptyData { op: "insert" });
    }

    let mut columns: Vec<String> = Vec::with_capacity(data.len());
    let mut value_frags: Vec<String> = Vec::with_capacity(data.len());
    let mut params: Vec<SqlValue> = Vec::new();
    let mut idx = 1usize;

    for (col, val) in data {
        check_identifier(col, "column")?;
        columns.push(format!("\"{col}\""));
        match val {
            SqlValue::Null => value_frags.push("NULL".to_string()),
            bound => {
                value_frags.push(format!("${idx}{}", write_cast(column_types, col)));
                params.push(bound.clone());
                idx += 1;
            }
        }
    }

    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        qualified_relation(schema, table),
        columns.join(", "),
        value_frags.join(", "),
    );
    Ok((sql, params))
}

/// Build a parameterized `UPDATE "table" SET … WHERE …`.
///
/// The `WHERE` placeholders continue the numbering after the `SET`
/// placeholders **actually emitted** — not after the column count.
/// Because a `NULL` `SET` value renders inline (no placeholder), the
/// offset is the count of non-`NULL` `SET` values. (The frozen Python
/// reference offsets by column count and so mis-numbers the moment a
/// `SET` value is `NULL`.)
///
/// §v1.36.2 — each `SET` value placeholder is cast to its column's
/// introspected type (`column_types`), the same `$N::<type>` cure
/// `build_insert_sql` applies, so a `text`-bound value writes into a
/// non-`text` column. §v1.36.4 — the same `column_types` map is now
/// threaded into the `WHERE` side too, so a `where`-clause value is
/// cast to its column's type (`"col" {op} $N::<type>`).
/// §Fase 37.x.c (D2) — `schema` schema-qualifies the relation
/// (`UPDATE "schema"."table"`); `None` renders the bare `"table"`.
pub fn build_update_sql(
    table: &str,
    schema: Option<&str>,
    where_expr: &str,
    data: &[(String, SqlValue)],
    bindings: &std::collections::HashMap<String, String>,
    column_types: &std::collections::HashMap<String, String>,
) -> Result<(String, Vec<SqlValue>), StoreError> {
    check_identifier(table, "table")?;
    if data.is_empty() {
        return Err(StoreError::EmptyData { op: "mutate" });
    }

    let mut set_frags: Vec<String> = Vec::with_capacity(data.len());
    let mut params: Vec<SqlValue> = Vec::new();
    let mut idx = 1usize;

    for (col, val) in data {
        check_identifier(col, "column")?;
        match val {
            SqlValue::Null => set_frags.push(format!("\"{col}\" = NULL")),
            bound => {
                set_frags.push(format!(
                    "\"{col}\" = ${idx}{}",
                    write_cast(column_types, col)
                ));
                params.push(bound.clone());
                idx += 1;
            }
        }
    }

    // `idx - 1` SET placeholders were emitted; WHERE continues there.
    let set_param_count = idx - 1;
    let (clause, where_params) =
        build_pg_where(where_expr, set_param_count, bindings, column_types)?;
    params.extend(where_params);

    let sql = format!(
        "UPDATE {} SET {} WHERE {clause}",
        qualified_relation(schema, table),
        set_frags.join(", "),
    );
    Ok((sql, params))
}

/// §Fase 64.C — build the ATOMIC, RELATIVE edge-weight reinforcement `UPDATE`
/// for the memory endofunctor's write-back over a store-sourced MDN corpus:
///
/// ```sql
/// UPDATE "tbl" SET "w" = LEAST(GREATEST("w" + $1::float8, $2::float8), 1.0)
/// WHERE "from" = $3<cast> AND "to" = $4<cast> AND "etype" = $5<cast>
/// ```
///
/// The increment happens INSIDE the database (`"w" + $1`), so concurrent
/// reinforcements of the same edge COMPOSE additively without a lost update —
/// the row write is serialized by the engine. This is not merely a concurrency
/// fix: the endofunctor's semantic reinforcement `ω += Δ` is **commutative**, so
/// the atomic relative update is also semantically faithful (two sessions
/// reinforcing the same edge ⇒ a stronger edge). `LEAST(GREATEST(…, ε), 1.0)`
/// clamps `ω ∈ [ε, 1] ⊆ (0, 1]` (G4) atomically. `$1` = Δ, `$2` = ε; the three
/// `WHERE` keys are cast to their introspected column types (`$N::<type>`), the
/// same cure `build_update_sql` applies, so a text-bound id/etype writes against
/// a `uuid`/enum column.
pub fn build_reinforce_sql(
    table: &str,
    schema: Option<&str>,
    weight_col: &str,
    from_col: &str,
    to_col: &str,
    etype_col: &str,
    column_types: &std::collections::HashMap<String, String>,
) -> Result<String, StoreError> {
    check_identifier(table, "table")?;
    check_identifier(weight_col, "column")?;
    check_identifier(from_col, "column")?;
    check_identifier(to_col, "column")?;
    check_identifier(etype_col, "column")?;
    Ok(format!(
        "UPDATE {rel} SET \"{w}\" = LEAST(GREATEST(\"{w}\" + $1::float8, $2::float8), 1.0) \
         WHERE \"{f}\" = $3{fc} AND \"{t}\" = $4{tc} AND \"{e}\" = $5{ec}",
        rel = qualified_relation(schema, table),
        w = weight_col,
        f = from_col,
        fc = write_cast(column_types, from_col),
        t = to_col,
        tc = write_cast(column_types, to_col),
        e = etype_col,
        ec = write_cast(column_types, etype_col),
    ))
}

// ════════════════════════════════════════════════════════════════════
//  Column-type catalog (D12 honest scope) + row → JSON mapping
// ════════════════════════════════════════════════════════════════════

/// The supported Postgres column-type classes. A column whose type is
/// outside this closed catalog is a [`StoreError::UnsupportedColumnType`]
/// — an honest, documented boundary rather than a silent miss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgTypeClass {
    /// `BOOL`
    Bool,
    /// `INT2` (smallint)
    Int2,
    /// `INT4` (integer)
    Int4,
    /// `INT8` (bigint)
    Int8,
    /// `FLOAT4` (real)
    Float4,
    /// `FLOAT8` (double precision)
    Float8,
    /// `NUMERIC` / `DECIMAL` — JSON-encoded as a string (precision-safe)
    Numeric,
    /// `TEXT` / `VARCHAR` / `BPCHAR` / `NAME`
    Text,
    /// `UUID` — JSON-encoded as a hyphenated string
    Uuid,
    /// `TIMESTAMPTZ` — JSON-encoded as an RFC 3339 string
    TimestampTz,
    /// `TIMESTAMP` — JSON-encoded as an ISO 8601 (no-zone) string
    Timestamp,
    /// `DATE` — JSON-encoded as a `YYYY-MM-DD` string
    Date,
    /// `TIME` — JSON-encoded as a `HH:MM:SS` string
    Time,
    /// `JSON` / `JSONB` — passed through as the JSON value
    Json,
    /// `BYTEA` — JSON-encoded as a base64 string
    Bytea,
}

/// Classify a Postgres type name into a [`PgTypeClass`], or `None` if
/// the type is outside the v1.30.0 supported catalog. Pure + total.
pub fn classify_pg_type(pg_type: &str) -> Option<PgTypeClass> {
    Some(match pg_type.to_ascii_uppercase().as_str() {
        "BOOL" => PgTypeClass::Bool,
        "INT2" => PgTypeClass::Int2,
        "INT4" => PgTypeClass::Int4,
        "INT8" => PgTypeClass::Int8,
        "FLOAT4" => PgTypeClass::Float4,
        "FLOAT8" => PgTypeClass::Float8,
        "NUMERIC" => PgTypeClass::Numeric,
        "TEXT" | "VARCHAR" | "BPCHAR" | "NAME" => PgTypeClass::Text,
        "UUID" => PgTypeClass::Uuid,
        "TIMESTAMPTZ" => PgTypeClass::TimestampTz,
        "TIMESTAMP" => PgTypeClass::Timestamp,
        "DATE" => PgTypeClass::Date,
        "TIME" => PgTypeClass::Time,
        "JSON" | "JSONB" => PgTypeClass::Json,
        "BYTEA" => PgTypeClass::Bytea,
        _ => return None,
    })
}

// §Fase 118.b.3 — `StoreRow` MOVED to `store::error`'s neighbour module; see
// `store/error.rs`. It is JSON-safe column/value pairs — `Vec<(String, JsonValue)>`
// — with zero driver content, and `store::epistemic` (which links no driver) needs
// it. Re-exported so every call site keeps resolving.
pub use super::row::StoreRow;

/// Decode one column of a `PgRow` into a JSON-safe value.
fn pg_value_to_json(
    row: &PgRow,
    idx: usize,
    column: &str,
    pg_type: &str,
) -> Result<JsonValue, StoreError> {
    let class = classify_pg_type(pg_type).ok_or_else(|| {
        StoreError::UnsupportedColumnType {
            column: column.to_string(),
            pg_type: pg_type.to_string(),
        }
    })?;

    // Each branch decodes as `Option<T>` so a SQL `NULL` maps to
    // `JsonValue::Null` rather than failing the decode.
    macro_rules! decode {
        ($t:ty, $conv:expr) => {{
            let opt: Option<$t> = row.try_get(idx).map_err(|e| {
                StoreError::Decode {
                    column: column.to_string(),
                    pg_type: pg_type.to_string(),
                    source: e.to_string(),
                }
            })?;
            match opt {
                None => JsonValue::Null,
                Some(v) => $conv(v),
            }
        }};
    }

    Ok(match class {
        PgTypeClass::Bool => decode!(bool, JsonValue::Bool),
        PgTypeClass::Int2 => decode!(i16, |v| JsonValue::from(v as i64)),
        PgTypeClass::Int4 => decode!(i32, |v| JsonValue::from(v as i64)),
        PgTypeClass::Int8 => decode!(i64, JsonValue::from),
        PgTypeClass::Float4 => decode!(f32, |v| JsonValue::from(v as f64)),
        PgTypeClass::Float8 => decode!(f64, JsonValue::from),
        PgTypeClass::Numeric => {
            decode!(sqlx::types::BigDecimal, |v: sqlx::types::BigDecimal| {
                JsonValue::String(v.to_string())
            })
        }
        PgTypeClass::Text => decode!(String, JsonValue::String),
        PgTypeClass::Uuid => {
            decode!(uuid::Uuid, |v: uuid::Uuid| JsonValue::String(
                v.hyphenated().to_string()
            ))
        }
        PgTypeClass::TimestampTz => {
            decode!(
                chrono::DateTime<chrono::Utc>,
                |v: chrono::DateTime<chrono::Utc>| JsonValue::String(
                    v.to_rfc3339()
                )
            )
        }
        PgTypeClass::Timestamp => {
            decode!(chrono::NaiveDateTime, |v: chrono::NaiveDateTime| {
                JsonValue::String(
                    v.format("%Y-%m-%dT%H:%M:%S%.f").to_string(),
                )
            })
        }
        PgTypeClass::Date => {
            decode!(chrono::NaiveDate, |v: chrono::NaiveDate| {
                JsonValue::String(v.to_string())
            })
        }
        PgTypeClass::Time => {
            decode!(chrono::NaiveTime, |v: chrono::NaiveTime| {
                JsonValue::String(v.to_string())
            })
        }
        PgTypeClass::Json => decode!(JsonValue, |v| v),
        PgTypeClass::Bytea => decode!(Vec<u8>, |v: Vec<u8>| {
            use base64::Engine;
            JsonValue::String(
                base64::engine::general_purpose::STANDARD.encode(v),
            )
        }),
    })
}

/// Map a whole `PgRow` to a [`StoreRow`]. `pub(crate)` so 35.i's
/// `row_stream` cursor drain shares one row-decode path with `query`.
pub(crate) fn map_pg_row(row: &PgRow) -> Result<StoreRow, StoreError> {
    let mut columns = Vec::with_capacity(row.len());
    for (idx, col) in row.columns().iter().enumerate() {
        let name = col.name().to_string();
        let pg_type = col.type_info().name().to_string();
        let value = pg_value_to_json(row, idx, &name, &pg_type)?;
        columns.push((name, value));
    }
    Ok(StoreRow { columns })
}

/// Bind one [`SqlValue`] onto a query. `NULL` is rendered inline by the
/// builders and so never reaches this function in practice; the `Null`
/// arm binds a typed NULL defensively to keep the function total.
/// `pub(crate)` so 35.i's `row_stream` binds cursor-query params
/// through the same path.
pub(crate) fn bind_value<'q>(
    q: Query<'q, Postgres, PgArguments>,
    value: &SqlValue,
) -> Query<'q, Postgres, PgArguments> {
    match value {
        SqlValue::Text(s) => q.bind(s.clone()),
        SqlValue::Integer(n) => q.bind(*n),
        SqlValue::Float(x) => q.bind(*x),
        SqlValue::Boolean(b) => q.bind(*b),
        SqlValue::Null => q.bind(Option::<String>::None),
    }
}

// ════════════════════════════════════════════════════════════════════
//  PostgresStoreBackend
// ════════════════════════════════════════════════════════════════════

/// A Postgres-backed `axonstore`. Holds one lazy, bounded `PgPool`.
/// Cheap to [`Clone`] (the pool is internally reference-counted).
#[derive(Clone)]
pub struct PostgresStoreBackend {
    /// The resolved DSN — masked whenever surfaced (`Debug`, errors).
    dsn: String,
    pool: PgPool,
}

impl fmt::Debug for PostgresStoreBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never expose the DSN password through `Debug`.
        f.debug_struct("PostgresStoreBackend")
            .field("dsn", &mask_dsn(&self.dsn))
            .finish()
    }
}

impl PostgresStoreBackend {
    /// Resolve `connection` and build a lazy, bounded connection pool.
    ///
    /// Equivalent to [`connect_named`](Self::connect_named) with no
    /// store name — the connection's `application_name` is the bare
    /// `axon-store`. Prefer `connect_named` so each session is
    /// attributable to its declaring `axonstore`.
    pub fn connect(connection: &str) -> Result<Self, StoreError> {
        Self::connect_named(connection, "")
    }

    /// Resolve `connection` and build a lazy, bounded connection pool,
    /// stamping each connection's `application_name` with `store_name`.
    ///
    /// Synchronous and cheap: the DSN is parsed into a
    /// [`PgConnectOptions`] (a malformed DSN is a typed
    /// [`StoreError::PoolInit`]) but `connect_lazy_with` opens **no**
    /// connection — the first real connection is made on the first
    /// operation (D7 — lazy).
    ///
    /// Two production-grade properties are set on every connection:
    ///
    /// - **`statement_cache_capacity(0)`** (Gap 3) — disables sqlx's
    ///   named server-side prepared-statement cache so the backend is
    ///   safe behind a transaction-mode pooler (PgBouncer
    ///   `pool_mode=transaction`, Supabase Supavisor `:6543`, Neon, RDS
    ///   Proxy), where a cached name minted on one physical session
    ///   collides on the next (`prepared statement "sqlx_s_1" already
    ///   exists`). Applied unconditionally — harmless on a direct
    ///   connection, and there is no knob to misconfigure.
    /// - **`application_name`** — `axon-store/<store_name>` (bare
    ///   `axon-store` when `store_name` is empty), capped at the
    ///   Postgres 63-byte `NAMEDATALEN-1` limit on a char boundary, so
    ///   every axon-owned session is identifiable in `pg_stat_activity`,
    ///   pooler logs and DBA dashboards.
    ///
    /// Must be called within a Tokio runtime context: a well-formed DSN
    /// registers a background connection reaper. In production this is
    /// always satisfied — the registry (35.d) is built while the axum
    /// server's runtime is live.
    pub fn connect_named(
        connection: &str,
        store_name: &str,
    ) -> Result<Self, StoreError> {
        Self::connect_named_with_namespace(connection, store_name, None)
    }

    /// §Fase 38.f (D3) — same as [`Self::connect_named`] but stamps an
    /// OPTIONAL per-tenant schema namespace into `application_name`.
    ///
    /// `connect_named_with_namespace("env:DB", "claims", Some("tenant_42"))`
    /// produces a pool whose every session's `application_name` reads
    /// `axon-store/claims/tenant_42` — so a DBA reading
    /// `pg_stat_activity`, pooler logs, or RDS Performance Insights
    /// sees both the `axonstore` declaration AND the resolved tenant.
    ///
    /// `None` for `namespace` is the pre-38 shape (`axon-store/<store>`,
    /// byte-identical to `connect_named`).
    pub fn connect_named_with_namespace(
        connection: &str,
        store_name: &str,
        namespace: Option<&str>,
    ) -> Result<Self, StoreError> {
        Self::connect_named_sized(connection, store_name, namespace, MAX_POOL_CONNECTIONS)
    }

    /// §Fase 113 — **the pool size finally comes from somewhere you can say.**
    ///
    /// Until §113 every `postgresql` axonstore in existence got exactly
    /// [`MAX_POOL_CONNECTIONS`] connections: hardcoded, no environment variable,
    /// no config, no source-level knob. The pool an adopter's flow depends on was
    /// the *least* configurable of the three pools in the product.
    ///
    /// `resource.capacity` is that knob. It was declared, lowered into the IR,
    /// and — as §113's census proved by exhaustive grep — **read by zero lines of
    /// code in either repository**, while the README sold it as a pool cap.
    ///
    /// Threading it to `max_connections` here is what makes `resource` a WIRE and
    /// not a LABEL. If this argument were ignored, §113 would be the nominal link
    /// its own plan forbids.
    pub fn connect_named_sized(
        connection: &str,
        store_name: &str,
        namespace: Option<&str>,
        max_connections: u32,
    ) -> Result<Self, StoreError> {
        let dsn = resolve_dsn(connection)?;
        let opts = PgConnectOptions::from_str(&dsn)
            .map_err(|e| StoreError::PoolInit {
                dsn_masked: mask_dsn(&dsn),
                source: e.to_string(),
            })?
            .statement_cache_capacity(0)
            .application_name(&application_name_for_with_namespace(
                store_name, namespace,
            ));
        // §Fase 38.x.a (D2) — `DEALLOCATE ALL` on every released conn.
        //
        // This is the SECOND layer of pooler-coherent transaction safety,
        // composing with the per-query `.persistent(false)` (D1). If a
        // future code path accidentally omits `.persistent(false)`, the
        // named prepared statement it allocated would otherwise survive
        // on the physical Postgres conn across logical sessions through a
        // transaction-mode pooler (Supabase Supavisor `:6543`, PgBouncer
        // `pool_mode=transaction`, Neon, RDS Proxy). The next logical
        // session that lands on the same physical conn would collide on
        // `PARSE sqlx_s_N` → Postgres `42710` `duplicate_prepared_statement`.
        //
        // Running `DEALLOCATE ALL` on `after_release` wipes every prepared
        // statement (named + unnamed) from the physical conn BEFORE the
        // pooler returns it to its pool. Belt-and-suspenders: D1 prevents
        // the bug at the source; D2 catches anything that slips past.
        //
        // The cleanup query itself uses `.persistent(false)` — the
        // meta-invariant: even the cleanup is unnamed, so no prepared
        // statement can ever survive a connection release.
        let pool = PgPoolOptions::new()
            // §Fase 113 — `resource.capacity`, or the legacy default when the
            // store is not on a resource.
            .max_connections(max_connections.max(1))
            .min_connections(0)
            .acquire_timeout(Duration::from_secs(ACQUIRE_TIMEOUT_SECS))
            .idle_timeout(Duration::from_secs(IDLE_TIMEOUT_SECS))
            .after_release(|conn, _meta| Box::pin(async move {
                // `DEALLOCATE ALL` clears every prepared statement —
                // named (`sqlx_s_N`) AND unnamed (`""`) — from the
                // physical Postgres connection. Cheap (<1ms typically).
                sqlx::query("DEALLOCATE ALL")
                    .persistent(false)
                    .execute(&mut *conn)
                    .await?;
                // `Ok(true)` keeps the conn alive in the pool for reuse.
                // We never drop a conn on `DEALLOCATE` failure because a
                // failure here means something more fundamental is wrong
                // (lost socket, server-side crash); the next acquire will
                // surface the real error.
                Ok(true)
            }))
            .connect_lazy_with(opts);
        Ok(Self { dsn, pool })
    }

    /// The resolved DSN with its password masked — safe to log.
    pub fn masked_dsn(&self) -> String {
        mask_dsn(&self.dsn)
    }

    /// The underlying pool — 35.i's `Stream<Row>` borrows it.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// §Fase 37.x.j (D1) — Acquire ONE physical Postgres connection
    /// from the pool to be held for the duration of a flow execution
    /// ([`crate::runner::ExecContext`] for the sync path,
    /// [`crate::flow_dispatcher::DispatchCtx`] for the async streaming
    /// path).
    ///
    /// The returned [`sqlx::pool::PoolConnection`] is wrapped by the
    /// caller in [`crate::store::store_conn::StoreConn::Pinned`] and
    /// passed to every operation (`query` / `insert` / `mutate` /
    /// `purge` / `ping`) against this axonstore for the flow lifetime.
    /// Because every op runs against the same physical Postgres backend
    /// connection, a transaction-mode pooler (Supabase Supavisor,
    /// PgBouncer, Neon, RDS Proxy) cannot swap the backend between
    /// queries — the D3 "unnamed prepared statement does not exist"
    /// race that Fase 37.x.j closes.
    ///
    /// The connection is released back to the pool on `Drop` of the
    /// returned `PoolConnection`. The existing
    /// `after_release(DEALLOCATE ALL)` hook (Fase 38.x.a D2) wipes any
    /// prepared statements before the conn is reused — composing
    /// cleanly with the per-flow pinning of 37.x.j.
    ///
    /// Failure modes:
    ///   - `StoreError::Connect` if the pool's `acquire_timeout`
    ///     elapses (no conn becomes available — pool exhausted or
    ///     Postgres unreachable).
    ///   - `StoreError::Connect` if the pool is in a permanently-bad
    ///     state (TLS handshake failure, DNS resolution failure, etc.).
    /// §Fase 118.a / D118.2 — returns the PORT
    /// ([`crate::pinned_conn::PinnedConn`]), not a raw
    /// `PoolConnection<Postgres>`. This module is where the sqlx types
    /// legitimately live; the executor that holds the result for the rest of the
    /// flow is not, and used to name the driver type in its own signatures.
    pub async fn acquire_pin(
        &self,
    ) -> Result<crate::pinned_conn::PinnedConn, StoreError> {
        // §Fase 96.a — under a SESSION pooler or a DIRECT connection, every pool
        // connection is already a coherent, stable session, so the §37.x.j pin
        // (one connection held for the WHOLE flow so ops route through the same
        // physical backend) is REDUNDANT — and harmful: it keeps a scarce
        // connection checked out across the flow's cognition (LLM) steps,
        // starving the pool under load. Refuse the pin so every
        // LAZY caller (`if let Ok(p) = acquire_pin()`) silently falls to its
        // per-op `StoreConn::Pool` path (acquire → op → release), releasing the
        // connection across cognition. The EAGER loops skip acquisition entirely
        // (they'd otherwise log a misleading warn). Only a TRANSACTION-mode
        // pooler (the default) needs the pin. Doctrine
        // `connections_release_across_cognition`.
        if !connection_pinning_enabled() {
            return Err(StoreError::Connect {
                source: "connection pinning disabled (AXON_DB_POOLER_MODE=session|direct) \
                         — store ops acquire per-op so the connection releases across \
                         cognition steps"
                    .to_string(),
            });
        }
        self.pool
            .acquire()
            .await
            .map(crate::pinned_conn::PinnedConn::new)
            .map_err(|e| StoreError::Connect { source: e.to_string() })
    }

    /// `retrieve` — run `SELECT * FROM "schema"."table" WHERE …` and map
    /// every row to a JSON-safe [`StoreRow`].
    ///
    /// §Fase 37.x.d (D3) — on a cache MISS the schema introspection and
    /// the `SELECT` execute inside ONE transaction, so a
    /// transaction-mode pooler pins one physical backend for both —
    /// they cannot split across sessions. A cache HIT needs no
    /// transaction: the cached resolution is already correct and the
    /// `SELECT` is schema-qualified, so it resolves on any session.
    ///
    /// v1.30.0 materializes the full result (`fetch_all`); 35.i adds the
    /// backpressured `Stream<Row>` variant (Pillar III).
    pub async fn query(
        &self,
        // §Fase 37.x.j (D1) — the connection source for this op.
        // `StoreConn::Pool(&self.pool)` for legacy callers (the
        // v1.38.5 and earlier behavior); `StoreConn::Pinned(conn)` for
        // 37.x.j flow-pinned execution where the caller acquired a
        // `PoolConnection` at flow start via `acquire_pin()`. Both
        // variants run the cache-HIT SELECT + cache-MISS introspect+
        // SELECT-in-tx paths identically; the pinned variant
        // additionally guarantees the same physical Postgres backend
        // services every op against this store for the flow lifetime.
        conn: &mut crate::store::store_conn::StoreConn<'_>,
        table: &str,
        where_expr: &str,
        bindings: &std::collections::HashMap<String, String>,
    ) -> Result<Vec<StoreRow>, StoreError> {
        // — cache HIT: operate with the cached resolution; no
        //   transaction. §37.x.f (D9) self-heals a stale cache. —
        if let Some(resolved) = self.cached_schema(table) {
            let (sql, params) = build_select_sql(
                table,
                Some(resolved.schema.as_str()),
                where_expr,
                bindings,
                &resolved.column_types,
            )?;
            // §Fase 38.x.a (D1) — see `introspect_conn` for the full rationale.
            let mut q = sqlx::query(&sql).persistent(false);
            for value in &params {
                q = bind_value(q, value);
            }
            // §Fase 37.x.j (D1) — dispatch through the StoreConn so
            // a pinned variant routes through the same physical conn
            // as every other op against this store for the flow.
            match conn.fetch_all(q).await {
                Ok(rows) => return rows.iter().map(map_pg_row).collect(),
                Err(e) => {
                    let err = classify_sql_error("retrieve", e);
                    if !err.is_schema_drift() {
                        return Err(err);
                    }
                    // §37.x.f (D9) — the cached schema is STALE; evict
                    // and fall through to the miss path: the single
                    // retry, with fresh introspection.
                    self.evict_schema(table);
                }
            }
        }
        // — cache MISS, or the §37.x.f (D9) self-heal retry: resolve +
        //   operate in ONE transaction (D3). —
        // §Fase 37.x.j (D1) — `conn.begin()` borrows the `StoreConn`
        // mutably for the transaction's lifetime; on the Pinned variant
        // the transaction runs on the same physical backend as the
        // cache-HIT attempt above (D3 invariant preserved).
        let mut tx = conn.begin().await.map_err(|e| {
            StoreError::Connect { source: e.to_string() }
        })?;
        // §Fase 37.x.j.11 (POST-CLOSE HOTFIX 2026-05-21) — ROLLBACK +
        // propagate the introspect error directly. Pre-hotfix the
        // code fell through to bare-table SQL with `(None, &no_types)`
        // inside the SAME (now poisoned) transaction. The cascade
        // error (`25P02 in_failed_sql_transaction` / `42703 column
        // does not exist`) was returned to the application layer,
        // masking the actual root cause from any caller that didn't
        // filter the `axon::store` tracing target.
        //
        // Honest scope cut: adopters whose introspect privileges
        // differ from query privileges (rare in practice — same DB
        // user) no longer get the fall-through. If real adopter
        // demand surfaces, a future fase can add an opt-in
        // `unsafe_skip_introspect` flag.
        let resolved = match introspect_conn(&mut tx, table).await {
            Ok(r) => r,
            Err(introspect_err) => {
                tracing::warn!(
                    target: "axon::store",
                    table = %table,
                    op = "introspect_in_tx",
                    error = %introspect_err,
                    d_letter = "37.x.j.11",
                    "store introspection failed; rolling back the \
                     transaction and returning the primary error \
                     directly. Pre-37.x.j.11 the runtime fell through \
                     to bare-table SQL inside the poisoned tx → \
                     cascade error masked the root cause."
                );
                let _ = tx.rollback().await;
                return Err(introspect_err);
            }
        };
        let (sql, params) = build_select_sql(
            table,
            Some(resolved.schema.as_str()),
            where_expr,
            bindings,
            &resolved.column_types,
        )?;
        // §Fase 38.x.a (D1) — `.persistent(false)` is mandatory inside the
        // `pool.begin()` transaction: the named PARSE protocol leaks across
        // logical sessions when the physical conn behind the pooler is
        // reused. See `introspect_conn` for the full rationale.
        let mut q = sqlx::query(&sql).persistent(false);
        for value in &params {
            q = bind_value(q, value);
        }
        let rows = q
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| classify_sql_error("retrieve", e))?;
        tx.commit().await.map_err(|e| StoreError::Connect {
            source: e.to_string(),
        })?;
        self.cache_schema(table, resolved);
        rows.iter().map(map_pg_row).collect()
    }

    /// §Fase 37.x.d (D3) — the cached `(schema, column_types)`
    /// resolution for `table`, or `None` on a cache miss. Pure — no
    /// I/O. A HIT lets an operation skip the transaction; a MISS makes
    /// the caller introspect ([`introspect_conn`]) inside the
    /// operation's own transaction, so a transaction-mode pooler pins
    /// one backend for resolution + operation.
    pub(crate) fn cached_schema(
        &self,
        table: &str,
    ) -> Option<std::sync::Arc<ResolvedTable>> {
        let key = (self.dsn.clone(), table.to_string());
        SCHEMA_CACHE.lock().unwrap().get(&key)
    }

    /// §Fase 37.x.d (D3) — store a successful resolution in the
    /// process-global `(dsn, table)` cache.
    ///
    /// §v1.36.5 rule preserved — an EMPTY resolution is NEVER cached: a
    /// real relation always has at least one column, so an empty map is
    /// a transient failure that must be retried, never a poisoned
    /// entry. §Fase 37.x.f (D9) adds the bounded-LRU + drift eviction.
    pub(crate) fn cache_schema(
        &self,
        table: &str,
        resolved: std::sync::Arc<ResolvedTable>,
    ) {
        if !resolved.column_types.is_empty() {
            let key = (self.dsn.clone(), table.to_string());
            SCHEMA_CACHE.lock().unwrap().insert(key, resolved);
        }
    }

    /// §Fase 37.x.f (D9) — evict `table`'s cached resolution so the
    /// next operation re-introspects. Called by the self-heal path when
    /// a store SQL statement fails with a schema-drift SQLSTATE — the
    /// live table has drifted from the cached schema. `pub(crate)` so
    /// the `row_stream` cursor drain shares the self-heal.
    pub(crate) fn evict_schema(&self, table: &str) {
        let key = (self.dsn.clone(), table.to_string());
        SCHEMA_CACHE.lock().unwrap().evict(&key);
        // §Fase 37.x.h (D6) — observability of the D9 self-heal. A live
        // `ALTER TABLE` is the expected trigger; a flood of these from
        // one `(masked_dsn, table)` means a misconfiguration (a migration
        // never finished, two services racing against the same table) and
        // an operator needs to SEE the eviction. The masked DSN gives the
        // physical-store context without ever leaking a credential.
        tracing::warn!(
            target: "axon::store::cache",
            store_table = %table,
            masked_dsn = %mask_dsn(&self.dsn),
            kind = "schema_drift_evict",
            d_letter = "D9",
            "axonstore evicted cached schema for `{table}` after a \
             schema-drift SQLSTATE — the next operation will \
             re-introspect against the live table"
        );
    }

    /// §Fase 37.x.g (D8) — EAGERLY resolve + introspect `table` against
    /// the live database, populating the process-global schema cache.
    /// The deploy-time verification entry point: a resolution failure
    /// surfaces at DEPLOY, not at the first production request.
    ///
    /// A cache hit is a no-op `Ok`. Otherwise one connection is
    /// acquired and the two-stage [`introspect_conn`] resolution runs;
    /// the result is cached so the first runtime operation hits a warm
    /// cache. The caller distinguishes the `Err`: a `TableNotResolved`
    /// / `AmbiguousTable` means the table genuinely does not resolve on
    /// a reachable store (a fatal deploy error); a `Connect` means the
    /// store is unreachable (a non-fatal deploy warning — the D9
    /// runtime resolution still applies).
    pub(crate) async fn warm_schema(&self, table: &str) -> Result<(), StoreError> {
        if self.cached_schema(table).is_some() {
            return Ok(());
        }
        let mut conn = self.pool.acquire().await.map_err(|e| {
            StoreError::Connect { source: e.to_string() }
        })?;
        let resolved = introspect_conn(&mut conn, table).await?;
        self.cache_schema(table, resolved);
        Ok(())
    }

    /// `persist` — run `INSERT INTO "schema"."table" (…) VALUES (…)`.
    /// Returns the number of rows inserted. §Fase 37.x.d (D3) — on a
    /// cache MISS the resolution + the `INSERT` execute in ONE
    /// transaction; a cache HIT needs no transaction.
    pub async fn insert(
        &self,
        // §Fase 37.x.j (D1) — see `query()` for the rationale on the
        // `StoreConn` connection-source parameter.
        conn: &mut crate::store::store_conn::StoreConn<'_>,
        table: &str,
        data: &[(String, SqlValue)],
    ) -> Result<u64, StoreError> {
        // — cache HIT: operate with the cached resolution; no
        //   transaction. §37.x.f (D9) self-heals a stale cache. —
        if let Some(resolved) = self.cached_schema(table) {
            let (sql, params) = build_insert_sql(
                table,
                Some(resolved.schema.as_str()),
                data,
                &resolved.column_types,
            )?;
            // §Fase 38.x.a (D1) — see `introspect_conn` for the full rationale.
            let mut q = sqlx::query(&sql).persistent(false);
            for value in &params {
                q = bind_value(q, value);
            }
            // §Fase 37.x.j (D1) — dispatch through StoreConn.
            match conn.execute(q).await {
                Ok(result) => return Ok(result.rows_affected()),
                Err(e) => {
                    let err = classify_sql_error("persist", e);
                    if !err.is_schema_drift() {
                        return Err(err);
                    }
                    // §37.x.f (D9) — stale cache: evict + fall through
                    // (the single retry). Safe — a drift SQLSTATE is a
                    // parse/plan-time rejection, so this `INSERT` wrote
                    // zero rows; the retry cannot double-write.
                    self.evict_schema(table);
                }
            }
        }
        // — cache MISS, or the §37.x.f (D9) self-heal retry: resolve +
        //   operate in ONE transaction (D3). —
        // §Fase 37.x.j (D1) — see `query()` for the begin() rationale.
        let mut tx = conn.begin().await.map_err(|e| {
            StoreError::Connect { source: e.to_string() }
        })?;
        // §Fase 37.x.j.11 — ROLLBACK + propagate introspect error
        // directly. See `query()` above for the full rationale.
        let resolved = match introspect_conn(&mut tx, table).await {
            Ok(r) => r,
            Err(introspect_err) => {
                tracing::warn!(
                    target: "axon::store",
                    table = %table,
                    op = "introspect_in_tx_persist",
                    error = %introspect_err,
                    d_letter = "37.x.j.11",
                    "persist introspection failed; rolling back the \
                     transaction and returning the primary error \
                     directly."
                );
                let _ = tx.rollback().await;
                return Err(introspect_err);
            }
        };
        let (sql, params) = build_insert_sql(
            table,
            Some(resolved.schema.as_str()),
            data,
            &resolved.column_types,
        )?;
        // §Fase 38.x.a (D1) — mandatory inside the `pool.begin()` tx.
        let mut q = sqlx::query(&sql).persistent(false);
        for value in &params {
            q = bind_value(q, value);
        }
        let result = q
            .execute(&mut *tx)
            .await
            .map_err(|e| classify_sql_error("persist", e))?;
        tx.commit().await.map_err(|e| StoreError::Connect {
            source: e.to_string(),
        })?;
        self.cache_schema(table, resolved);
        Ok(result.rows_affected())
    }

    /// `mutate` — run `UPDATE "schema"."table" SET … WHERE …`. Returns
    /// the number of rows affected. §Fase 37.x.d (D3) — on a cache MISS
    /// the resolution + the `UPDATE` execute in ONE transaction; a
    /// cache HIT needs no transaction.
    pub async fn mutate(
        &self,
        // §Fase 37.x.j (D1) — see `query()` for the rationale.
        conn: &mut crate::store::store_conn::StoreConn<'_>,
        table: &str,
        where_expr: &str,
        data: &[(String, SqlValue)],
        bindings: &std::collections::HashMap<String, String>,
    ) -> Result<u64, StoreError> {
        // — cache HIT: operate with the cached resolution; no
        //   transaction. §37.x.f (D9) self-heals a stale cache. —
        if let Some(resolved) = self.cached_schema(table) {
            let (sql, params) = build_update_sql(
                table,
                Some(resolved.schema.as_str()),
                where_expr,
                data,
                bindings,
                &resolved.column_types,
            )?;
            // §Fase 38.x.a (D1) — see `introspect_conn` for the full rationale.
            let mut q = sqlx::query(&sql).persistent(false);
            for value in &params {
                q = bind_value(q, value);
            }
            // §Fase 37.x.j (D1) — dispatch through StoreConn.
            match conn.execute(q).await {
                Ok(result) => return Ok(result.rows_affected()),
                Err(e) => {
                    let err = classify_sql_error("mutate", e);
                    if !err.is_schema_drift() {
                        return Err(err);
                    }
                    // §37.x.f (D9) — stale cache: evict + fall through
                    // (the single retry). Safe — a drift SQLSTATE is a
                    // parse/plan-time rejection, so this `UPDATE`
                    // modified zero rows; the retry cannot double-write.
                    self.evict_schema(table);
                }
            }
        }
        // — cache MISS, or the §37.x.f (D9) self-heal retry: resolve +
        //   operate in ONE transaction (D3). —
        // §Fase 37.x.j (D1) — see `query()` for the begin() rationale.
        let mut tx = conn.begin().await.map_err(|e| {
            StoreError::Connect { source: e.to_string() }
        })?;
        // §Fase 37.x.j.11 — ROLLBACK + propagate introspect error
        // directly. See `query()` above for the full rationale.
        let resolved = match introspect_conn(&mut tx, table).await {
            Ok(r) => r,
            Err(introspect_err) => {
                tracing::warn!(
                    target: "axon::store",
                    table = %table,
                    op = "introspect_in_tx_mutate",
                    error = %introspect_err,
                    d_letter = "37.x.j.11",
                    "mutate introspection failed; rolling back the \
                     transaction and returning the primary error \
                     directly."
                );
                let _ = tx.rollback().await;
                return Err(introspect_err);
            }
        };
        let (sql, params) = build_update_sql(
            table,
            Some(resolved.schema.as_str()),
            where_expr,
            data,
            bindings,
            &resolved.column_types,
        )?;
        // §Fase 38.x.a (D1) — mandatory inside the `pool.begin()` tx.
        let mut q = sqlx::query(&sql).persistent(false);
        for value in &params {
            q = bind_value(q, value);
        }
        let result = q
            .execute(&mut *tx)
            .await
            .map_err(|e| classify_sql_error("mutate", e))?;
        tx.commit().await.map_err(|e| StoreError::Connect {
            source: e.to_string(),
        })?;
        self.cache_schema(table, resolved);
        Ok(result.rows_affected())
    }

    /// §Fase 64.C — execute ONE atomic, relative edge-weight reinforcement
    /// ([`build_reinforce_sql`]) on the given **tenant-scoped** connection: the
    /// memory endofunctor's `ω += Δ` write-back over a store-sourced MDN corpus.
    /// Returns the rows affected (0 when the edge row no longer exists — a
    /// since-deleted edge must not error a navigation). Uses the cached schema
    /// for qualification + the `WHERE`-key type casts; if the schema is not yet
    /// warmed (the navigate-time READ normally warms it for this same store) the
    /// reinforcement is SKIPPED — learning is best-effort and never blocks or
    /// fails the navigation that produced it.
    #[allow(clippy::too_many_arguments)]
    pub async fn reinforce(
        &self,
        conn: &mut crate::store::store_conn::StoreConn<'_>,
        table: &str,
        weight_col: &str,
        from_col: &str,
        to_col: &str,
        etype_col: &str,
        from_val: &SqlValue,
        to_val: &SqlValue,
        etype_val: &SqlValue,
        delta: f64,
        epsilon: f64,
    ) -> Result<u64, StoreError> {
        let Some(resolved) = self.cached_schema(table) else {
            return Ok(0); // schema not warmed — best-effort skip
        };
        let sql = build_reinforce_sql(
            table,
            Some(resolved.schema.as_str()),
            weight_col,
            from_col,
            to_col,
            etype_col,
            &resolved.column_types,
        )?;
        let mut q = sqlx::query(&sql).persistent(false);
        q = bind_value(q, &SqlValue::Float(delta));
        q = bind_value(q, &SqlValue::Float(epsilon));
        q = bind_value(q, from_val);
        q = bind_value(q, to_val);
        q = bind_value(q, etype_val);
        match conn.execute(q).await {
            Ok(r) => Ok(r.rows_affected()),
            Err(e) => Err(classify_sql_error("reinforce", e)),
        }
    }

    /// `purge` — run `DELETE FROM "schema"."table" WHERE …`. Returns the
    /// number of rows deleted. §Fase 37.x.d (D3) — on a cache MISS the
    /// resolution + the `DELETE` execute in ONE transaction; a cache
    /// HIT needs no transaction.
    pub async fn purge(
        &self,
        // §Fase 37.x.j (D1) — see `query()` for the rationale.
        conn: &mut crate::store::store_conn::StoreConn<'_>,
        table: &str,
        where_expr: &str,
        bindings: &std::collections::HashMap<String, String>,
    ) -> Result<u64, StoreError> {
        // — cache HIT: operate with the cached resolution; no
        //   transaction. §37.x.f (D9) self-heals a stale cache. —
        if let Some(resolved) = self.cached_schema(table) {
            let (sql, params) = build_delete_sql(
                table,
                Some(resolved.schema.as_str()),
                where_expr,
                bindings,
                &resolved.column_types,
            )?;
            // §Fase 38.x.a (D1) — see `introspect_conn` for the full rationale.
            let mut q = sqlx::query(&sql).persistent(false);
            for value in &params {
                q = bind_value(q, value);
            }
            // §Fase 37.x.j (D1) — dispatch through StoreConn.
            match conn.execute(q).await {
                Ok(result) => return Ok(result.rows_affected()),
                Err(e) => {
                    let err = classify_sql_error("purge", e);
                    if !err.is_schema_drift() {
                        return Err(err);
                    }
                    // §37.x.f (D9) — stale cache: evict + fall through
                    // (the single retry). Safe — a drift SQLSTATE is a
                    // parse/plan-time rejection, so this `DELETE`
                    // removed zero rows; the retry cannot double-delete.
                    self.evict_schema(table);
                }
            }
        }
        // — cache MISS, or the §37.x.f (D9) self-heal retry: resolve +
        //   operate in ONE transaction (D3). —
        // §Fase 37.x.j (D1) — see `query()` for the begin() rationale.
        let mut tx = conn.begin().await.map_err(|e| {
            StoreError::Connect { source: e.to_string() }
        })?;
        // §Fase 37.x.j.11 — ROLLBACK + propagate introspect error
        // directly. See `query()` above for the full rationale.
        let resolved = match introspect_conn(&mut tx, table).await {
            Ok(r) => r,
            Err(introspect_err) => {
                tracing::warn!(
                    target: "axon::store",
                    table = %table,
                    op = "introspect_in_tx_purge",
                    error = %introspect_err,
                    d_letter = "37.x.j.11",
                    "purge introspection failed; rolling back the \
                     transaction and returning the primary error \
                     directly."
                );
                let _ = tx.rollback().await;
                return Err(introspect_err);
            }
        };
        let (sql, params) = build_delete_sql(
            table,
            Some(resolved.schema.as_str()),
            where_expr,
            bindings,
            &resolved.column_types,
        )?;
        // §Fase 38.x.a (D1) — mandatory inside the `pool.begin()` tx.
        let mut q = sqlx::query(&sql).persistent(false);
        for value in &params {
            q = bind_value(q, value);
        }
        let result = q
            .execute(&mut *tx)
            .await
            .map_err(|e| classify_sql_error("purge", e))?;
        tx.commit().await.map_err(|e| StoreError::Connect {
            source: e.to_string(),
        })?;
        self.cache_schema(table, resolved);
        Ok(result.rows_affected())
    }

    /// Verify database reachability with `SELECT 1`.
    pub async fn ping(&self) -> Result<(), StoreError> {
        // §Fase 38.x.a (D1) — even the trivial reachability probe carries
        // `.persistent(false)`: a `SELECT 1` PARSE collision is rare but
        // possible behind an aggressive transaction-mode pooler, and the
        // grep §-assertion in `fase38x_a_pooler_prepared_statement_regression.rs`
        // enforces the invariant uniformly.
        sqlx::query("SELECT 1")
            .persistent(false)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|e| StoreError::Connect { source: e.to_string() })
    }
}

// ════════════════════════════════════════════════════════════════════
//  Unit tests — pure surface (no database)
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

        // §Fase 118.b.3 — `pinning_mode_gate` MOVED to `store::pooler_mode` with its
        // subject. A test that stays behind when the thing it tests moves is how a
        // suite starts asserting about a file instead of about a behaviour.

    fn txt(s: &str) -> SqlValue {
        SqlValue::Text(s.to_string())
    }

    /// Empty bindings — these `build_*_sql` tests pin the pre-37.d
    /// behaviour (no `${name}` resolution). The §Fase 37.d resolution
    /// is exercised by `tests/fase37_d_*` and `store::filter`.
    fn nb() -> std::collections::HashMap<String, String> {
        std::collections::HashMap::new()
    }

    // ── resolve_dsn ──────────────────────────────────────────────────

    #[test]
    fn resolve_empty_connection_errors() {
        assert_eq!(resolve_dsn(""), Err(StoreError::EmptyConnection));
        assert_eq!(resolve_dsn("    "), Err(StoreError::EmptyConnection));
    }

    #[test]
    fn resolve_literal_dsn_is_returned_verbatim() {
        let dsn = "postgresql://u:p@localhost:5432/axon";
        assert_eq!(resolve_dsn(dsn), Ok(dsn.to_string()));
    }

    #[test]
    fn resolve_literal_dsn_is_trimmed() {
        assert_eq!(
            resolve_dsn("  postgresql://h/db  "),
            Ok("postgresql://h/db".to_string())
        );
    }

    #[test]
    fn resolve_bare_env_prefix_errors() {
        assert_eq!(resolve_dsn("env:"), Err(StoreError::EmptyEnvVarName));
        assert_eq!(resolve_dsn("env:   "), Err(StoreError::EmptyEnvVarName));
    }

    #[test]
    fn resolve_missing_env_var_errors() {
        match resolve_dsn("env:AXON_NONEXISTENT_VAR_FASE35C") {
            Err(StoreError::MissingEnvVar { var }) => {
                assert_eq!(var, "AXON_NONEXISTENT_VAR_FASE35C");
            }
            other => panic!("expected MissingEnvVar, got {other:?}"),
        }
    }

    #[test]
    fn resolve_env_var_reads_the_environment() {
        // `PATH` is set on every supported OS — exercise the success
        // path without mutating the process environment.
        let resolved = resolve_dsn("env:PATH").expect("PATH resolves");
        assert_eq!(resolved, std::env::var("PATH").unwrap());
        assert!(!resolved.is_empty());
    }

    // ── connect / masking ────────────────────────────────────────────

    #[tokio::test]
    async fn connect_with_valid_dsn_is_lazy_and_succeeds() {
        // `connect_lazy` opens no connection — a well-formed DSN to a
        // host that may not exist still yields Ok.
        let backend =
            PostgresStoreBackend::connect("postgresql://u:p@localhost:5432/db")
                .expect("a well-formed DSN builds a lazy pool");
        let _ = format!("{backend:?}");
    }

    #[tokio::test]
    async fn connect_masks_the_password_in_dsn_and_debug() {
        // A deliberately fake credential — this test asserts the
        // backend never surfaces a DSN password.
        let fake_secret = "fakecred0";
        let backend = PostgresStoreBackend::connect(&format!(
            "postgresql://user:{fake_secret}@localhost:5432/axon"
        ))
        .unwrap();
        let masked = backend.masked_dsn();
        assert!(!masked.contains(fake_secret), "password must be masked");
        assert!(masked.contains("***"));
        assert!(!format!("{backend:?}").contains(fake_secret));
    }

    #[test]
    fn connect_empty_connection_errors() {
        assert!(matches!(
            PostgresStoreBackend::connect(""),
            Err(StoreError::EmptyConnection)
        ));
    }

    #[test]
    fn connect_missing_env_var_errors() {
        assert!(matches!(
            PostgresStoreBackend::connect("env:AXON_NONEXISTENT_VAR_FASE35C"),
            Err(StoreError::MissingEnvVar { .. })
        ));
    }

    #[test]
    fn connect_malformed_dsn_errors() {
        assert!(matches!(
            PostgresStoreBackend::connect("not a valid dsn at all"),
            Err(StoreError::PoolInit { .. })
        ));
    }

    // ── Gap 3 (v1.36.3) — pooler safety + application_name ───────────

    #[tokio::test]
    async fn connect_named_with_valid_dsn_is_lazy_and_succeeds() {
        // `connect_named` builds the same lazy pool — Gap 3 only adds
        // `statement_cache_capacity(0)` + `application_name`, neither of
        // which opens a connection.
        let backend = PostgresStoreBackend::connect_named(
            "postgresql://u:p@localhost:5432/db",
            "claims",
        )
        .expect("a well-formed DSN builds a lazy pool");
        let _ = format!("{backend:?}");
    }

    #[test]
    fn connect_named_malformed_dsn_errors() {
        assert!(matches!(
            PostgresStoreBackend::connect_named("not a dsn", "claims"),
            Err(StoreError::PoolInit { .. })
        ));
    }

    #[test]
    fn application_name_carries_the_store_name() {
        assert_eq!(application_name_for("claims"), "axon-store/claims");
        assert_eq!(
            application_name_for("tenant_audit_log"),
            "axon-store/tenant_audit_log"
        );
    }

    #[test]
    fn application_name_empty_store_is_bare() {
        // `connect` delegates with no store name — the bare label, with
        // no dangling slash.
        assert_eq!(application_name_for(""), "axon-store");
    }

    #[test]
    fn application_name_capped_at_postgres_namedatalen() {
        // Postgres silently truncates `application_name` past 63 bytes;
        // we cap it ourselves so the stamped name is exactly observed.
        let long = "s".repeat(200);
        let name = application_name_for(&long);
        assert!(name.len() <= 63, "must fit NAMEDATALEN-1, got {}", name.len());
        assert!(name.starts_with("axon-store/s"));
    }

    #[test]
    fn application_name_truncation_respects_char_boundaries() {
        // A multi-byte tail must never be cut mid-codepoint — the result
        // is always valid UTF-8 (`String` guarantees it, but the cut
        // must land on a boundary or the slice panics).
        let name = application_name_for(&"é".repeat(100));
        assert!(name.len() <= 63);
        assert!(name.is_char_boundary(name.len()));
    }

    // ── build_select_sql ─────────────────────────────────────────────

    #[test]
    fn select_with_filter() {
        let (sql, params) =
            build_select_sql("users", None, "id = 1", &nb(), &nb()).unwrap();
        // §37.x.e (D4) — unknown column type + equality → `::text`.
        assert_eq!(sql, "SELECT * FROM \"users\" WHERE \"id\"::text = $1");
        assert_eq!(params, vec![SqlValue::Integer(1)]);
    }

    #[test]
    fn select_casts_the_filter_value_to_its_introspected_column_type() {
        // §v1.36.4 — a known column type casts the WHERE value, so the
        // comparison uses the native operator (`int4 = int4`).
        let types = std::collections::HashMap::from([(
            "id".to_string(),
            "int4".to_string(),
        )]);
        let (sql, _) =
            build_select_sql("users", None, "id = 1", &nb(), &types).unwrap();
        assert_eq!(sql, "SELECT * FROM \"users\" WHERE \"id\" = $1::int4");
    }

    #[test]
    fn select_with_empty_filter_renders_where_true() {
        let (sql, params) =
            build_select_sql("users", None, "", &nb(), &nb()).unwrap();
        assert_eq!(sql, "SELECT * FROM \"users\" WHERE TRUE");
        assert!(params.is_empty());
    }

    #[test]
    fn select_rejects_unsafe_table_name() {
        assert!(matches!(
            build_select_sql("users; DROP TABLE x", None, "", &nb(), &nb()),
            Err(StoreError::InvalidIdentifier { kind: "table", .. })
        ));
    }

    #[test]
    fn select_propagates_filter_errors() {
        assert!(matches!(
            build_select_sql("users", None, "id = 1 AND", &nb(), &nb()),
            Err(StoreError::Filter(_))
        ));
    }

    // ── build_delete_sql ─────────────────────────────────────────────

    #[test]
    fn delete_with_filter() {
        let (sql, params) =
            build_delete_sql("sessions", None, "expired = true", &nb(), &nb())
                .unwrap();
        assert_eq!(sql, "DELETE FROM \"sessions\" WHERE \"expired\"::text = $1");
        assert_eq!(params, vec![SqlValue::Boolean(true)]);
    }

    #[test]
    fn delete_rejects_unsafe_table() {
        assert!(matches!(
            build_delete_sql("evil\"table", None, "a = 1", &nb(), &nb()),
            Err(StoreError::InvalidIdentifier { .. })
        ));
    }

    // ── build_insert_sql ─────────────────────────────────────────────

    #[test]
    fn insert_basic() {
        let (sql, params) = build_insert_sql(
            "users",
            None,
            &[("name".into(), txt("Alice")), ("age".into(), SqlValue::Integer(30))],
            &nb(),
        )
        .unwrap();
        assert_eq!(
            sql,
            "INSERT INTO \"users\" (\"name\", \"age\") VALUES ($1, $2)"
        );
        assert_eq!(params, vec![txt("Alice"), SqlValue::Integer(30)]);
    }

    #[test]
    fn insert_casts_a_document_into_a_jsonb_column() {
        // §Fase 73.d — a `Json` document is bound as text and cast to the
        // introspected `jsonb` column type (`$N::jsonb`), so it lands as a
        // native jsonb value (Postgres parses + stores the binary form).
        // This is the write half of the jsonb round-trip; the read half is
        // `pg_value_to_json`'s `PgTypeClass::Json` decode → a live nested
        // `JsonValue`.
        let cols = std::collections::HashMap::from([
            ("id".to_string(), "uuid".to_string()),
            ("payload".to_string(), "jsonb".to_string()),
        ]);
        let (sql, params) = build_insert_sql(
            "events",
            None,
            &[
                ("id".into(), txt("11111111-1111-1111-1111-111111111111")),
                ("payload".into(), txt(r#"{"city":"Bogotá"}"#)),
            ],
            &cols,
        )
        .unwrap();
        assert_eq!(
            sql,
            "INSERT INTO \"events\" (\"id\", \"payload\") VALUES ($1::uuid, $2::jsonb)"
        );
        assert_eq!(params.len(), 2);
        // The read half: a jsonb column classifies as the Json type class.
        assert_eq!(classify_pg_type("JSONB"), Some(PgTypeClass::Json));
        assert_eq!(classify_pg_type("JSON"), Some(PgTypeClass::Json));
    }

    #[test]
    fn insert_renders_null_inline_consuming_no_placeholder() {
        let (sql, params) = build_insert_sql(
            "t",
            None,
            &[
                ("a".into(), SqlValue::Integer(1)),
                ("b".into(), SqlValue::Null),
                ("c".into(), txt("x")),
            ],
            &nb(),
        )
        .unwrap();
        assert_eq!(
            sql,
            "INSERT INTO \"t\" (\"a\", \"b\", \"c\") VALUES ($1, NULL, $2)"
        );
        assert_eq!(params, vec![SqlValue::Integer(1), txt("x")]);
    }

    #[test]
    fn insert_empty_data_errors() {
        assert_eq!(
            build_insert_sql("t", None, &[], &nb()),
            Err(StoreError::EmptyData { op: "insert" })
        );
    }

    #[test]
    fn insert_rejects_unsafe_column_name() {
        assert!(matches!(
            build_insert_sql("t", None, &[("a\"; DROP".into(), SqlValue::Integer(1))], &nb()),
            Err(StoreError::InvalidIdentifier { kind: "column", .. })
        ));
    }

    #[test]
    fn insert_rejects_unsafe_table_name() {
        assert!(matches!(
            build_insert_sql("t t", None, &[("a".into(), SqlValue::Integer(1))], &nb()),
            Err(StoreError::InvalidIdentifier { kind: "table", .. })
        ));
    }

    // ── build_update_sql ─────────────────────────────────────────────

    #[test]
    fn update_basic_where_offset_continues_after_set() {
        let (sql, params) = build_update_sql(
            "users",
            None,
            "id = 5",
            &[("name".into(), txt("Bob")), ("age".into(), SqlValue::Integer(40))],
            &nb(),
            &nb(),
        )
        .unwrap();
        assert_eq!(
            sql,
            "UPDATE \"users\" SET \"name\" = $1, \"age\" = $2 \
             WHERE \"id\"::text = $3"
        );
        assert_eq!(
            params,
            vec![txt("Bob"), SqlValue::Integer(40), SqlValue::Integer(5)]
        );
    }

    #[test]
    fn update_null_set_value_shifts_where_offset_by_non_null_count() {
        // The defect the Python reference has: a NULL SET value renders
        // inline, so the WHERE offset is the NON-NULL set count (1),
        // not the column count (2). `id` must be $2, not $3.
        let (sql, params) = build_update_sql(
            "users",
            None,
            "id = 5",
            &[("name".into(), SqlValue::Null), ("age".into(), SqlValue::Integer(40))],
            &nb(),
            &nb(),
        )
        .unwrap();
        assert_eq!(
            sql,
            "UPDATE \"users\" SET \"name\" = NULL, \"age\" = $1 \
             WHERE \"id\"::text = $2"
        );
        assert_eq!(params, vec![SqlValue::Integer(40), SqlValue::Integer(5)]);
    }

    #[test]
    fn update_with_empty_where_targets_all_rows() {
        let (sql, _) = build_update_sql(
            "t",
            None,
            "",
            &[("a".into(), SqlValue::Integer(1))],
            &nb(),
            &nb(),
        )
        .unwrap();
        assert_eq!(sql, "UPDATE \"t\" SET \"a\" = $1 WHERE TRUE");
    }

    #[test]
    fn reinforce_sql_is_atomic_relative_and_clamped() {
        // §Fase 64.C — the write-back UPDATE: the weight increment is computed
        // INSIDE the database (`"weight" + $1`), so it's race-free; the clamp
        // keeps ω ∈ [ε, 1]; the WHERE keys carry their introspected casts.
        let mut types = std::collections::HashMap::new();
        types.insert("from_id".to_string(), "uuid".to_string());
        types.insert("to_id".to_string(), "uuid".to_string());
        let sql = build_reinforce_sql(
            "ltm_edges",
            Some("public"),
            "weight",
            "from_id",
            "to_id",
            "etype",
            &types,
        )
        .unwrap();
        assert_eq!(
            sql,
            "UPDATE \"public\".\"ltm_edges\" SET \"weight\" = \
             LEAST(GREATEST(\"weight\" + $1::float8, $2::float8), 1.0) \
             WHERE \"from_id\" = $3::uuid AND \"to_id\" = $4::uuid AND \"etype\" = $5"
        );
    }

    #[test]
    fn update_empty_data_errors() {
        assert_eq!(
            build_update_sql("t", None, "id = 1", &[], &nb(), &nb()),
            Err(StoreError::EmptyData { op: "mutate" })
        );
    }

    #[test]
    fn update_rejects_unsafe_column() {
        assert!(matches!(
            build_update_sql(
                "t",
                None,
                "id = 1",
                &[("a-b".into(), SqlValue::Integer(1))],
                &nb(),
                &nb(),
            ),
            Err(StoreError::InvalidIdentifier { kind: "column", .. })
        ));
    }

    #[test]
    fn update_propagates_filter_errors() {
        assert!(matches!(
            build_update_sql(
                "t",
                None,
                "bad ;",
                &[("a".into(), SqlValue::Integer(1))],
                &nb(),
                &nb(),
            ),
            Err(StoreError::Filter(_))
        ));
    }

    // ── §v1.36.2 — typed-column write cast ───────────────────────────

    #[test]
    fn insert_casts_each_value_to_its_introspected_column_type() {
        let types = std::collections::HashMap::from([
            ("tenant_id".to_string(), "uuid".to_string()),
            ("note".to_string(), "text".to_string()),
            ("n".to_string(), "int4".to_string()),
        ]);
        let (sql, _) = build_insert_sql(
            "chat_history",
            None,
            &[
                ("tenant_id".into(), txt("83d078e1-b372-42ba-9572-ff8dc521386e")),
                ("note".into(), txt("hi")),
                ("n".into(), SqlValue::Integer(3)),
            ],
            &types,
        )
        .unwrap();
        assert_eq!(
            sql,
            "INSERT INTO \"chat_history\" (\"tenant_id\", \"note\", \"n\") \
             VALUES ($1::uuid, $2::text, $3::int4)",
            "each value placeholder is cast to its column's \
             introspected type so a text-bound value writes into a \
             uuid / int column"
        );
    }

    #[test]
    fn update_set_casts_each_value_to_its_introspected_column_type() {
        let types = std::collections::HashMap::from([(
            "status".to_string(),
            "uuid".to_string(),
        )]);
        let (sql, _) = build_update_sql(
            "t",
            None,
            "id = 1",
            &[("status".into(), txt("83d078e1-b372-42ba-9572-ff8dc521386e"))],
            &nb(),
            &types,
        )
        .unwrap();
        assert_eq!(
            sql,
            "UPDATE \"t\" SET \"status\" = $1::uuid WHERE \"id\"::text = $2",
            "the SET value is cast to the column type; `id` \
             is absent from the type map so the driver casts the \
             WHERE column to `text` for the equality"
        );
    }

    #[test]
    fn update_where_value_is_cast_to_its_column_type() {
        // §v1.36.4 — when the WHERE column's type IS known, its value
        // placeholder is cast too (the SET-side cure applied to WHERE).
        let types = std::collections::HashMap::from([
            ("status".to_string(), "text".to_string()),
            ("id".to_string(), "int8".to_string()),
        ]);
        let (sql, _) = build_update_sql(
            "t",
            None,
            "id = 1",
            &[("status".into(), txt("done"))],
            &nb(),
            &types,
        )
        .unwrap();
        assert_eq!(
            sql,
            "UPDATE \"t\" SET \"status\" = $1::text WHERE \"id\" = $2::int8"
        );
    }

    #[test]
    fn unknown_column_type_falls_back_to_a_bare_placeholder() {
        // An empty type map (introspection missed the table / column)
        // → bare `$N`, the pre-1.36.2 behaviour: a `text` column still
        // works, a typed column fails LOUDLY — no regression, no
        // silent-wrong write.
        let (sql, _) =
            build_insert_sql("t", None, &[("x".into(), txt("v"))], &nb()).unwrap();
        assert_eq!(sql, "INSERT INTO \"t\" (\"x\") VALUES ($1)");
    }

    #[test]
    fn an_unsafe_column_type_name_is_not_spliced_into_sql() {
        // Defense in depth: `udt_name` comes from Postgres, but a type
        // name that is not a safe identifier is never spliced — the
        // builder falls back to a bare `$N`.
        let types = std::collections::HashMap::from([(
            "x".to_string(),
            "uuid; DROP TABLE t".to_string(),
        )]);
        let (sql, _) =
            build_insert_sql("t", None, &[("x".into(), txt("v"))], &types).unwrap();
        assert_eq!(
            sql, "INSERT INTO \"t\" (\"x\") VALUES ($1)",
            "an unsafe type name yields no cast — never a splice"
        );
    }

    // ── D4 — injection resistance, end to end ────────────────────────

    #[test]
    fn injection_in_value_position_is_a_bound_parameter() {
        let (sql, params) = build_select_sql(
            "users",
            None,
            "name = '; DROP TABLE users; --'",
            &nb(),
            &nb(),
        )
        .unwrap();
        assert_eq!(sql, "SELECT * FROM \"users\" WHERE \"name\"::text = $1");
        assert_eq!(
            params,
            vec![txt("; DROP TABLE users; --")]
        );
    }

    #[test]
    fn injection_in_table_identifier_is_rejected_not_quoted() {
        assert!(matches!(
            build_select_sql("users\" WHERE 1=1; --", None, "", &nb(), &nb()),
            Err(StoreError::InvalidIdentifier { .. })
        ));
    }

    // ── §Fase 37.x.c — schema-anchored relation (D2) ─────────────────

    #[test]
    fn select_with_a_resolved_schema_is_qualified() {
        // §37.x.c (D2) — a resolved schema renders `"schema"."table"`,
        // so the SELECT resolves on any session regardless of the
        // ambient `search_path`.
        let (sql, _) =
            build_select_sql("tenants", Some("public"), "id = 1", &nb(), &nb())
                .unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM \"public\".\"tenants\" WHERE \"id\"::text = $1"
        );
    }

    #[test]
    fn every_builder_qualifies_with_a_resolved_schema() {
        // D2 must flip ALL FOUR builders, not three.
        let data = [("v".to_string(), SqlValue::Integer(1))];
        let (sel, _) =
            build_select_sql("t", Some("app"), "", &nb(), &nb()).unwrap();
        let (del, _) =
            build_delete_sql("t", Some("app"), "", &nb(), &nb()).unwrap();
        let (ins, _) = build_insert_sql("t", Some("app"), &data, &nb()).unwrap();
        let (upd, _) =
            build_update_sql("t", Some("app"), "", &data, &nb(), &nb()).unwrap();
        assert!(sel.contains("FROM \"app\".\"t\""), "SELECT: {sel}");
        assert!(del.contains("FROM \"app\".\"t\""), "DELETE: {del}");
        assert!(ins.contains("INTO \"app\".\"t\""), "INSERT: {ins}");
        assert!(upd.starts_with("UPDATE \"app\".\"t\""), "UPDATE: {upd}");
    }

    #[test]
    fn no_resolved_schema_renders_the_bare_table() {
        // D5 backwards-compat — `schema = None` (resolution failed or
        // not attempted) renders the pre-37.x un-qualified `"table"`.
        let (sql, _) = build_select_sql("t", None, "", &nb(), &nb()).unwrap();
        assert_eq!(sql, "SELECT * FROM \"t\" WHERE TRUE");
    }

    #[test]
    fn an_unsafe_schema_name_is_not_spliced_and_falls_back_to_bare_table() {
        // Defense in depth (D4) — a schema name from `pg_catalog` that
        // is not a safe identifier is NEVER spliced; the builder falls
        // back to the bare `"table"` (search_path resolves it), exactly
        // as an unsafe `udt_name` yields no cast.
        for unsafe_schema in ["a\"; DROP TABLE x", "my schema", "1schema"] {
            let (sql, _) =
                build_select_sql("t", Some(unsafe_schema), "", &nb(), &nb())
                    .unwrap();
            assert_eq!(
                sql, "SELECT * FROM \"t\" WHERE TRUE",
                "unsafe schema `{unsafe_schema}` must not be spliced"
            );
        }
    }

    #[test]
    fn a_qualified_table_still_casts_and_offsets_correctly() {
        // §37.x.c composes with §v1.36.2/§v1.36.4 — schema-qualification
        // is orthogonal to the value cast + the WHERE param offset.
        let types = std::collections::HashMap::from([
            ("status".to_string(), "uuid".to_string()),
            ("id".to_string(), "int8".to_string()),
        ]);
        let (sql, _) = build_update_sql(
            "t",
            Some("public"),
            "id = 1",
            &[("status".into(), txt("done"))],
            &nb(),
            &types,
        )
        .unwrap();
        assert_eq!(
            sql,
            "UPDATE \"public\".\"t\" SET \"status\" = $1::uuid \
             WHERE \"id\" = $2::int8"
        );
    }

    // ── classify_pg_type ─────────────────────────────────────────────

    #[test]
    fn classify_every_supported_type() {
        let cases = [
            ("BOOL", PgTypeClass::Bool),
            ("INT2", PgTypeClass::Int2),
            ("INT4", PgTypeClass::Int4),
            ("INT8", PgTypeClass::Int8),
            ("FLOAT4", PgTypeClass::Float4),
            ("FLOAT8", PgTypeClass::Float8),
            ("NUMERIC", PgTypeClass::Numeric),
            ("TEXT", PgTypeClass::Text),
            ("VARCHAR", PgTypeClass::Text),
            ("BPCHAR", PgTypeClass::Text),
            ("NAME", PgTypeClass::Text),
            ("UUID", PgTypeClass::Uuid),
            ("TIMESTAMPTZ", PgTypeClass::TimestampTz),
            ("TIMESTAMP", PgTypeClass::Timestamp),
            ("DATE", PgTypeClass::Date),
            ("TIME", PgTypeClass::Time),
            ("JSON", PgTypeClass::Json),
            ("JSONB", PgTypeClass::Json),
            ("BYTEA", PgTypeClass::Bytea),
        ];
        for (name, expected) in cases {
            assert_eq!(classify_pg_type(name), Some(expected), "type {name}");
        }
    }

    #[test]
    fn classify_is_case_insensitive() {
        assert_eq!(classify_pg_type("int4"), Some(PgTypeClass::Int4));
        assert_eq!(classify_pg_type("TimestampTz"), Some(PgTypeClass::TimestampTz));
    }

    #[test]
    fn classify_unsupported_types_return_none() {
        for name in ["INT4[]", "INET", "POINT", "HSTORE", "CIDR", "MONEY", ""] {
            assert_eq!(classify_pg_type(name), None, "type {name} unsupported");
        }
    }

    // ── StoreRow ─────────────────────────────────────────────────────

    #[test]
    fn store_row_get_and_to_json() {
        let row = StoreRow {
            columns: vec![
                ("id".into(), JsonValue::from(7)),
                ("name".into(), JsonValue::String("Eve".into())),
            ],
        };
        assert_eq!(row.get("id"), Some(&JsonValue::from(7)));
        assert_eq!(row.get("missing"), None);
        assert_eq!(
            row.to_json(),
            serde_json::json!({ "id": 7, "name": "Eve" })
        );
    }

    // ── §Fase 37.x.b — resolve_from_rows (D1 pure resolution core) ───

    fn triple(s: &str, c: &str, t: &str) -> (String, String, String) {
        (s.to_string(), c.to_string(), t.to_string())
    }

    #[test]
    fn resolve_from_rows_no_rows_is_table_not_resolved() {
        match resolve_from_rows("widgets", vec![]) {
            Err(StoreError::TableNotResolved { table }) => {
                assert_eq!(table, "widgets");
            }
            other => panic!("expected TableNotResolved, got {other:?}"),
        }
    }

    #[test]
    fn resolve_from_rows_one_schema_resolves_with_its_column_map() {
        let (schema, types) = resolve_from_rows(
            "tenants",
            vec![triple("public", "id", "uuid"), triple("public", "n", "int4")],
        )
        .expect("a single-schema result resolves");
        assert_eq!(schema, "public");
        assert_eq!(types.get("id"), Some(&"uuid".to_string()));
        assert_eq!(types.get("n"), Some(&"int4".to_string()));
        assert_eq!(types.len(), 2);
    }

    #[test]
    fn resolve_from_rows_two_schemas_is_ambiguous_with_sorted_schemas() {
        match resolve_from_rows(
            "widgets",
            vec![
                triple("tenant_b", "id", "uuid"),
                triple("tenant_a", "id", "int4"),
            ],
        ) {
            Err(StoreError::AmbiguousTable { table, schemas }) => {
                assert_eq!(table, "widgets");
                // `BTreeMap` keys iterate sorted — a deterministic list.
                assert_eq!(
                    schemas,
                    vec!["tenant_a".to_string(), "tenant_b".to_string()]
                );
            }
            other => panic!("expected AmbiguousTable, got {other:?}"),
        }
    }

    #[test]
    fn resolve_from_rows_three_schemas_is_still_one_ambiguous_error() {
        assert!(matches!(
            resolve_from_rows(
                "t",
                vec![
                    triple("s1", "a", "text"),
                    triple("s2", "a", "text"),
                    triple("s3", "a", "text"),
                ],
            ),
            Err(StoreError::AmbiguousTable { .. })
        ));
    }

    // ── §Fase 37.x.d — schema cache (D3) ─────────────────────────────

    #[tokio::test]
    async fn schema_cache_round_trips_a_resolution() {
        // The cache surface that lets a coherent-session operation skip
        // the transaction on a hit. `connect` is lazy — no database.
        let backend = PostgresStoreBackend::connect(
            "postgresql://u:p@localhost:5432/schema_cache_rt",
        )
        .unwrap();
        let table = "schema_cache_probe";
        assert!(
            backend.cached_schema(table).is_none(),
            "a cold cache is a miss"
        );
        let resolved = std::sync::Arc::new(ResolvedTable {
            schema: "public".to_string(),
            column_types: std::collections::HashMap::from([(
                "id".to_string(),
                "uuid".to_string(),
            )]),
        });
        backend.cache_schema(table, std::sync::Arc::clone(&resolved));
        let hit = backend
            .cached_schema(table)
            .expect("a warm cache is a hit");
        assert_eq!(hit.schema, "public");
        assert_eq!(hit.column_types.get("id"), Some(&"uuid".to_string()));
    }

    #[tokio::test]
    async fn schema_cache_never_stores_an_empty_resolution() {
        // §v1.36.5 rule preserved — a real relation always has ≥ 1
        // column, so an empty map is a transient failure to retry,
        // never a poisoned cache entry.
        let backend = PostgresStoreBackend::connect(
            "postgresql://u:p@localhost:5432/schema_cache_empty",
        )
        .unwrap();
        let table = "schema_empty_probe";
        backend.cache_schema(
            table,
            std::sync::Arc::new(ResolvedTable {
                schema: "public".to_string(),
                column_types: std::collections::HashMap::new(),
            }),
        );
        assert!(
            backend.cached_schema(table).is_none(),
            "an empty resolution must never be cached"
        );
    }

    // ── §Fase 37.x.f — D9 self-healing bounded cache ─────────────────

    #[test]
    fn is_schema_drift_sqlstate_recognises_exactly_the_drift_codes() {
        // The four parse/plan-time rejections that signal a stale cache.
        for code in ["42P01", "42703", "42804", "42883"] {
            assert!(
                is_schema_drift_sqlstate(code),
                "`{code}` must be a schema-drift SQLSTATE"
            );
        }
        // Non-drift samples — unique-violation, syntax error, connection
        // failure, check-violation, serialization failure, empty.
        for code in ["23505", "42601", "08006", "23514", "40001", ""] {
            assert!(
                !is_schema_drift_sqlstate(code),
                "`{code}` is NOT schema drift — must not trigger the \
                 self-heal retry"
            );
        }
    }

    #[test]
    fn store_error_is_schema_drift_predicate() {
        assert!(StoreError::SchemaDrift {
            op: "retrieve",
            sqlstate: "42883".to_string(),
            source: "operator does not exist: text = uuid".to_string(),
        }
        .is_schema_drift());
        assert!(!StoreError::Query {
            op: "retrieve",
            source: "syntax error".to_string(),
        }
        .is_schema_drift());
        assert!(!StoreError::TableNotResolved { table: "t".into() }
            .is_schema_drift());
    }

    /// A small `ResolvedTable` for the cache tests.
    fn rt(schema: &str) -> std::sync::Arc<ResolvedTable> {
        std::sync::Arc::new(ResolvedTable {
            schema: schema.to_string(),
            column_types: std::collections::HashMap::from([(
                "id".to_string(),
                "uuid".to_string(),
            )]),
        })
    }

    #[test]
    fn schema_cache_evicts_the_oldest_entry_at_capacity() {
        // §D9 — the bound: a many-table adopter cannot grow the cache
        // without limit; at capacity the OLDEST insertion is evicted.
        let mut cache = SchemaCache::new(2);
        let key = |t: &str| ("dsn".to_string(), t.to_string());
        cache.insert(key("a"), rt("s_a"));
        cache.insert(key("b"), rt("s_b"));
        cache.insert(key("c"), rt("s_c")); // over capacity → evict `a`.
        assert_eq!(cache.entries.len(), 2, "the cache is bounded at 2");
        assert!(
            cache.get(&key("a")).is_none(),
            "the oldest entry was evicted"
        );
        assert_eq!(
            cache.get(&key("b")).map(|r| r.schema.clone()),
            Some("s_b".to_string())
        );
        assert_eq!(
            cache.get(&key("c")).map(|r| r.schema.clone()),
            Some("s_c".to_string())
        );
    }

    #[test]
    fn schema_cache_evict_drops_a_named_entry() {
        // §D9 — the self-heal eviction primitive.
        let mut cache = SchemaCache::new(10);
        let key = ("dsn".to_string(), "t".to_string());
        cache.insert(key.clone(), rt("public"));
        assert!(cache.get(&key).is_some());
        cache.evict(&key);
        assert!(cache.get(&key).is_none(), "evict drops the entry");
    }

    #[test]
    fn schema_cache_reinsert_of_a_key_does_not_evict_another() {
        // Re-inserting an EXISTING key (a self-heal re-introspection)
        // refreshes it in place — it must not evict another entry.
        let mut cache = SchemaCache::new(2);
        let ka = ("dsn".to_string(), "a".to_string());
        let kb = ("dsn".to_string(), "b".to_string());
        cache.insert(ka.clone(), rt("public"));
        cache.insert(kb.clone(), rt("public"));
        cache.insert(ka.clone(), rt("public")); // re-insert — no eviction.
        assert_eq!(cache.entries.len(), 2);
        assert!(cache.get(&ka).is_some());
        assert!(cache.get(&kb).is_some(), "the re-insert evicted nothing");
    }

    // ── StoreError display ───────────────────────────────────────────

    #[test]
    fn every_store_error_has_a_non_empty_display() {
        let errors = [
            StoreError::EmptyConnection,
            StoreError::EmptyEnvVarName,
            StoreError::MissingEnvVar { var: "X".into() },
            StoreError::PoolInit {
                dsn_masked: "postgresql://u:***@h/db".into(),
                source: "bad".into(),
            },
            StoreError::InvalidIdentifier { kind: "table", name: "x;".into() },
            StoreError::EmptyData { op: "insert" },
            StoreError::Filter(FilterError::TooManyConditions { limit: 256 }),
            StoreError::Connect { source: "refused".into() },
            StoreError::Query { op: "retrieve", source: "syntax".into() },
            StoreError::UnsupportedColumnType {
                column: "geom".into(),
                pg_type: "POINT".into(),
            },
            StoreError::Decode {
                column: "ts".into(),
                pg_type: "TIMESTAMPTZ".into(),
                source: "overflow".into(),
            },
            StoreError::TableNotResolved { table: "ghost".into() },
            StoreError::AmbiguousTable {
                table: "dup".into(),
                schemas: vec!["a".into(), "b".into()],
            },
            StoreError::SchemaDrift {
                op: "retrieve",
                sqlstate: "42883".into(),
                source: "operator does not exist: text = uuid".into(),
            },
        ];
        for e in errors {
            assert!(!e.to_string().is_empty());
        }
    }

    #[test]
    fn filter_error_is_a_store_error_source() {
        use std::error::Error;
        let e = StoreError::Filter(FilterError::TooManyConditions { limit: 256 });
        assert!(e.source().is_some());
    }

    #[test]
    fn filter_error_converts_into_store_error() {
        let e: StoreError = FilterError::TooManyConditions { limit: 256 }.into();
        assert!(matches!(e, StoreError::Filter(_)));
    }

    // ── §Fase 37.x.h — D6 honest, actionable failure ─────────────────

    #[test]
    fn d6_table_not_resolved_display_carries_an_actionable_hint() {
        // The Display of `TableNotResolved` is the user-facing surface of
        // the D1 resolution failure — it MUST tell an adopter (a) what
        // happened, (b) the table involved, and (c) at least one concrete
        // remedy. A bare "could not resolve" is the *un-actionable* form
        // 37.x.h replaces.
        let err = StoreError::TableNotResolved {
            table: "claims".into(),
        };
        let text = err.to_string();
        assert!(
            text.contains("`claims`"),
            "the table name must appear verbatim, got: {text}"
        );
        assert!(
            text.contains("pg_catalog"),
            "the message must disclose pg_catalog (so an adopter knows \
             `search_path` is not the culprit), got: {text}"
        );
        assert!(
            text.contains("migration") || text.contains("SELECT"),
            "the message must name at least one concrete remedy \
             (migration / SELECT permission), got: {text}"
        );
    }

    #[test]
    fn d6_ambiguous_table_display_points_at_fase_38_schema_declaration() {
        // The Display of `AmbiguousTable` MUST tell an adopter both the
        // schemas the table resolved into AND the two real remedies —
        // narrow `search_path` OR declare the target schema explicitly
        // (the Fase 38 `schema:` declaration the gap report names).
        let err = StoreError::AmbiguousTable {
            table: "rates".into(),
            schemas: vec!["finance".into(), "legacy".into()],
        };
        let text = err.to_string();
        assert!(text.contains("`rates`"), "table name must appear");
        assert!(
            text.contains("finance") && text.contains("legacy"),
            "every resolving schema must appear, got: {text}"
        );
        assert!(
            text.contains("search_path"),
            "the search_path remedy must appear, got: {text}"
        );
        assert!(
            text.contains("schema:"),
            "the `schema:` declaration must be named (the \
             genuinely-superior remedy), got: {text}"
        );
        assert!(
            text.contains("`schema:` declaration"),
            "the message must anchor the remedy to the `schema:` declaration, got: {text}"
        );
    }

    #[test]
    fn d6_display_does_not_leak_internal_sqlstates_or_internal_paths() {
        // The Display is operator-facing prose, not a stack trace —
        // SQLSTATE codes belong on `SchemaDrift` (where they ARE the
        // diagnostic), not on a resolution failure. A regression would
        // be code spilling into the friendly arms.
        let nr = StoreError::TableNotResolved { table: "t".into() }.to_string();
        let amb = StoreError::AmbiguousTable {
            table: "t".into(),
            schemas: vec!["a".into()],
        }
        .to_string();
        for code in ["42P01", "42703", "42804", "42883"] {
            assert!(
                !nr.contains(code),
                "TableNotResolved must not leak SQLSTATE {code}"
            );
            assert!(
                !amb.contains(code),
                "AmbiguousTable must not leak SQLSTATE {code}"
            );
        }
    }
}
