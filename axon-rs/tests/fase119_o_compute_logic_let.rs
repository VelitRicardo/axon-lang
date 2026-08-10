//! §Fase 119.o — **`compute`'s published body form COMPUTES.**
//!
//! Four README blocks (§117 ledger rows 48–51) write the same shape:
//!
//! ```text
//! compute CalculatePremium {
//!     input: base_rate (Float), risk_factor (Float), years (Float)
//!     output: PremiumResult
//!     logic {
//!         let annual   = base_rate * risk_factor
//!         let total    = annual * years
//!         let discount = total * 0.05
//!         let final    = total - discount
//!         return final
//!     }
//! }
//! ```
//!
//! and none of it parsed. `input:` and `output:` reached `skip_value()`, so a
//! compute declared this way had NO PARAMETERS and `run_compute_apply` refused
//! it on arity; `logic { … }` fell through to `parse_expr()`, which met the bare
//! word `logic` and reported an expression the author never wrote. §111.f had
//! made `compute` a pure function over ONE expression, and `Expr` had no `Let`.
//!
//! **Why `Expr::Let` and not a binding list on the compute.** Substituting the
//! bindings into the return expression re-evaluates each bound term once per
//! mention — `let total = annual * years` is named three times in that block, so
//! `annual * years` would be computed three times, and for anything non-total
//! the meaning changes, not just the cost. As a nested term it is one evaluation
//! per binding, and shadowing falls out of the nesting rather than being
//! reimplemented.
//!
//! Everything below starts at `.axon` SOURCE (law 4 — a gate that hand-builds IR
//! proves the engine and says nothing about whether any published program
//! reaches it).

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::DispatchCtx;
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::IRProgram;
use tokio::sync::mpsc;

fn ir_of(src: &str) -> IRProgram {
    let tokens = axon_frontend::lexer::Lexer::new(src, "gate.axon")
        .tokenize()
        .expect("lex");
    let prog = axon_frontend::parser::Parser::new(tokens)
        .parse()
        .expect("the published compute form must parse");
    axon_frontend::ir_generator::IRGenerator::new().generate(&prog)
}

fn ctx() -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx),
        rx,
    )
}

/// README's `CalculatePremium`, verbatim.
const PREMIUM: &str = r#"
compute CalculatePremium {
    input: base_rate (Float), risk_factor (Float), years (Float)
    output: PremiumResult
    logic {
        let annual = base_rate * risk_factor
        let total = annual * years
        let discount = total * 0.05
        let final = total - discount
        return final
    }
}
"#;

// ── §1 — the published declaration parses and carries its parts ──────────────

#[test]
fn s1_input_output_and_logic_all_reach_the_ir() {
    let ir = ir_of(PREMIUM);
    let c = ir
        .compute_specs
        .first()
        .expect("the compute must reach the IR");

    assert_eq!(
        c.parameters.len(),
        3,
        "`input:` declares three parameters; they used to be skipped entirely, \
         which is why every published compute was refused on arity"
    );
    assert_eq!(c.parameters[0].name, "base_rate");
    assert_eq!(
        c.return_type, "PremiumResult",
        "`output:` is the field spelling of `-> T`"
    );
    assert!(c.body.is_some(), "`logic {{ }}` must lower to a body");
}

/// The chain nests — one `Let` per binding, not a flattened substitution.
#[test]
fn s1b_the_let_chain_lowers_to_nested_let_terms() {
    use axon::ir_nodes::IRExpr;
    let ir = ir_of(PREMIUM);
    let body = ir.compute_specs[0].body.clone().expect("body");

    let mut depth = 0;
    let mut cur = &body;
    while let IRExpr::Let { body: inner, .. } = cur {
        depth += 1;
        cur = inner;
    }
    assert_eq!(
        depth, 4,
        "four `let`s must produce four nested Let terms — a flat structure would \
         mean the bindings were substituted, and `total` is mentioned twice"
    );
}

// ── §2 — and it EVALUATES, natively ─────────────────────────────────────────

