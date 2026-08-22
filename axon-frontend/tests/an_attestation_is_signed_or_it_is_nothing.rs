//! v4.5.0 — where the compiler stops proving, a person signs.
//!
//! The previous cycle made declassification a typed act: the class must be real,
//! the control must declare the capability, the destination must not still carry
//! the class, and every identifier the destination keeps must be one the regime
//! permits in reduced form. That is a lot, and it is still not the claim an
//! adopter needs to make.
//!
//! The claim is *"this is no longer PHI"*, and 45 CFR 164.514 offers two routes
//! to it. Safe Harbor enumerates eighteen identifier classes — **seventeen** of
//! which this compiler decides from the type graph. The eighteenth, (R), asks
//! about "any other unique identifying number, characteristic, or code", and the
//! method is disqualified outright by 164.514(b)(2)(ii) if the covered entity
//! has actual knowledge the residual data could identify someone. Neither
//! quantifies over source text. The other route, expert determination, is a
//! statistician concluding the risk is very small; there is nothing in it for a
//! type system to check at all.
//!
//! # Why the compiler checks anything here
//!
//! Everything in an `attest` block is human assertion, so the tempting design is
//! to check nothing. The opposite follows: precisely because none of the CONTENT
//! is verifiable, the FORM has to be. An artifact carrying a determination with
//! no owner, no date, or a legal basis that does not exist is read downstream as
//! though a person stood behind it — and the whole point of separating proof
//! from signature is that a reader can tell which is which.
//!
//! And it is mandatory (`axon-T1234`). An act of governance with no trace is
//! indistinguishable from a shortcut.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn diagnostics(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

fn hits(src: &str, code: &str) -> Vec<String> {
    diagnostics(src)
        .into_iter()
        .filter(|m| m.contains(code))
        .collect()
}

const TYPES: &str = "type PatientRecord compliance [HIPAA] { ssn: String }\n\
                     type ClinicalNote { note: String }\n";
const SHIELD: &str = "shield SafeHarbor { scan: [pii_leak]  compliance: [HIPAA]  \
                      declassifies: [HIPAA]  suppress: [ssn]  on_breach: halt }\n";
const FLOW: &str = "flow Publish(rec: PatientRecord) -> String {\n\
    \x20   declassify HIPAA from rec -> ClinicalNote via SafeHarbor\n\
    \x20   step Emit { given: rec  ask: \"publish\"  output: String }\n}\n";

/// A complete determination: subject, basis, both residual judgements, signer,
/// date.
fn attestation(body: &str) -> String {
    format!("attest Determination {{ {body} }}\n")
}

const COMPLETE: &str = "for: ClinicalNote  basis: safe_harbor  \
                        residual: [other_unique_identifier, actual_knowledge]  \
                        by: \"Compliance Officer, Acme Health\"  on: \"2026-08-21\"";

// ── the shape that must pass ────────────────────────────────────────────────

#[test]
fn a_signed_determination_lets_the_declassification_through() {
    let src = format!("{TYPES}{SHIELD}{}{FLOW}", attestation(COMPLETE));
    let errors = diagnostics(&src);
    assert!(
        errors.is_empty(),
        "a complete determination is the legitimate shape and must cost nothing: {errors:#?}"
    );
}

#[test]
fn an_expert_determination_needs_no_residual_list() {
    // THE ASYMMETRY, AND IT IS DELIBERATE. Under Safe Harbor the compiler proved
    // seventeen classes and knows exactly what is left, so it demands a
    // signature over precisely that remainder. Under expert determination it
    // proved nothing about the content — a person took on the whole analysis —
    // so demanding they enumerate this compiler's leftovers would be theatre.
    let src = format!(
        "{TYPES}{SHIELD}{}{FLOW}",
        attestation(
            "for: ClinicalNote  basis: expert_determination  \
             by: \"K. Rivera, PhD, biostatistics\"  on: \"2026-08-21\""
        )
    );
    let errors = diagnostics(&src);
    assert!(
        errors.is_empty(),
        "the second legal route must be expressible: {errors:#?}"
    );
}

// ── and the ones that must not ──────────────────────────────────────────────

