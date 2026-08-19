//! v1.24.0 — Real-provider E2E lane (D10, opt-in via secret).
//!
//! D10 contract: every test in this file is `#[ignore]` by default
//! so normal `cargo test` runs SKIP them entirely — NO flake on PR
//! runs from network variance or expired API keys. The CI workflow
//! `.github/workflows/real_provider_2.yml` runs these tests
//! ONLY when the `AXON_RUN_REAL_PROVIDER_TEST` repository secret is
//! set to `"1"` (gated lane).
//!
//! # The measurable invariant (D10 + D3 cross-stack)
//!
//! Each real-provider lane (Anthropic / OpenAI / Gemini) hits a
//! short pinned prompt and verifies:
//!
//!   1. The HTTP stream actually opens (provider key + endpoint
//!      are reachable + the API hasn't broken our request shape).
//!   2. At least 5 chunks arrive (validates the provider's
//!      streaming protocol parser).
//!   3. **p95 inter-chunk arrival latency ≤ 100ms wall-clock**
//!      under healthy network — the D3 measurable invariant
//!      against real upstream providers (33.x.e validated the
//!      trait-layer cancel-aware adapter against a local mock;
//!      this lane closes the loop against the real wire).
//!
//! # Vertical canonical patterns
//!
//! The four high-profile regulated verticals (Banking, Government,
//! Legal, Medicine) each run a canonical-shape flow through a real
//! provider. Each test verifies:
//!
//!   - Stream produces ≥ 1 chunk (the wire is alive)
//!   - Total chunks count is sane (≥ 5 for a short prompt)
//!   - No `BackendError::Auth` (key is valid)
//!   - No `BackendError::RateLimit` (test budget is small enough)
//!   - Inter-chunk timing is plausible (no 30-second gaps from a
//!     stuck connection)
//!
//! # How to run locally
//!
//! ```ignore
//! # Set provider keys (at least one):
//! export ANTHROPIC_API_KEY=sk-ant-...
//! export OPENAI_API_KEY=sk-...
//! export GEMINI_API_KEY=AIza...
//!
//! # Run the gated lane:
//! cargo test --manifest-path axon-rs/Cargo.toml \
//!   --test real_provider -- --ignored --nocapture
//! ```
//!
//! Tests for a provider whose key isn't set will gracefully skip
//! (with `eprintln!` to surface the skip in the CI log) rather
//! than fail — so a lane that runs with only ANTHROPIC_API_KEY set
//! still passes for that provider while the OpenAI/Gemini lanes
//! skip cleanly.

#![allow(clippy::needless_return)]

use std::time::{Duration, Instant};

use axon::backends::{
    AnthropicBackend, Backend, ChatRequest, GeminiBackend, Message, OpenAIBackend,
};
use futures::StreamExt;

/// Pinned short prompt used by every lane — long enough to elicit
/// a multi-chunk stream from the LLM, short enough to stay under
/// any rate-limit / token-budget shared with adopter test runs.
const PINNED_PROMPT: &str =
    "Count from one to ten, one number per word, separated by spaces. Output only the numbers.";

/// Budget for the p95 inter-chunk latency. 100ms is the D3
/// measurable contract from the plan vivo. Network jitter in a
/// GitHub Actions runner adds ~10-30ms on top of provider
/// tokenization rate (~30 chunks/sec on Anthropic = ~33ms/chunk),
/// so 100ms p95 has reasonable headroom for healthy network days.
const P95_INTER_CHUNK_BUDGET_MS: u128 = 100;

/// Minimum chunks expected for the pinned prompt under healthy
/// streaming. Real providers should stream this prompt in 10-30
/// chunks; we assert ≥ 5 to remain robust against tokenizer-vocab
/// updates that may merge short numerals.
const MIN_CHUNKS: usize = 5;

