//! Runtime value representation for AXON algebraic effects.
//!
//! At runtime every value is a tagged dynamic [`Value`]. The Python
//! frontend's typechecker (v1.17.0) already proves that values flow
//! through compatible types, so the runtime does no type checking — it
//! only carries enough discriminant to print + serialise + compare.
//!
//! [`Value`] is intentionally small + cheap to clone. One-shot
//! continuations (D2) consume their captured value at most once, so
//! avoidable copies are minimal in the hot path.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A runtime value flowing through perform / resume / abort sites.
///
/// `Symbol` carries an unresolved identifier (e.g. a step output name
/// like `"Extract.output"`); the runtime treats it as opaque since
/// resolving it would require the surrounding flow's environment.
/// Production runs supply concrete values via the IR-loading layer or
/// via `EffectRuntime::bind_global`.
///
/// `Sentinel::Unit` and `Sentinel::Never` are the well-known unit /
/// bottom sentinels. `Never` should never appear at runtime in a
/// well-typed program; its presence indicates the typechecker missed
/// a `resume` on a `Never`-returning operation (a compiler bug).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Unit,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Map(BTreeMap<String, Value>),
    List(Vec<Value>),
    /// Symbolic reference (an identifier string in the IR — e.g.
    /// `token`, `state.last`). The runtime treats it opaquely; tests
    /// may resolve it via the `EffectRuntime::bind_global` table.
    Symbol(String),
}

impl Value {
    /// True iff this value is the unit sentinel.
    pub fn is_unit(&self) -> bool {
        matches!(self, Value::Unit)
    }

    /// Construct from a string — used when the IR carries an argument
    /// expression as raw text (consistent with Python convention for
    /// `arguments: list[str]` on perform / forward sites).
    pub fn from_argument_text(s: &str) -> Self {
        // Heuristic: bare identifiers / dotted paths become Symbols;
        // quoted literals become Strings. The IR already strips quotes
        // at parse time, so anything passed here is interpreted as a
        // symbolic reference unless it parses as a number or `true`/`false`.
        if let Ok(b) = s.parse::<bool>() {
            return Value::Bool(b);
        }
        if let Ok(i) = s.parse::<i64>() {
            return Value::Int(i);
        }
        if let Ok(f) = s.parse::<f64>() {
            return Value::Float(f);
        }
        Value::Symbol(s.to_string())
    }

    /// Best-effort string render for diagnostics + traces.
    pub fn render(&self) -> String {
        match self {
            Value::Unit => "()".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::String(s) => format!("{s:?}"),
            Value::List(items) => {
                let parts: Vec<String> = items.iter().map(|v| v.render()).collect();
                format!("[{}]", parts.join(", "))
            }
            Value::Map(m) => {
                let parts: Vec<String> =
                    m.iter().map(|(k, v)| format!("{k}: {}", v.render())).collect();
                format!("{{{}}}", parts.join(", "))
            }
            Value::Symbol(s) => s.clone(),
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Value::Unit
    }
}
