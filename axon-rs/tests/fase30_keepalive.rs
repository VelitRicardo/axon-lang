//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 30.f — Keepalive comment emission for SSE responses.
//!
//! Validates the D6-ratified keepalive surface end-to-end:
//!
//!   1. `parse_keepalive_duration` maps the closed enum {5s,15s,30s,60s}
//!      to the right `Duration` (and unknown values fall back to 15s).
//!   2. `lookup_keepalive_from_program` finds the declared value via
//!      the parsed AST.
//!   3. `source_text_axonendpoint_keepalive` finds the same value via
//!      the defensive source-text fallback (used when the Rust parser
//!      has gaps — see 30.e dual-signal rationale).
//!   4. `resolve_keepalive_for_flow` integrates both paths with the
//!      D6 default (15s) for the no-declaration case.
//!   5. End-to-end through `POST /v1/execute/sse`: the response Content-
//!      Type stays `text/event-stream`, the retry directive still leads,
//!      and the response body remains well-formed when keepalive is
//!      configured (regression).
//!
//! # What this file does NOT test
//!
//! It does not assert that `: keepalive\n\n` comment lines actually fire
//! at the configured interval during a real flow's execution window.
//! The smallest enum interval is 5 seconds — too long for fast unit
//! tests, and we trust `axum::response::sse::KeepAlive`'s implementation
//! (mature, used in production by hundreds of services) to emit the
//! comment line correctly when the inner stream is inactive for
//! `interval`. The 30.g sub-fase covers 100-iter conformance fuzz over
//! adopter-driven scenarios where actual emission is observed.

