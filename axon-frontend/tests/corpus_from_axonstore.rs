//! v2.14.0 — `corpus <N> from axonstore { documents: S(id,title) relations:
//! E(from,to,etype,weight) }`: the DYNAMIC, store-sourced MDN corpus graph. The
//! documents and typed edges are rows in two declared `axonstore`s; the graph
//! grows at runtime. The surface parses, type-checks the store + column mapping
//! against the v1.31.0 inline schema, and lowers to the IR (`store_source`). The
//! static v2.13.0 corpus stays byte-identical (no `store_source` in its IR).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog).check().into_iter().map(|e| e.message).collect()
}

const STORES: &str = r#"
axonstore LtmSummaries {
    backend: postgresql
    connection: "postgres://x"
    schema {
        id:         Uuid primary_key
        summary:    Text not_null
        created_at: Timestamptz
    }
}
axonstore LtmEdges {
    backend: postgresql
    connection: "postgres://x"
    schema {
        from_id: Uuid
        to_id:   Uuid
        etype:   Text
        weight:  Float
    }
}
"#;

fn with_stores(corpus: &str) -> String {
    format!("{STORES}\n{corpus}")
}

const VALID_CORPUS: &str = r#"
corpus LtmGraph from axonstore {
    documents: LtmSummaries( id, summary )
    relations: LtmEdges( from_id, to_id, etype, weight )
    adaptive: true
}
"#;

#[test]
fn valid_store_sourced_corpus_type_checks() {
    let e = errors(&with_stores(VALID_CORPUS));
    assert!(e.is_empty(), "a well-formed store-sourced MDN corpus must type-check: {e:?}");
}

#[test]
fn store_source_lowers_to_the_ir() {
    let src = with_stores(VALID_CORPUS);
    let tokens = Lexer::new(&src, "t.axon").tokenize().unwrap();
    let prog = Parser::new(tokens).parse().unwrap();
    let ir = IRGenerator::new().generate(&prog);
    let corpus = ir.corpus_specs.iter().find(|c| c.name == "LtmGraph").unwrap();
    let s = corpus.store_source.as_ref().expect("store_source lowered");
    assert_eq!(s.doc_store, "LtmSummaries");
    assert_eq!(s.doc_id, "id");
    assert_eq!(s.doc_title, "summary");
    assert_eq!(s.edge_store, "LtmEdges");
    assert_eq!(s.edge_from, "from_id");
    assert_eq!(s.edge_to, "to_id");
    assert_eq!(s.edge_type, "etype");
    assert_eq!(s.edge_weight, "weight");
    // The dynamic form needs no static documents/relations.
    assert!(corpus.documents.is_empty());
    assert!(corpus.relations.is_empty());
    assert!(corpus.adaptive, "adaptive flag carries onto the store-sourced graph");
}

#[test]
fn adaptive_store_sourced_needs_no_static_relations() {
    // v2.13.0's "adaptive requires relations" must accept the store-sourced edge
    // store as the graph (the edges live as rows, not literals).
    let e = errors(&with_stores(VALID_CORPUS));
    assert!(
        !e.iter().any(|m| m.contains("requires `relations:`")),
        "adaptive over a store-sourced graph must NOT demand static relations: {e:?}"
    );
}

#[test]
fn static_corpus_ir_is_byte_identical() {
    // Back-compat: a static v2.13.0 corpus has no store_source in its IR (the field
    // serde-skips when None).
    let src = r#"
type A { text: String }
type B { text: String }
corpus Flat { documents: [A, B]  relations: [ cite(B, A, 0.9) ] }
"#;
    let tokens = Lexer::new(src, "t.axon").tokenize().unwrap();
    let prog = Parser::new(tokens).parse().unwrap();
    let ir = IRGenerator::new().generate(&prog);
    let corpus = ir.corpus_specs.iter().find(|c| c.name == "Flat").unwrap();
    assert!(corpus.store_source.is_none(), "static corpus carries no store_source");
    let json = serde_json::to_string(corpus).unwrap();
    assert!(!json.contains("store_source"), "store_source must be elided from static-corpus IR JSON");
}

#[test]
fn undeclared_stores_are_rejected() {
    let src = r#"
corpus G from axonstore {
    documents: NoSuchDocStore( id, title )
    relations: NoSuchEdgeStore( a, b, t, w )
}
"#;
    let e = errors(src);
    assert!(
        e.iter().any(|m| m.contains("documents store 'NoSuchDocStore' is not a declared axonstore")),
        "undeclared documents store must error: {e:?}"
    );
    assert!(
        e.iter().any(|m| m.contains("relations store 'NoSuchEdgeStore' is not a declared axonstore")),
        "undeclared relations store must error: {e:?}"
    );
}

#[test]
fn missing_mapped_column_is_rejected() {
    let corpus = r#"
corpus G from axonstore {
    documents: LtmSummaries( id, nope_no_such_col )
    relations: LtmEdges( from_id, to_id, etype, weight )
}
"#;
    let e = errors(&with_stores(corpus));
    assert!(
        e.iter().any(|m| m.contains("has no column 'nope_no_such_col'")),
        "a title column that doesn't exist on the store must error: {e:?}"
    );
}

#[test]
fn wrong_column_types_are_rejected() {
    // title = created_at (Timestamptz, not text-like) → reject;
    // weight = etype (Text, not numeric) → reject;
    // from = (Uuid) vs id (Uuid) is fine, but map `from` to a Text col to force
    // a G2 type-mismatch is covered separately; here cover title + weight.
    let corpus = r#"
corpus G from axonstore {
    documents: LtmSummaries( id, created_at )
    relations: LtmEdges( from_id, to_id, etype, etype )
}
"#;
    let e = errors(&with_stores(corpus));
    assert!(
        e.iter().any(|m| m.contains("title column 'created_at' must be text-like")),
        "non-text title must error: {e:?}"
    );
    assert!(
        e.iter().any(|m| m.contains("weight column 'etype' must be numeric")),
        "non-numeric weight must error: {e:?}"
    );
}

#[test]
fn edge_endpoint_type_must_match_document_id() {
    // map the edge `from` to a Text column while the document id is Uuid → G2.
    let corpus = r#"
corpus G from axonstore {
    documents: LtmSummaries( id, summary )
    relations: LtmEdges( etype, to_id, etype, weight )
}
"#;
    let e = errors(&with_stores(corpus));
    assert!(
        e.iter().any(|m| m.contains("must match the document id column type")),
        "an edge endpoint whose type differs from the document id must error (G2): {e:?}"
    );
}
