//! v2.46.0 — the `CredentialMinter` port: the runtime seam behind the
//! `mint <Credential> as <binding>` flow verb (v2.46.0).
//!
//! The doctrine (`axon://logic/authority_only_attenuates`): delegation is
//! attenuation. A mint is admitted only when the minted `grants` are a
//! subset of the capabilities the MINTING principal itself holds — authority
//! flows down, never up. The law is enforced twice, fail-closed:
//!
//! 1. **At the dispatch handler** (`flow_dispatcher::run_mint`) when the
//!    request carries a capability context (`ctx.held_capabilities`, the
//! v1.30.0 JWT claim surface) — a request-bound mint can never exceed its
//!    bearer.
//! 2. **Inside the minter implementation** against
//!    [`MintRequest::minter_capabilities`] — so a port implementation is
//!    safe even if a future call site forgets the handler-side check.
//!    An implementation MUST refuse when `minter_capabilities` is `None`
//!    (no capability context = no provable authority to attenuate FROM).
//!
//! There is deliberately **no default production minter in OSS**: a `mint`
//! reached with no port configured is a loud
//! `DispatchError::MissingDependency` (the v2.41.0 no-silent-stub lesson). The
//! enterprise executor injects its PASETO-backed minter (v2.46.0); tests and
//! single-process adopters can use [`InMemoryMinter`], which enforces the
//! full attenuation law and keeps an in-process verify set.

use std::collections::HashMap;
use std::sync::Mutex;

/// What the handler asks a minter to do. All fields come from the compiled
/// contract (`IRCredential`) + the dispatch context — the minter never
/// consults ambient state to decide WHAT to mint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MintRequest {
    /// The declared contract's name (diagnostics + audit).
    pub credential_name: String,
    /// The capability slugs the minted bearer will carry (the contract's
    /// `grants:` — already validated dotted slugs).
    pub grants: Vec<String>,
    /// Bearer lifetime in seconds (the contract's `ttl:`, ceiling-checked
    /// at compile time by `axon-T894`).
    pub ttl_secs: u64,
    /// Tenant the bearer is scoped to (empty for single-tenant OSS).
    pub tenant: String,
    /// The MINTING principal's capability set, when the dispatch carries
    /// one (the v1.30.0 bearer claims). `None` = no capability context — an
    /// implementation MUST refuse (fail-closed: you cannot attenuate from
    /// authority you cannot prove).
    pub minter_capabilities: Option<Vec<String>>,
}

/// A successfully minted bearer. The raw token is shown ONCE — the v2.46.0
/// type-checker forbids it from entering a store (`axon-T896`), and the
/// dispatch handler binds it without echoing it onto the wire audit.
#[derive(Debug, Clone)]
pub struct MintedCredential {
    /// The raw bearer string the widget/visitor presents.
    pub token: String,
    /// Expiry instant (Unix ms) the implementation computed from
    /// `ttl_secs`. Diagnostic — the authoritative expiry is whatever the
    /// verifying side enforces.
    pub expires_at_ms: i64,
}

/// Why a mint was refused. Every variant is fail-closed and names its law.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MintError {
    /// `grants ⊄ capabilities(minter)` — the attenuation law
    /// (`authority_only_attenuates`). Carries the offending grants.
    AttenuationViolated { missing: Vec<String> },
    /// No capability context to attenuate from (`minter_capabilities`
    /// was `None`).
    NoMinterAuthority,
    /// Implementation-specific failure (key unavailable, storage error, …).
    Backend(String),
}

impl std::fmt::Display for MintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MintError::AttenuationViolated { missing } => write!(
                f,
                "attenuation violated (authority_only_attenuates): the minting principal \
                 does not hold {missing:?} — a credential can only carry capabilities its \
                 minter already holds"
            ),
            MintError::NoMinterAuthority => write!(
                f,
                "no minter authority: the dispatch carries no capability context to \
                 attenuate from (fail-closed)"
            ),
            MintError::Backend(msg) => write!(f, "minter backend error: {msg}"),
        }
    }
}

/// The port. Implementations MUST enforce the attenuation law against
/// [`MintRequest::minter_capabilities`] (see the module docs — the handler
/// also enforces it when it can, but the port must be safe standalone).
pub trait CredentialMinter: Send + Sync {
    fn mint(&self, req: MintRequest) -> Result<MintedCredential, MintError>;
}

