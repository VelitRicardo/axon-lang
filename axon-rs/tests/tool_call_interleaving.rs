//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.28.0 — Tool-call interleaving per dialect
//! (D11 wire-byte verification).
//!
//! D11 specifies that every dialect surfaces tool-call events per
//! its adopter-SDK ecosystem's expectation. The canonical agentic-AI
//! event stream — `StepToken*  ToolCall  StepToken*  StepComplete`
//! — must project onto each dialect's wire framing while preserving
//! the source's **arrival ordering** between text deltas and tool-
//! call frames. This is THE invariant adopters consuming streaming
//! LLM tool-use depend on.
//!
//! # Why a focused interleaving pack
//!
//! Sub-cycle 33.z.k.{d,e,f} unit-pinned each dialect's translation
//! of an INDIVIDUAL `FlowExecutionEvent` (one event in, N frames out).
//! That covers single-event byte-shape but doesn't pin the cross-
//! event **interleaving invariant** — what happens when a multi-step
//! agentic stream interleaves `StepToken` with `ToolCall` mid-step.
//! D11 explicitly calls out per-dialect interleaving as a closed-
//! catalog wire-shape decision; this pack pins it byte-exact.
//!
//! # Hermeticity note
//!
//! The production stub backend (`crate::backend::stub`) signals
//! `FinishReason::Stop` unconditionally — `FlowExecutionEvent::ToolCall`
//! is NEVER emitted on the hermetic test lane. So HTTP-level E2E for
//! tool-call interleaving requires a real upstream signalling ToolUse
//! (gated under the opt-in `AXON_RUN_REAL_PROVIDER_TEST` lane, 33.x.j
//! precedent). This pack tests interleaving at the **adapter** level
//! by driving each dialect with a synthetic event sequence directly —
//! same level + same helpers (`event_name` / `event_data`) as the
//! 33.z.k.{d,e,f} unit packs, but the input is a multi-event
//! interleaved stream instead of a single event in isolation.
//!
//! # Per-dialect tool-call surface (closed-catalog from D11)
//!
//! - **axon** → separate `event: axon.tool_call` frame. Interleaves
//!   with `event: axon.token` frames preserving arrival order
//!   verbatim. Each tool-call frame carries
//!   `{step, trace_id, tool_name, content, timestamp_ms}`.
//! - **openai** → inline `delta: {"tool_calls": [{...}]}` in a
//!   `chat.completion.chunk` frame; interleaves with content-delta
//!   chunks. Adopter SDKs (litellm, instructor, langchain) consume
//!   the `tool_calls` field per OpenAI's tool-call spec.
//! - **anthropic** → 3-frame burst per tool_use block:
//!   `content_block_start{type: tool_use}` +
//!   `content_block_delta{input_json_delta}` + `content_block_stop`.
//!   When a tool_use arrives MID-text-block, the open text block
//!   closes first (4-frame burst total).
//!
//! # Spec anchors
//!
//! Every assertion cites the relevant spec section in its docstring
//! so adopters reading the tests can verify against published refs:
//!
//! - OpenAI tool-calls in streaming:
//!   https://platform.openai.com/docs/guides/function-calling#streaming
//! - Anthropic tool-use streaming:
//!   https://docs.anthropic.com/en/docs/build-with-claude/tool-use#tool-use-in-streaming

use axon::flow_execution_event::FlowExecutionEvent;
use axon::wire_format::{select_adapter, WireFormatAdapter};

// ─── Helpers (mirror of 33.z.k.{d,e,f} pack helpers) ────────────────

/// Extract the `data:` payload from an axum Event via Debug
/// introspection. axum's Event type is opaque; this helper parses
/// the Debug representation
/// (`Event { buffer: Active(b"...") }`) and decodes Rust byte-string
/// literal escapes back into raw bytes.
///
/// Unlike the helpers in the axon/openai/anthropic dialect-adapter suites (which assume ASCII-only
/// payloads), this implementation correctly decodes `\xHH` byte
/// escapes so multibyte UTF-8 tokens (e.g. "Encontré") round-trip
/// faithfully. The producer emits UTF-8 verbatim; Debug formats it
/// as `\xc3\xa9` for non-ASCII bytes — we reverse the encoding.
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
    let raw_bytes = rest[..end].as_bytes();
    let mut buf: Vec<u8> = Vec::with_capacity(raw_bytes.len());
    let mut i = 0;
    while i < raw_bytes.len() {
        if raw_bytes[i] == b'\\' && i + 1 < raw_bytes.len() {
            match raw_bytes[i + 1] {
                b'n' => {
                    buf.push(b'\n');
                    i += 2;
                }
                b'r' => {
                    buf.push(b'\r');
                    i += 2;
                }
                b't' => {
                    buf.push(b'\t');
                    i += 2;
                }
                b'"' => {
                    buf.push(b'"');
                    i += 2;
                }
                b'\\' => {
                    buf.push(b'\\');
                    i += 2;
                }
                b'x' if i + 3 < raw_bytes.len() => {
                    // Decode 2 hex chars to a single byte.
                    let hex_bytes = &raw_bytes[i + 2..i + 4];
                    if let Ok(hex_str) = std::str::from_utf8(hex_bytes) {
                        if let Ok(byte_val) = u8::from_str_radix(hex_str, 16) {
                            buf.push(byte_val);
                            i += 4;
                            continue;
                        }
                    }
                    // Malformed escape — emit verbatim.
                    buf.push(raw_bytes[i]);
                    i += 1;
                }
                _ => {
                    buf.push(raw_bytes[i]);
                    i += 1;
                }
            }
        } else {
            buf.push(raw_bytes[i]);
            i += 1;
        }
    }
    let buf_str = String::from_utf8_lossy(&buf).to_string();
    let data_start = match buf_str.find("data: ") {
        Some(p) => p + "data: ".len(),
        None => return String::new(),
    };
    let data_rest = &buf_str[data_start..];
    let data_end = data_rest.find('\n').unwrap_or(data_rest.len());
    data_rest[..data_end].to_string()
}

