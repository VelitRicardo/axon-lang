//! v2.76.0 — the LINK is faithful: the anti-stub proof.
//!
//! The retired Python EMS injected signature stubs into the IR — a stub
//! of a flow has no steps, so an imported flow could never have executed.
//! The Rust linker merges full declarations at the AST tier and the IR
//! generates ONCE over the linked program, so these assertions hold by
//! construction — and this suite pins them against the REAL pipeline.

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

fn paper_example() -> ModuleSet {
    set_of(
        &[
            ("consultation.axon", CONSULTATION),
            ("axon/security.axon", SECURITY),
        ],
        "consultation.axon",
    )
}

#[test]
fn imported_persona_reaches_resolved_persona_with_full_body() {
    let out = compile_module_set(&paper_example(), None).expect("compiles");
    let run = &out.ir.runs[0];
    let persona = run.resolved_persona.as_ref().expect("persona resolved");
    assert_eq!(persona.name, "Expert");
    // FULL body, not a stub: the second domain entry survives the link.
    assert_eq!(persona.domain, vec!["medicine", "diagnostics"]);
    assert_eq!(persona.confidence_threshold, Some(0.9));
    assert_eq!(run.resolved_anchors.len(), 1);
    assert_eq!(run.resolved_anchors[0].name, "NoHallucination");
}

#[test]
fn imported_flow_steps_survive_the_link() {
    let lib = "flow Greet(name: String) -> Summary {\n  step Hello {\n    given: name\n    ask: \"greet\"\n    output: Summary\n  }\n}\n";
    let main = "import lib.{Greet}\n\nrun Greet(x)\n";
    let set = set_of(&[("main.axon", main), ("lib.axon", lib)], "main.axon");
    let out = compile_module_set(&set, None).expect("compiles");
    let flow = out
        .ir
        .flows
        .iter()
        .find(|f| f.name == "Greet")
        .expect("imported flow linked");
    assert!(!flow.steps.is_empty(), "the anti-stub proof: steps must survive");
    let run = &out.ir.runs[0];
    let resolved = run.resolved_flow.as_ref().expect("flow resolved");
    assert!(!resolved.steps.is_empty());
}

#[test]
fn provenance_and_import_marks_ride_the_ir() {
    let out = compile_module_set(&paper_example(), None).expect("compiles");
    assert_eq!(out.ir.modules.len(), 2);
    let dep = out
        .ir
        .modules
        .iter()
        .find(|m| m.module == "axon.security")
        .expect("dep provenance");
    assert_eq!(dep.declarations, vec!["Expert", "NoHallucination"]);
    assert_eq!(dep.content_hash.len(), 64);
    assert_eq!(dep.interface_hash.len(), 64);

    let import = &out.ir.imports[0];
    assert!(import.resolved, "IRImport.resolved — the field the paper promised");
    assert_eq!(
        import.interface_hash.as_deref(),
        Some(dep.interface_hash.as_str())
    );
}

#[test]
fn linked_artifact_is_deterministic_bytes() {
    let a = compile_module_set(&paper_example(), None).expect("compiles");
    let b = compile_module_set(&paper_example(), None).expect("compiles");
    let ja = serde_json::to_string(&a.ir).unwrap();
    let jb = serde_json::to_string(&b.ir).unwrap();
    assert_eq!(ja, jb, "same inputs ⇒ same linked bytes (section 4.4)");
}

#[test]
fn dependency_runs_do_not_execute_in_the_link() {
    let lib = "persona P { domain: [\"x\"] }\n\nflow Demo(d: Document) -> Summary {\n  step S {\n    given: d\n    ask: \"demo\"\n    output: Summary\n  }\n}\n\nrun Demo(sample)\n";
    let main = "import lib.{P}\n";
    let set = set_of(&[("main.axon", main), ("lib.axon", lib)], "main.axon");
    let out = compile_module_set(&set, None).expect("compiles");
    assert!(
        out.ir.runs.is_empty(),
        "a library's top-level runs are its own demos — only the entry orchestrates"
    );
}

#[test]
fn cross_module_deep_check_fires_at_the_merged_gate_with_entry_mapping() {
    // The tool's parameter schema lives in the dependency; the entry
    // calls it with a wrong argument name. The per-module pass cannot
    // see the schema (body-dependent second stages skip on foreign
    // subjects) — the merged gate MUST catch it, and the diagnostic must
    // map back to the ENTRY file.
    let lib = "tool Search {\n  provider: http\n  parameters: { query: String }\n  timeout: 10s\n}\n";
    let main = "import lib.{Search}\n\nflow F(d: Document) -> Summary {\n  use Search(wrong_arg = d)\n  step S {\n    given: d\n    ask: \"go\"\n    output: Summary\n  }\n}\n\nrun F(x)\n";
    let set = set_of(&[("main.axon", main), ("lib.axon", lib)], "main.axon");
    let err = compile_module_set(&set, None).err().expect("must refuse");
    let hit = err
        .errors
        .iter()
        .find(|e| e.message.contains("has no parameter 'wrong_arg'"))
        .unwrap_or_else(|| panic!("merged gate must catch the schema violation: {:?}", err.errors));
    assert_eq!(hit.file, "main.axon", "diagnostic maps to the entry module");
    assert!(hit.line >= 3, "line is module-local, not virtual: {}", hit.line);
}
