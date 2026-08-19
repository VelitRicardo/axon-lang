//! v1.31.0 — Integration tests for manifest discovery + multi-file
//! merge.
//!
//! The unit tests inside `axon-frontend/src/store_schema_manifest.rs`
//! cover the in-memory parsing / canonical-serialization / hash logic.
//! THIS pack covers the filesystem-touching surface: discovery via
//! `./` + `./schemas/`, multi-file merge, duplicate-store detection
//! across files, and the introspection-CLI round-trip (`introspect →
//! commit → axon check picks it up`).
//!
//! Each test owns a temp directory so the pack is parallel-safe.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use axon_frontend::store_schema::StoreColumnType;
use axon_frontend::store_schema_manifest::{
    discover_manifest_files, load_and_merge_manifests, Manifest, ManifestColumn,
    ManifestError, ManifestStore,
};

/// Create a uniquely-named temp directory under the system temp root.
fn fresh_tmp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("manifest_discovery_{label}_{nanos}"));
    fs::create_dir_all(&dir).expect("mkdir tmp");
    dir
}

fn write_manifest(dir: &std::path::Path, filename: &str, body: &str) -> PathBuf {
    let path = dir.join(filename);
    fs::write(&path, body).expect("write manifest");
    path
}

// ════════════════════════════════════════════════════════════════════
//  Discovery — `./` and `./schemas/`
// ════════════════════════════════════════════════════════════════════

