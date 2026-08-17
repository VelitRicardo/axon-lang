//! AXON Runtime — Cryptographic Provenance (§ESK Fase 6.2)
//!
//! Direct port of `axon/runtime/esk/provenance.py`.
//!
//! Signed ΛD envelopes + Merkle-hash audit chain. HMAC-SHA256 is the
//! always-available baseline; the hybrid Ed25519 ‖ ML-DSA-65 signer landed
//! in §Fase 124.b as `esk::hybrid_signer::HybridSigner` — plain text, not an
//! intra-doc link, because that module is gated on `csys-native` and a link to
//! it breaks rustdoc under the default features. Canonical
//! serialization guarantees the Python golden and Rust output produce
//! byte-identical `data_hash` and `payload_hash` values.

#![allow(dead_code)]

use std::fmt::Write as _;

use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// Shared signer protocol: algorithm name + sign + verify.
pub trait Signer: Send {
    fn algorithm(&self) -> &str;
    fn sign(&self, message: &[u8]) -> Vec<u8>;
    fn verify(&self, message: &[u8], signature: &[u8]) -> bool;
}

// ═══════════════════════════════════════════════════════════════════
//  HMAC-SHA256 baseline
// ═══════════════════════════════════════════════════════════════════

pub struct HmacSigner {
    key: Vec<u8>,
}

impl HmacSigner {
    pub fn new(key: Vec<u8>) -> Self {
        HmacSigner { key }
    }

    /// Cryptographically random 256-bit key via the system RNG.
    pub fn random() -> Self {
        let mut key = vec![0u8; 32];
        rand::rng().fill_bytes(&mut key);
        HmacSigner { key }
    }
}

impl Signer for HmacSigner {
    fn algorithm(&self) -> &str { "HMAC-SHA256" }

    fn sign(&self, message: &[u8]) -> Vec<u8> {
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("HMAC key any length");
        mac.update(message);
        mac.finalize().into_bytes().to_vec()
    }

    fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("HMAC key any length");
        mac.update(message);
        mac.verify_slice(signature).is_ok()
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Canonical serialization — must match Python `canonical_bytes`
// ═══════════════════════════════════════════════════════════════════

/// Deterministic JSON encoding: sorted keys, no whitespace, UTF-8.
///
/// Python uses `json.dumps(payload, sort_keys=True, separators=(",", ":"))`.
/// To match byte-identically, we walk the `Value` tree and build the string
/// manually — `serde_json` doesn't directly support key-sorted compact
/// output with exactly the same number formatting as Python.
pub fn canonical_bytes(payload: &Value) -> Vec<u8> {
    let mut out = String::new();
    write_canonical(&mut out, payload);
    out.into_bytes()
}

fn write_canonical(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            // Use serde_json's number formatting (matches Python on integers
            // and plain floats).
            out.push_str(&n.to_string());
        }
        Value::String(s) => {
            // Delegate string escaping to serde_json to match JSON spec.
            let encoded = serde_json::to_string(s).expect("string encode");
            out.push_str(&encoded);
        }
        Value::Array(arr) => {
            out.push('[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 { out.push(','); }
                write_canonical(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 { out.push(','); }
                let encoded_key = serde_json::to_string(k).expect("key encode");
                out.push_str(&encoded_key);
                out.push(':');
                write_canonical(out, &map[*k]);
            }
            out.push('}');
        }
    }
}

/// SHA-256 hex digest of the canonical serialization.
pub fn content_hash(payload: &Value) -> String {
    let mut h = Sha256::new();
    h.update(canonical_bytes(payload));
    to_hex(&h.finalize())
}

pub fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

pub fn from_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&hex[i..i + 2], 16).ok()?;
        out.push(byte);
    }
    Some(out)
}

// ═══════════════════════════════════════════════════════════════════
//  Signed entry + provenance chain
// ═══════════════════════════════════════════════════════════════════

/// One tamper-evident entry in a provenance chain.
#[derive(Debug, Clone, Serialize)]
pub struct SignedEntry {
    pub index: usize,
    pub previous_hash: String,
    pub payload_hash: String,
    /// Serde output uses `"signature"` to match Python's `to_dict`.
    #[serde(rename = "signature")]
    pub signature_hex: String,
    pub algorithm: String,
    pub chain_hash: String,
}

pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

pub struct ProvenanceChain<S: Signer> {
    pub signer: S,
    entries: Vec<SignedEntry>,
}

impl<S: Signer> ProvenanceChain<S> {
    pub fn new(signer: S) -> Self {
        ProvenanceChain { signer, entries: Vec::new() }
    }

    pub fn head(&self) -> String {
        self.entries
            .last()
            .map(|e| e.chain_hash.clone())
            .unwrap_or_else(|| GENESIS_HASH.to_string())
    }

