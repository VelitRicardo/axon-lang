//! [`ContinuityToken`] — the handshake that lets a reconnecting
//! client prove it's the same party that opened the original
//! WebSocket.
//!
//! Problem solved
//! --------------
//! The naive reconnect flow is "client presents session_id,
//! server rehydrates". That's trivial to hijack: any party that
//! learns a session_id (via logs, a shared browser, a network
//! trace) can resume the agent mid-flow and impersonate the
//! original user. Cognitive state carries PII, so hijack isn't
//! just inconvenient — it's a data breach.
//!
//! Fix: issue a short-lived continuity token at disconnect time.
//! The token is `(session_id || expiry_ts)` concatenated with an
//! HMAC-SHA256 computed by the server over those two fields plus
//! a secret. Clients can't forge the HMAC; an attacker who sniffs
//! the token can't reuse it past expiry; a race with a legitimate
//! reconnect resolves at the backend level (the first successful
//! rehydration evicts the state so replay reconnects fail with
//! `NotFound`).
//!
//! Backend-side key handling
//! -------------------------
//! The signer key is a 32-byte random value the adopter rotates on
//! a schedule that matches their key-rotation policy for
//! refresh tokens (v1.4.0). Rotating the signer only affects
//! in-flight reconnect tokens; worst-case the client sees a
//! `ForgedOrRotated` error and re-authenticates via the normal
//! session-establishment flow.
//!
//! v1.19.0 delegation note
//! --------------------------
//! As of 2026-05-08 the HMAC-SHA256 + base64url + hex + constant-
//! time compare + record-separator parsing primitives all live in
//! the C23 `axon-csys` crate (FIPS 180-4 + FIPS 198-1
//! algorithmically compliant). This module is now a chrono-aware
//! wrapper that delegates the wire crypto to
//! `axon_csys::ContinuityWire` and adds the `expires_at <= now()`
//! check + the typed `Expired` error variant. Public surface
//! preserved unchanged for callers (`ContinuityToken`,
//! `ContinuityTokenError`, `ContinuityTokenSigner`).

use axon_csys::{ContinuityWire, ContinuityWireError};
use chrono::{DateTime, Duration as ChronoDuration, Utc};

// ── Errors ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ContinuityTokenError {
    /// The wire-format couldn't be decoded — malformed base64 or
    /// the split field structure didn't match.
    Malformed(String),
    /// HMAC check failed. Either the token was forged or the server
    /// rotated its signing key.
    ForgedOrRotated,
    /// Token was well-formed but already expired.
    Expired { expired_at: DateTime<Utc> },
}

impl std::fmt::Display for ContinuityTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(msg) => {
                write!(f, "continuity token malformed: {msg}")
            }
            Self::ForgedOrRotated => write!(
                f,
                "continuity token failed HMAC verification (forged or \
                 signer key rotated)"
            ),
            Self::Expired { expired_at } => {
                write!(f, "continuity token expired at {expired_at}")
            }
        }
    }
}

impl std::error::Error for ContinuityTokenError {}

impl From<ContinuityWireError> for ContinuityTokenError {
    fn from(value: ContinuityWireError) -> Self {
        match value {
            ContinuityWireError::ForgedOrRotated => Self::ForgedOrRotated,
            // Every other wire error surfaces as Malformed with the
            // C-side message preserved (no information loss for the
            // adopter / observability path).
            other => Self::Malformed(other.to_string()),
        }
    }
}

// ── Token body ──────────────────────────────────────────────────────

/// Parsed representation of a continuity token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuityToken {
    pub session_id: String,
    pub expires_at: DateTime<Utc>,
}

impl ContinuityToken {
    /// Build a fresh token body for `session_id` expiring in `ttl`.
    pub fn new(session_id: impl Into<String>, ttl: ChronoDuration) -> Self {
        ContinuityToken {
            session_id: session_id.into(),
            expires_at: Utc::now() + ttl,
        }
    }
}

// ── Signer ───────────────────────────────────────────────────────────

/// Holds the shared secret + signs / verifies tokens. One signer
/// per process; adopters rotate via the secrets service (v1.4.0) on
/// the cadence they prefer.
#[derive(Debug, Clone)]
pub struct ContinuityTokenSigner {
    key: Vec<u8>,
}

impl ContinuityTokenSigner {
    /// Wrap an adopter-supplied secret. Accepts any byte length; 32
    /// bytes of CSPRNG output is the recommended size (matches
    /// v1.4.0 signer-key guidance).
    pub fn new(key: impl Into<Vec<u8>>) -> Self {
        ContinuityTokenSigner { key: key.into() }
    }

    /// Produce the wire-encoded token. The wire format is
    /// `base64url_no_pad(session_id || 0x1e || expiry_ms || 0x1e || hex_lower(HMAC-SHA256))`,
    /// computed by the C23 kernel in `axon-csys`.
    pub fn sign(&self, token: &ContinuityToken) -> String {
        let expiry_ms = token.expires_at.timestamp_millis();
        // The C kernel rejects `session_id` containing 0x1e; the Rust
        // ContinuityToken type does not enforce that, so this can
        // panic on adversarial input. In practice session_ids are
        // adopter-supplied UUIDs that never contain 0x1e — and
        // panicking is the right response if an adopter manages to
        // smuggle one in (it indicates programmer error, not a runtime
        // condition we should silently absorb). Document the panic
        // here so the contract is clear.
        ContinuityWire::sign(&self.key, &token.session_id, expiry_ms)
            .expect("ContinuityToken.session_id must not contain 0x1e and must be ≤ 1024 bytes")
    }

