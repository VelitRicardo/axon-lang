//! §Fase 117.b — **the published metadata tells the truth** —
//! `docs/fase/fase_117_the_public_artifact.md`, axon-enterprise repo.
//!
//! Two claims an adopter acts on before a single line of their code runs: what
//! toolchain they need, and where the compiler sends them for help. Both were
//! wrong in 2.80.0.
//!
//! 1. **MSRV.** `README.md` said *"Requires Rust 1.75+"* while every manifest
//!    (and crates.io) declared `rust-version = "1.95"` — twenty minors of
//!    understatement. A reader on 1.75 follows the docs, gets an MSRV error, and
//!    reasonably concludes the project is broken.
//! 2. **Help URLs.** The compiler emits `https://axon-lang.io/docs/…` in
//!    diagnostics — including the FIRST error the README's own demo produced —
//!    and the domain does not resolve. A field literally named `remediation_url`
//!    that leads to no remediation.
//!
//! Both gates are **offline and deterministic**. Reachability is not re-tested
//! here: a network call in `cargo test` is flaky, slow, and fails an offline
//! build. Instead every project-owned or cited-standard host must be CLASSIFIED
//! in [`CLASSIFIED_HOSTS`], which is where the "did you check that it resolves?"
//! review actually happens. A new host that nobody classified breaks the build.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("axon-frontend always has a parent directory")
        .to_path_buf()
}

// ───────────────────────── MSRV ─────────────────────────

/// Every manifest in the workspace that declares an MSRV.
const MANIFESTS: &[&str] = &[
    "axon-rs/Cargo.toml",
    "axon-frontend/Cargo.toml",
    "axon-agora/Cargo.toml",
    "axon-csys/Cargo.toml",
];

fn declared_msrv(manifest: &str) -> String {
    let path = repo_root().join(manifest);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()));
    text.lines()
        .find_map(|l| {
            let l = l.trim();
            l.strip_prefix("rust-version")
                .map(|rest| rest.trim_start_matches([' ', '=']).trim().trim_matches('"').to_string())
        })
        .unwrap_or_else(|| panic!("{manifest} declares no `rust-version`"))
}

#[test]
fn fase117b_every_manifest_agrees_on_the_msrv() {
    let versions: BTreeSet<String> = MANIFESTS.iter().map(|m| declared_msrv(m)).collect();
    assert_eq!(
        versions.len(),
        1,
        "the workspace crates disagree on their MSRV: {versions:?}. They are published \
         together and installed together — a reader cannot satisfy two floors."
    );
}

#[test]
fn fase117b_readme_states_the_real_msrv() {
    let msrv = declared_msrv("axon-rs/Cargo.toml");
    let readme = std::fs::read_to_string(repo_root().join("README.md")).expect("README.md");

    assert!(
        readme.contains(&format!("**{msrv}+**")),
        "README.md must state the real MSRV (`**{msrv}+**`, as declared by the manifests). \
         Before §117.b it claimed 1.75+ against a real floor of 1.95 — a reader on an older \
         toolchain hit a wall the docs told them was not there."
    );

    // The specific historical lie, pinned so it cannot come back by copy-paste.
    assert!(
        !readme.contains("Rust](https://rustup.rs) 1.75+")
            && !readme.contains("Requires Rust 1.75"),
        "the 1.75+ MSRV claim is back in README.md"
    );
}

#[test]
fn fase117b_readme_documents_the_c_toolchain_prerequisite() {
    // `axon-csys` is a NON-OPTIONAL dependency with a build script that compiles
    // C23 via `cc`. `cargo install axon-lang` therefore needs a C compiler, and
    // 2.80.0's docs never said so once — the failure surfaces as an opaque
    // linker error with nothing in the docs to explain it.
    let readme = std::fs::read_to_string(repo_root().join("README.md")).expect("README.md");
    assert!(
        readme.contains("C compiler"),
        "README.md must document the C-toolchain prerequisite (axon-csys builds C23 via `cc`)"
    );
}

// ───────────────────────── source URLs ─────────────────────────

/// Reachability status of a host that appears in a source-code URL literal.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Host {
    /// Verified to answer. Third-party API endpoints and live project domains.
    Resolves,
    /// Verified NOT to answer. Shipping a URL under one of these is a decision,
    /// not an accident — the reason field says whose decision it is.
    DoesNotResolve(&'static str),
}
use Host::{DoesNotResolve, Resolves};

