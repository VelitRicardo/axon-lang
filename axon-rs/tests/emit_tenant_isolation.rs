//! v2.83.0 — **`emit` must not wake another tenant's continuation.**
//!
//! # The defect, found 2026-08-08 in code shipped since 2.83.0
//!
//! [`ParkingLot`] is a process singleton. `by_event` is keyed by the event NAME
//! and nothing else, so `take_resumable(event_name, now)` claimed EVERY
//! unexpired continuation registered under that name — across every tenant in
//! the process. The production call site (`wire_integrations::run_emit`) passed
//! the channel name and spawned a resume for each match, unfiltered.
//!
//! `ParkedFlow` already carried `tenant_id` and `session_id`. The information
//! was there; the wake path did not look at it. And `resume_parked_flow`
//! RESTORES `ctx.tenant_id` from the parked flow — so tenant B's continuation
//! resumed as B, with A's payload bound under the event name. That is the v2.17.0
//! cross-tenant class, inside a primitive v2.83.0 shipped.
//!
//! It surfaced as a flaky test (`hibernate`, whose four cases all
//! park under the same event name) and was PROVEN pre-existing rather than
//! assumed: the tree was stashed to exactly HEAD and the binary run three times
//! — FAILED, ok, ok.
//!
//! # The two decisions this file pins
//!
//! **1. The tenant is the isolation boundary; the session is not.** `emit` is a
//! channel-level signal within a tenant, and v2.31.0's durable delivery is
//! tenant-scoped. Two sessions of the same tenant SHOULD see each other's
//! events — scoping to the session would silently break the ordinary
//! multi-connection case to fix a problem that is about tenancy.
//!
//! **2. A cross-tenant candidate is SKIPPED, not an error.** Tenant A's
//! legitimate emit must not fail because tenant B happens to have parked
//! something under the same name — that would let any tenant deny another by
//! choosing a channel name. And the skip is not logged with the other tenant's
//! id: telling A that "some other tenant" has a continuation on this channel
//! leaks tenancy structure into A's logs.
//!
//! Every test here uses a UNIQUE event name, which is also the fix for the root
//! cause of the flake that exposed all this: a process-global lot plus shared
//! event names between concurrent tests is a race by construction.

use axon::hibernation::{parking_lot, ParkedFlow};
use std::sync::Arc;

fn parked(id: &str, tenant: &str, session: &str, event: &str) -> ParkedFlow {
    ParkedFlow {
        continuation_id: id.into(),
        flow_name: "SleepyFlow".into(),
        event_name: event.into(),
        deadline_ms: None,
        remaining_nodes: Vec::new(),
        let_bindings: std::collections::HashMap::new(),
        step_counter: 0,
        backend_name: "stub".into(),
        system_prompt: String::new(),
        tenant_id: tenant.into(),
        session_id: session.into(),
        mandate_specs: Arc::new(Vec::new()),
        lambda_data_specs: Arc::new(Vec::new()),
        ots_specs: Arc::new(Vec::new()),
        compute_specs: Arc::new(Vec::new()),
        agent_specs: Arc::new(Vec::new()),
    }
}

// ── section 1 — the isolation itself ───────────────────────────────────────────────

#[test]
fn m4_1_an_emit_does_not_wake_another_tenants_continuation() {
    let lot = parking_lot();
    let event = "m4_1_quarterly_close";

    lot.park(parked("m4_1_a", "tenant-acme", "s1", event));
    lot.park(parked("m4_1_b", "tenant-globex", "s1", event));

    // Tenant acme emits on the channel. Both continuations await this exact
    // event name; only acme's may be claimed.
    let woken = lot.take_resumable("tenant-acme", event, 0);

    assert_eq!(
        woken.len(),
        1,
        "acme's emit must wake exactly its OWN continuation, not every one \
         parked under this event name"
    );
    assert_eq!(woken[0].continuation_id, "m4_1_a");
    assert_eq!(woken[0].tenant_id, "tenant-acme");

    assert!(
        lot.is_parked("m4_1_b"),
        "globex's continuation must still be parked — it was never acme's to \
         claim, and claiming it would resume B's flow with A's payload"
    );

    // Cleanup: leave the process-global lot as we found it.
    let _ = lot.claim("m4_1_b", 0);
}

