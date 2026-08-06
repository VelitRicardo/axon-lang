//! Persistent Epistemic Modeling (PEM) — §λ-L-E Fase 11.d.
//!
//! When a WebSocket connection drops mid-conversation, the agent's
//! cognitive state (density matrix, belief state, short-term
//! memory) must survive the disconnect so a reconnecting client
//! picks up the exact same probabilistic thread. Without this, a
//! tab refresh forces the agent to restart from scratch — which is
//! both a UX cliff and, for long sessions, a hallucination risk
//! (the re-primed agent may re-derive answers inconsistent with
//! what the human saw 30 seconds earlier).
//!
//! This module provides the primitives. The persistence backend
//! itself is pluggable: [`backend::InMemoryBackend`] for dev/tests,
//! and a Postgres + envelope-encrypted backend in
//! `axon_enterprise::cognitive_states` for production.
//!
//! Composition notes
//! =================
//!
//! - 11.a `Stream<T>` / `Trusted<T>` — state snapshots carry
//!   already-trusted user inputs, so the checker's refinement
//!   tracking continues to hold across reconnects.
//! - 11.b `ZeroCopyBuffer` — short-term memory stores symbolic
//!   pointers to audio/video buffers rather than embedding bytes
//!   into the state snapshot.
//! - 11.c `ReplayToken` — every state rehydration emits a
//!   `pem:state_restored` audit event that anchors to the tenant's
//!   audit chain.

pub mod backend;
pub mod continuity_token;
/// §Fase 119.b — `e(t)` for `mandate`. The paper places the semantic validator
/// inside the PEM engine (`docs/papers/paper_mandate.md`), so it lives here.
pub mod semantic_validator;
pub mod state;

pub use self::backend::{
    InMemoryBackend, PersistenceBackend, PersistenceError,
};
pub use self::continuity_token::{
    ContinuityToken, ContinuityTokenError, ContinuityTokenSigner,
};
pub use self::state::{
    CognitiveState, FixedPoint, MemoryEntry, Q32_32_SCALE,
};
pub use self::semantic_validator::{
    Clause, Constraint, ConstraintSet, Predicate, ValidatorError, Verdict, Violation,
};