/// Drain a stream + measure per-chunk arrival timestamps. Returns
/// `(chunk_count, inter_chunk_latencies_ms)` — empty Vec when
/// chunks < 2 (no inter-chunk gaps to measure).
async fn drain_with_timing<S>(
    mut stream: S,
) -> (usize, Vec<u128>)
where
    S: futures::Stream<
            Item = Result<
                axon::backends::ChatChunk,
                axon::backends::error::BackendError,
            >,
        > + Unpin,
{
    let mut count = 0usize;
    let mut latencies = Vec::new();
    let mut prev_arrival: Option<Instant> = None;
    while let Some(item) = stream.next().await {
        let now = Instant::now();
        if let Ok(chunk) = item {
            if !chunk.delta.is_empty() {
                count += 1;
                if let Some(prev) = prev_arrival {
                    latencies.push(now.duration_since(prev).as_millis());
                }
                prev_arrival = Some(now);
            }
        }
    }
    (count, latencies)
}

/// Compute p95 from a Vec of latencies in milliseconds. Returns
/// the largest value when the Vec is small enough that p95 lands
/// on the last element (< 20 samples).
fn p95(mut latencies: Vec<u128>) -> u128 {
    if latencies.is_empty() {
        return 0;
    }
    latencies.sort();
    let n = latencies.len();
    if n < 20 {
        // For small samples p95 lands on the max; return that
        // honestly rather than rounding into smaller buckets.
        return *latencies.last().unwrap();
    }
    // 95th percentile index (0-indexed): floor(0.95 * n).
    let idx = ((n as f64) * 0.95) as usize;
    let idx = idx.min(n - 1);
    latencies[idx]
}

/// Skip helper — log + early-return when the provider's API key
/// env var isn't set. Tests that depend on a key call this first.
fn maybe_skip(env_var: &str, provider: &str) -> bool {
    if std::env::var(env_var).is_err() {
        eprintln!(
            "SKIP: {provider} lane — {env_var} not set. \
             (Real-provider lane is opt-in; set the secret in CI \
             or export the env var locally.)"
        );
        return true;
    }
    false
}

fn pinned_request() -> ChatRequest {
    ChatRequest {
        messages: vec![Message::user(PINNED_PROMPT)],
        max_tokens: Some(128),
        stream: true,
        ..Default::default()
    }
}

// ─── section 1 — Anthropic real provider lane ──────────────────────────────

#[tokio::test]
#[ignore = "real-provider — opt-in via AXON_RUN_REAL_PROVIDER_TEST"]
async fn anthropic_real_stream_p95_inter_chunk_under_budget() {
    if maybe_skip("ANTHROPIC_API_KEY", "Anthropic") {
        return;
    }
    let backend = AnthropicBackend::from_env();
    let req = pinned_request();
    let start = Instant::now();
    let stream = backend
        .stream(req)
        .await
        .expect("Anthropic stream construction must succeed with valid key");
    let total_elapsed_before_drain = start.elapsed();
    let (count, latencies) = drain_with_timing(stream).await;
    let p95_ms = p95(latencies.clone());
    eprintln!(
        "Anthropic: chunks={count} setup={:?} p95_inter_chunk={p95_ms}ms (budget {P95_INTER_CHUNK_BUDGET_MS}ms); samples={latencies:?}",
        total_elapsed_before_drain,
    );
    assert!(
        count >= MIN_CHUNKS,
        "Anthropic stream must produce ≥{MIN_CHUNKS} chunks for the pinned prompt; got {count}"
    );
    assert!(
        p95_ms <= P95_INTER_CHUNK_BUDGET_MS,
        "Anthropic p95 inter-chunk latency = {p95_ms}ms (budget {P95_INTER_CHUNK_BUDGET_MS}ms). \
         Indicates either upstream slowdown (provider issue) or our parser is dropping wakes."
    );
}

// ─── section 2 — OpenAI real provider lane ─────────────────────────────────

#[tokio::test]
#[ignore = "real-provider — opt-in via AXON_RUN_REAL_PROVIDER_TEST"]
async fn openai_real_stream_p95_inter_chunk_under_budget() {
    if maybe_skip("OPENAI_API_KEY", "OpenAI") {
        return;
    }
    let backend = OpenAIBackend::from_env();
    let req = pinned_request();
    let stream = backend
        .stream(req)
        .await
        .expect("OpenAI stream construction must succeed with valid key");
    let (count, latencies) = drain_with_timing(stream).await;
    let p95_ms = p95(latencies.clone());
    eprintln!(
        "OpenAI: chunks={count} p95_inter_chunk={p95_ms}ms (budget {P95_INTER_CHUNK_BUDGET_MS}ms); samples={latencies:?}"
    );
    assert!(count >= MIN_CHUNKS);
    assert!(
        p95_ms <= P95_INTER_CHUNK_BUDGET_MS,
        "OpenAI p95 inter-chunk latency = {p95_ms}ms (budget {P95_INTER_CHUNK_BUDGET_MS}ms)"
    );
}

