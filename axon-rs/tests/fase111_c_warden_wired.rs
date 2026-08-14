//! §Fase 111.c — `warden` MADE REAL: the engine is wired to the keyword.
//!
//! # What §111 found (F12)
//!
//! `warden(<target>) within <Scope> { … }` was a **no-op wearing a completed
//! step's clothes**. `run_warden` inserted `__warden_scope` into the let-bindings
//! — a key nothing in the tree ever read — returned an empty output, **silently
//! discarded the block's body**, and emitted `StepComplete` on the wire. The
//! README advertised *"adversarial abduction over authorized evidence, emitting
//! attested `Vulnerability` findings"*.
//!
//! The engine had existed the whole time (`axon::warden`:
//! `ReferenceStaticWarden`, `Vulnerability`, `verify`), and enterprise's
//! abduction engine was mounted on the `POST /api/v1/warden/{scope}` HTTP route.
//! **Nobody had wired the math to the keyword.** `DispatchCtx` did not even have
//! a port, so not even enterprise could inject one.
//!
//! # Why the old shape was worse than a missing feature
//!
//! An analysis that finds nothing and an analysis that never ran are
//! **indistinguishable to the reader**. A security primitive whose silence
//! cannot be told apart from a clean bill of health is not a weak feature — it
//! is an anti-feature. Every refusal below exists to keep those two states
//! distinguishable.
//!
//! Pins:
//! 1. A `warden` over real evidence produces **real attested findings**.
//! 2. Every finding carries a witness that passes `warden::verify` (an
//!    un-witnessed finding is noise and must not cross the boundary).
//! 3. **The block's body RUNS** (it used to be dropped on the floor).
//! 4. No engine ⇒ `MissingDependency` — never "0 findings".
//! 5. Unresolvable `within <Scope>` ⇒ refusal (a scope that cannot be resolved
//!    authorises nothing — §88 fail-closed).
//! 6. Target not in the scope's allowlist ⇒ refusal (`TargetNotAuthorized`).
//! 7. An unapproved scope ⇒ refusal.
//! 8. A depth above the OSS reference's ceiling ⇒ refusal, not a silent downgrade.
//! 9. Evidence that cannot be read ⇒ refusal. **We never report "no findings"
//!    for a target we never opened.**

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, DispatchError, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::*;
use axon::warden::ReferenceStaticWarden;
use std::sync::Arc;
use tokio::sync::mpsc;

/// C source carrying two well-known, deterministic defects the OSS reference
/// analyzer detects: an unbounded copy and a hard-coded secret.
const VULNERABLE_SOURCE: &str = r#"
void load(char *in) {
    char buf[16];
    strcpy(buf, in);
}
const char *password = "hunter2";
"#;

fn scope(name: &str, targets: Vec<&str>, depth: &str, approver: &str) -> IRScope {
    IRScope {
        node_type: "scope",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        targets: targets.into_iter().map(String::from).collect(),
        depth: depth.into(),
        approver: approver.into(),
    }
}

fn warden_node(target: &str, scope_ref: &str, body: Vec<IRFlowNode>) -> IRFlowNode {
    IRFlowNode::Warden(IRWarden {
        node_type: "warden",
        source_line: 0,
        source_column: 0,
        target: target.into(),
        scope_ref: scope_ref.into(),
        body,
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

/// A context with the OSS reference engine + a scope catalog mounted — i.e. what
/// the runner now builds for every deployment.
fn ctx_with_warden(
    scopes: Vec<IRScope>,
) -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = DispatchCtx::new("AuditFlow", "stub", "", CancellationFlag::new(), tx)
        .with_warden(Arc::new(ReferenceStaticWarden), Arc::new(scopes));
    (ctx, rx)
}

/// The pre-§111 context: no port at all. Everything must fail CLOSED.
fn ctx_without_warden() -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        DispatchCtx::new("AuditFlow", "stub", "", CancellationFlag::new(), tx),
        rx,
    )
}

