//! v2.54.0 — the `Inferred`-extraction contract.
//!
//! v2.54.0 shipped the [`IngestProvenance::Inferred`](crate::ingest_provenance::IngestProvenance)
//! class with **no producer**: a type inhabited nowhere, so the
//! checker's `Inferred`-ceiling rules were vacuously satisfied. v2.54.0 introduces
//! the first producers — an OCR/geometric read, a PDF text path, a vision tier —
//! and this module is the contract they all satisfy.
//!
//! **Doctrine (`an_extraction_engine_is_a_witness_not_an_oracle`).** Every span an
//! engine returns is born [`IngestProvenance::Inferred`] + [`EpistemicTaint::Untrusted`]
//! and carries the engine's **measured** confidence — a geometric margin, a text-layer
//! hit, an engine-emitted per-glyph score — **never** a model's self-assessment
//!. It can never reach `know` (the design decision, enforced at compile time by
//! `axon-T1001`), and a span below the governing `anchor`'s `confidence_floor`
//! is quarantined, never delivered.
//!
//! **Typed outcomes, never silence.** No engine configured is
//! [`ExtractionError::NoEngineConfigured`], not an empty string. A blown bound is
//! a typed refusal. A crashed engine is a typed error. The one thing that must
//! never happen is a plausible-looking string of invented text — the failure mode
//! v2.54.0 made unreachable by fixing the dispatch fall-through.
//!
//! This module is **pure and engine-agnostic**: the trait, the span, the
//! confidence-floor routing. The engines themselves (the deterministic IDP-E
//! kernel v2.54.0, the sidecar front-end v2.54.0, the production engine v2.54.0)
//! implement [`ExtractionEngine`]; nothing here does pixel work.

use crate::emcp::EpistemicTaint;
use crate::ingest_provenance::IngestProvenance;

// ── Bounds ──────────────────────────────────────────────────────────

/// Max pages an extraction may process before it is refused — a 40,000-page PDF
/// is a resource-exhaustion vector.
pub const MAX_PAGES: u32 = 4096;
/// Max megapixels for a single rasterised page — a decompression-bomb image
/// blows up here, before the engine sees a pixel.
pub const MAX_MEGAPIXELS: u32 = 256;
/// Max spans a single extraction may emit — an adversarial page of noise must not
/// produce an unbounded span vector.
pub const MAX_SPANS: usize = 1_000_000;

/// The hostile-input bounds for an extraction. `Default` is the production
/// posture; enforced BEFORE the engine decodes a byte.
#[derive(Debug, Clone, Copy)]
pub struct ExtractionBounds {
    pub max_pages: u32,
    pub max_megapixels: u32,
    pub max_spans: usize,
}

impl Default for ExtractionBounds {
    fn default() -> Self {
        ExtractionBounds { max_pages: MAX_PAGES, max_megapixels: MAX_MEGAPIXELS, max_spans: MAX_SPANS }
    }
}

// ── Geometry ──────────────────────────────────────────────────────────────────

/// A span's location in the source image, in normalised page coordinates
/// (`[0,1]` each), so it is resolution-independent and replayable. This
/// is the `location` that rides into the `pix` canonical tree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl BBox {
    /// A well-formed box lies inside the unit page and has non-negative extent.
    pub fn is_valid(&self) -> bool {
        self.x >= 0.0
            && self.y >= 0.0
            && self.w >= 0.0
            && self.h >= 0.0
            && self.x + self.w <= 1.0 + f64::EPSILON
            && self.y + self.h <= 1.0 + f64::EPSILON
    }
}

// ── The hint ──────────────────────────────────────────────────────────────────

/// What the caller knows about the document, passed to the engine so it can pick
/// a strategy and (in v2.54.0) foveate. None of it is trusted; it only steers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtractionHint {
    /// `pdf | png | jpeg | tiff | …` — the source container, if known.
    pub format: Option<String>,
    /// The desired OTS transform (`pdf:text` | `image:text` | `image:description`)
    /// — which dispatch arm asked. One engine may serve several; this tells it
    /// which capability is wanted (OCR vs vision are distinct, the design decision).
    pub transform: Option<String>,
    /// A target field the caller is after (e.g. `"due_date"`) — enables foveation
    /// (v2.54.0): read the region whose information scent matches, not the page.
    pub target_field: Option<String>,
    /// A language hint for the recogniser's prototype set (e.g. `"en"`).
    pub language: Option<String>,
}

