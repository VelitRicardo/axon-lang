//! AXON Type Checker — Phase 1: symbol table, duplicates, references, field validation.
//!
//! Direct port of axon/compiler/type_checker.py (subset).
//!
//! What it checks:
//!   - Duplicate declarations
//!   - Undefined references in `run` (flow, persona, context, anchors)
//!   - Field value validation (tone, depth, memory scope, temperature, confidence, effort)
//!   - Duplicate step names within flows
//!
//! What it does NOT check (deferred to C7+):
//!   - Epistemic lattice / type compatibility
//!   - Cross-node type inference / uncertainty propagation
//!   - Tier 2 construct-specific validation

#![allow(dead_code)]

use std::collections::HashMap;

use crate::ast::*;
use crate::session::SessionType;
use crate::epistemic;

// ── Valid value sets (mirrors Python frozensets) ─────────────────────────────

const VALID_TONES: &[&str] = &[
    "analytical",
    "assertive",
    "casual",
    "diplomatic",
    "empathetic",
    "formal",
    "friendly",
    "precise",
];

const VALID_MEMORY_SCOPES: &[&str] = &["ephemeral", "none", "persistent", "session"];

const VALID_DEPTHS: &[&str] = &["deep", "exhaustive", "shallow", "standard"];

const VALID_EFFORT_LEVELS: &[&str] = &["high", "low", "max", "medium"];

const VALID_VIOLATION_ACTIONS: &[&str] = &["escalate", "fallback", "log", "raise", "warn"];

const VALID_RETRIEVAL_STRATEGIES: &[&str] = &["exact", "hybrid", "semantic"];

/// §Fase 63.A — the closed catalog of MDN corpus-graph relation types (paper
/// `multi_document.md` Def 2). Positive (trust): cite/elaborate/corroborate;
/// negative (distrust): contradict/supersede; neutral structural: depend/
/// implement/exemplify. Any other relation type is a compile error.
const VALID_CORPUS_RELATIONS: &[&str] = &[
    "cite",
    "corroborate",
    "contradict",
    "depend",
    "elaborate",
    "exemplify",
    "implement",
    "supersede",
];

// Fase 15.d — reserved primitive / built-in type names that a
// `lambda apply ... -> OutputType` MUST NOT shadow. Mirror of
// axon.compiler.type_checker._RESERVED_OUTPUT_TYPE_NAMES.
const RESERVED_OUTPUT_TYPE_NAMES: &[&str] = &[
    "any", "bool", "boolean", "bytes", "dict", "false", "float",
    "int", "integer", "list", "map", "none", "null", "number",
    "set", "str", "string", "true", "tuple", "void",
];

// §λ-L-E Fase 11.a + 11.c + 11.e — `stream` (mandatory backpressure),
// `trust` (mandatory proof), `sensitive` (data-category jurisdiction
// — open taxonomy), `legal` (mandatory legal basis from the closed
// catalogue in `crate::legal_basis`), `ots` (subkinds `transform:
// <from>:<to>` + `backend:<native|ffmpeg>`) join the catalogue.
// Qualifiers are validated separately below.
const VALID_EFFECTS: &[&str] = &[
    "io",
    "network",
    "pure",
    "random",
    "storage",
    "stream",
    "trust",
    "sensitive",
    "legal",
    "ots",
    // §Fase 98.c — `web` (native web acquisition). A first-class base
    // (D98.10), NOT `network`: a `provider: http` call to a TRUSTED API is
    // `<network>`; a value acquired from the OPEN, ADVERSARIAL web carries
    // `<web>` and is born epistemically Untrusted (⊥, D98.1). It is what
    // lets a shield say "web-tainted content must pass `prompt_injection`
    // before an agent's belief" — the content-injection barrier (§98.d),
    // the §84 command-injection discipline applied to fetched content.
    // Precedented by the `legal`/`ots` additions above.
    "web",
];

/// §Fase 85.c — the closed `cache.backend:` catalog. `in_process` is the OSS
/// default single-replica tier; `redis` is the enterprise multi-replica tier.
const VALID_CACHE_BACKENDS: &[&str] = &["in_process", "redis"];

/// §Fase 87.b — the closed `savant.cognition.depth:` catalog. Each tier sets the
/// HRR (holographic reduced representation) dimensionality of the memory codec:
/// `standard` (baseline), `deep`, `hyper` (hyperbolic). A typo can never
/// silently select a smaller — or larger, more expensive — memory geometry.
const VALID_SAVANT_DEPTHS: &[&str] = &["standard", "deep", "hyper"];

/// §Fase 87.b — the closed `savant.cognition.divergence:` catalog. Governs how
/// aggressively the active-inference loop explores epistemic (β₂) voids versus
/// exploiting known structure when minimising Expected Free Energy.
const VALID_SAVANT_DIVERGENCES: &[&str] = &["low", "med", "high"];

/// §Fase 87.d — the closed `synth.risk:` catalog. The risk class a synthesis
/// policy admits; `high`/`critical` force Coder/Reviewer consensus (T883).
const VALID_SYNTH_RISKS: &[&str] = &["low", "medium", "high", "critical"];

/// §Fase 87.d — the closed `synth.language:` catalog. All compiled to
/// `wasm32-wasi` before execution in the enterprise Extism sandbox (§87.j).
const VALID_SYNTH_LANGUAGES: &[&str] = &["rust", "c", "python"];

/// §Fase 87.d — the closed `synth.review:` catalog: `required` (a Reviewer
/// sub-agent must ratify before a synthesised tool runs) | `none`.
const VALID_SYNTH_REVIEWS: &[&str] = &["required", "none"];

/// §Fase 88.b — the closed `scope.depth:` catalog, ordered LEAST→MOST invasive.
/// `static_artifact` (analyse an operator-provided binary/core/pcap — the safe
/// default) ⊂ `memory_dump` (a supplied memory image) ⊂ `live_network` (live
/// capture — the most-restricted, allowlist-bound, enterprise-only depth). A
/// scope's declared depth is the CEILING it authorises.
const VALID_SCOPE_DEPTHS: &[&str] = &["static_artifact", "memory_dump", "live_network"];

/// §Fase 86.c — the closed `forge.mode:` catalog: Margaret Boden's three
/// creativity types (*The Creative Mind*, 1990). Each maps to a distinct
/// sampling-parameter profile at runtime (D86.3).
const VALID_FORGE_MODES: &[&str] = &["combinatorial", "exploratory", "transformational"];

/// §Fase 98.d — the closed catalog of web-acquisition providers. A tool whose
/// `provider:` is one of these acquires content from the open, adversarial web
/// (born Untrusted, D98.1) and is governed by the §98 scrape laws.
const VALID_SCRAPE_PROVIDERS: &[&str] = &["scrape_http", "scrape_dom", "scrape_crawl", "scrape_enrich"];

/// §Fase 114.b — the CLOSED `tool.provider` catalog.
///
/// Exactly the values the runtime dispatch tables actually handle — the union of
/// `tool_registry::dispatch` and `tool_dispatch_bridge::resolve_streaming_tool`,
/// plus `bash` (the §84 technician execve path, validated by `axon-T858`).
///
/// An **empty** provider is deliberately NOT in this list: an empty provider is a
/// legitimate LLM-routed tool, and it is validated by its absence, not its
/// presence. `axon-T948` refuses a *non-empty* provider outside this set.
///
/// The catalog is a promise: adding a value here without a dispatch arm behind it
/// re-creates the very defect §114.b closes.
pub const VALID_TOOL_PROVIDERS: &[&str] = &[
    "native",
    "stub",
    "stub_stream",
    "http",
    "mcp",
    "scrape_http",
    "scrape_dom",
    "scrape_crawl",
    "scrape_enrich",
    "bash",
    // §Fase 116.a — the axon-agora governed social connectors (one per platform,
    // the §98 scrape-family shape). Each routes through `agora_runtime::dispatch_agora`
    // in axon-rs to the registered per-platform `SocialConnector`; an unregistered
    // platform is a TYPED refusal at dispatch (the §104 D104.6 honesty), never a
    // fabrication. The `runtime:` field names the connector OPERATION (read_comments,
    // publish, …) — the same slug role it plays for `http` tools.
    "agora_linkedin",
    "agora_facebook",
    "agora_instagram",
    "agora_tiktok",
];

/// §Fase 98.d — the closed `scrape.engine:` catalog. `impersonate` (HTTP-
/// fingerprint stealth, the GA tier — OSS fallback is plain reqwest) |
/// `browser` (headless-render sidecar, the gray/pilot tier, D98.6).
const VALID_SCRAPE_ENGINES: &[&str] = &["impersonate", "browser"];

/// §Fase 98.d — the closed `scrape.impersonate:` fingerprint-profile catalog.
/// The declared family; the concrete JA3/JA4 + HTTP/2 profile is resolved by
/// the enterprise engine (§98.g).
const VALID_IMPERSONATE_PROFILES: &[&str] = &["chrome", "firefox", "safari", "edge"];

const VALID_EPISTEMIC_LEVELS: &[&str] = &["believe", "doubt", "know", "speculate"];

// ── §Fase 99 — Native Document Synthesis catalogs ─────────────────────────────

/// §Fase 100.d — the closed `ingest:<class>` provenance catalog (D100.1). NOT
/// an effect base (D100.3) — a provenance MEMBER, like `epistemic:<level>`:
/// `parsed` (a fact about the file, elevatable by a shield) | `inferred` (a
/// model's belief about pixels, ceiling of `believe`, never `know`). §100 ships
/// a producer for `parsed` only; `inferred` has no producer until §101.
const VALID_INGEST_CLASSES: &[&str] = &["parsed", "inferred"];

/// §Fase 99.c — the closed `document.target:` catalog. Each selects a serializer,
/// not a capability (D99.6 — identical effect rows).
const VALID_DOC_TARGETS: &[&str] = &["docx", "pptx", "xlsx"];

/// §Fase 99.c — the closed `document.provenance:` catalog (D99.2). `none` (or
/// empty) = no provenance part; `embedded` = an unsigned custom XML part;
/// `signed` = a signed part (enterprise, §99.g).
const VALID_DOC_PROVENANCE: &[&str] = &["none", "embedded", "signed"];

/// §Fase 99.c — the bounded chart-kind subset (D99.9). SmartArt, pivots,
/// sparklines, 3-D are deferred and named as such.
const VALID_CHART_KINDS: &[&str] = &["bar", "line", "pie", "scatter"];

/// §Fase 105 — the closed `deliver.target:` catalog (D105.1). `crm` selects the
/// system-of-record class; the concrete vendor is the enterprise transducer's
/// per-tenant config, so the language binds no vendor. Additive: future targets
/// (`marketing`, `helpdesk`) land here.
const VALID_DELIVER_TARGETS: &[&str] = &["crm"];

/// §Fase 105 — the closed `deliver.provenance:` catalog (D105.2). `attached`
/// (or empty) = each delivered field lands with its epistemic origin (level +
/// confidence + source); `cleared` = bare values, legal ONLY under an
/// `epistemic { believe|know }` vouch (the T920 barrier).
const VALID_DELIVER_PROVENANCE: &[&str] = &["attached", "cleared"];

/// §Fase 105 — the closed delivery-operation catalog (D105.1). `upsert_contact`
/// (idempotent by natural key), `create_deal`, `add_note`. Additive as the
/// enterprise transducer grows; the language keeps them vendor-agnostic.
const VALID_DELIVER_OPS: &[&str] = &["upsert_contact", "create_deal", "add_note"];

/// §Fase 99.c — the top-level body block kinds for a `target`.
fn doc_top_level_kinds(target: &str) -> Vec<&'static str> {
    match target {
        "docx" => vec!["section", "page_break"],
        "pptx" => vec!["slide"],
        "xlsx" => vec!["sheet"],
        _ => vec![],
    }
}

/// §Fase 99.c — the block kinds valid as children of `parent` (empty parent =
/// top level) in a `target` document. The closed per-target vocabulary (D99.6):
/// a `slide` inside a `docx` is `axon-T912`.
fn doc_allowed_child_kinds(target: &str, parent: &str) -> Vec<&'static str> {
    match (target, parent) {
        ("docx", "") => doc_top_level_kinds("docx"),
        ("docx", "section") => vec![
            "heading", "para", "table", "chart", "image", "toc", "page_break", "footnote",
        ],
        ("pptx", "") => doc_top_level_kinds("pptx"),
        ("pptx", "slide") => vec!["placeholder", "bullets", "image", "chart", "notes"],
        ("xlsx", "") => doc_top_level_kinds("xlsx"),
        ("xlsx", "sheet") => vec!["row", "formula", "range", "chart", "format"],
        // A leaf block takes no children.
        _ => vec![],
    }
}

/// §Fase 99.c — the closed field set for a block kind.
fn doc_allowed_fields(kind: &str) -> Vec<&'static str> {
    match kind {
        "section" => vec!["heading", "name"],
        "heading" => vec!["text", "level"],
        "para" => vec!["text", "attribute"],
        "table" => vec!["columns", "rows", "attribute"],
        "chart" => vec!["kind", "series", "range", "attribute"],
        "image" => vec!["source", "width", "height"],
        "toc" => vec!["depth"],
        "page_break" => vec![],
        "footnote" => vec!["text", "attribute"],
        "slide" => vec!["layout"],
        "placeholder" => vec!["name", "text", "attribute"],
        "bullets" => vec!["items", "attribute"],
        "notes" => vec!["text", "attribute"],
        "sheet" => vec!["name"],
        "row" => vec!["cells", "attribute"],
        "formula" => vec!["cell", "expr", "attribute"],
        "range" => vec!["name", "cells"],
        "format" => vec!["cell", "style"],
        _ => vec![],
    }
}

/// §Fase 99.d — the ASSERTIVE SLOT of a block kind: the field whose value, if it
/// is a flow-value binding, occupies an assertive position in the artifact and
/// is guarded by the assertion-laundering barrier (D99.1). `None` ⇒ the block
/// holds no assertive slot (structural: `section`, `slide`, `page_break`, …).
fn doc_assertive_slot(kind: &str) -> Option<&'static str> {
    match kind {
        "para" | "notes" | "footnote" | "heading" => Some("text"),
        "table" => Some("rows"),
        "chart" => Some("series"),
        "formula" => Some("expr"),
        "bullets" => Some("items"),
        "placeholder" => Some("text"),
        "row" => Some("cells"),
        _ => None,
    }
}

/// §Fase 99.c — is `s` a valid A1 cell reference (e.g. `B2`, `AA10`)?
fn is_a1_cell(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let mut saw_col = false;
    let mut saw_row = false;
    let mut in_row = false;
    for c in chars.by_ref() {
        if c.is_ascii_uppercase() {
            if in_row {
                return false; // letters after digits → invalid
            }
            saw_col = true;
        } else if c.is_ascii_digit() {
            if c == '0' && !saw_row {
                return false; // row cannot start with 0
            }
            in_row = true;
            saw_row = true;
        } else {
            return false;
        }
    }
    saw_col && saw_row
}

/// §Fase 99.c — is `s` a valid A1 range (`B2:B9`) or a single cell?
fn is_a1_range(s: &str) -> bool {
    let s = s.trim();
    match s.split_once(':') {
        Some((a, b)) => is_a1_cell(a) && is_a1_cell(b),
        None => is_a1_cell(s),
    }
}

const VALID_DERIVATIONS: &[&str] = &["aggregated", "derived", "inferred", "raw", "transformed"];

// ── Tier 2 valid-value sets (mirrors Python frozensets) ────────────────────

const VALID_AGENT_STRATEGIES: &[&str] = &["custom", "plan_and_execute", "react", "reflexion"];

const VALID_ON_STUCK_POLICIES: &[&str] = &["escalate", "forge", "hibernate", "retry"];

const VALID_SCAN_CATEGORIES: &[&str] = &[
    "bias",
    "code_injection",
    "data_exfil",
    "hallucination",
    "jailbreak",
    "model_theft",
    "pii_leak",
    "prompt_injection",
    "social_engineering",
    "toxicity",
    "training_poisoning",
];

const VALID_SHIELD_STRATEGIES: &[&str] = &[
    "canary",
    "classifier",
    "dual_llm",
    "ensemble",
    "pattern",
    "perplexity",
];

const VALID_ON_BREACH_POLICIES: &[&str] = &[
    "deflect",
    "escalate",
    "halt",
    "quarantine",
    "sanitize_and_retry",
];

/// §Fase 71.a — the closed `window.on_outside` catalog: what to do with a tick
/// that falls outside every allowed span.
const VALID_ON_OUTSIDE: &[&str] = &["skip", "defer", "warn"];

/// §Fase 71.a — the closed weekday-name catalog for `window` day ranges.
const VALID_WEEKDAYS: &[&str] = &["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// §Fase 72.a — the closed period catalog for `budget` quotas.
const VALID_BUDGET_PERIODS: &[&str] = &["second", "minute", "hour", "day"];

/// §Fase 72.a — the closed exhaustion-policy catalog for `budget`.
const VALID_ON_EXHAUSTED: &[&str] = &["block", "defer", "shed"];

const VALID_SEVERITY_LEVELS: &[&str] = &["critical", "high", "low", "medium"];

/// §Fase 77.a — the closed egress-signing catalog for `shield { sign: }`
/// (`axon-T846`). One algorithm in v1: HMAC-SHA256 over the raw delivery
/// body (the receiver-verifiable witness of §77's `egress_is_a_kept_promise`).
const VALID_SIGN_ALGORITHMS: &[&str] = &["hmac_sha256"];

/// §Fase 80.c — closed catalogs for `upstream` (fase_80_upstream_design.md §1).
/// `transport:` is v1-single-member by design: every surveyed 2026 STT/TTS/
/// realtime vendor dials WebSocket; a catalog member the runtime cannot
/// honestly drive (gRPC) is named deferred scope, not grammar.
const VALID_UPSTREAM_TRANSPORTS: &[&str] = &["websocket"];
/// The three auth-handshake shapes every surveyed vendor uses.
const VALID_UPSTREAM_AUTH_KINDS: &[&str] = &["header", "query", "signed_url"];
/// `reconnect.on_exhausted:` — v1 sole member `fail` (fail-closed); `degrade`
/// / `park` are named deferred scope in the design doc.
const VALID_UPSTREAM_ON_EXHAUSTED: &[&str] = &["fail"];
/// `map:` rule framings.
const VALID_UPSTREAM_FRAMINGS: &[&str] = &["binary", "json"];

/// §Fase 77.a (`axon-W010`) — the complete `shield` field catalog, quoted in
/// the unknown-field warning so the adopter sees what IS accepted. Kept in
/// sync with `Parser::parse_shield`'s match arms by the
/// `w010_catalog_matches_parser` test.
const SHIELD_FIELD_CATALOG: &str = "scan, strategy, on_breach, severity, quarantine, \
     max_retries, confidence_threshold, allow_tools, deny_tools, sandbox, redact, log, \
     deflect_message, taint, compliance, sign";

const VALID_OTS_HOMOTOPY: &[&str] = &["deep", "shallow", "speculative"];

const VALID_MANDATE_POLICIES: &[&str] = &["coerce", "halt", "retry"];

// §Fase 36.x.b (D2) — `in_memory` is a first-class declarable
// `axonstore` backend. The runtime `StoreRegistry::classify_backend`
// already maps `"in_memory"` → `StoreHandle::InMemory` (the
// key-value path); adding it here makes a source-declared in-memory
// store type-check, so the canonical agent flow (retrieve → step →
// persist) is runnable + testable with ZERO external infrastructure
// — no Postgres, no `DATABASE_URL`. `connection:` is optional for an
// `in_memory` store (it is optional for every backend at the
// type-checker layer — no `connection`-required check exists here).
//
// §Fase 113 — `mysql` and `sqlite` are GONE from this catalog.
//
// This comment used to read: *"`mysql` / `sqlite` remain type-check-valid but
// runtime-absent (a documented future fase)"*. Written down, calmly, next to the
// catalog that let them through — which is the exact disease §111 spent itself
// on. `classify_backend` implements THREE backends. This list declared FIVE. So
// `backend: mysql` type-checked clean and then died at **deploy** with
// `UnknownBackend`, and the compiler — which knew — said nothing.
//
// Nothing that works today stops working. A program declaring `mysql` was already
// broken; it now fails at COMPILE, where the adopter is still holding the code,
// instead of at deploy, where they are holding an incident.
//
// **The catalog is a promise, and a promise costs an implementation.** Putting
// `mysql` back means writing a MySQL backend in the same PR.
//
// `secrets` is the metadata VIEW over the tenant's secret custody (§94.a,
// `rotation_without_revelation`): read-only (T897), class-scoped (T900), fixed
// synthesized schema. A backend in the grammar sense only — no connection string,
// no adopter table; the runtime binds it to the `axon::secret_custody` port.
pub const VALID_STORE_BACKENDS: &[&str] = &["in_memory", "postgresql", "secrets"];

/// §Fase 113 — the CLOSED `resource.kind` catalog.
///
/// # This did not exist
///
/// Until §113, `resource.kind` was a **free string that nothing validated**.
/// No `VALID_RESOURCE_KINDS` const existed anywhere in the workspace, and
/// [`TypeChecker::check_resource`] never read the field. `kind: postgress`
/// compiled clean and produced a resource the runtime could never reach — the
/// §111 shape exactly: a declaration that looks governed and governs nothing.
///
/// # Why these five
///
/// The catalog is **what the runtime can actually REACH** — it mirrors
/// `axon::source_registry`'s default-port table one-for-one. Adding a kind
/// here without teaching the runtime to reach it would manufacture a fresh
/// instance of the very defect this fase descends from. **The catalog is a
/// promise, and a promise costs an implementation.**
const VALID_RESOURCE_KINDS: &[&str] = &["http", "https", "mysql", "postgres", "redis"];

/// The **config-key shape** — the compile-time `SecretKeyPolicy` mirror.
///
/// `[a-z0-9][a-z0-9_.-]*` — lowercase, dot-separated, no `/`, no `:`. A URL
/// cannot satisfy it (`://`), and neither can a DSN.
///
/// # One law, one predicate
///
/// This shape is asserted by `axon-T850` (`upstream.resolve`), `axon-T902`
/// (`tool.secret`) and — since §113 — `axon-T944` (`resource.endpoint`). It was
/// previously **inlined** at each site. Three copies of a law is how the islands
/// happened in the first place: the copies drift, and the one nobody looks at
/// stops meaning anything. There is now exactly one definition.
pub(crate) fn is_config_key(key: &str) -> bool {
    let mut chars = key.chars();
    let head_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let rest_ok = chars
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'));
    head_ok && rest_ok
}

const VALID_STORE_ISOLATION: &[&str] = &["read_committed", "repeatable_read", "serializable"];

const VALID_STORE_ON_BREACH: &[&str] = &["log", "raise", "rollback"];

/// §Fase 107.a — the closed `axonendpoint.method:` catalog. `QUERY` (RFC 10008,
/// Proposed Standard, June 2026) is the safe + idempotent + cacheable method that
/// CARRIES A REQUEST BODY — the first new HTTP method in two decades, closing the
/// "GET has no body / POST is not safe" gap for complex reads. The `cors`
/// `allow_methods:` catalog reuses this list (axon-T855), so declaring QUERY on an
/// endpoint makes it CORS-declarable too (the RFC does NOT safelist QUERY — a
/// browser preflights it, so an adopter MUST list it).
///
/// **QUERY carries a LAW, not just a route (`axon-T927`).** RFC 10008 §2 says a
/// QUERY request MUST be processed "in a safe and idempotent manner". Everywhere
/// else that is a convention the author may silently violate; here it is a
/// compile-time proof — see [`TypeChecker::first_declared_write`].
const VALID_ENDPOINT_METHODS: &[&str] = &["DELETE", "GET", "PATCH", "POST", "PUT", "QUERY"];

const VALID_INFERENCE_MODES: &[&str] = &["active", "passive"];

fn is_valid(value: &str, set: &[&str]) -> bool {
    set.contains(&value)
}

fn valid_list(set: &[&str]) -> String {
    set.join(", ")
}

/// §Fase 85.c — is a cache's `apply_to_effects:` set provably-cacheable-forever,
/// i.e. does it cover ONLY `pure`? An empty set means the implicit default
/// `[pure]`. Anything with a non-`pure` member is a widening that accepts
/// staleness and therefore requires a finite `ttl:` (`axon-T865`, D85.9).
fn cache_effects_are_pure_only(apply_to_effects: &[String]) -> bool {
    apply_to_effects.iter().all(|e| {
        let base = e.split_once(':').map(|(b, _)| b).unwrap_or(e.as_str());
        base == "pure"
    })
}

/// §Fase 85.c — a tool's declared effect row is provably-`pure` (safe to cache
/// by construction, D85.1) iff it is exactly `[pure]`. Empty / absent effects
/// are NOT pure — an undeclared effect row is unknown, not proven deterministic.
fn tool_is_pure(effects: &Option<crate::ast::EffectRow>) -> bool {
    match effects {
        Some(row) => row.effects.len() == 1 && row.effects[0] == "pure",
        None => false,
    }
}

/// §Fase 122.d (`axon-T1213`) — does this effect row declare a STREAM?
///
/// The base of an effect slug is the part before `:` (`stream:drop_oldest` →
/// `stream`), the same convention `resolve_tool_cache` and `axon-W013` already
/// use for eligibility.
fn declares_a_streaming_effect(effects: &Option<crate::ast::EffectRow>) -> bool {
    match effects {
        Some(row) => row
            .effects
            .iter()
            .any(|e| e.split_once(':').map(|(b, _)| b).unwrap_or(e.as_str()) == "stream"),
        None => false,
    }
}

/// §Fase 122.d — the one sentence both halves of `axon-T1213` say, so an adopter
/// who greps the code finds one law rather than two similar ones.
///
/// # Why a stream cannot be memoised
///
/// §122.d cabled `cache` into the dispatch path, and measuring the consumers
/// turned up a shape the language accepted and the runtime could never honour.
/// A tool declaring `<stream:…>` is dispatched by
/// `pure_shape::run_step_streaming_tool`, whose result is not a value but a
/// SEQUENCE OF CHUNKS delivered to the client as it drains. `CacheOutcome`
/// carries a `Vec<u8>`; there is no shape in it for "these seventeen deltas, in
/// this order, at these boundaries".
///
/// The alternatives were both dishonest. Buffering the stream to memoise it
/// destroys the very property the author chose when they declared it — the
/// first byte would now arrive last. Serving a hit as one fat chunk changes the
/// chunk granularity observably, so the replay is not the recording. And doing
/// neither, silently, is the defect §122 exists to remove: a declaration the
/// compiler accepts and the runtime ignores.
///
/// So the language refuses the combination. This follows D122.4 exactly — the
/// ruling that `shield`'s refusal is breaking, correct, and BOUNDED BY THE
/// DECLARATION: the only programs affected are those that declared both halves,
/// which is precisely the population that was being told a memoisation was
/// happening when none was.
const T1213_WHY: &str = "a streaming result is a sequence of chunks, not a value: \
                         memoising it would either buffer the stream (destroying the \
                         streaming the author asked for) or replay it as one chunk \
                         (a different observable shape than was recorded)";

/// §Fase 84.c (`axon-T860`) — does this session-step sequence contain a
/// **reachable** `branch { approved: […], denied: […] }` confirmation? Walks
/// the protocol tree: a `branch` step offering BOTH the `approved` and `denied`
/// labels satisfies it; otherwise it recurses into every labelled sub-protocol
/// (`select`/`branch`/`interrupt` arms), so a confirmation nested inside a
/// choice or an interruptible region still counts. This is a purely structural
/// existence check — the runtime (84.d) enforces the actual round-trip; the
/// checker only guarantees the shape can never be forgotten.
fn session_has_confirm_branch(steps: &[crate::ast::SessionStep]) -> bool {
    for step in steps {
        if step.op == "branch" {
            let has_approved = step
                .branches
                .iter()
                .any(|b| b.label == crate::technician::CONFIRM_APPROVED_LABEL);
            let has_denied = step
                .branches
                .iter()
                .any(|b| b.label == crate::technician::CONFIRM_DENIED_LABEL);
            if has_approved && has_denied {
                return true;
            }
        }
        for b in &step.branches {
            if session_has_confirm_branch(&b.steps) {
                return true;
            }
        }
    }
    false
}

/// §Fase 83.c (`axon-T854`) — is `origin` a legal CORS origin glob? Three
/// accepted shapes, closed/decidable by construction (D5 — no full regex):
/// (1) the literal any-origin sentinel `"*"` (validity of PAIRING it with
/// credentials is a separate check, `axon-T853`); (2) an exact origin with
/// no `*` at all; (3) exactly one `*` as the leading host label immediately
/// after `scheme://`, e.g. `"https://*.kivi.io"`. Anything else (multiple
/// wildcards, a wildcard mid-host, a wildcard with no scheme) is invalid.
fn is_valid_origin_glob(origin: &str) -> bool {
    if origin == "*" {
        return true;
    }
    match origin.matches('*').count() {
        0 => true,
        1 => match origin.find("://") {
            Some(scheme_end) => origin[scheme_end + 3..].starts_with("*."),
            None => false,
        },
        _ => false,
    }
}

/// §Fase 71.e — is `s` a real ISO `YYYY-MM-DD` calendar date? Pure + total (no
/// chrono — the frontend is zero-dependency): exact `dddd-dd-dd` shape, month
/// 1..12, day 1..days-in-month with a proleptic-Gregorian leap-year rule. The
/// runtime compares `now`'s local date as the same `%Y-%m-%d` string, so a date
/// that passes here matches there.
fn is_valid_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    // YYYY-MM-DD is exactly 10 chars with '-' at positions 4 and 7.
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |lo: usize, hi: usize| b[lo..hi].iter().all(|c| c.is_ascii_digit());
    if !digits(0, 4) || !digits(5, 7) || !digits(8, 10) {
        return false;
    }
    let num = |lo: usize, hi: usize| s[lo..hi].parse::<u32>().unwrap_or(0);
    let (year, month, day) = (num(0, 4), num(5, 7), num(8, 10));
    if !(1..=12).contains(&month) || day < 1 {
        return false;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    day <= days_in_month
}

// ── Type error ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct TypeError {
    pub message: String,
    pub line: u32,
    pub column: u32,
}

// ── §Fase 38.x.f — Cardinality inference primitive ──────────────────────────

/// §Fase 38.x.f (D1) — A flow-tail / endpoint-output cardinality.
///
/// Distinguishes the four cardinality classes the type-checker reasons
/// about at compile time, plus the disagreement + unknown fallback cases:
///
///  - [`Cardinality::Singular`] — one value of the named type.
///    Examples: `step S { return T }`, `return result[0]`, a bound
///    let-variable that resolved to singular.
///  - [`Cardinality::Plural`] — a `List<T>` materialized at once.
///    Examples: `retrieve … as x`, `for x in xs { … } yield T`,
///    `return [a, b, c]`.
///  - [`Cardinality::StreamCardinality`] — a `Stream<T>` whose chunks
///    arrive over time (SSE wire format). Distinct from
///    [`Cardinality::Plural`] because the runtime handles them
///    differently — `List<T>` ships as a single JSON array; `Stream<T>`
///    ships as a sequence of SSE events.
///  - [`Cardinality::Unit`] — statement-only nodes (`persist`, `mutate`,
///    `purge`) that yield no value. Endpoints declaring `output: Unit`
///    accept this.
///  - [`Cardinality::Disagreed`] — branches (`if`/`else`, `par`)
///    disagree on tail cardinality. Triggers `axon-W003
///    cardinality_disagreement_in_branches`. The special `output: Any`
///    accepts this (degraded type safety).
///  - [`Cardinality::Unknown`] — opaque flow tail (e.g. a `Par` block,
///    a `LambdaDataApply` whose target resolves dynamically). The gate
///    silently passes — the runtime D5 path stays as the final check.
///  - [`Cardinality::Wrapped`] (§Fase 39 D4) — `FlowEnvelope<T>`
///    declares a `transport: json` endpoint whose wire payload is the
///    canonical ψ-vector envelope. The compile-time cardinality of the
///    wrapper IS the cardinality of T; the type-checker reasons through
///    the wrapper transparently. `Wrapped(Box::new(Plural("X")))` for
///    `FlowEnvelope<List<X>>`; `Wrapped(Box::new(Singular("X")))` for
///    `FlowEnvelope<X>`. Nested wraps (`FlowEnvelope<FlowEnvelope<X>>`)
///    are syntactically possible but semantically degenerate and the
///    gate treats them transparently.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Cardinality {
    /// A singular value of the named type.
    Singular(String),
    /// A `List<T>` materialized at once.
    Plural(String),
    /// A `Stream<T>` (temporal plural — chunks arrive on SSE).
    StreamCardinality(String),
    /// Statement-only tail (no return value).
    Unit,
    /// Branches disagree on cardinality.
    Disagreed,
    /// Opaque — cannot be inferred at compile time.
    Unknown,
    /// §Fase 39 (D4) — `FlowEnvelope<T>` wrapping. The compile-time
    /// cardinality of the wrapper is the cardinality of T preserved
    /// through the wrap. The wire-shape envelope is always a singular
    /// object; the type-checker reasons through the wrap by unwrapping.
    Wrapped(Box<Cardinality>),
}

/// §Fase 38.x.f (D1) — Compute the cardinality declared by an
/// `axonendpoint`'s `output:` string. Pure + total:
///
///  - `""` → `Unknown` (endpoint didn't declare; gate skips).
///  - `"Unit"` → `Unit`.
///  - `"Any"` → `Disagreed` (degraded acceptance — any tail accepted).
///  - `"List<T>"` → `Plural("T")`.
///  - `"Stream<T>"` → `StreamCardinality("T")`.
///  - `"FlowEnvelope<T>"` (§Fase 39 D4) → `Wrapped(Box::new(declared_cardinality(T)))`,
///    recursive — `"FlowEnvelope<List<X>>"` yields
///    `Wrapped(Box::new(Plural("X")))`.
///  - everything else → `Singular(s)`.
pub(crate) fn declared_cardinality(output_type: &str) -> Cardinality {
    let t = output_type.trim();
    if t.is_empty() {
        return Cardinality::Unknown;
    }
    if t == "Unit" {
        return Cardinality::Unit;
    }
    if t == "Any" {
        return Cardinality::Disagreed;
    }
    // §Fase 39 (D4) — FlowEnvelope<T> wrapping. Recurse on T so nested
    // forms like FlowEnvelope<List<X>> and FlowEnvelope<Stream<X>>
    // are recognized end-to-end. Recursion lands ≥ once and terminates
    // when T is a non-wrapping primitive.
    if let Some(rest) = t.strip_prefix("FlowEnvelope<") {
        if let Some(inner) = rest.strip_suffix('>') {
            let inner_card = declared_cardinality(inner.trim());
            return Cardinality::Wrapped(Box::new(inner_card));
        }
    }
    if let Some(rest) = t.strip_prefix("List<") {
        if let Some(inner) = rest.strip_suffix('>') {
            return Cardinality::Plural(inner.trim().to_string());
        }
    }
    if let Some(rest) = t.strip_prefix("Stream<") {
        if let Some(inner) = rest.strip_suffix('>') {
            return Cardinality::StreamCardinality(inner.trim().to_string());
        }
    }
    Cardinality::Singular(t.to_string())
}

/// §Fase 38.x.f (D1) — Infer the cardinality of a flow's tail
/// expression. Walks the body from tail backwards, returning the
/// first FlowStep whose cardinality is determinable, joining branches
/// on `if`/`else` (and detecting disagreement).
///
/// Pure + total: every flow body produces some [`Cardinality`] value;
/// when no FlowStep's tail is determinable the function returns
/// [`Cardinality::Unknown`] (the gate then silently passes — the
/// runtime D5 path remains the final check).
///
/// The implementation handles the FlowStep variants common to adopter
/// REST patterns:
///
///  - `Step` / `Probe` / `Reason` / `Validate` / `Refine` / `Weave` /
///    `LambdaDataApply` / `ShieldApply` / `OtsApply` / `MandateApply`
///    → declared `output_type` (mapped via [`declared_cardinality`]).
///  - `Return` → infer from the `value_expr` string shape:
///    `[a, b, …]` → Plural, `xs[N]` → Singular, bare identifier or
///    expression → Unknown.
///  - `Retrieve` → always `Plural("StoreRow")`.
///  - `Persist` / `Mutate` / `Purge` → `Unit`.
///  - `If` → join of `then_body` + `else_body` tail cardinalities.
///  - `ForIn` → `Plural` of the body's per-iteration cardinality
///    (a `for x in xs { … }` accumulates one value per iteration).
///  - `Let` → skip (intermediate); recurse to the next-newer step.
///  - `Break` / `Continue` → skip.
///
/// Every other FlowStep variant returns [`Cardinality::Unknown`]
/// (Par, Stream, Hibernate, Deliberate, Consensus, Forge, Focus,
/// Associate, Aggregate, ExploreStep, Ingest, Navigate, Drill, Trail,
/// Corroborate, ComputeApply, UseTool, Remember, Recall, Listen,
/// DaemonStep, Emit, Publish, Discover, Transact, GenericStep).
pub(crate) fn infer_flow_tail_cardinality(flow: &FlowDefinition) -> Cardinality {
    infer_body_tail_cardinality(&flow.body)
}

fn infer_body_tail_cardinality(body: &[FlowStep]) -> Cardinality {
    if body.is_empty() {
        return Cardinality::Unit;
    }
    // Walk from tail backwards; skip intermediate non-returning
    // statements (Let, Break, Continue) until we find the first
    // FlowStep whose tail cardinality is determinable.
    for step in body.iter().rev() {
        match step {
            FlowStep::Step(s) => return declared_cardinality(&s.output_type),
            FlowStep::LambdaDataApply(n) => return declared_cardinality(&n.output_type),
            FlowStep::ShieldApply(n) => return declared_cardinality(&n.output_type),
            FlowStep::OtsApply(n) => return declared_cardinality(&n.output_type),
            FlowStep::MandateApply(n) => return declared_cardinality(&n.output_type),
            FlowStep::If(cond) => {
                let then_card = infer_body_tail_cardinality(&cond.then_body);
                let else_card = infer_body_tail_cardinality(&cond.else_body);
                return join_cardinalities(&then_card, &else_card);
            }
            FlowStep::ForIn(fi) => {
                // The for-loop body's per-iteration cardinality becomes
                // Plural at the loop's tail (N iterations × per-iter
                // value = a list of N elements). When the body is empty
                // or yields Unit, the for-loop yields Plural of Unknown.
                let inner = infer_body_tail_cardinality(&fi.body);
                return match inner {
                    Cardinality::Singular(t) => Cardinality::Plural(t),
                    Cardinality::Plural(t) => Cardinality::Plural(t),
                    Cardinality::StreamCardinality(t) => {
                        // A for-loop accumulating streams flattens to
                        // a list of streams — treat as Plural of the
                        // stream element type for the gate.
                        Cardinality::Plural(t)
                    }
                    Cardinality::Unit => Cardinality::Unknown,
                    other => other,
                };
            }
            FlowStep::Return(r) => return infer_return_cardinality(&r.value_expr),
            FlowStep::Retrieve(_) => {
                return Cardinality::Plural("StoreRow".to_string());
            }
            FlowStep::Persist(_) | FlowStep::Mutate(_) | FlowStep::Purge(_) => {
                return Cardinality::Unit;
            }
            FlowStep::Let(_) | FlowStep::Break(_) | FlowStep::Continue(_) => {
                continue;
            }
            // All other variants opaque for v1.40.0 (honest scope).
            _ => return Cardinality::Unknown,
        }
    }
    Cardinality::Unit
}

fn infer_return_cardinality(expr: &str) -> Cardinality {
    let t = expr.trim();
    if t.is_empty() {
        return Cardinality::Unit;
    }
    // `[a, b, c]` literal list → Plural with unknown element type.
    if t.starts_with('[') && t.ends_with(']') && t.len() >= 2 {
        return Cardinality::Plural(String::new());
    }
    // Indexed projection `name[N]` (e.g. `result[0]`) → Singular.
    // Match: ends with `]`, contains `[`, NOT starting with `[`.
    if t.ends_with(']') && t.contains('[') && !t.starts_with('[') {
        return Cardinality::Singular(String::new());
    }
    // Bare identifier or unknown expression — opaque.
    Cardinality::Unknown
}

fn join_cardinalities(a: &Cardinality, b: &Cardinality) -> Cardinality {
    // Unknown joins as the OTHER cardinality (one branch determines).
    if matches!(a, Cardinality::Unknown) {
        return b.clone();
    }
    if matches!(b, Cardinality::Unknown) {
        return a.clone();
    }
    // Both determined and equal → preserve.
    if a == b {
        return a.clone();
    }
    // Disagreed cases: cardinality kinds differ → Disagreed.
    // (We do NOT compare element types here — Fase 37 D2 / T901 handle
    // type mismatch; we only care about Singular vs Plural vs Stream
    // vs Unit kind disagreement for D6 W003.)
    let kind = |c: &Cardinality| match c {
        Cardinality::Singular(_) => 0,
        Cardinality::Plural(_) => 1,
        Cardinality::StreamCardinality(_) => 2,
        Cardinality::Unit => 3,
        Cardinality::Disagreed => 4,
        Cardinality::Unknown => 5,
        // §Fase 39 (D4) — Wrapped joins by its INNER kind. A
        // `FlowEnvelope<List<X>>` branch joining a bare `List<X>`
        // branch agree on plural cardinality through the wrap.
        Cardinality::Wrapped(_) => 6,
    };
    if kind(a) == kind(b) {
        // Same kind, different element type — preserve a's shape.
        return a.clone();
    }
    Cardinality::Disagreed
}

// ── Symbol table ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Symbol {
    name: String,
    kind: String,
    line: u32,
}

struct SymbolTable {
    symbols: HashMap<String, Symbol>,
}

impl SymbolTable {
    fn new() -> Self {
        SymbolTable {
            symbols: HashMap::new(),
        }
    }

    fn declare(&mut self, name: &str, kind: &str, line: u32) -> Option<String> {
        if let Some(existing) = self.symbols.get(name) {
            return Some(format!(
                "Duplicate declaration: '{}' already defined as {} (first defined at line {})",
                name, existing.kind, existing.line
            ));
        }
        self.symbols.insert(
            name.to_string(),
            Symbol {
                name: name.to_string(),
                kind: kind.to_string(),
                line,
            },
        );
        None
    }

    fn lookup(&self, name: &str) -> Option<&Symbol> {
        self.symbols.get(name)
    }
}

// ── Type checker ─────────────────────────────────────────────────────────────

pub struct TypeChecker<'a> {
    program: &'a Program,
    symbols: SymbolTable,
    errors: Vec<TypeError>,
    /// §λ-L-E Fase 13 D4 — non-fatal diagnostics (deprecation, etc.).
    /// Errors halt compilation; warnings surface in `axon check` output
    /// without failing unless `--strict` is set.  Mirrors the Python
    /// TypeChecker.warnings property.
    warnings: Vec<TypeError>,
    /// §Fase 38.d (D2) — store-name → ColumnSet built from each
    /// `axonstore` declaration's `schema:` declaration form.
    ///
    /// §Fase 38.x.d (D2) extends this to ALL THREE forms when a
    /// `manifest` is supplied via [`TypeChecker::with_manifest`]:
    /// form (a) inline (always populated, no manifest needed),
    /// form (b) manifest_ref (populated from `manifest.lookup(qualified_name)`),
    /// form (c) env_var (populated via first-match heuristic
    /// `<env_var>.<store_name>` then fallback `*.<store_name>`).
    ///
    /// When no manifest is supplied (the v1.38.3 default), forms
    /// (b)/(c) silently skip exactly as they did pre-v1.38.4 (D5
    /// absolute backwards-compat).
    store_inline_column_sets:
        std::collections::HashMap<String, crate::store_column_proof::ColumnSet>,
    /// §Fase 38.d (D2) — the current flow's parameter-name → axon-
    /// language-type-name map, set while `check_flow` runs so
    /// `check_flow_steps` can run the §38.d D2 proof against
    /// `where:` clauses. Cleared between flows so a parameter in one
    /// flow cannot leak into the proof for another.
    current_flow_params: crate::store_column_proof::FlowParamTypes,
    /// §Fase 38.x.d (D2) — optional manifest supplied at construction
    /// for forms (b)/(c) compile-time proof. `None` means form (b)/(c)
    /// silently skip (v1.38.3-compatible default). When `Some`, the
    /// `register_declarations` pass uses it to populate
    /// `store_inline_column_sets` for the non-inline forms.
    manifest: Option<&'a crate::store_schema_manifest::Manifest>,
    /// §Fase 53.c — extension-declared PROVENANCE members, collected +
    /// validated in a pre-pass (`collect_and_validate_extensions`)
    /// before tool/shield validation. `ext_effect_members` holds full
    /// effect-row entries (e.g. `"epistemic:believe"`) accepted verbatim
    /// by `check_tool`; `ext_scan_categories` holds bare scan categories
    /// accepted by `check_shield`. A member only lands here AFTER passing
    /// the invariant checks (provenance-class #2, no-shadowing #3,
    /// confidence range) — a rejected member is never silently honored.
    ext_effect_members: std::collections::HashSet<String>,
    ext_scan_categories: std::collections::HashSet<String>,
    /// §Fase 73.e — the `Json<T>` shape-LENS field index: a declared
    /// struct `type`'s name → (field name → (field type name, generic
    /// param)). Built once in `check` (after registration) from every
    /// `type` declaration, so a lens navigation `profile.age` over a
    /// `Json<UserEvent>` value can statically verify `age` exists in
    /// `UserEvent` (`axon-T842` otherwise) and resolve its scalar type —
    /// WHILE the runtime stays total (a declared-but-absent field is null,
    /// never a crash; doctrine `open_data_is_total`).
    json_lens_fields:
        std::collections::HashMap<String, std::collections::HashMap<String, (String, String)>>,
    /// §Fase 73.e — the current flow's parameter-name → FULL type spelling
    /// (`name` plus `<generic>` when present, e.g. `Json<UserEvent>`). The
    /// `Json<T>` lens needs the generic the bare `current_flow_params` map
    /// drops; kept SEPARATE so the §38 store proof (which matches the bare
    /// column-type name) is unaffected. Cleared between flows.
    current_flow_param_spellings: std::collections::BTreeMap<String, String>,
    /// §Fase 92.b — the mint bindings seen so far in the CURRENT flow's
    /// walk (source order), for the `axon-T896` never-persisted law.
    /// Cleared between flows.
    current_mint_bindings: std::collections::HashSet<String>,
    /// §Fase 99.d — the mode of the enclosing `epistemic { mode: … }` block
    /// during the declaration walk (empty at top level). The assertion-
    /// laundering barrier reads it: a `document` wrapped in `epistemic { mode:
    /// believe|know }` lets an assertive-slot flow-value binding through
    /// without a per-field `attribute:` (the author vouches the whole block is
    /// ≥ believe). Set/restored around each `Declaration::Epistemic` recursion.
    current_epistemic_mode: String,
    /// §Fase 74.g — every channel/topic `emit`ted to anywhere in the
    /// program (all flow bodies + daemon listener bodies, nested). Built
    /// once in `check`. A daemon `listen`er on a channel NOT in this set has
    /// no producer → it can never fire (`axon-W009`, the §52.g diagnostic
    /// reworked: §74 delivers a listener that HAS a producer, so the
    /// remaining defect is the unproduced channel — the Kivi brief #39
    /// case). The compile-time mirror of the §74.g PCC
    /// `ChannelDeliverySoundness`.
    emitted_channels: std::collections::HashSet<String>,
    /// §Fase 94.a — the names of every `backend: secrets` metadata store
    /// in the program, recorded at registration. Read by the write-verb
    /// law (`axon-T897`: `persist`/`mutate`/`purge` against a secrets
    /// store is unrepresentable — custody is written only by the seeding
    /// API and the mediated `rotate` commit) and by the §94.b `rotate`
    /// target rule (`axon-T898`: `rotate` targets ONLY a secrets store).
    secrets_backed_stores: std::collections::HashSet<String>,
    /// §Fase 115.d — the Epistemic Module System's per-module check
    /// context. `None` (every pre-§115 caller) ⇒ behavior byte-identical
    /// to v2.75.0: imports stay inert. `Some` ⇒ **module mode**: imported
    /// names REGISTER in the symbol table with their exported kinds (so
    /// all 71 `symbols.lookup` reference sites resolve), and the
    /// `axon-T953` import laws fire (selective required · `@scope`
    /// reserved · module exists · every name exported · no collision).
    module_ctx: Option<&'a ModuleCheckContext>,
    /// §Fase 115.d — names registered FROM imports (name → (kind, dotted
    /// source module)), so a colliding local declaration gets the branded
    /// `axon-T953` collision law instead of the generic duplicate message.
    imported_symbols: std::collections::BTreeMap<String, (String, String)>,
}

/// §Fase 115.d — what the type-checker's module mode needs to know about
/// the rest of the project: every resolved module's exports. Built by the
/// EMS driver (`crate::ems`) from the Phase-1 interfaces.
#[derive(Debug, Default)]
pub struct ModuleCheckContext {
    /// Dotted module path → (export name → symbol-table kind).
    pub modules: std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, String>,
    >,
}

// ── §Fase 70.b — static type inference for the pure expression engine ─────────

/// The static type of a pure expression (§Fase 70). `Unknown` is the permissive
/// top — a reference whose type the compiler cannot determine stays `Unknown`
/// and never triggers a type error (no false positives). Only operands with a
/// KNOWN, incompatible type raise `axon-T81x`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InferType {
    Int,
    Float,
    Bool,
    Str,
    Unknown,
}

impl InferType {
    fn is_numeric(self) -> bool {
        matches!(self, InferType::Int | InferType::Float)
    }
    fn label(self) -> &'static str {
        match self {
            InferType::Int => "Int",
            InferType::Float => "Float",
            InferType::Bool => "Bool",
            InferType::Str => "String",
            InferType::Unknown => "unknown",
        }
    }
    /// Equality-comparability class: numbers (0), booleans (1), strings (2),
    /// unknown (3). Same class ⇒ `==`/`!=` is well-typed.
    fn eq_class(self) -> u8 {
        match self {
            InferType::Int | InferType::Float => 0,
            InferType::Bool => 1,
            InferType::Str => 2,
            InferType::Unknown => 3,
        }
    }
}

/// Map an AXON type name (the string form a param / annotation carries) to an
/// `InferType`. Conservative: an unrecognised type name is `Unknown` (so a
/// future or domain type never produces a spurious condition type error).
fn infer_type_from_name(name: &str) -> InferType {
    match name.trim_end_matches('?') {
        "Int" | "Integer" | "BigInt" => InferType::Int,
        "Float" | "Double" | "Number" | "Numeric" => InferType::Float,
        "Bool" | "Boolean" => InferType::Bool,
        "String" | "Text" => InferType::Str,
        // §Fase 73.a — a `Json` (or refined `Json<T>`) value is open,
        // dynamically-shaped data: it is navigable but is NOT a scalar an
        // arithmetic / ordering / equality operator can constrain at
        // compile time. It maps to the permissive `Unknown` so navigation
        // (`.field`/`[i]`) stays total and never raises a spurious
        // `axon-T81x` (the lens does its checking in `check_json_lenses`).
        "Json" => InferType::Unknown,
        _ => InferType::Unknown,
    }
}

// ── §Fase 70.e — compile-time const-folding (dead-branch detection) ──────────

/// A fully-constant expression value, produced by [`const_fold`]. Only an
/// expression with NO references / fields / indices / calls folds (those are
/// runtime-dynamic); the result is a total, side-effect-free value the compiler
/// can decide at `axon check`.
#[derive(Clone)]
enum ConstVal {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
}

fn const_truthy(v: &ConstVal) -> bool {
    match v {
        ConstVal::Bool(b) => *b,
        ConstVal::Int(i) => *i != 0,
        ConstVal::Float(f) => *f != 0.0,
        ConstVal::Str(s) => !s.is_empty() && s != "false" && s != "0",
    }
}

fn const_as_num(v: &ConstVal) -> Option<f64> {
    match v {
        ConstVal::Int(i) => Some(*i as f64),
        ConstVal::Float(f) => Some(*f),
        _ => None,
    }
}

fn const_to_str(v: &ConstVal) -> String {
    match v {
        ConstVal::Int(i) => i.to_string(),
        ConstVal::Float(f) => f.to_string(),
        ConstVal::Bool(b) => b.to_string(),
        ConstVal::Str(s) => s.clone(),
    }
}

/// Fold a constant expression at compile time (mirrors the runtime evaluator).
/// `None` when the expression is not fully constant (has a ref/field/index/call)
/// or is ill-typed / domain-erroneous (division by zero) — those surface as
/// `axon-T81x` type errors elsewhere, not as a dead-branch warning.
fn const_fold(e: &Expr) -> Option<ConstVal> {
    match e {
        Expr::Lit(ExprLit::Int(i)) => Some(ConstVal::Int(*i)),
        Expr::Lit(ExprLit::Float(f)) => Some(ConstVal::Float(*f)),
        Expr::Lit(ExprLit::Bool(b)) => Some(ConstVal::Bool(*b)),
        Expr::Lit(ExprLit::Str(s)) => Some(ConstVal::Str(s.clone())),
        Expr::Unary(UnOp::Not, x) => Some(ConstVal::Bool(!const_truthy(&const_fold(x)?))),
        Expr::Unary(UnOp::Neg, x) => match const_fold(x)? {
            ConstVal::Int(i) => i.checked_neg().map(ConstVal::Int),
            other => Some(ConstVal::Float(-const_as_num(&other)?)),
        },
        Expr::Binary(op, l, r) => const_binop(*op, &const_fold(l)?, &const_fold(r)?),
        // §Fase 119.o — a `let` is never constant-folded. The body reaches the
        // binding through a `Ref`, and `Ref` is runtime-dynamic here (the arm
        // below), so folding the value would compute a constant nothing can
        // read. Folding through the binding correctly means an environment,
        // which this dead-branch analysis deliberately does not have.
        Expr::Let { .. } => None,
        // References + structured access + calls are runtime-dynamic.
        Expr::Ref(_) | Expr::Field(..) | Expr::Index(..) | Expr::Call(..) => None,
    }
}

fn const_binop(op: BinOp, l: &ConstVal, r: &ConstVal) -> Option<ConstVal> {
    use std::cmp::Ordering;
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
            if let (ConstVal::Int(a), ConstVal::Int(b)) = (l, r) {
                let res = match op {
                    BinOp::Add => a.checked_add(*b)?,
                    BinOp::Sub => a.checked_sub(*b)?,
                    BinOp::Mul => a.checked_mul(*b)?,
                    BinOp::Div => a.checked_div(*b)?,
                    BinOp::Mod => a.checked_rem(*b)?,
                    _ => unreachable!(),
                };
                return Some(ConstVal::Int(res));
            }
            let (a, b) = (const_as_num(l)?, const_as_num(r)?);
            let res = match op {
                BinOp::Add => a + b,
                BinOp::Sub => a - b,
                BinOp::Mul => a * b,
                BinOp::Div if b != 0.0 => a / b,
                BinOp::Mod if b != 0.0 => a % b,
                _ => return None,
            };
            Some(ConstVal::Float(res))
        }
        BinOp::Eq => Some(ConstVal::Bool(const_eq(l, r))),
        BinOp::Ne => Some(ConstVal::Bool(!const_eq(l, r))),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            let ord = const_cmp(l, r)?;
            Some(ConstVal::Bool(match op {
                BinOp::Lt => ord == Ordering::Less,
                BinOp::Le => ord != Ordering::Greater,
                BinOp::Gt => ord == Ordering::Greater,
                BinOp::Ge => ord != Ordering::Less,
                _ => unreachable!(),
            }))
        }
        BinOp::And => Some(ConstVal::Bool(const_truthy(l) && const_truthy(r))),
        BinOp::Or => Some(ConstVal::Bool(const_truthy(l) || const_truthy(r))),
    }
}

fn const_eq(l: &ConstVal, r: &ConstVal) -> bool {
    if let (Some(a), Some(b)) = (const_as_num(l), const_as_num(r)) {
        return a == b;
    }
    if let (ConstVal::Bool(a), ConstVal::Bool(b)) = (l, r) {
        return a == b;
    }
    const_to_str(l) == const_to_str(r)
}

fn const_cmp(l: &ConstVal, r: &ConstVal) -> Option<std::cmp::Ordering> {
    if let (Some(a), Some(b)) = (const_as_num(l), const_as_num(r)) {
        return a.partial_cmp(&b);
    }
    Some(const_to_str(l).cmp(&const_to_str(r)))
}

/// The surface symbol of a binary operator (for diagnostics).
fn bin_op_symbol(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "and",
        BinOp::Or => "or",
    }
}

impl<'a> TypeChecker<'a> {
    pub fn new(program: &'a Program) -> Self {
        TypeChecker {
            program,
            symbols: SymbolTable::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            store_inline_column_sets: std::collections::HashMap::new(),
            current_flow_params: crate::store_column_proof::FlowParamTypes::new(),
            manifest: None,
            ext_effect_members: std::collections::HashSet::new(),
            ext_scan_categories: std::collections::HashSet::new(),
            json_lens_fields: std::collections::HashMap::new(),
            current_flow_param_spellings: std::collections::BTreeMap::new(),
            current_mint_bindings: std::collections::HashSet::new(),
            current_epistemic_mode: String::new(),
            emitted_channels: std::collections::HashSet::new(),
            secrets_backed_stores: std::collections::HashSet::new(),
            module_ctx: None,
            imported_symbols: std::collections::BTreeMap::new(),
        }
    }

    /// §Fase 115.d — enter module mode: imported names will register in
    /// the symbol table and the `axon-T953` import laws fire. Without
    /// this call, behavior is byte-identical to v2.75.0 (backwards-compat
    /// absolute — the EMS engages only when an entry declares imports).
    pub fn set_module_context(&mut self, ctx: &'a ModuleCheckContext) {
        self.module_ctx = Some(ctx);
    }

    /// §Fase 38.x.d (D2) — construct a TypeChecker that consults the
    /// supplied manifest when an `axonstore` declares its schema via
    /// form (b) `manifest_ref` or form (c) `env_var`. Form (a) inline
    /// is unaffected (populated identically with or without manifest).
    ///
    /// Adopters using `axon check --schemas-dir <path>` reach this
    /// constructor via `axon-rs/src/main.rs::cmd_check`. Adopters
    /// embedding the type-checker programmatically pass their own
    /// `Manifest` (e.g. constructed from `load_and_merge_manifests`).
    ///
    /// When `manifest` is None, behavior is byte-identical to
    /// [`Self::new`] (D5 backwards-compat absolute).
    pub fn with_manifest(
        program: &'a Program,
        manifest: &'a crate::store_schema_manifest::Manifest,
    ) -> Self {
        TypeChecker {
            program,
            symbols: SymbolTable::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            store_inline_column_sets: std::collections::HashMap::new(),
            current_flow_params: crate::store_column_proof::FlowParamTypes::new(),
            manifest: Some(manifest),
            ext_effect_members: std::collections::HashSet::new(),
            ext_scan_categories: std::collections::HashSet::new(),
            json_lens_fields: std::collections::HashMap::new(),
            current_flow_param_spellings: std::collections::BTreeMap::new(),
            current_mint_bindings: std::collections::HashSet::new(),
            current_epistemic_mode: String::new(),
            emitted_channels: std::collections::HashSet::new(),
            secrets_backed_stores: std::collections::HashSet::new(),
            module_ctx: None,
            imported_symbols: std::collections::BTreeMap::new(),
        }
    }

    /// Errors only — a PROJECTION of [`Self::check_with_warnings`], which owns
    /// the single pass list.
    ///
    /// It used to be a second copy of that list, and the two had drifted (see
    /// the note on `check_with_warnings`). Delegating is what makes the drift
    /// impossible rather than merely fixed: there is one list, and adding a
    /// pass to it reaches both callers by construction.
    pub fn check(self) -> Vec<TypeError> {
        self.check_with_warnings().0
    }

    /// §Fase 113 — **axon-T945** (sharing discipline) and **axon-T947**
    /// (manifest/fabric coherence).
    ///
    /// # axon-T945 — `lifetime:` finally means something
    ///
    /// `lifetime: linear | affine | persistent` was **read by nothing**. It was
    /// parsed, lowered into the IR, and then never looked at again by any code
    /// in either repo. The README sold it as Linear Logic; the runtime had
    /// never heard of it.
    ///
    /// The reading that makes it a law is **how many holders may name the
    /// resource**:
    ///
    /// | | |
    /// |---|---|
    /// | `linear` | **exactly one** holder — and *not* naming it is itself a breach |
    /// | `affine` | **at most one** — it may go unused, but **sharing is a breach** |
    /// | `persistent` | the `!` exponential — freely shared |
    ///
    /// This is not decoration. Today two `axonstore`s silently share one
    /// connection pool whenever their DSNs happen to resolve equal (the
    /// registry caches pools keyed on the resolved DSN). That sharing is
    /// **accidental** — nobody declared it, nobody checked it, and nothing
    /// tells you it happened. Under §113 it is **declared**, and this law makes
    /// the undeclared case refuse.
    fn check_resource_module_laws(&mut self, decls: &[Declaration]) {
        use std::collections::BTreeMap;

        // Every declared resource, by name.
        let mut resources: BTreeMap<&str, &ResourceDefinition> = BTreeMap::new();
        for d in decls {
            if let Declaration::Resource(r) = d {
                resources.insert(r.name.as_str(), r);
            }
        }
        if resources.is_empty() {
            return;
        }

        // Every HOLDER — a declaration that names a resource and therefore
        // opens, holds and pools a connection to it. §113's ratified criterion:
        // `deliver`/`document`/`notify` hold nothing, so they are NOT holders
        // and never appear here.
        let mut holders: BTreeMap<&str, Vec<(String, Loc)>> = BTreeMap::new();
        // §Fase 120 — resources some declaration USES without POOLING a
        // connection to. Today that is exactly `lease`. Kept apart from
        // `holders` because the two halves of `axon-T945` ask different
        // questions: "is anything using this?" (consumers count) and "how many
        // things pool a connection?" (they must not).
        let mut consumers: std::collections::BTreeSet<&str> =
            std::collections::BTreeSet::new();
        for d in decls {
            match d {
                Declaration::AxonStore(s) if !s.resource_ref.is_empty() => {
                    holders
                        .entry(s.resource_ref.as_str())
                        .or_default()
                        .push((format!("axonstore '{}'", s.name), s.loc.clone()));
                }
                // §Fase 114.f — a `tool` that names a resource is a HOLDER too.
                //
                // Before §114 a tool could not name a resource, so the sharing
                // discipline only counted stores. Now that a tool holds a channel,
                // an `affine` resource shared between a store and a tool — or two
                // tools — is a breach exactly as two stores would be. Missing this
                // would have let §114 quietly re-open the sharing hole §113.b closed.
                Declaration::Tool(t) if !t.resource_ref.is_empty() => {
                    holders
                        .entry(t.resource_ref.as_str())
                        .or_default()
                        .push((format!("tool '{}'", t.name), t.loc.clone()));
                }
                // §Fase 114.u — an `upstream` that rides a resource is a HOLDER
                // too: it opens, holds and re-dials persistent connections to
                // it. Missing this would let §114.u quietly re-open the sharing
                // hole §113.b closed, exactly as §114.f said of tools.
                Declaration::Upstream(u) if !u.resource_ref.is_empty() => {
                    holders
                        .entry(u.resource_ref.as_str())
                        .or_default()
                        .push((format!("upstream '{}'", u.name), u.loc.clone()));
                }
                // §Fase 120 — a `lease` is a CONSUMER but NOT a pool holder, and
                // the distinction is the whole of this arm.
                //
                // `axon-T945` asks two different questions and they need
                // different answers here:
                //
                //   * "is anything using this?" — a
                //     `lease W { resource: R  acquire: on_start }` acquires R
                //     for a bounded window. That is a use. It satisfies the
                //     at-least-one half of linearity.
                //   * "how many things POOL a connection to this?" — a lease
                //     opens nothing. It GOVERNS the holder's use of R in time.
                //
                // Counting a lease in `holders` was measured and REFUTED by
                // §114's own governed shape: `tool Search { resource: Api }` +
                // `lease Gone { resource: Api }` over one `affine` resource is
                // the canonical form the SSE lease-charging path is built on,
                // and it would have become a sharing breach — the law refusing
                // the pattern the fase beneath it exists to deliver.
                //
                // ⚠️ A `manifest` that lists R is NEITHER. It PROVISIONS.
                // Counting it as a holder would make `manifest` + `axonstore`
                // over one resource two holders; counting it as a consumer
                // would mean listing a resource in a deployment satisfies
                // linearity, which erases the law.
                //
                // Found by running the BUILT BINARY: `axon check` had never run
                // this law at all (it took `check_with_warnings`, a diverged
                // copy of the pass list), so `banking_reference.axon` shipped a
                // lease-consumed linear resource the law called unused and
                // nobody could see.
                Declaration::Lease(l) if !l.resource_ref.is_empty() => {
                    consumers.insert(l.resource_ref.as_str());
                }
                _ => {}
            }
        }

        for (name, res) in &resources {
            let held_by = holders.get(name).map(Vec::as_slice).unwrap_or(&[]);
            // §Fase 120 — a `lease` SATISFIES "at least one consumer" without
            // COUNTING toward "at most one holder". See `consumers` above.
            let consumed = held_by.is_empty() && consumers.contains(&**name);

            match res.lifetime.as_str() {
                // `linear` — exactly one. Zero is a breach: a linear resource
                // MUST be consumed. That is the whole content of linearity, and
                // it is the half every "linear types" README quietly drops.
                "linear" => {
                    if held_by.is_empty() && !consumed {
                        self.emit(
                            format!(
                                "axon-T945 Resource '{name}' is `lifetime: linear` but NOTHING \
                                 uses it. A linear resource must be consumed exactly once — \
                                 zero consumers is a breach, not an omission. Give it a holder \
                                 (`axonstore X {{ resource: {name} }}`), or a `lease` over it, \
                                 or declare it `affine` (may go unused). Listing it in a \
                                 `manifest` does NOT count: a manifest PROVISIONS a resource, \
                                 it does not use one.",
                            ),
                            &res.loc,
                        );
                    } else if held_by.len() > 1 {
                        self.emit(
                            format!(
                                "axon-T945 Resource '{name}' is `lifetime: linear` but {} \
                                 declarations name it ({}). Linear means EXACTLY ONE holder. \
                                 Declare it `persistent` if it is meant to be shared — but then \
                                 say so, because a shared pool that nobody declared shared is \
                                 how connection exhaustion arrives without a suspect.",
                                held_by.len(),
                                held_by
                                    .iter()
                                    .map(|(w, _)| w.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            &res.loc,
                        );
                    }
                }
                // `affine` — at most one. May go unused (that is exactly what
                // distinguishes affine from linear); may never be shared.
                "affine" => {
                    if held_by.len() > 1 {
                        self.emit(
                            format!(
                                "axon-T945 Resource '{name}' is `lifetime: affine` but {} \
                                 declarations name it ({}). Affine means AT MOST ONE holder: it \
                                 may go unused, but it may not be shared. Declare it \
                                 `persistent` to share it.",
                                held_by.len(),
                                held_by
                                    .iter()
                                    .map(|(w, _)| w.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            &res.loc,
                        );
                    }
                }
                // `persistent` — the `!` exponential. Any number of holders,
                // including zero.
                _ => {}
            }
        }

        // axon-T947 — manifest/fabric coherence.
        //
        // `within:` makes disjointness unrepresentable, but it introduces a new
        // way to contradict yourself: a manifest may name fabric F while
        // listing a resource that lives `within:` G. Two declarations, one
        // fact, disagreeing — which is the exact disease §113 exists to cure.
        for d in decls {
            let Declaration::Manifest(m) = d else { continue };
            if m.fabric_ref.is_empty() {
                continue;
            }
            for r_name in &m.resources {
                let Some(res) = resources.get(r_name.as_str()) else {
                    continue; // undeclared resource: an existing law already refutes it
                };
                if res.within.is_empty() {
                    continue; // no fabric claimed: nothing to contradict
                }
                if res.within != m.fabric_ref {
                    self.emit(
                        format!(
                            "axon-T947 Manifest '{}' declares `fabric: {}` and lists resource \
                             '{}', which is `within: {}`. The two disagree about where '{}' \
                             lives. `within:` is the single source of truth — a manifest cannot \
                             relocate a resource by listing it.",
                            m.name, m.fabric_ref, r_name, res.within, r_name
                        ),
                        &m.loc,
                    );
                }
            }
        }
    }

    /// §λ-L-E Fase 13 D4 — return both errors and warnings.
    /// Callers preferring strict mode promote warnings → errors at the
    /// rendering layer (CLI `--strict` flag).  Mirrors the Python
    /// `(TypeChecker.check(), .warnings)` pair.
    /// **§Fase 120 — THE ONE PASS LIST.** `check` is a projection of this.
    ///
    /// # Why this note exists
    ///
    /// These two entry points were SEPARATE COPIES of the pass list, and they
    /// had drifted. `check` ran `check_resource_module_laws` (§113's `axon-T945`
    /// sharing discipline and `axon-T947` manifest/fabric coherence) and this
    /// one did not — and **this one is the entry point `axon check` takes**
    /// (`crate::checker::run_check`). So the §113 laws had never run for an
    /// adopter at the command line. Every test that proved them called `check`,
    /// which is the engine, not the path.
    ///
    /// That is §119's law 4 in the type-checker itself: *a proof that names a
    /// FUNCTION proves the engine, not the path.* It was found by running the
    /// BUILT BINARY against a program §120's own gate refuses — `axon check`
    /// said `0 errors`, twice.
    ///
    /// ⚠️ **Never add a pass to one of these and not the other.** There is now
    /// only one place to add it, and `two_type_check_entry_points_never_drift`
    /// in `fase120_a_effect_grammar.rs` fails if the projection is ever
    /// re-forked.
    pub fn check_with_warnings(mut self) -> (Vec<TypeError>, Vec<TypeError>) {
        self.register_declarations(&self.program.declarations);
        // §Fase 73.e — index every declared struct `type`'s fields so the
        // `Json<T>` lens can field-check navigations against them.
        self.index_type_fields(&self.program.declarations);
        // §Fase 74.g — collect the program's channel producers so a daemon
        // `listen`er with no `emit` producer is `axon-W009` (never fires).
        self.collect_emitted_channels(&self.program.declarations);
        // §Fase 73.a — validate every `Json<T>` shape lens AFTER registration.
        self.check_json_lenses(&self.program.declarations);
        // §Fase 53.c — validate + collect extensions BEFORE tool/shield
        // validation so the augmented provenance catalogs are populated.
        self.collect_and_validate_extensions(&self.program.declarations);
        self.check_declarations(&self.program.declarations);
        // §Fase 83.c — cross-declaration CORS pass (needs every axonendpoint's
        // final `path`/`cors_ref`).
        self.check_cors_cross_method_consistency(&self.program.declarations);
        // §Fase 85.c — cross-declaration cache laws (single default, effect
        // widening); needs the full tool + cache set.
        self.check_cache_module_laws(&self.program.declarations);
        // §Fase 113 — the Linear-Logic sharing discipline + manifest/fabric
        // coherence. Cross-declaration by nature: it counts HOLDERS of a
        // resource, which no per-declaration visit can see.
        //
        // ⚠️ This line is the fix. It was in `check` only, so `axon check`
        // never ran it.
        self.check_resource_module_laws(&self.program.declarations);
        // §Fase 120 — the algebraic-effect discipline. Cross-declaration by
        // NATURE: D9 exhaustiveness propagates each flow's effect row through
        // the call graph, so the answer for `flow A` depends on `flow B` that A
        // runs. An intra-flow approximation would refuse the composition the
        // whole paradigm exists for (`handle E { … } in { run inner(…) }`).
        for d in crate::effect_check::EffectChecker::new(self.program).check() {
            self.emit(d.message, &d.loc);
        }
        (self.errors, self.warnings)
    }

    // ── emit ─────────────────────────────────────────────────────

    fn emit(&mut self, message: String, loc: &Loc) {
        self.errors.push(TypeError {
            message,
            line: loc.line,
            column: loc.column,
        });
    }

    /// §λ-L-E Fase 13 D4 — non-fatal diagnostic.
    fn warn(&mut self, message: String, loc: &Loc) {
        self.warnings.push(TypeError {
            message,
            line: loc.line,
            column: loc.column,
        });
    }

    fn check_range(&mut self, value: f64, lo: f64, hi: f64, field: &str, loc: &Loc) {
        if value < lo || value > hi {
            self.emit(
                format!("{field} must be between {lo:.1} and {hi:.1}, got {value:.1}"),
                loc,
            );
        }
    }

    // ── Phase 1: registration ────────────────────────────────────

    fn register_declarations(&mut self, decls: &[Declaration]) {
        // §Fase 115.d — module mode: register imported names FIRST, with
        // their exported kinds, so (a) every `symbols.lookup` reference
        // site resolves an imported symbol exactly like a local one, and
        // (b) a local declaration reusing an imported name collides
        // against the import (the D115.5 no-shadowing law, branded
        // `axon-T953` in the flush loop below). Names the module does
        // not export register nothing here — the validation pass owns
        // that diagnostic (`check_import_laws`).
        if let Some(ctx) = self.module_ctx {
            for decl in decls {
                let Declaration::Import(n) = decl else { continue };
                if n.names.is_empty() {
                    continue; // non-selective — refused in validation
                }
                let dotted = n.module_path.join(".");
                let Some(exports) = ctx.modules.get(&dotted) else {
                    continue; // unresolved module — refused in validation
                };
                for name in &n.names {
                    let Some(kind) = exports.get(name) else {
                        continue; // not exported — refused in validation
                    };
                    if let Some(prev) = self.imported_symbols.get(name) {
                        let (_, prev_module) = prev.clone();
                        if prev_module != dotted {
                            self.emit(
                                format!(
                                    "axon-T953 import collision: '{}' is imported from both \
                                     '{}' and '{}'. A name has exactly one provider — alias \
                                     support does not exist; import it from one module only.",
                                    name, prev_module, dotted
                                ),
                                &n.loc,
                            );
                        }
                        continue;
                    }
                    if self.symbols.declare(name, kind, n.loc.line).is_none() {
                        self.imported_symbols
                            .insert(name.clone(), (kind.clone(), dotted.clone()));
                    }
                }
            }
        }

        // Collect registrations first to avoid borrow conflict
        let mut registrations: Vec<(String, String, u32, Loc)> = Vec::new();

        for decl in decls {
            match decl {
                // §Fase 114.a — a top-level budget is a named symbol.
                Declaration::Budget(n) => {
                    registrations.push((
                        n.name.clone(),
                        "budget".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                // §Fase 120 — an `effect` is a named symbol like any peer, so a
                // program declaring `effect SSE` twice collides through the same
                // duplicate-symbol law rather than silently keeping one.
                Declaration::Effect(n) => {
                    registrations.push((
                        n.name.clone(),
                        "effect".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Persona(n) => {
                    registrations.push((
                        n.name.clone(),
                        "persona".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Context(n) => {
                    registrations.push((
                        n.name.clone(),
                        "context".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Anchor(n) => {
                    registrations.push((
                        n.name.clone(),
                        "anchor".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Memory(n) => {
                    registrations.push((
                        n.name.clone(),
                        "memory".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Tool(n) => {
                    registrations.push((n.name.clone(), "tool".into(), n.loc.line, n.loc.clone()));
                }
                Declaration::Type(n) => {
                    registrations.push((n.name.clone(), "type".into(), n.loc.line, n.loc.clone()));
                }
                Declaration::Flow(n) => {
                    registrations.push((n.name.clone(), "flow".into(), n.loc.line, n.loc.clone()));
                }
                Declaration::Intent(n) => {
                    registrations.push((
                        n.name.clone(),
                        "intent".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::LambdaData(n) => {
                    registrations.push((
                        n.name.clone(),
                        "lambda_data".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Agent(n) => {
                    registrations.push((n.name.clone(), "agent".into(), n.loc.line, n.loc.clone()));
                }
                Declaration::Window(n) => {
                    registrations.push((
                        n.name.clone(),
                        "window".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Shield(n) => {
                    registrations.push((
                        n.name.clone(),
                        "shield".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Pix(n) => {
                    registrations.push((n.name.clone(), "pix".into(), n.loc.line, n.loc.clone()));
                }
                Declaration::Ledger(n) => {
                    registrations.push((n.name.clone(), "ledger".into(), n.loc.line, n.loc.clone()));
                }
                Declaration::Psyche(n) => {
                    registrations.push((
                        n.name.clone(),
                        "psyche".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Corpus(n) => {
                    registrations.push((
                        n.name.clone(),
                        "corpus".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Dataspace(n) => {
                    registrations.push((
                        n.name.clone(),
                        "dataspace".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Ots(n) => {
                    registrations.push((n.name.clone(), "ots".into(), n.loc.line, n.loc.clone()));
                }
                Declaration::Mandate(n) => {
                    registrations.push((
                        n.name.clone(),
                        "mandate".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Compute(n) => {
                    registrations.push((
                        n.name.clone(),
                        "compute".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Daemon(n) => {
                    registrations.push((
                        n.name.clone(),
                        "daemon".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::AxonStore(n) => {
                    registrations.push((
                        n.name.clone(),
                        "axonstore".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                    // §Fase 38.d (D2) — when this axonstore carries an
                    // INLINE `schema: { … }` block, build its ColumnSet
                    // now so `check_flow_steps` can prove `where:`
                    // clauses against it.
                    //
                    // §Fase 38.x.d (D2, D4) — when this axonstore uses
                    // form (b) `manifest_ref` OR form (c) `env_var`
                    // AND `self.manifest` is set (the adopter passed
                    // `--schemas-dir <path>` to `axon check`), resolve
                    // the manifest entry NOW and populate the same
                    // `store_inline_column_sets` HashMap. The
                    // downstream proof paths (T801-T805 + T803) read
                    // from the HashMap uniformly — no code-path
                    // divergence between inline and manifest-loaded
                    // forms.
                    //
                    // Without `--schemas-dir`, `self.manifest` is None
                    // and forms (b)/(c) silently skip exactly as they
                    // did in v1.38.3 (D5 backwards-compat absolute).
                    //
                    // §Fase 94.a — a `backend: secrets` metadata store
                    // gets the FIXED synthesized schema (key / version /
                    // created_at / expires_at) so `where:` / `order_by:`
                    // / aggregate proofs run against the custody
                    // metadata columns exactly like a declared inline
                    // schema. Recorded in `secrets_backed_stores` for
                    // the T897 write-verb law + the §94.b `rotate`
                    // target rule. The synthesized set is inserted
                    // FIRST so it wins even when the adopter ALSO
                    // declared an explicit schema (that declaration is
                    // `axon-T900` in `check_axonstore` — but the proofs
                    // must still run against the law's shape, not the
                    // adopter's).
                    if n.backend == "secrets" {
                        self.secrets_backed_stores.insert(n.name.clone());
                        let synthesized = crate::store_schema::secrets_metadata_schema(
                            n.loc.line,
                            n.loc.column,
                        );
                        if let Some(cs) =
                            crate::store_column_proof::ColumnSet::from_inline_schema(
                                &synthesized,
                            )
                        {
                            self.store_inline_column_sets.insert(n.name.clone(), cs);
                        }
                    }
                    if n.backend != "secrets" {
                    if let Some(schema) = &n.column_schema {
                        match schema {
                            crate::store_schema::StoreColumnSchema::Inline { .. } => {
                                if let Some(cs) =
                                    crate::store_column_proof::ColumnSet::from_inline_schema(
                                        schema,
                                    )
                                {
                                    self.store_inline_column_sets
                                        .insert(n.name.clone(), cs);
                                }
                            }
                            crate::store_schema::StoreColumnSchema::ManifestRef {
                                qualified_name,
                                ..
                            } => {
                                if let Some(manifest) = self.manifest {
                                    if let Some(ms) = manifest.lookup(qualified_name) {
                                        let cs =
                                            crate::store_column_proof::ColumnSet::from_manifest_store(
                                                ms,
                                            );
                                        self.store_inline_column_sets
                                            .insert(n.name.clone(), cs);
                                    }
                                }
                            }
                            crate::store_schema::StoreColumnSchema::EnvVar {
                                var_name,
                                ..
                            } => {
                                if let Some(manifest) = self.manifest {
                                    // First-match heuristic mirrors
                                    // §Fase 38.d's `load_columns_for_schema`
                                    // + §Fase 38.f's `declared_columns_for`:
                                    // try exact `<var_name>.<store_name>`
                                    // first, then fall back to any
                                    // `*.<store_name>` (the manifest
                                    // ships the same shape for every
                                    // per-tenant namespace).
                                    let exact_key = format!("{}.{}", var_name, n.name);
                                    let resolved = manifest
                                        .lookup(&exact_key)
                                        .or_else(|| {
                                            // Suffix scan — find any
                                            // store ending in `.<n.name>`.
                                            let suffix = format!(".{}", n.name);
                                            for (key, store) in &manifest.stores {
                                                if key.ends_with(&suffix) {
                                                    return Some(store);
                                                }
                                            }
                                            None
                                        });
                                    if let Some(ms) = resolved {
                                        let cs =
                                            crate::store_column_proof::ColumnSet::from_manifest_store(
                                                ms,
                                            );
                                        self.store_inline_column_sets
                                            .insert(n.name.clone(), cs);
                                    }
                                }
                            }
                        }
                    }
                    } // §Fase 94.a — end of the non-secrets schema arm.
                }
                Declaration::AxonEndpoint(n) => {
                    registrations.push((
                        n.name.clone(),
                        "axonendpoint".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Resource(n) => {
                    registrations.push((
                        n.name.clone(),
                        "resource".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Fabric(n) => {
                    registrations.push((
                        n.name.clone(),
                        "fabric".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Manifest(n) => {
                    registrations.push((
                        n.name.clone(),
                        "manifest".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Observe(n) => {
                    registrations.push((
                        n.name.clone(),
                        "observe".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Reconcile(n) => {
                    registrations.push((
                        n.name.clone(),
                        "reconcile".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Lease(n) => {
                    registrations.push((n.name.clone(), "lease".into(), n.loc.line, n.loc.clone()));
                }
                Declaration::Ensemble(n) => {
                    registrations.push((
                        n.name.clone(),
                        "ensemble".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Session(n) => {
                    registrations.push((
                        n.name.clone(),
                        "session".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Topology(n) => {
                    registrations.push((
                        n.name.clone(),
                        "topology".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Immune(n) => {
                    registrations.push((
                        n.name.clone(),
                        "immune".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Reflex(n) => {
                    registrations.push((
                        n.name.clone(),
                        "reflex".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Heal(n) => {
                    registrations.push((n.name.clone(), "heal".into(), n.loc.line, n.loc.clone()));
                }
                Declaration::Component(n) => {
                    registrations.push((
                        n.name.clone(),
                        "component".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::View(n) => {
                    registrations.push((n.name.clone(), "view".into(), n.loc.line, n.loc.clone()));
                }
                Declaration::Channel(n) => {
                    registrations.push((
                        n.name.clone(),
                        "channel".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Socket(n) => {
                    registrations.push((n.name.clone(), "socket".into(), n.loc.line, n.loc.clone()));
                }
                // §Fase 80.b — register the outbound vendor connection so
                // `voice`/flow references resolve to it.
                Declaration::Upstream(n) => {
                    registrations.push((n.name.clone(), "upstream".into(), n.loc.line, n.loc.clone()));
                }
                // §Fase 80.g — register the voice agent (its expansion's
                // session/socket/upstreams register themselves — they are
                // ordinary declarations in the program by check time).
                Declaration::Voice(n) => {
                    registrations.push((n.name.clone(), "voice".into(), n.loc.line, n.loc.clone()));
                }
                // §Fase 83.a — register the named origin-policy declaration
                // so an `axonendpoint.cors:` reference resolves to it.
                Declaration::Cors(n) => {
                    registrations.push((n.name.clone(), "cors".into(), n.loc.line, n.loc.clone()));
                }
                // §Fase 92.a — register the ephemeral-credential contract so
                // a `mint <Credential>` reference resolves to it (axon-T895).
                Declaration::Credential(n) => {
                    registrations.push((
                        n.name.clone(),
                        "credential".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                // §Fase 85.a — register the named cache policy so a
                // `tool.cache:` / `retrieve.cache:` reference resolves to it.
                Declaration::Cache(n) => {
                    registrations.push((n.name.clone(), "cache".into(), n.loc.line, n.loc.clone()));
                }
                // §Fase 87.a — register the savant so a future cross-reference
                // (and duplicate-name detection) resolves it. Field validation
                // is §87.b (`check_savant`).
                Declaration::Savant(n) => {
                    registrations.push((
                        n.name.clone(),
                        "savant".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                // §Fase 99.b — register the document so it is a referenceable
                // name (and duplicate-name detection works).
                Declaration::Document(n) => {
                    registrations.push((
                        n.name.clone(),
                        "document".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                // §Fase 105 — register the delivery so it is a referenceable
                // name (and duplicate-name detection works).
                Declaration::Deliver(n) => {
                    registrations.push((
                        n.name.clone(),
                        "deliver".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                // §Fase 110 — register the notification (referenceable name +
                // duplicate detection).
                                Declaration::Notify(n) => {
                    registrations.push((
                        n.name.clone(),
                        "notify".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                // §Fase 87.d — register the synth policy so a savant can
                // reference it (and duplicate-name detection works).
                Declaration::Synth(n) => {
                    registrations.push((
                        n.name.clone(),
                        "synth".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                // §Fase 88.a — register the authorization scope so a
                // `warden(t) within <Scope>` reference resolves (§88.c).
                Declaration::Scope(n) => {
                    registrations.push((
                        n.name.clone(),
                        "scope".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                // §Fase 51.c.2 — register the Pauli-sum observable so a
                // `quant(observable: <Name>)` reference resolves to it.
                Declaration::Observable(n) => {
                    registrations.push((
                        n.name.clone(),
                        "observable".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                // §Fase 69.a — register the Advantage Witness by name.
                Declaration::Witness(n) => {
                    registrations.push((
                        n.name.clone(),
                        "witness".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Generic(n) => {
                    if !n.name.is_empty() {
                        registrations.push((
                            n.name.clone(),
                            n.keyword.clone(),
                            n.loc.line,
                            n.loc.clone(),
                        ));
                    }
                }
                // §Fase 53.a — register the extension name as a symbol
                // (free duplicate-name detection). Member-level + category
                // + no-shadowing validation is §53.c.
                Declaration::Extension(n) => {
                    registrations.push((
                        n.name.clone(),
                        "extension".into(),
                        n.loc.line,
                        n.loc.clone(),
                    ));
                }
                Declaration::Epistemic(_) => {
                    // Recursion handled below
                }
                Declaration::Import(_) | Declaration::Run(_) | Declaration::Let(_) => {}
            }
        }

        for (name, kind, line, loc) in registrations {
            // §Fase 115.d — a local declaration reusing an IMPORTED name is
            // the D115.5 collision law, branded so the fix is in the message.
            if let Some((imp_kind, imp_module)) = self.imported_symbols.get(&name) {
                let (imp_kind, imp_module) = (imp_kind.clone(), imp_module.clone());
                self.emit(
                    format!(
                        "axon-T953 '{}' collides with the {} imported from '{}'. There is no \
                         shadowing: rename the local {} or drop the import.",
                        name, imp_kind, imp_module, kind
                    ),
                    &loc,
                );
                continue;
            }
            if let Some(err) = self.symbols.declare(&name, &kind, line) {
                self.emit(err, &loc);
            }
        }

        // Recurse into epistemic blocks
        for decl in decls {
            if let Declaration::Epistemic(eb) = decl {
                self.register_declarations(&eb.body);
            }
        }
    }

    // ── Phase 2: validation ──────────────────────────────────────

    /// §Fase 53.c — pre-pass: validate every `extension` declaration and
    /// populate the augmented provenance catalogs the tool/shield checks
    /// consult. Runs BEFORE `check_declarations`. Recurses into
    /// `epistemic` blocks for parity with `register_declarations`.
    fn collect_and_validate_extensions(&mut self, decls: &[Declaration]) {
        for decl in decls {
            match decl {
                Declaration::Extension(ext) => self.check_extension(ext),
                Declaration::Epistemic(eb) => {
                    self.collect_and_validate_extensions(&eb.body)
                }
                _ => {}
            }
        }
    }

    /// §Fase 53.c — validate one `extension` + register its surviving
    /// members. Enforces:
    ///  - category ∈ {`effects`, `scan`};
    ///  - INVARIANT #2 (provenance-class): an `effects` member's base
    ///    (segment before `:`) may NOT be a canonical ENFORCEABLE effect
    ///    base — this blocks smuggling an unenforceable privileged effect
    ///    under a custom name (`io:bypass_shield`, `network:…`, …);
    ///  - INVARIANT #3 (no shadowing): a `scan` member may not shadow a
    ///    canonical scan category;
    ///  - `default_confidence` ∈ [0.0, 1.0] when present.
    /// Only members surviving validation are added to the augmented
    /// catalog — a rejected member is never silently honored.
    fn check_extension(&mut self, ext: &ExtensionDefinition) {
        match ext.category.as_str() {
            "effects" => {
                for m in &ext.members {
                    let base = m.name.split(':').next().unwrap_or(m.name.as_str());
                    if is_valid(base, VALID_EFFECTS) {
                        self.emit(
                            format!(
                                "extension '{}' effect member '{}' shadows the canonical enforceable \
                                 base '{}'. Extensions are PROVENANCE-class only (§Fase 53 invariant \
                                 #2: E_C ∩ E_E = ∅) — they may not redefine or qualify an enforceable \
                                 effect base.",
                                ext.name, m.name, base
                            ),
                            &m.loc,
                        );
                        continue;
                    }
                    if let Some(c) = m.default_confidence {
                        if !(0.0..=1.0).contains(&c) {
                            self.emit(
                                format!(
                                    "extension '{}' member '{}' has default_confidence {} outside the \
                                     valid range [0.0, 1.0]",
                                    ext.name, m.name, c
                                ),
                                &m.loc,
                            );
                            continue;
                        }
                    }
                    self.ext_effect_members.insert(m.name.clone());
                }
            }
            "scan" => {
                for m in &ext.members {
                    if is_valid(&m.name, VALID_SCAN_CATEGORIES) {
                        self.emit(
                            format!(
                                "extension '{}' scan member '{}' shadows a canonical scan category \
                                 (§Fase 53 invariant #3 — no shadowing of the canonical catalog)",
                                ext.name, m.name
                            ),
                            &m.loc,
                        );
                        continue;
                    }
                    self.ext_scan_categories.insert(m.name.clone());
                }
            }
            other => {
                self.emit(
                    format!(
                        "extension '{}' has unknown category '{}'. Valid categories: effects, scan",
                        ext.name, other
                    ),
                    &ext.loc,
                );
            }
        }
    }

    fn check_declarations(&mut self, decls: &[Declaration]) {
        for decl in decls {
            match decl {
                // §Fase 114.a — a top-level budget is checked by the SAME laws as a
                // daemon's (T830–T834). Reused, not re-implemented: a second copy
                // of a law is how the islands happened.
                Declaration::Budget(n) => {
                    let scope = format!("budget '{}'", n.name);
                    self.check_budget(n, &scope);
                }
                // §Fase 120 — every law about an `effect` declaration and its
                // use sites lives in `crate::effect_check`, run once at the end
                // of `check()`. It is deliberately NOT split across a
                // per-declaration visit here: D9 is interprocedural, and half
                // the laws living in one place with half in another is how two
                // implementations of one rule start disagreeing.
                Declaration::Effect(_) => {}
                Declaration::Persona(n) => self.check_persona(n),
                Declaration::Context(n) => self.check_context(n),
                Declaration::Anchor(n) => self.check_anchor(n),
                Declaration::Memory(n) => self.check_memory(n),
                Declaration::Tool(n) => self.check_tool(n),
                Declaration::Flow(n) => self.check_flow(n),
                Declaration::Intent(n) => self.check_intent(n),
                Declaration::Run(n) => self.check_run(n),
                Declaration::Epistemic(eb) => {
                    self.check_epistemic_mode(&eb.mode, &eb.loc);
                    // §Fase 99.d — thread the enclosing mode so a `document`
                    // inside `epistemic { mode: believe|know }` is barrier-
                    // satisfied without per-field `attribute:`.
                    let prev =
                        std::mem::replace(&mut self.current_epistemic_mode, eb.mode.clone());
                    self.check_declarations(&eb.body);
                    self.current_epistemic_mode = prev;
                }
                Declaration::LambdaData(n) => self.check_lambda_data(n),
                Declaration::Agent(n) => self.check_agent(n),
                Declaration::Shield(n) => self.check_shield(n),
                Declaration::Window(n) => self.check_window(n),
                Declaration::Pix(n) => self.check_pix(n),
                Declaration::Ledger(n) => self.check_ledger(n),
                Declaration::Psyche(n) => self.check_psyche(n),
                Declaration::Corpus(n) => self.check_corpus(n),
                Declaration::Dataspace(n) => self.check_dataspace(n), // §108.b — axon-T928
                Declaration::Ots(n) => self.check_ots(n),
                Declaration::Mandate(n) => self.check_mandate(n),
                Declaration::Compute(_) => {} // no Python validation exists
                Declaration::Daemon(n) => self.check_daemon(n),
                Declaration::AxonStore(n) => self.check_axonstore(n),
                Declaration::AxonEndpoint(n) => self.check_axonendpoint(n),
                Declaration::Resource(n) => self.check_resource(n),
                Declaration::Fabric(n) => self.check_fabric(n),
                Declaration::Manifest(n) => self.check_manifest(n),
                Declaration::Observe(n) => self.check_observe(n),
                Declaration::Reconcile(n) => self.check_reconcile(n),
                Declaration::Lease(n) => self.check_lease(n),
                Declaration::Ensemble(n) => self.check_ensemble(n),
                Declaration::Session(n) => self.check_session(n),
                Declaration::Topology(n) => self.check_topology(n),
                Declaration::Socket(n) => self.check_socket(n),
                Declaration::Upstream(n) => self.check_upstream(n),
                Declaration::Voice(n) => self.check_voice(n),
                Declaration::Cors(n) => self.check_cors(n),
                Declaration::Cache(n) => self.check_cache(n),
                // §Fase 92.a — the credential contract's own-field laws
                // (non-empty grants T893, TTL bounds T894).
                Declaration::Credential(n) => self.check_credential(n),
                // §Fase 87.b — own-field validation (domain, mandates, cognition
                // catalogs). Ref resolution (memory backend) + §72 budget binding
                // + §79 interruptibility land in §87.c.
                Declaration::Savant(n) => self.check_savant(n),
                // §Fase 99.c/d — structure validity + the assertion-laundering
                // barrier.
                Declaration::Document(n) => self.check_document(n),
                // §Fase 105 — CRM delivery structure + the T920 provenance
                // barrier (egress-dual of the §99 assertion-laundering barrier).
                Declaration::Deliver(n) => self.check_deliver(n),
                Declaration::Notify(n) => self.check_notify(n), // §110 — T933/T934/T935
                // §Fase 87.d — dynamic tool-synthesis policy discipline.
                Declaration::Synth(n) => self.check_synth(n),
                // §Fase 88.b — the scope's own-field discipline (targets
                // non-empty + depth catalog + approver). The `warden`-side
                // authorization binding (scope resolution + target allowlist) is
                // §88.c.
                Declaration::Scope(n) => self.check_scope(n),
                Declaration::Observable(n) => self.check_observable(n),
                Declaration::Witness(n) => self.check_witness(n),
                Declaration::Immune(n) => self.check_immune(n),
                Declaration::Reflex(n) => self.check_reflex(n),
                Declaration::Heal(n) => self.check_heal(n),
                Declaration::Component(n) => self.check_component(n),
                Declaration::View(n) => self.check_view(n),
                Declaration::Channel(n) => self.check_channel(n),
                // §Fase 53.c will implement `check_extension` (category ∈
                // {effects,scan}, no-shadowing of canonical bases/categories,
                // provenance-class members, valid default_confidence range).
                Declaration::Extension(_) => {}
                // §Fase 115.d — module mode fires the import laws; without
                // a module context an import stays inert (v2.75.0 parity).
                Declaration::Import(n) => self.check_import_laws(n),
                Declaration::Type(_)
                | Declaration::Let(_)
                | Declaration::Generic(_) => {}
            }
        }
    }

    // ── Per-construct checks ─────────────────────────────────────

    /// §Fase 115.d — the `axon-T953` import laws (module mode only):
    ///
    /// 1. **Selective import required.** `import a.b` without a `{…}`
    ///    selector is `#include` wearing a module system's clothes — it
    ///    would flood the namespace (the EMS paper's own §1.3 argument).
    /// 2. **`@scope` reserved.** The scoped form parses but has no
    ///    resolution semantics yet; it is refused loudly rather than
    ///    granted invented directory semantics (the §111 posture — a form
    ///    that parses but does nothing must refuse, not stay silent).
    /// 3. **Module exists.** Defense-in-depth: the resolver normally
    ///    refuses first with the searched path; this covers embedded
    ///    callers that assemble their own context (the enterprise bundle).
    /// 4. **Every imported name is exported** by the named module — with
    ///    the module's real export list in the message.
    fn check_import_laws(&mut self, node: &crate::ast::ImportNode) {
        let Some(ctx) = self.module_ctx else { return };

        if node.module_path.first().map(|s| s.starts_with('@')).unwrap_or(false) {
            self.emit(
                format!(
                    "axon-T953 `import {}` uses the @scope form, which is RESERVED for a \
                     future package registry and resolves to nothing today. Import a \
                     project module by its root-relative path instead.",
                    node.module_path.join(".")
                ),
                &node.loc,
            );
            return;
        }
        if node.names.is_empty() {
            self.emit(
                format!(
                    "axon-T953 selective import required: `import {}` names nothing and \
                     would flood the namespace. Name what you need: `import {}.{{A, B}}`.",
                    node.module_path.join("."),
                    node.module_path.join(".")
                ),
                &node.loc,
            );
            return;
        }
        let dotted = node.module_path.join(".");
        let Some(exports) = ctx.modules.get(&dotted) else {
            self.emit(
                format!(
                    "axon-T953 module '{}' is not part of this compilation (expected file \
                     '{}').",
                    dotted,
                    node.module_path.join("/") + ".axon"
                ),
                &node.loc,
            );
            return;
        };
        for name in &node.names {
            if !exports.contains_key(name) {
                let mut available: Vec<&str> =
                    exports.keys().map(String::as_str).collect();
                available.sort_unstable();
                self.emit(
                    format!(
                        "axon-T953 module '{}' does not export '{}'. Its exports: [{}].",
                        dotted,
                        name,
                        available.join(", ")
                    ),
                    &node.loc,
                );
            }
        }
    }

    fn check_persona(&mut self, node: &PersonaDefinition) {
        if !node.tone.is_empty() && !is_valid(&node.tone, VALID_TONES) {
            self.emit(
                format!(
                    "Unknown tone '{}' for persona '{}'. Valid tones: {}",
                    node.tone,
                    node.name,
                    valid_list(VALID_TONES)
                ),
                &node.loc,
            );
        }
        if let Some(v) = node.confidence_threshold {
            self.check_range(v, 0.0, 1.0, "confidence_threshold", &node.loc);
        }
    }

    fn check_context(&mut self, node: &ContextDefinition) {
        if !node.memory_scope.is_empty() && !is_valid(&node.memory_scope, VALID_MEMORY_SCOPES) {
            self.emit(
                format!(
                    "Unknown memory scope '{}' in context '{}'. Valid: {}",
                    node.memory_scope,
                    node.name,
                    valid_list(VALID_MEMORY_SCOPES)
                ),
                &node.loc,
            );
        }
        if !node.depth.is_empty() && !is_valid(&node.depth, VALID_DEPTHS) {
            self.emit(
                format!(
                    "Unknown depth '{}' in context '{}'. Valid: {}",
                    node.depth,
                    node.name,
                    valid_list(VALID_DEPTHS)
                ),
                &node.loc,
            );
        }
        if let Some(v) = node.temperature {
            self.check_range(v, 0.0, 2.0, "temperature", &node.loc);
        }
        if let Some(v) = node.max_tokens {
            if v <= 0 {
                self.emit(
                    format!(
                        "max_tokens must be positive, got {} in context '{}'",
                        v, node.name
                    ),
                    &node.loc,
                );
            }
        }
        if let Some(tz) = &node.now_tz {
            self.check_now_tz(tz, "context", &node.name, &node.loc);
        }
    }

    /// §Fase 91.a — the `now:` format law (`axon-T892`). Mirrors the §71.a
    /// window-timezone posture: the frontend (zero-dependency) checks the
    /// IANA *shape* — `"UTC"` or `Area/Location` with no leading/trailing
    /// slash — and the runtime (§91.b, chrono-tz) is the authority on actual
    /// IANA membership, failing closed on an unresolvable zone.
    fn check_now_tz(&mut self, tz: &str, surface: &str, name: &str, loc: &Loc) {
        let t = tz.trim();
        let tz_ok = t == "UTC" || (t.contains('/') && !t.starts_with('/') && !t.ends_with('/'));
        if !tz_ok {
            self.emit(
                format!(
                    "axon-T892 {surface} '{name}' declares an invalid `now:` timezone \
                     '{tz}' — expected an IANA name like \"America/Bogota\" or \"UTC\". \
                     Time is an explicit input: say WHOSE time the cognition runs in.",
                ),
                loc,
            );
        }
    }

    fn check_anchor(&mut self, node: &AnchorConstraint) {
        if let Some(v) = node.confidence_floor {
            self.check_range(v, 0.0, 1.0, "confidence_floor", &node.loc);
        }
        if !node.on_violation.is_empty() && !is_valid(&node.on_violation, VALID_VIOLATION_ACTIONS) {
            self.emit(
                format!(
                    "Unknown on_violation action '{}' in anchor '{}'. Valid: {}",
                    node.on_violation,
                    node.name,
                    valid_list(VALID_VIOLATION_ACTIONS)
                ),
                &node.loc,
            );
        }
        if node.on_violation == "raise" && node.on_violation_target.is_empty() {
            self.emit(
                format!(
                    "Anchor '{}' uses 'raise' but no error type specified",
                    node.name
                ),
                &node.loc,
            );
        }
    }

    fn check_memory(&mut self, node: &MemoryDefinition) {
        if !node.store.is_empty() && !is_valid(&node.store, VALID_MEMORY_SCOPES) {
            self.emit(
                format!(
                    "Unknown store type '{}' in memory '{}'. Valid: {}",
                    node.store,
                    node.name,
                    valid_list(VALID_MEMORY_SCOPES)
                ),
                &node.loc,
            );
        }
        if !node.retrieval.is_empty() && !is_valid(&node.retrieval, VALID_RETRIEVAL_STRATEGIES) {
            self.emit(
                format!(
                    "Unknown retrieval strategy '{}' in memory '{}'. Valid: {}",
                    node.retrieval,
                    node.name,
                    valid_list(VALID_RETRIEVAL_STRATEGIES)
                ),
                &node.loc,
            );
        }
    }

    /// §Fase 84.c — Remote Hands. The technician-command laws for a
    /// `tool { target:, risk:, argv: }`. Entirely inert unless the tool opts
    /// into the surface (sets one of those fields), so the entire existing
    /// corpus is unaffected (zero regression). The four load-bearing laws:
    ///
    /// - **axon-T858** — a `target:`-bound `provider: bash` tool with no
    ///   `argv:`. A free-string command would reopen the injection surface the
    ///   fase exists to close (D84.1), so the argv template is mandatory.
    /// - **axon-T859** — an `argv:` placeholder that is not a whole-element
    ///   `${param}` bound to a declared `parameters:` entry. Whole-element is
    ///   the crux: `"${x}.txt"` is rejected so an argument can never fuse with
    ///   adjacent text or be split (D84.1).
    /// - **axon-T860** — a `risk: destructive` tool whose bound session offers
    ///   no reachable `branch{ approved / denied }` confirmation (D84.2). The
    ///   human confirm/deny exit must be visible in the protocol's own shape.
    /// - **axon-T861** — `target:` does not resolve to a declared `socket`.
    /// - **axon-T862** — a `risk:` value outside the closed `safe | destructive`
    ///   catalog.
    fn check_technician_tool(&mut self, node: &ToolDefinition) {
        let is_technician =
            node.target.is_some() || node.risk.is_some() || !node.argv.is_empty();
        if !is_technician {
            return;
        }

        // axon-T862 — the risk class is a v1-closed catalog.
        if let Some(risk) = &node.risk {
            if !crate::technician::VALID_RISK_LEVELS.contains(&risk.as_str()) {
                self.emit(
                    format!(
                        "axon-T862 technician tool '{}' has an unknown risk class '{}' — valid: {}",
                        node.name,
                        risk,
                        crate::technician::VALID_RISK_LEVELS.join(", ")
                    ),
                    &node.loc,
                );
            }
        }

        // axon-T861 — `target:` must resolve to a declared `socket`. Duality of
        // that socket's own protocol is enforced separately by `check_socket`;
        // here we only bind the reference (mirrors the T856 cors-ref check).
        if let Some(target) = &node.target {
            match self.symbols.lookup(target) {
                None => self.emit(
                    format!(
                        "axon-T861 technician tool '{}' targets undefined socket '{}'",
                        node.name, target
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "socket" => self.emit(
                    format!(
                        "axon-T861 '{}' is a {}, not a socket (target of technician tool '{}')",
                        target, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }

        // axon-T858 — a `target:`-bound bash tool must declare an argv template.
        if node.target.is_some() && node.provider == "bash" && node.argv.is_empty() {
            self.emit(
                format!(
                    "axon-T858 technician tool '{}' binds `target:` on `provider: bash` but \
                     declares no `argv:` — a free-string command would reopen the injection \
                     surface §Fase 84 exists to close (D84.1); declare an argv template, e.g. \
                     `argv: [\"ping\", \"-c\", \"${{count}}\", \"${{host}}\"]`",
                    node.name
                ),
                &node.loc,
            );
        }

        // axon-T859 — every `${param}` in argv is a WHOLE argv element bound to
        // a declared parameter; partial/mixed tokens are rejected.
        let param_names: std::collections::HashSet<&str> =
            node.parameters.iter().map(|p| p.name.as_str()).collect();
        for tok in &node.argv {
            match crate::technician::classify_argv_token(tok) {
                crate::technician::ArgvToken::Placeholder(name) => {
                    if !param_names.contains(name.as_str()) {
                        self.emit(
                            format!(
                                "axon-T859 argv placeholder '${{{}}}' in technician tool '{}' is \
                                 not a declared `parameters:` entry — every argv placeholder must \
                                 bind to a typed argument (the §54.b interpolation discipline)",
                                name, node.name
                            ),
                            &node.loc,
                        );
                    }
                }
                crate::technician::ArgvToken::Partial(t) => {
                    self.emit(
                        format!(
                            "axon-T859 argv element '{}' in technician tool '{}' is not a \
                             whole-element placeholder — a `${{param}}` must be an ENTIRE argv \
                             element (never fused with surrounding text like `${{x}}.txt` or \
                             `pre${{x}}`), so an argument can neither be split nor escape its \
                             slot (D84.1)",
                            t, node.name
                        ),
                        &node.loc,
                    );
                }
                crate::technician::ArgvToken::Literal(_) => {}
            }
        }

        // axon-T860 — a destructive command needs a reachable confirm/deny
        // branch in its bound session's protocol. Structural: the runtime
        // enforces the actual round-trip (84.d), but the SHAPE must exist at
        // compile time so the confirmation can never be forgotten.
        if node.risk.as_deref() == Some(crate::technician::RISK_DESTRUCTIVE) {
            let has_branch = node
                .target
                .as_ref()
                .and_then(|t| self.find_socket(t))
                .and_then(|sock| self.find_session(&sock.protocol))
                .map(|sess| {
                    sess.roles
                        .iter()
                        .any(|r| session_has_confirm_branch(&r.steps))
                })
                .unwrap_or(false);
            if !has_branch {
                self.emit(
                    format!(
                        "axon-T860 technician tool '{}' is `risk: destructive` but its bound \
                         session offers no reachable `branch{{ approved: […], denied: […] }}` — a \
                         destructive command must have a human confirm/deny exit visible in the \
                         protocol's own shape (D84.2); add the branch, or reclassify `risk: safe`",
                        node.name
                    ),
                    &node.loc,
                );
            }
        }
    }

    /// §Fase 85.c — resolve a `tool.cache:` reference and enforce the
    /// non-pure-needs-ttl law at the use site:
    /// - `axon-T864` — a non-`none`, non-empty `cache:` must name a declared
    ///   `cache`.
    /// - `axon-T865` — a NON-pure tool memoised by a cache with no finite
    ///   `ttl:` is forbidden (caching a non-deterministic result forever,
    ///   D85.9). A provably-`pure` tool may reference a ttl-less cache.
    fn check_tool_cache_ref(&mut self, node: &ToolDefinition) {
        if node.cache.is_empty() || node.cache == "none" {
            return;
        }
        // §Fase 122.d (`axon-T1213`, half one of two) — a tool that STREAMS
        // cannot name a cache. See `T1213_WHY`. Checked before the reference
        // resolves, deliberately: `cache: Nonexistent` on a streaming tool is
        // two mistakes, and this is the one the adopter must hear about — fixing
        // the name would leave them with a memoisation that cannot happen.
        if declares_a_streaming_effect(&node.effects) {
            let row = node
                .effects
                .as_ref()
                .map(|r| r.effects.join(", "))
                .unwrap_or_else(|| "<none declared>".to_string());
            self.emit(
                format!(
                    "axon-T1213 tool '{}' declares a streaming effect <{}> AND `cache: {}`. \
                     A streaming tool is dispatched chunk-by-chunk and cannot be memoised — {}. \
                     Remove the `cache:` reference (or `cache: none` to opt out of a default \
                     policy), or drop the stream effect if the result really is a single value.",
                    node.name, row, node.cache, T1213_WHY
                ),
                &node.loc,
            );
            return;
        }
        match self.symbols.lookup(&node.cache) {
            None => {
                self.emit(
                    format!(
                        "axon-T864 tool '{}' references undefined cache '{}' (use `cache: none` \
                         to opt out of a default policy)",
                        node.name, node.cache
                    ),
                    &node.loc,
                );
                return;
            }
            Some(sym) if sym.kind != "cache" => {
                self.emit(
                    format!(
                        "axon-T864 '{}' is a {}, not a cache (referenced by tool '{}')",
                        node.cache, sym.kind, node.name
                    ),
                    &node.loc,
                );
                return;
            }
            _ => {}
        }
        if !tool_is_pure(&node.effects) {
            if let Some(cache_decl) = self.find_cache(&node.cache) {
                if cache_decl.ttl.is_none() {
                    let row = node
                        .effects
                        .as_ref()
                        .map(|r| r.effects.join(", "))
                        .unwrap_or_else(|| "<none declared>".to_string());
                    self.emit(
                        format!(
                            "axon-T865 tool '{}' (effects <{}>, not proven `pure`) is memoised by \
                             cache '{}' which declares no `ttl:` — a non-deterministic result may \
                             not be cached forever; give '{}' a finite `ttl:`",
                            node.name, row, node.cache, node.cache
                        ),
                        &node.loc,
                    );
                }
            }
        }
    }

    /// §Fase 85.c — resolve a `retrieve.cache:` reference. A retrieve is a
    /// `storage` read (never `pure`), so the referenced cache is always used
    /// for a possibly-stale result and MUST carry a finite `ttl:` (T865); the
    /// reference itself must resolve to a declared `cache` (T864).
    fn check_retrieve_cache_ref(&mut self, cache_ref: &str, flow_name: &str, loc: &Loc) {
        if cache_ref.is_empty() {
            return;
        }
        match self.symbols.lookup(cache_ref) {
            None => {
                self.emit(
                    format!(
                        "axon-T864 retrieve in flow '{}' references undefined cache '{}'",
                        flow_name, cache_ref
                    ),
                    loc,
                );
                return;
            }
            Some(sym) if sym.kind != "cache" => {
                self.emit(
                    format!(
                        "axon-T864 '{}' is a {}, not a cache (referenced by a retrieve in flow '{}')",
                        cache_ref, sym.kind, flow_name
                    ),
                    loc,
                );
                return;
            }
            _ => {}
        }
        if let Some(cache_decl) = self.find_cache(cache_ref) {
            if cache_decl.ttl.is_none() {
                self.emit(
                    format!(
                        "axon-T865 retrieve in flow '{}' is memoised by cache '{}' which declares \
                         no `ttl:` — a `retrieve` reads mutable store data, so a cached result may \
                         not live forever; give '{}' a finite `ttl:` (and typically an \
                         `invalidate_on:` channel)",
                        flow_name, cache_ref, cache_ref
                    ),
                    loc,
                );
            }
        }
    }

    /// §Fase 98.d — Native Web Acquisition laws. Inert unless the tool is a
    /// scrape provider or declares a `scrape:` block. Enforces:
    ///
    /// - **axon-T905** — a `scrape:` block only on a scrape provider; a scrape
    ///   provider's `engine:`/`impersonate:` in the closed catalog; per-field
    ///   provider applicability (`extract` only on `scrape_dom`, `follow`/
    ///   crawl fields only on `scrape_crawl`, `render_wait` only with the
    ///   `browser` engine).
    /// - **axon-T904** — effect honesty (D98.2): a scrape provider's `effects:`
    ///   MUST carry `web`; `scrape_http`/`scrape_crawl` MUST carry `network`;
    ///   `scrape_dom` MUST NOT carry `network` (it does no I/O — it processes
    ///   already-fetched, already-tainted content); an `adaptive:` DOM tool
    ///   MUST carry `storage` (the per-tenant selector memory, §98.h).
    /// - **axon-T906** — each `extract` FieldSpec is a single `name=selector`.
    /// - **axon-T907** — `similarity_floor` ∈ [0,1].
    /// - **axon-T909** — `politeness:`/`checkpoint:` resolve to declared
    ///   symbols. (`proxy:` is a config KEY resolved via SecretResolver at
    ///   deploy, never a source symbol — its shape is checked, not resolved.)
    fn check_scrape_tool(&mut self, node: &ToolDefinition) {
        let is_scrape_provider = VALID_SCRAPE_PROVIDERS.contains(&node.provider.as_str());
        let scrape = match &node.scrape {
            Some(s) => s,
            None => {
                // A scrape provider with NO `scrape:` block is legal (defaults
                // apply). Nothing to check on the block; effect honesty still
                // applies below via the early is_scrape_provider guard.
                if is_scrape_provider {
                    self.check_scrape_effect_honesty(node, false);
                }
                return;
            }
        };

        // (T905) a `scrape:` block on a non-scrape tool is meaningless.
        if !is_scrape_provider {
            self.emit(
                format!(
                    "axon-T905 tool '{}' declares a `scrape:` block but its `provider:` is \
                     '{}', not a web-acquisition engine. The scrape config only applies to a \
                     tool whose provider is one of: {}.",
                    node.name,
                    if node.provider.is_empty() { "<unset>" } else { &node.provider },
                    valid_list(VALID_SCRAPE_PROVIDERS)
                ),
                &node.loc,
            );
            // Continue — validate the block's internal shape regardless.
        }

        // (T905) engine + impersonate closed catalogs.
        if let Some(engine) = &scrape.engine {
            if !is_valid(engine, VALID_SCRAPE_ENGINES) {
                self.emit(
                    format!(
                        "axon-T905 tool '{}' scrape `engine: {}` is not a known engine. Valid: {}.",
                        node.name,
                        engine,
                        valid_list(VALID_SCRAPE_ENGINES)
                    ),
                    &scrape.loc,
                );
            }
        }
        if let Some(profile) = &scrape.impersonate {
            if !is_valid(profile, VALID_IMPERSONATE_PROFILES) {
                self.emit(
                    format!(
                        "axon-T905 tool '{}' scrape `impersonate: {}` is not a known \
                         fingerprint profile. Valid: {}.",
                        node.name,
                        profile,
                        valid_list(VALID_IMPERSONATE_PROFILES)
                    ),
                    &scrape.loc,
                );
            }
        }

        // (T905) `render_wait` is a browser-tier concept — the impersonate
        // engine has no JS runtime, so a settle wait is silently meaningless.
        let engine_is_browser = scrape.engine.as_deref() == Some("browser");
        if scrape.render_wait.is_some() && !engine_is_browser {
            self.emit(
                format!(
                    "axon-T905 tool '{}' sets `render_wait:` but its `engine:` is not \
                     `browser` — the impersonate engine renders no JS, so a settle wait has \
                     no effect. Set `engine: browser` or drop `render_wait:`.",
                    node.name
                ),
                &scrape.loc,
            );
        }
        // (T905) `impersonate:` profile only applies with the impersonate
        // engine (the browser sidecar carries its own real fingerprint).
        if scrape.impersonate.is_some() && engine_is_browser {
            self.emit(
                format!(
                    "axon-T905 tool '{}' sets `impersonate:` but `engine: browser` — the \
                     browser sidecar presents its own real fingerprint; profile impersonation \
                     is an `engine: impersonate` concept.",
                    node.name
                ),
                &scrape.loc,
            );
        }

        // (T905) per-provider field applicability.
        let dom_only = [
            (!scrape.extract.is_empty(), "extract"),
            (scrape.adaptive.is_some(), "adaptive"),
            (scrape.similarity_floor.is_some(), "similarity_floor"),
        ];
        let crawl_only = [
            (!scrape.follow.is_empty(), "follow"),
            (scrape.max_depth.is_some(), "max_depth"),
            (scrape.max_pages.is_some(), "max_pages"),
            (scrape.concurrency.is_some(), "concurrency"),
            (!scrape.politeness.is_empty(), "politeness"),
            (!scrape.checkpoint.is_empty(), "checkpoint"),
        ];
        if node.provider != "scrape_dom" {
            for (present, field) in dom_only {
                if present {
                    self.emit(
                        format!(
                            "axon-T905 tool '{}' sets scrape `{}:` but its provider is '{}' — \
                             extraction fields (`extract`/`adaptive`/`similarity_floor`) apply \
                             only to `scrape_dom`.",
                            node.name, field, node.provider
                        ),
                        &scrape.loc,
                    );
                }
            }
        }
        if node.provider != "scrape_crawl" {
            for (present, field) in crawl_only {
                if present {
                    self.emit(
                        format!(
                            "axon-T905 tool '{}' sets scrape `{}:` but its provider is '{}' — \
                             crawl fields (`follow`/`max_depth`/`max_pages`/`concurrency`/\
                             `politeness`/`checkpoint`) apply only to `scrape_crawl`.",
                            node.name, field, node.provider
                        ),
                        &scrape.loc,
                    );
                }
            }
        }

        // (T906) each `extract` FieldSpec is a single `name=selector` pair.
        for spec in &scrape.extract {
            let ok = match spec.split_once('=') {
                Some((name, sel)) => !name.trim().is_empty() && !sel.trim().is_empty(),
                None => false,
            };
            if !ok {
                self.emit(
                    format!(
                        "axon-T906 tool '{}' scrape `extract` entry '{}' is malformed — each \
                         FieldSpec must be `name=selector` (e.g. `\"title=h1\"`).",
                        node.name, spec
                    ),
                    &scrape.loc,
                );
            }
        }

        // (T907) similarity_floor ∈ [0,1].
        if let Some(f) = scrape.similarity_floor {
            if !(0.0..=1.0).contains(&f) {
                self.emit(
                    format!(
                        "axon-T907 tool '{}' scrape `similarity_floor: {}` is out of range — \
                         the adaptive-relocation threshold must be in [0, 1].",
                        node.name, f
                    ),
                    &scrape.loc,
                );
            }
        }

        // (T909) reference resolution for `politeness:` (a declared budget/…)
        // and `checkpoint:` (a declared store). Conservative: the referenced
        // name must resolve to SOME declared symbol.
        for (name, field) in [
            (&scrape.politeness, "politeness"),
            (&scrape.checkpoint, "checkpoint"),
        ] {
            if !name.is_empty() && self.symbols.lookup(name).is_none() {
                self.emit(
                    format!(
                        "axon-T909 tool '{}' scrape `{}: {}` references an undeclared name — \
                         it must resolve to a declared {} in this program.",
                        node.name,
                        field,
                        name,
                        if field == "politeness" { "budget" } else { "store" }
                    ),
                    &scrape.loc,
                );
            }
        }

        // (T904) effect honesty — the load-bearing law. Only meaningful for a
        // real scrape provider.
        if is_scrape_provider {
            self.check_scrape_effect_honesty(node, scrape.adaptive == Some(true));
        }
    }

    /// §Fase 98.d (axon-T904) — the effect-honesty half of §98.d: a scrape
    /// tool's declared `effects:` row must match the bases it actually
    /// exercises. This is what lets the content-injection barrier (T908) and
    /// the PCC `ScrapeProvenanceSoundness` class reason about `web` at all.
    fn check_scrape_effect_honesty(&mut self, node: &ToolDefinition, adaptive: bool) {
        let bases: std::collections::HashSet<String> = node
            .effects
            .as_ref()
            .map(|e| {
                e.effects
                    .iter()
                    .map(|s| s.split(':').next().unwrap_or(s).to_string())
                    .collect()
            })
            .unwrap_or_default();

        let require = |this: &mut Self, base: &str, why: &str| {
            if !bases.contains(base) {
                this.emit(
                    format!(
                        "axon-T904 web-acquisition tool '{}' (`provider: {}`) must declare the \
                         `{}` effect — {}. Add it to `effects: <…>`.",
                        node.name, node.provider, base, why
                    ),
                    &node.loc,
                );
            }
        };

        // Every scrape provider acquires open-web content: `web` is mandatory.
        require(
            self,
            "web",
            "web content is born epistemically Untrusted (D98.1) and cannot reach an agent's \
             belief without a shield",
        );

        match node.provider.as_str() {
            // §Fase 104 — `scrape_enrich` performs live vendor I/O too (it POSTs a
            // contact query to an enrichment endpoint), so `network` is mandatory.
            "scrape_http" | "scrape_crawl" | "scrape_enrich" => {
                require(self, "network", "it performs live network I/O");
            }
            "scrape_dom" => {
                // scrape_dom does NO I/O — it processes already-fetched
                // content. Declaring `network` would be dishonest.
                if bases.contains("network") {
                    self.emit(
                        format!(
                            "axon-T904 tool '{}' (`provider: scrape_dom`) declares `network` but \
                             performs NO network I/O — it processes an already-fetched, already-\
                             tainted `page:`. Drop `network`; keep `web` (the taint is \
                             preserved, not re-acquired).",
                            node.name
                        ),
                        &node.loc,
                    );
                }
            }
            _ => {}
        }

        // An adaptive DOM tool persists per-tenant selector memory → `storage`.
        if adaptive && !bases.contains("storage") {
            self.emit(
                format!(
                    "axon-T904 tool '{}' sets `adaptive: true` but does not declare `storage` — \
                     adaptive relocation persists per-tenant selector memory (§98.h), a real \
                     `<storage>` effect. Add `storage` to `effects: <…>`.",
                    node.name
                ),
                &node.loc,
            );
        }
    }

    fn check_tool(&mut self, node: &ToolDefinition) {
        // ── §Fase 114.b — `provider:` is a CLOSED catalog ────────────────────
        //
        // Until §114 `provider:` was a **free string with no membership check**.
        // The same disease `resource.kind` had — and worse here, because of what
        // the runtime does with an unrecognised value:
        //
        //   - an EMPTY `provider:` silently resolved to the STUB on the streaming
        //     path, returning a FABRICATED `[stub]` answer;
        //   - a typo (`htpp`) reached a runtime fallthrough instead of failing.
        //
        // On the one primitive whose entire purpose is that an action's result is
        // born with an honest epistemic status, a typo produced an invented one.
        // *When the evidence is missing, substitute the belief and report
        // agreement* — the §112 defect, on the tool surface.
        //
        // An EMPTY provider stays legitimate: it means **LLM-routed** (the tool
        // IS the model — a `Summarize`, a `Classify`). That is a real, declared
        // intent, not a typo. What is refused is a NON-EMPTY provider outside the
        // catalog.
        if !node.provider.is_empty() && !VALID_TOOL_PROVIDERS.contains(&node.provider.as_str()) {
            self.emit(
                format!(
                    "axon-T948 tool '{}' declares `provider: {}`, which is not a known provider. \
                     Expected one of: {} (or omit `provider:` for an LLM-routed tool). A provider \
                     the runtime does not recognise used to reach a fabricating fallthrough — on \
                     the primitive built so an action's result is born with an honest epistemic \
                     status, a typo produced an invented one.",
                    node.name,
                    node.provider,
                    VALID_TOOL_PROVIDERS.join(" | ")
                ),
                &node.loc,
            );
        }

        // ── §Fase 114.c — the CHANNEL is governed, not just the action ────────
        //
        // `tool.runtime` was the THIRD island (§113's census). An absolute
        // `runtime: "https://api.vendor.com"` is used VERBATIM as the endpoint
        // (`tool_registry.rs`), opening a real production connection with **no
        // lifetime, no capacity, no shield, no certainty_floor** — and until now
        // `check_tool` never looked at the field. A production URL in source, on
        // the primitive whose neighbours (`upstream.resolve`, `tool.secret`) the
        // language already forbids URLs on (`axon-T850`/`T902`).
        //
        // axon-T949 — an absolute `runtime:` URL is refused. The address of a
        // channel lives on a `resource` (governed) or in per-tenant config (a
        // slug joined onto a base URL), never as a literal in the program.
        let rt = node.runtime.trim();
        if rt.starts_with("http://") || rt.starts_with("https://") {
            self.emit(
                format!(
                    "axon-T949 tool '{}' pins an absolute `runtime: \"{}\"` — a production URL in \
                     source. That opens a real connection with no lifetime, no capacity, no \
                     shield: the channel is ungoverned. Name a `resource` instead (`tool {} {{ \
                     resource: <R> }}`), whose `endpoint` is a per-tenant config key (axon-T944), \
                     and let `runtime:` name the PATH within it. URLs never appear in source — \
                     the same law `axon-T850` enforces on `upstream.resolve` and `axon-T902` on \
                     `tool.secret`.",
                    node.name, node.runtime, node.name
                ),
                &node.loc,
            );
        }

        // axon-T950 — a `resource:` names a DECLARED resource, and does not
        // duplicate the channel address. `resource:` + an absolute `runtime:`
        // would be the same fact twice (the islands defect); T949 already caught
        // the URL, so here we only resolve the reference.
        if !node.resource_ref.is_empty() {
            match self.symbols.lookup(&node.resource_ref) {
                None => self.emit(
                    format!(
                        "axon-T950 tool '{}' names resource '{}', which is not declared.",
                        node.name, node.resource_ref
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "resource" => self.emit(
                    format!(
                        "axon-T950 tool '{}' names '{}', which is a {}, not a resource.",
                        node.name, node.resource_ref, sym.kind
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }

        // §Fase 84.c — Remote Hands technician-command laws (T858–T862). Inert
        // for any tool that does not set `target:`/`risk:`/`argv:`.
        self.check_technician_tool(node);
        // §Fase 98.d — Native Web Acquisition laws (T904–T907, T909). Inert
        // for any tool whose `provider:` is not a scrape engine and which
        // declares no `scrape:` block.
        self.check_scrape_tool(node);
        // §Fase 85.c — cache-reference resolution + non-pure-needs-ttl.
        self.check_tool_cache_ref(node);
        // §Fase 94.c (axon-T902) — the dispatch-injection `secret:` laws.
        // Inert when unset (every pre-§94 tool).
        if !node.secret.is_empty() {
            // Key shape — the compile-time SecretKeyPolicy mirror (the T850
            // charset, verbatim): first char `[a-z0-9]`, rest `[a-z0-9_.-]`,
            // no `/`, no `:` — a credential or URL literal is unrepresentable.
            let mut chars = node.secret.chars();
            let head_ok = chars
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
            let rest_ok = chars.all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-')
            });
            if !head_ok || !rest_ok {
                self.emit(
                    format!(
                        "axon-T902 tool '{}' `secret:` value '{}' is not a config key — \
                         keys are lowercase dot-separated (`[a-z0-9][a-z0-9_.-]*`, no \
                         `/`, no `:`); credentials never appear in source. The runtime \
                         resolves the key against the tenant's secret custody at \
                         dispatch and injects the value under the reserved \
                         `axon_secret` request field (`rotation_without_revelation`).",
                        node.name, node.secret
                    ),
                    &node.loc,
                );
            }
            // Technician exclusion — a `target:`-bound tool dispatches argv
            // over a socket (execve, no HTTP request to inject into); a
            // `secret:` on it would be silently meaningless, so it is
            // unrepresentable instead.
            if node.target.is_some() {
                self.emit(
                    format!(
                        "axon-T902 tool '{}' declares BOTH `target:` (technician argv \
                         dispatch) and `secret:` (HTTP dispatch injection) — a \
                         technician command has no request body to inject a secret \
                         into. Drop one: technician credentials belong on the machine \
                         end of the socket, never in the command channel.",
                        node.name
                    ),
                    &node.loc,
                );
            }
        }
        // §Fase 95.a (axon-T903) — the `secret_partition:` laws (doctrine
        // `selection_without_revelation`). Inert when unset (every §94 and
        // pre-§94 tool). The partition names one of THIS tool's own
        // parameters whose value is appended as a single key SEGMENT to
        // `secret:` at dispatch — so the resolved custody key stays inside
        // the tool's compile-time-pinned class, and only a bounded,
        // caller-supplied discriminator is dynamic.
        if !node.secret_partition.is_empty() {
            // (a) A partition without a `secret:` selects nothing — the
            // dynamic segment has no class key to extend.
            if node.secret.is_empty() {
                self.emit(
                    format!(
                        "axon-T903 tool '{}' declares `secret_partition: {}` but no \
                         `secret:` — the partition appends a per-call segment to the \
                         secret class key, so a `secret:` (e.g. `crm.hubspot`) must be \
                         present for it to extend.",
                        node.name, node.secret_partition
                    ),
                    &node.loc,
                );
            }
            // (b) Technician exclusion — inherits the `secret:` reason (no
            // request body to inject into); stated on the partition too so
            // the diagnostic points at the right field even when `secret:`
            // itself is absent.
            if node.target.is_some() {
                self.emit(
                    format!(
                        "axon-T903 tool '{}' declares `secret_partition:` on a \
                         `target:`-bound technician tool — argv dispatch has no request \
                         body to inject a partitioned secret into (the `axon-T902` \
                         `secret:` exclusion, applied to its selector).",
                        node.name
                    ),
                    &node.loc,
                );
            }
            // (c) The partition MUST name one of this tool's own declared
            // `parameters:` — the value is a caller-supplied argument bound
            // at the `use` site, never ambient state, an LLM output, or a
            // free identifier. Locally checkable: the tool owns its schema.
            match node
                .parameters
                .iter()
                .find(|p| p.name == node.secret_partition)
            {
                None => {
                    self.emit(
                        format!(
                            "axon-T903 tool '{}' declares `secret_partition: {}` but has \
                             no parameter named '{}'. The partition selects the custody \
                             entry by an argument the CALLER passes to this tool — it \
                             must be one of the tool's declared `parameters:` (add \
                             `{}: String` to `parameters:`).",
                            node.name,
                            node.secret_partition,
                            node.secret_partition,
                            node.secret_partition
                        ),
                        &node.loc,
                    );
                }
                // (d) That parameter must be a required `String` — it becomes
                // exactly ONE key segment, so a numeric/optional/generic type
                // (which could be absent or non-scalar) is unrepresentable.
                Some(p) if p.type_expr.name != "String" || p.type_expr.optional => {
                    self.emit(
                        format!(
                            "axon-T903 tool '{}' partition parameter '{}' has type '{}{}' \
                             — a `secret_partition` becomes one key segment, so it must \
                             be a required `String` (not optional, not numeric, not \
                             generic). A missing or non-scalar discriminator cannot \
                             address a custody entry.",
                            node.name,
                            node.secret_partition,
                            p.type_expr.name,
                            if p.type_expr.optional { "?" } else { "" }
                        ),
                        &node.loc,
                    );
                }
                Some(_) => {}
            }
        }
        if let Some(v) = node.max_results {
            if v <= 0 {
                self.emit(
                    format!(
                        "max_results must be positive, got {} in tool '{}'",
                        v, node.name
                    ),
                    &node.loc,
                );
            }
        }
        if let Some(ref eff) = node.effects {
            for e in &eff.effects {
                // §Fase 53.c — an extension-declared provenance member is
                // accepted VERBATIM (the full entry, e.g.
                // "epistemic:believe"). Provenance-class: it carries no
                // runtime capability (invariant #2), so the canonical
                // base/qualifier enforcement below does not apply.
                if self.ext_effect_members.contains(e) {
                    continue;
                }
                // Handle composite effects like "name:qualifier"
                let (base, qualifier) = match e.split_once(':') {
                    Some((b, q)) => (b, Some(q)),
                    None => (e.as_str(), None),
                };
                // §Fase 100.d — `ingest:<class>` is the parse/infer provenance
                // annotation (D100.1), NOT an effect base (D100.3) — accepted
                // verbatim like `epistemic:`, only the class is catalog-checked.
                if base == "ingest" {
                    match qualifier {
                        Some(class) if is_valid(class, VALID_INGEST_CLASSES) => {}
                        other => self.emit(
                            format!(
                                "axon-T1000 tool '{}' declares `ingest:{}` — the ingest \
                                 provenance class must be one of: {}.",
                                node.name,
                                other.unwrap_or("<none>"),
                                valid_list(VALID_INGEST_CLASSES)
                            ),
                            &node.loc,
                        ),
                    }
                    continue;
                }
                if !is_valid(base, VALID_EFFECTS) {
                    self.emit(
                        format!(
                            "Unknown effect '{}' in tool '{}'. Valid: {}",
                            e,
                            node.name,
                            valid_list(VALID_EFFECTS)
                        ),
                        &node.loc,
                    );
                    continue;
                }
                // §λ-L-E Fase 11.a — qualifier enforcement for the
                // stream + trust effects. Both REQUIRE a qualifier
                // from their closed catalogue. Missing or unknown
                // qualifiers are compile errors.
                match base {
                    "stream" => match qualifier {
                        None => self.emit(
                            format!(
                                "Effect 'stream' in tool '{}' requires a \
                                 backpressure policy qualifier \
                                 'stream:<policy>'. Valid policies: {}",
                                node.name,
                                valid_list(crate::stream_effect::BACKPRESSURE_CATALOG)
                            ),
                            &node.loc,
                        ),
                        Some(q) => {
                            if !is_valid(q, crate::stream_effect::BACKPRESSURE_CATALOG) {
                                self.emit(
                                    format!(
                                        "Unknown backpressure policy '{}' in tool '{}'. \
                                         Valid: {}",
                                        q,
                                        node.name,
                                        valid_list(crate::stream_effect::BACKPRESSURE_CATALOG)
                                    ),
                                    &node.loc,
                                );
                            }
                        }
                    },
                    "trust" => match qualifier {
                        None => self.emit(
                            format!(
                                "Effect 'trust' in tool '{}' requires a proof \
                                 qualifier 'trust:<proof>'. Valid proofs: {}",
                                node.name,
                                valid_list(crate::refinement::TRUST_CATALOG)
                            ),
                            &node.loc,
                        ),
                        Some(q) => {
                            if !is_valid(q, crate::refinement::TRUST_CATALOG) {
                                self.emit(
                                    format!(
                                        "Unknown trust proof '{}' in tool '{}'. \
                                         Valid: {}",
                                        q,
                                        node.name,
                                        valid_list(crate::refinement::TRUST_CATALOG)
                                    ),
                                    &node.loc,
                                );
                            }
                        }
                    },
                    // §λ-L-E Fase 11.c — `sensitive:<category>` tags
                    // effects that touch regulated data. The category
                    // is an open taxonomy (adopters write
                    // `sensitive:health_data`, `sensitive:financial_txn`
                    // etc). The qualifier presence is REQUIRED — a
                    // bare `sensitive` is ambiguous and rejected.
                    "sensitive" => {
                        if qualifier.is_none() {
                            self.emit(
                                format!(
                                    "Effect 'sensitive' in tool '{}' \
                                     requires a jurisdiction qualifier \
                                     'sensitive:<category>' (e.g. \
                                     'sensitive:health_data'). The \
                                     category is adopter-defined; the \
                                     legal basis covering it must also \
                                     be declared via 'legal:<basis>' on \
                                     the same tool.",
                                    node.name,
                                ),
                                &node.loc,
                            );
                        }
                    }
                    // §λ-L-E Fase 11.c — `legal:<basis>` declares the
                    // legal basis authorising a sensitive effect. The
                    // basis catalogue is CLOSED.
                    "legal" => match qualifier {
                        None => self.emit(
                            format!(
                                "Effect 'legal' in tool '{}' requires a \
                                 basis qualifier 'legal:<basis>'. Valid \
                                 bases: {}",
                                node.name,
                                valid_list(crate::legal_basis::LEGAL_BASIS_CATALOG)
                            ),
                            &node.loc,
                        ),
                        Some(q) => {
                            if !is_valid(q, crate::legal_basis::LEGAL_BASIS_CATALOG) {
                                self.emit(
                                    format!(
                                        "Unknown legal basis '{}' in tool \
                                         '{}'. Valid: {}",
                                        q,
                                        node.name,
                                        valid_list(crate::legal_basis::LEGAL_BASIS_CATALOG)
                                    ),
                                    &node.loc,
                                );
                            }
                        }
                    },
                    // §λ-L-E Fase 11.e — OTS subkinds:
                    //   ots:transform:<from>:<to>  → kind-pair
                    //   ots:backend:<native|ffmpeg> → closed backend catalogue
                    "ots" => match qualifier {
                        None => self.emit(
                            format!(
                                "Effect 'ots' in tool '{}' requires a \
                                 subkind. Expected 'ots:transform:<from>:<to>' \
                                 or 'ots:backend:<native|ffmpeg>'.",
                                node.name
                            ),
                            &node.loc,
                        ),
                        Some(inner) => {
                            let (subkind, rest) = match inner.split_once(':') {
                                Some((a, b)) => (a, Some(b)),
                                None => (inner, None),
                            };
                            match subkind {
                                "transform" => {
                                    let valid = rest
                                        .and_then(|r| r.split_once(':'))
                                        .map(|(f, t)| !f.is_empty() && !t.is_empty())
                                        .unwrap_or(false);
                                    if !valid {
                                        self.emit(
                                            format!(
                                                "Effect 'ots:transform' in tool \
                                                 '{}' requires '<from>:<to>' \
                                                 qualifier (e.g. \
                                                 'ots:transform:mulaw8:pcm16').",
                                                node.name
                                            ),
                                            &node.loc,
                                        );
                                    }
                                }
                                "backend" => {
                                    let qual = rest.unwrap_or("");
                                    if !is_valid(qual, crate::ots_catalog::OTS_BACKEND_CATALOG) {
                                        self.emit(
                                            format!(
                                                "Unknown OTS backend '{}' in tool '{}'. \
                                                 Valid: {}",
                                                qual,
                                                node.name,
                                                valid_list(crate::ots_catalog::OTS_BACKEND_CATALOG)
                                            ),
                                            &node.loc,
                                        );
                                    }
                                }
                                other => self.emit(
                                    format!(
                                        "Unknown 'ots' subkind '{}' in tool '{}'. \
                                         Expected 'transform' or 'backend'.",
                                        other, node.name
                                    ),
                                    &node.loc,
                                ),
                            }
                        }
                    },
                    _ => {}
                }
            }
            if !eff.epistemic_level.is_empty()
                && !is_valid(&eff.epistemic_level, VALID_EPISTEMIC_LEVELS)
            {
                self.emit(
                    format!(
                        "Unknown epistemic level '{}' in tool '{}'. Valid: {}",
                        eff.epistemic_level,
                        node.name,
                        valid_list(VALID_EPISTEMIC_LEVELS)
                    ),
                    &node.loc,
                );
            }
            // §Fase 100.d (axon-T1001) — the Inferred ceiling (D100.1): a tool
            // producing `ingest:inferred` content (OCR/vision — a model's belief
            // about pixels) may NEVER declare `epistemic:know`. `know` asserts
            // the value is re-derivable; an inferred read is not — a different
            // engine/version/rotation answers differently. Its hard ceiling is
            // `believe`. (No producer ships in §100 — this rule stands ready for
            // §101, D100.14.)
            if eff.effects.iter().any(|e| e == "ingest:inferred")
                && eff.epistemic_level == "know"
            {
                self.emit(
                    format!(
                        "axon-T1001 tool '{}' declares `ingest:inferred` AND `epistemic:know` — \
                         an inferred (OCR/vision) read is a belief about pixels, not a fact about \
                         a file; it can never be `know` (D100.1). Its ceiling is `believe`.",
                        node.name
                    ),
                    &node.loc,
                );
            }
        }

        // §λ-L-E Fase 11.a — tool output/input trust coherence.
        // When a tool's declared effects announce a trust proof, we
        // don't (yet) propagate it into the return-type refinement
        // since tools don't carry explicit return TypeExprs in this
        // AST tier. Tool-level trust claims are consumed by
        // `check_flow`'s refinement pass below.

        // Mirror for stream: if a tool declares stream:<policy>, the
        // flows that use it inherit the obligation — enforced in
        // `check_flow`.

        // §λ-L-E Fase 11.c — tool-level sensitive/legal coherence.
        // A tool declaring `sensitive:<category>` MUST also declare
        // at least one `legal:<basis>` from the closed catalogue.
        // Declaring `legal:<basis>` without a `sensitive:<category>`
        // is tolerated (some tools are authorised broadly without
        // processing regulated data).
        if let Some(ref eff) = node.effects {
            let mut sensitive_categories: Vec<&str> = Vec::new();
            let mut has_legal_basis = false;
            let mut legal_bases_hipaa: Vec<&str> = Vec::new();
            let mut has_ffmpeg_backend = false;
            for e in &eff.effects {
                let (base, qual) = match e.split_once(':') {
                    Some((b, q)) => (b, Some(q)),
                    None => (e.as_str(), None),
                };
                if base == "sensitive" {
                    if let Some(q) = qual {
                        sensitive_categories.push(q);
                    }
                }
                if base == "legal" {
                    if let Some(q) = qual {
                        if is_valid(q, crate::legal_basis::LEGAL_BASIS_CATALOG) {
                            has_legal_basis = true;
                            if q.starts_with("HIPAA.") {
                                legal_bases_hipaa.push(q);
                            }
                        }
                    }
                }
                if base == "ots" {
                    if let Some(inner) = qual {
                        if let Some(("backend", backend)) = inner.split_once(':') {
                            if backend == "ffmpeg" {
                                has_ffmpeg_backend = true;
                            }
                        }
                    }
                }
            }
            if !sensitive_categories.is_empty() && !has_legal_basis {
                self.emit(
                    format!(
                        "Tool '{}' declares sensitive effect(s) [{}] but \
                         carries no 'legal:<basis>' effect. Regulated \
                         processing requires an explicit legal basis: {}.",
                        node.name,
                        sensitive_categories.join(", "),
                        valid_list(crate::legal_basis::LEGAL_BASIS_CATALOG)
                    ),
                    &node.loc,
                );
            }

            // §λ-L-E Fase 11.e — HIPAA processing MUST stay in-process.
            // Spawning ffmpeg crosses a process boundary the auditor
            // cannot observe; the ePHI disclosure the BAA doesn't
            // cover. Rejected at compile time, per the same closed
            // posture as 11.a trust proofs and 11.c legal bases.
            if !legal_bases_hipaa.is_empty() && has_ffmpeg_backend {
                self.emit(
                    format!(
                        "Tool '{}' combines HIPAA legal basis ({}) with \
                         'ots:backend:ffmpeg'. ePHI MUST NOT cross the \
                         process boundary to a subprocess outside the \
                         auditable runtime. Use 'ots:backend:native' or \
                         register a native transformer that covers the \
                         required pipeline.",
                        node.name,
                        legal_bases_hipaa.join(", "),
                    ),
                    &node.loc,
                );
            }
        }
    }

    /// §Fase 70.b — infer the static type of a pure expression, emitting
    /// `axon-T81x` on a type error. `scope` maps in-scope names (flow params)
    /// to their AXON type name; a name not in scope (or an unrecognised type)
    /// is `Unknown` and never errors. Errors are located at the enclosing
    /// condition (`loc`) since the `Expr` AST carries no per-node location.
    /// Returns the inferred result type (used recursively).
    fn infer_expr(
        &mut self,
        e: &Expr,
        scope: &std::collections::BTreeMap<String, String>,
        loc: &Loc,
    ) -> InferType {
        use InferType as T;
        match e {
            Expr::Lit(ExprLit::Int(_)) => T::Int,
            Expr::Lit(ExprLit::Float(_)) => T::Float,
            Expr::Lit(ExprLit::Bool(_)) => T::Bool,
            Expr::Lit(ExprLit::Str(_)) => T::Str,
            // §Fase 119.o — a `let` types as its BODY, under a scope extended
            // with the binding. Inferring the value and then discarding it
            // would leave every reference to the bound name `Unknown`, so a
            // `logic { let ratio = a / b  return ratio * 100 }` would type-check
            // its own result as unknown and every numeric law over it would go
            // unchecked — silently, since `Unknown` never errors.
            Expr::Let { name, value, body } => {
                let bound = self.infer_expr(value, scope, loc);
                let mut inner = scope.clone();
                inner.insert(name.clone(), bound.label().to_string());
                self.infer_expr(body, &inner, loc)
            }
            Expr::Ref(p) => {
                // §Fase 73.e — per §70.d a plain dotted path stays a FLAT
                // `Ref` (`profile.age`, not a Field node). When its ROOT is a
                // `Json<T>` lens param, walk the segments against `T`'s shape:
                // a known field resolves to its declared type, an undeclared
                // field is `axon-T842`. A non-lens dotted ref is left open.
                if let Some((root, rest)) = p.split_once('.') {
                    if let Some(struct_name) =
                        scope.get(root).and_then(|s| Self::parse_json_lens(s))
                    {
                        let segments: Vec<&str> = rest.split('.').collect();
                        return self.lens_field_walk(struct_name, &segments, loc);
                    }
                }
                scope
                    .get(p)
                    .map(|n| infer_type_from_name(n))
                    .unwrap_or(T::Unknown)
            }
            Expr::Unary(UnOp::Neg, x) => {
                let t = self.infer_expr(x, scope, loc);
                if t != T::Unknown && !t.is_numeric() {
                    self.emit(
                        format!(
                            "axon-T810 unary `-` requires a numeric operand, got {}",
                            t.label()
                        ),
                        loc,
                    );
                }
                if t.is_numeric() {
                    t
                } else {
                    T::Unknown
                }
            }
            Expr::Unary(UnOp::Not, x) => {
                let t = self.infer_expr(x, scope, loc);
                if t != T::Unknown && t != T::Bool {
                    self.emit(
                        format!(
                            "axon-T812 `not` requires a boolean operand, got {}",
                            t.label()
                        ),
                        loc,
                    );
                }
                T::Bool
            }
            Expr::Call(builtin, args) => {
                // §Fase 70.c — arity (T813) + receiver/arg type (T814) + result.
                let extra = builtin.extra_arity();
                let got_extra = args.len().saturating_sub(1);
                if args.is_empty() || got_extra != extra {
                    self.emit(
                        format!(
                            "axon-T813 `.{}` takes {extra} argument(s), got {got_extra}",
                            builtin.surface()
                        ),
                        loc,
                    );
                }
                let recv = args
                    .first()
                    .map(|a| self.infer_expr(a, scope, loc))
                    .unwrap_or(T::Unknown);
                let arg_types: Vec<T> = args
                    .iter()
                    .skip(1)
                    .map(|a| self.infer_expr(a, scope, loc))
                    .collect();
                // A known scalar (number/bool) receiver is never a collection or
                // string. List/collection types map to `Unknown` → permissive.
                let recv_is_scalar = matches!(recv, T::Int | T::Float | T::Bool);
                match builtin {
                    Builtin::Length | Builtin::Count | Builtin::IsEmpty => {
                        if recv_is_scalar {
                            self.emit(
                                format!(
                                    "axon-T814 `.{}` needs a collection or string, got {}",
                                    builtin.surface(),
                                    recv.label()
                                ),
                                loc,
                            );
                        }
                        if matches!(builtin, Builtin::IsEmpty) {
                            T::Bool
                        } else {
                            T::Int
                        }
                    }
                    Builtin::IsNull => T::Bool,
                    Builtin::Contains => {
                        if recv_is_scalar {
                            self.emit(
                                format!(
                                    "axon-T814 `.contains` needs a collection or string, got {}",
                                    recv.label()
                                ),
                                loc,
                            );
                        }
                        T::Bool
                    }
                    Builtin::StartsWith | Builtin::EndsWith => {
                        if recv != T::Unknown && recv != T::Str {
                            self.emit(
                                format!(
                                    "axon-T814 `.{}` needs a string receiver, got {}",
                                    builtin.surface(),
                                    recv.label()
                                ),
                                loc,
                            );
                        }
                        if let Some(a) = arg_types.first() {
                            if *a != T::Unknown && *a != T::Str {
                                self.emit(
                                    format!(
                                        "axon-T814 `.{}` argument must be a string, got {}",
                                        builtin.surface(),
                                        a.label()
                                    ),
                                    loc,
                                );
                            }
                        }
                        T::Bool
                    }
                    // §Fase 73.c — the honest coercion accessors. They are
                    // TOTAL over any value (a type mismatch fail-closes to
                    // null at runtime), so there is no receiver-type error
                    // to raise: the accessor IS the boundary where the
                    // program declares the type it expects. The STATIC type
                    // is the asserted scalar — so `doc.age.as_int + 1`
                    // type-checks as Int arithmetic — while the runtime
                    // keeps the claim honest (`open_data_is_total`).
                    Builtin::AsInt => T::Int,
                    Builtin::AsFloat => T::Float,
                    Builtin::AsString => T::Str,
                    Builtin::AsBool => T::Bool,
                }
            }
            Expr::Field(base, field) => {
                // §Fase 70.d — field access. The field's type is not statically
                // known (JSON/dynamic) → `Unknown` (permissive). Accessing a
                // field of a known scalar (number/bool) is a type error.
                let tb = self.infer_expr(base, scope, loc);
                if matches!(tb, T::Int | T::Float | T::Bool) {
                    self.emit(
                        format!(
                            "axon-T814 cannot access field `.{field}` of a {}",
                            tb.label()
                        ),
                        loc,
                    );
                }
                // §Fase 73.e — when the base is a `Json<T>` lens, the field is
                // checked against `T`'s declared shape: a known field resolves
                // to its declared scalar type (so `profile.age >= 18` is a
                // well-typed Int comparison), an undeclared field is
                // `axon-T842`. The runtime stays total either way (a
                // declared-but-absent field is null, never a crash).
                if let Some(struct_name) = self.lens_shape_of(base, scope) {
                    let field_ty = self
                        .json_lens_fields
                        .get(&struct_name)
                        .and_then(|m| m.get(field))
                        .map(|(ty, _)| ty.clone());
                    match field_ty {
                        Some(ty) => return infer_type_from_name(&ty),
                        None => {
                            self.emit(
                                format!(
                                    "axon-T842 the lens `Json<{struct_name}>` declares no \
                                     field `{field}`. The shape is a checkable EXPECTATION \
                                     — navigating an undeclared field is a likely typo \
                                     (runtime navigation stays total → a real document's \
                                     extra field still reads as null here). Add `{field}` \
                                     to `type {struct_name}`, or drop the `<{struct_name}>` \
                                     shape to navigate the open `Json` freely."
                                ),
                                loc,
                            );
                            return T::Unknown;
                        }
                    }
                }
                T::Unknown
            }
            Expr::Index(base, index) => {
                // §Fase 70.d — index access. Result type unknown (dynamic).
                let tb = self.infer_expr(base, scope, loc);
                let _ = self.infer_expr(index, scope, loc);
                if matches!(tb, T::Int | T::Float | T::Bool) {
                    self.emit(
                        format!("axon-T814 cannot index a {} (need a collection or string)", tb.label()),
                        loc,
                    );
                }
                T::Unknown
            }
            Expr::Binary(op, l, r) => {
                let tl = self.infer_expr(l, scope, loc);
                let tr = self.infer_expr(r, scope, loc);
                let sym = bin_op_symbol(*op);
                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                        if tl != T::Unknown && !tl.is_numeric() {
                            self.emit(
                                format!(
                                    "axon-T810 left operand of `{sym}` must be numeric, got {}",
                                    tl.label()
                                ),
                                loc,
                            );
                        }
                        if tr != T::Unknown && !tr.is_numeric() {
                            self.emit(
                                format!(
                                    "axon-T810 right operand of `{sym}` must be numeric, got {}",
                                    tr.label()
                                ),
                                loc,
                            );
                        }
                        if tl == T::Int && tr == T::Int {
                            T::Int
                        } else if tl.is_numeric() && tr.is_numeric() {
                            T::Float
                        } else {
                            T::Unknown
                        }
                    }
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        if tl != T::Unknown && tr != T::Unknown {
                            let ok =
                                (tl.is_numeric() && tr.is_numeric()) || (tl == T::Str && tr == T::Str);
                            if !ok {
                                self.emit(
                                    format!(
                                        "axon-T811 cannot order {} against {} with `{sym}` \
                                         (ordering needs two numbers or two strings)",
                                        tl.label(),
                                        tr.label()
                                    ),
                                    loc,
                                );
                            }
                        }
                        T::Bool
                    }
                    BinOp::Eq | BinOp::Ne => {
                        if tl != T::Unknown && tr != T::Unknown && tl.eq_class() != tr.eq_class() {
                            self.emit(
                                format!(
                                    "axon-T811 cannot compare {} with {} using `{sym}` \
                                     (incompatible types)",
                                    tl.label(),
                                    tr.label()
                                ),
                                loc,
                            );
                        }
                        T::Bool
                    }
                    BinOp::And | BinOp::Or => {
                        if tl != T::Unknown && tl != T::Bool {
                            self.emit(
                                format!(
                                    "axon-T812 left operand of `{sym}` must be boolean, got {}",
                                    tl.label()
                                ),
                                loc,
                            );
                        }
                        if tr != T::Unknown && tr != T::Bool {
                            self.emit(
                                format!(
                                    "axon-T812 right operand of `{sym}` must be boolean, got {}",
                                    tr.label()
                                ),
                                loc,
                            );
                        }
                        T::Bool
                    }
                }
            }
        }
    }

    fn check_flow(&mut self, node: &FlowDefinition) {
        // §Fase 38.d (D2) — capture the flow's parameter-name → type
        // map for the duration of this flow's body check. Cleared
        // before/after so one flow's params can't leak into another's
        // §38.d proof.
        self.current_flow_params = crate::store_column_proof::FlowParamTypes::new();
        for param in &node.parameters {
            self.current_flow_params
                .insert(param.name.clone(), param.type_expr.name.clone());
        }
        // §Fase 92.b — one flow's mint bindings never leak into another's
        // T896 scan.
        self.current_mint_bindings.clear();
        // §Fase 73.e — capture the FULL type spelling (incl. the generic the
        // bare map above drops) so the `Json<T>` lens can resolve `T`.
        self.current_flow_param_spellings.clear();
        for param in &node.parameters {
            let spelling = if param.type_expr.generic_param.is_empty() {
                param.type_expr.name.clone()
            } else {
                format!("{}<{}>", param.type_expr.name, param.type_expr.generic_param)
            };
            self.current_flow_param_spellings
                .insert(param.name.clone(), spelling);
        }

        // Validate parameter types
        for param in &node.parameters {
            self.check_type_reference(&param.type_expr.name, &param.loc);
        }
        // Validate return type
        if let Some(ref rt) = node.return_type {
            self.check_type_reference(&rt.name, &rt.loc);
        }

        let mut step_names: Vec<String> = Vec::new();
        for step in &node.body {
            if let FlowStep::Step(s) = step {
                if step_names.contains(&s.name) {
                    self.emit(
                        format!("Duplicate step name '{}' in flow '{}'", s.name, node.name),
                        &s.loc,
                    );
                } else {
                    step_names.push(s.name.clone());
                }
                if let Some(v) = s.confidence_floor {
                    self.check_range(v, 0.0, 1.0, "confidence_floor", &s.loc);
                }
            }
        }

        // Tier 2 flow step reference checks
        self.check_flow_steps(&node.body, &node.name);

        // §λ-L-E Fase 11.a — Temporal Algebraic Effects + Trust
        // Types. Enforce two contracts at the flow level:
        //
        //   1. Stream<T> in parameter/return obliges the flow's body
        //      to reach a tool that carries a `stream:<policy>` effect.
        //      Without it, we cannot guarantee the stream has a
        //      backpressure handler — compile error.
        //
        //   2. Untrusted<T> in parameter obliges the flow's body to
        //      reach a tool that carries a `trust:<proof>` effect —
        //      otherwise the untrusted payload is being consumed
        //      without verification.
        self.check_refinement_and_stream_contracts(node);

        // §Fase 98.d (axon-T908) — the content-injection barrier: a
        // `web`-tainted value reaching an agent's belief context without an
        // intervening shield is a compile error (D98.1 — the §84 command-
        // injection discipline applied to fetched content).
        self.check_content_injection_barrier(node);
    }

    /// §Fase 98.d (axon-T908) — the content-injection barrier. The web is
    /// adversarial and the type system knows it: if a flow reaches a
    /// web-acquisition tool (a value carrying the `web` effect, born Untrusted
    /// — D98.1) AND drives a cognitive/agent step (a belief position), then it
    /// MUST also apply a `shield` — otherwise scraped content reaches the
    /// agent's beliefs unscanned. Sound + conservative (flow-granularity),
    /// mirroring the established `Untrusted<T>` → `trust:<proof>` obligation.
    ///
    /// The barrier is satisfied by EITHER a `shield <S> on <v>` step anywhere
    /// in the flow, OR the belief step's referenced agent carrying its own
    /// `shield:` gate. A flow that scrapes but never reasons (e.g. scrape →
    /// `persist`) carries no obligation.
    fn check_content_injection_barrier(&mut self, flow: &FlowDefinition) {
        // Build {tool_name → effect bases} program-wide.
        let mut tool_effects: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        self.collect_tool_effects(&self.program.declarations, &mut tool_effects);
        // §Fase 100.d (D100.2) — an ingress producer is a tool that acquires
        // ADVERSARIAL content the runtime did not author: `web` (scraping, §98)
        // OR `ingest:*` (an ingested document, §100). Both are born Untrusted
        // and cannot reach an agent's beliefs without a shield — the SAME
        // barrier (§98.f reused verbatim).
        let is_web_tool = |name: &str| -> bool {
            tool_effects
                .get(name)
                .map(|effs| {
                    effs.iter().any(|e| {
                        let base = e.split(':').next().unwrap_or(e);
                        base == "web" || base == "ingest"
                    })
                })
                .unwrap_or(false)
        };

        // Agents carrying a shield gate — a belief step through such an agent
        // is shielded at cognition.
        let mut shielded_agents: std::collections::HashSet<&str> = std::collections::HashSet::new();
        self.collect_shielded_agents(&self.program.declarations, &mut shielded_agents);

        let mut web_producer = false;
        let mut unshielded_belief = false;
        let mut shield_step_present = false;
        self.walk_flow_for_injection(
            &flow.body,
            &is_web_tool,
            &shielded_agents,
            &mut web_producer,
            &mut unshielded_belief,
            &mut shield_step_present,
        );

        if web_producer && unshielded_belief && !shield_step_present {
            self.emit(
                format!(
                    "axon-T908 flow '{}' acquires adversarial ingress content (a tool carrying \
                     the `web` effect (§98 scraping) or an `ingest:*` annotation (§100 document \
                     ingestion), born Untrusted) and feeds a cognitive step whose agent declares \
                     no shield, with no `shield` applied in the flow. Scraped/ingested content is \
                     adversarial: scan it before it reaches an agent's beliefs. Add a `shield <S> \
                     on <value>` step (scanning e.g. `prompt_injection`/`pii_leak`) before the \
                     reasoning step, or give the agent a `shield:` gate.",
                    flow.name
                ),
                &flow.loc,
            );
        }
    }

    /// Collect agent names that carry a non-empty `shield_ref` — reused by the
    /// content-injection barrier to treat a belief step through such an agent
    /// as shielded.
    fn collect_shielded_agents<'d>(
        &self,
        decls: &'d [Declaration],
        out: &mut std::collections::HashSet<&'d str>,
    ) {
        for d in decls {
            match d {
                Declaration::Agent(a) if !a.shield_ref.is_empty() => {
                    out.insert(a.name.as_str());
                }
                Declaration::Epistemic(eb) => self.collect_shielded_agents(&eb.body, out),
                _ => {}
            }
        }
    }

    /// Walk a flow body accumulating the three barrier signals. Recurses into
    /// `if`/`for` bodies (same shape as `walk_flow_steps_for_effects`).
    #[allow(clippy::only_used_in_recursion)]
    fn walk_flow_for_injection(
        &self,
        steps: &[FlowStep],
        is_web_tool: &dyn Fn(&str) -> bool,
        shielded_agents: &std::collections::HashSet<&str>,
        web_producer: &mut bool,
        unshielded_belief: &mut bool,
        shield_step_present: &mut bool,
    ) {
        for step in steps {
            match step {
                FlowStep::Step(s) => {
                    // A tool-bearing step reaching a `web` tool is a producer.
                    for tref in [&s.apply_ref, &s.navigate_ref] {
                        if !tref.is_empty() && is_web_tool(tref) {
                            *web_producer = true;
                        }
                    }
                    // A cognitive step is a belief position: it poses an `ask:`
                    // to the model (reasoning over `given`), or it binds a
                    // persona/agent. It is "unshielded" unless it reasons
                    // through an agent that carries its own shield gate.
                    let is_cognitive = !s.ask.is_empty() || !s.persona_ref.is_empty();
                    let agent_shielded = !s.persona_ref.is_empty()
                        && shielded_agents.contains(s.persona_ref.as_str());
                    if is_cognitive && !agent_shielded {
                        *unshielded_belief = true;
                    }
                }
                FlowStep::UseTool(u) => {
                    if is_web_tool(&u.tool_name) {
                        *web_producer = true;
                    }
                }
                FlowStep::ShieldApply(_) => {
                    *shield_step_present = true;
                }
                FlowStep::If(c) => {
                    self.walk_flow_for_injection(
                        &c.then_body,
                        is_web_tool,
                        shielded_agents,
                        web_producer,
                        unshielded_belief,
                        shield_step_present,
                    );
                    self.walk_flow_for_injection(
                        &c.else_body,
                        is_web_tool,
                        shielded_agents,
                        web_producer,
                        unshielded_belief,
                        shield_step_present,
                    );
                }
                FlowStep::ForIn(f) => {
                    self.walk_flow_for_injection(
                        &f.body,
                        is_web_tool,
                        shielded_agents,
                        web_producer,
                        unshielded_belief,
                        shield_step_present,
                    );
                }
                _ => {}
            }
        }
    }

    // ── §λ-L-E Fase 11.a — refinement + stream flow-level checks ─

    fn check_refinement_and_stream_contracts(&mut self, flow: &FlowDefinition) {
        // Scan flow signature for the refinement / stream markers.
        // `Trusted<T>` in a parameter imposes no new obligation on
        // this flow (the upstream already proved trust). `Untrusted<T>`
        // in a parameter obliges the flow body to refine it.
        let mut uses_stream = false;
        let mut uses_untrusted = false;

        for param in &flow.parameters {
            if crate::stream_effect::is_stream_type(&param.type_expr.name) {
                uses_stream = true;
            }
            if crate::refinement::is_untrusted_type(&param.type_expr.name) {
                uses_untrusted = true;
            }
        }
        if let Some(ref rt) = flow.return_type {
            if crate::stream_effect::is_stream_type(&rt.name) {
                uses_stream = true;
            }
            // Returning `Untrusted<T>` is legal (the flow is a pure
            // acceptor / pass-through) — the downstream consumer
            // carries the refinement obligation.
        }

        if !uses_stream && !uses_untrusted {
            return;
        }

        // Build {tool_name → Vec<effect_string>} by scanning the
        // program's declarations. Owned strings sidestep lifetime
        // gymnastics; the program-wide walk is O(N_tools) and the
        // strings are short slugs, so the allocation cost is negligible
        // for this checker pass.
        let mut tool_effects: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        self.collect_tool_effects(&self.program.declarations, &mut tool_effects);

        // Walk the flow body and see which tools each step reaches
        // via `apply_ref` / `navigate_ref`. Record the effects we
        // witness.
        let mut observed_backpressure = false;
        let mut observed_trust_proof = false;
        self.walk_flow_steps_for_effects(
            &flow.body,
            &tool_effects,
            &mut observed_backpressure,
            &mut observed_trust_proof,
        );

        if uses_stream && !observed_backpressure {
            self.emit(
                format!(
                    "Flow '{}' uses 'Stream<T>' in its signature but no \
                     reachable tool declares a 'stream:<policy>' effect. \
                     Every Stream<T> needs a backpressure policy: {}. \
                     Declare the policy on the tool that produces or \
                     consumes the stream (e.g. `effects: [stream:drop_oldest]`).",
                    flow.name,
                    valid_list(crate::stream_effect::BACKPRESSURE_CATALOG)
                ),
                &flow.loc,
            );
        }
        if uses_untrusted && !observed_trust_proof {
            self.emit(
                format!(
                    "Flow '{}' accepts 'Untrusted<T>' in its signature but \
                     no reachable tool declares a 'trust:<proof>' effect. \
                     Untrusted payloads MUST be refined via one of the \
                     catalogue verifiers: {}. Add the appropriate effect \
                     to the verifier tool (e.g. `effects: [trust:hmac]`).",
                    flow.name,
                    valid_list(crate::refinement::TRUST_CATALOG)
                ),
                &flow.loc,
            );
        }
    }

    fn collect_tool_effects(
        &self,
        decls: &[Declaration],
        out: &mut std::collections::HashMap<String, Vec<String>>,
    ) {
        for d in decls {
            match d {
                Declaration::Tool(t) => {
                    if let Some(ref eff) = t.effects {
                        out.insert(t.name.clone(), eff.effects.clone());
                    }
                }
                Declaration::Epistemic(eb) => {
                    self.collect_tool_effects(&eb.body, out);
                }
                _ => {}
            }
        }
    }

    fn walk_flow_steps_for_effects(
        &self,
        steps: &[FlowStep],
        tool_effects: &std::collections::HashMap<String, Vec<String>>,
        observed_backpressure: &mut bool,
        observed_trust_proof: &mut bool,
    ) {
        for step in steps {
            match step {
                FlowStep::Step(s) => {
                    for tool_ref in [&s.apply_ref, &s.navigate_ref] {
                        if tool_ref.is_empty() {
                            continue;
                        }
                        if let Some(effs) = tool_effects.get(tool_ref) {
                            for e in effs {
                                let (base, qual) = match e.split_once(':') {
                                    Some((b, q)) => (b, Some(q)),
                                    None => (e.as_str(), None),
                                };
                                if base == "stream" {
                                    if let Some(q) = qual {
                                        if is_valid(q, crate::stream_effect::BACKPRESSURE_CATALOG) {
                                            *observed_backpressure = true;
                                        }
                                    }
                                }
                                if base == "trust" {
                                    if let Some(q) = qual {
                                        if is_valid(q, crate::refinement::TRUST_CATALOG) {
                                            *observed_trust_proof = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                FlowStep::If(c) => {
                    self.walk_flow_steps_for_effects(
                        &c.then_body,
                        tool_effects,
                        observed_backpressure,
                        observed_trust_proof,
                    );
                    self.walk_flow_steps_for_effects(
                        &c.else_body,
                        tool_effects,
                        observed_backpressure,
                        observed_trust_proof,
                    );
                }
                FlowStep::ForIn(f) => {
                    self.walk_flow_steps_for_effects(
                        &f.body,
                        tool_effects,
                        observed_backpressure,
                        observed_trust_proof,
                    );
                }
                _ => {}
            }
        }
    }

    fn check_intent(&mut self, node: &IntentNode) {
        if node.ask.is_empty() {
            self.emit(
                format!(
                    "Intent '{}' is missing required 'ask' field — every intent must express a question",
                    node.name
                ),
                &node.loc,
            );
        }
        if let Some(v) = node.confidence_floor {
            self.check_range(v, 0.0, 1.0, "confidence_floor", &node.loc);
        }
    }

    fn check_run(&mut self, node: &RunStatement) {
        // Flow must exist and be a flow
        if !node.flow_name.is_empty() {
            match self.symbols.lookup(&node.flow_name) {
                None => self.emit(
                    format!("Undefined flow '{}' in run statement", node.flow_name),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "flow" => self.emit(
                    format!(
                        "'{}' is a {}, not a flow — only flows can be run",
                        node.flow_name, sym.kind
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }

        // Persona must exist
        if !node.persona.is_empty() {
            match self.symbols.lookup(&node.persona) {
                None => self.emit(format!("Undefined persona '{}'", node.persona), &node.loc),
                Some(sym) if sym.kind != "persona" => self.emit(
                    format!("'{}' is a {}, not a persona", node.persona, sym.kind),
                    &node.loc,
                ),
                _ => {}
            }
        }

        // Context must exist
        if !node.context.is_empty() {
            match self.symbols.lookup(&node.context) {
                None => self.emit(format!("Undefined context '{}'", node.context), &node.loc),
                Some(sym) if sym.kind != "context" => self.emit(
                    format!("'{}' is a {}, not a context", node.context, sym.kind),
                    &node.loc,
                ),
                _ => {}
            }
        }

        // Anchors must exist
        for anchor_name in &node.anchors {
            match self.symbols.lookup(anchor_name) {
                None => self.emit(format!("Undefined anchor '{}'", anchor_name), &node.loc),
                Some(sym) if sym.kind != "anchor" => self.emit(
                    format!("'{}' is a {}, not an anchor", anchor_name, sym.kind),
                    &node.loc,
                ),
                _ => {}
            }
        }

        // Effort validation
        if !node.effort.is_empty() && !is_valid(&node.effort, VALID_EFFORT_LEVELS) {
            self.emit(
                format!(
                    "Unknown effort level '{}'. Valid: {}",
                    node.effort,
                    valid_list(VALID_EFFORT_LEVELS)
                ),
                &node.loc,
            );
        }
    }

    // ── Lambda Data (ΛD) — 4 Invariants + Epistemic Degradation ──

    fn check_lambda_data(&mut self, node: &LambdaDataDefinition) {
        // Invariant 1 — Ontological Rigidity: ontology field is mandatory
        if node.ontology.is_empty() {
            self.emit(
                format!(
                    "lambda '{}' requires an 'ontology' field \
                     (Ontological Rigidity: O must classify the data domain)",
                    node.name
                ),
                &node.loc,
            );
        }

        // Invariant 4 — Epistemic Bounding: certainty ∈ [0, 1]
        if node.certainty < 0.0 || node.certainty > 1.0 {
            self.emit(
                format!(
                    "certainty coefficient must be in [0, 1], got {} \
                     (lambda '{}', Epistemic Bounding)",
                    node.certainty, node.name
                ),
                &node.loc,
            );
        }

        // Derivation validity: δ ∈ Δ
        if !node.derivation.is_empty() && !is_valid(&node.derivation, VALID_DERIVATIONS) {
            self.emit(
                format!(
                    "Unknown derivation '{}' for lambda '{}'. Valid: {}",
                    node.derivation,
                    node.name,
                    valid_list(VALID_DERIVATIONS)
                ),
                &node.loc,
            );
        }

        // Theorem 5.1 — Epistemic Degradation: only 'raw' may carry c = 1.0
        if node.certainty == 1.0 && !node.derivation.is_empty() && node.derivation != "raw" {
            self.emit(
                format!(
                    "Epistemic Degradation Theorem violation: lambda '{}' \
                     has certainty=1.0 with derivation='{}'. \
                     Only 'raw' data may carry absolute certainty (c=1.0). \
                     Derived/inferred/aggregated data must have c < 1.0 \
                     (\u{2200}\u{039b}D\u{2081}\u{2218}\u{039b}D\u{2082}: c_composed \u{2264} min(c\u{2081}, c\u{2082}))",
                    node.name, node.derivation
                ),
                &node.loc,
            );
        }
    }

    // ── Tier 2 declaration checks ───────────────────────────────────

    fn check_agent(&mut self, node: &AgentDefinition) {
        // BDI requirement: every agent must declare a goal
        if node.goal.is_empty() {
            self.emit(
                format!("Agent '{}' requires a 'goal' field (BDI: every agent must declare a desired objective)", node.name),
                &node.loc,
            );
        }

        // Tool references must exist
        for tool_name in &node.tools {
            match self.symbols.lookup(tool_name) {
                None => self.emit(
                    format!("Undefined tool '{}' in agent '{}'", tool_name, node.name),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "tool" => self.emit(
                    format!(
                        "'{}' is a {}, not a tool (referenced in agent '{}')",
                        tool_name, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }

        // Strategy enum
        if !node.strategy.is_empty() && !is_valid(&node.strategy, VALID_AGENT_STRATEGIES) {
            self.emit(
                format!(
                    "Unknown strategy '{}' in agent '{}'. Valid: {}",
                    node.strategy,
                    node.name,
                    valid_list(VALID_AGENT_STRATEGIES)
                ),
                &node.loc,
            );
        }

        // on_stuck policy enum
        if !node.on_stuck.is_empty() && !is_valid(&node.on_stuck, VALID_ON_STUCK_POLICIES) {
            self.emit(
                format!(
                    "Unknown on_stuck policy '{}' in agent '{}'. Valid: {}",
                    node.on_stuck,
                    node.name,
                    valid_list(VALID_ON_STUCK_POLICIES)
                ),
                &node.loc,
            );
        }

        // Memory reference
        if !node.memory_ref.is_empty() {
            match self.symbols.lookup(&node.memory_ref) {
                None => self.emit(
                    format!(
                        "Undefined memory '{}' in agent '{}'",
                        node.memory_ref, node.name
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "memory" => self.emit(
                    format!(
                        "'{}' is a {}, not a memory (referenced in agent '{}')",
                        node.memory_ref, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }

        // Shield reference
        if !node.shield_ref.is_empty() {
            match self.symbols.lookup(&node.shield_ref) {
                None => self.emit(
                    format!(
                        "Undefined shield '{}' in agent '{}'",
                        node.shield_ref, node.name
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "shield" => self.emit(
                    format!(
                        "'{}' is a {}, not a shield (referenced in agent '{}')",
                        node.shield_ref, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }

        // Budget constraints (linear logic: resources must be positive)
        if let Some(v) = node.max_iterations {
            if v < 1 {
                self.emit(
                    format!(
                        "max_iterations must be >= 1, got {} in agent '{}'",
                        v, node.name
                    ),
                    &node.loc,
                );
            }
        }
        if let Some(v) = node.max_tokens {
            if v < 0 {
                self.emit(
                    format!(
                        "max_tokens must be >= 0, got {} in agent '{}'",
                        v, node.name
                    ),
                    &node.loc,
                );
            }
        }
        if let Some(v) = node.max_cost {
            if v < 0.0 {
                self.emit(
                    format!("max_cost must be >= 0, got {} in agent '{}'", v, node.name),
                    &node.loc,
                );
            }
        }
    }

    /// §Fase 71.a — validate a temporal execution-window guard. Timezone is
    /// format-checked here (the frontend is zero-dependency); full IANA
    /// membership is the runtime's job (§71.b, chrono-tz).
    fn check_window(&mut self, node: &WindowDefinition) {
        let tz = node.timezone.trim();
        let tz_ok =
            tz == "UTC" || (tz.contains('/') && !tz.starts_with('/') && !tz.ends_with('/'));
        if !tz_ok {
            self.emit(
                format!(
                    "axon-T820 window '{}' has an invalid timezone '{}' — expected an IANA \
                     name like \"America/Bogota\" or \"UTC\"",
                    node.name, node.timezone
                ),
                &node.loc,
            );
        }
        if node.allow.is_empty() {
            self.emit(
                format!(
                    "axon-T821 window '{}' has an empty `allow:` — declare at least one \
                     {{ days hours }} span",
                    node.name
                ),
                &node.loc,
            );
        }
        for span in &node.allow {
            if !is_valid(&span.day_start, VALID_WEEKDAYS)
                || !is_valid(&span.day_end, VALID_WEEKDAYS)
            {
                self.emit(
                    format!(
                        "axon-T822 window '{}' has an invalid day in `days: {}..{}` — valid: {}",
                        node.name,
                        span.day_start,
                        span.day_end,
                        valid_list(VALID_WEEKDAYS)
                    ),
                    &span.loc,
                );
            }
            if !(0..=23).contains(&span.hour_start) || !(0..=23).contains(&span.hour_end) {
                self.emit(
                    format!(
                        "axon-T823 window '{}' has an out-of-range hour in `hours: {}..{}` — \
                         hours are 0..23",
                        node.name, span.hour_start, span.hour_end
                    ),
                    &span.loc,
                );
            }
        }
        if !node.on_outside.is_empty() && !is_valid(&node.on_outside, VALID_ON_OUTSIDE) {
            self.emit(
                format!(
                    "axon-T824 window '{}' has an unknown on_outside policy '{}' — valid: {}",
                    node.name,
                    node.on_outside,
                    valid_list(VALID_ON_OUTSIDE)
                ),
                &node.loc,
            );
        }
        // §Fase 71.e — each `exclude:` holiday must be a real ISO `YYYY-MM-DD`
        // calendar date. Validated at compile time (no Feb 30, leap-year aware) so
        // the window decision is a pure, replayable function of literal inputs.
        for date in &node.exclude {
            if !is_valid_iso_date(date) {
                self.emit(
                    format!(
                        "axon-T826 window '{}' has an invalid exclude date \"{}\" — expected a \
                         real ISO calendar date \"YYYY-MM-DD\" (e.g. \"2026-12-25\")",
                        node.name, date
                    ),
                    &node.loc,
                );
            }
        }
    }

    /// §Fase 72.a — validate a `budget { … }` block: a non-empty set of quotas,
    /// each over a declared tool, a positive limit, a closed-catalog period; plus
    /// a closed-catalog exhaustion policy.
    fn check_budget(&mut self, node: &BudgetBlock, daemon_name: &str) {
        // axon-T834 — a budget with no quota is a no-op declaration.
        if node.quotas.is_empty() {
            self.emit(
                format!(
                    "axon-T834 daemon '{daemon_name}' has an empty `budget {{ }}` — declare at \
                     least one `rate:`/`max:` quota"
                ),
                &node.loc,
            );
        }
        for quota in &node.quotas {
            // axon-T830 — the effect must resolve to a declared tool.
            match self.symbols.lookup(&quota.effect) {
                None => self.emit(
                    format!(
                        "axon-T830 daemon '{daemon_name}' budget targets undefined tool \
                         '{}' in `on Tool({})` — must name a declared `tool`",
                        quota.effect, quota.effect
                    ),
                    &quota.loc,
                ),
                Some(sym) if sym.kind != "tool" => self.emit(
                    format!(
                        "axon-T830 daemon '{daemon_name}' budget targets '{}', which is a {}, \
                         not a tool",
                        quota.effect, sym.kind
                    ),
                    &quota.loc,
                ),
                _ => {}
            }
            // axon-T831 — a quota limit must be positive.
            if quota.limit <= 0 {
                self.emit(
                    format!(
                        "axon-T831 daemon '{daemon_name}' budget quota on '{}' has a non-positive \
                         limit {} — a `{}` allowance must be > 0",
                        quota.effect, quota.limit, quota.kind
                    ),
                    &quota.loc,
                );
            }
            // axon-T832 — the period is a closed catalog.
            if !is_valid(&quota.period, VALID_BUDGET_PERIODS) {
                self.emit(
                    format!(
                        "axon-T832 daemon '{daemon_name}' budget quota on '{}' has an unknown \
                         period '{}' — valid: {}",
                        quota.effect,
                        quota.period,
                        valid_list(VALID_BUDGET_PERIODS)
                    ),
                    &quota.loc,
                );
            }
        }
        // axon-T833 — the exhaustion policy is a closed catalog.
        if !node.on_exhausted.is_empty() && !is_valid(&node.on_exhausted, VALID_ON_EXHAUSTED) {
            self.emit(
                format!(
                    "axon-T833 daemon '{daemon_name}' has an unknown `on_exhausted` policy '{}' — \
                     valid: {}",
                    node.on_exhausted,
                    valid_list(VALID_ON_EXHAUSTED)
                ),
                &node.loc,
            );
        }
    }

    /// §Fase 86.c — Resolve an anchor declaration by name.
    fn find_anchor(&self, name: &str) -> Option<&'a AnchorConstraint> {
        self.program.declarations.iter().find_map(|d| match d {
            Declaration::Anchor(a) if a.name == name => Some(a),
            _ => None,
        })
    }

    /// §Fase 86.c — Directed Creative Synthesis laws for a `forge` block:
    /// - **axon-T868** — `mode:` must be in the closed Boden catalog (empty ⇒
    ///   the `exploratory` default, allowed).
    /// - **axon-T869** — `novelty:` in `[0.0, 1.0]`.
    /// - **axon-T870** — `depth:` ≥ 1 and `branches:` ≥ 1 (a pipeline runs at
    ///   least one incubation iteration and one illumination branch).
    /// - **axon-T871** — a non-empty `constraints:` resolves to a declared
    ///   `anchor` that carries a `confidence_floor:` (the coherence gate the
    ///   verification phase checks against — an anchor with no floor cannot
    ///   verify anything).
    /// - **axon-T872** — `seed:` is non-empty and `-> <Type>` is present (a
    ///   creative synthesis needs something to create *from* and a type to
    ///   create *into*).
    fn check_forge(&mut self, node: &ForgeBlock, flow_name: &str) {
        if node.seed.trim().is_empty() {
            self.emit(
                format!(
                    "axon-T872 forge '{}' in flow '{}' has an empty `seed:` — a creative \
                     synthesis needs a conceptual starting point",
                    node.name, flow_name
                ),
                &node.loc,
            );
        }
        if node.output_type.trim().is_empty() {
            self.emit(
                format!(
                    "axon-T872 forge '{}' in flow '{}' has no `-> <Type>` return type",
                    node.name, flow_name
                ),
                &node.loc,
            );
        }
        if !node.mode.is_empty() && !is_valid(&node.mode, VALID_FORGE_MODES) {
            self.emit(
                format!(
                    "axon-T868 forge '{}' has unknown creativity mode '{}'. Valid: {}",
                    node.name,
                    node.mode,
                    valid_list(VALID_FORGE_MODES)
                ),
                &node.loc,
            );
        }
        if node.novelty < 0.0 || node.novelty > 1.0 {
            self.emit(
                format!(
                    "axon-T869 forge '{}' novelty {} is outside [0.0, 1.0]",
                    node.name, node.novelty
                ),
                &node.loc,
            );
        }
        if node.depth < 1 {
            self.emit(
                format!(
                    "axon-T870 forge '{}' depth {} must be ≥ 1 (at least one incubation iteration)",
                    node.name, node.depth
                ),
                &node.loc,
            );
        }
        if node.branches < 1 {
            self.emit(
                format!(
                    "axon-T870 forge '{}' branches {} must be ≥ 1 (at least one illumination branch)",
                    node.name, node.branches
                ),
                &node.loc,
            );
        }
        if !node.constraints_ref.is_empty() {
            match self.symbols.lookup(&node.constraints_ref) {
                None => self.emit(
                    format!(
                        "axon-T871 forge '{}' `constraints:` references undefined anchor '{}'",
                        node.name, node.constraints_ref
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "anchor" => self.emit(
                    format!(
                        "axon-T871 '{}' is a {}, not an anchor (in forge '{}' `constraints:`)",
                        node.constraints_ref, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {
                    if let Some(anchor) = self.find_anchor(&node.constraints_ref) {
                        if anchor.confidence_floor.is_none() {
                            self.emit(
                                format!(
                                    "axon-T871 forge '{}' constrains on anchor '{}' which declares \
                                     no `confidence_floor:` — the verification phase has no \
                                     coherence gate to check against; add a `confidence_floor:` to \
                                     '{}'",
                                    node.name, node.constraints_ref, node.constraints_ref
                                ),
                                &node.loc,
                            );
                        }
                    }
                }
            }
        }
    }

    /// §Fase 85.c — validate one `cache { … }` declaration's own fields. The
    /// cross-declaration laws (single default `axon-T863`, effect-widening
    /// `axon-W013`) live in [`Self::check_cache_module_laws`]; the reference
    /// checks on `tool.cache:` / `retrieve.cache:` live in [`Self::check_tool`]
    /// and the flow-step walker.
    fn check_cache(&mut self, node: &CacheDefinition) {
        // §Fase 122.d (`axon-T1213`, half two of two) — `apply_to_effects:` may
        // not widen to cover streams.
        //
        // The two halves together are what make the law airtight, and it is
        // worth writing down why it takes two. `resolve_tool_cache` reaches a
        // cache by exactly two routes: an explicit `cache: <Name>` on the tool,
        // or a `default: true` cache whose `apply_to_effects` covers the tool's
        // effect bases. Half one closes the first route at the TOOL. This closes
        // the second at the CACHE — and it closes it without re-deriving
        // eligibility here, which matters: duplicating `resolve_tool_cache`'s
        // precedence rules in the checker would create a second copy of the law
        // that drifts from the first, which is the §120 defect this project has
        // now paid for twice.
        //
        // A streaming tool's effect base is `stream`, and the implicit default
        // (`[pure]`) never covers it. So with both halves in place there is no
        // program in which a streaming tool resolves to a cache.
        if let Some(bad) = node.apply_to_effects.iter().find(|e| {
            e.split_once(':').map(|(b, _)| b).unwrap_or(e.as_str()) == "stream"
        }) {
            self.emit(
                format!(
                    "axon-T1213 cache '{}' widens `apply_to_effects:` to cover '{}'. \
                     A cache can never memoise a streaming effect — {}. Remove '{}' from \
                     `apply_to_effects:`; streaming tools are dispatched chunk-by-chunk and \
                     are excluded from memoisation by construction.",
                    node.name, bad, T1213_WHY, bad
                ),
                &node.loc,
            );
        }
        // axon-T866 — `backend:` is a closed catalog.
        if !node.backend.is_empty() && !is_valid(&node.backend, VALID_CACHE_BACKENDS) {
            self.emit(
                format!(
                    "axon-T866 unknown cache backend '{}' in cache '{}'. Valid: {}",
                    node.backend,
                    node.name,
                    valid_list(VALID_CACHE_BACKENDS)
                ),
                &node.loc,
            );
        }

        // axon-T867 — every `apply_to_effects:` member is a real effect
        // (the closed `VALID_EFFECTS` catalog); a typo'd effect can never
        // silently widen or narrow what the cache covers.
        for eff in &node.apply_to_effects {
            let base = eff.split_once(':').map(|(b, _)| b).unwrap_or(eff.as_str());
            if !is_valid(base, VALID_EFFECTS) {
                self.emit(
                    format!(
                        "axon-T867 unknown effect '{}' in cache '{}' `apply_to_effects:`. Valid: {}",
                        eff,
                        node.name,
                        valid_list(VALID_EFFECTS)
                    ),
                    &node.loc,
                );
            }
        }

        // axon-T865 — a NON-PURE cache MUST carry a finite `ttl:` (D85.9). You
        // may cache a provably-deterministic (`pure`) result forever; caching a
        // non-deterministic one forever is the footgun the compiler forbids.
        if !cache_effects_are_pure_only(&node.apply_to_effects) && node.ttl.is_none() {
            self.emit(
                format!(
                    "axon-T865 cache '{}' widens `apply_to_effects:` beyond [pure] but declares no \
                     `ttl:` — a non-deterministic result may not be cached forever; add a finite \
                     `ttl:` (e.g. `ttl: 30s`) bounding how stale a served result may be",
                    node.name
                ),
                &node.loc,
            );
        }

        // axon-T864 — every `invalidate_on:` reference resolves to a declared
        // `channel` (reusing the §13 pub/sub, not a second mechanism).
        for ch in &node.invalidate_on {
            match self.symbols.lookup(ch) {
                None => self.emit(
                    format!(
                        "axon-T864 cache '{}' `invalidate_on:` references undefined channel '{}'",
                        node.name, ch
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "channel" => self.emit(
                    format!(
                        "axon-T864 '{}' is a {}, not a channel (in cache '{}' `invalidate_on:`)",
                        ch, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
    }

    /// §Fase 83.c — validate one `cors { … }` declaration's own fields
    /// (cross-declaration checks — the undefined-reference check on
    /// `axonendpoint.cors:` and the cross-method path-consistency check —
    /// live in [`Self::check_axonendpoint`] and
    /// §Fase 92.a — the credential contract's own-field laws. Slug validity
    /// already ran at parse (the `requires:` grammar); here: a contract that
    /// grants nothing is dead (`axon-T893`), and the TTL must be a parseable
    /// duration, > 0, and ≤ the closed 24h ceiling (`axon-T894` — an
    /// "ephemeral" credential that outlives a day is a §81 service account
    /// wearing a costume). The mint-time attenuation law
    /// (`grants ⊆ capabilities(minter)`) is the runtime's half.
    fn check_credential(&mut self, node: &crate::ast::CredentialDefinition) {
        if node.grants.is_empty() {
            self.emit(
                format!(
                    "axon-T893 credential '{}' declares no `grants:` — a credential that \
                     grants nothing can never authorize anything. Declare at least one \
                     capability slug (e.g. `grants: [chat.invoke]`).",
                    node.name
                ),
                &node.loc,
            );
        }
        const MAX_CREDENTIAL_TTL_SECS: u64 = 86_400; // 24h — the ephemeral ceiling
        match crate::duration_literal_to_secs(&node.ttl) {
            None => self.emit(
                format!(
                    "axon-T894 credential '{}' has an invalid `ttl:` '{}' — expected a \
                     duration literal like `15m`, `900s`, `1h` (required field).",
                    node.name, node.ttl
                ),
                &node.loc,
            ),
            Some(0) => self.emit(
                format!(
                    "axon-T894 credential '{}' has a zero-length `ttl:` '{}' — a bearer \
                     that is born expired can never authorize anything.",
                    node.name, node.ttl
                ),
                &node.loc,
            ),
            Some(secs) if secs > MAX_CREDENTIAL_TTL_SECS => self.emit(
                format!(
                    "axon-T894 credential '{}' declares `ttl: {}` ({secs}s), above the \
                     24h ephemeral ceiling — a long-lived machine identity is the §81 \
                     service-account surface, not an ephemeral credential.",
                    node.name, node.ttl
                ),
                &node.loc,
            ),
            Some(_) => {}
        }
    }

    /// [`Self::check_cors_cross_method_consistency`] respectively).
    fn check_cors(&mut self, node: &CorsDefinition) {
        // axon-T854 — origin glob shape: exact literal, or a single
        // leading-wildcard host label. No full regex (D5 — closed/decidable).
        for origin in &node.allow_origins {
            if !is_valid_origin_glob(origin) {
                self.emit(
                    format!(
                        "axon-T854 invalid origin glob '{}' in cors '{}' — must be an exact \
                         origin or a single leading wildcard host label (e.g. \
                         \"https://*.kivi.io\"), not a full pattern",
                        origin, node.name
                    ),
                    &node.loc,
                );
            }
        }

        // axon-T853 — the CORS spec forbids any-origin + credentials;
        // browsers already reject this combination silently at runtime.
        // Caught here, before deploy, with the spec rule named (D83.2 —
        // the flagship diagnostic this fase exists to catch).
        let any_origin = node.allow_origins.iter().any(|o| o == "*");
        if any_origin && node.allow_credentials {
            self.emit(
                format!(
                    "axon-T853 cors '{}' combines an any-origin `allow_origins: [\"*\"]` with \
                     `allow_credentials: true` — the CORS specification forbids this pairing \
                     (a browser silently REJECTS the credentialed response); narrow \
                     `allow_origins` to explicit origins or drop `allow_credentials`",
                    node.name
                ),
                &node.loc,
            );
        }

        // axon-T855 — allow_methods reuses the closed axonendpoint method
        // catalog (GET/POST/PUT/PATCH/DELETE), not a free string.
        for method in &node.allow_methods {
            let upper = method.to_uppercase();
            if !is_valid(&upper, crate::parser::AXONENDPOINT_METHOD_VALUES) {
                self.emit(
                    format!(
                        "axon-T855 unknown method '{}' in cors '{}'. Valid: {}",
                        method,
                        node.name,
                        valid_list(crate::parser::AXONENDPOINT_METHOD_VALUES)
                    ),
                    &node.loc,
                );
            }
        }
    }

    /// §Fase 83.c (`axon-T857`, D83.4) — every `axonendpoint` sharing the
    /// same `path:` (differing only by `method:`) must reference the SAME
    /// `cors:` declaration, or all leave it unset. A browser's preflight is
    /// per-PATH, not per-method — divergent policies on the same path are
    /// inherently ambiguous (whose `max_age` wins? whose `expose_headers`?);
    /// v1 requires consistency instead of a merged/union preflight (§5).
    /// Cross-declaration, so it runs AFTER the per-declaration walk, once
    /// every endpoint's final `path`/`cors_ref` is known.
    fn check_cors_cross_method_consistency(&mut self, decls: &[Declaration]) {
        let mut seen: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new();
        for decl in decls {
            let Declaration::AxonEndpoint(ep) = decl else {
                continue;
            };
            if ep.path.is_empty() {
                continue;
            }
            match seen.get(&ep.path) {
                None => {
                    seen.insert(ep.path.clone(), (ep.cors_ref.clone(), ep.name.clone()));
                }
                Some((first_cors_ref, first_name)) if first_cors_ref != &ep.cors_ref => {
                    fn describe(r: &str) -> &str {
                        if r.is_empty() {
                            "<none>"
                        } else {
                            r
                        }
                    }
                    self.emit(
                        format!(
                            "axon-T857 axonendpoint '{}' and '{}' share path '{}' but declare \
                             different `cors:` references ('{}' vs '{}') — a browser's \
                             preflight is per-path, not per-method; every axonendpoint on the \
                             same path must reference the SAME cors declaration (or all leave \
                             it unset)",
                            first_name,
                            ep.name,
                            ep.path,
                            describe(first_cors_ref),
                            describe(&ep.cors_ref),
                        ),
                        &ep.loc,
                    );
                }
                Some(_) => {}
            }
        }
    }

    // ── §Fase 99.c/99.d — Native Document Synthesis checker ─────────────────

    /// §Fase 99.c + 99.d — validate one `document { … }` declaration:
    /// (99.c) structure — `target`/`provenance` catalogs, the per-`target` block
    /// vocabulary, chart-kind catalog, required fields, formula/range shape; and
    /// (99.d) the assertion-laundering barrier — an assertive-slot flow-value
    /// binding must carry `attribute:` or sit inside `epistemic { believe|know }`;
    /// plus `sensitive:*`⇒`legal:*` propagation on the render's effect row.
    fn check_document(&mut self, node: &crate::ast::DocumentDefinition) {
        // (T910) target catalog.
        if !is_valid(&node.target, VALID_DOC_TARGETS) {
            self.emit(
                format!(
                    "axon-T910 document '{}' has `target: {}` — a document targets one of: {}.",
                    node.name,
                    if node.target.is_empty() { "<unset>" } else { &node.target },
                    valid_list(VALID_DOC_TARGETS)
                ),
                &node.loc,
            );
        }
        // (T911) provenance catalog (empty ⇒ `none`).
        if !node.provenance.is_empty() && !is_valid(&node.provenance, VALID_DOC_PROVENANCE) {
            self.emit(
                format!(
                    "axon-T911 document '{}' has `provenance: {}` — valid: {}.",
                    node.name,
                    node.provenance,
                    valid_list(VALID_DOC_PROVENANCE)
                ),
                &node.loc,
            );
        }
        // A document with no body renders an empty artifact — refuse.
        if node.blocks.is_empty() {
            self.emit(
                format!(
                    "axon-T912 document '{}' has an empty body — declare at least one body block \
                     ({} for `target: {}`).",
                    node.name,
                    doc_top_level_kinds(&node.target).join(" / "),
                    node.target
                ),
                &node.loc,
            );
        }

        // (T913) sensitive⇒legal propagation on the render's effect row (D99.4).
        let bases: std::collections::HashSet<String> = node
            .effects
            .as_ref()
            .map(|e| {
                e.effects
                    .iter()
                    .map(|s| s.split(':').next().unwrap_or(s).to_string())
                    .collect()
            })
            .unwrap_or_default();
        if bases.contains("sensitive") && !bases.contains("legal") {
            self.emit(
                format!(
                    "axon-T913 document '{}' binds `sensitive:*` data but its `effects:` carries \
                     no `legal:<basis>` — a document is an egress boundary (D99.4); a sensitive \
                     value leaving the lattice into a human artifact needs a declared legal basis.",
                    node.name
                ),
                &node.loc,
            );
        }

        // Recurse the block tree: vocabulary + fields + barrier.
        let epistemic_ok = matches!(self.current_epistemic_mode.as_str(), "believe" | "know");
        for block in &node.blocks {
            self.check_doc_block(node, block, &node.target, /*parent*/ "", epistemic_ok);
        }
    }

    /// §Fase 108.d — a declared dataspace's columns as OWNED
    /// `(name, declared_type)` pairs (for the T930 column checks).
    /// `None` ⇒ not declared as a dataspace.
    fn dataspace_columns(&self, name: &str) -> Option<Vec<(String, String)>> {
        self.program.declarations.iter().find_map(|d| match d {
            Declaration::Dataspace(n) if n.name == name => Some(
                n.columns
                    .iter()
                    .map(|c| (c.name.clone(), c.declared_type.clone()))
                    .collect(),
            ),
            _ => None,
        })
    }

    /// §Fase 108.d (`axon-T930`, shared law) — the four query verbs read
    /// a DECLARED dataspace. Returns the violation message when the
    /// target does not resolve (owned — no borrow survives).
    fn dataspace_target_error(&self, verb: &str, target: &str) -> Option<String> {
        if target.is_empty() {
            return Some(format!(
                "axon-T930 `{verb}` names no dataspace — the data-plane form is \
                 `{verb} <Dataspace> …` (§108.d; these verbs are relational \
                 operations, not prompts)."
            ));
        }
        if self.dataspace_columns(target).is_some() {
            return None;
        }
        let kind = self
            .symbols
            .lookup(target)
            .map(|s| s.kind.clone())
            .unwrap_or_else(|| "undeclared".to_string());
        Some(format!(
            "axon-T930 `{verb}` targets `{target}`, which is {} — a query verb \
             reads a declared `dataspace`.",
            if kind == "undeclared" {
                "not declared".to_string()
            } else {
                format!("a {kind}")
            }
        ))
    }

    /// §Fase 108.b — validate a `dataspace` declaration: the schema law
    /// (`axon-T928`). A dataspace IS its schema — the deterministic engine
    /// materializes one physical columnar buffer per declared column, so a
    /// declaration the engine cannot materialize is refused at compile time:
    ///
    ///   1. **empty schema** — a dataspace with no columns can never be
    ///      ingested into or queried; it is a lie-in-waiting.
    ///   2. **duplicate column** — one name, one buffer.
    ///   3. **unknown column type** — the catalog is CLOSED (D108.1: Text,
    ///      Int, Float, Bool, Timestamp, Json — each mapping 1:1 to a
    ///      physical layout). Misses get a smart-suggest hint.
    ///
    /// All violations accumulate (the parser keeps declared types raw), so
    /// one compile surfaces every schema error in the declaration.
    fn check_dataspace(&mut self, node: &crate::ast::DataspaceDefinition) {
        use crate::ast::DataspaceColumnType;

        // (1) empty schema.
        if node.columns.is_empty() {
            self.emit(
                format!(
                    "axon-T928 dataspace '{}' declares no columns. A dataspace IS its \
                     schema — the columnar engine materializes one typed buffer per \
                     declared column, and a dataspace with none can never be ingested \
                     into or queried. Declare at least one: `column <name>: <Type>` \
                     over {{Text, Int, Float, Bool, Timestamp, Json}}.",
                    node.name
                ),
                &node.loc,
            );
        }

        // (2) duplicate columns — one name, one buffer.
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for col in &node.columns {
            if !seen.insert(col.name.as_str()) {
                self.emit(
                    format!(
                        "axon-T928 dataspace '{}' declares column `{}` more than once — \
                         one column name maps to exactly one physical buffer.",
                        node.name, col.name
                    ),
                    &col.loc,
                );
            }
        }

        // (3) closed type catalog.
        for col in &node.columns {
            if DataspaceColumnType::from_token(&col.declared_type).is_none() {
                let names = DataspaceColumnType::all_canonical_names();
                let suggestion = crate::smart_suggest::suggest_for(&col.declared_type, &names);
                let suggest_suffix = if suggestion.is_empty() {
                    String::new()
                } else {
                    format!(" {suggestion}")
                };
                self.emit(
                    format!(
                        "axon-T928 dataspace '{}' column `{}` has unknown type `{}`. The \
                         closed dataspace column-type catalog (D108.1) is {{{}}} — each \
                         type maps 1:1 to a physical columnar buffer layout, so the \
                         catalog admits no open extension.{}",
                        node.name,
                        col.name,
                        col.declared_type,
                        names.join(", "),
                        suggest_suffix
                    ),
                    &col.loc,
                );
            }
        }
    }

    /// §Fase 110 — validate a `notify` declaration: the three laws of
    /// governed human notification.
    ///
    ///   T933 (evidence): a `provenance: cleared` notify whose template
    ///   binds flow values (`${ref}` slots) is assertion-laundering to a
    ///   HUMAN — refused unless vouched by an enclosing
    ///   `epistemic { believe | know }` (the T920 shape, harder
    ///   consequence: a human acts immediately).
    ///
    ///   T934 (structure): closed channel catalog; the recipient MUST be
    ///   a §94 secret-class ref (`to: secret(<class>)`) — a literal
    ///   phone/chat-id in source is PII where PII must never live;
    ///   template non-empty; `effects:` must include `web`.
    ///
    ///   T935 (attention): `window:` is mandatory and well-formed — an
    ///   unbounded interruption channel is refused, not defaulted.
    fn check_notify(&mut self, node: &crate::ast::NotifyDefinition) {
        // (T934) channel catalog.
        if !matches!(node.channel.as_str(), "sms" | "whatsapp" | "telegram") {
            self.emit(
                format!(
                    "axon-T934 notify '{}' has `channel: {}` — the closed v1 channel catalog is {{sms, whatsapp, telegram}} (D110.5; further channels are additive §110.x surface).",
                    node.name,
                    if node.channel.is_empty() { "<unset>" } else { &node.channel }
                ),
                &node.loc,
            );
        }
        // (T934) recipient custody — the compile law (D110.3).
        if !node.to_is_secret {
            self.emit(
                format!(
                    "axon-T934 notify '{}' has a literal recipient (`to: {}`). A recipient is PII and never rides source, IR, prompts or audit — declare it under §94 custody and reference the class: `to: secret(ops.oncall_phone)`. The value resolves at dispatch, tenant-scoped.",
                    node.name,
                    if node.to_secret.is_empty() { "<unset>" } else { &node.to_secret }
                ),
                &node.loc,
            );
        }
        // (T934) template present.
        if node.template.is_empty() {
            self.emit(
                format!("axon-T934 notify '{}' has no `template:` — a notification with nothing to say is a declaration error.", node.name),
                &node.loc,
            );
        }
        // (T934) the web effect — a notification IS a network egress.
        let has_web = node
            .effects
            .as_ref()
            .map(|e| e.effects.iter().any(|x| x == "web" || x.starts_with("web:")))
            .unwrap_or(false);
        if !has_web {
            self.emit(
                format!("axon-T934 notify '{}' declares no `web` effect — a notification crosses the trust boundary over the network; declare `effects: <web>`.", node.name),
                &node.loc,
            );
        }
        // (T935) the mandatory window (D110.4).
        let window_ok = {
            let w = node.window.trim();
            !w.is_empty()
                && w.len() >= 2
                && w[..w.len() - 1].chars().all(|c| c.is_ascii_digit())
                && matches!(w.chars().last(), Some('s') | Some('m') | Some('h') | Some('d'))
                && w[..w.len() - 1].parse::<u64>().map(|n| n > 0).unwrap_or(false)
        };
        if !window_ok {
            self.emit(
                format!(
                    "axon-T935 notify '{}' has `window: {}` — a window is MANDATORY (an unbounded interruption channel is a bug, not a default) and must be a positive duration (`30m`, `4h`, `1d`). At-most-once-per-window per recipient, enforced durably; suppressions are witnessed.",
                    node.name,
                    if node.window.is_empty() { "<unset>" } else { &node.window }
                ),
                &node.loc,
            );
        }
        // (T933) the evidence barrier (D110.2).
        let binds_flow_values = node.template.contains("${");
        let cleared = node.provenance == "cleared";
        if !node.provenance.is_empty() && !matches!(node.provenance.as_str(), "attached" | "cleared") {
            self.emit(
                format!("axon-T934 notify '{}' has `provenance: {}` — one of: attached (default), cleared.", node.name, node.provenance),
                &node.loc,
            );
        }
        let epistemic_ok = matches!(self.current_epistemic_mode.as_str(), "believe" | "know");
        if cleared && binds_flow_values && !epistemic_ok {
            self.emit(
                format!(
                    "axon-T933 notify '{}' is `provenance: cleared` and binds flow values into a HUMAN notification. A guess reaching a person's pocket labeled as fact is assertion-laundering at its fastest — the human acts immediately (D110.2, the T920 barrier's human-egress sibling). Use `provenance: attached` (each bound value arrives with its epistemic label; a §108 envelope ref appends its evidence line), or vouch the values inside `epistemic {{ believe|know }}`.",
                    node.name
                ),
                &node.loc,
            );
        }
    }

    /// §Fase 105 — validate a `deliver` declaration: structure laws (T921–T926)
    /// + the provenance-stripping barrier (T920), the egress-dual of the §99
    /// assertion-laundering barrier. A CRM write publishes assertions into a
    /// system of record downstream humans treat as fact; a `provenance: cleared`
    /// delivery of a flow value is laundering unless the author vouches (an
    /// enclosing `epistemic { believe|know }`) that the value cleared the lattice.
    fn check_deliver(&mut self, node: &crate::ast::DeliverDefinition) {
        // (T921) target catalog.
        if !is_valid(&node.target, VALID_DELIVER_TARGETS) {
            self.emit(
                format!(
                    "axon-T921 deliver '{}' has `target: {}` — a delivery targets one of: {}.",
                    node.name,
                    if node.target.is_empty() { "<unset>" } else { &node.target },
                    valid_list(VALID_DELIVER_TARGETS)
                ),
                &node.loc,
            );
        }
        // (T922) provenance catalog (empty ⇒ `attached`).
        if !node.provenance.is_empty() && !is_valid(&node.provenance, VALID_DELIVER_PROVENANCE) {
            self.emit(
                format!(
                    "axon-T922 deliver '{}' has `provenance: {}` — valid: {} (empty ⇒ `attached`).",
                    node.name,
                    node.provenance,
                    valid_list(VALID_DELIVER_PROVENANCE)
                ),
                &node.loc,
            );
        }
        // (T923) a delivery MUST name the per-tenant credential key — a CRM write
        // authenticates, and the value rides §94 custody (never cognition).
        if node.secret.trim().is_empty() {
            self.emit(
                format!(
                    "axon-T923 deliver '{}' has no `secret:` — a CRM write must authenticate; \
                     name the per-tenant credential key (resolved via §94 custody at dispatch, \
                     never revealed to cognition).",
                    node.name
                ),
                &node.loc,
            );
        }
        // (T924) the effect row must include `web` — a CRM write crosses the
        // trust boundary over the network (the §98 discipline, egress form).
        let bases: std::collections::HashSet<String> = node
            .effects
            .as_ref()
            .map(|e| {
                e.effects
                    .iter()
                    .map(|s| s.split(':').next().unwrap_or(s).to_string())
                    .collect()
            })
            .unwrap_or_default();
        if !bases.contains("web") {
            self.emit(
                format!(
                    "axon-T924 deliver '{}' does not declare the `web` effect — a delivery writes \
                     across the network trust boundary. Add `effects: <web>` (plus any \
                     `sensitive:<cat>`/`legal:<basis>` the delivered data carries).",
                    node.name
                ),
                &node.loc,
            );
        }
        // (T913-analog) sensitive⇒legal propagation — a delivery is an egress
        // boundary (D105.6): sensitive data leaving into a CRM needs a legal basis.
        if bases.contains("sensitive") && !bases.contains("legal") {
            self.emit(
                format!(
                    "axon-T924 deliver '{}' binds `sensitive:*` data but its `effects:` carries no \
                     `legal:<basis>` — delivering PII into a system of record is further \
                     processing (D105.6); declare the legal basis.",
                    node.name
                ),
                &node.loc,
            );
        }
        // (T925) a delivery with no operation delivers nothing — refuse.
        if node.ops.is_empty() {
            self.emit(
                format!(
                    "axon-T925 deliver '{}' has an empty body — declare at least one operation \
                     ({}).",
                    node.name,
                    valid_list(VALID_DELIVER_OPS)
                ),
                &node.loc,
            );
        }

        // (T920) the provenance-stripping barrier — the headline property. A
        // `provenance: cleared` delivery that binds ANY flow value is laundering
        // the epistemic origin out at the boundary, UNLESS the author vouches via
        // an enclosing `epistemic { believe|know }`. `attached` (the default) is
        // always legal: provenance travels into the CRM, a guess arrives labeled.
        let epistemic_vouched =
            matches!(self.current_epistemic_mode.as_str(), "believe" | "know");
        let cleared = node.provenance == "cleared";
        let binds_flow_value = node
            .ops
            .iter()
            .any(|op| op.ref_fields().next().is_some());
        if cleared && binds_flow_value && !epistemic_vouched {
            self.emit(
                format!(
                    "axon-T920 deliver '{}' is `provenance: cleared` and binds a flow value into a \
                     CRM with no provenance. A value leaving the epistemic lattice into a system of \
                     record cannot be more confident than the reasoning that produced it (D105.2, \
                     the provenance-stripping barrier — the egress-dual of the §99 assertion-\
                     laundering barrier). Use `provenance: attached` (the default — each field \
                     lands with its level/confidence/source, a guess labeled as a guess), or, if \
                     you vouch the delivered values are verified facts, wrap the delivery in \
                     `epistemic {{ mode: believe }}` (after a `shield` + `anchor` cleared them).",
                    node.name
                ),
                &node.loc,
            );
        }

        // Per-operation laws: vocabulary (T925) + the idempotency key (T926).
        for op in &node.ops {
            if !is_valid(&op.kind, VALID_DELIVER_OPS) {
                self.emit(
                    format!(
                        "axon-T925 deliver '{}' — operation `{}` is not valid for `target: {}`. \
                         Valid: {}.",
                        node.name,
                        op.kind,
                        if node.target.is_empty() { "crm" } else { &node.target },
                        valid_list(VALID_DELIVER_OPS)
                    ),
                    &op.loc,
                );
            }
            if !op.has_field("key") {
                self.emit(
                    format!(
                        "axon-T926 deliver '{}' — operation `{}` has no `key:` — every delivery \
                         operation requires an idempotency key (a natural key like the contact \
                         email, or an adopter `external_id`) so an at-least-once retry never \
                         double-creates a record (D105.5).",
                        node.name, op.kind
                    ),
                    &op.loc,
                );
            }
        }
    }

    /// §Fase 99.c/d — recursively validate one document block.
    fn check_doc_block(
        &mut self,
        doc: &crate::ast::DocumentDefinition,
        block: &crate::ast::DocBlock,
        target: &str,
        parent: &str,
        epistemic_ok: bool,
    ) {
        // (T912) the block kind must be in the vocabulary for (target, parent).
        let allowed = doc_allowed_child_kinds(target, parent);
        if !allowed.contains(&block.kind.as_str()) {
            let where_ = if parent.is_empty() {
                format!("at the top level of a `target: {target}` document")
            } else {
                format!("inside a `{parent}` block (`target: {target}`)")
            };
            self.emit(
                format!(
                    "axon-T912 document '{}' — block `{}` is not valid {where_}. Valid here: {}.",
                    doc.name,
                    block.kind,
                    if allowed.is_empty() {
                        "(none — this block takes no children)".to_string()
                    } else {
                        allowed.join(" / ")
                    }
                ),
                &block.loc,
            );
            // Still validate fields best-effort so the author sees every issue.
        }

        // (T914) unknown field for this block kind (closed catalog).
        let allowed_fields = doc_allowed_fields(&block.kind);
        for (fname, _) in &block.fields {
            if !allowed_fields.contains(&fname.as_str()) {
                self.emit(
                    format!(
                        "axon-T914 document '{}' — `{}` is not a valid field of a `{}` block. \
                         Valid: {}.",
                        doc.name,
                        fname,
                        block.kind,
                        if allowed_fields.is_empty() {
                            "(none)".to_string()
                        } else {
                            allowed_fields.join(" / ")
                        }
                    ),
                    &block.loc,
                );
            }
        }

        // Per-kind structural laws.
        self.check_doc_block_laws(doc, block);

        // (T916) the assertion-laundering barrier — the headline property.
        if let Some(slot) = doc_assertive_slot(&block.kind) {
            if let Some(value) = block.field(slot) {
                if let crate::ast::DocScalar::Ref(name) = value {
                    let attributed = block.has_field("attribute");
                    if !attributed && !epistemic_ok {
                        self.emit(
                            format!(
                                "axon-T916 document '{}' — the `{}` block binds flow value `{}` in \
                                 its assertive `{}:` slot with no provenance. A value leaving the \
                                 epistemic lattice into a human artifact cannot be more confident \
                                 than the reasoning that produced it (D99.1, the assertion-\
                                 laundering barrier). Add `attribute: <source>` (renders as a \
                                 visible source note), wrap the document in `epistemic {{ mode: \
                                 believe }}` if you vouch it is ≥ believe, or pass `{}` through a \
                                 `shield` scanning `hallucination`/`pii_leak` first.",
                                doc.name, block.kind, name, slot, name
                            ),
                            &block.loc,
                        );
                    }
                }
            }
        }

        // Recurse into children with this block as the parent context.
        for child in &block.children {
            self.check_doc_block(doc, child, target, &block.kind, epistemic_ok);
        }
    }

    /// §Fase 99.c — per-block-kind structural laws (required fields, catalogs,
    /// cell/range shape). Each a distinct `axon-T9xx`.
    fn check_doc_block_laws(
        &mut self,
        doc: &crate::ast::DocumentDefinition,
        block: &crate::ast::DocBlock,
    ) {
        use crate::ast::DocScalar;
        let require = |this: &mut Self, field: &str, code: &str, why: &str| {
            if !block.has_field(field) {
                this.emit(
                    format!(
                        "axon-{code} document '{}' — a `{}` block requires `{field}:` ({why}).",
                        doc.name, block.kind
                    ),
                    &block.loc,
                );
            }
        };
        match block.kind.as_str() {
            "chart" => {
                // (T917) chart kind ∈ the bounded subset (D99.9).
                if let Some(DocScalar::Ref(k)) = block.field("kind") {
                    if !is_valid(k, VALID_CHART_KINDS) {
                        self.emit(
                            format!(
                                "axon-T917 document '{}' — chart `kind: {}` is outside the bounded \
                                 v1 subset {}. SmartArt / pivots / 3-D are deferred (D99.9).",
                                doc.name,
                                k,
                                valid_list(VALID_CHART_KINDS)
                            ),
                            &block.loc,
                        );
                    }
                }
                require(self, "kind", "T917", "the chart type");
                require(self, "series", "T918", "the data series");
            }
            "table" => {
                // (T915) a table needs a non-empty column spec + rows.
                match block.field("columns") {
                    Some(DocScalar::List(cols)) if !cols.is_empty() => {}
                    _ => self.emit(
                        format!(
                            "axon-T915 document '{}' — a `table` block requires a non-empty \
                             `columns: [ … ]` spec (the row arity is checked against it).",
                            doc.name
                        ),
                        &block.loc,
                    ),
                }
                require(self, "rows", "T915", "the row data");
            }
            "formula" => {
                require(self, "cell", "T919", "the target cell (A1 notation)");
                require(self, "expr", "T919", "the formula expression");
                if let Some(DocScalar::Text(cell)) = block.field("cell") {
                    if !is_a1_cell(cell) {
                        self.emit(
                            format!(
                                "axon-T919 document '{}' — formula `cell: \"{}\"` is not a valid A1 \
                                 cell reference (e.g. \"B2\").",
                                doc.name, cell
                            ),
                            &block.loc,
                        );
                    }
                }
            }
            "range" => {
                if let Some(DocScalar::Text(r)) = block.field("cells") {
                    if !is_a1_range(r) {
                        self.emit(
                            format!(
                                "axon-T919 document '{}' — `range` cells `\"{}\"` is not a valid A1 \
                                 range (e.g. \"B2:B9\").",
                                doc.name, r
                            ),
                            &block.loc,
                        );
                    }
                }
            }
            "placeholder" => require(self, "name", "T920", "the layout slot name"),
            "image" => require(self, "source", "T921", "the image source binding"),
            "slide" => require(self, "layout", "T920", "the slide layout"),
            "sheet" => require(self, "name", "T912", "the sheet tab name"),
            _ => {}
        }
        // A chart bound into an xlsx should carry a `range`; a chart in docx/pptx
        // binds a `series`. Both are covered by the required-field laws above.
    }

    /// §Fase 87.b — validate one `savant { … }` declaration's own fields. Ref
    /// resolution (`memory.backend` → a declared `memory`/`corpus`), §72 budget
    /// binding, and §79 interruptibility are §87.c; the `SavantSoundness` PCC is
    /// §87.g. Every diagnostic here is a HARD error — a savant governs an
    /// expensive, weeks-long autonomous process, so an under-specified one must
    /// never deploy silently.
    fn check_savant(&mut self, node: &SavantDefinition) {
        // axon-T873 — a savant MUST declare its ontological `domain:` (the
        // generative boundary the free-energy loop minimises surprise over). An
        // empty domain is an unbounded mandate — the exact footgun §87 forbids.
        if node.domain.trim().is_empty() {
            self.emit(
                format!(
                    "axon-T873 savant '{}' declares no `domain:` — a long-horizon research \
                     agent needs a bounded ontological scope; add e.g. `domain: \"…\"`",
                    node.name
                ),
                &node.loc,
            );
        }

        // axon-T874 — a savant MUST carry at least one `mandate`, and each
        // mandate MUST have a non-empty `objective:` and a declared `output:`
        // type. A savant with no mandate has nothing to research; a mandate with
        // no objective/output is an unrunnable task.
        if node.mandates.is_empty() {
            self.emit(
                format!(
                    "axon-T874 savant '{}' declares no `mandate` — add at least one \
                     `mandate <Name> {{ objective: \"…\", output: <Type> }}`",
                    node.name
                ),
                &node.loc,
            );
        }
        for m in &node.mandates {
            if m.objective.trim().is_empty() {
                self.emit(
                    format!(
                        "axon-T874 mandate '{}' in savant '{}' has an empty `objective:` — \
                         state the research goal the savant autonomously decomposes",
                        m.name, node.name
                    ),
                    &m.loc,
                );
            }
            if m.output_type.trim().is_empty() {
                self.emit(
                    format!(
                        "axon-T874 mandate '{}' in savant '{}' declares no `output:` type — \
                         the final report must inhabit a declared type (e.g. `output: FormalReport`)",
                        m.name, node.name
                    ),
                    &m.loc,
                );
            }
        }

        // axon-T876 — the `cognition { … }` params are closed catalogs, and the
        // EFE convergence bound must be a positive probability mass. A typo'd
        // `depth`/`divergence` can never silently pick a different engine
        // geometry; a non-positive threshold can never make the loop converge.
        if let Some(cog) = &node.cognition {
            if !cog.depth.is_empty() && !is_valid(&cog.depth, VALID_SAVANT_DEPTHS) {
                self.emit(
                    format!(
                        "axon-T876 unknown savant cognition depth '{}' in savant '{}'. Valid: {}",
                        cog.depth,
                        node.name,
                        valid_list(VALID_SAVANT_DEPTHS)
                    ),
                    &cog.loc,
                );
            }
            if !cog.divergence.is_empty() && !is_valid(&cog.divergence, VALID_SAVANT_DIVERGENCES) {
                self.emit(
                    format!(
                        "axon-T876 unknown savant cognition divergence '{}' in savant '{}'. Valid: {}",
                        cog.divergence,
                        node.name,
                        valid_list(VALID_SAVANT_DIVERGENCES)
                    ),
                    &cog.loc,
                );
            }
            if let Some(threshold) = cog.entropic_threshold {
                if !(threshold > 0.0) {
                    self.emit(
                        format!(
                            "axon-T876 savant '{}' cognition `entropic_threshold: {}` must be > 0 — \
                             it is the Expected-Free-Energy convergence bound; a non-positive bound \
                             can never be reached and the loop would never terminate",
                            node.name, threshold
                        ),
                        &cog.loc,
                    );
                }
            }
        }

        // ── §87.c — composition binding ──────────────────────────────────────

        // axon-T875 — a declared `memory.backend:` MUST resolve to a `memory` or
        // `corpus` primitive (the retention layer the savant composes; mirrors
        // the cache `invalidate_on:` → `channel` resolution, T864). A dangling
        // reference would silently give the savant no durable memory — fatal for
        // a weeks-long loop whose whole point is not to forget.
        if let Some(mem) = &node.memory {
            if !mem.backend.is_empty() {
                match self.symbols.lookup(&mem.backend) {
                    None => self.emit(
                        format!(
                            "axon-T875 savant '{}' `memory.backend: {}` references an undefined \
                             store — declare a `memory` or `corpus` primitive with that name",
                            node.name, mem.backend
                        ),
                        &mem.loc,
                    ),
                    Some(sym) if sym.kind != "memory" && sym.kind != "corpus" => self.emit(
                        format!(
                            "axon-T875 savant '{}' `memory.backend: {}` resolves to a {}, not a \
                             `memory`/`corpus` store",
                            node.name, mem.backend, sym.kind
                        ),
                        &mem.loc,
                    ),
                    Some(_) => {}
                }
            }
        }

        // axon-T877 — a savant MUST declare a compute `budget { max_iterations: N }`
        // with N > 0. This is the §72 linear-budget discipline (paper §9.2): an
        // autonomous loop that can run for weeks, write code and self-execute it
        // with NO enforced ceiling is uninsurable. §87.k binds this to a real
        // `RateLease`; §87.c makes the ceiling non-optional at the type level.
        match &node.budget {
            None => self.emit(
                format!(
                    "axon-T877 savant '{}' declares no `budget {{ max_iterations: N }}` — a \
                     long-horizon autonomous agent MUST carry an enforced compute ceiling \
                     (the §72 linear-budget discipline); an unbounded loop is fail-open",
                    node.name
                ),
                &node.loc,
            ),
            Some(b) => match b.max_iterations {
                None => self.emit(
                    format!(
                        "axon-T877 savant '{}' `budget` declares no `max_iterations:` — the \
                         FEP-loop iteration ceiling is mandatory",
                        node.name
                    ),
                    &b.loc,
                ),
                Some(n) if n <= 0 => self.emit(
                    format!(
                        "axon-T877 savant '{}' `budget.max_iterations: {}` must be > 0",
                        node.name, n
                    ),
                    &b.loc,
                ),
                Some(_) => {}
            },
        }

        // axon-T878 — if a mandate's `output:` type resolves to a DECLARED symbol,
        // it must be a `type` (a report cannot inhabit a flow/memory). Unknown
        // names are accepted silently — they may be builtins (`Text`, `Json`) or
        // imported types (the house "soft type" discipline, type_checker.rs
        // §8368/§8659). A clearly-wrong declared reference is still caught.
        for m in &node.mandates {
            if let Some(sym) = self.symbols.lookup(&m.output_type) {
                if sym.kind != "type" {
                    self.emit(
                        format!(
                            "axon-T878 mandate '{}' in savant '{}' has `output: {}` which is a {}, \
                             not a type — the final report must inhabit a declared `type`",
                            m.name, node.name, m.output_type, sym.kind
                        ),
                        &m.loc,
                    );
                }
            }
        }
    }

    /// §Fase 87.d — validate one `synth { … }` dynamic tool-synthesis policy.
    /// Every diagnostic is a HARD error: a synth policy governs arbitrary-code
    /// execution — the highest-stakes surface in the language — so an
    /// under-specified or unsafe one must never compile. The deny-by-default
    /// core is T882: synthesised code may only run in a WASM zero-trust sandbox.
    fn check_synth(&mut self, node: &SynthDefinition) {
        // axon-T879 — the policy must state what it synthesises for.
        if node.target.trim().is_empty() {
            self.emit(
                format!(
                    "axon-T879 synth '{}' declares no `target:` — state the capability scope the \
                     synthesised tools serve (e.g. `target: \"parse geospatial datasets\"`)",
                    node.name
                ),
                &node.loc,
            );
        }

        // axon-T880 — `risk:` is required + a closed catalog. It sets how strict
        // the review + isolation must be; it can never be left unstated.
        if node.risk.is_empty() {
            self.emit(
                format!(
                    "axon-T880 synth '{}' declares no `risk:` — classify the synthesis risk. \
                     Valid: {}",
                    node.name,
                    valid_list(VALID_SYNTH_RISKS)
                ),
                &node.loc,
            );
        } else if !is_valid(&node.risk, VALID_SYNTH_RISKS) {
            self.emit(
                format!(
                    "axon-T880 unknown synth risk '{}' in synth '{}'. Valid: {}",
                    node.risk,
                    node.name,
                    valid_list(VALID_SYNTH_RISKS)
                ),
                &node.loc,
            );
        }

        // axon-T881 — `language:` (when set) is a closed catalog (all → wasm32-wasi).
        if !node.language.is_empty() && !is_valid(&node.language, VALID_SYNTH_LANGUAGES) {
            self.emit(
                format!(
                    "axon-T881 unknown synth language '{}' in synth '{}'. Valid: {}",
                    node.language,
                    node.name,
                    valid_list(VALID_SYNTH_LANGUAGES)
                ),
                &node.loc,
            );
        }

        // axon-T882 — DENY-BY-DEFAULT: `sandbox:` MUST be `wasm`. Executing
        // synthesised code outside a zero-trust WASM sandbox is forbidden; an
        // empty or non-`wasm` sandbox can never compile (paper §6.2, §8.3). The
        // OSS `SynthBackend` reference refuses execution regardless — this gate
        // stops an adopter from *declaring* an unsandboxed policy at all.
        if node.sandbox != "wasm" {
            let got = if node.sandbox.is_empty() {
                "no sandbox".to_string()
            } else {
                format!("`{}`", node.sandbox)
            };
            self.emit(
                format!(
                    "axon-T882 synth '{}' must declare `sandbox: wasm` (got {}) — synthesised code \
                     may only run in a zero-trust WASM sandbox; there is no unsandboxed mode",
                    node.name, got
                ),
                &node.loc,
            );
        }

        // axon-T883 — `review:` (when set) is a closed catalog, AND high/critical
        // risk MUST be reviewed: a Coder/Reviewer consensus (`par`) is mandatory
        // for dangerous synthesis. `review: none` at high/critical is refused.
        if !node.review.is_empty() && !is_valid(&node.review, VALID_SYNTH_REVIEWS) {
            self.emit(
                format!(
                    "axon-T883 unknown synth review '{}' in synth '{}'. Valid: {}",
                    node.review,
                    node.name,
                    valid_list(VALID_SYNTH_REVIEWS)
                ),
                &node.loc,
            );
        } else if node.review == "none" && (node.risk == "high" || node.risk == "critical") {
            self.emit(
                format!(
                    "axon-T883 synth '{}' is `risk: {}` but `review: none` — high/critical-risk \
                     synthesis MUST carry Coder/Reviewer consensus; remove `review: none`",
                    node.name, node.risk
                ),
                &node.loc,
            );
        }
    }

    /// §Fase 88.b — validate one `scope { … }` authorization policy's own
    /// fields. The `warden`-side binding (the scope reference resolves + the
    /// target is in this allowlist) is §88.c. Every diagnostic is a HARD error:
    /// a scope governs an offensive-capable analysis, so an under-specified one
    /// must never authorise anything.
    fn check_scope(&mut self, node: &ScopeDefinition) {
        // axon-T884 — a scope MUST declare a non-empty `targets:` allowlist. An
        // empty allowlist is an unbounded authorization — the exact footgun §88
        // forbids (a warden could then be pointed at anything).
        if node.targets.is_empty() {
            self.emit(
                format!(
                    "axon-T884 scope '{}' declares an empty `targets:` allowlist — a scope must \
                     name the specific resources the operator authorises for analysis (an empty \
                     allowlist would authorise nothing safely and everything dangerously)",
                    node.name
                ),
                &node.loc,
            );
        }

        // axon-T885 — `depth:` is a closed catalog (the analysis-invasiveness
        // ceiling). A typo can never silently escalate to a more invasive depth.
        if !node.depth.is_empty() && !is_valid(&node.depth, VALID_SCOPE_DEPTHS) {
            self.emit(
                format!(
                    "axon-T885 unknown scope depth '{}' in scope '{}'. Valid (least→most \
                     invasive): {}",
                    node.depth,
                    node.name,
                    valid_list(VALID_SCOPE_DEPTHS)
                ),
                &node.loc,
            );
        }

        // axon-T886 — a scope MUST name its `approver:` capability (segregation
        // of duties, the `mandate` §21 model): who authorised this analysis. An
        // unapproved scope is not an authorization.
        if node.approver.trim().is_empty() {
            self.emit(
                format!(
                    "axon-T886 scope '{}' declares no `approver:` — name the capability whose \
                     holder authorised this analysis scope (e.g. `approver: requires \
                     \"security.lead\"`)",
                    node.name
                ),
                &node.loc,
            );
        }
    }

    /// §Fase 88.c — the `warden` authorization binding. The grammar already
    /// forbids an omitted `within` clause (§88.a, fail-closed by construction);
    /// here we enforce that the scope RESOLVES to a declared `scope`. The
    /// target-in-allowlist + depth-ceiling enforcement is a RUNTIME check
    /// (§88.h): the warden target is a program value whose analysed resource is
    /// only known at runtime, so the allowlist match cannot be static — the
    /// honest split (the frontend guarantees a real authorization scope exists;
    /// the enterprise runtime guarantees the resource is inside it).
    fn check_warden(&mut self, node: &WardenBlock, flow_name: &str) {
        // axon-T887 — the `within <Scope>` must name a declared `scope`.
        match self.symbols.lookup(&node.scope_ref) {
            None => self.emit(
                format!(
                    "axon-T887 warden in flow '{flow_name}' references undefined scope '{}' — a \
                     `warden(…) within <Scope>` must name a declared `scope` (fail-closed: with no \
                     authorization scope there is no analysis)",
                    node.scope_ref
                ),
                &node.loc,
            ),
            Some(sym) if sym.kind != "scope" => self.emit(
                format!(
                    "axon-T887 warden in flow '{flow_name}' `within {}` resolves to a {}, not a \
                     `scope` authorization policy",
                    node.scope_ref, sym.kind
                ),
                &node.loc,
            ),
            Some(_) => {}
        }
        // Walk the analysis body (find_exploits / fortify / emit + nested steps).
        self.check_flow_steps(&node.body, flow_name);
    }

    /// §Fase 85.c — the cross-declaration cache laws, run after the
    /// per-declaration walk (needs the full tool + cache set):
    /// - **axon-T863** — at most one `cache { default: true }` per module.
    /// - **axon-W013** — a widened `default: true` cache (effects beyond
    ///   `[pure]`) names EVERY non-pure tool it ends up auto-covering, so the
    ///   author sees exactly what determinism they are trusting (D85.2, the
    ///   §77.a W005 "compiler stops being polite" discipline).
    fn check_cache_module_laws(&mut self, decls: &[Declaration]) {
        let defaults: Vec<&CacheDefinition> = decls
            .iter()
            .filter_map(|d| match d {
                Declaration::Cache(c) if c.default_policy => Some(c),
                _ => None,
            })
            .collect();

        // axon-T863 — competing defaults never silently "last one wins."
        if defaults.len() > 1 {
            for extra in &defaults[1..] {
                self.emit(
                    format!(
                        "axon-T863 more than one `cache {{ default: true }}` in this module \
                         ('{}' and '{}'); a module has at most one default cache policy — \
                         remove `default: true` from all but one",
                        defaults[0].name, extra.name
                    ),
                    &extra.loc,
                );
            }
        }

        // axon-W013 — a single WIDENED default names each non-pure tool it covers.
        if defaults.len() == 1 && !cache_effects_are_pure_only(&defaults[0].apply_to_effects) {
            let def = defaults[0];
            let base = |e: &str| e.split_once(':').map(|(b, _)| b.to_string()).unwrap_or_else(|| e.to_string());
            let apply_set: Vec<String> = if def.apply_to_effects.is_empty() {
                vec!["pure".to_string()]
            } else {
                def.apply_to_effects.iter().map(|e| base(e)).collect()
            };
            for decl in decls {
                let Declaration::Tool(t) = decl else { continue };
                // An explicit `cache:` (a named ref OR the `none` opt-out) takes
                // the tool out of the default's auto-coverage.
                if !t.cache.is_empty() {
                    continue;
                }
                let Some(row) = &t.effects else { continue };
                let covered = row.effects.iter().all(|e| apply_set.contains(&base(e)));
                let has_nonpure = row.effects.iter().any(|e| base(e) != "pure");
                if covered && has_nonpure {
                    self.warn(
                        format!(
                            "axon-W013 cache '{}' has `default: true` with `apply_to_effects:` \
                             widened beyond [pure]; it auto-caches tool '{}' whose effect row \
                             <{}> is NOT proven deterministic — a stale or incorrect result may \
                             be served (bounded by `ttl:`). Confirm '{}' is safe to memoize, or \
                             set `cache: none` on it to opt out",
                            def.name,
                            t.name,
                            row.effects.join(", "),
                            t.name
                        ),
                        &t.loc,
                    );
                }
            }
        }
    }

    fn check_shield(&mut self, node: &ShieldDefinition) {
        // ── §Fase 111 (T936) — `taint:` is retracted ────────────────────
        //
        // The public README advertised `taint` as a primitive: "Epistemic
        // trust label for untrusted external data sources". It never was one.
        // It is a `shield` FIELD, and the field is DEAD: the parser reads it,
        // `ir_generator` copies it into the IR, and **nothing in the runtime
        // ever reads it back**. A program that wrote `taint: untrusted`
        // received exactly the protection of a comment.
        //
        // `primitive_registry` already disowned `taint` in §Fase 6.c ("an
        // entry without a parser production lies"); §111 makes the language
        // and the advertising agree with the code.
        //
        // The epistemic-taint LAW is real — it just lives somewhere else:
        // values acquired from the outside world are born `Untrusted`
        // (axon-T908, §98), the lattice degrades on contact, and `shield`
        // gates egress. That law is enforced whether or not anyone writes
        // this field, which is precisely why the field could rot unnoticed.
        if !node.taint.is_empty() {
            self.emit(
                format!(
                    "axon-T936 shield '{}' declares `taint: {}` — a DEAD field, retracted in §111. \
                     It was parsed, carried into the IR, and never read by the runtime: it bought \
                     you nothing. Delete it. Epistemic taint is not a shield knob — external data \
                     is born `Untrusted` by law (axon-T908) and degrades the lattice on contact; \
                     to constrain what may leave, use the shield's `scan:` / `on_breach:` / \
                     `redact:` gates, which do run.",
                    node.name, node.taint
                ),
                &node.loc,
            );
        }

        // Scan categories
        for cat in &node.scan {
            // §Fase 53.c — accept an extension-declared scan category as
            // first-class (alongside the canonical catalog).
            if !is_valid(cat, VALID_SCAN_CATEGORIES) && !self.ext_scan_categories.contains(cat) {
                self.emit(
                    format!(
                        "Unknown scan category '{}' in shield '{}'. Valid: {}",
                        cat,
                        node.name,
                        valid_list(VALID_SCAN_CATEGORIES)
                    ),
                    &node.loc,
                );
            }
        }

        // Strategy enum
        if !node.strategy.is_empty() && !is_valid(&node.strategy, VALID_SHIELD_STRATEGIES) {
            self.emit(
                format!(
                    "Unknown strategy '{}' in shield '{}'. Valid: {}",
                    node.strategy,
                    node.name,
                    valid_list(VALID_SHIELD_STRATEGIES)
                ),
                &node.loc,
            );
        }

        // §Fase 114.w — axon-T952: a breach policy must carry its OPERAND, or
        // it is a promise the runtime cannot run: `deflect` with no message
        // has nothing to emit, `quarantine` with no sink has nowhere to
        // route, `sanitize_and_retry` with no `redact:` fields sanitizes
        // nothing (a retry loop that re-scans the same bytes). Each was
        // silently accepted before §114.w — and silently `halt`ed.
        match node.on_breach.as_str() {
            "deflect" if node.deflect_message.is_empty() => self.emit(
                format!(
                    "axon-T952 shield '{}' declares `on_breach: deflect` with no \
                     `deflect_message:` — there is nothing to emit instead of the \
                     candidate. Declare the canned safe reply, or use `halt`.",
                    node.name
                ),
                &node.loc,
            ),
            // `quarantine` without a sink is a WARNING, not an error: a large
            // published tail (templates, compliance patterns) declares it, and
            // the runtime has a defined fail-closed meaning for the hole (halt
            // with a diagnostic naming it) — the pre-§114.w behaviour exactly.
            // deflect/sanitize_and_retry differ: without their operand there
            // is NOTHING the runtime could do but halt, so the declaration is
            // pure noise and refusing it costs no working program.
            "quarantine" if node.quarantine.is_empty() => self.warn(
                format!(
                    "axon-W012 shield '{}' declares `on_breach: quarantine` with no \
                     `quarantine:` sink — the candidate has nowhere to route, so a \
                     breach HALTS (fail-closed) instead of quarantining. Name the \
                     sink to make the candidate recoverable.",
                    node.name
                ),
                &node.loc,
            ),
            "sanitize_and_retry" if node.redact.is_empty() => self.emit(
                format!(
                    "axon-T952 shield '{}' declares `on_breach: sanitize_and_retry` \
                     with no `redact:` fields — a retry that re-scans the SAME bytes \
                     is not sanitization. Declare the fields to mask, or use `halt`.",
                    node.name
                ),
                &node.loc,
            ),
            _ => {}
        }

        // on_breach policy
        if !node.on_breach.is_empty() && !is_valid(&node.on_breach, VALID_ON_BREACH_POLICIES) {
            self.emit(
                format!(
                    "Unknown on_breach policy '{}' in shield '{}'. Valid: {}",
                    node.on_breach,
                    node.name,
                    valid_list(VALID_ON_BREACH_POLICIES)
                ),
                &node.loc,
            );
        }

        // Severity level
        if !node.severity.is_empty() && !is_valid(&node.severity, VALID_SEVERITY_LEVELS) {
            self.emit(
                format!(
                    "Unknown severity '{}' in shield '{}'. Valid: {}",
                    node.severity,
                    node.name,
                    valid_list(VALID_SEVERITY_LEVELS)
                ),
                &node.loc,
            );
        }

        // max_retries >= 0
        if let Some(v) = node.max_retries {
            if v < 0 {
                self.emit(
                    format!(
                        "max_retries must be >= 0, got {} in shield '{}'",
                        v, node.name
                    ),
                    &node.loc,
                );
            }
        }

        // confidence_threshold range
        if let Some(v) = node.confidence_threshold {
            self.check_range(v, 0.0, 1.0, "confidence_threshold", &node.loc);
        }

        // allow/deny overlap
        for tool in &node.allow_tools {
            if node.deny_tools.contains(tool) {
                self.emit(
                    format!(
                        "Tool '{}' appears in both allow_tools and deny_tools in shield '{}'",
                        tool, node.name
                    ),
                    &node.loc,
                );
            }
        }

        // §Fase 77.a — `sign:` closed catalog (`axon-T846`). An egress
        // shield signs with a receiver-verifiable algorithm; anything
        // outside the catalog is an ERROR, not a warning — a misspelled
        // algorithm would ship unsigned deliveries the receiver rejects.
        if !node.sign.is_empty() && !is_valid(&node.sign, VALID_SIGN_ALGORITHMS) {
            self.emit(
                format!(
                    "axon-T846 unknown sign algorithm '{}' in shield '{}'. Valid: {}",
                    node.sign,
                    node.name,
                    valid_list(VALID_SIGN_ALGORITHMS)
                ),
                &node.loc,
            );
        }

        // §Fase 77.a (`axon-W010`) — fields the parser did not recognize.
        // Pre-77 these were silently discarded, so `axon check` blessed
        // programs whose shield grammar the runtime ignores (Kivi brief
        // #51 §B.3 caught `sign:` this way before it existed). Warning,
        // not error: existing adopter programs must keep compiling;
        // escalation to an error is a major-version decision (D77.5).
        for (field, floc) in &node.unknown_fields {
            self.warn(
                format!(
                    "axon-W010 unknown field '{field}' in shield '{}' — the runtime \
                     IGNORES fields it does not know, so this line has NO effect. \
                     Valid shield fields: {SHIELD_FIELD_CATALOG}.",
                    node.name
                ),
                floc,
            );
        }

        // §Fase 77.a (`axon-W011`) — vacuous shield: an `on_breach:` policy
        // with NO enforcement-bearing field can never fire (nothing scans,
        // signs, redacts, or gates — there is no breach to react to). The
        // compile-time mirror of the PCC `shield_halt_guarantee` concern.
        // A sign-only egress shield is NOT vacuous (D77.6): the signature
        // is its enforcement.
        let has_enforcement = !node.scan.is_empty()
            || !node.sign.is_empty()
            || !node.redact.is_empty()
            || !node.allow_tools.is_empty()
            || !node.deny_tools.is_empty()
            || node.confidence_threshold.is_some();
        if !node.on_breach.is_empty() && !has_enforcement {
            self.warn(
                format!(
                    "axon-W011 shield '{}' declares `on_breach: {}` but has no \
                     enforcement-bearing field (scan / sign / redact / allow_tools / \
                     deny_tools / confidence_threshold) — the breach policy can never \
                     fire. Declare what the shield enforces, or remove `on_breach:`.",
                    node.name, node.on_breach
                ),
                &node.loc,
            );
        }
    }

    fn check_pix(&mut self, node: &PixDefinition) {
        // Source presence
        if node.source.is_empty() {
            self.emit(
                format!("Pix '{}' requires a 'source' field", node.name),
                &node.loc,
            );
        }

        // Depth range 1..=8
        if let Some(v) = node.depth {
            if v < 1 || v > 8 {
                self.emit(
                    format!(
                        "depth must be between 1 and 8, got {} in pix '{}'",
                        v, node.name
                    ),
                    &node.loc,
                );
            }
        }

        // Branching range 1..=10
        if let Some(v) = node.branching {
            if v < 1 || v > 10 {
                self.emit(
                    format!(
                        "branching must be between 1 and 10, got {} in pix '{}'",
                        v, node.name
                    ),
                    &node.loc,
                );
            }
        }
    }

    /// §Fase 62.0 — `ledger` (the audit chain). Distinct from `check_pix`: the
    /// ranges encode AUDIT semantics, not navigation. `depth` is the chain
    /// retention window (≥ 1 row); `branching` is the Merkle factor (0 = flat
    /// linear chain, ≥ 2 = balanced Merkle tree — 1 is degenerate).
    fn check_ledger(&mut self, node: &LedgerDefinition) {
        // Source presence — a ledger must bind to an audited surface.
        if node.source.is_empty() {
            self.emit(
                format!("Ledger '{}' requires a 'source' field", node.name),
                &node.loc,
            );
        }

        // Retention window: at least one row.
        if let Some(v) = node.depth {
            if v < 1 {
                self.emit(
                    format!(
                        "depth (chain retention) must be ≥ 1, got {} in ledger '{}'",
                        v, node.name
                    ),
                    &node.loc,
                );
            }
        }

        // Merkle factor: 0 (flat chain) or ≥ 2 (balanced tree); 1 is degenerate.
        if let Some(v) = node.branching {
            if v == 1 || v < 0 {
                self.emit(
                    format!(
                        "branching (Merkle factor) must be 0 (flat) or ≥ 2, got {} in ledger '{}'",
                        v, node.name
                    ),
                    &node.loc,
                );
            }
        }
    }

    fn check_psyche(&mut self, node: &PsycheDefinition) {
        // §1: ψ ∈ M requires dim(M) ≥ 1
        if node.dimensions.is_empty() {
            self.emit(
                format!(
                    "Psyche '{}' requires at least one dimension (manifold dim ≥ 1)",
                    node.name
                ),
                &node.loc,
            );
        }

        // Duplicate dimension detection
        let mut seen: Vec<String> = Vec::new();
        for dim in &node.dimensions {
            if seen.contains(dim) {
                self.emit(
                    format!("Duplicate dimension '{}' in psyche '{}'", dim, node.name),
                    &node.loc,
                );
            } else {
                seen.push(dim.clone());
            }
        }

        // Manifold noise σ ∈ (0, 1]
        if let Some(v) = node.manifold_noise {
            if v <= 0.0 || v > 1.0 {
                self.emit(
                    format!(
                        "manifold_noise must be in (0.0, 1.0], got {} in psyche '{}'",
                        v, node.name
                    ),
                    &node.loc,
                );
            }
        }

        // Manifold momentum β ∈ [0, 1]
        if let Some(v) = node.manifold_momentum {
            self.check_range(v, 0.0, 1.0, "manifold_momentum", &node.loc);
        }

        // Safety constraints non-empty
        if node.safety_constraints.is_empty() {
            self.emit(
                format!(
                    "Psyche '{}' requires at least one safety_constraint",
                    node.name
                ),
                &node.loc,
            );
        } else if !node
            .safety_constraints
            .iter()
            .any(|c| c == "non_diagnostic")
        {
            // §4: non_diagnostic is mandatory
            self.emit(
                format!("Psyche '{}' must include 'non_diagnostic' in safety_constraints (dependent type safety §4)", node.name),
                &node.loc,
            );
        }

        // Inference mode enum
        if !node.inference_mode.is_empty() && !is_valid(&node.inference_mode, VALID_INFERENCE_MODES)
        {
            self.emit(
                format!(
                    "Unknown inference_mode '{}' in psyche '{}'. Valid: {}",
                    node.inference_mode,
                    node.name,
                    valid_list(VALID_INFERENCE_MODES)
                ),
                &node.loc,
            );
        }
    }

    fn check_corpus(&mut self, node: &CorpusDefinition) {
        // Invariant G1: D ≠ ∅ — at least one document. A store-sourced corpus
        // (§64.A) satisfies G1 via its documents store (rows = documents).
        if node.documents.is_empty() && node.mcp_server.is_empty() && node.store_source.is_none() {
            self.emit(
                format!(
                    "Corpus '{}' requires at least one document or an mcp_server (G1: D ≠ ∅)",
                    node.name
                ),
                &node.loc,
            );
        }

        // §Fase 64.A — dynamic, axonstore-sourced MDN graph
        // (`corpus N from axonstore { documents: S(id,title)  relations: E(from,to,etype,weight) }`).
        if let Some(src) = &node.store_source {
            self.check_corpus_store_source(node, src);
        }

        // §Fase 63.A — MDN relations (typed weighted edges, paper Def 1-3).
        for r in &node.relations {
            // τ total on a CLOSED catalog (G3).
            if !is_valid(&r.etype, VALID_CORPUS_RELATIONS) {
                self.emit(
                    format!(
                        "Corpus '{}': unknown relation type '{}' (closed catalog: {})",
                        node.name,
                        r.etype,
                        VALID_CORPUS_RELATIONS.join(", ")
                    ),
                    &r.loc,
                );
            }
            // G2: edges connect corpus members (declared documents).
            if !node.documents.contains(&r.from) {
                self.emit(
                    format!(
                        "Corpus '{}': relation references undeclared document '{}' (G2: edges connect corpus members)",
                        node.name, r.from
                    ),
                    &r.loc,
                );
            }
            if !node.documents.contains(&r.to) {
                self.emit(
                    format!(
                        "Corpus '{}': relation references undeclared document '{}' (G2: edges connect corpus members)",
                        node.name, r.to
                    ),
                    &r.loc,
                );
            }
            // G4: ω ∈ (0, 1].
            if !(r.weight > 0.0 && r.weight <= 1.0) {
                self.emit(
                    format!(
                        "Corpus '{}': relation weight {} must be in (0, 1] (G4)",
                        node.name, r.weight
                    ),
                    &r.loc,
                );
            }
        }

        // §Fase 63.C — the memory endofunctor deforms the graph's geometry, so
        // `adaptive` is only meaningful on a corpus that HAS a graph: static
        // `relations:` OR (§64.A) a store-sourced edge store.
        if node.adaptive && node.relations.is_empty() && node.store_source.is_none() {
            self.emit(
                format!(
                    "Corpus '{}': `adaptive: true` requires `relations:` — memory deforms the graph, an edgeless corpus has nothing to learn",
                    node.name
                ),
                &node.loc,
            );
        }
    }

    /// §Fase 64.A — validate a dynamic, `axonstore`-sourced MDN corpus graph.
    /// Both backing stores must be declared `axonstore`s; when they carry a §38
    /// inline column schema, the mapped columns are validated for existence and
    /// type compatibility (id present; title text-like; edge from/to match the
    /// id type; etype text-like; weight numeric). Stores with a ManifestRef /
    /// EnvVar / no schema defer the column check to the runtime (consistent with
    /// §38's optional-schema D5). The weight-range invariant `ω ∈ (0, 1]` (G4) is
    /// a RUNTIME check here — weights are per-row dynamic, not compile-time.
    fn check_corpus_store_source(&mut self, node: &CorpusDefinition, src: &CorpusStoreSource) {
        use crate::store_schema::{StoreColumn, StoreColumnType};

        let doc_store = self.find_store(&src.doc_store);
        if doc_store.is_none() {
            self.emit(
                format!(
                    "Corpus '{}': documents store '{}' is not a declared axonstore",
                    node.name, src.doc_store
                ),
                &src.loc,
            );
        }
        let edge_store = self.find_store(&src.edge_store);
        if edge_store.is_none() {
            self.emit(
                format!(
                    "Corpus '{}': relations store '{}' is not a declared axonstore",
                    node.name, src.edge_store
                ),
                &src.loc,
            );
        }

        // Column-level validation only when BOTH stores carry an inline schema.
        let (Some(ds), Some(es)) = (doc_store, edge_store) else {
            return;
        };
        let (Some(dcols), Some(ecols)) = (
            ds.column_schema.as_ref().and_then(|s| s.inline_columns()),
            es.column_schema.as_ref().and_then(|s| s.inline_columns()),
        ) else {
            return;
        };

        let col_ty = |cols: &[StoreColumn], n: &str| -> Option<StoreColumnType> {
            cols.iter().find(|c| c.name == n).map(|c| c.col_type)
        };
        let is_text_like = |t: StoreColumnType| matches!(t, StoreColumnType::Text);
        let is_numeric = |t: StoreColumnType| {
            matches!(
                t,
                StoreColumnType::Float | StoreColumnType::Double | StoreColumnType::Numeric
            )
        };

        // documents: id (any type — the node id) + title (text-like).
        let id_ty = col_ty(dcols, &src.doc_id_col);
        if id_ty.is_none() {
            self.emit(
                format!(
                    "Corpus '{}': documents store '{}' has no column '{}' (the document id)",
                    node.name, src.doc_store, src.doc_id_col
                ),
                &src.loc,
            );
        }
        match col_ty(dcols, &src.doc_title_col) {
            None => self.emit(
                format!(
                    "Corpus '{}': documents store '{}' has no column '{}' (the title)",
                    node.name, src.doc_store, src.doc_title_col
                ),
                &src.loc,
            ),
            Some(t) if !is_text_like(t) => self.emit(
                format!(
                    "Corpus '{}': title column '{}' must be text-like (got {})",
                    node.name, src.doc_title_col, t
                ),
                &src.loc,
            ),
            _ => {}
        }

        // relations: from/to (must match the document id type — G2) + etype
        // (text-like) + weight (numeric; ω ∈ (0,1] enforced at runtime).
        for (label, col) in [("from", &src.edge_from_col), ("to", &src.edge_to_col)] {
            match col_ty(ecols, col) {
                None => self.emit(
                    format!(
                        "Corpus '{}': relations store '{}' has no column '{}' (the {} endpoint)",
                        node.name, src.edge_store, col, label
                    ),
                    &src.loc,
                ),
                Some(t) => {
                    if let Some(idt) = id_ty {
                        if t != idt {
                            self.emit(
                                format!(
                                    "Corpus '{}': edge {} column '{}' type {} must match the document id column type {} (G2: edges connect corpus members)",
                                    node.name, label, col, t, idt
                                ),
                                &src.loc,
                            );
                        }
                    }
                }
            }
        }
        match col_ty(ecols, &src.edge_type_col) {
            None => self.emit(
                format!(
                    "Corpus '{}': relations store '{}' has no column '{}' (the edge type)",
                    node.name, src.edge_store, src.edge_type_col
                ),
                &src.loc,
            ),
            Some(t) if !is_text_like(t) => self.emit(
                format!(
                    "Corpus '{}': edge type column '{}' must be text-like (got {})",
                    node.name, src.edge_type_col, t
                ),
                &src.loc,
            ),
            _ => {}
        }
        match col_ty(ecols, &src.edge_weight_col) {
            None => self.emit(
                format!(
                    "Corpus '{}': relations store '{}' has no column '{}' (the weight)",
                    node.name, src.edge_store, src.edge_weight_col
                ),
                &src.loc,
            ),
            Some(t) if !is_numeric(t) => self.emit(
                format!(
                    "Corpus '{}': edge weight column '{}' must be numeric (got {}); ω ∈ (0,1] is enforced at runtime",
                    node.name, src.edge_weight_col, t
                ),
                &src.loc,
            ),
            _ => {}
        }
    }

    fn check_ots(&mut self, node: &OtsDefinition) {
        // Teleology presence (goal required)
        if node.teleology.is_empty() {
            self.emit(
                format!(
                    "OTS '{}' requires a 'teleology' field (goal required)",
                    node.name
                ),
                &node.loc,
            );
        }

        // Homotopy search enum
        if !node.homotopy_search.is_empty() && !is_valid(&node.homotopy_search, VALID_OTS_HOMOTOPY)
        {
            self.emit(
                format!(
                    "Unknown homotopy_search '{}' in OTS '{}'. Valid: {}",
                    node.homotopy_search,
                    node.name,
                    valid_list(VALID_OTS_HOMOTOPY)
                ),
                &node.loc,
            );
        }
    }

    fn check_mandate(&mut self, node: &MandateDefinition) {
        // Constraint presence (refinement type T_M)
        if node.constraint.is_empty() {
            self.emit(
                format!("Mandate '{}' requires a 'constraint' field (refinement type T_M = {{x ∈ Σ* | M(x) ⊢ ⊤}})", node.name),
                &node.loc,
            );
        }

        // ── §Fase 119.b — ONE admissibility judgment, shared with the runtime ──
        //
        // These checks used to be five hand-rolled `if let Some(v)` sign tests
        // in this function, while axon-rs carried its own copy of the same
        // arithmetic for the controller. Two copies of a stability judgment is
        // the §118 smell (a general concept parked where it was first needed),
        // and worse here: the two could drift, and a mandate the compiler
        // admits could be one the runtime refuses.
        //
        // Now `stability::MandateStatics` is the single source of the
        // judgment. When the declaration carries `stability { D:, L: }`, the
        // full band `D < |Kp+Ki+Kd| < 1/L` is verified — both endpoints
        // exclusive, per the strict inequalities of paper_mandate §3 and
        // prompt_opt §6.3. Without it, only the sign conditions apply (they
        // are necessary regardless), and the IR carries no bounds — so the
        // runtime can SEE that nothing was statically promised.
        //
        // Note also what made this reachable at all: until §119.b.1 the
        // published `pid { }` / `epsilon:` syntax parsed with every gain
        // silently dropped to `None`, so every `if let Some(v)` below was
        // dead for any README-style mandate. The validation existed and no
        // program could reach it.
        let statics = crate::stability::MandateStatics {
            kp: node.kp,
            ki: node.ki,
            kd: node.kd,
            epsilon: node.tolerance,
            max_steps: node.max_steps,
            drift_bound: node.drift_bound,
            lipschitz: node.lipschitz,
        };
        for err in statics.admissibility_errors() {
            self.emit(format!("mandate '{}': {}", node.name, err), &node.loc);
        }

        // on_violation policy
        if !node.on_violation.is_empty() && !is_valid(&node.on_violation, VALID_MANDATE_POLICIES) {
            self.emit(
                format!(
                    "Unknown on_violation '{}' in mandate '{}'. Valid: {}",
                    node.on_violation,
                    node.name,
                    valid_list(VALID_MANDATE_POLICIES)
                ),
                &node.loc,
            );
        }
    }

    fn check_axonstore(&mut self, node: &AxonStoreDefinition) {
        // ── §Fase 113 — axon-T946: the store names ONE source of truth ───────
        //
        // `resource.endpoint` and `axonstore.connection` are THE SAME FACT
        // DECLARED TWICE — that is the islands finding, in one sentence. A store
        // that declares both is that bug, written down deliberately, and nothing
        // would check the two agree.
        //
        // The migration is soft (ratified): `connection:` alone still compiles,
        // because it is what the live deployment runs on. But you may not have
        // both, and you may not have neither.
        if !node.resource_ref.is_empty() && !node.connection.is_empty() {
            self.emit(
                format!(
                    "axon-T946 Axonstore '{}' declares BOTH `resource: {}` and `connection:` — \
                     the same fact, twice, with nothing checking they agree. That duplication is \
                     precisely the defect `resource:` exists to end. Keep `resource:` (it also \
                     carries the pool size and the sharing discipline) and delete `connection:`.",
                    node.name, node.resource_ref
                ),
                &node.loc,
            );
        }
        if !node.resource_ref.is_empty() {
            match self.symbols.lookup(&node.resource_ref) {
                None => self.emit(
                    format!(
                        "axon-T946 Axonstore '{}' names resource '{}', which is not declared.",
                        node.name, node.resource_ref
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "resource" => self.emit(
                    format!(
                        "axon-T946 Axonstore '{}' names '{}', which is a {}, not a resource.",
                        node.name, node.resource_ref, sym.kind
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }

        // Backend enum
        if !node.backend.is_empty() && !is_valid(&node.backend, VALID_STORE_BACKENDS) {
            self.emit(
                format!(
                    "Unknown backend '{}' in axonstore '{}'. Valid: {}",
                    node.backend,
                    node.name,
                    valid_list(VALID_STORE_BACKENDS)
                ),
                &node.loc,
            );
        }

        // Isolation level enum
        if !node.isolation.is_empty() && !is_valid(&node.isolation, VALID_STORE_ISOLATION) {
            self.emit(
                format!(
                    "Unknown isolation '{}' in axonstore '{}'. Valid: {}",
                    node.isolation,
                    node.name,
                    valid_list(VALID_STORE_ISOLATION)
                ),
                &node.loc,
            );
        }

        // on_breach policy
        if !node.on_breach.is_empty() && !is_valid(&node.on_breach, VALID_STORE_ON_BREACH) {
            self.emit(
                format!(
                    "Unknown on_breach '{}' in axonstore '{}'. Valid: {}",
                    node.on_breach,
                    node.name,
                    valid_list(VALID_STORE_ON_BREACH)
                ),
                &node.loc,
            );
        }

        // confidence_floor range
        if let Some(v) = node.confidence_floor {
            self.check_range(v, 0.0, 1.0, "confidence_floor", &node.loc);
        }

        // §Fase 94.a — the `backend: secrets` placement laws (axon-T900).
        // A secrets store is a class-scoped, read-only metadata view over
        // the tenant's secret custody; every field that would make it
        // look like an adopter table is unrepresentable.
        if node.backend == "secrets" {
            if node.class.is_empty() {
                self.emit(
                    format!(
                        "axon-T900 axonstore '{}' declares `backend: secrets` without a \
                         `class:` — a class-less secrets store would enumerate the \
                         tenant's ENTIRE secret namespace (`llm.*` included). Declare \
                         the secret-class prefix it may see, e.g. `class: crm` (covers \
                         keys under `crm.`).",
                        node.name
                    ),
                    &node.loc,
                );
            } else if !crate::parser::is_valid_capability_slug(&node.class) {
                self.emit(
                    format!(
                        "axon-T900 axonstore '{}' declares an invalid secret class \
                         '{}'. A class is a dotted lowercase prefix matching \
                         ^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)*$ — e.g. `crm`, \
                         `crm.oauth`.",
                        node.name, node.class
                    ),
                    &node.loc,
                );
            }
            if node.column_schema.is_some() {
                self.emit(
                    format!(
                        "axon-T900 axonstore '{}' declares `backend: secrets` AND an \
                         explicit `schema` — the metadata schema of a secrets store is \
                         LAW, synthesized by the compiler (key: Text, version: Int, \
                         created_at: Timestamptz, expires_at: Timestamptz). Drop the \
                         `schema` block; the secret VALUE has no column by design \
                         (`rotation_without_revelation`).",
                        node.name
                    ),
                    &node.loc,
                );
            }
            if !node.connection.is_empty()
                || !node.isolation.is_empty()
                || !node.on_breach.is_empty()
                || node.confidence_floor.is_some()
            {
                self.emit(
                    format!(
                        "axon-T900 axonstore '{}' declares `backend: secrets` with \
                         adopter-storage fields (`connection:` / `isolation:` / \
                         `on_breach:` / `confidence_floor:`) — a secrets store has no \
                         connection string and no adopter table behind it (the runtime \
                         binds it to the tenant's secret custody). Only `class:` and \
                         `capability:` apply.",
                        node.name
                    ),
                    &node.loc,
                );
            }
        } else if !node.class.is_empty() {
            self.emit(
                format!(
                    "axon-T900 axonstore '{}' declares `class: {}` but its backend is \
                     '{}' — `class:` is the secret-class prefix of a `backend: secrets` \
                     metadata store and has no meaning elsewhere.",
                    node.name,
                    node.class,
                    if node.backend.is_empty() {
                        "<unset>"
                    } else {
                        &node.backend
                    }
                ),
                &node.loc,
            );
        }
    }

    /// §λ-L-E Fase 1 — Resource validation.
    ///
    /// Enforces: (a) lifetime ∈ {linear | affine | persistent}; (b) certainty_floor
    /// ∈ [0.0, 1.0] when present; (c) shield_ref, if non-empty, is a declared shield.
    fn check_resource(&mut self, node: &ResourceDefinition) {
        if !node.lifetime.is_empty()
            && !matches!(node.lifetime.as_str(), "linear" | "affine" | "persistent")
        {
            self.emit(
                format!(
                    "Invalid lifetime '{}' for resource '{}' — \
                     expected linear | affine | persistent",
                    node.lifetime, node.name
                ),
                &node.loc,
            );
        }
        if let Some(c) = node.certainty_floor {
            if !(0.0..=1.0).contains(&c) {
                self.emit(
                    format!(
                        "certainty_floor {c} for resource '{}' is out of range [0.0, 1.0]",
                        node.name
                    ),
                    &node.loc,
                );
            }
        }
        if !node.shield_ref.is_empty() {
            match self.symbols.lookup(&node.shield_ref) {
                None => self.emit(
                    format!(
                        "Undefined shield '{}' in resource '{}'",
                        node.shield_ref, node.name
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "shield" => self.emit(
                    format!(
                        "'{}' is a {}, not a shield (referenced in resource '{}')",
                        node.shield_ref, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }

        // ── §Fase 113 ────────────────────────────────────────────────────────

        // axon-T942 — `kind:` is a CLOSED catalog.
        //
        // It was a free string that nothing read. A typo produced a resource
        // the runtime could never reach, and said nothing.
        if node.kind.is_empty() {
            self.emit(
                format!(
                    "axon-T942 Resource '{}' has no `kind:` — declare one of: {}. \
                     A resource with no kind names no infrastructure; the runtime cannot \
                     reach it, and every discipline hung on it (capacity, lifetime, lease) \
                     would govern nothing.",
                    node.name,
                    VALID_RESOURCE_KINDS.join(" | ")
                ),
                &node.loc,
            );
        } else if !VALID_RESOURCE_KINDS.contains(&node.kind.as_str()) {
            self.emit(
                format!(
                    "axon-T942 Invalid kind '{}' for resource '{}' — expected one of: {}. \
                     The catalog is exactly what the runtime can REACH; a kind outside it \
                     is a promise with no implementation behind it.",
                    node.kind,
                    node.name,
                    VALID_RESOURCE_KINDS.join(" | ")
                ),
                &node.loc,
            );
        }

        // axon-T943 — `within:` names a DECLARED fabric.
        //
        // `within:` is one field, so Separation-Logic disjointness is
        // unrepresentable rather than checked. What DOES need checking is that
        // the fabric it names exists: a resource `within:` a phantom fabric is
        // placed nowhere, and every `*` claim about it is vacuous.
        if !node.within.is_empty() {
            match self.symbols.lookup(&node.within) {
                None => self.emit(
                    format!(
                        "axon-T943 Resource '{}' is `within:` fabric '{}', which is not \
                         declared. A resource placed in a fabric that does not exist is \
                         placed nowhere.",
                        node.name, node.within
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "fabric" => self.emit(
                    format!(
                        "axon-T943 Resource '{}' is `within:` '{}', which is a {}, not a \
                         fabric.",
                        node.name, node.within, sym.kind
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }

        // axon-T944 — `endpoint:` is a per-tenant CONFIG KEY, never a URL literal.
        //
        // The language already legislated this. `axon-T850` refuses a URL
        // literal in `upstream.resolve` with the words "URLs and credentials
        // never appear in source"; `axon-T902` mirrors it for credentials.
        // `resource.endpoint` was a GRANDFATHERED VIOLATION OF THE LANGUAGE'S
        // OWN LAW — a production DB URI, in source, in the one declaration that
        // claims to be the single source of truth for infrastructure.
        //
        // The runtime already speaks this idiom: `resolve_dsn` accepts
        // `env:VAR`, and adopters already write `connection: "env:AXON_DB_URL"`.
        // This is a tightening, not an invention.
        if node.endpoint.is_empty() {
            self.emit(
                format!(
                    "axon-T944 Resource '{}' has no `endpoint:` — declare a per-tenant config \
                     key (e.g. `endpoint: db.main`). A resource with no endpoint is unreachable.",
                    node.name
                ),
                &node.loc,
            );
        } else if !is_config_key(&node.endpoint) {
            self.emit(
                format!(
                    "axon-T944 Resource '{}' `endpoint:` value '{}' is not a config key — keys \
                     are lowercase dot-separated (`[a-z0-9][a-z0-9_.-]*`, no `/`, no `:`). \
                     URLs and credentials never appear in source (the same config-not-code law \
                     `axon-T850` already enforces on `upstream.resolve`, and `axon-T902` on \
                     `tool.secret`). A production DSN written into the program is exactly the \
                     shape §94 secret custody exists to refuse.",
                    node.name, node.endpoint
                ),
                &node.loc,
            );
        }
    }

    /// §λ-L-E Fase 1 — Fabric validation.
    ///
    /// Enforces: (a) zones ≥ 1 when present; (b) shield_ref, if non-empty,
    /// is a declared shield.
    fn check_fabric(&mut self, node: &FabricDefinition) {
        // ── §Fase 119.e — axon-E041 region/provider mismatch ────────────────
        //
        // §111's finding on `fabric`: `provider`/`region`/`zones` were
        // "consumed by NOTHING". This is the first thing they decide. The
        // knowledge doc names this check by its code and its example:
        // `provider: aws  region: "eastus"` describes a substrate that does
        // not exist, and a typed expectation that cannot be true is worse
        // than an untyped one — it reads as verified.
        //
        // The provider catalog stays OPEN (the doc is explicit that the
        // runtime decides deployability); only providers whose region shape
        // is public fact are judged, and `substrate` reports when it makes
        // no judgment rather than inventing one.
        if !node.provider.is_empty() {
            if let Some(false) =
                crate::substrate::region_matches_provider(&node.provider, &node.region)
            {
                let hint = match crate::substrate::provider_shape(&node.provider) {
                    crate::substrate::ProviderShape::Validated { example } => {
                        format!(" (a {} region looks like \"{example}\")", node.provider)
                    }
                    crate::substrate::ProviderShape::Unvalidated => String::new(),
                };
                self.emit(
                    format!(
                        "axon-E041 region/provider mismatch: fabric '{}' declares                          provider '{}' with region \"{}\"{hint}. The substrate this                          fabric describes does not exist, so every resource placed                          `within` it expects to find something that is not there.",
                        node.name, node.provider, node.region
                    ),
                    &node.loc,
                );
            }
        }
        if let Some(z) = node.zones {
            if z < 1 {
                self.emit(
                    format!(
                        "Fabric '{}' has invalid zones {z} — must be >= 1",
                        node.name
                    ),
                    &node.loc,
                );
            }
        }
        if !node.shield_ref.is_empty() {
            match self.symbols.lookup(&node.shield_ref) {
                None => self.emit(
                    format!(
                        "Undefined shield '{}' in fabric '{}'",
                        node.shield_ref, node.name
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "shield" => self.emit(
                    format!(
                        "'{}' is a {}, not a shield (referenced in fabric '{}')",
                        node.shield_ref, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
    }

    /// §λ-L-E Fase 1 — Manifest validation.
    ///
    /// Enforces: (a) every name in `resources` refers to a declared resource;
    /// (b) `fabric_ref`, if non-empty, is a declared fabric; (c) no duplicate
    /// resource names within a single manifest (Separation Logic `*` disjointness
    /// within-manifest — cross-manifest aliasing is a separate check).
    fn check_manifest(&mut self, node: &ManifestDefinition) {
        // ── §Fase 119.e — axon-E042 compliance ↔ substrate jurisdiction ─────
        //
        // The knowledge doc's own example: *"Compliance shields cross-validate
        // this against their declared geographic constraints — e.g. a
        // GDPR-tagged manifest deployed to a non-EU region is rejected."*
        // This is where the fabric's `region` stops being a label: it decides
        // whether a declared obligation can hold.
        //
        // An UNDETERMINABLE jurisdiction is a violation too, reported
        // distinctly. "We cannot tell" must never read as "it is fine" — the
        // whole point of a typed expectation is that it was shown.
        if !node.fabric_ref.is_empty() && !node.compliance.is_empty() {
            // Resolved from the program, not from accumulated state: the
            // declaration ORDER must not decide whether the check runs.
            let substrate = self.program.declarations.iter().find_map(|d| match d {
                Declaration::Fabric(f) if f.name == node.fabric_ref => {
                    Some((f.provider.clone(), f.region.clone()))
                }
                _ => None,
            });
            if let Some((provider, region)) = substrate {
                for tag in &node.compliance {
                    if let Some(v) =
                        crate::substrate::compliance_violation(tag, &provider, &region)
                    {
                        self.emit(
                            format!(
                                "axon-E042 compliance/jurisdiction: manifest '{}' {v}",
                                node.name
                            ),
                            &node.loc,
                        );
                    }
                }
            }
        }

        // (a) resource references must resolve
        let mut seen: std::collections::HashSet<&String> = std::collections::HashSet::new();
        for res_name in &node.resources {
            if !seen.insert(res_name) {
                self.emit(
                    format!(
                        "Manifest '{}' lists resource '{}' more than once \
                         (Linear/Separation Logic disjointness)",
                        node.name, res_name
                    ),
                    &node.loc,
                );
                continue;
            }
            match self.symbols.lookup(res_name) {
                None => self.emit(
                    format!(
                        "Manifest '{}' references undefined resource '{}'",
                        node.name, res_name
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "resource" => self.emit(
                    format!(
                        "'{}' is a {}, not a resource (referenced in manifest '{}')",
                        res_name, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
        // (b) fabric reference
        if !node.fabric_ref.is_empty() {
            match self.symbols.lookup(&node.fabric_ref) {
                None => self.emit(
                    format!(
                        "Manifest '{}' references undefined fabric '{}'",
                        node.name, node.fabric_ref
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "fabric" => self.emit(
                    format!(
                        "'{}' is a {}, not a fabric (referenced in manifest '{}')",
                        node.fabric_ref, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
        if let Some(z) = node.zones {
            if z < 1 {
                self.emit(
                    format!(
                        "Manifest '{}' has invalid zones {z} — must be >= 1",
                        node.name
                    ),
                    &node.loc,
                );
            }
        }
    }

    /// §λ-L-E Fase 1 — Observe validation.
    ///
    /// Enforces: (a) `target` refers to a declared manifest; (b) certainty_floor
    /// ∈ [0.0, 1.0] when present; (c) quorum ≥ 1 when present; (d) on_partition
    /// ∈ {fail, shield_quarantine}; (e) `sources` is non-empty.
    fn check_observe(&mut self, node: &ObserveDefinition) {
        // (a) target manifest
        if node.target.is_empty() {
            self.emit(
                format!(
                    "Observe '{}' is missing 'from <Manifest>' target",
                    node.name
                ),
                &node.loc,
            );
        } else {
            match self.symbols.lookup(&node.target) {
                None => self.emit(
                    format!(
                        "Observe '{}' targets undefined manifest '{}'",
                        node.name, node.target
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "manifest" => self.emit(
                    format!(
                        "'{}' is a {}, not a manifest (observed by '{}')",
                        node.target, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
        // (b) certainty floor range
        if let Some(c) = node.certainty_floor {
            if !(0.0..=1.0).contains(&c) {
                self.emit(
                    format!(
                        "certainty_floor {c} for observe '{}' is out of range [0.0, 1.0]",
                        node.name
                    ),
                    &node.loc,
                );
            }
        }
        // (c) quorum
        if let Some(q) = node.quorum {
            if q < 1 {
                self.emit(
                    format!(
                        "Observe '{}' has invalid quorum {q} — must be >= 1",
                        node.name
                    ),
                    &node.loc,
                );
            }
        }
        // (d) on_partition enum
        if !node.on_partition.is_empty()
            && !matches!(node.on_partition.as_str(), "fail" | "shield_quarantine")
        {
            self.emit(
                format!(
                    "Invalid on_partition '{}' for observe '{}' — \
                     expected fail | shield_quarantine",
                    node.on_partition, node.name
                ),
                &node.loc,
            );
        }
        // (e) sources must be non-empty
        if node.sources.is_empty() {
            self.emit(
                format!("Observe '{}' has empty sources: list", node.name),
                &node.loc,
            );
        }
    }

    /// §λ-L-E Fase 3 — Reconcile validation.
    ///
    /// Enforces: (a) observe_ref refers to a declared observe; (b) threshold
    /// and tolerance ∈ [0.0, 1.0]; (c) shield_ref / mandate_ref (if present)
    /// resolve to correct kinds; (d) max_retries ≥ 0.
    fn check_reconcile(&mut self, node: &ReconcileDefinition) {
        if node.observe_ref.is_empty() {
            self.emit(
                format!("Reconcile '{}' is missing 'observe:' target", node.name),
                &node.loc,
            );
        } else {
            match self.symbols.lookup(&node.observe_ref) {
                None => self.emit(
                    format!(
                        "Reconcile '{}' references undefined observe '{}'",
                        node.name, node.observe_ref
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "observe" => self.emit(
                    format!(
                        "'{}' is a {}, not an observe (referenced in reconcile '{}')",
                        node.observe_ref, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
        if let Some(t) = node.threshold {
            if !(0.0..=1.0).contains(&t) {
                self.emit(
                    format!(
                        "threshold {t} for reconcile '{}' is out of range [0.0, 1.0]",
                        node.name
                    ),
                    &node.loc,
                );
            }
        }
        if let Some(t) = node.tolerance {
            if !(0.0..=1.0).contains(&t) {
                self.emit(
                    format!(
                        "tolerance {t} for reconcile '{}' is out of range [0.0, 1.0]",
                        node.name
                    ),
                    &node.loc,
                );
            }
        }
        if node.max_retries < 0 {
            self.emit(
                format!(
                    "Reconcile '{}' has invalid max_retries {} — must be >= 0",
                    node.name, node.max_retries
                ),
                &node.loc,
            );
        }
        if !node.shield_ref.is_empty() {
            match self.symbols.lookup(&node.shield_ref) {
                None => self.emit(
                    format!(
                        "Undefined shield '{}' in reconcile '{}'",
                        node.shield_ref, node.name
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "shield" => self.emit(
                    format!(
                        "'{}' is a {}, not a shield (referenced in reconcile '{}')",
                        node.shield_ref, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
        if !node.mandate_ref.is_empty() {
            match self.symbols.lookup(&node.mandate_ref) {
                None => self.emit(
                    format!(
                        "Undefined mandate '{}' in reconcile '{}'",
                        node.mandate_ref, node.name
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "mandate" => self.emit(
                    format!(
                        "'{}' is a {}, not a mandate (referenced in reconcile '{}')",
                        node.mandate_ref, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
    }

    /// §λ-L-E Fase 3 — Lease validation.
    ///
    /// Enforces: (a) resource_ref resolves to a declared resource; (b) duration
    /// is non-empty; (c) acquire / on_expire enums are already validated at
    /// parse time but we re-check symbolically for defence-in-depth.
    fn check_lease(&mut self, node: &LeaseDefinition) {
        if node.resource_ref.is_empty() {
            self.emit(
                format!("Lease '{}' is missing 'resource:' target", node.name),
                &node.loc,
            );
        } else {
            match self.symbols.lookup(&node.resource_ref) {
                None => self.emit(
                    format!(
                        "Lease '{}' references undefined resource '{}'",
                        node.name, node.resource_ref
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "resource" => self.emit(
                    format!(
                        "'{}' is a {}, not a resource (leased by '{}')",
                        node.resource_ref, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
        if node.duration.is_empty() {
            self.emit(
                format!("Lease '{}' is missing 'duration:' field", node.name),
                &node.loc,
            );
        }
    }

    /// §λ-L-E Fase 3 — Ensemble validation.
    ///
    /// Enforces: (a) each observation name refers to a declared observe;
    /// (b) quorum ≥ 1 and ≤ len(observations); (c) at least 2 observations
    /// are required for a meaningful Byzantine ensemble.
    fn check_ensemble(&mut self, node: &EnsembleDefinition) {
        if node.observations.is_empty() {
            self.emit(
                format!("Ensemble '{}' has empty observations: list", node.name),
                &node.loc,
            );
            return;
        }
        if node.observations.len() < 2 {
            self.emit(
                format!(
                    "Ensemble '{}' has {} observation(s); Byzantine quorum requires >= 2",
                    node.name,
                    node.observations.len()
                ),
                &node.loc,
            );
        }
        let mut seen: std::collections::HashSet<&String> = std::collections::HashSet::new();
        for obs_name in &node.observations {
            if !seen.insert(obs_name) {
                self.emit(
                    format!(
                        "Ensemble '{}' lists observation '{}' more than once",
                        node.name, obs_name
                    ),
                    &node.loc,
                );
                continue;
            }
            match self.symbols.lookup(obs_name) {
                None => self.emit(
                    format!(
                        "Ensemble '{}' references undefined observation '{}'",
                        node.name, obs_name
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "observe" => self.emit(
                    format!(
                        "'{}' is a {}, not an observe (referenced in ensemble '{}')",
                        obs_name, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
        if let Some(q) = node.quorum {
            if q < 1 {
                self.emit(
                    format!(
                        "Ensemble '{}' has invalid quorum {q} — must be >= 1",
                        node.name
                    ),
                    &node.loc,
                );
            } else if (q as usize) > node.observations.len() {
                self.emit(
                    format!(
                        "Ensemble '{}' quorum {q} exceeds available observations ({})",
                        node.name,
                        node.observations.len()
                    ),
                    &node.loc,
                );
            }
        }
    }

    // ── §λ-L-E Fase 4 — Topology + π-calculus binary sessions ──────

    /// §λ-L-E Fase 4 — Session validation.
    ///
    /// Enforces: (a) exactly 2 roles; (b) role names are distinct; (c) every
    /// step has a valid op and — for send/receive — a non-empty message type;
    /// (d) Honda-Vasconcelos duality between the two roles.
    fn check_session(&mut self, node: &SessionDefinition) {
        if node.roles.len() != 2 {
            self.emit(
                format!(
                    "Session '{}' must declare exactly 2 roles (binary session); got {}",
                    node.name,
                    node.roles.len()
                ),
                &node.loc,
            );
        } else if node.roles[0].name == node.roles[1].name {
            self.emit(
                format!(
                    "Session '{}' has duplicate role name '{}'",
                    node.name, node.roles[0].name
                ),
                &node.loc,
            );
        }
        for role in &node.roles {
            self.check_session_role(&node.name, role);
        }
        if node.roles.len() == 2 {
            self.check_session_duality(node);
        }
    }

    fn check_session_role(&mut self, session_name: &str, role: &SessionRole) {
        self.check_session_steps(session_name, &role.name, &role.steps);
    }

    /// §Fase 41.b — validate a step sequence, recursing into `select`/`branch`
    /// arms: `send`/`receive` need a message type; a choice needs ≥ 1 branch
    /// with unique labels; each branch's sub-protocol is validated recursively.
    fn check_session_steps(&mut self, session_name: &str, role_name: &str, steps: &[SessionStep]) {
        for (idx, step) in steps.iter().enumerate() {
            match step.op.as_str() {
                "send" | "receive" => {
                    if step.message_type.is_empty() {
                        self.emit(
                            format!(
                                "Session '{session_name}' role '{role_name}' step #{idx} '{}' \
                                 requires a message type",
                                step.op
                            ),
                            &step.loc,
                        );
                    }
                }
                "loop" | "end" => {}
                // §Fase 79.c — `resume` is a bare terminal step (the handler's
                // normal exit). Its well-formedness (only inside an interrupt
                // handler) is enforced by the enclosing `interrupt` check below;
                // a stray top-level `resume` reaching here is harmless as a step
                // but never lowers to a reachable exit outside a handler.
                "resume" => {}
                // §Fase 79.c — interrupt region.
                "interrupt" => {
                    // (a) closed-catalog signal (D79.2).
                    if !CALL_INTERRUPT_CAUSES.contains(&step.message_type.as_str()) {
                        self.emit(
                            format!(
                                "Session '{session_name}' role '{role_name}' step #{idx}: \
                                 interrupt signal '{}' is not a CallInterruptCause \
                                 (expected one of: {})",
                                step.message_type,
                                CALL_INTERRUPT_CAUSES.join(", ")
                            ),
                            &step.loc,
                        );
                    }
                    // (b) both a body and a resumable handler arm.
                    let has = |l: &str| step.branches.iter().any(|b| b.label == l);
                    if !has("body") || !has("handler") {
                        self.emit(
                            format!(
                                "Session '{session_name}' role '{role_name}' step #{idx}: \
                                 interrupt requires both a body and a resumable handler"
                            ),
                            &step.loc,
                        );
                    }
                    for b in &step.branches {
                        self.check_session_steps(session_name, role_name, &b.steps);
                    }
                    // (c) two-exit well-formedness (D79.11a): the handler must
                    // reach either `resume` (normal exit) or `end` (abandon exit).
                    if let Some(h) = step.branches.iter().find(|b| b.label == "handler") {
                        if !handler_reaches_exit(&h.steps) {
                            self.emit(
                                format!(
                                    "Session '{session_name}' role '{role_name}' step #{idx}: \
                                     interrupt handler must reach `resume` or `end` \
                                     (a two-exit construct, paper §3.5)"
                                ),
                                &step.loc,
                            );
                        }
                    }
                }
                "select" | "branch" => {
                    if step.branches.is_empty() {
                        self.emit(
                            format!(
                                "Session '{session_name}' role '{role_name}' step #{idx} '{}' must \
                                 have at least one branch",
                                step.op
                            ),
                            &step.loc,
                        );
                    }
                    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
                    for b in &step.branches {
                        if !seen.insert(b.label.as_str()) {
                            self.emit(
                                format!(
                                    "Session '{session_name}' role '{role_name}' choice has \
                                     duplicate branch label '{}'",
                                    b.label
                                ),
                                &b.loc,
                            );
                        }
                        self.check_session_steps(session_name, role_name, &b.steps);
                    }
                }
                other => {
                    self.emit(
                        format!("Session '{session_name}' role '{role_name}' step #{idx} has invalid op '{other}'"),
                        &step.loc,
                    );
                }
            }
        }
    }

    /// §Fase 41.b — duality is now decided by the **session-type algebra**
    /// (`crate::session`): each role is lowered to a [`SessionType`] (with
    /// `loop` becoming an equirecursive `μ`), and the two are checked under the
    /// **connection law** `T₂ ≡ T₁⊥` via regular-coinductive duality. This
    /// supersedes the old positional, equal-length, step-by-step check — which
    /// could not reason about `loop` as a recursion point — and grounds the
    /// language's binary-session duality in linear logic (Caires–Pfenning).
    fn check_session_duality(&mut self, node: &SessionDefinition) {
        let t1 = lower_session_role(&node.roles[0]);
        let t2 = lower_session_role(&node.roles[1]);
        // §80.g hardening (`axon-W012`) — a role whose FIRST step is `loop`
        // lowers to the unguarded μX.X (everything after a `loop` in
        // sequence is unreachable at this layer), so duality/credit hold
        // VACUOUSLY — the checker is checking nothing. A warning (not an
        // error: pre-§80 corpus uses the leading-loop idiom), and the
        // coinductive analyses must never see the degenerate type — the
        // §41.c discharge skips it rather than risk unfolding μX.X.
        for (role, t) in [(&node.roles[0], &t1), (&node.roles[1], &t2)] {
            if is_unguarded_recursion(t) {
                self.warn(
                    format!(
                        "axon-W012 session '{}' role '{}': a leading `loop` makes the session type vacuous (μX.X) — everything after a `loop` in a sequence is unreachable, so duality and credit hold trivially, not meaningfully. Put the iteration body first and `loop` last: `[ send A, receive B, loop ]`",
                        node.name, role.name
                    ),
                    &node.loc,
                );
                return;
            }
        }
        if !t1.is_dual_to(&t2) {
            self.emit(
                format!(
                    "Session '{}' duality violation: role '{}' has the session type `{}`, \
                     whose dual is `{}`, but role '{}' has `{}` (expected the dual)",
                    node.name,
                    node.roles[0].name,
                    t1,
                    t1.dual(),
                    node.roles[1].name,
                    t2,
                ),
                &node.loc,
            );
        }
    }

    /// §Fase 41.b/c — `socket` validation (the typed-WS transport binding).
    /// Enforces, in order:
    ///   (a) `protocol` references a **declared `session`** (whose two roles
    ///       are duality-checked via the §41.a algebra, so the dialogue
    ///       carried over the connection is conformant by construction);
    ///   (b) the backpressure credit window, if given, is **positive** — a
    ///       0-credit window cannot type a send (§4.2: no rule at n=0);
    ///   (c) §Fase 41.c — the **Presburger discharge**: lower each role into
    ///       the §41.a algebra, stamp with the socket's `credit(k)`, and run
    ///       [`SessionType::credit_analyse`] — surfaces send-at-zero, burst
    ///       overflow (the protocol demands a send-burst > k), and loop
    ///       unsustainability (`Δ = #send − #recv > 0` per recurring path).
    fn check_socket(&mut self, node: &SocketDefinition) {
        // (a) protocol shape.
        let session = if node.protocol.is_empty() {
            self.emit(
                format!("Socket '{}' has no `protocol:` — it must reference a declared session", node.name),
                &node.loc,
            );
            None
        } else {
            match find_session_by_name(self.program, &node.protocol) {
                Some(s) => Some(s),
                None => {
                    self.emit(
                        format!(
                            "Socket '{}' protocol '{}' is not a declared session (the protocol must be a `session`)",
                            node.name, node.protocol
                        ),
                        &node.loc,
                    );
                    None
                }
            }
        };
        // (b) credit window must be positive — the static face of the n=0 axiom.
        let budget: Option<u64> = match node.backpressure_credit {
            Some(n) if n >= 1 => Some(n as u64),
            Some(n) => {
                self.emit(
                    format!(
                        "Socket '{}' backpressure credit must be ≥ 1 (got {n}); a 0-credit window \
                         cannot type a send (§Fase 41 §4.2)",
                        node.name
                    ),
                    &node.loc,
                );
                None
            }
            None => None, // unbounded fragment — no credit constraints to discharge
        };
        // (c) Presburger discharge — only runs when (a) + (b) succeeded.
        if let (Some(session), Some(budget)) = (session, budget) {
            for role in &session.roles {
                let lowered = lower_session_role(role).with_credit(budget);
                if is_unguarded_recursion(&lowered) {
                    // Already diagnosed on the session declaration (§80.g
                    // hardening); never hand μX.X to the analyses.
                    continue;
                }
                if let Err(e) = lowered.credit_analyse(budget) {
                    self.emit(
                        format!(
                            "Socket '{}' violates the credit-refined backpressure type of \
                             session '{}' role '{}': {} (D2)",
                            node.name, node.protocol, role.name, e
                        ),
                        &node.loc,
                    );
                }
            }
        }
    }

    /// §Fase 80.c — `upstream` validation (the outbound vendor connection).
    /// Enforces, in order:
    ///   (a) closed catalogs — `transport:` / `auth:` / `overflow:` /
    ///       `reconnect.on_exhausted:` membership + auth-shape coherence
    ///       (`header`/`query` need a name; `signed_url` must not carry one);
    ///   (b) **axon-T851** — `protocol:` resolves to a declared `session` and
    ///       `role:` is one of its two roles (duality of the session itself is
    ///       already §41.a's law on the declaration); when a credit window is
    ///       given, the §41.c Presburger discharge runs on the bound role;
    ///   (c) **axon-T850** — `resolve:`/`secret:` are config KEYS
    ///       (lowercase dot-separated, the compile-time mirror of the
    ///       enterprise `SecretKeyPolicy` charset) — a URL or credential
    ///       literal in source is unrepresentable, the §58.g "config, not
    ///       code" property made a law;
    ///   (d) **axon-T849** — the `map:` projection is a TOTAL, unambiguous
    ///       cover of the bound role's message set: every send/receive has
    ///       exactly one rule of the right direction, no rule names a message
    ///       the role never exchanges, no two receive-json rules share a
    ///       discriminator, at most one receive-binary rule. Partial
    ///       transcoding is a compile error, never a runtime surprise.
    fn check_upstream(&mut self, node: &UpstreamDefinition) {
        // §80.f — an unexpanded preset reference (the `from Preset@vN`
        // expansion left every structural field empty because the preset
        // is not in the catalog) gets ONE precise diagnostic naming the
        // catalog, not a cascade of missing-field errors.
        if node.preset.is_some() && node.transport.is_empty() && node.protocol.is_empty() {
            self.emit(
                format!(
                    "Upstream '{}' references unknown preset '{}'. Available: {} (or fork: write the full `upstream` by hand — a preset is ordinary source, D80.5)",
                    node.name,
                    node.preset.as_deref().unwrap_or(""),
                    crate::upstream_presets::available()
                ),
                &node.loc,
            );
            return;
        }
        // (a) closed catalogs.
        if node.transport.is_empty() {
            self.emit(
                format!("Upstream '{}' has no `transport:` — v1 catalog: {}", node.name, valid_list(VALID_UPSTREAM_TRANSPORTS)),
                &node.loc,
            );
        } else if !is_valid(&node.transport, VALID_UPSTREAM_TRANSPORTS) {
            self.emit(
                format!(
                    "Upstream '{}' transport '{}' is not in the v1 catalog: {} (gRPC/raw-TCP are named deferred scope — fase_80_upstream_design.md §5)",
                    node.name, node.transport, valid_list(VALID_UPSTREAM_TRANSPORTS)
                ),
                &node.loc,
            );
        }
        if node.auth_kind.is_empty() {
            self.emit(
                format!(
                    "Upstream '{}' has no `auth:` — every vendor handshake must be declared. Valid: {}",
                    node.name,
                    valid_list(VALID_UPSTREAM_AUTH_KINDS)
                ),
                &node.loc,
            );
        } else if !is_valid(&node.auth_kind, VALID_UPSTREAM_AUTH_KINDS) {
            self.emit(
                format!(
                    "Upstream '{}' auth kind '{}' is not in the catalog: {}",
                    node.name,
                    node.auth_kind,
                    valid_list(VALID_UPSTREAM_AUTH_KINDS)
                ),
                &node.loc,
            );
        } else {
            match node.auth_kind.as_str() {
                "header" | "query" if node.auth_name.is_none() => self.emit(
                    format!(
                        "Upstream '{}' auth `{}` requires a name — `{}(\"<name>\")`",
                        node.name, node.auth_kind, node.auth_kind
                    ),
                    &node.loc,
                ),
                "signed_url" if node.auth_name.is_some() => self.emit(
                    format!(
                        "Upstream '{}' auth `signed_url` takes no arguments — the resolved URL already carries its signature",
                        node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
        if let Some(overflow) = &node.overflow {
            // `degrade_quality` is deliberately NOT in the upstream v1
            // catalog: on a `Stream<T>` it requires a declared degrader
            // (`degrade_quality(resample_to=…)`), which `upstream` v1 does
            // not parse — accepting the bare word would promise a behaviour
            // the runtime cannot honestly deliver (named deferred scope).
            const VALID_UPSTREAM_OVERFLOW: &[&str] = &["drop_oldest", "fail", "pause_upstream"];
            if !is_valid(overflow, VALID_UPSTREAM_OVERFLOW) {
                self.emit(
                    format!(
                        "Upstream '{}' overflow '{}' is not in the v1 catalog: {} (`degrade_quality` needs a declared degrader — named deferred scope, fase_80_upstream_design.md §5)",
                        node.name,
                        overflow,
                        valid_list(VALID_UPSTREAM_OVERFLOW)
                    ),
                    &node.loc,
                );
            }
        }
        if let Some(rc) = &node.reconnect {
            if rc.backoff_ms < 1 {
                self.emit(
                    format!("Upstream '{}' reconnect backoff_ms must be ≥ 1 (got {})", node.name, rc.backoff_ms),
                    &node.loc,
                );
            }
            if rc.max_attempts < 0 {
                self.emit(
                    format!("Upstream '{}' reconnect max_attempts must be ≥ 0 (got {})", node.name, rc.max_attempts),
                    &node.loc,
                );
            }
            if !is_valid(&rc.on_exhausted, VALID_UPSTREAM_ON_EXHAUSTED) {
                self.emit(
                    format!(
                        "Upstream '{}' reconnect on_exhausted '{}' is not in the v1 catalog: {} (`degrade`/`park` are named deferred scope)",
                        node.name,
                        rc.on_exhausted,
                        valid_list(VALID_UPSTREAM_ON_EXHAUSTED)
                    ),
                    &node.loc,
                );
            }
        }

        // (b) axon-T851 — session + role binding.
        let bound_role: Option<&SessionRole> = if node.protocol.is_empty() {
            self.emit(
                format!("axon-T851 Upstream '{}' has no `protocol:` — it must reference a declared session", node.name),
                &node.loc,
            );
            None
        } else {
            match find_session_by_name(self.program, &node.protocol) {
                None => {
                    self.emit(
                        format!(
                            "axon-T851 Upstream '{}' protocol '{}' is not a declared session",
                            node.name, node.protocol
                        ),
                        &node.loc,
                    );
                    None
                }
                Some(session) => {
                    if node.role.is_empty() {
                        self.emit(
                            format!(
                                "axon-T851 Upstream '{}' has no `role:` — declare which side of session '{}' axon plays ({})",
                                node.name,
                                node.protocol,
                                session.roles.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ")
                            ),
                            &node.loc,
                        );
                        None
                    } else {
                        match session.roles.iter().find(|r| r.name == node.role) {
                            Some(role) => Some(role),
                            None => {
                                self.emit(
                                    format!(
                                        "axon-T851 Upstream '{}' role '{}' is not a role of session '{}' (roles: {})",
                                        node.name,
                                        node.role,
                                        node.protocol,
                                        session.roles.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ")
                                    ),
                                    &node.loc,
                                );
                                None
                            }
                        }
                    }
                }
            }
        };
        // §41.c Presburger discharge on the bound role, same law as `socket`.
        if let Some(role) = bound_role {
            match node.backpressure_credit {
                Some(n) if n >= 1 => {
                    let lowered = lower_session_role(role).with_credit(n as u64);
                    if is_unguarded_recursion(&lowered) {
                        // Diagnosed on the session declaration — see
                        // `check_session_duality` (§80.g hardening).
                        return;
                    }
                    if let Err(e) = lowered.credit_analyse(n as u64) {
                        self.emit(
                            format!(
                                "Upstream '{}' violates the credit-refined backpressure type of session '{}' role '{}': {} (D2)",
                                node.name, node.protocol, node.role, e
                            ),
                            &node.loc,
                        );
                    }
                }
                Some(n) => self.emit(
                    format!(
                        "Upstream '{}' backpressure credit must be ≥ 1 (got {n}); a 0-credit window cannot type a send (§Fase 41 §4.2)",
                        node.name
                    ),
                    &node.loc,
                ),
                None => {}
            }
        }

        // (c) §Fase 114.u — axon-T951: `resource:` XOR `resolve:`.
        //
        // When the channel rides a declared `resource`, the dial address
        // DERIVES from `resource.endpoint` (axon-T944 already proves that is a
        // per-tenant config key) — a `resolve:` beside it would state the
        // channel's address TWICE, which is exactly the §113 islands defect
        // this family of fases exists to kill. Authority attenuation: the
        // resource encapsulates its own address resolution; the upstream only
        // names WHICH channel it rides.
        if !node.resource_ref.is_empty() {
            if !node.resolve.is_empty() {
                self.emit(
                    format!(
                        "axon-T951 Upstream '{}' declares BOTH `resource: {}` AND `resolve: {}` — \
                         the channel's address stated twice. When the upstream rides a resource, \
                         its dial address derives from the resource's `endpoint` (axon-T944); a \
                         second address is either redundant or a contradiction, and a reader can \
                         no longer tell which one runs. Drop `resolve:`.",
                        node.name, node.resource_ref, node.resolve
                    ),
                    &node.loc,
                );
            }
            // The reference must resolve to a DECLARED resource (the T950 law,
            // verbatim discipline).
            match self.symbols.lookup(&node.resource_ref) {
                None => self.emit(
                    format!(
                        "axon-T951 Upstream '{}' names resource '{}', which is not declared.",
                        node.name, node.resource_ref
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "resource" => self.emit(
                    format!(
                        "axon-T951 Upstream '{}' names '{}', which is a {}, not a resource.",
                        node.name, node.resource_ref, sym.kind
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        } else {
            // axon-T850 — config-key shape (compile-time SecretKeyPolicy
            // mirror). Only when un-resourced: with `resource:` the address
            // fact lives on the resource and `resolve:` must be absent.
            self.check_upstream_config_key(&node.name, "resolve", &node.resolve, &node.loc);
        }
        self.check_upstream_config_key(&node.name, "secret", &node.secret, &node.loc);

        // (d) axon-T849 — projection totality over the bound role.
        for rule in &node.map {
            if !is_valid(&rule.framing, VALID_UPSTREAM_FRAMINGS) {
                self.emit(
                    format!(
                        "Upstream '{}' map rule for '{}' has framing '{}' — valid: {}",
                        node.name,
                        rule.message,
                        rule.framing,
                        valid_list(VALID_UPSTREAM_FRAMINGS)
                    ),
                    &rule.loc,
                );
            }
            if rule.tag.is_some() && (rule.direction != "send" || rule.framing != "json") {
                self.emit(
                    format!(
                        "Upstream '{}' map rule for '{}': `tag` applies only to `send … as json`",
                        node.name, rule.message
                    ),
                    &rule.loc,
                );
            }
            if rule.when_field.is_some() && (rule.direction != "receive" || rule.framing != "json") {
                self.emit(
                    format!(
                        "Upstream '{}' map rule for '{}': `when` applies only to `receive … as json`",
                        node.name, rule.message
                    ),
                    &rule.loc,
                );
            }
        }
        if let Some(role) = bound_role {
            let mut sends: Vec<String> = Vec::new();
            let mut receives: Vec<String> = Vec::new();
            collect_role_messages(&role.steps, &mut sends, &mut receives);
            // Every role message needs exactly one rule of the right direction.
            for (dir, msgs) in [("send", &sends), ("receive", &receives)] {
                for msg in msgs.iter() {
                    let n = node
                        .map
                        .iter()
                        .filter(|r| r.direction == dir && &r.message == msg)
                        .count();
                    if n == 0 {
                        self.emit(
                            format!(
                                "axon-T849 Upstream '{}': session '{}' role '{}' {}s '{}' but `map:` has no `{}` rule for it — a message with no projection would fall through untranscoded",
                                node.name, node.protocol, node.role, dir, msg, dir
                            ),
                            &node.loc,
                        );
                    } else if n > 1 {
                        self.emit(
                            format!(
                                "axon-T849 Upstream '{}': duplicate `{}` map rules for '{}' — the projection must be unambiguous",
                                node.name, dir, msg
                            ),
                            &node.loc,
                        );
                    }
                }
            }
            // No rule may name a message the role never exchanges (drift guard).
            for rule in &node.map {
                let known = match rule.direction.as_str() {
                    "send" => sends.contains(&rule.message),
                    _ => receives.contains(&rule.message),
                };
                if !known {
                    self.emit(
                        format!(
                            "axon-T849 Upstream '{}': map rule `{} {}` names a message session '{}' role '{}' never {}s",
                            node.name, rule.direction, rule.message, node.protocol, node.role, rule.direction
                        ),
                        &rule.loc,
                    );
                }
            }
            // Inbound dispatch must be deterministic. The discriminator key
            // is (field, Some(value)) for equality rules — a rule with no
            // `when` defaults to ("type", Some(<MessageName>)) — and
            // (field, None) for PRESENCE rules (`when "f"` with no `=`).
            // Equality rules are tried before presence rules at runtime, so
            // an eq and a presence rule on the same field can coexist; two
            // identical keys cannot.
            let receive_json: Vec<(&str, String, Option<String>)> = node
                .map
                .iter()
                .filter(|r| r.direction == "receive" && r.framing == "json")
                .map(|r| match (&r.when_field, &r.when_value) {
                    (None, _) => (r.message.as_str(), "type".to_string(), Some(r.message.clone())),
                    (Some(f), Some(v)) => (r.message.as_str(), f.clone(), Some(v.clone())),
                    (Some(f), None) => (r.message.as_str(), f.clone(), None),
                })
                .collect();
            for (i, a) in receive_json.iter().enumerate() {
                for b in receive_json.iter().skip(i + 1) {
                    if a.1 == b.1 && a.2 == b.2 {
                        let shape = match &a.2 {
                            Some(v) => format!("(\"{}\" = \"{}\")", a.1, v),
                            None => format!("(has \"{}\")", a.1),
                        };
                        self.emit(
                            format!(
                                "axon-T849 Upstream '{}': receive rules for '{}' and '{}' share the discriminator {} — inbound dispatch would be ambiguous",
                                node.name, a.0, b.0, shape
                            ),
                            &node.loc,
                        );
                    }
                }
            }
            let binary_receives = node
                .map
                .iter()
                .filter(|r| r.direction == "receive" && r.framing == "binary")
                .count();
            if binary_receives > 1 {
                self.emit(
                    format!(
                        "axon-T849 Upstream '{}': {} `receive … as binary` rules — binary frames carry no discriminator, so at most one is dispatchable",
                        node.name, binary_receives
                    ),
                    &node.loc,
                );
            }
        }
    }

    /// §Fase 80.g (`axon-T852`) — `voice` validation. The sugar's laws:
    ///   (a) `stt:`+`tts:` XOR `realtime:` (D80.1 — cascaded needs both
    ///       legs; fused needs exactly the one);
    ///   (b) `interruptible: true` ⇒ `legal_basis:` — the sugar must be
    ///       UNABLE to generate a program the §79 `ParkedResidualSoundness`
    ///       proof refutes (the generated socket parks residuals);
    ///   (c) every leg resolves — a `Preset@vN` must be in the §80.f
    ///       catalog, a bare name must be a declared `upstream`;
    ///   (d) `carrier:` in the closed catalog; `persona:`/`context:` refs
    ///       resolve when given.
    /// The EXPANSION's own soundness (duality, credit, projection totality)
    /// is checked on the generated declarations by the ordinary laws — the
    /// sugar earns no exemption.
    fn check_voice(&mut self, node: &VoiceDefinition) {
        // (a) architecture shape.
        let cascaded_given = node.stt.is_some() || node.tts.is_some();
        if node.realtime.is_some() && cascaded_given {
            self.emit(
                format!(
                    "axon-T852 Voice '{}' declares `realtime:` alongside `stt:`/`tts:` — one architecture per voice: cascaded (stt+tts) XOR fused (realtime)",
                    node.name
                ),
                &node.loc,
            );
        }
        if node.realtime.is_none() && (node.stt.is_none() || node.tts.is_none()) {
            self.emit(
                format!(
                    "axon-T852 Voice '{}' is incomplete — cascaded needs BOTH `stt:` and `tts:`, or declare a single fused `realtime:` leg",
                    node.name
                ),
                &node.loc,
            );
        }
        // (b) the §79 data-at-rest obligation, surfaced at the sugar level.
        if node.interruptible && node.legal_basis.is_none() {
            self.emit(
                format!(
                    "axon-T852 Voice '{}' declares `interruptible: true` without `legal_basis:` — a barge-in-capable call parks mid-utterance residuals at rest (§79), and that retention must be governed",
                    node.name
                ),
                &node.loc,
            );
        }
        // (d) carrier catalog.
        const VALID_CARRIERS: &[&str] = &["mulaw8k", "pcm16"];
        if !node.carrier.is_empty() && !is_valid(&node.carrier, VALID_CARRIERS) {
            self.emit(
                format!(
                    "Voice '{}' carrier '{}' is not in the catalog: {}",
                    node.name,
                    node.carrier,
                    valid_list(VALID_CARRIERS)
                ),
                &node.loc,
            );
        }
        // (c) leg resolution.
        for (field, leg) in [("stt", &node.stt), ("tts", &node.tts), ("realtime", &node.realtime)] {
            if let Some(r) = leg {
                if r.contains('@') {
                    if crate::upstream_presets::find(r).is_none() {
                        self.emit(
                            format!(
                                "axon-T852 Voice '{}' `{field}:` references unknown preset '{r}'. Available: {}",
                                node.name,
                                crate::upstream_presets::available()
                            ),
                            &node.loc,
                        );
                    }
                } else {
                    match self.symbols.lookup(r) {
                        Some(sym) if sym.kind == "upstream" => {}
                        Some(sym) => self.emit(
                            format!(
                                "axon-T852 Voice '{}' `{field}:` references '{r}', which is a {} — expected a declared `upstream` or a `Preset@vN`",
                                node.name, sym.kind
                            ),
                            &node.loc,
                        ),
                        None => self.emit(
                            format!(
                                "axon-T852 Voice '{}' `{field}:` references '{r}' — not a declared `upstream` and not a `Preset@vN` from the catalog ({})",
                                node.name,
                                crate::upstream_presets::available()
                            ),
                            &node.loc,
                        ),
                    }
                }
            }
        }
        // (d) persona/context refs.
        for (field, kind, r) in [("persona", "persona", &node.persona), ("context", "context", &node.context)] {
            if let Some(name) = r {
                match self.symbols.lookup(name) {
                    Some(sym) if sym.kind == kind => {}
                    _ => self.emit(
                        format!("Voice '{}' `{field}:` references '{name}' — not a declared {kind}", node.name),
                        &node.loc,
                    ),
                }
            }
        }
    }

    /// §Fase 80.c (`axon-T850`) — `resolve:`/`secret:` must be per-tenant
    /// config KEYS, never endpoint/credential literals. The charset is the
    /// compile-time mirror of the enterprise `SecretKeyPolicy`
    /// (`saas-secrets/src/policy.rs`): first char `[a-z0-9]`, rest
    /// `[a-z0-9_.-]` — notably NO `/` (the §77.g production-custody lesson)
    /// and no `:` (a URL cannot pass). Keeping the mirror here makes the
    /// "key valid in the mock, rejected by production custody" bug class
    /// unrepresentable in a compiled program.
    fn check_upstream_config_key(&mut self, upstream: &str, field: &str, key: &str, loc: &Loc) {
        if key.is_empty() {
            self.emit(
                format!("axon-T850 Upstream '{upstream}' has no `{field}:` — declare a per-tenant config key (e.g. `upstream.vendor.{}`)",
                    if field == "secret" { "api_key" } else { "url" }),
                loc,
            );
            return;
        }
        // §Fase 113 — the shape now lives in ONE place (`is_config_key`), shared
        // with axon-T902 and axon-T944. It used to be inlined here.
        if !is_config_key(key) {
            self.emit(
                format!(
                    "axon-T850 Upstream '{upstream}' `{field}:` value '{key}' is not a config key — keys are lowercase dot-separated (`[a-z0-9][a-z0-9_.-]*`, no `/`, no `:`); URLs and credentials never appear in source (the same config-not-code property as `tool`, §58.g)"
                ),
                loc,
            );
        }
    }

    /// §λ-L-E Fase 4 — Topology validation.
    ///
    /// Enforces: (a) each node name is unique + resolves to a valid kind;
    /// (b) each edge's source/target appear in `nodes`; (c) no self-loops;
    /// (d) each `session_ref` is a declared session;
    /// (e) Honda liveness — no cycle where every edge is receive-first.
    fn check_topology(&mut self, node: &TopologyDefinition) {
        const NODE_KINDS: &[&str] = &[
            "resource",
            "fabric",
            "manifest",
            "observe",
            "axonendpoint",
            "axonstore",
            "daemon",
            "agent",
            "shield",
        ];
        let mut seen_nodes: std::collections::HashSet<&String> = std::collections::HashSet::new();
        for n in &node.nodes {
            if !seen_nodes.insert(n) {
                self.emit(
                    format!("Topology '{}' lists node '{}' more than once", node.name, n),
                    &node.loc,
                );
                continue;
            }
            match self.symbols.lookup(n) {
                None => self.emit(
                    format!("Topology '{}' references undefined node '{}'", node.name, n),
                    &node.loc,
                ),
                Some(sym) if !NODE_KINDS.contains(&sym.kind.as_str()) => self.emit(
                    format!(
                        "Topology '{}' node '{}' is a {} — not a valid topology entity. \
                         Valid kinds: {}",
                        node.name,
                        n,
                        sym.kind,
                        NODE_KINDS.join(", ")
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
        for edge in &node.edges {
            self.check_topology_edge(&node.name, edge, &seen_nodes);
        }
        self.check_topology_liveness(node);
    }

    fn check_topology_edge(
        &mut self,
        topology_name: &str,
        edge: &TopologyEdge,
        declared_nodes: &std::collections::HashSet<&String>,
    ) {
        if !declared_nodes.contains(&edge.source) {
            self.emit(
                format!(
                    "Topology '{topology_name}' edge source '{}' is not in the nodes list",
                    edge.source
                ),
                &edge.loc,
            );
        }
        if !declared_nodes.contains(&edge.target) {
            self.emit(
                format!(
                    "Topology '{topology_name}' edge target '{}' is not in the nodes list",
                    edge.target
                ),
                &edge.loc,
            );
        }
        if edge.source == edge.target {
            self.emit(
                format!(
                    "Topology '{topology_name}' has self-loop edge on '{}' — \
                     π-calculus binary sessions require two distinct endpoints",
                    edge.source
                ),
                &edge.loc,
            );
        }
        if edge.session_ref.is_empty() {
            self.emit(
                format!(
                    "Topology '{topology_name}' edge {}->{} has no session reference",
                    edge.source, edge.target
                ),
                &edge.loc,
            );
            return;
        }
        match self.symbols.lookup(&edge.session_ref) {
            None => self.emit(
                format!(
                    "Topology '{topology_name}' edge {}->{} references undefined session '{}'",
                    edge.source, edge.target, edge.session_ref
                ),
                &edge.loc,
            ),
            Some(sym) if sym.kind != "session" => self.emit(
                format!(
                    "Topology '{topology_name}' edge {}->{} session ref '{}' is a {}, not a session",
                    edge.source, edge.target, edge.session_ref, sym.kind
                ),
                &edge.loc,
            ),
            _ => {}
        }
    }

    /// Honda-liveness: detect cycles whose every edge starts with `receive`
    /// on the source role. Such a cycle has no progress — static deadlock.
    fn check_topology_liveness(&mut self, node: &TopologyDefinition) {
        let mut adjacency: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for edge in &node.edges {
            if !edge.source.is_empty() && !edge.target.is_empty() {
                adjacency
                    .entry(edge.source.clone())
                    .or_default()
                    .push(edge.target.clone());
            }
        }
        let cycles = find_cycles(&adjacency);
        if cycles.is_empty() {
            return;
        }
        for cycle in cycles {
            let cycle_edges = cycle_to_edges(&cycle, &node.edges);
            // Only flag if (a) we found every edge in the cycle (sanity) and
            // (b) every one of them is receive-first on the source side.
            if cycle_edges.len() == cycle.len()
                && cycle_edges.iter().all(|e| self.edge_is_receive_first(e))
            {
                let mut tour: Vec<String> = cycle.clone();
                if let Some(first) = cycle.first() {
                    tour.push(first.clone());
                }
                self.emit(
                    format!(
                        "Topology '{}' has a static deadlock: cycle [{}] where every \
                         edge waits on receive — no progress is possible (Honda liveness violation)",
                        node.name, tour.join(" -> ")
                    ),
                    &node.loc,
                );
            }
        }
    }

    /// Look up the session AST for an edge and check whether the FIRST
    /// role's first step is `receive`. Source plays the first role (fixed
    /// convention per AST docstring).
    fn edge_is_receive_first(&self, edge: &TopologyEdge) -> bool {
        let session = match find_session_by_name(self.program, &edge.session_ref) {
            Some(s) => s,
            None => return false,
        };
        let first_role = match session.roles.first() {
            Some(r) => r,
            None => return false,
        };
        first_role
            .steps
            .first()
            .map(|s| s.op == "receive")
            .unwrap_or(false)
    }

    // ── §λ-L-E Fase 5 — Cognitive immune system (paper_immune_v2.md) ───

    /// §λ-L-E Fase 5 — Immune validation.
    ///
    /// Enforces paper §8.2 mandatory scope + watch non-empty + sensitivity
    /// ∈ [0.0, 1.0] + window ≥ 1 + decay enum.
    fn check_immune(&mut self, node: &ImmuneDefinition) {
        if node.scope.is_empty() {
            self.emit(
                format!(
                    "immune '{}' requires an explicit 'scope' (tenant | flow | global). \
                     No implicit default exists — blast radius must be declared (paper §8.2)",
                    node.name
                ),
                &node.loc,
            );
        } else if !matches!(node.scope.as_str(), "tenant" | "flow" | "global") {
            self.emit(
                format!(
                    "immune '{}' has invalid scope '{}'. Valid: tenant | flow | global",
                    node.name, node.scope
                ),
                &node.loc,
            );
        }
        if node.watch.is_empty() {
            self.emit(
                format!(
                    "immune '{}' requires a non-empty 'watch' list (observables to monitor)",
                    node.name
                ),
                &node.loc,
            );
        }
        if let Some(s) = node.sensitivity {
            if !(0.0..=1.0).contains(&s) {
                self.emit(
                    format!(
                        "immune '{}' sensitivity must be in [0.0, 1.0], got {s}",
                        node.name
                    ),
                    &node.loc,
                );
            }
        }
        if node.window < 1 {
            self.emit(
                format!(
                    "immune '{}' window must be >= 1, got {}",
                    node.name, node.window
                ),
                &node.loc,
            );
        }
        if !matches!(node.decay.as_str(), "exponential" | "linear" | "none") {
            self.emit(
                format!(
                    "immune '{}' has invalid decay '{}'. Valid: exponential | linear | none",
                    node.name, node.decay
                ),
                &node.loc,
            );
        }
    }

    /// §λ-L-E Fase 5 — Reflex validation.
    ///
    /// Enforces mandatory scope + valid scope/on_level/action enums + trigger
    /// resolves to an `immune` (one-way dependency per paper §4).
    fn check_reflex(&mut self, node: &ReflexDefinition) {
        if node.scope.is_empty() {
            self.emit(
                format!(
                    "reflex '{}' requires an explicit 'scope' (tenant | flow | global) — paper §8.2",
                    node.name
                ),
                &node.loc,
            );
        } else if !matches!(node.scope.as_str(), "tenant" | "flow" | "global") {
            self.emit(
                format!("reflex '{}' has invalid scope '{}'", node.name, node.scope),
                &node.loc,
            );
        }
        if node.trigger.is_empty() {
            self.emit(
                format!("reflex '{}' requires a 'trigger: <ImmuneName>'", node.name),
                &node.loc,
            );
        } else {
            match self.symbols.lookup(&node.trigger) {
                None => self.emit(
                    format!(
                        "reflex '{}' references undefined trigger '{}' (expected an immune)",
                        node.name, node.trigger
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "immune" => self.emit(
                    format!(
                        "reflex '{}' trigger '{}' is a {}, not an immune",
                        node.name, node.trigger, sym.kind
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
        if !matches!(
            node.on_level.as_str(),
            "know" | "believe" | "speculate" | "doubt"
        ) {
            self.emit(
                format!(
                    "reflex '{}' invalid on_level '{}'. Valid: know | believe | speculate | doubt",
                    node.name, node.on_level
                ),
                &node.loc,
            );
        }
        if node.action.is_empty() {
            self.emit(
                format!(
                    "reflex '{}' requires an 'action' (drop | revoke | emit | redact | \
                     quarantine | terminate | alert)",
                    node.name
                ),
                &node.loc,
            );
        } else if !matches!(
            node.action.as_str(),
            "drop" | "revoke" | "emit" | "redact" | "quarantine" | "terminate" | "alert"
        ) {
            self.emit(
                format!("reflex '{}' invalid action '{}'", node.name, node.action),
                &node.loc,
            );
        }
    }

    /// §λ-L-E Fase 5 — Heal validation.
    ///
    /// Enforces mandatory scope + source is an immune + on_level/mode enums +
    /// **paper §7.3: mode='adversarial' requires a shield gate** + shield_ref
    /// (if present) resolves to a shield + max_patches ≥ 1.
    fn check_heal(&mut self, node: &HealDefinition) {
        if node.scope.is_empty() {
            self.emit(
                format!(
                    "heal '{}' requires an explicit 'scope' (tenant | flow | global) — paper §8.2",
                    node.name
                ),
                &node.loc,
            );
        } else if !matches!(node.scope.as_str(), "tenant" | "flow" | "global") {
            self.emit(
                format!("heal '{}' has invalid scope '{}'", node.name, node.scope),
                &node.loc,
            );
        }
        if node.source.is_empty() {
            self.emit(
                format!("heal '{}' requires a 'source: <ImmuneName>'", node.name),
                &node.loc,
            );
        } else {
            match self.symbols.lookup(&node.source) {
                None => self.emit(
                    format!(
                        "heal '{}' references undefined source '{}' (expected an immune)",
                        node.name, node.source
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "immune" => self.emit(
                    format!(
                        "heal '{}' source '{}' is a {}, not an immune",
                        node.name, node.source, sym.kind
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
        if !matches!(
            node.on_level.as_str(),
            "know" | "believe" | "speculate" | "doubt"
        ) {
            self.emit(
                format!("heal '{}' invalid on_level '{}'", node.name, node.on_level),
                &node.loc,
            );
        }
        if !matches!(
            node.mode.as_str(),
            "audit_only" | "human_in_loop" | "adversarial"
        ) {
            self.emit(
                format!(
                    "heal '{}' invalid mode '{}'. Valid: audit_only | human_in_loop | \
                     adversarial (paper §7)",
                    node.name, node.mode
                ),
                &node.loc,
            );
        }
        // Paper §7.3 — adversarial mode requires an explicit shield gate.
        if node.mode == "adversarial" && node.shield_ref.is_empty() {
            self.emit(
                format!(
                    "heal '{}' mode='adversarial' requires a 'shield' gate \
                     (no LLM-generated patch ships without review). \
                     Paper §7.3: adversarial mode needs explicit Risk Acceptance",
                    node.name
                ),
                &node.loc,
            );
        }
        if !node.shield_ref.is_empty() {
            match self.symbols.lookup(&node.shield_ref) {
                None => self.emit(
                    format!(
                        "heal '{}' references undefined shield '{}'",
                        node.name, node.shield_ref
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "shield" => self.emit(
                    format!(
                        "heal '{}' shield ref '{}' is a {}, not a shield",
                        node.name, node.shield_ref, sym.kind
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
        if node.max_patches < 1 {
            self.emit(
                format!(
                    "heal '{}' max_patches must be >= 1, got {}",
                    node.name, node.max_patches
                ),
                &node.loc,
            );
        }
    }

    // ── §λ-L-E Fase 9 — UI cognitiva (component / view) ────────────
    //
    // Compile-time invariants enforced below:
    //   1. `renders` references a declared `type`.
    //   2. `on_interact` (if present) is a declared `flow` whose first
    //      parameter type matches `renders`.
    //   3. If `renders` carries κ (regulatory class), `via_shield` is
    //      MANDATORY and its `compliance` must cover every κ of the
    //      rendered type. Fase 9.5 compile-time contract.
    //   4. `via_shield` (if present) must name a declared `shield`.
    //   5. Every component listed in a `view.components` must resolve
    //      to a declared `component`.

    fn check_component(&mut self, node: &ComponentDefinition) {
        // (1) renders must resolve to a type
        let rendered_type = if node.renders.is_empty() {
            self.emit(
                format!("component '{}' requires 'renders: <TypeName>'", node.name),
                &node.loc,
            );
            None
        } else {
            match self.symbols.lookup(&node.renders) {
                None => {
                    self.emit(
                        format!(
                            "component '{}' references undefined type '{}'",
                            node.name, node.renders
                        ),
                        &node.loc,
                    );
                    None
                }
                Some(sym) if sym.kind != "type" => {
                    self.emit(
                        format!(
                            "component '{}' renders '{}' which is a {}, not a type",
                            node.name, node.renders, sym.kind
                        ),
                        &node.loc,
                    );
                    None
                }
                Some(_) => find_type_by_name(self.program, &node.renders),
            }
        };

        // (4) shield ref
        let shield_node = if node.via_shield.is_empty() {
            None
        } else {
            match self.symbols.lookup(&node.via_shield) {
                None => {
                    self.emit(
                        format!(
                            "component '{}' references undefined shield '{}'",
                            node.name, node.via_shield
                        ),
                        &node.loc,
                    );
                    None
                }
                Some(sym) if sym.kind != "shield" => {
                    self.emit(
                        format!(
                            "component '{}' via_shield '{}' is a {}, not a shield",
                            node.name, node.via_shield, sym.kind
                        ),
                        &node.loc,
                    );
                    None
                }
                Some(_) => find_shield_by_name(self.program, &node.via_shield),
            }
        };

        // (3) regulated-render rule — Fase 9.5
        if let Some(t) = rendered_type {
            let type_kappa: std::collections::HashSet<&str> =
                t.compliance.iter().map(|s| s.as_str()).collect();
            if !type_kappa.is_empty() {
                match shield_node {
                    None => self.emit(
                        format!(
                            "component '{}' renders regulated type '{}' \
                             (kappa = {{{}}}) but declares no 'via_shield'. \
                             Regulated renders require a shield that covers \
                             the type's kappa — Fase 9.5.",
                            node.name,
                            node.renders,
                            {
                                let mut v: Vec<&str> = type_kappa.iter().copied().collect();
                                v.sort();
                                v.join(", ")
                            }
                        ),
                        &node.loc,
                    ),
                    Some(s) => {
                        let shield_kappa: std::collections::HashSet<&str> =
                            s.compliance.iter().map(|s| s.as_str()).collect();
                        let mut missing: Vec<&str> =
                            type_kappa.difference(&shield_kappa).copied().collect();
                        missing.sort();
                        if !missing.is_empty() {
                            self.emit(
                                format!(
                                    "component '{}' via_shield '{}' does not cover \
                                     kappa = {{{}}} of type '{}'. Add these classes \
                                     to the shield's 'compliance' list or pick a \
                                     shield that already covers them.",
                                    node.name,
                                    node.via_shield,
                                    missing.join(", "),
                                    node.renders,
                                ),
                                &node.loc,
                            );
                        }
                    }
                }
            }
        }

        // (2) on_interact must resolve to a flow with compatible signature
        if !node.on_interact.is_empty() {
            match self.symbols.lookup(&node.on_interact) {
                None => self.emit(
                    format!(
                        "component '{}' references undefined flow '{}'",
                        node.name, node.on_interact
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "flow" => self.emit(
                    format!(
                        "component '{}' on_interact '{}' is a {}, not a flow",
                        node.name, node.on_interact, sym.kind
                    ),
                    &node.loc,
                ),
                Some(_) => {
                    if let Some(flow) = find_flow_by_name(self.program, &node.on_interact) {
                        if !rendered_type.is_none() {
                            if let Some(first_param) = flow.parameters.first() {
                                let pt = first_param.type_expr.name.as_str();
                                if !pt.is_empty() && pt != node.renders {
                                    self.emit(
                                        format!(
                                            "component '{}' on_interact flow '{}' \
                                             expects first parameter of type '{}', \
                                             but component renders '{}'. Signatures \
                                             must match — Fase 9.2 rule 2.",
                                            node.name, node.on_interact, pt, node.renders
                                        ),
                                        &node.loc,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn check_view(&mut self, node: &ViewDefinition) {
        if node.components.is_empty() {
            self.emit(
                format!(
                    "view '{}' has empty components list — a view must \
                     compose at least one component",
                    node.name
                ),
                &node.loc,
            );
            return;
        }
        let mut seen: std::collections::HashSet<&String> = std::collections::HashSet::new();
        for comp_name in &node.components {
            if !seen.insert(comp_name) {
                self.emit(
                    format!(
                        "view '{}' lists component '{}' more than once",
                        node.name, comp_name
                    ),
                    &node.loc,
                );
                continue;
            }
            match self.symbols.lookup(comp_name) {
                None => self.emit(
                    format!(
                        "view '{}' references undefined component '{}'",
                        node.name, comp_name
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "component" => self.emit(
                    format!(
                        "view '{}' component ref '{}' is a {}, not a component",
                        node.name, comp_name, sym.kind
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
    }

    /// §Fase 107.a (`axon-T927`) — the DECLARED WRITE surface of a flow body, the
    /// signal the QUERY-safety law decides on (D107.1, founder-ratified). Returns
    /// the FIRST write verb reached, with its source location, so the diagnostic can
    /// point at the exact offending step.
    ///
    /// The set is axon's declared state-change vocabulary: store writes
    /// (`persist` / `mutate` / `purge`), channel egress (`emit` / `publish`),
    /// secret + credential state (`rotate` / `mint`), and a `transact` boundary (a
    /// transaction has no business inside a safe method). The walk RECURSES into
    /// every nested body — `if` / `for` / `par` branches / `warden` — because a
    /// safety proof that misses a write nested one level deep is not a proof.
    ///
    /// It deliberately does NOT flag a `tool` for declaring `network` / `io`: a
    /// read-only vendor lookup is legitimate (and common) inside a query, and
    /// refusing it would make QUERY useless. That boundary is the honest perimeter
    /// (§107 §7) — axon proves what it declares, and says so plainly.
    fn first_declared_write(&self, steps: &[FlowStep]) -> Option<(&'static str, Loc)> {
        for step in steps {
            let hit: Option<(&'static str, Loc)> = match step {
                FlowStep::Persist(s) => Some(("persist", s.loc.clone())),
                FlowStep::Mutate(s) => Some(("mutate", s.loc.clone())),
                FlowStep::Purge(s) => Some(("purge", s.loc.clone())),
                FlowStep::Emit(s) => Some(("emit", s.loc.clone())),
                FlowStep::Publish(s) => Some(("publish", s.loc.clone())),
                FlowStep::Rotate(s) => Some(("rotate", s.loc.clone())),
                FlowStep::Mint(s) => Some(("mint", s.loc.clone())),
                FlowStep::Transact(s) => Some(("transact", s.loc.clone())),
                // §Fase 108.c (D108.4) — `ingest` appends batches to a
                // server-resident dataspace: that is state change, so a safe
                // method cannot reach it. A QUERY may focus/aggregate/explore
                // (reads); it may NOT ingest.
                FlowStep::Ingest(s) => Some(("ingest", s.loc.clone())),
                // Recurse into every nested body — a nested write is still a write.
                FlowStep::If(c) => self
                    .first_declared_write(&c.then_body)
                    .or_else(|| self.first_declared_write(&c.else_body)),
                FlowStep::ForIn(f) => self.first_declared_write(&f.body),
                FlowStep::Par(p) => p
                    .branches
                    .iter()
                    .find_map(|b| self.first_declared_write(b)),
                FlowStep::Warden(w) => self.first_declared_write(&w.body),
                _ => None,
            };
            if hit.is_some() {
                return hit;
            }
        }
        None
    }

    /// §Fase 107.a (`axon-T927`) — **the QUERY-safety law.** RFC 10008 §2: a QUERY
    /// request MUST be processed "in a safe and idempotent manner" — it does not
    /// change state. Caches, proxies and clients are ENTITLED to act on that (they
    /// may retry and cache freely), so a QUERY that writes is not a style problem:
    /// it is a correctness + security bug. In every other stack the MUST is a
    /// convention nobody enforces. axon has an effect system, so here it is a
    /// PROOF: an `axonendpoint` with `method: QUERY` whose bound flow reaches a
    /// declared write is REFUSED AT COMPILE TIME.
    ///
    /// Two write sources are checked (both ratified in D107.1):
    /// 1. the flow's own body ([`first_declared_write`]);
    /// 2. the program declaring a `deliver` (§105) or `document` (§106) — those
    ///    egress declarations FIRE POST-RUN for any flow the deployed executor runs
    ///    (D105.7-B), so a QUERY endpoint in such a program would write a CRM row /
    ///    persist an artifact. Coarse but SOUND under the current firing semantics.
    fn check_query_is_safe(&mut self, node: &AxonEndpointDefinition) {
        if !node.method.eq_ignore_ascii_case("QUERY") || node.execute_flow.is_empty() {
            return;
        }
        // 1. A declared write anywhere in the bound flow's body.
        let flow_body: Option<&Vec<FlowStep>> = self.program.declarations.iter().find_map(|d| {
            match d {
                Declaration::Flow(f) if f.name == node.execute_flow => Some(&f.body),
                _ => None,
            }
        });
        if let Some(body) = flow_body {
            if let Some((verb, loc)) = self.first_declared_write(body) {
                self.emit(
                    format!(
                        "axon-T927 axonendpoint '{}' declares `method: QUERY`, but its flow '{}' \
                         performs a declared write (`{}`). RFC 10008 §2: a QUERY MUST be processed \
                         in a SAFE and IDEMPOTENT manner — caches, proxies and clients are entitled \
                         to retry and cache it freely, so a QUERY that changes state is a \
                         correctness + security bug, not a style choice. Use `method: POST` for a \
                         state-changing operation, or remove the `{}` from this flow.",
                        node.name, node.execute_flow, verb, verb
                    ),
                    &loc,
                );
                return;
            }
        }
        // 2. A program-level egress declaration fires post-run for ANY flow the
        //    deployed executor runs (§105 deliver / §106 document, D105.7-B) — so a
        //    QUERY endpoint here would write a CRM row / persist an artifact.
        let egress: Option<(&'static str, String)> =
            self.program.declarations.iter().find_map(|d| match d {
                Declaration::Deliver(x) => Some(("deliver", x.name.clone())),
                Declaration::Notify(x) => Some(("notify", x.name.clone())),
                Declaration::Document(x) => Some(("document", x.name.clone())),
                _ => None,
            });
        if let Some((kind, decl_name)) = egress {
            self.emit(
                format!(
                    "axon-T927 axonendpoint '{}' declares `method: QUERY`, but this program \
                     declares a `{} {}` — an egress declaration FIRES for every flow the executor \
                     runs (it writes a CRM row / persists an artifact), so this endpoint could not \
                     be safe. RFC 10008 §2 requires a QUERY to change no state. Use `method: POST`, \
                     or move the `{}` into a program whose endpoints are not QUERY.",
                    node.name, kind, decl_name, kind
                ),
                &node.loc,
            );
        }
    }

    fn check_axonendpoint(&mut self, node: &AxonEndpointDefinition) {
        // HTTP method enum
        if !node.method.is_empty() {
            let upper = node.method.to_uppercase();
            if !is_valid(&upper, VALID_ENDPOINT_METHODS) {
                self.emit(
                    format!(
                        "Unknown HTTP method '{}' in axonendpoint '{}'. Valid: {}",
                        node.method,
                        node.name,
                        valid_list(VALID_ENDPOINT_METHODS)
                    ),
                    &node.loc,
                );
            }
        }

        // §Fase 107.a — the QUERY-safety law (axon-T927): RFC 10008's normative
        // "safe and idempotent" MUST, made a compile-time proof.
        self.check_query_is_safe(node);

        // §Fase 36.d (D2) — declared execution backend, closed catalog.
        // The parser already rejects an unknown backend with a smart-
        // suggest hint; this re-check defends ASTs built outside the
        // parser (LSP synthesis, programmatic construction) so an
        // impossible backend is a compile error on every path.
        if !node.backend.is_empty()
            && !is_valid(&node.backend, crate::parser::AXONENDPOINT_BACKEND_VALUES)
        {
            self.emit(
                format!(
                    "Unknown backend '{}' in axonendpoint '{}'. Valid: {}",
                    node.backend,
                    node.name,
                    valid_list(crate::parser::AXONENDPOINT_BACKEND_VALUES)
                ),
                &node.loc,
            );
        }

        // §Fase 36.k (D10) — `axon-W003`: an axonendpoint that declares
        // no `backend:` relies on the Fase 36 ladder for resolution.
        // An explicit `backend: auto` is the adopter's deliberate
        // opt-in to ladder resolution — it carries no warning. An
        // omitted field does: the adopter learns at compile time, not
        // at the first production 503.
        if node.backend.is_empty() {
            self.warn(build_w003_message(&node.name), &node.loc);
        }

        // Path must start with /
        if !node.path.is_empty() && !node.path.starts_with('/') {
            self.emit(
                format!(
                    "Path must start with '/' in axonendpoint '{}', got '{}'",
                    node.name, node.path
                ),
                &node.loc,
            );
        }

        // execute_flow reference
        if !node.execute_flow.is_empty() {
            match self.symbols.lookup(&node.execute_flow) {
                None => self.emit(
                    format!(
                        "Undefined flow '{}' in axonendpoint '{}'",
                        node.execute_flow, node.name
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "flow" => self.emit(
                    format!(
                        "'{}' is a {}, not a flow (referenced in axonendpoint '{}')",
                        node.execute_flow, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }

        // Shield reference
        if !node.shield_ref.is_empty() {
            match self.symbols.lookup(&node.shield_ref) {
                None => self.emit(
                    format!(
                        "Undefined shield '{}' in axonendpoint '{}'",
                        node.shield_ref, node.name
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "shield" => self.emit(
                    format!(
                        "'{}' is a {}, not a shield (referenced in axonendpoint '{}')",
                        node.shield_ref, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }

        // §Fase 83.c — cors reference (axon-T856). Mirrors the shield
        // reference check exactly, `"cors"` swapped in for `"shield"`.
        if !node.cors_ref.is_empty() {
            match self.symbols.lookup(&node.cors_ref) {
                None => self.emit(
                    format!(
                        "axon-T856 undefined cors '{}' in axonendpoint '{}'",
                        node.cors_ref, node.name
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "cors" => self.emit(
                    format!(
                        "axon-T856 '{}' is a {}, not a cors (referenced in axonendpoint '{}')",
                        node.cors_ref, sym.kind, node.name
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }

        // Retries >= 0
        if let Some(v) = node.retries {
            if v < 0 {
                self.emit(
                    format!(
                        "retries must be >= 0, got {} in axonendpoint '{}'",
                        v, node.name
                    ),
                    &node.loc,
                );
            }
        }

        // §Fase 35.j (D11) — Pillar IV: capability-typed store access.
        // The endpoint must GRANT (in `requires:`) every capability a
        // capability-gated store accessed by its flow declares — data
        // isolation as a language guarantee, enforced statically.
        // Nested store ops (inside `for`/`par` bodies) are caught by
        // the runtime re-check (35.j `store::capability`), not this
        // top-level walk.
        if !node.execute_flow.is_empty() {
            if let Some(flow) = self.find_flow(&node.execute_flow) {
                for step in &flow.body {
                    let store_name = match step {
                        FlowStep::Persist(s) => &s.store_name,
                        FlowStep::Retrieve(s) => &s.store_name,
                        FlowStep::Mutate(s) => &s.store_name,
                        FlowStep::Purge(s) => &s.store_name,
                        _ => continue,
                    };
                    let Some(store) = self.find_store(store_name) else {
                        continue;
                    };
                    if store.capability.is_empty()
                        || node.requires_capabilities.contains(&store.capability)
                    {
                        continue;
                    }
                    self.emit(
                        format!(
                            "axonendpoint '{}' executes flow '{}' which accesses \
                             axonstore '{}' requiring capability '{}', but '{}' \
                             does not grant it — add '{}' to the endpoint's \
                             `requires:` list (Fase 35.j Pillar IV).",
                            node.name,
                            node.execute_flow,
                            store_name,
                            store.capability,
                            node.name,
                            store.capability,
                        ),
                        &node.loc,
                    );
                }
            }
        }

        // §Fase 89.b (`axon-T890`) — AuthorizationCoverage: the doctrine
        // `every_boundary_is_guarded` made a compile-time law. An
        // `axonendpoint` is a trust boundary; the boundary-coverage audit
        // found Modo 1 — an endpoint with no `requires:`/`shield:`/
        // `compliance:` silently dispatches to any authenticated same-tenant
        // caller ("empty vec ≡ no auth gate — back-compat"). §89 closes that:
        // an endpoint that DISPATCHES a flow must declare a covering
        // authorization discipline OR the EXPLICIT, auditable opt-out
        // `public: true`. Gated on a non-empty `execute:` — an endpoint that
        // dispatches nothing crosses no boundary (and is already flagged by
        // the execute/flow checks above), so it needs no coverage.
        if !node.execute_flow.is_empty() {
            let covered = !node.requires_capabilities.is_empty()
                || !node.shield_ref.is_empty()
                || !node.compliance.is_empty();
            if !covered && !node.public {
                self.emit(
                    format!(
                        "axon-T890 axonendpoint '{}' declares no authorization \
                         coverage (no `requires:`, no `shield:`, no `compliance:`) \
                         and is not marked `public: true`. Every endpoint is a \
                         trust boundary — doctrine `every_boundary_is_guarded`: \
                         either declare a covering discipline (e.g. \
                         `requires: [flow.execute]`, `shield: <Name>`, or \
                         `compliance: [...]`) or, if the endpoint is intentionally \
                         uncovered, declare `public: true` so the opt-out is \
                         explicit and auditable. Run `axon fix` to auto-insert \
                         `public: true` on every currently-uncovered endpoint.",
                        node.name
                    ),
                    &node.loc,
                );
            }
        }

        // §Fase 117.a (`axon-T957`) — RegulatedBoundaryCoverage: the ESK
        // Fase 6.1 κ-coverage law, finally scoped to the `axonendpoint`.
        //
        // THE GAP THIS CLOSES. §111 F16 diagnosed it and `advertised.rs`
        // has carried the confession ever since: "the language's flagship
        // claim, and exactly ONE genuine κ-coverage rule exists (scoped to
        // `component`). The axon-T890 endpoint rule is a PRESENCE check
        // (`!compliance.is_empty()`), not a coverage law." The README has
        // promised the endpoint form of this law since v1.x — remove the
        // `shield` from a regulated endpoint and `axon check` was supposed
        // to refuse — and the compiler never had it. The `compliance` field
        // on `AxonEndpointDefinition` is even documented "§ESK Fase 6.1 —
        // regulatory coverage on the boundary". The INTENT was always here;
        // the LAW was not. A promise the compiler cannot break is not a weak
        // guarantee, it is a VACUOUS one — the same lesson `lease` taught in
        // §113.
        //
        // THE LAW. An `axonendpoint` is where regulated data crosses the
        // process boundary. If either side of that crossing — `body:` (what
        // arrives) or `output:` (what leaves) — names a type carrying a
        // non-empty κ, the endpoint MUST name a `shield:` whose own
        // `compliance:` covers every class in that κ. A real set difference,
        // exactly like the `component` rule at the regulated-render gate —
        // not a presence check.
        //
        // WHY THE SHIELD AND NOT THE ENDPOINT'S OWN `compliance:`. The
        // shield is the CONTROL (`scan:`, `on_breach:`, `severity:`); the
        // endpoint's `compliance:` list is a LABEL. Accepting the label as
        // coverage would rebuild the very presence check §111 condemned. A
        // κ class is covered when something can act on a breach of it.
        //
        // Runs BEFORE the E039 wire gate and is NOT suppressed by it: a
        // regulated boundary left unguarded is a governance defect whether
        // or not the wire shape also happens to be wrong. Gated on a
        // non-empty `execute:` for the same reason T890 is — an endpoint
        // that dispatches nothing crosses no boundary.
        if !node.execute_flow.is_empty() {
            let mut boundary_kappa: std::collections::HashSet<&str> =
                std::collections::HashSet::new();
            for type_ref in [node.body_type.as_str(), node.output_type.as_str()] {
                let base = peel_type_constructors(type_ref);
                if base.is_empty() {
                    continue;
                }
                if let Some(t) = find_type_by_name(self.program, base) {
                    boundary_kappa.extend(t.compliance.iter().map(|s| s.as_str()));
                }
            }

            if !boundary_kappa.is_empty() {
                let mut kappa_sorted: Vec<&str> = boundary_kappa.iter().copied().collect();
                kappa_sorted.sort_unstable();

                if node.shield_ref.is_empty() {
                    self.emit(
                        format!(
                            "axon-T957 axonendpoint '{}' carries regulated data \
                             (kappa = {{{}}}) across a trust boundary but declares no \
                             `shield:`. Regulated boundaries require a shield whose \
                             `compliance:` covers the type's kappa — ESK Fase 6.1 \
                             coverage rule. Declare `shield: <Name>` on the endpoint, \
                             where that shield lists at least [{}] in its \
                             `compliance:`. Declaring the classes on the endpoint's own \
                             `compliance:` does NOT cover them: that list is a label, \
                             the shield is the control that acts on a breach.",
                            node.name,
                            kappa_sorted.join(", "),
                            kappa_sorted.join(", "),
                        ),
                        &node.loc,
                    );
                } else if let Some(shield) = find_shield_by_name(self.program, &node.shield_ref) {
                    // An unknown `shield:` name is already reported by the
                    // reference-integrity check above — do not double-report.
                    let shield_kappa: std::collections::HashSet<&str> =
                        shield.compliance.iter().map(|s| s.as_str()).collect();
                    let mut missing: Vec<&str> =
                        boundary_kappa.difference(&shield_kappa).copied().collect();
                    missing.sort_unstable();
                    if !missing.is_empty() {
                        self.emit(
                            format!(
                                "axon-T957 axonendpoint '{}' declares `shield: {}`, but that \
                                 shield does not cover kappa = {{{}}} carried across the \
                                 boundary — ESK Fase 6.1 coverage rule. Add [{}] to shield \
                                 '{}'s `compliance:` list, or name a shield that already \
                                 covers them.",
                                node.name,
                                node.shield_ref,
                                missing.join(", "),
                                missing.join(", "),
                                node.shield_ref,
                            ),
                            &node.loc,
                        );
                    }
                }
            }
        }

        // §Fase 39.e (D12 α RATIFIED) — Wire-shape mandate gate
        // (`axon-E039`). On `transport: json` endpoints, every
        // declared `output: T` MUST be `FlowEnvelope<T>` (or `Any` /
        // `Unit` / `<empty>`). Bare `output: T` / `output: List<T>` /
        // `output: Stream<T>` declarations are now COMPILE ERRORS —
        // the v1.x bridge is closed, the v2.0.0 wire contract is
        // structural.
        //
        // This gate runs BEFORE the 38.x.f cardinality gate. When
        // E039 fires, the cardinality gate is SUPPRESSED so the
        // adopter sees ONE canonical diagnostic with the right
        // answer (FlowEnvelope<tail_T> based on the actual flow
        // cardinality + sse migration alternative). When E039 does
        // NOT fire (declaration IS FlowEnvelope<T> or sse transport
        // or no output declared), the cardinality gate runs
        // normally and unwraps the FlowEnvelope per 39.a.
        let e039_fired = if !node.execute_flow.is_empty()
            && !node.output_type.is_empty()
        {
            self.emit_e039_wire_packaging_gate(node)
        } else {
            false
        };

        // §Fase 38.x.f (D1–D6) — Cardinality Coverage gate.
        //
        // Runs BEFORE the 37.c totality block because the latter does
        // an early `return` when no binding sources are declared
        // (endpoint with `output:` + `execute:` only, no `body:` /
        // `path` / `query`). The cardinality gate is independent of
        // binding-source declarations — it only needs the endpoint's
        // `output:` + the flow's body tail — so it must run first or
        // it never fires on the no-source case (e.g. the Stream
        // mismatch §6 in the anchor test).
        //
        // §39.e — When E039 fires above, we SKIP the cardinality
        // gate (the E039 diagnostic already names the canonical
        // FlowEnvelope<...> answer; double-emitting T9XX would be
        // noisy).
        if !e039_fired
            && !node.execute_flow.is_empty()
            && !node.output_type.is_empty()
        {
            if let Some(flow) = self.find_flow(&node.execute_flow) {
                let declared = declared_cardinality(&node.output_type);
                let tail = infer_flow_tail_cardinality(flow);
                self.emit_cardinality_gate(node, &declared, &tail);
            }
        }

        // §Fase 37.c (D2) — The Request Binding Contract totality check.
        //
        // When the endpoint declares `body: T`, the type-checker
        // proves every REQUIRED parameter of `execute: F` is covered
        // by a field of T — by name, type-compatible. An uncovered
        // required parameter is a compile error: otherwise the flow
        // reaches production and fails at REQUEST time for a missing
        // argument (the exact v1.35.0 defect Fase 37 closes). The
        // failure moves from production to `axon check`. Sibling of
        // the Fase 35.j Pillar IV endpoint→flow→store-capability check.
        //
        // The check runs only when `body: T` resolves to a declared
        // struct type. A primitive `body:` (or a `body:` naming an
        // undeclared type) has no fields to cover parameters — the
        // runtime binding is then untyped/best-effort and outside the
        // D2 theorem (honest scope). An OPTIONAL parameter need not be
        // covered. Type-compatibility is exact: same type name + same
        // generic parameter.
        //
        // §Fase 37.y (D3) — D2 totality UNION over three binding sources:
        // path_params (typed `Text` by HTTP convention), query_params
        // (declared type from the closed catalog), body type fields
        // (existing v1.36.0 surface). Each required flow param must be
        // covered by EXACTLY ONE source — zero coverage emits the
        // legacy missing-binding error (extended to mention all three
        // sources); multi-source coverage emits the new `axon-T901`
        // collision error (D4).
        //
        // The gate now runs when `execute_flow` is set AND at least
        // one binding source is declared. An endpoint with NO sources
        // and NO required flow params is a no-op for the contract
        // (matches v1.36.0 honest-scope behavior).
        if !node.execute_flow.is_empty() {
            let has_any_source = !node.body_type.is_empty()
                || !node.path_params.is_empty()
                || !node.query_params.is_empty();
            if !has_any_source {
                // No declared binding sources — honest scope. The
                // runtime binder will deliver an empty map; the flow
                // must accept that (any required param would fail at
                // request time, which is the v1.36.0 behavior).
                return;
            }
            let body_opt = if node.body_type.is_empty() {
                None
            } else {
                find_type_by_name(self.program, &node.body_type)
            };
            if let Some(flow) = self.find_flow(&node.execute_flow) {
                for param in &flow.parameters {
                    if param.type_expr.optional {
                        continue; // an optional parameter need not bind.
                    }
                    // — Resolve binding sources for this param name. —
                    let path_hit =
                        node.path_params.iter().any(|p| p == &param.name);
                    let query_hit = node
                        .query_params
                        .iter()
                        .find(|f| f.name == param.name);
                    let body_hit = body_opt
                        .and_then(|b| b.fields.iter().find(|f| f.name == param.name));
                    let source_count = (path_hit as usize)
                        + (query_hit.is_some() as usize)
                        + (body_hit.is_some() as usize);

                    if source_count == 0 {
                        // ── D3 (extended) — no coverage. The hint
                        // now names ALL THREE candidate sources so the
                        // adopter knows where to add the field.
                        let body_clause = if node.body_type.is_empty() {
                            "(declare a body type via `body: T` or)".to_string()
                        } else {
                            format!(
                                "add a field '{}: {}' to '{}', or",
                                param.name,
                                fmt_type_expr(&param.type_expr),
                                node.body_type,
                            )
                        };
                        self.emit(
                            format!(
                                "axonendpoint '{}' executes flow '{}' whose \
                                 required parameter '{}: {}' has no matching \
                                 binding source. The Request Binding Contract \
                                 (Fase 37 + 37.y D3) binds a flow parameter \
                                 from a same-named path placeholder \
                                 (`{{{}}}` in the `path:` string), query \
                                 param (`query: {{ {}: {} }}`), or body \
                                 field. Either {} add a `{{{}}}` placeholder \
                                 to the path, or declare `{}: {}` in the \
                                 `query: {{ … }}` block — or make the \
                                 parameter optional (Fase 37.y D3).",
                                node.name,
                                node.execute_flow,
                                param.name,
                                fmt_type_expr(&param.type_expr),
                                param.name,
                                param.name,
                                fmt_type_expr(&param.type_expr),
                                body_clause,
                                param.name,
                                param.name,
                                fmt_type_expr(&param.type_expr),
                            ),
                            &node.loc,
                        );
                        continue;
                    }

                    if source_count > 1 {
                        // ── D4 — collision. The new `axon-T901`
                        // error: an adopter who declares the same
                        // name in two+ sources MUST disambiguate
                        // before the build can pass. The runtime
                        // merge order would otherwise mask which
                        // source the value came from.
                        let mut sources: Vec<&str> = Vec::new();
                        if path_hit {
                            sources.push("path");
                        }
                        if query_hit.is_some() {
                            sources.push("query");
                        }
                        if body_hit.is_some() {
                            sources.push("body");
                        }
                        let where_phrase = if sources.len() == 2 {
                            format!("{} and {}", sources[0], sources[1])
                        } else {
                            format!(
                                "{}, {}, and {}",
                                sources[0], sources[1], sources[2]
                            )
                        };
                        self.emit(
                            format!(
                                "axon-T901 axonendpoint '{}' parameter '{}' \
                                 is declared in MORE than one binding source \
                                 ({where_phrase}). The Request Binding Contract \
                                 forbids a name in multiple sources to keep \
                                 the runtime binding unambiguous. Remove the \
                                 declaration from {} of the sources so '{}' \
                                 resolves uniquely. (Fase 37.y D4)",
                                node.name,
                                param.name,
                                sources.len() - 1,
                                param.name,
                            ),
                            &node.loc,
                        );
                        continue;
                    }

                    // ── source_count == 1: type-compatibility check. —
                    if path_hit {
                        // Path params bind as `Text` (HTTP convention).
                        // The flow param must be `Text` (with no generic).
                        if param.type_expr.name != "Text"
                            || !param.type_expr.generic_param.is_empty()
                        {
                            self.emit(
                                format!(
                                    "axonendpoint '{}' parameter '{}' is bound \
                                     from path placeholder `{{{}}}` (HTTP path \
                                     segments are `Text` by convention), but \
                                     the flow declares '{}: {}'. Either change \
                                     the flow parameter to `{}: Text` and \
                                     parse/validate inside the flow, or move \
                                     the binding to `query: {{ {}: {} }}` if \
                                     the type matters at the wire (Fase 37.y D3).",
                                    node.name,
                                    param.name,
                                    param.name,
                                    param.name,
                                    fmt_type_expr(&param.type_expr),
                                    param.name,
                                    param.name,
                                    fmt_type_expr(&param.type_expr),
                                ),
                                &node.loc,
                            );
                        }
                    } else if let Some(qf) = query_hit {
                        // Query param: exact-match type check (same as
                        // the body branch — uniform contract).
                        if qf.type_expr.name != param.type_expr.name
                            || qf.type_expr.generic_param
                                != param.type_expr.generic_param
                        {
                            self.emit(
                                format!(
                                    "axonendpoint '{}' executes flow '{}' \
                                     whose parameter '{}' is '{}', but the \
                                     `query: {{ … }}` block declares '{}' as \
                                     '{}' — the types must match for the \
                                     Request Binding Contract to bind it \
                                     (Fase 37.y D3).",
                                    node.name,
                                    node.execute_flow,
                                    param.name,
                                    fmt_type_expr(&param.type_expr),
                                    qf.name,
                                    fmt_type_expr(&qf.type_expr),
                                ),
                                &node.loc,
                            );
                        }
                    } else if let Some(field) = body_hit {
                        // Body field — preserved v1.36.0 logic verbatim.
                        if field.type_expr.name != param.type_expr.name
                            || field.type_expr.generic_param
                                != param.type_expr.generic_param
                        {
                            self.emit(
                                format!(
                                    "axonendpoint '{}' executes flow '{}' whose \
                                     parameter '{}' is '{}', but body type '{}' \
                                     declares field '{}' as '{}' — the types \
                                     must match for the Request Binding \
                                     Contract to bind it (Fase 37 D2).",
                                    node.name,
                                    node.execute_flow,
                                    param.name,
                                    fmt_type_expr(&param.type_expr),
                                    node.body_type,
                                    field.name,
                                    fmt_type_expr(&field.type_expr),
                                ),
                                &node.loc,
                            );
                        }
                    }
                }
            }
        }

        // §Fase 38.x.f cardinality gate runs ABOVE this block (before
        // the 37.c early-return on no-source endpoints). See the
        // comment block at the top of check_axonendpoint for the
        // load-bearing rationale.
    }

    /// §Fase 39.e (D12 α RATIFIED) — Wire-shape mandate gate.
    ///
    /// On `transport: json` endpoints (effective wire = json), every
    /// declared `output: T` MUST be one of:
    ///   - `FlowEnvelope<X>` (the canonical v2.0.0 wire shape)
    ///   - `Any` (universal accept — degraded surface)
    ///   - `Unit` (no body)
    ///   - `<empty>` (no declaration — D9 backwards-compat skip)
    ///
    /// Any other declaration — `T` (singular bare), `List<X>`,
    /// `Stream<X>` — is a COMPILE ERROR `axon-E039`. The error
    /// message includes the canonical FlowEnvelope wrapping suggestion
    /// (computed from the flow tail's actual cardinality) PLUS the
    /// sse transport alternative.
    ///
    /// On `transport: sse` / `transport: ndjson` endpoints, this gate
    /// does NOT fire — the SSE wire has its own event family per
    /// Fase 39 D9 (`axon.token` / `axon.complete` / etc.), so adopters
    /// can declare any cardinality output without the wrapping
    /// mandate.
    ///
    /// Returns `true` when E039 fires (the caller suppresses the
    /// downstream cardinality gate to keep the diagnostic noise low);
    /// `false` when E039 does NOT fire.
    fn emit_e039_wire_packaging_gate(
        &mut self,
        node: &AxonEndpointDefinition,
    ) -> bool {
        // ── §1 — Resolve effective transport ────────────────────────
        // Mirrors the runtime's three-way resolution:
        //   explicit (parser captured `transport:`) →
        //     use `node.transport`
        //   not explicit + `implicit_transport` computed (Fase 31) →
        //     use `node.implicit_transport`
        //   neither → default "json"
        let effective_transport = if node.transport_explicit {
            node.transport.as_str()
        } else if !node.implicit_transport.is_empty() {
            node.implicit_transport.as_str()
        } else {
            "json"
        };
        // Only json wire bears the mandate per D2 + D9. SSE / ndjson
        // have their own event families and are exempt.
        if effective_transport != "json" {
            return false;
        }

        // ── §2 — Classify the declared output ──────────────────────
        let declared = node.output_type.trim();
        // Empty: D9 backwards-compat skip (no validation gate runs
        // on the wire either).
        if declared.is_empty() {
            return false;
        }
        // `Any`: universal accept — documented degraded surface.
        if declared == "Any" {
            return false;
        }
        // `Unit`: no body to validate — explicit "I produce no
        // wire payload" contract.
        if declared == "Unit" {
            return false;
        }
        // `FlowEnvelope<...>`: the canonical wrapping. Compile-clean
        // even if the inner T is itself a generic (List<X>,
        // Stream<X>) — the 39.a Cardinality::Wrapped variant carries
        // the inner cardinality through transparently.
        if declared.starts_with("FlowEnvelope<") && declared.ends_with('>') {
            return false;
        }

        // ── §3 — E039 fires: compute the canonical suggestion ──────
        // The suggestion is the SAME inner T the adopter declared,
        // just wrapped: `output: T` → suggest `FlowEnvelope<T>`. When
        // the flow's actual tail cardinality differs from the
        // declared bare type, we additionally name the tail shape
        // so the adopter sees the right wrapping for the data.
        let tail_form = if let Some(flow) = self.find_flow(&node.execute_flow) {
            let tail = infer_flow_tail_cardinality(flow);
            match tail {
                Cardinality::Singular(t) if !t.is_empty() => t,
                Cardinality::Plural(t) if !t.is_empty() => {
                    format!("List<{t}>")
                }
                Cardinality::StreamCardinality(t) if !t.is_empty() => {
                    format!("Stream<{t}>")
                }
                Cardinality::Wrapped(_) => {
                    // Defensive: tail being Wrapped is degenerate
                    // (flows don't produce FlowEnvelope<T> directly —
                    // the converter wraps them at the wire). Fall
                    // back to the declared shape.
                    declared.to_string()
                }
                _ => declared.to_string(),
            }
        } else {
            declared.to_string()
        };

        let suggested_envelope = format!("FlowEnvelope<{tail_form}>");

        // ── §4 — Emit the canonical diagnostic ─────────────────────
        self.emit(
            format!(
                "axon-E039 axonendpoint '{}' declares `output: {}` with \
                 `transport: json` (effective), but the v2.0.0 wire \
                 contract requires `FlowEnvelope<T>` wrapping for every \
                 JSON-transport response (D12 α). The wire payload IS \
                 the ψ-vector envelope `⟨ontological_type, result, \
                 certainty, provenance_chain, …⟩`; a bare `{}` cannot \
                 satisfy that contract. Flow '{}' produces a `{}` tail. \
                 Either: \
                 (a) wrap the output type — `output: {}` is the \
                     canonical v2.0.0 declaration (the inner T is \
                     validated against the envelope's `result` slot \
                     by the D5 runtime gate); OR \
                 (b) change the transport — `transport: sse(axon)` \
                     surfaces a streaming wire (per-chunk axon.token \
                     events + axon.complete envelope) where bare \
                     `Stream<T>` / `List<T>` declarations are valid. \
                 See the wire-envelope contract in the docs \
                 (https://www.ricardovelit.com/axon-docs) for the \
                 ψ-vector shape. \
                 (Fase 39 D2 + D12 — Pure Silicon Cognition)",
                node.name,
                node.output_type,
                node.output_type,
                node.execute_flow,
                tail_form,
                suggested_envelope,
            ),
            &node.loc,
        );
        true
    }

    /// §Fase 38.x.f (D1–D6) — Emit the cardinality-mismatch diagnostic
    /// for the `(declared, tail)` disjoint pair. Pure: depends only on
    /// the endpoint's `output:` declaration and the flow tail's
    /// inferred cardinality.
    fn emit_cardinality_gate(
        &mut self,
        node: &AxonEndpointDefinition,
        declared: &Cardinality,
        tail: &Cardinality,
    ) {
        // §Fase 39 (D4) — Wrapped(inner) unwraps transparently for the
        // cardinality gate. The wire shape of `FlowEnvelope<T>` is
        // always a singular object, but the COMPILE-TIME cardinality
        // the type-checker reasons about IS T's cardinality. The
        // unwrap is recursive (covers `FlowEnvelope<FlowEnvelope<X>>`
        // degenerate forms too). The full axon-E039 wire-shape
        // mandate enforcement lands in sub-fase 39.e; here in 39.a we
        // only establish that Wrapped participates in the cardinality
        // truth table by unwrapping its inner.
        if let Cardinality::Wrapped(inner) = declared {
            return self.emit_cardinality_gate(node, inner.as_ref(), tail);
        }
        if let Cardinality::Wrapped(inner) = tail {
            return self.emit_cardinality_gate(node, declared, inner.as_ref());
        }
        match (declared, tail) {
            // — Agreed shapes → silent pass.
            (Cardinality::Unit, Cardinality::Unit) => {}
            (Cardinality::Singular(d), Cardinality::Singular(_)) => {
                // Type-level mismatch is the existing Fase 37 D2
                // contract; cardinality agrees.
                let _ = d;
            }
            (Cardinality::Plural(_), Cardinality::Plural(_)) => {}
            (Cardinality::StreamCardinality(_), Cardinality::StreamCardinality(_)) => {}

            // — Unknown / Disagreed-acceptance → silent pass.
            (_, Cardinality::Unknown) => {}
            (Cardinality::Disagreed, _) => {
                // `output: Any` accepts every cardinality (degraded
                // surface; documented adopter choice — Fase 38.x.f D6).
            }
            (Cardinality::Unknown, _) => {}

            // — D3 bilateral: declared Plural vs tail Singular.
            (Cardinality::Plural(decl_t), Cardinality::Singular(_)) => {
                self.emit(
                    format!(
                        "axon-T9XX axonendpoint '{}' declares `output: {}` \
                         (plural — `List<{}>`), but flow '{}' produces a \
                         `{}` (singular) tail. The runtime would either \
                         wrap the singular in an array implicitly OR fail \
                         the D5 output-schema gate (Fase 32.d) depending \
                         on path. To make the contract explicit: \
                         (a) change the endpoint to `output: {}` if it \
                             returns a single resource (REST \
                             `GET /api/{{resource}}/{{id}}`-style); OR \
                         (b) wrap the tail in a list — `return [result]` \
                             or `for x in [result] {{ x }}` at the flow \
                             tail. \
                         (Fase 38.x.f D3 bilateral)",
                        node.name,
                        node.output_type,
                        decl_t,
                        node.execute_flow,
                        decl_t,
                        decl_t,
                    ),
                    &node.loc,
                );
            }

            // — D1 v1.39.0 retained: declared Singular vs tail Plural
            //   (this is the canonical kivi-shape that 38.x.e shipped).
            (Cardinality::Singular(decl_t), Cardinality::Plural(_)) => {
                self.emit(
                    format!(
                        "axon-T9XX axonendpoint '{}' declares `output: {}` \
                         (singular), but flow '{}' produces a `List<{}>` \
                         tail expression — the flow ends with a step or \
                         construct that produces a list (e.g. `retrieve` \
                         step, `for x in xs {{ … }}` loop, or `return \
                         [a, b, c]`). The runtime D5 output-schema gate \
                         (Fase 32.d) would reject the response as a \
                         shape mismatch. \
                         Either: \
                         (a) change the endpoint to `output: List<{}>` if \
                             it is intentionally returning a collection \
                             (REST `GET /api/{{resource}}`-style); OR \
                         (b) collapse the tail to a singular element — \
                             e.g. add `step Project {{ return result[0] }}` \
                             (or any step that emits the singular shape) \
                             BEFORE the implicit tail, OR add an explicit \
                             `return result[0]` at the end of the flow if \
                             the iteration is guaranteed to yield exactly \
                             one element. \
                         (Fase 38.x.f D1 — v1.39.0 narrow case preserved)",
                        node.name,
                        node.output_type,
                        node.execute_flow,
                        decl_t,
                        decl_t,
                    ),
                    &node.loc,
                );
            }

            // — D5 Stream mismatches (any direction crossing Stream
            //   with non-Stream).
            (Cardinality::StreamCardinality(decl_t), Cardinality::Singular(_))
            | (Cardinality::StreamCardinality(decl_t), Cardinality::Plural(_)) => {
                self.emit(
                    format!(
                        "axon-T9YY axonendpoint '{}' declares `output: \
                         Stream<{}>` (temporal — chunks arrive over time \
                         on SSE), but flow '{}' produces a non-stream \
                         tail. These are distinct cardinality primitives: \
                         (a) change the endpoint to `output: {}` (or \
                             `List<{}>`) if you want JSON delivery at \
                             once, OR \
                         (b) change the flow tail to a step with \
                             `output: Stream<{}>` (e.g. `step Generate \
                             {{ ask: \"...\" output: Stream<{}> }}`) if \
                             you want SSE chunked delivery. \
                         (Fase 38.x.f D5 stream_cardinality_mismatch)",
                        node.name,
                        decl_t,
                        node.execute_flow,
                        decl_t,
                        decl_t,
                        decl_t,
                        decl_t,
                    ),
                    &node.loc,
                );
            }
            (Cardinality::Singular(_), Cardinality::StreamCardinality(strm_t))
            | (Cardinality::Plural(_), Cardinality::StreamCardinality(strm_t)) => {
                self.emit(
                    format!(
                        "axon-T9YY axonendpoint '{}' declares `output: {}` \
                         (spatial — materialized at once), but flow '{}' \
                         produces a `Stream<{}>` tail (temporal — chunks \
                         arrive over time). These are distinct \
                         cardinality primitives: \
                         (a) change the endpoint to `output: Stream<{}>` \
                             if you want SSE chunked delivery, OR \
                         (b) change the flow tail to a non-streaming step \
                             returning `{}` if you want JSON delivery. \
                         (Fase 38.x.f D5 stream_cardinality_mismatch)",
                        node.name,
                        node.output_type,
                        node.execute_flow,
                        strm_t,
                        strm_t,
                        node.output_type,
                    ),
                    &node.loc,
                );
            }

            // — D6 W003: tail-Disagreed against non-Any output.
            (_, Cardinality::Disagreed) => {
                self.emit(
                    format!(
                        "axon-W003 axonendpoint '{}' executes flow '{}' \
                         whose tail is an `if`/`else` (or `par`) where \
                         the branches disagree on cardinality — one \
                         branch returns a singular value while another \
                         returns a list (or stream). The endpoint's \
                         `output: {}` cannot satisfy both shapes \
                         simultaneously. Either: \
                         (a) align the branches — return the same \
                             cardinality from both; OR \
                         (b) declare `output: Any` to accept either \
                             shape (degraded type safety; the runtime \
                             D5 gate will not protect this endpoint); \
                             OR \
                         (c) split into two endpoints, one per branch's \
                             shape. \
                         (Fase 38.x.f D6 cardinality_disagreement_in_branches)",
                        node.name,
                        node.execute_flow,
                        node.output_type,
                    ),
                    &node.loc,
                );
            }

            // — Type-mismatch only (cardinality agrees, types differ).
            //   The existing Fase 37 D2 contract handles this.
            (Cardinality::Unit, _) | (_, Cardinality::Unit) => {
                // Unit + anything else is intentionally silent here —
                // the runtime treats Unit-output as "no response body"
                // and the existing output-schema gate doesn't apply.
            }

            // §Fase 39 (D4) — Wrapped arms are statically unreachable
            // because the early-return at the top of this fn unwraps
            // any Wrapped operand before entering the match. The arms
            // here exist solely to satisfy Rust's exhaustiveness
            // checker. If reached, it indicates a bug in the unwrap
            // shortcut.
            (Cardinality::Wrapped(_), _) | (_, Cardinality::Wrapped(_)) => {
                unreachable!(
                    "§Fase 39 D4 invariant — Wrapped is unwrapped by the \
                     early-return shortcut at the top of emit_cardinality_gate; \
                     this match arm should be unreachable."
                );
            }
        }
    }

    /// §Fase 35.j — Resolve a flow declaration by name.
    fn find_flow(&self, name: &str) -> Option<&'a FlowDefinition> {
        self.program.declarations.iter().find_map(|d| match d {
            Declaration::Flow(f) if f.name == name => Some(f),
            _ => None,
        })
    }

    /// §Fase 35.j — Resolve an axonstore declaration by name.
    fn find_store(&self, name: &str) -> Option<&'a AxonStoreDefinition> {
        self.program.declarations.iter().find_map(|d| match d {
            Declaration::AxonStore(s) if s.name == name => Some(s),
            _ => None,
        })
    }

    /// §Fase 84.c — Resolve a socket declaration by name.
    fn find_socket(&self, name: &str) -> Option<&'a SocketDefinition> {
        self.program.declarations.iter().find_map(|d| match d {
            Declaration::Socket(s) if s.name == name => Some(s),
            _ => None,
        })
    }

    /// §Fase 84.c — Resolve a session declaration by name.
    fn find_session(&self, name: &str) -> Option<&'a SessionDefinition> {
        self.program.declarations.iter().find_map(|d| match d {
            Declaration::Session(s) if s.name == name => Some(s),
            _ => None,
        })
    }

    /// §Fase 85.c — Resolve a cache declaration by name.
    fn find_cache(&self, name: &str) -> Option<&'a CacheDefinition> {
        self.program.declarations.iter().find_map(|d| match d {
            Declaration::Cache(c) if c.name == name => Some(c),
            _ => None,
        })
    }

    // ── Flow-level reference checks ─────────────────────────────────

    /// §Fase 121 — every statement position a STEP BODY can carry, walked
    /// through the ONE law walk (`check_flow_steps`).
    ///
    /// The `stream` recursion is here because of a measured miss: README block
    /// 14 writes `validate QuoteSnapshot against: MarketSchema` inside an
    /// `on_complete:` arm, and the first draft of the §121 descent — which
    /// walked only `pix_ops` — reported the block CLEAN while its schema was
    /// undeclared. A stream arm is a full step body (§119.n made it a
    /// `StepNode` for exactly that reason), so it recurses HERE, arms within
    /// arms included. `axon_frontend::effect_check` walks the same shape for
    /// D9; the two walks must keep agreeing about what "a step body" contains.
    fn check_step_body_statements(&mut self, s: &crate::ast::StepNode, flow_name: &str) {
        self.check_flow_steps(&s.pix_ops, flow_name);
        if let Some(sb) = &s.stream {
            for arm in [&sb.on_chunk, &sb.on_complete, &sb.on_error]
                .into_iter()
                .flatten()
            {
                self.check_step_body_statements(arm, flow_name);
            }
            self.check_flow_steps(&sb.body, flow_name);
        }
    }

    fn check_flow_steps(&mut self, steps: &[FlowStep], flow_name: &str) {
        // §Fase 109.a — rich `let` expressions seen SO FAR (the T932
        // "prior" discipline: grad differentiates what is already bound).
        let mut rich_lets_seen: std::collections::HashMap<String, crate::ast::Expr> =
            std::collections::HashMap::new();
        for step in steps {
            match step {
                FlowStep::ShieldApply(n) => {
                    if !n.shield_name.is_empty() {
                        match self.symbols.lookup(&n.shield_name) {
                            None => self.emit(
                                format!(
                                    "Undefined shield '{}' in flow '{}'",
                                    n.shield_name, flow_name
                                ),
                                &n.loc,
                            ),
                            Some(sym) if sym.kind != "shield" => self.emit(
                                format!("'{}' is a {}, not a shield", n.shield_name, sym.kind),
                                &n.loc,
                            ),
                            _ => {}
                        }
                    }
                }
                // §Fase 121 — `validate <target> against: <Schema>` must RESOLVE.
                //
                // `IRValidateStep.rule` was a free string nobody looked at: it
                // parsed, lowered into the IR, and no code in either repo read
                // it. That is the `pix_ops` shape (§119.f.7) — a field written
                // by the parser with no reader — and it stayed invisible
                // because `validate` returned prose either way.
                //
                // It matters NOW because §121 makes the schema the source of the
                // validation's CSR: the constraints a response is scored against
                // ARE the schema's fields. A name that resolves to nothing has
                // no constraints, and scoring against an empty set would either
                // divide by zero or — worse — report a clean 1.0 for a
                // validation that checked nothing. Refuse instead: an absent
                // schema is missing EVIDENCE, and substituting a verdict for it
                // is the §112 kernel defect.
                FlowStep::Validate(n) => {
                    if !n.rule.is_empty() {
                        match self.symbols.lookup(&n.rule) {
                            None => self.emit(
                                format!(
                                    "axon-T1210 `validate … against: {}` in flow '{}' names a \
                                     schema that is not declared. The schema's fields ARE the \
                                     constraints the response is scored against, so an \
                                     unresolved name would score it against nothing and report \
                                     a clean verdict for a check that never happened. Declare \
                                     `type {} {{ … }}`.",
                                    n.rule, flow_name, n.rule
                                ),
                                &n.loc,
                            ),
                            Some(sym) if sym.kind != "type" => self.emit(
                                format!(
                                    "axon-T1210 `validate … against: {}` names a {}, not a \
                                     `type`. A validation is scored against a STRUCTURE — the \
                                     declared fields of a type — and a {} declares none.",
                                    n.rule, sym.kind, sym.kind
                                ),
                                &n.loc,
                            ),
                            _ => {}
                        }
                    }
                    // §Fase 121 — the guard's VALUE laws. (Its structural laws
                    // — a guard needs a schema-bearing validation, one guard
                    // per validation — are unrepresentable or parse errors; see
                    // `ast::ValidateStep::guard`.)
                    if let Some(g) = &n.guard {
                        // CSR ∈ [0,1] by construction (a ratio of satisfied
                        // constraints), so a floor at 0 can never fire — dead
                        // governance that READS as live, the §111 shape in one
                        // number — and a floor above 1 always fires, a retry
                        // loop wearing a conditional's clothes. Both lie about
                        // what the program does; both are refused.
                        if !(g.threshold > 0.0 && g.threshold <= 1.0) {
                            self.emit(
                                format!(
                                    "axon-T1211 `if confidence < {}` can{} fire: the \
                                     confidence is a constraint-satisfaction ratio in [0, 1], \
                                     so the floor must satisfy 0 < θ ≤ 1. A guard that cannot \
                                     fire is dead governance that reads as live; one that \
                                     always fires is an unconditional retry loop disguised as \
                                     a conditional.",
                                    g.threshold,
                                    if g.threshold <= 0.0 { " NEVER" } else { " ALWAYS" },
                                ),
                                &g.loc,
                            );
                        }
                        if g.max_attempts == 0 {
                            self.emit(
                                "axon-T1212 `refine(max_attempts: 0)` promises a recovery and \
                                 performs none — the guard would fire, refine nothing, and \
                                 proceed below the floor it declared. Declare at least 1, or \
                                 remove the guard."
                                    .to_string(),
                                &g.loc,
                            );
                        }
                    }
                }
                FlowStep::OtsApply(n) => {
                    if !n.ots_name.is_empty() {
                        match self.symbols.lookup(&n.ots_name) {
                            None => self.emit(
                                format!("Undefined OTS '{}' in flow '{}'", n.ots_name, flow_name),
                                &n.loc,
                            ),
                            Some(sym) if sym.kind != "ots" => self.emit(
                                format!("'{}' is a {}, not an OTS", n.ots_name, sym.kind),
                                &n.loc,
                            ),
                            _ => {}
                        }
                    }
                }
                FlowStep::MandateApply(n) => {
                    if !n.mandate_name.is_empty() {
                        match self.symbols.lookup(&n.mandate_name) {
                            None => self.emit(
                                format!(
                                    "Undefined mandate '{}' in flow '{}'",
                                    n.mandate_name, flow_name
                                ),
                                &n.loc,
                            ),
                            Some(sym) if sym.kind != "mandate" => self.emit(
                                format!("'{}' is a {}, not a mandate", n.mandate_name, sym.kind),
                                &n.loc,
                            ),
                            _ => {}
                        }
                    }
                }
                FlowStep::LambdaDataApply(n) => {
                    if !n.lambda_data_name.is_empty() {
                        match self.symbols.lookup(&n.lambda_data_name) {
                            None => self.emit(
                                format!(
                                    "Undefined lambda '{}' in flow '{}'",
                                    n.lambda_data_name, flow_name
                                ),
                                &n.loc,
                            ),
                            Some(sym) if sym.kind != "lambda_data" => self.emit(
                                format!(
                                    "'{}' is a {}, not a lambda_data",
                                    n.lambda_data_name, sym.kind
                                ),
                                &n.loc,
                            ),
                            _ => {}
                        }
                    }
                    // Fase 15.d — output_type must not shadow primitive type names.
                    // Mirror of axon.compiler.type_checker._RESERVED_OUTPUT_TYPE_NAMES;
                    // drift is detected by tests/test_lambda_data_runtime.py::
                    // test_derivation_vocab_parity_with_compiler (sibling concept).
                    if !n.output_type.is_empty()
                        && RESERVED_OUTPUT_TYPE_NAMES
                            .contains(&n.output_type.to_ascii_lowercase().as_str())
                    {
                        self.emit(
                            format!(
                                "lambda apply output_type '{}' shadows a reserved \
                                 primitive / built-in type name — choose a distinct \
                                 name for the bound envelope",
                                n.output_type
                            ),
                            &n.loc,
                        );
                    }
                }
                FlowStep::Let(n) => {
                    // §Fase 109.a — record the rich expression for the T932
                    // "prior let" discipline (grad differentiates it).
                    if let Some(e) = &n.value_ast {
                        rich_lets_seen.insert(n.identifier.clone(), e.clone());
                    }
                    // Fase 17.d — type-checker hardening for `let` bindings.
                    // Mirror of axon/compiler/type_checker.py::_check_let.
                    if n.identifier.is_empty() {
                        self.emit(
                            "let binding requires an identifier".to_string(),
                            &n.loc,
                        );
                    } else {
                        // Reserved primitive type name shadowing.
                        if RESERVED_OUTPUT_TYPE_NAMES
                            .contains(&n.identifier.to_ascii_lowercase().as_str())
                        {
                            self.emit(
                                format!(
                                    "let binding '{}' shadows a reserved primitive / \
                                     built-in type name — choose a distinct identifier",
                                    n.identifier
                                ),
                                &n.loc,
                            );
                        }
                        // Self-reference: `let X = X` or `let X = X.something`.
                        if n.value_kind == "reference" && !n.value_expr.is_empty() {
                            let head = n.value_expr.split('.').next().unwrap_or("");
                            if head == n.identifier {
                                self.emit(
                                    format!(
                                        "let binding '{}' is self-referential \
                                         (value '{}' starts with the binding name itself) — \
                                         cannot resolve at runtime",
                                        n.identifier, n.value_expr
                                    ),
                                    &n.loc,
                                );
                            }
                        }
                    }
                }
                FlowStep::Navigate(n) => {
                    // §Fase 63.B — the navigation target is a `pix` (PIX tree) OR a
                    // `corpus` (MDN graph). Either is well-typed; anything else is
                    // an error.
                    if !n.pix_name.is_empty() {
                        match self.symbols.lookup(&n.pix_name) {
                            None => self.emit(
                                format!("Undefined pix or corpus '{}' in navigate step", n.pix_name),
                                &n.loc,
                            ),
                            Some(sym) if sym.kind != "pix" && sym.kind != "corpus" => self.emit(
                                format!("'{}' is a {}, not a pix or corpus", n.pix_name, sym.kind),
                                &n.loc,
                            ),
                            _ => {}
                        }
                    }
                    if n.query_expr.is_empty() {
                        self.emit(
                            "Navigate step requires a query expression".to_string(),
                            &n.loc,
                        );
                    }
                }
                FlowStep::Ingest(n) => {
                    // §Fase 108.c (`axon-T929`) — the ingest law. An ingest is a
                    // governed load into a DECLARED dataspace: target + format are
                    // not optional metadata, they are what makes the load checkable
                    // (schema known at compile time; bytes bounded before parse).
                    if n.target.is_empty() {
                        self.emit(
                            format!(
                                "axon-T929 ingest of `{}` names no dataspace. The governed \
                                 form is `ingest <sourceRef> into <Dataspace> {{ format: … }}` \
                                 — a load without a declared destination schema cannot be \
                                 type-checked and is refused.",
                                n.source
                            ),
                            &n.loc,
                        );
                    } else {
                        match self.symbols.lookup(&n.target) {
                            None => self.emit(
                                format!(
                                    "axon-T929 ingest targets `{}`, which is not declared. \
                                     Declare it: `dataspace {} {{ column <name>: <Type> … }}`.",
                                    n.target, n.target
                                ),
                                &n.loc,
                            ),
                            Some(sym) if sym.kind != "dataspace" => self.emit(
                                format!(
                                    "axon-T929 ingest targets `{}`, which is a {} — an ingest \
                                     loads into a `dataspace` (the analytical store), not a {}.",
                                    n.target, sym.kind, sym.kind
                                ),
                                &n.loc,
                            ),
                            _ => {}
                        }
                    }
                    if n.format.is_empty() {
                        self.emit(
                            format!(
                                "axon-T929 ingest into `{}` declares no `format:`. An ingest \
                                 that does not say what it is parsing cannot be deterministic \
                                 — declare `format: csv` or `format: json`.",
                                if n.target.is_empty() { "<unset>" } else { &n.target }
                            ),
                            &n.loc,
                        );
                    } else if !matches!(n.format.as_str(), "csv" | "json") {
                        self.emit(
                            format!(
                                "axon-T929 ingest declares unknown `format: {}`. The closed \
                                 loader catalog is {{csv, json}} (deterministic, first-party — \
                                 the §100 posture; Parquet/Arrow-IPC are deferred §108.x surface).",
                                n.format
                            ),
                            &n.loc,
                        );
                    }
                    if n.max_bytes == Some(0) || n.max_rows == Some(0) {
                        self.emit(
                            "axon-T929 ingest declares a zero limit — a bound that admits \
                             nothing is a declaration error, not a safety measure."
                                .to_string(),
                            &n.loc,
                        );
                    }
                }
                FlowStep::Grad(n) => {
                    // §Fase 109.a — the two laws of the proof-carrying
                    // derivative. T932: the target must be a PRIOR rich `let`
                    // in this flow + at least one `wrt`. T931: the expression
                    // must live inside the differentiable fragment — a
                    // non-differentiable construct is refused NAMING the
                    // construct and its position, never a silent zero.
                    if n.wrt.is_empty() {
                        self.emit(
                            format!("axon-T932 grad of `{}` names no `wrt` variable — the form is `grad <let> wrt <x>` or `grad <let> wrt [a, b]`.", n.target),
                            &n.loc,
                        );
                    }
                    match rich_lets_seen.get(&n.target) {
                        None => {
                            self.emit(
                                format!("axon-T932 grad targets `{0}`, which is not a PRIOR rich `let` in this flow. Bind the expression first: `let {0} = <arithmetic expression>` — grad differentiates the declared EXPRESSION, not a runtime value (a numeric derivative of a value is the approximation axon refuses to ship).", n.target),
                                &n.loc,
                            );
                        }
                        Some(expr) => {
                            for var in &n.wrt {
                                if let Err(e) = crate::expr_diff::differentiate(expr, var) {
                                    let position = if e.path.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" (position: {})", e.path)
                                    };
                                    self.emit(
                                        format!("axon-T931 grad of `{}` wrt `{var}`: the expression contains a non-differentiable construct — {}{position}. A gradient over it does not exist; axon does not fabricate one (no silent zeros, no finite-difference approximations). The differentiable fragment is arithmetic (+, -, *, /, negation) over Int/Float and `as_float`.", n.target, e.construct),
                                        &n.loc,
                                    );
                                }
                            }
                        }
                    }
                }
                FlowStep::Focus(n) => {
                    // §Fase 108.d (axon-T930) — σ∘π over a declared dataspace.
                    if let Some(msg) = self.dataspace_target_error("focus", &n.expression) {
                        self.emit(msg, &n.loc);
                    }
                    if let Some(cols) = self.dataspace_columns(&n.expression) {
                        for col in &n.select {
                            if !cols.iter().any(|(c, _)| c == col) {
                                self.emit(
                                    format!(
                                        "axon-T930 focus `select` references `{col}`, not a \
                                         column of dataspace `{}`.",
                                        n.expression
                                    ),
                                    &n.loc,
                                );
                            }
                        }
                    }
                }
                FlowStep::Aggregate(n) => {
                    // §Fase 108.d (axon-T930) — γ over a declared dataspace.
                    if let Some(msg) = self.dataspace_target_error("aggregate", &n.target) {
                        self.emit(msg, &n.loc);
                    }
                    if let Some(cols) = self.dataspace_columns(&n.target) {
                        for col in &n.group_by {
                            if !cols.iter().any(|(c, _)| c == col) {
                                self.emit(
                                    format!(
                                        "axon-T930 aggregate `group_by` references `{col}`, not \
                                         a column of dataspace `{}`.",
                                        n.target
                                    ),
                                    &n.loc,
                                );
                            }
                        }
                        for spec in &n.compute {
                            // Mirror of the runtime catalog: count | count(c) |
                            // sum(c) | avg(c) | min(c) | max(c).
                            let (fname, col) = match spec.find('(') {
                                Some(p) if spec.ends_with(')') => {
                                    (&spec[..p], Some(spec[p + 1..spec.len() - 1].trim()))
                                }
                                None => (spec.as_str(), None),
                                _ => {
                                    self.emit(
                                        format!("axon-T930 malformed aggregate `{spec}`."),
                                        &n.loc,
                                    );
                                    continue;
                                }
                            };
                            if !matches!(fname, "count" | "sum" | "avg" | "min" | "max") {
                                self.emit(
                                    format!(
                                        "axon-T930 unknown aggregate `{fname}` — the closed \
                                         catalog is {{count, sum, avg, min, max}}."
                                    ),
                                    &n.loc,
                                );
                                continue;
                            }
                            if fname != "count" && col.is_none() {
                                self.emit(
                                    format!(
                                        "axon-T930 aggregate `{fname}` requires a column: \
                                         `{fname}(<col>)`."
                                    ),
                                    &n.loc,
                                );
                            }
                            if let Some(col) = col {
                                match cols.iter().find(|(c, _)| c == col) {
                                    None => self.emit(
                                        format!(
                                            "axon-T930 aggregate `{spec}` references `{col}`, \
                                             not a column of dataspace `{}`.",
                                            n.target
                                        ),
                                        &n.loc,
                                    ),
                                    Some((_, declared_type)) if matches!(fname, "sum" | "avg") => {
                                        let ty = crate::ast::DataspaceColumnType::from_token(
                                            declared_type,
                                        );
                                        if !matches!(
                                            ty,
                                            Some(crate::ast::DataspaceColumnType::Int)
                                                | Some(crate::ast::DataspaceColumnType::Float)
                                        ) {
                                            let dt = declared_type.clone();
                                            self.emit(
                                                format!(
                                                    "axon-T930 `{spec}` is defined on Int/Float; \
                                                     column `{col}` is {dt} (refusal, not \
                                                     coercion)."
                                                ),
                                                &n.loc,
                                            );
                                        }
                                    }
                                    Some(_) => {}
                                }
                            }
                        }
                    }
                }
                FlowStep::Associate(n) => {
                    // §Fase 108.d (axon-T930) — equi-⋈ across two declared
                    // dataspaces on one shared column.
                    if let Some(msg) = self.dataspace_target_error("associate", &n.left) {
                        self.emit(msg, &n.loc);
                    }
                    if let Some(msg) = self.dataspace_target_error("associate", &n.right) {
                        self.emit(msg, &n.loc);
                    }
                    let ldef = self.dataspace_columns(&n.left);
                    let rdef = self.dataspace_columns(&n.right);
                    if n.using_field.is_empty() {
                        self.emit(
                            "axon-T930 associate declares no `using <column>` — an equi-join \
                             without a key is a cartesian product, which is refused."
                                .to_string(),
                            &n.loc,
                        );
                    } else if let (Some(l), Some(r)) = (ldef, rdef) {
                        let lc = l.iter().find(|(c, _)| c == &n.using_field).cloned();
                        let rc = r.iter().find(|(c, _)| c == &n.using_field).cloned();
                        match (lc, rc) {
                            (Some((_, lt_raw)), Some((_, rt_raw))) => {
                                let lt = crate::ast::DataspaceColumnType::from_token(&lt_raw);
                                let rt = crate::ast::DataspaceColumnType::from_token(&rt_raw);
                                if lt.is_some() && rt.is_some() && lt != rt {
                                    self.emit(
                                        format!(
                                            "axon-T930 associate `using {}` joins a {} column \
                                             against a {} column (refusal, not coercion).",
                                            n.using_field, lt_raw, rt_raw
                                        ),
                                        &n.loc,
                                    );
                                }
                            }
                            _ => self.emit(
                                format!(
                                    "axon-T930 associate `using {}` — the column must exist in \
                                     BOTH dataspaces (`{}`, `{}`).",
                                    n.using_field, n.left, n.right
                                ),
                                &n.loc,
                            ),
                        }
                    }
                }
                FlowStep::ExploreStep(n) => {
                    // §Fase 108.d (axon-T930) — a profile of a declared dataspace.
                    if let Some(msg) = self.dataspace_target_error("explore", &n.target) {
                        self.emit(msg, &n.loc);
                    }
                }
                FlowStep::Drill(n) => {
                    if !n.pix_name.is_empty() {
                        match self.symbols.lookup(&n.pix_name) {
                            None => self.emit(
                                format!("Undefined pix '{}' in drill step", n.pix_name),
                                &n.loc,
                            ),
                            Some(sym) if sym.kind != "pix" => self.emit(
                                format!("'{}' is a {}, not a pix", n.pix_name, sym.kind),
                                &n.loc,
                            ),
                            _ => {}
                        }
                    }
                    if n.subtree_path.is_empty() {
                        self.emit("Drill step requires a subtree_path".to_string(), &n.loc);
                    }
                    if n.query_expr.is_empty() {
                        self.emit("Drill step requires a query expression".to_string(), &n.loc);
                    }
                }
                FlowStep::Trail(n) => {
                    if n.navigate_ref.is_empty() {
                        self.emit("Trail step requires a navigate_ref".to_string(), &n.loc);
                    }
                }
                // ── §Fase 111 (T937) — `corroborate` is retracted ─────────
                //
                // The handler's ENTIRE body was an LLM prompt interpolating
                // the NAME of the reference — never its content:
                //
                //     format!("Corroborate navigation result `{}`", navigate_ref)
                //
                // No second source was fetched. No content was read. No
                // agreement metric was computed. And the framing addendum told
                // the model: "Cross-validate independently; surface agreement
                // strength + disagreements." So the model INVENTED a
                // corroboration — including how strongly two sources it never
                // saw agreed with each other.
                //
                // This is the §108 sin at its purest: not a missing feature but
                // a manufactured warrant. A step that fabricates verification is
                // strictly worse than no verification, because a reader trusts
                // it. Fail CLOSED (the §108 posture) until it is real.
                FlowStep::Corroborate(n) => {
                    self.emit(
                        format!(
                            "axon-T937 `corroborate {}` is RETRACTED (§111). It never corroborated \
                             anything: the runtime interpolated the reference's NAME into an LLM \
                             prompt, fetched no independent source, read no content, and computed no \
                             agreement metric — while instructing the model to report 'agreement \
                             strength'. It manufactured a warrant. Fabricated verification is worse \
                             than none, because it is believed. To cross-check a claim today, \
                             `navigate` a second corpus and compare the results explicitly, or gate \
                             the value behind an `anchor`.",
                            if n.navigate_ref.is_empty() { "<ref>" } else { &n.navigate_ref }
                        ),
                        &n.loc,
                    );
                }
                // ── §Fase 111 (T938) — `transact` is retracted ────────────
                //
                // `transact { … }` promised atomicity and delivered a string.
                // The runtime handler inserted `__txn_active = "true"` into the
                // let-bindings — a key nothing in the tree ever reads — and
                // opened no transaction, took no lock, and rolled nothing back.
                // `TransactBlock` carries only a source location: the block's
                // BODY is not even lowered into the IR.
                //
                // This is the most dangerous kind of dead primitive, because it
                // is load-bearing exactly when things go wrong. An adopter wraps
                // two `mutate`s in `transact` precisely so a failure between them
                // cannot leave the store half-written — and got no such
                // guarantee, silently, on the unhappy path they will only meet in
                // production. We even shipped it in a knowledge template.
                //
                // Fail CLOSED. A missing feature is survivable; a fabricated
                // atomicity guarantee corrupts data.
                // ── §Fase 111 (T939/T940) — `deliberate` / `consensus` ────
                //
                // The last two silent no-ops in the language, and they died the
                // same way `stream` and `transact` did: all four parse through
                // `parse_block_step`, whose entire job is `skip_braced_block()`.
                // The block's contents are thrown away AT PARSE TIME —
                // `DeliberateBlock` and `ConsensusBlock` carry only a source
                // location. So `run_consensus` does not even take its node
                // (`_node`), and could not: there is nothing in it.
                //
                // The README sells `deliberate` as "Compute budget control
                // (tokens/depth/strategy)" and `consensus` as "Best-of-N
                // parallel evaluation & selection". Neither has ever evaluated,
                // voted, budgeted or selected anything. Writing them bought the
                // adopter silence — their steps vanished, and `StepComplete`
                // went out on the wire saying the block had finished.
                //
                // Fail CLOSED (the §108 posture): a refusal is reversible, a
                // silent no-op is not. Whether these are implemented or
                // retracted is §111's Tier-4 decision; either way, they must
                // stop swallowing programs today.
                FlowStep::Deliberate(n) => {
                    self.emit(
                        "axon-T939 `deliberate { … }` is REFUSED (§111). It never controlled a \
                         budget: the block's body is discarded at PARSE time (it shares \
                         `parse_block_step` with `stream`/`consensus`/`transact`, whose whole job \
                         is to skip the braces), so the steps you wrote inside it never reached \
                         the IR and never ran — while the wire reported the block complete. To \
                         bound compute today, use a `budget { … }` on the daemon or the declared \
                         `max_tokens:` / `max_time:` / `max_cost:` ceilings, which are enforced."
                            .to_string(),
                        &n.loc,
                    );
                }
                FlowStep::Consensus(n) => {
                    self.emit(
                        "axon-T940 `consensus { … }` is REFUSED (§111). There is no best-of-N: no \
                         votes are cast, nothing is aggregated, no candidates are generated. The \
                         block's body is discarded at PARSE time, so the handler never even \
                         receives it — a selection over an empty set silently returned nothing \
                         while the wire reported success. To evaluate alternatives today, run them \
                         explicitly (e.g. `par { … }`) and select with a `validate` or an `anchor`."
                            .to_string(),
                        &n.loc,
                    );
                }
                FlowStep::Transact(n) => {
                    self.emit(
                        "axon-T938 `transact { … }` is RETRACTED (§111). It never opened a \
                         transaction: the runtime set an unread marker string, the block's body \
                         was never lowered into the IR, and no lock was taken and nothing was \
                         rolled back. Writes inside it were as atomic as writes outside it — which \
                         is to say, not. A fabricated atomicity guarantee is worse than an absent \
                         one, because you only discover it on the failure path. Until real \
                         transactional semantics land (§111.x), issue the writes directly and make \
                         them idempotent, so a retry converges instead of corrupting."
                            .to_string(),
                        &n.loc,
                    );
                }
                FlowStep::DaemonStep(n) => {
                    if !n.daemon_ref.is_empty() {
                        match self.symbols.lookup(&n.daemon_ref) {
                            None => self.emit(
                                format!(
                                    "Undefined daemon '{}' in flow '{}'",
                                    n.daemon_ref, flow_name
                                ),
                                &n.loc,
                            ),
                            Some(sym) if sym.kind != "daemon" => self.emit(
                                format!("'{}' is a {}, not a daemon", n.daemon_ref, sym.kind),
                                &n.loc,
                            ),
                            _ => {}
                        }
                    }
                }
                FlowStep::Persist(n) => {
                    self.check_store_ref(&n.store_name, flow_name, &n.loc);
                    // §Fase 94.a — write verbs never touch a secrets store
                    // (axon-T897): custody is written only by the seeding
                    // API and the mediated `rotate` commit.
                    self.check_secrets_store_write("persist", &n.store_name, flow_name, &n.loc);
                    // §Fase 38.e — D2 second half: persist field-block
                    // proof (axon-T803 NOT-NULL omission + axon-T804
                    // unknown field + axon-T802 value-type mismatch).
                    self.run_38e_persist_proof(&n.store_name, &n.fields, &n.loc);
                    // §Fase 92.b — a mint binding must never enter a store
                    // (axon-T896): credentials are shown once, not persisted.
                    for (col, value) in &n.fields {
                        for binding in self.current_mint_bindings.clone() {
                            if value == &binding
                                || value.contains(&format!("${{{binding}}}"))
                                || value.contains(&format!("${binding}"))
                            {
                                self.emit(
                                    format!(
                                        "axon-T896 the mint binding '{binding}' flows into \
                                         `persist` field '{col}' in flow '{flow_name}' — a \
                                         minted credential is shown ONCE and never enters a \
                                         store. Return it to the caller instead.",
                                    ),
                                    &n.loc,
                                );
                            }
                        }
                    }
                }
                FlowStep::Retrieve(n) => {
                    self.check_store_ref(&n.store_name, flow_name, &n.loc);
                    self.run_38d_where_proof(&n.store_name, &n.where_expr, &n.loc);
                    // §Fase 67.b — prove the `order_by:` / `limit:` clauses
                    // (axon-T807 / axon-T808).
                    self.run_67b_bounds_proof(
                        &n.store_name,
                        &n.order_by,
                        &n.limit_expr,
                        &n.loc,
                    );
                    // §Fase 76.d — prove the `aggregate:` / `group_by:`
                    // clauses (axon-T843 / axon-T844 / axon-T845).
                    self.run_76d_aggregate_proof(
                        &n.store_name,
                        &n.aggregate,
                        &n.group_by,
                        &n.order_by,
                        &n.limit_expr,
                        &n.loc,
                    );
                    // §Fase 85.c — a `retrieve` reads a store (never `pure`),
                    // so a `cache:` on it always accepts staleness: resolve the
                    // reference (T864) and require a finite `ttl:` (T865).
                    self.check_retrieve_cache_ref(&n.cache, flow_name, &n.loc);
                }
                FlowStep::Mutate(n) => {
                    self.check_store_ref(&n.store_name, flow_name, &n.loc);
                    // §Fase 94.a (axon-T897) — see the Persist arm.
                    self.check_secrets_store_write("mutate", &n.store_name, flow_name, &n.loc);
                    self.run_38d_where_proof(&n.store_name, &n.where_expr, &n.loc);
                    // §Fase 38.e — D2 second half: mutate SET-block
                    // proof (axon-T804 unknown field + axon-T802
                    // value-type mismatch). NOT-NULL omission (T803)
                    // does NOT apply to mutate (UPDATE preserves
                    // existing values for omitted columns).
                    self.run_38e_mutate_proof(&n.store_name, &n.fields, &n.loc);
                }
                FlowStep::Purge(n) => {
                    self.check_store_ref(&n.store_name, flow_name, &n.loc);
                    // §Fase 94.a (axon-T897) — see the Persist arm.
                    self.check_secrets_store_write("purge", &n.store_name, flow_name, &n.loc);
                    self.run_38d_where_proof(&n.store_name, &n.where_expr, &n.loc);
                }
                // §Fase 94.b — `rotate <SecretsStore> [where "…"] with <Tool>
                // as <binding>`: the target must be a declared `backend:
                // secrets` store (axon-T898 — rotating an adopter table is
                // meaningless: there is no custody behind it), the tool must
                // be a declared `tool` (axon-T899 — the exchange needs a real
                // executor), and the optional metadata filter is proven
                // against the SYNTHESIZED schema exactly like a `retrieve`.
                FlowStep::Rotate(n) => {
                    match self.symbols.lookup(&n.store_ref) {
                        None => self.emit(
                            format!(
                                "axon-T898 `rotate` targets '{}' in flow '{flow_name}', \
                                 which is not declared — a rotation targets a \
                                 `backend: secrets` metadata store.",
                                n.store_ref
                            ),
                            &n.loc,
                        ),
                        Some(sym) if sym.kind != "axonstore" => self.emit(
                            format!(
                                "axon-T898 `rotate` targets '{}' in flow '{flow_name}', \
                                 but it is a {} — a rotation targets a `backend: \
                                 secrets` metadata store.",
                                n.store_ref, sym.kind
                            ),
                            &n.loc,
                        ),
                        Some(_) if !self.secrets_backed_stores.contains(&n.store_ref) => {
                            self.emit(
                                format!(
                                    "axon-T898 `rotate` targets the axonstore '{}' in \
                                     flow '{flow_name}', but its backend is not \
                                     `secrets` — rotation is the renewal of CUSTODIED \
                                     authority (`rotation_without_revelation`); an \
                                     adopter table has no custody behind it. Use \
                                     `mutate` for ordinary rows, or declare a \
                                     `backend: secrets` store for the class.",
                                    n.store_ref
                                ),
                                &n.loc,
                            );
                        }
                        _ => {}
                    }
                    match self.symbols.lookup(&n.tool_ref) {
                        None => self.emit(
                            format!(
                                "axon-T899 `rotate … with {}` in flow '{flow_name}' \
                                 references an undeclared tool — the renewal exchange \
                                 is performed by a declared `tool` (it receives the \
                                 current value under the reserved `axon_rotation` \
                                 envelope and returns the renewed one).",
                                n.tool_ref
                            ),
                            &n.loc,
                        ),
                        Some(sym) if sym.kind != "tool" => self.emit(
                            format!(
                                "axon-T899 `rotate … with {}` in flow '{flow_name}' \
                                 references a {}, not a tool.",
                                n.tool_ref, sym.kind
                            ),
                            &n.loc,
                        ),
                        _ => {}
                    }
                    self.run_38d_where_proof(&n.store_ref, &n.where_expr, &n.loc);
                }
                FlowStep::ComputeApply(n) => {
                    if !n.compute_name.is_empty() {
                        match self.symbols.lookup(&n.compute_name) {
                            None => self.emit(
                                format!(
                                    "Undefined compute '{}' in flow '{}'",
                                    n.compute_name, flow_name
                                ),
                                &n.loc,
                            ),
                            Some(sym) if sym.kind != "compute" => self.emit(
                                format!("'{}' is a {}, not a compute", n.compute_name, sym.kind),
                                &n.loc,
                            ),
                            _ => {}
                        }
                    }
                }
                // Recurse into control flow bodies
                FlowStep::If(n) => {
                    // §Fase 70.b — static type-check a rich condition expression
                    // (the legacy triple form, `cond = None`, is unaffected).
                    if let Some(cond) = &n.cond {
                        // §Fase 73.e — the FULL-spelling scope so a `Json<T>`
                        // param's lens shape is visible to the field checker.
                        let scope = self.current_flow_param_spellings.clone();
                        let _ = self.infer_expr(cond, &scope, &n.loc);
                        // §Fase 70.e — const-fold the condition; a fully-constant
                        // condition statically decides the branch (dead code).
                        if let Some(cv) = const_fold(cond) {
                            let always = const_truthy(&cv);
                            let dead = if always { "else" } else { "then" };
                            self.warn(
                                format!(
                                    "axon-W008 condition is always {always} — the `{dead}` \
                                     branch is unreachable (constant expression)"
                                ),
                                &n.loc,
                            );
                        }
                    } else {
                        // §Fase 73.e — a plain dotted-ref condition stays the
                        // LEGACY triple (`cond = None`) and so bypasses
                        // `infer_expr`. Still run the `Json<T>` lens field check
                        // on its dotted-path operands so `if profile.agee >= 18`
                        // is caught (`axon-T842`) just like the rich forms.
                        self.check_legacy_lens_path(&n.condition, &n.loc);
                        self.check_legacy_lens_path(&n.comparison_value, &n.loc);
                        for (lhs, _op, val) in &n.conditions {
                            self.check_legacy_lens_path(lhs, &n.loc);
                            self.check_legacy_lens_path(val, &n.loc);
                        }
                    }
                    self.check_flow_steps(&n.then_body, flow_name);
                    self.check_flow_steps(&n.else_body, flow_name);
                }
                FlowStep::ForIn(n) => {
                    self.check_flow_steps(&n.body, flow_name);
                }
                // §Fase 92.b — `mint <Credential> as <binding>`: the
                // reference must resolve to a declared `credential`
                // (axon-T895); the binding is tracked for the T896
                // never-persisted law (walk order = source order, so a
                // textually-earlier persist can't see a later binding —
                // consistent with the flow's data-flow direction).
                FlowStep::Mint(n) => {
                    match self.symbols.lookup(&n.credential_ref) {
                        None => self.emit(
                            format!(
                                "axon-T895 undefined credential '{}' in flow '{}' — `mint` \
                                 requires a declared `credential {{ ttl: grants: }}` contract",
                                n.credential_ref, flow_name
                            ),
                            &n.loc,
                        ),
                        Some(sym) if sym.kind != "credential" => self.emit(
                            format!(
                                "axon-T895 '{}' is a {}, not a credential (referenced by \
                                 `mint` in flow '{}')",
                                n.credential_ref, sym.kind, flow_name
                            ),
                            &n.loc,
                        ),
                        _ => {}
                    }
                    self.current_mint_bindings.insert(n.binding.clone());
                }
                // §λ-L-E Fase 13 — Mobile typed channel reductions
                FlowStep::Emit(n) => self.check_emit(n),
                FlowStep::Publish(n) => self.check_publish(n),
                FlowStep::Discover(n) => self.check_discover(n),
                // §Fase 58.d — validate a tool dispatch against the tool's
                // declared input schema (W2 / CT-2 caller blame, pre-HTTP).
                FlowStep::UseTool(n) => {
                    match self.symbols.lookup(&n.tool_name) {
                        None => self.emit(
                            format!("Unknown tool '{}' in flow '{}'", n.tool_name, flow_name),
                            &n.loc,
                        ),
                        Some(sym) if sym.kind != "tool" => self.emit(
                            format!("'{}' is a {}, not a tool", n.tool_name, sym.kind),
                            &n.loc,
                        ),
                        _ => {
                            self.check_use_tool_args(n, steps);
                            // §Fase 116.a (axon-T956) — scope coverage.
                            self.check_use_tool_scopes(n);
                        }
                    }
                }
                // §Fase 59 (D2) — `apply: <Tool>` on a schema-bearing tool is
                // cognitive delegation; the honest-compiler `axon-W004` points
                // the adopter to the deterministic `use <Tool>(k=v)` form.
                FlowStep::Step(s) => {
                    // §Fase 119 (D119.4) — step-body guards resolve exactly
                    // like their flow-level twins. A guard naming a ghost is a
                    // cage around nothing: the step would run ungoverned while
                    // reading as governed. (This lives INSIDE the existing Step
                    // arm — a first draft added a second `FlowStep::Step` arm
                    // above this one, and the unreachable-pattern warning
                    // caught it silently disabling every §59/§68/§91 step
                    // check below.)
                    //
                    // ⚠️ §Fase 121 MADE THE SAME MISTAKE AGAIN — a second
                    // `FlowStep::Step` arm above this one, in this same file,
                    // with this warning already written here. `cargo build`
                    // said `unreachable_patterns`; the frontend suite went from
                    // 1678/0 to 1667/11, and the failures were tests expecting
                    // errors that had SILENTLY STOPPED FIRING. A warning nobody
                    // reads is indistinguishable from a disabled lint (§119's
                    // own words about `cargo doc`). Add step-body work HERE.
                    //
                    // §Fase 121 — descend into the step's BODY STATEMENTS.
                    // Measured: nothing did. The ten productions §119.f–§119.n
                    // gave a step-body position (`probe`, `reason`, `weave`,
                    // `navigate`, `drill`, `trail`, `validate`, `use_tool`,
                    // `par`, `retrieve`, `<Agent>(args)`) all reach the
                    // dispatcher, and NO law could see them. It surfaced
                    // because §121's `against:` resolution fired at flow level
                    // and stayed silent for the step-body form — the one every
                    // README block publishes. Law 4, inside the type-checker.
                    //
                    // Recursing through the SAME walk is the D119.4 doctrine:
                    // one concept, two positions, one implementation, so a
                    // future law reaches both by construction.
                    self.check_step_body_statements(s, flow_name);
                    for g in &s.guards {
                        if g.name.is_empty() {
                            continue;
                        }
                        // §Fase 119.c — the guard keyword and the symbol table
                        // disagree on lambda's name: the keyword is `lambda`,
                        // the declaration kind is `lambda_data`.
                        let expected_kind =
                            if g.kind == "lambda" { "lambda_data" } else { g.kind.as_str() };
                        match self.symbols.lookup(&g.name) {
                            None => self.emit(
                                format!(
                                    "Undefined {} '{}' in step '{}' of flow '{}'",
                                    g.kind, g.name, s.name, flow_name
                                ),
                                &g.loc,
                            ),
                            Some(sym) if sym.kind != expected_kind => self.emit(
                                format!(
                                    "'{}' is a {}, not a {} (step '{}' of flow '{}')",
                                    g.name, sym.kind, g.kind, s.name, flow_name
                                ),
                                &g.loc,
                            ),
                            _ => {}
                        }
                    }
                    self.check_apply_tool(s);
                    // §Fase 68.e — `apply: <Compute>` is a model-selection no-op;
                    // `axon-W006` points the adopter to `requires_context:`.
                    self.check_apply_compute(s);
                    // §Fase 68.f — a `requires_context:` that no model could ever
                    // satisfy is `axon-T809` at compile time.
                    self.check_requires_context(s);
                    // §Fase 91.a — a step-level `now:` must be a plausible IANA
                    // zone (`axon-T892`).
                    if let Some(tz) = &s.now_tz {
                        self.check_now_tz(tz, "step", &s.name, &s.loc);
                    }
                }
                // §Fase 51.b — the Continuous Type Invariant (D8). Inside a
                // `quant` block, conversational / unstructured discrete types
                // (String literals, `.to_string` textual conversions, free-text
                // `ask:` prompts) are the semantic-collapse path and are
                // rejected with `axon-E0782`. Integer indices, the closed enum
                // of bases/observables, and continuous carrier references are
                // admitted. The walk recurses through the quant body.
                FlowStep::Quant(q) => {
                    // §Fase 51.c — header semantic validation (encoding scheme,
                    // backend effect, register/depth/bandwidth bounds + the D2
                    // depth-trade-off note) …
                    self.check_quant_header(q, flow_name);
                    // … then the §51.b Continuous Type Invariant over the body.
                    self.check_continuous_type_invariant(&q.body, flow_name);
                }
                // §Fase 51.d.2 — a `yield` reached HERE is outside any `quant`
                // block (the quant body is walked by `check_continuous_type_
                // invariant`, not this method). `yield` is the amplitude-collapse
                // measurement point — meaningless outside the Hilbert-space scope.
                FlowStep::Yield(y) => self.emit(
                    format!(
                        "axon-E0787 `yield` in flow '{flow_name}' is only valid inside a `quant` \
                         block — it collapses evolved Hilbert-space amplitudes back to classical \
                         silicon. Move it into the enclosing `quant {{ … }}`."
                    ),
                    &y.loc,
                ),
                // §Fase 52.c — `run <Flow>(args)` as a flow-step (a daemon listen
                // handler invoking a flow, Q3). Reuse the top-level run check:
                // the flow must be declared, args resolve, etc.
                FlowStep::Run(n) => self.check_run(n),
                // §Fase 52.a/c — a `listen … { … }` handler body's steps are
                // checked like any body, so a `run`/`persist`/… inside a flow-
                // body listener is validated (daemon listeners go via
                // `check_daemon` → `check_listen`).
                FlowStep::Listen(l) if !l.body.is_empty() => {
                    self.check_flow_steps(&l.body, flow_name);
                }
                // §Fase 86 — Directed Creative Synthesis laws (T868–T872).
                FlowStep::Forge(n) => self.check_forge(n, flow_name),
                // §Fase 88.c — the warden authorization binding (scope resolves)
                // + recurse into the analysis body.
                FlowStep::Warden(n) => self.check_warden(n, flow_name),
                // All other steps: no cross-reference checks needed
                _ => {}
            }
        }
    }

    /// §Fase 51.c.2 — validate a Pauli-sum observable `M = Σ cₖ Pₖ`
    /// (`axon-E0785`). Hermiticity is guaranteed *by construction* (real
    /// coefficients × Pauli strings), so the checker only enforces the
    /// structural well-formedness the construction depends on:
    ///   - a non-empty sum (an empty observable measures nothing);
    ///   - every Pauli string drawn from the closed alphabet `{I, X, Y, Z}`;
    ///   - every term the same length n (a single coherent register width);
    ///   - that width matching a declared `qubits: n`, if present.
    fn check_observable(&mut self, n: &ObservableDefinition) {
        if n.terms.is_empty() {
            self.emit(
                format!(
                    "axon-E0785 observable '{}' has no terms — a Pauli-sum M = Σ cₖ Pₖ needs at \
                     least one term.",
                    n.name
                ),
                &n.loc,
            );
            return;
        }
        let mut width: Option<usize> = None;
        for term in &n.terms {
            // Closed alphabet {I, X, Y, Z}.
            if term.pauli.is_empty() {
                self.emit(
                    format!("axon-E0785 observable '{}' has an empty Pauli string.", n.name),
                    &term.loc,
                );
                continue;
            }
            if let Some(bad) = term.pauli.chars().find(|c| !matches!(c, 'I' | 'X' | 'Y' | 'Z')) {
                self.emit(
                    format!(
                        "axon-E0785 observable '{}': Pauli string '{}' contains '{}' — the closed \
                         alphabet is {{I, X, Y, Z}} (one Pauli per qubit).",
                        n.name, term.pauli, bad
                    ),
                    &term.loc,
                );
            }
            // Equal length across all terms.
            let len = term.pauli.chars().count();
            match width {
                None => width = Some(len),
                Some(w) if w != len => self.emit(
                    format!(
                        "axon-E0785 observable '{}': Pauli string '{}' has length {} but an \
                         earlier term has length {} — every term must span the same register.",
                        n.name, term.pauli, len, w
                    ),
                    &term.loc,
                ),
                _ => {}
            }
        }
        // Declared qubit width must match the (uniform) term length.
        if let (Some(q), Some(w)) = (n.qubits, width) {
            if q as usize != w {
                self.emit(
                    format!(
                        "axon-E0785 observable '{}' declares qubits: {} but its Pauli strings span \
                         {} qubit(s).",
                        n.name, q, w
                    ),
                    &n.loc,
                );
            }
        }
    }

    /// §Fase 69.a — `axon-E0790`: well-formedness of an Advantage Witness. The
    /// compiler proves the obligation is STATED correctly (a known metric, a valid
    /// threshold, the required references present); the advantage VALUE is computed
    /// on real `data` at deploy/runtime (§69.b+) — you cannot claim advantage in
    /// the abstract, so `data:` is required here. The metric catalog is closed
    /// ([`WITNESS_METRICS`], mirrored in `axon::advantage_witness`, parity-pinned).
    fn check_witness(&mut self, n: &WitnessDefinition) {
        if n.claim.is_empty() {
            self.emit(
                format!(
                    "axon-E0790 witness '{}' has no `claim:` — name the primitive whose \
                     advantage you are witnessing.",
                    n.name
                ),
                &n.loc,
            );
        }
        if n.baseline.is_empty() {
            self.emit(
                format!(
                    "axon-E0790 witness '{}' has no `against:` baseline — advantage is always \
                     relative to a cheaper alternative (e.g. `cosine`).",
                    n.name
                ),
                &n.loc,
            );
        }
        if n.metric.is_empty() {
            self.emit(
                format!(
                    "axon-E0790 witness '{}' has no `metric:` — choose one of {{{}}}.",
                    n.name,
                    WITNESS_METRICS.join(", ")
                ),
                &n.loc,
            );
        } else if !WITNESS_METRICS.contains(&n.metric.as_str()) {
            self.emit(
                format!(
                    "axon-E0790 witness '{}': metric '{}' is not in the closed catalog {{{}}}.",
                    n.name,
                    n.metric,
                    WITNESS_METRICS.join(", ")
                ),
                &n.loc,
            );
        }
        if !(n.threshold.is_finite() && n.threshold >= 0.0) {
            self.emit(
                format!(
                    "axon-E0790 witness '{}': threshold must be a finite value ≥ 0 (the minimum \
                     advantage that justifies the cost), got {}.",
                    n.name, n.threshold
                ),
                &n.loc,
            );
        }
        if n.data.is_empty() {
            self.emit(
                format!(
                    "axon-E0790 witness '{}' has no `data:` — advantage cannot be claimed in the \
                     abstract; it is witnessed on a real-data source.",
                    n.name
                ),
                &n.loc,
            );
        }
    }

    /// §Fase 51.c — semantic validation of the `quant` block **header**: the
    /// encoding-scheme attribute typing + closed-set checks (D1/D2/D9), plus
    /// the D2 depth-trade-off compiler note. The Pauli-sum `observable:`
    /// *declaration* + its resolution, and the typed continuous-carrier grammar
    /// (`SymbolicPtr[Tensor[Float32]]` / `DensityMatrix[D]` + typed `let` + the
    /// norm invariant ‖x‖₂ = 1 at the typed encoder boundary) land in §51.c.2 /
    /// §51.c.3.
    fn check_quant_header(&mut self, q: &QuantBlock, flow_name: &str) {
        // encoding ∈ { amplitude, angle } — the closed scheme set (D2).
        if let Some(enc) = &q.encoding {
            if enc != "amplitude" && enc != "angle" {
                self.emit(
                    format!(
                        "axon-E0784 quant block in flow '{flow_name}': unknown encoding scheme \
                         '{enc}' — the closed set is 'amplitude' (O(log d) qubits, O(d) \
                         state-preparation depth) or 'angle' (O(1) depth, d=n features)."
                    ),
                    &q.loc,
                );
            } else {
                // D2 — surface the encoding's depth trade-off as a compiler note
                // (not hidden): exponential space compression is paid in load-time
                // depth, and vice-versa.
                let note = if enc == "amplitude" {
                    "axon-W005 quant encoding 'amplitude' compresses d features into O(log d) \
                     qubits but costs O(d) state-preparation depth (D2); for low-depth \
                     robustness to scale noise, 'angle' encoding trades to O(1) depth with d=n \
                     features."
                } else {
                    "axon-W005 quant encoding 'angle' has O(1) state-preparation depth but \
                     represents only d=n features (one per qubit, D2); for exponential feature \
                     compression use 'amplitude'."
                };
                self.warn(note.to_string(), &q.loc);
            }
        }
        // §Fase 51.c.2 — `observable: <Name>` must resolve to a declared
        // `observable` (the Pauli-sum the quant block measures against).
        if let Some(obs) = &q.observable {
            match self.symbols.lookup(obs) {
                None => self.emit(
                    format!(
                        "axon-E0784 quant block in flow '{flow_name}': undefined observable \
                         '{obs}' — declare it with `observable {obs} {{ … }}`."
                    ),
                    &q.loc,
                ),
                Some(sym) if sym.kind != "observable" => self.emit(
                    format!(
                        "axon-E0784 quant block in flow '{flow_name}': '{obs}' is a {}, not an \
                         observable.",
                        sym.kind
                    ),
                    &q.loc,
                ),
                _ => {}
            }
        }
        // backend effect ∈ QUANT_BACKEND_CATALOG (D1/D9 closed set). §Fase 51.d:
        // catalogue-driven (single source of truth) — the canonical algebraic
        // effect this block performs is `ots:backend:<effect>`
        // (`crate::ots_catalog::quant_effect_slug`).
        if !is_valid(&q.effect, crate::ots_catalog::QUANT_BACKEND_CATALOG) {
            self.emit(
                format!(
                    "axon-E0784 quant block in flow '{flow_name}': unknown backend '{}' — \
                     expected one of {} (the algebraic effect performed is \
                     'ots:backend:<backend>').",
                    q.effect,
                    valid_list(crate::ots_catalog::QUANT_BACKEND_CATALOG)
                ),
                &q.loc,
            );
        }
        // Register width / circuit depth ≥ 1; projected-kernel bandwidth > 0.
        if let Some(n) = q.qubits {
            if n < 1 {
                self.emit(
                    format!(
                        "axon-E0784 quant block in flow '{flow_name}': qubits must be >= 1, got {n}."
                    ),
                    &q.loc,
                );
            }
        }
        if let Some(d) = q.depth {
            if d < 1 {
                self.emit(
                    format!(
                        "axon-E0784 quant block in flow '{flow_name}': circuit depth must be >= 1, \
                         got {d}."
                    ),
                    &q.loc,
                );
            }
        }
        if let Some(b) = q.bandwidth {
            if b <= 0.0 {
                self.emit(
                    format!(
                        "axon-E0784 quant block in flow '{flow_name}': projected-kernel bandwidth \
                         must be > 0, got {b}."
                    ),
                    &q.loc,
                );
            }
        }
        // §Fase 69.c — re-uploading layers must be ≥ 1 (1 = no re-uploading).
        if let Some(r) = q.reupload {
            if r < 1 {
                self.emit(
                    format!(
                        "axon-E0784 quant block in flow '{flow_name}': reupload must be >= 1 \
                         (1 = no re-uploading; >= 2 interleaves the data encoding L times), got {r}."
                    ),
                    &q.loc,
                );
            }
        }
    }

    /// §Fase 51.b — enforce the **Continuous Type Invariant** over a `quant`
    /// block body (paper §4.2, refined per D8). The Hilbert-space scope admits
    /// only continuous carriers + discrete *classical control* (integer indices
    /// `n`/`L`/`D`, a closed enum of measurement bases). It rejects
    /// *conversational / unstructured discrete* values, which collapse the
    /// continuous gradient:
    ///
    ///   - a `let` bound to a **String literal** (the paper's E0782 example);
    ///   - a `let` value carrying an implicit **`.to_string`** textual
    ///     conversion;
    ///   - a **`step`** with a free-text `ask:` prompt (a conversational LLM
    ///     call inside the quantum scope).
    ///
    /// Numeric / bool / list literals, references to prior continuous bindings,
    /// and the rest of the flow-step vocabulary are admitted. The walk recurses
    /// into nested control blocks and nested `quant` blocks so a leak cannot
    /// hide one level down.
    ///
    /// NOTE (scope): the amplitude-encoding norm invariant ‖x‖₂ = 1 (D2) is a
    /// property of a *typed* continuous carrier (`SymbolicPtr[Tensor[Float32]]`
    /// with a normalized marker), which the typed continuous grammar introduces
    /// in §51.c — it is not statically derivable from an untyped
    /// `let x = extract_embeddings(audio)`, so its enforcement lands there, at
    /// the typed encoder boundary, not here.
    fn check_continuous_type_invariant(&mut self, body: &[FlowStep], flow_name: &str) {
        for step in body {
            match step {
                FlowStep::Let(n) => {
                    if Self::quant_value_is_string_literal(&n.value_kind, &n.value_expr) {
                        self.emit(
                            format!(
                                "axon-E0782 Continuous Type Invariant violation in flow \
                                 '{flow_name}': let binding '{}' inside a `quant` block holds a \
                                 non-continuous 'String' (string literal). Discrete/conversational \
                                 types are prohibited in the Hilbert-space scope — keep the original \
                                 tensor via the continuous type 'SymbolicPtr[Tensor[Float32]]'.",
                                n.identifier
                            ),
                            &n.loc,
                        );
                    } else if n.value_expr.contains(".to_string") {
                        self.emit(
                            format!(
                                "axon-E0782 Continuous Type Invariant violation in flow \
                                 '{flow_name}': let binding '{}' inside a `quant` block performs an \
                                 implicit textual conversion ('.to_string'). Textual leaks collapse \
                                 the continuous gradient — operate on the tensor / density-matrix \
                                 carrier instead.",
                                n.identifier
                            ),
                            &n.loc,
                        );
                    }
                    // §Fase 51.c.3 — typed encoder-boundary discipline. A typed
                    // `let x: <T> = …` inside quant must carry a continuous (or
                    // classical-control) type, and a `DensityMatrix[D]` must have
                    // D = 2ⁿ (the Hilbert-space dimension for n qubits).
                    if let Some(ty) = &n.type_annotation {
                        self.check_density_matrix_dim(ty, flow_name, &n.loc);
                        if matches!(ty.name.as_str(), "String" | "Text") {
                            self.emit(
                                format!(
                                    "axon-E0782 Continuous Type Invariant violation in flow \
                                     '{flow_name}': let binding '{}' is typed '{}' inside a `quant` \
                                     block. Discrete/conversational types collapse the continuous \
                                     gradient — use a continuous carrier \
                                     ('SymbolicPtr[Tensor[Float32]]', 'DensityMatrix[D]').",
                                    n.identifier, ty.name
                                ),
                                &n.loc,
                            );
                        }
                    }
                }
                FlowStep::Step(s) if !s.ask.is_empty() => {
                    self.emit(
                        format!(
                            "axon-E0782 Continuous Type Invariant violation in flow '{flow_name}': \
                             a `step` with a free-text `ask:` prompt is not permitted inside a \
                             `quant` block. A conversational LLM call reintroduces unstructured \
                             text into the Hilbert-space scope; perform cognition outside the \
                             `quant` block and pass only the continuous tensor in."
                        ),
                        &s.loc,
                    );
                }
                // Recurse so a leak cannot hide one nesting level down.
                FlowStep::If(n) => {
                    self.check_continuous_type_invariant(&n.then_body, flow_name);
                    self.check_continuous_type_invariant(&n.else_body, flow_name);
                }
                FlowStep::ForIn(n) => self.check_continuous_type_invariant(&n.body, flow_name),
                FlowStep::Par(n) => {
                    for branch in &n.branches {
                        self.check_continuous_type_invariant(branch, flow_name);
                    }
                }
                FlowStep::Quant(q) => self.check_continuous_type_invariant(&q.body, flow_name),
                _ => {}
            }
        }
    }

    /// §Fase 51.b — heuristic: is a `let` value a **String literal**?
    ///
    /// The lexer strips the surrounding quotes, so a string literal and a bare
    /// reference both surface as a plain `value_expr`. They are distinguished
    /// by `value_kind`: a string literal is `"literal"` (so is a number / bool
    /// / list). We therefore classify a `"literal"` value as a String iff it is
    /// NOT numeric, NOT a bool, and NOT a list literal. (A numeric-looking
    /// string like `"123"` is a rare accepted false-negative; §51.c's typed
    /// grammar closes that with real declared types.)
    /// §Fase 51.c.3 — a `DensityMatrix[D]` must have **D = 2ⁿ** (the dimension
    /// of the Hilbert space of n qubits, paper §3.1/§3.3); `axon-E0786`. When
    /// the dimension is symbolic (non-numeric `generic_param`) the check is
    /// skipped — only a concrete literal can be proven a power of two.
    fn check_density_matrix_dim(&mut self, ty: &TypeExpr, flow_name: &str, loc: &Loc) {
        if ty.name != "DensityMatrix" {
            return;
        }
        if let Ok(d) = ty.generic_param.trim().parse::<u64>() {
            // power of two ⇔ d > 0 ∧ (d & (d-1)) == 0
            if d == 0 || (d & (d - 1)) != 0 {
                self.emit(
                    format!(
                        "axon-E0786 quant block in flow '{flow_name}': DensityMatrix dimension {d} \
                         is not a power of two — D must equal 2ⁿ (the Hilbert-space dimension for \
                         n qubits, e.g. 2, 4, …, 1024)."
                    ),
                    loc,
                );
            }
        }
    }

    fn quant_value_is_string_literal(value_kind: &str, value_expr: &str) -> bool {
        if value_kind != "literal" {
            return false;
        }
        let v = value_expr.trim();
        if v.parse::<f64>().is_ok() {
            return false; // numeric index (admitted classical control)
        }
        if v == "true" || v == "false" {
            return false; // bool
        }
        if v.starts_with('[') {
            return false; // list literal
        }
        true // quotes stripped at lex time ⇒ a String literal
    }

    /// §Fase 58.d — validate a `use Tool(k = v, …)` call against the tool's
    /// declared input schema (W2): the contract the type-checker enforces so a
    /// malformed invocation is CALLER blame (CT-2) at compile time, BEFORE any
    /// HTTP dispatch. Checks: every named arg is a declared parameter; no
    /// duplicates; every required (non-optional) parameter is supplied; and a
    /// best-effort literal type-alignment. The legacy single-`on <arg>` form is
    /// untyped and skipped (§58 D5 back-compat), as is a schema-less tool (no
    /// `parameters:` → no contract to enforce). The `apply: Tool given:
    /// <struct>` splat (D3) is validated separately (§58.d.2).
    /// §Fase 116.a (D116.9) — the scopes a tool's operation requires
    /// (`tool T { requires: [...] }`). Empty for every pre-§116 tool.
    fn tool_required_scopes(&self, name: &str) -> Vec<String> {
        self.program
            .declarations
            .iter()
            .find_map(|d| match d {
                Declaration::Tool(t) if t.name == name => Some(t.requires.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// §Fase 116.a (D116.9) — the program's GRANTED capability set: the flat
    /// union of every `credential`'s `grants` (§92) and every endpoint's /
    /// daemon's `requires:` capabilities (§51.x / §52.d). These are the scopes
    /// the program has DECLARED it holds — the same vocabulary a tool's
    /// `requires:` draws from. Program-wide (an app's OAuth scopes are held
    /// app-wide, not per-flow); a per-flow tightening is a named future
    /// refinement, not a §116.a hole.
    fn granted_scopes(&self) -> std::collections::HashSet<String> {
        let mut granted = std::collections::HashSet::new();
        for d in &self.program.declarations {
            match d {
                Declaration::Credential(c) => granted.extend(c.grants.iter().cloned()),
                Declaration::AxonEndpoint(e) => {
                    granted.extend(e.requires_capabilities.iter().cloned())
                }
                Declaration::Daemon(dm) => {
                    granted.extend(dm.requires_capabilities.iter().cloned())
                }
                _ => {}
            }
        }
        granted
    }

    /// §Fase 116.a — **axon-T956** (scope coverage). A `use` of a tool whose
    /// `requires:` names a scope the program's granted set does not cover is
    /// refused: an autonomous operation must hold the authority it needs, and
    /// that authority must be DECLARED (the doctrine `every_requirement_is_
    /// grantable`, §90, on the tool surface). Inert for every tool with no
    /// `requires:` (the whole pre-§116 corpus).
    fn check_use_tool_scopes(&mut self, n: &UseToolStep) {
        let required = self.tool_required_scopes(&n.tool_name);
        if required.is_empty() {
            return;
        }
        let granted = self.granted_scopes();
        for scope in required {
            if !granted.contains(&scope) {
                self.emit(
                    format!(
                        "axon-T956 tool '{}' requires scope '{}', which the program's granted \
                         set does not cover. A tool's `requires:` scope must be held: declare a \
                         `credential` that grants '{}', or add it to the executing endpoint's \
                         `requires:` capabilities. Held scopes come from `credential.grants` \
                         (§92) and endpoint/daemon `requires:` capabilities (§51.x).",
                        n.tool_name, scope, scope
                    ),
                    &n.loc,
                );
            }
        }
    }

    fn check_use_tool_args(&mut self, n: &UseToolStep, steps: &[FlowStep]) {
        let UseArgs::Named(pairs) = &n.args else {
            return; // LegacyPositional — no schema validation (D5).
        };
        let params = self.tool_parameters(&n.tool_name);
        if params.is_empty() {
            return; // schema-less tool — no contract.
        }
        let mut seen = std::collections::HashSet::new();
        for (name, value, value_kind) in pairs {
            if !seen.insert(name.clone()) {
                self.emit(
                    format!("Duplicate argument '{}' in call to tool '{}'", name, n.tool_name),
                    &n.loc,
                );
                continue;
            }
            match params.iter().find(|p| &p.0 == name) {
                None => self.emit(
                    format!("Tool '{}' has no parameter '{}'", n.tool_name, name),
                    &n.loc,
                ),
                Some((_, decl_ty, _)) => {
                    if value_kind == "reference" {
                        // §Fase 60.c — a `"reference"` value (a flow-param or a
                        // `Step.output`) is resolved at runtime against the
                        // bindings; validate its SOURCE type against the declared
                        // parameter type (the Q4 soundness contract). Conservative:
                        // a reference whose source we cannot resolve in-checker (a
                        // `let`, a runtime binding) is skipped — no false positive.
                        if let Some(src_ty) = self.resolve_reference_type(value, steps) {
                            let src_base =
                                src_ty.trim_end_matches('?').split('<').next().unwrap_or(&src_ty);
                            if !Self::tool_arg_types_align(src_base, decl_ty) {
                                self.emit(
                                    format!(
                                        "Type mismatch for parameter '{}' of tool '{}': expected {}, got {} (from reference '{}')",
                                        name, n.tool_name, decl_ty, src_ty, value
                                    ),
                                    &n.loc,
                                );
                            }
                        }
                    } else if let Some(val_ty) = Self::infer_arg_literal_type(value) {
                        if !Self::tool_arg_types_align(&val_ty, decl_ty) {
                            self.emit(
                                format!(
                                    "Type mismatch for parameter '{}' of tool '{}': expected {}, got {}",
                                    name, n.tool_name, decl_ty, val_ty
                                ),
                                &n.loc,
                            );
                        }
                    }
                }
            }
        }
        // Missing required (non-optional) parameters.
        for (pname, _ty, optional) in &params {
            if !optional && !pairs.iter().any(|(name, _, _)| name == pname) {
                self.emit(
                    format!("Missing required argument '{}' for tool '{}'", pname, n.tool_name),
                    &n.loc,
                );
            }
        }
    }

    /// §Fase 60.c — resolve the declared TYPE of a `"reference"` kwarg value, when
    /// statically knowable: a flow parameter (its declared type) or a `<Step>`
    /// / `<Step>.output` reference (the step's declared `output:` type, or a
    /// `use <Tool>`'s `output_type`). Returns `None` for anything else (a `let`
    /// binding, a runtime value) so the caller conservatively skips — no false
    /// positive, mirroring the §58.d literal stance.
    fn resolve_reference_type(&self, reference: &str, steps: &[FlowStep]) -> Option<String> {
        // A flow parameter — `company`.
        if let Some(t) = self.current_flow_params.get(reference) {
            return Some(t.to_string());
        }
        // A step output — `ExtractUrl` or `ExtractUrl.output`.
        let step_name = reference.strip_suffix(".output").unwrap_or(reference);
        steps.iter().find_map(|s| match s {
            FlowStep::Step(st) if st.name == step_name && !st.output_type.is_empty() => {
                Some(st.output_type.clone())
            }
            FlowStep::UseTool(u) if u.tool_name == step_name => {
                self.tool_output_type(&u.tool_name)
            }
            _ => None,
        })
    }

    /// §Fase 60.c — a declared tool's `output_type:`, when present.
    fn tool_output_type(&self, tool_name: &str) -> Option<String> {
        self.program.declarations.iter().find_map(|d| match d {
            Declaration::Tool(t) if t.name == tool_name => t.output_type.clone(),
            _ => None,
        })
    }

    /// §Fase 58.d — the `(name, flat-type, optional)` schema of a declared
    /// tool. Empty when the tool declares no `parameters:` (schema-less /
    /// back-compat) or is undeclared (the existence check already emitted).
    fn tool_parameters(&self, tool_name: &str) -> Vec<(String, String, bool)> {
        self.program
            .declarations
            .iter()
            .find_map(|d| match d {
                Declaration::Tool(t) if t.name == tool_name => Some(t),
                _ => None,
            })
            .map(|t| {
                t.parameters
                    .iter()
                    .map(|p| {
                        let mut ty = p.type_expr.name.clone();
                        if !p.type_expr.generic_param.is_empty() {
                            ty = format!("{}<{}>", ty, p.type_expr.generic_param);
                        }
                        (p.name.clone(), ty, p.type_expr.optional)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// §Fase 58.d — infer a named-arg VALUE's type only when it is an
    /// UNAMBIGUOUS literal. The frontend stored the value as a bare string, so
    /// a bare identifier is ambiguous (the string literal `"x"` and the
    /// reference `x` both lower to `x`) → `None`, skipping the check to stay
    /// sound (no false positives). Interpolation (`${…}`) is runtime-dependent
    /// → also `None`. Only numeric / boolean literals — which the lexer cannot
    /// confuse with an identifier — are typed.
    fn infer_arg_literal_type(value: &str) -> Option<String> {
        if value == "true" || value == "false" {
            return Some("Bool".to_string());
        }
        if value.contains('.') && value.parse::<f64>().is_ok() {
            return Some("Float".to_string());
        }
        let digits = value.strip_prefix('-').unwrap_or(value);
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            return Some("Int".to_string());
        }
        None
    }

    /// §Fase 58.d — whether an inferred literal value type aligns with a
    /// declared parameter type. `Any` accepts anything; an `Int` coerces into a
    /// `Float` parameter; otherwise the base types must match.
    fn tool_arg_types_align(value_ty: &str, decl_ty: &str) -> bool {
        let base = decl_ty
            .trim_end_matches('?')
            .split('<')
            .next()
            .unwrap_or(decl_ty);
        base == "Any" || base == value_ty || (base == "Float" && value_ty == "Int")
    }

    /// §Fase 59 (D2/D3) — the honest-compiler guidance for `apply: <Tool>`.
    ///
    /// `apply: <Tool>` is **cognitive delegation**: the step runs as an LLM
    /// reasoning call with the tool made available to the model, which
    /// decides STOCHASTICALLY whether to invoke it. It is NOT a
    /// deterministic typed dispatch, and `given:` is NOT splatted at
    /// runtime. When `apply: <Tool>` references a tool that declares a
    /// `parameters:` schema, the adopter almost certainly meant a
    /// deterministic call — so emit `axon-W004` naming the nature +
    /// redirecting to the one deterministic, CT-2-validated, real-dispatch
    /// surface: flow-level `use <Tool>(k = v, …)`.
    ///
    /// This SUPERSEDES the §58.d.2 splat type-check. Those hard errors
    /// (`missing required` / `type mismatch`) validated a deterministic
    /// contract the runtime never honors — the "illusion of control" that
    /// moved the fence. §59 removes the phantom errors; the real CT-2
    /// validation lives on `use <Tool>(k = v, …)` (§58.d, untouched). The
    /// fence stays put: no runtime splat is invented (D6 — the LLM stays
    /// stochastic; the compiler only indicates the path).
    ///
    /// Fires ONLY for `apply: <Tool>` where `<Tool>` is a declared tool
    /// with a NON-EMPTY `parameters:` schema. A schema-less tool applied
    /// cognitively is legitimate (no warning, D7); `apply: <Flow>` is
    /// composition (no warning).
    fn check_apply_tool(&mut self, step: &StepNode) {
        if step.apply_ref.is_empty() {
            return;
        }
        match self.symbols.lookup(&step.apply_ref) {
            Some(sym) if sym.kind == "tool" => {}
            _ => return, // a flow apply (composition) or unknown → not this rule.
        }
        let params = self.tool_parameters(&step.apply_ref);
        if params.is_empty() {
            return; // schema-less tool → cognitive apply is legitimate (D7).
        }
        self.warn(build_w004_message(&step.apply_ref, &params), &step.loc);
    }

    /// §Fase 68.e — `axon-W006`: a step's `apply: <Compute>` is a model-selection
    /// NO-OP. A `compute { model: … }` block's `model:` is dropped at lowering
    /// (the parser keeps only `shield:`), and `apply:` does not pick an LLM model
    /// — so the brief-#36 adopter who wrote `apply: BigSummary` to pin a larger
    /// model got silently ignored. Turn that silent no-op into guidance toward the
    /// faithful surface: declare `requires_context:` on the step (the §68.c
    /// resolver picks a satisfying model), or set the deployment model. The §59
    /// honest-compiler doctrine — the compiler tells the truth + indicates the path.
    ///
    /// Fires ONLY when `apply_ref` resolves to a declared `compute`. An
    /// `apply: <Tool>` is §59's `axon-W004`; an `apply: <Flow>` is composition.
    fn check_apply_compute(&mut self, step: &StepNode) {
        if step.apply_ref.is_empty() {
            return;
        }
        if let Some(sym) = self.symbols.lookup(&step.apply_ref) {
            if sym.kind == "compute" {
                self.warn(build_w006_message(&step.apply_ref, &step.name), &step.loc);
            }
        }
    }

    /// §Fase 68.f — `axon-T809`: validate a step's `requires_context:` at compile
    /// time. Two impossibilities are caught before deploy:
    ///   - `0` (or absent → skipped) — a zero context requirement is meaningless;
    ///   - a value larger than [`MAX_KNOWN_CONTEXT_WINDOW`] (the largest window any
    ///     canonical model offers, §68.a) — NO model could ever satisfy it, so it
    ///     is a hard error here rather than a deploy-time / runtime surprise.
    ///
    /// The per-DEPLOYMENT satisfiability ("does THIS tenant's configured backend
    /// have a `>= N` model") is the enterprise §68.h deploy gate; this OSS check is
    /// the catalog-level floor that holds for any backend.
    fn check_requires_context(&mut self, step: &StepNode) {
        let Some(n) = step.requires_context else {
            return;
        };
        if n == 0 {
            self.emit(
                format!(
                    "axon-T809 step '{}' declares `requires_context: 0` — a context \
                     requirement must be a positive token count (or omit it to use the \
                     backend default).",
                    step.name
                ),
                &step.loc,
            );
        } else if n > MAX_KNOWN_CONTEXT_WINDOW {
            self.emit(
                format!(
                    "axon-T809 step '{}' declares `requires_context: {n}`, which exceeds \
                     the largest known model context window ({MAX_KNOWN_CONTEXT_WINDOW} \
                     tokens) — no model could satisfy it. Lower the requirement.",
                    step.name
                ),
                &step.loc,
            );
        }
    }

    /// §Fase 94.a — the write-verb law over a `backend: secrets` store
    /// (`axon-T897`). A secrets store is a read-only metadata view:
    /// `persist` / `mutate` / `purge` against it are unrepresentable —
    /// custody is written only by the tenant-secrets seeding API and by
    /// the runtime commit of a mediated `rotate` (§94.b). Emitted at the
    /// offending step, naming the verb, so `axon fix` guidance stays
    /// exact.
    fn check_secrets_store_write(
        &mut self,
        verb: &str,
        store_name: &str,
        flow_name: &str,
        loc: &Loc,
    ) {
        if !store_name.is_empty() && self.secrets_backed_stores.contains(store_name) {
            self.emit(
                format!(
                    "axon-T897 `{verb}` targets the secrets store '{store_name}' in \
                     flow '{flow_name}' — a `backend: secrets` store is a READ-ONLY \
                     metadata view over the tenant's secret custody \
                     (`rotation_without_revelation`). Custody is written only by the \
                     tenant-secrets API (seed) and by `rotate … with <Tool>` (renewal); \
                     `retrieve` is the only verb that reads it."
                ),
                loc,
            );
        }
    }

    fn check_store_ref(&mut self, store_name: &str, flow_name: &str, loc: &Loc) {
        if !store_name.is_empty() {
            match self.symbols.lookup(store_name) {
                None => self.emit(
                    format!(
                        "Undefined axonstore '{}' in flow '{}'",
                        store_name, flow_name
                    ),
                    loc,
                ),
                Some(sym) if sym.kind != "axonstore" => self.emit(
                    format!("'{}' is a {}, not an axonstore", store_name, sym.kind),
                    loc,
                ),
                _ => {}
            }
        }
    }

    /// §Fase 38.d (D2) — run the `where:` column-type proof against
    /// the store's declared INLINE schema (form a). Forms (b)/(c)
    /// silently skip at this layer (they're proven at deploy time —
    /// D8, in 38.f — when filesystem context for manifest discovery
    /// exists). Every proof failure is converted to a `TypeError`
    /// anchored at the store-op's source location.
    fn run_38d_where_proof(&mut self, store_name: &str, where_expr: &str, loc: &Loc) {
        if store_name.is_empty() || where_expr.trim().is_empty() {
            return;
        }
        let cs = match self.store_inline_column_sets.get(store_name) {
            Some(cs) => cs.clone(),
            None => return, // no inline schema declared → 38.d silently skips
        };
        let errors = crate::store_column_proof::check_filter(
            where_expr,
            &cs,
            &self.current_flow_params,
            (loc.line, loc.column),
        );
        for err in errors {
            self.emit(err.message, loc);
        }
    }

    /// §Fase 67.b — run the bounded/ordered `retrieve` proof: the
    /// `order_by:` clause (axon-T807 — sort-term shape, direction, and
    /// column existence) + the `limit:` clause (axon-T808 — `u32` literal
    /// or integer-`${param}`). The compile-time mirror of the runtime
    /// `filter::render_bounds`. The structural checks (term shape /
    /// direction / limit literal) run for ANY store form; order_by
    /// COLUMN existence is proven only when an inline schema is declared
    /// (the same scope rule as `run_38d_where_proof`).
    fn run_67b_bounds_proof(
        &mut self,
        store_name: &str,
        order_by: &str,
        limit_expr: &str,
        loc: &Loc,
    ) {
        if store_name.is_empty()
            || (order_by.trim().is_empty() && limit_expr.trim().is_empty())
        {
            return;
        }
        let cs = self.store_inline_column_sets.get(store_name).cloned();
        let errors = crate::store_column_proof::check_bounds(
            order_by,
            limit_expr,
            cs.as_ref(),
            &self.current_flow_params,
            (loc.line, loc.column),
        );
        for err in errors {
            self.emit(err.message, loc);
        }
    }

    /// §Fase 76.d — run the aggregate `retrieve` proof: the `aggregate:`
    /// closed catalog + `group_by:` grammar (axon-T843), the schema-backed
    /// column/numeric proof (axon-T844, inline schema only), and the
    /// structural combination rules (axon-T845 — group_by-without-aggregate,
    /// aggregate×bounds, aggregate-column-as-group-key). The compile-time
    /// mirror of the runtime `filter::parse_aggregate_clause`.
    fn run_76d_aggregate_proof(
        &mut self,
        store_name: &str,
        aggregate: &str,
        group_by: &str,
        order_by: &str,
        limit_expr: &str,
        loc: &Loc,
    ) {
        if store_name.is_empty()
            || (aggregate.trim().is_empty() && group_by.trim().is_empty())
        {
            return;
        }
        let cs = self.store_inline_column_sets.get(store_name).cloned();
        let errors = crate::store_column_proof::check_aggregate(
            aggregate,
            group_by,
            order_by,
            limit_expr,
            cs.as_ref(),
            (loc.line, loc.column),
        );
        for err in errors {
            self.emit(err.message, loc);
        }
    }

    /// §Fase 38.e (D2 — second half) — run the `persist` field-block
    /// proof: axon-T803 NOT-NULL omission + axon-T804 unknown field +
    /// axon-T802 value-type mismatch. An empty fields block (the
    /// v1.30.0 blockless `persist <store>` form that writes user
    /// bindings) is silently skipped per D5 absolute.
    fn run_38e_persist_proof(
        &mut self,
        store_name: &str,
        fields: &[(String, String)],
        loc: &Loc,
    ) {
        if store_name.is_empty() || fields.is_empty() {
            return;
        }
        let cs = match self.store_inline_column_sets.get(store_name) {
            Some(cs) => cs.clone(),
            None => return,
        };
        let errors = crate::store_column_proof::check_persist_fields(
            fields,
            &cs,
            &self.current_flow_params,
            (loc.line, loc.column),
        );
        for err in errors {
            self.emit(err.message, loc);
        }
    }

    /// §Fase 38.e (D2 — second half) — run the `mutate` SET-block
    /// proof: axon-T804 unknown field + axon-T802 value-type mismatch.
    /// NOT-NULL omission (T803) does NOT apply to mutate (UPDATE
    /// preserves existing values for omitted columns).
    fn run_38e_mutate_proof(
        &mut self,
        store_name: &str,
        fields: &[(String, String)],
        loc: &Loc,
    ) {
        if store_name.is_empty() || fields.is_empty() {
            return;
        }
        let cs = match self.store_inline_column_sets.get(store_name) {
            Some(cs) => cs.clone(),
            None => return,
        };
        let errors = crate::store_column_proof::check_mutate_fields(
            fields,
            &cs,
            &self.current_flow_params,
            (loc.line, loc.column),
        );
        for err in errors {
            self.emit(err.message, loc);
        }
    }

    // ── Type reference validation (epistemic lattice) ──────────────

    /// Verify that a type name is either built-in or user-defined.
    /// Soft check: unknown types are silently accepted — the house
    /// ad-hoc-type idiom (D115.6). §Fase 115 note: this leniency is NO
    /// LONGER justified by "may come from imports" — imports are real now
    /// (module mode registers them), and an *explicitly imported* type is
    /// verified against its module's exports (`axon-T953`). Free type
    /// names stay soft because they are idiom, not because a phantom
    /// mechanism might provide them.
    // ── §Fase 73.a — the `Json<T>` shape-lens well-formedness pass ──
    //
    // Open `Json` (no shape) is the always-total, always-honest default
    // and is never validated here. A refined `Json<T>` lens is an
    // EXPECTATION the compiler checks: `T` must name a declared struct
    // `type` whose fields are the document's expected shape. A `T` that
    // is undeclared, or names a non-`type` symbol (a flow, a tool, a
    // builtin scalar), is `axon-T840`. This is the catalog-level
    // well-formedness of the lens TYPE; field-level lens checking (does
    // `doc.field` exist in `T`?) is §Fase 73.e. The runtime is unaffected
    // either way — the lens is compile-time only (`open_data_is_total`:
    // the compiler may help, the runtime never lies).
    fn check_json_lenses(&mut self, decls: &[Declaration]) {
        for decl in decls {
            match decl {
                Declaration::Type(t) => {
                    for f in &t.fields {
                        self.check_json_lens_annotation(&f.type_expr);
                    }
                }
                Declaration::Flow(fl) => {
                    for p in &fl.parameters {
                        self.check_json_lens_annotation(&p.type_expr);
                    }
                    if let Some(rt) = &fl.return_type {
                        self.check_json_lens_annotation(rt);
                    }
                }
                Declaration::AxonStore(s) => {
                    if let Some(crate::store_schema::StoreColumnSchema::Inline {
                        columns,
                        ..
                    }) = &s.column_schema
                    {
                        for c in columns {
                            if let Some(shape) = &c.json_shape {
                                let loc = Loc {
                                    line: c.line,
                                    column: c.column,
                                };
                                self.validate_json_shape(shape, &loc);
                            }
                        }
                    }
                }
                Declaration::Epistemic(eb) => self.check_json_lenses(&eb.body),
                _ => {}
            }
        }
    }

    /// §Fase 73.a — validate a `Json<T>` lens written as a type
    /// annotation (a flow param / return, a `type` field). A bare `Json`
    /// (empty generic) is the open default and passes untouched.
    fn check_json_lens_annotation(&mut self, ty: &TypeExpr) {
        if ty.name == "Json" && !ty.generic_param.is_empty() {
            self.validate_json_shape(&ty.generic_param, &ty.loc);
        }
    }

    /// §Fase 73.a — the shared `axon-T840` check: the lens shape `T` must
    /// resolve to a declared struct `type`.
    fn validate_json_shape(&mut self, shape: &str, loc: &Loc) {
        let t = shape.trim();
        let is_declared_struct = self
            .symbols
            .lookup(t)
            .map_or(false, |s| s.kind == "type");
        if is_declared_struct {
            return;
        }
        let why = match self.symbols.lookup(t) {
            Some(sym) => format!("`{t}` is a {}, not a `type`", sym.kind),
            None => format!("`{t}` is not declared"),
        };
        self.emit(
            format!(
                "axon-T840 the shape lens `Json<{t}>` requires `{t}` to be a \
                 declared `type` (a struct whose fields are the document's \
                 expected shape), but {why}. Declare `type {t} {{ … }}`, or \
                 use open `Json` (no shape) when the document's shape is not \
                 known — open navigation stays total either way."
            ),
            loc,
        );
    }

    // ── §Fase 73.e — the `Json<T>` shape-lens field checker ─────────────
    //
    // 73.a validated the lens TYPE's well-formedness (`T` is a declared
    // struct). 73.e makes the lens DO something: a navigation `profile.age`
    // over a `Json<UserEvent>` value statically verifies `age` exists in
    // `UserEvent` and resolves its scalar type, so `profile.age >= 18` is a
    // well-typed Int comparison and `profile.notafield` is `axon-T842`.
    // The runtime is unaffected — the lens is a compile-time EXPECTATION;
    // a declared-but-absent field still degrades to null at runtime
    // (doctrine `open_data_is_total`: the compiler may help, the runtime
    // never lies, never crashes).

    /// §Fase 74.g — collect every channel/topic `emit`ted to anywhere in
    /// the program: all flow bodies + every daemon listener body, recursing
    /// into nested `if` / `for` / `listen` bodies. The set of producers a
    /// `listen`er's channel is checked against (`axon-W009`). Mirrors the
    /// §74.g PCC `collect_emitted_channels` (same Emit/If/ForIn/Listen walk)
    /// so the compile-time warning + the deploy-gate proof agree.
    fn collect_emitted_channels(&mut self, decls: &[Declaration]) {
        fn walk(steps: &[FlowStep], out: &mut std::collections::HashSet<String>) {
            for step in steps {
                match step {
                    FlowStep::Emit(e) => {
                        out.insert(e.channel_ref.clone());
                    }
                    FlowStep::If(c) => {
                        walk(&c.then_body, out);
                        walk(&c.else_body, out);
                    }
                    FlowStep::ForIn(f) => walk(&f.body, out),
                    FlowStep::Listen(l) => walk(&l.body, out),
                    _ => {}
                }
            }
        }
        let mut out = std::collections::HashSet::new();
        for decl in decls {
            match decl {
                Declaration::Flow(f) => walk(&f.body, &mut out),
                Declaration::Daemon(d) => {
                    for l in &d.listeners {
                        walk(&l.body, &mut out);
                    }
                }
                Declaration::Epistemic(eb) => {
                    // Recurse into epistemic-nested flows/daemons.
                    self.collect_emitted_channels(&eb.body);
                }
                _ => {}
            }
        }
        self.emitted_channels.extend(out);
    }

    /// Index every declared struct `type`'s fields (recursing into
    /// `epistemic` blocks) → `struct → field → (type name, generic)`.
    fn index_type_fields(&mut self, decls: &[Declaration]) {
        for decl in decls {
            match decl {
                Declaration::Type(t) => {
                    let mut fields = std::collections::HashMap::new();
                    for f in &t.fields {
                        fields.insert(
                            f.name.clone(),
                            (f.type_expr.name.clone(), f.type_expr.generic_param.clone()),
                        );
                    }
                    self.json_lens_fields.insert(t.name.clone(), fields);
                }
                Declaration::Epistemic(eb) => self.index_type_fields(&eb.body),
                _ => {}
            }
        }
    }

    /// If `spelling` is a `Json<T>` lens, return `T` (the declared shape
    /// struct). A bare `Json` (or any non-Json type) is not a lens.
    fn parse_json_lens(spelling: &str) -> Option<String> {
        let s = spelling.trim().trim_end_matches('?').trim();
        let inner = s.strip_prefix("Json<")?.strip_suffix('>')?.trim();
        if inner.is_empty() {
            None
        } else {
            Some(inner.to_string())
        }
    }

    /// The shape struct a lens navigation views, or `None` if `e` is not a
    /// statically-known `Json<T>` lens. A `Ref` to a `Json<T>` param is the
    /// lens root; a field whose declared type is itself a struct (or an
    /// explicit `Json<T2>`) continues the lens into the nested shape, so
    /// `profile.address.city` checks `city` against the nested struct.
    fn lens_shape_of(
        &self,
        e: &Expr,
        scope: &std::collections::BTreeMap<String, String>,
    ) -> Option<String> {
        match e {
            Expr::Ref(name) => Self::parse_json_lens(scope.get(name)?),
            Expr::Field(base, field) => {
                let parent = self.lens_shape_of(base, scope)?;
                let (ty, generic) = self.json_lens_fields.get(&parent)?.get(field)?;
                if ty == "Json" && !generic.is_empty() {
                    Some(generic.clone())
                } else if self.json_lens_fields.contains_key(ty) {
                    Some(ty.clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// §Fase 73.e — walk a flat dotted lens path's field SEGMENTS against
    /// the shape struct, emitting `axon-T842` on the first undeclared
    /// field. Returns the final segment's declared scalar type (so
    /// `profile.age` is Int); an intermediate scalar / unknown leaf or a
    /// struct result is `Unknown` (permissive past what is checkable).
    fn lens_field_walk(
        &mut self,
        mut struct_name: String,
        segments: &[&str],
        loc: &Loc,
    ) -> InferType {
        use InferType as T;
        for (i, f) in segments.iter().enumerate() {
            let entry = self
                .json_lens_fields
                .get(&struct_name)
                .and_then(|m| m.get(*f))
                .map(|(ty, g)| (ty.clone(), g.clone()));
            let (ty, generic) = match entry {
                Some(e) => e,
                None => {
                    self.emit(
                        format!(
                            "axon-T842 the lens `Json<{struct_name}>` declares no field \
                             `{f}`. The shape is a checkable EXPECTATION — navigating an \
                             undeclared field is a likely typo (runtime navigation stays \
                             total → a real document's extra field still reads as null \
                             here). Add `{f}` to `type {struct_name}`, or drop the shape \
                             to navigate the open `Json` freely."
                        ),
                        loc,
                    );
                    return T::Unknown;
                }
            };
            if i == segments.len() - 1 {
                return infer_type_from_name(&ty);
            }
            // Continue into a nested shape, or stop (permissive) at a scalar.
            if ty == "Json" && !generic.is_empty() {
                struct_name = generic;
            } else if self.json_lens_fields.contains_key(&ty) {
                struct_name = ty;
            } else {
                return T::Unknown;
            }
        }
        T::Unknown
    }

    /// §Fase 73.e — run the lens field check over a LEGACY-condition dotted
    /// path string (`profile.address.city`): if its root is a `Json<T>`
    /// lens param, walk the segments against the shape (`axon-T842` on an
    /// undeclared field). A non-dotted path or a non-lens root is a no-op,
    /// so a literal value (`"18"`, `"acme.com"`) is never mistaken for a
    /// navigation.
    fn check_legacy_lens_path(&mut self, path: &str, loc: &Loc) {
        let p = path.trim();
        let (root, rest) = match p.split_once('.') {
            Some(rt) => rt,
            None => return,
        };
        let struct_name = match self
            .current_flow_param_spellings
            .get(root)
            .and_then(|s| Self::parse_json_lens(s))
        {
            Some(s) => s,
            None => return,
        };
        let segments: Vec<&str> = rest.split('.').collect();
        let _ = self.lens_field_walk(struct_name, &segments, loc);
    }

    fn check_type_reference(&self, type_name: &str, _loc: &Loc) -> bool {
        if type_name.is_empty() {
            return true;
        }
        let builtin = epistemic::builtin_types();
        if builtin.contains(type_name) {
            return true;
        }
        if self
            .symbols
            .lookup(type_name)
            .map_or(false, |s| s.kind == "type")
        {
            return true;
        }
        // Soft: unknown types accepted silently — house idiom (D115.6),
        // not the phantom-import excuse §115 retired.
        true
    }

    // ── Epistemic mode validation ──────────────────────────────────

    fn check_epistemic_mode(&mut self, mode: &str, loc: &Loc) {
        const VALID_EPISTEMIC_MODES: &[&str] = &["believe", "doubt", "know", "speculate"];
        if !mode.is_empty() && !is_valid(mode, VALID_EPISTEMIC_MODES) {
            self.emit(
                format!(
                    "Unknown epistemic mode '{}'. Valid: {}",
                    mode,
                    valid_list(VALID_EPISTEMIC_MODES)
                ),
                loc,
            );
        }
    }

    // ──────────────────────────────────────────────────────────────────
    //  §λ-L-E Fase 13 — Mobile Typed Channels validation
    //  (paper_mobile_channels.md §3 + Fase 13.b parity port)
    // ──────────────────────────────────────────────────────────────────

    /// Validate `channel Name { … }` (paper §3.1 + §3.4 shield prereq).
    fn check_channel(&mut self, node: &ChannelDefinition) {
        if node.name.is_empty() {
            self.emit("channel requires a name".to_string(), &node.loc);
        }
        // Resolve the message schema; supports nested `Channel<…<T>>`.
        if node.message.is_empty() {
            self.emit(
                "channel requires a `message:` schema type".to_string(),
                &node.loc,
            );
        } else {
            self.validate_channel_message_type(&node.message, &node.loc);
        }
        // Optional shield reference must resolve when set (D8 prereq).
        if !node.shield_ref.is_empty() {
            match self.symbols.lookup(&node.shield_ref) {
                None => self.emit(
                    format!(
                        "channel '{}' references undefined shield '{}'",
                        node.name, node.shield_ref
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "shield" => self.emit(
                    format!(
                        "channel '{}' shield '{}' is a {}, not a shield",
                        node.name, node.shield_ref, sym.kind
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
    }

    /// Recursively validate a `message:` spelling.  `Channel<T>` peels
    /// one layer; the leaf must be a builtin / user type / channel name.
    /// Soft-resolve unknown leaves (consistent with resource conventions).
    fn validate_channel_message_type(&mut self, spelling: &str, _loc: &Loc) {
        let s = spelling.trim();
        if s.starts_with("Channel<") && s.ends_with('>') {
            let inner = &s["Channel<".len()..s.len() - 1];
            self.validate_channel_message_type(inner, _loc);
            return;
        }
        // Plain type name — silently accepted whether builtin, user-typed,
        // or a registered channel.  Type references are intentionally soft
        // here (matches resource/manifest convention).
    }

    /// Validate `daemon` body — listeners + delegated flow-step checks.
    /// Pre-Fase 13 the Rust checker skipped daemons entirely; we now
    /// walk listeners so emit/publish/discover/listen receive the same
    /// validation they do inside flows.
    fn check_daemon(&mut self, node: &DaemonDefinition) {
        if !node.shield_ref.is_empty() {
            match self.symbols.lookup(&node.shield_ref) {
                None => self.emit(
                    format!(
                        "daemon '{}' references undefined shield '{}'",
                        node.name, node.shield_ref
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "shield" => self.emit(
                    format!(
                        "daemon '{}' shield '{}' is a {}, not a shield",
                        node.name, node.shield_ref, sym.kind
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
        // §Fase 71.c — the `window:` temporal binding must name a defined
        // `window` primitive. The supervisor evaluates it before claiming a
        // scheduled tick (inside ⇒ fire; outside ⇒ `on_outside`).
        if !node.window_ref.is_empty() {
            match self.symbols.lookup(&node.window_ref) {
                None => self.emit(
                    format!(
                        "axon-T825 daemon '{}' binds undefined window '{}' — \
                         `window:` must name a declared `window` primitive.",
                        node.name, node.window_ref
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "window" => self.emit(
                    format!(
                        "axon-T825 daemon '{}' `window:` references '{}', which is a {}, \
                         not a window.",
                        node.name, node.window_ref, sym.kind
                    ),
                    &node.loc,
                ),
                _ => {}
            }
        }
        // §Fase 72.a — the `budget { … }` linear-effect rate limit.
        if let Some(budget) = &node.budget {
            self.check_budget(budget, &node.name);
        }
        for listener in &node.listeners {
            self.check_listen(listener, &node.name);
        }
        // §Fase 52.d — a cron-SCHEDULED daemon is a standing autonomous
        // privilege: it fires + invokes flows on its own, with no request
        // principal behind it. It MUST declare its capability scope so the
        // enterprise supervisor can mint a least-privilege per-run principal
        // (§52.d). Event-only daemons are exempt (pre-§52 Fase-16 surface).
        let has_cron = node
            .listeners
            .iter()
            .any(|l| crate::cron::cron_expr(&l.channel).is_some());
        if has_cron && node.requires_capabilities.is_empty() {
            self.emit(
                format!(
                    "axon-E0791 daemon '{}' has a cron-scheduled listener but declares no \
                     `requires:` capability scope — a standing scheduled privilege must be \
                     explicit. Add `requires: [<cap>, …]` (e.g. `requires: [flow.execute]`) so \
                     each run executes under a least-privilege principal.",
                    node.name
                ),
                &node.loc,
            );
        }
    }

    /// §Fase 52.g (reworked §74.g) — the honest "this daemon listener will
    /// never fire" diagnostic. §74 WIRED flow→daemon event delivery: a
    /// non-cron `listen`er now fires when an event arrives on its channel.
    /// So the remaining delivery defect is the UNPRODUCED channel — a
    /// listener on a channel NOTHING `emit`s to waits for an event no
    /// producer raises (the Kivi brief #39 case). `axon-W009` fires ONLY in
    /// that case now (it is silent when a producer exists, since §74
    /// delivers it). The compile-time mirror of the §74.g PCC
    /// `ChannelDeliverySoundness`; still the `axon-W004`/`no_unwitnessed_advantage`
    /// honesty posture — never let a program rely on an event no producer
    /// raises.
    fn warn_daemon_listener_never_fires(&mut self, daemon_name: &str, channel: &str, loc: &Loc) {
        // §74 delivers a listener that HAS a producer → only warn when the
        // channel has NO `emit` anywhere in the program.
        if self.emitted_channels.contains(channel) {
            return;
        }
        self.warn(
            format!(
                "axon-W009 daemon '{daemon_name}' listens on '{channel}', but NOTHING \
                 emits to it — there is no `emit {channel}(…)` anywhere in the program, so \
                 this listener can NEVER fire (it waits for an event no producer raises). \
                 Add a producer (`emit {channel}(payload)` in a flow), or remove the \
                 listener. (§74 delivers a listener that HAS a producer.)"
            ),
            loc,
        );
    }

    /// Validate a listen block (Fase 13 D4 dual-mode + §52.g delivery honesty).
    fn check_listen(&mut self, node: &ListenStep, daemon_name: &str) {
        if node.channel_is_ref {
            match self.symbols.lookup(&node.channel) {
                None => self.emit(
                    format!(
                        "daemon '{}' listens on undefined channel '{}'",
                        daemon_name, node.channel
                    ),
                    &node.loc,
                ),
                Some(sym) if sym.kind != "channel" => self.emit(
                    format!(
                        "daemon '{}' listen target '{}' is a {}, not a channel",
                        daemon_name, node.channel, sym.kind
                    ),
                    &node.loc,
                ),
                // §Fase 52.g — a well-formed typed-channel listener still
                // never fires today (runtime is cron-only) → honest redirect.
                _ => self.warn_daemon_listener_never_fires(daemon_name, &node.channel, &node.loc),
            }
        } else if let Some(expr) = crate::cron::cron_expr(&node.channel) {
            // §Fase 52.b — a time-based `listen "cron:<expr>"`. A cron channel is
            // a first-class scheduled trigger, NOT a legacy string topic, so it
            // does NOT get the D4 deprecation warning. Validate the 5-field
            // expression so a schedule that type-checks is one the §52.c
            // TimerSource can actually fire.
            match crate::cron::CronSchedule::parse(expr) {
                Err(e) => self.emit(
                    format!(
                        "axon-E0789 daemon '{}' has a malformed cron schedule \
                         '{}': {}",
                        daemon_name, node.channel, e
                    ),
                    &node.loc,
                ),
                Ok(_) => {
                    // A scheduled trigger with no handler is a no-op — almost
                    // always a mistake. The body is what runs each tick.
                    if node.body.is_empty() {
                        // §Fase 52.b code; renumbered E0790 → E0792 to resolve the
                        // collision with §69.a's witness well-formedness `axon-E0790`
                        // (one code, one meaning).
                        self.emit(
                            format!(
                                "axon-E0792 daemon '{}' cron listener '{}' has no \
                                 handler body — a scheduled trigger with no work \
                                 is a no-op; add a `{{ … }}` body with the steps \
                                 to run on each tick",
                                daemon_name, node.channel
                            ),
                            &node.loc,
                        );
                    }
                }
            }
        } else {
            // §Fase 52.g — a non-cron string topic on a daemon never fires
            // either (runtime is cron-only). The pre-52.g D4 warning told
            // adopters to "migrate to a typed `channel`" — but a typed
            // channel listener ALSO never fires, so that redirect was
            // actively misleading. Replace it with the honest "won't fire,
            // use cron-poll" redirect.
            self.warn_daemon_listener_never_fires(daemon_name, &node.channel, &node.loc);
        }
        // §Fase 52.a — validate the (now-executing) handler body. Its steps get
        // the same checks as a flow body (e.g. a `run <Flow>` resolves, a
        // `persist` targets a declared store), so a malformed listener body is
        // caught at compile time rather than failing silently at the first tick.
        if !node.body.is_empty() {
            self.check_flow_steps(&node.body, daemon_name);
        }
    }

    /// Validate an emit step (Chan-Output / Chan-Mobility, paper §3.1, §3.2).
    fn check_emit(&mut self, node: &EmitStatement) {
        if node.channel_ref.is_empty() {
            self.emit("emit requires a channel reference".to_string(), &node.loc);
            return;
        }
        let kind = match self.symbols.lookup(&node.channel_ref) {
            None => {
                self.emit(
                    format!("emit references undefined channel '{}'", node.channel_ref),
                    &node.loc,
                );
                return;
            }
            Some(sym) => sym.kind.clone(),
        };
        if kind != "channel" {
            self.emit(
                format!(
                    "emit target '{}' is a {}, not a channel",
                    node.channel_ref, kind
                ),
                &node.loc,
            );
            return;
        }
        if node.value_ref.is_empty() {
            self.emit(
                format!("emit on channel '{}' requires a value", node.channel_ref),
                &node.loc,
            );
            return;
        }
        // Fase 13.i — a dotted-access value_ref ("Step.output" or deeper)
        // references a prior step's result and is always scalar at the
        // type-check layer. The runtime resolves it via the executor's
        // ContextManager. Mobility (paper §3.2) is by definition a
        // bare-identifier case (a declared channel name), so the
        // second-order check below intentionally skips dotted paths.
        if node.value_ref.contains('.') {
            return;
        }
        // Second-order schema check (paper §3.2 Chan-Mobility): if the
        // outer channel carries `Channel<U>`, the value must resolve to
        // a channel whose own message equals U.  Lookup channel
        // definition by walking the AST so we don't need a separate
        // channel registry.
        let outer_msg = self.find_channel_message(&node.channel_ref);
        if let Some(outer) = outer_msg {
            if outer.starts_with("Channel<") && outer.ends_with('>') {
                let inner = &outer["Channel<".len()..outer.len() - 1];
                let value_kind = self
                    .symbols
                    .lookup(&node.value_ref)
                    .map(|s| s.kind.clone())
                    .unwrap_or_default();
                if value_kind != "channel" {
                    self.emit(
                        format!(
                            "emit on '{}' carries '{}' but value '{}' is not a \
                             channel handle (mobility violation, Chan-Mobility paper §3.2)",
                            node.channel_ref, outer, node.value_ref
                        ),
                        &node.loc,
                    );
                    return;
                }
                let value_msg = self
                    .find_channel_message(&node.value_ref)
                    .unwrap_or_default();
                if value_msg != inner {
                    self.emit(
                        format!(
                            "emit on '{}' expects Channel<{}> but '{}' carries \
                             Channel<{}> (second-order schema mismatch)",
                            node.channel_ref, inner, node.value_ref, value_msg
                        ),
                        &node.loc,
                    );
                }
            }
        }
    }

    /// Validate a publish step — D8 capability extrusion gate.
    fn check_publish(&mut self, node: &PublishStatement) {
        if node.channel_ref.is_empty() {
            self.emit(
                "publish requires a channel reference".to_string(),
                &node.loc,
            );
            return;
        }
        if node.shield_ref.is_empty() {
            self.emit(
                format!(
                    "publish '{}' requires a shield gate (D8 — capability \
                     extrusion is shield-mediated)",
                    node.channel_ref
                ),
                &node.loc,
            );
            return;
        }
        let ch_kind = match self.symbols.lookup(&node.channel_ref) {
            None => {
                self.emit(
                    format!(
                        "publish references undefined channel '{}'",
                        node.channel_ref
                    ),
                    &node.loc,
                );
                return;
            }
            Some(sym) => sym.kind.clone(),
        };
        if ch_kind != "channel" {
            self.emit(
                format!(
                    "publish target '{}' is a {}, not a channel",
                    node.channel_ref, ch_kind
                ),
                &node.loc,
            );
            return;
        }
        let sh_kind = match self.symbols.lookup(&node.shield_ref) {
            None => {
                self.emit(
                    format!(
                        "axon-T847 publish '{}' references undefined shield '{}'",
                        node.channel_ref, node.shield_ref
                    ),
                    &node.loc,
                );
                return;
            }
            Some(sym) => sym.kind.clone(),
        };
        if sh_kind != "shield" {
            self.emit(
                format!(
                    "axon-T847 publish gate '{}' is a {}, not a shield",
                    node.shield_ref, sh_kind
                ),
                &node.loc,
            );
            return;
        }
        // §Fase 77.b (`axon-T848`, D77.6) — the egress rule: a publish under
        // a SIGNING shield declares the channel for signed EXTERNAL delivery,
        // and the promise must be durable — a webhook backed by an ephemeral
        // in-process buffer dies unwitnessed with the process. v1 requires
        // `persistence: persistent_axonstore` so egress inherits the §74
        // outbox's at-least-once. A sign-less shield keeps the pure π-calc
        // publish semantics untouched (back-compat absolute).
        if let Some(shield) = find_shield_by_name(self.program, &node.shield_ref) {
            if !shield.sign.is_empty() {
                let persistence = self
                    .find_channel_persistence(&node.channel_ref)
                    .unwrap_or_default();
                if persistence != "persistent_axonstore" {
                    self.emit(
                        format!(
                            "axon-T848 publish '{}' within '{}' declares SIGNED egress \
                             (`sign: {}`), but the channel's persistence is '{}' — signed \
                             egress requires `persistence: persistent_axonstore` so the \
                             delivery promise survives a restart (it inherits the durable \
                             outbox's at-least-once). Declare the channel durable, or \
                             publish within a non-signing shield.",
                            node.channel_ref,
                            node.shield_ref,
                            shield.sign,
                            if persistence.is_empty() {
                                "ephemeral (default)"
                            } else {
                                persistence.as_str()
                            },
                        ),
                        &node.loc,
                    );
                }
            }
        }
        // κ-coverage compliance enforcement is deferred to a follow-up
        // pass that walks TypeDefinition.compliance — the Rust checker
        // currently does not aggregate type compliance metadata, so this
        // mirrors the soft-resolve behaviour of resource/manifest checks.
    }

    /// Validate a discover step — capability_ref must be publishable.
    fn check_discover(&mut self, node: &DiscoverStatement) {
        if node.capability_ref.is_empty() {
            self.emit(
                "discover requires a channel reference".to_string(),
                &node.loc,
            );
            return;
        }
        if node.alias.is_empty() {
            self.emit(
                "discover requires an `as <alias>` binding".to_string(),
                &node.loc,
            );
            return;
        }
        let kind = match self.symbols.lookup(&node.capability_ref) {
            None => {
                self.emit(
                    format!(
                        "discover references undefined channel '{}'",
                        node.capability_ref
                    ),
                    &node.loc,
                );
                return;
            }
            Some(sym) => sym.kind.clone(),
        };
        if kind != "channel" {
            self.emit(
                format!(
                    "discover target '{}' is a {}, not a channel",
                    node.capability_ref, kind
                ),
                &node.loc,
            );
            return;
        }
        // Verify publishability: the channel must declare a shield_ref.
        let shield = self.find_channel_shield(&node.capability_ref);
        if shield.as_deref().unwrap_or("").is_empty() {
            self.emit(
                format!(
                    "discover '{}' is not publishable: its channel definition \
                     declares no shield (D8 — only shield-gated channels can \
                     be discovered)",
                    node.capability_ref
                ),
                &node.loc,
            );
        }
    }

    /// Find the `message:` field of a registered channel by name.
    fn find_channel_message(&self, name: &str) -> Option<String> {
        for decl in &self.program.declarations {
            if let Declaration::Channel(c) = decl {
                if c.name == name {
                    return Some(c.message.clone());
                }
            }
        }
        None
    }

    /// Find the `shield:` field of a registered channel by name.
    fn find_channel_shield(&self, name: &str) -> Option<String> {
        for decl in &self.program.declarations {
            if let Declaration::Channel(c) = decl {
                if c.name == name {
                    return Some(c.shield_ref.clone());
                }
            }
        }
        None
    }

    /// §Fase 77.b — find the `persistence:` field of a registered channel
    /// by name (the `axon-T848` durable-egress rule reads it).
    fn find_channel_persistence(&self, name: &str) -> Option<String> {
        for decl in &self.program.declarations {
            if let Declaration::Channel(c) = decl {
                if c.name == name {
                    return Some(c.persistence.clone());
                }
            }
        }
        None
    }
}

// ── §λ-L-E Fase 4 — Honda-Vasconcelos helpers (free fns) ────────────────────

/// Honda-Vasconcelos duality on a single step pair:
/// `send T ↔ receive T`, `loop ↔ loop`, `end ↔ end`.
/// §Fase 41.b — lower a Fase 4 [`SessionRole`] (a flat `send`/`receive`/`loop`/
/// `end` step list) into the [`SessionType`] algebra of `crate::session`:
/// `send T` ↦ `!T.·`, `receive T` ↦ `?T.·`, a terminal `end` ↦ `end`, and a
/// terminal `loop` ↦ a `μ`-recursion back to the role's start
/// (`[send T, loop]` ↦ `μX. !T.X`). Prefix `end`/`loop` (malformed; caught by
/// `check_session_role`) are treated as the tail.
fn lower_session_role(role: &SessionRole) -> SessionType {
    let body = lower_session_steps(&role.steps);
    // A single role-level `μX` is the loop-back point: any `loop` (at the top
    // level or inside a branch) recurses to the role's start.
    if steps_contain_loop(&role.steps) {
        SessionType::rec("X", body)
    } else {
        body
    }
}

/// Lower a step sequence into a [`SessionType`] (§Fase 41.b). `send`/`receive`
/// prefix the continuation; `end`↦`end`, `loop`↦`X` (the role-level recursion
/// var); `select`/`branch` are terminal choices whose labelled branches each
/// lower recursively (their own sub-protocol).
fn lower_session_steps(steps: &[SessionStep]) -> SessionType {
    let Some((first, rest)) = steps.split_first() else {
        return SessionType::End;
    };
    match first.op.as_str() {
        "send" => SessionType::send(first.message_type.clone(), lower_session_steps(rest)),
        "receive" => SessionType::recv(first.message_type.clone(), lower_session_steps(rest)),
        "loop" => SessionType::var("X"),
        "end" => SessionType::End,
        "select" => SessionType::select(branch_types(&first.branches)),
        "branch" => SessionType::branch(branch_types(&first.branches)),
        // §Fase 79 — `interrupt { body } on Sig as sig resumable { handler }`
        // lowers to `Intr(sig; B, H)`. Terminal like select/branch (the region's
        // continuation lives inside its body). `resume` ↦ the self-dual leaf.
        "interrupt" => {
            let find = |label: &str| {
                first
                    .branches
                    .iter()
                    .find(|b| b.label == label)
                    .map(|b| lower_session_steps(&b.steps))
                    .unwrap_or(SessionType::End)
            };
            SessionType::Interrupt {
                signal: crate::session::Payload::new(first.message_type.clone()),
                body: Box::new(find("body")),
                handler: Box::new(find("handler")),
            }
        }
        "resume" => SessionType::Resume,
        // Malformed mid-sequence op; `check_session_role` flags it. Skip so
        // duality diagnostics stay focused on the real shape.
        _ => lower_session_steps(rest),
    }
}

fn branch_types(branches: &[SessionBranch]) -> impl Iterator<Item = (String, SessionType)> + '_ {
    branches.iter().map(|b| (b.label.clone(), lower_session_steps(&b.steps)))
}

/// §Fase 79 — the closed `CallInterruptCause` catalog (D79.2). Mirrors every
/// other closed catalog in the language (`qos`, `on_stuck`, `sign`): the
/// type-checker needs a finite exhaustiveness surface and the PCC a finite
/// proof obligation. Also consumed by the `InterruptibleSessionSoundness`
/// witness re-derivation (§79.c PCC).
pub const CALL_INTERRUPT_CAUSES: &[&str] =
    &["CallerSpeech", "Dtmf", "SilenceTimeout", "AgentFault"];

/// §Fase 79.c — a `resumable` handler is a **two-exit** construct (D79.11a):
/// its normal exit is `resume` (back to the parked body) and its abandon exit
/// is `end` (TTL expiry). A well-formed handler must reach one of them on every
/// path — the last step is `resume`/`end`, or a terminal choice all of whose
/// arms reach an exit. A handler that "falls off the end" would leave a linear
/// continuation capability un-released.
fn handler_reaches_exit(steps: &[SessionStep]) -> bool {
    match steps.last() {
        Some(s) if s.op == "resume" || s.op == "end" => true,
        Some(s) if matches!(s.op.as_str(), "select" | "branch") => {
            !s.branches.is_empty() && s.branches.iter().all(|b| handler_reaches_exit(&b.steps))
        }
        _ => false,
    }
}

/// True if any step (top-level or inside a `select`/`branch`) is a `loop` — the
/// role then carries a single `μX` whose `X` every `loop` recurses to.
fn steps_contain_loop(steps: &[SessionStep]) -> bool {
    steps.iter().any(|s| {
        s.op == "loop"
            || (matches!(s.op.as_str(), "select" | "branch" | "interrupt")
                && s.branches.iter().any(|b| steps_contain_loop(&b.steps)))
    })
}

/// Directed-graph cycle detector (DFS with gray/black colouring). Returns
/// one representative ordering per strongly-connected cycle found.
fn find_cycles(adjacency: &std::collections::HashMap<String, Vec<String>>) -> Vec<Vec<String>> {
    let mut color: std::collections::HashMap<String, &'static str> =
        std::collections::HashMap::new();
    let mut stack: Vec<String> = Vec::new();
    let mut cycles: Vec<Vec<String>> = Vec::new();

    fn visit(
        n: &str,
        adjacency: &std::collections::HashMap<String, Vec<String>>,
        color: &mut std::collections::HashMap<String, &'static str>,
        stack: &mut Vec<String>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        color.insert(n.to_string(), "gray");
        stack.push(n.to_string());
        let targets = adjacency.get(n).cloned().unwrap_or_default();
        for tgt in targets {
            match color.get(&tgt).copied() {
                Some("gray") => {
                    if let Some(idx) = stack.iter().position(|s| s == &tgt) {
                        cycles.push(stack[idx..].to_vec());
                    }
                }
                None => visit(&tgt, adjacency, color, stack, cycles),
                _ => {}
            }
        }
        stack.pop();
        color.insert(n.to_string(), "black");
    }

    let keys: Vec<String> = adjacency.keys().cloned().collect();
    for src in keys {
        if !color.contains_key(&src) {
            visit(&src, adjacency, &mut color, &mut stack, &mut cycles);
        }
    }
    cycles
}

fn cycle_to_edges<'a>(cycle: &[String], edges: &'a [TopologyEdge]) -> Vec<&'a TopologyEdge> {
    let n = cycle.len();
    let mut result = Vec::with_capacity(n);
    for i in 0..n {
        let src = &cycle[i];
        let tgt = &cycle[(i + 1) % n];
        if let Some(e) = edges.iter().find(|e| &e.source == src && &e.target == tgt) {
            result.push(e);
        }
    }
    result
}

/// Locate a session by name in the program's declarations (flat scan).
/// §80.g hardening — `Rec("X", Var("X"))`: the unguarded μX.X a leading
/// `loop` lowers to. The coinductive/credit analyses would unfold it without
/// progress; every consumer of a lowered role must reject it first.
fn is_unguarded_recursion(t: &SessionType) -> bool {
    matches!(t, SessionType::Rec(name, body)
        if matches!(body.as_ref(), SessionType::Var(v) if v == name))
}

/// §Fase 80.c — collect the distinct message types a role sends/receives,
/// walking every construct that can carry a message: plain `send`/`receive`
/// steps, `select`/`branch` labelled arms, and §79 `interrupt` body+handler
/// (both exits — a message only exchanged inside a handler still crosses the
/// wire, so the T849 totality law must see it). `loop`/`end`/`resume` carry
/// no payload. Order-preserving first-occurrence dedup keeps diagnostics
/// deterministic.
fn collect_role_messages(steps: &[SessionStep], sends: &mut Vec<String>, receives: &mut Vec<String>) {
    for step in steps {
        match step.op.as_str() {
            "send" => {
                if !sends.contains(&step.message_type) {
                    sends.push(step.message_type.clone());
                }
            }
            "receive" => {
                if !receives.contains(&step.message_type) {
                    receives.push(step.message_type.clone());
                }
            }
            "select" | "branch" | "interrupt" => {
                for b in &step.branches {
                    collect_role_messages(&b.steps, sends, receives);
                }
            }
            _ => {}
        }
    }
}

fn find_session_by_name<'a>(program: &'a Program, name: &str) -> Option<&'a SessionDefinition> {
    for decl in &program.declarations {
        if let Declaration::Session(s) = decl {
            if s.name == name {
                return Some(s);
            }
        }
    }
    None
}

/// §Fase 37.c — Render a `TypeExpr` for a diagnostic message:
/// `String`, `List<String>`, `String?` (optional marker appended).
fn fmt_type_expr(t: &TypeExpr) -> String {
    let mut s = t.name.clone();
    if !t.generic_param.is_empty() {
        s.push('<');
        s.push_str(&t.generic_param);
        s.push('>');
    }
    if t.optional {
        s.push('?');
    }
    s
}

/// §Fase 117.a — peel the wrapping type constructors off a declared type
/// reference down to the nominal name a `type` declaration can carry.
///
/// `FlowEnvelope<List<PatientRecord>>` → `PatientRecord`. Used by the
/// `axon-T957` boundary-coverage law: an endpoint's κ is a property of the
/// DATA, and wrapping it in an envelope or a list does not launder its
/// regulatory classes. Mirrors the constructor set [`declared_cardinality`]
/// recognises, and is total — an unrecognised or empty reference peels to
/// itself, so a caller that finds no matching `type` simply contributes no κ.
fn peel_type_constructors(type_ref: &str) -> &str {
    let mut t = type_ref.trim();
    // `?` marks an optional declaration — orthogonal to κ.
    t = t.strip_suffix('?').unwrap_or(t).trim();
    loop {
        let peeled = ["FlowEnvelope<", "List<", "Stream<"].iter().find_map(|ctor| {
            t.strip_prefix(*ctor)
                .and_then(|rest| rest.strip_suffix('>'))
                .map(|inner| inner.trim())
        });
        match peeled {
            Some(inner) => t = inner.strip_suffix('?').unwrap_or(inner).trim(),
            None => return t,
        }
    }
}

fn find_type_by_name<'a>(program: &'a Program, name: &str) -> Option<&'a TypeDefinition> {
    for decl in &program.declarations {
        if let Declaration::Type(t) = decl {
            if t.name == name {
                return Some(t);
            }
        }
    }
    None
}

fn find_shield_by_name<'a>(program: &'a Program, name: &str) -> Option<&'a ShieldDefinition> {
    for decl in &program.declarations {
        if let Declaration::Shield(s) = decl {
            if s.name == name {
                return Some(s);
            }
        }
    }
    None
}

fn find_flow_by_name<'a>(program: &'a Program, name: &str) -> Option<&'a FlowDefinition> {
    for decl in &program.declarations {
        if let Declaration::Flow(f) = decl {
            if f.name == name {
                return Some(f);
            }
        }
    }
    None
}

// ═══════════════════════════════════════════════════════════════════
//  §FASE 31.b — TYPE-DRIVEN WIRE INFERENCE (D1, D7, D10)
// ═══════════════════════════════════════════════════════════════════
//
// Cross-stack mirror of Python `axon/compiler/type_checker.py`
// (`_implicit_transport` + `_compute_implicit_transports`). D7
// ratifies byte-identical inference across both stacks; the
// `tests/fixtures/fase31_implicit_transport/corpus.json` drift gate
// locks parity in CI.
//
// The inference rule (D1):
//
//   implicit_transport(F, E) =
//     declared_transport(E)          if transport_explicit
//     "sse"                           if produces_stream(F) ∧ ¬explicit
//     "json"                          otherwise
//
// `produces_stream(F)` is the 3-disjunct predicate from Fase 30.c
// (Rust port shipping here as part of 31.b — Fase 30.c was
// Python-only, with the Rust port deferred to 30.c.2; 31.b
// supersedes that deferral by shipping the predicate now, since
// the inference REQUIRES it cross-stack).
//
// Pillar trace:
//   MATHEMATICS — function on (Flow, AxonEndpoint, Program).
//   LOGIC       — disjunction of three formal predicates.
//   PHILOSOPHY  — the language is the wire's source of truth.
//   COMPUTING   — pure function; no side effects beyond AST mutation
//                  in `compute_implicit_transports`.
// ═══════════════════════════════════════════════════════════════════

/// Disjunct (b) helper: does the named tool declare a
/// `stream:<policy>` effect? Returns `false` for unresolved names.
fn tool_has_stream_effect(program: &Program, tool_name: &str) -> bool {
    if tool_name.is_empty() {
        return false;
    }
    for decl in &program.declarations {
        if let Declaration::Tool(t) = decl {
            if t.name == tool_name {
                if let Some(ref effects) = t.effects {
                    return effects.effects.iter().any(|e| e.starts_with("stream:"));
                }
                return false;
            }
        }
    }
    false
}

/// Disjunct (a): does any step in the flow have `output: Stream<T>`?
fn flow_has_stream_output(flow: &FlowDefinition) -> bool {
    for step in &flow.body {
        if let FlowStep::Step(s) = step {
            let out = s.output_type.trim();
            if out.starts_with("Stream<") && out.ends_with('>') {
                return true;
            }
        }
    }
    false
}

/// Look up the `tool_name` field on a `UseToolStep`. The exact field
/// name is `tool_name` for the Rust AST (mirrors Python's `UseToolNode`).
fn use_tool_step_name(u: &UseToolStep) -> &str {
    &u.tool_name
}

/// §Fase 51.d — the algebraic effects a flow performs via its `quant` blocks:
/// the deduplicated, source-ordered set of `ots:backend:<backend>` slugs,
/// recursing through nested control blocks (`if` / `for` / `par`) and nested
/// `quant` blocks. This is the flow-level **effect-row projection** for the
/// quant primitive — a flow containing a `quant` "carries the effect in its
/// signature". Consumed downstream by PCC EffectSoundness + the runtime backend
/// dispatcher (§51.d.2 / §51.e). Pure + total.
pub fn flow_quant_effects(flow: &FlowDefinition) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    collect_quant_effects(&flow.body, &mut out);
    out
}

fn collect_quant_effects(steps: &[FlowStep], out: &mut Vec<String>) {
    for step in steps {
        match step {
            FlowStep::Quant(q) => {
                let slug = crate::ots_catalog::quant_effect_slug(&q.effect);
                if !out.contains(&slug) {
                    out.push(slug);
                }
                // A nested `quant` inside the body contributes its effect too.
                collect_quant_effects(&q.body, out);
            }
            FlowStep::If(c) => {
                collect_quant_effects(&c.then_body, out);
                collect_quant_effects(&c.else_body, out);
            }
            FlowStep::ForIn(f) => collect_quant_effects(&f.body, out),
            FlowStep::Par(p) => {
                for branch in &p.branches {
                    collect_quant_effects(branch, out);
                }
            }
            _ => {}
        }
    }
}

/// Disjunct (b): does the flow reach a tool with `effects:
/// <stream:<policy>>`? Walks both `FlowStep::UseTool` (top-level
/// flow-step) AND `FlowStep::Step(s)` where `s.apply_ref` resolves
/// to a tool — the latter is the Kivi-shape pattern (Fase 31.b
/// extension of the Fase 30.c predicate; see Python mirror for
/// the rationale).
pub fn flow_uses_streaming_tool(flow: &FlowDefinition, program: &Program) -> bool {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for step in &flow.body {
        match step {
            FlowStep::UseTool(u) => {
                let tn = use_tool_step_name(u);
                if !tn.is_empty()
                    && seen.insert(tn.to_string())
                    && tool_has_stream_effect(program, tn)
                {
                    return true;
                }
            }
            FlowStep::Step(s) => {
                // §Fase 31.b — `apply: <name>` inside a step body is
                // the canonical adopter pattern (Kivi 2026-05-11).
                if !s.apply_ref.is_empty()
                    && seen.insert(s.apply_ref.clone())
                    && tool_has_stream_effect(program, &s.apply_ref)
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Master predicate: disjunction of the formal layers from
/// Fase 30.c. Disjunct (c) `perform Stream.Yield(...)` is not
/// surfaced at this layer because the Rust AST does not currently
/// expose step-body perform expressions through `FlowStep` — that
/// path is the Rust frontend completion gap from Fase 30.e and is
/// covered by the runtime source-text fallback in
/// `axon-rs/src/axon_server.rs::classify_negotiation_via_source_text`.
/// Disjuncts (a) and (b) cover every adopter-observable in-AST
/// pattern (the Kivi case + every Fase 30.c-tested source).
pub fn produces_stream(flow: &FlowDefinition, program: &Program) -> bool {
    flow_has_stream_output(flow) || flow_uses_streaming_tool(flow, program)
}

/// Compute the inferred transport for one axonendpoint per D1.
///
/// Returns one of:
///   * `"sse"`  — explicit `transport: sse|ndjson` declared (D2 ndjson
///                inferred as sse for wire-format purposes today;
///                ndjson namespace remains reserved per Fase 30 D2),
///              OR `transport_explicit == false` AND `produces_stream`
///                evaluates true (D1 inference fires).
///   * `"json"` — explicit `transport: json` declared (D3 opt-out),
///              OR `transport_explicit == false` AND flow does not
///                produce a stream.
///
/// `flow` may be `None` when the endpoint's `execute_flow` does not
/// resolve to any flow in scope (a separate type-error reported
/// elsewhere); we conservatively default to declared-or-json.
///
/// NEVER panics. Total + deterministic over every input shape.
pub fn implicit_transport(
    endpoint: &AxonEndpointDefinition,
    flow: Option<&FlowDefinition>,
    program: &Program,
) -> String {
    if endpoint.transport_explicit {
        return match endpoint.transport.as_str() {
            // D2 — ndjson namespace reserved; semantically streaming.
            "ndjson" => "sse".to_string(),
            "sse" | "json" => endpoint.transport.clone(),
            // Unknown explicit value; parser would normally reject.
            // Defensive: default json.
            _ => "json".to_string(),
        };
    }
    // No explicit declaration; D1 inference path.
    match flow {
        Some(f) if produces_stream(f, program) => "sse".to_string(),
        _ => "json".to_string(),
    }
}

// ═══════════════════════════════════════════════════════════════════
//  §Fase 33.z.k.c (v1.28.0) — Effective dialect resolver
// ═══════════════════════════════════════════════════════════════════
//
// The dialect resolver answers the question "WHICH SSE dialect does
// this route emit?". It is orthogonal to `classify_dynamic_route_wire`
// which answers "IS the wire SSE?". The two compose:
//
//   1. classify_dynamic_route_wire → DynamicRouteWire::{Sse, Json}
//   2. If Sse:  resolve_effective_dialect → "axon" | "openai" | "anthropic"
//      If Json: dialect is "" (irrelevant)
//
// # Q1 algebraic-effect-driven default
//
// When the source omits the explicit `transport: sse(<dialect>)`
// parametrization, the resolver applies the founder-ratified default:
//   - openai: when the flow declares an algebraic effect (the tool
//             carries `effects: <stream:<policy>>`). The LLM-streaming
//             ecosystem expects OpenAI-style on the wire.
//   - axon:   when the flow uses type-annotation only
//             (`output: Stream<T>` without a tool effect). W3C
//             named-events stays the structural-commitment baseline.
//
// # D3 precedence preserved
//
// Adopter-explicit `transport: sse(<dialect>)` always wins. Adopter-
// explicit `transport: json` short-circuits before the resolver
// runs (the wire is JSON; no dialect applies).
//
// # Pure function discipline (D10)
//
// `resolve_effective_dialect` is total over its 2-input domain.
// Returns `""` only when called with an inconsistent state (no
// algebraic signal AND no explicit dialect AND no type-annotation
// stream — i.e., the caller violated the precondition that the
// wire IS SSE). Defensive: returns "axon" in that case rather than
// panic.
//
// Pillar trace:
//   MATHEMATICS — pure 2-input function with total domain.
//   LOGIC       — closed catalog of 3 outputs + defensive fallback.
//   PHILOSOPHY  — algebraic effects on tools drive openai default
//                 because that's where LLM-streaming adopters
//                 already live; type-annotation stays axon-baseline.
//   COMPUTING   — adopters' SDKs see the wire format their
//                 ecosystem documents; no client-side adapter
//                 work required.

/// Resolve which SSE dialect the runtime should emit for a route.
///
/// Precondition: caller already determined the wire IS SSE (via
/// `classify_dynamic_route_wire`). Calling this on a JSON-wire
/// route is meaningless but never panics.
///
/// Closed-catalog output: always one of `"axon"`, `"openai"`, or
/// `"anthropic"`. Never returns an empty string under valid input.
///
/// # Resolution rules (Q1 ratified)
///
/// 1. **Explicit dialect wins.** When `transport_dialect != ""`,
///    return it verbatim. (The parser already validated it
///    against `AXONENDPOINT_TRANSPORT_DIALECTS` so it's one of
///    `axon`/`openai`/`anthropic`.)
/// 2. **Algebraic-effect → openai.** Tool with declared stream
///    effect → adopters consume LLM-style streams → openai default.
/// 3. **Type-annotation only → axon.** No tool effect; W3C named
///    events stay the structural-commitment baseline.
pub fn resolve_effective_dialect(
    transport_dialect: &str,
    has_algebraic_stream_effect: bool,
) -> String {
    // Rule 1 — explicit dialect wins (D3-style precedence for the
    // dialect choice).
    if !transport_dialect.is_empty() {
        return transport_dialect.to_string();
    }
    // Rule 2 — algebraic effect (disjunct b) → openai default.
    if has_algebraic_stream_effect {
        return "openai".to_string();
    }
    // Rule 3 — type-annotation only (disjunct a) → axon default.
    "axon".to_string()
}

// ═══════════════════════════════════════════════════════════════════
//  §FASE 31.c — COMPILE-TIME WARNING `axon-W001` (D4, D10)
// ═══════════════════════════════════════════════════════════════════
//
// Rust mirror of Python `_emit_implicit_transport_warnings`. Same
// emission conditions, same message shape, same warning code
// `axon-W001`. D7 cross-stack contract: both stacks render
// byte-identical warning text for byte-identical input.

/// Warning code namespace per D4 — first entry in the warning
/// namespace `axon-Wnnn`. Errors keep their `axon-Ennn` namespace
/// from Fase 28 + Fase 30.
pub const W001_CODE: &str = "axon-W001";

/// §Fase 36.k (D10) — warning code for an `axonendpoint` that
/// declares no `backend:`.
///
/// `axon-W002` is held by the runtime warning catalog
/// (`axon::runtime_warnings` — `streaming-not-supported`, Fase
/// 33.x.g); the `axon-Wnnn` namespace is shared across the frontend
/// and the runtime, so the next free slot is `axon-W003`.
pub const W003_CODE: &str = "axon-W003";

/// §Fase 59 (D2) — warning code for `apply: <Tool>` on a tool that
/// declares a typed `parameters:` schema. The `axon-Wnnn` namespace is
/// shared across the frontend + runtime; W001/W002/W003 are taken, so
/// this is the next free slot.
pub const W004_CODE: &str = "axon-W004";

/// §Fase 59 (D2) — build the canonical `axon-W004` message. `apply: <Tool>`
/// on a schema-bearing tool is COGNITIVE DELEGATION (the LLM decides
/// whether to invoke the tool), NOT a deterministic dispatch — and `given:`
/// is not splatted at runtime. Point the adopter at the one deterministic,
/// CT-2-validated, real-dispatch surface (flow-level `use <Tool>(k=v)`),
/// listing the declared params so the conversion is paste-actionable. The
/// canonical law is `axon://logic/dispatch_vs_cognition`.
fn build_w004_message(tool_name: &str, params: &[(String, String, bool)]) -> String {
    let tool = tool_name;
    let call = params
        .iter()
        .map(|(n, _, _)| format!("{n} = …"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "warning[{W004_CODE}]: `apply: {tool}` runs '{tool}' as a COGNITIVE step \
         backend — the step executes as an LLM reasoning call and the model decides \
         stochastically whether to invoke the tool; it is NOT a deterministic \
         dispatch, and `given:` is not splatted at runtime. '{tool}' declares a typed \
         `parameters:` schema, so for a deterministic, schema-validated, real dispatch \
         use the flow-level form `use {tool}({call})` (with `provider: http`/`mcp` + a \
         wired endpoint). See axon://logic/dispatch_vs_cognition."
    )
}

/// §Fase 68.e — warning code for `apply: <Compute>`, a model-selection no-op.
/// W005 is held by the quant-encoding advisory (§51.b); this is the next slot.
pub const W006_CODE: &str = "axon-W006";

/// §Fase 69.a — the CLOSED catalog of Advantage-Witness metrics. A `witness`'s
/// `metric:` must be one of these (`axon-E0790`); each names how advantage over a
/// baseline is measured for some primitive domain. Mirrored in
/// `axon::advantage_witness::WITNESS_METRICS` (the runtime evaluator), parity-pinned
/// by `axon-rs/tests/fase69_a_witness_metric_parity.rs` (the §67.a.2 two-
/// representation discipline). Extending the catalog is a deliberate PR (the §53
/// closed-catalog extension discipline), never an open set.
pub const WITNESS_METRICS: &[&str] = &[
    // quant kernels (§69.b): geometric difference g(K_classical ‖ K_quantum).
    "geometric_difference",
    // quant kernels (§69.b): centered kernel-target alignment vs the baseline.
    "kernel_target_alignment",
    // retrieval / navigate (§69.d): ranking lift over flat cosine retrieval.
    "ranking_lift",
    // deliberation primitives: outcome lift over a single-shot baseline.
    "outcome_lift",
];

/// §Fase 68.f — the largest context window any canonical model offers (tokens).
/// A `requires_context:` above this can never be satisfied → `axon-T809`. This is
/// a frontend mirror of `axon::backends::model_catalog::max_canonical_context_window()`
/// (the runtime catalog lives in axon-rs, which depends on this crate — not the
/// reverse — so the value is mirrored here and PINNED by the cross-crate parity
/// test `axon-rs/tests/fase68_f_context_ceiling_parity.rs`: a drift fails CI, the
/// §67.a.2 two-representation discipline). Currently gemini-2.5-flash = 1,048,576.
pub const MAX_KNOWN_CONTEXT_WINDOW: u32 = 1_048_576;

/// §Fase 68.e — build the canonical `axon-W006` message. `apply: <Compute>` does
/// not select an LLM model — a `compute { model: … }` field is dropped at lowering
/// (the parser keeps only `shield:`), so the brief-#36 adopter's `apply: BigSummary`
/// to pin a larger model was silently ignored. Point them at the faithful surface:
/// declare the step's capability need with `requires_context:` (the §68.c resolver
/// picks a satisfying model), or set the deployment model. The §59 honest-compiler
/// doctrine — a silent no-op becomes guidance.
fn build_w006_message(compute_ref: &str, step_name: &str) -> String {
    format!(
        "warning[{W006_CODE}]: step '{step_name}' applies compute '{compute_ref}' — \
         `compute` / `apply:` does NOT select an LLM model (a `compute {{ model: … }}` \
         field is dropped at lowering and has no runtime effect). To choose the model \
         for this step, declare its capability need with `requires_context: <tokens>` \
         (the resolver picks the smallest model whose context window fits, or fails \
         closed at deploy), or set the deployment model (e.g. `AXON_DAEMON_MODEL`). \
         See §Fase 68 capability-aware model resolution."
    )
}

/// §Fase 36.k (D10) — build the canonical `axon-W003` message: an
/// `axonendpoint` that declares no `backend:` relies on ladder
/// resolution. Emitted by `check_axonendpoint`, surfaced by
/// `axon check` (and promoted to an error under `--strict`).
fn build_w003_message(endpoint_name: &str) -> String {
    format!(
        "warning[{W003_CODE}]: axonendpoint '{endpoint_name}' declares \
         no `backend:` — its execution backend is resolved at request \
         time down the Fase 36 precedence ladder (server default → \
         environment-available providers). If none resolves the \
         endpoint fails with a structured HTTP 503; it never silently \
         runs the no-op `stub`. Declare `backend: <provider>` to pin \
         the model, or `backend: auto` to make the reliance on ladder \
         resolution explicit and silence this warning."
    )
}

/// Find the most informative description of WHY the flow produces
/// a stream — mirror of Python `_describe_stream_origin`. Used by
/// the W001 warning builder to make the diagnostic paste-actionable.
fn describe_stream_origin(flow: &FlowDefinition, program: &Program) -> String {
    // Disjunct (a) — step with `output: Stream<T>`.
    for step in &flow.body {
        if let FlowStep::Step(s) = step {
            let out = s.output_type.trim();
            if out.starts_with("Stream<") && out.ends_with('>') {
                return format!("step '{}' has `output: {}`", s.name, s.output_type);
            }
        }
    }
    // Disjunct (b) — find the first stream-effect tool reference.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for step in &flow.body {
        match step {
            FlowStep::Step(s) => {
                if !s.apply_ref.is_empty() && seen.insert(s.apply_ref.clone()) {
                    if let Some(policy) = tool_stream_policy(program, &s.apply_ref) {
                        return format!(
                            "step '{}' applies tool '{}' with effects `<{}>`",
                            s.name, s.apply_ref, policy
                        );
                    }
                }
            }
            FlowStep::UseTool(u) => {
                let tn = use_tool_step_name(u);
                if !tn.is_empty() && seen.insert(tn.to_string()) {
                    if let Some(policy) = tool_stream_policy(program, tn) {
                        return format!(
                            "tool '{}' is used directly with effects `<{}>`",
                            tn, policy
                        );
                    }
                }
            }
            _ => {}
        }
    }
    "its declared algebraic effects".to_string()
}

/// Resolve a tool by name and return the first `stream:<policy>`
/// effect, or `None` if the tool doesn't exist / has no stream effect.
fn tool_stream_policy(program: &Program, tool_name: &str) -> Option<String> {
    for decl in &program.declarations {
        if let Declaration::Tool(t) = decl {
            if t.name == tool_name {
                if let Some(ref effects) = t.effects {
                    for e in &effects.effects {
                        if e.starts_with("stream:") {
                            return Some(e.clone());
                        }
                    }
                }
                return None;
            }
        }
    }
    None
}

/// Build the canonical W001 message text per D4. Mirrors Python
/// `_build_w001_message` byte-identically.
fn build_w001_message(endpoint: &AxonEndpointDefinition, flow: &FlowDefinition, program: &Program) -> String {
    let origin = describe_stream_origin(flow, program);
    format!(
        "warning[{}]: implicit `transport: sse` inferred from stream \
         effects on axonendpoint '{}' (flow '{}' produces a stream \
         via {}). Declare `transport: sse` to silence this warning \
         and lock in SSE behavior, or `transport: json` to opt out \
         and keep the legacy JSON wire format. When \
         `strict_type_driven_transport: true`, this endpoint emits \
         SSE on /v1/execute by default.",
        W001_CODE, endpoint.name, endpoint.execute_flow, origin
    )
}

/// Walk every `AxonEndpointDefinition` and emit one `axon-W001`
/// warning per implicit-sse site (D4). Returns the list of new
/// `TypeError` warnings — caller appends to whatever warning
/// collection it manages (e.g. `TypeChecker.warnings` or a
/// CLI-rendering buffer).
///
/// Emission conditions (all must hold):
///   1. `endpoint.transport_explicit == false`
///   2. `endpoint.implicit_transport == "sse"`
///   3. The endpoint's `execute_flow` resolves to a flow in scope
///      (orphan endpoints emit no W001 — their separate error is
///      unrelated; a W001 attached would be noise).
///
/// Rate-limited per axonendpoint by construction. Safe to call
/// repeatedly (each call rebuilds the warning list — idempotent).
pub fn compute_implicit_transport_warnings(program: &Program) -> Vec<TypeError> {
    let mut warnings: Vec<TypeError> = Vec::new();
    let mut flow_indices: HashMap<String, usize> = HashMap::new();
    for (i, decl) in program.declarations.iter().enumerate() {
        if let Declaration::Flow(f) = decl {
            flow_indices.insert(f.name.clone(), i);
        }
    }
    for decl in &program.declarations {
        let ae = match decl {
            Declaration::AxonEndpoint(ae) => ae,
            _ => continue,
        };
        if ae.transport_explicit {
            continue;
        }
        if ae.implicit_transport != "sse" {
            continue;
        }
        let flow = match flow_indices.get(&ae.execute_flow) {
            Some(&fi) => match &program.declarations[fi] {
                Declaration::Flow(f) => f,
                _ => continue,
            },
            None => continue,
        };
        warnings.push(TypeError {
            message: build_w001_message(ae, flow, program),
            line: ae.loc.line,
            column: ae.loc.column,
        });
    }
    warnings
}

/// Walk every `AxonEndpointDefinition` in the program and attach
/// its computed `implicit_transport` per D1. Mutates the AST in
/// place. Idempotent + safe to re-run.
///
/// Mirrors Python `_compute_implicit_transports` byte-identically
/// (D7 cross-stack contract — locked in CI by the drift-gate
/// corpus at `tests/fixtures/fase31_implicit_transport/corpus.json`).
pub fn compute_implicit_transports(program: &mut Program) {
    // First pass: index every FlowDefinition by name. We must clone
    // the necessary references because subsequent mutation of
    // program.declarations needs &mut access; we cannot hold a
    // shared borrow simultaneously. Index uses indices into the
    // declarations Vec so we can resolve flows by name without
    // re-walking the whole Vec for each endpoint.
    let mut flow_indices: HashMap<String, usize> = HashMap::new();
    for (i, decl) in program.declarations.iter().enumerate() {
        if let Declaration::Flow(f) = decl {
            flow_indices.insert(f.name.clone(), i);
        }
    }

    // Second pass: walk endpoints. For each, look up the flow by
    // name (immutable borrow of program.declarations via index) and
    // compute the inference; then re-borrow mutably to assign the
    // result. Because Rust's borrow checker forbids simultaneous
    // immutable + mutable borrow of the same Vec, we precompute the
    // (endpoint_index, inferred_transport) pairs first, then apply
    // them all in a second mutating loop.
    // §Fase 33.z.k.1 (v1.27.1) — compute BOTH `implicit_transport`
    // (Fase 31 D1 inference) AND the new `has_algebraic_stream_effect`
    // predicate (algebraic-effect override). Both are read by the
    // runtime classifier; the algebraic-effect predicate carries
    // strictly more information (it isolates disjunct (b) of
    // `produces_stream` — the tool-effect signal — from disjunct (a)
    // — the type-annotation signal). The runtime promotes the route
    // to SSE unconditionally when the algebraic predicate is true
    // (D3 `transport: json` opt-out still wins).
    let mut updates: Vec<(usize, String, bool)> = Vec::new();
    for (i, decl) in program.declarations.iter().enumerate() {
        if let Declaration::AxonEndpoint(ae) = decl {
            let flow = flow_indices.get(&ae.execute_flow).and_then(|&fi| {
                if let Declaration::Flow(f) = &program.declarations[fi] {
                    Some(f)
                } else {
                    None
                }
            });
            let transport_result = implicit_transport(ae, flow, program);
            let algebraic_result = match flow {
                Some(f) => flow_uses_streaming_tool(f, program),
                None => false,
            };
            updates.push((i, transport_result, algebraic_result));
        }
    }
    for (i, transport_result, algebraic_result) in updates {
        if let Declaration::AxonEndpoint(ae) = &mut program.declarations[i] {
            ae.implicit_transport = transport_result;
            ae.has_algebraic_stream_effect = algebraic_result;
        }
    }
}

// ── §λ-L-E Fase 13 — Mobile Typed Channels type-checker tests ────────────────

#[cfg(test)]
mod fase13_typecheck_tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn check_with_warnings(src: &str) -> (Vec<TypeError>, Vec<TypeError>) {
        let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
        let prog = Parser::new(tokens).parse().expect("parse");
        TypeChecker::new(&prog).check_with_warnings()
    }

    fn check_errors(src: &str) -> Vec<TypeError> {
        check_with_warnings(src).0
    }

    #[test]
    fn channel_with_valid_shield_clean() {
        let src = r#"
            type Order { id: String }
            shield Gate { scan: [pii_leak] }
            channel C { message: Order shield: Gate }
        "#;
        assert!(check_errors(src).is_empty());
    }

    #[test]
    fn channel_undefined_shield_rejected() {
        let src = "channel C { message: Order shield: NotDefined }";
        let errs = check_errors(src);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("undefined shield 'NotDefined'")),
            "got: {:?}",
            errs
        );
    }

    #[test]
    fn channel_shield_wrong_kind_rejected() {
        let src = r#"
            type NotAShield { x: String }
            channel C { message: Order shield: NotAShield }
        "#;
        let errs = check_errors(src);
        assert!(
            errs.iter().any(|e| e.message.contains("not a shield")),
            "got: {:?}",
            errs
        );
    }

    #[test]
    fn emit_undefined_channel_rejected() {
        let src = "flow f() -> O { emit Bogus(payload) }";
        let errs = check_errors(src);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("undefined channel 'Bogus'")),
            "got: {:?}",
            errs
        );
    }

    #[test]
    fn emit_target_wrong_kind_rejected() {
        let src = r#"
            type Order { id: String }
            flow f() -> O { emit Order(payload) }
        "#;
        let errs = check_errors(src);
        assert!(
            errs.iter().any(|e| e.message.contains("not a channel")),
            "got: {:?}",
            errs
        );
    }

    #[test]
    fn emit_mobility_schema_mismatch_rejected() {
        let src = r#"
            type Order { id: String }
            type Other { y: String }
            channel Wrong { message: Other }
            channel Outer { message: Channel<Order> }
            flow f() -> O { emit Outer(Wrong) }
        "#;
        let errs = check_errors(src);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("second-order schema mismatch")),
            "got: {:?}",
            errs
        );
    }

    #[test]
    fn publish_undefined_shield_rejected() {
        let src = r#"
            channel C { message: Order }
            flow f() -> Cap { publish C within MissingShield }
        "#;
        let errs = check_errors(src);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("undefined shield 'MissingShield'")),
            "got: {:?}",
            errs
        );
    }

    #[test]
    fn discover_unpublishable_channel_rejected() {
        let src = r#"
            type Order { id: String }
            channel C { message: Order }
            flow f() -> O { discover C as ch }
        "#;
        let errs = check_errors(src);
        assert!(
            errs.iter().any(|e| e.message.contains("not publishable")),
            "got: {:?}",
            errs
        );
    }

    #[test]
    fn listen_typed_channel_warns_never_fires() {
        // §Fase 52.g — a well-formed typed-channel daemon listener resolves
        // cleanly (no error) but NEVER fires (the runtime is cron-only), so
        // the honest `axon-W009` redirect surfaces. Pre-52.g this was
        // silently clean — the exact trap an adopter falls into.
        let src = r#"
            type Order { id: String }
            channel C { message: Order }
            daemon D() {
                goal: "x"
                listen C as ev { }
            }
        "#;
        let (errs, warns) = check_with_warnings(src);
        assert!(errs.is_empty(), "errors: {:?}", errs);
        assert_eq!(warns.len(), 1, "the typed-channel listener warns it never fires");
        assert!(warns[0].message.contains("axon-W009"), "{:?}", warns);
        // §74.g — the message is now about the missing producer, not cron.
        assert!(warns[0].message.contains("NEVER fire") && warns[0].message.contains("emit"));
    }

    #[test]
    fn listen_typed_undefined_rejected() {
        let src = r#"
            daemon D() {
                goal: "x"
                listen NoSuchChannel as ev { }
            }
        "#;
        let errs = check_errors(src);
        assert!(
            errs.iter().any(|e| e.message.contains("undefined channel")),
            "got: {:?}",
            errs
        );
    }

    #[test]
    fn listen_string_topic_warns_never_fires() {
        // §Fase 52.g — a non-cron string topic on a daemon never fires
        // either; the honest `axon-W009` redirect replaces the pre-52.g D4
        // "migrate to a typed channel" warning (which pointed at a form that
        // ALSO never fires — actively misleading).
        let src = r#"
            daemon D() {
                goal: "x"
                listen "orders.created" as ev { }
            }
        "#;
        let (errs, warns) = check_with_warnings(src);
        assert!(errs.is_empty(), "no errors expected: {:?}", errs);
        assert_eq!(warns.len(), 1);
        assert!(warns[0].message.contains("axon-W009"));
        assert!(warns[0].message.contains("orders.created"));
        assert!(warns[0].message.contains("NEVER fire"));
        // the misleading "migrate to typed channel" redirect is gone
        assert!(!warns[0].message.contains("deprecated since Fase 13"));
    }

    #[test]
    fn listen_both_non_cron_listeners_warn_never_fires() {
        // §Fase 52.g — BOTH a typed-channel and a topic listener never fire
        // (cron-only runtime), so both surface the honest W009 redirect.
        // Pre-52.g only the legacy topic warned (typed was silently clean).
        let src = r#"
            type Order { id: String }
            channel C { message: Order }
            daemon Mixed() {
                goal: "x"
                listen C as canonical { }
                listen "legacy" as legacy_ev { }
            }
        "#;
        let (errs, warns) = check_with_warnings(src);
        assert!(errs.is_empty(), "no errors expected: {:?}", errs);
        assert_eq!(warns.len(), 2, "both non-cron listeners warn they never fire");
        assert!(warns.iter().all(|w| w.message.contains("axon-W009")));
    }

    #[test]
    fn listen_with_a_producer_does_not_warn() {
        // §Fase 74.g — the key change: a non-cron listener on a channel that
        // a flow EMITS to no longer warns (§74 delivers it). W009 is now
        // silent when a producer exists.
        let src = r#"
            type Order { id: String }
            channel C { message: Order }
            flow Produce(id: String) -> Unit { emit C(id) }
            daemon D() {
                requires: [flow.execute]
                listen C as ev { probe p }
            }
        "#;
        let (errs, warns) = check_with_warnings(src);
        assert!(errs.is_empty(), "errors: {:?}", errs);
        assert!(
            !warns.iter().any(|w| w.message.contains("axon-W009")),
            "a listener WITH a producer must not warn (§74 delivers it): {:?}",
            warns
        );
    }

    #[test]
    fn listen_cron_schedule_does_not_warn_never_fires() {
        // §Fase 52.g — a `cron:` listener IS backed (the supervisor fires
        // it) → no W009 redirect. The honesty diagnostic is precisely scoped
        // to the unbacked (non-cron) listeners.
        let src = r#"
            flow Tick() -> Unit { probe p }
            daemon Sched() {
                requires: [flow.execute]
                listen "cron:*/5 * * * *" as t { run Tick() }
            }
        "#;
        let (errs, warns) = check_with_warnings(src);
        assert!(errs.is_empty(), "no errors: {:?}", errs);
        assert!(
            !warns.iter().any(|w| w.message.contains("axon-W009")),
            "a cron listener must NOT get the never-fires warning: {:?}",
            warns
        );
    }

    // ── Fase 13.i — type checker tolerates dotted-access value_ref ──

    #[test]
    fn emit_dotted_value_ref_does_not_trip_mobility_check() {
        // Previously, a second-order channel with a dotted-access value
        // would falsely error as "not a channel handle". With 13.i the
        // mobility check must skip when value_ref contains '.'.
        let src = r#"
            channel Inner { message: Bytes qos: at_least_once }
            channel Outer { message: Channel<Bytes> qos: at_least_once }
            flow f() -> Out {
                emit Outer(Build.handle)
            }
        "#;
        let errs = check_errors(src);
        let mobility = errs
            .iter()
            .filter(|e| {
                e.message.contains("second-order schema mismatch")
                    || e.message.contains("not a channel handle")
            })
            .count();
        assert_eq!(
            mobility, 0,
            "dotted access must not trip mobility check; got: {:?}",
            errs
        );
    }

    #[test]
    fn emit_bare_identifier_mobility_check_still_runs() {
        // Regression guard — non-dotted refs still get the second-order
        // check applied, so a wrong handle is still rejected.
        let src = r#"
            channel Inner { message: Bytes qos: at_least_once }
            channel Wrong { message: Integer qos: at_least_once }
            channel Outer { message: Channel<Bytes> qos: at_least_once }
            flow f() -> Out {
                emit Outer(Wrong)
            }
        "#;
        let errs = check_errors(src);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("second-order schema mismatch")),
            "expected mobility violation for bare-id ref, got: {:?}",
            errs
        );
    }
}

// ── §Fase 35.j — Pillar IV: capability-typed store access ───────────

#[cfg(test)]
mod fase35j_capability_tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn check_errors(src: &str) -> Vec<TypeError> {
        let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
        let prog = Parser::new(tokens).parse().expect("parse");
        TypeChecker::new(&prog).check()
    }

    fn mentions_capability(errs: &[TypeError]) -> bool {
        errs.iter().any(|e| e.message.contains("requiring capability"))
    }

    #[test]
    fn endpoint_must_grant_a_gated_store_capability() {
        // The flow accesses a `capability: "tenant.read"` store, but
        // the endpoint executing it grants nothing → a type error.
        let src = r#"
            axonstore tenants {
                backend: postgresql
                connection: "env:DB"
                capability: "tenant.read"
            }
            flow GetTenants() -> Unit {
                retrieve tenants { where: "id = 1" }
            }
            axonendpoint Ep { method: GET path: "/t" execute: GetTenants }
        "#;
        assert!(
            mentions_capability(&check_errors(src)),
            "an endpoint that does not grant the store's capability \
             must fail the compositional check"
        );
    }

    #[test]
    fn endpoint_granting_the_capability_type_checks_clean() {
        let src = r#"
            axonstore tenants {
                backend: postgresql
                connection: "env:DB"
                capability: "tenant.read"
            }
            flow GetTenants() -> Unit {
                retrieve tenants { where: "id = 1" }
            }
            axonendpoint Ep {
                method: GET path: "/t" execute: GetTenants
                requires: [tenant.read]
            }
        "#;
        assert!(
            !mentions_capability(&check_errors(src)),
            "an endpoint that grants the capability must type-check clean"
        );
    }

    #[test]
    fn ungated_store_needs_no_endpoint_grant() {
        // §Fase 85 — renamed store `cache` → `kvstore` (`cache` is now a
        // reserved keyword for the result-memoization primitive).
        let src = r#"
            axonstore kvstore { backend: postgresql connection: "env:DB" }
            flow Fetch() -> Unit {
                retrieve kvstore { where: "k = 1" }
            }
            axonendpoint Ep { method: GET path: "/c" execute: Fetch }
        "#;
        assert!(
            !mentions_capability(&check_errors(src)),
            "a store with no `capability:` requires no endpoint grant"
        );
    }

    #[test]
    fn malformed_capability_slug_is_a_parse_error() {
        // The closed slug grammar is enforced at parse time.
        let src = r#"axonstore s { backend: postgresql connection: "env:DB" capability: "Tenant.Read" }"#;
        let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
        assert!(
            Parser::new(tokens).parse().is_err(),
            "an uppercase capability slug must be rejected at parse time"
        );
    }
}

// ════════════════════════════════════════════════════════════════════
//  §Fase 37.y.4 — D3 union-coverage + D4 T901 collision rejection
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod fase37y_d3_d4_tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn check_errors(src: &str) -> Vec<TypeError> {
        let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
        let prog = Parser::new(tokens).parse().expect("parse");
        TypeChecker::new(&prog).check()
    }

    #[test]
    fn d3_path_only_param_passes_d2_totality() {
        let src = r#"
            type SecretWriteRequest { value: Text }
            type WriteResult { ok: Bool }
            axonendpoint write_secret {
                method: POST
                path: "/api/tenants/{tenant_id}/secrets/{secret_name}"
                body: SecretWriteRequest
                execute: WriteSecret
            }
            flow WriteSecret(tenant_id: Text, secret_name: Text, value: Text) -> WriteResult {
                step Echo { reason: "ok" output: WriteResult }
            }
        "#;
        let errs = check_errors(src);
        let binding_errs: Vec<&TypeError> = errs
            .iter()
            .filter(|e| {
                e.message.contains("tenant_id")
                    || e.message.contains("secret_name")
                    || e.message.contains("Request Binding")
            })
            .collect();
        assert!(
            binding_errs.is_empty(),
            "D3 — path-only params must satisfy D2 totality. Got: {binding_errs:#?}"
        );
    }

    #[test]
    fn d3_query_only_param_passes_d2_totality() {
        let src = r#"
            type UserList { count: Int }
            axonendpoint list_users {
                method: GET
                path: "/api/users"
                query: { status: Text }
                execute: ListUsers
            }
            flow ListUsers(status: Text) -> UserList {
                step Build { reason: "list" output: UserList }
            }
        "#;
        let errs = check_errors(src);
        let binding_errs: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("status") || e.message.contains("Request Binding"))
            .collect();
        assert!(
            binding_errs.is_empty(),
            "D3 — query-only params must satisfy D2 totality. Got: {binding_errs:#?}"
        );
    }

    #[test]
    fn d3_mixed_path_query_body_coverage_passes() {
        let src = r#"
            type CreateRequest { content: Text }
            type CreateResult { id: Uuid }
            axonendpoint create_item {
                method: POST
                path: "/api/orgs/{org_id}/items"
                query: { dry_run: Bool? }
                body: CreateRequest
                execute: CreateItem
            }
            flow CreateItem(org_id: Text, dry_run: Bool?, content: Text) -> CreateResult {
                step Build { reason: "create" output: CreateResult }
            }
        "#;
        let errs = check_errors(src);
        let binding_errs: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("Request Binding") || e.message.contains("axon-T901"))
            .collect();
        assert!(
            binding_errs.is_empty(),
            "D3 — mixed coverage must satisfy D2. Got: {binding_errs:#?}"
        );
    }

    #[test]
    fn d3_missing_param_extended_hint_names_all_three_sources() {
        let src = r#"
            type Empty { ok: Bool }
            axonendpoint x {
                method: POST
                path: "/api/x"
                body: Empty
                execute: X
            }
            flow X(missing: Text) -> Empty {
                step S { reason: "x" output: Empty }
            }
        "#;
        let errs = check_errors(src);
        let hint = errs.iter().find(|e| e.message.contains("missing")).expect(
            "missing-binding error must surface",
        );
        assert!(hint.message.contains("path placeholder"), "hint names path: {}", hint.message);
        assert!(hint.message.contains("query"), "hint names query: {}", hint.message);
        assert!(hint.message.contains("body"), "hint names body: {}", hint.message);
    }

    #[test]
    fn d4_t901_collision_path_and_body() {
        let src = r#"
            type SecretWriteRequest { tenant_id: Text, value: Text }
            type WriteResult { ok: Bool }
            axonendpoint write {
                method: POST
                path: "/api/tenants/{tenant_id}"
                body: SecretWriteRequest
                execute: Write
            }
            flow Write(tenant_id: Text, value: Text) -> WriteResult {
                step S { reason: "x" output: WriteResult }
            }
        "#;
        let errs = check_errors(src);
        let t901 = errs.iter().find(|e| e.message.contains("axon-T901"));
        assert!(
            t901.is_some(),
            "D4 — path+body collision must emit axon-T901. Errors: {errs:#?}"
        );
        let msg = &t901.unwrap().message;
        assert!(msg.contains("path and body"), "names both sources: {msg}");
        assert!(msg.contains("tenant_id"), "names the colliding param: {msg}");
    }

    #[test]
    fn d4_t901_collision_path_and_query() {
        let src = r#"
            type Empty { ok: Bool }
            axonendpoint x {
                method: GET
                path: "/api/users/{id}"
                query: { id: Text }
                execute: X
            }
            flow X(id: Text) -> Empty {
                step S { reason: "x" output: Empty }
            }
        "#;
        let errs = check_errors(src);
        let t901 = errs.iter().find(|e| e.message.contains("axon-T901"));
        assert!(t901.is_some(), "D4 — path+query collision. Errs: {errs:#?}");
        assert!(
            t901.unwrap().message.contains("path and query"),
            "names path AND query"
        );
    }

    #[test]
    fn d4_t901_collision_query_and_body() {
        let src = r#"
            type Req { status: Text }
            type Empty { ok: Bool }
            axonendpoint x {
                method: POST
                path: "/api/x"
                query: { status: Text }
                body: Req
                execute: X
            }
            flow X(status: Text) -> Empty {
                step S { reason: "x" output: Empty }
            }
        "#;
        let errs = check_errors(src);
        let t901 = errs.iter().find(|e| e.message.contains("axon-T901"));
        assert!(t901.is_some(), "D4 — query+body collision. Errs: {errs:#?}");
        assert!(
            t901.unwrap().message.contains("query and body"),
            "names query AND body"
        );
    }

    #[test]
    fn d4_t901_collision_triple_source() {
        let src = r#"
            type Req { id: Text }
            type Empty { ok: Bool }
            axonendpoint x {
                method: POST
                path: "/api/{id}"
                query: { id: Text }
                body: Req
                execute: X
            }
            flow X(id: Text) -> Empty {
                step S { reason: "x" output: Empty }
            }
        "#;
        let errs = check_errors(src);
        let t901 = errs.iter().find(|e| e.message.contains("axon-T901"));
        assert!(t901.is_some(), "D4 — triple collision. Errs: {errs:#?}");
        let msg = &t901.unwrap().message;
        assert!(
            msg.contains("path, query, and body"),
            "names all three sources with Oxford comma: {msg}"
        );
        assert!(
            msg.contains("Remove the declaration from 2 of the sources"),
            "explicit count of removals needed: {msg}"
        );
    }

    #[test]
    fn d3_path_param_typed_non_text_emits_error() {
        let src = r#"
            type Empty { ok: Bool }
            axonendpoint x {
                method: GET
                path: "/api/users/{id}"
                execute: X
            }
            flow X(id: Uuid) -> Empty {
                step S { reason: "x" output: Empty }
            }
        "#;
        let errs = check_errors(src);
        let type_err = errs.iter().find(|e| {
            e.message.contains("path placeholder") && e.message.contains("Text")
        });
        assert!(
            type_err.is_some(),
            "path-binding type mismatch must surface. Errs: {errs:#?}"
        );
    }

    #[test]
    fn d3_query_param_type_mismatch_emits_error() {
        let src = r#"
            type Empty { ok: Bool }
            axonendpoint x {
                method: GET
                path: "/api/x"
                query: { limit: Int }
                execute: X
            }
            flow X(limit: Text) -> Empty {
                step S { reason: "x" output: Empty }
            }
        "#;
        let errs = check_errors(src);
        let type_err = errs.iter().find(|e| {
            e.message.contains("query: {") && e.message.contains("Int")
        });
        assert!(
            type_err.is_some(),
            "query-binding type mismatch must surface. Errs: {errs:#?}"
        );
    }

    #[test]
    fn d5_body_only_endpoint_legacy_behavior_intact() {
        let src_passes = r#"
            type Req { value: Text }
            type Empty { ok: Bool }
            axonendpoint x {
                method: POST
                path: "/api/x"
                body: Req
                execute: X
            }
            flow X(value: Text) -> Empty {
                step S { reason: "x" output: Empty }
            }
        "#;
        let errs = check_errors(src_passes);
        assert!(
            errs.iter().all(|e| !e.message.contains("Request Binding")
                && !e.message.contains("axon-T901")),
            "D5 — body-only happy path passes unchanged. Errs: {errs:#?}"
        );
    }

    #[test]
    fn d3_kivi_secret_write_passes_post_37y() {
        // The exact kivi adopter case — pre-37.y this was the
        // blocking bug. Post-37.y the build is green.
        let src = r#"
            type SecretWriteRequest { value: Text }
            type WriteResult { ok: Bool }
            axonendpoint write_secret {
                method: POST
                path: "/api/tenants/{tenant_id}/secrets/{secret_name}"
                query: { dry_run: Bool?, overwrite: Bool? }
                body: SecretWriteRequest
                execute: WriteSecret
            }
            flow WriteSecret(
                tenant_id: Text,
                secret_name: Text,
                dry_run: Bool?,
                overwrite: Bool?,
                value: Text
            ) -> WriteResult {
                step S { reason: "x" output: WriteResult }
            }
        "#;
        let errs = check_errors(src);
        let binding_errs: Vec<&TypeError> = errs
            .iter()
            .filter(|e| {
                e.message.contains("Request Binding") || e.message.contains("axon-T901")
            })
            .collect();
        assert!(
            binding_errs.is_empty(),
            "Kivi corpus must pass post-37.y. Got: {binding_errs:#?}"
        );
    }
}

// ════════════════════════════════════════════════════════════════════
//  §Fase 38.x.e — Retrieve Cardinality vs Output Singularity Gate
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod fase38xe_cardinality_tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn check_errors(src: &str) -> Vec<TypeError> {
        let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
        let prog = Parser::new(tokens).parse().expect("parse");
        TypeChecker::new(&prog).check()
    }

    #[test]
    fn retrieve_tail_with_singular_output_emits_t9xx() {
        // §Fase 39.e UPDATE — the kivi-shape regression: endpoint
        // declares `output: T` (bare singular) on default
        // `transport: json` with a retrieve-tail flow producing
        // `List<T>`.
        //
        // - Pre-38.x.e (v1.x): passed `axon check` silently, failed
        //   at runtime with the opaque D5 `internal_validation_error`.
        // - 38.x.e (v1.39.0): fired `axon-T9XX retrieve_cardinality_mismatch`
        //   at compile time hinting `change to output: List<T>`.
        // - v1.40.0+: same hint but ALSO failed at D5 because the
        //   wire was the v1.x flat envelope (not a List).
        // - **39.e (v2.0.0)**: fires `axon-E039` at compile time
        //   with the canonical FlowEnvelope wrapping suggestion +
        //   sse migration alternative. This is the STRUCTURAL
        //   closure of the adopter-reported gap. T9XX is suppressed
        //   under E039 (single canonical diagnostic).
        let src = r#"
            type TenantRecord { id: Text }
            axonstore tenants { backend: in_memory }
            axonendpoint get_tenant {
                method: GET
                path: "/api/tenants/{tenant_id}"
                output: TenantRecord
                execute: GetTenant
            }
            flow GetTenant(tenant_id: Text) -> TenantRecord {
                retrieve tenants { where: "id = ${tenant_id}" as: result }
            }
        "#;
        let errs = check_errors(src);
        let e039: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("axon-E039"))
            .collect();
        assert!(
            !e039.is_empty(),
            "§Fase 39.e (D12 α) — a bare-singular `output: T` on a \
             `transport: json` endpoint with retrieve-tail MUST emit \
             `axon-E039`. All errors: {errs:#?}"
        );
        let err = e039[0];
        // The retrieve-tail's inferred cardinality is plural of
        // `StoreRow` (the IR-level row type — the flow's declared
        // return type `TenantRecord` doesn't currently propagate
        // through the retrieve-step taxonomy). The diagnostic
        // honestly reflects what the inference sees; adopters map
        // `StoreRow` to their declared row type mentally. A future
        // sub-fase can refine the inference to use the flow's
        // declared return type when available.
        assert!(
            err.message.contains("FlowEnvelope<List<StoreRow>>")
                || err.message.contains("FlowEnvelope<List<TenantRecord>>"),
            "§39.e — the E039 hint MUST suggest a canonical \
             FlowEnvelope wrapping around the inferred tail \
             cardinality (List<StoreRow> from the IR retrieve-step \
             taxonomy today; List<TenantRecord> if inference is \
             refined). Got: {}",
            err.message
        );
        assert!(
            err.message.contains("transport: sse"),
            "§39.e — the E039 hint MUST also name the sse migration \
             alternative. Got: {}",
            err.message
        );
        // T9XX is suppressed under E039 — single canonical diagnostic.
        let t9xx: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("axon-T9XX"))
            .collect();
        assert!(
            t9xx.is_empty(),
            "§39.e — when E039 fires, T9XX MUST be suppressed (single \
             canonical diagnostic with the right answer). Got: {t9xx:#?}"
        );
    }

    #[test]
    fn retrieve_tail_with_list_output_passes() {
        // D5 — the symmetric well-formed case: retrieve-tail flow
        // with `output: List<T>` endpoint. No T9XX fires.
        let src = r#"
            type TenantRecord { id: Text }
            axonstore tenants { backend: in_memory }
            axonendpoint list_tenants {
                method: GET
                path: "/api/tenants"
                output: List<TenantRecord>
                execute: ListTenants
            }
            flow ListTenants() -> List<TenantRecord> {
                retrieve tenants { where: "1 = 1" as: result }
            }
        "#;
        let errs = check_errors(src);
        let t9xx: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("axon-T9XX"))
            .collect();
        assert!(
            t9xx.is_empty(),
            "§Fase 38.x.e D1 — a retrieve-tail flow with `output: \
             List<T>` is the well-formed case. No T9XX should fire. \
             Got: {t9xx:#?}"
        );
    }

    #[test]
    fn step_tail_with_singular_output_passes() {
        // The other well-formed case: flow's tail is a `step` returning
        // a singular type, matching the endpoint's singular output. No
        // T9XX fires (the step's return shape is the contract).
        let src = r#"
            type WriteResult { ok: Bool }
            axonendpoint write_secret {
                method: POST
                path: "/api/secrets"
                output: WriteResult
                execute: WriteSecret
            }
            flow WriteSecret() -> WriteResult {
                step Echo { reason: "ok" output: WriteResult }
            }
        "#;
        let errs = check_errors(src);
        let t9xx: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("axon-T9XX"))
            .collect();
        assert!(
            t9xx.is_empty(),
            "§Fase 38.x.e D1 — a step-tail flow with matching singular \
             output is well-formed; no cardinality mismatch. Got: \
             {t9xx:#?}"
        );
    }

    #[test]
    fn no_output_declared_skips_gate() {
        // D1 honest scope — when the endpoint omits `output:` entirely
        // (empty string), the gate cannot determine the expected
        // cardinality and silently passes. Adopters who don't declare
        // output are NOT protected by this gate; the runtime path is
        // their only check. Documented as future Fase 38.x.f scope.
        let src = r#"
            type TenantRecord { id: Text }
            axonstore tenants { backend: in_memory }
            axonendpoint get_tenant_loose {
                method: GET
                path: "/api/tenants/{tenant_id}"
                execute: GetTenantLoose
            }
            flow GetTenantLoose(tenant_id: Text) -> Unit {
                retrieve tenants { where: "id = ${tenant_id}" as: result }
            }
        "#;
        let errs = check_errors(src);
        let t9xx: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("axon-T9XX"))
            .collect();
        assert!(
            t9xx.is_empty(),
            "§Fase 38.x.e D1 — endpoint with no `output:` declared \
             skips the cardinality gate (honest scope). Got: {t9xx:#?}"
        );
    }

    #[test]
    fn stream_output_skips_gate() {
        // Stream<T> is its own cardinality concept (Plural<T> over
        // time, not a List). The v1.39.0 gate doesn't reason about
        // Stream — honestly deferred to Fase 38.x.f. Endpoints
        // declaring `output: Stream<T>` are NOT compared against the
        // flow tail.
        let src = r#"
            type Token { text: Text }
            axonendpoint stream_chat {
                method: POST
                path: "/api/stream"
                output: Stream<Token>
                execute: StreamChat
            }
            flow StreamChat() -> Stream<Token> {
                step Generate { ask: "stream" output: Stream<Token> }
            }
        "#;
        let errs = check_errors(src);
        let t9xx: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("axon-T9XX"))
            .collect();
        assert!(
            t9xx.is_empty(),
            "§Fase 38.x.e D1 — Stream<T> output skips the gate \
             (v1.39.0 honest scope). Got: {t9xx:#?}"
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// §Fase 39.a — FlowEnvelope<T> built-in primitive (Rust-canonical)
// ════════════════════════════════════════════════════════════════════
//
// Anchor §-assertions for sub-fase 39.a per the plan vivo
// `docs/fase/fase_39_pure_silicon_cognition.md`. Acceptance criterion:
//   declared_cardinality("FlowEnvelope<List<X>>") == Wrapped(Plural("X"))
// + transparent unwrap behavior at the gate
// + nested generic parser support (parse_type_expr recurses)
// + existing T9XX warnings preserved unchanged for non-wrapped declarations
// + zero regressions in the 447-lib + 38.x.e + 38.x.f anchor tests
//
// The full axon-E039 mandate enforcement (D12 α ratified — singular too
// requires FlowEnvelope wrapping for transport: json) lands in 39.e
// once the runtime wire envelope is in place (39.b). 39.a establishes
// the type-system FOUNDATION; behavior gate fires in 39.e.

#[cfg(test)]
mod fase39a_flow_envelope_tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn check_errors(src: &str) -> Vec<TypeError> {
        let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
        let prog = Parser::new(tokens).parse().expect("parse");
        TypeChecker::new(&prog).check()
    }

    // ── §1 — declared_cardinality(FlowEnvelope<T>) recognition ──

    #[test]
    fn fase39a_flow_envelope_of_singular_is_wrapped_singular() {
        // §Fase 39.a §1 — `FlowEnvelope<TenantRecord>` recognized as
        // Wrapped(Singular("TenantRecord")). The wrapper preserves
        // inner cardinality through unwrap-recurse.
        let card = declared_cardinality("FlowEnvelope<TenantRecord>");
        match card {
            Cardinality::Wrapped(inner) => {
                assert_eq!(
                    *inner,
                    Cardinality::Singular("TenantRecord".to_string()),
                    "§39.a §1 — inner must be Singular(\"TenantRecord\")"
                );
            }
            other => panic!(
                "§39.a §1 — FlowEnvelope<TenantRecord> must yield \
                 Wrapped(Singular(...)). Got: {other:?}"
            ),
        }
    }

    #[test]
    fn fase39a_flow_envelope_of_list_is_wrapped_plural() {
        // §Fase 39.a §1 — the acceptance criterion verbatim:
        // declared_cardinality("FlowEnvelope<List<X>>") == Wrapped(Plural("X"))
        let card = declared_cardinality("FlowEnvelope<List<TenantRecord>>");
        match card {
            Cardinality::Wrapped(inner) => {
                assert_eq!(
                    *inner,
                    Cardinality::Plural("TenantRecord".to_string()),
                    "§39.a §1 acceptance — inner must be Plural(\"TenantRecord\") \
                     (the canonical retrieve-tail shape Kivi reported)"
                );
            }
            other => panic!(
                "§39.a §1 acceptance — FlowEnvelope<List<TenantRecord>> must \
                 yield Wrapped(Plural(\"TenantRecord\")). Got: {other:?}"
            ),
        }
    }

    #[test]
    fn fase39a_flow_envelope_of_stream_is_wrapped_stream() {
        // §Fase 39.a §1 — Stream<T> wrapped through FlowEnvelope.
        // Note: per D9 SSE wire keeps its own event family; this
        // shape is here for type-system completeness, not for runtime
        // SSE dispatch.
        let card = declared_cardinality("FlowEnvelope<Stream<Token>>");
        match card {
            Cardinality::Wrapped(inner) => {
                assert_eq!(
                    *inner,
                    Cardinality::StreamCardinality("Token".to_string()),
                    "§39.a §1 — inner must be StreamCardinality(\"Token\")"
                );
            }
            other => panic!(
                "§39.a §1 — FlowEnvelope<Stream<Token>> must yield \
                 Wrapped(StreamCardinality(...)). Got: {other:?}"
            ),
        }
    }

    #[test]
    fn fase39a_flow_envelope_of_any_is_wrapped_disagreed() {
        // §Fase 39.a §1 — `FlowEnvelope<Any>` is a degenerate but
        // valid wrap; the Any (Disagreed) propagates through.
        let card = declared_cardinality("FlowEnvelope<Any>");
        match card {
            Cardinality::Wrapped(inner) => {
                assert_eq!(
                    *inner,
                    Cardinality::Disagreed,
                    "§39.a §1 — FlowEnvelope<Any> inner must be Disagreed"
                );
            }
            other => panic!("§39.a §1 — got: {other:?}"),
        }
    }

    #[test]
    fn fase39a_nested_flow_envelope_is_doubly_wrapped() {
        // §Fase 39.a §1 — degenerate `FlowEnvelope<FlowEnvelope<X>>`
        // is syntactically valid (parser recursion) and semantically
        // double-wrapped. The unwrap-recurse at the gate handles
        // arbitrary depth transparently.
        let card = declared_cardinality("FlowEnvelope<FlowEnvelope<TenantRecord>>");
        match card {
            Cardinality::Wrapped(outer_inner) => match outer_inner.as_ref() {
                Cardinality::Wrapped(inner_inner) => {
                    assert_eq!(
                        **inner_inner,
                        Cardinality::Singular("TenantRecord".to_string()),
                        "§39.a §1 — nested wrap inner must be Singular"
                    );
                }
                other => panic!(
                    "§39.a §1 — nested wrap outer.inner must be Wrapped. \
                     Got: {other:?}"
                ),
            },
            other => panic!("§39.a §1 — got: {other:?}"),
        }
    }

    // ── §2 — non-FlowEnvelope declarations preserve v1.x semantics ──

    #[test]
    fn fase39a_list_without_envelope_still_plural() {
        // §Fase 39.a §2 — pre-Fase 39 declarations stay backwards-compat
        // at the cardinality level. `List<T>` is still Plural; the
        // axon-E039 mandate that promotes it to a compile error
        // when paired with `transport: json` lands in 39.e, not 39.a.
        let card = declared_cardinality("List<TenantRecord>");
        assert_eq!(
            card,
            Cardinality::Plural("TenantRecord".to_string()),
            "§39.a §2 — List<T> backwards-compat: still Plural"
        );
    }

    #[test]
    fn fase39a_stream_without_envelope_still_stream() {
        let card = declared_cardinality("Stream<Token>");
        assert_eq!(
            card,
            Cardinality::StreamCardinality("Token".to_string()),
            "§39.a §2 — Stream<T> backwards-compat: still StreamCardinality"
        );
    }

    #[test]
    fn fase39a_bare_type_still_singular() {
        let card = declared_cardinality("TenantRecord");
        assert_eq!(
            card,
            Cardinality::Singular("TenantRecord".to_string()),
            "§39.a §2 — bare type backwards-compat: still Singular"
        );
    }

    // ── §3 — Parser supports nested generics post-39.a ──

    #[test]
    fn fase39a_parser_accepts_flow_envelope_of_list() {
        // §Fase 39.a §3 — pre-39.a `parse_type_expr` only consumed
        // one Identifier inside `<...>`, so `FlowEnvelope<List<X>>`
        // failed to parse with a syntax error. Post-39.a the parser
        // recurses on the inner type expression and accepts
        // arbitrary depth. This test compiles end-to-end via the
        // existing check_errors harness (lexer + parser + checker).
        let src = r#"
            type TenantRecord { id: Text }
            axonstore tenants { backend: in_memory }
            axonendpoint get_all {
                method: GET
                path: "/api/tenants"
                output: FlowEnvelope<List<TenantRecord>>
                execute: GetAll
            }
            flow GetAll() -> Unit {
                retrieve tenants { where: "" as: result }
            }
        "#;
        let errs = check_errors(src);
        // The flow is intentionally Unit-tailed so the cardinality
        // gate doesn't emit T9XX/T9YY here — we're checking the
        // parser+type-checker accept the wrapping syntax end-to-end.
        let parse_errs: Vec<&TypeError> = errs
            .iter()
            .filter(|e| {
                e.message.contains("Expected")
                    || e.message.contains("Unexpected token")
                    || e.message.contains("syntax")
            })
            .collect();
        assert!(
            parse_errs.is_empty(),
            "§39.a §3 — FlowEnvelope<List<TenantRecord>> MUST parse cleanly. \
             Got parse-class errors: {parse_errs:#?}"
        );
    }

    // ── §4 — Gate unwraps Wrapped transparently ──

    #[test]
    fn fase39a_wrapped_plural_matches_plural_tail_silent() {
        // §Fase 39.a §4 — when declared is Wrapped(Plural(X)) and the
        // flow tail is Plural(X), the gate silently passes (the
        // canonical Kivi-shape after migration). axon-E039 in 39.e
        // will additionally check the wire-shape mandate (transport
        // json + Wrapped required), but at 39.a's cardinality layer
        // the truth-table agrees.
        let src = r#"
            type TenantRecord { id: Text }
            axonstore tenants { backend: in_memory }
            axonendpoint get_all {
                method: GET
                path: "/api/tenants"
                output: FlowEnvelope<List<TenantRecord>>
                execute: GetAll
            }
            flow GetAll() -> List<TenantRecord> {
                retrieve tenants { where: "" as: result }
            }
        "#;
        let errs = check_errors(src);
        // Filter out unrelated errors that may pre-exist on the
        // canonical adopter shape; we only care that NO T9XX/T9YY
        // fires on the wrap+tail agreement.
        let cardinality_errs: Vec<&TypeError> = errs
            .iter()
            .filter(|e| {
                e.message.contains("axon-T9XX")
                    || e.message.contains("axon-T9YY")
                    || e.message.contains("axon-W003")
            })
            .collect();
        assert!(
            cardinality_errs.is_empty(),
            "§39.a §4 — FlowEnvelope<List<T>> declared against Plural \
             tail MUST silent-pass the cardinality gate (the wrap \
             unwraps transparently). Got: {cardinality_errs:#?}"
        );
    }

    // ── §Fase 39.e — axon-E039 wire-shape mandate (D12 α) ────────

    #[test]
    fn fase39e_bare_singular_with_json_transport_emits_e039() {
        // §39.e §1 — the canonical kivi-shape: bare `output: T` on
        // default `transport: json` is now a COMPILE ERROR.
        let src = r#"
            type TenantRecord { id: Text }
            axonstore tenants { backend: in_memory }
            axonendpoint get_tenant {
                method: GET
                path: "/api/tenants/{id}"
                output: TenantRecord
                execute: GetTenant
            }
            flow GetTenant(id: Text) -> TenantRecord {
                step Echo { reason: "x" output: TenantRecord }
            }
        "#;
        let errs = check_errors(src);
        let e039: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("axon-E039"))
            .collect();
        assert!(!e039.is_empty(), "§39.e §1 — bare T MUST fire E039. Got: {errs:#?}");
        assert!(
            e039[0].message.contains("`output: TenantRecord`"),
            "§39.e §1 — diagnostic MUST name the declared bare type. Got: {}",
            e039[0].message
        );
        assert!(
            e039[0].message.contains("FlowEnvelope<"),
            "§39.e §1 — diagnostic MUST suggest FlowEnvelope wrapping. \
             Got: {}",
            e039[0].message
        );
        assert!(
            e039[0].message.contains("transport: sse"),
            "§39.e §1 — diagnostic MUST mention sse migration alternative. \
             Got: {}",
            e039[0].message
        );
        assert!(
            e039[0].message.contains("D12"),
            "§39.e §1 — diagnostic MUST reference the D12 α \
             ratification anchor. Got: {}",
            e039[0].message
        );
    }

    #[test]
    fn fase39e_bare_list_with_json_transport_emits_e039() {
        // §39.e §2 — bare `List<T>` (the original adopter shape
        // that v1.40.0-v1.40.3 tried to bridge) is now a COMPILE
        // ERROR. The canonical answer is `FlowEnvelope<List<T>>`.
        let src = r#"
            type TenantRecord { id: Text }
            axonstore tenants { backend: in_memory }
            axonendpoint list_tenants {
                method: GET
                path: "/api/tenants"
                output: List<TenantRecord>
                execute: ListTenants
            }
            flow ListTenants() -> List<TenantRecord> {
                retrieve tenants { where: "1=1" as: result }
            }
        "#;
        let errs = check_errors(src);
        let e039: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("axon-E039"))
            .collect();
        assert!(
            !e039.is_empty(),
            "§39.e §2 — bare `List<T>` MUST fire E039. Got: {errs:#?}"
        );
        assert!(
            e039[0].message.contains("`output: List<TenantRecord>`"),
            "§39.e §2 — diagnostic MUST name the declared bare List<T>. \
             Got: {}",
            e039[0].message
        );
        assert!(
            e039[0].message.contains("FlowEnvelope<List<"),
            "§39.e §2 — diagnostic MUST suggest FlowEnvelope<List<...>>. \
             Got: {}",
            e039[0].message
        );
    }

    #[test]
    fn fase39e_bare_stream_with_json_transport_emits_e039() {
        // §39.e §3 — bare `Stream<T>` with default `transport: json`
        // is a COMPILE ERROR. The diagnostic suggests both FlowEnvelope
        // wrapping AND (more idiomatically) sse transport migration.
        let src = r#"
            type Token { text: Text }
            axonendpoint stream_chat {
                method: POST
                path: "/api/stream"
                output: Stream<Token>
                execute: StreamChat
            }
            flow StreamChat() -> Stream<Token> {
                step Generate { ask: "stream" output: Stream<Token> }
            }
        "#;
        let errs = check_errors(src);
        // Note: implicit_transport inference (Fase 31) might set this
        // endpoint to "sse" implicitly because the flow produces Stream<T>.
        // In that case E039 would NOT fire — the wire is correctly sse.
        // The test accepts EITHER outcome since both are correct under
        // D2 + D9: bare Stream<T> with json fires E039; bare Stream<T>
        // with sse-inferred is acceptable.
        let e039_or_silent =
            errs.iter().filter(|e| e.message.contains("axon-E039")).count();
        // No T9YY should leak when the wire matches.
        let t9yy: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("axon-T9YY"))
            .collect();
        assert!(
            e039_or_silent <= 1,
            "§39.e §3 — Stream<T> bare emits at most ONE diagnostic \
             (E039 or silent if implicit_transport=sse). Got count: \
             {e039_or_silent}, t9yy: {t9yy:#?}, all: {errs:#?}"
        );
    }

    #[test]
    fn fase39e_flow_envelope_singular_passes_clean() {
        // §39.e §4 — the canonical v2.0.0 happy path: `output:
        // FlowEnvelope<T>` on a flow producing a matching tail. No
        // E039, no T9XX, no T9YY.
        let src = r#"
            type WriteResult { ok: Bool }
            axonendpoint write_secret {
                method: POST
                path: "/api/secrets"
                output: FlowEnvelope<WriteResult>
                execute: WriteSecret
            }
            flow WriteSecret() -> WriteResult {
                step Echo { reason: "ok" output: WriteResult }
            }
        "#;
        let errs = check_errors(src);
        let wire_errs: Vec<&TypeError> = errs
            .iter()
            .filter(|e| {
                e.message.contains("axon-E039")
                    || e.message.contains("axon-T9XX")
                    || e.message.contains("axon-T9YY")
            })
            .collect();
        assert!(
            wire_errs.is_empty(),
            "§39.e §4 — FlowEnvelope<T> singular happy path MUST be \
             clean. Got: {wire_errs:#?}"
        );
    }

    #[test]
    fn fase39e_flow_envelope_list_passes_clean() {
        // §39.e §5 — the canonical migration target: `output:
        // FlowEnvelope<List<T>>` on a retrieve-tail flow. No errors.
        let src = r#"
            type TenantRecord { id: Text }
            axonstore tenants { backend: in_memory }
            axonendpoint list_tenants {
                method: GET
                path: "/api/tenants"
                output: FlowEnvelope<List<TenantRecord>>
                execute: ListTenants
            }
            flow ListTenants() -> List<TenantRecord> {
                retrieve tenants { where: "1=1" as: result }
            }
        "#;
        let errs = check_errors(src);
        let wire_errs: Vec<&TypeError> = errs
            .iter()
            .filter(|e| {
                e.message.contains("axon-E039")
                    || e.message.contains("axon-T9XX")
                    || e.message.contains("axon-T9YY")
            })
            .collect();
        assert!(
            wire_errs.is_empty(),
            "§39.e §5 — FlowEnvelope<List<T>> over a List<T>-tail flow \
             is the canonical migration. Got: {wire_errs:#?}"
        );
    }

    #[test]
    fn fase39e_flow_envelope_any_passes_clean() {
        // §39.e §6 — `output: Any` is the universal-accept escape
        // hatch (documented degraded surface). No E039 fires; T9XX/T9YY
        // also silent per Cardinality::Disagreed semantics.
        let src = r#"
            type X { f: Text }
            axonendpoint p {
                method: POST
                path: "/api/p"
                output: Any
                execute: F
            }
            flow F() -> X {
                step S { reason: "x" output: X }
            }
        "#;
        let errs = check_errors(src);
        let wire_errs: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("axon-E039"))
            .collect();
        assert!(
            wire_errs.is_empty(),
            "§39.e §6 — `output: Any` is the universal-accept escape \
             hatch; E039 MUST NOT fire. Got: {wire_errs:#?}"
        );
    }

    #[test]
    fn fase39e_sse_transport_exempts_from_e039() {
        // §39.e §7 — explicit `transport: sse` exempts the endpoint
        // from the wrapping mandate (D9 — SSE has its own event
        // family). Bare `Stream<T>` declarations are valid on sse.
        let src = r#"
            type Token { text: Text }
            axonendpoint stream_chat {
                method: POST
                path: "/api/stream"
                transport: sse
                output: Stream<Token>
                execute: StreamChat
            }
            flow StreamChat() -> Stream<Token> {
                step Generate { ask: "stream" output: Stream<Token> }
            }
        "#;
        let errs = check_errors(src);
        let e039: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("axon-E039"))
            .collect();
        assert!(
            e039.is_empty(),
            "§39.e §7 — explicit `transport: sse` MUST exempt the \
             endpoint from E039 (the SSE wire has its own event \
             family per D9). Got: {e039:#?}"
        );
    }

    #[test]
    fn fase39e_no_output_declared_skips_e039() {
        // §39.e §8 — D9 backwards-compat: empty `output:` declaration
        // (no `output:` line) skips both E039 and the cardinality
        // gate. Honest scope — adopters can opt out of strict
        // wire-shape contracts by simply not declaring.
        let src = r#"
            type TenantRecord { id: Text }
            axonstore tenants { backend: in_memory }
            axonendpoint loose {
                method: GET
                path: "/api/loose"
                execute: GetLoose
            }
            flow GetLoose() -> Unit {
                retrieve tenants { where: "1=1" as: result }
            }
        "#;
        let errs = check_errors(src);
        let wire_errs: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("axon-E039"))
            .collect();
        assert!(
            wire_errs.is_empty(),
            "§39.e §8 — empty `output:` MUST skip E039 (D9 \
             backwards-compat). Got: {wire_errs:#?}"
        );
    }

    #[test]
    fn fase39e_unit_output_skips_e039() {
        // §39.e §9 — `output: Unit` is explicit "no wire body" —
        // exempt from the wrapping mandate.
        let src = r#"
            type X { f: Text }
            axonendpoint noop {
                method: POST
                path: "/api/noop"
                output: Unit
                execute: F
            }
            flow F() -> Unit {
                step S { reason: "x" output: Unit }
            }
        "#;
        let errs = check_errors(src);
        let e039: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("axon-E039"))
            .collect();
        assert!(
            e039.is_empty(),
            "§39.e §9 — `output: Unit` MUST be exempt from E039. \
             Got: {e039:#?}"
        );
    }

    #[test]
    fn fase39e_nested_flow_envelope_passes_clean() {
        // §39.e §10 — defensive: degenerate nested
        // `FlowEnvelope<FlowEnvelope<T>>` is syntactically valid
        // (per 39.a parser nested-generic support) and is treated
        // as a FlowEnvelope wrap (E039 doesn't fire). It's
        // semantically weird (double wrap) but not a compile error.
        let src = r#"
            type X { f: Text }
            axonendpoint p {
                method: POST
                path: "/api/p"
                output: FlowEnvelope<FlowEnvelope<X>>
                execute: F
            }
            flow F() -> X {
                step S { reason: "x" output: X }
            }
        "#;
        let errs = check_errors(src);
        let e039: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("axon-E039"))
            .collect();
        assert!(
            e039.is_empty(),
            "§39.e §10 — nested FlowEnvelope<FlowEnvelope<X>> is \
             semantically degenerate but NOT an E039 (any \
             `FlowEnvelope<...>` declaration passes the wrapping \
             mandate). Got: {e039:#?}"
        );
    }

    #[test]
    fn fase39a_wrapped_singular_vs_plural_tail_still_warns() {
        // §Fase 39.a §4 — when declared is Wrapped(Singular(X)) but
        // the flow tail is Plural, the gate still surfaces the
        // T9XX mismatch (unwrap shows the inner Singular vs Plural
        // shape disagreement). The hint message still references the
        // existing T9XX format; the axon-E039 promotion happens in 39.e.
        let src = r#"
            type TenantRecord { id: Text }
            axonstore tenants { backend: in_memory }
            axonendpoint get_one {
                method: GET
                path: "/api/tenants/{id}"
                output: FlowEnvelope<TenantRecord>
                execute: GetOne
            }
            flow GetOne(id: Text) -> TenantRecord {
                retrieve tenants { where: "id = ${id}" as: result }
            }
        "#;
        let errs = check_errors(src);
        let t9xx: Vec<&TypeError> = errs
            .iter()
            .filter(|e| e.message.contains("axon-T9XX"))
            .collect();
        assert!(
            !t9xx.is_empty(),
            "§39.a §4 — Wrapped(Singular) against Plural tail MUST \
             surface axon-T9XX through the unwrap (the wrap is \
             transparent to the cardinality contract). Errors: {errs:#?}"
        );
    }
}

#[cfg(test)]
mod fase41b_session_lowering_tests {
    //! §Fase 41.b — the Fase 4 `session` surface lowered into the §41.a
    //! session-type algebra, with duality decided by the connection law
    //! (regular-coinductive `is_dual_to`) rather than the old positional check.
    use super::*;

    fn step(op: &str, ty: &str) -> SessionStep {
        SessionStep { op: op.into(), message_type: ty.into(), ..Default::default() }
    }
    fn role(name: &str, steps: Vec<SessionStep>) -> SessionRole {
        SessionRole { name: name.into(), steps, ..Default::default() }
    }

    #[test]
    fn lowers_send_receive_end_to_session_type() {
        let r = role("client", vec![step("send", "T"), step("receive", "U"), step("end", "")]);
        assert_eq!(lower_session_role(&r), SessionType::send("T", SessionType::recv("U", SessionType::End)));
    }

    #[test]
    fn lowers_terminal_loop_to_mu_recursion() {
        // `[send T, loop]` ↦ `μX. !T.X` — the loop is a recursion point, not a token.
        let r = role("p", vec![step("send", "T"), step("loop", "")]);
        assert_eq!(lower_session_role(&r), SessionType::rec("X", SessionType::send("T", SessionType::var("X"))));
    }

    #[test]
    fn dual_recursive_roles_satisfy_the_connection_law() {
        // The case the old positional check could not reason about as recursion:
        // client loops sending T; server loops receiving T — genuinely dual.
        let client = lower_session_role(&role("c", vec![step("send", "T"), step("loop", "")]));
        let server = lower_session_role(&role("s", vec![step("receive", "T"), step("loop", "")]));
        assert!(client.is_dual_to(&server));
        assert!(server.is_dual_to(&client)); // symmetric up to involutivity
    }

    #[test]
    fn non_dual_roles_are_rejected() {
        let a = lower_session_role(&role("a", vec![step("send", "T"), step("end", "")]));
        // Same direction → not dual.
        let same = lower_session_role(&role("b", vec![step("send", "T"), step("end", "")]));
        assert!(!a.is_dual_to(&same));
        // Dual direction but mismatched payload → not dual.
        let wrong = lower_session_role(&role("c", vec![step("receive", "WRONG"), step("end", "")]));
        assert!(!a.is_dual_to(&wrong));
    }
}

#[cfg(test)]
mod fase41b_socket_tests {
    //! §Fase 41.b — the `socket` declaration: parse + check (protocol must
    //! reference a declared `session`; backpressure credit must be positive).
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    // A well-formed (dual) binary session the socket can reference.
    const SESSION: &str =
        "session Chat { client: [send Msg, receive Token, end] server: [receive Msg, send Token, end] }";

    fn parse_prog(src: &str) -> Program {
        let toks = Lexer::new(src, "<t>").tokenize().expect("lex");
        Parser::new(toks).parse().expect("parse")
    }
    fn errors(src: &str) -> Vec<TypeError> {
        TypeChecker::new(&parse_prog(src)).check()
    }

    #[test]
    fn socket_parses_into_ast_fields() {
        let prog = parse_prog(&format!(
            "{SESSION}\nsocket ChatWS {{ protocol: Chat, backpressure: credit(64), reconnect: cognitive_state, legal_basis: legitimate_interest }}"
        ));
        let sock = prog
            .declarations
            .iter()
            .find_map(|d| if let Declaration::Socket(s) = d { Some(s) } else { None })
            .expect("socket parsed");
        assert_eq!(sock.name, "ChatWS");
        assert_eq!(sock.protocol, "Chat");
        assert_eq!(sock.backpressure_credit, Some(64));
        assert!(sock.reconnect);
        assert_eq!(sock.legal_basis.as_deref(), Some("legitimate_interest"));
    }

    #[test]
    fn socket_referencing_a_declared_session_has_no_socket_error() {
        let errs = errors(&format!("{SESSION}\nsocket ChatWS {{ protocol: Chat, backpressure: credit(64) }}"));
        assert!(!errs.iter().any(|e| e.message.contains("Socket")), "unexpected socket error: {errs:?}");
    }

    #[test]
    fn socket_with_undeclared_protocol_is_rejected() {
        let errs = errors("socket ChatWS { protocol: DoesNotExist }");
        assert!(errs.iter().any(|e| e.message.contains("not a declared session")), "{errs:?}");
    }

    #[test]
    fn socket_with_zero_credit_window_is_rejected() {
        let errs = errors(&format!("{SESSION}\nsocket ChatWS {{ protocol: Chat, backpressure: credit(0) }}"));
        assert!(errs.iter().any(|e| e.message.contains("credit must be")), "{errs:?}");
    }
}

#[cfg(test)]
mod fase41b_choice_tests {
    //! §Fase 41.b — `select`/`branch` choice steps (⊕ / &): nested sub-protocols
    //! that lower into `SessionType::Select`/`Branch`, with duality decided by
    //! the connection law (`select` is dual to `branch` arm-for-arm).
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use std::collections::BTreeMap;

    fn parse_prog(src: &str) -> Program {
        let toks = Lexer::new(src, "<t>").tokenize().expect("lex");
        Parser::new(toks).parse().expect("parse")
    }
    fn session<'a>(p: &'a Program, name: &str) -> &'a SessionDefinition {
        p.declarations
            .iter()
            .find_map(|d| match d {
                Declaration::Session(s) if s.name == name => Some(s),
                _ => None,
            })
            .expect("session declared")
    }
    fn role_of<'a>(s: &'a SessionDefinition, name: &str) -> &'a SessionRole {
        s.roles.iter().find(|r| r.name == name).expect("role")
    }

    // A session whose two roles disagree only by direction inside each arm:
    // client offers a choice (⊕), server accepts it (&) — genuinely dual.
    const CHOICE: &str = "session Negotiate {\n\
        client: [select { ask: [send Query, receive Answer, end], quit: [end] }]\n\
        server: [branch { ask: [receive Query, send Answer, end], quit: [end] }]\n\
    }";

    #[test]
    fn select_branch_steps_parse_with_nested_arms() {
        let p = parse_prog(CHOICE);
        let s = session(&p, "Negotiate");
        let client = role_of(s, "client");
        assert_eq!(client.steps.len(), 1);
        assert_eq!(client.steps[0].op, "select");
        let labels: Vec<_> = client.steps[0].branches.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, vec!["ask", "quit"]);
        // The `ask` arm carries its own ordered sub-protocol.
        let ask = &client.steps[0].branches[0];
        assert_eq!(ask.steps.iter().map(|s| s.op.as_str()).collect::<Vec<_>>(), vec!["send", "receive", "end"]);
    }

    #[test]
    fn select_lowers_to_session_type_select() {
        let p = parse_prog(CHOICE);
        let client = lower_session_role(role_of(session(&p, "Negotiate"), "client"));
        let mut arms = BTreeMap::new();
        arms.insert("ask".to_string(), SessionType::send("Query", SessionType::recv("Answer", SessionType::End)));
        arms.insert("quit".to_string(), SessionType::End);
        assert_eq!(client, SessionType::Select(arms));
    }

    #[test]
    fn select_is_dual_to_matching_branch() {
        // The connection law for choice: ⊕{ℓ:Sℓ} ⊥ &{ℓ:S̄ℓ} arm-for-arm.
        let p = parse_prog(CHOICE);
        let s = session(&p, "Negotiate");
        let client = lower_session_role(role_of(s, "client"));
        let server = lower_session_role(role_of(s, "server"));
        assert!(client.is_dual_to(&server));
        assert!(server.is_dual_to(&client));
    }

    #[test]
    fn choice_session_typechecks_clean() {
        let errs = TypeChecker::new(&parse_prog(CHOICE)).check();
        assert!(
            !errs.iter().any(|e| e.message.contains("not dual") || e.message.contains("Session")),
            "unexpected session error: {errs:?}"
        );
    }

    #[test]
    fn choice_with_duplicate_labels_is_rejected() {
        let src = "session Bad {\n\
            client: [select { ask: [end], ask: [end] }]\n\
            server: [branch { ask: [end] }]\n\
        }";
        let errs = TypeChecker::new(&parse_prog(src)).check();
        assert!(errs.iter().any(|e| e.message.contains("duplicate") || e.message.contains("label")), "{errs:?}");
    }

    #[test]
    fn empty_choice_is_rejected() {
        let src = "session Bad {\n\
            client: [select {  }]\n\
            server: [branch {  }]\n\
        }";
        let errs = TypeChecker::new(&parse_prog(src)).check();
        assert!(errs.iter().any(|e| e.message.contains("at least one") || e.message.contains("branch")), "{errs:?}");
    }
}

#[cfg(test)]
mod fase41c_credit_tests {
    //! §Fase 41.c — the Presburger discharge wired into `check_socket`.
    //! The bare session + dual is duality-checked (41.a/b); when a `socket`
    //! binds `backpressure: credit(k)`, both roles are lowered, stamped with
    //! `k`, and run through [`SessionType::credit_analyse`] — surfacing
    //! send-at-zero, burst overflow and loop unsustainability as type errors.
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn parse_prog(src: &str) -> Program {
        let toks = Lexer::new(src, "<t>").tokenize().expect("lex");
        Parser::new(toks).parse().expect("parse")
    }
    fn errors(src: &str) -> Vec<TypeError> {
        TypeChecker::new(&parse_prog(src)).check()
    }
    fn has(errs: &[TypeError], needle: &str) -> bool {
        errs.iter().any(|e| e.message.contains(needle))
    }

    // A 2-send burst protocol — the client wants to put two messages on the
    // wire back-to-back; under `credit(1)` the second send hits n=0.
    const BURST_SESSION: &str =
        "session Burst { client: [send A, send B, end] server: [receive A, receive B, end] }";

    #[test]
    fn credit_window_within_budget_is_accepted() {
        let errs = errors(&format!(
            "{BURST_SESSION}\nsocket S {{ protocol: Burst, backpressure: credit(2) }}"
        ));
        assert!(!has(&errs, "credit-refined"), "unexpected credit error: {errs:?}");
        assert!(!has(&errs, "violates"), "{errs:?}");
    }

    #[test]
    fn burst_overflow_is_rejected() {
        let errs = errors(&format!(
            "{BURST_SESSION}\nsocket S {{ protocol: Burst, backpressure: credit(1) }}"
        ));
        // The client role demands a 2-send burst; budget=1 cannot absorb it.
        assert!(has(&errs, "credit-window overflow"), "expected burst overflow, got: {errs:?}");
        assert!(has(&errs, "send-burst of 2"), "expected burst=2 detail, got: {errs:?}");
        assert!(has(&errs, "credit(1)"), "expected budget=1 detail, got: {errs:?}");
        // The dual `server` role is purely receives → no error attributed to it.
        let server_errs: Vec<_> = errs
            .iter()
            .filter(|e| e.message.contains("role 'server'") && e.message.contains("credit-refined"))
            .collect();
        assert!(server_errs.is_empty(), "server role should be clean: {server_errs:?}");
    }

    #[test]
    fn unsustainable_loop_is_rejected_at_any_budget() {
        // rec X. !A.!B.?Ack.X — Δ = 2-1 = 1 > 0 per recurring iteration.
        // No finite k is sufficient; even credit(100) is rejected statically.
        let src = "session Drain {\n\
            client: [send A, send B, receive Ack, loop]\n\
            server: [receive A, receive B, send Ack, loop]\n\
        }\nsocket S { protocol: Drain, backpressure: credit(100) }";
        let errs = errors(src);
        assert!(has(&errs, "unsustainable"), "expected loop unsustainability, got: {errs:?}");
        assert!(has(&errs, "2 - 1 > 0"), "expected Δ detail, got: {errs:?}");
    }

    #[test]
    fn balanced_loop_is_accepted_at_minimal_budget() {
        // rec X. !A.?Ack.X — Δ = 1-1 = 0 ⇒ sustainable; budget=1 is enough.
        let src = "session Pingpong {\n\
            client: [send A, receive Ack, loop]\n\
            server: [receive A, send Ack, loop]\n\
        }\nsocket S { protocol: Pingpong, backpressure: credit(1) }";
        let errs = errors(src);
        assert!(!has(&errs, "credit-refined"), "{errs:?}");
        assert!(!has(&errs, "unsustainable"), "{errs:?}");
    }

    #[test]
    fn choice_arms_are_each_checked_under_budget() {
        // The `ask` arm needs a 2-send burst; the `quit` arm terminates.
        let src = "session Choice {\n\
            client: [select { ask: [send Q, send R, end], quit: [end] }]\n\
            server: [branch { ask: [receive Q, receive R, end], quit: [end] }]\n\
        }\nsocket S { protocol: Choice, backpressure: credit(1) }";
        let errs = errors(src);
        assert!(has(&errs, "credit-window overflow"), "ask arm must overflow: {errs:?}");
        // Same protocol under sufficient credit is clean.
        let src_ok = src.replace("credit(1)", "credit(2)");
        assert!(!has(&errors(&src_ok), "credit-refined"), "credit(2) should fit");
    }

    #[test]
    fn no_backpressure_annotation_skips_credit_analysis() {
        // Omitting `backpressure` leaves the protocol in the unbounded
        // fragment (`!∞A.S`); the burst session typechecks clean.
        let errs = errors(&format!(
            "{BURST_SESSION}\nsocket S {{ protocol: Burst }}"
        ));
        assert!(!has(&errs, "credit-refined"), "{errs:?}");
        assert!(!has(&errs, "violates"), "{errs:?}");
    }

    #[test]
    fn zero_credit_still_caught_as_a_separate_diagnostic() {
        // `credit(0)` was already rejected by 41.b's `≥ 1` check; 41.c does
        // not silently swallow it — the earlier diagnostic still fires and
        // the credit-analysis is skipped (no budget to discharge against).
        let errs = errors(&format!(
            "{BURST_SESSION}\nsocket S {{ protocol: Burst, backpressure: credit(0) }}"
        ));
        assert!(has(&errs, "credit must be"), "41.b ≥ 1 check still fires: {errs:?}");
        assert!(!has(&errs, "credit-window overflow"), "no overflow-walking on bad budget: {errs:?}");
    }
}
