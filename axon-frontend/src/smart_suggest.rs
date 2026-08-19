//! v1.20.0 — Smart-suggest "Did you mean X?" for unknown tokens.
//!
//! Byte-identical mirror of `axon/compiler/_smart_suggest.py`.
//! Same Levenshtein algorithm shape, same threshold (≤ 2), same
//! `MAX_RESULTS = 3`, same hint formatter. The cross-stack drift
//! gate (28.i) compares suggestion lists input-for-input across
//! Python ↔ Rust and asserts byte-identical equality (D7).
//!
//! Design contract (D3 ratified 2026-05-10):
//!   - Maximum edit distance: ≤ 2
//!   - Maximum candidates returned: 3
//!   - Always on (D11) — every unknown-keyword error path can
//!     append `suggest_for(unknown, candidates)` to its message.
//!
//! Pure module: zero runtime deps, no allocations beyond the
//! result `Vec<String>` + the rolling DP row.

/// D3 — Levenshtein distance threshold.
pub const MAX_DISTANCE: usize = 2;

/// D3 — Maximum number of candidates returned by `suggest`.
pub const MAX_RESULTS: usize = 3;

/// Iterative Levenshtein edit distance.
///
/// Single-row rolling DP for `O(b.len())` auxiliary space. Treats
/// each Unicode codepoint as a unit (parity with Python's
/// per-character iteration over `str`).
///
/// Edge cases:
///   - `levenshtein("", "abc") == 3`
///   - `levenshtein("abc", "") == 3`
///   - `levenshtein("", "") == 0`
#[must_use]
pub fn levenshtein(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    if a_chars.is_empty() {
        return b_chars.len();
    }
    if b_chars.is_empty() {
        return a_chars.len();
    }
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr: Vec<usize> = vec![0; b_chars.len() + 1];
    for (i, ca) in a_chars.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b_chars.iter().enumerate() {
            let cost = usize::from(ca != cb);
            let deletion = prev[j + 1] + 1;
            let insertion = curr[j] + 1;
            let substitution = prev[j] + cost;
            curr[j + 1] = deletion.min(insertion).min(substitution);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_chars.len()]
}

/// Return up to `max_results` candidates close enough to `unknown`.
///
/// Candidates with `levenshtein(unknown, cand) ≤ max_distance` are
/// kept, sorted by `(distance, candidate)` ascending. Ties are
/// broken alphabetically — output is deterministic across runs and
/// across stacks (Python mirror sorts the same way).
///
/// `unknown == ""` → empty result.
/// Exact matches (distance 0) are dropped.
/// Duplicate candidates in the input iterator are de-duplicated.
#[must_use]
pub fn suggest(
    unknown: &str,
    candidates: &[&str],
    max_distance: usize,
    max_results: usize,
) -> Vec<String> {
    if unknown.is_empty() {
        return Vec::new();
    }
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut scored: Vec<(usize, &str)> = Vec::new();
    for &cand in candidates {
        if cand == unknown || !seen.insert(cand) {
            continue;
        }
        let d = levenshtein(unknown, cand);
        if d <= max_distance {
            scored.push((d, cand));
        }
    }
    scored.sort_by(|x, y| x.0.cmp(&y.0).then_with(|| x.1.cmp(y.1)));
    scored
        .into_iter()
        .take(max_results)
        .map(|(_, c)| c.to_string())
        .collect()
}

/// Format a suggestion list into a sentence suffix.
///
/// Empty list → empty string. Single match → `"Did you mean
/// `flow`?"`. Two+ matches → `"Did you mean `flow`, `flow_v2`, or
/// `flowery`?"`. Identical formatting to the Python mirror so
/// adopters get byte-identical hints regardless of which stack
/// emitted the diagnostic (D7).
#[must_use]
pub fn format_suggestion_hint(suggestions: &[String]) -> String {
    match suggestions.len() {
        0 => String::new(),
        1 => format!("Did you mean `{}`?", suggestions[0]),
        2 => format!(
            "Did you mean `{}` or `{}`?",
            suggestions[0], suggestions[1]
        ),
        _ => {
            let last = suggestions.last().unwrap();
            let head: Vec<String> = suggestions[..suggestions.len() - 1]
                .iter()
                .map(|s| format!("`{s}`"))
                .collect();
            format!("Did you mean {}, or `{}`?", head.join(", "), last)
        }
    }
}

