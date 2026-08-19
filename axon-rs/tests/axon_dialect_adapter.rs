//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.28.0 — Unit tests for the axon dialect adapter.
//!
//! These tests verify the structural behavior of
//! `AxonDialectAdapter` — the D6 backwards-compat baseline adapter.
//! Per-byte wire-shape verification happens end-to-end via the
//! 33.z.k.a anchor (`tests/dialect_diagnostic_anchor.rs`) once
//! 33.z.k.e wires the adapter into the SSE producer.
//!
//! # What this file pins
//!
//! 1. AxonDialectAdapter::new constructs cleanly with monotonic
//!    event-ID counter starting at 1.
//! 2. translate() per FlowExecutionEvent variant produces the
//!    expected Vec<Event> count:
//!    - FlowStart       → 0 events (silently consumed; D6)
//!    - StepStart       → 0 events (silently consumed; D6)
//!    - StepToken       → 1 event (axon.token)
//!    - StepComplete    → 0 events (silently consumed; D6)
//!    - ToolCall        → 1 event (axon.tool_call)
//!    - FlowComplete    → 1 event (axon.complete; in-line terminator)
//!    - FlowError       → 1 event (axon.error; in-line terminator)
//! 3. flush_terminator() returns 0 events (axon emits terminator
//!    in-line with FlowComplete/FlowError).
//! 4. Event IDs increment monotonically across calls.
//! 5. dialect() returns "axon".
//! 6. select_adapter("axon", trace_id) returns an AxonDialectAdapter.
//! 7. select_adapter("openai" / "anthropic" / unknown, trace_id)
//!    falls through to AxonDialectAdapter pre-33.z.k.e/f
//! (defensive — adapters land in subsequent steps).

use axon::flow_execution_event::FlowExecutionEvent;
use axon::wire_format::select_adapter;

// ─── section 1 — AxonDialectAdapter constructs cleanly ─────────────────────

#[test]
fn s1_adapter_constructs_with_dialect_axon() {
    let adapter = select_adapter("axon", 42);
    assert_eq!(adapter.dialect(), "axon");
}

// ─── section 2 — translate() event counts per FlowExecutionEvent variant ──

#[test]
fn s2_flow_start_translates_to_zero_events_d6() {
    let mut adapter = select_adapter("axon", 1);
    let events = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "F".into(),
        backend: "stub".into(),
        timestamp_ms: 100,
    });
    assert_eq!(
        events.len(),
        0,
        "33.z.k.d D6 byte-compat: FlowStart silently consumed in v1.27.1; \
         AxonDialectAdapter MUST preserve that behavior"
    );
}

#[test]
fn s2_step_start_translates_to_zero_events_d6() {
    let mut adapter = select_adapter("axon", 1);
    let events = adapter.translate(&FlowExecutionEvent::StepStart {
        step_name: "S".into(),
        step_index: 0,
        step_type: "step".into(),
            branch_path: String::new(),
        timestamp_ms: 100,
    });
    assert_eq!(events.len(), 0);
}

#[test]
fn s2_step_token_translates_to_one_axon_token_event() {
    let mut adapter = select_adapter("axon", 1);
    let events = adapter.translate(&FlowExecutionEvent::StepToken {
        step_name: "Generate".into(),
        content: "Hola".into(),
        token_index: 0,
            branch_path: String::new(),
        timestamp_ms: 100,
    });
    assert_eq!(events.len(), 1);
}

#[test]
fn s2_step_complete_translates_to_zero_events_d6() {
    let mut adapter = select_adapter("axon", 1);
    let events = adapter.translate(&FlowExecutionEvent::StepComplete {
        step_name: "S".into(),
        step_index: 0,
        success: true,
        full_output: String::new(),
        tokens_input: 0,
        tokens_output: 1,
            branch_path: String::new(),
        timestamp_ms: 100,
    });
    assert_eq!(events.len(), 0);
}

#[test]
fn s2_tool_call_translates_to_one_axon_tool_call_event() {
    let mut adapter = select_adapter("axon", 1);
    let events = adapter.translate(&FlowExecutionEvent::ToolCall {
        step_name: "Generate".into(),
        tool_name: "search".into(),
        content: "{\"query\":\"axon\"}".into(),
        timestamp_ms: 100,
    });
    assert_eq!(events.len(), 1);
}

#[test]
fn s2_flow_complete_translates_to_one_axon_complete_event() {
    let mut adapter = select_adapter("axon", 1);
    let events = adapter.translate(&FlowExecutionEvent::FlowComplete {
        flow_name: "F".into(),
        backend: "stub".into(),
        success: true,
        steps_executed: 1,
        tokens_input: 0,
        tokens_output: 1,
        latency_ms: 10,
        timestamp_ms: 200,
    });
    assert_eq!(events.len(), 1);
}