// ─── section 3 — Gemini real provider lane ─────────────────────────────────

#[tokio::test]
#[ignore = "real-provider — opt-in via AXON_RUN_REAL_PROVIDER_TEST"]
async fn gemini_real_stream_p95_inter_chunk_under_budget() {
    if maybe_skip("GEMINI_API_KEY", "Gemini") {
        return;
    }
    let backend = GeminiBackend::from_env();
    let req = pinned_request();
    let stream = backend
        .stream(req)
        .await
        .expect("Gemini stream construction must succeed with valid key");
    let (count, latencies) = drain_with_timing(stream).await;
    let p95_ms = p95(latencies.clone());
    eprintln!(
        "Gemini: chunks={count} p95_inter_chunk={p95_ms}ms (budget {P95_INTER_CHUNK_BUDGET_MS}ms); samples={latencies:?}"
    );
    assert!(count >= MIN_CHUNKS);
    assert!(
        p95_ms <= P95_INTER_CHUNK_BUDGET_MS,
        "Gemini p95 inter-chunk latency = {p95_ms}ms (budget {P95_INTER_CHUNK_BUDGET_MS}ms)"
    );
}

// ─── section 4 — Vertical canonical patterns ───────────────────────────────
//
// Banking / Government / Legal / Medicine each run a canonical
// flow shape through a real provider. The streaming wire body is
// what regulated adopters depend on for audit-defensible AI.
// Each pattern uses ANTHROPIC_API_KEY by default (the most-tested
// provider in the cycle); adopters can re-run on OpenAI or Gemini
// by adjusting the backend constructor.

fn vertical_prompt(domain: &str, scenario: &str) -> String {
    format!(
        "You are a {domain} compliance assistant. Briefly assess this scenario in 3 short steps: {scenario}",
    )
}

async fn run_vertical_lane(domain: &str, scenario: &str) {
    let backend = AnthropicBackend::from_env();
    let req = ChatRequest {
        messages: vec![Message::user(vertical_prompt(domain, scenario))],
        max_tokens: Some(256),
        stream: true,
        ..Default::default()
    };
    let stream = backend
        .stream(req)
        .await
        .expect("vertical lane stream construction succeeds");
    let (count, latencies) = drain_with_timing(stream).await;
    let p95_ms = p95(latencies);
    eprintln!(
        "Vertical [{domain}]: chunks={count} p95_inter_chunk={p95_ms}ms"
    );
    assert!(
        count >= MIN_CHUNKS,
        "{domain} vertical lane stream MUST produce ≥{MIN_CHUNKS} chunks"
    );
    // Inter-chunk timing is not strictly asserted for verticals
    // (longer prompt → larger Anthropic chunks → naturally
    // higher gaps); we just verify the stream isn't stuck.
    assert!(
        p95_ms < 2000,
        "{domain} vertical p95 inter-chunk latency = {p95_ms}ms; \
         likely upstream stall (budget: <2s sanity gate, NOT D3's 100ms)"
    );
}

#[tokio::test]
#[ignore = "real-provider — opt-in via AXON_RUN_REAL_PROVIDER_TEST"]
async fn vertical_banking_pci_dss_loan_decision() {
    if maybe_skip("ANTHROPIC_API_KEY", "Banking-vertical") {
        return;
    }
    run_vertical_lane(
        "Banking",
        "A small-business loan applicant has 18 months operating history, \
         $400k revenue, and a 680 FICO. PCI DSS Req 10 audit trail required.",
    )
    .await;
}

#[tokio::test]
#[ignore = "real-provider — opt-in via AXON_RUN_REAL_PROVIDER_TEST"]
async fn vertical_government_fedramp_benefits_eligibility() {
    if maybe_skip("ANTHROPIC_API_KEY", "Government-vertical") {
        return;
    }
    run_vertical_lane(
        "Government",
        "Determine SNAP eligibility for a household of 3 with $2,400 monthly \
         gross income. FedRAMP AU-2 audit retention required.",
    )
    .await;
}

