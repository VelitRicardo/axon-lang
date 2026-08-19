//! v2.76.0 — Phase 0 of the Epistemic Module System: dependency discovery.
//!
//! Builds the module dependency DAG for a multi-file AXON project and
//! topologically sorts it (Kahn), refusing cycles (`axon-T955`).
//!
//! # Design
//!
//! - **In-memory-first.** The resolver operates over a [`ModuleSet`] — a
//!   deterministic map from [`ModulePath`] to source text. The filesystem
//!   walk ([`ModuleSet::from_entry_file`]) is one constructor on top; the
//!   enterprise bundle path and the LSP feed sources directly
//!   ([`ModuleSet::from_memory`]) and never touch a disk.
//! - **Lexer-true scanning.** [`scan_imports`] tokenizes with the real AXON
//!   lexer and walks tokens — no AST, and crucially no regex: discovery can
//!   never recognize a different import grammar than the parser does (the
//!   drift class the retired Python EMS's regex scanner invited).
//! - **Lenient scan, authoritative parse.** A malformed import statement is
//!   *skipped* by the scanner — the parser owns the canonical diagnostic.
//!   The scanner's only job is to know which files to load.
//! - **Deterministic everywhere.** `BTreeMap`/`BTreeSet` ordering, and the
//!   Kahn ready-queue pops the smallest module path first, so the
//! topological order is a pure function of the module set (section 4.4 of the
//!   EMS paper — the property the enterprise `ir_sha256` dedupe anchor
//!   relies on).
//!
//! # Refusal posture
//!
//! Two import forms parse but are **refused** downstream (`axon-T953`, in
//! the type-checker's module mode): the non-selective `import a.b` (name
//! pollution — `#include` wearing a module system's clothes) and the
//! `@scope`-prefixed form (reserved for a future package registry). The
//! resolver records them (so the diagnostics can fire with real locations)
//! but neither loads files nor contributes DAG edges for them.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::lexer::Lexer;
use crate::tokens::TokenType;

/// Hard ceiling on the number of modules a single project may load.
/// Fail-closed guard against runaway transitive graphs; generous by an
/// order of magnitude over any real deployment seen to date.
pub const MAX_MODULES: usize = 512;

// ════════════════════════════════════════════════════════════════════
//  ModulePath
// ════════════════════════════════════════════════════════════════════

/// A dotted module path: `axon.security` ⇔ `["axon", "security"]` ⇔
/// `<modules-root>/axon/security.axon`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModulePath(pub Vec<String>);

impl ModulePath {
    /// The dotted display form (`axon.security`).
    pub fn dotted(&self) -> String {
        self.0.join(".")
    }

    /// The root-relative file this path resolves to (`axon/security.axon`).
    pub fn relative_file(&self) -> PathBuf {
        let mut p = PathBuf::new();
        for part in &self.0 {
            p.push(part);
        }
        p.set_extension("axon");
        p
    }

    /// Whether this is the reserved `@scope` form (first segment keeps its
    /// literal `@` prefix, exactly as the parser stores it).
    pub fn is_scoped(&self) -> bool {
        self.0.first().map(|s| s.starts_with('@')).unwrap_or(false)
    }

    /// Build from a root-relative file path (`axon/security.axon` →
    /// `axon.security`). Returns `None` when a segment is not a valid
    /// module identifier (`[A-Za-z_][A-Za-z0-9_]*`) or the extension is
    /// not `.axon`.
    pub fn from_relative_file(rel: &str) -> Option<ModulePath> {
        let normalized = rel.replace('\\', "/");
        let stripped = normalized.strip_suffix(".axon")?;
        if stripped.is_empty() {
            return None;
        }
        let segments: Vec<String> = stripped.split('/').map(str::to_string).collect();
        if segments.iter().all(|s| is_module_ident(s)) {
            Some(ModulePath(segments))
        } else {
            None
        }
    }
}

impl fmt::Display for ModulePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.dotted())
    }
}

fn is_module_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ════════════════════════════════════════════════════════════════════
//  Scanned imports (Phase 0 sees imports only — no AST)
// ════════════════════════════════════════════════════════════════════

/// One `import` statement as seen by the Phase-0 token scan.
#[derive(Debug, Clone)]
pub struct ScannedImport {
    pub module_path: ModulePath,
    /// The `{…}` selector names. Empty ⇔ the non-selective form.
    pub names: Vec<String>,
    /// `@allow_downgrade` ECC valve present (v2.76.0).
    pub allow_downgrade: bool,
    /// Whether the `{…}` selector was present at all.
    pub selective: bool,
    pub line: u32,
    pub column: u32,
}

