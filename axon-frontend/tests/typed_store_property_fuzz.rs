//! v1.31.0 — Property / fuzz pack for the Declared & Compile-
//! Time-Typed Store Schema cycle.
//!
//! Three deterministic-LCG surfaces — ~6 000 total iterations,
//! mirroring the 35.k / 32.i / 33.g / 37.x.i fuzz discipline. No
//! `proptest` / `quickcheck` dep; every regression reproduces
//! verbatim from the printed seed.
//!
//!   - **Surface A — `Manifest::parse_json` totality** (~2 000 iters).
//!     Random JSON bodies (well-formed + malformed + non-object roots
//!     + missing-field shapes + unknown-type shapes) MUST never panic;
//!     the result is always `Ok(Manifest)` OR `Err(ManifestError)`
//!     from the closed 6-variant error catalog. Deterministic — the
//!     same input produces the same verdict bytes.
//!
//!   - **Surface B — `check_filter` totality** (~2 000 iters). Random
//!     `where:` strings × random declared `ColumnSet` × random flow-
//!     parameter maps. NEVER panic. Every emitted `ProofError` has a
//!     code in the `axon-T80x` namespace. Deterministic.
//!
//!   - **Surface C — `canonical_serialize` is a fixed point** (~2 000
//!     iters). A randomly-constructed Manifest serialised in canonical
//!     JSON, re-parsed, and re-serialised yields BYTE-IDENTICAL output
//!     on the second pass. The content_hash is stable across the
//!     round-trip. The hash-bearing canonical form verifies.
//!
//! Total iteration budget ≈ 6 000; pack runs in well under a second
//! (every surface is pure, no I/O).

#![allow(clippy::needless_return)]

use std::collections::BTreeMap;

use axon_frontend::store_column_proof::{
    check_filter, ColumnSet, FlowParamTypes, ProofErrorCode,
};
use axon_frontend::store_schema::StoreColumnType;
use axon_frontend::store_schema_manifest::{
    Manifest, ManifestColumn, ManifestError, ManifestStore,
};

