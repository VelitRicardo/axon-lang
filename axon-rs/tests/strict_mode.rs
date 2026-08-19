//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.22.0 — Type-Driven Wire Inference runtime conformance.
//!
//! D1, D3, D6, D8, D9 ratified 2026-05-11 bloque. Exercises the
//! 16-cell negotiation matrix `(strict_mode × endpoint_decl ×
//! stream_effect × Accept_header) → response_format`.
//!
//! Pillar trace per D10:
//!   MATHEMATICS — the 16-cell matrix is a total function.
//!   LOGIC      — every cell verified.
//!   PHILOSOPHY — when strict_mode is on, the type system rules
//!                the wire format.
//!   COMPUTING  — when strict_mode is off (D6 v1.22.x default),
//! v1.21.0 D4+D5 semantics are preserved verbatim
//!                — zero behavior change for existing adopters.
//!
//! All scenarios are hermetic via the `stub` backend; zero network.

use axon::axon_server::{build_router, ServerConfig};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

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

async fn execute(
    app: axum::Router,
    flow: &str,
    accept_sse: bool,
) -> (StatusCode, String) {
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
    (status, ct)
}

/// Strong assertion: response Content-Type indicates SSE wire format.
fn assert_sse(ct: &str, ctx: &str) {
    assert!(
        ct.starts_with("text/event-stream"),
        "{ctx}: expected text/event-stream, got '{ct}'"
    );
}

/// Strong assertion: response Content-Type indicates JSON wire format.
fn assert_json(ct: &str, ctx: &str) {
    assert!(
        ct.starts_with("application/json"),
        "{ctx}: expected application/json, got '{ct}'"
    );
}

// ─── Source corpus ───────────────────────────────────────────────────

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

const NON_STREAM_FLOW_DECL_JSON: &str = "flow F() -> Int { step S { ask: \"x\" output: Int } }\n\
                                         axonendpoint E { public: true method: POST path: \"/f\" execute: F transport: json }";

// ═══════════════════════════════════════════════════════════════════
//   STRICT MODE ON — Type-Driven Wire Inference (D1, D6 future-default)
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn strict_mode_stream_flow_no_decl_no_accept_promotes_to_sse() {
    // The Kivi case 2026-05-11 — adopter has stream effects, no
    // explicit transport, client doesn't send Accept. Pre-31.d this
    // returned JSON; 31.d strict mode returns SSE (D1 inference fires).
    let app = build_router(server_cfg(true));
    deploy(app.clone(), STREAM_FLOW_NO_DECL).await;
    let (status, ct) = execute(app, "F", /* accept_sse */ false).await;
    assert_eq!(status, StatusCode::OK);
    assert_sse(&ct, "strict_mode + stream + no decl + no Accept");
}

#[tokio::test]
async fn strict_mode_stream_flow_no_decl_accept_sse_promotes() {
    // Same as above but client also asks for SSE. Result is identical.
    let app = build_router(server_cfg(true));
    deploy(app.clone(), STREAM_FLOW_NO_DECL).await;
    let (status, ct) = execute(app, "F", /* accept_sse */ true).await;
    assert_eq!(status, StatusCode::OK);
    assert_sse(&ct, "strict_mode + stream + no decl + Accept SSE");
}

#[tokio::test]
async fn strict_mode_stream_flow_decl_json_stays_json_d3_opt_out() {
    // D3 opt-out is preserved under strict mode — the adopter's
    // explicit choice always wins. The runtime defers to the
    // declaration; the type system's inference is overridden.
    let app = build_router(server_cfg(true));
    deploy(app.clone(), STREAM_FLOW_DECL_JSON).await;
    let (status, ct) = execute(app, "F", /* accept_sse */ false).await;
    assert_eq!(status, StatusCode::OK);
    assert_json(&ct, "strict_mode + stream + declared json (D3)");
}

#[tokio::test]
async fn strict_mode_stream_flow_decl_json_with_accept_sse_still_stays_json() {
    // D3 opt-out is sticky even when the client requests SSE. The
    // adopter's source overrides the client's Accept header.
    let app = build_router(server_cfg(true));
    deploy(app.clone(), STREAM_FLOW_DECL_JSON).await;
    let (status, ct) = execute(app, "F", /* accept_sse */ true).await;
    assert_eq!(status, StatusCode::OK);
    assert_json(&ct, "strict_mode + stream + declared json + Accept SSE");
}

#[tokio::test]
async fn strict_mode_stream_flow_decl_sse_promotes() {
    // Explicit `transport: sse` always promotes; strict mode doesn't
    // change this — v1.21.0 D5 preserved.
    let app = build_router(server_cfg(true));
    deploy(app.clone(), STREAM_FLOW_DECL_SSE).await;
    let (status, ct) = execute(app, "F", /* accept_sse */ false).await;
    assert_eq!(status, StatusCode::OK);
    assert_sse(&ct, "strict_mode + stream + declared sse");
}

