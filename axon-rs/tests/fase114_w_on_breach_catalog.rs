//! §Fase 114.w — **the `on_breach:` catalog, HONORED.**
//!
//! The shield doc has promised five policies since Fase 20 —
//! `halt | quarantine | deflect | sanitize_and_retry | escalate` — and the
//! runtime ALWAYS halted: the catalog was documented, parsed, type-checked,
//! and dead. §114.w gives each policy its documented meaning, at BOTH
//! enforcement sites (the shield STEP and `run_emit`'s σ-gate), by riding
//! the policy on the IR node (`IRBreachPolicy`, resolved at lowering — the
//! §114 multi-path discipline).
//!
//! `§1` — lowering: the shield's policy resolves onto `IREmit` and
//!        `IRShieldApplyStep`; a policy-less shield stamps nothing (IR-SHA
//!        stable); axon-T952 refuses an operand-less policy.
//! `§2` — runtime, per policy, through the REAL `run_emit`:
//!        deflect proceeds with the DECLARED reply; quarantine routes to the
//!        sink then refuses (recoverable, not deliverable); an unmounted
//!        sink halts with a diagnostic; escalate hands off then refuses;
//!        sanitize_and_retry masks `redact:` fields, re-scans, and only a
//!        now-passing candidate proceeds; a non-JSON candidate halts.

use std::sync::{Arc, Mutex};

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::wire_integrations::run_emit;
use axon::flow_dispatcher::{DispatchCtx, DispatchError};
use axon::ir_nodes::{IRBreachPolicy, IREmit, IRFlowNode};
use axon::shield_registry::{
    register_breach_sink, register_shield_scanner, set_escalation_queue, unregister_breach_sink,
    unregister_shield_scanner, BreachSink, ShieldScanContext, ShieldScanner, ShieldVerdict,
};

// ── Harness ─────────────────────────────────────────────────────────────────

fn compile(src: &str) -> axon::ir_nodes::IRProgram {
    let tokens = axon_frontend::lexer::Lexer::new(src, "<t>").tokenize().expect("lex");
    let prog = axon_frontend::parser::Parser::new(tokens).parse().expect("parse");
    axon_frontend::ir_generator::IRGenerator::new().generate(&prog)
}

/// Rejects any candidate containing the needle; passes others UNCHANGED.
struct RejectNeedle(&'static str);
impl ShieldScanner for RejectNeedle {
    fn scan(&self, target: &str, _ctx: &ShieldScanContext) -> ShieldVerdict {
        if target.contains(self.0) {
            ShieldVerdict::reject("test.needle_present", "the needle is present")
        } else {
            ShieldVerdict::pass(target)
        }
    }
}

/// Records everything routed to it, including the tenant it was routed
/// for — a DLQ sink over regulated content must see WHOSE content it is
/// (the reason `BreachSink::route` carries `tenant_id`, mirroring
/// `axon::scrape_tool::ScrapeAuditSink::record`).
struct CaptureSink(Arc<Mutex<Vec<(String, String)>>>);
impl BreachSink for CaptureSink {
    fn route(
        &self,
        tenant_id: &str,
        _shield: &str,
        _code: &str,
        _reason: &str,
        candidate: &str,
    ) -> Result<(), String> {
        self.0.lock().unwrap().push((tenant_id.to_string(), candidate.to_string()));
        Ok(())
    }
}

type EventRx =
    tokio::sync::mpsc::UnboundedReceiver<axon::flow_execution_event::FlowExecutionEvent>;
fn ctx_with_payload(value: &str) -> (DispatchCtx, EventRx) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    // A concrete, non-empty tenant — proves tenant_id actually THREADS
    // through apply_on_breach into the sink, not just an empty default.
    let mut ctx = DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx)
        .with_tenant_id("t-114w");
    ctx.let_bindings.insert("payload".to_string(), value.to_string());
    (ctx, rx)
}

fn emit_node(shield: &str, policy: Option<IRBreachPolicy>) -> IREmit {
    IREmit {
        node_type: "emit",
        source_line: 0,
        source_column: 0,
        channel_ref: "Secure".to_string(),
        value_ref: "payload".to_string(),
        value_is_channel: false,
        shield_ref: shield.to_string(),
        breach_policy: policy,
        scan: Vec::new(),
    }
}

fn policy(on_breach: &str) -> IRBreachPolicy {
    IRBreachPolicy {
        on_breach: on_breach.into(),
        quarantine: String::new(),
        deflect_message: String::new(),
        redact: Vec::new(),
        max_retries: 3,
    }
}

// ── §1 — lowering + axon-T952 ───────────────────────────────────────────────

