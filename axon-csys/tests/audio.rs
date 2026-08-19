//! v1.19.0 — Audio kernel test suite.
//!
//! Exercises both the μ-law transcoders and the linear PCM16 resampler.
//! The drift gate against Rust reference impls is enforced by mirroring
//! the algorithms from `axon-rs/src/ots/native/{mulaw,resample}.rs`
//! inline in this file (kept small so the parity is auditable side-by-side).

use axon_csys::{
    mulaw_decode, mulaw_encode, resample_linear_pcm16, resample_linear_pcm16_output_len,
    ResampleError,
};

// ════════════════════════════════════════════════════════════════════════
// Rust reference impls — kept byte-for-byte aligned with
// axon-rs/src/ots/native/{mulaw,resample}.rs so the drift gate is
// auditable by reading both files side-by-side.
// ════════════════════════════════════════════════════════════════════════

const MULAW_BIAS: i16 = 0x84;
const MULAW_CLIP: i16 = 32_635;

fn ref_mulaw_decode_sample(byte: u8) -> i16 {
    let byte = !byte;
    let sign = (byte & 0x80) != 0;
    let exponent = ((byte >> 4) & 0x07) as i32;
    let mantissa = (byte & 0x0F) as i32;
    let magnitude: i32 = (((mantissa << 3) + 0x84) << exponent) - 0x84;
    if sign {
        -(magnitude as i16)
    } else {
        magnitude as i16
    }
}

fn ref_mulaw_encode_sample(sample: i16) -> u8 {
    let mut pcm = sample as i32;
    let sign = if pcm < 0 {
        pcm = -pcm;
        0x80u8
    } else {
        0x00u8
    };
    if pcm > MULAW_CLIP as i32 {
        pcm = MULAW_CLIP as i32;
    }
    pcm += MULAW_BIAS as i32;
    let mut exponent: i32 = 7;
    let mut mask: i32 = 0x4000;
    while exponent > 0 && (pcm & mask) == 0 {
        exponent -= 1;
        mask >>= 1;
    }
    let mantissa = (pcm >> (exponent + 3)) & 0x0F;
    !(sign | ((exponent << 4) as u8) | (mantissa as u8))
}

fn ref_resample_linear(samples: &[i16], from_hz: u32, to_hz: u32) -> Vec<i16> {
    if samples.is_empty() || from_hz == to_hz {
        return samples.to_vec();
    }
    let output_len = ((samples.len() as u64 * to_hz as u64) / from_hz as u64).max(1) as usize;
    let mut out = Vec::with_capacity(output_len);
    for i in 0..output_len {
        let src_pos = (i as u64 * from_hz as u64) as f64 / to_hz as f64;
        let src_idx = src_pos.floor() as usize;
        let frac = src_pos - src_idx as f64;
        if src_idx + 1 >= samples.len() {
            out.push(samples[samples.len() - 1]);
        } else {
            let a = samples[src_idx] as f64;
            let b = samples[src_idx + 1] as f64;
            out.push((a + (b - a) * frac).round() as i16);
        }
    }
    out
}

// ════════════════════════════════════════════════════════════════════════
// μ-law decode
// ════════════════════════════════════════════════════════════════════════

#[test]
fn mulaw_decode_g711_annex_a_reference_vectors() {
    // From `axon-rs/src/ots/native/mulaw.rs::mulaw_decode_matches_reference_vectors`:
    //   stored 0xFF → logical 0x00 → +0
    //   stored 0x7F → logical 0x80 → -0
    //   stored 0x80 → logical 0x7F → +32 124 (largest positive)
    //   stored 0x00 → logical 0xFF → -32 124 (largest negative)
    let pairs: &[(u8, i16)] = &[(0xFF, 0), (0x7F, 0), (0x80, 32_124), (0x00, -32_124)];
    let bytes: Vec<u8> = pairs.iter().map(|p| p.0).collect();
    let decoded = mulaw_decode(&bytes);
    for ((stored, expected), got) in pairs.iter().zip(decoded.iter()) {
        assert_eq!(
            *got, *expected,
            "μ-law 0x{:02X} decoded to {} but G.711 reference is {}",
            stored, got, expected
        );
    }
}

