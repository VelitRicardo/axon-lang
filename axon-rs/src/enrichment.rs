//! §Fase 104.a — Governed Contact Enrichment runtime: the OSS contract + the
//! `scrape_enrich` provider dispatch, the enterprise-engine injection seam
//! (D104.2), and the born-**Inferred** epistemic discipline (D104.4).
//!
//! **What enrichment is.** Given a partial contact (`name` + `company`, or a
//! `domain`, or a `linkedin` URL), resolve the missing `email` / `phone` /
//! `linkedin` via a 3rd-party enrichment vendor. Unlike [`crate::scrape_tool`]
//! (fetch a URL), enrichment is a *structured lookup* → a structured, per-field
//! result, each field carrying its own confidence + epistemic level.
//!
//! **The load-bearing property (D104.4): enrichment is INFERENCE, not fact.** An
//! enriched email is a vendor's probabilistic guess, NEVER a verified truth. Every
//! field is born at a BOUNDED epistemic level — `Speculate` (pattern/heuristic
//! guess) or `Believe` (the vendor deliverability-verified it), **never `Know`**
//! (the believe-ceiling, §101). And the value is born epistemically **Untrusted**
//! (⊥, reusing [`crate::emcp::EpistemicTaint::Untrusted`] exactly like scraped
//! content) — a vendor's channel is adversarial; the value is not a belief until
//! a `shield` scans it and the flow `anchor`'s `confidence_floor` clears it.
//!
//! **The engine is enterprise (D104.2).** OSS ships the CONTRACT + a default
//! [`NoProvider`] that TYPED-REFUSES every call — never a fabricated contact
//! (D104.6 / the §101 D101.7 honesty). The enterprise generic HTTP provider
//! (§104.a) registers via [`register_provider`]; the `scrape:enrich` RBAC gate +
//! the per-tenant legal flag + the credential resolution + the audit all live in
//! enterprise. With no engine wired, `scrape_enrich` is an honest refusal.

use std::sync::{Arc, OnceLock, RwLock};

use serde::{Deserialize, Serialize};

use crate::emcp::EpistemicTaint;
use crate::tool_executor::ToolResult;
use crate::tool_registry::ToolEntry;

// ════════════════════════════════════════════════════════════════════════════
//  ContactQuery — the structured lookup input
// ════════════════════════════════════════════════════════════════════════════

/// The partial contact to enrich. Every field is optional; a provider uses
/// whatever is supplied (e.g. `name` + `domain` → `email`). At least one
/// identifying field must be present or the call is a typed
/// [`EnrichmentError::MissingQuery`].
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ContactQuery {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub linkedin: String,
}

impl ContactQuery {
    /// A query is usable iff it carries at least one identifying field.
    pub fn is_empty(&self) -> bool {
        self.name.trim().is_empty()
            && self.company.trim().is_empty()
            && self.domain.trim().is_empty()
            && self.linkedin.trim().is_empty()
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  The bounded epistemic level (the believe-ceiling, D104.4)
// ════════════════════════════════════════════════════════════════════════════

/// How much an enriched field may be trusted — the believe-ceiling (D104.4).
/// A provider MAY return `Speculate` or `Believe`; it can NEVER mint `Know`. The
/// serialised form is the lattice name so `§55` epistemic wiring + the flow
/// `anchor` can read it directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnrichmentLevel {
    /// A pattern/heuristic guess (e.g. `first.last@domain`) — weakly held.
    Speculate,
    /// The vendor positively verified the field (e.g. SMTP-deliverable) — the
    /// CEILING; enrichment never rises above `believe` into `know`.
    Believe,
}

impl EnrichmentLevel {
    /// Clamp any provider-supplied intent to the ceiling: `Know`/`Fact` collapse
    /// to `Believe`; anything weaker to `Speculate`. Providers pass a confidence;
    /// the host maps it here so the ceiling is enforced BY CONSTRUCTION.
    pub fn from_confidence(confidence: f64) -> Self {
        if confidence >= 0.85 {
            EnrichmentLevel::Believe
        } else {
            EnrichmentLevel::Speculate
        }
    }
}

/// One resolved field: the value + the vendor's confidence + the (ceiling-bounded)
/// epistemic level. The value is born Untrusted; the level is born ≤ `Believe`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichedField {
    pub value: String,
    /// The vendor's confidence in `[0,1]`, clamped. Drives the flow `anchor`.
    pub confidence: f64,
    pub level: EnrichmentLevel,
}

