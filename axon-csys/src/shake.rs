//! v4.0.0 — FIPS 202 (SHA-3 / SHAKE), bound from the vendored kernel.
//!
//! # What this is, and what it is not
//!
//! Every function here is a thin binding over `c-src/vendor/pqclean/fips202.c`.
//! **This module implements nothing.** That is deliberate and is the whole
//! argument of the design decision: the cryptographic module boundary lives in `axon-csys`
//!, and it is populated with vetted reference implementations rather
//! than code written here, because a from-scratch lattice-adjacent primitive is
//! the first thing a CMVP assessor questions.
//!
//! The upstream is pinned by commit and SHA-256 in
//! `c-src/vendor/PROVENANCE.toml`, and `tests/vendor_provenance.rs` recomputes
//! those hashes on every build.
//!
//! # Why SHAKE, and why first
//!
//! ML-DSA-65 (FIPS 204) derives its matrix expansion, its secret-vector
//! sampling and its challenge from **SHAKE-128 and SHAKE-256** — never from
//! SHA-256. So FIPS 202 is a prerequisite of the signature work, not an
//! independent addition, and `axon-csys` had no Keccak of any kind.
//!
//! It is also the smallest self-contained unit that exercises the entire
//! vendoring mechanism end to end: the directory, the provenance manifest, the
//! hash gate, the separate relaxed `cc::Build`, and a known-answer test. Proving
//! that on one primitive before importing a lattice scheme is the difference
//! between a mechanism that works and a mechanism that is assumed to.
//!
//! # A note on symbol names
//!
//! PQClean's `common/fips202.c` exports **unprefixed** symbols — `shake256`,
//! `sha3_256` — rather than the `PQCLEAN_*` names its per-scheme directories
//! use. They land in `libaxon_csys_vendor`, so a second definition anywhere in
//! the link graph would be a duplicate-symbol error at link time rather than a
//! silent substitution. Loud, but worth knowing before adding another Keccak
//! consumer.

#![allow(dead_code)]

/// SHAKE-256 output length used when a fixed-size digest is wanted. SHAKE is an
/// extendable-output function: this is a convention, not a property of the
/// algorithm.
pub const SHAKE256_DEFAULT_LEN: usize = 32;

#[cfg(feature = "native")]
extern "C" {
    fn shake128(output: *mut u8, outlen: usize, input: *const u8, inlen: usize);
    fn shake256(output: *mut u8, outlen: usize, input: *const u8, inlen: usize);
    fn sha3_256(output: *mut u8, input: *const u8, inlen: usize);
    fn sha3_512(output: *mut u8, input: *const u8, inlen: usize);
}

/// SHAKE-128 with a caller-chosen output length.
///
/// # Panics
///
/// Never. An empty `out` is a no-op and an empty `input` is a valid message —
/// SHAKE of the empty string is defined and has a published test vector.
#[cfg(feature = "native")]
pub fn shake128_xof(input: &[u8], out: &mut [u8]) {
    // SAFETY: both slices are valid for their stated lengths for the duration
    // of the call, the C kernel neither retains nor frees them, and it performs
    // no allocation. Null base pointers only occur when the corresponding
    // length is zero, which the kernel handles as an empty absorb / squeeze.
    unsafe { shake128(out.as_mut_ptr(), out.len(), input.as_ptr(), input.len()) }
}

/// SHAKE-256 with a caller-chosen output length.
#[cfg(feature = "native")]
pub fn shake256_xof(input: &[u8], out: &mut [u8]) {
    // SAFETY: as `shake128_xof`.
    unsafe { shake256(out.as_mut_ptr(), out.len(), input.as_ptr(), input.len()) }
}

/// SHAKE-256 squeezed to 32 bytes — the convention used where a fixed digest is
/// needed.
#[cfg(feature = "native")]
pub fn shake256_32(input: &[u8]) -> [u8; SHAKE256_DEFAULT_LEN] {
    let mut out = [0u8; SHAKE256_DEFAULT_LEN];
    shake256_xof(input, &mut out);
    out
}

/// SHA3-256.
#[cfg(feature = "native")]
pub fn sha3_256_digest(input: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    // SAFETY: as `shake128_xof`; `out` is exactly the 32 bytes SHA3-256 writes.
    unsafe { sha3_256(out.as_mut_ptr(), input.as_ptr(), input.len()) }
    out
}

/// SHA3-512.
#[cfg(feature = "native")]
pub fn sha3_512_digest(input: &[u8]) -> [u8; 64] {
    let mut out = [0u8; 64];
    // SAFETY: as above; `out` is exactly the 64 bytes SHA3-512 writes.
    unsafe { sha3_512(out.as_mut_ptr(), input.as_ptr(), input.len()) }
    out
}
