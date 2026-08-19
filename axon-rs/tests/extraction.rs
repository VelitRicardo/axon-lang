//! v2.54.0 — the Inferred-extraction dispatch path: PDFExtractor,
//! ImageTextExtractor (OCR), and ImageAnalyzer (vision) become REAL native
//! dispatch arms. With no engine registered they typed-refuse
//! (`extraction.no_engine_configured`), NEVER fall through to the model
//! inventing the document's contents. With an engine registered,
//! their output is born `Inferred` + `Untrusted` with measured confidence, capped
//! at `believe`.

use axon::extraction::{
    clear_engine, register_engine, ExtractedSpan, ExtractionBounds, ExtractionEngine,
    ExtractionError, ExtractionHint, ExtractionResult, BBox,
};
use axon::tool_executor::dispatch;
use base64::Engine;
use std::sync::Arc;

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// A deterministic mock engine that echoes the requested transform, so the test
/// can assert the arm routed correctly and the born-Inferred contract holds.
#[derive(Debug)]
struct MockEngine;
impl ExtractionEngine for MockEngine {
    fn name(&self) -> &str {
        "mock"
    }
    fn version(&self) -> &str {
        "1"
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
            engine_version: "1".into(),
            spans: vec![ExtractedSpan::new(
                format!("read({t})"),
                0.91,
                0,
                BBox { x: 0.1, y: 0.1, w: 0.3, h: 0.05 },
            )],
        })
    }
}

/// One test drives the whole registry lifecycle so the process-global engine is
/// never raced by parallel tests: refuse (default) → register → serve → clear.
#[test]
fn extraction_arms_refuse_then_serve_born_inferred() {
    // Default: no engine → every arm typed-refuses, never fabricates.
    clear_engine();
    let arg = serde_json::json!({ "bytes_base64": b64(b"%PDF-1.7 ...") }).to_string();
    for tool in ["PDFExtractor", "ImageTextExtractor", "ImageAnalyzer"] {
        let r = dispatch(tool, &arg).expect("native dispatch arm");
        assert!(!r.success, "{tool} must refuse with no engine");
        assert!(
            r.output.contains("extraction.no_engine_configured"),
            "{tool} → {}",
            r.output
        );
    }

    // Register an engine → each arm routes to it; output born Inferred + Untrusted,
    // capped at believe, with the correct transform (OCR vs vision distinct).
    register_engine(Arc::new(MockEngine));
    let expect = [
        ("PDFExtractor", "pdf:text"),
        ("ImageTextExtractor", "image:text"),
        ("ImageAnalyzer", "image:description"),
    ];
    for (tool, transform) in expect {
        let r = dispatch(tool, &arg).expect("native dispatch arm");
        assert!(r.success, "{tool} → {}", r.output);
        let v: serde_json::Value = serde_json::from_str(&r.output).unwrap();
        assert_eq!(v["transform"], transform, "{tool} routed to wrong transform");
        // The load-bearing contract:
        assert_eq!(v["provenance"], "inferred", "an inferred read is never parsed");
        assert_eq!(v["taint"], "untrusted");
        assert_eq!(v["epistemic_ceiling"], "believe", "never `know`");
        assert_eq!(v["engine"], "mock");
        assert!((v["mean_confidence"].as_f64().unwrap() - 0.91).abs() < 1e-9);
        assert_eq!(v["spans"][0]["text"], format!("read({transform})"));
    }

    // Clear → back to refusal.
    clear_engine();
    let r = dispatch("PDFExtractor", &arg).expect("native dispatch arm");
    assert!(!r.success);
    assert!(r.output.contains("extraction.no_engine_configured"));
}

#[test]
fn malformed_request_is_typed_refusal_never_silent() {
    // A missing bytes field is a typed refusal, not an empty read.
    let r = dispatch("PDFExtractor", "{}").expect("native dispatch arm");
    assert!(!r.success);
    assert!(r.output.contains("missing `bytes_base64`"), "output: {}", r.output);

    let r = dispatch("ImageTextExtractor", "not json").expect("native dispatch arm");
    assert!(!r.success);
    assert!(r.output.contains("invalid extraction request JSON"), "output: {}", r.output);
}
