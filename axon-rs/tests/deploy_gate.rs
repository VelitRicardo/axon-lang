//! v2.81.0 — this gate drives the HTTP deploy surface, which lives behind
//! the `server` feature. Gated at the file level so a `cli`-only build still
//! compiles.
#![cfg(feature = "server")]

//! v2.89.0 — a program the mounted backend cannot honour is refused at
//! DEPLOY, before it acts.
//!
//! # The charge that opened v2.89.0
//!
//! *"The compiler accepts `depth: live_network`; the OSS runtime rejects it. A
//! program that compiles and cannot run makes 'it compiles' a claim with no
//! content."*
//!
//! As stated it does not survive the code: enterprise implements all three
//! depths, and the OSS runtime refuses LOUDLY — `DepthNotSupported`, with a
//! gate proving it does not silently downgrade, which is the one behaviour that
//! would have been indefensible.
//!
//! What the charge got right is the TIMING. `can_analyze_depth` ran only inside
//! `analyze()`, so the refusal arrived mid-flow: a `warden` block on the
//! seventh step let the six before it emit, persist and deliver first. The
//! program was refused *after it had already acted*.
//!
//! the design decision (ratified): the deploy gate is the SOURCE OF TRUTH — not a rung below
//! a compile-time capability check. A compile-time check is only as honest as
//! the manifest it reads, so a build that declares a capability and mounts a
//! backend without it moves the lie earlier rather than removing it.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn server_cfg() -> axon::axon_server::ServerConfig {
    axon::axon_server::ServerConfig {
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

fn fixture(rel: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

async fn deploy(app: &axum::Router, src: &str) -> serde_json::Value {
    let body = serde_json::json!({ "source": src, "source_file": "gate.axon", "backend": "stub" });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// 🎯 The flagship. A scope above the mounted backend's ceiling REFUSES THE
/// DEPLOY — the program never gets to run its first step.
#[tokio::test]
async fn a_scope_the_backend_cannot_reach_refuses_the_deploy() {
    const FIXTURE: &str = "tests/fixtures/deploy_gate/live_network_scope.axon";
    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());

    let out = deploy(&app, &fixture(FIXTURE)).await;

    assert_eq!(
        out["success"], false,
        "a program whose declared scope the mounted engine cannot analyse must not deploy: {out}"
    );
    assert_eq!(
        out["phase"], "backend_capability",
        "the refusal must name its phase so an operator can tell it from a type error: {out}"
    );

    let err = out["error"].as_str().unwrap_or_default();
    assert!(
        err.contains("LiveCapture") && err.contains("live_network"),
        "the diagnostic must name the SCOPE and the DEPTH the adopter wrote: {err}"
    );
    assert!(
        err.contains("mid-flow"),
        "and it must say why this is a deploy-time refusal rather than a runtime one: {err}"
    );
}

/// The same program with a reachable depth deploys clean. Without this the test
/// above would pass for a gate that refuses everything.
#[tokio::test]
async fn a_scope_within_the_backends_ceiling_deploys() {
    const FIXTURE: &str = "tests/fixtures/deploy_gate/live_network_scope.axon";
    let lowered = fixture(FIXTURE).replace("depth: live_network", "depth: static_artifact");

    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());
    let out = deploy(&app, &lowered).await;

    assert_eq!(
        out["success"], true,
        "`static_artifact` is exactly what the OSS reference supports; refusing it would make \
         the gate a blanket ban rather than a capability check: {out}"
    );
}

/// An `observable` wider than the mounted simulator is the same defect in the
/// other primitive: v2.67.0 refuses it with `axon-E0783`, but at the `yield`.
///
/// The declaration below is LEGAL AXON — sixteen Z's across sixteen declared
/// qubits, internally consistent — and that is what makes it the right test.
/// The first draft used `qubits: 24` with a one-qubit `"Z"`, and the type
/// checker refused it first with `axon-E0785` (declared width must match the
/// Pauli strings). That is a different law doing a different job, and it hid
/// this one.
///
/// The two are complementary, not redundant: `axon-E0785` decides whether the
/// DECLARATION is coherent with itself; this gate decides whether a coherent
/// declaration is REALISABLE on the backend actually mounted. A program can be
/// impeccable in the language and still un-runnable here — which is exactly the
/// distinction v2.89.0 exists to make legible.
#[tokio::test]
async fn an_observable_above_the_simulator_capacity_refuses_the_deploy() {
    let src = r#"
observable TooWide {
    qubits: 16
    term: 1.0 * "ZZZZZZZZZZZZZZZZ"
}

flow Measure() -> Unit {
    quant(observable: TooWide, qubits: 16) {
        let surrogate = "[0.0]"
        yield surrogate
    }
}
"#;
    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());
    let out = deploy(&app, src).await;

    assert_eq!(
        out["success"], false,
        "16 qubits is above the OSS cap of 10, so this program cannot run here: {out}"
    );
    assert_eq!(out["phase"], "backend_capability");
    assert!(
        out["error"].as_str().unwrap_or_default().contains("TooWide"),
        "the diagnostic must name the observable the adopter declared: {out}"
    );
}

/// …and one that fits deploys. The cap is 10; 2 is inside it.
#[tokio::test]
async fn an_observable_within_capacity_deploys() {
    let src = r#"
observable Narrow {
    qubits: 2
    term: 1.0 * "ZZ"
}

flow Measure() -> Unit {
    quant(observable: Narrow, qubits: 2) {
        let surrogate = "[0.0, 0.0]"
        yield surrogate
    }
}
"#;
    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());
    let out = deploy(&app, src).await;
    assert_eq!(out["success"], true, "2 qubits is well within the cap: {out}");
}
