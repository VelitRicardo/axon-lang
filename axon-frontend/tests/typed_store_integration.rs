//! v1.31.0 — Integration tests: end-to-end exercise of the typed
//! store schema cycle (38.b → 38.h).
//!
//! This pack composes the full v1.31.0 surface and asserts adopter-
//! observable behavior at the LIBRARY level (no CLI shell — see
//! `axon-rs/tests/introspect_integration.rs` for the
//! sqlx-backed CLI integration):
//!
//!   - **38.b grammar** — multi-store source with mixed inline /
//!     manifest_ref / env_var declarations parses cleanly.
//!   - **38.c manifest** — the 5-tenant curated fixture parses, its
//!     stores are reachable by qualified-name lookup.
//!   - **38.d / 38.e proof** — every D2 error code (T801, T802,
//!     T803, T804) fires for the right input; every good case
//!     passes silently.
//!   - **38.g composite suggestion** — the messages an adopter
//!     reads carry both the column name AND its declared type.
//!   - **38.h pure surface** — the introspection module's public
//! API stays usable as a regression guard alongside the section 5
//!     anchor.
//!
//! The "axon check against the curated 5-tenant manifest" surface
//! the plan-vivo v1.31.0 row mentions is exercised here: load the
//! manifest, lookup each tenant's entry, assert the closed-catalog
//! types + constraints survive the parse round-trip.
//!
//! Forms (b)/(c) at COMPILE time still silently skip per the
//! 38.d/38.e honest scope — their deploy-time gate is 38.f's
//! `verify_postgres_schemas_with_manifest`, exercised in
//! `axon-rs/src/store/registry.rs`'s own tests. THIS file therefore
//! exercises forms (b)/(c) only at the grammar + manifest levels —
//! the parser/IR shape is preserved, the manifest is consultable.

use std::path::PathBuf;

use axon_frontend::ir_nodes::IRStoreColumnSchema;
use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::store_schema::StoreColumnType;
use axon_frontend::store_schema_manifest::Manifest;
use axon_frontend::type_checker::{TypeChecker, TypeError};

/// Lex + parse + type-check the source, returning the error messages.
fn check_errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "typed_store_manifest.axon")
        .tokenize()
        .expect("lex must not fail");
    let program = Parser::new(tokens).parse().expect("parse must not fail");
    TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e: TypeError| e.message)
        .collect()
}

fn check_passes(src: &str) -> bool {
    check_errors(src).is_empty()
}

/// Locate `<repo-root>/tests/fixtures/typed_store_manifest/curated_manifest.axon-schema.json`
/// from a `cargo test` run in `axon-frontend/`.
fn curated_manifest_path() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .expect("workspace root")
        .join("tests")
        .join("fixtures")
        .join("typed_store_manifest")
        .join("curated_manifest.axon-schema.json")
}

fn load_curated_manifest() -> Manifest {
    let path = curated_manifest_path();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read curated manifest {}: {e}", path.display()));
    Manifest::parse_json(&text).expect("curated manifest must parse")
}

// ════════════════════════════════════════════════════════════════════
//  Section A — Curated 5-tenant manifest survives the parse contract
// ════════════════════════════════════════════════════════════════════

#[test]
fn curated_manifest_parses_with_five_stores_keyed_by_qualified_name() {
    let m = load_curated_manifest();
    assert_eq!(m.version, 1);
    assert_eq!(m.stores.len(), 5, "the curated fixture has five stores");

    // Each qualified-name shape is reachable.
    for key in [
        "audit.log",
        "public.events",
        "public.tenants",
        "tenant_alpha.usage",
        "tenant_beta.usage",
    ] {
        assert!(
            m.contains(key),
            "qualified-name `{key}` must be in the curated manifest"
        );
    }
}