#[tokio::test]
#[ignore = "real-provider — opt-in via AXON_RUN_REAL_PROVIDER_TEST"]
async fn vertical_legal_fre_502_privilege_assessment() {
    if maybe_skip("ANTHROPIC_API_KEY", "Legal-vertical") {
        return;
    }
    run_vertical_lane(
        "Legal",
        "Assess whether a document is attorney-client privileged: an email \
         from in-house counsel to a non-lawyer board member discussing \
         pending litigation strategy. FRE 502 waiver-doctrine considerations.",
    )
    .await;
}

#[tokio::test]
#[ignore = "real-provider — opt-in via AXON_RUN_REAL_PROVIDER_TEST"]
async fn vertical_medicine_21cfr_clinical_decision_support() {
    if maybe_skip("ANTHROPIC_API_KEY", "Medicine-vertical") {
        return;
    }
    run_vertical_lane(
        "Medicine",
        "A 67-year-old male presents with chest pain, diaphoresis, and \
         elevated troponin. Provide differential and next-step considerations. \
         21 CFR Part 11 section 11.10 audit required.",
    )
    .await;
}

// ─── section 5 — Sanity: no-key-no-flake gate ──────────────────────────────
//
// The maybe_skip helper short-circuits with an `eprintln!` when
// the provider key isn't set. This test verifies the helper
// itself: with a guaranteed-unset env var, it returns true.

#[tokio::test]
async fn maybe_skip_returns_true_when_env_var_is_missing() {
    let env_var_name = "AXON_REAL_PROVIDER_GUARANTEED_UNSET_DO_NOT_SET_THIS";
    std::env::remove_var(env_var_name);
    assert!(maybe_skip(env_var_name, "test-provider"));
}

#[tokio::test]
async fn maybe_skip_returns_false_when_env_var_is_set() {
    let env_var_name = "AXON_REAL_PROVIDER_TESTING_TEMP_SET_FLAG";
    std::env::set_var(env_var_name, "1");
    assert!(!maybe_skip(env_var_name, "test-provider"));
    std::env::remove_var(env_var_name);
}

// ─── section 6 — p95 helper correctness (deterministic) ────────────────────

#[tokio::test]
async fn p95_helper_returns_max_for_small_sample() {
    // < 20 samples → return the largest value (honest, no
    // interpolation shenanigans).
    let latencies = vec![10u128, 20, 30, 40, 50];
    assert_eq!(p95(latencies), 50);
}

#[tokio::test]
async fn p95_helper_returns_index_95pct_for_large_sample() {
    // 100 samples [1..=100]; p95 index = floor(0.95 * 100) = 95.
    // Sorted samples[95] = 96 (0-indexed, value at 95 is 96).
    let latencies: Vec<u128> = (1..=100).collect();
    assert_eq!(p95(latencies), 96);
}

#[tokio::test]
async fn p95_helper_handles_empty_input() {
    assert_eq!(p95(Vec::<u128>::new()), 0);
}

#[tokio::test]
async fn drain_with_timing_yields_n_minus_one_latencies_for_n_chunks() {
    // 4 chunks → 3 inter-chunk gaps.
    use axon::backends::ChatChunk;
    let chunks: Vec<Result<ChatChunk, axon::backends::error::BackendError>> = vec![
        Ok(ChatChunk { delta: "a".into(), ..Default::default() }),
        Ok(ChatChunk { delta: "b".into(), ..Default::default() }),
        Ok(ChatChunk { delta: "c".into(), ..Default::default() }),
        Ok(ChatChunk { delta: "d".into(), ..Default::default() }),
    ];
    let s = Box::pin(futures::stream::iter(chunks));
    // Cast to the trait we expect; futures::stream::iter is
    // Stream<Item = T>, the bound on drain_with_timing already
    // covers it. Add a tiny artificial gap so latencies are
    // measurable (not zero).
    let s = Box::pin(s.then(|item| async move {
        tokio::time::sleep(Duration::from_millis(5)).await;
        item
    }));
    let (count, latencies) = drain_with_timing(s).await;
    assert_eq!(count, 4);
    assert_eq!(
        latencies.len(),
        3,
        "4 chunks → 3 inter-chunk gaps; got {}",
        latencies.len()
    );
}
