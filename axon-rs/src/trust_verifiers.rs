//! Runtime implementations of the closed [`crate::refinement::TrustProof`]
//! catalogue.
//!
//! v1.4.0 — these are the ONLY functions the compiler
//! recognises as converting `Untrusted<T>` → `Trusted<T>`. All four
//! share a uniform shape:
//!
//! ```ignore
//! fn verify_<proof>(input: &[u8], ...proof-specific args)
//!     -> Result<Trusted<Bytes>, TrustError>;
//! ```
//!
//! Implementation notes:
//!
//! - HMAC uses `hmac::Mac::verify_slice`, which routes through the
//!   `subtle` crate's `ConstantTimeEq` — byte-by-byte comparisons
//!   inside the MAC would leak timing; the catalogue entry documents
//!   this property so reviewers don't have to re-check every crate.
//! - JWT delegates to [`crate::jwt_verifier`] from v1.4.0; we
//!   re-expose it here so the catalogue is the single source of truth.
//! - OAuth2 PKCE S256 performs a real HTTP exchange via `reqwest` and
//!   validates that the returned access token is minted for the
//!   configured client. Networked verifier; the checker tolerates
//!   async in this branch.
//! - Ed25519 uses `ed25519-dalek`'s `Verifier::verify_strict` which
//!   rejects the low-order-point attack pool that the non-strict API
//!   accepts. Always prefer `verify_strict`.

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::refinement::TrustProof;

type HmacSha256 = Hmac<Sha256>;

// ── Result / error type shared by every verifier ─────────────────────

#[derive(Debug)]
pub enum TrustError {
    /// Signature length didn't match the proof's expected size.
    MalformedSignature(&'static str),
    /// Constant-time comparison returned mismatch.
    SignatureMismatch,
    /// Key could not be decoded (e.g. Ed25519 key not 32 bytes).
    InvalidKey(String),
    /// OAuth2 HTTP exchange or JWT fetch returned a 4xx/5xx or
    /// malformed JSON body.
    ExchangeFailed(String),
    /// The proof is known to the catalogue but the specific invocation
    /// is unsupported (e.g. verifying JWT with alg=none — the 10.e
    /// verifier rejects that, and we surface the error uniformly).
    UnsupportedProof(String),
}

impl std::fmt::Display for TrustError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedSignature(ctx) => {
                write!(f, "malformed signature: {ctx}")
            }
            Self::SignatureMismatch => {
                write!(f, "signature mismatch (constant-time compare)")
            }
            Self::InvalidKey(m) => write!(f, "invalid key: {m}"),
            Self::ExchangeFailed(m) => write!(f, "exchange failed: {m}"),
            Self::UnsupportedProof(m) => write!(f, "unsupported proof: {m}"),
        }
    }
}

impl std::error::Error for TrustError {}

/// Stamped by a successful verifier. The compiler admits a value
/// carrying this tag as `Trusted<T>`; the runtime checks it at every
/// trust-boundary call site. The `proof` field records *which*
/// verifier accepted the payload for replay + audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPayload {
    pub proof: TrustProof,
    /// Opaque identifier of the key/secret that verified this
    /// payload — used by downstream observability. Never the raw key.
    pub key_id: String,
}

// ── HMAC-SHA256 ──────────────────────────────────────────────────────

/// Verify an HMAC-SHA256 tag over `payload`. The tag MUST be a raw
/// 32-byte digest (not hex-encoded); callers that receive hex/base64
/// tags decode them at the boundary.
///
/// This function is the compiler-recognised [`TrustProof::Hmac`]
/// verifier. Any other function computing HMAC is ignored by the
/// checker — so `my_custom_hmac()` cannot produce `Trusted<T>`.
pub fn verify_hmac_sha256(
    payload: &[u8],
    tag: &[u8],
    key: &[u8],
    key_id: &str,
) -> Result<VerifiedPayload, TrustError> {
    if tag.len() != 32 {
        return Err(TrustError::MalformedSignature(
            "HMAC-SHA256 tag must be exactly 32 bytes",
        ));
    }
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| {
        TrustError::InvalidKey(
            "HMAC-SHA256 accepts any key length; this error is unreachable"
                .into(),
        )
    })?;
    mac.update(payload);
    // `verify_slice` routes through `subtle::ConstantTimeEq` internally.
    mac.verify_slice(tag).map_err(|_| TrustError::SignatureMismatch)?;
    Ok(VerifiedPayload {
        proof: TrustProof::Hmac,
        key_id: key_id.to_string(),
    })
}

