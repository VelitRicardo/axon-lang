//! Phase 4 — every template shipped under `src/knowledge/templates/`
//! must compile end-to-end through the same `axon-frontend` pipeline
//! the `axon` CLI uses.
//!
//! This is the drift gate for `axon.compose`: the moment a template
//! stops parsing the test fails, so the agent never receives a
//! malformed scaffold even mid-refactor.

use axon_emcp::compiler_pipeline::{run, Outcome};

/// Locate the workspace's templates directory relative to this test
/// crate. The compose tool itself uses the catalogue, but for the
/// drift gate we read files directly so the test is independent of
/// catalogue plumbing.
fn templates_dir() -> std::path::PathBuf {
    // Release 0.2.0 — the corpus lives INSIDE the crate
    // (`src/axon-emcp/knowledge/`) so `cargo publish` ships it.
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("knowledge")
        .join("templates")
}

fn assert_template_compiles(slug: &str) {
    let path = templates_dir().join(format!("{slug}.axon"));
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("template {slug} missing at {}: {e}", path.display()));
    match run(&src, &format!("{slug}.axon")) {
        Outcome::Ok { .. } => { /* compiles clean — the whole assertion */ }
        Outcome::Err { stage, errors, warnings } => panic!(
            "template `{slug}` failed at {stage:?}:\n\
             errors   = {errors:#?}\n\
             warnings = {warnings:#?}\n\
             path     = {}",
            path.display()
        ),
    }
}

#[test]
fn template_generic_compiles() {
    assert_template_compiles("generic");
}

#[test]
fn template_healthcare_compiles() {
    assert_template_compiles("healthcare");
}

#[test]
fn template_banking_compiles() {
    assert_template_compiles("banking");
}

#[test]
fn template_government_compiles() {
    assert_template_compiles("government");
}

#[test]
fn template_legal_compiles() {
    assert_template_compiles("legal");
}

#[test]
fn template_chat_compiles() {
    assert_template_compiles("chat");
}

#[test]
fn template_retrieval_compiles() {
    assert_template_compiles("retrieval");
}

#[test]
fn template_multi_agent_compiles() {
    assert_template_compiles("multi_agent");
}

// v1.2.0 — vertical extension templates.

#[test]
fn template_legaltech_compiles() {
    assert_template_compiles("legaltech");
}

#[test]
fn template_fintech_compiles() {
    assert_template_compiles("fintech");
}

#[test]
fn template_pharmatech_compiles() {
    assert_template_compiles("pharmatech");
}

#[test]
fn template_medic_research_compiles() {
    assert_template_compiles("medic_research");
}

// v1.2.0 — agent-pattern templates.

#[test]
fn template_chat_research_compiles() {
    assert_template_compiles("chat_research");
}

#[test]
fn template_chat_tools_compiles() {
    assert_template_compiles("chat_tools");
}

#[test]
fn template_chat_skills_compiles() {
    assert_template_compiles("chat_skills");
}

#[test]
fn template_whatsapp_compiles() {
    assert_template_compiles("whatsapp");
}

#[test]
fn template_voice_compiles() {
    assert_template_compiles("voice");
}

/// v2.37.0 — the preset-based voice agent: the whole program is one
/// `voice` declaration; the expansion (ots + session/socket + upstreams)
/// must compile end-to-end through the same pipeline.
#[test]
fn template_voice_preset_compiles() {
    assert_template_compiles("voice_preset");
}

#[test]
fn template_dev_compiles() {
    assert_template_compiles("dev");
}

#[test]
fn template_sales_consultive_compiles() {
    assert_template_compiles("sales_consultive");
}

#[test]
fn template_sales_widget_compiles() {
    assert_template_compiles("sales_widget");
}

// v1.2.0 — application-pattern templates (closes the cycle at 33/33).

#[test]
fn template_workflow_automation_compiles() {
    assert_template_compiles("workflow_automation");
}

#[test]
fn template_business_intelligence_compiles() {
    assert_template_compiles("business_intelligence");
}

#[test]
fn template_corporate_integration_compiles() {
    assert_template_compiles("corporate_integration");
}

#[test]
fn template_self_learning_compiles() {
    assert_template_compiles("self_learning");
}

#[test]
fn template_document_analysis_compiles() {
    assert_template_compiles("document_analysis");
}

#[test]
fn template_ticket_triage_compiles() {
    assert_template_compiles("ticket_triage");
}

#[test]
fn template_content_moderation_compiles() {
    assert_template_compiles("content_moderation");
}

#[test]
fn template_knowledge_extraction_compiles() {
    assert_template_compiles("knowledge_extraction");
}

#[test]
fn template_compliance_monitoring_compiles() {
    assert_template_compiles("compliance_monitoring");
}

#[test]
fn template_recruitment_compiles() {
    assert_template_compiles("recruitment");
}

#[test]
fn template_education_compiles() {
    assert_template_compiles("education");
}

#[test]
fn template_financial_advisor_compiles() {
    assert_template_compiles("financial_advisor");
}

#[test]
fn template_data_pipeline_compiles() {
    assert_template_compiles("data_pipeline");
}
