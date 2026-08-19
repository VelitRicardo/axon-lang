//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.23.0 — Per-endpoint transport on dynamic routes.
//!
//! D6 ratificada 2026-05-11. Locks the contract that the **same**
//! v1.21.0 + v1.22.0 negotiation matrix that fires on
//! `POST /v1/execute` ALSO fires uniformly on the dynamically
//! registered routes from v1.23.0. The Kivi case is closed at the
//! wire layer: when an adopter writes
//!
//!     axonendpoint Chat { public: true
//!         method: POST
//!         path:   "/chat"
//!         execute: ChatFlow
//!         transport: sse
//!         keepalive: 5s
//!     }
//!
//! and the client hits `POST /chat`, the response is byte-identical
//! to what `POST /v1/execute` with `flow: "ChatFlow"` would have
//! produced under the same negotiation matrix — Content-Type
//! `text/event-stream`, the declared keepalive honored, diagnostic
//! header attached, all of it.
//!
//! Pillar trace per D12:
//!   - MATHEMATICS — the negotiation function is single-valued: for
//!                   a fixed (program, flow, accept, strict_mode)
//!                   tuple, the wire format is determined. Both
//!                   `/v1/execute` AND the dynamic route project
//!                   onto the SAME function.
//!   - LOGIC      — for every cell of the 8-cell matrix exercised
//!                   here, the dynamic route's verdict matches the
//!                   `/v1/execute` verdict (parity assertion).
//!   - PHILOSOPHY — the declarative path IS the contract; auditors
//!                   inspect source + know the wire format the
//!                   adopter's clients will see.
//!   - COMPUTING  — D10 absolute backwards-compat: `/v1/execute`
//!                   preserved verbatim; dynamic routes layer on top.
//!
//! Coverage:
//!   1. 8-cell negotiation matrix replayed on dynamic routes
//!      (`strict_mode × declaration × accept_sse` for stream and
//!      non-stream flows) — parity with `/v1/execute`.
//!   2. Keepalive declaration honored end-to-end on dynamic routes
//!      (each of the 5s/15s/30s/60s enum values triggers an SSE
//!      response; the runtime helpers `parse_keepalive_duration` +
//!      `resolve_keepalive_for_flow` consume the same axonendpoint
//!      regardless of which entrypoint the request arrived at).
//! 3. v1.22.0 `X-Axon-Stream-Available` diagnostic header fires
//!      on dynamic-route JSON responses too — adopter observability
//!      is uniform across the REST surface.
//!   4. Per-endpoint paths producing distinct flows under one
//!      strict-mode setting demonstrates dynamic routes coexist
//!      cleanly.
//!   5. D10 contract: hitting `/v1/execute` with the same flow_name
//!      and hitting the declared path produce the same wire format
//!      (true parity, not approximation).