/// Extract every `import` statement from `source` via the real lexer.
///
/// Lenient by design: a *malformed* import is skipped (the parser owns
/// the canonical error); a source that does not lex returns the lexer's
/// error verbatim (nothing downstream could load such a module anyway).
pub fn scan_imports(source: &str, filename: &str) -> Result<Vec<ScannedImport>, String> {
    let tokens = Lexer::new(source, filename)
        .tokenize()
        .map_err(|e| format!("{}:{}:{} {}", filename, e.line, e.column, e.message))?;

    let toks: Vec<_> = tokens
        .into_iter()
        .filter(|t| !is_comment(&t.ttype))
        .collect();

    let mut out = Vec::new();
    let mut i = 0usize;
    while i < toks.len() {
        if toks[i].ttype != TokenType::Import {
            i += 1;
            continue;
        }
        let (line, column) = (toks[i].line, toks[i].column);
        i += 1;

        // ── path: [@]ident (. ident)* ────────────────────────────
        let mut parts: Vec<String> = Vec::new();
        let scoped = i < toks.len() && toks[i].ttype == TokenType::At;
        if scoped {
            i += 1;
            match toks.get(i) {
                Some(t) if t.ttype == TokenType::Identifier => {
                    parts.push(format!("@{}", t.value));
                    i += 1;
                }
                _ => continue, // malformed — parser will refuse
            }
        } else {
            match toks.get(i) {
                Some(t) if t.ttype == TokenType::Identifier => {
                    parts.push(t.value.clone());
                    i += 1;
                }
                _ => continue,
            }
        }
        while i < toks.len() && toks[i].ttype == TokenType::Dot {
            // `a.b.{X}` — the dot immediately before the selector brace
            // terminates the path (mirror of `parse_import`).
            if toks.get(i + 1).map(|t| &t.ttype) == Some(&TokenType::LBrace) {
                i += 1;
                break;
            }
            match toks.get(i + 1) {
                Some(t) if t.ttype == TokenType::Identifier => {
                    parts.push(t.value.clone());
                    i += 2;
                }
                _ => break, // malformed tail — parser will refuse
            }
        }

        // ── selector: { A, B } ───────────────────────────────────
        let mut names = Vec::new();
        let mut selective = false;
        if i < toks.len() && toks[i].ttype == TokenType::LBrace {
            selective = true;
            i += 1;
            loop {
                match toks.get(i) {
                    Some(t) if t.ttype == TokenType::Identifier => {
                        names.push(t.value.clone());
                        i += 1;
                    }
                    _ => break,
                }
                if toks.get(i).map(|t| &t.ttype) == Some(&TokenType::Comma) {
                    i += 1;
                    continue;
                }
                break;
            }
            if toks.get(i).map(|t| &t.ttype) == Some(&TokenType::RBrace) {
                i += 1;
            }
        }

        // ── v2.76.0 valve: @allow_downgrade ───────────────────────
        let mut allow_downgrade = false;
        if toks.get(i).map(|t| &t.ttype) == Some(&TokenType::At)
            && toks
                .get(i + 1)
                .map(|t| t.ttype == TokenType::Identifier && t.value == "allow_downgrade")
                .unwrap_or(false)
        {
            allow_downgrade = true;
            i += 2;
        }

        out.push(ScannedImport {
            module_path: ModulePath(parts),
            names,
            allow_downgrade,
            selective,
            line,
            column,
        });
    }
    Ok(out)
}

fn is_comment(tt: &TokenType) -> bool {
    matches!(
        tt,
        TokenType::LineComment
            | TokenType::BlockComment
            | TokenType::DocLineComment
            | TokenType::DocBlockComment
            | TokenType::InnerDocLineComment
            | TokenType::InnerDocBlockComment
    )
}

// ════════════════════════════════════════════════════════════════════
//  Errors
// ════════════════════════════════════════════════════════════════════

/// A Phase-0 resolution failure. Rendered by the CLI in the house
/// `error [line N]:` shape against the *importing* file.
#[derive(Debug, Clone)]
pub struct ModuleError {
    pub code: &'static str,
    pub message: String,
    /// Display path (or bundle key) of the file the diagnostic anchors to.
    pub origin: String,
    pub line: u32,
    pub column: u32,
}

