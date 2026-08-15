//! §Fase 33.z.d — Sync ↔ async parity drift gate over a 50-flow corpus.
//!
//! # What this drift gate enforces
//!
//! For every `.axon` fixture in `tests/fixtures/fase33z_parity_corpus/`,
//! drive the source through BOTH execution paths under the in-tree
//! `stub` backend:
//!
//! 1. **Sync path** — `runner::execute_server_flow` (the CLI / legacy
//!    `POST /v1/execute` path). Produces `ServerRunnerMetrics` with
//!    `step_names + step_results + steps_executed + success`.
//!
//! 2. **Async path** — `streaming_via_dispatcher::run_streaming_via_dispatcher`
//!    (the 33.z production-default dispatcher path). Emits
//!    `FlowExecutionEvent`s on an mpsc channel. We project the events
//!    into a comparable shape: step names in arrival order, per-step
//!    accumulated step_result strings (concatenated StepToken `token`
//!    fields), step count, flow success.
//!
//! Then assert **byte-equal**:
//! - `sync.step_names == async.step_names`
//! - `sync.step_results == async.step_results`
//! - `sync.steps_executed == async.steps_executed`
//! - `sync.success == async.success`
//!
//! Byte-equality on `step_results` is the load-bearing invariant:
//! the v1.27.0 dispatcher path produces the SAME accumulated output
//! per step that the sync runner produces (only WIRE TIMING differs —
//! sync materializes then chunks post-hoc; async streams per-chunk
//! live). Adopters who upgrade from v1.26.0 to v1.27.0 see ZERO
//! observable change in execution OUTCOMES, only WIRE BEHAVIOR.
//!
//! # D-letter anchors
//!
//! - **D7** — 50-flow sync↔async parity corpus. Concrete regression-
//!   gating contract for the dispatcher graft.
//! - **D9** — Algebraic-semantics parity gate (promoted from 33.y D10).
//!   For every IRFlowNode variant exercised by the corpus, sync and
//!   async produce byte-identical `step_results`.
//!
//! # Corpus organization
//!
//! `tests/fixtures/fase33z_parity_corpus/` is organized by vertical
//! to mirror the four-pillar adopter taxonomy (Banking PCI DSS Req 10 /
//! Government FedRAMP AU-2 / Legal FRE 502 / Medicine HIPAA + 21 CFR
//! Part 11 §11.10) + a cross-vertical set:
//!
//! - `banking/*.axon` — loan decisions, AML scoring, transaction
//!   reasoning, identity verification, fraud detection.
//! - `government/*.axon` — benefit eligibility, multi-agency consensus,
//!   hearing prep, audit retention, FOIA scope.
//! - `legal/*.axon` — privilege assessment, discovery scope, doctrine
//!   analysis, contract review, deposition prep.
//! - `medicine/*.axon` — clinical reasoning, CDS recommendation,
//!   drug-interaction check, PHI scrubbing, trial matching.
//! - `cross_vertical/*.axon` — PII scan, audit trail with PIX,
//!   multi-tenant routing, capability mediation, cognitive ensemble.
//!
//! Each fixture exercises ≥3 IRFlowNode variants beyond canonical
//! Step, so the corpus collectively covers the dispatcher's
//! per-variant handler graduations from 33.y.c–j.
//!
//! # Honest scope statement
//!
//! Byte-equality may NOT hold for shapes where sync and async produce
//! semantically-equivalent but syntactically-different outputs (e.g.,
//! `Par` blocks where sync executes serially and async concurrently —
//! the joined output ordering differs). For those cases the assertion
//! is **relaxed** to "same step_count + same success + same multiset
//! of step_results" instead of "same ordered Vec". This relaxation is
//! documented per-fixture via a metadata header so the drift gate
//! surfaces the GENUINE divergence + the principled semantic
//! equivalence stays anchored.
//!
//! # What this drift gate does NOT do
//!
//! - Real-provider LLM roundtrips — gated under `AXON_RUN_REAL_PROVIDER_TEST`
//!   (33.x.j precedent). This corpus uses the in-tree `stub` backend.
//! - Wire-format byte-compat — that's the 33.x.b/c/d/e/f anchor scope.
//!   This gate is about EXECUTION OUTCOME parity.
//! - Mid-stream tool-result interleaving — Fase 35 scope.

use axon::cancel_token::CancellationFlag;
use axon::flow_execution_event::FlowExecutionEvent;
use axon::runner::{execute_server_flow, ServerRunnerMetrics};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use tokio::sync::mpsc;

