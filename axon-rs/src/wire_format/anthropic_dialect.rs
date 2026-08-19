//! v1.28.0 — `anthropic` Messages streaming dialect adapter.
//!
//! Matches the Anthropic Messages API streaming wire verbatim per
//! the published API reference:
//!
//!   https://docs.anthropic.com/en/api/messages-streaming
//!
//! # Wire shape (verbatim from Anthropic reference)
//!
//! Anthropic streams W3C SSE with NAMED events forming a structured
//! lifecycle:
//!
//! ```text
//! event: message_start
//! data: {"type": "message_start", "message": {"id": "...", "type": "message",
//!         "role": "assistant", "content": [], "model": "...",
//!         "stop_reason": null, "stop_sequence": null,
//!         "usage": {"input_tokens": N, "output_tokens": N}}}
//!
//! event: content_block_start
//! data: {"type": "content_block_start", "index": 0,
//!         "content_block": {"type": "text", "text": ""}}
//!
//! event: content_block_delta
//! data: {"type": "content_block_delta", "index": 0,
//!         "delta": {"type": "text_delta", "text": "<token>"}}
//!
//! event: content_block_stop
//! data: {"type": "content_block_stop", "index": 0}
//!
//! event: message_delta
//! data: {"type": "message_delta",
//!         "delta": {"stop_reason": "end_turn", "stop_sequence": null},
//!         "usage": {"output_tokens": N}}
//!
//! event: message_stop
//! data: {"type": "message_stop"}
//! ```
//!
//! Event taxonomy:
//!   - `message_start`: announces the message; carries id, role,
//!     model, initial usage
//!   - `content_block_start`: announces a content block (text,
//!     tool_use, etc.); index 0+, monotonically increasing
//!   - `content_block_delta`: incremental content within a block;
//!     for text blocks: `delta: {"type": "text_delta", "text": "..."}`;
//!     for tool_use blocks: `delta: {"type": "input_json_delta",
//!     "partial_json": "..."}`
//!   - `content_block_stop`: closes the block at the given index
//!   - `message_delta`: final usage + stop_reason envelope
//!   - `message_stop`: stream terminator
//!
//! Tool-use blocks (per Anthropic spec):
//!
//! ```text
//! event: content_block_start
//! data: {"type": "content_block_start", "index": N,
//!         "content_block": {"type": "tool_use", "id": "toolu_xxx",
//!                          "name": "<tool>", "input": {}}}
//!
//! event: content_block_delta
//! data: {"type": "content_block_delta", "index": N,
//!         "delta": {"type": "input_json_delta",
//!                  "partial_json": "<json string>"}}
//!
//! event: content_block_stop
//! data: {"type": "content_block_stop", "index": N}
//! ```
//!
//! # axon → anthropic event mapping
//!
//! Adopts on-demand text-block management: a text content block is
//! lazily opened on the first StepToken arrival and stopped on
//! StepComplete OR when a ToolCall arrives (interleaving the
//! tool_use block). The block index advances monotonically.
//!
//! | axon FlowExecutionEvent | anthropic wire frames                                 |
//! |--------------------------|-------------------------------------------------------|
//! | FlowStart                | 1 frame `event: message_start`                        |
//! | StepStart                | 0 frames (lazy text-block opens on first StepToken)   |
//! | StepToken                | (0 or 1) content_block_start(text) + 1 content_block_delta(text_delta) |
//! | StepComplete             | (0 or 1) content_block_stop (closes open text block)  |
//! | ToolCall                 | (0 or 1) content_block_stop(text) + 3 frames for tool_use block |
//! | FlowComplete             | (0 or 1) content_block_stop + 1 message_delta         |
//! | FlowError                | (0 or 1) content_block_stop + 1 message_delta (stop_reason=error_signal) |
//! | flush_terminator         | 1 axon.metadata frame (Q7) + 1 message_stop           |
//!
//! # Pillar trace (D10)
//!
//! - MATHEMATICS — content_block indices are monotonic integers;
//!   block lifecycle (start→delta*→stop) is a finite state machine.
//! - LOGIC — every translate() call respects the
//!   open-block-must-close-before-new-block invariant.
//! - PHILOSOPHY — adopters using Anthropic SDKs (python-anthropic,
//!   anthropic-sdk-typescript, vercel/ai-sdk anthropic provider)
//!   parse the wire verbatim without any axon-specific awareness.
//! - COMPUTING — state machine is bounded (one current text block
//!   index + one rolling block counter); per-event O(1).

