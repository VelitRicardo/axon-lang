//! v1.31.0 (D2 — first half) — the `StoreColumnProof` axon-check
//! pass for `where:` clauses.
//!
//! For every store operation in every flow, given a declared
//! [`StoreColumnSchema`] (inline) OR a resolved [`ManifestStore`]
//! entry (forms b/c via 38.c's manifest layer), this pass proves:
//!
//!  - **`axon-T801`** — every `where:` column reference exists in the
//! declared schema. Unknown columns surface a v1.20.0 Levenshtein
//!    "Did you mean X?" hint within edit-distance 2 (the standard cap).
//!  - **`axon-T802`** — every `where:` value (a bound `${param}` OR a
//!    literal) is type-compatible with the column's declared type per
//!    the closed compatibility matrix [`compat_matrix`].
//!  - **`axon-T805`** — propagated from the manifest layer when a
//!    manifest's `content_hash` does not match its canonical content
//!    (the manifest was hand-edited without recomputing the hash).
//!
//! The pass skips silently when no `schema:` is declared (D5 absolute
//! — undeclared stores run the v1.37.0 runtime+deploy path verbatim).
//!
//! # Why a proof-purpose scanner, not the runtime parse_filter
//!
//! The runtime `parse_filter` (in `axon-rs/src/store/filter.rs`)
//! INTERPOLATES `${param}` references at parse time using a runtime
//! `bindings: &HashMap<String,String>` — by the time the AST is
//! built, every parameter has already been substituted to a literal.
//! That is correct for the runtime SQL-compilation path but loses
//! the parameter-name information D2 needs to prove a flow
//! parameter's declared type matches the column's declared type.
//!
//! 38.d therefore ships a small proof-purpose scanner ([`scan_where`])
//! that preserves `${param}` references AS BoundParam refs (no
//! interpolation), classifies literals by lexical shape, and emits
//! the (column, op, value) triples the proof consumes. Honest scope:
//! the scanner is a STRICT SUBSET of the runtime grammar — it
//! recognises the exact same operator + literal + connector set that
//! `parse_filter` does but produces a leaner proof-friendly tree. A
//! malformed `where:` that the runtime parser would reject is also
//! rejected here; the pass remains silent only on filters that BOTH
//! stacks accept.
//!
//! # Form (b) / form (c) resolution at check time
//!
//! When a store declares `schema: "qualified.name"` (form b) or
//! `schema: env:VAR` (form c), the column set lives in a
//! `.axon-schema.json` manifest. [`load_columns_for_form`] resolves
//! the right [`ManifestStore`] entry using the discovery layer from
//! v1.31.0. Form (c) at check time uses a first-match heuristic:
//! look up `<env_var_name>.<store_name>` first; on miss, scan every
//! manifest entry whose key ends in `.<store_name>` and use the first
//! match (the assumption — typically true at deploy — is that
//! per-tenant schemas have IDENTICAL column shapes). Deploy-time
//! verification (D8, in 38.f) still proves the resolved namespace's
//! actual columns against the manifest.
//!
//! Honest scope: when no manifest is available at check time (the
//! discovery layer returned empty), forms (b)/(c) emit no `axon-T8xx`
//! errors — they're not provable without a manifest. The deploy-time
//! gate (D8) is the floor for these forms.

use std::collections::BTreeMap;
use std::path::Path;

use crate::smart_suggest;
use crate::store_schema::{StoreColumn, StoreColumnSchema, StoreColumnType};
use crate::store_schema_manifest::{
    self, Manifest, ManifestError, ManifestStore,
};

// ════════════════════════════════════════════════════════════════════
// Error catalog (axon-T80x family — v1.20.0 source-context blocks)
// ════════════════════════════════════════════════════════════════════

/// The closed axon-T80x error-code family v1.31.0 introduces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProofErrorCode {
    /// `axon-T801` — a `where:` column reference doesn't exist in the
    /// declared schema. Surfaces a Levenshtein "Did you mean X?" hint.
    T801UnknownColumn,
    /// `axon-T802` — a `where:`-clause OR field-block value type
    /// doesn't match its column's declared type.
    T802TypeMismatch,
    /// `axon-T803` — a `persist` omits a NOT-NULL column that has no
    /// default. The row would fail at the database with a NOT NULL
    /// constraint violation; 38.e catches it at compile time.
    T803NotNullOmitted,
    /// `axon-T804` — a `persist`/`mutate` field-block column reference
    /// doesn't exist in the declared schema. Surfaces a Levenshtein
    /// "Did you mean X?" hint.
    T804UnknownField,
    /// `axon-T805` — propagated from the manifest layer when its
    /// `content_hash` mismatches the canonical content.
    T805ManifestHashMismatch,
    /// `axon-T806` — v2.21.0 — a malformed `now() ± interval
    /// '<n> <unit>'` time value in a `where:` clause (a bad `now()`
    /// shape, a non-`u32` amount, an unknown unit, or `LIKE` against a
    /// time value), OR a time comparison against a non-temporal column.
    /// The runtime `parse_filter` rejects the SAME shape (v2.21.0);
    /// 67.a.2 moves that failure from the daemon run to `axon check`.
    T806BadTimeValue,
    /// `axon-T807` — v2.21.0 — a malformed `order_by:` clause: an
    /// empty sort term, a direction that is not `asc`/`desc`, or a
    /// column that does not exist in the declared schema. Mirror of the
    /// runtime `FilterError::BadOrderBy`.
    T807BadOrderBy,
    /// `axon-T808` — v2.21.0 — a malformed `limit:` clause: a literal
    /// that is not a non-negative `u32`, or a `${param}` whose declared
    /// flow-parameter type is not integer-compatible. Mirror of the
    /// runtime `FilterError::BadLimit`.
    T808BadLimit,
    /// `axon-T843` — v2.33.0 — a malformed `aggregate:`/`group_by:`
    /// clause: a function outside the CLOSED catalog (`count` /
    /// `sum(col)` / `avg(col)` / `min(col)` / `max(col)`), a bad column
    /// identifier, a `count` with a column, a column-less `sum`/`avg`/
    /// `min`/`max`, or a malformed group term. Mirror of the runtime
    /// `FilterError::BadAggregate`/`BadGroupBy` grammar rules.
    T843BadAggregate,
    /// `axon-T844` — v2.33.0 — an aggregate/group column proven
    /// against the declared schema: the column does not exist, or
    /// `sum`/`avg` targets a non-numeric column. Only fires when an
    /// inline schema is declared (the `run_38d_where_proof` scope rule).
    T844AggregateColumn,
    /// `axon-T845` — v2.33.0 — a structurally invalid aggregate
    /// combination: `group_by:` without an `aggregate:`, an `aggregate:`
    /// combined with `order_by:`/`limit:` (v1 closed scope — ordering
    /// grouped rows is named deferred scope), or an aggregate column
    /// that is also a group key. Mirror of the runtime
    /// `parse_aggregate_clause` cross-rules.
    T845AggregateCombo,
}

impl ProofErrorCode {
    /// The stable `axon-T8nn` slug for diagnostic rendering + JSON
    /// output + LSP integration.
    pub fn slug(self) -> &'static str {
        match self {
            Self::T801UnknownColumn => "axon-T801",
            Self::T802TypeMismatch => "axon-T802",
            Self::T803NotNullOmitted => "axon-T803",
            Self::T804UnknownField => "axon-T804",
            Self::T805ManifestHashMismatch => "axon-T805",
            Self::T806BadTimeValue => "axon-T806",
            Self::T807BadOrderBy => "axon-T807",
            Self::T808BadLimit => "axon-T808",
            Self::T843BadAggregate => "axon-T843",
            Self::T844AggregateColumn => "axon-T844",
            Self::T845AggregateCombo => "axon-T845",
        }
    }
}

/// One proof failure. Carries the error code, the source location
/// (where in the `.axon` to anchor the v1.20.0 source-context block),
/// and a human-readable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofError {
    pub code: ProofErrorCode,
    pub line: u32,
    pub column: u32,
    pub message: String,
}

impl ProofError {
    fn new(code: ProofErrorCode, line: u32, column: u32, message: String) -> Self {
        Self {
            code,
            line,
            column,
            message,
        }
    }
}

// ════════════════════════════════════════════════════════════════════
//  The unified "declared columns" view
// ════════════════════════════════════════════════════════════════════

/// One declared column — type + nullable flag + whether it carries a
/// default (so the v1.31.0 `axon-T803` NOT-NULL-omission check knows
/// whether the column can be safely omitted from a `persist` block).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredColumn {
    pub name: String,
    pub col_type: StoreColumnType,
    /// `true` iff the column is declared `not_null` OR `primary_key`
    /// (primary keys are implicitly NOT NULL).
    pub not_null: bool,
    /// `true` iff a `default <value>` constraint was declared OR the
    /// column is `auto_increment` (legacy SERIAL via `nextval(...)`
    /// default) OR the column is `identity` (v1.31.0 — `GENERATED
    /// ALWAYS/BY DEFAULT AS IDENTITY`; Postgres auto-fills it). A
    /// NOT-NULL column with `has_default = true` is safe to omit from
    /// a `persist`.
    pub has_default: bool,
    /// Informational — the column is a primary key. The proof does
    /// not USE this for T803/T804 (the `not_null` derivation already
    /// covers it), but adopter-facing diagnostics name it.
    pub primary_key: bool,
    /// v1.31.0 (D2) — `true` iff the column is declared with
    /// `GENERATED ALWAYS AS IDENTITY` or `GENERATED BY DEFAULT AS
    /// IDENTITY`. Folded into `has_default` for T803 (omittable from
    /// `persist`); also surfaced separately so future T802 arms can
    /// reject explicit `persist` values INTO a `GENERATED ALWAYS`
    /// column (Postgres rejects this server-side; the compile-time
    /// proof is a future 38.x.d candidate).
    pub identity: bool,
}

/// The proof's view of a store's declared columns. Built from either
/// an inline [`StoreColumnSchema::Inline`] OR a [`ManifestStore`]
/// entry — the proof code itself is source-agnostic.
#[derive(Debug, Clone, Default)]
pub struct ColumnSet {
    pub columns: BTreeMap<String, DeclaredColumn>,
}

impl ColumnSet {
    /// Construct from an inline column schema (form a). Returns
    /// `None` when the schema is not the inline form — the caller
    /// must use [`Self::from_manifest_store`] for forms b/c.
    pub fn from_inline_schema(schema: &StoreColumnSchema) -> Option<ColumnSet> {
        let StoreColumnSchema::Inline { columns, .. } = schema else {
            return None;
        };
        Some(Self::from_inline_columns(columns))
    }

    /// Construct from an inline column slice (the inner of
    /// `StoreColumnSchema::Inline`). Exposed for testability.
    pub fn from_inline_columns(columns: &[StoreColumn]) -> ColumnSet {
        let mut out = BTreeMap::new();
        for col in columns {
            // v1.31.0 (D3) — `identity: true` columns are
            // safe-to-omit from a `persist` because Postgres auto-fills
            // them. Fold into `has_default` so T803 treats them
            // identically to SERIAL / explicit-default columns.
            let has_default =
                !col.default_value.is_empty() || col.auto_increment || col.identity;
            out.insert(
                col.name.clone(),
                DeclaredColumn {
                    name: col.name.clone(),
                    col_type: col.col_type,
                    not_null: col.not_null || col.primary_key,
                    has_default,
                    primary_key: col.primary_key,
                    identity: col.identity,
                },
            );
        }
        ColumnSet { columns: out }
    }

    /// Construct from a [`ManifestStore`] entry (forms b/c).
    pub fn from_manifest_store(store: &ManifestStore) -> ColumnSet {
        let mut out = BTreeMap::new();
        for (name, mc) in &store.columns {
            // v1.31.0 (D3) — see `from_inline_columns` for rationale.
            let has_default =
                !mc.default_value.is_empty() || mc.auto_increment || mc.identity;
            out.insert(
                name.clone(),
                DeclaredColumn {
                    name: name.clone(),
                    col_type: mc.col_type,
                    not_null: mc.not_null || mc.primary_key,
                    has_default,
                    primary_key: mc.primary_key,
                    identity: mc.identity,
                },
            );
        }
        ColumnSet { columns: out }
    }

    /// `true` iff the named column is declared.
    pub fn contains(&self, name: &str) -> bool {
        self.columns.contains_key(name)
    }

    /// The declared column, or `None`.
    pub fn get(&self, name: &str) -> Option<&DeclaredColumn> {
        self.columns.get(name)
    }

    /// Every declared column name (used for Levenshtein suggestions).
    pub fn names(&self) -> Vec<&str> {
        self.columns.keys().map(|s| s.as_str()).collect()
    }
}

// ════════════════════════════════════════════════════════════════════
//  Flow parameter type map — for axon-T802 type-mismatch proof
// ════════════════════════════════════════════════════════════════════

/// Flow parameter name → its declared axon-language type name (as
/// written by the adopter, e.g. `"String"`, `"Int"`, `"Uuid"`,
/// `"Bool"`). The proof maps these to [`StoreColumnType`]-compatible
/// classes via [`axon_type_to_column_class`].
#[derive(Debug, Clone, Default)]
pub struct FlowParamTypes {
    pub types: BTreeMap<String, String>,
}

impl FlowParamTypes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: String, type_name: String) {
        self.types.insert(name, type_name);
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.types.get(name).map(|s| s.as_str())
    }
}

// ════════════════════════════════════════════════════════════════════
//  The proof-purpose where-scanner
// ════════════════════════════════════════════════════════════════════

/// One predicate observed by the where-scanner. The proof iterates
/// over a `Vec<ScannedPredicate>` and runs the per-predicate
/// validation (column existence + type compatibility).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedPredicate {
    pub column: String,
    pub op: WhereOp,
    pub value: WhereValue,
}

/// The closed set of operators the scanner recognises. Mirror of the
/// runtime `Operator` enum in `axon-rs/src/store/filter.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhereOp {
    Eq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
    Like,
    IsNull,
    IsNotNull,
}

impl WhereOp {
    pub fn is_equality(self) -> bool {
        matches!(self, Self::Eq | Self::NotEq)
    }
    pub fn is_ordering(self) -> bool {
        matches!(self, Self::Lt | Self::Gt | Self::Le | Self::Ge)
    }
    pub fn is_null_check(self) -> bool {
        matches!(self, Self::IsNull | Self::IsNotNull)
    }
}

/// The classified value side of a predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhereValue {
    /// `${name}` or `$name` — a bound flow parameter. The proof
    /// looks up the parameter's declared type in [`FlowParamTypes`]
    /// and runs the compatibility check.
    BoundParam(String),
    /// A literal — classified by lexical shape into one of the
    /// [`LiteralKind`] variants.
    Literal {
        kind: LiteralKind,
        raw: String,
    },
    /// `NULL` keyword (only valid with `IS [NOT] NULL`).
    NullKeyword,
    /// v2.21.0 — a structural time-relative value: `now()`,
    /// optionally offset by `± interval '<n> <unit>'`. The PROOF mirror
    /// of the runtime `filter::TimeValue` (`axon-rs/src/store/filter.rs`,
    /// v2.21.0). `None` = bare `now()`; the offset's amount is a
    /// validated `u32` + a closed-catalog [`TimeUnit`] — exactly the
    /// shape the runtime renders inline. The proof checks the COLUMN is
    /// a temporal type (a time comparison against e.g. a `Text` column
    /// is the T806 error); the value itself carries no adopter string,
    /// so there is no T802 literal-compatibility concern here.
    TimeNow {
        offset: Option<(TimeSign, u32, TimeUnit)>,
    },
}

/// v2.21.0 — `+` / `-` in `now() ± interval '…'`. Proof mirror of
/// the runtime `filter::TimeSign`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeSign {
    Plus,
    Minus,
}

/// v2.21.0 — the closed catalog of interval units. Proof mirror of
/// the runtime `filter::TimeUnit`; the cross-crate parity test
/// (`axon-rs/tests/where_time_parity.rs`) pins the two in
/// lockstep so the dual scanner cannot silently drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeUnit {
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

