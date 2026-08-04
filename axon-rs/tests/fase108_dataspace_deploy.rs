//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 108.b — the dataspace deploy hook: a DECLARED dataspace is
//! INSTANTIATED in the deterministic columnar engine at deploy time.
//!
//! The ground-truth finding this pins against regression: before §108.b,
//! `dataspace_specs` was `#[serde(skip)]`-hidden and read by nothing — a
//! declared dataspace reached no runtime state at all. These tests deploy
//! through the REAL `/v1/deploy` handler (the §95.f real-path discipline)
//! and assert the engine holds the declared store with the declared
//! schema.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::util::ServiceExt;

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

async fn deploy(app: axum::Router, src: &str) -> serde_json::Value {
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
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or_default()
}

#[tokio::test]
async fn deploy_instantiates_the_declared_dataspace_in_the_engine() {
    let (app, state) = axon::axon_server::build_router_with_state(server_cfg());
    let src = r#"
dataspace Leads {
    column email: Text
    column score: Float
    column visits: Int
}

flow Noop() -> Text {
    let x = "ok"
}
"#;
    let json = deploy(app, src).await;
    assert_eq!(
        json.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "deploy must succeed: {json}"
    );

    let engine = {
        let s = state.lock().unwrap();
        s.dataspace_engine.clone()
    };
    let engine = engine.read().unwrap();
    let store = engine
        .store("Leads")
        .expect("the DECLARED dataspace must exist in the engine — the §108 ground-truth fix");
    assert_eq!(store.schema().len(), 3);
    assert_eq!(store.column_index("email"), Some(0));
    assert_eq!(
        store.schema()[1].1,
        axon::dataspace_engine::ColumnType::Float
    );
    assert_eq!(store.row_count(), 0, "born empty — ingest is §108.c");
}

#[tokio::test]
async fn redeploy_replaces_the_store_new_names_accumulate() {
    let (app, state) = axon::axon_server::build_router_with_state(server_cfg());
    deploy(
        app.clone(),
        "dataspace A { column x: Int }\nflow F() -> Text { let v = \"1\" }",
    )
    .await;
    deploy(
        app,
        "dataspace B { column y: Text }\nflow G() -> Text { let v = \"2\" }",
    )
    .await;

    let engine = {
        let s = state.lock().unwrap();
        s.dataspace_engine.clone()
    };
    let engine = engine.read().unwrap();
    assert!(engine.store("A").is_some(), "earlier deploy's store survives");
    assert!(engine.store("B").is_some(), "new deploy's store lands");
}

/// §108.c — THE end-to-end: deploy a program whose flow `let`s raw CSV
/// and `ingest`s it into a declared dataspace, execute through the REAL
/// `/v1/execute` handler (the §95.f discipline), and observe REAL rows
/// in the engine — typed, bounded, born-Untrusted. This is the moment
/// `ingest` stops being a narration: pre-§108 this exact program would
/// have sent "You are ingesting…" to a language model.
#[tokio::test]
async fn deployed_flow_ingest_loads_real_rows_end_to_end() {
    let (app, state) = axon::axon_server::build_router_with_state(server_cfg());
    let src = r#"
dataspace Leads {
    column email: Text
    column score: Float
}

flow LoadLeads() -> Text {
    let raw_leads = "email,score\na@x.com,0.9\nb@x.com,0.4\nc@x.com,0.7\n"
    ingest raw_leads into Leads { format: csv, limits { max_bytes: 4096, max_rows: 100 } }
}
"#;
    let json = deploy(app.clone(), src).await;
    assert_eq!(
        json.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "deploy must succeed: {json}"
    );

    let body = serde_json::json!({ "flow": "LoadLeads", "backend": "stub" });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let exec_body = resp.into_body().collect().await.unwrap().to_bytes();
    let exec_json: serde_json::Value = serde_json::from_slice(&exec_body).unwrap_or_default();

    let engine = {
        let s = state.lock().unwrap();
        s.dataspace_engine.clone()
    };
    let engine = engine.read().unwrap();
    let store = engine.store("Leads").expect("declared store");
    assert_eq!(
        store.row_count(),
        3,
        "the rows are REAL — typed and counted (execute response: {exec_json})"
    );
    let batch = &store.batches()[0];
    assert_eq!(
        batch.provenance().taint,
        axon::emcp::EpistemicTaint::Untrusted,
        "external data is born Untrusted (§98)"
    );
    assert_eq!(batch.provenance().source_sha256.len(), 64, "witness hash");
    assert_eq!(batch.column(0).unwrap().get_text(2), Some("c@x.com"));
    assert_eq!(batch.column(1).unwrap().get_float(0), Some(0.9));
}

