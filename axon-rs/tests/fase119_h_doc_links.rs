//! §Fase 119.h — **the published documentation links somewhere**.
//!
//! `rustdoc::broken_intra_doc_links` is a lint that is ON BY DEFAULT. It was
//! firing 187 times in this crate, 16 in `axon-frontend` and 6 in `axon-csys`
//! — 209 dead links in the documentation that ships to docs.rs, which is the
//! surface an adopter reads before they read any source. Every one of them
//! had been reported on every `cargo doc` for months.
//!
//! Nothing failed, because `cargo doc` exits 0 on warnings and no suite ran
//! it. That is the third instance in this project of the same shape — a gate
//! that exists, is red, and is watched by nobody (§111.i's dead cable, §117's
//! gate that stayed red for a commit, §118's port gate that measured itself).
//! Here the gate was not even ours: it shipped with the toolchain.
//!
//! # Why a ratchet and not `#![deny]`
//!
//! `#![deny(rustdoc::broken_intra_doc_links)]` is the right end state and the
//! wrong next step: it turns 186 warnings into 186 build errors, so the only
//! way to land it today is to fix all of them in one commit or to disable it
//! again — and a lint that gets disabled is worse than one that was never
//! enabled, because now there is a line of code asserting we thought about it.
//!
//! A ratchet lands the enforcement immediately at the CURRENT number. New
//! broken links fail the build from this commit onward, and the baseline may
//! only be revised DOWNWARD. It is the same mechanism §117 used for
//! `KNOWN_DEBT`, which §119 drove to zero — the shape is known to work here.
//!
//! # What this costs
//!
//! One `cargo doc --no-deps` per suite run: 33 s warm, measured, in the
//! crate's own target directory so the dependency graph is already built.
//! Stated because a slow test that nobody can explain is a test that
//! eventually gets `#[ignore]`d, and an ignored gate is the failure mode
//! this file exists to end.

use std::process::Command;

/// Broken intra-doc links in `axon-lang`, measured 2026-08-08 at the commit
/// that introduced this gate.
///
/// **This number may only go DOWN.** Lowering it is the whole point; raising
/// it means someone shipped a doc link to a symbol that does not resolve and
/// then edited the gate rather than the link.
///
/// The remaining 186 are NOT all phantom symbols. `cargo doc` reports three
/// distinct faults under one warning:
///
///   * a symbol that no longer exists (`run_streaming_legacy_path`, deleted
///     in 33.z.e — §119.h corrected eleven doc sites that named it and its
///     sibling as if they were live code);
///   * a symbol that exists but is private, so rustdoc will not link it;
///   * a path that is merely wrong (`state::SessionRuntime` where the item
///     needs `crate::session_runtime::state::SessionRuntime`).
///
/// Only the first class is a lie. The other two are still dead links on
/// docs.rs, which is why they are all counted: the adopter clicking them
/// cannot tell the difference either.
const BASELINE_BROKEN_LINKS: usize = 186;

fn rustdoc_warnings() -> String {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let out = Command::new(cargo)
        .args(["doc", "--no-deps", "--color", "never"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        // Documenting is a build; without this the nested invocation can pick
        // up a different profile than the one the suite was built under and
        // rebuild the whole graph.
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .output()
        .expect("`cargo doc` must be runnable — this gate is worthless if it silently skips");

    assert!(
        out.status.success(),
        "`cargo doc --no-deps` failed outright. That is not this gate's finding to report, but \
         it does mean the link count below is meaningless, so the gate refuses to pass:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn fase119h_broken_intra_doc_links_only_ever_shrink() {
    let stderr = rustdoc_warnings();

    // A gate that cannot fail is not a gate. If rustdoc stops emitting the
    // string we grep for — a wording change upstream, a flag that silences
    // it — the count silently becomes 0 and the ratchet locks in a lie. So
    // the scanner is required to prove it is still reading rustdoc output.
    assert!(
        stderr.contains("Documenting axon-lang") || stderr.contains("Generated"),
        "the nested `cargo doc` produced no recognisable rustdoc output, so the count below \
         measured nothing:\n{stderr}"
    );

    let broken = stderr
        .lines()
        .filter(|l| l.trim_start().starts_with("warning: unresolved link to"))
        .count();

    assert!(
        broken <= BASELINE_BROKEN_LINKS,
        "broken intra-doc links went UP: {broken} > {BASELINE_BROKEN_LINKS}.\n\n\
         A doc comment now links to a path rustdoc cannot resolve. Fix the link — do not raise \
         the baseline. These render as dead references on docs.rs, which is where an adopter \
         looks first.\n\n\
         Find the new ones with: cargo doc --no-deps 2>&1 | grep -A2 'unresolved link'"
    );

    // The other direction: when the count drops, the baseline must follow it
    // down in the same commit. A stale-high baseline is slack that silently
    // re-admits every link somebody just fixed.
    assert_eq!(
        broken, BASELINE_BROKEN_LINKS,
        "broken intra-doc links dropped to {broken} — good. Now lower BASELINE_BROKEN_LINKS to \
         {broken} in this file so the ratchet holds the ground you just took. Leaving it at \
         {BASELINE_BROKEN_LINKS} would let {} links come back unnoticed.",
        BASELINE_BROKEN_LINKS - broken
    );
}
