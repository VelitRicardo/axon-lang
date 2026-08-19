//! v2.87.0 — **the ratchet on test targets that run NOTHING.**
//!
//! # What this exists to make impossible
//!
//! `default = ["cli"]`, and `axon_server` lives behind `server`. So a plain
//! `cargo test` compiles the file-level `#![cfg(feature = "server")]` suites
//! down to **zero tests each**, and cargo reports each one as
//!
//! ```text
//! test result: ok. 0 passed; 0 failed; 0 ignored
//! ```
//!
//! Green. Measured, not inferred: `cargo test` = **3934 passed**;
//! `cargo test --features server` = **5559 passed**. **1625 tests never ran**,
//! and nothing said so.
//!
//! # What that cost, before this file
//!
//! v2.81.0 (`bf59ae22`, *"the server is a feature"*) moved the server behind
//! the flag. From that commit forward, breaking a server test could not turn a
//! build red. The v2.87.0 audit turned the feature on and found FIVE breakages
//! that had accumulated in silence, from three different cycles:
//!
//! | broken by | what |
//! |---|---|
//! | v2.83.0 | `ToolEntry` gained `substrate`; a fixture stopped compiling |
//! | v2.83.0 | `axon-E042` landed; two compliance fixtures stopped type-checking |
//! | v2.83.0 | `IRFlowNode::AgentCall` left a match non-exhaustive |
//! | v2.87.0 | five new `IRFlowNode` variants, same match |
//! | v2.81.0 | `csys-native` became opt-in; two BPE assertions read as a broken chunker |
//!
//! Every one was proven pre-existing with `git stash --include-untracked`, per
//! v2.83.0's rule about claiming pre-existence rather than assuming it.
//!
//! # Why a ratchet and not "make `server` default"
//!
//! v2.81.0 measured what `server` costs — `axum` + `tower` + `tower-http` +
//! `http-body-util` — and made it opt-in as part of taking the install from 235
//! crates to 187 and the binary down 47%. Buying test coverage with install
//! weight for every adopter is the trade v2.81.0 explicitly refused.
//!
//! And why not just write it down: v2.83.0 wrote down that five
//! `*_pg_integration` suites needed a flag, and **nobody ran them for entire
//! cycles**. Its own conclusion: *a gate that depends on a flag the default
//! omits is honoured as often as someone reads the header.* So this is the
//! v2.83.0 remedy — REFUSE rather than skip — applied to features instead of a
//! DSN: the silently-empty set is PINNED, and adding to it is a deliberate act
//! that fails the build until someone acknowledges it.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// The pinned inventory lives in a DATA file, not a constant, because it is a
/// hundred-plus names and a constant that long stops being read.
///
/// Regenerate deliberately, never casually:
///
/// ```text
/// AXON_REGEN_DARK_TARGETS=1 cargo test --test no_test_target_runs_nothing
/// ```
///
/// The same shape `gate_1_5_rust_ir_regression_snapshot` uses for the IR golden.
/// Regenerating is a diff a reviewer sees; growing the file silently is what
/// this exists to prevent.
const PIN_FILE: &str = "dark_targets.pinned";

/// The file-level cfg gate a target uses, if any.
fn file_level_cfg_feature(src: &str) -> Option<String> {
    for line in src.lines().take(40) {
        let t = line.trim();
        if !t.starts_with("#![cfg(") {
            continue;
        }
        // `#![cfg(feature = "server")]` → `server`
        if let Some(rest) = t.split("feature").nth(1) {
            if let Some(open) = rest.find('"') {
                if let Some(close) = rest[open + 1..].find('"') {
                    return Some(rest[open + 1..open + 1 + close].to_string());
                }
            }
        }
    }
    None
}

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}

/// Which integration-test targets are switched off wholesale by a feature the
/// DEFAULT set does not enable.
fn dark_targets() -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    let entries = std::fs::read_dir(tests_dir()).expect("tests/ must be readable");
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let Ok(src) = std::fs::read_to_string(&path) else { continue };
        if let Some(feature) = file_level_cfg_feature(&src) {
            // `cli` IS in the default set, so a target gated on it still runs.
            if feature != "cli" {
                out.insert((stem.to_string(), feature));
            }
        }
    }
    out
}

