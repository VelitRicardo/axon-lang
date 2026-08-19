//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.31.0 (D3) — the route carries its backend.
//!
//! 36.d landed the `axonendpoint backend:` declaration on the AST.
//! 36.e carries it onto the deployed route: `DynamicEndpointRoute`
//! gains a `backend` field, `collect_axonendpoint_routes` populates
//! it from the declaration, and `deploy_handler` stops discarding
//! `DeployRequest.backend` — an undeclared route inherits the
//! deploy-request backend as a deploy-scoped default.
//!
//! Pins:
//!   1. A declared `backend:` reaches `DynamicEndpointRoute.backend`.
//!   2. An omitted `backend:` leaves the route field empty (D9).
//!   3. `apply_deploy_backend_default` fills empty routes from an
//!      explicit deploy backend.
//!   4. …but never overrides a route's own declared backend.
//!   5. `"auto"` / empty deploy backends are transparent — no fill
//!      (D5 — auto-resolution stays on the D1 ladder).
//!   6. `"stub"` IS an explicit deploy backend — it fills (D5 forbids
//!      a SILENT stub, not an explicit deploy-wide opt-in).
//!   7. The fill is deterministic + idempotent.
//!   8. `DeployRequest.backend` now defaults to `"auto"` (pre-36 it
//!      defaulted to `"anthropic"` — a silent provider pin).

use axon::axon_server::{
    apply_deploy_backend_default, collect_axonendpoint_routes, DeployRequest,
    DynamicEndpointRoute,
};
use axon::type_checker::compute_implicit_transports;
use std::collections::HashMap;

type RouteTable = HashMap<(String, String), DynamicEndpointRoute>;

fn collect(src: &str) -> RouteTable {
    let tokens = axon::lexer::Lexer::new(src, "<test>").tokenize().unwrap();
    let mut prog = axon::parser::Parser::new(tokens).parse().unwrap();
    compute_implicit_transports(&mut prog);
    collect_axonendpoint_routes(&prog, src, "<test>").unwrap()
}

fn route<'a>(table: &'a RouteTable, method: &str, path: &str) -> &'a DynamicEndpointRoute {
    table
        .get(&(method.to_string(), path.to_string()))
        .unwrap_or_else(|| panic!("route {method} {path} not found"))
}

// ─── section 1 — a declared backend reaches the route ─────────────────────

#[test]
fn s1_declared_backend_reaches_the_route() {
    let src = "flow F() -> Unit { step S { ask: \"x\" } }\n\
        axonendpoint E { public: true method: POST path: \"/chat\" execute: F backend: gemini }";
    let table = collect(src);
    assert_eq!(
        route(&table, "POST", "/chat").backend,
        "gemini",
        "36.e D3: `collect_axonendpoint_routes` must copy the \
         `axonendpoint backend:` declaration onto the route"
    );
}

// ─── section 2 — an omitted backend leaves the route field empty (D9) ─────

#[test]
fn s2_omitted_backend_is_empty_route_field_d9() {
    let src = "flow F() -> Unit { step S { ask: \"x\" } }\n\
        axonendpoint E { public: true method: POST path: \"/chat\" execute: F }";
    let table = collect(src);
    assert_eq!(
        route(&table, "POST", "/chat").backend,
        "",
        "36.e D9: an undeclared `backend:` leaves the route empty — \
         it resolves down the D1 ladder at request time"
    );
}

// ─── section 3 — explicit deploy backend fills empty routes ───────────────

#[test]
fn s3_explicit_deploy_backend_fills_empty_routes() {
    let src = "flow F() -> Unit { step S { ask: \"x\" } }\n\
        axonendpoint E { public: true method: POST path: \"/chat\" execute: F }";
    let mut table = collect(src);
    apply_deploy_backend_default(&mut table, "openai");
    assert_eq!(
        route(&table, "POST", "/chat").backend,
        "openai",
        "36.e D3: a route with no declared backend inherits the \
         explicit `DeployRequest.backend` as a deploy-scoped default"
    );
}

// ─── section 4 — the deploy default never overrides a declaration ─────────

#[test]
fn s4_deploy_default_never_overrides_a_declared_backend() {
    let src = "flow F() -> Unit { step S { ask: \"x\" } }\n\
        axonendpoint E { public: true method: POST path: \"/chat\" execute: F backend: gemini }";
    let mut table = collect(src);
    apply_deploy_backend_default(&mut table, "openai");
    assert_eq!(
        route(&table, "POST", "/chat").backend,
        "gemini",
        "36.e D3: the per-route `backend:` declaration outranks the \
         per-deploy default — a declared route is never overridden"
    );
}

