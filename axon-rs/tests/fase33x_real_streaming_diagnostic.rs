//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 33.x diagnostic anchor — capture the v1.24.0 production-path
//! SSE wire shape that the Fase 33.x cycle rewrites end-to-end.
//!
//! # Why a second diagnostic
//!
//! `fase33_sse_full_body_diagnostic.rs` (Fase 33.a anchor) captured the
//! v1.23.1 hollow shape (`step:""`, `token:""`, `steps_executed:0`).
//! 33.b shipped Layer 1 and the assertions in that diagnostic now
//! observe non-hollow content. 33.a-i closed the four-layer architecture
//! at the **primitive level** but the **production SSE path** still:
//!
//!   1. Routes `server_execute_streaming` through synchronous
//!      `server_execute_full` (`crate::backend::call_multi`, mono-file,
//!      NON-streaming). The `Backend::stream()` trait surface (Fase
//!      33.d) is never reached from the production handler.
//!   2. Chunks the FINAL materialized step output into 3-word groups
//!      via `split_whitespace().chunks(3)`. The granularity adopter
//!      EventSource clients see is SYNTHETIC, not the upstream
//!      provider's actual chunk granularity.
//!   3. Populates `stream_policies` on `axon.complete` for audit
//!      correlation, but the `StreamPolicyEnforcer` never runs against
//!      a real `Stream<ChatChunk>` in production. The declared
//!      `<stream:<policy>>` is compile-time documentation.
//!   4. Observes `CancellationFlag` between event emissions, but the
//!      reqwest body inside `crate::backend::call_multi` drains to
//!      completion regardless of the flag. Client disconnect today
//!      still pays the full upstream-completion latency.
//!   5. Absent `enforcement_summary` on `axon.complete` — there is
//!      nothing to summarize because the enforcer never ran.
//!   6. Absent `axon-W002 streaming-not-supported` warning surface —
//!      backends without `stream()` fall through silently.
//!   7. Absent per-step audit row for SSE routes — `replay::record_*`
//!      records only the final response.
//!
//! # What this anchor does
//!
//! Drives the production SSE handler end-to-end against the canonical
//! adopter Kivi-shape source with the `stub` backend, captures the
//! v1.24.0 wire shape verbatim, and asserts the **current** (pre-33.x)
//! invariants. As each 33.x sub-fase lands the assertions are rewritten
//! in lockstep — pre-33.x.b assertions read "1 synthetic axon.token per
//! 3-word group + no enforcement_summary"; post-33.x.b they read "1
//! axon.token per upstream chunk + enforcement_summary present per step
//! that declared a stream effect".
//!
//! When 33.x.m ships and tags v1.25.0, every assertion below reads the
//! post-33.x shape and the diagnostic is the closure proof of the cycle.
//!
//! # What this anchor does NOT do
//!
//! Real provider HTTP roundtrips (Anthropic/OpenAI/Gemini API keys live
//! in CI secrets) are covered by the opt-in `fase33x_real_provider.yml`
//! workflow (Fase 33.x.j). This diagnostic uses only the in-tree
//! `stub` backend so the test is hermetic + deterministic + fast (<2s
//! wall-clock on CI).
//!
//! # D-letter anchors
//!
//! - **D1** — `Backend::stream()` is the only production path for
//!   `output: Stream<T>` after 33.x.b. Pre-33.x.b assertions verify the
//!   synthetic-chunking degradation path; post-33.x.b assertions verify
//!   the trait-driven path runs even for stub (with `axon-W002` warning
//!   per D5).
//! - **D2** — `StreamPolicyEnforcer` activation in production. Pre-
//!   33.x.d: `stream_policies` array present, `enforcement_summary`
//!   absent. Post-33.x.d: both present, summary populated per step.
//! - **D4** — Wire-format byte-identical to v1.24.0. Assertions on the
//!   v1.24.0 fields (`step`, `trace_id`, `token`, `timestamp_ms`,
//!   `stream_policies`) MUST stay byte-identical across the 33.x
//!   cycle. New fields (`enforcement_summary`, `warnings`) ship as
//!   OPTIONAL with default-omitted shape.
//! - **D5** — `axon-W002` closed-catalog warning. Pre-33.x.g: `warnings`
//!   field absent from `FlowStart`. Post-33.x.g: `warnings: [{code:
//!   "axon-W002", ...}]` present when stream-unsupported backend is
//!   used.
//! - **D6** — Per-step audit row. Pre-33.x.f: `/v1/replay/<trace_id>`
//!   for SSE routes returns 1 row (final response). Post-33.x.f:
//!   returns N rows, one per step, with `tokens_emitted` +
//!   `output_hash` + `effect_policy_applied`.