/// §108.d — THE aggregate E2E: ingest then γ in one deployed flow,
/// through the real HTTP path. The number in the result was COMPUTED
/// over the columnar batches — pre-§108 this exact `aggregate` step
/// prompted a model to "group + summarize" and returned prose.
#[tokio::test]
async fn deployed_flow_aggregates_real_numbers_end_to_end() {
    let (app, state) = axon::axon_server::build_router_with_state(server_cfg());
    let src = r#"
dataspace Sales {
    column region: Text
    column amount: Float
}

flow Report() -> Text {
    let raw = "region,amount
north,10.0
north,30.0
south,5.0
"
    ingest raw into Sales { format: csv }
    aggregate Sales { group_by: [region], compute: [count, sum(amount)], as: by_region }
    return by_region
}
"#;
    let json = deploy(app.clone(), src).await;
    assert_eq!(json.get("success").and_then(|v| v.as_bool()), Some(true), "{json}");

    let body = serde_json::json!({ "flow": "Report", "backend": "stub" });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let out: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    // The server surfaces the deterministic envelope as parsed JSON
    // (an object), string-wrapped only on older paths — accept both.
    let envelope: serde_json::Value = match &out["result"] {
        serde_json::Value::String(sv) => serde_json::from_str(sv)
            .unwrap_or_else(|_| panic!("result is the deterministic envelope, got: {out}")),
        v => v.clone(),
    };
    let rows = envelope["rows"].as_array().expect("rows array");
    assert_eq!(rows.len(), 2, "two regions: {envelope}");
    assert_eq!(rows[0]["region"], "north");
    assert_eq!(rows[0]["count"], 2);
    assert_eq!(rows[0]["sum_amount"], 40.0, "COMPUTED, not narrated");
    assert_eq!(rows[1]["region"], "south");
    assert_eq!(
        envelope["taint"], "untrusted",
        "an aggregate over untrusted batches is born untrusted (plan 5.4)"
    );
    // The engine held the real rows behind the number.
    let engine = {
        let s = state.lock().unwrap();
        s.dataspace_engine.clone()
    };
    assert_eq!(engine.read().unwrap().store("Sales").unwrap().row_count(), 3);
}

#[tokio::test]
async fn a_t928_violation_refuses_the_deploy() {
    // The compile gate runs inside deploy: an empty-schema dataspace is
    // refused at the type_checker phase — the engine never sees it.
    let (app, state) = axon::axon_server::build_router_with_state(server_cfg());
    let json = deploy(
        app,
        "dataspace Empty { }\nflow F() -> Text { let v = \"1\" }",
    )
    .await;
    assert_eq!(json.get("success").and_then(|v| v.as_bool()), Some(false));
    assert!(
        json.get("error")
            .and_then(|e| e.as_str())
            .unwrap_or_default()
            .contains("T928"),
        "the refusal names the law: {json}"
    );
    let engine = {
        let s = state.lock().unwrap();
        s.dataspace_engine.clone()
    };
    assert!(
        engine.read().unwrap().is_empty(),
        "a refused deploy must leave no engine state"
    );
}

// ── §108.x (D108.6) — the re-founded /v1/dataspace REST surface ─────────────

async fn post_json(app: axum::Router, uri: &str, body: serde_json::Value) -> serde_json::Value {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or_default()
}

async fn get_json(app: axum::Router, uri: &str) -> serde_json::Value {
    let req = Request::builder().method("GET").uri(uri).body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or_default()
}

#[tokio::test]
async fn rest_surface_is_a_view_over_declared_dataspaces() {
    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());
    deploy(
        app.clone(),
        "dataspace Metrics { column city: Text\n column pop: Int }\nflow F() -> Text { let v = \"1\" }",
    )
    .await;

    // (1) Free-form creation is RETIRED (D108.6) — the refusal teaches.
    let r = post_json(app.clone(), "/v1/dataspace", serde_json::json!({"name": "Rogue"})).await;
    assert_eq!(r["retired"], true, "{r}");
    assert!(r["error"].as_str().unwrap().contains("axon-T928"), "{r}");

    // (2) The list shows the DECLARED store with its shape.
    let r = get_json(app.clone(), "/v1/dataspace").await;
    assert_eq!(r["total"], 1, "{r}");
    assert_eq!(r["dataspaces"][0]["dataspace"], "Metrics");

    // (3) Governed HTTP ingest through the SAME loaders.
    let r = post_json(
        app.clone(),
        "/v1/dataspace/Metrics/ingest",
        serde_json::json!({"source": "city,pop\nbogota,8000000\nmedellin,2500000\n", "format": "csv"}),
    )
    .await;
    assert_eq!(r["rows"], 2, "{r}");
    assert_eq!(r["taint"], "untrusted");
    assert_eq!(r["source_sha256"].as_str().unwrap().len(), 64);

    // (4) A typed refusal, not a coercion.
    let r = post_json(
        app.clone(),
        "/v1/dataspace/Metrics/ingest",
        serde_json::json!({"source": "city,pop\nx,not_a_number\n", "format": "csv"}),
    )
    .await;
    assert!(r["error"].as_str().unwrap().contains("`pop`"), "{r}");

    // (5) Aggregate: computed, with taint + stats.
    let r = post_json(
        app.clone(),
        "/v1/dataspace/Metrics/aggregate",
        serde_json::json!({"compute": ["count", "sum(pop)"]}),
    )
    .await;
    assert_eq!(r["rows"][0]["count"], 2, "{r}");
    assert_eq!(r["rows"][0]["sum_pop"], 10500000.0);
    assert_eq!(r["taint"], "untrusted");

    // (6) Explore: shape, never content.
    let r = get_json(app.clone(), "/v1/dataspace/Metrics/explore").await;
    assert!(!r.to_string().contains("bogota"), "shape only: {r}");

    // (7) DELETE clears data; the DECLARATION persists.
    let req = Request::builder()
        .method("DELETE")
        .uri("/v1/dataspace/Metrics")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let r: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(r["success"], true, "{r}");
    let r = get_json(app, "/v1/dataspace").await;
    assert_eq!(r["total"], 1, "the declared store survives a clear");
    assert_eq!(r["dataspaces"][0]["rows"], 0, "its data does not");
}