/// Extract the `event:` name (empty string for unnamed frames such as
/// the OpenAI `data:`-only chunks).
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

/// Build the canonical agentic-AI interleaved event stream — the
/// load-bearing shape this pack pins across all 3 dialects:
///
/// ```text
///   FlowStart → StepStart → StepToken("Pensando...") →
///   ToolCall(search, {"query":"axon"}) → StepToken("Encontré ") →
///   StepToken("resultados.") → StepComplete → FlowComplete
/// ```
///
/// Adopters consuming streaming LLM tool-use see EXACTLY this shape
/// when an LLM emits reasoning text → invokes a tool → emits more
/// reasoning text after the tool result returns. The relative order
/// of the (text, tool-call, text) sequence is the invariant D11
/// pins.
fn canonical_interleaved_stream() -> Vec<FlowExecutionEvent> {
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
            content: "Pensando...".into(),
            token_index: 1,
            branch_path: String::new(),
            timestamp_ms: 1715648400002,
        },
        FlowExecutionEvent::ToolCall {
            step_name: "Generate".into(),
            tool_name: "search".into(),
            content: "{\"query\":\"axon\"}".into(),
            timestamp_ms: 1715648400003,
        },
        FlowExecutionEvent::StepToken {
            step_name: "Generate".into(),
            content: "Encontré ".into(),
            token_index: 2,
            branch_path: String::new(),
            timestamp_ms: 1715648400004,
        },
        FlowExecutionEvent::StepToken {
            step_name: "Generate".into(),
            content: "resultados.".into(),
            token_index: 3,
            branch_path: String::new(),
            timestamp_ms: 1715648400005,
        },
        FlowExecutionEvent::StepComplete {
            step_name: "Generate".into(),
            step_index: 0,
            success: true,
            full_output: "Pensando...Encontré resultados.".into(),
            tokens_input: 0,
            tokens_output: 3,
            branch_path: String::new(),
            timestamp_ms: 1715648400006,
        },
        FlowExecutionEvent::FlowComplete {
            flow_name: "Chat".into(),
            backend: "gpt-4o".into(),
            success: true,
            steps_executed: 1,
            tokens_input: 0,
            tokens_output: 3,
            latency_ms: 50,
            timestamp_ms: 1715648400007,
        },
    ]
}

/// Run an event stream through an adapter + collect ALL emitted
/// frames (including the dialect's terminator from `flush_terminator()`).
fn drive_adapter(
    adapter: &mut Box<dyn WireFormatAdapter>,
    events: &[FlowExecutionEvent],
) -> Vec<axum::response::sse::Event> {
    let mut frames = Vec::new();
    for event in events {
        frames.extend(adapter.translate(event));
    }
    frames.extend(adapter.flush_terminator());
    frames
}

// ════════════════════════════════════════════════════════════════════
// section 1 — Axon dialect: arrival-order interleaving with axon.tool_call
// ════════════════════════════════════════════════════════════════════

