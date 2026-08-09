//! §Fase 33.y.l parity gate — assert the dispatcher is shim-free and
//! D7-compliant.
//!
//! # What this gate exercises
//!
//! 1. **No `unimplemented!()` / `todo!()` / `panic!()` markers** anywhere
//!    in `flow_dispatcher/*.rs` source. D7 mandates that every handler
//!    is born-mature; placeholder markers are forbidden.
//!
//! 2. **No `legacy_shim` references** anywhere in `flow_dispatcher/*.rs`
//!    source (other than this gate's own comment + `mod.rs`'s
//!    retirement notice). 33.y.l retired the shim end-to-end.
//!
//! 3. **No `ShimReason` references** anywhere in `flow_dispatcher/*.rs`
//!    source (other than this gate's own comment + retirement notices).
//!
//! 4. **No `LegacyShimHandled` references** anywhere in
//!    `flow_dispatcher/*.rs` source (other than this gate's own comment
//!    + retirement notices).
//!
//! Diagnostics are precise: when a marker is found, the test names the
//! exact file + line so a maintainer can fix the regression at PR time,
//! not at adopter runtime.
//!
//! # Why a grep gate?
//!
//! The Rust compiler can't reject `unimplemented!()` macros — they
//! type-check just fine. This gate is the smallest deterministic way to
//! enforce the D7 contract at every push. It runs as a regular
//! integration test (no extra tooling, no shell scripts) so it's
//! impossible to skip.

use std::fs;
use std::path::PathBuf;

/// Walk `axon-rs/src/flow_dispatcher` and return `(path, line_no, line_text)`
/// for every line in every `.rs` file under it. Determinism: directory
/// entries sorted lexicographically before reading.
fn read_all_flow_dispatcher_lines() -> Vec<(String, usize, String)> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let dispatcher_dir = PathBuf::from(manifest_dir)
        .join("src")
        .join("flow_dispatcher");

    let mut entries: Vec<PathBuf> = fs::read_dir(&dispatcher_dir)
        .unwrap_or_else(|e| {
            panic!(
                "33.y.l parity gate: failed to read {:?}: {e}",
                dispatcher_dir
            )
        })
        .filter_map(|res| res.ok())
        .map(|d| d.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("rs"))
        .collect();
    entries.sort();

    let mut out = Vec::new();
    for path in entries {
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("33.y.l parity gate: read {path:?}: {e}"));
        let rel = path
            .strip_prefix(manifest_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string());
        for (i, line) in text.lines().enumerate() {
            out.push((rel.clone(), i + 1, line.to_string()));
        }
    }
    out
}

/// Returns true if a line is a comment line (starts with `//` after
/// trimming whitespace) — comments may LEGITIMATELY reference retired
/// names in retirement notices + this gate's own description.
fn is_comment(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//")
}

fn lines_matching<'a>(
    lines: &'a [(String, usize, String)],
    needle: &str,
) -> Vec<&'a (String, usize, String)> {
    lines
        .iter()
        .filter(|(_, _, text)| !is_comment(text) && text.contains(needle))
        .collect()
}

// ────────────────────────────────────────────────────────────────────
//  §1 — No `unimplemented!()` / `todo!()` markers
// ────────────────────────────────────────────────────────────────────

#[test]
fn no_unimplemented_or_todo_markers_in_dispatcher() {
    let lines = read_all_flow_dispatcher_lines();
    let mut hits: Vec<String> = Vec::new();
    for (file, line_no, text) in &lines {
        if is_comment(text) {
            continue;
        }
        if text.contains("unimplemented!(") {
            hits.push(format!("{file}:{line_no} — unimplemented!() macro"));
        }
        if text.contains("todo!(") {
            hits.push(format!("{file}:{line_no} — todo!() macro"));
        }
    }
    assert!(
        hits.is_empty(),
        "33.y.l D7 parity gate FAILED: found {} placeholder marker(s) \
         in flow_dispatcher/*.rs. Every handler must be born-mature \
         (D7 + 33.y.j 45/45 graduation contract). Locations:\n  - {}",
        hits.len(),
        hits.join("\n  - ")
    );
}

