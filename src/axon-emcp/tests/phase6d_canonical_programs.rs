//! v1.2.0 — drift gate for the Tier 3 primitive docs.
//!
//! Every primitive doc shipped under `src/knowledge/primitives/` for
//! Tier 3 (`axonendpoint`, `axpoint`, `daemon`, `mcp`, `listen`,
//! `shield`, `mandate`, `compute`, `lambda`, `forge`, `ots`,
//! `psyche`, `immune`, `reflex`, `heal`) must be backed
//! by a canonical `.axon` program that round-trips through the same
//! `axon-frontend` pipeline the `axon` CLI uses.
//!
//! `taint` and `logic` were never in this batch — both were reserved
//! lexer tokens with no parser production (see the v1.2.0 commit
//! and the v1.2.0 registry note). v2.67.0 finished the job the
//! registry started: both are now RETRACTED from the language and
//! from the README that still advertised them.
//!
//! v2.67.0 also removed `transact` from this batch. It HAD a parser
//! production — so section 6.c's rule ("an entry without a parser production
//! lies") never caught it — and it lied anyway: it opened no
//! transaction. Its canonical program is kept below, inverted into a
//! retraction guard. The widened rule: an entry whose SUMMARY the
//! runtime does not honour lies just as loudly, and is more dangerous.
//!
//! Mirrors the pattern from `phase2/6b/6c_canonical_programs.rs`.

use axon_emcp::compiler_pipeline::{run, Outcome};

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

// ── Wire (5) ─────────────────────────────────────────────────────────

#[test]
fn axonendpoint_canonical_program_compiles() {
    let src = r#"
type AnalyzeRequest { doc: Text }
type RiskReport { summary: Text }

flow AnalyzeContract(doc: Text) -> FlowEnvelope<RiskReport> {
    step Extract {
        given: doc
        ask: "Extract every clause."
        output: FlowEnvelope<RiskReport>
    }
}

axonendpoint AnalyzeContractAPI {
    method:      POST
    path:        "/v1/contracts/analyze"
    body:        AnalyzeRequest
    execute:     AnalyzeContract
    output:      FlowEnvelope<RiskReport>
    backend:     auto
    compliance:  [SOC2]
    retries:     1
    timeout:     20s
}
"#;
    must_compile("axonendpoint/canonical", src);
}

#[test]
fn axpoint_canonical_program_compiles() {
    // `axpoint` is the lexer alias for `axonendpoint`; same parser,
    // same grammar. Drift gate verifies the alias survives.
    let src = r#"
type EchoRequest { message: Text }
type EchoResponse { echoed: Text }

flow Echo(message: Text) -> FlowEnvelope<EchoResponse> {
    step Reply {
        given: message
        ask: "Echo the message back."
        output: FlowEnvelope<EchoResponse>
    }
}

axpoint EchoAPI {
    method:   POST
    path:     "/v1/echo"
    body:     EchoRequest
    execute:  Echo
    output:   FlowEnvelope<EchoResponse>
    backend:  auto
    public:   true
}
"#;
    must_compile("axpoint/canonical", src);
}

#[test]
fn daemon_canonical_program_compiles() {
    let src = r#"
tool TicketDB {
    provider: http
    timeout:  3s
}

tool SlackNotifier {
    provider: http
    timeout:  5s
}

memory RouterState {
    store:     persistent
    backend:   postgresql
    retrieval: exact
}

shield CustomerDataShield {
    scan:       [pii_leak, data_exfil]
    on_breach:  quarantine
    severity:   high
    compliance: [SOC2]
}

daemon TicketRouter {
    goal:       "Route inbound tickets to the right SLA queue."
    tools:      [TicketDB, SlackNotifier]
    memory:     RouterState
    strategy:   react
    on_stuck:   retry
    shield:     CustomerDataShield
    max_tokens: 16000
    max_time:   30m

    listen "tickets.inbound" as msg
}
"#;
    must_compile("daemon/canonical", src);
}

#[test]
fn mcp_canonical_program_compiles() {
    // `mcp` uses the permissive generic-declaration form; the
    // canonical example carries a body but the parser tolerates
    // either form.
    let src = r#"
mcp ClinicalKB {
    // Body uses AXON `//` comments; field shape is validated at
    // deploy time, not parse time.
}
"#;
    must_compile("mcp/canonical", src);
}

#[test]
fn listen_canonical_program_compiles() {
    // Flow-body listen — single subscription, runs the body
    // once per arrival until the flow returns. Uses the
    // string-topic legacy form for cross-stack compatibility
    // (typed channels would need a `channel` declaration —
    // documented separately).
    let src = r#"
type Receipt { id: Text }

flow ProcessIncoming() -> Receipt {
    listen "tickets.urgent" as event {
    }
    step Acknowledge {
        ask: "Acknowledge the event."
        output: Receipt
    }
}
"#;
    must_compile("listen/canonical", src);
}

