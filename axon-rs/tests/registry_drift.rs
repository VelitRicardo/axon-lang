//! v1.29.0 — Tool registry `is_streaming` derivation
//! drift gate.
//!
//! Pins the **1-to-1 declaration → runtime contract**: for every
//! tool registered via `ToolRegistry::register_from_ir`, the
//! resulting `ToolEntry.is_streaming` MUST equal
//! `derive_is_streaming(spec.effect_row)`. The corpus exercises
//! the predicate over a synthetic 30-tool dataset covering:
//!
//! - **10 stream-producer tools** declaring closed-catalog
//!   `<stream:<policy>>` effects (`drop_oldest` / `degrade_quality`
//!   / `pause_upstream` / `fail`) with various co-occurring
//!   non-stream effects (`network`, `compute`, `io`, `epistemic`).
//! - **10 plain tools** declaring non-stream effects (`compute`,
//!   `network`, `io`, `read`, `write`, `epistemic:speculate`, etc.)
//!   in various combinations.
//! - **10 empty-effect-row tools** (no declarations).
//!
//! The drift gate asserts:
//!
//! 1. After registration, every entry's `is_streaming` matches
//!    `derive_is_streaming(effect_row)` byte-exact (the canonical
//!    rule).
//! 2. Exactly 10 of the 30 registered tools have `is_streaming ==
//!    true` (the declared cardinality).
//! 3. No false positives: `downstream` / `upstream-flow` / other
//!    `stream`-substring entries do NOT flag as streaming (the rule
//!    is `starts_with("stream:")`, not `contains`).
//! 4. The closed-catalog policy set the corpus exercises matches
//!    `BackpressurePolicy::all_slugs()` from `axon-rs/src/
//! stream_effect_dispatcher.rs` (v1.24.0) — a future axon-lang
//!    minor that adds a 5th policy must update both the catalog
//!    AND this corpus.

use axon::ir_nodes::IRToolSpec;
use axon::tool_registry::{derive_is_streaming, ToolRegistry};

// ════════════════════════════════════════════════════════════════════
//  Synthetic 30-tool corpus
// ════════════════════════════════════════════════════════════════════

/// Build a synthetic spec — terse + readable + deterministic.
fn spec(name: &str, effect_row: &[&str]) -> IRToolSpec {
    IRToolSpec {
        node_type: "ToolDefinition",
        source_line: 1,
        source_column: 1,
        name: name.to_string(),
        provider: "stub".to_string(),
        max_results: None,
        filter_expr: String::new(),
        timeout: String::new(),
        runtime: String::new(),
        resource_ref: String::new(),
        sandbox: None,
        input_schema: Vec::new(),
        output_schema: String::new(),
        parameters: Vec::new(),
        output_type: None,
        requires: Vec::new(),
        secret: String::new(),
        secret_partition: String::new(),
        effect_row: effect_row.iter().map(|s| s.to_string()).collect(),
        target: None,
        risk: None,
        argv: Vec::new(),
        cache: String::new(),
        scrape: None,
    }
}

