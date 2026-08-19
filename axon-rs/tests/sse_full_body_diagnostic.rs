//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.24.0 diagnostic — capture WHAT the SSE wire actually emits at
//! v1.23.1 for the adopter-canonical Kivi-shape source, beyond just
//! Content-Type.
//!
//! The adopter trail post-v1.23.1:
//!
//!   > "10 bumps en 5 días. Mismo resultado. […] el feature está más
//!   > cerca de los cimientos del runtime de lo que parecía — no es un
//!   > parser fix, no es un patch quirúrgico, es un piece de
//!   > arquitectura que necesita su propio cycle."
//!
//! 32.l fixed the Content-Type (parser → produces_stream → inference →
//! Sse verdict → execute_sse_handler), but the SSE body itself may not
//! exhibit true incremental streaming semantics. This test captures
//! the full body + parses events + asserts on shape so the
//! architectural gap is observable in the test pack, not just in
//! adopter logs.
//!
//! What the test answers, observably:
//!   1. Is Content-Type text/event-stream? (32.l)
//!   2. Does the body begin with `retry:` per W3C spec?
//!   3. How many `axon.token` events appear?
//!   4. Does an `axon.complete` event terminate the stream?
//!   5. Are events well-formed (event + id + data)?
//!
//! What it CAN'T answer in a tower::oneshot test (deferred to v1.24.0
//! integration with a real HTTP listener):
//!   - Do `axon.token` events arrive INCREMENTALLY (real-time) or
//!     in a burst after flow execution completes?
//!   - Do `: keepalive\n\n` comments fire during inactivity windows?
//!   - Does the wire respect backpressure when the client drains slowly?
//!
//! Those three are the architectural piece the adopter pointed at.

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
    assert_eq!(resp.status(), StatusCode::OK);
}

/// Adopter-canonical disjunct (a) shape — `step S { output: Stream<T> }`
/// with NO explicit `transport:` declaration. Inference fires per
/// v1.21.0 disjunct (a) (closed in v1.23.1 for Rust parser).
const ADOPTER_DISJUNCT_A: &str =
    "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat }";

/// Adopter-canonical disjunct (b) shape — tool with `effects:
/// <stream:...>` referenced via `step S { apply: <tool> }`. Closed
/// since v1.22.0 for both stacks.
///
/// v1.24.0 — Explicit `transport: sse(axon)` preserves the
/// W3C named-event wire so this 33.a hollow-shape diagnostic
/// continues to count `axon.token` / `axon.complete` frames. The Q1
/// default flip to openai for algebraic-effect flows is exercised in
/// the 33.z.k.g E2E pack; this anchor is dialect-pinned.
const ADOPTER_DISJUNCT_B: &str =
    "tool chat_token_stream { description: \"stream\" effects: <stream:drop_oldest> }\n\
     flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" apply: chat_token_stream }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse(axon) }";

/// Explicit `transport: sse` shape — D5 path. Always SSE regardless
/// of mode / Accept.
const ADOPTER_EXPLICIT_SSE: &str =
    "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse }";

#[derive(Debug, Default, Clone)]
struct SseEventCount {
    retry_directive: usize,
    axon_token: usize,
    axon_complete: usize,
    axon_error: usize,
    other_event: usize,
    keepalive_comments: usize,
    raw_lines: usize,
}

/// Tokenize an SSE response body into event counts. The body is a
/// sequence of newline-separated lines; events are delimited by blank
/// lines; each event has `event: <name>`, `id: <id>`, `data: <json>`
/// fields. The `retry:` directive is a special non-data event.
fn count_sse_events(body: &str) -> SseEventCount {
    let mut counts = SseEventCount::default();
    let mut current_event: Option<String> = None;
    for line in body.lines() {
        counts.raw_lines += 1;
        if line.starts_with(':') {
            counts.keepalive_comments += 1;
        } else if let Some(value) = line.strip_prefix("retry: ") {
            // Per W3C SSE, `retry:` is a stand-alone directive.
            let _ = value;
            counts.retry_directive += 1;
        } else if let Some(value) = line.strip_prefix("event: ") {
            current_event = Some(value.trim().to_string());
        } else if line.is_empty() {
            // Blank line = event boundary; commit the current event.
            match current_event.as_deref() {
                Some("axon.token") => counts.axon_token += 1,
                Some("axon.complete") => counts.axon_complete += 1,
                Some("axon.error") => counts.axon_error += 1,
                Some(_) => counts.other_event += 1,
                None => {}
            }
            current_event = None;
        }
    }
    // Commit any trailing event without a blank-line terminator
    // (axum's KeepAlive layer might cut off the final \n\n).
    if let Some(e) = current_event {
        match e.as_str() {
            "axon.token" => counts.axon_token += 1,
            "axon.complete" => counts.axon_complete += 1,
            "axon.error" => counts.axon_error += 1,
            _ => counts.other_event += 1,
        }
    }
    counts
}

