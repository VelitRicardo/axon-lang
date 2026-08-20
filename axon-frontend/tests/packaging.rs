//! v2.81.0 — **the published package stays lean** —
//! `the design plan`, axon-enterprise repo.
//!
//! Nothing had ever gated the artefact. Not `cargo package`, not its contents,
//! not the dependency tree, not the install time. Measured on 2.80.0:
//! **19m41s, 305 crates, 44.02 MiB of downloads, a 46.2 MiB binary** — for a CLI
//! whose flagship command (`axon check`) needs **12** crates. The single
//! heaviest item was `aws-lc-sys` (9.29 MiB of C and assembly), pulled in only
//! because a default-on feature reached AWS Secrets Manager.
//!
//! These assertions are **offline and deterministic** — they read the manifest
//! and the lockfile rather than shelling out to cargo, so they run in every
//! `cargo test` on every platform. They pin the specific regressions v2.81.0
//! fixed; they are not a general size budget.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("axon-frontend always has a parent directory")
        .to_path_buf()
}

fn axon_rs_manifest() -> String {
    std::fs::read_to_string(repo_root().join("axon-rs/Cargo.toml"))
        .expect("axon-rs/Cargo.toml is readable")
}

/// Count `[[package]] name = "<name>"` entries in the lockfile — i.e. how many
/// DISTINCT VERSIONS of a crate the resolution admits.
fn locked_versions(name: &str) -> usize {
    std::fs::read_to_string(repo_root().join("axon-rs/Cargo.lock"))
        .expect("axon-rs/Cargo.lock is readable")
        .lines()
        .filter(|l| l.trim() == format!("name = \"{name}\""))
        .count()
}

/// The exact `default` feature set of `axon-lang`, pinned so it cannot drift
/// silently.
///
/// v2.81.0 — this gate used to assert `default = []`, which was the whole
/// truth when v2.81.0 wrote it: `aws-secrets` was the only feature, so "no AWS by
/// default" and "no features by default" were the same sentence. v2.81.0 made
/// `default` non-empty BY DESIGN — `cli`, `documents` and `server` are additive
/// features that together reproduce 2.81.0's behaviour exactly — and the literal
/// assertion went red.
///
/// **It went red in `6110c714` (v2.81.0) and was not noticed, because that
/// step verified `axon-rs` and this gate lives in `axon-frontend`.** That is
/// the v2.81.0 anti-drift machinery working exactly as designed and being missed by
/// a suite that was never run. The lesson is not about this assertion; it is that
/// a cross-crate gate is only a gate if the cross-crate suite runs.
///
/// So the check is rewritten to say what the design decision actually MEANT — `aws-secrets` is
/// not on by default — and to pin the whole set, so the next change to `default`
/// has to be deliberate and has to come here.
/// v2.81.0 — MOVED, deliberately, and this is where the decision is recorded.
///
/// the design decision (ratified 2026-08-04 as option (c)): a bare `cargo install axon-lang`
/// builds the COMPILER alone; the runtime is a second binary you install only if
/// you run one. Measured 2026-08-06 — install 11m09s → 7m05s (−36%), `axon`
/// 36.1 → 19.1 MiB (−47%), 235 → 187 crates.
///
/// This gate did its job: the manifest change turned it red, its own failure
/// message named v2.81.0 and told the next reader to update this constant in the
/// same commit, and that is exactly what happened. A packaging default that can
/// move without a human noticing is how v2.81.0's crates.io README stayed wrong for
/// six releases.
const EXPECTED_DEFAULT: &str = r#"default = ["cli"]"#;

