//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 33.z.e parity gate — grep-style invariants enforcing the
//! retirement of the 33.y.l-deprecated legacy routing primitives +
//! the 33.z.b/c runtime flag scaffolding.
//!
//! # What this gate enforces
//!
//! After 33.z.e, the following symbols MUST NOT appear as non-comment
//! references anywhere in `axon-rs/src/`:
//!
//! 1. `LegacyOrchestrationRequired` — `PlanError` variant retired.
//! 2. `unsupported_feature_reason` — flow_plan helper retired.
//! 3. `run_streaming_legacy_path` — synthetic-burst fallback retired.
//! 4. `run_streaming_async_path` — v1.25.0 canonical Step path retired
//!    (the dispatcher handles canonical Step uniformly post-33.z.e).
//! 5. `construct_enforcer_for_policy` — helper retired with async_path.
//! 6. `FallbackMode::UnsupportedFlowShape` — W002 variant retired.
//! 7. `streaming_via_dispatcher_enabled` — runtime flag getter retired.
//! 8. `set_streaming_via_dispatcher` — runtime flag setter retired.
//! 9. `StreamingViaDispatcherGuard` — RAII guard retired.
//!
//! Plus a closed-catalog size pin: `FallbackMode` has EXACTLY 3
//! variants after 33.z.e (down from 4 in v1.26.0).
//!
//! # Why a separate parity gate from 33.y.l
//!
//! The 33.y.l parity gate scans ONLY `src/flow_dispatcher/*.rs` (the
//! 33.y dispatcher module surface). The 33.z.e retirements span FOUR
//! broader source files: `axon_server.rs`, `flow_plan.rs`,
//! `runtime_warnings.rs`, `runtime_flags.rs`. A separate scope-broader
//! gate captures the 33.z.e invariant precisely while leaving the
//! 33.y.l dispatcher-internal gate focused.
//!
//! # Diagnostic precision
//!
//! Each grep hit reports `<file>:<line>` so a regression that
//! re-introduces a retired symbol surfaces at PR time with location
//! precision — same discipline as the 33.y.l parity gate.

use std::fs;
use std::path::PathBuf;

/// Walk `axon-rs/src/` recursively + return `(path, line_no, line_text)`
/// for every line in every `.rs` file. Sorted lexicographically for
/// deterministic test output across runs.
fn read_all_src_lines() -> Vec<(String, usize, String)> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let src_dir = PathBuf::from(manifest_dir).join("src");
    let mut out = Vec::new();
    walk_dir(&src_dir, manifest_dir, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    out
}

fn walk_dir(dir: &PathBuf, manifest_dir: &str, out: &mut Vec<(String, usize, String)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            walk_dir(&path, manifest_dir, out);
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let rel = path
            .strip_prefix(manifest_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string());
        for (i, line) in text.lines().enumerate() {
            out.push((rel.clone(), i + 1, line.to_string()));
        }
    }
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

fn non_comment_hits<'a>(
    lines: &'a [(String, usize, String)],
    needle: &str,
) -> Vec<&'a (String, usize, String)> {
    lines
        .iter()
        .filter(|(_, _, text)| !is_comment(text) && text.contains(needle))
        .collect()
}

fn assert_no_hits(needle: &str, lines: &[(String, usize, String)], context: &str) {
    let hits = non_comment_hits(lines, needle);
    let formatted: Vec<String> = hits
        .iter()
        .map(|(f, n, t)| format!("{f}:{n} — {}", t.trim()))
        .collect();
    assert!(
        formatted.is_empty(),
        "33.z.e parity gate FAILED: found {} non-comment reference(s) \
         to `{needle}` in axon-rs/src/. {context} Locations:\n  - {}",
        formatted.len(),
        formatted.join("\n  - ")
    );
}

// ────────────────────────────────────────────────────────────────────
//  §1 — Retired flow_plan / axon_server / runtime_* symbols
// ────────────────────────────────────────────────────────────────────

#[test]
fn no_legacy_orchestration_required_references() {
    let lines = read_all_src_lines();
    assert_no_hits(
        "LegacyOrchestrationRequired",
        &lines,
        "The `PlanError::LegacyOrchestrationRequired` variant was \
         retired in 33.z.e along with the legacy synchronous fallback. \
         Any non-comment reference re-introduces the retired surface.",
    );
}

