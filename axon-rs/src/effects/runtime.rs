//! AXON Algebraic Effects Runtime — FSM dispatch loop.
//!
//! Direct-style interpreter: the Rust call stack mirrors the handler
//! stack, and a `resume` is implemented by returning a value from a
//! recursive call. Captured continuations are not boxed closures —
//! they are the remaining instruction list at the perform site, which
//! the dispatch loop walks through.
//!
//! # Execution model
//!
//! The interpreter walks an `&[Instruction]` block one node at a time.
//! On each node:
//!
//! * `HandlerFrame` — push a frame onto the active stack, recurse into
//!   the body. On exit, pop the frame.
//! * `Perform` — find the matching handler clause in the active stack
//!   (by effect name + operation name). Bind the operation parameters
//!   into the local environment, recurse into the clause body, capture
//!   the result. The result tells us what happened:
//!     - `Resumed(v)` — the perform site receives `v` and continues
//!       executing the rest of the surrounding block.
//!     - `Aborted(v)` — control yields past the enclosing handle
//!       expression; the handle's result is `v`.
//!     - `Forwarded { ... }` — the inner clause did `forward`; the
//!       effect propagates to the next outer frame, which will receive
//!       its own dispatch.
//! * `Resume(value)` — return `Resumed(value)` from the current
//!   recursive call so the perform site picks it up.
//! * `Abort(value)` — return `Aborted(value)`.
//! * `Forward(...)` — return `Forwarded { ... }` so the outer dispatch
//!   loop re-yields to the next frame outward.
//! * `Passthrough` — inert; the FSM treats legacy IR nodes as no-ops.
//!
//! # One-shot continuations (D2)
//!
//! Each captured continuation is consumed exactly once: when a clause
//! returns `Resumed(v)`, the loop advances `i += 1` and continues. If
//! the clause never invokes `resume` (returns `Aborted` or
//! `Forwarded`), the captured continuation is simply dropped — no
//! cleanup needed because nothing was heap-allocated. Multi-resume
//! would require cloning the captured remaining instructions; the
//! typechecker (D10) statically rejects that case so the runtime
//! never has to handle it.

use std::collections::HashMap;

use super::ir::{
    IREffectDeclaration, IREffectOperation, IRHandlerClause, IRHandlerFrame, IRPerform,
    Instruction,
};
use super::value::Value;

/// Errors raised at runtime by the effects FSM.
///
/// All variants are `CT-1` (compiler-bug) or `CT-2` (program-bug)
/// territory in the AXON blame calculus — the typechecker should
/// have caught any well-formedness issue before lowering. The runtime
/// surfaces them anyway so the FSM is operationally complete.
#[derive(Debug)]
pub enum EffectRuntimeError {
    /// Performed an effect that has no enclosing handler in scope.
    /// Typechecker D9 should reject this statically; if it gets here
    /// the compiler has a bug.
    UnhandledEffect { effect: String, operation: String },
    /// Performed an operation that does not exist on the named effect.
    UnknownOperation { effect: String, operation: String },
    /// Handler clause invoked `resume` more than once on a single path.
    /// Typechecker D10 should reject this statically.
    MultipleResumeInClause { operation: String },
    /// Handler clause finished without invoking resume / abort /
    /// forward. Typechecker D10 should reject this statically.
    NoDischarge { operation: String },
    /// `forward` invoked outside a handler clause body — same as a
    /// regular perform with no handler.
    ForwardWithoutOuterHandler { effect: String },
    /// Generic dispatch error (panic-converted).
    Internal(String),
}

impl std::fmt::Display for EffectRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EffectRuntimeError::UnhandledEffect { effect, operation } => write!(
                f,
                "unhandled effect at runtime: {effect}.{operation} (compiler bug — D9 should reject)"
            ),
            EffectRuntimeError::UnknownOperation { effect, operation } => write!(
                f,
                "unknown operation: {effect} has no '{operation}' (compiler bug)"
            ),
            EffectRuntimeError::MultipleResumeInClause { operation } => write!(
                f,
                "handler clause '{operation}' invoked resume more than once (compiler bug — D10 should reject)"
            ),
            EffectRuntimeError::NoDischarge { operation } => write!(
                f,
                "handler clause '{operation}' finished without resume/abort/forward (compiler bug — D10 should reject)"
            ),
            EffectRuntimeError::ForwardWithoutOuterHandler { effect } => write!(
                f,
                "forward of '{effect}' has no enclosing outer handler"
            ),
            EffectRuntimeError::Internal(s) => write!(f, "internal runtime error: {s}"),
        }
    }
}

