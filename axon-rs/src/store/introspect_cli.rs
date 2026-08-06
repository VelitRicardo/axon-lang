//! §Fase 38.h (D10) — `axon store introspect <store>` CLI orchestration.
//!
//! The IMPURE half of the introspection pipeline. The pure half
//! lives in `axon_frontend::store_introspect` (manifest building,
//! UDT mapping, omission tracking, diff). This module:
//!
//!  1. Opens a `sqlx::PgConnection` to the resolved DSN (37.x's
//!     `resolve_dsn` + the 38.f-style application_name stamping).
//!  2. Runs a DEEP `pg_catalog` introspection query that captures
//!     column-name + type-name + `attnotnull` + primary-key membership
//!     + unique-constraint membership + the default expression
//!     (`pg_get_expr(adbin, adrelid)`). Independent of the 37.x
//!     runtime cache — no Hot-path bloat.
//!  3. Hands the rows to the pure builder
//!     [`crate::store_introspect::build_manifest_store`] → returns a
//!     `(ManifestStore, Vec<OmittedColumn>)` pair.
//!  4. Embeds the store entry into a fresh [`Manifest`], refreshes the
//!     content hash, and hands the result + omissions to the CLI
//!     shell for serialization.
//!
//! Honest scope:
//!
//!   - The 37.x runtime `introspect_conn` query is deliberately
//!     LEFT INTACT. It captures `(schema, column_name, type_name)` —
//!     enough for the runtime cache. The deeper query here is a
//!     separate code path; the runtime hot path is unaffected.
//!
//!   - `application_name` for the introspection connection is
//!     stamped `axon-store/<store>/introspect` so DBAs see this is a
//!     CLI / one-shot operation, not adopter traffic.
//!
//!   - Errors propagate as
//!     [`crate::store::error::StoreError`] so the CLI
//!     shell renders them with the same v1.37.0 diagnostic shape
//!     adopters already know.

use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgRow};
use sqlx::{Connection, PgConnection, Row};

use crate::store::postgres_backend::{
    application_name_for_with_namespace, mask_dsn_pub, resolve_dsn, StoreError,
};
use crate::store_introspect::{
    build_manifest_store, IntrospectionRow, OmittedColumn,
};
use crate::store_schema_manifest::Manifest;

/// `application_name` suffix stamped on a CLI introspection session.
/// `axon-store/<store>/introspect` — distinct from runtime sessions,
/// distinct from per-tenant 38.f sessions.
pub const INTROSPECT_NAMESPACE: &str = "introspect";

/// §Fase 38.h — open a one-shot connection, run the deep `pg_catalog`
/// introspection for `store_name`, and assemble the manifest entry
/// + the per-column omission list. Pure-async — no global state.
///
/// `connection` accepts the same forms as the runtime's
/// [`crate::store::postgres_backend::resolve_dsn`]: a literal DSN OR
/// the `env:VAR` form. Returns a wrapped Manifest containing one
/// store entry (keyed by `<resolved-schema>.<store_name>`) AND a list
/// of columns omitted because their type is outside the closed 15-type
/// catalog (NEVER silently lossily mapped per D10 / D6).
pub async fn introspect_store(
    connection: &str,
    store_name: &str,
) -> Result<(Manifest, Vec<OmittedColumn>), StoreError> {
    let dsn = resolve_dsn(connection)?;
    let mut conn = open_introspection_connection(&dsn, store_name).await?;

    // Stage 1: resolve the table's schema via `to_regclass`. This is
    // the v1.37 search-path-correct primary; if it misses we fall
    // through to the search-path-INDEPENDENT scan below.
    let resolved_schema = resolve_table_schema(&mut conn, store_name)
        .await?
        .ok_or_else(|| StoreError::TableNotResolved {
            table: store_name.to_string(),
        })?;

    // Stage 2: run the deep introspection query against the resolved
    // (schema, table) pair, materialise rows.
    let rows = fetch_introspection_rows(&mut conn, &resolved_schema, store_name).await?;
    if rows.is_empty() {
        return Err(StoreError::TableNotResolved {
            table: store_name.to_string(),
        });
    }

    // Stage 3: pure manifest-building handles the catalog mapping +
    // omission tracking.
    let (manifest_store, omissions) = build_manifest_store(&rows);

    // Stage 4: package as a one-store Manifest keyed by
    // `<schema>.<store_name>` — the canonical key shape an adopter's
    // `schema: "<schema>.<store_name>"` form-b declaration looks up.
    let mut manifest = Manifest::new();
    let qualified = format!("{resolved_schema}.{store_name}");
    manifest.stores.insert(qualified, manifest_store);
    manifest.refresh_content_hash();
    Ok((manifest, omissions))
}

