//! v2.69.0 — **the declared `budget` finally bounds the tool calls an adopter
//! actually makes.**
//!
//! # Two holes, not one
//!
//! v2.69.0's census found `budget` to be a **real** engine — a refilling token bucket
//! (`rate:`), a tumbling window (`max:`), fail-closed, with `on_exhausted ∈
//! {block, defer, shed}` honoured. And then found that it governed almost nothing:
//!
//! **Hole 1 — the gate was on one path.** It was inlined in
//! `pure_shape::run_step_streaming_tool`, reachable only by a tool declaring
//! `effects: stream:*`, inside a `daemon`, on the enterprise supervisor. The
//! canonical `use Tool(…)` path had **zero** budget references — and that is the
//! very function `advertised.rs` cites as PROOF that `tool` is Real.
//!
//! **Hole 2 — the bound could not be WRITTEN.** `budget` was a field of `daemon`
//! and of nothing else. So an adopter deploying an HTTP endpoint that calls a
//! vendor tool had **no way in the language to bound how often it did so**. The
//! call site even said so, in a comment, as though it were a decision:
//!
//! > *"v2.28.0 — the HTTP path carries no daemon budget (effect budgets are a
//! > `daemon` surface). Tool dispatch is unconditional here."*
//!
//! **A gap written down until it reads like a design.** The HTTP endpoint is what
//! people actually deploy.
//!
//! # The assertion that separates a wire from a label
//!
//! A `BudgetGate` is a token bucket. **Build it per request and it starts full
//! every time** — `rate: 100 per minute` would silently mean *"100 per request,
//! forever"*, a gate that cannot deny, and every test of it would pass.
//!
//! So the gate is built ONCE at deploy, held on `ServerState`, and outlives the
//! requests it governs. `a_budget_is_not_refilled_by_the_arrival_of_a_new_request`
//! below is the test that proves it, and it is the point of the whole step.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::budget_gate::{charge, BudgetGrant};
use axon::flow_dispatcher::{DispatchCtx, DispatchError};
use axon::ir_nodes::{IRBudget, IRBudgetQuota};
use axon::runtime::budget_kernel::BudgetGate;
use std::sync::{Arc, Mutex};

fn quota(kind: &str, limit: i64, period: &str, tool: &str) -> IRBudgetQuota {
    IRBudgetQuota {
        kind: kind.into(),
        limit,
        period: period.into(),
        effect: tool.into(),
    }
}

fn budget(name: &str, on_exhausted: &str, quotas: Vec<IRBudgetQuota>) -> IRBudget {
    IRBudget {
        node_type: "budget",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        quotas,
        on_exhausted: on_exhausted.into(),
    }
}

/// A minimal `DispatchCtx` — the gate only reads `ctx.budget`.
fn ctx() -> DispatchCtx {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx)
}

fn gate_of(b: &IRBudget) -> Arc<Mutex<BudgetGate>> {
    Arc::new(Mutex::new(BudgetGate::from_ir(
        b,
        "program",
        chrono::Utc::now(),
    )))
}

// ── The grammar hole ─────────────────────────────────────────────────────────

/// **A top-level `budget` compiles.** Until v2.69.0 it could only be written inside a
/// `daemon`, so an HTTP endpoint — the thing adopters deploy — was unbudgetable.
#[test]
fn a_top_level_budget_compiles_it_used_to_be_a_daemon_field_and_nothing_else() {
    const SRC: &str = r#"
tool Search { provider: http  runtime: search }

budget VendorSpend {
    rate: 2 per minute on Tool(Search)
    on_exhausted: block
}

flow Ask() -> Unit {
    use Search
}
axonendpoint AskEndpoint { public: true  method: POST  path: "/ask"  execute: Ask }
"#;
    let tokens = axon_frontend::lexer::Lexer::new(SRC, "b.axon")
        .tokenize()
        .expect("lex");
    let prog = axon_frontend::parser::Parser::new(tokens).parse().expect("parse");
    let errs = axon_frontend::type_checker::TypeChecker::new(&prog).check();
    assert!(errs.is_empty(), "a top-level budget must type-check: {errs:?}");

    let ir = axon_frontend::ir_generator::IRGenerator::new().generate(&prog);
    assert_eq!(ir.budgets.len(), 1, "the budget must reach the IR");
    assert_eq!(ir.budgets[0].name, "VendorSpend");
    assert_eq!(ir.budgets[0].quotas[0].effect, "Search");
    assert_eq!(ir.budgets[0].quotas[0].limit, 2);
}

