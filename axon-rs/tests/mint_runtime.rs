//! v2.46.0 — runtime for `mint <Credential> as <binding>`
//! (`the design plan`, axon-enterprise repo).
//!
//! Pinned properties:
//! 1. Happy path: a mint with a configured port + a capability context
//!    covering the grants binds the raw bearer under the declared binding;
//!    the wire summary NEVER carries the token (the runtime half of
//!    `axon-T896`).
//! 2. No port configured ⇒ `MissingDependency` — fail-closed, no silent
//! stub (v2.41.0 lesson).
//! 3. Attenuation: a capability context missing a grant refuses the mint
//!    naming `authority_only_attenuates` (handler-side) — and the port
//!    itself refuses standalone (`InMemoryMinter` unit lane).
//! 4. End-to-end through the production engine: a `mint`-bearing flow with
//!    NO minter port fails the flow loudly (never an LLM-hallucinated
//!    token).

use axon::cancel_token::CancellationFlag;
use axon::credential_minter::InMemoryMinter;
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, NodeOutcome};
use axon::ir_nodes::{IRCredential, IRFlowNode, IRMintStep};
use std::sync::Arc;

fn mint_node() -> IRFlowNode {
    IRFlowNode::Mint(IRMintStep {
        node_type: "mint",
        source_line: 1,
        source_column: 1,
        credential_ref: "WidgetSession".to_string(),
        binding: "tok".to_string(),
    })
}

fn contract() -> IRCredential {
    IRCredential {
        node_type: "credential",
        source_line: 1,
        source_column: 1,
        name: "WidgetSession".to_string(),
        ttl_secs: 900,
        grants: vec!["chat.invoke".to_string()],
    }
}

fn ctx_with(
    minter: Option<Arc<InMemoryMinter>>,
    caps: Option<Vec<String>>,
) -> (
    DispatchCtx,
    tokio::sync::mpsc::UnboundedReceiver<axon::flow_execution_event::FlowExecutionEvent>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx);
    ctx.credentials = Arc::new(
        [(contract().name.clone(), contract())].into_iter().collect(),
    );
    if let Some(m) = minter {
        ctx.credential_minter = Some(m);
    }
    ctx.held_capabilities = caps;
    (ctx, rx)
}

// ── v2.89.0 — the same mint, from a program that lives on disk ──────────
//
// Every gate in this file hand-builds `IRCredential` and the mint node, which
// proves the HANDLER. Law 4 asks for the PATH: the contract and the verb as an
// adopter writes them, compiled by the real pipeline, dispatched against the
// same reference minter.

/// Compile a fixture through the real pipeline — INCLUDING the type checker.
///
/// v2.89.0: the first version of this helper stopped at the IR generator,
/// and that was a hole in the method rather than in the code under test. A
/// fixture that does not type-check is not a program an adopter could deploy,
/// so skipping the checker means the fixture silently tolerates illegal
/// programs — and a compile-time law (here `axon-T894`, `ttl ≤ 24h`) can be
/// broken in the fixture without any gate noticing. Found by mutating the
/// fixture's `ttl:` to `48h` and watching the test stay green.
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
        "the fixture must TYPE-CHECK — an adopter could not deploy it otherwise: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    axon_frontend::ir_generator::IRGenerator::new().generate(&prog)
}

#[tokio::test]
async fn a_compiled_credential_contract_mints_and_never_wires_the_token() {
    const FIXTURE: &str = "tests/fixtures/credential/visitor_session.axon";
    let ir = compile_fixture(FIXTURE);

    // The contract catalog the COMPILER produced from `credential WidgetSession`.
    assert!(
        !ir.credentials.is_empty(),
        "the fixture declares a `credential`; an empty catalog means the \
         declaration no longer reaches the IR"
    );

    let minter = Arc::new(InMemoryMinter::new());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx);
    ctx.credentials = Arc::new(
        ir.credentials
            .iter()
            .map(|c| (c.name.clone(), c.clone()))
            .collect(),
    );
    ctx.credential_minter = Some(minter.clone());
    ctx.held_capabilities = Some(vec!["chat.invoke".to_string(), "flow.execute".to_string()]);

    let steps: Vec<axon::ir_nodes::IRFlowNode> = ir
        .flows
        .iter()
        .flat_map(|f| f.steps.iter().cloned())
        .collect();
    let mut minted = None;
    for node in &steps {
        let outcome = dispatch_node(node, &mut ctx)
            .await
            .expect("every node in the compiled fixture must dispatch");
        if let NodeOutcome::Completed { output, .. } = outcome {
            minted = Some(output);
        }
    }

    // The binding name came from the SOURCE (`as tok`), and the grants from the
    // declared contract.
    let token = ctx
        .let_bindings
        .get("tok")
        .expect("the fixture binds the bearer under `tok`")
        .clone();
    let rec = minter.verify(&token).expect("token verifies");
    assert_eq!(
        rec.grants,
        vec!["chat.invoke"],
        "the grants must come from the DECLARED contract, not a default"
    );

    // And the secret never leaves: not in the outcome, not on the wire.
    let output = minted.expect("the mint node must complete");
    assert!(
        !output.contains(&token),
        "the raw bearer must not ride the outcome: {output}"
    );
    drop(ctx);
    while let Ok(ev) = rx.try_recv() {
        let json = serde_json::to_string(&ev).unwrap();
        assert!(!json.contains(&token), "token must not ride the wire: {json}");
    }
}