/// 🎯 **The shield's declared policy rides the enforcement nodes.** `deflect`
/// + its message resolve onto BOTH the emit and the shield-apply step at
/// lowering, regardless of declaration order.
#[test]
fn the_breach_policy_resolves_onto_the_enforcement_nodes() {
    let src = "flow F() -> Unit {\n\
                   let payload = \"x\"\n\
                   emit Secure(payload)\n\
                   shield Fw on payload -> Clean\n\
               }\n\
               channel Secure { message: String  shield: Fw }\n\
               shield Fw {\n\
                   scan: [pii_leak]\n\
                   on_breach: deflect\n\
                   deflect_message: \"[SAFE REPLY]\"\n\
               }\n";
    let ir = compile(src);
    let steps: Vec<&IRFlowNode> = ir.flows.iter().flat_map(|f| f.steps.iter()).collect();
    let emit_policy = steps
        .iter()
        .find_map(|n| match n {
            IRFlowNode::Emit(e) => e.breach_policy.as_ref(),
            _ => None,
        })
        .expect("emit must carry the policy");
    assert_eq!(emit_policy.on_breach, "deflect");
    assert_eq!(emit_policy.deflect_message, "[SAFE REPLY]");
    let step_policy = steps
        .iter()
        .find_map(|n| match n {
            IRFlowNode::ShieldApply(s) => s.breach_policy.as_ref(),
            _ => None,
        })
        .expect("shield step must carry the policy");
    assert_eq!(step_policy.on_breach, "deflect");
}

/// A policy-less shield stamps NOTHING — the emit serializes without the key
/// (IR-SHA stability for every pre-§114.w program).
#[test]
fn a_policyless_shield_stamps_nothing() {
    let src = "shield Fw { scan: [pii_leak] }\n\
               channel Secure { message: String  shield: Fw }\n\
               flow F() -> Unit {\n\
                   let payload = \"x\"\n\
                   emit Secure(payload)\n\
               }\n";
    let ir = compile(src);
    let emit = ir
        .flows
        .iter()
        .flat_map(|f| f.steps.iter())
        .find_map(|n| match n {
            IRFlowNode::Emit(e) => Some(e),
            _ => None,
        })
        .expect("emit");
    assert!(emit.breach_policy.is_none());
    let json = serde_json::to_string(emit).expect("serialize");
    assert!(!json.contains("breach_policy"), "elided when absent: {json}");
}

/// axon-T952 — an operand-less policy is refused at compile time.
#[test]
fn t952_refuses_operandless_policies() {
    // `quarantine` without a sink is axon-W012 (a WARNING — the runtime has a
    // defined fail-closed meaning for the hole and a large published tail
    // declares it); only the two policies with NOTHING to run are errors.
    for (decl, needle) in [
        ("on_breach: deflect", "deflect_message"),
        ("on_breach: sanitize_and_retry", "redact"),
    ] {
        let src = format!("shield Fw {{ scan: [pii_leak]  {decl} }}\n");
        let tokens = axon_frontend::lexer::Lexer::new(&src, "<t>").tokenize().expect("lex");
        let prog = axon_frontend::parser::Parser::new(tokens).parse().expect("parse");
        let errors: Vec<String> = axon_frontend::type_checker::TypeChecker::new(&prog)
            .check()
            .iter()
            .map(|e| e.message.clone())
            .collect();
        assert!(
            errors.iter().any(|e| e.contains("axon-T952") && e.contains(needle)),
            "`{decl}` without its operand must fail axon-T952 naming `{needle}`, got: {errors:?}"
        );
    }
}

// ── §2 — the runtime honors each policy (through the REAL run_emit) ─────────

/// **deflect** — the DECLARED safe reply egresses instead of the candidate;
/// no part of the rejected value leaves.
#[tokio::test]
async fn deflect_egresses_the_declared_reply_instead() {
    let name = "FwDeflect114w";
    register_shield_scanner(name, Arc::new(RejectNeedle("SSN")));

    let mut p = policy("deflect");
    p.deflect_message = "[SAFE REPLY]".into();
    let (mut ctx, _rx) = ctx_with_payload("SSN 123-45-6789");
    run_emit(&emit_node(name, Some(p)), &mut ctx)
        .await
        .expect("deflect proceeds with the canned reply");
    assert_eq!(
        ctx.let_bindings.get("__channel_Secure").map(String::as_str),
        Some("[SAFE REPLY]"),
        "the DECLARED reply egresses — never the candidate"
    );

    unregister_shield_scanner(name);
}