// ── section 0 — Deterministic LCG (37.x.i / 35.k pattern) ───────────────────

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next_u64(&mut self) -> u64 {
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
//  Surface A — Manifest::parse_json totality
//
//  Invariants:
//   A.1  Never panic for ANY input string (random ASCII / printable
//        bytes / well-formed + malformed JSON).
//   A.2  Every Err is a variant of the closed `ManifestError` catalog
//        (InvalidJson / InvalidStructure / UnsupportedVersion /
//        UnknownColumnType / ContentHashMismatch). DuplicateStore is
//        impossible from a SINGLE-file parse (it's a merge-time error)
//        — assert it never surfaces.
//   A.3  Pure function: same input → same verdict (parse twice, check
//        equality).
//   A.4  When `Ok(manifest)`: `canonical_serialize(false)` is non-empty,
//        starts with `{`, ends with `}`, and re-parses to the same
//        `Manifest` value (PartialEq).
// ════════════════════════════════════════════════════════════════════

const MANIFEST_FRAGMENTS: &[&str] = &[
    r#"{"version":1,"stores":{}}"#,
    r#"{"version":1,"stores":{"a":{"columns":{"id":{"type":"Uuid"}}}}}"#,
    r#"{"version":2,"stores":{}}"#,                              // UnsupportedVersion
    r#"{"version":1}"#,                                            // missing stores
    r#"{"stores":{"a":{"columns":{}}}}"#,                          // missing version
    r#"{"version":1,"stores":{"a":{"columns":{"x":{"type":"Money"}}}}}"#, // UnknownColumnType
    r#"["not", "an", "object"]"#,                                  // non-object root
    r#"{"version":1,"stores":{"a":{}}}"#,                          // missing columns
    r#"{"version":1,"stores":{"a":{"columns":{"x":42}}}}"#,         // non-object column
    r#"{not valid json"#,
    r#"null"#,
    r#"true"#,
    r#"{"version":"one","stores":{}}"#,                            // wrong-type version
    r#""#,                                                          // empty
    r#"{}"#,
    r#"{"version":1,"stores":{"a":{"columns":{"x":{"type":"Int","primary_key":true,"not_null":true}}}}}"#,
    r#"{"version":1,"stores":{"a":{"columns":{"x":{"type":"Text","default_value":"'standard'::text"}}}}}"#,
    r#"{"version":1,"stores":{"a":{"columns":{"x":{"type":"int"}}}}}"#,  // alias normalises
    r#"{"version":1,"stores":{"a":{"columns":{"x":{"type":"Uuid"}}}},"content_hash":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}"#,
];

fn random_manifest_input(rng: &mut Lcg) -> String {
    if rng.next_bool() {
        // Half the iterations: pick a curated fragment (well-formed
        // OR malformed shapes — exercises every Err arm).
        MANIFEST_FRAGMENTS[rng.next_in(0, MANIFEST_FRAGMENTS.len() - 1)].to_string()
    } else if rng.next_bool() {
        // Quarter: synthesise a programmatic random manifest body.
        synthesise_random_manifest_body(rng)
    } else {
        // Quarter: random ASCII garbage — exercises the InvalidJson
        // path purely.
        random_ascii_garbage(rng)
    }
}

fn synthesise_random_manifest_body(rng: &mut Lcg) -> String {
    let mut out = String::new();
    out.push('{');
    if rng.next_bool() {
        out.push_str(r#""version":1,"#);
    }
    out.push_str(r#""stores":{"#);
    let n_stores = rng.next_in(0, 3);
    for i in 0..n_stores {
        if i > 0 {
            out.push(',');
        }
        let store_name = ["public.tenants", "audit.log", "tenant_a.usage", "x.y"]
            [rng.next_in(0, 3)];
        out.push_str(&format!(r#""{store_name}":{{"columns":{{"#));
        let n_cols = rng.next_in(0, 4);
        for j in 0..n_cols {
            if j > 0 {
                out.push(',');
            }
            let col_name = ["id", "name", "active", "tenant_id", "count"]
                [rng.next_in(0, 4)];
            let type_token = ["Uuid", "Text", "Int", "BigInt", "Bool", "MysteryType", "int4"]
                [rng.next_in(0, 6)];
            out.push_str(&format!(r#""{col_name}":{{"type":"{type_token}""#));
            if rng.next_bool() {
                out.push_str(r#","primary_key":true"#);
            }
            if rng.next_bool() {
                out.push_str(r#","not_null":true"#);
            }
            out.push_str("}");
        }
        out.push_str("}}");
    }
    out.push_str("}}");
    out
}

fn random_ascii_garbage(rng: &mut Lcg) -> String {
    const ALPHABET: &[u8] = b"abcdef0123 {}[]:,\"'\\";
    let len = rng.next_in(0, 64);
    (0..len)
        .map(|_| ALPHABET[rng.next_in(0, ALPHABET.len() - 1)] as char)
        .collect()
}

#[test]
fn fuzz_surface_a_manifest_parse_is_total_and_deterministic() {
    let mut rng = Lcg::new(0x38_0a_d4_31_5e_9d_a1_b7);
    let iters: u64 = 2_000;
    for iter in 0..iters {
        let seed = rng.0;
        let src = random_manifest_input(&mut rng);

        // A.1 + A.2 — totality + closed error catalog.
        let verdict = Manifest::parse_json(&src);
        if let Err(ref e) = verdict {
            // A.2 — DuplicateStore is a merge-time-only error.
            assert!(
                !matches!(e, ManifestError::DuplicateStore { .. }),
                "seed {seed}: DuplicateStore must not surface from a \
                 single-file parse"
            );
        }

        // A.3 — determinism.
        let verdict_again = Manifest::parse_json(&src);
        assert_eq!(
            verdict, verdict_again,
            "seed {seed}: parse_json is not deterministic for input {src:?}"
        );

        // A.4 — canonical_serialize-then-reparse round-trip.
        if let Ok(m) = verdict {
            let canonical = m.canonical_serialize(false);
            assert!(
                canonical.starts_with('{') && canonical.ends_with('}'),
                "seed {seed}: canonical form malformed: {canonical:?}"
            );
            let reparsed = Manifest::parse_json(&canonical)
                .expect("canonical form must reparse");
            assert_eq!(
                m, reparsed,
                "seed {seed}: canonical round-trip lost structure"
            );
        }
        let _ = iter;
    }
}

// ════════════════════════════════════════════════════════════════════
//  Surface B — check_filter totality
//
//  Invariants:
//   B.1  Never panic for any input (random `where:` strings × random
//        ColumnSet × random FlowParamTypes).
//   B.2  Every emitted ProofError has a code in {T801, T802}
//        (T803/T804 are field-block-only; T805 is manifest-only).
//   B.3  Pure: same inputs → same verdict.
//   B.4  When `where:` is empty OR malformed → zero errors emitted
//        (the runtime parser owns the syntactic error; 38.d only
//        proves the well-formed subset — honest scope).
// ════════════════════════════════════════════════════════════════════

const COLUMN_TYPE_POOL: &[StoreColumnType] = &[
    StoreColumnType::Uuid,
    StoreColumnType::Text,
    StoreColumnType::Int,
    StoreColumnType::BigInt,
    StoreColumnType::Bool,
    StoreColumnType::Numeric,
    StoreColumnType::Timestamptz,
    StoreColumnType::Date,
    StoreColumnType::Jsonb,
];

const FLOW_PARAM_TYPE_POOL: &[&str] =
    &["String", "Int", "Bool", "Uuid", "BigInt", "Float", "Numeric"];

const WHERE_OPS: &[&str] = &["=", "==", "!=", "<>", "<", ">", "<=", ">="];

const WHERE_VALUES: &[&str] = &[
    "1", "42", "-7", "3.14", "true", "false", "null",
    "'plain'", "'standard'",
    "${id}", "${flag}", "${count}", "${name}",
    "${undeclared_param}",
];

const COLUMN_NAME_POOL: &[&str] = &[
    "id", "tenant_id", "name", "tier", "count", "active",
    "created_at", "amount", "tenantid", "wildcard",
];

fn random_column_set(rng: &mut Lcg) -> ColumnSet {
    use axon_frontend::store_schema::StoreColumn;
    let n = rng.next_in(0, 6);
    let mut seen = std::collections::HashSet::new();
    let cols: Vec<StoreColumn> = (0..n)
        .filter_map(|_| {
            let name = COLUMN_NAME_POOL[rng.next_in(0, COLUMN_NAME_POOL.len() - 1)];
            if !seen.insert(name) {
                return None;
            }
            let col_type = COLUMN_TYPE_POOL[rng.next_in(0, COLUMN_TYPE_POOL.len() - 1)];
            Some(StoreColumn {
                name: name.to_string(),
                col_type,
                primary_key: rng.next_bool(),
                auto_increment: false,
                not_null: rng.next_bool(),
                unique: false,
                default_value: String::new(),
                // v1.31.0 — field added by release e92c8ed; the fuzz
                // generator does not exercise IDENTITY columns, so the D5
                // default (false) preserves the prior corpus exactly.
                identity: false,
                // v2.26.0 — the fuzz generator does not exercise the
                // `index` declaration; `false` preserves the prior corpus.
                indexed: false,
                // v2.26.0 — the fuzz generator does not exercise the
                // `Json<T>` shape lens; `None` preserves the prior corpus.
                json_shape: None,
                line: 0,
                column: 0,
            })
        })
        .collect();
    ColumnSet::from_inline_columns(&cols)
}

fn random_flow_params(rng: &mut Lcg) -> FlowParamTypes {
    let mut p = FlowParamTypes::new();
    let n = rng.next_in(0, 4);
    let names = ["id", "flag", "count", "name", "tenant_id"];
    for _ in 0..n {
        let pname = names[rng.next_in(0, names.len() - 1)];
        let ptype = FLOW_PARAM_TYPE_POOL[rng.next_in(0, FLOW_PARAM_TYPE_POOL.len() - 1)];
        p.insert(pname.to_string(), ptype.to_string());
    }
    p
}

fn random_where_expr(rng: &mut Lcg) -> String {
    let mut parts: Vec<String> = Vec::new();
    let n_predicates = rng.next_in(1, 3);
    for i in 0..n_predicates {
        if i > 0 {
            parts.push(if rng.next_bool() { "AND".into() } else { "OR".into() });
        }
        let col = COLUMN_NAME_POOL[rng.next_in(0, COLUMN_NAME_POOL.len() - 1)];
        let op = WHERE_OPS[rng.next_in(0, WHERE_OPS.len() - 1)];
        let val = WHERE_VALUES[rng.next_in(0, WHERE_VALUES.len() - 1)];
        parts.push(format!("{col} {op} {val}"));
    }
    // 1-in-8 chance: inject syntactic noise (the scanner must skip
    // silently, the runtime parser owns the error).
    if rng.next_in(0, 7) == 0 {
        parts.push("AND".into()); // dangling connector
    }
    parts.join(" ")
}

#[test]
fn fuzz_surface_b_check_filter_is_total_and_deterministic() {
    let mut rng = Lcg::new(0x38_0b_d4_42_7e_91_b2_c8);
    let iters: u64 = 2_000;
    for iter in 0..iters {
        let seed = rng.0;
        let columns = random_column_set(&mut rng);
        let params = random_flow_params(&mut rng);
        let where_expr = random_where_expr(&mut rng);

        // B.1 — totality.
        let errors = check_filter(&where_expr, &columns, &params, (1, 1));

        // B.2 — closed error-code namespace.
        for err in &errors {
            assert!(
                matches!(
                    err.code,
                    ProofErrorCode::T801UnknownColumn
                        | ProofErrorCode::T802TypeMismatch
                ),
                "seed {seed}: check_filter emitted an out-of-namespace \
                 error code {:?} for input {where_expr:?}",
                err.code
            );
        }

        // B.3 — determinism.
        let errors_again = check_filter(&where_expr, &columns, &params, (1, 1));
        assert_eq!(
            errors, errors_again,
            "seed {seed}: check_filter is not deterministic for {where_expr:?}"
        );

        let _ = iter;
    }
}

#[test]
fn fuzz_surface_b_empty_where_emits_zero_errors() {
    // B.4 — an empty `where:` clause skips silently (mirrors the
    // runtime fall-through to `WHERE TRUE`).
    let mut rng = Lcg::new(0x38_0b_e0_53_8f_a2_c3_d9);
    for _ in 0..200 {
        let columns = random_column_set(&mut rng);
        let params = random_flow_params(&mut rng);
        for empty_input in ["", "   ", "\t\t"] {
            let errors = check_filter(empty_input, &columns, &params, (1, 1));
            assert!(errors.is_empty(), "empty `where:` must emit no errors");
        }
    }
}

// ════════════════════════════════════════════════════════════════════
//  Surface C — canonical_serialize is a fixed point
//
//  Invariants:
//   C.1  For any constructed Manifest:
//          parse_json(canonical_serialize(m, false)) == m  (PartialEq)
//   C.2  canonical_serialize is idempotent: serialise twice yields
//        the same bytes.
//   C.3  compute_content_hash is stable across the round-trip.
//   C.4  verify_content_hash holds for `refresh_content_hash`'d
//        manifests after the canonical-with-hash round-trip.
// ════════════════════════════════════════════════════════════════════

fn random_constructed_manifest(rng: &mut Lcg) -> Manifest {
    let mut m = Manifest::new();
    let n_stores = rng.next_in(0, 3);
    let store_names = ["public.a", "public.b", "tenant.events", "audit.log"];
    for i in 0..n_stores {
        let name = store_names[i.min(store_names.len() - 1)];
        let mut columns: BTreeMap<String, ManifestColumn> = BTreeMap::new();
        let n_cols = rng.next_in(0, 4);
        for j in 0..n_cols {
            let col_name = ["id", "name", "tier", "count", "active"][j.min(4)];
            let col_type = COLUMN_TYPE_POOL[rng.next_in(0, COLUMN_TYPE_POOL.len() - 1)];
            let default_value = if rng.next_bool() {
                String::new()
            } else {
                "'standard'".to_string()
            };
            columns.insert(
                col_name.to_string(),
                ManifestColumn {
                    col_type,
                    primary_key: rng.next_bool(),
                    auto_increment: false,
                    not_null: rng.next_bool(),
                    unique: rng.next_bool(),
                    default_value,
                    identity: rng.next_bool(),
                },
            );
        }
        m.stores.insert(name.to_string(), ManifestStore { columns });
    }
    m
}

#[test]
fn fuzz_surface_c_canonical_serialize_is_a_fixed_point() {
    let mut rng = Lcg::new(0x38_0c_d4_64_a3_b1_c2_e8);
    let iters: u64 = 2_000;
    for iter in 0..iters {
        let seed = rng.0;
        let m = random_constructed_manifest(&mut rng);

        // C.1 — parse(canonical(m)) == m.
        let canonical = m.canonical_serialize(false);
        let reparsed = Manifest::parse_json(&canonical)
            .unwrap_or_else(|e| panic!("seed {seed}: canonical reparse failed: {e}"));
        assert_eq!(
            m, reparsed,
            "seed {seed}: parse(canonical(m)) != m"
        );

        // C.2 — canonical_serialize is idempotent (byte-identical).
        let canonical_again = reparsed.canonical_serialize(false);
        assert_eq!(
            canonical, canonical_again,
            "seed {seed}: canonical form is not a byte-fixed point"
        );

        // C.3 — content hash is stable.
        let hash_a = m.compute_content_hash();
        let hash_b = reparsed.compute_content_hash();
        assert_eq!(
            hash_a, hash_b,
            "seed {seed}: content hash drifted across round-trip"
        );

        // C.4 — verify_content_hash holds after refresh.
        let mut hashed = reparsed.clone();
        hashed.refresh_content_hash();
        let with_hash = hashed.canonical_serialize(true);
        let with_hash_parsed = Manifest::parse_json(&with_hash)
            .unwrap_or_else(|e| panic!("seed {seed}: hash-bearing reparse failed: {e}"));
        with_hash_parsed
            .verify_content_hash()
            .unwrap_or_else(|e| panic!("seed {seed}: verify_content_hash failed: {e}"));

        let _ = iter;
    }
}

#[test]
fn fuzz_surface_c_hash_differs_when_content_differs() {
    // Spot-check the inverse: two DIFFERENT manifests have different
    // hashes. (A pure-luck collision would be sha256-grade
    // astronomically improbable; this is mostly a sanity pin.)
    let mut rng = Lcg::new(0x38_0c_e1_75_b4_c2_d3_f9);
    let mut seen_hashes: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut content_seen: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for _ in 0..400 {
        let m = random_constructed_manifest(&mut rng);
        let content = m.canonical_serialize(false);
        let hash = m.compute_content_hash();
        // If the canonical content is new, the hash should be too.
        if content_seen.insert(content.clone()) {
            assert!(
                seen_hashes.insert(hash),
                "two distinct canonical contents collided on hash — \
                 either a SHA-256 collision (astronomical) or a hash \
                 implementation regression"
            );
        }
    }
}
