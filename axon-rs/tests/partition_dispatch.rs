//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v2.49.0 — runtime for the parametric secret injection
//! (`the design plan`, axon-enterprise repo), doctrine
//! `selection_without_revelation`.
//!
//! Pinned properties:
//! 1. A `secret_partition:` tool resolves the custody key `class.<segment>`
//!    from the caller-bound partition parameter and injects the RIGHT
//!    sub-tenant's value under `axon_secret` — one tool, N sub-tenants.
//! 2. A different partition value selects a different custody entry (no
//!    cross-contamination).
//! 3. Class containment: a partition value carrying a `.` (a class-escape
//!    attempt) refuses the dispatch with a witness — the vendor is NEVER
//!    called, and no key outside the class is ever revealed.
//! 4. A missing/unbound partition argument fails the dispatch closed.
//! 5. The value never enters the flow's cognition space (bindings/wire).

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::{dispatch_node, DispatchCtx};
use axon::ir_nodes::{IRFlowNode, IRNamedArg, IRToolParam, IRToolSpec, IRUseToolStep};
use axon::secret_custody::InMemoryCustody;
use axon::tool_registry::ToolRegistry;
use std::sync::{Arc, Mutex};

const TENANT: &str = "kivi";
const TOKEN_ACME: &str = "acme-hubspot-token-111";
const TOKEN_GLOBEX: &str = "globex-hubspot-token-222";
const TOKEN_OTHER_CLASS: &str = "llm-provider-key-999";

/// A partitioned CRM tool: static class `crm.hubspot`, per-sub-tenant
/// segment from the `tenant_id` parameter.
fn partitioned_tool(url: &str) -> IRToolSpec {
    IRToolSpec {
        node_type: "tool_spec",
        source_line: 1,
        source_column: 1,
        name: "CrmCrearContacto".to_string(),
        provider: "http".to_string(),
        max_results: None,
        filter_expr: String::new(),
        timeout: "10s".to_string(),
        runtime: url.to_string(),
        resource_ref: String::new(),
        sandbox: None,
        input_schema: Vec::new(),
        output_schema: String::new(),
        parameters: vec![
            IRToolParam { name: "tenant_id".to_string(), type_name: "String".to_string(), optional: false },
            IRToolParam { name: "nombre".to_string(), type_name: "String".to_string(), optional: false },
        ],
        output_type: None,
        requires: Vec::new(),
        secret: "crm.hubspot".to_string(),
        secret_partition: "tenant_id".to_string(),
        effect_row: Vec::new(),
        target: None,
        risk: None,
        argv: Vec::new(),
        cache: String::new(),
        scrape: None,
    }
}

fn seeded_custody() -> Arc<InMemoryCustody> {
    let c = InMemoryCustody::new();
    c.seed(TENANT, "crm.hubspot.acme", TOKEN_ACME, None);
    c.seed(TENANT, "crm.hubspot.globex", TOKEN_GLOBEX, None);
    // A secret in ANOTHER class — a partition must never be able to reach it.
    c.seed(TENANT, "llm.kimi", TOKEN_OTHER_CLASS, None);
    Arc::new(c)
}

fn ctx_with(
    custody: Arc<InMemoryCustody>,
    tools: &[IRToolSpec],
) -> (
    DispatchCtx,
    tokio::sync::mpsc::UnboundedReceiver<axon::flow_execution_event::FlowExecutionEvent>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut ctx = DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx);
    ctx.tenant_id = TENANT.to_string();
    let mut registry = ToolRegistry::new();
    registry.register_from_ir(tools);
    ctx.tool_registry = Some(Arc::new(registry));
    ctx = ctx.with_secret_custody(custody);
    (ctx, rx)
}