#[test]
fn a_declassification_with_no_attestation_is_refused() {
    // THE LAW THAT MAKES THE REST HONEST. Without it a program could retire
    // HIPAA from a value with no human anywhere in the record, and the artifact
    // would carry a clean type and no author.
    let src = format!("{TYPES}{SHIELD}{FLOW}");
    let found = hits(&src, "axon-T1234");
    assert_eq!(found.len(), 1, "expected exactly one refusal: {found:#?}");
    assert!(
        found[0].contains("164.514(b)(2)(i)(R)") && found[0].contains("164.514(b)(2)(ii)"),
        "the diagnostic must name WHICH obligations the compiler could not \
         discharge, or the author reads it as bureaucracy: {}",
        found[0]
    );
}

#[test]
fn an_attestation_about_another_type_does_not_cover_this_one() {
    // Matching is by SUBJECT, not by presence. A determination about some other
    // type is not evidence about this one — and matching loosely would let a
    // single attestation anywhere in the program satisfy every declassification
    // in it, which is the rubber stamp this law exists to refuse.
    let src = format!(
        "{TYPES}type OtherNote {{ text: String }}\n{SHIELD}{}{FLOW}",
        attestation(
            "for: OtherNote  basis: safe_harbor  \
             residual: [other_unique_identifier, actual_knowledge]  \
             by: \"Compliance Officer\"  on: \"2026-08-21\""
        )
    );
    assert_eq!(
        hits(&src, "axon-T1234").len(),
        1,
        "an attestation over a different type must not satisfy this step"
    );
}

#[test]
fn a_basis_the_regulation_does_not_define_is_refused() {
    // An invented basis produces a document that LOOKS like a determination and
    // cites nothing — the exact failure mode a signature is supposed to prevent.
    let src = format!(
        "{TYPES}{SHIELD}{}{FLOW}",
        attestation(
            "for: ClinicalNote  basis: best_effort  by: \"Someone\"  on: \"2026-08-21\""
        )
    );
    let found = hits(&src, "axon-T1231");
    assert_eq!(found.len(), 1, "expected one refusal: {found:#?}");
    assert!(
        found[0].contains("safe_harbor") && found[0].contains("expert_determination"),
        "the refusal must show the two real routes: {}",
        found[0]
    );
}

#[test]
fn safe_harbor_requires_both_judgements_the_compiler_left_open() {
    // A PARTIAL SIGNATURE IS WORSE THAN NONE, because it reads as diligence.
    // The residual list is the remainder of a subtraction: seventeen classes
    // decided from the type graph, two left. Taking on one of the two leaves a
    // hole in a specific, citable place — and 164.514(b)(2)(ii) disqualifies the
    // whole method when it is the unsigned one.
    let src = format!(
        "{TYPES}{SHIELD}{}{FLOW}",
        attestation(
            "for: ClinicalNote  basis: safe_harbor  residual: [other_unique_identifier]  \
             by: \"Compliance Officer\"  on: \"2026-08-21\""
        )
    );
    let found = hits(&src, "axon-T1232");
    assert_eq!(found.len(), 1, "expected one refusal: {found:#?}");
    assert!(
        found[0].contains("actual_knowledge") && found[0].contains("164.514(b)(2)(ii)"),
        "the refusal must name the missing judgement and its citation: {}",
        found[0]
    );
}

#[test]
fn a_judgement_the_compiler_never_left_open_is_refused() {
    // Signing something outside the remainder claims to close a gap that was
    // never there, while the real gap stays open.
    let src = format!(
        "{TYPES}{SHIELD}{}{FLOW}",
        attestation(
            "for: ClinicalNote  basis: safe_harbor  \
             residual: [other_unique_identifier, actual_knowledge, statistical_disclosure]  \
             by: \"Compliance Officer\"  on: \"2026-08-21\""
        )
    );
    assert_eq!(
        hits(&src, "axon-T1232").len(),
        1,
        "a residual judgement outside the catalogue must be refused"
    );
}

