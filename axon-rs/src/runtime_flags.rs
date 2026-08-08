//! §Fase 33.x.h — Process-wide runtime opt-in flags.
//!
//! Adopter-tunable runtime behaviors that DEFAULT to OFF (to
//! preserve v1.24.0 wire byte-compat) and can be flipped ON for
//! production-time experimentation or vertical-aware enterprise
//! enhancements.
//!
//! # Why not on `ServerConfig`?
//!
//! ServerConfig is constructed in 29+ call sites across the test
//! suite; adding fields there means a one-time-but-broad churn.
//! Process-wide flags are simpler for OSS opt-in features that
//! don't affect the wire format or the auth surface. The
//! `std::sync::Mutex<bool>` indirection serializes read+write so
//! there's no torn-write under concurrent test access.
//!
//! # D9 contract (Fase 33.x cycle)
//!
//! [`tokenizer_fallback_enabled`] gates the BPE-tokenized chunking
//! that replaces the legacy whitespace 3-word grouping on the SSE
//! LEGACY path. Defaults to OFF — the wire body stays byte-
//! identical with v1.24.0 + with 33.x.b-g for adopters that
//! don't opt in.
//!
//! When ON + the LEGACY path activates (flow shape unsupported,
//! backend unknown, etc.), each step's full output goes through
//! `axon_csys::tokens::cl100k_base()` and one StepToken event is
//! emitted per BPE-token-decode-boundary. Adopter sees ~1-token
//! granularity that matches real provider chunk size on English
//! prose; non-English degrades to UTF-8-replacement chars at
//! invalid token-boundary slices (rare in practice).
//!
//! # Test isolation
//!
//! Tests that toggle the flag use the `tokenizer_fallback_guard`
//! RAII helper or the `with_tokenizer_fallback` scoped runner.
//! Both restore the previous flag value on drop, so a test that
//! crashes mid-body doesn't leak state into the next test.

use std::sync::Mutex;

/// Process-wide flag — OFF by default. `std::sync::Mutex` (not
/// `AtomicBool`) so the test-side guard can atomically capture the
/// previous value during set + restore it on drop without races.
static TOKENIZER_FALLBACK: Mutex<bool> = Mutex::new(false);

/// Read the current flag value. Cheap — single Mutex acquisition.
///
/// §Fase 119.h — NOT WIRED. This was read once per chunking decision in
/// `run_streaming_legacy_path`, which sub-fase 33.z.e deleted. Nothing in the
/// crate calls this function today except its own tests, and nothing calls
/// [`set_tokenizer_fallback`] either, so the flag is permanently `false`
/// whatever an adopter does. See the module header.
pub fn tokenizer_fallback_enabled() -> bool {
    *TOKENIZER_FALLBACK
        .lock()
        .expect("tokenizer_fallback flag mutex poisoned")
}

/// Set the flag explicitly. Returns the previous value so callers
/// can restore it (the [`TokenizerFallbackGuard`] RAII helper does
/// this automatically).
pub fn set_tokenizer_fallback(enabled: bool) -> bool {
    let mut g = TOKENIZER_FALLBACK
        .lock()
        .expect("tokenizer_fallback flag mutex poisoned");
    let prev = *g;
    *g = enabled;
    prev
}

/// RAII guard that restores the flag to its previous value when
/// dropped. Use in tests to scope a flag mutation to a single
/// `#[tokio::test]` body:
///
/// ```ignore
/// let _guard = TokenizerFallbackGuard::set(true);
/// // ... test body with flag enabled ...
/// // guard drops here → flag restored.
/// ```
pub struct TokenizerFallbackGuard {
    previous: bool,
}

impl TokenizerFallbackGuard {
    /// Set the flag to `enabled` and capture the previous value
    /// for restoration on drop.
    pub fn set(enabled: bool) -> Self {
        let previous = set_tokenizer_fallback(enabled);
        Self { previous }
    }
}

impl Drop for TokenizerFallbackGuard {
    fn drop(&mut self) {
        set_tokenizer_fallback(self.previous);
    }
}

// ────────────────────────────────────────────────────────────────────
//  §Fase 33.z.e — Streaming-via-dispatcher flag RETIRED
// ────────────────────────────────────────────────────────────────────
//
// Pre-33.z.e this module exposed `streaming_via_dispatcher_enabled()`
// + `set_streaming_via_dispatcher(bool)` + `StreamingViaDispatcherGuard`
// for the feature-flagged dispatcher graft (33.z.b alpha, 33.z.c
// stable default-on). 33.z.e DELETES all three symbols — the
// dispatcher is the unconditional production path; there is no
// opt-out.
//
// Any downstream crate that called `set_streaming_via_dispatcher(...)`
// hits an explicit compile error at the v1.26.0 → v1.27.0 upgrade —
// the intended failure shape for the deprecation cycle started in
// 33.y.l and closed here.

// ────────────────────────────────────────────────────────────────────
//  Tokenizer-aware chunking helper
// ────────────────────────────────────────────────────────────────────

