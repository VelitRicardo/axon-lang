//! §Fase 25.g — BPE tokeniser (Rust shim).
//!
//! Safe Rust wrapper around the C23 BPE merge engine in
//! `c-src/tokens/bpe.c`. Pillar split:
//!
//!   - C side: byte-level BPE merge loop with O(1) hash lookup,
//!     SIMD UTF-8 codepoint counter, embedded merges tables (via
//!     C23 `#embed` when the toolchain supports it; via Rust
//!     `include_bytes!` otherwise).
//!   - Rust side: pretokenisation via [`fancy-regex`] (PCRE-compat
//!     regex engine that supports the possessive quantifiers +
//!     lookaround the cl100k / o200k patterns rely on); special-
//!     token handling; thread-safe handle wrapper around the C
//!     encoder; `OnceLock` cache for the two default encoders.
//!
//! The resulting surface is a drop-in replacement for the
//! tiktoken-rs subset that `axon-rs::backends::tokens` consumed —
//! `count_tokens(model, text) -> TokenCount` with the same routing
//! semantics. Adopters who previously paid the tiktoken-rs HTTP
//! download at build-time now get an offline encoder backed by
//! the merges tables committed to the repo.
//!
//! # Drift gate (D6)
//!
//! For every text fed through `encode_with_special_tokens`, the
//! Rust shim's output MUST be byte-identical to tiktoken-rs's
//! `CoreBPE::encode_with_special_tokens` for the same encoding.
//! The drift gate test suite cross-validates against tiktoken-rs
//! kept as a `[dev-dependency]` (production builds do not link it).

#[cfg(feature = "native")]
use std::ffi::c_void;
#[cfg(feature = "native")]
use std::ptr;
use std::sync::OnceLock;

#[cfg(feature = "native")]
use fancy_regex::Regex;

// ──────────────────────────────────────────────────────────────────────
// Embedded merges blobs.
//
// The Rust shim ALWAYS includes the bytes via `include_bytes!`. When
// the C side's `#embed` path is also active (modern gcc/clang), the
// C-side bytes exist too — the Rust shim ignores them and feeds its
// own copy to `axon_csys_bpe_load`. The cost is one duplicated copy
// in the binary on toolchains that support both paths; in practice
// today's MSVC (the canonical target) has only the Rust path active.
// ──────────────────────────────────────────────────────────────────────

const MERGES_CL100K_BASE: &[u8] = include_bytes!("../c-src/tokens/merges_cl100k_base.bin");
const MERGES_O200K_BASE: &[u8] = include_bytes!("../c-src/tokens/merges_o200k_base.bin");

// ──────────────────────────────────────────────────────────────────────
// Raw FFI declarations — must mirror bpe.h byte-for-byte.
// ──────────────────────────────────────────────────────────────────────

#[allow(non_camel_case_types)]
#[cfg(feature = "native")]
type axon_csys_bpe_error_t = i32;

#[cfg(feature = "native")]
const AXON_CSYS_BPE_OK: axon_csys_bpe_error_t = 0;
#[cfg(feature = "native")]
const AXON_CSYS_BPE_BAD_MAGIC: axon_csys_bpe_error_t = -1;
#[cfg(feature = "native")]
const AXON_CSYS_BPE_BAD_VERSION: axon_csys_bpe_error_t = -2;
#[cfg(feature = "native")]
const AXON_CSYS_BPE_BAD_LAYOUT: axon_csys_bpe_error_t = -3;
#[cfg(feature = "native")]
const AXON_CSYS_BPE_OOM: axon_csys_bpe_error_t = -4;
#[cfg(feature = "native")]
const AXON_CSYS_BPE_NULL_ARG: axon_csys_bpe_error_t = -5;
#[cfg(feature = "native")]
const AXON_CSYS_BPE_BUFFER_TOO_SMALL: axon_csys_bpe_error_t = -6;
#[cfg(feature = "native")]
const AXON_CSYS_BPE_UNKNOWN_RANK: axon_csys_bpe_error_t = -7;
#[cfg(feature = "native")]
const AXON_CSYS_BPE_PIECE_TOO_LONG: axon_csys_bpe_error_t = -8;
#[cfg(feature = "native")]
const AXON_CSYS_BPE_NOT_FOUND: axon_csys_bpe_error_t = -9;

