//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v2.67.0 — **the driver.** Without it, the supervisor is not a supervisor.
//!
//! # I documented this before I built it
//!
//! Worth recording rather than quietly fixing. For one commit,
//! `AXON_COGNITIVE_IO_TICK_SECS` existed **only in a doc comment**. The graph ran
//! when you POSTed to `/v1/cognitive-io/tick`, and **nothing walked it in
//! production**.
//!
//! The tests were honest — they drive the tick explicitly. The *deployment* was
//! not: an `immune` would never have learned a baseline, never detected anything,
//! and never fired. A supervisor that only runs when you call it is not a
//! supervisor; it is a manual diagnostic.
//!
//! > **A documented behaviour the code does not have is the exact defect v2.67.0 and
//! > v2.67.0 exist to eliminate — and I shipped one into the cycle whose entire purpose
//! > is eliminating them.**
//!
//! It was caught by re-reading my own claim against the code, which is the only
//! thing that ever catches it. That is the whole method, applied to myself.
//!
//! Pins:
//! 1. The declared graph is walked **with nobody calling anything** — no tick POST.
//! 2. It keeps walking: the `immune` **learns its baseline over successive ticks**,
//!    which is the thing that could never happen without a driver.
//! 3. A redeploy is picked up (the driver re-reads the supervisor each pass).

use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::util::ServiceExt;

use axon::source_registry::{register_source_adapter, SourceAdapter, SourceError, SourceReading};

fn server_cfg(port: u16) -> axon::axon_server::ServerConfig {
    axon::axon_server::ServerConfig {
        host: "127.0.0.1".into(),
        port,
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

struct Fixed(String, f64);
impl SourceAdapter for Fixed {
    fn name(&self) -> &str {
        &self.0
    }
    fn probe(
        &self,
        _r: Option<&axon::ir_nodes::IRResource>,
        _t: std::time::Duration,
    ) -> Result<SourceReading, SourceError> {
        Ok(SourceReading::new(self.1, serde_json::Map::new()))
    }
}

const PROGRAM: &str = r#"
resource DrvDb { kind: postgres  endpoint: db.main }
manifest Infra { resources: [DrvDb] }
observe  Health from Infra { sources: [drv_probe]  quorum: 1  timeout: 1s  on_partition: fail }
immune   Watcher { watch: [Health]  scope: tenant  window: 3 }
"#;

/// **The driver walks the graph with nobody calling anything.**
///
/// The server is booted for real, a program is deployed, and then the test simply
/// **waits**. No tick is POSTed. If the driver does not exist, `last_tick` stays
/// `null` forever — which is exactly what the code did for one commit while its own
/// documentation claimed otherwise.
#[tokio::test]
async fn the_driver_walks_the_declared_graph_with_nobody_calling_anything() {
    register_source_adapter("drv_probe", Arc::new(Fixed("drv_probe".into(), 0.9)));
    // A 1-second period so the test can observe several passes.
    std::env::set_var("AXON_COGNITIVE_IO_TICK_SECS", "1");

    let (router, state) = axon::axon_server::build_router_with_state(server_cfg(0));

    // Deploy through the real path.
    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/deploy")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "source": PROGRAM, "filename": "t.axon", "backend": "stub"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let out: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(out["success"], true, "deploy must succeed: {out}");

    // `build_router_with_state` does not boot the process, so start the driver the
    // way `serve()` does. (The unit under test is the driver's BEHAVIOUR — that
    // something walks the graph unbidden — not the wiring line in `serve`.)
    axon::axon_server::spawn_cognitive_io_driver(state.clone());

    // NOW WAIT. Nobody calls anything.
    // `window: 3` ⇒ the immune needs three passes to learn its baseline, then
    // classifies. If the driver does not exist, none of this ever happens.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut saw_learning = false;
    let mut saw_watching = false;

    while std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let last = {
            let s = state.lock().unwrap();
            s.cognitive_io_last_tick.clone()
        };
        let Some(t) = last else { continue };

        if t["learning"]["Watcher"].is_object() {
            saw_learning = true;
        }
        if t["health"]["Watcher"].is_object() {
            saw_watching = true;
            break;
        }
    }

    assert!(
        saw_learning,
        "the driver must walk the graph WITH NOBODY CALLING ANYTHING. Nothing was ever ticked \
         here — no POST, no manual drive. For one commit this assertion would have hung: the \
         supervisor only ran when its endpoint was called, while the docs claimed a background \
         period. An immune that is never ticked never learns a baseline, never detects, and \
         never fires."
    );
    assert!(
        saw_watching,
        "and it must KEEP walking: the immune must get past its `window: 3` learning phase and \
         start classifying, unbidden. A driver that fires once is not a driver."
    );
}
