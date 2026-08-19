//! v2.83.0 (runtime half) — `weave` synthesises over the source VALUES.
//!
//! The grammar half is `axon-frontend/tests/weave_statement.rs`.
//!
//! **What was wrong.** `run_weave` built `" from sources [Extract.output,
//! Check.output]"` — the NAMES. `weave`'s whole job is to stitch prior outputs
//! into one result, and it never had the outputs: the model was handed a list
//! of identifiers it could not read and asked to synthesise them. Identical to
//! `reason`'s `given:` (v2.83.0), and `advertised.rs` attested
//! `("weave", Real { proof: "pure_shape::run_weave" })` through both.
//!
//! Asserted against the pure assembly, because the stub backend answers
//! `"(stub)"` whatever it is asked — a gate that cannot read the prompt could
//! not catch a prompt that was never built.

use axon::flow_dispatcher::pure_shape::weave_prompt;
use axon::ir_nodes::IRWeaveStep;
use std::collections::HashMap;

fn node(sources: &[&str], format: &str, include: &[&str]) -> IRWeaveStep {
    IRWeaveStep {
        node_type: "weave",
        source_line: 0,
        source_column: 0,
        sources: sources.iter().map(|s| s.to_string()).collect(),
        target: String::new(),
        format_type: format.into(),
        priority: Vec::new(),
        style: String::new(),
        include: include.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn f9r_1_the_sources_carry_their_values_not_their_names() {
    let mut b = HashMap::new();
    b.insert("Extract.output".to_string(), "PARTIES: Acme, Globex".to_string());
    b.insert("Check.output".to_string(), "RISK: indemnity clause".to_string());

    let (prompt, _) = weave_prompt(&node(&["Extract.output", "Check.output"], "", &[]), &b);

    assert!(
        prompt.contains("PARTIES: Acme, Globex") && prompt.contains("RISK: indemnity clause"),
        "every source must reach the model as its VALUE — synthesising a list \
         of identifiers the model cannot read is synthesising nothing. \
         Got: {prompt}"
    );
}

#[test]
fn f9r_2_include_is_a_requirement_the_model_sees() {
    // v2.67.0's rule: a field decided by nothing is the defect. `include:` is a
    // constraint on the synthesis, and it must reach the model to be one.
    let (_, framing) = weave_prompt(
        &node(&["a"], "", &["summary", "risks", "recommendations"]),
        &HashMap::new(),
    );
    for part in ["summary", "risks", "recommendations"] {
        assert!(
            framing.contains(part),
            "`include: [… {part} …]` must reach the model. Framing: {framing}"
        );
    }
}

#[test]
fn f9r_3_the_format_and_target_are_named_in_the_instruction() {
    let mut n = node(&["a"], "StructuredReport", &[]);
    n.target = "Report".into();
    let (prompt, _) = weave_prompt(&n, &HashMap::new());
    assert!(
        prompt.contains("StructuredReport") && prompt.contains("Report"),
        "the declared format and destination must be in the instruction. \
         Got: {prompt}"
    );
}

#[test]
fn f9r_4_a_sourceless_weave_keeps_the_pre_119f9_shape() {
    let mut n = node(&[], "", &[]);
    n.target = "report".into();
    let (prompt, _) = weave_prompt(&n, &HashMap::new());
    assert_eq!(prompt, "Weave: into report");
}

#[test]
fn f9r_5_an_unbound_source_contributes_itself_not_a_hole() {
    // Degrading to the name matches every other target-resolving handler on
    // this path. What must NOT happen is the source vanishing — a silently
    // dropped input is the v2.67.0 defect ("if the evidence is missing,
    // substitute the belief") in the synthesis position.
    let (prompt, _) = weave_prompt(&node(&["never_bound"], "", &[]), &HashMap::new());
    assert!(
        prompt.contains("never_bound"),
        "an unbound source must still appear. Got: {prompt}"
    );
}