#[test]
fn curated_manifest_columns_carry_the_closed_catalog_types() {
    let m = load_curated_manifest();
    let tenants = m.lookup("public.tenants").expect("tenants store");
    let tenant_id = tenants.columns.get("tenant_id").expect("tenant_id column");
    assert_eq!(tenant_id.col_type, StoreColumnType::Uuid);
    assert!(tenant_id.primary_key);
    assert!(tenant_id.not_null);

    let tier = tenants.columns.get("tier").expect("tier column");
    assert_eq!(tier.col_type, StoreColumnType::Text);
    assert!(tier.not_null);
    assert!(tier.default_value.contains("standard"));

    let usage = m.lookup("tenant_alpha.usage").expect("alpha usage store");
    let event_count = usage.columns.get("event_count").unwrap();
    assert_eq!(event_count.col_type, StoreColumnType::BigInt);
    assert!(event_count.not_null);
    assert_eq!(event_count.default_value, "0");
}

#[test]
fn curated_manifest_per_tenant_stores_share_the_same_column_shape() {
    // Per-tenant schemas typically have identical column shapes —
    // the curated fixture mirrors that real-world adopter pattern.
    // The 38.d/38.f first-match heuristic relies on this assumption.
    let m = load_curated_manifest();
    let alpha = m.lookup("tenant_alpha.usage").expect("alpha");
    let beta = m.lookup("tenant_beta.usage").expect("beta");
    let alpha_cols: Vec<(&String, &StoreColumnType)> = alpha
        .columns
        .iter()
        .map(|(n, c)| (n, &c.col_type))
        .collect();
    let beta_cols: Vec<(&String, &StoreColumnType)> = beta
        .columns
        .iter()
        .map(|(n, c)| (n, &c.col_type))
        .collect();
    assert_eq!(
        alpha_cols, beta_cols,
        "per-tenant stores must share identical column shapes for the \
         first-match heuristic to be a defensible policy"
    );
}

#[test]
fn curated_manifest_canonical_round_trip_preserves_content_hash() {
    let m = load_curated_manifest();
    let canonical = m.canonical_serialize(false);
    let m2 = Manifest::parse_json(&canonical).expect("re-parse canonical");
    assert_eq!(m2.compute_content_hash(), m.compute_content_hash());
    assert_eq!(m, m2, "canonical round-trip must preserve every column");
}

// ════════════════════════════════════════════════════════════════════
//  Section B — Mixed inline + manifest-ref + env-var declarations
// ════════════════════════════════════════════════════════════════════

const MIXED_SOURCE: &str = r#"
    axonstore tenants {
        backend: postgresql
        connection: "env:DATABASE_URL"
        schema {
            tenant_id: Uuid primary_key
            tier:      Text not_null
        }
    }

    axonstore events {
        backend: postgresql
        connection: "env:DATABASE_URL"
        schema: "public.events"
    }

    axonstore usage {
        backend: postgresql
        connection: "env:DATABASE_URL"
        schema: env:TENANT_SCHEMA
    }
"#;

#[test]
fn multi_store_source_with_three_schema_forms_parses_cleanly() {
    let tokens = Lexer::new(MIXED_SOURCE, "mixed.axon").tokenize().unwrap();
    let program = Parser::new(tokens).parse().expect("multi-store source must parse");
    let ir = IRGenerator::new().generate(&program);
    assert_eq!(ir.axonstore_specs.len(), 3);

    // Each store's column_schema is the right variant.
    let by_name: std::collections::HashMap<&str, &IRStoreColumnSchema> = ir
        .axonstore_specs
        .iter()
        .filter_map(|s| s.column_schema.as_ref().map(|c| (s.name.as_str(), c)))
        .collect();

    match by_name.get("tenants") {
        Some(IRStoreColumnSchema::Inline { columns }) => {
            assert_eq!(columns.len(), 2);
            assert_eq!(columns[0].name, "tenant_id");
            assert_eq!(columns[0].col_type, "Uuid");
        }
        other => panic!("tenants must be Inline, got {other:?}"),
    }
    match by_name.get("events") {
        Some(IRStoreColumnSchema::ManifestRef { qualified_name }) => {
            assert_eq!(qualified_name, "public.events");
        }
        other => panic!("events must be ManifestRef, got {other:?}"),
    }
    match by_name.get("usage") {
        Some(IRStoreColumnSchema::EnvVar { var_name }) => {
            assert_eq!(var_name, "TENANT_SCHEMA");
        }
        other => panic!("usage must be EnvVar, got {other:?}"),
    }
}

