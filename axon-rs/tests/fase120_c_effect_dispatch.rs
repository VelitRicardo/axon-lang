//! §Fase 120 — **the algebraic-effect machine RUNS, from `.axon` SOURCE.**
//!
//! This file exists to satisfy law 4 of [`axon_frontend::advertised`]: *"the
//! proof must cite the PATH, not the engine."* §Fase 23 built a real
//! Plotkin/Pretnar FSM with 49 green tests, and every one of them builds an
//! `Instruction` block **by hand**. That is exactly how the subsystem stayed
//! invisible for fifteen fases: the engine was proven, and no program could
//! reach it. `EffectRuntime::new()` had two call sites and both were inside
//! `mod tests`.
//!
//! So every assertion below starts at SOURCE, walks lexer → parser → IR
//! generator → `dispatch_node`, and observes the wire or the live bindings.
//!
//! # What is being pinned, and why each one
//!
//! | § | claim | why it is not obvious |
//! |---|---|---|
//! | 1 | the `in { … }` body EXECUTES | the rejected design (`Instruction`) would have run it as a no-op |
//! | 2 | a clause body EXECUTES | a handler that parses and does nothing is the §111 shape |
//! | 3 | the clause sees the VALUE, not the name | an unresolved reference resolves to ITSELF — the failure prints plausibly |
//! | 4 | `resume()` returns control to the perform site | without it the continuation is silently dropped and the rest of the body never runs |
//! | 5 | a clause with no `resume` ABORTS | `fase_23` §3.1 publishes exactly this (`Done() -> { … }`) and the §23 engine raises `NoDischarge` instead |
//! | 6 | `abort` reaches the frame that OWNS it | the §23 engine propagates it to the top of the run, against its own doc comment |
//! | 7 | `forward` reaches the next OUTER frame | D12's whole content |
//! | 8 | an unhandled `perform` FAILS CLOSED | D9 has no runtime fallback; a quiet completion would deploy |
//! | 9 | a clause binding does not leak | a handler's locals must not appear in the continuation it resumes |
//! | 10 | a step-body `perform` runs AFTER generation | the §119.n lesson: an elevation would perform an output that does not exist yet |
//!
//! The observable is deliberately `let` — a pure node needing no backend — so
//! the assertions are about the EFFECT MACHINE and not about a model.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, DispatchError, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::IRFlow;
use tokio::sync::mpsc;

/// SOURCE → AST → IR → the named flow. The whole point of this file.
fn flow_from_source(src: &str, name: &str) -> IRFlow {
    let tokens = axon_frontend::lexer::Lexer::new(src, "gate.axon")
        .tokenize()
        .expect("lex");
    let prog = axon_frontend::parser::Parser::new(tokens)
        .parse()
        .expect("the published surface must parse");
    let ir = axon_frontend::ir_generator::IRGenerator::new().generate(&prog);
    ir.flows
        .into_iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no flow named {name} in the generated IR"))
}

fn ctx() -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx),
        rx,
    )
}

/// Dispatch every node of a flow in order, returning the final ctx.
async fn run(src: &str, name: &str) -> (DispatchCtx, Vec<String>) {
    let flow = flow_from_source(src, name);
    let (mut c, mut rx) = ctx();
    for node in &flow.steps {
        dispatch_node(node, &mut c)
            .await
            .unwrap_or_else(|e| panic!("dispatch failed: {e:?}"));
    }
    let mut slugs = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let FlowExecutionEvent::StepStart { step_type, .. } = ev {
            slugs.push(step_type);
        }
    }
    (c, slugs)
}

/// Dispatch and return the error, for the fail-closed assertions.
async fn run_expecting_error(src: &str, name: &str) -> DispatchError {
    let flow = flow_from_source(src, name);
    let (mut c, _rx) = ctx();
    for node in &flow.steps {
        match dispatch_node(node, &mut c).await {
            Ok(_) => {}
            Err(e) => return e,
        }
    }
    panic!("expected the dispatch to FAIL CLOSED, but every node completed");
}

// ── §1 — the `in { … }` body executes ───────────────────────────────────────

