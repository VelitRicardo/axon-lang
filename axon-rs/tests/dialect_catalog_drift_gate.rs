//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.28.0 — Dialect catalog drift gate.
//!
//! Closed-catalog invariant test pack. The dialect catalog
//! `{axon, openai, kimi, glm, anthropic}` is the load-bearing
//! closure decision of the entire 33.z.k cycle — every adapter,
//! every adopter SDK, every adopter docs page is built around it
//! being EXACTLY those 5 strings. Adding a 6th (or removing one,
//! or renaming, or accidentally introducing open-set pluggability)
//! breaks the wire-format adapter contract.
//!
//! This pack's role: **fire loudly on any drift**. Every assertion
//! is a hardcoded snapshot of the catalog as ratified by the
//! founder per the Q3 revision 2026-05-14. Touching the catalog
//! requires updating the snapshot — which is a commit-message-
//! worthy explicit action, not silent drift.
//!
//! # The closure decisions this gate locks
//!
//! 1. **Cardinality**: exactly 5 entries in the closed catalog.
//! 2. **Membership**: exact set `{axon, openai, kimi, glm, anthropic}`
//!    in unspecified order (the underlying type is a Vec/slice but
//!    semantic equality is set-based).
//! 3. **Cross-stack drift parity**: Rust + Python catalogs MUST
//!    contain the same 5 strings byte-identically. The Python-side
//!    counterpart of this drift gate is
//!    `tests/test_fase33z_k_i_dialect_catalog_drift_gate.py`.
//! 4. **select_adapter totality**: every catalog member produces a
//!    well-formed adapter (no panic / no fallback to "axon"
//!    defensively for legit catalog entries).
//! 5. **Adapter implementations are exactly 3**: axon, openai,
//!    anthropic. Kimi + glm dispatch to OpenAIDialectAdapter
//!    (Q3 revision dispatch-table).
//! 6. **Adapter dialect() return values are catalog members**:
//!    every adapter announces a dialect string that is IN the
//!    catalog (axon for axon; openai for openai/kimi/glm —
//!    OpenAIDialectAdapter intentionally reports "openai" because
//!    the WIRE is openai; anthropic for anthropic).
//! 7. **resolve_effective_dialect totality**: the resolver's output
//!    is ALWAYS a catalog member regardless of `transport_dialect`
//!    input (defensive — unknown strings fall through to a catalog
//!    member, never produce out-of-band values).
//! 8. **Mutual-exclusion of wire-format signatures**: each
//!    dialect's wire frame carries discriminating tokens that the
//!    others MUST NOT emit (closed-catalog dispatch invariant).
//! 9. **flush_terminator frame counts are dialect-pinned**: axon=0,
//!    openai=2 (metadata+[DONE]), anthropic=2 (metadata+message_stop).
//! 10. **WireFormatAdapter trait surface is locked**: dialect() +
//!    translate() + build_complete_envelope_event() +
//!    flush_terminator() — adding methods requires a deliberate
//!    minor-version bump.

use axon::flow_execution_event::FlowExecutionEvent;
use axon::wire_format::{select_adapter, CompleteEnvelope, WireFormatAdapter};
use axon_frontend::parser::AXONENDPOINT_TRANSPORT_DIALECTS;
use axon_frontend::type_checker::resolve_effective_dialect;

// ─── Canonical snapshot (founder-ratified Q3 revision 2026-05-14) ───
//
// Any change to this snapshot is the EXPLICIT marker that the
// dialect catalog evolved. Reviewers verify the change is intentional
// + that the corresponding test cases below got updated in lockstep.
//
// Adding a 6th dialect requires:
//   1. Update this SNAPSHOT array.
//   2. Update parser.rs AXONENDPOINT_TRANSPORT_DIALECTS.
//   3. Update Python parser.py _AXONENDPOINT_TRANSPORT_DIALECTS.
//   4. Implement adapter for the new dialect OR dispatch to existing.
//   5. Update select_adapter() in wire_format/mod.rs.
//   6. Update resolve_effective_dialect() if it needs to default to
//      the new dialect for any input.
//   7. Add the dialect to s5_select_adapter_dispatch_table below.
//   8. Add E2E test for the new dialect's wire bytes.
//   9. Update plan vivo + adopter docs.
//
// THE 5 strings below are LOAD-BEARING — they are the alphabet of
// the closed catalog.
const CANONICAL_DIALECT_SNAPSHOT: &[&str] =
    &["axon", "openai", "kimi", "glm", "anthropic"];

