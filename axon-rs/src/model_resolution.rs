//! v2.22.0 — pure model resolution (the capability resolver).
//!
//! Given a step's declared `requires_context: N` (the v2.22.0 grammar) and the
//! resolved backend's model catalog (the v2.22.0 `ModelCap` list), pick the
//! concrete model that satisfies the requirement — or fail closed. The doctrine
//! (Kivi brief #36):
//!
//! - **smallest-model-that-fits**: the cheapest model whose context
//!     window is `>= requires_context`. The adopter declares the floor; the
//!     resolver optimises above it (a bigger model wastes money).
//! - **fail closed, never downgrade**: when nothing satisfies, return
//!     `NoModelSatisfies` — NEVER a too-small model that 400s in production. This
//! is the v1.31.0 "no silent stub" discipline, for models.
//! - **`None` requirement = backend default** (the design decision, back-compat): a step with
//!     no `requires_context:` resolves to the empty model string, which the
//!     `ChatRequest` layer reads as "use the backend's `default_model()`" — every
//! pre-v2.22.0 flow is byte-identical.
//!
//! Pure + total + no I/O/env/clock (the `backend_resolution.rs` mould): the
//! caller passes the catalog, so the enterprise v2.22.0 per-tenant catalog reuses
//! this exact function with different data — identical behaviour, governed source.

use crate::backends::model_catalog::ModelCap;

/// Why a model was chosen (the closed observability catalog).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelResolutionReason {
    /// A `requires_context:` selected the smallest satisfying model.
    RequirementSatisfied,
    /// No requirement was declared → the backend default (empty model string).
    BackendDefault,
}

impl ModelResolutionReason {
    /// Stable wire/observability slug.
    pub fn as_slug(self) -> &'static str {
        match self {
            ModelResolutionReason::RequirementSatisfied => "requirement_satisfied",
            ModelResolutionReason::BackendDefault => "backend_default",
        }
    }
}

/// A resolved model + the rung that chose it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModel {
    /// The provider-native model id, OR the empty string meaning "let the backend
    /// use its `default_model()`" (the back-compat path for a no-requirement step).
    pub model: String,
    pub reason: ModelResolutionReason,
}

/// The honest-failure outcome: the step needs more context than any model
/// in the resolved backend's catalog can serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoModelSatisfies {
    /// The context window the step declared it needs.
    pub requires_context: u32,
    /// The largest window any catalog model offers (`None` when the catalog is
    /// empty — a custom/unknown backend whose models must be declared per-tenant).
    pub largest_available: Option<u32>,
}

impl std::fmt::Display for NoModelSatisfies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.largest_available {
            Some(max) => write!(
                f,
                "no model satisfies the step's `requires_context: {}` — the resolved \
                 backend's largest context window is {max}. Lower the requirement, or \
                 configure a larger-context backend/model for this deployment.",
                self.requires_context
            ),
            None => write!(
                f,
                "no model satisfies the step's `requires_context: {}` — the resolved \
                 backend has no known model catalog (a custom backend). Declare the \
                 model's context window for this tenant, or use a canonical backend.",
                self.requires_context
            ),
        }
    }
}

impl std::error::Error for NoModelSatisfies {}

