/*
 * v1.19.0 — axon-csys public audio kernel header.
 *
 * Two transformer families live here:
 *
 *   1) G.711 μ-law ↔ PCM16 (8-bit logarithmic ↔ 16-bit linear)
 *   2) Linear-interpolation PCM16 resampler (rate conversion)
 *
 * Both are direct ports of `axon-rs/src/ots/native/{mulaw,resample}.rs`,
 * preserving the OTS paper's categorical structure: every kernel is a
 * pure morphism `Hom(A, B)` in the audio sub-category. Composing two
 * kernels is identical (byte-identical for integer kernels, ≤1 LSB for
 * floating-point kernels) to applying the equivalent Rust-side morphism.
 *
 * Linear-logic discipline (OTS section 3.2): inputs are `const`-correct and
 * never mutated; outputs are written exactly once; functions are pure
 * (referentially transparent). Caller owns all storage; the kernel
 * never allocates.
 *
 * Thread safety: all functions are stateless and reentrant.
 */

#ifndef AXON_CSYS_AUDIO_H
#define AXON_CSYS_AUDIO_H

#include <stddef.h>
#include <stdint.h>

/* Pre-C23 GCC short-circuit caveat — see probe.c (nested-#ifdef pattern). */
#ifdef __has_c_attribute
#  if __has_c_attribute(nodiscard)
#    define AXON_CSYS_AUDIO_NODISCARD [[nodiscard]]
#  endif
#endif
#ifndef AXON_CSYS_AUDIO_NODISCARD
#  define AXON_CSYS_AUDIO_NODISCARD
#endif

#ifdef __cplusplus
extern "C" {
#endif

/* --------------------------------------------------------------------------
 * G.711 μ-law (decode + encode)
 * --------------------------------------------------------------------------
 * Algorithm: bit-twiddle implementation per ITU-T G.711 Annex A — no
 * lookup tables (the inlined arithmetic gives the optimiser more room
 * than a 256-byte LUT and is already branch-free on the hot loop).
 *
 * Byte ordering: PCM16 samples are little-endian on the wire (matching
 * the Rust reference impl + adopters that round-trip via WebRTC / RTP).
 * The kernel writes/reads via `int16_t *` so endianness conversion is
 * the caller's responsibility on big-endian machines.
 *
 * Reference vectors (G.711 Annex A): see `axon-csys/tests/audio.rs`.
 * ------------------------------------------------------------------------ */

/* Decode `in_len` μ-law bytes to `in_len` PCM16 samples.
 *
 * Caller MUST allocate `out` with capacity ≥ `in_len * sizeof(int16_t)`.
 * The kernel writes exactly `in_len` `int16_t` samples.
 *
 * No error path — μ-law decode is total over the 8-bit input domain.
 * Empty input (in_len == 0) is a no-op. */
void axon_csys_mulaw_decode(
    const uint8_t *in,
    size_t in_len,
    int16_t *out
);

/* Encode `in_samples` PCM16 samples to `in_samples` μ-law bytes.
 *
 * Caller MUST allocate `out` with capacity ≥ `in_samples`. The kernel
 * writes exactly `in_samples` bytes.
 *
 * Saturating: PCM16 magnitudes above MULAW_CLIP (0x7F7B = 32 635) are
 * clipped before encoding (matches the Rust reference + G.711 spec).
 * Empty input is a no-op. */
void axon_csys_mulaw_encode(
    const int16_t *in,
    size_t in_samples,
    uint8_t *out
);

/* --------------------------------------------------------------------------
 * Linear-interpolation PCM16 resampler
 * --------------------------------------------------------------------------
 * Polyphase FIR would preserve spectral content better; linear
 * interpolation is what telephony-tier audio (μ-law, 8 kHz) tolerates
 * and that is the canonical OTS use case (matches
 * `axon-rs/src/ots/native/resample.rs`).
 *
 * Floating-point semantics: per-sample work uses `double` precision;
 * rounding is round-half-away-from-zero (C `round()` from <math.h>) to
 * match Rust `f64::round()` byte-identically when the FP path is
 * deterministic on the host. Drift-gate tolerance is ≤1 LSB
 * (founder ratification D6 — 2026-05-08).
 * ------------------------------------------------------------------------ */

/* Returns the number of `int16_t` samples that
 * axon_csys_resample_linear_pcm16() will write for the given input
 * length and sample-rate ratio. Caller uses this to size `out`.
 *
 * Returns 0 iff `in_samples == 0`. Otherwise returns at least 1.
 *
 * Pre-condition: `from_hz > 0` AND `to_hz > 0`.
 *   Violation invokes implementation-defined behaviour (in practice:
 *   division-by-zero trap on x86, undefined on ARM). The Rust shim
 *   asserts this on the safe wrapper boundary. */
AXON_CSYS_AUDIO_NODISCARD
size_t axon_csys_resample_linear_pcm16_output_len(
    size_t in_samples,
    uint32_t from_hz,
    uint32_t to_hz
);

/* Resample `in_samples` PCM16 samples from `from_hz` to `to_hz`
 * via linear interpolation. Writes the count returned by
 * axon_csys_resample_linear_pcm16_output_len(in_samples, from_hz, to_hz)
 * `int16_t` samples to `out`.
 *
 * Caller MUST allocate `out` with capacity ≥ that count.
 *
 * Returns the actual number of samples written. The return value
 * MUST equal axon_csys_resample_linear_pcm16_output_len(...) — any
 * divergence indicates a kernel bug + must be caught by tests.
 *
 * Pre-condition: `from_hz > 0` AND `to_hz > 0`.
 *
 * Identity case: when `from_hz == to_hz` OR `in_samples == 0`, the
 * kernel copies input unchanged (or writes nothing). */
AXON_CSYS_AUDIO_NODISCARD
size_t axon_csys_resample_linear_pcm16(
    const int16_t *in,
    size_t in_samples,
    uint32_t from_hz,
    uint32_t to_hz,
    int16_t *out
);

#ifdef __cplusplus
}
#endif

#endif /* AXON_CSYS_AUDIO_H */