/// §Fase 38.h — convenience: introspect MULTIPLE stores in one
/// connection, return one merged Manifest. The CLI uses this for
/// the `--all` / `axon store introspect *` shape.
pub async fn introspect_stores(
    connection: &str,
    store_names: &[String],
) -> Result<(Manifest, Vec<OmittedColumn>), StoreError> {
    let mut merged = Manifest::new();
    let mut all_omissions: Vec<OmittedColumn> = Vec::new();
    for name in store_names {
        let (m, omissions) = introspect_store(connection, name).await?;
        for (key, store) in m.stores {
            merged.stores.insert(key, store);
        }
        all_omissions.extend(omissions);
    }
    merged.refresh_content_hash();
    Ok((merged, all_omissions))
}

/// Open a single-use, namespace-stamped Postgres connection for the
/// introspection. `application_name` = `axon-store/<store>/introspect`
/// so DBA tooling distinguishes CLI sessions from runtime sessions.
async fn open_introspection_connection(
    dsn: &str,
    store_name: &str,
) -> Result<PgConnection, StoreError> {
    let opts = PgConnectOptions::from_str(dsn)
        .map_err(|e| StoreError::PoolInit {
            dsn_masked: mask_dsn_pub(dsn),
            source: e.to_string(),
        })?
        .statement_cache_capacity(0)
        .application_name(&application_name_for_with_namespace(
            store_name,
            Some(INTROSPECT_NAMESPACE),
        ));
    PgConnection::connect_with(&opts)
        .await
        .map_err(|e| StoreError::Connect {
            source: e.to_string(),
        })
}

/// Resolve the table's schema via `to_regclass`, then fall back to a
/// search-path-INDEPENDENT scan if `to_regclass` yields NULL.
/// Returns `Ok(Some(schema))` on success, `Ok(None)` when the table
/// is unknown.
async fn resolve_table_schema(
    conn: &mut PgConnection,
    table: &str,
) -> Result<Option<String>, StoreError> {
    // Primary — search-path-correct.
    // §Fase 38.x.a (D1) — `.persistent(false)` on every `sqlx::query` /
    // `sqlx::query_as` call. An adopter who points `axon store introspect`
    // at a transaction-mode pooler endpoint (Supavisor `:6543`) needs the
    // same collision-safety guarantee as the runtime store path. See
    // `postgres_backend::introspect_conn` for the full rationale.
    let primary: Option<(String,)> = sqlx::query_as(
        "SELECT n.nspname \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE c.oid = to_regclass($1)",
    )
    .persistent(false)
    .bind(format!("\"{table}\""))
    .fetch_optional(&mut *conn)
    .await
    .map_err(|e| StoreError::Query {
        op: "introspect",
        source: e.to_string(),
    })?;
    if let Some((schema,)) = primary {
        return Ok(Some(schema));
    }
    // Fallback — search-path-INDEPENDENT scan across non-system
    // schemas, exact `relname` match.
    let scan: Vec<(String,)> = sqlx::query_as(
        "SELECT n.nspname \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE c.relname = $1 \
           AND c.relkind IN ('r', 'v', 'm', 'p', 'f') \
           AND left(n.nspname, 3) <> 'pg_' \
           AND n.nspname <> 'information_schema' \
         ORDER BY n.nspname",
    )
    .persistent(false)
    .bind(table)
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| StoreError::Query {
        op: "introspect",
        source: e.to_string(),
    })?;
    if scan.len() == 1 {
        return Ok(Some(scan.into_iter().next().unwrap().0));
    }
    if scan.is_empty() {
        return Ok(None);
    }
    Err(StoreError::AmbiguousTable {
        table: table.to_string(),
        schemas: scan.into_iter().map(|(s,)| s).collect(),
    })
}