// ── The span ──────────────────────────────────────────────────────────────────

/// One extracted span — the atom of an inferred read. Born `Inferred`; there is
/// no constructor that builds a `Parsed` span (that is v2.54.0's reader), so this
/// type can only ever inhabit the `Inferred` class.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedSpan {
    /// The read text.
    pub text: String,
    /// The engine's MEASURED confidence in `[0,1]` — a geometric margin,
    /// a text-layer hit, an engine score. NOT a model self-grade.
    pub confidence: f64,
    /// 0-based page index.
    pub page: u32,
    /// Location in the source page, normalised.
    pub bbox: BBox,
}

impl ExtractedSpan {
    /// Build a span. `confidence` is clamped into `[0,1]`; a NaN confidence is a
    /// programming error and is treated as `0.0` (fail-closed — it will trip any
    /// non-zero floor).
    pub fn new(text: impl Into<String>, confidence: f64, page: u32, bbox: BBox) -> Self {
        let confidence = if confidence.is_nan() { 0.0 } else { confidence.clamp(0.0, 1.0) };
        ExtractedSpan { text: text.into(), confidence, page, bbox }
    }

    /// The provenance class of EVERY extracted span — always `Inferred`.
    /// There is deliberately no way to make this `Parsed`.
    pub const fn provenance(&self) -> IngestProvenance {
        IngestProvenance::Inferred
    }

    /// The epistemic ceiling of this span — always `believe`. No shield,
    /// no `know` block may raise it.
    pub fn epistemic_ceiling(&self) -> &'static str {
        IngestProvenance::Inferred.epistemic_ceiling()
    }
}

// ── The result ──────────────────────────────────────────────────────────────────

/// The result of an extraction: spans born `Inferred` + `Untrusted`, tagged with
/// the engine + version that produced them (the audit row's key fields, the design decision).
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractionResult {
    /// The engine that produced this (e.g. `"idp-e"`, `"pdf-text"`, `"vision"`).
    pub engine: String,
    /// The engine's version — part of what makes a read non-re-derivable across
    /// engines and what the audit row records.
    pub engine_version: String,
    /// The extracted spans, in reading order.
    pub spans: Vec<ExtractedSpan>,
}

impl ExtractionResult {
    /// Always `Untrusted` (the design decision, reuses v2.52.0/v2.54.0): an inferred read of a scan is
    /// as adversarial as the scan.
    pub const fn taint(&self) -> EpistemicTaint {
        EpistemicTaint::Untrusted
    }

    /// Always `Inferred`.
    pub const fn provenance(&self) -> IngestProvenance {
        IngestProvenance::Inferred
    }

    /// The mean confidence across spans — the `document:inferred` audit field
    ///. `0.0` for an empty read.
    pub fn mean_confidence(&self) -> f64 {
        if self.spans.is_empty() {
            return 0.0;
        }
        self.spans.iter().map(|s| s.confidence).sum::<f64>() / self.spans.len() as f64
    }

    /// The highest page index touched + 1 — the audit's page count.
    pub fn page_count(&self) -> u32 {
        self.spans.iter().map(|s| s.page).max().map_or(0, |m| m + 1)
    }
}

// ── Typed outcomes ─────────────────────────────────────────────────────

/// Everything that can go wrong extracting. Every variant is a typed outcome —
/// never a silent empty string, never a fall-through to a hallucinating model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractionError {
    /// No engine is wired for this transform. The OSS default: a flow
    /// calling an extraction tool with nothing behind it gets THIS, not fiction.
    NoEngineConfigured,
    /// The document exceeds the page cap.
    PageCapExceeded(u32),
    /// A page exceeds the pixel cap — a decompression bomb.
    PixelCapExceeded(u32),
    /// The engine emitted more spans than the cap allows.
    SpanCapExceeded(usize),
    /// The engine crashed or returned malformed output — typed + audited, never
    /// degraded to invented content.
    EngineFailed(String),
    /// The source bytes could not be decoded by the front-end.
    DecodeFailed(String),
}

