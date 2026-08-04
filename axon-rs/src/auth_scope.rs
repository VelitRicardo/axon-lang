//! §Fase 32.g — Auth scope (capability subset matching) for first-class
//! axonendpoint routes.
//!
//! D8 ratificada 2026-05-11. When an axonendpoint declares
//! `requires: [admin, legal.read, …]`, the runtime verifies the request
//! bearer's capabilities contain every declared capability (AND
//! semantics). Missing capability → 403 Forbidden with a structured
//! error so the client KNOWS which capability is needed.
//!
//! ## Closed slug grammar (mirror of `axon_frontend::parser::is_valid_capability_slug`)
//!
//! `^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*$`
//!
//! - Each segment starts with a lowercase letter.
//! - Each segment contains lowercase letters, digits, or underscores.
//! - Segments separated by single dots.
//!
//! Closed grammar refuses interpretation drift — adopters can't
//! accidentally introduce slug shapes that fail to match in production
//! (e.g. `Admin`, `legal-read`).
//!
//! ## OSS vs enterprise capability surface
//!
//! - **OSS (this module)**: capabilities are read from the JWT bearer's
//!   `capabilities` claim. No signature verification at this layer
//!   (signature is verified by `tenant_extractor_middleware` upstream
//!   when JWKS is configured). The unverified-decode path matches the
//!   existing `tenant_id_from_bearer_unverified` precedent for single-
//!   tenant / dev installs.
//! - **Enterprise** (Fase 21 integration surface): capabilities are
//!   registered + version-introspected via `/.well-known/axon-
//!   capabilities`; auditors verify the runtime's capability set matches
//!   the deployed source. Layered on top of this OSS primitive.
//!
//! ## Pillar trace per D12
//!
//! - **PHILOSOPHY** — the access contract IS the source declaration:
//!   auditors read source + KNOW which endpoints require which
//!   capabilities. No middleware side-channel.
//! - **LOGIC** — `declared_requires ⊆ token_capabilities` is the
//!   precise subset predicate; total boolean over the two sets.
//! - **MATHEMATICS** — capability matching is set-theoretic: subset
//!   check is associative, idempotent, and decidable.
//! - **COMPUTING** — D9 backwards-compat absolute: empty `requires`
//!   list short-circuits to pass-through; no behavior change for
//!   v1.20.x–v1.22.x adopters.

use std::collections::BTreeSet;

// §Fase 118.a — `http::HeaderMap`, not `axum::http::HeaderMap`. Same type: axum
// re-exports it. This module is about capability slugs, and its only tie to the
// web framework was a header type, so naming the real crate lets it stay outside
// the `server` feature.
use http::HeaderMap;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::Value;

/// Result of an auth-scope check. Total enum — every input maps to
/// exactly one variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthVerdict {
    /// No `requires:` declared, OR bearer holds every required slug.
    Allow,
    /// Bearer is missing one or more declared capabilities. The
    /// payload surfaces BOTH lists so the adopter client can correct
    /// the request without server-log diving (precise, auditable).
    Deny {
        missing: Vec<String>,
        required: Vec<String>,
        have: Vec<String>,
    },
}

/// Re-export the parser-layer slug validator so runtime callers
/// (config loaders, dynamic ingestion paths, fuzz harnesses) hit the
/// same predicate the parser enforces. Single source of truth across
/// compile time + runtime + tests.
pub fn is_valid_capability_slug(slug: &str) -> bool {
    crate::parser::is_valid_capability_slug(slug)
}

