//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 94.d — runtime for the secret-custody lifecycle
//! (`docs/fase/fase_94_secret_custody_lifecycle.md`, axon-enterprise repo):
//! the `backend: secrets` metadata retrieve, the mediated `rotate`
//! exchange, and the `tool { secret: }` dispatch injection.
//!
//! Pinned properties (doctrine `rotation_without_revelation`):
//! 1. `retrieve` over a secrets store binds the METADATA envelope —
//!    class-scoped, §67 time-filtered — and no secret value appears in
//!    the binding, the outcome, or the wire events.
//! 2. No custody port ⇒ retrieve / rotate / secret-bearing dispatch all
//!    fail CLOSED (`MissingDependency` / witnessed refusal) — never a
//!    silent stub, never a fabricated result.
//! 3. `rotate` happy path through a REAL local HTTP tool: the tool
//!    receives the current value under `axon_rotation`, returns
//!    `axon_rotated`, custody commits version+1 with the new expiry —
//!    and neither the old nor the new value ever reaches the summary,
//!    the bindings, or the wire.
//! 4. Per-key degradation: a tool that breaks the exchange contract
//!    fails THAT key with a witness; the old value stays intact.
//! 5. Write verbs against a secrets store are refused at dispatch (the
//!    axon-T897 runtime mirror — stale/hand-edited IR defense).
//! 6. `use <Tool>` with a declared `secret:` injects the custody value
//!    under `axon_secret` into the tool-server request body.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::{dispatch_node, DispatchCtx, NodeOutcome};
use axon::ir_nodes::{
    IRAxonStore, IRFlowNode, IRMutateStep, IRNamedArg, IRRetrieveStep, IRRotateStep,
    IRToolSpec, IRUseToolStep,
};
use axon::secret_custody::{InMemoryCustody, SecretCustody};
use axon::store::registry::StoreRegistry;
use axon::tool_registry::ToolRegistry;
use std::sync::{Arc, Mutex};

const TENANT: &str = "t1";
const OLD_TOKEN: &str = "old-refresh-token-abc";
const NEW_TOKEN: &str = "new-refresh-token-xyz";

fn secrets_store_spec() -> IRAxonStore {
    IRAxonStore {
        node_type: "axonstore",
        source_line: 1,
        source_column: 1,
        name: "CrmTokens".to_string(),
        backend: "secrets".to_string(),
        connection: String::new(),
        confidence_floor: None,
        isolation: String::new(),
        on_breach: String::new(),
        capability: String::new(),
        class: "crm".to_string(),
        column_schema: None,
        resource_ref: String::new(),
    }
}

fn tool_spec(name: &str, provider: &str, runtime: &str, secret: &str) -> IRToolSpec {
    IRToolSpec {
        node_type: "tool_spec",
        source_line: 1,
        source_column: 1,
        name: name.to_string(),
        provider: provider.to_string(),
        max_results: None,
        filter_expr: String::new(),
        timeout: "10s".to_string(),
        runtime: runtime.to_string(),
        resource_ref: String::new(),
        sandbox: None,
        input_schema: Vec::new(),
        output_schema: String::new(),
        parameters: Vec::new(),
        output_type: None,
        requires: Vec::new(),
        secret: secret.to_string(),
        secret_partition: String::new(),
        effect_row: Vec::new(),
        target: None,
        risk: None,
        argv: Vec::new(),
        cache: String::new(),
        scrape: None,
    }
}

/// A seeded custody: `crm.hubspot` expiring inside 10 minutes,
/// `crm.zoho` fresh (2h out), `llm.kimi` in ANOTHER class.
fn seeded_custody() -> Arc<InMemoryCustody> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let c = InMemoryCustody::new();
    c.seed(TENANT, "crm.hubspot", OLD_TOKEN, Some(now_ms + 5 * 60_000));
    c.seed(TENANT, "crm.zoho", "zoho-token", Some(now_ms + 2 * 3_600_000));
    c.seed(TENANT, "llm.kimi", "llm-key", None);
    Arc::new(c)
}

