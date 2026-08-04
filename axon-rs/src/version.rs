//! §Fase 118.a — the version string, and nothing else.
//!
//! **Why this module exists at all.** `AXON_VERSION` used to live in
//! `runner.rs`. That was itself a fix: the constant had been redeclared as a
//! string literal in five files, each carrying a different stale value, and
//! centralising it killed the drift. Right fix, wrong house.
//!
//! `runner.rs` is the flow executor. It reaches `sqlx`, `reqwest`, `tokio`,
//! `axum` and `axon-csys` — the whole runtime. So `use crate::runner::AXON_VERSION`
//! made the executor a dependency of every caller that wanted a string literal.
//! Measured on 2.81.0, that single import put **~132 modules** into the reachable
//! set of `axon compile`, `axon dossier`, `axon sbom`, `axon audit`,
//! `axon evidence-package`, `axon prove`, `axon verify` and `axon repl` — none of
//! which execute a flow, open a socket, or touch a database.
//!
//! `axon-csys` in particular enters the compiler-side closure through exactly one
//! module (`wire_envelope`, reached via `runner`), and `axon-csys` compiles C23 via
//! `cc`. So a constant living in the wrong file is part of why
//! `cargo install axon-lang` needs a **C toolchain** to build a type checker.
//!
//! This module has no dependencies and must acquire none. Anything that needs the
//! version string imports it from here; `runner` re-exports it so existing call
//! sites (including `axon::runner::AXON_VERSION` in the §39.f CLI parity test)
//! keep resolving unchanged.

/// Single source of truth for the AXON version string.
///
/// Resolved at compile time from `[package].version` in `Cargo.toml`, so a single
/// bump there propagates to every caller.
pub const AXON_VERSION: &str = env!("CARGO_PKG_VERSION");
