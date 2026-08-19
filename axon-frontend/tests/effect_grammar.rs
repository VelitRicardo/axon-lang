//! v2.87.0 — **the four algebraic-effect constructs reach the IR, from
//! SOURCE**, and the static laws that make `perform` safe REFUSE.
//!
//! # What was wrong, measured 2026-08-09/10 before any of this landed
//!
//! `axon-rs/src/effects/` is a real 590-line Plotkin/Pretnar FSM with a handler
//! stack, one-shot delimited continuations (D2) and 49 green tests — and the
//! language could not reach a line of it:
//!
//! | checked | result |
//! |---|---|
//! | lexer tokens for `effect`/`handle`/`perform`/`resume` | **ZERO** |
//! | AST nodes for them | **none** |
//! | `ir_generator` emitting effect IR | **never** |
//! | `EffectRuntime::new()` outside `#[cfg(test)]` | **none** — both call sites in `mod tests` |
//! | `IRProgram::effects` entries ever written | **none** — a Python-parity mirror that outlived Python |
//!
//! This is not v2.67.0's *motor real, cable muerto* (a published grammar reaching
//! the wrong engine). There was **no grammar at all**, and the runtime was only
//! ever constructed by its own tests.
//!
//! # What this file pins
//!
//! Every assertion starts at `.axon` SOURCE. A hand-built AST would prove the
//! same nothing the old effects tests proved — they build `Instruction` blocks
//! by hand, which is exactly how the subsystem stayed invisible.
//!
//!   - `effect E { … }` lowers into `IRProgram::effects` — **the field that
//! existed since v1.17.0 and had never held an entry**;
//!   - `handle E { … } in { … }` lowers with clause bodies that are ORDINARY
//! flow nodes (the design decision — the property that makes a handler able to `emit`);
//!   - `perform` in a step body lands on `IRStep::performs` and NOT on
//! `pix_ops` (the v2.83.0 lesson: an elevation runs BEFORE generation);
//! - **the design decision** — the bare form `perform Emit(x)` RESOLVES, and a colliding
//!     operation is REFUSED naming both candidates rather than picked;
//!   - **D9** — an undischarged effect is a COMPILE error, INTERPROCEDURALLY,
//!     at every entry point;
//!   - **D10** — two `resume`s on one clause path are refused;
//! - backward compatibility: a pre-v2.87.0 program's IR JSON is byte-identical.

use axon_frontend::ast::{Declaration, FlowStep, Program};
use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::IRFlowNode;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> Program {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("the published surface must parse")
}

fn parse_err(src: &str) -> String {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    match Parser::new(tokens).parse() {
        Ok(_) => panic!("expected a PARSE error, got a program"),
        Err(e) => e.message,
    }
}

fn ir(src: &str) -> axon_frontend::ir_nodes::IRProgram {
    IRGenerator::new().generate(&parse(src))
}

/// Every type error the program raises, joined — so an assertion can name the
/// code it expects without depending on error ordering.
///
/// ⚠️ Deliberately goes through **`check_with_warnings`**, which is the entry
/// point `axon check` takes (`crate::checker::run_check`). Calling `check`
/// would exercise the ENGINE; this file's whole discipline is to exercise the
/// PATH. That distinction was not academic — see
/// [`two_type_check_entry_points_never_drift`].
fn errors(src: &str) -> String {
    let prog = parse(src);
    TypeChecker::new(&prog)
        .check_with_warnings()
        .0
        .into_iter()
        .map(|e| format!("{} (line {})", e.message, e.line))
        .collect::<Vec<_>>()
        .join("\n---\n")
}

