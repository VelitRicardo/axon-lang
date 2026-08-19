//! v2.67.0 — the **source adapter registry**: what an `observe` actually looks at.
//!
//! # Why this exists
//!
//! ```text
//! observe ProdHealth from Infra {
//!     sources: [ prometheus, cloudwatch ]   // ← what ARE these?
//!     quorum: 2
//!     timeout: 5s
//!     on_partition: fail
//!     certainty_floor: 0.8
//! }
//! ```
//!
//! Nothing in the language said what a `source` name resolves to, and so nothing
//! ever resolved one. v2.67.0 (F14) found the whole Cognitive-I/O family unreachable;
//! v2.67.0 found that the architecture was complete and only two pieces were missing —
//! a supervisor, and **a `Handler` that actually goes and looks at something.**
//!
//! # The defect this replaces
//!
//! The only `Handler` in the tree was [`crate::handlers::dry_run::DryRunHandler`],
//! and its `observe` ends:
//!
//! ```ignore
//! make_envelope(1.0, &self.name, "observed", None)
//! ```
//!
//! **Certainty `1.0`. Always. Without going anywhere.** A perfect-confidence clean
//! bill of health for a system nobody ever examined — and it was the *only* option
//! available, so a supervisor wired to it would have reported perfect health for
//! everything, forever.
//!
//! That is the v2.67.0 defect in its purest form, and it is the thing this module
//! exists to make impossible.
//!
//! # The law
//!
//! **An observation that cannot be taken must REFUSE. It must never return a
//! default envelope.** A source that is not registered is not "probably fine" —
//! it is *unknown*, and the difference between *unknown* and *healthy* is the
//! entire reason `observe` exists. So the registry is **deny-by-default**:
//!
//! - unregistered source ⇒ [`SourceError::Unregistered`] ⇒ the observation refuses;
//! - a probe that fails ⇒ that source does not count toward quorum;
//! - fewer successes than `quorum:` ⇒ a **partition**, honoured per `on_partition:`;
//! - aggregate certainty below `certainty_floor:` ⇒ refused.
//!
//! OSS ships the registry with **zero remote adapters** — enterprise mounts
//! Prometheus / CloudWatch / Datadog behind the same trait, exactly as it does for
//! `tool_registry` and `shield_registry`. The one adapter OSS *does* ship is
//! [`ResourceProbeAdapter`] (below), which probes a source that names a **declared
//! `resource`** — the built-in family ratified in the v2.67.0 plan.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use crate::ir_nodes::IRResource;

/// What a source reported when it was actually asked.
#[derive(Debug, Clone)]
pub struct SourceReading {
    /// Epistemic certainty `c ∈ [0, 1]` in this reading. This is **not** a health
    /// score — it is how sure the adapter is that what it reports is *true*. An
    /// adapter that reached its target cleanly reports high `c`; one that got a
    /// stale cache or a partial answer reports low `c`.
    pub certainty: f64,
    /// The observed state. Opaque here; the kernels downstream (`immune`'s
    /// KL-divergence sensor, `reconcile`'s Jaccard drift) consume it.
    pub data: serde_json::Map<String, serde_json::Value>,
}

impl SourceReading {
    pub fn new(certainty: f64, data: serde_json::Map<String, serde_json::Value>) -> Self {
        SourceReading {
            certainty: certainty.clamp(0.0, 1.0),
            data,
        }
    }
}

/// Why a source could not be read. **Every variant is a refusal** — none of them
/// degrade into an observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// The source name resolves to no registered adapter.
    ///
    /// This is the load-bearing refusal. An unregistered source is **unknown**,
    /// not healthy, and an `observe` that silently skipped it would report a
    /// quorum it never actually reached.
    Unregistered { source: String },
    /// The adapter was reached but could not answer within `timeout:`.
    Timeout { source: String, after: String },
    /// The adapter reached its target and the target answered with a failure.
    Unreachable { source: String, detail: String },
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceError::Unregistered { source } => write!(
                f,
                "source '{source}' resolves to no registered adapter — an unregistered source is \
                 UNKNOWN, not healthy. Register an adapter for it, or name a declared `resource`. \
                 (an observation that cannot be taken must refuse, never return a default \
                 envelope)"
            ),
            SourceError::Timeout { source, after } => {
                write!(f, "source '{source}' did not answer within {after}")
            }
            SourceError::Unreachable { source, detail } => {
                write!(f, "source '{source}' is unreachable: {detail}")
            }
        }
    }
}

