//! AXON Standard Library — built-in personas, anchors, flows, and tools.
//!
//! This module provides a static registry of all stdlib components,
//! mirroring the Python `axon.stdlib` module with hardcoded definitions.
//!
//! 4 namespaces × 39 total entries:
//!   - 8 personas (Analyst, LegalExpert, Coder, Researcher, Writer, Summarizer, Critic, Translator)
//!   - 12 anchors (8 core + 4 logic/epistemic)
//!   - 8 flows (Summarize, ExtractEntities, CompareDocuments, etc.)
//!   - 11 tools (WebSearch, CodeExecutor, DocumentRenderer/Reader/Editor, etc.)

// ── Entry types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct StdlibPersona {
    pub name: &'static str,
    pub description: &'static str,
    pub tone: &'static str,
    pub domain: &'static [&'static str],
    pub confidence_threshold: f64,
    pub cite_sources: bool,
    pub category: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Clone)]
pub struct StdlibAnchor {
    pub name: &'static str,
    pub description: &'static str,
    pub severity: &'static str,
    pub require: &'static [&'static str],
    pub reject: &'static [&'static str],
    pub confidence_floor: Option<f64>,
    pub version: &'static str,
}

#[derive(Debug, Clone)]
pub struct StdlibFlow {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: &'static [(&'static str, &'static str)],
    pub return_type: &'static str,
    pub category: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Clone)]
pub struct StdlibTool {
    pub name: &'static str,
    pub description: &'static str,
    pub provider: &'static str,
    pub timeout: u32,
    pub requires_api_key: bool,
    pub sandbox: bool,
    pub version: &'static str,
}

#[derive(Debug, Clone)]
pub enum StdlibEntry {
    Persona(StdlibPersona),
    Anchor(StdlibAnchor),
    Flow(StdlibFlow),
    Tool(StdlibTool),
}

impl StdlibEntry {
    pub fn name(&self) -> &str {
        match self {
            StdlibEntry::Persona(p) => p.name,
            StdlibEntry::Anchor(a) => a.name,
            StdlibEntry::Flow(f) => f.name,
            StdlibEntry::Tool(t) => t.name,
        }
    }

    pub fn description(&self) -> &str {
        match self {
            StdlibEntry::Persona(p) => p.description,
            StdlibEntry::Anchor(a) => a.description,
            StdlibEntry::Flow(f) => f.description,
            StdlibEntry::Tool(t) => t.description,
        }
    }

    pub fn version(&self) -> &str {
        match self {
            StdlibEntry::Persona(p) => p.version,
            StdlibEntry::Anchor(a) => a.version,
            StdlibEntry::Flow(f) => f.version,
            StdlibEntry::Tool(t) => t.version,
        }
    }
}

// ── Personas (8) ────────────────────────────────────────────────────────────

pub const PERSONAS: &[StdlibPersona] = &[
    StdlibPersona {
        name: "Analyst",
        description: "Expert data analyst with deep pattern recognition skills.",
        tone: "precise",
        domain: &["data analysis", "pattern recognition", "statistics"],
        confidence_threshold: 0.85,
        cite_sources: true,
        category: "analysis",
        version: "0.1.0",
    },
    StdlibPersona {
        name: "LegalExpert",
        description: "A precise legal analyst for contract review, compliance checking, and regulatory analysis. Does not provide legal advice.",
        tone: "precise",
        domain: &["contract law", "compliance", "regulation"],
        confidence_threshold: 0.90,
        cite_sources: true,
        category: "legal",
        version: "0.1.0",
    },
    StdlibPersona {
        name: "Coder",
        description: "A technical coding expert for software development, debugging, code review, and architectural decisions.",
        tone: "technical",
        domain: &["software engineering", "debugging", "architecture"],
        confidence_threshold: 0.80,
        cite_sources: false,
        category: "engineering",
        version: "0.1.0",
    },
    StdlibPersona {
        name: "Researcher",
        description: "A rigorous academic researcher specializing in literature review, source verification, and methodological analysis.",
        tone: "technical",
        domain: &["academic research", "citation", "methodology"],
        confidence_threshold: 0.82,
        cite_sources: true,
        category: "research",
        version: "0.1.0",
    },
    StdlibPersona {
        name: "Writer",
        description: "A creative writer for content generation, editing, copywriting, and narrative crafting.",
        tone: "creative",
        domain: &["content creation", "editing", "copywriting"],
        confidence_threshold: 0.75,
        cite_sources: false,
        category: "creative",
        version: "0.1.0",
    },
    StdlibPersona {
        name: "Summarizer",
        description: "A condensation specialist that distills complex information into clear, concise summaries.",
        tone: "friendly",
        domain: &["condensation", "abstraction", "synthesis"],
        confidence_threshold: 0.80,
        cite_sources: false,
        category: "analysis",
        version: "0.1.0",
    },
    StdlibPersona {
        name: "Critic",
        description: "A formal evaluator specializing in critical assessment, risk analysis, and quality review.",
        tone: "formal",
        domain: &["evaluation", "risk assessment", "review"],
        confidence_threshold: 0.85,
        cite_sources: true,
        category: "analysis",
        version: "0.1.0",
    },
    StdlibPersona {
        name: "Translator",
        description: "A multilingual translator with deep understanding of cultural nuances and idiomatic expressions.",
        tone: "conversational",
        domain: &["cross-language translation", "cross-cultural adaptation"],
        confidence_threshold: 0.80,
        cite_sources: false,
        category: "translation",
        version: "0.1.0",
    },
];