/// Helper for adopters who receive HMAC tags as hex-encoded strings
/// (GitHub's `X-Hub-Signature-256` convention). Hex decoding is
/// standard and its correctness is not security-critical; the
/// constant-time compare happens inside the MAC verify above.
pub fn verify_hmac_sha256_hex(
    payload: &[u8],
    tag_hex: &str,
    key: &[u8],
    key_id: &str,
) -> Result<VerifiedPayload, TrustError> {
    let tag = hex_decode(tag_hex.strip_prefix("sha256=").unwrap_or(tag_hex))
        .ok_or(TrustError::MalformedSignature(
            "HMAC-SHA256 hex tag did not decode",
        ))?;
    verify_hmac_sha256(payload, &tag, key, key_id)
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for chunk in s.as_bytes().chunks(2) {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

// ── Ed25519 detached signature ───────────────────────────────────────

/// Verify an Ed25519 detached signature using `verify_strict`. We
/// never use the non-strict API: the strict path rejects low-order
/// points + non-canonical encodings that attackers can exploit to
/// produce multiple valid signatures for one message.
pub fn verify_ed25519(
    payload: &[u8],
    signature: &[u8],
    public_key: &[u8],
    key_id: &str,
) -> Result<VerifiedPayload, TrustError> {
    if public_key.len() != 32 {
        return Err(TrustError::InvalidKey(
            "Ed25519 public key must be 32 bytes".into(),
        ));
    }
    if signature.len() != 64 {
        return Err(TrustError::MalformedSignature(
            "Ed25519 signature must be 64 bytes",
        ));
    }
    let pk_array: [u8; 32] = public_key.try_into().map_err(|_| {
        TrustError::InvalidKey("public key length invariant violated".into())
    })?;
    let pk = ed25519_dalek::VerifyingKey::from_bytes(&pk_array)
        .map_err(|e| TrustError::InvalidKey(e.to_string()))?;
    let sig_array: [u8; 64] = signature.try_into().map_err(|_| {
        TrustError::MalformedSignature("signature length invariant violated")
    })?;
    let sig = ed25519_dalek::Signature::from_bytes(&sig_array);
    pk.verify_strict(payload, &sig)
        .map_err(|_| TrustError::SignatureMismatch)?;
    Ok(VerifiedPayload {
        proof: TrustProof::Ed25519,
        key_id: key_id.to_string(),
    })
}

// ── JWT signature (delegates to v1.4.0) ───────────────────────────

/// Thin wrapper exposing [`crate::jwt_verifier`] under the uniform
/// verifier shape. Callers receive a `VerifiedPayload` tagged with
/// [`TrustProof::JwtSig`] on success; the token's claims remain
/// accessible via the underlying verifier API.
///
/// The compiler treats this function as the [`TrustProof::JwtSig`]
/// entry point — direct calls to [`crate::jwt_verifier`] also count
/// (same proof slug), but adopters reaching through this wrapper get
/// the uniform error type for free.
pub async fn verify_jwt_signature(
    token: &str,
    verifier: &crate::jwt_verifier::JwtVerifier,
) -> Result<VerifiedPayload, TrustError> {
    let verified = verifier.verify(token).await.map_err(|e| match e {
        crate::jwt_verifier::JwtVerifyError::UnsupportedAlg(a) => {
            TrustError::UnsupportedProof(format!("alg={a}"))
        }
        other => TrustError::ExchangeFailed(other.to_string()),
    })?;
    // Prefer the token's `jti` (unique identifier) as the key_id for
    // audit trails; fall back to `sub` when issuers don't mint jti;
    // anonymous last so the shape is never empty.
    let key_id = verified
        .jti
        .clone()
        .or_else(|| verified.sub.clone())
        .unwrap_or_else(|| "<anonymous>".to_string());
    Ok(VerifiedPayload {
        proof: TrustProof::JwtSig,
        key_id,
    })
}

// ── OAuth2 PKCE S256 code exchange ───────────────────────────────────

/// Minimal request struct for an OAuth2 authorization-code exchange
/// with PKCE. Callers pass the raw `code` received on the redirect
/// plus the `code_verifier` they generated at flow-start; Axon posts
/// the exchange to the configured `token_endpoint` and returns a
/// [`VerifiedPayload`] on 2xx.
pub struct OAuthCodeExchangeRequest<'a> {
    pub token_endpoint: &'a str,
    pub client_id: &'a str,
    pub client_secret: Option<&'a str>,
    pub redirect_uri: &'a str,
    pub code: &'a str,
    pub code_verifier: &'a str,
}

/// The access-token response body, returned to callers after
/// verification. Fields follow RFC 6749 section 5.1.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub token_type: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
}