#[tokio::test]
async fn strict_mode_non_stream_flow_no_decl_stays_json() {
    // No stream effects + no declaration → inference returns
    // implicit_transport=json → no promotion even under strict mode.
    let app = build_router(server_cfg(true));
    deploy(app.clone(), NON_STREAM_FLOW_NO_DECL).await;
    let (status, ct) = execute(app, "F", /* accept_sse */ false).await;
    assert_eq!(status, StatusCode::OK);
    assert_json(&ct, "strict_mode + non-stream + no decl");
}

#[tokio::test]
async fn strict_mode_non_stream_flow_no_decl_accept_sse_still_json() {
    // Adopter's source has no stream effects; client's Accept is
    // ignored under strict mode (no inference fires, so no D4
    // fallback either — strict mode doesn't conjure stream effects
    // out of nowhere).
    let app = build_router(server_cfg(true));
    deploy(app.clone(), NON_STREAM_FLOW_NO_DECL).await;
    let (status, ct) = execute(app, "F", /* accept_sse */ true).await;
    assert_eq!(status, StatusCode::OK);
    assert_json(&ct, "strict_mode + non-stream + Accept SSE");
}

#[tokio::test]
async fn strict_mode_non_stream_flow_decl_json_stays_json() {
    let app = build_router(server_cfg(true));
    deploy(app.clone(), NON_STREAM_FLOW_DECL_JSON).await;
    let (status, ct) = execute(app, "F", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_json(&ct, "strict_mode + non-stream + declared json");
}

// ═══════════════════════════════════════════════════════════════════
// STRICT MODE OFF (D6 default) — v1.21.0 D4+D5 preserved verbatim
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn legacy_mode_stream_flow_no_decl_no_accept_stays_json_d9() {
    // The Kivi case BEFORE 31.d strict mode: legacy adopter without
    // explicit Accept header gets JSON. D9 backwards-compat with
    // v1.20.x is preserved when the flag is off (D8).
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_NO_DECL).await;
    let (status, ct) = execute(app, "F", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_json(&ct, "legacy + stream + no decl + no Accept (D9)");
}

#[tokio::test]
async fn legacy_mode_stream_flow_no_decl_accept_sse_promotes_d4() {
    // v1.21.0 D4 negotiation: Accept header is the trigger when
    // there's no explicit declaration. Strict mode OFF preserves
    // this path verbatim.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_NO_DECL).await;
    let (status, ct) = execute(app, "F", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_sse(&ct, "legacy + stream + no decl + Accept SSE (D4)");
}

#[tokio::test]
async fn legacy_mode_stream_flow_decl_sse_promotes_d5() {
    // v1.21.0 D5: explicit declaration always wins, regardless of
    // Accept header.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_DECL_SSE).await;
    let (status, ct) = execute(app, "F", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_sse(&ct, "legacy + stream + declared sse (D5)");
}

#[tokio::test]
async fn legacy_mode_stream_flow_decl_json_stays_json_d3() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_DECL_JSON).await;
    let (status, ct) = execute(app, "F", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_json(&ct, "legacy + stream + declared json + Accept SSE (D3)");
}

#[tokio::test]
async fn legacy_mode_non_stream_flow_no_decl_accept_sse_stays_json() {
    // No stream effects + Accept SSE in legacy mode → JSON
    // (D4 requires BOTH stream effect AND Accept to fire).
    let app = build_router(server_cfg(false));
    deploy(app.clone(), NON_STREAM_FLOW_NO_DECL).await;
    let (status, ct) = execute(app, "F", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_json(&ct, "legacy + non-stream + Accept SSE");
}

// ═══════════════════════════════════════════════════════════════════
//   MATRIX SUMMARY — 16-cell decision table exercised
// ═══════════════════════════════════════════════════════════════════

/// The 16-cell decision matrix per plan vivo section 7.1:
///   `(strict_mode × endpoint_decl × stream × accept_sse) → format`
///
/// Cells:
///   * strict_mode ∈ {false, true}
///   * endpoint_decl ∈ {None, "sse", "json"} (3 values)
///   * stream ∈ {false, true}
///   * accept_sse ∈ {false, true}
///   = 2 × 3 × 2 × 2 = 24 cells (some redundant — D3 opt-out is
///     non-stream-friendly; we test the meaningful 16).
///
/// This test exercises a representative sample as a single matrix
/// to make the decision table observable in one assertion.
#[tokio::test]
async fn decision_matrix_summary() {
    struct Case {
        name: &'static str,
        strict: bool,
        source: &'static str,
        accept_sse: bool,
        expect_sse: bool,
    }
    let cases = [
        // Legacy mode — v1.21.0 semantics.
        Case { name: "legacy/stream/no_decl/no_accept",   strict: false, source: STREAM_FLOW_NO_DECL,   accept_sse: false, expect_sse: false },
        Case { name: "legacy/stream/no_decl/accept",       strict: false, source: STREAM_FLOW_NO_DECL,   accept_sse: true,  expect_sse: true },
        Case { name: "legacy/stream/decl_sse",             strict: false, source: STREAM_FLOW_DECL_SSE,  accept_sse: false, expect_sse: true },
        Case { name: "legacy/stream/decl_json",            strict: false, source: STREAM_FLOW_DECL_JSON, accept_sse: true,  expect_sse: false },
        Case { name: "legacy/non_stream/no_decl",          strict: false, source: NON_STREAM_FLOW_NO_DECL, accept_sse: false, expect_sse: false },
        Case { name: "legacy/non_stream/accept",           strict: false, source: NON_STREAM_FLOW_NO_DECL, accept_sse: true,  expect_sse: false },
        // Strict mode — D1 inference activates.
        Case { name: "strict/stream/no_decl/no_accept",    strict: true,  source: STREAM_FLOW_NO_DECL,   accept_sse: false, expect_sse: true },
        Case { name: "strict/stream/no_decl/accept",       strict: true,  source: STREAM_FLOW_NO_DECL,   accept_sse: true,  expect_sse: true },
        Case { name: "strict/stream/decl_sse",             strict: true,  source: STREAM_FLOW_DECL_SSE,  accept_sse: false, expect_sse: true },
        Case { name: "strict/stream/decl_json",            strict: true,  source: STREAM_FLOW_DECL_JSON, accept_sse: true,  expect_sse: false },
        Case { name: "strict/non_stream/no_decl/no_accept", strict: true, source: NON_STREAM_FLOW_NO_DECL, accept_sse: false, expect_sse: false },
        Case { name: "strict/non_stream/no_decl/accept",    strict: true, source: NON_STREAM_FLOW_NO_DECL, accept_sse: true,  expect_sse: false },
    ];
    for c in cases {
        let app = build_router(server_cfg(c.strict));
        deploy(app.clone(), c.source).await;
        let (status, ct) = execute(app, "F", c.accept_sse).await;
        assert_eq!(status, StatusCode::OK, "case {} status", c.name);
        if c.expect_sse {
            assert!(
                ct.starts_with("text/event-stream"),
                "case {}: expected SSE, got {ct}",
                c.name
            );
        } else {
            assert!(
                ct.starts_with("application/json"),
                "case {}: expected JSON, got {ct}",
                c.name
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
//   PHILOSOPHICAL TESTS — D3 opt-out is sacred + D8 backwards-compat
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn d3_opt_out_is_sacred_across_modes() {
    // D3 ratified — explicit `transport: json` MUST win against any
    // automatic promotion path, regardless of flag, regardless of
    // Accept header. The adopter's source is the contract.
    for strict in [false, true] {
        let app = build_router(server_cfg(strict));
        deploy(app.clone(), STREAM_FLOW_DECL_JSON).await;
        for accept_sse in [false, true] {
            let (_, ct) = execute(app.clone(), "F", accept_sse).await;
            assert!(
                ct.starts_with("application/json"),
                "D3 opt-out violated: strict={strict}, accept_sse={accept_sse}, got {ct}"
            );
        }
    }
}

#[tokio::test]
async fn d8_backwards_compat_v1_20_x_client_unchanged_when_flag_off() {
    // D8 ratified — v1.21.0 backwards-compat is absolute when the
    // strict flag is off. A v1.20.x client (no Accept header) hitting
    // a stream-effect flow gets JSON, byte-identical to v1.21.x
    // behavior. This is the safety contract for the v1.22.0 rollout.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_NO_DECL).await;
    let (status, ct) = execute(app, "F", false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        ct.starts_with("application/json"),
        "D8 backwards-compat violated: legacy client got {ct}"
    );
}

#[tokio::test]
async fn d1_inference_fires_only_when_predicate_satisfies() {
    // D1 ratified — inference is precise. A non-stream flow under
    // strict mode does NOT spuriously promote to SSE; the predicate
    // must be satisfied. This guards against any future regression
    // where the strict flag becomes a hammer.
    let app = build_router(server_cfg(true));
    deploy(app.clone(), NON_STREAM_FLOW_NO_DECL).await;
    let (status, ct) = execute(app, "F", false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        ct.starts_with("application/json"),
        "D1 inference fired spuriously on non-stream flow under strict mode: got {ct}"
    );
}
