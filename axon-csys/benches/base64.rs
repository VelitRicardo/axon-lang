//! v1.19.0 — base64url-no-pad benchmark vs `base64::URL_SAFE_NO_PAD`.
//!
//! Measures encode + decode throughput on 64 B, 1 KiB, 64 KiB inputs.
//! Documented in plan vivo 25.j row.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

const SIZES: &[(&str, usize)] = &[("64B", 64), ("1KiB", 1024), ("64KiB", 65_536)];

fn fixed_pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(7)).collect()
}

fn bench_b64url(c: &mut Criterion) {
    let mut group = c.benchmark_group("b64url-encode");
    for &(label, len) in SIZES {
        let data = fixed_pattern(len);
        group.throughput(Throughput::Bytes(len as u64));
        group.bench_with_input(BenchmarkId::new("axon-csys", label), &data, |b, data| {
            b.iter(|| axon_csys::b64url_encode(criterion::black_box(data)));
        });
        group.bench_with_input(BenchmarkId::new("base64-crate", label), &data, |b, data| {
            b.iter(|| URL_SAFE_NO_PAD.encode(criterion::black_box(data)));
        });
    }
    group.finish();

    let mut group = c.benchmark_group("b64url-decode");
    for &(label, len) in SIZES {
        let data = fixed_pattern(len);
        let encoded = URL_SAFE_NO_PAD.encode(&data);
        group.throughput(Throughput::Bytes(encoded.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("axon-csys", label),
            &encoded,
            |b, encoded| {
                b.iter(|| axon_csys::b64url_decode(criterion::black_box(encoded)));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("base64-crate", label),
            &encoded,
            |b, encoded| {
                b.iter(|| URL_SAFE_NO_PAD.decode(criterion::black_box(encoded)));
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_b64url);
criterion_main!(benches);