/// The assertion that separates this fase from the design it replaced. Lowering
/// the handler body onto `axon::effects::Instruction` would have deserialised
/// this `let` to `Instruction::Passthrough` — inert — and the flow would have
/// completed clean with `marker` unbound.
#[tokio::test]
async fn c1_the_handle_body_actually_runs() {
    let (c, slugs) = run(
        r#"
effect SSE { Emit(t: Text) -> Unit }
flow F() -> Text {
    handle SSE { Emit(t) -> { resume() } } in {
        let marker = "the body ran"
    }
}
"#,
        "F",
    )
    .await;
    assert_eq!(
        c.let_bindings.get("marker").map(String::as_str),
        Some("the body ran"),
        "the `in {{ … }}` block must EXECUTE. If this is unbound the handler \
         parsed, lowered, dispatched — and ran nothing, which is the whole \
         defect class §120 exists to not rebuild"
    );
    assert!(slugs.contains(&"handle".to_string()), "the wire must name the frame: {slugs:?}");
}

// ── §2/§3 — the clause runs, and sees the VALUE ─────────────────────────────

/// A `perform` must reach the clause AND the clause's body must execute. And
/// the argument must arrive RESOLVED: an unresolved reference resolves to
/// itself in this runtime, so a broken binding would put the string
/// `"payload"` where the value belongs — output that looks plausible and is
/// wrong, which is the failure mode nobody catches in review.
#[tokio::test]
async fn c2_perform_reaches_the_clause_and_the_clause_sees_the_value() {
    let (c, slugs) = run(
        r#"
effect SSE { Emit(t: Text) -> Unit }
flow F() -> Text {
    let payload = "forty-two"
    handle SSE {
        Emit(token) -> {
            let seen = token
            resume()
        }
    } in {
        perform Emit(payload)
    }
}
"#,
        "F",
    )
    .await;
    assert_eq!(
        c.let_bindings.get("seen").map(String::as_str),
        Some("forty-two"),
        "the clause body must RUN, and its parameter must be bound to the \
         RESOLVED argument — not to the name `payload`"
    );
    assert!(slugs.contains(&"perform".to_string()), "the wire must name the perform: {slugs:?}");
}

// ── §4 — resume returns control to the perform site ─────────────────────────

/// `resume()` hands control back to the `perform`, and the node AFTER it runs.
/// Without this the continuation is silently dropped: the flow would complete,
/// `after` would be unbound, and nothing would say why.
#[tokio::test]
async fn c4_resume_returns_control_to_the_perform_site() {
    let (c, _) = run(
        r#"
effect SSE { Emit(t: Text) -> Unit }
flow F() -> Text {
    handle SSE {
        Emit(token) -> {
            let during = "handler"
            resume()
        }
    } in {
        perform Emit("x")
        let after = "continuation resumed"
    }
}
"#,
        "F",
    )
    .await;
    assert_eq!(c.let_bindings.get("during").map(String::as_str), Some("handler"));
    assert_eq!(
        c.let_bindings.get("after").map(String::as_str),
        Some("continuation resumed"),
        "the statement AFTER the perform must run — that is what `resume` \
         means. Its absence would be a dropped continuation with no diagnostic"
    );
}

/// `resume(v)` carries a value, and it becomes the perform's output.
#[tokio::test]
async fn c4b_resume_carries_its_value_to_the_perform_site() {
    let flow = flow_from_source(
        r#"
effect Ask { Get() -> Text }
flow F() -> Text {
    let answer = "from the handler"
    handle Ask { Get() -> { resume(answer) } } in {
        perform Get()
    }
}
"#,
        "F",
    );
    let (mut c, _rx) = ctx();
    // The `let` first, so `answer` is bound when the clause resumes with it.
    dispatch_node(&flow.steps[0], &mut c).await.expect("let");
    let outcome = dispatch_node(&flow.steps[1], &mut c).await.expect("handle");
    match outcome {
        NodeOutcome::Completed { output, .. } => assert_eq!(
            output, "from the handler",
            "`resume(v)` must deliver `v` to the perform site"
        ),
        other => panic!("expected a completion, got {other:?}"),
    }
}

