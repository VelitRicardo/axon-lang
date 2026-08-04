//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 58.f.2 — Real tool dispatch on the SSE / streaming path.
//!
//! Closes the last half of brief #22: the streaming dispatcher's
//! `use_tool` handler (`flow_dispatcher::lambda_tools::run_use_tool`)
//! used to return the static placeholder `"tool:<name>(<arg>)"`
//! (`invoke_tool`). After §58.f.2 it dispatches REAL against the
//! request-scoped `ctx.tool_registry` (wired by
//! `run_streaming_via_dispatcher` since §36.i), assembling the
//! STRUCTURED JSON body from `use Tool(k = v, …)` named args with the
//! SAME type-driven coercion the synchronous server path applies
//! (§58.e), reading the typed input schema carried on the
//! `ToolEntry` (§58.f.2 piece 1).
//!
//! Sections:
//!   §1 — real dispatch via the registry (stub provider, deterministic)
//!   §2 — D5 placeholder fall-backs (no registry / unregistered /
//!        LLM-routed provider) stay byte-for-byte unchanged
//!   §3 — D10 per-request isolation (registry is ctx-local)
//!   §4 — HTTP E2E: the structured JSON body reaches the wire with
//!        type-driven coercion (piece 1 + piece 2 end-to-end)

#![allow(clippy::needless_return)]

use std::sync::Arc;
use std::time::Duration;

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::lambda_tools::run_use_tool;
use axon::flow_dispatcher::{DispatchCtx, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::*;
use axon::tool_registry::{ToolEntry, ToolRegistry, ToolSource};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, Response, StatusCode};
use axum::routing::post;
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

// ────────────────────────────────────────────────────────────────────
//  Scaffolding
// ────────────────────────────────────────────────────────────────────

/// A `ToolEntry` with the `stub` provider — `ToolRegistry::dispatch`
/// returns `[stub] <name>(<arg>)` for it WITHOUT any network, so we can
/// prove "real dispatch through the registry, not the placeholder"
/// deterministically. `parameters` carries the typed input schema.
fn stub_entry(name: &str, parameters: Vec<(String, String)>) -> ToolEntry {
    ToolEntry {
        name: name.to_string(),
        provider: "stub".to_string(),
        timeout: String::new(),
        runtime: String::new(),
        resource_ref: String::new(),
        capacity: None,
        sandbox: None,
        max_results: None,
        output_schema: String::new(),
        effect_row: Vec::new(),
        parameters,
        secret: String::new(),
        secret_partition: String::new(),
        source: ToolSource::Program,
        is_streaming: false,
        scrape: None,
    }
}

fn http_entry(name: &str, url: &str, parameters: Vec<(String, String)>) -> ToolEntry {
    ToolEntry {
        name: name.to_string(),
        provider: "http".to_string(),
        timeout: "5s".to_string(),
        runtime: url.to_string(),
        resource_ref: String::new(),
        capacity: None,
        sandbox: None,
        max_results: None,
        output_schema: String::new(),
        effect_row: vec!["network".to_string()],
        parameters,
        secret: String::new(),
        secret_partition: String::new(),
        source: ToolSource::Program,
        is_streaming: false,
        scrape: None,
    }
}

fn named(args: &[(&str, &str)]) -> Vec<IRNamedArg> {
    args.iter()
        .map(|(n, v)| IRNamedArg {
            name: n.to_string(),
            value: v.to_string(),
            // §Fase 60 — these fixtures pass literals / `${…}` interpolation.
            value_kind: "literal".to_string(),
        })
        .collect()
}

fn use_node(tool_name: &str, argument: &str, named_args: Vec<IRNamedArg>) -> IRUseToolStep {
    IRUseToolStep {
        node_type: "use_tool",
        source_line: 0,
        source_column: 0,
        tool_name: tool_name.to_string(),
        argument: argument.to_string(),
        named_args,
    }
}

