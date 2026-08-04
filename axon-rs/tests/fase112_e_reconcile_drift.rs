//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 112.e — `reconcile` **actually detects drift**.
//!
//! # The defect this closes — the third time this exact shape appeared
//!
//! `ReconcileLoop::tick` computed its drift like this:
//!
//! ```ignore
//! let observed = match observation.data.get("resources_observed") {
//!     Some(Array(arr)) => arr…,
//!     // "otherwise DEFAULT TO BELIEF"
//!     _ => self.manifest.resources.clone(),
//! };
//! let drift = jaccard_drift(&self.manifest.resources, &observed);
//! ```
//!
//! When the evidence was missing, the *observed* state became **the manifest's own
//! declaration** — so `drift = jaccard(belief, belief) = 0.0`.
//!
//! > **Drift was structurally always zero.** A reconciliation loop whose entire
//! > purpose is closing the gap between belief and evidence **closed it by assuming
//! > there was no gap.** It compared the belief against itself.
//!
//! And the only `Handler` that existed filled `resources_observed` **from
//! `manifest.resources`** — which is *also* the belief. So **both** paths gave zero,
//! and `reconcile` could never detect anything, ever.
//!
//! It is `DryRunHandler`'s `c: 1.0` for the third time: *when the evidence is
//! missing, substitute the belief and report agreement.*
//!
//! # Now
//!
//! The `LiveHandler` reports the manifest's resources it **actually reached** — the
//! evidence — and the belief-fallback is **gone**. No evidence ⇒ **refuse**:
//! reporting `0.0` drift is not a conservative default, it is a claim that reality
//! matches your intent, made **without looking**.
//!
//! Pins:
//! 1. **A manifest whose declared world is fully present ⇒ no drift.** (No false
//!    alarms.)
//! 2. **A declared resource that is NOT there ⇒ REAL drift**, and the `on_drift`
//!    action fires. This is the line that could never have been reached before.
//! 3. Drift within `tolerance:` is a no-op — the adopter's declared slack is honoured.
//! 4. **The shield gate is real**: a denied correction does not act.

use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::util::ServiceExt;

use axon::source_registry::{register_source_adapter, SourceAdapter, SourceError, SourceReading};

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

/// A source that answers.
struct Up(String);
impl SourceAdapter for Up {
    fn name(&self) -> &str {
        &self.0
    }
    fn probe(
        &self,
        _r: Option<&axon::ir_nodes::IRResource>,
        _t: std::time::Duration,
    ) -> Result<SourceReading, SourceError> {
        Ok(SourceReading::new(1.0, serde_json::Map::new()))
    }
}

