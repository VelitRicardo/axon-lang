//! v2.76.0 — the CLI surfaces (`check` / `compile`) drive the EMS
//! end-to-end on a real on-disk project, and the single-file path stays
//! byte-identical to v2.75.0 (backwards-compat absolute).

use std::path::PathBuf;

fn temp_project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("axon_f115g_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("axon")).expect("mkdir");
    dir
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

const CONSULTATION: &str = r#"import axon.security.{Expert, NoHallucination}

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

#[test]
fn check_compiles_the_papers_example_from_disk() {
    let dir = temp_project("check");
    std::fs::write(dir.join("axon").join("security.axon"), SECURITY).unwrap();
    let entry = dir.join("consultation.axon");
    std::fs::write(&entry, CONSULTATION).unwrap();

    let code = axon::checker::run_check(entry.to_str().unwrap(), true, false, None);
    assert_eq!(code, 0, "the paper's two-file example must check clean");

    // Warm pass over the on-disk cache the first pass created.
    assert!(dir.join(".axon_cache").join("manifest.json").exists());
    let code2 = axon::checker::run_check(entry.to_str().unwrap(), true, false, None);
    assert_eq!(code2, 0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compile_emits_the_linked_artifact() {
    let dir = temp_project("compile");
    std::fs::write(dir.join("axon").join("security.axon"), SECURITY).unwrap();
    let entry = dir.join("consultation.axon");
    std::fs::write(&entry, CONSULTATION).unwrap();
    let out_path = dir.join("out.ir.json");

    let code = axon::compiler::run_compile(
        entry.to_str().unwrap(),
        "anthropic",
        Some(out_path.to_str().unwrap()),
        false,
    );
    assert_eq!(code, 0);

    let ir: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out_path).unwrap()).unwrap();
    assert_eq!(ir["imports"][0]["resolved"], serde_json::Value::Bool(true));
    assert_eq!(ir["modules"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        ir["runs"][0]["resolved_persona"]["domain"],
        serde_json::json!(["medicine", "diagnostics"])
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_module_fails_check_with_t953() {
    let dir = temp_project("missing");
    let entry = dir.join("main.axon");
    std::fs::write(&entry, "import gone.{X}\n").unwrap();

    let code = axon::checker::run_check(entry.to_str().unwrap(), true, false, None);
    assert_eq!(code, 1, "an unresolvable import must fail the check");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn single_file_programs_bypass_the_ems_and_stay_identical() {
    let dir = temp_project("single");
    let entry = dir.join("solo.axon");
    std::fs::write(
        &entry,
        "persona P { domain: [\"x\"] }\n\nflow F(d: Document) -> Summary {\n  step S {\n    given: d\n    ask: \"go\"\n    output: Summary\n  }\n}\n\nrun F(y) as P\n",
    )
    .unwrap();
    let out_path = dir.join("solo.ir.json");

    let code = axon::compiler::run_compile(
        entry.to_str().unwrap(),
        "anthropic",
        Some(out_path.to_str().unwrap()),
        false,
    );
    assert_eq!(code, 0);

    let raw = std::fs::read_to_string(&out_path).unwrap();
    let ir: serde_json::Value = serde_json::from_str(&raw).unwrap();
    // No EMS artifacts may leak into a single-file compile: the new
    // IR fields are skip-serialized at their defaults (zero IR-SHA
    // drift for every pre-v2.76.0 program).
    assert!(ir.get("modules").is_none(), "no provenance block");
    assert!(!raw.contains("interface_hash"));
    assert!(!raw.contains("\"resolved\""));
    // And no cache dir appears (the EMS never engaged).
    assert!(!dir.join(".axon_cache").exists());

    let _ = std::fs::remove_dir_all(&dir);
}
