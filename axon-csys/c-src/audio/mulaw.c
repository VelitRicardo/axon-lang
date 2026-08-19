/*
 * v1.19.0 — G.711 μ-law ↔ PCM16 transcoder (scalar baseline).
 *
 * Direct port of `axon-rs/src/ots/native/mulaw.rs`. The byte-identical
 * drift gate in `axon-csys/tests/audio.rs` re-implements the Rust
 * algorithm and asserts the C output matches sample-for-sample on
 * every test vector.
 *
 * SIMD note: scalar-only here. Per founder ratification D6 + 4-pillar
 * principle, correctness is the first deliverable; SIMD activation
 * (lookup-table 256-byte LUT for AVX-2 / NEON gather) lands in 25.j
 * once the benchmarks suite quantifies what speedup we actually need.
 * The scalar bit-twiddle below is already branch-free on the hot loop
 * and the Rust impl is the canonical reference — SIMD must produce
 * the SAME bytes (no precision relaxation possible for integer
 * kernels).
 */

#include "audio.h"

/* G.711 magic constants — match `MULAW_BIAS` / `MULAW_CLIP` in
 * `axon-rs/src/ots/native/mulaw.rs`. */
#define AXON_CSYS_MULAW_BIAS  ((int32_t)0x84)
#define AXON_CSYS_MULAW_CLIP  ((int32_t)32635)

/* Decode one μ-law byte to one PCM16 sample.
 *
 * Algorithm (per Rust reference + G.711 Annex A):
 *   1. Bitwise-NOT the stored byte (μ-law on the wire is logically
 *      inverted; storage convention).
 *   2. Extract sign bit (0x80), 3-bit exponent (0x70 >> 4), 4-bit
 *      mantissa (0x0F).
 *   3. Reconstruct magnitude: ((mantissa << 3) + 0x84) << exponent - 0x84.
 *   4. Apply sign.
 *
 * Output range: [-32 124, +32 124] (G.711's effective resolution). */
static inline int16_t axon_csys_mulaw_decode_sample(uint8_t byte) {
    uint8_t inverted = (uint8_t)(~byte);
    int sign = (inverted & 0x80) != 0;
    int32_t exponent = (int32_t)((inverted >> 4) & 0x07);
    int32_t mantissa = (int32_t)(inverted & 0x0F);
    int32_t magnitude = (((mantissa << 3) + AXON_CSYS_MULAW_BIAS) << exponent)
                      - AXON_CSYS_MULAW_BIAS;
    return sign ? (int16_t)(-magnitude) : (int16_t)magnitude;
}

/* Encode one PCM16 sample to one μ-law byte.
 *
 * Algorithm (per Rust reference + G.711 Annex A):
 *   1. Capture sign + take absolute value.
 *   2. Saturate magnitude to MULAW_CLIP (32 635) — anything louder
 *      collapses to the loudest representable sample.
 *   3. Add MULAW_BIAS (0x84).
 *   4. Find the position of the highest set bit (3-bit exponent).
 *   5. Extract 4-bit mantissa from the bits below the exponent.
 *   6. Pack as `~(sign | (exponent << 4) | mantissa)` — the bitwise-NOT
 *      matches the storage convention from decode().
 */
static inline uint8_t axon_csys_mulaw_encode_sample(int16_t sample) {
    int32_t pcm = (int32_t)sample;
    uint8_t sign;
    if (pcm < 0) {
        pcm = -pcm;
        sign = 0x80u;
    } else {
        sign = 0x00u;
    }
    if (pcm > AXON_CSYS_MULAW_CLIP) {
        pcm = AXON_CSYS_MULAW_CLIP;
    }
    pcm += AXON_CSYS_MULAW_BIAS;

    /* Reverse leading-zero scan for the 3-bit exponent. We start from
     * the highest possible exponent slot (bit 14 = 0x4000) and walk
     * down until we find the first set bit. The Rust impl uses the
     * same loop shape — keep them visually aligned to make the
     * drift gate easy to reason about. */
    int32_t exponent = 7;
    int32_t mask = 0x4000;
    while (exponent > 0 && (pcm & mask) == 0) {
        exponent--;
        mask >>= 1;
    }
    int32_t mantissa = (pcm >> (exponent + 3)) & 0x0F;
    uint8_t packed = sign
                   | (uint8_t)((uint32_t)exponent << 4)
                   | (uint8_t)mantissa;
    return (uint8_t)(~packed);
}

/* ---------------------------------------------------------------------------
 * Public API
 * ------------------------------------------------------------------------- */

void axon_csys_mulaw_decode(
    const uint8_t *in,
    size_t in_len,
    int16_t *out
) {
    /* Linear-logic discipline: read each input byte exactly once,
     * write each output sample exactly once. No mutation of `in`
     * (declared `const`). */
    for (size_t i = 0; i < in_len; ++i) {
        out[i] = axon_csys_mulaw_decode_sample(in[i]);
    }
}

void axon_csys_mulaw_encode(
    const int16_t *in,
    size_t in_samples,
    uint8_t *out
) {
    for (size_t i = 0; i < in_samples; ++i) {
        out[i] = axon_csys_mulaw_encode_sample(in[i]);
    }
}
