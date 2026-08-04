//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 37.e (D6) — honest streaming failure.
//!
//! The adopter's Finding B: a streaming flow that errored emitted a
//! HOLLOW terminator — the wire said `terminal_reason: error` but
//! never said WHY. The cause was dialect-specific: `FlowError` carries
//! an `error` diagnostic string, the `axon` dialect surfaced it, but
//! the `openai` (and therefore `kimi` / `glm`) and `anthropic`
//! dialects DROPPED it.
//!
//! 37.e closes it. Every dialect now surfaces `FlowError.error`:
//!   - `axon`     — the dedicated `event: axon.error` envelope.
//!   - `openai`   — the `axon_metadata` frame gains an `error` field.
//!   - `anthropic`— the `axon.metadata` frame gains an `error` field.
//! And the producer logs a structured server-side error line + names
//! the FAILING NODE in the diagnostic (`flow 'F' failed at <node>`).
//!
//! Fase 36 made backend-resolution failure honest (a structured 503);
//! Fase 37.e makes flow-execution failure on the streaming wire
//! honest. A stream that dies says why.
//!
//! Deterministic + infra-free: §1–§4 fail connecting to a dead
//! postgresql port (no DB needed for the connection to be refused —
//! `StoreRegistry::build` is lazy for postgresql so the route mounts,
//! then errors mid-walk at the `retrieve` node). §5 is the happy
//! path. (Pre-§35.f these used a `sqlite` store; the closed catalog
//! `{in_memory, postgresql}` now rejects `sqlite` at DEPLOY time, so
//! it can no longer drive a request-time streaming error.)

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
    assert_eq!(status, StatusCode::OK, "37.e deploy failed: {json}");
}

async fn hit_sse(app: &axum::Router, path: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "37.e {path} status");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

const ECHO_TOOL: &str = "tool Echo { provider: stub_stream description: \"echo\" \
                          effects: <stream:drop_oldest> }\n";

/// A flow that fails at request time on the streaming wire, behind
/// `dialect`. §Fase 35.f closed the axonstore catalog to
/// `{in_memory, postgresql}` — `sqlite` is now an UnknownBackend that
/// `StoreRegistry::build` rejects at DEPLOY time (the route never
/// mounts), so it can no longer drive a request-time streaming error.
/// A `postgresql` store pointed at a dead port is the canonical
/// request-time failure: `StoreRegistry::build` is lazy for
/// postgresql, so the flow deploys + mounts, then errors mid-walk at
/// the `retrieve` node when the connection is refused — which the
/// §Fase 37.e diagnostic NAMES on the wire.
fn dead_store_error_program(endpoint: &str, path: &str, dialect: &str) -> String {
    format!(
        "axonstore bad {{ backend: postgresql \
             connection: \"postgres://127.0.0.1:1/axon_37e_dead\" }}\n\
         {ECHO_TOOL}\
         flow ErrFlow() -> Unit {{\n\
             retrieve bad {{ where: \"1 = 1\" as: r }}\n\
             step Reply {{ ask: \"x\" apply: Echo }}\n\
         }}\n\
         axonendpoint {endpoint} {{ public: true method: POST path: \"{path}\" \
             execute: ErrFlow backend: stub transport: sse({dialect}) }}"
    )
}

// ── §1 — the openai dialect surfaces the error ──────────────────────

#[tokio::test]
async fn s1_openai_dialect_surfaces_the_error_detail() {
    let app = build_router(server_cfg());
    deploy(&app, &dead_store_error_program("OErrE", "/oerr", "openai")).await;

    let wire = hit_sse(&app, "/oerr").await;

    assert!(
        wire.contains("[DONE]") && wire.contains("terminal_reason"),
        "§37.e — the openai stream terminates with a metadata frame + \
         `[DONE]`. Wire:\n{wire}"
    );
    assert!(
        wire.contains("failed at retrieve from 'bad'"),
        "§37.e D6 — the openai `axon_metadata` frame must surface the \
         `FlowError.error` diagnostic in its `error` field — the wire \
         names the FAILING NODE (`failed at retrieve from 'bad'`), so \
         it says WHY, not just THAT it errored. Wire:\n{wire}"
    );
}

