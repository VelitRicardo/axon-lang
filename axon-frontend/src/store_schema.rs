//! v1.31.0 (D1) — the closed `axonstore` column-schema catalog,
//! Rust frontend side.
//!
//! Three closed forms an `axonstore` may declare its column schema in:
//!
//!  - **Inline** — `schema { col: Type [constraint…], … }`. The column
//!    schema lives in source. Use case: small static schemas, the
//!    schema that ships with the application source.
//!  - **Manifest reference** — `schema: "qualified.name"`. The column
//!    schema lives in a checked-in `.axon-schema.yml` (or
//!    `.axon-schema.json`) manifest, referenced by qualified name. Use
//!    case: large schemas, schemas captured by `axon store introspect`
//!    against an existing database.
//!  - **Per-tenant env-var schema namespace** — `schema: env:VAR` (or
//!    quoted `schema: "env:VAR"`). The schema NAMESPACE (e.g.
//!    `tenant_42`) is resolved at deploy time from the named
//!    environment variable; the columns themselves come from a
//!    manifest entry keyed on the resolved namespace + table name.
//!    Use case: schema-per-tenant topology.
//!
//! This module defines the AST surface only — the type-checker proof
//! against these declarations lives in v1.31.0 / v1.31.0 (the
//! `StoreColumnProof` pass), shipping in subsequent steps.
//!
//! Mirror: `axon/compiler/ast_nodes.py` (`StoreSchemaNode`,
//! `StoreColumnNode`) — the Python frontend has carried an
//! inline-form-only surface as forward-compat dead code since v1.30.0;
//! v1.31.0 makes both sides authoritative, brings the Rust side to
//! parity, and adds the new manifest-ref + env-var forms cross-stack.

use crate::tokens::Trivia;

// ════════════════════════════════════════════════════════════════════
//  D1 — the closed 15-type catalog (compile-time mirror of the v1.30.0
//  `PgTypeClass` runtime catalog)
// ════════════════════════════════════════════════════════════════════

/// The closed column-type catalog an `axonstore` may declare a column
/// as. Mirrors the v1.30.0 [`crate::ir_nodes::IRStoreColumnType`]
/// surface and the Postgres runtime's `PgTypeClass` (in
/// `axon-rs/src/store/postgres_backend.rs`) one-for-one.
///
/// Source-level surface accepts both the canonical PascalCase name
/// AND a small set of common lowercase aliases (`int` for `Int`,
/// `boolean` for `Bool`, `integer` for `Int`, …) — see
/// [`StoreColumnType::from_token`]. The AST always carries the
/// canonical PascalCase variant; the alias is normalized at parse
/// time.
///
/// A column whose declared type is OUTSIDE this catalog is a parse
/// error at `axon check` time with a precise message + Levenshtein
/// suggestions. The honest-scope boundary is named: Postgres types
/// outside the catalog — `enum`, `domain`, array, `citext`, PostGIS
/// `geometry`, custom composites — remain `UnsupportedColumnType`,
/// tracked for the v1.31.0+ "broaden the catalog" follow-on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoreColumnType {
    Uuid,
    Text,
    Int,
    BigInt,
    Float,
    Double,
    Bool,
    Timestamptz,
    Timestamp,
    Date,
    Time,
    Jsonb,
    Json,
    Bytea,
    Numeric,
}

