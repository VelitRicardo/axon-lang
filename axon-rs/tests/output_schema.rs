//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.23.0 — Output schema validation gate end-to-end tests.
//!
//! D5 ratificada 2026-05-11. Verifies the response-side schema
//! validation gate consumes the SAME `validate_body` primitive that
//! 32.c uses on the request side, with the **two architectural
//! differences** the D5 contract mandates:
//!
//!   1. **OWASP-safe 500**: validation failure returns a GENERIC
//!      `{error: "internal_validation_error", trace_id, hint,
//!      d_letter: "D5"}` to the client. Schema details
//!      (expected_type, field_path, expected, got) are NEVER
//!      serialised into the client response — leaking the contract
//!      to a potentially malicious caller is a recon vector.
//!   2. **Audit trail captures the full diagnostic**: the
//!      `BodyValidationError` is recorded verbatim in `audit_log`
//!      with `event: "output_schema_violation"` + `d_letter: "D5"`
//!      so the adopter inspects the trail to fix the FLOW.
//!
//! Pillar trace per D12:
//!   - MATHEMATICS — same pure + total `validate_body` primitive.
//!   - PHILOSOPHY — the declared `output:` IS the contract;
//!                   downstream consumers can trust the schema.
//!   - LOGIC      — no widening; flows producing a wider response
//!                   fail loudly at the audit boundary.
//!   - COMPUTING  — OWASP-aligned: client sees only `trace_id`,
//!                   never schema details.
//!
//! Coverage:
//!   - D9 backwards-compat: no `output:` declared → response passes
//!     through unchanged.
//!   - SSE/ndjson Content-Type → gate is a no-op (streaming bodies
//! can't be statically validated; deferred to a future cycle).
//!   - Non-2xx response → passes through (error path, not flow output).
//!   - Empty `output:` declaration → passes through.
//!   - `output: Any` → trivially accepts any response.
//!   - Schema-mismatched `output:` → 500 generic + audit entry.
//!   - Client response shape: `{error, trace_id, hint, d_letter}`
//!     ONLY — never the schema diagnostic.
//!   - Audit log entry shape: contains `event:
//!     "output_schema_violation"`, the violation triple, and the
//!     same `trace_id` the client received (correlation anchor).

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

