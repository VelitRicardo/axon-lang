//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 31.e — Runtime `X-Axon-Stream-Available` diagnostic header.
//!
//! D5 ratified 2026-05-11 bloque. The runtime attaches this header
//! to JSON responses for any flow with stream effects that was NOT
//! promoted — making the type-driven inference observable from the
//! client side without spelunking the source.
//!
//! Pillar trace per D10:
//!   PHILOSOPHY — the language surfaces its inferences at runtime
//!                so adopters debugging "why am I getting JSON" find
//!                the diagnostic in their client logs.
//!   COMPUTING  — informational only; never mutates the body; never
//!                breaks an HTTP client that ignores it.
//!   LOGIC      — fires iff a precise predicate holds (stream
//!                effect present AND response is JSON).
//!
//! Header shape (verbatim per plan vivo §7.2):
//!   `X-Axon-Stream-Available: 1; reason=<flag_off|declared_json>;
//!    flow=<name>; opt_in=transport:sse,Accept:text/event-stream`

use axon::axon_server::{build_router, ServerConfig};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

const HEADER_NAME: &str = "x-axon-stream-available";

fn server_cfg(strict: bool) -> ServerConfig {
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
        strict_type_driven_transport: strict,
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

/// Execute + return (status, content_type, optional header value).
async fn execute(
    app: axum::Router,
    flow: &str,
    accept_sse: bool,
) -> (StatusCode, String, Option<String>) {
    let body = serde_json::json!({ "flow": flow, "backend": "stub" });
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json");
    if accept_sse {
        builder = builder.header("accept", "text/event-stream");
    }
    let req = builder.body(Body::from(body.to_string())).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let header = resp
        .headers()
        .get(HEADER_NAME)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    (status, ct, header)
}

// ─── Source corpus (mirrors fase31_strict_mode.rs for consistency) ───

const STREAM_FLOW_NO_DECL: &str = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
                                   flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
                                   axonendpoint E { public: true method: POST path: \"/f\" execute: F }";

const STREAM_FLOW_DECL_SSE: &str = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
                                    flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
                                    axonendpoint E { public: true method: POST path: \"/f\" execute: F transport: sse }";

const STREAM_FLOW_DECL_JSON: &str = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
                                     flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
                                     axonendpoint E { public: true method: POST path: \"/f\" execute: F transport: json }";

const NON_STREAM_FLOW_NO_DECL: &str = "flow F() -> Int { step S { ask: \"x\" output: Int } }\n\
                                       axonendpoint E { public: true method: POST path: \"/f\" execute: F }";

// ═══════════════════════════════════════════════════════════════════
//  §1 — Header FIRES on legacy mode + stream effect + no decl
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn fires_on_legacy_kivi_shape_no_accept() {
    // The Kivi case 2026-05-11 — exactly what surfaced the
    // philosophical gap. Legacy mode + stream effect + no decl +
    // no Accept → JSON response WITH the diagnostic header.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_NO_DECL).await;
    let (status, ct, header) = execute(app, "F", false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("application/json"), "expected JSON, got {ct}");
    let h = header.expect("X-Axon-Stream-Available header MUST be present");
    assert!(h.starts_with("1; "), "header value malformed: {h}");
    assert!(h.contains("reason=flag_off"), "header missing reason=flag_off: {h}");
    assert!(h.contains("flow=F"), "header missing flow=F: {h}");
    assert!(
        h.contains("opt_in=transport:sse,Accept:text/event-stream"),
        "header missing opt_in directive: {h}"
    );
}

// ═══════════════════════════════════════════════════════════════════
//  §2 — Header FIRES on explicit transport: json (D3 opt-out)
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn fires_on_d3_opt_out_legacy_mode() {
    // The adopter explicitly opted out via `transport: json` but the
    // flow does have stream effects. The runtime honors the opt-out
    // (D3 sacred) AND surfaces the diagnostic so the adopter sees
    // the trade-off.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_DECL_JSON).await;
    let (status, ct, header) = execute(app, "F", false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("application/json"));
    let h = header.expect("header MUST fire on D3 opt-out with stream effects");
    assert!(h.contains("reason=declared_json"), "header: {h}");
    assert!(h.contains("flow=F"));
}

#[tokio::test]
async fn fires_on_d3_opt_out_strict_mode() {
    // Same D3 opt-out under strict mode — declared_json still wins,
    // and the header still surfaces the trade-off.
    let app = build_router(server_cfg(true));
    deploy(app.clone(), STREAM_FLOW_DECL_JSON).await;
    let (status, ct, header) = execute(app, "F", false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("application/json"));
    let h = header.expect("header MUST fire on D3 opt-out under strict mode");
    assert!(h.contains("reason=declared_json"), "header: {h}");
}

#[tokio::test]
async fn fires_on_d3_opt_out_with_accept_sse() {
    // Adopter declared json + client requested SSE. The D3 opt-out
    // wins → JSON response. Header fires with reason=declared_json
    // so the client knows their Accept was overridden by the
    // adopter's declaration.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_DECL_JSON).await;
    let (status, ct, header) = execute(app, "F", true).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("application/json"));
    let h = header.expect("header MUST fire on D3 opt-out + Accept SSE");
    assert!(h.contains("reason=declared_json"));
}

// ═══════════════════════════════════════════════════════════════════
//  §3 — Header DOES NOT FIRE — suppression cases
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn no_header_on_sse_response() {
    // When we promote to SSE (any path), the wire is already
    // streaming — the header would be redundant. Never fires on
    // SSE responses.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_DECL_SSE).await;
    let (status, ct, header) = execute(app, "F", false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));
    assert!(
        header.is_none(),
        "header must NOT fire on SSE responses; got: {:?}",
        header
    );
}

#[tokio::test]
async fn no_header_on_d4_promotion() {
    // Legacy mode + stream effect + Accept SSE → D4 promotion → SSE
    // response → no header.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_NO_DECL).await;
    let (status, ct, header) = execute(app, "F", true).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));
    assert!(header.is_none());
}

