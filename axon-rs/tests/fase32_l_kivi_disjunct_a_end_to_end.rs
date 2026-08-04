//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 32.l — End-to-end "last cable" test for adopter `MIGRATION_TO_AXON.md`.
//!
//! The adopter trail (2026-05-12) reported that even at v1.23.0 a flow
//! using disjunct (a) of the stream-effect predicate — `step S { output:
//! Stream<T> }` inside the flow body — STILL served `application/json`
//! on the declared dynamic route instead of `text/event-stream`.
//!
//! Root cause (closed in 32.l): the Rust frontend `parse_step` consumed
//! only the bare `Identifier` head of `output:`, dropping the `<T>`
//! generic payload. `type_checker::flow_has_stream_output`'s
//! `starts_with("Stream<") && ends_with('>')` predicate then returned
//! false → `implicit_transport == "json"` → `classify_dynamic_route_wire`
//! returned `Json` → the dispatch path served the legacy JSON envelope.
//!
//! This test deploys the EXACT adopter-canonical shape (per
//! `docs/STREAM_EFFECTS.md`) and asserts the dynamic-route response
//! Content-Type is `text/event-stream`. Pre-32.l this test failed.
//!
//! Pillar trace per D12: the test exercises all four pillars in one
//! contract:
//!   - **MATHEMATICS** — the inference function `produces_stream` is
//!     now total over disjunct (a) inputs.
//!   - **LOGIC** — declarative source IS the wire format.
//!   - **PHILOSOPHY** — adopters never diagnose our bugs; the wire
//!     layer honors the declaration.
//!   - **COMPUTING** — D11 cross-stack parity restored (Python parser
//!     already handled this since 2026-05-09).

use axon::axon_server::{build_router, ServerConfig};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn server_cfg(strict: bool) -> ServerConfig {
    ServerConfig {
        host: "127.0.0.1".into(),
        port: 0,
        channel: "memory".into(),
        auth_token: String::new(),
        log_level: "INFO".into(),
        log_format: "json".into(),
        log_file: None,
        database_url: None,
        config_path: None,
        strict_type_driven_transport: strict,
        default_backend: None,
        schemas_dir: None,
    }
}

async fn deploy(app: axum::Router, src: &str) {
    let body = serde_json::json!({
        "source": src,
        "source_file": "kivi.axon",
        "backend": "stub",
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(status, StatusCode::OK, "deploy failed: {json}");
    assert_eq!(
        json.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "deploy success=false: {json}"
    );
}

/// Adopter-canonical disjunct (a) shape per `docs/STREAM_EFFECTS.md`:
/// a flow whose step body declares `output: Stream<Token>` — NO
/// explicit `transport: sse` on the axonendpoint.
const KIVI_DISJUNCT_A_SOURCE: &str =
    "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat }";

// ─── Strict mode: D1 inference fires → SSE ──────────────────────────

#[tokio::test]
async fn kivi_disjunct_a_strict_mode_returns_sse() {
    // Strict mode flips Fase 31 D1 on: `produces_stream` true →
    // `implicit_transport = sse` → dynamic route serves SSE regardless
    // of `Accept:`. Pre-32.l this returned application/json because
    // the AST never saw the Stream<Token>.
    let app = build_router(server_cfg(true));
    deploy(app.clone(), KIVI_DISJUNCT_A_SOURCE).await;
    let req = Request::builder()
        .method("POST")
        .uri("/chat")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.starts_with("text/event-stream"),
        "Kivi disjunct (a) under strict mode: expected text/event-stream, got '{ct}'. \
         If this fails, the Rust parser is dropping `<T>` from `output: Stream<T>` again."
    );
}

// ─── Legacy mode + Accept SSE: D4 fallback fires → SSE ──────────────

#[tokio::test]
async fn kivi_disjunct_a_legacy_with_accept_returns_sse() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), KIVI_DISJUNCT_A_SOURCE).await;
    let req = Request::builder()
        .method("POST")
        .uri("/chat")
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.starts_with("text/event-stream"),
        "Kivi disjunct (a) + Accept SSE: expected text/event-stream, got '{ct}'."
    );
}

// ─── Legacy mode + no Accept: D9 backwards-compat → JSON ────────────

#[tokio::test]
async fn kivi_disjunct_a_legacy_no_accept_stays_json_d9() {
    // D9 backwards-compat: legacy mode + no explicit declaration +
    // no Accept SSE header → JSON. This is the v1.21.x-and-earlier
    // behavior that strict mode (off by default) preserves.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), KIVI_DISJUNCT_A_SOURCE).await;
    let req = Request::builder()
        .method("POST")
        .uri("/chat")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.starts_with("application/json"),
        "Kivi disjunct (a) legacy mode + no Accept: expected application/json (D9), got '{ct}'."
    );
    // The Fase 31.e diagnostic header MUST fire — adopter sees the
    // inference outcome from their REST client without spelunking
    // source. Pre-32.l this header was ABSENT (because the inference
    // never fired) — that's the silent failure the adopter trail
    // surfaced.
    let stream_hint = resp.headers().get("x-axon-stream-available");
    assert!(
        stream_hint.is_some(),
        "X-Axon-Stream-Available MUST fire on JSON responses to stream-effect flows; \
         pre-32.l it didn't because the inference never saw the Stream<Token> in the \
         step body — that silent absence was the adopter's diagnostic wall."
    );
}

// ─── Combined: explicit transport: sse always wins (D5) ────────────

const KIVI_EXPLICIT_SSE_SOURCE: &str =
    "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse }";

#[tokio::test]
async fn kivi_explicit_sse_always_serves_sse_even_legacy_mode() {
    // Belt + suspenders: even if the parser fix regressed somehow,
    // explicit `transport: sse` (D5) should still serve SSE. This
    // anchors the D5 path against accidental coupling with D1
    // inference.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), KIVI_EXPLICIT_SSE_SOURCE).await;
    let req = Request::builder()
        .method("POST")
        .uri("/chat")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let ct = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(ct.starts_with("text/event-stream"));
}
