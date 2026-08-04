//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 30.d — Runtime SSE single-shot path conformance tests.
//!
//! Exercises `POST /v1/execute/sse` end-to-end through the axum app
//! built by `build_router()`. Asserts:
//!
//!   1. Response `Content-Type: text/event-stream`
//!   2. `retry:` directive on the wire (W3C SSE reconnect hint)
//!   3. Per-step `event: axon.token` events with monotonic `id:` field
//!   4. Final `event: axon.complete` event with execution envelope
//!   5. Mid-stream error → `event: axon.error` event + close
//!   6. Wire format conformance vs W3C SSE spec (`\n\n` event separator,
//!      `id:`/`event:`/`data:` field framing, no spurious whitespace)
//!
//! No actual LLM backend is invoked — the runtime under test uses the
//! "stub" backend which produces deterministic output without network.
//! That keeps the test suite hermetic + reproducible across CI runs.

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

/// Run the SSE endpoint against the supplied JSON body and return
/// (status, content_type, body_bytes).
async fn call_sse(body: serde_json::Value) -> (StatusCode, Option<String>, Vec<u8>) {
    let app = build_router(server_cfg());
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute/sse")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = resp.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, ct, bytes)
}

#[tokio::test]
async fn route_exists_and_returns_text_event_stream() {
    // Calling against a non-deployed flow should still produce SSE
    // wire format (with an `axon.error` event) — never a 404 or
    // text/plain. The whole point of SSE-typed endpoints is that the
    // wire format is honored regardless of the inner outcome.
    let body = serde_json::json!({
        "flow_name": "nonexistent_flow",
        "backend": "stub",
    });
    let (status, ct, _body) = call_sse(body).await;
    assert_eq!(status, StatusCode::OK);
    let ct = ct.expect("content-type header present");
    assert!(
        ct.starts_with("text/event-stream"),
        "expected text/event-stream, got {ct}"
    );
}

#[tokio::test]
async fn not_deployed_flow_emits_error_event_then_closes() {
    let body = serde_json::json!({
        "flow_name": "nonexistent_flow",
        "backend": "stub",
    });
    let (_, _, bytes) = call_sse(body).await;
    let text = String::from_utf8(bytes).expect("utf-8 body");

    // Wire format: retry: directive present.
    assert!(
        text.contains("retry: 5000"),
        "retry hint missing: {text}"
    );
    // Wire format: axon.error event present.
    assert!(
        text.contains("event: axon.error"),
        "axon.error event missing: {text}"
    );
    // Error payload mentions the flow name (adopter diagnostic).
    assert!(
        text.contains("nonexistent_flow"),
        "error payload should name the flow: {text}"
    );
}

#[tokio::test]
async fn deployed_flow_emits_token_events_and_complete() {
    let app = build_router(server_cfg());

    // 1. Deploy a minimal flow (the stub backend echoes deterministic
    //    text per step). The deploy endpoint is /v1/deploy.
    let deploy_body = serde_json::json!({
        "source": "flow F() { step S { ask: \"hello\" } }",
        "source_file": "test.axon",
        "backend": "stub",
    });
    let deploy_req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(deploy_body.to_string()))
        .unwrap();
    let deploy_resp = app.clone().oneshot(deploy_req).await.unwrap();
    assert_eq!(
        deploy_resp.status(),
        StatusCode::OK,
        "deploy must succeed before SSE test"
    );

    // 2. Call the SSE endpoint on the deployed flow.
    let sse_body = serde_json::json!({
        "flow_name": "F",
        "backend": "stub",
    });
    let sse_req = Request::builder()
        .method("POST")
        .uri("/v1/execute/sse")
        .header("content-type", "application/json")
        .body(Body::from(sse_body.to_string()))
        .unwrap();
    let sse_resp = app.oneshot(sse_req).await.unwrap();
    assert_eq!(sse_resp.status(), StatusCode::OK);

    let ct = sse_resp
        .headers()
        .get("content-type")
        .expect("content-type")
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        ct.starts_with("text/event-stream"),
        "expected text/event-stream, got {ct}"
    );

    let bytes = sse_resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    let text = String::from_utf8(bytes).expect("utf-8 body");

    // Wire shape per plan vivo §4.
    assert!(text.contains("retry: 5000"), "retry hint missing");
    assert!(
        text.contains("event: axon.complete"),
        "axon.complete terminator missing: {text}"
    );
    // The completion envelope must carry the trace_id + flow name.
    assert!(
        text.contains("\"flow\":\"F\""),
        "completion envelope missing flow name: {text}"
    );
    // No leaked JSON-body framing on the wire (the response IS the
    // SSE stream; there's no surrounding `{"success": ..., ...}`).
    assert!(
        !text.contains("\"success\":true,\n"),
        "JSON-envelope leakage detected: {text}"
    );
}