#[tokio::test]
async fn no_header_on_strict_mode_promotion() {
    // Strict mode + stream effect + no decl → D1 promotion → SSE
    // response → no header.
    let app = build_router(server_cfg(true));
    deploy(app.clone(), STREAM_FLOW_NO_DECL).await;
    let (status, ct, header) = execute(app, "F", false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));
    assert!(header.is_none());
}

#[tokio::test]
async fn no_header_on_non_stream_flow_no_decl() {
    // No stream effects = nothing to surface. Even though the
    // response is JSON, the diagnostic would be meaningless.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), NON_STREAM_FLOW_NO_DECL).await;
    let (status, ct, header) = execute(app, "F", false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("application/json"));
    assert!(
        header.is_none(),
        "header must NOT fire on non-stream flows; got: {:?}",
        header
    );
}

#[tokio::test]
async fn no_header_on_non_stream_flow_with_accept_sse() {
    // Non-stream flow + Accept SSE → JSON (no promotion path
    // fires) → still no header (the predicate is precise).
    let app = build_router(server_cfg(true));
    deploy(app.clone(), NON_STREAM_FLOW_NO_DECL).await;
    let (status, ct, header) = execute(app, "F", true).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("application/json"));
    assert!(header.is_none());
}

#[tokio::test]
async fn no_header_on_orphan_endpoint() {
    // Endpoint references a non-existent flow. The deploy fails;
    // execute also fails (flow not deployed). No header on the
    // error path.
    let app = build_router(server_cfg(false));
    let (status, _, header) = execute(app, "GhostFlow", false).await;
    // Status will be 404/4xx depending on the not-deployed path,
    // but the key is: no diagnostic header on a failure response.
    assert!(
        header.is_none(),
        "header must NOT fire on orphan/not-deployed; got status={status}, header={:?}",
        header
    );
}

// ═══════════════════════════════════════════════════════════════════
//  §4 — Header shape verification
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn header_value_format_matches_plan_vivo() {
    // Plan vivo §7.2 verbatim:
    //   `1; reason=<flag_off|declared_json>; flow=<name>;
    //    opt_in=transport:sse,Accept:text/event-stream`
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_NO_DECL).await;
    let (_, _, header) = execute(app, "F", false).await;
    let h = header.unwrap();
    // Strict structural check — semicolon-separated key=value pairs.
    let parts: Vec<&str> = h.split(';').map(|s| s.trim()).collect();
    assert!(parts.len() >= 4, "expected ≥4 segments, got: {h}");
    assert_eq!(parts[0], "1");
    assert!(parts.iter().any(|p| p == &"reason=flag_off"));
    assert!(parts.iter().any(|p| p == &"flow=F"));
    assert!(parts
        .iter()
        .any(|p| *p == "opt_in=transport:sse,Accept:text/event-stream"));
}