#[test]
fn s1_axon_interleaving_preserves_arrival_order() {
    let mut adapter = select_adapter("axon", 0x1234);
    let frames = drive_adapter(&mut adapter, &canonical_interleaved_stream());

    // Axon dialect: FlowStart / StepStart / StepComplete are silently
    // consumed; each StepToken → 1 axon.token; ToolCall → 1
    // axon.tool_call; FlowComplete → 1 axon.complete (terminator
    // in-line, flush_terminator is empty). For the canonical stream
    // above: 3 StepTokens + 1 ToolCall + 1 FlowComplete → 5 frames.
    assert_eq!(
        frames.len(),
        5,
        "33.z.k.g.3 D11 axon: 3 StepToken + 1 ToolCall + 1 FlowComplete \
         = 5 wire frames (flush_terminator empty). Got {}: {:?}",
        frames.len(),
        frames.iter().map(event_name).collect::<Vec<_>>()
    );

    // Arrival order (D11 invariant):
    //   frame 0: axon.token "Pensando..."
    //   frame 1: axon.tool_call search {"query":"axon"}
    //   frame 2: axon.token "Encontré "
    //   frame 3: axon.token "resultados."
    //   frame 4: axon.complete
    assert_eq!(event_name(&frames[0]), "axon.token");
    assert_eq!(event_name(&frames[1]), "axon.tool_call");
    assert_eq!(event_name(&frames[2]), "axon.token");
    assert_eq!(event_name(&frames[3]), "axon.token");
    assert_eq!(event_name(&frames[4]), "axon.complete");

    // Per-frame payload sanity: each frame carries the SOURCE token /
    // tool_name verbatim — no reordering, no buffering, no fusion.
    assert!(
        event_data(&frames[0]).contains("\"token\":\"Pensando...\""),
        "axon.token frame[0] MUST carry source token verbatim. Got: {}",
        event_data(&frames[0])
    );
    let tool_call_data = event_data(&frames[1]);
    assert!(
        tool_call_data.contains("\"tool_name\":\"search\""),
        "axon.tool_call frame MUST carry tool_name verbatim. Got: {tool_call_data}"
    );
    assert!(
        tool_call_data.contains("\"content\":\"{\\\"query\\\":\\\"axon\\\"}\""),
        "axon.tool_call frame MUST carry tool args verbatim (escaped). \
         Got: {tool_call_data}"
    );
    assert!(
        event_data(&frames[2]).contains("\"token\":\"Encontré \""),
        "axon.token frame[2] (post-tool) MUST carry second source token. \
         Got: {}",
        event_data(&frames[2])
    );
    assert!(
        event_data(&frames[3]).contains("\"token\":\"resultados.\""),
        "axon.token frame[3] (post-tool) MUST carry third source token. \
         Got: {}",
        event_data(&frames[3])
    );
}

#[test]
fn s1_axon_back_to_back_tool_calls_emit_distinct_frames() {
    // Two ToolCalls in sequence — adapter MUST emit two distinct
    // axon.tool_call frames with monotonic event IDs.
    let mut adapter = select_adapter("axon", 0x42);
    let stream = vec![
        FlowExecutionEvent::FlowStart {
            flow_name: "F".into(),
            backend: "b".into(),
            timestamp_ms: 1,
        },
        FlowExecutionEvent::ToolCall {
            step_name: "S".into(),
            tool_name: "t1".into(),
            content: "{}".into(),
            timestamp_ms: 2,
        },
        FlowExecutionEvent::ToolCall {
            step_name: "S".into(),
            tool_name: "t2".into(),
            content: "{}".into(),
            timestamp_ms: 3,
        },
        FlowExecutionEvent::FlowComplete {
            flow_name: "F".into(),
            backend: "b".into(),
            success: true,
            steps_executed: 1,
            tokens_input: 0,
            tokens_output: 0,
            latency_ms: 1,
            timestamp_ms: 4,
        },
    ];
    let frames = drive_adapter(&mut adapter, &stream);
    let names: Vec<String> = frames.iter().map(event_name).collect();
    assert_eq!(
        names,
        vec!["axon.tool_call", "axon.tool_call", "axon.complete"],
        "axon: back-to-back ToolCalls each get their own axon.tool_call \
         frame, in arrival order; got {names:?}"
    );
    // Per-call data carries the correct tool_name.
    assert!(event_data(&frames[0]).contains("\"tool_name\":\"t1\""));
    assert!(event_data(&frames[1]).contains("\"tool_name\":\"t2\""));
}

// ════════════════════════════════════════════════════════════════════
// section 2 — OpenAI dialect: inline tool_calls delta interleaved with
//        content deltas (Chat Completions tool-call streaming spec)
// ════════════════════════════════════════════════════════════════════

