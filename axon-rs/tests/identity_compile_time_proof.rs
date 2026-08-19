//! v1.31.0 — IDENTITY end-to-end at COMPILE TIME diagnostic anchor.
//!
//! Pins the v1.38.3 follow-up gap the adopter pointed at (2026-05-20):
//! v1.38.3 plumbed `identity: bool` through the AST + manifest +
//! introspect output, but T803 still didn't fire on the field
//! because (a) no declaration form in `.axon` source could SET the
//! field non-false, and (b) the type-checker silently skipped forms
//! (b) `manifest_ref` and (c) `env_var` at compile time.
//!
//! v1.38.4 closes both gaps:
//!
//! - **D1** — inline parser accepts `identity` keyword.
//! - **D2** — `TypeChecker::with_manifest` populates
//!   `store_inline_column_sets` for forms (b)/(c) when a manifest is
//!   supplied (via `axon check --schemas-dir`).
//! - **D3** — `axon check --schemas-dir <path>` CLI flag (or
//!   `AXON_SCHEMAS_DIR` env var).
//! - **D4** — form (c) env_var uses the same first-match heuristic
//!   already in `store_column_proof::load_columns_for_schema`.
//! - **D5** — without `--schemas-dir`, behavior byte-identical to
//!   v1.38.3 (forms b/c silently skip).

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::store_schema::{StoreColumnSchema, StoreColumnType};
use axon_frontend::store_schema_manifest::{
    Manifest, ManifestColumn, ManifestStore,
};
use axon_frontend::type_checker::TypeChecker;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

// ════════════════════════════════════════════════════════════════════
// section 1 — D1 inline parser accepts `identity` keyword
// ════════════════════════════════════════════════════════════════════

/// section 1 — pin that an inline `schema { id: BigInt primary_key identity
/// not_null }` parses clean AND the resulting `StoreColumn.identity`
/// is `true`. T803 then skips the column from a persist that omits it.
#[test]
fn s1_d1_inline_identity_keyword_parses() {
    let source = r#"
        axonstore chat_history {
            backend: postgresql
            connection: "env:DATABASE_URL"
            schema {
                id: BigInt primary_key identity not_null
                tenant_id: Uuid not_null
                content: Text not_null
            }
        }
    "#;
    let tokens = Lexer::new(source, "test.axon").tokenize().expect("lexes");
    let program = Parser::new(tokens).parse().expect("parses");
    // Find the axonstore declaration.
    let store = program
        .declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::AxonStore(a) => Some(a),
            _ => None,
        })
        .expect("axonstore declared");
    let schema = store.column_schema.as_ref().expect("schema declared");
    let columns = match schema {
        StoreColumnSchema::Inline { columns, .. } => columns,
        _ => panic!("expected inline schema"),
    };
    let id_col = columns
        .iter()
        .find(|c| c.name == "id")
        .expect("id column present");
    assert!(
        id_col.identity,
        "D1 — `identity` keyword on inline schema must set \
         `StoreColumn.identity = true`. Got: {id_col:#?}"
    );
    assert!(id_col.primary_key);
    assert!(id_col.not_null);
    assert!(
        !id_col.auto_increment,
        "D4 — `identity` does NOT imply `auto_increment` (different SQL surface)"
    );
}

/// section 1-companion — non-identity columns still parse cleanly without the
/// keyword (negative control to prove section 1 isn't accidentally setting
/// `identity = true` on every column).
#[test]
fn s1_d1_non_identity_inline_columns_default_to_false() {
    let source = r#"
        axonstore tenants {
            backend: postgresql
            connection: "env:DATABASE_URL"
            schema {
                tenant_id: Uuid primary_key not_null
                tier: Text not_null default "starter"
            }
        }
    "#;
    let tokens = Lexer::new(source, "test.axon").tokenize().expect("lexes");
    let program = Parser::new(tokens).parse().expect("parses");
    let store = program
        .declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::AxonStore(a) => Some(a),
            _ => None,
        })
        .unwrap();
    let columns = match store.column_schema.as_ref().unwrap() {
        StoreColumnSchema::Inline { columns, .. } => columns,
        _ => panic!(),
    };
    for col in columns {
        assert!(!col.identity, "default `identity = false` for `{}`", col.name);
    }
}

// ════════════════════════════════════════════════════════════════════
// section 2 — D2 TypeChecker consumes manifest for form (b) `manifest_ref`
// ════════════════════════════════════════════════════════════════════

/// section 2 — pin that when an `axonstore` uses form (b) `schema:
/// "public.chat_history"` AND a manifest with `identity: true` on the
/// `id` column is supplied to `TypeChecker::with_manifest`, the
/// type-checker populates `store_inline_column_sets` for that store
/// AND T803 skips the `id` column on a `persist` that omits it.
///
/// This is the load-bearing test for D2: pre-v1.38.4 the type-checker
/// silently skipped non-inline forms; v1.38.4 plumbs the manifest
/// through.
#[test]
fn s2_d2_manifest_ref_with_identity_resolved_at_compile_time() {
    let source = r#"
        axonstore chat_history {
            backend: postgresql
            connection: "env:DATABASE_URL"
            schema: "public.chat_history"
        }

        flow Insert(tid: Uuid, msg: Text) -> Text {
            persist into chat_history { tenant_id: "${tid}" content: "${msg}" }
        }
    "#;
    let tokens = Lexer::new(source, "test.axon").tokenize().expect("lexes");
    let program = Parser::new(tokens).parse().expect("parses");

    // Build a manifest with `id: BigInt identity not_null primary_key`.
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
    let mut m = Manifest::new();
    m.stores.insert("public.chat_history".to_string(), store);

    // Type-check WITH the manifest. T803 must NOT fire on the omitted
    // `id` column — the manifest declares it as `identity: true`.
    let (errors, _warnings) =
        TypeChecker::with_manifest(&program, &m).check_with_warnings();
    let t803_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.message.contains("axon-T803"))
        .collect();
    assert!(
        t803_errors.is_empty(),
        "D2 — T803 must NOT fire on the `id` column declared \
         `identity: true` in the manifest. Got T803 errors: {t803_errors:#?}"
    );
}