/// **quarantine** — the candidate routes to the sink (recoverable), and the
/// emission itself is REFUSED.
#[tokio::test]
async fn quarantine_routes_to_the_sink_and_refuses() {
    let name = "FwQuarantine114w";
    register_shield_scanner(name, Arc::new(RejectNeedle("SSN")));
    let captured = Arc::new(Mutex::new(Vec::new()));
    register_breach_sink("Dlq114w", Arc::new(CaptureSink(Arc::clone(&captured))));

    let mut p = policy("quarantine");
    p.quarantine = "Dlq114w".into();
    let (mut ctx, _rx) = ctx_with_payload("SSN 123-45-6789");
    let err = run_emit(&emit_node(name, Some(p)), &mut ctx)
        .await
        .expect_err("quarantine refuses the emission");
    match err {
        DispatchError::BackendError { message, .. } => {
            assert!(message.contains("quarantined"), "names the routing: {message}");
        }
        other => panic!("expected BackendError, got {other:?}"),
    }
    assert_eq!(
        captured.lock().unwrap().as_slice(),
        &[("t-114w".to_string(), "SSN 123-45-6789".to_string())],
        "the candidate must be RECOVERABLE from the sink, tagged with the ACQUIRING tenant"
    );
    assert!(
        ctx.let_bindings.get("__channel_Secure").is_none(),
        "quarantine never delivers"
    );

    unregister_breach_sink("Dlq114w");
    unregister_shield_scanner(name);
}

/// **quarantine, sink not mounted** — halts with a diagnostic naming the
/// hole (fail-closed; a phantom quarantine would be a false sense of
/// recovery — the §53.e doctrine).
#[tokio::test]
async fn an_unmounted_quarantine_sink_halts_with_a_diagnostic() {
    let name = "FwQuarantineHole114w";
    register_shield_scanner(name, Arc::new(RejectNeedle("SSN")));

    let mut p = policy("quarantine");
    p.quarantine = "NeverMounted114w".into();
    let (mut ctx, _rx) = ctx_with_payload("SSN 123-45-6789");
    let err = run_emit(&emit_node(name, Some(p)), &mut ctx)
        .await
        .expect_err("an unmounted sink must halt");
    match err {
        DispatchError::BackendError { message, .. } => {
            assert!(
                message.contains("NOT mounted"),
                "the diagnostic must name the hole: {message}"
            );
        }
        other => panic!("expected BackendError, got {other:?}"),
    }

    unregister_shield_scanner(name);
}

/// **escalate** — hands the candidate to the escalation queue and REFUSES
/// the emission (a human decides).
#[tokio::test]
async fn escalate_hands_off_and_refuses() {
    let name = "FwEscalate114w";
    register_shield_scanner(name, Arc::new(RejectNeedle("SSN")));
    let captured = Arc::new(Mutex::new(Vec::new()));
    set_escalation_queue(Some(Arc::new(CaptureSink(Arc::clone(&captured)))));

    let (mut ctx, _rx) = ctx_with_payload("SSN 123-45-6789");
    let err = run_emit(&emit_node(name, Some(policy("escalate"))), &mut ctx)
        .await
        .expect_err("escalate refuses the emission");
    match err {
        DispatchError::BackendError { message, .. } => {
            assert!(message.contains("escalated"), "names the hand-off: {message}");
        }
        other => panic!("expected BackendError, got {other:?}"),
    }
    assert_eq!(captured.lock().unwrap().len(), 1, "the queue received the candidate");

    set_escalation_queue(None);
    unregister_shield_scanner(name);
}

/// **sanitize_and_retry** — the declared `redact:` fields are masked, the
/// masked candidate re-scans, and the now-PASSING value egresses.
#[tokio::test]
async fn sanitize_and_retry_masks_rescans_and_proceeds() {
    let name = "FwSanitize114w";
    register_shield_scanner(name, Arc::new(RejectNeedle("123-45")));

    let mut p = policy("sanitize_and_retry");
    p.redact = vec!["ssn".into()];
    let (mut ctx, _rx) = ctx_with_payload(r#"{"ssn":"123-45-6789","note":"ok"}"#);
    run_emit(&emit_node(name, Some(p)), &mut ctx)
        .await
        .expect("the masked candidate passes the re-scan");
    let egressed = ctx.let_bindings.get("__channel_Secure").expect("delivered");
    assert!(egressed.contains("[REDACTED]"), "the field was masked: {egressed}");
    assert!(!egressed.contains("123-45"), "the needle must be GONE: {egressed}");
    assert!(egressed.contains("ok"), "untouched fields survive: {egressed}");

    unregister_shield_scanner(name);
}

/// **sanitize_and_retry over a non-JSON candidate** — halts (fail-closed
/// beats guessing at a sanitization the adopter never declared).
#[tokio::test]
async fn sanitize_of_a_non_json_candidate_halts() {
    let name = "FwSanitizeNonJson114w";
    register_shield_scanner(name, Arc::new(RejectNeedle("SSN")));

    let mut p = policy("sanitize_and_retry");
    p.redact = vec!["ssn".into()];
    let (mut ctx, _rx) = ctx_with_payload("SSN plain text");
    let err = run_emit(&emit_node(name, Some(p)), &mut ctx)
        .await
        .expect_err("a non-JSON candidate cannot be field-masked");
    match err {
        DispatchError::BackendError { message, .. } => {
            assert!(message.contains("not JSON"), "names the reason: {message}");
        }
        other => panic!("expected BackendError, got {other:?}"),
    }

    unregister_shield_scanner(name);
}