impl StoreColumnType {
    /// The closed catalog, in canonical declaration order — useful for
    /// exhaustive iteration in tests + the smart-suggest dictionary.
    pub const ALL: &'static [StoreColumnType] = &[
        StoreColumnType::Uuid,
        StoreColumnType::Text,
        StoreColumnType::Int,
        StoreColumnType::BigInt,
        StoreColumnType::Float,
        StoreColumnType::Double,
        StoreColumnType::Bool,
        StoreColumnType::Timestamptz,
        StoreColumnType::Timestamp,
        StoreColumnType::Date,
        StoreColumnType::Time,
        StoreColumnType::Jsonb,
        StoreColumnType::Json,
        StoreColumnType::Bytea,
        StoreColumnType::Numeric,
    ];

    /// The canonical PascalCase declaration name — exactly what an
    /// adopter writes in source and exactly what the IR / manifest
    /// serializes as. Stable surface — adopters tooling can rely on it.
    pub fn canonical_name(self) -> &'static str {
        match self {
            StoreColumnType::Uuid => "Uuid",
            StoreColumnType::Text => "Text",
            StoreColumnType::Int => "Int",
            StoreColumnType::BigInt => "BigInt",
            StoreColumnType::Float => "Float",
            StoreColumnType::Double => "Double",
            StoreColumnType::Bool => "Bool",
            StoreColumnType::Timestamptz => "Timestamptz",
            StoreColumnType::Timestamp => "Timestamp",
            StoreColumnType::Date => "Date",
            StoreColumnType::Time => "Time",
            StoreColumnType::Jsonb => "Jsonb",
            StoreColumnType::Json => "Json",
            StoreColumnType::Bytea => "Bytea",
            StoreColumnType::Numeric => "Numeric",
        }
    }

    /// Parse a source-level token (an identifier or keyword) into a
    /// catalog variant. Accepts the canonical name AND a small set of
    /// common aliases — case-insensitive at the level of the alias
    /// table to maximise ergonomics, but the AST always carries the
    /// canonical variant so the IR is deterministic.
    ///
    /// Aliases (D5 ergonomic floor — not load-bearing, not promised in
    /// the public contract; the canonical name is the supported form):
    ///
    ///   - `int`, `integer`, `int4` → `Int`
    ///   - `bigint`, `int8` → `BigInt`
    ///   - `bool`, `boolean` → `Bool`
    ///   - `text`, `varchar`, `string` → `Text`
    ///   - `uuid` → `Uuid`
    ///   - `float`, `float4`, `real` → `Float`
    ///   - `double`, `float8` → `Double`
    ///   - `timestamptz` → `Timestamptz`
    ///   - `timestamp` → `Timestamp`
    ///   - `date` → `Date`
    ///   - `time` → `Time`
    ///   - `jsonb` → `Jsonb`
    ///   - `json` → `Json`
    ///   - `bytea` → `Bytea`
    ///   - `numeric`, `decimal` → `Numeric`
    ///
    /// Anything else returns `None` — the parser surfaces it as an
    /// `axon-T8xx`-class error with the closed-catalog list.
    pub fn from_token(name: &str) -> Option<StoreColumnType> {
        // Canonical (PascalCase) lookup first — exact-match.
        for &t in Self::ALL {
            if t.canonical_name() == name {
                return Some(t);
            }
        }
        // Alias table — case-insensitive on the source token.
        match name.to_ascii_lowercase().as_str() {
            "int" | "integer" | "int4" => Some(StoreColumnType::Int),
            "bigint" | "int8" => Some(StoreColumnType::BigInt),
            "bool" | "boolean" => Some(StoreColumnType::Bool),
            "text" | "varchar" | "string" => Some(StoreColumnType::Text),
            "uuid" => Some(StoreColumnType::Uuid),
            "float" | "float4" | "real" => Some(StoreColumnType::Float),
            "double" | "float8" => Some(StoreColumnType::Double),
            "timestamptz" => Some(StoreColumnType::Timestamptz),
            "timestamp" => Some(StoreColumnType::Timestamp),
            "date" => Some(StoreColumnType::Date),
            "time" => Some(StoreColumnType::Time),
            "jsonb" => Some(StoreColumnType::Jsonb),
            "json" => Some(StoreColumnType::Json),
            "bytea" => Some(StoreColumnType::Bytea),
            "numeric" | "decimal" => Some(StoreColumnType::Numeric),
            _ => None,
        }
    }

    /// All canonical names — useful for the smart-suggest dictionary
    /// when the parser rejects an unknown type.
    pub fn all_canonical_names() -> Vec<&'static str> {
        Self::ALL.iter().map(|t| t.canonical_name()).collect()
    }
}

impl std::fmt::Display for StoreColumnType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.canonical_name())
    }
}

// ════════════════════════════════════════════════════════════════════
//  AST nodes — inline schema form
// ════════════════════════════════════════════════════════════════════

