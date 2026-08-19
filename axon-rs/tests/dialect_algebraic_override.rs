//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.27.1 — Algebraic-effect override on dynamic
//! routes. Integration tests for the Kivi-shape adopter case.
//!
//! # The bug the override closes
//!
//! Pre-33.z.k.1, an adopter who wrote:
//!
//! ```axon
//! tool chat_token_stream {
//!     description: "Stream tokens"
//!     effects: <stream:drop_oldest>
//! }
//!
//! flow Chat() -> Unit {
//!     step Generate {
//!         ask: "..."
//!         apply: chat_token_stream
//!         output: Stream<Token>
//!     }
//! }
//!
//! axonendpoint ChatEndpoint { public: true
//!     method: POST
//!     path: "/chat"
//!     execute: Chat
//! }
//! ```
//!
//! and hit `POST /chat` with `Content-Type: application/json` (no
//! `Accept: text/event-stream` header) against a server WITHOUT
//! `AXON_STRICT_TYPE_DRIVEN_TRANSPORT=1` got back:
//!
//! ```
//! HTTP/1.1 200 OK
//! Content-Type: application/json
//! X-Axon-Stream-Available: 1; reason=flag_off; flow=Chat; ...
//!
//! { "report": ..., "warnings": [...] }
//! ```
//!
//! The `X-Axon-Stream-Available` header signaled the opt-in was
//! available, but the wire body stayed JSON. Adopters consuming
//! the response as an SSE stream (the obvious interpretation of
//! the source) found nothing to parse.
//!
//! The root cause: v1.22.0's D6 backwards-compat gate
//! (`classify_dynamic_route_wire` requires either `strict_mode` or
//! `client_wants_sse`) was applied uniformly to BOTH implicit-
//! transport disjuncts:
//!   - (a) `output: Stream<T>` type annotation
//!   - (b) tool with `effects: <stream:<policy>>` algebraic effect
//!
//! But disjunct (b) is a LANGUAGE-LEVEL commitment (the tool's type
//! includes the algebraic effect). It should NOT depend on client
//! cooperation. v1.27.1 closes the gap with an algebraic-effect
//! override: when the tool declares `effects: <stream:...>`, the
//! wire is unconditionally SSE (D3 `transport: json` still wins).
//!
//! # What this test pack pins
//!
//! 1. **section 1 — Kivi-shape end-to-end**: `step S { apply: T }` where
//!    `T` declares `effects: <stream:drop_oldest>` produces
//!    `Content-Type: text/event-stream` on POST with NO `Accept:`
//!    header and the server in NON-strict mode.
//!
//! 2. **section 2 — Wire body shape**: the SSE body contains at least one
//!    `event: axon.token` followed by `event: axon.complete`.
//!    Stub backend emits `(stub)` per the 33.y `pure_shape` handler.
//!
//! 3. **section 3 — D3 sacred opt-out preserved**: an adopter who
//!    EXPLICITLY declares `transport: json` on the endpoint STILL
//!    gets JSON even when the tool has a stream effect.
//!
//! 4. **section 4 — Type-annotation-only flows still respect D6**: an
//!    adopter whose flow has `output: Stream<T>` but NO tool with
//!    stream effects (i.e., the algebraic predicate is false) still
//!    needs `Accept:` or strict_mode to get SSE — the override is
//!    surgical to the algebraic-effect case.

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
        // v1.24.0 — non-strict mode (D6 backwards-compat).
        // The whole point of the override is to honor algebraic
        // effects WITHOUT requiring strict mode.
        strict_type_driven_transport: false,
        default_backend: None,
        schemas_dir: None,
    }
}