/// the design decision/3/4 — resolve the model for a step against a backend's catalog.
///
/// `models` is the resolved backend's `ModelCap` list (canonical v2.22.0 or the
/// enterprise per-tenant catalog), assumed smallest-window-first (the v2.22.0
/// invariant — verified by the catalog's own ordering test). `requires_context`
/// is the step's declared need (`None` → no requirement).
pub fn resolve_model(
    models: &[ModelCap],
    requires_context: Option<u32>,
) -> Result<ResolvedModel, NoModelSatisfies> {
    let need = match requires_context {
        // the design decision — no requirement: the backend default (empty model string is the
        // `ChatRequest` "use default" sentinel). Byte-identical to every pre-v2.22.0 flow.
        None => {
            return Ok(ResolvedModel {
                model: String::new(),
                reason: ModelResolutionReason::BackendDefault,
            })
        }
        Some(n) => n,
    };

    // the design decision — the smallest model whose window fits. The catalog is smallest-first,
    // so the first fit IS the smallest fit (a single forward scan).
    if let Some(m) = models.iter().find(|m| m.context_window >= need) {
        return Ok(ResolvedModel {
            model: m.name.to_string(),
            reason: ModelResolutionReason::RequirementSatisfied,
        });
    }

    // the design decision — fail closed. Never pick a too-small model.
    Err(NoModelSatisfies {
        requires_context: need,
        largest_available: models.iter().map(|m| m.context_window).max(),
    })
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::model_catalog::models_for;

    #[test]
    fn none_requirement_resolves_to_the_backend_default_empty_string() {
        // Back-compat: no requirement → empty model → backend default.
        let r = resolve_model(models_for("kimi"), None).unwrap();
        assert_eq!(r.model, "");
        assert_eq!(r.reason, ModelResolutionReason::BackendDefault);
    }

    #[test]
    fn kimi_16k_requirement_picks_the_smallest_that_fits_32k() {
        // The brief-#36 case: 16000 tokens against kimi's 8k/32k/128k ladder →
        // moonshot-v1-32k (8k too small, 32k fits and is cheaper than 128k).
        let r = resolve_model(models_for("kimi"), Some(16_000)).unwrap();
        assert_eq!(r.model, "moonshot-v1-32k");
        assert_eq!(r.reason, ModelResolutionReason::RequirementSatisfied);
    }

    #[test]
    fn exact_boundary_fits_the_equal_window_model() {
        // `>=` — a requirement equal to a window is satisfied by that model.
        let r = resolve_model(models_for("kimi"), Some(8_192)).unwrap();
        assert_eq!(r.model, "moonshot-v1-8k");
        let r = resolve_model(models_for("kimi"), Some(32_768)).unwrap();
        assert_eq!(r.model, "moonshot-v1-32k");
    }

    #[test]
    fn just_over_a_boundary_steps_up_to_the_next_model() {
        let r = resolve_model(models_for("kimi"), Some(8_193)).unwrap();
        assert_eq!(r.model, "moonshot-v1-32k");
        let r = resolve_model(models_for("kimi"), Some(32_769)).unwrap();
        assert_eq!(r.model, "moonshot-v1-128k");
    }

    #[test]
    fn unsatisfiable_requirement_fails_closed_never_downgrades() {
        // 200k > kimi's largest (128k) → honest failure, NOT moonshot-v1-128k.
        let err = resolve_model(models_for("kimi"), Some(200_000)).unwrap_err();
        assert_eq!(err.requires_context, 200_000);
        assert_eq!(err.largest_available, Some(131_072));
        assert!(err.to_string().contains("131072"));
    }

    #[test]
    fn empty_catalog_with_a_requirement_fails_closed_with_no_largest() {
        // A custom backend (no catalog) cannot prove satisfaction → fail closed.
        let err = resolve_model(&[], Some(4_096)).unwrap_err();
        assert_eq!(err.largest_available, None);
        assert!(err.to_string().contains("custom backend"));
    }

    #[test]
    fn empty_catalog_with_no_requirement_still_resolves_to_default() {
        // A custom backend with NO declared requirement is fine — backend default.
        let r = resolve_model(&[], None).unwrap();
        assert_eq!(r.model, "");
        assert_eq!(r.reason, ModelResolutionReason::BackendDefault);
    }

    #[test]
    fn large_context_backend_satisfies_from_its_single_default() {
        // gemini-2.5-flash (1M) satisfies a big requirement from its one entry.
        let r = resolve_model(models_for("gemini"), Some(500_000)).unwrap();
        assert_eq!(r.model, "gemini-2.5-flash");
        assert_eq!(r.reason, ModelResolutionReason::RequirementSatisfied);
    }
}
