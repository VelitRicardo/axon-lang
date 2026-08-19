//! Step dependency analysis — variable-based dependency graph between steps.
//!
//! Analyzes `$variable` / `${variable}` references in step prompts to build
//! a dependency graph. This enables:
//!   - Detection of which steps can potentially run in parallel
//!   - Validation that referenced variables are actually produced
//!   - Execution plan visualization with dependency chains
//!
//! Built-in variables ($result, $step_name, $flow_name, etc.) are excluded
//! from dependency analysis as they are runtime-injected, not step-produced.

use std::collections::{HashMap, HashSet};

// ── Built-in variables (not produced by steps) ─────────────────────────────

const BUILTIN_VARS: &[&str] = &[
    "result",
    "step_name",
    "step_type",
    "flow_name",
    "persona_name",
    "unit_index",
    "step_index",
];

fn is_builtin(var: &str) -> bool {
    BUILTIN_VARS.contains(&var)
}

// ── Variable extraction ────────────────────────────────────────────────────

/// Extract all variable references from a string.
/// Returns the set of variable names referenced via $name or ${name}.
pub fn extract_refs(text: &str) -> HashSet<String> {
    let mut refs = HashSet::new();
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'{' {
                // ${name} form
                if let Some(close) = text[i + 2..].find('}') {
                    let var_name = &text[i + 2..i + 2 + close];
                    if !var_name.is_empty() {
                        refs.insert(var_name.to_string());
                    }
                    i += 3 + close;
                    continue;
                }
            } else if bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_' {
                // $name form
                let start = i + 1;
                let mut end = start;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
                {
                    end += 1;
                }
                let var_name = &text[start..end];
                refs.insert(var_name.to_string());
                i = end;
                continue;
            }
        }
        i += 1;
    }

    refs
}

/// v2.11.0 — augment a step's dependency-analysis `argument` with the step
/// references carried by a `use Tool(k = v)` call's keyword arguments.
///
/// [`analyze`] only scans `user_prompt` + `argument` for `${name}` references.
/// A keyword-form tool call carries its values in `named_args`, NOT in the
/// single-`on <arg>` string, so without this its data-dependencies are
/// invisible: the scheduler classifies the call as a root and co-schedules it
/// in the SAME wave as the steps it consumes — whose results are therefore
/// absent from the call's pre-wave context snapshot, so the value resolves to
/// empty (v2.10.0 reference) or to the literal name (pre-v2.10.0). This was the unifying
/// root cause behind a multi-arg `use Tool` whose source steps are independent.
///
/// Each `named_args` entry is `(name, value, value_kind)`:
/// - `"reference"` — the value names the producing step directly (`Extract` or
///   `Extract.output`; the `.output` suffix maps to the step-name key, mirroring
///   the runtime resolver in `exec_context`).
/// - any other kind (`"literal"`) — the value may interpolate one via `${…}`.
///
/// We append the referenced names as `${name}` tokens to `base`, GATED to names
/// that are real steps (`step_names`) so a bare flow-param reference adds no
/// spurious dependency. The result feeds [`analyze`] as the step's `argument`;
/// `extract_refs` then rediscovers them as ordinary dependency edges.
pub fn use_tool_analysis_argument(
    base: &str,
    named_args: &[(String, String, String)],
    step_names: &HashSet<&str>,
) -> String {
    let mut arg = base.to_string();
    for (_name, value, kind) in named_args {
        if kind == "reference" {
            let dep = value.strip_suffix(".output").unwrap_or(value);
            if step_names.contains(dep) {
                arg.push_str(" ${");
                arg.push_str(dep);
                arg.push('}');
            }
        } else {
            for r in extract_refs(value) {
                if step_names.contains(r.as_str()) {
                    arg.push_str(" ${");
                    arg.push_str(&r);
                    arg.push('}');
                }
            }
        }
    }
    arg
}

// ── Step info for analysis ─────────────────────────────────────────────────

/// Minimal step representation for dependency analysis.
#[derive(Debug, Clone)]
pub struct StepInfo {
    pub name: String,
    pub step_type: String,
    pub user_prompt: String,
    /// For tool/memory steps: the argument expression.
    pub argument: String,
}

// ── Dependency analysis result ─────────────────────────────────────────────