#[test]
fn s2_openai_interleaving_inlines_tool_calls_between_content_deltas() {
    let mut adapter = select_adapter("openai", 0x1234);
    let frames = drive_adapter(&mut adapter, &canonical_interleaved_stream());

    // OpenAI dialect translation of the canonical interleaved stream:
    //   frame 0: role-marker chunk (from FlowStart)
    //   frame 1: content-delta "Pensando..." (from StepToken 1)
    //   frame 2: tool_calls delta search/{"query":"axon"} (from ToolCall)
    //   frame 3: content-delta "Encontré " (from StepToken 2)
    //   frame 4: content-delta "resultados." (from StepToken 3)
    //   frame 5: final chunk finish_reason: "stop" (from FlowComplete)
    //   frame 6: axon_metadata side-channel (from flush_terminator)
    //   frame 7: [DONE] sentinel (from flush_terminator)
    // StepStart + StepComplete are silently consumed per dialect.
    assert_eq!(
        frames.len(),
        8,
        "33.z.k.g.3 D11 openai: canonical agentic stream → 8 wire frames \
         (role-marker + 3 content/tool-calls deltas + 1 content delta \
         + 1 content delta + finish chunk + metadata + [DONE]). Got {}.",
        frames.len()
    );

    let datas: Vec<String> = frames.iter().map(event_data).collect();

    // Frame 0 — role marker (OpenAI spec: first chunk's delta is
    // `{"role": "assistant"}`).
    assert!(
        datas[0].contains("\"delta\":{\"role\":\"assistant\"}"),
        "OpenAI spec Streaming: first chunk MUST carry role-marker. \
         Got: {}",
        datas[0]
    );

    // Frame 1 — content delta "Pensando...".
    assert!(
        datas[1].contains("\"delta\":{\"content\":\"Pensando...\"}"),
        "OpenAI spec: pre-tool text MUST flow on chunk.delta.content. \
         Got: {}",
        datas[1]
    );

    // Frame 2 — tool_calls delta (D11 inline-interleaving signature).
    // Per OpenAI tool-call streaming spec, the tool-use surfaces as
    // `delta: {"tool_calls": [{index, id, type: function, function:
    // {name, arguments}}]}` interleaved with content deltas.
    for needle in &[
        "\"tool_calls\":",
        "\"index\":0",
        "\"type\":\"function\"",
        "\"name\":\"search\"",
        "\"arguments\":",
    ] {
        assert!(
            datas[2].contains(needle),
            "33.z.k.g.3 D11 openai: tool_calls delta MUST contain `{needle}` \
             per OpenAI tool-call streaming spec. Got: {}",
            datas[2]
        );
    }
    // Tool args carried verbatim (escaped per JSON).
    assert!(
        datas[2].contains("\\\"query\\\":\\\"axon\\\""),
        "openai tool-calls delta MUST carry args verbatim. Got: {}",
        datas[2]
    );

    // Frames 3 + 4 — post-tool content deltas (D11 arrival-order
    // invariant: content frames AFTER tool_calls MUST appear after
    // the tool_calls delta in wire order).
    assert!(
        datas[3].contains("\"delta\":{\"content\":\"Encontré \"}"),
        "openai: post-tool text frame[3] MUST carry second source token. \
         Got: {}",
        datas[3]
    );
    assert!(
        datas[4].contains("\"delta\":{\"content\":\"resultados.\"}"),
        "openai: post-tool text frame[4] MUST carry third source token. \
         Got: {}",
        datas[4]
    );

    // Frame 5 — final chunk with finish_reason: "stop".
    assert!(
        datas[5].contains("\"finish_reason\":\"stop\""),
        "OpenAI spec: final chunk MUST carry finish_reason. Got: {}",
        datas[5]
    );

    // Frame 6 — Q7 axon_metadata side-channel (algebraic-policy data).
    assert!(
        datas[6].contains("\"axon_metadata\""),
        "Q7: axon_metadata frame MUST precede [DONE]. Got: {}",
        datas[6]
    );

    // Frame 7 — [DONE] sentinel (OpenAI spec terminator).
    assert_eq!(
        datas[7], "[DONE]",
        "OpenAI spec: stream MUST terminate with literal `[DONE]`. Got: {}",
        datas[7]
    );

    // D11 closed-catalog invariant: openai dialect MUST NOT emit
    // axon-named `event:` lines. Every frame is `data:`-only.
    for (i, frame) in frames.iter().enumerate() {
        assert_eq!(
            event_name(frame),
            "",
            "33.z.k.g.3 D11 openai: frame[{i}] MUST NOT carry `event:` \
             field (axon-named events are exclusive to axon dialect). \
             Got name: {}",
            event_name(frame)
        );
    }
}