impl TimeUnit {
    /// Resolve a unit word (singular or plural, case-insensitive). The
    /// whitelist IS the catalog — an un-listed unit is a T806 error.
    fn from_word(w: &str) -> Option<TimeUnit> {
        Some(match w.to_ascii_lowercase().as_str() {
            "second" | "seconds" => TimeUnit::Second,
            "minute" | "minutes" => TimeUnit::Minute,
            "hour" | "hours" => TimeUnit::Hour,
            "day" | "days" => TimeUnit::Day,
            "week" | "weeks" => TimeUnit::Week,
            "month" | "months" => TimeUnit::Month,
            "year" | "years" => TimeUnit::Year,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiteralKind {
    /// A quoted string. May represent a UUID, a timestamp, a JSON
    /// blob, etc. — the proof checks compatibility against the
    /// column type (Text columns accept any string; Uuid/Date/Time
    /// columns accept canonical-form text).
    Text,
    /// An integer literal — `42`, `-7`.
    Int,
    /// A floating-point literal — `3.14`, `-0.5`.
    Float,
    /// `true` or `false`.
    Bool,
}

/// Errors the scanner emits for syntactically-malformed `where:`
/// strings. These are NOT proof errors (axon-T8xx); they're a hint
/// the caller can surface OR ignore (the runtime `parse_filter` will
/// reject the same shape with its own diagnostic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanError {
    /// The expression has a syntactic shape the scanner can't decode.
    /// 38.d documents this as "the runtime parser will surface the
    /// canonical error; 38.d only proves the well-formed subset".
    /// [`check_filter`] swallows this silently (the runtime parser owns
    /// the canonical syntactic diagnostic).
    Malformed { detail: String },
    /// v2.21.0 — a malformed `now() ± interval '<n> <unit>'` time
    /// value. UNLIKE [`ScanError::Malformed`], this is surfaced as a
    /// HARD `axon-T806` compile error by [`check_filter`]: once a value
    /// position opens with `now`, the form is UNAMBIGUOUSLY a time value
    /// (no other grammar production starts with `now`), so a malformed
    /// continuation is a definite adopter error worth catching at
    /// `axon check` rather than deferring to the daemon run (the cycle
    /// 67 / brief #34 failure mode). Backward-compatible: a non-`now`
    /// malformed `where:` still yields [`ScanError::Malformed`].
    BadTimeValue { detail: String },
}

/// Scan a `where:` string into a flat list of predicates. The scan
/// recognises the closed subset of the runtime filter grammar (the
/// surface 35.b's `parse_filter` accepts) sufficient for the D2
/// proof. `${param}` and `$param` references are preserved as
/// [`WhereValue::BoundParam`] — NOT interpolated.
///
/// On a syntactic miss, returns the predicates scanned so far + a
/// [`ScanError::Malformed`] error. The proof caller surfaces this
/// silently (the runtime parser owns the syntactic-error message).
pub fn scan_where(expr: &str) -> Result<Vec<ScannedPredicate>, ScanError> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let tokens = tokenize(trimmed)?;
    let mut out: Vec<ScannedPredicate> = Vec::new();
    let mut i = 0;

    while i < tokens.len() {
        let predicate = parse_predicate(&tokens, &mut i)?;
        out.push(predicate);

        if i < tokens.len() {
            // Consume the connector. Connectors are AND / OR (case-
            // insensitive). Anything else is malformed.
            let connector = match &tokens[i] {
                ScanToken::Word(w)
                    if w.eq_ignore_ascii_case("and") || w.eq_ignore_ascii_case("or") =>
                {
                    i += 1;
                    w.clone()
                }
                other => {
                    return Err(ScanError::Malformed {
                        detail: format!("expected `AND`/`OR` between predicates, got {other:?}"),
                    });
                }
            };
            // Trailing-connector check.
            if i == tokens.len() {
                return Err(ScanError::Malformed {
                    detail: format!("trailing `{connector}` with no following predicate"),
                });
            }
        }
    }
    Ok(out)
}

// ── Tokenizer ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum ScanToken {
    /// Identifier or keyword (`AND`/`OR`/`NOT`/`IS`/`NULL`/`LIKE`).
    Word(String),
    /// Operator symbol (`=`, `!=`, `<>`, `<`, `>`, `<=`, `>=`, `==`).
    Symbol(String),
    /// Quoted string literal — content WITHOUT the surrounding quotes.
    Str(String),
    /// Integer literal — raw token (the proof preserves the spelling).
    Int(String),
    /// Float literal.
    Float(String),
    /// `${name}` or `$name` — bound parameter reference.
    BoundParam(String),
}

fn tokenize(src: &str) -> Result<Vec<ScanToken>, ScanError> {
    let bytes = src.as_bytes();
    let mut out: Vec<ScanToken> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // ── Bound-parameter ${name} or $name ──
        if b == b'$' {
            i += 1;
            let braced = i < bytes.len() && bytes[i] == b'{';
            if braced {
                i += 1;
            }
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            if i == start {
                return Err(ScanError::Malformed {
                    detail: format!("empty parameter reference at position {start}"),
                });
            }
            let name = String::from_utf8_lossy(&bytes[start..i]).to_string();
            if braced {
                if i >= bytes.len() || bytes[i] != b'}' {
                    return Err(ScanError::Malformed {
                        detail: format!("unterminated `${{...}}` reference for `{name}`"),
                    });
                }
                i += 1;
            }
            out.push(ScanToken::BoundParam(name));
            continue;
        }
        // ── Quoted string ──
        if b == b'\'' {
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != b'\'' {
                // Escape: SQL-style doubled single-quote = '' inside a string
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                i += 1;
            }
            if i >= bytes.len() {
                return Err(ScanError::Malformed {
                    detail: format!("unterminated string starting at position {start}"),
                });
            }
            let content = String::from_utf8_lossy(&bytes[start..i]).to_string();
            i += 1;
            out.push(ScanToken::Str(content));
            continue;
        }
        // ── Number ──
        if b.is_ascii_digit() || (b == b'-' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit())
        {
            let start = i;
            if b == b'-' {
                i += 1;
            }
            let mut saw_dot = false;
            while i < bytes.len() {
                let c = bytes[i];
                if c.is_ascii_digit() {
                    i += 1;
                } else if c == b'.' && !saw_dot {
                    saw_dot = true;
                    i += 1;
                } else {
                    break;
                }
            }
            let tok = String::from_utf8_lossy(&bytes[start..i]).to_string();
            if saw_dot {
                out.push(ScanToken::Float(tok));
            } else {
                out.push(ScanToken::Int(tok));
            }
            continue;
        }
        // ── Identifier ──
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = String::from_utf8_lossy(&bytes[start..i]).to_string();
            out.push(ScanToken::Word(word));
            continue;
        }
        // ── Operator symbols ──
        // Multi-char first: `==`, `!=`, `<>`, `<=`, `>=`.
        if i + 1 < bytes.len() {
            let two = std::str::from_utf8(&bytes[i..i + 2]).unwrap_or("");
            if matches!(two, "==" | "!=" | "<>" | "<=" | ">=") {
                out.push(ScanToken::Symbol(two.to_string()));
                i += 2;
                continue;
            }
        }
        // Single-char: `=`, `<`, `>`.
        if matches!(b, b'=' | b'<' | b'>') {
            out.push(ScanToken::Symbol((b as char).to_string()));
            i += 1;
            continue;
        }
        // v2.21.0 — parens + interval sign for the `now() ± interval
        // '…'` time-value form (mirror of the v2.21.0 filter tokenizer). A
        // `-` followed by a digit was already consumed as a negative
        // number above; a standalone `( ) + -` becomes a bare symbol the
        // predicate parser interprets ONLY inside the time-value form —
        // anywhere else it falls through to `ScanError::Malformed`, so no
        // new well-formed surface opens outside the time grammar.
        if matches!(b, b'(' | b')' | b'+' | b'-') {
            out.push(ScanToken::Symbol((b as char).to_string()));
            i += 1;
            continue;
        }
        return Err(ScanError::Malformed {
            detail: format!("unexpected character {:?} at position {i}", b as char),
        });
    }
    Ok(out)
}

// ── Predicate parser ─────────────────────────────────────────────────

fn parse_predicate(
    tokens: &[ScanToken],
    cursor: &mut usize,
) -> Result<ScannedPredicate, ScanError> {
    // — Column —
    let column = match tokens.get(*cursor) {
        Some(ScanToken::Word(w)) => {
            // Reject connectors/keywords in column position.
            if matches!(
                w.to_ascii_uppercase().as_str(),
                "AND" | "OR" | "NOT" | "NULL"
            ) {
                return Err(ScanError::Malformed {
                    detail: format!("expected column name, got reserved word `{w}`"),
                });
            }
            w.clone()
        }
        other => {
            return Err(ScanError::Malformed {
                detail: format!("expected column identifier, got {other:?}"),
            });
        }
    };
    *cursor += 1;

    // — Operator —
    let op_token = tokens.get(*cursor).ok_or_else(|| ScanError::Malformed {
        detail: format!("missing operator after column `{column}`"),
    })?;

    let op: WhereOp = match op_token {
        ScanToken::Symbol(s) => match s.as_str() {
            "=" | "==" => WhereOp::Eq,
            "!=" | "<>" => WhereOp::NotEq,
            "<" => WhereOp::Lt,
            ">" => WhereOp::Gt,
            "<=" => WhereOp::Le,
            ">=" => WhereOp::Ge,
            other => {
                return Err(ScanError::Malformed {
                    detail: format!("unknown operator `{other}` after column `{column}`"),
                });
            }
        },
        ScanToken::Word(w) if w.eq_ignore_ascii_case("like") => WhereOp::Like,
        ScanToken::Word(w) if w.eq_ignore_ascii_case("is") => {
            // `IS NULL` or `IS NOT NULL`.
            *cursor += 1;
            let next = tokens.get(*cursor).ok_or_else(|| ScanError::Malformed {
                detail: format!("`IS` requires `NULL` or `NOT NULL` after column `{column}`"),
            })?;
            let mut is_not = false;
            let null_tok = match next {
                ScanToken::Word(w) if w.eq_ignore_ascii_case("not") => {
                    is_not = true;
                    *cursor += 1;
                    tokens.get(*cursor).ok_or_else(|| ScanError::Malformed {
                        detail: format!(
                            "`IS NOT` requires `NULL` after column `{column}`"
                        ),
                    })?
                }
                other => other,
            };
            match null_tok {
                ScanToken::Word(w) if w.eq_ignore_ascii_case("null") => {
                    *cursor += 1;
                    return Ok(ScannedPredicate {
                        column,
                        op: if is_not { WhereOp::IsNotNull } else { WhereOp::IsNull },
                        value: WhereValue::NullKeyword,
                    });
                }
                other => {
                    return Err(ScanError::Malformed {
                        detail: format!(
                            "expected `NULL` after `IS{}`, got {other:?}",
                            if is_not { " NOT" } else { "" }
                        ),
                    });
                }
            }
        }
        other => {
            return Err(ScanError::Malformed {
                detail: format!("expected operator after column `{column}`, got {other:?}"),
            });
        }
    };
    *cursor += 1;

    // — Value —
    let value_token = tokens.get(*cursor).ok_or_else(|| ScanError::Malformed {
        detail: format!("missing value after `{column} {op:?}`"),
    })?;
    // v2.21.0 — a `now`-led value is the structural time form
    // (`now()` / `now() ± interval '<n> <unit>'`). `now` is not a valid
    // bare literal otherwise (it would fall through to the catch-all
    // below), so routing it here does not shadow any pre-67.a value. The
    // helper consumes the multi-token form and advances `cursor`.
    if let ScanToken::Word(w) = value_token {
        if w.eq_ignore_ascii_case("now") {
            let offset = parse_scan_time_value(tokens, cursor, op)?;
            return Ok(ScannedPredicate {
                column,
                op,
                value: WhereValue::TimeNow { offset },
            });
        }
    }
    let value = match value_token {
        ScanToken::Str(s) => WhereValue::Literal {
            kind: LiteralKind::Text,
            raw: s.clone(),
        },
        ScanToken::Int(s) => WhereValue::Literal {
            kind: LiteralKind::Int,
            raw: s.clone(),
        },
        ScanToken::Float(s) => WhereValue::Literal {
            kind: LiteralKind::Float,
            raw: s.clone(),
        },
        ScanToken::BoundParam(n) => WhereValue::BoundParam(n.clone()),
        ScanToken::Word(w) if w.eq_ignore_ascii_case("true") || w.eq_ignore_ascii_case("false") => {
            WhereValue::Literal {
                kind: LiteralKind::Bool,
                raw: w.clone(),
            }
        }
        ScanToken::Word(w) if w.eq_ignore_ascii_case("null") => WhereValue::NullKeyword,
        other => {
            return Err(ScanError::Malformed {
                detail: format!("unexpected value-position token {other:?}"),
            });
        }
    };
    *cursor += 1;
    Ok(ScannedPredicate {
        column,
        op,
        value,
    })
}

/// v2.21.0 — parse `now` `(` `)` `[ (+|-) interval '<n> <unit>' ]`
/// at `*cursor` (the caller verified `tokens[*cursor]` is the `now`
/// word). Advances `*cursor` past every consumed token and returns the
/// offset (`None` = bare `now()`).
///
/// This is the PROOF mirror of the runtime `filter::parse_time_value` +
/// `parse_interval` (`axon-rs/src/store/filter.rs`, v2.21.0) — it
/// accepts EXACTLY the same shapes and rejects EXACTLY the same
/// malformed forms, with [`ScanError::BadTimeValue`] standing in for the
/// runtime's `FilterError::{BadTimeValue,LikeWithTime}`. The cross-crate
/// parity test pins the two parsers in lockstep.
fn parse_scan_time_value(
    tokens: &[ScanToken],
    cursor: &mut usize,
    op: WhereOp,
) -> Result<Option<(TimeSign, u32, TimeUnit)>, ScanError> {
    // `LIKE` against a time value is nonsensical (it is a timestamp, not
    // text) — the runtime rejects it with `FilterError::LikeWithTime`.
    if op == WhereOp::Like {
        return Err(ScanError::BadTimeValue {
            detail: "`LIKE` cannot be applied to a `now()` time value — use \
                     an ordering operator (`<`, `>`, `<=`, `>=`) or `=`/`!=`"
                .to_string(),
        });
    }
    let is_sym = |idx: usize, want: &str| {
        matches!(tokens.get(idx), Some(ScanToken::Symbol(s)) if s == want)
    };
    let base = *cursor; // `tokens[base]` is `now`
    // `now` `(` `)`
    if !is_sym(base + 1, "(") || !is_sym(base + 2, ")") {
        return Err(ScanError::BadTimeValue {
            detail: "expected `now()`".to_string(),
        });
    }
    // optional `± interval '<n> <unit>'`
    let sign = match tokens.get(base + 3) {
        Some(ScanToken::Symbol(s)) if s == "+" => TimeSign::Plus,
        Some(ScanToken::Symbol(s)) if s == "-" => TimeSign::Minus,
        // bare `now()` — no offset.
        _ => {
            *cursor = base + 3;
            return Ok(None);
        }
    };
    match tokens.get(base + 4) {
        Some(ScanToken::Word(w)) if w.eq_ignore_ascii_case("interval") => {}
        _ => {
            return Err(ScanError::BadTimeValue {
                detail: "expected `interval` after the `now()` sign".to_string(),
            })
        }
    }
    let raw = match tokens.get(base + 5) {
        Some(ScanToken::Str(s)) => s,
        _ => {
            return Err(ScanError::BadTimeValue {
                detail: "expected a quoted interval like '30 minutes'".to_string(),
            })
        }
    };
    let (amount, unit) = parse_scan_interval(raw)?;
    *cursor = base + 6;
    Ok(Some((sign, amount, unit)))
}

/// v2.21.0 — parse + validate the `'<n> <unit>'` interval body into
/// a typed `(u32, TimeUnit)`. Proof mirror of the runtime
/// `filter::parse_interval`.
fn parse_scan_interval(raw: &str) -> Result<(u32, TimeUnit), ScanError> {
    let mut parts = raw.split_whitespace();
    let (num, unit, extra) = (parts.next(), parts.next(), parts.next());
    let (num, unit) = match (num, unit, extra) {
        (Some(num), Some(unit), None) => (num, unit),
        _ => {
            return Err(ScanError::BadTimeValue {
                detail: format!("interval '{raw}' must be '<number> <unit>'"),
            })
        }
    };
    let amount = num.parse::<u32>().map_err(|_| ScanError::BadTimeValue {
        detail: format!("interval amount '{num}' is not a non-negative integer"),
    })?;
    let unit = TimeUnit::from_word(unit).ok_or_else(|| ScanError::BadTimeValue {
        detail: format!("unknown interval unit '{unit}'"),
    })?;
    Ok((amount, unit))
}

// ════════════════════════════════════════════════════════════════════
//  Type-compatibility matrix
// ════════════════════════════════════════════════════════════════════

/// `true` iff a literal of `lit` can compare against a column of type
/// `col`. The matrix follows the runtime D4 fallback shape — text
/// canonical-form values are accepted for date/time/uuid columns
/// (matching the v1.37.0 `"col"::text = $N` behaviour).
pub fn literal_compatible_with_column(lit: LiteralKind, col: StoreColumnType) -> bool {
    use LiteralKind as L;
    use StoreColumnType as C;
    match (lit, col) {
        (L::Text, C::Text) => true,
        (L::Text, C::Uuid) => true, // canonical UUID format
        (L::Text, C::Timestamptz | C::Timestamp | C::Date | C::Time) => true,
        (L::Text, C::Jsonb | C::Json) => true,
        (L::Text, C::Bytea) => true, // base64
        (L::Text, C::Numeric) => true, // precision-safe string
        (L::Int, C::Int | C::BigInt) => true,
        (L::Int, C::Float | C::Double) => true,
        (L::Int, C::Numeric) => true,
        (L::Float, C::Float | C::Double | C::Numeric) => true,
        (L::Bool, C::Bool) => true,
        _ => false,
    }
}

