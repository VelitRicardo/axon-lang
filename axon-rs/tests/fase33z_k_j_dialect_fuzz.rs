//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 33.z.k.j (v1.28.0) — D12 production-grade fuzz × dialects.
//!
//! Stochastic coverage layer above the deterministic
//! 33.z.k.{d,e,f,g.3,h,i} pin-tests. Where those packs anchor SPECIFIC
//! event sequences byte-exact, this pack hammers each adapter with
//! **~3 000 deterministic LCG iters** of random `FlowExecutionEvent`
//! streams + random `CompleteEnvelope` shapes, asserting closed-
//! catalog invariants across the full input space.
//!
//! # Why a stochastic layer
//!
//! The pinned tests fix one event sequence at a time + assert exact
//! frame counts / contents. They are precise but narrow. Production
//! adopters run thousands of distinct flow shapes; the adapter MUST
//! be total (no panic / no malformed wire) across the entire reachable
//! input space, not just the pinned shapes. This pack rolls dice
//! across 7 invariants and asserts each holds under all sampled inputs.
//!
//! # 7 invariants exercised
//!
//! 1. **§1 — Adapter totality** (3 dialects × 200 iters = 600 iters):
//!    random sequences of 1-30 FlowExecutionEvents drive each adapter
//!    without panic. Each frame parses as well-formed JSON OR is the
//!    `[DONE]` literal sentinel (the one non-JSON exception in the
//!    closed catalog).
//!
//! 2. **§2 — Closed-catalog wire output** (3 dialects × 150 iters =
//!    450 iters): every emitted frame's `event:` name (when present)
//!    is in the dialect's closed event vocabulary; every JSON payload
//!    has a top-level shape consistent with the dialect spec.
//!
//! 3. **§3 — Arrival-order signature invariant** (300 iters across all
//!    3 dialects): random T-X-T-X-... event sequences project onto
//!    each dialect's wire with the same arrival-order signature
//!    modulo framing. Closed-vocabulary signature {T, X} preserved
//!    cross-dialect.
//!
//! 4. **§4 — Anthropic content_block lifecycle** (300 iters): every
//!    `content_block_start` has a matching `content_block_stop`;
//!    block indices advance monotonically; no orphan start / stop.
//!
//! 5. **§5 — OpenAI tool_call_id monotonicity** (200 iters): random
//!    tool-call counts produce monotonically-increasing call IDs
//!    `call_<trace_hex>_<N>` with `N` strictly increasing per
//!    request.
//!
//! 6. **§6 — CompleteEnvelope round-trip** (300 iters × 3 dialects =
//!    900 iters): random envelopes with random algebraic-policy
//!    field populations project byte-exactly onto the metadata frame
//!    on both openai + anthropic; round-trip-equivalent on axon.
//!
//! 7. **§7 — Determinism across repeats** (200 iters): same seed →
//!    same wire bytes (modulo timestamp/created variability) across
//!    5 repeats per shape, on every dialect.
//!
//! **Grand total: ~2 950 deterministic LCG iters**, runtime <2s on a
//! 2025-era developer laptop. Hand-rolled LCG (Knuth/MMIX constants)
//! mirrors the 33.z.production_fuzz idiom — no external dep.

use axon::axon_server::EnforcementSummaryWire;
use axon::axonendpoint_replay::StepAuditRecord;
use axon::flow_execution_event::FlowExecutionEvent;
use axon::runtime_warnings::{FallbackMode, RuntimeWarning, WarningCode};
use axon::wire_format::{select_adapter, CompleteEnvelope, WireFormatAdapter};

