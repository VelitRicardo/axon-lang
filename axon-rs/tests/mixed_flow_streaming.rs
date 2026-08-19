//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.31.0 (D3) — the mixed flow is a first-class streaming
//! shape.
//!
//! A real agent flow — retrieve context → deliberate (the step) →
//! persist the result — mixes `axonstore` ops with a `step`. Before
//! 36.x this shape had ZERO test coverage on the streaming
//! `axonendpoint` path (no `transport: sse` test in the whole cycle
//! 35 axonstore suite) and could not even run without a live
//! Postgres. 36.x.b made `in_memory` declarable; 36.x.c fixed the
//! terminator. This pack is the coverage D3 requires.
//!
//! Pins (every flow uses an `in_memory` store — zero infrastructure):
//!   1. The canonical agent flow — retrieve → step → persist —
//!      streams to a clean `axon.complete`, no `axon.error`.
//!   2. The founder's ChatFlow shape — retrieve ×3 + persist + step
//!      + persist — streams clean; `steps_executed` counts all six
//!      nodes (the store-ops are real executed steps).
//!   3. Exactly one terminator — `axon.complete` ×1, `axon.error` ×0.
//!   4. The store-ops add NO wire frames — the SSE event vocabulary
//!      of a mixed flow is the same closed set as a pure-step flow
//!      (`axon.token` / `axon.complete`); D9 byte-compat.
//!   5. A mid-flow store error terminates with exactly one
//! `axon.error` and no `axon.complete` (the v1.31.0 flag).

use axon::axon_server::{build_router, ServerConfig};
use axon::backends::env_available_backends;
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

async fn deploy(app: &axum::Router, src: &str) {
    let body = serde_json::json!({ "source": src, "source_file": "agent.axon" });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(status, StatusCode::OK, "deploy failed: {json}");
    assert_eq!(json.get("success").and_then(|v| v.as_bool()), Some(true), "{json}");
}

async fn hit_sse(app: &axum::Router, path: &str) -> (StatusCode, String, String) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, ct, String::from_utf8_lossy(&bytes).into_owned())
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// The last `axon.complete` event's JSON `data:` payload, parsed.
fn complete_payload(wire: &str) -> serde_json::Value {
    for line in wire.lines() {
        if let Some(rest) = line.strip_prefix("data: ") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(rest) {
                if v.get("steps_executed").is_some() {
                    return v;
                }
            }
        }
    }
    serde_json::Value::Null
}

// ─── section 1 — the canonical agent flow streams clean ───────────────────

#[tokio::test]
async fn s1_canonical_agent_flow_streams_clean() {
    let app = build_router(server_cfg());
    deploy(
        &app,
        "axonstore mem { backend: in_memory }\n\
         flow AgentFlow() -> Unit {\n\
             retrieve mem { where: \"kind = 'history'\" as: history }\n\
             step Deliberate { ask: \"answer\" output: Stream<Token> }\n\
             persist into mem { kind: \"reply\" content: \"done\" }\n\
         }\n\
         axonendpoint AgentE { public: true method: POST path: \"/agent\" execute: AgentFlow \
         backend: stub transport: sse }",
    )
    .await;

    let (status, ct, wire) = hit_sse(&app, "/agent").await;
    eprintln!("v1.31.0 section 1 — status={status}\n{wire}");
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"), "got {ct}");
    assert!(
        wire.contains("axon.token"),
        "36.x.d D3: the deliberation step must stream a token. Wire:\n{wire}"
    );
    assert!(
        wire.contains("axon.complete") && !wire.contains("axon.error"),
        "36.x.d D3: the retrieve → step → persist agent flow must \
         stream to a clean `axon.complete` with no `axon.error`. \
         Wire:\n{wire}"
    );
}

// ─── section 2 — the founder's ChatFlow shape (6 nodes) ───────────────────

#[tokio::test]
async fn s2_founder_chatflow_shape_streams_and_counts_all_nodes() {
    let app = build_router(server_cfg());
    // retrieve ×3 + persist + step + persist — the exact multi-
    // statement agent shape from the founder's 2026-05-17 report.
    deploy(
        &app,
        "axonstore mem { backend: in_memory }\n\
         flow ChatFlow() -> Unit {\n\
             retrieve mem { where: \"kind = 'history'\" as: history }\n\
             retrieve mem { where: \"kind = 'profile'\" as: profile }\n\
             retrieve mem { where: \"kind = 'policy'\"  as: policy }\n\
             persist into mem { kind: \"user_msg\" content: \"hello\" }\n\
             step Generate { ask: \"deliberate\" output: Stream<Token> }\n\
             persist into mem { kind: \"assistant_msg\" content: \"reply\" }\n\
         }\n\
         axonendpoint ChatE { public: true method: POST path: \"/chat\" execute: ChatFlow \
         backend: stub transport: sse }",
    )
    .await;

    let (status, _ct, wire) = hit_sse(&app, "/chat").await;
    eprintln!("v1.31.0 section 2 — status={status}\n{wire}");
    assert_eq!(status, StatusCode::OK);
    assert!(
        !wire.contains("axon.error"),
        "36.x.d D3: the founder's ChatFlow (retrieve ×3 + persist + \
         step + persist) must stream cleanly. Wire:\n{wire}"
    );
    let done = complete_payload(&wire);
    let steps = done["steps_executed"].as_u64().unwrap_or(0);
    assert_eq!(
        steps, 6,
        "36.x.d D3: every node of the agent flow is a real executed \
         step — `steps_executed` must count all six (3 retrieve + 1 \
         persist + 1 step + 1 persist). Got {steps}. complete: {done}"
    );
    assert_eq!(done["success"], true, "complete: {done}");
}