use axon::axon_server::{build_router, ServerConfig};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn server_cfg(strict: bool) -> ServerConfig {
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
        strict_type_driven_transport: strict,
        default_backend: None,
        schemas_dir: None,
    }
}

async fn deploy(app: axum::Router, src: &str) {
    let body = serde_json::json!({
        "source": src,
        "source_file": "kivi.axon",
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

async fn fetch_sse_body(
    app: axum::Router,
    path: &str,
    request_body: &str,
) -> (StatusCode, String, String) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(request_body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let ct = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes).to_string();
    (status, ct, body)
}

/// Adopter-canonical disjunct (b) with explicit `<stream:drop_oldest>`
/// effect — the shape that exercises every D-letter. `transport: sse`
/// is declared on the endpoint so the anchor exercises the SSE path
/// deterministically without depending on strict-mode inference (which
/// is tested by `fase33_sse_full_body_diagnostic.rs`).
///   D1: Stream<T> output + transport: sse → SSE handler dispatch
///   D2: tool declares effect_policy → stream_policies wire field
///   D4: wire format from Fase 30.a-d + Fase 31 + Fase 32 + Fase 33.a-i
///   D6: replay binding active when transport=sse (Fase 32.h)
// §Fase 33.z.k.g.2 — Explicit `transport: sse(axon)` retains the W3C
// named-event wire on this algebraic-effect flow (Q5 escape valve).
// The 33.x diagnostic captures the axon-dialect wire envelope
// (stream_policies / enforcement_summary / warnings); the openai-
// dialect projection of the same fields is exercised in the
// 33.z.k.g E2E test pack via the Q7 axon_metadata frame.
const ADOPTER_STREAM_DROP_OLDEST: &str =
    "tool chat_token_stream { description: \"stream\" effects: <stream:drop_oldest> }\n\
     flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" apply: chat_token_stream }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse(axon) }";

/// Same shape but without effect declaration — D2 should NOT activate
/// enforcer; stream_policies array should be EMPTY (or omitted per D4
/// byte-compat). Explicit `transport: sse` for the same deterministic
/// dispatch reason.
const ADOPTER_STREAM_NO_EFFECT: &str =
    "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse }";

/// Parse SSE body into structured events for assertion clarity.
#[derive(Debug, Default, Clone)]
struct SseEvents {
    retry_directive_count: usize,
    tokens: Vec<serde_json::Value>,
    completes: Vec<serde_json::Value>,
    errors: Vec<serde_json::Value>,
    flow_starts: Vec<serde_json::Value>,
    other_events: Vec<(String, serde_json::Value)>,
    keepalive_lines: usize,
}

fn parse_sse_body(body: &str) -> SseEvents {
    let mut events = SseEvents::default();
    let mut current_event: Option<String> = None;
    let mut current_data: Option<String> = None;
    for line in body.lines() {
        if line.starts_with(':') {
            events.keepalive_lines += 1;
            continue;
        }
        if line.strip_prefix("retry: ").is_some() {
            events.retry_directive_count += 1;
            continue;
        }
        if let Some(ev) = line.strip_prefix("event: ") {
            current_event = Some(ev.trim().to_string());
            continue;
        }
        if let Some(data) = line.strip_prefix("data: ") {
            current_data = Some(data.trim().to_string());
            continue;
        }
        if line.is_empty() {
            if let (Some(ev), Some(data)) = (current_event.as_deref(), current_data.as_deref()) {
                let parsed: serde_json::Value =
                    serde_json::from_str(data).unwrap_or(serde_json::Value::Null);
                match ev {
                    "axon.token" => events.tokens.push(parsed),
                    "axon.complete" => events.completes.push(parsed),
                    "axon.error" => events.errors.push(parsed),
                    "axon.flow_start" => events.flow_starts.push(parsed),
                    other => events
                        .other_events
                        .push((other.to_string(), parsed)),
                }
            }
            current_event = None;
            current_data = None;
        }
    }
    // Commit trailing event with no terminator (axum's KeepAlive layer
    // may cut the final blank-line off — the parser tolerates this).
    if let (Some(ev), Some(data)) = (current_event, current_data) {
        let parsed: serde_json::Value =
            serde_json::from_str(&data).unwrap_or(serde_json::Value::Null);
        match ev.as_str() {
            "axon.token" => events.tokens.push(parsed),
            "axon.complete" => events.completes.push(parsed),
            "axon.error" => events.errors.push(parsed),
            "axon.flow_start" => events.flow_starts.push(parsed),
            _ => events.other_events.push((ev, parsed)),
        }
    }
    events
}

