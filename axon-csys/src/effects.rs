//! v1.19.0 — Algebraic effects FSM dispatcher (Rust shim).
//!
//! Safe Rust wrapper around the C23 dispatcher in
//! `c-src/effects/dispatch.c`. The boundary follows the founder
//! pillar split:
//!
//!   - C side: computed-goto opcode dispatch (gcc/clang) or switch
//!     fallback (MSVC); explicit exec / handler stacks (no C
//!     recursion); pre-resolved indices on the wire format.
//!   - Rust side: type-safe wire builder, lifetime-bounded slice
//!     borrows, [`Value`] conversion to / from the C tagged union,
//!     [`DispatchError`] surfacing of the runtime's defensive
//!     error codes.
//!
//! Mathematical pillar (preserved from the Rust reference impl):
//!   - One-shot continuations (D2): each clause discharges exactly
//!     once via resume / abort / forward.
//!   - Forward semantics: searches outward from the source frame's
//!     stack index, bypassing the source AND any frames nested
//!     beneath it.
//!   - No-coercion: handlers observe + decide; the dispatcher never
//!     forces a value beyond what the IR specifies.
//!
//! The dispatcher operates on a wire format with pre-resolved
//! integer indices for effects, operations, and frames. The Rust
//! shim's [`WireBuilder`] turns the IR-level string-named structures
//! into the index-keyed wire layout the C dispatcher walks.

use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr;

// ──────────────────────────────────────────────────────────────────────
// Raw FFI types — must match dispatch.h byte-for-byte.
// ──────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct RawStrSlice {
    ptr: *const u8,
    len: usize,
}

unsafe impl Send for RawStrSlice {}
unsafe impl Sync for RawStrSlice {}

#[repr(C)]
#[derive(Clone, Copy)]
union RawValuePayload {
    b: bool,
    i: i64,
    f: f64,
    s: RawStrSlice,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RawValue {
    tag: u32,
    payload: RawValuePayload,
}

const TAG_UNIT: u32 = 0;
const TAG_BOOL: u32 = 1;
const TAG_INT: u32 = 2;
const TAG_FLOAT: u32 = 3;
const TAG_STRING: u32 = 4;
const TAG_SYMBOL: u32 = 5;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct RawInstruction {
    opcode: u8,
    _pad: [u8; 3],
    effect_id: u32,
    operation_id: u32,
    args_count: u32,
    args_offset: u32,
    state_id: u32,
    frame_id: u32,
}

#[repr(C)]
struct RawFrame {
    effect_ids: *const u32,
    effect_count: u32,
    body_offset: u32,
    body_count: u32,
    clauses: *const RawClause,
    clause_count: u32,
    frame_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RawClause {
    effect_id: u32,
    operation_id: u32,
    parameter_count: u32,
    parameter_names_offset: u32,
    body_offset: u32,
    body_count: u32,
    operation_name: RawStrSlice,
}

#[repr(C)]
struct RawEffectDecl {
    name: RawStrSlice,
    operation_names: *const RawStrSlice,
    operation_arities: *const u32,
    operation_count: u32,
}

#[repr(C)]
struct RawWire {
    instructions: *const RawInstruction,
    instruction_count: u32,
    frames: *const RawFrame,
    frame_count: u32,
    effects: *const RawEffectDecl,
    effect_count: u32,
    arg_pool: *const RawValue,
    arg_pool_size: u32,
    parameter_name_pool: *const RawStrSlice,
    parameter_name_pool_size: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RawDispatchResult {
    kind: u32,
    value: RawValue,
    error_code: u32,
    error_effect_id: u32,
    error_operation_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RawTraceEvent {
    kind: u32,
    frame_id: u32,
    effect_id: u32,
    operation_id: u32,
    value: RawValue,
}

const RESULT_COMPLETED: u32 = 0;
const RESULT_ABORTED: u32 = 1;
const RESULT_ERROR: u32 = 2;

extern "C" {
    fn axon_csys_effects_run(
        wire: *const RawWire,
        globals_keys: *const RawStrSlice,
        globals_values: *const RawValue,
        globals_count: u32,
        trace_buffer: *mut RawTraceEvent,
        trace_capacity: u32,
        trace_count_out: *mut u32,
    ) -> RawDispatchResult;
    fn axon_csys_effects_uses_computed_gotos() -> bool;
}

// ──────────────────────────────────────────────────────────────────────
// Rust-facing safe types
// ──────────────────────────────────────────────────────────────────────

/// Runtime value flowing through the dispatcher.
///
/// Mirrors the supported subset of `axon_rs::effects::value::Value`.
/// `List` and `Map` (heap-managed) are intentionally absent — the
/// boundary keeps the C dispatcher zero-allocation; flows that
/// require List / Map values stay on the Rust dispatcher path.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Unit,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Symbol(String),
}

impl Value {
    fn to_raw(&self) -> RawValue {
        match self {
            Value::Unit => RawValue {
                tag: TAG_UNIT,
                payload: RawValuePayload { i: 0 },
            },
            Value::Bool(b) => RawValue {
                tag: TAG_BOOL,
                payload: RawValuePayload { b: *b },
            },
            Value::Int(i) => RawValue {
                tag: TAG_INT,
                payload: RawValuePayload { i: *i },
            },
            Value::Float(f) => RawValue {
                tag: TAG_FLOAT,
                payload: RawValuePayload { f: *f },
            },
            Value::String(s) => RawValue {
                tag: TAG_STRING,
                payload: RawValuePayload {
                    s: RawStrSlice {
                        ptr: s.as_ptr(),
                        len: s.len(),
                    },
                },
            },
            Value::Symbol(s) => RawValue {
                tag: TAG_SYMBOL,
                payload: RawValuePayload {
                    s: RawStrSlice {
                        ptr: s.as_ptr(),
                        len: s.len(),
                    },
                },
            },
        }
    }

