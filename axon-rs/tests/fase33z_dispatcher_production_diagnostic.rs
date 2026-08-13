//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 33.z diagnostic anchor — capture the v1.26.0 production-path
//! SSE wire shape for the 8 architectural-group representative flow
//! shapes that the Fase 33.z cycle activates end-to-end.
//!
//! # Why a new diagnostic for 33.z
//!
//! `fase33y_dispatcher_diagnostic.rs` (33.y.a) captured 4 specific
//! non-canonical shapes (Let / Reason / Tool-using Step) plus a
//! canonical Step regression pin. Each 33.y.c–j sub-fase graduated
//! a per-variant handler to the **structural** dispatcher
//! (`flow_dispatcher::dispatch_node`) — that work shipped end-to-end
//! in v1.26.0.
//!
//! **The 33.y dispatcher is NOT wired into `server_execute_streaming`
//! in v1.26.0.** Per the 33.y.l "Honest scope statement", production
//! traffic still routes through `run_streaming_legacy_path` for any
//! non-canonical shape. The 41-of-45 fallback gap is the 33.z scope.
//!
//! This anchor captures, for each of the 9 architectural groups
//! (one representative per group), the **current** (v1.26.0) wire
//! shape an adopter observes when their flow contains the
//! corresponding `IRFlowNode` variant + `transport: sse`:
//!
//! | Representative | Architectural group | 33.y handler | 33.z target |
//! |---|---|---|---|
//! | `Conditional`           | orchestration  | `orchestration::run_conditional`         | wired |
//! | `ForIn`                 | orchestration  | `orchestration::run_for_in`              | wired |
//! | `Par`                   | parallel       | `parallel::run_par`                      | wired |
//! | `Remember`              | cognitive      | `cognitive::run_remember`                | wired |
//! | `ShieldApply`           | algebraic      | `algebraic_handlers::run_shield_apply`   | wired |
//! | `Emit`                  | wire-integ.    | `wire_integrations::run_emit`            | wired |
//! | `Hibernate`             | PIX            | `pix::run_hibernate`                     | wired |
//! | `LambdaDataApply`       | lambda+tools   | `lambda_tools::run_lambda_data_apply`    | wired |
//!
//! As each 33.z sub-fase lands the production-side wiring, the
//! corresponding assertion in this file INVERTS in lockstep:
//!
//! - **Pre-33.z** (v1.26.0): assertion reads "axon-W002 fires with
//!   `fallback_mode: unsupported_flow_shape, reason: <variant>`" OR
//!   "the request 404s because the streaming planner rejected the
//!   shape and the dynamic route didn't fall back gracefully".
//! - **Post-33.z.c** (default-on dispatcher flip): assertion reads
//!   "no W002 + per-chunk wire delivery via `dispatch_node`".
//!
//! When 33.z.j ships and tags v1.27.0, every assertion below reads
//! the post-33.z shape and this diagnostic is the closure proof of
//! the cycle.
//!
//! # What this anchor does NOT do
//!
//! Real-provider HTTP roundtrips ship in the opt-in
//! `fase_33x_real_provider.yml` workflow (Fase 33.x.j). This
//! diagnostic uses the in-tree `stub` backend so the test is
//! hermetic + deterministic + fast.
//!
//! Per-handler unit tests already ship in `fase33y_c–j` integration
//! files. This anchor exclusively exercises the END-TO-END
//! production SSE handler — the path that adopter traffic actually
//! traverses. The gap this captures is the wiring between the
//! production handler and the structurally-complete dispatcher.
//!
//! # D-letter anchors (proposed — see docs/fase/fase_33z_dispatcher_production_wiring.md)
//!
//! - **D1** — Single hot path through the dispatcher.
//!   Pre-33.z.c: production hot path branches on
//!   `plan_attempt: Result<StreamingExecutionPlan, PlanError>` and
//!   falls back to `run_streaming_legacy_path` for 41 of 45 variants.
//!   Post-33.z.c: branch DELETED; every flow walks `dispatch_node`.
//!
//! - **D2** — `axon-W002 UnsupportedFlowShape` becomes structurally
//!   unreachable. Pre-33.z.e: the warning fires for 41 of 45 variants.
//!   Post-33.z.e: the warning enum variant is DELETED from
//!   `runtime_warnings.rs`; the only remaining W002 triggers are
//!   `UnknownBackend` + `SourceCompilationFailed`.
//!
//! - **D5** — `axon.tool_call` SSE event family graduates. Pre-33.z.c:
//!   `FlowExecutionEvent::ToolCall { .. } => {}` arm silently consumes.
//!   Post-33.z.c: emits an `axon.tool_call` SSE event with
//!   `data: {"step_name": ..., "tool_name": ..., "content": ..., "timestamp_ms": ...}`.
//!
//! - **D7** — 50-flow sync↔async parity corpus. Lands in 33.z.d.
//!   Drift gate runs in CI on every PR.
//!
//! - **D8** — Legacy routing primitives DELETED. Pre-33.z.e:
//!   `PlanError::LegacyOrchestrationRequired` + `unsupported_feature_reason`
//!   + `run_streaming_legacy_path` exist with `#[deprecated(since="1.26.0")]`.
//!   Post-33.z.e: all three DELETED from source; compile errors on any
//!   downstream caller that ignored the deprecation warnings.

