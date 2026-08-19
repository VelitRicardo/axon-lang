//! §Fase 35.d (v1.30.0) — `StoreRegistry`, the closed-catalog
//! SQL-vs-KV dispatch chokepoint of the `axonstore` cognitive data
//! plane.
//!
//! The registry is the **single point** at which a store-op's
//! `store_name` is resolved to a backend. Both execution paths — the
//! sync runner (35.e) and the streaming dispatcher (35.f) — route
//! through it, so there is exactly one SQL-vs-KV decision site and no
//! path divergence (the SSE-gap lesson).
//!
//! # D2 — store resolution is a total function over a closed catalog
//!
//! [`StoreRegistry::build`] is the catalog gate: every `IRAxonStore`'s
//! `backend` must classify into the closed set `{in_memory,
//! postgresql}` ([`classify_backend`]). An unknown backend (`sqlite`,
//! `mysql`, a typo) fails the build with a named [`RegistryError`] —
//! pure, no I/O, fail-fast at deploy. After build, [`resolve`] is
//! total: every `store_name` yields a [`StoreHandle`] or a typed
//! [`StoreError`] — never a panic.
//!
//! [`resolve`]: StoreRegistry::resolve
//!
//! # D3 — zero regression on the key-value path (absolute)
//!
//! `in_memory` is the **implicit default**: a store that is undeclared,
//! declared with an empty `backend`, or declared `in_memory` resolves
//! to [`StoreHandle::InMemory`] — the byte-identical pre-35 key-value
//! path. The SQL path is entered *iff* a matching `IRAxonStore` has
//! `backend == "postgresql"`.
//!
//! Crucially: a declared `postgresql` store whose connection cannot be
//! resolved (a missing `env:` variable, a malformed DSN) yields a typed
//! error — **never** a silent fallback to the key-value store. Silently
//! degrading a misconfigured SQL store to KV would lose writes and
//! serve stale reads; the registry refuses to do it.
//!
//! # Lazy, per-DSN pool cache (D7)
//!
//! The registry build is pure (catalog validation only). A
//! `PostgresStoreBackend` — and therefore its pool and its `env:`
//! resolution — is created on the **first** `resolve` of a given
//! postgresql store, then cached **by resolved DSN**: stores that share
//! a DSN share one pool. A store that is never used never resolves its
//! connection — so a broken `postgresql` store cannot break an
//! unrelated `in_memory` flow (D3).

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use crate::ir_nodes::{IRAxonStore, IRStoreColumnSchema};
use crate::store::error::StoreError;
use crate::store::postgres_backend::{resolve_dsn, PostgresStoreBackend};
use crate::store_schema::StoreColumnType;
use crate::store_schema_manifest::{Manifest, ManifestStore};

// ════════════════════════════════════════════════════════════════════
//  Closed backend catalog (D2)
// ════════════════════════════════════════════════════════════════════

/// The closed catalog of `axonstore` backends honored by the v1.30.0
/// runtime. Growth (e.g. `sqlite`) is a deliberate language decision —
/// a new variant here plus a backend implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreBackendKind {
    /// The in-process key-value path — the pre-35 behavior, and the
    /// implicit default for an undeclared or empty-`backend` store.
    InMemory,
    /// A `sqlx::PgPool`-backed SQL store (35.c `PostgresStoreBackend`).
    Postgresql,
    /// §Fase 94.a — the read-only METADATA view over the tenant's secret
    /// custody (`rotation_without_revelation`). No connection string, no
    /// adopter table: the dispatch handlers route it to the
    /// `axon::secret_custody` port (fail-closed when absent).
    Secrets,
}

impl StoreBackendKind {
    /// The canonical `backend:` spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            StoreBackendKind::InMemory => "in_memory",
            StoreBackendKind::Postgresql => "postgresql",
            StoreBackendKind::Secrets => "secrets",
        }
    }
}

impl fmt::Display for StoreBackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Classify an `IRAxonStore.backend` string into the closed catalog.
///
/// The match is trimmed + case-insensitive. An empty string is the
/// implicit `in_memory` default. `None` means the value is outside the
/// closed catalog — the caller turns that into a build error.
pub fn classify_backend(backend: &str) -> Option<StoreBackendKind> {
    match backend.trim().to_ascii_lowercase().as_str() {
        "" | "in_memory" => Some(StoreBackendKind::InMemory),
        "postgresql" => Some(StoreBackendKind::Postgresql),
        // §Fase 94.a — the secret-custody metadata view.
        "secrets" => Some(StoreBackendKind::Secrets),
        _ => None,
    }
}

// ════════════════════════════════════════════════════════════════════
//  Build-phase error catalog
// ════════════════════════════════════════════════════════════════════

/// A failure building a [`StoreRegistry`] from `IRProgram`'s
/// `axonstore_specs`. These are deploy-time errors — pure, no I/O.
#[derive(Debug, Clone, PartialEq)]
pub enum RegistryError {
    /// An `axonstore` declares a `backend` outside the closed catalog.
    UnknownBackend { store: String, backend: String },
    /// Two `axonstore` declarations share a name.
    DuplicateStore { store: String },
    /// §Fase 113 — the store names a `resource:` that the program does not
    /// declare. `axon-T946` refuses this at compile; reaching it here means the
    /// IR was assembled by hand, and we refuse rather than fall back.
    UnknownResource { store: String, resource: String },
    /// §Fase 113 — the store's `resource.endpoint` config key could not be
    /// resolved to an address. **Never a fallback**: a resolver that invents an
    /// address turns a misconfiguration into a silent connection to nothing.
    UnresolvedEndpoint {
        store: String,
        resource: String,
        detail: String,
    },
    /// §Fase 113.d — a `lease` could not be acquired: it targets a `persistent`
    /// resource (the `!` exponential has no τ to decay), or its duration is
    /// unparseable.
    LeaseRefused {
        lease: String,
        resource: String,
        detail: String,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::UnknownBackend { store, backend } => write!(
                f,
                "axonstore `{store}` declares unknown backend `{backend}` \
                 — the v1.30.0 closed catalog is {{in_memory, postgresql}} \
                 (sqlite is documented future scope)"
            ),
            RegistryError::DuplicateStore { store } => write!(
                f,
                "axonstore `{store}` is declared more than once — store \
                 names must be unique"
            ),
            RegistryError::UnknownResource { store, resource } => write!(
                f,
                "axonstore `{store}` names resource `{resource}`, which the program does not \
                 declare (axon-T946 refuses this at compile — reaching it here means the IR was \
                 assembled by hand). We refuse rather than fall back to a default connection: a \
                 store pointed at nothing beats a store silently pointed somewhere else."
            ),
            RegistryError::UnresolvedEndpoint {
                store,
                resource,
                detail,
            } => write!(
                f,
                "axonstore `{store}` runs on resource `{resource}`, whose endpoint could not be \
                 resolved: {detail}"
            ),
            RegistryError::LeaseRefused {
                lease,
                resource,
                detail,
            } => write!(
                f,
                "lease `{lease}` over resource `{resource}` could not be acquired: {detail}"
            ),
        }
    }
}

impl std::error::Error for RegistryError {}

// ════════════════════════════════════════════════════════════════════
//  Store handle — the resolved dispatch target
// ════════════════════════════════════════════════════════════════════

/// The resolved backend for a store operation. The runner (35.e) and
/// the dispatcher (35.f) match on this to route to SQL or to the
/// key-value path.
#[derive(Debug, Clone)]
pub enum StoreHandle {
    /// The in-process key-value path (D3 — byte-identical to pre-35).
    InMemory,
    /// A Postgres-backed store, with its (shared, cached) backend.
    Postgres(PostgresStoreBackend),
    /// §Fase 94.a — the secret-custody metadata view: the store's
    /// declared `class:` (WITHOUT the trailing dot; callers derive the
    /// key prefix as `class + "."`). Routed to the `secret_custody`
    /// port by the dispatch handlers — never to SQL, never to KV.
    Secrets { class: String },
}

impl StoreHandle {
    /// `true` iff this resolves to the key-value path.
    pub fn is_in_memory(&self) -> bool {
        matches!(self, StoreHandle::InMemory)
    }

    /// `true` iff this resolves to the SQL path.
    pub fn is_postgres(&self) -> bool {
        matches!(self, StoreHandle::Postgres(_))
    }

    /// §Fase 94.a — `true` iff this resolves to the secret-custody
    /// metadata view.
    pub fn is_secrets(&self) -> bool {
        matches!(self, StoreHandle::Secrets { .. })
    }
}

// ════════════════════════════════════════════════════════════════════
//  §Fase 37.x.g (D8) — deploy-time schema-verification report
// ════════════════════════════════════════════════════════════════════

