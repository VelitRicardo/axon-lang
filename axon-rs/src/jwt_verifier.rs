//! JWT signature verification + JWKS client.
//!
//! Closes the v1.4.0 gap where `tenant.rs` previously extracted
//! `tenant_id` from the payload without checking the signature.
//!
//! Wire contract (must match Python `axon_enterprise.jwt_issuer`):
//!
//! - `alg` ∈ { RS256, RS384, RS512 }. HS* / `none` / ES* rejected.
//! - `iss` must equal the configured issuer.
//! - `aud` must contain the configured audience.
//! - `exp` / `nbf` / `iat` validated with configurable clock-skew leeway.
//! - `tenant_id` claim is required — absence means the token is
//!   structurally valid but not usable for tenant extraction, so we
//!   treat it as rejection.
//!
//! JWKS is fetched lazily and cached for `jwks_ttl` seconds. On a `kid`
//! miss we force-refresh once — matches Python's behaviour so IdP
//! rotation (new kid published minutes before first use) works
//! transparently.

use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;

/// Errors the verifier can surface. Mapped to HTTP 401 by the middleware.
///
/// Hand-rolled (no `thiserror` dep) to keep the Rust crate's dependency
/// surface minimal.
#[derive(Debug)]
pub enum JwtVerifyError {
    UnsupportedAlg(String),
    MissingKid,
    UnknownKid { kid: String },
    JwksFetchFailed(String),
    Invalid(String),
    ClaimMissing(&'static str),
}

impl std::fmt::Display for JwtVerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedAlg(a) => write!(f, "unsupported algorithm: {a}"),
            Self::MissingKid => write!(f, "missing kid in JWT header"),
            Self::UnknownKid { kid } => write!(f, "kid {kid:?} not in JWKS after refresh"),
            Self::JwksFetchFailed(m) => write!(f, "JWKS fetch failed: {m}"),
            Self::Invalid(m) => write!(f, "signature or claim validation failed: {m}"),
            Self::ClaimMissing(n) => write!(f, "required claim missing: {n}"),
        }
    }
}

impl std::error::Error for JwtVerifyError {}

/// Subset of JWT claims the tenant extractor cares about. Additional
/// claims present in the token (roles, plan, jti, …) are available
/// via [`VerifiedToken::claims`].
#[derive(Debug, Clone, Deserialize)]
pub struct MinimalClaims {
    #[serde(rename = "tenant_id")]
    pub tenant_id: String,
}

#[derive(Debug, Clone)]
pub struct VerifiedToken {
    pub tenant_id: String,
    pub plan: Option<String>,
    pub roles: Vec<String>,
    pub jti: Option<String>,
    pub sub: Option<String>,
    pub claims: Value,
}

/// Configuration resolved at startup from env vars.
#[derive(Debug, Clone)]
pub struct JwtVerifierConfig {
    /// Expected `iss` claim.
    pub issuer: String,
    /// Expected `aud` claim.
    pub audience: String,
    /// Absolute URL of the JWKS document (typically
    /// `https://auth.<host>/.well-known/jwks.json`).
    pub jwks_url: String,
    /// Duration a cached JWKS document is trusted without a refresh.
    pub jwks_ttl: Duration,
    /// Clock-skew leeway in seconds applied to exp / nbf / iat.
    pub leeway_secs: u64,
    /// When true, missing JWTs cause a 401; when false (default for
    /// pre-10.e deployments still rolling out), the middleware falls
    /// back to header-based + unverified-payload extraction with a
    /// warning log. Production deployments flip this to true in 10.j.
    pub enforce: bool,
}

