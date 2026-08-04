//! Execution output formats — structured report for programmatic integration.
//!
//! Provides `ExecutionReport` — a serde-serializable struct that captures
//! the full result of an AXON execution: units, steps, results, token usage,
//! timing from HookManager, anchor results, and conversation turns.
//!
//! Output formats:
//!   text (default) — human-readable colored terminal output
//!   json           — structured JSON to stdout for CI/CD, tooling, dashboards

use serde::Serialize;

use crate::hooks::HookManager;
use crate::plan_export::SchemaHeader;

// ── Output format enum ─────────────────────────────────────────────────────

/// Output format for execution results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
}

impl OutputFormat {
    /// Parse from CLI string. Returns None for invalid values.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "text" => Some(OutputFormat::Text),
            "json" => Some(OutputFormat::Json),
            _ => None,
        }
    }

    pub fn is_json(&self) -> bool {
        *self == OutputFormat::Json
    }
}

// ── Report structures ──────────────────────────────────────────────────────

/// A single step result within a unit report.
#[derive(Debug, Clone, Serialize)]
pub struct StepReport {
    pub name: String,
    pub step_type: String,
    pub result: String,
    pub duration_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub anchor_breaches: u32,
    pub chain_activations: u32,
    pub was_retried: bool,
}

/// A single execution unit report.
#[derive(Debug, Clone, Serialize)]
pub struct UnitReport {
    pub flow_name: String,
    pub persona_name: String,
    pub steps: Vec<StepReport>,
    pub duration_ms: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_anchor_breaches: u32,
    pub total_chain_activations: u32,
}

/// Top-level execution report — the full structured output.
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionReport {
    pub _schema: SchemaHeader,
    pub axon_version: String,
    pub source_file: String,
    pub backend: String,
    pub mode: String,
    pub success: bool,
    pub units: Vec<UnitReport>,
    pub summary: ExecutionSummary,
}

/// Aggregate summary across all units.
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionSummary {
    pub total_units: usize,
    pub total_steps: usize,
    pub total_duration_ms: u64,
    pub avg_step_duration_ms: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
    pub retried_steps: usize,
}

// ── Report builder ─────────────────────────────────────────────────────────

/// Accumulates step results during execution, then builds the final report.
pub struct ReportBuilder {
    source_file: String,
    backend: String,
    mode: String,
    unit_reports: Vec<UnitReport>,
    // In-flight unit tracking
    current_unit_steps: Vec<StepReport>,
    current_flow_name: String,
    current_persona_name: String,
}

impl ReportBuilder {
    pub fn new(source_file: &str, backend: &str, mode: &str) -> Self {
        ReportBuilder {
            source_file: source_file.to_string(),
            backend: backend.to_string(),
            mode: mode.to_string(),
            unit_reports: Vec::new(),
            current_unit_steps: Vec::new(),
            current_flow_name: String::new(),
            current_persona_name: String::new(),
        }
    }

    /// Signal the start of a unit.
    pub fn begin_unit(&mut self, flow_name: &str, persona_name: &str) {
        self.current_flow_name = flow_name.to_string();
        self.current_persona_name = persona_name.to_string();
        self.current_unit_steps.clear();
    }

    /// Record a step result.
    pub fn record_step(&mut self, step: StepReport) {
        self.current_unit_steps.push(step);
    }

    /// Finalize the current unit using metrics from HookManager.
    pub fn end_unit(&mut self, hooks: &HookManager) {
        let unit_metrics = hooks.unit_metrics();
        let um = unit_metrics.last();

        self.unit_reports.push(UnitReport {
            flow_name: self.current_flow_name.clone(),
            persona_name: self.current_persona_name.clone(),
            steps: std::mem::take(&mut self.current_unit_steps),
            duration_ms: um.map(|u| u.duration_ms).unwrap_or(0),
            total_input_tokens: um.map(|u| u.total_input_tokens).unwrap_or(0),
            total_output_tokens: um.map(|u| u.total_output_tokens).unwrap_or(0),
            total_anchor_breaches: um.map(|u| u.total_anchor_breaches).unwrap_or(0),
            total_chain_activations: um.map(|u| u.total_chain_activations).unwrap_or(0),
        });
    }

    /// Build the final report.
    pub fn build(self, success: bool, hooks: &HookManager) -> ExecutionReport {
        ExecutionReport {
            _schema: SchemaHeader::new("axon.report"),
            axon_version: crate::version::AXON_VERSION.to_string(),
            source_file: self.source_file,
            backend: self.backend,
            mode: self.mode,
            success,
            units: self.unit_reports,
            summary: ExecutionSummary {
                total_units: hooks.unit_metrics().len(),
                total_steps: hooks.total_steps(),
                total_duration_ms: hooks.total_duration_ms(),
                avg_step_duration_ms: hooks.avg_step_duration_ms(),
                total_input_tokens: hooks.total_input_tokens(),
                total_output_tokens: hooks.total_output_tokens(),
                total_tokens: hooks.total_input_tokens() + hooks.total_output_tokens(),
                retried_steps: hooks.retried_steps(),
            },
        }
    }