async fn deploy(app: axum::Router, src: &str) -> StatusCode {
    let body = serde_json::json!({
        "source": src,
        "source_file": "kivi_shape.axon",
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

/// POST without `Accept:` header — the canonical Kivi-shape request.
async fn post_without_accept(
    app: axum::Router,
    path: &str,
) -> (StatusCode, String, String) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from("{}".to_string()))
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

// ─── Source fixtures ────────────────────────────────────────────────

/// Canonical Kivi-shape: tool with `effects: <stream:drop_oldest>` +
/// flow with a step that applies it + axonendpoint WITHOUT explicit
/// `transport:` declaration. Pre-33.z.k.1 this returned
/// Content-Type: application/json; post-33.z.k.1 it returns SSE.
fn kivi_shape_source() -> &'static str {
    "tool chat_token_stream { description: \"Stream tokens\" effects: <stream:drop_oldest> }\n\
     flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" apply: chat_token_stream output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true\n\
        method: POST\n\
        path: \"/chat\"\n\
        execute: Chat\n\
     }"
}

/// D3 opt-out variant: same flow + tool but the endpoint explicitly
/// declares `transport: json`. Sacred: stays JSON regardless of the
/// algebraic effect on the tool.
fn kivi_shape_with_d3_optout_source() -> &'static str {
    "tool chat_token_stream { description: \"Stream tokens\" effects: <stream:drop_oldest> }\n\
     flow ChatJson() -> Unit {\n\
        step Generate { ask: \"hi\" apply: chat_token_stream output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatJsonEndpoint { public: true\n\
        method: POST\n\
        path: \"/chat-json\"\n\
        execute: ChatJson\n\
        transport: json\n\
     }"
}

/// Type-annotation-only variant: `output: Stream<Token>` but NO tool
/// with stream effects. The algebraic-effect predicate is false; the
/// D6 backwards-compat gate STILL applies (must opt-in via Accept or
/// strict_mode). Pins that the override is surgical.
fn type_annotation_only_source() -> &'static str {
    "flow ChatTypeOnly() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatTypeOnlyEndpoint { public: true\n\
        method: POST\n\
        path: \"/chat-type-only\"\n\
        execute: ChatTypeOnly\n\
     }"
}

// ─── section 1 — Kivi-shape end-to-end (the bug the override closes) ───────

#[tokio::test]
async fn s1_kivi_shape_post_without_accept_returns_sse_d11() {
    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), kivi_shape_source()).await, StatusCode::OK);
    let (status, ct, _body) = post_without_accept(app, "/chat").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        ct.contains("text/event-stream"),
        "33.z.k.1 D11 algebraic-effect override: POST without Accept \
         header against a flow whose step applies a tool with \
         `effects: <stream:drop_oldest>` MUST return SSE \
         (Content-Type: text/event-stream). Got: {ct:?}"
    );
}

// ─── section 2 — Wire body shape (OpenAI chunks + [DONE] post-33.z.k.g.2) ───
//
// **Inverted post-33.z.k.g.2** — the Kivi-shape (tool with
// `effects: <stream:<policy>>`) resolves to the OpenAI dialect per
// the Q1 ratification. The wire body is OpenAI Chat Completions
// streaming format verbatim: `data: {"choices":[{"delta":{...}}]}`
// chunks terminated by `data: [DONE]`. The D11 algebraic-effect
// override (33.z.k.1) still fires — SSE wire IS served — but the
// FRAMING is the LLM-streaming ecosystem's wire, not axon's W3C
// named events. Adopters who want W3C named events declare
// `transport: sse(axon)` explicitly (Q5 escape valve).

#[tokio::test]
async fn s2_kivi_shape_wire_body_emits_openai_chunks_and_done_sentinel() {
    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), kivi_shape_source()).await, StatusCode::OK);
    let (status, ct, body) = post_without_accept(app, "/chat").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.contains("text/event-stream"), "expected SSE Content-Type");
    // POST-33.z.k.g.2: openai-dialect wire is the default for
    // algebraic-effect flows. Closed-catalog assertions:
    assert!(
        body.contains("\"object\":\"chat.completion.chunk\""),
        "33.z.k.g.2 Q1: SSE body MUST contain `chat.completion.chunk` object \
         shape per OpenAI Chat Completions streaming spec. Body: {body:?}"
    );
    assert!(
        body.contains("\"delta\":{\"role\":\"assistant\"}"),
        "33.z.k.g.2 Q1: first frame MUST carry role-marker \
         `delta: {{\"role\": \"assistant\"}}` per OpenAI spec. Body: {body:?}"
    );
    assert!(
        body.contains("data: [DONE]"),
        "33.z.k.g.2 Q1: SSE body MUST terminate with `data: [DONE]` sentinel \
         per OpenAI spec. Body: {body:?}"
    );
    // axon-named events MUST be structurally absent on the openai
    // wire (the dialect catalog is closed; adapters do not interleave).
    assert!(
        !body.contains("event: axon.token"),
        "33.z.k.g.2 Q1: openai-dialect wire MUST NOT carry W3C `event: axon.token` \
         lines (those are exclusive to the axon dialect — Q5 escape valve). \
         Body: {body:?}"
    );
    assert!(
        !body.contains("event: axon.complete"),
        "33.z.k.g.2 Q1: openai-dialect wire MUST NOT carry W3C `event: axon.complete` \
         lines. Body: {body:?}"
    );
    // D2 invariant — no unsupported_flow_shape on the wire (the
    // variant was deleted in 33.z.e; this is a defensive cross-check).
    assert!(
        !body.contains("unsupported_flow_shape"),
        "33.z.k.1 + 33.z.e: `axon-W002 unsupported_flow_shape` is structurally \
         unreachable. Body: {body:?}"
    );
}

