//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 33.z.k.f (v1.28.0) — Anthropic dialect adapter byte-exact tests.
//!
//! Each test pins a specific wire-frame shape against the Anthropic
//! Messages API streaming spec verbatim:
//!
//!   https://docs.anthropic.com/en/api/messages-streaming
//!
//! Each assertion cites the spec in its docstring so adopters
//! reading the tests can verify every expectation against the
//! published reference.

use axon::flow_execution_event::FlowExecutionEvent;
use axon::wire_format::select_adapter;

/// Extract the `data:` string from axum's Event Debug repr.
/// Same helper as in fase33z_k_e_openai_dialect_adapter.rs.
fn event_data(event: &axum::response::sse::Event) -> String {
    let debug = format!("{event:?}");
    let start = match debug.find("Active(b\"") {
        Some(p) => p + "Active(b\"".len(),
        None => return String::new(),
    };
    let rest = &debug[start..];
    let end = match rest.rfind("\")") {
        Some(p) => p,
        None => return String::new(),
    };
    let raw_bytes = &rest[..end];
    let mut buf = String::with_capacity(raw_bytes.len());
    let mut iter = raw_bytes.chars().peekable();
    while let Some(c) = iter.next() {
        if c == '\\' {
            match iter.next() {
                Some('n') => buf.push('\n'),
                Some('r') => buf.push('\r'),
                Some('t') => buf.push('\t'),
                Some('"') => buf.push('"'),
                Some('\\') => buf.push('\\'),
                Some(other) => {
                    buf.push('\\');
                    buf.push(other);
                }
                None => buf.push('\\'),
            }
        } else {
            buf.push(c);
        }
    }
    let data_start = match buf.find("data: ") {
        Some(p) => p + "data: ".len(),
        None => return String::new(),
    };
    let data_rest = &buf[data_start..];
    let data_end = data_rest.find('\n').unwrap_or(data_rest.len());
    data_rest[..data_end].to_string()
}

/// Extract the `event:` name from axum's Event Debug repr.
fn event_name(event: &axum::response::sse::Event) -> String {
    let debug = format!("{event:?}");
    let start = match debug.find("Active(b\"event: ") {
        Some(p) => p + "Active(b\"event: ".len(),
        None => return String::new(),
    };
    let rest = &debug[start..];
    let end = rest.find("\\n").unwrap_or(rest.len());
    rest[..end].to_string()
}

// ─── §1 — select_adapter("anthropic", ...) returns AnthropicDialectAdapter

#[test]
fn s1_select_adapter_anthropic_returns_anthropic_dialect() {
    let adapter = select_adapter("anthropic", 0xDEAD_BEEF);
    assert_eq!(
        adapter.dialect(),
        "anthropic",
        "33.z.k.f: select_adapter(\"anthropic\", _) MUST return the \
         Anthropic dialect adapter (replaces 33.z.k.d's defensive \
         fallback)"
    );
}

// ─── §2 — FlowStart emits message_start event ───────────────────────

#[test]
fn s2_flow_start_emits_message_start_event() {
    let mut adapter = select_adapter("anthropic", 0x1234);
    let events = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "Chat".into(),
        backend: "claude-3-5-sonnet-20241022".into(),
        timestamp_ms: 1715648400000,
    });
    assert_eq!(events.len(), 1, "FlowStart → 1 message_start frame");
    assert_eq!(
        event_name(&events[0]),
        "message_start",
        "33.z.k.f: FlowStart MUST emit `event: message_start` per \
         Anthropic spec §Streaming / message_start"
    );
    let data = event_data(&events[0]);
    assert!(
        data.contains("\"type\":\"message_start\""),
        "message_start payload MUST carry `type: \"message_start\"`. Got: {data}"
    );
    assert!(
        data.contains("\"role\":\"assistant\""),
        "message.role MUST be \"assistant\" per Anthropic spec. Got: {data}"
    );
    assert!(
        data.contains("\"model\":\"claude-3-5-sonnet-20241022\""),
        "message.model MUST reflect the backend identifier. Got: {data}"
    );
}

