//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.28.0 — Algebraic-policy preservation channel
//! populated end-to-end (D4 wire-byte verification).
//!
//! Pre-33.z.k.h, the openai dialect's `axon_metadata` frame + the
//! anthropic dialect's `event: axon.metadata` frame were emitted as
//! **empty placeholders** — adopters consuming those wires saw
//! `enforcement_summary: {}`, `runtime_warnings: []`, `step_audit: []`
//! regardless of how the flow ran. The closed-catalog wire SHAPE was
//! in place but the algebraic-policy DATA was missing.
//!
//! Sub-cycle 33.z.k.h closes the gap end-to-end:
//!
//! 1. `CompleteEnvelope` gains a `step_audit_records: Vec<StepAuditRecord>`
//!    field carrying the per-step audit rows the dispatcher populates
//! (v1.24.0). The producer reads `step_audit_records_for_consumer`
//!    UNCONDITIONALLY (was previously gated behind `replay_ctx.is_some()`)
//!    so the envelope always has the data.
//!
//! 2. `OpenAIDialectAdapter` + `AnthropicDialectAdapter` both override
//!    `build_complete_envelope_event` to stash the envelope in adapter
//!    state, then `flush_terminator()` reads the stash + emits a
//!    POPULATED metadata frame with the full envelope: trace_id, flow,
//!    backend, success, counters, latency, stream_policies,
//!    enforcement_summary, runtime_warnings, step_audit + a
//!    `terminal_reason` discriminator.
//!
//! 3. Empty fields are elided per the same D4 byte-compat pattern
//!    the axon adapter uses for `axon.complete`.
//!
//! 4. Adopters on the openai / anthropic wires now have a uniform
//!    surface for the vertical-regulator audit requirements they
//!    couldn't otherwise satisfy on those dialects:
//!      - Banking PCI DSS Req 10 — per-step LLM call provenance
//!      - Government FedRAMP AU-2 — FOIA per-step reasoning chain
//!      - Legal FRE 502 waiver-doctrine — per-step privilege check
//! - Medicine 21 CFR Part 11 section 11.10 — CDS clinician trail
//!
//! # What this pack pins
//!
//! ## Unit-level (direct adapter invocation)
//!
//! section 1 — OpenAI: build_complete_envelope_event stashes envelope +
//!      flush_terminator emits populated axon_metadata frame.
//! section 2 — Anthropic: same surface; `type: axon.metadata` wrapper.
//! section 3 — Empty-fields elision: a minimal envelope (only flow-level
//!      fields, no algebraic-policy data) emits a frame WITHOUT the
//!      optional keys (D4 byte-compat).
//! section 4 — Default path (translate(FlowComplete) without envelope):
//!      adapters MUST emit the metadata frame with terminal_reason
//!      only — flow-level fields absent. Defensive backwards-compat.
//! section 5 — terminal_reason discriminator: stop / error / none across
//!      every reachable path.
//!
//! ## E2E HTTP-level
//!
//! section 6 — Real algebraic-effect Kivi-shape POST → openai wire surfaces
//!      populated enforcement_summary on axon_metadata frame.
//! section 7 — Same flow via `transport: sse(anthropic)` → anthropic wire
//!      surfaces populated enforcement_summary on axon.metadata frame.
//! section 8 — Cross-dialect parity: openai + anthropic surface SAME
//!      enforcement_summary data modulo framing (D3 + D4).

use axon::axon_server::EnforcementSummaryWire;
use axon::axonendpoint_replay::StepAuditRecord;
use axon::flow_execution_event::FlowExecutionEvent;
use axon::runtime_warnings::{FallbackMode, RuntimeWarning};
use axon::wire_format::{select_adapter, CompleteEnvelope};

