//! v2.81.0 — exercises the Postgres data plane, behind the `postgres`
//! feature.
#![cfg(feature = "postgres")]

//! v1.30.0 — D13 robustness fuzz pack for the `axonstore` cognitive
//! data plane.
//!
//! Total + never-panic invariant for every pure public surface the
//! v1.30.0 cycle (35.a-j) exposes. Deterministic per-seed (a linear
//! congruential generator, mirroring the v1.23.0 pattern); a
//! regression reproduces verbatim from the printed seed.
//!
//! ## Surfaces under fuzz (the D13 enumeration)
//!
//! 1. **Filter compiler** (35.b) — arbitrary `where`-expression
//!    strings (garbage + structured + injection payloads). `parse_filter`
//!    / `build_pg_where` must never panic, and — the load-bearing D4
//!    invariant — a compiled clause must NEVER contain a raw `'`, `;`
//!    or `--`: every value is a `$N` placeholder, so an injected
//!    string can never reach SQL text. Placeholder count == params
//!    count; params never carry `Null`.
//!
//! 2. **Closed-catalog rejection** — `classify_backend`,
//!    `classify_pg_type`, `resolve_on_breach`, `is_safe_identifier`,
//!    `BackpressurePolicy::from_slug` over arbitrary strings: total,
//!    never-panic, every input maps to exactly one verdict.
//!
//! 3. **Epistemic grading** (35.g) — `confidence_of_json` /
//!    `confidence_of_sql` over arbitrary values; `enforce_retrieve_floor`
//!    / `enforce_persist_floor` over arbitrary rows + floors. Total;
//!    the floor partition never loses or duplicates a row.
//!
//! 4. **Audit-chain integrity** (35.h) — under an arbitrary sequence
//!    of mutation deltas, the HMAC-Merkle chain ALWAYS verifies
//!    `Intact`, the head advances monotonically (every head distinct),
//!    and the head is key-bound (the same sequence under a different
//!    key yields a different head).
//!
//! 5. **Store resolution** (35.d) — `StoreRegistry::build` over
//!    arbitrary `IRAxonStore` spec sets is total; `check_store_capability`
//!    (35.j) over arbitrary required/held sets is total.
//!
//! 6. **SQL builders** (35.c) — `build_select_sql` / `build_insert_sql`
//!    / `build_update_sql` / `build_delete_sql` over arbitrary tables,
//!    columns, data and where-expressions: total, and the same
//! injection-resistance invariant as section 1.
//!
//! ## Determinism + budget
//!
//! Each surface runs a fixed iteration count off a hard-coded prime
//! seed. Total ≈ 18 000 iterations across 6 surfaces; every surface
//! is pure (no I/O), so the pack runs in well under a second.

#![allow(clippy::needless_return)]

use axon::ir_nodes::IRAxonStore;
use axon::store::audit_chain::{
    apply_on_breach, resolve_on_breach, ChainVerdict, OnBreachPolicy,
    StoreAuditChain, StoreMutationKind,
};
use axon::store::capability::check_store_capability;
use axon::store::epistemic::{
    confidence_of_json, confidence_of_sql, enforce_persist_floor,
    enforce_retrieve_floor, mark_retrieved,
};
use axon::store::filter::{
    build_pg_where, is_safe_identifier, parse_filter, SqlValue,
};
use axon::store::postgres_backend::{
    build_delete_sql, build_insert_sql, build_select_sql, build_update_sql,
    classify_pg_type, resolve_dsn, StoreRow,
};
use axon::store::registry::{classify_backend, StoreRegistry};
use axon::stream_effect::BackpressurePolicy;

/// v1.32.0 — empty bindings: the v1.30.0 fuzz surface compiles
/// `where` clauses with no `${name}` placeholders. The v1.32.0
/// `${name}`-resolution fuzz lives in `tests/filter_injection.rs`.
fn nb() -> std::collections::HashMap<String, String> {
    std::collections::HashMap::new()
}

// ── section 0 — Deterministic PRNG (linear congruential) ────────────────────

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