#[test]
fn multi_store_source_passes_axon_check_when_flow_only_touches_proven_inline_store() {
    // The mixed source declares three stores. The flow below only
    // references the INLINE-declared `tenants`. Forms (b)/(c) silently
    // skip per 38.d honest scope — no false positives at compile time.
    let src = format!(
        "{MIXED_SOURCE}\n\
         flow LookupTenant(tenant_id: Uuid) -> Unit {{\n\
             retrieve tenants {{ where: \"tenant_id = ${{tenant_id}}\" as: result }}\n\
         }}\n"
    );
    assert!(
        check_passes(&src),
        "well-formed mixed source must pass axon check, errors: {:?}",
        check_errors(&src)
    );
}

#[test]
fn multi_store_source_flags_inline_proof_failure_only() {
    // Type the flow parameter incorrectly — INLINE store's proof
    // fires (T802). The manifest_ref and env_var stores are silent
    // at compile time (honest scope).
    let src = format!(
        "{MIXED_SOURCE}\n\
         flow LookupTenant(some_int: Int) -> Unit {{\n\
             retrieve tenants {{ where: \"tenant_id = ${{some_int}}\" as: result }}\n\
         }}\n"
    );
    let errs = check_errors(&src);
    assert!(errs.iter().any(|m| m.contains("axon-T802")));
    // Only ONE T802 — manifest_ref/env_var stores didn't fire.
    let t802_count = errs.iter().filter(|m| m.contains("axon-T802")).count();
    assert_eq!(t802_count, 1, "exactly one T802 from the inline store");
}

// ════════════════════════════════════════════════════════════════════
//  Section C — Every D2 error code (T801, T802, T803, T804) fires
// ════════════════════════════════════════════════════════════════════

const INLINE_SCHEMA_FIXTURE: &str = r#"
    axonstore tenants {
        backend: postgresql
        connection: "env:DATABASE_URL"
        schema {
            tenant_id:  Uuid primary_key
            tier:       Text not_null
            created_at: Timestamptz not_null default "now()"
        }
    }
"#;

#[test]
fn t801_fires_on_unknown_where_column() {
    let src = format!(
        "{INLINE_SCHEMA_FIXTURE}\n\
         flow F(tenant_id: Uuid) -> Unit {{\n\
             retrieve tenants {{ where: \"tenantid = ${{tenant_id}}\" }}\n\
         }}\n"
    );
    let errs = check_errors(&src);
    assert!(errs.iter().any(|m| m.contains("axon-T801")));
    // v1.31.0 composite — "Did you mean column `tenant_id` (Uuid)?" SHOULD appear.
    assert!(
        errs.iter().any(|m| m.contains("`tenant_id` (Uuid)")),
        "composite suggestion must include the type: {errs:?}"
    );
}

#[test]
fn t802_fires_on_param_type_mismatch_in_where() {
    let src = format!(
        "{INLINE_SCHEMA_FIXTURE}\n\
         flow F(flag: Bool) -> Unit {{\n\
             retrieve tenants {{ where: \"tenant_id = ${{flag}}\" }}\n\
         }}\n"
    );
    let errs = check_errors(&src);
    assert!(errs.iter().any(|m| m.contains("axon-T802")));
    // T802 message renders the parameter with the source-level `${name}`
    // shape (curly braces preserved) — assert the bare identifier
    // appears somewhere in the message.
    assert!(errs.iter().any(|m| m.contains("flag")));
    assert!(errs.iter().any(|m| m.contains("Uuid")));
}

#[test]
fn t802_literal_mismatch_in_where_surfaces_compat_hint() {
    let src = format!(
        "{INLINE_SCHEMA_FIXTURE}\n\
         flow F() -> Unit {{\n\
             retrieve tenants {{ where: \"tier = 42\" }}\n\
         }}\n"
    );
    let errs = check_errors(&src);
    assert!(errs.iter().any(|m| m.contains("axon-T802")));
    // No Int-class compatible column in `tenants` — schema has no Int.
    // The compat hint should be ABSENT (no false suggestion).
    let t802_msg = errs.iter().find(|m| m.contains("axon-T802")).unwrap();
    assert!(
        !t802_msg.contains("Compatible Int-class"),
        "no compat hint when no alternative exists: {t802_msg}"
    );
}

