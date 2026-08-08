//! Unified token-counting surface for native Rust LLM backends — Fase 24.b
//! (§Fase 25.g delegated re-export).
//!
//! Provides a single `count_tokens(model, text) -> usize` entry point that
//! dispatches by model-name prefix to the right per-provider tokenizer:
//!
//!   * `gpt-*`, `o1-*`, `o3-*`, `chatgpt-*`              → axon-csys cl100k_base / o200k_base
//!   * `kimi-*`, `moonshot-*`                            → axon-csys cl100k_base
//!   * `glm-*`                                            → axon-csys cl100k_base
//!   * `claude-*`                                        → 4-chars-per-token offline estimate
//!   * `gemini-*`                                        → 4-chars-per-token offline estimate
//!   * `llama-*`, `mistral-*`, `qwen-*`, `phi-*`         → 4-chars-per-token offline estimate
//!   * `openrouter:<provider>/<model>`                   → strip prefix + recurse
//!   * unknown / unmapped                                → 4-chars-per-token offline estimate
//!
//! As of Fase 25.g (2026-05-08) the BPE merge engine is the C23 kernel
//! in `axon-csys/c-src/tokens/bpe.c`; the merges tables for `cl100k_base`
//! and `o200k_base` are baked at compile time via `#embed` (when the
//! toolchain supports it) or `include_bytes!` (universal fallback). The
//! pretokeniser stays in Rust via `fancy-regex` (PCRE-compat). This
//! module is now a thin re-export — preserved unchanged in surface so
//! existing axon-rs call sites don't move.
//!
//! # Design intent (D10, carried over from 24.b)
//!
//! `tokens.rs` is designed to be extractable as a standalone crate
//! (`axon-tokens` 0.x.0) in the future — the public surface
//! (`count_tokens`, `CountKind`, the model-prefix routing) does not
//! depend on any axon-internal types. With Fase 25.g the BPE
//! implementation already lives in its own crate (`axon-csys`); the
//! extraction conversation is now mostly cosmetic.

pub use axon_csys::tokens::{count_tokens, estimate, CountKind, TokenCount};

