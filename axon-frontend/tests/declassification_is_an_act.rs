//! v4.5.0 — retiring a regulatory class is an ACT, and the compiler refuses the
//! ones that are structurally impossible.
//!
//! The previous cycle closed the egress border: six of seven exits require a
//! control covering the data's classes. Closing a border without opening a
//! legitimate door does not remove the traffic — it pushes it to the side, and
//! the side channel is trivial: copy the fields into an unlabelled type. κ does
//! not follow intra-expression flow, so it never sees that.
//!
//! An adopter who de-identifies legitimately — HIPAA Safe Harbor is literally
//! *"this is no longer PHI"* — had no way to say so. They would invent one.
//!
//! # What the compiler does and does not decide
//!
//! It does NOT decide that a value stopped being PHI. It refuses a claim that
//! cannot be true, and lets a possible one through with the author's name on
//! it. Three refusals:
//!
//! - `axon-T1226` — the class is not in Κ. Retiring something that was never a
//!   class is an assertion about nothing, which every coverage law downstream
//!   would then find satisfied.
//! - `axon-T1227` — the shield does not declare the capability. Scanning is not
//!   declassifying: a control that can END a regulatory obligation says so, and
//!   one that merely inspects content must not acquire the power by being named.
//! - `axon-T1228` — the destination type still carries the class. This is the
//!   one that makes the verb worth having.
//!
//! The verb NAMES the class rather than leaving it to the shield, so a reader of
//! the flow sees what was retired without going to look it up.

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
    diagnostics(src).into_iter().filter(|m| m.contains(code)).collect()
}

const SOURCE_TYPE: &str = "type PatientRecord compliance [HIPAA] { ssn: String }\n";
const CLEAN_TYPE: &str = "type ClinicalNote { note: String }\n";
const AUTHORISED: &str =
    "shield SafeHarbor { scan: [pii_leak]  compliance: [HIPAA]  declassifies: [HIPAA]  suppress: [ssn]  on_breach: halt }\n";

/// v4.5.0 — the signature over what the compiler could not decide.
///
/// Every legitimate declassification now carries one (`axon-T1234`), so the
/// positive fixtures below include it. Its own laws live in
/// `an_attestation_is_signed_or_it_is_nothing.rs`.
const ATTESTED: &str = "attest SafeHarborDetermination { for: ClinicalNote  basis: safe_harbor  residual: [other_unique_identifier, actual_knowledge]  by: \"Compliance Officer, Acme Health\"  on: \"2026-08-21\" }\n";

fn flow(stmt: &str) -> String {
    format!(
        "flow Publish(rec: PatientRecord) -> String {{\n    {stmt}\n    \
         step Emit {{ given: rec  ask: \"publish\"  output: String }}\n}}\n"
    )
}

// ── the shape that must pass ────────────────────────────────────────────────

#[test]
fn a_well_formed_declassification_compiles() {
    let src = format!(
        "{SOURCE_TYPE}{CLEAN_TYPE}{AUTHORISED}{ATTESTED}{}",
        flow("declassify HIPAA from rec -> ClinicalNote via SafeHarbor")
    );
    let errors = diagnostics(&src);
    assert!(
        errors.is_empty(),
        "the legitimate door must open, or adopters keep using the side channel: {errors:#?}"
    );
}

// ── and the three that must not ─────────────────────────────────────────────

#[test]
fn a_destination_that_still_carries_the_class_is_refused() {
    // THE LAW THAT MAKES THE VERB WORTH HAVING. You cannot declare a record
    // de-identified while the type you produce still says it is PHI — and the
    // boundary downstream reads the type, not the intent.
    let src = format!(
        "{SOURCE_TYPE}type StillPHI compliance [HIPAA] {{ note: String }}\n{AUTHORISED}{}",
        flow("declassify HIPAA from rec -> StillPHI via SafeHarbor")
    );
    let found = hits(&src, "axon-T1228");
    assert_eq!(found.len(), 1, "a still-regulated result must be refused: {found:#?}");
    assert!(
        found[0].contains("StillPHI"),
        "the diagnostic must name the type that failed to shed the class: {}",
        found[0]
    );
}

#[test]
fn a_shield_that_only_scans_cannot_authorise_a_declassification() {
    // Scanning is not declassifying. A control that merely inspects content
    // must not acquire the power to end a regulatory obligation by being named
    // in this position.
    let src = format!(
        "{SOURCE_TYPE}{CLEAN_TYPE}\
         shield JustScans {{ scan: [pii_leak]  compliance: [HIPAA]  on_breach: halt }}\n{}",
        flow("declassify HIPAA from rec -> ClinicalNote via JustScans")
    );
    let found = hits(&src, "axon-T1227");
    assert_eq!(found.len(), 1, "an unauthorised shield must be refused: {found:#?}");
    assert!(
        found[0].contains("declassifies"),
        "the diagnostic must name the missing capability, so the fix is obvious: {}",
        found[0]
    );
}