#[cfg(feature = "native")]
extern "C" {
    fn axon_csys_bpe_load(
        blob: *const u8,
        blob_len: usize,
        out_err: *mut axon_csys_bpe_error_t,
    ) -> *mut c_void;
    fn axon_csys_bpe_destroy(enc: *mut c_void);
    fn axon_csys_bpe_vocab_size(enc: *const c_void) -> u32;
    fn axon_csys_bpe_regex_pat(enc: *const c_void, out_len: *mut usize) -> *const u8;
    fn axon_csys_bpe_encode_piece(
        enc: *const c_void,
        piece: *const u8,
        piece_len: usize,
        out_ranks: *mut u32,
        out_ranks_capacity: usize,
        out_count: *mut usize,
    ) -> axon_csys_bpe_error_t;
    fn axon_csys_bpe_token_bytes(
        enc: *const c_void,
        rank: u32,
        out_ptr: *mut *const u8,
        out_len: *mut usize,
    ) -> axon_csys_bpe_error_t;
    fn axon_csys_bpe_lookup_rank(
        enc: *const c_void,
        bytes: *const u8,
        len: usize,
        out_rank: *mut u32,
    ) -> axon_csys_bpe_error_t;
    fn axon_csys_bpe_used_c23_embed() -> bool;
    fn axon_csys_utf8_boundary_floor(bytes: *const u8, len: usize, max_offset: usize) -> usize;
    fn axon_csys_utf8_count_chars(bytes: *const u8, len: usize) -> usize;
}

// ──────────────────────────────────────────────────────────────────────
// Error type
// ──────────────────────────────────────────────────────────────────────

/// Errors surfaced from the C BPE engine + Rust shim. Kept as a single
/// enum (not split into encoder-vs-shim) so adopters get a uniform error
/// surface regardless of which side detected the problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BpeError {
    /// Source merges blob does not start with the "AXBP" magic.
    BadMagic,
    /// Source merges blob has an unrecognised version number.
    BadVersion,
    /// Source merges blob is corrupt — entries don't match header sizes.
    BadLayout,
    /// Allocation failed during encoder construction.
    OutOfMemory,
    /// Required pointer was NULL (FFI invariant violation).
    NullArg,
    /// Output buffer too small for the encode result.
    BufferTooSmall,
    /// §Fase 119.h — the crate was built without the `native` feature, so the
    /// C23 BPE kernel is not linked and no tokeniser can be constructed.
    ///
    /// This is NOT a failure to recover from: it is the declared shape of a
    /// toolchain-free build. Callers degrade — `count_tokens` returns a
    /// `CountKind::Estimate`, and `mandate`'s Tier 1 logit-bias bans simply
    /// do not apply while Tier 2 carries the guarantee. Both degradations
    /// were designed in before this feature existed.
    NativeKernelUnavailable,
    /// Rank ID was out of vocabulary range.
    UnknownRank,
    /// Pretokenised piece exceeded `AXON_CSYS_BPE_MAX_PIECE` bytes.
    PieceTooLong,
    /// Byte sequence not present in the vocabulary (only happens on
    /// corrupted blob — every byte 0..255 has its own token in
    /// cl100k / o200k by construction).
    NotFound,
    /// Pretokeniser regex compilation failed (programmer error —
    /// the patterns are checked against fancy-regex at compile time
    /// of this module, but the error type exists for future
    /// custom-pattern support).
    RegexCompileError(String),
    /// fancy-regex returned an error during a match attempt
    /// (typically pathological input or recursion-limit hit).
    RegexMatchError(String),
}

impl std::fmt::Display for BpeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadMagic => write!(f, "BPE: source blob bad magic (expected \"AXBP\")"),
            Self::BadVersion => write!(f, "BPE: source blob unknown version"),
            Self::BadLayout => write!(f, "BPE: source blob corrupt layout"),
            Self::OutOfMemory => write!(f, "BPE: encoder allocation failed"),
            Self::NullArg => write!(f, "BPE: FFI received NULL pointer"),
            Self::BufferTooSmall => write!(f, "BPE: output buffer too small"),
            Self::NativeKernelUnavailable => write!(
                f,
                "BPE: built without the `native` feature, so the C23 kernel is not                  linked and no tokeniser exists. Token counts degrade to estimates                  and mandate's Tier 1 logit-bias bans do not apply; rebuild with                  `--features csys-native` (needs a C compiler) for exact BPE"
            ),
            Self::UnknownRank => write!(f, "BPE: rank id out of vocabulary range"),
            Self::PieceTooLong => write!(f, "BPE: pretokenised piece exceeds maximum length"),
            Self::NotFound => write!(f, "BPE: byte sequence not in vocabulary"),
            Self::RegexCompileError(s) => write!(f, "BPE: regex compile error: {s}"),
            Self::RegexMatchError(s) => write!(f, "BPE: regex match error: {s}"),
        }
    }
}