/// The deep introspection query — joins `pg_class`, `pg_namespace`,
/// `pg_attribute`, `pg_type`, plus subqueries for the column's
/// primary-key membership + unique-constraint membership + default
/// expression. Returns one [`IntrospectionRow`] per non-dropped
/// non-system-column.
async fn fetch_introspection_rows(
    conn: &mut PgConnection,
    schema: &str,
    table: &str,
) -> Result<Vec<IntrospectionRow>, StoreError> {
    let qualified = format!("\"{schema}\".\"{table}\"");
    // §Fase 38.x.c (D1) — added `a.attidentity AS identity_kind` to
    // the SELECT list. Postgres represents auto-fill via TWO channels:
    // `pg_attrdef.adbin` (legacy SERIAL → `nextval(...)` default) and
    // `pg_attribute.attidentity` (`'a'` = ALWAYS, `'d'` = BY DEFAULT,
    // empty = not identity). The pre-v1.38.3 query read only channel
    // #1; this query reads BOTH. `decode_introspection_row` maps the
    // text-returned attidentity to `Option<char>`.
    let pg_rows = sqlx::query(
        "SELECT \
             a.attname AS column_name, \
             t.typname AS pg_udt, \
             a.attnotnull AS not_null, \
             COALESCE( \
                 (SELECT pg_get_expr(d.adbin, d.adrelid) \
                  FROM pg_catalog.pg_attrdef d \
                  WHERE d.adrelid = a.attrelid AND d.adnum = a.attnum), \
                 '' \
             ) AS default_expression, \
             a.attidentity::text AS identity_kind, \
             EXISTS ( \
                 SELECT 1 FROM pg_catalog.pg_constraint c \
                 WHERE c.conrelid = a.attrelid \
                   AND c.contype = 'p' \
                   AND a.attnum = ANY(c.conkey) \
             ) AS primary_key, \
             EXISTS ( \
                 SELECT 1 FROM pg_catalog.pg_constraint c \
                 WHERE c.conrelid = a.attrelid \
                   AND c.contype = 'u' \
                   AND c.conkey = ARRAY[a.attnum] \
             ) AS unique_col \
         FROM pg_catalog.pg_class cl \
         JOIN pg_catalog.pg_namespace n ON n.oid = cl.relnamespace \
         JOIN pg_catalog.pg_attribute a ON a.attrelid = cl.oid \
         JOIN pg_catalog.pg_type t ON t.oid = a.atttypid \
         WHERE cl.oid = to_regclass($1) \
           AND a.attnum > 0 \
           AND NOT a.attisdropped \
         ORDER BY a.attnum",
    )
    .persistent(false)
    .bind(qualified)
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| StoreError::Query {
        op: "introspect",
        source: e.to_string(),
    })?;

    let mut out: Vec<IntrospectionRow> = Vec::with_capacity(pg_rows.len());
    for row in pg_rows {
        out.push(decode_introspection_row(&row)?);
    }
    Ok(out)
}