// ─── §3 — StepStart is silently consumed (lazy text-block open) ────

#[test]
fn s3_step_start_silently_consumed_lazy_text_block() {
    let mut adapter = select_adapter("anthropic", 0x1234);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "Chat".into(),
        backend: "claude-3-5-sonnet-20241022".into(),
        timestamp_ms: 1,
    });
    let events = adapter.translate(&FlowExecutionEvent::StepStart {
        step_name: "S".into(),
        step_index: 0,
        step_type: "step".into(),
            branch_path: String::new(),
        timestamp_ms: 2,
    });
    assert_eq!(
        events.len(),
        0,
        "33.z.k.f: StepStart silently consumed; text content block \
         opens lazily on first StepToken to avoid emitting empty \
         blocks per Anthropic semantics"
    );
}

// ─── §4 — First StepToken opens text block + emits text_delta ──────

#[test]
fn s4_first_step_token_opens_text_block_then_delta() {
    let mut adapter = select_adapter("anthropic", 0x1234);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "Chat".into(),
        backend: "claude".into(),
        timestamp_ms: 1,
    });
    let events = adapter.translate(&FlowExecutionEvent::StepToken {
        step_name: "Generate".into(),
        content: "Hola".into(),
        token_index: 1,
            branch_path: String::new(),
        timestamp_ms: 2,
    });
    assert_eq!(
        events.len(),
        2,
        "33.z.k.f: first StepToken emits content_block_start(text) + \
         content_block_delta(text_delta). Got {} frames.",
        events.len()
    );
    assert_eq!(event_name(&events[0]), "content_block_start");
    assert_eq!(event_name(&events[1]), "content_block_delta");

    let block_start_data = event_data(&events[0]);
    assert!(
        block_start_data.contains("\"type\":\"content_block_start\""),
        "content_block_start payload MUST carry `type: \"content_block_start\"`. Got: {block_start_data}"
    );
    // serde_json emits keys in alphabetical order; check for the
    // individual fields rather than a fixed substring ordering.
    assert!(
        block_start_data.contains("\"type\":\"text\""),
        "content_block_start MUST announce `content_block.type: \"text\"` \
         per Anthropic spec §Text content. Got: {block_start_data}"
    );
    assert!(
        block_start_data.contains("\"text\":\"\""),
        "content_block_start MUST carry empty `text: \"\"` initial content. Got: {block_start_data}"
    );

    let delta_data = event_data(&events[1]);
    assert!(
        delta_data.contains("\"type\":\"text_delta\""),
        "content_block_delta MUST be `delta.type: \"text_delta\"` \
         per Anthropic spec. Got: {delta_data}"
    );
    assert!(
        delta_data.contains("\"text\":\"Hola\""),
        "content_block_delta MUST carry `delta.text: \"<token>\"`. Got: {delta_data}"
    );
}

// ─── §5 — Second StepToken (same step) reuses the open block ───────

#[test]
fn s5_subsequent_step_token_reuses_open_text_block() {
    let mut adapter = select_adapter("anthropic", 0x1234);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "F".into(),
        backend: "c".into(),
        timestamp_ms: 1,
    });
    let _ = adapter.translate(&FlowExecutionEvent::StepToken {
        step_name: "S".into(),
        content: "Hola".into(),
        token_index: 1,
            branch_path: String::new(),
        timestamp_ms: 2,
    });
    let events = adapter.translate(&FlowExecutionEvent::StepToken {
        step_name: "S".into(),
        content: ", ¿".into(),
        token_index: 2,
            branch_path: String::new(),
        timestamp_ms: 3,
    });
    assert_eq!(
        events.len(),
        1,
        "33.z.k.f: subsequent StepToken emits ONLY content_block_delta \
         (text block already open). Got {} frames.",
        events.len()
    );
    assert_eq!(event_name(&events[0]), "content_block_delta");
}

// ─── §6 — StepComplete closes the open text block ──────────────────

