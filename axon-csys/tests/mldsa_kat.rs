//! §Fase 124.b — **known-answer and interop tests for the vendored ML-DSA-65.**
//!
//! # Why fixed vectors are not enough here, and what replaces them
//!
//! SHAKE got a published-vector KAT because SHAKE is a function: same input,
//! same output, compare against NIST's bytes. ML-DSA-65 signing is HEDGED —
//! it consumes fresh OS randomness on every call, so no fixed signature vector
//! can be pinned without a seeded API the vendored `clean` variant does not
//! expose.
//!
//! And a sign→verify roundtrip alone proves only SELF-consistency. A kernel
//! wrong in the same way on both sides passes its own roundtrip forever — the
//! §122.b lesson (a green check that never leaves the instrument proves the
//! instrument) applied to cryptography.
//!
//! So the load-bearing proof is **cross-implementation interop** against
//! RustCrypto's independent FIPS 204 implementation:
//!
//! - C signs → Rust verifies: proves the C kernel EMITS standard signatures.
//! - Rust signs → C verifies: proves the C kernel ACCEPTS standard signatures,
//!   not merely its own dialect.
//!
//! Both directions, because each closes a hole the other leaves open.
//!
//! # `native`-only, same reasoning as `shake_kat.rs`
//!
//! Without the C kernel there is nothing to exercise; the provenance gate is
//! the check that runs everywhere.

#![cfg(feature = "native")]

use axon_csys::mldsa::{
    keypair, sign, verify, MLDSA65_PUBLICKEY_BYTES, MLDSA65_SECRETKEY_BYTES,
    MLDSA65_SIGNATURE_BYTES,
};

/// The FIPS 204 §4 table sizes for ML-DSA-65, pinned as numbers rather than as
/// references to the constants — asserting `CONST == CONST` proves nothing when
/// both sides are the same wrong edit.
#[test]
fn the_sizes_are_the_fips_204_parameter_set() {
    assert_eq!(MLDSA65_PUBLICKEY_BYTES, 1952);
    assert_eq!(MLDSA65_SECRETKEY_BYTES, 4032);
    assert_eq!(MLDSA65_SIGNATURE_BYTES, 3309);
}

/// 🎯 Roundtrip — necessary, nowhere near sufficient (see module doc).
#[test]
fn sign_verify_roundtrip() {
    let (pk, sk) = keypair().expect("OS RNG available");
    let msg = b"axon: evidencia que se firma";
    let sig = sign(msg, &sk).expect("OS RNG available");
    assert!(
        verify(&sig, msg, &pk),
        "a signature this kernel just produced must verify"
    );
}

/// Hedged signing: two signatures over one message differ, and both verify.
///
/// This pins the property the module doc warns about — anything downstream
/// comparing signature bytes for equality is wrong by construction.
#[test]
fn two_signatures_over_one_message_differ_and_both_verify() {
    let (pk, sk) = keypair().expect("rng");
    let msg = b"hedged";
    let a = sign(msg, &sk).expect("rng");
    let b = sign(msg, &sk).expect("rng");
    assert_ne!(
        a.as_slice(),
        b.as_slice(),
        "FIPS 204 default signing is hedged; identical signatures mean the RNG fed the \
         kernel constant bytes, which is a much worse bug than a test failure"
    );
    assert!(verify(&a, msg, &pk));
    assert!(verify(&b, msg, &pk));
}

/// 🎯 Tampering, each surface separately: message, signature, key.
#[test]
fn any_tampered_byte_fails_verification() {
    let (pk, sk) = keypair().expect("rng");
    let msg = b"do not touch";
    let sig = sign(msg, &sk).expect("rng");

    let mut bad_msg = msg.to_vec();
    bad_msg[0] ^= 1;
    assert!(
        !verify(&sig, &bad_msg, &pk),
        "a flipped message bit must fail"
    );

    // Flip a byte at the front, middle and back of the signature — the three
    // regions encode different algebraic objects (c-tilde, z, hints) and a
    // verifier could plausibly check some and not others.
    for pos in [
        0usize,
        MLDSA65_SIGNATURE_BYTES / 2,
        MLDSA65_SIGNATURE_BYTES - 1,
    ] {
        let mut bad_sig = sig;
        bad_sig[pos] ^= 1;
        assert!(
            !verify(&bad_sig, msg, &pk),
            "a flipped signature byte at {pos} must fail"
        );
    }

    let mut bad_pk = pk;
    bad_pk[7] ^= 1;
    assert!(
        !verify(&sig, msg, &bad_pk),
        "a flipped public-key byte must fail"
    );

    assert!(
        !verify(&sig[..100], msg, &pk),
        "a truncated signature must fail"
    );
}