// ────────────────────────────────────────────────────────────────────
//  Tests — port of the pre-25.g cases against the new backend.
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// §Fase 119.h — the toolchain-free half of the contract.
    ///
    /// The tests above are gated on `csys-native` because they assert
    /// EXACT counts, which only the C23 BPE kernel can produce. Gating
    /// them and stopping there would leave the default profile — the one
    /// `cargo install axon-lang` actually produces — asserting nothing at
    /// all about tokenisation, which is the failure mode this project has
    /// hit repeatedly: a suite that shrinks silently reads as a suite that
    /// passes.
    ///
    /// So the degradation gets its own gate. Without the kernel every
    /// family falls to the 4-chars-per-token estimate, and the point is
    /// that the number arrives LABELLED: `CountKind::Estimate` travels
    /// with it, so no caller can mistake one for the other.
    #[cfg(not(feature = "csys-native"))]
    #[test]
    fn without_the_c_kernel_every_family_degrades_to_a_labelled_estimate() {
        for model in [
            "gpt-4o-mini",
            "gpt-3.5-turbo",
            "o1-preview",
            "o3-mini",
            "chatgpt-4o-latest",
            "kimi-k2.6",
            "moonshot-v1-8k",
            "glm-4",
            "openrouter:openai/gpt-4o-mini",
        ] {
            let got = count_tokens(model, "axon for axon — four-pillar streaming language.");
            assert_eq!(
                got.kind,
                CountKind::Estimate,
                "{model}: without `csys-native` there is no BPE kernel, so the count MUST be \
                 reported as an estimate. An `Exact` here would mean the count is unlabelled \
                 guesswork — the one outcome worse than a guess."
            );
            assert!(
                got.count > 0,
                "{model}: the estimate must still be a usable number — degrading to zero would \
                 silently zero out every budget and quota computed from it"
            );
        }
    }

    #[test]
    fn empty_text_is_zero_tokens() {
        let r = count_tokens("gpt-4o-mini", "");
        assert_eq!(r.count, 0);
    }

    #[test]
    fn empty_text_estimate_is_zero() {
        let r = estimate("");
        assert_eq!(r.count, 0);
        assert_eq!(r.kind, CountKind::Estimate);
    }

    #[test]
    fn estimate_rounds_up() {
        // 5 chars → ceil(5/4) = 2 tokens.
        assert_eq!(estimate("hello").count, 2);
        // 4 chars → exactly 1 token.
        assert_eq!(estimate("ABCD").count, 1);
        // 8 chars → 2 tokens.
        assert_eq!(estimate("ABCDEFGH").count, 2);
        // 9 chars → ceil(9/4) = 3 tokens.
        assert_eq!(estimate("ABCDEFGHI").count, 3);
    }

    #[test]
    fn anthropic_uses_estimate() {
        let r = count_tokens("claude-sonnet-4-5", "hello world this is a test");
        assert_eq!(r.kind, CountKind::Estimate);
    }

    #[test]
    fn gemini_uses_estimate() {
        let r = count_tokens("gemini-2.5-pro", "hello world this is a test");
        assert_eq!(r.kind, CountKind::Estimate);
    }

    #[test]
    fn unknown_model_uses_estimate() {
        let r = count_tokens("totally-fake-model-7b", "hello world");
        assert_eq!(r.kind, CountKind::Estimate);
    }

    #[test]
    fn ollama_local_models_use_estimate() {
        for model in &["llama-3.1-70b", "mistral-7b", "qwen-2.5-7b", "phi-4"] {
            let r = count_tokens(model, "hello world");
            assert_eq!(
                r.kind,
                CountKind::Estimate,
                "model {model} unexpected kind {r:?}"
            );
        }
    }

    #[cfg(feature = "csys-native")]
    #[test]
    fn gpt_4o_uses_o200k_exact() {
        let r = count_tokens("gpt-4o-mini", "hello world");
        assert_eq!(r.kind, CountKind::Exact);
        // Exact count for "hello world" via o200k is 2 tokens; sanity-
        // check it's small + nonzero.
        assert!(r.count >= 1);
        assert!(r.count <= 5);
    }

    #[cfg(feature = "csys-native")]
    #[test]
    fn gpt_3_5_turbo_uses_cl100k_exact() {
        let r = count_tokens("gpt-3.5-turbo", "hello world");
        assert_eq!(r.kind, CountKind::Exact);
    }

    #[cfg(feature = "csys-native")]
    #[test]
    fn o1_and_o3_use_o200k_exact() {
        let a = count_tokens("o1-mini", "hello world");
        let b = count_tokens("o3-mini", "hello world");
        assert_eq!(a.kind, CountKind::Exact);
        assert_eq!(b.kind, CountKind::Exact);
        // Same text, same tokenizer → same count.
        assert_eq!(a.count, b.count);
    }

    #[cfg(feature = "csys-native")]
    #[test]
    fn kimi_and_glm_use_cl100k_exact() {
        let a = count_tokens("kimi-k2.6", "hello world");
        let b = count_tokens("glm-4-plus", "hello world");
        assert_eq!(a.kind, CountKind::Exact);
        assert_eq!(b.kind, CountKind::Exact);
    }

    #[cfg(feature = "csys-native")]
    #[test]
    fn moonshot_alias_uses_cl100k_exact() {
        let r = count_tokens("moonshot-v1-8k", "hello world");
        assert_eq!(r.kind, CountKind::Exact);
    }

    #[cfg(feature = "csys-native")]
    #[test]
    fn openrouter_strips_prefix_and_recurses() {
        // openrouter:openai/gpt-4o-mini → gpt-4o-mini → o200k_base
        let r = count_tokens("openrouter:openai/gpt-4o-mini", "hello world");
        assert_eq!(r.kind, CountKind::Exact);
    }

    #[test]
    fn openrouter_to_anthropic_falls_back_to_estimate() {
        let r = count_tokens("openrouter:anthropic/claude-sonnet-4-5", "hello world");
        assert_eq!(r.kind, CountKind::Estimate);
    }

    #[cfg(feature = "csys-native")]
    #[test]
    fn case_insensitive_model_matching() {
        // Adopters sometimes specify mixed case; the dispatch must work.
        let r = count_tokens("GPT-4o-mini", "hello");
        assert_eq!(r.kind, CountKind::Exact);
    }

    #[test]
    fn unicode_text_counts_chars_not_bytes() {
        // 5 multi-byte chars → ceil(5/4) = 2 tokens (estimate path).
        let r = estimate("héllo");
        assert_eq!(r.count, 2);
    }
}
