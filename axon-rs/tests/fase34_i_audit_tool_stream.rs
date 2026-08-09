//! §Fase 34.i (v1.29.0) — Audit-row extension end-to-end tests.
//!
//! 34.i extends [`StepAuditRecord`] with FOUR new optional fields
//! capturing per-step tool-stream provenance:
//!
//! - `tool_name: Option<String>` — `Some(name)` for disjunct (b)
//!   `apply: <stream-tool>` rows; `None` for LLM-side (disjunct a)
//!   + algebraic-effect Yield (disjunct d).
//! - `tool_chunks_emitted: Option<u64>` — count of `ToolChunk`s the
//!   source stream produced (distinct from `tokens_emitted` which
//!   counts non-empty deltas reaching the wire post-policy).
//! - `tool_output_hash_hex: Option<String>` — SHA-256 of the
//!   concatenated tool-stream deltas. Same scope as
//!   `output_hash_hex` today; explicit field provides D6 provenance
//!   and lets a future fase diverge the two (raw chunks pre-
//!   degrader vs post-policy wire emission).
//! - `tool_terminator_kind: Option<String>` — closed-catalog slug
//!   `"stop" | "error" | "cancelled"` carrying the final
//!   `ToolFinishReason` variant.
//!
//! **D4 byte-compat invariant**: serde elides `None` via
//! `skip_serializing_if = "Option::is_none"`. Legacy LLM-side rows
//! (where all four are `None`) serialize byte-identical to the
//! pre-34.i wire shape. Streaming-tool rows include the new fields.
//! This pack asserts BOTH directions: the new fields are populated
//! correctly on streaming-tool rows AND the new fields are elided
//! from non-streaming-tool rows.
//!
//! 14 tests across 4 sections.

#![allow(clippy::needless_return)]

use std::sync::Arc;

use axon::axonendpoint_replay::StepAuditRecord;
use axon::cancel_token::CancellationFlag;
use axon::effects::EffectRuntime;
use axon::effects::ir::{
    IRHandlerClause, IRHandlerFrame, IRPerform, IRResume, Instruction,
};
use axon::flow_dispatcher::effects_bridge::{
    bridge_effect_stream_yield, bridge_effect_stream_yield_unified,
};
use axon::flow_dispatcher::pure_shape::run_step;
use axon::flow_dispatcher::{DispatchCtx, NodeOutcome};
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::*;
use axon::stream_effect::BackpressurePolicy;
use axon::tool_registry::{ToolEntry, ToolRegistry, ToolSource};
use tokio::sync::mpsc;

// ────────────────────────────────────────────────────────────────────
//  Scaffolding
// ────────────────────────────────────────────────────────────────────

fn fresh_ctx() -> (
    DispatchCtx,
    mpsc::UnboundedReceiver<FlowExecutionEvent>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = DispatchCtx::new(
        "TestFlow",
        "stub",
        "",
        CancellationFlag::new(),
        tx,
    );
    (ctx, rx)
}

fn fresh_ctx_with_registry(
    registry: Arc<ToolRegistry>,
) -> (
    DispatchCtx,
    mpsc::UnboundedReceiver<FlowExecutionEvent>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = DispatchCtx::new(
        "TestFlow",
        "stub",
        "",
        CancellationFlag::new(),
        tx,
    )
    .with_tool_registry(registry);
    (ctx, rx)
}

fn step_with_apply(name: &str, ask: &str, apply_ref: &str) -> IRStep {
    IRStep {
        node_type: "step",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        persona_ref: String::new(),
        given: String::new(),
        ask: ask.into(),
        use_tool: None,
        probe: None,
        reason: None,
        weave: None,
        output_type: String::new(),
        confidence_floor: None,
        navigate_ref: String::new(),
        apply_ref: apply_ref.into(),
        pix_ops: Vec::new(),
        stream: None,
        guards: Vec::new(),
        requires_context: None,        now_tz: None,        body: Vec::new(),
    }
}