// ─── Helpers ────────────────────────────────────────────────────────

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
                    let hex_bytes = &raw_bytes[i + 2..i + 4];
                    if let Ok(hex_str) = std::str::from_utf8(hex_bytes) {
                        if let Ok(byte_val) = u8::from_str_radix(hex_str, 16) {
                            buf.push(byte_val);
                            i += 4;
                            continue;
                        }
                    }
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

/// Build a canonical full envelope with every algebraic-policy field
/// populated so we can assert the wire frame projects every field.
fn full_envelope_fixture() -> CompleteEnvelope {
    CompleteEnvelope {
        trace_id: 0xDEAD_BEEF,
        flow_name: "Chat".into(),
        backend: "gpt-4o".into(),
        success: true,
        steps_executed: 2,
        tokens_input: 42,
        tokens_output: 73,
        latency_ms: 1234,
        effect_policies: vec![
            ("Generate".into(), "drop_oldest".into()),
            ("Audit".into(), "fail".into()),
        ],
        enforcement_summaries: vec![(
            "Generate".into(),
            EnforcementSummaryWire {
                policy_slug: "drop_oldest".into(),
                chunks_pushed: 7,
                chunks_delivered: 5,
                drop_oldest_hits: 2,
                degrade_quality_hits: 0,
                pause_upstream_blocks: 0,
                fail_overflows: 0,
                failed: false,
            },
        )],
        runtime_warnings: vec![RuntimeWarning::streaming_not_supported(
            "Chat",
            "stub",
            FallbackMode::BackendLacksStream,
            "stub backend has no native streaming",
        )],
        step_audit_records: vec![StepAuditRecord {
            step_name: "Generate".into(),
            step_index: 0,
            success: true,
            tokens_emitted: 5,
            output_hash_hex: "0123abcd".repeat(8),
            effect_policy_applied: Some("drop_oldest".into()),
            chunks_dropped: 2,
            chunks_degraded: 0,
            timestamp_ms: 1715648400500,
            // v1.29.0 — tool-stream provenance fields stay None
            // for legacy LLM-side construction; serde elides them
            // (D4 byte-compat with the pre-34.i wire shape).
            ..Default::default()
        }],
        epistemic_envelopes: Vec::new(),
        temporal_context: None,
    }
}

/// Minimal envelope: every algebraic-policy field empty. Used to
/// exercise the elision path.
fn minimal_envelope_fixture() -> CompleteEnvelope {
    CompleteEnvelope {
        trace_id: 0x42,
        flow_name: "F".into(),
        backend: "b".into(),
        success: true,
        steps_executed: 0,
        tokens_input: 0,
        tokens_output: 0,
        latency_ms: 0,
        effect_policies: Vec::new(),
        enforcement_summaries: Vec::new(),
        runtime_warnings: Vec::new(),
        step_audit_records: Vec::new(),
        epistemic_envelopes: Vec::new(),
        temporal_context: None,
    }
}

/// Extract + parse the axon_metadata payload from an openai
/// dialect's terminator frame[0] (the axon_metadata frame; frame[1]
/// is `[DONE]`).
fn parse_openai_metadata(adapter_frame_data: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(adapter_frame_data)
        .expect("openai metadata frame data MUST be valid JSON");
    v.get("axon_metadata")
        .cloned()
        .expect("openai metadata frame MUST have top-level `axon_metadata` key")
}

/// Extract + parse the axon_metadata payload from an anthropic
/// dialect's terminator frame[0] (the axon.metadata frame; frame[1]
/// is message_stop).
fn parse_anthropic_metadata(adapter_frame_data: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(adapter_frame_data)
        .expect("anthropic metadata frame data MUST be valid JSON");
    assert_eq!(
        v.get("type").and_then(|t| t.as_str()),
        Some("axon.metadata"),
        "anthropic metadata frame MUST carry `type: axon.metadata`"
    );
    v.get("axon_metadata")
        .cloned()
        .expect("anthropic metadata frame MUST have `axon_metadata` key")
}

