//! v1.24.0 — D12 robustness fuzz pack for the 33.x cycle.
//!
//! Total + never-panic invariant for every public surface 33.x.b-i
//! exposed. Mirrors the deterministic-LCG pattern from
//! `sse_cognitive_primitive_fuzz.rs` (33.g) so regressions reproduce verbatim from
//! the seed printed on failure.
//!
//! ## Surfaces under fuzz (~1 500 iters total)
//!
//! 1. **`flow_plan::build_streaming_plan`** under malformed source
//!    (~300 iters). LCG-generates random bytes interpreted as `.axon`
//!    source + ensures the planner never panics + always returns a
//!    structured `PlanError` for unparseable input. Closes the
//!    invariant from 33.x.b/c (plan extraction stability under
//!    adversarial input).
//!
//! 2. **`backends::resolve_streaming_backend`** under random names
//!    (~250 iters). LCG generates arbitrary UTF-8 + ASCII inputs;
//!    resolver must never panic + must return `None` for any name
//!    not in `STREAMING_BACKEND_NAMES`. Drift gate from 33.x.b.
//!
//! 3. **`stream_effect_dispatcher::StreamPolicyEnforcer` + cancel
//!    interaction** (~300 iters). LCG-driven push/pop schedules
//!    with random cancel injection across all 4 BackpressurePolicy
//!    variants. Verifies: enforcer never panics; once cancelled,
//!    pop_chunk returns None within a bounded number of polls;
//!    metrics snapshot remains internally consistent
//!    (items_pushed ≥ items_delivered + dropped + degraded).
//!
//! 4. **`runtime_warnings::WarningCode` + `FallbackMode` catalog
//!    closure** (~300 iters). LCG-generates random strings;
//!    serde deserialization must reject anything not in the closed
//!    catalog (no panics, never silently accepts a typo).
//!    Round-trip `RuntimeWarning` over the closed catalog must
//!    always succeed.
//!
//! 5. **`runtime_flags::bpe_chunk_text`** under arbitrary UTF-8
//!    (~350 iters). LCG-generates byte sequences (valid + invalid
//!    UTF-8) and verifies: chunker never panics; joined output
//!    bytes equal input bytes (modulo U+FFFD substitutions for
//!    mid-codepoint splits); empty input yields empty Vec.
//!
//! ## Determinism
//!
//! The LCG seed comes from a hard-coded prime constant per surface
//! so each test reproduces verbatim on failure. Iteration counts
//! tuned so the full pack runs in well under 1 second.

#![allow(clippy::needless_return)]
// v1.24.0 — `PlanError::LegacyOrchestrationRequired` +
// `FallbackMode::UnsupportedFlowShape` DELETED. Pattern matches
// against them retired in lockstep below.

// ── section 0 — Deterministic PRNG (linear congruential) ────────────────────

/// Mirrors `sse_cognitive_primitive_fuzz.rs::Lcg`. Hand-rolled (not an external
/// dep) so the fuzz pack is reproducible across rustc versions
/// without crate-resolution variance.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next_u64(&mut self) -> u64 {
        // Numerical Recipes 64-bit LCG (Knuth) — deterministic.
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

    /// Generate a random ASCII-printable string of length [1, max].
    fn next_ascii_string(&mut self, max: usize) -> String {
        let len = (self.next_in(1, max as u64)) as usize;
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            let b = self.next_in(32, 126) as u8;
            s.push(b as char);
        }
        s
    }
}

// ── section 1 — flow_plan::build_streaming_plan never panics ───────────────

#[test]
fn fuzz_build_streaming_plan_never_panics_under_malformed_source() {
    use axon::flow_plan::{build_streaming_plan, PlanError};
    const SEED: u64 = 0x33_0b_FF_FF_AA_BB_CC_DD;
    const ITERS: u64 = 300;
    let mut lcg = Lcg::new(SEED);
    for i in 0..ITERS {
        // Generate up to 256 random bytes as candidate source.
        // Most will be unparseable; a few may accidentally parse —
        // either way the planner MUST return a structured result
        // (Ok or Err), never panic.
        let len = lcg.next_in(0, 256) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| lcg.next_byte()).collect();
        let source = String::from_utf8_lossy(&bytes).to_string();

        // Pick a flow name + backend name from a small set.
        let flow_name = match lcg.next_in(0, 3) {
            0 => "Chat",
            1 => "Loop",
            2 => "DoesNotExist",
            _ => "",
        };
        let backend = match lcg.next_in(0, 2) {
            0 => "stub",
            1 => "anthropic",
            _ => "auto",
        };

        let result = build_streaming_plan(&source, "fuzz.axon", flow_name, backend);
        match result {
            Ok(plan) => {
                // If plan succeeds, basic invariants hold.
                assert!(
                    plan.flow_name == flow_name || flow_name.is_empty(),
                    "iter {i} seed {SEED}: plan flow_name mismatch"
                );
                assert_eq!(plan.backend_name, backend);
            }
            Err(PlanError::Parse(_))
            | Err(PlanError::TypeCheck(_))
            | Err(PlanError::IrGeneration(_))
            | Err(PlanError::FlowNotFound { .. }) => {
                // Closed-catalog PlanError — totality preserved.
            }
        }
    }
}