/// `true` iff a flow parameter of axon-language type `param_axon` can
/// compare against a column of `col` (closed match table mirroring
/// the runtime D4 + the v1.30.0 supported catalog).
///
/// `param_axon` is the type name as written in the flow header
/// (`String`, `Int`, `Bool`, `Float`, `Uuid`, …). The mapping ignores
/// `Optional<T>` wrapping for the compatibility check — the optional
/// flag handles the nullable concern separately.
pub fn axon_param_compatible_with_column(
    param_axon: &str,
    col: StoreColumnType,
) -> bool {
    let normalised = strip_optional_wrap(param_axon);
    use StoreColumnType as C;
    match (normalised, col) {
        // Text family — String matches any text-shaped column (incl.
        // uuid/date/time/json/bytea — same as the runtime D4 fallback).
        ("String", _) => true,
        ("Text", _) => true,
        // Numeric family.
        ("Int", C::Int | C::BigInt | C::Numeric | C::Float | C::Double) => true,
        ("Integer", C::Int | C::BigInt | C::Numeric | C::Float | C::Double) => true,
        ("BigInt", C::BigInt | C::Numeric | C::Float | C::Double) => true,
        ("Float", C::Float | C::Double | C::Numeric) => true,
        ("Double", C::Float | C::Double | C::Numeric) => true,
        ("Number", C::Int | C::BigInt | C::Float | C::Double | C::Numeric) => true,
        // Boolean.
        ("Bool", C::Bool) => true,
        ("Boolean", C::Bool) => true,
        // Typed exact matches.
        ("Uuid", C::Uuid) => true,
        ("UUID", C::Uuid) => true,
        ("Timestamptz", C::Timestamptz) => true,
        ("Timestamp", C::Timestamp) => true,
        ("Date", C::Date) => true,
        ("Time", C::Time) => true,
        ("Json", C::Json | C::Jsonb) => true,
        ("Jsonb", C::Jsonb | C::Json) => true,
        ("Bytea", C::Bytea) => true,
        _ => false,
    }
}

fn strip_optional_wrap(name: &str) -> &str {
    // Pre-v1.31.0 the optional flag is a separate AST flag. But some
    // adopters write `Optional<T>` literally; strip that for compat
    // purposes.
    if let Some(inner) = name
        .strip_prefix("Optional<")
        .and_then(|s| s.strip_suffix('>'))
    {
        return inner;
    }
    if let Some(inner) = name.strip_prefix("Option<").and_then(|s| s.strip_suffix('>')) {
        return inner;
    }
    name
}

// ════════════════════════════════════════════════════════════════════
//  The proof entry point
// ════════════════════════════════════════════════════════════════════

// ════════════════════════════════════════════════════════════════════
// v1.31.0 (D6) — composite Levenshtein suggestion helpers
//
// The v1.20.0 `smart_suggest::suggest_for` infrastructure returns
//  bare candidate names. 38.g lifts the suggestion to include the
//  column's DECLARED TYPE in the rendered output — adopters reading
//  the error see both the spelling fix AND the type context in one
//  glance:
//
//    Before 38.g: `Did you mean \`tenant_id\`?`
//    After  38.g: `Did you mean column \`tenant_id\` (Uuid)?`
//
// Vertical-aware dictionary integration (the v1.21.0 enterprise
//  hook) layers ON TOP without changes here: an enterprise tenant
//  that registers `medical_record_number` as a synonym for `mrn`
//  passes the AUGMENTED column-name list to `suggest_columns_composite`
//  — the underlying `smart_suggest::suggest` already accepts any
// candidate slice (v1.20.0 D3). Documented for extensibility.
// ════════════════════════════════════════════════════════════════════

/// Render the declared columns as `{name: Type, name: Type, …}` for
/// the body of an axon-T801/T804 error message. Sorted alphabetically
/// (via the underlying `BTreeMap`) so the output is deterministic
/// across runs + cross-stack drift gates.
pub fn format_column_list(columns: &ColumnSet) -> String {
    let parts: Vec<String> = columns
        .columns
        .iter()
        .map(|(name, col)| format!("{name}: {}", col.col_type.canonical_name()))
        .collect();
    parts.join(", ")
}

/// v1.31.0 — produce a composite "Did you mean column `X` (Type)?"
/// hint for an unknown-column situation. Uses the same `MAX_DISTANCE = 2`
/// and `MAX_RESULTS = 3` defaults as v1.20.0 (`suggest_for`) but
/// rewrites the rendering to include the column type.
///
/// Returns the empty string when no candidate is within edit-distance
/// 2 — adopters see no guess-laden hint (mirrors v1.20.0's discipline:
/// a confidently-close suggestion is more useful than a noisy one).
///
/// Examples (with declared columns `tenant_id: Uuid`, `tier: Text`):
///   - `suggest_columns_composite("tenantid", &cs)` →
///     `Did you mean column \`tenant_id\` (Uuid)?`
///   - `suggest_columns_composite("trer", &cs)` →
///     `Did you mean column \`tier\` (Text)?`
///   - `suggest_columns_composite("WildlyDifferent", &cs)` → `""`
pub fn suggest_columns_composite(unknown: &str, columns: &ColumnSet) -> String {
    let names = columns.names();
    let suggestions = smart_suggest::suggest(
        unknown,
        &names,
        smart_suggest::MAX_DISTANCE,
        smart_suggest::MAX_RESULTS,
    );
    if suggestions.is_empty() {
        return String::new();
    }
    // Map each candidate name → "`name` (Type)" pair, preserving the
    // Levenshtein order from `suggest`.
    let labelled: Vec<String> = suggestions
        .iter()
        .filter_map(|name| {
            columns
                .get(name)
                .map(|c| format!("`{name}` ({})", c.col_type.canonical_name()))
        })
        .collect();
    if labelled.is_empty() {
        return String::new();
    }
    match labelled.len() {
        1 => format!("Did you mean column {}?", labelled[0]),
        2 => format!(
            "Did you mean column {} or {}?",
            labelled[0], labelled[1]
        ),
        _ => {
            let last = labelled.last().unwrap();
            let head: Vec<String> = labelled[..labelled.len() - 1].to_vec();
            format!(
                "Did you mean column {}, or {}?",
                head.join(", "),
                last
            )
        }
    }
}

/// v1.31.0 — produce a "compatible columns in this schema"
/// suggestion for an axon-T802 type-mismatch.
///
/// When a literal-shape (or bound-param-type) doesn't match its
/// declared column type, scan the rest of the schema for columns
/// whose type WOULD accept the value. Returns up to
/// [`MAX_COMPAT_SUGGESTIONS`] columns, formatted with their types.
/// Returns the empty string when no compatible column exists OR
/// when the unmatched column is itself the only candidate (we don't
/// suggest the column that just failed).
///
/// Adopter ergonomics: a `where: "tenant_id = 42"` against `tenant_id:
/// Uuid` surfaces the T802 mismatch + a hint like
/// "Compatible Int-class columns in this schema: `account_id` (Int)."
pub fn suggest_type_compatible_columns_for_literal(
    lit: LiteralKind,
    columns: &ColumnSet,
    excluded_column: &str,
) -> String {
    let compat: Vec<&DeclaredColumn> = columns
        .columns
        .values()
        .filter(|c| c.name != excluded_column && literal_compatible_with_column(lit, c.col_type))
        .take(MAX_COMPAT_SUGGESTIONS)
        .collect();
    render_compat_suggestions(&compat, format!("{lit:?}-class"))
}

/// v1.31.0 — twin of the literal helper for the bound-param side.
pub fn suggest_type_compatible_columns_for_param(
    param_axon_type: &str,
    columns: &ColumnSet,
    excluded_column: &str,
) -> String {
    let compat: Vec<&DeclaredColumn> = columns
        .columns
        .values()
        .filter(|c| {
            c.name != excluded_column
                && axon_param_compatible_with_column(param_axon_type, c.col_type)
        })
        .take(MAX_COMPAT_SUGGESTIONS)
        .collect();
    render_compat_suggestions(&compat, format!("`{param_axon_type}`-compatible"))
}

/// Max number of "compatible columns" suggestions surfaced for T802.
/// Mirrors `smart_suggest::MAX_RESULTS` — 3 candidates is the
/// adopter-ergonomic sweet spot.
pub const MAX_COMPAT_SUGGESTIONS: usize = 3;

fn render_compat_suggestions(compat: &[&DeclaredColumn], class_label: String) -> String {
    if compat.is_empty() {
        return String::new();
    }
    let labelled: Vec<String> = compat
        .iter()
        .map(|c| format!("`{}` ({})", c.name, c.col_type.canonical_name()))
        .collect();
    let joined = labelled.join(", ");
    format!("Compatible {class_label} columns in this schema: {joined}.")
}

/// Run the D2 proof against `where_expr` given a declared column set
/// and the flow parameters in scope. Returns every proof failure
/// (`axon-T801` / `axon-T802`) anchored at `where_loc` for v1.20.0
/// source-context rendering.
///
/// Empty `where:` clauses skip silently (no predicates → nothing to
/// prove). Syntactically-malformed where strings ALSO skip silently
/// — the runtime parser surfaces the canonical syntactic error;
/// 38.d only proves the well-formed subset.
pub fn check_filter(
    where_expr: &str,
    columns: &ColumnSet,
    flow_params: &FlowParamTypes,
    where_loc: (u32, u32),
) -> Vec<ProofError> {
    let predicates = match scan_where(where_expr) {
        Ok(ps) => ps,
        // v2.21.0 — a malformed time value is a HARD compile error
        // (the runtime `parse_filter` rejects the same shape; 67.a.2
        // moves that failure to `axon check`).
        Err(ScanError::BadTimeValue { detail }) => {
            return vec![ProofError::new(
                ProofErrorCode::T806BadTimeValue,
                where_loc.0,
                where_loc.1,
                format!(
                    "axon-T806 malformed time value: {detail} — the supported \
                     forms are `now()` and `now() ± interval '<n> <unit>'` \
                     where <unit> is \
                     second/minute/hour/day/week/month/year (e.g. \
                     `last_activity_at < now() - interval '30 minutes'`)"
                ),
            )];
        }
        // A non-time syntactic miss stays silent — the runtime parser
        // owns the canonical diagnostic (the pre-67.a.2 38.d contract).
        Err(ScanError::Malformed { .. }) => return Vec::new(),
    };
    let mut out: Vec<ProofError> = Vec::new();
    for pred in predicates {
        check_predicate(&pred, columns, flow_params, where_loc, &mut out);
    }
    out
}