impl std::error::Error for SourceError {}

/// A thing an `observe` can actually look at.
///
/// Implementors go somewhere real. **An adapter must never manufacture a reading**
/// — if it cannot reach its target it returns a [`SourceError`], and the caller
/// counts that source as *not answered* rather than as *answered fine*.
pub trait SourceAdapter: Send + Sync {
    /// The source name this adapter answers to (the identifier in `sources: [ … ]`).
    fn name(&self) -> &str;

    /// Probe the source. `resource` is `Some` when the source names a declared
    /// `resource` (the built-in family); `None` for a purely external source.
    ///
    /// `timeout` is the observe's declared `timeout:`, already parsed.
    fn probe(
        &self,
        resource: Option<&IRResource>,
        timeout: std::time::Duration,
    ) -> Result<SourceReading, SourceError>;
}

// ────────────────────────────────────────────────────────────────────
//  The registry — deny by default
// ────────────────────────────────────────────────────────────────────

static REGISTRY: LazyLock<RwLock<HashMap<String, Arc<dyn SourceAdapter>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Register an adapter under a source name. Enterprise calls this at boot for
/// Prometheus / CloudWatch / Datadog / …; OSS registers nothing by default.
pub fn register_source_adapter(name: impl Into<String>, adapter: Arc<dyn SourceAdapter>) {
    REGISTRY
        .write()
        .expect("source registry RwLock poisoned")
        .insert(name.into(), adapter);
}

/// Resolve a source name. `None` ⇒ **unregistered** ⇒ the caller must REFUSE.
pub fn lookup_source_adapter(name: &str) -> Option<Arc<dyn SourceAdapter>> {
    REGISTRY
        .read()
        .expect("source registry RwLock poisoned")
        .get(name)
        .cloned()
}

/// Test/teardown helper — drop every registration.
pub fn clear_source_adapters() {
    REGISTRY
        .write()
        .expect("source registry RwLock poisoned")
        .clear();
}

// ────────────────────────────────────────────────────────────────────
//  The one adapter OSS ships: probe a declared `resource`
// ────────────────────────────────────────────────────────────────────

/// v2.67.0 — the built-in adapter family (ratified): a `source` that names a
/// **declared `resource`** is probed by reaching that resource's `endpoint`.
///
/// This is the honest OSS default. It uses only what the program already declares
/// — `resource Db { kind: postgres  endpoint: db.main }` — **resolves that config
/// key** (v2.67.0: the address lives in configuration, never in source — `axon-T944`)
/// and goes to the real address. It reports:
///
/// - `certainty: 1.0` when the endpoint accepts a connection (we reached it and it
///   is up — that is a fact we established, not one we assumed);
/// - a [`SourceError::Unreachable`] when it does not.
///
/// It deliberately does **not** claim to understand the protocol behind the
/// endpoint. A TCP reachability check is a *modest* claim, and a modest claim we
/// can actually back is worth infinitely more than a confident one we cannot —
/// which is exactly the trade the `DryRunHandler`'s `c: 1.0` got backwards.
/// Deeper, protocol-aware probes (a real `SELECT 1`, a Prometheus query) are
/// enterprise adapters behind this same trait.
pub struct ResourceProbeAdapter {
    name: String,
    /// v2.67.0 — `resource.endpoint` is a **config key** (`axon-T944`), not an
    /// address. Something has to turn it into one, and deciding *where
    /// configuration lives* is not this module's business — that is the port's.
    resolver: std::sync::Arc<dyn crate::resource_resolver::ResourceResolver>,
}