    fn from_raw(raw: RawValue) -> Self {
        // SAFETY: tag uniquely determines which payload variant is
        // initialised. `RawStrSlice` carries borrowed pointers from
        // the wire's lifetime; we copy the bytes here so the returned
        // Value owns its data.
        unsafe {
            match raw.tag {
                TAG_UNIT => Value::Unit,
                TAG_BOOL => Value::Bool(raw.payload.b),
                TAG_INT => Value::Int(raw.payload.i),
                TAG_FLOAT => Value::Float(raw.payload.f),
                TAG_STRING => {
                    let s = raw.payload.s;
                    if s.ptr.is_null() || s.len == 0 {
                        Value::String(String::new())
                    } else {
                        let bytes = std::slice::from_raw_parts(s.ptr, s.len);
                        Value::String(String::from_utf8_lossy(bytes).into_owned())
                    }
                }
                TAG_SYMBOL => {
                    let s = raw.payload.s;
                    if s.ptr.is_null() || s.len == 0 {
                        Value::Symbol(String::new())
                    } else {
                        let bytes = std::slice::from_raw_parts(s.ptr, s.len);
                        Value::Symbol(String::from_utf8_lossy(bytes).into_owned())
                    }
                }
                _ => Value::Unit,
            }
        }
    }
}

/// Opcode set — kept synchronised with the C `AxonCsysOpcode` enum
/// via the `as u8` cast in [`Instruction::to_raw`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    Passthrough = 0,
    Perform = 1,
    HandlerFrame = 2,
    Resume = 3,
    Abort = 4,
    Forward = 5,
}

/// One instruction in the flattened block.
#[derive(Debug, Clone)]
pub struct Instruction {
    pub opcode: Opcode,
    pub effect_id: u32,
    pub operation_id: u32,
    pub args_count: u32,
    pub args_offset: u32,
    pub state_id: u32,
    pub frame_id: u32,
}

impl Instruction {
    fn to_raw(&self) -> RawInstruction {
        RawInstruction {
            opcode: self.opcode as u8,
            _pad: [0; 3],
            effect_id: self.effect_id,
            operation_id: self.operation_id,
            args_count: self.args_count,
            args_offset: self.args_offset,
            state_id: self.state_id,
            frame_id: self.frame_id,
        }
    }
}

/// Pre-resolved handler clause (operation_id within an effect).
#[derive(Debug, Clone)]
pub struct Clause {
    pub effect_id: u32,
    pub operation_id: u32,
    pub parameter_count: u32,
    pub parameter_names_offset: u32,
    pub body_offset: u32,
    pub body_count: u32,
    pub operation_name: String,
}

/// Pre-resolved handler frame metadata.
#[derive(Debug, Clone)]
pub struct Frame {
    pub effect_ids: Vec<u32>,
    pub body_offset: u32,
    pub body_count: u32,
    pub clauses: Vec<Clause>,
    pub frame_id: u32,
}

/// Effect declaration metadata (used by the dispatcher for arity
/// validation; mostly informational at runtime).
#[derive(Debug, Clone)]
pub struct EffectDecl {
    pub name: String,
    pub operation_names: Vec<String>,
    pub operation_arities: Vec<u32>,
}

// ──────────────────────────────────────────────────────────────────────
// WireBuilder — owned, type-safe builder for the wire format.
// ──────────────────────────────────────────────────────────────────────

/// Owned representation of the wire-format input to the dispatcher.
///
/// Construct via [`WireBuilder::new`] + the various `add_*` methods,
/// then call [`WireBuilder::build`] to produce a [`BuiltWire`] that
/// can be passed to [`Dispatcher::run`].
///
/// Two distinct instruction pools live here:
///   - **body pool**: frame + clause bodies, accessed by offset/count.
///     Use [`Self::add_instruction`] to append; the returned u32 is
///     the body-pool offset.
///   - **top-level block**: the instructions the dispatcher walks
///     first (typically a single HandlerFrame). Use
///     [`Self::add_top_level_instruction`] to append.
///
/// At [`Self::build`] time the two pools are concatenated as
/// `[top-level | body]` and frame/clause body offsets are shifted to
/// account for the top-level block sitting at offsets `0..top_count`.
#[derive(Debug, Default)]
pub struct WireBuilder {
    /// Body-pool instructions (frame bodies + clause bodies, indexed
    /// by offset). Final layout shifts these past the top-level block.
    body_instructions: Vec<Instruction>,
    /// Top-level block — the dispatcher's run() entry. Sits at offsets
    /// `0..top_level_instructions.len()` in the final layout.
    top_level_instructions: Vec<Instruction>,
    frames: Vec<Frame>,
    effects: Vec<EffectDecl>,
    arg_pool: Vec<Value>,
    parameter_name_pool: Vec<String>,
}

impl WireBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_effect(&mut self, effect: EffectDecl) -> u32 {
        let id = self.effects.len() as u32;
        self.effects.push(effect);
        id
    }

