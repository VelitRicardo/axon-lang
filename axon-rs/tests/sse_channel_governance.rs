//! v2.69.0 (SSE catch-up) — **the streaming path governs the channel too.**
//!
//! v2.69.0 gave a `tool` a governed channel: `resource.capacity` bounds concurrency
//! and a `lease` over the resource makes a post-expiry vendor call a CT-2 Anchor
//! Breach. The sync path (`execute_server_flow`) threaded the guards onto its
//! `DispatchCtx`. The **SSE / real-per-token path** (`server_execute_streaming` →
//! `run_streaming_via_dispatcher`) did NOT — its ctx carried `None`, so a
//! `capacity: N` tool invoked from a streaming endpoint ran unbounded and an
//! expired `lease` never breached. That is the exact "real-on-one-path,
//! dead-on-the-other" defect v2.67.0→v2.69.0 exists to end — and it shipped in the
//! v2.69.0 core (OSS 2.69.0) on the sync half only.
//!
//! This gate drives the ACTUAL production SSE entry point
//! (`run_streaming_via_dispatcher`) with an EXPIRED lease over a resourced tool
//! and proves the breach fires on the wire — parity with the sync path. The two
//! drives differ ONLY in whether the guard is threaded, so a breach in the first
//! and none in the second isolates the guard as the cause (not a compile/dispatch
//! artifact). Both drives resolve the resource endpoint so the tool SURVIVES the
//! deny-by-default `resolve_from_resources` drop — otherwise there is no tool to
//! charge and the test would prove nothing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axon::cancel_token::CancellationFlag;
use axon::channel_semaphore::ChannelSemaphores;
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::IRResource;
use axon::resource_lease::ResourceLeaseGuard;

/// A program whose flow USES a resourced tool, with a `lease` over that resource.
/// The tool call is a *use* of the resource — so past τ it must breach.
const SRC: &str = "\
resource Api { kind: https  endpoint: vendor.base  lifetime: affine }\n\
tool Search { provider: stub  resource: Api  runtime: search }\n\
lease Gone { resource: Api  duration: 1h  on_expire: anchor_breach }\n\
flow Run() -> Text { use Search on \"q\" }\n";

/// Build an `Api` lease guard, then advance its clock past τ so a charge breaches.
fn expired_guard() -> Arc<ResourceLeaseGuard> {
    let tokens = axon_frontend::lexer::Lexer::new(SRC, "<sse>")
        .tokenize()
        .expect("lex");
    let prog = axon_frontend::parser::Parser::new(tokens).parse().expect("parse");
    let ir = axon_frontend::ir_generator::IRGenerator::new().generate(&prog);

    let now = Arc::new(Mutex::new(chrono::Utc::now()));
    let c = now.clone();
    let guard = ResourceLeaseGuard::from_ir_with_clock(
        &ir.leases,
        &ir.resources,
        Box::new(move || *c.lock().unwrap()),
    )
    .expect("the lease acquires (affine resource)")
    .expect("a lease was declared");
    *now.lock().unwrap() += chrono::Duration::seconds(3601);
    Arc::new(guard)
}

/// Drive the SSE dispatcher on `SRC`, threading the given channel guards, and
/// collect every emitted event. The `channel_semaphores` / `tool_leases` params
/// are the SAME two the OSS SSE catch-up added to `run_streaming_via_dispatcher`.
async fn drive_sse(
    channel_semaphores: Option<Arc<ChannelSemaphores>>,
    tool_leases: Option<Arc<ResourceLeaseGuard>>,
) -> Vec<FlowExecutionEvent> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = CancellationFlag::new();
    let enforcement = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
    let audit = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let warnings = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    axon::streaming_via_dispatcher::run_streaming_via_dispatcher(
        SRC.to_string(),
        "<sse>".to_string(),
        "Run".to_string(),
        "stub".to_string(),
        cancel,
        tx,
        enforcement,
        audit,
        warnings,
        Arc::new(Mutex::new(axon::temporal_context::TemporalState::default())),
        None, // held_capabilities
        None, // request_body
        std::collections::HashMap::new(),
        std::collections::HashMap::new(),
        None, // tool_base_url
        None, // api_key
        channel_semaphores,
        tool_leases,
        // v2.89.0 — unscoped tenant, passed verbatim (v2.49.0).
        String::new(),
    )
    .await;

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }
    events
}

fn breach_output(events: &[FlowExecutionEvent]) -> Option<String> {
    events.iter().find_map(|e| match e {
        FlowExecutionEvent::StepComplete {
            success,
            full_output,
            ..
        } if !success && full_output.contains("ANCHOR BREACH") => Some(full_output.clone()),
        _ => None,
    })
}

