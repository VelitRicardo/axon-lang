//! axon-csys — C23 metal-bound kernels for axon-lang.
//!
//! Fase 25 — Silicon + Cognition (sesión 1). This crate is the Rust shim
//! around a small set of carefully chosen C23 kernels. The C side handles
//! what C does best — cache-line layout, bit twiddling, hardware
//! intrinsics, FSM dispatch with computed gotos. The Rust side handles
//! correctness, ownership, and async glue, exposing a safe API that
//! adopters consume without ever writing `unsafe` themselves.
//!
//! ## Layout
//!
//! - `c-src/probe/`   — build-infra probe (25.b)  ✅ shipped
//! - `c-src/audio/`   — G.711 + resample (25.c)   — pending
//! - `c-src/buffer/`  — slab allocator (25.d)     — pending
//! - `c-src/effects/` — FSM dispatcher (25.e)     — pending
//! - `c-src/tokens/`  — BPE + #embed (25.g)       — pending
//! - `c-src/crypto/`  — HMAC continuity (25.h)    — pending
//!
//! ## Founder principle
//!
//! Per the four-pillar reminder (2026-05-08), every C kernel must preserve
//! the Mathematics / Philosophy / Logic / Computing pillars of the module
//! it ports. The Rust shim layer is responsible for asserting the byte-
//! identical / epsilon-bounded drift gate that catches divergence before
//! it leaves CI.

// §Fase 119.h — these four modules ARE the C kernels: a cache-line slab
// allocator, a G.711/PCM resampler, a computed-goto effects FSM, and the
// build-infrastructure probe. There is no meaningful pure-Rust "fallback" for
// them — a Rust slab allocator is a different artifact, not the same one —
// and nothing upstream reaches them (`axon-lang` uses only `tokens`,
// `crypto` and `envelope`). So without `native` they are simply ABSENT,
// which is the honest shape: the feature that compiles the C is the feature
// that provides them.
#[cfg(feature = "native")]
pub mod audio;
#[cfg(feature = "native")]
pub mod buffer;
pub mod crypto;
#[cfg(feature = "native")]
pub mod effects;
pub mod envelope;
#[cfg(feature = "native")]
/// §Fase 124.b — Ed25519 (RFC 8032) bound from the VENDORED kernel.
/// Implements nothing itself; see `c-src/vendor/PROVENANCE.toml`.
pub mod ed25519;
/// §Fase 124.b — ML-DSA-65 (FIPS 204) bound from the VENDORED kernel.
/// Implements nothing itself; see `c-src/vendor/PROVENANCE.toml`.
pub mod mldsa;
pub mod probe;
/// §Fase 124.b — FIPS 202 (SHA-3 / SHAKE) bound from the VENDORED kernel.
/// Implements nothing itself; see `c-src/vendor/PROVENANCE.toml`.
pub mod shake;
pub mod tokens;

// §Fase 39.c.x — re-export the epistemic envelope public surface.
pub use envelope::{
    clamp_ceiling, theorem_5_1_ceiling_from_c, validate_degradation, EpistemicEnvelope,
    EpistemicEnvelopeCRepr, EpistemicKind, THEOREM_5_1_CEILING,
};

#[cfg(feature = "native")]
pub use audio::{
    mulaw_decode, mulaw_encode, resample_linear_pcm16, resample_linear_pcm16_output_len,
    ResampleError,
};
#[cfg(feature = "native")]
pub use buffer::{BufferPool, BufferPoolSnapshot, PoolClass, Slab};
pub use crypto::{
    b64url_decode, b64url_encode, ct_eq, hex_decode, hex_encode, hmac_sha256, sha256,
    ContinuityWire, ContinuityWireError, HmacSha256, Sha256, SHA256_BLOCK_SIZE, SHA256_DIGEST_SIZE,
};
#[cfg(feature = "native")]
pub use effects::{
    BuiltWire, Clause, DispatchError, DispatchResult, Dispatcher, EffectDecl, Frame, Instruction,
    Opcode, TraceEvent, Value as EffectValue, WireBuilder,
};
#[cfg(feature = "native")]
pub use probe::{
    probe_add, probe_c_standard, probe_cacheline_alignment, probe_cacheline_marker,
    probe_cacheline_size, probe_features, probe_version, AxonCsysFeatures, AxonCsysVersion,
};
pub use tokens::{
    cl100k_base, count_tokens, estimate, o200k_base, utf8_boundary_floor, utf8_count_chars,
    BpeError, CountKind, TokenCount, Tokenizer,
};
