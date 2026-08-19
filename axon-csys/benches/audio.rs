//! v1.19.0 — Audio kernel benchmark.
//!
//! Measures throughput of:
//!   • `axon_csys::mulaw_decode` (μ-law → PCM16)
//!   • `axon_csys::mulaw_encode` (PCM16 → μ-law)
//!   • `axon_csys::resample_linear_pcm16` (8 kHz → 16 kHz, 16 → 8)
//!
//! No widely-used Rust competitor exists for these — the OTS reference
//! impl in `axon-rs` is the source the C kernel was ported from. Since
//! the byte-identical drift gate (tests/audio.rs + tests/drift_gate.rs)
//! already proves output parity with the Rust reference, the benchmark
//! reports absolute throughput (samples/sec, MiB/sec) and a target
//! comparison against the per-D10 ratified ≥3× audio threshold (the
//! threshold is documented per kernel in the plan vivo).
//!
//! On x86_64 with a modern compiler, μ-law decode hits ~1 GB/s
//! (cache-bound, single-threaded) and resample 8→16 kHz hits ~250 M
//! samples/sec. Numbers in plan vivo 25.j row.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

const SAMPLE_COUNTS: &[(&str, usize)] = &[
    ("80smp", 80),        // 10 ms @ 8 kHz frame
    ("160smp", 160),      // 20 ms @ 8 kHz frame
    ("8000smp", 8000),    // 1 s @ 8 kHz
    ("48000smp", 48_000), // 1 s @ 48 kHz
];

fn pcm_pattern(samples: usize) -> Vec<i16> {
    (0..samples)
        .map(|i| (i as i32 * 37 - 16384) as i16)
        .collect()
}

fn mulaw_pattern(samples: usize) -> Vec<u8> {
    (0..samples).map(|i| (i as u8).wrapping_mul(13)).collect()
}

fn bench_mulaw(c: &mut Criterion) {
    let mut group = c.benchmark_group("mulaw-decode");
    for &(label, samples) in SAMPLE_COUNTS {
        let input = mulaw_pattern(samples);
        group.throughput(Throughput::Bytes(samples as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &input, |b, input| {
            b.iter(|| axon_csys::mulaw_decode(criterion::black_box(input)));
        });
    }
    group.finish();

    let mut group = c.benchmark_group("mulaw-encode");
    for &(label, samples) in SAMPLE_COUNTS {
        let input = pcm_pattern(samples);
        // Throughput in bytes-of-i16 input.
        group.throughput(Throughput::Bytes((samples * 2) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &input, |b, input| {
            b.iter(|| axon_csys::mulaw_encode(criterion::black_box(input)));
        });
    }
    group.finish();
}

fn bench_resample(c: &mut Criterion) {
    let cases: &[(&str, u32, u32, usize)] = &[
        ("8k-to-16k-160smp", 8_000, 16_000, 160),
        ("16k-to-8k-160smp", 16_000, 8_000, 160),
        ("8k-to-48k-1000smp", 8_000, 48_000, 1_000),
        ("48k-to-16k-1000smp", 48_000, 16_000, 1_000),
    ];
    let mut group = c.benchmark_group("resample");
    for &(label, in_hz, out_hz, samples) in cases {
        let input = pcm_pattern(samples);
        group.throughput(Throughput::Bytes((samples * 2) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &input, |b, input| {
            b.iter(|| {
                axon_csys::resample_linear_pcm16(criterion::black_box(input), in_hz, out_hz)
                    .expect("resample")
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_mulaw, bench_resample);
criterion_main!(benches);