impl ResourceProbeAdapter {
    /// Probe using the OSS default resolver (`AXON_RESOURCE_<KEY>` env vars).
    pub fn new(name: impl Into<String>) -> Self {
        ResourceProbeAdapter {
            name: name.into(),
            resolver: std::sync::Arc::new(crate::resource_resolver::EnvResourceResolver),
        }
    }

    /// v2.67.0 — probe using an explicit resolver: enterprise's per-tenant
    /// config, or a test's in-memory map.
    pub fn with_resolver(
        name: impl Into<String>,
        resolver: std::sync::Arc<dyn crate::resource_resolver::ResourceResolver>,
    ) -> Self {
        ResourceProbeAdapter {
            name: name.into(),
            resolver,
        }
    }

    /// Resolve the resource's endpoint **key** and reduce it to `host:port`.
    ///
    /// v2.67.0 — the endpoint is a config key, so it is RESOLVED first. That
    /// also *improves the refusal*: where the old code could only say "no
    /// reachable address", this can say **which key is unset** — a failure that
    /// names the knob the operator has to turn instead of one that reads like a
    /// network fault.
    ///
    /// `Err` ⇒ the caller refuses rather than guessing. **A refusal we can back
    /// is worth infinitely more than a confident answer we cannot** — the exact
    /// trade `DryRunHandler`'s `c: 1.0` got backwards.
    fn socket_addr(&self, resource: &IRResource) -> Result<String, String> {
        let key = resource.endpoint.trim();
        if key.is_empty() {
            return Err(format!(
                "resource '{}' declares no `endpoint:` — it names no infrastructure",
                resource.name
            ));
        }
        let resolved = self.resolver.resolve(key).map_err(|e| e.to_string())?;
        Self::host_port(&resolved, resource).ok_or_else(|| {
            format!(
                "resource '{}' (kind: {}) resolves to '{}', which yields no reachable \
                 host:port — refusing rather than guessing one",
                resource.name, resource.kind, resolved
            )
        })
    }

    /// Reduce an already-resolved address to `host:port`, using the kind's
    /// default port when it omits one. `None` ⇒ no address can be determined.
    fn host_port(ep: &str, resource: &IRResource) -> Option<String> {
        let ep = ep.trim();
        if ep.is_empty() {
            return None;
        }
        // Strip a scheme if present, then any path/query tail.
        let after_scheme = ep.split("://").last()?;
        let authority = after_scheme.split(['/', '?']).next()?;
        // Strip credentials (`user:pass@host:port`).
        let hostport = authority.rsplit('@').next()?;
        if hostport.is_empty() {
            return None;
        }
        if hostport.contains(':') {
            return Some(hostport.to_string());
        }
        let port = match resource.kind.as_str() {
            "postgres" => 5432,
            "redis" => 6379,
            "mysql" => 3306,
            "http" => 80,
            "https" => 443,
            // An unknown kind with no explicit port gives us no address to reach.
            // We refuse rather than invent one.
            _ => return None,
        };
        Some(format!("{hostport}:{port}"))
    }
}