/// **The drift gate on the type-checker's own two doors.**
///
/// `TypeChecker` exposes `check` and `check_with_warnings`, and until v2.87.0 they
/// were SEPARATE COPIES of the pass list that had already diverged: `check` ran
/// v2.67.0's `check_resource_module_laws` (`axon-T945` / `axon-T947`) and
/// `check_with_warnings` did not — and `check_with_warnings` is the one
/// `axon check` calls. So the v2.67.0 resource laws had never once run for an
/// adopter at the command line, while every test that proved them called
/// `check`.
///
/// That is v2.83.0's law 4 inside the compiler: a proof that names a FUNCTION
/// proves the engine, not the path. It was found by running the BUILT BINARY
/// against a program this file refuses — `axon check` reported `0 errors`.
///
/// `check` is now a projection of `check_with_warnings`. This test fails if
/// anyone re-forks them.
#[test]
fn two_type_check_entry_points_never_drift() {
    // A program that trips laws from BOTH lists at once: v2.87.0's D9 (was only
    // ever in one copy when it landed) and v2.67.0's resource discipline (the law
    // that was actually missing from the adopter's door).
    let src = "resource Pool { kind: database endpoint: \"env:DB\" lifetime: linear }\n\
               effect SSE { Emit(t: Token) -> Unit }\n\
               flow leaky(p: Text) -> Text { perform Emit(p) }\n\
               run leaky(p: \"x\")\n";
    let prog = parse(src);
    let via_check: Vec<String> = TypeChecker::new(&prog)
        .check()
        .into_iter()
        .map(|e| format!("{}:{}:{}", e.line, e.column, e.message))
        .collect();
    let prog2 = parse(src);
    let via_warnings: Vec<String> = TypeChecker::new(&prog2)
        .check_with_warnings()
        .0
        .into_iter()
        .map(|e| format!("{}:{}:{}", e.line, e.column, e.message))
        .collect();

    assert_eq!(
        via_check, via_warnings,
        "`check` and `check_with_warnings` disagree. They are two doors into \
         ONE type-checker and `axon check` takes the second one. When they were \
         separate pass lists they drifted silently for entire cycles — a law \
         every test proved, that no adopter's command ever ran."
    );
    assert!(
        !via_check.is_empty(),
        "the fixture must actually trip a law, or this gate compares two empty \
         lists and passes forever"
    );
    assert!(
        via_check.iter().any(|e| e.contains("axon-T966")),
        "v2.87.0's D9 must fire through BOTH doors: {via_check:?}"
    );
    assert!(
        via_check.iter().any(|e| e.contains("axon-T945")),
        "v2.67.0's resource law must fire through BOTH doors — this is the one \
         that was missing from the adopter's: {via_check:?}"
    );
}

/// The canonical program of `the design plan` section 3.1, rendered in grammar AXON actually
/// has (`step` is not a top-level declaration and `run` names a FLOW). The
/// shape is what matters and it is preserved exactly: the performing flow knows
/// NOTHING about how the token leaves, and the handler is somewhere else.
const CANONICAL: &str = r#"
effect SSE {
    Emit(token: Token) -> Unit
    Done() -> Never
}

flow generate(prompt: Text) -> Text {
    step Gen {
        ask: "Generate a response"
        perform Emit(Gen.output)
        perform Done()
        output: Text
    }
}

flow stream_chat(user_input: Text) -> Text {
    handle SSE {
        Emit(token) -> {
            step Send { ask: "deliver the token" output: Text }
            resume()
        }
        Done() -> {
            step Close { ask: "close the channel" output: Text }
        }
    } in {
        run generate(prompt: user_input)
    }
}
"#;

// ── section 1 — the declaration reaches the IR field that never held one ───────────

#[test]
fn a1_effect_declaration_lands_in_the_ir_effects_field() {
    let ir = ir(CANONICAL);
    assert_eq!(
        ir.effects.len(),
        1,
        "`effect SSE {{ … }}` must lower into IRProgram::effects — the field \
         v1.17.0 created as a Python-parity mirror and that NOTHING had ever \
         written to, which is why EffectRuntime was constructible only from \
         its own tests"
    );
    let e = &ir.effects[0];
    assert_eq!(e.name, "SSE");
    assert_eq!(e.node_type, "effect_declaration");
    assert_eq!(e.operations.len(), 2, "both operations must survive");

    let emit = &e.operations[0];
    assert_eq!(emit.name, "Emit");
    assert_eq!(emit.parameter_names, vec!["token".to_string()]);
    assert_eq!(emit.parameter_types, vec!["Token".to_string()]);
    assert_eq!(emit.return_type, "Unit");

    let done = &e.operations[1];
    assert_eq!(done.name, "Done");
    assert!(done.parameter_names.is_empty(), "`Done()` takes no parameters");
    assert_eq!(
        done.return_type, "Never",
        "`Never` is the bottom sentinel the design plan writes — an operation \
         whose handler is not expected to resume. Losing it would erase the \
         one piece of information distinguishing Done from Emit"
    );
}

