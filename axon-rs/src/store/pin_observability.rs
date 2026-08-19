//! v1.32.0 (D4) — pin observability emitter.
//!
//! Centralizes the `tracing::info!` events fired on every flow-scoped
//! `PoolConnection<Postgres>` acquire. The structured fields are the
//! load-bearing diagnostic surface for two operational scenarios:
//!
//! 1. **Pin saturation observability** — under load the pool may run
//!    out of connections; the operator wants to see WHICH flow held
//!    HOW MANY pins for HOW LONG. The acquire-time event with
//!    `store_name`, `flow_name`, `trace_id`, and `source` (eager vs
//!    lazy) gives a foundation to compute these metrics in any log
//!    aggregator (Loki, Datadog, ELK).
//!
//! 2. **Pin-leak detection** — an `acquire` event without a matching
//!    drop-time release indicates a code path holds a pin past the
//!    flow's lifetime. v1.39.0 ships ONLY acquire-time emit (the
//!    release is implicit at `PoolConnection::drop` and happens
//!    automatically when the flow-scoped HashMap drops); v1.40.0 may
//!    add a `PinObserved` wrapper struct that emits at Drop for
//!    explicit per-pin lifetime tracking. The minimal v1.39.0
//!    surface honors the rule "no unnecessary observability machinery"
//!    while still giving operators enough to detect saturation.
//!
//! `target = "axon::store::pin"` so adopters / SREs can filter the
//! tracing stream via standard subscriber-level filters
//! (`RUST_LOG=axon::store::pin=info`).

/// v1.32.0 (D4) — Emit a structured `tracing::info!` event for a
/// flow-scoped pin acquisition.
///
/// `source` is one of:
///
///  - `"eager"` — acquired at flow start by
///    [`crate::runner::execute_server_flow`] (sync path) or
///    [`crate::streaming_via_dispatcher::run_streaming_via_dispatcher`]
///    (async path) during the eager pin-discovery walk.
///  - `"lazy"` — acquired on-demand at first store-op touch in a
///    par-branch sub-context whose `pinned_conns` map is empty
///    (D6.a default — see [`crate::flow_dispatcher::parallel`]).
///
/// `branch_index` is `Some(idx)` when the acquire happens inside a
/// par-block branch (D6.c); `None` for the parent flow's linear walk.
///
/// The emitted event surface (post-substitution):
///
/// ```text
/// INFO axon::store::pin: pin acquired
///     path="acquire"
///     source="eager" | "lazy"
///     store_name=<axonstore name>
///     flow_name=<executing flow name>
///     trace_id=<request trace id, or empty for CLI / unwired callers>
///     branch_index=<par-block index, or empty for linear path>
///     d_letter="37.x.j.D4"
/// ```
///
/// Pure + total: this is a single `tracing::info!` macro call with
/// structured fields; never panics; zero allocations beyond the
/// formatter's transient string buffers.
#[inline]
pub fn emit_pin_acquire(
    store_name: &str,
    flow_name: &str,
    trace_id: &str,
    source: &str,
    branch_index: Option<usize>,
) {
    tracing::info!(
        target: "axon::store::pin",
        path = "acquire",
        source = source,
        store_name = %store_name,
        flow_name = %flow_name,
        trace_id = %trace_id,
        branch_index = ?branch_index,
        d_letter = "37.x.j.D4",
        "pin acquired"
    );
}

/// v1.32.0 (D4) — Symmetric counterpart for the implicit release
/// at end-of-flow. Called once by the flow's outer scope (sync runner
/// or async dispatcher) AFTER the pin map drops, with the total count
/// of pins released. This is a SUMMARY event, not per-pin: per-pin
/// drop happens via `PoolConnection::drop` which fires the `after_release
/// DEALLOCATE ALL` hook (v1.31.0 D2) on the way back to the pool.
///
/// A `released_count == acquired_count` is the canonical healthy
/// signal. A persistent mismatch over time would suggest a code path
/// holds a pin in a structure outlasting the flow — load-bearing pin-
/// leak detection signal.
#[inline]
pub fn emit_pin_flow_summary(
    flow_name: &str,
    trace_id: &str,
    released_count: usize,
) {
    tracing::info!(
        target: "axon::store::pin",
        path = "flow_end",
        flow_name = %flow_name,
        trace_id = %trace_id,
        released_count = released_count,
        d_letter = "37.x.j.D4",
        "flow ended; pins released to pool"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // The emitters are tracing-side-effect-only. Their correctness is
    // verified by (a) the type signature compiling + (b) the anchor
    // test at `axon-rs/tests/connection_pinning.rs` capturing
    // a tracing subscriber and asserting the structured fields land.
    // Here we only pin that the API surface exists and is callable.

    #[test]
    fn emit_pin_acquire_is_callable_with_typical_inputs() {
        emit_pin_acquire(
            "chat_history",
            "ChatFlow",
            "trace-abc-123",
            "eager",
            None,
        );
        emit_pin_acquire(
            "tenant_secrets",
            "WriteSecret",
            "",
            "lazy",
            Some(2),
        );
    }

    #[test]
    fn emit_pin_flow_summary_is_callable() {
        emit_pin_flow_summary("ChatFlow", "trace-abc-123", 6);
        emit_pin_flow_summary("ChatFlow", "", 0);
    }
}
