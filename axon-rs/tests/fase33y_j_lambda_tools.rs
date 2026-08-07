//! §Fase 33.y.j integration tests — Lambda + UseTool (the FINAL
//! 2 variants needed to reach 45/45 IRFlowNode graduation).
//!
//! After this sub-fase: every IRFlowNode variant has a NAMED async
//! handler in `dispatch_node`; `legacy_shim` is structurally
//! unreachable from the dispatcher entry; 33.y.l retires the shim
//! + LegacyShimHandled outcome variant in lockstep cleanup.
//!
//! D-letter coverage:
//! - D1 — 45 of 45 variants graduated (FINAL).
//! - D3 — cancel propagation across both handlers.
//! - D7 — every error case routes through DispatchError.
//! - D8 (preview) — UseTool is the 33.y.k cross-cutting anchor;
//!   33.y.j ships the explicit `use_tool: <name>` IR variant
//!   wire shape; 33.y.k extends pure_shape::run_pure_shape to
//!   plumb ChatRequest.tools through every Step with `apply:
//!   <tool>` declared.
//! - D10 — sync-runner parity: lambda + tool produce deterministic
//!   placeholders for OSS path; enterprise integration preserves
//!   SAME wire envelope.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::lambda_tools::invoke_tool;
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, DispatchError, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::*;
use tokio::sync::mpsc;

fn lambda_spec(name: &str) -> axon::ir_nodes::IRLambdaData {
    axon::ir_nodes::IRLambdaData {
        node_type: "lambda_data",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        ontology: "test.ontology".into(),
        certainty: 0.8,
        temporal_frame_start: String::new(),
        temporal_frame_end: String::new(),
        provenance: "test".into(),
        derivation: "derived".into(),
    }
}

fn fresh_ctx() -> (
    DispatchCtx,
    mpsc::UnboundedReceiver<FlowExecutionEvent>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new(
        "TestFlow",
        "stub",
        "",
        CancellationFlag::new(),
        tx,
    );
    // §Fase 119.c — the ΛD declarations these fixtures apply. The handler
    // elevates for real now; an undeclared name refuses (covered by the
    // fuzz arm below), so the named fixtures must resolve.
    ctx.lambda_data_specs = std::sync::Arc::new(vec![
        lambda_spec("normalize"),
        lambda_spec("clean"),
        lambda_spec("x"),
        lambda_spec("polish"),
        lambda_spec("process"),
    ]);
    (ctx, rx)
}

fn lambda_node(name: &str, target: &str, output_type: &str) -> IRFlowNode {
    IRFlowNode::LambdaDataApply(IRLambdaDataApply {
        node_type: "lambda_data_apply",
        source_line: 0,
        source_column: 0,
        lambda_data_name: name.into(),
        target: target.into(),
        output_type: output_type.into(),
    })
}

fn use_tool_node(tool: &str, arg: &str) -> IRFlowNode {
    IRFlowNode::UseTool(IRUseToolStep {
        node_type: "use_tool",
        source_line: 0,
        source_column: 0,
        tool_name: tool.into(),
        argument: arg.into(),
        named_args: Vec::new(),
    })
}

// ────────────────────────────────────────────────────────────────────
//  §1 — Public helpers
// ────────────────────────────────────────────────────────────────────

// §Fase 119.c — `apply_lambda_data_resolves_target` and
// `apply_lambda_data_literal_when_target_unset` were DELETED here. They
// pinned the placeholder string "lambda:<name>(<target>)" in green — the
// §111 F18 shape — while the real evaluator sat unreached on the dead sync
// path. The elevation suite lives in `flow_dispatcher::lambda_tools::tests`,
// and its fail-closed inversion is
// `an_undeclared_lambda_fails_closed_never_placeholder`.

#[test]
fn invoke_tool_resolves_argument() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("query".into(), "weather".into());
    assert_eq!(
        invoke_tool("web_search", "query", &ctx),
        "tool:web_search(weather)"
    );
}

#[test]
fn invoke_tool_literal_when_argument_unset() {
    let (ctx, _rx) = fresh_ctx();
    assert_eq!(
        invoke_tool("eval", "2+2", &ctx),
        "tool:eval(2+2)"
    );
}