/// Build a fresh ctx; `registry` is `None` to exercise the D5
/// placeholder fall-back, `Some(reg)` to exercise real dispatch.
fn ctx_with(
    registry: Option<ToolRegistry>,
) -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new("TestFlow", "stub", "", CancellationFlag::new(), tx);
    if let Some(reg) = registry {
        ctx = ctx.with_tool_registry(Arc::new(reg));
    }
    (ctx, rx)
}

fn drain(rx: &mut mpsc::UnboundedReceiver<FlowExecutionEvent>) -> Vec<FlowExecutionEvent> {
    let mut out = Vec::new();
    while let Ok(e) = rx.try_recv() {
        out.push(e);
    }
    out
}

fn step_complete_success(events: &[FlowExecutionEvent]) -> Option<bool> {
    events.iter().find_map(|e| match e {
        FlowExecutionEvent::StepComplete { success, .. } => Some(*success),
        _ => None,
    })
}

// ════════════════════════════════════════════════════════════════════
//  §1 — Real dispatch via the registry (stub provider)
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn named_args_dispatch_real_structured_body_via_registry() {
    // A registered stub tool with a typed schema. The keyword form
    // assembles a structured JSON body and dispatches REAL — the
    // output is the registry's stub response, NOT the placeholder.
    let mut reg = ToolRegistry::new();
    reg.register(stub_entry(
        "Search",
        vec![
            ("query".to_string(), "String".to_string()),
            ("max_results".to_string(), "Int".to_string()),
        ],
    ));
    let (mut ctx, mut rx) = ctx_with(Some(reg));
    let node = use_node(
        "Search",
        "",
        named(&[("query", "Acme"), ("max_results", "5")]),
    );

    let outcome = run_use_tool(&node, &mut ctx).await.unwrap();
    let output = match outcome {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("expected Completed, got {other:?}"),
    };

    // Real dispatch: stub provider echoes the dispatched body, which is
    // the STRUCTURED JSON object — not `tool:Search(...)`.
    assert!(
        output.starts_with("[stub] Search("),
        "expected real stub dispatch, got: {output}"
    );
    assert!(
        !output.starts_with("tool:"),
        "placeholder must NOT be used when the tool dispatches real: {output}"
    );
    // The dispatched argument is the structured JSON body with
    // type-driven coercion (max_results is a NUMBER, query a STRING).
    let inner = output
        .strip_prefix("[stub] Search(")
        .and_then(|s| s.strip_suffix(")"))
        .expect("stub wrapper");
    let body: serde_json::Value = serde_json::from_str(inner).expect("structured JSON body");
    assert_eq!(body["query"], serde_json::json!("Acme"));
    assert_eq!(body["max_results"], serde_json::json!(5));

    // Result bound under the canonical key for downstream steps.
    assert_eq!(ctx.let_bindings.get("Search_result").unwrap(), &output);
    // StepComplete carries the real success flag.
    assert_eq!(step_complete_success(&drain(&mut rx)), Some(true));
}

#[tokio::test]
async fn named_args_interpolate_from_let_bindings() {
    // Named-arg VALUES interpolate `${name}` against ctx.let_bindings,
    // mirroring the synchronous path's ExecContext::interpolate.
    let mut reg = ToolRegistry::new();
    reg.register(stub_entry(
        "Search",
        vec![("query".to_string(), "String".to_string())],
    ));
    let (mut ctx, _rx) = ctx_with(Some(reg));
    ctx.let_bindings
        .insert("topic".to_string(), "quantum computing".to_string());
    let node = use_node("Search", "", named(&[("query", "${topic}")]));

    let outcome = run_use_tool(&node, &mut ctx).await.unwrap();
    let output = match outcome {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("expected Completed, got {other:?}"),
    };
    assert!(output.contains("quantum computing"), "got: {output}");
}