use axon::axon_server::{
    build_router, lookup_keepalive_from_program, parse_keepalive_duration,
    resolve_keepalive_for_flow, source_text_axonendpoint_keepalive, ServerConfig,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use std::time::Duration;
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

// ─── 1. parse_keepalive_duration ────────────────────────────────────────

#[test]
fn parse_keepalive_accepts_each_enum_value() {
    assert_eq!(parse_keepalive_duration("5s"), Duration::from_secs(5));
    assert_eq!(parse_keepalive_duration("15s"), Duration::from_secs(15));
    assert_eq!(parse_keepalive_duration("30s"), Duration::from_secs(30));
    assert_eq!(parse_keepalive_duration("60s"), Duration::from_secs(60));
}

#[test]
fn parse_keepalive_defaults_on_empty() {
    assert_eq!(parse_keepalive_duration(""), Duration::from_secs(15));
}

#[test]
fn parse_keepalive_defaults_on_unknown() {
    // Out-of-enum values are silently mapped to the D6 default (15s).
    // The parser enforces the closed enum upstream; this safety net
    // catches stale-deployment paths where pre-30.f sources reach a
    // 30.f+ runtime.
    assert_eq!(parse_keepalive_duration("10s"), Duration::from_secs(15));
    assert_eq!(parse_keepalive_duration("1m"), Duration::from_secs(15));
    assert_eq!(parse_keepalive_duration("forever"), Duration::from_secs(15));
    assert_eq!(parse_keepalive_duration("5"), Duration::from_secs(15));
}

#[test]
fn parse_keepalive_trims_whitespace() {
    assert_eq!(parse_keepalive_duration("  5s  "), Duration::from_secs(5));
    assert_eq!(parse_keepalive_duration("\t15s\n"), Duration::from_secs(15));
}

// ─── 2. lookup_keepalive_from_program (AST path) ───────────────────────

fn parse_to_program(source: &str) -> axon::ast::Program {
    let tokens = axon::lexer::Lexer::new(source, "test.axon")
        .tokenize()
        .expect("lex");
    axon::parser::Parser::new(tokens).parse().expect("parse")
}

#[test]
fn lookup_from_program_finds_explicit_5s() {
    let src = r#"
        flow F() { step S { ask: "hi" } }
        axonendpoint E { public: true
            method: POST
            path: "/f"
            execute: F
            transport: sse
            keepalive: 5s
        }
    "#;
    let program = parse_to_program(src);
    assert_eq!(
        lookup_keepalive_from_program(&program, "F"),
        Some("5s".to_string())
    );
}

#[test]
fn lookup_from_program_finds_all_enum_values() {
    for v in ["5s", "15s", "30s", "60s"] {
        let src = format!(
            "flow F() {{ step S {{ ask: \"x\" }} }}\n\
             axonendpoint E {{ public: true\n\
                method: POST\n\
                path: \"/f\"\n\
                execute: F\n\
                transport: sse\n\
                keepalive: {v}\n\
             }}"
        );
        let program = parse_to_program(&src);
        assert_eq!(
            lookup_keepalive_from_program(&program, "F"),
            Some(v.to_string()),
            "expected {v} to round-trip via AST"
        );
    }
}

#[test]
fn lookup_from_program_returns_empty_string_when_axonendpoint_omits_keepalive() {
    let src = r#"
        flow F() { step S { ask: "hi" } }
        axonendpoint E { public: true
            method: POST
            path: "/f"
            execute: F
            transport: sse
        }
    "#;
    let program = parse_to_program(src);
    // Some("") — the AxonEndpoint exists but the field was omitted.
    // Caller is expected to apply D6 default for empty strings.
    assert_eq!(
        lookup_keepalive_from_program(&program, "F"),
        Some(String::new())
    );
}

#[test]
fn lookup_from_program_returns_none_when_no_axonendpoint_matches_flow() {
    let src = r#"
        flow F() { step S { ask: "hi" } }
        flow G() { step T { ask: "bye" } }
        axonendpoint E { public: true
            method: POST
            path: "/g"
            execute: G
            transport: sse
            keepalive: 30s
        }
    "#;
    let program = parse_to_program(src);
    assert_eq!(lookup_keepalive_from_program(&program, "F"), None);
    assert_eq!(
        lookup_keepalive_from_program(&program, "G"),
        Some("30s".to_string())
    );
}

// ─── 3. source_text_axonendpoint_keepalive (fallback path) ─────────────

#[test]
fn source_text_finds_keepalive_with_space_after_colon() {
    let src = "axonendpoint E { public: true\n  execute: F\n  keepalive: 30s\n}\n";
    assert_eq!(
        source_text_axonendpoint_keepalive(src, "F"),
        Some("30s".to_string())
    );
}

#[test]
fn source_text_finds_keepalive_without_space_after_colon() {
    let src = "axonendpoint E { public: true execute:F keepalive:60s }";
    assert_eq!(
        source_text_axonendpoint_keepalive(src, "F"),
        Some("60s".to_string())
    );
}

#[test]
fn source_text_word_boundary_does_not_confuse_5s_with_15s() {
    // The keepalive value is "15s" but the substring "5s" appears
    // inside it. Without word-boundary anchoring we'd return Some("5s").
    let src = "axonendpoint E { public: true execute: F keepalive: 15s }";
    assert_eq!(
        source_text_axonendpoint_keepalive(src, "F"),
        Some("15s".to_string())
    );
}

#[test]
fn source_text_returns_none_when_no_axonendpoint_matches_flow() {
    let src = "axonendpoint E { public: true execute: G keepalive: 30s }";
    assert_eq!(source_text_axonendpoint_keepalive(src, "F"), None);
}

#[test]
fn source_text_returns_none_when_axonendpoint_has_no_keepalive_field() {
    let src = "axonendpoint E { public: true execute: F transport: sse }";
    assert_eq!(source_text_axonendpoint_keepalive(src, "F"), None);
}

#[test]
fn source_text_string_aware_braces_dont_break_lookup() {
    // The brace counter must not confuse `{` inside the path string
    // literal with the structural close. (Defensive: if it did, we'd
    // exit the block early and miss the keepalive field.)
    let src = r#"axonendpoint E { public: true
        method: POST
        path: "/users/{id}/messages/{mid}"
        execute: F
        transport: sse
        keepalive: 5s
    }"#;
    assert_eq!(
        source_text_axonendpoint_keepalive(src, "F"),
        Some("5s".to_string())
    );
}