    pub fn add_frame(&mut self, frame: Frame) -> u32 {
        let id = self.frames.len() as u32;
        self.frames.push(frame);
        id
    }

    /// Append a body-pool instruction (frame body or clause body
    /// content). Returns the body-pool offset, which the caller
    /// stores in the parent [`Frame::body_offset`] /
    /// [`Clause::body_offset`] field.
    pub fn add_instruction(&mut self, instruction: Instruction) -> u32 {
        let id = self.body_instructions.len() as u32;
        self.body_instructions.push(instruction);
        id
    }

    /// Append a top-level instruction — runs first in the dispatcher.
    /// Most flows have a single top-level HandlerFrame.
    pub fn add_top_level_instruction(&mut self, instruction: Instruction) -> u32 {
        let id = self.top_level_instructions.len() as u32;
        self.top_level_instructions.push(instruction);
        id
    }

    /// Current size of the body pool — handy for capturing
    /// body_offset before appending instructions.
    pub fn instructions_len(&self) -> u32 {
        self.body_instructions.len() as u32
    }

    /// Append `args` to the arg pool; returns (offset, count).
    pub fn add_args(&mut self, args: impl IntoIterator<Item = Value>) -> (u32, u32) {
        let offset = self.arg_pool.len() as u32;
        let mut count = 0u32;
        for a in args {
            self.arg_pool.push(a);
            count += 1;
        }
        (offset, count)
    }

