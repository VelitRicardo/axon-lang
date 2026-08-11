//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 55.b — the captured epistemic envelope reaches the WIRE on both
//! transports.
//!
//! §55.a derived the `(base, scope, confidence)` triple; §55.b surfaces it:
//!   * sync `transport: json` → `FlowEnvelope.epistemic_envelopes`
//!     (`ServerExecutionResult` → `from_execution_result` → `seal`);
//!   * streaming `transport: sse(axon)` → the `epistemic_envelopes` array
//!     on the `axon.complete` event (`CompleteEnvelope` → axon dialect).
//!
//! Both keys + element shapes are byte-identical by construction (the same
//! `EpistemicEnvelope` serialized on each path). The exhaustive
//! cross-transport parity gate is §55.c; this test proves each path
//! surfaces the triple and previews their agreement.

use axon::axon_server::{EnforcementSummaryWire, ServerExecutionResult};
use axon::epistemic_capture::EpistemicEnvelope;
use axon::wire_envelope::FlowEnvelope;
use axon::wire_format::{select_adapter, CompleteEnvelope};

fn sample_envelope() -> EpistemicEnvelope {
    EpistemicEnvelope {
        base: "speculate".into(),
        scope: "tool:WebSearch".into(),
        confidence: 0.80,
        output_type: None, // §58.i.2 — no declared output type on this sample
    }
}

// ─── SSE Event data extraction (mirrors fase33z_k_h's helper) ───────

fn event_data(event: &axum::response::sse::Event) -> String {
    let debug = format!("{event:?}");
    let Some(start) = debug.find("Active(b\"").map(|p| p + "Active(b\"".len()) else {
        return String::new();
    };
    let rest = &debug[start..];
    let Some(end) = rest.rfind("\")") else { return String::new() };
    let raw = rest[..end].as_bytes();
    let mut buf: Vec<u8> = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\\' && i + 1 < raw.len() {
            match raw[i + 1] {
                b'n' => { buf.push(b'\n'); i += 2; }
                b'r' => { buf.push(b'\r'); i += 2; }
                b't' => { buf.push(b'\t'); i += 2; }
                b'"' => { buf.push(b'"'); i += 2; }
                b'\\' => { buf.push(b'\\'); i += 2; }
                b'x' if i + 3 < raw.len() => {
                    let hx = std::str::from_utf8(&raw[i + 2..i + 4]).ok();
                    match hx.and_then(|s| u8::from_str_radix(s, 16).ok()) {
                        Some(v) => { buf.push(v); i += 4; }
                        None => { buf.push(raw[i]); i += 1; }
                    }
                }
                _ => { buf.push(raw[i]); i += 1; }
            }
        } else {
            buf.push(raw[i]);
            i += 1;
        }
    }
    let s = String::from_utf8_lossy(&buf).to_string();
    let Some(ds) = s.find("data: ").map(|p| p + "data: ".len()) else {
        return String::new();
    };
    let rest = &s[ds..];
    let de = rest.find('\n').unwrap_or(rest.len());
    rest[..de].to_string()
}

