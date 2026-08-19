//! v2.60.0 — grammar + AST + IR + structure checker + the provenance-stripping
//! barrier for Governed CRM Delivery (`deliver`). The egress-dual of `scrape`
//! (v2.52.0) and the egress-form of v2.53.0's assertion-laundering barrier.
//! See `docs/cycle/governed_crm_delivery.md` (axon-enterprise).
//!
//! Pinned properties:
//! 1. A `deliver` parses into `DeliverDefinition` with an operation list.
//! 2. It lowers to `IRProgram.deliveries`; a delivery-less program elides it.
//! 3. **axon-T920** — the barrier: a `provenance: cleared` delivery binding a
//!    flow value fails; `attached` (default) passes; wrapped in
//!    `epistemic{believe}` it passes; a literal-only cleared delivery passes.
//! 4. **axon-T921..T926** — structure laws (target / provenance / secret / web
//!    effect / operation catalog + non-empty / idempotency key).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn check_errors(src: &str) -> Vec<String> {
    let prog = parse(src);
    TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

fn ir_json(src: &str) -> String {
    let prog = parse(src);
    let ir = IRGenerator::new().generate(&prog);
    serde_json::to_string(&ir).expect("serialize IR")
}

const GOOD: &str = r#"
deliver push_lead {
    target:     crm
    provenance: attached
    secret:     crm_api_key
    effects:    <web>
    upsert_contact {
        key:       resolved_email
        email:     resolved_email
        firstname: resolved_name
        company:   company_name
    }
}
"#;

// ── 1. Grammar + AST ─────────────────────────────────────────────────────────

#[test]
fn parses_deliver_into_ast() {
    let prog = parse(GOOD);
    let d = prog
        .declarations
        .iter()
        .find_map(|x| match x {
            axon_frontend::ast::Declaration::Deliver(v) => Some(v),
            _ => None,
        })
        .expect("deliver declaration");
    assert_eq!(d.target, "crm");
    assert_eq!(d.provenance, "attached");
    assert_eq!(d.secret, "crm_api_key");
    assert_eq!(d.ops.len(), 1);
    let op = &d.ops[0];
    assert_eq!(op.kind, "upsert_contact");
    assert!(op.has_field("key"));
    assert_eq!(op.ref_fields().count(), 4); // key/email/firstname/company all Refs
}

#[test]
fn good_deliver_type_checks_clean() {
    let errs = check_errors(GOOD);
    assert!(errs.iter().all(|e| !e.contains("axon-T9")), "unexpected T9xx: {errs:?}");
}

// ── 2. IR lowering ───────────────────────────────────────────────────────────

#[test]
fn lowers_to_ir_deliveries() {
    let json = ir_json(GOOD);
    assert!(json.contains("\"deliveries\""));
    assert!(json.contains("push_lead"));
    assert!(json.contains("\"kind\":\"upsert_contact\""));
}

#[test]
fn delivery_less_program_elides_field() {
    let json = ir_json("flow F() -> Unit { step S { ask: \"hi\" } }\n");
    assert!(!json.contains("\"deliveries\""));
}

// ── 3. axon-T920 — the provenance-stripping barrier (the flagship) ──────────

#[test]
fn t920_cleared_delivery_binding_flow_value_is_refused() {
    let src = r#"
deliver bad { target: crm  secret: k  effects: <web>  provenance: cleared
    upsert_contact { key: guessed_email  email: guessed_email }
}
"#;
    let errs = check_errors(src);
    assert!(errs.iter().any(|e| e.contains("axon-T920")), "want T920: {errs:?}");
}

#[test]
fn t920_attached_default_delivery_of_flow_value_passes() {
    // The same binding, but `attached` (the default) — provenance travels, legal.
    let errs = check_errors(GOOD);
    assert!(!errs.iter().any(|e| e.contains("axon-T920")), "unexpected T920: {errs:?}");
}

#[test]
fn t920_cleared_under_epistemic_believe_passes() {
    let src = r#"
believe {
    deliver vouched { target: crm  secret: k  effects: <web>  provenance: cleared
        upsert_contact { key: verified_email  email: verified_email }
    }
}
"#;
    let errs = check_errors(src);
    assert!(!errs.iter().any(|e| e.contains("axon-T920")), "unexpected T920 under vouch: {errs:?}");
}

#[test]
fn t920_cleared_with_only_literals_passes() {
    // A cleared delivery that binds no flow value launders nothing.
    let src = r#"
deliver lit { target: crm  secret: k  effects: <web>  provenance: cleared
    add_note { key: "ext-42"  body: "manual note" }
}
"#;
    let errs = check_errors(src);
    assert!(!errs.iter().any(|e| e.contains("axon-T920")), "unexpected T920 for literals: {errs:?}");
}

// ── 4. axon-T921..T926 — structure laws ─────────────────────────────────────

#[test]
fn t921_unknown_target_is_refused() {
    let errs = check_errors(
        "deliver d { target: mailchimp  secret: k  effects: <web>  add_note { key: \"x\"  body: \"y\" } }\n",
    );
    assert!(errs.iter().any(|e| e.contains("axon-T921")), "want T921: {errs:?}");
}

#[test]
fn t922_unknown_provenance_is_refused() {
    let errs = check_errors(
        "deliver d { target: crm  secret: k  effects: <web>  provenance: opaque  add_note { key: \"x\"  body: \"y\" } }\n",
    );
    assert!(errs.iter().any(|e| e.contains("axon-T922")), "want T922: {errs:?}");
}

#[test]
fn t923_missing_secret_is_refused() {
    let errs = check_errors(
        "deliver d { target: crm  effects: <web>  add_note { key: \"x\"  body: \"y\" } }\n",
    );
    assert!(errs.iter().any(|e| e.contains("axon-T923")), "want T923: {errs:?}");
}

#[test]
fn t924_missing_web_effect_is_refused() {
    let errs = check_errors(
        "deliver d { target: crm  secret: k  add_note { key: \"x\"  body: \"y\" } }\n",
    );
    assert!(errs.iter().any(|e| e.contains("axon-T924")), "want T924: {errs:?}");
}

#[test]
fn t925_unknown_operation_is_refused() {
    let errs = check_errors(
        "deliver d { target: crm  secret: k  effects: <web>  delete_contact { key: \"x\" } }\n",
    );
    assert!(errs.iter().any(|e| e.contains("axon-T925")), "want T925: {errs:?}");
}

#[test]
fn t925_empty_body_is_refused() {
    let errs = check_errors("deliver d { target: crm  secret: k  effects: <web> }\n");
    assert!(errs.iter().any(|e| e.contains("axon-T925")), "want T925 (empty): {errs:?}");
}

#[test]
fn t926_missing_idempotency_key_is_refused() {
    let errs = check_errors(
        "deliver d { target: crm  secret: k  effects: <web>  add_note { body: \"y\" } }\n",
    );
    assert!(errs.iter().any(|e| e.contains("axon-T926")), "want T926: {errs:?}");
}