// ────────────────────────────────────────────────────────────────────
//  §2 — LambdaDataApply through dispatch_node
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_node_routes_lambda_with_explicit_output() {
    let (mut ctx, mut rx) = fresh_ctx();
    ctx.let_bindings.insert("data".into(), "raw_input".into());
    dispatch_node(&lambda_node("normalize", "data", "normalized"), &mut ctx)
        .await
        .unwrap();
    let psi: serde_json::Value =
        serde_json::from_str(ctx.let_bindings.get("normalized").unwrap()).expect("ψ JSON");
    assert_eq!(psi["V"], "raw_input", "the elevation carries the resolved target");
    let first = rx.try_recv().unwrap();
    match first {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "lambda_data_apply");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

#[tokio::test]
async fn lambda_canonical_fallback_when_output_empty() {
    let (mut ctx, _rx) = fresh_ctx();
    dispatch_node(&lambda_node("clean", "doc", ""), &mut ctx).await.unwrap();
    let psi: serde_json::Value =
        serde_json::from_str(ctx.let_bindings.get("doc_lambda_applied").unwrap())
            .expect("ψ JSON");
    assert_eq!(psi["V"], "doc", "unresolved binding elevates the literal");
}

// ────────────────────────────────────────────────────────────────────
//  §3 — UseTool through dispatch_node
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_node_routes_use_tool_with_literal_arg() {
    let (mut ctx, mut rx) = fresh_ctx();
    dispatch_node(&use_tool_node("calc", "5*3"), &mut ctx).await.unwrap();
    assert_eq!(
        ctx.let_bindings.get("calc_result").unwrap(),
        "tool:calc(5*3)"
    );
    let first = rx.try_recv().unwrap();
    match first {
        FlowExecutionEvent::StepStart { step_type, .. } => {
            assert_eq!(step_type, "use_tool");
        }
        e => panic!("expected StepStart, got {e:?}"),
    }
}

#[tokio::test]
async fn use_tool_resolves_argument_through_let_bindings() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("user_query".into(), "what is rust".into());
    dispatch_node(&use_tool_node("search", "user_query"), &mut ctx)
        .await
        .unwrap();
    assert_eq!(
        ctx.let_bindings.get("search_result").unwrap(),
        "tool:search(what is rust)"
    );
}