fn check_predicate(
    pred: &ScannedPredicate,
    columns: &ColumnSet,
    flow_params: &FlowParamTypes,
    where_loc: (u32, u32),
    out: &mut Vec<ProofError>,
) {
    // T801 unknown column (v1.31.0 — composite suggestion) —
    let Some(col) = columns.get(&pred.column) else {
        let suggestion = suggest_columns_composite(&pred.column, columns);
        let suggest_suffix = if suggestion.is_empty() {
            String::new()
        } else {
            format!(" {suggestion}")
        };
        out.push(ProofError::new(
            ProofErrorCode::T801UnknownColumn,
            where_loc.0,
            where_loc.1,
            format!(
                "axon-T801 unknown column `{}` in `where:` clause. The \
                 declared schema has columns: {{{}}}.{suggest_suffix}",
                pred.column,
                format_column_list(columns),
            ),
        ));
        return;
    };

    // — IS [NOT] NULL — only the column-existence check matters; the
    //   nullability is a semantic concern but isn't a T8xx error
    //   (a `not_null` column compared `IS NULL` is always false but
    //   that's a logic-bug warning, not a type error).
    if pred.op.is_null_check() {
        return;
    }

    // — LIKE requires a Text-class column —
    if pred.op == WhereOp::Like {
        if !matches!(
            col.col_type,
            StoreColumnType::Text | StoreColumnType::Jsonb | StoreColumnType::Json
        ) {
            out.push(ProofError::new(
                ProofErrorCode::T802TypeMismatch,
                where_loc.0,
                where_loc.1,
                format!(
                    "axon-T802 `LIKE` requires a Text-class column. Column \
                     `{}` is declared as `{}`. Use `=` for exact equality, \
                     or change the column to `Text` if pattern matching \
                     is intended.",
                    pred.column,
                    col.col_type.canonical_name()
                ),
            ));
        }
    }

    // Value type compatibility (T802, with v1.31.0 composite hint) —
    match &pred.value {
        WhereValue::Literal { kind, .. } => {
            if !literal_compatible_with_column(*kind, col.col_type) {
                let compat = suggest_type_compatible_columns_for_literal(
                    *kind,
                    columns,
                    &pred.column,
                );
                let compat_suffix = if compat.is_empty() {
                    String::new()
                } else {
                    format!(" {compat}")
                };
                out.push(ProofError::new(
                    ProofErrorCode::T802TypeMismatch,
                    where_loc.0,
                    where_loc.1,
                    format!(
                        "axon-T802 `where:` literal of class {kind:?} is not \
                         type-compatible with column `{}` declared as `{}`. \
                         A {kind:?} literal cannot compare against a {} \
                         column without an explicit conversion.{compat_suffix}",
                        pred.column,
                        col.col_type.canonical_name(),
                        col.col_type.canonical_name()
                    ),
                ));
            }
        }
        WhereValue::BoundParam(name) => {
            // If the parameter is declared in the flow, prove
            // compatibility. If not declared, that's the v1.32.0 D2
            // binding-totality concern — silently ignore here.
            if let Some(axon_type) = flow_params.get(name) {
                if !axon_param_compatible_with_column(axon_type, col.col_type) {
                    let compat = suggest_type_compatible_columns_for_param(
                        axon_type,
                        columns,
                        &pred.column,
                    );
                    let compat_suffix = if compat.is_empty() {
                        String::new()
                    } else {
                        format!(" {compat}")
                    };
                    out.push(ProofError::new(
                        ProofErrorCode::T802TypeMismatch,
                        where_loc.0,
                        where_loc.1,
                        format!(
                            "axon-T802 flow parameter `${{{name}}}` of type \
                             `{axon_type}` is not type-compatible with \
                             column `{}` declared as `{}`. Either align the \
                             parameter type with the column type, or convert \
                             the value at the binding site.{compat_suffix}",
                            pred.column,
                            col.col_type.canonical_name()
                        ),
                    ));
                }
            }
        }
        WhereValue::NullKeyword => {
            // `col = NULL` is malformed SQL (and the runtime parse_filter
            // rejects it). The scanner shouldn't surface NullKeyword
            // except in `IS [NOT] NULL` context; if it does, it's a
            // proof-irrelevant syntactic anomaly.
        }
        WhereValue::TimeNow { .. } => {
            // v2.21.0 — a `now()` time value renders inline against
            // a timestamp expression. The column MUST be a temporal type
            // (the runtime emits `"col" {op} now() …`, which Postgres
            // rejects against a non-temporal column). Catching it here
            // turns that runtime `operator does not exist` failure into a
            // clear compile error. The value itself carries no adopter
            // string, so there is no literal/param compatibility branch.
            if !matches!(
                col.col_type,
                StoreColumnType::Timestamptz
                    | StoreColumnType::Timestamp
                    | StoreColumnType::Date
                    | StoreColumnType::Time
            ) {
                out.push(ProofError::new(
                    ProofErrorCode::T806BadTimeValue,
                    where_loc.0,
                    where_loc.1,
                    format!(
                        "axon-T806 `now()` time value compared against column \
                         `{}` declared as `{}` — a time/`interval` comparison \
                         requires a temporal column (Timestamptz/Timestamp/\
                         Date/Time). Compare a timestamp column instead (e.g. \
                         `last_activity_at < now() - interval '30 minutes'`).",
                        pred.column,
                        col.col_type.canonical_name()
                    ),
                ));
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// v2.21.0 — bounded + ordered retrieve proof (`order_by:` / `limit:`)
// ════════════════════════════════════════════════════════════════════

/// v2.21.0 — validate a bounded/ordered `retrieve`'s `order_by:` +
/// `limit:` clauses at `axon check`, the compile-time mirror of the
/// runtime `filter::{parse_order_by, parse_limit}` (the cross-crate
/// parity test pins them in lockstep).
///
/// `columns` is `Some` only when the store declares an inline schema —
/// `order_by` column EXISTENCE is then proven (`axon-T807`). The
/// structural checks (sort-term shape + direction, the `limit:` literal
/// `u32` / `${param}` type) run regardless of schema form.
pub fn check_bounds(
    order_by: &str,
    limit_expr: &str,
    columns: Option<&ColumnSet>,
    flow_params: &FlowParamTypes,
    loc: (u32, u32),
) -> Vec<ProofError> {
    let mut out: Vec<ProofError> = Vec::new();
    check_order_by(order_by, columns, loc, &mut out);
    check_limit(limit_expr, flow_params, loc, &mut out);
    out
}

/// v2.33.0 — the compile-time aggregate proof: the `aggregate:` /
/// `group_by:` clauses proven BEFORE the daemon tick / request ever runs.
/// The exact mirror of the runtime `filter::parse_aggregate_clause`
/// grammar + cross-rules (axon-T843 / axon-T845), plus the schema-backed
/// column proof (axon-T844) the runtime cannot do (it has no declared
/// schema) — the same shift-left split as `check_bounds` (T807/T808).
///
/// Grammar / structural checks run for ANY store form; column existence
/// + numeric-family checks only when an inline schema is declared.
pub fn check_aggregate(
    aggregate: &str,
    group_by: &str,
    order_by: &str,
    limit_expr: &str,
    columns: Option<&ColumnSet>,
    loc: (u32, u32),
) -> Vec<ProofError> {
    let mut out: Vec<ProofError> = Vec::new();
    let agg_raw = aggregate.trim();
    let has_group = !group_by.trim().is_empty();
    if agg_raw.is_empty() {
        // — group_by without aggregate (T845) —
        if has_group {
            out.push(ProofError::new(
                ProofErrorCode::T845AggregateCombo,
                loc.0,
                loc.1,
                "axon-T845 `group_by:` requires an `aggregate:` — grouping \
                 without an aggregate has no result shape."
                    .to_string(),
            ));
        }
        return out;
    }

    // — the closed function catalog + column-argument shape (T843) —
    // Mirrors the runtime `parse_aggregate` byte-for-byte in its rules.
    let (func_name, agg_column): (&str, Option<&str>) = match agg_raw.find('(') {
        None => (agg_raw, None),
        Some(open) => {
            if !agg_raw.ends_with(')') {
                out.push(ProofError::new(
                    ProofErrorCode::T843BadAggregate,
                    loc.0,
                    loc.1,
                    format!("axon-T843 `aggregate: \"{agg_raw}\"` is missing the closing `)`."),
                ));
                return out;
            }
            let col = agg_raw[open + 1..agg_raw.len() - 1].trim();
            if col.is_empty() {
                out.push(ProofError::new(
                    ProofErrorCode::T843BadAggregate,
                    loc.0,
                    loc.1,
                    format!(
                        "axon-T843 `{}(…)` needs a column argument.",
                        agg_raw[..open].trim()
                    ),
                ));
                return out;
            }
            (agg_raw[..open].trim(), Some(col))
        }
    };
    let func = func_name.to_ascii_lowercase();
    let is_known = matches!(func.as_str(), "count" | "sum" | "avg" | "min" | "max");
    if !is_known {
        out.push(ProofError::new(
            ProofErrorCode::T843BadAggregate,
            loc.0,
            loc.1,
            format!(
                "axon-T843 `{func_name}` is not an aggregate function. The \
                 closed catalog is `count`, `sum(<col>)`, `avg(<col>)`, \
                 `min(<col>)`, `max(<col>)`."
            ),
        ));
        return out;
    }
    match (func.as_str(), agg_column) {
        ("count", Some(col)) => {
            out.push(ProofError::new(
                ProofErrorCode::T843BadAggregate,
                loc.0,
                loc.1,
                format!(
                    "axon-T843 `count` takes no column (found `{col}`) — it \
                     counts rows; to bound WHICH rows, use `where:`."
                ),
            ));
            return out;
        }
        ("count", None) => {}
        (_, None) => {
            out.push(ProofError::new(
                ProofErrorCode::T843BadAggregate,
                loc.0,
                loc.1,
                format!("axon-T843 `{func}` needs a column argument."),
            ));
            return out;
        }
        (_, Some(col)) => {
            if !is_safe_sql_identifier(col) {
                out.push(ProofError::new(
                    ProofErrorCode::T843BadAggregate,
                    loc.0,
                    loc.1,
                    format!(
                        "axon-T843 aggregate column `{col}` is not a valid \
                         identifier ([A-Za-z_][A-Za-z0-9_]*, ≤ 63 bytes)."
                    ),
                ));
                return out;
            }
        }
    }

    // — the v1 bounds combination (T845) —
    if !order_by.trim().is_empty() || !limit_expr.trim().is_empty() {
        out.push(ProofError::new(
            ProofErrorCode::T845AggregateCombo,
            loc.0,
            loc.1,
            "axon-T845 `aggregate:` cannot combine with `order_by:` / \
             `limit:` yet (ordering grouped results is deferred scope) — \
             drop the bounds or the aggregate."
                .to_string(),
        ));
    }

    // — group_by terms: identifier grammar (T843), duplicates (T843),
    //   aggregate-column-as-group-key (T845), existence (T844) —
    let mut groups: Vec<&str> = Vec::new();
    if has_group {
        for term in group_by.split(',') {
            let column = term.trim();
            if column.is_empty() {
                out.push(ProofError::new(
                    ProofErrorCode::T843BadAggregate,
                    loc.0,
                    loc.1,
                    "axon-T843 empty group term in `group_by:` (a stray comma?)."
                        .to_string(),
                ));
                continue;
            }
            if !is_safe_sql_identifier(column) {
                out.push(ProofError::new(
                    ProofErrorCode::T843BadAggregate,
                    loc.0,
                    loc.1,
                    format!(
                        "axon-T843 `group_by:` column `{column}` is not a valid \
                         identifier ([A-Za-z_][A-Za-z0-9_]*, ≤ 63 bytes)."
                    ),
                ));
                continue;
            }
            if groups.contains(&column) {
                out.push(ProofError::new(
                    ProofErrorCode::T843BadAggregate,
                    loc.0,
                    loc.1,
                    format!("axon-T843 `group_by:` column `{column}` appears twice."),
                ));
                continue;
            }
            groups.push(column);
        }
    }
    if let Some(col) = agg_column {
        if groups.contains(&col) {
            out.push(ProofError::new(
                ProofErrorCode::T845AggregateCombo,
                loc.0,
                loc.1,
                format!(
                    "axon-T845 column `{col}` is both the aggregate argument \
                     and a group key — aggregate a different column or drop \
                     it from `group_by:`."
                ),
            ));
        }
    }

    // — schema-backed column proof (T844; inline schema only) —
    if let Some(cols) = columns {
        if let Some(col) = agg_column {
            match cols.get(col) {
                None => {
                    let suggestion = suggest_columns_composite(col, cols);
                    let suggest_suffix = if suggestion.is_empty() {
                        String::new()
                    } else {
                        format!(" {suggestion}")
                    };
                    out.push(ProofError::new(
                        ProofErrorCode::T844AggregateColumn,
                        loc.0,
                        loc.1,
                        format!(
                            "axon-T844 unknown column `{col}` in `aggregate:`. \
                             The declared schema has columns: {{{}}}.{suggest_suffix}",
                            format_column_list(cols),
                        ),
                    ));
                }
                Some(decl) if matches!(func.as_str(), "sum" | "avg") => {
                    use crate::store_schema::StoreColumnType as C;
                    let numeric = matches!(
                        decl.col_type,
                        C::Int | C::BigInt | C::Float | C::Double | C::Numeric
                    );
                    if !numeric {
                        out.push(ProofError::new(
                            ProofErrorCode::T844AggregateColumn,
                            loc.0,
                            loc.1,
                            format!(
                                "axon-T844 `{func}(\"{col}\")` needs a numeric \
                                 column (Int/BigInt/Float/Double/Numeric); \
                                 `{col}` is declared `{:?}`.",
                                decl.col_type
                            ),
                        ));
                    }
                }
                Some(_) => {}
            }
        }
        for col in &groups {
            if cols.get(col).is_none() {
                let suggestion = suggest_columns_composite(col, cols);
                let suggest_suffix = if suggestion.is_empty() {
                    String::new()
                } else {
                    format!(" {suggestion}")
                };
                out.push(ProofError::new(
                    ProofErrorCode::T844AggregateColumn,
                    loc.0,
                    loc.1,
                    format!(
                        "axon-T844 unknown column `{col}` in `group_by:`. \
                         The declared schema has columns: {{{}}}.{suggest_suffix}",
                        format_column_list(cols),
                    ),
                ));
            }
        }
    }
    out
}

fn check_order_by(
    order_by: &str,
    columns: Option<&ColumnSet>,
    loc: (u32, u32),
    out: &mut Vec<ProofError>,
) {
    if order_by.trim().is_empty() {
        return;
    }
    for term in order_by.split(',') {
        let mut parts = term.split_whitespace();
        let column = match parts.next() {
            Some(c) => c,
            None => {
                out.push(ProofError::new(
                    ProofErrorCode::T807BadOrderBy,
                    loc.0,
                    loc.1,
                    "axon-T807 empty sort term in `order_by:` (a stray comma?) \
                     — a term is `column [asc|desc]`"
                        .to_string(),
                ));
                continue;
            }
        };
        // — identifier discipline (always — the runtime `parse_order_by`
        //   rejects an unsafe column via `is_safe_identifier`; mirror it
        //   here so a schema-less store still catches e.g. `1id`). —
        if !is_safe_sql_identifier(column) {
            out.push(ProofError::new(
                ProofErrorCode::T807BadOrderBy,
                loc.0,
                loc.1,
                format!(
                    "axon-T807 `order_by:` column `{column}` is not a valid \
                     identifier ([A-Za-z_][A-Za-z0-9_]*, ≤ 63 bytes)."
                ),
            ));
            continue;
        }
        // — direction —
        match parts.next() {
            None => {}
            Some(d) if d.eq_ignore_ascii_case("asc") || d.eq_ignore_ascii_case("desc") => {}
            Some(other) => {
                out.push(ProofError::new(
                    ProofErrorCode::T807BadOrderBy,
                    loc.0,
                    loc.1,
                    format!(
                        "axon-T807 `order_by:` direction `{other}` on column \
                         `{column}` is not `asc` or `desc`."
                    ),
                ));
            }
        }
        if let Some(extra) = parts.next() {
            out.push(ProofError::new(
                ProofErrorCode::T807BadOrderBy,
                loc.0,
                loc.1,
                format!(
                    "axon-T807 unexpected `{extra}` after `order_by:` term \
                     `{column}` — a term is `column [asc|desc]`."
                ),
            ));
        }
        // — column existence (only with an inline schema) —
        if let Some(cols) = columns {
            if cols.get(column).is_none() {
                let suggestion = suggest_columns_composite(column, cols);
                let suggest_suffix = if suggestion.is_empty() {
                    String::new()
                } else {
                    format!(" {suggestion}")
                };
                out.push(ProofError::new(
                    ProofErrorCode::T807BadOrderBy,
                    loc.0,
                    loc.1,
                    format!(
                        "axon-T807 unknown column `{column}` in `order_by:`. \
                         The declared schema has columns: {{{}}}.{suggest_suffix}",
                        format_column_list(cols),
                    ),
                ));
            }
        }
    }
}

fn check_limit(
    limit_expr: &str,
    flow_params: &FlowParamTypes,
    loc: (u32, u32),
    out: &mut Vec<ProofError>,
) {
    let raw = limit_expr.trim();
    if raw.is_empty() {
        return;
    }
    // — a `${param}` / `$param` reference — prove its declared type is
    // integer-compatible (an undeclared param is the v1.32.0 D2 binding-
    //   totality concern; silently ignored here, mirroring 38.d). —
    if let Some(name) = bound_param_name(raw) {
        if let Some(axon_type) = flow_params.get(name) {
            if !param_is_integer(axon_type) {
                out.push(ProofError::new(
                    ProofErrorCode::T808BadLimit,
                    loc.0,
                    loc.1,
                    format!(
                        "axon-T808 `limit:` parameter `${{{name}}}` of type \
                         `{axon_type}` is not integer-compatible — `limit:` \
                         requires a non-negative integer (Int/Integer/BigInt)."
                    ),
                ));
            }
        }
        return;
    }
    // — a literal — must be a non-negative `u32`. —
    if raw.parse::<u32>().is_err() {
        out.push(ProofError::new(
            ProofErrorCode::T808BadLimit,
            loc.0,
            loc.1,
            format!(
                "axon-T808 `limit: {raw}` is not a non-negative integer in \
                 `u32` range. Use a literal (`limit: 100`) or a `${{binding}}` \
                 that resolves to one."
            ),
        ));
    }
}

/// `true` iff `name` is a safe SQL identifier — ASCII
/// `[A-Za-z_][A-Za-z0-9_]*`, 1..=63 bytes. The compile-time mirror of
/// the runtime `filter::is_safe_identifier`, so the `order_by:` proof
/// rejects exactly the columns the runtime renderer would.
fn is_safe_sql_identifier(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name.bytes().enumerate().all(|(i, b)| {
            b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit())
        })
}

/// Extract the parameter name from a `${name}` / `$name` reference, or
/// `None` if `raw` is not a bound-parameter reference.
fn bound_param_name(raw: &str) -> Option<&str> {
    if let Some(rest) = raw.strip_prefix("${") {
        return rest.strip_suffix('}').filter(|n| !n.is_empty());
    }
    if let Some(rest) = raw.strip_prefix('$') {
        if !rest.is_empty()
            && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Some(rest);
        }
    }
    None
}

/// `true` iff an axon flow-parameter type is integer-compatible (a valid
/// `limit:` binding type). Ignores `Optional<T>` wrapping.
fn param_is_integer(axon_type: &str) -> bool {
    matches!(
        strip_optional_wrap(axon_type),
        "Int" | "Integer" | "BigInt" | "Number"
    )
}

// ════════════════════════════════════════════════════════════════════
// v1.31.0 (D2 — second half) — field-block proof for
//  `persist into <store> { col: value … }` and
//  `mutate <store> { where: … col: value … }` SET assignments
// ════════════════════════════════════════════════════════════════════

/// Classify a `persist`/`mutate` field-block value string into its
/// proof-relevant shape. The runtime parser preserves the value as
/// a single token's string (the token's `.value`); for a
/// `tenant_id: "${tenant_id}"` field the value is `${tenant_id}` —
/// unquoted, with the parameter reference syntactically intact.
///
/// Pure + total — every string maps to exactly one classification.
pub fn classify_field_value(raw: &str) -> WhereValue {
    let trimmed = raw.trim();

    // Bound-parameter forms: `${name}`, `$name`, or — special-case —
    // a literal-string token like `"${name}"` is captured by the
    // parser with the outer quotes stripped, leaving the raw bytes
    // `${name}` which our matcher catches the same way.
    if let Some(rest) = trimmed.strip_prefix('$') {
        let (inner, has_braces) = match rest.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            Some(inner) => (inner, true),
            None => (rest, false),
        };
        if !inner.is_empty()
            && inner
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            && inner
                .chars()
                .next()
                .map(|c| c.is_ascii_alphabetic() || c == '_')
                .unwrap_or(false)
        {
            let _ = has_braces;
            return WhereValue::BoundParam(inner.to_string());
        }
    }

    // Empty value — surfaces as Text "" so the column-existence check
    // still runs; the type-compat against an empty Text is benign.
    if trimmed.is_empty() {
        return WhereValue::Literal {
            kind: LiteralKind::Text,
            raw: String::new(),
        };
    }

    // Integer / float literal.
    if let Some(stripped) = trimmed.strip_prefix('-') {
        if !stripped.is_empty() && stripped.chars().all(|c| c.is_ascii_digit()) {
            return WhereValue::Literal {
                kind: LiteralKind::Int,
                raw: trimmed.to_string(),
            };
        }
    } else if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return WhereValue::Literal {
            kind: LiteralKind::Int,
            raw: trimmed.to_string(),
        };
    }
    if let Ok(_) = trimmed.parse::<f64>() {
        if trimmed.contains('.') {
            return WhereValue::Literal {
                kind: LiteralKind::Float,
                raw: trimmed.to_string(),
            };
        }
    }

    // Bool.
    if trimmed.eq_ignore_ascii_case("true") || trimmed.eq_ignore_ascii_case("false") {
        return WhereValue::Literal {
            kind: LiteralKind::Bool,
            raw: trimmed.to_string(),
        };
    }

    // NULL keyword.
    if trimmed.eq_ignore_ascii_case("null") {
        return WhereValue::NullKeyword;
    }

    // Default: Text literal (catches `'literal'`-stripped strings,
    // multi-interpolation values like `prefix ${a} suffix`, raw
    // identifiers used as values, etc.).
    WhereValue::Literal {
        kind: LiteralKind::Text,
        raw: trimmed.to_string(),
    }
}

/// v1.31.0 — run the D2 proof against a `persist` field block.
///
/// Three error code surfaces:
///   - `axon-T803` for every NOT-NULL column without a default that
///     was OMITTED from `fields` (the row would otherwise fail at the
///     database with a NOT NULL constraint violation).
///   - `axon-T804` for every field whose column doesn't exist in the
///     declared schema (with Levenshtein "Did you mean X?" hint).
///   - `axon-T802` (reused from 38.d) for every value whose
///     classified shape is not type-compatible with its column's
///     declared type.
///
/// Honest scope: when `fields.is_empty()` the persist has no block —
/// the runtime falls back to writing the flow's user bindings (the
/// v1.30.0 backwards-compat path). 38.e treats an empty block as
/// "don't prove" — D5 absolute preservation for adopters who use the
/// blockless form.
pub fn check_persist_fields(
    fields: &[(String, String)],
    columns: &ColumnSet,
    flow_params: &FlowParamTypes,
    op_loc: (u32, u32),
) -> Vec<ProofError> {
    let mut out: Vec<ProofError> = Vec::new();
    if fields.is_empty() {
        return out;
    }

    // — T803 NOT-NULL omission —
    let provided: std::collections::BTreeSet<&str> =
        fields.iter().map(|(c, _)| c.as_str()).collect();
    for (col_name, col) in &columns.columns {
        if col.not_null && !col.has_default && !provided.contains(col_name.as_str()) {
            let pk_hint = if col.primary_key {
                " (primary key)"
            } else {
                ""
            };
            out.push(ProofError::new(
                ProofErrorCode::T803NotNullOmitted,
                op_loc.0,
                op_loc.1,
                format!(
                    "axon-T803 `persist` omits NOT-NULL column `{col_name}`{pk_hint} \
                     declared as `{}` with no default. The row would fail at \
                     the database with a NOT NULL constraint violation. \
                     Either bind a value in the persist field block, declare \
                     a `default <value>` on the column, or make the column \
                     nullable.",
                    col.col_type.canonical_name()
                ),
            ));
        }
    }

    // — T804 + T802 per field —
    check_field_block_columns(fields, columns, flow_params, op_loc, &mut out);
    out
}

