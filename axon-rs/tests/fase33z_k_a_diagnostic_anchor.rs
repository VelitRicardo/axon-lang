//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 33.z.k.a — Diagnostic anchor for the wire-format adapter cycle.
//!
//! Captures the **current v1.27.1 axon-named-events wire surface** as
//! a forensic baseline. Every subsequent sub-fase (33.z.k.b through
//! 33.z.k.m) inverts a specific aspect of this anchor in lockstep —
//! same discipline as the 33.y.a + 33.z.a anchors that bookend their
//! cycles.
//!
//! # What this file pins
//!
//! 1. **§1 Canonical Step + stub backend → exactly 1 `axon.token` +
//!    1 `axon.complete`.** This is the D4 byte-compat anchor; it must
//!    stay GREEN across the entire cycle. When the openai or
//!    anthropic dialect lands (33.z.k.e / 33.z.k.f), this assertion
//!    only fires for the axon dialect (the others get their own
//!    parallel anchors).
//!
//! 2. **§2 Tool-streaming flow (algebraic effect) → axon-named
//!    wire surface today.** Post-33.z.k.1, the wire IS SSE
//!    (`Content-Type: text/event-stream`) for the Kivi-shape; the
//!    body emits W3C named events (`event: axon.token` + `data: {...}`).
//!    This is the **state to invert**: once 33.z.k.e lands with
//!    openai default for algebraic-effect flows, the body shifts to
//!    `data: {"chunk": "..."}` + `data: [DONE]` — this anchor's
//!    assertion REVERSES at that sub-fase. The capture here is the
//!    BEFORE picture.
//!
//! 3. **§3 Wire-format event catalog pinned at 4 axon-named events.**
//!    `axon.start` / `axon.token` / `axon.complete` / `axon.tool_call`
//!    are the closed catalog v1.27.1 emits. The dialect adapters
//!    introduce parallel catalogs — this pin remains the axon-
//!    dialect anchor.
//!
//! 4. **§4 D3 explicit opt-out grammar already accepts `sse` value.**
//!    Pre-33.z.k.b the grammar is bare `transport: sse`; this anchor
//!    captures the current parse behavior so the 33.z.k.b
//!    parametrized-grammar lift (`transport: sse(<dialect>)`) inverts
//!    in lockstep with this anchor.
//!
//! # Diagnostic discipline
//!
//! The anchor file uses `eprintln!` to surface observed wire shapes
//! into `cargo test -- --nocapture` output. Assertions are minimal +
//! defensive — the goal is FORENSIC CAPTURE of v1.27.1 behavior, not
//! gate enforcement. The cycle's actual gates land per-sub-fase as
//! their handlers ship.

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
        "source_file": "anchor.axon",
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

fn count_event_lines(body: &str, event_name: &str) -> usize {
    let needle = format!("event: {event_name}");
    body.lines().filter(|l| l.trim_start() == needle).count()
}

// ─── §1 — Canonical Step + stub → 1 axon.token + 1 axon.complete ────

#[tokio::test]
async fn s1_canonical_step_stub_emits_one_token_one_complete() {
    let src = "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
    }\n\
    axonendpoint ChatEndpoint { public: true method: POST path: \"/canonical\" execute: Chat transport: sse }";

    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);
    let (status, ct, body) = post_no_accept(app, "/canonical").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.contains("text/event-stream"));

    let tokens = count_event_lines(&body, "axon.token");
    let completes = count_event_lines(&body, "axon.complete");

    eprintln!(
        "§1 anchor (canonical Step + stub, axon dialect baseline):\n\
         Content-Type: {ct}\n\
         axon.token count = {tokens} (expected 1)\n\
         axon.complete count = {completes} (expected 1)\n\
         body sample:\n{}",
        body.chars().take(500).collect::<String>()
    );

    assert_eq!(tokens, 1, "§1 D4 anchor: exactly 1 axon.token for stub");
    assert_eq!(completes, 1, "§1 D4 anchor: exactly 1 axon.complete");
}

// ─── §2 — Algebraic-effect flow → OpenAI wire surface (Q1 ratification) ─────
//
// **Inverted post-33.z.k.g.2** — the consumer-loop refactor + dialect
// adapter wiring closes the cycle's core promise. Algebraic-effect
// flows (disjunct b: tool with `effects: <stream:<policy>>`) default
// to the openai dialect per the founder's Q1 ratification: the LLM-
// streaming ecosystem's adopter SDKs (litellm, langchain, vercel-ai,
// instructor) parse the openai wire verbatim, so axon delivers it.
// Type-annotation-only flows (disjunct a) still default to the axon
// dialect (W3C named events) — see §1.
//
// Q5 escape valve preserved: an adopter who wants the W3C named-event
// dialect on an algebraic-effect flow declares
// `transport: sse(axon)` explicitly. That path is exercised in the
// 33.z.k.g E2E test pack.

