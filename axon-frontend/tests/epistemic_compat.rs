//! v2.76.0 — the ECC through the REAL driver: errors fail the
//! compile, valve-acknowledged downgrades stay VISIBLE as warnings.

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

const SPECULATE_LIB: &str = "speculate {\n  persona Wild { domain: [\"ideas\"] }\n}\n";

#[test]
fn severe_downgrade_fails_the_compile() {
    let main = "import lib.{Wild}\nanchor Strict { require: source_citation }\n";
    let set = set_of(&[("main.axon", main), ("lib.axon", SPECULATE_LIB)], "main.axon");
    let err = compile_module_set(&set, None).err().expect("must refuse");
    assert!(
        err.errors
            .iter()
            .any(|e| e.message.contains("axon-T954") || e.message.contains("epistemic conflict")),
        "{:?}",
        err.errors
    );
}

#[test]
fn valve_downgrades_to_a_visible_warning_and_compiles() {
    let main =
        "import lib.{Wild} @allow_downgrade\nanchor Strict { require: source_citation }\n";
    let set = set_of(&[("main.axon", main), ("lib.axon", SPECULATE_LIB)], "main.axon");
    let out = compile_module_set(&set, None).expect("valve permits the compile");
    assert!(
        out.warnings
            .iter()
            .any(|w| w.message.contains("acknowledged severe epistemic downgrade")),
        "the downgrade must stay VISIBLE: {:?}",
        out.warnings
    );
}

#[test]
fn small_gap_warns_but_compiles() {
    let lib = "shield Soft { scan: [pii_leak] on_breach: halt }\npersona Wild { domain: [\"ideas\"] }\n";
    let main = "import lib.{Wild}\nanchor Strict { require: source_citation }\n";
    let set = set_of(&[("main.axon", main), ("lib.axon", lib)], "main.axon");
    let out = compile_module_set(&set, None).expect("gap 1 is a warning, not an error");
    assert!(
        out.warnings
            .iter()
            .any(|w| w.message.contains("epistemic downgrade")),
        "{:?}",
        out.warnings
    );
}
