/*
 * v1.19.0 — BPE merge engine (public ABI).
 *
 * Pure byte-level Byte-Pair-Encoding kernel that consumes a deterministic
 * binary blob produced by `tools/gen_merges.py` (or any compatible
 * generator) and exposes a single `encode_piece` entry point. The merge
 * algorithm is the canonical tiktoken impl ported verbatim — preserves
 * the mathematics of greedy lowest-rank merge over a doubly-linked
 * sequence of byte spans.
 *
 * Pillar split (founder principle, 2026-05-08):
 *   - C side: hash-table lookup, BPE merge loop, byte-level UTF-8
 *     boundary detection. No allocation in the hot encode path —
 *     the parts array is caller-supplied (Rust shim provides a
 *     stack-allocated SmallVec-equivalent for short pieces).
 *   - Rust side: pretokenisation via fancy-regex (PCRE-compat;
 *     supports the possessive quantifiers + lookahead that
 *     cl100k / o200k patterns rely on); byte-piece feed loop;
 *     special-token handling.
 *
 * Mathematical pillar (preserved verbatim from tiktoken):
 *   - Greedy merge order: at each step pick the adjacent pair with
 *     the LOWEST rank in the merges table; ties broken by leftmost
 *     position. This is observable behaviour — any deviation
 *     produces tokens that drift from tiktoken's reference output
 *     and breaks the byte-identical drift gate (D6).
 *   - Open-addressed hash for O(1) (bytes → rank) lookup via FNV-1a.
 *
 * Embedding (D-tokens, founder-ratified 2026-05-08):
 *   - Primary path: C23 `#embed "<bin>.bin"` directly in `bpe.c` when
 *     the toolchain supports it (`__has_embed` predicate). Currently
 *     gcc ≥15, clang ≥19. MSVC has not shipped #embed yet.
 *   - Fallback path: `build.rs` generates `merges_tables.c` (xxd-style
 *     `static const unsigned char[]`) from the .bin files and links
 *     it in. The C source declares the table symbols `extern` and
 *     either definition wins.
 *   - Either way the embedded blob is available as
 *     (axon_csys_bpe_embedded_cl100k_base, axon_csys_bpe_embedded_o200k_base).
 *
 * Wire format of the .bin blob (all little-endian, no alignment):
 *
 *   offset  bytes  field
 *   ──────  ─────  ─────
 *   0       4      magic "AXBP" (0x42505841)
 *   4       4      version u32 = 1
 *   8       4      vocab_size u32
 *   12      4      regex_pat_len u32
 *   16      N      regex_pat (UTF-8)
 *   16+N    4      entries_byte_count u32 (sanity)
 *   20+N    …      entries: [u8 len][bytes][u32 LE rank]
 *
 * The Rust shim uses its own copy of the regex pat (compiled into
 * the binary). The embedded copy is for cross-validation in the
 * drift gate.
 */

#ifndef AXON_CSYS_TOKENS_BPE_H
#define AXON_CSYS_TOKENS_BPE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* Pre-C23 GCC short-circuit caveat — see probe.c (nested-#ifdef pattern). */
#ifdef __has_c_attribute
#  if __has_c_attribute(nodiscard)
#    define AXON_CSYS_BPE_NODISCARD [[nodiscard]]
#  endif
#endif
#ifndef AXON_CSYS_BPE_NODISCARD
#  define AXON_CSYS_BPE_NODISCARD
#endif