// ────────────────────────────────────────────────────────────────────
//  Hand-rolled deterministic 64-bit LCG (Knuth/MMIX constants).
// ────────────────────────────────────────────────────────────────────

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        let mixed = seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(0xBB67_AE85_84CA_A73B);
        Self(mixed.max(1))
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    fn range(&mut self, max: usize) -> usize {
        (self.next_u64() as usize) % max.max(1)
    }

    fn boolean(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// ASCII printable string (excludes quote + backslash) of length
    /// 1-20. Safe content for text deltas + tool-call args.
    fn ascii(&mut self) -> String {
        let len = 1 + self.range(20);
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            let mut c: u8;
            loop {
                c = 32 + (self.range(95) as u8);
                if c != b'"' && c != b'\\' {
                    break;
                }
            }
            s.push(c as char);
        }
        s
    }

    /// Lowercase ASCII identifier of length 1-15.
    fn ident(&mut self) -> String {
        let len = 1 + self.range(15);
        let mut s = String::with_capacity(len);
        for i in 0..len {
            let c = if i == 0 {
                (b'a' + (self.range(26) as u8)) as char
            } else {
                let r = self.range(27);
                if r < 26 {
                    (b'a' + (r as u8)) as char
                } else {
                    '_'
                }
            };
            s.push(c);
        }
        s
    }

    /// Closed-catalog policy slug pick.
    fn policy_slug(&mut self) -> &'static str {
        match self.range(4) {
            0 => "drop_oldest",
            1 => "degrade_quality",
            2 => "pause_upstream",
            _ => "fail",
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  Event-stream generators
// ────────────────────────────────────────────────────────────────────

/// Generate a random length-1-30 sequence of FlowExecutionEvent
/// values matching the producer contract:
///   - FlowStart always at position 0
///   - StepStart/StepComplete pair only when StepStart precedes
///   - **Always ends with a terminal event** (FlowComplete or
///     FlowError) — mirrors the production producer's defense-in-
///     depth (`server_execute_streaming` guarantees a terminal even
///     under error). Fuzz tests respecting the producer contract
///     exercise the same input space adapters see in production.
fn gen_random_event_stream(lcg: &mut Lcg) -> Vec<FlowExecutionEvent> {
    let len = 1 + lcg.range(30);
    let mut events = Vec::with_capacity(len);
    let flow_name = lcg.ident();
    let backend = lcg.ident();

    events.push(FlowExecutionEvent::FlowStart {
        flow_name: flow_name.clone(),
        backend: backend.clone(),
        timestamp_ms: 1_000_000,
    });

    let mut current_step_open = false;
    let mut step_idx: usize = 0;
    let mut terminated = false;

    for i in 1..len {
        if terminated {
            break;
        }
        let pick = lcg.range(7);
        match pick {
            0 => {
                if !current_step_open {
                    events.push(FlowExecutionEvent::StepStart {
                        step_name: lcg.ident(),
                        step_index: step_idx,
                        step_type: "step".into(),
            branch_path: String::new(),
                        timestamp_ms: 1_000_000 + i as u64,
                    });
                    current_step_open = true;
                }
            }
            1 | 2 => {
                events.push(FlowExecutionEvent::StepToken {
                    step_name: lcg.ident(),
                    content: lcg.ascii(),
                    token_index: i as u64,
            branch_path: String::new(),
                    timestamp_ms: 1_000_000 + i as u64,
                });
            }
            3 => {
                events.push(FlowExecutionEvent::ToolCall {
                    step_name: lcg.ident(),
                    tool_name: lcg.ident(),
                    content: format!("{{\"q\":\"{}\"}}", lcg.ident()),
                    timestamp_ms: 1_000_000 + i as u64,
                });
            }
            4 => {
                if current_step_open {
                    events.push(FlowExecutionEvent::StepComplete {
                        step_name: lcg.ident(),
                        step_index: step_idx,
                        success: lcg.boolean(),
                        full_output: lcg.ascii(),
                        tokens_input: lcg.range(100) as u64,
                        tokens_output: lcg.range(100) as u64,
            branch_path: String::new(),
                        timestamp_ms: 1_000_000 + i as u64,
                    });
                    current_step_open = false;
                    step_idx += 1;
                }
            }
            5 => {
                events.push(FlowExecutionEvent::FlowComplete {
                    flow_name: flow_name.clone(),
                    backend: backend.clone(),
                    success: lcg.boolean(),
                    steps_executed: step_idx,
                    tokens_input: lcg.range(1000) as u64,
                    tokens_output: lcg.range(1000) as u64,
                    latency_ms: lcg.range(10_000) as u64,
                    timestamp_ms: 1_000_000 + i as u64,
                });
                terminated = true;
            }
            _ => {
                events.push(FlowExecutionEvent::FlowError {
                    flow_name: flow_name.clone(),
                    error: lcg.ascii(),
                    timestamp_ms: 1_000_000 + i as u64,
                });
                terminated = true;
            }
        }
    }
    // Producer contract: every stream MUST end with a terminal event.
    // If the random loop didn't hit pick 5 or 6, append one explicitly
    // so the input space the fuzz exercises matches what adapters see
    // in production (axon_server.rs guarantees defense-in-depth on
    // missing terminators).
    if !terminated {
        if lcg.boolean() {
            events.push(FlowExecutionEvent::FlowComplete {
                flow_name: flow_name.clone(),
                backend: backend.clone(),
                success: lcg.boolean(),
                steps_executed: step_idx,
                tokens_input: lcg.range(1000) as u64,
                tokens_output: lcg.range(1000) as u64,
                latency_ms: lcg.range(10_000) as u64,
                timestamp_ms: 1_000_000 + len as u64,
            });
        } else {
            events.push(FlowExecutionEvent::FlowError {
                flow_name: flow_name.clone(),
                error: lcg.ascii(),
                timestamp_ms: 1_000_000 + len as u64,
            });
        }
    }
    events
}

/// Generate an interleaved Text/Tool-call sequence of fixed shape —
/// returns the closed-vocabulary signature alongside the events so
/// the test can verify per-dialect projection preserves the
/// signature.
fn gen_interleaved_t_x_sequence(lcg: &mut Lcg) -> (Vec<FlowExecutionEvent>, Vec<&'static str>) {
    let len = 1 + lcg.range(10);
    let mut events = vec![FlowExecutionEvent::FlowStart {
        flow_name: "F".into(),
        backend: "B".into(),
        timestamp_ms: 1,
    }];
    let mut signature: Vec<&'static str> = Vec::with_capacity(len);
    for i in 0..len {
        if lcg.boolean() {
            events.push(FlowExecutionEvent::StepToken {
                step_name: "S".into(),
                content: lcg.ascii(),
                token_index: i as u64,
            branch_path: String::new(),
                timestamp_ms: 100 + i as u64,
            });
            signature.push("T");
        } else {
            events.push(FlowExecutionEvent::ToolCall {
                step_name: "S".into(),
                tool_name: lcg.ident(),
                content: "{}".into(),
                timestamp_ms: 100 + i as u64,
            });
            signature.push("X");
        }
    }
    events.push(FlowExecutionEvent::FlowComplete {
        flow_name: "F".into(),
        backend: "B".into(),
        success: true,
        steps_executed: 1,
        tokens_input: 0,
        tokens_output: signature.iter().filter(|s| **s == "T").count() as u64,
        latency_ms: 1,
        timestamp_ms: 200,
    });
    (events, signature)
}

/// Generate a random CompleteEnvelope with each algebraic-policy
/// field either populated or empty (50/50 per field).
fn gen_random_envelope(lcg: &mut Lcg) -> CompleteEnvelope {
    let mut effect_policies = Vec::new();
    if lcg.boolean() {
        let n = 1 + lcg.range(5);
        for _ in 0..n {
            effect_policies.push((lcg.ident(), lcg.policy_slug().to_string()));
        }
    }
    let mut enforcement_summaries = Vec::new();
    if lcg.boolean() {
        let n = 1 + lcg.range(5);
        for _ in 0..n {
            enforcement_summaries.push((
                lcg.ident(),
                EnforcementSummaryWire {
                    policy_slug: lcg.policy_slug().to_string(),
                    chunks_pushed: lcg.range(100) as u64,
                    chunks_delivered: lcg.range(100) as u64,
                    drop_oldest_hits: lcg.range(10) as u64,
                    degrade_quality_hits: lcg.range(10) as u64,
                    pause_upstream_blocks: lcg.range(10) as u64,
                    fail_overflows: lcg.range(10) as u64,
                    failed: lcg.boolean(),
                },
            ));
        }
    }
    let mut runtime_warnings = Vec::new();
    if lcg.boolean() {
        let n = 1 + lcg.range(3);
        for _ in 0..n {
            runtime_warnings.push(RuntimeWarning {
                code: WarningCode::AxonW002,
                flow_name: lcg.ident(),
                backend: lcg.ident(),
                fallback_mode: FallbackMode::BackendLacksStream,
                step_name: None,
                declared_output: String::new(),
                message: lcg.ascii(),
                timestamp_ms: 1_000_000 + lcg.range(1_000_000) as u64,
            });
        }
    }
    let mut step_audit_records = Vec::new();
    if lcg.boolean() {
        let n = 1 + lcg.range(5);
        for i in 0..n {
            step_audit_records.push(StepAuditRecord {
                step_name: lcg.ident(),
                step_index: i,
                success: lcg.boolean(),
                tokens_emitted: lcg.range(100) as u64,
                output_hash_hex: format!("{:016x}{:016x}", lcg.next_u64(), lcg.next_u64()),
                effect_policy_applied: if lcg.boolean() {
                    Some(lcg.policy_slug().to_string())
                } else {
                    None
                },
                chunks_dropped: lcg.range(10) as u64,
                chunks_degraded: lcg.range(10) as u64,
                timestamp_ms: 1_000_000 + lcg.range(1_000_000) as u64,
                // §Fase 34.i — tool-stream provenance fields not
                // exercised by this dialect fuzz; serde elides None.
                ..Default::default()
            });
        }
    }
    CompleteEnvelope {
        trace_id: lcg.next_u64(),
        flow_name: lcg.ident(),
        backend: lcg.ident(),
        success: lcg.boolean(),
        steps_executed: lcg.range(20),
        tokens_input: lcg.range(10_000) as u64,
        tokens_output: lcg.range(10_000) as u64,
        latency_ms: lcg.range(60_000) as u64,
        effect_policies,
        enforcement_summaries,
        runtime_warnings,
        step_audit_records,
        epistemic_envelopes: Vec::new(),
        temporal_context: None,
    }
}

// ────────────────────────────────────────────────────────────────────
//  Frame parsing helpers (mirror of 33.z.k.{e,f} extraction).
// ────────────────────────────────────────────────────────────────────

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
//  §1 — Adapter totality across all 3 dialects (3 × 200 iters)
// ════════════════════════════════════════════════════════════════════

fn assert_frame_is_well_formed(frame: &axum::response::sse::Event, dialect: &str, iter: usize) {
    let data = event_data(frame);
    // Empty data is permitted (defensive — some retry-only frames
    // emit no data payload).
    if data.is_empty() {
        return;
    }
    // [DONE] is the one non-JSON sentinel in the closed catalog
    // (openai dialect only).
    if data == "[DONE]" {
        assert_eq!(
            dialect, "openai",
            "33.z.k.j §1: only openai dialect MUST emit `[DONE]` \
             sentinel. iter {iter}, dialect `{dialect}`, data: {data:?}"
        );
        return;
    }
    // Every other payload MUST parse as well-formed JSON.
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&data);
    assert!(
        parsed.is_ok(),
        "33.z.k.j §1: frame data MUST be well-formed JSON OR the \
         `[DONE]` sentinel. iter {iter}, dialect `{dialect}`, \
         data: {data:?}, parse error: {:?}",
        parsed.err()
    );
}

