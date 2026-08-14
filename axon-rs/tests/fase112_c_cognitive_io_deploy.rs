//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 112.c — the Cognitive-I/O graph, live through the REAL deploy path.
//!
//! Every test here goes through `POST /v1/deploy` — the §95.f discipline §111 spent
//! itself learning. A supervisor that works when you hand-build it, and never gets
//! built by the thing that deploys programs, is exactly the class of defect this
//! whole line of work exists to end.
//!
//! # What was true before this
//!
//! `observe` · `ensemble` · `immune` · `reflex` · `heal` · `reconcile` were
//! declared, type-checked, carried into the IR — and **consumed by nothing**
//! (§111 F14). The kernels took the compiled IR *directly*; nobody had ever handed
//! them to one. Deploying a program with a fully-declared immune system produced
//! **no immune system.**
//!
//! Pins:
//! 1. Deploying a program with a Cognitive-I/O graph **instantiates** it.
//! 2. A tick **actually observes** — through a registered adapter, to a real target.
//! 3. The `immune` receives what it watches; the health report is **derived**, not
//!    defaulted.
//! 4. **A refused observation is reported as a refusal and feeds nothing.** A system
//!    we could not see is not a system that is fine.
//! 5. A program with no Cognitive-I/O declarations gets no supervisor (and pays
//!    nothing).
//! 6. An **invalid graph refuses the DEPLOY** — all-or-nothing. A half-instantiated
//!    immune system is worse than none, because it looks like one.

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

/// A source that answers with a fixed certainty — stands in for the enterprise
/// Prometheus/CloudWatch adapters without needing one.
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

async fn get(app: &axum::Router, uri: &str) -> serde_json::Value {
    let res = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
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

/// A fully-declared immune system: observe → immune → {reflex, heal}.
const IMMUNE_PROGRAM: &str = r#"
resource Db { kind: postgres  endpoint: db.main  lifetime: affine }
fabric   Vpc { provider: aws }
manifest Infra { resources: [Db]  fabric: Vpc }
observe  Health from Infra { sources: [dep_probe]  quorum: 1  timeout: 1s  on_partition: fail }
immune   Sentinel { watch: [Health]  scope: tenant  window: 8 }
reflex   Quarantine { trigger: Sentinel  on_level: doubt  action: quarantine  scope: tenant  sla: 1ms }
heal     Repair { source: Sentinel  on_level: doubt  mode: audit_only  scope: tenant }
"#;

// ── 1-3. The graph is instantiated, and it actually observes ────────────────

/// **The flagship.** Deploying a program with a declared immune system now
/// produces one — and a tick walks `observe` → `immune` → `reflex`/`heal` on an
/// observation that was *actually taken*.
#[tokio::test]
async fn deploying_a_declared_immune_system_produces_one() {
    register_source_adapter("dep_probe", Arc::new(Fixed("dep_probe".into(), 0.93)));
    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());

    let out = deploy(&app, IMMUNE_PROGRAM).await;
    assert_eq!(out["success"], true, "deploy must succeed: {out}");

    // The graph exists.
    let status = get(&app, "/v1/cognitive-io").await;
    assert_eq!(
        status["active"], true,
        "a declared observe/immune graph must be INSTANTIATED at deploy — before §112 it was \
         carried into the IR and consumed by nothing (§111 F14). Got: {status}"
    );

    // And it runs. §112.d: `window: 8` ⇒ the first eight ticks LEARN the baseline
    // (`immune.baseline: learned` is the language's default). Until it exists there
    // is nothing to be anomalous with respect to — and a reflex fired against an
    // untrained baseline is a false positive by construction.
    for _ in 0..8 {
        let t = post(&app, "/v1/cognitive-io/tick", serde_json::json!({})).await["tick"].clone();
        assert!(
            t["learning"]["Sentinel"].is_object(),
            "while estimating its baseline the immune must SAY so — silence would be \
             indistinguishable from 'watching and finding nothing wrong'. Got: {t}"
        );
    }

    let tick = post(&app, "/v1/cognitive-io/tick", serde_json::json!({})).await;
    let t = &tick["tick"];

    assert_eq!(
        t["observations"]["Health"]["certainty"], 0.93,
        "the envelope must carry what the source ACTUALLY reported — the only Handler that \
         existed before §112.a returned c: 1.0 unconditionally, without going anywhere. Got: {t}"
    );
    assert_eq!(t["observations"]["Health"]["status"], "ok");

    // The immune received what it watches, and the report is DERIVED.
    assert!(
        t["health"]["Sentinel"].is_object(),
        "with a learned baseline the immune must classify the observation it watches. Got: {t}"
    );
    assert!(
        t["health"]["Sentinel"]["kl_divergence"].is_number(),
        "the KL-divergence sensor must have actually run"
    );
}

// ── 4. The law: a refusal feeds nothing ─────────────────────────────────────

/// **The law the supervisor must not soften.** `ghost_probe` is never registered,
/// so the observation REFUSES (deny-by-default). It is reported as a refusal — and
/// the immune produces nothing, and no reflex fires.
///
/// An immune that learned a baseline from an observation nobody took would be a
/// monitor that had become a liar. And a report that only showed what it *managed*
/// to see would be the `DryRunHandler`'s `c: 1.0` in report form.
#[tokio::test]
async fn a_refused_observation_is_reported_as_a_refusal_and_feeds_nothing() {
    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());

    let src = r#"