// ────────────────────────────────────────────────────────────────────
//  §2 — `panic!()` only inside `#[cfg(test)]` blocks
// ────────────────────────────────────────────────────────────────────
//
// `panic!()` is permissible inside in-module unit-test blocks for
// negative assertions, but a panic on the hot dispatch path is a D7
// violation. We approximate the check by flagging `panic!(` lines that
// don't appear inside a `mod tests` / `#[cfg(test)]` region. Precise
// AST-aware scanning would be over-engineered; a coarse line-based
// check is sufficient for the cascade we care about.

#[test]
fn no_panic_outside_test_modules_in_dispatcher() {
    let lines = read_all_flow_dispatcher_lines();
    // Track file → list of (start_line, end_line) for #[cfg(test)] /
    // mod tests blocks. Heuristic: a line containing
    // "#[cfg(test)]" or "mod tests {" begins a test region; matching
    // brace depth ends it. For 33.y.l we keep it simple — any panic!(
    // line is flagged unless preceded earlier in the same file by
    // a non-comment "#[cfg(test)]" line that hasn't closed yet.
    //
    // Approximation: collect file → line ranges by scanning for the
    // `#[cfg(test)]` marker + counting brace depth thereafter. Coarse
    // but deterministic.

    let mut hits: Vec<String> = Vec::new();
    let mut file_grouped: std::collections::BTreeMap<String, Vec<(usize, String)>> =
        std::collections::BTreeMap::new();
    for (file, line_no, text) in &lines {
        file_grouped
            .entry(file.clone())
            .or_default()
            .push((*line_no, text.clone()));
    }

    for (file, file_lines) in &file_grouped {
        // Compute the inclusive line ranges of all `#[cfg(test)]` regions.
        let mut test_ranges: Vec<(usize, usize)> = Vec::new();
        let mut i = 0;
        while i < file_lines.len() {
            let (lineno, text) = &file_lines[i];
            if !is_comment(text) && (text.contains("#[cfg(test)]") || text.trim_start().starts_with("mod tests")) {
                // Find the opening `{` (on this line or next) + count
                // braces forward.
                let start = *lineno;
                let mut depth = 0i32;
                let mut found_open = false;
                let mut j = i;
                while j < file_lines.len() {
                    let (jn, jt) = &file_lines[j];
                    for ch in jt.chars() {
                        if ch == '{' {
                            depth += 1;
                            found_open = true;
                        } else if ch == '}' {
                            depth -= 1;
                        }
                    }
                    if found_open && depth == 0 {
                        test_ranges.push((start, *jn));
                        i = j;
                        break;
                    }
                    j += 1;
                }
                if !found_open {
                    // Defensive fallback: skip
                    i = j;
                }
            }
            i += 1;
        }

        for (lineno, text) in file_lines {
            if is_comment(text) {
                continue;
            }
            if !text.contains("panic!(") {
                continue;
            }
            let in_test_region = test_ranges
                .iter()
                .any(|(s, e)| *lineno >= *s && *lineno <= *e);
            if !in_test_region {
                hits.push(format!("{file}:{lineno} — panic!() on hot dispatch path"));
            }
        }
    }

    assert!(
        hits.is_empty(),
        "33.y.l D7 parity gate FAILED: found {} panic!() on the hot \
         dispatch path (outside #[cfg(test)] regions). Locations:\n  - {}",
        hits.len(),
        hits.join("\n  - ")
    );
}

// ────────────────────────────────────────────────────────────────────
//  §3 — No `legacy_shim` / `ShimReason` / `LegacyShimHandled` symbols
// ────────────────────────────────────────────────────────────────────
//
// These three identifiers were the legacy shim infrastructure; 33.y.l
// retired them in lockstep. Any non-comment reference in
// flow_dispatcher/*.rs is a regression that re-introduces the shim.

#[test]
fn no_legacy_shim_symbol_references_in_dispatcher() {
    let lines = read_all_flow_dispatcher_lines();
    let hits = lines_matching(&lines, "legacy_shim");
    let formatted: Vec<String> = hits
        .iter()
        .map(|(f, n, t)| format!("{f}:{n} — {}", t.trim()))
        .collect();
    assert!(
        formatted.is_empty(),
        "33.y.l parity gate FAILED: found {} non-comment reference(s) \
         to `legacy_shim` in flow_dispatcher/*.rs. The shim was retired \
         in 33.y.l. Locations:\n  - {}",
        formatted.len(),
        formatted.join("\n  - ")
    );
}