// ── §Fase 122.b — the same flagship, driven from SOURCE ─────────────────────
//
// Every gate in this file hand-builds `IRScope` and `IRWarden` in Rust. That is
// a proof of the HANDLER, and §119.f.8 is the expensive lesson about what such a
// proof does not say: `reason` was attested on `pure_shape::run_reason`, the
// citation was accurate, and the grammar that reaches it did not exist.
//
// Law 4 asks for the PATH. This test takes a `.axon` file an adopter could have
// written, compiles it through the REAL pipeline (lexer → parser → IR
// generator), lifts the `scope` catalog and the `warden` node the compiler
// actually produced, and dispatches THAT against the same `ReferenceStaticWarden`
// the runner mounts for every deployment. Nothing about the primitive changes;
// the citation stops being an engine name.

/// Compile a fixture through the real pipeline — INCLUDING the type checker.
///
/// §Fase 122.b: skipping the checker would let a fixture break a compile-time
/// law without any gate noticing. A program that does not type-check is not one
/// an adopter could deploy, so it cannot stand as a Law 4 citation.
fn compile_fixture(rel: &str) -> IRProgram {
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
async fn a_compiled_program_reaches_the_warden_and_is_analysed() {
    const FIXTURE: &str = "tests/fixtures/fase122_b_warden/authorized_audit.axon";
    let ir = compile_fixture(FIXTURE);

    // The scope catalog the COMPILER produced — not one written here.
    assert!(
        !ir.scopes.is_empty(),
        "the fixture declares `scope InternalAudit`; if this is empty the \
         declaration no longer reaches the IR"
    );
    let (mut ctx, _rx) = ctx_with_warden(ir.scopes.clone());

    // Drive the flow's nodes in order, exactly as lowered.
    let steps: Vec<IRFlowNode> = ir
        .flows
        .iter()
        .flat_map(|f| f.steps.iter().cloned())
        .collect();
    assert!(
        steps.iter().any(|n| matches!(n, IRFlowNode::Warden(_))),
        "the fixture's `warden(...) within ...` must lower to an IRWarden node"
    );

    let mut summary = None;
    for node in &steps {
        let outcome = dispatch_node(node, &mut ctx)
            .await
            .expect("every node in the compiled fixture must dispatch");
        if matches!(node, IRFlowNode::Warden(_)) {
            summary = match outcome {
                NodeOutcome::Completed { output, .. } => Some(output),
                other => panic!("expected Completed from warden, got {other:?}"),
            };
        }
    }

    let output = summary.expect("the warden node must have produced a summary");
    let v: serde_json::Value = serde_json::from_str(&output).expect("warden binds a JSON summary");

    // The same two deterministic defects, found on evidence that arrived through
    // the compiler instead of through a Rust string constant.
    let classes: Vec<&str> = v["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .map(|f| f["class"].as_str().unwrap())
        .collect();
    assert!(
        classes.contains(&"unsafe_call") && classes.contains(&"hardcoded_secret"),
        "the reference analyzer must find both defects in the compiled fixture; got {classes:?}"
    );

    // The authorization envelope came from the DECLARATION, not from a literal.
    assert_eq!(v["scope"], "InternalAudit");
    assert_eq!(v["depth"], "static_artifact");
    assert_eq!(v["approver"], "security.lead");
}

// ── 1-3. The flagship: a warden that actually analyses ──────────────────────

#[tokio::test]
async fn warden_produces_real_attested_findings_and_runs_its_body() {
    let (mut ctx, _rx) = ctx_with_warden(vec![scope(
        "InternalAudit",
        vec!["payments_core"],
        "static_artifact",
        "security.lead",
    )]);

    // Bind the artifact under the target name, then audit it.
    dispatch_node(&let_node("payments_core", VULNERABLE_SOURCE), &mut ctx)
        .await
        .expect("let must bind the evidence");

    // The body used to be DISCARDED. Pin that it runs.
    let body = vec![let_node("body_ran", "yes")];
    let outcome = dispatch_node(&warden_node("payments_core", "InternalAudit", body), &mut ctx)
        .await
        .expect("warden must analyse, not refuse: the scope authorises this target");

    let output = match outcome {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("expected Completed, got {other:?}"),
    };

    let v: serde_json::Value = serde_json::from_str(&output).expect("warden binds a JSON summary");

    // (1) Real findings — not an empty result, not LLM prose.
    let count = v["count"].as_u64().expect("count");
    assert!(
        count >= 2,
        "the reference analyzer must find the unbounded copy AND the hard-coded secret; \
         got {count} findings in {output}"
    );

    let classes: Vec<&str> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["class"].as_str().unwrap())
        .collect();
    assert!(
        classes.contains(&"unsafe_call"),
        "strcpy( must surface as unsafe_call; got {classes:?}"
    );
    assert!(
        classes.contains(&"hardcoded_secret"),
        "a hard-coded password must surface as hardcoded_secret; got {classes:?}"
    );

    // (2) The authorization envelope travels with the result — an auditor reading
    //     this must be able to see WHO approved the analysis and at what depth.
    assert_eq!(v["scope"], "InternalAudit");
    assert_eq!(v["depth"], "static_artifact");
    assert_eq!(v["approver"], "security.lead");

    // (3) The body RAN. This is the line that would have failed before §111.c.
    assert_eq!(
        ctx.let_bindings.get("body_ran").map(String::as_str),
        Some("yes"),
        "the warden block's body was silently discarded — it must execute"
    );
}

/// Every emitted finding must be witnessed. `warden::verify` is the
/// paraconsistent validator (paper §5.3): an un-witnessed finding is noise and
/// does not cross the type boundary. The handler filters through it, so a
/// backend cannot smuggle an unattested claim into a flow.
#[tokio::test]
async fn only_witnessed_findings_cross_the_boundary() {
    use axon::warden::{verify, AnalysisScope, Evidence, Vulnerability, Witness, WardenBackend};

    // The reference engine's own findings must all verify.
    let findings = ReferenceStaticWarden
        .analyze(
            &Evidence {
                target: "payments_core".into(),
                content: VULNERABLE_SOURCE.as_bytes().to_vec(),
            },
            &AnalysisScope {
                targets: vec!["payments_core".into()],
                depth: "static_artifact".into(),
                approver: "security.lead".into(),
            },
        )
        .expect("authorised analysis");
    assert!(!findings.is_empty());
    for f in &findings {
        assert!(verify(f), "the engine emitted an unwitnessed finding: {f:?}");
    }

    // And a fabricated, unwitnessed finding is rejected by the same gate.
    let noise = Vulnerability {
        class: "made_up".into(),
        target: "payments_core".into(),
        severity: "critical".into(),
        confidence: 0.99,
        witness: Witness {
            input: String::new(),
            trace: String::new(),
            contract_violated: String::new(),
        },
    };
    assert!(
        !verify(&noise),
        "an unwitnessed finding must not cross the boundary — that is the whole point of §5.3"
    );
}

// ── 4-9. Every joint fails CLOSED ───────────────────────────────────────────

/// No engine ⇒ refusal. **Never "0 findings".** This is the state the entire
/// codebase was in before §111.c, and it must now be loud.
#[tokio::test]
async fn no_engine_refuses_instead_of_reporting_zero_findings() {
    let (mut ctx, _rx) = ctx_without_warden();
    ctx.let_bindings
        .insert("payments_core".into(), VULNERABLE_SOURCE.into());

    let err = dispatch_node(&warden_node("payments_core", "InternalAudit", vec![]), &mut ctx)
        .await
        .expect_err("a warden with no engine must REFUSE, not silently complete");

    match err {
        DispatchError::MissingDependency { name } => assert_eq!(name, "warden_backend"),
        other => panic!("expected MissingDependency{{warden_backend}}, got {other:?}"),
    }
}

/// A `within <Scope>` that resolves to nothing authorises nothing.
#[tokio::test]
async fn unresolvable_scope_refuses() {
    let (mut ctx, _rx) = ctx_with_warden(vec![]); // engine present, catalog empty
    ctx.let_bindings
        .insert("payments_core".into(), VULNERABLE_SOURCE.into());

    let err = dispatch_node(&warden_node("payments_core", "GhostScope", vec![]), &mut ctx)
        .await
        .expect_err("an unresolvable scope must refuse");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("does not resolve to a declared scope"),
        "the refusal must name the missing authorization envelope; got {msg}"
    );
}