impl std::error::Error for BpeError {}

#[cfg(feature = "native")]
fn err_from_code(code: axon_csys_bpe_error_t) -> Option<BpeError> {
    match code {
        AXON_CSYS_BPE_OK => None,
        AXON_CSYS_BPE_BAD_MAGIC => Some(BpeError::BadMagic),
        AXON_CSYS_BPE_BAD_VERSION => Some(BpeError::BadVersion),
        AXON_CSYS_BPE_BAD_LAYOUT => Some(BpeError::BadLayout),
        AXON_CSYS_BPE_OOM => Some(BpeError::OutOfMemory),
        AXON_CSYS_BPE_NULL_ARG => Some(BpeError::NullArg),
        AXON_CSYS_BPE_BUFFER_TOO_SMALL => Some(BpeError::BufferTooSmall),
        AXON_CSYS_BPE_UNKNOWN_RANK => Some(BpeError::UnknownRank),
        AXON_CSYS_BPE_PIECE_TOO_LONG => Some(BpeError::PieceTooLong),
        AXON_CSYS_BPE_NOT_FOUND => Some(BpeError::NotFound),
        // Defensive — covers future error codes added on the C side
        // before the Rust shim is updated.
        _ => Some(BpeError::BadLayout),
    }
}

// ──────────────────────────────────────────────────────────────────────
// Tokenizer — encoder + pretokeniser bundle
// ──────────────────────────────────────────────────────────────────────

/// A BPE tokeniser tied to a specific encoding (cl100k_base / o200k_base
/// / a custom blob). Construction is non-trivial — building the hash
/// table for the vocabulary is O(vocab_size). Adopters should cache the
/// tokeniser at process scope (the [`cl100k_base`] / [`o200k_base`]
/// helpers do this via `OnceLock`).
/// §Fase 119.h — `Tokenizer` without the C23 kernel: **uninhabited**.
///
/// A toolchain-free build links no BPE engine, so a tokeniser cannot exist.
/// Modelling that as an empty enum rather than a struct-that-errors is the
/// §118 `PinnedConn` pattern, and it buys two things:
///
/// 1. **Signatures stay profile-independent.** `cl100k_base() ->
///    Result<&'static Tokenizer, BpeError>` is the same type in both builds,
///    so downstream code compiles unchanged and simply takes the `Err` arm.
/// 2. **The impossible paths are proved impossible, not asserted.** Each
///    method below is `match *self {}` — the compiler accepts it precisely
///    because no value of this type can be constructed, so there is no
///    `unreachable!()` to trust and no panic to ship.
#[cfg(not(feature = "native"))]
pub enum Tokenizer {}

#[cfg(not(feature = "native"))]
impl Tokenizer {
    pub fn from_blob(
        _blob: &'static [u8],
        _pat: &str,
        _special: Vec<(String, u32)>,
    ) -> Result<Self, BpeError> {
        Err(BpeError::NativeKernelUnavailable)
    }
    pub fn vocab_size(&self) -> u32 {
        match *self {}
    }
    pub fn encode_with_special_tokens(&self, _text: &str) -> Result<Vec<u32>, BpeError> {
        match *self {}
    }
    pub fn encode_ordinary(&self, _text: &str) -> Result<Vec<u32>, BpeError> {
        match *self {}
    }
    pub fn lookup_rank(&self, _bytes: &[u8]) -> Option<u32> {
        match *self {}
    }
    pub fn decode_bytes(&self, _ranks: &[u32]) -> Result<Vec<u8>, BpeError> {
        match *self {}
    }
    pub fn embedded_pat(&self) -> &str {
        match *self {}
    }
    /// No kernel, so nothing was embedded by `#embed` or otherwise.
    pub fn embedded_via_c23_embed() -> bool {
        false
    }
}