/// Extract the bearer's capability slugs from `Authorization: Bearer
/// <jwt>`. Returns an empty vec when there's no bearer, the token
/// isn't a structurally-valid JWT, or the payload doesn't carry a
/// `capabilities` claim.
///
/// Signature verification is performed UPSTREAM by
/// `tenant_extractor_middleware` when `AXON_JWT_JWKS_URL` is set.
/// In OSS / single-tenant installs without JWKS, this decode is
/// unverified — matching the existing precedent in
/// `axon::tenant::tenant_id_from_bearer_unverified`. Adopters in
/// production who need verified extraction layer enterprise auth
/// middleware on top.
///
/// The `capabilities` claim MUST be a JSON array of strings. Other
/// shapes (object, scalar) are treated as "no capabilities" — be
/// strict in what we accept (D8 + LOGIC pillar).
pub fn extract_capabilities_from_bearer(headers: &HeaderMap) -> Vec<String> {
    let auth = match headers.get("authorization").and_then(|v| v.to_str().ok()) {
        Some(a) => a,
        None => return Vec::new(),
    };
    let token = match auth.strip_prefix("Bearer ") {
        Some(t) => t,
        None => return Vec::new(),
    };
    let parts: Vec<&str> = token.splitn(3, '.').collect();
    if parts.len() < 2 {
        return Vec::new();
    }
    let payload_bytes = match URL_SAFE_NO_PAD.decode(parts[1]) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    let claims: Value = match serde_json::from_slice(&payload_bytes) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let arr = match claims.get("capabilities").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return Vec::new(),
    };
    arr.iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect()
}

/// Subset check: are all `declared` capabilities present in `have`?
///
/// Total predicate. Empty `declared` returns `Allow` unconditionally
/// (D9 backwards-compat). Returns `Deny` with `missing` = `declared \
/// have`, preserving declaration order for client-side diagnostic
/// continuity.
pub fn check_capabilities(declared: &[String], have: &[String]) -> AuthVerdict {
    if declared.is_empty() {
        return AuthVerdict::Allow;
    }
    let have_set: BTreeSet<&str> = have.iter().map(|s| s.as_str()).collect();
    let missing: Vec<String> = declared
        .iter()
        .filter(|d| !have_set.contains(d.as_str()))
        .cloned()
        .collect();
    if missing.is_empty() {
        AuthVerdict::Allow
    } else {
        AuthVerdict::Deny {
            missing,
            required: declared.to_vec(),
            have: have.to_vec(),
        }
    }
}

// ════════════════════════════════════════════════════════════════════
//  §Fase 90.a — canonical capability projection `π` + the grantability
//  law (`every_requirement_is_grantable`).
//
//  §89 proved *every boundary declares a guard* (`every_boundary_is_
//  guarded`, `axon-T890`). §90 proves *every declared guard is
//  satisfiable*: a `requires: [x]` whose `x` no authority can grant is a
//  dead boundary — a locked door with no fabricable key. This module
//  supplies the total, injective projection between the two authority
//  representations that fractured in production (Kivi brief #55): the
//  RBAC control-plane catalog is COLON (`resource:action`); the
//  `requires:` data-plane grammar is DOTTED (`resource.action`); nothing
//  bridged them, so a role-derived authority could never satisfy a
//  `requires:`. `π` is that bridge.
//
//  ## Pillar trace
//  - **MATHEMATICS** — `π` is a total function on well-formed authority
//    strings; it is injective on the single-colon catalog (the dot in
//    `r.a` can only have come from the colon in `r:a`), so distinct
//    authorities never collapse to one capability.
//  - **LOGIC** — the grantability law is the subset predicate
//    `requires ⊆ π(catalog) ∪ reserved`; decidable, total.
//  - **PHILOSOPHY** — `π` creates NO authority. A principal gains no
//    capability they did not already hold as a permission; `π` only
//    makes the *representation* of held authority match the
//    *representation* a boundary requires. No `no_unwitnessed_advantage`
//    (§69) concern — there is no cognition here, only a total function.
//  - **COMPUTING** — fail-closed: a permission that does not project to
//    a valid canonical slug is `NotProjectable` (never silently
//    dropped), and a fractured namespace (two authorities colliding to
//    one capability) is surfaced, never hidden.
// ════════════════════════════════════════════════════════════════════

/// A validated capability in the canonical (dotted) namespace — the
/// codomain of `π`. Constructed only through [`project_permission`] or
/// [`Capability::parse`], so an existing `Capability` is always a valid
/// slug (`is_valid_capability_slug`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Capability(String);