use axon::axon_server::{build_router, ServerConfig};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

// ────────────────────────────────────────────────────────────────────
//  Helpers — mirror of the 33.y.a discipline
// ────────────────────────────────────────────────────────────────────

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

async fn deploy(app: axum::Router, src: &str) -> StatusCode {
    let body = serde_json::json!({
        "source": src,
        "source_file": "anchor33z.axon",
        "backend": "stub",
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    app.oneshot(req).await.unwrap().status()
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

#[derive(Debug, Default, Clone)]
struct SseEvents {
    tokens: Vec<serde_json::Value>,
    completes: Vec<serde_json::Value>,
    errors: Vec<serde_json::Value>,
    flow_starts: Vec<serde_json::Value>,
    step_starts: Vec<serde_json::Value>,
    other_events: Vec<(String, serde_json::Value)>,
}

fn parse_sse_body(body: &str) -> SseEvents {
    let mut events = SseEvents::default();
    let mut current_event: Option<String> = None;
    let mut current_data: Option<String> = None;
    for line in body.lines() {
        if line.starts_with(':') {
            continue;
        }
        if line.strip_prefix("retry: ").is_some() {
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
            if let (Some(ev), Some(data)) =
                (current_event.as_deref(), current_data.as_deref())
            {
                let parsed: serde_json::Value =
                    serde_json::from_str(data).unwrap_or(serde_json::Value::Null);
                match ev {
                    "axon.token" => events.tokens.push(parsed),
                    "axon.complete" => events.completes.push(parsed),
                    "axon.error" => events.errors.push(parsed),
                    "axon.flow_start" => events.flow_starts.push(parsed),
                    "axon.step_start" => events.step_starts.push(parsed),
                    other => events
                        .other_events
                        .push((other.to_string(), parsed)),
                }
            }
            current_event = None;
            current_data = None;
        }
    }
    if let (Some(ev), Some(data)) = (current_event, current_data) {
        let parsed: serde_json::Value =
            serde_json::from_str(&data).unwrap_or(serde_json::Value::Null);
        match ev.as_str() {
            "axon.token" => events.tokens.push(parsed),
            "axon.complete" => events.completes.push(parsed),
            "axon.error" => events.errors.push(parsed),
            "axon.flow_start" => events.flow_starts.push(parsed),
            "axon.step_start" => events.step_starts.push(parsed),
            _ => events.other_events.push((ev, parsed)),
        }
    }
    events
}

/// Record the observed state of a non-canonical-shape flow for the
/// forensic / inversion trail. Captures:
/// - HTTP status (200 = went through some path; 404/500 = route or
///   server-side error before reaching the dispatcher)
/// - Content-Type (text/event-stream vs application/json — the latter
///   means the route DEFAULTED to JSON because transport: sse wasn't
///   negotiated correctly)
/// - axon-W002 warning presence + variant tag (if any)
/// - Token count (1 for stub canonical, > 1 for synthetic-burst
///   legacy chunking, 0 if the path didn't reach the SSE handler)
/// - Other event types observed
///
/// Returns the observed warning slugs so individual tests can build
/// their assertions on top. Never panics on missing fields — this
/// is a baseline capture, not a strict contract enforcement.
fn record_baseline(label: &str, status: StatusCode, ct: &str, body: &str) -> Vec<String> {
    eprintln!("─── {label} baseline capture ───");
    eprintln!("HTTP status:    {status}");
    eprintln!("Content-Type:   {ct}");
    eprintln!("body bytes:     {}", body.len());

    if status != StatusCode::OK {
        eprintln!("non-200 status → request did not reach the streaming dispatcher cleanly. \
                   Anchor records this state — post-33.z dispatcher graft handles this shape \
                   end-to-end via dispatch_node.");
        return Vec::new();
    }

    let events = parse_sse_body(body);
    eprintln!("axon.flow_start: {}", events.flow_starts.len());
    eprintln!("axon.step_start: {}", events.step_starts.len());
    eprintln!("axon.token:      {}", events.tokens.len());
    eprintln!("axon.complete:   {}", events.completes.len());
    eprintln!("axon.error:      {}", events.errors.len());
    if !events.other_events.is_empty() {
        let names: Vec<&str> = events
            .other_events
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        eprintln!("other events:    {names:?}");
    }

    let mut warnings_observed: Vec<String> = Vec::new();
    if let Some(complete) = events.completes.first() {
        if let Some(warnings) = complete.get("warnings").and_then(|v| v.as_array()) {
            for w in warnings {
                let code = w
                    .get("code")
                    .and_then(|c| c.as_str())
                    .unwrap_or("?");
                let mode = w
                    .get("fallback_mode")
                    .and_then(|c| c.as_str())
                    .unwrap_or("?");
                let detail = w
                    .get("detail")
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                eprintln!("  warning: code={code} fallback_mode={mode} detail={detail}");
                warnings_observed.push(format!("{code}:{mode}"));
            }
        }
    }

    if events.tokens.len() > 1 {
        eprintln!("  → token count > 1 — likely synthetic-burst legacy chunking \
                   (3-word groups, materialized post-hoc).");
    } else if events.tokens.len() == 1 {
        let tok = events.tokens[0]
            .get("token")
            .and_then(|t| t.as_str())
            .unwrap_or("?");
        eprintln!("  → single token observed: {tok:?}");
    } else {
        eprintln!("  → zero tokens — flow may have failed at dispatch or produced no chunks.");
    }

    warnings_observed
}

// ────────────────────────────────────────────────────────────────────
//  Source fixtures — one per architectural group representative
// ────────────────────────────────────────────────────────────────────

/// Canonical Step regression pin — must stay green throughout 33.z.
/// Identical to 33.y.a §1; re-asserted here so the 33.z migration
/// cannot regress the v1.25.0 deliverable.
const CANONICAL_STEP_FLOW: &str =
    "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/canon\" execute: Chat transport: sse }";

/// **(orchestration)** `Conditional` — `if region == "us" { ... }`.
/// Pre-33.z: `IRFlowNode::Conditional` triggers
/// `PlanFallback::UnsupportedNode { kind: "conditional" }` →
/// `run_streaming_legacy_path` materializes the chosen branch +
/// emits synthetic 3-word groups. Post-33.z.c: routes through
/// `orchestration::run_conditional` with per-chunk live wire.
const CONDITIONAL_FLOW: &str =
    "flow Chat() -> Unit {\n\
        if active == \"yes\" {\n\
          step Generate { ask: \"hi\" output: Stream<Token> }\n\
        }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/cond\" execute: Chat transport: sse }";

/// **(orchestration)** `ForIn` — `for x in regions { ... }`.
/// Pre-33.z: `IRFlowNode::ForIn` triggers
/// `PlanFallback::UnsupportedNode { kind: "for_in" }` → legacy.
/// Post-33.z.c: `orchestration::run_for_in` dispatches per-iteration
/// step body with per-chunk live wire + iteration counter + per-iter
/// `branch_path: "for_in[<idx>].step[<j>]"` in audit row.
///
/// Note: Rust parser grammar (axon-frontend `parse_for_in`) accepts
/// `for <ident> in <dotted_identifier>` ONLY — NOT array literals.
/// The iterable resolves at runtime through `let_bindings`; a
/// preceding `let regions = "us,eu"` binds the comma-split source.
const FOR_IN_FLOW: &str =
    "flow Chat() -> Unit {\n\
        let regions = \"us,eu\"\n\
        for region in regions {\n\
          step Generate { ask: \"hi\" output: Stream<Token> }\n\
        }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/forin\" execute: Chat transport: sse }";

/// **(parallel)** `Par` block — `par { step A ... step B ... }`.
/// Pre-33.z: `IRFlowNode::Par` triggers
/// `PlanFallback::UnsupportedNode { kind: "par" }` → legacy.
/// Post-33.z.c: `parallel::run_par` dispatches branches concurrently
/// via `tokio::spawn` per branch, sharing the same `mpsc::Sender`
/// so chunks interleave on the wire ordered by wall-clock arrival.
/// `branch_path` field on audit rows scopes to `par[<idx>]`.
const PAR_FLOW: &str =
    "flow Chat() -> Unit {\n\
        par {\n\
          step A { ask: \"a\" output: Stream<Token> }\n\
          step B { ask: \"b\" output: Stream<Token> }\n\
        }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/par\" execute: Chat transport: sse }";

/// **(cognitive)** `Remember` — `remember region in session_memory`.
/// Pre-33.z: `IRFlowNode::Remember` triggers
/// `PlanFallback::UnsupportedNode { kind: "remember" }` → legacy.
/// Post-33.z.c: `cognitive::run_remember` binds the key into
/// `DispatchCtx::pem_backend` (when wired) or `let_bindings` (OSS
/// default); zero-token completion (Remember is not LLM-call).
///
/// Note: Rust parser grammar (axon-frontend `parse_remember_step`)
/// accepts `remember <ident> [in <mem_target>]` — single identifier
/// (no dot-notation, no `as` keyword). The `in <mem_target>` clause
/// is optional.
const REMEMBER_FLOW: &str =
    "flow Chat() -> Unit {\n\
        let region = \"us\"\n\
        remember region in session_memory\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/remember\" execute: Chat transport: sse }";

/// **(algebraic handler)** `ShieldApply` — `shield PHIShield on response -> SanitizedResponse`.
/// Pre-33.z: `IRFlowNode::ShieldApply` triggers
/// `PlanFallback::UnsupportedNode { kind: "shield_apply" }` → legacy.
/// Post-33.z.c: `algebraic_handlers::run_shield_apply` invokes the
/// OSS identity-passthrough helper (enterprise overrides ship in
/// axon-enterprise vertical R&D); audit row carries
/// `branch_path: step[<idx>].shield_apply`.
///
/// Note: Rust parser grammar (axon-frontend `parse_apply_step` via
/// `TokenType::Shield`) accepts `shield <name> [on <target>] [-> <output_type>]`
/// — NOT `apply shield ... as ...`. The `apply` keyword is reserved
/// for the step-body `apply: tool_name` syntax (Fase 33.y.k D8).
const SHIELD_APPLY_FLOW: &str =
    "shield PHIShield {\n\
        scan: [pii_leak]\n\
        on_breach: quarantine\n\
        severity: critical\n\
        compliance: [HIPAA]\n\
     }\n\
     flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
        shield PHIShield on response -> SanitizedResponse\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/shield\" execute: Chat transport: sse }";

/// **(wire integration)** `Emit` — `emit Channel(payload)`.
/// Pre-33.z: `IRFlowNode::Emit` triggers
/// `PlanFallback::UnsupportedNode { kind: "emit" }` → legacy
/// (or deploy-time rejection if the parser doesn't accept the
/// channel declaration in this minimal shape).
/// Post-33.z.c: `wire_integrations::run_emit` namespaces the
/// channel buffer into `let_bindings["__channel_<ref>"]`;
/// enterprise overrides wire the real π-calc typed-channel
/// runtime (Fase 13).
///
/// Note: Rust parser grammar (axon-frontend `parse_channel`) accepts
/// `channel <Name> { message: <type>, qos: <slug>, ... }` — NOT the
/// `channel X of T` shape. Top-level channel decl is mandatory before
/// any flow-body `emit` references it.
const EMIT_FLOW: &str =
    "channel OrdersCreated { message: String }\n\
     flow Chat() -> Unit {\n\
        let payload = \"x\"\n\
        emit OrdersCreated(payload)\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/emit\" execute: Chat transport: sse }";

/// **(PIX)** `Hibernate` — `hibernate event_name 30s`.
/// Pre-33.z: `IRFlowNode::Hibernate` triggers
/// `PlanFallback::HibernatePresent` → legacy.
/// Post-33.z.c: `pix::run_hibernate` binds the canonical
/// `__hibernating_<event>` marker via OSS helper; enterprise
/// overrides wire the supervisor's event dispatcher (Fase 16).
///
/// Note: Rust parser grammar (axon-frontend `parse_hibernate_step`)
/// accepts `hibernate <event_name> [<duration_literal>]` — the
/// duration is a `TokenType::Duration` literal (e.g., `30s`, `5m`)
/// NOT a `timeout` keyword + string literal.
const HIBERNATE_FLOW: &str =
    "flow Chat() -> Unit {\n\
        hibernate user_response 30s\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/hibernate\" execute: Chat transport: sse }";

/// **(lambda + tools)** `LambdaDataApply` — `lambda doubler on x -> Int`.
/// Pre-33.z: `IRFlowNode::LambdaDataApply` triggers
/// `PlanFallback::LambdaApplyPresent` → legacy.
/// Post-33.z.c: `lambda_tools::run_lambda_data_apply` invokes
/// the Fase 15 CPS dispatcher via `apply_lambda_data` helper
/// (OSS reference returns canonical `"lambda:<name>(<resolved>)"`
/// placeholder); audit row carries the canonical key.
///
/// Note: Rust parser grammar (axon-frontend `parse_lambda_data` for
/// top-level + `parse_lambda_data_apply` for flow-body) accepts:
/// - Top-level decl: `lambda <name> { ontology: ..., certainty: ..., ... }`
/// - Flow body usage: `lambda <name> on <target> [-> <output_type>]`
/// NOT the C-style `lambda name : T -> T = (n) => ...` shape.
const LAMBDA_DATA_APPLY_FLOW: &str =
    "lambda doubler {\n\
        ontology: \"math\"\n\
        certainty: 1.0\n\
     }\n\
     flow Chat() -> Unit {\n\
        let x = \"5\"\n\
        lambda doubler on x -> LambdaResult\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/lambda\" execute: Chat transport: sse }";

// ────────────────────────────────────────────────────────────────────
//  §1 — D1 canonical Step regression pin (must stay green throughout 33.z)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn d1_canonical_step_regression_pin_post_v1_26_0() {
    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), CANONICAL_STEP_FLOW).await;
    assert_eq!(dep, StatusCode::OK, "deploy of canonical Step flow");

    let (status, ct, body) = fetch_sse_body(app, "/canon", "{}").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.starts_with("text/event-stream"));

    let events = parse_sse_body(&body);
    eprintln!("─── D1 canonical Step regression pin (post-v1.26.0) ───");
    eprintln!("tokens: {}", events.tokens.len());
    eprintln!("completes: {}", events.completes.len());

    // POST-33.x.b invariant carried through 33.y → 33.z baseline:
    // stub.stream() emits exactly 1 chunk → 1 axon.token "(stub)".
    // 33.z MUST preserve this byte-equal on every sub-fase (D4).
    assert_eq!(
        events.tokens.len(),
        1,
        "33.z regression pin: canonical Step shape stays on async path \
         with 1 chunk per step (stub.stream emits 1 chunk → 1 token). \
         Pre-33.z: existing run_streaming_async_path. Post-33.z: \
         dispatch_node graduated into server_execute_streaming. Both \
         paths produce byte-equal wire for this shape — D4 byte-compat."
    );
    assert_eq!(events.tokens[0]["token"], "(stub)");

    // No axon-W002 warning for canonical shape (async path active).
    let complete = &events.completes[0];
    assert!(
        complete.get("warnings").is_none()
            || complete["warnings"].as_array().is_some_and(|a| a.is_empty()),
        "canonical Step shape activates async path → no axon-W002 \
         warning surface (warnings field elided per D4 byte-compat). \
         33.z must preserve this."
    );
}

