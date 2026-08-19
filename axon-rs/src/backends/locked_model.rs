//! Locked-parameter dispatch for reasoning models — v1.18.0.
//!
//! Direct port of the v1.16.2 Python registry
//! (`axon.server.model_clients::_LOCKED_PARAMETER_MODELS`). Several
//! reasoning model families reject sampling parameters that the
//! provider has hard-coded server-side; sending them yields an
//! HTTP 400 response that breaks production workloads.
//!
//! The pattern → locked-set mapping below MUST stay in lockstep with
//! the Python registry — drift is detected by
//! `tests/test_fase24_locked_model_parity.py` (v1.18.0).
//!
//! # Use
//!
//! ```ignore
//! use serde_json::json;
//! use axon::backends::locked_model::apply_sampling_params;
//!
//! let mut body = json!({
//!     "model": "kimi-k2.6",
//!     "messages": [...],
//!     "temperature": 0.5,        // ← will be removed (locked)
//!     "top_p": 0.9,              // ← will be removed (locked)
//!     "max_tokens": 2048,        // ← kept (not locked)
//! });
//! apply_sampling_params(&mut body, "kimi-k2.6");
//! ```

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

/// One entry in the locked-model registry — a regex pattern paired with
/// the set of body parameter names the matching models reject.
struct LockedEntry {
    pattern: Regex,
    locked: &'static [&'static str],
}

/// Static registry. Lazily compiled on first use; thereafter regex
/// evaluation is amortised on the hot request path.
fn registry() -> &'static [LockedEntry] {
    static REG: OnceLock<Vec<LockedEntry>> = OnceLock::new();
    REG.get_or_init(|| {
        vec![
            // Moonshot Kimi K2.x reasoning family.
            // Provider docs: https://platform.moonshot.ai/docs/api/chat
            // The K2.6 generation locks temperature to 1.0 (default mode)
            // or 0.6 (max mode); supplying any sampling parameter yields
            // HTTP 400. Verified against Moonshot 2025-04 docs (Kivi
            // K2.6 incident, v1.16.2).
            LockedEntry {
                pattern: Regex::new(r"^kimi-k2\.").expect("static regex"),
                locked: &[
                    "temperature",
                    "top_p",
                    "top_k",
                    "n",
                    "presence_penalty",
                    "frequency_penalty",
                ],
            },
            // OpenAI o1 family (o1, o1-mini, o1-preview).
            // Provider docs: https://platform.openai.com/docs/guides/reasoning
            // o1* models hard-code temperature=1, top_p=1, and reject
            // logprobs / logit_bias entirely.
            LockedEntry {
                pattern: Regex::new(r"^o1").expect("static regex"),
                locked: &[
                    "temperature",
                    "top_p",
                    "presence_penalty",
                    "frequency_penalty",
                    "logprobs",
                    "logit_bias",
                ],
            },
            // OpenAI o3 family (o3, o3-mini, o3-pro).
            // Provider docs: https://platform.openai.com/docs/guides/reasoning
            // Same locked set as o1; documented separately because the
            // pattern + family is distinct.
            LockedEntry {
                pattern: Regex::new(r"^o3").expect("static regex"),
                locked: &[
                    "temperature",
                    "top_p",
                    "presence_penalty",
                    "frequency_penalty",
                    "logprobs",
                    "logit_bias",
                ],
            },
        ]
    })
}

/// Normalise a model identifier for locked-pattern matching. Strips
/// any leading `provider/` prefix so OpenRouter slug forms
/// (e.g. `openai/o1-mini`, `moonshot/kimi-k2.6`) match the same
/// patterns as direct calls (`o1-mini`, `kimi-k2.6`).
///
/// This is a backward-compatible widening — model names that lack a
/// `/` are returned unchanged, so direct calls to OpenAI / Kimi / etc.
/// behave identically. The OpenRouter path (24.i) was the trigger for
/// this normalisation; without it, `openai/o1-mini` would silently
/// keep the locked sampling params + return HTTP 400 from OpenRouter.
fn normalise(model_name: &str) -> &str {
    // `rsplit('/').next()` returns the segment after the last `/`,
    // or the full string when there's no `/`. Equivalent to
    // `model_name.rsplit('/').next().unwrap_or(model_name)` but the
    // unwrap is unreachable because rsplit always yields ≥1 element.
    model_name.rsplit('/').next().unwrap_or(model_name)
}

