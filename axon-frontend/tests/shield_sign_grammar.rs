//! v2.34.0 — the honest shield grammar: `sign:` is a REAL field and the
//! parser stops silently discarding what it does not know.
//!
//! Three properties, each pinned:
//!
//! 1. **`sign:` round-trips** parse → AST → `IRShield.sign`, closed catalog
//!    (`hmac_sha256`, `axon-T846`), elided from the IR JSON when empty (zero
//! IR-SHA drift for every pre-v2.34.0 program).
//! 2. **`axon-W010`** — an unknown shield field is still SKIPPED (leniency
//!    preserved: the program compiles) but now recorded + surfaced as a
//!    warning naming the field and the valid catalog. Pre-77 the parser's
//! `_ => skip_value()` swallowed it unremarked (Kivi brief #51 B.3).
//! 3. **`axon-W011`** — an `on_breach:` with NO enforcement-bearing field is
//!    a vacuous shield (nothing can breach). A sign-only egress shield is
//! NOT vacuous: the signature is its enforcement.

use axon_frontend::ir_nodes::IRShield;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::{TypeChecker, TypeError};

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn check_with_warnings(src: &str) -> (Vec<TypeError>, Vec<TypeError>) {
    let prog = parse(src);
    TypeChecker::new(&prog).check_with_warnings()
}

fn first_ir_shield(src: &str) -> IRShield {
    let prog = parse(src);
    let ir = axon_frontend::ir_generator::IRGenerator::new().generate(&prog);
    ir.shields.first().expect("no shield in IR").clone()
}

/// The brief's exact egress shield — the program Kivi verified with
/// `axon.check` (B.1). After v2.34.0 it must STILL be well-formed, with
/// zero errors AND zero warnings (sign-only is a legitimate egress shield).
const WEBHOOK_EGRESS: &str = r#"
shield WebhookEgress {
    sign: hmac_sha256
    on_breach: halt
}
"#;

#[test]
fn sign_field_parses_and_lowers_into_the_ir() {
    let shield = first_ir_shield(WEBHOOK_EGRESS);
    assert_eq!(
        shield.sign, "hmac_sha256",
        "the declared sign algorithm must reach IRShield for the v2.34.0 egress worker"
    );
}

#[test]
fn absent_sign_is_elided_from_the_ir_json() {
    let shield = first_ir_shield(
        r#"
shield Guard {
    scan: [pii_leak]
    on_breach: halt
}
"#,
    );
    assert_eq!(shield.sign, "", "back-compat: absent → empty");
    let json = serde_json::to_string(&shield).expect("serialize");
    assert!(
        !json.contains("\"sign\""),
        "an absent sign must be elided from the IR JSON (zero IR-SHA drift), got: {json}"
    );
}

#[test]
fn present_sign_is_serialized() {
    let shield = first_ir_shield(WEBHOOK_EGRESS);
    let json = serde_json::to_string(&shield).expect("serialize");
    assert!(
        json.contains("\"sign\":\"hmac_sha256\""),
        "a declared sign must ride the IR JSON, got: {json}"
    );
}

#[test]
fn brief_51_webhook_egress_shield_is_clean() {
    // The B.1 program property, now with honest semantics behind it:
    // zero errors, zero warnings.
    let (errs, warns) = check_with_warnings(WEBHOOK_EGRESS);
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
    assert!(warns.is_empty(), "unexpected warnings: {warns:?}");
}

#[test]
fn t846_unknown_sign_algorithm_is_an_error() {
    // A misspelled algorithm would ship unsigned deliveries — ERROR, not
    // warning.
    let (errs, _) = check_with_warnings(
        r#"
shield W {
    sign: hmac_sha512
    on_breach: halt
}
"#,
    );
    assert_eq!(errs.len(), 1, "exactly one error expected: {errs:?}");
    assert!(
        errs[0].message.contains("axon-T846") && errs[0].message.contains("hmac_sha512"),
        "T846 must name the bad algorithm, got: {}",
        errs[0].message
    );
    assert!(
        errs[0].message.contains("hmac_sha256"),
        "T846 must quote the valid catalog, got: {}",
        errs[0].message
    );
}

