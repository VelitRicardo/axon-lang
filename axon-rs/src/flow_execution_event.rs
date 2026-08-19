//! v1.24.0 — Flow execution event stream (Layer 1: data-flow integrity).
//!
//! D2 ratificada: `ServerExecutionResult` is replaced by an event-stream
//! return type — the runner emits each event AS IT OCCURS, the SSE
//! handler consumes the stream and forwards directly to the wire.
//!
//! ## The closed event-shape catalog
//!
//! Every observable moment in a flow's execution is one of five
//! events. The catalog is **closed** — adding a new variant requires a
//! D-letter amendment, not a runtime-only patch. Cross-stack drift gate
//! locks the JSON shape so Python + Rust agree on every field byte-for-
//! byte (`tests/fixtures/flow_execution_event/corpus.json`).
//!
//! - **FlowStart** — emitted once before any step. Establishes the
//!   trace identity + chosen backend.
//! - **StepStart** — emitted once per step at its boundary. Carries the
//!   step's source-declared `step_type` so adopters can correlate the
//!   wire event back to the AST.
//! - **StepToken** — emitted per chunk produced by the step's
//!   underlying backend. For streaming backends (Anthropic SSE,
//!   OpenAI SSE, …) this fires per chunk AS THE BYTE ARRIVES.
//!   For non-streaming backends (stub, future deterministic-only),
//!   this fires once with the full step output (post-completion).
//! - **StepComplete** — emitted once per step at its end boundary.
//!   Carries the full output text + token-input/output counters.
//! - **FlowComplete** — terminator (success path). Receiver MUST
//!   treat this as the stream's end.
//! - **FlowError** — terminator (failure path). Receiver MUST treat
//!   this as the stream's end.
//!
//! ## Pillar trace per D2 + D10
//!
//! - **MATHEMATICS** — the catalog is a closed sum type; pattern matching
//!   is exhaustive. Adding a sixth variant breaks the build cross-stack.
//! - **LOGIC** — the receiver invariant is precise: exactly one
//!   `FlowStart`, followed by per-step (`StepStart` → 0..N
//!   `StepToken` → `StepComplete`), followed by exactly one
//!   `FlowComplete` OR `FlowError`. Any sequence violating this
//!   invariant is a producer bug, not a consumer concern.
//! - **PHILOSOPHY** — the source declaration IS the runtime contract:
//!   every `step S { ... }` declaration produces a `StepStart` /
//!   `StepComplete` pair at runtime, named identically.
//! - **COMPUTING** — events are JSON-serializable + clonable + the
//!   stream is a `tokio::sync::mpsc::UnboundedReceiver`; the
//!   producer never blocks the executor on a slow consumer (33.b
//!   layer; backpressure policy from `<stream:<policy>>` is honored
//!   in 33.e).

use serde::{Deserialize, Serialize};

/// One observable moment in a flow's execution. Closed catalog per D2.
///
/// Field naming + JSON serde-rename match the Python mirror
/// (`axon/runtime/flow_execution_event.py`) byte-for-byte. The drift
/// gate at `tests/fixtures/flow_execution_event/corpus.json`
/// asserts both stacks produce byte-identical JSON for each variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FlowExecutionEvent {
    /// Emitted exactly once at the very start of execution.
    FlowStart {
        flow_name: String,
        backend: String,
        timestamp_ms: u64,
    },
    /// Emitted exactly once per step at its start boundary.
    StepStart {
        step_name: String,
        step_index: usize,
        step_type: String,
        /// v2.15.0 (D-letter amendment) — the MULTIPLEXING KEY for honest
        /// out-of-order SSE. A `par`-block's branches execute concurrently and
        /// emit their events INTERLEAVED on the one stream (reactive, wall-clock
        /// arrival — NOT serialized). This carries the emitting node's
        /// `branch_path` (`"par[0]"`, `"par[1].step[2]"`, …) so the SSE consumer
        /// DEMUXES the concurrent branch streams instead of guessing by arrival.
        /// Empty (`skip_serializing_if`) for a sequential top-level step →
        /// byte-identical to the pre-v2.15.0 wire shape (drift gate stays green).
        #[serde(default, skip_serializing_if = "String::is_empty")]
        branch_path: String,
        timestamp_ms: u64,
    },
    /// Emitted per token / chunk produced by the step's underlying
    /// backend. The granularity matches the backend's chunk size
    /// (Anthropic SSE delta, OpenAI streaming chunk, etc.). For
    /// non-streaming backends, fires once per step with the full
    /// output (post-StepComplete in practice; the catalog allows it
    /// to fire either before or after but the convention is during
    /// execution).
    StepToken {
        step_name: String,
        content: String,
        /// Monotonic counter, per-flow. Restarts at 1 for each new
        /// FlowStart. Adopter clients use this to correlate
        /// `Last-Event-ID` resumes (W3C SSE spec).
        token_index: u64,
        /// v2.15.0 — multiplexing key (see `StepStart::branch_path`): routes a
        /// concurrent `par`-branch's tokens to the right logical stream. Empty
        /// for sequential steps.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        branch_path: String,
        timestamp_ms: u64,
    },
    /// Emitted exactly once per step at its end boundary.
    StepComplete {
        step_name: String,
        step_index: usize,
        success: bool,
        full_output: String,
        tokens_input: u64,
        tokens_output: u64,
        /// v2.15.0 — multiplexing key (see `StepStart::branch_path`): marks the
        /// end of a concurrent `par`-branch's step. Empty for sequential steps.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        branch_path: String,
        timestamp_ms: u64,
    },
    /// v1.24.0 — Tool invocation chunk. Emitted by per-step
    /// handlers when the upstream backend's chunk stream signals
    /// `FinishReason::ToolUse` (provider invoked a tool mid-stream).
    /// Closed-catalog event variant; D4 byte-compat preserves
    /// adopter parsers — flows without declared `apply: <tool>`
    /// never emit this event.
    ///
    /// `content` carries the tool-call's structured payload as a
    /// canonical wire-stable string (provider-specific shape today;
    /// future v1.24.0 standardizes per-provider extraction
    /// into a unified `tool_call_id + arguments` schema).
    ToolCall {
        step_name: String,
        tool_name: String,
        content: String,
        timestamp_ms: u64,
    },
    /// Terminator — success path. Receiver MUST close the stream.
    FlowComplete {
        flow_name: String,
        backend: String,
        success: bool,
        steps_executed: usize,
        tokens_input: u64,
        tokens_output: u64,
        latency_ms: u64,
        timestamp_ms: u64,
    },
    /// Terminator — failure path. Receiver MUST close the stream.
    FlowError {
        flow_name: String,
        error: String,
        timestamp_ms: u64,
    },
}

