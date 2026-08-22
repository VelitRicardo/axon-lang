//! v4.5.0 — the runtime half: a declassification actually changes the value.
//!
//! Up to here the compiler decided everything — which class is retired, which
//! control authorises it, what the destination may keep. This is where the data
//! is transformed, and the property that matters is narrow and absolute:
//!
//! **the output is a value the program has declared no longer regulated.**
//!
//! If the transformation is skipped, partially applied, or applied to something
//! it does not understand, the result is a record carrying identifiers that
//! every downstream boundary will now wave through — because the boundary reads
//! the type, and the type says this is clean. There is no safe approximation
//! available, so every fork below fails closed.
//!
//! # The compiler resolved the plan; this executes it
//!
//! `keep` and `generalise` ride the IR node. The runtime does not re-derive
//! them, because re-deriving would mean shipping the type graph to dispatch and
//! hoping two derivations agree — the shape that has cost this project real
//! work every time it appeared.

use axon::shield_registry::{declassify_value, generalise_value};
use serde_json::json;

// ── the projection IS the suppression ───────────────────────────────────────

#[test]
fn a_field_the_destination_does_not_have_cannot_survive() {
    // The destination type has no slot for `ssn`, so the projection removes it —
    // by construction, rather than by someone remembering to name it. That is
    // why `suppress:` on the shield is a declaration of capability and not the
    // mechanism: the mechanism cannot forget.
    let src = json!({ "dob": "1954-03-02", "ssn": "078-05-1120", "note": "stable" });
    let out = declassify_value(&src, &["dob".into(), "note".into()], &[]).expect("projects");

    assert_eq!(out.get("ssn"), None, "the SSN must not survive the projection");
    assert_eq!(out.get("note").and_then(|v| v.as_str()), Some("stable"));
}

// ── the regulation lets some data survive, reduced ──────────────────────────

#[test]
fn a_date_is_reduced_to_its_year_not_deleted() {
    // THE PROPERTY a masking-only mechanism cannot express, and the reason
    // `generalise` exists: Safe Harbor asks for the year, and an adopter who
    // can keep the year keeps analysable data. A system that costs more than
    // the law gets routed around.
    let src = json!({ "dob": "1954-03-02", "note": "stable" });
    let out = declassify_value(
        &src,
        &["dob".into(), "note".into()],
        &[("dob".into(), "year".into())],
    )
    .expect("reduces");

    assert_eq!(
        out.get("dob").and_then(|v| v.as_str()),
        Some("1954"),
        "the day and month go; the year stays"
    );
}

#[test]
fn each_reduction_keeps_what_the_regulation_permits_and_no_more() {
    assert_eq!(generalise_value("year", &json!("1954-03-02")).unwrap(), json!("1954"));
    assert_eq!(generalise_value("zip3", &json!("02139")).unwrap(), json!("021"));
    assert_eq!(generalise_value("age_cap", &json!(92)).unwrap(), json!("90+"));

    // Under the cap, the age is not a narrowing identifier and survives intact.
    assert_eq!(generalise_value("age_cap", &json!(64)).unwrap(), json!("64"));
}

// ── and every fork that cannot be honest, fails ─────────────────────────────

#[test]
fn a_value_that_does_not_have_the_expected_shape_is_refused() {
    // A date that does not begin with four digits cannot be reduced to a year.
    // Passing it through would leave a full date in a record the program calls
    // de-identified — the one outcome this whole line of work exists to
    // prevent, and the one the adopter would only discover from outside.
    let err = generalise_value("year", &json!("March 2nd, 1954")).unwrap_err();
    assert!(
        err.contains("Refusing"),
        "the failure must say it refused rather than approximated: {err}"
    );

    assert!(generalise_value("zip3", &json!("7")).is_err(), "too few digits");
    assert!(
        generalise_value("age_cap", &json!("ninety two")).is_err(),
        "not a whole number of years"
    );
}

#[test]
fn a_scalar_has_no_fields_and_is_refused() {
    let err = declassify_value(&json!("just a string"), &["dob".into()], &[]).unwrap_err();
    assert!(
        err.contains("structured"),
        "a scalar has nothing to project — say so rather than returning it: {err}"
    );
}

#[test]
fn an_operation_this_runtime_does_not_implement_is_refused() {
    // The compiler and the runtime hold two halves of one catalogue. If they
    // drift, this is the seam — and it must not be crossed silently, because a
    // skipped reduction looks exactly like a successful one from downstream.
    let err = generalise_value("k_anonymise", &json!("x")).unwrap_err();
    assert!(
        err.contains("drifted"),
        "an unknown operation means the catalogues disagree, and the message \
         should say which failure this is: {err}"
    );
}

#[test]
fn one_field_that_cannot_be_reduced_fails_the_whole_operation() {
    // Partial success is the dangerous outcome: a record where four fields were
    // reduced and the fifth was skipped is emitted as de-identified and is not.
    let src = json!({ "dob": "1954-03-02", "admitted": "last Tuesday" });
    let out = declassify_value(
        &src,
        &["dob".into(), "admitted".into()],
        &[("dob".into(), "year".into()), ("admitted".into(), "year".into())],
    );
    assert!(
        out.is_err(),
        "one unreducible field must fail the operation, not produce a partly \
         de-identified record"
    );
}
