//! §Fase 33.z.k.g.2 (v1.28.0) — End-to-end Kimi + GLM dispatch.
//!
//! Per the Q3 revision 2026-05-14, the closed-catalog dialect set
//! expanded 3 → 5: `{axon, openai, kimi, glm, anthropic}`. Both Kimi
//! (Moonshot's K2.x series) and GLM (Zhipu's ChatGLM-4.x) publish
//! Chat Completions APIs that mirror OpenAI's streaming wire verbatim
//! — same `data: {"choices": [...]}` chunk shape, same `data: [DONE]`
//! terminator. The project's primary adopter pipelines route through
//! these providers, so first-class catalog entries let adopters
//! declare intent precisely:
//!
//! ```axon
//! axonendpoint Endpoint { transport: sse(kimi) ... }
//! axonendpoint Endpoint { transport: sse(glm) ... }
//! ```
//!
//! At the wire layer both dialects dispatch to
//! `OpenAIDialectAdapter` (the bytes are canonical-OpenAI). The
//! intent declaration is preserved in the AST + observable on
//! downstream audit / replay / metrics surfaces, but the wire is
//! IDENTICAL to `transport: sse(openai)` — Kimi/GLM SDKs parse it
//! verbatim because they're OpenAI-compat.
//!
//! # What this pack pins
//!
//! 1. **§1 — `transport: sse(kimi)` emits canonical OpenAI wire**.
//! 2. **§2 — `transport: sse(glm)` emits canonical OpenAI wire**.
//! 3. **§3 — kimi + glm + openai all dispatch to the same adapter**.
//!    The three routes' wire bodies are byte-identical modulo volatile
//!    fields.

use axon::axon_server::{build_router, ServerConfig};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn server_cfg() -> ServerConfig {
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
        strict_type_driven_transport: false,
        default_backend: None,
        schemas_dir: None,
    }
}

async fn deploy(app: axum::Router, src: &str) -> StatusCode {
    let body = serde_json::json!({
        "source": src,
        "source_file": "kimi_glm.axon",
        "backend": "stub",
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    app.oneshot(req).await.unwrap().status()
}

async fn post_no_accept(app: axum::Router, path: &str) -> (StatusCode, String, String) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let ct = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes).to_string();
    (status, ct, body)
}

/// Strip volatile fields so byte-equivalence assertions compare
/// wire shape, not wall-clock or per-request noise.
///
/// Stripped fields (per dispatch comparison):
///   - Top-level chunk: `created` (Unix-seconds timestamp varies per
///     adapter construction), `id` (synthesized from trace_id but
///     adapter constructions race the timestamp).
///   - §Fase 33.z.k.h `axon_metadata`: `latency_ms` (wall-clock
///     accumulator from request start).
///   - §Fase 33.x.f `step_audit[*]`: `timestamp_ms` (per-step
///     wall-clock).
fn strip_volatile_openai(body: &str) -> String {
    let mut out = String::new();
    for line in body.lines() {
        if let Some(data_pos) = line.find("data: {") {
            let prefix = &line[..data_pos + 6];
            let payload = &line[data_pos + 6..];
            if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(payload) {
                strip_volatile_in_place(&mut v);
                out.push_str(prefix);
                out.push_str(&serde_json::to_string(&v).unwrap_or_default());
            } else {
                out.push_str(line);
            }
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

fn strip_volatile_in_place(v: &mut serde_json::Value) {
    if let Some(obj) = v.as_object_mut() {
        obj.remove("created");
        obj.remove("id");
        // §Fase 33.z.k.h — axon_metadata.latency_ms varies per request.
        if let Some(meta) = obj.get_mut("axon_metadata").and_then(|m| m.as_object_mut()) {
            meta.remove("latency_ms");
            if let Some(audit) = meta.get_mut("step_audit").and_then(|a| a.as_array_mut()) {
                for entry in audit.iter_mut() {
                    if let Some(eo) = entry.as_object_mut() {
                        eo.remove("timestamp_ms");
                    }
                }
            }
        }
    }
}

// ─── §1 — `transport: sse(kimi)` emits canonical OpenAI wire ───────

#[tokio::test]
async fn explicit_sse_kimi_emits_canonical_openai_wire() {
    let src = "flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/kimi\" execute: Chat transport: sse(kimi) }";
    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);
    let (status, ct, body) = post_no_accept(app, "/kimi").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.contains("text/event-stream"));
    // The wire is OpenAI-compatible (Moonshot's published Kimi API
    // mirrors OpenAI Chat Completions streaming verbatim).
    assert!(
        body.contains("\"object\":\"chat.completion.chunk\""),
        "Kimi dialect: wire MUST be OpenAI-compatible chunks. Body: {body:?}"
    );
    assert!(
        body.contains("\"delta\":{\"role\":\"assistant\"}"),
        "Kimi dialect: role-marker on first chunk per OpenAI spec. \
         Body: {body:?}"
    );
    assert!(
        body.contains("data: [DONE]"),
        "Kimi dialect: stream MUST terminate with `[DONE]`. Body: {body:?}"
    );
}

// ─── §2 — `transport: sse(glm)` emits canonical OpenAI wire ────────

#[tokio::test]
async fn explicit_sse_glm_emits_canonical_openai_wire() {
    let src = "flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/glm\" execute: Chat transport: sse(glm) }";
    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);
    let (status, ct, body) = post_no_accept(app, "/glm").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.contains("text/event-stream"));
    assert!(
        body.contains("\"object\":\"chat.completion.chunk\""),
        "GLM dialect: wire MUST be OpenAI-compatible chunks. Body: {body:?}"
    );
    assert!(
        body.contains("\"delta\":{\"role\":\"assistant\"}"),
        "GLM dialect: role-marker on first chunk per OpenAI spec. \
         Body: {body:?}"
    );
    assert!(
        body.contains("data: [DONE]"),
        "GLM dialect: stream MUST terminate with `[DONE]`. Body: {body:?}"
    );
}

// ─── §3 — kimi + glm + openai share the same adapter (byte-equiv) ─

#[tokio::test]
async fn kimi_glm_openai_dialects_dispatch_to_same_adapter() {
    // Three routes, three dialect declarations, ONE adapter
    // (OpenAIDialectAdapter). Wire bodies MUST be byte-identical
    // modulo volatile created/id fields.
    let src_openai = "flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/d\" execute: Chat transport: sse(openai) }";
    let src_kimi = "flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/d\" execute: Chat transport: sse(kimi) }";
    let src_glm = "flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/d\" execute: Chat transport: sse(glm) }";

    let bodies: Vec<String> = {
        let mut out = Vec::new();
        for src in [src_openai, src_kimi, src_glm] {
            let app = build_router(server_cfg());
            assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);
            let (_, _, body) = post_no_accept(app, "/d").await;
            out.push(strip_volatile_openai(&body));
        }
        out
    };

    assert_eq!(
        bodies[0], bodies[1],
        "Q3 revision: kimi dialect MUST dispatch to OpenAIDialectAdapter — \
         wire bytes byte-identical to canonical openai dialect.\n\
         openai:\n{}\n\
         kimi:\n{}",
        bodies[0], bodies[1]
    );
    assert_eq!(
        bodies[0], bodies[2],
        "Q3 revision: glm dialect MUST dispatch to OpenAIDialectAdapter — \
         wire bytes byte-identical to canonical openai dialect.\n\
         openai:\n{}\n\
         glm:\n{}",
        bodies[0], bodies[2]
    );
}
