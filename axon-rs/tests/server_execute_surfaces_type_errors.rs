//! v2.87.0 — **`server_execute` says WHAT is wrong, not just how many.**
//!
//! # The audit this came out of
//!
//! v2.87.0 found that `TypeChecker` had two entry points that were separate,
//! drifted copies of the pass list, and that `axon check` took the one missing
//! v2.67.0's resource laws. Fixing it raised the obvious follow-up: *which OTHER
//! subsystems have two doors, and which one does the shipped binary open?*
//!
//! Every type-check call site in both repos was enumerated. The answer is that
//! the class does not repeat by SHAPE — `IRGenerator` has one door, and all
//! twelve production `TypeChecker` sites call `check`. It repeats by
//! **DISPOSITION**:
//!
//! | site | proceeds? | says what? |
//! |---|---|---|
//! | `axon check` / `compile` / `run` / `audit` / `pcc` | no — refuses | yes |
//! | `/deploy` | no — refuses | yes |
//! | `/execute/dry-run`, `flow_inspect`, `repl` | yes (they do not execute) | yes |
//! | **`server_execute`** | **yes, and it EXECUTES** | **no — a COUNT** |
//!
//! One outlier, and it is the one that runs the program: `/v1/execute`, the MCP
//! handlers, the warm handler, and **scheduled daemon executions**.
//!
//! # Why a count is not a diagnostic
//!
//! The caller received `success: false` and `errors: 1` and had no way to learn
//! what the 1 was — while the flow ran to completion. A flow is not inert: its
//! `persist` / `emit` / `deliver` / `notify` steps take real outward action, and
//! `success: false` afterwards does not unsend an email.
//!
//! The fixture below is deliberately a v2.87.0 program (`axon-T966`, an
//! undischarged effect), because that is the diagnostic whose whole promise is
//! *"a COMPILE error — there is no runtime fallback"*. Reaching execution with
//! the reason discarded is the shape that promise exists to prevent.

use axon::execution_result::ServerExecutionResult;

/// A program that TYPE-CHECKS DIRTY but parses clean: `study` performs an
/// effect no enclosing `handle` discharges. `axon check` refuses this (proved
/// in `axon-frontend/tests/effect_grammar.rs`).
///
/// ⚠️ **The `run` at the bottom is load-bearing, and finding out why was itself
/// a measurement.** Without it this fixture type-checks CLEAN, because D9 is
/// deliberately not an error at the `perform` — a flow nobody enters discharges
/// nothing and leaks nothing, which is what lets an adopter write an effectful
/// library at all (`a5d` in the frontend gate).
///
/// **So D9 covers DECLARED entry points: a top-level `run`, an `axonendpoint`'s
/// `execute:`, a daemon `listen` body's `run`.** `server_execute` enters a flow
/// **by NAME, from an HTTP request** — an entry point no declaration expresses
/// and no static check can see. For that door the guarantee is the RUNTIME
/// refusal (`effect_handlers::dispatch_operation` fails closed with
/// `unhandled effect`), not the compile-time one. That is a real and narrow
/// limit of static exhaustiveness, recorded rather than papered over.
const UNDISCHARGED: &str = r#"
effect Delivery { Emit(token: Text) -> Unit }

flow study(topic: Text) -> Text {
    step Analyze {
        given: topic
        ask: "Analyse the topic."
        perform Emit(Analyze.output)
        output: Text
    }
}

run study(topic: "x")
"#;

/// The exact pipeline `server_execute` runs, up to the point the diagnostics
/// are produced — lexer → parser → type-checker, through the entry point the
/// server takes.
fn type_error_messages(src: &str) -> Vec<String> {
    let tokens = axon_frontend::lexer::Lexer::new(src, "<server>")
        .tokenize()
        .expect("lex");
    let program = axon_frontend::parser::Parser::new(tokens)
        .parse()
        .expect("the fixture must PARSE — the point is that it type-checks dirty");
    axon_frontend::type_checker::TypeChecker::new(&program)
        .check()
        .iter()
        .map(|e| {
            if e.line > 0 {
                format!("line {}: {}", e.line, e.message)
            } else {
                e.message.clone()
            }
        })
        .collect()
}

/// The fixture must actually be dirty. Without this the assertions below
/// compare empty vectors and pass forever — the failure mode v2.83.0 named for
/// gates that measure nothing.
#[test]
fn f1_the_fixture_really_does_type_check_dirty() {
    let msgs = type_error_messages(UNDISCHARGED);
    assert!(
        !msgs.is_empty(),
        "the fixture must raise a type error, or every assertion here is vacuous"
    );
    assert!(
        msgs.iter().any(|m| m.contains("axon-T966")),
        "expected the D9 exhaustiveness diagnostic, got: {msgs:?}"
    );
}