impl FlowExecutionEvent {
    /// Closed predicate: is this the terminator of the stream? After
    /// emitting a terminator, the producer MUST drop the sender so
    /// the receiver's `recv()` returns `None`.
    pub fn is_terminator(&self) -> bool {
        matches!(
            self,
            FlowExecutionEvent::FlowComplete { .. } | FlowExecutionEvent::FlowError { .. }
        )
    }

    /// Closed predicate: is this event step-scoped (carries
    /// `step_name`)?
    pub fn is_step_scoped(&self) -> bool {
        matches!(
            self,
            FlowExecutionEvent::StepStart { .. }
                | FlowExecutionEvent::StepToken { .. }
                | FlowExecutionEvent::StepComplete { .. }
                | FlowExecutionEvent::ToolCall { .. }
        )
    }

    /// String kind for diagnostic / log lines. Matches the JSON
    /// `kind` discriminator field (`snake_case` per serde-rename).
    pub fn kind(&self) -> &'static str {
        match self {
            FlowExecutionEvent::FlowStart { .. } => "flow_start",
            FlowExecutionEvent::StepStart { .. } => "step_start",
            FlowExecutionEvent::StepToken { .. } => "step_token",
            FlowExecutionEvent::StepComplete { .. } => "step_complete",
            FlowExecutionEvent::ToolCall { .. } => "tool_call",
            FlowExecutionEvent::FlowComplete { .. } => "flow_complete",
            FlowExecutionEvent::FlowError { .. } => "flow_error",
        }
    }
}