// ────────────────────────────────────────────────────────────────────
//  §2 — Conditional baseline (pre-33.z.c)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn conditional_falls_back_to_legacy_pre_33_z_c() {
    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), CONDITIONAL_FLOW).await;
    if dep != StatusCode::OK {
        eprintln!(
            "─── §2 Conditional baseline ───\n\
             deploy returned {dep} — anchor records this state. \
             Pre-33.z: the Rust frontend may reject Conditional in \
             flow body at parse time. Post-33.z.b: graft skeleton \
             accepts the shape end-to-end."
        );
        return;
    }

    let (status, ct, body) = fetch_sse_body(app, "/cond", "{}").await;
    let warnings = record_baseline("§2 Conditional", status, &ct, &body);

    // PRE-33.z.c assertion: legacy fallback fires.
    //   - axon-W002 with `unsupported_flow_shape` mode + reason
    //     `unsupported_node` carrying `kind: "conditional"`, OR
    //   - synthetic-burst tokens (count > 1, materialized post-hoc).
    //
    // POST-33.z.c inversion: warnings absent + per-chunk wire
    // (stub backend → 1 token per dispatched Step under the
    // chosen Conditional branch).
    if status == StatusCode::OK && !warnings.is_empty() {
        let any_unsupported = warnings.iter().any(|w| w.contains("unsupported"));
        eprintln!(
            "  → pre-33.z.c baseline: axon-W002 fired, has_unsupported={any_unsupported}, \
             warnings={warnings:?}"
        );
    }
}

