//! §Fase 33.y.g integration tests — algebraic-effect handler nodes.
//!
//! Exercises the 6 graduated handlers + their public `apply_*` /
//! `invoke_*` helpers through `dispatch_node`:
//!
//! - ShieldApply / OtsApply / MandateApply (same shape: name, target, output_type)
//! - ComputeApply (compute_name, arguments, output_name)
//! - Listen (channel, channel_is_ref, event_alias)
//! - DaemonStep (daemon_ref)
//!
//! D-letter coverage:
//! - D1 — 30 of 45 variants graduated (cumulative).
//! - D3 — cancel propagation across each handler.
//! - D7 — every error case routes through DispatchError; OSS
//!   helpers are identity passthrough / placeholder so they
//!   cannot fail; enterprise overrides will surface BackendError.
//! - D10 — sync-runner parity: OSS-default helpers are identity
//!   passthrough; capability application is semantically a no-op
//!   in OSS (consistent with the sync runner's reference
//!   implementation).

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::algebraic_handlers::{
    apply_shield_to_target,
    invoke_daemon, listen_on_channel,
};
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, DispatchError, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::*;
use tokio::sync::mpsc;

/// §Fase 119.b — a declared mandate for fixtures that route MandateApply
/// through the real enforcement path.
/// §Fase 119.c — a declared ots + deterministic transformer for fixtures
/// that route OtsApply through the real registry path.
fn test_ots_spec(name: &str) -> axon::ir_nodes::IROts {
    axon::ir_nodes::IROts {
        node_type: "ots",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        teleology: "test transform".into(),
        homotopy_search: "shallow".into(),
        loss_function: "L2".into(),
    }
}

struct UpperTransformer;
impl axon::ots_registry::OtsTransformer for UpperTransformer {
    fn transform(
        &self,
        content: &str,
        _ctx: &axon::ots_registry::OtsTransformContext,
    ) -> axon::ots_registry::OtsVerdict {
        axon::ots_registry::OtsVerdict::Transformed(content.to_uppercase())
    }
}

fn test_mandate_spec(name: &str, constraint: &str) -> axon::ir_nodes::IRMandate {
    axon::ir_nodes::IRMandate {
        node_type: "mandate",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        constraint: constraint.into(),
        kp: Some(1.0),
        ki: None,
        kd: None,
        tolerance: Some(0.05),
        max_steps: Some(1),
        drift_bound: None,
        lipschitz: None,
        on_violation: "halt".into(),
    }
}

fn fresh_ctx() -> (
    DispatchCtx,
    mpsc::UnboundedReceiver<FlowExecutionEvent>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = DispatchCtx::new(
        "TestFlow",
        "stub",
        "",
        CancellationFlag::new(),
        tx,
    );
    (ctx, rx)
}

fn shield_apply(shield_name: &str, target: &str, output_type: &str) -> IRFlowNode {
    IRFlowNode::ShieldApply(IRShieldApplyStep {
        node_type: "shield_apply",
        source_line: 0,
        source_column: 0,
        shield_name: shield_name.into(),
        target: target.into(),
        output_type: output_type.into(),
    
        breach_policy: None,
    })
}

fn ots_apply(ots_name: &str, target: &str, output_type: &str) -> IRFlowNode {
    IRFlowNode::OtsApply(IROtsApplyStep {
        node_type: "ots_apply",
        source_line: 0,
        source_column: 0,
        ots_name: ots_name.into(),
        target: target.into(),
        output_type: output_type.into(),
    })
}

fn mandate_apply(mandate_name: &str, target: &str, output_type: &str) -> IRFlowNode {
    IRFlowNode::MandateApply(IRMandateApplyStep {
        node_type: "mandate_apply",
        source_line: 0,
        source_column: 0,
        mandate_name: mandate_name.into(),
        target: target.into(),
        output_type: output_type.into(),
    })
}

fn compute_apply(compute_name: &str, args: Vec<&str>, output_name: &str) -> IRFlowNode {
    IRFlowNode::ComputeApply(IRComputeApplyStep {
        node_type: "compute_apply",
        source_line: 0,
        source_column: 0,
        compute_name: compute_name.into(),
        arguments: args.into_iter().map(String::from).collect(),
        output_name: output_name.into(),
    })
}

fn listen(channel: &str, event_alias: &str) -> IRFlowNode {
    IRFlowNode::Listen(IRListenStep {
        node_type: "listen",
        source_line: 0,
        source_column: 0,
        channel: channel.into(),
        channel_is_ref: true,
        event_alias: event_alias.into(),
        body: Vec::new(),    })
}