#[test]
fn s2_openai_tool_call_id_synthesized_stable_within_request() {
    // Per OpenAI spec, each tool_calls[] entry carries `id` so
    // adopters can correlate tool results back to their requests.
    // The adapter synthesizes IDs from trace_id; back-to-back
    // ToolCalls in the same request get distinct stable IDs.
    let mut adapter = select_adapter("openai", 0xCAFE_BABE);
    let stream = vec![
        FlowExecutionEvent::FlowStart {
            flow_name: "F".into(),
            backend: "gpt-4o".into(),
            timestamp_ms: 1,
        },
        FlowExecutionEvent::ToolCall {
            step_name: "S".into(),
            tool_name: "t1".into(),
            content: "{}".into(),
            timestamp_ms: 2,
        },
        FlowExecutionEvent::ToolCall {
            step_name: "S".into(),
            tool_name: "t2".into(),
            content: "{}".into(),
            timestamp_ms: 3,
        },
    ];
    let frames = drive_adapter(&mut adapter, &stream);
    // frames: [role-marker, tool_calls(t1), tool_calls(t2), axon_metadata, [DONE]]
    // (no FlowComplete in stream → no finish_reason chunk, only flush_terminator runs).
    let datas: Vec<String> = frames.iter().map(event_data).collect();
    // First tool_call frame at index 1, second at index 2.
    // Adapter synthesizes IDs as `call_<trace_id_hex>_<counter>`.
    assert!(
        datas[1].contains("\"id\":\"call_cafebabe_1\""),
        "openai: first ToolCall id MUST be `call_<trace_hex>_1`. Got: {}",
        datas[1]
    );
    assert!(
        datas[2].contains("\"id\":\"call_cafebabe_2\""),
        "openai: second ToolCall id MUST be `call_<trace_hex>_2` (monotonic \
         counter). Got: {}",
        datas[2]
    );
    assert!(
        datas[1].contains("\"name\":\"t1\""),
        "openai: first tool_calls entry MUST carry t1's name. Got: {}",
        datas[1]
    );
    assert!(
        datas[2].contains("\"name\":\"t2\""),
        "openai: second tool_calls entry MUST carry t2's name. Got: {}",
        datas[2]
    );
}

// ════════════════════════════════════════════════════════════════════
// section 3 — Anthropic dialect: tool_use block triad interleaved with
//        text_block_delta (Messages tool-use streaming spec)
// ════════════════════════════════════════════════════════════════════

#[test]
fn s3_anthropic_interleaving_closes_text_block_around_tool_use() {
    let mut adapter = select_adapter("anthropic", 0x1234);
    let frames = drive_adapter(&mut adapter, &canonical_interleaved_stream());

    // Anthropic dialect translation of the canonical stream:
    //   1. message_start (FlowStart)
    //   2. content_block_start (text, index 0) — lazy-opened on
    //      first StepToken
    //   3. content_block_delta (text_delta "Pensando...")
    //   4. content_block_stop (text, index 0) — ToolCall arrives
    //      mid-text-block, must close it first
    //   5. content_block_start (tool_use, index 1)
    //   6. content_block_delta (input_json_delta)
    //   7. content_block_stop (tool_use, index 1)
    //   8. content_block_start (text, index 2) — second StepToken
    //      arrives, opens a new text block
    //   9. content_block_delta (text_delta "Encontré ")
    //   10. content_block_delta (text_delta "resultados.")
    //   11. content_block_stop (text, index 2) — StepComplete closes
    //   12. message_delta (FlowComplete; stop_reason)
    //   13. axon.metadata (flush_terminator Q7)
    //   14. message_stop (flush_terminator)
    assert_eq!(
        frames.len(),
        14,
        "33.z.k.g.3 D11 anthropic: canonical agentic stream → 14 wire \
         frames (message_start + text-block-around-tool-use 7-frame \
         span + reopened text block + message_delta + Q7 metadata + \
         message_stop). Got {}: {:?}",
        frames.len(),
        frames.iter().map(event_name).collect::<Vec<_>>()
    );

    let names: Vec<String> = frames.iter().map(event_name).collect();
    assert_eq!(names[0], "message_start");
    assert_eq!(names[1], "content_block_start");
    assert_eq!(names[2], "content_block_delta");
    assert_eq!(names[3], "content_block_stop");
    assert_eq!(names[4], "content_block_start");
    assert_eq!(names[5], "content_block_delta");
    assert_eq!(names[6], "content_block_stop");
    assert_eq!(names[7], "content_block_start");
    assert_eq!(names[8], "content_block_delta");
    assert_eq!(names[9], "content_block_delta");
    assert_eq!(names[10], "content_block_stop");
    assert_eq!(names[11], "message_delta");
    assert_eq!(names[12], "axon.metadata");
    assert_eq!(names[13], "message_stop");

    let datas: Vec<String> = frames.iter().map(event_data).collect();

    // First text block (frames 1-3) carries "Pensando...".
    assert!(
        datas[1].contains("\"type\":\"text\""),
        "Anthropic spec: first content_block_start MUST carry text type. \
         Got: {}",
        datas[1]
    );
    assert!(
        datas[2].contains("\"type\":\"text_delta\""),
        "Anthropic spec: text content delta MUST carry text_delta type. \
         Got: {}",
        datas[2]
    );
    assert!(
        datas[2].contains("\"text\":\"Pensando...\""),
        "Anthropic: pre-tool text MUST flow on first text_delta. Got: {}",
        datas[2]
    );

    // Tool-use block (frames 4-6) — D11 signature for anthropic.
    assert!(
        datas[4].contains("\"type\":\"tool_use\""),
        "Anthropic spec: tool_use block opens with type: tool_use. Got: {}",
        datas[4]
    );
    assert!(
        datas[4].contains("\"name\":\"search\""),
        "Anthropic: tool_use block MUST carry tool name. Got: {}",
        datas[4]
    );
    assert!(
        datas[5].contains("\"type\":\"input_json_delta\""),
        "Anthropic spec: tool_use delta MUST be input_json_delta. Got: {}",
        datas[5]
    );
    assert!(
        datas[5].contains("\"partial_json\""),
        "Anthropic spec: input_json_delta MUST carry partial_json field. \
         Got: {}",
        datas[5]
    );

    // Second text block (frames 7-10) — post-tool reasoning text
    // reopens a NEW text block (D11 arrival-order invariant).
    assert!(
        datas[7].contains("\"type\":\"text\""),
        "Anthropic: post-tool text reopens a fresh text block. Got: {}",
        datas[7]
    );
    assert!(
        datas[8].contains("\"text\":\"Encontré \""),
        "Anthropic: post-tool text frame[8] MUST carry second source token. \
         Got: {}",
        datas[8]
    );
    assert!(
        datas[9].contains("\"text\":\"resultados.\""),
        "Anthropic: post-tool text frame[9] MUST carry third source token. \
         Got: {}",
        datas[9]
    );

    // Q7 axon.metadata between message_delta and message_stop.
    let metadata_pos = names.iter().position(|n| n == "axon.metadata").unwrap();
    let stop_pos = names.iter().position(|n| n == "message_stop").unwrap();
    assert!(
        metadata_pos < stop_pos,
        "Q7: axon.metadata frame MUST precede message_stop terminator. \
         Got positions metadata={metadata_pos} stop={stop_pos}"
    );

    // D11 closed-catalog invariant: anthropic dialect MUST NOT emit
    // openai sentinels or axon-named non-metadata events.
    for (i, frame) in frames.iter().enumerate() {
        let name = event_name(frame);
        let data = event_data(frame);
        assert!(
            !data.contains("\"object\":\"chat.completion.chunk\""),
            "anthropic frame[{i}] ({name}) MUST NOT carry openai chunk \
             shape. Got: {data}"
        );
        assert_ne!(
            data, "[DONE]",
            "anthropic frame[{i}] ({name}) MUST NOT be `[DONE]` sentinel."
        );
        assert_ne!(
            name, "axon.token",
            "anthropic frame[{i}] MUST NOT carry axon-W3C tokens."
        );
        assert_ne!(
            name, "axon.tool_call",
            "anthropic frame[{i}] MUST NOT carry axon-W3C tool_call frames."
        );
    }
}

