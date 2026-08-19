//! v2.81.0 — the refusal contract, TESTED rather than assumed.
//!
//! This file is deliberately NOT gated. It runs under every profile, and half
//! its assertions are about the profile it is running under.
//!
//! The v2.81.0 thesis is v2.67.0 doctrine applied one level down: v2.67.0 proved every
//! advertised *primitive* is REAL; v2.81.0 proves the advertised *surface stays
//! advertised* when you install less of it. Concretely, three properties that a
//! feature split can silently break and a compile check will never catch:
//!
//!   1. **`--help` parity.** All 28 subcommands are listed under every profile.
//! A capability that vanishes from `--help` is the v2.67.0 defect in a new
//!      costume — the adopter cannot tell a missing feature from a missing
//!      capability, and has no string to search for.
//!   2. **The refusal is legible and actionable.** An absent feature produces a
//!      written message naming the exact reinstall command — never a linker
//!      error, never a panic, never "unknown subcommand".
//!   3. **The declaration cannot drift.** `Commands::Serve` must stay free of
//!      `#[cfg]`. Gating the enum variant would satisfy every compiler and
//!      quietly delete property (1), which is exactly the kind of regression
//! that took v2.67.0 a whole cycle to find the first time.

use std::process::Command;

/// The 25 TOP-LEVEL subcommands, as clap renders them in `axon --help`.
///
/// Kept as a literal list rather than derived from the source: a drift gate
/// whose expectation is computed from the thing it is gating proves nothing
/// (v2.67.0 F16 — a coverage law that was vacuous because it re-derived its own
/// subject).
const TOP_LEVEL: &[&str] = &[
    "check",
    "desugar",
    "compile",
    "run",
    "trace",
    "version",
    "repl",
    "inspect",
    "serve",
    "ld",
    "diff",
    "replay",
    "stats",
    "graph",
    "estimate",
    "deploy",
    "dossier",
    "sbom",
    "audit",
    "evidence-package",
    "store",
    "pcc",
    "parse",
    "fmt",
    "fix",
];

/// The 3 NESTED subcommands, as `(parent, child)`. `introspect` lives under
/// `axon store`, `prove` and `verify` under `axon pcc` — they are variants of
/// `StoreCommands` / `PccCommands`, not of `Commands`.
///
/// They are checked separately and deliberately: 25 + 3 is where the "28
/// subcommands" of the design decision comes from, and a gate that only looked at the top
/// level would silently stop covering three of them. Getting this wrong is how
/// a coverage claim becomes decoration — the first draft of this file asserted
/// all 28 against `axon --help` and failed, correctly, on the three that were
/// never there.
const NESTED: &[(&str, &str)] = &[("store", "introspect"), ("pcc", "prove"), ("pcc", "verify")];

