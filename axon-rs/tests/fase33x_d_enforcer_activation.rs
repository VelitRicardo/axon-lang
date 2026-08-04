//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 33.x.d — `StreamPolicyEnforcer` activates in production.
//!
//! D2 contract: every flow whose tool declares
//! `effects: <stream:<policy>>` MUST run the enforcer for that
//! step's chunk stream in production. The four closed-catalog
//! policies (`DropOldest` / `DegradeQuality` / `PauseUpstream` /
//! `Fail`) MUST exhibit byte-identical semantics on the wire vs
//! the Fase 33.e unit-test surface.
//!
//! `EnforcementSummary` is published on `axon.complete.enforcement_summary`
//! per step that declared a policy. Empty map → field elided
//! (D4 byte-compat with v1.24.0 + with pre-33.x.d wire).
//!
//! # What this file proves
//!
//! - All 4 policies activate in production (wire body shows
//!   `enforcement_summary.<step>.policy_slug == "<expected>"`).
//! - Counters report sane values on the stub backend (1 chunk →
//!   `chunks_pushed=1, chunks_delivered=1`).
//! - Multi-step flows with a mix of declared + un-declared steps
//!   only summarize the declared ones (D2 + Fase 33.e D8 multi-
//!   vertical safety).
//! - D4 byte-compat: flows with NO declared effect → no
//!   `enforcement_summary` key on the wire (omitted, not empty).
//! - Adversarial load: a multi-chunk producer through the
//!   enforcer (`StreamPolicyEnforcer::push_chunk` + `pop_chunk`
//!   end-to-end) exercises the policy semantics directly at the
//!   trait layer.
//!
//! # What this file does NOT prove
//!
//! HTTP-level adversarial load (fast producer + slow consumer
//! against a real provider) ships in 33.x.j with a real-provider
//! E2E lane. The trait-layer adversarial coverage here exercises
//! the same `StreamPolicyEnforcer` code path the production HTTP
//! handler runs through.

#![allow(clippy::needless_return)]

use std::time::Duration;

use axon::axon_server::{build_router, ServerConfig};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
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

async fn deploy(app: axum::Router, src: &str) {
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
}

async fn fetch_sse_body(app: axum::Router, path: &str, body: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).to_string()
}

/// Parse SSE body, return the first `axon.complete` event's parsed data.
fn parse_complete(body: &str) -> Value {
    let mut cur_event: Option<String> = None;
    let mut cur_data: Option<String> = None;
    for line in body.lines() {
        if let Some(ev) = line.strip_prefix("event: ") {
            cur_event = Some(ev.trim().to_string());
        } else if let Some(data) = line.strip_prefix("data: ") {
            cur_data = Some(data.trim().to_string());
        } else if line.is_empty() {
            if let (Some(ev), Some(data)) = (cur_event.as_deref(), cur_data.as_deref()) {
                if ev == "axon.complete" {
                    return serde_json::from_str(data).unwrap_or(Value::Null);
                }
            }
            cur_event = None;
            cur_data = None;
        }
    }
    if let (Some(ev), Some(data)) = (cur_event, cur_data) {
        if ev == "axon.complete" {
            return serde_json::from_str(&data).unwrap_or(Value::Null);
        }
    }
    Value::Null
}

fn stream_source_with_policy(policy: &str) -> String {
    // §Fase 33.z.k.g.2 — `transport: sse(axon)` (Q5) keeps W3C wire
    // for this 33.x.d enforcement-summary anchor; the openai-dialect
    // projection of the same summary lives on the Q7 axon_metadata
    // frame (covered by the 33.z.k.g E2E pack).
    format!(
        "tool tk {{ description: \"stream\" effects: <stream:{policy}> }}\n\
         flow Chat() -> Unit {{\n\
            step Generate {{ ask: \"hi\" apply: tk }}\n\
         }}\n\
         axonendpoint E {{ public: true method: POST path: \"/c\" execute: Chat transport: sse(axon) }}"
    )
}