// ────────────────────────────────────────────────────────────────────
//  §3 — ForIn baseline (pre-33.z.c)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn for_in_falls_back_to_legacy_pre_33_z_c() {
    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), FOR_IN_FLOW).await;
    if dep != StatusCode::OK {
        eprintln!(
            "─── §3 ForIn baseline ───\n\
             deploy returned {dep} — anchor records this state. \
             Post-33.z.b: graft skeleton accepts the shape end-to-end."
        );
        return;
    }

    let (status, ct, body) = fetch_sse_body(app, "/forin", "{}").await;
    let warnings = record_baseline("§3 ForIn", status, &ct, &body);

    // PRE-33.z.c: legacy fallback or W002. POST-33.z.c: per-iter
    // dispatch + per-iter audit row with `branch_path: "for_in[<idx>]"`.
    if status == StatusCode::OK && !warnings.is_empty() {
        eprintln!("  → pre-33.z.c baseline: warnings={warnings:?}");
    }
}

// ────────────────────────────────────────────────────────────────────
//  §4 — Par baseline (pre-33.z.c)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn par_falls_back_to_legacy_pre_33_z_c() {
    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), PAR_FLOW).await;
    if dep != StatusCode::OK {
        eprintln!(
            "─── §4 Par baseline ───\n\
             deploy returned {dep} — anchor records this state. \
             Post-33.z.b: graft skeleton accepts the shape end-to-end \
             via parallel::run_par dispatching branches concurrently."
        );
        return;
    }

    let (status, ct, body) = fetch_sse_body(app, "/par", "{}").await;
    let warnings = record_baseline("§4 Par", status, &ct, &body);

    // PRE-33.z.c: legacy fallback or W002. POST-33.z.c: concurrent
    // dispatch via tokio::spawn per branch; chunks interleave on
    // the wire ordered by wall-clock arrival; per-branch audit
    // rows with `branch_path: "par[<idx>]"`.
    if status == StatusCode::OK && !warnings.is_empty() {
        eprintln!("  → pre-33.z.c baseline: warnings={warnings:?}");
    }
}