/// The full 30-tool corpus. Cardinality breakdown:
/// - 10 stream-producer tools (rows 0-9)
/// - 10 plain non-stream tools (rows 10-19)
/// - 10 empty-effect-row tools (rows 20-29)
fn corpus() -> Vec<IRToolSpec> {
    vec![
        // ── 10 stream-producer tools across 4 closed-catalog
        // policies × varied co-occurring effects ──────────────────
        spec("ChatStreamDrop", &["stream:drop_oldest"]),
        spec(
            "ClinicalReasonerDrop",
            &["stream:drop_oldest", "network", "epistemic:speculate"],
        ),
        spec("DegradeStreamerA", &["stream:degrade_quality"]),
        spec(
            "DegradeStreamerB",
            &["stream:degrade_quality", "compute"],
        ),
        spec("PauseStreamerA", &["stream:pause_upstream"]),
        spec(
            "PauseStreamerB",
            &["stream:pause_upstream", "io", "network"],
        ),
        spec("FailStreamerA", &["stream:fail"]),
        spec("FailStreamerB", &["stream:fail", "epistemic:speculate"]),
        spec(
            "MultiEffectStreamer",
            &["compute", "stream:drop_oldest", "network"],
        ),
        spec(
            "FullEffectStreamer",
            &[
                "stream:fail",
                "compute",
                "io",
                "network",
                "epistemic:speculate",
            ],
        ),
        // ── 10 plain non-stream tools ──────────────────────────────
        spec("Calculator", &["compute"]),
        spec("DateTimeReader", &["read"]),
        spec("HttpProbe", &["network"]),
        spec("FileScanner", &["io"]),
        spec("WriteSink", &["write"]),
        spec("EpistemicProbe", &["epistemic:speculate"]),
        spec(
            "CompositeNonStream",
            &["compute", "network", "io"],
        ),
        // Edge cases: `stream`-substring NOT at prefix — MUST NOT
        // flag as streaming (the rule is `starts_with("stream:")`,
        // not `contains`).
        spec("DownstreamProcessor", &["downstream"]),
        spec("UpstreamFlowControl", &["upstream-flow", "network"]),
        spec("StreamWordTool", &["stream"]),
        // ── 10 empty-effect-row tools ──────────────────────────────
        spec("EmptyA", &[]),
        spec("EmptyB", &[]),
        spec("EmptyC", &[]),
        spec("EmptyD", &[]),
        spec("EmptyE", &[]),
        spec("EmptyF", &[]),
        spec("EmptyG", &[]),
        spec("EmptyH", &[]),
        spec("EmptyI", &[]),
        spec("EmptyJ", &[]),
    ]
}

// ════════════════════════════════════════════════════════════════════
// section 1 — Corpus cardinality + 1-to-1 declaration → runtime contract
// ════════════════════════════════════════════════════════════════════

#[test]
fn s1_corpus_size_is_exactly_thirty() {
    let c = corpus();
    assert_eq!(
        c.len(),
        30,
        "34.c drift gate corpus size MUST be 30. Got {}.",
        c.len()
    );
}