/// Analysis result for a single step.
#[derive(Debug, Clone)]
pub struct StepDependency {
    /// Step name.
    pub name: String,
    /// Step type.
    pub step_type: String,
    /// Steps this step depends on (via variable references).
    pub depends_on: Vec<String>,
    /// All variable references found (including builtins).
    pub all_refs: Vec<String>,
    /// Variable references that are step-produced (non-builtin).
    pub step_refs: Vec<String>,
    /// Whether this step has no step dependencies (can run first).
    pub is_root: bool,
}

/// Full dependency graph for a unit's steps.
#[derive(Debug)]
pub struct DependencyGraph {
    pub steps: Vec<StepDependency>,
    /// Steps that can potentially run in parallel (no mutual dependencies).
    pub parallel_groups: Vec<Vec<String>>,
    /// Steps that reference undefined variables (not produced by any prior step).
    pub unresolved_refs: Vec<(String, String)>,
    /// Maximum depth of the dependency chain.
    pub max_depth: usize,
}

// ── Analysis ───────────────────────────────────────────────────────────────

/// Analyze dependencies between steps in a unit.
pub fn analyze(steps: &[StepInfo]) -> DependencyGraph {
    // 1. Build the set of step names (these are the "producers")
    let step_names: HashSet<&str> = steps.iter().map(|s| s.name.as_str()).collect();

    // 2. For each step, extract variable refs and resolve dependencies
    let mut deps: Vec<StepDependency> = Vec::new();
    let mut unresolved: Vec<(String, String)> = Vec::new();

    for step in steps {
        // Scan both user_prompt and argument for references
        let mut all_refs: HashSet<String> = extract_refs(&step.user_prompt);
        if !step.argument.is_empty() {
            all_refs.extend(extract_refs(&step.argument));
        }

        let mut step_refs: Vec<String> = Vec::new();
        let mut depends_on: Vec<String> = Vec::new();

        for r in &all_refs {
            if is_builtin(r) {
                continue;
            }
            if step_names.contains(r.as_str()) {
                step_refs.push(r.clone());
                depends_on.push(r.clone());
            } else {
                unresolved.push((step.name.clone(), r.clone()));
            }
        }

        depends_on.sort();
        depends_on.dedup();
        step_refs.sort();

        let mut all_refs_sorted: Vec<String> = all_refs.into_iter().collect();
        all_refs_sorted.sort();

        deps.push(StepDependency {
            name: step.name.clone(),
            step_type: step.step_type.clone(),
            is_root: depends_on.is_empty(),
            depends_on,
            all_refs: all_refs_sorted,
            step_refs,
        });
    }

    // 3. Detect parallel groups (steps with no mutual dependencies)
    let parallel_groups = find_parallel_groups(&deps);

    // 4. Calculate max depth
    let max_depth = calculate_max_depth(&deps);

    DependencyGraph {
        steps: deps,
        parallel_groups,
        unresolved_refs: unresolved,
        max_depth,
    }
}

/// Find groups of steps that can potentially execute in parallel.
/// Steps at the same depth level with no mutual dependencies form a group.
fn find_parallel_groups(deps: &[StepDependency]) -> Vec<Vec<String>> {
    // Build transitive dependency sets via depth calculation
    let dep_map: HashMap<&str, &StepDependency> =
        deps.iter().map(|d| (d.name.as_str(), d)).collect();

    // Calculate depth for each step
    let mut depth_cache: HashMap<String, usize> = HashMap::new();
    fn step_depth(
        name: &str,
        dep_map: &HashMap<&str, &StepDependency>,
        cache: &mut HashMap<String, usize>,
    ) -> usize {
        if let Some(&cached) = cache.get(name) {
            return cached;
        }
        let d = match dep_map.get(name) {
            Some(d) => d,
            None => return 0,
        };
        if d.depends_on.is_empty() {
            cache.insert(name.to_string(), 0);
            return 0;
        }
        let max_child = d
            .depends_on
            .iter()
            .map(|dep| step_depth(dep, dep_map, cache))
            .max()
            .unwrap_or(0);
        let result = max_child + 1;
        cache.insert(name.to_string(), result);
        result
    }

    for d in deps {
        step_depth(&d.name, &dep_map, &mut depth_cache);
    }

    // Group steps by depth level
    let mut by_depth: HashMap<usize, Vec<String>> = HashMap::new();
    for d in deps {
        let depth = depth_cache.get(&d.name).copied().unwrap_or(0);
        by_depth.entry(depth).or_default().push(d.name.clone());
    }

    // Return only groups with more than one step (actual parallelism)
    let mut groups: Vec<Vec<String>> = by_depth
        .into_values()
        .filter(|g| g.len() > 1)
        .collect();
    groups.sort_by_key(|g| g[0].clone());
    groups
}

