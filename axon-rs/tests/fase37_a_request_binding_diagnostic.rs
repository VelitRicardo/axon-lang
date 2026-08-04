//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 37.a — The Request Binding Contract: diagnostic anchor.
//!
//! This pack PINS the v1.35.0 broken state. Each later sub-fase of
//! Fase 37 inverts one of these §-assertions in place; until then
//! the broken behaviour is a committed, reproducible fact.
//!
//! Two findings, from the adopter report 2026-05-18:
//!
//!   FINDING A — the `body:` ↔ flow-parameter binding is type-checked
//!     but never executed. An `axonendpoint` declares `body: T` (the
//!     typed request body) and `execute: F` (a flow with declared
//!     parameters). The promise of `body: T` is that the request
//!     body's fields populate `F`'s parameters. AXON type-checks the
//!     promise and breaks it: `dynamic_endpoint_handler` parses the
//!     body (schema validation, idempotency hash, replay capture) and
//!     then DISCARDS it — the execution request is built from
//!     `flow_name + backend` only, on BOTH transports, and nothing
//!     seeds `DispatchCtx.let_bindings` from `IRFlow.parameters`. A
//!     `${param}` therefore interpolates to the literal.
//!     §1 pinned it; §2 is the positive control (a `let` binding DOES
//!     interpolate — so the defect is body-params specifically, not a
//!     broken harness). §Fase 37.b/D1 SHIPPED — §1 was inverted in
//!     place and is now a green regression guard for the contract.
//!
//!   FINDING B — an errored streaming flow emits a hollow terminator.
//!     `FlowExecutionEvent::FlowError` carries an `error` string and
//!     the producer populates it, but the `openai` wire dialect's
//!     `FlowError` arm (`wire_format/openai_dialect.rs`) DROPS it —
//!     the wire shows the flow errored (`terminal_reason: error`) but
//!     never said WHY. §3 pinned it. §Fase 37.e/D6 SHIPPED — §3 was
//!     inverted in place and is now a green regression guard.
//!
//! All three tests are deterministic + infra-free (stub backend,
//! `sqlite`-registry-build failure for the error path — no DB, no
//! network, no env).

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