#[test]
fn t803_fires_on_persist_not_null_omission() {
    let src = format!(
        "{INLINE_SCHEMA_FIXTURE}\n\
         flow F(tenant_id: Uuid) -> Unit {{\n\
             persist into tenants {{ tenant_id: \"${{tenant_id}}\" }}\n\
         }}\n"
    );
    let errs = check_errors(&src);
    assert!(
        errs.iter().any(|m| m.contains("axon-T803") && m.contains("`tier`")),
        "T803 must name the omitted NOT-NULL column `tier`: {errs:?}"
    );
    // `created_at` has a default → NOT omitted from the check.
    assert!(
        !errs.iter().any(|m| m.contains("`created_at`") && m.contains("axon-T803")),
        "T803 must NOT fire for a column with a default"
    );
}

#[test]
fn t804_fires_on_persist_field_name_typo() {
    let src = format!(
        "{INLINE_SCHEMA_FIXTURE}\n\
         flow F(tenant_id: Uuid, tier: Text) -> Unit {{\n\
             persist into tenants {{ \
                 tenantid: \"${{tenant_id}}\" \
                 tier: \"${{tier}}\" \
             }}\n\
         }}\n"
    );
    let errs = check_errors(&src);
    assert!(errs.iter().any(|m| m.contains("axon-T804")));
    assert!(errs.iter().any(|m| m.contains("`tenantid`")));
    // v1.31.0 composite suggestion.
    assert!(errs.iter().any(|m| m.contains("`tenant_id` (Uuid)")));
}

#[test]
fn t804_fires_on_mutate_field_typo_no_t803_because_update_is_partial() {
    let src = format!(
        "{INLINE_SCHEMA_FIXTURE}\n\
         flow F(tenant_id: Uuid) -> Unit {{\n\
             mutate tenants {{ \
                 where: \"tenant_id = ${{tenant_id}}\" \
                 teir: \"premium\" \
             }}\n\
         }}\n"
    );
    let errs = check_errors(&src);
    assert!(errs.iter().any(|m| m.contains("axon-T804") && m.contains("`teir`")));
    assert!(
        !errs.iter().any(|m| m.contains("axon-T803")),
        "mutate must NOT emit T803 — UPDATE preserves omitted columns: {errs:?}"
    );
}

#[test]
fn happy_path_passes_with_zero_errors() {
    let src = format!(
        "{INLINE_SCHEMA_FIXTURE}\n\
         flow OnboardTenant(tenant_id: Uuid, tier: Text) -> Unit {{\n\
             persist into tenants {{ \
                 tenant_id: \"${{tenant_id}}\" \
                 tier: \"${{tier}}\" \
             }}\n\
         }}\n"
    );
    assert!(check_passes(&src), "errors: {:?}", check_errors(&src));
}

#[test]
fn happy_path_retrieve_uuid_via_canonical_string_literal() {
    let src = format!(
        "{INLINE_SCHEMA_FIXTURE}\n\
         flow LookupOne() -> Unit {{\n\
             retrieve tenants {{ where: \"tenant_id = '83d078e1-b372-42ba-9572-ff8dc521386e'\" }}\n\
         }}\n"
    );
    assert!(check_passes(&src), "errors: {:?}", check_errors(&src));
}

#[test]
fn happy_path_purge_with_where_only_no_field_block() {
    let src = format!(
        "{INLINE_SCHEMA_FIXTURE}\n\
         flow CleanupOld() -> Unit {{\n\
             purge tenants {{ where: \"tier = 'archived'\" }}\n\
         }}\n"
    );
    assert!(check_passes(&src), "errors: {:?}", check_errors(&src));
}

// ════════════════════════════════════════════════════════════════════
//  Section D — D5 absolute backwards-compatibility
// ════════════════════════════════════════════════════════════════════

#[test]
fn d5_undeclared_schema_skips_the_proof_verbatim() {
    // No `schema:` declaration → 37.x runtime+deploy path applies
    // verbatim; the type-checker emits no axon-T8xx errors even when
    // a column reference is "wrong" (it can't know).
    let src = r#"
        axonstore tenants {
            backend: postgresql
            connection: "env:DATABASE_URL"
        }

        flow F(p: Int) -> Unit {
            retrieve tenants { where: "anything_goes = ${p}" }
        }
    "#;
    let errs = check_errors(src);
    assert!(
        !errs.iter().any(|m| m.starts_with("axon-T8")),
        "D5 absolute — no schema means no T8xx errors. Got: {errs:?}"
    );
}

