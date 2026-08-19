//! v2.76.0 — Phase 2 of the Epistemic Module System: the Epistemic
//! Compatibility Check (ECC) — the novel contribution of the EMS paper.
//!
//! No existing module system validates *epistemic guarantees* across
//! import boundaries. The ECC does: every import edge is checked against
//! the floors of its two modules (section 3.4/section 3.5 of the paper).
//!
//! # The law
//!
//! `gap = floor(importer) − floor(imported)`, on the rank scale
//! know=4 · believe=3 · doubt=2 · speculate=1 · unspecified=0.
//!
//! - Either floor `unspecified` ⇒ neutral, no edge fires.
//! - gap ≤ 0 ⇒ OK (same level or an upgrade).
//! - gap 1–2 ⇒ **warning `axon-W017`** (an epistemic downgrade) —
//!   silenced by the `@allow_downgrade` valve on the import statement.
//! - gap ≥ 3 ⇒ **error `axon-T954`** (severe mismatch: `know` importing
//!   `speculate`) — the valve downgrades it to `axon-W017`, so an
//!   acknowledged downgrade is *visible*, never hidden.
//!
//! `axon check --strict` escalates warnings to errors at the CLI layer
//! (the existing v1.6.0 D4 posture) — the CI stance for regulated
//! adopters.

use crate::module_interface::{EpistemicFloor, ModuleRegistry};
use crate::module_resolver::{ModuleGraph, ModulePath, ModuleSet};

/// Severity of one ECC finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EccSeverity {
    Error,
    Warning,
}

/// One ECC finding, anchored to the import statement that caused it.
#[derive(Debug, Clone)]
pub struct EccDiagnostic {
    pub code: &'static str,
    pub severity: EccSeverity,
    pub message: String,
    /// Display origin (path / bundle key) of the *importing* module.
    pub origin: String,
    pub line: u32,
    pub column: u32,
}

/// Run the ECC over every resolvable import edge. Deterministic order
/// (module-path order, then import source order).
pub fn check_compatibility(
    set: &ModuleSet,
    graph: &ModuleGraph,
    registry: &ModuleRegistry,
) -> Vec<EccDiagnostic> {
    let mut out = Vec::new();

    for (module, imports) in &graph.imports {
        let Some(importer_iface) = registry.interface(module) else {
            continue;
        };
        let importer_floor = importer_iface.epistemic_floor;
        let origin = set
            .get(module)
            .map(|m| m.origin.clone())
            .unwrap_or_else(|| module.dotted());

        for imp in imports {
            if !imp.selective || imp.module_path.is_scoped() {
                continue; // refused elsewhere (axon-T953, v2.76.0)
            }
            let Some(dep_iface) = registry.interface(&imp.module_path) else {
                continue; // unresolved — axon-T953 owns that diagnostic
            };
            let dep_floor = dep_iface.epistemic_floor;

            if importer_floor == EpistemicFloor::Unspecified
                || dep_floor == EpistemicFloor::Unspecified
            {
                continue; // neutral — no declared guarantee on one side
            }

            let gap = i32::from(importer_floor.rank()) - i32::from(dep_floor.rank());
            if gap <= 0 {
                continue;
            }

            let describe = |m: &ModulePath, f: EpistemicFloor| {
                format!("'{}' ({}-level)", m.dotted(), f.as_str())
            };
            if gap >= 3 {
                if imp.allow_downgrade {
                    out.push(EccDiagnostic {
                        code: "axon-W017",
                        severity: EccSeverity::Warning,
                        message: format!(
                            "acknowledged severe epistemic downgrade: {} imports {} under \
                             @allow_downgrade. The import executes with speculative-grade \
                             reasoning where {}-grade rigor is guaranteed — the valve makes \
                             this visible, not silent.",
                            describe(module, importer_floor),
                            describe(&imp.module_path, dep_floor),
                            importer_floor.as_str(),
                        ),
                        origin: origin.clone(),
                        line: imp.line,
                        column: imp.column,
                    });
                } else {
                    out.push(EccDiagnostic {
                        code: "axon-T954",
                        severity: EccSeverity::Error,
                        message: format!(
                            "epistemic conflict: {} imports {}. A {}-level module cannot \
                             silently depend on {}-level definitions — factual rigor would \
                             execute over speculative reasoning. Either raise the imported \
                             module's floor (add anchors / a `know` block) or acknowledge \
                             the downgrade explicitly with `@allow_downgrade`.",
                            describe(module, importer_floor),
                            describe(&imp.module_path, dep_floor),
                            importer_floor.as_str(),
                            dep_floor.as_str(),
                        ),
                        origin: origin.clone(),
                        line: imp.line,
                        column: imp.column,
                    });
                }
            } else if !imp.allow_downgrade {
                out.push(EccDiagnostic {
                    code: "axon-W017",
                    severity: EccSeverity::Warning,
                    message: format!(
                        "epistemic downgrade: {} imports {}. The imported guarantees are \
                         weaker than this module's floor; acknowledge with \
                         `@allow_downgrade` or raise the imported module's floor.",
                        describe(module, importer_floor),
                        describe(&imp.module_path, dep_floor),
                    ),
                    origin: origin.clone(),
                    line: imp.line,
                    column: imp.column,
                });
            }
        }
    }

    out
}

