//! v2.69.0 — **`upstream { resource: R }`: the client leg rides a governed
//! channel.**
//!
//! Three runtime laws, each proven against a REAL local WebSocket vendor and a
//! spec compiled by the REAL frontend (grammar → T951 → Phase-0 derivation →
//! dial — the whole v2.69.0 chain, not a hand-built literal):
//!
//! 1. **`capacity` bounds connection INSTANCES.** `resource { capacity: 1 }`
//! ⇒ the second concurrent dial WAITS (the v2.69.0 held-across-requests
//!    semantics); dropping the first handle releases the slot and the waiter
//!    completes. Frames were never the unit — `backpressure_credit` already
//!    governs frames; capacity bounding frames would state one fact twice.
//! 2. **A dial is a USE: a post-expiry lease breaches CT-2, fail-closed.**
//! The v2.67.0/v2.69.0 law on the client leg — a live lease permits the
//!    dial, an expired one refuses it BEFORE any handshake.
//! 3. **An un-resourced upstream is unchanged** — no bound, no lease charge,
//! the pre-v2.69.0 behaviour (and its IR carries neither new key).

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axon::ir_nodes::{IRLease, IRProgram, IRResource, IRUpstream};
use axon::resource_lease::ResourceLeaseGuard;
use axon::upstream_runtime::{
    dial_upstream, TracingLifecycleWitness, UpstreamConfigResolver, UpstreamError,
};
use futures::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

// ── The real pipeline: axon source → IRProgram ─────────────────────────────

fn compile(src: &str) -> IRProgram {
    let tokens = axon_frontend::lexer::Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = axon_frontend::parser::Parser::new(tokens).parse().expect("parse");
    let errors = axon_frontend::type_checker::TypeChecker::new(&prog).check();
    assert!(errors.is_empty(), "program must type-check: {errors:?}");
    axon_frontend::ir_generator::IRGenerator::new().generate(&prog)
}

/// `capacity: 1` — ONE concurrent instance of this upstream.
fn resourced_program() -> IRProgram {
    compile(
        r#"
session SttDialogue {
    axon:   [ send AudioChunk, receive Transcript, loop ]
    vendor: [ receive AudioChunk, send Transcript, loop ]
}
resource SttVendor {
    kind: https
    endpoint: upstream.vendor.url
    capacity: 1
    lifetime: affine
}
upstream DeepgramSTT {
    transport: websocket
    protocol: SttDialogue
    role: axon
    resource: SttVendor
    secret: upstream.vendor.api_key
    auth: header("Authorization", "Token ")
    map: [
        send AudioChunk as binary,
        receive Transcript as json when "type" = "Results",
    ]
}
"#,
    )
}

struct FixedResolver {
    url: String,
}

impl UpstreamConfigResolver for FixedResolver {
    fn resolve(&self, key: &str) -> Option<String> {
        // The ONLY key the artifact may present is the one DERIVED from the
        // resource's endpoint — pinning this here proves the v2.69.0 wire.
        (key == "upstream.vendor.url").then(|| self.url.clone())
    }
    fn reveal_secret(&self, _key: &str) -> Option<String> {
        Some("sk-test".into())
    }
}

/// Minimal vendor: accepts WS handshakes and parks until the client closes.
async fn spawn_vendor() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { return };
            tokio::spawn(async move {
                let Ok(mut ws) = tokio_tungstenite::accept_async(stream).await else { return };
                while let Some(Ok(msg)) = ws.next().await {
                    if let Message::Close(_) = msg {
                        let _ = ws.send(Message::Close(None)).await;
                        return;
                    }
                }
            });
        }
    });
    addr
}

// ── 1. capacity bounds INSTANCES ────────────────────────────────────────────

#[tokio::test]
async fn the_second_instance_waits_for_the_first_to_release() {
    let addr = spawn_vendor().await;
    let ir = resourced_program();
    let spec = ir.upstreams.first().expect("upstream lowered").clone();

    // The artifact carries the derived wire.
    assert_eq!(spec.resolve, "upstream.vendor.url", "address derives from resource.endpoint");
    assert_eq!(spec.capacity, Some(1), "instance bound derives from resource.capacity");

    let resolver = FixedResolver { url: format!("ws://{addr}/v1") };

    // Instance #1 occupies the sole slot.
    let handle1 = dial_upstream(&spec, &resolver, Arc::new(TracingLifecycleWitness), None)
        .await
        .expect("first dial");

    // Instance #2 WAITS — the bound queues, it does not lie by refusing.
    let waited = tokio::time::timeout(
        Duration::from_millis(300),
        dial_upstream(&spec, &resolver, Arc::new(TracingLifecycleWitness), None),
    )
    .await;
    assert!(
        waited.is_err(),
        "with capacity: 1 held, the second dial must still be WAITING at 300ms"
    );

    // Releasing the slot lets the next instance through.
    drop(handle1);
    let handle2 = tokio::time::timeout(
        Duration::from_secs(5),
        dial_upstream(&spec, &resolver, Arc::new(TracingLifecycleWitness), None),
    )
    .await
    .expect("the released slot must admit the waiter")
    .expect("second dial");
    drop(handle2);
}

