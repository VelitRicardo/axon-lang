//! v1.28.0 — `axon` W3C named-events dialect adapter.
//!
//! This adapter is the **D6 backwards-compat baseline** — its wire
//! output is byte-identical to the v1.27.1 inline emission helpers
//! `build_token_event` / `build_complete_event` / `build_tool_call_event`
//! / `build_error_event` in `axon_server.rs`.
//!
//! # Wire shape (event grammar)
//!
//! ```text
//! event: axon.start
//! id: 0
//! data: {"trace_id": N, "flow": "...", "backend": "...", "timestamp_ms": N}
//!
//! event: axon.token
//! id: 1
//! data: {"step": "...", "trace_id": N, "token": "...", "timestamp_ms": N}
//!
//! event: axon.tool_call
//! id: 2
//! data: {"step": "...", "trace_id": N, "tool_name": "...", "content": "...", "timestamp_ms": N}
//!
//! event: axon.complete
//! id: 3
//! data: {"trace_id": N, "flow": "...", "backend": "...", "steps_executed": N,
//!        "tokens_input": N, "tokens_output": N, "latency_ms": N, "success": bool,
//!        "stream_policies": [...], "enforcement_summary": {...}, "warnings": [...]}
//! ```
//!
//! The `axon.start` event is the structural marker the v1.27.1 wire
//! does NOT emit (it relies on FlowStart being silently consumed at
//! the producer). For 33.z.k.d we PRESERVE that behavior — FlowStart
//! returns an empty `Vec<Event>` from `translate()` so the wire body
//! stays byte-identical with v1.27.1 stub-mode output (1 axon.token +
//! 1 axon.complete).
//!
//! # Stateful tracking
//!
//! - `event_id_counter` — monotonic event ID starting at 1; matches
//!   the v1.27.1 producer's counter.
//! - `trace_id` — request-scoped UUID (reserved upstream).
//! - `terminal_emitted` — flag set when FlowComplete / FlowError
//!   has been translated; `flush_terminator()` no-ops once true
//!   (axon dialect emits the terminator IN-LINE with FlowComplete).

use super::{CompleteEnvelope, WireFormatAdapter};
use crate::flow_execution_event::FlowExecutionEvent;
use axum::response::sse::Event;

/// The axon W3C named-events adapter.
pub struct AxonDialectAdapter {
    event_id_counter: u64,
    trace_id: u64,
    terminal_emitted: bool,
}

impl AxonDialectAdapter {
    /// Construct a fresh adapter for a request. `trace_id` is the
    /// request-scoped UUID reserved by the producer (v1.24.0).
    /// Event ID counter starts at 1 to match v1.27.1's producer
    /// semantics (id=0 reserved for the silently-elided start frame).
    pub fn new(trace_id: u64) -> Self {
        Self {
            event_id_counter: 1,
            trace_id,
            terminal_emitted: false,
        }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.event_id_counter;
        self.event_id_counter += 1;
        id
    }

    /// Build an `axon.token` SSE event byte-identical to v1.27.1's
    /// inline `build_token_event` helper.
    fn build_token_event(&mut self, step_name: &str, token: &str, timestamp_ms: i64) -> Event {
        let data = serde_json::json!({
            "step": step_name,
            "trace_id": self.trace_id,
            "token": token,
            "timestamp_ms": timestamp_ms,
        });
        let event_id = self.next_id();
        Event::default()
            .event("axon.token")
            .id(event_id.to_string())
            .data(serde_json::to_string(&data).unwrap_or_default())
    }

    /// Build an `axon.tool_call` SSE event byte-identical to v1.27.1's
    /// inline `build_tool_call_event` helper.
    fn build_tool_call_event(
        &mut self,
        step_name: &str,
        tool_name: &str,
        content: &str,
        timestamp_ms: u64,
    ) -> Event {
        let data = serde_json::json!({
            "step": step_name,
            "trace_id": self.trace_id,
            "tool_name": tool_name,
            "content": content,
            "timestamp_ms": timestamp_ms,
        });
        let event_id = self.next_id();
        Event::default()
            .event("axon.tool_call")
            .id(event_id.to_string())
            .data(serde_json::to_string(&data).unwrap_or_default())
    }