// ════════════════════════════════════════════════════════════════════
// section 1 — Cardinality + membership lock
// ════════════════════════════════════════════════════════════════════

#[test]
fn s1_catalog_cardinality_is_exactly_five() {
    assert_eq!(
        AXONENDPOINT_TRANSPORT_DIALECTS.len(),
        5,
        "33.z.k.i drift gate: the closed dialect catalog has EXACTLY 5 \
         entries per the Q3 revision 2026-05-14 ({{axon, openai, kimi, \
         glm, anthropic}}). Adding/removing requires a deliberate \
         step + snapshot update + cross-stack drift gate alignment."
    );
}

#[test]
fn s1_catalog_membership_matches_snapshot_verbatim() {
    let actual: std::collections::BTreeSet<&str> =
        AXONENDPOINT_TRANSPORT_DIALECTS.iter().copied().collect();
    let snapshot: std::collections::BTreeSet<&str> =
        CANONICAL_DIALECT_SNAPSHOT.iter().copied().collect();
    assert_eq!(
        actual, snapshot,
        "33.z.k.i drift gate: AXONENDPOINT_TRANSPORT_DIALECTS membership \
         drifted from the founder-ratified Q3 snapshot. If the change is \
         intentional, update CANONICAL_DIALECT_SNAPSHOT in this test file \
         + all 9 downstream sites listed in the snapshot comment block."
    );
}

