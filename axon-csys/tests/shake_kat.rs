//! v4.0.0 — **known-answer tests for the vendored FIPS 202 kernel.**
//!
//! # Two kinds of proof, because they catch different things
//!
//! **Published vectors** prove the kernel computes *the standard* — that it is
//! SHAKE and not some other sponge that happens to be self-consistent. A drift
//! gate alone cannot tell you that: two implementations of the same wrong thing
//! agree perfectly.
//!
//! **A drift gate against an independent implementation** proves agreement
//! across arbitrary inputs, including the length and padding edges no fixed
//! vector reaches. A published vector alone cannot tell you that: SHAKE's
//! absorb/squeeze boundaries fall at 168 and 136 bytes, and a kernel can be
//! exactly right on the empty string and wrong at the second block.
//!
//! Both, or neither is worth much. This is the same posture v1.19.0 took for
//! SHA-256 and HMAC, applied to the first vendored primitive.
//!
//! # Why this matters more than an ordinary drift gate
//!
//! `fips202.c` is third-party code inside the FIPS module boundary. The
//! provenance gate proves it is byte-identical to the pinned upstream; it says
//! nothing about whether it *works* in this build — on this compiler, under
//! this `-std`, with the relaxed warning set the vendored archive uses. A file
//! can hash correctly and miscompile.

//! # Why this file is `native`-only, deliberately
//!
//! Under `--no-default-features` this target compiles to zero tests and cargo
//! reports `ok. 0 passed` — the v2.87.0 shape, where an empty run is
//! indistinguishable from a passing one. It is the right answer here and the
//! reason is worth stating so nobody mistakes it for the defect.
//!
//! `crypto.rs`'s drift gates are implementation-agnostic: SHA-256 and HMAC have
//! pure-Rust twins in this crate, so those gates test *something* either way.
//! SHAKE has no twin — the only implementation is the vendored C kernel, and
//! without the `native` feature there is nothing to exercise. A test that
//! "passed" without it would be asserting against a function that does not
//! exist.
//!
//! The provenance gate (`vendor_provenance.rs`) is the one that runs
//! unconditionally, because a hash can be checked with no compiler at all.

#![cfg(feature = "native")]

use axon_csys::shake::{sha3_256_digest, sha3_512_digest, shake128_xof, shake256_xof};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ── Published vectors ───────────────────────────────────────────────────────

/// 🎯 SHAKE-256 of the empty message — the canonical FIPS 202 vector.
///
/// The value below is the one NIST publishes for `SHAKE256("")` squeezed to 32
/// bytes, and it is also derivable from the reference implementation the drift
/// gate below uses. Two independent sources agreeing is what makes it a KAT
/// rather than a snapshot of whatever this build happened to produce.
#[test]
fn shake256_of_the_empty_message_matches_the_published_vector() {
    let mut out = [0u8; 32];
    shake256_xof(b"", &mut out);
    assert_eq!(
        hex(&out),
        "46b9dd2b0ba88d13233b3feb743eeb243fcd52ea62b81b82b50c27646ed5762f",
        "SHAKE-256 of the empty message must equal the FIPS 202 published value"
    );
}

/// SHAKE-128 of the empty message.
#[test]
fn shake128_of_the_empty_message_matches_the_published_vector() {
    let mut out = [0u8; 32];
    shake128_xof(b"", &mut out);
    assert_eq!(
        hex(&out),
        "7f9c2ba4e88f827d616045507605853ed73b8093f6efbc88eb1a6eacfa66ef26",
        "SHAKE-128 of the empty message must equal the FIPS 202 published value"
    );
}

// ── Drift gate against an independent implementation ────────────────────────

/// 🎯 The vendored C agrees with the `sha3` crate across the block boundaries.
///
/// The lengths are chosen, not arbitrary. SHAKE-128 absorbs in 168-byte blocks
/// and SHAKE-256 in 136; the padding rule fires differently at `rate - 1`,
/// `rate`, and `rate + 1`. A kernel that is right on short inputs and wrong at a
/// block edge is the realistic miscompilation, and it is invisible to a
/// single-vector test.
#[test]
fn the_vendored_kernel_agrees_with_an_independent_implementation() {
    use sha3::digest::{ExtendableOutput, Update, XofReader};

    // rate-1, rate, rate+1 for both rates, plus a couple of ordinary sizes.
    let lengths = [0usize, 1, 32, 135, 136, 137, 167, 168, 169, 200, 512];
    for len in lengths {
        let msg: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();

        for outlen in [1usize, 32, 64, 200] {
            let mut ours = vec![0u8; outlen];
            shake256_xof(&msg, &mut ours);
            let mut theirs = vec![0u8; outlen];
            let mut x = sha3::Shake256::default();
            x.update(&msg);
            x.finalize_xof().read(&mut theirs);
            assert_eq!(
                hex(&ours),
                hex(&theirs),
                "SHAKE-256 diverged at input len {len}, output len {outlen}"
            );

            let mut ours128 = vec![0u8; outlen];
            shake128_xof(&msg, &mut ours128);
            let mut theirs128 = vec![0u8; outlen];
            let mut x128 = sha3::Shake128::default();
            x128.update(&msg);
            x128.finalize_xof().read(&mut theirs128);
            assert_eq!(
                hex(&ours128),
                hex(&theirs128),
                "SHAKE-128 diverged at input len {len}, output len {outlen}"
            );
        }
    }
}

/// The fixed-length SHA-3 digests the same kernel exports.
///
/// ML-DSA does not use these, but they ship in the same translation unit, and a
/// primitive inside the cryptographic boundary that nothing verifies is the
/// shape this project has spent several cycles removing.
#[test]
fn the_sha3_digests_agree_with_an_independent_implementation() {
    use sha3::Digest;

    for len in [0usize, 1, 71, 72, 73, 135, 136, 137, 300] {
        let msg: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();

        assert_eq!(
            hex(&sha3_256_digest(&msg)),
            hex(&sha3::Sha3_256::digest(&msg)),
            "SHA3-256 diverged at input len {len}"
        );
        assert_eq!(
            hex(&sha3_512_digest(&msg)),
            hex(&sha3::Sha3_512::digest(&msg)),
            "SHA3-512 diverged at input len {len}"
        );
    }
}

/// SHAKE is an extendable-output function: a longer squeeze must EXTEND the
/// shorter one, not produce an unrelated stream.
///
/// This is the property ML-DSA depends on when it squeezes matrix entries
/// incrementally, and it is not implied by any single fixed vector.
#[test]
fn a_longer_squeeze_extends_the_shorter_one() {
    let msg = b"axon: the squeeze is a prefix";
    let mut short = [0u8; 32];
    let mut long = [0u8; 200];
    shake256_xof(msg, &mut short);
    shake256_xof(msg, &mut long);
    assert_eq!(
        hex(&short),
        hex(&long[..32]),
        "a 200-byte squeeze must begin with the same bytes a 32-byte squeeze produces — \
         otherwise the output depends on the requested length, which is not SHAKE"
    );
}