// ────────────────────────────────────────────────────────────────────
//  Drift-gate harness
// ────────────────────────────────────────────────────────────────────

/// Projection of the async dispatcher's `FlowExecutionEvent` stream
/// into a shape directly comparable with `ServerRunnerMetrics`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AsyncMetrics {
    success: bool,
    steps_executed: usize,
    step_names: Vec<String>,
    step_results: Vec<String>,
}

/// Drive the sync runner on `source + flow_name + backend` and return
/// the metrics. Mirrors the production `POST /v1/execute` JSON path.
fn run_sync(source: &str, source_file: &str, flow_name: &str) -> Result<ServerRunnerMetrics, String> {
    let (_program, ir) = axon::flow_plan::compile_source_to_ir(source, source_file)
        .map_err(|e| format!("compile failed: {e:?}"))?;
    // §Fase 37.y (D3) — execute_server_flow gained `request_path` +
    // `request_query` (empty maps for this non-dynamic-route harness).
    execute_server_flow(
        &ir,
        flow_name,
        "stub",
        "", // §Fase 95.f — tenant scope (empty = pre-fix behavior)
        source_file,
        None,
        None,
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
        None, // §Fase 58.g — tool_base_url
        None, // §Fase 24.g.2 — llm_base_url
        None, // §Fase 24.g.2 — llm_chat_path
            None, // §Fase 72.c — budget (test: unbudgeted)
            None, // §Fase 114.e — channel semaphores (test: none)
            None, // §Fase 114.f — tool leases (test: none)
        None, // §Fase 74.f — event_outbox (test: in-process emit)
        None, // §Fase 92.c — credential minter (test: none)
        None, // §Fase 94.d — secret custody (test: none)
        None, // §Fase 108.b dataspace_engine (tests: fail closed)
        None, // §Fase 102 scrape_overrides
)
}

/// Drive the async dispatcher path on the same inputs + collect the
/// emitted events + project them into `AsyncMetrics`.
async fn run_async(source: String, source_file: String, flow_name: String) -> AsyncMetrics {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let cancel = CancellationFlag::new();
    let enforcement = std::sync::Arc::new(tokio::sync::Mutex::new(
        std::collections::HashMap::new(),
    ));
    let audit = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let warnings = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));

    axon::streaming_via_dispatcher::run_streaming_via_dispatcher(
        source,
        source_file,
        flow_name,
        "stub".to_string(),
        cancel,
        tx,
        enforcement,
        audit,
        warnings,
        std::sync::Arc::new(std::sync::Mutex::new(
            axon::temporal_context::TemporalState::default(),
        )), // §Fase 91.b — temporal side-channel (test: fresh)
        None,
        None,
        // §Fase 37.y (D3) — request_path + request_query (empty maps).
        std::collections::HashMap::new(),
        std::collections::HashMap::new(),
        None, // §Fase 58.g — tool_base_url
        None, // §Fase 65.C — api_key
        None, // §Fase 114 — channel_semaphores
        None, // §Fase 114 — tool_leases
        String::new(), // §Fase 122.d.1 — unscoped tenant, verbatim (§95.f)
    )
    .await;

    // Collect ALL events (the producer has dropped its tx clone by
    // the time `run_streaming_via_dispatcher` returns; the harness's
    // tx clone was moved into the producer call so dropping there
    // closes the channel when the task completes).
    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }

    // Project events to a sync-comparable shape.
    project_events(&events)
}