impl ModuleError {
    fn new(code: &'static str, message: String, origin: &str, line: u32, column: u32) -> Self {
        ModuleError {
            code,
            message,
            origin: origin.to_string(),
            line,
            column,
        }
    }
}

impl fmt::Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{} {} {}",
            self.origin, self.line, self.column, self.code, self.message
        )
    }
}

// ════════════════════════════════════════════════════════════════════
//  ModuleSet
// ════════════════════════════════════════════════════════════════════

/// One loaded module: its display origin (path or bundle key) + source.
#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub origin: String,
    pub source: String,
}

/// The complete, deterministic set of modules for one compilation:
/// the entry plus every transitively imported module.
#[derive(Debug)]
pub struct ModuleSet {
    pub entry: ModulePath,
    modules: BTreeMap<ModulePath, LoadedModule>,
}

impl ModuleSet {
    /// Walk the filesystem from `entry_file`, loading every transitively
    /// imported module under `modules_root` (default: the entry file's
    /// directory — the design decision).
    ///
    /// Only **selective, unscoped** imports load files; the refused forms
    /// surface later with real locations, so an unresolvable
    /// `@scope` path can never abort the load of an otherwise-valid
    /// project.
    pub fn from_entry_file(
        entry_file: &Path,
        modules_root: Option<&Path>,
    ) -> Result<ModuleSet, ModuleError> {
        let entry_origin = entry_file.display().to_string();
        let source = std::fs::read_to_string(entry_file).map_err(|e| {
            ModuleError::new(
                "axon-T953",
                format!("cannot read entry file: {e}"),
                &entry_origin,
                0,
                0,
            )
        })?;

        let stem = entry_file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "main".to_string());
        let entry_path = ModulePath(vec![stem]);

