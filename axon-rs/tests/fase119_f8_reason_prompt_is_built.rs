//! §Fase 119.f.8 (runtime half) — the `reason` block's fields become the PROMPT.
//!
//! The grammar half lives in `axon-frontend/tests/fase119_f8_reason_block.rs`.
//! This is the other end of the same cable: the fields now reach the IR, and
//! `pure_shape::run_reason` must actually put them in front of the model.
//!
//! Asserted against the PURE assembly (`reason_prompt`) rather than the wire,
//! deliberately: the stub backend answers `"(stub)"` whatever it is asked, so
//! a prompt bug is invisible from the wire — and a prompt that was never built
//! is precisely the defect §119.f.8 repairs. A gate that cannot read the prompt
//! could not have caught the thing it is here to prevent.
//!
//! [[feedback_which_suite_runs_the_gate]] — the frontend suite proves the
//! fields reach the IR and stops there; without this file, "the IR carries it"
//! and "the model sees it" would be two claims with one test.

use axon::flow_dispatcher::pure_shape::reason_prompt;
use axon::ir_nodes::IRReasonStep;
use std::collections::HashMap;

fn node(given: &str, ask: &str, depth: Option<u32>, strategy: &str) -> IRReasonStep {
    IRReasonStep {
        node_type: "reason",
        source_line: 0,
        source_column: 0,
        strategy: strategy.into(),
        target: String::new(),
        given: given.into(),
        ask: ask.into(),
        depth,
    }
}

#[test]
fn f8r_1_the_ask_is_the_prompt() {
    let (prompt, _) = reason_prompt(
        &node("", "Is this a genuine anomaly?", None, ""),
        &HashMap::new(),
    );
    assert!(
        prompt.contains("Is this a genuine anomaly?"),
        "the `ask:` must BE the deliberation's question. Got: {prompt}"
    );
}

#[test]
fn f8r_2_given_is_resolved_against_the_flow_bindings() {
    // `given: Extract.output` names a prior step's output. Carrying the NAME
    // instead of the VALUE would send the model a variable it cannot read —
    // the §112 defect ("if the evidence is missing, substitute the belief")
    // wearing a prompt.
    let mut bindings = HashMap::new();
    bindings.insert(
        "Extract.output".to_string(),
        "PARTIES: Acme, Globex".to_string(),
    );

    let (prompt, _) = reason_prompt(
        &node("Extract.output", "Are there risky clauses?", None, ""),
        &bindings,
    );

    assert!(
        prompt.contains("PARTIES: Acme, Globex"),
        "the evidence named by `given:` must reach the model as its VALUE. \
         Got: {prompt}"
    );
    assert!(
        prompt.contains("Are there risky clauses?"),
        "…and the question must survive alongside it. Got: {prompt}"
    );
}

#[test]
fn f8r_3_a_comma_list_and_a_bracketed_list_both_resolve_every_member() {
    let mut bindings = HashMap::new();
    bindings.insert("Initialize.output".to_string(), "BASELINE".to_string());
    bindings.insert("sessions".to_string(), "SESSION_DATA".to_string());

    for given in ["Initialize.output, sessions", "[Initialize.output, sessions]"] {
        let (prompt, _) = reason_prompt(&node(given, "how did it evolve?", None, ""), &bindings);
        assert!(
            prompt.contains("BASELINE") && prompt.contains("SESSION_DATA"),
            "every member of `given: {given}` must be resolved, not just the \
             first. Got: {prompt}"
        );
    }
}

#[test]
fn f8r_4_depth_and_strategy_are_consumed_by_the_framing() {
    // §111's rule: a field decided by nothing is the defect this fase exists to
    // end. `depth:` and `strategy:` are declared POSTURE — they ride the
    // framing, which is the same channel `strategy` has always used — and that
    // is a real consumption, observable here.
    let (_, framing) = reason_prompt(&node("", "q", Some(4), "chain_of_thought"), &HashMap::new());
    assert!(
        framing.contains('4'),
        "`depth: 4` must reach the model or it is a field decided by nothing. \
         Framing: {framing}"
    );
    assert!(
        framing.contains("chain_of_thought"),
        "`chain_of_thought: enabled` must reach the model. Framing: {framing}"
    );
}

#[test]
fn f8r_5_the_positional_form_is_byte_unchanged() {
    // Back-compat: `reason <target>` with no block fields must produce exactly
    // the pre-§119.f.8 prompt.
    let mut n = node("", "", None, "deductive");
    n.target = "claim".into();
    let (prompt, _) = reason_prompt(&n, &HashMap::new());
    assert_eq!(prompt, "Reason about: claim using strategy `deductive`");
}