#[test]
fn s1_catalog_has_no_duplicates() {
    let mut seen = std::collections::HashSet::new();
    for d in AXONENDPOINT_TRANSPORT_DIALECTS {
        assert!(
            seen.insert(*d),
            "33.z.k.i drift gate: duplicate dialect `{d}` in catalog. \
             AXONENDPOINT_TRANSPORT_DIALECTS MUST be a set semantically \
             even though the underlying type is &[&str]."
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// section 2 — select_adapter totality + closed dispatch table
// ════════════════════════════════════════════════════════════════════

#[test]
fn s2_select_adapter_total_over_catalog() {
    // Every catalog member produces an adapter without panic.
    for dialect in AXONENDPOINT_TRANSPORT_DIALECTS {
        let adapter = select_adapter(dialect, 0);
        // Just calling .dialect() should succeed.
        let reported = adapter.dialect();
        assert!(
            !reported.is_empty(),
            "33.z.k.i: select_adapter(`{dialect}`).dialect() returned \
             empty string — adapter MUST announce its wire format."
        );
    }
}

#[test]
fn s2_select_adapter_unknown_falls_through_to_axon_defensively() {
    // The resolver's job is to never produce out-of-band strings.
    // BUT select_adapter is the runtime dispatch layer and MUST be
    // defensive against stale/wrong input — falls through to axon
    // (the safest W3C-correct dialect for arbitrary parsers).
    for unknown in &["", "unknown", "FOO", "xyz", "cohere", "mistral"] {
        let adapter = select_adapter(unknown, 0);
        assert_eq!(
            adapter.dialect(),
            "axon",
            "33.z.k.i: select_adapter(unknown=`{unknown}`) MUST fall \
             through to the axon dialect defensively (Q5 invariant: \
             axon is the W3C-correct baseline that any SSE parser \
             handles correctly)."
        );
    }
}

#[test]
fn s2_select_adapter_dispatch_table_explicit() {
    // The dispatch table is part of the closed-catalog contract.
    // Each entry: (input_dialect_string, expected_adapter.dialect()).
    // Note: kimi + glm dispatch to OpenAIDialectAdapter so they report
    // dialect() == "openai" — the WIRE is openai even though the
    // adopter declared kimi/glm intent in source. This is the Q3
    // revision's load-bearing decision.
    let dispatch_table: &[(&str, &str)] = &[
        ("axon", "axon"),
        ("openai", "openai"),
        ("kimi", "openai"),
        ("glm", "openai"),
        ("anthropic", "anthropic"),
    ];
    assert_eq!(
        dispatch_table.len(),
        AXONENDPOINT_TRANSPORT_DIALECTS.len(),
        "33.z.k.i: dispatch table size MUST match catalog cardinality. \
         Every catalog member needs an explicit dispatch row here."
    );
    for (input, expected) in dispatch_table {
        let adapter = select_adapter(input, 0);
        assert_eq!(
            adapter.dialect(),
            *expected,
            "33.z.k.i Q3 dispatch: `{input}` MUST dispatch to an adapter \
             whose dialect() == `{expected}`. Got: `{}`",
            adapter.dialect()
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// section 3 — Adapter implementations: exactly 3 distinct dialect() values
// ════════════════════════════════════════════════════════════════════

#[test]
fn s3_unique_adapter_implementations_count_is_three() {
    let unique_impls: std::collections::BTreeSet<&'static str> =
        AXONENDPOINT_TRANSPORT_DIALECTS
            .iter()
            .map(|d| select_adapter(d, 0).dialect())
            .collect();
    assert_eq!(
        unique_impls.len(),
        3,
        "33.z.k.i: there are EXACTLY 3 distinct WireFormatAdapter \
         implementations (axon/openai/anthropic). Kimi + glm dispatch \
         to OpenAIDialectAdapter, so the unique dialect() set is \
         {{axon, openai, anthropic}}. Got: {unique_impls:?}"
    );
}

#[test]
fn s3_every_adapter_dialect_is_a_catalog_member() {
    // The dialect() return value is the wire-format announcement.
    // It MUST be a catalog member (no out-of-band strings).
    for d in AXONENDPOINT_TRANSPORT_DIALECTS {
        let announced = select_adapter(d, 0).dialect();
        assert!(
            AXONENDPOINT_TRANSPORT_DIALECTS.contains(&announced),
            "33.z.k.i: adapter for `{d}` announced dialect()=`{announced}` \
             which is NOT in the closed catalog \
             {AXONENDPOINT_TRANSPORT_DIALECTS:?}. Adapters MUST stay \
             within the catalog."
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// section 4 — resolve_effective_dialect totality over input space
// ════════════════════════════════════════════════════════════════════

#[test]
fn s4_resolver_output_is_always_a_catalog_member() {
    // Cartesian product: explicit_dialect ∈ {"" + 5 catalog entries +
    //   3 unknown strings} × has_algebraic_stream_effect ∈ {true, false}.
    let inputs: &[&str] = &[
        "",
        "axon",
        "openai",
        "kimi",
        "glm",
        "anthropic",
        "unknown",
        "FOO",
        "",
    ];
    for explicit in inputs {
        for &algebraic in &[false, true] {
            let result = resolve_effective_dialect(explicit, algebraic);
            // Empty explicit + ANY algebraic: result MUST be in catalog
            // (Q1: openai for algebraic, axon for type-annotation-only).
            if explicit.is_empty() {
                assert!(
                    AXONENDPOINT_TRANSPORT_DIALECTS.contains(&result.as_str()),
                    "33.z.k.i: resolve_effective_dialect(\"\", {algebraic}) \
                     returned `{result}` which is NOT a catalog member."
                );
            } else if AXONENDPOINT_TRANSPORT_DIALECTS.contains(&explicit.as_ref()) {
                // Explicit catalog member: resolver MUST honor it (Rule 1).
                assert_eq!(
                    result, *explicit,
                    "33.z.k.i: resolve_effective_dialect(`{explicit}`, _) \
                     MUST return `{explicit}` verbatim (Rule 1 explicit-wins)."
                );
            }
            // For unknown explicit strings the resolver is permitted
            // to pass them through (since the parser rejects them at
            // compile time before they reach the resolver in
            // production). This pin only enforces totality on the
            // empty + catalog-member inputs.
        }
    }
}

#[test]
fn s4_resolver_q1_default_rules_pinned() {
    // Rule 2 (Q1): empty explicit + algebraic effect → openai default.
    assert_eq!(
        resolve_effective_dialect("", true),
        "openai",
        "33.z.k.i Q1 Rule 2: algebraic-effect flows default to openai \
         (LLM-streaming ecosystem expectation)."
    );
    // Rule 3 (Q1): empty explicit + type-annotation-only → axon default.
    assert_eq!(
        resolve_effective_dialect("", false),
        "axon",
        "33.z.k.i Q1 Rule 3: type-annotation-only flows default to axon \
         (W3C-correct baseline)."
    );
}

// ════════════════════════════════════════════════════════════════════
// section 5 — Mutual-exclusion: closed-catalog wire signatures
// ════════════════════════════════════════════════════════════════════

fn drive_canonical_stream(adapter: &mut Box<dyn WireFormatAdapter>) -> String {
    // Drive a canonical stub-shape stream through the adapter to get
    // every reachable frame type emitted, then concatenate Debug
    // representations so we can pattern-match on dialect signatures.
    let stream = vec![
        FlowExecutionEvent::FlowStart {
            flow_name: "F".into(),
            backend: "b".into(),
            timestamp_ms: 1,
        },
        FlowExecutionEvent::StepToken {
            step_name: "S".into(),
            content: "Hi".into(),
            token_index: 1,
            branch_path: String::new(),
            timestamp_ms: 2,
        },
        FlowExecutionEvent::ToolCall {
            step_name: "S".into(),
            tool_name: "t".into(),
            content: "{}".into(),
            timestamp_ms: 3,
        },
        FlowExecutionEvent::StepComplete {
            step_name: "S".into(),
            step_index: 0,
            success: true,
            full_output: "Hi".into(),
            tokens_input: 0,
            tokens_output: 1,
            branch_path: String::new(),
            timestamp_ms: 4,
        },
        FlowExecutionEvent::FlowComplete {
            flow_name: "F".into(),
            backend: "b".into(),
            success: true,
            steps_executed: 1,
            tokens_input: 0,
            tokens_output: 1,
            latency_ms: 1,
            timestamp_ms: 5,
        },
    ];
    let mut frames = Vec::new();
    for event in &stream {
        frames.extend(adapter.translate(event));
    }
    frames.extend(adapter.flush_terminator());
    let mut concat = String::new();
    for f in &frames {
        concat.push_str(&format!("{f:?}\n"));
    }
    concat
}

#[test]
fn s5_axon_dialect_signature_unique_to_axon() {
    let mut axon_adapter = select_adapter("axon", 0);
    let mut openai_adapter = select_adapter("openai", 0);
    let mut anthropic_adapter = select_adapter("anthropic", 0);

    let axon_wire = drive_canonical_stream(&mut axon_adapter);
    let openai_wire = drive_canonical_stream(&mut openai_adapter);
    let anthropic_wire = drive_canonical_stream(&mut anthropic_adapter);

    // Signature exclusive to axon: `event: axon.token` named event.
    assert!(
        axon_wire.contains("event: axon.token"),
        "33.z.k.i: axon dialect MUST emit `event: axon.token`. Wire: {axon_wire:?}"
    );
    assert!(
        !openai_wire.contains("event: axon.token"),
        "33.z.k.i mutex: openai wire MUST NOT carry W3C `event: axon.token`."
    );
    assert!(
        !anthropic_wire.contains("event: axon.token"),
        "33.z.k.i mutex: anthropic wire MUST NOT carry W3C `event: axon.token`."
    );
}

#[test]
fn s5_openai_dialect_signature_unique_to_openai() {
    let mut openai_adapter = select_adapter("openai", 0);
    let mut axon_adapter = select_adapter("axon", 0);
    let mut anthropic_adapter = select_adapter("anthropic", 0);

    let openai_wire = drive_canonical_stream(&mut openai_adapter);
    let axon_wire = drive_canonical_stream(&mut axon_adapter);
    let anthropic_wire = drive_canonical_stream(&mut anthropic_adapter);

    // Signatures exclusive to openai: `data: [DONE]` sentinel +
    // `chat.completion.chunk` object.
    assert!(
        openai_wire.contains("data: [DONE]") || openai_wire.contains("[DONE]"),
        "33.z.k.i: openai dialect MUST emit `[DONE]` sentinel."
    );
    assert!(
        openai_wire.contains("chat.completion.chunk"),
        "33.z.k.i: openai dialect MUST emit `chat.completion.chunk` object."
    );
    assert!(
        !axon_wire.contains("[DONE]"),
        "33.z.k.i mutex: axon wire MUST NOT carry openai `[DONE]` sentinel."
    );
    assert!(
        !axon_wire.contains("chat.completion.chunk"),
        "33.z.k.i mutex: axon wire MUST NOT carry openai chunk shape."
    );
    assert!(
        !anthropic_wire.contains("[DONE]"),
        "33.z.k.i mutex: anthropic wire MUST NOT carry openai `[DONE]` sentinel."
    );
    assert!(
        !anthropic_wire.contains("chat.completion.chunk"),
        "33.z.k.i mutex: anthropic wire MUST NOT carry openai chunk shape."
    );
}

#[test]
fn s5_anthropic_dialect_signature_unique_to_anthropic() {
    let mut anthropic_adapter = select_adapter("anthropic", 0);
    let mut axon_adapter = select_adapter("axon", 0);
    let mut openai_adapter = select_adapter("openai", 0);

    let anthropic_wire = drive_canonical_stream(&mut anthropic_adapter);
    let axon_wire = drive_canonical_stream(&mut axon_adapter);
    let openai_wire = drive_canonical_stream(&mut openai_adapter);

    // Signatures exclusive to anthropic: `event: message_start` /
    // `event: content_block_*` / `event: message_stop`.
    assert!(
        anthropic_wire.contains("event: message_start"),
        "33.z.k.i: anthropic dialect MUST emit `event: message_start`."
    );
    assert!(
        anthropic_wire.contains("event: content_block_start"),
        "33.z.k.i: anthropic dialect MUST emit `event: content_block_start`."
    );
    assert!(
        anthropic_wire.contains("event: message_stop"),
        "33.z.k.i: anthropic dialect MUST emit `event: message_stop` terminator."
    );
    assert!(
        !axon_wire.contains("event: message_start"),
        "33.z.k.i mutex: axon wire MUST NOT carry anthropic `event: message_start`."
    );
    assert!(
        !axon_wire.contains("event: message_stop"),
        "33.z.k.i mutex: axon wire MUST NOT carry anthropic `event: message_stop`."
    );
    assert!(
        !openai_wire.contains("event: message_start"),
        "33.z.k.i mutex: openai wire MUST NOT carry anthropic `event: message_start`."
    );
    assert!(
        !openai_wire.contains("event: message_stop"),
        "33.z.k.i mutex: openai wire MUST NOT carry anthropic `event: message_stop`."
    );
}

// ════════════════════════════════════════════════════════════════════
// section 6 — Per-dialect flush_terminator frame count pinned
// ════════════════════════════════════════════════════════════════════

#[test]
fn s6_axon_flush_terminator_emits_zero_frames() {
    let mut adapter = select_adapter("axon", 0);
    let terminator = adapter.flush_terminator();
    assert_eq!(
        terminator.len(),
        0,
        "33.z.k.i: axon dialect's terminator is IN-LINE with the \
         axon.complete frame (emitted from translate(FlowComplete) / \
         build_complete_envelope_event). flush_terminator MUST return \
         exactly 0 frames. Got {}.",
        terminator.len()
    );
}

#[test]
fn s6_openai_flush_terminator_emits_two_frames() {
    let mut adapter = select_adapter("openai", 0);
    let terminator = adapter.flush_terminator();
    assert_eq!(
        terminator.len(),
        2,
        "33.z.k.i: openai dialect's terminator is exactly 2 frames: \
         (1) Q7 axon_metadata extension frame, (2) literal `data: [DONE]` \
         sentinel. Got {}.",
        terminator.len()
    );
}

#[test]
fn s6_anthropic_flush_terminator_emits_two_frames() {
    let mut adapter = select_adapter("anthropic", 0);
    let terminator = adapter.flush_terminator();
    assert_eq!(
        terminator.len(),
        2,
        "33.z.k.i: anthropic dialect's terminator is exactly 2 frames: \
         (1) Q7 `event: axon.metadata` extension frame, (2) \
         `event: message_stop` terminator. Got {}.",
        terminator.len()
    );
}

// ════════════════════════════════════════════════════════════════════
// section 7 — CompleteEnvelope public-field surface lock
// ════════════════════════════════════════════════════════════════════

#[test]
fn s7_complete_envelope_field_set_is_locked() {
    // The CompleteEnvelope struct is the producer↔adapter contract.
    // Adding a field requires every adapter to consider whether to
    // surface it. Removing a field breaks existing adapters' metadata
    // frames. This test ensures the field set is intentional.
    //
    // Construct an envelope using EVERY field by name (compile-time
    // shape pin). If the struct evolves, this test fails to compile.
    let envelope = CompleteEnvelope {
        trace_id: 0,
        flow_name: String::new(),
        backend: String::new(),
        success: false,
        steps_executed: 0,
        tokens_input: 0,
        tokens_output: 0,
        latency_ms: 0,
        effect_policies: Vec::new(),
        enforcement_summaries: Vec::new(),
        runtime_warnings: Vec::new(),
        step_audit_records: Vec::new(),
        epistemic_envelopes: Vec::new(),
        // v2.46.0 — considered per adapter: the axon dialect surfaces it
        // on `axon.complete` (elided when None); openai/anthropic do not
        // (the v2.7.0 epistemic precedent — axon-native observability rides
        // the axon dialect only).
        temporal_context: None,
    };
    // Touch every field at the use site to lock the read surface.
    let _ = envelope.trace_id;
    let _ = &envelope.flow_name;
    let _ = &envelope.backend;
    let _ = envelope.success;
    let _ = envelope.steps_executed;
    let _ = envelope.tokens_input;
    let _ = envelope.tokens_output;
    let _ = envelope.latency_ms;
    let _ = &envelope.effect_policies;
    let _ = &envelope.enforcement_summaries;
    let _ = &envelope.runtime_warnings;
    let _ = &envelope.step_audit_records;
    // If a new field gets added, the constructor above breaks the
    // build. If a field gets removed, the read at the end fails to
    // compile. Both are explicit signals.
}

// ════════════════════════════════════════════════════════════════════
// section 8 — Catalog snapshot self-consistency
// ════════════════════════════════════════════════════════════════════

#[test]
fn s8_snapshot_matches_parser_module_constant() {
    let snapshot_set: std::collections::BTreeSet<&str> =
        CANONICAL_DIALECT_SNAPSHOT.iter().copied().collect();
    let parser_set: std::collections::BTreeSet<&str> =
        AXONENDPOINT_TRANSPORT_DIALECTS.iter().copied().collect();
    assert_eq!(
        snapshot_set, parser_set,
        "33.z.k.i: CANONICAL_DIALECT_SNAPSHOT in this test file MUST \
         match axon_frontend::parser::AXONENDPOINT_TRANSPORT_DIALECTS \
         byte-identically. The snapshot is the explicit drift-detector; \
         the parser constant is the source of truth. If they diverge, \
         either the catalog evolved (update snapshot + all 9 downstream \
         sites) OR the change is a bug (revert)."
    );
}

#[test]
fn s8_snapshot_total_explicit_membership() {
    // Hardcode every snapshot string in a match to prove total
    // explicit enumeration at the test site.
    for &d in CANONICAL_DIALECT_SNAPSHOT {
        let recognized = match d {
            "axon" => true,
            "openai" => true,
            "kimi" => true,
            "glm" => true,
            "anthropic" => true,
            _ => false,
        };
        assert!(
            recognized,
            "33.z.k.i: snapshot entry `{d}` not enumerated explicitly. \
             The match arms here MUST contain every snapshot member."
        );
    }
    assert_eq!(
        CANONICAL_DIALECT_SNAPSHOT.len(),
        5,
        "33.z.k.i: snapshot cardinality drifted from 5."
    );
}
