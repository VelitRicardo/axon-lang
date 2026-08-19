//! AXON Runtime — Handler layer (v1.1.0).
//!
//! Direct port of `axon/runtime/handlers/`.
//!
//! The Handler trait is the single dispatch point for interpreting the
//! Intention Tree (Free Monad F_Σ(X)) into physical side effects. Each
//! concrete handler (Terraform, Kubernetes, AWS, Docker, MQ, gRPC, File)
//! lives in its own sub-module and implements the trait.

pub mod base;
pub mod dry_run;
/// v2.67.0 — the `Handler` that actually looks at something. Until it landed,
/// the only implementation was `DryRunHandler`, whose `observe` returns certainty
/// `1.0` unconditionally — a perfect-confidence clean bill of health for a system
/// nobody ever examined.
pub mod live;

pub use base::{
    BLAME_CALLEE, BLAME_CALLER, BLAME_INFRASTRUCTURE, Continuation, Handler, HandlerError,
    HandlerErrorKind, HandlerOutcome, HandlerRegistry, LambdaEnvelope, VALID_DERIVATIONS,
    VALID_OUTCOME_STATUSES, identity_continuation, make_envelope, now_iso,
};
pub use dry_run::{DryRunHandler, DryRunState};
pub use live::LiveHandler;
