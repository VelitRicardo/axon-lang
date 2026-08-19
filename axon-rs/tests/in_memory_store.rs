//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.31.0 (D2) — `in_memory` axonstore backend, end-to-end.
//!
//! 36.x.b makes `in_memory` a first-class declarable `axonstore`
//! backend. The frontend now type-checks it; the runtime
//! `StoreRegistry` already resolves it. This pack proves the loop
//! closes — a SOURCE-declared `in_memory` store deploys and a flow
//! against it runs on the production streaming path, with zero
//! external infrastructure (no Postgres, no `DATABASE_URL`).
//!
//! Pins:
//!   1. `classify_backend("in_memory")` → `StoreBackendKind::InMemory`
//!      — the runtime side of the catalog.
//!   2. The canonical agent flow — `in_memory` store + persist +
//!      retrieve + step, behind a streaming `axonendpoint` — deploys
//!      and streams to a clean `axon.complete` with NO `axon.error`.

use axon::axon_server::{build_router, ServerConfig};
use axon::store::registry::{classify_backend, StoreBackendKind};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

// ─── section 1 — the runtime catalog accepts `in_memory` ──────────────────

#[test]
fn s1_classify_backend_maps_in_memory_to_the_in_memory_handle() {
    assert_eq!(
        classify_backend("in_memory"),
        Some(StoreBackendKind::InMemory),
        "36.x.b D2: the runtime `classify_backend` must map \
         `in_memory` to the in-memory store kind — the frontend \
         catalog (`VALID_STORE_BACKENDS`) and the runtime catalog \
         must agree on `in_memory`"
    );
    // `postgresql` still resolves; an unknown backend still does not.
    assert_eq!(classify_backend("postgresql"), Some(StoreBackendKind::Postgresql));
    assert_eq!(classify_backend("redis"), None);
}

// ─── section 2 — the agent flow runs against an in-memory store ───────────

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
    let body = serde_json::json!({ "source": src, "source_file": "test.axon" });
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

#[tokio::test]
async fn s2_agent_flow_runs_against_an_in_memory_store_no_infra() {
    let app = build_router(server_cfg());
    // The canonical agent shape — persist → retrieve → deliberate —
    // against a source-declared `in_memory` store. No DATABASE_URL,
    // no Postgres: this is the structural unblocker working.
    let src = "axonstore mem { backend: in_memory }\n\
        flow AgentFlow() -> Unit {\n\
            persist into mem { kind: \"user\" content: \"hola\" }\n\
            retrieve mem { where: \"kind = 'user'\" as: history }\n\
            step Deliberate { ask: \"answer\" output: Stream<Token> }\n\
        }\n\
        axonendpoint AgentE { public: true method: POST path: \"/agent\" \
        execute: AgentFlow backend: stub transport: sse }";
    let (dstatus, dbody) = deploy(&app, src).await;
    assert_eq!(
        dstatus,
        StatusCode::OK,
        "36.x.b D2: a source-declared `in_memory` store must deploy. \
         Body: {dbody}"
    );
    assert_eq!(dbody["success"], true, "{dbody}");

    let (status, ct, wire) = hit_sse(&app, "/agent").await;
    eprintln!("36.x.b section 2 — status={status} ct={ct}\n wire:\n{wire}");
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"), "got {ct}");
    assert!(
        !wire.contains("axon.error"),
        "36.x.b D2: the agent flow against an `in_memory` store must \
         NOT error — `in_memory` resolves to the key-value path with \
         no external dependency. Wire:\n{wire}"
    );
    assert!(
        wire.contains("axon.complete"),
        "36.x.b D2: the agent flow must stream to completion. Wire:\n{wire}"
    );
}
