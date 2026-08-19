//! v1.2.0 — drift gate for the Tier 2 primitive docs.
//!
//! Every primitive doc shipped under `src/knowledge/primitives/` for
//! Tier 2 (`resource`, `fabric`, `manifest`, `observe`, `reconcile`,
//! `lease`, `ensemble`, `session`, `axonstore`, `dataspace`,
//! `corpus`, `pix`) must be backed by a canonical `.axon` program
//! that round-trips through the same `axon-frontend` pipeline the
//! `axon` CLI uses.
//!
//! Mirrors the pattern from `phase2_canonical_programs.rs` (Tier 0)
//! and `phase6b_canonical_programs.rs` (Tier 1). The drift gate is
//! identical: a canonical .axon → `compiler_pipeline::run` →
//! `Outcome::Ok` or test fails with structured diagnostics.

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

#[test]
fn resource_canonical_program_compiles() {
    let src = r#"
shield PHIShield {
    scan:       [pii_leak]
    on_breach:  quarantine
    severity:   critical
    compliance: [HIPAA]
}

resource EHRDatabase {
    kind:            postgres
    endpoint:        ehr.clinical.internal
    capacity:        300
    lifetime:        linear
    certainty_floor: 0.95
    shield:          PHIShield
}
"#;
    must_compile("resource/canonical", src);
}

#[test]
fn fabric_canonical_program_compiles() {
    let src = r#"
shield PHIShield {
    scan:       [pii_leak]
    on_breach:  quarantine
    severity:   critical
    compliance: [HIPAA]
}

fabric ClinicalCloud {
    provider:  aws
    region:    "us-east-1"
    zones:     3
    ephemeral: false
    shield:    PHIShield
}
"#;
    must_compile("fabric/canonical", src);
}

#[test]
fn manifest_canonical_program_compiles() {
    let src = r#"
resource EHRDatabase {
    kind:      postgres
    endpoint:  ehr.clinical.internal
    capacity:  300
    lifetime:  linear
}

resource TrialArchive {
    kind:      https
    endpoint:  clinical_trial_archive
    lifetime:  persistent
}

resource InferenceEngine {
    kind:      https
    endpoint:  dx_ml.internal
    capacity:  4
    lifetime:  affine
}

fabric ClinicalCloud {
    provider:  aws
    region:    "us-east-1"
    zones:     3
    ephemeral: false
}

manifest ProductionHealthcare {
    resources:   [EHRDatabase, TrialArchive, InferenceEngine]
    fabric:      ClinicalCloud
    region:      "us-east-1"
    zones:       3
    compliance:  [HIPAA, GDPR, GxP, SOC2]
}
"#;
    must_compile("manifest/canonical", src);
}

#[test]
fn observe_canonical_program_compiles() {
    let src = r#"
resource EHRDatabase {
    kind:      postgres
    endpoint:  ehr.clinical.internal
    lifetime:  linear
}

fabric ClinicalCloud {
    provider:  aws
    region:    "us-east-1"
    zones:     3
    ephemeral: false
}

manifest ProductionHealthcare {
    resources:   [EHRDatabase]
    fabric:      ClinicalCloud
    region:      "us-east-1"
    zones:       3
}

observe ClinicalHealth from ProductionHealthcare {
    sources:         [prometheus, cloudwatch, healthcheck]
    quorum:          2
    timeout:         5s
    on_partition:    fail
    certainty_floor: 0.92
}
"#;
    must_compile("observe/canonical", src);
}

#[test]
fn reconcile_canonical_program_compiles() {
    let src = r#"
shield PHIShield {
    scan:       [pii_leak]
    on_breach:  quarantine
    severity:   critical
    compliance: [HIPAA]
}

resource EHRDatabase {
    kind:      postgres
    endpoint:  ehr.clinical.internal
    lifetime:  linear
}

fabric ClinicalCloud {
    provider:  aws
    region:    "us-east-1"
    zones:     3
    ephemeral: false
}

manifest ProductionHealthcare {
    resources:   [EHRDatabase]
    fabric:      ClinicalCloud
}

observe ClinicalHealth from ProductionHealthcare {
    sources:  [prometheus, cloudwatch]
    quorum:   2
    timeout:  5s
}

reconcile EHRReconciler {
    observe:      ClinicalHealth
    threshold:    0.92
    tolerance:    0.05
    on_drift:     provision
    shield:       PHIShield
    max_retries:  3
}
"#;
    must_compile("reconcile/canonical", src);
}

#[test]
fn lease_canonical_program_compiles() {
    let src = r#"
resource BillingDatabase {
    kind:      postgres
    endpoint:  billing.internal
    lifetime:  affine
}

lease BillingLease {
    resource:  BillingDatabase
    duration:  1h
    acquire:   on_start
    on_expire: release
}
"#;
    must_compile("lease/canonical", src);
}