#[cfg(feature = "native")]
pub struct Tokenizer {
    /// Owned C handle. Freed in `Drop`.
    handle: *mut c_void,
    /// Pretokeniser regex (Rust-side, compiled by fancy-regex).
    pretokeniser: Regex,
    /// Special-token name → rank. Cheap to clone; small map.
    special_tokens: Vec<(String, u32)>,
    /// Vocab size cached for fast access.
    vocab_size: u32,
}

// SAFETY: the C encoder is read-only after construction (the BPE
// merge loop never mutates encoder state — it allocates parts on
// the caller's stack). Therefore the handle can cross threads
// freely.
#[cfg(feature = "native")]
unsafe impl Send for Tokenizer {}
#[cfg(feature = "native")]
unsafe impl Sync for Tokenizer {}

#[cfg(feature = "native")]
impl Tokenizer {
    /// Construct a tokeniser from a serialised merges blob, a
    /// pretokeniser regex pattern, and a list of special tokens.
    /// The blob must remain valid for the lifetime of the tokeniser
    /// — adopters typically use a `'static` slice (`include_bytes!`).
    pub fn from_blob(
        blob: &'static [u8],
        pretokeniser_pat: &str,
        special_tokens: Vec<(String, u32)>,
    ) -> Result<Self, BpeError> {
        let mut err: axon_csys_bpe_error_t = 0;
        let handle = unsafe { axon_csys_bpe_load(blob.as_ptr(), blob.len(), &mut err as *mut _) };
        if handle.is_null() {
            return Err(err_from_code(err).unwrap_or(BpeError::BadLayout));
        }
        let pretokeniser =
            Regex::new(pretokeniser_pat).map_err(|e| BpeError::RegexCompileError(e.to_string()))?;
        let vocab_size = unsafe { axon_csys_bpe_vocab_size(handle) };
        Ok(Self {
            handle,
            pretokeniser,
            special_tokens,
            vocab_size,
        })
    }

    /// Number of (bytes, rank) entries in the vocabulary — does NOT
    /// include special tokens (those are appended by the shim).
    pub fn vocab_size(&self) -> u32 {
        self.vocab_size
    }

    /// True when the C side embedded the merges blobs via the C23
    /// `#embed` directive at compile time. Useful for adopters
    /// verifying modern-toolchain posture in their CI matrix.
    pub fn embedded_via_c23_embed() -> bool {
        unsafe { axon_csys_bpe_used_c23_embed() }
    }

    /// Encode a UTF-8 string into a sequence of BPE rank IDs,
    /// recognising special tokens as atomic units. This mirrors
    /// tiktoken's `encode_with_special_tokens` semantics:
    ///
    ///   1. Find any occurrence of a special-token literal in the
    ///      remaining text — emit its rank, advance past it.
    ///   2. Otherwise, pretokenise the next slice up to the nearest
    ///      special-token occurrence using the regex; for each
    ///      pretoken piece, run BPE merge over its bytes.
    ///
    /// Returns the rank IDs in encounter order.
    pub fn encode_with_special_tokens(&self, text: &str) -> Result<Vec<u32>, BpeError> {
        let mut out = Vec::with_capacity(text.len() / 4 + 1);
        let mut cursor = 0usize;
        let bytes = text.as_bytes();
        while cursor < bytes.len() {
            // Find the nearest special-token literal in text[cursor..].
            let mut next_special: Option<(usize, &(String, u32))> = None;
            for spec in &self.special_tokens {
                if let Some(pos) = text[cursor..].find(spec.0.as_str()) {
                    let abs = cursor + pos;
                    match next_special {
                        None => next_special = Some((abs, spec)),
                        Some((current, _)) if abs < current => {
                            next_special = Some((abs, spec));
                        }
                        _ => {}
                    }
                }
            }
            let (slice_end, after_special) = match next_special {
                Some((pos, spec)) => (pos, Some((pos + spec.0.len(), spec.1))),
                None => (bytes.len(), None),
            };
            // Pretokenise + BPE-encode text[cursor..slice_end].
            self.encode_ordinary_into(&text[cursor..slice_end], &mut out)?;
            // Emit the special token if any.
            if let Some((next_cursor, rank)) = after_special {
                out.push(rank);
                cursor = next_cursor;
            } else {
                break;
            }
        }
        Ok(out)
    }