fn stream_tool_entry(name: &str, effect_row: Vec<&str>) -> ToolEntry {
    let row: Vec<String> = effect_row.into_iter().map(String::from).collect();
    let is_streaming = axon::tool_registry::derive_is_streaming(&row);
    ToolEntry {
        name: name.into(),
        provider: "stub_stream".into(),
        timeout: String::new(),
        runtime: String::new(),
        resource_ref: String::new(),
        substrate: None,
        capacity: None,
        sandbox: None,
        max_results: None,
        output_schema: String::new(),
        effect_row: row,
        parameters: Vec::new(),
        secret: String::new(),
        secret_partition: String::new(),
        source: ToolSource::Program,
        is_streaming,
        scrape: None,
    }
}

fn perform_yield(arg: &str) -> Instruction {
    Instruction::Perform(IRPerform {
        effect_name: "Stream".into(),
        operation_name: "Yield".into(),
        arguments: vec![arg.into()],
        state_id: 0,
        resume_label: String::new(),
    })
}

fn handle_stream_with_body(body: Vec<Instruction>) -> Vec<Instruction> {
    vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["Stream".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Yield".into(),
            parameter_names: vec!["v".into()],
            body: vec![Instruction::Resume(IRResume {
                value_expr: String::new(),
                frame_id: 0,
            })],
            source_line: 0,
            source_column: 0,
        }],
        body,
        frame_id: 0,
        body_states: Vec::new(),
        source_line: 0,
        source_column: 0,
    })]
}

async fn drain_audit_rows(ctx: &DispatchCtx) -> Vec<StepAuditRecord> {
    ctx.step_audit_records.lock().await.clone()
}

// ════════════════════════════════════════════════════════════════════
//  §1 — Disjunct (b) tool-stream rows populate all 4 fields × 5
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s1_disjunct_b_audit_row_carries_tool_name_from_registry_entry() {
    let mut reg = ToolRegistry::new();
    reg.register(stream_tool_entry("SearchStreamer", vec!["stream:drop_oldest"]));
    let (mut ctx, _rx) = fresh_ctx_with_registry(Arc::new(reg));
    let s = step_with_apply("Search", "query", "SearchStreamer");
    let outcome = run_step(&s, &mut ctx).await;
    assert!(matches!(outcome, Ok(NodeOutcome::Completed { .. })));

    let rows = drain_audit_rows(&ctx).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].tool_name.as_deref(),
        Some("SearchStreamer"),
        "tool_name MUST be Some(<registry entry name>) for disjunct (b)"
    );
}

#[tokio::test]
async fn s1_disjunct_b_audit_row_carries_tool_chunks_emitted_count() {
    let mut reg = ToolRegistry::new();
    reg.register(stream_tool_entry("FourFrameTool", vec!["stream:fail"]));
    let (mut ctx, _rx) = fresh_ctx_with_registry(Arc::new(reg));
    let s = step_with_apply("S", "args", "FourFrameTool");
    let _ = run_step(&s, &mut ctx).await;
    let rows = drain_audit_rows(&ctx).await;
    // StubStreamingTool emits 4 frames (3 intermediate + 1 terminator).
    // The unified handler's summary.chunks_pushed = 4 (when no policy
    // overflow fires) — that's what tool_chunks_emitted captures.
    assert_eq!(rows[0].tool_chunks_emitted, Some(4));
    // tokens_emitted is the count of NON-EMPTY DELTAS reaching wire =
    // 3 (the 3 intermediate frames; terminator has empty delta).
    assert_eq!(rows[0].tokens_emitted, 3);
    assert!(
        rows[0].tool_chunks_emitted.unwrap() >= rows[0].tokens_emitted,
        "tool_chunks_emitted ({:?}) must be ≥ tokens_emitted ({}) — \
         empty deltas + terminator don't reach wire",
        rows[0].tool_chunks_emitted,
        rows[0].tokens_emitted,
    );
}