/// §Fase 33.x.h — Tokenize `text` into BPE chunks via
/// `axon_csys::tokens::cl100k_base()` and return one `String` per
/// token (or per safe UTF-8 boundary group when a single token
/// produces non-UTF-8 bytes).
///
/// # When this fires
///
/// §Fase 119.h — NEVER, in the current build. It fired from
/// `run_streaming_legacy_path` when [`tokenizer_fallback_enabled`] returned
/// `true`; 33.z.e deleted that function, and no other caller took its place.
/// The paragraph that used to stand here described the wiring as live.
///
/// # Fallback semantics
///
/// If tokenizer construction or encoding fails — which is now the DEFAULT,
/// since §119.h made the C23 BPE kernel opt-in — this returns an empty Vec.
/// The contract was that the caller then falls back to whitespace chunking;
/// with no caller, the empty Vec goes nowhere. Stated plainly so nobody
/// re-derives the guarantee from the old text: calling this directly in a
/// build without `csys-native` yields NOTHING, and the round-trip property
/// its tests assert holds only with the kernel present.
///
/// # UTF-8 boundary safety
///
/// BPE tokens can split mid-codepoint (e.g., a single Chinese
/// character may take multiple tokens). For each token's decoded
/// bytes we use `String::from_utf8_lossy` which substitutes
/// U+FFFD for invalid sequences. Adopters on non-Latin scripts
/// may see replacement chars when tokens land mid-codepoint;
/// for English prose this never happens in practice.
pub fn bpe_chunk_text(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let tokenizer = match axon_csys::tokens::cl100k_base() {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let token_ids = match tokenizer.encode_ordinary(text) {
        Ok(ids) => ids,
        Err(_) => return Vec::new(),
    };
    let mut chunks = Vec::with_capacity(token_ids.len());
    for id in &token_ids {
        let bytes = match tokenizer.decode_bytes(&[*id]) {
            Ok(b) => b,
            Err(_) => continue,
        };
        // `String::from_utf8_lossy` substitutes U+FFFD for invalid
        // UTF-8 sequences (mid-codepoint token splits). For most
        // English prose tokens are entire words or word-fragments,
        // never split codepoints.
        let s = String::from_utf8_lossy(&bytes).to_string();
        if !s.is_empty() {
            chunks.push(s);
        }
    }
    chunks
}

// ────────────────────────────────────────────────────────────────────
//  Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize all flag-mutating tests via a shared Mutex.
    /// The lock is held for the duration of the test body so the
    /// flag's value during this test isn't observed by parallel
    /// tests. Tests that don't touch the flag don't need this
    /// guard — `tokenizer_fallback_enabled()` always returns the
    /// default false outside flag-mutation scopes.
    static FLAG_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn flag_default_is_off() {
        let _serial = FLAG_TEST_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // Defensive: another test may have left the flag ON if
        // its panic happened before drop. Reset.
        set_tokenizer_fallback(false);
        assert!(!tokenizer_fallback_enabled());
    }

    #[test]
    fn set_returns_previous_value() {
        let _serial = FLAG_TEST_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        set_tokenizer_fallback(false);
        let prev = set_tokenizer_fallback(true);
        assert!(!prev);
        let prev = set_tokenizer_fallback(false);
        assert!(prev);
    }

    #[test]
    fn guard_restores_flag_on_drop() {
        let _serial = FLAG_TEST_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        set_tokenizer_fallback(false);
        {
            let _g = TokenizerFallbackGuard::set(true);
            assert!(tokenizer_fallback_enabled());
        }
        assert!(!tokenizer_fallback_enabled(), "guard must restore on drop");
    }

    #[test]
    fn guard_restores_to_previous_not_default() {
        let _serial = FLAG_TEST_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        set_tokenizer_fallback(true);
        {
            let _g = TokenizerFallbackGuard::set(false);
            assert!(!tokenizer_fallback_enabled());
        }
        assert!(
            tokenizer_fallback_enabled(),
            "guard restores to PREVIOUS (true), not default (false)"
        );
        // Cleanup.
        set_tokenizer_fallback(false);
    }

    #[test]
    fn bpe_chunk_empty_text_returns_empty_vec() {
        let chunks = bpe_chunk_text("");
        assert!(chunks.is_empty());
    }

    #[cfg(feature = "csys-native")]
    #[test]
    fn bpe_chunk_english_produces_token_level_granularity() {
        // "Hello world" via cl100k_base ⇒ ~2 tokens
        // ("Hello" + " world"). Compare to whitespace chunking
        // (which would emit 1 chunk for "Hello world" via
        // chunks(3) of [Hello, world]).
        let chunks = bpe_chunk_text("Hello world");
        // BPE for English usually yields 1 token per word; we
        // assert ≥1 to remain robust against tokenizer-vocab
        // updates that may merge or split.
        assert!(
            !chunks.is_empty(),
            "BPE on 'Hello world' must produce ≥1 chunk"
        );
        // Concat round-trip preserves content.
        let joined: String = chunks.join("");
        assert_eq!(joined, "Hello world");
    }

    #[cfg(feature = "csys-native")]
    #[test]
    fn bpe_chunk_finer_than_whitespace_for_long_text() {
        // Long English prose: whitespace chunks(3) groups 3 words
        // at a time; BPE chunks ≥1 token per word. BPE should
        // produce strictly more chunks for non-trivial text.
        let text = "The quick brown fox jumps over the lazy dog repeatedly.";
        let word_chunk_count = text.split_whitespace().count().div_ceil(3);
        let bpe_chunks = bpe_chunk_text(text);
        assert!(
            bpe_chunks.len() > word_chunk_count,
            "BPE ({}) must be finer than whitespace chunks-of-3 ({})",
            bpe_chunks.len(),
            word_chunk_count
        );
        // Round-trip content preservation.
        let joined: String = bpe_chunks.join("");
        assert_eq!(joined, text);
    }

    #[cfg(feature = "csys-native")]
    #[test]
    fn bpe_chunk_round_trip_preserves_content() {
        // Round-trip pin: joining all BPE chunks reconstructs the
        // original text byte-for-byte (modulo non-UTF-8 tokens
        // which substitute U+FFFD).
        let text = "axon for axon — four-pillar streaming language.";
        let chunks = bpe_chunk_text(text);
        let joined: String = chunks.join("");
        assert_eq!(joined, text);
    }
}
