//! v2.7.0 — capture the epistemic envelope from a tool's effect row.
//!
//! When a `use_tool` step dispatches a tool whose declared effect row
//! carries `epistemic:<level>`, the result inherits an epistemic CEILING:
//! a tool annotated `effects: <epistemic:speculate>` can only yield
//! speculative knowledge, so any confidence derived *through* it is clamped
//! to the `speculate` ceiling. That clamp is the Theorem 5.1
//! (*Stochastic Degenerative Soundness*) degradation made observable — the
//! exact contract the v2.4.0 parity gate names and v2.7.0 surfaces.
//!
//! This module is the pure **capture** half: it derives the
//! `(base, scope, confidence)` triple from `(effect_row, scope,
//! input_confidence)`. v2.7.0 wires the captured envelope onto the
//! `FlowEnvelope` in both transports; v2.7.0 locks cross-transport parity.
//! Keeping capture pure and side-effect-free makes the lattice arithmetic
//! exhaustively testable without the runner.

use crate::ir_nodes::{IRFlow, IRFlowNode, IRToolSpec};
use crate::lambda_data::apply_provenance_ceiling;
use serde::{Deserialize, Serialize};

/// The closed catalog of epistemic levels, ordered along the λD lattice
/// `⊥ ⊑ doubt ⊑ speculate ⊑ believe ⊑ know ⊑ ⊤`. Mirrors the frontend
/// `VALID_EPISTEMIC_LEVELS` (the type-checker rejects any other level, so a
/// level reaching this module is always a member of this set).
pub const EPISTEMIC_LEVELS: &[&str] = &["doubt", "speculate", "believe", "know"];

/// Theorem 5.1 — the ceiling for `know`, the apex of *derived* knowledge.
/// Mirrors the C23 kernel constant `AXON_CSYS_THEOREM_5_1_CEILING`
/// (`axon-csys/c-src/effects/envelope.c`): a stochastically derived claim
/// never reaches `1.0` — `⊤` (apodictic certainty) is reserved for
/// claims that are true by construction, never for anything a tool derives.
pub const KNOW_CEILING: f64 = 0.99;

/// The confidence CEILING imposed by each epistemic level — the maximum
/// certainty a claim derived through a tool at that level may carry.
///
/// Monotone non-decreasing along the lattice. The band boundaries
/// (`doubt ≤ 0.50`, `speculate ≤ 0.80`) mirror the certainty→lattice
/// thresholds the runtime already uses across the `axon_server` cognitive
/// handlers (e.g. `> 0.8 ⇒ believe`, `> 0.5 ⇒ speculate`); `know` is the
/// Theorem 5.1 derived apex ([`KNOW_CEILING`]); `believe` sits strictly
/// between `speculate` and `know`. Returns `None` for an unknown level.
pub fn level_ceiling(level: &str) -> Option<f64> {
    match level {
        "doubt" => Some(0.50),
        "speculate" => Some(0.80),
        "believe" => Some(0.95),
        "know" => Some(KNOW_CEILING),
        _ => None,
    }
}

/// The captured epistemic envelope of one tool dispatch — the Theorem 5.1
/// `(base, scope, confidence)` triple. Serialized verbatim onto the wire
/// (`FlowEnvelope.epistemic_envelopes` on the sync path; the `epistemic`
/// array on the streaming `axon.complete`) — v2.7.0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpistemicEnvelope {
    /// The lattice position imposed by the tool's `epistemic:<level>`
    /// effect (`doubt` | `speculate` | `believe` | `know`).
    pub base: String,
    /// The provenance scope the ceiling applies to — the dispatch site,
    /// e.g. `tool:WebSearch` or `step:Summarize`. Mirrors the
    /// `kind:identifier` shape of `FlowEnvelope::provenance_chain`.
    pub scope: String,
    /// The input certainty `ψ.c` clamped to the level's ceiling
    /// (`min(input, ceiling)` — never raised; "no silent upgrade").
    pub confidence: f64,
    /// v2.8.0 (D9) — the tool's DECLARED output type (`output_type:`,
    /// v2.8.0 D8), when it has one. This ties the epistemic ceiling to the
    /// TYPED output the dispatch produces: the `(base, scope, confidence)`
    /// triple is the ceiling on a value of THIS type, so a downstream
    /// `${Step.output}` / `reason { given: Step }` reference inherits the
    /// declared output's epistemic position in the lattice. `None` for a
    /// tool that declares no `output_type` (the v2.7.0 byte-identical wire —
    /// the field is ELIDED, not emitted as null, so existing flows are
    /// unchanged). Additive: a tool with `output_type` is a v2.8.0-new
    /// construct, so no pre-v2.8.0 flow's wire shifts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_type: Option<String>,
}