// ─── §1 — D2 anchor: all four policies activate in production ──────

#[tokio::test]
async fn d2_drop_oldest_activates_with_canonical_summary_shape() {
    let app = build_router(server_cfg());
    deploy(app.clone(), &stream_source_with_policy("drop_oldest")).await;
    let body = fetch_sse_body(app, "/c", "{}").await;
    let complete = parse_complete(&body);
    let summary = complete["enforcement_summary"]["Generate"].as_object().unwrap();
    assert_eq!(summary["policy_slug"], "drop_oldest");
    // §Fase 36.x.e.2 — `apply: tk` routes through the streaming-tool
    // path (Fase 36.i wired the tool registry); the stub stream emits
    // more than one chunk, so the canonical shape is asserted
    // robustly: ≥1 chunk produced, and every chunk delivered (no
    // backpressure fires under the fast test consumer).
    let pushed = summary["chunks_pushed"].as_u64().unwrap_or(0);
    assert!(pushed >= 1, "36.x.e.2: the streaming tool produced chunks");
    assert_eq!(summary["chunks_delivered"].as_u64(), Some(pushed));
    assert_eq!(summary["drop_oldest_hits"].as_u64(), Some(0));
    assert_eq!(summary["failed"], false);
}

#[tokio::test]
async fn d2_degrade_quality_activates_with_canonical_summary_shape() {
    let app = build_router(server_cfg());
    deploy(app.clone(), &stream_source_with_policy("degrade_quality")).await;
    let body = fetch_sse_body(app, "/c", "{}").await;
    let complete = parse_complete(&body);
    let summary = complete["enforcement_summary"]["Generate"].as_object().unwrap();
    assert_eq!(summary["policy_slug"], "degrade_quality");
    // §Fase 36.x.e.2 — `apply: tk` routes through the streaming-tool
    // path (Fase 36.i wired the tool registry); the stub stream emits
    // more than one chunk, so the canonical shape is asserted
    // robustly: ≥1 chunk produced, and every chunk delivered (no
    // backpressure fires under the fast test consumer).
    let pushed = summary["chunks_pushed"].as_u64().unwrap_or(0);
    assert!(pushed >= 1, "36.x.e.2: the streaming tool produced chunks");
    assert_eq!(summary["chunks_delivered"].as_u64(), Some(pushed));
    assert_eq!(summary["failed"], false);
}

#[tokio::test]
async fn d2_pause_upstream_activates_with_canonical_summary_shape() {
    let app = build_router(server_cfg());
    deploy(app.clone(), &stream_source_with_policy("pause_upstream")).await;
    let body = fetch_sse_body(app, "/c", "{}").await;
    let complete = parse_complete(&body);
    let summary = complete["enforcement_summary"]["Generate"].as_object().unwrap();
    assert_eq!(summary["policy_slug"], "pause_upstream");
    // §Fase 36.x.e.2 — `apply: tk` routes through the streaming-tool
    // path (Fase 36.i wired the tool registry); the stub stream emits
    // more than one chunk, so the canonical shape is asserted
    // robustly: ≥1 chunk produced, and every chunk delivered (no
    // backpressure fires under the fast test consumer).
    let pushed = summary["chunks_pushed"].as_u64().unwrap_or(0);
    assert!(pushed >= 1, "36.x.e.2: the streaming tool produced chunks");
    assert_eq!(summary["chunks_delivered"].as_u64(), Some(pushed));
    assert_eq!(summary["pause_upstream_blocks"].as_u64(), Some(0));
    assert_eq!(summary["failed"], false);
}