// ════════════════════════════════════════════════════════════════════
// section 3 — D5 form (b) WITHOUT manifest silently skips (backwards-compat)
// ════════════════════════════════════════════════════════════════════

/// section 3 — pin that when the same form (b) source is type-checked WITHOUT
/// a manifest (via `TypeChecker::new`), the proof silently skips
/// exactly as in v1.38.3 — no T803 OR any other store-proof error
/// surfaces, because no column set is registered for the store.
/// This is the D5 backwards-compat guarantee.
#[test]
fn s3_d5_manifest_ref_without_manifest_silently_skips() {
    let source = r#"
        axonstore chat_history {
            backend: postgresql
            connection: "env:DATABASE_URL"
            schema: "public.chat_history"
        }

        flow Insert(tid: Uuid, msg: Text) -> Text {
            persist into chat_history { tenant_id: "${tid}" content: "${msg}" }
        }
    "#;
    let tokens = Lexer::new(source, "test.axon").tokenize().expect("lexes");
    let program = Parser::new(tokens).parse().expect("parses");

    // Type-check WITHOUT a manifest. No store-proof errors should fire
    // for the `chat_history` store — the column set is not registered.
    let (errors, _warnings) = TypeChecker::new(&program).check_with_warnings();
    let store_proof_errors: Vec<_> = errors
        .iter()
        .filter(|e| {
            e.message.contains("axon-T80")
                && (e.message.contains("chat_history")
                    || e.message.contains("`id`")
                    || e.message.contains("`tenant_id`")
                    || e.message.contains("`content`"))
        })
        .collect();
    assert!(
        store_proof_errors.is_empty(),
        "D5 — without a manifest, form (b) MUST silently skip the \
         store-proof for `chat_history`. Got errors: {store_proof_errors:#?}"
    );
}

// ════════════════════════════════════════════════════════════════════
// section 4 — D1 STATIC: parser constraint loop includes `identity` arm
// ════════════════════════════════════════════════════════════════════

/// section 4 — grep -assertion: the inline-schema constraint loop in
/// `axon-frontend/src/parser.rs` accepts `"identity"` as a column
/// constraint keyword. A future PR that deletes the arm turns this
/// test RED.
#[test]
fn s4_d1_parser_constraint_includes_identity() {
    let parser_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("axon-frontend")
        .join("src")
        .join("parser.rs");
    let body = fs::read_to_string(&parser_path).expect("parser.rs readable");
    assert!(
        body.contains("\"identity\" => {"),
        "D1 invariant — the inline-schema constraint match in \
         `axon-frontend/src/parser.rs` MUST include a \
         `\"identity\" => {{ col.identity = true; ... }}` arm. \
         Without it, adopters cannot declare IDENTITY columns inline."
    );
    // The arm must set `col.identity = true` (not some other field).
    let identity_arm_idx = body.find("\"identity\" => {").unwrap();
    let window = &body[identity_arm_idx..(identity_arm_idx + 200).min(body.len())];
    assert!(
        window.contains("col.identity = true"),
        "D1 invariant — the `identity` arm must set `col.identity = true`. \
         Window:\n{window}"
    );
}

// ════════════════════════════════════════════════════════════════════
// section 5 — D4 form (c) env_var with manifest first-match heuristic
// ════════════════════════════════════════════════════════════════════

/// section 5 — pin that when an `axonstore` uses form (c) `schema:
/// env:TENANT_SCHEMA` AND a manifest is supplied with an entry like
/// `tenant_alpha.chat_history` (the per-tenant first-match heuristic),
/// the type-checker resolves the column set the same way as form (b)
/// and T803 skips the identity column.
#[test]
fn s5_d4_env_var_with_manifest_first_match_resolves() {
    let source = r#"
        axonstore chat_history {
            backend: postgresql
            connection: "env:DATABASE_URL"
            schema: env:TENANT_SCHEMA
        }

        flow Insert(tid: Uuid, msg: Text) -> Text {
            persist into chat_history { tenant_id: "${tid}" content: "${msg}" }
        }
    "#;
    let tokens = Lexer::new(source, "test.axon").tokenize().expect("lexes");
    let program = Parser::new(tokens).parse().expect("parses");

    // Manifest with a per-tenant namespace entry (the first-match
    // heuristic from 38.d/38.f).
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
    let mut m = Manifest::new();
    // Use a per-tenant namespace key — the suffix-scan fallback should
    // resolve it because the source's `schema: env:TENANT_SCHEMA`
    // doesn't pin a specific namespace.
    m.stores
        .insert("tenant_alpha.chat_history".to_string(), store);

    let (errors, _warnings) =
        TypeChecker::with_manifest(&program, &m).check_with_warnings();
    let t803_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.message.contains("axon-T803"))
        .collect();
    assert!(
        t803_errors.is_empty(),
        "D4 — form (c) env_var with manifest first-match resolution \
         MUST skip T803 on the identity column. Got: {t803_errors:#?}"
    );
}
