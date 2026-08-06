//! §Fase 118.b.3 — this suite exercises the SQL data plane (the Postgres
//! backend, the pinned-connection contract, live introspection), which lives
//! behind the `postgres` feature. Gated at the file level: the core suites keep
//! running in a build with no driver.
#![cfg(feature = "postgres")]

//! §Fase 38.x.c — IDENTITY Column Recognition diagnostic anchor.
//!
//! Pins the third kivi smoke-16 finding (2026-05-20 migration doc msg
//! #11) and the §-assertions that close it. Every §-assertion inverts
//! in place — pre-v1.38.3 the test pins the BROKEN state; v1.38.3's
//! `identity: bool` field + T803 skip make the assertions GREEN. A
//! future PR that regresses the IDENTITY surface turns this anchor RED.
//!
//! # The contract (D1–D5)
//!
//! - **D1** — `axon-rs/src/store/introspect_cli.rs` queries
//!   `pg_attribute.attidentity` and surfaces it as
//!   `IntrospectionRow.identity_kind: Option<char>`.
//! - **D2** — `StoreColumn` / `ManifestColumn` / `DeclaredColumn` /
//!   `IRStoreColumn` all carry a `pub identity: bool` field.
//!   Manifest canonical JSON omits the key when `false` (D5 absolute).
//! - **D3** — T803 treats `identity: true` as safely omittable from a
//!   `persist` (Postgres auto-fills the column).
//! - **D4** — `auto_increment` semantics unchanged (only legacy SERIAL
//!   via `nextval(...)` default).
//! - **D5** — backwards-compat absolute. v1.38.2 manifests parse
//!   byte-identically against v1.38.3.

use axon_frontend::store_column_proof::{
    check_persist_fields, ColumnSet, FlowParamTypes, ProofErrorCode,
};
use axon_frontend::store_introspect::{
    build_manifest_store, detect_auto_increment, IntrospectionRow,
};
use axon_frontend::store_schema::StoreColumnType;
use axon_frontend::store_schema_manifest::{Manifest, ManifestColumn, ManifestStore};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

// ════════════════════════════════════════════════════════════════════
//  §1 — The kivi smoke-16 #11 corpus pin
// ════════════════════════════════════════════════════════════════════

/// §1 — pin the kivi `chat_history.id` collision class structurally.
/// The adopter's symptom (paraphrased): T803 fires on a `persist`
/// that omits `id` because the type-checker sees `id BigInt
/// primary_key not_null` with no default. The adopter's workaround
/// was to delete `id` from the manifest entirely — which then
/// silences EVERY proof for that column.
///
/// This §-assertion never inverts — it's the intent pin so any
/// future test author breaking the IDENTITY surface knows what
/// regression class to look for.
#[test]
fn s1_kivi_chat_history_identity_corpus() {
    let corpus = [
        // The DDL kivi runs:
        "CREATE TABLE chat_history (id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY, ...);",
        // The error v1.38.2 emitted before v1.38.3:
        "axon-T803 `persist` omits NOT-NULL column `id` (BigInt, primary_key) with no default",
        // The workaround the adopter had to apply:
        "manifest omits the `id` column entirely",
        // The two Postgres channels for auto-fill:
        "pg_attrdef.adbin",       // channel #1: legacy SERIAL default
        "pg_attribute.attidentity", // channel #2: IDENTITY syntax
    ];
    for s in &corpus {
        assert!(!s.is_empty(), "corpus entry non-empty");
    }
    assert_eq!(corpus.len(), 5);
}

// ════════════════════════════════════════════════════════════════════
//  §2 — D3 BEHAVIOURAL — T803 SKIPS a NOT-NULL identity column
// ════════════════════════════════════════════════════════════════════

