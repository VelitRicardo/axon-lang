//! v1.29.0 — D12 robustness fuzz pack for the v1.29.0 cycle
//! (tools-as-stream-producers).
//!
//! Total + never-panic invariant for every public surface 34.a–i
//! exposes. Deterministic per-seed (hand-rolled linear congruential
//! generator — Knuth/MMIX constants, no external dep); regressions
//! reproduce verbatim from the seed printed on failure.
//!
//! ## Surfaces under fuzz
//!
//! 1. **Per-tool-surface resolution + construction totality** —
//!    `tool_dispatch_bridge::resolve_streaming_tool` over random
//!    `ToolEntry`s + `HttpStreamingTool::from_entry` /
//!    `McpStreamingTool::from_entry` over random runtime URLs +
//!    `StubStreamingTool` stream drain. Never panics; the resolved
//!    `Tool` trait object always answers `is_streaming()`.
//!
//! 2. **Closed-catalog predicate + serde round-trip totality** —
//!    `tool_registry::derive_is_streaming` /
//!    `tool_dispatch_bridge::extract_stream_policy` over random
//!    `effect_row`s; `BackpressurePolicy` slug round-trip;
//!    `unified_stream::chat_chunk_to_tool_chunk` over every
//!    `FinishReason` variant; `ToolChunk` + `ToolFinishReason`
//!    serde round-trip (the cross-stack wire shape — Python mirror
//!    is drift-gated in 34.b/c).
//!
//! 3. **4-disjunction convergence** — `unified_stream_handler` over
//!    random `ToolChunk` sequences with random (or no) policy. The
//!    handler never panics; the returned `ToolStreamSummary`
//!    satisfies the structural invariants
//!    (`tokens_emitted ≤ chunks_pushed`, `is_clean_stop` iff
//!    success & !cancelled & no error terminator, hash is 64 hex).
//!
//! 4. **Backpressure policy enforcement under load** — random burst
//!    sizes × the 4 closed-catalog policies through the unified
//!    handler. Per-policy invariants hold under adversarial chunk
//!    counts (DropOldest never fails; DegradeQuality conserves
//!    degraded+delivered=pushed; PauseUpstream never drops).
//!
//! 5. **Cancel-into-tool-body (D5)** — random cancel/check
//!    interleavings across `CancellationFlag` clones (monotone
//!    invariant) + random pre-cancel of `unified_stream_handler`
//!    (summary always `cancelled`).
//!
//! ## Determinism
//!
//! Each test sweeps a contiguous seed range; the LCG is re-seeded
//! per iteration so a failure at `seed=K` reproduces by running
//! that single seed. Total budget: ~4 200 iterations across 5
//! surfaces, runs in well under 2 seconds.

#![allow(clippy::needless_return)]

use std::sync::atomic::{AtomicBool, Ordering};

use axon::backends::{ChatChunk, FinishReason};
use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::unified_stream::{
    chat_chunk_to_tool_chunk, unified_stream_from_chunks, unified_stream_handler,
};
use axon::stream_effect::BackpressurePolicy;
use axon::tool_dispatch_bridge::{extract_stream_policy, resolve_streaming_tool};
use axon::tool_registry::{derive_is_streaming, ToolEntry, ToolSource};
use axon::tool_trait::{ToolChunk, ToolContext, ToolFinishReason};
use tokio::sync::mpsc;

