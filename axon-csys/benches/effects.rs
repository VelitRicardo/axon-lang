//! v1.19.0 — Algebraic-effects FSM dispatcher benchmark.
//!
//! Characterises the C23 dispatcher's per-opcode throughput on
//! representative wire shapes:
//!   • empty wire        — pure entry/exit overhead
//!   • single resume     — one perform → handle → resume cycle
//!   • bulk resume       — 10× and 100× cycles in a tight loop
//!
//! No widely-used Rust competitor exists for the algebraic-effects
//! dispatcher specifically — the original Rust tree-walker lives in
//! `axon-rs/src/effects/runtime.rs`, which we can't depend on from
//! axon-csys (would create a circular dep). The "≥10× faster than
//! tree-walker" target ratified in D10 is established by the original
//! v1.17.0 paper section 5 work; this benchmark documents the C kernel's
//! absolute throughput so adopters can compute their own ratio
//! against any baseline they care to measure.
//!
//! Numbers measured locally on Windows MSVC + Intel are documented
//! per-shape in plan vivo 25.j row.

use axon_csys::effects::{
    BuiltWire, Clause, Dispatcher, EffectDecl, Frame, Instruction, Opcode, Value, WireBuilder,
};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

fn declare_effect(name: &str, ops: &[(&str, u32)]) -> EffectDecl {
    EffectDecl {
        name: name.to_string(),
        operation_names: ops.iter().map(|(n, _)| (*n).to_string()).collect(),
        operation_arities: ops.iter().map(|(_, a)| *a).collect(),
    }
}

/// Build a wire with `n` perform→resume cycles inside one handler frame.
/// Each iteration emits a fresh `perform Foo.bar()` whose clause body
/// is a single `resume(Int(42))`. Total opcode count: 2 (top-level
/// HandlerFrame + frame body of n Performs) + n clauses (each just
/// Resume) ≈ 2n + 2 instructions.
fn build_resume_loop_wire(n: u32) -> BuiltWire {
    let mut b = WireBuilder::new();
    b.add_effect(declare_effect("Foo", &[("bar", 0)]));

    // arg pool[0] = the resume value
    let (resume_val_offset, _) = b.add_args([Value::Int(42)]);

    // Clause body — one Resume instruction, shared by all clauses.
    let clause_body_offset = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Resume,
        effect_id: 0,
        operation_id: 0,
        args_count: 1,
        args_offset: resume_val_offset,
        state_id: 0,
        frame_id: 1,
    });
    let clause_body_count = b.instructions_len() - clause_body_offset;

    // Frame body — n Perform instructions.
    let body_offset = b.instructions_len();
    for _ in 0..n {
        b.add_instruction(Instruction {
            opcode: Opcode::Perform,
            effect_id: 0,
            operation_id: 0,
            args_count: 0,
            args_offset: 0,
            state_id: 100,
            frame_id: 0,
        });
    }
    let body_count = b.instructions_len() - body_offset;

    let clauses = vec![Clause {
        effect_id: 0,
        operation_id: 0,
        parameter_count: 0,
        parameter_names_offset: 0,
        body_offset: clause_body_offset,
        body_count: clause_body_count,
        operation_name: "bar".to_string(),
    }];
    let frame_id = b.add_frame(Frame {
        effect_ids: vec![0],
        body_offset,
        body_count,
        clauses,
        frame_id: 1,
    });
    b.add_top_level_instruction(Instruction {
        opcode: Opcode::HandlerFrame,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: frame_id,
        state_id: 0,
        frame_id: 1,
    });
    b.build()
}

fn bench_effects(c: &mut Criterion) {
    let mut group = c.benchmark_group("effects-dispatch");

    // 1. Empty wire — pure dispatcher overhead (entry + exit).
    let empty = WireBuilder::new().build();
    group.bench_function("empty-wire", |b| {
        b.iter(|| {
            let (_result, _trace) = Dispatcher::run(criterion::black_box(&empty), &[], None);
        });
    });

    // 2. Increasing perform/resume counts.
    for &n in &[1u32, 10, 100, 1000] {
        let wire = build_resume_loop_wire(n);
        // Throughput in opcodes (each iteration dispatches ~2n+2 opcodes).
        group.throughput(Throughput::Elements((2 * n + 2) as u64));
        group.bench_with_input(
            BenchmarkId::new("perform-resume-cycles", n),
            &wire,
            |b, wire| {
                b.iter(|| {
                    let (_result, _trace) = Dispatcher::run(criterion::black_box(wire), &[], None);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_effects);
criterion_main!(benches);
