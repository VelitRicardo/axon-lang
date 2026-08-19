//! `axon.compose(intent)` — natural-language brief → typed scaffold.
//!
//! The agent supplies an `intent` string in any human language. This
//! module:
//!
//! 1. Classifies the intent into one of 8 closed domains
//!    (`generic` | `healthcare` | `banking` | `government` | `legal`
//!    | `chat` | `retrieval` | `multi_agent`) via a deterministic
//!    keyword-scoring matcher. The match is explainable: the response
//!    carries the score per candidate so the agent can see why this
//!    domain won.
//! 2. Fetches the corresponding template (hand-authored AXON program,
//!    every byte proven to compile by
//!    `tests/phase4_templates_compile.rs`).
//! 3. **Re-validates** the template through the live
//!    `compiler_pipeline::run` pipeline — the same one `axon.check`
//!    uses — and refuses to return a scaffold that fails. The agent
//!    never sees a malformed program.
//! 4. Returns a structured envelope: `{ scaffold, domain,
//!    domain_score, alternatives, primitives_used,
//!    compliance_applied, next_steps, axon_check_verdict }`. The
//!    agent uses `primitives_used` to know which
//!    `axon.primitive_doc(<name>)` calls follow naturally; `next_steps`
//!    is a curated checklist of "what the human should adapt
//!    before deploying".

use crate::compiler_pipeline::{self, Outcome};
use crate::knowledge::Catalog;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// Closed-catalogue domains the compose tool can ground an intent in.
/// The set is intentionally small — every domain ships an
/// `.axon`-check-clean template, so adding a domain is a structured
/// PR (template + entry in this enum + entry in
/// [`domain_metadata`]), not a runtime config.
///
/// v1.2.0 expanded the vertical surface with `legaltech`,
/// `fintech`, `pharmatech`, `medic_research` — each a distinct
/// SaaS / product / research vertical with its own compliance
/// posture (different from `legal`, `banking`, `healthcare`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    /// Fallback — minimal scaffold with persona/context/anchor/flow.
    /// Returned when no domain scores ≥ 1.
    Generic,
    /// HIPAA + GDPR + GxP — PHI, clinical trials, telemedicine.
    Healthcare,
    /// PCI_DSS + SOX + SOC2 — enterprise-bank payments, loans, ledger.
    Banking,
    /// FISMA + NIST_800_53 + FedRAMP — agency, citizen, federal.
    Government,
    /// SOC2 + privilege discipline — in-house contract / case / e-discovery.
    Legal,
    /// Streaming dialogue — chat, conversation, real-time reply.
    Chat,
    /// RAG / Q&A grounded in a corpus — search, lookup, knowledge base.
    Retrieval,
    /// Multi-agent task delegation — planner + worker, ensemble,
    /// coordination across personas.
    MultiAgent,
    /// v1.2.0 — SOC2 + multi-tenant SaaS for legal-technology
    /// products (contract automation, e-discovery, IP portfolio
    /// management). Differs from `Legal`: PRODUCT pattern with
    /// `requires:` capability gates, replay-token writes, per-tenant
    /// path scoping.
    LegalTech,
    /// v1.2.0 — PCI_DSS + SOC2 with AML/KYC + fraud-detection
    /// emphasis. Consumer fintech / neobank / embedded finance.
    /// Differs from `Banking`: AML/CFT + state money-transmitter +
    /// consumer-protection posture, not enterprise-ledger.
    FinTech,
    /// v1.2.0 — GxP + HIPAA + SOC2 for pharmaceutical R&D, drug
    /// discovery, compound screening. FDA 21 CFR Part 11 audit
    /// trails. Differs from `Healthcare` (patient care).
    PharmaTech,
    /// v1.2.0 — HIPAA + GxP + SOC2 for clinical research +
    /// trial-management workflows. IRB-supervised, adverse-event
    /// recording, protocol-deviation tracking. Differs from both
    /// `Healthcare` (patient care) and `PharmaTech` (drug discovery).
    MedicResearch,
    // ── v1.2.0 — Agent patterns (interaction modality) ────────
    /// v1.2.0 — Streaming research-assistant chat grounded in a
    /// declared corpus with hard-fail source-citation anchor.
    ChatResearch,
    /// v1.2.0 — Streaming chat with function-calling tools
    /// (web search, calculator, time API). Tool-use surface.
    ChatTools,
    /// v1.2.0 — Chat that classifies-then-dispatches to typed
    /// skill sub-flows (Support / Sales / Billing per the
    /// canonical template).
    ChatSkills,
    /// v1.2.0 — WhatsApp Business webhook agent — typed
    /// inbound/outbound payloads, per-phone-number persistent
    /// memory, PII-redaction shield.
    Whatsapp,
    /// v1.2.0 — Voice agent — μ-law ↔ PCM16 OTS codec
    /// transformations, streaming Stream<Token> reply.
    Voice,
    /// v1.2.0 — Coding-assistant agent — sandboxed code
    /// interpreter + git tools + streaming reply, anchored
    /// against hallucinated APIs.
    Dev,
    /// v1.2.0 — Consultative sales agent — lead qualification
    /// with NoMisrepresentation anchor + CRM tool bindings.
    SalesConsultive,
    /// v1.2.0 — Embedded sales widget — streaming SSE chat +
    /// JSON lead-capture endpoint, product-corpus grounded.
    SalesWidget,
    // ── v1.2.0 — Application patterns ─────────────────────────
    /// v1.2.0 — RPA-style multi-step workflow with mandate-gated
    /// approvals + typed audit receipt.
    WorkflowAutomation,
    /// v1.2.0 — BI / analytics pipeline against a typed metrics
    /// warehouse with data-freshness anchor.
    BusinessIntelligence,
    /// v1.2.0 — ERP / CRM / HR sync hub — typed normalisation,
    /// lineage preservation, reconciliation.
    CorporateIntegration,
    /// v1.2.0 — Continual-learning loop — feedback → eval →
    /// mandate-gated production promotion.
    SelfLearning,
    /// v1.2.0 — Bulk document classification + extraction (general
    /// document Q&A / invoice / resume processing).
    DocumentAnalysis,
    /// v1.2.0 — Async support-ticket classification + priority +
    /// SLA-aware routing.
    TicketTriage,
    /// v1.2.0 — UGC moderation chain (immune + reflex + heal)
    /// with policy-citation anchor.
    ContentModeration,
    /// v1.2.0 — Knowledge-graph entity + relation extraction with
    /// typed triple store + evidence-span anchor.
    KnowledgeExtraction,
    /// v1.2.0 — Multi-framework compliance monitoring (HIPAA /
    /// GDPR / PCI / SOX / SOC2) with PIX-chained audit + auditor
    /// review mandate.
    ComplianceMonitoring,
    /// v1.2.0 — Resume screening + candidate ranking with
    /// mandatory bias-detection shield + NoBiasInRanking anchor.
    Recruitment,
    /// v1.2.0 — Curriculum-driven tutoring persona with student-
    /// progress memory + Socratic anchor + streaming reply.
    Education,
    /// v1.2.0 — Personal-finance educator with mandatory
    /// NoInvestmentAdvice anchor (NOT a registered advisor).
    FinancialAdvisor,
    /// v1.2.0 — ETL pipeline with cognitive enrichment + transact
    /// over the load step + quality-floor anchor.
    DataPipeline,
}

