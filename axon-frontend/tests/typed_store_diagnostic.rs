//! v1.31.0 — Diagnostic anchor for the Declared & Compile-Time-
//! Typed Store Schema cycle.
//!
//! 5 -assertions pin the post-37.x state v1.31.0 inverts:
//!
//! - section 1 (38-A) — a `where:` clause references a column that does NOT
//!    exist in the declared schema. The type-checker currently passes
//!    it. 38.d's `StoreColumnProof` pass inverts this (`axon-T801`
//!    unknown column with Levenshtein "Did you mean X?" hint).
//!
//! - section 2 (38-B) — a `where:` value's type does NOT match the declared
//!    column type (a `String` parameter against a `Uuid` column). The
//!    type-checker currently passes it. 38.d's type-mismatch arm
//!    inverts (`axon-T802`).
//!
//! - section 3 (38-C) — a `persist` field-block references a column that
//!    does NOT exist in the declared schema. The type-checker currently
//!    passes it. 38.e's field-name proof inverts (`axon-T804`).
//!
//! - section 4 — the `schema:` parser surface is PRESENT (38.b shipped it).
//! This -assertion is a REGRESSION GUARD — every later step
//!    must keep parser parity for the three closed declaration forms.
//!
//! - section 5 — the `axon store introspect` CLI is ABSENT. 38.h adds it.
//! This -assertion currently observes the absence; 38.h flips it
//!    to a presence-guard.
//!
//! Honest-scope correction (mirror of 37.x.a): v1.31.0 is a *compile-
//! time* cycle, not a runtime-bug-reproduction cycle. The "broken"
//! state section 1-section 3 pin is "axon check passes through a typo'd store
//! reference"; the bug surfaces ONLY when the operation runs against
//! the live database. So section 1-section 3 cannot use a runtime smoke — they
//! exercise the type-checker directly, which is the exact surface
//! 38.d/e inverts.

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::IRStoreColumnSchema;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn check_passes(src: &str) -> bool {
    let tokens = Lexer::new(src, "anchor.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    let errors = TypeChecker::new(&program).check();
    errors.is_empty()
}

fn check_errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "anchor.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

// ════════════════════════════════════════════════════════════════════
// section 1 — Finding 38-A: column-name typo in a `where:` clause currently
//       passes `axon check`. (`tenantid` ≠ the declared `tenant_id`.)
//
//       38.d's `StoreColumnProof` pass MUST invert this — surface an
//       `axon-T801` error naming the unknown column and offering a
//       Levenshtein suggestion.
// ════════════════════════════════════════════════════════════════════

#[test]
fn s1_column_name_typo_in_where_is_rejected_with_axon_t801_and_levenshtein_hint() {
    // section 1 INVERTED in place by 38.d's `StoreColumnProof::check_filter`
    // pass. The column reference `tenantid` (no underscore) does NOT
    // exist in the declared schema; axon check now rejects with
    // axon-T801 + a Levenshtein "Did you mean `tenant_id`?" hint.
    let src = r#"
        axonstore tenants {
            backend: postgresql
            connection: "env:DB"
            schema {
                tenant_id: Uuid primary_key
                tier:      Text not_null
            }
        }

        flow LookupTenant(tenant_id: String) -> Unit {
            retrieve tenants { where: "tenantid = ${tenant_id}" as: result }
        }
    "#;
    let errs = check_errors(src);
    assert!(
        errs.iter().any(|m| m.contains("axon-T801")),
        "section 1 INVERTED: a column-name typo must surface axon-T801. \
         Errors observed: {errs:?}"
    );
    assert!(
        errs.iter().any(|m| m.contains("tenantid")),
        "section 1: axon-T801 must name the offending typo `tenantid`. \
         Errors observed: {errs:?}"
    );
    assert!(
        errs.iter().any(|m| m.contains("tenant_id")),
        "section 1: axon-T801 must surface the Levenshtein-closest column \
         `tenant_id` as the suggestion. Errors observed: {errs:?}"
    );
}

// ════════════════════════════════════════════════════════════════════
// section 2 — Finding 38-B: a `where:`-value's type does NOT match the
//       declared column type. The flow parameter `tenant_id: Int` is
//       bound into `where: "tenant_id = ${tenant_id}"` against a
//       `tenant_id: Uuid` column. The current type-checker passes it.
//
//       38.d's type-mismatch arm MUST invert this — surface an
//       `axon-T802` error naming the parameter type, the column type,
//       and the actionable remedy (align the parameter type, change
//       the column type, or convert at the boundary).
// ════════════════════════════════════════════════════════════════════

#[test]
fn s2_where_value_type_mismatch_is_rejected_with_axon_t802() {
    // section 2 INVERTED in place by 38.d's `StoreColumnProof::check_filter`
    // pass. The flow parameter type `Int` is not compatible with a
    // `Uuid` column per the closed compat matrix; axon check now
    // rejects with axon-T802 + names the parameter, its type, the
    // column, and the column's type.
    let src = r#"
        axonstore tenants {
            backend: postgresql
            connection: "env:DB"
            schema {
                tenant_id: Uuid primary_key
                tier: Text
            }
        }

        flow LookupTenant(tenant_id: Int) -> Unit {
            retrieve tenants { where: "tenant_id = ${tenant_id}" as: result }
        }
    "#;
    let errs = check_errors(src);
    assert!(
        errs.iter().any(|m| m.contains("axon-T802")),
        "section 2 INVERTED: an Int → Uuid binding must surface axon-T802. \
         Errors observed: {errs:?}"
    );
    assert!(
        errs.iter().any(|m| m.contains("tenant_id") && m.contains("Int") && m.contains("Uuid")),
        "section 2: axon-T802 must name the parameter, its declared type \
         `Int`, and the column type `Uuid`. Errors observed: {errs:?}"
    );
    // Reference `check_passes` so the helper stays exercised + the
    // import warning stays quiet (the helper now serves other -tests).
    let _: fn(&str) -> bool = check_passes;
}

// ════════════════════════════════════════════════════════════════════
// section 3 — Finding 38-C: a `persist` field-block references a column that
//       does NOT exist in the declared schema. The current type-checker
//       passes it; 37.x D8 catches it ONLY when a live database is
//       reachable at deploy time. Offline CI without a database has no
//       defense.
//
//       38.e's `StoreColumnProof` extension MUST invert this — surface
//       an `axon-T804` error at compile time, no DB required.
// ════════════════════════════════════════════════════════════════════

#[test]
fn s3_persist_field_typo_is_rejected_with_axon_t804() {
    // section 3 INVERTED in place by 38.e's `check_persist_fields` pass. The
    // field name `tenantid` (no underscore) is a typo — the declared
    // column is `tenant_id`. axon check now rejects with axon-T804 +
    // a Levenshtein "Did you mean `tenant_id`?" hint at compile time,
    // no live database required (the offline-checkable gap 37.x D8
    // alone couldn't close).
    let src = r#"
        axonstore tenants {
            backend: postgresql
            connection: "env:DB"
            schema {
                tenant_id: Uuid primary_key
                tier: Text
            }
        }

        flow OnboardTenant(tenant_id: String, tier: String) -> Unit {
            persist into tenants { tenantid: "${tenant_id}" tier: "${tier}" }
        }
    "#;
    let errs = check_errors(src);
    assert!(
        errs.iter().any(|m| m.contains("axon-T804")),
        "section 3 INVERTED: a persist field-name typo must surface axon-T804. \
         Errors observed: {errs:?}"
    );
    assert!(
        errs.iter().any(|m| m.contains("tenantid")),
        "section 3: axon-T804 must name the offending typo `tenantid`. \
         Errors observed: {errs:?}"
    );
    assert!(
        errs.iter().any(|m| m.contains("tenant_id")),
        "section 3: axon-T804 must surface the Levenshtein-closest column \
         `tenant_id` as the suggestion. Errors observed: {errs:?}"
    );
    let _: fn(&str) -> bool = check_passes;
}

// ════════════════════════════════════════════════════════════════════
// section 4 — `schema:` parser surface is PRESENT (38.b shipped it). This
// -assertion is a REGRESSION GUARD — every later step must
//       keep parser parity for the three closed declaration forms.
//
//       A breakage here means a parser regression (someone removed or
//       broke 38.b's grammar surface). The IR's `column_schema` MUST
//       remain populated by the three forms.
// ════════════════════════════════════════════════════════════════════

#[test]
fn s4_schema_parser_surface_remains_live_across_the_three_closed_forms() {
    for (src, expected_form) in [
        (
            r#"
                axonstore s {
                    backend: postgresql
                    connection: "env:X"
                    schema { id: Uuid primary_key }
                }
            "#,
            "inline",
        ),
        (
            r#"
                axonstore s {
                    backend: postgresql
                    connection: "env:X"
                    schema: "public.s"
                }
            "#,
            "manifest_ref",
        ),
        (
            r#"
                axonstore s {
                    backend: postgresql
                    connection: "env:X"
                    schema: env:NAMESPACE
                }
            "#,
            "env_var",
        ),
    ] {
        let tokens = Lexer::new(src, "anchor.axon").tokenize().expect("lex");
        let program = Parser::new(tokens).parse().expect(
            "section 4 REGRESSION GUARD: 38.b's `schema:` parser must keep \
             parsing the three closed forms. A parse failure here is \
             a 38.b regression.",
        );
        let ir = IRGenerator::new().generate(&program);
        let store = ir
            .axonstore_specs
            .first()
            .expect("section 4 REGRESSION GUARD: the corpus must lower to one IRAxonStore");
        let schema = store.column_schema.as_ref().expect(
            "section 4 REGRESSION GUARD: the IR must populate column_schema for \
             every declared schema:; missing here is a 38.b regression.",
        );
        let observed_form = match schema {
            IRStoreColumnSchema::Inline { .. } => "inline",
            IRStoreColumnSchema::ManifestRef { .. } => "manifest_ref",
            IRStoreColumnSchema::EnvVar { .. } => "env_var",
        };
        assert_eq!(
            observed_form, expected_form,
            "section 4 REGRESSION GUARD: form discriminator mismatch on input \
             {src:?}"
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// section 5 — `axon store introspect` CLI PRESENT (38.h shipped it). The
// -assertion is INVERTED in place: was "module absent"; now
//       a REGRESSION GUARD for the pure module's public API surface
//       + the omission-tracking contract. A breakage here flags a
//       38.h regression on the manifest-export contract.
// ════════════════════════════════════════════════════════════════════

#[test]
fn s5_axon_store_introspect_frontend_surface_is_live_with_pure_api() {
    // section 5 INVERTED in place by 38.h. The pure-Rust
    // `axon_frontend::store_introspect` module is now live with the
    // public surface adopter tooling builds on:
    //
    //   - `udt_to_canonical_type(pg_udt) -> Option<StoreColumnType>`
    //   - `build_manifest_store(rows) -> (ManifestStore, Vec<OmittedColumn>)`
    //   - `detect_auto_increment(default_expr) -> bool`
    //   - `manifest_diff(old, new) -> ManifestDiff`
    //   - `format_manifest_diff(diff) -> String`
    //   - `IntrospectionRow` / `OmittedColumn` / `ColumnDelta` /
    //     `ManifestDiff` data types
    //
    // A regression here (the module is gone, an entry point renamed,
    // the omission contract loosened) fails this test — protecting
    // adopter CLI tooling that consumes the public API.
    use axon_frontend::store_introspect::{
        build_manifest_store, detect_auto_increment, format_manifest_diff,
        manifest_diff, udt_to_canonical_type, IntrospectionRow,
        ManifestDiff, OmittedColumn,
    };
    use axon_frontend::store_schema::StoreColumnType;
    use axon_frontend::store_schema_manifest::Manifest;

    // — udt_to_canonical_type covers the closed catalog. —
    assert_eq!(udt_to_canonical_type("uuid"), Some(StoreColumnType::Uuid));
    assert_eq!(udt_to_canonical_type("text"), Some(StoreColumnType::Text));
    assert_eq!(udt_to_canonical_type("geometry"), None);

    // — detect_auto_increment recognises `nextval(...)`. —
    assert!(detect_auto_increment("nextval('users_id_seq'::regclass)"));
    assert!(!detect_auto_increment("'standard'::text"));

    // — build_manifest_store maps known UDTs + tracks omissions. —
    let rows = vec![
        IntrospectionRow {
            column_name: "id".into(),
            pg_udt: "uuid".into(),
            not_null: true,
            primary_key: true,
            unique: false,
            default_expression: String::new(),
            identity_kind: None,
        },
        IntrospectionRow {
            column_name: "tier".into(),
            pg_udt: "tier_enum".into(), // outside the catalog
            not_null: true,
            primary_key: false,
            unique: false,
            default_expression: String::new(),
            identity_kind: None,
        },
    ];
    let (store, omissions) = build_manifest_store(&rows);
    assert_eq!(store.columns.len(), 1, "only `id` survives");
    assert_eq!(omissions.len(), 1);
    assert_eq!(omissions[0].name, "tier");
    assert!(
        omissions[0].reason.contains("closed type catalog"),
        "honest-omission reason must name the catalog"
    );
    // The OmittedColumn render is stable.
    assert!(omissions[0].as_comment_line().starts_with("# omitted: "));

    // — manifest_diff + format_manifest_diff round-trip. —
    let old = Manifest::parse_json(
        r#"{"version":1,"stores":{"t":{"columns":{"id":{"type":"Int"}}}}}"#,
    )
    .unwrap();
    let new = Manifest::parse_json(
        r#"{"version":1,"stores":{"t":{"columns":{"id":{"type":"Uuid"}}}}}"#,
    )
    .unwrap();
    let diff: ManifestDiff = manifest_diff(&old, &new);
    assert!(!diff.is_empty());
    let summary = format_manifest_diff(&diff);
    assert!(summary.contains("Int → Uuid"));

    // The -assertion is a REGRESSION GUARD now — the module + its
    // public surface MUST stay alive for the rest of the v1.31.0
    // cycle (38.j CI lane + 38.k release) + every future cycle that
    // consumes the manifest format.
}