#[test]
fn source_text_walks_multiple_axonendpoints_until_match() {
    // First axonendpoint targets G with keepalive 30s. Second targets
    // F with keepalive 5s. The lookup must walk past the first and
    // return the second.
    let src = r#"
        axonendpoint E1 { public: true execute: G transport: sse keepalive: 30s }
        axonendpoint E2 { public: true execute: F transport: sse keepalive: 5s }
    "#;
    assert_eq!(
        source_text_axonendpoint_keepalive(src, "F"),
        Some("5s".to_string())
    );
    assert_eq!(
        source_text_axonendpoint_keepalive(src, "G"),
        Some("30s".to_string())
    );
}

// ─── 4. resolve_keepalive_for_flow (integrated) ────────────────────────

#[test]
fn resolve_returns_default_15s_when_no_axonendpoint() {
    let src = "flow F() { step S { ask: \"hi\" } }";
    assert_eq!(resolve_keepalive_for_flow(src, "F"), Duration::from_secs(15));
}

#[test]
fn resolve_returns_default_15s_when_axonendpoint_omits_keepalive() {
    let src = r#"
        flow F() { step S { ask: "hi" } }
        axonendpoint E { public: true
            method: POST
            path: "/f"
            execute: F
            transport: sse
        }
    "#;
    assert_eq!(resolve_keepalive_for_flow(src, "F"), Duration::from_secs(15));
}

#[test]
fn resolve_returns_declared_value_via_ast() {
    for (declared, expected_secs) in [("5s", 5u64), ("15s", 15), ("30s", 30), ("60s", 60)] {
        let src = format!(
            "flow F() {{ step S {{ ask: \"x\" }} }}\n\
             axonendpoint E {{ public: true\n\
                method: POST\n\
                path: \"/f\"\n\
                execute: F\n\
                transport: sse\n\
                keepalive: {declared}\n\
             }}"
        );
        assert_eq!(
            resolve_keepalive_for_flow(&src, "F"),
            Duration::from_secs(expected_secs),
            "AST path failed for {declared}"
        );
    }
}

#[test]
fn resolve_falls_back_to_source_text_when_ast_parse_fails() {
    // Source-text-only fragment that's not a parseable axon program
    // (no flow declaration, just the axonendpoint shape). The AST
    // path returns None → fallback engages and finds the keepalive.
    let src = "axonendpoint E { public: true execute: F transport: sse keepalive: 60s }";
    // Note: lexer/parser may succeed or fail on this fragment depending
    // on grammar; either way the resolve function must NOT panic and
    // must return either the AST verdict (60s) or the source-text
    // verdict (60s). Both yield the same answer.
    assert_eq!(resolve_keepalive_for_flow(src, "F"), Duration::from_secs(60));
}

#[test]
fn resolve_default_when_flow_name_does_not_match_any_axonendpoint() {
    let src = r#"
        flow F() { step S { ask: "x" } }
        flow G() { step T { ask: "y" } }
        axonendpoint E { public: true
            method: POST
            path: "/g"
            execute: G
            transport: sse
            keepalive: 60s
        }
    "#;
    // F has no matching axonendpoint → default 15s.
    assert_eq!(resolve_keepalive_for_flow(src, "F"), Duration::from_secs(15));
    // G has its declared 60s.
    assert_eq!(resolve_keepalive_for_flow(src, "G"), Duration::from_secs(60));
}

// ─── 5. End-to-end regression through /v1/execute/sse ──────────────────