// ════════════════════════════════════════════════════════════════════
//  Unit tests (integration suite: tests/epistemic_compat.rs)
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::module_interface::generate_interface;
    use crate::parser::Parser;
    use std::collections::BTreeMap;

    /// Build set + graph + registry from (path, source) pairs.
    fn world(pairs: &[(&str, &str)], entry: &str) -> (ModuleSet, ModuleGraph, ModuleRegistry) {
        let files: BTreeMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let set = ModuleSet::from_memory(&files, entry).expect("set");
        let graph = ModuleGraph::build(&set).expect("graph");
        let mut registry = ModuleRegistry::new();
        for (path, module) in set.iter() {
            let tokens = Lexer::new(&module.source, &module.origin)
                .tokenize()
                .expect("lex");
            let program = Parser::new(tokens).parse().expect("parse");
            registry.register(path.clone(), generate_interface(path, &program, &module.source));
        }
        (set, graph, registry)
    }

    const KNOW_MAIN: &str = "import lib.{Wild}\nanchor Strict { require: source_citation }\n";
    const SPECULATE_LIB: &str = "speculate {\n  persona Wild { domain: [\"ideas\"] }\n}\n";
    const BELIEVE_LIB: &str = "shield Soft { scan: [pii_leak] on_breach: halt }\npersona Wild { domain: [\"ideas\"] }\n";

    #[test]
    fn know_importing_speculate_is_t954() {
        let (set, graph, registry) =
            world(&[("main.axon", KNOW_MAIN), ("lib.axon", SPECULATE_LIB)], "main.axon");
        let diags = check_compatibility(&set, &graph, &registry);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "axon-T954");
        assert_eq!(diags[0].severity, EccSeverity::Error);
    }

    #[test]
    fn allow_downgrade_downgrades_t954_to_visible_warning() {
        let main = "import lib.{Wild} @allow_downgrade\nanchor Strict { require: source_citation }\n";
        let (set, graph, registry) =
            world(&[("main.axon", main), ("lib.axon", SPECULATE_LIB)], "main.axon");
        let diags = check_compatibility(&set, &graph, &registry);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "axon-W017");
        assert_eq!(diags[0].severity, EccSeverity::Warning);
    }

    #[test]
    fn know_importing_believe_is_w017() {
        let (set, graph, registry) =
            world(&[("main.axon", KNOW_MAIN), ("lib.axon", BELIEVE_LIB)], "main.axon");
        let diags = check_compatibility(&set, &graph, &registry);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "axon-W017");
    }

    #[test]
    fn allow_downgrade_silences_small_gap() {
        let main = "import lib.{Wild} @allow_downgrade\nanchor Strict { require: source_citation }\n";
        let (set, graph, registry) =
            world(&[("main.axon", main), ("lib.axon", BELIEVE_LIB)], "main.axon");
        assert!(check_compatibility(&set, &graph, &registry).is_empty());
    }

    #[test]
    fn upgrade_and_same_level_are_clean() {
        // speculate-level main importing know-level lib: an upgrade.
        let main = "import lib.{Strict}\nspeculate {\n  persona Wild { domain: [\"ideas\"] }\n}\n";
        let lib = "anchor Strict { require: source_citation }\n";
        let (set, graph, registry) =
            world(&[("main.axon", main), ("lib.axon", lib)], "main.axon");
        assert!(check_compatibility(&set, &graph, &registry).is_empty());
    }

    #[test]
    fn unspecified_floors_are_neutral() {
        let main = "import lib.{P}\nanchor Strict { require: source_citation }\n";
        let lib = "persona P { domain: [\"x\"] }\n"; // unspecified floor
        let (set, graph, registry) =
            world(&[("main.axon", main), ("lib.axon", lib)], "main.axon");
        assert!(check_compatibility(&set, &graph, &registry).is_empty());
    }
}
