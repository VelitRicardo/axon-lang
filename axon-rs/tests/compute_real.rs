//! v2.67.0 — `compute` MADE REAL. The loudest lie in the README, retired.
//!
//! # What it was (v2.67.0 F10)
//!
//! `invoke_compute_capability` returned the **literal string**
//! `"compute:CalculatePremium(x_value, 1.2)"`. That string was bound under the
//! step's output name, and **a downstream step consumed it as if it were a
//! number.**
//!
//! It did not fall through to the LLM, so it was not *hallucinating* in the v2.63.0
//! sense. It was worse in one specific way: a **fabricated determinism
//! guarantee**. The README advertises `compute` as *"Deterministic muscle —
//! native Fast-Path execution **bypassing the LLM**"* and asserts a complexity
//! class (*"compute steps: O(n) — linear in input size, native execution"*).
//!
//! `IRCompute` carried only `name` and `shield_ref` — **no parameters, no body**.
//! The parser skipped the parameter list token by token, and the apply site
//! hardcoded `arguments: Vec::new()`. There was nothing to execute, natively or
//! otherwise. The promise was not merely unmet; it was **unmeetable**.
//!
//! # What it is
//!
//! A named **pure function over the v2.26.0 expression language** — the closed,
//! total, side-effect-free term algebra the runtime already evaluates natively
//! with `eval_expr` (the same evaluator behind `let`, `grad` and `conditional`).
//! Linear in the term. No model in the loop. The advertised claim, made *true*
//! rather than made louder.
//!
//! Pins:
//! 1. A `compute` returns a **NUMBER** — the arithmetic is real and exact.
//! 2. **Zero tokens.** The LLM is not in the loop, which is the entire promise.
//! 3. The result is usable downstream *as a number* (it used to be the text
//!    `"compute:Name(args)"`).
//! 4-8. Every failure is a **refusal**, never a string wearing a number's clothes:
//!    unknown compute · no body · arity mismatch · unevaluable term (division by
//!    zero) · unresolvable argument.
//! 9. A parameter name cannot leak out of the call frame and shadow the caller.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::*;
use std::sync::Arc;
use tokio::sync::mpsc;

fn param(name: &str) -> IRParameter {
    IRParameter {
        node_type: "parameter",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        type_name: "Number".into(),
        generic_param: String::new(),
        optional: false,
    }
}

fn num(v: i64) -> IRExpr {
    IRExpr::Lit {
        lit: IRExprLit::Int { value: v },
    }
}

fn r(path: &str) -> IRExpr {
    IRExpr::Ref { path: path.into() }
}