impl Capability {
    /// Parse an already-canonical dotted slug into a `Capability`.
    /// Returns `None` if the input is not a valid slug.
    pub fn parse(slug: &str) -> Option<Capability> {
        if is_valid_capability_slug(slug) {
            Some(Capability(slug.to_string()))
        } else {
            None
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why an authority string could not be projected into the canonical
/// capability namespace. Fail-closed: the caller must decide (reject the
/// grant, error the deploy), never silently drop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionError {
    /// The projected candidate is not a valid canonical slug — e.g. the
    /// input had more than one `:` (not `resource:action` shape),
    /// uppercase, a hyphen, or an empty segment.
    NotProjectable(String),
}

impl std::fmt::Display for ProjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectionError::NotProjectable(s) => {
                write!(f, "authority `{s}` does not project to a valid capability slug")
            }
        }
    }
}

/// `π` — project an authority string into the canonical capability
/// namespace.
///
/// - A COLON RBAC permission `resource:action` (exactly one colon) maps
///   to the dotted `resource.action`.
/// - An already-canonical DOTTED cap (no colon, e.g. the reserved
///   `store.platform_read`, `chat.invoke`) maps to itself.
/// - Anything else (≥2 colons, or a candidate that fails the slug
///   grammar) is `NotProjectable`.
///
/// **Total** on well-formed authority strings; **injective** on the
/// single-colon catalog: the unique dot in the image came from the
/// unique colon in the source, so `π(a) == π(b) ⇒ a == b` for any two
/// single-colon inputs. (Cross-collisions between a projected colon-perm
/// and a native dotted cap are detected by [`build_grantable_set`], not
/// assumed away.)
pub fn project_permission(perm: &str) -> Result<Capability, ProjectionError> {
    let colon_count = perm.bytes().filter(|b| *b == b':').count();
    let candidate = match colon_count {
        0 => perm.to_string(),
        1 => perm.replace(':', "."),
        _ => return Err(ProjectionError::NotProjectable(perm.to_string())),
    };
    Capability::parse(&candidate).ok_or_else(|| ProjectionError::NotProjectable(perm.to_string()))
}

/// The image of `π` over a HELD authority set, with the authorities that
/// did not project surfaced explicitly — never silently dropped.
///
/// §Fase 93.a (`every_granted_authority_projects`). [`build_grantable_set`]
/// serves the *catalog* side of the law (deploy gate: is a requirement
/// satisfiable by ANY authority?). This serves the *principal* side: given
/// the permissions a principal actually holds (built-in role expansion ∪
/// DB-resolved custom-role rows ∪ machine grants), what capability set do
/// they project to — and which held authorities were unprojectable? A
/// verify-time caller that swallowed projection failures would turn a
/// tampered or legacy `role_permissions` row into a silent authority gap,
/// indistinguishable from a revocation; surfacing `dropped` lets the
/// runtime degrade per-item WITH a witness (structured log), the
/// boot-hydrate self-heal posture applied to authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedSet {
    /// The canonical (dotted) projection of every projectable input.
    pub capabilities: BTreeSet<Capability>,
    /// `(authority, why)` for every input that did not project.
    pub dropped: Vec<(String, ProjectionError)>,
}

impl ProjectedSet {
    /// True iff every held authority projected — the healthy state.
    pub fn is_total(&self) -> bool {
        self.dropped.is_empty()
    }
}

/// `π` lifted to a SET of held authorities — total over any input, with
/// explicit drops (the dual of [`build_grantable_set`], which serves the
/// catalog; this serves the principal). Duplicate authorities are
/// idempotent; the image is a set. Unlike `build_grantable_set` this does
/// NOT collision-check: on the principal side two held authorities
/// projecting to one capability is harmless (the principal holds it
/// either way); fracture detection belongs to the catalog gate.
pub fn project_permission_set<I, S>(authorities: I) -> ProjectedSet
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut capabilities: BTreeSet<Capability> = BTreeSet::new();
    let mut dropped: Vec<(String, ProjectionError)> = Vec::new();
    for auth in authorities {
        let auth = auth.as_ref();
        match project_permission(auth) {
            Ok(cap) => {
                capabilities.insert(cap);
            }
            Err(e) => dropped.push((auth.to_string(), e)),
        }
    }
    ProjectedSet {
        capabilities,
        dropped,
    }
}