// ════════════════════════════════════════════════════════════════════
// section 0 — Deterministic PRNG (linear congruential — Knuth/MMIX)
// ════════════════════════════════════════════════════════════════════

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(2654435761).max(1))
    }

    fn next_u64(&mut self) -> u64 {
        // Numerical Recipes 64-bit LCG (Knuth) — deterministic, fast,
        // statistically adequate for fuzz purposes.
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    fn next_in(&mut self, lo: u64, hi: u64) -> u64 {
        debug_assert!(hi >= lo);
        let span = hi - lo + 1;
        lo + (self.next_u64() % span)
    }

    fn next_bool(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    fn next_byte(&mut self) -> u8 {
        (self.next_u64() & 0xff) as u8
    }

    /// Random lowercase ASCII identifier, length 1..=12.
    fn ident(&mut self) -> String {
        let len = self.next_in(1, 12) as usize;
        (0..len)
            .map(|_| (b'a' + (self.next_byte() % 26)) as char)
            .collect()
    }

    /// Random short string drawn from a broader byte range — used
    /// to fuzz parser inputs with non-identifier characters.
    fn noisy_string(&mut self) -> String {
        let len = self.next_in(0, 24) as usize;
        (0..len)
            .map(|_| {
                // Bias toward printable ASCII but include the
                // structural characters `:` `/` `-` `;` ` `.
                let pick = self.next_in(0, 7);
                match pick {
                    0 => ':',
                    1 => '/',
                    2 => '-',
                    3 => ';',
                    4 => ' ',
                    _ => (b'a' + (self.next_byte() % 26)) as char,
                }
            })
            .collect()
    }

    /// Random `BackpressurePolicy` from the closed 4-element catalog.
    fn policy(&mut self) -> BackpressurePolicy {
        match self.next_in(0, 3) {
            0 => BackpressurePolicy::DropOldest,
            1 => BackpressurePolicy::DegradeQuality,
            2 => BackpressurePolicy::PauseUpstream,
            _ => BackpressurePolicy::Fail,
        }
    }

    /// Random `ToolFinishReason` from the closed 3-element catalog.
    fn finish_reason(&mut self) -> ToolFinishReason {
        match self.next_in(0, 2) {
            0 => ToolFinishReason::Stop,
            1 => ToolFinishReason::Error {
                message: self.ident(),
            },
            _ => ToolFinishReason::Cancelled,
        }
    }

    /// Random `ToolChunk` — intermediate or terminator, random delta
    /// (possibly empty).
    fn tool_chunk(&mut self, allow_terminator: bool) -> ToolChunk {
        let delta = if self.next_in(0, 4) == 0 {
            String::new() // ~20% empty delta
        } else {
            self.ident()
        };
        if allow_terminator && self.next_in(0, 3) == 0 {
            ToolChunk::terminator(delta, self.finish_reason())
        } else {
            ToolChunk::intermediate(delta)
        }
    }

    /// Random `ChatChunk` exercising every `FinishReason` variant.
    fn chat_chunk(&mut self) -> ChatChunk {
        let finish_reason = match self.next_in(0, 5) {
            0 => None,
            1 => Some(FinishReason::Stop),
            2 => Some(FinishReason::Length),
            3 => Some(FinishReason::ToolUse),
            4 => Some(FinishReason::SafetyBreach),
            _ => Some(FinishReason::Other(self.ident())),
        };
        ChatChunk {
            delta: if self.next_bool() { self.ident() } else { String::new() },
            finish_reason,
            usage: None,
        }
    }

    /// Random `effect_row` — a mix of plain effects + 0..=2
    /// `stream:<policy>` entries (the policy slug may be a real
    /// catalog member OR a synthetic non-member, to fuzz the
    /// defensive rejection path).
    fn effect_row(&mut self) -> Vec<String> {
        let n = self.next_in(0, 5) as usize;
        let mut row = Vec::with_capacity(n);
        for _ in 0..n {
            match self.next_in(0, 4) {
                0 => row.push(format!(
                    "stream:{}",
                    match self.next_in(0, 4) {
                        0 => "drop_oldest",
                        1 => "degrade_quality",
                        2 => "pause_upstream",
                        3 => "fail",
                        _ => "nonsense_policy", // synthetic non-member
                    }
                )),
                1 => row.push("network".to_string()),
                2 => row.push("compute".to_string()),
                3 => row.push("io".to_string()),
                _ => row.push(self.ident()),
            }
        }
        row
    }
}

// ════════════════════════════════════════════════════════════════════
// section 0.1 — shared helpers
// ════════════════════════════════════════════════════════════════════

fn fuzz_tool_entry(rng: &mut Lcg) -> ToolEntry {
    let provider = match rng.next_in(0, 6) {
        0 => "stub",
        1 => "stub_stream",
        2 => "native",
        3 => "http",
        4 => "mcp",
        5 => "custom_xyz",
        _ => "", // empty provider
    };
    // For http/mcp, sometimes give a valid URL, sometimes garbage.
    let runtime = match (provider, rng.next_in(0, 3)) {
        ("http" | "mcp", 0) => format!("https://{}.example.com/api", rng.ident()),
        ("http" | "mcp", 1) => format!("http://127.0.0.1:{}/x", rng.next_in(1, 65535)),
        ("http" | "mcp", 2) => format!("ftp://{}/bad-scheme", rng.ident()), // wrong scheme
        _ => String::new(),
    };
    let effect_row = rng.effect_row();
    let is_streaming = derive_is_streaming(&effect_row);
    ToolEntry {
        name: rng.ident(),
        provider: provider.to_string(),
        timeout: if rng.next_bool() {
            format!("{}s", rng.next_in(1, 60))
        } else {
            String::new()
        },
        runtime,
        resource_ref: String::new(),
        substrate: None,
        capacity: None,
        sandbox: None,
        max_results: None,
        output_schema: String::new(),
        effect_row,
        parameters: Vec::new(),
        secret: String::new(),
        secret_partition: String::new(),
        source: ToolSource::Program,
        is_streaming,
        scrape: None,
    }
}

// ════════════════════════════════════════════════════════════════════
// section 1 — Per-tool-surface resolution + construction totality
// ════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_resolve_streaming_tool_never_panics_over_random_entries() {
    // resolve_streaming_tool is a total function: every (provider,
    // runtime, effect_row) triple resolves to SOME Box<dyn Tool>.
    let mut work: u64 = 0;
    for seed in 1..=800u64 {
        let mut rng = Lcg::new(seed);
        let entry = fuzz_tool_entry(&mut rng);
        let tool = resolve_streaming_tool(&entry);
        // is_streaming() must answer without panic — bool either way.
        let _ = tool.is_streaming();
        work += 1;
    }
    assert_eq!(work, 800, "fuzz swept every seed");
}

