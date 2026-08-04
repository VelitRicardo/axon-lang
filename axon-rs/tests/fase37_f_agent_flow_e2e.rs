//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 37.f (D1, D3, D5) — the Request Binding Contract end-to-end
//! on the canonical agent flow.
//!
//! 37.b–37.e shipped the contract's parts; 37.f proves them together
//! on the shape that matters — a parameterised agent flow behind a
//! streaming `axonendpoint body: T`, hit with a real HTTP request body:
//!
//!   §1 — the founder's `ChatFlow` shape (retrieve ×3 → step → persist),
//!        parameterised, behind `transport: sse`: every body parameter
//!        reaches the `step` deliberation; the mixed flow streams clean.
//!   §2 — the data round-trips: a body parameter is `persist`ed into an
//!        `in_memory` store and `retrieve`d back into a downstream step.
//!   §3 — the error path stays honest: an agent flow that errors names
//!        WHY on the wire (ties 37.e), even with the body bound.
//!   §4 — the JSON transport mirror — the agent flow runs on
//!        `transport: json` with the body threaded + bound.
//!   §5 — D3: an adversarial body value (SQL metacharacters + a nested
//!        `${...}` token) flows through the agent pattern INERT — it is
//!        data, never re-interpreted, never an injection.
//!   §6 — D5: the pre-37 shape (a parameter-less flow, no `body:`)
//!        streams byte-clean — the contract is inert when unused.
//!
//! All `in_memory` — zero external infrastructure (Fase 36.x.b). The
//! `stub_stream` echo tool echoes its interpolated argument onto the
//! wire, so `${param}` substitution is directly observable.

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
    assert_eq!(status, StatusCode::OK, "37.f deploy failed: {json}");
}

async fn hit_sse(app: &axum::Router, path: &str, body: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "37.f {path} status");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

async fn hit_json(app: &axum::Router, path: &str, body: &str) -> serde_json::Value {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "37.f {path} status");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or_default()
}

const ECHO_TOOL: &str = "tool Echo { provider: stub_stream description: \"echo\" \
                          effects: <stream:drop_oldest> }\n";

// ── §1 — the founder's ChatFlow shape, parameterised, E2E ───────────

#[tokio::test]
async fn s1_founder_chatflow_shape_threads_every_body_param() {
    let app = build_router(server_cfg());
    // retrieve ×3 (context) → step (deliberate) → persist — the exact
    // mixed agent shape from the founder's 2026-05-17 hypothesis, now
    // parameterised and fed from a real request body.
    let src = format!(
        "type ChatBody {{ message: String session_id: String tenant_id: String }}\n\
         axonstore mem {{ backend: in_memory }}\n\
         {ECHO_TOOL}\
         flow ChatFlow(message: String, session_id: String, tenant_id: String) -> Unit {{\n\
             retrieve mem {{ where: \"history\" as: h1 }}\n\
             retrieve mem {{ where: \"profile\" as: h2 }}\n\
             retrieve mem {{ where: \"prefs\" as: h3 }}\n\
             step Deliberate {{ ask: \"t=${{tenant_id}} s=${{session_id}} m=${{message}}\" \
                 apply: Echo }}\n\
             persist into mem {{ stage: \"replied\" }}\n\
         }}\n\
         axonendpoint ChatE {{ public: true method: POST path: \"/chat\" \
             body: ChatBody execute: ChatFlow backend: stub transport: sse(axon) }}"
    );
    deploy(&app, &src).await;

    let wire = hit_sse(
        &app,
        "/chat",
        r#"{"message":"MSG_F1","session_id":"SES_F1","tenant_id":"TEN_F1"}"#,
    )
    .await;

    assert!(
        wire.contains("t=TEN_F1") && wire.contains("s=SES_F1") && wire.contains("m=MSG_F1"),
        "§37.f D1 — every body parameter must thread to the `step` \
         deliberation through the mixed retrieve→step→persist flow. \
         Wire:\n{wire}"
    );
    assert!(
        wire.contains("axon.complete") && !wire.contains("axon.error"),
        "§37.f D1 — the founder's mixed agent flow streams to a clean \
         terminator. Wire:\n{wire}"
    );
}

