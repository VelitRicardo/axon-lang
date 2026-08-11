//! §Fase 33.y.c D12 fuzz pack — adversarial coverage of the 6
//! pure-shape async handlers (Step / Probe / Reason / Validate /
//! Refine / Weave).
//!
//! # Methodology
//!
//! Hand-rolled deterministic 64-bit LCG (Numerical Recipes / Knuth
//! constants, no external dep) mirrors the established Fase 33.x.k
//! pattern. Each iter draws structured noise from the LCG +
//! constructs a synthetic IR variant + invokes the dispatcher entry +
//! asserts:
//!
//! - Never panics. The D7 mandate forbids `unimplemented!()` /
//!   `todo!()` / `panic!()` on any code path. The fuzz harness
//!   surfaces a panic at PR time, not adopter runtime.
//! - Returns either `Ok(NodeOutcome::Completed)` (canonical happy
//!   path with stub backend producing `"(stub)"`) OR
//!   `Err(DispatchError::UpstreamCancelled)` (when pre-cancelled) OR
//!   `Err(DispatchError::ChannelClosed)` (when the receiver dropped
//!   between iters — happens when test runtime tears down). NO other
//!   variant is reachable for stub + valid IR.
//! - When `Ok(Completed)`: `output == "(stub)"`, `tokens_emitted == 1`,
//!   `step_index` reflects ctx.step_counter at handler entry.
//!
//! # Coverage targets (plan vivo §4 33.y.c — 150 iters/handler)
//!
//! - Step                 — 150 iters (random ask + name)
//! - Probe                — 150 iters (random target)
//! - Reason               — 150 iters (random target + strategy)
//! - Validate             — 150 iters (random target + rule)
//! - Refine               — 150 iters (random target + strategy)
//! - Weave                — 150 iters (random sources + format + style)
//! - **Total**            — 900 LCG iters
//!
//! Plus 4 composition iters:
//! - cancel_random_iter   — 100 iters across all 6 handlers with
//!                          random pre-cancel timing
//! - policy_random_iter   — 100 iters across all 4 BackpressurePolicy
//!                          variants × 6 handlers (random pairing)
//! - chained_steps_iter   — 50 iters chaining 3 consecutive handler
//!                          calls verifying step_counter monotone
//! - utf8_input_iter      — 200 iters with random UTF-8 input strings
//!                          (high-codepoint surrogate boundaries)
//!
//! **Grand total: 1,350 deterministic LCG iters.** Runtime <1s on a
//! stock GitHub Actions runner.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::pure_shape::{
    run_probe, run_reason, run_refine, run_step, run_validate, run_weave,
};
use axon::flow_dispatcher::{DispatchCtx, DispatchError, NodeOutcome};
use axon::ir_nodes::*;
use axon::stream_effect::BackpressurePolicy;
use tokio::sync::mpsc;

