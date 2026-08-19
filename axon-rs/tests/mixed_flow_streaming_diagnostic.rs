//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.31.0 — Mixed-flow streaming diagnostic anchor.
//!
//! Pins the v1.34.0 state that v1.31.0 closes — the agent pattern
//! (retrieve context → deliberate → persist) behind a streaming
//! `axonendpoint`. This file is the committed baseline; each later
//! step inverts a -assertion:
//!
//! section 1 — `in_memory` is NOT a source-declarable `axonstore` backend.
//!        The type-checker rejects `backend: in_memory` even though
//!        the runtime `StoreRegistry` already supports it — so a
//!        mixed flow cannot run or be tested without a live Postgres.
//! → v1.31.0 inverts this (D2).
//!
//! section 2 — the streaming producer emits a DOUBLE TERMINATOR on the
//!        error path: `run_streaming_via_dispatcher` emits
//!        `FlowError` and then, unconditionally, `FlowComplete` — so
//!        an errored streaming flow puts BOTH `axon.error` AND
//! `axon.complete` on the wire, violating the v1.24.0
//!        "exactly one terminator" contract.
//! → v1.31.0 inverts this (D1).
//!
//! The diagnosis (founder hypothesis 2026-05-17): a real agent flow
//! mixes `axonstore` ops with a `step`; the streaming path was never
//! tested with that shape. Verified — the path DOES dispatch mixed
//! flows, but the wire is malformed on error and the shape is
//! structurally un-runnable without external infrastructure.

use axon::axon_server::{build_router, ServerConfig};
use axon::lexer::Lexer;
use axon::parser::Parser;
use axon::type_checker::TypeChecker;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

// ─── section 1 — `in_memory` is a source-declarable store backend ─────────
//
// INVERTED by v1.31.0 (D2). The v1.34.0 baseline rejected
// `backend: in_memory` at the type-checker (`VALID_STORE_BACKENDS`
// omitted it) — so the canonical agent flow could not be declared
// against an in-memory store. 36.x.b added `in_memory` to the
// catalog; this assertion is now flipped to its fixed form and
// stands as the regression guard.

#[test]
fn s1_in_memory_store_backend_is_declarable_post_36xb() {
    let src = "axonstore mem { backend: in_memory }";
    let tokens = Lexer::new(src, "<diag>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    let errors = TypeChecker::new(&prog).check();
    assert!(
        !errors.iter().any(|e| {
            let m = e.message.to_lowercase();
            m.contains("backend") && m.contains("in_memory")
        }),
        "v1.31.0 section 1 (inverted by 36.x.b / D2): `backend: \
         in_memory` is a first-class declarable axonstore backend — \
         it must type-check with no backend error. Errors: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ─── section 2 — the streaming error path emits a DOUBLE terminator ───────

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

async fn deploy(app: &axum::Router, src: &str) -> (StatusCode, serde_json::Value) {
    let body = serde_json::json!({ "source": src, "source_file": "diag.axon" });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

async fn hit_sse(app: &axum::Router, path: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn s2_streaming_error_path_emits_exactly_one_terminator_post_36xc() {
    // INVERTED by v1.31.0 (D1). The v1.34.0 baseline emitted a
    // double terminator on the streaming error path — `FlowError`
    // AND, unconditionally, `FlowComplete`. 36.x.c gated the section 7
    // `FlowComplete` emit; this assertion is now flipped to its fixed
    // form (exactly one terminator) and stands as the regression
    // guard.
    //
    // v1.30.0 closed the axonstore catalog to `{in_memory,
    // postgresql}` — `sqlite` is now an UnknownBackend that
    // `StoreRegistry::build` rejects at DEPLOY time (the route never
    // mounts), so it can no longer drive a request-time streaming
    // error. A `postgresql` store pointed at a dead port is the
    // canonical request-time failure: `StoreRegistry::build` is lazy
    // for postgresql, so the flow deploys + mounts, then errors
    // mid-walk at the `retrieve` node when the connection is refused —
    // the streaming producer terminates via `FlowError`. Deterministic
    // — no live database (the port is closed). `transport: sse(axon)`
    // selects the axon dialect so the terminator is `axon.error` /
    // `axon.complete`.
    let app = build_router(server_cfg());
    let src = "axonstore mem { backend: postgresql \
            connection: \"postgres://127.0.0.1:1/axon_36xa_dead\" }\n\
        flow ChatFlow() -> Unit {\n\
            retrieve mem { where: \"kind = 'history'\" as: history }\n\
            step Generate { ask: \"deliberate\" output: Stream<Token> }\n\
        }\n\
        axonendpoint ChatE { public: true method: POST path: \"/chat\" execute: ChatFlow \
        backend: stub transport: sse(axon) }";
    let (dstatus, dbody) = deploy(&app, src).await;
    assert_eq!(
        dstatus,
        StatusCode::OK,
        "v1.31.0 section 2: a `postgresql` store deploys (lazy build) — the \
         deploy must succeed + mount the route. Body: {dbody}"
    );
    assert_eq!(
        dbody.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "v1.31.0 section 2: the deploy must succeed + mount the route. Body: {dbody}"
    );

    let (status, wire) = hit_sse(&app, "/chat").await;
    eprintln!("v1.31.0 section 2 — status={status}\n wire:\n{wire}");

    let has_error = wire.contains("axon.error");
    let has_complete = wire.contains("axon.complete");

    assert!(
        has_error && !has_complete,
        "v1.31.0 section 2 (inverted by 36.x.c / D1): the streaming error \
         path must emit EXACTLY ONE terminator — `axon.error` ONLY, \
         never a trailing `axon.complete`. has_error={has_error} \
         has_complete={has_complete}\n  wire:\n{wire}"
    );
}

// ─── section 3 — diagnostic narrative, emitted for the record ─────────────

#[test]
fn s3_diagnostic_narrative() {
    eprintln!(
        "v1.31.0 — mixed-flow streaming gap (v1.34.0 baseline):\n\
         A. run_streaming_via_dispatcher emits FlowError THEN an\n\
            unconditional FlowComplete — the SSE wire carries a double\n\
            terminator on every error path.\n\
         B. Zero test coverage — no `transport: sse` test in the\n\
            entire v1.30.0 axonstore suite.\n\
         C. The agent pattern cannot run without a live Postgres —\n\
            `in_memory` is not a source-declarable store backend, and\n\
            sqlite/mysql type-check but have no runtime backend.\n\
         POST-36.x: `in_memory` is declarable (D2); the streaming\n\
         producer emits exactly one terminator (D1); the mixed\n\
         retrieve→step→persist flow is a tested primitive (D3)."
    );
}
