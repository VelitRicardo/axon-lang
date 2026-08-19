//! v1.31.0 — the Backend Resolution Contract (D1).
//!
//! axon resolves the execution backend of any flow — behind an
//! `axonendpoint` route or a `/v1/execute` call — through ONE
//! deterministic, total, published precedence ladder. The first rung
//! that yields a usable concrete backend wins:
//!
//!   1. **request-explicit** — a concrete backend named on the request
//!   2. **endpoint-declared** — the `axonendpoint backend:` field
//!   3. **server-default** — `axon serve --backend` / `AXON_DEFAULT_BACKEND`
//!   4. **environment-available `auto`** — the registry-ranked list if
//!      non-empty, else the canonical providers whose API key is
//!      present in the environment, in canonical priority order
//!   5. **honest failure** — `Err(NoBackendAvailable)`; the ladder
//!      NEVER falls through to `stub`
//!
//! A rung carrying `"auto"` (or empty) is *transparent* — it does not
//! fire, it falls through to the next rung. `stub` is reachable ONLY
//! by an explicit rung-1/2/3 value of `"stub"` — never from the `auto`
//! rungs (D5: `stub` is filtered out of the registry / env lists here,
//! so auto-resolution can never land on it).
//!
//! This module is **pure** — no I/O, no `std::env`, no clock. The
//! environment scan (which keys are present) and the registry scoring
//! are computed by the caller and passed in; that keeps the contract
//! exhaustively unit-testable and deterministic. The shared corpus in
//! the v1.31.0 tests pins it.

/// Is `name` an explicit, concrete backend choice — i.e. a rung that
/// should FIRE rather than fall through? Empty and `"auto"` are
/// transparent (they mean "resolve normally").
pub fn is_explicit_backend(name: &str) -> bool {
    !name.is_empty() && name != "auto"
}

/// Which rung of the precedence ladder resolved the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendResolutionReason {
    /// Rung 1 — a concrete backend named on the request.
    RequestExplicit,
    /// Rung 2 — the `axonendpoint backend:` declaration.
    EndpointDeclared,
    /// Rung 3 — the server-wide default.
    ServerDefault,
    /// Rung 4a — the top of the operator-tuned `backend_registry` scores.
    RegistryRanked,
    /// Rung 4b — the first canonical provider with an API key in the env.
    EnvironmentAvailable,
}

impl BackendResolutionReason {
    /// Stable wire/observability slug (D8). The closed catalog.
    pub fn as_slug(self) -> &'static str {
        match self {
            BackendResolutionReason::RequestExplicit => "request_explicit",
            BackendResolutionReason::EndpointDeclared => "endpoint_declared",
            BackendResolutionReason::ServerDefault => "server_default",
            BackendResolutionReason::RegistryRanked => "registry_ranked",
            BackendResolutionReason::EnvironmentAvailable => {
                "environment_available"
            }
        }
    }
}

/// A resolved backend + the rung that chose it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendResolution {
    pub backend: String,
    pub reason: BackendResolutionReason,
}

/// The honest-failure outcome (D5) — every ladder rung was empty and
/// `stub` was not explicitly requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoBackendAvailable;

impl std::fmt::Display for NoBackendAvailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no execution backend available — axon will not silently \
             run the no-op `stub`. Fix one of: declare `backend:` on \
             the axonendpoint; set a provider API key in the server \
             environment (ANTHROPIC_API_KEY / OPENAI_API_KEY / \
             GEMINI_API_KEY / KIMI_API_KEY / GLM_API_KEY / \
             OPENROUTER_API_KEY / OLLAMA_API_KEY); pass `--backend \
             <name>` to `axon serve`; or request `backend=stub` \
             explicitly to opt into the no-op."
        )
    }
}

impl std::error::Error for NoBackendAvailable {}