/// **The ratchet.** The set of targets that run nothing by default is pinned.
#[test]
fn test_targets_that_run_nothing_by_default_are_pinned() {
    let dark = dark_targets();
    let rendered: String = dark
        .iter()
        .map(|(name, feature)| format!("{feature}\t{name}\n"))
        .collect();
    let pin_path = tests_dir().join(PIN_FILE);

    if std::env::var("AXON_REGEN_DARK_TARGETS").is_ok() {
        std::fs::write(&pin_path, &rendered).expect("write the pin file");
        eprintln!("regenerated {} ({} targets)", pin_path.display(), dark.len());
        return;
    }

    let pinned = std::fs::read_to_string(&pin_path).unwrap_or_else(|_| {
        panic!(
            "missing {}. Regenerate with:\n  \
             AXON_REGEN_DARK_TARGETS=1 cargo test --test no_test_target_runs_nothing",
            pin_path.display()
        )
    });

    let found: BTreeSet<&str> = rendered.lines().collect();
    let expected: BTreeSet<&str> = pinned.lines().filter(|l| !l.trim().is_empty()).collect();

    let newly_dark: Vec<&&str> = found.difference(&expected).collect();
    assert!(
        newly_dark.is_empty(),
        "these test targets now compile to ZERO tests under the default feature \
         set and are NOT pinned:\n{newly_dark:#?}\n\n\
         cargo reports each as `ok. 0 passed` — indistinguishable from a suite \
         that ran and succeeded. v2.81.0 created this blind spot and FIVE \
         breakages accumulated in it across three cycles before anyone looked.\n\n\
         If the gating is intended, regenerate the pin AND make sure something \
         runs the target (`cargo test --features <feature>`). Going dark is \
         allowed; going dark SILENTLY is not."
    );

    let relit: Vec<&&str> = expected.difference(&found).collect();
    assert!(
        relit.is_empty(),
        "these targets are pinned as dark but now RUN under the default \
         features:\n{relit:#?}\n\nGood news — regenerate the pin. The ratchet \
         may only shrink, and that only means anything if shrinking is recorded."
    );
}

/// The count, stated in ONE place so a reader learns the size of the blind spot
/// without counting lines. Measured 2026-08-10:
/// `cargo test` = 3934 passed · `cargo test --features server` = 5559 passed.
#[test]
fn the_size_of_the_blind_spot_is_stated_not_implied() {
    let dark = dark_targets();
    let by_feature: BTreeSet<&str> = dark.iter().map(|(_, f)| f.as_str()).collect();
    assert!(
        dark.len() >= 100,
        "the inventory shrank below the recorded floor ({} targets). That is \
         GOOD — update this floor and say what was re-lit.",
        dark.len()
    );
    assert!(
        by_feature.contains("server"),
        "`server` is the dominant gate and must appear: {by_feature:?}"
    );
    assert!(
        by_feature.contains("postgres"),
        "the `postgres` suites belong in this inventory too — v2.83.0 found \
         them skipping behind TWO doors, and this is the instrument that would \
         have shown it: {by_feature:?}"
    );
}

/// The scanner must prove it can still SEE a gate. If the cfg syntax it greps
/// for ever changes, `dark_targets()` silently returns an empty set and the
/// ratchet above passes forever while measuring nothing — the failure mode
/// v2.83.0's doc-link gate had to defend against in the same words.
#[test]
fn the_scanner_still_recognises_a_file_level_feature_gate() {
    assert_eq!(
        file_level_cfg_feature("#![cfg(feature = \"server\")]\nfn x() {}"),
        Some("server".to_string()),
        "the scanner no longer parses the cfg form it exists to find"
    );
    assert_eq!(
        file_level_cfg_feature("//! docs\nfn x() {}"),
        None,
        "an ungated file must not be reported as dark"
    );
    assert!(
        !dark_targets().is_empty(),
        "the scanner found NO gated targets at all. Either every suite is now \
         unconditional (delete this file) or the scan is broken (fix it) — but \
         a ratchet over an empty set is not a ratchet"
    );
}

/// The pinned names must correspond to real files. A stale entry would let a
/// deleted target keep the ratchet artificially wide.
#[test]
fn every_pinned_target_exists_on_disk() {
    let pin_path = tests_dir().join(PIN_FILE);
    let Ok(pinned) = std::fs::read_to_string(&pin_path) else {
        return; // the ratchet test above already reports a missing pin file
    };
    for line in pinned.lines().filter(|l| !l.trim().is_empty()) {
        let name = line.split('\t').nth(1).unwrap_or(line);
        let p = tests_dir().join(format!("{name}.rs"));
        assert!(
            p.exists(),
            "pinned dark target `{name}` has no file at {}. Regenerate the pin — \
             an entry for something that does not exist widens the ratchet for \
             free",
            p.display()
        );
    }
}