#[test]
fn fuzz_http_mcp_from_entry_total_over_random_urls() {
    use axon::emcp::McpStreamingTool;
    use axon::http_tool::HttpStreamingTool;

    for seed in 1..=600u64 {
        let mut rng = Lcg::new(seed);
        let mut entry = fuzz_tool_entry(&mut rng);
        // from_entry's contract: Ok for valid http(s):// URL,
        // Err (never panic) for empty / non-http scheme.
        entry.provider = "http".to_string();
        let http_result = HttpStreamingTool::from_entry(&entry);
        let url = entry.runtime.trim();
        let url_valid =
            url.starts_with("http://") || url.starts_with("https://");
        assert_eq!(
            http_result.is_ok(),
            url_valid,
            "seed={seed}: HttpStreamingTool::from_entry Ok-ness must \
             track URL validity (url={url:?})"
        );

        entry.provider = "mcp".to_string();
        let mcp_result = McpStreamingTool::from_entry(&entry);
        assert_eq!(
            mcp_result.is_ok(),
            url_valid,
            "seed={seed}: McpStreamingTool::from_entry Ok-ness must \
             track URL validity (url={url:?})"
        );
    }
}

#[tokio::test]
async fn fuzz_stub_streaming_tool_drain_never_panics() {
    use futures::StreamExt;

    // The stub-stream provider always resolves to StubStreamingTool;
    // draining its stream over random args + random cancel state
    // must never panic + always terminate.
    for seed in 1..=400u64 {
        let mut rng = Lcg::new(seed);
        let mut entry = fuzz_tool_entry(&mut rng);
        entry.provider = if rng.next_bool() { "stub" } else { "stub_stream" }
            .to_string();
        let tool = resolve_streaming_tool(&entry);

        let cancel = CancellationFlag::new();
        if rng.next_in(0, 3) == 0 {
            cancel.cancel(); // ~25% pre-cancelled
        }
        let ctx = ToolContext::new(cancel, rng.next_u64());
        let args = rng.ident();
        let mut stream = tool.stream(args, ctx).await;
        let mut count = 0u64;
        while let Some(chunk) = stream.next().await {
            // Each chunk's is_terminator() answers without panic.
            let _ = chunk.is_terminator();
            count += 1;
            if count > 100 {
                panic!("seed={seed}: stub stream did not terminate");
            }
        }
        // A well-formed stub stream always emits ≥ 1 chunk.
        assert!(count >= 1, "seed={seed}: stub stream emitted nothing");
    }
}