#[test]
fn s6_step_complete_closes_open_text_block() {
    let mut adapter = select_adapter("anthropic", 0x1234);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "F".into(),
        backend: "c".into(),
        timestamp_ms: 1,
    });
    let _ = adapter.translate(&FlowExecutionEvent::StepToken {
        step_name: "S".into(),
        content: "Hola".into(),
        token_index: 1,
            branch_path: String::new(),
        timestamp_ms: 2,
    });
    let events = adapter.translate(&FlowExecutionEvent::StepComplete {
        step_name: "S".into(),
        step_index: 0,
        success: true,
        full_output: "Hola".into(),
        tokens_input: 0,
        tokens_output: 1,
            branch_path: String::new(),
        timestamp_ms: 3,
    });
    assert_eq!(events.len(), 1, "StepComplete → 1 content_block_stop frame");
    assert_eq!(event_name(&events[0]), "content_block_stop");
    let data = event_data(&events[0]);
    assert!(
        data.contains("\"type\":\"content_block_stop\""),
        "content_block_stop MUST carry `type: \"content_block_stop\"`. Got: {data}"
    );
}

// ─── §7 — ToolCall emits tool_use block (start + delta + stop) ─────

#[test]
fn s7_tool_call_emits_tool_use_block_triad() {
    let mut adapter = select_adapter("anthropic", 0x1234);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "F".into(),
        backend: "c".into(),
        timestamp_ms: 1,
    });
    let events = adapter.translate(&FlowExecutionEvent::ToolCall {
        step_name: "Generate".into(),
        tool_name: "search".into(),
        content: "{\"query\":\"axon\"}".into(),
        timestamp_ms: 100,
    });
    // ToolCall arrives WITHOUT an open text block — adapter doesn't
    // need to close one first. Result: 3 frames (tool_use block triad).
    assert_eq!(
        events.len(),
        3,
        "33.z.k.f: ToolCall (no open text block) → content_block_start \
         + content_block_delta(input_json_delta) + content_block_stop = \
         3 frames per Anthropic tool_use spec. Got {} frames.",
        events.len()
    );
    assert_eq!(event_name(&events[0]), "content_block_start");
    assert_eq!(event_name(&events[1]), "content_block_delta");
    assert_eq!(event_name(&events[2]), "content_block_stop");

    let start_data = event_data(&events[0]);
    assert!(
        start_data.contains("\"type\":\"tool_use\""),
        "content_block_start MUST announce `content_block.type: \"tool_use\"`. Got: {start_data}"
    );
    assert!(
        start_data.contains("\"name\":\"search\""),
        "tool_use content_block MUST carry the tool name. Got: {start_data}"
    );

    let delta_data = event_data(&events[1]);
    assert!(
        delta_data.contains("\"type\":\"input_json_delta\""),
        "tool_use delta MUST be `input_json_delta` per Anthropic spec. Got: {delta_data}"
    );
    assert!(
        delta_data.contains("\"partial_json\""),
        "input_json_delta MUST carry `partial_json` field. Got: {delta_data}"
    );
}

// ─── §8 — ToolCall mid-step closes open text block first ───────────

#[test]
fn s8_tool_call_mid_step_closes_text_block_first() {
    let mut adapter = select_adapter("anthropic", 0x1234);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "F".into(),
        backend: "c".into(),
        timestamp_ms: 1,
    });
    // Open a text block via a StepToken.
    let _ = adapter.translate(&FlowExecutionEvent::StepToken {
        step_name: "S".into(),
        content: "Pensando".into(),
        token_index: 1,
            branch_path: String::new(),
        timestamp_ms: 2,
    });
    // Now a ToolCall arrives — adapter must close the text block first.
    let events = adapter.translate(&FlowExecutionEvent::ToolCall {
        step_name: "S".into(),
        tool_name: "search".into(),
        content: "{\"q\":\"x\"}".into(),
        timestamp_ms: 3,
    });
    assert_eq!(
        events.len(),
        4,
        "33.z.k.f: ToolCall mid-text-block → 4 frames: \
         content_block_stop(text) + content_block_start(tool_use) + \
         content_block_delta(input_json_delta) + content_block_stop(tool_use). \
         Got {} frames.",
        events.len()
    );
    assert_eq!(event_name(&events[0]), "content_block_stop");
    assert_eq!(event_name(&events[1]), "content_block_start");
    assert_eq!(event_name(&events[2]), "content_block_delta");
    assert_eq!(event_name(&events[3]), "content_block_stop");
}