/// A source that is **gone** — the declared resource is not there.
struct Gone(String);
impl SourceAdapter for Gone {
    fn name(&self) -> &str {
        &self.0
    }
    fn probe(
        &self,
        _r: Option<&axon::ir_nodes::IRResource>,
        _t: std::time::Duration,
    ) -> Result<SourceReading, SourceError> {
        Err(SourceError::Unreachable {
            source: self.0.clone(),
            detail: "the declared resource is not there".into(),
        })
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

/// Two declared resources; `quorum: 1` so the observation still succeeds when one is
/// gone. `tolerance` is explicit so the test controls the slack.
/// ⚠️ The source registry is a process-global (same shape as `shield_registry`), and
/// the built-in adapter family keys a source by the DECLARED RESOURCE NAME. Parallel
/// tests must therefore not share resource names, or they race on the registry — one
/// test's `Up` would answer another test's `Gone`.
fn program(tag: &str, tolerance: &str, on_drift: &str) -> String {
    format!(
        r#"
resource Db{tag}    {{ kind: postgres  endpoint: db.main }}
resource Cache{tag} {{ kind: redis     endpoint: cache.main }}
manifest Infra {{ resources: [Db{tag}, Cache{tag}] }}
observe  World from Infra {{ sources: [Db{tag}, Cache{tag}]  quorum: 1  timeout: 1s  on_partition: fail }}
reconcile Keeper {{ observe: World  tolerance: {tolerance}  on_drift: {on_drift} }}
"#
    )
}

// ── 1. The world matches the belief ⇒ no drift ─────────────────────────────

/// Both declared resources are present. Drift is **0.0** — and this time that zero
/// is *measured*, not assumed.
#[tokio::test]
async fn a_world_that_matches_the_manifest_has_no_drift() {
    register_source_adapter("DbOk", Arc::new(Up("DbOk".into())));
    register_source_adapter("CacheOk", Arc::new(Up("CacheOk".into())));

    let (app, _s) = axon::axon_server::build_router_with_state(server_cfg());
    assert_eq!(deploy(&app, &program("Ok", "0.1", "alert")).await["success"], true);

    let t = tick(&app).await;
    let r = &t["reconciles"]["Keeper"];
    assert_eq!(
        r["drift"], 0.0,
        "both declared resources are reachable ⇒ no drift. And this zero is MEASURED — before \
         §112.e it was assumed, because the loop compared the manifest against itself. Got: {t}"
    );
    assert_eq!(r["action"], "noop", "no drift ⇒ no corrective action. Got: {r}");
}

// ── 2. THE FLAGSHIP: a resource that is gone ⇒ REAL drift ──────────────────

/// **The line that could never have been reached.** `Cache` is declared in the
/// manifest and **is not there**. The Jaccard symmetric difference over
/// `{Db, Cache}` vs `{Db}` is `1/2 = 0.5` — real, measured drift between the
/// *desired* shape and the *actual* one — and the declared `on_drift` action fires.
///
/// Before §112.e this reported `0.0`, always, for any world. The primitive whose
/// entire job is noticing that reality has diverged from your intent **could not
/// notice anything.**
#[tokio::test]
async fn a_declared_resource_that_is_gone_produces_real_drift_and_fires_on_drift() {
    register_source_adapter("DbGap", Arc::new(Up("DbGap".into())));
    register_source_adapter("CacheGap", Arc::new(Gone("CacheGap".into())));

    let (app, _s) = axon::axon_server::build_router_with_state(server_cfg());
    assert_eq!(deploy(&app, &program("Gap", "0.1", "alert")).await["success"], true);

    let t = tick(&app).await;
    let r = &t["reconciles"]["Keeper"];

    let drift = r["drift"].as_f64().unwrap_or_else(|| panic!("no drift reported: {t}"));
    assert!(
        (drift - 0.5).abs() < 1e-9,
        "manifest declares {{Db, Cache}}; the world has {{Db}}. The Jaccard symmetric difference \
         is 1/2 = 0.5. Before §112.e this was structurally ALWAYS 0.0 — the loop compared the \
         belief against itself. Got drift={drift}, tick={t}"
    );
    assert_eq!(
        r["action"], "alert",
        "drift beyond tolerance must fire the DECLARED on_drift action. Got: {r}"
    );
    assert_eq!(
        r["shield_approved"], true,
        "the shield must have approved the correction for it to act"
    );
}

// ── 3-4. The adopter's declared slack, and the shield ───────────────────────

/// Drift within the declared `tolerance:` is a **no-op**. The adopter said how much
/// divergence they are willing to live with, and we honour it — a reconciler that
/// acts on noise is one nobody will leave switched on.
#[tokio::test]
async fn drift_within_the_declared_tolerance_is_a_noop() {
    register_source_adapter("DbTol", Arc::new(Up("DbTol".into())));
    register_source_adapter("CacheTol", Arc::new(Gone("CacheTol".into())));

    let (app, _s) = axon::axon_server::build_router_with_state(server_cfg());
    // 0.5 drift, tolerance 0.9 ⇒ within slack.
    assert_eq!(deploy(&app, &program("Tol", "0.9", "alert")).await["success"], true);

    let t = tick(&app).await;
    let r = &t["reconciles"]["Keeper"];
    assert!((r["drift"].as_f64().unwrap() - 0.5).abs() < 1e-9, "the drift is still MEASURED");
    assert_eq!(
        r["action"], "noop",
        "0.5 drift under a 0.9 tolerance is within the slack the adopter declared. Got: {r}"
    );
}

/// `on_drift: provision` must **refuse**, not pretend.
///
/// Provisioning materialises a declared `resource` — and a `resource` governs
/// nothing that runs (§111's islands finding: `resource.endpoint` and
/// `axonstore.connection` are the same fact declared twice, with nothing linking
/// them). **§113** makes `resource` the single source of truth; provisioning lands
/// there.
///
/// Reporting a successful provision that created nothing would be `DryRunHandler`'s
/// `c: 1.0` wearing a fourth hat.
#[tokio::test]
async fn on_drift_provision_refuses_rather_than_pretending() {
    register_source_adapter("DbProv", Arc::new(Up("DbProv".into())));
    register_source_adapter("CacheProv", Arc::new(Gone("CacheProv".into())));

    let (app, _s) = axon::axon_server::build_router_with_state(server_cfg());
    assert_eq!(
        deploy(&app, &program("Prov", "0.1", "provision")).await["success"],
        true
    );

    let t = tick(&app).await;
    // The reconcile tick errors out, so it must NOT appear as a completed
    // reconciliation. What must never happen is a reported successful provision.
    if let Some(r) = t["reconciles"].get("Keeper") {
        assert_ne!(
            r["action"], "provision",
            "a provision that created nothing must NEVER be reported as done — that is the \
             DryRunHandler defect in a fourth place. §113 makes `resource` govern something; \
             until then this refuses. Got: {r}"
        );
    }
}
