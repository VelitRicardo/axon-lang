//! Refinement types — `Trusted<T>` / `Untrusted<T>` + closed Trust Catalog.
//!
//! v1.4.0 — Temporal Algebraic Effects + Trust Types.
//!
//! Axon's type system tracks a payload's *refinement status* at the
//! type level. An `Untrusted<T>` value — e.g. an HTTP body, a
//! WebSocket frame, an OAuth2 redirect code — cannot reach any effect
//! that consumes `Trusted<T>` unless it first passes through a
//! verifier registered in [`TRUST_CATALOG`]. Forgetting to verify =
//! compile-time error emitted by [`crate::type_checker`].
//!
//! The catalogue is **closed**: adding a new proof kind requires a
//! compiler patch + security review, because new verifiers are a
//! load-bearing security boundary. An adopter who wants to refine
//! payloads by some custom predicate must contribute the verifier
//! upstream — not wire their own `verify_hmac` with a non-constant-
//! time comparison.
//!
//! Naming is generic: Axon is adopter-agnostic. These primitives exist
//! at the language layer and are consumed by any adopter that needs
//! to validate HMAC-signed webhooks, JWT-bearing requests, OAuth2
//! code exchanges, or Ed25519-signed detached payloads.

use std::fmt;

// ── Closed catalogue ─────────────────────────────────────────────────

/// Canonical identifiers of the verifiers that can produce a
/// `Trusted<T>` from an `Untrusted<T>`. Each identifier maps to a
/// runtime impl in [`crate::trust_verifiers`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrustProof {
    /// HMAC-SHA256 over the raw payload with a shared secret; used for
    /// webhook signature verification (Stripe, GitHub, generic HMAC).
    Hmac,
    /// JWT signature verification (RS256/RS384/RS512) via a JWKS
    /// endpoint. Shared with v1.4.0's [`crate::jwt_verifier`].
    JwtSig,
    /// OAuth2 authorization-code exchange using PKCE S256. Returns an
    /// access token proof that the bearer controls the code verifier.
    OAuthCodeExchange,
    /// Ed25519 detached signature verification (Sigstore-style).
    Ed25519,
}

impl TrustProof {
    /// Every variant. Keeping this as an explicit slice ensures the
    /// compiler errors if we add a variant without updating the
    /// catalogue consumers.
    pub const ALL: &'static [TrustProof] = &[
        TrustProof::Hmac,
        TrustProof::JwtSig,
        TrustProof::OAuthCodeExchange,
        TrustProof::Ed25519,
    ];

    /// Slug used in source text — appears in `#[refines(...)]`
    /// annotations and in [`crate::type_checker`] diagnostics.
    pub fn slug(self) -> &'static str {
        match self {
            TrustProof::Hmac => "hmac",
            TrustProof::JwtSig => "jwt_sig",
            TrustProof::OAuthCodeExchange => "oauth_code_exchange",
            TrustProof::Ed25519 => "ed25519",
        }
    }

    /// Resolve a slug to a proof, returning `None` for unknown
    /// identifiers. The checker uses this to reject annotations that
    /// don't map to the closed catalogue.
    pub fn from_slug(slug: &str) -> Option<TrustProof> {
        Self::ALL.iter().copied().find(|p| p.slug() == slug)
    }
}

impl fmt::Display for TrustProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

/// Catalogue lookup for the checker's diagnostic messages.
///
/// ```text
/// error: unknown trust proof 'crc32'. Valid: hmac, jwt_sig, oauth_code_exchange, ed25519
/// ```
pub const TRUST_CATALOG: &[&str] = &["hmac", "jwt_sig", "oauth_code_exchange", "ed25519"];

// ── Refinement type names ────────────────────────────────────────────

/// Type constructor names the checker recognises at the AST level.
/// Both take a single generic parameter (`Trusted<HttpBody>`,
/// `Untrusted<WsFrame>`). The parameter itself is an arbitrary type;
/// the refinement is carried by the constructor.
pub const TRUSTED_TYPE_CTOR: &str = "Trusted";
pub const UNTRUSTED_TYPE_CTOR: &str = "Untrusted";