// ── Shared fuzz alphabet for `where`-expression strings ──────────────
//
// A mix of identifier chars, digits, whitespace, quotes, comparison
// symbols, SQL punctuation, and the keyword letters — so the lexer's
// every branch + the parser's success AND error paths are exercised.
const WHERE_ALPHABET: &[u8] =
    b"abcdef_0123 '\"=!<>().,:;-%&|ANDORTUElikenul \\";

fn random_where_expr(rng: &mut Lcg) -> String {
    let len = rng.next_in(0, 48);
    let mut s = String::with_capacity(len);
    for _ in 0..len {
        let idx = rng.next_in(0, WHERE_ALPHABET.len() - 1);
        s.push(WHERE_ALPHABET[idx] as char);
    }
    s
}

/// A structured (often well-formed) `where` expression — exercises the
/// parser's success path + embeds injection payloads in value position.
fn structured_where_expr(rng: &mut Lcg) -> String {
    const COLS: &[&str] = &["id", "name", "status", "col_1", "_x", "AND"];
    const OPS: &[&str] = &["=", "!=", ">", "<", ">=", "<=", "LIKE", "==", "<>"];
    const VALS: &[&str] = &[
        "1",
        "-42",
        "3.14",
        "true",
        "false",
        "null",
        "'plain'",
        "'; DROP TABLE users; --'",
        "'\\' OR 1=1'",
        "\"double\"",
        "bareword",
        "1.2.3",
    ];
    const CONNS: &[&str] = &["AND", "OR", "and", "or"];

    let conditions = rng.next_in(1, 4);
    let mut parts: Vec<String> = Vec::new();
    for i in 0..conditions {
        if i > 0 {
            parts.push(CONNS[rng.next_in(0, CONNS.len() - 1)].to_string());
        }
        parts.push(COLS[rng.next_in(0, COLS.len() - 1)].to_string());
        parts.push(OPS[rng.next_in(0, OPS.len() - 1)].to_string());
        parts.push(VALS[rng.next_in(0, VALS.len() - 1)].to_string());
    }
    parts.join(" ")
}

/// The D4 injection-resistance invariant: a compiled `WHERE` clause is
/// pure structure — double-quoted identifiers, operators, `$N`
/// placeholders, `IS [NOT] NULL`, connectors. It can therefore NEVER
/// contain a single quote, a statement terminator, or a SQL comment —
/// every user value rode out as a bind parameter.
fn assert_clause_injection_safe(clause: &str, params: &[SqlValue], seed: u64) {
    assert!(
        !clause.contains('\''),
        "seed {seed}: a single quote leaked into the clause `{clause}` \
         — a string value reached SQL text unparameterized"
    );
    assert!(
        !clause.contains(';'),
        "seed {seed}: a `;` leaked into the clause `{clause}`"
    );
    assert!(
        !clause.contains("--"),
        "seed {seed}: a `--` comment leaked into the clause `{clause}`"
    );
    assert_eq!(
        clause.matches('$').count(),
        params.len(),
        "seed {seed}: clause `{clause}` placeholder count must equal \
         the bind-parameter count"
    );
    assert!(
        !params.iter().any(|v| matches!(v, SqlValue::Null)),
        "seed {seed}: a NULL must never occupy a bind slot — it folds \
         to IS NULL / IS NOT NULL"
    );
}

// ════════════════════════════════════════════════════════════════════
// section 1 — Filter compiler totality + SQL-injection resistance
// ════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_s1_filter_compiler_is_total_and_injection_proof() {
    let mut rng = Lcg::new(0x35_0b_f1_17_3e_5a_91_c7);
    for iter in 0..6000u64 {
        let seed = rng.0;
        // Alternate pure garbage with structured exprs so both the
        // lexer's error branches and the parser's success path run.
        let expr = if rng.next_bool() {
            random_where_expr(&mut rng)
        } else {
            structured_where_expr(&mut rng)
        };
        let offset = rng.next_in(0, 64);

        // `parse_filter` is total — never panics.
        let _ = parse_filter(&expr, &nb());

        // `build_pg_where` is total; a compiled clause is injection-safe.
        match build_pg_where(&expr, offset, &nb(), &nb()) {
            Ok((clause, params)) => {
                assert_clause_injection_safe(&clause, &params, seed);
                // Placeholders are sequential from `offset + 1`.
                for (i, _) in params.iter().enumerate() {
                    let placeholder = format!("${}", offset + 1 + i);
                    assert!(
                        clause.contains(&placeholder),
                        "seed {seed}: clause `{clause}` missing \
                         placeholder `{placeholder}`"
                    );
                }
            }
            Err(_) => { /* a typed error is a valid total outcome */ }
        }
        let _ = iter;
    }
}

