//! v4.5.0 — what a declassification actually DOES to the data.
//!
//! The previous step made retiring a class an act with three refusals, and the
//! sharpest of them checks that the destination type no longer carries the
//! class. That is necessary and not sufficient: Safe Harbor is a statement
//! about IDENTIFIERS, and a record whose `compliance:` list is empty while it
//! still carries a medical record number is de-identified in name only.
//!
//! So the shield declares what it does, and the compiler checks the result
//! against the regulation's own table:
//!
//! - `suppress: [<kind>, …]` — kinds removed entirely.
//! - `generalise: [<kind>, …]` — kinds KEPT in reduced form.
//!
//! # Kinds, not field names
//!
//! A control naming `[mrn, ssn]` would be welded to one type's spelling and
//! useless on the next. A control naming `[medical_record_number, ssn]`
//! describes what it does to DATA and is reusable wherever that data appears —
//! and it is the only form the catalogue can check, because field names are
//! prose.
//!
//! # The shield says which, the regime says how
//!
//! `generalise: [date]` does not say *"to its year"*. `HIPAA_SAFE_HARBOR` says
//! that, citing 164.514(b)(2)(i)(C). An author who could restate the operation
//! could restate it wrong, and the regulation is the source.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn hits(src: &str, code: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e| e.message)
        .filter(|m| m.contains(code))
        .collect()
}

const KINDS: &str = "type BirthDate identifier date { value: String }\n\
                     type SSN identifier ssn { value: String }\n";

fn program(dest: &str, shield_ops: &str) -> String {
    format!(
        "{KINDS}\
         type PatientRecord compliance [HIPAA] {{ dob: BirthDate  s: SSN }}\n\
         {dest}\n\
         shield SafeHarbor {{ scan: [pii_leak]  compliance: [HIPAA]  \
         declassifies: [HIPAA] {shield_ops} on_breach: halt }}\n\
         flow Publish(rec: PatientRecord) -> String {{\n    \
         declassify HIPAA from rec -> Dest via SafeHarbor\n    \
         step Emit {{ given: rec  ask: \"x\"  output: String }}\n}}\n"
    )
}

// ── the regulation lets some data survive ───────────────────────────────────

#[test]
fn a_kept_date_is_legitimate_when_the_shield_reduces_it() {
    // THE PROPERTY a masking-only mechanism cannot express. Safe Harbor does
    // not ask you to delete the date; it asks for the year. An adopter who can
    // keep the year keeps analysable data — and a system that costs more than
    // the law is a system that gets routed around.
    let src = program("type Dest { dob: BirthDate }", "suppress: [ssn]  generalise: [date]");
    assert!(
        hits(&src, "axon-T1229").is_empty(),
        "keeping a date the shield reduces is exactly what the regulation permits: {:#?}",
        hits(&src, "axon-T1229")
    );
}

#[test]
fn a_kept_date_the_shield_does_not_reduce_is_refused() {
    let src = program("type Dest { dob: BirthDate }", "suppress: [ssn]");
    let found = hits(&src, "axon-T1229");
    assert_eq!(found.len(), 1, "an unreduced date must be refused: {found:#?}");
    assert!(
        found[0].contains("date") && found[0].contains("generalise"),
        "the diagnostic must name the kind AND the field that would fix it: {}",
        found[0]
    );
}

// ── and some it does not ────────────────────────────────────────────────────

#[test]
fn a_kept_identifier_with_no_reduced_form_is_refused_even_if_declared() {
    // `generalise: [ssn]` is a claim the regulation does not allow: there is no
    // shortened social-security number that is not still one. The shield saying
    // it does not make it so — which is the difference between a control and a
    // declaration.
    let src = program("type Dest { s: SSN }", "generalise: [ssn]");
    let found = hits(&src, "axon-T1229");
    assert_eq!(found.len(), 1, "a kept SSN must be refused: {found:#?}");
    assert!(
        found[0].contains("REMOVED"),
        "the diagnostic must say the kind has to go, not be shortened: {}",
        found[0]
    );
}

#[test]
fn a_destination_that_keeps_no_identifier_needs_no_operations_declared() {
    // The common case: the destination simply does not have the fields. The law
    // checks what SURVIVES, so a clean destination passes without the shield
    // enumerating everything it dropped.
    let src = program("type Dest { note: String }", "suppress: [ssn]");
    assert!(
        hits(&src, "axon-T1229").is_empty(),
        "a destination carrying no identifiers is clean: {:#?}",
        hits(&src, "axon-T1229")
    );
}

// ── the control must have a mechanism ───────────────────────────────────────

#[test]
fn a_shield_that_retires_a_class_while_doing_nothing_is_refused() {
    // This law checks something the others cannot: they all check the SHAPE of
    // the result, and a destination type with no identifier fields satisfies
    // every one of them. A shield that removes and reduces nothing would sail
    // through — a rubber stamp with a keyword.
    let src = program("type Dest { note: String }", "");
    let found = hits(&src, "axon-T1230");
    assert_eq!(found.len(), 1, "a shield with no mechanism must be refused: {found:#?}");
    assert!(
        found[0].contains("rubber stamp"),
        "the diagnostic should say plainly what it is: {}",
        found[0]
    );
}

#[test]
fn an_ordinary_shield_declares_nothing_and_pays_nothing() {
    // Almost every shield scans and never declassifies. None of this may cost
    // them anything, or the rule becomes a tax on the common case.
    let src = "shield Plain { scan: [pii_leak]  on_breach: halt }\n";
    assert!(
        hits(src, "axon-T1230").is_empty(),
        "a scanning shield must be untouched by the declassification laws"
    );
}

#[test]
fn the_operations_name_kinds_and_a_field_name_is_refused() {
    let src = program("type Dest { note: String }", "suppress: [patient_ssn_field]");
    let found = hits(&src, "axon-T1230");
    assert_eq!(found.len(), 1, "a field name is not an identifier kind: {found:#?}");
    assert!(
        found[0].contains("KINDS"),
        "the diagnostic must explain the distinction, since the mistake is natural: {}",
        found[0]
    );
}

// ── the regime table is the source of the operation ─────────────────────────

#[test]
fn the_shield_says_which_kind_and_the_regulation_says_how() {
    use axon_frontend::compliance::{safe_harbor_rule, Deidentify, Generalisation};

    // A shield writes `generalise: [date]` and nothing about years. If an author
    // could restate the operation they could restate it wrong, and the
    // regulation is the source — so the operation is looked up, never declared.
    assert_eq!(
        safe_harbor_rule("date").map(|r| r.operation),
        Some(Deidentify::Generalise(Generalisation::Year))
    );
    assert_eq!(
        safe_harbor_rule("geography").map(|r| r.operation),
        Some(Deidentify::Generalise(Generalisation::Zip3))
    );
    assert_eq!(
        safe_harbor_rule("ssn").map(|r| r.operation),
        Some(Deidentify::Suppress)
    );
}