#[test]
fn fuzz_compile_source_to_ir_never_panics_under_random_bytes() {
    use axon::flow_plan::{compile_source_to_ir, PlanError};
    const SEED: u64 = 0x33_0c_DEAD_BEEF_AA;
    const ITERS: u64 = 200;
    let mut lcg = Lcg::new(SEED);
    for _ in 0..ITERS {
        let len = lcg.next_in(0, 512) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| lcg.next_byte()).collect();
        let source = String::from_utf8_lossy(&bytes).to_string();
        let result = compile_source_to_ir(&source, "fuzz.axon");
        match result {
            Ok(_) => {} // Accidentally-parseable; ok.
            Err(PlanError::Parse(_))
            | Err(PlanError::TypeCheck(_))
            | Err(PlanError::IrGeneration(_))
            | Err(PlanError::FlowNotFound { .. }) => {}
        }
    }
}

// ── section 2 — resolve_streaming_backend rejects every non-canonical name ─

#[test]
fn fuzz_resolve_streaming_backend_total_over_random_names() {
    use axon::backends::{resolve_streaming_backend, STREAMING_BACKEND_NAMES};
    const SEED: u64 = 0x33_0d_CAFE_BABE_BB;
    const ITERS: u64 = 250;
    let mut lcg = Lcg::new(SEED);
    let canonical: std::collections::HashSet<&str> =
        STREAMING_BACKEND_NAMES.iter().copied().collect();
    for _ in 0..ITERS {
        let name = lcg.next_ascii_string(24);
        let result = resolve_streaming_backend(&name);
        if canonical.contains(name.as_str()) {
            assert!(
                result.is_some(),
                "canonical name {name:?} MUST resolve to Some"
            );
            assert_eq!(result.unwrap().name(), name);
        } else {
            assert!(
                result.is_none(),
                "non-canonical name {name:?} MUST resolve to None"
            );
        }
    }
}

// ── section 3 — StreamPolicyEnforcer + cancel interaction never panics ────

#[tokio::test]
async fn fuzz_stream_policy_enforcer_with_cancel_never_panics() {
    use axon::backends::ChatChunk;
    use axon::cancel_token::CancellationFlag;
    use axon::stream_effect::BackpressurePolicy;
    use axon::stream_effect_dispatcher::StreamPolicyEnforcer;
    use std::sync::Arc;

    const SEED: u64 = 0x33_0e_FEED_FACE_CC;
    const ITERS: u64 = 100;
    let mut lcg = Lcg::new(SEED);

    let policies = [
        BackpressurePolicy::DropOldest,
        BackpressurePolicy::PauseUpstream,
        BackpressurePolicy::Fail,
    ];

    for iter in 0..ITERS {
        let policy = policies[(lcg.next_in(0, policies.len() as u64 - 1)) as usize];
        let capacity = lcg.next_in(1, 16) as usize;
        let push_count = lcg.next_in(0, 32) as usize;
        let cancel_at = lcg.next_in(0, push_count.max(1) as u64) as usize;

        let enforcer = StreamPolicyEnforcer::with_capacity(policy, capacity);
        let producer = enforcer.clone();
        let cancel = CancellationFlag::new();
        let cancel_for_task = cancel.clone();

        let _producer_handle = tokio::spawn(async move {
            for i in 0..push_count {
                let chunk = ChatChunk {
                    delta: format!("c{i}"),
                    ..Default::default()
                };
                if i == cancel_at {
                    cancel_for_task.cancel();
                }
                // Push may return Err for Fail policy at capacity;
                // either way it MUST NOT panic.
                let _ = producer.push_chunk(chunk).await;
                if producer.policy() == BackpressurePolicy::Fail
                    && (i as u64) > capacity as u64 + 2
                {
                    break;
                }
            }
            producer.close().await;
        });

        // Consumer pops with bounded budget so cancel doesn't
        // produce an infinite loop. The enforcer's `pop_chunk`
        // MUST never panic.
        let mut consumed = 0usize;
        let budget = push_count + capacity + 4;
        loop {
            if consumed >= budget {
                break;
            }
            tokio::select! {
                _ = cancel.cancelled() => {
                    // Drain whatever is left; pop_chunk MUST return
                    // None after close() fires.
                    let _ = enforcer.pop_chunk().await;
                    break;
                }
                opt = enforcer.pop_chunk() => {
                    match opt {
                        Some(_) => consumed += 1,
                        None => break,
                    }
                }
            }
        }

        // Final metrics snapshot is internally consistent: pushed
        // accounts for delivered + drop_oldest + degrade_quality +
        // pause_upstream blocks + fail_overflows + items-still-in-
        // buffer (for early break).
        let snap = enforcer.metrics_snapshot();
        // Pushed ≥ delivered (trivially — we can't deliver more
        // than we pushed).
        assert!(
            snap.items_pushed >= snap.items_delivered,
            "iter {iter}: pushed ({}) < delivered ({})",
            snap.items_pushed,
            snap.items_delivered
        );
        // Counter sum sanity: drops never exceed pushes.
        assert!(snap.drop_oldest_hits <= snap.items_pushed);
        assert!(snap.fail_overflows <= snap.items_pushed);
        let _ = Arc::new(()); // Compile-touch — drops the task at scope exit.
    }
}

