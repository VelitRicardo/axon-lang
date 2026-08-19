//! v2.26.0 (OSS half) — the `index` column declaration: an index as a
//! CAPABILITY-HONEST EFFECT. A store column declared `index` records the
//! intent in the program's source, surfaces it through the AST and the IR,
//! and so the deployment layer (the enterprise deploy gate) SEES the index
//! as a declared capability — never a silent out-of-band DBA action. The
//! index METHOD is the backend's call (a GIN path index for a `Json`/`Jsonb`
//! column, a b-tree otherwise); the OSS half ships the *declaration*.
//!
//! The enterprise GIN-DDL materialization consumes this flag at the v2.26.0
//! release catch-up (the enterprise layer pins a published axon-lang).

use axon_frontend::ast::Declaration;
use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::IRStoreColumnSchema;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::store_schema::StoreColumnSchema;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn inline_columns(prog: &axon_frontend::ast::Program) -> Vec<(String, bool)> {
    for decl in &prog.declarations {
        if let Declaration::AxonStore(s) = decl {
            if let Some(StoreColumnSchema::Inline { columns, .. }) = &s.column_schema {
                return columns.iter().map(|c| (c.name.clone(), c.indexed)).collect();
            }
        }
    }
    panic!("no inline axonstore schema");
}

const STORE: &str = r#"
    axonstore Events {
        backend: postgresql
        connection: "env:DB"
        schema {
            id:      Uuid primary_key
            payload: Json index
            tier:    Text
        }
    }
"#;

#[test]
fn index_constraint_sets_the_indexed_flag() {
    let cols = inline_columns(&parse(STORE));
    let payload = cols.iter().find(|(n, _)| n == "payload").expect("payload column");
    assert!(payload.1, "`payload: Json index` must set indexed = true");
}

#[test]
fn columns_without_index_default_to_not_indexed() {
    let cols = inline_columns(&parse(STORE));
    let id = cols.iter().find(|(n, _)| n == "id").expect("id column");
    let tier = cols.iter().find(|(n, _)| n == "tier").expect("tier column");
    assert!(!id.1, "a column without `index` must default to indexed = false");
    assert!(!tier.1, "a column without `index` must default to indexed = false");
}

#[test]
fn index_composes_with_other_constraints() {
    // `index` is position-independent alongside the other constraints.
    let src = r#"
        axonstore Events {
            backend: postgresql
            connection: "env:DB"
            schema {
                id:   Uuid primary_key
                doc:  Jsonb not_null index
            }
        }
    "#;
    let cols = inline_columns(&parse(src));
    let doc = cols.iter().find(|(n, _)| n == "doc").expect("doc column");
    assert!(doc.1, "`Jsonb not_null index` must set indexed = true");
}

#[test]
fn index_is_also_allowed_on_a_non_jsonb_column() {
    // The OSS declaration is type-agnostic — the backend picks GIN vs b-tree.
    let src = r#"
        axonstore Events {
            backend: postgresql
            connection: "env:DB"
            schema {
                id:    Uuid primary_key
                email: Text index
            }
        }
    "#;
    let cols = inline_columns(&parse(src));
    let email = cols.iter().find(|(n, _)| n == "email").expect("email column");
    assert!(email.1, "`Text index` must set indexed = true (b-tree at the backend)");
}

#[test]
fn indexed_flag_lowers_into_the_ir() {
    let prog = parse(STORE);
    let ir = IRGenerator::new().generate(&prog);
    let store = ir
        .axonstore_specs
        .iter()
        .find(|s| s.name == "Events")
        .expect("Events store in IR");
    let cols = match &store.column_schema {
        Some(IRStoreColumnSchema::Inline { columns, .. }) => columns,
        other => panic!("expected an inline IR schema, got {other:?}"),
    };
    let payload = cols.iter().find(|c| c.name == "payload").expect("payload in IR");
    assert!(payload.indexed, "the IR must round-trip indexed = true for the deploy gate");
    let id = cols.iter().find(|c| c.name == "id").expect("id in IR");
    assert!(!id.indexed, "a non-indexed column stays indexed = false in the IR");
}