#[test]
fn mulaw_decode_empty_input_returns_empty() {
    assert!(mulaw_decode(&[]).is_empty());
}

#[test]
fn mulaw_decode_single_byte() {
    let result = mulaw_decode(&[0xFF]);
    assert_eq!(result, vec![0]);
}

#[test]
fn mulaw_decode_drift_gate_byte_identical_to_rust_ref() {
    // Exhaustive: μ-law decode is total over the 8-bit input domain, so
    // we can drift-check every possible byte in O(256) work.
    let all_bytes: Vec<u8> = (0u8..=255).collect();
    let c_out = mulaw_decode(&all_bytes);
    let ref_out: Vec<i16> = all_bytes
        .iter()
        .map(|&b| ref_mulaw_decode_sample(b))
        .collect();
    assert_eq!(
        c_out, ref_out,
        "C μ-law decode diverged from Rust reference on the 256-byte input domain",
    );
}

#[test]
fn mulaw_decode_preserves_length() {
    for len in [0usize, 1, 7, 64, 1000, 8192] {
        let input: Vec<u8> = (0..len).map(|i| (i % 256) as u8).collect();
        let out = mulaw_decode(&input);
        assert_eq!(
            out.len(),
            len,
            "decode of {len} bytes produced {} samples",
            out.len()
        );
    }
}

// ════════════════════════════════════════════════════════════════════════
// μ-law encode
// ════════════════════════════════════════════════════════════════════════

#[test]
fn mulaw_encode_zero_is_negative_zero_sentinel() {
    // PCM16 zero (positive) encodes to G.711 stored byte 0xFF
    // (logical 0x00 = +0 magnitude class). This is the canonical
    // "silence" byte adopters check for in voice-activity detection.
    let result = mulaw_encode(&[0]);
    assert_eq!(result, vec![0xFFu8]);
}

#[test]
fn mulaw_encode_saturates_above_clip_threshold() {
    // PCM16 magnitudes > 32 635 saturate to the loudest representable
    // class. INT16_MAX = 32 767 > 32 635 → must saturate.
    let high = mulaw_encode(&[i16::MAX]);
    let clipped = mulaw_encode(&[MULAW_CLIP]);
    assert_eq!(
        high, clipped,
        "encoding INT16_MAX must saturate to encoding MULAW_CLIP",
    );
}

#[test]
fn mulaw_encode_saturates_below_neg_clip_threshold() {
    // Symmetric: most-negative PCM16 saturates to most-negative G.711.
    let low = mulaw_encode(&[i16::MIN]);
    let clipped_neg = mulaw_encode(&[-MULAW_CLIP]);
    assert_eq!(low, clipped_neg);
}

#[test]
fn mulaw_encode_empty_input_returns_empty() {
    assert!(mulaw_encode(&[]).is_empty());
}

#[test]
fn mulaw_encode_preserves_length() {
    for len in [0usize, 1, 7, 64, 1000, 8192] {
        let input: Vec<i16> = (0..len).map(|i| (i as i16).wrapping_mul(31)).collect();
        let out = mulaw_encode(&input);
        assert_eq!(out.len(), len);
    }
}

#[test]
fn mulaw_encode_drift_gate_byte_identical_to_rust_ref_on_canonical_grid() {
    // Step over the i16 domain with a stride that gives full coverage
    // of every (sign, exponent, mantissa) class without paying the
    // 65 536-iteration round-trip.
    let inputs: Vec<i16> = (i16::MIN..=i16::MAX).step_by(7).collect();
    let c_out = mulaw_encode(&inputs);
    let ref_out: Vec<u8> = inputs.iter().map(|&s| ref_mulaw_encode_sample(s)).collect();
    assert_eq!(
        c_out, ref_out,
        "C μ-law encode diverged from Rust reference (stride-7 sweep over i16 domain)",
    );
}

// ════════════════════════════════════════════════════════════════════════
// μ-law round-trip + categorical properties
// ════════════════════════════════════════════════════════════════════════

