//! §Fase 117.c — **the published package stays lean** —
//! `docs/fase/fase_117_the_public_artifact.md`, axon-enterprise repo.
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
//! `cargo test` on every platform. They pin the specific regressions §117.c
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

#[test]
fn fase117c_aws_sdk_is_opt_in() {
    let manifest = axon_rs_manifest();
    assert!(
        manifest.contains("\ndefault = []"),
        "`default` must be EMPTY (D117.1). Putting `aws-secrets` back in the default profile \
         costs every adopter +57 crates and +15.90 MiB of a 44 MiB download (36%), drags in \
         `aws-lc-sys` — 9.29 MiB of C and assembly needing a C toolchain — and compiles a SECOND \
         complete HTTP stack (hyper 0.14 + http 0.2 + h2 0.3 + rustls 0.21) alongside the modern \
         one, so the binary links two TLS implementations with two CVE timelines. All to reach a \
         service most adopters never touch."
    );
    assert!(
        manifest.contains("aws-secrets = [\"dep:aws-config\", \"dep:aws-sdk-secretsmanager\"]"),
        "the `aws-secrets` feature must still EXIST — opt-in, not deleted. Removing it would \
         strand any consumer that genuinely resolves per-tenant keys from AWS Secrets Manager, \
         and the point of D117.1 was to stop charging everyone for it, not to withdraw it."
    );
}

#[test]
fn fase117c_exactly_one_websocket_implementation() {
    // Before §117.c the manifest pinned `tokio-tungstenite = "0.24"` while
    // `axum 0.8` pulled 0.29, so the binary linked TWO complete RFC 6455
    // implementations — one driving `upstream` (client), one driving `socket`
    // (server). Worse, the manifest comment asserted they were the same
    // library. `cargo tree -i` disagreed: 0.24 had exactly one parent.
    for crate_name in ["tokio-tungstenite", "tungstenite"] {
        assert_eq!(
            locked_versions(crate_name),
            1,
            "`{crate_name}` resolves to more than one version. Two implementations of one \
             protocol in one process is the client/server asymmetry §111.i already cost a fase \
             to discover. Match the direct pin to whatever `axum` pulls."
        );
    }
}

#[test]
fn fase117c_crate_readme_is_package_local_and_current() {
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
fn fase117c_dockerfile_is_not_published() {
    assert!(
        axon_rs_manifest().contains("exclude = [\"Dockerfile\"]"),
        "the Dockerfile must stay out of the published crate — it is a monorepo build recipe \
         whose paths (`axon-rs/Dockerfile`, `../axon-frontend`) do not exist inside the extracted \
         package, so it is both dead weight and misleading."
    );
}

#[test]
fn fase117c_migrations_are_still_published() {
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