/// A declared effect with no operations is refused. A frame over it could never
/// intercept anything, so it would read as governance and be dead code.
#[test]
fn a1b_an_empty_effect_is_refused() {
    let errs = errors("effect Hollow { }\n");
    assert!(
        errs.contains("axon-T960"),
        "an effect with no operations must be refused, got: {errs}"
    );
}

// ── section 2 — `handle … in …` lowers, and its bodies are REAL nodes ──────────────

/// **the design decision.** The clause body must lower to ordinary `IRFlowNode`s. This is
/// the assertion that separates this cycle from the plan's original step 2:
/// lowering the body onto `axon-rs`'s `Instruction` alphabet would have made
/// every non-effect node a `Passthrough` — inert — so `run generate(…)` inside
/// a handler would have executed NOTHING while compiling clean.
#[test]
fn a2_handle_lowers_with_real_flow_nodes_in_both_bodies() {
    let ir = ir(CANONICAL);
    let flow = ir
        .flows
        .iter()
        .find(|f| f.name == "stream_chat")
        .expect("stream_chat");
    let IRFlowNode::Handle(h) = &flow.steps[0] else {
        panic!("`handle … in …` must lower to IRFlowNode::Handle, got {:?}", flow.steps[0]);
    };

    assert_eq!(h.effect_names, vec!["SSE".to_string()]);
    assert_eq!(h.clauses.len(), 2);

    let emit = &h.clauses[0];
    assert_eq!(emit.operation_name, "Emit");
    assert_eq!(emit.parameter_names, vec!["token".to_string()]);
    assert!(
        emit.body.iter().any(|n| matches!(n, IRFlowNode::Step(_))),
        "a clause body must lower to REAL flow nodes — a step in a handler has \
         to actually run, or the handler is decoration"
    );
    assert!(
        emit.body.iter().any(|n| matches!(n, IRFlowNode::Resume(_))),
        "`resume()` must lower inside the clause"
    );

    assert!(
        h.body.iter().any(|n| matches!(n, IRFlowNode::Run(_))),
        "the `in {{ … }}` body must lower to real nodes — `run generate(…)` is \
         the whole composition the paradigm exists for"
    );
}

/// A `handle` with no `in { … }` is a scope with no extent. Refused at the
/// parser, because accepting it would let an author believe an effect was
/// handled when nothing is inside anything.
#[test]
fn a2b_handle_without_in_is_refused() {
    let msg = parse_err(
        "effect E { Op() -> Unit }\nflow F() -> Text { handle E { Op() -> { resume() } } }\n",
    );
    assert!(
        msg.contains("in {"),
        "the diagnostic must name the missing `in {{ … }}`, got: {msg}"
    );
}

// ── section 3 — `perform` in a step body is NOT an elevation ───────────────────────

/// The v2.83.0 lesson, applied a second time. A `pix_ops` entry runs BEFORE the
/// step generates; `perform Emit(Gen.output)` performs the step's OWN output,
/// so running it early would hand the handler an unresolved symbol and put a
/// NAME on the wire where the adopter expected a token — a defect that produces
/// garbage output, never an error.
#[test]
fn a3_step_body_perform_lands_on_performs_not_pix_ops() {
    let ir = ir(CANONICAL);
    let flow = ir.flows.iter().find(|f| f.name == "generate").unwrap();
    let IRFlowNode::Step(step) = &flow.steps[0] else {
        panic!("expected a step");
    };
    assert_eq!(
        step.performs.len(),
        2,
        "both `perform`s in the step body must reach IRStep::performs"
    );
    assert_eq!(step.performs[0].operation_name, "Emit");
    assert_eq!(step.performs[0].arguments, vec!["Gen.output".to_string()]);
    assert_eq!(step.performs[1].operation_name, "Done");
    assert!(
        step.pix_ops.is_empty(),
        "a `perform` is NOT an elevation — filing it under pix_ops would run it \
         BEFORE generation, against a binding that does not exist yet"
    );
    assert_eq!(
        step.ask, "Generate a response",
        "the step's own cognition must survive alongside the performs"
    );
}

// ── section 4 — the design decision, the resolution rule ────────────────────────────────────────

