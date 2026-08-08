//! Execution Cost Estimator — estimate token usage and USD cost before running a flow.
//!
//! Analyzes IR to count steps by type and estimate:
//!   - Input tokens per step (prompt construction)
//!   - Output tokens per step (model generation)
//!   - Tool call overhead
//!   - Total estimated cost in USD
//!
//! Pricing model: configurable per-token rates (default: Claude Sonnet).
//! Output formats: text (human), json (machine).

use crate::ir_nodes::{IRFlowNode, IRProgram};
use serde::Serialize;

// ── Pricing ──────────────────────────────────────────────────────────────

/// Token pricing configuration (per million tokens).
#[derive(Debug, Clone, Serialize)]
pub struct PricingModel {
    pub name: String,
    pub input_per_million: f64,
    pub output_per_million: f64,
}

impl PricingModel {
    /// Default pricing: Claude Sonnet 4 ($3/$15 per million).
    pub fn default_sonnet() -> Self {
        PricingModel {
            name: "claude-sonnet-4".to_string(),
            input_per_million: 3.0,
            output_per_million: 15.0,
        }
    }

    /// Claude Opus 4 ($15/$75 per million).
    pub fn opus() -> Self {
        PricingModel {
            name: "claude-opus-4".to_string(),
            input_per_million: 15.0,
            output_per_million: 75.0,
        }
    }

    /// Claude Haiku 3.5 ($0.80/$4 per million).
    pub fn haiku() -> Self {
        PricingModel {
            name: "claude-haiku-3.5".to_string(),
            input_per_million: 0.80,
            output_per_million: 4.0,
        }
    }

    /// Compute cost from token counts.
    pub fn compute_cost(&self, input_tokens: u64, output_tokens: u64) -> f64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.input_per_million;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * self.output_per_million;
        input_cost + output_cost
    }
}

// ── Step classification ──────────────────────────────────────────────────

/// Classification of a step for cost estimation purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// Standard ask step — single prompt/response.
    Ask,
    /// Tool use — prompt + tool call overhead.
    ToolCall,
    /// Reasoning step — deeper inference, higher output.
    Reason,
    /// Probe — targeted inspection.
    Probe,
    /// Validate — verification pass.
    Validate,
    /// Refine — iterative improvement.
    Refine,
    /// Weave — multi-source synthesis.
    Weave,
    /// Memory ops — remember/recall/persist/retrieve (minimal LLM cost).
    Memory,
    /// Control flow — conditional/loop (no direct LLM cost).
    Control,
    /// Parallel — container for parallel execution.
    Parallel,
    /// Deliberate/Consensus/Forge — multi-agent blocks.
    MultiAgent,
    /// Other cognitive steps (focus, associate, aggregate, explore, etc.).
    Cognitive,
}