// ─── §1 — D1 anchor: Backend::stream() reached on production path ──
//
// **POST-33.x.b STATE** (this version): stub backend implements
// `Backend::stream()` (axon-rs/src/backends/stub.rs) and resolves
// through the streaming registry. The production async path
// `run_streaming_async_path` calls `backend.stream(req).await` per
// step and forwards each chunk's delta verbatim as one
// `StepToken` event.
//
// For stub backend specifically (one-chunk-per-call), the wire is
// BYTE-IDENTICAL with the pre-33.x.b synthetic-chunking output:
// one `axon.token` event with content `"(stub)"` per step. This is
// the D4 byte-compat invariant — D1 activates the trait surface;
// D4 keeps the wire schema unchanged.
//
// The DIFFERENCE post-33.x.b vs pre-33.x.b is observable when
// adopters wire a real backend (Anthropic / OpenAI / etc.):
// pre-33.x.b sliced the final materialized String into 3-word
// groups; post-33.x.b each upstream provider chunk becomes one
// wire event. The real-backend invariant is verified via the
// `fase33x_b_real_backend_e2e.rs` E2E tests at the trait layer
// (HTTP-level real-provider verification ships in 33.x.j gated
// on `AXON_RUN_REAL_PROVIDER_TEST`).
//
// Post-33.x.h with `axon_runtime.streaming.tokenizer_fallback = true`:
// adopter sees BPE-tokenized chunks (~1 per token) PLUS `axon-W002`.

#[tokio::test]
async fn d1_v1_24_0_synthetic_three_word_chunking_baseline() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), ADOPTER_STREAM_NO_EFFECT).await;
    let (status, ct, body) = fetch_sse_body(app, "/chat", "{}").await;

    eprintln!("─── D1 anchor — v1.24.0 SSE production path ───");
    eprintln!("status: {status}");
    eprintln!("content-type: {ct}");
    eprintln!("body bytes: {}", body.len());
    eprintln!("--- body BEGIN ---\n{body}\n--- body END ---");

    let events = parse_sse_body(&body);
    eprintln!("parsed: {events:#?}");

    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));
    assert!(events.retry_directive_count >= 1);

    // POST-33.x.b invariant: stub backend's `Backend::stream()`
    // emits exactly 1 chunk per step → exactly 1 axon.token event
    // per step. For this single-step flow → 1 axon.token (D4 byte-
    // compat with pre-33.x.b synthetic chunking of `"(stub)"`).
    //
    // For real backends post-33.x.b the count would be N (one per
    // provider chunk); that invariant is asserted by
    // `fase33x_b_real_backend_e2e.rs::backend_stream_emits_one_chunk_per_upstream_event`.
    assert_eq!(
        events.tokens.len(),
        1,
        "post-33.x.b: stub.stream() emits 1 chunk → 1 axon.token (D1 \
         activates the trait surface; D4 keeps the schema unchanged)"
    );
    assert_eq!(
        events.tokens[0]["token"], "(stub)",
        "D4 byte-compat: stub chunk delta is canonical (stub) marker"
    );
    assert_eq!(
        events.completes.len(),
        1,
        "Exactly one axon.complete terminates the stream — wire \
         format invariant unchanged across the 33.x cycle (D4)."
    );
    assert!(events.errors.is_empty(), "No error events expected on the happy path");

    // PRE-33.x.b token shape: synthetic group from final materialized
    // String. POST-33.x.b: chunk.delta from `Backend::stream()` —
    // monotonically finer granularity, but the field schema is
    // identical (`step`, `trace_id`, `token`, `timestamp_ms`,
    // `token_index`).
    for tok in &events.tokens {
        assert!(tok.get("step").is_some(), "axon.token MUST carry step field (D4)");
        assert!(tok.get("trace_id").is_some(), "axon.token MUST carry trace_id (Fase 33.c D6 anchor)");
        assert!(tok.get("token").is_some(), "axon.token MUST carry token field (D4)");
        assert!(
            tok.get("timestamp_ms").is_some(),
            "axon.token MUST carry timestamp_ms — adopter dashboards bind to this"
        );
    }
}