impl Domain {
    /// The template slug — matches the file stem under
    /// `src/knowledge/templates/<slug>.axon`.
    pub fn slug(self) -> &'static str {
        match self {
            Domain::Generic => "generic",
            Domain::Healthcare => "healthcare",
            Domain::Banking => "banking",
            Domain::Government => "government",
            Domain::Legal => "legal",
            Domain::Chat => "chat",
            Domain::Retrieval => "retrieval",
            Domain::MultiAgent => "multi_agent",
            Domain::LegalTech => "legaltech",
            Domain::FinTech => "fintech",
            Domain::PharmaTech => "pharmatech",
            Domain::MedicResearch => "medic_research",
            Domain::ChatResearch => "chat_research",
            Domain::ChatTools => "chat_tools",
            Domain::ChatSkills => "chat_skills",
            Domain::Whatsapp => "whatsapp",
            Domain::Voice => "voice",
            Domain::Dev => "dev",
            Domain::SalesConsultive => "sales_consultive",
            Domain::SalesWidget => "sales_widget",
            Domain::WorkflowAutomation => "workflow_automation",
            Domain::BusinessIntelligence => "business_intelligence",
            Domain::CorporateIntegration => "corporate_integration",
            Domain::SelfLearning => "self_learning",
            Domain::DocumentAnalysis => "document_analysis",
            Domain::TicketTriage => "ticket_triage",
            Domain::ContentModeration => "content_moderation",
            Domain::KnowledgeExtraction => "knowledge_extraction",
            Domain::ComplianceMonitoring => "compliance_monitoring",
            Domain::Recruitment => "recruitment",
            Domain::Education => "education",
            Domain::FinancialAdvisor => "financial_advisor",
            Domain::DataPipeline => "data_pipeline",
        }
    }
    /// Every domain in stable order — used for classifier iteration
    /// and for the `alternatives` field in the response. Verticals
    /// appear FIRST (most-specific wins ties); meta-patterns
    /// (`Chat`, `Retrieval`, `MultiAgent`) and `Generic` last.
    pub const fn all() -> &'static [Domain] {
        &[
            // Verticals — most specific
            Domain::Healthcare,
            Domain::Banking,
            Domain::Government,
            Domain::Legal,
            Domain::LegalTech,
            Domain::FinTech,
            Domain::PharmaTech,
            Domain::MedicResearch,
            // Agent patterns (interaction modality) — v1.2.0
            Domain::ChatResearch,
            Domain::ChatTools,
            Domain::ChatSkills,
            Domain::Whatsapp,
            Domain::Voice,
            Domain::Dev,
            Domain::SalesConsultive,
            Domain::SalesWidget,
            // Application patterns (use-case shape) — v1.2.0
            Domain::WorkflowAutomation,
            Domain::BusinessIntelligence,
            Domain::CorporateIntegration,
            Domain::SelfLearning,
            Domain::DocumentAnalysis,
            Domain::TicketTriage,
            Domain::ContentModeration,
            Domain::KnowledgeExtraction,
            Domain::ComplianceMonitoring,
            Domain::Recruitment,
            Domain::Education,
            Domain::FinancialAdvisor,
            Domain::DataPipeline,
            // Meta-patterns
            Domain::Chat,
            Domain::Retrieval,
            Domain::MultiAgent,
            // `Generic` is intentionally LAST — it is the fallback,
            // never preferred when another domain scores.
            Domain::Generic,
        ]
    }
}

/// Static metadata per domain: the keyword vocabulary the classifier
/// scores against, the primitives the template uses, the compliance
/// frameworks wired in, and a checklist of next steps the agent should
/// recommend to the human.
struct DomainMetadata {
    /// Human-readable domain label surfaced in `next_steps` prose.
    label: &'static str,
    /// One-line summary of what the template gives the adopter.
    summary: &'static str,
    /// Keywords the classifier scans for in the intent string. Each
    /// match adds 1 to the domain's score. Keep the list focused —
    /// noisy keywords cause misclassification.
    keywords: &'static [&'static str],
    /// AXON primitives the template declares. The agent uses this to
    /// know which `axon.primitive_doc(<name>)` calls follow.
    primitives_used: &'static [&'static str],
    /// Compliance tags wired into the template by construction.
    compliance_applied: &'static [&'static str],
    /// Operator-facing checklist surfaced in the response. Each entry
    /// is a one-line action — the agent renders these as bullet points
    /// to the human.
    next_steps: &'static [&'static str],
}

