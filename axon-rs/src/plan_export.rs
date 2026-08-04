//! Execution plan export — JSONB-compatible structured output.
//!
//! Produces a self-describing JSON representation of the execution plan
//! before execution begins. Designed for external consumption by:
//!   - PostgreSQL JSONB columns (stable schema, predictable paths)
//!   - CI/CD pipelines (plan review before execution)
//!   - Dashboards and visualization tools
//!   - External orchestrators
//!
//! Every exported JSON document includes a `_schema` header with:
//!   - `type`: document type identifier (e.g., "axon.plan", "axon.report")
//!   - `version`: schema version for backwards compatibility
//!   - `axon_version`: AXON compiler version that produced it
//!
//! JSONB path conventions:
//!   $.units[*].flow_name          — all flow names
//!   $.units[*].steps[*].name      — all step names
//!   $.dependencies.parallel_groups — parallelizable step groups
//!   $.tools.registered[*].name    — all registered tool names

use serde::Serialize;

// ── Schema metadata ────────────────────────────────────────────────────────

/// Schema metadata header — included in every exported JSON document.
#[derive(Debug, Clone, Serialize)]
pub struct SchemaHeader {
    /// Document type identifier.
    #[serde(rename = "type")]
    pub doc_type: String,
    /// Schema version (semver).
    pub version: String,
    /// AXON version that produced this document.
    pub axon_version: String,
}

impl SchemaHeader {
    pub fn new(doc_type: &str) -> Self {
        SchemaHeader {
            doc_type: doc_type.to_string(),
            version: "1.0.0".to_string(),
            axon_version: crate::version::AXON_VERSION.to_string(),
        }
    }
}

// ── Plan export structures ─────────────────────────────────────────────────

/// Exported execution plan — the full pre-execution view.
#[derive(Debug, Clone, Serialize)]
pub struct PlanExport {
    pub _schema: SchemaHeader,
    pub source_file: String,
    pub backend: String,
    pub units: Vec<PlanUnit>,
    pub tools: PlanTools,
    pub dependencies: PlanDependencies,
    pub summary: PlanSummary,
}

/// A unit in the exported plan.
#[derive(Debug, Clone, Serialize)]
pub struct PlanUnit {
    pub flow_name: String,
    pub persona_name: String,
    pub context_name: String,
    pub effort: String,
    pub anchor_count: usize,
    pub anchors: Vec<String>,
    pub steps: Vec<PlanStep>,
}

/// A step in the exported plan.
#[derive(Debug, Clone, Serialize)]
pub struct PlanStep {
    pub name: String,
    pub step_type: String,
    pub prompt_preview: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_argument: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_expression: Option<String>,
    pub depends_on: Vec<String>,
    pub is_root: bool,
}

/// Tool registry summary in the exported plan.
#[derive(Debug, Clone, Serialize)]
pub struct PlanTools {
    pub total: usize,
    pub builtin: Vec<String>,
    pub program: Vec<String>,
    pub registered: Vec<PlanToolEntry>,
}

/// A tool entry in the exported plan.
#[derive(Debug, Clone, Serialize)]
pub struct PlanToolEntry {
    pub name: String,
    pub provider: String,
    pub source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub output_schema: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub effect_row: Vec<String>,
}

/// Dependency analysis summary in the exported plan.
#[derive(Debug, Clone, Serialize)]
pub struct PlanDependencies {
    pub max_depth: usize,
    pub parallel_groups: Vec<Vec<String>>,
    pub unresolved_refs: Vec<UnresolvedRef>,
}

/// An unresolved variable reference.
#[derive(Debug, Clone, Serialize)]
pub struct UnresolvedRef {
    pub step: String,
    pub variable: String,
}

/// Plan-level summary.
#[derive(Debug, Clone, Serialize)]
pub struct PlanSummary {
    pub total_units: usize,
    pub total_steps: usize,
    pub total_anchors: usize,
    pub total_tools: usize,
    pub has_parallel_steps: bool,
    pub has_unresolved_refs: bool,
}

// ── Plan builder ───────────────────────────────────────────────────────────

/// Build a plan export from execution components.
pub struct PlanBuilder;