// ─── §9 — FlowComplete emits message_delta (terminator deferred) ──

#[test]
fn s9_flow_complete_emits_message_delta_no_message_stop() {
    let mut adapter = select_adapter("anthropic", 0x1234);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "F".into(),
        backend: "c".into(),
        timestamp_ms: 1,
    });
    let events = adapter.translate(&FlowExecutionEvent::FlowComplete {
        flow_name: "F".into(),
        backend: "c".into(),
        success: true,
        steps_executed: 1,
        tokens_input: 0,
        tokens_output: 1,
        latency_ms: 10,
        timestamp_ms: 100,
    });
    // FlowComplete emits message_delta only — message_stop is in
    // flush_terminator() so Q7 axon.metadata can interpose between
    // them.
    assert!(events.iter().any(|e| event_name(e) == "message_delta"));
    assert!(
        !events.iter().any(|e| event_name(e) == "message_stop"),
        "33.z.k.f: message_stop MUST come from flush_terminator(), \
         NOT translate(FlowComplete). This lets Q7 axon.metadata \
         interpose between message_delta and message_stop."
    );
}

// ─── §10 — flush_terminator emits axon.metadata + message_stop ─────

#[test]
fn s10_flush_terminator_emits_axon_metadata_then_message_stop() {
    let mut adapter = select_adapter("anthropic", 0x1234);
    let terminator = adapter.flush_terminator();
    assert_eq!(
        terminator.len(),
        2,
        "33.z.k.f: flush_terminator MUST emit exactly 2 frames: \
         (1) axon.metadata, (2) message_stop. Got {} frames.",
        terminator.len()
    );
    assert_eq!(
        event_name(&terminator[0]),
        "axon.metadata",
        "33.z.k.f Q7: first terminator frame MUST be `event: axon.metadata` \
         (the algebraic-policy preservation extension; \
         anthropic-compat clients ignore unknown events)"
    );
    assert_eq!(
        event_name(&terminator[1]),
        "message_stop",
        "33.z.k.f D5: second terminator frame MUST be `event: message_stop` \
         per Anthropic spec §Streaming / message_stop"
    );
}

// ─── §11 — Block indices advance monotonically across the stream ───

#[test]
fn s11_block_indices_advance_monotonically() {
    let mut adapter = select_adapter("anthropic", 0x1234);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "F".into(),
        backend: "c".into(),
        timestamp_ms: 1,
    });
    // First text block: index 0.
    let s1_events = adapter.translate(&FlowExecutionEvent::StepToken {
        step_name: "S1".into(),
        content: "A".into(),
        token_index: 1,
            branch_path: String::new(),
        timestamp_ms: 2,
    });
    let s1_start_data = event_data(&s1_events[0]);
    assert!(s1_start_data.contains("\"index\":0"));
    // Close + open new step: index 1.
    let _ = adapter.translate(&FlowExecutionEvent::StepComplete {
        step_name: "S1".into(),
        step_index: 0,
        success: true,
        full_output: "A".into(),
        tokens_input: 0,
        tokens_output: 1,
            branch_path: String::new(),
        timestamp_ms: 3,
    });
    let s2_events = adapter.translate(&FlowExecutionEvent::StepToken {
        step_name: "S2".into(),
        content: "B".into(),
        token_index: 1,
            branch_path: String::new(),
        timestamp_ms: 4,
    });
    let s2_start_data = event_data(&s2_events[0]);
    assert!(
        s2_start_data.contains("\"index\":1"),
        "33.z.k.f: second text block MUST have index 1 (monotonic \
         advance after StepComplete closed block 0). Got: {s2_start_data}"
    );
}