/// The inputs to one backend resolution — all pre-computed by the
/// caller so this contract stays pure.
#[derive(Debug, Clone, Default)]
pub struct BackendResolutionInputs {
    /// Rung 1 — a backend named on the request. `None`, empty, or
    /// `"auto"` = no request preference.
    pub request_backend: Option<String>,
    /// Rung 2 — the `axonendpoint backend:` declaration, if any.
    pub endpoint_backend: Option<String>,
    /// Rung 3 — the server-wide default, if configured.
    pub server_default: Option<String>,
    /// Rung 4a — registry-scored backends, best-first (from
    /// `compute_backend_scores`). `stub` entries are ignored.
    pub registry_ranked: Vec<String>,
    /// Rung 4b — canonical providers with an API key present in the
    /// environment, in canonical priority order. `stub` is never here.
    pub env_available: Vec<String>,
}

/// D1 — resolve the execution backend by the precedence ladder.
///
/// Total and deterministic: the same inputs always produce the same
/// result. `stub` is returned ONLY when an explicit rung-1/2/3 value
/// is literally `"stub"`; the `auto` rungs skip every `"stub"` entry,
/// so auto-resolution can never silently degrade to the no-op (D5).
pub fn resolve_backend(
    inputs: &BackendResolutionInputs,
) -> Result<BackendResolution, NoBackendAvailable> {
    // Rungs 1–3 — an explicit, concrete declaration fires immediately.
    for (slot, reason) in [
        (&inputs.request_backend, BackendResolutionReason::RequestExplicit),
        (&inputs.endpoint_backend, BackendResolutionReason::EndpointDeclared),
        (&inputs.server_default, BackendResolutionReason::ServerDefault),
    ] {
        if let Some(name) = slot {
            if is_explicit_backend(name) {
                return Ok(BackendResolution {
                    backend: name.clone(),
                    reason,
                });
            }
        }
    }

    // Rung 4 — `auto` resolution. The registry's operator-tuned
    // ranking wins when populated; else the environment-available
    // providers. An auto rung fires ONLY on a usable CONCRETE backend
    // — `is_explicit_backend` (non-empty, not `"auto"`) AND not
    // `"stub"`. So the transparent tokens and the no-op are skipped:
    // auto-resolution can never land on `stub` (D5), nor on an empty
    // / `"auto"` entry should one slip into the caller's list. The
    // resolver stays total — it never returns a non-backend.
    let is_usable_auto = |b: &&String| {
        is_explicit_backend(b.as_str()) && b.as_str() != "stub"
    };
    if let Some(top) = inputs.registry_ranked.iter().find(is_usable_auto) {
        return Ok(BackendResolution {
            backend: top.clone(),
            reason: BackendResolutionReason::RegistryRanked,
        });
    }
    if let Some(top) = inputs.env_available.iter().find(is_usable_auto) {
        return Ok(BackendResolution {
            backend: top.clone(),
            reason: BackendResolutionReason::EnvironmentAvailable,
        });
    }

    // Rung 5 — honest failure. No silent stub.
    Err(NoBackendAvailable)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> BackendResolutionInputs {
        BackendResolutionInputs::default()
    }

    #[test]
    fn is_explicit_rejects_auto_and_empty() {
        assert!(!is_explicit_backend(""));
        assert!(!is_explicit_backend("auto"));
        assert!(is_explicit_backend("gemini"));
        assert!(is_explicit_backend("stub"));
    }

    #[test]
    fn request_explicit_wins_over_everything() {
        let mut i = inputs();
        i.request_backend = Some("kimi".into());
        i.endpoint_backend = Some("gemini".into());
        i.server_default = Some("anthropic".into());
        i.registry_ranked = vec!["openai".into()];
        let r = resolve_backend(&i).unwrap();
        assert_eq!(r.backend, "kimi");
        assert_eq!(r.reason, BackendResolutionReason::RequestExplicit);
    }

    #[test]
    fn endpoint_declared_wins_when_request_is_auto() {
        let mut i = inputs();
        i.request_backend = Some("auto".into()); // transparent
        i.endpoint_backend = Some("gemini".into());
        i.server_default = Some("anthropic".into());
        let r = resolve_backend(&i).unwrap();
        assert_eq!(r.backend, "gemini");
        assert_eq!(r.reason, BackendResolutionReason::EndpointDeclared);
    }

    #[test]
    fn server_default_wins_when_request_and_endpoint_absent() {
        let mut i = inputs();
        i.server_default = Some("anthropic".into());
        i.env_available = vec!["gemini".into()];
        let r = resolve_backend(&i).unwrap();
        assert_eq!(r.backend, "anthropic");
        assert_eq!(r.reason, BackendResolutionReason::ServerDefault);
    }

    #[test]
    fn registry_ranked_wins_over_env_in_auto_mode() {
        let mut i = inputs();
        i.registry_ranked = vec!["openai".into(), "kimi".into()];
        i.env_available = vec!["gemini".into()];
        let r = resolve_backend(&i).unwrap();
        assert_eq!(r.backend, "openai");
        assert_eq!(r.reason, BackendResolutionReason::RegistryRanked);
    }

    #[test]
    fn environment_available_resolves_when_registry_empty() {
        let mut i = inputs();
        i.env_available = vec!["gemini".into(), "anthropic".into()];
        let r = resolve_backend(&i).unwrap();
        assert_eq!(r.backend, "gemini");
        assert_eq!(r.reason, BackendResolutionReason::EnvironmentAvailable);
    }

    #[test]
    fn empty_and_auto_slots_are_transparent() {
        let mut i = inputs();
        i.request_backend = Some(String::new());
        i.endpoint_backend = Some("auto".into());
        i.server_default = Some("kimi".into());
        let r = resolve_backend(&i).unwrap();
        assert_eq!(r.backend, "kimi");
        assert_eq!(r.reason, BackendResolutionReason::ServerDefault);
    }

    #[test]
    fn no_backend_anywhere_is_honest_failure_never_stub() {
        // Every rung empty — D5: honest failure, not a silent stub.
        assert_eq!(resolve_backend(&inputs()), Err(NoBackendAvailable));
    }

    #[test]
    fn auto_rungs_never_land_on_stub() {
        // D5 — even if `stub` somehow appears in the registry / env
        // lists, the `auto` rungs skip it. A stub entry alone with no
        // real backend is an honest failure.
        let mut i = inputs();
        i.registry_ranked = vec!["stub".into()];
        i.env_available = vec!["stub".into()];
        assert_eq!(resolve_backend(&i), Err(NoBackendAvailable));

        // …but a real backend behind a stub entry still resolves.
        i.registry_ranked = vec!["stub".into(), "gemini".into()];
        assert_eq!(resolve_backend(&i).unwrap().backend, "gemini");
    }

    #[test]
    fn stub_is_reachable_only_by_an_explicit_rung() {
        // An operator who explicitly asks for stub gets it — D5 forbids
        // SILENT stub, not explicit opt-in.
        let mut i = inputs();
        i.request_backend = Some("stub".into());
        let r = resolve_backend(&i).unwrap();
        assert_eq!(r.backend, "stub");
        assert_eq!(r.reason, BackendResolutionReason::RequestExplicit);
    }

    #[test]
    fn resolution_is_deterministic() {
        let mut i = inputs();
        i.endpoint_backend = Some("gemini".into());
        i.env_available = vec!["anthropic".into(), "kimi".into()];
        let a = resolve_backend(&i).unwrap();
        let b = resolve_backend(&i).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn reason_slugs_are_the_closed_catalog() {
        for (reason, slug) in [
            (BackendResolutionReason::RequestExplicit, "request_explicit"),
            (BackendResolutionReason::EndpointDeclared, "endpoint_declared"),
            (BackendResolutionReason::ServerDefault, "server_default"),
            (BackendResolutionReason::RegistryRanked, "registry_ranked"),
            (
                BackendResolutionReason::EnvironmentAvailable,
                "environment_available",
            ),
        ] {
            assert_eq!(reason.as_slug(), slug);
        }
    }

    #[test]
    fn honest_failure_message_names_the_fixes() {
        let msg = NoBackendAvailable.to_string();
        assert!(msg.contains("backend:"));
        assert!(msg.contains("ANTHROPIC_API_KEY"));
        assert!(msg.contains("--backend"));
        assert!(msg.contains("stub"));
    }
}
