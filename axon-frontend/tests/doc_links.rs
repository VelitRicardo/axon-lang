//! v2.83.0 — **the published documentation links somewhere, here too.**
//!
//! v2.83.0 landed this ratchet on `axon-lang` and measured the other two crates
//! without gating them: 16 broken intra-doc links in `axon-frontend`, 6 in
//! `axon-csys`. That left the CLASS open — one instance was protected and the
//! other two were not, which is the state where a fix in one crate coexists
//! with a regression in the next and the build stays green.
//!
//! `rustdoc::broken_intra_doc_links` is on by default and `cargo doc` exits 0
//! on warnings, so an ungated crate reports its dead links on every build and
//! nothing reads them. That is the same shape three times over in this project
//! (v2.67.0's dead cable, v2.81.0's gate that stayed red, v2.81.0's port gate that
//! measured itself) — and here the gate is not even ours, it ships with the
//! toolchain.
//!
//! # Why the baseline is 17 and not v2.83.0's 16
//!
//! Measured, not copied. `cargo doc --no-deps` reports **17** today. The
//! difference is NOT this cycle's doing, and that was checked rather than
//! assumed: of the links reported in files v2.83.0 touched, all are pre-existing
//! mathematical notation on lines this cycle did not edit — `[0,1]` as an
//! interval, `derivatives[i]` as an index — which rustdoc reads as link syntax.
//!
//! v2.83.0 did introduce exactly one, and the measurement caught it before the
//! ratchet went in: `[`algebraic_effects_runtime.md`]` in `ast.rs`,
//! linking a *file path* as if it were a Rust symbol. Fixed at the source. The
//! gate earning its keep against the commit that installs it is the only real
//! test of a gate.
//!
//! # Why a ratchet and not `#![deny]`
//!
//! Same reason as v2.83.0: `#![deny]` turns 17 warnings into 17 build errors, so
//! the only ways to land it are to fix every one in a single commit or to
//! switch the lint back off — and a lint that gets disabled is worse than one
//! never enabled, because now a line of code asserts we thought about it.
//!
//! A ratchet enforces at the CURRENT number immediately: new broken links fail
//! from this commit onward, and the baseline may only move DOWN.

use std::process::Command;

/// Broken intra-doc links in `axon-frontend`, measured 2026-08-09.
///
/// **This number may only go DOWN.** Raising it means someone shipped a doc
/// link to a path that does not resolve and then edited the gate instead of the
/// link. These render as dead references on docs.rs, which is where an adopter
/// looks before they look at any source.
const BASELINE_BROKEN_LINKS: usize = 17;

fn rustdoc_stderr() -> String {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let out = Command::new(cargo)
        .args(["doc", "--no-deps", "--color", "never"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        // Documenting is a build; without this the nested invocation can pick
        // a different profile than the suite was built under and rebuild the
        // whole graph.
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        // v2.89.0 — do not inherit the parent's build flags. The identical
        // gate in `axon-csys` failed in the `sanitizer (thread)` lane because
        // this nested `cargo doc` picked up `RUSTFLAGS=-Zsanitizer=thread`
        // without the `-Zbuild-std` that makes it valid. This crate has no
        // sanitizer lane today, so the leak is latent here — fixed as a CLASS
        // rather than only in the copy that went red (v2.87.0's lesson).
        .env_remove("RUSTFLAGS")
        .env_remove("CFLAGS")
        .env_remove("LDFLAGS")
        .output()
        .expect("`cargo doc` must be runnable — this gate is worthless if it silently skips");

    assert!(
        out.status.success(),
        "`cargo doc --no-deps` failed outright. That is not this gate's finding to report, but \
         it does mean the count below is meaningless, so the gate refuses to pass:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn frontend_broken_intra_doc_links_only_ever_shrink() {
    let stderr = rustdoc_stderr();

    // A gate that cannot fail is not a gate. If rustdoc stops emitting the
    // string this greps for — an upstream wording change, a flag that silences
    // it — the count silently becomes 0 and the ratchet locks in a lie. So the
    // scanner must prove it is still reading rustdoc output.
    assert!(
        stderr.contains("Documenting axon-frontend") || stderr.contains("Generated"),
        "the nested `cargo doc` produced no recognisable rustdoc output, so the count below \
         measured nothing:\n{stderr}"
    );

    let broken = stderr
        .lines()
        .filter(|l| l.trim_start().starts_with("warning: unresolved link to"))
        .count();

    assert!(
        broken <= BASELINE_BROKEN_LINKS,
        "broken intra-doc links went UP in axon-frontend: {broken} > {BASELINE_BROKEN_LINKS}.\n\n\
         A doc comment now links to a path rustdoc cannot resolve. Fix the link — do not raise \
         the baseline.\n\n\
         Find the new ones with:\n  \
         cargo doc --no-deps --manifest-path axon-frontend/Cargo.toml 2>&1 | grep -A2 'unresolved link'"
    );

    // The other direction: when the count drops, the baseline follows it down
    // in the same commit. A stale-high baseline is slack that silently
    // re-admits every link somebody just fixed.
    assert_eq!(
        broken, BASELINE_BROKEN_LINKS,
        "broken intra-doc links dropped to {broken} — good. Now lower BASELINE_BROKEN_LINKS to \
         {broken} in this file so the ratchet holds the ground you just took. Leaving it at \
         {BASELINE_BROKEN_LINKS} would let {} links come back unnoticed.",
        BASELINE_BROKEN_LINKS - broken
    );
}