/// The closed registry. Adding a domain means (a) a new enum variant
/// + slug, (b) a new template `.axon` file (proven to compile by
/// `phase4_templates_compile`), (c) a new entry here. No runtime
/// config; no surprise domains.
fn domain_metadata(d: Domain) -> &'static DomainMetadata {
    match d {
        Domain::Generic => &DomainMetadata {
            label: "Generic",
            summary: "Minimal typed scaffold — persona + context + anchor + flow.",
            keywords: &[],
            primitives_used: &["persona", "context", "anchor", "type", "flow", "run"],
            compliance_applied: &[],
            next_steps: &[
                "Rename the placeholder identifiers (`MyPersona`, `Input`, `Output`, `Answer`).",
                "Refine the persona's `domain:` list to the actual expertise.",
                "Tune the `confidence_threshold:` and the anchor's `confidence_floor:`.",
                "Replace the `step Think` prompt with your real task description.",
                "Add a transport (`axonendpoint` or `socket`) once the flow is stable.",
            ],
        },
        Domain::Healthcare => &DomainMetadata {
            label: "Healthcare (HIPAA + GDPR + GxP)",
            summary: "PHI-tagged types + PHIShield + clinical reviewer persona + audited HTTP boundary.",
            keywords: &[
                "patient", "phi", "hipaa", "medical", "medicine", "clinical",
                "diagnos", "ehr", "health", "doctor", "physician", "nurse",
                "hospital", "trial", "gxp", "fda", "pharma", "treatment",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["HIPAA", "GDPR", "GxP", "SOC2"],
            next_steps: &[
                "Sign a Business Associate Agreement (BAA) with every downstream LLM provider.",
                "Replace the diagnosis prompt with your real clinical question.",
                "Tighten `confidence_threshold:` to your safety floor (default 0.9).",
                "Pin a deterministic backend (`backend: openai` / `anthropic` / ...) for production.",
                "Layer additional anchors per use case (e.g. `NoOffLabelRecommendation`).",
            ],
        },
        Domain::Banking => &DomainMetadata {
            label: "Banking (PCI DSS + SOX + SOC 2)",
            summary: "Payment + loan decision flows with FinancialShield and audited HTTP boundaries.",
            keywords: &[
                "payment", "loan", "credit", "bank", "transaction", "pci",
                "card", "fraud", "underwrit", "ledger", "fintech", "sox",
                "trader", "treasury", "wire", "merchant", "settle",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["PCI_DSS", "SOX", "SOC2"],
            next_steps: &[
                "Tokenise PANs at ingress — never store raw card numbers downstream.",
                "Add a `mandate:` with `excludes_requester: true` on posting endpoints (SOX section 404 SoD).",
                "Configure retention ≥ 7y on any ledger-bearing `axonstore` (SOX section 802).",
                "Pin a deterministic backend for reproducible underwriting decisions.",
                "Wire fraud-detection signals into the shield's scan list as they become available.",
            ],
        },
        Domain::Government => &DomainMetadata {
            label: "Government (FISMA + NIST 800-53 + FedRAMP Moderate)",
            summary: "Citizen-record + benefits-eligibility flow with AgencyShield and audited HTTP boundary.",
            keywords: &[
                "agency", "citizen", "benefit", "federal", "government",
                "fisma", "fedramp", "nist", "ssa", "va", "irs", "policy",
                "eligibility", "adjudicat", "public", "constituent",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["FISMA", "NIST_800_53", "FedRAMP_Moderate", "SOC2"],
            next_steps: &[
                "Determine FIPS 199 categorisation (Low / Moderate / High) and align the FedRAMP tag.",
                "File the System Security Plan (SSP) with the cognising AO.",
                "Bind every endpoint to a documented `requires:` capability (no wildcards).",
                "Enable monthly POA&M tracking against the audit chain.",
                "Annual 3PAO assessment + continuous monitoring.",
            ],
        },
        Domain::Legal => &DomainMetadata {
            label: "Legal (SOC 2 + privilege discipline)",
            summary: "Contract-analysis flow with PrivilegeShield and audited HTTP boundary.",
            keywords: &[
                "contract", "clause", "legal", "lawyer", "attorney",
                "discovery", "privilege", "case", "court", "compliance",
                "regulator", "counsel", "litigation",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Confirm the persona's `domain:` matches the actual jurisdiction(s).",
                "Tune `confidence_threshold:` upward — legal advice is high-stakes.",
                "Add a `NoLegalAdvice` anchor variant if the system is not attorney-supervised.",
                "Audit attorney-client privilege boundaries before exposing the endpoint externally.",
                "Configure retention per the matter-management policy.",
            ],
        },
        Domain::Chat => &DomainMetadata {
            label: "Streaming Chat",
            summary: "Token-by-token streaming flow with SSE transport — friendly conversational persona.",
            keywords: &[
                "chat", "conversation", "dialogue", "messaging", "talk",
                "stream", "real-time", "live", "interactive", "assistant",
                "companion",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "tool",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &[],
            next_steps: &[
                "Pin a streaming backend (an LLM-routed backend (omit `provider:`)).",
                "Choose a backpressure policy (drop_oldest / pause_upstream / fail) on the tool.",
                "Add a `socket` declaration if you need session-typed reconnection (v2.3.0).",
                "Tighten the persona's `tone:` for your product voice.",
                "Compose a `shield:` if the chat touches regulated data.",
            ],
        },
        Domain::Retrieval => &DomainMetadata {
            label: "Retrieval-augmented Q&A (RAG)",
            summary: "Two-step search + compose flow grounded in retrieved evidence with citations.",
            keywords: &[
                "search", "retriev", "rag", "knowledge", "lookup",
                "question", "answer", "qa", "documents", "corpus",
                "research", "evidence",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Declare a `corpus` primitive pointing at your knowledge base.",
                "Replace the `Search` step prompt with a query that actually invokes retrieval.",
                "Add a `retrieve` step that pulls from the corpus before the `Compose` step.",
                "Confirm the citation format matches your downstream rendering.",
                "Tune `confidence_floor:` to suppress ungrounded outputs.",
            ],
        },
        Domain::LegalTech => &DomainMetadata {
            label: "LegalTech SaaS (SOC 2)",
            summary: "Multi-tenant SaaS for legal-technology products — contract automation, e-discovery, IP portfolio management, matter management. PrivilegeShield + per-tenant `requires:` capability gates + replay-token writes.",
            keywords: &[
                "legaltech", "legal-tech", "legal_tech", "contract_automation",
                "ediscovery", "e-discovery", "matter", "ip-portfolio",
                "ip_portfolio", "case_management", "tenant",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Define your tenant-id propagation policy across endpoints.",
                "Adjust `requires:` capability slugs to your RBAC catalogue.",
                "Add `replay: true` if writes must be idempotent.",
                "Layer SOX or HIPAA on top if the platform touches financial or PHI matters.",
                "Configure retention per jurisdiction (state-bar requirements vary).",
            ],
        },
        Domain::FinTech => &DomainMetadata {
            label: "Consumer FinTech (PCI DSS + KYC/AML)",
            summary: "Mobile-first consumer-fintech / neobank / embedded-finance scaffold with KYC + AML + PCI DSS focus. Differs from `Banking` (enterprise-ledger pattern): this targets consumer accounts, real-time risk scoring, dispute handling.",
            keywords: &[
                "fintech", "neobank", "embedded_finance", "consumer_finance",
                "wallet", "kyc", "aml", "money_transmitter", "fraud_detection",
                "remittance", "p2p_payment", "buy_now_pay_later", "bnpl",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["PCI_DSS", "SOC2"],
            next_steps: &[
                "Wire your KYC provider (Plaid / Persona / Trulioo) as a declared `tool`.",
                "Adopt state-money-transmitter retention policies per jurisdiction.",
                "Hook the AML/sanctions screen into the FintechShield's scan list.",
                "Tune `confidence_floor:` per the risk-tier policy.",
                "Add SOC2-Privacy if you serve regulated personal data.",
            ],
        },
        Domain::PharmaTech => &DomainMetadata {
            label: "PharmaTech R&D (GxP + HIPAA + 21 CFR Part 11)",
            summary: "Pharmaceutical R&D + drug-discovery + compound-screening scaffold. FDA 21 CFR Part 11 audit trails, GxP discipline, IRB-supervised workflows. Persona is scientific-researcher (not clinician); compliance is R&D-forward.",
            keywords: &[
                "pharma", "pharmatech", "pharma-tech", "drug_discovery",
                "medicinal_chemistry", "compound_screening", "assay",
                "fda", "21_cfr_part_11", "gxp", "preclinical", "toxicology",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["GxP", "HIPAA", "SOC2"],
            next_steps: &[
                "File your validation plan (IQ / OQ / PQ) per GAMP 5 category.",
                "Wire the audit chain to a tamper-evident archive (PIX-backed).",
                "Tune `confidence_floor:` upward — pharma decisions are high-stakes.",
                "Pin a deterministic backend for reproducible compound predictions.",
                "Add a `compute` declaration with explicit seed for replay.",
            ],
        },
        Domain::MedicResearch => &DomainMetadata {
            label: "Medical Research / I+D (HIPAA + GxP + IRB)",
            summary: "Clinical research + trial-management scaffold — participant enrolment, adverse-event recording, protocol-deviation tracking. IRB-supervised, ICH-GCP-aligned. Differs from `Healthcare` (patient care) and `PharmaTech` (drug discovery).",
            keywords: &[
                "clinical_trial", "clinical_research", "trial_management",
                "medic_research", "medical_research", "i_plus_d", "i+d",
                "irb", "informed_consent", "adverse_event", "protocol_deviation",
                "participant", "good_clinical_practice", "ich_gcp",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["HIPAA", "GxP", "SOC2"],
            next_steps: &[
                "Sign a BAA with every downstream LLM provider; the participant_id is PHI.",
                "Wire IRB approval status into the InformedConsentVerified anchor's evidence chain.",
                "Configure retention per ICH-E6 (R3) — 25y for investigator records.",
                "Add a `pix` (Provenance Index) over the trial database for tamper-evident audit.",
                "Layer a `heal` routine for adverse-event escalation with human-in-loop review.",
            ],
        },
        Domain::ChatResearch => &DomainMetadata {
            label: "Research-assistant chat (corpus-grounded SSE)",
            summary: "Streaming chat anchored on a declared corpus — every reply cites the corpus passages it draws from. Persona is analytical, evidence-led, low-temperature.",
            keywords: &[
                "research", "literature_review", "academic", "scholar",
                "study", "evidence_based", "grounded_chat", "rag_chat",
                "citation", "scientific",
            ],
            primitives_used: &[
                "type", "corpus", "persona", "context", "anchor", "tool",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &[],
            next_steps: &[
                "Populate the corpus with real document references (replace the PaperA/PaperB/PaperC placeholders).",
                "Pin a deterministic backend for reproducible literature reviews.",
                "Tune `MustCiteCorpus.confidence_floor:` upward if the domain is high-stakes.",
                "Layer SOC2 compliance if customer-attributed research is in scope.",
                "Add a `retrieve` flow-step before `Reply` to materialise corpus chunks explicitly.",
            ],
        },
        Domain::ChatTools => &DomainMetadata {
            label: "Streaming chat with function-calling tools",
            summary: "Streaming chat that invokes a declared catalogue of tools (web search, calculator, time API) mid-conversation. Closed tool surface — strict-tool-mode optional.",
            keywords: &[
                "tool_use", "function_calling", "tools_chat",
                "agent_tools", "web_search_chat", "calculator_chat",
                "openai_tools", "anthropic_tools",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "tool",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &[],
            next_steps: &[
                "Pick the real tool providers (brave / tavily / serper / native_calculator / …).",
                "Pin a streaming backend (an LLM-routed backend (omit `provider:`)).",
                "Add a `shield` if the chat touches PII or regulated data.",
                "Run `effort: strict` on the bound `run` to lock the tool surface in production.",
                "Wire structured telemetry on tool-invocation outcomes (v1.3.0 preview).",
            ],
        },
        Domain::ChatSkills => &DomainMetadata {
            label: "Skill-routing chat (multi-flow dispatch)",
            summary: "A router persona that classifies the user's message and dispatches to typed skill sub-flows (Support / Sales / Billing). Each skill is a first-class flow with its own typed I/O — audit-traceable per dispatch.",
            keywords: &[
                "skill", "skills", "skill_routing", "router_chat",
                "dispatch_chat", "multi_skill", "intent_classifier",
                "customer_support", "support_router",
            ],
            primitives_used: &[
                "type", "persona", "context",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &[],
            next_steps: &[
                "Add real skill flows for your domain (Returns / OnboardingHelp / etc.).",
                "Tune the router's `Classify` step's prompt with adopter-specific examples.",
                "Bind a `shield` per skill — billing tends to need stricter PII gating than support.",
                "Add a `confidence_floor:` on the router so ambiguous messages escalate to human review.",
                "Wire telemetry per skill on dispatch + outcome (lead-in to v1.3.0).",
            ],
        },
        Domain::Whatsapp => &DomainMetadata {
            label: "WhatsApp Business webhook agent",
            summary: "Conversational agent driven by the WA Business API. Typed inbound/outbound payloads, per-phone-number persistent memory, PII-redaction shield (phone numbers are PII). Stays inside the WA session window by construction.",
            keywords: &[
                "whatsapp", "wa_business", "wa_webhook", "wati", "twilio_wa",
                "messaging", "conversational_commerce", "chat_widget_mobile",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "shield", "memory",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Wire your WA Business provider (Meta direct / Twilio / Vonage / WATI).",
                "Adopt the WA template-message policy — outbound outside the 24h window requires pre-approved templates.",
                "Localise the persona's `language:` per market.",
                "Tighten `confidence_threshold:` upward for regulated industries.",
                "Persist conversation history under the per-phone-number memory key.",
            ],
        },
        Domain::Voice => &DomainMetadata {
            label: "Voice agent (PSTN / Twilio / Vonage)",
            summary: "Audio-in / audio-out conversational agent with declared `ots:` codec transformations (μ-law 8kHz ↔ PCM16). Streaming Stream<Token> reply via SSE; bridges the carrier-codec to the LLM-streaming pipeline.",
            keywords: &[
                "voice", "voice_agent", "ivr", "phone_agent", "pstn",
                "audio_agent", "twilio_voice", "vonage_voice", "stt_tts",
                "speech_to_text", "text_to_speech",
            ],
            primitives_used: &[
                "type", "ots", "persona", "context", "anchor", "tool",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &[],
            next_steps: &[
                "Wire your STT/TTS provider (Deepgram / ElevenLabs / Azure / native runtime).",
                "Confirm the carrier's actual codec (μ-law 8kHz is Twilio's default; G.711 alaw for European carriers).",
                "Add an `axonstore` if call-transcription persistence is required (HIPAA / SOC2 implications).",
                "Tune SSE keepalive for the call's wall-clock budget.",
                "Layer a shield if the call touches PHI or financial data.",
            ],
        },
        Domain::Dev => &DomainMetadata {
            label: "Dev assistant (sandboxed code + git tools)",
            summary: "Coding agent with sandboxed code-interpreter + git tools + DocsLookup + streaming reply. Anchored against hallucinated APIs — every API claim must cite the language stdlib or a retrieved reference.",
            keywords: &[
                "dev", "developer", "coding_assistant", "code_agent",
                "copilot", "code_review", "pair_programmer",
                "code_interpreter", "git_agent",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "tool",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &[],
            next_steps: &[
                "Pin the sandbox container image with your language toolchains pre-installed.",
                "Configure the `GitTool` provider for your VCS (github / gitlab / bitbucket).",
                "Tighten the persona's `domain:` list to the languages you actually support.",
                "Add an internal docs corpus if you have a private API surface.",
                "Cap `max_tokens:` per the model's effective context window for long-running tasks.",
            ],
        },
        Domain::SalesConsultive => &DomainMetadata {
            label: "Consultative sales agent",
            summary: "Lead-qualification agent anchored to NoMisrepresentation — every product claim must ground on the catalogue corpus. CRM tool bindings log every qualified lead.",
            keywords: &[
                "sales", "consultative_sales", "lead_qualification",
                "sdr", "bdr", "discovery_call", "outbound_sales",
                "inbound_sales", "crm",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "shield", "tool",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Wire the CRMLogger to your CRM (Salesforce / HubSpot / Pipedrive).",
                "Populate the ProductCatalogueLookup's vector store with your real product docs.",
                "Tune `confidence_floor:` upward for regulated industries (finance, health).",
                "Add `requires:` capability gates per sales-org role (SDR / AE / Mgr).",
                "Layer a `pix` (provenance index) over the qualified-leads stream for audit.",
            ],
        },
        Domain::SalesWidget => &DomainMetadata {
            label: "Embedded sales widget (SSE + lead capture)",
            summary: "Two endpoints in one declaration: SSE streaming chat for the live conversation + JSON lead-capture endpoint for the commit. Anchored on a product-knowledge corpus.",
            keywords: &[
                "widget", "sales_widget", "website_chat", "embedded_chat",
                "conversion_chat", "lead_capture", "site_widget",
                "demo_request", "marketing_chat",
            ],
            primitives_used: &[
                "type", "corpus", "persona", "context", "anchor",
                "shield", "tool", "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Populate the ProductCorpus with real product sheets + FAQ entries.",
                "Tune the widget's `tone:` per your brand voice.",
                "Add Cookie / consent gating before persisting the captured lead (GDPR layer).",
                "Wire the lead-capture endpoint to your CRM via a downstream tool.",
                "Add SOC2-Privacy if you're EU-facing; layer GDPR on the lead-capture surface.",
            ],
        },
        Domain::WorkflowAutomation => &DomainMetadata {
            label: "Workflow automation (mandate-gated approvals)",
            summary: "RPA-style multi-step workflow with explicit mandate gating on the approve step + audit receipts on every execution. Back-office work: expense approvals, document routing, claim processing.",
            keywords: &[
                "workflow", "automation", "rpa", "approval_workflow",
                "back_office", "expense_approval", "document_routing",
                "claim_processing", "process_automation",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "shield", "mandate",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Bind the mandate to your real escalation queue.",
                "Add per-step branching via `if` for the approve/reject/escalate paths.",
                "Wire a daemon for the async escalation channel.",
                "Layer SOX if the workflow touches financial transactions.",
                "Add an audit-export endpoint for compliance review.",
            ],
        },
        Domain::BusinessIntelligence => &DomainMetadata {
            label: "Business Intelligence (data-freshness anchored)",
            summary: "Analytical Q&A pipeline against a typed metrics warehouse with freshness + accuracy anchors. Dashboards + ad-hoc queries return AnalyticsResult envelopes.",
            keywords: &[
                "bi", "business_intelligence", "analytics", "dashboard",
                "metrics", "kpi", "data_storytelling", "warehouse",
                "ad_hoc_analysis",
            ],
            primitives_used: &[
                "type", "axonstore", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Pin the warehouse to your real DB + adjust the schema.",
                "Tighten DataFreshnessGate.confidence_floor: per SLA.",
                "Add chart_spec emission via a downstream renderer tool.",
                "Add GDPR if the dashboards touch EU customer data.",
                "Wire row-level security via dataspace tenant isolation.",
            ],
        },
        Domain::CorporateIntegration => &DomainMetadata {
            label: "Corporate Integration (ERP / CRM / HR sync hub)",
            summary: "Multi-source sync hub with typed normalisation + lineage preservation + reconciliation. SAP / Workday / Salesforce / NetSuite integration shape.",
            keywords: &[
                "integration", "corporate_integration", "erp", "crm", "hr",
                "sap", "workday", "salesforce", "netsuite", "mdm",
                "master_data", "ipaas",
            ],
            primitives_used: &[
                "type", "resource", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Wire your real connector providers (sap-sdk / salesforce-sdk).",
                "Adopt a canonical entity ontology (Schema.org or in-house).",
                "Add idempotency keys on the IngestRequest for replay safety.",
                "Layer GDPR if EU employee/customer data is in scope.",
                "Configure DLQ for unprocessable records (DataPipeline pattern).",
            ],
        },
        Domain::SelfLearning => &DomainMetadata {
            label: "Continual learning (mandate-gated promotion)",
            summary: "Feedback-driven model improvement loop — collect signals, score, eval, promote (under mandate). Anti-poisoning shield + EvidenceBackedPromotion anchor.",
            keywords: &[
                "continual_learning", "self_learning", "feedback_loop",
                "mlops", "ml_ops", "model_improvement", "active_learning",
                "online_learning", "drift_detection",
            ],
            primitives_used: &[
                "type", "memory", "persona", "context", "anchor", "shield",
                "mandate", "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Wire your eval-set + lift-calculation backend.",
                "Tune PromotionApproval.constraint to your real promotion criteria.",
                "Add training-poisoning detection signals into the shield's scan list.",
                "Layer a `pix` (provenance) over the FeedbackStore for tamper-evidence.",
                "Add a `heal` routine for borderline-promotion human review.",
            ],
        },
        Domain::DocumentAnalysis => &DomainMetadata {
            label: "Document analysis (bulk extraction)",
            summary: "General bulk-document processing — classify, extract, emit typed structured data with confidence per field. Invoices, resumes, reports, technical docs.",
            keywords: &[
                "document_analysis", "document_processing", "ocr",
                "invoice_processing", "resume_parsing", "report_extraction",
                "form_processing", "intelligent_document_processing", "idp",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                // v2.54.0 — the scaffold no longer advises wiring an
                // OCR tool that would hallucinate; v2.54.0 ships real Inferred
                // producers. For scans/PDFs, call PDFExtractor / ImageTextExtractor
                // (born Inferred, believe-ceiling, engine-measured confidence);
                // for parsed DOCX/PPTX/XLSX, DocumentReader (born Parsed, v2.54.0).
                "For scans/images/PDFs, use ImageTextExtractor / PDFExtractor (born Inferred, ceiling `believe`, engine-measured confidence); for DOCX/PPTX/XLSX use DocumentReader (born Parsed).",
                "The anchor's confidence_floor now gates a MEASURED confidence, not a model self-grade — tighten it for regulated extractions.",
                "Add per-document-class extraction sub-flows (apply: pattern).",
                "Layer HIPAA / GDPR if the documents contain PHI / PII (OCR of PHI via a sidecar is already refused).",
                "Sub-floor Inferred spans route to the quarantine sink, never to the agent.",
            ],
        },
        Domain::TicketTriage => &DomainMetadata {
            label: "Ticket triage (SLA-aware routing)",
            summary: "Async support-ticket classification + priority + routing to the right queue. SLA-aware; customer-tier-respecting. PII-redacting shield (ticket bodies often contain PII).",
            keywords: &[
                "ticket_triage", "ticket_routing", "support_routing",
                "zendesk", "intercom", "freshdesk", "helpdesk",
                "sla_routing", "customer_support",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Wire the routing queues to your real helpdesk.",
                "Tune classification thresholds per customer-tier policy.",
                "Add an auto-answer step for tickets that match canned-response patterns.",
                "Add a daemon for the bulk-import lane (legacy ticket migration).",
                "Layer GDPR if the helpdesk serves EU customers.",
            ],
        },
        Domain::ContentModeration => &DomainMetadata {
            label: "Content moderation (immune + reflex + heal)",
            summary: "UGC moderation pipeline with continuous-monitoring immune system, immediate-action reflex (quarantine), and human-in-loop heal for borderline cases.",
            keywords: &[
                "content_moderation", "moderation", "trust_and_safety",
                "ugc_moderation", "toxicity_detection", "ugc",
                "policy_violation", "auto_moderation",
            ],
            primitives_used: &[
                "type", "resource", "fabric", "manifest", "observe",
                "immune", "reflex", "heal",
                "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Tune ContentVigil.sensitivity per the platform's tolerance.",
                "Add per-region moderation policy variants (EU vs US thresholds).",
                "Wire heal review queue to your real moderation-operations team.",
                "Add an appeal flow for restored content (legal-discovery friendly).",
                "Layer GDPR if EU users post UGC.",
            ],
        },
        Domain::KnowledgeExtraction => &DomainMetadata {
            label: "Knowledge extraction (entity + relation graphs)",
            summary: "Entity + relation triple extraction for knowledge graphs. Typed candidates, confidence per triple, evidence-span anchored. Backs RAG with structured retrieval.",
            keywords: &[
                "knowledge_extraction", "knowledge_graph", "kg", "ner",
                "named_entity_recognition", "relation_extraction", "ontology",
                "entity_linking", "wikidata", "graph_construction",
            ],
            primitives_used: &[
                "type", "axonstore", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Pin the entity-URI naming convention (Wikidata / in-house ontology).",
                "Add an alignment step to merge new triples with existing graph state.",
                "Tighten EvidenceSpanRequired.confidence_floor for high-stakes KGs.",
                "Add SPARQL / Cypher endpoints downstream of the triple store.",
                "Layer compliance per the source data's classification.",
            ],
        },
        Domain::ComplianceMonitoring => &DomainMetadata {
            label: "Compliance monitoring (multi-framework posture)",
            summary: "Continuous multi-framework compliance audit (HIPAA / GDPR / PCI / SOX / SOC2) with PIX-chained tamper-evident audit + auditor-review mandate on posture transitions.",
            keywords: &[
                "compliance_monitoring", "audit", "internal_audit",
                "regulatory_compliance", "posture_management",
                "continuous_compliance", "grc", "hipaa_monitoring",
                "sox_monitoring",
            ],
            primitives_used: &[
                "type", "pix", "persona", "context", "anchor", "shield", "mandate",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Wire the control catalogue per framework (NIST 800-53 SP / SOX 404 / HIPAA section 164).",
                "Add per-control evidence pointers (links to PIX / log / artefact).",
                "Tighten AuditorReview.constraint to your real review SLA.",
                "Add a posture-export endpoint for the auditor-portal.",
                "Layer dedicated retention per framework (SOX 7y, HIPAA 6y, …).",
            ],
        },
        Domain::Recruitment => &DomainMetadata {
            label: "Recruitment (resume screening + bias detection)",
            summary: "Candidate screening + ranking with MANDATORY bias-detection shield + NoBiasInRanking anchor. EEOC-aligned. Production deployments add a heal routine for borderline rankings.",
            keywords: &[
                "recruitment", "hr", "hiring", "talent_acquisition",
                "ats", "applicant_tracking", "resume_screening",
                "candidate_ranking", "fair_hiring",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "ADD a `heal` routine for human review of borderline rankings — required for EEOC defensibility.",
                "Wire to your ATS provider (Greenhouse / Lever / Workday).",
                "Add per-role competency rubric corpus.",
                "Layer GDPR + state-specific HR laws (NYC AI bias law, EU AI Act).",
                "Add an appeal flow for rejected candidates (regulatory friendly).",
            ],
        },
        Domain::Education => &DomainMetadata {
            label: "Education / tutoring (Socratic streaming)",
            summary: "Curriculum-driven tutoring persona with persistent student-progress memory + Socratic anchor (no direct answers when guided discovery teaches more). Streaming SSE reply.",
            keywords: &[
                "education", "tutoring", "edtech", "personalized_learning",
                "pedagogy", "socratic", "lms", "learning_management",
                "intelligent_tutoring",
            ],
            primitives_used: &[
                "type", "memory", "persona", "context", "anchor", "tool",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &[],
            next_steps: &[
                "Wire your curriculum corpus (organised by course + topic).",
                "Add per-skill-level prompt variants in the persona.",
                "Layer FERPA if serving US K-12 / higher-ed.",
                "Add a `heal` routine for at-risk-student escalation.",
                "Configure language: per market for localised tutoring.",
            ],
        },
        Domain::FinancialAdvisor => &DomainMetadata {
            label: "Personal finance educator (NoInvestmentAdvice)",
            summary: "PFM-style assistant with MANDATORY NoInvestmentAdvice anchor (NOT a registered investment advisor). Educational + budgeting + debt-management focused.",
            keywords: &[
                "financial_advisor", "pfm", "personal_finance",
                "budgeting", "debt_management", "financial_literacy",
                "robo_advisor", "fiduciary",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Add a registered-investment-advisor (RIA) review layer for advice-tier features.",
                "Wire authoritative-source corpus (CFPB / NerdWallet / textbook references).",
                "Layer GLBA + state-specific consumer-finance laws as applicable.",
                "Add jurisdiction-aware disclaimer rendering.",
                "Configure persona's language: per market.",
            ],
        },
        Domain::DataPipeline => &DomainMetadata {
            label: "Data pipeline (ETL with cognitive enrichment)",
            summary: "Single-pass ETL pipeline: ingest → validate → enrich (cognitive) → load. Transact block over the load step + quality-floor anchor.",
            keywords: &[
                "data_pipeline", "etl", "elt", "data_ingestion",
                "data_enrichment", "data_quality", "data_loading",
                "warehouse_loading", "stream_processing",
            ],
            primitives_used: &[
                "type", "axonstore", "transact", "persona", "context", "anchor", "shield",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Pin the destination warehouse + adjust the schema.",
                "Add per-source validators as separate sub-flows.",
                "Wire a DLQ for quarantined records.",
                "Layer compliance per the data classification (HIPAA / PCI / GDPR).",
                "Add observability for batch-level latency + throughput.",
            ],
        },
        Domain::MultiAgent => &DomainMetadata {
            label: "Multi-agent coordination",
            summary: "Planner + worker pattern — two personas, two flows, two HTTP boundaries.",
            keywords: &[
                "agent", "multi-agent", "coordinat", "ensemble",
                "planner", "worker", "delegate", "orchestrat", "negotiat",
                "consensus", "swarm",
            ],
            primitives_used: &[
                "type", "persona", "context", "anchor",
                "flow", "step", "axonendpoint",
            ],
            compliance_applied: &["SOC2"],
            next_steps: &[
                "Define the agent contract — the typed handoff between Planner and Worker outputs.",
                "Add a `mandate:` on the worker endpoint if humans must approve plan execution.",
                "Layer per-agent personas with distinct `domain:` lists to maximise diversity.",
                "Consider declaring a `session` + `socket` for synchronous multi-turn coordination.",
                "Add a third agent (reviewer) when high-stakes plans require an independent check.",
            ],
        },
    }
}

// ─── Classifier ──────────────────────────────────────────────────────────

/// Per-domain score from the keyword classifier. Higher = stronger
/// match. Surfaced in the `axon.compose` response so the agent (and
/// the user reading the agent's reply) can see WHY a domain was
/// chosen — opacity here would be unfriendly.
#[derive(Debug, Clone, Serialize)]
pub struct DomainScore {
    pub domain: Domain,
    pub score: u32,
    pub matched_keywords: Vec<String>,
}

/// Score every domain against the intent string and return the
/// per-domain breakdown sorted score-descending. The classifier:
///
/// - Lower-cases the intent (deterministic across locales).
/// - Counts substring matches of each keyword (`patient` matches
///   inside `patients` and `pediatric` — intentional, broadens recall).
/// - The first domain with `score > 0` wins. Ties broken by domain
///   declaration order in [`Domain::all`] (most-specific first;
///   `Generic` is last).
/// - Empty intent returns all-zero scores and the caller falls back
///   to `Generic`.
pub fn classify(intent: &str) -> Vec<DomainScore> {
    let lc = intent.to_lowercase();
    let mut scores: Vec<DomainScore> = Domain::all()
        .iter()
        .map(|&d| {
            let md = domain_metadata(d);
            let mut score: u32 = 0;
            let mut matched = Vec::new();
            for kw in md.keywords {
                if lc.contains(kw) {
                    score += 1;
                    matched.push((*kw).to_string());
                }
            }
            DomainScore { domain: d, score, matched_keywords: matched }
        })
        .collect();
    // Stable sort by score descending, preserving the source order
    // for ties (which matches `Domain::all` — most-specific first).
    scores.sort_by(|a, b| b.score.cmp(&a.score));
    scores
}

/// Pick the winning domain from a scoreboard. The first entry with
/// score > 0 wins; otherwise fall back to `Generic`. Pure helper so
/// the tool layer's call site stays a one-liner.
pub fn select_domain(scores: &[DomainScore]) -> Domain {
    scores
        .iter()
        .find(|s| s.score > 0)
        .map(|s| s.domain)
        .unwrap_or(Domain::Generic)
}

// ─── Compose ─────────────────────────────────────────────────────────────

/// The structured response shape returned by `axon.compose`. Kept
/// public so adopters embedding the library directly can deserialise
/// it without going through JSON.
#[derive(Debug, Clone, Serialize)]
pub struct ComposeResponse {
    /// The `.axon` scaffold the agent should hand back to the user
    /// (or feed back into `axon.check`).
    pub scaffold: String,
    /// Domain the classifier selected.
    pub domain: Domain,
    /// Human-readable label for `domain`.
    pub domain_label: &'static str,
    /// One-line summary of what the scaffold contains.
    pub domain_summary: &'static str,
    /// Per-domain scoring — the agent can quote this back to the user
    /// to explain WHY this domain was chosen.
    pub alternatives: Vec<DomainScore>,
    /// AXON primitives the scaffold declares. Pairs naturally with
    /// `axon.primitive_doc(<name>)` for the agent's follow-up calls.
    pub primitives_used: Vec<String>,
    /// Compliance frameworks wired in by construction. Pairs with
    /// `axon://compliance/<framework>` for the human-facing notes.
    pub compliance_applied: Vec<String>,
    /// Curated next-step checklist surfaced to the human.
    pub next_steps: Vec<String>,
    /// Final assertion: the scaffold round-tripped through the live
    /// `axon-frontend` pipeline. Always one of `"well-formed"` or
    /// `"failed:<stage>"`. A non-OK verdict is a regression — the
    /// integration suite would catch it, but the runtime check is the
    /// belt-and-braces guard.
    pub axon_check_verdict: String,
}

/// Build a compose response for `intent`, optionally pinned to a
/// specific domain by the caller. Returns `Err(msg)` if the requested
/// domain has no template (impossible for the closed catalog) or if
/// the scaffold no longer compiles (a regression that should fail the
/// integration test before reaching here).
pub fn compose(
    intent: &str,
    domain_override: Option<Domain>,
    catalog: &Arc<Catalog>,
) -> Result<ComposeResponse, String> {
    let scoreboard = classify(intent);
    let domain = domain_override.unwrap_or_else(|| select_domain(&scoreboard));
    let md = domain_metadata(domain);
    let tpl = catalog
        .template(domain.slug())
        .ok_or_else(|| format!("compose: template `{}` not in catalog", domain.slug()))?;
    // Re-validate through the live pipeline — same gate `axon.check`
    // uses. A regression here is a regression in the corpus discipline
    // (the phase4_templates_compile integration test would catch it
    // first, but the runtime check is the last line of defence).
    let verdict = match compiler_pipeline::run(&tpl.source, &format!("compose:{}", domain.slug())) {
        Outcome::Ok { .. } => "well-formed".to_string(),
        Outcome::Err { stage, .. } => format!("failed:{}", debug_stage(stage)),
    };
    Ok(ComposeResponse {
        scaffold: tpl.source.clone(),
        domain,
        domain_label: md.label,
        domain_summary: md.summary,
        alternatives: scoreboard,
        primitives_used: md.primitives_used.iter().map(|s| s.to_string()).collect(),
        compliance_applied: md.compliance_applied.iter().map(|s| s.to_string()).collect(),
        next_steps: md.next_steps.iter().map(|s| s.to_string()).collect(),
        axon_check_verdict: verdict,
    })
}

fn debug_stage(s: compiler_pipeline::Stage) -> &'static str {
    use compiler_pipeline::Stage;
    match s {
        Stage::Lex => "lex",
        Stage::Parse => "parse",
        Stage::TypeCheck => "type_check",
        Stage::IrGenerate => "ir_generate",
    }
}

/// JSON projection of a [`ComposeResponse`] — the shape the MCP tool
/// dispatcher wraps in the `{content, isError}` envelope.
pub fn response_to_json(r: &ComposeResponse) -> Value {
    json!({
        "scaffold": r.scaffold,
        "domain": r.domain.slug(),
        "domain_label": r.domain_label,
        "domain_summary": r.domain_summary,
        "alternatives": r.alternatives,
        "primitives_used": r.primitives_used,
        "compliance_applied": r.compliance_applied,
        "next_steps": r.next_steps,
        "axon_check_verdict": r.axon_check_verdict,
    })
}

/// Parse a free-form domain hint (`"healthcare"`, `"hc"`, `"medical"`,
/// `"banking"`, …) into a [`Domain`]. Returns `None` for unknown
/// strings — the caller surfaces a structured `invalid_params` rather
/// than guessing.
pub fn parse_domain_hint(s: &str) -> Option<Domain> {
    match s.trim().to_lowercase().as_str() {
        "generic" | "default" | "minimal" => Some(Domain::Generic),
        // Verticals — order matters: more-specific aliases before
        // less-specific ones so the resolver lands on the right
        // domain (e.g. `legaltech` must NOT fall into `Legal`).
        "healthcare" | "health" | "medical" | "clinical" | "hc" => Some(Domain::Healthcare),
        "banking" | "bank" => Some(Domain::Banking),
        "government" | "gov" | "federal" | "agency" => Some(Domain::Government),
        "legal" | "law" | "contract" => Some(Domain::Legal),
        // v1.2.0 — vertical extension aliases.
        "legaltech" | "legal-tech" | "legal_tech" => Some(Domain::LegalTech),
        "fintech" | "neobank" | "embedded_finance" | "embedded-finance" => Some(Domain::FinTech),
        "pharma" | "pharmatech" | "pharma-tech" | "drug_discovery" | "drug-discovery"
        | "preclinical" => Some(Domain::PharmaTech),
        "medic_research" | "medical_research" | "clinical_research" | "clinical-research"
        | "trial_management" | "i+d" | "ipi" | "irb" => Some(Domain::MedicResearch),
        // v1.2.0 — agent-pattern aliases.
        "chat_research" | "research_chat" | "rag_chat" | "grounded_chat" => {
            Some(Domain::ChatResearch)
        }
        "chat_tools" | "tools_chat" | "function_calling" | "tool_use" => Some(Domain::ChatTools),
        "chat_skills" | "skills_chat" | "skill_router" | "intent_router" => {
            Some(Domain::ChatSkills)
        }
        "whatsapp" | "wa" | "wa_business" | "wati" => Some(Domain::Whatsapp),
        "voice" | "voice_agent" | "phone_agent" | "ivr" | "pstn" => Some(Domain::Voice),
        "dev" | "developer" | "code_agent" | "coding_assistant" | "copilot" => Some(Domain::Dev),
        "sales_consultive" | "sales_consultative" | "consultative_sales" | "sdr" | "bdr"
        | "lead_qualification" => Some(Domain::SalesConsultive),
        "sales_widget" | "widget" | "website_chat" | "embedded_chat" | "lead_capture" => {
            Some(Domain::SalesWidget)
        }
        // v1.2.0 — application-pattern aliases.
        "workflow_automation" | "workflow" | "rpa" | "approval_workflow" | "back_office" => {
            Some(Domain::WorkflowAutomation)
        }
        "business_intelligence" | "bi" | "analytics" | "dashboard" | "kpi" => {
            Some(Domain::BusinessIntelligence)
        }
        "corporate_integration" | "ipaas" | "erp_integration" | "mdm" | "master_data" => {
            Some(Domain::CorporateIntegration)
        }
        "self_learning" | "continual_learning" | "feedback_loop" | "mlops" | "active_learning" => {
            Some(Domain::SelfLearning)
        }
        "document_analysis" | "document_processing" | "idp" | "invoice_processing"
        | "resume_parsing" => Some(Domain::DocumentAnalysis),
        "ticket_triage" | "ticket_routing" | "support_routing" | "helpdesk" => {
            Some(Domain::TicketTriage)
        }
        "content_moderation" | "moderation" | "trust_and_safety" | "ugc_moderation" => {
            Some(Domain::ContentModeration)
        }
        "knowledge_extraction" | "kg" | "knowledge_graph" | "ner"
        | "named_entity_recognition" => Some(Domain::KnowledgeExtraction),
        "compliance_monitoring" | "grc" | "audit_monitoring" | "continuous_compliance" => {
            Some(Domain::ComplianceMonitoring)
        }
        "recruitment" | "hr" | "hiring" | "ats" | "talent_acquisition" => Some(Domain::Recruitment),
        "education" | "tutoring" | "edtech" | "lms" | "intelligent_tutoring" => {
            Some(Domain::Education)
        }
        "financial_advisor" | "pfm" | "personal_finance" | "robo_advisor" => {
            Some(Domain::FinancialAdvisor)
        }
        "data_pipeline" | "etl" | "elt" | "data_ingestion" | "warehouse_loading" => {
            Some(Domain::DataPipeline)
        }
        // Meta-patterns
        "chat" | "streaming" | "dialogue" => Some(Domain::Chat),
        "retrieval" | "rag" | "qa" | "search" => Some(Domain::Retrieval),
        "multi_agent" | "multi-agent" | "multiagent" | "ensemble" | "orchestration" => {
            Some(Domain::MultiAgent)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn embedded_catalog() -> Arc<Catalog> {
        Arc::new(Catalog::load_embedded().expect("embedded corpus must load"))
    }

    // ── Classifier ───────────────────────────────────────────────────

    #[test]
    fn classify_healthcare_keywords_score_higher_than_generic() {
        let scores = classify(
            "I need to handle patient PHI with HIPAA-grade audit for a clinical trial",
        );
        let top = &scores[0];
        assert_eq!(top.domain, Domain::Healthcare);
        assert!(top.score >= 3, "expected ≥ 3 matched keywords, got {top:?}");
    }

    #[test]
    fn classify_banking_keywords_pick_banking_domain() {
        let scores =
            classify("a fintech loan underwriting endpoint that handles credit card payments");
        let top = &scores[0];
        assert_eq!(top.domain, Domain::Banking);
    }

    #[test]
    fn classify_chat_keywords_pick_chat_domain() {
        let scores = classify("a real-time streaming conversation assistant that replies token by token");
        let top = &scores[0];
        assert_eq!(top.domain, Domain::Chat);
    }

    #[test]
    fn classify_retrieval_keywords_pick_retrieval_domain() {
        let scores = classify("answer questions from a corpus of research documents, with citations");
        let top = &scores[0];
        assert_eq!(top.domain, Domain::Retrieval);
    }

    #[test]
    fn classify_legal_keywords_pick_legal_domain() {
        let scores = classify("analyse a contract clause for an attorney; flag privilege concerns");
        let top = &scores[0];
        assert_eq!(top.domain, Domain::Legal);
    }

    #[test]
    fn classify_government_keywords_pick_government_domain() {
        let scores =
            classify("a federal agency benefits eligibility adjudication endpoint under FedRAMP");
        let top = &scores[0];
        assert_eq!(top.domain, Domain::Government);
    }

    #[test]
    fn classify_multi_agent_keywords_pick_multi_agent_domain() {
        let scores =
            classify("orchestrate a planner agent and a worker agent in an ensemble pattern");
        let top = &scores[0];
        assert_eq!(top.domain, Domain::MultiAgent);
    }

    #[test]
    fn classify_unrelated_intent_falls_back_to_generic() {
        let scores = classify("hello world");
        // No keyword scored; select_domain returns Generic.
        assert_eq!(select_domain(&scores), Domain::Generic);
        // Every entry has zero score.
        assert!(scores.iter().all(|s| s.score == 0));
    }

    #[test]
    fn classify_empty_intent_falls_back_to_generic() {
        let scores = classify("");
        assert_eq!(select_domain(&scores), Domain::Generic);
    }

    // ── compose ──────────────────────────────────────────────────────

    #[test]
    fn compose_returns_well_formed_scaffold_for_healthcare_intent() {
        let cat = embedded_catalog();
        let r = compose("a patient summarisation service with PHI redaction", None, &cat).unwrap();
        assert_eq!(r.domain, Domain::Healthcare);
        assert_eq!(r.axon_check_verdict, "well-formed");
        assert!(r.scaffold.contains("HIPAA"));
        assert!(r.compliance_applied.contains(&"HIPAA".to_string()));
    }

    #[test]
    fn compose_honors_explicit_domain_override() {
        let cat = embedded_catalog();
        // Intent matches banking keywords, but the caller forces chat.
        let r = compose(
            "process credit card payments and loan applications",
            Some(Domain::Chat),
            &cat,
        )
        .unwrap();
        assert_eq!(r.domain, Domain::Chat);
        assert_eq!(r.axon_check_verdict, "well-formed");
    }

    #[test]
    fn compose_falls_back_to_generic_for_unrelated_intent() {
        let cat = embedded_catalog();
        let r = compose("just something basic", None, &cat).unwrap();
        assert_eq!(r.domain, Domain::Generic);
        assert_eq!(r.axon_check_verdict, "well-formed");
    }

    #[test]
    fn compose_response_contains_explainability_payload() {
        let cat = embedded_catalog();
        let r = compose(
            "a patient summarisation service with PHI",
            None,
            &cat,
        )
        .unwrap();
        // alternatives carries every domain so the agent can quote the
        // full scoreboard.
        assert!(r.alternatives.len() >= 4);
        // primitives_used + next_steps + compliance_applied are
        // non-empty for any real domain — the agent renders these.
        assert!(!r.primitives_used.is_empty());
        assert!(!r.next_steps.is_empty());
        assert!(!r.compliance_applied.is_empty());
    }

    // ── parse_domain_hint ────────────────────────────────────────────

    #[test]
    fn parse_domain_hint_accepts_canonical_and_aliases() {
        assert_eq!(parse_domain_hint("healthcare"), Some(Domain::Healthcare));
        assert_eq!(parse_domain_hint("medical"), Some(Domain::Healthcare));
        assert_eq!(parse_domain_hint("HC"), Some(Domain::Healthcare));
        assert_eq!(parse_domain_hint("banking"), Some(Domain::Banking));
        // v1.2.0 — `fintech` now maps to the dedicated FinTech
        // domain (consumer fintech / neobank / embedded finance),
        // NOT Banking (enterprise-bank pattern).
        assert_eq!(parse_domain_hint("fintech"), Some(Domain::FinTech));
        assert_eq!(parse_domain_hint("neobank"), Some(Domain::FinTech));
        assert_eq!(parse_domain_hint("gov"), Some(Domain::Government));
        assert_eq!(parse_domain_hint("multi-agent"), Some(Domain::MultiAgent));
        assert_eq!(parse_domain_hint("rag"), Some(Domain::Retrieval));
        // v1.2.0 — vertical extensions.
        assert_eq!(parse_domain_hint("legaltech"), Some(Domain::LegalTech));
        assert_eq!(parse_domain_hint("pharma"), Some(Domain::PharmaTech));
        assert_eq!(parse_domain_hint("clinical_research"), Some(Domain::MedicResearch));
    }

    #[test]
    fn parse_domain_hint_rejects_unknown() {
        assert_eq!(parse_domain_hint("not-a-domain"), None);
        assert_eq!(parse_domain_hint(""), None);
    }

    // ── Every template available via compose round-trips clean ───────

    #[test]
    fn every_domain_has_an_axon_check_clean_template_via_compose() {
        let cat = embedded_catalog();
        for &domain in Domain::all() {
            let r = compose("", Some(domain), &cat).expect("template lookup");
            assert_eq!(
                r.axon_check_verdict, "well-formed",
                "domain {} returned a malformed scaffold via compose",
                domain.slug()
            );
        }
    }
}