impl std::error::Error for EffectRuntimeError {}

/// The terminal result of running an instruction block (top-level).
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionResult {
    /// Block completed normally — value is whatever the last
    /// `resume(v)` produced (or `Unit` if the block was a sequence
    /// of perform/handle returning Unit).
    Completed(Value),
    /// Block aborted (a clause invoked `abort(v)` and the abort
    /// propagated to the top of the run).
    Aborted(Value),
}

// ════════════════════════════════════════════════════════════════════
//  Internal dispatch types
// ════════════════════════════════════════════════════════════════════

/// What a (recursive) call to `dispatch_block` returned.
#[derive(Debug)]
enum BlockOutcome {
    /// The block ran to completion.
    Done(Value),
    /// The block hit `abort(v)`; control should propagate to the
    /// enclosing `HandlerFrame` (which converts it to its own value).
    Aborted(Value),
    /// The block hit `forward Effect.Op(args)`; control should
    /// propagate to the next outer handler frame.
    Forwarded {
        effect: String,
        operation: String,
        args: Vec<Value>,
    },
}

/// What a clause body ran to. Distinct from `BlockOutcome` because a
/// clause is *expected* to terminate via resume/abort/forward (D10).
#[derive(Debug)]
enum ClauseOutcome {
    /// Clause invoked `resume(v)`.
    Resumed(Value),
    /// Clause invoked `abort(v)`.
    Aborted(Value),
    /// Clause invoked `forward Effect.Op(args)`.
    Forwarded {
        effect: String,
        operation: String,
        args: Vec<Value>,
    },
}

// ════════════════════════════════════════════════════════════════════
//  EffectRuntime — the FSM interpreter
// ════════════════════════════════════════════════════════════════════

/// The Algebraic Effects runtime.
///
/// Construct with `EffectRuntime::new()`, register effect declarations
/// via `register_effect`, then execute a block of instructions via
/// `run`.
pub struct EffectRuntime {
    /// Effect declarations indexed by name. Used for arity validation
    /// at perform-site dispatch + future operation-polymorphism
    /// monomorphisation.
    effects: HashMap<String, IREffectDeclaration>,
    /// Named values bound by the host (e.g. test fixtures, step
    /// outputs). Looked up when resolving `Symbol(name)` arguments.
    globals: HashMap<String, Value>,
    /// Optional event sink for tracing every dispatch step. Useful in
    /// tests to assert FSM transitions in order.
    trace: Vec<TraceEvent>,
    /// Whether to record trace events. Off by default for
    /// performance.
    tracing_enabled: bool,
}

/// One event emitted by the runtime during execution. Used by tests +
/// observability layers to verify the FSM transitions.
#[derive(Debug, Clone, PartialEq)]
pub enum TraceEvent {
    EnterFrame { frame_id: u32, effects: Vec<String> },
    ExitFrame { frame_id: u32 },
    Perform { effect: String, operation: String, state_id: u32 },
    Resume { frame_id: u32, value: Value },
    Abort { frame_id: u32, value: Value },
    Forward { effect: String, operation: String, source_frame_id: u32 },
}

impl Default for EffectRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl EffectRuntime {
    pub fn new() -> Self {
        Self {
            effects: HashMap::new(),
            globals: HashMap::new(),
            trace: Vec::new(),
            tracing_enabled: false,
        }
    }

    /// Register an effect declaration so perform sites can validate
    /// operation arity + parameter shapes at dispatch.
    pub fn register_effect(&mut self, eff: IREffectDeclaration) {
        self.effects.insert(eff.name.clone(), eff);
    }

    /// Bind a named value into the global environment (used to
    /// resolve symbolic arguments at perform sites).
    pub fn bind_global(&mut self, name: impl Into<String>, value: Value) {
        self.globals.insert(name.into(), value);
    }

    /// Enable trace event recording. Off by default.
    pub fn enable_tracing(&mut self) {
        self.tracing_enabled = true;
    }

    /// Drain the recorded trace, leaving the runtime tracing in a
    /// fresh state.
    pub fn take_trace(&mut self) -> Vec<TraceEvent> {
        std::mem::take(&mut self.trace)
    }