        let root: PathBuf = match modules_root {
            Some(r) => r.to_path_buf(),
            None => entry_file
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from(".")),
        };

        let mut modules = BTreeMap::new();
        modules.insert(
            entry_path.clone(),
            LoadedModule {
                origin: entry_origin.clone(),
                source,
            },
        );

        // BFS over selective, unscoped imports.
        let mut queue: VecDeque<ModulePath> = VecDeque::new();
        queue.push_back(entry_path.clone());
        while let Some(current) = queue.pop_front() {
            let loaded = &modules[&current];
            let origin = loaded.origin.clone();
            let imports = scan_imports(&loaded.source, &origin).map_err(|msg| {
                ModuleError::new("axon-T953", format!("lex error during discovery: {msg}"), &origin, 0, 0)
            })?;
            for imp in imports {
                if !imp.selective || imp.module_path.is_scoped() {
                    continue; // refused later with a real location
                }
                if modules.contains_key(&imp.module_path) {
                    continue;
                }
                if modules.len() >= MAX_MODULES {
                    return Err(ModuleError::new(
                        "axon-T953",
                        format!(
                            "module ceiling exceeded: a project may load at most {MAX_MODULES} modules"
                        ),
                        &origin,
                        imp.line,
                        imp.column,
                    ));
                }
                let file = root.join(imp.module_path.relative_file());
                let dep_source = std::fs::read_to_string(&file).map_err(|_| {
                    ModuleError::new(
                        "axon-T953",
                        format!(
                            "module '{}' not found: searched {}",
                            imp.module_path,
                            file.display()
                        ),
                        &origin,
                        imp.line,
                        imp.column,
                    )
                })?;
                modules.insert(
                    imp.module_path.clone(),
                    LoadedModule {
                        origin: file.display().to_string(),
                        source: dep_source,
                    },
                );
                queue.push_back(imp.module_path);
            }
        }

        Ok(ModuleSet {
            entry: entry_path,
            modules,
        })
    }

    /// Build from an in-memory bundle: root-relative file paths → sources.
    /// `entry` names one of the keys. Every file must be **reachable** from
    /// the entry through selective, unscoped imports — a bundle carrying
    /// dead files is refused rather than silently shipping them (imports
    /// are static; unreachable means unreferenced, and an artifact should
    /// not quietly contain source nobody asked to link).
    pub fn from_memory(
        files: &BTreeMap<String, String>,
        entry: &str,
    ) -> Result<ModuleSet, ModuleError> {
        if files.len() > MAX_MODULES {
            return Err(ModuleError::new(
                "axon-T953",
                format!("bundle exceeds the {MAX_MODULES}-module ceiling"),
                entry,
                0,
                0,
            ));
        }

        // Map every bundle key to a ModulePath up front (validates keys).
        let mut by_path: BTreeMap<ModulePath, (String, String)> = BTreeMap::new();
        for (key, source) in files {
            let mp = ModulePath::from_relative_file(key).ok_or_else(|| {
                ModuleError::new(
                    "axon-T953",
                    format!(
                        "bundle file '{key}' is not a valid module path: segments must be \
                         identifiers and the extension must be .axon"
                    ),
                    key,
                    0,
                    0,
                )
            })?;
            if by_path
                .insert(mp.clone(), (key.clone(), source.clone()))
                .is_some()
            {
                return Err(ModuleError::new(
                    "axon-T953",
                    format!("bundle files collide on module path '{mp}'"),
                    key,
                    0,
                    0,
                ));
            }
        }

        let entry_path = ModulePath::from_relative_file(entry).ok_or_else(|| {
            ModuleError::new(
                "axon-T953",
                format!("bundle entry '{entry}' is not a valid module path"),
                entry,
                0,
                0,
            )
        })?;
        if !by_path.contains_key(&entry_path) {
            return Err(ModuleError::new(
                "axon-T953",
                format!("bundle entry '{entry}' is not among the bundle files"),
                entry,
                0,
                0,
            ));
        }

        // Reachability from the entry (selective, unscoped imports only).
        let mut reached: BTreeSet<ModulePath> = BTreeSet::new();
        reached.insert(entry_path.clone());
        let mut queue: VecDeque<ModulePath> = VecDeque::new();
        queue.push_back(entry_path.clone());
        while let Some(current) = queue.pop_front() {
            let (origin, source) = &by_path[&current];
            let imports = scan_imports(source, origin).map_err(|msg| {
                ModuleError::new("axon-T953", format!("lex error during discovery: {msg}"), origin, 0, 0)
            })?;
            for imp in imports {
                if !imp.selective || imp.module_path.is_scoped() {
                    continue;
                }
                if !by_path.contains_key(&imp.module_path) {
                    return Err(ModuleError::new(
                        "axon-T953",
                        format!(
                            "module '{}' not found in bundle (expected file '{}')",
                            imp.module_path,
                            imp.module_path.relative_file().display()
                        ),
                        origin,
                        imp.line,
                        imp.column,
                    ));
                }
                if reached.insert(imp.module_path.clone()) {
                    queue.push_back(imp.module_path);
                }
            }
        }
        let dead: Vec<String> = by_path
            .keys()
            .filter(|p| !reached.contains(*p))
            .map(|p| p.relative_file().display().to_string())
            .collect();
        if !dead.is_empty() {
            return Err(ModuleError::new(
                "axon-T953",
                format!(
                    "bundle contains files unreachable from the entry: {}",
                    dead.join(", ")
                ),
                entry,
                0,
                0,
            ));
        }

        let modules = by_path
            .into_iter()
            .map(|(mp, (origin, source))| (mp, LoadedModule { origin, source }))
            .collect();
        Ok(ModuleSet {
            entry: entry_path,
            modules,
        })
    }

    pub fn get(&self, path: &ModulePath) -> Option<&LoadedModule> {
        self.modules.get(path)
    }

    /// Deterministic iteration over (path, module).
    pub fn iter(&self) -> impl Iterator<Item = (&ModulePath, &LoadedModule)> {
        self.modules.iter()
    }

    pub fn len(&self) -> usize {
        self.modules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }
}

// ════════════════════════════════════════════════════════════════════
//  ModuleGraph — DAG + Kahn + cycle refusal (axon-T955)
// ════════════════════════════════════════════════════════════════════

/// The resolved dependency graph: every module's scanned imports plus the
/// deterministic topological order (dependencies first, entry last).
#[derive(Debug)]
pub struct ModuleGraph {
    /// Topological order, dependencies before dependents. With no cycles
    /// this always contains every module of the set exactly once.
    pub order: Vec<ModulePath>,
    /// Every scanned import per module — including the refused forms, so
    /// downstream diagnostics fire with real locations.
    pub imports: BTreeMap<ModulePath, Vec<ScannedImport>>,
}