fn ctx_with(
    custody: Option<Arc<InMemoryCustody>>,
    tools: &[IRToolSpec],
) -> (
    DispatchCtx,
    tokio::sync::mpsc::UnboundedReceiver<axon::flow_execution_event::FlowExecutionEvent>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx);
    ctx.tenant_id = TENANT.to_string();
    ctx.store_registry = Some(Arc::new(
        StoreRegistry::build(&[secrets_store_spec()]).expect("registry builds"),
    ));
    let mut registry = ToolRegistry::new();
    registry.register_from_ir(tools);
    ctx.tool_registry = Some(Arc::new(registry));
    if let Some(c) = custody {
        ctx = ctx.with_secret_custody(c);
    }
    (ctx, rx)
}

fn retrieve_node(where_expr: &str) -> IRFlowNode {
    IRFlowNode::Retrieve(IRRetrieveStep {
        node_type: "retrieve",
        source_line: 1,
        source_column: 1,
        store_name: "CrmTokens".to_string(),
        where_expr: where_expr.to_string(),
        alias: "rows".to_string(),
        order_by: String::new(),
        limit_expr: String::new(),
        aggregate: String::new(),
        group_by: String::new(),
        cache: String::new(),
    })
}

fn rotate_node(where_expr: &str, tool: &str) -> IRFlowNode {
    IRFlowNode::Rotate(IRRotateStep {
        node_type: "rotate",
        source_line: 1,
        source_column: 1,
        store_ref: "CrmTokens".to_string(),
        where_expr: where_expr.to_string(),
        tool_ref: tool.to_string(),
        binding: "result".to_string(),
    })
}

