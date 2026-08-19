//! v2.76.0 — the cache laws observed through the REAL
//! driver via [`CacheStats`] — the tests' witness that the laws run.

use std::collections::BTreeMap;
use std::path::PathBuf;

use axon_frontend::ems::compile_module_set;
use axon_frontend::module_resolver::ModuleSet;

fn set_of(pairs: &[(&str, &str)], entry: &str) -> ModuleSet {
    let files: BTreeMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    ModuleSet::from_memory(&files, entry).expect("module set")
}

fn temp_cache(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("axon_ems_cache_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

const LIB: &str = "persona P { domain: [\"d\"] }\nanchor A { require: source_citation }\n";
const MAIN: &str = "import lib.{P, A}\n\nflow F(d: Document) -> Summary {\n  step S {\n    given: d\n    ask: \"go\"\n    output: Summary\n  }\n}\n\nrun F(x) as P constrained_by [A]\n";

#[test]
fn cold_then_warm_then_source_invalidation() {
    let dir = temp_cache("basic");
    let set = set_of(&[("main.axon", MAIN), ("lib.axon", LIB)], "main.axon");

    // Cold: every module validates.
    let cold = compile_module_set(&set, Some(&dir)).expect("cold compiles");
    assert_eq!(cold.stats.validation_misses, 2);
    assert_eq!(cold.stats.validation_hits, 0);

    // Warm: nothing changed — full hit (laws 3 + the sound merged-gate skip).
    let warm = compile_module_set(&set, Some(&dir)).expect("warm compiles");
    assert_eq!(warm.stats.validation_hits, 2);
    assert_eq!(warm.stats.validation_misses, 0);

    // Law 1: editing the ENTRY invalidates the entry only.
    let edited = MAIN.replace("\"go\"", "\"proceed\"");
    let set2 = set_of(&[("main.axon", edited.as_str()), ("lib.axon", LIB)], "main.axon");
    let after = compile_module_set(&set2, Some(&dir)).expect("compiles");
    assert_eq!(after.stats.validation_hits, 1, "lib untouched — still a hit");
    assert_eq!(after.stats.validation_misses, 1, "entry re-validates");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn early_cutoff_on_comment_only_dependency_edit() {
    let dir = temp_cache("cutoff");
    let set = set_of(&[("main.axon", MAIN), ("lib.axon", LIB)], "main.axon");
    compile_module_set(&set, Some(&dir)).expect("cold");

    // A comment changes lib's CONTENT hash but not its INTERFACE hash.
    let lib_commented = format!("// a comment\n{LIB}");
    let set2 = set_of(
        &[("main.axon", MAIN), ("lib.axon", lib_commented.as_str())],
        "main.axon",
    );
    let warm = compile_module_set(&set2, Some(&dir)).expect("compiles");
    assert_eq!(warm.stats.validation_misses, 1, "lib re-validates (source changed)");
    assert_eq!(warm.stats.validation_hits, 1, "entry SKIPS — law 4");
    assert_eq!(warm.stats.early_cutoffs, 1, "and it is counted as an early cutoff");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn interface_change_invalidates_dependents() {
    let dir = temp_cache("iface");
    let set = set_of(&[("main.axon", MAIN), ("lib.axon", LIB)], "main.axon");
    compile_module_set(&set, Some(&dir)).expect("cold");

    // Changing the anchor's constraint changes lib's INTERFACE hash
    // (constraint_hash is part of the signature) — law 2 for the entry.
    let lib_changed = LIB.replace("source_citation", "factual_claims_only");
    let set2 = set_of(
        &[("main.axon", MAIN), ("lib.axon", lib_changed.as_str())],
        "main.axon",
    );
    let warm = compile_module_set(&set2, Some(&dir)).expect("compiles");
    assert_eq!(warm.stats.validation_misses, 2, "law 2: dependent invalidates too");
    assert_eq!(warm.stats.validation_hits, 0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn corrupt_cache_self_heals_through_the_driver() {
    let dir = temp_cache("heal");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("manifest.json"), b"{ definitely not json").unwrap();

    let set = set_of(&[("main.axon", MAIN), ("lib.axon", LIB)], "main.axon");
    let out = compile_module_set(&set, Some(&dir)).expect("law 5: corruption is not an error");
    assert_eq!(out.stats.validation_misses, 2, "fresh cache — everything validates");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failing_project_never_caches_its_way_to_green() {
    let dir = temp_cache("nogreen");

    // First run: cross-module ERROR at the merged gate (wrong tool arg).
    let lib = "tool Search {\n  provider: http\n  parameters: { query: String }\n  timeout: 10s\n}\n";
    let bad_main = "import lib.{Search}\n\nflow F(d: Document) -> Summary {\n  use Search(wrong = d)\n  step S {\n    given: d\n    ask: \"go\"\n    output: Summary\n  }\n}\n\nrun F(x)\n";
    let set = set_of(&[("main.axon", bad_main), ("lib.axon", lib)], "main.axon");
    compile_module_set(&set, Some(&dir)).err().expect("refuses");

    // Second run, unchanged: the SAME error must re-emit (the soundness
    // property of the full-hit skip — a prior failure clears the project
    // entry, so the merged gate re-runs).
    let again = compile_module_set(&set, Some(&dir)).err().expect("still refuses");
    assert!(
        again
            .errors
            .iter()
            .any(|e| e.message.contains("has no parameter 'wrong'")),
        "{:?}",
        again.errors
    );

    let _ = std::fs::remove_dir_all(&dir);
}
