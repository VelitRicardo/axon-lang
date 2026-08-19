//! v2.34.0 — `publish <Channel> within <EgressShield>` egress typing + IR.
//!
//! Three properties, each pinned:
//!
//! 1. **`axon-T848`** — a publish under a SIGNING shield requires the channel
//!    be durable (`persistence: persistent_axonstore`): signed external
//! delivery inherits the v2.31.0 outbox's at-least-once; an ephemeral egress
//! promise dies unwitnessed with the process.
//! 2. **`axon-T847`** — publish shield references carry the typed code.
//! 3. **IR egress marking** — the lowering resolves the shield's algorithm
//!    onto `IRPublish.sign` AND stamps `IRChannel.egress_sign`
//!    (order-independent; first site wins), both elided from JSON when
//! empty (zero IR-SHA drift for every pre-v2.34.0 program).

use axon_frontend::ir_nodes::IRProgram;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::{TypeChecker, TypeError};

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn check(src: &str) -> Vec<TypeError> {
    let prog = parse(src);
    TypeChecker::new(&prog).check()
}

fn ir(src: &str) -> IRProgram {
    let prog = parse(src);
    axon_frontend::ir_generator::IRGenerator::new().generate(&prog)
}

/// The brief-#51 program shape: durable channel + sign-only egress shield.
const DURABLE_EGRESS: &str = r#"
type SkillResult { task_id: String  tenant_id: String  status: String  result: String }
shield WebhookEgress { sign: hmac_sha256  on_breach: halt }
channel SkillCompleted {
    message: SkillResult  qos: at_least_once  lifetime: affine
    persistence: persistent_axonstore  shield: WebhookEgress
}
flow CompleteSkill(task_id: String, tenant_id: String, result: String) -> Unit {
    step Build { ask: "Build the skill result payload."  output: SkillResult }
    emit SkillCompleted(Build)
    publish SkillCompleted within WebhookEgress
}
run CompleteSkill()
"#;

#[test]
fn brief_51_program_checks_clean() {
    // The exact program Kivi verified with `axon.check` (B.1) — now the
    // well-formed verdict is backed by real egress semantics.
    let errs = check(DURABLE_EGRESS);
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
}

#[test]
fn t848_signed_egress_on_ephemeral_channel_is_an_error() {
    let errs = check(
        r#"
type T { id: String }
shield WebhookEgress { sign: hmac_sha256  on_breach: halt }
channel C { message: T  shield: WebhookEgress }
flow F(id: String) -> Unit {
    publish C within WebhookEgress
}
"#,
    );
    assert_eq!(errs.len(), 1, "exactly one error expected: {errs:?}");
    assert!(
        errs[0].message.contains("axon-T848")
            && errs[0].message.contains("persistent_axonstore"),
        "T848 must name the durable requirement, got: {}",
        errs[0].message
    );
}

#[test]
fn t848_silent_for_non_signing_shield_on_ephemeral_channel() {
    // Pure π-calc publish (no sign:) keeps its pre-v2.34.0 semantics — an
    // ephemeral channel is fine.
    let errs = check(
        r#"
type T { id: String }
shield Gate { scan: [pii_leak]  on_breach: halt }
channel C { message: T  shield: Gate }
flow F(id: String) -> Unit {
    publish C within Gate
}
"#,
    );
    assert!(errs.is_empty(), "back-compat broken: {errs:?}");
}

#[test]
fn t847_undefined_publish_shield_carries_the_code() {
    let errs = check(
        r#"
type T { id: String }
channel C { message: T }
flow F(id: String) -> Unit {
    publish C within Ghost
}
"#,
    );
    assert!(
        errs.iter()
            .any(|e| e.message.contains("axon-T847") && e.message.contains("Ghost")),
        "T847 must name the undefined shield, got: {errs:?}"
    );
}

#[test]
fn ir_publish_carries_the_resolved_sign() {
    let program = ir(DURABLE_EGRESS);
    let json = serde_json::to_string(&program).expect("serialize");
    assert!(
        json.contains("\"sign\":\"hmac_sha256\""),
        "the publish node must carry the resolved algorithm, got: {json}"
    );
}

#[test]
fn ir_channel_is_stamped_egress() {
    let program = ir(DURABLE_EGRESS);
    let ch = program
        .channels
        .iter()
        .find(|c| c.name == "SkillCompleted")
        .expect("channel lowered");
    assert_eq!(
        ch.egress_sign, "hmac_sha256",
        "the channel handle must carry the egress algorithm for the v2.34.0 worker"
    );
}

#[test]
fn egress_resolution_is_declaration_order_independent() {
    // The flow (and its publish) precede the shield AND the channel in
    // source — the Phase-0 pre-pass + Phase-1.5 post-pass must still
    // resolve the marking.
    let program = ir(
        r#"
flow F(id: String) -> Unit {
    publish C within WebhookEgress
}
type T { id: String }
channel C { message: T  persistence: persistent_axonstore  shield: WebhookEgress }
shield WebhookEgress { sign: hmac_sha256  on_breach: halt }
"#,
    );
    assert_eq!(program.channels[0].egress_sign, "hmac_sha256");
}

/// v2.34.0 — the COMPILE-GATED example that backs the
/// `ADOPTER_WEBHOOKS.md` section 1 snippet (the "published grammar MUST compile"
/// discipline, Kivi brief #29). If this fails, the shipped guide's egress
/// declaration no longer compiles — the doc and the grammar have drifted.
#[test]
fn adopter_webhooks_md_snippet_compiles_clean() {
    let prog = parse(
        r#"
type SkillResult { task_id: String  tenant_id: String  status: String  result: String }

shield WebhookEgress {
  sign:      hmac_sha256
  on_breach: halt
}

channel SkillCompleted {
  message:     SkillResult
  qos:         at_least_once
  lifetime:    affine
  persistence: persistent_axonstore
  shield:      WebhookEgress
}

flow CompleteSkill(task_id: String, tenant_id: String, result: String) -> Unit {
  step Build { ask: "Build the skill result payload."  output: SkillResult }
  emit SkillCompleted(Build)
  publish SkillCompleted within WebhookEgress
}
"#,
    );
    let (errs, warns) = TypeChecker::new(&prog).check_with_warnings();
    assert!(
        errs.is_empty(),
        "the ADOPTER_WEBHOOKS.md egress snippet must compile clean: {errs:?}"
    );
    // The sign-only egress shield is legitimate — no vacuity (W011) or
    // unknown-field (W010) warning.
    assert!(
        warns
            .iter()
            .all(|w| !w.message.contains("axon-W010") && !w.message.contains("axon-W011")),
        "no egress-grammar warnings expected: {warns:?}"
    );
}

#[test]
fn non_signing_publish_leaves_ir_byte_identical() {
    // Zero IR-SHA drift: a scan-shield publish elides both new fields.
    let program = ir(
        r#"
type T { id: String }
shield Gate { scan: [pii_leak]  on_breach: halt }
channel C { message: T  shield: Gate }
flow F(id: String) -> Unit {
    publish C within Gate
}
"#,
    );
    let json = serde_json::to_string(&program).expect("serialize");
    assert!(
        !json.contains("\"sign\"") && !json.contains("egress_sign"),
        "a non-egress program must elide the v2.34.0 fields, got: {json}"
    );
}
