//! v2.3.0 — the **runtime** of a session-typed dialogue.
//!
//! `axon-frontend::session` (v2.3.0) gives the static algebra: the
//! `SessionType` grammar, the duality involution, the regular-coinductive
//! equality, and the Presburger-decidable credit-refined backpressure
//! index `!ⁿA.S`. This crate's `session_runtime` module is the dynamic
//! counterpart — the operational state machine that *runs* a session
//! type over a network carrier.
//!
//! Layering:
//! - [`state::SessionRuntime`] is **transport-agnostic**: it owns a
//!   cursor (the residual session type after the trace so far), a
//!   [`state::CreditWindow`] (the runtime witness of `!ⁿA.S`), and one
//!   method per operational rule (`try_send` / `try_recv` / `try_select`
//!   / `try_offer` / `try_end`). The same runtime would plug into a
//!   QUIC stream, an in-process channel, or unit-test scaffolding.
//! - [`wire::Frame`] is the **closed-catalog JSON envelope** carried by
//!   one WebSocket text message per operational step. Versioned (`v:1`)
//!   first key, `kind` discriminator, fully `serde`-checked.
//! - [`ws`] is the **RFC 6455 carrier**: an axum upgrade-handler-shaped
//!   driver that reads peer frames, routes them onto the runtime, emits
//!   our outgoing frames in turn, and closes with `1000 normal closure`
//!   on `end` or `1002 protocol error` on a [`error::ProtocolError`].
//!
//! The carrier in 41.d is WebSocket; future cycles extend the same
//! `SessionRuntime` over:
//! - 41.f: the enterprise axum server (multi-tenant + RLS + audit per
//!   utterance — the Kivi single-image unblock);
//! - 41.g: typed reconnection via `cognitive_states` (the v2.3.0 residual
//!   type sealed at disconnect resumes from the same cursor);
//! - 41.h: multiparty projection (the global-type `G` projected to each
//!   role `G⌐r`, each running its own `SessionRuntime`).

/// v2.67.0 — the SessionType compiler: `IRSession` → [`crate::session::SessionType`].
///
/// The declarations reached the IR all along; nothing read them. The type-checker
/// lowered the roles to prove duality and dropped the result, and the enterprise
/// server — which *does* serve the wire — substituted a hardcoded chat schema for
/// every deployed socket. So a protocol could be *proven* dual at compile time and
/// a **different one** enforced at runtime. This module closes that gap.
pub mod compile;
pub mod error;
pub mod state;
pub mod wire;

// v2.81.0 — the two CARRIERS live behind the `server` feature; the
// protocol itself does not. `compile` (IRSession → SessionType, the v2.67.0 gap
// this module exists to close), `state` (the runtime, credit window, sealed
// resume) and `wire` (the frame encoding) are transport-agnostic and stay in
// every build — so `axon check` still proves a socket's protocol dual, and a
// `cli` install still type-checks every `socket` declaration it is handed. Only
// SERVING one needs `axum`: `ws` is the RFC 6455 upgrade handler, `sse` builds
// `axum::response::sse::Event`.
//
// NOTE for whoever comes next: `ws.rs` also owns `PeerRole`, `apply_outgoing`
// and `next_outgoing_frame` — the transport-agnostic core of the protocol loop,
// which `sse.rs` imports FROM the WebSocket carrier. That is the same smell this
// step moved three times (`AXON_VERSION`, `IngestProvenance`,
// `ServerExecutionResult`), and it is recorded here rather than fixed because it
// does not bite: both carriers are gated together, so nothing outside `server`
// names them. It WOULD bite the moment a third carrier arrives, or an SSE-only
// build is wanted.
#[cfg(feature = "server")]
pub mod sse;
#[cfg(feature = "server")]
pub mod ws;

pub use error::ProtocolError;
#[cfg(feature = "server")]
pub use sse::drive_sse_producer;
pub use state::{
    CreditWindow, ParkedContinuation, ResumeError, SealedRuntime, SessionRuntime,
    SEALED_RUNTIME_VERSION,
};
pub use compile::{credit_for_socket, schema_for_socket, server_schema, session_type_of_role};
pub use wire::{Frame, AXON_WIRE_VERSION};
#[cfg(feature = "server")]
pub use ws::{drive, PeerRole};