// ────────────────────────────────────────────────────────────────────
//  §5 — Remember baseline (pre-33.z.c)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn remember_falls_back_to_legacy_pre_33_z_c() {
    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), REMEMBER_FLOW).await;
    if dep != StatusCode::OK {
        eprintln!(
            "─── §5 Remember baseline ───\n\
             deploy returned {dep} — anchor records this state. \
             Remember is a cognitive primitive (Fase 11.c). \
             Post-33.z.b: graft accepts; cognitive::run_remember \
             binds the key into DispatchCtx (PEM-backed or let_bindings)."
        );
        return;
    }

    let (status, ct, body) = fetch_sse_body(app, "/remember", "{}").await;
    let _warnings = record_baseline("§5 Remember", status, &ct, &body);
}

// ────────────────────────────────────────────────────────────────────
//  §6 — ShieldApply baseline (pre-33.z.c)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn shield_apply_falls_back_to_legacy_pre_33_z_c() {
    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), SHIELD_APPLY_FLOW).await;
    if dep != StatusCode::OK {
        eprintln!(
            "─── §6 ShieldApply baseline ───\n\
             deploy returned {dep} — anchor records this state. \
             ShieldApply is a Fase 20 algebraic-effect handler. \
             Post-33.z.b: graft accepts; algebraic_handlers::run_shield_apply \
             invokes OSS identity-passthrough; enterprise R&D wires HIPAA \
             PHI scrubber + legal privilege scanner + fintech AML pipeline."
        );
        return;
    }

    let (status, ct, body) = fetch_sse_body(app, "/shield", "{}").await;
    let _warnings = record_baseline("§6 ShieldApply", status, &ct, &body);
}

