//! v1.31.0 — Rust frontend grammar surface for the `schema:`
//! declaration on `axonstore` (D1).
//!
//! Three closed forms, each parsed into a structured AST node + lowered
//! into an IR variant. The Rust frontend is authoritative (per D9); the
//! Python frontend (`axon/compiler/parser.py:_parse_store_schema_declaration`)
//! mirrors the same shape for cross-stack drift-gate purposes.
//!
//! NO type-checking lives here — that's the v1.31.0 / v1.31.0
//! `StoreColumnProof` pass, shipping later. 38.b's job is the grammar.

use axon_frontend::ast::{AxonStoreDefinition, Declaration};
use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::{IRStoreColumnSchema, IRAxonStore};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::store_schema::{StoreColumnSchema, StoreColumnType};

/// Lex + parse + IR-generate a source snippet and return its
/// `IRAxonStore` for the (unique) declared store.
fn lower_store(src: &str) -> IRAxonStore {
    let tokens = Lexer::new(src, "test.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    let ir = IRGenerator::new().generate(&program);
    assert_eq!(ir.axonstore_specs.len(), 1, "expected exactly one axonstore");
    ir.axonstore_specs[0].clone()
}

/// Lex + parse, returning the (unique) `AxonStoreDefinition` AST node.
fn parse_store(src: &str) -> AxonStoreDefinition {
    let tokens = Lexer::new(src, "test.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    program
        .declarations
        .into_iter()
        .find_map(|d| match d {
            Declaration::AxonStore(s) => Some(s),
            _ => None,
        })
        .expect("axonstore declaration")
}

// ════════════════════════════════════════════════════════════════════
//  Form (a) — inline column block
// ════════════════════════════════════════════════════════════════════

#[test]
fn form_a_inline_column_block_parses_with_pascal_case_types() {
    let src = r#"
        axonstore tenants {
            backend: postgresql
            connection: "env:DATABASE_URL"
            schema {
                tenant_id: Uuid primary_key
                tier:      Text not_null
                created_at: Timestamptz
                balance:   Numeric default 0.00
                active:    Bool default true
            }
        }
    "#;
    let store = parse_store(src);
    let schema = store.column_schema.expect("inline schema");
    match schema {
        StoreColumnSchema::Inline { columns, .. } => {
            assert_eq!(columns.len(), 5);
            assert_eq!(columns[0].name, "tenant_id");
            assert_eq!(columns[0].col_type, StoreColumnType::Uuid);
            assert!(columns[0].primary_key);
            assert_eq!(columns[1].col_type, StoreColumnType::Text);
            assert!(columns[1].not_null);
            assert_eq!(columns[2].col_type, StoreColumnType::Timestamptz);
            assert_eq!(columns[3].col_type, StoreColumnType::Numeric);
            assert_eq!(columns[3].default_value, "0.00");
            assert_eq!(columns[4].col_type, StoreColumnType::Bool);
            assert_eq!(columns[4].default_value, "true");
        }
        other => panic!("expected Inline, got {other:?}"),
    }
}

#[test]
fn form_a_inline_accepts_common_lowercase_aliases_normalised_to_canonical() {
    // D5 ergonomic floor — `int`/`integer`/`bool`/`boolean`/… are
    // accepted and rewritten to the canonical PascalCase form on the
    // AST + IR. The IR's `col_type` is the canonical name.
    let src = r#"
        axonstore tenants {
            backend: postgresql
            connection: "env:DATABASE_URL"
            schema {
                id: int primary_key
                age: integer
                big: bigint
                yes: boolean
                d: decimal
                v: varchar
                s: string
                t: text
            }
        }
    "#;
    let ir_store = lower_store(src);
    let schema = ir_store.column_schema.expect("schema");
    match schema {
        IRStoreColumnSchema::Inline { columns } => {
            let observed: Vec<(&str, &str)> = columns
                .iter()
                .map(|c| (c.name.as_str(), c.col_type.as_str()))
                .collect();
            assert_eq!(
                observed,
                vec![
                    ("id", "Int"),
                    ("age", "Int"),
                    ("big", "BigInt"),
                    ("yes", "Bool"),
                    ("d", "Numeric"),
                    ("v", "Text"),
                    ("s", "Text"),
                    ("t", "Text"),
                ],
                "aliases must normalize to canonical PascalCase names"
            );
        }
        other => panic!("expected Inline, got {other:?}"),
    }
}

#[test]
fn form_a_rejects_unknown_column_type_with_an_actionable_message() {
    // An unknown type fails parse with a v1.20.0-style message naming
    // the closed catalog + offering a Levenshtein hint when one fits.
    let src = r#"
        axonstore claims {
            backend: postgresql
            connection: "env:DATABASE_URL"
            schema {
                claim_id: UUUID
            }
        }
    "#;
    let tokens = Lexer::new(src, "test.axon").tokenize().expect("lex");
    let err = Parser::new(tokens).parse().expect_err("must reject");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("UUUID"),
        "error must name the offending type, got: {msg}"
    );
    assert!(
        msg.contains("closed v1.38.0 column-type catalog"),
        "error must name the catalog, got: {msg}"
    );
    assert!(
        msg.contains("Uuid"),
        "error must surface a suggestion (Levenshtein-closest), got: {msg}"
    );
}

// ════════════════════════════════════════════════════════════════════
//  Form (b) — manifest reference
// ════════════════════════════════════════════════════════════════════

#[test]
fn form_b_manifest_reference_parses_as_a_string_literal() {
    let src = r#"
        axonstore tenants {
            backend: postgresql
            connection: "env:DATABASE_URL"
            schema: "public.tenants"
        }
    "#;
    let ir_store = lower_store(src);
    match ir_store.column_schema.expect("manifest_ref") {
        IRStoreColumnSchema::ManifestRef { qualified_name } => {
            assert_eq!(qualified_name, "public.tenants");
        }
        other => panic!("expected ManifestRef, got {other:?}"),
    }
}

#[test]
fn form_b_empty_manifest_reference_is_an_error() {
    let src = r#"
        axonstore tenants {
            backend: postgresql
            connection: "env:DATABASE_URL"
            schema: ""
        }
    "#;
    let tokens = Lexer::new(src, "test.axon").tokenize().expect("lex");
    let err = Parser::new(tokens).parse().expect_err("must reject");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("manifest reference is empty"),
        "expected named-empty diagnostic, got: {msg}"
    );
}