/// The same laws apply as to a daemon's budget — reused, not re-implemented.
/// `on Tool(Ghost)` names no declared tool ⇒ **axon-T830**.
#[test]
fn a_top_level_budget_is_held_to_the_same_laws_t830_t834() {
    const SRC: &str = r#"
budget Bad { rate: 5 per minute on Tool(Ghost) }
"#;
    let tokens = axon_frontend::lexer::Lexer::new(SRC, "b.axon")
        .tokenize()
        .expect("lex");
    let prog = axon_frontend::parser::Parser::new(tokens).parse().expect("parse");
    let errs = axon_frontend::type_checker::TypeChecker::new(&prog).check();
    let joined = errs
        .iter()
        .map(|e| e.message.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("axon-T830"),
        "a budget over an undeclared tool must be refuted — a quota on a tool that does not \
         exist bounds nothing and hides a typo. Got: {joined}"
    );
}

// ── The gate itself ──────────────────────────────────────────────────────────

/// 🔴 **THE ASSERTION THIS SUB-FASE EXISTS FOR.**
///
/// A budget of 2 permits two calls and **denies the third** — and the denial
/// **survives across requests**, because the bucket is not refilled by the arrival
/// of a new one.
///
/// If the gate were rebuilt per request (the obvious, wrong implementation), this
/// test would pass its first two charges forever, `rate: 2 per minute` would mean
/// *"2 per request"*, and the budget would be a **label**: honoured by a gate
/// incapable of denying.
#[test]
fn a_budget_is_not_refilled_by_the_arrival_of_a_new_request() {
    let b = budget("VendorSpend", "block", vec![quota("max", 2, "minute", "Search")]);
    // ONE gate, shared — exactly as `ServerState::budgets` holds it.
    let gate = gate_of(&b);

    let mut c = ctx().with_budget(gate.clone());

    // Request 1 — two calls granted, the third denied.
    assert_eq!(charge(&c, "Search").unwrap(), BudgetGrant::Granted);
    assert_eq!(charge(&c, "Search").unwrap(), BudgetGrant::Granted);
    assert!(
        matches!(
            charge(&c, "Search"),
            Err(DispatchError::EffectQuotaExhausted { .. })
        ),
        "the third call must be REFUSED — `max: 2 per minute` means two"
    );

    // A NEW request arrives. It resolves the gate from ServerState (the same Arc);
    // it does NOT build a fresh one.
    c = ctx().with_budget(gate.clone());

    assert!(
        matches!(
            charge(&c, "Search"),
            Err(DispatchError::EffectQuotaExhausted { .. })
        ),
        "🔴 THE POINT OF v2.69.0. The quota must STILL be exhausted. A token bucket rebuilt per \
         request starts full every time, so `max: 2 per minute` would silently mean '2 per \
         request, forever' — a gate that cannot deny, and a budget that is a decoration. This \
         is the difference between v2.69.0 being a wire and being a label."
    );
}

/// An **unbudgeted** tool is granted unconditionally. A budget you did not declare
/// cannot deny you — byte-identical to pre-v2.28.0.
#[test]
fn a_tool_no_budget_names_is_granted_unconditionally() {
    let b = budget("VendorSpend", "block", vec![quota("max", 1, "minute", "Search")]);
    let ctx = ctx().with_budget(gate_of(&b));

    for _ in 0..50 {
        assert_eq!(
            charge(&ctx, "SomeOtherTool").unwrap(),
            BudgetGrant::Granted,
            "a tool no quota names must never be denied"
        );
    }
}