// ════════════════════════════════════════════════════════════════════
// section 1 — OpenAI: populated axon_metadata frame end-to-end
// ════════════════════════════════════════════════════════════════════

#[test]
fn s1_openai_envelope_event_populates_axon_metadata_frame_fully() {
    let mut adapter = select_adapter("openai", 0xDEAD_BEEF);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "Chat".into(),
        backend: "gpt-4o".into(),
        timestamp_ms: 1,
    });
    let _ = adapter.translate(&FlowExecutionEvent::StepToken {
        step_name: "Generate".into(),
        content: "Hi".into(),
        token_index: 1,
            branch_path: String::new(),
        timestamp_ms: 2,
    });
    let envelope = full_envelope_fixture();
    // build_complete_envelope_event stashes the envelope + emits the
    // final chunk frame.
    let complete_frames = adapter.build_complete_envelope_event(&envelope);
    assert_eq!(
        complete_frames.len(),
        1,
        "openai: build_complete_envelope_event MUST emit exactly 1 \
         chunk frame (final chunk with finish_reason: stop). Got {}",
        complete_frames.len()
    );
    // flush_terminator emits 2 frames: axon_metadata + [DONE].
    let terminator = adapter.flush_terminator();
    assert_eq!(
        terminator.len(),
        2,
        "openai: flush_terminator MUST emit exactly 2 frames \
         (axon_metadata + [DONE]). Got {}",
        terminator.len()
    );
    let metadata_data = event_data(&terminator[0]);
    let metadata = parse_openai_metadata(&metadata_data);

    // Every envelope field projects onto the metadata payload.
    assert_eq!(metadata["trace_id"], serde_json::json!(0xDEAD_BEEFu64));
    assert_eq!(metadata["flow"], serde_json::json!("Chat"));
    assert_eq!(metadata["backend"], serde_json::json!("gpt-4o"));
    assert_eq!(metadata["success"], serde_json::json!(true));
    assert_eq!(metadata["steps_executed"], serde_json::json!(2));
    assert_eq!(metadata["tokens_input"], serde_json::json!(42));
    assert_eq!(metadata["tokens_output"], serde_json::json!(73));
    assert_eq!(metadata["latency_ms"], serde_json::json!(1234));
    assert_eq!(metadata["terminal_reason"], serde_json::json!("stop"));

    // stream_policies array.
    let policies = metadata["stream_policies"]
        .as_array()
        .expect("stream_policies MUST be an array");
    assert_eq!(policies.len(), 2);
    assert_eq!(policies[0]["step"], "Generate");
    assert_eq!(policies[0]["policy"], "drop_oldest");
    assert_eq!(policies[1]["step"], "Audit");
    assert_eq!(policies[1]["policy"], "fail");

    // enforcement_summary keyed by step name.
    let summary = metadata["enforcement_summary"]["Generate"]
        .as_object()
        .expect("enforcement_summary.Generate MUST be an object");
    assert_eq!(summary["policy_slug"], "drop_oldest");
    assert_eq!(summary["chunks_pushed"], 7);
    assert_eq!(summary["chunks_delivered"], 5);
    assert_eq!(summary["drop_oldest_hits"], 2);
    assert_eq!(summary["failed"], false);

    // runtime_warnings array.
    let warnings = metadata["runtime_warnings"]
        .as_array()
        .expect("runtime_warnings MUST be an array");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["code"], "axon-W002");

    // step_audit array.
    let audit = metadata["step_audit"]
        .as_array()
        .expect("step_audit MUST be an array");
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0]["step_name"], "Generate");
    assert_eq!(audit[0]["step_index"], 0);
    assert_eq!(audit[0]["tokens_emitted"], 5);
    assert_eq!(audit[0]["effect_policy_applied"], "drop_oldest");
    assert_eq!(audit[0]["chunks_dropped"], 2);

    // [DONE] sentinel.
    assert_eq!(event_data(&terminator[1]), "[DONE]");
}