fn daemon_step(daemon_ref: &str) -> IRFlowNode {
    IRFlowNode::DaemonStep(IRDaemonStepNode {
        node_type: "daemon_step",
        source_line: 0,
        source_column: 0,
        daemon_ref: daemon_ref.into(),
    })
}

// ────────────────────────────────────────────────────────────────────
//  §1 — Public apply_* helpers (OSS identity)
// ────────────────────────────────────────────────────────────────────

#[test]
fn apply_shield_oss_passthrough() {
    let (ctx, _rx) = fresh_ctx();
    assert_eq!(apply_shield_to_target("hipaa", "PII text", &ctx), "PII text");
}

// §Fase 119.c — `apply_ots_oss_passthrough` was DELETED here. It pinned the
// §111 F18 identity (a µ-law codec transforming nothing, in green). The
// registry-or-refusal gates live in `flow_dispatcher::algebraic_handlers::
// tests`; the fail-closed inversion is
// `an_unregistered_ots_refuses_never_identity`.

// §Fase 119.b — `apply_mandate_oss_passthrough` was DELETED here. It pinned
// the §111 F18 identity ("gdpr_erasure" transforming nothing, in green, for
// years). The passthrough helper no longer exists; the enforcement loop's
// gates live in `flow_dispatcher::algebraic_handlers::tests` and the
// fail-closed regression is
// `an_unresolved_mandate_name_fails_closed_never_identity`.

#[test]
fn listen_returns_awaiting_placeholder() {
    let (ctx, _rx) = fresh_ctx();
    assert_eq!(listen_on_channel("user.events", true, &ctx), "(awaiting user.events)");
}

#[test]
fn invoke_daemon_returns_invocation_placeholder() {
    let (ctx, _rx) = fresh_ctx();
    assert_eq!(invoke_daemon("scheduler", &ctx), "daemon:scheduler");
}