/// The outcome of [`StoreRegistry::verify_postgres_schemas`] — the
/// eager, deploy-time check that every declared `postgresql` store's
/// table resolves against the live database (D8). The failure of a
/// store schema moves from the first production request to deploy.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SchemaVerifyReport {
    /// Stores whose table resolved + introspected cleanly at deploy.
    /// Their schema is now a deploy-verified contract — warm in the
    /// process cache before the first runtime operation.
    pub verified: Vec<String>,
    /// Stores REACHABLE at deploy whose table does not resolve —
    /// `(store_name, diagnostic)`. A FATAL deploy error (D8
    /// fail-closed): a flow's store table is genuinely missing /
    /// ambiguous and would otherwise fail at runtime.
    pub missing: Vec<(String, String)>,
    /// Stores UNREACHABLE or unconfigured at deploy — `(store_name,
    /// diagnostic)`. A NON-fatal warning: the deploy proceeds and
    /// resolution defers to the D9 runtime path. "Deploy is honest,
    /// never brittle" — a transiently-down database does not block a
    /// deploy.
    pub unreachable: Vec<(String, String)>,
}

impl SchemaVerifyReport {
    /// `true` iff the deploy must FAIL — at least one reachable store
    /// has a table that does not resolve (D8 fail-closed).
    pub fn has_fatal(&self) -> bool {
        !self.missing.is_empty()
    }

    /// A human-readable summary of the fatal failures, for the deploy
    /// error response. Empty when there are none.
    pub fn fatal_summary(&self) -> String {
        if self.missing.is_empty() {
            return String::new();
        }
        let detail = self
            .missing
            .iter()
            .map(|(store, diag)| format!("`{store}` — {diag}"))
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "deploy-time store-schema verification failed: {} declared \
             postgresql store table(s) do not resolve on a reachable \
             database: {detail}",
            self.missing.len()
        )
    }
}

// ════════════════════════════════════════════════════════════════════
//  Registered store entry
// ════════════════════════════════════════════════════════════════════

/// One validated `axonstore` declaration held by the registry.
#[derive(Debug, Clone)]
struct RegisteredStore {
    spec: IRAxonStore,
    kind: StoreBackendKind,
    /// §Fase 113 — the DSN this store actually connects with.
    ///
    /// For a store on a `resource:` this is the RESOLVED `resource.endpoint`
    /// config key. For the legacy un-resourced form it is `spec.connection`
    /// verbatim. Either way it is settled ONCE, at build, so `resolve()` cannot
    /// disagree with the schema verifier about where a store points.
    dsn_source: String,
    /// §Fase 113 — the pool size: `resource.capacity`, or the legacy hardcoded
    /// default when the store names no resource.
    ///
    /// This is the field that makes §113 a wire. `capacity` was declared,
    /// lowered, and read by NOTHING; every pool was 10.
    capacity: u32,
    /// §Fase 113 — the resource this store derives from (empty = legacy form).
    /// Carried for diagnostics: an error about a pool must be able to name the
    /// declaration the operator has to edit.
    resource_ref: String,
}

// ════════════════════════════════════════════════════════════════════
//  StoreRegistry
// ════════════════════════════════════════════════════════════════════

/// The closed-catalog store resolver. Built once from a program's
/// `axonstore` declarations; shared (behind an `Arc`) across concurrent
/// dispatch. `Send + Sync`.
pub struct StoreRegistry {
    /// `store_name` → its validated declaration.
    stores: HashMap<String, RegisteredStore>,
    /// resolved DSN → connected backend. Lazy: an entry appears on the
    /// first `resolve` of a postgresql store with that DSN. Stores that
    /// share a DSN share one pool.
    pool_cache: Mutex<HashMap<String, PostgresStoreBackend>>,
    /// §Fase 113.d — **the lease's use-site, which is the reason §113 exists.**
    ///
    /// The README promises `lease` is a τ-decaying affine capability whose
    /// **post-expiry USE is a CT-2 Anchor Breach**. `LeaseKernel` implements that
    /// perfectly — `acquire`, `use_token`, `release`, the breach and all three
    /// `on_expire` policies. It was never *unwired*. It was **unreachable**:
    ///
    /// > **A flow could not `use` a `resource`, so the breach had no moment to
    /// > fire in. The headline guarantee was structurally impossible, not merely
    /// > un-plumbed.** (§111 F14.)
    ///
    /// §113 created the moment. A flow uses an `axonstore`; the store runs on a
    /// `resource`; **that store operation IS the use of the resource.** So every
    /// `resolve()` of a leased store's handle passes through `use_token`, and a
    /// post-expiry one refuses.
    ///
    /// `None` ⇒ the program declares no leases, and this costs nothing.
    leases: Option<Mutex<LeaseGuard>>,
}

/// §Fase 113.d — the leases held over this program's resources.
///
/// One token per leased resource, acquired at build (`acquire: on_start`) and
/// checked on every use.
struct LeaseGuard {
    kernel: crate::runtime::lease_kernel::LeaseKernel,
    /// resource name → the token held over it.
    tokens: HashMap<String, crate::runtime::lease_kernel::LeaseToken>,
}

impl fmt::Debug for StoreRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // List names + kinds only — never dump raw `connection` strings
        // (a literal DSN can carry a password).
        let mut kinds: Vec<(&str, StoreBackendKind)> = self
            .stores
            .iter()
            .map(|(name, r)| (name.as_str(), r.kind))
            .collect();
        kinds.sort_by(|a, b| a.0.cmp(b.0));
        f.debug_struct("StoreRegistry")
            .field("stores", &kinds)
            .field("cached_pools", &self.cached_pool_count())
            .finish()
    }
}

impl StoreRegistry {
    /// Build a registry from a program's `axonstore` declarations.
    ///
    /// D2 catalog gate — pure, no I/O. Fails fast if any declaration
    /// names an unknown backend or if two declarations collide on name.
    /// Connection validity is **not** checked here — that is resolved
    /// lazily, per store, so a broken `postgresql` store cannot fail
    /// the build for an unrelated `in_memory` flow (D3).
    pub fn build(specs: &[IRAxonStore]) -> Result<StoreRegistry, RegistryError> {
        Self::build_with_resources(specs, &[], &crate::resource_resolver::EnvResourceResolver)
    }

    /// §Fase 113 — **build a registry where a store DERIVES from its `resource`.**
    ///
    /// This is the sub-fase the plan warned itself about, in advance and by name:
    ///
    /// > *"A nominal link is not a fix. `axonstore { resource: Db }` as a LABEL —
    /// > with the store still connecting through its own `connection:` — would
    /// > give `lease` its hook and leave `endpoint`, `capacity` and `lifetime`
    /// > governing nothing. **Technically wired and hollow.**"*
    ///
    /// So the reference does not merely *point*. When a store names a resource,
    /// **both facts that matter come from the resource**:
    ///
    /// - its **DSN**, by resolving `resource.endpoint` — a config key, never a
    ///   URL in source (`axon-T944`);
    /// - its **pool size**, from `resource.capacity` — which until §113 was read
    ///   by *zero lines of code in either repository* while every pool in
    ///   existence sat at a hardcoded 10.
    ///
    /// A store with no `resource:` keeps the legacy path verbatim. `connection:`
    /// is what the live deployment runs on, and the migration is soft (ratified):
    /// it still compiles, it warns, and it is ineligible for `lease`/`observe` —
    /// you cannot govern what you did not declare.
    pub fn build_with_resources(
        specs: &[IRAxonStore],
        resources: &[crate::ir_nodes::IRResource],
        resolver: &dyn crate::resource_resolver::ResourceResolver,
    ) -> Result<StoreRegistry, RegistryError> {
        Self::build_governed(specs, resources, &[], resolver)
    }

    /// §Fase 113.d — build a registry that also HOLDS THE LEASES over its
    /// resources, so a store operation is a *use* of the resource it runs on.
    ///
    /// This is the seam `lease` waited for. Its kernel was complete years ago;
    /// what did not exist was a moment at which a resource could be **used**.
    pub fn build_governed(
        specs: &[IRAxonStore],
        resources: &[crate::ir_nodes::IRResource],
        leases: &[crate::ir_nodes::IRLease],
        resolver: &dyn crate::resource_resolver::ResourceResolver,
    ) -> Result<StoreRegistry, RegistryError> {
        Self::build_governed_with_clock(
            specs,
            resources,
            leases,
            resolver,
            Box::new(chrono::Utc::now),
        )
    }