// ════════════════════════════════════════════════════════════════════
//  Form (c) — per-tenant env-var schema namespace
// ════════════════════════════════════════════════════════════════════

#[test]
fn form_c_env_var_unquoted_resolves_to_envvar_variant() {
    let src = r#"
        axonstore tenants {
            backend: postgresql
            connection: "env:DATABASE_URL"
            schema: env:TENANT_SCHEMA
        }
    "#;
    let ir_store = lower_store(src);
    match ir_store.column_schema.expect("env_var") {
        IRStoreColumnSchema::EnvVar { var_name } => {
            assert_eq!(var_name, "TENANT_SCHEMA");
        }
        other => panic!("expected EnvVar, got {other:?}"),
    }
}

#[test]
fn form_c_env_var_quoted_resolves_to_envvar_variant() {
    // The string-literal form `schema: "env:TENANT_SCHEMA"` is
    // semantically identical to the unquoted form — both lower to
    // EnvVar with `var_name=TENANT_SCHEMA`. This matches the
    // `connection: "env:..."` precedent's string-literal form for
    // adopter familiarity.
    let src = r#"
        axonstore tenants {
            backend: postgresql
            connection: "env:DATABASE_URL"
            schema: "env:TENANT_SCHEMA"
        }
    "#;
    let ir_store = lower_store(src);
    match ir_store.column_schema.expect("env_var (quoted)") {
        IRStoreColumnSchema::EnvVar { var_name } => {
            assert_eq!(var_name, "TENANT_SCHEMA");
        }
        other => panic!("expected EnvVar, got {other:?}"),
    }
}