// ── §2 — the axon dialect surfaces the error ────────────────────────

#[tokio::test]
async fn s2_axon_dialect_surfaces_the_error_detail() {
    let app = build_router(server_cfg());
    deploy(&app, &dead_store_error_program("AErrE", "/aerr", "axon")).await;

    let wire = hit_sse(&app, "/aerr").await;

    assert!(
        wire.contains("axon.error"),
        "§37.e — the axon dialect emits a dedicated `event: axon.error`. \
         Wire:\n{wire}"
    );
    assert!(
        wire.contains("failed at retrieve from 'bad'"),
        "§37.e D6 — the `axon.error` event carries the `error` \
         diagnostic naming the failing node. Wire:\n{wire}"
    );
}

// ── §3 — the anthropic dialect surfaces the error ───────────────────

#[tokio::test]
async fn s3_anthropic_dialect_surfaces_the_error_detail() {
    let app = build_router(server_cfg());
    deploy(&app, &dead_store_error_program("NErrE", "/nerr", "anthropic")).await;

    let wire = hit_sse(&app, "/nerr").await;

    assert!(
        wire.contains("axon.metadata"),
        "§37.e — the anthropic dialect emits an `axon.metadata` frame. \
         Wire:\n{wire}"
    );
    assert!(
        wire.contains("failed at retrieve from 'bad'"),
        "§37.e D6 — the anthropic `axon.metadata` frame must surface \
         the `error` diagnostic naming the failing node. Wire:\n{wire}"
    );
}

// ── §4 — the failing node is named ──────────────────────────────────

#[tokio::test]
async fn s4_the_failing_node_is_named_in_the_diagnostic() {
    let app = build_router(server_cfg());
    // A `postgresql` store whose connection points at a dead port:
    // `StoreRegistry::build` is lazy for postgresql, so the failure is
    // a mid-walk dispatch error at the `retrieve` node — which the
    // §Fase 37.e diagnostic NAMES.
    let src = format!(
        "axonstore pg {{ backend: postgresql \
             connection: \"postgres://127.0.0.1:1/axon_37e_dead\" }}\n\
         {ECHO_TOOL}\
         flow NodeErrFlow() -> Unit {{\n\
             retrieve pg {{ where: \"1 = 1\" as: r }}\n\
             step Reply {{ ask: \"x\" apply: Echo }}\n\
         }}\n\
         axonendpoint NodeErrE {{ public: true method: POST path: \"/nodeerr\" \
             execute: NodeErrFlow backend: stub transport: sse(openai) }}"
    );
    deploy(&app, &src).await;

    let wire = hit_sse(&app, "/nodeerr").await;

    assert!(
        wire.contains("retrieve from 'pg'"),
        "§37.e D6 — a mid-walk dispatch failure must NAME the failing \
         node: the diagnostic reads `flow '…' failed at retrieve from \
         'pg': …`, not a generic `dispatcher error`. Wire:\n{wire}"
    );
}

// ── §5 — D5: a successful streaming flow carries no `error` ─────────

#[tokio::test]
async fn s5_d5_successful_flow_wire_has_no_error_field() {
    let app = build_router(server_cfg());
    let src = "flow OkFlow() -> Unit {\n\
            step Reply { ask: \"hello\" output: Stream<Token> }\n\
        }\n\
        axonendpoint OkE { public: true method: POST path: \"/ok\" \
            execute: OkFlow backend: stub transport: sse(openai) }";
    deploy(&app, src).await;

    let wire = hit_sse(&app, "/ok").await;

    assert!(
        wire.contains("terminal_reason") && !wire.contains("\"error\""),
        "§37.e D5 — a flow that does NOT error carries NO `error` field \
         on its metadata frame; the honest-failure surface is elided on \
         the happy path. Wire:\n{wire}"
    );
}
