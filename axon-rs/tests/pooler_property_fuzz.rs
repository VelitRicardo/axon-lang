//! v2.81.0 — exercises the Postgres schema-resolution helpers, behind the
//! `postgres` feature.
#![cfg(feature = "postgres")]

//! v1.32.0 — Property / fuzz pack for the Pooler-Coherent Store
//! Contract.
//!
//! Counterpart to 37.x.i's `pgbouncer_integration.rs`: where
//! the integration suite proves the contract holds against a REAL
//! transaction-mode pooler, this pack proves the pure-total predicates
//! the contract is built on are *unconditionally* total and never panic
//! — independent of any database. The same deterministic-LCG discipline
//! 35.k / 32.i / 33.g uses (no `proptest` / `quickcheck` dependency;
//! every regression reproduces verbatim from the printed seed).
//!
//! Three surfaces — one per primary D-letter group:
//!
//!  - **Surface A — `resolve_from_rows` (D1).** The pure verdict core
//!    that both 37.x.b's `to_regclass` primary and the `pg_catalog`
//!    fallback feed through. Total over arbitrary schema topologies;
//!    `TableNotResolved` iff zero schemas; `AmbiguousTable` iff ≥2;
//!    schemas always sorted (deterministic diagnostic).
//!
//!  - **Surface B — `build_pg_where` (D4).** Total over the cross
//!    product of {arbitrary expr} × {known/unknown column type} ×
//!    {arbitrary bindings}. The post-37.x.e equality fallback never
//!    introduces a stray `'` / `;` / `--` (the 35.k D4 invariant
//!    extends across the type-known/type-unknown × eq/ord/LIKE
//!    matrix); placeholder count == param count; offset numbering
//!    is correct.
//!
//!  - **Surface C — D9 self-heal predicates.** `is_schema_drift_sqlstate`
//!    is total over every ASCII string + a corpus of 5-char SQLSTATE
//!    codes, returns `true` for EXACTLY the closed set
//!    {42P01, 42703, 42804, 42883} and `false` for everything else;
//!    `StoreError::is_schema_drift()` agrees with the variant.
//!
//! Total iteration budget ≈ 7 500 deterministic iterations across the
//! three surfaces, every surface pure; pack runs in well under a
//! second.

#![allow(clippy::needless_return)]

use std::collections::HashMap;

use axon::store::filter::{build_pg_where, SqlValue};
use axon::store::error::StoreError;
use axon::store::postgres_backend::{is_schema_drift_sqlstate, resolve_from_rows};