#[tokio::test]
async fn legacy_positional_arg_dispatches_real_when_registered() {
    // The legacy `use Tool on <arg>` form (D5) ALSO reaches real
    // dispatch when the tool is registered — the interpolated single
    // argument is the dispatched body.
    let mut reg = ToolRegistry::new();
    reg.register(stub_entry("Calc", Vec::new()));
    let (mut ctx, _rx) = ctx_with(Some(reg));
    ctx.let_bindings
        .insert("expr".to_string(), "2+2".to_string());
    let node = use_node("Calc", "${expr}", Vec::new());

    let outcome = run_use_tool(&node, &mut ctx).await.unwrap();
    let output = match outcome {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("expected Completed, got {other:?}"),
    };
    assert_eq!(output, "[stub] Calc(2+2)");
}

#[tokio::test]
async fn builtin_calculator_dispatches_real_on_streaming_path() {
    // A fresh registry carries the native built-ins; Calculator
    // evaluates natively (real dispatch) on the streaming path.
    let reg = ToolRegistry::new();
    let (mut ctx, _rx) = ctx_with(Some(reg));
    let node = use_node("Calculator", "2 + 3", Vec::new());

    let outcome = run_use_tool(&node, &mut ctx).await.unwrap();
    let output = match outcome {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("expected Completed, got {other:?}"),
    };
    assert_eq!(output, "5");
}

// ════════════════════════════════════════════════════════════════════
//  §2 — D5 placeholder fall-backs (unchanged from pre-58)
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn no_registry_falls_back_to_placeholder() {
    // No registry on the ctx → the canonical placeholder, byte-for-byte
    // the pre-58 behavior.
    let (mut ctx, mut rx) = ctx_with(None);
    ctx.let_bindings
        .insert("q".to_string(), "weather".to_string());
    let node = use_node("web_search", "q", Vec::new());

    let outcome = run_use_tool(&node, &mut ctx).await.unwrap();
    let output = match outcome {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("expected Completed, got {other:?}"),
    };
    assert_eq!(output, "tool:web_search(weather)");
    assert_eq!(step_complete_success(&drain(&mut rx)), Some(true));
}

#[tokio::test]
async fn unregistered_tool_falls_back_to_placeholder() {
    // Registry present but the tool is not registered → placeholder.
    let reg = ToolRegistry::new(); // builtins only
    let (mut ctx, _rx) = ctx_with(Some(reg));
    let node = use_node("UnknownTool", "arg", Vec::new());

    let outcome = run_use_tool(&node, &mut ctx).await.unwrap();
    let output = match outcome {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("expected Completed, got {other:?}"),
    };
    assert_eq!(output, "tool:UnknownTool(arg)");
}

#[tokio::test]
async fn llm_routed_provider_falls_back_to_placeholder() {
    // §114.b — an LLM-ROUTED tool: no provider (the tool IS the model). `dispatch`
    // returns None → the caller uses the D5 placeholder. This used to be spelled
    // `provider: "brave"` — a non-empty *unknown* provider that fell through to the
    // LLM. Post-§114.b an unknown provider is refused at compile, and the honest
    // way to say "route this to the model" is to OMIT the provider, validated by
    // its absence.
    let mut reg = ToolRegistry::new();
    reg.register(ToolEntry {
        name: "WebSearch".to_string(),
        provider: String::new(),
        timeout: String::new(),
        runtime: String::new(),
        resource_ref: String::new(),
        capacity: None,
        sandbox: None,
        max_results: Some(5),
        output_schema: String::new(),
        effect_row: Vec::new(),
        parameters: Vec::new(),
        secret: String::new(),
        secret_partition: String::new(),
        source: ToolSource::Program,
        is_streaming: false,
        scrape: None,
    });
    let (mut ctx, _rx) = ctx_with(Some(reg));
    let node = use_node("WebSearch", "cats", Vec::new());

    let outcome = run_use_tool(&node, &mut ctx).await.unwrap();
    let output = match outcome {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("expected Completed, got {other:?}"),
    };
    assert_eq!(output, "tool:WebSearch(cats)");
}

