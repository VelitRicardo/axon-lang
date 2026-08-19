//! v1.19.0 — Buffer pool benchmark vs `Vec::with_capacity` baseline.
//!
//! Measures acquire+release throughput of `axon_csys::BufferPool`
//! against the canonical "naive Rust" baseline of `Vec::with_capacity`
//! followed by drop. The pool's win comes from:
//!   • Cache-line-aligned slabs (no malloc → arena → free traffic).
//!   • Bitmap free-list with O(1) slot pick (vs malloc's
//!     class-bucketed best-fit).
//!   • Per-class hot pool (vs system allocator's global lock).
//!
//! Numbers from this benchmark are documented per-class in plan vivo
//! 25.j row; the absolute multiplier varies by allocator
//! (jemalloc/tcmalloc/glibc/mimalloc) but the pool consistently wins
//! ≥4× on the small + medium classes where its bitmap is in L1.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

fn bench_buffer_pool(c: &mut Criterion) {
    let pool = axon_csys::BufferPool::new(/* tenant_soft_limit */ 1 << 30, false);
    let cases: &[(&str, usize)] = &[
        ("256B-small", 256),
        ("4KiB-medium", 4096),
        ("64KiB-large", 65_536),
        ("1MiB-huge", 1_048_576),
    ];
    let mut group = c.benchmark_group("buffer-pool-acquire-release");
    for &(label, size) in cases {
        group.bench_with_input(BenchmarkId::new("axon-csys", label), &size, |b, &size| {
            b.iter(|| {
                let slab = pool.acquire(criterion::black_box(size));
                // Bind via `let _ =` so the unused-must-use lint stays
                // happy; Drop on iteration end releases the slab.
                let _ = criterion::black_box(slab);
            });
        });
        group.bench_with_input(
            BenchmarkId::new("vec-with-capacity", label),
            &size,
            |b, &size| {
                b.iter(|| {
                    let v: Vec<u8> = Vec::with_capacity(criterion::black_box(size));
                    criterion::black_box(v);
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_buffer_pool);
criterion_main!(benches);