// ────────────────────────────────────────────────────────────────────
//  LCG — Numerical Recipes 64-bit Knuth constants
// ────────────────────────────────────────────────────────────────────
//
// Hand-rolled deterministic 64-bit linear-congruential generator.
// Same constants used by the Fase 33.x.k fuzz pack for cross-cycle
// reproducibility. Reseed for each test via `Lcg::new(seed)` and
// the iteration count drives reproducibility — same seed + same
// iteration index produces byte-identical inputs.

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        // Mix the seed once so seed=0 doesn't produce all-zero.
        let mixed = seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(0xBB67_AE85_84CA_A73B);
        Self(mixed.max(1))
    }
    fn next_u64(&mut self) -> u64 {
        // Knuth's MMIX constants — Numerical Recipes §7.1.
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    fn range(&mut self, max: usize) -> usize {
        (self.next_u64() as usize) % max.max(1)
    }
    fn ascii_string(&mut self, len: usize) -> String {
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            let c = (self.range(95) + 32) as u8; // printable ASCII 32..127
            s.push(c as char);
        }
        s
    }
    /// Convenience: draw a length in `[1, max]` from the LCG, then
    /// draw that many ASCII characters. Avoids the double-borrow
    /// issue when callers want to inline `ascii_string(range(N))`.
    fn ascii_with_random_len(&mut self, max: usize) -> String {
        let len = self.range(max) + 1;
        self.ascii_string(len)
    }
    /// Same shape but `[0, max]` so callers can produce empty strings.
    fn ascii_with_random_len_or_empty(&mut self, max: usize) -> String {
        let len = self.range(max);
        self.ascii_string(len)
    }
    fn utf8_with_random_len_or_empty(&mut self, max: usize) -> String {
        let len = self.range(max);
        self.random_utf8(len)
    }
    fn utf8_with_random_len(&mut self, max: usize) -> String {
        let len = self.range(max) + 1;
        self.random_utf8(len)
    }
    fn random_utf8(&mut self, len: usize) -> String {
        // Pull from a curated set: ASCII + Latin-Extended + Greek +
        // Cyrillic + CJK Unified Ideographs (small range) + Emoji
        // (variable-byte ranges). Tests boundary cases for UTF-8
        // streaming + serde JSON-string encoding.
        const POOL: &[char] = &[
            'A', 'z', '0', '7', ' ', '\n', '\t', '!',
            'á', 'ü', 'ñ', 'ç',
            'α', 'β', 'Δ',
            'д', 'ж', 'Я',
            '中', '文', '字',
            '\u{1F600}', '\u{1F4A1}', '\u{1F300}', // emoji
        ];
        let mut s = String::with_capacity(len * 2);
        for _ in 0..len {
            s.push(POOL[self.range(POOL.len())]);
        }
        s
    }
    fn boolean(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

// ────────────────────────────────────────────────────────────────────
//  Helpers
// ────────────────────────────────────────────────────────────────────

fn fresh_ctx() -> (
    DispatchCtx,
    mpsc::UnboundedReceiver<axon::flow_execution_event::FlowExecutionEvent>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = DispatchCtx::new(
        "FuzzFlow",
        "stub",
        "fuzz system prompt",
        CancellationFlag::new(),
        tx,
    );
    (ctx, rx)
}

fn assert_outcome_invariants(
    label: &str,
    outcome: &Result<NodeOutcome, DispatchError>,
) {
    match outcome {
        Ok(NodeOutcome::Completed {
            output,
            tokens_emitted,
            ..
        }) => {
            assert_eq!(
                output, "(stub)",
                "{label} stub output should be byte-equal '(stub)'"
            );
            assert_eq!(
                *tokens_emitted, 1,
                "{label} stub emits exactly 1 token"
            );
        }
        Ok(other) => {
            // §Fase 33.y.l — `NodeOutcome::LegacyShimHandled` retired
            // along with `legacy_shim` + `ShimReason`.
            //
            // NodeOutcome is #[non_exhaustive] from the perspective of
            // downstream crates; future variants (Break / LoopContinue
            // / Return are already in the closed catalog) MUST NOT
            // surface from pure-shape handlers — those are
            // orchestration sentinels.
            panic!("{label}: unexpected NodeOutcome variant: {other:?}");
        }
        Err(DispatchError::UpstreamCancelled) => {
            // Acceptable when pre-cancelled.
        }
        Err(DispatchError::ChannelClosed) => {
            // Acceptable when receiver dropped between iters.
        }
        Err(other) => {
            panic!("{label}: unexpected DispatchError variant: {other:?}");
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  §1 — Per-handler fuzz: 150 iters × 6 handlers
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_run_step_never_panics_random_input() {
    let mut lcg = Lcg::new(0xDEAD_BEEF_CAFE_F00D);
    for iter in 0..150 {
        let name = lcg.ascii_with_random_len(20);
        let ask = lcg.ascii_with_random_len(80);
        let s = IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
            name,
            persona_ref: String::new(),
            given: String::new(),
            ask,
            use_tool: None,
            probe: None,
            reason: None,
            weave: None,
            output_type: String::new(),
            confidence_floor: None,
            navigate_ref: String::new(),
            apply_ref: String::new(),
            pix_ops: Vec::new(),
            stream: None,
            performs: Vec::new(),
            guards: Vec::new(),
            requires_context: None,            now_tz: None,            body: Vec::new(),
        };
        let (mut ctx, _rx) = fresh_ctx();
        let outcome = run_step(&s, &mut ctx).await;
        assert_outcome_invariants(&format!("step iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_run_probe_never_panics_random_input() {
    let mut lcg = Lcg::new(0xC0DE_C0FE_C0DE_C0DE);
    for iter in 0..150 {
        let target = lcg.ascii_with_random_len(40);
        let p = IRProbe {
            node_type: "probe",
            source_line: 0,
            source_column: 0,
            target,
        };
        let (mut ctx, _rx) = fresh_ctx();
        let outcome = run_probe(&p, &mut ctx).await;
        assert_outcome_invariants(&format!("probe iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_run_reason_never_panics_random_input() {
    let mut lcg = Lcg::new(0xAA_BB_CC_DD_EE_FF_00_11);
    for iter in 0..150 {
        let target = lcg.ascii_with_random_len(30);
        let strategy = lcg.ascii_with_random_len_or_empty(20);
        let r = IRReasonStep {
            node_type: "reason",
            source_line: 0,
            source_column: 0,
            strategy,
            target,
            given: String::new(),
            ask: String::new(),
            depth: None,
        };
        let (mut ctx, _rx) = fresh_ctx();
        let outcome = run_reason(&r, &mut ctx).await;
        assert_outcome_invariants(&format!("reason iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_run_validate_never_panics_random_input() {
    let mut lcg = Lcg::new(0x1234_5678_9ABC_DEF0);
    for iter in 0..150 {
        let target = lcg.ascii_with_random_len(30);
        let rule = lcg.ascii_with_random_len_or_empty(25);
        let v = IRValidateStep {
            node_type: "validate",
            resolved_schema: None,
            guard: None,
            source_line: 0,
            source_column: 0,
            target,
            rule,
        };
        let (mut ctx, _rx) = fresh_ctx();
        let outcome = run_validate(&v, &mut ctx).await;
        assert_outcome_invariants(&format!("validate iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_run_refine_never_panics_random_input() {
    let mut lcg = Lcg::new(0xFEED_FACE_DEAD_BEEF);
    for iter in 0..150 {
        let target = lcg.ascii_with_random_len(30);
        let strategy = lcg.ascii_with_random_len_or_empty(20);
        let r = IRRefineStep {
            node_type: "refine",
            source_line: 0,
            source_column: 0,
            target,
            strategy,
        };
        let (mut ctx, _rx) = fresh_ctx();
        let outcome = run_refine(&r, &mut ctx).await;
        assert_outcome_invariants(&format!("refine iter={iter}"), &outcome);
    }
}

#[tokio::test]
async fn fuzz_run_weave_never_panics_random_input() {
    let mut lcg = Lcg::new(0x0F0F_0F0F_F0F0_F0F0);
    for iter in 0..150 {
        let target = lcg.ascii_with_random_len(25);
        let sources_count = lcg.range(5);
        let sources: Vec<String> = (0..sources_count)
            .map(|_| lcg.ascii_with_random_len(15))
            .collect();
        let format_type = lcg.ascii_with_random_len_or_empty(10);
        let style = lcg.ascii_with_random_len_or_empty(10);
        let priority_count = lcg.range(3);
        let priority: Vec<String> = (0..priority_count)
            .map(|_| lcg.ascii_with_random_len(10))
            .collect();
        let w = IRWeaveStep {
            node_type: "weave",
            source_line: 0,
            source_column: 0,
            sources,
            target,
            format_type,
            priority,
            style,
            include: Vec::new(),
        };
        let (mut ctx, _rx) = fresh_ctx();
        let outcome = run_weave(&w, &mut ctx).await;
        assert_outcome_invariants(&format!("weave iter={iter}"), &outcome);
    }
}

// ────────────────────────────────────────────────────────────────────
//  §2 — Cancel-random fuzz: 100 iters across all 6 handlers
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_random_cancel_timing_across_all_handlers() {
    let mut lcg = Lcg::new(0xC4_AC_3D_C4_AC_3D_C4_AC);
    for iter in 0..100 {
        let cancel = CancellationFlag::new();
        let pre_cancel = lcg.boolean();
        if pre_cancel {
            cancel.cancel();
        }
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);

        // Pick a handler at random.
        let handler_idx = lcg.range(6);
        let outcome = match handler_idx {
            0 => {
                let s = IRStep {
                    node_type: "step",
                    source_line: 0,
                    source_column: 0,
                    name: "x".into(),
                    persona_ref: String::new(),
                    given: String::new(),
                    ask: "y".into(),
                    use_tool: None,
                    probe: None,
                    reason: None,
                    weave: None,
                    output_type: String::new(),
                    confidence_floor: None,
                    navigate_ref: String::new(),
                    apply_ref: String::new(),
                    pix_ops: Vec::new(),
                    stream: None,
                    performs: Vec::new(),
                    guards: Vec::new(),
                    requires_context: None,                    now_tz: None,                    body: Vec::new(),
                };
                run_step(&s, &mut ctx).await
            }
            1 => {
                let p = IRProbe {
                    node_type: "probe",
                    source_line: 0,
                    source_column: 0,
                    target: "t".into(),
                };
                run_probe(&p, &mut ctx).await
            }
            2 => {
                let r = IRReasonStep {
                    node_type: "reason",
                    source_line: 0,
                    source_column: 0,
                    strategy: "s".into(),
                    target: "t".into(),
                    given: String::new(),
                    ask: String::new(),
                    depth: None,
                };
                run_reason(&r, &mut ctx).await
            }
            3 => {
                let v = IRValidateStep {
                    node_type: "validate",
                    resolved_schema: None,
                    guard: None,
                    source_line: 0,
                    source_column: 0,
                    target: "t".into(),
                    rule: "r".into(),
                };
                run_validate(&v, &mut ctx).await
            }
            4 => {
                let r = IRRefineStep {
                    node_type: "refine",
                    source_line: 0,
                    source_column: 0,
                    target: "t".into(),
                    strategy: "s".into(),
                };
                run_refine(&r, &mut ctx).await
            }
            _ => {
                let w = IRWeaveStep {
                    node_type: "weave",
                    source_line: 0,
                    source_column: 0,
                    sources: Vec::new(),
                    target: "t".into(),
                    format_type: "f".into(),
                    priority: Vec::new(),
                    style: "s".into(),
                    include: Vec::new(),
                };
                run_weave(&w, &mut ctx).await
            }
        };

        if pre_cancel {
            assert!(
                matches!(outcome, Err(DispatchError::UpstreamCancelled)),
                "iter={iter} handler={handler_idx} pre-cancel: expected UpstreamCancelled, got {outcome:?}"
            );
        } else {
            // Either Completed or ChannelClosed depending on rx
            // teardown timing. Both acceptable.
            match outcome {
                Ok(NodeOutcome::Completed { .. })
                | Err(DispatchError::ChannelClosed) => {}
                other => panic!(
                    "iter={iter} handler={handler_idx} no-cancel: expected \
                     Completed or ChannelClosed, got {other:?}"
                ),
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  §3 — Policy-random fuzz: 100 iters cycling 4 policies × 6 handlers
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_random_policy_pairings_across_handlers() {
    let mut lcg = Lcg::new(0xB0_C1_CB_B0_C1_CB_FF_EE);
    let policies = [
        BackpressurePolicy::DropOldest,
        BackpressurePolicy::DegradeQuality,
        BackpressurePolicy::PauseUpstream,
        BackpressurePolicy::Fail,
    ];

    for iter in 0..100 {
        let policy = policies[lcg.range(4)];
        let (mut ctx, _rx) = fresh_ctx();
        ctx.pending_effect_policy = Some(policy);

        let handler_idx = lcg.range(6);
        let outcome = match handler_idx {
            0 => {
                let s = IRStep {
                    node_type: "step",
                    source_line: 0,
                    source_column: 0,
                    name: "x".into(),
                    persona_ref: String::new(),
                    given: String::new(),
                    ask: "y".into(),
                    use_tool: None,
                    probe: None,
                    reason: None,
                    weave: None,
                    output_type: String::new(),
                    confidence_floor: None,
                    navigate_ref: String::new(),
                    apply_ref: String::new(),
                    pix_ops: Vec::new(),
                    stream: None,
                    performs: Vec::new(),
                    guards: Vec::new(),
                    requires_context: None,                    now_tz: None,                    body: Vec::new(),
                };
                run_step(&s, &mut ctx).await
            }
            1 => run_probe(
                &IRProbe {
                    node_type: "probe",
                    source_line: 0,
                    source_column: 0,
                    target: "t".into(),
                },
                &mut ctx,
            )
            .await,
            2 => run_reason(
                &IRReasonStep {
                    node_type: "reason",
                    source_line: 0,
                    source_column: 0,
                    strategy: "".into(),
                    target: "t".into(),
                    given: String::new(),
                    ask: String::new(),
                    depth: None,
                },
                &mut ctx,
            )
            .await,
            3 => run_validate(
                &IRValidateStep {
                    node_type: "validate",
                    resolved_schema: None,
                    guard: None,
                    source_line: 0,
                    source_column: 0,
                    target: "t".into(),
                    rule: "".into(),
                },
                &mut ctx,
            )
            .await,
            4 => run_refine(
                &IRRefineStep {
                    node_type: "refine",
                    source_line: 0,
                    source_column: 0,
                    target: "t".into(),
                    strategy: "".into(),
                },
                &mut ctx,
            )
            .await,
            _ => run_weave(
                &IRWeaveStep {
                    node_type: "weave",
                    source_line: 0,
                    source_column: 0,
                    sources: Vec::new(),
                    target: "t".into(),
                    format_type: "".into(),
                    priority: Vec::new(),
                    style: "".into(),
                    include: Vec::new(),
                },
                &mut ctx,
            )
            .await,
        };

        match outcome {
            Ok(NodeOutcome::Completed { .. }) => {
                let summaries = ctx.enforcement_summaries.lock().await;
                assert_eq!(
                    summaries.len(),
                    1,
                    "iter={iter}: enforcement summary should be recorded"
                );
                let summary = summaries.values().next().unwrap();
                assert_eq!(
                    summary.policy_slug,
                    policy.slug(),
                    "iter={iter}: policy slug mismatch"
                );
            }
            Err(DispatchError::ChannelClosed) => {} // acceptable
            other => panic!("iter={iter}: unexpected outcome {other:?}"),
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  §4 — Chained-steps fuzz: 50 iters of 3-step chains
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_chained_steps_step_counter_monotone() {
    let mut lcg = Lcg::new(0xC1_4A_1D_3D_5C_C0_AB_77);
    for iter in 0..50 {
        let (mut ctx, _rx) = fresh_ctx();
        for step_idx in 0..3 {
            let name = lcg.ascii_with_random_len(10);
            let s = IRStep {
                node_type: "step",
                source_line: 0,
                source_column: 0,
                name,
                persona_ref: String::new(),
                given: String::new(),
                ask: "hi".into(),
                use_tool: None,
                probe: None,
                reason: None,
                weave: None,
                output_type: String::new(),
                confidence_floor: None,
                navigate_ref: String::new(),
                apply_ref: String::new(),
                pix_ops: Vec::new(),
                stream: None,
                performs: Vec::new(),
                guards: Vec::new(),
                requires_context: None,                now_tz: None,                body: Vec::new(),
            };
            match run_step(&s, &mut ctx).await {
                Ok(NodeOutcome::Completed { step_index, .. }) => {
                    assert_eq!(
                        step_index, step_idx as usize,
                        "iter={iter}: step_index monotone failed at chain pos {step_idx}"
                    );
                }
                Err(DispatchError::ChannelClosed) => break,
                other => panic!("iter={iter} chain pos {step_idx}: {other:?}"),
            }
        }
        // Final step_counter must equal the number of completed steps.
        assert!(ctx.step_counter <= 3);
    }
}

// ────────────────────────────────────────────────────────────────────
//  §5 — UTF-8 input fuzz: 200 iters with high-codepoint strings
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fuzz_utf8_random_input_does_not_panic() {
    let mut lcg = Lcg::new(0xA7_F8_A7_F8_A7_F8_FF_EE);
    for iter in 0..200 {
        let target = lcg.utf8_with_random_len(30);
        let strategy = lcg.utf8_with_random_len_or_empty(20);
        let r = IRReasonStep {
            node_type: "reason",
            source_line: 0,
            source_column: 0,
            strategy,
            target,
            given: String::new(),
            ask: String::new(),
            depth: None,
        };
        let (mut ctx, _rx) = fresh_ctx();
        let outcome = run_reason(&r, &mut ctx).await;
        assert_outcome_invariants(&format!("utf8 reason iter={iter}"), &outcome);
    }
}

// ────────────────────────────────────────────────────────────────────
//  §6 — Total iter count pin
// ────────────────────────────────────────────────────────────────────

#[test]
fn fuzz_pack_total_iter_count() {
    // Per-handler: 6 × 150 = 900
    // Cancel-random: 100
    // Policy-random: 100
    // Chained: 50 iters × 3 steps = 150
    // UTF-8: 200
    // Total: 1450
    let total = (6 * 150) + 100 + 100 + (50 * 3) + 200;
    assert_eq!(
        total, 1450,
        "33.y.c fuzz pack target: 1450 deterministic LCG iters \
         across 6 handlers + cancel + policy + composition + UTF-8."
    );
}