#[tokio::test]
async fn cancelled_ctx_short_circuits_before_dispatch() {
    let cancel = CancellationFlag::new();
    cancel.cancel();
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut reg = ToolRegistry::new();
    reg.register(stub_entry("Search", Vec::new()));
    let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx).with_tool_registry(Arc::new(reg));
    let node = use_node("Search", "x", Vec::new());

    assert!(matches!(
        run_use_tool(&node, &mut ctx).await,
        Err(axon::flow_dispatcher::DispatchError::UpstreamCancelled)
    ));
}

// ════════════════════════════════════════════════════════════════════
//  §3 — D10 per-request isolation (registry is ctx-local)
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn registry_is_per_request_no_cross_contamination() {
    // Tenant A's ctx registers `Secret`; Tenant B's ctx does not.
    // B must NOT see A's tool — it falls back to the placeholder.
    let mut reg_a = ToolRegistry::new();
    reg_a.register(stub_entry("Secret", Vec::new()));
    let (mut ctx_a, _rx_a) = ctx_with(Some(reg_a));
    let (mut ctx_b, _rx_b) = ctx_with(Some(ToolRegistry::new())); // builtins only

    let node = use_node("Secret", "payload", Vec::new());

    let out_a = match run_use_tool(&node, &mut ctx_a).await.unwrap() {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("got {other:?}"),
    };
    let out_b = match run_use_tool(&node, &mut ctx_b).await.unwrap() {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("got {other:?}"),
    };

    assert_eq!(out_a, "[stub] Secret(payload)", "A dispatches real");
    assert_eq!(
        out_b, "tool:Secret(payload)",
        "B has no such tool → placeholder; no cross-tenant leakage"
    );
}

// ════════════════════════════════════════════════════════════════════
//  §4 — HTTP E2E: structured JSON body on the wire (piece 1 + 2)
// ════════════════════════════════════════════════════════════════════

type Captured = Arc<tokio::sync::Mutex<Option<String>>>;

async fn capture_handler(State(cap): State<Captured>, req: Request) -> Response<Body> {
    let bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    *cap.lock().await = Some(String::from_utf8_lossy(&bytes).to_string());
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from("RESULT_OK"))
        .expect("response")
}