    /// Append `names` to the parameter-name pool; returns offset.
    pub fn add_parameter_names(&mut self, names: impl IntoIterator<Item = String>) -> u32 {
        let offset = self.parameter_name_pool.len() as u32;
        self.parameter_name_pool.extend(names);
        offset
    }

    /// Finalise into a [`BuiltWire`] that owns all backing storage.
    pub fn build(self) -> BuiltWire {
        BuiltWire::new(self)
    }
}

/// Owned wire data + the auxiliary index tables the C dispatcher reads
/// (effect_ids per frame, clauses per frame, operation_names + arities
/// per effect, raw value/string forms of the arg pool + parameter names).
///
/// Keep alive for the entire duration of any [`Dispatcher::run`] call.
pub struct BuiltWire {
    /// Concatenated `[top-level | body]` instruction array. The
    /// dispatcher walks the first `top_level_count` entries as the
    /// top-level block; frame + clause body offsets index into the
    /// remaining region.
    instructions: Vec<RawInstruction>,
    /// Number of leading entries in `instructions` that form the
    /// top-level block.
    top_level_count: u32,
    raw_frames: Vec<RawFrame>,
    raw_effects: Vec<RawEffectDecl>,
    raw_arg_pool: Vec<RawValue>,
    raw_parameter_names: Vec<RawStrSlice>,

