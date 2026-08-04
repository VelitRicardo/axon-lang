//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 31.f — CLI flag + env var opt-in surfaces for
//! `strict_type_driven_transport` (D6 + D7).
//!
//! Tests the cross-stack contract: the env var
//! `AXON_STRICT_TYPE_DRIVEN_TRANSPORT` is the canonical signal
//! shared by Python `axon serve` and Rust `axon-rs`. Truthy
//! parsing is byte-identical per D7.
//!
//! The CLI flag `--strict-type-driven-transport` is parsed by
//! clap in the Rust binary; that path is exercised via E2E in
//! 31.g CI matrix lanes. This pack focuses on the env-var parser
//! and ServerConfig construction.

use axon::axon_server::{parse_truthy_env, ServerConfig};

// ─── §1 — Truthy env var parser, accept matrix ───────────────────────

#[test]
fn parse_truthy_accepts_canonical_truthy_values() {
    // Sequential because env vars are process-global. We use a
    // unique var name per test to avoid cross-test leakage.
    let cases: &[(&str, bool)] = &[
        ("1", true),
        ("true", true),
        ("TRUE", true),
        ("True", true),
        ("yes", true),
        ("YES", true),
        ("Yes", true),
        ("on", true),
        ("ON", true),
        ("On", true),
    ];
    let var_name = "AXON_TEST_FASE31_F_TRUTHY";
    for (val, expected) in cases {
        std::env::set_var(var_name, val);
        let got = parse_truthy_env(var_name);
        std::env::remove_var(var_name);
        assert_eq!(
            got, *expected,
            "value '{val}' expected truthy={expected}, got {got}"
        );
    }
}

#[test]
fn parse_truthy_rejects_falsy_values() {
    let cases: &[(&str, bool)] = &[
        ("0", false),
        ("false", false),
        ("FALSE", false),
        ("no", false),
        ("off", false),
        ("disabled", false),
        ("anything", false),
        ("", false),
        ("2", false),
        ("truee", false), // typo
        ("01", false),    // leading zero — strict
    ];
    let var_name = "AXON_TEST_FASE31_F_FALSY";
    for (val, expected) in cases {
        std::env::set_var(var_name, val);
        let got = parse_truthy_env(var_name);
        std::env::remove_var(var_name);
        assert_eq!(
            got, *expected,
            "value '{val}' expected truthy={expected}, got {got}"
        );
    }
}

#[test]
fn parse_truthy_returns_false_when_var_unset() {
    let var_name = "AXON_TEST_FASE31_F_UNSET_XYZQ";
    std::env::remove_var(var_name);
    assert!(!parse_truthy_env(var_name));
}

#[test]
fn parse_truthy_trims_whitespace() {
    let var_name = "AXON_TEST_FASE31_F_TRIM";
    std::env::set_var(var_name, "  true  ");
    let got = parse_truthy_env(var_name);
    std::env::remove_var(var_name);
    assert!(got, "whitespace-padded 'true' should still be truthy");

    std::env::set_var(var_name, "\t1\n");
    let got = parse_truthy_env(var_name);
    std::env::remove_var(var_name);
    assert!(got);
}

// ─── §2 — ServerConfig construction precedence ──────────────────────

#[test]
fn server_config_default_is_false() {
    let cfg = ServerConfig {
        host: "127.0.0.1".into(),
        port: 0,
        channel: "memory".into(),
        auth_token: String::new(),
        log_level: "INFO".into(),
        log_format: "json".into(),
        log_file: None,
        database_url: None,
        config_path: None,
        strict_type_driven_transport: false,
        default_backend: None,
        schemas_dir: None,
    };
    assert!(!cfg.strict_type_driven_transport);
}

#[test]
fn server_config_explicit_true_takes_effect() {
    let cfg = ServerConfig {
        host: "127.0.0.1".into(),
        port: 0,
        channel: "memory".into(),
        auth_token: String::new(),
        log_level: "INFO".into(),
        log_format: "json".into(),
        log_file: None,
        database_url: None,
        config_path: None,
        strict_type_driven_transport: true,
        default_backend: None,
        schemas_dir: None,
    };
    assert!(cfg.strict_type_driven_transport);
}

// ─── §3 — Precedence rule (CLI > env > default) ─────────────────────

#[test]
fn cli_flag_wins_over_env_var_when_both_set_truthy() {
    // Simulates the main.rs precedence: `cli_flag || env_var`.
    // When both are truthy, result is truthy (no surprise).
    let var_name = "AXON_TEST_FASE31_F_PRECEDENCE_BOTH";
    std::env::set_var(var_name, "1");
    let cli_flag = true;
    let resolved = cli_flag || parse_truthy_env(var_name);
    std::env::remove_var(var_name);
    assert!(resolved);
}

#[test]
fn cli_flag_off_falls_back_to_env_var() {
    // CLI flag absent (false) → fall back to env var.
    let var_name = "AXON_TEST_FASE31_F_FALLBACK_ENV";
    std::env::set_var(var_name, "true");
    let cli_flag = false;
    let resolved = cli_flag || parse_truthy_env(var_name);
    std::env::remove_var(var_name);
    assert!(resolved, "env var should win when CLI flag is unset");
}

#[test]
fn no_cli_no_env_resolves_to_false() {
    let var_name = "AXON_TEST_FASE31_F_NO_OPT_IN_PATH";
    std::env::remove_var(var_name);
    let cli_flag = false;
    let resolved = cli_flag || parse_truthy_env(var_name);
    assert!(!resolved, "absent CLI + absent env should resolve to false (D6)");
}

#[test]
fn env_var_falsy_doesnt_override_cli_true() {
    // Edge case: CLI is true, env var is "false". CLI must win.
    // (The `||` short-circuit gives this for free.)
    let var_name = "AXON_TEST_FASE31_F_CLI_TRUE_ENV_FALSE";
    std::env::set_var(var_name, "false");
    let cli_flag = true;
    let resolved = cli_flag || parse_truthy_env(var_name);
    std::env::remove_var(var_name);
    assert!(resolved, "CLI true must win over env false");
}

// ─── §4 — Cross-stack contract anchors ──────────────────────────────

#[test]
fn env_var_name_is_canonical() {
    // D7 cross-stack contract — the var name `AXON_STRICT_TYPE_
    // DRIVEN_TRANSPORT` is shared verbatim with Python. This test
    // anchors the constant; a future rename would require a
    // coordinated cross-stack change + adopter migration note.
    const CANONICAL_NAME: &str = "AXON_STRICT_TYPE_DRIVEN_TRANSPORT";
    // Set + parse + clear, using the constant directly.
    std::env::set_var(CANONICAL_NAME, "1");
    let got = parse_truthy_env(CANONICAL_NAME);
    std::env::remove_var(CANONICAL_NAME);
    assert!(got, "canonical env var name '{CANONICAL_NAME}' must parse truthy");
}

#[test]
fn truthy_alphabet_is_intentionally_constrained() {
    // The truthy alphabet is intentionally small + opinionated:
    // {"1", "true", "yes", "on"}. We do NOT accept "y", "t",
    // "enabled", "active", etc. — those would be silent
    // interpretation drift. This test pins the contract.
    let var_name = "AXON_TEST_FASE31_F_NOT_ACCEPTED";
    for val in ["y", "t", "enabled", "active", "yep", "TRUE!", "1.0"] {
        std::env::set_var(var_name, val);
        let got = parse_truthy_env(var_name);
        std::env::remove_var(var_name);
        assert!(!got, "non-canonical value '{val}' should NOT be truthy");
    }
}