#[test]
fn w010_unknown_shield_field_warns_but_compiles() {
    // The pre-v2.34.0 lie: `sing:` (typo) was silently discarded and the shield
    // passed `axon check` unremarked. Now: still compiles (leniency
    // preserved, the design decision) but the checker says so.
    let (errs, warns) = check_with_warnings(
        r#"
shield W {
    sing: hmac_sha256
    scan: [pii_leak]
    on_breach: halt
}
"#,
    );
    assert!(errs.is_empty(), "leniency preserved — no errors: {errs:?}");
    assert_eq!(warns.len(), 1, "exactly one warning expected: {warns:?}");
    assert!(
        warns[0].message.contains("axon-W010") && warns[0].message.contains("'sing'"),
        "W010 must name the unknown field, got: {}",
        warns[0].message
    );
    assert!(
        warns[0].message.contains("sign"),
        "W010 must quote the valid-field catalog, got: {}",
        warns[0].message
    );
}

#[test]
fn w010_names_the_offending_line() {
    let (_, warns) = check_with_warnings(
        r#"
shield W {
    scan: [pii_leak]
    on_breach: halt
    frobnicate: [a, b]
}
"#,
    );
    assert_eq!(warns.len(), 1, "one warning: {warns:?}");
    assert_eq!(
        warns[0].line, 5,
        "the warning must anchor to the unknown field's line, got line {}",
        warns[0].line
    );
}

#[test]
fn w011_vacuous_on_breach_warns() {
    // `on_breach: halt` with nothing enforced — the halt can never fire.
    let (errs, warns) = check_with_warnings(
        r#"
shield Hollow {
    on_breach: halt
    severity: high
}
"#,
    );
    assert!(errs.is_empty(), "vacuity is a warning, not an error: {errs:?}");
    assert_eq!(warns.len(), 1, "exactly one warning expected: {warns:?}");
    assert!(
        warns[0].message.contains("axon-W011") && warns[0].message.contains("Hollow"),
        "W011 must name the vacuous shield, got: {}",
        warns[0].message
    );
}

#[test]
fn w011_silent_for_tool_gating_and_scanning_shields() {
    // Every enforcement-bearing field suppresses the vacuity warning.
    for body in [
        "scan: [pii_leak]  on_breach: halt",
        "sign: hmac_sha256  on_breach: halt",
        "redact: [ssn]  on_breach: deflect  deflect_message: \"no\"",
        "deny_tools: [Shell]  on_breach: halt",
        "allow_tools: [Search]  on_breach: halt",
        "confidence_threshold: 0.9  on_breach: escalate",
    ] {
        let src = format!("shield G {{ {body} }}");
        let (errs, warns) = check_with_warnings(&src);
        assert!(errs.is_empty(), "`{body}` errored: {errs:?}");
        assert!(
            warns.is_empty(),
            "`{body}` must not be flagged vacuous, got: {warns:?}"
        );
    }
}

#[test]
fn w010_catalog_matches_parser() {
    // Pin parser ↔ SHIELD_FIELD_CATALOG sync: a shield exercising EVERY
    // documented field must produce ZERO W010 warnings. A field added to
    // the parser without updating the catalog trips the message-side;
    // a field named in the catalog but dropped from the parser trips here.
    let (errs, warns) = check_with_warnings(
        r#"
shield Everything {
    scan: [pii_leak]
    strategy: pattern
    on_breach: halt
    severity: high
    quarantine: "vault"
    max_retries: 2
    confidence_threshold: 0.8
    allow_tools: [Search]
    deny_tools: [Shell]
    sandbox: true
    redact: [ssn]
    log: full
    deflect_message: "no"
    compliance: [GDPR]
    sign: hmac_sha256
}
"#,
    );
    // v2.67.0 — `taint:` was here, and it was the ONE field in this
    // "everything-shield" that did nothing at all: parsed, lowered into the IR,
    // never read by the runtime. It is now RETRACTED (axon-T936), so it is
    // deliberately absent — a shield field that errors is not a documented
    // field. Do not "fix" this test by putting it back; see
    // `tests/retractions.rs::t936_shield_taint_is_refused`.
    //
    // It stays in the parser + SHIELD_FIELD_CATALOG on purpose: that is what
    // lets T936 greet an existing program with a full explanation, instead of a
    // vague W010 "unknown field" warning that would let it keep compiling.
    assert!(errs.is_empty(), "the everything-shield must check clean: {errs:?}");
    let w010: Vec<_> = warns
        .iter()
        .filter(|w| w.message.contains("axon-W010"))
        .collect();
    assert!(
        w010.is_empty(),
        "every cataloged field must be recognized by the parser: {w010:?}"
    );
}
