//! v2.76.0 — the EMS driver: the one orchestration of Phases 0–4.
//!
//! Pure and in-memory-first: the CLI (`axon-rs`) hands it an entry file,
//! the enterprise bundle loader hands it a [`ModuleSet`] — both get the
//! same pipeline, the same diagnostics, the same linked artifact. This is
//! deliberate: "where does PRODUCTION call it" has one answer per surface
//! and they share every law.
//!
//! ```text
//! ModuleSet ──▶ ModuleGraph (Kahn, T955)
//!    │
//!    ├─ per module (topo order): lex → renumber lines by base offset
//!    │  (the source map) → parse → .axi interface
//!    │
//!    ├─ per module: type-check in MODULE MODE (imports register, T953)
//! │      · skipped on a cache validation hit (v2.76.0)
//!    ├─ ECC over every import edge (T954 / W017)
//!    │
//!    ├─ LINK: merge ASTs (entry last, dep `run`s dropped)
//!    ├─ merged revalidation — the SEMANTIC AUTHORITY: every deep law
//!    │  the language has, applied cross-module by construction
//!    └─ IR generation over the linked program (+ module provenance)
//! ```
//!
//! # Diagnostic protocol
//!
//! - **Errors** — union of resolver, parse, per-module (import laws +
//!   local checks), ECC and merged-gate errors, deduplicated by
//!   `(file, line, message)` and reported against the module-LOCAL line.
//! - **Warnings** — the merged gate's (global semantics are the truth;
//!   a per-module pass cannot see another module's producers and would
//!   emit spurious ones) plus the ECC's.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::compilation_cache::{CacheStats, CompilationCache, CACHE_DIR_NAME};
use crate::epistemic_compat::{check_compatibility, EccSeverity};
use crate::ir_nodes::{IRModuleProvenance, IRProgram};
use crate::lexer::Lexer;
use crate::module_interface::{generate_interface, CognitiveInterface, ModuleRegistry};
use crate::module_linker::{link, LinkUnit, LinkedProgram};
use crate::module_resolver::{scan_imports, ModuleGraph, ModulePath, ModuleSet};
use crate::parser::Parser;
use crate::type_checker::{ModuleCheckContext, TypeChecker};

/// Does this source declare any `import` at all? The EMS engagement
/// criterion: one import statement — selective or not — means module
/// semantics were requested (a refused form then refuses loudly, v2.76.0,
/// instead of staying decorative as in v2.75.0).
pub fn source_declares_imports(source: &str, filename: &str) -> bool {
    scan_imports(source, filename)
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

/// Driver options.
#[derive(Debug, Default)]
pub struct EmsOptions {
    /// Module root override (the design decision; default = entry file's directory).
    pub modules_root: Option<PathBuf>,
    /// Consult/populate the on-disk cache (`--no-cache` disables).
    pub use_cache: bool,
    /// Cache directory override (default `<entry dir>/.axon_cache`).
    pub cache_dir: Option<PathBuf>,
}

/// One mapped diagnostic: `file` is the module origin, `line` is LOCAL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmsDiagnostic {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub message: String,
}

/// A failed compilation: errors (non-empty) + the warnings gathered
/// before the failure point.
#[derive(Debug)]
pub struct EmsFailure {
    pub errors: Vec<EmsDiagnostic>,
    pub warnings: Vec<EmsDiagnostic>,
}

/// A successful compilation of a multi-module project.
pub struct EmsSuccess {
    /// The LINKED program's IR — the deployable artifact, module
    /// provenance included (`ir.modules`).
    pub ir: IRProgram,
    pub warnings: Vec<EmsDiagnostic>,
    pub stats: CacheStats,
    pub module_count: usize,
    /// Total tokens / declarations across the project (the `axon check`
    /// report numbers).
    pub token_count: usize,
    pub declaration_count: usize,
}

/// Compile a multi-file project from its entry file (the CLI path).
pub fn compile_project(entry_file: &Path, opts: &EmsOptions) -> Result<EmsSuccess, EmsFailure> {
    compile_project_with_manifest(entry_file, opts, None)
}

/// [`compile_project`] with the v1.31.0 store-schema manifest (the
/// `--schemas-dir` surface): forms (b)/(c) column proofs (T801–T805)
/// run identically in module mode.
pub fn compile_project_with_manifest(
    entry_file: &Path,
    opts: &EmsOptions,
    manifest: Option<&crate::store_schema_manifest::Manifest>,
) -> Result<EmsSuccess, EmsFailure> {
    let set = ModuleSet::from_entry_file(entry_file, opts.modules_root.as_deref())
        .map_err(|e| failure_of_module_error(&e))?;
    let cache_dir = if opts.use_cache {
        Some(opts.cache_dir.clone().unwrap_or_else(|| {
            entry_file
                .parent()
                .map(|p| p.join(CACHE_DIR_NAME))
                .unwrap_or_else(|| PathBuf::from(CACHE_DIR_NAME))
        }))
    } else {
        None
    };
    compile_module_set_with_manifest(&set, cache_dir.as_deref(), manifest)
}