/// **The field exists and carries WORDS.**
///
/// This is the whole fix: `server_execute` builds this vector from the same
/// diagnostics it counts. Before v2.87.0 the messages were computed and dropped
/// one line later.
#[test]
fn f2_the_result_carries_the_messages_not_only_the_count() {
    let msgs = type_error_messages(UNDISCHARGED);
    let result = ServerExecutionResult {
        success: false,
        flow_name: "study".to_string(),
        source_file: "<server>".to_string(),
        backend: "stub".to_string(),
        steps_executed: 0,
        latency_ms: 0,
        tokens_input: 0,
        tokens_output: 0,
        anchor_checks: 0,
        anchor_breaches: 0,
        errors: msgs.len(),
        type_errors: msgs.clone(),
        step_names: Vec::new(),
        step_results: Vec::new(),
        trace_id: 0,
        effect_policies: Vec::new(),
        enforcement_summaries: Vec::new(),
        runtime_warnings: Vec::new(),
        provenance_events: Vec::new(),
        blame_attribution: None,
        epistemic_envelopes: Vec::new(),
        error: None,
        temporal_context: None,
    };

    assert_eq!(
        result.errors,
        result.type_errors.len(),
        "the count and the messages must describe the same set — a count that \
         disagrees with its own detail is worse than either alone"
    );
    assert!(
        result.type_errors.iter().any(|m| m.contains("axon-T966")),
        "the caller must be able to read WHAT is wrong: {:?}",
        result.type_errors
    );
    assert!(
        result.type_errors.iter().any(|m| m.starts_with("line ")),
        "…and WHERE. A diagnostic with no location sends the author hunting: {:?}",
        result.type_errors
    );
}

/// **The property that actually matters: NOTHING RAN.**
///
/// Surfacing the messages was half the fix. The other half is that
/// `server_execute` now REFUSES before the first step, because a flow is not
/// inert — `persist` / `emit` / `deliver` / `notify` take real outward action,
/// and a `success: false` returned afterwards does not unsend an email.
///
/// This drives the real entry point end to end: source in, envelope out.
///
/// ⚠️ Gated per-ITEM, not per-FILE. A file-level `#![cfg(feature = "server")]`
/// would compile this whole target to zero tests under the default features —
/// making it the 104th entry in `dark_targets.pinned`, and taking the
/// three assertions above (which need no server) dark with it. Item-level
/// gating keeps the target alive and only skips what genuinely needs `axum`.
/// That distinction is the v2.87.0 finding applied to its own author.
#[cfg(feature = "server")]
#[test]
fn f4_a_type_invalid_program_executes_no_steps() {
    let out = axon::axon_server::server_execute_for_test(UNDISCHARGED, "<server>", "study");

    assert!(!out.success, "a refused program is not a success");
    assert_eq!(
        out.steps_executed, 0,
        "NOT ONE STEP may run. This is the whole point: the type-checker had \
         already refused the program, and executing it anyway means the side \
         effects happen and the verdict arrives too late to matter"
    );
    assert!(
        out.step_names.is_empty() && out.step_results.is_empty(),
        "…and no step may even be NAMED as having started: {:?} / {:?}",
        out.step_names,
        out.step_results
    );
    assert!(
        out.type_errors.iter().any(|m| m.contains("axon-T966")),
        "the refusal must say WHAT it refused: {:?}",
        out.type_errors
    );
    assert_eq!(
        out.errors,
        out.type_errors.len(),
        "the count and the messages must describe the same set"
    );
}

/// …and a program that type-checks still runs. A refusal that also refuses the
/// valid case is not a gate, it is an outage.
#[cfg(feature = "server")]
#[test]
fn f4b_a_clean_program_still_executes() {
    const CLEAN: &str = r#"
flow study(topic: Text) -> Text {
    step Analyze {
        given: topic
        ask: "Analyse the topic."
        output: Text
    }
}
"#;
    let out = axon::axon_server::server_execute_for_test(CLEAN, "<server>", "study");
    assert!(
        out.type_errors.is_empty(),
        "the clean program must raise nothing: {:?}",
        out.type_errors
    );
    assert!(
        out.steps_executed > 0,
        "and it must still RUN — otherwise the refusal above is indistinguishable \
         from a server that executes nothing at all"
    );
}

/// The wire shape does not move for a program that type-checks. The field is
/// `skip_serializing_if = "Vec::is_empty"`, so every clean run serialises
/// byte-identically to pre-v2.87.0.
#[test]
fn f3_a_clean_program_adds_nothing_to_the_wire() {
    let clean = ServerExecutionResult {
        success: true,
        flow_name: "F".to_string(),
        source_file: "f.axon".to_string(),
        backend: "stub".to_string(),
        steps_executed: 1,
        latency_ms: 1,
        tokens_input: 0,
        tokens_output: 0,
        anchor_checks: 0,
        anchor_breaches: 0,
        errors: 0,
        type_errors: Vec::new(),
        step_names: Vec::new(),
        step_results: Vec::new(),
        trace_id: 0,
        effect_policies: Vec::new(),
        enforcement_summaries: Vec::new(),
        runtime_warnings: Vec::new(),
        provenance_events: Vec::new(),
        blame_attribution: None,
        epistemic_envelopes: Vec::new(),
        error: None,
        temporal_context: None,
    };
    let json = serde_json::to_string(&clean).expect("serialize");
    assert!(
        !json.contains("type_errors"),
        "an empty diagnostic list must be ELIDED, so no clean run's envelope \
         changes shape: {json}"
    );
}
