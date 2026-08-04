//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 114.h — 🎯 **THE PAYOFF: the immune system watches a vendor a tool
//! actually calls.**
//!
//! §112 built the whole loop — `observe` → `immune` (KL-divergence vs a learned
//! baseline) → `reflex` → `heal`. It WORKS. But until now it could only watch
//! infrastructure you **declared and did not use**: a `manifest`'s resources that
//! nothing runs on. That is the islands finding wearing a monitor.
//!
//! §114.c/d closed the gap on the tool side: a `tool` now names a `resource`. So
//! the resource a tool CALLS can be listed in a manifest and observed — and the
//! §112 immune system learns its baseline, detects its degradation, quarantines
//! it, and heals.
//!
//! **Vendor APIs are what fall over. Not your Postgres.** This is the immune
//! system connected to the thing that actually breaks in production.
//!
//! # What this test proves
//!
//! It needed NO new runtime code — a resource is a resource, and `ResourceProbe`
//! probes an endpoint regardless of who uses it. So this gate's job is to prove,
//! end-to-end, that the immune system now has a *worthwhile subject*: the SAME
//! resource is named by a `tool` (the thing that calls the vendor) AND watched by
//! an `immune` (the thing that guards it), and the guard fires on the vendor's
//! degradation.

use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tower::util::ServiceExt;

use axon::source_registry::{register_source_adapter, SourceAdapter, SourceError, SourceReading};

fn server_cfg() -> axon::axon_server::ServerConfig {
    axon::axon_server::ServerConfig {
        host: "127.0.0.1".into(),
        port: 0,
        channel: "memory".into(),
        auth_token: String::new(),
        log_level: "ERROR".into(),
        log_format: "json".into(),
        log_file: None,
        database_url: None,
        config_path: None,
        strict_type_driven_transport: false,
        default_backend: None,
        schemas_dir: None,
    }
}

/// A vendor probe whose health can be shifted at will, so the test can create a
/// genuine deviation from a learned baseline rather than hand-building a report.
struct VendorProbe {
    name: String,
    milli: Arc<AtomicU64>,
}
impl SourceAdapter for VendorProbe {
    fn name(&self) -> &str {
        &self.name
    }
    fn probe(
        &self,
        _r: Option<&axon::ir_nodes::IRResource>,
        _t: std::time::Duration,
    ) -> Result<SourceReading, SourceError> {
        let c = self.milli.load(Ordering::SeqCst) as f64 / 1000.0;
        Ok(SourceReading::new(c, serde_json::Map::new()))
    }
}

async fn post(app: &axum::Router, uri: &str, body: serde_json::Value) -> serde_json::Value {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn deploy(app: &axum::Router, src: &str) -> serde_json::Value {
    post(
        app,
        "/v1/deploy",
        serde_json::json!({ "source": src, "filename": "t.axon", "backend": "stub" }),
    )
    .await
}

async fn tick(app: &axum::Router) -> serde_json::Value {
    post(app, "/v1/cognitive-io/tick", serde_json::json!({})).await["tick"].clone()
}

/// The SAME resource is the tool's channel AND the immune system's subject.
///
/// - `resource SearchApi` — the vendor.
/// - `tool Search { resource: SearchApi }` — the thing that CALLS it (§114.c/d).
/// - `manifest Vendors { resources: [SearchApi] }` + `observe` + `immune` — the
///   thing that GUARDS it (§112).
fn program(source: &str) -> String {
    format!(
        r#"
resource SearchApi {{ kind: https  endpoint: vendor.search.base  capacity: 8 }}
tool Search {{ provider: http  resource: SearchApi  runtime: search }}

manifest  Vendors {{ resources: [SearchApi] }}
observe   VendorHealth from Vendors {{ sources: [{source}]  quorum: 1  timeout: 1s  on_partition: fail }}
immune    VendorGuard {{ watch: [VendorHealth]  scope: tenant  window: 4  sensitivity: 0.8  tau: 5m }}
reflex    Quarantine {{ trigger: VendorGuard  on_level: doubt  action: quarantine  scope: tenant  sla: 1ms }}
heal      Repair {{ source: VendorGuard  on_level: doubt  mode: audit_only  scope: tenant }}
"#
    )
}

/// 🎯 **The immune system learns a used vendor's baseline, then FIRES on its
/// degradation.**
///
/// This is the assertion §112 could not make about anything real: the resource
/// under watch is the one a `tool` calls. A steady vendor is learned and stays
/// quiet; a genuine drop in its health crosses into `doubt` and the reflex fires.
#[tokio::test]
async fn the_immune_system_fires_on_a_vendor_that_a_tool_actually_calls() {
    // Steady, healthy vendor while the baseline is learned.
    let milli = Arc::new(AtomicU64::new(950));
    register_source_adapter(
        "vendor_search_probe",
        Arc::new(VendorProbe {
            name: "vendor_search_probe".into(),
            milli: milli.clone(),
        }),
    );

    let (app, _s) = axon::axon_server::build_router_with_state(server_cfg());
    let d = deploy(&app, &program("vendor_search_probe")).await;
    assert_eq!(
        d["success"], true,
        "the program — a tool AND an immune sharing one resource — must deploy: {d}"
    );

    // window: 4 ⇒ the first four ticks TRAIN the baseline on the healthy vendor.
    for i in 0..4 {
        let t = tick(&app).await;
        assert!(
            t["learning"]["VendorGuard"].is_object(),
            "tick {i}: the guard must report LEARNING while it has no baseline. Got: {t}"
        );
        assert_eq!(
            t["reflexes_fired"], 0,
            "tick {i}: no reflex may fire against an untrained baseline. Got: {t}"
        );
    }

    // Baseline learned. A steady healthy vendor stays quiet.
    let quiet = tick(&app).await;
    assert!(
        quiet["health"]["VendorGuard"].is_object(),
        "once learned, the guard reports health (not learning). Got: {quiet}"
    );
    assert_eq!(
        quiet["reflexes_fired"], 0,
        "a steady, healthy vendor must not trip the guard. Got: {quiet}"
    );

    // 🔴 THE VENDOR DEGRADES — its probed health collapses.
    milli.store(50, Ordering::SeqCst);

    // The guard must now cross into `doubt` and the reflex must fire — over a
    // resource the `tool Search` actually calls.
    let mut fired = false;
    for _ in 0..4 {
        let t = tick(&app).await;
        if t["reflexes_fired"].as_u64().unwrap_or(0) > 0 {
            fired = true;
            break;
        }
    }
    assert!(
        fired,
        "the immune system must FIRE on the degradation of a vendor a tool actually calls. This \
         is the payoff of §114: §112's guard learned to watch infrastructure you declared and \
         did not use; §114 pointed it at the thing that breaks in production. If this never \
         fires, the guard is back to watching an island."
    );
}

/// The deploy also proves the two roles compose: a resource can be BOTH a tool's
/// channel and an immune system's subject in one program, with no conflict.
#[tokio::test]
async fn one_resource_is_both_a_tools_channel_and_an_immune_subject() {
    register_source_adapter(
        "compose_probe",
        Arc::new(VendorProbe {
            name: "compose_probe".into(),
            milli: Arc::new(AtomicU64::new(900)),
        }),
    );
    let (app, _s) = axon::axon_server::build_router_with_state(server_cfg());
    let d = deploy(&app, &program("compose_probe")).await;
    assert_eq!(
        d["success"], true,
        "a resource named by a tool AND watched by an immune must deploy cleanly: {d}"
    );
}