/// One column entry in an inline `schema { col: Type [constraint…], … }`
/// block.
///
/// The closed-set constraint vocabulary is shared with the Python AST
/// (`StoreColumnNode`): `primary_key`, `auto_increment`, `not_null`,
/// `unique`, `default <literal>`.
#[derive(Debug, Clone)]
pub struct StoreColumn {
    pub name: String,
    pub col_type: StoreColumnType,
    pub primary_key: bool,
    pub auto_increment: bool,
    pub not_null: bool,
    pub unique: bool,
    /// Literal default value, source-text verbatim. The runtime does
    /// not interpolate; the database supplies the default. Empty when
    /// no `default …` constraint is declared.
    pub default_value: String,
    /// v1.31.0 (D2) — `true` iff this column is declared with
    /// `GENERATED ALWAYS AS IDENTITY` or `GENERATED BY DEFAULT AS
    /// IDENTITY` in the live database (`pg_attribute.attidentity` is
    /// `'a'` or `'d'`). Distinct from `auto_increment` (which marks
    /// the legacy SERIAL pattern via a `nextval(...)` default
    /// expression). T803 treats an `identity` column as safe-to-omit
    /// from a `persist` because Postgres auto-fills it.
    ///
    /// Backwards-compatibility (D5): the field defaults to `false`,
    /// matching v1.38.2 behavior for every column. A manifest written
    /// against v1.38.2 round-trips byte-identically.
    pub identity: bool,
    /// v2.26.0 (D1) — `true` iff the column is declared with the
    /// `index` constraint, e.g. `payload: Json index`. This is the
    /// program-visible declaration of an index as a CAPABILITY-HONEST
    /// EFFECT: the index is part of the source the deploy gate sees, never
    /// a silent out-of-band DBA action (doctrine: infrastructure is
    /// visible to the program). The index METHOD is chosen by the backend
    /// from the column type — a `Json`/`Jsonb` column gets a Postgres GIN
    /// path index (`USING gin (col jsonb_path_ops)`), any other column a
    /// plain b-tree. Defaults to `false` — a pre-v2.26.0 manifest round-trips
    /// byte-identically.
    pub indexed: bool,
    /// v2.26.0 (D1) — the OPTIONAL shape LENS on a `Json` / `Jsonb`
    /// column: `payload: Json<UserEvent>` records `Some("UserEvent")`,
    /// a bare `payload: Json` records `None`. The lens is a COMPILE-TIME
    /// expectation only — the column's physical type stays `jsonb` and
    /// the runtime navigates it totally regardless (a declared-but-absent
    /// field degrades to null, never crashes; doctrine
    /// `open_data_is_total`). The type-checker validates that the named
    /// shape is a declared struct `type` (`axon-T840`); the parser
    /// rejects a `<T>` on any non-`Json` column type (`axon-T841`). Only
    /// ever `Some(_)` for `StoreColumnType::{Json, Jsonb}`. Defaults to
    /// `None` — a v1.38–1.23 manifest round-trips byte-identically.
    pub json_shape: Option<String>,
    pub line: u32,
    pub column: u32,
}

/// v2.48.0 — the FIXED, compiler-synthesized column schema of a
/// `backend: secrets` metadata store (doctrine
/// `rotation_without_revelation`). A secrets-backed axonstore is a
/// read-only METADATA view over the tenant's secret custody: the four
/// columns below are everything a flow may ever see — the secret VALUE
/// has no column, no type, and no term that evaluates to it. Declaring
/// an explicit `schema` on a secrets store is `axon-T900` (the shape is
/// law, not adopter choice); write verbs against it are `axon-T897`.
///
/// `expires_at` is nullable BY DESIGN: expiry is declared metadata
/// (`time_is_an_explicit_input`), written by the seeder or the rotation
/// commit — a secret without a declared expiry simply never matches an
/// `expires_at <` filter.
pub fn secrets_metadata_schema(line: u32, column: u32) -> StoreColumnSchema {
    let col = |name: &str, col_type: StoreColumnType, not_null: bool| StoreColumn {
        name: name.to_string(),
        col_type,
        primary_key: false,
        auto_increment: false,
        not_null,
        unique: false,
        default_value: String::new(),
        identity: false,
        indexed: false,
        json_shape: None,
        line,
        column,
    };
    StoreColumnSchema::Inline {
        columns: vec![
            col("key", StoreColumnType::Text, true),
            col("version", StoreColumnType::Int, true),
            col("created_at", StoreColumnType::Timestamptz, true),
            col("expires_at", StoreColumnType::Timestamptz, false),
        ],
        leading_trivia: Vec::new(),
        line,
        column,
    }
}