/// 🎯 **A post-expiry vendor call over a leased resource breaches on the SSE
/// path — and only when the guard is threaded.** This is the assertion the
/// catch-up exists for: the streaming dispatcher now populates `ctx.tool_leases`
/// from its new param, so the shared `charge_tool_lease_by_name` seam (reached by
/// the tool step) reports the breach at parity with the sync path.
///
/// One test (not two) so the process-global resource env var is set once, with no
/// cross-thread env race.
#[tokio::test]
async fn the_sse_tool_path_breaches_a_post_expiry_lease_iff_the_guard_is_threaded() {
    // Resolve the resource endpoint so the tool SURVIVES the deny-by-default drop
    // in `resolve_from_resources` — otherwise `Search` is removed and there is no
    // tool to charge. `vendor.base` → `AXON_RESOURCE_VENDOR_BASE` (edition 2021:
    // `set_var` is safe; this binary holds exactly this one test).
    std::env::set_var("AXON_RESOURCE_VENDOR_BASE", "https://vendor.example");

    // WITH the guard: the expired lease breaches on the streaming tool path.
    let with_guard = drive_sse(None, Some(expired_guard())).await;
    let breach = breach_output(&with_guard).expect(
        "the SSE tool path must breach a post-expiry lease — a None here means the streaming \
         dispatcher is NOT threading tool_leases onto the ctx (v2.69.0 real on sync, inert on SSE)",
    );
    assert!(breach.contains("ANCHOR BREACH"), "got: {breach}");

    // WITHOUT the guard: the SAME program + SAME resolved endpoint does NOT breach
    // — the tool call proceeds. The only difference is the guard, so it is the
    // guard reaching the SSE ctx that breaches, not a compile/dispatch artifact.
    let no_guard = drive_sse(None, None).await;
    assert!(
        breach_output(&no_guard).is_none(),
        "no lease guard threaded — nothing must breach; got a breach without a guard"
    );
}

/// An `Api` resource carrying `capacity: 1` — the semaphore the SSE dispatcher
/// must acquire a permit from before it calls the tool.
fn cap1_resource() -> IRResource {
    let mut r = IRResource::new("Api".into(), 0, 0);
    r.kind = "https".into();
    r.endpoint = "vendor.base".into();
    r.capacity = Some(1);
    r
}

/// 🎯 **`capacity:` is a REAL bound on the SSE path — the streaming tool call
/// acquires (and blocks on) the channel permit from the threaded ctx.**
///
/// The lease test above proves the guards reach the SSE ctx; this proves the
/// *semaphore* half is enforced there behaviorally, not just wired. A `capacity: 1`
/// channel whose single permit is held externally must PARK the SSE tool call: the
/// flow reaches `StepStart` but cannot complete the tool step while the permit is
/// unavailable. Release the permit → the call proceeds and the flow completes.
///
/// If the SSE path did NOT acquire the permit from the ctx (the "inert on SSE"
/// bug), the flow would complete WHILE the permit is held — which this test fails
/// on. Deterministic: holding the sole permit blocks indefinitely, so the "not yet
/// complete" observation never races.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_sse_tool_call_is_bounded_by_the_channel_capacity_permit() {
    // Resolve the endpoint so the tool survives `resolve_from_resources` (as in the
    // lease test) — otherwise there is no tool to bound.
    std::env::set_var("AXON_RESOURCE_VENDOR_BASE", "https://vendor.example");

    // A `capacity: 1` channel for `Api`, threaded into the SSE dispatcher. Hold its
    // SOLE permit before the run so the dispatcher's acquire must park.
    let sems = Arc::new(ChannelSemaphores::from_resources(&[cap1_resource()]));
    let held = sems
        .for_resource("Api")
        .expect("capacity: 1 builds a semaphore")
        .try_acquire_owned()
        .expect("the sole permit is free before the run");

    // Drive the SSE dispatcher in a task; flip `completed` only when it returns
    // (which requires the tool step to finish + the flow to complete + tx to drop).
    let completed = Arc::new(AtomicBool::new(false));
    let sems_for_run = sems.clone();
    let completed_for_run = completed.clone();
    let task = tokio::spawn(async move {
        let events = drive_sse(Some(sems_for_run), None).await;
        completed_for_run.store(true, Ordering::SeqCst);
        events
    });

    // While the permit is HELD, the tool call parks — the flow must NOT complete.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !completed.load(Ordering::SeqCst),
        "the SSE flow completed WHILE the sole capacity permit was held — the streaming tool \
         path is NOT acquiring the permit from the ctx (v2.69.0 capacity inert on SSE)"
    );

    // Release the permit → the parked acquire proceeds, the flow completes.
    drop(held);
    let events = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("once the permit frees, the SSE flow must complete promptly")
        .expect("the dispatcher task must not panic");
    assert!(
        completed.load(Ordering::SeqCst),
        "releasing the permit must let the SSE flow complete"
    );
    // And it ran to a terminal FlowComplete once unblocked.
    assert!(
        events
            .iter()
            .any(|e| matches!(e, FlowExecutionEvent::FlowComplete { .. })),
        "the unblocked SSE flow must reach FlowComplete"
    );
}