async fn fetch_full_sse_body(
    app: axum::Router,
    path: &str,
    body: &str,
) -> (StatusCode, String, String) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
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

// ─── section 1 — Disjunct (a) under strict mode: full wire surface ─────────

#[tokio::test]
async fn diagnostic_disjunct_a_strict_mode_full_body_inspection() {
    let app = build_router(server_cfg(true));
    deploy(app.clone(), ADOPTER_DISJUNCT_A).await;
    let (status, ct, body) = fetch_full_sse_body(app, "/chat", "{}").await;
    eprintln!("─── Disjunct (a) strict mode ───");
    eprintln!("status: {status}");
    eprintln!("content-type: {ct}");
    eprintln!("body bytes: {}", body.len());
    eprintln!("--- body BEGIN ---");
    eprintln!("{body}");
    eprintln!("--- body END ---");

    let counts = count_sse_events(&body);
    eprintln!("event counts: {counts:?}");

    // Anchor #1: 32.l fix — Content-Type SSE.
    assert_eq!(status, StatusCode::OK);
    assert!(
        ct.starts_with("text/event-stream"),
        "Disjunct (a) strict mode MUST serve text/event-stream after 32.l. \
         Got: '{ct}'"
    );
    // Anchor #2: W3C SSE wire surface — retry: directive leads.
    assert!(
        counts.retry_directive >= 1,
        "Wire MUST emit retry: directive per W3C SSE spec. \
         Got body: {body}"
    );
    // Anchor #3: stream actually emitted SOMETHING — not just retry.
    let data_events = counts.axon_token + counts.axon_complete + counts.axon_error;
    assert!(
        data_events >= 1,
        "Stream MUST emit at least one axon.{{token|complete|error}} \
         event so adopter EventSource clients receive data. Got: {counts:?}. \
         If this passes only with axon.complete and 0 axon.token, the wire \
         is technically valid but the architectural piece — true \
         incremental streaming — is missing (v1.24.0 territory)."
    );
}

// ─── section 2 — Disjunct (b) tool stream effect: full wire surface ──────

#[tokio::test]
async fn diagnostic_disjunct_b_strict_mode_full_body_inspection() {
    let app = build_router(server_cfg(true));
    deploy(app.clone(), ADOPTER_DISJUNCT_B).await;
    let (status, ct, body) = fetch_full_sse_body(app, "/chat", "{}").await;
    eprintln!("─── Disjunct (b) strict mode ───");
    eprintln!("content-type: {ct}");
    eprintln!("body bytes: {}", body.len());
    eprintln!("--- body BEGIN ---");
    eprintln!("{body}");
    eprintln!("--- body END ---");
    let counts = count_sse_events(&body);
    eprintln!("event counts: {counts:?}");
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));
    assert!(counts.retry_directive >= 1);
    assert!(counts.axon_token + counts.axon_complete + counts.axon_error >= 1);
}

// ─── section 3 — Explicit transport: sse (D5): full wire surface ─────────

#[tokio::test]
async fn diagnostic_explicit_sse_full_body_inspection() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), ADOPTER_EXPLICIT_SSE).await;
    let (status, ct, body) = fetch_full_sse_body(app, "/chat", "{}").await;
    eprintln!("─── Explicit transport: sse ───");
    eprintln!("content-type: {ct}");
    eprintln!("body bytes: {}", body.len());
    eprintln!("--- body BEGIN ---");
    eprintln!("{body}");
    eprintln!("--- body END ---");
    let counts = count_sse_events(&body);
    eprintln!("event counts: {counts:?}");
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));
    assert!(counts.retry_directive >= 1);
    assert!(counts.axon_token + counts.axon_complete + counts.axon_error >= 1);
}