fn help_of(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_axon"))
        .args(args)
        .arg("--help")
        .output()
        .unwrap_or_else(|e| panic!("run `axon {} --help`: {e}", args.join(" ")));
    assert!(
        out.status.success(),
        "`axon {} --help` must exit 0 under every profile; got {:?}",
        args.join(" "),
        out.status.code()
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn axon_help() -> String {
    help_of(&[])
}

// ── section 1 — `--help` parity under every profile ─────────────────────────────────

/// True when `help` lists `name` as a SUBCOMMAND, not merely as a substring
/// somewhere in the prose. clap renders each entry as an indented line whose
/// first token is the name — `  run   Compile and execute an .axon file.` — so
/// a plain `contains` would pass on the word "run" inside a description and turn
/// this gate into decoration.
fn lists_subcommand(help: &str, name: &str) -> bool {
    help.lines()
        .any(|l| l.trim_start().split_whitespace().next() == Some(name))
}

#[test]
fn s1_every_subcommand_stays_in_help() {
    let help = axon_help();
    let missing: Vec<&str> = TOP_LEVEL
        .iter()
        .copied()
        .filter(|c| !lists_subcommand(&help, c))
        .collect();
    assert!(
        missing.is_empty(),
        "v2.81.0 section 1 — these subcommands vanished from `axon --help`: {missing:?}.\n\
         Every one of the {} stays advertised under EVERY build profile. A build \
         that silently drops a subcommand cannot be distinguished by an adopter \
         from a build where the capability never existed.\n\n--help was:\n{help}",
        TOP_LEVEL.len()
    );
}

#[test]
fn s1_nested_subcommands_stay_advertised_too() {
    for (parent, child) in NESTED {
        let help = help_of(&[parent]);
        assert!(
            lists_subcommand(&help, child),
            "v2.81.0 section 1 — `axon {parent} {child}` vanished from `axon {parent} --help`. \
             The nested surface is advertised on exactly the same terms as the top \
             level.\n\n--help was:\n{help}"
        );
    }
}

#[test]
fn s1_the_advertised_surface_is_twenty_eight_commands() {
    // the design decision: "all 28 subcommands stay in `--help` under every profile". Pinning
    // the arithmetic keeps that sentence honest — if a command is added without
    // updating these lists, the claim silently becomes "27 of 28" and nothing
    // notices.
    assert_eq!(
        TOP_LEVEL.len() + NESTED.len(),
        28,
        "v2.81.0 section 1 — the advertised surface is 25 top-level + 3 nested = 28. \
         Update the design decision's wording together with these lists, or the refusal \
         contract starts describing a CLI that no longer exists."
    );
}

#[test]
fn s1_serve_is_advertised_even_without_the_feature() {
    // The single most important line of section 1, called out separately so a failure
    // names the actual regression instead of a list.
    let help = axon_help();
    assert!(
        lists_subcommand(&help, "serve"),
        "v2.81.0 section 1 — `axon serve` MUST stay in `--help` even when the `server` \
         feature is absent. It refuses in writing (see section 2); it does not \
         disappear.\n--help was:\n{help}"
    );
}

// ── section 2 — the refusal names the exact way out ─────────────────────────────────

#[cfg(not(feature = "server"))]
#[test]
fn s2_serve_refuses_in_writing_without_the_feature() {
    let out = Command::new(env!("CARGO_BIN_EXE_axon"))
        .arg("serve")
        .output()
        .expect("run `axon serve`");
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert_eq!(
        out.status.code(),
        Some(2),
        "v2.81.0 section 2 — an absent feature exits 2 (the code v2.81.0 established \
         for `axon evidence-package`), distinct from 1 so CI can tell \"wrong \
         build profile\" from \"the flow was rejected\". stderr was:\n{stderr}"
    );

    for needle in [
        // names the feature
        "`server` feature",
        // names the exact reinstall command
        "cargo install axon-lang --features server",
        // names the OTHER BINARY — under the design decision the server is `axon-server`,
        // so a message that only mentioned the feature would leave the adopter
        // running a command that will never be the answer
        "axon-server",
    ] {
        assert!(
            stderr.contains(needle),
            "v2.81.0 section 2 — the refusal must contain {needle:?}. \
             A refusal that does not name the way out is a failure with extra \
             steps. stderr was:\n{stderr}"
        );
    }

    assert!(
        !stderr.contains("panicked"),
        "v2.81.0 section 2 — the refusal is a message and an exit code, never a panic. \
         stderr was:\n{stderr}"
    );
}

#[cfg(feature = "server")]
#[test]
fn s2_serve_is_wired_when_the_feature_is_present() {
    // The dual of the refusal test: under `server`, `axon serve` reaches the
    // real runtime rather than the refusal. `--help` on the subcommand proves
    // the dispatch arm is bound without starting a listener.
    let out = Command::new(env!("CARGO_BIN_EXE_axon"))
        .args(["serve", "--help"])
        .output()
        .expect("run `axon serve --help`");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "`axon serve --help` must exit 0");
    for flag in ["--host", "--port", "--database-url", "--schemas-dir"] {
        assert!(
            stdout.contains(flag),
            "v2.81.0 section 2 — `axon serve` keeps its full flag surface under the \
             `server` feature; {flag} is missing.\n{stdout}"
        );
    }
}

// ── section 3 — the second binary (the design decision, option (c)) ──────────────────────────────

#[cfg(feature = "server")]
#[test]
fn s3_axon_server_binary_exists_and_mirrors_the_flag_surface() {
    let out = Command::new(env!("CARGO_BIN_EXE_axon-server"))
        .arg("--help")
        .output()
        .expect("run `axon-server --help`");
    assert!(out.status.success(), "`axon-server --help` must exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    // Same flags, same names as `axon serve` — it is the same `ServerConfig`
    // and the same `run_serve`. A second entry point that resolved its
    // configuration differently would be a second product.
    for flag in [
        "--host",
        "--port",
        "--channel",
        "--auth-token",
        "--log-level",
        "--log-format",
        "--log-file",
        "--database-url",
        "--strict-type-driven-transport",
        "--backend",
        "--schemas-dir",
    ] {
        assert!(
            stdout.contains(flag),
            "v2.81.0 section 3 — `axon-server` must carry `axon serve`'s full flag \
             surface; {flag} is missing.\n{stdout}"
        );
    }

    // It must point back at the compiler binary: an adopter who installed the
    // server and now wants `axon check` should not have to guess.
    assert!(
        stdout.contains("axon"),
        "v2.81.0 section 3 — `axon-server --help` must name the `axon` binary.\n{stdout}"
    );
}

// ── section 4 — the declaration cannot drift back behind a cfg ──────────────────────

#[test]
fn s4_serve_subcommand_declaration_is_not_feature_gated() {
    // Normalised to LF: this repository is developed on Windows and git may
    // check the file out with CRLF, which would make every multi-line assertion
    // below fail for a reason that has nothing to do with the contract.
    let src = std::fs::read_to_string("src/main.rs")
        .expect("read src/main.rs")
        .replace("\r\n", "\n");

    // The `Serve` variant of the `Commands` enum must not be preceded by a
    // `#[cfg(`. Gating it would compile perfectly and silently delete section 1.
    let idx = src
        .find("\n    Serve {")
        .expect("v2.81.0 section 4 — the `Serve` variant must exist in `Commands`");
    let preceding = &src[idx.saturating_sub(400)..idx];
    assert!(
        !preceding.contains("#[cfg("),
        "v2.81.0 section 4 — the `Serve` subcommand declaration MUST NOT be behind a \
         `#[cfg]`. What is gated is the DISPATCH (`run_serve_dispatch`), never \
         the declaration — otherwise `axon serve` disappears from `--help` in a \
         lean build and the refusal contract becomes unobservable.\n\
         Preceding source was:\n{preceding}"
    );

    // And the dispatcher must exist in both shapes, so the refusal is a real
    // code path rather than a comment describing one.
    assert!(
        src.contains("#[cfg(feature = \"server\")]\n#[allow(clippy::too_many_arguments)]\nfn run_serve_dispatch("),
        "v2.81.0 section 4 — the `server` build must dispatch to the real runtime."
    );
    assert!(
        src.contains("#[cfg(not(feature = \"server\"))]\n#[allow(clippy::too_many_arguments)]\nfn run_serve_dispatch("),
        "v2.81.0 section 4 — the lean build must have a REAL refusal function, not an \
         absent arm."
    );
}