// ════════════════════════════════════════════════════════════════════
// section 2 — Closed-catalog classifier totality
// ════════════════════════════════════════════════════════════════════

const ASCII_ISH: &[u8] =
    b"abcdefghijklmnop_.0123456789 -ABCXYZ[]\t/qrstuvwxyz";

fn random_token(rng: &mut Lcg) -> String {
    let len = rng.next_in(0, 24);
    let mut s = String::with_capacity(len);
    for _ in 0..len {
        s.push(ASCII_ISH[rng.next_in(0, ASCII_ISH.len() - 1)] as char);
    }
    s
}

#[test]
fn fuzz_s2_closed_catalog_classifiers_are_total() {
    let mut rng = Lcg::new(0x35_2c_a7_09_d1_44_be_03);
    for _ in 0..6000u64 {
        let tok = random_token(&mut rng);

        // Every classifier is total: a value, never a panic.
        let _ = classify_backend(&tok);
        let _ = classify_pg_type(&tok);
        let _ = is_safe_identifier(&tok);
        let _ = BackpressurePolicy::from_slug(&tok);
        // resolve_on_breach is total (closed catalog, defensive default).
        let policy = resolve_on_breach(&tok);
        assert!(matches!(
            policy,
            OnBreachPolicy::Log | OnBreachPolicy::Raise | OnBreachPolicy::Rollback
        ));
        // resolve_dsn is total — Ok(dsn) or a typed error, never panic.
        let _ = resolve_dsn(&tok);

        // A safe identifier is ASCII, non-empty, ≤ 63 bytes.
        if is_safe_identifier(&tok) {
            assert!(!tok.is_empty() && tok.len() <= 63);
            assert!(tok.is_ascii());
        }
        // A classified backend round-trips its canonical spelling.
        if let Some(kind) = classify_backend(&tok) {
            assert!(!kind.as_str().is_empty());
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// section 3 — Epistemic grading totality
// ════════════════════════════════════════════════════════════════════

fn random_sql_value(rng: &mut Lcg) -> SqlValue {
    match rng.next_in(0, 4) {
        0 => SqlValue::Text(random_token(rng)),
        1 => SqlValue::Integer(rng.next_u64() as i64),
        2 => SqlValue::Float(f64::from_bits(rng.next_u64())),
        3 => SqlValue::Boolean(rng.next_bool()),
        _ => SqlValue::Null,
    }
}

fn random_json(rng: &mut Lcg) -> serde_json::Value {
    match rng.next_in(0, 5) {
        0 => serde_json::Value::Null,
        1 => serde_json::Value::Bool(rng.next_bool()),
        2 => serde_json::json!(rng.next_u64() as i64),
        3 => serde_json::json!(f64::from_bits(rng.next_u64())),
        4 => serde_json::Value::String(random_token(rng)),
        _ => serde_json::json!({ "k": random_token(rng) }),
    }
}

fn random_optional_floor(rng: &mut Lcg) -> Option<f64> {
    if rng.next_bool() {
        None
    } else {
        Some(f64::from_bits(rng.next_u64()))
    }
}

#[test]
fn fuzz_s3_epistemic_grading_is_total() {
    let mut rng = Lcg::new(0x35_3e_11_88_aa_67_cd_2f);
    for _ in 0..3000u64 {
        // confidence extraction — total over arbitrary values.
        let _ = confidence_of_json(&random_json(&mut rng));
        let _ = confidence_of_sql(&random_sql_value(&mut rng));

        // retrieve-floor partition — total, and lossless: every row
        // lands in exactly one of (trusted, below_floor).
        let row_count = rng.next_in(0, 12);
        let rows: Vec<StoreRow> = (0..row_count)
            .map(|_| {
                let mut cols = vec![(
                    "id".to_string(),
                    serde_json::json!(rng.next_u64() as i64),
                )];
                if rng.next_bool() {
                    cols.push((
                        "_confidence".to_string(),
                        random_json(&mut rng),
                    ));
                }
                StoreRow { columns: cols }
            })
            .collect();
        let floor = random_optional_floor(&mut rng);
        let outcome =
            enforce_retrieve_floor(mark_retrieved(rows), floor);
        assert_eq!(
            outcome.trusted.len() + outcome.below_floor.len(),
            row_count,
            "the floor partition must neither lose nor duplicate a row"
        );

        // persist-floor — total: Ok or a typed epistemic error.
        let data_len = rng.next_in(0, 6);
        let data: Vec<(String, SqlValue)> = (0..data_len)
            .map(|i| {
                let key = if rng.next_in(0, 3) == 0 {
                    "_confidence".to_string()
                } else {
                    format!("col_{i}")
                };
                (key, random_sql_value(&mut rng))
            })
            .collect();
        let _ = enforce_persist_floor(
            &data,
            random_optional_floor(&mut rng),
            "fuzz_store",
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// section 4 — Audit-chain integrity under arbitrary delta sequences
// ════════════════════════════════════════════════════════════════════

fn random_mutation_kind(rng: &mut Lcg) -> StoreMutationKind {
    match rng.next_in(0, 2) {
        0 => StoreMutationKind::Persist,
        1 => StoreMutationKind::Mutate,
        _ => StoreMutationKind::Purge,
    }
}

#[test]
fn fuzz_s4_audit_chain_integrity_under_arbitrary_sequences() {
    let mut rng = Lcg::new(0x35_4d_c0_3b_72_19_ef_85);
    for _ in 0..1500u64 {
        let key_a: Vec<u8> =
            (0..32).map(|_| (rng.next_u64() & 0xff) as u8).collect();
        let key_b: Vec<u8> =
            (0..32).map(|_| (rng.next_u64() & 0xff) as u8).collect();
        let mut chain = StoreAuditChain::with_key(key_a.clone());
        let mut twin = StoreAuditChain::with_key(key_b.clone());

        let deltas = rng.next_in(0, 20);
        let mut heads: Vec<String> = vec![chain.head()];
        for _ in 0..deltas {
            let kind = random_mutation_kind(&mut rng);
            let store = random_token(&mut rng);
            let summary = random_token(&mut rng);
            chain.record(kind, &store, &summary);
            twin.record(kind, &store, &summary);
            heads.push(chain.head());
        }

        // An untampered chain ALWAYS verifies Intact, whatever the
        // sequence — the core D9 integrity invariant.
        assert_eq!(
            chain.verify(),
            ChainVerdict::Intact,
            "an untampered chain must verify Intact"
        );
        assert_eq!(chain.len(), deltas);

        // The head advances monotonically — every recorded head is
        // distinct (a Merkle chain never repeats a root).
        for window in heads.windows(2) {
            assert_ne!(
                window[0], window[1],
                "the chain head must advance on every append"
            );
        }

        // The head is key-bound: the SAME delta sequence under a
        // DIFFERENT key yields a DIFFERENT head — a forger without the
        // key cannot reproduce the chain. (Distinct keys w.h.p.)
        if deltas > 0 && key_a != key_b {
            assert_ne!(
                chain.head(),
                twin.head(),
                "the chain head must be HMAC-key-bound"
            );
        }

        // `audit` is total — Intact chain → a Clean outcome for any
        // resolved on_breach policy.
        for raw in ["log", "raise", "rollback", "", "GARBAGE"] {
            let outcome = chain.audit("fuzz_store", resolve_on_breach(raw));
            assert!(!outcome.is_halting(), "an Intact chain never halts");
        }
        // apply_on_breach is total over the Tampered verdict too.
        let _ = apply_on_breach(
            "s",
            ChainVerdict::Tampered,
            resolve_on_breach("raise"),
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// section 5 — Store-resolution totality
// ════════════════════════════════════════════════════════════════════

fn random_backend_str(rng: &mut Lcg) -> String {
    match rng.next_in(0, 5) {
        0 => "postgresql".to_string(),
        1 => "in_memory".to_string(),
        2 => String::new(),
        3 => "sqlite".to_string(),
        4 => "mysql".to_string(),
        _ => random_token(rng),
    }
}

#[test]
fn fuzz_s5_store_resolution_is_total() {
    let mut rng = Lcg::new(0x35_5a_2f_d6_84_b1_07_3e);
    for _ in 0..1500u64 {
        // Build a registry from an arbitrary set of axonstore specs.
        let spec_count = rng.next_in(0, 8);
        let specs: Vec<IRAxonStore> = (0..spec_count)
            .map(|i| IRAxonStore {
                node_type: "axonstore",
                source_line: 0,
                source_column: 0,
                // Some specs collide on name (force the duplicate path).
                name: if rng.next_bool() {
                    "shared".to_string()
                } else {
                    format!("store_{i}")
                },
                backend: random_backend_str(&mut rng),
                connection: random_token(&mut rng),
                confidence_floor: None,
                isolation: String::new(),
                on_breach: String::new(),
                capability: String::new(),
                class: String::new(),
                column_schema: None,
                resource_ref: String::new(),
            })
            .collect();

        // `build` is total — Ok(registry) or a typed RegistryError.
        match StoreRegistry::build(&specs) {
            Ok(registry) => {
                // resolution of an in_memory / undeclared store is
                // pure + total (the postgresql path needs a runtime,
                // exercised in 35.l).
                let handle = registry.resolve("never_declared");
                assert!(handle.is_ok());
            }
            Err(_) => { /* a typed build error is a valid outcome */ }
        }

        // `check_store_capability` is total over arbitrary slugs.
        let required = random_token(&mut rng);
        let held: Vec<String> =
            (0..rng.next_in(0, 5)).map(|_| random_token(&mut rng)).collect();
        let verdict =
            check_store_capability("fuzz_store", &required, &held);
        // An empty requirement, or a held requirement, ⇒ allowed.
        if required.is_empty() || held.iter().any(|h| h == &required) {
            assert!(verdict.is_ok(), "an held/empty capability must allow");
        } else {
            assert!(verdict.is_err(), "a missing capability must deny");
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// section 6 — SQL builder totality + injection resistance
// ════════════════════════════════════════════════════════════════════

fn random_identifier_ish(rng: &mut Lcg) -> String {
    // Sometimes a valid identifier, sometimes garbage — both paths.
    if rng.next_bool() {
        let names = ["users", "tenants", "ledger", "col_1", "_x"];
        names[rng.next_in(0, names.len() - 1)].to_string()
    } else {
        random_token(rng)
    }
}

#[test]
fn fuzz_s6_sql_builders_are_total_and_injection_proof() {
    let mut rng = Lcg::new(0x35_6f_b8_41_2d_9c_56_a0);
    for _ in 0..6000u64 {
        let seed = rng.0;
        let table = random_identifier_ish(&mut rng);
        let where_expr = if rng.next_bool() {
            random_where_expr(&mut rng)
        } else {
            structured_where_expr(&mut rng)
        };
        let data: Vec<(String, SqlValue)> = (0..rng.next_in(0, 5))
            .map(|_| (random_identifier_ish(&mut rng), random_sql_value(&mut rng)))
            .collect();

        // v1.32.0 — the four builders gained a `schema:
        // Option<&str>` parameter; `None` renders the bare table (this
        // section 6 fuzz exercises builder totality + injection-safety, which
        // is orthogonal to schema-qualification).
        if let Ok((sql, params)) =
            build_select_sql(&table, None, &where_expr, &nb(), &nb())
        {
            assert_clause_injection_safe(&sql, &params, seed);
        }
        if let Ok((sql, params)) =
            build_delete_sql(&table, None, &where_expr, &nb(), &nb())
        {
            assert_clause_injection_safe(&sql, &params, seed);
        }
        if let Ok((sql, params)) = build_insert_sql(&table, None, &data, &nb())
        {
            assert_clause_injection_safe(&sql, &params, seed);
        }
        if let Ok((sql, params)) =
            build_update_sql(&table, None, &where_expr, &data, &nb(), &nb())
        {
            assert_clause_injection_safe(&sql, &params, seed);
        }
    }
}