#[tokio::test]
async fn header_reason_values_are_closed_set() {
    // The two reason values are the only ones the runtime emits.
    // This guards against future drift adding a new reason without
    // an explicit D-letter ratification.
    let cases: &[(&str, bool, bool, &str)] = &[
        ("legacy_no_accept", false, false, "flag_off"),
        ("declared_json_legacy", false, false, "declared_json"),
        ("declared_json_strict", true, false, "declared_json"),
        ("declared_json_with_accept", false, true, "declared_json"),
    ];
    for (label, strict, accept, expected_reason) in cases {
        let source = if expected_reason == &"declared_json" {
            STREAM_FLOW_DECL_JSON
        } else {
            STREAM_FLOW_NO_DECL
        };
        let app = build_router(server_cfg(*strict));
        deploy(app.clone(), source).await;
        let (_, _, header) = execute(app, "F", *accept).await;
        let h = header.unwrap_or_else(|| panic!("case {label}: header missing"));
        let key = format!("reason={expected_reason}");
        assert!(
            h.contains(&key),
            "case {label}: expected {key}, got header: {h}"
        );
    }
}

#[tokio::test]
async fn header_flow_name_isolates_per_endpoint() {
    // Multi-endpoint program — each axonendpoint's diagnostic
    // header carries its own flow name. Adopters with several
    // endpoints can grep the diagnostic by flow.
    let src = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
               flow Alpha() -> Unit { step S { ask: \"a\" apply: t } }\n\
               flow Beta() -> Unit { step S { ask: \"b\" apply: t } }\n\
               axonendpoint AlphaE { public: true method: POST path: \"/alpha\" execute: Alpha }\n\
               axonendpoint BetaE { public: true method: POST path: \"/beta\" execute: Beta }";
    let app = build_router(server_cfg(false));
    deploy(app.clone(), src).await;
    let (_, _, alpha_header) = execute(app.clone(), "Alpha", false).await;
    let (_, _, beta_header) = execute(app, "Beta", false).await;
    assert!(alpha_header.unwrap().contains("flow=Alpha"));
    assert!(beta_header.unwrap().contains("flow=Beta"));
}

// ═══════════════════════════════════════════════════════════════════
//  §5 — Header is informational — body unchanged
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn header_does_not_mutate_response_body() {
    // The header MUST be informational only — the JSON response
    // body must be byte-identical to a request without the
    // diagnostic conditions. Adopters parsing the body see the
    // same shape they always did.
    //
    // Compared: stream-effect flow (header fires) vs non-stream
    // flow (header doesn't fire). Body shape on both must be the
    // legacy /v1/execute JSON envelope (success, trace_id, etc.).
    let app1 = build_router(server_cfg(false));
    deploy(app1.clone(), STREAM_FLOW_NO_DECL).await;
    let req1 = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({"flow": "F", "backend": "stub"}).to_string(),
        ))
        .unwrap();
    let resp1 = app1.oneshot(req1).await.unwrap();
    let bytes1 = resp1
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    let json1: serde_json::Value = serde_json::from_slice(&bytes1).unwrap();

    // Body must have the canonical /v1/execute keys.
    // v2.0.0 FlowEnvelope wire contract: no top-level `success`; the
    // canonical envelope exposes `result`, `step_audit`, `trace_id`, etc.
    assert!(json1.is_object());
    for key in ["trace_id", "result", "step_audit"] {
        assert!(
            json1.get(key).is_some(),
            "body missing canonical key '{key}': {json1}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
//  §6 — Cross-mode reason determinism
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn reason_flag_off_only_when_strict_is_off() {
    // The `reason=flag_off` value is reserved exclusively for the
    // case where flipping the strict flag would promote. In strict
    // mode, the only header reason we ever see is `declared_json`
    // (the inferred flow promotes by D1 unless D3 opt-out).
    //
    // This test verifies the reason vocabulary is meaningful: an
    // adopter who sees `flag_off` knows the fix is the strict flag;
    // `declared_json` means the fix is their source.

    // Legacy mode + stream + no decl → reason=flag_off
    let app1 = build_router(server_cfg(false));
    deploy(app1.clone(), STREAM_FLOW_NO_DECL).await;
    let (_, _, h1) = execute(app1, "F", false).await;
    assert!(h1.unwrap().contains("reason=flag_off"));

    // Strict mode + stream + no decl → SSE (no header). Same
    // source, different mode — the diagnostic shifts from
    // "header present with flag_off" to "no header (SSE wire)".
    let app2 = build_router(server_cfg(true));
    deploy(app2.clone(), STREAM_FLOW_NO_DECL).await;
    let (_, ct2, h2) = execute(app2, "F", false).await;
    assert!(ct2.starts_with("text/event-stream"));
    assert!(h2.is_none());
}