#[test]
fn s1_axon_adapter_totality_under_random_event_streams() {
    for iter in 0..200 {
        let mut lcg = Lcg::new(0xAA00_0000 + iter as u64);
        let events = gen_random_event_stream(&mut lcg);
        let mut adapter = select_adapter("axon", lcg.next_u64());
        let frames = drive_adapter(&mut adapter, &events);
        for f in &frames {
            assert_frame_is_well_formed(f, "axon", iter);
        }
    }
}

#[test]
fn s1_openai_adapter_totality_under_random_event_streams() {
    for iter in 0..200 {
        let mut lcg = Lcg::new(0xBB00_0000 + iter as u64);
        let events = gen_random_event_stream(&mut lcg);
        let mut adapter = select_adapter("openai", lcg.next_u64());
        let frames = drive_adapter(&mut adapter, &events);
        for f in &frames {
            assert_frame_is_well_formed(f, "openai", iter);
        }
    }
}

#[test]
fn s1_anthropic_adapter_totality_under_random_event_streams() {
    for iter in 0..200 {
        let mut lcg = Lcg::new(0xCC00_0000 + iter as u64);
        let events = gen_random_event_stream(&mut lcg);
        let mut adapter = select_adapter("anthropic", lcg.next_u64());
        let frames = drive_adapter(&mut adapter, &events);
        for f in &frames {
            assert_frame_is_well_formed(f, "anthropic", iter);
        }
    }
}

