//! v2.76.0 — the EMS compilation cache: content-addressed, with
//! GHC-style early cutoff. Nix model: a compile is a pure function of its
//! inputs, so matching inputs ⇒ the recorded outcome is the outcome.
//!
//! # What is actually cached (stated precisely, so the claim can be true)
//!
//! The unit of *persisted skip* is a module's **validation** (the
//! type-check — by far the expensive pass; the 680k-line checker dwarfs
//! parse + IR generation). Parsing and IR generation always re-run: IR
//! nodes are Serialize-only by design (consumers re-derive from source),
//! so the honest cache skips what it can prove skippable and recomputes
//! the cheap, total passes.
//!
//! - **Module validation hit** — a module whose `content_hash` AND
//!   dependency `interface_hash` set match a recorded CLEAN validation
//!   skips its per-module type-check.
//! - **Early cutoff** — a dependency edited without changing its public
//!   surface (comment, body-only edit) keeps its `interface_hash`, so
//!   every dependent's key still matches: the dependents skip
//!   re-validation. This is real, observable via [`CacheStats`], and
//!   sound because per-module validation consumes only interface facts
//! (v2.76.0) — nothing body-derived is cached per-dependent.
//! - **The merged revalidation re-runs whenever any module changed.**
//!   Cross-module semantics are global; v1 does not scope it. When NO
//!   module changed, the project-level entry marks the whole compile
//!   clean and the driver skips validation entirely.
//!
//! # Laws
//!
//! 1. Source hash changed → module miss.
//! 2. Any dependency interface hash changed → module miss.
//! 3. Both match a recorded clean validation → hit.
//! 4. Dependency source changed, interface stable → dependents still hit
//!    (early cutoff).
//! 5. Writes are atomic (`.tmp` + rename). A corrupt or unreadable cache
//!    is NOT an error: it self-heals by re-deriving from source (the
//!    boot-hydrate doctrine — a cache is never the source of truth).
//! 6. The manifest pins `schema_version` + `axi_format` + the compiler
//!    version; any mismatch busts the cache wholesale.
//!
//! Only CLEAN validations are recorded: a failing module re-validates
//! every run so diagnostics re-emit from source, never from a replay.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::module_interface::AXI_FORMAT_VERSION;

/// On-disk manifest schema version.
pub const CACHE_SCHEMA_VERSION: u32 = 1;

/// Cache directory name, created beside the entry file.
pub const CACHE_DIR_NAME: &str = ".axon_cache";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModuleEntry {
    content_hash: String,
    /// Dotted dependency path → its interface hash at validation time.
    dep_interfaces: BTreeMap<String, String>,
    interface_hash: String,
}

/// One persisted diagnostic (merged-gate warnings only — errors are
/// NEVER cached; a failing compile re-derives from source every run).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CachedDiagnostic {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub message: String,
}

/// The whole-project entry: recorded ONLY after a fully-clean compile
/// (per-module passes AND the merged revalidation). This is what makes
/// the full-hit skip of the merged gate SOUND: a project whose previous
/// run ended in a cross-module error has no entry, so the merged gate
/// re-runs and the error re-emits.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectEntry {
    /// SHA-256 over the sorted (module, content_hash) pairs.
    key: String,
    merged_warnings: Vec<CachedDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheManifest {
    schema_version: u32,
    axi_format: u32,
    compiler_version: String,
    /// Dotted module path → its last CLEAN validation inputs.
    modules: BTreeMap<String, ModuleEntry>,
    /// The last fully-clean whole-project compile, if any.
    #[serde(default)]
    project: Option<ProjectEntry>,
}

impl CacheManifest {
    fn fresh() -> Self {
        CacheManifest {
            schema_version: CACHE_SCHEMA_VERSION,
            axi_format: AXI_FORMAT_VERSION,
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
            modules: BTreeMap::new(),
            project: None,
        }
    }