// ─── section 3 — exactly one terminator ───────────────────────────────────

#[tokio::test]
async fn s3_mixed_flow_emits_exactly_one_terminator() {
    let app = build_router(server_cfg());
    deploy(
        &app,
        "axonstore mem { backend: in_memory }\n\
         flow AgentFlow() -> Unit {\n\
             retrieve mem { where: \"1 = 1\" as: ctx }\n\
             step Think { ask: \"x\" output: Stream<Token> }\n\
             persist into mem { kind: \"r\" content: \"v\" }\n\
         }\n\
         axonendpoint E { public: true method: POST path: \"/a\" execute: AgentFlow \
         backend: stub transport: sse }",
    )
    .await;
    let (_s, _ct, wire) = hit_sse(&app, "/a").await;
    assert_eq!(
        count(&wire, "event: axon.complete"),
        1,
        "36.x.d D1: exactly one `axon.complete`. Wire:\n{wire}"
    );
    assert_eq!(
        count(&wire, "event: axon.error"),
        0,
        "36.x.d D1: a clean mixed flow emits zero `axon.error`. Wire:\n{wire}"
    );
}

// ─── section 4 — store-ops add no wire frames (D9 byte-compat) ────────────

#[tokio::test]
async fn s4_store_ops_add_no_wire_frames() {
    let app = build_router(server_cfg());
    deploy(
        &app,
        "axonstore mem { backend: in_memory }\n\
         flow AgentFlow() -> Unit {\n\
             retrieve mem { where: \"1 = 1\" as: ctx }\n\
             step Think { ask: \"x\" output: Stream<Token> }\n\
             persist into mem { kind: \"r\" content: \"v\" }\n\
         }\n\
         axonendpoint E { public: true method: POST path: \"/a\" execute: AgentFlow \
         backend: stub transport: sse }",
    )
    .await;
    let (_s, _ct, wire) = hit_sse(&app, "/a").await;
    // Every `event:` frame on a clean mixed-flow stream is one of the
    // closed pure-step catalog — `axon.token` / `axon.complete`. The
    // retrieve / persist ops execute but emit no extra wire frame
    // (their `StepStart`/`StepComplete` are consumed silently per
    // v1.24.0 — D9 byte-compat: a mixed flow's wire vocabulary is
    // identical to a pure-step flow's).
    for line in wire.lines().filter(|l| l.starts_with("event:")) {
        assert!(
            line == "event: axon.token" || line == "event: axon.complete",
            "36.x.d D9: a mixed flow's SSE wire carries only the \
             pure-step event vocabulary — store-ops add no frame. \
             Unexpected: `{line}`\n  wire:\n{wire}"
        );
    }
}

// ─── section 5 — a mid-flow store error → exactly one `axon.error` ────────

#[tokio::test]
async fn s5_mid_flow_store_error_emits_one_terminator() {
    // A `postgresql` store with no `DATABASE_URL` errors INSIDE the
    // dispatch loop (`run_retrieve` → section 6 `Err(e)`), exercising the
    // v1.31.0 `flow_errored` flag on the dispatch-loop path
    // (distinct from 36.x.a section 2's section 2.5 registry-build path). Gated on
    // the standard CI condition (no DATABASE_URL).
    if std::env::var("DATABASE_URL").is_ok() || !env_available_backends().is_empty() {
        eprintln!("36.x.d s5: skipped — DATABASE_URL / provider keys present.");
        return;
    }
    let app = build_router(server_cfg());
    deploy(
        &app,
        "axonstore live { backend: postgresql connection: \"env:DATABASE_URL\" }\n\
         flow AgentFlow() -> Unit {\n\
             retrieve live { where: \"1 = 1\" as: ctx }\n\
             step Think { ask: \"x\" output: Stream<Token> }\n\
         }\n\
         axonendpoint E { public: true method: POST path: \"/a\" execute: AgentFlow \
         backend: stub transport: sse }",
    )
    .await;
    let (status, _ct, wire) = hit_sse(&app, "/a").await;
    eprintln!("v1.31.0 section 5 — status={status}\n{wire}");
    assert_eq!(
        count(&wire, "event: axon.error"),
        1,
        "36.x.d D1: a mid-flow store error emits exactly one \
         `axon.error`. Wire:\n{wire}"
    );
    assert_eq!(
        count(&wire, "event: axon.complete"),
        0,
        "36.x.d D1: …and NO trailing `axon.complete` — exactly one \
         terminator (the v1.31.0 fix). Wire:\n{wire}"
    );
}
