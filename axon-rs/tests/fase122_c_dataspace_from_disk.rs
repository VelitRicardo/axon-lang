//! §Fase 122.c — the §108 data plane, from a program that lives on disk.
//!
//! # What §108 found, and what this keeps closed
//!
//! `dataspace` was declared, type-checked and carried into the IR — and
//! `dataspace_specs` was `#[serde(skip)]`-hidden and read by nothing. A declared
//! dataspace reached no runtime state at all, so `ingest`, `focus`, `associate`,
//! `aggregate` and `explore` were NARRATION: this exact program would have sent
//! "You are ingesting…" to a language model and returned prose.
//!
//! Every one of those six rows was attested on an engine name
//! (`dataspace_engine::focus_query`, `cognitive::run_ingest`, …). §119.f.8 is
//! the lesson about what an engine name does not say.
//!
//! # Why this is NOT a live-store corpus
//!
//! Group C was scoped as needing a real database. Measured: it does not. The
//! §108 engine is first-party COLUMNAR and in-process, and the server's deploy
//! path builds it. What cannot reach it is the PARITY corpus, which passes
//! `None` for the engine — so §108's verbs fail closed there by design (the
//! honesty floor, `MissingDependency`). The distinction matters: "the parity
//! harness cannot drive it" is not "it needs Postgres".
//!
//! So this gate uses the §112 shape — deploy through `/v1/deploy`, execute
//! through `/v1/execute`, and read the REAL engine state afterwards.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

const FIXTURE: &str = "tests/fixtures/fase122_c_dataspace/lead_pipeline.axon";

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

fn fixture_source() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

async fn post(app: &axum::Router, uri: &str, body: serde_json::Value) -> serde_json::Value {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "{uri} must answer 200");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or_default()
}

#[tokio::test]
async fn a_deployed_fixture_ingests_real_rows_and_the_verbs_run_over_them() {
    let (app, state) = axon::axon_server::build_router_with_state(server_cfg());
    let src = fixture_source();

    let deployed = post(
        &app,
        "/v1/deploy",
        serde_json::json!({ "source": src, "source_file": FIXTURE, "backend": "stub" }),
    )
    .await;
    assert_eq!(
        deployed.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "the fixture must deploy: {deployed}"
    );

    let executed = post(
        &app,
        "/v1/execute",
        serde_json::json!({ "flow": "LoadAndSummarise", "backend": "stub" }),
    )
    .await;

    // Without this the test would prove `ingest` and nothing else: the flow's
    // other four verbs could each fail at dispatch and the row assertions below
    // would still pass. §108's verbs fail CLOSED (`MissingDependency`) when the
    // engine is absent, so a green execute is what says they ran.
    assert_ne!(
        executed.get("success").and_then(|v| v.as_bool()),
        Some(false),
        "every verb in the fixture must dispatch — focus / associate / aggregate / \
         explore fail closed when the engine is missing, so a failed execute means \
         one of them did not run: {executed}"
    );
    assert!(
        executed.get("error").is_none(),
        "no verb may error: {executed}"
    );

    // The rows are REAL — read from the engine the deploy instantiated, not
    // from the execute response.
    let engine = {
        let s = state.lock().unwrap();
        s.dataspace_engine.clone()
    };
    let engine = engine.read().unwrap();
    let store = engine
        .store("Leads")
        .expect("the DECLARED dataspace must exist in the engine (§108's ground truth)");

    assert_eq!(
        store.row_count(),
        3,
        "three CSV rows must land as three typed rows (execute: {executed})"
    );

    let batch = &store.batches()[0];
    assert_eq!(
        batch.provenance().taint,
        axon::emcp::EpistemicTaint::Untrusted,
        "external data is born Untrusted (§98) — that is not a default, it is the claim"
    );
    assert_eq!(
        batch.provenance().source_sha256.len(),
        64,
        "and it carries a witness hash"
    );

    // Typed, not stringly: the Float column reads back as a float.
    assert_eq!(
        batch.column(2).unwrap().get_float(0),
        Some(0.9),
        "the declared `column score: Float` must be typed storage"
    );
    assert_eq!(batch.column(0).unwrap().get_text(2), Some("c@x.com"));
}