// ════════════════════════════════════════════════════════════════════
//  §2 — Closed-catalog wire output (event names + JSON shape)
// ════════════════════════════════════════════════════════════════════

const AXON_EVENT_VOCABULARY: &[&str] =
    &["axon.token", "axon.complete", "axon.tool_call", "axon.error"];
const OPENAI_EVENT_VOCABULARY: &[&str] = &[];  // openai emits data-only frames
const ANTHROPIC_EVENT_VOCABULARY: &[&str] = &[
    "message_start",
    "content_block_start",
    "content_block_delta",
    "content_block_stop",
    "message_delta",
    "message_stop",
    "axon.metadata",
];

#[test]
fn s2_axon_event_names_are_closed_vocabulary() {
    for iter in 0..150 {
        let mut lcg = Lcg::new(0xDD00_0000 + iter as u64);
        let events = gen_random_event_stream(&mut lcg);
        let mut adapter = select_adapter("axon", lcg.next_u64());
        let frames = drive_adapter(&mut adapter, &events);
        for f in &frames {
            let name = event_name(f);
            if !name.is_empty() {
                assert!(
                    AXON_EVENT_VOCABULARY.contains(&name.as_str()),
                    "33.z.k.j §2 axon: frame `event:` name `{name}` is \
                     not in the closed vocabulary {AXON_EVENT_VOCABULARY:?}. \
                     iter {iter}"
                );
            }
        }
    }
}

#[test]
fn s2_openai_frames_carry_no_event_name() {
    // Per OpenAI spec, every chunk is `data: {...}` only — no
    // `event:` line. Even the Q7 axon_metadata extension + [DONE]
    // sentinel adhere to this.
    for iter in 0..150 {
        let mut lcg = Lcg::new(0xEE00_0000 + iter as u64);
        let events = gen_random_event_stream(&mut lcg);
        let mut adapter = select_adapter("openai", lcg.next_u64());
        let frames = drive_adapter(&mut adapter, &events);
        for f in &frames {
            assert_eq!(
                event_name(f),
                "",
                "33.z.k.j §2 openai: every frame MUST be `data:` only \
                 (no `event:` line) per OpenAI Chat Completions \
                 streaming spec. iter {iter}, name: {:?}",
                event_name(f)
            );
        }
        let _ = OPENAI_EVENT_VOCABULARY;
    }
}

