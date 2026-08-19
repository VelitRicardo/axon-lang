//! v4.0.0 — **known-answer and byte-identity tests for the vendored Ed25519.**
//!
//! # Why this gate is STRONGER than ML-DSA's
//!
//! Ed25519 signing is deterministic (RFC 8032 section 5.1.6: the nonce is a hash, not
//! a random draw), which unlocks two proofs the hedged ML-DSA could not have:
//!
//! 1. **Published vectors, end to end.** RFC 8032 section 7.1 prints seed, public key
//!    and signature. We assert all three — the seed→pk derivation AND the
//!    signature bytes — against the RFC's own hex.
//! 2. **Byte-identity across implementations.** Same seed + message must
//!    produce the SAME 64 bytes from the vendored C and from `ed25519-dalek`.
//!    Not "verifies under" — *identical*.
//!
//! This closes `FIPS.KAT_ED25519`'s ask: the audit-engine control whose
//! evidence locator has said `"PENDING"` since the framework matrix was
//! written. The v4.0.0 signer work points that control here.
//!
//! `native`-only for the same reason as the other kernel KATs: without the C
//! there is nothing to exercise, and the provenance gate is the check that
//! runs everywhere.

#![cfg(feature = "native")]

use axon_csys::ed25519::{generate_seed, keypair_from_seed, sign, verify};

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// One RFC 8032 section 7.1 vector, asserted in full: seed → pk, sign → sig, verify.
fn rfc8032_case(seed_hex: &str, pk_hex: &str, msg_hex: &str, sig_hex: &str) {
    let seed: [u8; 32] = unhex(seed_hex).try_into().unwrap();
    let msg = unhex(msg_hex);

    let kp = keypair_from_seed(&seed);
    assert_eq!(
        hex(&kp.public),
        pk_hex,
        "seed→public-key derivation must match RFC 8032 — a wrong pk here means the \
         clamping or the basepoint multiply is not the RFC's"
    );

    let sig = sign(&msg, &kp);
    assert_eq!(
        hex(&sig),
        sig_hex,
        "the signature must be byte-identical to RFC 8032's — Ed25519 is deterministic, \
         so anything else is a different algorithm wearing the name"
    );

    assert!(verify(&sig, &msg, &kp.public), "and it must verify");
}

/// 🎯 RFC 8032 section 7.1 TEST 1 — the empty message.
#[test]
fn rfc8032_test_1_empty_message() {
    rfc8032_case(
        "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        "",
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
    );
}

/// 🎯 RFC 8032 section 7.1 TEST 2 — one byte, and a DIFFERENT keypair. Two vectors
/// with two keys kill a kernel that hardcodes anything key-dependent.
#[test]
fn rfc8032_test_2_one_byte() {
    rfc8032_case(
        "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
        "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        "72",
        "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    );
}

/// 🎯 RFC 8032 section 7.1 TEST 3 — two bytes, third keypair.
#[test]
fn rfc8032_test_3_two_bytes() {
    rfc8032_case(
        "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
        "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
        "af82",
        "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
    );
}

/// 🎯 Byte-identity against the independent implementation, on a fresh seed
/// from the module's own RNG and a message long enough to cross SHA-512's
/// 128-byte block boundary inside the signing hash.
#[test]
fn c_and_dalek_produce_identical_signatures() {
    use ed25519_dalek::{Signer, SigningKey};

    let seed = generate_seed().expect("module RNG available");
    let msg: Vec<u8> = (0..300u32).map(|i| (i % 251) as u8).collect();

    let kp = keypair_from_seed(&seed);
    let ours = sign(&msg, &kp);

    let dalek_sk = SigningKey::from_bytes(&seed);
    assert_eq!(
        hex(&kp.public),
        hex(dalek_sk.verifying_key().as_bytes()),
        "same seed must derive the same public key in both implementations"
    );
    let theirs = dalek_sk.sign(&msg).to_bytes();

    assert_eq!(
        hex(&ours),
        hex(&theirs),
        "Ed25519 is deterministic: same seed + message must yield the SAME bytes from the \
         vendored C and from ed25519-dalek. Divergence here is a real bug in one of them, \
         with no third possibility"
    );
}

/// The other direction: dalek signs, the vendored C verifies. Redundant with
/// byte-identity while identity holds — and the survivor that still bites if a
/// future upstream refresh ever made signatures differ legitimately (it cannot
/// under RFC 8032, which is the point of asserting both).
#[test]
fn the_c_kernel_verifies_a_dalek_signature() {
    use ed25519_dalek::{Signer, SigningKey};

    let seed = generate_seed().expect("rng");
    let dalek_sk = SigningKey::from_bytes(&seed);
    let msg = b"direccion dos, clasica";
    let sig = dalek_sk.sign(msg).to_bytes();
    let pk: [u8; 32] = *dalek_sk.verifying_key().as_bytes();

    assert!(verify(&sig, msg, &pk));
    assert!(!verify(&sig, b"otro mensaje", &pk));
}

/// Tampering on every surface, plus the wrong-length path the shim handles
/// before the FFI call.
#[test]
fn any_tampered_byte_fails_verification() {
    let seed = generate_seed().expect("rng");
    let kp = keypair_from_seed(&seed);
    let msg = b"no tocar";
    let sig = sign(msg, &kp);

    let mut bad = sig;
    bad[0] ^= 1;
    assert!(
        !verify(&bad, msg, &kp.public),
        "flipped first sig byte (R) must fail"
    );
    let mut bad = sig;
    bad[63] ^= 1;
    assert!(
        !verify(&bad, msg, &kp.public),
        "flipped last sig byte (S) must fail"
    );

    let mut bad_msg = msg.to_vec();
    bad_msg[0] ^= 1;
    assert!(
        !verify(&sig, &bad_msg, &kp.public),
        "flipped message must fail"
    );

    let mut bad_pk = kp.public;
    bad_pk[5] ^= 1;
    assert!(!verify(&sig, msg, &bad_pk), "flipped pk must fail");

    assert!(
        !verify(&sig[..63], msg, &kp.public),
        "truncated signature must fail"
    );
}