    /// Build an `axon.error` SSE event byte-identical to v1.27.1's
    /// inline `build_error_event` helper.
    fn build_error_event(&mut self, error_msg: &str) -> Event {
        let data = serde_json::json!({
            "trace_id": self.trace_id,
            "error": error_msg,
            "recoverable": false,
        });
        let event_id = self.next_id();
        Event::default()
            .event("axon.error")
            .id(event_id.to_string())
            .data(serde_json::to_string(&data).unwrap_or_default())
    }

    /// Build an `axon.complete` SSE event. Carries the
    /// success/steps/tokens/latency envelope plus the optional
    /// algebraic-policy side-channels (stream_policies / enforcement_summary
    /// / warnings) when non-empty (D4 byte-compat: empty fields elided).
    ///
    /// NOTE: 33.z.k.d ships this stub that emits a minimal
    /// envelope from the FlowExecutionEvent's data. The full
    /// algebraic-policy population (33.z.k.h) is handled by the
    /// producer-side wiring; this method receives the data via
    /// the FlowComplete event's fields verbatim.
    fn build_complete_event(
        &mut self,
        flow_name: &str,
        backend: &str,
        success: bool,
        steps_executed: u64,
        tokens_input: u64,
        tokens_output: u64,
        latency_ms: u64,
    ) -> Event {
        let data = serde_json::json!({
            "trace_id": self.trace_id,
            "flow": flow_name,
            "backend": backend,
            "steps_executed": steps_executed,
            "tokens_input": tokens_input,
            "tokens_output": tokens_output,
            "latency_ms": latency_ms,
            "success": success,
        });
        let event_id = self.next_id();
        Event::default()
            .event("axon.complete")
            .id(event_id.to_string())
            .data(serde_json::to_string(&data).unwrap_or_default())
    }
}

impl AxonDialectAdapter {
    /// v1.24.0 — Build the full `axon.complete` event with
    /// all the v1.27.1 envelope fields including the optional
    /// algebraic-policy side-channels (stream_policies,
    /// enforcement_summary, warnings). Each side-channel is elided
    /// from the JSON when empty (D4 byte-compat with v1.27.1 inline
    /// `build_complete_event` helper).
    fn build_full_complete_event(&mut self, envelope: &CompleteEnvelope) -> Event {
        let mut data = serde_json::json!({
            "trace_id": envelope.trace_id,
            "flow": envelope.flow_name,
            "backend": envelope.backend,
            "steps_executed": envelope.steps_executed,
            "tokens_input": envelope.tokens_input,
            "tokens_output": envelope.tokens_output,
            "latency_ms": envelope.latency_ms,
            "success": envelope.success,
        });
        // v1.24.0 — stream_policies array, elided when empty.
        if !envelope.effect_policies.is_empty() {
            let arr = envelope
                .effect_policies
                .iter()
                .map(|(step, policy)| serde_json::json!({"step": step, "policy": policy}))
                .collect::<Vec<_>>();
            data.as_object_mut()
                .expect("json object")
                .insert("stream_policies".to_string(), serde_json::Value::Array(arr));
        }
        // v1.24.0 — enforcement_summary, elided when empty.
        if !envelope.enforcement_summaries.is_empty() {
            let mut obj = serde_json::Map::new();
            for (step, summary) in &envelope.enforcement_summaries {
                obj.insert(
                    step.clone(),
                    serde_json::to_value(summary).unwrap_or(serde_json::Value::Null),
                );
            }
            data.as_object_mut().expect("json object").insert(
                "enforcement_summary".to_string(),
                serde_json::Value::Object(obj),
            );
        }
        // v1.24.0 — runtime_warnings, elided when empty.
        if !envelope.runtime_warnings.is_empty() {
            let arr = envelope
                .runtime_warnings
                .iter()
                .map(|w| serde_json::to_value(w).unwrap_or(serde_json::Value::Null))
                .collect::<Vec<_>>();
            data.as_object_mut()
                .expect("json object")
                .insert("warnings".to_string(), serde_json::Value::Array(arr));
        }
        // v2.7.0 — epistemic_envelopes array, elided when empty. The
        // key + element shape ({base, scope, confidence}) match the sync
        // `FlowEnvelope.epistemic_envelopes` byte-for-byte (v2.7.0 parity).
        if !envelope.epistemic_envelopes.is_empty() {
            let arr = envelope
                .epistemic_envelopes
                .iter()
                .map(|e| serde_json::to_value(e).unwrap_or(serde_json::Value::Null))
                .collect::<Vec<_>>();
            data.as_object_mut().expect("json object").insert(
                "epistemic_envelopes".to_string(),
                serde_json::Value::Array(arr),
            );
        }
        // v2.46.0 — temporal_context object, elided when None. The key +
        // shape ({captured_utc, tzdb_version, zones}) match the sync
        // `FlowEnvelope.temporal_context` byte-for-byte (v2.7.0 parity).
        if let Some(temporal) = &envelope.temporal_context {
            data.as_object_mut().expect("json object").insert(
                "temporal_context".to_string(),
                serde_json::to_value(temporal).unwrap_or(serde_json::Value::Null),
            );
        }
        let event_id = self.next_id();
        Event::default()
            .event("axon.complete")
            .id(event_id.to_string())
            .data(serde_json::to_string(&data).unwrap_or_default())
    }
}