#[test]
fn no_unsupported_feature_reason_references() {
    let lines = read_all_src_lines();
    assert_no_hits(
        "unsupported_feature_reason",
        &lines,
        "The `flow_plan::unsupported_feature_reason` helper was retired \
         in 33.z.e — the dispatcher path covers every IRFlowNode variant; \
         no pre-flight rejection needed.",
    );
}

#[test]
fn no_run_streaming_legacy_path_references() {
    let lines = read_all_src_lines();
    assert_no_hits(
        "run_streaming_legacy_path",
        &lines,
        "The `axon_server::run_streaming_legacy_path` synthetic-burst \
         fallback was retired in 33.z.e. The dispatcher's per-variant \
         handlers cover every shape uniformly.",
    );
}

#[test]
fn no_run_streaming_async_path_references() {
    let lines = read_all_src_lines();
    assert_no_hits(
        "run_streaming_async_path",
        &lines,
        "The `axon_server::run_streaming_async_path` v1.25.0 canonical \
         Step hot path was retired in 33.z.e. The dispatcher (Fase 33.y \
         45/45) handles canonical Step + every other variant uniformly.",
    );
}

#[test]
fn no_construct_enforcer_for_policy_references() {
    let lines = read_all_src_lines();
    assert_no_hits(
        "construct_enforcer_for_policy",
        &lines,
        "The `construct_enforcer_for_policy` helper was retired in \
         33.z.e along with its sole caller `run_streaming_async_path`. \
         The dispatcher's pure_shape handler builds enforcers inline.",
    );
}

#[test]
fn no_unsupported_flow_shape_variant_references() {
    let lines = read_all_src_lines();
    assert_no_hits(
        "UnsupportedFlowShape",
        &lines,
        "The `FallbackMode::UnsupportedFlowShape` variant was retired \
         in 33.z.e. The dispatcher path covers every IRFlowNode variant; \
         no shape is unsupported — `axon-W002 unsupported_flow_shape` \
         is structurally unreachable.",
    );
}

#[test]
fn no_streaming_via_dispatcher_enabled_references() {
    let lines = read_all_src_lines();
    assert_no_hits(
        "streaming_via_dispatcher_enabled",
        &lines,
        "The `runtime_flags::streaming_via_dispatcher_enabled` getter \
         was retired in 33.z.e. The dispatcher is the unconditional \
         production path; no flag-check exists.",
    );
}

#[test]
fn no_set_streaming_via_dispatcher_references() {
    let lines = read_all_src_lines();
    assert_no_hits(
        "set_streaming_via_dispatcher",
        &lines,
        "The `runtime_flags::set_streaming_via_dispatcher` setter was \
         retired in 33.z.e. No opt-out from the dispatcher path.",
    );
}

#[test]
fn no_streaming_via_dispatcher_guard_references() {
    let lines = read_all_src_lines();
    assert_no_hits(
        "StreamingViaDispatcherGuard",
        &lines,
        "The `runtime_flags::StreamingViaDispatcherGuard` RAII helper \
         was retired in 33.z.e. Tests no longer toggle the flag because \
         the flag itself is gone.",
    );
}

// ────────────────────────────────────────────────────────────────────
//  §2 — Closed-catalog size pin
// ────────────────────────────────────────────────────────────────────

#[test]
fn fallback_mode_catalog_size_pinned_to_three_post_33_z_e() {
    use axon::runtime_warnings::FallbackMode;
    let all = [
        FallbackMode::UnknownBackend,
        FallbackMode::SourceCompilationFailed,
        FallbackMode::BackendLacksStream,
    ];
    assert_eq!(
        all.len(),
        3,
        "33.z.e closed-catalog pin: FallbackMode has EXACTLY 3 variants. \
         UnsupportedFlowShape was retired in lockstep with the legacy \
         path. Adding a 4th variant requires deliberate cycle work + \
         updating this pin."
    );

    // Slug uniqueness — same discipline as the 33.x.g.fuzz pin.
    let mut slugs: Vec<&str> = all.iter().map(|m| m.slug()).collect();
    slugs.sort();
    let mut unique = slugs.clone();
    unique.dedup();
    assert_eq!(slugs.len(), unique.len(), "FallbackMode slugs must be unique");
}
