//! v1.19.0 — HMAC-SHA256 benchmark vs `hmac::Hmac<Sha256>`.
//!
//! Measures throughput of `axon_csys::hmac_sha256` against the
//! `hmac` crate parametric over `sha2::Sha256`. Both back onto the
//! same FIPS 198-1 construction; the variable is whether the C
//! kernel's no-allocator + zero-trait-dispatch path beats the Rust
//! `Mac` trait through-call. Documented in plan vivo 25.j row.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use hmac::{Hmac, Mac};
use sha2::Sha256;

type RefHmac = Hmac<Sha256>;

const SIZES: &[(&str, usize)] = &[("64B", 64), ("1KiB", 1024), ("64KiB", 65_536)];

const KEY: &[u8] = b"a 32-byte key matches axon-csys ";

fn fixed_pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(17)).collect()
}

fn bench_hmac(c: &mut Criterion) {
    let mut group = c.benchmark_group("hmac");
    for &(label, len) in SIZES {
        let data = fixed_pattern(len);
        group.throughput(Throughput::Bytes(len as u64));
        group.bench_with_input(BenchmarkId::new("axon-csys", label), &data, |b, data| {
            b.iter(|| {
                axon_csys::hmac_sha256(criterion::black_box(KEY), criterion::black_box(data))
            });
        });
        group.bench_with_input(BenchmarkId::new("hmac-crate", label), &data, |b, data| {
            b.iter(|| {
                let mut mac = RefHmac::new_from_slice(criterion::black_box(KEY)).unwrap();
                mac.update(criterion::black_box(data));
                let _: [u8; 32] = mac.finalize().into_bytes().into();
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_hmac);
criterion_main!(benches);
