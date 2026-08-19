//! v2.21.0 — cross-parser parity for the `now() ± interval` time
//! form in `where:` clauses.
//!
//! There are, by deliberate design, TWO parsers for the `where:`
//! grammar:
//!
//!  - the RUNTIME compiler `axon::store::filter::parse_filter`
//! (`axon-rs/src/store/filter.rs`, v1.30.0 + v2.21.0) — renders the
//!    parameterized Postgres `WHERE` clause, interpolating `${param}`;
//!  - the PROOF scanner `axon_frontend::store_column_proof::scan_where`
//! (v1.31.0) — keeps `${param}` symbolic so the type-checker can prove
//!    a parameter's declared type against the column's declared type at
//!    `axon check` time, which the interpolating runtime parser cannot.
//!
//! Two parsers means drift risk: v2.21.0 added the `now() ± interval` time
//! form to the runtime parser; v2.21.0 mirrors it into the proof
//! scanner. This test pins the two in lockstep on that time surface — a
//! shape one accepts and the other rejects fails CI. It is the
//! executable form of the "published grammar must compile" discipline:
//! the compile-time validator and the runtime renderer agree on EXACTLY
//! which `now()` shapes are well-formed.

use axon::store::filter::parse_filter;
use axon_frontend::store_column_proof::{scan_where, ScanError};
use std::collections::HashMap;

/// `parse_filter` accepts the expr (empty bindings — the time form
/// carries no `${param}`, so interpolation is a no-op).
fn runtime_ok(expr: &str) -> bool {
    parse_filter(expr, &HashMap::new()).is_ok()
}

/// `scan_where` accepts the expr.
fn scan_ok(expr: &str) -> bool {
    scan_where(expr).is_ok()
}

/// Well-formed time forms BOTH parsers must accept.
const VALID_TIME: &[&str] = &[
    "t < now()",
    "t >= now()",
    "t != now()",
    "t < now() - interval '30 minutes'",
    "t < now() - interval '30 minute'",
    "t > now() + interval '7 days'",
    "t <= now() - interval '1 hour'",
    "t >= now() - interval '2 weeks'",
    "t < now() - interval '90 seconds'",
    "t < now() - interval '1 month'",
    "t < now() - interval '3 years'",
    "status == 'ACTIVE' AND last_activity_at < now() - interval '30 minutes'",
];

/// Malformed `now`-led time forms BOTH parsers must reject.
const BAD_TIME: &[&str] = &[
    "t < now( - interval '5 minutes'",      // missing `)`
    "t < now() - interval '30'",            // no unit
    "t < now() - interval '30 fortnights'", // unknown unit
    "t < now() - interval 'abc minutes'",   // non-integer amount
    "t < now() - interval '-5 minutes'",    // negative amount
    "t < now() - 'interval string'",        // no `interval` keyword
    "t LIKE now()",                         // LIKE against a time value
    "t < now() - interval '5 minutes); DROP TABLE x;--'", // injection attempt
];

#[test]
fn valid_time_forms_are_accepted_by_both_parsers() {
    for expr in VALID_TIME {
        assert!(
            runtime_ok(expr),
            "runtime parse_filter rejected a valid time form: {expr:?}"
        );
        assert!(
            scan_ok(expr),
            "proof scan_where rejected a valid time form: {expr:?}"
        );
    }
}

#[test]
fn malformed_time_forms_are_rejected_by_both_parsers() {
    for expr in BAD_TIME {
        assert!(
            !runtime_ok(expr),
            "runtime parse_filter accepted a malformed time form: {expr:?}"
        );
        assert!(
            !scan_ok(expr),
            "proof scan_where accepted a malformed time form: {expr:?}"
        );
    }
}

#[test]
fn the_proof_scanner_classifies_malformed_time_as_bad_time_value() {
    // Once a value opens with `now`, a malformed continuation is
    // UNAMBIGUOUSLY a bad time value — the scanner returns the dedicated
    // `BadTimeValue` (which `check_filter` surfaces as a hard axon-T806),
    // not the silently-swallowed generic `Malformed`.
    for expr in BAD_TIME {
        match scan_where(expr) {
            Err(ScanError::BadTimeValue { .. }) => {}
            other => panic!("{expr:?} → expected ScanError::BadTimeValue, got {other:?}"),
        }
    }
}

#[test]
fn the_two_parsers_agree_on_a_broad_corpus() {
    // A wider net mixing time + non-time forms: whatever the runtime
    // accepts on this corpus, the scanner accepts, and vice versa. (The
    // corpus is restricted to the shapes both parsers fully model — no
    // `${param}` here, since the scanner keeps those symbolic while the
    // runtime interpolates, which is an intended semantic difference,
    // not a drift.)
    const CORPUS: &[&str] = &[
        // valid non-time
        "id = 1",
        "name = 'Alice'",
        "active = true",
        "deleted_at = null",
        "a = 1 AND b = 2",
        "a = 1 OR b = 2",
        "score > 3.14",
        // valid time
        "t < now()",
        "t < now() - interval '30 minutes'",
        // invalid (structural)
        "id = 1 AND",
        "a = 1 b = 2",
        "1 = 1",
        // invalid (time)
        "t < now( - interval '5 minutes'",
        "t < now() - interval '30 fortnights'",
        "t LIKE now()",
    ];
    for expr in CORPUS {
        assert_eq!(
            runtime_ok(expr),
            scan_ok(expr),
            "parser disagreement on {expr:?}: runtime_ok={}, scan_ok={}",
            runtime_ok(expr),
            scan_ok(expr),
        );
    }
}