/// Deploy `src`; assert the deploy succeeded.
async fn deploy(app: &axum::Router, src: &str) {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::json!({ "source": src }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(status, StatusCode::OK, "37.a deploy failed: {json}");
}

/// Hit `path` with a POST + JSON `body`, `Accept: text/event-stream`;
/// drain the full SSE response into a String.
async fn hit_sse(app: &axum::Router, path: &str, body: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "37.a {path} status");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

// ── §1 — FINDING A: the request body never reaches the flow ─────────

/// A parameterised flow behind an `axonendpoint body: T`, hit with a
/// well-formed body carrying the parameter value. The body value
/// reaches the flow — `${message}` (a declared flow parameter) binds
/// from the body field `message` and interpolates.
///
/// §Fase 37.b (D1) SHIPPED — this assertion was inverted in place:
/// it pinned the v1.35.0 broken state (body discarded) and is now a
/// green regression guard for the Request Binding Contract.
#[tokio::test]
async fn s1_request_body_value_reaches_the_flow() {
    let app = build_router(server_cfg());
    // `Echo` is a `stub_stream` tool — `StubStreamingTool` echoes its
    // argument verbatim, so the interpolated `ask` is observable on
    // the SSE wire. `EchoFlow` declares a `message` parameter; the
    // endpoint declares `body: EchoBody { message: String }`.
    let src = "type EchoBody { message: String }\n\
        tool Echo { provider: stub_stream description: \"echo\" \
                    effects: <stream:drop_oldest> }\n\
        flow EchoFlow(message: String) -> Unit {\n\
            step Reply { ask: \"BODYVAL=${message}\" apply: Echo }\n\
        }\n\
        axonendpoint EchoE { public: true method: POST path: \"/echo\" \
            body: EchoBody execute: EchoFlow backend: stub transport: sse }";
    deploy(&app, src).await;

    let wire = hit_sse(
        &app,
        "/echo",
        r#"{"message":"SENTINEL_BODY_VALUE_37A"}"#,
    )
    .await;

    // ── §Fase 37.b SHIPPED — the Request Binding Contract ──────────
    assert!(
        wire.contains("SENTINEL_BODY_VALUE_37A"),
        "§37.b D1 — the request body value MUST reach the flow: \
         `${{message}}` is a declared parameter of `EchoFlow`, bound \
         by name from the body field `message` and seeded into \
         `DispatchCtx.let_bindings` before the flow walk. Wire:\n{wire}"
    );
    assert!(
        !wire.contains("${message}"),
        "§37.b D1 — the literal `${{message}}` must NOT survive to the \
         wire — it interpolated to the bound body value. Wire:\n{wire}"
    );
}

// ── §2 — positive control: a `let` binding DOES interpolate ─────────

/// The harness control. The SAME echo-tool observation technique,
/// but the interpolated variable is a flow-body `let` binding (which
/// `run_step` DOES seed into `ctx.let_bindings`) rather than a flow
/// parameter from the request body. This MUST interpolate — proving
/// the §1 failure is the request-body binding specifically, not a
/// broken observation harness. This test stays green across all of
/// Fase 37 (a regression guard, never inverted).
#[tokio::test]
async fn s2_control_let_binding_does_interpolate() {
    let app = build_router(server_cfg());
    let src = "tool Echo { provider: stub_stream description: \"echo\" \
                    effects: <stream:drop_oldest> }\n\
        flow CtrlFlow() -> Unit {\n\
            let greeting = \"HARNESS_OK_37A\"\n\
            step Reply { ask: \"V=${greeting}\" apply: Echo }\n\
        }\n\
        axonendpoint CtrlE { public: true method: POST path: \"/ctrl\" \
            execute: CtrlFlow backend: stub transport: sse }";
    deploy(&app, src).await;

    let wire = hit_sse(&app, "/ctrl", "{}").await;

    assert!(
        wire.contains("HARNESS_OK_37A"),
        "§37.a control — a flow-body `let` binding MUST interpolate \
         into a step `ask:` (Fase 36.x.e wired this). If this fails, \
         the echo-tool observation harness is broken and §1's \
         conclusion is unsound. Wire:\n{wire}"
    );
}

// ── §3 — FINDING B: an errored streaming flow is hollow ─────────────

/// A flow that errors on the streaming path. A `postgresql` axonstore
/// pointed at a dead port type-checks AND deploys (its
/// `StoreRegistry::build` is lazy), then fails mid-walk at the
/// `retrieve` node when the connection is refused → `FlowError`. The
/// endpoint declares the `openai` SSE dialect.
///
/// (§Fase 35.f closed the axonstore catalog to `{in_memory,
/// postgresql}`; `sqlite` — the pre-§35.f vehicle here — is now an
/// UnknownBackend rejected at DEPLOY time, so the route would never
/// mount and the request-time streaming error could not be observed.)
///
/// §Fase 37.e (D6) SHIPPED — this assertion was inverted in place: it
/// pinned the v1.35.0 hollow terminator (the openai dialect dropped
/// `FlowError.error`) and is now a green regression guard — the wire
/// names WHY the flow errored.
#[tokio::test]
async fn s3_errored_streaming_flow_names_why_it_failed() {
    let app = build_router(server_cfg());
    // `Echo`'s `<stream:…>` effect makes the flow stream-producing so
    // `transport: sse` type-checks; the flow errors mid-walk at the
    // `retrieve` node (dead postgresql port) BEFORE the step runs.
    let src = "axonstore bad { backend: postgresql \
            connection: \"postgres://127.0.0.1:1/axon_37a_dead\" }\n\
        tool Echo { provider: stub_stream description: \"echo\" \
                    effects: <stream:drop_oldest> }\n\
        flow ErrFlow() -> Unit {\n\
            retrieve bad { where: \"1 = 1\" as: r }\n\
            step Reply { ask: \"x\" apply: Echo }\n\
        }\n\
        axonendpoint ErrE { public: true method: POST path: \"/err\" \
            execute: ErrFlow backend: stub transport: sse(openai) }";
    deploy(&app, src).await;

    let wire = hit_sse(&app, "/err", "{}").await;

    // The wire is well-formed + knows the flow errored …
    assert!(
        wire.contains("[DONE]"),
        "§37.a — the openai-dialect SSE stream must terminate with \
         `[DONE]`. Wire:\n{wire}"
    );
    assert!(
        wire.contains("terminal_reason") && wire.contains("error"),
        "§37.a — the wire must signal the flow errored \
         (`terminal_reason: error` in the axon_metadata frame). \
         Wire:\n{wire}"
    );
    // ── §Fase 37.e SHIPPED — honest failure ────────────────────────
    // … and now it says WHY. The `FlowError.error` string for a mid-
    // walk dispatch failure (`flow '…' failed at retrieve from
    // 'bad': …`) reaches the wire — the openai dialect surfaces it in
    // the axon_metadata frame's `error` field.
    assert!(
        wire.contains("failed at retrieve from 'bad'"),
        "§37.e D6 — the error DETAIL must reach the wire: a streaming \
         flow that fails names WHY (the failing node), not just THAT. \
         Wire:\n{wire}"
    );
}
