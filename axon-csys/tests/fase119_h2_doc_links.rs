//! §Fase 119.h.2 — **the published documentation links somewhere, here too.**
//!
//! The third and last crate. §119.h ratcheted `axon-lang` and measured this one
//! at 6 broken intra-doc links without gating it; §119.h.2 closes the CLASS
//! rather than one more instance.
//!
//! `rustdoc::broken_intra_doc_links` is on by default and `cargo doc` exits 0
//! on warnings, so an ungated crate reports its dead links on every build and
//! nobody reads them.
//!
//! # This crate is mostly C
//!
//! `axon-csys` is C23 behind a thin Rust surface, so its doc links are few —
//! and that is exactly why it is worth gating rather than skipping. A crate
//! with six broken links and no gate is one careless `[`symbol`]` away from
//! twelve, and it is the crate an adopter reaches for when they want to know
//! what the FIPS-routable kernels actually expose.
//!
//! The gate runs `cargo doc --no-deps` in this crate's own manifest directory,
//! so the C build has already happened and the nested invocation is a
//! documentation pass, not a rebuild of the C sources.

use std::process::Command;

/// Broken intra-doc links in `axon-csys`, measured 2026-08-09 — unchanged from
/// §119.h's measurement of the same crate on 2026-08-08.
///
/// **This number may only go DOWN.**
const BASELINE_BROKEN_LINKS: usize = 6;

fn rustdoc_stderr() -> String {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let out = Command::new(cargo)
        .args(["doc", "--no-deps", "--color", "never"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
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
fn fase119h2_csys_broken_intra_doc_links_only_ever_shrink() {
    let stderr = rustdoc_stderr();

    // Prove the scanner is still reading rustdoc output — otherwise a wording
    // change upstream silently makes the count 0 and the ratchet locks a lie.
    assert!(
        stderr.contains("Documenting axon-csys") || stderr.contains("Generated"),
        "the nested `cargo doc` produced no recognisable rustdoc output, so the count below \
         measured nothing:\n{stderr}"
    );

    let broken = stderr
        .lines()
        .filter(|l| l.trim_start().starts_with("warning: unresolved link to"))
        .count();

    assert!(
        broken <= BASELINE_BROKEN_LINKS,
        "broken intra-doc links went UP in axon-csys: {broken} > {BASELINE_BROKEN_LINKS}.\n\n\
         Fix the link — do not raise the baseline.\n\n\
         Find the new ones with:\n  \
         cargo doc --no-deps --manifest-path axon-csys/Cargo.toml 2>&1 | grep -A2 'unresolved link'"
    );

    assert_eq!(
        broken, BASELINE_BROKEN_LINKS,
        "broken intra-doc links dropped to {broken} — good. Now lower BASELINE_BROKEN_LINKS to \
         {broken} in this file so the ratchet holds the ground you just took."
    );
}
