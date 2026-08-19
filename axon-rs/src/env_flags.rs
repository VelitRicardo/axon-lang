//! v2.81.0 — cross-stack environment-flag parsing, dependency-free.
//!
//! [`parse_truthy_env`] is the canonical truthy-value contract shared with the
//! Python CLI (`axon/cli/serve_cmd.py`). It reads an env var and compares
//! lowercase strings; it links nothing.
//!
//! It lived in `axon_server.rs`, and `main.rs` called
//! `axon::axon_server::parse_truthy_env` while assembling the `ServerConfig` —
//! which is fine while the server is unconditional, and a hard build failure the
//! moment it is not. Same shape as `AXON_VERSION` in the flow executor
//! (v2.81.0) and `IngestProvenance` in the OOXML reader (v2.81.0): the general
//! utility living wherever it was first needed. `axon_server` re-exports it, so
//! `axon::axon_server::parse_truthy_env` keeps resolving under a `server` build.

/// v1.22.0 (D7) — Parse a truthy env var per the cross-stack
/// contract. Truthy values (case-insensitive): "1", "true", "yes",
/// "on". Empty / unset / any other value → false.
///
/// Public so the Python CLI can call out to the same canonical
/// parser via FFI if needed (though Python ships its own parser
/// of the same shape per `axon/cli/serve_cmd.py` — the contract
/// is the VALUE SET, not a binary link).
///
/// Examples:
///   `AXON_STRICT_TYPE_DRIVEN_TRANSPORT=1`      → true
///   `AXON_STRICT_TYPE_DRIVEN_TRANSPORT=true`   → true
///   `AXON_STRICT_TYPE_DRIVEN_TRANSPORT=YES`    → true
///   `AXON_STRICT_TYPE_DRIVEN_TRANSPORT=on`     → true
///   `AXON_STRICT_TYPE_DRIVEN_TRANSPORT=0`      → false
///   `AXON_STRICT_TYPE_DRIVEN_TRANSPORT=false`  → false
///   `AXON_STRICT_TYPE_DRIVEN_TRANSPORT=`       → false (empty)
///   (unset)                                    → false
pub fn parse_truthy_env(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on",
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // v2.81.0 — the v1.22.0 value-set contract, proved WHERE THE FUNCTION
    // LIVES. Its coverage was `tests/flag_surface.rs`, which also names
    // `ServerConfig` and therefore moves behind the `server` feature — so gating
    // it would have left this function shipping in every profile with coverage
    // in only one. The cross-stack contract (`axon/cli/serve_cmd.py` parses the
    // same value set) is not a server contract; it is this function's contract.

    #[test]
    fn parse_truthy_accepts_canonical_truthy_values() {
        // Sequential because env vars are process-global. A unique var name per
        // test avoids cross-test leakage.
        let cases = [
            "1", "true", "TRUE", "True", "yes", "YES", "Yes", "on", "ON", "On",
        ];
        let var_name = "AXON_TEST_FASE118_B2_TRUTHY";
        for val in cases {
            std::env::set_var(var_name, val);
            let got = parse_truthy_env(var_name);
            std::env::remove_var(var_name);
            assert!(got, "value '{val}' must be truthy");
        }
    }

    #[test]
    fn parse_truthy_rejects_falsy_values() {
        let cases = [
            "0", "false", "FALSE", "no", "off", "disabled", "anything", "", "2",
            "truee", // typo
            "01",    // leading zero — strict
        ];
        let var_name = "AXON_TEST_FASE118_B2_FALSY";
        for val in cases {
            std::env::set_var(var_name, val);
            let got = parse_truthy_env(var_name);
            std::env::remove_var(var_name);
            assert!(!got, "value '{val}' must NOT be truthy");
        }
    }

    #[test]
    fn parse_truthy_returns_false_when_var_unset() {
        let var_name = "AXON_TEST_FASE118_B2_UNSET_XYZQ";
        std::env::remove_var(var_name);
        assert!(!parse_truthy_env(var_name));
    }

    #[test]
    fn parse_truthy_trims_whitespace() {
        let var_name = "AXON_TEST_FASE118_B2_TRIM";
        std::env::set_var(var_name, "  true  ");
        let got = parse_truthy_env(var_name);
        std::env::remove_var(var_name);
        assert!(got, "whitespace-padded 'true' should still be truthy");

        std::env::set_var(var_name, "\t1\n");
        let got = parse_truthy_env(var_name);
        std::env::remove_var(var_name);
        assert!(got, "tab/newline-padded '1' should still be truthy");
    }
}