/// The BARE form is what `the design plan` section 3.1 publishes. It must resolve.
#[test]
fn a4_bare_perform_resolves_against_the_closed_catalog() {
    let ir = ir(CANONICAL);
    let flow = ir.flows.iter().find(|f| f.name == "generate").unwrap();
    let IRFlowNode::Step(step) = &flow.steps[0] else { panic!() };
    assert_eq!(
        step.performs[0].effect_name, "SSE",
        "a bare `perform Emit(x)` must reach the IR with its effect RESOLVED — \
         an empty effect_name fails closed at dispatch"
    );
    assert!(
        step.performs[0].resolved_from_bare,
        "the IR records that the author wrote the bare spelling"
    );
}

/// The qualified form works too, and pins the effect explicitly.
#[test]
fn a4b_qualified_perform_is_taken_at_its_word() {
    let ir = ir("effect SSE { Emit(t: Token) -> Unit }\n\
                 flow F(x: Text) -> Text {\n\
                     handle SSE { Emit(t) -> { resume() } } in {\n\
                         perform SSE.Emit(x)\n\
                     }\n\
                 }\n");
    let IRFlowNode::Handle(h) = &ir.flows[0].steps[0] else { panic!() };
    let IRFlowNode::Perform(p) = &h.body[0] else {
        panic!("expected a perform in the handle body, got {:?}", h.body[0])
    };
    assert_eq!(p.effect_name, "SSE");
    assert!(!p.resolved_from_bare);
}

/// **The case the whole rule exists for.** Two effects declaring `Emit`: the
/// compiler must REFUSE and name both, never pick. Picking is a bug that
/// produces output — a token routed to the logger instead of the wire — and
/// those are the ones nobody finds.
#[test]
fn a4c_a_colliding_bare_operation_is_refused_naming_every_candidate() {
    let errs = errors(
        "effect SSE { Emit(t: Token) -> Unit }\n\
         effect Log { Emit(m: Text) -> Unit }\n\
         flow F(x: Text) -> Text {\n\
             handle SSE { Emit(t) -> { resume() } } in { perform Emit(x) }\n\
         }\n",
    );
    assert!(errs.contains("axon-T964"), "expected the ambiguity code, got: {errs}");
    assert!(errs.contains("SSE"), "the diagnostic must name SSE: {errs}");
    assert!(errs.contains("Log"), "the diagnostic must name Log: {errs}");
}

/// …and qualifying is the escape hatch the diagnostic points at, so it must
/// actually work.
#[test]
fn a4d_qualifying_clears_the_ambiguity() {
    let errs = errors(
        "effect SSE { Emit(t: Token) -> Unit }\n\
         effect Log { Emit(m: Text) -> Unit }\n\
         flow F(x: Text) -> Text {\n\
             handle SSE { Emit(t) -> { resume() } } in { perform SSE.Emit(x) }\n\
         }\n",
    );
    assert!(!errs.contains("axon-T964"), "qualified must not be ambiguous: {errs}");
    assert!(!errs.contains("axon-T966"), "and SSE is handled here: {errs}");
}

#[test]
fn a4e_an_undeclared_operation_is_refused() {
    let errs = errors(
        "effect SSE { Emit(t: Token) -> Unit }\n\
         flow F(x: Text) -> Text { handle SSE { Emit(t) -> { resume() } } in { perform Nope(x) } }\n",
    );
    assert!(errs.contains("axon-T965"), "expected the undeclared code, got: {errs}");
}

/// A qualified site naming an operation the effect does not own must say THAT,
/// not "ambiguous" — the catalog takes a qualifier at its word, so the
/// membership check has to happen at the site or the node lowers with an effect
/// name and an operation that effect lacks.
#[test]
fn a4f_a_qualified_site_still_checks_membership() {
    let errs = errors(
        "effect SSE { Emit(t: Token) -> Unit }\n\
         flow F(x: Text) -> Text { handle SSE { Emit(t) -> { resume() } } in { perform SSE.Emitt(x) } }\n",
    );
    assert!(errs.contains("axon-T965"), "expected a membership error, got: {errs}");
    assert!(errs.contains("Emitt"), "and it must quote what was written: {errs}");
}

