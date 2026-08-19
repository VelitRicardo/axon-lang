//! v2.26.0 — the `jsonb` store data-plane (frontend proof half).
//!
//! `persist` / `retrieve` / `where:` over a `Json` (or `Json<T>` lens)
//! column type-checks clean under the v1.31.0 store-column proof: a `jsonb`
//! column accepts a document value (T802 compat), a known field name is
//! accepted (T804), and a `where:` filter against the column resolves
//! (T801). The runtime round-trip (Postgres `$N::jsonb` cast + `JsonValue`
//! decode) and the `${alias.col.field}` navigation are pinned in `axon-rs`
//! (`postgres_backend` + `exec_context` tests).
//!
//! OSS reference backends: `postgresql` (the real typed-column jsonb data
//! plane) + `in_memory` (the KV fallback). `sqlite` remains a documented
//! future cycle — NOT claimed here (honest-limit, v2.23.0 spirit).

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn check_errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog).check().into_iter().map(|e| e.message).collect()
}

#[test]
fn persist_a_document_into_a_jsonb_column_type_checks_clean() {
    let src = r#"
        axonstore Events {
            backend: postgresql
            connection: "env:DB"
            schema {
                id:      Uuid primary_key
                payload: Json
            }
        }

        flow Ingest(payload: String) -> Unit {
            persist into Events { id: "${id}" payload: "${payload}" }
        }
    "#;
    let errs = check_errors(src);
    assert!(
        !errs.iter().any(|m| m.contains("axon-T80")),
        "persist of a document into a Json column must be clean: {errs:?}"
    );
}

#[test]
fn persist_into_a_jsonb_lens_column_type_checks_clean() {
    let src = r#"
        type UserEvent { name: String }
        axonstore Events {
            backend: postgresql
            connection: "env:DB"
            schema {
                id:      Uuid primary_key
                profile: Json<UserEvent>
            }
        }

        flow Ingest(profile: String) -> Unit {
            persist into Events { id: "${id}" profile: "${profile}" }
        }
    "#;
    let errs = check_errors(src);
    assert!(
        !errs.iter().any(|m| m.contains("axon-T80") || m.contains("axon-T840")),
        "persist into a Json<T> lens column must be clean: {errs:?}"
    );
}

#[test]
fn retrieve_with_where_over_a_jsonb_store_type_checks_clean() {
    let src = r#"
        axonstore Events {
            backend: postgresql
            connection: "env:DB"
            schema {
                id:      Uuid primary_key
                payload: Json
            }
        }

        flow Lookup(id: String) -> Unit {
            retrieve Events { where: "id = ${id}" as: result }
        }
    "#;
    let errs = check_errors(src);
    assert!(
        !errs.iter().any(|m| m.contains("axon-T80")),
        "retrieve over a store with a jsonb column must be clean: {errs:?}"
    );
}

#[test]
fn persist_field_typo_still_caught_alongside_a_jsonb_column() {
    // The jsonb column does not weaken the proof: an unknown field is still
    // axon-T804.
    let src = r#"
        axonstore Events {
            backend: postgresql
            connection: "env:DB"
            schema {
                id:      Uuid primary_key
                payload: Json
            }
        }

        flow Ingest(payload: String) -> Unit {
            persist into Events { id: "${id}" payloadd: "${payload}" }
        }
    "#;
    let errs = check_errors(src);
    assert!(
        errs.iter().any(|m| m.contains("axon-T804") && m.contains("payloadd")),
        "an unknown persist field must still be axon-T804: {errs:?}"
    );
}