#[tokio::test]
async fn s1_disjunct_b_audit_row_carries_tool_output_hash_hex_matching_output_hash() {
    let mut reg = ToolRegistry::new();
    reg.register(stream_tool_entry("HashTool", vec!["stream:drop_oldest"]));
    let (mut ctx, _rx) = fresh_ctx_with_registry(Arc::new(reg));
    let s = step_with_apply("H", "x", "HashTool");
    let _ = run_step(&s, &mut ctx).await;
    let rows = drain_audit_rows(&ctx).await;
    assert_eq!(rows.len(), 1);
    // 34.i scope: tool_output_hash_hex is byte-equal to output_hash_hex.
    // Future fases may diverge them (raw pre-degrader vs post-policy wire).
    assert_eq!(
        rows[0].tool_output_hash_hex.as_ref(),
        Some(&rows[0].output_hash_hex)
    );
    // Hash is 64 hex chars (SHA-256 hex).
    assert_eq!(rows[0].tool_output_hash_hex.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn s1_disjunct_b_audit_row_terminator_kind_stop_for_natural_end() {
    let mut reg = ToolRegistry::new();
    reg.register(stream_tool_entry("StopTool", vec!["stream:drop_oldest"]));
    let (mut ctx, _rx) = fresh_ctx_with_registry(Arc::new(reg));
    let s = step_with_apply("S", "x", "StopTool");
    let _ = run_step(&s, &mut ctx).await;
    let rows = drain_audit_rows(&ctx).await;
    assert_eq!(rows[0].tool_terminator_kind.as_deref(), Some("stop"));
}

#[tokio::test]
async fn s1_disjunct_b_audit_row_all_four_fields_populated_simultaneously() {
    // Bundled invariant pin — a single streaming-tool row has all
    // four fields populated together, never partially.
    let mut reg = ToolRegistry::new();
    reg.register(stream_tool_entry("BundleTool", vec!["stream:degrade_quality"]));
    let (mut ctx, _rx) = fresh_ctx_with_registry(Arc::new(reg));
    let s = step_with_apply("S", "x", "BundleTool");
    let _ = run_step(&s, &mut ctx).await;
    let rows = drain_audit_rows(&ctx).await;
    let r = &rows[0];
    assert!(r.tool_name.is_some());
    assert!(r.tool_chunks_emitted.is_some());
    assert!(r.tool_output_hash_hex.is_some());
    assert!(r.tool_terminator_kind.is_some());
    assert_eq!(r.effect_policy_applied.as_deref(), Some("degrade_quality"));
}

// ════════════════════════════════════════════════════════════════════
//  §2 — Disjunct (d) Yield + legacy bridge populate 3 of 4 fields × 3
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s2_disjunct_d_unified_yield_carries_tool_metadata_with_no_tool_name() {
    // Algebraic-effect bridge has no Tool trait impl backing the
    // stream — `tool_name` stays `None` while the other three
    // fields populate from the unified handler's summary.
    let block = handle_stream_with_body(vec![
        perform_yield("a"),
        perform_yield("b"),
        perform_yield("c"),
    ]);
    let mut runtime = EffectRuntime::new();
    let (mut ctx, _rx) = fresh_ctx();
    let _ = bridge_effect_stream_yield_unified(
        &block,
        &mut runtime,
        "YieldStep",
        None,
        &mut ctx,
    )
    .await
    .expect("ok");

    let rows = drain_audit_rows(&ctx).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tool_name, None, "disjunct d → no tool name");
    assert!(rows[0].tool_chunks_emitted.is_some());
    assert!(rows[0].tool_output_hash_hex.is_some());
    assert_eq!(rows[0].tool_terminator_kind.as_deref(), Some("stop"));
    assert_eq!(rows[0].tokens_emitted, 3);
}

#[tokio::test]
async fn s2_legacy_bridge_effect_stream_yield_populates_provenance_with_stop_terminator() {
    // D9 backwards-compat: the legacy bridge populates the 3
    // provenance fields with sensible defaults so post-34.i replay
    // queries get useful data even from un-migrated adopters.
    let block = handle_stream_with_body(vec![
        perform_yield("legacy-a"),
        perform_yield("legacy-b"),
    ]);
    let mut runtime = EffectRuntime::new();
    let (mut ctx, _rx) = fresh_ctx();
    let _ = bridge_effect_stream_yield(&block, &mut runtime, "LegacyStep", &mut ctx)
        .await
        .expect("ok");

    let rows = drain_audit_rows(&ctx).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tool_name, None);
    assert_eq!(rows[0].tool_chunks_emitted, Some(2));
    assert!(rows[0].tool_output_hash_hex.is_some());
    assert_eq!(rows[0].tool_terminator_kind.as_deref(), Some("stop"));
    // Legacy path: counters stay 0 (no policy enforcement).
    assert_eq!(rows[0].chunks_dropped, 0);
    assert_eq!(rows[0].chunks_degraded, 0);
    assert_eq!(rows[0].effect_policy_applied, None);
}