#[test]
fn no_shim_reason_symbol_references_in_dispatcher() {
    let lines = read_all_flow_dispatcher_lines();
    let hits = lines_matching(&lines, "ShimReason");
    let formatted: Vec<String> = hits
        .iter()
        .map(|(f, n, t)| format!("{f}:{n} — {}", t.trim()))
        .collect();
    assert!(
        formatted.is_empty(),
        "33.y.l parity gate FAILED: found {} non-comment reference(s) \
         to `ShimReason` in flow_dispatcher/*.rs. Retired in 33.y.l. \
         Locations:\n  - {}",
        formatted.len(),
        formatted.join("\n  - ")
    );
}

#[test]
fn no_legacy_shim_handled_outcome_references_in_dispatcher() {
    let lines = read_all_flow_dispatcher_lines();
    let hits = lines_matching(&lines, "LegacyShimHandled");
    let formatted: Vec<String> = hits
        .iter()
        .map(|(f, n, t)| format!("{f}:{n} — {}", t.trim()))
        .collect();
    assert!(
        formatted.is_empty(),
        "33.y.l parity gate FAILED: found {} non-comment reference(s) \
         to `LegacyShimHandled` in flow_dispatcher/*.rs. The outcome \
         variant was retired in 33.y.l. Locations:\n  - {}",
        formatted.len(),
        formatted.join("\n  - ")
    );
}

#[test]
fn no_legacy_shim_failed_error_references_in_dispatcher() {
    let lines = read_all_flow_dispatcher_lines();
    let hits = lines_matching(&lines, "LegacyShimFailed");
    let formatted: Vec<String> = hits
        .iter()
        .map(|(f, n, t)| format!("{f}:{n} — {}", t.trim()))
        .collect();
    assert!(
        formatted.is_empty(),
        "33.y.l parity gate FAILED: found {} non-comment reference(s) \
         to `LegacyShimFailed` in flow_dispatcher/*.rs. The error \
         variant was retired in 33.y.l. Locations:\n  - {}",
        formatted.len(),
        formatted.join("\n  - ")
    );
}

// ────────────────────────────────────────────────────────────────────
//  §4 — All 10 expected dispatcher module files present
// ────────────────────────────────────────────────────────────────────
//
// A dropped module file (rebase accident, etc.) would silently shrink
// the dispatcher surface. This test pins the expected file set so the
// drift surfaces at PR time.

#[test]
fn dispatcher_module_files_pinned_to_expected_set() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let dispatcher_dir = PathBuf::from(manifest_dir)
        .join("src")
        .join("flow_dispatcher");
    let mut found: Vec<String> = fs::read_dir(&dispatcher_dir)
        .unwrap()
        .filter_map(|r| r.ok())
        .filter_map(|d| {
            d.path()
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .filter(|name| name.ends_with(".rs"))
        .collect();
    found.sort();

    let expected: Vec<&str> = vec![
        // §Fase 119.m.1 — the `agent` executor. `strategy:` is a CONTROL
        // policy (D119.5): three loops, three termination proofs. Composed
        // from `pure_shape` / `lambda_tools` / `cognitive` rather than
        // reimplemented beside them, which is why it belongs in this
        // directory and under this pin.
        "agent_loop.rs",
        "algebraic_handlers.rs",
        // §Fase 114.a — the budget gate, extracted to ONE place so every tool path
        // charges the same law (it was inlined in `pure_shape` and reachable only
        // by a streaming tool inside a daemon).
        "budget_gate.rs",
        "cognitive.rs",
        "effects_bridge.rs",
        "lambda_tools.rs",
        "mod.rs",
        "orchestration.rs",
        "parallel.rs",
        "pix.rs",
        "pure_shape.rs",
        // §Fase 34.g — added with the 4-disjunction convergence:
        // `unified_stream_handler` lives here as the single drain
        // loop both disjunct (b) (Tool::stream) + disjunct (d)
        // (bridge_effect_stream_yield_unified) route through.
        "unified_stream.rs",
        "wire_integrations.rs",
    ];

    assert_eq!(
        found,
        expected
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>(),
        "33.y.l parity gate: flow_dispatcher module file set drifted. \
         Expected 12 files (10 per sub-fase 33.y.c–j plus mod.rs, \
         plus unified_stream.rs added by Fase 34.g for the \
         4-disjunction convergence, plus budget_gate.rs added by \
         Fase 114.a); got {:?}. Adding a new sub-module requires \
         updating this expected list + adding the corresponding \
         `pub mod` in mod.rs.",
        found
    );
}