/// Token estimate for a single step kind.
#[derive(Debug, Clone, Serialize)]
pub struct StepEstimate {
    pub kind: StepKind,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Default token estimates per step kind.
fn default_estimate(kind: StepKind) -> StepEstimate {
    let (input, output) = match kind {
        StepKind::Ask =>        (800, 400),
        StepKind::ToolCall =>   (1000, 300),
        StepKind::Reason =>     (1200, 800),
        StepKind::Probe =>      (600, 200),
        StepKind::Validate =>   (700, 300),
        StepKind::Refine =>     (900, 600),
        StepKind::Weave =>      (1500, 600),
        StepKind::Memory =>     (100, 50),
        StepKind::Control =>    (0, 0),
        StepKind::Parallel =>   (0, 0),
        StepKind::MultiAgent => (2000, 1000),
        StepKind::Cognitive =>  (800, 400),
    };
    StepEstimate { kind, input_tokens: input, output_tokens: output }
}

// ── IR analysis ──────────────────────────────────────────────────────────

/// Classify an IR flow node into a step kind.
fn classify_node(node: &IRFlowNode) -> StepKind {
    match node {
        IRFlowNode::Step(_) => StepKind::Ask,
        // §Fase 109 — a compile-time-derived derivative evaluates with the
        // pure expression evaluator: no LLM, no store, no cost — the same
        // Control class as `let`/`return`.
        IRFlowNode::Grad(_) => StepKind::Control,
        IRFlowNode::UseTool(_) => StepKind::ToolCall,
        IRFlowNode::Reason(_) => StepKind::Reason,
        IRFlowNode::Probe(_) => StepKind::Probe,
        IRFlowNode::Validate(_) => StepKind::Validate,
        IRFlowNode::Refine(_) => StepKind::Refine,
        IRFlowNode::Weave(_) => StepKind::Weave,
        IRFlowNode::Remember(_) | IRFlowNode::Recall(_)
        | IRFlowNode::Persist(_) | IRFlowNode::Retrieve(_)
        | IRFlowNode::Mutate(_) | IRFlowNode::Purge(_) => StepKind::Memory,
        IRFlowNode::Conditional(_) | IRFlowNode::ForIn(_)
        | IRFlowNode::Let(_) | IRFlowNode::Return(_)
        // Fase 19.e — break/continue are pure control transfers, no
        // model call, no I/O. Classify as Control alongside Return.
        | IRFlowNode::Break(_) | IRFlowNode::Continue(_) => StepKind::Control,
        IRFlowNode::Par(_) | IRFlowNode::Stream(_) => StepKind::Parallel,
        IRFlowNode::Deliberate(_) | IRFlowNode::Consensus(_)
        | IRFlowNode::Forge(_) => StepKind::MultiAgent,
        IRFlowNode::Focus(_) | IRFlowNode::Associate(_)
        | IRFlowNode::Aggregate(_) | IRFlowNode::Explore(_)
        | IRFlowNode::Ingest(_) | IRFlowNode::Navigate(_)
        | IRFlowNode::Drill(_) | IRFlowNode::Trail(_)
        | IRFlowNode::Corroborate(_) | IRFlowNode::Listen(_)
        | IRFlowNode::DaemonStep(_) | IRFlowNode::Hibernate(_) => StepKind::Cognitive,
        IRFlowNode::ShieldApply(_) | IRFlowNode::OtsApply(_)
        | IRFlowNode::MandateApply(_) | IRFlowNode::ComputeApply(_)
        | IRFlowNode::LambdaDataApply(_) | IRFlowNode::Transact(_) => StepKind::Control,
        // §λ-L-E Fase 13 — Mobile typed channel reductions are π-calc
        // prefixes, classified as Cognitive alongside Listen/DaemonStep.
        IRFlowNode::Emit(_) | IRFlowNode::Publish(_) | IRFlowNode::Discover(_) => StepKind::Cognitive,
        // §Fase 51.a — the `quant` Hilbert-space projection block is cognitive
        // computation (variational / kernel-feature evaluation).
        IRFlowNode::Quant(_) => StepKind::Cognitive,
        // §Fase 88.a — the `warden` adversarial-analysis block is cognitive
        // (LLM abduction over authorized evidence).
        IRFlowNode::Warden(_) => StepKind::Cognitive,
        // §Fase 51.d.2 — `yield` is the amplitude-collapse measurement point.
        IRFlowNode::Yield(_) => StepKind::Cognitive,
        // §Fase 52.c — `run <Flow>` is orchestration: it dispatches a declared
        // flow whose own cost is attributed to that flow, not to this step.
        IRFlowNode::Run(_) => StepKind::Control,
        // §Fase 92.c — `mint` is a pure effect (no model call, no store I/O):
        // classify as Control alongside the other non-cognitive verbs.
        IRFlowNode::Mint(_) => StepKind::Control,
        // §Fase 94.b — `rotate` performs one mediated tool exchange per
        // matching custody entry (network I/O, no model call): classify as
        // ToolCall — the closest honest kind for a set-oriented dispatch.
        IRFlowNode::Rotate(_) => StepKind::ToolCall,
    }
}

/// Count steps by kind, recursively walking nested blocks.
fn count_steps(nodes: &[IRFlowNode]) -> Vec<(StepKind, u32)> {
    let mut counts = std::collections::HashMap::new();

    fn walk(nodes: &[IRFlowNode], counts: &mut std::collections::HashMap<StepKind, u32>) {
        for node in nodes {
            let kind = classify_node(node);
            *counts.entry(kind).or_insert(0) += 1;

            // Recurse into nested blocks
            match node {
                IRFlowNode::Conditional(c) => {
                    walk(&c.then_body, counts);
                    walk(&c.else_body, counts);
                }
                IRFlowNode::ForIn(f) => walk(&f.body, counts),
                // Par, Deliberate, Consensus, Forge, Stream, Transact are stub
                // structs without body fields in current IR — no recursion needed.
                _ => {}
            }
        }
    }

    walk(nodes, &mut counts);
    let mut result: Vec<_> = counts.into_iter().collect();
    result.sort_by_key(|(k, _)| format!("{:?}", k));
    result
}

// ── Cost report ──────────────────────────────────────────────────────────

/// Per-flow cost breakdown.
#[derive(Debug, Clone, Serialize)]
pub struct FlowCostEstimate {
    pub flow_name: String,
    pub step_counts: Vec<StepCountEntry>,
    pub total_steps: u32,
    pub estimated_input_tokens: u64,
    pub estimated_output_tokens: u64,
    pub estimated_total_tokens: u64,
}

/// Step count entry for a specific kind.
#[derive(Debug, Clone, Serialize)]
pub struct StepCountEntry {
    pub kind: StepKind,
    pub count: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Full cost report for an AXON program.
#[derive(Debug, Clone, Serialize)]
pub struct CostReport {
    pub pricing: PricingModel,
    pub flows: Vec<FlowCostEstimate>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
}

/// Estimate cost for an entire IR program.
pub fn estimate_program(ir: &IRProgram, pricing: &PricingModel) -> CostReport {
    let mut flows = Vec::new();
    let mut total_input: u64 = 0;
    let mut total_output: u64 = 0;

    for flow in &ir.flows {
        let step_counts = count_steps(&flow.steps);
        let mut flow_input: u64 = 0;
        let mut flow_output: u64 = 0;
        let mut total_steps: u32 = 0;

        let entries: Vec<StepCountEntry> = step_counts
            .iter()
            .map(|(kind, count)| {
                let est = default_estimate(*kind);
                let input = est.input_tokens * (*count as u64);
                let output = est.output_tokens * (*count as u64);
                flow_input += input;
                flow_output += output;
                total_steps += count;
                StepCountEntry {
                    kind: *kind,
                    count: *count,
                    input_tokens: input,
                    output_tokens: output,
                }
            })
            .collect();

        total_input += flow_input;
        total_output += flow_output;

        flows.push(FlowCostEstimate {
            flow_name: flow.name.clone(),
            step_counts: entries,
            total_steps,
            estimated_input_tokens: flow_input,
            estimated_output_tokens: flow_output,
            estimated_total_tokens: flow_input + flow_output,
        });
    }

    let cost = pricing.compute_cost(total_input, total_output);

    CostReport {
        pricing: pricing.clone(),
        flows,
        total_input_tokens: total_input,
        total_output_tokens: total_output,
        total_tokens: total_input + total_output,
        estimated_cost_usd: cost,
    }
}

// ── Output formatting ────────────────────────────────────────────────────

/// Format cost report as human-readable text.
pub fn format_text(report: &CostReport) -> String {
    let mut out = String::new();

    out.push_str(&format!("AXON Execution Cost Estimate ({})\n", report.pricing.name));
    out.push_str(&format!("Pricing: ${}/M input, ${}/M output\n",
        report.pricing.input_per_million, report.pricing.output_per_million));
    out.push_str(&"─".repeat(60));
    out.push('\n');

    for flow in &report.flows {
        out.push_str(&format!("\nFlow: {}\n", flow.flow_name));
        out.push_str(&format!("  Steps: {}\n", flow.total_steps));

        for entry in &flow.step_counts {
            if entry.count > 0 {
                out.push_str(&format!("    {:12} x{:<3}  ~{} input + {} output tokens\n",
                    format!("{:?}", entry.kind),
                    entry.count,
                    entry.input_tokens,
                    entry.output_tokens,
                ));
            }
        }

        out.push_str(&format!("  Subtotal: ~{} tokens ({} in + {} out)\n",
            flow.estimated_total_tokens,
            flow.estimated_input_tokens,
            flow.estimated_output_tokens,
        ));
    }

    out.push_str(&format!("\n{}\n", "─".repeat(60)));
    out.push_str(&format!("Total: ~{} tokens ({} in + {} out)\n",
        report.total_tokens, report.total_input_tokens, report.total_output_tokens));
    out.push_str(&format!("Estimated cost: ${:.6} USD\n", report.estimated_cost_usd));

    out
}

/// CLI entry point: estimate cost for an .axon file.
pub fn run_estimate(file: &str, format: &str, model: &str) -> i32 {
    let source = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", file, e);
            return 1;
        }
    };