async fn local_tool_server() -> (String, Arc<Mutex<Vec<String>>>) {
    use axum::routing::post;
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_for_handler = seen.clone();
    let app = axum::Router::new().route(
        "/",
        post(move |body: String| {
            let seen = seen_for_handler.clone();
            async move {
                seen.lock().unwrap().push(body.clone());
                serde_json::json!({ "ok": true }).to_string()
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

fn use_node(tenant_arg: Option<&str>) -> IRFlowNode {
    let mut named = vec![IRNamedArg {
        name: "nombre".to_string(),
        value: "Ada".to_string(),
        value_kind: "literal".to_string(),
    }];
    if let Some(t) = tenant_arg {
        named.insert(
            0,
            IRNamedArg {
                name: "tenant_id".to_string(),
                value: t.to_string(),
                value_kind: "literal".to_string(),
            },
        );
    }
    IRFlowNode::UseTool(IRUseToolStep {
        node_type: "use_tool",
        source_line: 1,
        source_column: 1,
        tool_name: "CrmCrearContacto".to_string(),
        argument: String::new(),
        named_args: named,
    })
}

#[tokio::test]
async fn partition_resolves_the_right_sub_tenant_value() {
    let (url, seen) = local_tool_server().await;
    let (mut ctx, mut rx) = ctx_with(seeded_custody(), &[partitioned_tool(&url)]);

    dispatch_node(&use_node(Some("acme")), &mut ctx)
        .await
        .expect("dispatch admits");

    let bodies = seen.lock().unwrap().clone();
    assert_eq!(bodies.len(), 1);
    let body: serde_json::Value = serde_json::from_str(&bodies[0]).unwrap();
    // The resolved key was `crm.hubspot.acme` — acme's token, not globex's.
    assert_eq!(body["axon_secret"].as_str(), Some(TOKEN_ACME), "{}", bodies[0]);
    assert_eq!(body["tenant_id"].as_str(), Some("acme"));

    // The value never entered cognition (bindings / wire).
    for (k, v) in &ctx.let_bindings {
        assert!(!v.contains(TOKEN_ACME), "binding {k} leaked the value");
    }
    drop(ctx);
    while let Ok(ev) = rx.try_recv() {
        let json = serde_json::to_string(&ev).unwrap();
        assert!(!json.contains(TOKEN_ACME), "value on the wire: {json}");
    }
}

#[tokio::test]
async fn a_different_partition_value_selects_a_different_entry() {
    let (url, seen) = local_tool_server().await;
    let (mut ctx, _rx) = ctx_with(seeded_custody(), &[partitioned_tool(&url)]);

    dispatch_node(&use_node(Some("globex")), &mut ctx)
        .await
        .expect("dispatch admits");

    let bodies = seen.lock().unwrap().clone();
    let body: serde_json::Value = serde_json::from_str(&bodies[0]).unwrap();
    assert_eq!(body["axon_secret"].as_str(), Some(TOKEN_GLOBEX), "{}", bodies[0]);
    // Never acme's — no cross-sub-tenant contamination.
    assert!(!bodies[0].contains(TOKEN_ACME), "{}", bodies[0]);
}

#[tokio::test]
async fn a_dotted_partition_value_cannot_escape_the_class() {
    let (url, seen) = local_tool_server().await;
    let (mut ctx, _rx) = ctx_with(seeded_custody(), &[partitioned_tool(&url)]);

    // `llm.kimi` lives in another class. A partition value crafted to reach
    // it (`.kimi` would make `crm.hubspot..kimi`, or any dotted value) must
    // be refused: the segment charset forbids `.`. The vendor is NEVER
    // called, so no cross-class credential can even be attempted.
    let _ = dispatch_node(&use_node(Some("kimi.evil")), &mut ctx).await;

    assert!(
        seen.lock().unwrap().is_empty(),
        "a dotted partition segment must refuse before any vendor call"
    );
    for (_k, v) in &ctx.let_bindings {
        assert!(!v.contains(TOKEN_OTHER_CLASS), "other-class value must never surface");
    }
}

#[tokio::test]
async fn a_missing_partition_argument_fails_closed() {
    let (url, seen) = local_tool_server().await;
    let (mut ctx, _rx) = ctx_with(seeded_custody(), &[partitioned_tool(&url)]);

    // No `tenant_id` bound → the segment is unresolved → refuse.
    let _ = dispatch_node(&use_node(None), &mut ctx).await;

    assert!(
        seen.lock().unwrap().is_empty(),
        "an unbound partition must refuse before any vendor call"
    );
}
