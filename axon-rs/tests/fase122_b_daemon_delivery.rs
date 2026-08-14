//! §Fase 122.b (group B) — `daemon`, from a program that lives on disk.
//!
//! # Why this file exists
//!
//! `daemon` was attested on `daemon.rs — OTP-style supervision + §74 durable
//! event delivery`: a module name. That is an ENGINE citation, and §119.f.8 is
//! the expensive lesson about what an engine citation does not say. The
//! delivery proof itself is real and stays — it lives in `src/daemon.rs`'s unit
//! tests — but it builds its source inline, so no file on disk holds the
//! program and nothing can catch the citation going stale.
//!
//! # What is proven here
//!
//! §74.a's headline, driven from a fixture: a producer emits on a typed
//! channel, the bus carries it, and the consumer daemon's `listen` body RUNS
//! with the event payload bound to the consumer flow's parameters. §52.g's
//! `axon-W009` said that delivery did not exist.
//!
//! The bus is built FROM the compiled IR, so the channel is registered because
//! the program declared it — not because the test registered it.

use axon::daemon::deliver_typed_event;
use axon::runtime::channels::{TypedEventBus, TypedPayload};

const FIXTURE: &str = "tests/fixtures/fase122_b_daemon/intent_learner.axon";

/// Compile a fixture through the real pipeline — INCLUDING the type checker.
/// A fixture that does not type-check is not a program an adopter could deploy,
/// so it cannot stand as a Law 4 citation.
fn compile_fixture(rel: &str) -> axon::ir_nodes::IRProgram {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let tokens = axon_frontend::lexer::Lexer::new(&src, rel)
        .tokenize()
        .expect("fixture must lex");
    let prog = axon_frontend::parser::Parser::new(tokens)
        .parse()
        .expect("fixture must parse");
    let errors = axon_frontend::type_checker::TypeChecker::new(&prog).check();
    assert!(
        errors.is_empty(),
        "the fixture must TYPE-CHECK: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    axon_frontend::ir_generator::IRGenerator::new().generate(&prog)
}

#[tokio::test]
async fn a_compiled_daemon_receives_the_event_its_listener_declares() {
    let ir = compile_fixture(FIXTURE);
    assert!(
        !ir.daemons.is_empty(),
        "the fixture declares a `daemon`; an empty catalog means the declaration \
         no longer reaches the IR"
    );

    // The bus is built from the COMPILED program, so `HibCh` is registered
    // because the fixture declares it.
    let bus = TypedEventBus::from_ir_program(&ir);
    let payload = serde_json::json!({ "tenant_id": "acme", "session_id": "s1" });
    bus.emit("HibCh", TypedPayload::Scalar(payload.clone()))
        .await
        .expect("emit to the channel the fixture declares");

    let event = bus
        .receive("HibCh")
        .await
        .expect("the emitted event is queued");
    let received = match event.payload {
        TypedPayload::Scalar(v) => v,
        other => panic!("expected scalar payload, got {other:?}"),
    };
    assert_eq!(received["tenant_id"], "acme");

    let results = deliver_typed_event(&ir, "HibCh", &received, "stub", FIXTURE, None);
    assert_eq!(
        results.len(),
        1,
        "exactly one listener must fire — the one the fixture DECLARES"
    );
    let (daemon, flow, _res) = &results[0];
    assert_eq!(
        daemon, "IntentLearner",
        "the daemon that fires must be the one declared in the fixture"
    );
    assert_eq!(
        flow, "Learn",
        "and it must run the flow its `listen` body declares"
    );
}