impl PlanBuilder {
    /// Build the plan export from components.
    pub fn build(
        source_file: &str,
        backend: &str,
        units: &[PlanUnit],
        tools: PlanTools,
        deps: PlanDependencies,
    ) -> PlanExport {
        let total_steps: usize = units.iter().map(|u| u.steps.len()).sum();
        let total_anchors: usize = units.iter().map(|u| u.anchor_count).sum();

        PlanExport {
            _schema: SchemaHeader::new("axon.plan"),
            source_file: source_file.to_string(),
            backend: backend.to_string(),
            units: units.to_vec(),
            tools: tools.clone(),
            dependencies: deps.clone(),
            summary: PlanSummary {
                total_units: units.len(),
                total_steps,
                total_anchors,
                total_tools: tools.total,
                has_parallel_steps: !deps.parallel_groups.is_empty(),
                has_unresolved_refs: !deps.unresolved_refs.is_empty(),
            },
        }
    }

    /// Serialize to JSON string.
    pub fn to_json(plan: &PlanExport) -> String {
        serde_json::to_string_pretty(plan).unwrap_or_else(|e| {
            format!("{{\"error\": \"serialization failed: {e}\"}}")
        })
    }
}

// ── JSONB path query ───────────────────────────────────────────────────────

/// Simple JSONB-style path query on a serde_json::Value.
///
/// Supports a subset of JSONPath:
///   $.field           — object field access
///   $.field[N]        — array index access
///   $.field[*]        — array wildcard (returns all elements)
///   $.a.b.c           — nested field access
///
/// Returns a Vec of matched values.
pub fn jsonb_query(value: &serde_json::Value, path: &str) -> Vec<serde_json::Value> {
    let path = path.strip_prefix("$.").unwrap_or(path.strip_prefix('$').unwrap_or(path));
    if path.is_empty() {
        return vec![value.clone()];
    }

    let segments = parse_path(path);
    let mut current = vec![value.clone()];

    for seg in &segments {
        let mut next = Vec::new();
        for val in &current {
            match seg {
                PathSegment::Field(name) => {
                    if let Some(v) = val.get(name.as_str()) {
                        next.push(v.clone());
                    }
                }
                PathSegment::Index(idx) => {
                    if let Some(v) = val.get(*idx) {
                        next.push(v.clone());
                    }
                }
                PathSegment::Wildcard => {
                    if let Some(arr) = val.as_array() {
                        next.extend(arr.iter().cloned());
                    }
                }
            }
        }
        current = next;
    }

    current
}

#[derive(Debug)]
enum PathSegment {
    Field(String),
    Index(usize),
    Wildcard,
}