    // Backing storage for borrowed pointers — these MUST NOT move
    // after construction; we hand stable pointers into them.
    _frames_owned: Vec<Frame>,
    _effects_owned: Vec<EffectDecl>,
    _args_owned: Vec<Value>,
    _parameter_names_owned: Vec<String>,
    _frame_effect_ids: Vec<Vec<u32>>,
    _frame_clauses: Vec<Vec<RawClause>>,
    _effect_operation_names: Vec<Vec<RawStrSlice>>,
    _effect_operation_arities: Vec<Vec<u32>>,
}

// SAFETY: BuiltWire is read-only after construction. The raw pointers
// it carries point into its own owned Vecs which never move (the
// builder consumes the input + produces a finalised value). Multiple
// threads can call `Dispatcher::run(&wire, ...)` concurrently because
// the C dispatcher does not mutate the wire — it only reads.
unsafe impl Send for BuiltWire {}
unsafe impl Sync for BuiltWire {}

impl BuiltWire {
    fn new(builder: WireBuilder) -> Self {
        let WireBuilder {
            body_instructions,
            top_level_instructions,
            frames,
            effects,
            arg_pool,
            parameter_name_pool,
        } = builder;

        // Concatenate as [top-level | body]. Body offsets need to be
        // shifted by the top-level block's length so they land at
        // their final positions in the wire array.
        let top_level_count = top_level_instructions.len() as u32;
        let mut all_instructions: Vec<Instruction> =
            Vec::with_capacity(top_level_instructions.len() + body_instructions.len());
        all_instructions.extend(top_level_instructions);
        all_instructions.extend(body_instructions);

        let raw_instructions: Vec<RawInstruction> =
            all_instructions.iter().map(Instruction::to_raw).collect();

        // Build per-frame effect_ids + clause arrays. Body offsets
        // captured by the caller refer to the body pool; we shift them
        // by top_level_count to hit the final wire layout.
        let mut frame_effect_ids: Vec<Vec<u32>> = Vec::with_capacity(frames.len());
        let mut frame_clauses: Vec<Vec<RawClause>> = Vec::with_capacity(frames.len());
        for frame in &frames {
            frame_effect_ids.push(frame.effect_ids.clone());
            let raw_clauses: Vec<RawClause> = frame
                .clauses
                .iter()
                .map(|c| RawClause {
                    effect_id: c.effect_id,
                    operation_id: c.operation_id,
                    parameter_count: c.parameter_count,
                    parameter_names_offset: c.parameter_names_offset,
                    body_offset: c.body_offset + top_level_count,
                    body_count: c.body_count,
                    operation_name: RawStrSlice {
                        ptr: c.operation_name.as_ptr(),
                        len: c.operation_name.len(),
                    },
                })
                .collect();
            frame_clauses.push(raw_clauses);
        }

        let raw_frames: Vec<RawFrame> = frames
            .iter()
            .enumerate()
            .map(|(i, frame)| RawFrame {
                effect_ids: frame_effect_ids[i].as_ptr(),
                effect_count: frame_effect_ids[i].len() as u32,
                body_offset: frame.body_offset + top_level_count,
                body_count: frame.body_count,
                clauses: frame_clauses[i].as_ptr(),
                clause_count: frame_clauses[i].len() as u32,
                frame_id: frame.frame_id,
            })
            .collect();

        // Per-effect operation names + arities.
        let mut effect_operation_names: Vec<Vec<RawStrSlice>> = Vec::with_capacity(effects.len());
        let mut effect_operation_arities: Vec<Vec<u32>> = Vec::with_capacity(effects.len());
        for effect in &effects {
            let names: Vec<RawStrSlice> = effect
                .operation_names
                .iter()
                .map(|n| RawStrSlice {
                    ptr: n.as_ptr(),
                    len: n.len(),
                })
                .collect();
            effect_operation_names.push(names);
            effect_operation_arities.push(effect.operation_arities.clone());
        }

        let raw_effects: Vec<RawEffectDecl> = effects
            .iter()
            .enumerate()
            .map(|(i, e)| RawEffectDecl {
                name: RawStrSlice {
                    ptr: e.name.as_ptr(),
                    len: e.name.len(),
                },
                operation_names: effect_operation_names[i].as_ptr(),
                operation_arities: effect_operation_arities[i].as_ptr(),
                operation_count: e.operation_names.len() as u32,
            })
            .collect();

        let raw_arg_pool: Vec<RawValue> = arg_pool.iter().map(Value::to_raw).collect();
        let raw_parameter_names: Vec<RawStrSlice> = parameter_name_pool
            .iter()
            .map(|n| RawStrSlice {
                ptr: n.as_ptr(),
                len: n.len(),
            })
            .collect();

        BuiltWire {
            instructions: raw_instructions,
            top_level_count,
            raw_frames,
            raw_effects,
            raw_arg_pool,
            raw_parameter_names,
            _frames_owned: frames,
            _effects_owned: effects,
            _args_owned: arg_pool,
            _parameter_names_owned: parameter_name_pool,
            _frame_effect_ids: frame_effect_ids,
            _frame_clauses: frame_clauses,
            _effect_operation_names: effect_operation_names,
            _effect_operation_arities: effect_operation_arities,
        }
    }

