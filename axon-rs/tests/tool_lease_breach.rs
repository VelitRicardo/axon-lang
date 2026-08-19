//! v2.69.0 — **`lease` over a vendor: the CT-2 Anchor Breach fires on a tool
//! call.**
//!
//! v2.67.0 gave `lease` its first use-site — a store operation is a *use* of the
//! resource, so a post-expiry store op breaches. v2.69.0 made a `tool` name a
//! resource too, so a **tool call is also a use**, and a post-expiry vendor call
//! must breach the same way. This is that use-site.
//!
//! The mechanism is the shared `ResourceLeaseGuard`, keyed by resource. These
//! tests exercise it directly (the end-to-end acquisition at the tool dispatch is
//! threaded on the server path via `lambda_tools::charge_tool_lease`).

use axon::ir_nodes::{IRLease, IRResource};
use axon::resource_lease::ResourceLeaseGuard;
use std::sync::{Arc, Mutex};

fn resource(name: &str, lifetime: &str) -> IRResource {
    let mut r = IRResource::new(name.into(), 0, 0);
    r.kind = "https".into();
    r.endpoint = "vendor.base".into();
    r.lifetime = lifetime.into();
    r
}

fn lease(name: &str, resource_ref: &str, duration: &str, on_expire: &str) -> IRLease {
    IRLease {
        node_type: "lease",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        resource_ref: resource_ref.into(),
        duration: duration.into(),
        acquire: "on_start".into(),
        on_expire: on_expire.into(),
    }
}

/// 🎯 **A post-expiry tool call over a leased vendor is a CT-2 Anchor Breach.**
///
/// This is the assertion v2.69.0 exists for — the same guarantee v2.67.0 proved for
/// stores, now on the tool channel. A live lease permits the call; past τ, the
/// same call breaches.
#[test]
fn a_post_expiry_vendor_call_is_a_ct2_anchor_breach() {
    let now = Arc::new(Mutex::new(chrono::Utc::now()));
    let c = now.clone();
    let guard = ResourceLeaseGuard::from_ir_with_clock(
        &[lease("Nightly", "SearchApi", "1h", "anchor_breach")],
        &[resource("SearchApi", "affine")],
        Box::new(move || *c.lock().unwrap()),
    )
    .expect("the lease acquires")
    .expect("a lease was declared");

    // Within τ: the capability is held, the vendor call is permitted.
    assert!(
        guard.charge("SearchApi").is_ok(),
        "a live lease must permit the vendor call — the capability IS held"
    );

    // …an hour and a second later, the same call is a breach.
    *now.lock().unwrap() += chrono::Duration::seconds(3601);

    let breach = guard
        .charge("SearchApi")
        .expect_err("post-expiry USE is the CT-2 Anchor Breach");
    assert_eq!(breach.resource, "SearchApi");
    assert_eq!(breach.lease, "Nightly");
    assert!(breach.to_string().contains("ANCHOR BREACH"));
}

/// `on_expire: extend` renews the window — the vendor call proceeds.
#[test]
fn on_expire_extend_renews_and_the_call_proceeds() {
    let now = Arc::new(Mutex::new(chrono::Utc::now()));
    let c = now.clone();
    let guard = ResourceLeaseGuard::from_ir_with_clock(
        &[lease("Rolling", "SearchApi", "1h", "extend")],
        &[resource("SearchApi", "affine")],
        Box::new(move || *c.lock().unwrap()),
    )
    .unwrap()
    .unwrap();

    *now.lock().unwrap() += chrono::Duration::seconds(3601);
    assert!(guard.charge("SearchApi").is_ok(), "extend renews the τ window");
    // The renewal is real: another minute on, still live.
    *now.lock().unwrap() += chrono::Duration::seconds(60);
    assert!(guard.charge("SearchApi").is_ok(), "the renewed token is the one now held");
}

/// `on_expire: release` surrenders the capability — the call still cannot proceed.
/// Releasing is not permission.
#[test]
fn on_expire_release_surrenders_and_the_call_is_refused() {
    let now = Arc::new(Mutex::new(chrono::Utc::now()));
    let c = now.clone();
    let guard = ResourceLeaseGuard::from_ir_with_clock(
        &[lease("Batch", "SearchApi", "1h", "release")],
        &[resource("SearchApi", "affine")],
        Box::new(move || *c.lock().unwrap()),
    )
    .unwrap()
    .unwrap();

    *now.lock().unwrap() += chrono::Duration::seconds(3601);
    assert!(
        guard.charge("SearchApi").is_err(),
        "a released lease is a capability no longer held — the vendor call must refuse"
    );
}