#[test]
fn a_class_outside_the_vocabulary_is_refused_with_a_suggestion() {
    let src = format!(
        "{SOURCE_TYPE}{CLEAN_TYPE}{AUTHORISED}{}",
        flow("declassify HIPPA from rec -> ClinicalNote via SafeHarbor")
    );
    let found = hits(&src, "axon-T1226");
    assert_eq!(found.len(), 1, "an unknown class must be refused: {found:#?}");
    assert!(
        found[0].contains("HIPAA"),
        "someone who typed HIPPA needs the spelling, not a rejection: {}",
        found[0]
    );
}

#[test]
fn a_shield_that_does_not_exist_is_refused() {
    let src = format!(
        "{SOURCE_TYPE}{CLEAN_TYPE}{}",
        flow("declassify HIPAA from rec -> ClinicalNote via Nowhere")
    );
    assert_eq!(hits(&src, "axon-T1227").len(), 1, "an undeclared shield must be refused");
}

// ── partial declassification, from the first release ────────────────────────

#[test]
fn retiring_one_class_leaves_the_others_standing() {
    // A real pipeline retires HIPAA and keeps GDPR. All-or-nothing would push
    // adopters back to the side channel this verb exists to close, so partial
    // declassification is not a later feature.
    let src = format!(
        "type Both compliance [HIPAA, GDPR] {{ ssn: String }}\n\
         type StillGDPR compliance [GDPR] {{ note: String }}\n{AUTHORISED}\
         flow Publish(rec: Both) -> String {{\n    \
         declassify HIPAA from rec -> StillGDPR via SafeHarbor\n    \
         step Emit {{ given: rec  ask: \"publish\"  output: String }}\n}}\n"
    );
    assert!(
        hits(&src, "axon-T1228").is_empty(),
        "retiring HIPAA while GDPR remains is legitimate — the destination sheds \
         the class being retired, not every class: {:#?}",
        hits(&src, "axon-T1228")
    );
}

#[test]
fn the_class_being_retired_must_actually_be_shed() {
    // The other half of the same property: retiring HIPAA from a type that still
    // carries HIPAA is refused even though it sheds GDPR.
    let src = format!(
        "type Both compliance [HIPAA, GDPR] {{ ssn: String }}\n\
         type StillHIPAA compliance [HIPAA] {{ note: String }}\n{AUTHORISED}\
         flow Publish(rec: Both) -> String {{\n    \
         declassify HIPAA from rec -> StillHIPAA via SafeHarbor\n    \
         step Emit {{ given: rec  ask: \"publish\"  output: String }}\n}}\n"
    );
    assert_eq!(
        hits(&src, "axon-T1228").len(),
        1,
        "shedding a DIFFERENT class is not shedding this one"
    );
}

// ── a wrapper does not hide the class here either ───────────────────────────

#[test]
fn a_destination_wrapping_a_regulated_type_is_still_regulated() {
    // The law reads `transitive_kappa`, the same walk the boundary rules read.
    // If it had grown its own κ computation, this is where the copies would
    // part company — and the escape would be one struct deep.
    let src = format!(
        "{SOURCE_TYPE}\
         type Inner compliance [HIPAA] {{ ssn: String }}\n\
         type Wrapper {{ inner: Inner }}\n{AUTHORISED}{}",
        flow("declassify HIPAA from rec -> Wrapper via SafeHarbor")
    );
    assert_eq!(
        hits(&src, "axon-T1228").len(),
        1,
        "wrapping the regulated type in a struct must not pass as de-identification"
    );
}

// ── the grammar refuses an incomplete act ───────────────────────────────────

#[test]
fn every_part_of_the_statement_is_required() {
    // An apply-step tolerates a missing target because it still means
    // something. A declassification with no source, no destination type or no
    // authorising shield does not — it would be an assertion about nothing that
    // the coverage laws would then honour.
    for (stmt, want) in [
        ("declassify HIPAA rec -> ClinicalNote via SafeHarbor", "from"),
        ("declassify HIPAA from rec via SafeHarbor", "-> <Type>"),
        ("declassify HIPAA from rec -> ClinicalNote", "via <Shield>"),
    ] {
        let src = format!("{SOURCE_TYPE}{CLEAN_TYPE}{AUTHORISED}{}", flow(stmt));
        let tokens = Lexer::new(&src, "<test>").tokenize().expect("lex");
        let err = Parser::new(tokens)
            .parse()
            .expect_err(&format!("`{stmt}` must not parse"));
        assert!(
            err.message.contains(want),
            "the parse error must say what is missing — `{want}` — so the author can \
             fix it without reading the grammar: {}",
            err.message
        );
    }
}