    fn raw(&self) -> RawWire {
        RawWire {
            instructions: self.instructions.as_ptr(),
            // Dispatcher walks only the top-level block; body bodies
            // are accessed via their own offset/count out of the same
            // base pointer.
            instruction_count: self.top_level_count,
            frames: self.raw_frames.as_ptr(),
            frame_count: self.raw_frames.len() as u32,
            effects: self.raw_effects.as_ptr(),
            effect_count: self.raw_effects.len() as u32,
            arg_pool: self.raw_arg_pool.as_ptr(),
            arg_pool_size: self.raw_arg_pool.len() as u32,
            parameter_name_pool: self.raw_parameter_names.as_ptr(),
            parameter_name_pool_size: self.raw_parameter_names.len() as u32,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────
// Dispatcher — entry point + result/error types
// ──────────────────────────────────────────────────────────────────────

/// Trace event emitted during a dispatch run. Mirrors the C
/// `AxonCsysTraceEvent` with owned values.
#[derive(Debug, Clone, PartialEq)]
pub enum TraceEvent {
    EnterFrame {
        frame_id: u32,
    },
    ExitFrame {
        frame_id: u32,
    },
    Perform {
        state_id: u32,
        effect_id: u32,
        operation_id: u32,
    },
    Resume {
        frame_id: u32,
        value: Value,
    },
    Abort {
        frame_id: u32,
        value: Value,
    },
    Forward {
        source_frame_idx: u32,
        effect_id: u32,
        operation_id: u32,
    },
}

impl TraceEvent {
    fn from_raw(raw: RawTraceEvent) -> Self {
        match raw.kind {
            0 => TraceEvent::EnterFrame {
                frame_id: raw.frame_id,
            },
            1 => TraceEvent::ExitFrame {
                frame_id: raw.frame_id,
            },
            2 => TraceEvent::Perform {
                state_id: raw.frame_id,
                effect_id: raw.effect_id,
                operation_id: raw.operation_id,
            },
            3 => TraceEvent::Resume {
                frame_id: raw.frame_id,
                value: Value::from_raw(raw.value),
            },
            4 => TraceEvent::Abort {
                frame_id: raw.frame_id,
                value: Value::from_raw(raw.value),
            },
            5 => TraceEvent::Forward {
                source_frame_idx: raw.frame_id,
                effect_id: raw.effect_id,
                operation_id: raw.operation_id,
            },
            _ => TraceEvent::EnterFrame {
                frame_id: raw.frame_id,
            },
        }
    }
}

/// Outcome of a dispatcher run.
#[derive(Debug, Clone, PartialEq)]
pub enum DispatchResult {
    /// Block ran to completion; `value` is the last expression result.
    Completed(Value),
    /// A clause invoked `abort(v)` and the abort propagated to the
    /// top of the run; `value` is the abort payload.
    Aborted(Value),
}

/// Defensive errors surfaced by the dispatcher when the typechecker
/// missed an invariant. All variants are `CT-1` / `CT-2` blame in
/// the AXON calculus — the typechecker should reject these statically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchError {
    UnhandledEffect { effect_id: u32, operation_id: u32 },
    UnknownOperation { effect_id: u32, operation_id: u32 },
    NoDischarge,
    ForwardWithoutOuterHandler { effect_id: u32, operation_id: u32 },
    ControlOpcodeOutsideClauseBody,
    StackOverflow,
    Internal,
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::UnhandledEffect { effect_id, operation_id } => write!(
                f,
                "unhandled effect at runtime: effect_id={effect_id}, operation_id={operation_id} \
                 (compiler bug — D9 should reject)"
            ),
            DispatchError::UnknownOperation { effect_id, operation_id } => write!(
                f,
                "unknown operation: effect_id={effect_id}, operation_id={operation_id} \
                 (compiler bug)"
            ),
            DispatchError::NoDischarge => write!(
                f,
                "handler clause walked off without resume/abort/forward (compiler bug — D10)"
            ),
            DispatchError::ForwardWithoutOuterHandler { effect_id, operation_id } => write!(
                f,
                "forward of effect_id={effect_id} (op {operation_id}) has no enclosing outer handler"
            ),
            DispatchError::ControlOpcodeOutsideClauseBody => write!(
                f,
                "control-flow opcode (resume/abort/forward) appeared outside a handler clause body"
            ),
            DispatchError::StackOverflow => write!(
                f,
                "dispatcher stack overflow (max exec depth 256, max handler depth 64)"
            ),
            DispatchError::Internal => write!(f, "internal dispatcher error (likely ABI drift)"),
        }
    }
}

impl std::error::Error for DispatchError {}

/// The dispatcher entry point.
pub struct Dispatcher;

impl Dispatcher {
    /// True if this build of axon-csys uses computed gotos for the
    /// effects dispatcher (gcc / clang). Returns false on MSVC,
    /// which uses a switch-based fallback per founder D5.
    pub fn uses_computed_gotos() -> bool {
        // SAFETY: pure helper, no pointer ops.
        unsafe { axon_csys_effects_uses_computed_gotos() }
    }