/// The grantable capability set built from an authority catalog, PLUS
/// any collisions surfaced during projection. A collision means two
/// distinct authorities projected to the *same* capability — a fractured
/// namespace where requiring that capability is ambiguous about which
/// authority grants it. Fail-closed: the caller (deploy gate) must
/// reject a catalog with collisions rather than pick a winner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantableSet {
    /// The projected, canonical grantable capabilities.
    pub caps: BTreeSet<Capability>,
    /// `(authority_a, authority_b, shared_capability)` — non-empty iff
    /// the namespace is fractured.
    pub collisions: Vec<(String, String, String)>,
    /// Authorities that did not project (fail-closed surface, never
    /// silently dropped).
    pub unprojectable: Vec<String>,
}

impl GrantableSet {
    /// A clean set has no collisions and no unprojectable authorities —
    /// the precondition the §90.b deploy gate requires before checking
    /// `requires ⊆ grantable`.
    pub fn is_clean(&self) -> bool {
        self.collisions.is_empty() && self.unprojectable.is_empty()
    }
}

/// Build the grantable set from a catalog of authority strings (RBAC
/// colon perms ∪ reserved dotted caps ∪ SA-grantable). Projects each
/// through `π`, detecting namespace fractures (two authorities → one
/// capability) and unprojectable entries. Order-independent + total.
pub fn build_grantable_set<I, S>(authorities: I) -> GrantableSet
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    use std::collections::BTreeMap;
    let mut origin: BTreeMap<String, String> = BTreeMap::new();
    let mut caps: BTreeSet<Capability> = BTreeSet::new();
    let mut collisions: Vec<(String, String, String)> = Vec::new();
    let mut unprojectable: Vec<String> = Vec::new();

    for auth in authorities {
        let auth = auth.as_ref();
        match project_permission(auth) {
            Ok(cap) => {
                let key = cap.as_str().to_string();
                if let Some(prev) = origin.get(&key) {
                    // Only a genuine cross-authority collision (distinct
                    // source strings) fractures the namespace; the same
                    // authority listed twice is idempotent.
                    if prev != auth {
                        collisions.push((prev.clone(), auth.to_string(), key.clone()));
                    }
                } else {
                    origin.insert(key, auth.to_string());
                    caps.insert(cap);
                }
            }
            Err(ProjectionError::NotProjectable(s)) => unprojectable.push(s),
        }
    }
    GrantableSet {
        caps,
        collisions,
        unprojectable,
    }
}

/// Verdict of the grantability law: is every `requires:` scope a member
/// of the grantable set?
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantabilityVerdict {
    /// Every required capability is grantable.
    Grantable,
    /// One or more required capabilities are dead — no authority grants
    /// them. `axon-T891` at deploy.
    Ungrantable {
        ungrantable: Vec<String>,
        required: Vec<String>,
    },
}