impl std::fmt::Display for ExtractionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use ExtractionError::*;
        match self {
            NoEngineConfigured => write!(
                f,
                "extraction.no_engine_configured — no extraction engine is wired for this \
                 transform; refusing rather than inventing content"
            ),
            PageCapExceeded(n) => write!(f, "page_cap_exceeded — {n} pages (max {MAX_PAGES}) — refused"),
            PixelCapExceeded(n) => {
                write!(f, "pixel_cap_exceeded — {n} megapixels (max {MAX_MEGAPIXELS}) — refused")
            }
            SpanCapExceeded(n) => write!(f, "span_cap_exceeded — {n} spans (max {MAX_SPANS}) — refused"),
            EngineFailed(e) => write!(f, "extraction engine failed: {e} — typed refusal, never fabricated"),
            DecodeFailed(e) => write!(f, "decode_failed — {e}"),
        }
    }
}
impl std::error::Error for ExtractionError {}

impl ExtractionError {
    /// The stable slug (mirrors the section 6 error codes / the enterprise audit slug).
    pub fn slug(&self) -> &'static str {
        use ExtractionError::*;
        match self {
            NoEngineConfigured => "extraction.no_engine_configured",
            PageCapExceeded(_) => "page_cap_exceeded",
            PixelCapExceeded(_) => "pixel_cap_exceeded",
            SpanCapExceeded(_) => "span_cap_exceeded",
            EngineFailed(_) => "extraction.engine_failed",
            DecodeFailed(_) => "decode_failed",
        }
    }
}

// ── The confidence-floor gate ─────────────────────────────────

/// The disposition of an inferred span against a governing `anchor`'s
/// `confidence_floor`. A believed span is deliverable (still `Inferred`, still
/// capped at `believe`); a quarantined span routes to the anchor's
/// `on_violation` (`unknown_response`) and the v2.54.0 quarantine sink — it NEVER
/// reaches the agent's beliefs.
#[derive(Debug, Clone, PartialEq)]
pub enum SpanDisposition {
    /// Confidence ≥ floor: the span may be believed (behind a shield).
    Believed(ExtractedSpan),
    /// Confidence < floor: quarantined. Carries the span + the floor it missed,
    /// for the audit row, but the text does not flow onward.
    Quarantined { span: ExtractedSpan, floor: f64 },
}

impl SpanDisposition {
    pub fn is_believed(&self) -> bool {
        matches!(self, SpanDisposition::Believed(_))
    }
}

/// Apply a governing `anchor`'s `confidence_floor` to an extraction's spans.
/// Spans at or above the floor are `Believed`; those below are `Quarantined`
///. A floor outside `[0,1]` is clamped (the checker already range-checks
/// it, `type_checker.rs`, but the runtime is fail-closed regardless).
pub fn apply_confidence_floor(spans: &[ExtractedSpan], floor: f64) -> Vec<SpanDisposition> {
    let floor = if floor.is_nan() { 1.0 } else { floor.clamp(0.0, 1.0) };
    spans
        .iter()
        .cloned()
        .map(|span| {
            if span.confidence + f64::EPSILON >= floor {
                SpanDisposition::Believed(span)
            } else {
                SpanDisposition::Quarantined { span, floor }
            }
        })
        .collect()
}

