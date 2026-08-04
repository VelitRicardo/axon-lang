//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 36.j (D8) — resolution observability.
//!
//! "Why this model?" must always be answerable post-hoc. 36.j
//! surfaces the resolved backend AND the precedence rung that chose
//! it on two observability surfaces of every dynamic-route response:
//!
//!   - the `X-Axon-Backend` response header — `<backend>;
//!     reason=<rung>` — uniform across the JSON and SSE wires;
//!   - a `backend_resolution` object injected into the JSON wire
//!     body. For a replay-enabled route the injected body is what the
//!     replay log persists, so `GET /v1/replay/<trace_id>` carries
//!     the resolution into the audit trail too.
//!
//! Pins:
//!   1. `inject_backend_resolution` adds the object to a JSON object
//!      body; leaves a non-object body byte-for-byte untouched.
//!   2. A JSON dynamic-route response carries the `X-Axon-Backend`
//!      header AND the `backend_resolution` body object — with the
//!      rung that actually fired (`endpoint_declared`,
//!      `server_default`, …).
//!   3. The SSE response carries the `X-Axon-Backend` header.
//!   4. The honest failure carries `X-Axon-Backend: none;
//!      reason=no_backend_available`.

use axon::axon_server::{build_router, inject_backend_resolution, ServerConfig};
use axon::backends::env_available_backends;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

// ─── §1 — inject_backend_resolution (pure) ─────────────────────────

#[test]
fn s1_injects_into_a_json_object_body() {
    let body = br#"{"success":true,"backend":"gemini"}"#;
    let out = inject_backend_resolution(body, "gemini", "endpoint_declared");
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["backend_resolution"]["backend"], "gemini");
    assert_eq!(json["backend_resolution"]["reason"], "endpoint_declared");
    // existing keys preserved
    assert_eq!(json["success"], true);
    assert_eq!(json["backend"], "gemini");
}

#[test]
fn s1_leaves_a_non_object_body_untouched() {
    for raw in [
        &b"[1,2,3]"[..],
        &b"\"a scalar\""[..],
        &b"not json at all"[..],
        &b""[..],
    ] {
        let out = inject_backend_resolution(raw, "stub", "server_default");
        assert_eq!(
            out, raw,
            "36.j: a non-object body must be returned byte-for-byte \
             untouched — observability is best-effort, never corrupting"
        );
    }
}

// ─── integration helpers ───────────────────────────────────────────

fn server_cfg(default_backend: Option<&str>) -> ServerConfig {
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
        default_backend: default_backend.map(|s| s.to_string()),
        schemas_dir: None,
    }
}

async fn deploy(app: &axum::Router, src: &str) {
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
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(status, StatusCode::OK, "deploy failed: {json}");
    assert_eq!(json.get("success").and_then(|v| v.as_bool()), Some(true), "{json}");
}

async fn hit(
    app: &axum::Router,
    path: &str,
    accept_sse: bool,
) -> (StatusCode, Option<String>, Vec<u8>) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json");
    if accept_sse {
        builder = builder.header("accept", "text/event-stream");
    }
    let resp = app.clone().oneshot(builder.body(Body::from("{}")).unwrap()).await.unwrap();
    let status = resp.status();
    let backend_hdr = resp
        .headers()
        .get("x-axon-backend")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = resp.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, backend_hdr, bytes)
}

// ─── §2 — JSON response: header + body backend_resolution ──────────

#[tokio::test]
async fn s2_json_response_carries_header_and_body_resolution() {
    let app = build_router(server_cfg(None));
    deploy(
        &app,
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat backend: stub }",
    )
    .await;

    let (status, backend_hdr, body) = hit(&app, "/chat", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        backend_hdr.as_deref(),
        Some("stub; reason=endpoint_declared"),
        "36.j D8: the X-Axon-Backend header must carry `<backend>; \
         reason=<rung>` — here the route declared `backend: stub`"
    );
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["backend_resolution"]["backend"], "stub");
    assert_eq!(
        json["backend_resolution"]["reason"], "endpoint_declared",
        "36.j D8: the JSON body must carry the `backend_resolution` \
         object naming the rung. Body: {json}"
    );
}

// ─── §3 — server-default route: reason is `server_default` ─────────

#[tokio::test]
async fn s3_server_default_route_reports_the_server_default_rung() {
    let app = build_router(server_cfg(Some("stub")));
    deploy(
        &app,
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat }",
    )
    .await;

    let (status, backend_hdr, body) = hit(&app, "/chat", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        backend_hdr.as_deref(),
        Some("stub; reason=server_default"),
        "36.j D8: an undeclared route resolved by the server default \
         must report `reason=server_default`"
    );
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["backend_resolution"]["reason"], "server_default");
}

// ─── §4 — SSE response carries the X-Axon-Backend header ───────────

#[tokio::test]
async fn s4_sse_response_carries_the_backend_header() {
    let app = build_router(server_cfg(None));
    deploy(
        &app,
        "flow Stream() -> Unit { step S { ask: \"hi\" output: Stream<Token> } }\n\
         axonendpoint E { public: true method: POST path: \"/stream\" execute: Stream \
         backend: stub transport: sse }",
    )
    .await;

    let (status, backend_hdr, _body) = hit(&app, "/stream", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        backend_hdr.as_deref(),
        Some("stub; reason=endpoint_declared"),
        "36.j D8: the SSE response must carry the same X-Axon-Backend \
         header — observability is uniform across the JSON and SSE wires"
    );
}

// ─── §5 — honest failure reports `no_backend_available` ────────────

#[tokio::test]
async fn s5_honest_failure_header_reports_no_backend_available() {
    // Reached only when every ladder rung is empty — the standard CI
    // condition (no provider API keys). Gated like 36.h's s3.
    if !env_available_backends().is_empty() {
        eprintln!(
            "36.j s5: skipped — provider API key(s) present ({:?}); the \
             route resolves and the honest-failure path is not taken.",
            env_available_backends()
        );
        return;
    }
    let app = build_router(server_cfg(None));
    deploy(
        &app,
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/chat\" execute: Chat }",
    )
    .await;

    let (status, backend_hdr, _body) = hit(&app, "/chat", false).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        backend_hdr.as_deref(),
        Some("none; reason=no_backend_available"),
        "36.j D8: the honest failure must still answer 'why?' — the \
         X-Axon-Backend header reports `none; reason=no_backend_available`"
    );
}
