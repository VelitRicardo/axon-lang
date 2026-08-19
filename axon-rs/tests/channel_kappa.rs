//! v4.0.0 — **κ-coverage at the channel egress seam, from source.**
//!
//! # What this is the proof of
//!
//! The `compliance` ledger row's gap named this seam precisely: the typed
//! bus HAD a compliance predicate (`ShieldComplianceFn`, v1.6.0-era), and the
//! only production callers — `daemon.rs` and `runner.rs`, both via
//! `from_ir_program` — handed it `|_, _| true`. A shield covering NOTHING
//! satisfied a channel carrying PHI, at the one operation (D8 `publish`)
//! that extrudes a capability to parties outside the flow.
//!
//! v4.0.0 closes both doors of the rule, and this gate exercises both FROM
//! THE SAME SOURCE FILES on disk — v2.83.0: a gate is proof of a PATH only
//! if its input is `.axon` source:
//!
//! - **check time** — `axon-T1215` (dual of T957): the uncovered program is
//!   refused at declaration, naming exactly the missing classes.
//! - **publish time** — `TypedEventBus::from_ir_program` now derives its
//!   predicate from the same IR it registers channels from, so IR that
//!   never met the checker still cannot extrude an uncovered capability.
//!
//! The two fixtures differ ONLY in the shield's `compliance:` list — the
//! covered one is the uncovered one with the missing class added, which is
//! precisely the fix T1215's message prescribes.

use axon::runtime::channels::{TypedChannelError, TypedEventBus};

const COVERED: &str = "tests/fixtures/channel_kappa/phi_relay.axon";
const UNCOVERED: &str = "tests/fixtures/channel_kappa/phi_relay_uncovered.axon";
const GHOST: &str = "tests/fixtures/channel_kappa/phi_relay_ghost_shield.axon";

fn read_fixture(rel: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn parse(src: &str, name: &str) -> axon_frontend::ast::Program {
    let tokens = axon_frontend::lexer::Lexer::new(src, name)
        .tokenize()
        .expect("fixture must lex");
    axon_frontend::parser::Parser::new(tokens)
        .parse()
        .expect("fixture must parse")
}

/// Lower WITHOUT the type checker — the population the runtime door exists
/// for: hand-assembled or pre-v4.0.0 IR that never met `axon-T1215`.
fn ir_unchecked(src: &str, name: &str) -> axon::ir_nodes::IRProgram {
    axon_frontend::ir_generator::IRGenerator::new().generate(&parse(src, name))
}

/// 🎯 The covered program type-checks, and `publish` through the production
/// constructor extrudes a capability — the rule refuses, it does not smother.
#[tokio::test]
async fn a_covering_shield_type_checks_and_publishes() {
    let src = read_fixture(COVERED);
    let prog = parse(&src, COVERED);
    let errors = axon_frontend::type_checker::TypeChecker::new(&prog).check();
    assert!(
        errors.is_empty(),
        "the covered fixture must TYPE-CHECK: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );

    let ir = axon_frontend::ir_generator::IRGenerator::new().generate(&prog);
    let bus = TypedEventBus::from_ir_program(&ir);
    let cap = bus
        .publish("PhiFeed", "PhiGate")
        .await
        .expect("a shield whose kappa covers the payload's kappa must publish");
    assert_eq!(cap.channel_name, "PhiFeed");
}

/// 🎯 Door 1 — the checker refuses the uncovered DECLARATION, citing
/// `axon-T1215` and naming exactly the class the shield is missing.
#[test]
fn the_uncovered_declaration_is_refused_at_check_time() {
    let src = read_fixture(UNCOVERED);
    let prog = parse(&src, UNCOVERED);
    let errors = axon_frontend::type_checker::TypeChecker::new(&prog).check();
    assert!(
        errors.iter().any(|e| {
            e.message.contains("axon-T1215")
                && e.message.contains("PCI_DSS")
                && e.message.contains("WeakGate")
        }),
        "axon-T1215 must name the channel's uncovered class (PCI_DSS) and the \
         offending shield — got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// 🎯 The fail-closed branch: a channel whose `shield:` names something the
/// IR cannot resolve, carrying regulated κ. The checker refuses this as an
/// undefined reference — but IR that never met the checker reaches `publish`
/// with it, and an unresolvable control covers NOTHING. This test exists
/// because the first mutation round flipped `None => false` to `None => true`
/// in the predicate and every other test stayed green.
#[tokio::test]
async fn a_shield_the_ir_cannot_resolve_covers_nothing() {
    let src = read_fixture(GHOST);
    let ir = ir_unchecked(&src, GHOST);
    let bus = TypedEventBus::from_ir_program(&ir);

    let err = bus
        .publish("PhiFeed", "GhostGate")
        .await
        .expect_err("GhostGate is declared by no shield — it must fail closed");
    match &err {
        TypedChannelError::CapabilityGate(msg) => assert!(
            msg.contains("does not cover compliance"),
            "the refusal must come from the compliance predicate — got: {msg}"
        ),
        other => panic!("expected CapabilityGate, got {other:?}"),
    }
}

/// 🎯 Door 2 — the SAME source, lowered past the checker, still cannot
/// extrude an uncovered capability: `from_ir_program` derives the predicate
/// from the IR it registers channels from. Before v4.0.0 this publish
/// SUCCEEDED — the production constructor injected `|_, _| true`.
#[tokio::test]
async fn the_uncovered_shield_is_refused_at_publish_time() {
    let src = read_fixture(UNCOVERED);
    let ir = ir_unchecked(&src, UNCOVERED);
    let bus = TypedEventBus::from_ir_program(&ir);

    let err = bus
        .publish("PhiFeed", "WeakGate")
        .await
        .expect_err("WeakGate covers [HIPAA] but the payload carries [HIPAA, PCI_DSS]");
    match &err {
        TypedChannelError::CapabilityGate(msg) => assert!(
            msg.contains("does not cover compliance"),
            "the refusal must be the compliance gate, not the shield-identity \
             gate — got: {msg}"
        ),
        other => panic!("expected CapabilityGate, got {other:?}"),
    }
}