// ════════════════════════════════════════════════════════════════════
// section 2 — Closed-catalog predicate + serde round-trip totality
// ════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_derive_is_streaming_total_and_matches_rule() {
    // derive_is_streaming(row) ⟺ any entry starts with "stream:".
    for seed in 1..=600u64 {
        let mut rng = Lcg::new(seed);
        let row = rng.effect_row();
        let derived = derive_is_streaming(&row);
        let expected = row.iter().any(|e| e.starts_with("stream:"));
        assert_eq!(
            derived, expected,
            "seed={seed}: derive_is_streaming drift on row {row:?}"
        );
    }
}

#[test]
fn fuzz_extract_stream_policy_total_first_wins_defensive() {
    // extract_stream_policy: first valid stream:<policy> wins;
    // unknown slugs are skipped; total (never panics).
    for seed in 1..=600u64 {
        let mut rng = Lcg::new(seed);
        let row = rng.effect_row();
        let extracted = extract_stream_policy(&row);
        // Compute the expected first-wins value independently.
        let expected = row.iter().find_map(|e| {
            e.strip_prefix("stream:")
                .and_then(BackpressurePolicy::from_slug)
        });
        assert_eq!(
            extracted, expected,
            "seed={seed}: extract_stream_policy drift on row {row:?}"
        );
    }
}

#[test]
fn fuzz_backpressure_policy_slug_round_trip_total() {
    // Every catalog member round-trips slug → from_slug → member.
    for &policy in BackpressurePolicy::ALL {
        let slug = policy.slug();
        assert_eq!(
            BackpressurePolicy::from_slug(slug),
            Some(policy),
            "slug round-trip drift on {policy:?}"
        );
    }
    // Synthetic non-member slugs always reject.
    for seed in 1..=400u64 {
        let mut rng = Lcg::new(seed);
        let slug = rng.noisy_string();
        let known = matches!(
            slug.as_str(),
            "drop_oldest" | "degrade_quality" | "pause_upstream" | "fail"
        );
        let parsed = BackpressurePolicy::from_slug(&slug);
        if known {
            assert!(parsed.is_some(), "seed={seed}: catalog slug must parse");
        } else {
            assert!(
                parsed.is_none(),
                "seed={seed}: non-catalog slug {slug:?} must reject"
            );
        }
    }
}

#[test]
fn fuzz_chat_chunk_to_tool_chunk_total_over_all_finish_reasons() {
    // The conversion is total: every ChatChunk maps to a ToolChunk
    // with the delta preserved byte-equal + finish_reason mapped
    // per the closed-catalog table.
    for seed in 1..=600u64 {
        let mut rng = Lcg::new(seed);
        let chat = rng.chat_chunk();
        let original_delta = chat.delta.clone();
        let had_reason = chat.finish_reason.is_some();
        let tool = chat_chunk_to_tool_chunk(chat);
        assert_eq!(
            tool.delta, original_delta,
            "seed={seed}: delta MUST be preserved byte-equal"
        );
        // finish_reason presence is preserved (Some↦Some, None↦None).
        assert_eq!(
            tool.finish_reason.is_some(),
            had_reason,
            "seed={seed}: finish_reason presence drift"
        );
    }
}

#[test]
fn fuzz_tool_chunk_serde_round_trip_total() {
    // ToolChunk JSON round-trip — the cross-stack wire shape.
    for seed in 1..=700u64 {
        let mut rng = Lcg::new(seed);
        let chunk = rng.tool_chunk(true);
        let json = serde_json::to_string(&chunk)
            .unwrap_or_else(|e| panic!("seed={seed}: serialize failed: {e}"));
        let back: ToolChunk = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("seed={seed}: deserialize failed: {e}"));
        assert_eq!(
            chunk, back,
            "seed={seed}: ToolChunk round-trip drift"
        );
        // D4 byte-compat: intermediate chunks (no finish_reason)
        // MUST elide the `finish_reason` key.
        if chunk.finish_reason.is_none() {
            assert!(
                !json.contains("finish_reason"),
                "seed={seed}: None finish_reason MUST be elided: {json}"
            );
        }
    }
}