async fn deploy(app: axum::Router, src: &str) {
    let body = serde_json::json!({
        "source": src,
        "source_file": "test.axon",
        "backend": "stub",
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(status, StatusCode::OK, "deploy failed: {json}");
    assert_eq!(
        json.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "deploy success=false: {json}"
    );
}

/// Helper: build a request shaped to hit the dynamic endpoint at
/// (POST, path). Sends an empty JSON object as the body since the
/// test programs in this file don't declare `body:` — D9 path applies
/// on the request side, so the body validation gate is a no-op and
/// what we're exercising is purely the RESPONSE-side gate.
fn post_to(path: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap()
}

// ── D9 backwards-compat ──────────────────────────────────────────────

#[tokio::test]
async fn d9_no_output_declaration_passes_response_through() {
    let app = build_router(server_cfg());
    let src = "flow Ping() -> String { let result = \"pong\" return result }\n\
               axonendpoint PingEndpoint { public: true method: POST path: \"/ping\" execute: Ping }";
    deploy(app.clone(), src).await;

    let resp = app.oneshot(post_to("/ping")).await.expect("request");
    // Validation never fires — status whatever the dispatch produces,
    // but NEVER 500 internal_validation_error.
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_ne!(
        payload["error"], "internal_validation_error",
        "D9: no output: declared, validation gate must NOT fire (status={status})"
    );
}

// ── Output: Any — trivially passes ──────────────────────────────────

#[tokio::test]
async fn output_any_accepts_response_unchanged() {
    let app = build_router(server_cfg());
    let src = "flow Ping() -> String { let result = \"pong\" return result }\n\
               axonendpoint AnyEndpoint { public: true method: POST path: \"/any\" execute: Ping output: Any }";
    deploy(app.clone(), src).await;

    let resp = app.oneshot(post_to("/any")).await.expect("request");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_ne!(
        payload["error"], "internal_validation_error",
        "output: Any must accept the response"
    );
}

// ── Schema mismatch fires the gate ──────────────────────────────────

#[tokio::test]
async fn output_string_rejects_object_response_with_owasp_500() {
    // The dispatched handler returns an ExecutionResult envelope
    // (JSON object). Declaring `output: String` declares "I produce
    // a JSON string" — but the actual response is an OBJECT, not a
    // string. The validator catches the mismatch and the gate fires
    // a generic 500 with NO schema details exposed.
    let app = build_router(server_cfg());
    // v2.0.0 wire contract: json transport requires `FlowEnvelope<T>`.
    // The flow declares `-> Integer` (so the endpoint compiles) and returns a
    // stub STEP's output: on the stub backend `Compute.output` is the STRING
    // `"(stub)"`, so the envelope's `result` slot fails to validate against the
    // inner `Integer`, firing the D5 output-schema gate.
    //
    // (v2.15.0: the unified dispatcher actually EXECUTES the flow, so the
    // pre-v2.15.0 form `let result = 1 return result` now correctly yields the
    // Integer `1` and validates — no longer a violation. We drive the genuine
    // mismatch through a stub step's string output instead, exercising the
    // SAME D5 gate.)
    let src = "flow Ping() -> Integer { step Compute { ask: \"n\" } return Compute.output }\n\
               axonendpoint Wrong { public: true method: POST path: \"/wrong\" execute: Ping output: FlowEnvelope<Integer> }";
    deploy(app.clone(), src).await;

    let resp = app.oneshot(post_to("/wrong")).await.expect("request");
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(payload["error"], "internal_validation_error");
    assert_eq!(payload["d_letter"], "D5");
    // OWASP — the client must NEVER see schema details.
    assert!(
        payload.get("expected_type").is_none(),
        "schema detail leaked to client"
    );
    assert!(
        payload.get("field_path").is_none(),
        "schema detail leaked to client"
    );
    assert!(
        payload.get("expected").is_none(),
        "schema detail leaked to client"
    );
    assert!(
        payload.get("got").is_none(),
        "schema detail leaked to client"
    );
    // The trace_id is the correlation anchor between client + audit.
    assert!(payload["trace_id"].is_string(), "trace_id must be present");
}

// ── Audit log captures the full diagnostic ──────────────────────────

#[tokio::test]
async fn output_schema_violation_recorded_in_audit_log_with_full_diagnostic() {
    let app = build_router(server_cfg());
    // See `output_string_rejects_object_response_with_owasp_500` — the flow
    // declares `Integer` but returns a stub STEP's STRING output `"(stub)"`, so
    // the `FlowEnvelope<Integer>` inner-T validation fails at the D5 gate.
    let src = "flow Ping() -> Integer { step Compute { ask: \"n\" } return Compute.output }\n\
               axonendpoint Wrong { public: true method: POST path: \"/wrong-audit\" execute: Ping output: FlowEnvelope<Integer> }";
    deploy(app.clone(), src).await;

    // Get the audit log starting size so we can find OUR entry.
    let pre_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/audit")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("audit-log GET");
    let pre_bytes = pre_resp.into_body().collect().await.unwrap().to_bytes();
    let pre_entries: serde_json::Value =
        serde_json::from_slice(&pre_bytes).unwrap_or_default();
    let pre_count = pre_entries["entries"].as_array().map(|a| a.len()).unwrap_or(0);

    // Trigger the violation.
    let resp = app.clone().oneshot(post_to("/wrong-audit")).await.expect("violation");
    let client_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let client_payload: serde_json::Value =
        serde_json::from_slice(&client_bytes).unwrap_or_default();
    let trace_id = client_payload["trace_id"].as_str().unwrap().to_string();

    // Now fetch the audit log and find the violation entry by trace_id.
    let post_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/audit")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("audit-log GET");
    let post_bytes = post_resp.into_body().collect().await.unwrap().to_bytes();
    let post_entries: serde_json::Value =
        serde_json::from_slice(&post_bytes).unwrap_or_default();
    let entries = post_entries["entries"].as_array().expect("audit entries");
    assert!(
        entries.len() > pre_count,
        "audit log must have grown after the violation"
    );

    // Find the entry where detail.trace_id == our trace_id AND
    // event == "output_schema_violation".
    let violation = entries
        .iter()
        .find(|e| {
            e["detail"]["trace_id"].as_str() == Some(&trace_id)
                && e["detail"]["event"] == "output_schema_violation"
        })
        .expect("violation entry must be in audit log");

    // Full diagnostic must be present in audit log (the adopter
    // inspects this to fix the FLOW).
    let detail = &violation["detail"];
    assert_eq!(detail["d_letter"], "D5");
    assert_eq!(detail["endpoint"], "Wrong");
    assert_eq!(detail["flow_name"], "Ping");
    assert_eq!(detail["method"], "POST");
    assert_eq!(detail["path"], "/wrong-audit");
    assert!(detail["expected_type"].is_string());
    assert!(detail["field_path"].is_string());
    assert!(detail["expected"].is_string());
    assert!(detail["got"].is_string());
    assert!(detail["hint"].as_str().unwrap().contains("Body field"));
    // The audit entry is recorded with success=false.
    assert_eq!(violation["success"], false);
}

// ── SSE/ndjson responses pass through (streaming N/A) ───────────────

#[tokio::test]
async fn sse_response_bypasses_output_validation_gate() {
    // SSE / token-by-token streams cannot be validated against a
    // static `output: T` at the wire layer — each event is a chunk,
    // not the full T. The gate must be a no-op for stream responses.
    let app = build_router(server_cfg());
    let src = "tool chat_token_stream { description: \"stream\" effects: <stream:drop_oldest> }\n\
               flow Chat() -> Unit { step Generate { ask: \"hi\" apply: chat_token_stream } }\n\
               axonendpoint ChatSse { public: true method: POST path: \"/chat-sse\" execute: Chat transport: sse output: String }";
    deploy(app.clone(), src).await;

    let resp = app.oneshot(post_to("/chat-sse")).await.expect("request");
    // SSE response: status 200, Content-Type text/event-stream.
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("text/event-stream"),
        "expected SSE wire format on declared transport:sse endpoint (got '{ct}')"
    );
    // The body should NOT be the validation error envelope — the
    // gate passed through.
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let txt = String::from_utf8_lossy(&bytes);
    assert!(
        !txt.contains("internal_validation_error"),
        "SSE response must bypass the output validation gate"
    );
}

// ── Non-existent flow path: error path passes through ──────────────

#[tokio::test]
async fn unknown_path_404_passes_through_not_500_validation_error() {
    let app = build_router(server_cfg());
    // No deploy — every dynamic-route lookup misses.
    let resp = app.oneshot(post_to("/does-not-exist")).await.expect("request");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(payload["error"], "axonendpoint_not_found");
    // CRITICAL: 404 from the 32.b fallback must NOT be misclassified
    // as a 32.d validation error. The two error envelopes are
    // distinct and must remain so.
    assert_ne!(payload["error"], "internal_validation_error");
}

// ── Trace ID is a UUID v4 ──────────────────────────────────────────

#[tokio::test]
async fn trace_id_in_500_envelope_is_uuid_v4_shape() {
    let app = build_router(server_cfg());
    // See `output_string_rejects_object_response_with_owasp_500` — the flow
    // declares `Integer` but returns a stub STEP's STRING output `"(stub)"`, so
    // the `FlowEnvelope<Integer>` inner-T validation fails at the D5 gate.
    let src = "flow Ping() -> Integer { step Compute { ask: \"n\" } return Compute.output }\n\
               axonendpoint Wrong { public: true method: POST path: \"/wrong-uuid\" execute: Ping output: FlowEnvelope<Integer> }";
    deploy(app.clone(), src).await;

    let resp = app.oneshot(post_to("/wrong-uuid")).await.expect("request");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    let trace_id = payload["trace_id"].as_str().expect("trace_id present");
    // UUID v4 canonical shape: 8-4-4-4-12 hex digits with `4` in pos 14
    // and `8|9|a|b` in pos 19. Hand-rolled validator (no regex dep).
    assert_eq!(trace_id.len(), 36, "UUID v4 is 36 chars");
    let bytes = trace_id.as_bytes();
    assert_eq!(bytes[8], b'-');
    assert_eq!(bytes[13], b'-');
    assert_eq!(bytes[18], b'-');
    assert_eq!(bytes[23], b'-');
    assert_eq!(bytes[14], b'4', "UUID v4 marker");
    assert!(
        matches!(bytes[19], b'8' | b'9' | b'a' | b'b'),
        "UUID v4 variant byte"
    );
    for (i, b) in bytes.iter().enumerate() {
        if matches!(i, 8 | 13 | 18 | 23) {
            continue;
        }
        assert!(
            b.is_ascii_hexdigit(),
            "char at {i} must be hex digit: {trace_id}"
        );
    }
}

// ── D9 + output: Any path coexistence with body validation ──────────

#[tokio::test]
async fn body_and_output_validation_compose_independently() {
    // Both `body: T` (32.c) AND `output: U` (32.d) declared. Sending
    // a valid body must pass the request gate, then the response is
    // validated against U. With `output: Any`, the response passes.
    let app = build_router(server_cfg());
    let src = "type PingReq { msg: String }\n\
               flow Ping() -> String { let result = \"pong\" return result }\n\
               axonendpoint Both { public: true method: POST path: \"/both\" execute: Ping body: PingReq output: Any }";
    deploy(app.clone(), src).await;

    // Well-formed body — passes 32.c gate.
    let body = serde_json::json!({"msg": "hi"});
    let req = Request::builder()
        .method("POST")
        .uri("/both")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.expect("request");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_ne!(payload["error"], "body_schema_violation");
    assert_ne!(payload["error"], "internal_validation_error");

    // Malformed body — 32.c gate fires FIRST. 32.d never runs because
    // the request never reaches dispatch. Verifies the gates compose
    // in the correct order.
    let bad = serde_json::json!({"wrong_field": "x"});
    let req = Request::builder()
        .method("POST")
        .uri("/both")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&bad).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.expect("request");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(payload["error"], "body_schema_violation");
}