/// Extract the epistemic level from an effect row whose entries have the
/// form `epistemic:<level>` (the runtime effect_row representation produced
/// by `ir_generator`). Returns the first such entry's level — a tool
/// declares at most one epistemic base.
pub fn epistemic_level_of(effect_row: &[String]) -> Option<&str> {
    effect_row
        .iter()
        .find_map(|e| e.strip_prefix("epistemic:"))
        .filter(|level| !level.is_empty())
}

/// v2.7.0 — derive the epistemic envelope for one tool dispatch.
///
/// * `effect_row` — the tool's declared effects, e.g.
///   `["network", "epistemic:speculate"]`.
/// * `scope` — the dispatch-site identifier (`tool:<name>` / `step:<name>`).
/// * `input_confidence` — the incoming ψ certainty (`c ∈ [0, 1]`).
///
/// Returns `None` when the tool carries no epistemic base (there is no
/// degradation to surface) or carries an unrecognized level (defensive —
/// the frontend type-checker already rejects those). On `Some`, the
/// `confidence` is `apply_provenance_ceiling(input_confidence, ceiling)` —
/// the ceiling is a maximum, so a high-confidence input is degraded to the
/// level's cap while a low-confidence input is left untouched.
/// * `output_type` — the tool's declared `output_type:` (v2.8.0 D8), or
/// `None`. v2.8.0 (D9): when present, it rides the envelope so the
///   ceiling is tied to the typed output the dispatch yields.
pub fn capture(
    effect_row: &[String],
    scope: &str,
    input_confidence: f64,
    output_type: Option<&str>,
) -> Option<EpistemicEnvelope> {
    let level = epistemic_level_of(effect_row)?;
    let ceiling = level_ceiling(level)?;
    Some(EpistemicEnvelope {
        base: level.to_string(),
        scope: scope.to_string(),
        confidence: apply_provenance_ceiling(input_confidence, ceiling),
        output_type: output_type.map(|s| s.to_string()),
    })
}