// ════════════════════════════════════════════════════════════════════
// section 2 — Anthropic: populated axon.metadata frame end-to-end
// ════════════════════════════════════════════════════════════════════

#[test]
fn s2_anthropic_envelope_event_populates_axon_metadata_frame_fully() {
    let mut adapter = select_adapter("anthropic", 0xCAFE_BABE);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "Chat".into(),
        backend: "claude-3-5-sonnet".into(),
        timestamp_ms: 1,
    });
    let _ = adapter.translate(&FlowExecutionEvent::StepToken {
        step_name: "Generate".into(),
        content: "Hi".into(),
        token_index: 1,
            branch_path: String::new(),
        timestamp_ms: 2,
    });
    let envelope = full_envelope_fixture();
    let complete_frames = adapter.build_complete_envelope_event(&envelope);
    // anthropic emits: content_block_stop (closes open text block) +
    // message_delta = 2 frames.
    assert_eq!(
        complete_frames.len(),
        2,
        "anthropic: build_complete_envelope_event with open text \
         block MUST emit content_block_stop + message_delta = 2 frames. \
         Got {}",
        complete_frames.len()
    );
    let terminator = adapter.flush_terminator();
    assert_eq!(
        terminator.len(),
        2,
        "anthropic: flush_terminator MUST emit 2 frames \
         (axon.metadata + message_stop). Got {}",
        terminator.len()
    );
    let metadata_data = event_data(&terminator[0]);
    let metadata = parse_anthropic_metadata(&metadata_data);

    // Same envelope projection invariants as openai (D3 + Q7).
    assert_eq!(metadata["trace_id"], serde_json::json!(0xDEAD_BEEFu64));
    assert_eq!(metadata["flow"], serde_json::json!("Chat"));
    assert_eq!(metadata["backend"], serde_json::json!("gpt-4o"));
    assert_eq!(metadata["success"], true);
    assert_eq!(metadata["steps_executed"], 2);
    assert_eq!(metadata["tokens_input"], 42);
    assert_eq!(metadata["tokens_output"], 73);
    assert_eq!(metadata["latency_ms"], 1234);
    assert_eq!(metadata["terminal_reason"], "stop");
    assert_eq!(metadata["stream_policies"].as_array().unwrap().len(), 2);
    assert_eq!(
        metadata["enforcement_summary"]["Generate"]["policy_slug"],
        "drop_oldest"
    );
    assert_eq!(metadata["runtime_warnings"].as_array().unwrap().len(), 1);
    assert_eq!(metadata["step_audit"].as_array().unwrap().len(), 1);
}

// ════════════════════════════════════════════════════════════════════
// section 3 — Empty-field elision (D4 byte-compat with placeholder shape)
// ════════════════════════════════════════════════════════════════════

#[test]
fn s3_openai_minimal_envelope_elides_optional_fields() {
    let mut adapter = select_adapter("openai", 0x42);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "F".into(),
        backend: "b".into(),
        timestamp_ms: 1,
    });
    let _ = adapter.build_complete_envelope_event(&minimal_envelope_fixture());
    let terminator = adapter.flush_terminator();
    let metadata = parse_openai_metadata(&event_data(&terminator[0]));

    // Required flow-level fields present.
    assert!(metadata.get("trace_id").is_some());
    assert!(metadata.get("flow").is_some());
    assert!(metadata.get("terminal_reason").is_some());
    // Optional algebraic-policy fields ELIDED when empty.
    assert!(
        metadata.get("stream_policies").is_none(),
        "empty stream_policies MUST be elided (D4 byte-compat). Got: {metadata}"
    );
    assert!(
        metadata.get("enforcement_summary").is_none(),
        "empty enforcement_summary MUST be elided. Got: {metadata}"
    );
    assert!(
        metadata.get("runtime_warnings").is_none(),
        "empty runtime_warnings MUST be elided. Got: {metadata}"
    );
    assert!(
        metadata.get("step_audit").is_none(),
        "empty step_audit MUST be elided. Got: {metadata}"
    );
}