impl WireFormatAdapter for AxonDialectAdapter {
    fn dialect(&self) -> &'static str {
        "axon"
    }

    fn build_complete_envelope_event(&mut self, envelope: &CompleteEnvelope) -> Vec<Event> {
        self.terminal_emitted = true;
        vec![self.build_full_complete_event(envelope)]
    }

    fn translate(&mut self, event: &FlowExecutionEvent) -> Vec<Event> {
        match event {
            // FlowStart is silently consumed in v1.27.1 — preserve
            // for D6 byte-compat.
            FlowExecutionEvent::FlowStart { .. } => Vec::new(),
            // StepStart is silently consumed in v1.27.1 — preserve
            // for D6 byte-compat.
            FlowExecutionEvent::StepStart { .. } => Vec::new(),
            FlowExecutionEvent::StepToken {
                step_name,
                content,
                timestamp_ms,
                ..
            } => vec![self.build_token_event(step_name, content, *timestamp_ms as i64)],
            // StepComplete is silently consumed in v1.27.1 — preserve.
            FlowExecutionEvent::StepComplete { .. } => Vec::new(),
            FlowExecutionEvent::ToolCall {
                step_name,
                tool_name,
                content,
                timestamp_ms,
            } => vec![self.build_tool_call_event(
                step_name,
                tool_name,
                content,
                *timestamp_ms,
            )],
            FlowExecutionEvent::FlowComplete {
                flow_name,
                backend,
                success,
                steps_executed,
                tokens_input,
                tokens_output,
                latency_ms,
                ..
            } => {
                self.terminal_emitted = true;
                vec![self.build_complete_event(
                    flow_name,
                    backend,
                    *success,
                    *steps_executed as u64,
                    *tokens_input as u64,
                    *tokens_output as u64,
                    *latency_ms,
                )]
            }
            FlowExecutionEvent::FlowError { error, .. } => {
                self.terminal_emitted = true;
                vec![self.build_error_event(error)]
            }
        }
    }

    fn flush_terminator(&mut self) -> Vec<Event> {
        // axon dialect emits its terminator IN-LINE with the
        // FlowComplete / FlowError translation. No additional
        // terminator frame.
        Vec::new()
    }
}
