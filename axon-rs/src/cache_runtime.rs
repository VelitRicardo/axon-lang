//! §Fase 85.d — the result-memoization cache core.
//!
//! This is the production-hardened runtime behind the `cache` primitive. The
//! type checker (§85.c) already proved WHAT is safe to cache (a `pure` tool by
//! construction; a widened one only with a finite TTL); this module implements
//! HOW, with the properties a naïve cache omits and that cause real outages:
//!
//! - **Content-addressed, deploy-safe, tenant-isolated keys (D85.7):** the key
//!   is a hash of `(tenant ‖ cache ‖ tool ‖ tool-declaration-fingerprint ‖
//!   output_type ‖ selected params)`. A redeploy that changes a tool changes
//!   its fingerprint → a new key → no stale cross-deploy hit; the tenant is a
//!   key component → no cross-tenant leak even if a backend mis-namespaces.
//! - **Single-flight (D85.8):** concurrent misses for one key compute ONCE;
//!   the rest wait for that result (no thundering herd).
//! - **Provable-forever, never non-deterministic-forever (D85.9):** enforced at
//!   compile time; the runtime simply honours the (optional) TTL.
//! - **Production hygiene (D85.10):** errors are never cached; oversized values
//!   are not cached (never truncated into a wrong value); TTL expiry is
//!   *jittered* (deterministically, per key) so entries don't expire in a herd.
//!
//! The `CacheBackend` trait lets the enterprise inject a Redis (multi-replica)
//! tier; with none injected, the in-process tier is fully functional
//! single-replica.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use axon_frontend::ir_nodes::{IRCache, IRProgram, IRToolSpec};

/// Default cap on the in-process tier (entries), mirroring `IdempotencyStore`.
pub const DEFAULT_CAPACITY: usize = 10_000;
/// Default per-value size ceiling (bytes). An oversized result is simply not
/// cached (D85.10) — never truncated into a wrong value.
pub const DEFAULT_MAX_VALUE_BYTES: usize = 512 * 1024;

// ── Duration parsing (mirrors the lexer's `<n><unit>` Duration token) ────────

/// Parse a duration literal (`"10s"`, `"500ms"`, `"5m"`, `"2h"`, `"1d"`) to a
/// `Duration`. `None` for a malformed string (the lexer already guarantees the
/// shape for a `ttl:` field, so this is defence in depth).
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, unit): (&str, &str) = if let Some(p) = s.strip_suffix("ms") {
        (p, "ms")
    } else if let Some(p) = s.strip_suffix('s') {
        (p, "s")
    } else if let Some(p) = s.strip_suffix('m') {
        (p, "m")
    } else if let Some(p) = s.strip_suffix('h') {
        (p, "h")
    } else if let Some(p) = s.strip_suffix('d') {
        (p, "d")
    } else {
        return None;
    };
    let n: u64 = num.parse().ok()?;
    Some(match unit {
        "ms" => Duration::from_millis(n),
        "s" => Duration::from_secs(n),
        "m" => Duration::from_secs(n * 60),
        "h" => Duration::from_secs(n * 3600),
        "d" => Duration::from_secs(n * 86400),
        _ => return None,
    })
}

// ── Content-addressed key derivation (D85.7) ─────────────────────────────────

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// A length-prefixed hash component — length-prefixing makes element boundaries
/// forgery-proof (no value can fake a boundary, the §84 argv-hash discipline
/// strengthened with explicit lengths).
fn update_part(h: &mut Sha256, part: &str) {
    h.update((part.len() as u64).to_le_bytes());
    h.update(part.as_bytes());
}

/// The stable fingerprint of a tool's DECLARATION — a hash of its IR spec. A
/// redeploy that changes the tool's provider, effects, output type, or
/// parameters changes this, so a behaviour change can never serve a result
/// cached under the old behaviour (D85.7).
pub fn tool_fingerprint(tool: &IRToolSpec) -> String {
    match serde_json::to_vec(tool) {
        Ok(bytes) => {
            let mut h = Sha256::new();
            h.update(&bytes);
            hex(&h.finalize())[..16].to_string()
        }
        Err(_) => "unfingerprintable".to_string(),
    }
}

/// Derive the content-addressed cache key. `key_args` are the selected
/// `(param_name, value)` pairs (the full bound set, or the `key:` subset).
pub fn derive_key(
    tenant: &str,
    cache_name: &str,
    tool_name: &str,
    tool_fingerprint: &str,
    output_type: &str,
    key_args: &[(String, String)],
) -> String {
    let mut h = Sha256::new();
    for part in [tenant, cache_name, tool_name, tool_fingerprint, output_type] {
        update_part(&mut h, part);
    }
    // Sort so argument order never changes the key.
    let mut sorted: Vec<&(String, String)> = key_args.iter().collect();
    sorted.sort();
    update_part(&mut h, &format!("__argc={}", sorted.len()));
    for (k, v) in sorted {
        update_part(&mut h, k);
        update_part(&mut h, v);
    }
    hex(&h.finalize())
}