// ── The interop gates ───────────────────────────────────────────────────────

/// 🎯 C signs → the independent Rust implementation verifies.
#[test]
fn a_c_signature_verifies_under_the_independent_implementation() {
    use ml_dsa::signature::Verifier;

    let (pk, sk) = keypair().expect("rng");
    let msg = b"interoperabilidad, direccion uno";
    let sig = sign(msg, &sk).expect("rng");

    let enc_vk = ml_dsa::EncodedVerifyingKey::<ml_dsa::MlDsa65>::try_from(pk.as_slice())
        .expect("1952 bytes is the encoded verifying-key length");
    let vk = ml_dsa::VerifyingKey::<ml_dsa::MlDsa65>::decode(&enc_vk);

    let enc_sig = ml_dsa::EncodedSignature::<ml_dsa::MlDsa65>::try_from(sig.as_slice())
        .expect("3309 bytes is the encoded signature length");
    let rust_sig = ml_dsa::Signature::<ml_dsa::MlDsa65>::decode(&enc_sig)
        .expect("a signature the C kernel produced must be a valid FIPS 204 encoding");

    vk.verify(msg, &rust_sig).expect(
        "the C kernel's signature must verify under RustCrypto's independent FIPS 204 \
         implementation — if this fails, the roundtrip above is two matching mistakes",
    );

    // And the negative through the same independent path: a tampered message
    // must NOT verify, or the gate would pass for a verifier that accepts
    // everything.
    assert!(
        vk.verify(b"otro mensaje", &rust_sig).is_err(),
        "the independent implementation must reject the signature over a different message"
    );
}

/// 🎯 The independent Rust implementation signs → C verifies.
///
/// Uses the crate's deterministic seeded keygen and deterministic signing so
/// this direction needs no second RNG plumbing; determinism on the Rust side is
/// irrelevant to what is being proven, which is that the C VERIFIER accepts
/// standard FIPS 204 bytes it did not produce.
#[test]
fn the_c_kernel_verifies_an_independent_signature() {
    use ml_dsa::signature::Keypair;

    let seed_bytes: [u8; 32] = *b"axon 124.b interop direction two";
    let seed: ml_dsa::Seed = seed_bytes.into();
    let sk_rust = ml_dsa::SigningKey::<ml_dsa::MlDsa65>::from_seed(&seed);

    let msg = b"interoperabilidad, direccion dos";
    // Deterministic signing with the empty context — FIPS 204 "pure" mode,
    // which is exactly what the C kernel's non-`_ctx` verify expects.
    let rust_sig = sk_rust
        .expanded_key()
        .sign_deterministic(msg, b"")
        .expect("empty context always fits");

    let pk_bytes: [u8; MLDSA65_PUBLICKEY_BYTES] = sk_rust
        .verifying_key()
        .encode()
        .as_slice()
        .try_into()
        .expect("encoded verifying key is 1952 bytes");
    let sig_bytes: [u8; MLDSA65_SIGNATURE_BYTES] = rust_sig
        .encode()
        .as_slice()
        .try_into()
        .expect("encoded signature is 3309 bytes");

    assert!(
        verify(&sig_bytes, msg, &pk_bytes),
        "the C kernel must accept a standard FIPS 204 signature produced by an independent \
         implementation — a verifier that only accepts its own signer's output is a dialect, \
         not a standard"
    );
    assert!(
        !verify(&sig_bytes, b"otro mensaje", &pk_bytes),
        "and reject it over a different message"
    );
}