/// Return the set of body-parameter names the model rejects.
///
/// Empty set for models without known restrictions — callers can include
/// any sampling parameter freely. The function is cheap (regex search +
/// set union); production-safe in the hot path.
///
/// Slug-form names (`provider/model`) are normalised by stripping the
/// `provider/` prefix before matching, so OpenRouter calls to
/// `openai/o1-mini` / `moonshot/kimi-k2.6` are correctly recognised.
pub fn locked_params_for_model(model_name: &str) -> HashSet<&'static str> {
    let mut locked: HashSet<&'static str> = HashSet::new();
    if model_name.is_empty() {
        return locked;
    }
    let normalised = normalise(model_name);
    for entry in registry() {
        if entry.pattern.is_match(normalised) {
            for param in entry.locked {
                locked.insert(*param);
            }
        }
    }
    locked
}

/// Whether a single named parameter is locked by the supplied model.
///
/// See [`locked_params_for_model`] for slug-form normalisation behaviour.
pub fn is_locked(model_name: &str, parameter: &str) -> bool {
    if model_name.is_empty() {
        return false;
    }
    let normalised = normalise(model_name);
    for entry in registry() {
        if entry.pattern.is_match(normalised)
            && entry.locked.iter().any(|p| *p == parameter)
        {
            return true;
        }
    }
    false
}

/// Strip every locked sampling parameter from the request body in-place.
///
/// Mirrors `_apply_sampling_params` from the Python side. Only mutates
/// when the body is a JSON object; other shapes are left untouched.
/// Returns the list of (name, value) pairs that were removed so the
/// caller can log a per-process dedup'd warning when an adopter
/// supplied a value the provider would have rejected.
pub fn apply_sampling_params(body: &mut Value, model_name: &str) -> Vec<(String, Value)> {
    let locked = locked_params_for_model(model_name);
    if locked.is_empty() {
        return Vec::new();
    }
    let Some(obj) = body.as_object_mut() else {
        return Vec::new();
    };
    let mut removed: Vec<(String, Value)> = Vec::new();
    for name in locked {
        if let Some(value) = obj.remove(name) {
            removed.push((name.to_string(), value));
        }
    }
    removed
}

/// Iterate the (pattern, locked-set) pairs for parity testing — used by
/// the v1.18.0 drift gate against the Python registry.
pub fn registered_patterns() -> Vec<(String, Vec<&'static str>)> {
    registry()
        .iter()
        .map(|e| (e.pattern.as_str().to_string(), e.locked.to_vec()))
        .collect()
}

