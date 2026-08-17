//! §Fase 124.b — Ed25519 (RFC 8032), bound from the vendored kernel.
//!
//! Thin bindings over `c-src/vendor/orlp-ed25519/` — **this module implements
//! nothing**, the same posture as [`crate::shake`] and [`crate::mldsa`].
//! Upstream is pinned by commit and per-file SHA-256 in
//! `c-src/vendor/PROVENANCE.toml`.
//!
//! # The conventions this shim normalises, and why they are written down
//!
//! Three ways the orlp API differs from the PQClean one two modules over, each
//! established by reading the vendored source rather than assumed:
//!
//! 1. **`ed25519_verify` returns 1 = valid, 0 = invalid** — the OPPOSITE of
//!    PQClean's `0 = ok`. A shim author pattern-matching on the ML-DSA binding
//!    would invert the verifier, and an inverted verifier is not a bug that
//!    announces itself: it rejects everything real and accepts everything
//!    tampered, which a lazy "roundtrip fails, must be the keys" reading can
//!    chase for hours. The KATs kill the inversion directly.
//! 2. **The private key is the 64-byte EXPANDED form** (SHA-512 of the seed,
//!    clamped), not the 32-byte RFC 8032 seed that `ed25519-dalek` and most
//!    modern APIs traffic in. This shim's public surface speaks SEEDS, and
//!    expansion happens at the boundary — so the caller never holds two
//!    "private key" shapes with the same name.
//! 3. **Signing takes the public key as an argument** rather than re-deriving
//!    it. The shim passes the pair together so a caller cannot mismatch them.
//!
//! # Determinism
//!
//! Unlike ML-DSA's hedged signing, Ed25519 signing is fully deterministic:
//! same seed, same message ⇒ same 64 bytes, across implementations. The KATs
//! exploit this for the strongest available cross-check — byte-identical
//! signatures against RFC 8032's published vectors AND against an independent
//! implementation.

#![allow(dead_code)]

use core::ffi::c_int;

/// RFC 8032 seed length — the canonical private-key form.
pub const ED25519_SEED_BYTES: usize = 32;
/// Public key length.
pub const ED25519_PUBLICKEY_BYTES: usize = 32;
/// The vendored kernel's expanded private key (SHA-512 of seed, clamped).
pub const ED25519_EXPANDEDKEY_BYTES: usize = 64;
/// Detached signature length.
pub const ED25519_SIGNATURE_BYTES: usize = 64;

#[cfg(feature = "native")]
extern "C" {
    fn ed25519_create_keypair(public_key: *mut u8, private_key: *mut u8, seed: *const u8);
    fn ed25519_sign(
        signature: *mut u8,
        message: *const u8,
        message_len: usize,
        public_key: *const u8,
        private_key: *const u8,
    );
    fn ed25519_verify(
        signature: *const u8,
        message: *const u8,
        message_len: usize,
        public_key: *const u8,
    ) -> c_int;
    /// PQClean common — the module's entropy source, shared with ML-DSA.
    ///
    /// The C-visible name `randombytes` is a PREPROCESSOR alias
    /// (`#define randombytes PQCLEAN_randombytes` in randombytes.h), so the
    /// symbol the linker actually exports is the prefixed one. An extern
    /// declared as `randombytes` compiles cleanly and fails at LINK time —
    /// which is exactly how this line was first written, and how it was
    /// corrected.
    #[link_name = "PQCLEAN_randombytes"]
    fn randombytes(output: *mut u8, n: usize) -> c_int;
}

/// A keypair in this module's canonical shapes: 32-byte public key plus the
/// kernel's 64-byte expanded private key. The seed that produced it is the
/// caller's to keep or destroy — the expanded key alone signs.
#[cfg(feature = "native")]
pub struct Ed25519Keypair {
    pub public: [u8; ED25519_PUBLICKEY_BYTES],
    pub expanded: [u8; ED25519_EXPANDEDKEY_BYTES],
}

/// Draw a fresh RFC 8032 seed from the module's own entropy source.
///
/// `None` = OS RNG failure, surfaced rather than panicked for the same reason
/// as [`crate::mldsa::keypair`]: the evidence signer must be able to refuse
/// loudly instead of aborting a server.
#[cfg(feature = "native")]
pub fn generate_seed() -> Option<[u8; ED25519_SEED_BYTES]> {
    let mut seed = [0u8; ED25519_SEED_BYTES];
    // SAFETY: exact-length buffer; the C side writes fully on rc == 0.
    let rc = unsafe { randombytes(seed.as_mut_ptr(), seed.len()) };
    (rc == 0).then_some(seed)
}

/// Derive the keypair a seed determines. Deterministic — RFC 8032 §5.1.5.
#[cfg(feature = "native")]
pub fn keypair_from_seed(seed: &[u8; ED25519_SEED_BYTES]) -> Ed25519Keypair {
    let mut pk = [0u8; ED25519_PUBLICKEY_BYTES];
    let mut sk = [0u8; ED25519_EXPANDEDKEY_BYTES];
    // SAFETY: all three buffers are exactly the lengths the kernel contracts;
    // it writes pk and sk fully and retains nothing.
    unsafe { ed25519_create_keypair(pk.as_mut_ptr(), sk.as_mut_ptr(), seed.as_ptr()) };
    Ed25519Keypair {
        public: pk,
        expanded: sk,
    }
}

/// Sign `msg`, detached. Deterministic: same keypair + message ⇒ same bytes.
#[cfg(feature = "native")]
pub fn sign(msg: &[u8], kp: &Ed25519Keypair) -> [u8; ED25519_SIGNATURE_BYTES] {
    let mut sig = [0u8; ED25519_SIGNATURE_BYTES];
    // SAFETY: exact-length buffers; empty msg is a valid message (RFC 8032's
    // own first test vector signs it).
    unsafe {
        ed25519_sign(
            sig.as_mut_ptr(),
            msg.as_ptr(),
            msg.len(),
            kp.public.as_ptr(),
            kp.expanded.as_ptr(),
        )
    };
    sig
}

/// Verify a detached signature.
#[cfg(feature = "native")]
pub fn verify(sig: &[u8], msg: &[u8], pk: &[u8; ED25519_PUBLICKEY_BYTES]) -> bool {
    if sig.len() != ED25519_SIGNATURE_BYTES {
        return false;
    }
    // SAFETY: read-only slices, valid for their lengths.
    let rc = unsafe { ed25519_verify(sig.as_ptr(), msg.as_ptr(), msg.len(), pk.as_ptr()) };
    // orlp convention: 1 = valid. NOT the PQClean `0 = ok` — see module doc.
    rc == 1
}