async fn deploy(app: axum::Router, source: &str) {
    let body = serde_json::json!({
        "source": source,
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
    assert_eq!(status, StatusCode::OK, "deploy status: body={json}");
    assert_eq!(
        json.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "deploy success=false: body={json}"
    );
}

async fn call_sse(app: axum::Router, flow: &str) -> (StatusCode, String, String) {
    let body = serde_json::json!({ "flow_name": flow, "backend": "stub" });
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
        .unwrap_or("")
        .to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes().to_vec();
    let text = String::from_utf8(bytes).expect("utf-8 sse body");
    (status, ct, text)
}

#[tokio::test]
async fn sse_response_still_well_formed_with_default_keepalive() {
    // No axonendpoint declaration → default 15s keepalive applied.
    // The response should still produce a complete event with all
    // expected wire fields. Keepalive doesn't fire during the test
    // because execution is fast (<<15s).
    let app = build_router(server_cfg());
    deploy(
        app.clone(),
        "flow F() { step S { ask: \"hello\" } }",
    )
    .await;

    let (status, ct, text) = call_sse(app, "F").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));
    assert!(text.contains("retry: 5000"), "retry hint missing");
    assert!(
        text.contains("event: axon.complete"),
        "complete terminator missing: {text}"
    );
    assert!(text.ends_with("\n\n"), "wire terminator missing");
}

#[tokio::test]
async fn sse_response_well_formed_with_declared_5s_keepalive() {
    // axonendpoint declares 5s keepalive. Response should be wire-
    // compliant. Stub backend is too fast for the 5s interval to
    // fire in this test, but the regression that the configuration
    // applies cleanly is asserted (no parse error, no 5xx, etc).
    let app = build_router(server_cfg());
    let src = r#"
        flow F() { step S { ask: "hi" } }
        axonendpoint E { public: true
            method: POST
            path: "/f"
            execute: F
            transport: sse
            keepalive: 5s
        }
    "#;
    deploy(app.clone(), src).await;

    let (status, ct, text) = call_sse(app, "F").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));
    assert!(text.contains("retry: 5000"));
    assert!(text.contains("event: axon.complete"));
    assert!(text.ends_with("\n\n"));
}

#[tokio::test]
async fn sse_response_well_formed_with_declared_60s_keepalive() {
    let app = build_router(server_cfg());
    let src = r#"
        flow F() { step S { ask: "hi" } }
        axonendpoint E { public: true
            method: POST
            path: "/f"
            execute: F
            transport: sse
            keepalive: 60s
        }
    "#;
    deploy(app.clone(), src).await;

    let (status, ct, text) = call_sse(app, "F").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));
    assert!(text.contains("event: axon.complete"));
}

#[tokio::test]
async fn sse_not_deployed_path_still_works_with_keepalive_wired() {
    // The not-deployed branch goes through the unified channel + still
    // wires the KeepAlive (uses the 15s default). Wire format must
    // remain `retry: 5000` + `event: axon.error` + blank-line
    // terminator.
    let app = build_router(server_cfg());
    let (status, ct, text) = call_sse(app, "ghost").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));
    assert!(text.contains("retry: 5000"));
    assert!(text.contains("event: axon.error"));
    assert!(text.contains("\"recoverable\":false"));
    assert!(text.ends_with("\n\n"));
}

#[tokio::test]
async fn sse_response_does_not_contain_keepalive_comment_for_fast_flow() {
    // Sanity: with a fast flow + default 15s interval, the keepalive
    // comment should NOT appear (because the stream completes well
    // before the interval elapses). This guards against accidental
    // emission of `: keepalive` lines for the happy-path fast case.
    let app = build_router(server_cfg());
    deploy(app.clone(), "flow F() { step S { ask: \"hi\" } }").await;
    let (_, _, text) = call_sse(app, "F").await;
    assert!(
        !text.contains(": keepalive"),
        "keepalive comment should not fire for fast flows; got: {text}"
    );
}