// ─── section 5 — `auto` / empty deploy backends are transparent ───────────

#[test]
fn s5_auto_and_empty_deploy_backends_do_not_fill() {
    let src = "flow F() -> Unit { step S { ask: \"x\" } }\n\
        axonendpoint E { public: true method: POST path: \"/chat\" execute: F }";
    for transparent in ["auto", ""] {
        let mut table = collect(src);
        apply_deploy_backend_default(&mut table, transparent);
        assert_eq!(
            route(&table, "POST", "/chat").backend,
            "",
            "36.e D5: deploy backend `{transparent:?}` is transparent — \
             the route stays empty and resolves down the D1 ladder, \
             never a silent stub"
        );
    }
}

// ─── section 6 — `stub` is an explicit deploy backend (D5) ────────────────

#[test]
fn s6_stub_deploy_backend_is_explicit_and_fills() {
    let src = "flow F() -> Unit { step S { ask: \"x\" } }\n\
        axonendpoint E { public: true method: POST path: \"/chat\" execute: F }";
    let mut table = collect(src);
    apply_deploy_backend_default(&mut table, "stub");
    assert_eq!(
        route(&table, "POST", "/chat").backend,
        "stub",
        "36.e D5: `stub` named explicitly on the deploy request IS an \
         explicit opt-in — D5 forbids a SILENT stub, not a written one"
    );
}

// ─── section 7 — the fill is deterministic + idempotent ───────────────────

#[test]
fn s7_fill_is_deterministic_and_idempotent() {
    let src = "flow F() -> Unit { step S { ask: \"x\" } }\n\
        axonendpoint A { public: true method: POST path: \"/a\" execute: F }\n\
        axonendpoint B { public: true method: GET  path: \"/b\" execute: F backend: kimi }";
    let mut once = collect(src);
    apply_deploy_backend_default(&mut once, "openai");

    let mut twice = collect(src);
    apply_deploy_backend_default(&mut twice, "openai");
    apply_deploy_backend_default(&mut twice, "openai");

    for (key, r) in &once {
        assert_eq!(
            r.backend, twice[key].backend,
            "36.e: `apply_deploy_backend_default` must be deterministic \
             + idempotent for route {key:?}"
        );
    }
    assert_eq!(route(&once, "POST", "/a").backend, "openai");
    assert_eq!(route(&once, "GET", "/b").backend, "kimi");
}

// ─── section 8 — mixed table: declared kept, undeclared filled ────────────

#[test]
fn s8_mixed_table_declared_kept_undeclared_filled() {
    let src = "flow F() -> Unit { step S { ask: \"x\" } }\n\
        axonendpoint Declared   { public: true method: POST path: \"/d\" execute: F backend: anthropic }\n\
        axonendpoint Undeclared { public: true method: POST path: \"/u\" execute: F }";
    let mut table = collect(src);
    apply_deploy_backend_default(&mut table, "glm");
    assert_eq!(route(&table, "POST", "/d").backend, "anthropic");
    assert_eq!(route(&table, "POST", "/u").backend, "glm");
}

// ─── section 9 — DeployRequest.backend default is now `auto` ──────────────

#[test]
fn s9_deploy_request_backend_defaults_to_auto() {
    // v1.31.0 (D3) — pre-36 the serde default was `"anthropic"`,
    // a silent provider pin. The dynamic-route path never consulted
    // it, so the change is regression-free (D9); `"auto"` is honest —
    // an unspecified deploy backend resolves down the D1 ladder.
    let req: DeployRequest = serde_json::from_str(
        r#"{"source":"flow F() -> Unit { step S { ask: \"x\" } }"}"#,
    )
    .expect("DeployRequest must deserialize without an explicit backend");
    assert_eq!(
        req.backend, "auto",
        "36.e D3: an unspecified `DeployRequest.backend` defaults to \
         `auto` — transparent, not a silent `anthropic` pin"
    );
}

#[test]
fn s9_deploy_request_backend_explicit_is_preserved() {
    let req: DeployRequest = serde_json::from_str(
        r#"{"source":"flow F() -> Unit {}","backend":"gemini"}"#,
    )
    .expect("DeployRequest must deserialize with an explicit backend");
    assert_eq!(req.backend, "gemini");
}

// ─── v1.4.0 — empty route table is handled cleanly ────────────────────

#[test]
fn s10_empty_route_table_is_a_clean_noop() {
    let mut table: RouteTable = HashMap::new();
    apply_deploy_backend_default(&mut table, "openai");
    assert!(
        table.is_empty(),
        "36.e: applying the deploy default to an empty table is a no-op"
    );
}