use axon::axon_server::{
    build_router, parse_keepalive_duration, resolve_keepalive_for_flow, ServerConfig,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use std::time::Duration;
use tower::ServiceExt;

const STREAM_HEADER: &str = "x-axon-stream-available";

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

/// Hit the declared dynamic path. Returns (status, content_type).
async fn hit_dynamic(
    app: axum::Router,
    method: &str,
    path: &str,
    accept_sse: bool,
) -> (StatusCode, String) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if accept_sse {
        builder = builder.header("accept", "text/event-stream");
    }
    let req = builder.body(Body::from("{}")).unwrap();
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

/// Hit `/v1/execute` for the same flow — the parity reference.
async fn hit_v1_execute(
    app: axum::Router,
    flow: &str,
    accept_sse: bool,
) -> (StatusCode, String) {
    let body = serde_json::json!({"flow": flow, "backend": "stub"});
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

fn assert_sse(ct: &str, ctx: &str) {
    assert!(
        ct.starts_with("text/event-stream"),
        "{ctx}: expected text/event-stream, got '{ct}'"
    );
}

fn assert_json(ct: &str, ctx: &str) {
    assert!(
        ct.starts_with("application/json"),
        "{ctx}: expected application/json, got '{ct}'"
    );
}

// ─── Source corpus — dynamic routes ─────────────────────────────────
//
// v1.27.1 — These fixtures use the TYPE-ANNOTATION
// disjunct (`output: Stream<Token>` on a step) rather than the
// ALGEBRAIC-EFFECT disjunct (`apply: tool` where the tool declares
// `effects: <stream:<policy>>`). The two disjuncts now have DIFFERENT
// gate behaviors post-33.z.k.1:
//   - Type-annotation only → D6 backwards-compat (requires
//     strict_mode OR Accept: text/event-stream)
//   - Algebraic effect      → D11 override (unconditional SSE,
//     D3 transport:json still wins)
// The 8-cell matrix below pins the TYPE-ANNOTATION disjunct. The
// algebraic-effect disjunct has its own dedicated test pack in
// `tests/dialect_algebraic_override.rs`.

const STREAM_FLOW_NO_DECL: &str =
    "flow F() -> Unit { step S { ask: \"x\" output: Stream<Token> } }\n\
     axonendpoint E { public: true method: POST path: \"/dyn-stream\" execute: F }";

const STREAM_FLOW_DECL_SSE: &str =
    "flow F() -> Unit { step S { ask: \"x\" output: Stream<Token> } }\n\
     axonendpoint E { public: true method: POST path: \"/dyn-sse\" execute: F transport: sse }";

const STREAM_FLOW_DECL_JSON: &str =
    "flow F() -> Unit { step S { ask: \"x\" output: Stream<Token> } }\n\
     axonendpoint E { public: true method: POST path: \"/dyn-json\" execute: F transport: json }";

const NON_STREAM_FLOW_NO_DECL: &str =
    "flow F() -> Int { step S { ask: \"x\" output: Int } }\n\
     axonendpoint E { public: true method: POST path: \"/dyn-plain\" execute: F }";

// ═══════════════════════════════════════════════════════════════════
//   8-cell matrix replay on dynamic routes
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn dyn_strict_stream_no_decl_no_accept_promotes_to_sse() {
    // v1.22.0 D1 inference fires on declared path under strict mode.
    let app = build_router(server_cfg(true));
    deploy(app.clone(), STREAM_FLOW_NO_DECL).await;
    let (status, ct) = hit_dynamic(app, "POST", "/dyn-stream", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_sse(&ct, "strict + dynamic + stream + no decl + no Accept");
}

#[tokio::test]
async fn dyn_strict_stream_no_decl_accept_sse_promotes() {
    let app = build_router(server_cfg(true));
    deploy(app.clone(), STREAM_FLOW_NO_DECL).await;
    let (status, ct) = hit_dynamic(app, "POST", "/dyn-stream", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_sse(&ct, "strict + dynamic + stream + no decl + Accept SSE");
}

#[tokio::test]
async fn dyn_strict_stream_decl_json_stays_json_d3() {
    // D3 sacred opt-out — adopter's explicit declaration wins even
    // under strict mode and even when the client asks for SSE.
    let app = build_router(server_cfg(true));
    deploy(app.clone(), STREAM_FLOW_DECL_JSON).await;
    let (status, ct) = hit_dynamic(app, "POST", "/dyn-json", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_json(&ct, "strict + dynamic + stream + declared json (D3)");
}

#[tokio::test]
async fn dyn_legacy_stream_no_decl_no_accept_stays_json_d9() {
    // D9 backwards-compat: legacy mode + no declaration + no Accept →
    // JSON. The Kivi case BEFORE v1.22.0 strict mode flipped on.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_NO_DECL).await;
    let (status, ct) = hit_dynamic(app, "POST", "/dyn-stream", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_json(&ct, "legacy + dynamic + stream + no decl + no Accept (D9)");
}

#[tokio::test]
async fn dyn_legacy_stream_no_decl_accept_sse_promotes_d4() {
    // v1.21.0 D4 negotiation fires on dynamic routes: Accept header
    // is the trigger when there's no explicit declaration.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_NO_DECL).await;
    let (status, ct) = hit_dynamic(app, "POST", "/dyn-stream", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_sse(&ct, "legacy + dynamic + stream + no decl + Accept SSE (D4)");
}

#[tokio::test]
async fn dyn_legacy_stream_decl_sse_promotes_d5() {
    // v1.21.0 D5: explicit declaration always wins regardless of Accept.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_DECL_SSE).await;
    let (status, ct) = hit_dynamic(app, "POST", "/dyn-sse", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_sse(&ct, "legacy + dynamic + stream + declared sse (D5)");
}

#[tokio::test]
async fn dyn_non_stream_no_decl_no_accept_stays_json() {
    // No stream effects → no inference fires → JSON regardless of mode.
    let app = build_router(server_cfg(true));
    deploy(app.clone(), NON_STREAM_FLOW_NO_DECL).await;
    let (status, ct) = hit_dynamic(app, "POST", "/dyn-plain", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_json(&ct, "strict + dynamic + non-stream + no decl");
}

#[tokio::test]
async fn dyn_non_stream_accept_sse_still_json() {
    // Accept alone cannot promote a non-stream flow — D4 requires
    // BOTH a stream effect AND an Accept SSE header.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), NON_STREAM_FLOW_NO_DECL).await;
    let (status, ct) = hit_dynamic(app, "POST", "/dyn-plain", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_json(&ct, "legacy + dynamic + non-stream + Accept SSE");
}

// ═══════════════════════════════════════════════════════════════════
//   /v1/execute ↔ declared-path PARITY (D10 + D6 invariant)
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn parity_legacy_stream_no_decl_no_accept() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_NO_DECL).await;
    let (dyn_status, dyn_ct) = hit_dynamic(app.clone(), "POST", "/dyn-stream", false).await;
    let (leg_status, leg_ct) = hit_v1_execute(app, "F", false).await;
    assert_eq!(dyn_status, leg_status, "status parity");
    assert_eq!(
        dyn_ct.split(';').next(),
        leg_ct.split(';').next(),
        "content-type parity (D6 invariant): /v1/execute and dynamic path must agree"
    );
}

#[tokio::test]
async fn parity_legacy_stream_decl_sse() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_DECL_SSE).await;
    let (dyn_status, dyn_ct) = hit_dynamic(app.clone(), "POST", "/dyn-sse", false).await;
    let (leg_status, leg_ct) = hit_v1_execute(app, "F", false).await;
    assert_eq!(dyn_status, leg_status);
    assert_sse(&dyn_ct, "dynamic SSE");
    assert_sse(&leg_ct, "/v1/execute SSE");
}

#[tokio::test]
async fn parity_strict_stream_no_decl_no_accept_both_promote() {
    // Strict mode D1 fires on BOTH /v1/execute and the dynamic route.
    let app = build_router(server_cfg(true));
    deploy(app.clone(), STREAM_FLOW_NO_DECL).await;
    let (_dyn_status, dyn_ct) = hit_dynamic(app.clone(), "POST", "/dyn-stream", false).await;
    let (_leg_status, leg_ct) = hit_v1_execute(app, "F", false).await;
    assert_sse(&dyn_ct, "strict mode dynamic");
    assert_sse(&leg_ct, "strict mode /v1/execute");
}

#[tokio::test]
async fn parity_non_stream_returns_json_on_both_entrypoints() {
    let app = build_router(server_cfg(true));
    deploy(app.clone(), NON_STREAM_FLOW_NO_DECL).await;
    let (_, dyn_ct) = hit_dynamic(app.clone(), "POST", "/dyn-plain", false).await;
    let (_, leg_ct) = hit_v1_execute(app, "F", false).await;
    assert_json(&dyn_ct, "dynamic non-stream");
    assert_json(&leg_ct, "/v1/execute non-stream");
}

// ═══════════════════════════════════════════════════════════════════
//   Keepalive declaration honored on dynamic routes
// ═══════════════════════════════════════════════════════════════════

fn stream_with_keepalive(ka: &str, path: &str) -> String {
    format!(
        "tool t {{ description: \"t\" effects: <stream:drop_oldest> }}\n\
         flow F() -> Unit {{ step S {{ ask: \"x\" apply: t }} }}\n\
         axonendpoint E {{ public: true method: POST path: \"{path}\" execute: F \
            transport: sse keepalive: {ka} }}"
    )
}

#[tokio::test]
async fn keepalive_5s_on_dynamic_route_returns_sse() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), &stream_with_keepalive("5s", "/dyn-ka-5")).await;
    let (status, ct) = hit_dynamic(app, "POST", "/dyn-ka-5", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_sse(&ct, "dynamic + keepalive:5s");
}

#[tokio::test]
async fn keepalive_15s_on_dynamic_route_returns_sse() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), &stream_with_keepalive("15s", "/dyn-ka-15")).await;
    let (status, ct) = hit_dynamic(app, "POST", "/dyn-ka-15", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_sse(&ct, "dynamic + keepalive:15s");
}

#[tokio::test]
async fn keepalive_30s_on_dynamic_route_returns_sse() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), &stream_with_keepalive("30s", "/dyn-ka-30")).await;
    let (status, ct) = hit_dynamic(app, "POST", "/dyn-ka-30", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_sse(&ct, "dynamic + keepalive:30s");
}