use super::{CompleteEnvelope, WireFormatAdapter};
use crate::flow_execution_event::FlowExecutionEvent;
use axum::response::sse::Event;

/// The Anthropic Messages streaming dialect adapter.
pub struct AnthropicDialectAdapter {
    /// Stable message id across the stream. Synthesized from trace_id.
    message_id: String,
    /// Model identifier; constant across the stream. Captured from
    /// the FlowStart event's backend.
    model: String,
    /// Whether `message_start` has been emitted (defensive — should
    /// be true after the first FlowStart translate).
    message_started: bool,
    /// Next content_block index to assign. Monotonically increases.
    next_block_index: usize,
    /// Index of the currently-open text block, if any. Lazy-opened on
    /// the first StepToken; closed on StepComplete / ToolCall /
    /// FlowComplete.
    open_text_block: Option<usize>,
    /// Per-request running counter for synthesizing tool_use IDs.
    tool_use_counter: u64,
    /// Output token count accumulator (best-effort; populated from
    /// StepToken arrivals). Surfaced on the terminal message_delta.
    output_tokens_accumulated: u64,
    /// Whether FlowComplete / FlowError has been translated.
    terminal_emitted: bool,
    /// Tracks how the stream terminated for adopter visibility on
    /// the axon.metadata frame.
    saw_terminal_reason: TerminalReason,
    /// v1.24.0 — Algebraic-policy envelope stashed at
    /// FlowComplete time so `flush_terminator()` can emit a
    /// POPULATED `axon.metadata` frame. `None` pre-FlowComplete.
    /// Adopters consuming the anthropic wire receive per-step
    /// provenance (banking PCI DSS Req 10 / government FedRAMP
    /// AU-2 / legal FRE 502 / medicine 21 CFR Part 11 section 11.10) via
    /// this extension frame; anthropic-compat SDKs ignore unknown
    /// event names verbatim per Anthropic spec Streaming.
    stashed_envelope: Option<CompleteEnvelope>,
    /// v1.32.0 (D6) — the `FlowError.error` diagnostic, stashed when
    /// a `FlowError` is translated so `build_axon_metadata_frame()`
    /// surfaces it as the metadata frame's `error` field. Anthropic
    /// has no native error stop_reason; without this the wire signalled
    /// `terminal_reason: error` but never said WHY. `None` on a flow
    /// that did not error.
    error_detail: Option<String>,
}

/// Closed-catalog terminal reason mirror (parallel to OpenAI dialect).
#[derive(Debug, Clone, Copy, PartialEq)]
enum TerminalReason {
    None,
    Stop,
    Error,
}

impl AnthropicDialectAdapter {
    /// Construct a fresh adapter for a request.
    pub fn new(trace_id: u64) -> Self {
        let message_id = format!("msg_axon_{trace_id:x}");
        Self {
            message_id,
            model: "axon".to_string(),
            message_started: false,
            next_block_index: 0,
            open_text_block: None,
            tool_use_counter: 0,
            output_tokens_accumulated: 0,
            terminal_emitted: false,
            saw_terminal_reason: TerminalReason::None,
            stashed_envelope: None,
            error_detail: None,
        }
    }

    fn build_event(event_name: &'static str, payload: serde_json::Value) -> Event {
        Event::default()
            .event(event_name)
            .data(serde_json::to_string(&payload).unwrap_or_default())
    }