#[test]
fn s3_anthropic_minimal_envelope_elides_optional_fields() {
    let mut adapter = select_adapter("anthropic", 0x42);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "F".into(),
        backend: "b".into(),
        timestamp_ms: 1,
    });
    let _ = adapter.build_complete_envelope_event(&minimal_envelope_fixture());
    let terminator = adapter.flush_terminator();
    let metadata = parse_anthropic_metadata(&event_data(&terminator[0]));
    assert!(metadata.get("trace_id").is_some());
    assert!(metadata.get("terminal_reason").is_some());
    assert!(metadata.get("stream_policies").is_none());
    assert!(metadata.get("enforcement_summary").is_none());
    assert!(metadata.get("runtime_warnings").is_none());
    assert!(metadata.get("step_audit").is_none());
}

// ════════════════════════════════════════════════════════════════════
// section 4 — Default path: translate(FlowComplete) without envelope
// ════════════════════════════════════════════════════════════════════

#[test]
fn s4_openai_translate_flow_complete_emits_metadata_with_terminal_reason_only() {
    // The trait's default build_complete_envelope_event calls
    // translate(FlowComplete) — which the openai adapter handles
    // without stashing an envelope. flush_terminator emits a metadata
    // frame carrying ONLY terminal_reason (no flow-level fields,
    // because no envelope was stashed). Defensive backwards-compat.
    let mut adapter = select_adapter("openai", 0x42);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "F".into(),
        backend: "b".into(),
        timestamp_ms: 1,
    });
    let _ = adapter.translate(&FlowExecutionEvent::FlowComplete {
        flow_name: "F".into(),
        backend: "b".into(),
        success: true,
        steps_executed: 1,
        tokens_input: 0,
        tokens_output: 1,
        latency_ms: 5,
        timestamp_ms: 10,
    });
    let terminator = adapter.flush_terminator();
    let metadata = parse_openai_metadata(&event_data(&terminator[0]));
    assert_eq!(
        metadata["terminal_reason"], "stop",
        "without an envelope, terminal_reason MUST still surface \
         since translate(FlowComplete) sets the discriminator. Got: {metadata}"
    );
    assert!(
        metadata.get("flow").is_none(),
        "no envelope stashed → no flow-level fields. Got: {metadata}"
    );
}

#[test]
fn s4_anthropic_translate_flow_complete_emits_metadata_with_terminal_reason_only() {
    let mut adapter = select_adapter("anthropic", 0x42);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "F".into(),
        backend: "b".into(),
        timestamp_ms: 1,
    });
    let _ = adapter.translate(&FlowExecutionEvent::FlowComplete {
        flow_name: "F".into(),
        backend: "b".into(),
        success: true,
        steps_executed: 1,
        tokens_input: 0,
        tokens_output: 1,
        latency_ms: 5,
        timestamp_ms: 10,
    });
    let terminator = adapter.flush_terminator();
    let metadata = parse_anthropic_metadata(&event_data(&terminator[0]));
    assert_eq!(metadata["terminal_reason"], "stop");
    assert!(metadata.get("flow").is_none());
}

// ════════════════════════════════════════════════════════════════════
// section 5 — terminal_reason discriminator covers stop / error / none
// ════════════════════════════════════════════════════════════════════

#[test]
fn s5_openai_terminal_reason_error_after_flow_error() {
    let mut adapter = select_adapter("openai", 0x42);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "F".into(),
        backend: "b".into(),
        timestamp_ms: 1,
    });
    let _ = adapter.translate(&FlowExecutionEvent::FlowError {
        flow_name: "F".into(),
        error: "boom".into(),
        timestamp_ms: 2,
    });
    let terminator = adapter.flush_terminator();
    let metadata = parse_openai_metadata(&event_data(&terminator[0]));
    assert_eq!(
        metadata["terminal_reason"], "error",
        "33.z.k.h: FlowError translation MUST set terminal_reason=error"
    );
}