// ── Anchors (12) ────────────────────────────────────────────────────────────

pub const ANCHORS: &[StdlibAnchor] = &[
    // Core anchors (8)
    StdlibAnchor {
        name: "NoHallucination",
        description: "Requires cited sources for all claims. Rejects speculation and unverifiable assertions.",
        severity: "error",
        require: &["source_citation"],
        reject: &["speculation", "unverifiable_claim"],
        confidence_floor: Some(0.80),
        version: "0.1.0",
    },
    StdlibAnchor {
        name: "FactualOnly",
        description: "Restricts output to factual claims only. No opinions, unless explicitly declared as Opinion type.",
        severity: "error",
        require: &["factual_grounding"],
        reject: &["opinion", "speculation"],
        confidence_floor: Some(0.85),
        version: "0.1.0",
    },
    StdlibAnchor {
        name: "SafeOutput",
        description: "Rejects harmful content, violence, and hate speech.",
        severity: "error",
        require: &[],
        reject: &["harmful_content", "violence", "hate_speech"],
        confidence_floor: None,
        version: "0.1.0",
    },
    StdlibAnchor {
        name: "PrivacyGuard",
        description: "Prevents exposure of PII (SSNs, credit cards, emails, phone numbers).",
        severity: "error",
        require: &[],
        reject: &["pii", "personal_data", "ssn", "phone_number"],
        confidence_floor: None,
        version: "0.1.0",
    },
    StdlibAnchor {
        name: "NoBias",
        description: "Enforces political and demographic neutrality. Detects loaded language and explicit bias.",
        severity: "warning",
        require: &[],
        reject: &["political_bias", "demographic_bias", "gender_bias"],
        confidence_floor: None,
        version: "0.1.0",
    },
    StdlibAnchor {
        name: "ChildSafe",
        description: "Ensures all content is appropriate for minors. Rejects adult content, graphic violence, profanity, and drugs.",
        severity: "error",
        require: &[],
        reject: &["adult_content", "violence", "profanity", "drugs"],
        confidence_floor: None,
        version: "0.1.0",
    },
    StdlibAnchor {
        name: "NoCodeExecution",
        description: "Prevents code execution, system commands, and file operations.",
        severity: "error",
        require: &[],
        reject: &["code_execution", "system_command", "file_write"],
        confidence_floor: None,
        version: "0.1.0",
    },
    StdlibAnchor {
        name: "AuditTrail",
        description: "Forces full reasoning trace in output. Requires visible reasoning steps for audit and review purposes.",
        severity: "warning",
        require: &["human_review"],
        reject: &[],
        confidence_floor: None,
        version: "0.1.0",
    },
    // Logic & Epistemic anchors (4)
    StdlibAnchor {
        name: "SyllogismChecker",
        description: "Syntactically enforces standard logical format using 'Premise:' and 'Conclusion:' identifiers.",
        severity: "error",
        require: &["logical_structure"],
        reject: &[],
        confidence_floor: None,
        version: "0.1.0",
    },
    StdlibAnchor {
        name: "ChainOfThoughtValidator",
        description: "Forces the model to explicitly write out step sequences before producing a final answer.",
        severity: "error",
        require: &["step_by_step"],
        reject: &[],
        confidence_floor: None,
        version: "0.1.0",
    },
    StdlibAnchor {
        name: "RequiresCitation",
        description: "Strict verification enforcing explicit academic-style inline citations or external URLs.",
        severity: "error",
        require: &["inline_citation"],
        reject: &["unverifiable_claim"],
        confidence_floor: None,
        version: "0.1.0",
    },
    StdlibAnchor {
        name: "AgnosticFallback",
        description: "Requires the model to explicitly state a lack of information instead of speculating.",
        severity: "error",
        require: &["epistemic_honesty"],
        reject: &["unwarranted_speculation"],
        confidence_floor: None,
        version: "0.1.0",
    },
];