/// The allowlist is enforced at RUNTIME (the check §88.c deferred). A target
/// outside the scope is refused even though the engine is present and willing.
#[tokio::test]
async fn target_outside_the_allowlist_refuses() {
    let (mut ctx, _rx) = ctx_with_warden(vec![scope(
        "InternalAudit",
        vec!["ledger"], // payments_core is NOT authorised
        "static_artifact",
        "security.lead",
    )]);
    ctx.let_bindings
        .insert("payments_core".into(), VULNERABLE_SOURCE.into());

    let err = dispatch_node(&warden_node("payments_core", "InternalAudit", vec![]), &mut ctx)
        .await
        .expect_err("an unauthorised target must refuse");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("not in the authorization scope"),
        "got {msg}"
    );
}

/// An unapproved scope authorises nothing — a signed envelope with no signatory
/// is not an envelope.
#[tokio::test]
async fn unapproved_scope_refuses() {
    let (mut ctx, _rx) = ctx_with_warden(vec![scope(
        "InternalAudit",
        vec!["payments_core"],
        "static_artifact",
        "", // no approver
    )]);
    ctx.let_bindings
        .insert("payments_core".into(), VULNERABLE_SOURCE.into());

    let err = dispatch_node(&warden_node("payments_core", "InternalAudit", vec![]), &mut ctx)
        .await
        .expect_err("an unapproved scope must refuse");
    assert!(format!("{err:?}").contains("no approver"));
}