/// A resource with no lease over it is never charged — the call proceeds.
#[test]
fn a_resource_with_no_lease_is_ungoverned() {
    let guard = ResourceLeaseGuard::from_ir_with_clock(
        &[lease("Nightly", "SearchApi", "1h", "anchor_breach")],
        &[resource("SearchApi", "affine")],
        Box::new(chrono::Utc::now),
    )
    .unwrap()
    .unwrap();
    // A DIFFERENT resource has no token — charging it is a no-op.
    assert!(guard.charge("SomeOtherApi").is_ok());
}

/// A lease over a `persistent` resource is refused at acquire — the `!` exponential
/// has no τ to decay. (The same law v2.67.0 proved for stores.)
#[test]
fn a_lease_over_a_persistent_resource_is_refused() {
    let err = ResourceLeaseGuard::from_ir_with_clock(
        &[lease("Nightly", "SearchApi", "1h", "anchor_breach")],
        &[resource("SearchApi", "persistent")],
        Box::new(chrono::Utc::now),
    )
    .expect_err("a lease over persistent has no τ to decay");
    assert!(err.detail.contains("persistent"), "got: {}", err.detail);
}

/// A program with no leases builds no guard — the feature costs nothing.
#[test]
fn no_leases_means_no_guard() {
    let g = ResourceLeaseGuard::from_ir(&[], &[resource("SearchApi", "affine")]).unwrap();
    assert!(g.is_none());
}

// ── v2.69.0 — the STREAMING path enforces it too ──────────────────────────────

/// 🔴 **Both tool dispatch paths charge the lease.**
///
/// v2.69.0 wired `charge_tool_lease` into the canonical `use Tool(…)` path AND the
/// streaming step-tool path (`pure_shape`). Governing one but not the other would
/// be the "real-on-one-path, dead-on-the-other" defect v2.67.0 exists to end. This
/// pins that the shared charge point is reachable by name — the seam both paths
/// call.
#[test]
fn the_lease_charge_is_shared_by_both_tool_paths() {
    use axon::cancel_token::CancellationFlag;
    use axon::flow_dispatcher::lambda_tools::charge_tool_lease_by_name;
    use axon::flow_dispatcher::DispatchCtx;

    // Compile a program: a resourced tool + an EXPIRED lease over its resource.
    let src = "resource Api { kind: https  endpoint: vendor.base  lifetime: affine }\n\
               tool Search { provider: http  resource: Api  runtime: search }\n\
               lease Gone { resource: Api  duration: 1h  on_expire: anchor_breach }\n";
    let tokens = axon_frontend::lexer::Lexer::new(src, "<t>").tokenize().unwrap();
    let prog = axon_frontend::parser::Parser::new(tokens).parse().unwrap();
    let ir = axon_frontend::ir_generator::IRGenerator::new().generate(&prog);

    // Build a registry + a lease guard acquired NOW, then advance the clock past τ
    // so the charge sees an expired lease.
    let mut reg = axon::tool_registry::ToolRegistry::new();
    reg.register_from_ir(&ir.tools);
    let now = Arc::new(Mutex::new(chrono::Utc::now()));
    let c = now.clone();
    let guard = ResourceLeaseGuard::from_ir_with_clock(
        &ir.leases,
        &ir.resources,
        Box::new(move || *c.lock().unwrap()),
    )
    .unwrap()
    .unwrap();
    // An hour and a second later — the lease has expired.
    *now.lock().unwrap() += chrono::Duration::seconds(3601);

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let ctx = DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx)
        .with_tool_leases(Arc::new(guard));
    let ctx = {
        let mut c = ctx;
        c.tool_registry = Some(Arc::new(reg));
        c
    };

    // The shared charge point — the one BOTH paths call — reports the breach.
    let breach = charge_tool_lease_by_name("Search", &ctx)
        .expect("a post-expiry vendor call must breach on the shared path");
    assert!(breach.contains("ANCHOR BREACH"), "got: {breach}");

    // A tool with no resource is ungoverned on the same shared path.
    assert!(charge_tool_lease_by_name("Nonexistent", &ctx).is_none());
}