#[tokio::test]
async fn mint_binds_the_bearer_and_never_wires_the_token() {
    let minter = Arc::new(InMemoryMinter::new());
    let (mut ctx, mut rx) = ctx_with(
        Some(minter.clone()),
        Some(vec!["chat.invoke".to_string(), "flow.execute".to_string()]),
    );
    let outcome = dispatch_node(&mint_node(), &mut ctx).await.expect("mint admits");

    let token = ctx.let_bindings.get("tok").expect("binding present").clone();
    assert!(token.starts_with("axep_"), "reference token shape: {token}");
    // The minted token verifies against the reference minter.
    let rec = minter.verify(&token).expect("token verifies");
    assert_eq!(rec.grants, vec!["chat.invoke"]);

    // The wire summary + the outcome NEVER carry the raw bearer.
    let NodeOutcome::Completed { output, .. } = outcome else {
        panic!("expected Completed");
    };
    assert!(output.contains("credential 'WidgetSession' minted"), "{output}");
    assert!(!output.contains(&token), "token must not ride the outcome");
    drop(ctx); // close tx
    while let Ok(ev) = rx.try_recv() {
        let json = serde_json::to_string(&ev).unwrap();
        assert!(!json.contains(&token), "token must not ride the wire: {json}");
    }
}

#[tokio::test]
async fn missing_minter_port_fails_closed() {
    let (mut ctx, _rx) = ctx_with(None, Some(vec!["chat.invoke".to_string()]));
    let err = dispatch_node(&mint_node(), &mut ctx)
        .await
        .expect_err("no port ⇒ fail closed");
    let msg = format!("{err:?}");
    assert!(msg.contains("credential_minter"), "names the dependency: {msg}");
    assert!(ctx.let_bindings.get("tok").is_none(), "no binding on refusal");
}

#[tokio::test]
async fn attenuation_violation_is_refused_at_the_handler() {
    let minter = Arc::new(InMemoryMinter::new());
    // The bearer holds a DIFFERENT capability — grants ⊄ caps.
    let (mut ctx, _rx) = ctx_with(Some(minter), Some(vec!["flow.execute".to_string()]));
    let err = dispatch_node(&mint_node(), &mut ctx)
        .await
        .expect_err("amplification refused");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("authority_only_attenuates"),
        "names the law: {msg}"
    );
    assert!(msg.contains("chat.invoke"), "names the missing grant: {msg}");
}

#[test]
fn e2e_mint_without_a_port_fails_the_flow_loudly() {
    // Through the PRODUCTION engine (execute_server_flow → dispatcher via
    // BufferSink): no minter is configured on the OSS server path, so the
    // flow fails closed — never an LLM-fabricated bearer.
    let src = "credential WidgetSession { ttl: 15m grants: [chat.invoke] }\n\
               flow Bootstrap() -> Unit {\n\
                   mint WidgetSession as tok\n\
                   step S { ask: \"hi\" }\n\
               }\n";
    let (_prog, ir) = axon::flow_plan::compile_source_to_ir(src, "<v2.46.0>").expect("compile");
    let empty = std::collections::HashMap::new();
    let m = axon::runner::execute_server_flow(
        &ir, "Bootstrap", "stub", "", "<v2.46.0>", None, None, &empty, &empty, None, None, None,
        None, // budget
        None, // v2.69.0 channel semaphores
        None, // v2.69.0 — tool leases (test: none)
        None, // event_outbox
        None, // v2.46.0 credential minter
        None, // v2.48.0 — secret custody (test: none)
        None, // v2.63.0 dataspace_engine (tests: fail closed)
        None, // v2.56.0 scrape_overrides
)
    .expect("execute returns metrics");
    assert!(!m.success, "mint without a port must fail the flow");
    let err = m.error.expect("honest failure detail");
    assert!(err.contains("credential_minter"), "names the dependency: {err}");
}