    /// Serialize the report to JSON string.
    pub fn to_json(report: &ExecutionReport) -> String {
        serde_json::to_string_pretty(report).unwrap_or_else(|e| {
            format!("{{\"error\": \"serialization failed: {e}\"}}")
        })
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::HookManager;

    #[test]
    fn output_format_parsing() {
        assert_eq!(OutputFormat::from_str("text"), Some(OutputFormat::Text));
        assert_eq!(OutputFormat::from_str("json"), Some(OutputFormat::Json));
        assert_eq!(OutputFormat::from_str("xml"), None);
        assert_eq!(OutputFormat::from_str(""), None);
    }

    #[test]
    fn output_format_is_json() {
        assert!(!OutputFormat::Text.is_json());
        assert!(OutputFormat::Json.is_json());
    }

    #[test]
    fn report_builder_empty() {
        let hooks = HookManager::new();
        let rb = ReportBuilder::new("test.axon", "anthropic", "stub");
        let report = rb.build(true, &hooks);

        assert_eq!(report.source_file, "test.axon");
        assert_eq!(report.backend, "anthropic");
        assert_eq!(report.mode, "stub");
        assert!(report.success);
        assert!(report.units.is_empty());
        assert_eq!(report.summary.total_units, 0);
        assert_eq!(report.summary.total_steps, 0);
    }

    #[test]
    fn report_builder_with_steps() {
        let mut hooks = HookManager::new();
        let mut rb = ReportBuilder::new("demo.axon", "openai", "real");

        hooks.on_unit_start("Analyze", "Expert");
        rb.begin_unit("Analyze", "Expert");

        hooks.on_step_start("Gather", "step");
        hooks.on_step_end(100, 50, 0, 0, false);
        rb.record_step(StepReport {
            name: "Gather".into(),
            step_type: "step".into(),
            result: "gathered data".into(),
            duration_ms: 0,
            input_tokens: 100,
            output_tokens: 50,
            anchor_breaches: 0,
            chain_activations: 0,
            was_retried: false,
        });

        hooks.on_step_start("Summarize", "step");
        hooks.on_step_end(200, 100, 1, 0, true);
        rb.record_step(StepReport {
            name: "Summarize".into(),
            step_type: "step".into(),
            result: "summary text".into(),
            duration_ms: 0,
            input_tokens: 200,
            output_tokens: 100,
            anchor_breaches: 1,
            chain_activations: 0,
            was_retried: true,
        });

        hooks.on_unit_end();
        rb.end_unit(&hooks);

        let report = rb.build(true, &hooks);
        assert_eq!(report.units.len(), 1);
        assert_eq!(report.units[0].flow_name, "Analyze");
        assert_eq!(report.units[0].steps.len(), 2);
        assert_eq!(report.units[0].steps[0].name, "Gather");
        assert_eq!(report.units[0].steps[1].name, "Summarize");
        assert!(report.units[0].steps[1].was_retried);
        assert_eq!(report.summary.total_steps, 2);
        assert_eq!(report.summary.total_input_tokens, 300);
        assert_eq!(report.summary.total_output_tokens, 150);
        assert_eq!(report.summary.total_tokens, 450);
        assert_eq!(report.summary.retried_steps, 1);
    }

    #[test]
    fn report_serializes_to_json() {
        let hooks = HookManager::new();
        let rb = ReportBuilder::new("test.axon", "anthropic", "stub");
        let report = rb.build(true, &hooks);
        let json = ReportBuilder::to_json(&report);

        assert!(json.contains("\"axon_version\""));
        assert!(json.contains("\"source_file\""));
        assert!(json.contains("\"test.axon\""));
        assert!(json.contains("\"summary\""));
        assert!(json.contains("\"total_steps\""));
    }

    #[test]
    fn report_multiple_units() {
        let mut hooks = HookManager::new();
        let mut rb = ReportBuilder::new("multi.axon", "gemini", "real");

        // Unit 1
        hooks.on_unit_start("Flow1", "P1");
        rb.begin_unit("Flow1", "P1");
        hooks.on_step_start("S1", "step");
        hooks.on_step_end(10, 5, 0, 0, false);
        rb.record_step(StepReport {
            name: "S1".into(),
            step_type: "step".into(),
            result: "r1".into(),
            duration_ms: 0,
            input_tokens: 10,
            output_tokens: 5,
            anchor_breaches: 0,
            chain_activations: 0,
            was_retried: false,
        });
        hooks.on_unit_end();
        rb.end_unit(&hooks);

        // Unit 2
        hooks.on_unit_start("Flow2", "P2");
        rb.begin_unit("Flow2", "P2");
        hooks.on_step_start("S2", "step");
        hooks.on_step_end(20, 10, 0, 0, false);
        rb.record_step(StepReport {
            name: "S2".into(),
            step_type: "step".into(),
            result: "r2".into(),
            duration_ms: 0,
            input_tokens: 20,
            output_tokens: 10,
            anchor_breaches: 0,
            chain_activations: 0,
            was_retried: false,
        });
        hooks.on_unit_end();
        rb.end_unit(&hooks);

        let report = rb.build(true, &hooks);
        assert_eq!(report.units.len(), 2);
        assert_eq!(report.summary.total_units, 2);
        assert_eq!(report.summary.total_tokens, 45);
    }

    #[test]
    fn report_json_round_trip() {
        let mut hooks = HookManager::new();
        let mut rb = ReportBuilder::new("rt.axon", "anthropic", "stub");

        hooks.on_unit_start("F", "P");
        rb.begin_unit("F", "P");
        hooks.on_step_start("S", "step");
        hooks.on_step_end(42, 21, 0, 0, false);
        rb.record_step(StepReport {
            name: "S".into(),
            step_type: "step".into(),
            result: "hello world".into(),
            duration_ms: 0,
            input_tokens: 42,
            output_tokens: 21,
            anchor_breaches: 0,
            chain_activations: 0,
            was_retried: false,
        });
        hooks.on_unit_end();
        rb.end_unit(&hooks);

        let report = rb.build(true, &hooks);
        let json = ReportBuilder::to_json(&report);

        // Parse back and verify key fields
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["source_file"], "rt.axon");
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["units"][0]["flow_name"], "F");
        assert_eq!(parsed["units"][0]["steps"][0]["result"], "hello world");
        assert_eq!(parsed["summary"]["total_tokens"], 63);
    }
}