#[tokio::test]
async fn s2_disjunct_d_with_policy_carries_policy_slug_plus_provenance() {
    let block = handle_stream_with_body((0..10).map(|i| perform_yield(&format!("y{i}"))).collect());
    let mut runtime = EffectRuntime::new();
    let (mut ctx, _rx) = fresh_ctx();
    let _ = bridge_effect_stream_yield_unified(
        &block,
        &mut runtime,
        "PolicyYield",
        Some(BackpressurePolicy::PauseUpstream),
        &mut ctx,
    )
    .await
    .expect("ok");

    let rows = drain_audit_rows(&ctx).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].effect_policy_applied.as_deref(),
        Some("pause_upstream")
    );
    assert_eq!(rows[0].tool_name, None); // No Tool impl
    // tool_chunks_emitted = chunks_pushed = 10 yields + 1 synthetic terminator
    assert_eq!(rows[0].tool_chunks_emitted, Some(11));
    assert_eq!(rows[0].tool_terminator_kind.as_deref(), Some("stop"));
}

// ════════════════════════════════════════════════════════════════════
//  §3 — D4 byte-compat: None fields elided + populated fields surface × 4
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s3_legacy_llm_side_record_serializes_byte_identical_to_pre_34_i() {
    // LLM-side `run_pure_shape` constructs a row with all 4 new
    // fields = None. The serde elision MUST produce a JSON object
    // with EXACTLY the pre-34.i field set (no new keys, no
    // explicit nulls).
    let row = StepAuditRecord {
        step_name: "Generate".into(),
        step_index: 0,
        success: true,
        tokens_emitted: 5,
        output_hash_hex: "abc".repeat(21) + "x", // 64 chars
        effect_policy_applied: None,
        chunks_dropped: 0,
        chunks_degraded: 0,
        timestamp_ms: 1715648400500,
        tool_name: None,
        substrate: None,
        tool_chunks_emitted: None,
        tool_output_hash_hex: None,
        tool_terminator_kind: None,
        anchor_breaches: Vec::new(),
    };
    let json = serde_json::to_value(&row).unwrap();
    let obj = json.as_object().expect("object");
    // Expected pre-34.i field set:
    let expected_keys = [
        "step_name",
        "step_index",
        "success",
        "tokens_emitted",
        "output_hash_hex",
        "effect_policy_applied",
        "chunks_dropped",
        "chunks_degraded",
        "timestamp_ms",
    ];
    let actual_keys: Vec<&str> = obj.keys().map(|s| s.as_str()).collect();
    for k in expected_keys {
        assert!(actual_keys.contains(&k), "missing field {k}");
    }
    // No tool_* keys present (elided).
    for k in [
        "tool_name",
        "tool_chunks_emitted",
        "tool_output_hash_hex",
        "tool_terminator_kind",
    ] {
        assert!(
            !actual_keys.contains(&k),
            "field {k} should be elided when None, but found in {actual_keys:?}"
        );
    }
}

#[tokio::test]
async fn s3_streaming_tool_record_includes_all_four_tool_fields_in_json() {
    let row = StepAuditRecord {
        step_name: "Stream".into(),
        step_index: 0,
        success: true,
        tokens_emitted: 3,
        output_hash_hex: "abcdef".repeat(10) + "1234", // 64 chars
        effect_policy_applied: Some("drop_oldest".into()),
        chunks_dropped: 1,
        chunks_degraded: 0,
        timestamp_ms: 1715648400500,
        tool_name: Some("SearchTool".into()),
        substrate: None,
        tool_chunks_emitted: Some(4),
        tool_output_hash_hex: Some("abcdef".repeat(10) + "1234"),
        tool_terminator_kind: Some("stop".into()),
        anchor_breaches: Vec::new(),
    };
    let json = serde_json::to_value(&row).unwrap();
    let obj = json.as_object().expect("object");
    assert_eq!(obj["tool_name"], "SearchTool");
    assert_eq!(obj["tool_chunks_emitted"], 4);
    assert_eq!(obj["tool_terminator_kind"], "stop");
    assert_eq!(obj["tool_output_hash_hex"].as_str().unwrap().len(), 64);
}

