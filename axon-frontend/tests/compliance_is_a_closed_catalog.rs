//! v4.0.0 — **`compliance:` is a closed catalogue, as the paper always said.**
//!
//! # The promise, and where it went
//!
//! The ESK paper section 6.1 (*Regulatory Type Theory*) states the rule as a
//! compile-time law:
//!
//! > *"κ es un subconjunto del registro canónico Κ = {HIPAA, PCI_DSS, GDPR,
//! > SOX, FINRA, ISO27001, SOC2, FISMA, GxP, CCPA, NIST_800_53}. Cualquier
//! > etiqueta fuera de Κ es **compile-time error** (typos como "HIPPA"
//! > rechazados, cf. `TestComplianceCoverage.test_unknown_regulatory_class_rejected`)."*
//!
//! That citation is to a **Python** test. The rule was real in the retired
//! interpreter and did not survive the Rust rewrite — and not because anyone
//! decided against it. The canonical registry landed in
//! `axon-rs::esk::compliance`, while `axon-frontend` depends on `serde` and
//! nothing else. **The catalogue ended up downstream of the type checker that
//! needed it**, so the law could not be written where it belonged.
//!
//! What that cost, measured 2026-08-14 and recorded in `advertised.rs` before
//! this cycle existed: `compliance: [NOT_A_FRAMEWORK]` on an `axonendpoint`
//! compiled clean, while `effects: <not_an_effect>` in the same file was
//! refused with its closed catalogue named. The fourth recorded instance in
//! this project of *a free string field breeding an imaginary catalogue*, and
//! the most exposed — the values are `PCI_DSS`, `SOX`, `HIPAA`, read by a
//! regulated adopter as an assertion.
//!
//! # Why a typo here is worse than a typo elsewhere
//!
//! `HIPPA` does not merely fail to mean `HIPAA`. It **passes every downstream
//! check by being consistently wrong**: `axon-T957` enforces
//! `κ(shield) ⊇ κ(body) ∪ κ(output)` as a set difference over these strings, so
//! a `type` labelled `[HIPPA]` was perfectly covered by a shield labelled
//! `[HIPPA]`. The coverage law was correct and was operating on a vocabulary
//! nobody had closed.
//!
//! `a_symmetric_typo_can_no_longer_satisfy_the_coverage_law` is the test for
//! that, and it is the one worth reading: closing the vocabulary shuts the hole
//! **by construction**, without adding a second check to T957.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<t>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    // Through BOTH doors — v2.87.0 found these two silently divergent for entire
    // cycles, with the CLI taking the one that had lost two laws.
    let via_check: Vec<String> = TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect();
    let (errs, _warns) = TypeChecker::new(&prog).check_with_warnings();
    let via_other: Vec<String> = errs.iter().map(|e| e.message.clone()).collect();
    assert_eq!(via_check, via_other, "the two checker entry points disagree");
    via_check
}

fn t1214(src: &str) -> Vec<String> {
    errors(src)
        .into_iter()
        .filter(|e| e.contains("axon-T1214"))
        .collect()
}

const FLOW: &str = "flow Handle() -> Unit { step S { ask: \"go\" } }\n";

// ── The four declarations that can carry a label ────────────────────────────

/// 🎯 The exact program `advertised.rs` recorded as compiling clean.
#[test]
fn not_a_framework_on_an_endpoint_is_refused() {
    let src = format!(
        "axonendpoint Intake {{ public: true  method: POST  path: \"/i\"  execute: Handle  \
         compliance: [NOT_A_FRAMEWORK]  backend: stub }}\n{FLOW}"
    );
    let hits = t1214(&src);
    assert_eq!(hits.len(), 1, "expected exactly one T1214: {hits:?}");
    assert!(
        hits[0].contains("NOT_A_FRAMEWORK") && hits[0].contains("Intake"),
        "the diagnostic must quote the label AND name the declaration: {}",
        hits[0]
    );
}