#[test]
fn m4_2_the_other_tenants_emit_wakes_its_own() {
    let lot = parking_lot();
    let event = "m4_2_quarterly_close";
    lot.park(parked("m4_2_a", "tenant-acme", "s1", event));
    lot.park(parked("m4_2_b", "tenant-globex", "s1", event));

    let woken = lot.take_resumable("tenant-globex", event, 0);
    assert_eq!(woken.len(), 1);
    assert_eq!(woken[0].continuation_id, "m4_2_b");
    assert!(lot.is_parked("m4_2_a"), "acme's is untouched");
    let _ = lot.claim("m4_2_a", 0);
}

// ── section 2 — the session is NOT the boundary ───────────────────────────────────

/// Decision, stated as a test: two sessions of the SAME tenant see each other's
/// events. Scoping the wake to the session would break the ordinary
/// multi-connection case to fix a problem that is about tenancy.
#[test]
fn m4_3_sessions_of_one_tenant_are_not_isolated_from_each_other() {
    let lot = parking_lot();
    let event = "m4_3_quarterly_close";
    lot.park(parked("m4_3_a", "tenant-acme", "session-1", event));
    lot.park(parked("m4_3_b", "tenant-acme", "session-2", event));

    let woken = lot.take_resumable("tenant-acme", event, 0);
    assert_eq!(
        woken.len(),
        2,
        "both sessions of the same tenant wake — the tenant is the boundary"
    );
}

// ── section 3 — back-compat for the single-tenant (OSS) case ──────────────────────

/// An OSS run has an empty `tenant_id` on both sides, so the match is an
/// equality between two empty strings and the behaviour is byte-identical to
/// pre-v2.83.0. This is what keeps the fix from being a breaking change for
/// every adopter who never had tenants.
#[test]
fn m4_4_the_untenanted_oss_case_is_unchanged() {
    let lot = parking_lot();
    let event = "m4_4_quarterly_close";
    lot.park(parked("m4_4_a", "", "", event));

    let woken = lot.take_resumable("", event, 0);
    assert_eq!(woken.len(), 1, "an untenanted emit wakes an untenanted park");
    assert_eq!(woken[0].continuation_id, "m4_4_a");
}

/// …and strictness in the other direction: a TENANTED emit must not wake an
/// untenanted continuation. A parked flow with no tenant in a multi-tenant
/// process is an artifact, and guessing which tenant it belongs to is exactly
/// the kind of substitution this project refuses.
#[test]
fn m4_5_a_tenanted_emit_does_not_wake_an_untenanted_park() {
    let lot = parking_lot();
    let event = "m4_5_quarterly_close";
    lot.park(parked("m4_5_orphan", "", "", event));

    let woken = lot.take_resumable("tenant-acme", event, 0);
    assert!(
        woken.is_empty(),
        "strict equality: an orphaned park is not adoptable by whichever \
         tenant happens to emit next"
    );
    assert!(lot.is_parked("m4_5_orphan"));
    let _ = lot.claim("m4_5_orphan", 0);
}

// ── section 4 — expiry is still enforced, per tenant ──────────────────────────────

#[test]
fn m4_6_expiry_is_reaped_only_within_the_emitting_tenant() {
    let lot = parking_lot();
    let event = "m4_6_quarterly_close";
    let mut stale = parked("m4_6_stale", "tenant-acme", "s1", event);
    stale.deadline_ms = Some(1_000);
    lot.park(stale);
    let mut other = parked("m4_6_other", "tenant-globex", "s1", event);
    other.deadline_ms = Some(1_000);
    lot.park(other);

    // acme emits well past both deadlines.
    let woken = lot.take_resumable("tenant-acme", event, 5_000);
    assert!(woken.is_empty(), "acme's own entry expired");
    assert!(!lot.is_parked("m4_6_stale"), "and was reaped in passing");
    assert!(
        lot.is_parked("m4_6_other"),
        "globex's expired entry is NOT acme's to reap — lazy expiry must not \
         become a cross-tenant side effect"
    );
    let _ = lot.claim("m4_6_other", 0);
}