#[test]
fn discovery_finds_axon_schema_files_in_root_and_schemas_subdir() {
    let dir = fresh_tmp_dir("disco_basic");
    let schemas = dir.join("schemas");
    fs::create_dir(&schemas).unwrap();

    let body = r#"{"version":1,"stores":{"tenants":{"columns":{"id":{"type":"Uuid"}}}}}"#;
    let p_root = write_manifest(&dir, "tenants.axon-schema.json", body);
    let p_sub = write_manifest(&schemas, "claims.axon-schema.json", body);
    // A file that doesn't match the discovery filter is ignored.
    write_manifest(&dir, "README.md", "not a manifest");
    write_manifest(&dir, "scratch.json", body);

    let found = discover_manifest_files(&dir);
    assert!(found.contains(&p_root), "root manifest must be found");
    assert!(found.contains(&p_sub), "schemas/ manifest must be found");
    assert_eq!(found.len(), 2, "only `*.axon-schema.json` files count");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn discovery_returns_sorted_paths_for_determinism() {
    let dir = fresh_tmp_dir("disco_sort");
    let body = r#"{"version":1,"stores":{"x":{"columns":{"id":{"type":"Uuid"}}}}}"#;
    write_manifest(&dir, "zeta.axon-schema.json", body);
    write_manifest(&dir, "alpha.axon-schema.json", body);
    write_manifest(&dir, "mike.axon-schema.json", body);

    let found = discover_manifest_files(&dir);
    let names: Vec<String> = found
        .iter()
        .filter_map(|p| p.file_name().and_then(|s| s.to_str()).map(|s| s.to_string()))
        .collect();
    assert_eq!(
        names,
        vec![
            "alpha.axon-schema.json",
            "mike.axon-schema.json",
            "zeta.axon-schema.json"
        ],
        "discovery order must be deterministic (alphabetic)"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn discovery_returns_empty_when_no_manifests_present() {
    let dir = fresh_tmp_dir("disco_empty");
    let found = discover_manifest_files(&dir);
    assert!(found.is_empty(), "empty directory yields no manifests");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn discovery_tolerates_a_missing_schemas_subdir() {
    let dir = fresh_tmp_dir("disco_no_sub");
    let body = r#"{"version":1,"stores":{"t":{"columns":{"id":{"type":"Uuid"}}}}}"#;
    write_manifest(&dir, "t.axon-schema.json", body);
    let found = discover_manifest_files(&dir);
    assert_eq!(found.len(), 1, "absent schemas/ subdir is not an error");
    fs::remove_dir_all(&dir).ok();
}

// ════════════════════════════════════════════════════════════════════
//  Multi-file merge + duplicate detection
// ════════════════════════════════════════════════════════════════════

#[test]
fn merge_combines_disjoint_manifests_into_one_view() {
    let dir = fresh_tmp_dir("merge_disjoint");
    write_manifest(
        &dir,
        "tenants.axon-schema.json",
        r#"{"version":1,"stores":{"tenants":{"columns":{"id":{"type":"Uuid"}}}}}"#,
    );
    write_manifest(
        &dir,
        "claims.axon-schema.json",
        r#"{"version":1,"stores":{"claims":{"columns":{"claim_id":{"type":"Uuid","primary_key":true}}}}}"#,
    );

    let merged = load_and_merge_manifests(&dir).expect("merge");
    assert!(merged.contains("tenants"));
    assert!(merged.contains("claims"));
    let claims = merged.lookup("claims").unwrap();
    let claim_id = claims.columns.get("claim_id").unwrap();
    assert_eq!(claim_id.col_type, StoreColumnType::Uuid);
    assert!(claim_id.primary_key);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn merge_rejects_duplicate_store_names_across_files() {
    let dir = fresh_tmp_dir("merge_dup");
    let body = r#"{"version":1,"stores":{"tenants":{"columns":{"id":{"type":"Uuid"}}}}}"#;
    let p_a = write_manifest(&dir, "a.axon-schema.json", body);
    let p_b = write_manifest(&dir, "b.axon-schema.json", body);

    let err = load_and_merge_manifests(&dir).expect_err("duplicate must reject");
    match err {
        ManifestError::DuplicateStore { store, path_a, path_b } => {
            assert_eq!(store, "tenants");
            // Discovery returns sorted paths, so `a.axon-schema.json`
            // is seen first and becomes `path_a`.
            assert_eq!(path_a, p_a);
            assert_eq!(path_b, p_b);
        }
        other => panic!("expected DuplicateStore, got {other:?}"),
    }

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn merge_verifies_content_hash_per_file_before_merging() {
    // A manifest with an explicit content_hash that's wrong must fail
    // the merge — the per-file verification runs BEFORE combination,
    // so a tampered file never contaminates the merged view.
    let dir = fresh_tmp_dir("merge_bad_hash");
    let bad = r#"{
        "version": 1,
        "stores": { "t": { "columns": { "id": { "type": "Uuid" } } } },
        "content_hash": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
    }"#;
    write_manifest(&dir, "bad.axon-schema.json", bad);

    let err = load_and_merge_manifests(&dir).expect_err("bad hash rejects");
    assert!(matches!(err, ManifestError::ContentHashMismatch { .. }));

    fs::remove_dir_all(&dir).ok();
}

// ════════════════════════════════════════════════════════════════════
// Introspection-CLI round-trip — what v1.31.0 will produce
// ════════════════════════════════════════════════════════════════════

#[test]
fn programmatic_construct_then_serialize_then_reparse_is_byte_identical() {
    // The shape `axon store introspect` (v1.31.0) produces: build a
    // Manifest programmatically from introspected live columns,
    // refresh the content hash, canonical-serialize, write. THIS
    // test exercises the producer side without a CLI.
    let mut m = Manifest::new();
    let mut tenants_cols = BTreeMap::new();
    tenants_cols.insert(
        "tenant_id".to_string(),
        ManifestColumn {
            col_type: StoreColumnType::Uuid,
            primary_key: true,
            auto_increment: false,
            not_null: false,
            unique: false,
            default_value: String::new(),
            identity: false,
        },
    );
    tenants_cols.insert(
        "tier".to_string(),
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
    tenants_cols.insert(
        "created_at".to_string(),
        ManifestColumn {
            col_type: StoreColumnType::Timestamptz,
            primary_key: false,
            auto_increment: false,
            not_null: true,
            unique: false,
            default_value: "now()".to_string(),
            identity: false,
        },
    );
    m.stores.insert(
        "public.tenants".to_string(),
        ManifestStore { columns: tenants_cols },
    );
    m.refresh_content_hash();

    let canonical = m.canonical_serialize(true);
    // Round-trip: parse + verify hash + canonical-serialize again.
    let parsed = Manifest::parse_json(&canonical).expect("reparse");
    parsed.verify_content_hash().expect("hash verifies after round-trip");
    let canonical_again = parsed.canonical_serialize(true);
    assert_eq!(
        canonical, canonical_again,
        "canonical form must round-trip byte-identically through \
         parse + verify + canonical-serialize"
    );

    // The store + columns survive the round-trip.
    let s = parsed.lookup("public.tenants").unwrap();
    assert_eq!(s.columns.len(), 3);
    assert!(s.columns.get("tenant_id").unwrap().primary_key);
    assert_eq!(
        s.columns.get("created_at").unwrap().default_value,
        "now()"
    );
}