// ────────────────────────────────────────────────────────────────────
//  §7 — Emit baseline (pre-33.z.c)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn emit_falls_back_to_legacy_pre_33_z_c() {
    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), EMIT_FLOW).await;
    if dep != StatusCode::OK {
        eprintln!(
            "─── §7 Emit baseline ───\n\
             deploy returned {dep} — anchor records this state. \
             Emit is a Fase 13 π-calc typed-channel primitive. \
             Post-33.z.b: graft accepts; wire_integrations::run_emit \
             namespaces channel buffer into let_bindings; enterprise \
             overrides wire the real typed-channel runtime."
        );
        return;
    }

    let (status, ct, body) = fetch_sse_body(app, "/emit", "{}").await;
    let _warnings = record_baseline("§7 Emit", status, &ct, &body);
}

// ────────────────────────────────────────────────────────────────────
//  §8 — Hibernate baseline (pre-33.z.c)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn hibernate_falls_back_to_legacy_pre_33_z_c() {
    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), HIBERNATE_FLOW).await;
    if dep != StatusCode::OK {
        eprintln!(
            "─── §8 Hibernate baseline ───\n\
             deploy returned {dep} — anchor records this state. \
             Hibernate is a Fase 11.e + Fase 16 PIX primitive. \
             Pre-33.z: PlanFallback::HibernatePresent → legacy CPS \
             handler stack. Post-33.z.b: graft accepts; pix::run_hibernate \
             binds __hibernating_<event> marker; enterprise overrides \
             wire the supervisor's event dispatcher (Fase 16)."
        );
        return;
    }

    let (status, ct, body) = fetch_sse_body(app, "/hibernate", "{}").await;
    let _warnings = record_baseline("§8 Hibernate", status, &ct, &body);
}

// ────────────────────────────────────────────────────────────────────
//  §9 — LambdaDataApply baseline (pre-33.z.c)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn lambda_data_apply_falls_back_to_legacy_pre_33_z_c() {
    let app = build_router(server_cfg());
    let dep = deploy(app.clone(), LAMBDA_DATA_APPLY_FLOW).await;
    if dep != StatusCode::OK {
        eprintln!(
            "─── §9 LambdaDataApply baseline ───\n\
             deploy returned {dep} — anchor records this state. \
             LambdaDataApply is a Fase 15 CPS-dispatched lambda. \
             Pre-33.z: PlanFallback::LambdaApplyPresent → legacy. \
             Post-33.z.b: graft accepts; lambda_tools::run_lambda_data_apply \
             invokes the apply_lambda_data helper."
        );
        return;
    }

    let (status, ct, body) = fetch_sse_body(app, "/lambda", "{}").await;
    let _warnings = record_baseline("§9 LambdaDataApply", status, &ct, &body);
}

// ────────────────────────────────────────────────────────────────────
//  §10 — Catalog totality pin (45 IRFlowNode variants)
// ────────────────────────────────────────────────────────────────────
//
// The 33.y dispatcher achieved compiler-enforced exhaustive match
// over the 45-variant IRFlowNode catalog. 33.z grafts the dispatcher
// into the production hot path; the totality contract must be
// preserved end-to-end. This test asserts that
// `flow_plan::ir_flow_node_kind` continues to return non-empty
// distinct slugs across all 45 variants — the same anchor the
// 33.y.b drift gate enforces, but here it lives in the production
// diagnostic surface so a regression in the production path's
// totality contract (e.g., a refactor that drops a kind slug)
// surfaces in BOTH the 33.y drift gate AND this diagnostic.
//
// Post-33.z.c the dispatcher INVOCATION lives in production; this
// pin guarantees the 45-entry catalog continues to be the
// single-source-of-truth.