#[tokio::test]
async fn s3_deserialize_legacy_json_without_tool_fields_succeeds_with_none() {
    // Wire shape from pre-34.i deployments lands at a 34.i runtime
    // → deserialize MUST succeed + the new fields default to None.
    let legacy_json = serde_json::json!({
        "step_name": "Generate",
        "step_index": 0,
        "success": true,
        "tokens_emitted": 5,
        "output_hash_hex": "0123456789abcdef".repeat(4),
        "effect_policy_applied": null,
        "chunks_dropped": 0,
        "chunks_degraded": 0,
        "timestamp_ms": 1715648400500_u64,
    });
    let row: StepAuditRecord = serde_json::from_value(legacy_json).unwrap();
    assert_eq!(row.tool_name, None);
    assert_eq!(row.tool_chunks_emitted, None);
    assert_eq!(row.tool_output_hash_hex, None);
    assert_eq!(row.tool_terminator_kind, None);
    assert_eq!(row.step_name, "Generate");
}

#[tokio::test]
async fn s3_round_trip_streaming_tool_record_preserves_all_four_fields() {
    let original = StepAuditRecord {
        step_name: "Round".into(),
        step_index: 7,
        success: false,
        tokens_emitted: 0,
        output_hash_hex: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
        effect_policy_applied: Some("fail".into()),
        chunks_dropped: 0,
        chunks_degraded: 0,
        timestamp_ms: 1715648400500,
        tool_name: Some("FailingTool".into()),
        substrate: None,
        tool_chunks_emitted: Some(0),
        tool_output_hash_hex: Some(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
        ),
        tool_terminator_kind: Some("error".into()),
        anchor_breaches: Vec::new(),
    };
    let json = serde_json::to_string(&original).unwrap();
    let back: StepAuditRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(back.tool_name, original.tool_name);
    assert_eq!(back.tool_chunks_emitted, original.tool_chunks_emitted);
    assert_eq!(back.tool_output_hash_hex, original.tool_output_hash_hex);
    assert_eq!(back.tool_terminator_kind, original.tool_terminator_kind);
    assert_eq!(back.success, false);
}

// ════════════════════════════════════════════════════════════════════
//  §4 — Replay endpoint surfaces extended fields + composition × 2
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s4_sequential_streaming_steps_each_have_independent_tool_metadata() {
    // Two consecutive tool-stream steps with DIFFERENT tool names →
    // audit rows carry each step's own tool_name + provenance.
    let mut reg = ToolRegistry::new();
    reg.register(stream_tool_entry("FirstTool", vec!["stream:drop_oldest"]));
    reg.register(stream_tool_entry("SecondTool", vec!["stream:fail"]));
    let (mut ctx, _rx) = fresh_ctx_with_registry(Arc::new(reg));
    let s1 = step_with_apply("First", "a", "FirstTool");
    let s2 = step_with_apply("Second", "b", "SecondTool");
    let _ = run_step(&s1, &mut ctx).await;
    let _ = run_step(&s2, &mut ctx).await;
    let rows = drain_audit_rows(&ctx).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].tool_name.as_deref(), Some("FirstTool"));
    assert_eq!(rows[1].tool_name.as_deref(), Some("SecondTool"));
    assert_eq!(rows[0].step_index, 0);
    assert_eq!(rows[1].step_index, 1);
    // Each row's provenance hash is INDEPENDENT (different tools +
    // different args produce different deltas + different hashes).
    let h0 = rows[0].tool_output_hash_hex.as_ref().unwrap();
    let h1 = rows[1].tool_output_hash_hex.as_ref().unwrap();
    assert_ne!(h0, h1, "different tools+args MUST yield different hashes");
}