impl EnrichedField {
    /// Build a field, enforcing the ceiling: confidence clamped to `[0,1]`, the
    /// level derived from it so a vendor can never over-assert (D104.4).
    pub fn new(value: impl Into<String>, confidence: f64) -> Self {
        let c = if confidence.is_nan() { 0.0 } else { confidence.clamp(0.0, 1.0) };
        EnrichedField {
            value: value.into(),
            confidence: c,
            level: EnrichmentLevel::from_confidence(c),
        }
    }
}

/// The enrichment result: the resolved fields (each optional — a miss is `None`,
/// never a fabricated value, D104.6) + the vendor tag for the audit trail.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnrichmentResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<EnrichedField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<EnrichedField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linkedin: Option<EnrichedField>,
    /// The provider that produced this result (`"none"` for the OSS default) —
    /// provenance for the audit row.
    pub provider: String,
}

impl EnrichmentResult {
    /// The count of fields actually resolved — the audit witnesses this (never
    /// the values, D104.5).
    pub fn resolved_count(&self) -> usize {
        self.email.is_some() as usize
            + self.phone.is_some() as usize
            + self.linkedin.is_some() as usize
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Errors — every failure is a typed refusal, never a fabricated contact
// ════════════════════════════════════════════════════════════════════════════

/// Everything that can go wrong enriching a contact. A miss is NOT an error (it
/// is an empty result); an error is a refusal to proceed (D104.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnrichmentError {
    /// No enterprise provider is registered — the OSS build cannot enrich.
    NoProviderConfigured,
    /// The query carried no identifying field.
    MissingQuery,
    /// The vendor call failed (transport / auth / malformed response).
    ProviderFailed(String),
    /// The tenant's enrichment quota with the vendor is exhausted — degrade to a
    /// typed empty, never a stale/fabricated contact.
    QuotaExceeded,
}

impl std::fmt::Display for EnrichmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnrichmentError::NoProviderConfigured => write!(
                f,
                "enrich: no enrichment provider is configured — the OSS build has no engine \
                 (this is a typed refusal, never a fabricated contact)"
            ),
            EnrichmentError::MissingQuery => write!(
                f,
                "enrich: the query carried no identifying field (need one of name/company/domain/linkedin)"
            ),
            EnrichmentError::ProviderFailed(e) => write!(f, "enrich: provider failed: {e}"),
            EnrichmentError::QuotaExceeded => write!(f, "enrich: vendor quota exhausted"),
        }
    }
}

impl std::error::Error for EnrichmentError {}

// ════════════════════════════════════════════════════════════════════════════
//  The engine contract + the injection seam (D104.2)
// ════════════════════════════════════════════════════════════════════════════

/// The contract every enrichment engine satisfies: a `ContactQuery` → an
/// `EnrichmentResult` whose fields are born ≤ `Believe` (D104.4). The enterprise
/// generic HTTP provider (§104.a) implements this. Failure is a typed
/// [`EnrichmentError`], never a fabricated contact (D104.6).
pub trait EnrichmentProvider: Send + Sync {
    /// A stable provider identifier for the audit row (e.g. `"http-generic"`).
    fn name(&self) -> &str;
    /// Resolve the missing fields. MUST return a typed error on failure and a
    /// `None` field on a miss — never a fabricated value.
    fn enrich(&self, query: &ContactQuery) -> Result<EnrichmentResult, EnrichmentError>;
}

/// The OSS default engine: **none**. It typed-refuses every enrichment — the
/// honest state of a runtime with no engine wired (the §101 `NoEngine` shape).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProvider;

