//! v2.22.0 — Model capability catalog (declare the cognitive need, not the SKU).
//!
//! The single source of truth for *what each canonical provider's models can
//! do* — v1: the **context window** (in tokens). It backs:
//!
//! - the v2.22.0 pure resolver (`model_resolution::resolve_model`): given a
//!     step's declared `requires_context: N`, pick the smallest model whose
//!     window satisfies it (cost-optimal that-fits), or fail closed;
//! - the v2.22.0 `axon check` satisfiability gate (a `requires_context:` no
//!     canonical model could ever serve is a compile error);
//! - the enterprise v2.22.0 per-tenant catalog, which layers operator overrides
//!     on top of this canonical seed.
//!
//! **This is DATA, not a runtime probe.** The windows are the documented
//! provider values as of the knowledge cutoff; an operator running a self-hosted
//! or fine-tuned model with a different window overrides the catalog per-tenant
//! (the enterprise half) rather than the resolver guessing. The catalog is
//! drift-gated by the unit tests below + the v2.22.0 compile check.
//!
//! Scope note: a provider exposes many models; the catalog lists the ones the
//! resolver may *choose among* for capability satisfaction. For providers whose
//! default already has a very large window (gemini 1M, anthropic 200k) a single
//! entry suffices in v1; kimi — the brief-#36 provider — carries its full
//! `moonshot-v1-{8k,32k,128k}` context ladder because that ladder is exactly the
//! choice the resolver must make. More models are additive (a new `ModelCap`
//! row), never a breaking change.

/// One model's declared capabilities. v1 carries the context window only; the
/// struct is the extension point for future capability tiers (reasoning, vision,
/// tool-use) — `Backend::supports(Capability, model)` already exists to back them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelCap {
    /// The provider-native model identifier (the wire `model` string).
    pub name: &'static str,
    /// Maximum input+output context window, in tokens.
    pub context_window: u32,
    /// `true` for the provider's default model (the one used when a step
    /// declares no `requires_context:` and the operator set no override).
    pub is_default: bool,
}

// ── Per-provider catalogs ────────────────────────────────────────────────────
//
// Documented context windows as of the knowledge cutoff. Kept deliberately
// conservative + provider-native. `is_default` mirrors each backend's
// `default_model()` so the catalog and the live default never disagree (the
// `catalog_default_matches_backend_default` drift test pins it).

const ANTHROPIC: &[ModelCap] = &[ModelCap {
    name: "claude-3-5-haiku-latest",
    context_window: 200_000,
    is_default: true,
}];

const OPENAI: &[ModelCap] = &[ModelCap {
    name: "gpt-4o-mini",
    context_window: 128_000,
    is_default: true,
}];

const GEMINI: &[ModelCap] = &[ModelCap {
    name: "gemini-2.5-flash",
    context_window: 1_048_576,
    is_default: true,
}];

// The brief-#36 provider — the full moonshot-v1 context ladder, since this is
// exactly the choice the resolver makes (`requires_context: 16000` → -32k).
const KIMI: &[ModelCap] = &[
    ModelCap {
        name: "moonshot-v1-8k",
        context_window: 8_192,
        is_default: true,
    },
    ModelCap {
        name: "moonshot-v1-32k",
        context_window: 32_768,
        is_default: false,
    },
    ModelCap {
        name: "moonshot-v1-128k",
        context_window: 131_072,
        is_default: false,
    },
];

const GLM: &[ModelCap] = &[ModelCap {
    name: "glm-4-plus",
    context_window: 128_000,
    is_default: true,
}];

const OPENROUTER: &[ModelCap] = &[ModelCap {
    name: "openai/gpt-4o-mini",
    context_window: 128_000,
    is_default: true,
}];

const OLLAMA: &[ModelCap] = &[ModelCap {
    name: "llama3.1:8b",
    context_window: 131_072,
    is_default: true,
}];

/// The canonical catalog for a provider, or `&[]` for an unknown / custom /
/// operator-registered backend (the resolver then falls back to the backend
/// default — never guesses a window).
pub fn models_for(backend: &str) -> &'static [ModelCap] {
    match backend {
        "anthropic" => ANTHROPIC,
        "openai" => OPENAI,
        "gemini" => GEMINI,
        "kimi" => KIMI,
        "glm" => GLM,
        "openrouter" => OPENROUTER,
        "ollama" => OLLAMA,
        _ => &[],
    }
}

