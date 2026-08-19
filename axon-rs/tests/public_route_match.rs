//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v2.35.0 (Kivi brief #54) — public-API lock for the route-match
//! primitives.
//!
//! `match_path_template` + `parse_query_string` were `pub(crate)`
//! helpers behind the OSS `dynamic_endpoint_handler`. Brief #54 exposed
//! that the enterprise dispatcher (`flow_dispatch::DispatchTable`) served
//! the SAME `/api/v1/flows/{*path}` surface with an exact-string matcher
//! only, so every `{param}` endpoint was mounted-but-unreachable. The fix
//! makes the enterprise dispatcher REUSE these primitives rather than
//! fork a parallel `{param}` implementation — so they must stay `pub`.
//!
//! This integration test compiles ONLY against the crate's PUBLIC API
//! (it lives outside the crate). If either function is re-privatized, or
//! its signature changes, this test fails to compile — a compile-time
//! canary for the enterprise catch-up dependency.

use axon::axon_server::{match_path_template, parse_query_string};

#[test]
fn match_path_template_captures_single_param() {
    let caps = match_path_template("/api/tenants/{tenant_id}/config", "/api/tenants/acme/config")
        .expect("concrete URL must match the template");
    assert_eq!(caps.get("tenant_id").map(String::as_str), Some("acme"));
}

#[test]
fn match_path_template_captures_multiple_params() {
    let caps = match_path_template(
        "/api/chat/history/{session_id}/msg/{msg_id}",
        "/api/chat/history/s1/msg/42",
    )
    .expect("multi-param template must match");
    assert_eq!(caps.get("session_id").map(String::as_str), Some("s1"));
    assert_eq!(caps.get("msg_id").map(String::as_str), Some("42"));
}

#[test]
fn match_path_template_rejects_segment_count_mismatch() {
    // A missing trailing segment is not a match (no multi-segment wildcard).
    assert!(match_path_template("/api/tenants/{id}/config", "/api/tenants/acme").is_none());
    // An empty capture segment must not match.
    assert!(match_path_template("/api/tenants/{id}/config", "/api/tenants//config").is_none());
}

#[test]
fn match_path_template_is_exact_match_when_no_placeholders() {
    // Placeholder-free templates reduce to byte-equality — the same
    // behaviour the enterprise `exact` bucket relies on.
    assert!(match_path_template("/api/health", "/api/health").is_some());
    assert!(match_path_template("/api/health", "/api/health/x").is_none());
}

#[test]
fn parse_query_string_first_value_semantics() {
    let q = parse_query_string(Some("limit=10&group=day&limit=99"));
    assert_eq!(q.get("limit").map(String::as_str), Some("10"), "first value wins");
    assert_eq!(q.get("group").map(String::as_str), Some("day"));

    assert!(parse_query_string(None).is_empty());
    assert!(parse_query_string(Some("")).is_empty());
}
