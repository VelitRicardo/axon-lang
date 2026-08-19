//! v2.43.0 — drift gate for the `warden` + `scope` docs.
//!
//! The canonical programs published in `knowledge/primitives/warden.md` and
//! `knowledge/primitives/scope.md` must round-trip through the same
//! `axon-frontend` pipeline the `axon` CLI uses — the "published grammar MUST
//! compile" discipline. A doc example that does not compile is a lie the corpus
//! must never ship.

use axon_emcp::compiler_pipeline::{run, Outcome};

fn must_compile(label: &str, source: &str) {
    match run(source, label) {
        Outcome::Ok { .. } => {}
        Outcome::Err {
            stage,
            errors,
            warnings,
        } => panic!(
            "{label}: expected well-formed program, got {stage:?} failure:\n\
             errors   = {errors:#?}\n\
             warnings = {warnings:#?}\n\
             source   = {source}"
        ),
    }
}

/// The published warden.md + scope.md example: a warden analysis running within
/// a well-formed authorization scope.
#[test]
fn warden_doc_example_compiles() {
    let src = r#"
scope InternalAudit {
    targets: [ "svc://payments-core" ]
    depth: static_artifact
    approver: requires "security.lead"
}

flow Audit() -> Unit {
    warden(payments_core) within InternalAudit {
        step Analyse { ask: "enumerate contract violations" }
    }
}
"#;
    must_compile("warden/canonical", src);
}

/// A standalone `scope` declaration (the scope.md example).
#[test]
fn scope_doc_example_compiles() {
    let src = r#"
scope InternalAudit {
    targets: [ "svc://payments-core" ]
    depth: static_artifact
    approver: requires "security.lead"
}
"#;
    must_compile("scope/canonical", src);
}