/// The context window of a specific `(backend, model)`, or `None` when the model
/// is not in the canonical catalog (a custom model — the operator declares its
/// window per-tenant in the enterprise catalog).
pub fn context_window(backend: &str, model: &str) -> Option<u32> {
    models_for(backend)
        .iter()
        .find(|m| m.name == model)
        .map(|m| m.context_window)
}

/// The largest context window any canonical model offers (across all providers).
/// Backs the v2.22.0 compile-time ceiling: a `requires_context:` above this can
/// never be satisfied by any canonical backend, so it is a hard `axon check`
/// error rather than a deploy-time surprise.
pub fn max_canonical_context_window() -> u32 {
    crate::backends::CANONICAL_PROVIDERS
        .iter()
        .flat_map(|p| models_for(p))
        .map(|m| m.context_window)
        .max()
        .unwrap_or(0)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::CANONICAL_PROVIDERS;

    #[test]
    fn every_canonical_provider_has_a_catalog_with_exactly_one_default() {
        for p in CANONICAL_PROVIDERS {
            let models = models_for(p);
            assert!(!models.is_empty(), "{p} must have a catalog");
            let defaults = models.iter().filter(|m| m.is_default).count();
            assert_eq!(defaults, 1, "{p} must have EXACTLY one default model");
        }
    }

    #[test]
    fn catalog_default_matches_each_backend_default_model() {
        // The catalog's `is_default` model must equal the live
        // `Backend::default_model()` — else the resolver and the runtime
        // disagree about the no-requirement model (the v2.22.0 drift guard).
        use crate::backends::Backend;
        let pairs: Vec<(&str, Box<dyn Backend>)> = vec![
            ("anthropic", Box::new(crate::backends::AnthropicBackend::with_api_key(Some("k".into())))),
            ("openai", Box::new(crate::backends::OpenAIBackend::with_api_key(Some("k".into())))),
            ("gemini", Box::new(crate::backends::GeminiBackend::with_api_key(Some("k".into())))),
            ("kimi", Box::new(crate::backends::KimiBackend::with_api_key(Some("k".into())))),
            ("glm", Box::new(crate::backends::GLMBackend::with_api_key(Some("k".into())))),
            ("openrouter", Box::new(crate::backends::OpenRouterBackend::with_api_key(Some("k".into())))),
            ("ollama", Box::new(crate::backends::OllamaBackend::with_api_key(Some("k".into())))),
        ];
        for (name, backend) in pairs {
            let catalog_default = models_for(name)
                .iter()
                .find(|m| m.is_default)
                .map(|m| m.name)
                .unwrap_or("<none>");
            assert_eq!(
                catalog_default,
                backend.default_model(),
                "{name}: catalog default must equal the live backend default"
            );
        }
    }

    #[test]
    fn kimi_carries_the_full_moonshot_context_ladder() {
        // The brief-#36 case: the resolver must be able to pick among 8k/32k/128k.
        assert_eq!(context_window("kimi", "moonshot-v1-8k"), Some(8_192));
        assert_eq!(context_window("kimi", "moonshot-v1-32k"), Some(32_768));
        assert_eq!(context_window("kimi", "moonshot-v1-128k"), Some(131_072));
    }

    #[test]
    fn context_window_is_strictly_increasing_within_a_provider_ladder() {
        // A provider's catalog is ordered smallest-window-first so the v2.22.0
        // resolver's "smallest that fits" is a single forward scan.
        for p in CANONICAL_PROVIDERS {
            let windows: Vec<u32> = models_for(p).iter().map(|m| m.context_window).collect();
            let mut sorted = windows.clone();
            sorted.sort_unstable();
            assert_eq!(windows, sorted, "{p} catalog must be smallest-window-first");
        }
    }

    #[test]
    fn unknown_backend_has_empty_catalog_and_no_window() {
        assert!(models_for("custom-self-hosted").is_empty());
        assert_eq!(context_window("custom-self-hosted", "x"), None);
        assert_eq!(context_window("kimi", "moonshot-v1-512k"), None);
    }

    #[test]
    fn max_canonical_window_is_the_largest_known() {
        // gemini-2.0-flash (1,048,576) is currently the largest canonical window.
        assert_eq!(max_canonical_context_window(), 1_048_576);
    }
}
