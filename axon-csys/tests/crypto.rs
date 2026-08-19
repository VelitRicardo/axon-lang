//! v1.19.0 — crypto kernel test suite.
//!
//! Coverage:
//!   1. SHA-256 NIST short-message vectors (FIPS 180-4 reference).
//!   2. SHA-256 streaming-equivalence vs one-shot.
//!   3. HMAC-SHA256 RFC 4231 reference test cases.
//!   4. HMAC-SHA256 streaming-equivalence vs one-shot.
//!   5. Constant-time compare correctness + smoke timing.
//!   6. Hex round-trip + case-insensitive decode + invalid-input rejection.
//! 7. Base64url-no-pad RFC 4648 section 10 reference vectors + round-trip.
//!   8. Continuity wire sign / verify round-trip.
//!   9. Continuity wire tamper rejection (session_id, expiry, MAC).
//!  10. Continuity wire wrong-key rejection.
//!  11. Cross-stack drift gate against sha2 / hmac / base64 / subtle crates.
//!
//! The drift gate is the load-bearing test: every other test could
//! pass while a subtle byte-order bug in our SHA-256 produced
//! algorithmically-different (but consistent) output. Cross-validating
//! against the Rust ecosystem's canonical impls (kept as
//! [dev-dependencies] only) catches that immediately.

use axon_csys::crypto::{
    self, b64url_decode, b64url_encode, ct_eq, hex_decode, hex_encode, hmac_sha256, sha256,
    ContinuityWire, ContinuityWireError, HmacSha256, Sha256,
};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Digest;
use subtle::ConstantTimeEq;

type RefHmacSha256 = Hmac<sha2::Sha256>;

// ──────────────────────────────────────────────────────────────────────
// 1. SHA-256 — FIPS 180-4 reference vectors
// ──────────────────────────────────────────────────────────────────────

#[test]
fn sha256_empty_message() {
    // FIPS 180-4 Appendix B / NIST CAVS: SHA-256 of empty input.
    let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    assert_eq!(hex_encode(&sha256(b"")), expected);
}

#[test]
fn sha256_abc() {
    // FIPS 180-4 Appendix B.1: "abc" → 0xba7816bf...
    let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(hex_encode(&sha256(b"abc")), expected);
}

#[test]
fn sha256_long_message() {
    // FIPS 180-4 Appendix B.2: "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq" (56 bytes)
    let m = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    let expected = "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1";
    assert_eq!(hex_encode(&sha256(m)), expected);
}

#[test]
fn sha256_million_a() {
    // FIPS 180-4 Appendix B.3: 1,000,000 'a' bytes.
    let m = vec![b'a'; 1_000_000];
    let expected = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";
    assert_eq!(hex_encode(&sha256(&m)), expected);
}

#[test]
fn sha256_block_boundary() {
    // 64 bytes — exactly one SHA-256 block. Stresses the padding
    // logic that has to add a full extra block (since the 0x80 byte
    // pushes past 56).
    let m = vec![b'x'; 64];
    let want: [u8; 32] = sha2::Sha256::digest(&m).into();
    assert_eq!(sha256(&m), want);
}

#[test]
fn sha256_padding_corner_55_56_57_bytes() {
    // The padding switches from "fits in same block" to "needs an
    // extra block" between 55 and 56 input bytes (55 leaves room for
    // the 0x80 + 8-byte length; 56 does not). Verify all three
    // adjacent lengths against the reference impl.
    for len in [55usize, 56, 57] {
        let m = vec![b'q'; len];
        let want: [u8; 32] = sha2::Sha256::digest(&m).into();
        assert_eq!(sha256(&m), want, "drift at len={len}");
    }
}

// ──────────────────────────────────────────────────────────────────────
// 2. SHA-256 streaming
// ──────────────────────────────────────────────────────────────────────

#[test]
fn sha256_streaming_equals_one_shot() {
    let m = b"The quick brown fox jumps over the lazy dog";
    let one_shot = sha256(m);
    let mut stream = Sha256::new();
    stream.update(b"The quick brown fox ");
    stream.update(b"jumps over ");
    stream.update(b"the lazy dog");
    assert_eq!(stream.finalize(), one_shot);
}