    fn build_message_start(&self) -> Event {
        let payload = serde_json::json!({
            "type": "message_start",
            "message": {
                "id": &self.message_id,
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": &self.model,
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {
                    "input_tokens": 0,
                    "output_tokens": 0,
                }
            }
        });
        Self::build_event("message_start", payload)
    }

    fn build_text_block_start(&self, index: usize) -> Event {
        let payload = serde_json::json!({
            "type": "content_block_start",
            "index": index,
            "content_block": {
                "type": "text",
                "text": "",
            }
        });
        Self::build_event("content_block_start", payload)
    }

    fn build_text_delta(&self, index: usize, text: &str) -> Event {
        let payload = serde_json::json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {
                "type": "text_delta",
                "text": text,
            }
        });
        Self::build_event("content_block_delta", payload)
    }

    fn build_block_stop(&self, index: usize) -> Event {
        let payload = serde_json::json!({
            "type": "content_block_stop",
            "index": index,
        });
        Self::build_event("content_block_stop", payload)
    }

    fn build_tool_use_start(&mut self, index: usize, tool_name: &str) -> Event {
        self.tool_use_counter += 1;
        let tool_id = format!(
            "toolu_axon_{}_{}",
            self.message_id.strip_prefix("msg_axon_").unwrap_or("0"),
            self.tool_use_counter
        );
        let payload = serde_json::json!({
            "type": "content_block_start",
            "index": index,
            "content_block": {
                "type": "tool_use",
                "id": tool_id,
                "name": tool_name,
                "input": {},
            }
        });
        Self::build_event("content_block_start", payload)
    }

    fn build_tool_input_delta(&self, index: usize, partial_json: &str) -> Event {
        let payload = serde_json::json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {
                "type": "input_json_delta",
                "partial_json": partial_json,
            }
        });
        Self::build_event("content_block_delta", payload)
    }

    fn build_message_delta(&self, stop_reason: &str) -> Event {
        let payload = serde_json::json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": stop_reason,
                "stop_sequence": null,
            },
            "usage": {
                "output_tokens": self.output_tokens_accumulated,
            }
        });
        Self::build_event("message_delta", payload)
    }

    fn build_message_stop() -> Event {
        let payload = serde_json::json!({
            "type": "message_stop",
        });
        Self::build_event("message_stop", payload)
    }

    /// Q7 + v1.24.0 — Build the `axon.metadata` extension
    /// frame carrying the full algebraic-policy side-channel data
    /// + flow envelope. Anthropic-compat clients ignore unknown
    /// `event:` names per Anthropic spec Streaming / "Other events"
    /// ("any event-type your client doesn't recognize should be
    /// silently dropped"); SDK-free clients + axon-aware tooling
    /// consume `event: axon.metadata` directly to surface vertical-
    /// regulator audit data.
    ///
    /// Frame shape mirrors the openai dialect's `axon_metadata` JSON
    /// payload — adopters using both dialects (multi-provider
    /// orchestration) get a uniform algebraic-policy surface.
    fn build_axon_metadata_frame(&self) -> Event {
        let mut metadata = serde_json::Map::new();
        if let Some(envelope) = self.stashed_envelope.as_ref() {
            metadata.insert("trace_id".to_string(), serde_json::json!(envelope.trace_id));
            metadata.insert("flow".to_string(), serde_json::json!(&envelope.flow_name));
            metadata.insert(
                "backend".to_string(),
                serde_json::json!(&envelope.backend),
            );
            metadata.insert("success".to_string(), serde_json::json!(envelope.success));
            metadata.insert(
                "steps_executed".to_string(),
                serde_json::json!(envelope.steps_executed),
            );
            metadata.insert(
                "tokens_input".to_string(),
                serde_json::json!(envelope.tokens_input),
            );
            metadata.insert(
                "tokens_output".to_string(),
                serde_json::json!(envelope.tokens_output),
            );
            metadata.insert(
                "latency_ms".to_string(),
                serde_json::json!(envelope.latency_ms),
            );

            // v1.24.0 — stream_policies (elided when empty).
            if !envelope.effect_policies.is_empty() {
                let arr: Vec<serde_json::Value> = envelope
                    .effect_policies
                    .iter()
                    .map(|(step, policy)| serde_json::json!({"step": step, "policy": policy}))
                    .collect();
                metadata.insert(
                    "stream_policies".to_string(),
                    serde_json::Value::Array(arr),
                );
            }
            // v1.24.0 — enforcement_summary (elided when empty).
            if !envelope.enforcement_summaries.is_empty() {
                let mut obj = serde_json::Map::new();
                for (step, summary) in &envelope.enforcement_summaries {
                    obj.insert(
                        step.clone(),
                        serde_json::to_value(summary).unwrap_or(serde_json::Value::Null),
                    );
                }
                metadata.insert(
                    "enforcement_summary".to_string(),
                    serde_json::Value::Object(obj),
                );
            }
            // v1.24.0 — runtime_warnings (elided when empty).
            if !envelope.runtime_warnings.is_empty() {
                let arr: Vec<serde_json::Value> = envelope
                    .runtime_warnings
                    .iter()
                    .map(|w| serde_json::to_value(w).unwrap_or(serde_json::Value::Null))
                    .collect();
                metadata.insert(
                    "runtime_warnings".to_string(),
                    serde_json::Value::Array(arr),
                );
            }
            // v1.24.0 — step_audit (elided when empty).
            if !envelope.step_audit_records.is_empty() {
                let arr: Vec<serde_json::Value> = envelope
                    .step_audit_records
                    .iter()
                    .map(|r| serde_json::to_value(r).unwrap_or(serde_json::Value::Null))
                    .collect();
                metadata.insert("step_audit".to_string(), serde_json::Value::Array(arr));
            }
        }
        let terminal_str = match self.saw_terminal_reason {
            TerminalReason::None => "none",
            TerminalReason::Stop => "stop",
            TerminalReason::Error => "error",
        };
        metadata.insert(
            "terminal_reason".to_string(),
            serde_json::json!(terminal_str),
        );
        // v1.32.0 (D6) — honest failure: when the flow errored, the
        // metadata frame names WHY. Elided on a non-erroring flow.
        if let Some(err) = self.error_detail.as_ref() {
            metadata.insert("error".to_string(), serde_json::json!(err));
        }

        let payload = serde_json::json!({
            "type": "axon.metadata",
            "axon_metadata": metadata,
        });
        Self::build_event("axon.metadata", payload)
    }

    /// Close the currently-open text block (if any). Returns the
    /// content_block_stop frame OR an empty vec if no block is open.
    fn close_text_block_if_open(&mut self) -> Vec<Event> {
        if let Some(idx) = self.open_text_block.take() {
            vec![self.build_block_stop(idx)]
        } else {
            Vec::new()
        }
    }
}