#[test]
fn d5_blockless_persist_against_inline_schema_skips_the_proof() {
    // The v1.30.0 blockless `persist <store>` form (user-bindings
    // fallback) is preserved — even when an inline schema IS declared,
    // an empty field-block bypasses T803/T804.
    let src = format!(
        "{INLINE_SCHEMA_FIXTURE}\n\
         flow F(tenant_id: Uuid, tier: Text) -> Unit {{\n\
             persist tenants\n\
         }}\n"
    );
    let errs = check_errors(&src);
    assert!(
        !errs.iter().any(|m| m.contains("axon-T8")),
        "blockless persist is the v1.30.0 fallback; D5 absolute. Got: {errs:?}"
    );
}

// ════════════════════════════════════════════════════════════════════
//  Section E — Levenshtein boundary integration (38.g composite hints)
// ════════════════════════════════════════════════════════════════════

#[test]
fn levenshtein_distance_two_typo_surfaces_composite_suggestion() {
    let src = format!(
        "{INLINE_SCHEMA_FIXTURE}\n\
         flow F() -> Unit {{\n\
             retrieve tenants {{ where: \"teir = 'standard'\" }}\n\
         }}\n"
    );
    let errs = check_errors(&src);
    let t801 = errs.iter().find(|m| m.contains("axon-T801")).expect("T801");
    // Edit-distance 2 from `tier` → composite suggestion fires.
    assert!(
        t801.contains("`tier` (Text)"),
        "composite suggestion must include type: {t801}"
    );
}

#[test]
fn levenshtein_out_of_distance_typo_suppresses_suggestion() {
    let src = format!(
        "{INLINE_SCHEMA_FIXTURE}\n\
         flow F() -> Unit {{\n\
             retrieve tenants {{ where: \"WildlyDifferent = 'x'\" }}\n\
         }}\n"
    );
    let errs = check_errors(&src);
    let t801 = errs.iter().find(|m| m.contains("axon-T801")).expect("T801");
    assert!(
        !t801.contains("Did you mean"),
        "no guess for an out-of-distance typo: {t801}"
    );
    // But the declared columns ARE listed (so the adopter can pick one).
    assert!(t801.contains("tenant_id: Uuid"));
    assert!(t801.contains("tier: Text"));
}

// ════════════════════════════════════════════════════════════════════
//  Section F — Spot-check the 38.h public API stays consumable
// ════════════════════════════════════════════════════════════════════

#[test]
fn store_introspect_public_surface_is_reachable() {
    // The section 5 anchor proves this AT THE TYPE LEVEL — this test is the
    // integration counterpart: every public symbol can be imported
    // + used from a downstream consumer crate (which is what the
    // axon CLI binary is). A future regression that hides any of
    // these symbols (e.g. demoting `pub` to `pub(crate)`) fails this
    // test.
    use axon_frontend::store_introspect::{
        build_manifest_store, detect_auto_increment, format_manifest_diff,
        manifest_diff, udt_to_canonical_type, IntrospectionRow,
        ManifestDiff, OmittedColumn,
    };

    assert_eq!(udt_to_canonical_type("uuid"), Some(StoreColumnType::Uuid));
    assert!(detect_auto_increment("nextval('seq')"));

    let rows = vec![IntrospectionRow {
        column_name: "id".into(),
        pg_udt: "uuid".into(),
        not_null: true,
        primary_key: true,
        unique: false,
        default_expression: String::new(),
        identity_kind: None,
    }];
    let (store, omissions) = build_manifest_store(&rows);
    assert_eq!(store.columns.len(), 1);
    assert!(omissions.is_empty());

    let _o = OmittedColumn {
        name: "x".into(),
        pg_udt: "geometry".into(),
        reason: "outside catalog".into(),
    };

    let diff: ManifestDiff = manifest_diff(&load_curated_manifest(), &load_curated_manifest());
    assert!(diff.is_empty());
    assert_eq!(format_manifest_diff(&diff), "");
}
