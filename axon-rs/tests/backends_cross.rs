//! v1.18.0 — Cross-backend Rust integration tests.
//!
//! Exercises the full registry of native Rust backends through the
//! public API surface. Distinct from the per-provider unit tests
//! (which live in `src/backends/<provider>.rs::tests`) — these tests
//! verify that:
//!
//!   1. All 7 backends construct via their `from_env()` factory
//!      without panicking, even when their env var is unset.
//!   2. The `Registry::production()` constructor populates all 7.
//!   3. `provider_names()` returns the canonical sorted set.
//!   4. Each backend's `name()` matches the expected canonical short
//!      name (the same string Python's `BACKEND_REGISTRY` uses).
//!   5. Each backend's `default_model()` matches the documented preset.
//!   6. Capability discovery is consistent with the per-provider
//!      docstrings (Vision / LockedParams / SafetySettings / etc.).
//!   7. The shared `tokens::count_tokens` dispatch is reachable through
//!      every backend.
//!
//! Live HTTP integration tests (smoke against real provider endpoints)
//! are deferred to a 24.j.2 follow-up and live in adopters' CI — they
//! require credentials + are flaky against rate-limited endpoints.
//! Marking them `#[ignore]` here would force CI to skip without
//! visibility; better to keep them in dedicated infrastructure where
//! the failure signal is meaningful.

use axon::backends::{
    AnthropicBackend, Backend, Capability, GLMBackend, GeminiBackend, KimiBackend,
    OllamaBackend, OpenAIBackend, OpenRouterBackend, Registry,
};

// ────────────────────────────────────────────────────────────────────
//  Construction — factories don't panic when env vars are unset
// ────────────────────────────────────────────────────────────────────

#[test]
fn anthropic_from_env_constructs_without_panic() {
    let _ = AnthropicBackend::from_env();
}

#[test]
fn openai_from_env_constructs_without_panic() {
    let _ = OpenAIBackend::from_env();
}

#[test]
fn gemini_from_env_constructs_without_panic() {
    let _ = GeminiBackend::from_env();
}

#[test]
fn kimi_from_env_constructs_without_panic() {
    let _ = KimiBackend::from_env();
}

#[test]
fn glm_from_env_constructs_without_panic() {
    let _ = GLMBackend::from_env();
}

#[test]
fn ollama_from_env_constructs_without_panic() {
    let _ = OllamaBackend::from_env();
}

#[test]
fn openrouter_from_env_constructs_without_panic() {
    let _ = OpenRouterBackend::from_env();
}

// ────────────────────────────────────────────────────────────────────
//  Registry — production() populates all 7 backends
// ────────────────────────────────────────────────────────────────────

#[test]
fn registry_production_holds_all_seven_backends() {
    let r = Registry::production();
    assert_eq!(r.len(), 7);
}

#[test]
fn registry_production_provider_names_match_python_canonical_set() {
    // The v1.18.0 drift gate (`tests/test_fase24_backend_parity.py`)
    // asserts the same set against Python's `BACKEND_REGISTRY` keys.
    // This test is the Rust-side counterpart.
    let r = Registry::production();
    let expected = [
        "anthropic",
        "gemini",
        "glm",
        "kimi",
        "ollama",
        "openai",
        "openrouter",
    ];
    let names = r.provider_names();
    assert_eq!(names.len(), expected.len());
    for name in &expected {
        assert!(
            names.iter().any(|n| n == name),
            "provider {name} missing from Registry::production()",
        );
    }
}

#[test]
fn registry_production_lookups_succeed_for_all_seven() {
    let r = Registry::production();
    for name in &["anthropic", "gemini", "glm", "kimi", "ollama", "openai", "openrouter"] {
        let backend = r.get(name);
        assert!(backend.is_some(), "Registry::production() missing {name}");
        let backend = backend.unwrap();
        assert_eq!(backend.name(), *name);
    }
}

// ────────────────────────────────────────────────────────────────────
//  Default models — documented presets per provider
// ────────────────────────────────────────────────────────────────────

#[test]
fn default_models_match_documented_presets() {
    // The presets are documented in the per-provider docstrings + in
    // `OpenAICompatConfig`'s factory functions. This test pins them
    // so a future change to a default forces a deliberate review.
    let cases: &[(&dyn Fn() -> Box<dyn Backend>, &str)] = &[
        (&|| Box::new(AnthropicBackend::from_env()), "claude-3-5-haiku-latest"),
        (&|| Box::new(OpenAIBackend::from_env()), "gpt-4o-mini"),
        (&|| Box::new(GeminiBackend::from_env()), "gemini-2.5-flash"),
        (&|| Box::new(KimiBackend::from_env()), "moonshot-v1-8k"),
        (&|| Box::new(GLMBackend::from_env()), "glm-4-plus"),
        (&|| Box::new(OllamaBackend::from_env()), "llama3.1:8b"),
        (&|| Box::new(OpenRouterBackend::from_env()), "openai/gpt-4o-mini"),
    ];
    for (factory, expected) in cases {
        let backend = factory();
        assert_eq!(
            backend.default_model(),
            *expected,
            "default model drift for {}",
            backend.name(),
        );
    }
}