#[test]
fn sha256_streaming_with_block_cross() {
    // Update sequence whose chunk boundaries straddle SHA-256 block
    // boundaries (64 bytes). Verifies the buffer-fill-then-flush
    // path in `update`.
    let mut stream = Sha256::new();
    stream.update(&[b'a'; 30]);
    stream.update(&[b'b'; 50]); // pushes across the block boundary
    stream.update(&[b'c'; 100]);
    let combined: Vec<u8> = std::iter::repeat_n(b'a', 30)
        .chain(std::iter::repeat_n(b'b', 50))
        .chain(std::iter::repeat_n(b'c', 100))
        .collect();
    assert_eq!(stream.finalize(), sha256(&combined));
}

// ──────────────────────────────────────────────────────────────────────
// 3. HMAC-SHA256 — RFC 4231 reference vectors
// ──────────────────────────────────────────────────────────────────────

#[test]
fn hmac_rfc4231_test_case_1() {
    // RFC 4231 section 4.2 Test Case 1: 20-byte key, 8-byte data.
    let key = [0x0bu8; 20];
    let data = b"Hi There";
    let expected = "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7";
    assert_eq!(hex_encode(&hmac_sha256(&key, data)), expected);
}

#[test]
fn hmac_rfc4231_test_case_2() {
    // RFC 4231 section 4.3 Test Case 2: 4-byte key, 28-byte data.
    let key = b"Jefe";
    let data = b"what do ya want for nothing?";
    let expected = "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843";
    assert_eq!(hex_encode(&hmac_sha256(key, data)), expected);
}

#[test]
fn hmac_rfc4231_test_case_3() {
    // RFC 4231 section 4.4 Test Case 3: 20-byte key (all 0xaa), 50-byte data (all 0xdd).
    let key = [0xaau8; 20];
    let data = [0xddu8; 50];
    let expected = "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe";
    assert_eq!(hex_encode(&hmac_sha256(&key, &data)), expected);
}

#[test]
fn hmac_rfc4231_test_case_4() {
    // RFC 4231 section 4.5 Test Case 4: 25-byte key (0x01..0x19), 50-byte data.
    let key: Vec<u8> = (1u8..=25u8).collect();
    let data = [0xcdu8; 50];
    let expected = "82558a389a443c0ea4cc819899f2083a85f0faa3e578f8077a2e3ff46729665b";
    assert_eq!(hex_encode(&hmac_sha256(&key, &data)), expected);
}

#[test]
fn hmac_rfc4231_test_case_6_long_key() {
    // RFC 4231 section 4.7 Test Case 6: key longer than 64 bytes (131 0xaa
    // bytes); the hmac construction pre-hashes such keys per FIPS 198-1
    // section 5. Expected MAC verifies the long-key code path.
    let key = [0xaau8; 131];
    let data = b"Test Using Larger Than Block-Size Key - Hash Key First";
    let expected = "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54";
    assert_eq!(hex_encode(&hmac_sha256(&key, data)), expected);
}

// ──────────────────────────────────────────────────────────────────────
// 4. HMAC-SHA256 streaming
// ──────────────────────────────────────────────────────────────────────

#[test]
fn hmac_streaming_equals_one_shot() {
    let key = b"a-key-of-medium-length-32-bytes!";
    let data = b"streaming hmac update sequence test";
    let one_shot = hmac_sha256(key, data);
    let mut stream = HmacSha256::new(key);
    stream.update(b"streaming hmac ");
    stream.update(b"update sequence ");
    stream.update(b"test");
    assert_eq!(stream.finalize(), one_shot);
}

// ──────────────────────────────────────────────────────────────────────
// 5. Constant-time equality
// ──────────────────────────────────────────────────────────────────────

#[test]
fn ct_eq_basic() {
    assert!(ct_eq(b"hello", b"hello"));
    assert!(!ct_eq(b"hello", b"hellx"));
    assert!(!ct_eq(b"hello", b"hellow"));
    assert!(ct_eq(b"", b""));
}

#[test]
fn ct_eq_first_byte_differs() {
    // Regression smoke for early-exit bugs: a difference in the
    // first byte must yield false the same way as a difference in
    // the last byte. We do not assert constant time directly
    // (timing measurement is brittle in CI) but verify correctness
    // across many positions.
    let a = b"0123456789abcdef0123456789abcdef";
    for i in 0..a.len() {
        let mut b = *a;
        b[i] ^= 0xFF;
        assert!(!ct_eq(a, &b), "expected mismatch at pos {i}");
    }
}

#[test]
fn ct_eq_drift_gate_vs_subtle() {
    // Cross-validate against subtle::ConstantTimeEq.
    let pairs: &[(&[u8], &[u8])] = &[
        (b"foo", b"foo"),
        (b"foo", b"bar"),
        (b"", b""),
        (&[0u8; 32], &[0u8; 32]),
        (&[0u8; 32], &[1u8; 32]),
    ];
    for (a, b) in pairs {
        let ours = ct_eq(a, b);
        let theirs = bool::from(a.ct_eq(b));
        assert_eq!(ours, theirs, "drift on {a:?} vs {b:?}");
    }
}