/// True when `name` is one of the refinement constructors.
pub fn is_refinement_type(name: &str) -> bool {
    name == TRUSTED_TYPE_CTOR || name == UNTRUSTED_TYPE_CTOR
}

/// True when `name` is specifically the safe-to-consume side.
pub fn is_trusted_type(name: &str) -> bool {
    name == TRUSTED_TYPE_CTOR
}

/// True when `name` is specifically the unsafe side.
pub fn is_untrusted_type(name: &str) -> bool {
    name == UNTRUSTED_TYPE_CTOR
}

// ── Refinement annotation parsing ────────────────────────────────────

/// A `#[refines(proof, ...options...)]` annotation attached to a
/// function body or to a specific `let` binding, declaring that the
/// body produces a `Trusted<T>` iff the named proof succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefinementAnnotation {
    pub proof: TrustProof,
    /// Raw `key=value` pairs accepted by the verifier (e.g.
    /// `key=env.HMAC_KEY`, `algorithm=SHA256`). The checker does not
    /// interpret these; the runtime verifier does.
    pub options: Vec<(String, String)>,
}

/// Parse a refinement annotation body of the form `hmac` or
/// `hmac, key=env.HMAC_KEY`. Returns `None` when the proof slug is not
/// in [`TRUST_CATALOG`] so the checker can emit a targeted error.
pub fn parse_refinement_annotation(body: &str) -> Option<RefinementAnnotation> {
    let mut parts = body.split(',').map(|p| p.trim());
    let proof_slug = parts.next()?.trim();
    let proof = TrustProof::from_slug(proof_slug)?;

    let mut options = Vec::new();
    for raw in parts {
        if raw.is_empty() {
            continue;
        }
        if let Some((k, v)) = raw.split_once('=') {
            options.push((k.trim().to_string(), v.trim().to_string()));
        } else {
            // Malformed option — signal by returning None so the
            // checker can emit `expected key=value, got '<raw>'`.
            return None;
        }
    }
    Some(RefinementAnnotation { proof, options })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_roundtrip_covers_closed_catalog() {
        for proof in TrustProof::ALL {
            let slug = proof.slug();
            assert_eq!(Some(*proof), TrustProof::from_slug(slug));
            assert!(TRUST_CATALOG.contains(&slug));
        }
        assert_eq!(TrustProof::ALL.len(), TRUST_CATALOG.len());
    }

    #[test]
    fn unknown_slug_is_rejected() {
        assert!(TrustProof::from_slug("crc32").is_none());
        assert!(TrustProof::from_slug("").is_none());
        assert!(TrustProof::from_slug("HMAC").is_none()); // case-sensitive
    }

    #[test]
    fn refinement_constructors_recognised() {
        assert!(is_refinement_type("Trusted"));
        assert!(is_refinement_type("Untrusted"));
        assert!(!is_refinement_type("Option"));
        assert!(!is_refinement_type("trusted")); // case-sensitive
    }

    #[test]
    fn parse_annotation_minimal() {
        let ann = parse_refinement_annotation("hmac").unwrap();
        assert_eq!(ann.proof, TrustProof::Hmac);
        assert!(ann.options.is_empty());
    }

    #[test]
    fn parse_annotation_with_options() {
        let ann = parse_refinement_annotation("hmac, key=env.HMAC_KEY, algorithm=SHA256").unwrap();
        assert_eq!(ann.proof, TrustProof::Hmac);
        assert_eq!(
            ann.options,
            vec![
                ("key".to_string(), "env.HMAC_KEY".to_string()),
                ("algorithm".to_string(), "SHA256".to_string()),
            ]
        );
    }

    #[test]
    fn parse_annotation_rejects_unknown_proof() {
        assert!(parse_refinement_annotation("crc32, key=foo").is_none());
    }

    #[test]
    fn parse_annotation_rejects_malformed_option() {
        // 'key_without_value' has no '=' sign.
        assert!(parse_refinement_annotation("hmac, key_without_value").is_none());
    }
}