// ────────────────────────────────────────────────────────────────────
//  Capability matrix — cross-backend consistency check
// ────────────────────────────────────────────────────────────────────

#[test]
fn streaming_supported_by_all_backends() {
    // Every backend reports Streaming = true for its default model.
    let backends: Vec<Box<dyn Backend>> = vec![
        Box::new(AnthropicBackend::from_env()),
        Box::new(OpenAIBackend::from_env()),
        Box::new(GeminiBackend::from_env()),
        Box::new(KimiBackend::from_env()),
        Box::new(GLMBackend::from_env()),
        Box::new(OllamaBackend::from_env()),
        Box::new(OpenRouterBackend::from_env()),
    ];
    for b in backends {
        let model = b.default_model().to_string();
        assert!(
            b.supports(Capability::Streaming, &model),
            "{} should support Streaming",
            b.name(),
        );
    }
}

#[test]
fn tool_use_supported_by_all_backends() {
    let backends: Vec<Box<dyn Backend>> = vec![
        Box::new(AnthropicBackend::from_env()),
        Box::new(OpenAIBackend::from_env()),
        Box::new(GeminiBackend::from_env()),
        Box::new(KimiBackend::from_env()),
        Box::new(GLMBackend::from_env()),
        Box::new(OllamaBackend::from_env()),
        Box::new(OpenRouterBackend::from_env()),
    ];
    for b in backends {
        let model = b.default_model().to_string();
        assert!(
            b.supports(Capability::ToolUse, &model),
            "{} should support ToolUse",
            b.name(),
        );
    }
}

#[test]
fn prompt_caching_only_anthropic() {
    let any_model = "claude-sonnet-4-5";
    assert!(AnthropicBackend::from_env().supports(Capability::PromptCaching, any_model));
    assert!(!OpenAIBackend::from_env().supports(Capability::PromptCaching, "gpt-4o-mini"));
    assert!(!GeminiBackend::from_env().supports(Capability::PromptCaching, "gemini-2.5-flash"));
    assert!(!KimiBackend::from_env().supports(Capability::PromptCaching, "moonshot-v1-8k"));
    assert!(!GLMBackend::from_env().supports(Capability::PromptCaching, "glm-4-plus"));
    assert!(!OllamaBackend::from_env().supports(Capability::PromptCaching, "llama3.1:8b"));
    assert!(!OpenRouterBackend::from_env()
        .supports(Capability::PromptCaching, "openai/gpt-4o-mini"));
}

#[test]
fn safety_settings_only_gemini() {
    assert!(GeminiBackend::from_env().supports(Capability::SafetySettings, "gemini-2.5-flash"));
    assert!(!AnthropicBackend::from_env()
        .supports(Capability::SafetySettings, "claude-sonnet-4-5"));
    assert!(!OpenAIBackend::from_env().supports(Capability::SafetySettings, "gpt-4o-mini"));
    assert!(!KimiBackend::from_env().supports(Capability::SafetySettings, "moonshot-v1-8k"));
    assert!(!GLMBackend::from_env().supports(Capability::SafetySettings, "glm-4-plus"));
    assert!(!OllamaBackend::from_env().supports(Capability::SafetySettings, "llama3.1:8b"));
    assert!(!OpenRouterBackend::from_env()
        .supports(Capability::SafetySettings, "openai/gpt-4o-mini"));
}

#[test]
fn structured_output_supported_by_openai_compat_family() {
    // OpenAI / Kimi / GLM / Ollama / OpenRouter all expose structured
    // outputs through the shared base; Gemini supports it natively;
    // Anthropic does not have an equivalent first-class field
    // (response_format on the messages API is not StructuredOutput
    // per the canonical OpenAI definition).
    assert!(OpenAIBackend::from_env().supports(Capability::StructuredOutput, "gpt-4o-mini"));
    assert!(KimiBackend::from_env().supports(Capability::StructuredOutput, "moonshot-v1-8k"));
    assert!(GLMBackend::from_env().supports(Capability::StructuredOutput, "glm-4-plus"));
    assert!(OllamaBackend::from_env().supports(Capability::StructuredOutput, "llama3.1:8b"));
    assert!(OpenRouterBackend::from_env()
        .supports(Capability::StructuredOutput, "openai/gpt-4o-mini"));
    assert!(GeminiBackend::from_env().supports(Capability::StructuredOutput, "gemini-2.5-flash"));
    assert!(!AnthropicBackend::from_env()
        .supports(Capability::StructuredOutput, "claude-sonnet-4-5"));
}