    fn is_current(&self) -> bool {
        self.schema_version == CACHE_SCHEMA_VERSION
            && self.axi_format == AXI_FORMAT_VERSION
            && self.compiler_version == env!("CARGO_PKG_VERSION")
    }
}

/// Observable cache behavior — the tests' witness that the laws run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Modules whose validation was skipped (laws 3–4).
    pub validation_hits: usize,
    /// Modules that re-validated (laws 1–2, or first sight).
    pub validation_misses: usize,
    /// Subset of hits where at least one dependency's SOURCE had changed
    /// while its interface stayed stable (law 4 — the early cutoff).
    pub early_cutoffs: usize,
}

/// The EMS compilation cache. All operations are infallible by contract:
/// any I/O or shape problem degrades to "no cache" (law 5).
pub struct CompilationCache {
    root: PathBuf,
    manifest: CacheManifest,
    /// Content hashes seen by the PREVIOUS manifest, kept to detect the
    /// early-cutoff condition before entries are overwritten.
    previous_content: BTreeMap<String, String>,
    pub stats: CacheStats,
    dirty: bool,
}

impl CompilationCache {
    /// Open (or initialize) the cache under `dir` (typically
    /// `<entry dir>/.axon_cache`). Never fails: a corrupt manifest, a
    /// version mismatch, or an unreadable directory yields a fresh cache.
    pub fn open(dir: &Path) -> CompilationCache {
        let manifest_path = dir.join("manifest.json");
        let manifest = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|s| serde_json::from_str::<CacheManifest>(&s).ok())
            .filter(CacheManifest::is_current)
            .unwrap_or_else(CacheManifest::fresh);
        let previous_content = manifest
            .modules
            .iter()
            .map(|(k, v)| (k.clone(), v.content_hash.clone()))
            .collect();
        CompilationCache {
            root: dir.to_path_buf(),
            manifest,
            previous_content,
            stats: CacheStats::default(),
            dirty: false,
        }
    }

    /// Law 3/4 — may this module skip re-validation? Records the
    /// hit/miss/early-cutoff in [`CacheStats`].
    pub fn validation_hit(
        &mut self,
        module: &str,
        content_hash: &str,
        dep_interfaces: &BTreeMap<String, String>,
    ) -> bool {
        let hit = self
            .manifest
            .modules
            .get(module)
            .map(|e| e.content_hash == content_hash && &e.dep_interfaces == dep_interfaces)
            .unwrap_or(false);
        if hit {
            self.stats.validation_hits += 1;
            // Early cutoff: some dependency's source changed under a
            // stable interface. Detect against the previous manifest.
            let cutoff = dep_interfaces.keys().any(|dep| {
                match (
                    self.previous_content.get(dep),
                    self.manifest.modules.get(dep),
                ) {
                    // The dep re-validated this run under a NEW content
                    // hash while our recorded interface for it matched —
                    // i.e. body changed, surface stable.
                    (Some(prev), Some(entry)) => &entry.content_hash != prev,
                    _ => false,
                }
            });
            if cutoff {
                self.stats.early_cutoffs += 1;
            }
        } else {
            self.stats.validation_misses += 1;
        }
        hit
    }

    /// Record a CLEAN validation (never a failing one — diagnostics must
    /// re-emit from source) and persist the module's `.axi`.
    pub fn record_clean(
        &mut self,
        module: &str,
        content_hash: &str,
        dep_interfaces: BTreeMap<String, String>,
        interface_hash: &str,
        axi_json: &str,
    ) {
        self.manifest.modules.insert(
            module.to_string(),
            ModuleEntry {
                content_hash: content_hash.to_string(),
                dep_interfaces,
                interface_hash: interface_hash.to_string(),
            },
        );
        self.dirty = true;
        let axi_dir = self.root.join("interfaces");
        let _ = std::fs::create_dir_all(&axi_dir);
        let _ = atomic_write(&axi_dir.join(format!("{module}.axi")), axi_json.as_bytes());
    }

    /// The recorded merged-gate warnings for a fully-clean project whose
    /// key matches — `Some` authorizes skipping the merged revalidation
    /// (its outcome is provably identical), `None` demands it re-run
    /// (contents changed, or the previous run was not fully clean).
    pub fn project_warnings(&self, key: &str) -> Option<Vec<CachedDiagnostic>> {
        self.manifest
            .project
            .as_ref()
            .filter(|p| p.key == key)
            .map(|p| p.merged_warnings.clone())
    }

    /// Record a fully-clean whole-project compile (per-module passes AND
    /// merged gate) with the merged gate's warnings.
    pub fn record_project(&mut self, key: &str, merged_warnings: Vec<CachedDiagnostic>) {
        self.manifest.project = Some(ProjectEntry {
            key: key.to_string(),
            merged_warnings,
        });
        self.dirty = true;
    }

    /// Any compile that did NOT end fully clean must drop the project
    /// entry, so the merged gate re-runs next time (soundness of the
    /// full-hit skip).
    pub fn clear_project(&mut self) {
        if self.manifest.project.is_some() {
            self.manifest.project = None;
            self.dirty = true;
        }
    }

    /// Flush the manifest (atomic). Infallible by contract.
    pub fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        let _ = std::fs::create_dir_all(&self.root);
        if let Ok(json) = serde_json::to_string_pretty(&self.manifest) {
            let _ = atomic_write(&self.root.join("manifest.json"), json.as_bytes());
        }
        self.dirty = false;
    }
}

