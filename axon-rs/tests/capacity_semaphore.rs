//! v2.69.0 — **`capacity` becomes a real bound: the concurrency semaphore.**
//!
//! v2.69.0 *derived* `resource.capacity` into `ToolEntry::capacity` — the number
//! reached the runtime. But a number that reaches the runtime and bounds nothing
//! is the nominal link this whole line exists to avoid. I said so explicitly in
//! the v2.69.0 commit: *"the number travels but is not yet enforced."*
//!
//! v2.69.0 is the enforcement — a `tokio::Semaphore` per channel, held across
//! requests on `ServerState`, keyed by resource. A permit is acquired before each
//! call and held across it, so at most `capacity` calls are in flight against a
//! channel at once. The N+1th parks until one returns.
//!
//! These tests exercise the semaphore itself. The end-to-end acquisition at the
//! tool call site (`lambda_tools::acquire_channel_permit`) is threaded on the
//! server path; here we prove the bound the derivation produces is real.

use axon::channel_semaphore::ChannelSemaphores;
use axon::ir_nodes::IRResource;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn res(name: &str, capacity: Option<i64>) -> IRResource {
    let mut r = IRResource::new(name.into(), 0, 0);
    r.kind = "https".into();
    r.endpoint = "vendor.base".into();
    r.capacity = capacity;
    r
}

/// **`capacity: 8` produces a semaphore of eight.** The number derived in v2.69.0
/// is the permit count.
#[test]
fn the_declared_capacity_is_the_permit_count() {
    let sems = ChannelSemaphores::from_resources(&[res("Api", Some(8))]);
    assert_eq!(sems.permits_of("Api"), Some(8));
}

/// **A resource with no `capacity:` is unbounded** — no semaphore, byte-identical
/// to pre-v2.69.0. Capacity is optional discipline.
#[test]
fn a_resource_without_capacity_is_unbounded() {
    let sems = ChannelSemaphores::from_resources(&[res("Api", None)]);
    assert!(sems.for_resource("Api").is_none());
}

/// 🔴 **The bound is REAL: `capacity: 2` permits exactly two calls in flight, and
/// the third WAITS until one returns.**
///
/// This is the assertion that separates v2.69.0 (a wire) from v2.69.0 alone (a
/// number). Three tasks race for a 2-permit channel; the peak concurrency
/// observed must never exceed 2.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn capacity_two_never_lets_three_calls_run_at_once() {
    let sems = Arc::new(ChannelSemaphores::from_resources(&[res("Api", Some(2))]));
    let sem = sems.for_resource("Api").unwrap();

    let in_flight = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();
    for _ in 0..6 {
        let sem = sem.clone();
        let in_flight = in_flight.clone();
        let peak = peak.clone();
        handles.push(tokio::spawn(async move {
            // Acquire a permit — the N+1th call parks here until one frees.
            let _permit = sem.acquire_owned().await.unwrap();
            let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            // Simulate the vendor call holding the connection.
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            in_flight.fetch_sub(1, Ordering::SeqCst);
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    assert!(
        peak.load(Ordering::SeqCst) <= 2,
        "capacity: 2 must NEVER allow more than two calls in flight against the channel — the \
         third waits. Peak observed: {}. Before v2.69.0 a tool had no bound; a `par` over N items \
         opened N connections to a vendor that tolerated ten.",
        peak.load(Ordering::SeqCst)
    );
    // And all six eventually ran (the bound throttles, it does not drop).
    assert_eq!(in_flight.load(Ordering::SeqCst), 0);
}

/// Two resources have independent semaphores — one channel at capacity does not
/// block a different channel.
#[test]
fn two_channels_have_independent_bounds() {
    let sems = ChannelSemaphores::from_resources(&[res("A", Some(1)), res("B", Some(3))]);
    assert_eq!(sems.permits_of("A"), Some(1));
    assert_eq!(sems.permits_of("B"), Some(3));

    let a = sems.for_resource("A").unwrap();
    let _held = a.clone().try_acquire_owned().unwrap();
    // A is now exhausted, but B is untouched.
    assert!(a.try_acquire_owned().is_err(), "A at capacity");
    assert!(
        sems.for_resource("B").unwrap().try_acquire_owned().is_ok(),
        "B is a different channel and must be unaffected"
    );
}
