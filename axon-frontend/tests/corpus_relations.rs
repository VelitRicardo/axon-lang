//! v2.13.0 — `corpus { relations: [...] }`: the MDN corpus-graph surface.
//! Typed weighted edges parse, type-check against the closed relation catalog +
//! the declared documents + the weight range, and lower to the IR.

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog).check().into_iter().map(|e| e.message).collect()
}

const VALID: &str = r#"
corpus SessionKnowledge {
    documents: [sess_a, sess_b, sess_c]
    relations: [
        cite(sess_b, sess_a, 0.9)
        contradict(sess_c, sess_a, 0.7)
        elaborate(sess_c, sess_b, 0.5)
    ]
}
"#;

#[test]
fn valid_corpus_graph_type_checks() {
    let e = errors(VALID);
    assert!(e.is_empty(), "a well-formed MDN corpus graph must type-check: {e:?}");
}

#[test]
fn relations_lower_to_the_ir() {
    let tokens = Lexer::new(VALID, "t.axon").tokenize().unwrap();
    let prog = Parser::new(tokens).parse().unwrap();
    let ir = IRGenerator::new().generate(&prog);
    let corpus = ir.corpus_specs.iter().find(|c| c.name == "SessionKnowledge").unwrap();
    assert_eq!(corpus.relations.len(), 3, "all three edges lowered");
    let cite = corpus.relations.iter().find(|r| r.etype == "cite").unwrap();
    assert_eq!(cite.from, "sess_b");
    assert_eq!(cite.to, "sess_a");
    assert!((cite.weight - 0.9).abs() < 1e-9);
}

#[test]
fn unknown_relation_type_is_rejected() {
    let src = "corpus C { documents: [a, b] relations: [ hugs(a, b, 0.5) ] }";
    assert!(
        errors(src).iter().any(|m| m.contains("unknown relation type")),
        "an off-catalog relation type must error"
    );
}

#[test]
fn relation_to_undeclared_document_is_rejected() {
    let src = "corpus C { documents: [a, b] relations: [ cite(a, z, 0.5) ] }";
    assert!(
        errors(src).iter().any(|m| m.contains("undeclared document")),
        "an edge to a non-member document must error (G2)"
    );
}

#[test]
fn relation_weight_out_of_range_is_rejected() {
    let high = "corpus C { documents: [a, b] relations: [ cite(a, b, 1.5) ] }";
    let zero = "corpus C { documents: [a, b] relations: [ cite(a, b, 0.0) ] }";
    assert!(errors(high).iter().any(|m| m.contains("(0, 1]")), "ω > 1 must error (G4)");
    assert!(errors(zero).iter().any(|m| m.contains("(0, 1]")), "ω = 0 must error (G4)");
}

#[test]
fn edgeless_corpus_still_type_checks() {
    // Back-compat: a corpus with no `relations:` is the flat corpus, unchanged.
    let e = errors("corpus Flat { documents: [a, b] }");
    assert!(e.is_empty(), "an edgeless corpus is the pre-v2.13.0 flat corpus: {e:?}");
}

// ── v2.13.0 — the `adaptive:` memory flag ──────────────────────────────────────

#[test]
fn adaptive_corpus_with_relations_type_checks() {
    let src = "corpus M { documents: [a, b] relations: [ cite(a, b, 0.9) ] adaptive: true }";
    assert!(errors(src).is_empty(), "an adaptive MDN graph must type-check");
}

#[test]
fn adaptive_without_relations_is_rejected() {
    // Memory deforms the graph — an edgeless corpus has nothing to learn.
    let src = "corpus M { documents: [a, b] adaptive: true }";
    assert!(
        errors(src).iter().any(|m| m.contains("requires `relations:`")),
        "`adaptive` without `relations` must error"
    );
}
