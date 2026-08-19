//! v2.44.0 — `axon-T890` AuthorizationCoverage: the `every_boundary_is_guarded`
//! law at the endpoint boundary.
//!
//! The boundary-coverage audit found Modo 1: an `axonendpoint` with no
//! `requires:`/`shield:`/`compliance:` silently dispatches to any authenticated
//! same-tenant caller. v2.44.0 closes it — a DISPATCHING endpoint must declare a
//! covering discipline OR the explicit, auditable opt-out `public: true`, else
//! `axon check` fails with `axon-T890`. This is a deliberate BREAKING change
//! (Kivi pre-launch); `axon fix` (v2.44.0) migrates existing programs.
//!
//! Pins:
//!   1. A dispatching endpoint with NO coverage and NO `public:` → `axon-T890`.
//!   2. `public: true` clears it (explicit opt-out).
//!   3. `requires: [..]` clears it (capability coverage).
//!   4. `shield: <Name>` clears it (shield coverage).
//!   5. `compliance: [..]` clears it (regulatory coverage).
//!   6. An endpoint with no `execute:` does NOT fire T890 (crosses no boundary).
//!   7. `public: false` explicit + no coverage still fires (false ≠ opt-out).

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

/// Type-check `src` and return the diagnostics as strings.
fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

fn has_t890(src: &str) -> bool {
    errors(src).iter().any(|e| e.contains("axon-T890"))
}

const FLOW: &str = "flow Chat() -> Unit { step S { ask: \"hi\" } }\n";
const SHIELD: &str = "shield Guard { scan: [pii_leak] on_breach: halt }\n";

// ─── section 1 — uncovered + no public ⇒ T890 ─────────────────────────────

#[test]
fn s1_uncovered_endpoint_fires_t890() {
    let src = format!(
        "{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat }}"
    );
    assert!(
        has_t890(&src),
        "89.b: a dispatching endpoint with no coverage + no `public:` must fire \
         axon-T890. Got: {:?}",
        errors(&src)
    );
}

// ─── section 2 — public: true clears it ───────────────────────────────────

#[test]
fn s2_public_true_clears_t890() {
    let src = format!(
        "{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat public: true }}"
    );
    assert!(!has_t890(&src), "89.b: `public: true` must clear T890. Got: {:?}", errors(&src));
}

// ─── section 3/section 4/section 5 — each coverage discipline clears it ─────────────────

#[test]
fn s3_requires_clears_t890() {
    let src = format!(
        "{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat requires: [flow.execute] }}"
    );
    assert!(!has_t890(&src), "89.b: `requires:` coverage must clear T890. Got: {:?}", errors(&src));
}

#[test]
fn s4_shield_clears_t890() {
    let src = format!(
        "{SHIELD}{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat shield: Guard }}"
    );
    assert!(!has_t890(&src), "89.b: `shield:` coverage must clear T890. Got: {:?}", errors(&src));
}

#[test]
fn s5_compliance_clears_t890() {
    let src = format!(
        "{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat compliance: [SOC2] }}"
    );
    assert!(!has_t890(&src), "89.b: `compliance:` coverage must clear T890. Got: {:?}", errors(&src));
}

// ─── section 6 — no execute ⇒ no boundary ⇒ no T890 ───────────────────────

#[test]
fn s6_no_execute_does_not_fire_t890() {
    let src = "axonendpoint E { method: POST path: \"/c\" }";
    assert!(
        !has_t890(src),
        "89.b: an endpoint that dispatches nothing crosses no boundary — no T890. \
         Got: {:?}",
        errors(src)
    );
}

// ─── section 7 — public: false is NOT an opt-out ──────────────────────────

#[test]
fn s7_public_false_still_fires_t890() {
    let src = format!(
        "{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat public: false }}"
    );
    assert!(
        has_t890(&src),
        "89.b: `public: false` is the default, NOT an opt-out — T890 must still fire. \
         Got: {:?}",
        errors(&src)
    );
}