/// Compile an assembled [`ModuleSet`] (the enterprise-bundle path; also
/// the CLI path after discovery). `cache_dir: None` disables the cache.
pub fn compile_module_set(
    set: &ModuleSet,
    cache_dir: Option<&Path>,
) -> Result<EmsSuccess, EmsFailure> {
    compile_module_set_with_manifest(set, cache_dir, None)
}

/// [`compile_module_set`] with the v1.31.0 manifest threaded into BOTH the
/// per-module passes and the merged revalidation.
pub fn compile_module_set_with_manifest(
    set: &ModuleSet,
    cache_dir: Option<&Path>,
    manifest: Option<&crate::store_schema_manifest::Manifest>,
) -> Result<EmsSuccess, EmsFailure> {
    let graph = ModuleGraph::build(set).map_err(|e| failure_of_module_error(&e))?;

    // ── Pass 1 (topo order): lex → renumber → parse → interface ──────
    struct Compiled {
        path: ModulePath,
        origin: String,
        program: crate::ast::Program,
        interface: CognitiveInterface,
        line_base: u32,
        line_count: u32,
        token_count: usize,
    }

    let mut errors: Vec<EmsDiagnostic> = Vec::new();
    let mut compiled: Vec<Compiled> = Vec::new();
    let mut next_base: u32 = 0;

    for path in &graph.order {
        let module = set.get(path).expect("graph order ⊆ set");
        let line_count = (module.source.lines().count() as u32).max(1) + 1;
        let line_base = next_base;
        next_base += line_count;

        let tokens = match Lexer::new(&module.source, &module.origin).tokenize() {
            Ok(t) => t,
            Err(e) => {
                errors.push(EmsDiagnostic {
                    file: module.origin.clone(),
                    line: e.line,
                    column: e.column,
                    message: e.message,
                });
                continue;
            }
        };
        let token_count = tokens.len();
        // The source map: every token (and thus every Loc, every IR
        // source_line) moves into this module's virtual window.
        let tokens: Vec<_> = tokens
            .into_iter()
            .map(|mut t| {
                t.line += line_base;
                t
            })
            .collect();

        let program = match Parser::new(tokens).parse() {
            Ok(p) => p,
            Err(e) => {
                errors.push(EmsDiagnostic {
                    file: module.origin.clone(),
                    line: e.line.saturating_sub(line_base),
                    column: e.column,
                    message: format!("Parse error: {}", e.message),
                });
                continue;
            }
        };

        let interface = generate_interface(path, &program, &module.source);
        compiled.push(Compiled {
            path: path.clone(),
            origin: module.origin.clone(),
            program,
            interface,
            line_base,
            line_count,
            token_count,
        });
    }

    if !errors.is_empty() {
        return Err(EmsFailure {
            errors,
            warnings: Vec::new(),
        });
    }

    // ── Registry + module-mode check context ─────────────────────────
    let mut registry = ModuleRegistry::new();
    for c in &compiled {
        registry.register(c.path.clone(), c.interface.clone());
    }
    let mut ctx = ModuleCheckContext::default();
    for (path, iface) in registry.iter() {
        let exports: BTreeMap<String, String> = iface
            .exports
            .iter()
            .map(|(name, sig)| (name.clone(), sig.kind().to_string()))
            .collect();
        ctx.modules.insert(path.dotted(), exports);
    }

    // ── Pass 2: per-module validation (module mode), cache-aware ─────
    let mut cache = cache_dir.map(CompilationCache::open);
    // Whole-project key: sorted (module, content_hash) pairs. The merged
    // gate may be skipped ONLY when the cache proves the SAME project
    // ended fully clean before (see `CompilationCache::project_warnings`).
    let project_key = {
        let mut basis = String::new();
        for c in &compiled {
            basis.push_str(&c.path.dotted());
            basis.push('\u{1f}');
            basis.push_str(&c.interface.content_hash);
            basis.push('\u{1e}');
        }
        crate::store_schema_manifest::sha256_hex(basis.as_bytes())
    };
    let mut all_hits = true;

    for c in &compiled {
        let dep_interfaces: BTreeMap<String, String> = graph
            .dependencies_of(&c.path)
            .into_iter()
            .filter_map(|d| {
                registry
                    .interface(d)
                    .map(|i| (d.dotted(), i.interface_hash.clone()))
            })
            .collect();

        let hit = cache
            .as_mut()
            .map(|k| {
                k.validation_hit(&c.path.dotted(), &c.interface.content_hash, &dep_interfaces)
            })
            .unwrap_or(false);
        if hit {
            continue;
        }
        all_hits = false;

        let mut checker = match manifest {
            Some(m) => TypeChecker::with_manifest(&c.program, m),
            None => TypeChecker::new(&c.program),
        };
        checker.set_module_context(&ctx);
        let (module_errors, _module_warnings) = checker.check_with_warnings();
        // Module-level warnings are deliberately dropped: global analyses
        // (channel producers, …) can only be judged by the merged gate.
        if module_errors.is_empty() {
            if let Some(k) = cache.as_mut() {
                k.record_clean(
                    &c.path.dotted(),
                    &c.interface.content_hash,
                    dep_interfaces,
                    &c.interface.interface_hash,
                    &c.interface.to_axi_json(),
                );
            }
        } else {
            for e in module_errors {
                errors.push(EmsDiagnostic {
                    file: c.origin.clone(),
                    line: e.line.saturating_sub(c.line_base),
                    column: e.column,
                    message: e.message,
                });
            }
        }
    }

    // ── ECC (pure, always re-runs) ───────────────────────────────────
    let mut warnings: Vec<EmsDiagnostic> = Vec::new();
    for d in check_compatibility(set, &graph, &registry) {
        let diag = EmsDiagnostic {
            file: d.origin,
            line: d.line,
            column: d.column,
            message: d.message,
        };
        match d.severity {
            EccSeverity::Error => errors.push(diag),
            EccSeverity::Warning => warnings.push(diag),
        }
    }

    if !errors.is_empty() {
        if let Some(k) = cache.as_mut() {
            k.clear_project();
            k.flush();
        }
        return Err(EmsFailure { errors, warnings });
    }

    // ── Phase 4: LINK ────────────────────────────────────────────────
    let token_count: usize = compiled.iter().map(|c| c.token_count).sum();
    let module_count = compiled.len();
    let units: Vec<LinkUnit> = compiled
        .into_iter()
        .map(|c| LinkUnit {
            path: c.path,
            origin: c.origin,
            program: c.program,
            line_base: c.line_base,
            line_count: c.line_count,
            interface: c.interface,
        })
        .collect();
    let linked = link(units);
    let declaration_count = linked.program.declarations.len();

    // ── Merged revalidation — the semantic authority ─────────────────
    // Skipped ONLY when every module validation hit AND the cache holds
    // a fully-clean project entry under the same content key — i.e. the
    // previous run of THIS EXACT project passed the merged gate too. A
    // prior cross-module failure clears the entry, so its error can
    // never vanish behind per-module hits.
    let cached_project = if all_hits {
        cache
            .as_ref()
            .and_then(|k| k.project_warnings(&project_key))
    } else {
        None
    };
    match cached_project {
        Some(cached_warnings) => {
            for w in cached_warnings {
                warnings.push(EmsDiagnostic {
                    file: w.file,
                    line: w.line,
                    column: w.column,
                    message: w.message,
                });
            }
        }
        None => {
            let merged_checker = match manifest {
                Some(m) => TypeChecker::with_manifest(&linked.program, m),
                None => TypeChecker::new(&linked.program),
            };
            let (merged_errors, merged_warnings) = merged_checker.check_with_warnings();
            for e in merged_errors {
                let (file, line) = linked.locate(e.line);
                let diag = EmsDiagnostic {
                    file,
                    line,
                    column: e.column,
                    message: e.message,
                };
                if !errors.contains(&diag) {
                    errors.push(diag);
                }
            }
            let mut merged_cached: Vec<crate::compilation_cache::CachedDiagnostic> = Vec::new();
            for w in merged_warnings {
                let (file, line) = linked.locate(w.line);
                merged_cached.push(crate::compilation_cache::CachedDiagnostic {
                    file: file.clone(),
                    line,
                    column: w.column,
                    message: w.message.clone(),
                });
                warnings.push(EmsDiagnostic {
                    file,
                    line,
                    column: w.column,
                    message: w.message,
                });
            }
            if !errors.is_empty() {
                if let Some(k) = cache.as_mut() {
                    k.clear_project();
                    k.flush();
                }
                return Err(EmsFailure { errors, warnings });
            }
            if let Some(k) = cache.as_mut() {
                k.record_project(&project_key, merged_cached);
            }
        }
    }

    // ── IR generation over the linked program ────────────────────────
    let resolution: BTreeMap<String, String> = linked
        .modules
        .iter()
        .map(|m| (m.path.dotted(), m.interface_hash.clone()))
        .collect();
    let mut ir = crate::ir_generator::IRGenerator::new()
        .with_import_resolution(resolution)
        .generate(&linked.program);
    ir.modules = provenance_of(&linked);

    if let Some(k) = cache.as_mut() {
        k.flush();
    }
    let stats = cache.map(|k| k.stats).unwrap_or_default();

    Ok(EmsSuccess {
        ir,
        warnings,
        stats,
        module_count,
        token_count,
        declaration_count,
    })
}

fn provenance_of(linked: &LinkedProgram) -> Vec<IRModuleProvenance> {
    linked
        .modules
        .iter()
        .map(|m| IRModuleProvenance {
            module: m.path.dotted(),
            origin: m.origin.clone(),
            content_hash: m.content_hash.clone(),
            interface_hash: m.interface_hash.clone(),
            line_base: m.line_base,
            line_count: m.line_count,
            declarations: m.export_names.clone(),
        })
        .collect()
}

fn failure_of_module_error(e: &crate::module_resolver::ModuleError) -> EmsFailure {
    EmsFailure {
        errors: vec![EmsDiagnostic {
            file: e.origin.clone(),
            line: e.line,
            column: e.column,
            message: format!("{} {}", e.code, e.message),
        }],
        warnings: Vec::new(),
    }
}
