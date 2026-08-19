//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.28.0 — OpenAI dialect adapter byte-exact tests.
//!
//! Each test pins a specific wire-frame shape against the OpenAI
//! Chat Completions streaming spec verbatim. Test fixtures are
//! extracted from the published API reference at
//! https://platform.openai.com/docs/api-reference/chat/streaming.
//!
//! # Validation strategy
//!
//! axum's `Event` type is opaque (no public byte-extraction API).
//! We validate by:
//!   1. Driving the adapter through a fixture event sequence.
//!   2. Inspecting the returned `Vec<Event>` count + ordering.
//!   3. Parsing the JSON payload from each event's `data:` line
//!      via a small helper that pokes into the `Event`'s Debug
//!      representation (axum's Event debug-prints the data field
//!      verbatim).
//!
//! This is good enough for unit-level shape verification. The
//! end-to-end byte verification happens via the 33.z.k.a anchor +
//! the dedicated 33.z.k integration tests once the producer is
//! wired in 33.z.k.g.
//!
//! # Spec anchors
//!
//! All wire-shape assertions cite the OpenAI streaming spec
//! verbatim in their test docstring. Adopters reading these tests
//! can verify each expectation against the published reference.

use axon::flow_execution_event::FlowExecutionEvent;
use axon::wire_format::select_adapter;

/// Helper to extract the data string from an Event via its Debug
/// representation. axum's Event Debug shape is:
///   `Event { buffer: Active(b"event: <name>\nid: <id>\ndata: <data>\n"), flags: ... }`
///
/// We extract the bytes between `Active(b"` and the matching `")`,
/// then parse the SSE-framed form to pluck the `data:` field.
fn event_data(event: &axum::response::sse::Event) -> String {
    let debug = format!("{event:?}");
    // Find the `Active(b"` prefix.
    let start = match debug.find("Active(b\"") {
        Some(p) => p + "Active(b\"".len(),
        None => return String::new(),
    };
    // Find the matching `")` after start.
    let rest = &debug[start..];
    let end = match rest.rfind("\")") {
        Some(p) => p,
        None => return String::new(),
    };
    let raw_bytes = &rest[..end];
    // raw_bytes is a Rust byte-string literal body. Replace escape
    // sequences inline: \\n → \n, \\" → ", \\\\ → \\.
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
    // buf is now the SSE framed form like:
    //   "data: {...}\n" or "event: <name>\nid: <id>\ndata: {...}\n"
    // Find the `data: ` prefix + read until the next \n (or end).
    let data_start = match buf.find("data: ") {
        Some(p) => p + "data: ".len(),
        None => return String::new(),
    };
    let data_rest = &buf[data_start..];
    let data_end = data_rest.find('\n').unwrap_or(data_rest.len());
    data_rest[..data_end].to_string()
}

/// Build the same canonical event sequence we'd see for a single-
/// step stub-backend flow: FlowStart + StepStart + StepToken("Hola") +
/// StepComplete + FlowComplete.
fn canonical_stub_event_sequence() -> Vec<FlowExecutionEvent> {
    vec![
        FlowExecutionEvent::FlowStart {
            flow_name: "Chat".into(),
            backend: "gpt-4o".into(),
            timestamp_ms: 1715648400000,
        },
        FlowExecutionEvent::StepStart {
            step_name: "Generate".into(),
            step_index: 0,
            step_type: "step".into(),
            branch_path: String::new(),
            timestamp_ms: 1715648400001,
        },
        FlowExecutionEvent::StepToken {
            step_name: "Generate".into(),
            content: "Hola".into(),
            token_index: 1,
            branch_path: String::new(),
            timestamp_ms: 1715648400002,
        },
        FlowExecutionEvent::StepComplete {
            step_name: "Generate".into(),
            step_index: 0,
            success: true,
            full_output: "Hola".into(),
            tokens_input: 0,
            tokens_output: 1,
            branch_path: String::new(),
            timestamp_ms: 1715648400003,
        },
        FlowExecutionEvent::FlowComplete {
            flow_name: "Chat".into(),
            backend: "gpt-4o".into(),
            success: true,
            steps_executed: 1,
            tokens_input: 0,
            tokens_output: 1,
            latency_ms: 3,
            timestamp_ms: 1715648400004,
        },
    ]
}

// ─── section 1 — select_adapter("openai", ...) returns OpenAIDialectAdapter

#[test]
fn s1_select_adapter_openai_returns_openai_dialect() {
    let adapter = select_adapter("openai", 0xDEAD_BEEF);
    assert_eq!(
        adapter.dialect(),
        "openai",
        "33.z.k.e: select_adapter(\"openai\", _) MUST return the \
         OpenAI dialect adapter (replaces 33.z.k.d's defensive \
         fallback)"
    );
}