/// The law covers all four declarations that parse the field — a label is an
/// assertion wherever it is written.
#[test]
fn every_declaration_that_carries_a_label_is_checked() {
    let cases = [
        (
            "type",
            format!("type T compliance [BOGUS] {{ a: String }}\n{FLOW}"),
        ),
        (
            "shield",
            format!("shield S {{ scan: [pii_leak]  compliance: [BOGUS] }}\n{FLOW}"),
        ),
        (
            "axonendpoint",
            format!(
                "axonendpoint E {{ public: true  method: POST  path: \"/e\"  execute: Handle  \
                 compliance: [BOGUS]  backend: stub }}\n{FLOW}"
            ),
        ),
        (
            "manifest",
            format!("manifest M {{ compliance: [BOGUS] }}\n{FLOW}"),
        ),
    ];
    for (what, src) in cases {
        let hits = t1214(&src);
        assert!(
            !hits.is_empty(),
            "`{what}` parses `compliance:` but the law does not reach it — a label unchecked \
             on one declaration is the whole gap, moved. All errors: {:?}",
            errors(&src)
        );
    }
}

/// The negative that gives the rest their meaning: the eleven real classes
/// still compile. Without this the law could be "refuse everything".
#[test]
fn every_canonical_class_still_compiles() {
    for class in axon_frontend::compliance::REGULATORY_CLASSES {
        let src = format!("type T compliance [{class}] {{ a: String }}\n{FLOW}");
        assert!(
            t1214(&src).is_empty(),
            "`{class}` is in Κ and must compile — all errors: {:?}",
            errors(&src)
        );
    }
}

/// A multi-class list where only ONE member is bad reports that one, and only
/// that one. A diagnostic that blames the whole list makes the author hunt.
#[test]
fn only_the_offending_label_is_reported() {
    let src = format!("type T compliance [HIPAA, HIPPA, SOC2] {{ a: String }}\n{FLOW}");
    let hits = t1214(&src);
    assert_eq!(hits.len(), 1, "one bad label, one diagnostic: {hits:?}");
    assert!(hits[0].contains("HIPPA"), "{}", hits[0]);
    assert!(
        hits[0].contains("Did you mean `HIPAA`?"),
        "the typo the paper itself names must get its suggestion: {}",
        hits[0]
    );
}

/// Case is part of the label. `hipaa` and `HIPAA` are different strings to every
/// consumer that groups by them — the dossier, the SBOM filter, the evidence
/// packager — so accepting both would make one class look like two.
#[test]
fn case_variants_are_not_the_class() {
    let src = format!("type T compliance [hipaa] {{ a: String }}\n{FLOW}");
    assert_eq!(t1214(&src).len(), 1, "`hipaa` is not `HIPAA`");
}

// ── The side effect worth naming ────────────────────────────────────────────

/// 🎯 **A symmetric typo can no longer satisfy `axon-T957`.**
///
/// T957 requires `κ(shield) ⊇ κ(body) ∪ κ(output)` — a real set difference, and
/// a good law. It compares raw strings, so before v4.0.0 a `type` labelled
/// `[HIPPA]` was *perfectly covered* by a shield labelled `[HIPPA]`: the
/// difference was empty, the program compiled, and the endpoint asserted
/// coverage of a regulation that does not exist.
///
/// This is not fixed by patching T957. It is fixed by there being no such
/// string: closing the vocabulary makes the shape unrepresentable, so the
/// coverage law keeps its exact previous logic and stops being satisfiable by
/// nonsense. Recorded as a test because "closed by construction" is the kind of
/// claim that is true until someone reopens the construction.
#[test]
fn a_symmetric_typo_can_no_longer_satisfy_the_coverage_law() {
    let src = format!(
        "type PatientRecord compliance [HIPPA] {{ ssn: String }}\n\
         shield PHIShield {{ scan: [pii_leak]  compliance: [HIPPA] }}\n\
         axonendpoint Intake {{ public: true  method: POST  path: \"/i\"  execute: Handle  \
         body: PatientRecord  shield: PHIShield  backend: stub }}\n{FLOW}"
    );
    let hits = t1214(&src);
    assert_eq!(
        hits.len(),
        2,
        "BOTH sides of the symmetric typo must be refused — the type and the shield. \
         Refusing only one would leave the coverage law comparing a real class against a \
         phantom. Errors: {:?}",
        errors(&src)
    );
    // And the same program with the real class compiles: the law refuses the
    // typo, not the pattern.
    let fixed = src.replace("HIPPA", "HIPAA");
    assert!(
        t1214(&fixed).is_empty(),
        "spelled correctly, this is exactly the shape section 6.1 exists to bless: {:?}",
        errors(&fixed)
    );
}