fn exec_result(epistemic: Vec<EpistemicEnvelope>) -> ServerExecutionResult {
    ServerExecutionResult {
        success: true,
        flow_name: "ResearchQuery".into(),
        source_file: "research.axon".into(),
        backend: "stub".into(),
        steps_executed: 1,
        latency_ms: 10,
        tokens_input: 0,
        tokens_output: 0,
        anchor_checks: 0,
        anchor_breaches: 0,
        errors: 0,
        type_errors: Vec::new(),
        step_names: vec!["Summarize".into()],
        step_results: vec![r#"{"summary":"x"}"#.into()],
        trace_id: 0,
        effect_policies: Vec::new(),
        enforcement_summaries: Vec::<(String, EnforcementSummaryWire)>::new(),
        runtime_warnings: Vec::new(),
        provenance_events: Vec::new(),
        blame_attribution: None,
        epistemic_envelopes: epistemic,
        error: None,
        temporal_context: None,
    }
}

fn complete_envelope(epistemic: Vec<EpistemicEnvelope>) -> CompleteEnvelope {
    CompleteEnvelope {
        trace_id: 0x55b,
        flow_name: "ResearchQuery".into(),
        backend: "stub".into(),
        success: true,
        steps_executed: 1,
        tokens_input: 0,
        tokens_output: 0,
        latency_ms: 10,
        effect_policies: Vec::new(),
        enforcement_summaries: Vec::new(),
        runtime_warnings: Vec::new(),
        step_audit_records: Vec::new(),
        epistemic_envelopes: epistemic,
        temporal_context: None,
    }
}

// ─── §1 — sync FlowEnvelope surfaces the triple ────────────────────

#[test]
fn sync_flow_envelope_carries_the_epistemic_triple() {
    let env =
        FlowEnvelope::from_execution_result(exec_result(vec![sample_envelope()]), "Report".into())
            .seal();
    let json = serde_json::to_value(&env).expect("serialize FlowEnvelope");
    let arr = json["epistemic_envelopes"]
        .as_array()
        .expect("sync wire MUST carry an epistemic_envelopes array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["base"], "speculate");
    assert_eq!(arr[0]["scope"], "tool:WebSearch");
    assert_eq!(arr[0]["confidence"], 0.80);
}

#[test]
fn sync_envelope_elides_the_field_when_no_epistemic_tool() {
    let env =
        FlowEnvelope::from_execution_result(exec_result(Vec::new()), "Report".into()).seal();
    let json = serde_json::to_value(&env).expect("serialize");
    assert!(
        json.get("epistemic_envelopes").is_none(),
        "empty epistemic_envelopes MUST be elided (D5 byte-compat); got: {json}"
    );
}

// ─── §2 — streaming axon.complete surfaces the triple ──────────────

#[test]
fn streaming_axon_complete_carries_the_epistemic_triple() {
    let mut adapter = select_adapter("axon", 0x55b);
    let frames = adapter.build_complete_envelope_event(&complete_envelope(vec![sample_envelope()]));
    assert_eq!(frames.len(), 1, "axon dialect emits one axon.complete frame");
    let data = event_data(&frames[0]);
    let json: serde_json::Value =
        serde_json::from_str(&data).expect("axon.complete data MUST be valid JSON");
    let arr = json["epistemic_envelopes"]
        .as_array()
        .expect("streaming wire MUST carry an epistemic_envelopes array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["base"], "speculate");
    assert_eq!(arr[0]["scope"], "tool:WebSearch");
    assert_eq!(arr[0]["confidence"], 0.80);
}

#[test]
fn streaming_elides_the_field_when_no_epistemic_tool() {
    let mut adapter = select_adapter("axon", 0x55b);
    let frames = adapter.build_complete_envelope_event(&complete_envelope(Vec::new()));
    let json: serde_json::Value =
        serde_json::from_str(&event_data(&frames[0])).expect("valid JSON");
    assert!(
        json.get("epistemic_envelopes").is_none(),
        "empty epistemic_envelopes MUST be elided on the streaming path too; got: {json}"
    );
}

// ─── §3 — cross-transport agreement (preview of the §55.c gate) ────

#[test]
fn both_transports_emit_byte_identical_epistemic_elements() {
    let envs = vec![sample_envelope()];

    let sync = FlowEnvelope::from_execution_result(exec_result(envs.clone()), "Report".into())
        .seal();
    let sync_json = serde_json::to_value(&sync).unwrap();
    let sync_arr = sync_json["epistemic_envelopes"].clone();

    let mut adapter = select_adapter("axon", 0x55b);
    let frames = adapter.build_complete_envelope_event(&complete_envelope(envs));
    let stream_json: serde_json::Value =
        serde_json::from_str(&event_data(&frames[0])).unwrap();
    let stream_arr = stream_json["epistemic_envelopes"].clone();

    assert_eq!(
        sync_arr, stream_arr,
        "the (base, scope, confidence) elements MUST be identical across transports — \
         the §55.c parity invariant (sync FlowEnvelope vs streaming axon.complete)"
    );
}