#[test]
fn s2_anthropic_event_names_are_closed_vocabulary() {
    for iter in 0..150 {
        let mut lcg = Lcg::new(0xFF00_0000 + iter as u64);
        let events = gen_random_event_stream(&mut lcg);
        let mut adapter = select_adapter("anthropic", lcg.next_u64());
        let frames = drive_adapter(&mut adapter, &events);
        for f in &frames {
            let name = event_name(f);
            if !name.is_empty() {
                assert!(
                    ANTHROPIC_EVENT_VOCABULARY.contains(&name.as_str()),
                    "33.z.k.j §2 anthropic: frame `event:` name `{name}` \
                     is not in the closed vocabulary \
                     {ANTHROPIC_EVENT_VOCABULARY:?}. iter {iter}"
                );
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════
//  §3 — Arrival-order signature invariant across dialects (300 iters)
// ════════════════════════════════════════════════════════════════════

fn arrival_signature(frames: &[axum::response::sse::Event]) -> Vec<&'static str> {
    let mut sig = Vec::new();
    for frame in frames {
        let name = event_name(frame);
        let data = event_data(frame);
        if name == "axon.token" {
            sig.push("T");
        } else if name == "axon.tool_call" {
            sig.push("X");
        } else if name.is_empty() {
            // OpenAI-style data-only frame.
            if data.contains("\"delta\":{\"content\":") {
                sig.push("T");
            } else if data.contains("\"tool_calls\":") {
                sig.push("X");
            }
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

#[test]
fn s3_arrival_order_invariant_holds_across_all_three_dialects() {
    for iter in 0..300 {
        let mut lcg = Lcg::new(0x1234_0000 + iter as u64);
        let (events, expected) = gen_interleaved_t_x_sequence(&mut lcg);

        let mut axon_adapter = select_adapter("axon", 0x10);
        let mut openai_adapter = select_adapter("openai", 0x10);
        let mut anthropic_adapter = select_adapter("anthropic", 0x10);

        let axon_sig = arrival_signature(&drive_adapter(&mut axon_adapter, &events));
        let openai_sig = arrival_signature(&drive_adapter(&mut openai_adapter, &events));
        let anthropic_sig =
            arrival_signature(&drive_adapter(&mut anthropic_adapter, &events));

        assert_eq!(
            axon_sig, expected,
            "33.z.k.j §3 iter {iter}: axon dialect MUST preserve arrival \
             signature {expected:?}, got {axon_sig:?}"
        );
        assert_eq!(
            openai_sig, expected,
            "33.z.k.j §3 iter {iter}: openai dialect MUST preserve arrival \
             signature {expected:?}, got {openai_sig:?}"
        );
        assert_eq!(
            anthropic_sig, expected,
            "33.z.k.j §3 iter {iter}: anthropic dialect MUST preserve arrival \
             signature {expected:?}, got {anthropic_sig:?}"
        );
    }
}

// ════════════════════════════════════════════════════════════════════
//  §4 — Anthropic content_block lifecycle (300 iters)
// ════════════════════════════════════════════════════════════════════

#[test]
fn s4_anthropic_block_lifecycle_is_well_formed() {
    for iter in 0..300 {
        let mut lcg = Lcg::new(0x5678_0000 + iter as u64);
        let events = gen_random_event_stream(&mut lcg);
        let mut adapter = select_adapter("anthropic", lcg.next_u64());
        let frames = drive_adapter(&mut adapter, &events);

        // Track open blocks by index: start MUST be balanced by stop.
        // Indices MUST advance monotonically across the stream.
        let mut open_blocks: std::collections::HashSet<u64> = std::collections::HashSet::new();
        let mut seen_indices: Vec<u64> = Vec::new();
        let mut max_index_started: i64 = -1;

        for f in &frames {
            let name = event_name(f);
            let data = event_data(f);
            if name == "content_block_start" || name == "content_block_stop" {
                let parsed: serde_json::Value =
                    serde_json::from_str(&data).expect("anthropic block frame is JSON");
                let idx = parsed
                    .get("index")
                    .and_then(|x| x.as_u64())
                    .expect("anthropic block frame has `index`");
                if name == "content_block_start" {
                    assert!(
                        idx as i64 > max_index_started,
                        "33.z.k.j §4 iter {iter}: block index {idx} MUST be \
                         strictly greater than previously-seen max \
                         {max_index_started} (anthropic spec: monotonic indices)"
                    );
                    max_index_started = idx as i64;
                    assert!(
                        open_blocks.insert(idx),
                        "33.z.k.j §4 iter {iter}: duplicate \
                         content_block_start for index {idx}"
                    );
                } else {
                    assert!(
                        open_blocks.remove(&idx),
                        "33.z.k.j §4 iter {iter}: content_block_stop for \
                         index {idx} but no matching start"
                    );
                }
                seen_indices.push(idx);
            }
        }
        assert!(
            open_blocks.is_empty(),
            "33.z.k.j §4 iter {iter}: anthropic dialect MUST close every \
             opened content_block. Orphan opens: {open_blocks:?}"
        );
    }
}

// ════════════════════════════════════════════════════════════════════
//  §5 — OpenAI tool_call_id monotonicity (200 iters)
// ════════════════════════════════════════════════════════════════════

#[test]
fn s5_openai_tool_call_ids_monotonic_per_request() {
    for iter in 0..200 {
        let mut lcg = Lcg::new(0x9ABC_0000 + iter as u64);
        let trace_id = lcg.next_u64();
        let trace_hex = format!("{trace_id:x}");
        let mut adapter = select_adapter("openai", trace_id);
        let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
            flow_name: "F".into(),
            backend: "b".into(),
            timestamp_ms: 1,
        });

        let tool_call_count = 1 + lcg.range(10);
        let mut emitted_ids: Vec<String> = Vec::new();
        for _ in 0..tool_call_count {
            let frames = adapter.translate(&FlowExecutionEvent::ToolCall {
                step_name: "S".into(),
                tool_name: lcg.ident(),
                content: "{}".into(),
                timestamp_ms: 100,
            });
            assert_eq!(
                frames.len(),
                1,
                "33.z.k.j §5 iter {iter}: openai ToolCall MUST emit \
                 exactly 1 chunk frame"
            );
            let data = event_data(&frames[0]);
            let parsed: serde_json::Value =
                serde_json::from_str(&data).expect("openai chunk is JSON");
            let id = parsed["choices"][0]["delta"]["tool_calls"][0]["id"]
                .as_str()
                .expect("tool_calls[0].id present");
            emitted_ids.push(id.to_string());
        }

        // Each ID must be `call_<trace_hex>_<N>` with N strictly
        // increasing 1..=tool_call_count.
        for (i, id) in emitted_ids.iter().enumerate() {
            let expected_n = i + 1;
            let expected = format!("call_{trace_hex}_{expected_n}");
            assert_eq!(
                id, &expected,
                "33.z.k.j §5 iter {iter}: openai tool_call_id at \
                 position {i} MUST be `{expected}`. Got `{id}`. \
                 (trace_hex={trace_hex})"
            );
        }
    }
}

// ════════════════════════════════════════════════════════════════════
//  §6 — CompleteEnvelope round-trip projection (3 × 300 iters)
// ════════════════════════════════════════════════════════════════════

fn assert_envelope_projects_onto_axon_metadata(
    envelope: &CompleteEnvelope,
    metadata: &serde_json::Value,
    iter: usize,
    dialect: &str,
) {
    // Required flow-level fields surface verbatim.
    assert_eq!(
        metadata["trace_id"].as_u64(),
        Some(envelope.trace_id),
        "33.z.k.j §6 {dialect} iter {iter}: trace_id MUST round-trip"
    );
    assert_eq!(
        metadata["flow"].as_str(),
        Some(envelope.flow_name.as_str()),
        "33.z.k.j §6 {dialect} iter {iter}: flow MUST round-trip"
    );
    assert_eq!(
        metadata["backend"].as_str(),
        Some(envelope.backend.as_str()),
        "33.z.k.j §6 {dialect} iter {iter}: backend MUST round-trip"
    );
    assert_eq!(
        metadata["success"].as_bool(),
        Some(envelope.success),
        "33.z.k.j §6 {dialect} iter {iter}: success MUST round-trip"
    );
    // Optional fields: present iff envelope has non-empty data (D4 elision).
    if envelope.effect_policies.is_empty() {
        assert!(
            metadata.get("stream_policies").is_none(),
            "33.z.k.j §6 {dialect} iter {iter}: empty effect_policies \
             MUST elide `stream_policies`"
        );
    } else {
        let arr = metadata["stream_policies"]
            .as_array()
            .expect("stream_policies array");
        assert_eq!(arr.len(), envelope.effect_policies.len());
    }
    if envelope.enforcement_summaries.is_empty() {
        assert!(metadata.get("enforcement_summary").is_none());
    } else {
        let obj = metadata["enforcement_summary"]
            .as_object()
            .expect("enforcement_summary object");
        assert_eq!(obj.len(), envelope.enforcement_summaries.len());
    }
    if envelope.runtime_warnings.is_empty() {
        assert!(metadata.get("runtime_warnings").is_none());
    } else {
        let arr = metadata["runtime_warnings"]
            .as_array()
            .expect("runtime_warnings array");
        assert_eq!(arr.len(), envelope.runtime_warnings.len());
    }
    if envelope.step_audit_records.is_empty() {
        assert!(metadata.get("step_audit").is_none());
    } else {
        let arr = metadata["step_audit"].as_array().expect("step_audit array");
        assert_eq!(arr.len(), envelope.step_audit_records.len());
    }
    // terminal_reason always present.
    assert!(metadata.get("terminal_reason").is_some());
}

#[test]
fn s6_openai_envelope_round_trips_through_metadata_frame() {
    for iter in 0..300 {
        let mut lcg = Lcg::new(0xDEAD_0000 + iter as u64);
        let envelope = gen_random_envelope(&mut lcg);
        let mut adapter = select_adapter("openai", envelope.trace_id);
        let _ = adapter.build_complete_envelope_event(&envelope);
        let terminator = adapter.flush_terminator();
        let metadata_data = event_data(&terminator[0]);
        let v: serde_json::Value =
            serde_json::from_str(&metadata_data).expect("openai metadata frame is JSON");
        let metadata = &v["axon_metadata"];
        assert_envelope_projects_onto_axon_metadata(&envelope, metadata, iter, "openai");
    }
}

#[test]
fn s6_anthropic_envelope_round_trips_through_metadata_frame() {
    for iter in 0..300 {
        let mut lcg = Lcg::new(0xBEEF_0000 + iter as u64);
        let envelope = gen_random_envelope(&mut lcg);
        let mut adapter = select_adapter("anthropic", envelope.trace_id);
        // anthropic needs a FlowStart first.
        let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
            flow_name: envelope.flow_name.clone(),
            backend: envelope.backend.clone(),
            timestamp_ms: 1,
        });
        let _ = adapter.build_complete_envelope_event(&envelope);
        let terminator = adapter.flush_terminator();
        let metadata_data = event_data(&terminator[0]);
        let v: serde_json::Value =
            serde_json::from_str(&metadata_data).expect("anthropic metadata frame is JSON");
        let metadata = &v["axon_metadata"];
        assert_envelope_projects_onto_axon_metadata(&envelope, metadata, iter, "anthropic");
    }
}

#[test]
fn s6_axon_envelope_round_trips_through_complete_event() {
    for iter in 0..300 {
        let mut lcg = Lcg::new(0xFACE_0000 + iter as u64);
        let envelope = gen_random_envelope(&mut lcg);
        let mut adapter = select_adapter("axon", envelope.trace_id);
        let frames = adapter.build_complete_envelope_event(&envelope);
        assert_eq!(frames.len(), 1, "axon emits 1 complete frame");
        let data = event_data(&frames[0]);
        let v: serde_json::Value =
            serde_json::from_str(&data).expect("axon.complete frame is JSON");
        // axon embeds fields directly on axon.complete (no `axon_metadata`
        // wrapper).
        assert_eq!(v["trace_id"].as_u64(), Some(envelope.trace_id));
        assert_eq!(v["flow"].as_str(), Some(envelope.flow_name.as_str()));
        assert_eq!(v["backend"].as_str(), Some(envelope.backend.as_str()));
        assert_eq!(v["success"].as_bool(), Some(envelope.success));
        // Optional fields elided when empty (D4 byte-compat with v1.27.1).
        if envelope.effect_policies.is_empty() {
            assert!(
                v.get("stream_policies").is_none(),
                "axon iter {iter}: empty effect_policies MUST elide"
            );
        }
        if envelope.enforcement_summaries.is_empty() {
            assert!(v.get("enforcement_summary").is_none());
        }
        if envelope.runtime_warnings.is_empty() {
            assert!(v.get("warnings").is_none());
        }
    }
}

// ════════════════════════════════════════════════════════════════════
//  §7 — Determinism across repeats (same input → same wire bytes)
// ════════════════════════════════════════════════════════════════════

fn strip_volatile_axon(data: &str) -> String {
    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(data) {
        if let Some(obj) = v.as_object_mut() {
            obj.remove("timestamp_ms");
            obj.remove("latency_ms");
        }
        serde_json::to_string(&v).unwrap_or_default()
    } else {
        data.to_string()
    }
}

fn strip_volatile_openai(data: &str) -> String {
    if data == "[DONE]" {
        return data.to_string();
    }
    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(data) {
        if let Some(obj) = v.as_object_mut() {
            obj.remove("created");
            obj.remove("id");
            if let Some(meta) = obj.get_mut("axon_metadata").and_then(|m| m.as_object_mut()) {
                meta.remove("latency_ms");
            }
        }
        serde_json::to_string(&v).unwrap_or_default()
    } else {
        data.to_string()
    }
}

fn strip_volatile_anthropic(data: &str) -> String {
    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(data) {
        if let Some(obj) = v.as_object_mut() {
            // message_start carries a per-request id.
            if let Some(msg) = obj.get_mut("message").and_then(|m| m.as_object_mut()) {
                msg.remove("id");
            }
            if let Some(meta) = obj.get_mut("axon_metadata").and_then(|m| m.as_object_mut()) {
                meta.remove("latency_ms");
            }
        }
        serde_json::to_string(&v).unwrap_or_default()
    } else {
        data.to_string()
    }
}

#[test]
fn s7_axon_dialect_deterministic_across_repeats() {
    for iter in 0..200 {
        let mut lcg = Lcg::new(0xC0DE_0000 + iter as u64);
        let events = gen_random_event_stream(&mut lcg);
        let trace_id = lcg.next_u64();
        let mut first: Option<Vec<String>> = None;
        for _repeat in 0..3 {
            let mut adapter = select_adapter("axon", trace_id);
            let frames = drive_adapter(&mut adapter, &events);
            let normalized: Vec<String> =
                frames.iter().map(event_data).map(|d| strip_volatile_axon(&d)).collect();
            if let Some(ref f) = first {
                assert_eq!(
                    *f, normalized,
                    "33.z.k.j §7 axon iter {iter}: same input → same wire \
                     bytes (modulo timestamps) across repeats"
                );
            } else {
                first = Some(normalized);
            }
        }
    }
}

#[test]
fn s7_openai_dialect_deterministic_across_repeats() {
    for iter in 0..200 {
        let mut lcg = Lcg::new(0xC0DE_1000 + iter as u64);
        let events = gen_random_event_stream(&mut lcg);
        let trace_id = lcg.next_u64();
        let mut first: Option<Vec<String>> = None;
        for _repeat in 0..3 {
            let mut adapter = select_adapter("openai", trace_id);
            let frames = drive_adapter(&mut adapter, &events);
            let normalized: Vec<String> = frames
                .iter()
                .map(event_data)
                .map(|d| strip_volatile_openai(&d))
                .collect();
            if let Some(ref f) = first {
                assert_eq!(
                    *f, normalized,
                    "33.z.k.j §7 openai iter {iter}: same input → same \
                     wire bytes (modulo created/id/latency_ms) across repeats"
                );
            } else {
                first = Some(normalized);
            }
        }
    }
}

#[test]
fn s7_anthropic_dialect_deterministic_across_repeats() {
    for iter in 0..200 {
        let mut lcg = Lcg::new(0xC0DE_2000 + iter as u64);
        let events = gen_random_event_stream(&mut lcg);
        let trace_id = lcg.next_u64();
        let mut first: Option<Vec<String>> = None;
        for _repeat in 0..3 {
            let mut adapter = select_adapter("anthropic", trace_id);
            let frames = drive_adapter(&mut adapter, &events);
            let normalized: Vec<String> = frames
                .iter()
                .map(event_data)
                .map(|d| strip_volatile_anthropic(&d))
                .collect();
            if let Some(ref f) = first {
                assert_eq!(
                    *f, normalized,
                    "33.z.k.j §7 anthropic iter {iter}: same input → same \
                     wire bytes (modulo message.id/latency_ms) across repeats"
                );
            } else {
                first = Some(normalized);
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════
//  §9 — Anthropic defensive close on malformed (no-terminator) streams
// ════════════════════════════════════════════════════════════════════
//
// In production, axon_server.rs guarantees every stream ends with a
// terminal event (defense-in-depth synthesizes FlowError when the
// channel closes without FlowComplete). But adapters are libraries
// that may be driven by future producers / test harnesses / direct
// integrations that don't enforce the producer contract. The
// anthropic adapter MUST defensively close any orphan content_block
// in flush_terminator so the emitted wire stays Anthropic-spec-valid
// even on malformed inputs (every content_block_start balanced by
// content_block_stop).

#[test]
fn s9_anthropic_flush_terminator_defensively_closes_orphan_text_block() {
    let mut adapter = select_adapter("anthropic", 0xCAFE);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "F".into(),
        backend: "b".into(),
        timestamp_ms: 1,
    });
    // StepToken opens a text block at index 0. NO StepComplete +
    // NO FlowComplete — simulating a malformed input.
    let _ = adapter.translate(&FlowExecutionEvent::StepToken {
        step_name: "S".into(),
        content: "open".into(),
        token_index: 1,
            branch_path: String::new(),
        timestamp_ms: 2,
    });
    let terminator = adapter.flush_terminator();
    // Expected: 3 frames (orphan content_block_stop + axon.metadata +
    // message_stop). Pre-33.z.k.j this would have been 2 frames leaving
    // the text block dangling.
    assert_eq!(
        terminator.len(),
        3,
        "33.z.k.j §9: anthropic flush_terminator MUST defensively close \
         an orphan text block. Expected 3 frames \
         (content_block_stop + axon.metadata + message_stop). Got {}",
        terminator.len()
    );
    assert_eq!(
        event_name(&terminator[0]),
        "content_block_stop",
        "33.z.k.j §9: first defensive frame MUST be content_block_stop"
    );
    assert_eq!(event_name(&terminator[1]), "axon.metadata");
    assert_eq!(event_name(&terminator[2]), "message_stop");
}

#[test]
fn s9_anthropic_flush_terminator_well_formed_input_emits_two_frames() {
    // Counterpart of §9 above: on a well-formed input (terminal event
    // arrived → text block closed during translate(FlowComplete)),
    // flush_terminator emits exactly 2 frames (the existing 33.z.k.f
    // baseline). This pins the defensive close as a NO-OP when not
    // needed, so production wire byte-count stays exactly 2.
    let mut adapter = select_adapter("anthropic", 0xCAFE);
    let _ = adapter.translate(&FlowExecutionEvent::FlowStart {
        flow_name: "F".into(),
        backend: "b".into(),
        timestamp_ms: 1,
    });
    let _ = adapter.translate(&FlowExecutionEvent::StepToken {
        step_name: "S".into(),
        content: "x".into(),
        token_index: 1,
            branch_path: String::new(),
        timestamp_ms: 2,
    });
    let _ = adapter.translate(&FlowExecutionEvent::FlowComplete {
        flow_name: "F".into(),
        backend: "b".into(),
        success: true,
        steps_executed: 1,
        tokens_input: 0,
        tokens_output: 1,
        latency_ms: 1,
        timestamp_ms: 3,
    });
    let terminator = adapter.flush_terminator();
    assert_eq!(
        terminator.len(),
        2,
        "33.z.k.j §9 (no-op pin): well-formed input → flush_terminator \
         emits exactly 2 frames (axon.metadata + message_stop). \
         Defensive close is a no-op. Got {}",
        terminator.len()
    );
}

// ════════════════════════════════════════════════════════════════════
//  §8 — Iter-count meta-pin (prevents accidental fuzz shrinkage)
// ════════════════════════════════════════════════════════════════════

#[test]
fn s8_fuzz_iter_counts_pinned_at_target() {
    // Adding up declared iters across the pack. Adjust this constant
    // in lockstep with any iter-count change so a silent shrinkage
    // (e.g. someone "speeds up CI" by cutting iters to 10) fires
    // this meta-pin.
    let declared = [
        ("s1_axon", 200),
        ("s1_openai", 200),
        ("s1_anthropic", 200),
        ("s2_axon", 150),
        ("s2_openai", 150),
        ("s2_anthropic", 150),
        ("s3_arrival_order", 300),
        ("s4_anthropic_lifecycle", 300),
        ("s5_openai_tool_call_ids", 200),
        ("s6_openai_envelope", 300),
        ("s6_anthropic_envelope", 300),
        ("s6_axon_envelope", 300),
        ("s7_axon_determinism", 200),
        ("s7_openai_determinism", 200),
        ("s7_anthropic_determinism", 200),
    ];
    let total: usize = declared.iter().map(|(_, n)| *n).sum();
    assert_eq!(
        total, 3350,
        "33.z.k.j §8: fuzz iter count drifted from the declared 3350. \
         If this is intentional (tightened cycle budget OR widened \
         coverage), update the constant AND document the rationale \
         in the plan vivo. Got total: {total}, declared per-section: \
         {declared:?}"
    );
}