fn bin(op: &str, lhs: IRExpr, rhs: IRExpr) -> IRExpr {
    IRExpr::Binary {
        op: op.into(),
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

/// `compute Premium(base: Number, rate: Number) -> Number { base * rate + 5 }`
fn premium_spec(body: Option<IRExpr>, params: Vec<IRParameter>) -> IRCompute {
    IRCompute {
        node_type: "compute",
        source_line: 0,
        source_column: 0,
        name: "Premium".into(),
        shield_ref: String::new(),
        parameters: params,
        return_type: "Number".into(),
        body,
    }
}

fn apply(args: Vec<&str>, out: &str) -> IRFlowNode {
    IRFlowNode::ComputeApply(IRComputeApplyStep {
        node_type: "compute_apply",
        source_line: 0,
        source_column: 0,
        compute_name: "Premium".into(),
        arguments: args.into_iter().map(String::from).collect(),
        output_name: out.into(),
    })
}

fn let_node(target: &str, value: &str) -> IRFlowNode {
    IRFlowNode::Let(IRLetBinding {
        node_type: "let_binding",
        source_line: 0,
        source_column: 0,
        target: target.into(),
        value: value.into(),
        value_kind: "literal".into(),
        value_ast: None,
    })
}

fn ctx_with(specs: Vec<IRCompute>) -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = DispatchCtx::new("CFlow", "stub", "", CancellationFlag::new(), tx)
        .with_computes(Arc::new(specs));
    (ctx, rx)
}

/// The canonical spec: `{ base * rate + 5 }` over two params.
fn canonical() -> IRCompute {
    premium_spec(
        Some(bin("add", bin("mul", r("base"), r("rate")), num(5))),
        vec![param("base"), param("rate")],
    )
}

// ── 1-3. It actually computes ───────────────────────────────────────────────

/// **The flagship.** `Premium(7, 3) = 7*3 + 5 = 26`. Exactly. A number.
///
/// The old handler bound the string `"compute:Premium(7, 3)"` here.
#[tokio::test]
async fn compute_returns_a_real_number() {
    let (mut c, _rx) = ctx_with(vec![canonical()]);
    dispatch_node(&let_node("amount", "7"), &mut c).await.unwrap();
    dispatch_node(&let_node("r", "3"), &mut c).await.unwrap();

    let outcome = dispatch_node(&apply(vec!["amount", "r"], "premium"), &mut c)
        .await
        .expect("compute must evaluate");

    match outcome {
        NodeOutcome::Completed { output, tokens_emitted, .. } => {
            assert_eq!(
                output, "26",
                "7 * 3 + 5 = 26. The old runtime bound the literal string \
                 \"compute:Premium(7, 3)\" here and a downstream step read it as a number"
            );
            // (2) The entire promise: the LLM is NOT in the loop.
            assert_eq!(
                tokens_emitted, 0,
                "`compute` is advertised as BYPASSING the LLM — a single token here would \
                 falsify the primitive's whole reason to exist"
            );
        }
        other => panic!("expected Completed, got {other:?}"),
    }

    // (3) …and the binding is a NUMBER, usable downstream.
    assert_eq!(c.let_bindings.get("premium").map(String::as_str), Some("26"));
}

/// The arithmetic is real, not a lookup: a different input gives a different
/// answer, and the answer is exact (integers stay integers).
#[tokio::test]
async fn the_arithmetic_is_exact_and_input_dependent() {
    let (mut c, _rx) = ctx_with(vec![canonical()]);
    dispatch_node(&let_node("amount", "100"), &mut c).await.unwrap();
    dispatch_node(&let_node("r", "2"), &mut c).await.unwrap();

    dispatch_node(&apply(vec!["amount", "r"], "premium"), &mut c)
        .await
        .expect("compute must evaluate");

    assert_eq!(
        c.let_bindings.get("premium").map(String::as_str),
        Some("205"),
        "100 * 2 + 5 = 205 — exact integer arithmetic, no float drift"
    );
}

// ── 4-8. Every failure is a REFUSAL, never a plausible-looking string ────────

#[tokio::test]
async fn an_unknown_compute_refuses() {
    let (mut c, _rx) = ctx_with(vec![]);
    let err = dispatch_node(&apply(vec![], "out"), &mut c)
        .await
        .expect_err("an undeclared compute must refuse");
    assert!(format!("{err:?}").contains("does not resolve to a declared compute"));
}

/// The legacy form `compute N { shield: G }` still parses — and applying it is
/// REFUSED, rather than binding the placeholder that started all this.
#[tokio::test]
async fn a_compute_with_no_body_refuses_instead_of_binding_a_placeholder() {
    let (mut c, _rx) = ctx_with(vec![premium_spec(None, vec![])]);
    let err = dispatch_node(&apply(vec![], "out"), &mut c)
        .await
        .expect_err("a bodyless compute must refuse");
    let msg = format!("{err:?}");
    assert!(msg.contains("declares no body"), "got {msg}");
    assert!(
        msg.contains("F10") || msg.contains("expected a number"),
        "the diagnostic must name the failure mode it prevents — a downstream step reading \
         placeholder TEXT as a number; got {msg}"
    );
}

/// A silent arity mismatch would evaluate the body against a stale or missing
/// binding and **still produce a number** — just the wrong one. That is the most
/// dangerous shape available, so it refuses.
#[tokio::test]
async fn an_arity_mismatch_refuses_rather_than_producing_the_wrong_number() {
    let (mut c, _rx) = ctx_with(vec![canonical()]);
    dispatch_node(&let_node("amount", "7"), &mut c).await.unwrap();

    let err = dispatch_node(&apply(vec!["amount"], "premium"), &mut c)
        .await
        .expect_err("1 argument for 2 parameters must refuse");
    let msg = format!("{err:?}");
    assert!(msg.contains("declares 2 parameter"), "got {msg}");
    assert!(
        msg.contains("just the wrong one"),
        "the diagnostic must say WHY silence is unacceptable here; got {msg}"
    );
}

/// The v2.26.0 evaluator fails closed on division by zero. The old handler would
/// have papered over it with a plausible-looking string.
#[tokio::test]
async fn an_unevaluable_term_refuses() {
    let spec = premium_spec(
        Some(bin("div", r("base"), num(0))),
        vec![param("base")],
    );
    let (mut c, _rx) = ctx_with(vec![spec]);
    dispatch_node(&let_node("amount", "7"), &mut c).await.unwrap();

    let err = dispatch_node(&apply(vec!["amount"], "premium"), &mut c)
        .await
        .expect_err("division by zero must refuse");
    assert!(
        format!("{err:?}").contains("must not return something that LOOKS like a result"),
        "got {err:?}"
    );
}

#[tokio::test]
async fn an_unresolvable_argument_refuses() {
    let (mut c, _rx) = ctx_with(vec![canonical()]);
    // Neither `nope` nor `alsonope` is bound.
    let err = dispatch_node(&apply(vec!["nope", "alsonope"], "premium"), &mut c)
        .await
        .expect_err("unresolvable arguments must refuse");
    assert!(format!("{err:?}").contains("did not evaluate"), "got {err:?}");
}

// ── 9. Hygiene: the call frame is a frame ───────────────────────────────────

/// A parameter name must not leak out of the compute and shadow the caller's own
/// binding. `base` is a parameter here AND a caller binding; after the call, the
/// caller's value must be intact.
#[tokio::test]
async fn parameter_names_do_not_leak_out_of_the_call_frame() {
    let (mut c, _rx) = ctx_with(vec![canonical()]);
    dispatch_node(&let_node("base", "999"), &mut c).await.unwrap(); // the CALLER's `base`
    dispatch_node(&let_node("amount", "7"), &mut c).await.unwrap();
    dispatch_node(&let_node("r", "3"), &mut c).await.unwrap();

    dispatch_node(&apply(vec!["amount", "r"], "premium"), &mut c)
        .await
        .expect("compute must evaluate");

    assert_eq!(c.let_bindings.get("premium").map(String::as_str), Some("26"));
    assert_eq!(
        c.let_bindings.get("base").map(String::as_str),
        Some("999"),
        "the compute's parameter `base` leaked out and clobbered the caller's binding — a call \
         frame that does not restore is not a frame"
    );
}