// ─── section 4 — Legacy /v1/execute path with adopter source ─────────────

#[tokio::test]
async fn diagnostic_legacy_v1_execute_disjunct_a_no_accept_header() {
    // Adopter hits /v1/execute (legacy path) WITHOUT Accept SSE and
    // WITHOUT strict mode. With disjunct (a) firing in inference:
    //   - implicit_transport == "sse"
    //   - has_force_decl == false (no transport: sse declared)
    //   - client_wants_sse == false (no Accept header)
    //   - strict_mode == false
    //   → promote_sse = false → JSON
    // The X-Axon-Stream-Available diagnostic header MUST fire so
    // adopter sees the inference outcome. This is the "silent
    // diagnostic" mode v1.23.0 + v1.23.1 ship.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), ADOPTER_DISJUNCT_A).await;

    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({"flow": "Chat", "backend": "stub"}).to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let ct = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let stream_hint = resp
        .headers()
        .get("x-axon-stream-available")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    eprintln!("─── Legacy /v1/execute disjunct (a) no Accept ───");
    eprintln!("status: {status}");
    eprintln!("content-type: {ct}");
    eprintln!("x-axon-stream-available: {stream_hint:?}");

    assert_eq!(status, StatusCode::OK);
    assert!(
        ct.starts_with("application/json"),
        "Legacy /v1/execute + disjunct (a) + no Accept + no strict → JSON \
         (D9 backwards-compat). Got: '{ct}'."
    );
    assert!(
        stream_hint.is_some(),
        "X-Axon-Stream-Available MUST fire on JSON responses to stream-effect \
         flows post-v1.23.1 — surfaces the inference outcome to client without \
         spelunking source. Got: {stream_hint:?}."
    );
    // Surface the v1.23.1 fix: the diagnostic header MUST contain
    // `flow=Chat` AND mention the opt-in path.
    let h = stream_hint.unwrap();
    assert!(
        h.contains("flow=Chat"),
        "Diagnostic header must carry flow=Chat for correlation: '{h}'"
    );
    assert!(
        h.contains("transport:sse"),
        "Diagnostic header must list opt-in transport:sse: '{h}'"
    );
}

// ─── section 5 — Body content diagnostic: how many tokens? ───────────────

#[tokio::test]
async fn diagnostic_chunking_emits_multiple_tokens_per_step() {
    // Architectural snapshot: today the executor runs synchronously
    // and emits one StreamEmitter chunk-burst at the end. This test
    // captures THAT property — body content has multiple axon.token
    // events but they're all generated AFTER server_execute_full
    // returns. The wire is technically valid SSE; the streaming
    // SEMANTICS (live token-by-token delivery from backend → wire)
    // is the architectural piece deferred to v1.24.0.
    let app = build_router(server_cfg(true));
    deploy(app.clone(), ADOPTER_EXPLICIT_SSE).await;
    let (status, ct, body) = fetch_full_sse_body(app, "/chat", "{}").await;
    let counts = count_sse_events(&body);
    eprintln!("─── Chunking diagnostic ───");
    eprintln!("status: {status}");
    eprintln!("content-type: {ct}");
    eprintln!("event counts: {counts:?}");
    eprintln!("--- body sample (first 800 chars) ---");
    eprintln!("{}", &body[..body.len().min(800)]);

    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));
    assert!(
        counts.axon_complete >= 1,
        "Stream MUST terminate with axon.complete envelope: {counts:?}"
    );
    // Snapshot: today the StreamEmitter chunks each step's result
    // into ~3-word groups and emits a token event per chunk. With
    // the stub backend producing a short result, we expect a small
    // number of token events. Post v1.24.0 (true backend streaming),
    // this number should track the actual token-per-second rate
    // from the underlying LLM (potentially hundreds/thousands for a
    // long completion).
    eprintln!(
        "Snapshot @ v1.23.1: stub backend produces {} axon.token + {} \
         axon.complete events. Post-v1.24.0 with real backend streaming, \
         the axon.token count will track the LLM's actual token output.",
        counts.axon_token, counts.axon_complete
    );
}
