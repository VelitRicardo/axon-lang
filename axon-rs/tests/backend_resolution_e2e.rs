//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.31.0 (D12) — real-backend end-to-end capstone.
//!
//! The gap report's exact shape: deploy an `axonendpoint`, hit it
//! over HTTP, observe the execution. Pre-36 the `axon_metadata`
//! revealed `backend: stub`, `steps_executed: 0`, `success: false`
//! — the endpoint routed + streamed perfectly but ran NOTHING real.
//!
//! This pack closes the loop end-to-end:
//!
//! section 1 — The full v1.31.0 chain over HTTP: deploy → resolve (D1) →
//!        route → execute → observe (D8). A declared `backend: stub`
//!        endpoint runs a REAL step and the resolution is visible on
//!        the `X-Axon-Backend` header + in the `backend_resolution`
//!        body object.
//! section 2 — A real backend behind a step: a `provider: http` streaming
//!        tool executes against a LOCAL MOCK server (deterministic,
//! no API keys, no external network — the section 7 discipline).
//!        The mock's streamed tokens reach the SSE wire — proof the
//!        deploy→route→dispatch→tool-stream path runs real I/O.
//! section 3 — Honest failure end-to-end: an unresolvable endpoint fails
//!        with a structured 503, never the silent no-op.

use axon::axon_server::{build_router, ServerConfig};
use axon::backends::env_available_backends;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, Response, StatusCode};
use axum::routing::post;
use axum::Router;
use http_body_util::BodyExt;
use std::convert::Infallible;
use std::time::Duration;
use tokio::net::TcpListener;
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
    let body = serde_json::json!({ "source": src, "source_file": "e2e.axon" });
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

struct Hit {
    status: StatusCode,
    content_type: String,
    backend_header: Option<String>,
    trace_header: Option<String>,
    body: Vec<u8>,
}

async fn hit(app: &axum::Router, path: &str, accept_sse: bool) -> Hit {
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json");
    if accept_sse {
        builder = builder.header("accept", "text/event-stream");
    }
    let resp = app.clone().oneshot(builder.body(Body::from("{}")).unwrap()).await.unwrap();
    let status = resp.status();
    let hdr = |n: &str| {
        resp.headers().get(n).and_then(|v| v.to_str().ok()).map(|s| s.to_string())
    };
    let content_type = hdr("content-type").unwrap_or_default();
    let backend_header = hdr("x-axon-backend");
    let trace_header = hdr("x-axon-trace-id");
    let body = resp.into_body().collect().await.unwrap().to_bytes().to_vec();
    Hit { status, content_type, backend_header, trace_header, body }
}

// ════ section 1 — the full v1.31.0 chain over HTTP ════════════════════════

#[tokio::test]
async fn s1_full_chain_deploy_resolve_route_execute_observe() {
    let app = build_router(server_cfg());
    deploy(
        &app,
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat backend: stub }",
    )
    .await;

    let h = hit(&app, "/chat", false).await;
    assert_eq!(h.status, StatusCode::OK, "the deployed endpoint must dispatch");
    assert!(h.content_type.starts_with("application/json"));

    // D8 — resolution observability on the header.
    assert_eq!(
        h.backend_header.as_deref(),
        Some("stub; reason=endpoint_declared"),
        "36.l: the X-Axon-Backend header must answer 'why this model?'"
    );
    assert!(h.trace_header.is_some(), "36.l: every response carries X-Axon-Trace-Id");

    let json: serde_json::Value = serde_json::from_slice(&h.body).unwrap();
    // D11 — a REAL step ran (the gap report's `steps_executed: 0` is
    // structurally inverted).
    // v2.0.0 FlowEnvelope wire: no top-level `success`; a real step ran
    // is observable via step_audit.steps_executed.
    assert!(
        json["step_audit"]["steps_executed"].as_u64().unwrap_or(0) >= 1,
        "36.l D11: the deployed endpoint runs a real step. Body: {json}"
    );
    // D8 — resolution observability in the body.
    assert_eq!(json["backend_resolution"]["backend"], "stub");
    assert_eq!(json["backend_resolution"]["reason"], "endpoint_declared");
}

// ════ section 2 — a real backend behind a step (local mock server) ════════

const MOCK_SSE_BODY: &str = concat!(
    "data: axon-e2e-token-ALPHA\n\n",
    "data: axon-e2e-token-BRAVO\n\n",
    "data: axon-e2e-token-CHARLIE\n\n",
);

async fn mock_llm_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from(MOCK_SSE_BODY))
        .unwrap())
}

async fn spawn_mock_llm() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let router = Router::new().route("/", post(mock_llm_handler));
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    tokio::time::sleep(Duration::from_millis(25)).await;
    format!("http://{addr}")
}

#[tokio::test]
async fn s2_endpoint_executes_a_step_against_a_mock_llm_server() {
    let mock_url = spawn_mock_llm().await;
    let app = build_router(server_cfg());

    // v2.69.0 — the tool's channel is a governed `resource`, not a pinned
    // URL. Pinning `runtime: "http://…"` is now `axon-T949` (the third island).
    // The mock server's dynamic address lives in configuration exactly as a
    // production endpoint does — the `resource.endpoint` config key resolved via
    // `AXON_RESOURCE_<KEY>`, the same seam v2.67.0 established for `axonstore`.
    std::env::set_var("AXON_RESOURCE_MOCK_LLM", &mock_url);
    let src =
        "resource MockLlm { kind: http  endpoint: mock.llm  capacity: 4 }\n\
         tool MockLLM { provider: http  resource: MockLlm  effects: <stream:drop_oldest> }\n\
         flow Chat() -> Unit { step S { ask: \"hi\" apply: MockLLM } }\n\
         axonendpoint E { public: true method: POST path: \"/llm\" execute: Chat \
         backend: stub transport: sse }"
            .to_string();
    deploy(&app, &src).await;

    let h = hit(&app, "/llm", true).await;
    assert_eq!(h.status, StatusCode::OK);
    assert!(
        h.content_type.starts_with("text/event-stream"),
        "36.l: the streaming endpoint serves SSE, got {}",
        h.content_type
    );
    let body = String::from_utf8_lossy(&h.body);
    for token in ["axon-e2e-token-ALPHA", "axon-e2e-token-BRAVO", "axon-e2e-token-CHARLIE"] {
        assert!(
            body.contains(token),
            "36.l D12: the mock LLM server's streamed token `{token}` \
             must reach the SSE wire — proof the deploy → route → \
             dispatch → tool-stream path runs real HTTP I/O end-to-end. \
             Body: {body}"
        );
    }
}

// ════ section 3 — honest failure end-to-end ═══════════════════════════════

#[tokio::test]
async fn s3_unresolvable_endpoint_fails_honestly_end_to_end() {
    // Gated on the env carrying no provider keys (standard CI).
    if !env_available_backends().is_empty() {
        eprintln!(
            "36.l s3: skipped — provider API key(s) present ({:?}).",
            env_available_backends()
        );
        return;
    }
    let app = build_router(server_cfg());
    deploy(
        &app,
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat }",
    )
    .await;

    let h = hit(&app, "/chat", false).await;
    assert_eq!(
        h.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "36.l D5: an unresolvable endpoint fails with a structured 503 \
         — never the silent no-op the gap report observed"
    );
    assert_eq!(
        h.backend_header.as_deref(),
        Some("none; reason=no_backend_available")
    );
    let json: serde_json::Value = serde_json::from_slice(&h.body).unwrap();
    assert_eq!(json["error"], "no_backend_available");
}