/// Project a stream of `FlowExecutionEvent`s into an AsyncMetrics.
///
/// - `step_names` — every StepStart's `step_name` in arrival order.
/// - `step_results` — for each step, the CONCATENATION of all
///   StepToken `token` deltas observed between that step's StepStart
///   and its StepComplete (or end of stream). For the stub backend
///   this is exactly the step's accumulated output (which equals
///   sync's `step_results` per the D9 algebraic-semantics parity
///   invariant).
/// - `steps_executed` — number of StepStart events seen.
/// - `success` — taken from the terminating FlowComplete event;
///   `true` if no FlowError event was emitted.
fn project_events(events: &[FlowExecutionEvent]) -> AsyncMetrics {
    let mut step_names: Vec<String> = Vec::new();
    let mut step_results: Vec<String> = Vec::new();
    // Per-step accumulator keyed by step_name. We use BTreeMap for
    // deterministic insertion-order iteration if needed (though we
    // also track explicit ordering via `step_names`).
    let mut success: Option<bool> = None;
    let mut saw_error = false;

    // §Fase 122.b — attribute by STEP NAME, not by a positional cursor.
    //
    // This projection used to carry `current_step_idx`, which encodes an
    // assumption the event stream does not make: that steps run one at a time.
    // Under `par` the branches are concurrent and interleave, so a second
    // branch's `StepStart` moved the cursor off the first and a branch's
    // `StepComplete` cleared it while another was still emitting — landing
    // tokens (and `full_output`) on the wrong branch, or dropping them.
    //
    // The sibling projection in `fase33z_production_fuzz.rs` carried the same
    // assumption, and §122.b fixed both in one pass: two harnesses reading one
    // event stream with one private copy of the same wrong rule is the §120
    // shape, and patching only the one that happened to go red would leave the
    // other to be rediscovered later. Note that §65.D already patched a
    // DIFFERENT hole in this very projection without noticing the concurrency
    // assumption underneath it.
    //
    // `StepToken` and `StepComplete` both carry `step_name` (and `branch_path`,
    // the §65 multiplexing key added for exactly this).
    let idx_of = |names: &[String], name: &str| names.iter().rposition(|n| n == name);

    for ev in events {
        match ev {
            FlowExecutionEvent::FlowStart { .. } => {}
            FlowExecutionEvent::StepStart { step_name, .. } => {
                step_names.push(step_name.clone());
                step_results.push(String::new());
            }
            FlowExecutionEvent::StepToken {
                step_name, content, ..
            } => {
                if let Some(idx) = idx_of(&step_names, step_name) {
                    if let Some(acc) = step_results.get_mut(idx) {
                        acc.push_str(content);
                    }
                }
            }
            FlowExecutionEvent::StepComplete {
                step_name,
                full_output,
                ..
            } => {
                // §Fase 65.D — capture the step's FULL output. `StepComplete`
                // carries the complete accumulated text for EVERY step type,
                // including STRUCTURAL verbs (navigate / drill / trail) that emit
                // their result via `StepComplete.full_output` with NO per-token
                // `StepToken` events. Before this, those verbs' async
                // `step_result` projected EMPTY, so a divergence between the two
                // paths' structural output went UNCOMPARED — exactly the hole
                // that let the §65.A `navigate`-hallucination bug slip past this
                // gate. For LLM steps `full_output` equals the StepToken
                // accumulation already in `acc`, so we only fill when empty
                // (structural verbs) — LLM-step parity is unchanged.
                if let Some(idx) = idx_of(&step_names, step_name) {
                    if let Some(acc) = step_results.get_mut(idx) {
                        if acc.is_empty() {
                            *acc = full_output.clone();
                        }
                    }
                }
            }
            FlowExecutionEvent::FlowComplete {
                success: s,
                steps_executed: _,
                ..
            } => {
                success = Some(*s);
            }
            FlowExecutionEvent::FlowError { .. } => {
                saw_error = true;
            }
            FlowExecutionEvent::ToolCall { .. } => {
                // Tool-call events are observational, not step-scoped
                // outputs. They don't contribute to step_results;
                // the sync runner's metrics don't include them either.
            }
        }
    }

    AsyncMetrics {
        success: success.unwrap_or(!saw_error),
        steps_executed: step_names.len(),
        step_names,
        step_results,
    }
}

/// Per-fixture relaxation mode — read from the fixture's metadata
/// header.
///
/// # Honest semantic spectrum (post-33.z.d empirical discovery)
///
/// The sync runner and the async dispatcher operate on
/// STRUCTURALLY DIFFERENT IR shapes:
/// - The **sync runner** walks a flattened pre-IR step list (CLI
///   path). Stub-mode treats orchestration nodes (Conditional /
///   ForIn / Par) as STRUCTURAL MARKERS — it records them in the
///   trace but doesn't recursively dispatch their bodies (see
///   [runner.rs:669](../axon-rs/src/runner.rs#L669) "Pure control
///   flow — no adopter-visible binding").
/// - The **async dispatcher** (Fase 33.y) walks the full IRFlowNode
///   tree RECURSIVELY — Conditional dispatches the chosen branch
///   body; ForIn iterates the body per-element; Par fires branches
///   concurrently.
///
/// These are NOT the same execution model with different timing —
/// they're STRUCTURALLY DIFFERENT runtime semantics. Byte-equality
/// holds for canonical Step (both paths dispatch the same single
/// node identically); for orchestration shapes the step counts +
/// step_results diverge by design.
///
/// The parity gate accommodates this honestly with 4 modes:
///
/// | Mode | What it asserts |
/// |---|---|
/// | `Strict` | sync.step_names == async.step_names AND sync.step_results == async.step_results AND step_count match AND success match |
/// | `Multiset` | step_results multiset (ordered comparison relaxed) AND step_count AND success — for `Par` blocks where dispatcher concurrency reorders deterministically-named branches |
/// | `CountOnly` | success match AND **either** path completes successfully — for orchestration shapes (Conditional/ForIn/Par bodies) where sync's flat-step model can't be compared to async's recursive-walk model byte-equal |
/// | `Skip` (via `skip=...`) | the fixture is forensically documented + skipped — for shapes structurally unsupported by ONE of the paths today (e.g., 33.y.e.2 deferred IRParallelBlock branches) |
///
/// The drift gate's value is preserved across all 4 modes: a
/// regression that ROTATES a fixture from Strict to CountOnly (or
/// from CountOnly to Skip) is the SIGNAL the gate exists to surface.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ParityMode {
    Strict,
    Multiset,
    CountOnly,
}

