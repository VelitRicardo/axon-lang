//! v2.76.0 — the import laws (`axon-T953`), driven through the REAL
//! EMS pipeline (`ems::compile_module_set`), never through mocks of it.
//!
//! Every case here was IMPOSSIBLE to express before v2.76.0: in v2.75.0 the
//! type checker matched `Declaration::Import(_) => {}` in both passes and
//! the paper's own two-file example died with `Undefined persona`.

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

const SECURITY: &str = r#"persona Expert {
  domain: ["medicine", "diagnostics"]
  tone: precise
  confidence_threshold: 0.9
}

anchor NoHallucination {
  require: source_citation
  confidence_floor: 0.75
  on_violation: raise AnchorBreachError
}
"#;

#[test]
fn imported_symbols_resolve_a_run() {
    let main = r#"import axon.security.{Expert, NoHallucination}

flow Consultation(symptoms: Document) -> DiagnosticReport {
  step Diagnose {
    given: symptoms
    ask: "Diagnose the patient's symptoms"
    output: DiagnosticReport
  }
}

run Consultation(case_file)
  as Expert
  constrained_by [NoHallucination]
"#;
    let set = set_of(
        &[("main.axon", main), ("axon/security.axon", SECURITY)],
        "main.axon",
    );
    let out = compile_module_set(&set, None).expect("the paper's example must compile");
    assert_eq!(out.module_count, 2);
}

#[test]
fn missing_export_is_t953_with_export_list() {
    let main = "import axon.security.{Ghost}\n";
    let set = set_of(
        &[("main.axon", main), ("axon/security.axon", SECURITY)],
        "main.axon",
    );
    let err = compile_module_set(&set, None).err().expect("must refuse");
    let msg = &err.errors[0].message;
    assert!(msg.contains("axon-T953"), "{msg}");
    assert!(msg.contains("does not export 'Ghost'"), "{msg}");
    assert!(msg.contains("Expert") && msg.contains("NoHallucination"), "{msg}");
}

#[test]
fn missing_module_in_bundle_is_refused_at_assembly() {
    let files: BTreeMap<String, String> =
        [("main.axon".to_string(), "import gone.{X}\n".to_string())].into();
    let err = ModuleSet::from_memory(&files, "main.axon").unwrap_err();
    assert_eq!(err.code, "axon-T953");
    assert!(err.message.contains("gone"));
}

#[test]
fn local_declaration_colliding_with_import_is_t953() {
    let main = "import axon.security.{Expert}\npersona Expert { domain: [\"other\"] }\n";
    let set = set_of(
        &[("main.axon", main), ("axon/security.axon", SECURITY)],
        "main.axon",
    );
    let err = compile_module_set(&set, None).err().expect("must refuse");
    assert!(
        err.errors.iter().any(|e| e.message.contains("axon-T953")
            && e.message.contains("collides")
            && e.message.contains("axon.security")),
        "{:?}",
        err.errors
    );
}

#[test]
fn cross_import_collision_is_t953() {
    let a = "persona Twin { domain: [\"a\"] }\n";
    let b = "persona Twin { domain: [\"b\"] }\n";
    let main = "import liba.{Twin}\nimport libb.{Twin}\n";
    let set = set_of(
        &[("main.axon", main), ("liba.axon", a), ("libb.axon", b)],
        "main.axon",
    );
    let err = compile_module_set(&set, None).err().expect("must refuse");
    assert!(
        err.errors.iter().any(|e| e.message.contains("axon-T953")
            && e.message.contains("imported from both")),
        "{:?}",
        err.errors
    );
}

#[test]
fn non_selective_import_is_refused() {
    // A non-selective import contributes no reachability edge, so the
    // bundle holds only the entry — the refusal comes from the T953
    // selective-import law, with the file untouched.
    let main = "import axon.security\n";
    let set = set_of(&[("main.axon", main)], "main.axon");
    let err = compile_module_set(&set, None).err().expect("must refuse");
    assert!(
        err.errors
            .iter()
            .any(|e| e.message.contains("selective import required")),
        "{:?}",
        err.errors
    );
}

#[test]
fn scoped_import_is_reserved() {
    let main = "import @myscope.pkg.{X}\n";
    let set = set_of(&[("main.axon", main)], "main.axon");
    let err = compile_module_set(&set, None).err().expect("must refuse");
    assert!(
        err.errors.iter().any(|e| e.message.contains("RESERVED")),
        "{:?}",
        err.errors
    );
}

#[test]
fn referencing_without_importing_stays_undefined() {
    // Selectivity law: the persona EXISTS in the linked project, but the
    // entry never imported it — the module-level pass refuses.
    let main = "import axon.security.{NoHallucination}\n\nflow F(d: Document) -> Summary {\n  step S {\n    given: d\n    ask: \"summarize\"\n    output: Summary\n  }\n}\n\nrun F(x) as Expert\n";
    let set = set_of(
        &[("main.axon", main), ("axon/security.axon", SECURITY)],
        "main.axon",
    );
    let err = compile_module_set(&set, None).err().expect("must refuse");
    assert!(
        err.errors
            .iter()
            .any(|e| e.message.contains("Undefined persona 'Expert'")),
        "{:?}",
        err.errors
    );
}

#[test]
fn soft_types_stay_soft_in_module_mode() {
    // `DiagnosticReport` / `Document` are declared nowhere — the house
    // ad-hoc-type idiom must survive module mode.
    let main = "import axon.security.{Expert}\n\nflow F(d: Document) -> DiagnosticReport {\n  step S {\n    given: d\n    ask: \"go\"\n    output: DiagnosticReport\n  }\n}\n\nrun F(x) as Expert\n";
    let set = set_of(
        &[("main.axon", main), ("axon/security.axon", SECURITY)],
        "main.axon",
    );
    compile_module_set(&set, None).expect("ad-hoc types remain accepted");
}
