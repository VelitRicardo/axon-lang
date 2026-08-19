//! IR deserialization for AXON algebraic effects.
//!
//! Mirrors the 8 IR nodes emitted by the Python frontend (v1.17.0):
//! `IREffectDeclaration`, `IREffectOperation`, `IRPerform`,
//! `IRHandlerFrame`, `IRHandlerClause`, `IRResume`, `IRAbort`,
//! `IRForward`. Field shapes match the JSON output of
//! `axon.compiler.ir_nodes.IR*.to_dict()` exactly — verified by the
//! drift gate in 23.h (cross-stack opcode parity).
//!
//! `Instruction` is the discriminated union the runtime walks. Each
//! variant carries the CPS state coordinate (`state_id` / `frame_id`)
//! the FSM dispatch loop uses.

use serde::Deserialize;

use super::value::Value;

/// Top-level effect declaration: `effect Name { ops... }`.
#[derive(Debug, Clone, Deserialize)]
pub struct IREffectDeclaration {
    pub name: String,
    #[serde(default)]
    pub operations: Vec<IREffectOperation>,
    #[serde(default)]
    pub source_line: u32,
    #[serde(default)]
    pub source_column: u32,
}

/// One operation inside an effect declaration.
#[derive(Debug, Clone, Deserialize)]
pub struct IREffectOperation {
    pub name: String,
    #[serde(default)]
    pub type_parameters: Vec<String>,
    #[serde(default)]
    pub parameter_names: Vec<String>,
    #[serde(default)]
    pub parameter_types: Vec<String>,
    #[serde(default)]
    pub return_type: String,
    #[serde(default)]
    pub source_line: u32,
    #[serde(default)]
    pub source_column: u32,
}

/// `perform Effect.Op(args)` — yields control to the matching handler frame.
#[derive(Debug, Clone, Deserialize)]
pub struct IRPerform {
    pub effect_name: String,
    pub operation_name: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub state_id: u32,
    #[serde(default)]
    pub resume_label: String,
}

/// One clause of an `IRHandlerFrame`: `Op(params) -> { body }`.
#[derive(Debug, Clone, Deserialize)]
pub struct IRHandlerClause {
    pub operation_name: String,
    #[serde(default)]
    pub parameter_names: Vec<String>,
    #[serde(default)]
    pub body: Vec<Instruction>,
    #[serde(default)]
    pub source_line: u32,
    #[serde(default)]
    pub source_column: u32,
}

/// `handle E1, E2 { clauses } in { body }` — a single handler frame.
#[derive(Debug, Clone, Deserialize)]
pub struct IRHandlerFrame {
    pub effect_names: Vec<String>,
    #[serde(default)]
    pub clauses: Vec<IRHandlerClause>,
    #[serde(default)]
    pub body: Vec<Instruction>,
    #[serde(default)]
    pub frame_id: u32,
    #[serde(default)]
    pub body_states: Vec<u32>,
    #[serde(default)]
    pub source_line: u32,
    #[serde(default)]
    pub source_column: u32,
}

/// `resume(value)` — invoke the captured one-shot continuation.
#[derive(Debug, Clone, Deserialize)]
pub struct IRResume {
    #[serde(default)]
    pub value_expr: String,
    #[serde(default)]
    pub frame_id: u32,
}

/// `abort(value)` — terminate the handle without resuming.
#[derive(Debug, Clone, Deserialize)]
pub struct IRAbort {
    #[serde(default)]
    pub value_expr: String,
    #[serde(default)]
    pub frame_id: u32,
}

/// `forward Effect.Op(args)` — propagate to the next outer handler.
#[derive(Debug, Clone, Deserialize)]
pub struct IRForward {
    pub effect_name: String,
    pub operation_name: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub source_frame_id: u32,
    #[serde(default)]
    pub state_id: u32,
    #[serde(default)]
    pub resume_label: String,
}

/// Discriminated union over the runtime instructions the FSM dispatch
/// loop walks. Matches `node_type` in the JSON IR exactly.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "node_type", rename_all = "snake_case")]
pub enum Instruction {
    /// `perform Effect.Op(args)` — yield to handler.
    Perform(IRPerform),
    /// `handle E { ... } in { ... }` — push handler frame, run body.
    HandlerFrame(IRHandlerFrame),
    /// `resume(value)` — return from handler to perform site.
    Resume(IRResume),
    /// `abort(value)` — exit handle expression.
    Abort(IRAbort),
    /// `forward Effect.Op(args)` — propagate to outer handler.
    Forward(IRForward),
    /// Catch-all for legacy IR opcodes the runtime currently
    /// passes through unchanged (steps, conditionals, lets, etc.).
    /// Carrying them here lets a handler body composed of mixed
    /// algebraic-effect + legacy nodes deserialize without errors;
    /// the runtime treats them as inert leaves (no-op for FSM).
    #[serde(other)]
    Passthrough,
}

/// Errors arising from IR shape mismatches at deserialization time.
#[derive(Debug)]
pub enum EffectIRError {
    /// The JSON was not parseable into the expected shape.
    DeserializeFailed(serde_json::Error),
    /// A required field was missing or had an unexpected value.
    InvalidShape(String),
}

impl std::fmt::Display for EffectIRError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EffectIRError::DeserializeFailed(e) => write!(f, "deserialize failed: {e}"),
            EffectIRError::InvalidShape(s) => write!(f, "invalid IR shape: {s}"),
        }
    }
}

impl std::error::Error for EffectIRError {}

impl From<serde_json::Error> for EffectIRError {
    fn from(e: serde_json::Error) -> Self {
        EffectIRError::DeserializeFailed(e)
    }
}

/// Parse a `[Instruction]` block from a JSON array — convenience for
/// loading a flow body or handler-clause body in tests / CLI tooling.
pub fn parse_block(json: &str) -> Result<Vec<Instruction>, EffectIRError> {
    Ok(serde_json::from_str(json)?)
}

/// Parse a single `IREffectDeclaration` from JSON.
pub fn parse_effect(json: &str) -> Result<IREffectDeclaration, EffectIRError> {
    Ok(serde_json::from_str(json)?)
}

/// Resolve an argument-text vector to runtime values via the
/// (`Value::from_argument_text`) heuristic. Matches the Python
/// convention for perform/forward arguments stored as strings.
pub fn resolve_arguments(args: &[String]) -> Vec<Value> {
    args.iter().map(|s| Value::from_argument_text(s)).collect()
}
