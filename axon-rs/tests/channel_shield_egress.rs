//! v2.69.0 (owed) — **a `channel`'s `shield:` is REAL on egress: `emit` scans
//! the value through the shield before it leaves.**
//!
//! The channel grammar carried `shield: σ` and it was PCC-checked, but the runtime
//! `run_emit` never invoked the scanner — the σ-shield gate was DECLARED, PROVEN,
//! and DEAD (the v2.67.0 "real engine, dead wire" pattern: the shield SCANNER exists
//! and is already wired to the `shield` STEP, but the channel egress path skipped
//! it). Now the channel's shield is resolved onto the `IREmit` node at lowering
//! (Phase 0, like `IRPublish.sign`) and `run_emit` scans the egressing value
//! through it — on EVERY dispatch path, by construction (the shield rides the node,
//! not a per-ctx map that a forgotten dispatch site would miss).
//!
//! `section 1` — lowering: the channel's shield resolves onto the emit node.
//! `section 2` — runtime: `Reject` fails closed (the value never leaves); `Pass(redacted)`
//!        emits the redacted value; an unshielded channel is untouched.

use std::sync::Arc;

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::wire_integrations::run_emit;
use axon::flow_dispatcher::DispatchCtx;
use axon::ir_nodes::{IREmit, IRFlowNode};
use axon::shield_registry::{
    register_shield_scanner, unregister_shield_scanner, ShieldScanContext, ShieldScanner,
    ShieldVerdict,
};

fn compile(src: &str) -> axon::ir_nodes::IRProgram {
    let tokens = axon_frontend::lexer::Lexer::new(src, "<t>")
        .tokenize()
        .expect("lex");
    let prog = axon_frontend::parser::Parser::new(tokens).parse().expect("parse");
    axon_frontend::ir_generator::IRGenerator::new().generate(&prog)
}

/// Find the first `emit`'s `shield_ref` in the first flow.
fn first_emit_shield(ir: &axon::ir_nodes::IRProgram) -> Option<String> {
    ir.flows.iter().flat_map(|f| f.steps.iter()).find_map(|n| match n {
        IRFlowNode::Emit(e) => Some(e.shield_ref.clone()),
        _ => None,
    })
}

// ── section 1 — lowering ────────────────────────────────────────────────────────────

/// 🎯 **A `channel C { shield: S }`'s shield resolves onto `emit C(v)` at
/// lowering** — regardless of declaration order (Phase 0 pre-pass). This is what
/// lets `run_emit` scan on every dispatch path without threading a channel→shield
/// map through each ctx-build site (the multi-path trap this owed item avoids).
#[test]
fn the_channel_shield_resolves_onto_the_emit_node() {
    let src = "shield Fw { scan: [pii_leak]  on_breach: halt }\n\
               channel Secure { message: String  shield: Fw }\n\
               flow F() -> Unit {\n\
                   let payload = \"x\"\n\
                   emit Secure(payload)\n\
               }\n";
    assert_eq!(first_emit_shield(&compile(src)).as_deref(), Some("Fw"));
}

/// An UNSHIELDED channel leaves the emit node's `shield_ref` empty — byte-identical
/// to a pre-v2.69.0 emit (no scan, `skip_serializing_if` elides the field).
#[test]
fn an_unshielded_channel_leaves_the_emit_shield_empty() {
    let src = "channel Plain { message: String }\n\
               flow F() -> Unit {\n\
                   let payload = \"x\"\n\
                   emit Plain(payload)\n\
               }\n";
    assert_eq!(first_emit_shield(&compile(src)).as_deref(), Some(""));
}

// ── section 2 — runtime enforcement ─────────────────────────────────────────────────

struct AlwaysReject;
impl ShieldScanner for AlwaysReject {
    fn scan(&self, _target: &str, ctx: &ShieldScanContext) -> ShieldVerdict {
        ShieldVerdict::reject(format!("{}.blocked", ctx.shield_name), "policy rejection (test)")
    }
}

struct Uppercase;
impl ShieldScanner for Uppercase {
    fn scan(&self, target: &str, _ctx: &ShieldScanContext) -> ShieldVerdict {
        ShieldVerdict::pass(target.to_uppercase())
    }
}