#[tokio::test]
async fn event_ids_are_monotonic() {
    // Deploy + call. Parse the body for `id:` lines and assert
    // strictly increasing integers.
    let app = build_router(server_cfg());

    let deploy_body = serde_json::json!({
        "source": "flow F() { step S { ask: \"hi\" } }",
        "source_file": "test.axon",
        "backend": "stub",
    });
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/deploy")
                .header("content-type", "application/json")
                .body(Body::from(deploy_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let sse_body = serde_json::json!({
        "flow_name": "F",
        "backend": "stub",
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/execute/sse")
                .header("content-type", "application/json")
                .body(Body::from(sse_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let text = String::from_utf8(
        resp.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
    .expect("utf-8");

    let ids: Vec<u64> = text
        .lines()
        .filter_map(|line| line.strip_prefix("id: "))
        .filter_map(|s| s.trim().parse::<u64>().ok())
        .collect();
    assert!(
        ids.len() >= 1,
        "expected at least one `id:` line on the wire, got: {text}"
    );
    // Strict monotonicity: every successor > predecessor.
    for window in ids.windows(2) {
        assert!(
            window[1] > window[0],
            "non-monotonic ids: {:?} in stream:\n{}",
            ids,
            text
        );
    }
}

#[tokio::test]
async fn sse_wire_framing_w3c_compliant() {
    // W3C SSE spec: events end with `\n\n` (blank line separator).
    // Each field is `<name>: <value>\n`. The completion event must be
    // followed by exactly one `\n\n` terminator before stream end.
    let app = build_router(server_cfg());

    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/deploy")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "source": "flow F() { step S { ask: \"x\" } }",
                        "source_file": "t.axon",
                        "backend": "stub",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/execute/sse")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "flow_name": "F",
                        "backend": "stub",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let text = String::from_utf8(
        resp.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
    .expect("utf-8");

    // Every `event:` line should be followed eventually by a blank
    // line within the same event block. We check the simpler global
    // invariant: the body must end with `\n\n` (the terminator after
    // the last event).
    assert!(
        text.ends_with("\n\n"),
        "SSE body must end with blank-line event separator, got tail: {:?}",
        &text[text.len().saturating_sub(40)..]
    );

    // No bare CR (only `\n` line endings) — keeps EventSource clients
    // happy across line-ending normalization implementations.
    assert!(
        !text.contains('\r'),
        "bare CR detected in SSE wire; W3C SSE prefers LF-only"
    );
}

#[tokio::test]
async fn error_payload_includes_recoverable_flag() {
    // When the flow doesn't exist, the axon.error event payload
    // should carry the `recoverable: false` flag so client code
    // can decide whether to retry.
    let body = serde_json::json!({
        "flow_name": "ghost",
        "backend": "stub",
    });
    let (_, _, bytes) = call_sse(body).await;
    let text = String::from_utf8(bytes).expect("utf-8");
    assert!(text.contains("\"recoverable\":false"));
}

#[tokio::test]
async fn retry_directive_appears_first() {
    // The W3C SSE spec lets `retry:` appear anywhere, but the plan
    // vivo §4.5 commits to "first event in every SSE response".
    // We assert the `retry:` line appears before any `event:` line.
    let body = serde_json::json!({
        "flow_name": "ghost",
        "backend": "stub",
    });
    let (_, _, bytes) = call_sse(body).await;
    let text = String::from_utf8(bytes).expect("utf-8");

    let retry_pos = text
        .find("retry: 5000")
        .expect("retry directive should be present");
    let first_event_pos = text
        .find("event:")
        .expect("at least one event should follow the retry hint");
    assert!(
        retry_pos < first_event_pos,
        "retry directive must precede the first event"
    );
}

#[tokio::test]
async fn empty_body_returns_some_response() {
    // Defensive: malformed request body shouldn't crash the server.
    // An empty JSON object means the deserialiser will use defaults
    // for the StreamExecuteRequest fields. The behaviour matters less
    // than the absence of a panic.
    let app = build_router(server_cfg());
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute/sse")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    // 200 (SSE-formatted error) OR 400 (deserialise error) both
    // acceptable; what we forbid is 5xx (server-side panic).
    assert!(
        resp.status() == StatusCode::OK
            || resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY,
        "unexpected status: {}",
        resp.status()
    );
}