/// v1.31.0 — run the D2 proof against a `mutate` SET field block.
///
/// `mutate` SETs are an UPDATE, not an INSERT — so NOT-NULL omission
/// (T803) does NOT apply (the existing row's NOT-NULL columns stay
/// populated unless explicitly nulled, which our grammar doesn't
/// express). T804 + T802 apply identically to persist.
///
/// The `where:` clause of a `mutate` is proven by 38.d's
/// `check_filter` separately — `check_mutate_fields` covers only the
/// SET-assignment side.
pub fn check_mutate_fields(
    fields: &[(String, String)],
    columns: &ColumnSet,
    flow_params: &FlowParamTypes,
    op_loc: (u32, u32),
) -> Vec<ProofError> {
    let mut out: Vec<ProofError> = Vec::new();
    if fields.is_empty() {
        return out;
    }
    check_field_block_columns(fields, columns, flow_params, op_loc, &mut out);
    out
}

/// Shared helper — T804 unknown column + T802 value-type mismatch for
/// every field in the block. Used by both `check_persist_fields` and
/// `check_mutate_fields`.
fn check_field_block_columns(
    fields: &[(String, String)],
    columns: &ColumnSet,
    flow_params: &FlowParamTypes,
    op_loc: (u32, u32),
    out: &mut Vec<ProofError>,
) {
    for (col_name, raw_value) in fields {
        let Some(col) = columns.get(col_name) else {
            // v1.31.0 — composite suggestion includes the type.
            let suggestion = suggest_columns_composite(col_name, columns);
            let suggest_suffix = if suggestion.is_empty() {
                String::new()
            } else {
                format!(" {suggestion}")
            };
            out.push(ProofError::new(
                ProofErrorCode::T804UnknownField,
                op_loc.0,
                op_loc.1,
                format!(
                    "axon-T804 unknown column `{col_name}` in field block. \
                     The declared schema has columns: {{{}}}.{suggest_suffix}",
                    format_column_list(columns)
                ),
            ));
            continue;
        };

        let value = classify_field_value(raw_value);
        match &value {
            WhereValue::Literal { kind, .. } => {
                if !literal_compatible_with_column(*kind, col.col_type) {
                    // v1.31.0 — append compatible-columns hint.
                    let compat = suggest_type_compatible_columns_for_literal(
                        *kind, columns, col_name,
                    );
                    let compat_suffix = if compat.is_empty() {
                        String::new()
                    } else {
                        format!(" {compat}")
                    };
                    out.push(ProofError::new(
                        ProofErrorCode::T802TypeMismatch,
                        op_loc.0,
                        op_loc.1,
                        format!(
                            "axon-T802 field-block literal of class {kind:?} is \
                             not type-compatible with column `{col_name}` \
                             declared as `{}`. A {kind:?} literal cannot \
                             populate a {} column without an explicit \
                             conversion.{compat_suffix}",
                            col.col_type.canonical_name(),
                            col.col_type.canonical_name()
                        ),
                    ));
                }
            }
            WhereValue::BoundParam(name) => {
                if let Some(axon_type) = flow_params.get(name) {
                    if !axon_param_compatible_with_column(axon_type, col.col_type) {
                        let compat = suggest_type_compatible_columns_for_param(
                            axon_type, columns, col_name,
                        );
                        let compat_suffix = if compat.is_empty() {
                            String::new()
                        } else {
                            format!(" {compat}")
                        };
                        out.push(ProofError::new(
                            ProofErrorCode::T802TypeMismatch,
                            op_loc.0,
                            op_loc.1,
                            format!(
                                "axon-T802 field-block flow parameter `${{{name}}}` \
                                 of type `{axon_type}` is not type-compatible with \
                                 column `{col_name}` declared as `{}`. Either align \
                                 the parameter type with the column type, or convert \
                                 at the binding site.{compat_suffix}",
                                col.col_type.canonical_name()
                            ),
                        ));
                    }
                }
                // Undeclared param → silently pass (v1.32.0 D2 concern,
                // mirroring 38.d's policy).
            }
            WhereValue::NullKeyword => {
                if col.not_null {
                    out.push(ProofError::new(
                        ProofErrorCode::T802TypeMismatch,
                        op_loc.0,
                        op_loc.1,
                        format!(
                            "axon-T802 field-block writes `NULL` into NOT-NULL \
                             column `{col_name}` declared as `{}`. Either provide \
                             a non-null value or make the column nullable.",
                            col.col_type.canonical_name()
                        ),
                    ));
                }
            }
            // v2.21.0 — the `now()` time form is a `where:`-only
            // surface; `classify_field_value` never produces `TimeNow`,
            // so this arm is unreachable (a persist/mutate SET-block
            // `now()` is out of v2.21.0 scope). Present for exhaustiveness.
            WhereValue::TimeNow { .. } => {}
        }
    }
}

// ════════════════════════════════════════════════════════════════════
//  Manifest-form resolution (forms b/c — first-match heuristic)
// ════════════════════════════════════════════════════════════════════

/// Resolve a store's declared column set from one of the three D1
/// forms. Returns `None` when:
///  - the schema is undeclared (form a missing), OR
///  - the manifest is unavailable at check time (forms b/c with no
///    manifest in the discovery root)
///
/// Honest scope: form (c) `env:VAR` uses a first-match heuristic
/// (look up `<env_var>.<store_name>`, then any `*.<store_name>`
/// manifest entry) — at deploy, D8 (v1.31.0) does the canonical
/// per-tenant resolution.
pub fn load_columns_for_schema(
    schema: &StoreColumnSchema,
    store_name: &str,
    manifest: Option<&Manifest>,
) -> Option<ColumnSet> {
    match schema {
        StoreColumnSchema::Inline { .. } => ColumnSet::from_inline_schema(schema),
        StoreColumnSchema::ManifestRef { qualified_name, .. } => {
            let m = manifest?;
            let store = m.lookup(qualified_name)?;
            Some(ColumnSet::from_manifest_store(store))
        }
        StoreColumnSchema::EnvVar { var_name, .. } => {
            let m = manifest?;
            // Look up `<env_var_name>.<store_name>` first.
            let exact_key = format!("{var_name}.{store_name}");
            if let Some(store) = m.lookup(&exact_key) {
                return Some(ColumnSet::from_manifest_store(store));
            }
            // First-match heuristic: any manifest entry ending in
            // `.<store_name>`.
            let suffix = format!(".{store_name}");
            for (key, store) in &m.stores {
                if key.ends_with(&suffix) {
                    return Some(ColumnSet::from_manifest_store(store));
                }
            }
            None
        }
    }
}

/// Best-effort manifest discovery from a source-file's directory.
/// Returns `Ok(None)` when no manifests are present (forms b/c then
/// skip silently); `Err` when manifests are present but failed to
/// parse / hash-verify (axon-T805 propagates).
pub fn load_manifest_from_source_dir(source_dir: &Path) -> Result<Option<Manifest>, ProofError> {
    let files = store_schema_manifest::discover_manifest_files(source_dir);
    if files.is_empty() {
        return Ok(None);
    }
    match store_schema_manifest::load_and_merge_manifests(source_dir) {
        Ok(m) => Ok(Some(m)),
        Err(e) => Err(manifest_error_to_proof(e)),
    }
}

fn manifest_error_to_proof(err: ManifestError) -> ProofError {
    let (code, msg) = match &err {
        ManifestError::ContentHashMismatch { .. } => (
            ProofErrorCode::T805ManifestHashMismatch,
            format!("axon-T805 {err}"),
        ),
        _ => (
            ProofErrorCode::T805ManifestHashMismatch,
            format!("axon-T805 {err}"),
        ),
    };
    ProofError::new(code, 0, 0, msg)
}