/// Atomic file write: temp sibling + rename (law 5). Windows rename over
/// an existing file fails, so the stale target is removed first — the
/// worst crash outcome is a MISSING cache entry, which self-heals.
fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    let _ = std::fs::remove_file(path);
    std::fs::rename(&tmp, path)
}

// ════════════════════════════════════════════════════════════════════
//  Unit tests (integration suite: tests/cache_laws.rs)
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn deps(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn miss_then_hit_then_source_invalidation() {
        let dir = std::env::temp_dir().join(format!(
            "axon_cache_test_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let mut c = CompilationCache::open(&dir);
        assert!(!c.validation_hit("m", "h1", &deps(&[])));
        c.record_clean("m", "h1", deps(&[]), "i1", "{}");
        c.flush();

        let mut c2 = CompilationCache::open(&dir);
        assert!(c2.validation_hit("m", "h1", &deps(&[])), "law 3");
        assert!(!c2.validation_hit("m", "h2", &deps(&[])), "law 1");
        assert_eq!(c2.stats.validation_hits, 1);
        assert_eq!(c2.stats.validation_misses, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dependency_interface_invalidates() {
        let dir = std::env::temp_dir().join(format!(
            "axon_cache_dep_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let mut c = CompilationCache::open(&dir);
        c.record_clean("main", "h1", deps(&[("lib", "i1")]), "im", "{}");
        c.flush();

        let mut c2 = CompilationCache::open(&dir);
        assert!(c2.validation_hit("main", "h1", &deps(&[("lib", "i1")])));
        assert!(!c2.validation_hit("main", "h1", &deps(&[("lib", "i2")])), "law 2");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_manifest_self_heals() {
        let dir = std::env::temp_dir().join(format!(
            "axon_cache_heal_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), b"{ not json").unwrap();

        let mut c = CompilationCache::open(&dir); // law 5: no panic, no error
        assert!(!c.validation_hit("m", "h1", &deps(&[])));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn schema_version_busts_wholesale() {
        let dir = std::env::temp_dir().join(format!(
            "axon_cache_ver_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let stale = serde_json::json!({
            "schema_version": 0,
            "axi_format": 0,
            "compiler_version": "0.0.0",
            "modules": { "m": { "content_hash": "h1", "dep_interfaces": {}, "interface_hash": "i1" } }
        });
        std::fs::write(dir.join("manifest.json"), stale.to_string()).unwrap();

        let mut c = CompilationCache::open(&dir);
        assert!(!c.validation_hit("m", "h1", &deps(&[])), "law 6");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