// ─── section 3 — D3 sacred opt-out preserved above the override ────────────

#[tokio::test]
async fn s3_d3_transport_json_explicit_opt_out_wins_above_algebraic_override() {
    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), kivi_shape_with_d3_optout_source()).await, StatusCode::OK);
    let (status, ct, body) = post_without_accept(app, "/chat-json").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        ct.contains("application/json"),
        "33.z.k.1: explicit `transport: json` MUST stay JSON even when the \
         flow's tool declares a stream effect. D3 dominates D11. Got: {ct:?}"
    );
    // Defensive — must NOT emit SSE frames inside what claims to be JSON.
    assert!(
        !body.contains("event: axon.token"),
        "33.z.k.1 D3: JSON wire body MUST NOT carry SSE frames"
    );
}

// ─── section 4 — Type-annotation-only still respects D6 backwards-compat ───

#[tokio::test]
async fn s4_type_annotation_only_still_respects_d6_backwards_compat() {
    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), type_annotation_only_source()).await, StatusCode::OK);
    let (status, ct, _body) = post_without_accept(app, "/chat-type-only").await;
    assert_eq!(status, StatusCode::OK);
    // Pre-33.z.k.1 behavior preserved: type-annotation-only stream
    // flows (no tool with effects:<stream:...>) still need Accept or
    // strict_mode. The override is surgical to algebraic effects.
    assert!(
        ct.contains("application/json"),
        "33.z.k.1: flow with `output: Stream<T>` type annotation but NO tool \
         with `effects: <stream:...>` declaration MUST still respect D6 \
         backwards-compat (legacy default JSON without Accept or strict). \
         The override is surgical. Got: {ct:?}"
    );
}

// ─── section 5 — Override fires for POST + PUT + PATCH ─────────────────────

#[tokio::test]
async fn s5_override_fires_across_http_methods() {
    // v1.24.0 — Override is method-agnostic. The wire format
    // for tool-streaming flows is SSE regardless of POST/PUT/PATCH.
    // (GET/DELETE typically don't carry streamed bodies; they're not
    // exercised here.)
    let src = "tool chat_token_stream { description: \"S\" effects: <stream:drop_oldest> }\n\
        flow ChatPost() -> Unit {\n\
            step S { ask: \"hi\" apply: chat_token_stream output: Stream<Token> }\n\
        }\n\
        flow ChatPut() -> Unit {\n\
            step S { ask: \"hi\" apply: chat_token_stream output: Stream<Token> }\n\
        }\n\
        flow ChatPatch() -> Unit {\n\
            step S { ask: \"hi\" apply: chat_token_stream output: Stream<Token> }\n\
        }\n\
        axonendpoint EpPost { public: true method: POST path: \"/method-post\" execute: ChatPost }\n\
        axonendpoint EpPut { public: true method: PUT path: \"/method-put\" execute: ChatPut }\n\
        axonendpoint EpPatch { public: true method: PATCH path: \"/method-patch\" execute: ChatPatch }";
    let app = build_router(server_cfg());
    assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);

    for (method, path) in [("POST", "/method-post"), ("PUT", "/method-put"), ("PATCH", "/method-patch")] {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let ct = resp
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        assert!(
            ct.contains("text/event-stream"),
            "33.z.k.1: algebraic-effect override MUST fire for {method} {path}; got Content-Type {ct:?}"
        );
    }
}
