//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 112.d — the immune system **actually fires**.
//!
//! §112.c proved a health report gets *produced*. That is not the same as proving
//! the immune system *works*: a monitor that reports is not a monitor that
//! **detects**.
//!
//! This proves the whole reflex arc, end to end, through the real deploy path:
//!
//! ```text
//!   observe ──► immune (KL-divergence vs a LEARNED baseline) ──┬──► reflex (HMAC-signed)
//!                                                              └──► heal   (patch FSM)
//! ```
//!
//! # The defect this closes, and it is a subtle one
//!
//! `immune.baseline: learned` is the language's default, and `window:` is — in the
//! AST's own words — *"samples used to estimate baseline"*. **The baseline is
//! LEARNED.** Until it is, there is nothing to deviate *from*.
//!
//! A detector classifying against an **empty** baseline sees every symbol as novel,
//! reports a high KL divergence, and fires every reflex — **on a perfectly healthy
//! system, on the very first tick.**
//!
//! > **A monitor that cries wolf from the first tick is as useless as one that never
//! > cries.** Both tell you nothing about your infrastructure.
//!
//! So the supervisor runs two phases, and **no reflex may fire during learning** —
//! a reflex fired against an untrained baseline is a false positive *by
//! construction*.
//!
//! Pins:
//! 1. **Learning is visible.** "I am still learning" ≠ "I am watching and all is
//!    well", and an operator must be able to tell them apart.
//! 2. **No reflex fires while learning.** There is no anomaly before there is a
//!    baseline.
//! 3. **A steady system stays quiet** once the baseline is learned. No false
//!    positives.
//! 4. **A real deviation FIRES the reflex** — the declared `action:`, with an
//!    HMAC-signed trace, within its `sla:`.
//! 5. **`heal` decides** under its declared `mode:`.
//! 6. **Idempotency**: the same anomaly signature does not re-fire.

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