    /// Encode a UTF-8 string treating special-token literals as
    /// regular text (no atomic recognition). Mirrors tiktoken's
    /// `encode_ordinary`. Useful for pretokenised-pipeline callers
    /// that handle specials at a higher layer.
    pub fn encode_ordinary(&self, text: &str) -> Result<Vec<u32>, BpeError> {
        let mut out = Vec::with_capacity(text.len() / 4 + 1);
        self.encode_ordinary_into(text, &mut out)?;
        Ok(out)
    }

    fn encode_ordinary_into(&self, text: &str, out: &mut Vec<u32>) -> Result<(), BpeError> {
        // Pretokenise via fancy-regex. find_iter yields non-overlapping
        // matches in left-to-right order, which is the contract the
        // tiktoken algorithm requires.
        let matches = self.pretokeniser.find_iter(text);
        for m in matches {
            let m = m.map_err(|e| BpeError::RegexMatchError(e.to_string()))?;
            let piece = &text.as_bytes()[m.start()..m.end()];
            // Fast path: if the entire piece is itself a single token
            // in the vocabulary, emit that rank without invoking the
            // BPE merge loop. This matches tiktoken's optimisation.
            if let Some(rank) = self.lookup_rank(piece) {
                out.push(rank);
                continue;
            }
            self.encode_piece_into(piece, out)?;
        }
        Ok(())
    }

    fn encode_piece_into(&self, piece: &[u8], out: &mut Vec<u32>) -> Result<(), BpeError> {
        // Worst case: every byte is its own token. Pre-allocate that
        // much capacity in a temp buffer to avoid the C side reporting
        // BUFFER_TOO_SMALL.
        let mut tmp: Vec<u32> = vec![0u32; piece.len()];
        let mut count: usize = 0;
        let err = unsafe {
            axon_csys_bpe_encode_piece(
                self.handle,
                piece.as_ptr(),
                piece.len(),
                tmp.as_mut_ptr(),
                tmp.len(),
                &mut count as *mut _,
            )
        };
        if let Some(e) = err_from_code(err) {
            return Err(e);
        }
        tmp.truncate(count);
        out.extend(tmp);
        Ok(())
    }

    /// Look up a single byte sequence's rank, or `None` if absent.
    pub fn lookup_rank(&self, bytes: &[u8]) -> Option<u32> {
        let mut rank: u32 = 0;
        let err = unsafe {
            axon_csys_bpe_lookup_rank(
                self.handle,
                bytes.as_ptr(),
                bytes.len(),
                &mut rank as *mut _,
            )
        };
        if err == AXON_CSYS_BPE_OK {
            Some(rank)
        } else {
            None
        }
    }

    /// Decode a sequence of rank IDs back to bytes, concatenated in
    /// order. Special-token ranks are skipped (mirrors tiktoken's
    /// `decode_bytes` when called on a stream that may contain
    /// specials emitted by `encode_with_special_tokens`).
    pub fn decode_bytes(&self, ranks: &[u32]) -> Result<Vec<u8>, BpeError> {
        let mut out: Vec<u8> = Vec::with_capacity(ranks.len() * 4);
        for &rank in ranks {
            if self.special_tokens.iter().any(|(_, r)| *r == rank) {
                continue;
            }
            let mut ptr: *const u8 = ptr::null();
            let mut len: usize = 0;
            let err = unsafe {
                axon_csys_bpe_token_bytes(self.handle, rank, &mut ptr as *mut _, &mut len as *mut _)
            };
            if let Some(e) = err_from_code(err) {
                return Err(e);
            }
            // SAFETY: the C engine returned a (ptr, len) borrowed from
            // the merges blob. The blob is `'static` (`include_bytes!`)
            // so the slice outlives the function. `ptr` is non-null on
            // OK return (defensive: also asserted below).
            assert!(!ptr.is_null(), "C engine returned OK with NULL pointer");
            let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
            out.extend_from_slice(slice);
        }
        Ok(out)
    }

    /// Pretokeniser regex pattern as embedded in the source merges
    /// blob. Returned by reference to the C-side blob (zero-copy).
    pub fn embedded_pat(&self) -> &str {
        let mut len: usize = 0;
        let ptr = unsafe { axon_csys_bpe_regex_pat(self.handle, &mut len as *mut _) };
        // SAFETY: the C side pulls the pattern bytes from the source
        // blob (which is 'static). The pat was validated UTF-8 by the
        // generator (gen_merges.py uses .encode("utf-8")).
        let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
        std::str::from_utf8(slice).expect("merges-blob regex_pat must be valid UTF-8")
    }
}

