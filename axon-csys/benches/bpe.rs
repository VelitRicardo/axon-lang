//! v1.19.0 — BPE encoder benchmark vs `tiktoken-rs`.
//!
//! Measures encode throughput of `axon_csys::cl100k_base().encode_ordinary`
//! against `tiktoken_rs::cl100k_base().encode_ordinary` on inputs of
//! 100 chars, 1 KiB, 10 KiB. The expected result is parity (±20 %)
//! since both impls share the canonical tiktoken `byte_pair_merge`
//! algorithm + identical merge tables; the C kernel's win, if any,
//! comes from the no-allocator hot path. Documented in plan vivo
//! 25.j row.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

const PARAGRAPH: &str = "The Lorem ipsum text is a placeholder used by designers, \
    printers, and typesetters since the 1500s. Its origin lies in a scrambled passage \
    from Cicero's De finibus bonorum et malorum, a treatise on the theory of ethics \
    widely studied during the Renaissance. The garbled Latin reads correctly on first \
    glance but conveys no meaning; this is precisely why it was selected as filler — \
    readers focus on layout rather than content. ";

fn input_of(approx_bytes: usize) -> String {
    let mut s = String::with_capacity(approx_bytes);
    while s.len() < approx_bytes {
        s.push_str(PARAGRAPH);
    }
    s.truncate(approx_bytes);
    // Realign to a UTF-8 boundary to avoid panic-on-encode for the
    // truncated em-dash inside PARAGRAPH.
    while !s.is_char_boundary(s.len()) {
        s.pop();
    }
    s
}

fn bench_bpe(c: &mut Criterion) {
    let csys = axon_csys::cl100k_base().expect("cl100k load");
    let reference = tiktoken_rs::cl100k_base().expect("tiktoken-rs cl100k");
    let mut group = c.benchmark_group("bpe-cl100k-encode");
    for &(label, len) in &[("100B", 100usize), ("1KiB", 1024), ("10KiB", 10 * 1024)] {
        let text = input_of(len);
        group.throughput(Throughput::Bytes(text.len() as u64));
        group.bench_with_input(BenchmarkId::new("axon-csys", label), &text, |b, text| {
            b.iter(|| {
                csys.encode_ordinary(criterion::black_box(text))
                    .expect("axon-csys encode")
            });
        });
        group.bench_with_input(BenchmarkId::new("tiktoken-rs", label), &text, |b, text| {
            b.iter(|| reference.encode_ordinary(criterion::black_box(text)));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_bpe);
criterion_main!(benches);