/// Convenience: `suggest()` + `format_suggestion_hint()` with the
/// D3 defaults (`MAX_DISTANCE=2`, `MAX_RESULTS=3`). Returns the
/// formatted hint or empty string.
///
/// Use at the error-raising site to keep the call short:
///
/// ```ignore
/// let mut msg = "Unexpected token at top level".to_string();
/// let hint = suggest_for(tok.value.as_str(), &TOP_LEVEL_KEYWORD_NAMES);
/// if !hint.is_empty() {
///     msg = format!("{msg}. {hint}");
/// }
/// ```
#[must_use]
pub fn suggest_for(unknown: &str, candidates: &[&str]) -> String {
    let s = suggest(unknown, candidates, MAX_DISTANCE, MAX_RESULTS);
    format_suggestion_hint(&s)
}

// ── Adopter-facing keyword sets for parser integration ───────────
//
// Mirror of `_TOP_LEVEL_KEYWORD_NAMES` and `_FLOW_BODY_KEYWORD_NAMES`
// in `axon/compiler/parser.py`. Both lists are sorted alphabetically
// for cross-stack diff readability; ordering doesn't matter at runtime
// because `suggest()` re-sorts by (distance, name).

/// Top-level declaration keywords adopters might typo (e.g. `flwo`,
/// `intnt`).
pub const TOP_LEVEL_KEYWORD_NAMES: &[&str] = &[
    "agent", "anchor", "axonendpoint", "axonstore", "believe", "channel",
    "component", "compute", "context", "corpus", "daemon", "dataspace",
    "doubt", "effect", "ensemble", "fabric", "flow", "heal", "immune",
    "import", "ingest", "intent", "know", "lambda", "lease", "let",
    "mandate", "manifest", "memory", "mutate", "observe", "ots",
    "persist", "persona", "pix", "psyche", "purge", "reconcile", "reflex",
    "resource", "retrieve", "run", "session", "shield", "speculate",
    "tool", "topology", "transact", "type", "upstream", "view", "voice",
];