#[cfg(feature = "native")]
impl Drop for Tokenizer {
    fn drop(&mut self) {
        unsafe { axon_csys_bpe_destroy(self.handle) };
    }
}

// ──────────────────────────────────────────────────────────────────────
// Pretokeniser patterns + special-token tables for the two default
// encoders. Lifted from the canonical tiktoken Python sources at
// generator time (see tools/gen_merges.py).
// ──────────────────────────────────────────────────────────────────────

const PAT_CL100K: &str = "'(?i:[sdmt]|ll|ve|re)|[^\\r\\n\\p{L}\\p{N}]?+\\p{L}++|\\p{N}{1,3}+| ?[^\\s\\p{L}\\p{N}]++[\\r\\n]*+|\\s++$|\\s*[\\r\\n]|\\s+(?!\\S)|\\s";
const PAT_O200K: &str = "[^\\r\\n\\p{L}\\p{N}]?[\\p{Lu}\\p{Lt}\\p{Lm}\\p{Lo}\\p{M}]*[\\p{Ll}\\p{Lm}\\p{Lo}\\p{M}]+(?i:'s|'t|'re|'ve|'m|'ll|'d)?|[^\\r\\n\\p{L}\\p{N}]?[\\p{Lu}\\p{Lt}\\p{Lm}\\p{Lo}\\p{M}]+[\\p{Ll}\\p{Lm}\\p{Lo}\\p{M}]*(?i:'s|'t|'re|'ve|'m|'ll|'d)?|\\p{N}{1,3}| ?[^\\s\\p{L}\\p{N}]+[\\r\\n/]*|\\s*[\\r\\n]+|\\s+(?!\\S)|\\s+";

fn cl100k_special_tokens() -> Vec<(String, u32)> {
    vec![
        ("<|endoftext|>".to_string(), 100257),
        ("<|fim_prefix|>".to_string(), 100258),
        ("<|fim_middle|>".to_string(), 100259),
        ("<|fim_suffix|>".to_string(), 100260),
        ("<|endofprompt|>".to_string(), 100276),
    ]
}

fn o200k_special_tokens() -> Vec<(String, u32)> {
    vec![
        ("<|endoftext|>".to_string(), 199999),
        ("<|endofprompt|>".to_string(), 200018),
    ]
}

/// Process-scoped cl100k_base tokeniser. Construction is lazy on
/// first use; subsequent calls return the same handle. Encodes
/// content for OpenAI gpt-3.5 / gpt-4, Kimi, GLM, and Moonshot
/// model families.
pub fn cl100k_base() -> Result<&'static Tokenizer, BpeError> {
    static CELL: OnceLock<Result<Tokenizer, BpeError>> = OnceLock::new();
    CELL.get_or_init(|| {
        Tokenizer::from_blob(MERGES_CL100K_BASE, PAT_CL100K, cl100k_special_tokens())
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// Process-scoped o200k_base tokeniser. Used by gpt-4o / o1 / o3
/// model families.
pub fn o200k_base() -> Result<&'static Tokenizer, BpeError> {
    static CELL: OnceLock<Result<Tokenizer, BpeError>> = OnceLock::new();
    CELL.get_or_init(|| Tokenizer::from_blob(MERGES_O200K_BASE, PAT_O200K, o200k_special_tokens()))
        .as_ref()
        .map_err(Clone::clone)
}

// ──────────────────────────────────────────────────────────────────────
// `count_tokens` — drop-in replacement for axon-rs::backends::tokens.
// ──────────────────────────────────────────────────────────────────────

/// Result kind from [`count_tokens`] — distinguishes exact
/// tokeniser counts from approximate estimates. Mirrors the
/// pre-existing `axon-rs::backends::tokens::CountKind` type so
/// adopters can switch backends without changing call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountKind {
    /// Exact count from the BPE encoder (cl100k / o200k families).
    Exact,
    /// Approximate count from the 4-chars-per-token fallback —
    /// used for Anthropic, Gemini, and unknown models.
    Estimate,
}

/// Unified token count + provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenCount {
    pub count: usize,
    pub kind: CountKind,
}