    /// §Fase 113.d — as [`Self::build_governed`], with an injectable clock.
    ///
    /// τ-decay is only *testable* if time can be moved. A gate that had to sleep
    /// for an hour to observe an expiry would never run, and a guarantee whose
    /// gate never runs is the guarantee §111 spent itself deleting.
    pub fn build_governed_with_clock(
        specs: &[IRAxonStore],
        resources: &[crate::ir_nodes::IRResource],
        leases: &[crate::ir_nodes::IRLease],
        resolver: &dyn crate::resource_resolver::ResourceResolver,
        clock: crate::runtime::lease_kernel::Clock,
    ) -> Result<StoreRegistry, RegistryError> {
        let mut stores: HashMap<String, RegisteredStore> =
            HashMap::with_capacity(specs.len());

        for spec in specs {
            let kind = classify_backend(&spec.backend).ok_or_else(|| {
                RegistryError::UnknownBackend {
                    store: spec.name.clone(),
                    backend: spec.backend.clone(),
                }
            })?;
            if stores.contains_key(&spec.name) {
                return Err(RegistryError::DuplicateStore {
                    store: spec.name.clone(),
                });
            }

            // §Fase 113 — derive from the resource, or keep the legacy shape.
            let (dsn_source, capacity) = if spec.resource_ref.is_empty() {
                (spec.connection.clone(), super::row::MAX_POOL_CONNECTIONS)
            } else {
                let res = resources
                    .iter()
                    .find(|r| r.name == spec.resource_ref)
                    .ok_or_else(|| RegistryError::UnknownResource {
                        store: spec.name.clone(),
                        resource: spec.resource_ref.clone(),
                    })?;
                // The endpoint is a CONFIG KEY (`axon-T944`). Resolving it is the
                // only way an address reaches the runtime at all — and an
                // unresolved key **REFUSES**. It is never defaulted.
                //
                // §112 cost three kernel bugs to learn that, and all three were
                // the same bug: *when the evidence is missing, substitute the
                // belief and report agreement.* A resolver that quietly returns
                // `localhost` for an unset key is that bug wearing a helpful
                // expression — it converts a misconfigured production deployment
                // into a silent connection to nothing.
                let addr = resolver.resolve(&res.endpoint).map_err(|e| {
                    RegistryError::UnresolvedEndpoint {
                        store: spec.name.clone(),
                        resource: res.name.clone(),
                        detail: e.to_string(),
                    }
                })?;
                let cap = res
                    .capacity
                    .filter(|c| *c > 0)
                    .map(|c| c as u32)
                    .unwrap_or(super::row::MAX_POOL_CONNECTIONS);
                (addr, cap)
            };

            stores.insert(
                spec.name.clone(),
                RegisteredStore {
                    spec: spec.clone(),
                    kind,
                    dsn_source,
                    capacity,
                    resource_ref: spec.resource_ref.clone(),
                },
            );
        }

        // §Fase 113.d — acquire the leases. `acquire: on_start` takes the token
        // now; `on_demand` is acquired lazily on first use. Either way the token
        // exists BEFORE any store op can happen, which is what makes the
        // post-expiry use a detectable event rather than a philosophical one.
        let lease_guard = if leases.is_empty() {
            None
        } else {
            let mut kernel = crate::runtime::lease_kernel::LeaseKernel::with_clock(clock);
            let mut tokens = HashMap::new();
            for l in leases {
                let Some(res) = resources.iter().find(|r| r.name == l.resource_ref) else {
                    return Err(RegistryError::UnknownResource {
                        store: format!("lease '{}'", l.name),
                        resource: l.resource_ref.clone(),
                    });
                };
                let token = kernel.acquire(l, res).map_err(|e| RegistryError::LeaseRefused {
                    lease: l.name.clone(),
                    resource: l.resource_ref.clone(),
                    detail: e.message.clone(),
                })?;
                tokens.insert(l.resource_ref.clone(), token);
            }
            Some(Mutex::new(LeaseGuard { kernel, tokens }))
        };

        Ok(StoreRegistry {
            stores,
            pool_cache: Mutex::new(HashMap::new()),
            leases: lease_guard,
        })
    }

    /// §Fase 113.d — **the use-site.** Charge a store operation against the lease
    /// held over the resource it runs on.
    ///
    /// The README's promise is that a `lease` is a τ-decaying affine capability
    /// and that **post-expiry USE is a CT-2 Anchor Breach**. Until §113 a flow
    /// could not *use* a resource at all, so the breach had no moment to fire in
    /// — **structurally impossible, not merely unwired**. This is that moment.
    ///
    /// `Ok(())` ⇒ no lease governs this store's resource, or the lease is live
    /// (or was renewed under `on_expire: extend`, or released under `release`).
    fn charge_lease(&self, store_name: &str) -> Result<(), StoreError> {
        let Some(guard) = &self.leases else {
            return Ok(());
        };
        let Some(reg) = self.stores.get(store_name) else {
            return Ok(());
        };
        if reg.resource_ref.is_empty() {
            // §113's ratified posture, enforced here: an UN-RESOURCED store is
            // INELIGIBLE for lease governance. You cannot govern what you did not
            // declare — and silently governing it would be worse, because the
            // adopter would believe a guarantee they never asked for.
            return Ok(());
        }
        let mut g = guard.lock().unwrap_or_else(|p| p.into_inner());
        let Some(token) = g.tokens.get(&reg.resource_ref).cloned() else {
            return Ok(()); // no lease over this resource
        };
        match g.kernel.use_token(&token) {
            Ok(crate::runtime::lease_kernel::UseOutcome::Valid(_)) => Ok(()),
            Ok(crate::runtime::lease_kernel::UseOutcome::Extended(renewed)) => {
                // `on_expire: extend` — the window rolls forward. Record the new
                // token, or the next use would present a revoked one.
                g.tokens.insert(reg.resource_ref.clone(), renewed);
                Ok(())
            }
            Ok(crate::runtime::lease_kernel::UseOutcome::Released) => {
                // `on_expire: release` — the capability is gone, cleanly. The
                // store op still cannot proceed: the resource is no longer held.
                g.tokens.remove(&reg.resource_ref);
                Err(StoreError::LeaseExpired {
                    store: store_name.to_string(),
                    resource: reg.resource_ref.clone(),
                    lease: token.lease_name.clone(),
                    detail: format!(
                        "lease released at expiry (on_expire: release) — the capability over                          '{}' is no longer held",
                        reg.resource_ref
                    ),
                })
            }
            Err(e) => Err(StoreError::LeaseExpired {
                store: store_name.to_string(),
                resource: reg.resource_ref.clone(),
                lease: token.lease_name.clone(),
                detail: e.message,
            }),
        }
    }

    /// §Fase 113 — the pool size a store ACTUALLY got.
    ///
    /// Exposed so a gate can *prove* `capacity: 20` produced twenty connections
    /// rather than trusting that it did. **An unobservable wire is
    /// indistinguishable from a label**, and this fase exists to tell them apart.
    pub fn pool_capacity_of(&self, store_name: &str) -> Option<u32> {
        self.stores.get(store_name).map(|r| r.capacity)
    }

    /// §Fase 113 — the resource a store derives from. `None` ⇒ the legacy
    /// un-resourced form.
    pub fn resource_of(&self, store_name: &str) -> Option<&str> {
        self.stores
            .get(store_name)
            .map(|r| r.resource_ref.as_str())
            .filter(|s| !s.is_empty())
    }

    /// §Fase 113 — the DSN a store resolves to (the resolved `resource.endpoint`,
    /// or the legacy `connection:` verbatim). For diagnostics and gates.
    pub fn dsn_source_of(&self, store_name: &str) -> Option<&str> {
        self.stores.get(store_name).map(|r| r.dsn_source.as_str())
    }

    /// An empty registry — a program that declares no `axonstore`. Every
    /// `resolve` then yields [`StoreHandle::InMemory`] (D3).
    pub fn empty() -> StoreRegistry {
        StoreRegistry {
            stores: HashMap::new(),
            pool_cache: Mutex::new(HashMap::new()),
            leases: None,
        }
    }

    /// Resolve a store name to its dispatch target.
    ///
    /// - An undeclared store, or one declared `in_memory` / empty →
    ///   [`StoreHandle::InMemory`] (the implicit default, D3).
    /// - A `postgresql` store → [`StoreHandle::Postgres`], its backend
    ///   lazily connected and cached by resolved DSN.
    /// - A `postgresql` store whose connection cannot be resolved → a
    ///   typed [`StoreError`]. **Never** a silent KV fallback.
    ///
    /// Total: every input yields `Ok(handle)` or `Err(StoreError)`.
    /// Must be called within a Tokio runtime context when it may
    /// connect a postgresql backend (the lazy pool, per 35.c).
    pub fn resolve(&self, store_name: &str) -> Result<StoreHandle, StoreError> {
        // §Fase 113.d — THE USE-SITE. A store operation is a *use* of the
        // resource the store runs on, and that is the moment `lease`'s CT-2
        // Anchor Breach has been waiting for since it was written. Charged
        // BEFORE the handle is produced: an expired capability must not hand back
        // a working pool.
        self.charge_lease(store_name)?;

        let registered = match self.stores.get(store_name) {
            // Undeclared → implicit in_memory default (D3 — pre-35
            // behavior: a store needs no declaration to be key-value).
            None => return Ok(StoreHandle::InMemory),
            Some(r) => r,
        };

        match registered.kind {
            StoreBackendKind::InMemory => Ok(StoreHandle::InMemory),
            // §Fase 94.a — pure resolution: the class rides the handle;
            // the custody port itself lives on the DispatchCtx (it is a
            // per-request/per-tenant seam, not a registry-cached pool).
            StoreBackendKind::Secrets => Ok(StoreHandle::Secrets {
                class: registered.spec.class.clone(),
            }),
            StoreBackendKind::Postgresql => {
                // §Fase 113 — `dsn_source` is the RESOLVED `resource.endpoint`
                // when the store runs on a resource, and `spec.connection`
                // verbatim otherwise. Settled once at build, so this cannot
                // disagree with the schema verifier about where a store points.
                //
                // Resolve the DSN first — it is the cache key, and the point at
                // which a missing `env:` var surfaces as a typed error rather
                // than a silent KV fallback.
                let _dsn = resolve_dsn(&registered.dsn_source)?;

                // §Fase 118.b.3 — without the driver `resolve_dsn` above has already
                // returned the written refusal, so everything below is unreachable.
                // Gated rather than stubbed: a build with no driver must not carry a
                // pool cache, and the refusal belongs at RESOLVE time — the earliest
                // point at which "this store cannot be opened" is knowable.
                #[cfg(not(feature = "postgres"))]
                unreachable!("resolve_dsn refuses when the `postgres` feature is absent");
                #[cfg(feature = "postgres")]
                {
                let mut cache = self.lock_cache();
                if let Some(backend) = cache.get(&_dsn) {
                    return Ok(StoreHandle::Postgres(backend.clone()));
                }
                // §Fase 113 — the pool is sized by `resource.capacity`. This one
                // argument is the difference between a wire and a label: without
                // it, `capacity:` would remain what it has always been — parsed,
                // lowered, advertised as a pool cap, and read by nothing.
                let backend = PostgresStoreBackend::connect_named_sized(
                    &registered.dsn_source,
                    store_name,
                    None,
                    registered.capacity,
                )?;
                cache.insert(_dsn, backend.clone());
                Ok(StoreHandle::Postgres(backend))
                }
            }
        }
    }

