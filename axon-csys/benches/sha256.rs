//! v1.19.0 — SHA-256 benchmark vs `sha2` crate.
//!
//! Measures throughput of `axon_csys::sha256` against `sha2::Sha256`
//! on inputs of 64 B, 1 KiB, 64 KiB, 1 MiB. The expected outcome on
//! mainstream optimising compilers is parity (±10 %) — both impls
//! compile to the same canonical FIPS 180-4 transform; the C kernel's
//! win, when present, comes from removing the `sha2` crate's generic
//! `Digest` dispatch overhead at small input sizes. We document
//! measured numbers in `docs/cycle/silicon_cognition.md` 25.j row.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use sha2::Digest;

const SIZES: &[(&str, usize)] = &[
    ("64B", 64),
    ("1KiB", 1024),
    ("64KiB", 65_536),
    ("1MiB", 1_048_576),
];

fn fixed_pattern(len: usize) -> Vec<u8> {
    // Pattern bytes (not zeros) so optimisers can't elide the work.
    (0..len).map(|i| (i as u8).wrapping_mul(31)).collect()
}

fn bench_sha256(c: &mut Criterion) {
    let mut group = c.benchmark_group("sha256");
    for &(label, len) in SIZES {
        let data = fixed_pattern(len);
        group.throughput(Throughput::Bytes(len as u64));
        group.bench_with_input(BenchmarkId::new("axon-csys", label), &data, |b, data| {
            b.iter(|| axon_csys::sha256(criterion::black_box(data)));
        });
        group.bench_with_input(BenchmarkId::new("sha2-crate", label), &data, |b, data| {
            b.iter(|| {
                let _: [u8; 32] = sha2::Sha256::digest(criterion::black_box(data)).into();
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_sha256);
criterion_main!(benches);