#[test]
fn mulaw_pcm_roundtrip_quantisation_error_is_bounded() {
    // μ-law is lossy by design; quantisation error stays within a few
    // percent of magnitude. Mirrors the Rust reference test.
    for pcm in (-30_000..=30_000).step_by(512) {
        let byte = mulaw_encode(&[pcm])[0];
        let recovered = mulaw_decode(&[byte])[0];
        let error = (pcm - recovered).abs();
        let tolerance = (pcm.abs() / 10).max(256);
        assert!(
            error <= tolerance,
            "pcm={pcm} → byte=0x{byte:02X} → {recovered} (err={error}, tol={tolerance})",
        );
    }
}

#[test]
fn mulaw_decode_then_encode_collapses_signed_zero_only() {
    // G.711 has two byte representations of zero:
    //   stored 0xFF (logical 0x00) → +0  [canonical coset representative]
    //   stored 0x7F (logical 0x80) → -0  [the "negative-zero" sentinel]
    // Both decode to integer 0, which then encodes back to 0xFF (the
    // positive coset representative). So decode∘encode is idempotent
    // EVERYWHERE EXCEPT 0x7F, which collapses to 0xFF. This is a
    // categorical property of the μ-law quantisation map: the only
    // non-trivial coset-collapse class lives at zero.
    //
    // Asserting the property explicitly documents that the C kernel
    // matches the mathematical structure of G.711 — not a bug, a fact.
    let all_bytes: Vec<u8> = (0u8..=255).collect();
    let decoded = mulaw_decode(&all_bytes);
    let re_encoded = mulaw_encode(&decoded);
    for (i, (orig, recovered)) in all_bytes.iter().zip(re_encoded.iter()).enumerate() {
        let expected = if *orig == 0x7F { 0xFF } else { *orig };
        assert_eq!(
            *recovered, expected,
            "decode∘encode at byte 0x{orig:02X} (idx {i}): got 0x{recovered:02X}, expected 0x{expected:02X}",
        );
    }
}

#[test]
fn mulaw_encoding_is_sign_symmetric() {
    // For non-zero PCM samples in the unsaturated range, encode(-x)
    // and encode(x) should agree on exponent + mantissa and only
    // differ in the sign bit. Test by pair-decoding and checking
    // magnitude equality.
    for pcm in (1..=20_000).step_by(173) {
        let pos_byte = mulaw_encode(&[pcm])[0];
        let neg_byte = mulaw_encode(&[-pcm])[0];
        let pos_decoded = mulaw_decode(&[pos_byte])[0];
        let neg_decoded = mulaw_decode(&[neg_byte])[0];
        assert_eq!(
            pos_decoded, -neg_decoded,
            "encoding asymmetry at pcm={pcm}: pos→{pos_decoded}, neg→{neg_decoded}",
        );
    }
}

// ════════════════════════════════════════════════════════════════════════
// Resample — output length contract
// ════════════════════════════════════════════════════════════════════════

#[test]
fn resample_output_len_zero_input_yields_zero() {
    let len = resample_linear_pcm16_output_len(0, 8_000, 16_000).unwrap();
    assert_eq!(len, 0);
}

#[test]
fn resample_output_len_identity_rate_returns_input_len() {
    let len = resample_linear_pcm16_output_len(1234, 16_000, 16_000).unwrap();
    assert_eq!(len, 1234);
}

#[test]
fn resample_output_len_upsample_8k_to_16k() {
    let len = resample_linear_pcm16_output_len(100, 8_000, 16_000).unwrap();
    assert_eq!(len, 200);
}

#[test]
fn resample_output_len_downsample_16k_to_8k() {
    let len = resample_linear_pcm16_output_len(200, 16_000, 8_000).unwrap();
    assert_eq!(len, 100);
}

#[test]
fn resample_output_len_triple_ratios() {
    assert_eq!(
        resample_linear_pcm16_output_len(1000, 16_000, 48_000).unwrap(),
        3_000
    );
    assert_eq!(
        resample_linear_pcm16_output_len(3000, 48_000, 16_000).unwrap(),
        1_000
    );
}

#[test]
fn resample_output_len_clamps_to_one_for_small_input() {
    // Single sample at 48k → 16k by integer math = 0; clamped to 1.
    let len = resample_linear_pcm16_output_len(1, 48_000, 16_000).unwrap();
    assert_eq!(len, 1);
}