/// The grantability law — `requires ⊆ grantable`. A `requires:` scope
/// absent from the grantable set is a DEAD boundary (§90 Modo-C): it can
/// be declared but never satisfied. Total; declaration order preserved
/// for diagnostic continuity (mirrors [`check_capabilities`]).
pub fn check_grantable(
    requires: &[String],
    grantable: &BTreeSet<Capability>,
) -> GrantabilityVerdict {
    let grantable_strs: BTreeSet<&str> = grantable.iter().map(|c| c.as_str()).collect();
    let ungrantable: Vec<String> = requires
        .iter()
        .filter(|r| !grantable_strs.contains(r.as_str()))
        .cloned()
        .collect();
    if ungrantable.is_empty() {
        GrantabilityVerdict::Grantable
    } else {
        GrantabilityVerdict::Ungrantable {
            ungrantable,
            required: requires.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderValue;

    fn jwt_with_caps(caps: &[&str]) -> String {
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\",\"typ\":\"JWT\"}");
        let payload_json = serde_json::json!({"capabilities": caps});
        let payload =
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload_json).unwrap());
        format!("{header}.{payload}.")
    }

    fn headers_with_auth(token: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        let value = format!("Bearer {token}");
        h.insert("authorization", HeaderValue::from_str(&value).unwrap());
        h
    }

    #[test]
    fn no_bearer_returns_empty_capabilities() {
        let h = HeaderMap::new();
        assert!(extract_capabilities_from_bearer(&h).is_empty());
    }

    #[test]
    fn malformed_token_returns_empty_capabilities() {
        let h = headers_with_auth("not-a-jwt");
        assert!(extract_capabilities_from_bearer(&h).is_empty());
    }

    #[test]
    fn token_without_capabilities_claim_returns_empty() {
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\"}");
        let payload = URL_SAFE_NO_PAD.encode(b"{\"sub\":\"alice\"}");
        let token = format!("{header}.{payload}.");
        let h = headers_with_auth(&token);
        assert!(extract_capabilities_from_bearer(&h).is_empty());
    }

    #[test]
    fn extracts_array_of_capabilities() {
        let h = headers_with_auth(&jwt_with_caps(&["admin", "legal.read"]));
        let caps = extract_capabilities_from_bearer(&h);
        assert_eq!(caps, vec!["admin".to_string(), "legal.read".to_string()]);
    }

    #[test]
    fn check_allows_empty_required() {
        // D9 backwards-compat path.
        let v = check_capabilities(&[], &[]);
        assert_eq!(v, AuthVerdict::Allow);
        let v = check_capabilities(&[], &["admin".to_string()]);
        assert_eq!(v, AuthVerdict::Allow);
    }

    #[test]
    fn check_allows_exact_match() {
        let v = check_capabilities(
            &["admin".to_string()],
            &["admin".to_string()],
        );
        assert_eq!(v, AuthVerdict::Allow);
    }

    #[test]
    fn check_allows_superset() {
        let v = check_capabilities(
            &["admin".to_string()],
            &["admin".to_string(), "other".to_string()],
        );
        assert_eq!(v, AuthVerdict::Allow);
    }

    #[test]
    fn check_denies_missing() {
        let v = check_capabilities(
            &["admin".to_string(), "legal.read".to_string()],
            &["admin".to_string()],
        );
        match v {
            AuthVerdict::Deny { missing, required, have } => {
                assert_eq!(missing, vec!["legal.read".to_string()]);
                assert_eq!(required.len(), 2);
                assert_eq!(have.len(), 1);
            }
            _ => panic!("expected Deny"),
        }
    }

    #[test]
    fn check_denies_empty_have() {
        let v = check_capabilities(
            &["admin".to_string()],
            &[],
        );
        match v {
            AuthVerdict::Deny { missing, .. } => {
                assert_eq!(missing, vec!["admin".to_string()]);
            }
            _ => panic!("expected Deny"),
        }
    }

    #[test]
    fn check_preserves_declaration_order_in_missing() {
        // Diagnostic continuity — adopter sees missing in source-
        // declaration order, not hash-table order.
        let v = check_capabilities(
            &[
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
                "d".to_string(),
            ],
            &["b".to_string()],
        );
        match v {
            AuthVerdict::Deny { missing, .. } => {
                assert_eq!(missing, vec!["a", "c", "d"]);
            }
            _ => panic!("expected Deny"),
        }
    }

    #[test]
    fn capabilities_claim_with_non_string_values_drops_them() {
        // Defensive: a malformed token containing mixed types in the
        // capabilities array drops the non-strings (extract_*) which
        // preserves the strict type contract — capabilities are
        // strings, period.
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\"}");
        let payload = URL_SAFE_NO_PAD.encode(
            b"{\"capabilities\":[\"admin\",42,null,\"legal.read\"]}",
        );
        let token = format!("{header}.{payload}.");
        let h = headers_with_auth(&token);
        let caps = extract_capabilities_from_bearer(&h);
        assert_eq!(caps, vec!["admin".to_string(), "legal.read".to_string()]);
    }

    #[test]
    fn slug_validator_round_trip_with_parser() {
        // Anchor: the runtime + parser share ONE predicate. Confirm
        // a sample of accepted + rejected slugs.
        assert!(is_valid_capability_slug("admin"));
        assert!(is_valid_capability_slug("legal.read"));
        assert!(!is_valid_capability_slug("Admin"));
        assert!(!is_valid_capability_slug("bank-officer"));
        assert!(!is_valid_capability_slug(""));
    }

    // ── §Fase 90.a — projection `π` + grantability law ──────────────

    /// The exact RBAC control-plane catalog shape (colon `resource:action`)
    /// the enterprise projects at mint. Kept in sync by the §90.f drift
    /// gate on the enterprise side; here a representative sample anchors
    /// the projection contract.
    const CATALOG_SAMPLE: &[&str] = &[
        "flow:execute",
        "flow:deploy",
        "tenant:update",
        "secret:read",
        "secret:write",
        "warden:execute",
        "savant:execute",
        "tech:dispatch",
        "tech:approve",
        "daemon:run",
    ];

    #[test]
    fn pi_projects_colon_perm_to_dotted_capability() {
        assert_eq!(project_permission("flow:execute").unwrap().as_str(), "flow.execute");
        assert_eq!(project_permission("tenant:update").unwrap().as_str(), "tenant.update");
        assert_eq!(project_permission("secret:read").unwrap().as_str(), "secret.read");
    }

    #[test]
    fn pi_is_identity_on_already_canonical_dotted_caps() {
        // Reserved owner-derived caps + cognitive-primitive caps are
        // already canonical — π must not disturb them.
        assert_eq!(project_permission("store.platform_read").unwrap().as_str(), "store.platform_read");
        assert_eq!(project_permission("chat.invoke").unwrap().as_str(), "chat.invoke");
        assert_eq!(project_permission("admin").unwrap().as_str(), "admin");
    }

    #[test]
    fn pi_is_total_over_the_catalog() {
        // Totality: every well-formed catalog perm has a canonical image.
        // The mint must never panic on a well-formed authority set.
        for perm in CATALOG_SAMPLE {
            let cap = project_permission(perm)
                .unwrap_or_else(|_| panic!("π must be defined on catalog perm `{perm}`"));
            assert!(is_valid_capability_slug(cap.as_str()));
        }
    }

    #[test]
    fn pi_is_injective_on_the_single_colon_catalog() {
        // Injectivity: distinct authorities never collapse to one
        // capability. Project the whole catalog sample and assert the
        // image set has the same cardinality as the (deduped) source.
        let images: BTreeSet<String> = CATALOG_SAMPLE
            .iter()
            .map(|p| project_permission(p).unwrap().into_string())
            .collect();
        let sources: BTreeSet<&str> = CATALOG_SAMPLE.iter().copied().collect();
        assert_eq!(images.len(), sources.len(), "π collapsed two distinct authorities");
    }

    #[test]
    fn pi_rejects_multi_colon_and_malformed() {
        // Fail-closed: not `resource:action` shape, or a candidate that
        // fails the slug grammar, is NotProjectable — never silently
        // coerced.
        assert!(matches!(project_permission("a:b:c"), Err(ProjectionError::NotProjectable(_))));
        assert!(matches!(project_permission("Flow:Execute"), Err(ProjectionError::NotProjectable(_))));
        assert!(matches!(project_permission("bank-officer:read"), Err(ProjectionError::NotProjectable(_))));
        assert!(matches!(project_permission(""), Err(ProjectionError::NotProjectable(_))));
    }

    #[test]
    fn build_grantable_set_is_clean_for_disjoint_catalog_plus_reserved() {
        // The real invariant verified against deployed code (§90 §1):
        // π(colon catalog) ∩ reserved-dotted = ∅. No `store:platform_*`
        // perm exists, so projecting the catalog + the reserved caps
        // yields no collision.
        let mut authorities: Vec<&str> = CATALOG_SAMPLE.to_vec();
        authorities.push("store.platform_read");
        authorities.push("store.platform_write");
        let g = build_grantable_set(authorities);
        assert!(g.is_clean(), "collisions={:?} unprojectable={:?}", g.collisions, g.unprojectable);
        assert!(g.caps.contains(&Capability::parse("flow.execute").unwrap()));
        assert!(g.caps.contains(&Capability::parse("store.platform_read").unwrap()));
    }

    #[test]
    fn build_grantable_set_detects_a_fractured_namespace() {
        // If the catalog ever gained a `store:platform_read` colon perm,
        // it would project to `store.platform_read` and COLLIDE with the
        // reserved dotted cap — a fracture the deploy gate must catch,
        // not silently resolve.
        let g = build_grantable_set(vec!["store:platform_read", "store.platform_read"]);
        assert!(!g.is_clean());
        assert_eq!(g.collisions.len(), 1);
        assert_eq!(g.collisions[0].2, "store.platform_read");
    }

    #[test]
    fn build_grantable_set_is_idempotent_on_duplicate_authority() {
        // The same authority listed twice is not a collision.
        let g = build_grantable_set(vec!["flow:execute", "flow:execute"]);
        assert!(g.is_clean());
        assert_eq!(g.caps.len(), 1);
    }

    #[test]
    fn grantability_law_admits_a_projected_requirement() {
        // The whole point: a `requires: [flow.execute]` is grantable
        // because `flow:execute` projects into the grantable set.
        let g = build_grantable_set(CATALOG_SAMPLE.to_vec());
        let v = check_grantable(&["flow.execute".to_string()], &g.caps);
        assert_eq!(v, GrantabilityVerdict::Grantable);
    }

    #[test]
    fn grantability_law_rejects_a_dead_requirement() {
        // Kivi brief #55: `requires: [tenant.write]` — but the catalog
        // has `tenant:update`, not `tenant:write`. `tenant.write` is a
        // DEAD boundary → axon-T891.
        let g = build_grantable_set(CATALOG_SAMPLE.to_vec());
        let v = check_grantable(&["tenant.write".to_string()], &g.caps);
        match v {
            GrantabilityVerdict::Ungrantable { ungrantable, .. } => {
                assert_eq!(ungrantable, vec!["tenant.write".to_string()]);
            }
            _ => panic!("expected Ungrantable — tenant.write is not grantable"),
        }
    }

    // ── §Fase 93.a — `π` lifted to held-authority sets ──────────────

    #[test]
    fn project_set_is_total_over_a_clean_held_set() {
        // The principal side of the law: built-in expansion + custom-role
        // rows + an already-dotted reserved cap all project, no drops.
        let p = project_permission_set(vec![
            "tenant:update",
            "secret:write",
            "store.platform_read",
        ]);
        assert!(p.is_total());
        assert!(p.capabilities.contains(&Capability::parse("tenant.update").unwrap()));
        assert!(p.capabilities.contains(&Capability::parse("secret.write").unwrap()));
        assert!(p.capabilities.contains(&Capability::parse("store.platform_read").unwrap()));
    }

    #[test]
    fn project_set_surfaces_drops_instead_of_swallowing() {
        // A tampered/legacy role_permissions row must degrade WITH a
        // witness — the verify-time caller logs `dropped`, never
        // silently narrows authority.
        let p = project_permission_set(vec!["tenant:update", "a:b:c", "Bad-Key"]);
        assert!(!p.is_total());
        assert_eq!(p.capabilities.len(), 1);
        assert_eq!(p.dropped.len(), 2);
        assert_eq!(p.dropped[0].0, "a:b:c");
        assert_eq!(p.dropped[1].0, "Bad-Key");
    }

    #[test]
    fn project_set_is_idempotent_and_collision_tolerant() {
        // Duplicates are idempotent, and — unlike the catalog gate — a
        // colon perm and its dotted twin held TOGETHER are harmless on
        // the principal side (the principal holds the capability either
        // way); the image is simply the set.
        let p = project_permission_set(vec!["flow:execute", "flow:execute", "flow.execute"]);
        assert!(p.is_total());
        assert_eq!(p.capabilities.len(), 1);
    }

    #[test]
    fn project_set_of_empty_is_empty_and_total() {
        let p = project_permission_set(Vec::<&str>::new());
        assert!(p.is_total());
        assert!(p.capabilities.is_empty());
    }

    #[test]
    fn grantability_law_preserves_declaration_order() {
        let g = build_grantable_set(CATALOG_SAMPLE.to_vec());
        let v = check_grantable(
            &["zzz.dead".to_string(), "flow.execute".to_string(), "aaa.dead".to_string()],
            &g.caps,
        );
        match v {
            GrantabilityVerdict::Ungrantable { ungrantable, .. } => {
                assert_eq!(ungrantable, vec!["zzz.dead".to_string(), "aaa.dead".to_string()]);
            }
            _ => panic!("expected Ungrantable"),
        }
    }
}