// ────────────────────────────────────────────────────────────────────
//  §2 — ShieldApply through dispatch_node
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_node_routes_shield_apply_with_explicit_output_type() {
    let (mut ctx, mut rx) = fresh_ctx();
    ctx.let_bindings.insert("input".into(), "raw-data".into());
    let outcome = dispatch_node(&shield_apply("hipaa", "input", "scrubbed"), &mut ctx)
        .await
        .unwrap();
    match outcome {
        NodeOutcome::Completed { output, tokens_emitted, .. } => {
            assert_eq!(output, "raw-data");
            assert_eq!(tokens_emitted, 0);
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    assert_eq!(ctx.let_bindings.get("scrubbed").unwrap(), "raw-data");
    let first = rx.try_recv().unwrap();
    match first {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "shield_apply");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

#[tokio::test]
async fn shield_apply_canonical_fallback_when_output_type_empty() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("doc".into(), "content".into());
    dispatch_node(&shield_apply("hipaa", "doc", ""), &mut ctx).await.unwrap();
    // Falls back to <target>_shielded.
    assert_eq!(ctx.let_bindings.get("doc_shielded").unwrap(), "content");
}

// ────────────────────────────────────────────────────────────────────
//  §3 — OtsApply through dispatch_node
// ────────────────────────────────────────────────────────────────────

/// §Fase 119.c — this routing test used to ride the identity passthrough.
/// It now routes a DECLARED ots through a REGISTERED transformer; the bound
/// value is the transform's output, which is itself proof the real path ran.
#[tokio::test]
async fn dispatch_node_routes_ots_apply() {
    let (mut ctx, mut rx) = fresh_ctx();
    ctx.ots_specs = std::sync::Arc::new(vec![test_ots_spec("resample")]);
    axon::ots_registry::register_ots_transformer(
        "resample",
        std::sync::Arc::new(UpperTransformer),
    );
    ctx.let_bindings.insert("audio_raw".into(), "samples".into());
    dispatch_node(&ots_apply("resample", "audio_raw", "pcm"), &mut ctx)
        .await
        .unwrap();
    axon::ots_registry::unregister_ots_transformer("resample");
    assert_eq!(ctx.let_bindings.get("pcm").unwrap(), "SAMPLES");
    let first = rx.try_recv().unwrap();
    match first {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "ots_apply");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

// ────────────────────────────────────────────────────────────────────
//  §4 — MandateApply through dispatch_node
// ────────────────────────────────────────────────────────────────────

/// §Fase 119.b — this routing test used to pass through the identity
/// passthrough with an unresolvable name. It now routes a DECLARED mandate
/// whose constraint the target already satisfies, so the real enforcement
/// path completes via the free pre-check (zero attempts) and the binding
/// still lands — same wire shape, real handler.
#[tokio::test]
async fn dispatch_node_routes_mandate_apply() {
    let (mut ctx, mut rx) = fresh_ctx();
    ctx.mandate_specs = std::sync::Arc::new(vec![test_mandate_spec(
        "retention_30d",
        "must contain \"log\"",
    )]);
    dispatch_node(&mandate_apply("retention_30d", "log_entry", "retained"), &mut ctx)
        .await
        .unwrap();
    assert_eq!(ctx.let_bindings.get("retained").unwrap(), "log_entry");
    let first = rx.try_recv().unwrap();
    match first {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "mandate_apply");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

// ────────────────────────────────────────────────────────────────────
//  §5 — ComputeApply through dispatch_node
// ────────────────────────────────────────────────────────────────────


/// §Fase 111.f — a real declared compute: `Sum(a, b) = a + b`.
///
/// Before §111, none of the tests below needed one: `run_compute_apply` bound the
/// literal string `"compute:sum(100, 200)"` for ANY name, declared or not, and the
/// assertions below pinned exactly that. The placeholder was not something the
/// suite missed — the suite REQUIRED it.
fn declare_sum(ctx: &mut DispatchCtx) {
    use axon::ir_nodes::{IRCompute, IRExpr, IRParameter};
    let p = |n: &str| IRParameter {
        node_type: "parameter",
        source_line: 0,
        source_column: 0,
        name: n.into(),
        type_name: "Number".into(),
        generic_param: String::new(),
        optional: false,
    };
    ctx.compute_specs = std::sync::Arc::new(vec![IRCompute {
        node_type: "compute",
        source_line: 0,
        source_column: 0,
        name: "sum".into(),
        shield_ref: String::new(),
        parameters: vec![p("a"), p("b")],
        return_type: "Number".into(),
        body: Some(IRExpr::Binary {
            op: "add".into(),
            lhs: Box::new(IRExpr::Ref { path: "a".into() }),
            rhs: Box::new(IRExpr::Ref { path: "b".into() }),
        }),
    }]);
}

#[tokio::test]
async fn dispatch_node_routes_compute_apply() {
    let (mut ctx, mut rx) = fresh_ctx();
    ctx.let_bindings.insert("x".into(), "100".into());
    ctx.let_bindings.insert("y".into(), "200".into());
    declare_sum(&mut ctx);
    dispatch_node(&compute_apply("sum", vec!["x", "y"], "total"), &mut ctx)
        .await
        .unwrap();
    // 100 + 200 = 300. A NUMBER. This assertion used to read
    // `"compute:sum(100, 200)"` — the lie, pinned green (§111 F10).
    assert_eq!(ctx.let_bindings.get("total").unwrap(), "300");
    let first = rx.try_recv().unwrap();
    match first {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "compute_apply");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

#[tokio::test]
async fn compute_apply_to_an_undeclared_compute_refuses() {
    // Was `compute_apply_with_no_arguments`, and it asserted that applying an
    // UNDECLARED compute called `ping` bound the string `"compute:ping()"`. The
    // handler could not tell a declared function from an imaginary one, because
    // it never looked: it formatted a string from whatever name it was given.
    let (mut ctx, _rx) = fresh_ctx();
    let err = dispatch_node(&compute_apply("ping", vec![], "result"), &mut ctx)
        .await
        .expect_err("an undeclared compute must refuse");
    assert!(format!("{err:?}").contains("does not resolve to a declared compute"));
    assert!(
        ctx.let_bindings.get("result").is_none(),
        "nothing may be bound for a compute that did not compute"
    );
}

// ────────────────────────────────────────────────────────────────────
//  §6 — Listen through dispatch_node
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_node_routes_listen() {
    let (mut ctx, mut rx) = fresh_ctx();
    dispatch_node(&listen("inbox_chan", "msg"), &mut ctx).await.unwrap();
    assert_eq!(ctx.let_bindings.get("msg").unwrap(), "(awaiting inbox_chan)");
    let first = rx.try_recv().unwrap();
    match first {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "listen");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

// ────────────────────────────────────────────────────────────────────
//  §7 — DaemonStep through dispatch_node
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_node_routes_daemon_step() {
    let (mut ctx, mut rx) = fresh_ctx();
    dispatch_node(&daemon_step("supervisor"), &mut ctx).await.unwrap();
    assert_eq!(
        ctx.let_bindings.get("supervisor_invoked").unwrap(),
        "daemon:supervisor"
    );
    let first = rx.try_recv().unwrap();
    match first {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "daemon_step");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

// ────────────────────────────────────────────────────────────────────
//  §8 — Cancel propagation
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cancel_propagates_into_every_algebraic_handler() {
    let nodes: Vec<IRFlowNode> = vec![
        shield_apply("s", "t", "o"),
        ots_apply("s", "t", "o"),
        mandate_apply("s", "t", "o"),
        compute_apply("sum", vec!["a", "b"], "o"),
        listen("c", "e"),
        daemon_step("d"),
    ];

    for node in nodes {
        let cancel = CancellationFlag::new();
        cancel.cancel();
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert!(
            matches!(outcome, Err(DispatchError::UpstreamCancelled)),
            "expected UpstreamCancelled for {node:?}, got {outcome:?}"
        );
    }
}

// ────────────────────────────────────────────────────────────────────
//  §9 — Composition with cognitive + orchestration
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn shield_apply_inside_for_in_per_iter() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("docs".into(), "doc1,doc2,doc3".into());
    let for_in = IRFlowNode::ForIn(IRForIn {
        node_type: "for_in",
        source_line: 0,
        source_column: 0,
        variable: "current_doc".into(),
        iterable: "docs".into(),
        body: vec![shield_apply("hipaa", "current_doc", "current_doc_safe")],
    });
    dispatch_node(&for_in, &mut ctx).await.unwrap();
    // After last iter, current_doc_safe holds the last shielded value.
    assert_eq!(ctx.let_bindings.get("current_doc_safe").unwrap(), "doc3");
}

#[tokio::test]
async fn compute_apply_chained_with_remember() {
    let (mut ctx, _rx) = fresh_ctx();
    declare_sum(&mut ctx);
    ctx.let_bindings.insert("base".into(), "40".into());
    ctx.let_bindings.insert("other".into(), "2".into());
    dispatch_node(
        &compute_apply("sum", vec!["base", "other"], "computed"),
        &mut ctx,
    )
    .await
    .unwrap();
    assert_eq!(ctx.let_bindings.get("computed").unwrap(), "42");

    // Now Remember the result under a stable key.
    dispatch_node(
        &IRFlowNode::Remember(IRRememberStep {
            node_type: "remember",
            source_line: 0,
            source_column: 0,
            expression: "computed".into(),
            memory_target: "checkpoint".into(),
        }),
        &mut ctx,
    )
    .await
    .unwrap();

    // The remembered checkpoint is the COMPUTED value (40 + 2), not the text
    // "compute:transform(value-A)" — which is what this assertion used to demand.
    assert_eq!(ctx.let_bindings.get("checkpoint").unwrap(), "42");
}

#[tokio::test]
async fn shield_chain_idempotent_across_two_invocations() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("phi_doc".into(), "patient John".into());

    dispatch_node(&shield_apply("hipaa", "phi_doc", "round1"), &mut ctx)
        .await
        .unwrap();
    dispatch_node(&shield_apply("hipaa", "round1", "round2"), &mut ctx)
        .await
        .unwrap();

    // OSS identity passthrough — both rounds preserve content.
    assert_eq!(ctx.let_bindings.get("round1").unwrap(), "patient John");
    assert_eq!(ctx.let_bindings.get("round2").unwrap(), "patient John");
}

// ────────────────────────────────────────────────────────────────────
//  §10 — Step counter discipline
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn algebraic_handlers_advance_step_counter() {
    let (mut ctx, _rx) = fresh_ctx();
    declare_sum(&mut ctx);
    ctx.let_bindings.insert("a".into(), "1".into());
    ctx.let_bindings.insert("b".into(), "2".into());
    assert_eq!(ctx.step_counter, 0);
    dispatch_node(&shield_apply("s", "t", "o"), &mut ctx).await.unwrap();
    assert_eq!(ctx.step_counter, 1);
    // §Fase 119.c — the ots must resolve now; register a transformer for it.
    ctx.ots_specs = std::sync::Arc::new(vec![test_ots_spec("s")]);
    axon::ots_registry::register_ots_transformer("s", std::sync::Arc::new(UpperTransformer));
    dispatch_node(&ots_apply("s", "t", "o"), &mut ctx).await.unwrap();
    axon::ots_registry::unregister_ots_transformer("s");
    assert_eq!(ctx.step_counter, 2);
    // §Fase 119.b — the mandate must resolve now; "t" satisfies its clause.
    ctx.mandate_specs = std::sync::Arc::new(vec![test_mandate_spec(
        "s",
        "must contain \"t\"",
    )]);
    dispatch_node(&mandate_apply("s", "t", "o"), &mut ctx).await.unwrap();
    assert_eq!(ctx.step_counter, 3);
    dispatch_node(&compute_apply("sum", vec!["a", "b"], "o"), &mut ctx).await.unwrap();
    assert_eq!(ctx.step_counter, 4);
    dispatch_node(&listen("c", "e"), &mut ctx).await.unwrap();
    assert_eq!(ctx.step_counter, 5);
    dispatch_node(&daemon_step("d"), &mut ctx).await.unwrap();
    assert_eq!(ctx.step_counter, 6);
}
