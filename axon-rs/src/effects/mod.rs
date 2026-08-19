//! AXON Algebraic Effects Runtime — v1.17.0
//! ============================================
//!
//! Native Rust runtime for AXON algebraic effects (paper section 1-section 6,
//! `docs/algebraic_effects_streaming.md`). Implements Plotkin/Pretnar
//! handlers + one-shot delimited continuations as a deterministic FSM
//! interpreter that consumes the JSON IR emitted by the Python frontend
//! (v1.17.0).
//!
//! # Decisions materialised here
//!
//! * **D1** — Operation polymorphism. Operation type parameters survive
//!   into the IR and are honoured at perform-site dispatch (the runtime
//!   is type-erased — values are tagged dynamically; the typechecker
//!   already proved soundness statically).
//! * **D2** — One-shot continuations. Each `Resume` consumes the
//!   captured continuation exactly once. Multi-resume is rejected
//!   statically by the typechecker (D10) so the runtime can assume the
//!   property and skip clone/allocation in the hot path.
//! * **D3** — Delimited handler scope. The handler stack is per-flow,
//!   pushed on `HandleEntry`, popped on exit.
//! * **D6** — Rust-only runtime. Python frontend emits IR, this crate
//!   executes. There is no Python runtime.
//! * **D9** — Compile-time exhaustiveness. The runtime trusts the
//!   typechecker and panics on violations (those would be impossible
//!   in well-typed programs and indicate a bug in the compiler).
//! * **D11** — Effect row polymorphism. Open vs closed rows are a
//!   compile-time concept; at runtime every effect is dispatched by
//!   name regardless of how the row was inferred.
//! * **D12** — `Forward` propagates to the outer handler frame —
//!   `find_handler` walks the stack from the innermost frame outward,
//!   skipping the source frame for forward instructions.
//!
//! # Architecture
//!
//! ```text
//!  Python frontend               Rust runtime (this crate)
//!  ────────────────               ────────────────────────
//!  parser          \              ┌──────────────────────┐
//!  typechecker      \             │  effects::ir          │
//!  ir_generator (CPS) \──> JSON ──▶  Deserialize structs  │
//!                                  │  (mirror of Python    │
//!                                  │   IRPerform / etc.)   │
//!                                  ├──────────────────────┤
//!                                  │  effects::runtime     │
//!                                  │   • EffectRuntime     │
//!                                  │   • FSM dispatch loop │
//!                                  │   • handler stack     │
//!                                  │   • Value env         │
//!                                  └──────────────────────┘
//! ```
//!
//! The interpreter is direct-style — the Rust call stack mirrors the
//! handler stack, and a `Resume` is implemented by returning a value
//! from a recursive call. Captured continuations are *not* boxed
//! closures; they are just the remaining instruction list at the
//! perform site, which the dispatch loop walks through.
//!
//! This is the FSM canonical shape promised by paper section 5: the
//! `(flow_name, state_id)` coordinate the Python compiler emits is
//! exactly what the Rust runtime indexes by. A future v1.18.0 may
//! lower this further to native `jmp` instructions via codegen; the
//! interpreter shipped here is the operational ground truth.

#![allow(dead_code)]

pub mod ir;
pub mod runtime;
pub mod value;

#[cfg(test)]
mod tests;

pub use ir::{
    EffectIRError, IRAbort, IREffectDeclaration, IREffectOperation, IRForward, IRHandlerClause,
    IRHandlerFrame, IRPerform, IRResume, Instruction,
};
pub use runtime::{EffectRuntime, EffectRuntimeError, ExecutionResult};
pub use value::Value;
