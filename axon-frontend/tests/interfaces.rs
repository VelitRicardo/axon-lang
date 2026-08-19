//! v2.76.0 — the `.axi` interface tier through the REAL driver, and
//! the KIND-PARITY gate: an imported name's exported kind must be the
//! EXACT string the type-checker's symbol table expects, or every
//! kind-sensitive reference law (T950 and friends) would misfire on
//! imports. These references only pass when parity holds.

use std::collections::BTreeMap;

use axon_frontend::ems::compile_module_set;
use axon_frontend::module_resolver::ModuleSet;

fn set_of(pairs: &[(&str, &str)], entry: &str) -> ModuleSet {
    let files: BTreeMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    ModuleSet::from_memory(&files, entry).expect("module set")
}

#[test]
fn kind_parity_run_family() {
    // flow / persona / context / anchor — all four kinds referenced by a
    // `run`, all four imported. Any kind-string drift ⇒ "is a X, not a Y".
    let lib = r#"persona P { domain: ["d"] }

context C {
  memory: session
  depth: exhaustive
}

anchor A {
  require: source_citation
}

flow F(d: Document) -> Summary {
  step S {
    given: d
    ask: "go"
    output: Summary
  }
}
"#;
    let main = "import lib.{F, P, C, A}\n\nrun F(x)\n  as P\n  within C\n  constrained_by [A]\n";
    let set = set_of(&[("main.axon", main), ("lib.axon", lib)], "main.axon");
    compile_module_set(&set, None).expect("run-family kinds are parity-exact");
}

#[test]
fn kind_parity_resource_on_tool() {
    // axon-T950 demands symbol kind == "resource" EXACTLY.
    let lib = "resource Api {\n  kind: https\n  endpoint: vendor.api.base\n  capacity: 4\n  lifetime: persistent\n}\n";
    let main = "import lib.{Api}\n\ntool Search {\n  provider: http\n  resource: Api\n  runtime: search\n  timeout: 10s\n}\n";
    let set = set_of(&[("main.axon", main), ("lib.axon", lib)], "main.axon");
    compile_module_set(&set, None).expect("resource kind is parity-exact (T950)");
}

#[test]
fn kind_mismatch_still_blames_correctly_across_modules() {
    // Importing a PERSONA and naming it as a tool's resource must produce
    // the same "is a persona, not a resource" the local case produces.
    let lib = "persona Api { domain: [\"d\"] }\n";
    let main = "import lib.{Api}\n\ntool Search {\n  provider: http\n  resource: Api\n  runtime: search\n  timeout: 10s\n}\n";
    let set = set_of(&[("main.axon", main), ("lib.axon", lib)], "main.axon");
    let err = compile_module_set(&set, None).err().expect("must refuse");
    assert!(
        err.errors
            .iter()
            .any(|e| e.message.contains("is a persona, not a resource")),
        "{:?}",
        err.errors
    );
}

#[test]
fn axi_files_persist_under_the_cache() {
    let dir = std::env::temp_dir().join(format!("axon_axi_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let lib = "anchor A { require: source_citation }\n";
    let main = "import lib.{A}\n";
    let set = set_of(&[("main.axon", main), ("lib.axon", lib)], "main.axon");
    compile_module_set(&set, Some(&dir)).expect("compiles");

    let axi = std::fs::read_to_string(dir.join("interfaces").join("lib.axi"))
        .expect(".axi persisted for the dependency");
    let parsed = axon_frontend::module_interface::CognitiveInterface::from_axi_json(&axi)
        .expect(".axi parses back");
    assert_eq!(parsed.module, "lib");
    assert_eq!(parsed.epistemic_floor, axon_frontend::module_interface::EpistemicFloor::Know);
    assert!(parsed.exports.contains_key("A"));
    assert!(!axi.contains("source_citation"), "anchor text never leaves the module");

    let _ = std::fs::remove_dir_all(&dir);
}