#[test]
fn s5_anthropic_terminal_reason_error_after_flow_error() {
    let mut adapter = select_adapter("anthropic", 0x42);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "F".into(),
        backend: "b".into(),
        timestamp_ms: 1,
    });
    let _ = adapter.translate(&FlowExecutionEvent::FlowError {
        flow_name: "F".into(),
        error: "boom".into(),
        timestamp_ms: 2,
    });
    let terminator = adapter.flush_terminator();
    let metadata = parse_anthropic_metadata(&event_data(&terminator[0]));
    assert_eq!(metadata["terminal_reason"], "error");
}

#[test]
fn s5_openai_terminal_reason_none_when_no_terminal_event() {
    // Defensive: flush_terminator called without any FlowComplete /
    // FlowError translation. terminal_reason="none" surfaces so
    // adopters can detect an aborted stream (e.g. a producer crashed
    // mid-flight before emitting any terminator).
    let mut adapter = select_adapter("openai", 0x42);
    let terminator = adapter.flush_terminator();
    let metadata = parse_openai_metadata(&event_data(&terminator[0]));
    assert_eq!(metadata["terminal_reason"], "none");
}

// ════════════════════════════════════════════════════════════════════
// section 6 — E2E: HTTP POST surfaces populated metadata on openai wire
// ════════════════════════════════════════════════════════════════════

mod e2e {
    use axon::axon_server::{build_router, ServerConfig};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn server_cfg() -> ServerConfig {
        ServerConfig {
            host: "127.0.0.1".into(),
            port: 0,
            channel: "memory".into(),
            auth_token: String::new(),
            log_level: "INFO".into(),
            log_format: "json".into(),
            log_file: None,
            database_url: None,
            config_path: None,
            strict_type_driven_transport: false,
            default_backend: None,
        schemas_dir: None,
        }
    }

    async fn deploy(app: axum::Router, src: &str) -> StatusCode {
        let body = serde_json::json!({
            "source": src,
            "source_file": "h.axon",
            "backend": "stub",
        });
        let req = Request::builder()
            .method("POST")
            .uri("/v1/deploy")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        app.oneshot(req).await.unwrap().status()
    }

    async fn post_no_accept(app: axum::Router, path: &str) -> (StatusCode, String, String) {
        let req = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let ct = resp
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8_lossy(&bytes).to_string();
        (status, ct, body)
    }

    /// Extract the axon_metadata payload from a real HTTP SSE body
    /// (openai dialect).
    fn extract_openai_metadata(body: &str) -> serde_json::Value {
        for line in body.lines() {
            if let Some(rest) = line.strip_prefix("data: ") {
                if rest.starts_with('{') && rest.contains("axon_metadata") {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(rest) {
                        if let Some(m) = v.get("axon_metadata") {
                            return m.clone();
                        }
                    }
                }
            }
        }
        panic!("no axon_metadata frame found in body: {body:?}");
    }

    /// Extract the axon.metadata payload from an anthropic dialect's
    /// HTTP SSE body. Anthropic frames have an `event: axon.metadata`
    /// line preceding the `data: {...}` payload.
    fn extract_anthropic_metadata(body: &str) -> serde_json::Value {
        let mut lines = body.lines().peekable();
        while let Some(line) = lines.next() {
            if line == "event: axon.metadata" {
                if let Some(next) = lines.next() {
                    if let Some(rest) = next.strip_prefix("data: ") {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(rest) {
                            if let Some(m) = v.get("axon_metadata") {
                                return m.clone();
                            }
                        }
                    }
                }
            }
        }
        panic!("no axon.metadata frame found in body: {body:?}");
    }