#[tokio::test]
async fn s2_algebraic_effect_emits_openai_wire_post_33_z_k_g_2() {
    let src = "tool chat_token_stream { description: \"S\" effects: <stream:drop_oldest> }\n\
        flow Chat() -> Unit {\n\
            step Generate { ask: \"hi\" apply: chat_token_stream output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatEndpoint { public: true method: POST path: \"/algebraic\" execute: Chat }";

    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);
    let (status, ct, body) = post_no_accept(app, "/algebraic").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        ct.contains("text/event-stream"),
        "§2 post-33.z.k.1 anchor: algebraic-effect flow gets SSE wire"
    );

    let axon_tokens = count_event_lines(&body, "axon.token");
    let axon_completes = count_event_lines(&body, "axon.complete");
    let has_done_sentinel = body.contains("data: [DONE]");
    let has_chat_completion_chunk = body.contains("\"object\":\"chat.completion.chunk\"");
    let has_role_assistant = body.contains("\"delta\":{\"role\":\"assistant\"}");
    let has_finish_reason_stop = body.contains("\"finish_reason\":\"stop\"");

    eprintln!(
        "§2 anchor (algebraic-effect flow, POST-33.z.k.g.2 OpenAI dialect):\n\
         Content-Type: {ct}\n\
         axon.token count = {axon_tokens} (expected 0 — openai wire)\n\
         axon.complete count = {axon_completes} (expected 0 — openai wire)\n\
         has `data: [DONE]` sentinel = {has_done_sentinel} (expected true)\n\
         has `chat.completion.chunk` object = {has_chat_completion_chunk} (expected true)\n\
         has `delta.role: assistant` marker = {has_role_assistant} (expected true)\n\
         has `finish_reason: stop` = {has_finish_reason_stop} (expected true)\n\
         body sample:\n{}",
        body.chars().take(500).collect::<String>()
    );

    // POST-33.z.k.g.2 (Q1 ratification): algebraic-effect flows
    // default to the openai dialect. W3C named axon.* events are
    // structurally absent; the wire is OpenAI Chat Completions
    // streaming verbatim.
    assert_eq!(
        axon_tokens, 0,
        "§2 POST-33.z.k.g.2: axon.token MUST NOT appear on openai-dialect wire"
    );
    assert_eq!(
        axon_completes, 0,
        "§2 POST-33.z.k.g.2: axon.complete MUST NOT appear on openai-dialect wire"
    );
    assert!(
        has_done_sentinel,
        "§2 POST-33.z.k.g.2: openai-style `data: [DONE]` sentinel MUST terminate the stream"
    );
    assert!(
        has_chat_completion_chunk,
        "§2 POST-33.z.k.g.2: every chunk frame MUST carry `object: \"chat.completion.chunk\"` \
         per OpenAI Chat Completions streaming spec"
    );
    assert!(
        has_role_assistant,
        "§2 POST-33.z.k.g.2: first chunk MUST emit role-marker `delta: {{\"role\": \"assistant\"}}` \
         per OpenAI Chat Completions streaming spec"
    );
    assert!(
        has_finish_reason_stop,
        "§2 POST-33.z.k.g.2: final chunk MUST carry `finish_reason: \"stop\"` per OpenAI spec"
    );
}

// ─── §3 — Wire-format event catalog pinned at 4 axon-named events ───

#[tokio::test]
async fn s3_axon_named_event_catalog_pinned_at_four_events() {
    // Closed catalog of axon-named SSE events emitted by the current
    // producer. Future dialects introduce parallel catalogs — this
    // pin remains the axon-dialect anchor.
    const AXON_EVENTS: &[&str] = &[
        "axon.start",
        "axon.token",
        "axon.complete",
        "axon.tool_call",
    ];
    assert_eq!(
        AXON_EVENTS.len(),
        4,
        "§3 33.z.k.a anchor: axon dialect closed catalog = 4 events; \
         adding a 5th requires a deliberate sub-fase + cross-stack \
         drift gate update"
    );
    eprintln!("§3 anchor: axon dialect catalog = {AXON_EVENTS:?}");
}

// ─── §4 — D3 grammar accepts bare `transport: sse` (pre-33.z.k.b) ───

#[tokio::test]
async fn s4_bare_transport_sse_grammar_accepts_pre_33_z_k_b() {
    // Pre-33.z.k.b the only accepted forms are bare `transport: sse`
    // / `transport: json` / `transport: ndjson`. Post-33.z.k.b the
    // grammar extends to `transport: sse(<dialect>)`. This anchor
    // captures the pre-state so the parser-grammar lift is observable
    // as a single change (33.z.k.b) rather than an unsignaled drift.
    let src = "flow F() -> Unit { step S { ask: \"x\" output: Stream<Token> } }\n\
        axonendpoint E { public: true method: POST path: \"/grammar\" execute: F transport: sse }";

    let app = build_router(server_cfg());
    let status = deploy(app.clone(), src).await;
    eprintln!(
        "§4 anchor (bare `transport: sse` grammar, PRE-33.z.k.b):\n\
         deploy status = {status:?} (expected OK)"
    );
    assert_eq!(status, StatusCode::OK);

    // Future 33.z.k.b cell: `transport: sse(openai)` will parse cleanly
    // post-grammar-extension. Today it would fail (no dialect token in
    // the parser's transport-value path). We do NOT exercise that
    // negative case here — the diagnostic anchor's role is forensic
    // capture of accepted shapes only.
}
