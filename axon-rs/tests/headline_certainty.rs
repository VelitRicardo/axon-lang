//! v2.7.0 — the epistemic ceiling propagates to the HEADLINE
//! `FlowEnvelope.certainty` at `seal()`.
//!
//! Founder decision (2026-06-03): a gateway reading the envelope's headline
//! certainty must NOT see a nominal `know` (c≈1.0) while an internal tool
//! degraded the computation to `speculate` (c≤0.80). `seal()` clamps the
//! headline to the λD lattice meet (⊓) of the per-tool ceilings — i.e. the
//! minimum of the `epistemic_envelopes` confidences — folded through the C23
//! kernel's Theorem 5.1 egress. No epistemic tool ⇒ no change.

use axon::epistemic_capture::EpistemicEnvelope;
use axon::wire_envelope::{FlowEnvelope, StepAuditTrail};

fn envelope(
    certainty: f64,
    anchor_breaches: usize,
    epistemic: Vec<EpistemicEnvelope>,
) -> FlowEnvelope {
    FlowEnvelope {
        ontological_type: "R".into(),
        result: serde_json::Value::Null,
        certainty,
        epistemic_envelopes: epistemic,
        provenance_chain: vec!["flow:F".into()],
        step_audit: StepAuditTrail {
            anchor_breaches,
            ..Default::default()
        },
        audit_chain_hash: String::new(),
        blame_attribution: None,
        execution_metrics: Default::default(),
        trace_id: "t".into(),
        error: None,
        temporal_context: None,
    }
}

fn eps(base: &str, confidence: f64) -> EpistemicEnvelope {
    EpistemicEnvelope {
        base: base.into(),
        scope: format!("tool:{base}"),
        confidence,
        output_type: None, // v2.8.0 — headline-certainty test is output-type-agnostic
    }
}

#[test]
fn speculate_tool_caps_the_headline_certainty() {
    // Clean flow (no breach), but a speculate tool (ceiling 0.80) was used.
    let sealed = envelope(1.0, 0, vec![eps("speculate", 0.80)]).seal();
    assert_eq!(
        sealed.certainty, 0.80,
        "the headline MUST decay to the speculate ceiling — no silent `know`"
    );
}

#[test]
fn the_minimum_ceiling_wins_across_multiple_tools() {
    let sealed = envelope(
        1.0,
        0,
        vec![eps("know", 0.99), eps("speculate", 0.80), eps("believe", 0.95)],
    )
    .seal();
    assert_eq!(sealed.certainty, 0.80, "the λD meet ⊓ is the minimum ceiling");
}

#[test]
fn a_know_tool_still_caps_below_apodictic_certainty() {
    // `know` is the apex of DERIVED knowledge (0.99) — never ⊤ (1.0).
    let sealed = envelope(1.0, 0, vec![eps("know", 0.99)]).seal();
    assert_eq!(sealed.certainty, 0.99);
}

#[test]
fn no_epistemic_tool_leaves_the_headline_untouched() {
    // D5 wire byte-compat: a pre-55 flow's certainty is unchanged.
    let clean = envelope(1.0, 0, Vec::new()).seal();
    assert_eq!(clean.certainty, 1.0, "clean flow, no epistemic tool ⇒ 1.0");
}

#[test]
fn theorem_5_1_kernel_clamp_still_composes_with_the_epistemic_meet() {
    // Derived (anchor breach) + a speculate tool: the headline is the meet
    // of the kernel's 0.99 derived-clamp and the 0.80 ceiling → 0.80.
    let derived_speculate = envelope(1.0, 1, vec![eps("speculate", 0.80)]).seal();
    assert_eq!(derived_speculate.certainty, 0.80);

    // Derived + NO epistemic tool: only the Theorem 5.1 clamp applies → 0.99.
    let derived_only = envelope(1.0, 1, Vec::new()).seal();
    assert_eq!(derived_only.certainty, 0.99);
}

#[test]
fn a_low_input_certainty_is_not_raised_by_a_higher_ceiling() {
    // The ceiling is a max, never a floor (no silent upgrade): an already
    // low headline stays low even under a `know` tool.
    let sealed = envelope(0.40, 0, vec![eps("know", 0.99)]).seal();
    assert_eq!(sealed.certainty, 0.40);
}