#[test]
fn ensemble_canonical_program_compiles() {
    let src = r#"
resource USDatabase  { kind: postgres  endpoint: us.db  lifetime: affine }
resource EUDatabase  { kind: postgres  endpoint: eu.db  lifetime: affine }
resource APDatabase  { kind: postgres  endpoint: ap.db  lifetime: affine }

fabric Global {
    provider:  aws
    region:    "us-east-1"
    zones:     3
    ephemeral: false
}

manifest ProdUS { resources: [USDatabase] fabric: Global }
manifest ProdEU { resources: [EUDatabase] fabric: Global }
manifest ProdAP { resources: [APDatabase] fabric: Global }

observe ClinicalHealthUS   from ProdUS { sources: [prometheus] quorum: 1 timeout: 5s }
observe ClinicalHealthEU   from ProdEU { sources: [prometheus] quorum: 1 timeout: 5s }
observe ClinicalHealthAPAC from ProdAP { sources: [prometheus] quorum: 1 timeout: 5s }

ensemble GlobalHealth {
    observations:   [ClinicalHealthUS, ClinicalHealthEU, ClinicalHealthAPAC]
    quorum:         2
    aggregation:    byzantine
    certainty_mode: harmonic
}
"#;
    must_compile("ensemble/canonical", src);
}

#[test]
fn session_canonical_program_compiles() {
    // The v2.3.0 duality checker is exhaustive over regular-coinductive
    // equality; rich recursive shapes (loop + select + branch nested)
    // are mathematically supported but computationally heavy in the
    // current implementation. For the drift gate we exercise a
    // **flat dual-pair** that proves the grammar surface end-to-end
    // (`session <Name> { role: [send/receive], ... }`) without
    // stressing the coinductive engine. The richer Chat example in
    // `session.md` documents the full algebra; the v2.3.0 paper proves
    // it; this test pins the grammar.
    let src = r#"
type Request  { body: String }
type Response { body: String }

session SimpleRpc {
    client: [ send Request, receive Response, end ]
    server: [ receive Request, send Response, end ]
}
"#;
    must_compile("session/canonical-rpc", src);
}

#[test]
fn session_canonical_program_with_select_branch_compiles() {
    // Closed-finite session: select / branch with no recursion. The
    // duality checker resolves this in finite steps; it's the
    // canonical shape an agent reaches for in request/response
    // protocols with multiple labels.
    let src = r#"
type Query    { text: String }
type Answer   { text: String }
type Error    { message: String }

session QueryProtocol {
    client: [
        send Query,
        branch {
            ok:   [receive Answer, end],
            fail: [receive Error,  end]
        }
    ]
    server: [
        receive Query,
        select {
            ok:   [send Answer, end],
            fail: [send Error,  end]
        }
    ]
}
"#;
    must_compile("session/canonical-select-branch", src);
}

#[test]
fn axonstore_canonical_with_inline_schema_compiles() {
    // Column types are the closed v1.38.0 catalog (Uuid|Text|Int|
    // BigInt|Float|Double|Bool|Timestamp|Timestamptz|Date|Time|
    // Json|Jsonb|Bytea|Numeric) — NOT the general type system.
    let src = r#"
axonstore PaymentVault {
    backend:     postgresql
    connection:  "postgres://payments.internal/vault"
    isolation:   serializable
    on_breach:   raise
    capability:  "payment.write"
    schema {
        txn_id:     Text primary_key
        amount:     Numeric not_null
        card_token: Text not_null
    }
}
"#;
    must_compile("axonstore/canonical-inline-schema", src);
}

#[test]
fn axonstore_canonical_with_manifest_ref_schema_compiles() {
    let src = r#"
axonstore Tenants {
    backend:    postgresql
    connection: "postgres://core/tenants"
    isolation:  serializable
    on_breach:  raise
    schema:     "public.tenants"
}
"#;
    must_compile("axonstore/canonical-manifest-ref", src);
}

#[test]
fn dataspace_canonical_program_compiles() {
    // v2.63.0 — the body is TYPED now: `column <name>: <Type>` over
    // the closed 6-type catalog. (The pre-108.b "free-form body skipped
    // structurally" is gone — an unknown body entry is a parse error,
    // and an empty schema is an axon-T928 refusal: a dataspace IS its
    // schema.)
    let src = r#"
dataspace ClinicalData {
    column patient_id: Text
    column cost:       Float
    column visits:     Int
}

dataspace BillingData {
    column invoice_id: Text
    column issued_at:  Timestamp
    column paid:       Bool
    column detail:     Json
}
"#;
    must_compile("dataspace/canonical", src);
}

#[test]
fn corpus_canonical_inline_documents_compiles() {
    let src = r#"
type PrivacyPolicy   { text: String }
type TermsOfService  { text: String }
type RefundPolicy    { text: String }

corpus PolicyDocs {
    documents: [PrivacyPolicy, TermsOfService, RefundPolicy]
}
"#;
    must_compile("corpus/canonical-inline", src);
}