// ── 2. a dial is a USE: post-expiry lease ⇒ CT-2, fail-closed ───────────────

fn lease(name: &str, resource_ref: &str) -> IRLease {
    IRLease {
        node_type: "lease",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        resource_ref: resource_ref.into(),
        duration: "1h".into(),
        acquire: "on_start".into(),
        on_expire: "anchor_breach".into(),
    }
}

fn ir_resource(name: &str) -> IRResource {
    let mut r = IRResource::new(name.into(), 0, 0);
    r.kind = "https".into();
    r.endpoint = "upstream.vendor.url".into();
    r.lifetime = "affine".into();
    r
}

#[tokio::test]
async fn a_post_expiry_dial_is_a_ct2_anchor_breach() {
    let addr = spawn_vendor().await;
    let ir = resourced_program();
    let spec: IRUpstream = ir.upstreams.first().expect("upstream lowered").clone();
    let resolver = FixedResolver { url: format!("ws://{addr}/v1") };

    let now = Arc::new(Mutex::new(chrono::Utc::now()));
    let c = now.clone();
    let guard = ResourceLeaseGuard::from_ir_with_clock(
        &[lease("VendorWindow", "SttVendor")],
        &[ir_resource("SttVendor")],
        Box::new(move || *c.lock().unwrap()),
    )
    .expect("the lease acquires")
    .expect("a lease was declared");

    // Within τ: the capability is held — the dial proceeds.
    let live = dial_upstream(&spec, &resolver, Arc::new(TracingLifecycleWitness), Some(&guard))
        .await
        .expect("a live lease permits the dial");
    drop(live);

    // Past τ: the SAME dial is the breach, refused before any handshake.
    *now.lock().unwrap() += chrono::Duration::seconds(3601);
    let err = dial_upstream(&spec, &resolver, Arc::new(TracingLifecycleWitness), Some(&guard))
        .await
        .expect_err("a post-expiry dial must refuse");
    match err {
        UpstreamError::LeaseBreach { upstream, detail } => {
            assert_eq!(upstream, "DeepgramSTT");
            assert!(
                detail.contains("CT-2 ANCHOR BREACH"),
                "the refusal must be the CT-2 Anchor Breach, got: {detail}"
            );
        }
        other => panic!("expected LeaseBreach, got {other}"),
    }
}

// ── 3. the un-resourced upstream is unchanged ───────────────────────────────

#[tokio::test]
async fn an_unresourced_upstream_stays_unbounded() {
    let addr = spawn_vendor().await;
    let ir = compile(
        r#"
session SttDialogue {
    axon:   [ send AudioChunk, receive Transcript, loop ]
    vendor: [ receive AudioChunk, send Transcript, loop ]
}
upstream DeepgramSTT {
    transport: websocket
    protocol: SttDialogue
    role: axon
    resolve: upstream.vendor.url
    secret: upstream.vendor.api_key
    auth: header("Authorization", "Token ")
    map: [
        send AudioChunk as binary,
        receive Transcript as json when "type" = "Results",
    ]
}
"#,
    );
    let spec = ir.upstreams.first().expect("upstream lowered").clone();
    assert!(spec.resource_ref.is_empty());
    assert_eq!(spec.capacity, None);

    let resolver = FixedResolver { url: format!("ws://{addr}/v1") };
    // Two concurrent instances, both admitted immediately — no bound.
    let h1 = dial_upstream(&spec, &resolver, Arc::new(TracingLifecycleWitness), None)
        .await
        .expect("dial 1");
    let h2 = tokio::time::timeout(
        Duration::from_secs(5),
        dial_upstream(&spec, &resolver, Arc::new(TracingLifecycleWitness), None),
    )
    .await
    .expect("no bound — must not wait")
    .expect("dial 2");
    drop(h1);
    drop(h2);
}
