//! §Fase 118.b.3 — the `postgres` refusal contract, TESTED rather than assumed.
//!
//! Deliberately NOT gated: it runs under every profile, and half its assertions
//! are about the profile it is running under.
//!
//! `server` (§118.b.2) removed a capability an adopter *runs*. `postgres` removes
//! one an adopter's PROGRAM DECLARES — a `backend: postgresql` axonstore — which
//! makes the contract sharper and easier to get wrong. The load-bearing claim is
//! a POSITIVE one:
//!
//! > **`axon check` type-checks a Postgres-backed program in a build with no
//! > PostgreSQL driver.**
//!
//! That is not a nicety. §118's whole product thesis (D118.1) is `axon check`
//! running on every PR — "compliance verification that installs in seconds and
//! runs anywhere". If dropping the driver silently made half the language
//! un-checkable, the lean build would be a different, smaller language wearing
//! the same name, and the split would be a lie. The compiler never needed a
//! driver; this file is what stops that from quietly ceasing to be true.

use std::process::Command;

/// A complete program declaring a Postgres-backed `axonstore`, via a `resource`
/// (the §113 shape — `capacity:` governs the pool).
const POSTGRES_PROGRAM: &str = r#"
resource  Db    { kind: postgres  endpoint: gate.db  lifetime: affine  capacity: 27 }
axonstore Users { backend: postgresql  resource: Db }
"#;

fn write_fixture(name: &str, body: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("axon_fase118_b3");
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    let p = dir.join(name);
    std::fs::write(&p, body).expect("write fixture");
    p
}

// ── §1 — the compiler needs no driver, in EVERY profile ──────────────────────

#[test]
fn fase118_b3_s1_axon_check_type_checks_a_postgres_program_without_a_driver() {
    let file = write_fixture("declares_postgres.axon", POSTGRES_PROGRAM);
    let out = Command::new(env!("CARGO_BIN_EXE_axon"))
        .arg("check")
        .arg(&file)
        .output()
        .expect("run `axon check`");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert!(
        out.status.success(),
        "§118.b.3 §1 — `axon check` MUST type-check a program declaring \
         `backend: postgresql` even when the `postgres` feature is absent. The \
         compiler resolves a DECLARATION; it opens no connection and needs no \
         driver.\n\n\
         If this fails in a lean build, the feature split has quietly made the \
         lean binary a SMALLER LANGUAGE rather than a smaller install — which \
         would break D118.1's entire premise (`axon check` on every PR, \
         installable in seconds, runnable anywhere).\n\n\
         exit={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code()
    );
}

// ── §2 — the refusal, when the driver is absent ──────────────────────────────

#[cfg(not(feature = "postgres"))]
#[test]
fn fase118_b3_s2_store_introspect_refuses_in_writing() {
    let out = Command::new(env!("CARGO_BIN_EXE_axon"))
        .args(["store", "introspect", "users", "--connection", "postgres://nope/db"])
        .output()
        .expect("run `axon store introspect`");
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert_eq!(
        out.status.code(),
        Some(2),
        "§118.b.3 §2 — an absent feature exits 2, the code §118.b.1 established \
         for `axon evidence-package` and §118.b.2 reused for `axon serve`. \
         Distinct from 1 so CI can tell \"wrong build profile\" from \"the \
         operation failed\".\nstderr:\n{stderr}"
    );
    for needle in [
        "`postgres` feature",
        "cargo install axon-lang --features postgres",
        // the refusal must say what STILL works, or it reads as a broken install
        "axon check",
    ] {
        assert!(
            stderr.contains(needle),
            "§118.b.3 §2 — the refusal must contain {needle:?}. A refusal that \
             does not name the way out is a failure with extra steps.\n\
             stderr:\n{stderr}"
        );
    }
    assert!(
        !stderr.contains("panicked"),
        "§118.b.3 §2 — a message and an exit code, never a panic.\nstderr:\n{stderr}"
    );
}