// ─── section 2 — FlowStart emits role-marker chunk (OpenAI spec First chunk)

#[test]
fn s2_flow_start_emits_role_marker_chunk() {
    let mut adapter = select_adapter("openai", 0x1234);
    let events = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "Chat".into(),
        backend: "gpt-4o".into(),
        timestamp_ms: 1715648400000,
    });
    assert_eq!(events.len(), 1, "FlowStart → exactly 1 wire frame");
    let data = event_data(&events[0]);
    // OpenAI spec: first chunk's delta carries {"role": "assistant"}.
    assert!(
        data.contains("\"role\":\"assistant\""),
        "33.z.k.e: FlowStart frame MUST contain `delta: {{\"role\": \"assistant\"}}` \
         per OpenAI spec Chat / Create / Streaming / Response chunks. \
         Got data: {data}"
    );
    assert!(
        data.contains("\"object\":\"chat.completion.chunk\""),
        "33.z.k.e: every frame MUST contain `object: \"chat.completion.chunk\"`. \
         Got data: {data}"
    );
}

// ─── section 3 — StepToken emits content-delta chunk ──────────────────────

#[test]
fn s3_step_token_emits_content_delta_chunk() {
    let mut adapter = select_adapter("openai", 0x1234);
    // Walk past the FlowStart frame first to set the model field.
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "Chat".into(),
        backend: "gpt-4o".into(),
        timestamp_ms: 1,
    });
    let events = adapter.translate(&FlowExecutionEvent::StepToken {
        step_name: "Generate".into(),
        content: "Hola".into(),
        token_index: 1,
            branch_path: String::new(),
        timestamp_ms: 2,
    });
    assert_eq!(events.len(), 1);
    let data = event_data(&events[0]);
    assert!(
        data.contains("\"content\":\"Hola\""),
        "33.z.k.e: StepToken frame MUST contain `delta: {{\"content\": \"<token>\"}}` \
         per OpenAI spec. Got data: {data}"
    );
}

// ─── section 4 — StepStart and StepComplete are silently consumed ──────────

#[test]
fn s4_step_start_silently_consumed_in_openai_dialect() {
    let mut adapter = select_adapter("openai", 0x1234);
    let events = adapter.translate(&FlowExecutionEvent::StepStart {
        step_name: "S".into(),
        step_index: 0,
        step_type: "step".into(),
            branch_path: String::new(),
        timestamp_ms: 1,
    });
    assert_eq!(
        events.len(),
        0,
        "33.z.k.e: OpenAI dialect has no multi-step concept; \
         StepStart/StepComplete are silently consumed"
    );
}

#[test]
fn s4_step_complete_silently_consumed_in_openai_dialect() {
    let mut adapter = select_adapter("openai", 0x1234);
    let events = adapter.translate(&FlowExecutionEvent::StepComplete {
        step_name: "S".into(),
        step_index: 0,
        success: true,
        full_output: String::new(),
        tokens_input: 0,
        tokens_output: 1,
            branch_path: String::new(),
        timestamp_ms: 1,
    });
    assert_eq!(events.len(), 0);
}

// ─── section 5 — FlowComplete emits final chunk with finish_reason: "stop" ─

#[test]
fn s5_flow_complete_emits_final_chunk_with_finish_reason_stop() {
    let mut adapter = select_adapter("openai", 0x1234);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "Chat".into(),
        backend: "gpt-4o".into(),
        timestamp_ms: 1,
    });
    let events = adapter.translate(&FlowExecutionEvent::FlowComplete {
        flow_name: "Chat".into(),
        backend: "gpt-4o".into(),
        success: true,
        steps_executed: 1,
        tokens_input: 0,
        tokens_output: 1,
        latency_ms: 10,
        timestamp_ms: 100,
    });
    assert_eq!(events.len(), 1);
    let data = event_data(&events[0]);
    assert!(
        data.contains("\"finish_reason\":\"stop\""),
        "33.z.k.e: FlowComplete frame MUST carry `finish_reason: \"stop\"` \
         per OpenAI spec Chat / Streaming / Final chunk. Got data: {data}"
    );
}

// ─── section 6 — flush_terminator emits axon_metadata + [DONE] sentinel ───