/// v2.7.0 — derive the epistemic envelopes for one flow's tool
/// dispatches, straight from the IR. For each flow-level `use <Tool>` step,
/// look up the tool's declared effect row in `tools` and [`capture`] its
/// epistemic envelope. Steps whose tool has no epistemic base (or whose
/// tool is undeclared) contribute nothing.
///
/// `input_confidence` is the clean pre-dispatch ψ certainty — `1.0` for a
/// top-level flow (a tool's own ceiling is what surfaces the degradation);
/// a finer running ψ, when one exists, flows through `capture`'s `min`.
///
/// This is the SINGLE derivation both transports call (the sync runner with
/// its in-hand `ir`, the streaming resolver after re-deriving the IR from
/// source), so the wire carries byte-identical envelopes by construction —
/// the v2.7.0 parity invariant.
pub fn collect_for_flow(
    flow: &IRFlow,
    tools: &[IRToolSpec],
    input_confidence: f64,
) -> Vec<EpistemicEnvelope> {
    flow.steps
        .iter()
        .filter_map(|node| match node {
            IRFlowNode::UseTool(u) => {
                let tool = tools.iter().find(|t| t.name == u.tool_name)?;
                capture(
                    &tool.effect_row,
                    &format!("tool:{}", u.tool_name),
                    input_confidence,
                    // v2.8.0 (D9) — the declared output type rides the
                    // envelope so the ceiling binds to the typed result.
                    tool.output_type.as_deref(),
                )
            }
            // v2.12.0 — PIX retrieval reads controlled, pre-indexed
            // content: its intrinsic effect row is ⟨io, epistemic:believe⟩
            // (paper section 5.2). Retrieved leaf content is `believe` — trusted but
            // unverified, so it is degraded to the believe ceiling (0.95) and
            // never reaches `know` without anchor/shield validation (paper section 5.1,
            // program invariant #4). `navigate`/`drill` carry this base
            // intrinsically (not a user-declared effect), so we capture it here.
            IRFlowNode::Navigate(n) => {
                let label = if n.output_name.is_empty() { "Navigate" } else { &n.output_name };
                capture(&pix_believe_row(), &format!("navigate:{label}"), input_confidence, None)
            }
            IRFlowNode::Drill(d) => {
                let label = if d.output_name.is_empty() { "Drill" } else { &d.output_name };
                capture(&pix_believe_row(), &format!("drill:{label}"), input_confidence, None)
            }
            _ => None,
        })
        .collect()
}

