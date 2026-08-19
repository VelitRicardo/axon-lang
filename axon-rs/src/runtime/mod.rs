//! AXON Runtime primitives (λ-L-E v1.1.0 + v1.1.0; v1.6.0 typed channels).
//!
//! Direct port of `axon/runtime/` sub-modules (lease_kernel, reconcile_loop,
//! ensemble_aggregator, immune kernels). v1.6.0 adds the typed
//! channels runtime (`channels::typed::TypedEventBus`) — the Rust-runtime
//! parity for the Python `axon/runtime/channels/typed.py` module.

pub mod budget_kernel;
pub mod channels;
pub mod ensemble_aggregator;
pub mod immune;
pub mod lease_kernel;
pub mod reconcile_loop;