impl TokenCount {
    pub const fn exact(count: usize) -> Self {
        Self {
            count,
            kind: CountKind::Exact,
        }
    }
    pub const fn estimate(count: usize) -> Self {
        Self {
            count,
            kind: CountKind::Estimate,
        }
    }
}

/// 4-chars-per-token offline estimate, ceiling division.
pub fn estimate(text: &str) -> TokenCount {
    let chars = utf8_count_chars(text.as_bytes());
    TokenCount::estimate(chars.div_ceil(4))
}

/// Count tokens in `text` for the given `model`. Routing mirrors
/// the legacy tiktoken-rs-backed implementation in
/// `axon-rs::backends::tokens`:
///
///   - `gpt-4o` / `o1*` / `o3*`        → o200k_base (exact)
///   - `gpt-*` / `chatgpt-*` / `kimi-*` /
///     `moonshot-*` / `glm-*`           → cl100k_base (exact)
///   - `openrouter:<provider>/<model>` → strip prefix + recurse
///   - everything else (Claude, Gemini,
///     Llama, Mistral, Qwen, Phi, …)   → 4-cpt estimate
pub fn count_tokens(model: &str, text: &str) -> TokenCount {
    let model_lc = model.to_lowercase();
    if let Some(rest) = model_lc.strip_prefix("openrouter:") {
        if let Some((_, model_only)) = rest.split_once('/') {
            return count_tokens(model_only, text);
        }
        return count_tokens(rest, text);
    }
    if model_lc.starts_with("o1") || model_lc.starts_with("o3") || model_lc.starts_with("gpt-4o") {
        if let Ok(tok) = o200k_base() {
            if let Ok(ranks) = tok.encode_with_special_tokens(text) {
                return TokenCount::exact(ranks.len());
            }
        }
    }
    if model_lc.starts_with("gpt-")
        || model_lc.starts_with("chatgpt-")
        || model_lc.starts_with("kimi-")
        || model_lc.starts_with("moonshot-")
        || model_lc.starts_with("glm-")
    {
        if let Ok(tok) = cl100k_base() {
            if let Ok(ranks) = tok.encode_with_special_tokens(text) {
                return TokenCount::exact(ranks.len());
            }
        }
    }
    estimate(text)
}

// ──────────────────────────────────────────────────────────────────────
// SIMD UTF-8 helpers (re-exports of the C kernel surface).
// ──────────────────────────────────────────────────────────────────────

/// Largest offset ≤ `max_offset` that is a valid UTF-8 character
/// boundary in `bytes`. Useful for chunking long inputs without
/// splitting a multi-byte codepoint. See `c-src/tokens/bpe.h` for
/// the full contract.
pub fn utf8_boundary_floor(bytes: &[u8], max_offset: usize) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    #[cfg(feature = "native")]
    {
        unsafe { axon_csys_utf8_boundary_floor(bytes.as_ptr(), bytes.len(), max_offset) }
    }
    // §Fase 119.h — the C kernel is a SIMD FAST PATH, not the meaning. The
    // meaning is "walk back to the nearest non-continuation byte", which is
    // four lines of safe Rust; the drift gate compares them.
    #[cfg(not(feature = "native"))]
    {
        let mut i = max_offset.min(bytes.len());
        while i > 0 && (bytes[i - 1] & 0b1100_0000) == 0b1000_0000 {
            i -= 1;
        }
        // `i` now sits just after a lead byte or at 0; step back over the lead
        // itself only when the sequence it starts is incomplete.
        if i > 0 && i < bytes.len() && (bytes[i] & 0b1100_0000) == 0b1000_0000 {
            i -= 1;
        }
        i
    }
}

/// SIMD-accelerated UTF-8 codepoint count over `bytes`. Counts
/// non-continuation bytes — equivalent to `text.chars().count()`
/// when `bytes` is well-formed UTF-8, but ~5–8× faster on x86_64
/// with SSE2 + ~5× faster on aarch64 with NEON for inputs over
/// ~64 bytes.
pub fn utf8_count_chars(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    #[cfg(feature = "native")]
    {
        unsafe { axon_csys_utf8_count_chars(bytes.as_ptr(), bytes.len()) }
    }
    // §Fase 119.h — same contract as the C: count non-continuation bytes.
    #[cfg(not(feature = "native"))]
    {
        bytes
            .iter()
            .filter(|b| (*b & 0b1100_0000) != 0b1000_0000)
            .count()
    }
}