// ── Flows (8) ───────────────────────────────────────────────────────────────

pub const FLOWS: &[StdlibFlow] = &[
    StdlibFlow {
        name: "Summarize",
        description: "Condense any document into a concise summary.",
        parameters: &[("doc", "Document")],
        return_type: "Summary",
        category: "analysis",
        version: "0.1.0",
    },
    StdlibFlow {
        name: "ExtractEntities",
        description: "Extract and classify named entities from a document.",
        parameters: &[("doc", "Document")],
        return_type: "EntityMap",
        category: "extraction",
        version: "0.1.0",
    },
    StdlibFlow {
        name: "CompareDocuments",
        description: "Compare two documents side-by-side with detailed analysis.",
        parameters: &[("doc_a", "Document"), ("doc_b", "Document")],
        return_type: "StructuredReport",
        category: "analysis",
        version: "0.1.0",
    },
    StdlibFlow {
        name: "TranslateDocument",
        description: "Translate a document with cultural context preservation.",
        parameters: &[("doc", "Document"), ("target_lang", "String")],
        return_type: "Translation",
        category: "translation",
        version: "0.1.0",
    },
    StdlibFlow {
        name: "FactCheck",
        description: "Verify factual claims with sourced evidence.",
        parameters: &[("claims", "Document")],
        return_type: "StructuredReport",
        category: "verification",
        version: "0.1.0",
    },
    StdlibFlow {
        name: "SentimentAnalysis",
        description: "Analyze tone and sentiment with nuanced scoring.",
        parameters: &[("doc", "Document")],
        return_type: "SentimentScore",
        category: "analysis",
        version: "0.1.0",
    },
    StdlibFlow {
        name: "ClassifyContent",
        description: "Classify content into user-defined categories.",
        parameters: &[("doc", "Document"), ("categories", "String")],
        return_type: "EntityMap",
        category: "classification",
        version: "0.1.0",
    },
    StdlibFlow {
        name: "GenerateReport",
        description: "Generate a structured report from raw data.",
        parameters: &[("data", "Document")],
        return_type: "StructuredReport",
        category: "reporting",
        version: "0.1.0",
    },
];

// ── Tools (12) ───────────────────────────────────────────────────────────────

