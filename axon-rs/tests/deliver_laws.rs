//! v2.89.0 (group B) — `deliver`, from a program that lives on disk.
//!
//! # Why this file exists, and what it honestly proves
//!
//! `deliver` was attested on `delivery.rs + axon-T920` — a module plus a code.
//! That is an ENGINE citation.
//!
//! It is NOT group A: `secret:` is a per-tenant key under v2.48.0 custody
//! (`axon-T923`), so no fixture without a mounted minter can DISPATCH one. What
//! a fixture can do — and what this gate does — is drive the compile-time laws
//! that are the primitive's substance and the ones an adopter meets first.
//!
//! Stating the boundary precisely matters more than the count: this proves the
//! DECLARATION path (source → type checker → IR), not the egress itself. The
//! egress is proven by v2.60.0's own runtime gates, which stay.
//!
//! # The laws
//!
//! v2.60.0's rule is that provenance travels or the delivery refuses:
//!
//! - `axon-T921` — `target:` is a closed system-of-record class
//! - `axon-T922` — `provenance:` decides how field origin crosses
//! - `axon-T923` — `secret:` is a custody key, never a literal
//! - `axon-T924` — the effect row must include `web`, because it leaves
//! - `axon-T926` — every operation carries an idempotency `key:`

const FIXTURE: &str = "tests/fixtures/deliver/crm_handoff.axon";

/// Read a fixture with line endings NORMALISED to `\n`.
///
/// v2.89.0 — without this the T926 perturbation below (which spans a line
/// break) matches nothing on a Windows checkout, where git hands the file over
/// as CRLF. The test then failed on `windows-latest` only, and it failed on the
/// right assertion: `assert_ne!(mutated, src)` exists precisely so a
/// perturbation that changes nothing cannot masquerade as a passing mutation.
/// It caught its own author on a platform he never ran.
///
/// Normalising is correct rather than a workaround: the fixture's SEMANTIC
/// content is what is under test, and `\r\n` versus `\n` is not part of it.
fn read_fixture(rel: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .replace("\r\n", "\n")
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

fn ir_of(src: &str) -> axon::ir_nodes::IRProgram {
    let tokens = axon_frontend::lexer::Lexer::new(src, FIXTURE)
        .tokenize()
        .expect("lex");
    let prog = axon_frontend::parser::Parser::new(tokens).parse().expect("parse");
    axon_frontend::ir_generator::IRGenerator::new().generate(&prog)
}

#[test]
fn the_fixture_is_a_legal_delivery_and_reaches_the_ir_as_declared() {
    let src = read_fixture(FIXTURE);
    let errors = errors_of(&src);
    assert!(
        errors.is_empty(),
        "the fixture must TYPE-CHECK — an adopter could not deploy it otherwise: {errors:?}"
    );

    let ir = ir_of(&src);
    let d = ir
        .deliveries
        .first()
        .expect("the fixture declares a `deliver`; an empty catalog means it no longer lowers");
    assert_eq!(d.name, "CrmHandoff");
    assert_eq!(d.target, "crm", "the declared system-of-record class");
    assert_eq!(
        d.secret, "crm.hubspot",
        "the custody KEY must ride the artifact — never a literal credential"
    );
}

/// Each law, watched refusing on this very fixture. A citation is worth what its
/// failure mode proves, so each perturbation is applied to the file's own text.
#[test]
fn every_declared_law_refuses_when_the_fixture_breaks_it() {
    let src = read_fixture(FIXTURE);

    for (what, from, to, expect) in [
        (
            "T921 target catalog",
            "target:     crm",
            "target:     ledger",
            "target",
        ),
        (
            "T924 the delivery must declare the `web` it crosses",
            "<web, sensitive:contact",
            "<sensitive:contact",
            "web",
        ),
        // the design decision — binding `sensitive:*` into a system of record is FURTHER
        // PROCESSING, so the legal basis is not optional decoration.
        (
            "T924 sensitive data needs a declared legal basis",
            ", legal:legitimate_interest>",
            ">",
            "legal",
        ),
        (
            "T926 idempotency key",
            "key:       \"lead-4471\"\n        ",
            "",
            "key",
        ),
    ] {
        let mutated = src.replace(from, to);
        assert_ne!(mutated, src, "{what}: the perturbation must actually change the source");
        let errors = errors_of(&mutated).join(" | ").to_lowercase();
        assert!(
            errors.contains(expect),
            "{what}: breaking it must be REFUSED, mentioning `{expect}`. Got: {errors}"
        );
    }
}