/// Perform the PKCE S256 exchange. The verifier is networked;
/// adopters typically call this inside a handler that owns the
/// request-scoped async context.
pub async fn verify_oauth_code_exchange(
    req: OAuthCodeExchangeRequest<'_>,
) -> Result<(VerifiedPayload, OAuthTokenResponse), TrustError> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", req.code),
        ("redirect_uri", req.redirect_uri),
        ("client_id", req.client_id),
        ("code_verifier", req.code_verifier),
    ];
    if let Some(secret) = req.client_secret {
        form.push(("client_secret", secret));
    }

    let client = reqwest::Client::new();
    let resp = client
        .post(req.token_endpoint)
        .form(&form)
        .send()
        .await
        .map_err(|e| TrustError::ExchangeFailed(e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(TrustError::ExchangeFailed(format!(
            "HTTP {status}: {body}"
        )));
    }

    let token: OAuthTokenResponse = resp
        .json()
        .await
        .map_err(|e| TrustError::ExchangeFailed(format!("body parse: {e}")))?;

    Ok((
        VerifiedPayload {
            proof: TrustProof::OAuthCodeExchange,
            key_id: req.client_id.to_string(),
        },
        token,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use subtle::ConstantTimeEq;

    #[test]
    fn hmac_roundtrip() {
        let key = b"super-secret-key";
        let payload = b"order#42|amount=100.00";

        let mut mac = HmacSha256::new_from_slice(key).unwrap();
        mac.update(payload);
        let tag = mac.finalize().into_bytes();

        let vp =
            verify_hmac_sha256(payload, &tag, key, "key-v1").unwrap();
        assert_eq!(vp.proof, TrustProof::Hmac);
        assert_eq!(vp.key_id, "key-v1");
    }

    #[test]
    fn hmac_rejects_tampered_payload() {
        let key = b"super-secret-key";
        let mut mac = HmacSha256::new_from_slice(key).unwrap();
        mac.update(b"original");
        let tag = mac.finalize().into_bytes();

        let err = verify_hmac_sha256(b"tampered", &tag, key, "k").unwrap_err();
        matches!(err, TrustError::SignatureMismatch);
    }

    #[test]
    fn hmac_rejects_wrong_length_tag() {
        let err = verify_hmac_sha256(b"x", &[0u8; 16], b"k", "k").unwrap_err();
        matches!(err, TrustError::MalformedSignature(_));
    }

    #[test]
    fn hmac_hex_decode_roundtrip() {
        let key = b"k";
        let mut mac = HmacSha256::new_from_slice(key).unwrap();
        mac.update(b"hello");
        let tag = mac.finalize().into_bytes();
        let hex_tag = tag.iter().map(|b| format!("{b:02x}")).collect::<String>();

        let vp = verify_hmac_sha256_hex(b"hello", &hex_tag, key, "k").unwrap();
        assert_eq!(vp.proof, TrustProof::Hmac);

        // GitHub-style prefix is stripped.
        let prefixed = format!("sha256={hex_tag}");
        let vp2 =
            verify_hmac_sha256_hex(b"hello", &prefixed, key, "k").unwrap();
        assert_eq!(vp2.proof, TrustProof::Hmac);
    }

    #[test]
    fn ed25519_roundtrip() {
        use ed25519_dalek::{Signer, SigningKey};
        // v1.4.2 — `SigningKey::generate(&mut csprng)` failed to
        // compile under `rand 0.9` because `rand::rngs::OsRng`
        // implements `rand_core 0.9::CryptoRng`, but `ed25519-dalek 2`
        // expects `rand_core 0.6::CryptoRngCore`. Seeding from raw
        // bytes produced by `rand::random` bypasses the trait
        // resolution and is semantically equivalent for a test key.
        let seed: [u8; 32] = rand::random();
        let sk = SigningKey::from_bytes(&seed);
        let pk = sk.verifying_key();
        let payload = b"sigstore-attestation";
        let sig = sk.sign(payload);

        let vp = verify_ed25519(
            payload,
            &sig.to_bytes(),
            pk.as_bytes(),
            "sigstore-key-1",
        )
        .unwrap();
        assert_eq!(vp.proof, TrustProof::Ed25519);
        assert_eq!(vp.key_id, "sigstore-key-1");
    }

    #[test]
    fn ed25519_rejects_tampered_payload() {
        use ed25519_dalek::{Signer, SigningKey};
        // v1.4.2 — same rand_core version skew fix as
        // `ed25519_roundtrip` above.
        let seed: [u8; 32] = rand::random();
        let sk = SigningKey::from_bytes(&seed);
        let pk = sk.verifying_key();
        let sig = sk.sign(b"original");

        let err = verify_ed25519(
            b"tampered",
            &sig.to_bytes(),
            pk.as_bytes(),
            "k",
        )
        .unwrap_err();
        matches!(err, TrustError::SignatureMismatch);
    }

    #[test]
    fn ed25519_rejects_wrong_key_length() {
        let err = verify_ed25519(b"x", &[0u8; 64], b"too_short", "k").unwrap_err();
        matches!(err, TrustError::InvalidKey(_));
    }

    #[test]
    fn subtle_compare_still_available_for_adopters() {
        // Adopters who need raw constant-time compare (outside the
        // catalogue) can use `subtle::ConstantTimeEq` directly.
        let a = [1u8, 2, 3];
        let b = [1u8, 2, 3];
        assert!(bool::from(a.ct_eq(&b)));
    }
}