// ── The backend trait + in-process tier ──────────────────────────────────────

/// A pluggable cache tier. `namespace` is the cache declaration's name so
/// `invalidate` can flush exactly one cache's entries. The enterprise injects a
/// Redis impl of this; the OSS default is [`InProcessCache`].
pub trait CacheBackend: Send + Sync {
    fn get(&self, namespace: &str, key: &str) -> Option<Vec<u8>>;
    fn put(&self, namespace: &str, key: &str, value: Vec<u8>, ttl: Option<Duration>);
    /// Flush every entry belonging to `namespace` (an `emit` on an
    /// `invalidate_on:` channel triggers this).
    fn invalidate(&self, namespace: &str);
}

struct Entry {
    value: Vec<u8>,
    expires_at: Option<Instant>,
    last_access: Instant,
}

struct State {
    entries: HashMap<(String, String), Entry>,
    capacity: usize,
    max_value_bytes: usize,
}

/// The OSS default single-replica tier: a bounded map with per-entry TTL
/// (jittered), LRU eviction, a value-size bound, and single-flight miss
/// coalescing via per-key locks.
pub struct InProcessCache {
    state: Mutex<State>,
    /// Per-key locks that serialise concurrent computers for the same key
    /// (single-flight, D85.8). Held only during a compute; opportunistically
    /// reclaimed when no computer references it.
    keylocks: Mutex<HashMap<(String, String), Arc<Mutex<()>>>>,
}

impl Default for InProcessCache {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY, DEFAULT_MAX_VALUE_BYTES)
    }
}

impl InProcessCache {
    pub fn new(capacity: usize, max_value_bytes: usize) -> Self {
        InProcessCache {
            state: Mutex::new(State {
                entries: HashMap::new(),
                capacity: capacity.max(1),
                max_value_bytes,
            }),
            keylocks: Mutex::new(HashMap::new()),
        }
    }

    fn now() -> Instant {
        Instant::now()
    }

    /// Deterministic per-key jitter (0..=ttl/10) so entries sharing a TTL do
    /// NOT expire in a synchronised herd (D85.10). Deterministic (derived from
    /// the key) — no RNG, reproducible, and still spreads expiries across keys.
    fn jitter(key: &str, ttl: Duration) -> Duration {
        let span = ttl.as_millis() as u64 / 10;
        if span == 0 {
            return Duration::ZERO;
        }
        let mut h = Sha256::new();
        h.update(key.as_bytes());
        let digest = h.finalize();
        let seed = u64::from_le_bytes(digest[..8].try_into().unwrap_or([0; 8]));
        Duration::from_millis(seed % (span + 1))
    }