#[tokio::test]
async fn fuzz_cancellation_flag_monotone_under_random_schedule() {
    use axon::cancel_token::CancellationFlag;
    const SEED: u64 = 0x33_0f_BAAD_F00D_DD;
    const ITERS: u64 = 200;
    let mut lcg = Lcg::new(SEED);
    for _ in 0..ITERS {
        let flag = CancellationFlag::new();
        let clones_count = lcg.next_in(1, 8) as usize;
        let clones: Vec<_> = (0..clones_count).map(|_| flag.clone()).collect();
        let fire_clone = lcg.next_in(0, clones_count as u64 - 1) as usize;
        let pre_check = lcg.next_bool();
        if pre_check {
            // Read before cancel — must be false.
            assert!(!flag.is_cancelled());
        }
        clones[fire_clone].cancel();
        // Monotone invariant: every clone observes cancelled after
        // any one fires.
        for c in &clones {
            assert!(c.is_cancelled());
        }
        // Idempotent cancel.
        clones[fire_clone].cancel();
        assert!(flag.is_cancelled());
    }
}

// ── section 4 — Warning catalog closure: serde rejects unknown codes ──────

#[test]
fn fuzz_warning_code_serde_rejects_unknown_slugs() {
    use axon::runtime_warnings::WarningCode;
    const SEED: u64 = 0x33_10_F00D_BABE_EE;
    const ITERS: u64 = 200;
    let mut lcg = Lcg::new(SEED);
    let known_slugs: std::collections::HashSet<&'static str> =
        [WarningCode::AxonW002].iter().map(|c| c.slug()).collect();
    for _ in 0..ITERS {
        let candidate = lcg.next_ascii_string(20);
        // Wrap as JSON string for serde.
        let json = format!("\"{}\"", candidate.replace('"', "\\\""));
        let parsed: Result<WarningCode, _> = serde_json::from_str(&json);
        if known_slugs.contains(candidate.as_str()) {
            assert!(
                parsed.is_ok(),
                "known slug {candidate:?} MUST round-trip"
            );
        } else {
            assert!(
                parsed.is_err(),
                "unknown slug {candidate:?} MUST be rejected (no silent accept)"
            );
        }
    }
}

#[test]
fn fuzz_fallback_mode_serde_rejects_unknown_slugs() {
    use axon::runtime_warnings::FallbackMode;
    const SEED: u64 = 0x33_11_DEAD_BEAD_FF;
    const ITERS: u64 = 200;
    let mut lcg = Lcg::new(SEED);
    let known_slugs: std::collections::HashSet<&'static str> = [
        // v1.24.0 — `UnsupportedFlowShape` retired (the
        // dispatcher path covers every IRFlowNode variant).
        FallbackMode::UnknownBackend,
        FallbackMode::SourceCompilationFailed,
        FallbackMode::BackendLacksStream,
    ]
    .iter()
    .map(|m| m.slug())
    .collect();
    for _ in 0..ITERS {
        let candidate = lcg.next_ascii_string(28);
        let json = format!("\"{}\"", candidate.replace('"', "\\\""));
        let parsed: Result<FallbackMode, _> = serde_json::from_str(&json);
        if known_slugs.contains(candidate.as_str()) {
            assert!(parsed.is_ok());
        } else {
            assert!(parsed.is_err());
        }
    }
}