/// §2 — pin that T803 does NOT fire when a `persist` omits a NOT-NULL
/// identity column. This is the load-bearing behavioural change of
/// v1.38.3.
///
/// Pre-fix: `manifest_column.identity = false` (the field didn't
/// exist) → `has_default = !default_value.is_empty() || auto_increment`
/// → `false` → T803 fires.
///
/// Post-fix: `manifest_column.identity = true` (introspect emits it)
/// → `has_default = ... || identity` → `true` → T803 skips the
/// column. The persist with only the adopter-supplied columns
/// compiles green.
#[test]
fn s2_d3_t803_skips_notnull_identity_column() {
    let mut cols = BTreeMap::new();
    cols.insert(
        "id".to_string(),
        ManifestColumn {
            col_type: StoreColumnType::BigInt,
            primary_key: true,
            auto_increment: false,
            not_null: true,
            unique: false,
            default_value: String::new(),
            // §38.x.c — the kivi `chat_history.id` case:
            // `GENERATED ALWAYS AS IDENTITY`
            identity: true,
        },
    );
    cols.insert(
        "tenant_id".to_string(),
        ManifestColumn {
            col_type: StoreColumnType::Uuid,
            primary_key: false,
            auto_increment: false,
            not_null: true,
            unique: false,
            default_value: String::new(),
            identity: false,
        },
    );
    cols.insert(
        "content".to_string(),
        ManifestColumn {
            col_type: StoreColumnType::Text,
            primary_key: false,
            auto_increment: false,
            not_null: true,
            unique: false,
            default_value: String::new(),
            identity: false,
        },
    );
    let store = ManifestStore { columns: cols };
    let columns = ColumnSet::from_manifest_store(&store);

    // Persist provides everything EXCEPT `id` (Postgres will auto-fill).
    let fields: Vec<(String, String)> = vec![
        ("tenant_id".into(), "${tid}".into()),
        ("content".into(), "${msg}".into()),
    ];
    let mut params = FlowParamTypes::new();
    params.insert("tid".into(), "Uuid".into());
    params.insert("msg".into(), "Text".into());

    let errs = check_persist_fields(&fields, &columns, &params, (1, 1));

    assert!(
        errs.is_empty(),
        "v1.38.3 D3 invariant — T803 must SKIP a NOT-NULL identity \
         column omitted from a `persist`. Got errors: {:#?}",
        errs
    );
}

/// §2-companion — the negative control: T803 STILL fires on a plain
/// NOT-NULL column (no default, no auto_increment, no identity).
#[test]
fn s2_d3_t803_still_fires_on_plain_notnull_column() {
    let mut cols = BTreeMap::new();
    cols.insert(
        "required_col".to_string(),
        ManifestColumn {
            col_type: StoreColumnType::Text,
            primary_key: false,
            auto_increment: false,
            not_null: true,
            unique: false,
            default_value: String::new(),
            identity: false,
        },
    );
    cols.insert(
        "other_col".to_string(),
        ManifestColumn {
            col_type: StoreColumnType::Text,
            primary_key: false,
            auto_increment: false,
            not_null: false,
            unique: false,
            default_value: String::new(),
            identity: false,
        },
    );
    let store = ManifestStore { columns: cols };
    let columns = ColumnSet::from_manifest_store(&store);

    let fields: Vec<(String, String)> =
        vec![("other_col".into(), "${val}".into())];
    let mut params = FlowParamTypes::new();
    params.insert("val".into(), "Text".into());

    let errs = check_persist_fields(&fields, &columns, &params, (1, 1));
    assert!(
        errs.iter().any(|e| matches!(
            e.code,
            ProofErrorCode::T803NotNullOmitted
        )),
        "T803 must still fire on plain NOT-NULL omission. Got: {:#?}",
        errs
    );
}

// ════════════════════════════════════════════════════════════════════
//  §3 — D2 manifest round-trip: `identity: true` survives canonical
// ════════════════════════════════════════════════════════════════════

/// §3 — pin that `identity: true` round-trips through canonical JSON
/// serialization + parsing. The byte-deterministic emission is what
/// the §38.c content_hash relies on.
#[test]
fn s3_d2_identity_field_canonical_roundtrip() {
    let mut cols = BTreeMap::new();
    cols.insert(
        "id".to_string(),
        ManifestColumn {
            col_type: StoreColumnType::BigInt,
            primary_key: true,
            auto_increment: false,
            not_null: true,
            unique: false,
            default_value: String::new(),
            identity: true,
        },
    );
    let store = ManifestStore { columns: cols };
    let mut m = Manifest::new();
    m.stores.insert("public.chat_history".to_string(), store);
    m.refresh_content_hash();

    let canonical = m.canonical_serialize(true);
    assert!(
        canonical.contains("\"identity\":true"),
        "v1.38.3 canonical serialization must emit `identity:true` \
         when set. Got:\n{canonical}"
    );

    let reparsed = Manifest::parse_json(&canonical).expect("reparses clean");
    let col = reparsed
        .stores
        .get("public.chat_history")
        .unwrap()
        .columns
        .get("id")
        .unwrap();
    assert!(col.identity, "identity:true survives round-trip");
    assert!(!col.auto_increment, "identity ≠ auto_increment (D4)");
    reparsed
        .verify_content_hash()
        .expect("content_hash valid post-roundtrip");
}

