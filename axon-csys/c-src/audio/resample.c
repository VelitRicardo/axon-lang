/*
 * v1.19.0 — Linear-interpolation PCM16 resampler (scalar baseline).
 *
 * Direct port of `axon-rs/src/ots/native/resample.rs`. The drift gate
 * in `axon-csys/tests/audio.rs` asserts the C output matches the Rust
 * reference within ≤1 LSB (epsilon-bounded per founder ratification D6).
 * Byte-identical match is achieved on most x86-64 + ARM64 hosts when
 * the FP path is deterministic, but standards-strict guarantees on
 * cross-vendor float ordering aren't available — hence the ≤1 LSB
 * tolerance.
 *
 * SIMD note: scalar-only here. Float-SIMD (AVX-2 + NEON) gathers a
 * different rounding sequence vs the per-sample scalar path; activating
 * it would require widening the drift-gate tolerance, which we'd rather
 * gate behind an explicit benchmark in 25.j. Scalar is the canonical
 * baseline; SIMD is opt-in opportunism.
 */

#include "audio.h"

#include <math.h>   /* floor, round */

size_t axon_csys_resample_linear_pcm16_output_len(
    size_t in_samples,
    uint32_t from_hz,
    uint32_t to_hz
) {
    /* Empty input → empty output (preserves identity element of the
     * audio category). Identity rate also returns input unchanged. */
    if (in_samples == 0) {
        return 0;
    }
    if (from_hz == to_hz) {
        return in_samples;
    }
    /* Integer-truncating division matches Rust:
     *   (samples.len() as u64 * to_hz as u64) / from_hz as u64
     * Result clamped up to 1 to ensure non-empty input always
     * yields at least one output sample (the Rust `.max(1)` clause). */
    uint64_t out64 = ((uint64_t)in_samples * (uint64_t)to_hz)
                   / (uint64_t)from_hz;
    if (out64 < 1u) {
        out64 = 1u;
    }
    return (size_t)out64;
}

size_t axon_csys_resample_linear_pcm16(
    const int16_t *in,
    size_t in_samples,
    uint32_t from_hz,
    uint32_t to_hz,
    int16_t *out
) {
    /* Identity short-circuits — matches Rust resample_linear() prelude. */
    if (in_samples == 0) {
        return 0;
    }
    if (from_hz == to_hz) {
        for (size_t i = 0; i < in_samples; ++i) {
            out[i] = in[i];
        }
        return in_samples;
    }

    size_t out_len = axon_csys_resample_linear_pcm16_output_len(
        in_samples, from_hz, to_hz
    );

    /* Pre-cache the FP rate ratio operands. The hot loop computes
     *   src_pos = (i * from_hz) / to_hz   (in FP space)
     * Keeping the cast/multiply outside the inner expression avoids
     * Wconversion warnings + matches Rust's evaluation order. */
    const double from_hz_f = (double)from_hz;
    const double to_hz_f   = (double)to_hz;

    for (size_t i = 0; i < out_len; ++i) {
        /* Map output index back into input timeline. */
        double src_pos = ((double)((uint64_t)i * (uint64_t)from_hz)) / to_hz_f;
        (void)from_hz_f; /* silence -Wunused-but-set-variable on some toolchains */

        double src_idx_f = floor(src_pos);
        size_t src_idx = (size_t)src_idx_f;
        double frac = src_pos - src_idx_f;

        if (src_idx + 1u >= in_samples) {
            /* Boundary: clamp to last sample (matches Rust). */
            out[i] = in[in_samples - 1u];
        } else {
            /* Linear interpolation between adjacent input samples.
             * round() rounds half away from zero, matching Rust's
             * f64::round() — this keeps the drift gate tight. */
            double a = (double)in[src_idx];
            double b = (double)in[src_idx + 1u];
            double interp = a + (b - a) * frac;
            double rounded = round(interp);

            /* Saturate to int16_t — the reference impl casts via
             * `as i16` which is a saturating cast for floats in Rust.
             * Match that semantic explicitly here so the drift gate
             * sees byte-identical bytes on extreme inputs. */
            if (rounded > 32767.0) {
                out[i] = (int16_t)32767;
            } else if (rounded < -32768.0) {
                out[i] = (int16_t)(-32768);
            } else {
                out[i] = (int16_t)rounded;
            }
        }
    }

    return out_len;
}