#[test]
fn an_unsigned_or_undated_determination_is_not_one() {
    // Its entire value is that a named person took responsibility at a knowable
    // moment. Strip either and what remains is a keyword.
    let unsigned = format!(
        "{TYPES}{SHIELD}{}{FLOW}",
        attestation(
            "for: ClinicalNote  basis: safe_harbor  \
             residual: [other_unique_identifier, actual_knowledge]  on: \"2026-08-21\""
        )
    );
    assert_eq!(hits(&unsigned, "axon-T1233").len(), 1, "no signer");

    let undated = format!(
        "{TYPES}{SHIELD}{}{FLOW}",
        attestation(
            "for: ClinicalNote  basis: safe_harbor  \
             residual: [other_unique_identifier, actual_knowledge]  by: \"Officer\""
        )
    );
    assert_eq!(hits(&undated, "axon-T1233").len(), 1, "no date");

    // And a date that is not one. A determination describes the world on a day;
    // re-identification risk is exactly what changes over time, so a
    // determination that cannot be dated cannot be superseded.
    let vague = format!(
        "{TYPES}{SHIELD}{}{FLOW}",
        attestation(
            "for: ClinicalNote  basis: safe_harbor  \
             residual: [other_unique_identifier, actual_knowledge]  by: \"Officer\"  \
             on: \"last spring\""
        )
    );
    assert_eq!(hits(&vague, "axon-T1233").len(), 1, "not a date");
}

#[test]
fn a_determination_about_a_type_that_does_not_exist_certifies_nothing() {
    // And it would satisfy `axon-T1234` — a law about presence is only as strong
    // as the check that the subject is real.
    let src = format!(
        "{TYPES}{SHIELD}{}{FLOW}",
        attestation(
            "for: GhostNote  basis: safe_harbor  \
             residual: [other_unique_identifier, actual_knowledge]  \
             by: \"Officer\"  on: \"2026-08-21\""
        )
    );
    assert_eq!(hits(&src, "axon-T1233").len(), 1, "the subject must exist");
}

// ── the fact about the world that a reduction leans on ──────────────────────

#[test]
fn a_three_digit_postal_code_cites_the_census_it_relied_on() {
    // Safe Harbor (B) permits three digits only where the unit holds 20,000
    // people or more, and requires `000` where it does not. Which units those
    // are changes between censuses and appears nowhere in the program, so the
    // compiler cannot decide it — but it can refuse to let the assumption stay
    // invisible. The deployment names the source; the compiler checks that it
    // was named, never that it is true.
    let geo_shield = "shield GeoShield { scan: [pii_leak]  compliance: [HIPAA]  \
                      declassifies: [HIPAA]  generalise: [geography]  on_breach: halt }\n";

    let uncited = format!("{TYPES}{geo_shield}");
    let found = hits(&uncited, "axon-T1235");
    assert_eq!(found.len(), 1, "expected one refusal: {found:#?}");
    assert!(
        found[0].contains("20,000") && found[0].contains("164.514(b)(2)(i)(B)"),
        "the refusal must say what fact is missing and where it comes from: {}",
        found[0]
    );

    let cited = format!(
        "{TYPES}{geo_shield}manifest Prod {{ region: \"us-east-1\"  \
         compliance: [HIPAA]  census: us_2020_decennial }}\n"
    );
    assert!(
        hits(&cited, "axon-T1235").is_empty(),
        "a named census satisfies the law: {:#?}",
        hits(&cited, "axon-T1235")
    );

    // A citation nobody can look up is not a citation.
    let invented = format!(
        "{TYPES}{geo_shield}manifest Prod {{ region: \"us-east-1\"  \
         compliance: [HIPAA]  census: our_internal_estimate }}\n"
    );
    assert_eq!(
        hits(&invented, "axon-T1235").len(),
        1,
        "an unrecognised census source must be refused"
    );
}

#[test]
fn a_shield_that_does_not_reduce_geography_owes_no_census() {
    // The obligation is created by the operation, not by the regime. A program
    // that never emits a three-digit postal code has no census assumption to
    // declare, and charging it one would be the kind of ambient cost that
    // teaches adopters to route around the feature.
    let src = format!("{TYPES}{SHIELD}");
    assert!(
        hits(&src, "axon-T1235").is_empty(),
        "no geography reduction, no census obligation"
    );
}