/// The arithmetic, end to end. base=100, risk=1.5, years=2
///   annual   = 150
///   total    = 300
///   discount = 15
///   final    = 285
#[tokio::test]
async fn s2_the_published_compute_produces_the_right_number() {
    let src = format!(
        "{PREMIUM}\nflow F(x: String) -> Text {{\n  \
         compute CalculatePremium on base_rate, risk_factor, years -> premium\n}}"
    );
    let ir = ir_of(&src);
    let (mut c, _rx) = ctx();
    c.let_bindings.insert("base_rate".into(), "100".into());
    c.let_bindings.insert("risk_factor".into(), "1.5".into());
    c.let_bindings.insert("years".into(), "2".into());

    let node = ir.flows[0]
        .steps
        .iter()
        .find_map(|n| match n {
            axon::ir_nodes::IRFlowNode::ComputeApply(a) => Some(a.clone()),
            _ => None,
        })
        .expect("the compute application must lower");

    c.compute_specs = std::sync::Arc::new(ir.compute_specs.clone());

    axon::flow_dispatcher::algebraic_handlers::run_compute_apply(&node, &mut c)
        .await
        .expect("a compute with a real body must compute");

    assert_eq!(
        c.let_bindings.get("premium").map(String::as_str),
        Some("285"),
        "the let chain must evaluate to 285 — bindings: {:?}",
        c.let_bindings
    );
}

// ── §3 — scoping is lexical ─────────────────────────────────────────────────

/// An inner binding shadows the CALLER's frame, and the caller's value is not
/// consulted for a name the block itself binds. Getting this backwards would let
/// a stale outer binding silently win over the value the author just wrote.
#[tokio::test]
async fn s3_a_let_shadows_the_callers_frame() {
    let src = r#"
compute Shadow {
    input: seed (Float)
    output: Float
    logic {
        let scale = 2
        return seed * scale
    }
}
flow F(x: String) -> Text {
  compute Shadow on seed -> out
}
"#;
    let ir = ir_of(src);
    let (mut c, _rx) = ctx();
    c.let_bindings.insert("seed".into(), "10".into());
    // A caller-frame binding of the SAME name the block binds internally.
    c.let_bindings.insert("scale".into(), "999".into());

    let node = ir.flows[0]
        .steps
        .iter()
        .find_map(|n| match n {
            axon::ir_nodes::IRFlowNode::ComputeApply(a) => Some(a.clone()),
            _ => None,
        })
        .expect("compute application");
    c.compute_specs = std::sync::Arc::new(ir.compute_specs.clone());

    axon::flow_dispatcher::algebraic_handlers::run_compute_apply(&node, &mut c)
        .await
        .expect("compute");

    assert_eq!(
        c.let_bindings.get("out").map(String::as_str),
        Some("20"),
        "`let scale = 2` must win over the caller's `scale = 999`"
    );
    assert_eq!(
        c.let_bindings.get("scale").map(String::as_str),
        Some("999"),
        "and the caller's binding must SURVIVE — a `let` inside a compute is \
         scoped to it, not a mutation of the frame it was called from"
    );
}

// ── §4 — the block is closed, and `return` is required ──────────────────────

fn parse_err(src: &str) -> String {
    let tokens = axon_frontend::lexer::Lexer::new(src, "t.axon")
        .tokenize()
        .expect("lex");
    axon_frontend::parser::Parser::new(tokens)
        .parse()
        .err()
        .map(|e| e.message)
        .unwrap_or_else(|| "<parsed>".to_string())
}

/// Without `return` the block binds names and yields nothing, and the compute
/// would have to invent a result — the one thing a deterministic primitive must
/// never do.
#[test]
fn s4_a_logic_block_without_return_is_refused() {
    let msg = parse_err(
        "compute NoReturn {\n input: a (Float)\n output: Float\n logic {\n let b = a * 2\n }\n}",
    );
    assert!(
        msg.contains("return"),
        "the diagnostic must name what is missing: {msg}"
    );
}

/// `compute` is a PURE function (its paper: *pureza categórica de los morfismos
/// funcionales*), so a statement that could have an effect is refused rather
/// than parsed and dropped.
#[test]
fn s4b_a_non_let_statement_in_logic_is_refused() {
    let msg = parse_err(
        "compute Impure {\n input: a (Float)\n output: Float\n logic {\n persist into S { x: a }\n return a\n }\n}",
    );
    assert!(
        msg.contains("logic") && msg.contains("let"),
        "the diagnostic must say what the block admits: {msg}"
    );
}

// ── §5 — the older form still compiles ──────────────────────────────────────

/// §111.f's `compute N(p: T) -> T { <expr> }` is untouched. This landing is
/// additive: an adopter's existing program must keep compiling.
#[test]
fn s5_the_typed_parameter_form_still_parses() {
    let ir = ir_of("compute Add(a: Float, b: Float) -> Float { a + b }");
    let c = &ir.compute_specs[0];
    assert_eq!(c.parameters.len(), 2);
    assert_eq!(c.return_type, "Float");
    assert!(c.body.is_some());
}