impl JwtVerifierConfig {
    /// Build the config from env vars, returning `None` when the JWKS
    /// URL is unset — the middleware treats `None` as "no verifier
    /// configured" and keeps the legacy behaviour for tests / OSS users.
    pub fn from_env() -> Option<Self> {
        let jwks_url = std::env::var("AXON_JWT_JWKS_URL").ok().filter(|s| !s.is_empty())?;
        let issuer = std::env::var("AXON_JWT_ISSUER")
            .unwrap_or_else(|_| "https://auth.bemarking.com".into());
        let audience =
            std::env::var("AXON_JWT_AUDIENCE").unwrap_or_else(|_| "axon-api".into());
        let jwks_ttl_secs: u64 = std::env::var("AXON_JWT_JWKS_TTL_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(600);
        let leeway_secs: u64 = std::env::var("AXON_JWT_LEEWAY_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);
        let enforce = std::env::var("AXON_ENFORCE_JWT_VERIFICATION")
            .ok()
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes"))
            .unwrap_or(true);
        Some(Self {
            issuer,
            audience,
            jwks_url,
            jwks_ttl: Duration::from_secs(jwks_ttl_secs),
            leeway_secs,
            enforce,
        })
    }
}

// ── JWKS cache ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct JwksEntry {
    kid: String,
    kty: String,
    /// Wire-shape documentation (JWKS carries it); verification pins the alg
    /// via the header check, so this entry field has no reader.
    #[allow(dead_code)]
    alg: Option<String>,
    n: Option<String>,
    e: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct JwksDocument {
    keys: Vec<JwksEntry>,
}

struct CacheSlot {
    loaded_at: Instant,
    keys: Vec<JwksEntry>,
}

/// Thread-safe JWKS fetcher with TTL + rotation-on-miss.
pub struct JwksClient {
    url: String,
    ttl: Duration,
    http: reqwest::Client,
    slot: Mutex<Option<CacheSlot>>,
}

impl JwksClient {
    pub fn new(url: String, ttl: Duration) -> Self {
        Self {
            url,
            ttl,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("reqwest client"),
            slot: Mutex::new(None),
        }
    }

    async fn resolve_key(&self, kid: &str) -> Result<JwksEntry, JwtVerifyError> {
        {
            let slot = self.slot.lock().await;
            if let Some(c) = slot.as_ref() {
                if c.loaded_at.elapsed() < self.ttl {
                    if let Some(k) = c.keys.iter().find(|k| k.kid == kid) {
                        return Ok(k.clone());
                    }
                }
            }
        }
        self.refresh().await?;
        let slot = self.slot.lock().await;
        let cache = slot.as_ref().ok_or_else(|| {
            JwtVerifyError::JwksFetchFailed("empty cache after refresh".into())
        })?;
        cache
            .keys
            .iter()
            .find(|k| k.kid == kid)
            .cloned()
            .ok_or_else(|| JwtVerifyError::UnknownKid { kid: kid.to_string() })
    }

    async fn refresh(&self) -> Result<(), JwtVerifyError> {
        let resp = self
            .http
            .get(&self.url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| JwtVerifyError::JwksFetchFailed(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(JwtVerifyError::JwksFetchFailed(format!(
                "HTTP {}",
                resp.status()
            )));
        }
        let doc: JwksDocument = resp
            .json()
            .await
            .map_err(|e| JwtVerifyError::JwksFetchFailed(e.to_string()))?;
        let mut slot = self.slot.lock().await;
        *slot = Some(CacheSlot {
            loaded_at: Instant::now(),
            keys: doc.keys,
        });
        Ok(())
    }
}

// ── Verifier ────────────────────────────────────────────────────────────────

pub struct JwtVerifier {
    cfg: JwtVerifierConfig,
    jwks: Arc<JwksClient>,
}

impl JwtVerifier {
    pub fn new(cfg: JwtVerifierConfig) -> Self {
        let jwks = Arc::new(JwksClient::new(cfg.jwks_url.clone(), cfg.jwks_ttl));
        Self { cfg, jwks }
    }

    pub fn config(&self) -> &JwtVerifierConfig {
        &self.cfg
    }

    pub async fn verify(&self, token: &str) -> Result<VerifiedToken, JwtVerifyError> {
        let header =
            decode_header(token).map_err(|e| JwtVerifyError::Invalid(e.to_string()))?;
        let alg = match header.alg {
            Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => header.alg,
            other => {
                return Err(JwtVerifyError::UnsupportedAlg(format!("{other:?}")));
            }
        };
        let kid = header.kid.ok_or(JwtVerifyError::MissingKid)?;
        let entry = self.jwks.resolve_key(&kid).await?;

        if entry.kty != "RSA" {
            return Err(JwtVerifyError::UnsupportedAlg(format!(
                "non-RSA JWK kty={}",
                entry.kty
            )));
        }

        let n = entry.n.ok_or_else(|| {
            JwtVerifyError::Invalid("JWK missing modulus".into())
        })?;
        let e = entry.e.ok_or_else(|| {
            JwtVerifyError::Invalid("JWK missing exponent".into())
        })?;
        let key = DecodingKey::from_rsa_components(&n, &e)
            .map_err(|err| JwtVerifyError::Invalid(err.to_string()))?;

        let mut validation = Validation::new(alg);
        validation.set_issuer(&[self.cfg.issuer.clone()]);
        validation.set_audience(&[self.cfg.audience.clone()]);
        validation.leeway = self.cfg.leeway_secs;
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.required_spec_claims =
            ["iss", "aud", "exp", "iat", "sub"].iter().map(|s| s.to_string()).collect();

        let data = decode::<Value>(token, &key, &validation)
            .map_err(|err| JwtVerifyError::Invalid(err.to_string()))?;
        let claims = data.claims;

        let tenant_id = claims
            .get("tenant_id")
            .and_then(|v| v.as_str())
            .ok_or(JwtVerifyError::ClaimMissing("tenant_id"))?
            .to_string();
        let plan = claims.get("plan").and_then(|v| v.as_str()).map(String::from);
        let roles = claims
            .get("roles")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let jti = claims.get("jti").and_then(|v| v.as_str()).map(String::from);
        let sub = claims.get("sub").and_then(|v| v.as_str()).map(String::from);

        Ok(VerifiedToken {
            tenant_id,
            plan,
            roles,
            jti,
            sub,
            claims,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// v2.83.0 — serialises the two env-reading tests.
    ///
    /// The environment is PROCESS-GLOBAL and these two tests write opposite
    /// values into the same variable: one removes `AXON_JWT_JWKS_URL` to prove
    /// the config refuses without it, the other sets it to prove the config
    /// reads it. Run concurrently — which `cargo test` does by default — each
    /// can observe the other's write, and `config_from_env_reads_values`
    /// failed intermittently in exactly that way.
    ///
    /// The comment "Safety: isolate from other tests that may set the env var"
    /// was already on the first test, and save/restore does not isolate
    /// anything: it protects the value ACROSS the test, not DURING it. A lock
    /// is what makes the two mutually exclusive.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn config_from_env_requires_jwks_url() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prev = std::env::var("AXON_JWT_JWKS_URL").ok();
        std::env::remove_var("AXON_JWT_JWKS_URL");
        assert!(JwtVerifierConfig::from_env().is_none());
        if let Some(v) = prev {
            std::env::set_var("AXON_JWT_JWKS_URL", v);
        }
    }

    #[test]
    fn config_from_env_reads_values() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("AXON_JWT_JWKS_URL", "https://x/jwks.json");
        std::env::set_var("AXON_JWT_ISSUER", "https://x");
        std::env::set_var("AXON_JWT_AUDIENCE", "x-api");
        let cfg = JwtVerifierConfig::from_env().unwrap();
        assert_eq!(cfg.issuer, "https://x");
        assert_eq!(cfg.audience, "x-api");
        std::env::remove_var("AXON_JWT_JWKS_URL");
        std::env::remove_var("AXON_JWT_ISSUER");
        std::env::remove_var("AXON_JWT_AUDIENCE");
    }
}