/// The store layer refuses at RESOLVE time — the earliest point at which "this
/// store cannot be opened" is knowable — rather than somewhere deeper and less
/// legible.
#[cfg(not(feature = "postgres"))]
#[test]
fn fase118_b3_s2_resolve_refuses_at_the_earliest_honest_point() {
    let err = axon::store::postgres_backend::resolve_dsn("postgres://localhost/db")
        .expect_err("a build without the driver cannot resolve a Postgres DSN");
    let msg = format!("{err}");
    assert!(
        msg.contains("`postgres` feature")
            && msg.contains("cargo install axon-lang --features postgres"),
        "§118.b.3 §2 — `resolve_dsn`'s refusal must name the feature and the \
         reinstall command; it is what an adopter sees when a flow reaches a \
         Postgres store in a lean build.\ngot: {msg}"
    );
}

// ── §3 — the dual: under `postgres`, the surface is wired ────────────────────

#[cfg(feature = "postgres")]
#[test]
fn fase118_b3_s3_store_introspect_is_wired_when_the_feature_is_present() {
    let out = Command::new(env!("CARGO_BIN_EXE_axon"))
        .args(["store", "introspect", "--help"])
        .output()
        .expect("run `axon store introspect --help`");
    assert!(out.status.success(), "`axon store introspect --help` must exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        stdout.contains("--connection"),
        "§118.b.3 §3 — the introspect flag surface must survive the gate.\n{stdout}"
    );
}

// ── §4 — the gate cannot drift back into the declaration ─────────────────────

#[test]
fn fase118_b3_s4_the_store_subcommand_declaration_is_not_feature_gated() {
    // Normalised to LF: this repository is developed on Windows and git may check
    // the file out with CRLF.
    let src = std::fs::read_to_string("src/main.rs")
        .expect("read src/main.rs")
        .replace("\r\n", "\n");

    let idx = src
        .find("\n    Store {")
        .expect("§118.b.3 §4 — the `Store` variant must exist in `Commands`");
    let preceding = &src[idx.saturating_sub(400)..idx];
    assert!(
        !preceding.contains("#[cfg("),
        "§118.b.3 §4 — the `Store` subcommand declaration MUST NOT be behind a \
         `#[cfg]`. What is gated is the DISPATCH, never the declaration — \
         otherwise `axon store introspect` disappears from `--help` in a lean \
         build and the refusal becomes unobservable.\nPreceding:\n{preceding}"
    );

    // And the refusal must be a real code path, not a comment describing one.
    assert!(
        src.contains("`axon store introspect` requires the `postgres` feature"),
        "§118.b.3 §4 — the lean build must carry the written refusal text."
    );
}

// ── §5 — the port's promise: no `#[cfg]` in the cognition path ───────────────

/// D118.2 chose a newtype over `#[cfg]`-on-signatures specifically so the
/// executor would keep ONE shape across profiles. §118.b.3 is the first build
/// that could break that promise, so it is asserted rather than trusted.
#[test]
fn fase118_b3_s5_the_executor_signature_is_profile_independent() {
    let runner = std::fs::read_to_string("src/runner.rs")
        .expect("read src/runner.rs")
        .replace("\r\n", "\n");
    assert!(
        runner.contains(
            "pinned_conns: &mut std::collections::HashMap<String, crate::pinned_conn::PinnedConn>"
        ),
        "§118.b.3 §5 — the executor must carry the PORT, not a driver type and \
         not two `#[cfg]`-selected shapes. `PinnedConn` is a real struct under \
         `postgres` and an uninhabited enum without it, so this one signature is \
         correct in both profiles — which is the entire reason D118.2 built the \
         port before the gate existed."
    );
    assert!(
        !runner.contains("sqlx::pool::PoolConnection"),
        "§118.b.3 §5 — `runner.rs` must never name the driver's connection type \
         again. That is D118.2's exit criterion and this is its ratchet."
    );
}