impl EnrichmentProvider for NoProvider {
    fn name(&self) -> &str {
        "none"
    }
    fn enrich(&self, _query: &ContactQuery) -> Result<EnrichmentResult, EnrichmentError> {
        Err(EnrichmentError::NoProviderConfigured)
    }
}

fn registry() -> &'static RwLock<Option<Arc<dyn EnrichmentProvider>>> {
    static REG: OnceLock<RwLock<Option<Arc<dyn EnrichmentProvider>>>> = OnceLock::new();
    REG.get_or_init(|| RwLock::new(None))
}

/// §Fase 104.a — register the process-wide enrichment provider. OSS ships none
/// (every enrichment typed-refuses); the enterprise host mounts the generic HTTP
/// provider here at boot. Replaces any prior registration. Exactly the
/// [`crate::extraction::register_engine`] / [`crate::scrape_tool::register_scrape_fetcher`]
/// injection shape.
pub fn register_provider(provider: Arc<dyn EnrichmentProvider>) {
    *registry().write().expect("enrichment registry poisoned") = Some(provider);
}

/// Clear the registered provider (back to the `NoProvider` typed refusal).
pub fn clear_provider() {
    *registry().write().expect("enrichment registry poisoned") = None;
}

/// The active provider handle, if one is registered.
pub fn active_provider() -> Option<Arc<dyn EnrichmentProvider>> {
    registry().read().expect("enrichment registry poisoned").clone()
}