    /// Single-flight compute-through: return a cached value, or compute it
    /// exactly once even under concurrent misses for the same key. A computed
    /// ERROR is propagated but NEVER cached (D85.10).
    pub fn get_or_compute<F, E>(
        &self,
        namespace: &str,
        key: &str,
        ttl: Option<Duration>,
        compute: F,
    ) -> Result<Vec<u8>, E>
    where
        F: FnOnce() -> Result<Vec<u8>, E>,
    {
        if let Some(v) = self.get(namespace, key) {
            return Ok(v);
        }
        // Acquire (or create) the per-key lock and serialise computers for it.
        let keylock = {
            let mut locks = self.keylocks.lock().unwrap();
            locks
                .entry((namespace.to_string(), key.to_string()))
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _flight = keylock.lock().unwrap();
        // Re-check under the flight lock: a peer may have filled it.
        if let Some(v) = self.get(namespace, key) {
            self.reclaim_keylock(namespace, key, &keylock);
            return Ok(v);
        }
        let result = compute();
        if let Ok(ref value) = result {
            self.put(namespace, key, value.clone(), ttl);
        }
        drop(_flight);
        self.reclaim_keylock(namespace, key, &keylock);
        result
    }

    /// Drop the per-key lock from the map once no other computer references it
    /// (strong_count == 2: the map's + our local clone).
    fn reclaim_keylock(&self, namespace: &str, key: &str, held: &Arc<Mutex<()>>) {
        let mut locks = self.keylocks.lock().unwrap();
        if Arc::strong_count(held) <= 2 {
            locks.remove(&(namespace.to_string(), key.to_string()));
        }
    }

    /// Current entry count (test/introspection).
    pub fn len(&self) -> usize {
        self.state.lock().unwrap().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl CacheBackend for InProcessCache {
    fn get(&self, namespace: &str, key: &str) -> Option<Vec<u8>> {
        let mut st = self.state.lock().unwrap();
        let k = (namespace.to_string(), key.to_string());
        let expired = match st.entries.get(&k) {
            Some(e) => e.expires_at.map(|t| Self::now() >= t).unwrap_or(false),
            None => return None,
        };
        if expired {
            st.entries.remove(&k);
            return None;
        }
        let now = Self::now();
        let e = st.entries.get_mut(&k)?;
        e.last_access = now;
        Some(e.value.clone())
    }

    fn put(&self, namespace: &str, key: &str, value: Vec<u8>, ttl: Option<Duration>) {
        let mut st = self.state.lock().unwrap();
        // D85.10 — an oversized value is simply not cached.
        if value.len() > st.max_value_bytes {
            return;
        }
        // LRU eviction when at capacity (and not overwriting an existing key).
        let k = (namespace.to_string(), key.to_string());
        if st.entries.len() >= st.capacity && !st.entries.contains_key(&k) {
            if let Some(oldest) = st
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_access)
                .map(|(k, _)| k.clone())
            {
                st.entries.remove(&oldest);
            }
        }
        let expires_at = ttl.map(|d| Self::now() + d + Self::jitter(key, d));
        st.entries.insert(
            k,
            Entry {
                value,
                expires_at,
                last_access: Self::now(),
            },
        );
    }

    fn invalidate(&self, namespace: &str) {
        let mut st = self.state.lock().unwrap();
        st.entries.retain(|(ns, _), _| ns != namespace);
    }
}

// ── Policy resolution (which cache governs a tool) ───────────────────────────

/// Resolve which `cache` (if any) governs a tool's memoization, given the whole
/// program IR (D85.2). Precedence: an explicit `cache: none` opts out; an
/// explicit `cache: <Name>` selects that cache; otherwise the single
/// `default: true` cache applies IFF the tool is eligible (provably `pure`, or
/// its effects are a subset of the default's `apply_to_effects`). Returns
/// `None` when nothing caches the tool.
pub fn resolve_tool_cache<'a>(ir: &'a IRProgram, tool: &IRToolSpec) -> Option<&'a IRCache> {
    // Explicit opt-out.
    if tool.cache == "none" {
        return None;
    }
    // Explicit reference.
    if !tool.cache.is_empty() {
        return ir.caches.iter().find(|c| c.name == tool.cache);
    }
    // Module default (if exactly one and the tool is eligible).
    let default = ir.caches.iter().find(|c| c.default_policy)?;
    let apply: Vec<String> = if default.apply_to_effects.is_empty() {
        vec!["pure".to_string()]
    } else {
        default
            .apply_to_effects
            .iter()
            .map(|e| e.split_once(':').map(|(b, _)| b.to_string()).unwrap_or_else(|| e.clone()))
            .collect()
    };
    // The tool's effect row (IR lowers effects with an optional `epistemic:`
    // suffix; compare on the base).
    let eligible = !tool.effect_row.is_empty()
        && tool.effect_row.iter().all(|e| {
            let base = e.split_once(':').map(|(b, _)| b).unwrap_or(e.as_str());
            apply.iter().any(|a| a == base)
        });
    if eligible {
        Some(default)
    } else {
        None
    }
}

// ── §Fase 122.d — resolution, hoisted to plan-build time ────────────────────

/// Everything the dispatch path needs to key ONE memoised call, resolved from
/// the `IRProgram` once, when the plan is built.
///
/// # Why this type exists
///
/// [`CacheRuntime::dispatch`] took `&IRProgram` because [`resolve_tool_cache`]
/// needs the whole module to answer "which cache governs this tool?" — the
/// default-policy rule is a property of the module, not of the tool. But
/// `DispatchCtx` carries no `IRProgram`, deliberately: it carries narrow,
/// pre-resolved catalogs (`credentials`, `anchors`, the §114.w shield policies)
/// so the hot path looks nothing up that could have been looked up once.
///
/// Threading the IR through the runtime to satisfy one call would have inverted
/// that, and for no gain: the answer cannot change between the deploy and the
/// call. So resolution moves to where the IR already is, and the runtime
/// receives the answer.
///
/// This is the §114.w `collect_shield_policies` shape, and it is also the §120
/// discipline — [`resolve_tool_cache`] stays the ONE place that decides, and
/// gains a second caller rather than a second copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCachePolicy {
    /// The governing cache's name — also the backend NAMESPACE, so `invalidate`
    /// flushes exactly this cache's entries.
    pub cache_name: String,
    /// Raw TTL literal (`"5m"`); `None` ⇒ cache-forever, sound only because
    /// `axon-T865` proved the memoised thing deterministic.
    pub ttl: Option<String>,
    /// The `key:` subset; empty ⇒ every bound argument keys the entry.
    pub key_params: Vec<String>,
    /// The declaration fingerprint (D85.7) — a redeploy that changes what is
    /// memoised changes this, so a behaviour change can never be served a
    /// result cached under the old behaviour.
    pub fingerprint: String,
    /// Part of the key so two tools with identical arguments but different
    /// result types never collide.
    pub output_type: String,
}

