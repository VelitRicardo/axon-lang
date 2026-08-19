//! v2.81.0 — **every published example compiles**. The ratchet that makes
//! the shop window a build input — `docs/cycle/the_public_artifact.md`,
//! axon-enterprise repo.
//!
//! **What was wrong.** `ci.yml` gated exactly ONE example —
//! `contract_analyzer.axon` — which happened to be among the six that passed.
//! The other eleven were never compiled by anything. Measured 2026-08-03
//! against the published 2.80.0 binary: **6 of 12 examples failed**, including
//! all three "Reference programs" the README features (healthcare / banking /
//! government, on v2.67.0's closed `resource` catalog) and two that did not even
//! PARSE. A green suite and a broken shop window, simultaneously.
//!
//! This test walks the directory. There is no list to forget to update: a new
//! example is covered the moment it lands, and a grammar change that breaks an
//! old one breaks the build instead of an adopter's first five minutes.
//!
//! It calls `axon_frontend::checker::run_check` — the SAME entry point the
//! `axon check` CLI subcommand dispatches to (`main.rs` → `checker::run_check`),
//! so this is not a re-implementation that can drift from the real command.

use std::path::{Path, PathBuf};

/// Every `.axon` file under `examples/`, recursively, sorted for a stable
/// failure order.
fn every_example() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("axon-frontend always has a parent directory")
        .join("examples");
    assert!(
        root.is_dir(),
        "examples/ must exist at {} — this gate is worthless if the path silently misses",
        root.display()
    );

    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("readable examples directory") {
            let path = entry.expect("readable directory entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "axon") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

#[test]
fn every_example_compiles() {
    let examples = every_example();

    // A directory walk that finds nothing would "pass" vacuously. Pin the
    // floor: this is the count at v2.81.0, and it may only grow.
    assert!(
        examples.len() >= 12,
        "expected at least the 12 examples present at v2.81.0, found {}: {examples:#?}",
        examples.len()
    );

    let mut failed: Vec<String> = Vec::new();
    for path in &examples {
        // `no_color: true` (deterministic output), `strict: false` (warnings
        // are not errors here — an example may legitimately warn, e.g. W003
        // for an unpinned backend), `schemas_dir: None`.
        let code = axon_frontend::checker::run_check(
            &path.to_string_lossy(),
            true,
            false,
            None,
        );
        if code != 0 {
            failed.push(format!("{} (exit {code})", path.display()));
        }
    }

    assert!(
        failed.is_empty(),
        "{} of {} published examples do not compile:\n  {}\n\n\
         Every file under examples/ is documentation an adopter is invited to \
         read and run. If a grammar change makes one obsolete, migrate it in the \
         same PR — or delete it. Leaving it broken ships a shop window that \
         refutes the pitch.",
        failed.len(),
        examples.len(),
        failed.join("\n  "),
    );
}