#[test]
fn s2_flow_error_translates_to_one_axon_error_event() {
    let mut adapter = select_adapter("axon", 1);
    let events = adapter.translate(&FlowExecutionEvent::FlowError {
        flow_name: "F".into(),
        error: "boom".into(),
        timestamp_ms: 200,
    });
    assert_eq!(events.len(), 1);
}

// ─── section 3 — flush_terminator() returns zero events (in-line terminator)

#[test]
fn s3_flush_terminator_is_empty_axon_terminator_inline() {
    let mut adapter = select_adapter("axon", 1);
    let events = adapter.flush_terminator();
    assert_eq!(
        events.len(),
        0,
        "33.z.k.d: axon dialect emits the terminator (axon.complete or \
         axon.error) IN-LINE with translate(); flush_terminator() is \
         a no-op for this dialect"
    );
}

// ─── section 4 — Event IDs increment monotonically across translate() calls

#[test]
fn s4_event_ids_monotonic_across_translate_calls() {
    let mut adapter = select_adapter("axon", 1);
    let mut total_events = 0;
    for i in 0..5 {
        let events = adapter.translate(&FlowExecutionEvent::StepToken {
            step_name: "S".into(),
            content: format!("t{i}"),
            token_index: i as u64,
            branch_path: String::new(),
            timestamp_ms: 100,
        });
        total_events += events.len();
    }
    assert_eq!(total_events, 5);
}

// ─── section 5 — Catalog: dialect() returns "axon" ─────────────────────────

#[test]
fn s5_dialect_returns_axon_string() {
    let adapter = select_adapter("axon", 1);
    assert_eq!(adapter.dialect(), "axon");
}

// ─── section 6 — select_adapter("openai" / "anthropic") defensively falls
//       through to axon pre-33.z.k.e/f ──────────────────────────────

#[test]
fn s6_select_adapter_openai_returns_openai_post_33_z_k_e() {
    let adapter = select_adapter("openai", 1);
    assert_eq!(
        adapter.dialect(),
        "openai",
        "33.z.k.e flipped this assertion from `axon` (33.z.k.d \
         defensive fallthrough) to `openai`. The OpenAI dialect \
         adapter ships in 33.z.k.e and dispatches via select_adapter."
    );
}

#[test]
fn s6_select_adapter_anthropic_returns_anthropic_post_33_z_k_f() {
    let adapter = select_adapter("anthropic", 1);
    assert_eq!(
        adapter.dialect(),
        "anthropic",
        "33.z.k.f flipped this assertion from `axon` (33.z.k.d \
         defensive fallthrough) to `anthropic`. The Anthropic \
         dialect adapter ships in 33.z.k.f."
    );
}

#[test]
fn s6_select_adapter_unknown_falls_through_to_axon_defensively() {
    let adapter = select_adapter("bogus", 1);
    assert_eq!(
        adapter.dialect(),
        "axon",
        "33.z.k.d: unknown dialect string defaults to axon defensively. \
         Parser already validates against AXONENDPOINT_TRANSPORT_DIALECTS \
         at parse time, so unknown values reaching the runtime would be \
         a structural bug — but we never panic on a stale input."
    );
}

// Q3 revision 2026-05-14 — Kimi + GLM dispatch to OpenAIDialectAdapter
// (they share the OpenAI-compat wire byte-identically). The dialect
// string survives as a first-class identifier for adopter intent
// declaration + observability correlation, but the wire is canonical
// OpenAI Chat Completions streaming.

#[test]
fn s7_select_adapter_kimi_dispatches_to_openai_adapter_q3_revision() {
    let adapter = select_adapter("kimi", 1);
    assert_eq!(
        adapter.dialect(),
        "openai",
        "Q3 revision: kimi shares the OpenAI-compat wire — dispatched \
         to OpenAIDialectAdapter so the bytes are canonical-OpenAI. \
         Adopter declares `transport: sse(kimi)` for intent; runtime \
         delivers OpenAI-shape wire."
    );
}

#[test]
fn s7_select_adapter_glm_dispatches_to_openai_adapter_q3_revision() {
    let adapter = select_adapter("glm", 1);
    assert_eq!(
        adapter.dialect(),
        "openai",
        "Q3 revision: glm shares the OpenAI-compat wire — dispatched \
         to OpenAIDialectAdapter so the bytes are canonical-OpenAI. \
         Adopter declares `transport: sse(glm)` for intent; runtime \
         delivers OpenAI-shape wire."
    );
}