// ════════════════════════════════════════════════════════════════════
//  AST node — the three closed `schema:` declaration forms
// ════════════════════════════════════════════════════════════════════

/// v1.31.0 (D1) — the three closed forms an `axonstore` may declare
/// its column schema in. The AST captures the form; the v1.31.0 / v1.31.0
/// `StoreColumnProof` pass consumes the resolved column set (regardless
/// of form) and proves every store reference against it.
///
/// `pub` so consumers (the type-checker, the runtime registry, the LSP)
/// can match exhaustively. Variants are `#[non_exhaustive]`-style only
/// at the doc level — additions go through a plan ratification per the
/// founder discipline.
#[derive(Debug, Clone)]
pub enum StoreColumnSchema {
    /// Form (a) — `schema { col: Type [constraint…], … }`.
    Inline {
        columns: Vec<StoreColumn>,
        /// Trivia attached to the opening `schema` keyword.
        leading_trivia: Vec<Trivia>,
        line: u32,
        column: u32,
    },
    /// Form (b) — `schema: "qualified.name"`. The qualified name
    /// resolves against a checked-in manifest entry (`.axon-schema.yml`
    /// / `.axon-schema.json`) at `axon check` time.
    ManifestRef {
        qualified_name: String,
        line: u32,
        column: u32,
    },
    /// Form (c) — `schema: env:VAR` (or quoted `schema: "env:VAR"`).
    /// The env-var resolves to the schema NAMESPACE at deploy time;
    /// the manifest then provides the column set for `<namespace>.<table>`.
    EnvVar {
        /// The env-var name (no `env:` prefix; the prefix was stripped
        /// at parse time).
        var_name: String,
        line: u32,
        column: u32,
    },
}

impl StoreColumnSchema {
    /// `true` iff this is the inline form. Convenience for the
    /// v1.31.0 / v1.31.0 type-checker, which can short-circuit a manifest
    /// lookup when the columns are already in the AST.
    pub fn is_inline(&self) -> bool {
        matches!(self, StoreColumnSchema::Inline { .. })
    }

    /// Returns the inline columns when the form is inline; `None`
    /// otherwise. The type-checker uses this to obtain the column
    /// set without a manifest round-trip.
    pub fn inline_columns(&self) -> Option<&[StoreColumn]> {
        match self {
            StoreColumnSchema::Inline { columns, .. } => Some(columns),
            _ => None,
        }
    }

    /// The source location of the `schema` keyword, for diagnostic
    /// rendering (the v1.20.0 source-context block points at this).
    pub fn loc(&self) -> (u32, u32) {
        match self {
            StoreColumnSchema::Inline { line, column, .. }
            | StoreColumnSchema::ManifestRef { line, column, .. }
            | StoreColumnSchema::EnvVar { line, column, .. } => (*line, *column),
        }
    }

    /// A short form name (`"inline"` / `"manifest_ref"` / `"env_var"`)
    /// for diagnostic prose + the IR's tagged-union serialization.
    pub fn form_name(&self) -> &'static str {
        match self {
            StoreColumnSchema::Inline { .. } => "inline",
            StoreColumnSchema::ManifestRef { .. } => "manifest_ref",
            StoreColumnSchema::EnvVar { .. } => "env_var",
        }
    }
}