resource Db { kind: postgres  endpoint: db.main }
manifest Infra { resources: [Db] }
observe  Blind from Infra { sources: [ghost_probe]  quorum: 1  timeout: 1s  on_partition: fail }
immune   Watch { watch: [Blind]  scope: tenant  window: 8 }
reflex   React { trigger: Watch  on_level: doubt  action: quarantine  scope: tenant  sla: 1ms }
"#;
    assert_eq!(deploy(&app, src).await["success"], true);

    let tick = post(&app, "/v1/cognitive-io/tick", serde_json::json!({})).await;
    let t = &tick["tick"];

    assert!(
        t["observations"].as_object().unwrap().is_empty(),
        "an unregistered source must yield NO observation. Got: {t}"
    );
    assert!(
        t["refusals"]["Blind"].is_string(),
        "the refusal must be reported, first-class — a system we could not see is NOT a system \
         that is fine. Got: {t}"
    );
    assert!(
        t["health"].as_object().unwrap().is_empty(),
        "the immune must NOT produce a health report from an observation nobody took — learning \
         a baseline from silence is how a monitor becomes a liar. Got: {t}"
    );
    assert_eq!(
        t["reflexes_fired"], 0,
        "no reflex may fire on a health report that was never produced"
    );
}

// ── 5-6. Absence and refusal ────────────────────────────────────────────────

/// A program with no Cognitive-I/O declarations gets no supervisor — and pays
/// nothing for it.
#[tokio::test]
async fn a_program_with_no_cognitive_io_gets_no_supervisor() {
    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());
    assert_eq!(
        deploy(&app, "flow F() -> Unit { let a = \"x\" }").await["success"],
        true
    );

    let status = get(&app, "/v1/cognitive-io").await;
    assert_eq!(status["active"], false);

    let tick = post(&app, "/v1/cognitive-io/tick", serde_json::json!({})).await;
    assert_eq!(tick["active"], false);
}

// ── §Fase 122.b — the SATISFIABLE ensemble, from a program on disk ──────────
//
// The refusal case below is a real law and stays. But it left `ensemble` with no
// program exercising the path its claim is actually about: aggregating
// observations that WERE taken. This fixture does that, from disk, through the
// same deploy path.

/// Read a fixture program from disk.
fn fixture(rel: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[tokio::test]
async fn a_deployed_fixture_aggregates_a_satisfiable_ensemble() {
    const FIXTURE: &str = "tests/fixtures/fase122_b_cognitive_io/ensemble_quorum.axon";
    register_source_adapter("ensemble_probe", Arc::new(Fixed("ensemble_probe".into(), 0.91)));
    register_source_adapter(
        "ensemble_probe_b",
        Arc::new(Fixed("ensemble_probe_b".into(), 0.88)),
    );
    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());

    let out = deploy(&app, &fixture(FIXTURE)).await;
    assert_eq!(
        out["success"], true,
        "a satisfiable ensemble must DEPLOY — the refusal law below is about \
         unsatisfiable quorums, not about ensembles. Got: {out}"
    );

    let status = get(&app, "/v1/cognitive-io").await;
    assert_eq!(
        status["active"], true,
        "the declared graph must be instantiated at deploy. Got: {status}"
    );

    let tick = post(&app, "/v1/cognitive-io/tick", serde_json::json!({})).await;
    let t = &tick["tick"];

    // The observation the ensemble aggregates was actually TAKEN, and carries what
    // the source really reported — not a default.
    assert_eq!(
        t["observations"]["EnsHealth"]["certainty"], 0.91,
        "the envelope must carry what the source ACTUALLY reported. Got: {t}"
    );
    assert_eq!(t["observations"]["EnsHealth"]["status"], "ok");
}

/// **All-or-nothing** (the §108 deploy discipline). An `ensemble` naming a quorum
/// its declared observations cannot satisfy is an invalid graph, and it must refuse
/// the DEPLOY.
///
/// A half-instantiated immune system is worse than none, because it *looks* like
/// one.
#[tokio::test]
async fn an_invalid_graph_refuses_the_deploy_rather_than_half_existing() {
    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());

    // quorum 5 over 1 declared observation — unsatisfiable by construction.
    let src = r#"
resource Db { kind: postgres  endpoint: db.main }
manifest Infra { resources: [Db] }
observe  A from Infra { sources: [p]  quorum: 1  timeout: 1s  on_partition: fail }
ensemble Impossible { observations: [A]  quorum: 5  aggregation: byzantine }
"#;
    let out = deploy(&app, src).await;

    // Either the type-checker or the supervisor must refuse — what must NOT happen
    // is a successful deploy with a graph that cannot work.
    if out["success"] == true {
        let status = get(&app, "/v1/cognitive-io").await;
        panic!(
            "an unsatisfiable ensemble quorum deployed cleanly — a graph that cannot work must \
             never half-exist. status: {status}, deploy: {out}"
        );
    }
    assert!(
        out["error"].is_string(),
        "the refusal must say why. Got: {out}"
    );
}