/// Shared helper: the attenuation law. `Ok(())` iff a capability context is
/// present and every grant is held. Used by [`InMemoryMinter`] and intended
/// for reuse by enterprise implementations so the two can never diverge.
pub fn check_attenuation(req: &MintRequest) -> Result<(), MintError> {
    let Some(caps) = &req.minter_capabilities else {
        return Err(MintError::NoMinterAuthority);
    };
    let missing: Vec<String> = req
        .grants
        .iter()
        .filter(|g| !caps.iter().any(|c| c == *g))
        .cloned()
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(MintError::AttenuationViolated { missing })
    }
}

/// A minted-token record the in-memory reference keeps for verification.
#[derive(Debug, Clone)]
pub struct InMemoryMintRecord {
    pub credential_name: String,
    pub grants: Vec<String>,
    pub tenant: String,
    pub expires_at_ms: i64,
}

/// The reference in-process minter: CSPRNG-quality opaque tokens
/// (`axep_…`), full attenuation-law enforcement, and an in-memory verify
/// set so a single-process adopter (or a test) can round-trip
/// mint → verify. NOT a distributed credential system — the v2.46.0
/// enterprise minter (stateless PASETO `v4.local` + tenant epoch) is the
/// production surface.
#[derive(Default)]
pub struct InMemoryMinter {
    minted: Mutex<HashMap<String, InMemoryMintRecord>>,
}

impl InMemoryMinter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a previously minted token (tests / single-process verify).
    /// `None` for unknown tokens; the caller checks `expires_at_ms`.
    pub fn verify(&self, token: &str) -> Option<InMemoryMintRecord> {
        self.minted.lock().unwrap().get(token).cloned()
    }
}

impl CredentialMinter for InMemoryMinter {
    fn mint(&self, req: MintRequest) -> Result<MintedCredential, MintError> {
        check_attenuation(&req)?;
        // Two v4 UUIDs ≈ 244 bits of CSPRNG entropy — ample for an
        // in-process reference token (the production format is v2.46.0).
        let token = format!(
            "axep_{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| MintError::Backend(e.to_string()))?
            .as_millis() as i64;
        let expires_at_ms = now_ms + (req.ttl_secs as i64) * 1000;
        self.minted.lock().unwrap().insert(
            token.clone(),
            InMemoryMintRecord {
                credential_name: req.credential_name,
                grants: req.grants,
                tenant: req.tenant,
                expires_at_ms,
            },
        );
        Ok(MintedCredential {
            token,
            expires_at_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(grants: &[&str], caps: Option<&[&str]>) -> MintRequest {
        MintRequest {
            credential_name: "WidgetSession".into(),
            grants: grants.iter().map(|s| s.to_string()).collect(),
            ttl_secs: 900,
            tenant: "t1".into(),
            minter_capabilities: caps.map(|c| c.iter().map(|s| s.to_string()).collect()),
        }
    }

    #[test]
    fn mint_round_trips_and_records_the_contract() {
        let m = InMemoryMinter::new();
        let minted = m
            .mint(req(&["chat.invoke"], Some(&["chat.invoke", "flow.execute"])))
            .expect("attenuated mint admits");
        assert!(minted.token.starts_with("axep_"), "{}", minted.token);
        let rec = m.verify(&minted.token).expect("verifiable");
        assert_eq!(rec.grants, vec!["chat.invoke"]);
        assert_eq!(rec.tenant, "t1");
        assert!(rec.expires_at_ms > 0);
    }

    #[test]
    fn attenuation_violation_fails_closed_and_names_the_missing_grant() {
        let m = InMemoryMinter::new();
        let err = m
            .mint(req(&["chat.invoke", "tenant.update"], Some(&["chat.invoke"])))
            .expect_err("amplification must be refused");
        assert_eq!(
            err,
            MintError::AttenuationViolated {
                missing: vec!["tenant.update".to_string()]
            }
        );
        assert!(err.to_string().contains("authority_only_attenuates"));
    }

    #[test]
    fn no_capability_context_fails_closed() {
        let m = InMemoryMinter::new();
        let err = m.mint(req(&["chat.invoke"], None)).expect_err("no authority");
        assert_eq!(err, MintError::NoMinterAuthority);
    }

    #[test]
    fn tokens_are_unique_and_unknown_tokens_do_not_verify() {
        let m = InMemoryMinter::new();
        let caps = Some(&["chat.invoke"][..]);
        let a = m.mint(req(&["chat.invoke"], caps)).unwrap();
        let b = m.mint(req(&["chat.invoke"], caps)).unwrap();
        assert_ne!(a.token, b.token);
        assert!(m.verify("axep_forged").is_none());
    }
}