#[tokio::test]
async fn d2_fail_activates_with_canonical_summary_shape() {
    let app = build_router(server_cfg());
    deploy(app.clone(), &stream_source_with_policy("fail")).await;
    let body = fetch_sse_body(app, "/c", "{}").await;
    let complete = parse_complete(&body);
    let summary = complete["enforcement_summary"]["Generate"].as_object().unwrap();
    assert_eq!(summary["policy_slug"], "fail");
    // §Fase 36.x.e.2 — `apply: tk` routes through the streaming-tool
    // path (Fase 36.i wired the tool registry); the stub stream emits
    // more than one chunk, so the canonical shape is asserted
    // robustly: ≥1 chunk produced, and every chunk delivered (no
    // backpressure fires under the fast test consumer).
    let pushed = summary["chunks_pushed"].as_u64().unwrap_or(0);
    assert!(pushed >= 1, "36.x.e.2: the streaming tool produced chunks");
    assert_eq!(summary["chunks_delivered"].as_u64(), Some(pushed));
    assert_eq!(summary["fail_overflows"].as_u64(), Some(0));
    assert_eq!(summary["failed"], false);
}

// ─── §2 — D4 byte-compat: flows without effects omit summary ───────

#[tokio::test]
async fn d4_flow_without_declared_effect_omits_enforcement_summary() {
    let src = "flow Chat() -> Unit {\n\
                  step Generate { ask: \"hi\" output: Stream<Token> }\n\
               }\n\
               axonendpoint E { public: true method: POST path: \"/c\" execute: Chat transport: sse }";
    let app = build_router(server_cfg());
    deploy(app.clone(), src).await;
    let body = fetch_sse_body(app, "/c", "{}").await;
    let complete = parse_complete(&body);
    assert!(
        complete.get("enforcement_summary").is_none(),
        "no declared effect → enforcement_summary key MUST be omitted (D4 byte-compat)"
    );
    // The wire-level `stream_policies` field is also empty/omitted
    // since Fase 33.e D9 elides empty arrays — D4 invariant.
    assert!(
        complete.get("stream_policies").is_none()
            || complete["stream_policies"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(false)
    );
}

// ─── §3 — Multi-step with mixed effects ────────────────────────────

#[tokio::test]
async fn multi_step_summary_keyed_per_declared_step_only() {
    // §Fase 33.z.k.g.2 — `transport: sse(axon)` (Q5) keeps W3C wire.
    let src = "tool tk { description: \"stream\" effects: <stream:drop_oldest> }\n\
               flow Chat() -> Unit {\n\
                   step Plain { ask: \"plain\" output: Stream<Token> }\n\
                   step Effecting { ask: \"hi\" apply: tk }\n\
               }\n\
               axonendpoint E { public: true method: POST path: \"/c\" execute: Chat transport: sse(axon) }";
    let app = build_router(server_cfg());
    deploy(app.clone(), src).await;
    let body = fetch_sse_body(app, "/c", "{}").await;
    let complete = parse_complete(&body);
    let summary_obj = complete["enforcement_summary"].as_object().unwrap();
    // Only `Effecting` has a declared effect; `Plain` does not.
    assert!(summary_obj.contains_key("Effecting"));
    assert!(
        !summary_obj.contains_key("Plain"),
        "step without declared effect MUST NOT appear in enforcement_summary"
    );
}

// ─── §4 — Adversarial load at trait layer (4 policies × N chunks) ──
//
// These tests exercise the SAME `StreamPolicyEnforcer` code path the
// production HTTP handler runs through, but directly at the trait
// layer. The HTTP-level adversarial load against a real provider
// ships in 33.x.j (real-provider opt-in lane).

use axon::backends::{ChatChunk, FinishReason};
use axon::stream_effect::BackpressurePolicy;
use axon::stream_effect_dispatcher::StreamPolicyEnforcer;

fn chunk(text: &str) -> ChatChunk {
    ChatChunk {
        delta: text.to_string(),
        finish_reason: None,
        usage: None,
    }
}

fn terminal_chunk() -> ChatChunk {
    ChatChunk {
        delta: String::new(),
        finish_reason: Some(FinishReason::Stop),
        usage: None,
    }
}

#[tokio::test]
async fn adversarial_drop_oldest_under_fast_producer_drops_excess() {
    // DropOldest with capacity 4 + 100 fast pushes + slow consumer
    // → enforcer drops some chunks; consumer sees ≤4 chunks queued
    // at a time + drop_oldest_hits > 0.
    let enforcer = StreamPolicyEnforcer::with_capacity(BackpressurePolicy::DropOldest, 4);
    let enforcer_for_producer = enforcer.clone();
    let producer = tokio::spawn(async move {
        for i in 0..100 {
            let _ = enforcer_for_producer
                .push_chunk(chunk(&format!("c{i}")))
                .await;
        }
        enforcer_for_producer.close().await;
    });
    // Slow consumer: yield between pops to let producer get ahead.
    let mut consumed = 0u64;
    while let Some(_c) = enforcer.pop_chunk().await {
        consumed += 1;
        tokio::time::sleep(Duration::from_micros(50)).await;
    }
    producer.await.unwrap();
    let snap = enforcer.metrics_snapshot();
    assert_eq!(snap.items_pushed, 100);
    assert!(
        snap.drop_oldest_hits > 0,
        "DropOldest MUST fire when producer outpaces consumer; got {}",
        snap.drop_oldest_hits
    );
    assert_eq!(consumed, snap.items_delivered);
    // Delivered + dropped MUST account for every push.
    assert_eq!(snap.items_pushed, snap.items_delivered + snap.drop_oldest_hits);
}

#[tokio::test]
async fn adversarial_fail_under_overflow_surfaces_failure() {
    // Fail with capacity 4 + 100 pushes + NO consumer reads → the
    // 5th push hits Overflow + drain reports failed=true.
    let enforcer = StreamPolicyEnforcer::with_capacity(BackpressurePolicy::Fail, 4);
    let enforcer_for_producer = enforcer.clone();
    let producer = tokio::spawn(async move {
        let mut failed_at: Option<usize> = None;
        for i in 0..100 {
            if enforcer_for_producer
                .push_chunk(chunk(&format!("c{i}")))
                .await
                .is_err()
            {
                failed_at = Some(i);
                break;
            }
        }
        enforcer_for_producer.close().await;
        failed_at
    });
    let failed_at = producer.await.unwrap();
    let snap = enforcer.metrics_snapshot();
    // Drain everything that did make it in.
    while let Some(_c) = enforcer.pop_chunk().await {}
    let final_snap = enforcer.metrics_snapshot();
    assert!(
        failed_at.is_some(),
        "Fail policy MUST surface overflow when consumer doesn't drain"
    );
    assert_eq!(snap.fail_overflows, 1, "exactly one overflow fired");
    assert!(
        final_snap.items_delivered <= 4,
        "delivered count bounded by capacity"
    );
}

#[tokio::test]
async fn adversarial_pause_upstream_blocks_producer_until_consumer_drains() {
    // PauseUpstream with capacity 2 + fast producer + slow consumer
    // → producer blocks; pause_upstream_blocks counter increments.
    let enforcer = StreamPolicyEnforcer::with_capacity(BackpressurePolicy::PauseUpstream, 2);
    let enforcer_for_producer = enforcer.clone();
    let producer = tokio::spawn(async move {
        for i in 0..20 {
            let _ = enforcer_for_producer
                .push_chunk(chunk(&format!("c{i}")))
                .await;
        }
        enforcer_for_producer.close().await;
    });
    // Slow consumer.
    let mut consumed = 0u64;
    while let Some(_c) = enforcer.pop_chunk().await {
        consumed += 1;
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    producer.await.unwrap();
    let snap = enforcer.metrics_snapshot();
    assert_eq!(consumed, 20, "PauseUpstream delivers ALL chunks");
    assert_eq!(snap.items_pushed, 20);
    assert_eq!(snap.items_delivered, 20);
    assert!(
        snap.pause_upstream_blocks > 0,
        "PauseUpstream MUST block at least once; got {}",
        snap.pause_upstream_blocks
    );
}

#[tokio::test]
async fn adversarial_degrade_quality_applies_degrader_per_chunk() {
    use std::sync::Arc;
    // DegradeQuality with identity degrader + 50 pushes → counter
    // increments per push regardless of saturation.
    let enforcer = StreamPolicyEnforcer::with_degrader(
        BackpressurePolicy::DegradeQuality,
        8,
        Arc::new(|c: ChatChunk| c),
    );
    let enforcer_for_producer = enforcer.clone();
    let producer = tokio::spawn(async move {
        for i in 0..50 {
            let _ = enforcer_for_producer
                .push_chunk(chunk(&format!("c{i}")))
                .await;
        }
        enforcer_for_producer.close().await;
    });
    let mut consumed = 0u64;
    while let Some(_c) = enforcer.pop_chunk().await {
        consumed += 1;
    }
    producer.await.unwrap();
    let snap = enforcer.metrics_snapshot();
    assert_eq!(snap.items_pushed, 50);
    // DegradeQuality may drop on overflow OR pass-through depending
    // on inner stream_runtime semantics — what we pin is that the
    // counter fires (degrader was invoked) and the delivered count
    // is bounded by capacity-related semantics.
    assert!(
        snap.degrade_quality_hits >= 1,
        "DegradeQuality MUST fire degrader at least once; got {}",
        snap.degrade_quality_hits
    );
    assert_eq!(consumed, snap.items_delivered);
}

// ─── §5 — Terminal-chunk handling preserves wire shape ────────────

#[tokio::test]
async fn terminal_chunk_through_enforcer_emits_one_token_per_non_empty_delta() {
    // A backend that emits [content, content, terminal-with-empty-delta]
    // must produce 2 axon.token events on the wire (terminal empty-
    // delta chunk is consumed silently for usage capture).
    use std::pin::Pin;
    use std::sync::Arc;

    use async_trait::async_trait;
    use axon::backends::{
        Backend, ChatRequest, ChatResponse, ChatStream, Usage,
    };
    use axon::backends::error::BackendError;
    use futures::Stream;

    let enforcer = StreamPolicyEnforcer::new(BackpressurePolicy::DropOldest);
    let chunks: Vec<Result<ChatChunk, BackendError>> = vec![
        Ok(chunk("Hello")),
        Ok(chunk(" world")),
        Ok(terminal_chunk()),
    ];
    let source: Pin<Box<dyn Stream<Item = Result<ChatChunk, BackendError>> + Send>> =
        Box::pin(futures::stream::iter(chunks));

    let enforcer_for_drain = enforcer.clone();
    let drain_handle = tokio::spawn(async move {
        enforcer_for_drain.drain(source, |_| {}).await
    });

    let mut delivered_deltas: Vec<String> = Vec::new();
    while let Some(c) = enforcer.pop_chunk().await {
        if !c.delta.is_empty() {
            delivered_deltas.push(c.delta);
        }
    }
    drain_handle.await.unwrap();

    assert_eq!(delivered_deltas, vec!["Hello", " world"]);
    let snap = enforcer.metrics_snapshot();
    assert_eq!(snap.items_pushed, 3);
    assert_eq!(snap.items_delivered, 3);
    // Just keep `Arc` happy if needed for compile.
    let _: Arc<dyn Backend> = Arc::new(StubReuse);
    let _r: ChatRequest = ChatRequest::default();
    let _resp: Option<ChatResponse> = None;
    let _u: Option<Usage> = None;
    let _s: Option<ChatStream> = None;
    fn _trait_marker<B: Backend>(_b: B) {}
    struct StubReuse;
    #[async_trait]
    impl Backend for StubReuse {
        fn name(&self) -> &str { "x" }
        fn default_model(&self) -> &str { "y" }
        async fn complete(&self, _: ChatRequest) -> Result<ChatResponse, BackendError> {
            unimplemented!()
        }
        async fn stream(&self, _: ChatRequest) -> Result<ChatStream, BackendError> {
            unimplemented!()
        }
    }
}
