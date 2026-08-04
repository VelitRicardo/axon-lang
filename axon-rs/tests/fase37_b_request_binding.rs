//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 37.b (D1, D4) — The Request Binding Contract: runtime delivery.
//!
//! An `axonendpoint` declares `body: T` and `execute: F`; the request
//! body's fields populate F's declared parameters, BY NAME. This pack
//! proves the contract holds end-to-end through the real HTTP surface:
//!
//!   §1 — D1: every declared parameter of a multi-parameter flow binds
//!         from its same-named body field and interpolates in a step
//!         `ask:` prompt.
//!   §2 — D1: a bound `${param}` threads through the FULL agent flow
//!         — `retrieve` context → `step` deliberate → `persist` — so
//!         the body parameter is in scope across every node type, not
//!         only a pure-`step` flow.
//!   §3 — D4: a body field that matches NO declared parameter is NOT
//!         bound — `${undeclared}` survives un-interpolated, so the
//!         compile-time totality check (37.c) stays the single gate.
//!   §4 — D5: a parameter-less flow behind an endpoint with no `body:`
//!         streams byte-clean — the binding machinery is inert when
//!         there is nothing to bind.
//!   §5 — D1: the binding holds on the JSON transport too — a
//!         parameterised flow behind `transport: json` executes
//!         through the full `ExecuteRequest` → `execute_handler` →
//!         `execute_with_fallback` → `server_execute` →
//!         `runner::execute_server_flow` chain.
//!
//! Observation technique (mirrors Fase 36.x.e): a `stub_stream` echo
//! tool echoes its interpolated argument verbatim onto the wire, so
//! `${param}` substitution is directly observable. Infra-free — the
//! `in_memory` store (Fase 36.x.b) needs no Postgres.

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

async fn deploy(app: &axum::Router, src: &str) {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::json!({ "source": src }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(status, StatusCode::OK, "37.b deploy failed: {json}");
}

/// POST `path` with a JSON `body` + `Accept: text/event-stream`;
/// drain the full SSE response into a String.
async fn hit_sse(app: &axum::Router, path: &str, body: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "37.b {path} status");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// POST `path` with a JSON `body`; return the parsed JSON response.
async fn hit_json(app: &axum::Router, path: &str, body: &str) -> serde_json::Value {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "37.b {path} status");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or_default()
}

const ECHO_TOOL: &str = "tool Echo { provider: stub_stream description: \"echo\" \
                          effects: <stream:drop_oldest> }\n";

// ── §1 — D1: every declared parameter binds by name ─────────────────

#[tokio::test]
async fn s1_every_declared_parameter_binds_from_its_body_field() {
    let app = build_router(server_cfg());
    let src = format!(
        "type MultiBody {{ message: String tenant_id: String }}\n\
         {ECHO_TOOL}\
         flow MultiFlow(message: String, tenant_id: String) -> Unit {{\n\
             step Reply {{ ask: \"m=${{message}}|t=${{tenant_id}}\" apply: Echo }}\n\
         }}\n\
         axonendpoint MultiE {{ public: true method: POST path: \"/multi\" \
             body: MultiBody execute: MultiFlow backend: stub transport: sse }}"
    );
    deploy(&app, &src).await;

    let wire = hit_sse(
        &app,
        "/multi",
        r#"{"message":"MSG_VALUE_B2","tenant_id":"TEN_VALUE_B2"}"#,
    )
    .await;

    assert!(
        wire.contains("m=MSG_VALUE_B2") && wire.contains("t=TEN_VALUE_B2"),
        "§37.b D1 — every declared flow parameter must bind from its \
         same-named body field and interpolate in the step `ask:`. \
         Wire:\n{wire}"
    );
    assert!(
        !wire.contains("${message}") && !wire.contains("${tenant_id}"),
        "§37.b D1 — no parameter token may survive un-interpolated. \
         Wire:\n{wire}"
    );
}

// ── §2 — D1: the body param threads through the full agent flow ─────