pub const TOOLS: &[StdlibTool] = &[
    StdlibTool {
        name: "WebSearch",
        description: "Live web search via Brave Search API.",
        provider: "http",
        timeout: 10,
        requires_api_key: true,
        sandbox: false,
        version: "0.1.0",
    },
    StdlibTool {
        name: "CodeExecutor",
        description: "Safe sandboxed code execution.",
        provider: "",
        timeout: 30,
        requires_api_key: false,
        sandbox: true,
        version: "0.1.0",
    },
    StdlibTool {
        name: "FileReader",
        description: "Read local or remote files.",
        provider: "",
        timeout: 5,
        requires_api_key: false,
        sandbox: false,
        version: "0.1.0",
    },
    // v2.54.0 — Inferred Ingestion: extract a PDF's text via the registered
    // extraction engine (born `Inferred`, measured confidence). Native dispatch
    // arm — with no engine, a TYPED refusal, never model-invented text.
    StdlibTool {
        name: "PDFExtractor",
        description: "Extract text from a PDF as born-Inferred spans with measured confidence (native).",
        provider: "native",
        timeout: 15,
        requires_api_key: false,
        sandbox: false,
        version: "0.1.0",
    },
    // v2.54.0 — OCR: read an image's text (`ots:transform:image:text`) via
    // the deterministic IDP-E / registered engine. Distinct from vision.
    StdlibTool {
        name: "ImageTextExtractor",
        description: "OCR an image into born-Inferred text spans with measured confidence (native).",
        provider: "native",
        timeout: 20,
        requires_api_key: false,
        sandbox: false,
        version: "0.1.0",
    },
    // v2.54.0 — vision captioning (`ots:transform:image:description`). A
    // model's interpretation of a scene — born `Inferred`, same ceiling as OCR
    // but a distinct capability, cost, and RBAC posture. Provider-backed.
    StdlibTool {
        name: "ImageAnalyzer",
        description: "Describe an image via a vision model as born-Inferred spans (native, provider-backed).",
        provider: "native",
        timeout: 20,
        requires_api_key: true,
        sandbox: false,
        version: "0.1.0",
    },
    StdlibTool {
        name: "Calculator",
        description: "Precise arithmetic with safe expression eval.",
        provider: "",
        timeout: 2,
        requires_api_key: false,
        sandbox: true,
        version: "0.1.0",
    },
    StdlibTool {
        name: "DateTimeTool",
        description: "Temporal reasoning — current date, time, timestamps.",
        provider: "",
        timeout: 1,
        requires_api_key: false,
        sandbox: true,
        version: "0.1.0",
    },
    StdlibTool {
        name: "APICall",
        description: "Generic REST API caller.",
        provider: "",
        timeout: 30,
        requires_api_key: true,
        sandbox: false,
        version: "0.1.0",
    },
    // v2.53.0 — Native Document Synthesis: renders a `document` IR + bound
    // values into deterministic OOXML (DOCX/PPTX/XLSX) bytes. A REAL native
    // executor (`tool_executor::document_render_execute`), never a fall-through
    // to the model (v2.53.0 section 8).
    StdlibTool {
        name: "DocumentRenderer",
        description: "Render a declared `document` into deterministic DOCX/PPTX/XLSX bytes (native).",
        provider: "native",
        timeout: 30,
        requires_api_key: false,
        sandbox: false,
        version: "0.1.0",
    },
    // v2.54.0 — read an ingested DOCX/PPTX/XLSX into a bounded, born-
    // Untrusted, Parsed text tree. A REAL native executor, never a fall-through.
    StdlibTool {
        name: "DocumentReader",
        description: "Read an ingested DOCX/PPTX/XLSX into a bounded, born-Untrusted Parsed text tree (native).",
        provider: "native",
        timeout: 30,
        requires_api_key: false,
        sandbox: true,
        version: "0.1.0",
    },
    // v2.54.0 — surgical edit with a per-part hash manifest (native).
    StdlibTool {
        name: "DocumentEditor",
        description: "Surgically edit an OOXML document; touch only targeted parts + emit a hash manifest (native).",
        provider: "native",
        timeout: 30,
        requires_api_key: false,
        sandbox: false,
        version: "0.1.0",
    },
];

// ── Public API ──────────────────────────────────────────────────────────────

pub const VALID_NAMESPACES: &[&str] = &["anchors", "flows", "personas", "tools"];

/// List all entries in a namespace, sorted by name.
pub fn list_namespace(namespace: &str) -> Vec<StdlibEntry> {
    match namespace {
        "personas" => PERSONAS.iter().map(|p| StdlibEntry::Persona(p.clone())).collect(),
        "anchors" => ANCHORS.iter().map(|a| StdlibEntry::Anchor(a.clone())).collect(),
        "flows" => FLOWS.iter().map(|f| StdlibEntry::Flow(f.clone())).collect(),
        "tools" => TOOLS.iter().map(|t| StdlibEntry::Tool(t.clone())).collect(),
        _ => Vec::new(),
    }
}

/// Resolve a specific entry by name across all namespaces.
pub fn resolve(name: &str) -> Option<StdlibEntry> {
    if let Some(p) = PERSONAS.iter().find(|p| p.name == name) {
        return Some(StdlibEntry::Persona(p.clone()));
    }
    if let Some(a) = ANCHORS.iter().find(|a| a.name == name) {
        return Some(StdlibEntry::Anchor(a.clone()));
    }
    if let Some(f) = FLOWS.iter().find(|f| f.name == name) {
        return Some(StdlibEntry::Flow(f.clone()));
    }
    if let Some(t) = TOOLS.iter().find(|t| t.name == name) {
        return Some(StdlibEntry::Tool(t.clone()));
    }
    None
}

/// Check if a name exists in any namespace.
pub fn has(name: &str) -> bool {
    resolve(name).is_some()
}

/// Total count of all stdlib entries.
pub fn total_count() -> usize {
    PERSONAS.len() + ANCHORS.len() + FLOWS.len() + TOOLS.len()
}
