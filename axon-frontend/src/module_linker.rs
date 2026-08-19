//! v2.76.0 — Phase 4 of the Epistemic Module System: the LINKER.
//!
//! The phase the retired Python EMS listed as *"(future)"* — and the one
//! production actually needs, because the enterprise deploy path stores
//! ONE compiled artifact per program and re-hydrates it without sources.
//!
//! # Why the link merges **ASTs**, not IR (the anti-stub design)
//!
//! The Python EMS injected signature *stubs* into the IR: a stub of a
//! flow has no steps, so a program running an imported flow would have
//! executed an empty shell. The Rust EMS dissolves the defect
//! structurally: the linker concatenates the modules' *declarations*
//! (topological order, entry last) into one [`Program`], and the
//! standard [`crate::type_checker::TypeChecker`] +
//! [`crate::ir_generator::IRGenerator`] run ONCE over that linked
//! program. Every deep semantic law the language has therefore applies
//! cross-module **by construction** — no per-primitive injection matrix
//! to drift — and every resolved reference carries its full declaration,
//! because the declaration is *there*.
//!
//! # Virtual lines
//!
//! The EMS driver renumbers each module's tokens by a base offset before
//! parsing (a source map, in the rustc `Span` sense), so every `Loc` in
//! the linked program is globally unambiguous. [`LinkedProgram::locate`]
//! maps a virtual line back to `(origin file, local line)`; the same
//! windows ship inside the IR as [`crate::ir_nodes::IRModuleProvenance`].
//!
//! # Link laws
//!
//! - **Dependency `run` statements do not link.** A library's top-level
//!   `run`s are its own demos; only the ENTRY orchestrates execution.
//!   Every other declaration links (v1 performs no tree-shaking: the
//!   reachable module set *is* the program — selectivity is enforced at
//! the reference level by the v2.76.0 import laws, not by pruning).
//! - **Global name uniqueness.** Two modules declaring the same top-level
//! name collide at the merged registration pass (the the design decision law) — the
//!   diagnostic names both virtual locations, which map back to both
//!   files.

use crate::ast::{declaration_surface, Declaration, Loc, Program};
use crate::module_interface::CognitiveInterface;
use crate::module_resolver::ModulePath;

/// One module's contribution to the link, as assembled by the EMS driver.
pub struct LinkUnit {
    pub path: ModulePath,
    /// Display origin (file path or bundle key).
    pub origin: String,
    /// The module's parsed program, tokens already renumbered to the
    /// virtual line window `[line_base, line_base + line_count)`.
    pub program: Program,
    /// First virtual line of this module's window.
    pub line_base: u32,
    /// Source line count of the module.
    pub line_count: u32,
    /// The module's Phase-1 interface (hashes + export names).
    pub interface: CognitiveInterface,
}

/// Per-module provenance retained after the merge (the source map).
#[derive(Debug, Clone)]
pub struct LinkedModule {
    pub path: ModulePath,
    pub origin: String,
    pub line_base: u32,
    pub line_count: u32,
    pub content_hash: String,
    pub interface_hash: String,
    pub export_names: Vec<String>,
}

/// The linked program + its source map.
pub struct LinkedProgram {
    pub program: Program,
    pub modules: Vec<LinkedModule>,
}

impl LinkedProgram {
    /// Map a virtual line to `(origin file, module-local line)`. Lines
    /// outside every window (never produced by a correct link) fall back
    /// to the entry module's origin so a diagnostic is never orphaned.
    pub fn locate(&self, virtual_line: u32) -> (String, u32) {
        for m in &self.modules {
            let end = m.line_base + m.line_count;
            if virtual_line >= m.line_base && virtual_line < end {
                return (m.origin.clone(), virtual_line - m.line_base);
            }
        }
        let fallback = self
            .modules
            .last()
            .map(|m| m.origin.clone())
            .unwrap_or_default();
        (fallback, virtual_line)
    }
}

/// Merge the units (already in topological order, entry LAST) into one
/// [`Program`]. Deterministic: order in ⇒ order out.
pub fn link(units: Vec<LinkUnit>) -> LinkedProgram {
    let total: usize = units.iter().map(|u| u.program.declarations.len()).sum();
    let mut declarations: Vec<Declaration> = Vec::with_capacity(total);
    let mut modules: Vec<LinkedModule> = Vec::with_capacity(units.len());

    let last_index = units.len().saturating_sub(1);
    for (i, unit) in units.into_iter().enumerate() {
        let is_entry = i == last_index;
        let mut export_names: Vec<String> = unit
            .interface
            .exports
            .keys()
            .cloned()
            .collect();
        export_names.sort_unstable();

        for decl in unit.program.declarations {
            // Link law: a dependency's top-level `run` does not execute
            // in the linked artifact.
            if !is_entry && matches!(decl, Declaration::Run(_)) {
                continue;
            }
            declarations.push(decl);
        }

        modules.push(LinkedModule {
            path: unit.path,
            origin: unit.origin,
            line_base: unit.line_base,
            line_count: unit.line_count,
            content_hash: unit.interface.content_hash.clone(),
            interface_hash: unit.interface.interface_hash.clone(),
            export_names,
        });
    }

    LinkedProgram {
        program: Program {
            declarations,
            // The linked program is a compile artifact, not a formatting
            // surface: trivia stays with the per-module ASTs.
            declaration_trivia: Vec::new(),
            loc: Loc { line: 1, column: 1 },
        },
        modules,
    }
}

/// The declaration names a linked program provides, with their kinds —
/// used by the driver for the "declared in module X but not imported"
/// hint on unresolved-reference diagnostics.
pub fn linked_name_index(linked: &LinkedProgram) -> std::collections::BTreeMap<String, String> {
    let mut index = std::collections::BTreeMap::new();
    for m in &linked.modules {
        for name in &m.export_names {
            index
                .entry(name.clone())
                .or_insert_with(|| m.path.dotted());
        }
    }
    index
}

/// Convenience for callers that need the surface of a merged program
/// directly (parity helper for tests).
pub fn surface_names(program: &Program) -> Vec<(String, String)> {
    program
        .declarations
        .iter()
        .filter_map(|d| declaration_surface(d).map(|(n, k, _)| (n, k)))
        .collect()
}