/// Arity. A mismatch leaves a binder unbound at dispatch, and an unbound name
/// resolves to ITSELF — so the handler would put the name on the wire with no
/// error anywhere.
#[test]
fn a4g_arity_mismatch_is_refused_on_both_ends() {
    let at_site = errors(
        "effect SSE { Emit(t: Token) -> Unit }\n\
         flow F(x: Text) -> Text { handle SSE { Emit(t) -> { resume() } } in { perform Emit(x, x) } }\n",
    );
    assert!(at_site.contains("axon-T963"), "perform-site arity: {at_site}");

    let at_clause = errors(
        "effect SSE { Emit(t: Token) -> Unit }\n\
         flow F(x: Text) -> Text { handle SSE { Emit() -> { resume() } } in { perform Emit(x) } }\n",
    );
    assert!(at_clause.contains("axon-T963"), "clause-binder arity: {at_clause}");
}

/// A clause for an operation none of the frame's effects declares is
/// unreachable — no `perform` can route to it, so the handler silently does
/// less than it reads as doing.
#[test]
fn a4h_an_unreachable_clause_is_refused() {
    let errs = errors(
        "effect SSE { Emit(t: Token) -> Unit }\n\
         flow F(x: Text) -> Text { handle SSE { Ghost(t) -> { resume() } Emit(t) -> { resume() } } in { perform Emit(x) } }\n",
    );
    assert!(errs.contains("axon-T962"), "expected the dead-clause code, got: {errs}");
}

#[test]
fn a4i_handling_an_undeclared_effect_is_refused() {
    let errs = errors("flow F() -> Text { handle Ghost { Op() -> { resume() } } in { } }\n");
    assert!(errs.contains("axon-T961"), "expected the undeclared-effect code, got: {errs}");
}

// ── section 5 — D9, and it is INTERPROCEDURAL ──────────────────────────────────────

/// The canonical program must COMPILE CLEAN. `generate` performs an effect it
/// knows nothing about; `stream_chat` handles it around the `run`. If D9 were
/// intra-flow it would refuse this — and refusing it would make the composition
/// the whole paradigm rests on illegal.
#[test]
fn a5_the_canonical_composition_compiles_clean() {
    let errs = errors(CANONICAL);
    assert!(
        errs.is_empty(),
        "the design plan's own program must compile — an outer handler intercepting \
         an inner computation's effects IS the feature. Got:\n{errs}"
    );
}

/// …and the SAME inner flow, run WITHOUT the handler, must be refused. This is
/// the other half: an intra-flow check would pass both, a naive one would
/// refuse both. Only the row propagation tells them apart.
#[test]
fn a5b_the_same_flow_run_bare_is_refused_at_the_run_site() {
    let errs = errors(
        "effect SSE { Emit(t: Token) -> Unit }\n\
         flow generate(p: Text) -> Text { step Gen { ask: \"g\" perform Emit(Gen.output) output: Text } }\n\
         flow bare(p: Text) -> Text { run generate(prompt: p) }\n\
         run bare(p: \"hi\")\n",
    );
    assert!(
        errs.contains("axon-T966"),
        "an undischarged effect reaching a top-level run is a COMPILE error \
         (the design plan D9 — there is no runtime fallback). Got:\n{errs}"
    );
    assert!(errs.contains("SSE"), "the diagnostic must name the effect: {errs}");
}

/// **The entry point an intra-flow check misses most expensively.** A deployed
/// HTTP surface whose flow leaks an effect would reach production green.
#[test]
fn a5c_an_axonendpoint_is_an_entry_point_for_d9() {
    let errs = errors(
        "effect SSE { Emit(t: Token) -> Unit }\n\
         flow leaky(p: Text) -> Text { step G { ask: \"g\" perform Emit(G.output) output: Text } }\n\
         axonendpoint Api { method: POST path: \"/x\" execute: leaky output: Text }\n",
    );
    assert!(
        errs.contains("axon-T966"),
        "an axonendpoint executing a leaking flow must be refused — it is a \
         DEPLOYED entry outside every handler scope. Got:\n{errs}"
    );
}