#[test]
fn corpus_canonical_mdn_graph_compiles() {
    // v2.13.0 — the MDN corpus graph (form c): typed weighted edges + the
    // `adaptive:` memory flag, navigated by `navigate <corpus>`.
    let src = r#"
type SessA { text: String }
type SessB { text: String }
type SessC { text: String }

corpus SessionKnowledge {
    documents: [SessA, SessB, SessC]
    relations: [
        cite(SessB, SessA, 0.9)
        contradict(SessC, SessA, 0.7)
        elaborate(SessC, SessB, 0.5)
    ]
    adaptive: true
}

flow Recall(q: String) -> String {
    navigate SessionKnowledge {
        query: "${q}"
        from: SessA
        budget: 5
        output: hits
    }
    return hits
}
"#;
    must_compile("corpus/canonical-mdn-graph", src);
}

#[test]
fn corpus_canonical_store_sourced_compiles() {
    // v2.14.0 — the DYNAMIC, store-sourced MDN graph (form d): documents +
    // typed edges are rows in two axonstores; the graph is built from the live
    // rows at navigate-time and `adaptive:` persists its reinforcement.
    let src = r#"
axonstore LtmSummaries {
    backend: postgresql
    connection: "postgres://x"
    schema {
        id:         Uuid primary_key
        summary:    Text not_null
        created_at: Timestamptz
    }
}
axonstore LtmEdges {
    backend: postgresql
    connection: "postgres://x"
    schema {
        from_id: Uuid
        to_id:   Uuid
        etype:   Text
        weight:  Float
    }
}

corpus LtmGraph from axonstore {
    documents: LtmSummaries( id, summary )
    relations: LtmEdges( from_id, to_id, etype, weight )
    adaptive: true
}

flow Recall(q: String) -> String {
    navigate LtmGraph {
        query: "${q}"
        from: LtmSummaries
        budget: 5
        output: hits
    }
    return hits
}
"#;
    must_compile("corpus/canonical-store-sourced", src);
}

#[test]
fn corpus_canonical_from_mcp_shorthand_compiles() {
    let src = r#"
corpus ClinicalGuidelines from mcp("clinical-mcp.internal", "kb://guidelines/2025")
"#;
    must_compile("corpus/canonical-from-mcp", src);
}

#[test]
fn pix_canonical_program_compiles() {
    // v2.12.0 — `pix` is the embeddings-free retrieval navigator (the
    // audit-chain example moved to `ledger_canonical_program_compiles`).
    let src = r#"
pix ContractIndex {
    source:    "contracts/master_agreement.pdf"
    depth:     4
    branching: 3
    model:     fast
}
"#;
    must_compile("pix/canonical", src);
}

#[test]
fn ledger_canonical_program_compiles() {
    // v2.12.0 — `ledger` is the append-only, hash-linked audit chain
    // (the former Provenance-Index reading of `pix`).
    let src = r#"
ledger LedgerAudit {
    source:    "axonstore://GeneralLedger"
    branching: 2
    model:     sha256
}
"#;
    must_compile("ledger/canonical", src);
}

/// v2.67.0 — this used to be `deliberate_canonical_program_compiles`, and
/// the program below is worth reading twice before it is deleted.
///
/// ```text
/// flow Decide(q: String) -> String {
///     deliberate {
///         let draft = "${q}"
///         return draft          // ← this never ran
///     }
/// }
/// ```
///
/// `deliberate` parses through `parse_block_step`, whose entire job is
/// `skip_braced_block()`. The body was discarded **at parse time**. So the `let`
/// never bound, the **`return` never returned**, and the flow silently fell off
/// its end — while the wire reported the block complete.
///
/// **We shipped this as the canonical example.** An adopter copying it got a flow
/// that does not return, and no diagnostic anywhere.
///
/// `deliberate` now refuses (axon-T939). The gate inverts: it guards the
/// RETRACTION, so a future change that silently resurrects a body-swallowing
/// `deliberate` fails the build.
#[test]
fn deliberate_is_refused_and_no_longer_swallows_its_body() {
    let src = r#"
flow Decide(q: String) -> String {
    deliberate {
        let draft = "${q}"
        return draft
    }
}
"#;
    match run(src, "deliberate/retracted") {
        Outcome::Ok { .. } => panic!(
            "`deliberate` compiled clean — but its body is discarded at parse time, so the \
             `return` inside it never runs. If a real implementation has landed, replace this \
             gate with a canonical program AND give the block a body in the AST; do not simply \
             delete the refusal."
        ),
        Outcome::Err { errors, .. } => {
            let joined = format!("{errors:#?}");
            assert!(
                joined.contains("axon-T939"),
                "expected the T939 retraction diagnostic, got: {joined}"
            );
        }
    }
}