impl ResolvedCachePolicy {
    fn from_parts(cache: &IRCache, fingerprint: String, output_type: String) -> Self {
        ResolvedCachePolicy {
            cache_name: cache.name.clone(),
            ttl: cache.ttl.clone(),
            key_params: cache.key_params.clone(),
            fingerprint,
            output_type,
        }
    }

    /// §Fase 122.d — the policy governing a `retrieve … cache: <Name>`.
    ///
    /// A retrieve names its cache directly, so there is no eligibility question
    /// to resolve — but it still needs a fingerprint, and the honest one is the
    /// STORE it reads. A redeploy that changes the store's shape must not serve
    /// rows cached against the old one, exactly as a changed tool declaration
    /// must not (D85.7). `axon-T865` already forces a finite `ttl:` here,
    /// because a store read is never `pure`.
    pub fn for_retrieve(cache: &IRCache, store_name: &str) -> Self {
        let mut h = Sha256::new();
        update_part(&mut h, "retrieve");
        update_part(&mut h, store_name);
        Self::from_parts(
            cache,
            hex(&h.finalize())[..16].to_string(),
            String::new(),
        )
    }
}

/// Resolve, for every tool in the program, which cache (if any) memoises it —
/// keyed by TOOL NAME, which is how the dispatch path knows a tool.
///
/// Empty for a program with no `cache` declaration, which is the overwhelming
/// majority: an absent entry is the same "not memoised" answer
/// [`resolve_tool_cache`] gives, so a cache-less program pays one failed hash
/// lookup per tool call and behaves byte-identically to pre-§122.d.
pub fn resolve_tool_cache_policies(ir: &IRProgram) -> HashMap<String, ResolvedCachePolicy> {
    let mut out = HashMap::new();
    if ir.caches.is_empty() {
        return out;
    }
    for tool in &ir.tools {
        if let Some(cache) = resolve_tool_cache(ir, tool) {
            out.insert(
                tool.name.clone(),
                ResolvedCachePolicy::from_parts(
                    cache,
                    tool_fingerprint(tool),
                    tool.output_type.clone().unwrap_or_default(),
                ),
            );
        }
    }
    out
}

/// §Fase 122.d — **everything a deployment memoises**, resolved once from the
/// `IRProgram`.
///
/// # Why one struct and not three parameters
///
/// The three maps are not independent knobs; they are one answer to "what does
/// this program memoise?". A caller that supplied two of them would get a
/// runtime that caches but never invalidates, or one that flushes a namespace
/// nothing writes to — silent wrong answers, both, and the shape that makes
/// them reachable is a builder with three setters.
///
/// Bundled, the only way to half-wire the cache is not to wire it at all, which
/// is the honest `None` default. This is the §120 lesson expressed as an API:
/// make the second door impossible rather than remembering to walk through it.
///
/// Travels the same route as `scopes`, `observables` and `credentials` — built
/// where the IR is, passed down as a resolved catalog, so the hot path looks
/// nothing up that could have been looked up once.
#[derive(Debug, Clone, Default)]
pub struct CachePlan {
    /// Tool name → the policy memoising it.
    pub tool_policies: HashMap<String, ResolvedCachePolicy>,
    /// Cache name → declaration, for `retrieve … cache:` and namespace flushes.
    pub caches: HashMap<String, IRCache>,
    /// Channel name → the namespaces an `emit` on it flushes.
    pub invalidation_channels: HashMap<String, Vec<String>>,
}

impl CachePlan {
    /// Resolve the whole plan from a compiled program.
    pub fn from_ir(ir: &IRProgram) -> Self {
        CachePlan {
            tool_policies: resolve_tool_cache_policies(ir),
            caches: ir
                .caches
                .iter()
                .map(|c| (c.name.clone(), c.clone()))
                .collect(),
            invalidation_channels: resolve_invalidation_channels(ir),
        }
    }

    /// `true` for a program that declares no `cache` at all — the overwhelming
    /// majority, and the case where attaching a runtime buys nothing.
    pub fn is_empty(&self) -> bool {
        self.caches.is_empty()
    }
}

/// Channel name → the cache namespaces an `emit` on it flushes (`invalidate_on:`).
///
/// Inverted at plan-build time so `run_emit` answers "does this emit invalidate
/// anything?" with one hash lookup instead of scanning every cache declaration
/// on every emit. Empty ⇒ no cache in this program declares `invalidate_on:`,
/// and the emit path is byte-identical to pre-§122.d.
pub fn resolve_invalidation_channels(ir: &IRProgram) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for cache in &ir.caches {
        for channel in &cache.invalidate_on {
            out.entry(channel.clone())
                .or_default()
                .push(cache.name.clone());
        }
    }
    out
}