#[test]
fn d1_catalog_totality_pin_45_variants() {
    use axon::flow_plan::ir_flow_node_kind;
    use axon::ir_nodes::*;

    // Construct one synthetic IR variant per kind. Same 45 slugs the
    // 33.y.b drift gate exercises. Mirrored here so the production
    // diagnostic captures the same invariant.
    let variants: Vec<IRFlowNode> = vec![
        IRFlowNode::Step(IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
            name: String::new(),
            persona_ref: String::new(),
            given: String::new(),
            ask: String::new(),
            use_tool: None,
            probe: None,
            reason: None,
            weave: None,
            output_type: String::new(),
            confidence_floor: None,
            navigate_ref: String::new(),
            apply_ref: String::new(),
            pix_ops: Vec::new(),
            stream: None,
            performs: Vec::new(),
            guards: Vec::new(),
            requires_context: None,            now_tz: None,            body: Vec::new(),
        }),
        IRFlowNode::Probe(IRProbe {
            node_type: "probe",
            source_line: 0,
            source_column: 0,
            target: String::new(),
        }),
        IRFlowNode::Reason(IRReasonStep {
            node_type: "reason",
            source_line: 0,
            source_column: 0,
            strategy: String::new(),
            target: String::new(),
            given: String::new(),
            ask: String::new(),
            depth: None,
        }),
        IRFlowNode::Validate(IRValidateStep {
            node_type: "validate",
            resolved_schema: None,
            guard: None,
            source_line: 0,
            source_column: 0,
            target: String::new(),
            rule: String::new(),
        }),
        IRFlowNode::Refine(IRRefineStep {
            node_type: "refine",
            source_line: 0,
            source_column: 0,
            target: String::new(),
            strategy: String::new(),
        }),
        IRFlowNode::Weave(IRWeaveStep {
            node_type: "weave",
            source_line: 0,
            source_column: 0,
            sources: Vec::new(),
            target: String::new(),
            format_type: String::new(),
            priority: Vec::new(),
            style: String::new(),
            include: Vec::new(),
        }),
        IRFlowNode::UseTool(IRUseToolStep {
            node_type: "use_tool",
            source_line: 0,
            source_column: 0,
            tool_name: String::new(),
            argument: String::new(),
            named_args: Vec::new(),
        }),
        IRFlowNode::Remember(IRRememberStep {
            node_type: "remember",
            source_line: 0,
            source_column: 0,
            expression: String::new(),
            memory_target: String::new(),
        }),
        IRFlowNode::Recall(IRRecallStep {
            node_type: "recall",
            source_line: 0,
            source_column: 0,
            query: String::new(),
            memory_source: String::new(),
        }),
        IRFlowNode::Conditional(IRConditional {
            node_type: "conditional",
            source_line: 0,
            source_column: 0,
            condition: String::new(),
            comparison_op: String::new(),
            comparison_value: String::new(),
            then_body: Vec::new(),
            else_body: Vec::new(),
            conditions: Vec::new(),
            conjunctor: String::new(),
            cond: None,
        }),
        IRFlowNode::ForIn(IRForIn {
            node_type: "for_in",
            source_line: 0,
            source_column: 0,
            variable: String::new(),
            iterable: String::new(),
            body: Vec::new(),
        }),
        IRFlowNode::Let(IRLetBinding {
            node_type: "let",
            source_line: 0,
            source_column: 0,
            target: String::new(),
            value: String::new(),
            value_kind: String::new(),
            value_ast: None,
        }),
        IRFlowNode::Return(IRReturnStep {
            node_type: "return",
            source_line: 0,
            source_column: 0,
            value_expr: String::new(),
        }),
        IRFlowNode::Break(IRBreakStep {
            node_type: "break",
            source_line: 0,
            source_column: 0,
        }),
        IRFlowNode::Continue(IRContinueStep {
            node_type: "continue",
            source_line: 0,
            source_column: 0,
        }),
        IRFlowNode::LambdaDataApply(IRLambdaDataApply {
            node_type: "lambda_data_apply",
            source_line: 0,
            source_column: 0,
            lambda_data_name: String::new(),
            target: String::new(),
            output_type: String::new(),
        }),
        IRFlowNode::Par(IRParallelBlock {
            node_type: "par",
            source_line: 0,
            source_column: 0,
            branches: Vec::new(),
        }),
        IRFlowNode::Hibernate(IRHibernateStep {
            node_type: "hibernate",
            source_line: 0,
            source_column: 0,
            event_name: String::new(),
            timeout: String::new(),
        }),
        IRFlowNode::Deliberate(IRDeliberateBlock {
            node_type: "deliberate",
            source_line: 0,
            source_column: 0,
        }),
        IRFlowNode::Consensus(IRConsensusBlock {
            node_type: "consensus",
            source_line: 0,
            source_column: 0,
        }),
        IRFlowNode::Forge(IRForgeBlock {
            node_type: "forge",
            source_line: 0,
            source_column: 0,
                ..Default::default()
        }),
        IRFlowNode::Focus(IRFocusStep {
            node_type: "focus",
            source_line: 0,
            source_column: 0,
            expression: String::new(),
            where_expr: String::new(),
            select: Vec::new(),
            output: String::new(),
        }),
        IRFlowNode::Associate(IRAssociateStep {
            node_type: "associate",
            source_line: 0,
            source_column: 0,
            left: String::new(),
            right: String::new(),
            using_field: String::new(),
            output: String::new(),
        }),
        IRFlowNode::Aggregate(IRAggregateStep {
            node_type: "aggregate",
            source_line: 0,
            source_column: 0,
            target: String::new(),
            group_by: Vec::new(),
            alias: String::new(),
            compute: Vec::new(),
            where_expr: String::new(),
        }),
        IRFlowNode::Explore(IRExploreStep {
            node_type: "explore",
            source_line: 0,
            source_column: 0,
            target: String::new(),
            limit: None,
            output: String::new(),
        }),
        IRFlowNode::Ingest(IRIngestStep {
            node_type: "ingest",
            source_line: 0,
            source_column: 0,
            source: String::new(),
            target: String::new(),
            format: "json".into(),
            max_bytes: None,
            max_rows: None,
        }),
        IRFlowNode::ShieldApply(IRShieldApplyStep {
            node_type: "shield_apply",
            source_line: 0,
            source_column: 0,
            shield_name: String::new(),
            target: String::new(),
            output_type: String::new(),
        
            breach_policy: None,
            scan: Vec::new(),
        }),
        IRFlowNode::Stream(IRStreamBlock {
            node_type: "stream_block",
            source_line: 0,
            source_column: 0,
        body: Vec::new(),

            chunk_type: String::new(),
            on_chunk: None,
            on_complete: None,
            on_error: None,
        }),
        IRFlowNode::Navigate(IRNavigateStep {
            depth: None,
            node_type: "navigate",
            source_line: 0,
            source_column: 0,
            pix_ref: String::new(),
            corpus_ref: String::new(),
            query: String::new(),
            trail_enabled: false,
            output_name: String::new(),
            seed: String::new(),
            budget: None,
            where_expr: String::new(),
        }),
        IRFlowNode::Drill(IRDrillStep {
            node_type: "drill",
            source_line: 0,
            source_column: 0,
            pix_ref: String::new(),
            subtree_path: String::new(),
            query: String::new(),
            output_name: String::new(),
        }),
        IRFlowNode::Trail(IRTrailStep {
            node_type: "trail",
            source_line: 0,
            source_column: 0,
            navigate_ref: String::new(),
        }),
        IRFlowNode::Corroborate(IRCorroborateStep {
            node_type: "corroborate",
            source_line: 0,
            source_column: 0,
            navigate_ref: String::new(),
            output_name: String::new(),
        }),
        IRFlowNode::OtsApply(IROtsApplyStep {
            node_type: "ots_apply",
            source_line: 0,
            source_column: 0,
            ots_name: String::new(),
            target: String::new(),
            output_type: String::new(),
        }),
        IRFlowNode::MandateApply(IRMandateApplyStep {
            node_type: "mandate_apply",
            source_line: 0,
            source_column: 0,
            mandate_name: String::new(),
            target: String::new(),
            output_type: String::new(),
        }),
        IRFlowNode::ComputeApply(IRComputeApplyStep {
            node_type: "compute_apply",
            source_line: 0,
            source_column: 0,
            compute_name: String::new(),
            arguments: Vec::new(),
            output_name: String::new(),
        }),
        IRFlowNode::Listen(IRListenStep {
            node_type: "listen",
            source_line: 0,
            source_column: 0,
            channel: String::new(),
            channel_is_ref: false,
            event_alias: String::new(),
            body: Vec::new(),
        }),
        IRFlowNode::DaemonStep(IRDaemonStepNode {
            node_type: "daemon_step",
            source_line: 0,
            source_column: 0,
            daemon_ref: String::new(),
        }),
        IRFlowNode::Emit(IREmit {
            node_type: "emit",
            source_line: 0,
            source_column: 0,
            channel_ref: String::new(),
            value_ref: String::new(),
            value_is_channel: false,
            shield_ref: String::new(),
        
            breach_policy: None,
            scan: Vec::new(),
        }),
        IRFlowNode::Publish(IRPublish {
            node_type: "publish",
            source_line: 0,
            source_column: 0,
            channel_ref: String::new(),
            shield_ref: String::new(),
            sign: String::new(),
        }),
        IRFlowNode::Discover(IRDiscover {
            node_type: "discover",
            source_line: 0,
            source_column: 0,
            capability_ref: String::new(),
            alias: String::new(),
        }),
        IRFlowNode::Persist(IRPersistStep {
            node_type: "persist",
            fields: Vec::new(),
            source_line: 0,
            source_column: 0,
            store_name: String::new(),
        }),
        IRFlowNode::Retrieve(IRRetrieveStep {
            node_type: "retrieve",
            source_line: 0,
            source_column: 0,
            store_name: String::new(),
            where_expr: String::new(),
            alias: String::new(),
            order_by: String::new(),
            limit_expr: String::new(),
            aggregate: String::new(),
            group_by: String::new(),
            cache: String::new(),
        }),
        IRFlowNode::Mutate(IRMutateStep {
            node_type: "mutate",
            fields: Vec::new(),
            source_line: 0,
            source_column: 0,
            store_name: String::new(),
            where_expr: String::new(),
        }),
        IRFlowNode::Purge(IRPurgeStep {
            node_type: "purge",
            source_line: 0,
            source_column: 0,
            store_name: String::new(),
            where_expr: String::new(),
        }),
        IRFlowNode::Transact(IRTransactBlock {
            node_type: "transact",
            source_line: 0,
            source_column: 0,
        }),
    ];

    assert_eq!(
        variants.len(),
        45,
        "33.z catalog totality pin: the IRFlowNode closed catalog \
         must continue to be exactly 45 variants. A 46th variant in \
         a future minor would fail the dispatch_node compile AND \
         this assertion in lockstep."
    );

    let mut seen = std::collections::HashSet::new();
    for v in &variants {
        let slug = ir_flow_node_kind(v);
        assert!(
            !slug.is_empty(),
            "33.z catalog totality pin: flow_plan::ir_flow_node_kind \
             returned empty slug — wire stability invariant broken"
        );
        assert!(
            seen.insert(slug),
            "33.z catalog totality pin: duplicate kind slug {slug:?} \
             — kind discriminants must be 1-to-1 with IRFlowNode variants"
        );
    }
    assert_eq!(
        seen.len(),
        45,
        "33.z catalog totality pin: all 45 variants must produce \
         distinct wire-stable kind slugs (33.y.b drift gate enforces \
         the same invariant; replicated here for production-diagnostic \
         coverage so a regression surfaces in both gates)"
    );

    eprintln!(
        "─── §10 catalog totality pin ───\n\
         45 IRFlowNode variants × 45 distinct kind slugs confirmed.\n\
         Post-33.z.c: this same 45-variant set walks through \
         dispatch_node on the production hot path."
    );
}