    pub fn append(&mut self, payload: &Value) -> SignedEntry {
        let payload_bytes = canonical_bytes(payload);
        let mut hp = Sha256::new();
        hp.update(&payload_bytes);
        let payload_h = to_hex(&hp.finalize());
        let prev = self.head();
        let message = format!("{prev}|{payload_h}");
        let signature = self.signer.sign(message.as_bytes());
        let signature_hex = to_hex(&signature);
        let mut hc = Sha256::new();
        hc.update(format!("{prev}|{payload_h}|{signature_hex}").as_bytes());
        let chain_h = to_hex(&hc.finalize());
        let entry = SignedEntry {
            index: self.entries.len(),
            previous_hash: prev,
            payload_hash: payload_h,
            signature_hex,
            algorithm: self.signer.algorithm().into(),
            chain_hash: chain_h,
        };
        self.entries.push(entry.clone());
        entry
    }

    pub fn entries(&self) -> &[SignedEntry] { &self.entries }

    /// Re-derive chain hashes from supplied payloads and verify each
    /// signature + linkage. `payloads` must be in append order.
    pub fn verify(&self, payloads: &[Value]) -> bool {
        let mut prev = GENESIS_HASH.to_string();
        for (entry, payload) in self.entries.iter().zip(payloads.iter()) {
            let mut hp = Sha256::new();
            hp.update(canonical_bytes(payload));
            let payload_h = to_hex(&hp.finalize());
            if payload_h != entry.payload_hash {
                return false;
            }
            if entry.previous_hash != prev {
                return false;
            }
            let message = format!("{prev}|{payload_h}");
            let Some(sig) = from_hex(&entry.signature_hex) else { return false; };
            if !self.signer.verify(message.as_bytes(), &sig) {
                return false;
            }
            let mut hc = Sha256::new();
            hc.update(format!("{prev}|{payload_h}|{}", entry.signature_hex).as_bytes());
            if to_hex(&hc.finalize()) != entry.chain_hash {
                return false;
            }
            prev = entry.chain_hash.clone();
        }
        true
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Signed envelope helper
// ═══════════════════════════════════════════════════════════════════

/// A cryptographically signed ΛD envelope, parallel to `LambdaEnvelope`
/// but with a provenance signature bound to (c, τ, ρ, δ, data_hash).
#[derive(Debug, Clone, Serialize)]
pub struct SignedEnvelope {
    pub c: f64,
    pub tau: String,
    pub rho: String,
    pub delta: String,
    pub data_hash: String,
    #[serde(rename = "signature")]
    pub signature_hex: String,
    pub algorithm: String,
}

pub fn sign_envelope(
    c: f64,
    tau: &str,
    rho: &str,
    delta: &str,
    data: &Value,
    signer: &dyn Signer,
) -> SignedEnvelope {
    let data_hash = content_hash(data);
    let mut m = Map::new();
    m.insert("c".into(), serde_json::Value::from(c));
    m.insert("tau".into(), tau.into());
    m.insert("rho".into(), rho.into());
    m.insert("delta".into(), delta.into());
    m.insert("data_hash".into(), data_hash.clone().into());
    let message = canonical_bytes(&Value::Object(m));
    let signature = signer.sign(&message);
    SignedEnvelope {
        c,
        tau: tau.into(),
        rho: rho.into(),
        delta: delta.into(),
        data_hash,
        signature_hex: to_hex(&signature),
        algorithm: signer.algorithm().into(),
    }
}

pub fn verify_envelope(
    envelope: &SignedEnvelope,
    data: &Value,
    signer: &dyn Signer,
) -> bool {
    let data_hash = content_hash(data);
    if data_hash != envelope.data_hash {
        return false;
    }
    let mut m = Map::new();
    m.insert("c".into(), serde_json::Value::from(envelope.c));
    m.insert("tau".into(), envelope.tau.clone().into());
    m.insert("rho".into(), envelope.rho.clone().into());
    m.insert("delta".into(), envelope.delta.clone().into());
    m.insert("data_hash".into(), data_hash.into());
    let message = canonical_bytes(&Value::Object(m));
    let Some(signature) = from_hex(&envelope.signature_hex) else { return false; };
    signer.verify(&message, &signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_bytes_sorts_keys() {
        let v = json!({"b": 1, "a": 2, "c": 3});
        let bytes = canonical_bytes(&v);
        assert_eq!(std::str::from_utf8(&bytes).unwrap(), r#"{"a":2,"b":1,"c":3}"#);
    }

    #[test]
    fn canonical_bytes_compact_no_whitespace() {
        let v = json!({"x": [1, 2, 3], "y": "hi"});
        let s = String::from_utf8(canonical_bytes(&v)).unwrap();
        assert!(!s.contains(' '));
        assert!(!s.contains('\n'));
    }

    #[test]
    fn canonical_bytes_nested_objects_sorted() {
        let v = json!({"outer": {"z": 1, "a": 2}, "alpha": [{"k": 1, "j": 2}]});
        let s = String::from_utf8(canonical_bytes(&v)).unwrap();
        // Nested object keys sorted too.
        assert!(s.contains(r#""outer":{"a":2,"z":1}"#));
        assert!(s.contains(r#"{"j":2,"k":1}"#));
    }

    #[test]
    fn content_hash_deterministic() {
        let v = json!({"x": 1, "y": "z"});
        let a = content_hash(&v);
        let b = content_hash(&v);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn content_hash_matches_python_reference_for_known_payload() {
        // Python: hashlib.sha256(b'{"a":1,"b":[2,3]}').hexdigest() →
        // 6b71d33f12ceb2ff12e8a6a9afb95c0fc3d1eb4b78625f7e5faa3bfde5c3ee3d
        // (this is the literal canonical encoding we're committing to)
        let expected = {
            let bytes = r#"{"a":1,"b":[2,3]}"#.as_bytes();
            let mut h = Sha256::new();
            h.update(bytes);
            to_hex(&h.finalize())
        };
        let v = json!({"a": 1, "b": [2, 3]});
        assert_eq!(content_hash(&v), expected);
    }

    #[test]
    fn hmac_sign_then_verify_roundtrip() {
        let signer = HmacSigner::new(vec![0u8; 32]);
        let msg = b"hello world";
        let sig = signer.sign(msg);
        assert!(signer.verify(msg, &sig));
        assert!(!signer.verify(b"tampered", &sig));
    }

    #[test]
    fn hmac_random_keys_differ() {
        let a = HmacSigner::random();
        let b = HmacSigner::random();
        let msg = b"axon";
        assert_ne!(a.sign(msg), b.sign(msg));
    }

    #[test]
    fn hex_roundtrip() {
        let bytes = vec![0xde, 0xad, 0xbe, 0xef, 0x00, 0xff];
        let hex = to_hex(&bytes);
        assert_eq!(hex, "deadbeef00ff");
        assert_eq!(from_hex(&hex).unwrap(), bytes);
    }

    #[test]
    fn chain_head_is_genesis_when_empty() {
        let c = ProvenanceChain::new(HmacSigner::new(vec![1u8; 32]));
        assert_eq!(c.head(), GENESIS_HASH);
    }

    #[test]
    fn chain_append_links_payloads() {
        let mut c = ProvenanceChain::new(HmacSigner::new(vec![7u8; 32]));
        let e1 = c.append(&json!({"n": 1}));
        let e2 = c.append(&json!({"n": 2}));
        assert_eq!(e1.previous_hash, GENESIS_HASH);
        assert_eq!(e2.previous_hash, e1.chain_hash);
        assert_eq!(e1.index, 0);
        assert_eq!(e2.index, 1);
    }

    #[test]
    fn chain_verify_succeeds_on_unmodified_payloads() {
        let mut c = ProvenanceChain::new(HmacSigner::new(vec![42u8; 32]));
        let p1 = json!({"n": 1, "note": "a"});
        let p2 = json!({"n": 2, "note": "b"});
        c.append(&p1);
        c.append(&p2);
        assert!(c.verify(&[p1, p2]));
    }

    #[test]
    fn chain_verify_detects_tampered_payload() {
        let mut c = ProvenanceChain::new(HmacSigner::new(vec![42u8; 32]));
        let p1 = json!({"n": 1});
        c.append(&p1);
        // Feed a different payload on verify → must fail.
        assert!(!c.verify(&[json!({"n": 2})]));
    }

    #[test]
    fn signed_envelope_roundtrip_detects_data_tamper() {
        let signer = HmacSigner::new(vec![9u8; 32]);
        let data = json!({"x": 1, "y": "ok"});
        let env = sign_envelope(0.95, "T", "h", "observed", &data, &signer);
        assert!(verify_envelope(&env, &data, &signer));
        assert!(!verify_envelope(&env, &json!({"x": 1, "y": "bad"}), &signer));
    }

    #[test]
    fn signed_envelope_wrong_key_fails_verify() {
        let signer_a = HmacSigner::new(vec![1u8; 32]);
        let signer_b = HmacSigner::new(vec![2u8; 32]);
        let data = json!({"x": 1});
        let env = sign_envelope(1.0, "T", "h", "axiomatic", &data, &signer_a);
        assert!(!verify_envelope(&env, &data, &signer_b));
    }
}