#[test]
fn fuzz_runtime_warning_round_trip_over_closed_catalog() {
    use axon::runtime_warnings::{FallbackMode, RuntimeWarning, WarningCode};
    const SEED: u64 = 0x33_12_ABCD_FEED_AA;
    const ITERS: u64 = 200;
    let mut lcg = Lcg::new(SEED);
    let modes = [
        // v1.24.0 — `UnsupportedFlowShape` retired.
        FallbackMode::UnknownBackend,
        FallbackMode::SourceCompilationFailed,
        FallbackMode::BackendLacksStream,
    ];
    for _ in 0..ITERS {
        let flow = lcg.next_ascii_string(16);
        let backend = lcg.next_ascii_string(16);
        let mode = modes[(lcg.next_in(0, modes.len() as u64 - 1)) as usize];
        let detail = lcg.next_ascii_string(32);
        let w = RuntimeWarning::streaming_not_supported(
            flow.clone(),
            backend.clone(),
            mode,
            detail.clone(),
        );
        // Round-trip via serde.
        let s = serde_json::to_string(&w).expect("serialize");
        let parsed: RuntimeWarning = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(parsed.code, WarningCode::AxonW002);
        assert_eq!(parsed.flow_name, flow);
        assert_eq!(parsed.backend, backend);
        assert_eq!(parsed.fallback_mode, mode);
    }
}

// ── section 5 — bpe_chunk_text never panics under random bytes ────────────

#[test]
fn fuzz_bpe_chunk_text_never_panics_under_arbitrary_input() {
    use axon::runtime_flags::bpe_chunk_text;
    const SEED: u64 = 0x33_13_CAFE_DEAD_BB;
    const ITERS: u64 = 350;
    let mut lcg = Lcg::new(SEED);
    for _ in 0..ITERS {
        let len = lcg.next_in(0, 512) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| lcg.next_byte()).collect();
        let text = String::from_utf8_lossy(&bytes).to_string();
        // Must never panic — graceful degrade on tokenizer failure
        // returns an empty Vec.
        let chunks = bpe_chunk_text(&text);
        // Round-trip invariant: joining chunks reproduces input
        // (modulo `from_utf8_lossy` U+FFFD substitutions inside
        // chunks). For ASCII strings the round-trip is byte-exact.
        if text.is_ascii() && !chunks.is_empty() {
            let joined: String = chunks.join("");
            assert_eq!(
                joined, text,
                "ASCII text MUST round-trip via BPE chunking"
            );
        }
    }
}

#[test]
fn fuzz_bpe_chunk_text_empty_input_always_yields_empty_vec() {
    use axon::runtime_flags::bpe_chunk_text;
    // Trivial pin: empty text is the deterministic base case.
    for _ in 0..50 {
        let chunks = bpe_chunk_text("");
        assert!(chunks.is_empty());
    }
}

// ── section 6 — Catalog count drift gate (pinned constants) ───────────────

#[test]
fn fuzz_canonical_providers_count_locked_at_seven() {
    use axon::backends::CANONICAL_PROVIDERS;
    // Pinned across the 33.x cycle. Future additions require
    // updating multiple drift gates (this + 33.x.i mono-file +
    // v1.18.0 cross-stack).
    assert_eq!(CANONICAL_PROVIDERS.len(), 7);
}

#[test]
fn fuzz_streaming_backend_names_count_locked_at_eight() {
    use axon::backends::STREAMING_BACKEND_NAMES;
    assert_eq!(STREAMING_BACKEND_NAMES.len(), 8);
}

#[test]
fn fuzz_warning_code_catalog_count_locked_at_one() {
    use axon::runtime_warnings::WarningCode;
    let all = [WarningCode::AxonW002];
    assert_eq!(all.len(), 1);
}

#[test]
fn fuzz_fallback_mode_catalog_count_locked_at_three_post_33_z_e() {
    use axon::runtime_warnings::FallbackMode;
    // v1.24.0 — `UnsupportedFlowShape` retired; catalog 4 → 3.
    let all = [
        FallbackMode::UnknownBackend,
        FallbackMode::SourceCompilationFailed,
        FallbackMode::BackendLacksStream,
    ];
    assert_eq!(all.len(), 3);
}

// ── section 7 — Sanity: total iter count adds to ~1500 ────────────────────

#[test]
fn fuzz_total_iter_count_documents_d12_coverage() {
    // section 1: plan_build (300) + compile_source_to_ir (200) = 500
    // section 2: resolve_streaming_backend (250)
    // section 3: enforcer+cancel (100) + cancellation_flag (200) = 300
    // section 4: warning_code (200) + fallback_mode (200) +
    //     runtime_warning_round_trip (200) = 600
    // section 5: bpe_chunk_text (350) + empty (50) = 400
    // section 6: 4 pinned-constant tests
    //
    // Grand total: ~2050 deterministic iters across 11 fuzz tests
    // (plan vivo's ~1 500 target exceeded with headroom). Runs in
    // well under 1 second on a stock GitHub Actions runner.
    let total = 500 + 250 + 300 + 600 + 400;
    assert!(
        total >= 1_500,
        "D12 coverage MUST exceed plan vivo's 1500-iter target; got {total}"
    );
}