// ────────────────────────────────────────────────────────────────────
//  Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_model_has_no_locked_params() {
        assert!(locked_params_for_model("").is_empty());
    }

    #[test]
    fn unrelated_model_has_no_locked_params() {
        assert!(locked_params_for_model("gpt-4o").is_empty());
        assert!(locked_params_for_model("claude-sonnet-4-5").is_empty());
        assert!(locked_params_for_model("gemini-2.5-pro").is_empty());
    }

    #[test]
    fn kimi_k2_6_locks_six_params() {
        let locked = locked_params_for_model("kimi-k2.6");
        assert!(locked.contains("temperature"));
        assert!(locked.contains("top_p"));
        assert!(locked.contains("top_k"));
        assert!(locked.contains("n"));
        assert!(locked.contains("presence_penalty"));
        assert!(locked.contains("frequency_penalty"));
        assert_eq!(locked.len(), 6);
    }

    #[test]
    fn kimi_k2_8_also_locked() {
        // Pattern is `^kimi-k2\.` so any K2.* slug matches.
        assert!(is_locked("kimi-k2.8", "temperature"));
    }

    #[test]
    fn kimi_k1_not_locked() {
        // Pre-K2 generations don't lock params.
        assert!(!is_locked("kimi-k1.5", "temperature"));
        assert!(locked_params_for_model("kimi-k1.5").is_empty());
    }

    #[test]
    fn o1_family_locks_six_params() {
        let locked = locked_params_for_model("o1");
        assert!(locked.contains("temperature"));
        assert!(locked.contains("logprobs"));
        assert!(locked.contains("logit_bias"));
        assert_eq!(locked.len(), 6);
    }

    #[test]
    fn o1_mini_and_o1_preview_match_pattern() {
        assert!(is_locked("o1-mini", "temperature"));
        assert!(is_locked("o1-preview", "logit_bias"));
    }

    #[test]
    fn o3_family_locks_same_set_as_o1() {
        let o1 = locked_params_for_model("o1");
        let o3 = locked_params_for_model("o3-mini");
        assert_eq!(o1, o3);
    }

    #[test]
    fn apply_strips_locked_params_from_object() {
        let mut body = json!({
            "model": "kimi-k2.6",
            "messages": [{"role": "user", "content": "hi"}],
            "temperature": 0.5,
            "top_p": 0.9,
            "max_tokens": 2048,
        });
        let removed = apply_sampling_params(&mut body, "kimi-k2.6");
        let removed_names: HashSet<String> = removed.iter().map(|(n, _)| n.clone()).collect();
        assert!(removed_names.contains("temperature"));
        assert!(removed_names.contains("top_p"));
        // Non-locked field preserved.
        assert!(body.get("max_tokens").is_some());
        // Locked fields gone.
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
    }

    #[test]
    fn apply_no_op_for_unlocked_model() {
        let mut body = json!({
            "model": "gpt-4o",
            "temperature": 0.5,
        });
        let removed = apply_sampling_params(&mut body, "gpt-4o");
        assert!(removed.is_empty());
        assert_eq!(body["temperature"], 0.5);
    }

    #[test]
    fn apply_handles_non_object_body_gracefully() {
        let mut body = json!("not an object");
        let removed = apply_sampling_params(&mut body, "kimi-k2.6");
        assert!(removed.is_empty());
    }

    #[test]
    fn registered_patterns_returns_all_three_families() {
        let patterns = registered_patterns();
        assert_eq!(patterns.len(), 3);
        let pattern_strs: Vec<&str> = patterns.iter().map(|(p, _)| p.as_str()).collect();
        assert!(pattern_strs.contains(&r"^kimi-k2\."));
        assert!(pattern_strs.contains(&r"^o1"));
        assert!(pattern_strs.contains(&r"^o3"));
    }

    #[test]
    fn slug_form_strips_provider_prefix_for_openrouter() {
        // OpenRouter sends model identifiers as `provider/model` slugs.
        // The locked-model dispatch must treat them equivalently to
        // direct calls so reasoning models accessed via OpenRouter
        // don't return HTTP 400 from the gateway.
        assert!(is_locked("openai/o1-mini", "temperature"));
        assert!(is_locked("openai/o3", "logprobs"));
        assert!(is_locked("moonshot/kimi-k2.6", "top_p"));
    }

    #[test]
    fn slug_form_does_not_widen_match_for_unrelated_models() {
        // Stripping the `provider/` prefix must NOT cause spurious
        // matches for model families outside the locked registry.
        assert!(!is_locked("openai/gpt-4o-mini", "temperature"));
        assert!(!is_locked("anthropic/claude-sonnet-4-5", "temperature"));
        assert!(!is_locked("google/gemini-2.5-pro", "temperature"));
    }

    #[test]
    fn slug_normalisation_idempotent_for_direct_model_names() {
        // Direct call form (no slash) must behave identically.
        let direct = locked_params_for_model("o1-mini");
        let slug = locked_params_for_model("openai/o1-mini");
        assert_eq!(direct, slug);
    }

    #[test]
    fn apply_returns_removed_values_for_warning() {
        // The caller logs a per-process dedup'd warning per (model, param)
        // combo. Verify the mutation API gives back enough detail.
        let mut body = json!({
            "model": "o1-mini",
            "temperature": 0.7,
            "logprobs": true,
        });
        let removed = apply_sampling_params(&mut body, "o1-mini");
        let map: std::collections::HashMap<String, Value> = removed.into_iter().collect();
        assert_eq!(map.get("temperature"), Some(&json!(0.7)));
        assert_eq!(map.get("logprobs"), Some(&json!(true)));
    }
}