impl ModuleGraph {
    /// Build + topologically sort. Refuses cycles with `axon-T955`,
    /// naming the full cycle path.
    pub fn build(set: &ModuleSet) -> Result<ModuleGraph, ModuleError> {
        let mut imports: BTreeMap<ModulePath, Vec<ScannedImport>> = BTreeMap::new();
        // dependency edges: module → set of modules it imports (resolvable
        // forms only), restricted to modules present in the set.
        let mut deps: BTreeMap<ModulePath, BTreeSet<ModulePath>> = BTreeMap::new();

        for (path, module) in set.iter() {
            let scanned = scan_imports(&module.source, &module.origin).map_err(|msg| {
                ModuleError::new("axon-T953", format!("lex error during discovery: {msg}"), &module.origin, 0, 0)
            })?;
            let mut dep_set: BTreeSet<ModulePath> = BTreeSet::new();
            for imp in &scanned {
                if imp.selective && !imp.module_path.is_scoped() && set.get(&imp.module_path).is_some()
                {
                    // Self-import is a 1-cycle; keep the edge so Kahn
                    // refuses it with the honest diagnostic.
                    dep_set.insert(imp.module_path.clone());
                }
            }
            imports.insert(path.clone(), scanned);
            deps.insert(path.clone(), dep_set);
        }

        // Kahn, smallest-path-first for determinism.
        let mut in_degree: BTreeMap<&ModulePath, usize> =
            deps.iter().map(|(p, d)| (p, d.len())).collect();
        let mut dependents: BTreeMap<&ModulePath, Vec<&ModulePath>> = BTreeMap::new();
        for (p, dset) in &deps {
            for d in dset {
                dependents.entry(d).or_default().push(p);
            }
        }

        let mut ready: BTreeSet<&ModulePath> = in_degree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(p, _)| *p)
            .collect();
        let mut order: Vec<ModulePath> = Vec::with_capacity(deps.len());
        while let Some(&next) = ready.iter().next() {
            ready.remove(next);
            order.push(next.clone());
            if let Some(deps_of_next) = dependents.get(next) {
                for &dependent in deps_of_next {
                    let deg = in_degree.get_mut(dependent).expect("known module");
                    *deg -= 1;
                    if *deg == 0 {
                        ready.insert(dependent);
                    }
                }
            }
        }

        if order.len() != deps.len() {
            // Cycle: walk dependency edges among the unsorted remainder
            // from the smallest leftover node until a repeat, then trim to
            // the cycle proper.
            let leftover: BTreeSet<&ModulePath> = deps
                .keys()
                .filter(|p| !order.contains(*p))
                .collect();
            let start = *leftover.iter().next().expect("non-empty leftover");
            let mut path_walk: Vec<&ModulePath> = vec![start];
            let mut seen: BTreeMap<&ModulePath, usize> = BTreeMap::new();
            seen.insert(start, 0);
            let mut current = start;
            let cycle_text = loop {
                let next = deps[current]
                    .iter()
                    .find(|d| leftover.contains(*d))
                    .expect("a leftover node always has a leftover dependency");
                if let Some(&idx) = seen.get(next) {
                    let mut cyc: Vec<String> =
                        path_walk[idx..].iter().map(|p| p.dotted()).collect();
                    cyc.push(next.dotted());
                    break cyc.join(" → ");
                }
                seen.insert(next, path_walk.len());
                path_walk.push(next);
                current = next;
            };
            let origin = set
                .get(start)
                .map(|m| m.origin.clone())
                .unwrap_or_default();
            return Err(ModuleError::new(
                "axon-T955",
                format!(
                    "import cycle detected: {cycle_text}. Cognitive modules must form a DAG — \
                     a persona cannot depend on an anchor that depends on that persona's \
                     definition. Break the cycle by moving the shared definitions into a \
                     module both sides import."
                ),
                &origin,
                0,
                0,
            ));
        }

        Ok(ModuleGraph { order, imports })
    }

    /// The resolvable dependency paths of `module` (deterministic order).
    pub fn dependencies_of(&self, module: &ModulePath) -> Vec<&ModulePath> {
        let mut out: Vec<&ModulePath> = Vec::new();
        if let Some(imps) = self.imports.get(module) {
            let mut seen = BTreeSet::new();
            for imp in imps {
                if imp.selective && !imp.module_path.is_scoped() && seen.insert(&imp.module_path) {
                    out.push(&imp.module_path);
                }
            }
        }
        out
    }
}