/// Flow-body step keywords adopters might typo (e.g. `stepp`,
/// `reasn`, `validte`).
pub const FLOW_BODY_KEYWORD_NAMES: &[&str] = &[
    "abort", "aggregate", "associate", "break", "continue", "corroborate",
    "daemon", "drill", "explore", "focus", "forward", "handle",
    "hibernate", "if", "ingest", "lambda", "let", "listen", "mandate",
    "mutate", "navigate", "ots", "par", "perform", "persist", "probe",
    "purge", "quant", "reason", "recall", "refine", "remember", "resume",
    "retrieve", "return", "shield", "step", "stream", "trail", "transact",
    "use", "validate", "weave", "yield",
];

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod smart_suggest_tests {
    use super::*;

    // ── Levenshtein ──────────────────────────────────────────────

    #[test]
    fn identical_strings_distance_zero() {
        assert_eq!(levenshtein("flow", "flow"), 0);
    }

    #[test]
    fn empty_left_returns_len_right() {
        assert_eq!(levenshtein("", "abc"), 3);
    }

    #[test]
    fn empty_right_returns_len_left() {
        assert_eq!(levenshtein("abc", ""), 3);
    }

    #[test]
    fn both_empty_distance_zero() {
        assert_eq!(levenshtein("", ""), 0);
    }

    #[test]
    fn flwo_to_flow_is_two() {
        assert_eq!(levenshtein("flwo", "flow"), 2);
    }

    #[test]
    fn deletion_one() {
        assert_eq!(levenshtein("flowx", "flow"), 1);
    }

    #[test]
    fn insertion_one() {
        assert_eq!(levenshtein("flo", "flow"), 1);
    }

    #[test]
    fn substitution_one() {
        assert_eq!(levenshtein("flop", "flow"), 1);
    }

    #[test]
    fn kitten_sitting_three() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
    }

    #[test]
    fn unicode_codepoints() {
        assert_eq!(levenshtein("héllo", "hello"), 1);
    }

    #[test]
    fn symmetry() {
        assert_eq!(
            levenshtein("flow", "fblo"),
            levenshtein("fblo", "flow")
        );
    }

    // ── suggest ──────────────────────────────────────────────────

    fn s(unknown: &str, cands: &[&str]) -> Vec<String> {
        suggest(unknown, cands, MAX_DISTANCE, MAX_RESULTS)
    }

    #[test]
    fn distance_one_returned() {
        assert_eq!(s("flo", &["flow", "type", "intent"]), vec!["flow"]);
    }

    #[test]
    fn distance_two_returned() {
        assert_eq!(s("flwo", &["flow", "type", "intent"]), vec!["flow"]);
    }

    #[test]
    fn distance_three_dropped() {
        assert_eq!(s("abcdef", &["flow", "type"]), Vec::<String>::new());
    }

    #[test]
    fn max_results_cap_at_three() {
        let cands = ["aaaa", "aaab", "aaac", "aaad", "aaae"];
        let r = s("aaa", &cands);
        assert_eq!(r.len(), MAX_RESULTS);
        assert_eq!(r, vec!["aaaa", "aaab", "aaac"]);
    }

    #[test]
    fn results_sorted_by_distance_then_alpha() {
        let r = s("flow", &["flow", "flop", "floor", "flux"]);
        assert_eq!(r, vec!["flop", "floor", "flux"]);
    }

    #[test]
    fn exact_match_dropped() {
        assert_eq!(s("flow", &["flow"]), Vec::<String>::new());
    }

    #[test]
    fn dedup_input_candidates() {
        assert_eq!(s("flo", &["flow", "flow", "flow"]), vec!["flow"]);
    }

    #[test]
    fn empty_unknown_returns_empty() {
        assert_eq!(s("", &["flow", "type"]), Vec::<String>::new());
    }

    #[test]
    fn empty_candidates_returns_empty() {
        assert_eq!(s("flow", &[]), Vec::<String>::new());
    }

    #[test]
    fn custom_max_distance() {
        let r = suggest("flowwer", &["flower", "flow"], 3, MAX_RESULTS);
        assert_eq!(r, vec!["flower", "flow"]);
    }

    #[test]
    fn custom_max_results() {
        let r = suggest("z", &["a", "b", "c", "d", "e"], MAX_DISTANCE, 2);
        assert_eq!(r, vec!["a", "b"]);
    }

    // ── format_suggestion_hint ───────────────────────────────────

    #[test]
    fn hint_empty_list_returns_empty_string() {
        assert_eq!(format_suggestion_hint(&[]), "");
    }

    #[test]
    fn hint_single_match() {
        assert_eq!(
            format_suggestion_hint(&["flow".to_string()]),
            "Did you mean `flow`?"
        );
    }

    #[test]
    fn hint_two_matches() {
        assert_eq!(
            format_suggestion_hint(&["flow".to_string(), "flop".to_string()]),
            "Did you mean `flow` or `flop`?"
        );
    }

    #[test]
    fn hint_three_matches_oxford_or() {
        assert_eq!(
            format_suggestion_hint(&[
                "flow".to_string(),
                "flop".to_string(),
                "flux".to_string()
            ]),
            "Did you mean `flow`, `flop`, or `flux`?"
        );
    }

    // ── suggest_for ──────────────────────────────────────────────

    #[test]
    fn suggest_for_end_to_end_single_match() {
        assert_eq!(
            suggest_for("flo", &["flow", "type", "intent"]),
            "Did you mean `flow`?"
        );
    }

    #[test]
    fn suggest_for_end_to_end_no_match() {
        assert_eq!(suggest_for("xyz", &["flow", "type"]), "");
    }

    #[test]
    fn suggest_for_end_to_end_three_matches() {
        assert_eq!(
            suggest_for("aaa", &["aaab", "aaac", "aaad", "aaae", "aaaf"]),
            "Did you mean `aaab`, `aaac`, or `aaad`?"
        );
    }

    // ── Cross-stack golden parity (mirrors Python TestRustParityShape) ──

    #[test]
    fn golden_levenshtein_pairs() {
        let pairs: &[(&str, &str, usize)] = &[
            ("flow", "flow", 0),
            ("flo", "flow", 1),
            ("flwo", "flow", 2),
            ("flox", "flow", 1),
            ("kitten", "sitting", 3),
            ("", "abc", 3),
            ("abc", "", 3),
            ("", "", 0),
        ];
        for (a, b, expected) in pairs {
            assert_eq!(
                levenshtein(a, b),
                *expected,
                "levenshtein({a:?}, {b:?}) != {expected}"
            );
        }
    }

    #[test]
    fn golden_suggest_top_level() {
        let cands = ["flow", "intent", "type", "tool", "persona"];
        assert_eq!(s("flwo", &cands), vec!["flow"]);
        assert_eq!(s("toll", &cands), vec!["tool"]);
        assert_eq!(s("intt", &cands), vec!["intent"]);
    }

    #[test]
    fn golden_hint_three_match() {
        assert_eq!(
            format_suggestion_hint(&[
                "a".to_string(),
                "b".to_string(),
                "c".to_string()
            ]),
            "Did you mean `a`, `b`, or `c`?"
        );
    }

    #[test]
    fn constants_documented() {
        assert_eq!(MAX_DISTANCE, 2);
        assert_eq!(MAX_RESULTS, 3);
    }
}