/// A flow that performs but is never entered raises nothing. That is not
/// leniency — it is the compositional property: the caller may handle it, and
/// refusing here would forbid writing an effectful library at all.
#[test]
fn a5d_an_unrun_effectful_flow_is_not_an_error_by_itself() {
    let errs = errors(
        "effect SSE { Emit(t: Token) -> Unit }\n\
         flow generate(p: Text) -> Text { step G { ask: \"g\" perform Emit(G.output) output: Text } }\n",
    );
    assert!(
        !errs.contains("axon-T966"),
        "a flow nobody runs discharges nothing and leaks nothing: {errs}"
    );
}

/// D9 must see through the BARE spelling. Reading `effect_name` straight off
/// the AST returns `None` for it (resolution happens downstream), so a check
/// that skipped unresolved sites would fire only for the qualified form — green
/// exactly where the published surface lives.
#[test]
fn a5e_d9_fires_for_the_bare_spelling_too() {
    let bare = errors(
        "effect SSE { Emit(t: Token) -> Unit }\n\
         flow leaky(p: Text) -> Text { perform Emit(p) }\n\
         run leaky(p: \"x\")\n",
    );
    let qualified = errors(
        "effect SSE { Emit(t: Token) -> Unit }\n\
         flow leaky(p: Text) -> Text { perform SSE.Emit(p) }\n\
         run leaky(p: \"x\")\n",
    );
    assert!(bare.contains("axon-T966"), "bare spelling must be caught: {bare}");
    assert!(qualified.contains("axon-T966"), "qualified too: {qualified}");
}

/// A clause that performs the operation it handles must NOT be discharged by
/// its own frame — that would be a compiler-blessed infinite loop.
#[test]
fn a5f_a_clause_does_not_discharge_itself() {
    let errs = errors(
        "effect SSE { Emit(t: Token) -> Unit }\n\
         flow F(x: Text) -> Text { handle SSE { Emit(t) -> { perform Emit(t) resume() } } in { perform Emit(x) } }\n\
         run F(x: \"hi\")\n",
    );
    assert!(
        errs.contains("axon-T966"),
        "a clause performing its own operation escapes to an OUTER frame; with \
         none, it leaks. Discharging it against itself would be an infinite \
         loop the compiler blessed. Got:\n{errs}"
    );
}

/// `forward` reaches the next OUTER frame. Two nested handlers over the same
/// effect is the decorator shape D12 exists for, and it must compile.
#[test]
fn a5g_forward_is_discharged_by_the_outer_frame() {
    let errs = errors(
        "effect SSE { Emit(t: Token) -> Unit }\n\
         flow F(x: Text) -> Text {\n\
             handle SSE { Emit(t) -> { resume() } } in {\n\
                 handle SSE { Emit(t) -> { forward Emit(t) } } in { perform Emit(x) }\n\
             }\n\
         }\n\
         run F(x: \"hi\")\n",
    );
    assert!(
        errs.is_empty(),
        "an inner clause forwarding to an outer frame is the decorator shape \
         D12 specifies. Got:\n{errs}"
    );
}

/// …and with no outer frame, the same `forward` leaks.
#[test]
fn a5h_forward_with_no_outer_frame_leaks() {
    let errs = errors(
        "effect SSE { Emit(t: Token) -> Unit }\n\
         flow F(x: Text) -> Text {\n\
             handle SSE { Emit(t) -> { forward Emit(t) } } in { perform Emit(x) }\n\
         }\n\
         run F(x: \"hi\")\n",
    );
    assert!(
        errs.contains("axon-T966"),
        "a forward with nothing outside it has nowhere to go: {errs}"
    );
}

// ── section 6 — the clause-scope law and D10 ───────────────────────────────────────

#[test]
fn a6_resume_outside_a_clause_is_refused() {
    let errs = errors("flow F() -> Text { resume() }\n");
    assert!(
        errs.contains("axon-T967"),
        "outside a clause there is no captured continuation to resume: {errs}"
    );
}

#[test]
fn a6b_abort_and_forward_outside_a_clause_are_refused() {
    let a = errors("flow F() -> Text { abort() }\n");
    assert!(a.contains("axon-T967"), "abort: {a}");
    let f = errors("effect E { Op() -> Unit }\nflow F() -> Text { forward Op() }\n");
    assert!(f.contains("axon-T967"), "forward: {f}");
}