// ── section 0 — Deterministic PRNG (linear congruential, 35.k's pattern) ────

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next_u64(&mut self) -> u64 {
        // Numerical Recipes 64-bit LCG — deterministic, fast.
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    fn next_in(&mut self, lo: usize, hi: usize) -> usize {
        debug_assert!(hi >= lo);
        let span = (hi - lo + 1) as u64;
        lo + (self.next_u64() % span) as usize
    }

    fn next_bool(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

// ════════════════════════════════════════════════════════════════════
//  Surface A — `resolve_from_rows` (D1 verdict core) is total over
//  arbitrary schema topologies. Three invariants:
//
//   I.1  no input shape panics
//   I.2  the verdict is determined by the SCHEMA SET only:
//          0 schemas  → TableNotResolved
//          1 schema   → Ok((that schema, its column → udt map))
//          ≥2 schemas → AmbiguousTable (schemas sorted)
//   I.3  the diagnostic is deterministic: same input → same `Vec`
//        of schemas in the `AmbiguousTable` payload (sorted).
// ════════════════════════════════════════════════════════════════════

const SCHEMA_POOL: &[&str] = &[
    "public",
    "tenant_a",
    "tenant_b",
    "audit",
    "legacy",
    "alt",
    "x",
];

const COLUMN_POOL: &[&str] =
    &["id", "tenant_id", "name", "tier", "payload", "created_at"];

const UDT_POOL: &[&str] = &[
    "int4", "uuid", "text", "varchar", "timestamptz", "jsonb", "numeric",
    "bool", "bytea",
];

fn random_triples(rng: &mut Lcg) -> Vec<(String, String, String)> {
    let n = rng.next_in(0, 12);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let s = SCHEMA_POOL[rng.next_in(0, SCHEMA_POOL.len() - 1)];
        let c = COLUMN_POOL[rng.next_in(0, COLUMN_POOL.len() - 1)];
        let t = UDT_POOL[rng.next_in(0, UDT_POOL.len() - 1)];
        out.push((s.to_string(), c.to_string(), t.to_string()));
    }
    out
}

fn distinct_schemas(triples: &[(String, String, String)]) -> Vec<String> {
    let mut set: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    for (s, _, _) in triples {
        set.insert(s.clone());
    }
    set.into_iter().collect()
}

#[test]
fn fuzz_surface_a_resolve_from_rows_is_total_and_deterministic() {
    let mut rng = Lcg::new(0x37_0a_d1_2b_7f_3e_91_a7);
    let iterations: u64 = 2_500;

    for iter in 0..iterations {
        let seed = rng.0;
        let table = if rng.next_bool() {
            // A "real" table name.
            COLUMN_POOL[rng.next_in(0, COLUMN_POOL.len() - 1)].to_string()
        } else {
            // An arbitrary token (incl. empty) — `resolve_from_rows` does
            // not validate the name; that's `check_identifier`'s job.
            format!("t_{}", rng.next_u64())
        };
        let triples = random_triples(&mut rng);
        let expected_schemas = distinct_schemas(&triples);

        // I.1 — totality: never panic.
        let verdict = resolve_from_rows(&table, triples.clone());

        match (expected_schemas.len(), &verdict) {
            (0, Err(StoreError::TableNotResolved { table: t })) => {
                assert_eq!(
                    t, &table,
                    "seed {seed}: TableNotResolved must echo the table name"
                );
            }
            (1, Ok((schema, column_types))) => {
                assert_eq!(
                    schema, &expected_schemas[0],
                    "seed {seed}: single-schema verdict must name the schema"
                );
                // Every (column, udt) for that schema appears.
                for (s, c, t) in &triples {
                    if s == schema {
                        assert_eq!(
                            column_types.get(c).map(|s| s.as_str()),
                            Some(t.as_str()),
                            "seed {seed}: column `{c}` must map to its udt"
                        );
                    }
                }
            }
            (n, Err(StoreError::AmbiguousTable { table: t, schemas })) if n >= 2 => {
                assert_eq!(t, &table, "seed {seed}: AmbiguousTable echoes table");
                assert_eq!(
                    schemas, &expected_schemas,
                    "seed {seed}: AmbiguousTable's schemas must equal the \
                     distinct-schema set, sorted (I.3 — deterministic \
                     diagnostic)"
                );
                // I.3 strengthening: schemas appear strictly sorted, no
                // duplicates.
                for window in schemas.windows(2) {
                    assert!(
                        window[0] < window[1],
                        "seed {seed}: schemas must be strictly sorted, got \
                         {schemas:?}"
                    );
                }
            }
            (n, v) => panic!(
                "seed {seed}: invariant I.2 broken — {n} distinct schemas \
                 produced verdict {v:?}"
            ),
        }

        // Determinism — same input, same verdict.
        let again = resolve_from_rows(&table, triples);
        assert_eq!(
            verdict, again,
            "seed {seed}: `resolve_from_rows` must be a pure function"
        );

        let _ = iter;
    }
}

// ════════════════════════════════════════════════════════════════════
//  Surface B — `build_pg_where` totality + D4 injection-resistance,
//  extended to the 37.x.e equality fallback. Invariants (across the
//  cross product of {expr} × {known/unknown column_types} × {bindings}
//  × {offset}):
//
//   J.1  never panic
//   J.2  on Ok, the clause is injection-safe (no `'`, `;`, `--`)
//   J.3  on Ok, placeholder count == params count
//   J.4  on Ok, placeholders are numbered consecutively from offset+1
//   J.5  on Ok, no `Null` reaches a `$N` slot (NULL folds to IS [NOT] NULL)
//   J.6  the 37.x.e (D4) shape: when an equality op meets an UNKNOWN
//        type, the rendered fragment for that column contains a
//        `"col"::text =` cast (the column-side `::text` cast). When the
//        type is KNOWN, the cast lands on the value side (`$N::<udt>`).
//        Unknown type + ordering or LIKE: bare `$N` (fail-loud).
// ════════════════════════════════════════════════════════════════════

const COLS_FOR_FILTER: &[&str] = &["id", "tenant_id", "name", "tier", "x"];
const OPS: &[&str] = &["=", "!=", ">", "<", ">=", "<=", "LIKE", "==", "<>"];
const VALUES: &[&str] = &[
    "1",
    "-42",
    "3.14",
    "true",
    "false",
    "null",
    "'safe'",
    "'; DROP TABLE users; --'",
    "'\\' OR 1=1'",
    "${tenant_id}",
    "${_x}",
];
const BIND_PAYLOADS: &[&str] = &[
    "8b3e1c12-7a04-4f7e-9d05-1d6df2c6c2a1",
    "alpha",
    "'; DROP TABLE x; --",
    "O'Connor",
    "\"weird\"",
    "0",
    "",
];
const KNOWN_UDTS: &[&str] = &["int4", "uuid", "text", "timestamptz", "bool"];

fn structured_expr(rng: &mut Lcg) -> String {
    let conditions = rng.next_in(1, 3);
    let mut parts: Vec<String> = Vec::new();
    for i in 0..conditions {
        if i > 0 {
            parts.push(if rng.next_bool() { "AND" } else { "OR" }.into());
        }
        parts.push(COLS_FOR_FILTER[rng.next_in(0, COLS_FOR_FILTER.len() - 1)].into());
        parts.push(OPS[rng.next_in(0, OPS.len() - 1)].into());
        parts.push(VALUES[rng.next_in(0, VALUES.len() - 1)].into());
    }
    parts.join(" ")
}

fn random_bindings(rng: &mut Lcg) -> HashMap<String, String> {
    let n = rng.next_in(0, 3);
    let mut out = HashMap::new();
    for _ in 0..n {
        let k = if rng.next_bool() { "tenant_id" } else { "_x" };
        out.insert(
            k.to_string(),
            BIND_PAYLOADS[rng.next_in(0, BIND_PAYLOADS.len() - 1)].to_string(),
        );
    }
    out
}

fn random_column_types(rng: &mut Lcg) -> HashMap<String, String> {
    // Half the iterations: empty (unknown type) — 37.x.e's hot path.
    if rng.next_bool() {
        return HashMap::new();
    }
    let mut out = HashMap::new();
    let n = rng.next_in(1, COLS_FOR_FILTER.len());
    for col in COLS_FOR_FILTER.iter().take(n) {
        out.insert(
            (*col).to_string(),
            KNOWN_UDTS[rng.next_in(0, KNOWN_UDTS.len() - 1)].to_string(),
        );
    }
    out
}

fn assert_clause_injection_safe(clause: &str, params: &[SqlValue], seed: u64) {
    assert!(
        !clause.contains('\''),
        "seed {seed}: single quote leaked into `{clause}` — a value \
         reached SQL text unparameterized"
    );
    assert!(
        !clause.contains(';'),
        "seed {seed}: `;` leaked into clause `{clause}`"
    );
    assert!(
        !clause.contains("--"),
        "seed {seed}: `--` comment leaked into clause `{clause}`"
    );
    assert_eq!(
        clause.matches('$').count(),
        params.len(),
        "seed {seed}: `{clause}` placeholder count must equal param count"
    );
    assert!(
        !params.iter().any(|v| matches!(v, SqlValue::Null)),
        "seed {seed}: NULL must never occupy a bind slot"
    );
}

#[test]
fn fuzz_surface_b_build_pg_where_totality_and_injection_resistance() {
    let mut rng = Lcg::new(0x37_0e_d4_3c_8a_4f_a2_b8);
    let iterations: u64 = 4_000;

    for iter in 0..iterations {
        let seed = rng.0;
        let expr = structured_expr(&mut rng);
        let offset = rng.next_in(0, 32);
        let bindings = random_bindings(&mut rng);
        let column_types = random_column_types(&mut rng);

        // J.1 — totality.
        let outcome =
            build_pg_where(&expr, offset, &bindings, &column_types);

        match outcome {
            Ok((clause, params)) => {
                // J.2–J.5 — the 35.k D4 invariant, EXTENDED across the
                // unknown-type / known-type matrix.
                assert_clause_injection_safe(&clause, &params, seed);
                // J.4 — placeholders sequential from offset+1.
                for (i, _) in params.iter().enumerate() {
                    let p = format!("${}", offset + 1 + i);
                    assert!(
                        clause.contains(&p),
                        "seed {seed}: clause `{clause}` missing \
                         placeholder `{p}`"
                    );
                }
                // J.6 — the 37.x.e shape: an UNKNOWN-type equality
                // emits `::text` SOMEWHERE in the clause (the column-
                // side cast); a KNOWN type emits a `$N::<udt>` value-
                // side cast. We can't trivially per-token inspect the
                // clause (the parser may have folded operators), but
                // we CAN check the cross-cutting weakest property: if
                // the column types are empty AND the rendered clause
                // contains `=` or `<>`/`!=`, then it MUST also
                // contain `::text` — because every equality in the
                // expr should have triggered the fallback. (The
                // expression may have failed compilation entirely, in
                // which case we landed on Err and the property is
                // vacuous.)
                if column_types.is_empty()
                    && (clause.contains(" = ") || clause.contains(" <> "))
                {
                    assert!(
                        clause.contains("::text"),
                        "seed {seed}: unknown-type equality clause \
                         `{clause}` must carry the 37.x.e `::text` \
                         column-side cast"
                    );
                }
            }
            Err(_) => {
                // A typed error is a valid total outcome — `parse_filter`
                // rejects ill-formed input.
            }
        }
        let _ = iter;
    }
}

// ════════════════════════════════════════════════════════════════════
//  Surface C — D9 self-heal predicates totality.
//
//   K.1  `is_schema_drift_sqlstate` is total over every input length 0..6
//        AND ASCII-printable input — never panics
//   K.2  `is_schema_drift_sqlstate` returns TRUE on exactly the closed
//        set {42P01, 42703, 42804, 42883}; FALSE on every other input
//        (incl. case-shifted variants, prefix matches, etc.)
//   K.3  `StoreError::is_schema_drift()` agrees with the
//        `StoreError::SchemaDrift { .. }` discriminant — TRUE on that
//        variant only, FALSE on every other variant
// ════════════════════════════════════════════════════════════════════

const DRIFT_CODES: &[&str] = &["42P01", "42703", "42804", "42883"];

#[test]
fn fuzz_surface_c_d9_is_schema_drift_sqlstate_closed_set() {
    let mut rng = Lcg::new(0x37_0f_d9_4d_9b_5a_b3_c9);
    let iterations: u64 = 1_000;

    // K.2(a) — every drift code returns TRUE.
    for code in DRIFT_CODES {
        assert!(
            is_schema_drift_sqlstate(code),
            "the ratified drift code `{code}` MUST classify as drift"
        );
    }

    // K.2(b) — random ASCII strings: TRUE iff in the closed set.
    let alphabet: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ_-+/.";
    for iter in 0..iterations {
        let seed = rng.0;
        let len = rng.next_in(0, 6);
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            s.push(alphabet[rng.next_in(0, alphabet.len() - 1)] as char);
        }
        // K.1 — never panics; K.2 — closed-set membership.
        let verdict = is_schema_drift_sqlstate(&s);
        let expected = DRIFT_CODES.contains(&s.as_str());
        assert_eq!(
            verdict, expected,
            "seed {seed}: `{s}` — verdict {verdict}, expected {expected} \
             (the closed set is {DRIFT_CODES:?})"
        );
        let _ = iter;
    }

    // K.2(c) — case-shifted variants are NOT drift codes (the SQLSTATE
    // alphabet is upper-case only; `42p01` is a syntactically distinct
    // string the planner never emits).
    for code in DRIFT_CODES {
        let lowered = code.to_ascii_lowercase();
        if &lowered != code {
            assert!(
                !is_schema_drift_sqlstate(&lowered),
                "case-shifted `{lowered}` must NOT classify as drift"
            );
        }
    }

    // K.2(d) — every neighbour SQLSTATE class is NOT drift (sanity:
    // adjacent error categories share the leading `42` "syntax error or
    // access rule violation" class, but only the four ratified codes
    // are zero-side-effect parse rejections eligible for the D9 retry).
    for near in [
        "42000", "42501", "42601", "42602", "42622", "42701", "42702",
        "42704", "42710", "42712", "42723", "42725", "42803", "42P02",
        "42P10",
    ] {
        assert!(
            !is_schema_drift_sqlstate(near),
            "neighbour code `{near}` must NOT classify as drift — only \
             the ratified four are"
        );
    }
}