/// Calculate the maximum dependency chain depth.
fn calculate_max_depth(deps: &[StepDependency]) -> usize {
    let dep_map: HashMap<&str, &StepDependency> =
        deps.iter().map(|d| (d.name.as_str(), d)).collect();

    fn depth(
        name: &str,
        dep_map: &HashMap<&str, &StepDependency>,
        cache: &mut HashMap<String, usize>,
    ) -> usize {
        if let Some(&cached) = cache.get(name) {
            return cached;
        }
        let d = match dep_map.get(name) {
            Some(d) => d,
            None => return 0,
        };
        if d.depends_on.is_empty() {
            cache.insert(name.to_string(), 0);
            return 0;
        }
        let max_child = d
            .depends_on
            .iter()
            .map(|dep| depth(dep, dep_map, cache))
            .max()
            .unwrap_or(0);
        let result = max_child + 1;
        cache.insert(name.to_string(), result);
        result
    }

    let mut cache = HashMap::new();
    deps.iter()
        .map(|d| depth(&d.name, &dep_map, &mut cache))
        .max()
        .unwrap_or(0)
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_refs_dollar_name() {
        let refs = extract_refs("Use $result from $Analyze");
        assert!(refs.contains("result"));
        assert!(refs.contains("Analyze"));
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn extract_refs_braced() {
        let refs = extract_refs("Given ${Extract} and ${Validate}");
        assert!(refs.contains("Extract"));
        assert!(refs.contains("Validate"));
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn extract_refs_mixed() {
        let refs = extract_refs("$result is ${Analyze} plus $flow_name");
        assert!(refs.contains("result"));
        assert!(refs.contains("Analyze"));
        assert!(refs.contains("flow_name"));
        assert_eq!(refs.len(), 3);
    }

    #[test]
    fn extract_refs_no_vars() {
        let refs = extract_refs("plain text with no variables");
        assert!(refs.is_empty());
    }

    #[test]
    fn extract_refs_dollar_at_end() {
        let refs = extract_refs("trailing $");
        assert!(refs.is_empty());
    }

    #[test]
    fn analyze_independent_steps() {
        let steps = vec![
            StepInfo {
                name: "A".into(),
                step_type: "step".into(),
                user_prompt: "Do task A".into(),
                argument: String::new(),
            },
            StepInfo {
                name: "B".into(),
                step_type: "step".into(),
                user_prompt: "Do task B".into(),
                argument: String::new(),
            },
        ];

        let graph = analyze(&steps);
        assert_eq!(graph.steps.len(), 2);
        assert!(graph.steps[0].is_root);
        assert!(graph.steps[1].is_root);
        assert_eq!(graph.max_depth, 0);
        // Both independent → one parallel group
        assert_eq!(graph.parallel_groups.len(), 1);
        assert_eq!(graph.parallel_groups[0].len(), 2);
    }

    #[test]
    fn analyze_linear_chain() {
        let steps = vec![
            StepInfo {
                name: "Extract".into(),
                step_type: "step".into(),
                user_prompt: "Extract entities".into(),
                argument: String::new(),
            },
            StepInfo {
                name: "Analyze".into(),
                step_type: "step".into(),
                user_prompt: "Analyze ${Extract}".into(),
                argument: String::new(),
            },
            StepInfo {
                name: "Report".into(),
                step_type: "step".into(),
                user_prompt: "Report on ${Analyze}".into(),
                argument: String::new(),
            },
        ];

        let graph = analyze(&steps);

        // Extract is root
        assert!(graph.steps[0].is_root);
        assert!(graph.steps[0].depends_on.is_empty());

        // Analyze depends on Extract
        assert!(!graph.steps[1].is_root);
        assert_eq!(graph.steps[1].depends_on, vec!["Extract"]);

        // Report depends on Analyze
        assert!(!graph.steps[2].is_root);
        assert_eq!(graph.steps[2].depends_on, vec!["Analyze"]);

        // Max depth is 2 (Extract→Analyze→Report)
        assert_eq!(graph.max_depth, 2);

        // No parallel groups (all sequential)
        assert!(graph.parallel_groups.is_empty());
    }

    #[test]
    fn analyze_diamond_pattern() {
        // A → B, A → C, B+C → D
        let steps = vec![
            StepInfo {
                name: "A".into(),
                step_type: "step".into(),
                user_prompt: "Start".into(),
                argument: String::new(),
            },
            StepInfo {
                name: "B".into(),
                step_type: "step".into(),
                user_prompt: "Process ${A} path B".into(),
                argument: String::new(),
            },
            StepInfo {
                name: "C".into(),
                step_type: "step".into(),
                user_prompt: "Process ${A} path C".into(),
                argument: String::new(),
            },
            StepInfo {
                name: "D".into(),
                step_type: "step".into(),
                user_prompt: "Merge ${B} and ${C}".into(),
                argument: String::new(),
            },
        ];

        let graph = analyze(&steps);

        assert!(graph.steps[0].is_root); // A
        assert_eq!(graph.steps[1].depends_on, vec!["A"]); // B→A
        assert_eq!(graph.steps[2].depends_on, vec!["A"]); // C→A
        assert_eq!(graph.steps[3].depends_on, vec!["B", "C"]); // D→B,C

        // B and C can be parallel
        assert!(!graph.parallel_groups.is_empty());
        let has_bc_group = graph.parallel_groups.iter().any(|g| {
            g.len() == 2 && g.contains(&"B".to_string()) && g.contains(&"C".to_string())
        });
        assert!(has_bc_group);

        // Max depth: A→B→D or A→C→D = 2
        assert_eq!(graph.max_depth, 2);
    }

    #[test]
    fn analyze_builtin_vars_excluded() {
        let steps = vec![
            StepInfo {
                name: "S1".into(),
                step_type: "step".into(),
                user_prompt: "Current step is $step_name in $flow_name".into(),
                argument: String::new(),
            },
        ];

        let graph = analyze(&steps);
        assert!(graph.steps[0].is_root);
        assert!(graph.steps[0].depends_on.is_empty());
        // All refs include builtins
        assert!(graph.steps[0].all_refs.contains(&"step_name".to_string()));
        assert!(graph.steps[0].all_refs.contains(&"flow_name".to_string()));
        // But step_refs is empty (no step-produced refs)
        assert!(graph.steps[0].step_refs.is_empty());
    }

    #[test]
    fn analyze_unresolved_refs() {
        let steps = vec![
            StepInfo {
                name: "S1".into(),
                step_type: "step".into(),
                user_prompt: "Use ${NonExistent} data".into(),
                argument: String::new(),
            },
        ];

        let graph = analyze(&steps);
        assert_eq!(graph.unresolved_refs.len(), 1);
        assert_eq!(graph.unresolved_refs[0], ("S1".to_string(), "NonExistent".to_string()));
    }

    #[test]
    fn analyze_argument_refs() {
        let steps = vec![
            StepInfo {
                name: "Gather".into(),
                step_type: "step".into(),
                user_prompt: "Gather data".into(),
                argument: String::new(),
            },
            StepInfo {
                name: "Calc".into(),
                step_type: "use_tool".into(),
                user_prompt: "Calculate".into(),
                argument: "${Gather}".into(),
            },
        ];

        let graph = analyze(&steps);
        assert_eq!(graph.steps[1].depends_on, vec!["Gather"]);
    }

    // ── v2.11.0 — use Tool(k = v) named-arg dependencies ───────────────────

    #[test]
    fn use_tool_reference_dotted_creates_dep() {
        let names: HashSet<&str> = ["ExtractUrl"].into_iter().collect();
        let na = vec![("url".into(), "ExtractUrl.output".into(), "reference".into())];
        let arg = use_tool_analysis_argument("", &na, &names);
        assert!(extract_refs(&arg).contains("ExtractUrl"));
    }

    #[test]
    fn use_tool_reference_bare_creates_dep() {
        let names: HashSet<&str> = ["ExtractUrl"].into_iter().collect();
        let na = vec![("url".into(), "ExtractUrl".into(), "reference".into())];
        let arg = use_tool_analysis_argument("", &na, &names);
        assert!(extract_refs(&arg).contains("ExtractUrl"));
    }

    #[test]
    fn use_tool_literal_interpolation_creates_dep() {
        let names: HashSet<&str> = ["ExtractCompany"].into_iter().collect();
        let na = vec![("c".into(), "${ExtractCompany}".into(), "literal".into())];
        let arg = use_tool_analysis_argument("", &na, &names);
        assert!(extract_refs(&arg).contains("ExtractCompany"));
    }

    #[test]
    fn use_tool_flow_param_reference_is_not_a_step_dep() {
        // `src = user_input` — a flow-param reference is valid (v2.10.0 type-checks
        // it) but is NOT a step, so it must add no synthetic dependency token.
        let names: HashSet<&str> = ["ExtractUrl"].into_iter().collect();
        let na = vec![("src".into(), "user_input".into(), "reference".into())];
        let arg = use_tool_analysis_argument("base", &na, &names);
        assert!(!arg.contains("${user_input}"));
        assert!(!extract_refs(&arg).contains("user_input"));
    }

    #[test]
    fn use_tool_literal_plain_value_no_dep() {
        let names: HashSet<&str> = ["ExtractUrl"].into_iter().collect();
        let na = vec![("mode".into(), "production".into(), "literal".into())];
        let arg = use_tool_analysis_argument("", &na, &names);
        assert!(extract_refs(&arg).is_empty());
    }

    #[test]
    fn use_tool_multi_arg_orders_after_independent_sources() {
        // The Kivi repro: two independent extraction steps (roots that depend
        // only on the flow-param) + a `use Tool` consuming both via `.output`.
        // Pre-v2.11.0 the call was a root → same wave as its sources → snapshot
        // race. Post-v2.11.0 the dependency is visible → a strictly later wave.
        let names: HashSet<&str> =
            ["ExtractCompany", "ExtractDomain", "GenerateRadar"].into_iter().collect();
        let na = vec![
            ("company".into(), "ExtractCompany.output".into(), "reference".into()),
            ("domain".into(), "ExtractDomain.output".into(), "reference".into()),
        ];
        let arg = use_tool_analysis_argument("", &na, &names);
        let steps = vec![
            StepInfo {
                name: "ExtractCompany".into(),
                step_type: "step".into(),
                user_prompt: "extract the company from ${user_input}".into(),
                argument: String::new(),
            },
            StepInfo {
                name: "ExtractDomain".into(),
                step_type: "step".into(),
                user_prompt: "extract the domain from ${user_input}".into(),
                argument: String::new(),
            },
            StepInfo {
                name: "GenerateRadar".into(),
                step_type: "use_tool".into(),
                user_prompt: String::new(),
                argument: arg,
            },
        ];
        let graph = analyze(&steps);
        let radar = graph.steps.iter().find(|s| s.name == "GenerateRadar").unwrap();
        assert!(radar.depends_on.contains(&"ExtractCompany".to_string()));
        assert!(radar.depends_on.contains(&"ExtractDomain".to_string()));
        assert!(!radar.is_root);

        // The schedule must NOT place GenerateRadar in wave 0 with the extractors.
        let sched = crate::parallel::build_schedule(&graph);
        assert!(!sched.waves[0].steps.contains(&"GenerateRadar".to_string()));
    }

    #[test]
    fn analyze_empty_steps() {
        let graph = analyze(&[]);
        assert!(graph.steps.is_empty());
        assert!(graph.parallel_groups.is_empty());
        assert_eq!(graph.max_depth, 0);
    }

    #[test]
    fn max_depth_flat() {
        let steps = vec![
            StepInfo { name: "A".into(), step_type: "step".into(), user_prompt: "a".into(), argument: String::new() },
            StepInfo { name: "B".into(), step_type: "step".into(), user_prompt: "b".into(), argument: String::new() },
            StepInfo { name: "C".into(), step_type: "step".into(), user_prompt: "c".into(), argument: String::new() },
        ];
        assert_eq!(analyze(&steps).max_depth, 0);
    }
}