// ── §5 — a clause with no resume is an implicit ABORT ───────────────────────

/// `fase_23` §3.1 publishes `Done() -> { websocket.close() }` with the comment
/// *"sin resume — abort implícito"*, and §3.6 states *"0 resumes: aborto
/// explícito, OK."* The §23 engine raises `NoDischarge` for exactly this shape.
/// Under §119's doctrine the published surface wins.
#[tokio::test]
async fn c5_a_clause_without_resume_aborts_the_handle() {
    let (c, _) = run(
        r#"
effect SSE { Done() -> Never }
flow F() -> Text {
    handle SSE {
        Done() -> { let closed = "handler ran" }
    } in {
        perform Done()
        let unreachable = "must not run"
    }
}
"#,
        "F",
    )
    .await;
    assert_eq!(
        c.let_bindings.get("closed").map(String::as_str),
        Some("handler ran"),
        "the clause itself must run"
    );
    assert!(
        !c.let_bindings.contains_key("unreachable"),
        "a clause that never resumes DROPS the continuation — the rest of the \
         handle body must not run. Running it would resume a continuation the \
         handler declined to invoke"
    );
}

// ── §6 — abort reaches the frame that OWNS it ──────────────────────────────

#[tokio::test]
async fn c6_abort_yields_the_handle_with_its_value() {
    let flow = flow_from_source(
        r#"
effect SSE { Emit(t: Text) -> Unit }
flow F() -> Text {
    handle SSE {
        Emit(token) -> { abort("aborted value") }
    } in {
        perform Emit("x")
        let unreachable = "must not run"
    }
}
"#,
        "F",
    );
    let (mut c, _rx) = ctx();
    let outcome = dispatch_node(&flow.steps[0], &mut c).await.expect("handle");
    match outcome {
        NodeOutcome::Completed { output, .. } => {
            assert_eq!(output, "aborted value", "the handle yields the aborted value")
        }
        other => panic!("the handle must COMPLETE with the abort's value, got {other:?}"),
    }
    assert!(!c.let_bindings.contains_key("unreachable"));
}

/// **Nested frames.** An `abort` raised by the OUTER handler's clause must
/// terminate the OUTER handle, not the inner one it happens to pass through.
/// `axon::effects::EffectRuntime` propagates an abort straight to the top of
/// the run — contradicting its own doc comment — and nobody could ever have
/// seen it, because nothing reached the engine.
#[tokio::test]
async fn c6b_an_abort_terminates_the_frame_that_owns_it_not_the_innermost() {
    let flow = flow_from_source(
        r#"
effect Outer { Bail(v: Text) -> Unit }
effect Inner { Tick() -> Unit }
flow F() -> Text {
    handle Outer {
        Bail(v) -> { abort("outer aborted") }
    } in {
        handle Inner {
            Tick() -> { resume() }
        } in {
            perform Bail("go")
            let inner_tail = "must not run"
        }
        let outer_tail = "must not run either"
    }
}
"#,
        "F",
    );
    let (mut c, _rx) = ctx();
    let outcome = dispatch_node(&flow.steps[0], &mut c).await.expect("handle");
    match outcome {
        NodeOutcome::Completed { output, .. } => assert_eq!(
            output, "outer aborted",
            "the OUTER handle owns the abort and yields its value"
        ),
        other => panic!("expected the outer handle to complete, got {other:?}"),
    }
    assert!(
        !c.let_bindings.contains_key("inner_tail"),
        "the inner handle's remaining body must not run"
    );
    assert!(
        !c.let_bindings.contains_key("outer_tail"),
        "and neither must the OUTER handle's — the abort terminated it. If \
         this binding exists, the inner frame swallowed the outer frame's exit"
    );
}

// ── §7 — forward reaches the next OUTER frame ──────────────────────────────