/// A program with **no budget at all** charges nothing and behaves exactly as
/// before v2.69.0.
#[test]
fn a_program_with_no_budget_is_unchanged() {
    let ctx = ctx();
    for _ in 0..50 {
        assert_eq!(charge(&ctx, "Search").unwrap(), BudgetGrant::Granted);
    }
}

// ── The policies ─────────────────────────────────────────────────────────────

/// `on_exhausted: shed` — the call is **not made**, and the flow continues.
/// Skipping is not the same as succeeding, and the caller must record it.
#[test]
fn on_exhausted_shed_skips_the_call_and_continues() {
    let b = budget("Lax", "shed", vec![quota("max", 1, "minute", "Search")]);
    let ctx = ctx().with_budget(gate_of(&b));

    assert_eq!(charge(&ctx, "Search").unwrap(), BudgetGrant::Granted);
    assert!(
        matches!(charge(&ctx, "Search"), Ok(BudgetGrant::Shed { .. })),
        "`shed` skips the call rather than failing the flow"
    );
}

/// `on_exhausted: defer` is a **distinct** error from `block`, so a supervisor
/// reschedules instead of failing the run.
#[test]
fn on_exhausted_defer_is_a_distinct_error_from_block() {
    let b = budget("Deferred", "defer", vec![quota("max", 1, "minute", "Search")]);
    let ctx = ctx().with_budget(gate_of(&b));

    assert_eq!(charge(&ctx, "Search").unwrap(), BudgetGrant::Granted);
    assert!(matches!(
        charge(&ctx, "Search"),
        Err(DispatchError::EffectDeferred { .. })
    ));
}

/// **An unknown `on_exhausted` policy fails CLOSED.**
///
/// A policy the runtime does not understand is not a licence to proceed. The whole
/// content of a quota is that exceeding it is impossible; a gate that widened on
/// confusion would invert that.
#[test]
fn an_unknown_policy_fails_closed_it_is_not_a_licence() {
    let b = budget("Weird", "yolo", vec![quota("max", 1, "minute", "Search")]);
    let ctx = ctx().with_budget(gate_of(&b));

    assert_eq!(charge(&ctx, "Search").unwrap(), BudgetGrant::Granted);
    assert!(matches!(
        charge(&ctx, "Search"),
        Err(DispatchError::EffectQuotaExhausted { .. })
    ));
}

// ── Merging must never widen ─────────────────────────────────────────────────

/// **Two budgets over the same tool: BOTH must grant.**
///
/// Declaring an extra budget must never *increase* what a program may do. A quota
/// whose presence buys you permissions is not a quota — so the merge is all-or-none
/// over an effect's quotas, and the **strictest** `on_exhausted` wins.
#[test]
fn merging_budgets_tightens_it_never_widens() {
    let strict = budget("Strict", "block", vec![quota("max", 1, "minute", "Search")]);
    let lax = budget("Lax", "shed", vec![quota("max", 100, "minute", "Search")]);

    let now = chrono::Utc::now();
    let merged = BudgetGate::from_ir(&strict, "strict", now)
        .merged_with(BudgetGate::from_ir(&lax, "lax", now));
    let ctx = ctx().with_budget(Arc::new(Mutex::new(merged)));

    assert_eq!(charge(&ctx, "Search").unwrap(), BudgetGrant::Granted);
    assert!(
        matches!(
            charge(&ctx, "Search"),
            Err(DispatchError::EffectQuotaExhausted { .. })
        ),
        "the STRICT quota (1) must bind even though a laxer one (100) is also declared, and the \
         strict `block` policy must win over the lax `shed`. If adding a budget could soften an \
         existing one, a budget would be a way to BUY leniency."
    );
}