// ──────────────────────────────────────────────────────────────────────
// 6. Hex codec
// ──────────────────────────────────────────────────────────────────────

#[test]
fn hex_round_trip() {
    for input in [
        &b""[..],
        &b"\x00"[..],
        &b"\xff"[..],
        &b"hello"[..],
        &[0xDEu8, 0xAD, 0xBE, 0xEF][..],
    ] {
        let encoded = hex_encode(input);
        assert_eq!(encoded.len(), input.len() * 2);
        let decoded = hex_decode(&encoded).expect("hex decode");
        assert_eq!(&decoded[..], input);
    }
}

#[test]
fn hex_decode_accepts_uppercase() {
    let encoded = "DEADBEEF";
    assert_eq!(hex_decode(encoded).unwrap(), [0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn hex_decode_rejects_odd_length() {
    assert_eq!(hex_decode("a"), None);
    assert_eq!(hex_decode("abc"), None);
}

#[test]
fn hex_decode_rejects_bad_chars() {
    assert_eq!(hex_decode("zz"), None);
    assert_eq!(hex_decode("a!"), None);
    assert_eq!(hex_decode("0g"), None);
}

// ──────────────────────────────────────────────────────────────────────
// 7. Base64url-no-pad codec
// ──────────────────────────────────────────────────────────────────────

#[test]
fn b64url_round_trip() {
    for input in [
        &b""[..],
        &b"f"[..],
        &b"fo"[..],
        &b"foo"[..],
        &b"foob"[..],
        &b"fooba"[..],
        &b"foobar"[..],
    ] {
        let encoded = b64url_encode(input);
        let decoded = b64url_decode(&encoded).expect("b64 decode");
        assert_eq!(&decoded[..], input);
    }
}

#[test]
fn b64url_no_pad_drift_gate() {
    // The byte sequence 0xFF 0xFC contains both special characters
    // (`-` and `_`) of the URL-safe alphabet. Cross-validate.
    let inputs: &[&[u8]] = &[
        b"",
        b"\x00",
        b"\xff\xfc",
        b"hello world",
        b"\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff",
        &[0u8; 64],
    ];
    for input in inputs {
        let ours = b64url_encode(input);
        let theirs = URL_SAFE_NO_PAD.encode(input);
        assert_eq!(ours, theirs, "encode drift on {input:?}");
        let round = b64url_decode(&ours).unwrap();
        assert_eq!(&round[..], *input);
    }
}

#[test]
fn b64url_decode_rejects_invalid_alphabet() {
    // `=` is not in the no-pad alphabet.
    assert_eq!(b64url_decode("aGVsbG8="), None);
    // `+` is the standard base64 alphabet, not URL-safe.
    assert_eq!(b64url_decode("aGVs+G8"), None);
}

#[test]
fn b64url_decode_rejects_4k_plus_1_length() {
    // 1 / 5 / 9 / 13 chars → can't represent a whole byte count.
    assert_eq!(b64url_decode("a"), None);
    assert_eq!(b64url_decode("aGVsb"), None);
}

// ──────────────────────────────────────────────────────────────────────
// 8. Continuity wire — sign / verify round-trip
// ──────────────────────────────────────────────────────────────────────

#[test]
fn continuity_round_trip() {
    let key = [7u8; 32];
    let wire = ContinuityWire::sign(&key, "sess-1", 1_700_000_000_000).expect("sign");
    let (sid, ms) = ContinuityWire::verify(&key, &wire).expect("verify");
    assert_eq!(sid, "sess-1");
    assert_eq!(ms, 1_700_000_000_000);
}

#[test]
fn continuity_round_trip_with_unicode_session_id() {
    let key = b"my-unicode-key-32-bytes-padded!!";
    let sid = "sesión-完了-1";
    let wire = ContinuityWire::sign(key, sid, 0).expect("sign");
    let (recovered, ms) = ContinuityWire::verify(key, &wire).expect("verify");
    assert_eq!(recovered, sid);
    assert_eq!(ms, 0);
}

#[test]
fn continuity_round_trip_with_negative_expiry() {
    // Expired-by-design tokens still ROUND-TRIP at the wire level —
    // expiry checking is a separate concern (axon-rs::pem layer).
    let key = [0u8; 32];
    let wire = ContinuityWire::sign(&key, "sess", -1).expect("sign");
    let (_sid, ms) = ContinuityWire::verify(&key, &wire).expect("verify");
    assert_eq!(ms, -1);
}

#[test]
fn continuity_round_trip_with_extreme_expiry() {
    let key = [0u8; 32];
    let wire = ContinuityWire::sign(&key, "sess", i64::MAX).unwrap();
    let (_sid, ms) = ContinuityWire::verify(&key, &wire).unwrap();
    assert_eq!(ms, i64::MAX);
    let wire2 = ContinuityWire::sign(&key, "sess", i64::MIN).unwrap();
    let (_sid2, ms2) = ContinuityWire::verify(&key, &wire2).unwrap();
    assert_eq!(ms2, i64::MIN);
}

// ──────────────────────────────────────────────────────────────────────
// 9. Continuity wire — tamper rejection
// ──────────────────────────────────────────────────────────────────────

#[test]
fn continuity_rejects_tampered_session_id() {
    let key = [7u8; 32];
    let wire = ContinuityWire::sign(&key, "sess-a", 1_700_000_000_000).unwrap();
    // Decode, mutate the session id, re-encode without refreshing the MAC.
    let decoded = b64url_decode(&wire).unwrap();
    let text = std::str::from_utf8(&decoded).unwrap();
    let tampered = text.replacen("sess-a", "sess-b", 1);
    let tampered_wire = b64url_encode(tampered.as_bytes());
    let err = ContinuityWire::verify(&key, &tampered_wire).unwrap_err();
    assert_eq!(err, ContinuityWireError::ForgedOrRotated);
}

#[test]
fn continuity_rejects_tampered_expiry() {
    let key = [7u8; 32];
    let wire = ContinuityWire::sign(&key, "sess", 1_000_000_000_000).unwrap();
    let decoded = b64url_decode(&wire).unwrap();
    let text = std::str::from_utf8(&decoded).unwrap();
    let tampered = text.replacen("1000000000000", "9000000000000", 1);
    let tampered_wire = b64url_encode(tampered.as_bytes());
    let err = ContinuityWire::verify(&key, &tampered_wire).unwrap_err();
    assert_eq!(err, ContinuityWireError::ForgedOrRotated);
}

#[test]
fn continuity_rejects_tampered_mac() {
    let key = [7u8; 32];
    let wire = ContinuityWire::sign(&key, "sess", 1_000_000_000_000).unwrap();
    let mut decoded = b64url_decode(&wire).unwrap();
    // Flip the last byte (last hex char of MAC).
    let last = decoded.len() - 1;
    decoded[last] = if decoded[last] == b'a' { b'b' } else { b'a' };
    let tampered_wire = b64url_encode(&decoded);
    let err = ContinuityWire::verify(&key, &tampered_wire).unwrap_err();
    assert_eq!(err, ContinuityWireError::ForgedOrRotated);
}

#[test]
fn continuity_rejects_wrong_key() {
    let signer_a = [1u8; 32];
    let signer_b = [2u8; 32];
    let wire = ContinuityWire::sign(&signer_a, "sess", 0).unwrap();
    let err = ContinuityWire::verify(&signer_b, &wire).unwrap_err();
    assert_eq!(err, ContinuityWireError::ForgedOrRotated);
}

#[test]
fn continuity_rejects_malformed_base64() {
    let key = [0u8; 32];
    let err = ContinuityWire::verify(&key, "not-valid-base64!@#").unwrap_err();
    assert_eq!(err, ContinuityWireError::BadBase64);
}

#[test]
fn continuity_rejects_wrong_field_count() {
    let key = [0u8; 32];
    // Only 2 fields (one 0x1e separator).
    let bad = b64url_encode(b"sess\x1e1234567890");
    let err = ContinuityWire::verify(&key, &bad).unwrap_err();
    assert_eq!(err, ContinuityWireError::BadFieldCount);
}

#[test]
fn continuity_rejects_bad_mac_length() {
    let key = [0u8; 32];
    // 3 fields but MAC is 62 chars (not 64) — short by one hex pair.
    let mut bad_text: Vec<u8> = Vec::new();
    bad_text.extend_from_slice(b"sess\x1e1234567890\x1e");
    bad_text.extend(std::iter::repeat_n(b'a', 62));
    assert_eq!(bad_text.len(), 4 + 1 + 10 + 1 + 62);
    let bad = b64url_encode(&bad_text);
    let err = ContinuityWire::verify(&key, &bad).unwrap_err();
    assert_eq!(err, ContinuityWireError::BadHex);
}

#[test]
fn continuity_rejects_session_id_with_separator() {
    let key = [0u8; 32];
    let err = ContinuityWire::sign(&key, "bad\x1eid", 0).unwrap_err();
    assert_eq!(err, ContinuityWireError::PayloadTooLarge);
}

#[test]
fn continuity_rejects_oversized_session_id() {
    let key = [0u8; 32];
    let oversized = "a".repeat(2048);
    let err = ContinuityWire::sign(&key, &oversized, 0).unwrap_err();
    assert_eq!(err, ContinuityWireError::PayloadTooLarge);
}

// ──────────────────────────────────────────────────────────────────────
// 10. Cross-stack drift gate
// ──────────────────────────────────────────────────────────────────────

#[test]
fn sha256_drift_gate_vs_sha2_crate() {
    let inputs: &[&[u8]] = &[
        b"",
        b"a",
        b"abc",
        b"hello world",
        &[0u8; 100],
        &[0xFFu8; 1024],
    ];
    for input in inputs {
        let ours = sha256(input);
        let theirs: [u8; 32] = sha2::Sha256::digest(input).into();
        assert_eq!(ours, theirs, "sha256 drift on len={}", input.len());
    }
}

#[test]
fn hmac_drift_gate_vs_hmac_crate() {
    let cases: &[(&[u8], &[u8])] = &[
        (b"", b""),
        (b"k", b""),
        (b"key", b"data"),
        (&[0u8; 64], &[0u8; 64]),
        // Long key (>64 bytes) exercises the SHA256-pre-hash branch.
        (&[0xAAu8; 200], b"long-key-data"),
    ];
    for (key, data) in cases {
        let ours = hmac_sha256(key, data);
        let mut reference = RefHmacSha256::new_from_slice(key).expect("hmac");
        reference.update(data);
        let theirs: [u8; 32] = reference.finalize().into_bytes().into();
        assert_eq!(
            ours,
            theirs,
            "hmac drift on key_len={} data_len={}",
            key.len(),
            data.len()
        );
    }
}

#[test]
fn continuity_drift_gate_full_round_trip() {
    // Sign with the C kernel; verify by decomposing the wire manually
    // using the Rust ecosystem reference impls — this catches any
    // cross-stack divergence in the wire format itself.
    let key = [0xABu8; 32];
    let session_id = "drift-gate-session-1";
    let expiry_ms: i64 = 1_700_000_000_000;
    let wire = ContinuityWire::sign(&key, session_id, expiry_ms).unwrap();

    // Manual decode using the reference base64 + sha2/hmac impls.
    let decoded = URL_SAFE_NO_PAD.decode(&wire).unwrap();
    let text = std::str::from_utf8(&decoded).unwrap();
    let parts: Vec<&str> = text.split('\x1e').collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], session_id);
    assert_eq!(parts[1], expiry_ms.to_string());
    let body = format!("{session_id}\x1e{expiry_ms}");
    let mut reference = RefHmacSha256::new_from_slice(&key).unwrap();
    reference.update(body.as_bytes());
    let expected_mac: [u8; 32] = reference.finalize().into_bytes().into();
    let expected_hex = hex::encode_lower(&expected_mac);
    assert_eq!(parts[2], expected_hex);
}

// Lightweight inline hex impl — avoids adding the `hex` crate just
// for one drift-gate helper.
mod hex {
    pub fn encode_lower(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            out.push_str(&format!("{b:02x}"));
        }
        out
    }
}

// ──────────────────────────────────────────────────────────────────────
// 11. Re-exported lib surface smoke
// ──────────────────────────────────────────────────────────────────────

#[test]
fn crypto_surface_reachable_through_top_level_use() {
    // The top-level `pub use crypto::*` exports a small set of names —
    // make sure the canonical entry points are reachable so adopters
    // don't have to know about the inner module path.
    let _: [u8; 32] = axon_csys::sha256(b"smoke");
    let _: [u8; 32] = axon_csys::hmac_sha256(b"k", b"d");
    let _: bool = axon_csys::ct_eq(b"a", b"a");
    let _: String = axon_csys::hex_encode(b"x");
    let _: Option<Vec<u8>> = axon_csys::hex_decode("78");
    let _: String = axon_csys::b64url_encode(b"x");
    let _: Option<Vec<u8>> = axon_csys::b64url_decode("eA");
    let _: Result<String, _> = axon_csys::ContinuityWire::sign(b"k", "s", 0);
    let _ = crypto::SHA256_DIGEST_SIZE;
}