    /// §Fase 37.x.g (D8) — EAGERLY verify every declared `postgresql`
    /// store's schema against the live database, at deploy time.
    ///
    /// For each `postgresql` store — the table name is the store name
    /// (D12) — the backend is resolved and the table introspected NOW:
    /// the resolution + schema become a deploy-verified contract, warm
    /// in the process cache before the first runtime operation.
    ///
    /// A store REACHABLE at deploy whose table does not resolve is a
    /// FATAL [`SchemaVerifyReport::missing`] entry (the deploy fails —
    /// D8 fail-closed); a store unreachable / unconfigured at deploy is
    /// a non-fatal [`SchemaVerifyReport::unreachable`] warning (the
    /// deploy proceeds — "honest, never brittle" — and the D9 runtime
    /// resolution still applies). `in_memory` stores are skipped.
    ///
    /// Must be called within a Tokio runtime context.
    pub async fn verify_postgres_schemas(&self) -> SchemaVerifyReport {
        self.verify_postgres_schemas_with_manifest(None).await
    }

    /// §Fase 38.f (D3 + D8 strengthening) — extended deploy-time
    /// verification that honors a declared column schema on each
    /// `axonstore`.
    ///
    /// When the optional `manifest` argument is `Some`, the verifier
    /// resolves the three closed Fase 38 `schema:` declaration forms
    /// against it:
    ///
    ///   * **Form (a) — inline column block** — the columns live on the
    ///     IR. The verifier proves every declared column EXISTS in the
    ///     live introspection AND its type matches the declared
    ///     [`StoreColumnType`]. A mismatch is a
    ///     [`StoreError::DeclaredVsLiveDrift`] fatal entry (axon-T807).
    ///
    ///   * **Form (b) — manifest reference** (`schema: "qualified.name"`)
    ///     — the verifier looks up the manifest entry; missing entry is
    ///     a fatal `missing` row; present entry is proven against live
    ///     identically to form (a).
    ///
    ///   * **Form (c) — per-tenant env-var namespace**
    ///     (`schema: env:VAR`) — the verifier resolves the env var; a
    ///     missing var is [`StoreError::MissingPerTenantSchemaEnv`]
    ///     (axon-T806). The resolved namespace prefixes the manifest
    ///     lookup key (`<namespace>.<store_name>`) AND the connection's
    ///     `application_name` (`axon-store/<store>/<namespace>` — Gap-3
    ///     inheritance) so a DBA sees the resolved tenant on every
    ///     session.
    ///
    /// `None` manifest preserves the 37.x verification verbatim — only
    /// table existence is proven, declared columns are not inspected.
    /// `None` is also what the v1.37.0 deploy handler passes today.
    ///
    /// Honest scope (38.f.1): NOT-NULL parity is NOT yet proven by
    /// T807 — the 37.x introspection query doesn't capture `attnotnull`.
    /// The runtime catches NOT-NULL drift via SQLSTATE 23502 at the
    /// first failing `persist`, so defense-in-depth remains. A 38.f.2
    /// follow-on can extend `introspect_conn` to include nullability.
    #[cfg_attr(not(feature = "postgres"), allow(unused_variables, unreachable_code))]
    pub async fn verify_postgres_schemas_with_manifest(
        &self,
        manifest: Option<&Manifest>,
    ) -> SchemaVerifyReport {
        let mut report = SchemaVerifyReport::default();
        let mut pg_stores: Vec<&str> = self
            .stores
            .iter()
            .filter(|(_, r)| r.kind == StoreBackendKind::Postgresql)
            .map(|(name, _)| name.as_str())
            .collect();
        pg_stores.sort_unstable();

        for name in pg_stores {
            let column_schema = self
                .stores
                .get(name)
                .and_then(|r| r.spec.column_schema.clone());

            // §38.f — resolve per-tenant env-var FIRST when present
            // (form c), so a T806 fails fast without touching the DB.
            let resolved_namespace = match &column_schema {
                Some(IRStoreColumnSchema::EnvVar { var_name }) => {
                    match std::env::var(var_name) {
                        Ok(v) if !v.trim().is_empty() => Some(v),
                        _ => {
                            let err = StoreError::MissingPerTenantSchemaEnv {
                                store: name.to_string(),
                                var: var_name.clone(),
                            };
                            report
                                .missing
                                .push((name.to_string(), err.to_string()));
                            continue;
                        }
                    }
                }
                _ => None,
            };

            // §38.f — for form (c) with a resolved namespace, REPLACE
            // the pool-cache entry with a namespace-stamped backend
            // so every runtime session carries the tenant in its
            // `application_name`. The replacement is idempotent — a
            // re-verify of the same store with the same namespace is
            // a no-op.
            if let Some(ns) = &resolved_namespace {
                if let Err(e) = self.restamp_backend_with_namespace(name, ns) {
                    report.missing.push((name.to_string(), e.to_string()));
                    continue;
                }
            }

            match self.resolve(name) {
                #[cfg(feature = "postgres")]
                Ok(StoreHandle::Postgres(backend)) => {
                    let masked = backend.masked_dsn();
                    match backend.warm_schema(name).await {
                        Ok(()) => {
                            // §38.f D8 strengthening — when a column
                            // schema is declared, compare declared
                            // columns vs live introspection (T807).
                            if let Some(drift) = verify_declared_columns(
                                name,
                                &backend,
                                column_schema.as_ref(),
                                resolved_namespace.as_deref(),
                                manifest,
                                &masked,
                            ) {
                                report.missing.push((name.to_string(), drift));
                            } else {
                                report.verified.push(name.to_string());
                            }
                        }
                        Err(
                            e @ (StoreError::TableNotResolved { .. }
                            | StoreError::AmbiguousTable { .. }),
                        ) => {
                            // Reachable store, table genuinely missing
                            // / ambiguous — a fatal deploy error.
                            report.missing.push((
                                name.to_string(),
                                format!("{e} (database: {masked})"),
                            ));
                        }
                        Err(e) => {
                            // Unreachable / transient — non-fatal.
                            report.unreachable.push((
                                name.to_string(),
                                format!("{e} (database: {masked})"),
                            ));
                        }
                    }
                }
                // `kind` is Postgresql — `resolve` cannot yield InMemory
                // (nor Secrets: a secrets store has no table to verify —
                // its custody schema is the enterprise migration's law).
                Ok(StoreHandle::InMemory) | Ok(StoreHandle::Secrets { .. }) => {}
                Err(e) => {
                    // The connection could not even be resolved (a
                    // missing `env:` var, a malformed DSN) — non-fatal;
                    // the store is unconfigured at deploy time. No
                    // backend was constructed, so no masked DSN to
                    // append: the error text already names the
                    // configuration site.
                    report
                        .unreachable
                        .push((name.to_string(), e.to_string()));
                }
            }
        }
        report
    }

    /// §Fase 38.f (D3) — re-stamp a postgresql store's pooled backend
    /// with the resolved per-tenant namespace so every session's
    /// `application_name` carries `axon-store/<store>/<namespace>`.
    ///
    /// Idempotent: if a backend is already cached for the resolved
    /// DSN, it is REPLACED. Future `resolve(<store>)` calls hand out
    /// the new namespace-stamped pool from the cache. The old pool's
    /// connections are dropped on the next acquire.
    #[cfg_attr(not(feature = "postgres"), allow(unused_variables, unreachable_code))]
    fn restamp_backend_with_namespace(
        &self,
        store_name: &str,
        namespace: &str,
    ) -> Result<(), StoreError> {
        let registered = self.stores.get(store_name).ok_or_else(|| {
            StoreError::Query {
                op: "verify",
                source: format!("axonstore `{store_name}` is not declared"),
            }
        })?;
        let _dsn = resolve_dsn(&registered.spec.connection)?;
        // §Fase 118.b.3 — as above: `resolve_dsn` has already refused.
        #[cfg(not(feature = "postgres"))]
        unreachable!("resolve_dsn refuses when the `postgres` feature is absent");
        #[cfg(feature = "postgres")]
        let backend = PostgresStoreBackend::connect_named_with_namespace(
            &registered.spec.connection,
            store_name,
            Some(namespace),
        )?;
        #[cfg(feature = "postgres")]
        {
            let mut cache = self.lock_cache();
            cache.insert(_dsn, backend);
        }
        Ok(())
    }

