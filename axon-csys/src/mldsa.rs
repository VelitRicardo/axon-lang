//! §Fase 124.b — ML-DSA-65 (FIPS 204), bound from the vendored kernel.
//!
//! # What this is, and what it is not
//!
//! Thin bindings over `c-src/vendor/pqclean/ml-dsa-65/` — **this module
//! implements nothing**, same posture as [`crate::shake`] and for the same
//! reason: the cryptographic module boundary lives in `axon-csys` (D124.3) and
//! is populated with vetted reference implementations (D124.6), because a
//! from-scratch lattice scheme is the first thing a CMVP assessor questions.
//! Upstream is pinned by commit and per-file SHA-256 in
//! `c-src/vendor/PROVENANCE.toml`; the provenance gate recomputes the hashes on
//! every build.
//!
//! # The shape of the scheme, for readers who know Ed25519 and not this
//!
//! ML-DSA is "hedged" by default: signing consumes fresh OS randomness (via the
//! vendored `randombytes`, part of the module), so **two signatures over the
//! same message differ** — unlike Ed25519, where signing is deterministic.
//! Verification is deterministic in both. Anything downstream that compares
//! signature bytes for equality is therefore wrong by construction; compare by
//! verifying.
//!
//! Sizes are fixed by FIPS 204 for this parameter set and pinned in
//! `tests/mldsa_kat.rs`: pk 1952, sk 4032, sig 3309.
//!
//! # Secret-key handling
//!
//! The functions here hand the caller raw key material and do nothing else with
//! it. Lifecycle — zeroization, custody, never logging — is the SIGNER's
//! responsibility (the §124.b `Signer` implementation in axon-rs), not this
//! binding's: a binding that quietly wipes buffers it does not own would be a
//! surprise, and a binding that clones them into managed wrappers would put
//! policy inside the module boundary where an assessor least wants it.

#![allow(dead_code)]

use core::ffi::c_int;

/// FIPS 204 ML-DSA-65 public key length.
pub const MLDSA65_PUBLICKEY_BYTES: usize = 1952;
/// FIPS 204 ML-DSA-65 secret key length.
pub const MLDSA65_SECRETKEY_BYTES: usize = 4032;
/// FIPS 204 ML-DSA-65 signature length.
pub const MLDSA65_SIGNATURE_BYTES: usize = 3309;

#[cfg(feature = "native")]
extern "C" {
    fn PQCLEAN_MLDSA65_CLEAN_crypto_sign_keypair(pk: *mut u8, sk: *mut u8) -> c_int;
    fn PQCLEAN_MLDSA65_CLEAN_crypto_sign_signature(
        sig: *mut u8,
        siglen: *mut usize,
        m: *const u8,
        mlen: usize,
        sk: *const u8,
    ) -> c_int;
    fn PQCLEAN_MLDSA65_CLEAN_crypto_sign_verify(
        sig: *const u8,
        siglen: usize,
        m: *const u8,
        mlen: usize,
        pk: *const u8,
    ) -> c_int;
}

/// Generate a keypair from the module's own entropy source.
///
/// `None` means the OS RNG failed — the one runtime failure this scheme has.
/// It is not collapsed into a panic because the caller (the evidence signer)
/// must be able to refuse loudly rather than abort a server.
#[cfg(feature = "native")]
pub fn keypair() -> Option<([u8; MLDSA65_PUBLICKEY_BYTES], [u8; MLDSA65_SECRETKEY_BYTES])> {
    let mut pk = [0u8; MLDSA65_PUBLICKEY_BYTES];
    let mut sk = [0u8; MLDSA65_SECRETKEY_BYTES];
    // SAFETY: both buffers are exactly the lengths api.h declares for this
    // parameter set; the kernel writes fully and retains nothing.
    let rc = unsafe { PQCLEAN_MLDSA65_CLEAN_crypto_sign_keypair(pk.as_mut_ptr(), sk.as_mut_ptr()) };
    (rc == 0).then_some((pk, sk))
}

/// Sign `msg`, detached, with the FIPS 204 "pure" mode and empty context —
/// the non-`_ctx` entry point, which forwards an empty context internally.
///
/// Hedged: consumes OS randomness, so repeated calls yield different (all
/// valid) signatures. `None` = RNG failure.
#[cfg(feature = "native")]
pub fn sign(
    msg: &[u8],
    sk: &[u8; MLDSA65_SECRETKEY_BYTES],
) -> Option<[u8; MLDSA65_SIGNATURE_BYTES]> {
    let mut sig = [0u8; MLDSA65_SIGNATURE_BYTES];
    let mut siglen: usize = 0;
    // SAFETY: `sig` has the exact capacity api.h promises the kernel writes;
    // `msg` may be empty (null base + zero len is handled as an empty absorb).
    let rc = unsafe {
        PQCLEAN_MLDSA65_CLEAN_crypto_sign_signature(
            sig.as_mut_ptr(),
            &mut siglen,
            msg.as_ptr(),
            msg.len(),
            sk.as_ptr(),
        )
    };
    if rc != 0 {
        return None;
    }
    // ML-DSA signatures are fixed-length; a short write is not a smaller
    // signature, it is a broken kernel, and silently returning a buffer with
    // trailing zeros would VERIFY as garbage downstream.
    assert_eq!(
        siglen, MLDSA65_SIGNATURE_BYTES,
        "ML-DSA-65 produced a {siglen}-byte signature; FIPS 204 fixes it at {MLDSA65_SIGNATURE_BYTES}"
    );
    Some(sig)
}

/// Verify a detached signature. Deterministic; no randomness involved.
#[cfg(feature = "native")]
pub fn verify(sig: &[u8], msg: &[u8], pk: &[u8; MLDSA65_PUBLICKEY_BYTES]) -> bool {
    // A wrong-length signature is invalid by definition — the kernel would
    // reject it too, but answering here keeps the FFI call on the happy path
    // only and makes the contract visible in Rust.
    if sig.len() != MLDSA65_SIGNATURE_BYTES {
        return false;
    }
    // SAFETY: all three slices are valid for their lengths; the kernel only
    // reads.
    let rc = unsafe {
        PQCLEAN_MLDSA65_CLEAN_crypto_sign_verify(
            sig.as_ptr(),
            sig.len(),
            msg.as_ptr(),
            msg.len(),
            pk.as_ptr(),
        )
    };
    rc == 0
}