/// A source whose reported certainty can be **changed at will** — the seam that
/// lets a test create a genuine deviation from a learned baseline, rather than
/// asserting against a hand-built health report.
struct Shifty {
    name: String,
    /// certainty × 1000, so it can live in an atomic.
    milli: Arc<AtomicU64>,
}
impl SourceAdapter for Shifty {
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

/// `window: 4` keeps the learning phase short enough to drive in a test.
/// `sensitivity: 0.8` amplifies the KL so a genuine shift crosses into `doubt`.
fn program(source: &str) -> String {
    format!(
        r#"
resource Db {{ kind: postgres  endpoint: db.main }}
manifest Infra {{ resources: [Db] }}
observe  Health from Infra {{ sources: [{source}]  quorum: 1  timeout: 1s  on_partition: fail }}
immune   Sentinel {{ watch: [Health]  scope: tenant  window: 4  sensitivity: 0.8  tau: 5m }}
reflex   Quarantine {{ trigger: Sentinel  on_level: doubt  action: quarantine  scope: tenant  sla: 1ms }}
heal     Repair {{ source: Sentinel  on_level: doubt  mode: audit_only  scope: tenant }}
"#
    )
}

// ── 1-3. Learning, then quiet ───────────────────────────────────────────────

/// **The subtle defect, pinned.** While the baseline is being learned, the immune
/// emits **no health report** — and therefore **no reflex fires**. A reflex fired
/// against an untrained baseline is a false positive *by construction*: everything
/// looks novel when you have seen nothing.
///
/// And the learning phase is **visible**: "I am still learning" and "I am watching
/// and all is well" are very different statements about your infrastructure, and an
/// operator must be able to tell them apart.
#[tokio::test]
async fn no_reflex_fires_while_the_baseline_is_still_being_learned() {
    let milli = Arc::new(AtomicU64::new(950));
    register_source_adapter(
        "learn_probe",
        Arc::new(Shifty {
            name: "learn_probe".into(),
            milli: milli.clone(),
        }),
    );
    let (app, _s) = axon::axon_server::build_router_with_state(server_cfg());
    assert_eq!(deploy(&app, &program("learn_probe")).await["success"], true);

    // window: 4 ⇒ the first ticks TRAIN.
    for i in 0..4 {
        let t = tick(&app).await;
        assert!(
            t["learning"]["Sentinel"].is_object(),
            "tick {i}: the immune must report that it is LEARNING — silence here would be \
             indistinguishable from 'watching and finding nothing wrong'. Got: {t}"
        );
        assert!(
            t["health"].as_object().unwrap().is_empty(),
            "tick {i}: no health report may be emitted before a baseline exists — there is \
             nothing to be anomalous with respect to. Got: {t}"
        );
        assert_eq!(
            t["reflexes_fired"], 0,
            "tick {i}: a reflex fired against an untrained baseline is a FALSE POSITIVE BY \
             CONSTRUCTION — everything looks novel when you have seen nothing. Got: {t}"
        );
    }
}

/// Once the baseline is learned, a **steady** system stays quiet. No false
/// positives: the monitor that cries wolf is as useless as the one that never does.
#[tokio::test]
async fn a_steady_system_stays_quiet_once_the_baseline_is_learned() {
    let milli = Arc::new(AtomicU64::new(900));
    register_source_adapter(
        "steady_probe",
        Arc::new(Shifty {
            name: "steady_probe".into(),
            milli: milli.clone(),
        }),
    );
    let (app, _s) = axon::axon_server::build_router_with_state(server_cfg());
    assert_eq!(deploy(&app, &program("steady_probe")).await["success"], true);

    for _ in 0..4 {
        tick(&app).await; // learn
    }

    // Now watching — and nothing has changed.
    let t = tick(&app).await;
    assert!(
        t["health"]["Sentinel"].is_object(),
        "the baseline is learned; the immune must now be WATCHING. Got: {t}"
    );
    assert_eq!(
        t["health"]["Sentinel"]["classification"], "know",
        "an unchanged system must classify as `know` — a healthy system that trips the immune \
         system is a monitor nobody will keep listening to. Got: {t}"
    );
    assert_eq!(t["reflexes_fired"], 0, "no anomaly ⇒ no reflex. Got: {t}");
}

// ── 4-6. The flagship: a REAL deviation fires a REAL reflex ─────────────────

/// **The whole point of the primitive.** A learned baseline, then the observed
/// world genuinely changes — and the KL-divergence sensor detects it, the reflex
/// fires its **declared action** with an **HMAC-signed trace**, and the `heal`
/// decides under its declared `mode:`.
///
/// This is the arc §111 found unreachable, §112.b built the loop for, and §112.c
/// deployed. Here it actually *works*.
#[tokio::test]
async fn a_real_deviation_fires_the_declared_reflex_with_a_signed_trace() {
    let milli = Arc::new(AtomicU64::new(980));
    register_source_adapter(
        "shift_probe",
        Arc::new(Shifty {
            name: "shift_probe".into(),
            milli: milli.clone(),
        }),
    );
    let (app, _s) = axon::axon_server::build_router_with_state(server_cfg());
    assert_eq!(deploy(&app, &program("shift_probe")).await["success"], true);

    // 1. Learn a baseline of a healthy world.
    for _ in 0..4 {
        tick(&app).await;
    }

    // 2. THE WORLD CHANGES. The source now reports something it has never reported.
    milli.store(120, Ordering::SeqCst);

    // 3. The immune must SEE it.
    let t = tick(&app).await;

    let kl = t["health"]["Sentinel"]["kl_divergence"]
        .as_f64()
        .unwrap_or_else(|| panic!("the immune must have classified. Got: {t}"));
    assert!(
        kl > 0.0,
        "the KL-divergence sensor must register a deviation from the learned baseline. Got: {t}"
    );
    assert_eq!(
        t["health"]["Sentinel"]["classification"], "doubt",
        "a total shift away from everything ever observed must reach `doubt` — the level the \
         reflex declares it triggers on. kl={kl}, got: {t}"
    );

    // 4. THE REFLEX FIRES — its declared action, attestably.
    assert_eq!(
        t["reflexes_fired"], 1,
        "the declared reflex must FIRE on the anomaly it declares it triggers on. This is the \
         arc §111 found unreachable. Got: {t}"
    );
    let r = &t["reflexes"][0];
    assert_eq!(r["fired"], true);
    assert_eq!(
        r["action"], "quarantine",
        "it must take the action the adopter DECLARED, not a default. Got: {r}"
    );
    assert!(
        r["signed_trace"].as_str().is_some_and(|s| !s.is_empty()),
        "a reflex's firing must be ATTESTABLE — an HMAC-signed trace, not merely a log line. \
         Got: {r}"
    );

    // 5. And `heal` decides, under its declared mode.
    assert!(
        !t["heal_decisions"].as_array().unwrap().is_empty(),
        "the heal bound to this immune must render a decision under its declared `mode: \
         audit_only`. Got: {t}"
    );

    // 6. Idempotency: the same anomaly signature must not re-fire.
    let again = tick(&app).await;
    assert_eq!(
        again["reflexes_fired"], 0,
        "the SAME anomaly signature must not fire the reflex twice — a motor response that \
         re-fires on every tick for one incident is a pager that nobody will answer. Got: {again}"
    );
}