/// The OSS reference analyses only `static_artifact`. An invasive depth is
/// REFUSED, not silently downgraded to a weaker analysis — a downgrade would
/// hand back a clean-looking result for an audit that never happened.
#[tokio::test]
async fn depth_above_the_oss_ceiling_refuses_rather_than_downgrading() {
    let (mut ctx, _rx) = ctx_with_warden(vec![scope(
        "InternalAudit",
        vec!["payments_core"],
        "live_probe", // enterprise-only depth
        "security.lead",
    )]);
    ctx.let_bindings
        .insert("payments_core".into(), VULNERABLE_SOURCE.into());

    let err = dispatch_node(&warden_node("payments_core", "InternalAudit", vec![]), &mut ctx)
        .await
        .expect_err("a depth above the backend's ceiling must refuse");
    assert!(format!("{err:?}").contains("not supported by this backend"));
}

/// Evidence that cannot be read is a refusal, not an empty analysis. This is the
/// sharpest edge of the whole fase: analysing an unread target and reporting
/// "no findings" is the one result a security primitive must never fabricate.
#[tokio::test]
async fn unreadable_evidence_refuses_rather_than_reporting_a_clean_bill_of_health() {
    let (mut ctx, _rx) = ctx_with_warden(vec![scope(
        "InternalAudit",
        vec!["payments_core"],
        "static_artifact",
        "security.lead",
    )]);
    // NOTE: `payments_core` is deliberately NOT bound.

    let err = dispatch_node(&warden_node("payments_core", "InternalAudit", vec![]), &mut ctx)
        .await
        .expect_err("unreadable evidence must refuse");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("does not resolve to readable evidence"),
        "got {msg}"
    );
    assert!(
        msg.contains("never opened"),
        "the diagnostic must name the failure mode it is preventing — a clean-looking result \
         for a target that was never read; got {msg}"
    );
}
