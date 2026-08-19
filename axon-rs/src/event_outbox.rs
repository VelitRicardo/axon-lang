//! v2.31.0 — the durable event outbox.
//!
//! v2.31.0 wired `emit` → a daemon `listen` through the v1.6.0 `TypedEventBus`;
//! v2.31.0 made delivery at-least-once WHILE THE PROCESS LIVES (retry over
//! the ephemeral mpsc queue). v2.31.0 adds the DURABLE substrate: an `emit`
//! to a `persistent_axonstore` channel APPENDS to an outbox — an
//! append-only log keyed by channel, with a per-channel processed cursor.
//! The receive driver DRAINS the unprocessed tail, delivers each event
//! (at-least-once, v2.31.0), and MARKS it processed. An event stays
//! redeliverable until acked + marked — so a consumer that was DOWN when
//! the event was emitted picks it up when it returns. This is the Q3
//! guarantee over a persisted log rather than an ephemeral queue.
//!
//! **Scope (honest).** This OSS module is the ABSTRACTION + an in-memory
//! reference. The two stronger guarantees layer on top:
//!
//! - **Crash durability** — the in-memory reference survives the *consumer*
//!   being down, but not a *process restart*. The per-tenant **Postgres**
//! outbox that survives a crash is v2.31.0 (the [`EventOutbox`] trait is
//!   the seam the enterprise `PgEventOutbox` implements).
//! - **Transactional atomicity** — the outbox row committed in the SAME
//!   store transaction as the producing flow's `mutate`/`persist`, so the
//! event is written IFF the state change committed. The v1.30.0/v1.32.0 store
//!   ops are per-op transactions today (no flow-level tx), so this is a
//!   documented refinement: it closes the small lost-/phantom-event window
//!   between the two separate commits. Until it lands, `emit` appends to
//!   the outbox as its own durable op — durable, but with that bounded
//!   atomicity gap. We do not call this layer "transactional" until the
//!   flow-level transaction is real (`delivery_is_a_kept_promise`).

use std::collections::HashMap;
use std::sync::Mutex;

/// One event in the outbox: its monotonic per-channel offset + payload.
#[derive(Debug, Clone, PartialEq)]
pub struct OutboxEntry {
    pub offset: u64,
    pub channel: String,
    pub payload: serde_json::Value,
}

/// The durable event outbox seam. `append` records an emitted event;
/// `unprocessed` returns the not-yet-acked tail for a channel (in offset
/// order — the redelivery set); `mark_processed` acks an offset so it is
/// never redelivered. The enterprise `PgEventOutbox` (v2.31.0) implements
/// this over a per-tenant Postgres table; the in-memory reference below is
/// the OSS default + the test double.
pub trait EventOutbox: Send + Sync {
    /// Append an event to `channel`'s log; returns its monotonic offset.
    fn append(&self, channel: &str, payload: serde_json::Value) -> u64;
    /// The unprocessed (not-yet-acked) events for `channel`, offset-ordered.
    fn unprocessed(&self, channel: &str) -> Vec<OutboxEntry>;
    /// Ack `offset` on `channel` — it will not be redelivered.
    fn mark_processed(&self, channel: &str, offset: u64);
}

#[derive(Default)]
struct ChannelLog {
    /// (offset, payload) in append order.
    entries: Vec<(u64, serde_json::Value)>,
    /// Acked offsets.
    processed: std::collections::HashSet<u64>,
    /// Next offset to assign (monotonic per channel).
    next_offset: u64,
}

/// In-memory reference [`EventOutbox`]. Durable WITHIN the process (an
/// event survives the consumer being down → it stays unprocessed +
/// redelivers); NOT across a process restart (that is the v2.31.0 Postgres
/// outbox). Thread-safe (one `Mutex` over the per-channel logs).
#[derive(Default)]
pub struct InMemoryEventOutbox {
    channels: Mutex<HashMap<String, ChannelLog>>,
}

impl InMemoryEventOutbox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Total unprocessed count across all channels (introspection / tests).
    pub fn pending_total(&self) -> usize {
        self.channels
            .lock()
            .unwrap()
            .values()
            .map(|log| log.entries.iter().filter(|(o, _)| !log.processed.contains(o)).count())
            .sum()
    }
}

impl EventOutbox for InMemoryEventOutbox {
    fn append(&self, channel: &str, payload: serde_json::Value) -> u64 {
        let mut chans = self.channels.lock().unwrap();
        let log = chans.entry(channel.to_string()).or_default();
        let offset = log.next_offset;
        log.next_offset += 1;
        log.entries.push((offset, payload));
        offset
    }

    fn unprocessed(&self, channel: &str) -> Vec<OutboxEntry> {
        let chans = self.channels.lock().unwrap();
        match chans.get(channel) {
            None => Vec::new(),
            Some(log) => log
                .entries
                .iter()
                .filter(|(o, _)| !log.processed.contains(o))
                .map(|(o, p)| OutboxEntry {
                    offset: *o,
                    channel: channel.to_string(),
                    payload: p.clone(),
                })
                .collect(),
        }
    }

    fn mark_processed(&self, channel: &str, offset: u64) {
        let mut chans = self.channels.lock().unwrap();
        if let Some(log) = chans.get_mut(channel) {
            log.processed.insert(offset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn append_assigns_monotonic_offsets_per_channel() {
        let ob = InMemoryEventOutbox::new();
        assert_eq!(ob.append("A", json!(1)), 0);
        assert_eq!(ob.append("A", json!(2)), 1);
        // Offsets are per-channel — B starts at 0.
        assert_eq!(ob.append("B", json!(9)), 0);
    }

    #[test]
    fn unprocessed_returns_the_redelivery_tail_in_order() {
        let ob = InMemoryEventOutbox::new();
        ob.append("A", json!("x"));
        ob.append("A", json!("y"));
        let tail = ob.unprocessed("A");
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].offset, 0);
        assert_eq!(tail[0].payload, json!("x"));
        assert_eq!(tail[1].offset, 1);
    }

    #[test]
    fn mark_processed_removes_from_the_redelivery_tail() {
        // The headline: an event stays redeliverable until acked. A consumer
        // that was down sees it on `unprocessed`; once it acks, it is gone.
        let ob = InMemoryEventOutbox::new();
        let off = ob.append("A", json!("hib"));
        assert_eq!(ob.unprocessed("A").len(), 1, "redeliverable until acked");
        ob.mark_processed("A", off);
        assert!(ob.unprocessed("A").is_empty(), "acked → not redelivered");
        assert_eq!(ob.pending_total(), 0);
    }

    #[test]
    fn unknown_channel_has_no_unprocessed_events() {
        let ob = InMemoryEventOutbox::new();
        assert!(ob.unprocessed("never").is_empty());
    }

    #[test]
    fn pending_total_counts_only_unacked() {
        let ob = InMemoryEventOutbox::new();
        ob.append("A", json!(1));
        let o2 = ob.append("A", json!(2));
        ob.append("B", json!(3));
        assert_eq!(ob.pending_total(), 3);
        ob.mark_processed("A", o2);
        assert_eq!(ob.pending_total(), 2);
    }
}