    /// Run the wire's top-level instruction block.
    ///
    /// `globals` is an iterable of `(name, value)` pairs that the
    /// dispatcher uses to resolve `Symbol`-typed arguments. Empty if
    /// not needed.
    ///
    /// `trace_capacity`: if > 0, the dispatcher records up to that
    /// many trace events; if `None`, tracing is disabled (fast path).
    pub fn run(
        wire: &BuiltWire,
        globals: &[(String, Value)],
        trace_capacity: Option<usize>,
    ) -> (Result<DispatchResult, DispatchError>, Vec<TraceEvent>) {
        let raw_wire = wire.raw();
        let raw_keys: Vec<RawStrSlice> = globals
            .iter()
            .map(|(k, _)| RawStrSlice {
                ptr: k.as_ptr(),
                len: k.len(),
            })
            .collect();
        let raw_values: Vec<RawValue> = globals.iter().map(|(_, v)| v.to_raw()).collect();

        let trace_cap = trace_capacity.unwrap_or(0);
        // Initialise the trace buffer with zeroed events. The C dispatcher
        // overwrites however many slots it actually uses (reported via
        // `trace_count_out`); the rest stay zero. Zero-initialising avoids
        // UB from reading uninitialised memory in the truncate step below.
        let zero_event = RawTraceEvent {
            kind: 0,
            frame_id: 0,
            effect_id: 0,
            operation_id: 0,
            value: RawValue {
                tag: TAG_UNIT,
                payload: RawValuePayload { i: 0 },
            },
        };
        let mut trace_buf: Vec<RawTraceEvent> = vec![zero_event; trace_cap];
        let mut trace_count_out: u32 = 0;

        // SAFETY: all pointers are stable for the duration of the call;
        // the C dispatcher does not retain them.
        let raw_result = unsafe {
            axon_csys_effects_run(
                &raw_wire,
                if raw_keys.is_empty() {
                    ptr::null()
                } else {
                    raw_keys.as_ptr()
                },
                if raw_values.is_empty() {
                    ptr::null()
                } else {
                    raw_values.as_ptr()
                },
                raw_keys.len() as u32,
                if trace_cap == 0 {
                    ptr::null_mut()
                } else {
                    trace_buf.as_mut_ptr()
                },
                trace_cap as u32,
                &mut trace_count_out,
            )
        };

        // Truncate to the count actually written by the dispatcher.
        trace_buf.truncate(trace_count_out as usize);
        let trace_events: Vec<TraceEvent> =
            trace_buf.into_iter().map(TraceEvent::from_raw).collect();

        let result = match raw_result.kind {
            RESULT_COMPLETED => Ok(DispatchResult::Completed(Value::from_raw(raw_result.value))),
            RESULT_ABORTED => Ok(DispatchResult::Aborted(Value::from_raw(raw_result.value))),
            RESULT_ERROR => Err(decode_error(
                raw_result.error_code,
                raw_result.error_effect_id,
                raw_result.error_operation_id,
            )),
            _ => Err(DispatchError::Internal),
        };

        (result, trace_events)
    }
}

fn decode_error(code: u32, effect_id: u32, operation_id: u32) -> DispatchError {
    match code {
        1 => DispatchError::UnhandledEffect {
            effect_id,
            operation_id,
        },
        2 => DispatchError::UnknownOperation {
            effect_id,
            operation_id,
        },
        3 => DispatchError::NoDischarge,
        4 => DispatchError::ForwardWithoutOuterHandler {
            effect_id,
            operation_id,
        },
        5 => DispatchError::ControlOpcodeOutsideClauseBody,
        6 => DispatchError::StackOverflow,
        _ => DispatchError::Internal,
    }
}

// Keep PhantomData around to silence warnings if we ever add lifetime
// parameters to BuiltWire / Dispatcher in a follow-up.
#[doc(hidden)]
pub struct DispatcherMarker<'a>(PhantomData<&'a c_void>);