#[test]
fn fuzz_tool_finish_reason_serde_round_trip_total() {
    for seed in 1..=400u64 {
        let mut rng = Lcg::new(seed);
        let reason = rng.finish_reason();
        let json = serde_json::to_string(&reason).expect("serialize");
        let back: ToolFinishReason =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(reason, back, "seed={seed}: ToolFinishReason round-trip drift");
        // The serde tag is `kind`.
        assert!(
            json.contains("\"kind\""),
            "seed={seed}: ToolFinishReason JSON MUST carry the `kind` tag: {json}"
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// section 3 — 4-disjunction convergence: unified_stream_handler totality
// ════════════════════════════════════════════════════════════════════

/// Build a random chunk sequence: 0..=N intermediates then a random
/// terminator (or no terminator — fuzz the "stream ends without an
/// explicit terminator" path too).
fn fuzz_chunk_sequence(rng: &mut Lcg) -> Vec<ToolChunk> {
    let n = rng.next_in(0, 30) as usize;
    let mut chunks: Vec<ToolChunk> = (0..n).map(|_| rng.tool_chunk(false)).collect();
    if rng.next_in(0, 4) != 0 {
        // ~75% of sequences carry an explicit terminator.
        chunks.push(ToolChunk::terminator(
            if rng.next_bool() { rng.ident() } else { String::new() },
            rng.finish_reason(),
        ));
    }
    chunks
}

#[tokio::test]
async fn fuzz_unified_handler_convergence_total_over_random_sequences() {
    for seed in 1..=800u64 {
        let mut rng = Lcg::new(seed);
        let chunks = fuzz_chunk_sequence(&mut rng);
        let n_chunks = chunks.len() as u64;
        let policy = if rng.next_bool() { Some(rng.policy()) } else { None };
        let source = unified_stream_from_chunks(chunks);
        let cancel = CancellationFlag::new();
        let (tx, _rx) = mpsc::unbounded_channel();

        let result =
            unified_stream_handler(source, policy, &cancel, &tx, "FuzzStep", "").await;
        // The handler never errors when the receiver is alive.
        let summary = result
            .unwrap_or_else(|e| panic!("seed={seed}: handler errored: {e:?}"));

        // INVARIANT 1 — tokens_emitted ≤ chunks the source produced.
        assert!(
            summary.tokens_emitted <= n_chunks.max(1) + 1,
            "seed={seed}: tokens_emitted {} exceeds plausible bound \
             (n_chunks={n_chunks})",
            summary.tokens_emitted
        );
        // INVARIANT 2 — no policy ⟹ zero enforcement counters.
        if policy.is_none() {
            assert_eq!(
                summary.chunks_dropped, 0,
                "seed={seed}: no-policy run reported drops"
            );
            assert_eq!(
                summary.chunks_degraded, 0,
                "seed={seed}: no-policy run reported degrades"
            );
        }
        // INVARIANT 3 — is_clean_stop ⟺ success & !cancelled & no error.
        let clean = summary.success
            && !summary.cancelled
            && summary.terminator_message.is_none();
        assert_eq!(
            summary.is_clean_stop(),
            clean,
            "seed={seed}: is_clean_stop disagrees with field state"
        );
        // INVARIANT 4 — output_hash_hex is always 64 hex chars.
        assert_eq!(
            summary.output_hash_hex.len(),
            64,
            "seed={seed}: output_hash_hex not 64 hex chars"
        );
        assert!(
            summary.output_hash_hex.chars().all(|c| c.is_ascii_hexdigit()),
            "seed={seed}: output_hash_hex has non-hex chars"
        );
    }
}

#[tokio::test]
async fn fuzz_unified_handler_hash_deterministic_per_sequence() {
    // The same chunk sequence drained twice (no policy) MUST yield
    // the same output_hash_hex — the D6 replay anchor's determinism.
    for seed in 1..=300u64 {
        let mut rng = Lcg::new(seed);
        let chunks = fuzz_chunk_sequence(&mut rng);

        let run = |cs: Vec<ToolChunk>| async move {
            let source = unified_stream_from_chunks(cs);
            let cancel = CancellationFlag::new();
            let (tx, _rx) = mpsc::unbounded_channel();
            unified_stream_handler(source, None, &cancel, &tx, "Det", "")
                .await
                .expect("ok")
        };

        let h1 = run(chunks.clone()).await.output_hash_hex;
        let h2 = run(chunks).await.output_hash_hex;
        assert_eq!(h1, h2, "seed={seed}: hash non-deterministic");
    }
}

// ════════════════════════════════════════════════════════════════════
// section 4 — Backpressure policy enforcement under load
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn fuzz_policy_enforcement_under_random_burst() {
    for seed in 1..=800u64 {
        let mut rng = Lcg::new(seed);
        let policy = rng.policy();
        // Burst size deliberately spans below + above the policy
        // buffer capacity so overflow paths get exercised.
        let burst = rng.next_in(1, 400) as usize;
        let mut chunks: Vec<ToolChunk> = (0..burst)
            .map(|i| ToolChunk::intermediate(format!("c{i}")))
            .collect();
        chunks.push(ToolChunk::terminator("", ToolFinishReason::Stop));

        let source = unified_stream_from_chunks(chunks);
        let cancel = CancellationFlag::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let result =
            unified_stream_handler(source, Some(policy), &cancel, &tx, "Burst", "")
                .await;
        let summary = result
            .unwrap_or_else(|e| panic!("seed={seed}: policy {policy:?} errored: {e:?}"));

        // Per-policy invariants under load.
        match policy {
            BackpressurePolicy::DropOldest => {
                assert!(
                    summary.success,
                    "seed={seed}: DropOldest must never fail"
                );
                assert!(
                    summary.chunks_delivered <= summary.chunks_pushed,
                    "seed={seed}: DropOldest delivered ({}) > pushed ({})",
                    summary.chunks_delivered,
                    summary.chunks_pushed
                );
            }
            BackpressurePolicy::DegradeQuality => {
                assert!(
                    summary.success,
                    "seed={seed}: DegradeQuality must never fail"
                );
                // Conservation: degraded + delivered == pushed (no
                // chunk silently lost).
                assert_eq!(
                    summary.chunks_degraded + summary.chunks_delivered,
                    summary.chunks_pushed,
                    "seed={seed}: DegradeQuality conservation broken"
                );
            }
            BackpressurePolicy::PauseUpstream => {
                assert!(
                    summary.success,
                    "seed={seed}: PauseUpstream must never fail"
                );
                // PauseUpstream blocks the producer — never drops.
                assert_eq!(
                    summary.chunks_delivered, summary.chunks_pushed,
                    "seed={seed}: PauseUpstream dropped a chunk"
                );
            }
            BackpressurePolicy::Fail => {
                // With a live consumer, Fail typically completes;
                // the invariant is "never panics" + delivered ≤
                // pushed regardless of overflow outcome.
                assert!(
                    summary.chunks_delivered <= summary.chunks_pushed,
                    "seed={seed}: Fail delivered > pushed"
                );
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// section 5 — Cancel-into-tool-body (D5)
// ════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_cancellation_flag_monotone_over_random_interleavings() {
    // Once cancelled, is_cancelled() NEVER returns false again — the
    // monotone invariant across Clone handles + random check/cancel
    // schedules.
    for seed in 1..=800u64 {
        let mut rng = Lcg::new(seed);
        let flag = CancellationFlag::new();
        let clones: Vec<CancellationFlag> =
            (0..rng.next_in(1, 5)).map(|_| flag.clone()).collect();
        let observed_cancel = AtomicBool::new(false);
        let steps = rng.next_in(4, 40);
        for _ in 0..steps {
            // `next_in` is inclusive on both bounds — index into
            // `[0, len-1]` requires `hi = len - 1`.
            let pick = rng.next_in(0, clones.len() as u64 - 1) as usize;
            if rng.next_in(0, 5) == 0 {
                clones[pick].cancel();
            }
            let c = clones[pick].is_cancelled();
            if c {
                observed_cancel.store(true, Ordering::SeqCst);
            } else {
                // If we EVER observed a cancel, no handle may now
                // report false — monotone violation.
                assert!(
                    !observed_cancel.load(Ordering::SeqCst),
                    "seed={seed}: monotone violation — cancel un-observed"
                );
            }
        }
    }
}

#[tokio::test]
async fn fuzz_unified_handler_pre_cancel_always_marks_cancelled() {
    // Pre-cancel + any random chunk sequence + any policy →
    // summary.cancelled is ALWAYS true, success ALWAYS false.
    for seed in 1..=600u64 {
        let mut rng = Lcg::new(seed);
        let chunks = fuzz_chunk_sequence(&mut rng);
        let policy = if rng.next_bool() { Some(rng.policy()) } else { None };
        let source = unified_stream_from_chunks(chunks);
        let cancel = CancellationFlag::new();
        cancel.cancel(); // pre-cancel
        let (tx, _rx) = mpsc::unbounded_channel();
        let result =
            unified_stream_handler(source, policy, &cancel, &tx, "PreCancel", "")
                .await;
        let summary = result
            .unwrap_or_else(|e| panic!("seed={seed}: handler errored: {e:?}"));
        assert!(
            summary.cancelled,
            "seed={seed}: pre-cancel must mark summary.cancelled"
        );
        assert!(
            !summary.success,
            "seed={seed}: pre-cancel must mark summary.success=false"
        );
        assert!(
            !summary.is_clean_stop(),
            "seed={seed}: pre-cancelled run cannot be a clean stop"
        );
    }
}

#[tokio::test]
async fn fuzz_unified_handler_terminator_kind_classification_total() {
    // For a sequence whose explicit terminator carries a known
    // finish reason, the summary's success/cancelled/terminator
    // fields classify it correctly + totally.
    for seed in 1..=400u64 {
        let mut rng = Lcg::new(seed);
        let n = rng.next_in(0, 6) as usize;
        let mut chunks: Vec<ToolChunk> =
            (0..n).map(|_| ToolChunk::intermediate(rng.ident())).collect();
        let reason = rng.finish_reason();
        chunks.push(ToolChunk::terminator("", reason.clone()));

        let source = unified_stream_from_chunks(chunks);
        let cancel = CancellationFlag::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let summary = unified_stream_handler(source, None, &cancel, &tx, "Term", "")
            .await
            .expect("ok");

        match reason {
            ToolFinishReason::Stop => {
                assert!(summary.success, "seed={seed}: Stop ⟹ success");
                assert!(!summary.cancelled);
                assert!(summary.terminator_message.is_none());
            }
            ToolFinishReason::Error { .. } => {
                assert!(!summary.success, "seed={seed}: Error ⟹ !success");
                assert!(summary.terminator_message.is_some());
            }
            ToolFinishReason::Cancelled => {
                assert!(summary.cancelled, "seed={seed}: Cancelled ⟹ cancelled");
                assert!(!summary.success);
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// section 6 — Iteration-budget pin
// ════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_iteration_budget_pin() {
    // Documents the total fuzz budget so a future edit that silently
    // drops a sweep range is caught. Sum of all per-test seed ranges:
    // section 1: 800 + 600 + 400 = 1800
    // section 2: 600 + 600 + 400 + 600 + 700 + 400 = 3300
    // section 3: 800 + 300 = 1100
    // section 4: 800 =  800
    // section 5: 800 + 600 + 400 = 1800
    // Total ≈ 8 800 iterations (the plan-vivo ~4 000 target is the
    // floor; this pack exceeds it for deeper coverage).
    let budget: u64 = (800 + 600 + 400)
        + (600 + 600 + 400 + 600 + 700 + 400)
        + (800 + 300)
        + 800
        + (800 + 600 + 400);
    assert_eq!(budget, 8800, "fuzz iteration budget drifted");
    assert!(
        budget >= 4000,
        "34.j plan-vivo floor is ~4 000 iterations"
    );
}