#[test]
fn aws_sdk_is_opt_in() {
    let manifest = axon_rs_manifest();

    let default_line = manifest
        .lines()
        .find(|l| l.starts_with("default = "))
        .expect("axon-rs/Cargo.toml must declare a `default` feature set");

    // ── The the design decision invariant itself, stated directly ──
    assert!(
        !default_line.contains("aws-secrets"),
        "`aws-secrets` must NOT be in `default`. Putting it back costs every adopter \
         +57 crates and +15.90 MiB of a 44 MiB download (36%), drags in `aws-lc-sys` — 9.29 MiB \
         of C and assembly needing a C toolchain — and compiles a SECOND complete HTTP stack \
         (hyper 0.14 + http 0.2 + h2 0.3 + rustls 0.21) alongside the modern one, so the binary \
         links two TLS implementations with two CVE timelines. All to reach a service most \
         adopters never touch.\nFound: {default_line}"
    );

    // ── And the set is pinned, so a change is a decision ──
    assert_eq!(
        default_line, EXPECTED_DEFAULT,
        "the `default` feature set changed. That is allowed — v2.81.0 is expected to move it to \
         `[\"cli\"]` so a bare `cargo install axon-lang` builds the compiler alone — but it must \
         be DELIBERATE, and this constant is where the decision is recorded. Update \
         EXPECTED_DEFAULT in the same commit that changes the manifest, and say why in the \
         release note (v2.81.0's precedent: the SemVer consequence goes in the first line, not a \
         footnote)."
    );

    assert!(
        manifest.contains("aws-secrets = [\"dep:aws-config\", \"dep:aws-sdk-secretsmanager\"]"),
        "the `aws-secrets` feature must still EXIST — opt-in, not deleted. Removing it would \
         strand any consumer that genuinely resolves per-tenant keys from AWS Secrets Manager, \
         and the point of the design decision was to stop charging everyone for it, not to withdraw it."
    );
}

#[test]
fn exactly_one_websocket_implementation() {
    // Before v2.81.0 the manifest pinned `tokio-tungstenite = "0.24"` while
    // `axum 0.8` pulled 0.29, so the binary linked TWO complete RFC 6455
    // implementations — one driving `upstream` (client), one driving `socket`
    // (server). Worse, the manifest comment asserted they were the same
    // library. `cargo tree -i` disagreed: 0.24 had exactly one parent.
    for crate_name in ["tokio-tungstenite", "tungstenite"] {
        assert_eq!(
            locked_versions(crate_name),
            1,
            "`{crate_name}` resolves to more than one version. Two implementations of one \
             protocol in one process is the client/server asymmetry v2.67.0 already cost a cycle \
             to discover. Match the direct pin to whatever `axum` pulls."
        );
    }
}

#[test]
fn crate_readme_is_package_local_and_current() {
    let manifest = axon_rs_manifest();
    assert!(
        manifest.contains("\nreadme = \"README.md\""),
        "`readme` must point INSIDE the package. Pointing at `../README.md` does not work: cargo \
         silently prefers the package-root README and warns on every publish. That warning was \
         ignored across at least six releases, and crates.io served a v1.0.0 README for the \
         2.80.0 crate."
    );

    let readme = std::fs::read_to_string(repo_root().join("axon-rs/README.md"))
        .expect("axon-rs/README.md is readable");
    assert!(
        !readme.contains("282 HTTP routes"),
        "the v1.0.0 crate README is back — this is the text crates.io actually renders"
    );
    assert!(
        readme.contains("cargo install axon-lang"),
        "the crate landing page must show how to install the crate"
    );
}

#[test]
fn dockerfile_is_not_published() {
    assert!(
        axon_rs_manifest().contains("exclude = [\"Dockerfile\"]"),
        "the Dockerfile must stay out of the published crate — it is a monorepo build recipe \
         whose paths (`axon-rs/Dockerfile`, `../axon-frontend`) do not exist inside the extracted \
         package, so it is both dead weight and misleading."
    );
}

#[test]
fn migrations_are_still_published() {
    // The mirror of the test above: `migrations/` must NOT be excluded. It is a
    // COMPILE-TIME input (`sqlx::migrate!("./migrations")`), not documentation —
    // dropping it to save 21 KB would break the build for every consumer.
    // Scope the check to the actual `exclude`/`include` VALUES — the manifest
    // prose mentions `migrations` on purpose, explaining why it stays.
    for key in ["exclude", "include"] {
        if let Some(list) = axon_rs_manifest()
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("{key} = [")))
        {
            assert!(
                !list.contains("migrations"),
                "`migrations/` appears in `{key}` — `sqlx::migrate!(\"./migrations\")` reads it at \
                 COMPILE time, so dropping it to save 21 KB breaks the build for every consumer"
            );
        }
    }
    assert!(
        repo_root().join("axon-rs/migrations").is_dir(),
        "axon-rs/migrations must exist — `migrations.rs` embeds it via `sqlx::migrate!`"
    );
}