/// **D2/D10.** The runtime is allowed to ASSUME one-shot (it does not clone the
/// captured continuation), so the compiler owes the guarantee.
#[test]
fn a6c_two_resumes_on_one_clause_path_are_refused() {
    let errs = errors(
        "effect SSE { Emit(t: Token) -> Unit }\n\
         flow F(x: Text) -> Text { handle SSE { Emit(t) -> { resume() resume() } } in { perform Emit(x) } }\n",
    );
    assert!(
        errs.contains("axon-T968"),
        "a second resume has nothing to resume — the continuation is consumed: {errs}"
    );
}

// ── section 7 — backward compatibility, measured ───────────────────────────────────

/// Six new hard keywords could break existing source. Every `.axon` in this
/// repository must still lex and parse — measured, not asserted.
#[test]
fn a7_every_axon_source_in_the_repo_still_parses() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf();
    let mut checked = 0usize;
    let mut broken: Vec<String> = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                // `target/` holds vendored copies of PUBLISHED crates; parsing
                // those would measure old releases, not this tree.
                if name != "target" && name != ".git" && name != "node_modules" {
                    stack.push(path);
                }
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) != Some("axon") {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else { continue };
            checked += 1;
            let tokens = match Lexer::new(&src, "x.axon").tokenize() {
                Ok(t) => t,
                Err(e) => {
                    broken.push(format!("{} — lex: {:?}", path.display(), e));
                    continue;
                }
            };
            if let Err(e) = Parser::new(tokens).parse() {
                broken.push(format!("{} — parse: {}", path.display(), e.message));
            }
        }
    }
    assert!(checked > 50, "expected to find the .axon corpus, found {checked} files");
    assert!(
        broken.is_empty(),
        "adding `effect`/`handle`/`perform`/`resume`/`abort`/`forward` as hard \
         keywords broke {} of {checked} existing .axon sources:\n{}",
        broken.len(),
        broken.join("\n")
    );
}

/// A program with no effects must serialise byte-identically to how it did
/// before v2.87.0 — every new IR field is `skip_serializing_if`, so no pre-v2.87.0
/// artifact's IR-SHA moves.
#[test]
fn a7b_a_program_without_effects_has_no_effect_fields_in_its_ir_json() {
    let json = serde_json::to_string(&ir(
        "flow F(x: Text) -> Text { step S { ask: \"hi\" output: Text } }\n",
    ))
    .expect("json");
    assert!(
        !json.contains("\"performs\""),
        "an effect-free step must not emit `performs` — the field is elided \
         when empty so no pre-v2.87.0 IR-SHA moves"
    );
    assert!(
        !json.contains("\"handle\""),
        "…and no handler node appears: {json}"
    );
    assert_eq!(
        json.matches("\"effects\":[]").count(),
        1,
        "IRProgram::effects keeps its pre-v2.87.0 serialization (an empty array, \
         not elided) — it was never `skip_serializing_if` and changing that \
         WOULD move every artifact's SHA"
    );
}

/// `effects: <io, network>` on a tool is a DIFFERENT concept that happens to
/// share a word. Adding the `effect` keyword must not disturb it.
#[test]
fn a7c_the_tool_effect_row_still_parses() {
    let ir = ir("tool T { timeout: 5s effects: <io, network> }\n\
                 effect E { Op() -> Unit }\n");
    assert_eq!(ir.tools.len(), 1, "the tool must survive");
    assert_eq!(
        ir.effects.len(),
        1,
        "…and the algebraic effect declaration is a separate list"
    );
}

// ── section 8 — the AST position, pinned ───────────────────────────────────────────

/// `handle` nests: a handler inside a handler's `in` body must parse, or the
/// decorator shape of section 5g could not be written.
#[test]
fn a8_handlers_nest() {
    let prog = parse(
        "effect A { Op() -> Unit }\n\
         effect B { Other() -> Unit }\n\
         flow F() -> Text {\n\
             handle A { Op() -> { resume() } } in {\n\
                 handle B { Other() -> { abort() } } in { perform Other() }\n\
             }\n\
         }\n",
    );
    let Declaration::Flow(f) = &prog.declarations[2] else { panic!("expected the flow") };
    let FlowStep::Handle(outer) = &f.body[0] else { panic!("expected the outer handle") };
    assert!(
        matches!(outer.body[0], FlowStep::Handle(_)),
        "a `handle` must be legal inside another handler's `in` body"
    );
}