// ════════════════════════════════════════════════════════════════════
//  Unit tests — 33 cases covering the 38.d plan-vivo target of 30+
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: StoreColumnType, not_null: bool) -> StoreColumn {
        StoreColumn {
            name: name.to_string(),
            col_type: ty,
            primary_key: false,
            auto_increment: false,
            not_null,
            unique: false,
            default_value: String::new(),
            identity: false,
            indexed: false,
            json_shape: None,
            line: 0,
            column: 0,
        }
    }

    fn columns_for(specs: &[(&str, StoreColumnType, bool)]) -> ColumnSet {
        let inline: Vec<StoreColumn> = specs
            .iter()
            .map(|(n, t, nn)| col(n, *t, *nn))
            .collect();
        ColumnSet::from_inline_columns(&inline)
    }

    fn params(specs: &[(&str, &str)]) -> FlowParamTypes {
        let mut p = FlowParamTypes::new();
        for (n, t) in specs {
            p.insert((*n).to_string(), (*t).to_string());
        }
        p
    }

    // ── Scanner happy paths ──────────────────────────────────────────

    #[test]
    fn scan_empty_yields_no_predicates() {
        assert_eq!(scan_where("").unwrap(), vec![]);
        assert_eq!(scan_where("   ").unwrap(), vec![]);
    }

    #[test]
    fn scan_eq_literal_int() {
        let p = scan_where("id = 42").unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].column, "id");
        assert_eq!(p[0].op, WhereOp::Eq);
        match &p[0].value {
            WhereValue::Literal { kind, raw } => {
                assert_eq!(*kind, LiteralKind::Int);
                assert_eq!(raw, "42");
            }
            other => panic!("expected Int literal, got {other:?}"),
        }
    }

    #[test]
    fn scan_eq_quoted_string() {
        let p = scan_where("tier = 'premium'").unwrap();
        assert_eq!(p.len(), 1);
        match &p[0].value {
            WhereValue::Literal { kind, raw } => {
                assert_eq!(*kind, LiteralKind::Text);
                assert_eq!(raw, "premium");
            }
            other => panic!("expected Text literal, got {other:?}"),
        }
    }

    #[test]
    fn scan_recognises_bound_param_braced_and_bare() {
        let p1 = scan_where("id = ${tenant_id}").unwrap();
        let p2 = scan_where("id = $tenant_id").unwrap();
        for p in [&p1, &p2] {
            assert_eq!(p.len(), 1);
            match &p[0].value {
                WhereValue::BoundParam(n) => assert_eq!(n, "tenant_id"),
                other => panic!("expected BoundParam, got {other:?}"),
            }
        }
    }

    #[test]
    fn scan_eq_bool_true_false() {
        let p = scan_where("active = true").unwrap();
        match &p[0].value {
            WhereValue::Literal { kind: LiteralKind::Bool, raw } => assert_eq!(raw, "true"),
            other => panic!("got {other:?}"),
        }
        let p2 = scan_where("active = false").unwrap();
        match &p2[0].value {
            WhereValue::Literal { kind: LiteralKind::Bool, raw } => assert_eq!(raw, "false"),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn scan_float_literal() {
        let p = scan_where("price = 3.14").unwrap();
        match &p[0].value {
            WhereValue::Literal { kind: LiteralKind::Float, raw } => assert_eq!(raw, "3.14"),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn scan_all_six_comparison_operators() {
        for (src, expected) in [
            ("x = 1", WhereOp::Eq),
            ("x == 1", WhereOp::Eq),
            ("x != 1", WhereOp::NotEq),
            ("x <> 1", WhereOp::NotEq),
            ("x < 1", WhereOp::Lt),
            ("x > 1", WhereOp::Gt),
            ("x <= 1", WhereOp::Le),
            ("x >= 1", WhereOp::Ge),
        ] {
            let p = scan_where(src).unwrap();
            assert_eq!(p[0].op, expected, "src={src}");
        }
    }

    #[test]
    fn scan_like_keyword_case_insensitive() {
        for src in ["name LIKE 'A%'", "name like 'A%'", "name LiKe 'A%'"] {
            let p = scan_where(src).unwrap();
            assert_eq!(p[0].op, WhereOp::Like, "src={src}");
        }
    }

    #[test]
    fn scan_is_null_and_is_not_null() {
        let p1 = scan_where("deleted_at IS NULL").unwrap();
        assert_eq!(p1[0].op, WhereOp::IsNull);
        assert_eq!(p1[0].value, WhereValue::NullKeyword);

        let p2 = scan_where("deleted_at IS NOT NULL").unwrap();
        assert_eq!(p2[0].op, WhereOp::IsNotNull);
    }

    #[test]
    fn scan_multiple_predicates_joined_by_and() {
        let p = scan_where("id = 1 AND tier = 'premium'").unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].column, "id");
        assert_eq!(p[1].column, "tier");
    }

    #[test]
    fn scan_or_connector_is_recognised() {
        let p = scan_where("tier = 'a' OR tier = 'b'").unwrap();
        assert_eq!(p.len(), 2);
    }

    // ── Scanner malformed paths ──────────────────────────────────────

    #[test]
    fn scan_unterminated_string_is_malformed() {
        assert!(matches!(scan_where("name = 'oops"), Err(ScanError::Malformed { .. })));
    }

    #[test]
    fn scan_trailing_connector_is_malformed() {
        assert!(matches!(scan_where("id = 1 AND"), Err(ScanError::Malformed { .. })));
    }

    #[test]
    fn scan_reserved_word_in_column_position_is_malformed() {
        assert!(matches!(scan_where("AND = 1"), Err(ScanError::Malformed { .. })));
    }

    // ── T801 — unknown column ────────────────────────────────────────

    #[test]
    fn t801_unknown_column_with_levenshtein_hint() {
        let cs = columns_for(&[
            ("tenant_id", StoreColumnType::Uuid, true),
            ("tier", StoreColumnType::Text, true),
        ]);
        let errs = check_filter("tenantid = 'x'", &cs, &params(&[]), (1, 1));
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, ProofErrorCode::T801UnknownColumn);
        assert!(errs[0].message.contains("tenantid"));
        assert!(
            errs[0].message.contains("tenant_id"),
            "expected Levenshtein hint pointing at `tenant_id`, got: {}",
            errs[0].message
        );
    }

    #[test]
    fn t801_unknown_column_without_suggestion_when_too_far() {
        let cs = columns_for(&[("tenant_id", StoreColumnType::Uuid, true)]);
        let errs = check_filter("WildlyDifferent = 1", &cs, &params(&[]), (1, 1));
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, ProofErrorCode::T801UnknownColumn);
        assert!(
            !errs[0].message.contains("Did you mean"),
            "an out-of-distance typo must not surface a guess: {}",
            errs[0].message
        );
    }

    #[test]
    fn known_column_passes_silently() {
        let cs = columns_for(&[("id", StoreColumnType::Int, false)]);
        let errs = check_filter("id = 42", &cs, &params(&[]), (1, 1));
        assert!(errs.is_empty(), "expected zero proof errors, got {errs:?}");
    }

    // ── T802 — literal × column-type matrix ──────────────────────────

    #[test]
    fn t802_int_literal_against_text_column_is_rejected() {
        let cs = columns_for(&[("tier", StoreColumnType::Text, true)]);
        let errs = check_filter("tier = 42", &cs, &params(&[]), (1, 1));
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, ProofErrorCode::T802TypeMismatch);
    }

    #[test]
    fn t802_bool_literal_against_uuid_column_is_rejected() {
        let cs = columns_for(&[("id", StoreColumnType::Uuid, true)]);
        let errs = check_filter("id = true", &cs, &params(&[]), (1, 1));
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, ProofErrorCode::T802TypeMismatch);
    }

    #[test]
    fn text_literal_against_uuid_column_passes_canonical_form_rule() {
        let cs = columns_for(&[("id", StoreColumnType::Uuid, true)]);
        let errs = check_filter(
            "id = '83d078e1-b372-42ba-9572-ff8dc521386e'",
            &cs,
            &params(&[]),
            (1, 1),
        );
        assert!(errs.is_empty(), "text literal must match Uuid column, got {errs:?}");
    }

    #[test]
    fn int_literal_against_int_column_passes() {
        let cs = columns_for(&[("id", StoreColumnType::Int, false)]);
        let errs = check_filter("id = 42", &cs, &params(&[]), (1, 1));
        assert!(errs.is_empty());
    }

    #[test]
    fn float_literal_against_numeric_column_passes() {
        let cs = columns_for(&[("amount", StoreColumnType::Numeric, false)]);
        let errs = check_filter("amount = 3.14", &cs, &params(&[]), (1, 1));
        assert!(errs.is_empty());
    }

    #[test]
    fn bool_literal_against_bool_column_passes() {
        let cs = columns_for(&[("active", StoreColumnType::Bool, false)]);
        let errs = check_filter("active = true", &cs, &params(&[]), (1, 1));
        assert!(errs.is_empty());
    }

    #[test]
    fn t802_like_against_uuid_column_is_rejected() {
        let cs = columns_for(&[("id", StoreColumnType::Uuid, true)]);
        let errs = check_filter("id LIKE 'abc%'", &cs, &params(&[]), (1, 1));
        assert!(
            errs.iter().any(|e| e.message.contains("LIKE")),
            "LIKE on Uuid must surface T802 with a `LIKE` mention; got {errs:?}"
        );
    }

    #[test]
    fn like_against_text_column_passes() {
        let cs = columns_for(&[("name", StoreColumnType::Text, false)]);
        let errs = check_filter("name LIKE 'A%'", &cs, &params(&[]), (1, 1));
        assert!(errs.is_empty());
    }

    // ── T802 — bound param × column-type matrix ──────────────────────

    #[test]
    fn bound_param_string_against_uuid_column_passes() {
        // The runtime D4 fallback casts a String parameter to a Uuid
        // column. Compile-time proof must mirror that as
        // type-compatible.
        let cs = columns_for(&[("id", StoreColumnType::Uuid, true)]);
        let fp = params(&[("tenant_id", "String")]);
        let errs = check_filter("id = ${tenant_id}", &cs, &fp, (1, 1));
        assert!(errs.is_empty(), "got {errs:?}");
    }

    #[test]
    fn t802_bound_param_int_against_uuid_column_is_rejected() {
        let cs = columns_for(&[("id", StoreColumnType::Uuid, true)]);
        let fp = params(&[("some_int", "Int")]);
        let errs = check_filter("id = ${some_int}", &cs, &fp, (1, 1));
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, ProofErrorCode::T802TypeMismatch);
        assert!(errs[0].message.contains("some_int"));
        assert!(errs[0].message.contains("Uuid"));
    }

    #[test]
    fn bound_param_int_against_int_column_passes() {
        let cs = columns_for(&[("id", StoreColumnType::Int, false)]);
        let fp = params(&[("the_id", "Int")]);
        let errs = check_filter("id = ${the_id}", &cs, &fp, (1, 1));
        assert!(errs.is_empty());
    }

    #[test]
    fn bound_param_undeclared_in_flow_silently_passes() {
        // A bound param NOT declared as a flow parameter is the
        // v1.32.0 D2 binding-totality concern — 38.d does not
        // duplicate that check.
        let cs = columns_for(&[("id", StoreColumnType::Uuid, true)]);
        let errs = check_filter("id = ${not_a_param}", &cs, &params(&[]), (1, 1));
        assert!(errs.is_empty());
    }

    #[test]
    fn bound_param_with_optional_wrapper_unwraps_correctly() {
        let cs = columns_for(&[("id", StoreColumnType::Int, false)]);
        let fp = params(&[("maybe_id", "Optional<Int>")]);
        let errs = check_filter("id = ${maybe_id}", &cs, &fp, (1, 1));
        assert!(errs.is_empty());
    }

    // ── NULL handling ────────────────────────────────────────────────

    #[test]
    fn is_null_passes_against_any_column() {
        let cs = columns_for(&[("deleted_at", StoreColumnType::Timestamptz, true)]);
        let errs = check_filter("deleted_at IS NULL", &cs, &params(&[]), (1, 1));
        assert!(errs.is_empty());
    }

    #[test]
    fn is_not_null_passes_against_any_column() {
        let cs = columns_for(&[("deleted_at", StoreColumnType::Timestamptz, true)]);
        let errs = check_filter("deleted_at IS NOT NULL", &cs, &params(&[]), (1, 1));
        assert!(errs.is_empty());
    }

    #[test]
    fn is_null_against_unknown_column_still_flags_t801() {
        let cs = columns_for(&[("id", StoreColumnType::Uuid, true)]);
        let errs = check_filter("ghost IS NULL", &cs, &params(&[]), (1, 1));
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, ProofErrorCode::T801UnknownColumn);
    }

    // ── Multi-predicate cases ────────────────────────────────────────

    #[test]
    fn multiple_predicates_all_proven_independently() {
        let cs = columns_for(&[
            ("id", StoreColumnType::Uuid, true),
            ("active", StoreColumnType::Bool, false),
        ]);
        // First predicate good; second has unknown column → exactly
        // one T801.
        let errs = check_filter(
            "id = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa' AND statuz = 'on'",
            &cs,
            &params(&[]),
            (1, 1),
        );
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, ProofErrorCode::T801UnknownColumn);
        assert!(errs[0].message.contains("statuz"));
    }

    #[test]
    fn multiple_errors_in_one_expression_are_all_reported() {
        let cs = columns_for(&[("id", StoreColumnType::Uuid, true)]);
        let errs = check_filter("ghost1 = 1 AND ghost2 = 2", &cs, &params(&[]), (1, 1));
        assert_eq!(errs.len(), 2);
        assert!(errs.iter().all(|e| e.code == ProofErrorCode::T801UnknownColumn));
    }

    // ── 15-type catalog coverage ─────────────────────────────────────

    #[test]
    fn every_catalog_type_accepts_a_string_literal_or_int_or_bool() {
        // Smoke pin: for each of the 15 catalog types, at least one
        // class of literal is accepted (so a real schema declared with
        // ANY catalog type stays proof-able).
        for &t in StoreColumnType::ALL {
            let cs = columns_for(&[("col", t, false)]);
            let candidates = match t {
                StoreColumnType::Bool => vec!["col = true"],
                StoreColumnType::Int
                | StoreColumnType::BigInt => vec!["col = 1"],
                StoreColumnType::Float
                | StoreColumnType::Double => vec!["col = 1.5"],
                _ => vec!["col = 'x'"],
            };
            for src in candidates {
                let errs = check_filter(src, &cs, &params(&[]), (1, 1));
                assert!(
                    errs.is_empty(),
                    "catalog type {} rejected `{src}` unexpectedly: {errs:?}",
                    t.canonical_name()
                );
            }
        }
    }

    #[test]
    fn every_catalog_type_accepts_a_compatible_string_param() {
        // The runtime D4 fallback accepts a String param into any
        // column. Compile-time proof must agree.
        let fp = params(&[("p", "String")]);
        for &t in StoreColumnType::ALL {
            let cs = columns_for(&[("col", t, false)]);
            let errs = check_filter("col = ${p}", &cs, &fp, (1, 1));
            assert!(
                errs.is_empty(),
                "String param rejected by catalog type {}: {errs:?}",
                t.canonical_name()
            );
        }
    }

    // ── Form (b) + (c) — manifest resolution ─────────────────────────

    #[test]
    fn form_b_manifest_ref_resolves_against_provided_manifest() {
        let manifest = Manifest::parse_json(
            r#"{
                "version": 1,
                "stores": {
                    "public.tenants": {
                        "columns": {
                            "tenant_id": { "type": "Uuid", "primary_key": true }
                        }
                    }
                }
            }"#,
        )
        .unwrap();
        let schema = StoreColumnSchema::ManifestRef {
            qualified_name: "public.tenants".to_string(),
            line: 1,
            column: 1,
        };
        let cs = load_columns_for_schema(&schema, "tenants", Some(&manifest)).unwrap();
        assert!(cs.contains("tenant_id"));
    }

    #[test]
    fn form_b_manifest_ref_returns_none_when_manifest_absent() {
        let schema = StoreColumnSchema::ManifestRef {
            qualified_name: "public.tenants".to_string(),
            line: 1,
            column: 1,
        };
        assert!(load_columns_for_schema(&schema, "tenants", None).is_none());
    }

    #[test]
    fn form_c_env_var_uses_first_match_heuristic_when_exact_key_missing() {
        let manifest = Manifest::parse_json(
            r#"{
                "version": 1,
                "stores": {
                    "tenant_42.events": {
                        "columns": {
                            "event_id": { "type": "Uuid" }
                        }
                    }
                }
            }"#,
        )
        .unwrap();
        let schema = StoreColumnSchema::EnvVar {
            var_name: "TENANT_SCHEMA".to_string(),
            line: 1,
            column: 1,
        };
        // Exact `TENANT_SCHEMA.events` is not present → first-match
        // heuristic finds `tenant_42.events`.
        let cs = load_columns_for_schema(&schema, "events", Some(&manifest)).unwrap();
        assert!(cs.contains("event_id"));
    }

    #[test]
    fn form_c_env_var_prefers_exact_key_when_present() {
        let manifest = Manifest::parse_json(
            r#"{
                "version": 1,
                "stores": {
                    "TENANT_SCHEMA.events": {
                        "columns": {
                            "exact_match_only": { "type": "Uuid" }
                        }
                    },
                    "tenant_42.events": {
                        "columns": {
                            "first_match_fallback": { "type": "Text" }
                        }
                    }
                }
            }"#,
        )
        .unwrap();
        let schema = StoreColumnSchema::EnvVar {
            var_name: "TENANT_SCHEMA".to_string(),
            line: 1,
            column: 1,
        };
        let cs = load_columns_for_schema(&schema, "events", Some(&manifest)).unwrap();
        // Exact-key match wins.
        assert!(cs.contains("exact_match_only"));
        assert!(!cs.contains("first_match_fallback"));
    }

    // ── Error code slugs (LSP / JSON output) ─────────────────────────

    #[test]
    fn error_code_slugs_match_the_axon_t801_through_t808_namespace() {
        assert_eq!(ProofErrorCode::T801UnknownColumn.slug(), "axon-T801");
        assert_eq!(ProofErrorCode::T802TypeMismatch.slug(), "axon-T802");
        assert_eq!(ProofErrorCode::T803NotNullOmitted.slug(), "axon-T803");
        assert_eq!(ProofErrorCode::T804UnknownField.slug(), "axon-T804");
        assert_eq!(ProofErrorCode::T805ManifestHashMismatch.slug(), "axon-T805");
        assert_eq!(ProofErrorCode::T806BadTimeValue.slug(), "axon-T806");
        assert_eq!(ProofErrorCode::T807BadOrderBy.slug(), "axon-T807");
        assert_eq!(ProofErrorCode::T808BadLimit.slug(), "axon-T808");
    }

    // ════════════════════════════════════════════════════════════════
    // v1.31.0 — `persist` / `mutate` field-block proof
    // ════════════════════════════════════════════════════════════════

    fn col_full(
        name: &str,
        ty: StoreColumnType,
        not_null: bool,
        primary_key: bool,
        default_value: &str,
        auto_increment: bool,
    ) -> StoreColumn {
        StoreColumn {
            name: name.to_string(),
            col_type: ty,
            primary_key,
            auto_increment,
            not_null,
            unique: false,
            default_value: default_value.to_string(),
            identity: false,
            indexed: false,
            json_shape: None,
            line: 0,
            column: 0,
        }
    }

    fn columns_full(
        specs: &[(&str, StoreColumnType, bool, bool, &str, bool)],
    ) -> ColumnSet {
        let inline: Vec<StoreColumn> = specs
            .iter()
            .map(|(n, t, nn, pk, dv, ai)| col_full(n, *t, *nn, *pk, dv, *ai))
            .collect();
        ColumnSet::from_inline_columns(&inline)
    }

    // ── classify_field_value — the value-shape classifier ────────────

    #[test]
    fn classify_field_value_recognises_braced_bound_param() {
        assert_eq!(
            classify_field_value("${tenant_id}"),
            WhereValue::BoundParam("tenant_id".to_string())
        );
    }

    #[test]
    fn classify_field_value_recognises_bare_bound_param() {
        assert_eq!(
            classify_field_value("$tenant_id"),
            WhereValue::BoundParam("tenant_id".to_string())
        );
    }

    #[test]
    fn classify_field_value_recognises_int_literal() {
        assert_eq!(
            classify_field_value("42"),
            WhereValue::Literal {
                kind: LiteralKind::Int,
                raw: "42".to_string()
            }
        );
        assert_eq!(
            classify_field_value("-7"),
            WhereValue::Literal {
                kind: LiteralKind::Int,
                raw: "-7".to_string()
            }
        );
    }

    #[test]
    fn classify_field_value_recognises_float_literal() {
        match classify_field_value("3.14") {
            WhereValue::Literal { kind: LiteralKind::Float, raw } => assert_eq!(raw, "3.14"),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn classify_field_value_recognises_bool_literal() {
        for src in ["true", "false", "TRUE", "False"] {
            match classify_field_value(src) {
                WhereValue::Literal { kind: LiteralKind::Bool, .. } => {}
                other => panic!("`{src}` → {other:?}"),
            }
        }
    }

    #[test]
    fn classify_field_value_recognises_null_keyword() {
        assert_eq!(classify_field_value("null"), WhereValue::NullKeyword);
        assert_eq!(classify_field_value("NULL"), WhereValue::NullKeyword);
    }

    #[test]
    fn classify_field_value_falls_back_to_text_for_multi_interpolation_or_raw() {
        // A value like `prefix ${a} suffix` is sent as a single string
        // by the runtime — classify as Text.
        match classify_field_value("prefix ${a} suffix") {
            WhereValue::Literal { kind: LiteralKind::Text, .. } => {}
            other => panic!("got {other:?}"),
        }
        // A naked identifier (used as a value) → Text.
        match classify_field_value("standard") {
            WhereValue::Literal { kind: LiteralKind::Text, .. } => {}
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn classify_field_value_empty_string_is_a_text_literal() {
        assert_eq!(
            classify_field_value(""),
            WhereValue::Literal {
                kind: LiteralKind::Text,
                raw: String::new()
            }
        );
    }

    // ── check_persist_fields — T803 NOT-NULL omission ────────────────

    #[test]
    fn t803_persist_omits_not_null_column_with_no_default() {
        let cs = columns_full(&[
            ("tenant_id", StoreColumnType::Uuid, true, true, "", false), // PK = NOT NULL
            ("tier", StoreColumnType::Text, true, false, "", false),     // NOT NULL, no default
        ]);
        let errs = check_persist_fields(
            &[("tenant_id".into(), "${tenant_id}".into())],
            &cs,
            &params(&[("tenant_id", "String")]),
            (1, 1),
        );
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, ProofErrorCode::T803NotNullOmitted);
        assert!(errs[0].message.contains("tier"));
        assert!(errs[0].message.contains("NOT NULL"));
    }

    #[test]
    fn t803_skips_not_null_column_with_default() {
        let cs = columns_full(&[
            ("id", StoreColumnType::Uuid, true, true, "", false),
            ("created_at", StoreColumnType::Timestamptz, true, false, "now()", false),
        ]);
        let errs = check_persist_fields(
            &[("id".into(), "${id}".into())],
            &cs,
            &params(&[("id", "String")]),
            (1, 1),
        );
        // `created_at` has a default → safe to omit.
        assert!(
            errs.is_empty(),
            "default-bearing NOT-NULL column must be omittable: {errs:?}"
        );
    }

    #[test]
    fn t803_skips_auto_increment_column() {
        let cs = columns_full(&[
            ("id", StoreColumnType::Int, true, true, "", true), // auto_increment
            ("name", StoreColumnType::Text, false, false, "", false),
        ]);
        let errs = check_persist_fields(
            &[("name".into(), "'Alice'".into())],
            &cs,
            &params(&[]),
            (1, 1),
        );
        // The auto_increment PK can be omitted.
        assert!(errs.is_empty(), "auto_increment column must be omittable: {errs:?}");
    }

    #[test]
    fn t803_nullable_column_can_be_omitted() {
        let cs = columns_full(&[
            ("id", StoreColumnType::Uuid, true, true, "", false),
            ("notes", StoreColumnType::Text, false, false, "", false), // nullable
        ]);
        let errs = check_persist_fields(
            &[("id".into(), "${id}".into())],
            &cs,
            &params(&[("id", "String")]),
            (1, 1),
        );
        assert!(errs.is_empty());
    }

    #[test]
    fn t803_multiple_omitted_not_null_columns_each_surface_an_error() {
        let cs = columns_full(&[
            ("a", StoreColumnType::Text, true, false, "", false),
            ("b", StoreColumnType::Text, true, false, "", false),
            ("c", StoreColumnType::Text, false, false, "", false), // nullable
        ]);
        let errs = check_persist_fields(&[], &cs, &params(&[]), (1, 1));
        // Empty fields = blockless persist = SKIP (D5 backward-compat).
        assert!(errs.is_empty());
        // With ANY field populated, omitted ones surface.
        let errs2 = check_persist_fields(
            &[("a".into(), "'x'".into())],
            &cs,
            &params(&[]),
            (1, 1),
        );
        assert_eq!(errs2.len(), 1);
        assert_eq!(errs2[0].code, ProofErrorCode::T803NotNullOmitted);
        assert!(errs2[0].message.contains("`b`"));
    }

    // ── check_persist_fields — T804 unknown field ────────────────────

    #[test]
    fn t804_persist_field_typo_with_levenshtein_hint() {
        let cs = columns_full(&[
            ("tenant_id", StoreColumnType::Uuid, true, true, "", false),
            ("tier", StoreColumnType::Text, true, false, "", false),
        ]);
        let errs = check_persist_fields(
            &[
                ("tenant_id".into(), "${tid}".into()),
                ("tier".into(), "'std'".into()),
                ("tenantid".into(), "${tid}".into()), // typo
            ],
            &cs,
            &params(&[("tid", "String")]),
            (1, 1),
        );
        let t804: Vec<&ProofError> = errs
            .iter()
            .filter(|e| e.code == ProofErrorCode::T804UnknownField)
            .collect();
        assert_eq!(t804.len(), 1);
        assert!(t804[0].message.contains("tenantid"));
        assert!(t804[0].message.contains("tenant_id"));
    }

    #[test]
    fn t804_persist_unknown_field_without_suggestion_when_too_far() {
        let cs = columns_full(&[("id", StoreColumnType::Uuid, true, true, "", false)]);
        let errs = check_persist_fields(
            &[
                ("id".into(), "${id}".into()),
                ("CompletelyDifferent".into(), "'x'".into()),
            ],
            &cs,
            &params(&[("id", "String")]),
            (1, 1),
        );
        let t804: Vec<&ProofError> = errs
            .iter()
            .filter(|e| e.code == ProofErrorCode::T804UnknownField)
            .collect();
        assert_eq!(t804.len(), 1);
        assert!(!t804[0].message.contains("Did you mean"));
    }

    // ── check_persist_fields — T802 value-type mismatch in field block

    #[test]
    fn t802_persist_int_literal_against_text_column() {
        let cs = columns_full(&[
            ("id", StoreColumnType::Int, true, true, "", true),
            ("name", StoreColumnType::Text, true, false, "", false),
        ]);
        let errs = check_persist_fields(
            &[("name".into(), "42".into())],
            &cs,
            &params(&[]),
            (1, 1),
        );
        let t802: Vec<&ProofError> = errs
            .iter()
            .filter(|e| e.code == ProofErrorCode::T802TypeMismatch)
            .collect();
        assert_eq!(t802.len(), 1);
        assert!(t802[0].message.contains("Int"));
        assert!(t802[0].message.contains("Text"));
    }

    #[test]
    fn t802_persist_bound_param_type_mismatch_in_field_block() {
        // `String` is universally compatible per the D4-fallback
        // mirror — to trigger T802, use a parameter type that
        // GENUINELY clashes with the column type. `Bool` param
        // against an `Int` column is the cleanest case.
        let cs = columns_full(&[
            ("id", StoreColumnType::Uuid, true, true, "", false),
            ("count", StoreColumnType::Int, true, false, "", false),
        ]);
        let errs = check_persist_fields(
            &[
                ("id".into(), "${id}".into()),
                ("count".into(), "${flag}".into()),
            ],
            &cs,
            &params(&[("id", "String"), ("flag", "Bool")]),
            (1, 1),
        );
        let t802: Vec<&ProofError> = errs
            .iter()
            .filter(|e| e.code == ProofErrorCode::T802TypeMismatch)
            .collect();
        assert_eq!(t802.len(), 1, "expected exactly one T802, got: {errs:?}");
        assert!(t802[0].message.contains("flag"));
        assert!(t802[0].message.contains("Bool"));
        assert!(t802[0].message.contains("Int"));
    }

    #[test]
    fn persist_bool_param_into_bool_column_passes() {
        let cs = columns_full(&[
            ("id", StoreColumnType::Uuid, true, true, "", false),
            ("active", StoreColumnType::Bool, true, false, "", false),
        ]);
        let errs = check_persist_fields(
            &[
                ("id".into(), "${id}".into()),
                ("active".into(), "${active}".into()),
            ],
            &cs,
            &params(&[("id", "String"), ("active", "Bool")]),
            (1, 1),
        );
        assert!(errs.is_empty());
    }

    #[test]
    fn t802_persist_null_into_not_null_column_is_rejected() {
        let cs = columns_full(&[
            ("id", StoreColumnType::Uuid, true, true, "", false),
            ("name", StoreColumnType::Text, true, false, "", false),
        ]);
        let errs = check_persist_fields(
            &[
                ("id".into(), "${id}".into()),
                ("name".into(), "null".into()),
            ],
            &cs,
            &params(&[("id", "String")]),
            (1, 1),
        );
        let t802: Vec<&ProofError> = errs
            .iter()
            .filter(|e| e.code == ProofErrorCode::T802TypeMismatch)
            .collect();
        assert_eq!(t802.len(), 1);
        assert!(t802[0].message.contains("NULL"));
        assert!(t802[0].message.contains("NOT-NULL"));
    }

    #[test]
    fn persist_null_into_nullable_column_passes() {
        let cs = columns_full(&[
            ("id", StoreColumnType::Uuid, true, true, "", false),
            ("notes", StoreColumnType::Text, false, false, "", false),
        ]);
        let errs = check_persist_fields(
            &[
                ("id".into(), "${id}".into()),
                ("notes".into(), "null".into()),
            ],
            &cs,
            &params(&[("id", "String")]),
            (1, 1),
        );
        assert!(errs.is_empty());
    }

    // ── check_persist_fields — happy path + D5 absolute ──────────────

    #[test]
    fn well_formed_persist_passes_with_zero_errors() {
        let cs = columns_full(&[
            ("tenant_id", StoreColumnType::Uuid, true, true, "", false),
            ("tier", StoreColumnType::Text, true, false, "", false),
            ("active", StoreColumnType::Bool, false, false, "", false),
        ]);
        let errs = check_persist_fields(
            &[
                ("tenant_id".into(), "${tid}".into()),
                ("tier".into(), "'standard'".into()),
            ],
            &cs,
            &params(&[("tid", "String")]),
            (1, 1),
        );
        assert!(errs.is_empty(), "got {errs:?}");
    }

    #[test]
    fn d5_blockless_persist_is_skipped() {
        // Empty field-list = the v1.30.0 blockless `persist <store>`
        // form (runtime writes user bindings). 38.e MUST skip it
        // — D5 absolute backwards-compat.
        let cs = columns_full(&[
            ("a", StoreColumnType::Text, true, false, "", false),
            ("b", StoreColumnType::Text, true, false, "", false),
        ]);
        let errs = check_persist_fields(&[], &cs, &params(&[]), (1, 1));
        assert!(errs.is_empty());
    }

    // ── check_mutate_fields — T803 does NOT apply ────────────────────

    #[test]
    fn mutate_does_not_emit_t803_for_omitted_not_null_columns() {
        // UPDATE preserves existing row values; omitted NOT-NULL
        // columns stay populated. T803 doesn't apply.
        let cs = columns_full(&[
            ("id", StoreColumnType::Uuid, true, true, "", false),
            ("tier", StoreColumnType::Text, true, false, "", false),
            ("active", StoreColumnType::Bool, true, false, "", false),
        ]);
        let errs = check_mutate_fields(
            &[("tier".into(), "'premium'".into())],
            &cs,
            &params(&[]),
            (1, 1),
        );
        let t803: Vec<&ProofError> = errs
            .iter()
            .filter(|e| e.code == ProofErrorCode::T803NotNullOmitted)
            .collect();
        assert!(t803.is_empty(), "mutate must not emit T803: {errs:?}");
    }

    #[test]
    fn t804_mutate_field_typo_with_levenshtein() {
        let cs = columns_full(&[
            ("tenant_id", StoreColumnType::Uuid, true, true, "", false),
            ("tier", StoreColumnType::Text, true, false, "", false),
        ]);
        let errs = check_mutate_fields(
            &[("teir".into(), "'premium'".into())],
            &cs,
            &params(&[]),
            (1, 1),
        );
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, ProofErrorCode::T804UnknownField);
        assert!(errs[0].message.contains("teir"));
        assert!(errs[0].message.contains("tier"));
    }

    #[test]
    fn t802_mutate_value_type_mismatch() {
        let cs = columns_full(&[
            ("id", StoreColumnType::Uuid, true, true, "", false),
            ("count", StoreColumnType::Int, false, false, "", false),
        ]);
        let errs = check_mutate_fields(
            &[("count".into(), "'not_an_int'".into())],
            &cs,
            &params(&[]),
            (1, 1),
        );
        let t802: Vec<&ProofError> = errs
            .iter()
            .filter(|e| e.code == ProofErrorCode::T802TypeMismatch)
            .collect();
        assert_eq!(t802.len(), 1);
    }

    #[test]
    fn well_formed_mutate_set_block_passes() {
        let cs = columns_full(&[
            ("id", StoreColumnType::Uuid, true, true, "", false),
            ("tier", StoreColumnType::Text, true, false, "", false),
        ]);
        let errs = check_mutate_fields(
            &[("tier".into(), "${new_tier}".into())],
            &cs,
            &params(&[("new_tier", "String")]),
            (1, 1),
        );
        assert!(errs.is_empty());
    }

    #[test]
    fn d5_blockless_mutate_is_skipped() {
        let cs = columns_full(&[("a", StoreColumnType::Text, true, false, "", false)]);
        let errs = check_mutate_fields(&[], &cs, &params(&[]), (1, 1));
        assert!(errs.is_empty());
    }

    // ── 15-type catalog smoke for persist ────────────────────────────

    #[test]
    fn persist_string_param_accepted_against_every_catalog_type() {
        // Mirror of the v1.31.0 catalog smoke — at the field-block side
        // a String param maps to every column type via the D4
        // fallback.
        let fp = params(&[("p", "String")]);
        for &t in StoreColumnType::ALL {
            let cs = columns_full(&[("col", t, false, false, "", false)]);
            let errs = check_persist_fields(
                &[("col".into(), "${p}".into())],
                &cs,
                &fp,
                (1, 1),
            );
            assert!(
                errs.is_empty(),
                "persist String → {} rejected: {errs:?}",
                t.canonical_name()
            );
        }
    }

    // ════════════════════════════════════════════════════════════════
    // v1.31.0 — composite Levenshtein suggestion shape
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn format_column_list_renders_name_colon_type_alphabetically() {
        // v1.31.0 — adopters reading a T801/T804 error see the
        // declared schema with EACH column's type, not bare names.
        let cs = columns_for(&[
            ("tier", StoreColumnType::Text, false),
            ("tenant_id", StoreColumnType::Uuid, true),
            ("created_at", StoreColumnType::Timestamptz, false),
        ]);
        let rendered = format_column_list(&cs);
        // BTreeMap iteration is alphabetic — deterministic.
        assert_eq!(
            rendered,
            "created_at: Timestamptz, tenant_id: Uuid, tier: Text"
        );
    }

    #[test]
    fn format_column_list_for_empty_schema_is_empty_string() {
        let cs = ColumnSet::default();
        assert_eq!(format_column_list(&cs), "");
    }

    #[test]
    fn suggest_columns_composite_single_match_includes_type() {
        let cs = columns_for(&[
            ("tenant_id", StoreColumnType::Uuid, true),
            ("tier", StoreColumnType::Text, false),
        ]);
        let hint = suggest_columns_composite("tenantid", &cs);
        assert_eq!(hint, "Did you mean column `tenant_id` (Uuid)?");
    }

    #[test]
    fn suggest_columns_composite_two_matches_uses_or_separator() {
        // Two equidistant candidates → `column \`a\` (T1) or \`b\` (T2)`.
        let cs = columns_for(&[
            ("tier", StoreColumnType::Text, false),
            ("tear", StoreColumnType::Int, false), // edit-dist 1 from `ter`
        ]);
        let hint = suggest_columns_composite("ter", &cs);
        // Both `tier` and `tear` are within distance 2 — accept either
        // ordering of the candidates (smart_suggest sorts by
        // (distance, name) — so `tear` comes before `tier` alphabetically).
        assert!(hint.starts_with("Did you mean column "));
        assert!(hint.contains("`tear` (Int)"));
        assert!(hint.contains("`tier` (Text)"));
        assert!(hint.contains(" or "));
    }

    #[test]
    fn suggest_columns_composite_three_matches_uses_oxford_comma() {
        let cs = columns_for(&[
            ("ax", StoreColumnType::Int, false),
            ("bx", StoreColumnType::Int, false),
            ("cx", StoreColumnType::Int, false),
        ]);
        let hint = suggest_columns_composite("x", &cs);
        // 3 equidistant candidates → "column `ax` (Int), `bx` (Int), or `cx` (Int)?"
        assert!(hint.starts_with("Did you mean column "));
        assert!(hint.contains("`ax` (Int)"));
        assert!(hint.contains("`bx` (Int)"));
        assert!(hint.contains("`cx` (Int)"));
        // Oxford comma style — the last candidate is preceded by ", or".
        assert!(hint.contains(", or `"));
    }

    #[test]
    fn suggest_columns_composite_returns_empty_when_no_candidate_in_distance() {
        let cs = columns_for(&[("id", StoreColumnType::Uuid, true)]);
        assert_eq!(suggest_columns_composite("WildlyDifferent", &cs), "");
    }

    #[test]
    fn suggest_columns_composite_caps_at_three_results() {
        // smart_suggest::MAX_RESULTS = 3 — even with 5 close
        // candidates, only the 3 closest are surfaced.
        let cs = columns_for(&[
            ("ax", StoreColumnType::Int, false),
            ("bx", StoreColumnType::Int, false),
            ("cx", StoreColumnType::Int, false),
            ("dx", StoreColumnType::Int, false),
            ("ex", StoreColumnType::Int, false),
        ]);
        let hint = suggest_columns_composite("x", &cs);
        // Count backtick-wrapped column names: each match contributes
        // exactly one backtick-pair.
        let matches = hint.matches("` (Int)").count();
        assert_eq!(matches, smart_suggest::MAX_RESULTS, "got: {hint}");
    }

    // ── v1.31.0 — T801 composite messages in flight ───────────────

    #[test]
    fn t801_message_renders_columns_with_types_and_composite_suggestion() {
        let cs = columns_for(&[
            ("tenant_id", StoreColumnType::Uuid, true),
            ("tier", StoreColumnType::Text, true),
        ]);
        let errs = check_filter("tenantid = 'x'", &cs, &params(&[]), (1, 1));
        assert_eq!(errs.len(), 1);
        let msg = &errs[0].message;
        assert_eq!(errs[0].code, ProofErrorCode::T801UnknownColumn);
        // Composite suggestion includes the column type.
        assert!(
            msg.contains("Did you mean column `tenant_id` (Uuid)?"),
            "T801 message must carry the composite suggestion, got: {msg}"
        );
        // The declared-columns list itself shows `name: Type`.
        assert!(
            msg.contains("tenant_id: Uuid"),
            "T801 message must list columns with types, got: {msg}"
        );
        assert!(msg.contains("tier: Text"));
    }

    #[test]
    fn t804_message_renders_columns_with_types_and_composite_suggestion() {
        let cs = columns_full(&[
            ("tenant_id", StoreColumnType::Uuid, true, true, "", false),
            ("tier", StoreColumnType::Text, true, false, "", false),
        ]);
        let errs = check_persist_fields(
            &[
                ("tenant_id".into(), "${id}".into()),
                ("tier".into(), "'std'".into()),
                ("tenantid".into(), "${id}".into()), // typo
            ],
            &cs,
            &params(&[("id", "String")]),
            (1, 1),
        );
        let t804 = errs
            .iter()
            .find(|e| e.code == ProofErrorCode::T804UnknownField)
            .expect("expected T804");
        assert!(
            t804.message.contains("Did you mean column `tenant_id` (Uuid)?"),
            "T804 message must carry the composite suggestion, got: {}",
            t804.message
        );
        assert!(t804.message.contains("tier: Text"));
    }

    // ── v1.31.0 — T802 compatible-columns suggestion ──────────────

    #[test]
    fn t802_literal_mismatch_surfaces_a_compatible_columns_hint() {
        // Adopter writes `tier = 42` against `tier: Text`. The schema
        // has another column (`count: Int`) that WOULD accept an Int
        // literal — the T802 message surfaces it as a tactical hint.
        let cs = columns_for(&[
            ("tier", StoreColumnType::Text, true),
            ("count", StoreColumnType::Int, false),
        ]);
        let errs = check_filter("tier = 42", &cs, &params(&[]), (1, 1));
        assert_eq!(errs.len(), 1);
        let msg = &errs[0].message;
        assert_eq!(errs[0].code, ProofErrorCode::T802TypeMismatch);
        assert!(
            msg.contains("Compatible") && msg.contains("`count` (Int)"),
            "T802 message must surface compatible columns, got: {msg}"
        );
    }

    #[test]
    fn t802_compatible_columns_omits_the_failing_column_itself() {
        // The failing column is excluded from the compatible-columns
        // suggestion (we don't suggest the column that just failed).
        let cs = columns_for(&[("tier", StoreColumnType::Text, true)]);
        let errs = check_filter("tier = 42", &cs, &params(&[]), (1, 1));
        let msg = &errs[0].message;
        // T802 fires (Int → Text rejected) but NO compatible-columns
        // hint because the only column is the one that failed.
        assert!(
            !msg.contains("Compatible"),
            "no compatible-column hint when no alternative exists: {msg}"
        );
    }

    #[test]
    fn t802_bound_param_mismatch_surfaces_compatible_columns_for_the_param_type() {
        // `${count}: Bool` → `id: Uuid` rejects; the schema has
        // `active: Bool` that WOULD accept a Bool param.
        let cs = columns_for(&[
            ("id", StoreColumnType::Uuid, true),
            ("active", StoreColumnType::Bool, false),
        ]);
        let fp = params(&[("count", "Bool")]);
        let errs = check_filter("id = ${count}", &cs, &fp, (1, 1));
        assert_eq!(errs.len(), 1);
        let msg = &errs[0].message;
        assert!(
            msg.contains("Compatible") && msg.contains("`active` (Bool)"),
            "T802 bound-param mismatch must surface a Bool-compatible \
             column hint, got: {msg}"
        );
    }

    #[test]
    fn t802_field_block_literal_mismatch_surfaces_compatible_columns() {
        // Persist field block: `name: 42` against `name: Text`,
        // schema also has `count: Int` → hint surfaces.
        let cs = columns_full(&[
            ("id", StoreColumnType::Uuid, true, true, "", false),
            ("name", StoreColumnType::Text, false, false, "", false),
            ("count", StoreColumnType::Int, false, false, "", false),
        ]);
        let errs = check_persist_fields(
            &[
                ("id".into(), "${id}".into()),
                ("name".into(), "42".into()),
            ],
            &cs,
            &params(&[("id", "String")]),
            (1, 1),
        );
        let t802 = errs
            .iter()
            .find(|e| e.code == ProofErrorCode::T802TypeMismatch)
            .expect("expected field-block T802");
        assert!(
            t802.message.contains("Compatible") && t802.message.contains("`count` (Int)"),
            "field-block T802 must surface compatible columns: {}",
            t802.message
        );
    }

    #[test]
    fn t802_compatible_columns_caps_at_max_compat_suggestions() {
        // With 5 Int columns in the schema and an Int literal in a
        // Text column, only the first MAX_COMPAT_SUGGESTIONS surface.
        let cs = columns_for(&[
            ("a", StoreColumnType::Int, false),
            ("b", StoreColumnType::Int, false),
            ("c", StoreColumnType::Int, false),
            ("d", StoreColumnType::Int, false),
            ("e", StoreColumnType::Int, false),
            ("tier", StoreColumnType::Text, true),
        ]);
        let errs = check_filter("tier = 42", &cs, &params(&[]), (1, 1));
        let msg = &errs[0].message;
        // Count backtick-quoted column references in the "Compatible"
        // tail — exactly MAX_COMPAT_SUGGESTIONS hits.
        let after_compat = msg.split("Compatible").nth(1).unwrap_or("");
        let hits = after_compat.matches("` (Int)").count();
        assert_eq!(
            hits, MAX_COMPAT_SUGGESTIONS,
            "expected exactly {} compatible-column hits, got: {msg}",
            MAX_COMPAT_SUGGESTIONS
        );
    }

    #[test]
    fn t802_like_against_non_text_does_not_surface_a_misleading_compat_hint() {
        // LIKE against Uuid surfaces T802 (LIKE requires Text-class).
        // 38.g does NOT add a compatible-columns hint for the LIKE
        // case (the existing LIKE-specific message is the right
        // diagnostic — composite suggestions are for value-type
        // mismatches, not operator-class mismatches).
        let cs = columns_for(&[
            ("id", StoreColumnType::Uuid, true),
            ("name", StoreColumnType::Text, false),
        ]);
        let errs = check_filter("id LIKE 'abc%'", &cs, &params(&[]), (1, 1));
        let t802_msg = errs
            .iter()
            .find(|e| e.code == ProofErrorCode::T802TypeMismatch)
            .map(|e| e.message.as_str())
            .unwrap_or("");
        assert!(t802_msg.contains("LIKE"), "LIKE message missing: {t802_msg}");
        // The LIKE branch goes through a separate code path that
        // doesn't apply the compatible-columns helper — that's
        // intentional (LIKE wants a Text column, not a String value).
    }

    // ─── v2.21.0 — compile-time `now() ± interval` validation ───────
    //
    // The proof scanner mirrors the v2.21.0 runtime time grammar. A VALID
    // time form against a temporal column proves clean; a MALFORMED time
    // form is the hard `axon-T806` compile error that moves the brief #34
    // failure (daemon `retrieve` silently 0 rows) from the run to
    // `axon check`. The cross-crate parity test
    // (`axon-rs/tests/where_time_parity.rs`) pins this scanner
    // to the runtime `parse_filter` so the two cannot drift.

    #[test]
    fn time_bare_now_against_a_timestamp_column_proves_clean() {
        let cs = columns_for(&[("last_activity_at", StoreColumnType::Timestamptz, false)]);
        let errs = check_filter("last_activity_at < now()", &cs, &params(&[]), (1, 1));
        assert!(errs.is_empty(), "bare now() must prove clean, got {errs:?}");
    }

    #[test]
    fn time_now_minus_interval_against_a_timestamp_column_proves_clean() {
        for col in [
            StoreColumnType::Timestamptz,
            StoreColumnType::Timestamp,
            StoreColumnType::Date,
            StoreColumnType::Time,
        ] {
            let cs = columns_for(&[("t", col, false)]);
            let errs = check_filter("t < now() - interval '30 minutes'", &cs, &params(&[]), (1, 1));
            assert!(errs.is_empty(), "{col:?}: {errs:?}");
        }
    }

    #[test]
    fn time_value_covers_every_sign_and_unit() {
        let cs = columns_for(&[("t", StoreColumnType::Timestamptz, false)]);
        for expr in [
            "t > now() + interval '7 days'",
            "t <= now() - interval '1 hour'",
            "t >= now() - interval '2 weeks'",
            "t != now()",
            "t < now() - interval '90 seconds'",
            "t < now() - interval '1 month'",
            "t < now() - interval '3 years'",
        ] {
            let errs = check_filter(expr, &cs, &params(&[]), (1, 1));
            assert!(errs.is_empty(), "`{expr}` should prove clean, got {errs:?}");
        }
    }

    #[test]
    fn the_canonical_sweep_predicate_proves_clean() {
        // brief #34's autonomous-sweep predicate: a status literal (a $N
        // bind at runtime) AND a structural time comparison.
        let cs = columns_for(&[
            ("status", StoreColumnType::Text, false),
            ("last_activity_at", StoreColumnType::Timestamptz, false),
        ]);
        let errs = check_filter(
            "status == 'ACTIVE' AND last_activity_at < now() - interval '30 minutes'",
            &cs,
            &params(&[]),
            (1, 1),
        );
        assert!(errs.is_empty(), "the canonical sweep must prove clean, got {errs:?}");
    }

    #[test]
    fn malformed_time_forms_are_hard_t806_errors() {
        // Every malformed time form is now a compile error — not a silent
        // pass that fails at the daemon run. Mirrors the runtime
        // `parse_filter` rejection corpus (v2.21.0).
        let cs = columns_for(&[("t", StoreColumnType::Timestamptz, false)]);
        for bad in [
            "t < now( - interval '5 minutes'",      // missing `)`
            "t < now() - interval '30'",            // no unit
            "t < now() - interval '30 fortnights'", // unknown unit
            "t < now() - interval 'abc minutes'",   // non-integer amount
            "t < now() - interval '-5 minutes'",    // negative amount
            "t < now() - 'interval string'",        // no `interval` keyword
        ] {
            let errs = check_filter(bad, &cs, &params(&[]), (1, 1));
            assert_eq!(errs.len(), 1, "`{bad}` should be one T806, got {errs:?}");
            assert_eq!(errs[0].code, ProofErrorCode::T806BadTimeValue, "`{bad}`");
        }
    }

    #[test]
    fn like_against_a_time_value_is_a_t806() {
        let cs = columns_for(&[("t", StoreColumnType::Timestamptz, false)]);
        let errs = check_filter("t LIKE now()", &cs, &params(&[]), (1, 1));
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, ProofErrorCode::T806BadTimeValue);
        assert!(errs[0].message.contains("LIKE"));
    }

    #[test]
    fn time_value_against_a_non_temporal_column_is_a_t806() {
        // `now()` against a Text column would fail at runtime with a
        // Postgres `operator does not exist` — caught at compile now.
        let cs = columns_for(&[("name", StoreColumnType::Text, false)]);
        let errs = check_filter("name < now()", &cs, &params(&[]), (1, 1));
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, ProofErrorCode::T806BadTimeValue);
        assert!(
            errs[0].message.contains("temporal"),
            "message should name the temporal requirement: {}",
            errs[0].message
        );
    }

    #[test]
    fn time_value_against_an_unknown_column_still_reports_t801() {
        // The time form does not shadow column-existence: a typo'd time
        // column surfaces the usual T801 (the proof recognised the value
        // as a time form, so the column check still runs).
        let cs = columns_for(&[("last_activity_at", StoreColumnType::Timestamptz, false)]);
        let errs = check_filter("last_activty_at < now()", &cs, &params(&[]), (1, 1));
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, ProofErrorCode::T801UnknownColumn);
    }

    #[test]
    fn scan_where_recognises_the_time_form_structurally() {
        let p = scan_where("t < now() - interval '30 minutes'").unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].column, "t");
        assert_eq!(p[0].op, WhereOp::Lt);
        assert_eq!(
            p[0].value,
            WhereValue::TimeNow {
                offset: Some((TimeSign::Minus, 30, TimeUnit::Minute))
            }
        );
        // bare now()
        let p = scan_where("t >= now()").unwrap();
        assert_eq!(p[0].value, WhereValue::TimeNow { offset: None });
    }

    // ─── v2.21.0 — bounded + ordered retrieve (`order_by:` / `limit:`) ──
    //
    // `check_bounds` is the compile-time mirror of the runtime
    // `filter::render_bounds`. A valid clause proves clean; a malformed
    // one is a hard `axon-T807` (order_by) / `axon-T808` (limit). Pinned
    // to the runtime by `axon-rs/tests/bounds_parity.rs`.

    fn no_params() -> FlowParamTypes {
        params(&[])
    }

    #[test]
    fn valid_order_by_and_limit_prove_clean() {
        let cs = columns_for(&[
            ("last_activity_at", StoreColumnType::Timestamptz, false),
            ("id", StoreColumnType::Uuid, true),
        ]);
        for (ob, lim) in [
            ("last_activity_at asc", "100"),
            ("last_activity_at desc, id asc", "1"),
            ("id", ""),     // direction defaults to asc; no limit
            ("", "250"),    // limit only
            ("id DESC", "0"), // case-insensitive direction; zero limit ok
        ] {
            let errs = check_bounds(ob, lim, Some(&cs), &no_params(), (1, 1));
            assert!(errs.is_empty(), "`{ob}` / `{lim}` should be clean, got {errs:?}");
        }
    }

    #[test]
    fn order_by_unknown_column_is_t807() {
        let cs = columns_for(&[("last_activity_at", StoreColumnType::Timestamptz, false)]);
        let errs = check_bounds("last_activty_at asc", "", Some(&cs), &no_params(), (1, 1));
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, ProofErrorCode::T807BadOrderBy);
        assert!(errs[0].message.contains("unknown column"));
    }

    #[test]
    fn order_by_bad_direction_is_t807() {
        let cs = columns_for(&[("id", StoreColumnType::Uuid, true)]);
        let errs = check_bounds("id ascending", "", Some(&cs), &no_params(), (1, 1));
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, ProofErrorCode::T807BadOrderBy);
        assert!(errs[0].message.contains("asc"));
    }

    #[test]
    fn order_by_structural_checks_run_without_a_schema() {
        // No ColumnSet (manifest/none store): existence isn't proven, but
        // the direction shape still is.
        let errs = check_bounds("id sideways", "", None, &no_params(), (1, 1));
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, ProofErrorCode::T807BadOrderBy);
    }

    #[test]
    fn limit_non_integer_literal_is_t808() {
        for bad in ["abc", "-5", "3.5", "99999999999999999999"] {
            let errs = check_bounds("", bad, None, &no_params(), (1, 1));
            assert_eq!(errs.len(), 1, "`{bad}` should be one T808, got {errs:?}");
            assert_eq!(errs[0].code, ProofErrorCode::T808BadLimit, "`{bad}`");
        }
    }

    #[test]
    fn limit_integer_param_proves_clean_but_non_integer_param_is_t808() {
        let fp = params(&[("max", "Int"), ("label", "String")]);
        // Int param — clean.
        assert!(check_bounds("", "${max}", None, &fp, (1, 1)).is_empty());
        // String param — T808.
        let errs = check_bounds("", "${label}", None, &fp, (1, 1));
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, ProofErrorCode::T808BadLimit);
        // Undeclared param — silent (the v1.32.0 D2 binding-totality concern).
        assert!(check_bounds("", "${unknown}", None, &fp, (1, 1)).is_empty());
    }

    #[test]
    fn order_by_and_limit_errors_are_independent() {
        let cs = columns_for(&[("id", StoreColumnType::Uuid, true)]);
        let errs = check_bounds("missing desc", "nope", Some(&cs), &no_params(), (1, 1));
        // one T807 (unknown column) + one T808 (bad literal).
        assert_eq!(errs.len(), 2);
        assert!(errs.iter().any(|e| e.code == ProofErrorCode::T807BadOrderBy));
        assert!(errs.iter().any(|e| e.code == ProofErrorCode::T808BadLimit));
    }

    // ── v2.33.0 — check_aggregate (axon-T843/T844/T845) ────────────

    #[test]
    fn aggregate_happy_paths_yield_no_errors() {
        let cs = columns_for(&[
            ("tokens", StoreColumnType::Int, false),
            ("industry", StoreColumnType::Text, false),
        ]);
        // count, no schema needed.
        assert!(check_aggregate("count", "", "", "", None, (1, 1)).is_empty());
        // sum over a numeric column, grouped by an existing column.
        assert!(
            check_aggregate("sum(tokens)", "industry", "", "", Some(&cs), (1, 1)).is_empty()
        );
        // empty aggregate + empty group_by = no proof to run.
        assert!(check_aggregate("", "", "id asc", "10", Some(&cs), (1, 1)).is_empty());
    }

    #[test]
    fn t843_malformed_aggregate_shapes() {
        for bad in ["median(x)", "sum", "sum()", "count(id)", "sum(1id)", "sum(x"] {
            let errs = check_aggregate(bad, "", "", "", None, (1, 1));
            assert!(
                errs.iter().any(|e| e.code == ProofErrorCode::T843BadAggregate),
                "`{bad}` must surface axon-T843; got {errs:?}"
            );
        }
        // Bad group terms are T843 too (grammar level).
        let errs = check_aggregate("count", "a,,b", "", "", None, (1, 1));
        assert!(errs.iter().any(|e| e.code == ProofErrorCode::T843BadAggregate));
    }

    #[test]
    fn t844_schema_backed_column_proofs() {
        let cs = columns_for(&[
            ("tokens", StoreColumnType::Int, false),
            ("name", StoreColumnType::Text, false),
        ]);
        // Unknown aggregate column.
        let errs = check_aggregate("sum(tokns)", "", "", "", Some(&cs), (1, 1));
        assert!(
            errs.iter().any(|e| e.code == ProofErrorCode::T844AggregateColumn
                && e.message.contains("tokns")),
            "unknown column must surface axon-T844; got {errs:?}"
        );
        // sum over a Text column.
        let errs = check_aggregate("sum(name)", "", "", "", Some(&cs), (1, 1));
        assert!(
            errs.iter().any(|e| e.code == ProofErrorCode::T844AggregateColumn),
            "sum over Text must surface axon-T844; got {errs:?}"
        );
        // min/max over Text is fine (orderable).
        assert!(check_aggregate("max(name)", "", "", "", Some(&cs), (1, 1)).is_empty());
        // Unknown group column.
        let errs = check_aggregate("count", "industri", "", "", Some(&cs), (1, 1));
        assert!(errs.iter().any(|e| e.code == ProofErrorCode::T844AggregateColumn));
        // Schema-less: column proofs silently skipped (grammar still runs).
        assert!(check_aggregate("sum(anything)", "", "", "", None, (1, 1)).is_empty());
    }

    #[test]
    fn t845_structural_combinations() {
        // group_by without aggregate.
        let errs = check_aggregate("", "industry", "", "", None, (1, 1));
        assert!(errs.iter().any(|e| e.code == ProofErrorCode::T845AggregateCombo));
        // aggregate with order_by / limit.
        let errs = check_aggregate("count", "", "id asc", "", None, (1, 1));
        assert!(errs.iter().any(|e| e.code == ProofErrorCode::T845AggregateCombo));
        let errs = check_aggregate("count", "", "", "10", None, (1, 1));
        assert!(errs.iter().any(|e| e.code == ProofErrorCode::T845AggregateCombo));
        // aggregate column as group key.
        let errs = check_aggregate("sum(tokens)", "tokens", "", "", None, (1, 1));
        assert!(errs.iter().any(|e| e.code == ProofErrorCode::T845AggregateCombo));
    }

    #[test]
    fn t843_t845_slugs_are_pinned() {
        assert_eq!(ProofErrorCode::T843BadAggregate.slug(), "axon-T843");
        assert_eq!(ProofErrorCode::T844AggregateColumn.slug(), "axon-T844");
        assert_eq!(ProofErrorCode::T845AggregateCombo.slug(), "axon-T845");
    }
}