/// Hosts that are ours, or standards bodies we cite by URI. Third-party vendor
/// APIs (`api.anthropic.com`, `openrouter.ai`, …) and the `example.com`-family
/// fixtures used throughout the inline tests are deliberately out of scope: they
/// are exercised by the code that calls them, not by documentation links.
///
/// Probed 2026-08-03 with calibrated controls (github.com 200, example.com 200).
const CLASSIFIED_HOSTS: &[(&str, Host)] = &[
    // ── ours ──
    ("axon-lang.io", DoesNotResolve(
        "HTTP 000 / no DNS answer. Used for the in-toto/SLSA builder IDs in the audit-evidence \
         bundle AND for user-facing help links (`remediation_url`, the type checker's \
         wire-envelope hint — the first error the README demo produces). D117.3 is ratified as \
         written: §117.b lands this classifier, and the string rewrite waits on the founder \
         naming the canonical documentation host. NOTE: `axon.dev` (301) and \
         `bemarking.com.co` (308) both answer, if either is the intended target.",
    )),
    ("axon.dev", Resolves),
    ("auth.bemarking.com", Resolves),
    // ── standards cited by URI ──
    // These are identifiers in attestation predicates as much as they are pages;
    // they also happen to resolve.
    ("in-toto.io", Resolves),
    ("slsa.dev", Resolves),
];

/// Every `https://` host appearing in a Rust source literal across the published
/// crates, restricted to the project/standards scope described above.
fn project_hosts_in_source() -> BTreeSet<String> {
    let root = repo_root();
    let mut hosts = BTreeSet::new();
    for crate_dir in ["axon-frontend/src", "axon-rs/src", "axon-agora/src"] {
        let dir = root.join(crate_dir);
        assert!(dir.is_dir(), "{} must exist — this gate is worthless if the path silently misses", dir.display());
        let mut stack = vec![dir];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d).expect("readable source directory") {
                let path = entry.expect("readable entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).expect("readable source file");
                    for (_, after) in text.match_indices("https://").map(|(i, _)| (i, &text[i + 8..])) {
                        let host: String = after
                            .chars()
                            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
                            .collect();
                        // In scope: our own domains and the standards we cite.
                        let in_scope = host.contains("axon")
                            || host.contains("bemarking")
                            || host == "in-toto.io"
                            || host == "slsa.dev";
                        if in_scope && host.contains('.') {
                            hosts.insert(host);
                        }
                    }
                }
            }
        }
    }
    hosts
}

#[test]
fn fase117b_every_project_url_host_is_classified() {
    let found = project_hosts_in_source();
    assert!(
        !found.is_empty(),
        "the scanner found no project hosts at all — it has stopped working, and a gate that \
         cannot fail is not a gate"
    );

    let classified: BTreeSet<&str> = CLASSIFIED_HOSTS.iter().map(|(h, _)| *h).collect();
    let unclassified: Vec<&String> =
        found.iter().filter(|h| !classified.contains(h.as_str())).collect();

    assert!(
        unclassified.is_empty(),
        "these hosts appear in source URL literals and are not in CLASSIFIED_HOSTS: {unclassified:?}\n\n\
         Check whether each one actually answers, then add it with `Resolves` or \
         `DoesNotResolve(\"why + who owns the fix\")`. A URL the compiler prints to an adopter \
         is a promise; 2.80.0 shipped `remediation_url` pointing at a domain with no DNS record."
    );

    // The ledger may only shrink: a classified host that no longer appears in
    // the source is a stale row, and stale rows make the list lie.
    let stale: Vec<&str> = classified
        .iter()
        .copied()
        .filter(|h| !found.contains(*h))
        .collect();
    assert!(
        stale.is_empty(),
        "these CLASSIFIED_HOSTS rows no longer appear in any source URL — delete them: {stale:?}"
    );
}

#[test]
fn fase117b_dead_hosts_are_declared_not_discovered() {
    // The point of the classifier is that shipping a dead URL becomes a written
    // decision. This test asserts the one we know about is still written down —
    // if someone fixes the URLs, this test fails and they delete the row, which
    // is exactly the ratchet turning.
    let dead: Vec<&str> = CLASSIFIED_HOSTS
        .iter()
        .filter(|(_, s)| matches!(s, DoesNotResolve(_)))
        .map(|(h, _)| *h)
        .collect();
    assert_eq!(
        dead,
        vec!["axon-lang.io"],
        "the set of knowingly-dead hosts changed. Removing one means the URLs were fixed \
         (delete its row here too). Adding one means we are now shipping a NEW dead link to \
         adopters — say why in the row, or do not add it."
    );
}