// ── §2 — the data round-trips through an in_memory store ────────────

#[tokio::test]
async fn s2_body_param_round_trips_through_the_store() {
    let app = build_router(server_cfg());
    // `persist` snapshots the bound parameters; `retrieve` reads one
    // back by name into `loaded`; the step echoes it. If `${secret}`
    // never bound, nothing is persisted and `loaded` is empty.
    let src = format!(
        "type RtBody {{ secret: String }}\n\
         axonstore mem {{ backend: in_memory }}\n\
         {ECHO_TOOL}\
         flow RoundTrip(secret: String) -> Unit {{\n\
             persist into mem {{ stage: \"saved\" }}\n\
             retrieve mem {{ where: \"secret\" as: loaded }}\n\
             step Reply {{ ask: \"loaded=${{loaded}}\" apply: Echo }}\n\
         }}\n\
         axonendpoint RtE {{ public: true method: POST path: \"/rt\" \
             body: RtBody execute: RoundTrip backend: stub transport: sse(axon) }}"
    );
    deploy(&app, &src).await;

    let wire = hit_sse(&app, "/rt", r#"{"secret":"ROUNDTRIP_F2"}"#).await;

    assert!(
        wire.contains("loaded=ROUNDTRIP_F2"),
        "§37.f D1 — a body parameter bound by the Request Binding \
         Contract must `persist` into the store and `retrieve` back \
         into a downstream step — the agent pattern's full data \
         pipeline. Wire:\n{wire}"
    );
}

// ── §3 — the error path stays honest ────────────────────────────────

#[tokio::test]
async fn s3_errored_agent_flow_names_why_on_the_wire() {
    let app = build_router(server_cfg());
    // A `postgresql` store pointed at a dead port deploys (lazy build)
    // then errors mid-walk at the `retrieve` node when the connection
    // is refused. Even with the body bound, the wire must say WHY.
    // (§Fase 35.f closed the axonstore catalog to `{in_memory,
    // postgresql}`; `sqlite` — the pre-§35.f vehicle — is now rejected
    // at DEPLOY time, so the route would never mount.)
    let src = format!(
        "type ErrBody {{ tenant_id: String }}\n\
         axonstore bad {{ backend: postgresql \
             connection: \"postgres://127.0.0.1:1/axon_37f_dead\" }}\n\
         {ECHO_TOOL}\
         flow ErrAgent(tenant_id: String) -> Unit {{\n\
             retrieve bad {{ where: \"ctx\" as: r }}\n\
             step Reply {{ ask: \"t=${{tenant_id}}\" apply: Echo }}\n\
         }}\n\
         axonendpoint ErrAgentE {{ public: true method: POST path: \"/erragent\" \
             body: ErrBody execute: ErrAgent backend: stub transport: sse(axon) }}"
    );
    deploy(&app, &src).await;

    let wire = hit_sse(&app, "/erragent", r#"{"tenant_id":"TEN_F3"}"#).await;

    assert!(
        wire.contains("axon.error") && wire.contains("failed at retrieve from 'bad'"),
        "§37.f D6 — an agent flow that errors emits an honest \
         `axon.error` naming the cause (the failing node), not a \
         hollow terminator. Wire:\n{wire}"
    );
    assert!(
        !wire.contains("axon.complete"),
        "§37.f D1 — exactly one terminator: an errored flow emits \
         `axon.error`, never also `axon.complete`. Wire:\n{wire}"
    );
}

// ── §4 — the JSON transport mirror ──────────────────────────────────

#[tokio::test]
async fn s4_agent_flow_runs_on_the_json_transport() {
    let app = build_router(server_cfg());
    let src = "type JBody { topic: String }\n\
        axonstore mem { backend: in_memory }\n\
        flow JsonAgent(topic: String) -> Unit {\n\
            retrieve mem { where: \"ctx\" as: r }\n\
            step Reply { ask: \"on ${topic}\" output: String }\n\
            persist into mem { stage: \"done\" }\n\
        }\n\
        axonendpoint JsonAgentE { public: true method: POST path: \"/jsonagent\" \
            body: JBody execute: JsonAgent backend: stub transport: json }";
    deploy(&app, src).await;

    let resp = hit_json(&app, "/jsonagent", r#"{"topic":"TOPIC_F4"}"#).await;

    // §Fase 39.b — the JSON transport returns a FlowEnvelope: no
    // top-level `success`; the step trail lives under `step_audit`. A
    // clean execution carries `errors: 0`.
    assert_eq!(
        resp.pointer("/step_audit/errors").and_then(|v| v.as_u64()),
        Some(0),
        "§37.f D1 — the agent flow runs through the synchronous JSON \
         transport with the request body threaded + bound (FlowEnvelope, \
         errors:0). Response: {resp}"
    );
    assert_eq!(
        resp.pointer("/step_audit/steps_executed").and_then(|v| v.as_u64()),
        Some(3),
        "§37.f — all three nodes (retrieve, step, persist) executed. \
         Response: {resp}"
    );
}

// ── §5 — D3: an adversarial body value is inert data ────────────────

#[tokio::test]
async fn s5_d3_adversarial_body_value_is_inert_data() {
    let app = build_router(server_cfg());
    let src = format!(
        "type AdvBody {{ payload: String }}\n\
         {ECHO_TOOL}\
         flow AdvFlow(payload: String) -> Unit {{\n\
             step Reply {{ ask: \"got=[${{payload}}]\" apply: Echo }}\n\
         }}\n\
         axonendpoint AdvE {{ public: true method: POST path: \"/adv\" \
             body: AdvBody execute: AdvFlow backend: stub transport: sse(axon) }}"
    );
    deploy(&app, &src).await;

    // The body value carries SQL metacharacters AND a nested `${...}`
    // interpolation token. It must flow through as INERT DATA: echoed
    // verbatim, never re-interpreted, never an injection, no crash.
    let wire = hit_sse(
        &app,
        "/adv",
        r#"{"payload":"'; DROP TABLE x; -- ${nested}"}"#,
    )
    .await;

    assert!(
        wire.contains("got=['; DROP TABLE x; -- ${nested}]"),
        "§37.f D3 — an adversarial body value is INERT DATA: \
         interpolated ONCE into the step argument, echoed verbatim, \
         the nested `${{nested}}` NOT re-interpreted. Wire:\n{wire}"
    );
    assert!(
        wire.contains("axon.complete") && !wire.contains("axon.error"),
        "§37.f D3 — the adversarial value does not break the flow. \
         Wire:\n{wire}"
    );
}

// ── §6 — D5: the pre-37 shape streams byte-clean ────────────────────

#[tokio::test]
async fn s6_d5_parameterless_flow_unaffected() {
    let app = build_router(server_cfg());
    let src = "flow PlainFlow() -> Unit {\n\
            step S { ask: \"static\" output: Stream<Token> }\n\
        }\n\
        axonendpoint PlainE { public: true method: POST path: \"/plain37f\" \
            execute: PlainFlow backend: stub transport: sse }";
    deploy(&app, src).await;

    let wire = hit_sse(&app, "/plain37f", "{}").await;

    assert!(
        wire.contains("axon.complete") && !wire.contains("axon.error"),
        "§37.f D5 — a parameter-less flow behind an endpoint with no \
         `body:` streams byte-clean; the Request Binding Contract is \
         inert when there is nothing to bind. Wire:\n{wire}"
    );
}