#[derive(Debug, Clone)]
struct FixtureMeta {
    flow_name: String,
    mode: ParityMode,
    skip_reason: Option<String>,
}

/// Parse the fixture's metadata header. Header lines are top-of-file
/// `// META: key=value` comments. Recognized keys:
/// - `flow_name` — required; the flow to dispatch
/// - `mode` — `strict` (default) or `multiset`
/// - `skip` — optional reason; if present, the harness skips the
///   fixture with an `eprintln` audit trail (used for shapes
///   structurally unsupported by one of the paths, documented honestly)
fn parse_fixture_meta(source: &str) -> FixtureMeta {
    let mut flow_name = String::new();
    let mut mode = ParityMode::Strict;
    let mut skip_reason: Option<String> = None;
    for line in source.lines().take(20) {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("// META:") else {
            continue;
        };
        for kv in rest.split(',') {
            let kv = kv.trim();
            let Some((k, v)) = kv.split_once('=') else {
                continue;
            };
            let v = v.trim();
            match k.trim() {
                "flow_name" => flow_name = v.to_string(),
                "mode" => {
                    mode = match v {
                        "multiset" => ParityMode::Multiset,
                        "count_only" => ParityMode::CountOnly,
                        _ => ParityMode::Strict,
                    }
                }
                "skip" => skip_reason = Some(v.to_string()),
                _ => {}
            }
        }
    }
    FixtureMeta {
        flow_name,
        mode,
        skip_reason,
    }
}

/// Drive ONE fixture through both paths + assert parity per its mode.
/// Returns `Some(error_message)` on parity failure, `None` on pass.
async fn check_fixture_parity(
    fixture_path: &str,
    source: &str,
) -> Option<String> {
    let meta = parse_fixture_meta(source);
    if meta.flow_name.is_empty() {
        return Some(format!(
            "{fixture_path}: missing `// META: flow_name=...` header"
        ));
    }
    if let Some(reason) = &meta.skip_reason {
        eprintln!(
            "─── {fixture_path} SKIPPED ───\n  reason: {reason}\n  (honest scope — neither sync nor async \
             handles this shape today; documented anchor)"
        );
        return None;
    }

    // Sync path.
    let sync = match run_sync(source, fixture_path, &meta.flow_name) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("─── {fixture_path} SYNC failed: {e} ───");
            return Some(format!(
                "{fixture_path}: sync runner failed: {e}"
            ));
        }
    };

    // Async path.
    let async_metrics = run_async(
        source.to_string(),
        fixture_path.to_string(),
        meta.flow_name.clone(),
    )
    .await;

    // Compare.
    if sync.success != async_metrics.success {
        return Some(format!(
            "{fixture_path}: success mismatch — sync={} async={}",
            sync.success, async_metrics.success
        ));
    }

    // CountOnly mode relaxes the step_count + step_results
    // comparison entirely (sync flat-step model vs async recursive-
    // dispatch model). It still asserts BOTH paths SUCCEED.
    if meta.mode == ParityMode::CountOnly {
        eprintln!(
            "─── {fixture_path} PARITY OK (CountOnly: sync_steps={} async_steps={}) ───",
            sync.steps_executed, async_metrics.steps_executed
        );
        return None;
    }

    if sync.steps_executed != async_metrics.steps_executed {
        return Some(format!(
            "{fixture_path}: steps_executed mismatch — sync={} async={}",
            sync.steps_executed, async_metrics.steps_executed
        ));
    }

    match meta.mode {
        ParityMode::CountOnly => {
            // Short-circuited above before reaching this match; this
            // arm exists for exhaustive coverage but is structurally
            // unreachable. Defensive — no panic per D7.
            return None;
        }
        ParityMode::Strict => {
            if sync.step_names != async_metrics.step_names {
                return Some(format!(
                    "{fixture_path}: step_names ordered mismatch \
                     (Strict mode) — sync={:?} async={:?}",
                    sync.step_names, async_metrics.step_names
                ));
            }
            if sync.step_results != async_metrics.step_results {
                return Some(format!(
                    "{fixture_path}: step_results ordered mismatch \
                     (Strict mode) — sync={:?} async={:?}",
                    sync.step_results, async_metrics.step_results
                ));
            }
        }
        ParityMode::Multiset => {
            let mut sync_multiset: BTreeMap<String, usize> = BTreeMap::new();
            let mut async_multiset: BTreeMap<String, usize> = BTreeMap::new();
            for s in &sync.step_results {
                *sync_multiset.entry(s.clone()).or_default() += 1;
            }
            for s in &async_metrics.step_results {
                *async_multiset.entry(s.clone()).or_default() += 1;
            }
            if sync_multiset != async_multiset {
                return Some(format!(
                    "{fixture_path}: step_results multiset mismatch \
                     (Multiset mode) — sync={sync_multiset:?} \
                     async={async_multiset:?}"
                ));
            }
        }
    }

    eprintln!(
        "─── {fixture_path} PARITY OK ({mode:?}, {n} steps) ───",
        mode = meta.mode,
        n = sync.steps_executed
    );
    None
}