#[test]
fn s1_register_from_ir_derives_is_streaming_1_to_1() {
    let mut reg = ToolRegistry::new();
    let specs = corpus();
    reg.register_from_ir(&specs);

    // Every registered entry's is_streaming MUST equal the canonical
    // derivation from its effect_row.
    for spec in &specs {
        let entry = reg.get(&spec.name).unwrap_or_else(|| {
            panic!("34.c: tool `{}` MUST be registered", spec.name)
        });
        let expected = derive_is_streaming(&spec.effect_row);
        assert_eq!(
            entry.is_streaming,
            expected,
            "34.c 1-to-1 contract violation for tool `{}`: \
             is_streaming={} but derive_is_streaming({:?})={}",
            spec.name, entry.is_streaming, spec.effect_row, expected
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// section 2 — Cardinality pin: exactly 10 streaming tools in the corpus
// ════════════════════════════════════════════════════════════════════

#[test]
fn s2_corpus_has_exactly_ten_streaming_tools() {
    let mut reg = ToolRegistry::new();
    let specs = corpus();
    reg.register_from_ir(&specs);

    let streaming_count = specs
        .iter()
        .filter(|s| {
            let entry = reg.get(&s.name).unwrap();
            entry.is_streaming
        })
        .count();
    assert_eq!(
        streaming_count, 10,
        "34.c corpus cardinality pin: EXACTLY 10 of 30 corpus tools \
         declare a stream effect. Got {}.",
        streaming_count
    );
}

#[test]
fn s2_corpus_has_exactly_twenty_non_streaming_tools() {
    let mut reg = ToolRegistry::new();
    let specs = corpus();
    reg.register_from_ir(&specs);

    let non_streaming_count = specs
        .iter()
        .filter(|s| {
            let entry = reg.get(&s.name).unwrap();
            !entry.is_streaming
        })
        .count();
    assert_eq!(
        non_streaming_count, 20,
        "34.c corpus cardinality pin: EXACTLY 20 of 30 corpus tools \
         are non-streaming (10 plain + 10 empty). Got {}.",
        non_streaming_count
    );
}

// ════════════════════════════════════════════════════════════════════
// section 3 — No false positives: `stream`-substring NOT at prefix
// ════════════════════════════════════════════════════════════════════

#[test]
fn s3_substring_stream_does_not_flag_streaming() {
    let mut reg = ToolRegistry::new();
    reg.register_from_ir(&corpus());

    // These three tools have `stream` in their effect-row entries
    // but NOT at the `stream:` prefix position. They MUST be
    // detected as NON-streaming (the rule is `starts_with("stream:")`,
    // not `contains("stream")`).
    for name in &["DownstreamProcessor", "UpstreamFlowControl", "StreamWordTool"] {
        let entry = reg.get(name).unwrap();
        assert!(
            !entry.is_streaming,
            "34.c section 3: tool `{}` has `stream` in effect_row but NOT as \
             `stream:` prefix — MUST NOT flag as streaming. Got \
             is_streaming={}, effect_row={:?}",
            name, entry.is_streaming, entry.effect_row
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// section 4 — Closed-catalog policy coverage (cross-reference v1.24.0)
// ════════════════════════════════════════════════════════════════════

#[test]
fn s4_corpus_covers_all_four_closed_catalog_policies() {
    // The v1.24.0 closed-catalog BackpressurePolicy set is
    // {drop_oldest, degrade_quality, pause_upstream, fail}. The
    // 34.c drift gate corpus exercises every member at least once
    // — ensuring future policy additions force a corpus update.
    let mut policy_hits: std::collections::BTreeSet<&str> =
        std::collections::BTreeSet::new();
    for spec in corpus() {
        for effect in &spec.effect_row {
            if let Some(rest) = effect.strip_prefix("stream:") {
                if !rest.is_empty() {
                    // Statically `rest` is one of the closed-catalog
                    // policy slugs. We collect into a set (de-duplicate
                    // across the 10 streaming tools).
                    let leaked: &'static str = Box::leak(rest.to_string().into_boxed_str());
                    policy_hits.insert(leaked);
                }
            }
        }
    }
    let observed: Vec<&str> = policy_hits.iter().copied().collect();
    let expected = vec!["degrade_quality", "drop_oldest", "fail", "pause_upstream"];
    assert_eq!(
        observed, expected,
        "34.c section 4 closed-catalog coverage: the corpus MUST exercise all \
         4 BackpressurePolicy slugs from v1.24.0. Got {observed:?}, \
         expected {expected:?}."
    );
}

// ════════════════════════════════════════════════════════════════════
// section 5 — Derivation rule pure-function totality + idempotence
// ════════════════════════════════════════════════════════════════════

#[test]
fn s5_derive_is_streaming_is_pure_and_idempotent() {
    // Pure: same input → same output across N calls. Idempotent:
    // calling twice yields the same result.
    for spec in corpus() {
        let first = derive_is_streaming(&spec.effect_row);
        for _ in 0..10 {
            let again = derive_is_streaming(&spec.effect_row);
            assert_eq!(
                first, again,
                "34.c section 5: derive_is_streaming MUST be a pure function \
                 — repeated calls on the same input yield the same \
                 output. Tool `{}` drifted.",
                spec.name
            );
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// section 6 — Compose with register() direct path (caller-set field)
// ════════════════════════════════════════════════════════════════════

#[test]
fn s6_register_direct_path_respects_caller_set_is_streaming() {
    use axon::tool_registry::{ToolEntry, ToolSource};
    let mut reg = ToolRegistry::new();
    // Caller sets is_streaming explicitly (programmatic registration).
    // This is the path for adopters who construct ToolEntry without
    // going through IR — they take responsibility for the field.
    reg.register(ToolEntry {
        name: "ProgrammaticStreamer".to_string(),
        provider: "custom".to_string(),
        timeout: String::new(),
        runtime: String::new(),
        resource_ref: String::new(),
        substrate: None,
        capacity: None,
        sandbox: None,
        max_results: None,
        output_schema: String::new(),
        effect_row: vec!["custom_effect".to_string()],
        parameters: Vec::new(),
        secret: String::new(),
        secret_partition: String::new(),
        source: ToolSource::Program,
        // Adopter explicitly flags as streaming even though
        // effect_row doesn't have `stream:` prefix. This is a
        // valid path: adopters who programmatically construct
        // streaming tools without source-level effect declarations
        // can do so.
        is_streaming: true,
        scrape: None,
    });
    let entry = reg.get("ProgrammaticStreamer").unwrap();
    assert!(
        entry.is_streaming,
        "34.c section 6: register() direct path MUST respect caller-set \
         is_streaming. The auto-derivation only fires via \
         register_from_ir(); programmatic registration trusts the \
         caller."
    );
}