/// Spin a local axum tool-server; every request body is captured. The
/// responder decides the reply from the captured body.
async fn local_tool_server(
    reply: fn(&str) -> String,
) -> (String, Arc<Mutex<Vec<String>>>) {
    use axum::routing::post;
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_for_handler = seen.clone();
    let app = axum::Router::new().route(
        "/",
        post(move |body: String| {
            let seen = seen_for_handler.clone();
            async move {
                seen.lock().unwrap().push(body.clone());
                reply(&body)
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}/"), seen)
}

// ── 1 + 2: the metadata retrieve ────────────────────────────────────

#[tokio::test]
async fn secrets_retrieve_binds_class_scoped_metadata_only() {
    let (mut ctx, mut rx) = ctx_with(Some(seeded_custody()), &[]);
    let outcome = dispatch_node(
        &retrieve_node("expires_at < now() + interval '10 minutes'"),
        &mut ctx,
    )
    .await
    .expect("retrieve admits");

    let envelope = ctx.let_bindings.get("rows").expect("binding present").clone();
    assert!(envelope.contains("crm.hubspot"), "{envelope}");
    assert!(!envelope.contains("crm.zoho"), "fresh entry filtered: {envelope}");
    assert!(!envelope.contains("llm.kimi"), "other class invisible: {envelope}");
    assert!(envelope.contains("\"count\":1"), "{envelope}");
    // The doctrine: NO value, anywhere.
    assert!(!envelope.contains(OLD_TOKEN), "value must never bind: {envelope}");
    let NodeOutcome::Completed { output, .. } = outcome else {
        panic!("expected Completed")
    };
    assert!(!output.contains(OLD_TOKEN));
    drop(ctx);
    while let Ok(ev) = rx.try_recv() {
        let json = serde_json::to_string(&ev).unwrap();
        assert!(!json.contains(OLD_TOKEN), "value must not ride the wire: {json}");
    }
}

#[tokio::test]
async fn secrets_retrieve_without_custody_fails_closed() {
    let (mut ctx, _rx) = ctx_with(None, &[]);
    let err = dispatch_node(&retrieve_node(""), &mut ctx)
        .await
        .expect_err("no custody ⇒ fail closed");
    assert!(format!("{err:?}").contains("secret_custody"), "{err:?}");
}

// ── 5: write verbs refused at dispatch ──────────────────────────────

#[tokio::test]
async fn mutate_against_secrets_store_is_refused_at_dispatch() {
    let (mut ctx, _rx) = ctx_with(Some(seeded_custody()), &[]);
    let node = IRFlowNode::Mutate(IRMutateStep {
        node_type: "mutate",
        source_line: 1,
        source_column: 1,
        store_name: "CrmTokens".to_string(),
        where_expr: "key = 'crm.hubspot'".to_string(),
        fields: Vec::new(),
    });
    let err = dispatch_node(&node, &mut ctx)
        .await
        .expect_err("write on custody refused");
    let msg = format!("{err:?}");
    assert!(msg.contains("READ-ONLY"), "{msg}");
    assert!(msg.contains("axon-T897"), "{msg}");
}

// ── 3: the mediated rotation exchange ───────────────────────────────

#[tokio::test]
async fn rotate_happy_path_commits_cas_and_never_reveals() {
    let (url, seen) = local_tool_server(|_body| {
        serde_json::json!({
            "axon_rotated": {
                "value": NEW_TOKEN,
                "expires_at_ms": 9_999_999_999_999i64,
            }
        })
        .to_string()
    })
    .await;
    let custody = seeded_custody();
    let (mut ctx, mut rx) = ctx_with(
        Some(custody.clone()),
        &[tool_spec("RefreshCrmToken", "http", &url, "")],
    );
    let outcome = dispatch_node(
        &rotate_node("expires_at < now() + interval '10 minutes'", "RefreshCrmToken"),
        &mut ctx,
    )
    .await
    .expect("rotate admits");

    // The summary: metadata only, exactly the expiring key.
    let summary = ctx.let_bindings.get("result").expect("binding").clone();
    assert!(summary.contains("\"attempted\":1"), "{summary}");
    assert!(summary.contains("crm.hubspot"), "{summary}");
    assert!(summary.contains("\"failed\":[]"), "{summary}");
    assert!(!summary.contains(OLD_TOKEN) && !summary.contains(NEW_TOKEN), "{summary}");

    // Custody committed: version 1 → 2, the tool's value + expiry stand.
    assert_eq!(custody.version_of(TENANT, "crm.hubspot"), Some(2));
    let revealed = custody
        .reveal_for_rotation(TENANT, "crm.hubspot")
        .await
        .expect("revealable");
    assert_eq!(revealed.value, NEW_TOKEN);
    assert_eq!(revealed.expires_at_ms, Some(9_999_999_999_999));
    // The untouched keys did not move.
    assert_eq!(custody.version_of(TENANT, "crm.zoho"), Some(1));

    // The tool SAW the old value (custody → tool channel) — the one
    // legitimate revelation — under the reserved envelope.
    let bodies = seen.lock().unwrap().clone();
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].contains("axon_rotation"), "{}", bodies[0]);
    assert!(bodies[0].contains(OLD_TOKEN), "{}", bodies[0]);
    assert!(bodies[0].contains("crm.hubspot"), "{}", bodies[0]);

    // And NOTHING on the flow side carries either value.
    let NodeOutcome::Completed { output, .. } = outcome else {
        panic!("expected Completed")
    };
    assert!(!output.contains(OLD_TOKEN) && !output.contains(NEW_TOKEN));
    drop(ctx);
    while let Ok(ev) = rx.try_recv() {
        let json = serde_json::to_string(&ev).unwrap();
        assert!(
            !json.contains(OLD_TOKEN) && !json.contains(NEW_TOKEN),
            "no value on the wire: {json}"
        );
    }
}

// ── 4: per-key degradation ──────────────────────────────────────────

#[tokio::test]
async fn broken_exchange_contract_degrades_that_key_with_a_witness() {
    // The stub provider replies "[stub] …" — not the reserved envelope.
    let custody = seeded_custody();
    let (mut ctx, _rx) = ctx_with(
        Some(custody.clone()),
        &[tool_spec("BadRefresher", "stub", "", "")],
    );
    let outcome = dispatch_node(&rotate_node("", "BadRefresher"), &mut ctx)
        .await
        .expect("sweep completes with per-key failures");
    let NodeOutcome::Completed { output, .. } = outcome else {
        panic!("expected Completed")
    };
    // Both class keys attempted, both failed with the contract witness.
    assert!(output.contains("\"attempted\":2"), "{output}");
    assert!(
        output.contains("rotation tool response"),
        "witness names the broken contract: {output}"
    );
    assert!(output.contains("\"rotated\":[]"), "{output}");
    // Old values intact — a failed exchange is never destructive.
    assert_eq!(custody.version_of(TENANT, "crm.hubspot"), Some(1));
    let revealed = custody
        .reveal_for_rotation(TENANT, "crm.hubspot")
        .await
        .unwrap();
    assert_eq!(revealed.value, OLD_TOKEN);
}

#[tokio::test]
async fn rotate_without_custody_fails_closed() {
    let (mut ctx, _rx) = ctx_with(None, &[tool_spec("R", "stub", "", "")]);
    let err = dispatch_node(&rotate_node("", "R"), &mut ctx)
        .await
        .expect_err("no custody ⇒ fail closed");
    assert!(format!("{err:?}").contains("secret_custody"), "{err:?}");
}

// ── 6: dispatch injection ───────────────────────────────────────────

#[tokio::test]
async fn use_tool_with_secret_injects_axon_secret_into_the_request() {
    let (url, seen) = local_tool_server(|_body| {
        // The vendor-shaped reply never echoes the credential.
        serde_json::json!({ "ok": true }).to_string()
    })
    .await;
    let custody = seeded_custody();
    let (mut ctx, _rx) = ctx_with(
        Some(custody),
        &[tool_spec("CrmCrearContacto", "http", &url, "crm.hubspot")],
    );
    let node = IRFlowNode::UseTool(IRUseToolStep {
        node_type: "use_tool",
        source_line: 1,
        source_column: 1,
        tool_name: "CrmCrearContacto".to_string(),
        argument: String::new(),
        named_args: vec![IRNamedArg {
            name: "nombre".to_string(),
            value: "Ada".to_string(),
            value_kind: "literal".to_string(),
        }],
    });
    dispatch_node(&node, &mut ctx).await.expect("dispatch admits");

    let bodies = seen.lock().unwrap().clone();
    assert_eq!(bodies.len(), 1);
    let body: serde_json::Value = serde_json::from_str(&bodies[0]).unwrap();
    assert_eq!(
        body["axon_secret"].as_str(),
        Some(OLD_TOKEN),
        "the custody value rides ONLY the tool request: {}",
        bodies[0]
    );
    assert_eq!(body["nombre"].as_str(), Some("Ada"));
    // The flow's bindings never carry the value.
    for (k, v) in &ctx.let_bindings {
        assert!(!v.contains(OLD_TOKEN), "binding {k} leaked the value");
    }
}

#[tokio::test]
async fn use_tool_with_secret_but_no_custody_fails_the_dispatch() {
    let (url, seen) = local_tool_server(|_| "{}".to_string()).await;
    let (mut ctx, _rx) = ctx_with(
        None,
        &[tool_spec("CrmCrearContacto", "http", &url, "crm.hubspot")],
    );
    let node = IRFlowNode::UseTool(IRUseToolStep {
        node_type: "use_tool",
        source_line: 1,
        source_column: 1,
        tool_name: "CrmCrearContacto".to_string(),
        argument: String::new(),
        named_args: vec![IRNamedArg {
            name: "nombre".to_string(),
            value: "Ada".to_string(),
            value_kind: "literal".to_string(),
        }],
    });
    // The dispatch completes as a FAILED tool result (witnessed), and the
    // vendor is NEVER called unauthenticated.
    let _ = dispatch_node(&node, &mut ctx).await;
    assert!(
        seen.lock().unwrap().is_empty(),
        "no custody ⇒ no unauthenticated vendor call"
    );
}