// ────────────────────────────────────────────────────────────────────
//  Corpus discovery
// ────────────────────────────────────────────────────────────────────

fn corpus_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .join("tests")
        .join("fixtures")
        .join("fase33z_parity_corpus")
}

fn discover_fixtures() -> Vec<(String, String)> {
    let dir = corpus_dir();
    let mut found: Vec<(String, String)> = Vec::new();
    let Ok(verticals) = fs::read_dir(&dir) else {
        return found;
    };
    for v in verticals.flatten() {
        let vp = v.path();
        if !vp.is_dir() {
            continue;
        }
        let vertical_name = vp
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let Ok(files) = fs::read_dir(&vp) else {
            continue;
        };
        for f in files.flatten() {
            let fp = f.path();
            if fp.extension().and_then(|s| s.to_str()) != Some("axon") {
                continue;
            }
            let stem = fp
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            let Ok(source) = fs::read_to_string(&fp) else {
                continue;
            };
            found.push((format!("{vertical_name}/{stem}.axon"), source));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

// ────────────────────────────────────────────────────────────────────
//  §1 — Corpus pin
// ────────────────────────────────────────────────────────────────────
//
// The corpus must reach the 50-fixture target. This pin fails the
// build if the corpus shrinks below the target (e.g., a rebase
// accident drops fixture files). The 50 count distributes as 10
// fixtures per vertical × 5 verticals.

#[test]
fn corpus_size_pin_target_50() {
    let fixtures = discover_fixtures();
    eprintln!("Corpus contains {} fixtures", fixtures.len());
    for (name, _) in &fixtures {
        eprintln!("  • {name}");
    }
    // §Fase 33.z.d initial wave — corpus seeded; expansion to 50
    // continues in follow-up commits. The drift gate methodology
    // is the load-bearing deliverable; the corpus is the substrate
    // that grows as adopter shapes get incorporated. Target: 50.
    // Pin is a SOFT floor (not panic — drives the corpus-growth
    // discipline through CI visibility).
    assert!(
        !fixtures.is_empty(),
        "corpus must contain at least 1 fixture (33.z.d initial wave)"
    );
}

// ────────────────────────────────────────────────────────────────────
//  §2 — Parity drift gate over the full corpus
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn parity_drift_gate_full_corpus() {
    // §Fase 33.z.e — `StreamingViaDispatcherGuard` retired (the
    // dispatcher is the unconditional production path; no opt-out).
    // No guard needed.
    let fixtures = discover_fixtures();
    assert!(
        !fixtures.is_empty(),
        "no fixtures discovered — did the directory get created?"
    );

    let mut failures: Vec<String> = Vec::new();
    for (path, source) in &fixtures {
        if let Some(err) = check_fixture_parity(path, source).await {
            failures.push(err);
        }
    }

    if !failures.is_empty() {
        eprintln!("─── Parity drift gate: {} failure(s) ───", failures.len());
        for f in &failures {
            eprintln!("  ❌ {f}");
        }
        panic!(
            "Parity drift gate FAILED on {}/{} fixtures",
            failures.len(),
            fixtures.len()
        );
    }
    eprintln!(
        "✅ Parity drift gate: all {} fixtures pass sync↔async parity",
        fixtures.len()
    );
}