/// D12's whole content: a handler that logs and delegates. The inner clause
/// must run AND the outer clause must run, in that order, and the resume must
/// come from the outer one.
#[tokio::test]
async fn c7_forward_delegates_to_the_outer_handler() {
    let (c, _) = run(
        r#"
effect SSE { Emit(t: Text) -> Unit }
flow F() -> Text {
    handle SSE {
        Emit(token) -> {
            let outer_saw = token
            resume()
        }
    } in {
        handle SSE {
            Emit(token) -> {
                let inner_saw = token
                forward Emit(token)
            }
        } in {
            perform Emit("payload")
            let after = "resumed through the outer handler"
        }
    }
}
"#,
        "F",
    )
    .await;
    assert_eq!(
        c.let_bindings.get("inner_saw").map(String::as_str),
        Some("payload"),
        "the INNER clause intercepts first"
    );
    assert_eq!(
        c.let_bindings.get("outer_saw").map(String::as_str),
        Some("payload"),
        "…and `forward` delegates to the OUTER one, with the arguments intact"
    );
    assert_eq!(
        c.let_bindings.get("after").map(String::as_str),
        Some("resumed through the outer handler"),
        "the outer clause's `resume` must return control to the ORIGINAL \
         perform site — a forward is a delegation, not a new perform"
    );
}

// ── §8 — an unhandled perform FAILS CLOSED ─────────────────────────────────

/// D9 has no runtime fallback. The type-checker refuses this program
/// (`axon-T966`); the runtime refusal is the belt-and-braces for a hand-built
/// IR, and it must be a REFUSAL. A quiet completion is how an effect nobody
/// handled ships.
#[tokio::test]
async fn c8_an_unhandled_perform_fails_closed() {
    let err = run_expecting_error(
        r#"
effect SSE { Emit(t: Text) -> Unit }
flow F() -> Text {
    perform Emit("x")
}
"#,
        "F",
    )
    .await;
    match err {
        DispatchError::BackendError { name, message } => {
            assert_eq!(name, "algebraic_effects");
            assert!(
                message.contains("unhandled effect") && message.contains("SSE"),
                "the diagnostic must name what had no handler: {message}"
            );
        }
        other => panic!("expected a named refusal, got {other:?}"),
    }
}