/// v2.12.0 — the intrinsic effect row of a PIX retrieval step:
/// `io` (reads pre-indexed content) + `epistemic:believe` (trusted, unverified).
pub fn pix_believe_row() -> Vec<String> {
    vec!["io".to_string(), "epistemic:believe".to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ceilings_are_monotone_non_decreasing_along_the_lattice() {
        let mut prev = 0.0;
        for level in EPISTEMIC_LEVELS {
            let c = level_ceiling(level).expect("every catalog level has a ceiling");
            assert!(
                c >= prev,
                "ceiling for {level} ({c}) must be ≥ the previous level's ({prev}) — \
                 the lattice is ordered doubt ⊑ speculate ⊑ believe ⊑ know"
            );
            prev = c;
        }
    }

    #[test]
    fn pix_retrieval_carries_a_believe_envelope() {
        // v2.12.0 — navigate/drill surface ⟨io, believe⟩ at the believe ceiling
        // (paper section 5.2); leaf content is trusted-but-unverified, never `know`.
        let row = pix_believe_row();
        assert_eq!(epistemic_level_of(&row), Some("believe"));
        let env = capture(&row, "navigate:sections", 1.0, None).unwrap();
        assert_eq!(env.base, "believe");
        assert_eq!(env.scope, "navigate:sections");
        assert_eq!(
            env.confidence, 0.95,
            "degraded to the believe ceiling — retrieval never reaches know without validation"
        );
    }

    #[test]
    fn know_ceiling_is_the_theorem_5_1_derived_apex() {
        assert_eq!(level_ceiling("know"), Some(0.99));
        assert_eq!(KNOW_CEILING, 0.99, "must mirror the C23 kernel constant");
    }

    #[test]
    fn unknown_level_has_no_ceiling() {
        assert_eq!(level_ceiling("certain"), None);
        assert_eq!(level_ceiling(""), None);
    }

    #[test]
    fn extracts_the_epistemic_level_from_a_mixed_effect_row() {
        let row = vec!["network".to_string(), "epistemic:speculate".to_string()];
        assert_eq!(epistemic_level_of(&row), Some("speculate"));
    }

    #[test]
    fn no_epistemic_entry_yields_none() {
        let row = vec!["network".to_string(), "read".to_string()];
        assert_eq!(epistemic_level_of(&row), None);
        assert_eq!(capture(&row, "tool:Search", 0.9, None), None);
    }

    #[test]
    fn high_confidence_input_is_degraded_to_the_level_ceiling() {
        // A `speculate` tool caps confidence at 0.80 even if the input ψ
        // arrives near-certain — the Theorem 5.1 degradation, observable.
        let row = vec!["epistemic:speculate".to_string()];
        let env = capture(&row, "tool:WebSearch", 0.97, None).expect("epistemic envelope");
        assert_eq!(env.base, "speculate");
        assert_eq!(env.scope, "tool:WebSearch");
        assert_eq!(env.confidence, 0.80);
        assert_eq!(env.output_type, None);
    }

    #[test]
    fn low_confidence_input_is_left_untouched_by_a_higher_ceiling() {
        // No silent UPGRADE: a `know` tool does not raise a doubtful input.
        let row = vec!["epistemic:know".to_string()];
        let env = capture(&row, "step:Resolve", 0.30, None).expect("epistemic envelope");
        assert_eq!(env.base, "know");
        assert_eq!(env.confidence, 0.30, "the ceiling is a max, never a floor");
    }

    #[test]
    fn out_of_range_input_is_clamped_into_the_unit_interval_then_capped() {
        let row = vec!["epistemic:doubt".to_string()];
        // 1.5 → clamp to 1.0 → cap at doubt's 0.50.
        assert_eq!(capture(&row, "tool:T", 1.5, None).unwrap().confidence, 0.50);
        // -0.2 → clamp to 0.0 → still 0.0.
        assert_eq!(capture(&row, "tool:T", -0.2, None).unwrap().confidence, 0.0);
    }

    // ── v2.7.0 — collect_for_flow (IR-driven derivation) ──────────────

    fn tool(name: &str, effects: &[&str]) -> IRToolSpec {
        IRToolSpec {
            node_type: "tool",
            source_line: 0,
            source_column: 0,
            name: name.into(),
            provider: String::new(),
            max_results: None,
            filter_expr: String::new(),
            timeout: String::new(),
            runtime: String::new(),
            resource_ref: String::new(),
            sandbox: None,
            input_schema: Vec::new(),
            output_schema: String::new(),
            parameters: Vec::new(),
            output_type: None,
            requires: Vec::new(),
            secret: String::new(),
            secret_partition: String::new(),
            effect_row: effects.iter().map(|e| e.to_string()).collect(),
            target: None,
            risk: None,
            argv: Vec::new(),
            cache: String::new(),
            scrape: None,
        }
    }

    fn use_tool(tool_name: &str) -> IRFlowNode {
        IRFlowNode::UseTool(crate::ir_nodes::IRUseToolStep {
            node_type: "use_tool",
            source_line: 0,
            source_column: 0,
            tool_name: tool_name.into(),
            argument: "${query}".into(),
            named_args: Vec::new(),
        })
    }

    fn flow_with_steps(steps: Vec<IRFlowNode>) -> IRFlow {
        IRFlow {
            node_type: "flow",
            source_line: 0,
            source_column: 0,
            name: "F".into(),
            parameters: Vec::new(),
            return_type_name: "Unit".into(),
            return_type_generic: String::new(),
            return_type_optional: false,
            steps,
            edges: Vec::new(),
            execution_levels: Vec::new(),
        }
    }

    #[test]
    fn collects_one_envelope_per_epistemic_tool_dispatch() {
        let tools = vec![
            tool("WebSearch", &["network", "epistemic:speculate"]),
            tool("ExactLookup", &["compute", "epistemic:know"]),
        ];
        let flow = flow_with_steps(vec![use_tool("WebSearch"), use_tool("ExactLookup")]);
        let envs = collect_for_flow(&flow, &tools, 1.0);
        assert_eq!(envs.len(), 2);
        assert_eq!(envs[0], EpistemicEnvelope {
            base: "speculate".into(),
            scope: "tool:WebSearch".into(),
            confidence: 0.80,
            output_type: None,
        });
        assert_eq!(envs[1], EpistemicEnvelope {
            base: "know".into(),
            scope: "tool:ExactLookup".into(),
            confidence: 0.99,
            output_type: None,
        });
    }

    // ── v2.8.0 (D9) — output_type rides the lattice envelope ────

    /// A tool declaring BOTH an epistemic level AND an `output_type` ties
    /// the ceiling to the typed output: the envelope carries the declared
    /// type, so a downstream `${Step.output}` reference inherits the
    /// epistemic position of a value of THAT type.
    fn tool_with_output(name: &str, effects: &[&str], output_type: &str) -> IRToolSpec {
        let mut t = tool(name, effects);
        t.output_type = Some(output_type.to_string());
        t
    }

    #[test]
    fn output_type_rides_the_envelope_when_declared() {
        let env = capture(
            &["epistemic:believe".to_string()],
            "tool:CrmRadar",
            1.0,
            Some("CrmReport"),
        )
        .expect("epistemic envelope");
        assert_eq!(env.base, "believe");
        assert_eq!(env.confidence, 0.95, "believe ceiling");
        assert_eq!(
            env.output_type.as_deref(),
            Some("CrmReport"),
            "the declared output type rides the envelope (D9)"
        );
    }

    #[test]
    fn collect_for_flow_attaches_output_type_from_the_tool() {
        let tools = vec![tool_with_output(
            "CrmRadar",
            &["network", "epistemic:speculate"],
            "CrmReport",
        )];
        let flow = flow_with_steps(vec![use_tool("CrmRadar")]);
        let envs = collect_for_flow(&flow, &tools, 1.0);
        assert_eq!(envs.len(), 1);
        assert_eq!(
            envs[0],
            EpistemicEnvelope {
                base: "speculate".into(),
                scope: "tool:CrmRadar".into(),
                confidence: 0.80,
                output_type: Some("CrmReport".into()),
            }
        );
    }

    #[test]
    fn output_type_is_elided_from_the_wire_when_absent() {
        // v2.7.0 wire byte-compat: a tool with no output_type serializes
        // WITHOUT the field (skip-if-none), so pre-v2.8.0 flows are unchanged.
        let env = capture(&["epistemic:doubt".to_string()], "tool:T", 0.4, None).unwrap();
        let json = serde_json::to_value(&env).unwrap();
        assert!(
            json.get("output_type").is_none(),
            "output_type must be elided (not null) when absent: {json}"
        );
        assert_eq!(json["base"], "doubt");
        assert_eq!(json["confidence"], 0.4);
    }

    #[test]
    fn output_type_is_present_on_the_wire_when_declared() {
        let env = capture(
            &["epistemic:know".to_string()],
            "tool:Exact",
            1.0,
            Some("Answer"),
        )
        .unwrap();
        let json = serde_json::to_value(&env).unwrap();
        assert_eq!(json["output_type"], "Answer");
    }

    #[test]
    fn envelope_round_trips_through_json_with_output_type() {
        let env = capture(
            &["epistemic:believe".to_string()],
            "tool:CrmRadar",
            1.0,
            Some("CrmReport"),
        )
        .unwrap();
        let json = serde_json::to_string(&env).unwrap();
        let back: EpistemicEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn legacy_wire_without_output_type_deserializes_to_none() {
        // v2.7.0 back-compat: an old (pre-v2.8.0) wire envelope carries no
        // `output_type` key; `#[serde(default)]` restores it as None.
        let legacy = r#"{"base":"speculate","scope":"tool:Old","confidence":0.8}"#;
        let env: EpistemicEnvelope = serde_json::from_str(legacy).unwrap();
        assert_eq!(env.output_type, None);
        assert_eq!(env.base, "speculate");
    }

    #[test]
    fn a_non_epistemic_tool_contributes_no_envelope() {
        let tools = vec![tool("PlainHttp", &["network"])];
        let flow = flow_with_steps(vec![use_tool("PlainHttp")]);
        assert!(collect_for_flow(&flow, &tools, 1.0).is_empty());
    }

    #[test]
    fn an_undeclared_tool_is_skipped_not_panicked() {
        let tools: Vec<IRToolSpec> = Vec::new();
        let flow = flow_with_steps(vec![use_tool("Ghost")]);
        assert!(collect_for_flow(&flow, &tools, 1.0).is_empty());
    }
}