// ─── §2 — D2 anchor: stream_policies wire field for declared effect ─
//
// Pre-33.x.d: `axon.complete.stream_policies` array is populated
// when the flow's tool declares `<stream:<policy>>` (Fase 33.e wired
// the field). The enforcer NEVER runs in production — only the
// reflection-into-wire happens.
//
// Post-33.x.d: same `stream_policies` array (byte-identical per D4)
// PLUS new optional `enforcement_summary` object reporting pushed/
// delivered/dropped/degraded counts per step that had a policy.

#[tokio::test]
async fn d2_post_33_x_d_stream_policies_and_enforcement_summary_both_present() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), ADOPTER_STREAM_DROP_OLDEST).await;
    let (status, _ct, body) = fetch_sse_body(app, "/chat", "{}").await;
    assert_eq!(status, StatusCode::OK);

    let events = parse_sse_body(&body);
    eprintln!("D2 anchor parsed: {events:#?}");
    let complete = events
        .completes
        .first()
        .expect("axon.complete must be present per D4");

    // PRE-33.x.d + POST-33.x.d: stream_policies stays populated by
    // Fase 33.e for declared effects (D4 byte-compat).
    let policies = complete
        .get("stream_policies")
        .expect("stream_policies array MUST be present (Fase 33.e wired this field — D4 byte-compat)");
    let policies_arr = policies.as_array().expect("stream_policies is an array");
    assert_eq!(policies_arr.len(), 1);
    assert_eq!(policies_arr[0]["step"], "Generate");
    assert_eq!(policies_arr[0]["policy"], "drop_oldest");

    // POST-33.x.d: enforcement_summary is now PRESENT — the enforcer
    // runs on the production async path. Shape: `{step_name -> {
    // policy_slug, chunks_pushed, chunks_delivered, drop_oldest_hits,
    // degrade_quality_hits, pause_upstream_blocks, fail_overflows,
    // failed }}`.
    let summary_obj = complete
        .get("enforcement_summary")
        .expect("POST-33.x.d: enforcement_summary MUST be present when a step declares <stream:<policy>>")
        .as_object()
        .expect("enforcement_summary is an object");
    let generate_summary = summary_obj
        .get("Generate")
        .expect("summary keyed by step_name 'Generate'");
    assert_eq!(generate_summary["policy_slug"], "drop_oldest");
    // For the stub stream-producer tool, the enforcer pushed exactly
    // the chunks it received from the source and the consumer drained
    // all of them — no policy fired.
    assert_eq!(generate_summary["chunks_pushed"].as_u64(), Some(4));
    assert_eq!(generate_summary["chunks_delivered"].as_u64(), Some(4));
    assert_eq!(generate_summary["drop_oldest_hits"].as_u64(), Some(0));
    assert_eq!(generate_summary["degrade_quality_hits"].as_u64(), Some(0));
    assert_eq!(generate_summary["pause_upstream_blocks"].as_u64(), Some(0));
    assert_eq!(generate_summary["fail_overflows"].as_u64(), Some(0));
    assert_eq!(generate_summary["failed"], false);
}

// ─── §3 — D2 anchor: no effect → no stream_policies (D4 byte-compat) ─

#[tokio::test]
async fn d2_no_declared_effect_omits_stream_policies() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), ADOPTER_STREAM_NO_EFFECT).await;
    let (status, _ct, body) = fetch_sse_body(app, "/chat", "{}").await;
    assert_eq!(status, StatusCode::OK);

    let events = parse_sse_body(&body);
    let complete = events.completes.first().expect("axon.complete required");

    // No declared effect → stream_policies elided (per Fase 33.e D9
    // byte-compat: empty array is omitted, not serialized as `[]`).
    // This invariant MUST hold across the 33.x cycle (D4).
    assert!(
        complete.get("stream_policies").is_none()
            || complete["stream_policies"].as_array().map(|a| a.is_empty()).unwrap_or(false),
        "axon.complete without declared effects MUST elide \
         stream_policies (D4 byte-compat with Fase 30/31/32 adopters)"
    );
}