// ── Operators (8) ────────────────────────────────────────────────────

#[test]
fn shield_canonical_program_compiles() {
    let src = r#"
shield PHIShield {
    scan:       [prompt_injection, pii_leak, data_exfil]
    on_breach:  quarantine
    severity:   critical
    redact:     [ssn, dob]
    compliance: [HIPAA, GDPR, SOC2]
}
"#;
    must_compile("shield/canonical", src);
}

#[test]
fn window_canonical_program_compiles() {
    // v2.27.0 — a timezone-aware temporal window with a holiday exclusion,
    // bound by a scheduled daemon. The v2.27.0 compile-gated corpus example.
    let src = r#"
flow SendBatch() -> Unit {
    step S { ask: "send the outbound batch" output: Unit }
}

window BusinessHours {
    timezone:   "America/Bogota"
    allow:      [ { days: Mon..Fri, hours: 9..18 } ]
    exclude:    [ "2026-12-25", "2026-01-01" ]
    on_outside: defer
}

daemon OutboundScheduler {
    window:   BusinessHours
    requires: [flow.execute]
    listen "cron:*/5 * * * *" as tick {
        run SendBatch()
    }
}
"#;
    must_compile("window/canonical", src);
}

#[test]
fn mandate_canonical_program_compiles() {
    let src = r#"
mandate FinancialApproval {
    constraint:   "Posting > $10k requires CFO + Controller dual approval"
    kp:           1.0
    ki:           0.1
    kd:           0.0
    tolerance:    0.05
    max_steps:    10
    on_violation: halt
}
"#;
    must_compile("mandate/canonical", src);
}

#[test]
fn compute_canonical_program_compiles() {
    // compute's parser currently models only `shield:` as a structured
    // field; the doc explains other fields land permissively. The
    // canonical example pins the `shield:` binding.
    let src = r#"
shield FinancialShield {
    scan:       [pii_leak]
    on_breach:  halt
    severity:   critical
    compliance: [PCI_DSS, SOC2]
}

compute LoanUnderwriterCompute {
    shield: FinancialShield
}
"#;
    must_compile("compute/canonical", src);
}

#[test]
fn lambda_canonical_top_level_compiles() {
    // Top-level lambda declaration — the metadata-only form. The
    // `lambda apply` flow-step form requires a flow body context;
    // we exercise the declaration surface here.
    let src = r#"
lambda DiagnosisCandidate {
    ontology:       "ClinicalInference"
    certainty:      0.85
    temporal_frame: "2025-01-01" "2026-12-31"
    provenance:     "EHR cohort 2024 + clinical guideline ICD-11"
    derivation:     inferred
}
"#;
    must_compile("lambda/canonical-top-level", src);
}

#[test]
fn forge_canonical_program_compiles() {
    let src = r#"
type CaseFacts { body: Text }
type CaseHistory { entries: Text }
type RulingsList { items: Text }
type CaseReport { summary: Text }

flow AssembleReport(case: Text) -> CaseReport {
    step LoadFacts {
        given: case
        ask: "Fetch case facts."
        output: CaseFacts
    }
    step LoadHistory {
        given: case
        ask: "Fetch case history."
        output: CaseHistory
    }
    forge Synthesis(seed: "a novel legal argument from these case facts") -> CaseReport {
        mode: exploratory
        novelty: 0.6
    }
    step Render {
        given: LoadFacts.output
        ask: "Render the final report."
        output: CaseReport
    }
}
"#;
    must_compile("forge/canonical", src);
}

#[test]
fn ots_canonical_program_compiles() {
    // `ots Name<InType, OutType> { teleology, homotopy_search,
    // loss_function }` — exercise the generic-parameter form too.
    let src = r#"
ots AudioMulawToPcm16 {
    teleology:       "Convert mu-law 8kHz audio to PCM16 for downstream processing"
    homotopy_search: deep
    loss_function:   "RMSE on reconstructed signal"
}
"#;
    must_compile("ots/canonical", src);
}

#[test]
fn psyche_canonical_program_compiles() {
    let src = r#"
psyche AnalyticalDisposition {
    dimensions:         [analytical, cautious, evidence_seeking, contrarian]
    manifold_noise:     0.1
    manifold_momentum:  0.7
    // `non_diagnostic` is required by Dependent Type Safety section 4.
    safety_constraints: [non_diagnostic, no_self_harm, no_deception]
    quantum_enabled:    false
    inference_mode:     active
}
"#;
    must_compile("psyche/canonical", src);
}

// ── Cognitive immune system (3) ──────────────────────────────────────

