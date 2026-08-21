//! v4.4.0 — `document`, `deliver` and `notify` get a typed input, and with it a
//! κ they can be checked against.
//!
//! These three were the exits the previous cycle had to publish as
//! **uncoverable**, and the reason was structural rather than an oversight: κ
//! originates in a `type` declaration's `compliance:` list, and these three
//! bound bare value references. *"Is this value attributed?"* is answerable
//! about a bare reference — which is why their provenance barriers (T916, T920,
//! T934) work. *"What regulatory classes does it carry?"* is not.
//!
//! `payload: <Type>` is what gives them an answer. It names a DECLARED TYPE,
//! and the coverage rule reads κ from it exactly as `axon-T957` does at an
//! endpoint.
//!
//! # The field is optional, and that is deliberate
//!
//! A declaration that names no payload carries no class to cover, so it stays
//! legal. The rule bites on a payload that CARRIES a class without a covering
//! shield — not on the absence of one. Every existing program keeps compiling;
//! what changes is that a regulated one can no longer leave unguarded.
//!
//! # One rule, three call sites
//!
//! `check_egress_kappa` is shared. The endpoint and channel rules are duals
//! written twice, and keeping those two in step has already cost real work;
//! three more copies would drift the same way. Only the primitive name and the
//! diagnostic code vary, which is why the three assertions below can be read as
//! one property tested three times.

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

const REGULATED: &str =
    "type Payload compliance [HIPAA] { ssn: String }\n";
const REGULATED_TWO: &str =
    "type Payload compliance [HIPAA, GDPR] { ssn: String }\n";
const PLAIN: &str = "type Payload { ssn: String }\n";
const SHIELD: &str =
    "shield Guard { scan: [pii_leak]  compliance: [HIPAA]  on_breach: halt }\n";

/// The three declarations, parameterised over what they carry.
fn document(payload: &str, shield: &str) -> String {
    format!(
        "document Report {{ target: docx  provenance: embedded  effects: <io, storage> \
         {payload} {shield} section {{ heading: \"Q3\" }} }}\n"
    )
}

fn deliver(payload: &str, shield: &str) -> String {
    format!(
        "deliver PushLead {{ target: crm  provenance: attached  secret: crm_api_key  \
         effects: <web> {payload} {shield} upsert_contact {{ key: lead_email  \
         email: lead_email }} }}\n"
    )
}

fn notify(payload: &str, shield: &str) -> String {
    format!(
        "notify Alert {{ channel: sms  to: secret(ops.oncall)  template: \"t\"  \
         effects: <web>  window: business_hours {payload} {shield} }}\n"
    )
}

// ── the rule fires where a class would otherwise leave unguarded ────────────

#[test]
fn a_regulated_payload_leaving_through_a_document_needs_a_shield() {
    let src = format!("{REGULATED}{}", document("payload: Payload", ""));
    let found = hits(&src, "axon-T1222");
    assert_eq!(found.len(), 1, "an uncovered document egress must be refused: {found:#?}");
    assert!(found[0].contains("HIPAA"), "name the class: {}", found[0]);
}

#[test]
fn a_regulated_payload_leaving_through_a_deliver_needs_a_shield() {
    let src = format!("{REGULATED}{}", deliver("payload: Payload", ""));
    let found = hits(&src, "axon-T1223");
    assert_eq!(found.len(), 1, "an uncovered CRM delivery must be refused: {found:#?}");
    assert!(found[0].contains("HIPAA"), "name the class: {}", found[0]);
}

#[test]
fn a_regulated_payload_leaving_through_a_notify_needs_a_shield() {
    let src = format!("{REGULATED}{}", notify("payload: Payload", ""));
    let found = hits(&src, "axon-T1224");
    assert_eq!(found.len(), 1, "an uncovered notification must be refused: {found:#?}");
    assert!(found[0].contains("HIPAA"), "name the class: {}", found[0]);
}

// ── and stays silent where there is nothing to cover ────────────────────────