#[test]
fn resample_output_len_rejects_zero_rate() {
    assert!(matches!(
        resample_linear_pcm16_output_len(100, 0, 16_000),
        Err(ResampleError::InvalidRate {
            from_hz: 0,
            to_hz: 16_000
        }),
    ));
    assert!(matches!(
        resample_linear_pcm16_output_len(100, 8_000, 0),
        Err(ResampleError::InvalidRate {
            from_hz: 8_000,
            to_hz: 0
        }),
    ));
}

// ════════════════════════════════════════════════════════════════════════
// Resample — value contract
// ════════════════════════════════════════════════════════════════════════

#[test]
fn resample_identity_returns_input() {
    let samples = vec![100i16, 200, 300, -100, -200];
    let result = resample_linear_pcm16(&samples, 16_000, 16_000).unwrap();
    assert_eq!(result, samples);
}

#[test]
fn resample_empty_input_returns_empty() {
    let result = resample_linear_pcm16(&[], 8_000, 16_000).unwrap();
    assert!(result.is_empty());
}

#[test]
fn resample_upsample_doubles_length() {
    let samples = vec![0i16; 100];
    let result = resample_linear_pcm16(&samples, 8_000, 16_000).unwrap();
    assert_eq!(result.len(), 200);
}

#[test]
fn resample_downsample_halves_length() {
    let samples = vec![0i16; 200];
    let result = resample_linear_pcm16(&samples, 16_000, 8_000).unwrap();
    assert_eq!(result.len(), 100);
}

#[test]
fn resample_constant_signal_is_preserved() {
    // A constant signal must remain constant after any rate conversion.
    let samples = vec![1234i16; 50];
    let upsampled = resample_linear_pcm16(&samples, 8_000, 16_000).unwrap();
    for (i, &sample) in upsampled.iter().enumerate() {
        assert_eq!(
            sample, 1234,
            "constant signal diverged at index {i}: got {sample}",
        );
    }
}

#[test]
fn resample_linear_ramp_interpolates_correctly() {
    // A ramp 0, 100, 200, 300 upsampled 1:2 should produce
    // approximately 0, 50, 100, 150, 200, 250, 300, 300 (last
    // sample clamped per kernel boundary semantics).
    let samples = vec![0i16, 100, 200, 300];
    let result = resample_linear_pcm16(&samples, 8_000, 16_000).unwrap();
    let expected: Vec<i16> = vec![0, 50, 100, 150, 200, 250, 300, 300];
    assert_eq!(result.len(), expected.len());
    for (i, (got, want)) in result.iter().zip(expected.iter()).enumerate() {
        let drift = (got - want).abs();
        assert!(
            drift <= 1,
            "ramp interp at index {i}: got {got}, expected {want} (drift {drift} > 1 LSB)",
        );
    }
}

#[test]
fn resample_rejects_zero_rate() {
    let samples = vec![0i16; 10];
    assert!(matches!(
        resample_linear_pcm16(&samples, 0, 16_000),
        Err(ResampleError::InvalidRate {
            from_hz: 0,
            to_hz: 16_000
        }),
    ));
    assert!(matches!(
        resample_linear_pcm16(&samples, 8_000, 0),
        Err(ResampleError::InvalidRate {
            from_hz: 8_000,
            to_hz: 0
        }),
    ));
}

// ════════════════════════════════════════════════════════════════════════
// Resample — drift gate vs Rust reference (≤1 LSB epsilon)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn resample_drift_gate_8k_to_16k_within_one_lsb() {
    // Synthetic sine sampled at 8 kHz; resample to 16 kHz; compare to
    // Rust reference within ≤1 LSB tolerance per founder D6.
    let samples: Vec<i16> = (0..200)
        .map(|i| {
            let phase = (i as f64) * 2.0 * std::f64::consts::PI / 50.0;
            (phase.sin() * 16_000.0).round() as i16
        })
        .collect();
    let c_out = resample_linear_pcm16(&samples, 8_000, 16_000).unwrap();
    let ref_out = ref_resample_linear(&samples, 8_000, 16_000);
    assert_eq!(c_out.len(), ref_out.len());
    for (i, (c, r)) in c_out.iter().zip(ref_out.iter()).enumerate() {
        let drift = (c - r).abs();
        assert!(
            drift <= 1,
            "drift at index {i}: c={c}, ref={r}, drift={drift} > 1 LSB",
        );
    }
}