#[test]
fn immune_canonical_program_compiles() {
    let src = r#"
resource MetricsSource { kind: http  endpoint: metrics.internal  lifetime: persistent }
fabric Global { provider: aws  region: "us-east-1"  zones: 3  ephemeral: false }
manifest Production { resources: [MetricsSource]  fabric: Global }

observe ClinicalHealth from Production {
    sources: [prometheus]
    quorum:  1
    timeout: 5s
}

immune ClinicalVigil {
    watch:       [ClinicalHealth]
    sensitivity: 0.90
    baseline:    learned
    window:      800
    scope:       tenant
    tau:         300s
    decay:       exponential
}
"#;
    must_compile("immune/canonical", src);
}

#[test]
fn reflex_canonical_program_compiles() {
    let src = r#"
resource MetricsSource { kind: http  endpoint: metrics.internal  lifetime: persistent }
fabric Global { provider: aws  region: "us-east-1"  zones: 3  ephemeral: false }
manifest Production { resources: [MetricsSource]  fabric: Global }

observe ClinicalHealth from Production { sources: [prometheus]  quorum: 1  timeout: 5s }

immune ClinicalVigil {
    watch:       [ClinicalHealth]
    sensitivity: 0.90
    baseline:    learned
    scope:       tenant
}

reflex QuarantineExfil {
    trigger:  ClinicalVigil
    on_level: speculate
    action:   quarantine
    scope:    tenant
    sla:      1s
}
"#;
    must_compile("reflex/canonical", src);
}

#[test]
fn heal_canonical_program_compiles() {
    let src = r#"
resource MetricsSource { kind: http  endpoint: metrics.internal  lifetime: persistent }
fabric Global { provider: aws  region: "us-east-1"  zones: 3  ephemeral: false }
manifest Production { resources: [MetricsSource]  fabric: Global }

observe ClinicalHealth from Production { sources: [prometheus]  quorum: 1  timeout: 5s }

immune ClinicalVigil {
    watch:       [ClinicalHealth]
    sensitivity: 0.90
    baseline:    learned
    scope:       tenant
}

shield PHIShield {
    scan:       [pii_leak]
    on_breach:  quarantine
    severity:   critical
    compliance: [HIPAA]
}

heal MitigateExposure {
    source:       ClinicalVigil
    on_level:     doubt
    mode:         human_in_loop
    scope:        tenant
    review_sla:   1h
    shield:       PHIShield
    max_patches:  3
}
"#;
    must_compile("heal/canonical", src);
}

// ── Data plane block (1) — `transact` RETRACTED in v2.67.0 ─────────

/// v2.67.0 — this used to be `transact_canonical_program_compiles`, and its
/// canonical program was **double-entry accounting**: post a journal entry
/// inside a `transact { }` block. It is the sharpest possible illustration of
/// the defect v2.67.0 found, so it is worth stating plainly rather than quietly
/// deleting.
///
/// `transact` never opened a transaction. The runtime inserted an unread marker
/// string (`__txn_active = "true"`); `TransactBlock` carries only a source
/// location, so the block's BODY was never lowered into the IR; no lock was
/// taken and nothing was rolled back. And the canonical example we shipped —
/// teaching an accountant that their journal posting was atomic — contained an
/// **empty** `transact { }` block. It was decorative.
///
/// A fabricated atomicity guarantee is worse than an absent one: it is
/// load-bearing exactly on the failure path, which is the one path the adopter
/// will not exercise until production.
///
/// The primitive is now refused at compile time (axon-T938), removed from
/// `PRIMITIVE_REGISTRY`, and its corpus doc deleted. This test inverts: the gate
/// now guards the RETRACTION, so a future change that silently resurrects a
/// no-op `transact` fails here.
#[test]
fn transact_is_retracted_and_refuses_to_compile() {
    let src = r#"
type JournalEntry { period: Text  account: Text  amount: Numeric }
type ValidatedEntry { entry: JournalEntry }
type PostReceipt { id: Text }

flow PostJournalEntry(entry: JournalEntry) -> PostReceipt {
    step Validate {
        given: entry
        ask: "Validate the entry's accounting balance."
        output: ValidatedEntry
    }
    transact {
    }
    step Acknowledge {
        given: Validate.output
        ask: "Render the post receipt."
        output: PostReceipt
    }
}
"#;
    match run(src, "transact/retracted") {
        Outcome::Ok { .. } => panic!(
            "`transact` compiled clean — but it opens no transaction and rolls nothing back. \
             If real transactional semantics have landed (v2.67.0), replace this gate with a \
             canonical program AND restore the registry entry + corpus doc; do not simply \
             delete the refusal."
        ),
        Outcome::Err { errors, .. } => {
            let joined = format!("{errors:#?}");
            assert!(
                joined.contains("axon-T938"),
                "expected the T938 retraction diagnostic, got: {joined}"
            );
        }
    }
}