#[test]
fn s6_flush_terminator_emits_axon_metadata_then_done() {
    let mut adapter = select_adapter("openai", 0x1234);
    let terminator = adapter.flush_terminator();
    assert_eq!(
        terminator.len(),
        2,
        "33.z.k.e: flush_terminator() MUST emit exactly 2 frames: \
         (1) the axon_metadata side-channel frame, (2) the [DONE] sentinel"
    );
    let metadata_data = event_data(&terminator[0]);
    let done_data = event_data(&terminator[1]);
    assert!(
        metadata_data.contains("axon_metadata"),
        "33.z.k.e Q7: first terminator frame MUST contain the \
         `axon_metadata` extension key for algebraic-policy preservation. \
         Got data: {metadata_data}"
    );
    assert_eq!(
        done_data, "[DONE]",
        "33.z.k.e D5: second terminator frame MUST be the literal \
         `data: [DONE]` per OpenAI spec Chat / Streaming / Final \
         sentinel. Got data: {done_data:?}"
    );
}

// ─── section 7 — Tool-call emits delta with tool_calls[0].function ─────────

#[test]
fn s7_tool_call_emits_delta_with_tool_calls_function() {
    let mut adapter = select_adapter("openai", 0x1234);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "Chat".into(),
        backend: "gpt-4o".into(),
        timestamp_ms: 1,
    });
    let events = adapter.translate(&FlowExecutionEvent::ToolCall {
        step_name: "Generate".into(),
        tool_name: "search".into(),
        content: "{\"query\":\"axon\"}".into(),
        timestamp_ms: 100,
    });
    assert_eq!(events.len(), 1);
    let data = event_data(&events[0]);
    // Per OpenAI spec Tool calls in streaming:
    //   delta: { tool_calls: [{ index: 0, id: "...", type: "function",
    //                            function: { name: "...", arguments: "..." } }] }
    let must_contain = [
        "tool_calls",
        "\"index\":0",
        "\"type\":\"function\"",
        "\"name\":\"search\"",
        "\"arguments\":",
    ];
    for needle in must_contain {
        assert!(
            data.contains(needle),
            "33.z.k.e tool-call frame MUST contain `{needle}` per OpenAI \
             spec. Got data: {data}"
        );
    }
}

// ─── section 8 — Canonical-stub end-to-end frame ordering ─────────────────

#[test]
fn s8_canonical_stub_sequence_emits_3_frames_plus_2_terminator() {
    let mut adapter = select_adapter("openai", 0x1234);
    let mut all_events = Vec::new();
    for event in canonical_stub_event_sequence() {
        all_events.extend(adapter.translate(&event));
    }
    all_events.extend(adapter.flush_terminator());

    // Expected wire-frame sequence for canonical Step + stub:
    //   1. role-marker chunk (from FlowStart)
    //   2. content chunk "Hola" (from StepToken)
    //   3. final chunk with finish_reason: "stop" (from FlowComplete)
    //   4. axon_metadata frame (from flush_terminator)
    //   5. [DONE] sentinel (from flush_terminator)
    assert_eq!(
        all_events.len(),
        5,
        "33.z.k.e: canonical Step + stub backend emits exactly 5 \
         wire frames in openai dialect: role-marker + content + final-stop + \
         axon_metadata + [DONE]. Got {} frames.",
        all_events.len()
    );
}

// ─── section 9 — Response id stable across the stream ──────────────────────

#[test]
fn s9_response_id_stable_across_stream() {
    let mut adapter = select_adapter("openai", 0xDEAD_BEEF);
    let mut all_events = Vec::new();
    for event in canonical_stub_event_sequence() {
        all_events.extend(adapter.translate(&event));
    }
    // All emitted frames except [DONE] should carry the SAME id field.
    // Per OpenAI spec: "the id field is constant across all chunks
    // in a single response".
    let id_token = "chatcmpl-axon-deadbeef";
    for (i, event) in all_events.iter().enumerate() {
        let data = event_data(event);
        assert!(
            data.contains(id_token),
            "33.z.k.e frame {i}: response id MUST stay constant \
             across the stream. Expected to contain '{id_token}' but got: {data}"
        );
    }
}

// ─── v1.4.0 — Model identifier captured from FlowStart.backend ────────

#[test]
fn s10_model_captured_from_flow_start_backend() {
    let mut adapter = select_adapter("openai", 0x1);
    let events = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "Chat".into(),
        backend: "claude-3-5-sonnet-20241022".into(),
        timestamp_ms: 1,
    });
    let data = event_data(&events[0]);
    assert!(
        data.contains("\"model\":\"claude-3-5-sonnet-20241022\""),
        "33.z.k.e: model field MUST reflect the FlowStart's backend \
         identifier (per OpenAI spec, model is constant across the \
         response; we pin it once from FlowStart). Got data: {data}"
    );
}
