//! v1.19.0 — BPE tokeniser test suite.
//!
//! Coverage:
//!   1. Vocabulary metadata (size, regex pattern parity).
//!   2. Single-byte / single-token round-trips.
//!   3. ASCII reference vectors against tiktoken-rs.
//!   4. Multi-byte UTF-8 reference vectors against tiktoken-rs.
//!   5. CJK reference vectors against tiktoken-rs.
//!   6. Long-input drift gate (paragraph + document scale).
//!   7. Special-token recognition.
//!   8. `count_tokens` routing parity vs the legacy implementation.
//!   9. UTF-8 boundary helper edge cases.
//!  10. SIMD UTF-8 codepoint counter parity vs `chars().count()`.
//!  11. Thread-safety smoke (concurrent encode).
//!  12. Decode round-trip.
//!
//! The drift gate (`encode == tiktoken_rs.encode`) is the
//! load-bearing test — every other test could pass while the
//! BPE merge order silently diverged. The drift gate catches
//! that immediately.

use std::sync::Arc;
use std::thread;

use axon_csys::tokens::{self, BpeError, CountKind, Tokenizer};

// ──────────────────────────────────────────────────────────────────────
// 1. Vocabulary metadata
// ──────────────────────────────────────────────────────────────────────

#[test]
fn cl100k_vocab_size_matches_tiktoken() {
    let tok = tokens::cl100k_base().expect("cl100k_base must load");
    assert_eq!(tok.vocab_size(), 100_256);
}

#[test]
fn o200k_vocab_size_matches_tiktoken() {
    let tok = tokens::o200k_base().expect("o200k_base must load");
    assert_eq!(tok.vocab_size(), 199_998);
}

#[test]
fn cl100k_regex_pat_round_trips_through_blob() {
    let tok = tokens::cl100k_base().expect("cl100k_base must load");
    let pat = tok.embedded_pat();
    // The pat literal in tokens.rs must be byte-identical to the one
    // serialised into the blob by tools/gen_merges.py.
    assert!(
        pat.starts_with("'(?i:[sdmt]"),
        "unexpected cl100k pat: {pat:?}"
    );
}

#[test]
fn o200k_regex_pat_round_trips_through_blob() {
    let tok = tokens::o200k_base().expect("o200k_base must load");
    let pat = tok.embedded_pat();
    assert!(
        pat.starts_with("[^\\r\\n\\p{L}\\p{N}]?"),
        "unexpected o200k pat: {pat:?}"
    );
}

// ──────────────────────────────────────────────────────────────────────
// 2. Single-byte / single-token
// ──────────────────────────────────────────────────────────────────────

#[test]
fn single_byte_lookup_for_every_byte() {
    // Per tiktoken construction: every byte 0..255 is its own token in
    // the BPE vocabulary. Verify lookup_rank returns Some for all.
    let tok = tokens::cl100k_base().expect("cl100k_base must load");
    for b in 0u8..=255u8 {
        let bytes = [b];
        assert!(
            tok.lookup_rank(&bytes).is_some(),
            "cl100k missing single-byte rank for 0x{b:02X}"
        );
    }
}

#[test]
fn empty_string_encodes_to_empty() {
    let tok = tokens::cl100k_base().unwrap();
    assert!(tok.encode_ordinary("").unwrap().is_empty());
    assert!(tok.encode_with_special_tokens("").unwrap().is_empty());
}

// ──────────────────────────────────────────────────────────────────────
// 3. ASCII drift gate vs tiktoken-rs
// ──────────────────────────────────────────────────────────────────────

fn assert_drift_cl100k(text: &str) {
    let csys = tokens::cl100k_base().unwrap();
    let reference = tiktoken_rs::cl100k_base().expect("tiktoken-rs cl100k must load");
    let actual = csys.encode_ordinary(text).expect("axon-csys encode failed");
    let expected = reference.encode_ordinary(text);
    assert_eq!(
        actual, expected,
        "cl100k drift on {text:?}\n  actual:   {actual:?}\n  expected: {expected:?}"
    );
}

fn assert_drift_o200k(text: &str) {
    let csys = tokens::o200k_base().unwrap();
    let reference = tiktoken_rs::o200k_base().expect("tiktoken-rs o200k must load");
    let actual = csys.encode_ordinary(text).expect("axon-csys encode failed");
    let expected = reference.encode_ordinary(text);
    assert_eq!(
        actual, expected,
        "o200k drift on {text:?}\n  actual:   {actual:?}\n  expected: {expected:?}"
    );
}

#[test]
fn drift_cl100k_hello_world() {
    assert_drift_cl100k("hello world");
}

