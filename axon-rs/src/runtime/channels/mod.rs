//! AXON Runtime — Typed Channels (v1.6.0).
//!
//! Native Rust port of the Python reference module
//! `axon/runtime/channels/typed.py` (v1.6.0). Closes the runtime gap
//! left open by v1.6.0's release v1.4.2: the frontend was at parity but
//! `axon-rs` had no executor for the new `channel`/`emit`/`publish`/
//! `discover` surface. End-to-end programs running on the Rust runtime
//! now get the same typed-channel guarantees the Python runtime offers.
//!
//! Surface re-exports: see `typed`.

pub mod typed;
pub mod executor;

pub use typed::{
    Capability, ShieldComplianceFn, TypedChannelError, TypedChannelHandle,
    TypedChannelRegistry, TypedEvent, TypedEventBus, TypedPayload,
};
pub use executor::{
    DispatchError, RunContext, RunValue,
    dispatch_emit, dispatch_publish, dispatch_discover, dispatch_listen,
};