/// The believed text of a floor-gated extraction, in reading order — what a
/// shield then scans and a `pix` navigator descends. Quarantined spans are
/// dropped from the flow (their text is audited, not delivered).
pub fn believed_text(dispositions: &[SpanDisposition]) -> String {
    dispositions
        .iter()
        .filter_map(|d| match d {
            SpanDisposition::Believed(s) => Some(s.text.as_str()),
            SpanDisposition::Quarantined { .. } => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ── The engine contract ─────────────────────────────────────────────────────────

/// The contract every extraction engine satisfies: `bytes + hint → spans`. The
/// deterministic IDP-E kernel (v2.54.0), the PDF text path, and the vision tier
/// (v2.54.0) all implement this. Output is ALWAYS born `Inferred` + `Untrusted`
/// with measured confidence — the trait's guarantee, upheld by construction
/// (every `ExtractedSpan` is `Inferred`, the design decision).
pub trait ExtractionEngine: Send + Sync {
    /// A stable engine identifier for the audit row (e.g. `"idp-e"`).
    fn name(&self) -> &str;
    /// The engine version — part of the read's non-re-derivability.
    fn version(&self) -> &str;
    /// Extract from raw bytes under a hint. Bounds are the caller's; the engine
    /// must honour them. Failure is typed, never fabricated.
    fn extract(
        &self,
        bytes: &[u8],
        hint: &ExtractionHint,
        bounds: &ExtractionBounds,
    ) -> Result<ExtractionResult, ExtractionError>;
}

/// The OSS default engine: **none**. It typed-refuses every extraction
/// the honest state of a runtime with no engine wired, and the exact
/// behaviour v2.54.0 made possible by replacing the dispatch fall-through. An
/// adopter mounts a real engine (the IDP-E kernel, a sidecar client) in its
/// place; until then, a flow calling `PDFExtractor` gets a refusal, never fiction.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoEngine;

impl ExtractionEngine for NoEngine {
    fn name(&self) -> &str {
        "none"
    }
    fn version(&self) -> &str {
        "0"
    }
    fn extract(
        &self,
        _bytes: &[u8],
        _hint: &ExtractionHint,
        _bounds: &ExtractionBounds,
    ) -> Result<ExtractionResult, ExtractionError> {
        Err(ExtractionError::NoEngineConfigured)
    }
}

// ── The engine registry (the injection seam, the design decision) ───────────────────────────

use std::sync::{Arc, OnceLock, RwLock};

fn registry() -> &'static RwLock<Option<Arc<dyn ExtractionEngine>>> {
    static REG: OnceLock<RwLock<Option<Arc<dyn ExtractionEngine>>>> = OnceLock::new();
    REG.get_or_init(|| RwLock::new(None))
}

/// Register the process-wide extraction engine. This is the seam the design decision keeps
/// open: OSS ships **no** engine (every extraction typed-refuses); the host —
/// the sidecar client (v2.54.0) or the enterprise engine (v2.54.0) — mounts a real
/// [`ExtractionEngine`] here. Replaces any prior registration.
pub fn register_engine(engine: Arc<dyn ExtractionEngine>) {
    *registry().write().expect("extraction registry poisoned") = Some(engine);
}

/// Clear the registered engine (back to the `NoEngine` typed refusal).
pub fn clear_engine() {
    *registry().write().expect("extraction registry poisoned") = None;
}

/// The active engine handle, if one is registered.
pub fn active_engine() -> Option<Arc<dyn ExtractionEngine>> {
    registry().read().expect("extraction registry poisoned").clone()
}

/// Run the active engine, or typed-refuse with [`ExtractionError::NoEngineConfigured`]
/// if none is registered. This is what the dispatch arms call.
pub fn run_active(
    bytes: &[u8],
    hint: &ExtractionHint,
    bounds: &ExtractionBounds,
) -> Result<ExtractionResult, ExtractionError> {
    match active_engine() {
        Some(engine) => engine.extract(bytes, hint, bounds),
        None => Err(ExtractionError::NoEngineConfigured),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises the registry-touching tests — the registry is process-global,
    /// so `register`/`clear` must not race across parallel test threads.
    static REG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn bbox() -> BBox {
        BBox { x: 0.1, y: 0.1, w: 0.2, h: 0.05 }
    }

    /// A deterministic mock engine for the registry + dispatch tests.
    #[derive(Debug)]
    struct MockEngine;
    impl ExtractionEngine for MockEngine {
        fn name(&self) -> &str {
            "mock"
        }
        fn version(&self) -> &str {
            "9"
        }
        fn extract(
            &self,
            _bytes: &[u8],
            hint: &ExtractionHint,
            _bounds: &ExtractionBounds,
        ) -> Result<ExtractionResult, ExtractionError> {
            let t = hint.transform.clone().unwrap_or_default();
            Ok(ExtractionResult {
                engine: "mock".into(),
                engine_version: "9".into(),
                spans: vec![ExtractedSpan::new(format!("read:{t}"), 0.9, 0, bbox())],
            })
        }
    }

    #[test]
    fn every_span_is_born_inferred_never_parsed() {
        // the design decision: the engine can only inhabit the `Inferred` class.
        let s = ExtractedSpan::new("1250.00", 0.94, 0, bbox());
        assert_eq!(s.provenance(), IngestProvenance::Inferred);
        assert_eq!(s.epistemic_ceiling(), "believe");
    }

    #[test]
    fn result_is_untrusted_and_inferred() {
        let r = ExtractionResult {
            engine: "idp-e".into(),
            engine_version: "1".into(),
            spans: vec![ExtractedSpan::new("x", 0.8, 0, bbox())],
        };
        assert_eq!(r.taint(), EpistemicTaint::Untrusted);
        assert_eq!(r.provenance(), IngestProvenance::Inferred);
    }

    #[test]
    fn confidence_is_clamped_and_nan_fails_closed() {
        assert_eq!(ExtractedSpan::new("a", 1.5, 0, bbox()).confidence, 1.0);
        assert_eq!(ExtractedSpan::new("a", -0.2, 0, bbox()).confidence, 0.0);
        // A NaN confidence must trip any non-zero floor (fail-closed).
        assert_eq!(ExtractedSpan::new("a", f64::NAN, 0, bbox()).confidence, 0.0);
    }

    #[test]
    fn floor_gate_quarantines_sub_floor_spans() {
        // the design decision: a span below the anchor floor is quarantined, never delivered.
        let spans = vec![
            ExtractedSpan::new("high", 0.96, 0, bbox()),
            ExtractedSpan::new("low", 0.42, 0, bbox()),
        ];
        let disp = apply_confidence_floor(&spans, 0.75);
        assert!(disp[0].is_believed());
        assert!(matches!(disp[1], SpanDisposition::Quarantined { floor, .. } if floor == 0.75));
        // Only the believed text flows onward.
        assert_eq!(believed_text(&disp), "high");
    }

    #[test]
    fn floor_boundary_is_inclusive() {
        // Exactly at the floor is believed (≥, not >).
        let spans = vec![ExtractedSpan::new("edge", 0.75, 0, bbox())];
        assert!(apply_confidence_floor(&spans, 0.75)[0].is_believed());
    }

    #[test]
    fn no_engine_typed_refuses_never_empty_string() {
        // the design decision: the fatal failure mode is a plausible empty/invented
        // read. NoEngine returns a TYPED refusal instead.
        let err = NoEngine.extract(b"whatever", &ExtractionHint::default(), &ExtractionBounds::default())
            .unwrap_err();
        assert_eq!(err, ExtractionError::NoEngineConfigured);
        assert_eq!(err.slug(), "extraction.no_engine_configured");
    }

    #[test]
    fn audit_fields_are_computed_not_reported() {
        let r = ExtractionResult {
            engine: "idp-e".into(),
            engine_version: "1".into(),
            spans: vec![
                ExtractedSpan::new("a", 0.9, 0, bbox()),
                ExtractedSpan::new("b", 0.7, 2, bbox()),
            ],
        };
        assert!((r.mean_confidence() - 0.8).abs() < 1e-9);
        assert_eq!(r.page_count(), 3); // highest page index 2 → 3 pages
    }

    #[test]
    fn bbox_validity_rejects_out_of_page() {
        assert!(bbox().is_valid());
        assert!(!BBox { x: 0.9, y: 0.0, w: 0.5, h: 0.1 }.is_valid());
        assert!(!BBox { x: -0.1, y: 0.0, w: 0.1, h: 0.1 }.is_valid());
    }

    #[test]
    fn error_slugs_are_stable() {
        assert_eq!(ExtractionError::PageCapExceeded(9000).slug(), "page_cap_exceeded");
        assert_eq!(ExtractionError::PixelCapExceeded(999).slug(), "pixel_cap_exceeded");
        assert_eq!(ExtractionError::EngineFailed("x".into()).slug(), "extraction.engine_failed");
    }

    #[test]
    fn registry_default_refuses_then_serves_a_registered_engine() {
        let _guard = REG_LOCK.lock().unwrap();
        // Default (nothing registered): typed refusal, never fiction.
        clear_engine();
        let hint = ExtractionHint { transform: Some("image:text".into()), ..Default::default() };
        assert_eq!(
            run_active(b"x", &hint, &ExtractionBounds::default()).unwrap_err(),
            ExtractionError::NoEngineConfigured
        );
        // Register an engine → dispatch reaches it, output born Inferred+Untrusted.
        register_engine(Arc::new(MockEngine));
        let out = run_active(b"x", &hint, &ExtractionBounds::default()).unwrap();
        assert_eq!(out.engine, "mock");
        assert_eq!(out.provenance(), IngestProvenance::Inferred);
        assert_eq!(out.taint(), EpistemicTaint::Untrusted);
        assert_eq!(out.spans[0].text, "read:image:text");
        // Clear → back to refusal.
        clear_engine();
        assert!(run_active(b"x", &hint, &ExtractionBounds::default()).is_err());
    }
}