// ────────────────────────────────────────────────────────────────────
//  §4 — Cancel propagation
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cancel_propagates_into_both_handlers() {
    for node in vec![
        lambda_node("x", "y", "z"),
        use_tool_node("t", "a"),
    ] {
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
//  §5 — Composition with orchestration + cognitive
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn lambda_chain_with_remember_round_trip() {
    let (mut ctx, _rx) = fresh_ctx();
    // Bind a target via Remember
    dispatch_node(
        &IRFlowNode::Remember(IRRememberStep {
            node_type: "remember",
            source_line: 0,
            source_column: 0,
            expression: "intermediate-value".into(),
            memory_target: "draft".into(),
        }),
        &mut ctx,
    )
    .await
    .unwrap();

    // Now apply a lambda over the bound target
    dispatch_node(&lambda_node("polish", "draft", "final"), &mut ctx)
        .await
        .unwrap();
    // §Fase 119.c — the recalled binding elevates to a REAL ψ whose V is the
    // remembered value; the placeholder shape is dead.
    let psi: serde_json::Value =
        serde_json::from_str(ctx.let_bindings.get("final").unwrap()).expect("ψ JSON");
    assert_eq!(psi["V"], "intermediate-value");
    assert_eq!(psi["spec_name"], "polish");
}

#[tokio::test]
async fn use_tool_inside_for_in_per_iter() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("queries".into(), "rust,python,go".into());
    let for_in = IRFlowNode::ForIn(IRForIn {
        node_type: "for_in",
        source_line: 0,
        source_column: 0,
        variable: "q".into(),
        iterable: "queries".into(),
        body: vec![use_tool_node("web_search", "q")],
    });
    dispatch_node(&for_in, &mut ctx).await.unwrap();
    // After last iter, web_search_result holds the last invocation.
    assert_eq!(
        ctx.let_bindings.get("web_search_result").unwrap(),
        "tool:web_search(go)"
    );
}

#[tokio::test]
async fn lambda_then_use_tool_chain() {
    let (mut ctx, _rx) = fresh_ctx();
    ctx.let_bindings.insert("input".into(), "raw".into());

    // Apply lambda → bind to "processed"
    dispatch_node(&lambda_node("process", "input", "processed"), &mut ctx)
        .await
        .unwrap();
    // Now use a tool on the processed result
    dispatch_node(&use_tool_node("validate", "processed"), &mut ctx)
        .await
        .unwrap();

    // §Fase 119.c — the chain now threads a REAL ψ into the tool: the bound
    // value is `tool:validate(<ψ JSON>)`, not the old nested placeholder.
    let bound = ctx.let_bindings.get("validate_result").unwrap();
    assert!(bound.starts_with("tool:validate("), "{bound}");
    assert!(
        bound.contains("\"V\":\"raw\"") && bound.contains("\"spec_name\":\"process\""),
        "the elevated ψ must flow through the chain: {bound}"
    );
}

// ────────────────────────────────────────────────────────────────────
//  §6 — Step counter discipline
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn both_handlers_advance_step_counter() {
    let (mut ctx, _rx) = fresh_ctx();
    assert_eq!(ctx.step_counter, 0);
    dispatch_node(&lambda_node("x", "y", "z"), &mut ctx).await.unwrap();
    assert_eq!(ctx.step_counter, 1);
    dispatch_node(&use_tool_node("t", "a"), &mut ctx).await.unwrap();
    assert_eq!(ctx.step_counter, 2);
}

// ────────────────────────────────────────────────────────────────────
//  §7 — Fuzz pack (~800 LCG iters)
// ────────────────────────────────────────────────────────────────────

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        let mixed = seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(0xBB67_AE85_84CA_A73B);
        Self(mixed.max(1))
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    fn range(&mut self, max: usize) -> usize {
        (self.next_u64() as usize) % max.max(1)
    }
    fn ascii_string(&mut self, len: usize) -> String {
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            let c = (self.range(95) + 32) as u8;
            s.push(c as char);
        }
        s
    }
    fn ascii_with_random_len(&mut self, max: usize) -> String {
        let len = self.range(max) + 1;
        self.ascii_string(len)
    }
    fn boolean(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

fn assert_no_panic(label: &str, outcome: &Result<NodeOutcome, DispatchError>) {
    match outcome {
        Ok(_) => {}
        Err(DispatchError::UpstreamCancelled) => {}
        Err(DispatchError::ChannelClosed) => {}
        // §Fase 119.c — a random name is an UNDECLARED ΛD, and refusing is
        // the correct total outcome (this fuzz used to "pass" because the
        // handler fabricated a placeholder string for any input at all).
        Err(DispatchError::BackendError { name, .. }) if name.starts_with("lambda:") => {}
        Err(other) => panic!("{label}: unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn fuzz_lambda_never_panics() {
    let mut lcg = Lcg::new(0xAB_CD_EF_12_34_56_78_9A);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::LambdaDataApply(IRLambdaDataApply {
            node_type: "lambda_data_apply",
            source_line: 0,
            source_column: 0,
            lambda_data_name: lcg.ascii_with_random_len(20),
            target: lcg.ascii_with_random_len(20),
            output_type: lcg.ascii_with_random_len(15),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("lambda iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_use_tool_never_panics() {
    let mut lcg = Lcg::new(0xBC_DE_F1_23_45_67_89_AB);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRFlowNode::UseTool(IRUseToolStep {
            node_type: "use_tool",
            source_line: 0,
            source_column: 0,
            tool_name: lcg.ascii_with_random_len(20),
            argument: lcg.ascii_with_random_len(30),
            named_args: Vec::new(),
        });
        let outcome = dispatch_node(&node, &mut ctx).await;
        assert_no_panic(&format!("use_tool iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_lambda_use_tool_cancel_random_pre_dispatch() {
    let mut lcg = Lcg::new(0xCD_EF_12_34_56_78_9A_BC);
    for iter in 0..200 {
        let cancel = CancellationFlag::new();
        let pre_cancel = lcg.boolean();
        if pre_cancel {
            cancel.cancel();
        }
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);

        let node = if lcg.boolean() {
            IRFlowNode::LambdaDataApply(IRLambdaDataApply {
                node_type: "lambda_data_apply",
                source_line: 0,
                source_column: 0,
                lambda_data_name: "n".into(),
                target: "t".into(),
                output_type: "o".into(),
            })
        } else {
            IRFlowNode::UseTool(IRUseToolStep {
                node_type: "use_tool",
                source_line: 0,
                source_column: 0,
                tool_name: "t".into(),
                argument: "a".into(),
                named_args: Vec::new(),
            })
        };

        let outcome = dispatch_node(&node, &mut ctx).await;
        if pre_cancel {
            assert!(
                matches!(outcome, Err(DispatchError::UpstreamCancelled)),
                "iter={iter}: expected UpstreamCancelled, got {outcome:?}"
            );
        } else {
            assert_no_panic(&format!("cancel iter={iter}"), &outcome);
        }
    }
}

#[tokio::test]
async fn fuzz_lambda_tool_chains_never_panic() {
    let mut lcg = Lcg::new(0xDE_F1_23_45_67_89_AB_CD);
    for iter in 0..200 {
        let (mut ctx, _rx) = fresh_ctx();
        // Optionally pre-seed a target.
        if lcg.boolean() {
            ctx.let_bindings.insert("base".into(), lcg.ascii_with_random_len(15));
        }
        // Chain: lambda(base) → use_tool(lambda_output)
        let l1 = lambda_node("step1", "base", "after_lambda");
        let t1 = use_tool_node("validator", "after_lambda");
        let _ = dispatch_node(&l1, &mut ctx).await;
        let outcome = dispatch_node(&t1, &mut ctx).await;
        assert_no_panic(&format!("chain iter={iter}"), &outcome);
    }
}

#[test]
fn fuzz_pack_total_iter_count() {
    let total = (2 * 200) + 200 + 200;
    assert_eq!(total, 800, "33.y.j fuzz pack target: 800 LCG iters");
}
