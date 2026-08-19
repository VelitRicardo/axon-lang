//! v2.67.0 — **resolving a `resource.endpoint` config key to a real address.**
//!
//! # Why this port has to exist
//!
//! `axon-T944` makes `resource.endpoint` a **config key** (`db.main`), never a
//! URL or a DSN. That is the law the language already enforced everywhere else —
//! `axon-T850` on `upstream.resolve`, `axon-T902` on `tool.secret`, in the same
//! words: *URLs and credentials never appear in source.* `resource` was a
//! grandfathered violation of it.
//!
//! But a law that removes the address from the source has to say where the
//! address **is**. That is this module. Without it, `axon-T944` would be a rule
//! that makes programs *less* runnable — which is exactly the kind of "safety"
//! that gets switched off.
//!
//! # Deny by default
//!
//! An unresolved key is an **error**, never a fallback. This is the v2.67.0 lesson,
//! which cost three kernel bugs to learn: *when the evidence is missing,
//! substitute the belief and report agreement* was the shape of every one of
//! them. A resolver that quietly returns `localhost` when a key is unset is that
//! bug with a friendlier face — it would turn a misconfigured production
//! deployment into a silent connection to nothing.
//!
//! # The OSS default is TOTAL and MECHANICAL
//!
//! [`EnvResourceResolver`] maps a key to an environment variable by one
//! deterministic rule, with no table to fall out of sync:
//!
//! ```text
//!   db.main            →  AXON_RESOURCE_DB_MAIN
//!   crm.salesforce.base →  AXON_RESOURCE_CRM_SALESFORCE_BASE
//! ```
//!
//! Enterprise substitutes a per-tenant config resolver (the same shape
//! `upstream.resolve` already uses), and the OSS runtime never learns what a
//! tenant is. The port is what keeps that split honest.

use std::collections::HashMap;
use std::fmt;

/// Why a `resource.endpoint` key could not be turned into an address.
///
/// Every variant names the key AND what the operator has to do about it. A
/// resolver failure is a deployment failure, and a deployment failure that does
/// not say which knob to turn is a support ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceResolveError {
    /// The key is not configured. **Never a fallback** — see the module doc.
    Unset {
        key: String,
        /// What the OSS resolver looked for, so the message is actionable.
        looked_for: String,
    },
    /// The key resolves, but to an empty value. An empty address is not an
    /// address; treating it as one would defer the failure to a connect
    /// timeout, far from the cause.
    Empty { key: String },
}

impl fmt::Display for ResourceResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceResolveError::Unset { key, looked_for } => write!(
                f,
                "resource endpoint '{key}' is not configured (looked for {looked_for}). \
                 The address lives in configuration, never in source (axon-T944) — set it, \
                 or the resource names infrastructure that does not exist. It is NOT defaulted: \
                 a resolver that invents an address turns a misconfiguration into a silent \
                 connection to nothing."
            ),
            ResourceResolveError::Empty { key } => write!(
                f,
                "resource endpoint '{key}' resolves to an EMPTY value. An empty address is not \
                 an address; accepting it would defer the failure to a connect timeout, far \
                 from the cause."
            ),
        }
    }
}

impl std::error::Error for ResourceResolveError {}

/// The port: a `resource.endpoint` config key → the address it names.
pub trait ResourceResolver: Send + Sync {
    fn resolve(&self, key: &str) -> Result<String, ResourceResolveError>;
}

/// The OSS default — a **total, mechanical** key → environment-variable rule.
///
/// `db.main` → `AXON_RESOURCE_DB_MAIN`. Uppercase; `.` and `-` become `_`.
///
/// There is deliberately no lookup table: a table is a second place the truth
/// can live, and a second place the truth can live is how the islands happened.
#[derive(Debug, Default, Clone, Copy)]
pub struct EnvResourceResolver;

/// The key → env-var name rule, exposed so diagnostics can *show the operator
/// what to set* rather than making them guess.
pub fn env_var_for_key(key: &str) -> String {
    let body: String = key
        .chars()
        .map(|c| if c == '.' || c == '-' { '_' } else { c })
        .collect();
    format!("AXON_RESOURCE_{}", body.to_ascii_uppercase())
}

impl ResourceResolver for EnvResourceResolver {
    fn resolve(&self, key: &str) -> Result<String, ResourceResolveError> {
        let var = env_var_for_key(key);
        match std::env::var(&var) {
            Err(_) => Err(ResourceResolveError::Unset {
                key: key.to_string(),
                looked_for: format!("${var}"),
            }),
            Ok(v) if v.trim().is_empty() => Err(ResourceResolveError::Empty {
                key: key.to_string(),
            }),
            Ok(v) => Ok(v.trim().to_string()),
        }
    }
}

/// An explicit map — for tests, and for any host that already holds its config
/// in memory. Unset keys still REFUSE; the deny-by-default property is a
/// property of the port, not of one implementation.
#[derive(Debug, Default, Clone)]
pub struct MapResourceResolver {
    entries: HashMap<String, String>,
}

impl MapResourceResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, key: &str, value: &str) -> Self {
        self.entries.insert(key.to_string(), value.to_string());
        self
    }
}

impl ResourceResolver for MapResourceResolver {
    fn resolve(&self, key: &str) -> Result<String, ResourceResolveError> {
        match self.entries.get(key) {
            None => Err(ResourceResolveError::Unset {
                key: key.to_string(),
                looked_for: format!("an entry '{key}' in the configured resolver"),
            }),
            Some(v) if v.trim().is_empty() => Err(ResourceResolveError::Empty {
                key: key.to_string(),
            }),
            Some(v) => Ok(v.trim().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_to_env_var_rule_is_mechanical() {
        assert_eq!(env_var_for_key("db.main"), "AXON_RESOURCE_DB_MAIN");
        assert_eq!(
            env_var_for_key("crm.salesforce.base"),
            "AXON_RESOURCE_CRM_SALESFORCE_BASE"
        );
        assert_eq!(env_var_for_key("cache.hot-tier"), "AXON_RESOURCE_CACHE_HOT_TIER");
    }

    /// **An unset key REFUSES.** It does not default, and it does not guess.
    ///
    /// v2.67.0 cost three kernel bugs to learn this, and all three were the same
    /// bug: *when the evidence is missing, substitute the belief and report
    /// agreement*. A resolver that returns `localhost` for an unset key is that
    /// bug wearing a helpful expression.
    #[test]
    fn an_unset_key_refuses_it_does_not_default_to_anything() {
        let r = MapResourceResolver::new();
        let err = r.resolve("db.main").expect_err("an unset key must REFUSE");
        assert!(matches!(err, ResourceResolveError::Unset { .. }));
        // And the message must tell the operator which knob to turn.
        assert!(err.to_string().contains("db.main"));
    }

    /// An empty value is not an address. Accepting it would defer the failure to
    /// a connect timeout, far away from the cause — the classic way a config
    /// mistake becomes a 3 a.m. incident about the network.
    #[test]
    fn an_empty_value_is_not_an_address() {
        let r = MapResourceResolver::new().with("db.main", "   ");
        assert!(matches!(
            r.resolve("db.main"),
            Err(ResourceResolveError::Empty { .. })
        ));
    }

    #[test]
    fn a_configured_key_resolves() {
        let r = MapResourceResolver::new().with("db.main", "postgres://h/app");
        assert_eq!(r.resolve("db.main").unwrap(), "postgres://h/app");
    }
}