#[tokio::test]
async fn keepalive_60s_on_dynamic_route_returns_sse() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), &stream_with_keepalive("60s", "/dyn-ka-60")).await;
    let (status, ct) = hit_dynamic(app, "POST", "/dyn-ka-60", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_sse(&ct, "dynamic + keepalive:60s");
}

#[tokio::test]
async fn keepalive_resolution_helper_consumes_dynamic_axonendpoint() {
    // The runtime's `resolve_keepalive_for_flow` consumes the SAME
    // axonendpoint regardless of which entrypoint serves the request.
    // Anchor the contract at the helper layer: declared keepalive
    // round-trips through resolution unchanged.
    let src = stream_with_keepalive("30s", "/whatever");
    let dur = resolve_keepalive_for_flow(&src, "F");
    assert_eq!(dur, Duration::from_secs(30));
    let dur = resolve_keepalive_for_flow(
        &stream_with_keepalive("5s", "/whatever"),
        "F",
    );
    assert_eq!(dur, Duration::from_secs(5));
}

#[tokio::test]
async fn keepalive_default_15s_when_omitted_on_sse_dynamic_route() {
    // No keepalive declared on a transport:sse axonendpoint → runtime
    // default (15s) per v1.21.0 D6.
    let src = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
               flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
               axonendpoint E { public: true method: POST path: \"/dyn-no-ka\" \
                  execute: F transport: sse }";
    let dur = resolve_keepalive_for_flow(src, "F");
    assert_eq!(dur, parse_keepalive_duration(""));
    assert_eq!(dur, Duration::from_secs(15));
}

// ═══════════════════════════════════════════════════════════════════
//   X-Axon-Stream-Available diagnostic header on dynamic routes
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn diagnostic_header_fires_on_dynamic_route_json_response() {
    // v1.22.0 D5 — when a stream flow's response stays JSON (no
    // promotion fired), the header is attached. Adopter sees the
    // diagnostic from their REST client without spelunking source.
    // SAME behavior on dynamic routes as on /v1/execute.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_NO_DECL).await;
    let req = Request::builder()
        .method("POST")
        .uri("/dyn-stream")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let header = resp
        .headers()
        .get(STREAM_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    assert_eq!(status, StatusCode::OK);
    assert!(
        header.is_some(),
        "X-Axon-Stream-Available must fire on dynamic-route JSON for stream flows"
    );
    let h = header.unwrap();
    assert!(h.starts_with("1;"), "header must start with `1;`: '{h}'");
    assert!(h.contains("flow=F"), "header must carry flow=F: '{h}'");
    assert!(
        h.contains("opt_in="),
        "header must carry opt_in hint: '{h}'"
    );
}

#[tokio::test]
async fn diagnostic_header_absent_on_dynamic_route_sse_response() {
    // When the response is already SSE, there's nothing to diagnose —
    // the header doesn't fire (v1.22.0 D5 negative-space).
    let app = build_router(server_cfg(false));
    deploy(app.clone(), STREAM_FLOW_DECL_SSE).await;
    let req = Request::builder()
        .method("POST")
        .uri("/dyn-sse")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let header = resp.headers().get(STREAM_HEADER);
    assert!(
        header.is_none(),
        "header must not fire on SSE responses (already promoted)"
    );
}

#[tokio::test]
async fn diagnostic_header_absent_on_dynamic_route_non_stream_flow() {
    // Non-stream flows don't have stream effects → no diagnostic.
    let app = build_router(server_cfg(false));
    deploy(app.clone(), NON_STREAM_FLOW_NO_DECL).await;
    let req = Request::builder()
        .method("POST")
        .uri("/dyn-plain")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let header = resp.headers().get(STREAM_HEADER);
    assert!(
        header.is_none(),
        "header must not fire when there are no stream effects"
    );
}

// ═══════════════════════════════════════════════════════════════════
//   Multiple dynamic routes coexist with their own transport semantics
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn multiple_dynamic_routes_each_honor_their_own_declaration() {
    // Two axonendpoints sharing one flow but with different transport
    // declarations — each path returns its own wire format. The
    // negotiation is PER-ROUTE, not per-flow.
    let src = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
               flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
               axonendpoint Sse { public: true method: POST path: \"/multi-sse\" execute: F transport: sse }\n\
               axonendpoint Json { public: true method: POST path: \"/multi-json\" execute: F transport: json }";
    let app = build_router(server_cfg(false));
    deploy(app.clone(), src).await;

    let (_, sse_ct) = hit_dynamic(app.clone(), "POST", "/multi-sse", false).await;
    let (_, json_ct) = hit_dynamic(app, "POST", "/multi-json", true).await;
    assert_sse(&sse_ct, "/multi-sse honors declared sse");
    assert_json(&json_ct, "/multi-json honors declared json even with Accept SSE");
}

// ═══════════════════════════════════════════════════════════════════
//   D6 sanity: the same matrix applies — single source of truth
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn d6_matrix_summary_dynamic_routes() {
    // Compressed truth table from v1.22.0 strict-mode matrix, replayed
    // through dynamic routes. Asserts the function from
    // (strict, declaration, accept_sse) → wire_format projects the
    // SAME values for declared paths as for /v1/execute.
    struct Case {
        name: &'static str,
        strict: bool,
        source: &'static str,
        path: &'static str,
        accept_sse: bool,
        expect_sse: bool,
    }
    let cases = [
        Case { name: "legacy/stream/no_decl/no_accept",  strict: false, source: STREAM_FLOW_NO_DECL,   path: "/dyn-stream", accept_sse: false, expect_sse: false },
        Case { name: "legacy/stream/no_decl/accept",     strict: false, source: STREAM_FLOW_NO_DECL,   path: "/dyn-stream", accept_sse: true,  expect_sse: true },
        Case { name: "legacy/stream/decl_sse",           strict: false, source: STREAM_FLOW_DECL_SSE,  path: "/dyn-sse",    accept_sse: false, expect_sse: true },
        Case { name: "legacy/stream/decl_json/accept",   strict: false, source: STREAM_FLOW_DECL_JSON, path: "/dyn-json",   accept_sse: true,  expect_sse: false },
        Case { name: "strict/stream/no_decl/no_accept",  strict: true,  source: STREAM_FLOW_NO_DECL,   path: "/dyn-stream", accept_sse: false, expect_sse: true },
        Case { name: "strict/stream/decl_json",          strict: true,  source: STREAM_FLOW_DECL_JSON, path: "/dyn-json",   accept_sse: false, expect_sse: false },
        Case { name: "strict/non_stream/no_decl",        strict: true,  source: NON_STREAM_FLOW_NO_DECL, path: "/dyn-plain", accept_sse: false, expect_sse: false },
        Case { name: "legacy/non_stream/accept",         strict: false, source: NON_STREAM_FLOW_NO_DECL, path: "/dyn-plain", accept_sse: true,  expect_sse: false },
    ];
    for case in cases.iter() {
        let app = build_router(server_cfg(case.strict));
        deploy(app.clone(), case.source).await;
        let (status, ct) = hit_dynamic(app, "POST", case.path, case.accept_sse).await;
        assert_eq!(status, StatusCode::OK, "[{}] status", case.name);
        if case.expect_sse {
            assert_sse(&ct, case.name);
        } else {
            assert_json(&ct, case.name);
        }
    }
}