/// §3-companion — D5 absolute: a v1.38.2 manifest (no `identity`
/// field) parses with `identity = false` for every column AND
/// re-serializes byte-identically (the `identity` key is OMITTED
/// when false).
#[test]
fn s3_d5_v1_38_2_manifest_round_trips_without_identity_key() {
    // A v1.38.2-style manifest body (no `identity` field anywhere).
    // Parsing must default every column's `identity` to `false`, and
    // re-serializing in canonical form must NOT introduce an
    // `identity` key — keeping the on-disk shape byte-identical to
    // what v1.38.2 would have emitted for the same logical content.
    let v1_38_2_json = r#"{"stores":{"public.tenants":{"columns":{"tenant_id":{"type":"Uuid","primary_key":true,"not_null":true}}}},"version":1}"#;
    let parsed = Manifest::parse_json(v1_38_2_json).expect("legacy manifest parses");
    let col = parsed
        .stores
        .get("public.tenants")
        .unwrap()
        .columns
        .get("tenant_id")
        .unwrap();
    assert!(!col.identity, "absent `identity` field defaults to false (D5)");
    let reserialized = parsed.canonical_serialize(false);
    assert!(
        !reserialized.contains("\"identity\""),
        "D5 absolute — re-serializing a column with `identity = false` \
         must NOT introduce the `identity` key in the canonical form. \
         Got:\n{reserialized}"
    );
    // Re-parse the canonical form and confirm the second round-trip is
    // a fixed point (byte-identical post-canonicalisation).
    let reparsed = Manifest::parse_json(&reserialized).expect("canonical reparses");
    let recanonical = reparsed.canonical_serialize(false);
    assert_eq!(
        recanonical, reserialized,
        "canonical-form fixed point: second round-trip is byte-identical"
    );
}

// ════════════════════════════════════════════════════════════════════
//  §4 — D1 STATIC: introspect_cli query mentions attidentity
// ════════════════════════════════════════════════════════════════════

/// §4 — grep §-assertion: the deep introspection query in
/// `axon-rs/src/store/introspect_cli.rs` SELECTs
/// `a.attidentity AS identity_kind`. Without this, the channel-#2
/// auto-fill signal never reaches Rust → the manifest emitted by
/// `axon store introspect` would not carry `identity: true` even
/// when the live DB has GENERATED IDENTITY columns.
#[test]
fn s4_d1_introspect_query_selects_attidentity() {
    let cli_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("store")
        .join("introspect_cli.rs");
    let body = fs::read_to_string(&cli_path).expect("introspect_cli readable");
    assert!(
        body.contains("a.attidentity"),
        "D1 invariant — the deep introspection query in \
         `axon-rs/src/store/introspect_cli.rs` MUST SELECT \
         `a.attidentity` from `pg_attribute`. Without this, IDENTITY \
         columns can never round-trip through the manifest."
    );
    assert!(
        body.contains("identity_kind"),
        "D1 invariant — the query MUST expose the column AS \
         `identity_kind` (the IntrospectionRow field name). Found no \
         such alias in `introspect_cli.rs`."
    );
}

// ════════════════════════════════════════════════════════════════════
//  §5 — D1 BUILDER: build_manifest_store emits identity:true
// ════════════════════════════════════════════════════════════════════

/// §5 — pin `build_manifest_store` translates `identity_kind: Some(_)`
/// into `ManifestColumn.identity = true`. Tests the pure-Rust
/// half of D1 (no Postgres required).
#[test]
fn s5_d1_build_manifest_store_emits_identity_on_attidentity_a() {
    let rows = vec![IntrospectionRow {
        column_name: "id".into(),
        pg_udt: "int8".into(),
        not_null: true,
        primary_key: true,
        unique: false,
        default_expression: String::new(),
        identity_kind: Some('a'), // GENERATED ALWAYS AS IDENTITY
    }];
    let (store, omissions) = build_manifest_store(&rows);
    let col = store.columns.get("id").expect("id column present");
    assert!(col.identity, "identity_kind=Some('a') → identity:true");
    assert!(
        !col.auto_increment,
        "identity columns have NO nextval default → auto_increment:false"
    );
    assert!(omissions.is_empty());
}

#[test]
fn s5_d1_build_manifest_store_emits_identity_on_attidentity_d() {
    let rows = vec![IntrospectionRow {
        column_name: "id".into(),
        pg_udt: "int8".into(),
        not_null: true,
        primary_key: true,
        unique: false,
        default_expression: String::new(),
        identity_kind: Some('d'), // GENERATED BY DEFAULT AS IDENTITY
    }];
    let (store, _) = build_manifest_store(&rows);
    let col = store.columns.get("id").expect("id column present");
    assert!(col.identity, "identity_kind=Some('d') → identity:true");
    assert!(!col.auto_increment);
}

#[test]
fn s5_d1_serial_pattern_still_recognized_as_auto_increment_not_identity() {
    let rows = vec![IntrospectionRow {
        column_name: "id".into(),
        pg_udt: "int4".into(),
        not_null: true,
        primary_key: true,
        unique: false,
        default_expression: "nextval('public.users_id_seq'::regclass)".into(),
        identity_kind: None,
    }];
    let (store, _) = build_manifest_store(&rows);
    let col = store.columns.get("id").expect("id column present");
    assert!(col.auto_increment, "nextval default → auto_increment:true");
    assert!(!col.identity, "SERIAL is NOT identity (D4 separation)");
    assert!(detect_auto_increment(&rows[0].default_expression));
}