#[test]
fn s3_anthropic_block_indices_advance_monotonically_across_tool_use() {
    // Anthropic spec: every content_block has a numeric `index` that
    // advances monotonically. Across the canonical interleaved stream,
    // indices should be 0 (text) → 1 (tool_use) → 2 (text).
    let mut adapter = select_adapter("anthropic", 0x1234);
    let frames = drive_adapter(&mut adapter, &canonical_interleaved_stream());

    let mut observed_indices = Vec::new();
    for frame in &frames {
        let name = event_name(frame);
        if name == "content_block_start" || name == "content_block_stop" {
            let data = event_data(frame);
            // Extract index field by JSON parse.
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(idx) = v.get("index").and_then(|x| x.as_u64()) {
                    observed_indices.push(idx);
                }
            }
        }
    }
    // Sequence: text_start(0) text_stop(0) tool_use_start(1)
    // tool_use_stop(1) text_start(2) text_stop(2)
    assert_eq!(
        observed_indices,
        vec![0, 0, 1, 1, 2, 2],
        "33.z.k.g.3 D11 anthropic: content_block indices MUST advance \
         monotonically across text/tool_use/text boundaries per spec. \
         Got: {observed_indices:?}"
    );
}

#[test]
fn s3_anthropic_back_to_back_tool_calls_emit_distinct_blocks() {
    // Two ToolCalls in sequence (no text between) → 2 tool_use 3-frame
    // triads (no intervening text_block_stop because no text block
    // is open). Block indices advance monotonically.
    let mut adapter = select_adapter("anthropic", 0x42);
    let stream = vec![
        FlowExecutionEvent::FlowStart {
            flow_name: "F".into(),
            backend: "claude".into(),
            timestamp_ms: 1,
        },
        FlowExecutionEvent::ToolCall {
            step_name: "S".into(),
            tool_name: "t1".into(),
            content: "{}".into(),
            timestamp_ms: 2,
        },
        FlowExecutionEvent::ToolCall {
            step_name: "S".into(),
            tool_name: "t2".into(),
            content: "{}".into(),
            timestamp_ms: 3,
        },
        FlowExecutionEvent::FlowComplete {
            flow_name: "F".into(),
            backend: "claude".into(),
            success: true,
            steps_executed: 1,
            tokens_input: 0,
            tokens_output: 0,
            latency_ms: 1,
            timestamp_ms: 4,
        },
    ];
    let frames = drive_adapter(&mut adapter, &stream);
    let names: Vec<String> = frames.iter().map(event_name).collect();
    // Expected: message_start, [tool_use triad x2], message_delta,
    // axon.metadata, message_stop = 1 + 6 + 3 = 10 frames.
    assert_eq!(
        names,
        vec![
            "message_start",
            "content_block_start",
            "content_block_delta",
            "content_block_stop",
            "content_block_start",
            "content_block_delta",
            "content_block_stop",
            "message_delta",
            "axon.metadata",
            "message_stop",
        ],
        "anthropic: back-to-back ToolCalls each get their own 3-frame \
         tool_use triad. Got names: {names:?}"
    );
    let datas: Vec<String> = frames.iter().map(event_data).collect();
    assert!(datas[1].contains("\"name\":\"t1\""));
    assert!(datas[4].contains("\"name\":\"t2\""));
}

