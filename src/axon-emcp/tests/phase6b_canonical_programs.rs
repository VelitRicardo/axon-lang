//! v1.2.0 — drift gate for the Tier 1 primitive docs.
//!
//! Every primitive doc shipped under `src/knowledge/primitives/`
//! must be backed by a canonical `.axon` program that round-trips
//! through the same `axon-frontend` pipeline the `axon` CLI uses.
//! This file ships one test per Tier 1 primitive: `context`,
//! `intent`, `memory`, `agent`, `probe`, `validate`, `refine`,
//! `weave`, `type`, `run`.
//!
//! Pattern mirrors `phase2_canonical_programs.rs` (the seven Tier 0
//! primitives' drift gate). The instant a Tier 1 doc's claimed
//! grammar stops parsing, the matching test below fails.

use axon_emcp::compiler_pipeline::{run, Outcome};

/// Run a canonical program through the full pipeline; panic with
/// structured diagnostics on failure. The phrasing of the panic
/// mirrors what an agent sees through `axon.check` — same
/// diagnostic surface, same vocabulary.
fn must_compile(label: &str, source: &str) {
    match run(source, label) {
        Outcome::Ok { .. } => { /* well-formed — the whole assertion */ }
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

#[test]
fn context_canonical_program_compiles() {
    // Mirrors the example in `context.md`. Exercises every field
    // documented in the YAML grammar block.
    let src = r#"
context LegalReview {
    memory: session
    language: "en"
    depth: exhaustive
    max_tokens: 4096
    temperature: 0.3
    cite_sources: true
}
"#;
    must_compile("context/canonical", src);
}

#[test]
fn intent_canonical_program_compiles() {
    let src = r#"
type ContractSummary { text: String }

intent SummarizeContract {
    given: doc
    ask: "Produce a one-page executive summary of the contract."
    output: ContractSummary
    confidence_floor: 0.8
}
"#;
    must_compile("intent/canonical", src);
}

#[test]
fn memory_canonical_program_compiles() {
    // `store:` is the closed lifecycle catalogue
    // (ephemeral | none | persistent | session) — NOT a store-kind
    // catalogue. The store kind is picked by `backend:`.
    let src = r#"
memory ClientNotes {
    store: persistent
    backend: pgvector
    retrieval: semantic
    decay: never
}
"#;
    must_compile("memory/canonical-never", src);
}

#[test]
fn memory_canonical_program_with_duration_decay() {
    let src = r#"
memory ShortLived {
    store: session
    retrieval: exact
    decay: 24h
}
"#;
    must_compile("memory/canonical-duration", src);
}

#[test]
fn agent_canonical_program_compiles() {
    // Exercises the agent's full surface area: goal, tools, memory,
    // strategy (react), on_stuck (retry), shield, all four budgets.
    let src = r#"
tool WebSearch {
    provider: http
    timeout:  10s
}

tool CorpusQuery {
    provider: http
    timeout:  5s
}

memory ClientNotes {
    store: persistent
    backend: pgvector
    retrieval: semantic
}

shield HallucinationShield {
    scan:       [hallucination, prompt_injection]
    on_breach:  quarantine
    severity:   high
    compliance: [SOC2]
}

agent ResearchAssistant {
    goal:           "Answer the user's question, retrieving evidence from the corpus."
    tools:          [WebSearch, CorpusQuery]
    memory:         ClientNotes
    strategy:       react
    on_stuck:       retry
    shield:         HallucinationShield
    max_iterations: 8
    max_tokens:     32000
    max_time:       5m
}
"#;
    must_compile("agent/canonical", src);
}

#[test]
fn type_canonical_program_with_range_refinement_compiles() {
    // Range-only refinement: `type X(0.0..1.0)` — no body required.
    let src = r#"
type RiskScore(0.0..1.0)
type Rating(1..5)
"#;
    must_compile("type/canonical-range", src);
}

#[test]
fn type_canonical_program_with_compliance_and_body_compiles() {
    // `type X compliance [...] { fields }` — the section 6.1 ESK form.
    // Note `compliance` is a PREFIX (no colon) — the historical
    // type-specific grammar.
    let src = r#"
type PatientRecord compliance [HIPAA, GDPR] {
    patient_id:     String,
    ssn:            String,
    diagnosis_code: String,
    dob:            String
}

type DiagnosticReport {
    summary: String
}
"#;
    must_compile("type/canonical-compliance", src);
}

#[test]
fn probe_canonical_program_compiles() {
    // Flow-level probe (sibling of step). The canonical doc also
    // shows a step-level probe; that path is exercised by the
    // step.md drift gate in phase2_canonical_programs.rs.
    let src = r#"
type SymptomList { items: String }
type ClusteredSymptoms { groups: String }
type Diagnosis { name: String }

flow DiagnoseSymptoms(symptoms: SymptomList) -> Diagnosis {
    step Cluster {
        given: symptoms
        ask: "Cluster the symptoms by organ system."
        output: ClusteredSymptoms
    }
    probe Cluster
    step Decide {
        given: Cluster.output
        ask: "Emit the most likely diagnosis."
        output: Diagnosis
    }
}
"#;
    must_compile("probe/canonical", src);
}

#[test]
fn validate_canonical_program_compiles() {
    // `validate <Target>` — flow-step sibling of step.
    let src = r#"
type LoanApplication { applicant_id: String, amount: Number }
type RiskScore(0.0..1.0)
type CreditDecision { decision: String }

flow ScoreCredit(applicant: LoanApplication) -> CreditDecision {
    step ComputeRisk {
        given: applicant
        ask: "Compute the credit-risk score."
        output: RiskScore
    }
    validate ComputeRisk
    step Decide {
        given: ComputeRisk.output
        ask: "Emit the credit decision."
        output: CreditDecision
    }
}
"#;
    must_compile("validate/canonical", src);
}

#[test]
fn refine_canonical_program_compiles() {
    let src = r#"
type EmailBrief { topic: String }
type Email { subject: String, body: String }

flow DraftEmail(brief: EmailBrief) -> Email {
    step Compose {
        given: brief
        ask: "Draft the email."
        output: Email
    }
    refine Compose
    step Send {
        given: Compose.output
        ask: "Render the final email."
        output: Email
    }
}
"#;
    must_compile("refine/canonical", src);
}

#[test]
fn weave_canonical_program_compiles() {
    // weave uses a structured body — sources, target, format,
    // priority, style. The canonical doc's full example.
    let src = r#"
type SymptomList { items: String }
type PatientHistory { entries: String }
type DiagnosisList { items: String }
type Diagnosis { name: String }

flow DiagnoseCase(symptoms: SymptomList, history: PatientHistory) -> Diagnosis {
    step ProposeFromSymptoms {
        given: symptoms
        ask: "Propose 3 differentials from the symptoms alone."
        output: DiagnosisList
    }
    step ProposeFromHistory {
        given: history
        ask: "Propose 3 differentials from the patient history."
        output: DiagnosisList
    }
    step ProposeFromImaging {
        given: history
        ask: "Propose 3 differentials from any imaging in the history."
        output: DiagnosisList
    }
    weave {
        sources:  [ProposeFromSymptoms, ProposeFromHistory, ProposeFromImaging]
        target:   Unified
        format:   structured
        priority: [ProposeFromImaging, ProposeFromSymptoms]
        style:    reconcile
    }
    step Decide {
        given: ProposeFromSymptoms.output
        ask: "Emit the single most-likely diagnosis."
        output: Diagnosis
    }
}
"#;
    must_compile("weave/canonical", src);
}

#[test]
fn run_canonical_program_compiles() {
    // Exercises every modifier on the `run` statement: as / within /
    // constrained_by / on_failure (with params) / output_to / effort.
    let src = r#"
persona LegalExpert {
    domain: ["contract-law"]
    tone:   precise
}

context LegalReview {
    memory:      session
    depth:       exhaustive
    temperature: 0.3
}

anchor NoHallucination {
    require: source_citation
    on_violation: raise AnchorBreachError
}

anchor NoPHI {
    require: source_citation
    reject:  [pii_exposed]
    on_violation: raise PHIBreachError
}

type Contract { body: String }
type ContractAnalysis { summary: String }

flow AnalyzeContract(doc: Contract) -> ContractAnalysis {
    step Extract {
        given: doc
        ask: "Extract the contract's structure."
        output: ContractAnalysis
    }
}

run AnalyzeContract(myContract)
    as LegalExpert
    within LegalReview
    constrained_by [NoHallucination, NoPHI]
    on_failure: retry(backoff: exponential)
    output_to: "report.json"
    effort: high
"#;
    must_compile("run/canonical", src);
}