    #[tokio::test]
    async fn s6_openai_e2e_kivi_shape_surfaces_populated_enforcement_summary() {
        // Algebraic-effect flow (Q1 default → openai dialect) +
        // stub backend with the `drop_oldest` policy declared.
        let src = "tool chat_token_stream { description: \"S\" \
                   effects: <stream:drop_oldest> }\n\
            flow Chat() -> Unit {\n\
                step Generate { ask: \"hi\" apply: chat_token_stream output: Stream<Token> }\n\
            }\n\
            axonendpoint ChatEndpoint { public: true method: POST path: \"/oai\" execute: Chat }";
        let app = build_router(server_cfg());
        assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);
        let (status, _ct, body) = post_no_accept(app, "/oai").await;
        assert_eq!(status, StatusCode::OK);
        let metadata = extract_openai_metadata(&body);

        // Flow-level envelope present + populated.
        assert_eq!(metadata["flow"], "Chat");
        assert_eq!(metadata["success"], true);
        assert_eq!(metadata["steps_executed"], 1);
        assert_eq!(metadata["terminal_reason"], "stop");

        // v1.24.0 — stream_policies populated from the declared
        // tool effect.
        let policies = metadata["stream_policies"]
            .as_array()
            .expect("E2E openai: stream_policies MUST be populated for algebraic-effect flow");
        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0]["step"], "Generate");
        assert_eq!(policies[0]["policy"], "drop_oldest");

        // v1.24.0 — enforcement_summary populated for the step
        // that ran the enforcer.
        let summary = metadata["enforcement_summary"]["Generate"]
            .as_object()
            .expect("E2E openai: enforcement_summary.Generate MUST be populated");
        assert_eq!(summary["policy_slug"], "drop_oldest");
        // v1.31.0 — `apply:`-streaming-tool steps route through
        // the streaming-tool path (v1.31.0); the stub stream emits
        // more than one chunk. Assert the canonical shape robustly:
        // ≥1 chunk produced, every chunk delivered (no backpressure
        // under the fast test consumer).
        let pushed = summary["chunks_pushed"].as_u64().unwrap_or(0);
        assert!(pushed >= 1, "36.x.e.2: the streaming tool produced chunks");
        assert_eq!(summary["chunks_delivered"].as_u64(), Some(pushed));

        // v1.24.0 — step_audit populated unconditionally.
        let audit = metadata["step_audit"]
            .as_array()
            .expect("E2E openai: step_audit MUST be populated for algebraic-effect flow");
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0]["step_name"], "Generate");
        assert_eq!(audit[0]["effect_policy_applied"], "drop_oldest");
        // v1.31.0 — the streaming-tool path (v1.31.0) emits
        // the tool body's own chunks; `tokens_emitted` is ≥1, not the
        // single materialized chunk of the pre-36.i LLM-side path.
        let emitted = audit[0]["tokens_emitted"].as_u64().unwrap_or(0);
        assert!(emitted >= 1, "36.x.e.2: the streaming tool step emitted tokens");
    }

    // ────────────────────────────────────────────────────────────────
    // section 7 — E2E: same flow via sse(anthropic) surfaces same data
    // ────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn s7_anthropic_e2e_surfaces_populated_axon_metadata_frame() {
        let src = "tool chat_token_stream { description: \"S\" \
                   effects: <stream:drop_oldest> }\n\
            flow Chat() -> Unit {\n\
                step Generate { ask: \"hi\" apply: chat_token_stream output: Stream<Token> }\n\
            }\n\
            axonendpoint ChatEndpoint { public: true method: POST path: \"/anth\" \
                                        execute: Chat transport: sse(anthropic) }";
        let app = build_router(server_cfg());
        assert_eq!(deploy(app.clone(), src).await, StatusCode::OK);
        let (status, _ct, body) = post_no_accept(app, "/anth").await;
        assert_eq!(status, StatusCode::OK);
        let metadata = extract_anthropic_metadata(&body);

        assert_eq!(metadata["flow"], "Chat");
        assert_eq!(metadata["success"], true);
        assert_eq!(metadata["steps_executed"], 1);
        assert_eq!(metadata["terminal_reason"], "stop");

        let policies = metadata["stream_policies"].as_array().unwrap();
        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0]["policy"], "drop_oldest");

        let summary = metadata["enforcement_summary"]["Generate"]
            .as_object()
            .unwrap();
        assert_eq!(summary["policy_slug"], "drop_oldest");

        let audit = metadata["step_audit"].as_array().unwrap();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0]["effect_policy_applied"], "drop_oldest");
    }

    // ────────────────────────────────────────────────────────────────
    // section 8 — Cross-dialect parity: openai + anthropic surface same
    //        algebraic-policy data modulo framing (D3 + D4)
    // ────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn s8_openai_and_anthropic_surface_same_metadata_modulo_framing() {
        let src_openai = "tool chat_token_stream { description: \"S\" \
                          effects: <stream:drop_oldest> }\n\
            flow Chat() -> Unit {\n\
                step Generate { ask: \"hi\" apply: chat_token_stream output: Stream<Token> }\n\
            }\n\
            axonendpoint ChatEndpoint { public: true method: POST path: \"/p\" execute: Chat }";
        let src_anthropic = "tool chat_token_stream { description: \"S\" \
                             effects: <stream:drop_oldest> }\n\
            flow Chat() -> Unit {\n\
                step Generate { ask: \"hi\" apply: chat_token_stream output: Stream<Token> }\n\
            }\n\
            axonendpoint ChatEndpoint { public: true method: POST path: \"/p\" \
                                        execute: Chat transport: sse(anthropic) }";
        let app_openai = build_router(server_cfg());
        assert_eq!(deploy(app_openai.clone(), src_openai).await, StatusCode::OK);
        let (_, _, body_openai) = post_no_accept(app_openai, "/p").await;
        let app_anthropic = build_router(server_cfg());
        assert_eq!(deploy(app_anthropic.clone(), src_anthropic).await, StatusCode::OK);
        let (_, _, body_anthropic) = post_no_accept(app_anthropic, "/p").await;

        let openai_meta = extract_openai_metadata(&body_openai);
        let anthropic_meta = extract_anthropic_metadata(&body_anthropic);

        // Algebraic-policy fields are byte-equivalent across dialects
        // (the data origin is the same: dispatcher side-channels →
        // CompleteEnvelope → adapter projection).
        assert_eq!(
            openai_meta["stream_policies"], anthropic_meta["stream_policies"],
            "D3 + D4: stream_policies MUST be byte-identical across \
             openai + anthropic dialects (same upstream side-channel)."
        );
        assert_eq!(
            openai_meta["enforcement_summary"], anthropic_meta["enforcement_summary"],
            "D3 + D4: enforcement_summary MUST be byte-identical across \
             dialects."
        );
        // step_audit fields agree on structural shape (timestamp_ms +
        // output_hash_hex are wall-clock-dependent / output-dependent;
        // tokens_emitted + effect_policy_applied are deterministic).
        let oai_audit = openai_meta["step_audit"].as_array().unwrap();
        let anth_audit = anthropic_meta["step_audit"].as_array().unwrap();
        assert_eq!(oai_audit.len(), anth_audit.len());
        for (oai, anth) in oai_audit.iter().zip(anth_audit.iter()) {
            assert_eq!(oai["step_name"], anth["step_name"]);
            assert_eq!(oai["step_index"], anth["step_index"]);
            assert_eq!(oai["effect_policy_applied"], anth["effect_policy_applied"]);
            assert_eq!(oai["tokens_emitted"], anth["tokens_emitted"]);
            assert_eq!(oai["chunks_dropped"], anth["chunks_dropped"]);
            assert_eq!(oai["chunks_degraded"], anth["chunks_degraded"]);
            assert_eq!(oai["success"], anth["success"]);
        }
    }
}