async fn spawn_capture_server() -> (String, Captured) {
    let cap: Captured = Arc::new(tokio::sync::Mutex::new(None));
    let router = Router::new()
        .route("/", post(capture_handler))
        .with_state(cap.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    (format!("http://{addr}/"), cap)
}

#[tokio::test]
async fn http_tool_receives_structured_typed_body_end_to_end() {
    let (url, cap) = spawn_capture_server().await;

    let mut reg = ToolRegistry::new();
    reg.register(http_entry(
        "CrmRadar",
        &url,
        vec![
            ("company".to_string(), "String".to_string()),
            ("max_results".to_string(), "Int".to_string()),
            ("active".to_string(), "Bool".to_string()),
            // A String param whose value is all-digits MUST stay a
            // string (no shape-guessing — type-driven coercion only).
            ("account_id".to_string(), "String".to_string()),
        ],
    ));
    let (mut ctx, _rx) = ctx_with(Some(reg));
    let node = use_node(
        "CrmRadar",
        "",
        named(&[
            ("company", "Acme Corp"),
            ("max_results", "5"),
            ("active", "true"),
            ("account_id", "007"),
        ]),
    );

    let outcome = run_use_tool(&node, &mut ctx).await.unwrap();
    let output = match outcome {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("expected Completed, got {other:?}"),
    };

    // Real HTTP response flowed back as the bound result.
    assert_eq!(output, "RESULT_OK");
    assert_eq!(
        ctx.let_bindings.get("CrmRadar_result").unwrap(),
        "RESULT_OK"
    );

    // The endpoint received the STRUCTURED, TYPE-COERCED JSON body.
    let received = cap.lock().await.clone().expect("server captured a body");
    let body: serde_json::Value =
        serde_json::from_str(&received).expect("server received valid JSON object");
    assert_eq!(body["company"], serde_json::json!("Acme Corp"));
    assert_eq!(body["max_results"], serde_json::json!(5)); // Int → number
    assert_eq!(body["active"], serde_json::json!(true)); // Bool → bool
    assert_eq!(body["account_id"], serde_json::json!("007")); // String → "007", not 7
                                                              // It is NOT the flat `{"input": …}` legacy envelope.
    assert!(
        body.get("input").is_none(),
        "must be structured, not flat input: {received}"
    );
}

#[tokio::test]
async fn http_legacy_positional_arg_wraps_as_input_envelope() {
    // D5: a legacy single-arg dispatch to an HTTP tool still wraps as
    // the `{"input": <arg>}` envelope (http_tool's non-JSON path).
    let (url, cap) = spawn_capture_server().await;
    let mut reg = ToolRegistry::new();
    reg.register(http_entry("Echo", &url, Vec::new()));
    let (mut ctx, _rx) = ctx_with(Some(reg));
    let node = use_node("Echo", "hello world", Vec::new());

    let outcome = run_use_tool(&node, &mut ctx).await.unwrap();
    let output = match outcome {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("got {other:?}"),
    };
    assert_eq!(output, "RESULT_OK");

    let received = cap.lock().await.clone().expect("captured body");
    let body: serde_json::Value = serde_json::from_str(&received).expect("valid JSON");
    assert_eq!(body["input"], serde_json::json!("hello world"));
}

// ════════════════════════════════════════════════════════════════════
//  §Fase 60 — kwarg REFERENCE values resolve from the bindings
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn named_arg_reference_resolves_step_output_at_dispatch() {
    // §Fase 60 — a `"reference"` kwarg value (`ExtractTopic.output`) is
    // resolved against the bindings (the prior step's output, bound under its
    // step name) — NOT passed as the literal name (the pre-60 bug).
    let mut reg = ToolRegistry::new();
    reg.register(stub_entry(
        "Search",
        vec![("query".to_string(), "String".to_string())],
    ));
    let (mut ctx, _rx) = ctx_with(Some(reg));
    // A prior step `ExtractTopic` bound its output under its step name.
    ctx.let_bindings
        .insert("ExtractTopic".to_string(), "quantum computing".to_string());

    let node = use_node(
        "Search",
        "",
        vec![IRNamedArg {
            name: "query".to_string(),
            value: "ExtractTopic.output".to_string(),
            value_kind: "reference".to_string(),
        }],
    );

    let outcome = run_use_tool(&node, &mut ctx).await.unwrap();
    let output = match outcome {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("expected Completed, got {other:?}"),
    };
    let inner = output
        .strip_prefix("[stub] Search(")
        .and_then(|s| s.strip_suffix(")"))
        .expect("stub wrapper");
    let body: serde_json::Value = serde_json::from_str(inner).expect("structured JSON body");
    // The RESOLVED value reached the wire — not the literal "ExtractTopic.output".
    assert_eq!(body["query"], serde_json::json!("quantum computing"));
}

#[tokio::test]
async fn named_arg_reference_to_flow_param_resolves() {
    let mut reg = ToolRegistry::new();
    reg.register(stub_entry(
        "Search",
        vec![("query".to_string(), "String".to_string())],
    ));
    let (mut ctx, _rx) = ctx_with(Some(reg));
    ctx.let_bindings
        .insert("company".to_string(), "Acme".to_string());

    let node = use_node(
        "Search",
        "",
        vec![IRNamedArg {
            name: "query".to_string(),
            value: "company".to_string(),
            value_kind: "reference".to_string(),
        }],
    );

    let outcome = run_use_tool(&node, &mut ctx).await.unwrap();
    let output = match outcome {
        NodeOutcome::Completed { output, .. } => output,
        other => panic!("got {other:?}"),
    };
    let inner = output
        .strip_prefix("[stub] Search(")
        .and_then(|s| s.strip_suffix(")"))
        .expect("stub wrapper");
    let body: serde_json::Value = serde_json::from_str(inner).expect("JSON");
    assert_eq!(body["query"], serde_json::json!("Acme"));
}