impl WireFormatAdapter for AnthropicDialectAdapter {
    fn dialect(&self) -> &'static str {
        "anthropic"
    }

    fn translate(&mut self, event: &FlowExecutionEvent) -> Vec<Event> {
        match event {
            FlowExecutionEvent::FlowStart { backend, .. } => {
                // Capture the model identifier; emit message_start.
                self.model = backend.clone();
                self.message_started = true;
                vec![self.build_message_start()]
            }
            FlowExecutionEvent::StepStart { .. } => {
                // Lazy text-block open: defer until first StepToken
                // (StepStart-without-tokens shouldn't open an empty
                // block per Anthropic's content semantics).
                Vec::new()
            }
            FlowExecutionEvent::StepToken { content, .. } => {
                // Account toward the usage counter (best-effort).
                self.output_tokens_accumulated += 1;
                let mut events = Vec::new();
                let block_idx = match self.open_text_block {
                    Some(idx) => idx,
                    None => {
                        let idx = self.next_block_index;
                        self.next_block_index += 1;
                        events.push(self.build_text_block_start(idx));
                        self.open_text_block = Some(idx);
                        idx
                    }
                };
                events.push(self.build_text_delta(block_idx, content));
                events
            }
            FlowExecutionEvent::StepComplete { .. } => {
                // Close any open text block; the next step's tokens
                // will lazy-open a fresh block at a new index.
                self.close_text_block_if_open()
            }
            FlowExecutionEvent::ToolCall {
                tool_name, content, ..
            } => {
                // Interleave a tool_use block: close any open text
                // block first, emit start + delta + stop for the
                // tool_use block, advance block index past it.
                let mut events = self.close_text_block_if_open();
                let tool_block_idx = self.next_block_index;
                self.next_block_index += 1;
                events.push(self.build_tool_use_start(tool_block_idx, tool_name));
                events.push(self.build_tool_input_delta(tool_block_idx, content));
                events.push(self.build_block_stop(tool_block_idx));
                events
            }
            FlowExecutionEvent::FlowComplete { .. } => {
                // Close any lingering text block then emit message_delta.
                // message_stop emits from flush_terminator() — separating
                // the terminator allows Q7 axon.metadata to interpose.
                self.terminal_emitted = true;
                self.saw_terminal_reason = TerminalReason::Stop;
                let mut events = self.close_text_block_if_open();
                events.push(self.build_message_delta("end_turn"));
                events
            }
            FlowExecutionEvent::FlowError { error, .. } => {
                // Anthropic has no native error stop_reason; the closest
                // canonical value for an interrupted/failed stream is
                // "stop_sequence" or simply omitting. We emit
                // stop_reason="end_turn" defensively + rely on the
                // axon.metadata frame for adopters who need explicit
                // error signaling (terminal_reason: "error").
                self.terminal_emitted = true;
                self.saw_terminal_reason = TerminalReason::Error;
                // v1.32.0 (D6) — stash the diagnostic for the
                // axon.metadata frame's `error` field.
                self.error_detail = Some(error.clone());
                let mut events = self.close_text_block_if_open();
                events.push(self.build_message_delta("end_turn"));
                events
            }
        }
    }

    /// v1.24.0 — Stash the full envelope so flush_terminator()
    /// can populate the `axon.metadata` frame with real algebraic-
    /// policy data, then emit the standard message_delta sequence
    /// (close-text-block-if-open + message_delta with stop_reason).
    fn build_complete_envelope_event(&mut self, envelope: &CompleteEnvelope) -> Vec<Event> {
        self.terminal_emitted = true;
        self.saw_terminal_reason = TerminalReason::Stop;
        // Stash before emitting frames so a partial-burst send failure
        // can't strand the side-channel data.
        self.stashed_envelope = Some(envelope.clone());
        let mut events = self.close_text_block_if_open();
        events.push(self.build_message_delta("end_turn"));
        events
    }

    fn flush_terminator(&mut self) -> Vec<Event> {
        // v1.24.0 defense-in-depth — close any orphan text
        // block before emitting the terminator. In production this
        // is unreachable (build_complete_envelope_event +
        // translate(FlowComplete/FlowError) both call
        // close_text_block_if_open before returning), but a malformed
        // input stream (producer crashed mid-flight, channel dropped
        // before FlowComplete, fuzz harness driving incomplete
        // sequences) could leave a block open. The Anthropic spec
        // (Streaming / "ill-formed streams") requires every
        // content_block_start to be balanced by content_block_stop;
        // emitting message_stop with an orphan open block produces
        // wire that some strict-validating clients reject.
        let mut frames = self.close_text_block_if_open();
        // Q7 axon.metadata extension frame BEFORE the terminator
        // (anthropic-compat clients ignore unknown events; adopters
        // who want the algebraic-policy data subscribe to the
        // `axon.metadata` event name explicitly).
        frames.push(self.build_axon_metadata_frame());
        // Then D5 message_stop per Anthropic spec.
        frames.push(Self::build_message_stop());
        frames
    }
}