    fn record(&mut self, event: TraceEvent) {
        if self.tracing_enabled {
            self.trace.push(event);
        }
    }

    /// Run a block of instructions and produce a terminal
    /// `ExecutionResult`.
    pub fn run(&mut self, block: &[Instruction]) -> Result<ExecutionResult, EffectRuntimeError> {
        // Run with an empty handler stack — perform sites without an
        // enclosing HandlerFrame will surface as `UnhandledEffect`.
        let mut stack: Vec<HandlerFrameRef<'_>> = Vec::new();
        match self.dispatch_block(block, &mut stack)? {
            BlockOutcome::Done(v) => Ok(ExecutionResult::Completed(v)),
            BlockOutcome::Aborted(v) => Ok(ExecutionResult::Aborted(v)),
            BlockOutcome::Forwarded { effect, .. } => {
                Err(EffectRuntimeError::UnhandledEffect {
                    effect,
                    operation: "<forwarded>".to_string(),
                })
            }
        }
    }

    /// Resolve the argument text vector into runtime values, looking
    /// up symbols against the globals table.
    fn resolve_args(&self, args: &[String]) -> Vec<Value> {
        args.iter()
            .map(|s| {
                let v = Value::from_argument_text(s);
                if let Value::Symbol(ref name) = v {
                    if let Some(bound) = self.globals.get(name) {
                        return bound.clone();
                    }
                }
                v
            })
            .collect()
    }