// ════════════════════════════════════════════════════════════════════
// section 4 — Cross-dialect arrival-order semantic equivalence (D11 + D3)
// ════════════════════════════════════════════════════════════════════

#[test]
fn s4_arrival_order_invariant_holds_across_all_three_dialects() {
    // The relative order of source events (text → tool → text) MUST
    // project onto every dialect's wire — same arrival ordering, just
    // different framing. This is the D3 semantic-equivalence invariant.
    let stream = canonical_interleaved_stream();

    // Helper: from a sequence of frames, derive a closed-vocabulary
    // arrival sequence of just (text-token | tool-call) markers.
    fn arrival_signature(frames: &[axum::response::sse::Event]) -> Vec<&'static str> {
        let mut sig = Vec::new();
        for frame in frames {
            let name = event_name(frame);
            let data = event_data(frame);
            // Axon dialect markers.
            if name == "axon.token" {
                sig.push("T");
            } else if name == "axon.tool_call" {
                sig.push("X");
            // OpenAI dialect markers (no `event:` line → name="").
            } else if name.is_empty() {
                if data.contains("\"delta\":{\"content\":") {
                    sig.push("T");
                } else if data.contains("\"tool_calls\":") {
                    sig.push("X");
                }
            // Anthropic dialect markers.
            } else if name == "content_block_delta" {
                if data.contains("\"type\":\"text_delta\"") {
                    sig.push("T");
                } else if data.contains("\"type\":\"input_json_delta\"") {
                    sig.push("X");
                }
            }
        }
        sig
    }

    let mut axon_adapter = select_adapter("axon", 0x10);
    let mut openai_adapter = select_adapter("openai", 0x10);
    let mut anthropic_adapter = select_adapter("anthropic", 0x10);

    let axon_frames = drive_adapter(&mut axon_adapter, &stream);
    let openai_frames = drive_adapter(&mut openai_adapter, &stream);
    let anthropic_frames = drive_adapter(&mut anthropic_adapter, &stream);

    let axon_sig = arrival_signature(&axon_frames);
    let openai_sig = arrival_signature(&openai_frames);
    let anthropic_sig = arrival_signature(&anthropic_frames);

    // The canonical stream contains 3 text deltas + 1 tool call in
    // order: T X T T. Every dialect MUST preserve this signature.
    let expected = vec!["T", "X", "T", "T"];
    assert_eq!(
        axon_sig, expected,
        "D11 + D3: axon dialect MUST preserve arrival order T-X-T-T. \
         Got: {axon_sig:?}"
    );
    assert_eq!(
        openai_sig, expected,
        "D11 + D3: openai dialect MUST preserve arrival order T-X-T-T. \
         Got: {openai_sig:?}"
    );
    assert_eq!(
        anthropic_sig, expected,
        "D11 + D3: anthropic dialect MUST preserve arrival order T-X-T-T \
         (modulo block-start/stop bookkeeping). Got: {anthropic_sig:?}"
    );
}

// ════════════════════════════════════════════════════════════════════
// section 5 — Kimi + GLM dialect dispatch (Q3 revision): tool-call wire
//        is byte-identical to canonical openai
// ════════════════════════════════════════════════════════════════════