/// A `perform` whose effect the innermost frame does not name must NOT be
/// caught by that frame. Searching by operation name alone would deliver the
/// effect to whichever handler is nearest — the silent mis-delivery D120.2's
/// ambiguity error exists to prevent, arriving through the back door.
#[tokio::test]
async fn c8b_a_frame_for_another_effect_does_not_catch_it() {
    let err = run_expecting_error(
        r#"
effect A { Op(v: Text) -> Unit }
effect B { Other(v: Text) -> Unit }
flow F() -> Text {
    handle A { Op(v) -> { resume() } } in {
        perform B.Other("x")
    }
}
"#,
        "F",
    )
    .await;
    match err {
        DispatchError::BackendError { message, .. } => assert!(
            message.contains("unhandled effect") && message.contains('B'),
            "a frame over A must not intercept B: {message}"
        ),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

// ── §9 — a clause binding does not leak into the continuation ──────────────

/// The clause parameter is scoped to the clause. If it survived, the perform
/// site's continuation would silently see the handler's locals — and a later
/// step interpolating `${token}` would render a value from a scope it never
/// entered.
#[tokio::test]
async fn c9_a_clause_parameter_does_not_leak_into_the_continuation() {
    let (c, _) = run(
        r#"
effect SSE { Emit(t: Text) -> Unit }
flow F() -> Text {
    let token = "the outer binding"
    handle SSE {
        Emit(token) -> { resume() }
    } in {
        perform Emit("the performed value")
    }
}
"#,
        "F",
    )
    .await;
    assert_eq!(
        c.let_bindings.get("token").map(String::as_str),
        Some("the outer binding"),
        "the clause SHADOWS `token` for its own body and RESTORES it after. If \
         this reads `the performed value`, the handler leaked its parameter \
         into the surrounding scope"
    );
}

/// …and a parameter that shadowed NOTHING is removed, not left bound to the
/// last value the handler saw.
#[tokio::test]
async fn c9b_an_unshadowed_clause_parameter_is_removed_after() {
    let (c, _) = run(
        r#"
effect SSE { Emit(t: Text) -> Unit }
flow F() -> Text {
    handle SSE { Emit(token) -> { resume() } } in { perform Emit("v") }
}
"#,
        "F",
    )
    .await;
    assert!(
        !c.let_bindings.contains_key("token"),
        "a clause binder that shadowed nothing must be REMOVED on exit, not \
         left behind for the next step to interpolate"
    );
}

// ── §10 — a step-body perform runs AFTER generation ────────────────────────

/// The §119.n lesson applied a second time, and the one assertion that would
/// fail if `perform` had been filed under `pix_ops` like every other step-body
/// statement: the argument is the step's OWN output, so an elevation would hand
/// the handler the unresolved name.
///
/// The assertion compares the handler's view against the step's OWN binding
/// rather than against a hardcoded string, so it holds whatever the backend
/// produced. That comparison is only satisfiable if the perform ran after
/// generation: an elevation would have resolved `Gen.output` before `Gen`
/// existed, and an unresolved reference resolves to ITSELF.
#[tokio::test]
async fn c10_a_step_body_perform_runs_after_generation_over_the_steps_output() {
    let flow = flow_from_source(
        r#"
effect SSE { Emit(t: Text) -> Unit }
flow F() -> Text {
    handle SSE {
        Emit(token) -> {
            let handler_saw = token
            resume()
        }
    } in {
        step Gen {
            given: seed
            perform Emit(Gen.output)
            output: Text
        }
    }
}
"#,
        "F",
    );
    let (mut c, _rx) = ctx();
    dispatch_node(&flow.steps[0], &mut c)
        .await
        .expect("the handle must dispatch");

    let generated = c
        .let_bindings
        .get("Gen")
        .cloned()
        .expect("the step must bind its output under its own name");
    let handler_saw = c
        .let_bindings
        .get("handler_saw")
        .cloned()
        .expect("the handler must have run");

    assert_ne!(
        handler_saw, "Gen.output",
        "the handler received the NAME. That is what a `perform` filed under \
         `pix_ops` would produce: the elevation runs BEFORE generation, \
         `Gen.output` resolves to nothing, and an unresolved reference \
         resolves to itself — so the wire carries an identifier where the \
         adopter expected the step's output, with no error anywhere"
    );
    assert_eq!(
        handler_saw, generated,
        "the handler must see exactly what the step GENERATED — which is only \
         possible if the perform ran after generation, against the live bindings"
    );
}

// ── §11 — nothing changed for programs without effects ─────────────────────

/// The frame stack is empty for every pre-§120 program, and an effect-free flow
/// dispatches exactly as before.
#[tokio::test]
async fn c11_an_effect_free_flow_carries_no_frames() {
    let (c, slugs) = run(
        "flow F() -> Text { let x = \"plain\" }\n",
        "F",
    )
    .await;
    assert_eq!(c.let_bindings.get("x").map(String::as_str), Some("plain"));
    assert!(
        c.effect_frames.is_empty(),
        "no effects declared ⇒ no frames ever pushed"
    );
    assert!(
        !slugs.iter().any(|s| s == "handle" || s == "perform"),
        "and no effect events on the wire: {slugs:?}"
    );
}

/// The frame stack is balanced: a `handle` that completes leaves nothing
/// behind, so a later `perform` in the same flow cannot be caught by a frame
/// whose scope already closed (D3 — a handler scope is DELIMITED).
#[tokio::test]
async fn c11b_the_frame_stack_is_balanced_so_a_closed_scope_catches_nothing() {
    let err = run_expecting_error(
        r#"
effect SSE { Emit(t: Text) -> Unit }
flow F() -> Text {
    handle SSE { Emit(t) -> { resume() } } in {
        perform Emit("inside")
    }
    perform Emit("outside")
}
"#,
        "F",
    )
    .await;
    match err {
        DispatchError::BackendError { message, .. } => assert!(
            message.contains("unhandled effect"),
            "a `perform` AFTER the handle's scope closed must not find the \
             frame — that is what delimited means: {message}"
        ),
        other => panic!("expected a refusal, got {other:?}"),
    }
}