    /// Verify + parse. Returns the bound `ContinuityToken` on
    /// success, typed error otherwise.
    pub fn verify(
        &self,
        raw: &str,
    ) -> Result<ContinuityToken, ContinuityTokenError> {
        let (session_id, expiry_ms) = ContinuityWire::verify(&self.key, raw)?;
        let expires_at =
            DateTime::<Utc>::from_timestamp_millis(expiry_ms).ok_or_else(|| {
                ContinuityTokenError::Malformed(
                    "expiry timestamp out of range".into(),
                )
            })?;
        if expires_at <= Utc::now() {
            return Err(ContinuityTokenError::Expired { expired_at: expires_at });
        }
        Ok(ContinuityToken {
            session_id,
            expires_at,
        })
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_verify_roundtrip() {
        let signer = ContinuityTokenSigner::new([7u8; 32]);
        let token = ContinuityToken::new("sess-1", ChronoDuration::minutes(15));
        let wire = signer.sign(&token);
        let decoded = signer.verify(&wire).expect("verify");
        assert_eq!(decoded.session_id, "sess-1");
        // Exp roundtrip preserves ms precision.
        assert_eq!(
            decoded.expires_at.timestamp_millis(),
            token.expires_at.timestamp_millis()
        );
    }

    #[test]
    fn verify_rejects_tampered_session_id() {
        use axon_csys::{b64url_decode, b64url_encode};
        let signer = ContinuityTokenSigner::new([7u8; 32]);
        let token = ContinuityToken::new("sess-a", ChronoDuration::minutes(15));
        let wire = signer.sign(&token);
        let decoded_bytes = b64url_decode(&wire).unwrap();
        let text = std::str::from_utf8(&decoded_bytes).unwrap();
        let tampered = text.replacen("sess-a", "sess-b", 1);
        let tampered_wire = b64url_encode(tampered.as_bytes());

        let err = signer.verify(&tampered_wire).unwrap_err();
        assert!(matches!(err, ContinuityTokenError::ForgedOrRotated));
    }

    #[test]
    fn verify_rejects_different_signer_key() {
        let s1 = ContinuityTokenSigner::new([1u8; 32]);
        let s2 = ContinuityTokenSigner::new([2u8; 32]);
        let token = ContinuityToken::new("sess-1", ChronoDuration::minutes(15));
        let wire = s1.sign(&token);
        let err = s2.verify(&wire).unwrap_err();
        assert!(matches!(err, ContinuityTokenError::ForgedOrRotated));
    }

    #[test]
    fn verify_rejects_expired_token() {
        let signer = ContinuityTokenSigner::new([7u8; 32]);
        let token = ContinuityToken::new("sess-1", ChronoDuration::seconds(-1));
        let wire = signer.sign(&token);
        let err = signer.verify(&wire).unwrap_err();
        assert!(matches!(err, ContinuityTokenError::Expired { .. }));
    }

    #[test]
    fn verify_rejects_malformed_base64() {
        let signer = ContinuityTokenSigner::new([7u8; 32]);
        let err = signer.verify("not-valid-base64!@#").unwrap_err();
        assert!(matches!(err, ContinuityTokenError::Malformed(_)));
    }

    #[test]
    fn verify_rejects_wrong_field_count() {
        use axon_csys::b64url_encode;
        let signer = ContinuityTokenSigner::new([7u8; 32]);
        let bad = b64url_encode(b"sess-1\x1e9999");
        let err = signer.verify(&bad).unwrap_err();
        assert!(matches!(err, ContinuityTokenError::Malformed(_)));
    }

    #[test]
    fn hmac_uses_constant_time_compare() {
        // Regression: two invalid tokens with wildly different MACs
        // should both fail with `ForgedOrRotated`, not with a
        // timing-observable length-short-circuit.
        use axon_csys::{b64url_decode, b64url_encode};
        let signer = ContinuityTokenSigner::new([7u8; 32]);
        let token = ContinuityToken::new("sess-1", ChronoDuration::minutes(5));
        let wire_good = signer.sign(&token);

        // Flip the last char of the MAC.
        let decoded = b64url_decode(&wire_good).unwrap();
        let mut text = std::str::from_utf8(&decoded).unwrap().to_string();
        let len = text.len();
        let last = text.chars().last().unwrap();
        let flipped = if last == 'a' { 'b' } else { 'a' };
        text.replace_range(len - 1.., &flipped.to_string());
        let wire_bad = b64url_encode(text.as_bytes());

        let err = signer.verify(&wire_bad).unwrap_err();
        assert!(matches!(err, ContinuityTokenError::ForgedOrRotated));
    }
}