#[test]
fn drift_cl100k_short_phrases() {
    for phrase in [
        "The quick brown fox jumps over the lazy dog.",
        "abc",
        "    leading spaces",
        "trailing spaces    ",
        "\n\nempty lines\n\n",
        "tabs\there",
    ] {
        assert_drift_cl100k(phrase);
    }
}

#[test]
fn drift_o200k_hello_world() {
    assert_drift_o200k("hello world");
}

#[test]
fn drift_o200k_short_phrases() {
    for phrase in [
        "The quick brown fox jumps over the lazy dog.",
        "ChatGPT-4o is multimodal.",
        "1234567890",
        "Mixed CASE TEXT here.",
    ] {
        assert_drift_o200k(phrase);
    }
}

// ──────────────────────────────────────────────────────────────────────
// 4. Multi-byte UTF-8 drift gate
// ──────────────────────────────────────────────────────────────────────

#[test]
fn drift_cl100k_latin_extended() {
    for s in ["héllo", "café au lait", "naïve", "résumé"] {
        assert_drift_cl100k(s);
    }
}

#[test]
fn drift_cl100k_emoji() {
    for s in ["🦀 Rust", "Hello 🌍 world", "😀😃😄😁"] {
        assert_drift_cl100k(s);
    }
}

#[test]
fn drift_o200k_emoji() {
    for s in ["🦀 Rust", "Hello 🌍 world", "😀😃😄😁"] {
        assert_drift_o200k(s);
    }
}

// ──────────────────────────────────────────────────────────────────────
// 5. CJK drift gate
// ──────────────────────────────────────────────────────────────────────

#[test]
fn drift_cl100k_chinese() {
    for s in ["你好世界", "中文测试", "我喜欢编程"] {
        assert_drift_cl100k(s);
    }
}

#[test]
fn drift_cl100k_japanese() {
    for s in ["こんにちは", "日本語テスト", "ありがとう"] {
        assert_drift_cl100k(s);
    }
}

#[test]
fn drift_cl100k_korean() {
    for s in ["안녕하세요", "한국어 테스트"] {
        assert_drift_cl100k(s);
    }
}

#[test]
fn drift_o200k_cjk_mix() {
    assert_drift_o200k("你好世界 + こんにちは + 안녕하세요");
}

// ──────────────────────────────────────────────────────────────────────
// 6. Long-input drift
// ──────────────────────────────────────────────────────────────────────

#[test]
fn drift_cl100k_paragraph() {
    let text = "The Lorem ipsum text is a placeholder used by designers, \
        printers, and typesetters since the 1500s. Its origin lies in a \
        scrambled passage from Cicero's De finibus bonorum et malorum, \
        a treatise on the theory of ethics widely studied during the \
        Renaissance. The garbled Latin reads correctly on first glance \
        but conveys no meaning; this is precisely why it was selected \
        as filler — readers focus on layout rather than content.";
    assert_drift_cl100k(text);
}

#[test]
fn drift_o200k_paragraph() {
    let text = "Recursive structures unfold themselves into the world \
        in patterns that mirror their generative grammar. A tree is the \
        simplest case: each branch a self-similar copy of the whole. \
        More elaborate cases arise when the recursion carries state — \
        when each invocation observes and is observed.";
    assert_drift_o200k(text);
}

// ──────────────────────────────────────────────────────────────────────
// 7. Special-token recognition
// ──────────────────────────────────────────────────────────────────────

#[test]
fn cl100k_endoftext_emits_canonical_rank() {
    let tok = tokens::cl100k_base().unwrap();
    let ranks = tok.encode_with_special_tokens("<|endoftext|>").unwrap();
    assert_eq!(ranks, vec![100_257]);
}

#[test]
fn cl100k_special_in_middle_of_text() {
    let tok = tokens::cl100k_base().unwrap();
    let ranks = tok
        .encode_with_special_tokens("hello <|endoftext|> world")
        .unwrap();
    // Must contain the endoftext rank somewhere in the middle.
    assert!(
        ranks.contains(&100_257),
        "expected endoftext rank present: {ranks:?}"
    );
}

#[test]
fn cl100k_encode_ordinary_does_not_recognise_special() {
    let tok = tokens::cl100k_base().unwrap();
    let ranks = tok.encode_ordinary("<|endoftext|>").unwrap();
    // encode_ordinary treats specials as plain text → multiple BPE
    // ranks (NOT a single 100257).
    assert!(ranks.len() > 1, "expected multi-token, got {ranks:?}");
    assert!(!ranks.contains(&100_257));
}

// ──────────────────────────────────────────────────────────────────────
// 8. count_tokens routing parity
// ──────────────────────────────────────────────────────────────────────

#[test]
fn count_tokens_anthropic_uses_estimate() {
    let r = tokens::count_tokens("claude-sonnet-4-5", "hello world");
    assert_eq!(r.kind, CountKind::Estimate);
}