// ─── §4 — D5 anchor: axon-W002 surface ────────────────────────────
//
// POST-33.x.g: closed-catalog warning code `axon-W002
// streaming-not-supported` surfaces on `axon.complete.warnings[*]`
// when the streaming async path falls back to legacy synchronous
// delivery. The wire field is OPTIONAL — elided on the happy
// async-streaming path (D4 byte-compat preserved with v1.24.0 +
// pre-33.x.g adopter parsers).
//
// The 33.x.g implementation chose `axon.complete.warnings` (not a
// separate `axon.flow_start` event or a per-token warning field)
// because:
//   - It keeps the wire event catalog closed (no new event type)
//   - Adopters already parse `axon.complete` for `success` /
//     `stream_policies` / `enforcement_summary` — adding a
//     `warnings` field on the same event reuses the existing
//     parsing infrastructure.

#[tokio::test]
async fn d5_post_33_x_g_happy_path_elides_warnings_field() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), ADOPTER_STREAM_NO_EFFECT).await;
    let (_status, _ct, body) = fetch_sse_body(app, "/chat", "{}").await;

    let events = parse_sse_body(&body);
    let complete = events
        .completes
        .first()
        .expect("axon.complete must be present");
    // The async streaming path activated (stub backend, simple
    // step) → no W002 warning → wire field elided.
    assert!(
        complete.get("warnings").is_none(),
        "POST-33.x.g: happy async-streaming path MUST elide the \
         `warnings` field (D4 byte-compat preserved). Got: {complete:?}"
    );
    // axon-W002 string MUST be absent from the wire body when the
    // happy path activates.
    assert!(
        !body.contains("axon-W002"),
        "axon-W002 slug must NOT appear on the happy async path"
    );
}

// ─── §5 — D6 anchor: per-step replay binding ────────────────────────
//
// Pre-33.x.f: `/v1/replay/<trace_id>` for SSE routes returns ONE row
// (final response, Fase 32.h binding).
//
// Post-33.x.f: returns N rows for an N-step flow, each with
// step_name + tokens_emitted + output_hash + effect_policy_applied +
// enforcement_summary slice (matches §2's `enforcement_summary`).

/// §Fase 33.x.f — Replay-bound SSE source. Adds `replay: true` to
/// the canonical disjunct (b) shape so the dynamic-route dispatcher
/// records an `AxonendpointReplayEntry` with per-step audit records.
const ADOPTER_STREAM_DROP_OLDEST_WITH_REPLAY: &str =
    "tool chat_token_stream { description: \"stream\" effects: <stream:drop_oldest> }\n\
     flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" apply: chat_token_stream }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse(axon) replay: true }";