#[test]
fn s5_kimi_glm_tool_call_wire_byte_identical_to_openai() {
    // Per Q3 revision 2026-05-14, `kimi` + `glm` dispatch to the
    // OpenAIDialectAdapter — adopter SDKs for Moonshot Kimi K2.x +
    // Zhipu ChatGLM-4.x consume OpenAI-compatible Chat Completions
    // wire verbatim. Tool-call interleaving MUST be byte-identical
    // across openai / kimi / glm dialect declarations.
    let stream = canonical_interleaved_stream();

    let mut openai_adapter = select_adapter("openai", 0x20);
    let mut kimi_adapter = select_adapter("kimi", 0x20);
    let mut glm_adapter = select_adapter("glm", 0x20);

    let openai_frames = drive_adapter(&mut openai_adapter, &stream);
    let kimi_frames = drive_adapter(&mut kimi_adapter, &stream);
    let glm_frames = drive_adapter(&mut glm_adapter, &stream);

    // Frame counts identical.
    assert_eq!(
        openai_frames.len(),
        kimi_frames.len(),
        "Q3 + D11: kimi tool-call frame count MUST match openai."
    );
    assert_eq!(
        openai_frames.len(),
        glm_frames.len(),
        "Q3 + D11: glm tool-call frame count MUST match openai."
    );

    // Per-frame data identical (modulo `created` timestamp which
    // is reset per-adapter; strip before compare).
    fn strip_created(data: &str) -> String {
        if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(data) {
            if let Some(obj) = v.as_object_mut() {
                obj.remove("created");
            }
            return serde_json::to_string(&v).unwrap_or_default();
        }
        data.to_string()
    }

    for (i, (openai_f, kimi_f)) in openai_frames.iter().zip(kimi_frames.iter()).enumerate() {
        let openai_data = strip_created(&event_data(openai_f));
        let kimi_data = strip_created(&event_data(kimi_f));
        assert_eq!(
            openai_data, kimi_data,
            "Q3 + D11: kimi frame[{i}] data MUST be byte-identical to \
             openai frame[{i}] (both dispatch to OpenAIDialectAdapter)."
        );
    }
    for (i, (openai_f, glm_f)) in openai_frames.iter().zip(glm_frames.iter()).enumerate() {
        let openai_data = strip_created(&event_data(openai_f));
        let glm_data = strip_created(&event_data(glm_f));
        assert_eq!(
            openai_data, glm_data,
            "Q3 + D11: glm frame[{i}] data MUST be byte-identical to \
             openai frame[{i}] (both dispatch to OpenAIDialectAdapter)."
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// section 6 — Closed-catalog mutex: no dialect's tool-call surface leaks
//        into another dialect's wire (D11 final invariant)
// ════════════════════════════════════════════════════════════════════

#[test]
fn s6_closed_catalog_mutex_no_cross_dialect_tool_call_leak() {
    let stream = canonical_interleaved_stream();

    // Axon dialect: `event: axon.tool_call` is the EXCLUSIVE signature.
    let mut axon_adapter = select_adapter("axon", 0x30);
    let axon_frames = drive_adapter(&mut axon_adapter, &stream);
    for (i, frame) in axon_frames.iter().enumerate() {
        let data = event_data(frame);
        assert!(
            !data.contains("\"tool_calls\":["),
            "33.z.k.g.3 D11: axon dialect frame[{i}] MUST NOT carry \
             openai-shape `tool_calls` field. Got: {data}"
        );
        assert!(
            !data.contains("\"type\":\"tool_use\""),
            "33.z.k.g.3 D11: axon dialect frame[{i}] MUST NOT carry \
             anthropic-shape `type: tool_use` field. Got: {data}"
        );
    }

    // OpenAI dialect: `delta.tool_calls[...]` is the EXCLUSIVE signature.
    let mut openai_adapter = select_adapter("openai", 0x30);
    let openai_frames = drive_adapter(&mut openai_adapter, &stream);
    for (i, frame) in openai_frames.iter().enumerate() {
        assert_ne!(
            event_name(frame),
            "axon.tool_call",
            "33.z.k.g.3 D11: openai dialect frame[{i}] MUST NOT carry \
             axon-W3C `event: axon.tool_call` line."
        );
        let data = event_data(frame);
        assert!(
            !data.contains("\"type\":\"tool_use\""),
            "33.z.k.g.3 D11: openai dialect frame[{i}] MUST NOT carry \
             anthropic-shape `type: tool_use` field. Got: {data}"
        );
    }

    // Anthropic dialect: `content_block_start{type: tool_use}` triad
    // is the EXCLUSIVE signature.
    let mut anthropic_adapter = select_adapter("anthropic", 0x30);
    let anthropic_frames = drive_adapter(&mut anthropic_adapter, &stream);
    for (i, frame) in anthropic_frames.iter().enumerate() {
        assert_ne!(
            event_name(frame),
            "axon.tool_call",
            "33.z.k.g.3 D11: anthropic dialect frame[{i}] MUST NOT carry \
             axon-W3C `event: axon.tool_call` line."
        );
        let data = event_data(frame);
        assert!(
            !data.contains("\"tool_calls\":["),
            "33.z.k.g.3 D11: anthropic dialect frame[{i}] MUST NOT carry \
             openai-shape `tool_calls` array. Got: {data}"
        );
    }
}