// ════════════════════════════════════════════════════════════════════
//  Unit tests — the closed catalog + parse/canonical-form discipline
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_exactly_15_variants() {
        // The plan-vivo section 4 D1 commits to exactly 15 types. A future
        // catalog broadening goes through a plan ratification — this
        // pin catches an accidental addition.
        assert_eq!(StoreColumnType::ALL.len(), 15);
    }

    #[test]
    fn every_variant_has_a_unique_canonical_name() {
        let mut names: Vec<&'static str> =
            StoreColumnType::ALL.iter().map(|t| t.canonical_name()).collect();
        names.sort();
        let total = names.len();
        names.dedup();
        assert_eq!(
            names.len(),
            total,
            "canonical names must be unique across the catalog"
        );
    }

    #[test]
    fn every_canonical_name_parses_back_to_its_variant() {
        for &t in StoreColumnType::ALL {
            assert_eq!(
                StoreColumnType::from_token(t.canonical_name()),
                Some(t),
                "{} did not round-trip",
                t.canonical_name()
            );
        }
    }

    #[test]
    fn common_aliases_resolve_to_the_canonical_variant() {
        for (alias, expected) in [
            ("int", StoreColumnType::Int),
            ("integer", StoreColumnType::Int),
            ("int4", StoreColumnType::Int),
            ("bigint", StoreColumnType::BigInt),
            ("int8", StoreColumnType::BigInt),
            ("bool", StoreColumnType::Bool),
            ("boolean", StoreColumnType::Bool),
            ("text", StoreColumnType::Text),
            ("varchar", StoreColumnType::Text),
            ("string", StoreColumnType::Text),
            ("uuid", StoreColumnType::Uuid),
            ("float", StoreColumnType::Float),
            ("real", StoreColumnType::Float),
            ("double", StoreColumnType::Double),
            ("float8", StoreColumnType::Double),
            ("numeric", StoreColumnType::Numeric),
            ("decimal", StoreColumnType::Numeric),
            ("timestamptz", StoreColumnType::Timestamptz),
            ("timestamp", StoreColumnType::Timestamp),
            ("date", StoreColumnType::Date),
            ("time", StoreColumnType::Time),
            ("jsonb", StoreColumnType::Jsonb),
            ("json", StoreColumnType::Json),
            ("bytea", StoreColumnType::Bytea),
        ] {
            assert_eq!(
                StoreColumnType::from_token(alias),
                Some(expected),
                "alias `{alias}` did not resolve to `{}`",
                expected.canonical_name()
            );
        }
    }

    #[test]
    fn alias_lookup_is_case_insensitive_on_the_alias_table() {
        // Adopter ergonomics — the alias table tolerates case.
        // (Canonical names match exact-case; aliases are case-insensitive.)
        assert_eq!(StoreColumnType::from_token("INTEGER"), Some(StoreColumnType::Int));
        assert_eq!(StoreColumnType::from_token("Boolean"), Some(StoreColumnType::Bool));
        assert_eq!(StoreColumnType::from_token("UUID"), Some(StoreColumnType::Uuid));
    }

    #[test]
    fn unknown_type_names_return_none() {
        for unknown in [
            "Money", "Interval", "Cidr", "Inet", "Macaddr", "Geometry",
            "enum", "domain", "citext", "array", "anything", "", "   ",
            "Tier", "MyCustomType",
        ] {
            assert_eq!(
                StoreColumnType::from_token(unknown),
                None,
                "unknown type `{unknown}` must not resolve"
            );
        }
    }

    #[test]
    fn display_is_canonical_name() {
        for &t in StoreColumnType::ALL {
            assert_eq!(t.to_string(), t.canonical_name());
        }
    }

    #[test]
    fn schema_form_names_are_the_three_closed_forms() {
        let inline = StoreColumnSchema::Inline {
            columns: vec![],
            leading_trivia: vec![],
            line: 0,
            column: 0,
        };
        let manifest_ref = StoreColumnSchema::ManifestRef {
            qualified_name: "public.tenants".into(),
            line: 0,
            column: 0,
        };
        let env_var = StoreColumnSchema::EnvVar {
            var_name: "TENANT_SCHEMA".into(),
            line: 0,
            column: 0,
        };
        assert_eq!(inline.form_name(), "inline");
        assert_eq!(manifest_ref.form_name(), "manifest_ref");
        assert_eq!(env_var.form_name(), "env_var");
        assert!(inline.is_inline());
        assert!(!manifest_ref.is_inline());
        assert!(!env_var.is_inline());
        assert!(inline.inline_columns().is_some());
        assert!(manifest_ref.inline_columns().is_none());
        assert!(env_var.inline_columns().is_none());
    }
}
