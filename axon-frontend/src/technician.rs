//! v2.39.0 — Remote Hands: the shared, compiler-and-runtime vocabulary for
//! the technician-command surface (`tool { target:, risk:, argv: }`).
//!
//! This module is deliberately dependency-free and pure so BOTH the frontend
//! type-checker (`axon-frontend::type_checker`) and the runtime dispatcher
//! (`axon-lang`, which consumes this crate) classify an argv template the
//! **same** way — a single source of truth for the one property the whole cycle
//! rests on: a `${param}` placeholder is a *whole* argv element, bound
//! to a typed argument, substituted opaquely, and never re-parsed by a shell.
//!
//! Nothing here executes a command; it only *classifies* the template so the
//! checker can reject an unsafe shape at compile time and the runtime can
//! substitute arguments without ever tokenising them.

/// The v1-closed `risk:` catalog. Additive-only; a third level is a cycle, not a
/// config edit (section 5 restraint).
pub const RISK_SAFE: &str = "safe";
pub const RISK_DESTRUCTIVE: &str = "destructive";
pub const VALID_RISK_LEVELS: &[&str] = &[RISK_SAFE, RISK_DESTRUCTIVE];

/// The two mandatory labels a `risk: destructive` tool's bound session must
/// offer as a reachable `branch{…}` (the human confirm/deny exit — the design decision).
pub const CONFIRM_APPROVED_LABEL: &str = "approved";
pub const CONFIRM_DENIED_LABEL: &str = "denied";

/// One classified argv element. The classification is total: every element is
/// exactly one of these, so the checker's exhaustiveness is the argument that
/// no ambiguous ("partly a placeholder") token can slip through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgvToken {
    /// A fixed argv element — passed to the process verbatim (`"ping"`, `"-c"`).
    Literal(String),
    /// A whole-element `${name}` placeholder. `name` is the bound parameter
    /// (the `${…}` wrapper stripped). Substituted as ONE opaque argv argument.
    Placeholder(String),
    /// A malformed / mixed element that contains `${` but is NOT a clean
    /// whole-element placeholder — e.g. `"${host}.txt"`, `"pre${x}"`,
    /// `"${a}${b}"`, `"${}"`, `"${1bad}"`. This is the shape the cycle exists to
    /// forbid: it would let an argument fuse with surrounding text or be split,
    /// reopening escape/injection. Always a compile error (`axon-T859`); it
    /// never reaches the runtime.
    Partial(String),
}

/// `true` for an ASCII identifier (`[A-Za-z_][A-Za-z0-9_]*`) — the same shape a
/// `parameters:` entry name and a `{param}` route capture use.
fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Classify one argv element. The ONLY accepted placeholder shape is an element
/// whose ENTIRE text is `${<ident>}` — anything else that mentions `${` is
/// `Partial` (rejected), and an element with no `${` is a `Literal`.
///
/// This is intentionally strict: `"${host}"` binds, `"${host}.txt"` does not.
/// A partial token is where a market-style string template would let an
/// argument fuse with adjacent text; refusing it is what makes the argv model
/// injection-safe rather than merely tidy.
pub fn classify_argv_token(tok: &str) -> ArgvToken {
    if let Some(inner) = tok.strip_prefix("${").and_then(|s| s.strip_suffix('}')) {
        if !inner.contains("${") && is_ident(inner) {
            return ArgvToken::Placeholder(inner.to_string());
        }
        // `${}`, `${1bad}`, `${a}${b}` (inner still has `${`) → malformed.
        return ArgvToken::Partial(tok.to_string());
    }
    if tok.contains("${") {
        return ArgvToken::Partial(tok.to_string());
    }
    ArgvToken::Literal(tok.to_string())
}

/// Classify a whole argv template.
pub fn classify_argv(argv: &[String]) -> Vec<ArgvToken> {
    argv.iter().map(|t| classify_argv_token(t)).collect()
}

/// A stable, canonical string identity of a rendered/declared argv template,
/// used by the enterprise confirmation-hash binding and the reference
/// agent's template allowlist. NUL-separates elements so no argument
/// value can forge an element boundary. The caller hashes this (SHA-256) — this
/// module stays crypto-free so the frontend has zero new deps.
pub fn argv_canonical_bytes(argv: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, el) in argv.iter().enumerate() {
        if i > 0 {
            out.push(0u8);
        }
        out.extend_from_slice(el.as_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_placeholder_binds() {
        assert_eq!(
            classify_argv_token("${host}"),
            ArgvToken::Placeholder("host".into())
        );
        assert_eq!(
            classify_argv_token("${count_2}"),
            ArgvToken::Placeholder("count_2".into())
        );
    }

    #[test]
    fn literals_pass_through() {
        assert_eq!(classify_argv_token("ping"), ArgvToken::Literal("ping".into()));
        assert_eq!(classify_argv_token("-c"), ArgvToken::Literal("-c".into()));
        // A dollar with no brace is an ordinary literal, not a placeholder.
        assert_eq!(classify_argv_token("$HOME"), ArgvToken::Literal("$HOME".into()));
    }

    #[test]
    fn partial_tokens_are_rejected() {
        for bad in ["${host}.txt", "pre${x}", "${a}${b}", "${}", "${1bad}", "x${y}z"] {
            assert!(
                matches!(classify_argv_token(bad), ArgvToken::Partial(_)),
                "expected Partial for {bad:?}"
            );
        }
    }

    #[test]
    fn canonical_bytes_separate_elements() {
        let a = argv_canonical_bytes(&["a".into(), "b".into()]);
        let b = argv_canonical_bytes(&["ab".into()]);
        assert_ne!(a, b, "element boundary must be forgery-proof");
    }
}