// ════════════════════════════════════════════════════════════════════
//  Unit tests (integration suite: tests/module_resolver.rs)
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn set_of(pairs: &[(&str, &str)], entry: &str) -> ModuleSet {
        let files: BTreeMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        ModuleSet::from_memory(&files, entry).expect("valid set")
    }

    #[test]
    fn scan_finds_selective_import() {
        let imps = scan_imports("import axon.security.{A, B}\n", "t.axon").unwrap();
        assert_eq!(imps.len(), 1);
        assert_eq!(imps[0].module_path.dotted(), "axon.security");
        assert_eq!(imps[0].names, vec!["A", "B"]);
        assert!(imps[0].selective);
        assert!(!imps[0].allow_downgrade);
    }

    #[test]
    fn scan_finds_allow_downgrade_valve() {
        let imps = scan_imports("import a.b.{X} @allow_downgrade\n", "t.axon").unwrap();
        assert!(imps[0].allow_downgrade);
    }

    #[test]
    fn scan_flags_non_selective_and_scoped() {
        // NB: the scope segment must not collide with a language keyword
        // (`scope` is one) — the scanner mirrors the parser's grammar,
        // which requires an Identifier after `@`.
        let imps = scan_imports("import a.b\nimport @myscope.pkg.{X}\n", "t.axon").unwrap();
        assert_eq!(imps.len(), 2);
        assert!(!imps[0].selective);
        assert!(imps[1].module_path.is_scoped());
    }

    #[test]
    fn kahn_orders_dependencies_first() {
        let set = set_of(
            &[
                ("main.axon", "import lib.a.{X}\n"),
                ("lib/a.axon", "import lib.b.{Y}\n"),
                ("lib/b.axon", "persona Y { domain: [\"d\"] }\n"),
            ],
            "main.axon",
        );
        let g = ModuleGraph::build(&set).unwrap();
        let pos = |d: &str| g.order.iter().position(|p| p.dotted() == d).unwrap();
        assert!(pos("lib.b") < pos("lib.a"));
        assert!(pos("lib.a") < pos("main"));
    }

    #[test]
    fn diamond_resolves_once_deterministically() {
        let set = set_of(
            &[
                ("main.axon", "import b.{X}\nimport c.{Y}\n"),
                ("b.axon", "import d.{Z}\n"),
                ("c.axon", "import d.{Z}\n"),
                ("d.axon", "anchor Z { require: source_citation }\n"),
            ],
            "main.axon",
        );
        let g = ModuleGraph::build(&set).unwrap();
        assert_eq!(g.order.len(), 4);
        assert_eq!(g.order.first().unwrap().dotted(), "d");
        assert_eq!(g.order.last().unwrap().dotted(), "main");
    }

    #[test]
    fn cycle_is_refused_with_named_path() {
        let set = set_of(
            &[
                ("main.axon", "import a.{X}\n"),
                ("a.axon", "import b.{Y}\n"),
                ("b.axon", "import a.{X}\n"),
            ],
            "main.axon",
        );
        let err = ModuleGraph::build(&set).unwrap_err();
        assert_eq!(err.code, "axon-T955");
        assert!(err.message.contains("a → b → a") || err.message.contains("b → a → b"));
    }

    #[test]
    fn self_import_is_a_cycle() {
        let set = set_of(&[("main.axon", "import main.{X}\n")], "main.axon");
        let err = ModuleGraph::build(&set).unwrap_err();
        assert_eq!(err.code, "axon-T955");
    }

    #[test]
    fn bundle_missing_module_is_refused() {
        let files: BTreeMap<String, String> =
            [("main.axon".to_string(), "import gone.{X}\n".to_string())].into();
        let err = ModuleSet::from_memory(&files, "main.axon").unwrap_err();
        assert_eq!(err.code, "axon-T953");
        assert!(err.message.contains("gone"));
    }

    #[test]
    fn bundle_dead_file_is_refused() {
        let files: BTreeMap<String, String> = [
            ("main.axon".to_string(), "persona P { domain: [\"x\"] }\n".to_string()),
            ("dead.axon".to_string(), "persona Q { domain: [\"y\"] }\n".to_string()),
        ]
        .into();
        let err = ModuleSet::from_memory(&files, "main.axon").unwrap_err();
        assert!(err.message.contains("unreachable"));
        assert!(err.message.contains("dead.axon"));
    }
}
