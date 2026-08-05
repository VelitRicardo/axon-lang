//! §Fase 118.b.3 — the §Fase 96 pooler-topology decision, driver-free.
//!
//! `connections_release_across_cognition` is a DEPLOYMENT question — what kind
//! of pooler sits in front of Postgres — answered by reading one environment
//! variable and comparing two strings. It lived in `store/postgres_backend.rs`,
//! so the eager-pin loops in `runner.rs` had to reach into the driver module to
//! ask "should I even try to pin?" — a question that must be answerable *before*
//! any driver is involved, and in a build that has none.
//!
//! Part of the same cluster as [`super::error`] and [`super::row`]: three
//! general concepts parked in the first module that needed them. See
//! `store/error.rs` for the full list.
//!
//! **This module must never acquire a dependency.**

/// §Fase 96.a — is eager §37.x.j connection pinning enabled for this
/// deployment? Read ONCE from `AXON_DB_POOLER_MODE` and cached (the pooler
/// topology is fixed for a process's life):
///   - `transaction` (default, or unset) → pinning ON (unchanged behavior;
///     a transaction-mode pooler needs one connection held per flow so
///     consecutive ops keep the same physical backend / prepared-statement
///     session).
///   - `session` | `direct` → pinning OFF. Each pool connection is already a
///     coherent session, so store ops acquire per-op and RELEASE the
///     connection between them — including across a flow's cognition (LLM)
///     steps, so a slow flow never holds a scarce connection idle under a
///     bounded pooler. Doctrine `connections_release_across_cognition`.
pub fn connection_pinning_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        pinning_enabled_for_mode(&std::env::var("AXON_DB_POOLER_MODE").unwrap_or_default())
    })
}

/// The pure decision (testable without the env/`OnceLock`): pinning is ON for
/// every mode EXCEPT `session`/`direct` (case/space-insensitive). An unset or
/// unrecognised value defaults to ON (`transaction`) — zero regression for
/// existing deployments.
fn pinning_enabled_for_mode(mode: &str) -> bool {
    !matches!(mode.trim().to_ascii_lowercase().as_str(), "session" | "direct")
}

#[cfg(test)]
mod tests {
    use super::*;

/// §Fase 96.a — the pooler-mode pin decision (`connections_release_across_cognition`).
#[test]
fn pinning_mode_gate() {
    // Default / transaction / unrecognised → pin ON (zero regression).
    assert!(pinning_enabled_for_mode(""));
    assert!(pinning_enabled_for_mode("transaction"));
    assert!(pinning_enabled_for_mode("TRANSACTION"));
    assert!(pinning_enabled_for_mode("pgbouncer-txn"));
    // Session / direct → pin OFF (release connections across cognition),
    // case- and space-insensitive.
    assert!(!pinning_enabled_for_mode("session"));
    assert!(!pinning_enabled_for_mode(" Session "));
    assert!(!pinning_enabled_for_mode("direct"));
    assert!(!pinning_enabled_for_mode("DIRECT"));
}
}