impl SourceAdapter for ResourceProbeAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn probe(
        &self,
        resource: Option<&IRResource>,
        timeout: std::time::Duration,
    ) -> Result<SourceReading, SourceError> {
        let resource = resource.ok_or_else(|| SourceError::Unregistered {
            source: self.name.clone(),
        })?;

        let addr = self
            .socket_addr(resource)
            .map_err(|detail| SourceError::Unreachable {
                source: self.name.clone(),
                detail,
            })?;

        let socket: std::net::SocketAddr = addr
            .parse()
            .or_else(|_| {
                use std::net::ToSocketAddrs;
                addr.to_socket_addrs()
                    .map_err(|e| SourceError::Unreachable {
                        source: self.name.clone(),
                        detail: format!("cannot resolve '{addr}': {e}"),
                    })
                    .and_then(|mut it| {
                        it.next().ok_or_else(|| SourceError::Unreachable {
                            source: self.name.clone(),
                            detail: format!("'{addr}' resolved to no address"),
                        })
                    })
            })?;

        match std::net::TcpStream::connect_timeout(&socket, timeout) {
            Ok(_) => {
                let mut data = serde_json::Map::new();
                data.insert("resource".into(), resource.name.clone().into());
                data.insert("kind".into(), resource.kind.clone().into());
                data.insert("address".into(), addr.into());
                data.insert("reachable".into(), true.into());
                // c = 1.0 because we ESTABLISHED reachability — we connected. This
                // is a fact, not an assumption, and that is the whole difference
                // between this adapter and the DryRunHandler it replaces.
                Ok(SourceReading::new(1.0, data))
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Err(SourceError::Timeout {
                source: self.name.clone(),
                after: format!("{timeout:?}"),
            }),
            Err(e) => Err(SourceError::Unreachable {
                source: self.name.clone(),
                detail: format!("{addr}: {e}"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(name: &str, kind: &str, endpoint: &str) -> IRResource {
        IRResource {
            node_type: "resource",
            source_line: 0,
            source_column: 0,
            name: name.into(),
            kind: kind.into(),
            endpoint: endpoint.into(),
            capacity: None,
            lifetime: "affine".into(),
            certainty_floor: None,
            shield_ref: String::new(),
            within: String::new(),
        }
    }

    /// **The load-bearing law.** An unregistered source resolves to nothing, and
    /// the caller must refuse. There is no default adapter, and there must never
    /// be one: a source nobody registered is UNKNOWN, and reporting unknown as
    /// healthy is the entire defect v2.67.0 exists to end.
    ///
    /// NOTE: the registry is a process-global (same shape as `shield_registry`), so
    /// these tests use names nobody else registers rather than clearing it — a
    /// global `clear()` would race with the parallel tests around them.
    #[test]
    fn the_registry_is_deny_by_default() {
        assert!(
            lookup_source_adapter("prometheus").is_none(),
            "OSS must ship ZERO remote adapters — an unregistered source must resolve to nothing"
        );
        assert!(lookup_source_adapter("cloudwatch").is_none());
        assert!(lookup_source_adapter("datadog").is_none());
    }

    #[test]
    fn a_registered_adapter_resolves() {
        register_source_adapter("reg_probe", Arc::new(ResourceProbeAdapter::new("reg_probe")));
        assert!(lookup_source_adapter("reg_probe").is_some());
    }

    /// The probe reaches a REAL address. This is the test that separates v2.67.0
    /// from the `DryRunHandler` it replaces: the certainty is `1.0` because we
    /// **connected**, not because we assumed.
    #[test]
    fn a_reachable_resource_is_observed_because_we_actually_connected() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        // v2.67.0 — the resource names a config KEY; the resolver supplies the
        // address. The probe still opens a real socket to a real listener.
        let r = resource("Db", "postgres", "db.main");
        let resolver = std::sync::Arc::new(
            crate::resource_resolver::MapResourceResolver::new()
                .with("db.main", &format!("postgres://{addr}/app")),
        );

        let reading = ResourceProbeAdapter::with_resolver("db", resolver)
            .probe(Some(&r), std::time::Duration::from_secs(2))
            .expect("a listening endpoint must be observable");

        assert_eq!(reading.certainty, 1.0);
        assert_eq!(reading.data["reachable"], true);
        assert_eq!(reading.data["resource"], "Db");
    }

    /// **The refusal that matters.** An unreachable resource is an ERROR, never a
    /// reading. The `DryRunHandler` would have returned `c: 1.0` here — a clean
    /// bill of health for a database that is down.
    #[test]
    fn an_unreachable_resource_refuses_rather_than_reporting_health() {
        // Port 1 on loopback: nothing listens there.
        let r = resource("Db", "postgres", "db.main");
        let resolver = std::sync::Arc::new(
            crate::resource_resolver::MapResourceResolver::new()
                .with("db.main", "postgres://127.0.0.1:1/app"),
        );
        let err = ResourceProbeAdapter::with_resolver("db", resolver)
            .probe(Some(&r), std::time::Duration::from_millis(500))
            .expect_err(
                "an unreachable resource must REFUSE — returning an envelope here would be a \
                 clean bill of health for a system that is down",
            );
        assert!(matches!(err, SourceError::Unreachable { .. } | SourceError::Timeout { .. }));
    }

    /// We refuse to invent an address. A resource whose endpoint gives us nowhere
    /// to go is not probed optimistically — it is refused.
    ///
    /// v2.67.0 — the endpoint is now a config KEY, so "nowhere to go" has two
    /// distinct shapes, and telling them apart is the whole improvement:
    /// **the key is unset** (a knob to turn) versus **the key resolves to
    /// something with no host** (a bad value). Both refuse; neither guesses.
    #[test]
    fn an_unset_endpoint_key_refuses_and_names_the_knob_to_turn() {
        let r = resource("Blob", "https", "blob.archive");
        let err = ResourceProbeAdapter::with_resolver(
            "blob",
            std::sync::Arc::new(crate::resource_resolver::MapResourceResolver::new()),
        )
        .probe(Some(&r), std::time::Duration::from_millis(200))
        .expect_err("an unset key ⇒ refuse");
        let msg = format!("{err}");
        assert!(
            msg.contains("blob.archive"),
            "the refusal must name the KEY the operator has to set — where the old code \
             could only say 'no reachable address', which reads like a network fault. Got: {msg}"
        );
    }

    /// A key that resolves to a value with no host yields no address, and that
    /// too is a refusal — never an optimistic probe.
    #[test]
    fn a_resolved_value_with_no_host_refuses_rather_than_guessing() {
        let r = resource("Blob", "https", "blob.archive");
        let err = ResourceProbeAdapter::with_resolver(
            "blob",
            std::sync::Arc::new(
                crate::resource_resolver::MapResourceResolver::new().with("blob.archive", "///"),
            ),
        )
        .probe(Some(&r), std::time::Duration::from_millis(200))
        .expect_err("no host ⇒ refuse");
        assert!(format!("{err}").contains("refusing rather than guessing"));
    }

    /// The kind's default port is applied to the **resolved** address.
    #[test]
    fn default_ports_come_from_the_declared_kind() {
        let pg = resource("Db", "postgres", "db.main");
        assert_eq!(
            ResourceProbeAdapter::host_port("postgres://db.internal/app", &pg).as_deref(),
            Some("db.internal:5432")
        );
        let redis = resource("Cache", "redis", "cache.main");
        assert_eq!(
            ResourceProbeAdapter::host_port("redis://cache.internal", &redis).as_deref(),
            Some("cache.internal:6379")
        );
        // Credentials are stripped; an explicit port wins over the default.
        assert_eq!(
            ResourceProbeAdapter::host_port("postgres://user:pw@db.internal:6000/app", &pg)
                .as_deref(),
            Some("db.internal:6000")
        );
    }

    /// **The address comes from configuration, and only from configuration.**
    ///
    /// `axon-T944` took the DSN out of the source; this is the other half of that
    /// bargain — the runtime knows where to look. A law that removes the address
    /// from the program without saying where it lives would make programs *less*
    /// runnable, and that is the kind of "safety" that gets switched off.
    #[test]
    fn the_probed_address_is_the_resolved_key_not_the_key_itself() {
        let pg = resource("Db", "postgres", "db.main");
        let adapter = ResourceProbeAdapter::with_resolver(
            "db",
            std::sync::Arc::new(
                crate::resource_resolver::MapResourceResolver::new()
                    .with("db.main", "postgres://db.internal:6000/app"),
            ),
        );
        assert_eq!(
            adapter.socket_addr(&pg).as_deref(),
            Ok("db.internal:6000"),
            "the probe must reach the RESOLVED address, never the config key"
        );
    }
}