#ifdef __cplusplus
extern "C" {
#endif

/* ──────────────────────────────────────────────────────────────────────
 * Error codes — surface to the Rust shim as `BpeError`. Negative space
 * keeps positive returns free for "successful encode produced N tokens".
 * ────────────────────────────────────────────────────────────────── */

typedef enum {
    AXON_CSYS_BPE_OK              = 0,
    AXON_CSYS_BPE_BAD_MAGIC       = -1,  /* blob does not start with "AXBP" */
    AXON_CSYS_BPE_BAD_VERSION     = -2,  /* unknown blob version */
    AXON_CSYS_BPE_BAD_LAYOUT      = -3,  /* corrupt entries / size mismatch */
    AXON_CSYS_BPE_OOM             = -4,  /* allocation failed */
    AXON_CSYS_BPE_NULL_ARG        = -5,  /* required pointer is NULL */
    AXON_CSYS_BPE_BUFFER_TOO_SMALL = -6, /* out_ranks_capacity insufficient */
    AXON_CSYS_BPE_UNKNOWN_RANK    = -7,  /* rank not present in vocabulary */
    AXON_CSYS_BPE_PIECE_TOO_LONG  = -8,  /* piece exceeds AXON_CSYS_BPE_MAX_PIECE */
    AXON_CSYS_BPE_NOT_FOUND       = -9,  /* token bytes not in vocabulary */
} AxonCsysBpeError;

/* Maximum byte length of a single pretokenized piece the C kernel will
 * encode in one call. tiktoken regexes virtually never produce pieces
 * over a few dozen bytes; 1024 is a comfortable upper bound that keeps
 * the parts array on the stack. */
#define AXON_CSYS_BPE_MAX_PIECE 1024u

/* Opaque encoder handle. Owned by the C side — Rust shim wraps in a
 * struct with `Drop` calling `axon_csys_bpe_destroy`. */
typedef struct AxonCsysBpeEncoder AxonCsysBpeEncoder;

/* ──────────────────────────────────────────────────────────────────────
 * Construction / destruction
 * ────────────────────────────────────────────────────────────────── */

/* Build an encoder from a serialised merges blob. The blob must remain
 * valid for the lifetime of the returned encoder (the encoder borrows
 * pointers into it — zero-copy, no defensive copy). On error returns
 * NULL and writes the error code into `*out_err`. */
AXON_CSYS_BPE_NODISCARD
AxonCsysBpeEncoder* axon_csys_bpe_load(
    const uint8_t* blob,
    size_t blob_len,
    AxonCsysBpeError* out_err);

void axon_csys_bpe_destroy(AxonCsysBpeEncoder* enc);

/* Vocabulary size (number of (bytes, rank) entries). */
uint32_t axon_csys_bpe_vocab_size(const AxonCsysBpeEncoder* enc);

/* Pretokeniser regex pattern as embedded in the blob. Pointer borrowed
 * from the blob; valid for encoder lifetime. */
const uint8_t* axon_csys_bpe_regex_pat(
    const AxonCsysBpeEncoder* enc,
    size_t* out_len);

/* ──────────────────────────────────────────────────────────────────────
 * Encode + decode
 * ────────────────────────────────────────────────────────────────── */

/* Encode a single pretokenised byte piece into a sequence of rank IDs.
 *
 * The classical tiktoken byte_pair_merge algorithm:
 *   1. Initialise parts as the per-byte spans (parts[i] = i).
 *   2. Compute the rank of each adjacent pair.
 *   3. While a pair with rank < UINT32_MAX exists: pick the leftmost
 *      pair with the lowest rank, merge by deleting the right span,
 *      and recompute the ranks of the merged pair (parts[i]) and
 *      its left neighbour (parts[i-1]).
 *   4. Output: the rank of each remaining span.
 *
 * `out_ranks` receives the output rank IDs; `out_ranks_capacity` is
 * the maximum. On success returns AXON_CSYS_BPE_OK and writes the
 * count to `*out_count`. The caller can determine the maximum count
 * upfront — it is bounded by `piece_len` (degenerate worst case
 * where no merges happen). */
AXON_CSYS_BPE_NODISCARD
AxonCsysBpeError axon_csys_bpe_encode_piece(
    const AxonCsysBpeEncoder* enc,
    const uint8_t* piece,
    size_t piece_len,
    uint32_t* out_ranks,
    size_t out_ranks_capacity,
    size_t* out_count);

/* Look up the byte sequence of a single token rank. Returns
 * AXON_CSYS_BPE_OK and writes (`*out_ptr`, `*out_len`) on success.
 * Pointer borrows from the blob. */
AXON_CSYS_BPE_NODISCARD
AxonCsysBpeError axon_csys_bpe_token_bytes(
    const AxonCsysBpeEncoder* enc,
    uint32_t rank,
    const uint8_t** out_ptr,
    size_t* out_len);

/* Look up the rank of a given byte sequence (i.e. inverse of the
 * above). Useful for special-token resolution + drift testing. */
AXON_CSYS_BPE_NODISCARD
AxonCsysBpeError axon_csys_bpe_lookup_rank(
    const AxonCsysBpeEncoder* enc,
    const uint8_t* bytes,
    size_t len,
    uint32_t* out_rank);

/* ──────────────────────────────────────────────────────────────────────
 * Embedded default blobs (cl100k_base + o200k_base)
 *
 * Provided either via C23 #embed (when the toolchain supports it) or
 * via a build.rs-generated `merges_tables.c` (xxd-style fallback). In
 * both cases these symbols are populated by the linker. Returns
 * (NULL, 0) only if the build pipeline is broken — this is a CI-time
 * failure, not a runtime branch.
 * ────────────────────────────────────────────────────────────────── */

const uint8_t* axon_csys_bpe_embedded_cl100k_base(size_t* out_len);
const uint8_t* axon_csys_bpe_embedded_o200k_base(size_t* out_len);

/* True when the primary path (`#embed`) was used at compile time. The
 * Rust shim surfaces this via `Tokenizer::embedded_via_c23_embed()` so
 * adopters can verify modern-toolchain posture in their own CI. */
bool axon_csys_bpe_used_c23_embed(void);

/* ──────────────────────────────────────────────────────────────────────
 * UTF-8 byte boundary detection
 *
 * Returns the largest offset ≤ `max_offset` that lies on a valid UTF-8
 * character boundary in `bytes[0..len]`. Useful when a streaming caller
 * needs to truncate at a safe boundary without splitting a multi-byte
 * codepoint. Scalar fallback always available; SIMD lane-acceleration
 * activates on x86_64 (SSE2) and aarch64 (NEON) at compile time.
 *
 * Behaviour:
 *   - max_offset == 0 → returns 0 (always valid).
 *   - max_offset >= len → returns len (full slice).
 *   - bytes[max_offset] is a continuation byte → walks left until
 *     finding a leading byte (or 0).
 *
 * NOT a full UTF-8 validator — assumes input is already valid UTF-8.
 * For invalid input the result is unspecified but always ≤ max_offset.
 * ────────────────────────────────────────────────────────────────── */

size_t axon_csys_utf8_boundary_floor(
    const uint8_t* bytes,
    size_t len,
    size_t max_offset);

/* Count UTF-8 codepoints in `bytes[0..len]`. Counts non-continuation
 * bytes (top 2 bits ≠ `10`). SIMD-accelerated on x86_64 (SSE2) and
 * aarch64 (NEON); scalar fallback elsewhere. Genuinely a hot path
 * for token-cost estimation on long inputs.
 *
 * Assumes input is valid UTF-8 (or at least has well-formed leading-
 * byte pattern); does not validate. For invalid input the count is
 * still well-defined (counts non-continuation bytes) but loses the
 * "codepoint count" interpretation. */
size_t axon_csys_utf8_count_chars(
    const uint8_t* bytes,
    size_t len);

/* Returns true if `bytes[offset]` is the start of a UTF-8 character
 * (either an ASCII byte 0x00..0x7F or a leading byte 0xC0..0xFF).
 * Returns false for continuation bytes 0x80..0xBF. Single-byte helper
 * used by the boundary-floor scan. */
static inline bool axon_csys_utf8_is_leading_byte(uint8_t b) {
    /* UTF-8 leading bytes: 0xxxxxxx (ASCII) or 11xxxxxx (multi-byte
     * leader). Continuation bytes are 10xxxxxx. */
    return (b & 0xC0u) != 0x80u;
}

#ifdef __cplusplus
}
#endif

#endif /* AXON_CSYS_TOKENS_BPE_H */