#[test]
fn count_tokens_gpt_4o_uses_o200k_exact() {
    let r = tokens::count_tokens("gpt-4o-mini", "hello world");
    assert_eq!(r.kind, CountKind::Exact);
    assert!((1..=5).contains(&r.count));
}

#[test]
fn count_tokens_o1_o3_use_o200k_exact() {
    let a = tokens::count_tokens("o1-mini", "hello world");
    let b = tokens::count_tokens("o3-mini", "hello world");
    assert_eq!(a.kind, CountKind::Exact);
    assert_eq!(b.kind, CountKind::Exact);
    assert_eq!(a.count, b.count);
}

#[test]
fn count_tokens_kimi_glm_use_cl100k_exact() {
    let a = tokens::count_tokens("kimi-k2.6", "hello world");
    let b = tokens::count_tokens("glm-4-plus", "hello world");
    assert_eq!(a.kind, CountKind::Exact);
    assert_eq!(b.kind, CountKind::Exact);
}

#[test]
fn count_tokens_openrouter_strips_prefix_and_recurses() {
    let r = tokens::count_tokens("openrouter:openai/gpt-4o-mini", "hello world");
    assert_eq!(r.kind, CountKind::Exact);
}

#[test]
fn count_tokens_case_insensitive_model_matching() {
    let r = tokens::count_tokens("GPT-4o-mini", "hello");
    assert_eq!(r.kind, CountKind::Exact);
}

#[test]
fn count_tokens_empty_text_is_zero() {
    let r = tokens::count_tokens("gpt-4o-mini", "");
    assert_eq!(r.count, 0);
}

#[test]
fn estimate_rounds_up() {
    assert_eq!(tokens::estimate("").count, 0);
    assert_eq!(tokens::estimate("ABCD").count, 1);
    assert_eq!(tokens::estimate("hello").count, 2);
    assert_eq!(tokens::estimate("ABCDEFGH").count, 2);
    assert_eq!(tokens::estimate("ABCDEFGHI").count, 3);
}

// ──────────────────────────────────────────────────────────────────────
// 9. UTF-8 boundary helper
// ──────────────────────────────────────────────────────────────────────

#[test]
fn utf8_boundary_floor_empty() {
    assert_eq!(tokens::utf8_boundary_floor(b"", 5), 0);
    assert_eq!(tokens::utf8_boundary_floor(b"abc", 0), 0);
}

#[test]
fn utf8_boundary_floor_ascii_is_identity() {
    let bytes = b"hello world";
    for i in 0..=bytes.len() {
        assert_eq!(tokens::utf8_boundary_floor(bytes, i), i);
    }
}

#[test]
fn utf8_boundary_floor_walks_back_from_continuation() {
    // "Aé" = [0x41, 0xC3, 0xA9]. Boundaries are at 0, 1, 3.
    let bytes = "Aé".as_bytes();
    assert_eq!(tokens::utf8_boundary_floor(bytes, 0), 0);
    assert_eq!(tokens::utf8_boundary_floor(bytes, 1), 1);
    // max_offset = 2 lands inside é → walk back to 1.
    assert_eq!(tokens::utf8_boundary_floor(bytes, 2), 1);
    assert_eq!(tokens::utf8_boundary_floor(bytes, 3), 3);
}

#[test]
fn utf8_boundary_floor_max_offset_past_end() {
    let bytes = b"abc";
    assert_eq!(tokens::utf8_boundary_floor(bytes, 100), bytes.len());
}

#[test]
fn utf8_boundary_floor_4byte_codepoint() {
    // "🦀" = [0xF0, 0x9F, 0xA6, 0x80]. The only valid boundaries
    // are 0 (start) and 4 (end). Any max in 1..=3 lands inside the
    // codepoint and walks back to 0.
    let bytes = "🦀".as_bytes();
    for max in 0..=3 {
        assert_eq!(
            tokens::utf8_boundary_floor(bytes, max),
            0,
            "max={max} should walk back to 0"
        );
    }
    assert_eq!(tokens::utf8_boundary_floor(bytes, 4), 4);
}

// ──────────────────────────────────────────────────────────────────────
// 10. SIMD UTF-8 codepoint counter parity
// ──────────────────────────────────────────────────────────────────────

#[test]
fn utf8_count_chars_matches_chars_count() {
    let cases = [
        "",
        "a",
        "abc",
        "héllo",
        "🦀 Rust 🦀",
        "你好世界",
        "Mixed: ASCII + héllo + 你好 + 🦀 + こんにちは",
    ];
    for s in cases {
        let want = s.chars().count();
        let got = tokens::utf8_count_chars(s.as_bytes());
        assert_eq!(got, want, "char count drift on {s:?}");
    }
}

