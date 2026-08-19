//! v1.19.0 — Audio kernels (Rust shim).
//!
//! Safe Rust wrappers around the C23 audio kernels in
//! `c-src/audio/{mulaw,resample}.c`. Each kernel preserves the OTS
//! categorical structure documented in the paper: G.711 μ-law decode
//! is a morphism `Hom(mulaw8, pcm16)`; G.711 encode is its right
//! inverse (modulo G.711 quantisation); resample is parametric in
//! the rate pair `(from_hz, to_hz)` and produces a deterministic
//! output length given by [`resample_linear_pcm16_output_len`].
//!
//! Linear-logic discipline: input slices are immutable references
//! (Rust ownership maps directly onto the C `const` pointer + length
//! contract); outputs are freshly allocated `Vec<_>` instances —
//! the kernel writes them once and the caller owns them thereafter.
//! No double-free or double-spend possible.
//!
//! Drift-gate posture (founder ratification D6):
//!   - μ-law (integer) → byte-identical match against Rust ref impl.
//!   - resample (FP)   → ≤1 LSB tolerance (round-half-away-from-zero
//!     matches Rust `f64::round()` on every host we ship).

use std::os::raw::c_void;

extern "C" {
    fn axon_csys_mulaw_decode(input: *const u8, in_len: usize, output: *mut i16);
    fn axon_csys_mulaw_encode(input: *const i16, in_samples: usize, output: *mut u8);
    fn axon_csys_resample_linear_pcm16_output_len(
        in_samples: usize,
        from_hz: u32,
        to_hz: u32,
    ) -> usize;
    fn axon_csys_resample_linear_pcm16(
        input: *const i16,
        in_samples: usize,
        from_hz: u32,
        to_hz: u32,
        output: *mut i16,
    ) -> usize;
}

// Suppress unused-import warning on void if we ever add allocator-
// awareness; keeping the import documents the kernel's no-alloc contract.
const _: () = {
    let _: Option<*mut c_void> = None;
};

// ────────────────────────────────────────────────────────────────────────
// G.711 μ-law ↔ PCM16
// ────────────────────────────────────────────────────────────────────────

/// Decode μ-law-encoded bytes to PCM16 samples (one input byte → one i16).
///
/// Total over the 8-bit input domain — never fails. Empty input returns
/// an empty `Vec`.
pub fn mulaw_decode(input: &[u8]) -> Vec<i16> {
    if input.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0i16; input.len()];
    // SAFETY: `input` is a valid slice of `input.len()` bytes; `out` is a
    // freshly allocated buffer of `input.len()` `i16` slots. The C kernel
    // writes exactly `input.len()` samples — no over-write possible.
    unsafe {
        axon_csys_mulaw_decode(input.as_ptr(), input.len(), out.as_mut_ptr());
    }
    out
}

/// Encode PCM16 samples to μ-law-encoded bytes (one input i16 → one byte).
///
/// Magnitudes above the G.711 clip threshold (32 635) are saturated.
/// Empty input returns an empty `Vec`.
pub fn mulaw_encode(input: &[i16]) -> Vec<u8> {
    if input.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0u8; input.len()];
    // SAFETY: see mulaw_decode — symmetric contract.
    unsafe {
        axon_csys_mulaw_encode(input.as_ptr(), input.len(), out.as_mut_ptr());
    }
    out
}

// ────────────────────────────────────────────────────────────────────────
// Linear-interpolation PCM16 resampler
// ────────────────────────────────────────────────────────────────────────

/// Errors that can arise at the safe Rust boundary of the resample kernel.
///
/// The C kernel itself has no error path — preconditions are enforced
/// here so adopters never see undefined behaviour from a bad rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResampleError {
    /// Either `from_hz` or `to_hz` was zero — division-by-zero risk.
    InvalidRate { from_hz: u32, to_hz: u32 },
}

impl std::fmt::Display for ResampleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResampleError::InvalidRate { from_hz, to_hz } => write!(
                f,
                "resample rates must both be > 0 (got from={from_hz}, to={to_hz})"
            ),
        }
    }
}

impl std::error::Error for ResampleError {}

/// Returns the number of `i16` samples that [`resample_linear_pcm16`]
/// will write for the given input length and rate pair.
///
/// Wraps the C `axon_csys_resample_linear_pcm16_output_len` exactly;
/// the `Result` adds the precondition check that the C side relies on.
pub fn resample_linear_pcm16_output_len(
    in_samples: usize,
    from_hz: u32,
    to_hz: u32,
) -> Result<usize, ResampleError> {
    if from_hz == 0 || to_hz == 0 {
        return Err(ResampleError::InvalidRate { from_hz, to_hz });
    }
    // SAFETY: pure arithmetic, no pointer ops; precondition checked above.
    let len = unsafe { axon_csys_resample_linear_pcm16_output_len(in_samples, from_hz, to_hz) };
    Ok(len)
}

/// Resample PCM16 samples from `from_hz` to `to_hz` via linear interpolation.
///
/// Produces a `Vec<i16>` whose length is exactly the value returned by
/// [`resample_linear_pcm16_output_len`] for the same arguments.
///
/// Identity rate (`from_hz == to_hz`) returns the input unchanged;
/// empty input returns an empty `Vec`.
pub fn resample_linear_pcm16(
    input: &[i16],
    from_hz: u32,
    to_hz: u32,
) -> Result<Vec<i16>, ResampleError> {
    if from_hz == 0 || to_hz == 0 {
        return Err(ResampleError::InvalidRate { from_hz, to_hz });
    }
    if input.is_empty() {
        return Ok(Vec::new());
    }
    // SAFETY: pure arithmetic check above; identity case handled by C.
    let out_len =
        unsafe { axon_csys_resample_linear_pcm16_output_len(input.len(), from_hz, to_hz) };
    let mut out = vec![0i16; out_len];
    // SAFETY: `input` is valid for `input.len()` reads; `out` is a fresh
    // buffer of `out_len` slots; the kernel writes exactly `out_len`
    // samples and returns that count (asserted below).
    let written = unsafe {
        axon_csys_resample_linear_pcm16(
            input.as_ptr(),
            input.len(),
            from_hz,
            to_hz,
            out.as_mut_ptr(),
        )
    };
    debug_assert_eq!(
        written, out_len,
        "resample kernel wrote {written} samples; expected {out_len}",
    );
    Ok(out)
}