// ── Integration layer (the one seam the runner calls) ───────────────────────

/// Ties policy resolution + content-addressed key derivation + single-flight
/// compute-through into one call the dispatch path makes per tool. The
/// enterprise injects a Redis `backend` + the real `tenant`; the OSS default is
/// an in-process backend under a `"local"` tenant. This is the whole runtime
/// contract for §85 — a hit returns before `compute` runs (so a budget gate
/// placed after the lookup never sees it, D85.3).
pub struct CacheRuntime {
    backend: Arc<dyn CacheBackend>,
    tenant: String,
}

/// The outcome of a cache-mediated dispatch — lets the caller emit the right
/// `cache:hit` / `cache:miss` audit signal (D85.3) without re-deriving it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheOutcome {
    Hit(Vec<u8>),
    Miss(Vec<u8>),
    /// The tool is not cache-eligible; carries the freshly computed value so
    /// the caller uses it exactly as it would a `Miss`, minus the audit signal.
    Uncached(Vec<u8>),
}

impl CacheOutcome {
    /// The result value, regardless of hit/miss/uncached.
    pub fn value(&self) -> &[u8] {
        match self {
            CacheOutcome::Hit(v) | CacheOutcome::Miss(v) | CacheOutcome::Uncached(v) => v,
        }
    }
}

/// §Fase 122.d — a reserved place to put a computed value, handed out by
/// [`CacheRuntime::probe`] on a miss and consumed by [`CacheRuntime::store`].
///
/// It carries the derived key rather than the inputs, so the value is stored
/// under the key the lookup missed on — a caller cannot accidentally store
/// against a key derived from arguments that changed in between.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheSlot {
    namespace: String,
    key: String,
    ttl: Option<Duration>,
}

/// §Fase 122.d — what a pre-dispatch probe found.
///
/// The three arms are the three different things a caller must do, which is why
/// this is not an `Option<Vec<u8>>`: "nothing memoises this call" and
/// "memoised, but absent" look identical to an `Option` and are not the same
/// fact — the first stores nothing afterwards, the second must.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheProbe {
    /// No policy governs this call. Dispatch normally; store nothing.
    NotCached,
    /// A memoised value. **Do not dispatch, and do not charge a budget** — that
    /// ordering is D85.3, and it is the caller's to honour.
    Hit(Vec<u8>),
    /// Memoised but absent. Dispatch, then hand the value to
    /// [`CacheRuntime::store`] with this slot.
    Miss(CacheSlot),
}

impl CacheRuntime {
    pub fn new(backend: Arc<dyn CacheBackend>, tenant: impl Into<String>) -> Self {
        CacheRuntime {
            backend,
            tenant: tenant.into(),
        }
    }

    /// In-process, single-tenant default (OSS runtime with no injected tier).
    pub fn in_process() -> Self {
        Self::new(Arc::new(InProcessCache::default()), "local")
    }

    /// §Fase 122.d — the OSS runtime a production flow run gets: the
    /// **process-wide** in-process tier, keyed to **this run's tenant**.
    ///
    /// # Why the split, and what each half prevents
    ///
    /// This looks like a detail and is the difference between a cache and a
    /// decoration.
    ///
    /// Build the whole `CacheRuntime` per flow run and the backend is empty
    /// every time: a tool called once per run — which is most tools — never
    /// hits anything, and §122.d would ship a memoiser that memoises within a
    /// single run and forgets between them. Wired, tested, and worthless.
    ///
    /// Share the whole `CacheRuntime` across runs and it is worse than
    /// worthless. `tenant` is a field of the runtime, not a parameter of the
    /// call, so one shared instance would key every tenant's results under
    /// whichever tenant built it first — and D85.7 puts the tenant IN the key
    /// precisely so that a mis-namespacing backend still cannot leak. A shared
    /// runtime with a fixed tenant defeats that from above the backend, where
    /// the key is derived.
    ///
    /// So the BACKEND is process-wide (entries survive between runs, which is
    /// what makes it a cache) and the TENANT comes from the run (which is what
    /// keeps them apart). Constructing this is two `Arc` clones.
    ///
    /// The enterprise §85.f Redis tier replaces the backend here and inherits
    /// the same discipline unchanged — it is a different `CacheBackend`, not a
    /// different call site.
    pub fn process_local(tenant: impl Into<String>) -> Self {
        static TIER: std::sync::OnceLock<Arc<InProcessCache>> = std::sync::OnceLock::new();
        let backend = TIER.get_or_init(|| Arc::new(InProcessCache::default()));
        Self::new(backend.clone(), tenant)
    }