#[test]
fn locked_params_dispatch_uniform_across_backends() {
    // Every backend with access to o1 / o3 / kimi-k2 should report
    // LockedParams = true; chat-only models should report false.
    // OpenRouter exercises the slug-form normalisation introduced in
    // 24.i (locked_model::normalise).
    let openai = OpenAIBackend::from_env();
    assert!(openai.supports(Capability::LockedParams, "o1-mini"));
    assert!(openai.supports(Capability::LockedParams, "o3"));
    assert!(!openai.supports(Capability::LockedParams, "gpt-4o-mini"));

    let kimi = KimiBackend::from_env();
    assert!(kimi.supports(Capability::LockedParams, "kimi-k2.6"));
    assert!(!kimi.supports(Capability::LockedParams, "moonshot-v1-8k"));

    let openrouter = OpenRouterBackend::from_env();
    assert!(openrouter.supports(Capability::LockedParams, "openai/o1-mini"));
    assert!(openrouter.supports(Capability::LockedParams, "moonshot/kimi-k2.6"));
    assert!(!openrouter.supports(Capability::LockedParams, "openai/gpt-4o-mini"));
    assert!(!openrouter.supports(Capability::LockedParams, "anthropic/claude-sonnet-4-5"));

    // Non-OpenAI-compat backends never report LockedParams (Anthropic /
    // Gemini have no documented locked-param families).
    assert!(!AnthropicBackend::from_env()
        .supports(Capability::LockedParams, "claude-sonnet-4-5"));
    assert!(!GeminiBackend::from_env().supports(Capability::LockedParams, "gemini-2.5-flash"));
}

// ────────────────────────────────────────────────────────────────────
//  count_tokens reachable through every backend
// ────────────────────────────────────────────────────────────────────

#[test]
fn count_tokens_returns_nonzero_for_all_backends() {
    // Sample text has 11 chars; estimate path returns ceil(11/4) = 3,
    // exact tokenizer paths return 1-5 tokens. Every backend should
    // produce > 0 + ≤ 10 for "hello world".
    let cases: Vec<(Box<dyn Backend>, &str)> = vec![
        (Box::new(AnthropicBackend::from_env()), "claude-sonnet-4-5"),
        (Box::new(OpenAIBackend::from_env()), "gpt-4o-mini"),
        (Box::new(GeminiBackend::from_env()), "gemini-2.5-flash"),
        (Box::new(KimiBackend::from_env()), "moonshot-v1-8k"),
        (Box::new(GLMBackend::from_env()), "glm-4-plus"),
        (Box::new(OllamaBackend::from_env()), "llama3.1:8b"),
        (Box::new(OpenRouterBackend::from_env()), "openai/gpt-4o-mini"),
    ];
    for (b, model) in cases {
        let n = b.count_tokens(model, "hello world");
        assert!(
            (1..=10).contains(&n),
            "{} count_tokens out of range: {n}",
            b.name(),
        );
    }
}

// ────────────────────────────────────────────────────────────────────
//  Registry dispatch
// ────────────────────────────────────────────────────────────────────

#[test]
fn registry_dispatch_returns_correct_backend_per_name() {
    let r = Registry::production();
    let pairs = [
        ("anthropic", "claude-3-5-haiku-latest"),
        ("openai", "gpt-4o-mini"),
        ("gemini", "gemini-2.5-flash"),
        ("kimi", "moonshot-v1-8k"),
        ("glm", "glm-4-plus"),
        ("ollama", "llama3.1:8b"),
        ("openrouter", "openai/gpt-4o-mini"),
    ];
    for (name, expected_default) in pairs {
        let backend = r.get(name).unwrap_or_else(|| panic!("{name} missing"));
        assert_eq!(backend.name(), name);
        assert_eq!(backend.default_model(), expected_default);
    }
}

#[test]
fn registry_unknown_provider_returns_none() {
    let r = Registry::production();
    assert!(r.get("not-a-real-provider").is_none());
    assert!(r.get("").is_none());
}

#[test]
fn registry_provider_names_sorted_alphabetically() {
    let r = Registry::production();
    let names = r.provider_names();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "provider_names() must be sorted");
}
