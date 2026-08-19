//! v1.32.0 (D2) — The Request Binding Contract: compile-time totality.
//!
//! An `axonendpoint` declares `body: T` and `execute: F`. The Request
//! Binding Contract binds each flow parameter from the same-named body
//! field (37.b). D2 makes the binding a COMPILE-TIME THEOREM: the
//! type-checker proves every REQUIRED parameter of F is covered by a
//! field of T — by name, type-compatible. An uncovered required
//! parameter is a compile error, so the failure moves from a
//! production request to `axon check`.
//!
//! This is what no mainstream framework offers — FastAPI / Spring /
//! NestJS bind a typed body to a handler and discover a missing field
//! at runtime. AXON proves the binding total before the endpoint can
//! deploy.
//!
//! Pins:
//! section 1 — a required parameter with NO covering field → compile error.
//! section 2 — a required parameter covered by name + type → clean.
//! section 3 — a covering field of the WRONG type → compile error.
//! section 4 — an OPTIONAL parameter need not be covered → clean.
//! section 5 — no `body:` declared → D2 does not fire (honest scope: the
//!        binding is then untyped/best-effort).
//! section 6 — the multi-parameter agent shape, fully covered → clean.
//! section 7 — partial coverage: exactly the uncovered parameter is named.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

/// The v1.32.0 D2 totality errors only — every binding-contract
/// diagnostic carries the phrase "Request Binding Contract".
fn d2_errors(src: &str) -> Vec<String> {
    errors(src)
        .into_iter()
        .filter(|m| m.contains("Request Binding Contract"))
        .collect()
}

// ─── section 1 — an uncovered required parameter is a compile error ────────

#[test]
fn s1_uncovered_required_parameter_is_a_compile_error() {
    let src = "type Body1 { other: String }\n\
        flow Flow1(message: String) -> Unit { step S { ask: \"x\" output: String } }\n\
        axonendpoint E1 { method: POST path: \"/e1\" \
            body: Body1 execute: Flow1 backend: stub }";
    let errs = d2_errors(src);
    assert!(
        errs.iter().any(|m| m.contains("'message:")),
        "v1.32.0 D2 — a required flow parameter (`message`) with no \
         same-named field in the declared `body:` type must be a \
         COMPILE ERROR. Got: {errs:?}"
    );
}

// ─── section 2 — a covered required parameter type-checks clean ────────────

#[test]
fn s2_covered_required_parameter_type_checks_clean() {
    let src = "type Body2 { message: String }\n\
        flow Flow2(message: String) -> Unit { step S { ask: \"x\" output: String } }\n\
        axonendpoint E2 { method: POST path: \"/e2\" \
            body: Body2 execute: Flow2 backend: stub public: true }";
    assert!(
        errors(src).is_empty(),
        "v1.32.0 D2 — a required parameter covered by a same-named, \
         same-type body field must type-check with zero errors. \
         Got: {:?}",
        errors(src)
    );
}

// ─── section 3 — a covering field of the wrong type is a compile error ─────

#[test]
fn s3_type_mismatch_on_a_covering_field_is_a_compile_error() {
    let src = "type Body3 { amount: String }\n\
        flow Flow3(amount: Float) -> Unit { step S { ask: \"x\" output: String } }\n\
        axonendpoint E3 { method: POST path: \"/e3\" \
            body: Body3 execute: Flow3 backend: stub }";
    let errs = d2_errors(src);
    assert!(
        errs.iter().any(|m| m.contains("amount")
            && m.contains("Float")
            && m.contains("String")),
        "v1.32.0 D2 — a body field that covers a parameter by NAME but \
         declares an incompatible TYPE must be a compile error naming \
         both types. Got: {errs:?}"
    );
}

// ─── section 4 — an optional parameter need not be covered ─────────────────

#[test]
fn s4_optional_parameter_need_not_be_covered() {
    let src = "type Body4 { something: String }\n\
        flow Flow4(note: String?) -> Unit { step S { ask: \"x\" output: String } }\n\
        axonendpoint E4 { method: POST path: \"/e4\" \
            body: Body4 execute: Flow4 backend: stub }";
    assert!(
        d2_errors(src).is_empty(),
        "v1.32.0 D2 — an OPTIONAL flow parameter need not be covered by \
         the body type; no D2 error fires. Got: {:?}",
        d2_errors(src)
    );
}

// ─── section 5 — no `body:` declared → D2 does not fire ────────────────────

#[test]
fn s5_no_body_type_means_no_d2_check() {
    // Honest scope: without a declared `body: T` there is no typed
    // contract to prove total against — the runtime binding is then
    // untyped/best-effort. D2 must NOT fire (no false positive).
    let src = "flow Flow5(message: String) -> Unit { step S { ask: \"x\" output: String } }\n\
        axonendpoint E5 { method: POST path: \"/e5\" execute: Flow5 backend: stub }";
    assert!(
        d2_errors(src).is_empty(),
        "v1.32.0 D2 — an endpoint with no `body:` declaration must not \
         trigger the totality check. Got: {:?}",
        d2_errors(src)
    );
}

// ─── section 6 — the multi-parameter agent shape, fully covered ────────────

#[test]
fn s6_multi_parameter_agent_shape_fully_covered_is_clean() {
    let src = "type ChatBody { message: String session_id: String tenant_id: String }\n\
        flow ChatFlow(message: String, session_id: String, tenant_id: String) -> Unit {\n\
            step Deliberate { ask: \"x\" output: String }\n\
        }\n\
        axonendpoint ChatE { method: POST path: \"/chat\" \
            body: ChatBody execute: ChatFlow backend: stub public: true }";
    assert!(
        errors(src).is_empty(),
        "v1.32.0 D2 — the canonical multi-parameter agent endpoint, \
         every parameter covered by a body field, must type-check \
         with zero errors. Got: {:?}",
        errors(src)
    );
}

// ─── section 7 — partial coverage names exactly the uncovered parameter ────

#[test]
fn s7_partial_coverage_names_the_uncovered_parameter() {
    // `message` + `tenant_id` are covered; `session_id` is not.
    let src = "type PartialBody { message: String tenant_id: String }\n\
        flow PartialFlow(message: String, session_id: String, tenant_id: String) -> Unit {\n\
            step S { ask: \"x\" output: String }\n\
        }\n\
        axonendpoint PartialE { method: POST path: \"/partial\" \
            body: PartialBody execute: PartialFlow backend: stub }";
    let errs = d2_errors(src);
    assert_eq!(
        errs.len(),
        1,
        "v1.32.0 D2 — exactly one D2 error: the single uncovered \
         parameter. Got: {errs:?}"
    );
    assert!(
        errs[0].contains("'session_id:"),
        "v1.32.0 D2 — the error must name the uncovered parameter \
         `session_id`, not the covered ones. Got: {errs:?}"
    );
}