#[test]
fn utf8_count_chars_long_input_simd_path() {
    // Trigger the SIMD lane (>16 bytes ASCII) plus the tail.
    let s: String = "abcdefghijklmnop".repeat(64) + "🦀";
    let want = s.chars().count();
    let got = tokens::utf8_count_chars(s.as_bytes());
    assert_eq!(got, want);
}

// ──────────────────────────────────────────────────────────────────────
// 11. Thread-safety smoke
// ──────────────────────────────────────────────────────────────────────

#[test]
fn cl100k_concurrent_encode_is_safe() {
    let tok: Arc<&'static Tokenizer> = Arc::new(tokens::cl100k_base().unwrap());
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let t = Arc::clone(&tok);
            thread::spawn(move || {
                let text = format!("thread {i} encoding multiple times in parallel");
                let mut last: Option<Vec<u32>> = None;
                for _ in 0..50 {
                    let r = t.encode_ordinary(&text).expect("encode failed");
                    if let Some(prev) = &last {
                        assert_eq!(prev, &r, "thread-{i} produced divergent encodes");
                    }
                    last = Some(r);
                }
                last.unwrap()
            })
        })
        .collect();
    for h in handles {
        let r = h.join().unwrap();
        assert!(!r.is_empty());
    }
}

// ──────────────────────────────────────────────────────────────────────
// 12. Decode round-trip
// ──────────────────────────────────────────────────────────────────────

#[test]
fn cl100k_encode_then_decode_round_trips() {
    let tok = tokens::cl100k_base().unwrap();
    for s in [
        "hello world",
        "The quick brown fox.",
        "héllo café",
        "你好世界",
        "🦀 Rust 🦀",
    ] {
        let ranks = tok.encode_ordinary(s).unwrap();
        let bytes = tok.decode_bytes(&ranks).unwrap();
        let recovered = std::str::from_utf8(&bytes)
            .unwrap_or_else(|e| panic!("decoded bytes not UTF-8 for {s:?}: {e}"));
        assert_eq!(recovered, s, "round-trip failure on {s:?}");
    }
}

#[test]
fn cl100k_decode_skips_special_tokens() {
    let tok = tokens::cl100k_base().unwrap();
    let ranks = tok
        .encode_with_special_tokens("hi <|endoftext|> bye")
        .unwrap();
    let bytes = tok.decode_bytes(&ranks).unwrap();
    let recovered = std::str::from_utf8(&bytes).unwrap();
    // The endoftext literal is consumed as a special token at encode
    // time and skipped at decode time → only its surrounding text
    // round-trips. The space delimiters around it become a single
    // run because both adjoin the special slot.
    assert!(recovered.contains("hi"));
    assert!(recovered.contains("bye"));
    assert!(!recovered.contains("<|endoftext|>"));
}

// ──────────────────────────────────────────────────────────────────────
// 13. Error surface
// ──────────────────────────────────────────────────────────────────────

#[test]
fn loading_corrupt_blob_returns_bad_magic() {
    let bogus: &'static [u8] = b"NOTAVALIDBLOBHEADER--------";
    let r = Tokenizer::from_blob(bogus, "foo", vec![]);
    assert!(matches!(
        r,
        Err(BpeError::BadMagic) | Err(BpeError::BadLayout)
    ));
}

#[test]
fn embedded_via_c23_embed_returns_bool() {
    // Either path is valid — we only assert the call doesn't panic.
    let _ = Tokenizer::embedded_via_c23_embed();
}

// ──────────────────────────────────────────────────────────────────────
// 14. count_tokens drift gate against tiktoken-rs
// ──────────────────────────────────────────────────────────────────────

#[test]
fn count_tokens_drift_gate_cl100k() {
    let reference = tiktoken_rs::cl100k_base().unwrap();
    for text in [
        "hello world",
        "The Lorem ipsum text is a placeholder.",
        "你好世界 + héllo + 🦀",
        "",
        "single",
    ] {
        let ours = tokens::count_tokens("gpt-4-turbo", text);
        let theirs = reference.encode_with_special_tokens(text).len();
        assert_eq!(
            ours.count, theirs,
            "count drift on {text:?}: ours={} theirs={}",
            ours.count, theirs
        );
    }
}

#[test]
fn count_tokens_drift_gate_o200k() {
    let reference = tiktoken_rs::o200k_base().unwrap();
    for text in [
        "hello world",
        "ChatGPT-4o is multimodal.",
        "你好世界 + héllo + 🦀",
        "",
    ] {
        let ours = tokens::count_tokens("gpt-4o-mini", text);
        let theirs = reference.encode_with_special_tokens(text).len();
        assert_eq!(
            ours.count, theirs,
            "count drift on {text:?}: ours={} theirs={}",
            ours.count, theirs
        );
    }
}
