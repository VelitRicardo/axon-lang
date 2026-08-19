//! v2.21.0 — cross-parser parity for the bounded/ordered `retrieve`
//! clauses (`order_by:` / `limit:`).
//!
//! As with the `where:` grammar (v2.21.0), the clause is validated in TWO
//! places: the RUNTIME renderer `axon::store::filter::{parse_order_by,
//! parse_limit}` (which builds the `ORDER BY … LIMIT …` SQL suffix) and
//! the COMPILE-TIME proof `axon_frontend::store_column_proof::check_bounds`
//! (which surfaces `axon-T807` / `axon-T808` at `axon check`). This test
//! pins the two on a corpus of order_by + literal-limit shapes — a shape
//! one accepts and the other rejects fails CI.
//!
//! Scope note: a `${param}` limit is validated differently by the two
//! (the runtime resolves a binding VALUE to a `u32`; the proof checks the
//! PARAMETER's declared type) — an intended semantic difference, so the
//! limit corpus here is restricted to literals (each crate covers the
//! binding/param case in its own unit tests).

use axon::store::filter::{parse_limit, parse_order_by};
use axon_frontend::store_column_proof::{check_bounds, FlowParamTypes};
use std::collections::HashMap;

/// The runtime accepts this `order_by:`.
fn runtime_order_ok(ob: &str) -> bool {
    parse_order_by(ob).is_ok()
}

/// The proof accepts this `order_by:` (structural — no schema, so no
/// column-existence check, matching the runtime which has no schema).
fn proof_order_ok(ob: &str) -> bool {
    check_bounds(ob, "", None, &FlowParamTypes::default(), (1, 1)).is_empty()
}

fn runtime_limit_ok(lim: &str) -> bool {
    parse_limit(lim, &HashMap::new()).is_ok()
}

fn proof_limit_ok(lim: &str) -> bool {
    check_bounds("", lim, None, &FlowParamTypes::default(), (1, 1)).is_empty()
}

const ORDER_BY: &[&str] = &[
    // valid
    "",
    "id",
    "id asc",
    "id DESC",
    "last_activity_at desc, id asc",
    "a, b, c",
    // invalid
    "id sideways",
    "id asc desc",
    "a,,b",
    "id; DROP TABLE x",
    "1id asc",
    "id ascending",
];

const LIMIT_LITERAL: &[&str] = &[
    // valid
    "", "0", "1", "100", "4294967295", // u32::MAX
    // invalid
    "-1", "abc", "3.5", "4294967296", // u32::MAX + 1
    "1 OR 1=1", "100; DROP TABLE x",
];

#[test]
fn order_by_parsers_agree() {
    for ob in ORDER_BY {
        assert_eq!(
            runtime_order_ok(ob),
            proof_order_ok(ob),
            "order_by disagreement on {ob:?}: runtime={}, proof={}",
            runtime_order_ok(ob),
            proof_order_ok(ob),
        );
    }
}

#[test]
fn limit_literal_parsers_agree() {
    for lim in LIMIT_LITERAL {
        assert_eq!(
            runtime_limit_ok(lim),
            proof_limit_ok(lim),
            "limit disagreement on {lim:?}: runtime={}, proof={}",
            runtime_limit_ok(lim),
            proof_limit_ok(lim),
        );
    }
}

#[test]
fn valid_shapes_are_accepted_and_invalid_rejected_by_both() {
    // Sanity anchors so a parser that rejects EVERYTHING can't pass the
    // agreement test vacuously.
    assert!(runtime_order_ok("last_activity_at desc, id asc"));
    assert!(proof_order_ok("last_activity_at desc, id asc"));
    assert!(!runtime_order_ok("id sideways"));
    assert!(!proof_order_ok("id sideways"));
    assert!(runtime_limit_ok("100"));
    assert!(proof_limit_ok("100"));
    assert!(!runtime_limit_ok("-1"));
    assert!(!proof_limit_ok("-1"));
}