// Returns the receiver too — `run_emit` sends a StepStart on `ctx.tx`, so the rx
// must stay alive for the whole test (a dropped rx makes the send fail with
// `ChannelClosed` BEFORE the shield scan runs).
type EventRx = tokio::sync::mpsc::UnboundedReceiver<axon::flow_execution_event::FlowExecutionEvent>;
fn ctx_with_payload(value: &str) -> (DispatchCtx, EventRx) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx);
    ctx.let_bindings.insert("payload".to_string(), value.to_string());
    (ctx, rx)
}

fn emit_node(shield: &str) -> IREmit {
    IREmit {
        node_type: "emit",
        source_line: 0,
        source_column: 0,
        channel_ref: "Secure".to_string(),
        value_ref: "payload".to_string(),
        value_is_channel: false,
        shield_ref: shield.to_string(),
        breach_policy: None,
        // v2.89.0 — these tests exercise the REGISTERED-scanner path (and
        // the honest-identity path); an empty `scan:` is what a v2.69.0 shield
        // that declares no scan list lowers to.
        scan: Vec::new(),
    }
}

/// 🔴 **A shield `Reject` FAILS CLOSED: the emit errors and the value NEVER
/// reaches the channel.** This is the security floor the dead wire was hiding —
/// a violating payload no longer egresses just because nobody wired the scan.
#[tokio::test]
async fn a_rejected_emit_fails_closed_and_the_value_never_leaves() {
    let name = "FwRejectGate";
    register_shield_scanner(name, Arc::new(AlwaysReject));

    let (mut ctx, _rx) = ctx_with_payload("phi-secret");
    let err = run_emit(&emit_node(name), &mut ctx)
        .await
        .expect_err("a Reject verdict must fail the emit closed");
    match err {
        axon::flow_dispatcher::DispatchError::BackendError { name: n, message } => {
            assert!(n.starts_with("shield:"), "blame attributed to the shield: {n}");
            assert!(message.contains("blocked"), "carries the rejection code: {message}");
        }
        other => panic!("expected a shield BackendError, got {other:?}"),
    }
    // The load-bearing assertion: the value did NOT reach the channel buffer.
    assert!(
        ctx.let_bindings.get("__channel_Secure").is_none(),
        "a rejected value must NEVER leave the channel — found it in the buffer"
    );

    unregister_shield_scanner(name);
}

/// **A shield `Pass(redacted)` emits the REDACTED value** — the scanner may
/// transform (redact) content, and it is the transformed value that egresses.
#[tokio::test]
async fn a_passing_emit_egresses_the_redacted_value() {
    let name = "FwRedactGate";
    register_shield_scanner(name, Arc::new(Uppercase));

    let (mut ctx, _rx) = ctx_with_payload("secret");
    run_emit(&emit_node(name), &mut ctx)
        .await
        .expect("a Pass verdict must let the emit proceed");
    assert_eq!(
        ctx.let_bindings.get("__channel_Secure").map(String::as_str),
        Some("SECRET"),
        "the REDACTED (transformed) value is what reaches the channel"
    );

    unregister_shield_scanner(name);
}

/// An UNSHIELDED emit (empty `shield_ref`) is untouched — no scan, the raw value
/// egresses, byte-identical to pre-v2.69.0.
#[tokio::test]
async fn an_unshielded_emit_is_not_scanned() {
    let (mut ctx, _rx) = ctx_with_payload("plain");
    run_emit(&emit_node(""), &mut ctx)
        .await
        .expect("an unshielded emit proceeds");
    assert_eq!(
        ctx.let_bindings.get("__channel_Secure").map(String::as_str),
        Some("plain"),
        "no shield ⇒ the raw value egresses unchanged"
    );
}

/// A shielded emit whose shield has NO registered scanner (the OSS default) is an
/// identity passthrough — the wire is live but OSS ships no scanners, so the value
/// egresses unchanged. The enterprise layer registers the real scanners.
#[tokio::test]
async fn an_unregistered_shield_is_an_oss_identity_passthrough() {
    let (mut ctx, _rx) = ctx_with_payload("plain");
    run_emit(&emit_node("NeverRegisteredGate"), &mut ctx)
        .await
        .expect("no scanner registered ⇒ identity passthrough, emit proceeds");
    assert_eq!(
        ctx.let_bindings.get("__channel_Secure").map(String::as_str),
        Some("plain"),
        "OSS with no scanner registered passes through unchanged"
    );
}