fn parse_path(path: &str) -> Vec<PathSegment> {
    let mut segments = Vec::new();
    let mut remaining = path;

    while !remaining.is_empty() {
        // Strip leading dot
        remaining = remaining.strip_prefix('.').unwrap_or(remaining);
        if remaining.is_empty() {
            break;
        }

        // Check for bracket notation
        if let Some(bracket_start) = remaining.find('[') {
            // Field before bracket
            let field = &remaining[..bracket_start];
            if !field.is_empty() {
                segments.push(PathSegment::Field(field.to_string()));
            }

            // Parse bracket content
            if let Some(bracket_end) = remaining[bracket_start..].find(']') {
                let inner = &remaining[bracket_start + 1..bracket_start + bracket_end];
                if inner == "*" {
                    segments.push(PathSegment::Wildcard);
                } else if let Ok(idx) = inner.parse::<usize>() {
                    segments.push(PathSegment::Index(idx));
                }
                remaining = &remaining[bracket_start + bracket_end + 1..];
            } else {
                break;
            }
        } else {
            // Find next dot or end
            let end = remaining.find('.').unwrap_or(remaining.len());
            let field = &remaining[..end];
            if !field.is_empty() {
                segments.push(PathSegment::Field(field.to_string()));
            }
            remaining = &remaining[end..];
        }
    }

    segments
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_header_defaults() {
        let h = SchemaHeader::new("axon.plan");
        assert_eq!(h.doc_type, "axon.plan");
        assert_eq!(h.version, "1.0.0");
        assert!(!h.axon_version.is_empty());
    }

    #[test]
    fn plan_builder_empty() {
        let plan = PlanBuilder::build(
            "test.axon",
            "anthropic",
            &[],
            PlanTools {
                total: 2,
                builtin: vec!["Calculator".into(), "DateTimeTool".into()],
                program: vec![],
                registered: vec![],
            },
            PlanDependencies {
                max_depth: 0,
                parallel_groups: vec![],
                unresolved_refs: vec![],
            },
        );

        assert_eq!(plan._schema.doc_type, "axon.plan");
        assert_eq!(plan.source_file, "test.axon");
        assert_eq!(plan.summary.total_units, 0);
        assert_eq!(plan.summary.total_steps, 0);
        assert!(!plan.summary.has_parallel_steps);
    }

    #[test]
    fn plan_builder_with_units() {
        let units = vec![PlanUnit {
            flow_name: "Analyze".into(),
            persona_name: "Expert".into(),
            context_name: "Review".into(),
            effort: "high".into(),
            anchor_count: 1,
            anchors: vec!["NoHallucination".into()],
            steps: vec![
                PlanStep {
                    name: "Extract".into(),
                    step_type: "step".into(),
                    prompt_preview: "Extract entities".into(),
                    tool_argument: None,
                    memory_expression: None,
                    depends_on: vec![],
                    is_root: true,
                },
                PlanStep {
                    name: "Assess".into(),
                    step_type: "step".into(),
                    prompt_preview: "Assess ${Extract}".into(),
                    tool_argument: None,
                    memory_expression: None,
                    depends_on: vec!["Extract".into()],
                    is_root: false,
                },
            ],
        }];

        let plan = PlanBuilder::build(
            "contract.axon",
            "anthropic",
            &units,
            PlanTools {
                total: 2,
                builtin: vec!["Calculator".into()],
                program: vec![],
                registered: vec![],
            },
            PlanDependencies {
                max_depth: 1,
                parallel_groups: vec![],
                unresolved_refs: vec![],
            },
        );

        assert_eq!(plan.summary.total_units, 1);
        assert_eq!(plan.summary.total_steps, 2);
        assert_eq!(plan.summary.total_anchors, 1);
    }

    #[test]
    fn plan_serializes_to_json() {
        let plan = PlanBuilder::build(
            "test.axon",
            "anthropic",
            &[],
            PlanTools { total: 0, builtin: vec![], program: vec![], registered: vec![] },
            PlanDependencies { max_depth: 0, parallel_groups: vec![], unresolved_refs: vec![] },
        );
        let json = PlanBuilder::to_json(&plan);
        assert!(json.contains("\"_schema\""));
        assert!(json.contains("\"axon.plan\""));
        assert!(json.contains("\"version\""));
    }

    #[test]
    fn plan_json_has_schema_header() {
        let plan = PlanBuilder::build(
            "test.axon",
            "anthropic",
            &[],
            PlanTools { total: 0, builtin: vec![], program: vec![], registered: vec![] },
            PlanDependencies { max_depth: 0, parallel_groups: vec![], unresolved_refs: vec![] },
        );
        let json = PlanBuilder::to_json(&plan);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["_schema"]["type"], "axon.plan");
        assert_eq!(parsed["_schema"]["version"], "1.0.0");
        assert!(parsed["_schema"]["axon_version"].is_string());
    }

    // ── JSONB path query tests ─────────────────────────────────────

    #[test]
    fn jsonb_query_simple_field() {
        let val: serde_json::Value = serde_json::json!({"name": "test", "version": 1});
        let results = jsonb_query(&val, "$.name");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "test");
    }

    #[test]
    fn jsonb_query_nested_field() {
        let val: serde_json::Value = serde_json::json!({"a": {"b": {"c": 42}}});
        let results = jsonb_query(&val, "$.a.b.c");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], 42);
    }

    #[test]
    fn jsonb_query_array_index() {
        let val: serde_json::Value = serde_json::json!({"items": [10, 20, 30]});
        let results = jsonb_query(&val, "$.items[1]");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], 20);
    }

    #[test]
    fn jsonb_query_wildcard() {
        let val: serde_json::Value = serde_json::json!({"units": [
            {"flow_name": "A"},
            {"flow_name": "B"},
        ]});
        let results = jsonb_query(&val, "$.units[*].flow_name");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], "A");
        assert_eq!(results[1], "B");
    }

    #[test]
    fn jsonb_query_missing_field() {
        let val: serde_json::Value = serde_json::json!({"name": "test"});
        let results = jsonb_query(&val, "$.nonexistent");
        assert!(results.is_empty());
    }

    #[test]
    fn jsonb_query_root() {
        let val: serde_json::Value = serde_json::json!(42);
        let results = jsonb_query(&val, "$");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], 42);
    }

    #[test]
    fn jsonb_query_nested_wildcard() {
        let val: serde_json::Value = serde_json::json!({
            "units": [
                {"steps": [{"name": "A"}, {"name": "B"}]},
                {"steps": [{"name": "C"}]},
            ]
        });
        let results = jsonb_query(&val, "$.units[*].steps[*].name");
        assert_eq!(results.len(), 3);
        assert_eq!(results[0], "A");
        assert_eq!(results[1], "B");
        assert_eq!(results[2], "C");
    }
}
