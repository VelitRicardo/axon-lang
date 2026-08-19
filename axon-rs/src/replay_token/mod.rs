//! Deterministic replay tokens — v1.4.0.
//!
//! A [`ReplayToken`] is a compact receipt emitted at every effect
//! invocation (`call_tool`, `llm_infer`, `db_read`, `http_post`,
//! `ws_send`). It carries the minimum information needed to re-run
//! that effect and confirm the output is bit-identical: effect name,
//! canonical-JSON hash of inputs, canonical-JSON hash of outputs,
//! model version, sampling parameters (temperature, top_p, seed),
//! timestamp, and a 128-bit nonce.
//!
//! Regulated verticals (banking, fintech, legaltech, medicaltech)
//! use the token stream as an independently verifiable replay log —
//! an auditor loads the tokens, re-runs each effect under the
//! original model + parameters, and confirms the outputs match. If
//! they diverge, the divergence point is the exact token where the
//! system behaved differently.
//!
//! The on-the-wire format is canonical JSON with the Record
//! Separator (`\x1e`) separator so the hash computation is
//! byte-identical to the 10.g audit chain. Keeping the canonicaliser
//! in lockstep with enterprise audit means tokens emitted here can
//! be anchored to the enterprise hash chain with zero reformatting.
//!
//! This module hosts:
//!
//! - [`token`] — the canonical struct + hash derivation.
//! - [`log`] — the [`ReplayLog`] trait + two impls (in-memory,
//!   enterprise-audit-chain adapter shape).
//! - [`executor`] — [`ReplayExecutor`] that re-runs from a token and
//!   reports divergence.

pub mod executor;
pub mod log;
pub mod token;

pub use self::executor::{
    ReplayDivergence, ReplayExecutor, ReplayExecutorError, ReplayOutcome,
};
pub use self::log::{InMemoryReplayLog, ReplayLog, ReplayLogError};
pub use self::token::{
    canonical_hash, ReplayToken, ReplayTokenBuilder, SamplingParams,
};