#[tokio::test]
async fn s2_body_param_threads_through_the_full_agent_flow() {
    let app = build_router(server_cfg());
    // The canonical agent shape — retrieve context → deliberate →
    // persist — MIXED with store ops, not a pure-`step` flow. The
    // body parameter `${query}` is seeded BEFORE the whole walk, so it
    // is in scope at the `step` even though a `retrieve` precedes it
    // and a `persist` (whose field block also interpolates `${query}`)
    // follows it.
    let src = format!(
        "type AgentBody {{ query: String }}\n\
         axonstore mem {{ backend: in_memory }}\n\
         {ECHO_TOOL}\
         flow AgentFlow(query: String) -> Unit {{\n\
             retrieve mem {{ where: \"1 = 1\" as: ctx }}\n\
             step Reply {{ ask: \"answering: ${{query}}\" apply: Echo }}\n\
             persist into mem {{ q: \"${{query}}\" answer: \"done\" }}\n\
         }}\n\
         axonendpoint AgentE {{ public: true method: POST path: \"/agent\" \
             body: AgentBody execute: AgentFlow backend: stub transport: sse }}"
    );
    deploy(&app, &src).await;

    let wire = hit_sse(&app, "/agent", r#"{"query":"AGENT_QUERY_B2"}"#).await;

    assert!(
        wire.contains("answering: AGENT_QUERY_B2"),
        "§37.b D1 — in a MIXED agent flow (retrieve → step → persist) \
         the body parameter `${{query}}` must be seeded before the \
         whole walk, so it interpolates at the `step` regardless of \
         the surrounding store ops. Wire:\n{wire}"
    );
    assert!(
        !wire.contains("${query}"),
        "§37.b D1 — no `${{query}}` token survives un-interpolated. \
         Wire:\n{wire}"
    );
}

// ── §3 — D4: an undeclared body field is NOT bound ──────────────────

#[tokio::test]
async fn s3_d4_undeclared_body_field_is_not_bound() {
    let app = build_router(server_cfg());
    // `D4Body` carries two fields; `D4Flow` declares ONLY `declared`
    // as a parameter. `undeclared` is a valid body field but not a
    // flow parameter — D4 forbids binding it.
    let src = format!(
        "type D4Body {{ declared: String undeclared: String }}\n\
         {ECHO_TOOL}\
         flow D4Flow(declared: String) -> Unit {{\n\
             step Reply {{ ask: \"d=${{declared}}|u=${{undeclared}}\" apply: Echo }}\n\
         }}\n\
         axonendpoint D4E {{ public: true method: POST path: \"/d4\" \
             body: D4Body execute: D4Flow backend: stub transport: sse }}"
    );
    deploy(&app, &src).await;

    let wire = hit_sse(
        &app,
        "/d4",
        r#"{"declared":"DECLARED_VAL_B","undeclared":"UNDECLARED_VAL_B"}"#,
    )
    .await;

    assert!(
        wire.contains("d=DECLARED_VAL_B"),
        "§37.b D1 — the declared parameter must bind. Wire:\n{wire}"
    );
    assert!(
        wire.contains("${undeclared}") && !wire.contains("UNDECLARED_VAL_B"),
        "§37.b D4 — a body field matching NO declared parameter must \
         NOT bind: `${{undeclared}}` survives un-interpolated and the \
         body value never enters the interpolation scope. Wire:\n{wire}"
    );
}

// ── §4 — D5: a parameter-less flow is unaffected ────────────────────

#[tokio::test]
async fn s4_d5_parameterless_flow_streams_clean() {
    let app = build_router(server_cfg());
    // A type-level `output: Stream<Token>` flow → the `axon` wire
    // dialect (W3C named events) → the terminator is `axon.complete`.
    let src = "flow PlainFlow() -> Unit {\n\
             step Reply { ask: \"static prompt\" output: Stream<Token> }\n\
         }\n\
         axonendpoint PlainE { public: true method: POST path: \"/plain\" \
             execute: PlainFlow backend: stub transport: sse }";
    deploy(&app, src).await;

    let wire = hit_sse(&app, "/plain", "{}").await;

    assert!(
        wire.contains("axon.complete") && !wire.contains("axon.error"),
        "§37.b D5 — a parameter-less flow behind an endpoint with no \
         `body:` streams byte-clean; the Request Binding Contract is \
         inert when there is nothing to bind. Wire:\n{wire}"
    );
}

// ── §5 — D1: the contract holds on the JSON transport ───────────────

#[tokio::test]
async fn s5_request_binding_holds_on_the_json_transport() {
    let app = build_router(server_cfg());
    // The JSON dynamic route runs through the synchronous runner
    // (`execute_handler` → `execute_with_fallback` → `server_execute`
    // → `runner::execute_server_flow`); 37.b threads the request body
    // through that whole chain and seeds each `ExecContext`. This
    // exercises the chain end-to-end; the value-level correctness of
    // the binding is pinned by the `request_binding` unit pack and by
    // §1–§3 above (the same `bind_request_body` + `interpolate_vars`).
    let src = "type JsonBody { payload: String }\n\
         flow JsonFlow(payload: String) -> Unit {\n\
             step Reply { ask: \"got ${payload}\" output: String }\n\
         }\n\
         axonendpoint JsonE { public: true method: POST path: \"/json\" \
             body: JsonBody execute: JsonFlow backend: stub transport: json }";
    deploy(&app, src).await;

    let resp = hit_json(&app, "/json", r#"{"payload":"JSON_VALUE_B2"}"#).await;

    // §Fase 39.b — the JSON transport now returns a FlowEnvelope: no
    // top-level `success`; the step trail lives under `step_audit`. A
    // clean execution carries `errors: 0`.
    assert_eq!(
        resp.pointer("/step_audit/errors").and_then(|v| v.as_u64()),
        Some(0),
        "§37.b D1 — a parameterised flow behind `transport: json` must \
         execute through the synchronous runner with the request body \
         threaded + bound (FlowEnvelope, errors:0). Response: {resp}"
    );
    assert_eq!(
        resp.pointer("/step_audit/steps_executed").and_then(|v| v.as_u64()),
        Some(1),
        "§37.b — the single step executed. Response: {resp}"
    );
}
