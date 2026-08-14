//! §Fase 122.b — `memory`, `pix` and `axonstore`, from a program on disk.
//!
//! # What this proves, and what it does not
//!
//! All three were attested on an engine name. This gate proves the DECLARATION
//! path: source → type checker → IR, with each declaration's closed catalogs
//! watched refusing on the fixture's own text.
//!
//! It does NOT prove the wire. `axonstore`'s Postgres path is owned by
//! `fase38_i_integration`, `memory`'s PEM write-through by §112's gates, and
//! `pix`'s navigation by §Fase 63's. Those stay. Saying so precisely is the
//! point: a citation that claims more than it shows is the defect this fase
//! exists to close, and `backend: in_memory` here follows §113.d's precedent
//! deliberately — the properties under test are the DECLARED ones.

const FIXTURE: &str = "tests/fixtures/fase122_b_declarations/memory_pix_store.axon";

fn read_fixture() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn errors_of(src: &str) -> Vec<String> {
    let tokens = axon_frontend::lexer::Lexer::new(src, FIXTURE)
        .tokenize()
        .expect("fixture must lex");
    let prog = axon_frontend::parser::Parser::new(tokens)
        .parse()
        .expect("fixture must parse");
    axon_frontend::type_checker::TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

#[test]
fn the_fixture_is_legal_and_all_three_declarations_reach_the_ir() {
    let src = read_fixture();
    let errors = errors_of(&src);
    assert!(
        errors.is_empty(),
        "the fixture must TYPE-CHECK — an adopter could not deploy it otherwise: {errors:?}"
    );

    let tokens = axon_frontend::lexer::Lexer::new(&src, FIXTURE)
        .tokenize()
        .unwrap();
    let prog = axon_frontend::parser::Parser::new(tokens).parse().unwrap();
    let ir = axon_frontend::ir_generator::IRGenerator::new().generate(&prog);

    let mem = ir
        .memories
        .first()
        .expect("the fixture declares a `memory`; an empty catalog means it no longer lowers");
    assert_eq!(mem.name, "Recall");
    assert_eq!(mem.store, "session", "the DECLARED lifecycle scope");
    assert_eq!(mem.retrieval, "semantic", "the DECLARED retrieval strategy");

    let pix = ir
        .pix_specs
        .first()
        .expect("the fixture declares a `pix`; an empty catalog means it no longer lowers");
    assert_eq!(pix.name, "ContractIndex");
    assert_eq!(pix.depth, Some(4), "the DECLARED navigation depth");

    let store = ir
        .axonstore_specs
        .first()
        .expect("the fixture declares an `axonstore`; an empty catalog means it no longer lowers");
    assert_eq!(store.name, "Ledger");
    assert_eq!(store.backend, "in_memory");
    assert_eq!(store.isolation, "serializable", "the DECLARED isolation level");
    assert!(
        store.column_schema.is_some(),
        "the §38.b inline schema must ride the artifact — a store whose declared \
         shape reaches no consumer is the §111 shape"
    );
}

// ── `rotate` — the same bounded shape, for the same structural reason ───────

const ROTATE_FIXTURE: &str = "tests/fixtures/fase122_b_credential/rotate_crm_class.axon";

fn read(rel: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn errors_of_named(src: &str, name: &str) -> Vec<String> {
    let tokens = axon_frontend::lexer::Lexer::new(src, name)
        .tokenize()
        .expect("fixture must lex");
    let prog = axon_frontend::parser::Parser::new(tokens)
        .parse()
        .expect("fixture must parse");
    axon_frontend::type_checker::TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

/// `rotate` cannot be dispatched from a static file, and the reason is
/// structural rather than a testing gap: §94's runtime gate drives a local tool
/// server on an ephemeral port, and a tool endpoint has no config-key
/// indirection the way §80 requires for `upstream`. The wire stays proven by
/// §94's own gates; what a file can carry are the two laws that make the verb
/// governable at all.
#[test]
fn rotate_requires_a_custody_store_and_a_declared_tool() {
    let src = read(ROTATE_FIXTURE);
    assert!(
        errors_of_named(&src, ROTATE_FIXTURE).is_empty(),
        "the fixture must TYPE-CHECK: {:?}",
        errors_of_named(&src, ROTATE_FIXTURE)
    );

    for (what, from, to, expect) in [
        // axon-T898 — you cannot rotate what is not custody.
        (
            "T898 the store must be `backend: secrets`",
            "backend: secrets",
            "backend: in_memory",
            "secrets",
        ),
        // axon-T899 — the exchange is not ad-hoc.
        (
            "T899 the tool must be declared",
            "with RotateCrmToken as result",
            "with NoSuchTool as result",
            "tool",
        ),
    ] {
        let mutated = src.replace(from, to);
        assert_ne!(mutated, src, "{what}: the perturbation must change the source");
        let errors = errors_of_named(&mutated, ROTATE_FIXTURE)
            .join(" | ")
            .to_lowercase();
        assert!(
            errors.contains(expect),
            "{what}: breaking it must be REFUSED, mentioning `{expect}`. Got: {errors}"
        );
    }
}

/// Each closed catalog, watched refusing on the fixture's own text. A citation
/// is worth what its failure mode proves.
#[test]
fn every_declared_catalog_refuses_a_value_outside_it() {
    let src = read_fixture();

    for (what, from, to, expect) in [
        ("memory.store", "store:     session", "store:     forever", "store"),
        (
            "memory.retrieval",
            "retrieval: semantic",
            "retrieval: telepathic",
            "retrieval",
        ),
        (
            "axonstore.backend",
            "backend:   in_memory",
            "backend:   papyrus",
            "backend",
        ),
        (
            "axonstore.isolation",
            "isolation: serializable",
            "isolation: whenever",
            "isolation",
        ),
    ] {
        let mutated = src.replace(from, to);
        assert_ne!(
            mutated, src,
            "{what}: the perturbation must actually change the source"
        );
        let errors = errors_of(&mutated).join(" | ").to_lowercase();
        assert!(
            errors.contains(expect),
            "{what}: a value outside the closed catalog must be REFUSED, mentioning \
             `{expect}`. Got: {errors}"
        );
    }
}