    /// Walk a block of instructions in the active handler-stack
    /// context. Returns either the block's terminal value or a control
    /// transfer (Aborted / Forwarded).
    fn dispatch_block<'a>(
        &mut self,
        block: &'a [Instruction],
        stack: &mut Vec<HandlerFrameRef<'a>>,
    ) -> Result<BlockOutcome, EffectRuntimeError> {
        let mut last_value = Value::Unit;
        let mut i = 0;
        while i < block.len() {
            match &block[i] {
                Instruction::Passthrough => {
                    i += 1;
                }

                Instruction::HandlerFrame(frame) => {
                    let outcome = self.dispatch_handler_frame(frame, stack)?;
                    match outcome {
                        BlockOutcome::Done(v) => {
                            last_value = v;
                            i += 1;
                        }
                        BlockOutcome::Aborted(v) => return Ok(BlockOutcome::Aborted(v)),
                        BlockOutcome::Forwarded { effect, operation, args } => {
                            return Ok(BlockOutcome::Forwarded { effect, operation, args });
                        }
                    }
                }

                Instruction::Perform(perf) => {
                    let outcome = self.dispatch_perform(perf, stack)?;
                    match outcome {
                        BlockOutcome::Done(v) => {
                            last_value = v;
                            i += 1;
                        }
                        BlockOutcome::Aborted(v) => return Ok(BlockOutcome::Aborted(v)),
                        BlockOutcome::Forwarded { effect, operation, args } => {
                            // The perform was forwarded by an inner clause and
                            // bubbled back up; propagate further.
                            return Ok(BlockOutcome::Forwarded { effect, operation, args });
                        }
                    }
                }

                Instruction::Resume(_) | Instruction::Abort(_) | Instruction::Forward(_) => {
                    // These are clause-body terminators; reaching them
                    // inside a regular block means the IR is malformed
                    // (typechecker should have caught it). Treat as
                    // internal error rather than panic so callers can
                    // surface the location.
                    return Err(EffectRuntimeError::Internal(format!(
                        "control-flow opcode {:?} appeared outside a handler clause body",
                        std::mem::discriminant(&block[i]),
                    )));
                }
            }
        }
        Ok(BlockOutcome::Done(last_value))
    }

    /// Push a handler frame onto the stack, run its body, pop the
    /// frame. Result is the body's terminal value (or an Abort/Forward
    /// that propagated past the body).
    fn dispatch_handler_frame<'a>(
        &mut self,
        frame: &'a IRHandlerFrame,
        stack: &mut Vec<HandlerFrameRef<'a>>,
    ) -> Result<BlockOutcome, EffectRuntimeError> {
        self.record(TraceEvent::EnterFrame {
            frame_id: frame.frame_id,
            effects: frame.effect_names.clone(),
        });
        stack.push(HandlerFrameRef { frame });
        let result = self.dispatch_block(&frame.body, stack);
        stack.pop();
        self.record(TraceEvent::ExitFrame { frame_id: frame.frame_id });
        result
    }

    /// Dispatch a perform site: find the matching handler clause,
    /// bind its parameters, run the clause body, propagate the
    /// outcome.
    fn dispatch_perform<'a>(
        &mut self,
        perf: &IRPerform,
        stack: &mut Vec<HandlerFrameRef<'a>>,
    ) -> Result<BlockOutcome, EffectRuntimeError> {
        self.record(TraceEvent::Perform {
            effect: perf.effect_name.clone(),
            operation: perf.operation_name.clone(),
            state_id: perf.state_id,
        });
        // Search from the top (innermost) frame outward.
        let start = stack.len();
        let frame_idx = self.find_handler_index(stack, &perf.effect_name, start)?;
        let args = self.resolve_args(&perf.arguments);
        self.dispatch_clause_for(perf.effect_name.as_str(), perf.operation_name.as_str(), &args, stack, frame_idx)
    }

    /// Walk the handler stack outward from `start_exclusive - 1` to 0
    /// (i.e., from `start_exclusive`-1 toward the outermost frame),
    /// returning the first frame index that lists `effect_name`.
    ///
    /// For `perform`, `start_exclusive = stack.len()` so the walk
    /// includes the topmost (innermost) frame.
    ///
    /// For `forward`, `start_exclusive = source_frame_stack_idx`
    /// so the source frame is bypassed AND any frame nested below it
    /// is also bypassed (those nested frames live at *higher*
    /// stack indices than the source — they cannot be the next
    /// outer handler the forward should reach).
    fn find_handler_index<'a>(
        &self,
        stack: &[HandlerFrameRef<'a>],
        effect_name: &str,
        start_exclusive: usize,
    ) -> Result<usize, EffectRuntimeError> {
        for i in (0..start_exclusive).rev() {
            if stack[i].frame.effect_names.iter().any(|e| e == effect_name) {
                return Ok(i);
            }
        }
        Err(EffectRuntimeError::UnhandledEffect {
            effect: effect_name.to_string(),
            operation: String::new(),
        })
    }

    /// Run a clause body with the operation parameters bound in scope.
    /// Returns the BlockOutcome produced by the surrounding context.
    fn dispatch_clause_for<'a>(
        &mut self,
        effect_name: &str,
        operation_name: &str,
        args: &[Value],
        stack: &mut Vec<HandlerFrameRef<'a>>,
        frame_idx: usize,
    ) -> Result<BlockOutcome, EffectRuntimeError> {
        let frame_ref = stack[frame_idx];
        let frame = frame_ref.frame;
        let clause = frame
            .clauses
            .iter()
            .find(|c| c.operation_name == operation_name)
            .ok_or_else(|| EffectRuntimeError::UnknownOperation {
                effect: effect_name.to_string(),
                operation: operation_name.to_string(),
            })?;
        // Bind clause parameters into globals scoped to this dispatch.
        // The host language has no formal locals/params here yet —
        // future v1.18.0 may add a per-clause env. For now we mutate
        // globals + restore after.
        let saved: Vec<(String, Option<Value>)> = clause
            .parameter_names
            .iter()
            .map(|name| (name.clone(), self.globals.get(name).cloned()))
            .collect();
        for (name, value) in clause.parameter_names.iter().zip(args.iter()) {
            self.globals.insert(name.clone(), value.clone());
        }

        let clause_result = self.dispatch_clause_body(clause, stack);

        // Restore prior globals bindings (or remove if newly bound).
        for (name, prior) in saved {
            match prior {
                Some(v) => {
                    self.globals.insert(name, v);
                }
                None => {
                    self.globals.remove(&name);
                }
            }
        }

        match clause_result? {
            ClauseOutcome::Resumed(v) => Ok(BlockOutcome::Done(v)),
            ClauseOutcome::Aborted(v) => Ok(BlockOutcome::Aborted(v)),
            ClauseOutcome::Forwarded { effect, operation, args } => {
                // Forward: bypass this frame AND any frames nested
                // inside it; search outward only (toward index 0).
                // `frame_idx` is the source frame's stack index, so
                // the search starts strictly below it.
                let outer_idx_result = self.find_handler_index(stack, &effect, frame_idx);
                let args_vec = args.clone();
                match outer_idx_result {
                    Ok(outer_idx) => {
                        self.dispatch_clause_for(&effect, &operation, &args_vec, stack, outer_idx)
                    }
                    Err(_) => Ok(BlockOutcome::Forwarded { effect, operation, args }),
                }
            }
        }
    }

    /// Walk a clause body. The body is *expected* to terminate via
    /// resume / abort / forward — the typechecker (D10) guarantees
    /// this for well-typed programs. If we walk off the end without
    /// any of those, we surface `NoDischarge`.
    fn dispatch_clause_body<'a>(
        &mut self,
        clause: &'a IRHandlerClause,
        stack: &mut Vec<HandlerFrameRef<'a>>,
    ) -> Result<ClauseOutcome, EffectRuntimeError> {
        for instr in &clause.body {
            match instr {
                Instruction::Resume(r) => {
                    let value = self.resolve_value_expr(&r.value_expr);
                    self.record(TraceEvent::Resume {
                        frame_id: r.frame_id,
                        value: value.clone(),
                    });
                    return Ok(ClauseOutcome::Resumed(value));
                }
                Instruction::Abort(a) => {
                    let value = self.resolve_value_expr(&a.value_expr);
                    self.record(TraceEvent::Abort {
                        frame_id: a.frame_id,
                        value: value.clone(),
                    });
                    return Ok(ClauseOutcome::Aborted(value));
                }
                Instruction::Forward(fwd) => {
                    self.record(TraceEvent::Forward {
                        effect: fwd.effect_name.clone(),
                        operation: fwd.operation_name.clone(),
                        source_frame_id: fwd.source_frame_id,
                    });
                    let args = self.resolve_args(&fwd.arguments);
                    return Ok(ClauseOutcome::Forwarded {
                        effect: fwd.effect_name.clone(),
                        operation: fwd.operation_name.clone(),
                        args,
                    });
                }
                Instruction::HandlerFrame(inner_frame) => {
                    // Nested handler inside a clause body — run it,
                    // capture whatever value it produces, continue.
                    match self.dispatch_handler_frame(inner_frame, stack)? {
                        BlockOutcome::Done(_) => continue,
                        BlockOutcome::Aborted(v) => {
                            // Inner abort propagates as the clause's outcome.
                            return Ok(ClauseOutcome::Aborted(v));
                        }
                        BlockOutcome::Forwarded { effect, operation, args } => {
                            return Ok(ClauseOutcome::Forwarded { effect, operation, args });
                        }
                    }
                }
                Instruction::Perform(perf) => {
                    // Perform inside a clause body — if the outer scope
                    // (excluding this clause's frame) handles it, run it
                    // then continue. Otherwise the clause body is itself
                    // unhandled, which is a compiler bug.
                    match self.dispatch_perform(perf, stack)? {
                        BlockOutcome::Done(_) => continue,
                        BlockOutcome::Aborted(v) => return Ok(ClauseOutcome::Aborted(v)),
                        BlockOutcome::Forwarded { effect, operation, args } => {
                            return Ok(ClauseOutcome::Forwarded { effect, operation, args });
                        }
                    }
                }
                Instruction::Passthrough => {
                    continue;
                }
            }
        }
        Err(EffectRuntimeError::NoDischarge {
            operation: clause.operation_name.clone(),
        })
    }

    /// Resolve a `value_expr` string (the IR carries it verbatim
    /// from the AXON source) into a runtime Value. Identifiers look
    /// up the globals table; literals parse directly.
    fn resolve_value_expr(&self, expr: &str) -> Value {
        if expr.is_empty() {
            return Value::Unit;
        }
        let v = Value::from_argument_text(expr);
        if let Value::Symbol(ref name) = v {
            if let Some(bound) = self.globals.get(name) {
                return bound.clone();
            }
        }
        v
    }

    /// Look up an effect operation by name. Returns `None` if the
    /// effect is not registered or the operation is not declared.
    pub fn lookup_operation(
        &self,
        effect: &str,
        operation: &str,
    ) -> Option<&IREffectOperation> {
        self.effects
            .get(effect)?
            .operations
            .iter()
            .find(|op| op.name == operation)
    }

    /// Read-only access to registered effects (test introspection).
    pub fn effects(&self) -> &HashMap<String, IREffectDeclaration> {
        &self.effects
    }
}

/// A handler frame on the active handler stack. Holds a reference
/// into the IR so the runtime never clones frame bodies.
#[derive(Clone, Copy)]
struct HandlerFrameRef<'a> {
    frame: &'a IRHandlerFrame,
}
