//! v2.44.0 — Proof-Carrying Code for AuthorizationCoverage: the doctrine
//! `every_boundary_is_guarded` made an INDEPENDENTLY-VERIFIABLE, deploy-gated
//! fact. The v2.44.0 `axon-T890` type-check is now a proof the consumer
//! re-derives from the IR alone (never trusting the compiler): every
//! DISPATCHING `axonendpoint` is authorized (covered by ≥1 discipline OR
//! `public: true`), so the audit's Modo 1 (an unguarded boundary reachable by
//! silent omission) is caught at the deploy gate even if the type-check was
//! bypassed.
//!
//! Tests build the IR via parse → IRGenerator DIRECTLY (bypassing the v2.44.0
//! type-check) so an uncovered-endpoint IR can be constructed and the PCC's
//! independent refutation exercised.
//!
//! Pins:
//!   1. A dispatching `public: true` endpoint → one proof, VERIFIED.
//!   2. `requires:` / `shield:` / `compliance:` each → VERIFIED.
//!   3. An uncovered dispatching endpoint → one proof, REFUTED (T890).
//!   4. A non-dispatching endpoint → NO proof (crosses no boundary).
//!   5. A FORGED witness (`authorized: true` on an uncovered endpoint) is
//!      caught by recomputation → REFUTED.
//!   6. `generate_all_proofs` includes the AuthorizationCoverage proof.

use axon::ir_nodes::IRProgram;
use axon::pcc::{
    check_proof, generate_all_proofs, generate_authorization_coverage_proofs, CheckOutcome,
    PropertyClass, Witness,
};

const VERSION: &str = "2.44.0-test";
const FLOW: &str = "flow Chat() -> Unit { step S { ask: \"hi\" } }\n";
const SHIELD: &str = "shield Guard { scan: [pii_leak] on_breach: halt }\n";

/// Build IR WITHOUT the type-checker (so uncovered endpoints survive to the IR
/// for the PCC to refute independently).
fn ir_of(src: &str) -> IRProgram {
    let tokens = axon::lexer::Lexer::new(src, "<t>").tokenize().expect("lex");
    let program = axon::parser::Parser::new(tokens).parse().expect("parse");
    axon::ir_generator::IRGenerator::new().generate(&program)
}

// ─── section 1 — public endpoint → verified ───────────────────────────────

#[test]
fn public_endpoint_generates_a_verified_proof() {
    let ir = ir_of(&format!(
        "{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat public: true }}"
    ));
    let proofs = generate_authorization_coverage_proofs(&ir, VERSION);
    assert_eq!(proofs.len(), 1, "one dispatching endpoint => one proof");
    assert_eq!(proofs[0].property, PropertyClass::AuthorizationCoverage);
    assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
}

// ─── section 2 — each coverage discipline → verified ──────────────────────

#[test]
fn each_coverage_discipline_verifies() {
    for (label, src) in [
        (
            "requires",
            format!("{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat requires: [flow.execute] }}"),
        ),
        (
            "shield",
            format!("{SHIELD}{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat shield: Guard }}"),
        ),
        (
            "compliance",
            format!("{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat compliance: [SOC2] }}"),
        ),
    ] {
        let ir = ir_of(&src);
        let proofs = generate_authorization_coverage_proofs(&ir, VERSION);
        assert_eq!(proofs.len(), 1, "{label}: one proof");
        assert_eq!(
            check_proof(&proofs[0], &ir),
            CheckOutcome::Verified,
            "{label}: coverage must verify"
        );
    }
}

// ─── section 3 — uncovered dispatching endpoint → refuted ─────────────────

#[test]
fn uncovered_endpoint_is_refuted() {
    let ir = ir_of(&format!(
        "{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat }}"
    ));
    let proofs = generate_authorization_coverage_proofs(&ir, VERSION);
    assert_eq!(proofs.len(), 1);
    match check_proof(&proofs[0], &ir) {
        CheckOutcome::Refuted { reason } => assert!(
            reason.contains("axon-T890"),
            "refutation must cite T890, got: {reason}"
        ),
        other => panic!("uncovered boundary must be REFUTED, got {other:?}"),
    }
}

// ─── section 4 — non-dispatching endpoint → no proof ──────────────────────

#[test]
fn non_dispatching_endpoint_yields_no_proof() {
    let ir = ir_of("axonendpoint E { method: POST path: \"/c\" }");
    assert!(
        generate_authorization_coverage_proofs(&ir, VERSION).is_empty(),
        "an endpoint that dispatches nothing crosses no boundary => no proof"
    );
}

// ─── section 5 — forged witness caught by recomputation ───────────────────

#[test]
fn forged_authorized_witness_is_refuted() {
    // Uncovered endpoint, but forge a witness claiming it is authorized.
    let ir = ir_of(&format!(
        "{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat }}"
    ));
    let mut proof = generate_authorization_coverage_proofs(&ir, VERSION)
        .into_iter()
        .next()
        .expect("one proof");
    if let Witness::AuthorizationCoverage(w) = &mut proof.witness {
        w.authorized = true; // the lie
        w.public = true;
    } else {
        panic!("expected an AuthorizationCoverage witness");
    }
    match check_proof(&proof, &ir) {
        CheckOutcome::Refuted { reason } => assert!(
            reason.contains("re-derivation"),
            "a forged witness must be caught by recomputation, got: {reason}"
        ),
        other => panic!("forged witness must be REFUTED, got {other:?}"),
    }
}

// ─── section 6 — generate_all_proofs includes the class ───────────────────

#[test]
fn generate_all_includes_authorization_coverage() {
    let ir = ir_of(&format!(
        "{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat public: true }}"
    ));
    let all = generate_all_proofs(&ir, VERSION);
    assert!(
        all.iter()
            .any(|p| p.property == PropertyClass::AuthorizationCoverage),
        "generate_all_proofs must include the AuthorizationCoverage class"
    );
}