/// Current Unix-milliseconds timestamp. Helper used by producers
/// emitting events.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev_flow_start() -> FlowExecutionEvent {
        FlowExecutionEvent::FlowStart {
            flow_name: "F".to_string(),
            backend: "stub".to_string(),
            timestamp_ms: 1_000_000,
        }
    }

    fn ev_step_token() -> FlowExecutionEvent {
        FlowExecutionEvent::StepToken {
            step_name: "S".to_string(),
            content: "hello".to_string(),
            token_index: 1,
            branch_path: String::new(),
            timestamp_ms: 1_000_001,
        }
    }

    fn ev_flow_complete() -> FlowExecutionEvent {
        FlowExecutionEvent::FlowComplete {
            flow_name: "F".to_string(),
            backend: "stub".to_string(),
            success: true,
            steps_executed: 1,
            tokens_input: 0,
            tokens_output: 1,
            latency_ms: 50,
            timestamp_ms: 1_000_010,
        }
    }

    #[test]
    fn flow_start_serializes_with_kind_discriminator() {
        let s = serde_json::to_string(&ev_flow_start()).unwrap();
        // Externally tagged enum with kind = "flow_start"; field order
        // matches Python mirror.
        assert!(s.contains(r#""kind":"flow_start""#));
        assert!(s.contains(r#""flow_name":"F""#));
        assert!(s.contains(r#""backend":"stub""#));
        assert!(s.contains(r#""timestamp_ms":1000000"#));
    }

    #[test]
    fn step_token_serializes_with_token_index() {
        let s = serde_json::to_string(&ev_step_token()).unwrap();
        assert!(s.contains(r#""kind":"step_token""#));
        assert!(s.contains(r#""token_index":1"#));
        assert!(s.contains(r#""content":"hello""#));
    }

    #[test]
    fn flow_complete_serializes_with_latency_ms() {
        let s = serde_json::to_string(&ev_flow_complete()).unwrap();
        assert!(s.contains(r#""kind":"flow_complete""#));
        assert!(s.contains(r#""steps_executed":1"#));
        assert!(s.contains(r#""latency_ms":50"#));
        assert!(s.contains(r#""success":true"#));
    }

    #[test]
    fn round_trip_through_json_preserves_every_variant() {
        let cases = vec![
            ev_flow_start(),
            FlowExecutionEvent::StepStart {
                step_name: "S".to_string(),
                step_index: 0,
                step_type: "step".to_string(),
            branch_path: String::new(),
                timestamp_ms: 1,
            },
            ev_step_token(),
            FlowExecutionEvent::StepComplete {
                step_name: "S".to_string(),
                step_index: 0,
                success: true,
                full_output: "hello world".to_string(),
                tokens_input: 0,
                tokens_output: 2,
            branch_path: String::new(),
                timestamp_ms: 2,
            },
            ev_flow_complete(),
            FlowExecutionEvent::FlowError {
                flow_name: "F".to_string(),
                error: "boom".to_string(),
                timestamp_ms: 3,
            },
        ];
        for e in cases {
            let s = serde_json::to_string(&e).unwrap();
            let back: FlowExecutionEvent = serde_json::from_str(&s).unwrap();
            assert_eq!(back, e, "round-trip MUST preserve every variant");
        }
    }

    #[test]
    fn is_terminator_predicate_is_total() {
        assert!(!ev_flow_start().is_terminator());
        assert!(!ev_step_token().is_terminator());
        assert!(!FlowExecutionEvent::StepStart {
            step_name: "S".to_string(),
            step_index: 0,
            step_type: "step".to_string(),
            branch_path: String::new(),
            timestamp_ms: 0,
        }
        .is_terminator());
        assert!(!FlowExecutionEvent::StepComplete {
            step_name: "S".to_string(),
            step_index: 0,
            success: true,
            full_output: "".to_string(),
            tokens_input: 0,
            tokens_output: 0,
            branch_path: String::new(),
            timestamp_ms: 0,
        }
        .is_terminator());
        assert!(ev_flow_complete().is_terminator());
        assert!(FlowExecutionEvent::FlowError {
            flow_name: "F".to_string(),
            error: "x".to_string(),
            timestamp_ms: 0,
        }
        .is_terminator());
    }

    #[test]
    fn is_step_scoped_predicate_is_total() {
        assert!(!ev_flow_start().is_step_scoped());
        assert!(ev_step_token().is_step_scoped());
        assert!(FlowExecutionEvent::StepStart {
            step_name: "S".to_string(),
            step_index: 0,
            step_type: "step".to_string(),
            branch_path: String::new(),
            timestamp_ms: 0,
        }
        .is_step_scoped());
        assert!(FlowExecutionEvent::StepComplete {
            step_name: "S".to_string(),
            step_index: 0,
            success: true,
            full_output: "".to_string(),
            tokens_input: 0,
            tokens_output: 0,
            branch_path: String::new(),
            timestamp_ms: 0,
        }
        .is_step_scoped());
        assert!(!ev_flow_complete().is_step_scoped());
        assert!(!FlowExecutionEvent::FlowError {
            flow_name: "F".to_string(),
            error: "x".to_string(),
            timestamp_ms: 0,
        }
        .is_step_scoped());
    }

    #[test]
    fn kind_strings_match_serde_rename() {
        assert_eq!(ev_flow_start().kind(), "flow_start");
        assert_eq!(ev_step_token().kind(), "step_token");
        assert_eq!(ev_flow_complete().kind(), "flow_complete");
        assert_eq!(
            FlowExecutionEvent::StepStart {
                step_name: "S".to_string(),
                step_index: 0,
                step_type: "".to_string(),
            branch_path: String::new(),
                timestamp_ms: 0,
            }
            .kind(),
            "step_start"
        );
        assert_eq!(
            FlowExecutionEvent::StepComplete {
                step_name: "S".to_string(),
                step_index: 0,
                success: true,
                full_output: "".to_string(),
                tokens_input: 0,
                tokens_output: 0,
            branch_path: String::new(),
                timestamp_ms: 0,
            }
            .kind(),
            "step_complete"
        );
        assert_eq!(
            FlowExecutionEvent::FlowError {
                flow_name: "F".to_string(),
                error: "x".to_string(),
                timestamp_ms: 0,
            }
            .kind(),
            "flow_error"
        );
    }
}