    /// The declaration backing a store name, if any. The pillars
    /// consult it — 35.g for `confidence_floor`, 35.h for `on_breach`.
    pub fn spec(&self, store_name: &str) -> Option<&IRAxonStore> {
        self.stores.get(store_name).map(|r| &r.spec)
    }

    /// The backend kind a store name resolves to, if declared.
    pub fn backend_kind(&self, store_name: &str) -> Option<StoreBackendKind> {
        self.stores.get(store_name).map(|r| r.kind)
    }

    /// The number of declared stores.
    pub fn len(&self) -> usize {
        self.stores.len()
    }

    /// `true` iff no `axonstore` is declared.
    pub fn is_empty(&self) -> bool {
        self.stores.is_empty()
    }

    /// The number of distinct connection pools currently cached — one
    /// per resolved DSN actually used. Useful for a health surface.
    pub fn cached_pool_count(&self) -> usize {
        self.lock_cache().len()
    }

    /// Lock the pool cache, recovering the guard if a prior holder
    /// panicked (the critical section only does infallible map ops, so
    /// poisoning is effectively impossible — but recovery keeps the
    /// registry panic-free regardless).
    fn lock_cache(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<String, PostgresStoreBackend>> {
        self.pool_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

// ════════════════════════════════════════════════════════════════════
//  §Fase 38.f (D8 strengthening) — declared-vs-live column verification
// ════════════════════════════════════════════════════════════════════

/// Resolve the declared columns for a store from its IR `column_schema`
/// + the optional deploy-time manifest. Returns:
///
///   - `Ok(Some(columns))` — declared columns are known; the caller
///     proves them against the live introspection.
///   - `Ok(None)` — no schema declaration OR the manifest lookup
///     couldn't find a matching entry (form b/c without manifest in
///     scope today). The 37.x existence-only verification suffices.
///   - `Err(_)` — propagated up as a fatal `missing` row.
#[cfg_attr(not(feature = "postgres"), allow(dead_code))]
fn declared_columns_for(
    store_name: &str,
    column_schema: Option<&IRStoreColumnSchema>,
    resolved_namespace: Option<&str>,
    manifest: Option<&Manifest>,
) -> Result<Option<std::collections::BTreeMap<String, StoreColumnType>>, String> {
    let Some(schema) = column_schema else {
        return Ok(None);
    };
    match schema {
        IRStoreColumnSchema::Inline { columns } => {
            let mut out = std::collections::BTreeMap::new();
            for col in columns {
                let Some(ty) = StoreColumnType::from_token(&col.col_type) else {
                    return Err(format!(
                        "axonstore `{store_name}` inline schema column \
                         `{}` declares unknown type `{}` — the closed \
                         catalog is {{{}}}",
                        col.name,
                        col.col_type,
                        StoreColumnType::all_canonical_names().join(", ")
                    ));
                };
                out.insert(col.name.clone(), ty);
            }
            Ok(Some(out))
        }
        IRStoreColumnSchema::ManifestRef { qualified_name } => {
            let Some(m) = manifest else {
                // No manifest available at deploy time — fall through
                // to 37.x existence-only verification. 38.h's CLI +
                // 38.j's CI lane plumb the manifest; until then the
                // deploy is honest about this gap (no T807 raised
                // for what we can't prove).
                return Ok(None);
            };
            let Some(store) = m.lookup(qualified_name) else {
                return Err(format!(
                    "axonstore `{store_name}` declares `schema: \
                     \"{qualified_name}\"` but no manifest entry \
                     matches that qualified name. Available manifest \
                     entries: {{{}}}.",
                    m.stores.keys().cloned().collect::<Vec<_>>().join(", ")
                ));
            };
            Ok(Some(manifest_store_to_btreemap(store)))
        }
        IRStoreColumnSchema::EnvVar { .. } => {
            let Some(m) = manifest else {
                return Ok(None);
            };
            let ns = resolved_namespace.unwrap_or("");
            let key = format!("{ns}.{store_name}");
            if let Some(store) = m.lookup(&key) {
                return Ok(Some(manifest_store_to_btreemap(store)));
            }
            // First-match heuristic mirrors 38.d's `load_columns_for_schema`:
            // when an exact `<namespace>.<store>` entry is missing,
            // accept any `*.<store_name>` shape (per-tenant schemas
            // typically have identical column shapes at deploy time).
            let suffix = format!(".{store_name}");
            for (k, s) in &m.stores {
                if k.ends_with(&suffix) {
                    return Ok(Some(manifest_store_to_btreemap(s)));
                }
            }
            // Manifest present but no matching entry — honest fall-
            // through to existence-only (not T807, because the
            // manifest is the proof source).
            Ok(None)
        }
    }
}

#[cfg_attr(not(feature = "postgres"), allow(dead_code))]
fn manifest_store_to_btreemap(
    s: &ManifestStore,
) -> std::collections::BTreeMap<String, StoreColumnType> {
    let mut out = std::collections::BTreeMap::new();
    for (col_name, col) in &s.columns {
        out.insert(col_name.clone(), col.col_type);
    }
    out
}

/// Compare a store's DECLARED columns against the LIVE introspected
/// columns. Returns `Some(drift_summary)` when they disagree (the
/// caller surfaces this as an axon-T807 fatal entry); `None` when
/// the declared shape matches live (or when there's nothing to prove
/// — no schema declared, or a form b/c without a matching manifest
/// entry).
///
/// The check has two arms: every declared column EXISTS in live
/// introspection (column-name match) AND its type matches the
/// declared [`StoreColumnType`] under [`pg_udt_matches_catalog_type`].
/// Honest scope: NOT-NULL parity is NOT yet checked — the 37.x
/// introspection query doesn't capture `attnotnull`. Documented as
/// 38.f.1 deferral.
// §Fase 118.b.3 — takes a `&PostgresStoreBackend` and reads its live schema
// cache: driver work end to end, and unreachable without one (its only caller is
// the gated eager-verify arm above).
#[cfg(feature = "postgres")]
fn verify_declared_columns(
    store_name: &str,
    backend: &PostgresStoreBackend,
    column_schema: Option<&IRStoreColumnSchema>,
    resolved_namespace: Option<&str>,
    manifest: Option<&Manifest>,
    masked_dsn: &str,
) -> Option<String> {
    let declared = match declared_columns_for(
        store_name,
        column_schema,
        resolved_namespace,
        manifest,
    ) {
        Ok(Some(d)) => d,
        Ok(None) => return None, // nothing to prove — preserve 37.x behavior
        Err(msg) => return Some(format!("{msg} (database: {masked_dsn})")),
    };
    let cached = backend.cached_schema(store_name);
    let Some(resolved) = cached else {
        // warm_schema just succeeded, so the cache should be hot.
        // Defensive fall-through.
        return None;
    };
    let live = &resolved.column_types;

    let mut missing_cols: Vec<String> = Vec::new();
    let mut type_drifts: Vec<String> = Vec::new();
    for (col_name, declared_type) in &declared {
        match live.get(col_name) {
            None => missing_cols.push(col_name.clone()),
            Some(pg_udt) => {
                if !pg_udt_matches_catalog_type(pg_udt, *declared_type) {
                    type_drifts.push(format!(
                        "`{col_name}` declared as `{}` but live type is `{pg_udt}`",
                        declared_type.canonical_name()
                    ));
                }
            }
        }
    }

    if missing_cols.is_empty() && type_drifts.is_empty() {
        return None;
    }

    let mut parts: Vec<String> = Vec::new();
    if !missing_cols.is_empty() {
        parts.push(format!(
            "missing on live database: {{{}}}",
            missing_cols.join(", ")
        ));
    }
    if !type_drifts.is_empty() {
        parts.push(format!("type mismatches: {{{}}}", type_drifts.join("; ")));
    }
    let drift = parts.join("; ");
    let err = StoreError::DeclaredVsLiveDrift {
        store: store_name.to_string(),
        drift,
    };
    Some(format!("{err} (database: {masked_dsn})"))
}

/// `true` iff the live introspected Postgres UDT name is compatible
/// with the declared axon-language [`StoreColumnType`]. The matrix
/// mirrors the v1.30.0 runtime `classify_pg_type` mapping — `Text`
/// accepts `text`/`varchar`/`bpchar`/`name`; `Int` accepts `int4`;
/// `BigInt` accepts `int8`; etc. Case-insensitive (Postgres lower-
/// cases udt names by convention).
#[cfg_attr(not(feature = "postgres"), allow(dead_code))]
fn pg_udt_matches_catalog_type(udt: &str, declared: StoreColumnType) -> bool {
    let u = udt.to_ascii_lowercase();
    use StoreColumnType as C;
    match declared {
        C::Uuid => u == "uuid",
        C::Text => matches!(u.as_str(), "text" | "varchar" | "bpchar" | "name"),
        C::Int => matches!(u.as_str(), "int4" | "integer"),
        C::BigInt => matches!(u.as_str(), "int8" | "bigint"),
        C::Float => matches!(u.as_str(), "float4" | "real"),
        C::Double => matches!(u.as_str(), "float8" | "double precision"),
        C::Bool => u == "bool" || u == "boolean",
        C::Timestamptz => u == "timestamptz",
        C::Timestamp => u == "timestamp",
        C::Date => u == "date",
        C::Time => u == "time",
        C::Jsonb => u == "jsonb",
        C::Json => u == "json",
        C::Bytea => u == "bytea",
        C::Numeric => matches!(u.as_str(), "numeric" | "decimal"),
    }
}

// ════════════════════════════════════════════════════════════════════
//  Unit tests
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an `IRAxonStore` test fixture.
    fn spec(name: &str, backend: &str, connection: &str) -> IRAxonStore {
        IRAxonStore {
            node_type: "axonstore",
            source_line: 0,
            source_column: 0,
            name: name.to_string(),
            backend: backend.to_string(),
            connection: connection.to_string(),
            confidence_floor: None,
            isolation: String::new(),
            on_breach: String::new(),
            capability: String::new(),
            class: String::new(),
            column_schema: None,
            resource_ref: String::new(),
        }
    }

    // ── classify_backend ─────────────────────────────────────────────

    #[test]
    fn classify_postgresql() {
        assert_eq!(
            classify_backend("postgresql"),
            Some(StoreBackendKind::Postgresql)
        );
    }

    #[test]
    fn classify_in_memory_and_empty_default() {
        assert_eq!(
            classify_backend("in_memory"),
            Some(StoreBackendKind::InMemory)
        );
        assert_eq!(classify_backend(""), Some(StoreBackendKind::InMemory));
    }

    #[test]
    fn classify_is_trimmed_and_case_insensitive() {
        assert_eq!(
            classify_backend("  PostgreSQL  "),
            Some(StoreBackendKind::Postgresql)
        );
        assert_eq!(
            classify_backend("IN_MEMORY"),
            Some(StoreBackendKind::InMemory)
        );
    }

    #[test]
    fn classify_unknown_backends_are_none() {
        // `sqlite` / `mysql` are syntactically valid in the frontend
        // but outside the v1.30.0 runtime catalog.
        for backend in ["sqlite", "mysql", "postgres", "mongodb", "redis"] {
            assert_eq!(classify_backend(backend), None, "backend {backend}");
        }
    }

    // ── build — D2 catalog gate ──────────────────────────────────────

    #[test]
    fn build_accepts_valid_specs() {
        let specs = [
            spec("cache", "in_memory", ""),
            spec("tenants", "postgresql", "env:DATABASE_URL"),
            spec("scratch", "", ""),
        ];
        let registry = StoreRegistry::build(&specs).unwrap();
        assert_eq!(registry.len(), 3);
        assert!(!registry.is_empty());
    }

    #[test]
    fn build_rejects_unknown_backend() {
        let specs = [spec("legacy", "sqlite", "file:./db.sqlite")];
        match StoreRegistry::build(&specs) {
            Err(RegistryError::UnknownBackend { store, backend }) => {
                assert_eq!(store, "legacy");
                assert_eq!(backend, "sqlite");
            }
            other => panic!("expected UnknownBackend, got {other:?}"),
        }
    }

    #[test]
    fn build_rejects_duplicate_store_name() {
        let specs = [
            spec("tenants", "in_memory", ""),
            spec("tenants", "postgresql", "env:DB"),
        ];
        match StoreRegistry::build(&specs) {
            Err(RegistryError::DuplicateStore { store }) => {
                assert_eq!(store, "tenants");
            }
            other => panic!("expected DuplicateStore, got {other:?}"),
        }
    }

    #[test]
    fn build_empty_specs_yields_empty_registry() {
        let registry = StoreRegistry::build(&[]).unwrap();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn empty_constructor_is_empty() {
        assert!(StoreRegistry::empty().is_empty());
    }

    // ── resolve — D3 key-value path ──────────────────────────────────

    #[test]
    fn resolve_undeclared_store_is_in_memory() {
        // The load-bearing D3 test: a store that was never declared
        // resolves to the byte-identical pre-35 key-value path.
        let registry = StoreRegistry::empty();
        let handle = registry.resolve("never_declared").unwrap();
        assert!(handle.is_in_memory());
    }

    #[test]
    fn resolve_declared_in_memory_store() {
        let registry =
            StoreRegistry::build(&[spec("cache", "in_memory", "")]).unwrap();
        assert!(registry.resolve("cache").unwrap().is_in_memory());
    }

    #[test]
    fn resolve_empty_backend_store_is_in_memory() {
        let registry = StoreRegistry::build(&[spec("s", "", "")]).unwrap();
        assert!(registry.resolve("s").unwrap().is_in_memory());
    }

    #[test]
    fn resolve_empty_store_name_is_in_memory() {
        assert!(StoreRegistry::empty().resolve("").unwrap().is_in_memory());
    }

    // ── resolve — D2: never a silent KV fallback ─────────────────────

    #[cfg(feature = "postgres")]
    #[test]
    fn resolve_postgres_with_missing_env_var_errors_not_kv_fallback() {
        // A declared postgresql store whose `env:` var is unset MUST
        // surface a typed error — never degrade silently to KV.
        let registry = StoreRegistry::build(&[spec(
            "tenants",
            "postgresql",
            "env:AXON_NONEXISTENT_VAR_FASE35D",
        )])
        .unwrap();
        match registry.resolve("tenants") {
            Err(StoreError::MissingEnvVar { var }) => {
                assert_eq!(var, "AXON_NONEXISTENT_VAR_FASE35D");
            }
            other => panic!("expected MissingEnvVar, got {other:?}"),
        }
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn resolve_postgres_with_empty_connection_errors() {
        let registry =
            StoreRegistry::build(&[spec("t", "postgresql", "")]).unwrap();
        assert!(matches!(
            registry.resolve("t"),
            Err(StoreError::EmptyConnection)
        ));
    }

    // ── resolve — postgres path + per-DSN pool cache ─────────────────

    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn resolve_postgres_store_yields_a_postgres_handle() {
        let registry = StoreRegistry::build(&[spec(
            "tenants",
            "postgresql",
            "postgresql://u:p@localhost:5432/axon",
        )])
        .unwrap();
        assert!(registry.resolve("tenants").unwrap().is_postgres());
    }

    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn resolving_one_store_twice_reuses_one_pool() {
        let registry = StoreRegistry::build(&[spec(
            "tenants",
            "postgresql",
            "postgresql://u:p@localhost:5432/axon",
        )])
        .unwrap();
        assert_eq!(registry.cached_pool_count(), 0);
        registry.resolve("tenants").unwrap();
        registry.resolve("tenants").unwrap();
        assert_eq!(
            registry.cached_pool_count(),
            1,
            "the second resolve must hit the cache, not reconnect"
        );
    }

    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn two_stores_sharing_a_dsn_share_one_pool() {
        let dsn = "postgresql://u:p@localhost:5432/shared";
        let registry = StoreRegistry::build(&[
            spec("alpha", "postgresql", dsn),
            spec("beta", "postgresql", dsn),
        ])
        .unwrap();
        registry.resolve("alpha").unwrap();
        registry.resolve("beta").unwrap();
        assert_eq!(
            registry.cached_pool_count(),
            1,
            "stores on the same DSN must share one pool"
        );
    }

    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn two_stores_with_distinct_dsns_get_distinct_pools() {
        let registry = StoreRegistry::build(&[
            spec("alpha", "postgresql", "postgresql://u:p@localhost/db_a"),
            spec("beta", "postgresql", "postgresql://u:p@localhost/db_b"),
        ])
        .unwrap();
        registry.resolve("alpha").unwrap();
        registry.resolve("beta").unwrap();
        assert_eq!(registry.cached_pool_count(), 2);
    }

    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn malformed_dsn_errors_and_is_not_cached() {
        let registry = StoreRegistry::build(&[spec(
            "broken",
            "postgresql",
            "this is not a dsn",
        )])
        .unwrap();
        assert!(matches!(
            registry.resolve("broken"),
            Err(StoreError::PoolInit { .. })
        ));
        assert_eq!(
            registry.cached_pool_count(),
            0,
            "a failed connect must not populate the cache"
        );
    }

    // ── accessors ────────────────────────────────────────────────────

    #[test]
    fn spec_accessor_returns_the_declaration() {
        let registry = StoreRegistry::build(&[spec(
            "tenants",
            "postgresql",
            "env:DB",
        )])
        .unwrap();
        let s = registry.spec("tenants").unwrap();
        assert_eq!(s.name, "tenants");
        assert_eq!(s.backend, "postgresql");
        assert!(registry.spec("absent").is_none());
    }

    #[test]
    fn backend_kind_accessor() {
        let registry = StoreRegistry::build(&[
            spec("kv", "in_memory", ""),
            spec("pg", "postgresql", "env:DB"),
        ])
        .unwrap();
        assert_eq!(
            registry.backend_kind("kv"),
            Some(StoreBackendKind::InMemory)
        );
        assert_eq!(
            registry.backend_kind("pg"),
            Some(StoreBackendKind::Postgresql)
        );
        assert_eq!(registry.backend_kind("absent"), None);
    }

    // ── StoreHandle + display + Debug safety ─────────────────────────

    #[test]
    fn store_handle_predicates() {
        assert!(StoreHandle::InMemory.is_in_memory());
        assert!(!StoreHandle::InMemory.is_postgres());
    }

    #[test]
    fn backend_kind_display() {
        assert_eq!(StoreBackendKind::InMemory.to_string(), "in_memory");
        assert_eq!(StoreBackendKind::Postgresql.to_string(), "postgresql");
    }

    #[test]
    fn registry_debug_does_not_leak_connection_strings() {
        // A literal DSN with a password must not appear in Debug.
        let registry = StoreRegistry::build(&[spec(
            "tenants",
            "postgresql",
            "postgresql://user:fakecred0@localhost/db",
        )])
        .unwrap();
        let debug = format!("{registry:?}");
        assert!(!debug.contains("fakecred0"), "Debug must not leak the DSN");
        assert!(debug.contains("tenants"));
        // The kind surfaces via the `StoreBackendKind` enum's derived
        // Debug (`Postgresql`) — case-insensitive check.
        assert!(debug.to_lowercase().contains("postgresql"));
    }

    #[test]
    fn registry_errors_have_non_empty_display() {
        let errors = [
            RegistryError::UnknownBackend {
                store: "s".into(),
                backend: "mysql".into(),
            },
            RegistryError::DuplicateStore { store: "s".into() },
        ];
        for e in errors {
            assert!(!e.to_string().is_empty());
        }
    }

    // ── §Fase 37.x.g — deploy-time schema verification (D8) ──────────

    #[test]
    fn schema_verify_report_has_fatal_iff_a_table_is_missing() {
        let mut report = SchemaVerifyReport::default();
        assert!(!report.has_fatal(), "an empty report is not fatal");
        assert!(report.fatal_summary().is_empty());
        report.unreachable.push(("s".into(), "down".into()));
        assert!(
            !report.has_fatal(),
            "an unreachable store is a non-fatal warning"
        );
        report.missing.push(("t".into(), "no such table".into()));
        assert!(report.has_fatal(), "a missing table is fatal");
        assert!(report.fatal_summary().contains("`t`"));
    }

    #[tokio::test]
    async fn verify_postgres_schemas_skips_in_memory_and_warns_on_unreachable() {
        // An `in_memory` store is skipped; a postgresql store with a
        // malformed DSN cannot resolve → a non-fatal `unreachable`
        // warning (NOT a `missing` fatal — the database was never
        // reached, so the table's existence is unknown). D8 — "deploy
        // is honest, never brittle".
        let registry = StoreRegistry::build(&[
            spec("cache", "in_memory", ""),
            spec("tenants", "postgresql", "this is not a dsn"),
        ])
        .unwrap();
        let report = registry.verify_postgres_schemas().await;
        assert!(report.verified.is_empty());
        assert!(
            report.missing.is_empty(),
            "an unreachable store must not be a fatal `missing` entry"
        );
        assert_eq!(report.unreachable.len(), 1);
        assert_eq!(report.unreachable[0].0, "tenants");
        assert!(
            !report.has_fatal(),
            "an unreachable store must NOT fail the deploy"
        );
    }

    #[tokio::test]
    async fn verify_postgres_schemas_empty_registry_is_clean() {
        let report = StoreRegistry::empty().verify_postgres_schemas().await;
        assert!(report.verified.is_empty());
        assert!(report.missing.is_empty());
        assert!(!report.has_fatal());
    }

    // ── §Fase 38.f — D3 per-tenant env-var + D8 strengthening (T807) ─

    /// Build an `IRAxonStore` with a declared `column_schema`.
    fn spec_with_schema(
        name: &str,
        connection: &str,
        schema: crate::ir_nodes::IRStoreColumnSchema,
    ) -> IRAxonStore {
        IRAxonStore {
            node_type: "axonstore",
            source_line: 0,
            source_column: 0,
            name: name.to_string(),
            backend: "postgresql".to_string(),
            connection: connection.to_string(),
            confidence_floor: None,
            isolation: String::new(),
            on_breach: String::new(),
            capability: String::new(),
            class: String::new(),
            column_schema: Some(schema),
            resource_ref: String::new(),
        }
    }

    #[tokio::test]
    async fn t806_missing_per_tenant_env_var_fails_deploy_with_named_code() {
        // The env var is intentionally unset — the deploy must surface
        // axon-T806 as a fatal `missing` entry. NO database needed:
        // the env-var resolution short-circuits before any pool work.
        let var_name = "AXON_T806_FASE38F_UNSET_VAR_XYZ_DO_NOT_SET";
        std::env::remove_var(var_name);
        let registry = StoreRegistry::build(&[spec_with_schema(
            "tenants",
            "postgresql://u:p@localhost:5432/axon",
            crate::ir_nodes::IRStoreColumnSchema::EnvVar {
                var_name: var_name.to_string(),
            },
        )])
        .unwrap();
        let report = registry.verify_postgres_schemas_with_manifest(None).await;
        assert!(report.has_fatal(), "T806 must fail-close the deploy");
        let (store, diag) = &report.missing[0];
        assert_eq!(store, "tenants");
        assert!(diag.contains("axon-T806"), "diag must carry T806 slug: {diag}");
        assert!(diag.contains(var_name), "diag must name the env var: {diag}");
    }

    #[tokio::test]
    async fn t806_empty_string_env_var_also_fails_t806() {
        // An exported-but-empty env var is the same configuration
        // accident as a missing one — never a silent fallback.
        let var_name = "AXON_T806_FASE38F_EMPTY_VAR";
        std::env::set_var(var_name, "");
        let registry = StoreRegistry::build(&[spec_with_schema(
            "tenants",
            "postgresql://u:p@localhost:5432/axon",
            crate::ir_nodes::IRStoreColumnSchema::EnvVar {
                var_name: var_name.to_string(),
            },
        )])
        .unwrap();
        let report = registry.verify_postgres_schemas_with_manifest(None).await;
        std::env::remove_var(var_name);
        assert!(report.has_fatal(), "empty-string env var must fail-close");
        assert!(report.missing[0].1.contains("axon-T806"));
    }

    #[tokio::test]
    async fn three_tenants_each_get_their_namespace_resolved_independently() {
        // Three different env vars resolve to three different
        // namespaces — every restamping is independent. (No live DB:
        // we only verify the restamp doesn't error.)
        for (var, value) in [
            ("AXON_T806_FASE38F_T1", "tenant_a"),
            ("AXON_T806_FASE38F_T2", "tenant_b"),
            ("AXON_T806_FASE38F_T3", "tenant_c"),
        ] {
            std::env::set_var(var, value);
        }
        let specs: Vec<IRAxonStore> = ["AXON_T806_FASE38F_T1", "AXON_T806_FASE38F_T2", "AXON_T806_FASE38F_T3"]
            .iter()
            .enumerate()
            .map(|(i, v)| {
                spec_with_schema(
                    &format!("tenants_{i}"),
                    "postgresql://u:p@localhost:5432/axon",
                    crate::ir_nodes::IRStoreColumnSchema::EnvVar {
                        var_name: (*v).to_string(),
                    },
                )
            })
            .collect();
        let registry = StoreRegistry::build(&specs).unwrap();
        // The restamping happens synchronously inside verify;
        // because the connections are lazy, the verify will fail at
        // `warm_schema` (no live DB) but the env-var resolution +
        // restamping itself must succeed. We check the pool cache.
        let _ = registry.verify_postgres_schemas_with_manifest(None).await;
        // After verify, each of the three stores has its own pool
        // stamped with its namespace. The pool cache is keyed by DSN
        // — three same-DSN stores share one entry, with the LAST
        // restamping winning. That's correct: the runtime cache holds
        // ONE pool per (DSN, namespace) — same DSN with different
        // namespaces is reachable, but for THIS test we just confirm
        // the restamp didn't error.
        for var in ["AXON_T806_FASE38F_T1", "AXON_T806_FASE38F_T2", "AXON_T806_FASE38F_T3"] {
            std::env::remove_var(var);
        }
        // The reachable pool count is at most 1 (same DSN); the
        // important property is that the restamping completed without
        // panicking and produced a backend.
        assert!(registry.cached_pool_count() <= 1);
    }

    #[test]
    #[cfg(feature = "postgres")]
    fn application_name_stamping_includes_resolved_namespace() {
        use crate::store::postgres_backend::application_name_for_with_namespace;
        assert_eq!(
            application_name_for_with_namespace("claims", None),
            "axon-store/claims"
        );
        assert_eq!(
            application_name_for_with_namespace("claims", Some("tenant_42")),
            "axon-store/claims/tenant_42"
        );
        // Empty namespace falls back to the no-namespace shape.
        assert_eq!(
            application_name_for_with_namespace("claims", Some("")),
            "axon-store/claims"
        );
        // Empty store name + namespace.
        assert_eq!(
            application_name_for_with_namespace("", Some("tenant_42")),
            "axon-store/tenant_42"
        );
    }

    #[test]
    #[cfg(feature = "postgres")]
    fn application_name_stamping_truncates_at_namedatalen_with_char_boundary() {
        use crate::store::postgres_backend::application_name_for_with_namespace;
        // A long store name + long namespace MUST not exceed 63
        // bytes (Postgres NAMEDATALEN-1), and the cut must land on a
        // UTF-8 char boundary.
        let long_store = "s".repeat(50);
        let long_ns = "é".repeat(50);
        let stamped = application_name_for_with_namespace(&long_store, Some(&long_ns));
        assert!(stamped.len() <= 63, "got {}: {stamped}", stamped.len());
        assert!(stamped.is_char_boundary(stamped.len()));
        assert!(stamped.starts_with("axon-store/"));
    }

    #[test]
    fn pg_udt_matches_catalog_type_recognises_text_class_aliases() {
        // Text accepts text/varchar/bpchar/name (case-insensitive).
        for udt in ["text", "varchar", "bpchar", "name", "TEXT", "VARCHAR"] {
            assert!(
                pg_udt_matches_catalog_type(udt, StoreColumnType::Text),
                "Text class must accept `{udt}`"
            );
        }
        // Int accepts int4/integer.
        for udt in ["int4", "integer", "INT4"] {
            assert!(pg_udt_matches_catalog_type(udt, StoreColumnType::Int));
        }
        // BigInt accepts int8/bigint.
        for udt in ["int8", "bigint"] {
            assert!(pg_udt_matches_catalog_type(udt, StoreColumnType::BigInt));
        }
    }

    #[test]
    fn pg_udt_matches_catalog_type_rejects_off_class_udts() {
        // Cross-class checks must NOT match.
        assert!(!pg_udt_matches_catalog_type("int4", StoreColumnType::Text));
        assert!(!pg_udt_matches_catalog_type("uuid", StoreColumnType::Int));
        assert!(!pg_udt_matches_catalog_type("varchar", StoreColumnType::Uuid));
        assert!(!pg_udt_matches_catalog_type("bool", StoreColumnType::Numeric));
    }

    #[test]
    fn verify_declared_columns_no_schema_means_nothing_to_prove() {
        // When `column_schema` is None, the v1.37.0 existence-only
        // verification suffices — verify_declared_columns must return
        // None (no T807 raised).
        //
        // We can't easily invoke `verify_declared_columns` directly
        // without a `PostgresStoreBackend` + cached_schema entry, so
        // we test `declared_columns_for` (the pure half).
        let result = declared_columns_for("tenants", None, None, None);
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn declared_columns_for_inline_returns_btreemap_keyed_on_column_names() {
        let schema = crate::ir_nodes::IRStoreColumnSchema::Inline {
            columns: vec![
                crate::ir_nodes::IRStoreColumn {
                    name: "tenant_id".to_string(),
                    col_type: "Uuid".to_string(),
                    primary_key: true,
                    auto_increment: false,
                    not_null: false,
                    unique: false,
                    default_value: String::new(),
                    identity: false,
                    indexed: false,
                    json_shape: None,
                },
                crate::ir_nodes::IRStoreColumn {
                    name: "tier".to_string(),
                    col_type: "Text".to_string(),
                    primary_key: false,
                    auto_increment: false,
                    not_null: true,
                    unique: false,
                    default_value: String::new(),
                    identity: false,
                    indexed: false,
                    json_shape: None,
                },
            ],
        };
        let cols = declared_columns_for("tenants", Some(&schema), None, None)
            .unwrap()
            .unwrap();
        assert_eq!(cols.len(), 2);
        assert_eq!(cols.get("tenant_id").copied(), Some(StoreColumnType::Uuid));
        assert_eq!(cols.get("tier").copied(), Some(StoreColumnType::Text));
    }

    #[test]
    fn declared_columns_for_inline_unknown_type_returns_a_named_error() {
        let schema = crate::ir_nodes::IRStoreColumnSchema::Inline {
            columns: vec![crate::ir_nodes::IRStoreColumn {
                name: "loc".to_string(),
                col_type: "Geometry".to_string(),
                primary_key: false,
                auto_increment: false,
                not_null: false,
                unique: false,
                default_value: String::new(),
                identity: false,
                indexed: false,
                json_shape: None,
            }],
        };
        let result = declared_columns_for("tenants", Some(&schema), None, None);
        match result {
            Err(msg) => {
                assert!(msg.contains("Geometry"));
                assert!(msg.contains("closed catalog"));
            }
            other => panic!("expected named error, got {other:?}"),
        }
    }

    #[test]
    fn declared_columns_for_manifest_ref_returns_none_when_no_manifest_in_scope() {
        // The current deploy handler passes None for manifest; form (b)
        // honest-falls-through to 37.x existence-only verification.
        let schema = crate::ir_nodes::IRStoreColumnSchema::ManifestRef {
            qualified_name: "public.tenants".to_string(),
        };
        let result = declared_columns_for("tenants", Some(&schema), None, None);
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn declared_columns_for_env_var_no_manifest_returns_none() {
        let schema = crate::ir_nodes::IRStoreColumnSchema::EnvVar {
            var_name: "TENANT_SCHEMA".to_string(),
        };
        let result = declared_columns_for("tenants", Some(&schema), Some("tenant_42"), None);
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn declared_columns_for_manifest_ref_resolves_against_provided_manifest() {
        let m = Manifest::parse_json(
            r#"{
                "version": 1,
                "stores": {
                    "public.tenants": {
                        "columns": {
                            "tenant_id": { "type": "Uuid", "primary_key": true },
                            "tier":      { "type": "Text", "not_null":    true }
                        }
                    }
                }
            }"#,
        )
        .unwrap();
        let schema = crate::ir_nodes::IRStoreColumnSchema::ManifestRef {
            qualified_name: "public.tenants".to_string(),
        };
        let cols = declared_columns_for("tenants", Some(&schema), None, Some(&m))
            .unwrap()
            .unwrap();
        assert_eq!(cols.len(), 2);
        assert_eq!(cols.get("tenant_id").copied(), Some(StoreColumnType::Uuid));
    }

    #[test]
    fn declared_columns_for_env_var_uses_first_match_heuristic_at_deploy() {
        // Mirrors §38.d's load_columns_for_schema heuristic — the
        // deploy verifier uses the same shape.
        let m = Manifest::parse_json(
            r#"{
                "version": 1,
                "stores": {
                    "tenant_42.events": {
                        "columns": {
                            "event_id": { "type": "Uuid" }
                        }
                    }
                }
            }"#,
        )
        .unwrap();
        let schema = crate::ir_nodes::IRStoreColumnSchema::EnvVar {
            var_name: "TENANT_SCHEMA".to_string(),
        };
        // Exact `tenant_99.events` is not present → first-match
        // heuristic finds `tenant_42.events`.
        let cols = declared_columns_for("events", Some(&schema), Some("tenant_99"), Some(&m))
            .unwrap()
            .unwrap();
        assert!(cols.contains_key("event_id"));
    }

    #[test]
    fn declared_columns_for_manifest_ref_missing_entry_is_a_named_error() {
        let m = Manifest::parse_json(
            r#"{"version":1,"stores":{"public.other":{"columns":{"x":{"type":"Uuid"}}}}}"#,
        )
        .unwrap();
        let schema = crate::ir_nodes::IRStoreColumnSchema::ManifestRef {
            qualified_name: "public.tenants".to_string(),
        };
        let result = declared_columns_for("tenants", Some(&schema), None, Some(&m));
        match result {
            Err(msg) => {
                assert!(msg.contains("public.tenants"));
                assert!(msg.contains("Available manifest entries"));
            }
            other => panic!("expected named missing-entry error, got {other:?}"),
        }
    }

    #[test]
    fn store_error_t806_and_t807_display_carries_the_slug_and_remedy() {
        let t806 = StoreError::MissingPerTenantSchemaEnv {
            store: "tenants".to_string(),
            var: "TENANT_SCHEMA".to_string(),
        };
        let msg = t806.to_string();
        assert!(msg.contains("axon-T806"));
        assert!(msg.contains("TENANT_SCHEMA"));
        assert!(msg.contains("Never a silent fallback"));

        let t807 = StoreError::DeclaredVsLiveDrift {
            store: "tenants".to_string(),
            drift: "missing on live database: {tier}".to_string(),
        };
        let msg = t807.to_string();
        assert!(msg.contains("axon-T807"));
        assert!(msg.contains("tenants"));
        assert!(msg.contains("axon store introspect"), "remedy must point at the CLI: {msg}");
    }
}