fn decode_introspection_row(row: &PgRow) -> Result<IntrospectionRow, StoreError> {
    let column_name: String =
        row.try_get("column_name").map_err(|e| StoreError::Decode {
            column: "column_name".into(),
            pg_type: "name".into(),
            source: e.to_string(),
        })?;
    let pg_udt: String = row.try_get("pg_udt").map_err(|e| StoreError::Decode {
        column: "pg_udt".into(),
        pg_type: "name".into(),
        source: e.to_string(),
    })?;
    let not_null: bool = row.try_get("not_null").map_err(|e| StoreError::Decode {
        column: "not_null".into(),
        pg_type: "bool".into(),
        source: e.to_string(),
    })?;
    let default_expression: String = row
        .try_get("default_expression")
        .map_err(|e| StoreError::Decode {
            column: "default_expression".into(),
            pg_type: "text".into(),
            source: e.to_string(),
        })?;
    let primary_key: bool = row.try_get("primary_key").map_err(|e| StoreError::Decode {
        column: "primary_key".into(),
        pg_type: "bool".into(),
        source: e.to_string(),
    })?;
    let unique_col: bool = row.try_get("unique_col").map_err(|e| StoreError::Decode {
        column: "unique_col".into(),
        pg_type: "bool".into(),
        source: e.to_string(),
    })?;
    // §Fase 38.x.c (D1) — `pg_attribute.attidentity` ships as PG's
    // single-char `name`/`text`. Empty string ('') = not an identity
    // column; 'a' / 'd' = ALWAYS / BY DEFAULT.
    let identity_kind_text: String =
        row.try_get("identity_kind").map_err(|e| StoreError::Decode {
            column: "identity_kind".into(),
            pg_type: "text".into(),
            source: e.to_string(),
        })?;
    let identity_kind: Option<char> = match identity_kind_text.chars().next() {
        Some('a') => Some('a'),
        Some('d') => Some('d'),
        Some(_) | None => None, // empty string OR any non-identity marker
    };
    Ok(IntrospectionRow {
        column_name,
        pg_udt,
        not_null,
        primary_key,
        unique: unique_col,
        default_expression,
        identity_kind,
    })
}

/// §Fase 38.h — render the final adopter-facing output: canonical
/// JSON manifest + a tail of `# omitted: …` comment lines. The CLI
/// shell calls this; the function is pure (no I/O) so the test
/// surface stays large.
pub fn render_introspection_output(
    manifest: &Manifest,
    omissions: &[OmittedColumn],
) -> String {
    let mut out = manifest.canonical_serialize(true);
    if !omissions.is_empty() {
        out.push('\n');
        for omission in omissions {
            out.push_str(&omission.as_comment_line());
            out.push('\n');
        }
    }
    // The canonical-serialize output ends without a trailing newline;
    // omissions block (if any) adds them. When omissions are empty we
    // DELIBERATELY don't add a trailing newline either — keeps the
    // canonical-form invariant adopter tooling expects.
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store_introspect::OmittedColumn;

    #[test]
    fn render_introspection_output_emits_canonical_json_with_no_omission_tail() {
        let mut m = Manifest::new();
        m.refresh_content_hash();
        let out = render_introspection_output(&m, &[]);
        // The canonical-serialize output is the JSON form, no
        // trailing newline.
        assert!(out.contains(r#""version":1"#));
        assert!(!out.contains("# omitted"));
        assert!(!out.ends_with('\n'));
    }

    #[test]
    fn render_introspection_output_appends_per_column_omission_comments() {
        let mut m = Manifest::new();
        m.refresh_content_hash();
        let omissions = vec![
            OmittedColumn {
                name: "tier".into(),
                pg_udt: "tier_enum".into(),
                reason: "outside the v1.38.0 closed type catalog".into(),
            },
            OmittedColumn {
                name: "shape".into(),
                pg_udt: "geometry".into(),
                reason: "outside the v1.38.0 closed type catalog".into(),
            },
        ];
        let out = render_introspection_output(&m, &omissions);
        assert!(out.contains("# omitted: column `tier` (pg type `tier_enum`)"));
        assert!(out.contains("# omitted: column `shape` (pg type `geometry`)"));
        // The omissions block ends with a trailing newline (each line
        // terminated by '\n').
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn introspect_namespace_constant_is_stable() {
        // The `application_name` suffix for CLI sessions is stable +
        // adopter-observable. Pinned so a regression flags the
        // DBA-facing contract change.
        assert_eq!(INTROSPECT_NAMESPACE, "introspect");
    }
}