/// Run the active provider, or typed-refuse if none is registered (D104.6).
fn run_active(query: &ContactQuery) -> Result<EnrichmentResult, EnrichmentError> {
    if query.is_empty() {
        return Err(EnrichmentError::MissingQuery);
    }
    match active_provider() {
        Some(p) => p.enrich(query),
        None => Err(EnrichmentError::NoProviderConfigured),
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Provenance-tagged outcome + the `scrape_enrich` dispatch
// ════════════════════════════════════════════════════════════════════════════

/// The provenance-tagged outcome. `taint` is ALWAYS [`EpistemicTaint::Untrusted`]
/// — an enriched value is born ⊥, exactly like scraped content (D104.4). The
/// registry integration flattens this to a [`ToolResult`]; the taint is exposed
/// here for the §98.f-style IFC + the audit.
#[derive(Debug, Clone)]
pub struct EnrichmentOutcome {
    pub result: ToolResult,
    pub taint: EpistemicTaint,
}

impl EnrichmentOutcome {
    fn ok(tool_name: &str, output: String) -> Self {
        EnrichmentOutcome {
            result: ToolResult { success: true, output, tool_name: tool_name.to_string() },
            taint: EpistemicTaint::Untrusted,
        }
    }
    fn err(tool_name: &str, e: EnrichmentError) -> Self {
        EnrichmentOutcome {
            result: ToolResult { success: false, output: e.to_string(), tool_name: tool_name.to_string() },
            taint: EpistemicTaint::Untrusted,
        }
    }
}

/// Dispatch a `scrape_enrich` tool call. The argument is the structured
/// `ContactQuery` JSON (from `use Enrich(name=…, company=…)`). Returns the
/// [`ToolResult`] the registry integrates; the born-Untrusted taint lives on the
/// [`EnrichmentOutcome`].
pub fn dispatch_enrich(entry: &ToolEntry, argument: &str) -> ToolResult {
    dispatch_enrich_outcome(entry, argument).result
}

/// The taint-carrying dispatch (used by IFC + tests).
pub fn dispatch_enrich_outcome(entry: &ToolEntry, argument: &str) -> EnrichmentOutcome {
    let query: ContactQuery = match serde_json::from_str(argument) {
        Ok(q) => q,
        // A non-object body is treated as an empty query → typed MissingQuery.
        Err(_) => ContactQuery::default(),
    };
    match run_active(&query) {
        Ok(result) => match serde_json::to_string(&result) {
            Ok(json) => EnrichmentOutcome::ok(&entry.name, json),
            Err(e) => EnrichmentOutcome::err(&entry.name, EnrichmentError::ProviderFailed(format!("encode: {e}"))),
        },
        Err(e) => EnrichmentOutcome::err(&entry.name, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises the registry-touching tests — the provider registry is
    /// process-global, so `register`/`clear` must not race across parallel test
    /// threads (the §101 extraction `REG_LOCK` discipline).
    static REG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn entry() -> ToolEntry {
        ToolEntry {
            name: "Enrich".into(),
            provider: "scrape_enrich".into(),
            timeout: String::new(),
            runtime: String::new(),
            resource_ref: String::new(),
            substrate: None,
            capacity: None,
            sandbox: None,
            max_results: None,
            output_schema: String::new(),
            effect_row: vec!["network".into(), "web".into()],
            parameters: Vec::new(),
            secret: String::new(),
            secret_partition: String::new(),
            source: crate::tool_registry::ToolSource::Program,
            is_streaming: false,
            scrape: None,
        }
    }

    #[test]
    fn no_provider_is_a_typed_refusal_never_a_fabricated_contact() {
        let _g = REG_LOCK.lock().unwrap();
        clear_provider();
        let out = dispatch_enrich_outcome(&entry(), r#"{"name":"Ada","company":"acme.com"}"#);
        assert!(!out.result.success);
        assert!(out.result.output.contains("no enrichment provider"));
        // Still born Untrusted even on refusal.
        assert_eq!(out.taint, EpistemicTaint::Untrusted);
    }

    #[test]
    fn empty_query_is_missing_query() {
        let _g = REG_LOCK.lock().unwrap();
        clear_provider();
        let out = dispatch_enrich_outcome(&entry(), "{}");
        assert!(!out.result.success);
        assert!(out.result.output.contains("no identifying field"));
    }

    #[test]
    fn the_believe_ceiling_holds_a_high_confidence_field_at_believe_not_know() {
        // Even a 1.0-confidence vendor field can rise no higher than `Believe`.
        let f = EnrichedField::new("ada@acme.com", 1.0);
        assert_eq!(f.level, EnrichmentLevel::Believe);
        assert!((f.confidence - 1.0).abs() < f64::EPSILON);
        // A weak guess is Speculate.
        assert_eq!(EnrichedField::new("guess@acme.com", 0.4).level, EnrichmentLevel::Speculate);
    }

    #[test]
    fn a_registered_provider_result_serialises_and_is_born_untrusted() {
        let _g = REG_LOCK.lock().unwrap();
        struct Mock;
        impl EnrichmentProvider for Mock {
            fn name(&self) -> &str {
                "mock"
            }
            fn enrich(&self, _q: &ContactQuery) -> Result<EnrichmentResult, EnrichmentError> {
                Ok(EnrichmentResult {
                    email: Some(EnrichedField::new("ada@acme.com", 0.9)),
                    phone: None,
                    linkedin: None,
                    provider: "mock".into(),
                })
            }
        }
        register_provider(Arc::new(Mock));
        let out = dispatch_enrich_outcome(&entry(), r#"{"name":"Ada","domain":"acme.com"}"#);
        assert!(out.result.success, "output: {}", out.result.output);
        assert_eq!(out.taint, EpistemicTaint::Untrusted);
        let v: serde_json::Value = serde_json::from_str(&out.result.output).unwrap();
        assert_eq!(v["email"]["value"], "ada@acme.com");
        assert_eq!(v["email"]["level"], "believe");
        assert_eq!(v["provider"], "mock");
        clear_provider();
    }

    #[test]
    fn resolved_count_counts_present_fields_only() {
        let r = EnrichmentResult {
            email: Some(EnrichedField::new("a@b.com", 0.9)),
            phone: None,
            linkedin: Some(EnrichedField::new("in/ada", 0.7)),
            provider: "mock".into(),
        };
        assert_eq!(r.resolved_count(), 2);
    }
}
