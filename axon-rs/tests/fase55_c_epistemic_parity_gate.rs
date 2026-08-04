//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 55.c — the epistemic-wire parity drift gate (the §50.i.4 gate,
//! promoted to the competitive differential).
//!
//! Locks, end-to-end from a real epistemic-tool source, that the
//! `(base, scope, confidence)` triple reaches BOTH transports identically:
//!
//!   1. the two derivation ENTRY POINTS agree — the sync runner's
//!      `derive_epistemic_envelopes_for_flow(&ir, …)` and the streaming
//!      `resolve_epistemic_envelopes_for_flow(source, …)` (which re-derives
//!      the IR from source) return the same envelopes;
//!   2. the two WIRE SHAPES agree — the sync `FlowEnvelope.epistemic_envelopes`
//!      JSON array is byte-identical to the streaming `axon.complete`
//!      `epistemic_envelopes` array.
//!
//! If anyone changes one site without the other (a different
//! `input_confidence`, a different ceiling, a renamed key), this gate goes
//! red. The parity is structural — both funnel through ONE
//! `epistemic_capture::collect_for_flow` — and this gate is the proof.

use axon::axon_server::resolve_epistemic_envelopes_for_flow;
use axon::epistemic_capture::EpistemicEnvelope;
use axon::ir_generator::IRGenerator;
use axon::lexer::Lexer;
use axon::parser::Parser;
use axon::runner::derive_epistemic_envelopes_for_flow;
use axon::wire_envelope::FlowEnvelope;
use axon::wire_format::{select_adapter, CompleteEnvelope};

const SOURCE: &str = "\
tool WebSearch {
  provider: http
  effects: <network, epistemic:speculate>
}
tool ExactLookup {
  provider: internal
  effects: <compute, epistemic:know>
}
type SearchReport { summary: String }
flow Research(query: String) -> SearchReport {
  use WebSearch on \"${query}\"
  use ExactLookup on \"${query}\"
  step Summarize { ask: \"summarize\" output: SearchReport }
}
";

const FLOW: &str = "Research";

fn ir_of(source: &str) -> axon::ir_nodes::IRProgram {
    let tokens = Lexer::new(source, "parity.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    IRGenerator::new().generate(&program)
}

// ── SSE Event data extraction (mirrors fase55_b's helper) ───────────

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

fn sync_wire_array(envs: Vec<EpistemicEnvelope>) -> serde_json::Value {
    let env = FlowEnvelope {
        ontological_type: "SearchReport".into(),
        result: serde_json::Value::Null,
        certainty: 1.0,
        epistemic_envelopes: envs,
        provenance_chain: vec!["flow:Research".into()],
        step_audit: Default::default(),
        audit_chain_hash: String::new(),
        blame_attribution: None,
        execution_metrics: Default::default(),
        trace_id: "t".into(),
        error: None,
        temporal_context: None,
    }
    .seal();
    serde_json::to_value(&env).expect("serialize FlowEnvelope")["epistemic_envelopes"].clone()
}

fn streaming_wire_array(envs: Vec<EpistemicEnvelope>) -> serde_json::Value {
    let envelope = CompleteEnvelope {
        trace_id: 1,
        flow_name: "Research".into(),
        backend: "stub".into(),
        success: true,
        steps_executed: 1,
        tokens_input: 0,
        tokens_output: 0,
        latency_ms: 0,
        effect_policies: Vec::new(),
        enforcement_summaries: Vec::new(),
        runtime_warnings: Vec::new(),
        step_audit_records: Vec::new(),
        epistemic_envelopes: envs,
        temporal_context: None,
    };
    let mut adapter = select_adapter("axon", 1);
    let frames = adapter.build_complete_envelope_event(&envelope);
    let data = event_data(&frames[0]);
    serde_json::from_str::<serde_json::Value>(&data).expect("axon.complete JSON")
        ["epistemic_envelopes"]
        .clone()
}

// ── §1 — the two derivation entry points agree ─────────────────────

#[test]
fn sync_and_streaming_derivations_are_identical() {
    let ir = ir_of(SOURCE);
    let sync = derive_epistemic_envelopes_for_flow(&ir, FLOW);
    let streaming = resolve_epistemic_envelopes_for_flow(SOURCE, "parity.axon", FLOW);
    assert_eq!(
        sync, streaming,
        "the sync runner site and the streaming resolver site MUST derive the \
         identical epistemic envelopes — they funnel through one collect_for_flow"
    );
}

// ── §2 — the derivation is real + non-empty (degradation lock) ─────

#[test]
fn derivation_surfaces_the_expected_triples_from_source() {
    let ir = ir_of(SOURCE);
    let envs = derive_epistemic_envelopes_for_flow(&ir, FLOW);
    assert_eq!(
        envs,
        vec![
            EpistemicEnvelope {
                base: "speculate".into(),
                scope: "tool:WebSearch".into(),
                confidence: 0.80,
                output_type: None, // §58.i.2 — these sample tools declare no output_type
            },
            EpistemicEnvelope {
                base: "know".into(),
                scope: "tool:ExactLookup".into(),
                confidence: 0.99,
                output_type: None,
            },
        ],
        "an epistemic:speculate tool MUST surface confidence ≤ 0.80 and an \
         epistemic:know tool ≤ 0.99 — the Theorem 5.1 degradation, observable"
    );
}

// ── §3 — the two wire shapes are byte-identical ────────────────────

#[test]
fn sync_and_streaming_wire_arrays_are_byte_identical() {
    let ir = ir_of(SOURCE);
    let envs = derive_epistemic_envelopes_for_flow(&ir, FLOW);

    let sync_arr = sync_wire_array(envs.clone());
    let stream_arr = streaming_wire_array(envs);

    assert!(
        sync_arr.is_array() && sync_arr.as_array().map(|a| a.len()) == Some(2),
        "sanity: the sync wire MUST carry the 2 epistemic envelopes; got {sync_arr}"
    );
    assert_eq!(
        sync_arr, stream_arr,
        "PARITY GATE: the `epistemic_envelopes` array on the sync FlowEnvelope and \
         the streaming axon.complete MUST be byte-identical — the §50.i.4 \
         differential, locked"
    );
}

// ── §4 — a non-epistemic flow surfaces nothing on either path ──────

#[test]
fn a_flow_with_no_epistemic_tool_yields_empty_on_both_paths() {
    let src = "\
tool PlainHttp { provider: http effects: <network> }
type R { x: String }
flow Plain(q: String) -> R {
  use PlainHttp on \"${q}\"
  step S { ask: \"x\" output: R }
}
";
    let ir = ir_of(src);
    let sync = derive_epistemic_envelopes_for_flow(&ir, "Plain");
    let streaming = resolve_epistemic_envelopes_for_flow(src, "plain.axon", "Plain");
    assert!(sync.is_empty() && streaming.is_empty(), "no epistemic tool ⇒ empty both paths");
    assert_eq!(sync, streaming);
}