#[tokio::test]
async fn d6_post_33_x_f_replay_returns_step_audit_for_sse_flow() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), ADOPTER_STREAM_DROP_OLDEST_WITH_REPLAY).await;

    // POST the dynamic route + capture the X-Axon-Trace-Id header.
    // 33.x.f lifts the UUID generation BEFORE dispatch so the
    // header is set even on SSE responses (where the body capture
    // path doesn't fire).
    let req = Request::builder()
        .method("POST")
        .uri("/chat")
        .header("content-type", "application/json")
        .body(Body::from("{}".to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let trace_uuid = resp
        .headers()
        .get("x-axon-trace-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .expect("X-Axon-Trace-Id header MUST be present on dynamic-route SSE responses (33.x.f)");
    // Drain the SSE body so the producer's FlowComplete fires +
    // the replay entry gets written.
    let _ = resp.into_body().collect().await.unwrap().to_bytes();

    // GET /v1/replay/<uuid> — post-33.x.f shape: payload's
    // `step_audit` field is a non-empty array with per-step records.
    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/replay/{trace_uuid}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "POST-33.x.f: SSE route with `replay: true` MUST record a replay entry"
    );
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed: serde_json::Value =
        serde_json::from_slice(&bytes).expect("replay GET returns JSON");

    let step_audit = parsed["step_audit"]
        .as_array()
        .expect("POST-33.x.f: step_audit MUST be a JSON array");
    // 1-step flow → exactly 1 audit record. **D6 anchor flips
    // from `≤1 (often 0)` pre-33.x.f to `== step_count` post-
    // 33.x.f**: the cycle's closure-proof diff is now observable.
    assert_eq!(step_audit.len(), 1, "1-step flow → 1 step_audit entry");
    let entry = &step_audit[0];
    assert_eq!(entry["step_name"], "Generate");
    assert_eq!(entry["step_index"].as_u64(), Some(0));
    assert_eq!(entry["tokens_emitted"].as_u64(), Some(3));
    // The declared `<stream:drop_oldest>` effect surfaces in the
    // audit record's `effect_policy_applied` field.
    assert_eq!(entry["effect_policy_applied"], "drop_oldest");
    // For stub backend, no chunks dropped/degraded.
    assert_eq!(entry["chunks_dropped"].as_u64(), Some(0));
    assert_eq!(entry["chunks_degraded"].as_u64(), Some(0));
}

// ─── §6 — D3 anchor: cancel observed between emissions, not in-body ──
//
// Pre-33.x.e: cancel flag is checked between `emit()` calls in
// `server_execute_streaming`. The reqwest body inside
// `crate::backend::call_multi` keeps draining. For the stub backend
// there is no reqwest body to abort (stub is in-process) so this
// anchor is structurally inactive on stub — the integration test for
// real cancel-in-body landing in 33.x.e lives in
// `fase33x_cancel_inside_reqwest_body.rs` (lands with 33.x.e + uses a
// local axum slow-drip backend mock).
//
// This stub-driven anchor verifies the wire shape is well-formed end-
// to-end (precondition for §1/§2/§4/§5 to hold).

#[tokio::test]
async fn d3_v1_24_0_wire_shape_well_formed_on_happy_path() {
    let app = build_router(server_cfg(false));
    deploy(app.clone(), ADOPTER_STREAM_DROP_OLDEST).await;
    let (status, ct, body) = fetch_sse_body(app, "/chat", "{}").await;

    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));

    let events = parse_sse_body(&body);

    // Catalog closure: every event must be in {token, complete,
    // error, flow_start_when_33xg_lands}. No drift.
    assert!(
        events.other_events.is_empty(),
        "Unexpected SSE event types: {:?}. Wire catalog is closed.",
        events.other_events
    );

    // Catalog closure: at least one of {token, complete} per wire.
    assert!(
        !events.tokens.is_empty() || !events.completes.is_empty(),
        "Wire MUST emit content events per Fase 33.c live forwarding"
    );

    // Pre-33.x.f: trace_id is monotonic + present on every event that
    // carries one. This invariant is preserved across the cycle (D4).
    let trace_ids: Vec<&str> = events
        .tokens
        .iter()
        .chain(events.completes.iter())
        .filter_map(|e| e.get("trace_id").and_then(|v| v.as_str()))
        .collect();
    if let Some(first_trace_id) = trace_ids.first() {
        for tid in &trace_ids {
            assert_eq!(
                tid, first_trace_id,
                "Every event in a flow MUST carry the same trace_id \
                 (Fase 33.c D6 anchor — preserved across 33.x cycle)"
            );
        }
    }
}

// ─── §7 — Cycle closure summary (commentary, no assertions) ─────────
//
// **POST-33.x.b STATE** (this version): §1 D1 anchor has been
// rewritten — the assertion now reads "stub.stream() emits 1
// chunk → 1 axon.token" instead of "≥ 1 token from synthetic
// chunking". The real-backend N-chunks-in-N-tokens-out invariant
// is asserted by `fase33x_b_real_backend_e2e.rs` against the
// trait surface; HTTP-level real-provider verification ships in
// 33.x.j.
//
// **REMAINING REWRITES** as each subsequent sub-fase lands:
//
//   §2 (D2) — `complete.get("enforcement_summary").is_some()` (vs
//             `.is_none()` today) — flips in 33.x.d
//   §3 (D2) — unchanged (D4 byte-compat invariant)
//   §4 (D5) — `body.contains("axon-W002")` (vs `!body.contains(...)`
//             today) when adopter custom backend has no stream() —
//             flips in 33.x.g
//   §5 (D6) — `entries.len() == step_count` (vs `<= 1` today) for SSE
//             flows with `replay: true` — flips in 33.x.f
//   §6 (D3) — unchanged structurally; the real cancel-in-body anchor
//             lives in `fase33x_cancel_inside_reqwest_body.rs` (lands
//             33.x.e) with a local axum slow-drip mock
//
// Each rewrite is part of the corresponding sub-fase's commit. The
// diff between v1.24.0 anchor and v1.25.0 anchor is the cycle's
// closure proof.