    let tokens = match crate::lexer::Lexer::new(&source, file).tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Lexer error: {:?}", e);
            return 1;
        }
    };
    let ast = match crate::parser::Parser::new(tokens).parse() {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("Parse error: {:?}", e);
            return 1;
        }
    };

    let ir = crate::ir_generator::IRGenerator::new().generate(&ast);

    let pricing = match model {
        "opus" => PricingModel::opus(),
        "haiku" => PricingModel::haiku(),
        _ => PricingModel::default_sonnet(),
    };

    let report = estimate_program(&ir, &pricing);

    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        }
        _ => {
            print!("{}", format_text(&report));
        }
    }

    0
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pricing_sonnet_defaults() {
        let p = PricingModel::default_sonnet();
        assert_eq!(p.input_per_million, 3.0);
        assert_eq!(p.output_per_million, 15.0);
    }

    #[test]
    fn pricing_compute_cost() {
        let p = PricingModel::default_sonnet();
        // 1M input + 1M output = $3 + $15 = $18
        let cost = p.compute_cost(1_000_000, 1_000_000);
        assert!((cost - 18.0).abs() < 0.001);
    }

    #[test]
    fn pricing_zero_tokens() {
        let p = PricingModel::default_sonnet();
        assert_eq!(p.compute_cost(0, 0), 0.0);
    }

    #[test]
    fn pricing_opus_rates() {
        let p = PricingModel::opus();
        assert_eq!(p.input_per_million, 15.0);
        assert_eq!(p.output_per_million, 75.0);
        let cost = p.compute_cost(1_000_000, 1_000_000);
        assert!((cost - 90.0).abs() < 0.001);
    }

    #[test]
    fn pricing_haiku_rates() {
        let p = PricingModel::haiku();
        assert_eq!(p.input_per_million, 0.80);
        assert_eq!(p.output_per_million, 4.0);
    }

    #[test]
    fn classify_step_kinds() {
        use crate::ir_nodes::*;

        let step = IRFlowNode::Step(IRStep {
            node_type: "Step",
            source_line: 1, source_column: 1,
            name: "s1".into(), persona_ref: "".into(),
            given: "".into(), ask: "do something".into(),
            use_tool: None, probe: None, reason: None, weave: None,
            output_type: "".into(), confidence_floor: None,
            navigate_ref: "".into(), apply_ref: "".into(),
            pix_ops: Vec::new(),
            guards: Vec::new(),
            requires_context: None,            now_tz: None,            body: vec![],
        });
        assert_eq!(classify_node(&step), StepKind::Ask);

        let tool = IRFlowNode::UseTool(IRUseToolStep {
            node_type: "UseTool",
            source_line: 1, source_column: 1,
            tool_name: "search".into(), argument: "q".into(),
            named_args: Vec::new(),
        });
        assert_eq!(classify_node(&tool), StepKind::ToolCall);

        let reason = IRFlowNode::Reason(IRReasonStep {
            node_type: "Reason",
            source_line: 1, source_column: 1,
            strategy: "deductive".into(), target: "t".into(),
            given: String::new(), ask: String::new(), depth: None,
        });
        assert_eq!(classify_node(&reason), StepKind::Reason);
    }

    #[test]
    fn count_steps_flat() {
        use crate::ir_nodes::*;

        let nodes = vec![
            IRFlowNode::Step(IRStep {
                node_type: "Step", source_line: 1, source_column: 1,
                name: "s1".into(), persona_ref: "".into(),
                given: "".into(), ask: "a".into(),
                use_tool: None, probe: None, reason: None, weave: None,
                output_type: "".into(), confidence_floor: None,
                navigate_ref: "".into(), apply_ref: "".into(),
                pix_ops: Vec::new(),
                guards: Vec::new(),
                requires_context: None,                now_tz: None,                body: vec![],
            }),
            IRFlowNode::Step(IRStep {
                node_type: "Step", source_line: 2, source_column: 1,
                name: "s2".into(), persona_ref: "".into(),
                given: "".into(), ask: "b".into(),
                use_tool: None, probe: None, reason: None, weave: None,
                output_type: "".into(), confidence_floor: None,
                navigate_ref: "".into(), apply_ref: "".into(),
                pix_ops: Vec::new(),
                guards: Vec::new(),
                requires_context: None,                now_tz: None,                body: vec![],
            }),
            IRFlowNode::UseTool(IRUseToolStep {
                node_type: "UseTool", source_line: 3, source_column: 1,
                tool_name: "t".into(), argument: "a".into(),
                named_args: Vec::new(),
            }),
        ];

        let counts = count_steps(&nodes);
        let ask_count = counts.iter().find(|(k, _)| *k == StepKind::Ask).map(|(_, c)| *c).unwrap_or(0);
        let tool_count = counts.iter().find(|(k, _)| *k == StepKind::ToolCall).map(|(_, c)| *c).unwrap_or(0);
        assert_eq!(ask_count, 2);
        assert_eq!(tool_count, 1);
    }

    #[test]
    fn count_steps_nested_conditional() {
        use crate::ir_nodes::*;

        let inner_step = IRFlowNode::Step(IRStep {
            node_type: "Step", source_line: 1, source_column: 1,
            name: "inner".into(), persona_ref: "".into(),
            given: "".into(), ask: "x".into(),
            use_tool: None, probe: None, reason: None, weave: None,
            output_type: "".into(), confidence_floor: None,
            navigate_ref: "".into(), apply_ref: "".into(),
            pix_ops: Vec::new(),
            guards: Vec::new(),
            requires_context: None,            now_tz: None,            body: vec![],
        });

        let cond = IRFlowNode::Conditional(IRConditional {
            node_type: "Conditional", source_line: 1, source_column: 1,
            condition: "c".into(), comparison_op: "==".into(),
            comparison_value: "true".into(),
            then_body: vec![inner_step],
            else_body: vec![],
            conditions: vec![],
            conjunctor: "".into(),
            cond: None,
        });

        let counts = count_steps(&[cond]);
        let control = counts.iter().find(|(k, _)| *k == StepKind::Control).map(|(_, c)| *c).unwrap_or(0);
        let ask = counts.iter().find(|(k, _)| *k == StepKind::Ask).map(|(_, c)| *c).unwrap_or(0);
        assert_eq!(control, 1); // the conditional itself
        assert_eq!(ask, 1); // the nested step
    }

    #[test]
    fn estimate_program_empty() {
        let ir = IRProgram {
            node_type: "Program",
            source_line: 0, source_column: 0,
            personas: vec![], contexts: vec![], anchors: vec![],
            tools: vec![], memories: vec![], types: vec![],
            flows: vec![], runs: vec![], imports: vec![],
            agents: vec![], shields: vec![], windows: vec![], budgets: vec![], daemons: vec![],
            ots_specs: vec![], pix_specs: vec![], ledger_specs: vec![], corpus_specs: vec![],
            psyche_specs: vec![], mandate_specs: vec![],
            lambda_data_specs: vec![], compute_specs: vec![],
            axonstore_specs: vec![], endpoints: vec![],
            extensions: vec![],
            dataspace_specs: vec![],
            notifications: vec![],
            resources: vec![],
            fabrics: vec![],
            manifests: vec![],
            observations: vec![],
            intention_tree: None,
            reconciles: vec![],
            leases: vec![],
            ensembles: vec![],
            sessions: vec![],
            topologies: vec![],
            immunes: vec![],
            reflexes: vec![],
            heals: vec![],
            components: vec![],
            views: vec![],
            channels: vec![],
            sockets: vec![],
            observables: vec![],
            witnesses: vec![],
            upstreams: vec![],
            cors_policies: vec![],
            caches: vec![],
            credentials: vec![],
            savants: vec![],
            documents: vec![],
            deliveries: vec![],
            synths: vec![],
            scopes: vec![],
            effects: vec![],
            modules: vec![],
        };

        let pricing = PricingModel::default_sonnet();
        let report = estimate_program(&ir, &pricing);
        assert_eq!(report.total_tokens, 0);
        assert_eq!(report.estimated_cost_usd, 0.0);
        assert!(report.flows.is_empty());
    }

    #[test]
    fn estimate_program_single_flow() {
        use crate::ir_nodes::*;

        let flow = IRFlow {
            node_type: "Flow", source_line: 1, source_column: 1,
            name: "Analyze".into(),
            parameters: vec![], return_type_name: "".into(),
            return_type_generic: "".into(), return_type_optional: false,
            steps: vec![
                IRFlowNode::Step(IRStep {
                    node_type: "Step", source_line: 2, source_column: 1,
                    name: "gather".into(), persona_ref: "".into(),
                    given: "".into(), ask: "gather data".into(),
                    use_tool: None, probe: None, reason: None, weave: None,
                    output_type: "".into(), confidence_floor: None,
                    navigate_ref: "".into(), apply_ref: "".into(),
                    pix_ops: Vec::new(),
                    guards: Vec::new(),
                    requires_context: None,                    now_tz: None,                    body: vec![],
                }),
                IRFlowNode::Reason(IRReasonStep {
                    node_type: "Reason", source_line: 3, source_column: 1,
                    strategy: "deductive".into(), target: "conclusion".into(),
                    given: String::new(), ask: String::new(), depth: None,
                }),
            ],
            edges: vec![],
            execution_levels: vec![],
        };

        let ir = IRProgram {
            node_type: "Program",
            source_line: 0, source_column: 0,
            personas: vec![], contexts: vec![], anchors: vec![],
            tools: vec![], memories: vec![], types: vec![],
            flows: vec![flow], runs: vec![], imports: vec![],
            agents: vec![], shields: vec![], windows: vec![], budgets: vec![], daemons: vec![],
            ots_specs: vec![], pix_specs: vec![], ledger_specs: vec![], corpus_specs: vec![],
            psyche_specs: vec![], mandate_specs: vec![],
            lambda_data_specs: vec![], compute_specs: vec![],
            axonstore_specs: vec![], endpoints: vec![],
            extensions: vec![],
            dataspace_specs: vec![],
            notifications: vec![],
            resources: vec![],
            fabrics: vec![],
            manifests: vec![],
            observations: vec![],
            intention_tree: None,
            reconciles: vec![],
            leases: vec![],
            ensembles: vec![],
            sessions: vec![],
            topologies: vec![],
            immunes: vec![],
            reflexes: vec![],
            heals: vec![],
            components: vec![],
            views: vec![],
            channels: vec![],
            sockets: vec![],
            observables: vec![],
            witnesses: vec![],
            upstreams: vec![],
            cors_policies: vec![],
            caches: vec![],
            credentials: vec![],
            savants: vec![],
            documents: vec![],
            deliveries: vec![],
            synths: vec![],
            scopes: vec![],
            effects: vec![],
            modules: vec![],
        };

        let pricing = PricingModel::default_sonnet();
        let report = estimate_program(&ir, &pricing);

        assert_eq!(report.flows.len(), 1);
        assert_eq!(report.flows[0].flow_name, "Analyze");
        assert_eq!(report.flows[0].total_steps, 2);

        // Ask: 800 input + 400 output, Reason: 1200 input + 800 output
        assert_eq!(report.total_input_tokens, 800 + 1200);
        assert_eq!(report.total_output_tokens, 400 + 800);
        assert_eq!(report.total_tokens, 3200);
        assert!(report.estimated_cost_usd > 0.0);
    }

    #[test]
    fn format_text_contains_flow_name() {
        use crate::ir_nodes::*;

        let flow = IRFlow {
            node_type: "Flow", source_line: 1, source_column: 1,
            name: "TestFlow".into(),
            parameters: vec![], return_type_name: "".into(),
            return_type_generic: "".into(), return_type_optional: false,
            steps: vec![
                IRFlowNode::Step(IRStep {
                    node_type: "Step", source_line: 2, source_column: 1,
                    name: "s1".into(), persona_ref: "".into(),
                    given: "".into(), ask: "do".into(),
                    use_tool: None, probe: None, reason: None, weave: None,
                    output_type: "".into(), confidence_floor: None,
                    navigate_ref: "".into(), apply_ref: "".into(),
                    pix_ops: Vec::new(),
                    guards: Vec::new(),
                    requires_context: None,                    now_tz: None,                    body: vec![],
                }),
            ],
            edges: vec![],
            execution_levels: vec![],
        };

        let ir = IRProgram {
            node_type: "Program", source_line: 0, source_column: 0,
            personas: vec![], contexts: vec![], anchors: vec![],
            tools: vec![], memories: vec![], types: vec![],
            flows: vec![flow], runs: vec![], imports: vec![],
            agents: vec![], shields: vec![], windows: vec![], budgets: vec![], daemons: vec![],
            ots_specs: vec![], pix_specs: vec![], ledger_specs: vec![], corpus_specs: vec![],
            psyche_specs: vec![], mandate_specs: vec![],
            lambda_data_specs: vec![], compute_specs: vec![],
            axonstore_specs: vec![], endpoints: vec![],
            extensions: vec![],
            dataspace_specs: vec![],
            notifications: vec![],
            resources: vec![],
            fabrics: vec![],
            manifests: vec![],
            observations: vec![],
            intention_tree: None,
            reconciles: vec![],
            leases: vec![],
            ensembles: vec![],
            sessions: vec![],
            topologies: vec![],
            immunes: vec![],
            reflexes: vec![],
            heals: vec![],
            components: vec![],
            views: vec![],
            channels: vec![],
            sockets: vec![],
            observables: vec![],
            witnesses: vec![],
            upstreams: vec![],
            cors_policies: vec![],
            caches: vec![],
            credentials: vec![],
            savants: vec![],
            documents: vec![],
            deliveries: vec![],
            synths: vec![],
            scopes: vec![],
            effects: vec![],
            modules: vec![],
        };

        let pricing = PricingModel::default_sonnet();
        let report = estimate_program(&ir, &pricing);
        let text = format_text(&report);

        assert!(text.contains("TestFlow"));
        assert!(text.contains("claude-sonnet-4"));
        assert!(text.contains("Estimated cost:"));
        assert!(text.contains("$"));
    }

    #[test]
    fn report_serializes_to_json() {
        let pricing = PricingModel::default_sonnet();
        let report = CostReport {
            pricing: pricing.clone(),
            flows: vec![],
            total_input_tokens: 1000,
            total_output_tokens: 500,
            total_tokens: 1500,
            estimated_cost_usd: pricing.compute_cost(1000, 500),
        };

        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"total_tokens\":1500"));
        assert!(json.contains("\"claude-sonnet-4\""));
    }

    #[test]
    fn default_estimates_nonzero_for_llm_steps() {
        for kind in &[StepKind::Ask, StepKind::ToolCall, StepKind::Reason,
                      StepKind::Probe, StepKind::Validate, StepKind::Refine,
                      StepKind::Weave, StepKind::MultiAgent, StepKind::Cognitive] {
            let est = default_estimate(*kind);
            assert!(est.input_tokens > 0, "{:?} should have nonzero input", kind);
            assert!(est.output_tokens > 0, "{:?} should have nonzero output", kind);
        }
    }

    #[test]
    fn control_and_parallel_zero_cost() {
        let est_ctrl = default_estimate(StepKind::Control);
        assert_eq!(est_ctrl.input_tokens, 0);
        assert_eq!(est_ctrl.output_tokens, 0);

        let est_par = default_estimate(StepKind::Parallel);
        assert_eq!(est_par.input_tokens, 0);
        assert_eq!(est_par.output_tokens, 0);
    }

    #[test]
    fn memory_steps_low_cost() {
        let est = default_estimate(StepKind::Memory);
        assert!(est.input_tokens <= 200);
        assert!(est.output_tokens <= 100);
    }
}