#[test]
fn fuzz_surface_c_d9_store_error_is_schema_drift_agrees_with_variant() {
    // K.3 — exhaustive over every `StoreError` variant. The
    // `SchemaDrift` arm is the ONLY one that returns true. A new
    // variant added without updating `is_schema_drift()` would default
    // to `_ => false` and this enumeration would catch the miss the
    // moment the new variant joins the list.
    use axon::store::filter::FilterError;

    let drift = StoreError::SchemaDrift {
        op: "retrieve",
        sqlstate: "42883".into(),
        source: "operator does not exist: text = uuid".into(),
    };
    assert!(
        drift.is_schema_drift(),
        "K.3: SchemaDrift must classify as drift"
    );

    let non_drift_corpus = [
        StoreError::EmptyConnection,
        StoreError::EmptyEnvVarName,
        StoreError::MissingEnvVar { var: "X".into() },
        StoreError::PoolInit {
            dsn_masked: "postgresql://u:***@h/db".into(),
            source: "bad".into(),
        },
        StoreError::InvalidIdentifier {
            kind: "table",
            name: "x;".into(),
        },
        StoreError::EmptyData { op: "insert" },
        StoreError::Filter(FilterError::TooManyConditions { limit: 256 }),
        StoreError::Connect { source: "refused".into() },
        StoreError::Query {
            op: "retrieve",
            source: "syntax".into(),
        },
        StoreError::UnsupportedColumnType {
            column: "geom".into(),
            pg_type: "POINT".into(),
        },
        StoreError::Decode {
            column: "ts".into(),
            pg_type: "TIMESTAMPTZ".into(),
            source: "overflow".into(),
        },
        StoreError::TableNotResolved {
            table: "ghost".into(),
        },
        StoreError::AmbiguousTable {
            table: "dup".into(),
            schemas: vec!["a".into(), "b".into()],
        },
    ];
    for e in &non_drift_corpus {
        assert!(
            !e.is_schema_drift(),
            "K.3: non-drift variant `{e:?}` MUST NOT classify as drift"
        );
    }
}