    /// Look up (or compute-and-store) a tool result. `args` is the full bound
    /// `(name, value)` set; the `key:` subset (if any) is applied here.
    /// `compute` runs ONLY on a miss and its error is never cached (D85.10).
    pub fn dispatch<F, E>(
        &self,
        ir: &IRProgram,
        tool: &IRToolSpec,
        args: &[(String, String)],
        compute: F,
    ) -> Result<CacheOutcome, E>
    where
        F: FnOnce() -> Result<Vec<u8>, E>,
    {
        // §Fase 122.d — resolution and execution split, so the dispatch path can
        // supply a policy resolved once at plan-build time (see
        // [`ResolvedCachePolicy`]). This entry point keeps its `&IRProgram`
        // signature and resolves on the spot; both routes run the SAME body
        // below, so there is one memoisation law and two ways to reach it —
        // never two laws.
        let policy = resolve_tool_cache(ir, tool).map(|cache| {
            ResolvedCachePolicy::from_parts(
                cache,
                tool_fingerprint(tool),
                tool.output_type.clone().unwrap_or_default(),
            )
        });
        self.dispatch_resolved(policy.as_ref(), &tool.name, args, compute)
    }

    /// §Fase 122.d — the memoisation body, against an already-resolved policy.
    ///
    /// `subject` is what the entry is keyed to — a tool's name, or a store's
    /// name for a `retrieve`. `policy: None` means "nothing memoises this":
    /// `compute` runs and the value comes back as [`CacheOutcome::Uncached`],
    /// which the caller uses exactly as a `Miss` minus the audit signal.
    ///
    /// # The ordering that D85.3 rests on
    ///
    /// A hit returns BEFORE `compute` is called. That is not an optimisation,
    /// it is the guarantee: the caller places its budget charge inside
    /// `compute`'s caller, so a hit cannot decrement a `budget { rate: … }`
    /// quota. §85's plan calls this *"structurally guaranteed by ordering the
    /// cache lookup before the budget gate"* — the structure is right here.
    pub fn dispatch_resolved<F, E>(
        &self,
        policy: Option<&ResolvedCachePolicy>,
        subject: &str,
        args: &[(String, String)],
        compute: F,
    ) -> Result<CacheOutcome, E>
    where
        F: FnOnce() -> Result<Vec<u8>, E>,
    {
        match self.probe(policy, subject, args) {
            CacheProbe::NotCached => compute().map(CacheOutcome::Uncached),
            CacheProbe::Hit(v) => Ok(CacheOutcome::Hit(v)),
            CacheProbe::Miss(slot) => {
                let value = compute()?;
                self.store(&slot, value.clone());
                Ok(CacheOutcome::Miss(value))
            }
        }
    }

    /// §Fase 122.d — **look, without computing.**
    ///
    /// # Why the seam had to split
    ///
    /// [`dispatch_resolved`](Self::dispatch_resolved) takes a `compute` closure,
    /// which is the right shape when the work is synchronous and owns nothing.
    /// The real tool-call path is neither: the work between the lookup and the
    /// value is `async`, it borrows `&mut DispatchCtx`, and it is not one call
    /// but four in sequence — the budget charge, the lease charge, the
    /// concurrency permit, then the vendor dispatch. None of that fits inside
    /// an `FnOnce() -> Result<Vec<u8>, E>`, and contorting it to fit would have
    /// meant either blocking the executor or duplicating the memoisation law at
    /// the call site.
    ///
    /// So the law splits into the two moments an async caller actually has:
    /// probe before the work, store after it. `dispatch_resolved` is now
    /// implemented in terms of these, so the synchronous seam §85 designed and
    /// the asynchronous one §122.d needed run the SAME key derivation, the same
    /// TTL parse and the same namespace — one law, two ways in.
    ///
    /// **The ordering D85.3 rests on is the caller's to keep**: a
    /// [`CacheProbe::Hit`] means the call must not be dispatched AND no budget
    /// charged. Returning early on a hit is what makes "a hit never consumes a
    /// quota" structural rather than hopeful, and it is asserted from a real
    /// deploy in `fase122_d_cache_hits.rs`.
    pub fn probe(
        &self,
        policy: Option<&ResolvedCachePolicy>,
        subject: &str,
        args: &[(String, String)],
    ) -> CacheProbe {
        let Some(policy) = policy else {
            return CacheProbe::NotCached;
        };
        // Apply the `key:` subset (empty ⇒ all args).
        let key_args: Vec<(String, String)> = if policy.key_params.is_empty() {
            args.to_vec()
        } else {
            args.iter()
                .filter(|(k, _)| policy.key_params.contains(k))
                .cloned()
                .collect()
        };
        let key = derive_key(
            &self.tenant,
            &policy.cache_name,
            subject,
            &policy.fingerprint,
            &policy.output_type,
            &key_args,
        );
        if let Some(v) = self.backend.get(&policy.cache_name, &key) {
            return CacheProbe::Hit(v);
        }
        CacheProbe::Miss(CacheSlot {
            namespace: policy.cache_name.clone(),
            key,
            ttl: policy.ttl.as_deref().and_then(parse_duration),
        })
    }