#[test]
fn resample_drift_gate_16k_to_48k_within_one_lsb() {
    let samples: Vec<i16> = (0..160)
        .map(|i| ((i as i16).wrapping_mul(127)).wrapping_sub(8_000))
        .collect();
    let c_out = resample_linear_pcm16(&samples, 16_000, 48_000).unwrap();
    let ref_out = ref_resample_linear(&samples, 16_000, 48_000);
    assert_eq!(c_out.len(), ref_out.len());
    for (i, (c, r)) in c_out.iter().zip(ref_out.iter()).enumerate() {
        assert!(
            (c - r).abs() <= 1,
            "16k→48k drift at index {i}: c={c}, ref={r}",
        );
    }
}

#[test]
fn resample_drift_gate_48k_to_16k_within_one_lsb() {
    let samples: Vec<i16> = (0..480).map(|i| (i as i16) - 240).collect();
    let c_out = resample_linear_pcm16(&samples, 48_000, 16_000).unwrap();
    let ref_out = ref_resample_linear(&samples, 48_000, 16_000);
    assert_eq!(c_out.len(), ref_out.len());
    for (c, r) in c_out.iter().zip(ref_out.iter()) {
        assert!((c - r).abs() <= 1);
    }
}

// ════════════════════════════════════════════════════════════════════════
// Stress / categorical composition
// ════════════════════════════════════════════════════════════════════════

#[test]
fn pipeline_mulaw_decode_then_resample_then_encode_does_not_crash() {
    // A canonical OTS pipeline: μ-law-encoded telephony at 8 kHz →
    // PCM16 → resample to 16 kHz → re-encode to μ-law. Confirms the
    // morphism composition path is operational end-to-end. The output
    // must be: half the rate-conversion factor in length and round-
    // trippable through μ-law without panicking.
    let mulaw_input: Vec<u8> = (0u8..=255).cycle().take(800).collect();
    let pcm = mulaw_decode(&mulaw_input);
    let resampled = resample_linear_pcm16(&pcm, 8_000, 16_000).unwrap();
    let mulaw_output = mulaw_encode(&resampled);
    assert_eq!(
        mulaw_output.len(),
        1_600,
        "8 kHz → 16 kHz upsample doubles length"
    );
}

#[test]
fn large_input_does_not_overflow_or_crash() {
    // 100 KB of synthetic data through every kernel.
    let mulaw_in: Vec<u8> = (0..100_000).map(|i| (i % 256) as u8).collect();
    let pcm = mulaw_decode(&mulaw_in);
    assert_eq!(pcm.len(), 100_000);
    let resampled = resample_linear_pcm16(&pcm, 16_000, 8_000).unwrap();
    assert_eq!(resampled.len(), 50_000);
    let mulaw_out = mulaw_encode(&resampled);
    assert_eq!(mulaw_out.len(), 50_000);
}

#[test]
fn resample_concurrent_calls_match_serial() {
    use std::sync::Arc;
    use std::thread;

    let samples: Arc<Vec<i16>> = Arc::new((0..400).map(|i| (i as i16) * 17).collect());
    let serial_out = resample_linear_pcm16(&samples, 8_000, 16_000).unwrap();
    let serial_arc = Arc::new(serial_out);

    let mut handles = Vec::new();
    for _ in 0..4 {
        let samples = Arc::clone(&samples);
        let serial = Arc::clone(&serial_arc);
        handles.push(thread::spawn(move || {
            for _ in 0..50 {
                let parallel_out = resample_linear_pcm16(&samples, 8_000, 16_000).unwrap();
                assert_eq!(
                    *serial, parallel_out,
                    "concurrent resample diverged from serial"
                );
            }
        }));
    }
    for h in handles {
        h.join().expect("resample worker panicked");
    }
}

#[test]
fn resample_error_display_is_readable() {
    let err = ResampleError::InvalidRate {
        from_hz: 0,
        to_hz: 16_000,
    };
    let msg = format!("{err}");
    assert!(msg.contains("0"));
    assert!(msg.contains("16000"));
}