#[test]
fn form_c_env_var_with_empty_var_name_is_an_error() {
    let src = r#"
        axonstore tenants {
            backend: postgresql
            connection: "env:DATABASE_URL"
            schema: "env:"
        }
    "#;
    let tokens = Lexer::new(src, "test.axon").tokenize().expect("lex");
    let err = Parser::new(tokens).parse().expect_err("must reject");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("missing the variable name"),
        "expected named-missing-var diagnostic, got: {msg}"
    );
}

// ════════════════════════════════════════════════════════════════════
//  D5 absolute — an axonstore without a `schema:` declaration runs
//  the 37.x runtime+deploy path verbatim. The IR's `column_schema`
//  is `None` and the serialized JSON omits the field entirely.
// ════════════════════════════════════════════════════════════════════

#[test]
fn d5_absolute_no_schema_declaration_lowers_to_none_in_ir() {
    let src = r#"
        axonstore tenants {
            backend: postgresql
            connection: "env:DATABASE_URL"
        }
    "#;
    let ir_store = lower_store(src);
    assert!(
        ir_store.column_schema.is_none(),
        "an axonstore without `schema:` must have column_schema=None"
    );
    // Serialised JSON must not carry the field — `skip_serializing_if`
    // discipline preserves the v1.37.0 IR byte shape for adopters who
    // haven't opted into a schema declaration (D5 absolute).
    let json = serde_json::to_value(&ir_store).expect("serialize");
    assert!(
        !json.as_object().unwrap().contains_key("column_schema"),
        "the IR JSON of an undeclared-schema store must be byte-identical \
         to v1.37.0 (no column_schema key)"
    );
}

// ════════════════════════════════════════════════════════════════════
//  Existing axonstore fields keep working alongside the new `schema:`.
//  Important: a `capability:` declared adjacent to a `schema:` block
//  must not corrupt the schema parser, and vice versa.
// ════════════════════════════════════════════════════════════════════

#[test]
fn schema_composes_cleanly_with_capability_and_other_fields() {
    let src = r#"
        axonstore tenants {
            backend: postgresql
            connection: "env:DATABASE_URL"
            capability: "tenant.read"
            schema {
                tenant_id: Uuid primary_key
                tier: Text not_null
            }
            confidence_floor: 0.95
            isolation: read_committed
            on_breach: raise
        }
    "#;
    let store = parse_store(src);
    assert_eq!(store.capability, "tenant.read");
    assert_eq!(store.isolation, "read_committed");
    assert_eq!(store.on_breach, "raise");
    assert_eq!(store.confidence_floor, Some(0.95));
    let schema = store.column_schema.expect("inline schema");
    assert!(schema.is_inline());
    assert_eq!(schema.inline_columns().unwrap().len(), 2);
}

// ════════════════════════════════════════════════════════════════════
//  IR tagged-union serialization — the `form` discriminator is
//  stable adopter-facing surface for the LSP + manifest tooling.
// ════════════════════════════════════════════════════════════════════

#[test]
fn ir_serialization_carries_a_form_discriminator() {
    for (src, expected_form) in [
        (
            r#"axonstore s { backend: postgresql connection: "env:X" schema { id: Uuid } }"#,
            "inline",
        ),
        (
            r#"axonstore s { backend: postgresql connection: "env:X" schema: "public.s" }"#,
            "manifest_ref",
        ),
        (
            r#"axonstore s { backend: postgresql connection: "env:X" schema: env:NS }"#,
            "env_var",
        ),
    ] {
        let ir_store = lower_store(src);
        let json = serde_json::to_value(&ir_store).expect("serialize");
        let schema_obj = json
            .get("column_schema")
            .expect("column_schema present")
            .as_object()
            .expect("schema is object");
        assert_eq!(
            schema_obj.get("form").and_then(|v| v.as_str()),
            Some(expected_form),
            "form discriminator must match the declared shape"
        );
    }
}