#[test]
fn an_egress_with_no_payload_is_left_alone() {
    // The compatibility promise. Every `document`, `deliver` and `notify`
    // written before this field existed names no payload, and none of them
    // suddenly stops compiling.
    for (code, src) in [
        ("axon-T1222", document("", "")),
        ("axon-T1223", deliver("", "")),
        ("axon-T1224", notify("", "")),
    ] {
        assert!(
            hits(&src, code).is_empty(),
            "{code} fired on a declaration that names no payload — the rule must bite on \
             an uncovered CLASS, not on the absence of a field: {:#?}",
            hits(&src, code)
        );
    }
}

#[test]
fn an_unregulated_payload_needs_no_shield() {
    // A typed payload that carries no class is not a compliance event. A rule
    // that demanded a shield here would tax every ordinary program, and a tax
    // on ordinary programs is how a real rule gets switched off.
    for (code, src) in [
        ("axon-T1222", format!("{PLAIN}{}", document("payload: Payload", ""))),
        ("axon-T1223", format!("{PLAIN}{}", deliver("payload: Payload", ""))),
        ("axon-T1224", format!("{PLAIN}{}", notify("payload: Payload", ""))),
    ] {
        assert!(
            hits(&src, code).is_empty(),
            "{code} fired on an unregulated payload: {:#?}",
            hits(&src, code)
        );
    }
}

#[test]
fn a_covering_shield_satisfies_all_three() {
    for (code, src) in [
        ("axon-T1222", format!("{REGULATED}{SHIELD}{}", document("payload: Payload", "shield: Guard"))),
        ("axon-T1223", format!("{REGULATED}{SHIELD}{}", deliver("payload: Payload", "shield: Guard"))),
        ("axon-T1224", format!("{REGULATED}{SHIELD}{}", notify("payload: Payload", "shield: Guard"))),
    ] {
        assert!(
            hits(&src, code).is_empty(),
            "{code} fired despite a shield covering the class: {:#?}",
            hits(&src, code)
        );
    }
}

// ── the set difference is a work list, not a restatement ────────────────────

#[test]
fn partial_coverage_names_exactly_the_uncovered_class() {
    for (code, src) in [
        ("axon-T1222", format!("{REGULATED_TWO}{SHIELD}{}", document("payload: Payload", "shield: Guard"))),
        ("axon-T1223", format!("{REGULATED_TWO}{SHIELD}{}", deliver("payload: Payload", "shield: Guard"))),
        ("axon-T1224", format!("{REGULATED_TWO}{SHIELD}{}", notify("payload: Payload", "shield: Guard"))),
    ] {
        let found = hits(&src, code);
        assert_eq!(found.len(), 1, "{code}: partial coverage is a violation: {found:#?}");
        assert!(
            found[0].contains("GDPR"),
            "{code} must name GDPR, the class the shield does NOT cover: {}",
            found[0]
        );
        assert!(
            !found[0].contains("{GDPR, HIPAA}"),
            "{code} must not re-list the class already covered — the message is a work \
             list: {}",
            found[0]
        );
    }
}

// ── a wrapper does not launder a class at an egress either ──────────────────

#[test]
fn the_egress_rule_sees_through_a_wrapper() {
    // The three read the same `transitive_kappa` the endpoint and the channel
    // read. If any of them had grown its own κ computation, this is where the
    // copies would part company.
    let types = "type Inner compliance [HIPAA] { ssn: String }\n\
                 type Payload { inner: Inner }\n";
    for (code, src) in [
        ("axon-T1222", format!("{types}{}", document("payload: Payload", ""))),
        ("axon-T1223", format!("{types}{}", deliver("payload: Payload", ""))),
        ("axon-T1224", format!("{types}{}", notify("payload: Payload", ""))),
    ] {
        assert_eq!(
            hits(&src, code).len(),
            1,
            "{code} did not see the class through one level of wrapping — the shared κ \
             walk is not shared after all"
        );
    }
}