    /// §Fase 122.d — fill the slot a [`CacheProbe::Miss`] reserved.
    ///
    /// Errors are never stored (D85.10) — that is the caller's decision, and it
    /// is expressed by simply not calling this. Oversized values are dropped by
    /// the backend, never truncated into a wrong value.
    pub fn store(&self, slot: &CacheSlot, value: Vec<u8>) {
        self.backend.put(&slot.namespace, &slot.key, value, slot.ttl);
    }

    /// Flush a cache namespace (called when an `emit` fires on one of its
    /// `invalidate_on:` channels).
    pub fn invalidate(&self, cache_name: &str) {
        self.backend.invalidate(cache_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc as StdArc;

    fn ir_from(src: &str) -> IRProgram {
        let toks = axon_frontend::lexer::Lexer::new(src, "<cache-test>")
            .tokenize()
            .unwrap();
        let prog = axon_frontend::parser::Parser::new(toks).parse().unwrap();
        axon_frontend::ir_generator::IRGenerator::new().generate(&prog)
    }

    const CACHE_PROG: &str = concat!(
        "flow F() -> Unit { step S { ask: \"hi\" } }\n",
        "tool Enrich { provider: http effects: <pure> output_type: Report parameters: { id: String } }\n",
        "cache DefaultPure { default: true }\n",
    );

    #[test]
    fn end_to_end_pure_tool_second_call_is_a_hit() {
        let ir = ir_from(CACHE_PROG);
        let tool = ir.tools.iter().find(|t| t.name == "Enrich").unwrap();
        let rt = CacheRuntime::in_process();
        let computes = StdArc::new(AtomicUsize::new(0));
        let args = vec![("id".to_string(), "42".to_string())];

        let call = || {
            let computes = computes.clone();
            rt.dispatch::<_, ()>(&ir, tool, &args, || {
                computes.fetch_add(1, Ordering::SeqCst);
                Ok(b"enriched".to_vec())
            })
        };
        // First call → miss (computes once).
        assert_eq!(call().unwrap(), CacheOutcome::Miss(b"enriched".to_vec()));
        // Second call, same args → hit (no recompute).
        assert_eq!(call().unwrap(), CacheOutcome::Hit(b"enriched".to_vec()));
        assert_eq!(computes.load(Ordering::SeqCst), 1, "pure tool computed once");

        // A different arg value → a fresh miss (distinct content-addressed key).
        let args2 = vec![("id".to_string(), "99".to_string())];
        let out = rt
            .dispatch::<_, ()>(&ir, tool, &args2, || Ok(b"other".to_vec()))
            .unwrap();
        assert_eq!(out, CacheOutcome::Miss(b"other".to_vec()));
    }

    #[test]
    fn ineligible_tool_is_uncached() {
        // A network tool with no cache reference and no covering default.
        let ir = ir_from(concat!(
            "flow F() -> Unit { step S { ask: \"hi\" } }\n",
            "tool Fetch { provider: http effects: <network> parameters: { url: String } }\n",
        ));
        let tool = ir.tools.iter().find(|t| t.name == "Fetch").unwrap();
        let rt = CacheRuntime::in_process();
        let out = rt
            .dispatch::<_, ()>(&ir, tool, &[], || Ok(b"x".to_vec()))
            .unwrap();
        assert_eq!(out, CacheOutcome::Uncached(b"x".to_vec()));
    }

    #[test]
    fn invalidate_forces_recompute() {
        let ir = ir_from(CACHE_PROG);
        let tool = ir.tools.iter().find(|t| t.name == "Enrich").unwrap();
        let rt = CacheRuntime::in_process();
        let args = vec![("id".to_string(), "1".to_string())];
        rt.dispatch::<_, ()>(&ir, tool, &args, || Ok(b"v1".to_vec())).unwrap();
        rt.invalidate("DefaultPure");
        let out = rt
            .dispatch::<_, ()>(&ir, tool, &args, || Ok(b"v2".to_vec()))
            .unwrap();
        assert_eq!(out, CacheOutcome::Miss(b"v2".to_vec()), "invalidated → recompute");
    }

    #[test]
    fn duration_parsing() {
        assert_eq!(parse_duration("10s"), Some(Duration::from_secs(10)));
        assert_eq!(parse_duration("500ms"), Some(Duration::from_millis(500)));
        assert_eq!(parse_duration("5m"), Some(Duration::from_secs(300)));
        assert_eq!(parse_duration("2h"), Some(Duration::from_secs(7200)));
        assert_eq!(parse_duration("1d"), Some(Duration::from_secs(86400)));
        assert_eq!(parse_duration("bogus"), None);
    }

    #[test]
    fn key_is_content_addressed_and_deploy_safe() {
        let args = vec![("city".to_string(), "London".to_string())];
        let base = derive_key("t1", "C", "Weather", "fp1", "Out", &args);
        // Same everything → same key.
        assert_eq!(base, derive_key("t1", "C", "Weather", "fp1", "Out", &args));
        // Different tenant → different key (D85.11 isolation in the key).
        assert_ne!(base, derive_key("t2", "C", "Weather", "fp1", "Out", &args));
        // Different tool fingerprint (a redeploy) → different key (D85.7).
        assert_ne!(base, derive_key("t1", "C", "Weather", "fp2", "Out", &args));
        // Different arg value → different key.
        let args2 = vec![("city".to_string(), "Paris".to_string())];
        assert_ne!(base, derive_key("t1", "C", "Weather", "fp1", "Out", &args2));
    }

    #[test]
    fn arg_order_does_not_change_key() {
        let a = vec![("a".to_string(), "1".to_string()), ("b".to_string(), "2".to_string())];
        let b = vec![("b".to_string(), "2".to_string()), ("a".to_string(), "1".to_string())];
        assert_eq!(
            derive_key("t", "C", "T", "fp", "O", &a),
            derive_key("t", "C", "T", "fp", "O", &b)
        );
    }

    #[test]
    fn arg_boundaries_are_forgery_proof() {
        // ("ab","c") vs ("a","bc") must NOT collide (length-prefixing).
        let a = vec![("ab".to_string(), "c".to_string())];
        let b = vec![("a".to_string(), "bc".to_string())];
        assert_ne!(
            derive_key("t", "C", "T", "fp", "O", &a),
            derive_key("t", "C", "T", "fp", "O", &b)
        );
    }

    #[test]
    fn hit_returns_stored_value() {
        let c = InProcessCache::default();
        c.put("C", "k", b"value".to_vec(), None);
        assert_eq!(c.get("C", "k"), Some(b"value".to_vec()));
        assert_eq!(c.get("C", "missing"), None);
    }

    #[test]
    fn ttl_expiry_evicts() {
        let c = InProcessCache::default();
        c.put("C", "k", b"v".to_vec(), Some(Duration::from_millis(1)));
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(c.get("C", "k"), None, "expired entry must be gone");
    }

    #[test]
    fn invalidate_flushes_only_its_namespace() {
        let c = InProcessCache::default();
        c.put("A", "k", b"1".to_vec(), None);
        c.put("B", "k", b"2".to_vec(), None);
        c.invalidate("A");
        assert_eq!(c.get("A", "k"), None);
        assert_eq!(c.get("B", "k"), Some(b"2".to_vec()), "other cache untouched");
    }

    #[test]
    fn oversized_value_is_not_cached() {
        let c = InProcessCache::new(10, 4);
        c.put("C", "k", vec![0u8; 100], None);
        assert_eq!(c.get("C", "k"), None, "oversized value must not be cached");
    }

    #[test]
    fn capacity_evicts_lru() {
        let c = InProcessCache::new(2, DEFAULT_MAX_VALUE_BYTES);
        c.put("C", "a", b"1".to_vec(), None);
        c.put("C", "b", b"2".to_vec(), None);
        let _ = c.get("C", "a"); // touch a → b is now LRU
        c.put("C", "c", b"3".to_vec(), None); // evicts b
        assert_eq!(c.get("C", "a"), Some(b"1".to_vec()));
        assert_eq!(c.get("C", "b"), None, "LRU entry evicted");
        assert_eq!(c.get("C", "c"), Some(b"3".to_vec()));
    }

    #[test]
    fn errors_are_never_cached() {
        let c = InProcessCache::default();
        let r: Result<Vec<u8>, &str> =
            c.get_or_compute("C", "k", None, || Err("boom"));
        assert!(r.is_err());
        assert_eq!(c.get("C", "k"), None, "a computed error must not be cached");
    }

    #[test]
    fn single_flight_coalesces_concurrent_misses() {
        let c = StdArc::new(InProcessCache::default());
        let computes = StdArc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..16 {
            let c = c.clone();
            let computes = computes.clone();
            handles.push(std::thread::spawn(move || {
                c.get_or_compute::<_, ()>("C", "hot", None, || {
                    computes.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(20));
                    Ok(b"result".to_vec())
                })
                .unwrap()
            }));
        }
        for h in handles {
            assert_eq!(h.join().unwrap(), b"result".to_vec());
        }
        assert_eq!(
            computes.load(Ordering::SeqCst),
            1,
            "single-flight: concurrent misses for one key compute exactly once"
        );
    }
}
