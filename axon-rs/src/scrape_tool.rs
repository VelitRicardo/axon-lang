//! v2.52.0 — Native Web Acquisition runtime: the OSS sibling of
//! [`crate::http_tool`], the dispatch core for the three web-acquisition
//! providers (`scrape_http` / `scrape_dom` / `scrape_crawl`).
//!
//! **The load-bearing property:** every value this module produces is
//! born epistemically **Untrusted** (⊥ in the lattice `doubt ⊑ speculate ⊑
//! believe ⊑ know`) — reusing [`crate::emcp::EpistemicTaint::Untrusted`]
//! verbatim. The open web is adversarial; a page an agent fetched is not a
//! fact until a `shield` scans it. The compile-time content-injection barrier
//! (v2.52.0) refuses to let a `web`-tainted value reach an agent's belief
//! context without an intervening shield — this runtime realises the OTHER
//! half of that contract: the taint is stamped on the outcome so the audit
//! trail and any runtime IFC (v2.52.0) can observe it.
//!
//! **Tiers.** Acquisition is fingerprint-first, browser-second, both
//! ENTERPRISE. OSS ships the *contract* plus a plain-`reqwest` fallback that
//! is functional but un-stealthed: a [`ScrapeFetcher`] the enterprise engine
//! (v2.52.0) registers to supply JA3/JA4 + HTTP/2 impersonation and per-tenant
//! proxies. With no enterprise engine injected, `impersonate` degrades to a
//! plain request (honest, un-stealthed) and `browser` returns an explicit
//! "no sidecar configured" refusal — **never a silent wrong result**.
//!
//! **Bounded.** Bodies are size-capped + truncated; crawls are
//! `max_pages`/`max_depth`-bounded; timeouts apply. A hostile or infinite
//! site can neither OOM the runtime nor spin the crawler forever.
//!
//! **Honest heuristics.** Adaptive selector relocation is a *heuristic*, not a
//! proof; the OSS DOM engine is a deterministic reference extractor
//! (regex-scanned elements over a closed selector subset), not a full browser
//! DOM — the enterprise/browser tier owns fidelity. Both are named as such.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::emcp::EpistemicTaint;
use crate::tool_executor::ToolResult;
use crate::tool_registry::{ScrapeConfig, ToolEntry};

/// Default fetched-body ceiling (5 MiB). A page larger than this is truncated
/// and flagged `truncated: true` — the output is always bounded.
pub const DEFAULT_BODY_LIMIT: usize = 5 * 1024 * 1024;

/// Default per-request timeout when the tool declares none.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Hard ceiling on a single crawl regardless of the declared `max_pages`, so a
/// mis-declared `max_pages: 0` (unbounded sentinel) can never run away.
pub const CRAWL_HARD_PAGE_CAP: usize = 10_000;

/// v2.69.0 (owed) — hard ceiling on in-flight fetches for a single crawl,
/// regardless of the declared `concurrency`. Politeness first: a `concurrency:
/// 1000` can never open a thousand sockets to a vendor. The declared value is
/// clamped to `[1, CRAWL_MAX_CONCURRENCY]`; `1` is the pre-v2.69.0 sequential crawl.
pub const CRAWL_MAX_CONCURRENCY: usize = 32;

// ════════════════════════════════════════════════════════════════════════════
//  RawPage — the typed acquisition result the grammar names
// ════════════════════════════════════════════════════════════════════════════

/// A fetched page. The `output_type: RawPage` a `scrape_http` / `scrape_crawl`
/// tool declares. Serialised to JSON as the tool's output string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawPage {
    /// HTTP status of the final response.
    pub status: u16,
    /// The final URL after redirects (the effective origin of `body`).
    pub final_url: String,
    /// A bounded subset of response headers (lowercased names).
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// The response body, capped at [`DEFAULT_BODY_LIMIT`].
    pub body: String,
    /// Whether this page was served from a cache/checkpoint (crawl resume).
    #[serde(default)]
    pub from_cache: bool,
    /// Whether `body` was truncated to the size cap.
    #[serde(default)]
    pub truncated: bool,
    /// The engine that produced this page (`impersonate` | `browser` |
    /// `reqwest-fallback`) — provenance for the audit trail.
    #[serde(default)]
    pub engine: String,
}

impl RawPage {
    /// Cap `body` to the limit, flagging truncation. Total.
    fn capped(mut self, limit: usize) -> Self {
        if self.body.len() > limit {
            // Truncate on a char boundary to keep valid UTF-8.
            let mut end = limit;
            while end > 0 && !self.body.is_char_boundary(end) {
                end -= 1;
            }
            self.body.truncate(end);
            self.truncated = true;
        }
        self
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Errors + provenance-tagged outcome
// ════════════════════════════════════════════════════════════════════════════

/// Everything that can go wrong acquiring a page. Every variant is a *typed
/// refusal* — a failed challenge or a robots denial is a first-class outcome,
/// never a silently-empty body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScrapeError {
    /// The structured tool body did not carry the required argument.
    MissingArgument(String),
    /// The URL was empty or had a non-http(s) scheme.
    InvalidUrl(String),
    /// `robots.txt` disallows this path for our user-agent and the tool did
    /// not set the (enterprise-gated) `respect_robots: false` override.
    RobotsDenied(String),
    /// The `browser` engine was requested but no sidecar is configured — the
    /// OSS build has no headless renderer (v2.52.0 is the enterprise sidecar).
    NoBrowserSidecar,
    /// The upstream fetch failed (connect/timeout/transport).
    FetchFailed(String),
    /// An anti-bot challenge blocked the request (typed, not silent).
    Blocked(String),
    /// The `scrape_dom` `page:` argument was not a decodable `RawPage`.
    MalformedPage(String),
}

impl std::fmt::Display for ScrapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScrapeError::MissingArgument(a) => {
                write!(f, "scrape: missing required argument '{a}'")
            }
            ScrapeError::InvalidUrl(u) => write!(f, "scrape: invalid URL '{u}' (need http/https)"),
            ScrapeError::RobotsDenied(u) => {
                write!(f, "scrape: robots.txt disallows '{u}' (respect_robots is on)")
            }
            ScrapeError::NoBrowserSidecar => write!(
                f,
                "scrape: engine 'browser' requested but no headless sidecar is configured — \
                 the OSS build has no renderer (this is not a silent empty result)"
            ),
            ScrapeError::FetchFailed(e) => write!(f, "scrape: fetch failed: {e}"),
            ScrapeError::Blocked(e) => write!(f, "scrape: request blocked by anti-bot: {e}"),
            ScrapeError::MalformedPage(e) => write!(f, "scrape_dom: malformed page argument: {e}"),
        }
    }
}

impl std::error::Error for ScrapeError {}

/// The provenance-tagged outcome of a scrape dispatch. `taint` is ALWAYS
/// [`EpistemicTaint::Untrusted`] on success — the born-⊥ property. The
/// registry integration flattens this to a [`ToolResult`] (which does not
/// carry taint, matching the ℰMCP discipline where the shield/know step
/// elevates), but the taint is exposed here so `v2.52.0` IFC + audit can observe
/// it directly.
#[derive(Debug, Clone)]
pub struct ScrapeOutcome {
    pub result: ToolResult,
    pub taint: EpistemicTaint,
}

impl ScrapeOutcome {
    fn ok(tool_name: &str, output: String) -> Self {
        ScrapeOutcome {
            result: ToolResult {
                success: true,
                output,
                tool_name: tool_name.to_string(),
            },
            // Web content is born at ⊥ — untrusted until a shield elevates it.
            taint: EpistemicTaint::Untrusted,
        }
    }

    fn err(tool_name: &str, e: ScrapeError) -> Self {
        ScrapeOutcome {
            result: ToolResult {
                success: false,
                output: e.to_string(),
                tool_name: tool_name.to_string(),
            },
            // A failed acquisition carries no content, but the channel is
            // still adversarial — keep the taint at ⊥.
            taint: EpistemicTaint::Untrusted,
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// ScrapeFetcher — the enterprise injection seam (v2.52.0)
// ════════════════════════════════════════════════════════════════════════════

/// A single fetch request, resolved from the tool's [`ScrapeConfig`] + the
/// per-call `url`. The enterprise engine reads `engine`/`impersonate`/`proxy`
/// to select a JA3/JA4 fingerprint + proxy; the OSS default fetcher ignores
/// them (plain `reqwest`).
#[derive(Debug, Clone)]
pub struct FetchRequest {
    pub url: String,
    pub engine: String,
    pub impersonate: String,
    pub proxy: String,
    pub respect_robots: bool,
    pub render_wait: String,
    pub timeout: Duration,
    pub body_limit: usize,
    /// v2.57.0-1 — the acquiring tenant, carried from
    /// [`ScrapeConfig::tenant`] (v2.56.0) so a fetch-grain audit row (a robots
    /// denial / anti-bot block) can be attributed to the right tenant's
    /// hash-chained log. Empty in OSS standalone use (no tenant) ⇒ no audit row
    /// is emitted (an unattributable acquisition is never witnessed cross-tenant).
    pub tenant: String,
}

/// The pluggable fetch engine. OSS registers nothing → the plain-`reqwest`
/// [`default_fetch`] is used. The enterprise stealth engine (v2.52.0) registers
/// an impl via [`register_scrape_fetcher`] to supply fingerprint impersonation
/// + proxy pools. Exactly the [`crate::shield_registry`] injection shape.
pub trait ScrapeFetcher: Send + Sync {
    /// Fetch a single page. MUST return a typed [`ScrapeError`] on failure —
    /// never an empty [`RawPage`] masquerading as success.
    fn fetch(&self, req: &FetchRequest) -> Result<RawPage, ScrapeError>;

    /// A short slug identifying the engine, for the `RawPage.engine`
    /// provenance field + audit.
    fn engine_slug(&self) -> &'static str;
}

static SCRAPE_FETCHER: OnceLock<Arc<dyn ScrapeFetcher>> = OnceLock::new();

/// v2.52.0 — register the enterprise stealth fetcher. Idempotent-first:
/// the first registration wins (the enterprise host installs it once at boot).
/// Returns `true` if this call installed the fetcher.
pub fn register_scrape_fetcher(fetcher: Arc<dyn ScrapeFetcher>) -> bool {
    SCRAPE_FETCHER.set(fetcher).is_ok()
}

/// Whether an enterprise fetcher is installed (else the OSS fallback is used).
pub fn has_registered_fetcher() -> bool {
    SCRAPE_FETCHER.get().is_some()
}

/// v2.56.0 — the per-tenant adaptive-selector memory seam. The extractor
/// consults a registered memory to (a) **recall** a previously-learned selector
/// for a drifted field before scanning, and (b) **learn** the selector a
/// relocation just recovered — so the lead pipeline *heals* a target's HTML
/// drift across runs instead of a human rewriting selectors. Strictly
/// tenant-keyed. OSS ships none (recall → `None`, learn → noop); the enterprise
/// Postgres store (v2.56.0) registers here, exactly the [`register_scrape_fetcher`]
/// shape. The `(tenant, tool, field, domain)` key is supplied per-call by the
/// extractor (the tenant rides `ScrapeConfig.tenant`, the design decision).
pub trait SelectorMemory: Send + Sync {
    /// A previously-learned selector for this coordinate, if any.
    fn recall(&self, tenant: &str, tool: &str, field: &str, domain: &str) -> Option<String>;
    /// Record a selector a relocation recovered (last-writer-wins — the most
    /// recent successful relocation is the best current guess).
    fn learn(&self, tenant: &str, tool: &str, field: &str, domain: &str, selector: &str);
}

fn selector_memory_reg() -> &'static std::sync::RwLock<Option<Arc<dyn SelectorMemory>>> {
    static REG: OnceLock<std::sync::RwLock<Option<Arc<dyn SelectorMemory>>>> = OnceLock::new();
    REG.get_or_init(|| std::sync::RwLock::new(None))
}

/// v2.56.0 — register the enterprise durable selector store (the host calls
/// this once at boot). Replaces any prior registration.
pub fn register_selector_memory(memory: Arc<dyn SelectorMemory>) {
    *selector_memory_reg().write().expect("selector memory poisoned") = Some(memory);
}

/// Clear the registered memory (back to the OSS no-memory default).
pub fn clear_selector_memory() {
    *selector_memory_reg().write().expect("selector memory poisoned") = None;
}

// ════════════════════════════════════════════════════════════════════════════
// ScrapeAuditSink — the per-fetch audit seam (v2.57.0-1)
// ════════════════════════════════════════════════════════════════════════════

/// A witnessed fetch-grain outcome. The runtime emits these AS a fetch resolves
/// (the executor pre-flight, v2.56.0, only sees the *declared* tier — not whether
/// a live fetch was actually robots-denied or challenge-blocked). Host-only: an
/// event NEVER carries the fetched body (born-Untrusted content stays out of the
/// audit trail — the v2.39.0 discipline); the enterprise sink extracts just the
/// host from `url` for the row.
#[derive(Debug, Clone)]
pub enum ScrapeAuditEvent<'a> {
    /// `robots.txt` disallowed the path and no (governed) aggressive override was
    /// authorized — a first-class refusal, witnessed at the fetch grain.
    RobotsDenied { url: &'a str },
    /// An anti-bot challenge blocked the fetch (typed, never a silent empty). The
    /// `engine` is the requested acquisition engine; `reason` is the typed cause.
    Blocked {
        url: &'a str,
        engine: &'a str,
        reason: &'a str,
    },
}

/// v2.57.0-1 — the per-fetch audit seam. OSS ships none (`record` → noop);
/// the enterprise host registers one that appends to the tenant's hash-chained
/// audit log (the `scrape:robots_denied` / `scrape:blocked` catalog rows, v2.52.0).
/// Exactly the [`register_scrape_fetcher`] / [`register_selector_memory`]
/// injection shape. The tenant is supplied per-call (it rides
/// [`FetchRequest::tenant`], the design decision).
pub trait ScrapeAuditSink: Send + Sync {
    /// Witness a fetch-grain outcome for `tenant`. Best-effort: the AUTHORIZATION
    /// controls are enforced elsewhere; this is the observability row.
    fn record(&self, tenant: &str, event: ScrapeAuditEvent<'_>);
}

fn scrape_audit_reg() -> &'static std::sync::RwLock<Option<Arc<dyn ScrapeAuditSink>>> {
    static REG: OnceLock<std::sync::RwLock<Option<Arc<dyn ScrapeAuditSink>>>> = OnceLock::new();
    REG.get_or_init(|| std::sync::RwLock::new(None))
}

/// v2.57.0-1 — register the enterprise fetch-grain audit sink (the host calls
/// this once at boot). Replaces any prior registration.
pub fn register_scrape_audit_sink(sink: Arc<dyn ScrapeAuditSink>) {
    *scrape_audit_reg().write().expect("scrape audit sink poisoned") = Some(sink);
}

/// Clear the registered sink (back to the OSS no-audit default).
pub fn clear_scrape_audit_sink() {
    *scrape_audit_reg().write().expect("scrape audit sink poisoned") = None;
}

/// Witness a fetch-grain outcome — noop without a registered sink or an
/// unstamped tenant (an unattributable acquisition is never witnessed across
/// tenants — the same guard `recall_selector` uses).
fn audit_scrape(tenant: &str, event: ScrapeAuditEvent<'_>) {
    if tenant.is_empty() {
        return;
    }
    if let Some(s) = scrape_audit_reg().read().expect("scrape audit sink poisoned").as_ref() {
        s.record(tenant, event);
    }
}

/// Recall a learned selector — `None` if no memory is registered or the tenant
/// is unstamped (an unscoped read must never cross tenants).
fn recall_selector(tenant: &str, tool: &str, field: &str, domain: &str) -> Option<String> {
    if tenant.is_empty() {
        return None;
    }
    selector_memory_reg()
        .read()
        .expect("selector memory poisoned")
        .as_ref()
        .and_then(|m| m.recall(tenant, tool, field, domain))
}

/// Learn a selector a relocation recovered — noop without a memory or a tenant.
fn learn_selector(tenant: &str, tool: &str, field: &str, domain: &str, selector: &str) {
    if tenant.is_empty() {
        return;
    }
    if let Some(m) = selector_memory_reg().read().expect("selector memory poisoned").as_ref() {
        m.learn(tenant, tool, field, domain, selector);
    }
}

/// Resolve the active fetcher: the registered enterprise engine, or the OSS
/// plain-`reqwest` fallback. v2.57.0-1 — witnesses a fetch-grain robots
/// denial / anti-bot block on the tenant's audit log as the fetch resolves (the
/// executor pre-flight only sees the declared tier). Host-only; noop in OSS.
fn fetch_page(req: &FetchRequest) -> Result<RawPage, ScrapeError> {
    let result = if let Some(f) = SCRAPE_FETCHER.get() {
        f.fetch(req)
    } else {
        default_fetch(req)
    };
    match &result {
        Err(ScrapeError::RobotsDenied(url)) => {
            audit_scrape(&req.tenant, ScrapeAuditEvent::RobotsDenied { url });
        }
        Err(ScrapeError::Blocked(reason)) => {
            audit_scrape(
                &req.tenant,
                ScrapeAuditEvent::Blocked {
                    url: &req.url,
                    engine: &req.engine,
                    reason,
                },
            );
        }
        _ => {}
    }
    result
}

// ════════════════════════════════════════════════════════════════════════════
//  Default OSS fetcher — plain reqwest, robots-respecting, bounded
// ════════════════════════════════════════════════════════════════════════════

/// The default AXON user-agent for OSS fetches. Honest (identifies the
/// runtime) — the enterprise engine impersonates a real browser instead.
pub const DEFAULT_USER_AGENT: &str = "AxonScrape/1.0 (+https://axon.dev)";

/// OSS fallback fetch: plain `reqwest::blocking`. No fingerprint stealth. The
/// `browser` engine is refused (no OSS renderer). robots.txt honored unless
/// the caller disabled it.
fn default_fetch(req: &FetchRequest) -> Result<RawPage, ScrapeError> {
    let url = req.url.trim();
    if url.is_empty() || (!url.starts_with("http://") && !url.starts_with("https://")) {
        return Err(ScrapeError::InvalidUrl(url.to_string()));
    }
    // The browser tier has no OSS engine — refuse explicitly.
    if req.engine == "browser" {
        return Err(ScrapeError::NoBrowserSidecar);
    }
    // robots-respecting default. A denial is a typed refusal.
    if req.respect_robots && !robots_allows(url, req.timeout) {
        return Err(ScrapeError::RobotsDenied(url.to_string()));
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(req.timeout)
        // Brief #63 — bound the CONNECT phase (DNS + TCP + TLS) explicitly. The
        // total-request `.timeout()` can be escaped by a DNS stall on some platforms
        // (the "timeout not honored" the adopter reported); `.connect_timeout` caps
        // the phase so a hung name-resolution degrades to a typed error, not a hang.
        .connect_timeout(clamp_connect_timeout(req.timeout))
        .user_agent(DEFAULT_USER_AGENT)
        .build()
        .map_err(|e| ScrapeError::FetchFailed(format!("client build: {e}")))?;

    let resp = client.get(url).send().map_err(|e| {
        if e.is_timeout() {
            ScrapeError::FetchFailed(format!("timed out after {}s", req.timeout.as_secs()))
        } else if e.is_connect() {
            ScrapeError::FetchFailed(format!("connection failed to {url}"))
        } else {
            ScrapeError::FetchFailed(e.to_string())
        }
    })?;

    let status = resp.status().as_u16();
    let final_url = resp.url().to_string();
    // A 403/429 with an anti-bot signature is a typed BLOCK, not a page.
    if status == 403 || status == 429 {
        return Err(ScrapeError::Blocked(format!("HTTP {status} from {final_url}")));
    }
    let mut headers = BTreeMap::new();
    for name in ["content-type", "content-length", "server", "last-modified"] {
        if let Some(v) = resp.headers().get(name).and_then(|v| v.to_str().ok()) {
            headers.insert(name.to_string(), v.to_string());
        }
    }
    let body = resp
        .text()
        .map_err(|e| ScrapeError::FetchFailed(format!("read body: {e}")))?;

    Ok(RawPage {
        status,
        final_url,
        headers,
        body,
        from_cache: false,
        truncated: false,
        engine: "reqwest-fallback".to_string(),
    }
    .capped(req.body_limit))
}

// ── robots.txt (OSS default enforcement) ─────────────────────────────────────

/// Fetch + evaluate `robots.txt` for `url`, honoring `Disallow` rules for the
/// `*` user-agent group. Fail-OPEN on fetch/parse failure (the standard
/// posture — an unreachable robots.txt does not block). The enterprise layer
/// (v2.52.0) adds the RBAC-gated override + the audit row; this is the sound
/// default so an OSS deployment is polite out of the box.
/// Brief #63 — the connect-phase budget derived from a request's total timeout.
/// Connecting (DNS + TCP + TLS) should be fast; cap it at 15s so a stalled
/// name-resolution fails quickly, but never above the caller's own total timeout
/// and never below 1s (so a tiny total timeout still permits a real connection).
fn clamp_connect_timeout(total: Duration) -> Duration {
    total
        .min(Duration::from_secs(15))
        .max(Duration::from_secs(1))
}

fn robots_allows(url: &str, timeout: Duration) -> bool {
    let (scheme, host, path) = match split_url(url) {
        Some(t) => t,
        None => return true,
    };
    let robots_url = format!("{scheme}://{host}/robots.txt");
    let client = match reqwest::blocking::Client::builder()
        .timeout(timeout.min(Duration::from_secs(10)))
        // Brief #63 — bound the connect phase of the robots.txt pre-fetch; it runs
        // BEFORE the page fetch and a DNS stall here would hang the whole scrape.
        .connect_timeout(clamp_connect_timeout(timeout))
        .user_agent(DEFAULT_USER_AGENT)
        .build()
    {
        Ok(c) => c,
        Err(_) => return true,
    };
    let body = match client.get(&robots_url).send().and_then(|r| r.text()) {
        Ok(b) => b,
        // No robots.txt reachable → allowed (fail-open).
        Err(_) => return true,
    };
    robots_path_allowed(&body, &path)
}

/// Pure robots.txt evaluator: does the `*` user-agent group permit `path`?
/// Longest-match `Allow`/`Disallow` wins (the de-facto standard). Public for
/// the v2.52.0 unit tests + the enterprise governance layer.
pub fn robots_path_allowed(robots_txt: &str, path: &str) -> bool {
    let mut in_star = false;
    let mut applicable = false;
    // (is_allow, pattern_len, allowed) longest-match accumulator.
    let mut best: Option<(usize, bool)> = None;
    for raw in robots_txt.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (key, val) = match line.split_once(':') {
            Some((k, v)) => (k.trim().to_ascii_lowercase(), v.trim().to_string()),
            None => continue,
        };
        match key.as_str() {
            "user-agent" => {
                // A new group. `*` (or a run of them) applies to us.
                if val == "*" {
                    in_star = true;
                    applicable = true;
                } else {
                    in_star = false;
                }
            }
            "disallow" | "allow" if in_star || applicable && in_star => {
                if val.is_empty() {
                    continue;
                }
                if path.starts_with(&val) {
                    let is_allow = key == "allow";
                    let len = val.len();
                    if best.map(|(bl, _)| len > bl).unwrap_or(true) {
                        best = Some((len, is_allow));
                    }
                }
            }
            _ => {}
        }
    }
    match best {
        Some((_, is_allow)) => is_allow,
        // No matching rule → allowed.
        None => true,
    }
}

/// Split an http(s) URL into `(scheme, host[:port], path)`. Minimal, dep-free
/// (OSS has no `url` crate). Returns `None` for non-http(s).
fn split_url(url: &str) -> Option<(String, String, String)> {
    let (scheme, rest) = if let Some(r) = url.strip_prefix("https://") {
        ("https", r)
    } else if let Some(r) = url.strip_prefix("http://") {
        ("http", r)
    } else {
        return None;
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    // Drop any userinfo + fragment; keep host[:port].
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let path = path.split('#').next().unwrap_or(path);
    Some((scheme.to_string(), host.to_string(), path.to_string()))
}

// ════════════════════════════════════════════════════════════════════════════
//  Synchronous dispatch — scrape_http + scrape_dom
// ════════════════════════════════════════════════════════════════════════════

/// Dispatch a synchronous scrape tool call. Routes by `entry.provider`:
/// `scrape_http` (fetch), `scrape_dom` (parse — no network). `scrape_crawl`
/// is streaming and handled by [`ScrapeStreamingTool`], not here. Returns the
/// [`ToolResult`] the registry integrates; the born-Untrusted taint lives on
/// the internal [`ScrapeOutcome`] (see [`dispatch_scrape_outcome`]).
pub fn dispatch_scrape(entry: &ToolEntry, argument: &str) -> ToolResult {
    dispatch_scrape_outcome(entry, argument).result
}

/// The taint-carrying dispatch (used by v2.52.0 IFC + tests). Always yields an
/// `Untrusted` outcome for a successful acquisition.
pub fn dispatch_scrape_outcome(entry: &ToolEntry, argument: &str) -> ScrapeOutcome {
    let cfg = entry.scrape.clone().unwrap_or_default();
    let args = parse_args(argument);
    match entry.provider.as_str() {
        "scrape_http" => run_scrape_http(&entry.name, &cfg, &entry.timeout, &args),
        "scrape_dom" => run_scrape_dom(&entry.name, &cfg, &args),
        // scrape_crawl is a streaming provider; a synchronous dispatch of it
        // returns a single seed fetch as a courtesy (the dispatcher routes
        // crawl through the streaming path).
        "scrape_crawl" => run_scrape_http(&entry.name, &cfg, &entry.timeout, &args),
        other => ScrapeOutcome::err(
            &entry.name,
            ScrapeError::FetchFailed(format!("unknown scrape provider '{other}'")),
        ),
    }
}

/// Parse the structured tool body (`{ "url": "...", ... }`) into a JSON map.
/// A non-object body is wrapped as `{ "input": <text> }` (mirrors http_tool's
/// body discipline) so a bare-string `on <arg>` call still finds a `url`.
fn parse_args(argument: &str) -> serde_json::Value {
    let trimmed = argument.trim_start();
    if trimmed.starts_with('{') {
        serde_json::from_str(argument).unwrap_or_else(|_| serde_json::json!({ "input": argument }))
    } else {
        serde_json::json!({ "input": argument, "url": argument, "page": argument })
    }
}

fn arg_str<'a>(args: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

fn run_scrape_http(
    name: &str,
    cfg: &ScrapeConfig,
    timeout: &str,
    args: &serde_json::Value,
) -> ScrapeOutcome {
    let url = match arg_str(args, "url") {
        Some(u) if !u.is_empty() => u.to_string(),
        _ => return ScrapeOutcome::err(name, ScrapeError::MissingArgument("url".into())),
    };
    let req = FetchRequest {
        url,
        engine: cfg.effective_engine().to_string(),
        impersonate: cfg.impersonate.clone(),
        proxy: cfg.proxy.clone(),
        respect_robots: cfg.respect_robots,
        render_wait: cfg.render_wait.clone(),
        timeout: crate::http_tool::parse_timeout_pub(timeout).unwrap_or(DEFAULT_TIMEOUT),
        body_limit: DEFAULT_BODY_LIMIT,
        // v2.57.0-1 — the tenant rides the request so a fetch-grain audit row
        // attributes to the right hash-chained log (v2.56.0 stamps it on cfg).
        tenant: cfg.tenant.clone(),
    };
    match fetch_page(&req) {
        Ok(page) => match serde_json::to_string(&page) {
            Ok(json) => ScrapeOutcome::ok(name, json),
            Err(e) => ScrapeOutcome::err(name, ScrapeError::FetchFailed(format!("encode: {e}"))),
        },
        Err(e) => ScrapeOutcome::err(name, e),
    }
}

fn run_scrape_dom(name: &str, cfg: &ScrapeConfig, args: &serde_json::Value) -> ScrapeOutcome {
    // The `page:` argument is a `RawPage` (from a prior scrape_http), or a bare
    // HTML string. Either way, the taint is PRESERVED — scrape_dom does no I/O,
    // it only processes already-Untrusted content, so its output stays ⊥.
    // Capture the source domain from the RawPage's `final_url` so the v2.56.0
    // selector memory keys a learned selector to the site (a selector learned on
    // `news.acme.com` must not leak into `shop.acme.com`). A bare-HTML input has
    // no URL → empty domain → the memory is simply not consulted.
    let domain_of = |url: &str| split_url(url).map(|(_, host, _)| host).unwrap_or_default();
    let (html, domain) = match args.get("page") {
        Some(serde_json::Value::String(s)) => {
            // Could be a JSON-encoded RawPage or raw HTML.
            match serde_json::from_str::<RawPage>(s) {
                Ok(p) => {
                    let d = domain_of(&p.final_url);
                    (p.body, d)
                }
                Err(_) => (s.clone(), String::new()),
            }
        }
        Some(v @ serde_json::Value::Object(_)) => match serde_json::from_value::<RawPage>(v.clone())
        {
            Ok(p) => {
                let d = domain_of(&p.final_url);
                (p.body, d)
            }
            Err(e) => return ScrapeOutcome::err(name, ScrapeError::MalformedPage(e.to_string())),
        },
        _ => return ScrapeOutcome::err(name, ScrapeError::MissingArgument("page".into())),
    };

    let extracted = extract_fields(
        &html,
        &cfg.extract,
        cfg.adaptive,
        cfg.similarity_floor,
        &cfg.tenant,
        name,
        &domain,
    );
    match serde_json::to_string(&extracted) {
        Ok(json) => ScrapeOutcome::ok(name, json),
        Err(e) => ScrapeOutcome::err(name, ScrapeError::MalformedPage(format!("encode: {e}"))),
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Deterministic DOM extractor (reference engine) + adaptive heuristic
// ════════════════════════════════════════════════════════════════════════════

/// One scanned HTML element: `(tag, id, classes, inner_text)`.
#[derive(Debug, Clone)]
struct Element {
    tag: String,
    id: String,
    classes: Vec<String>,
    text: String,
}

/// A parsed selector from the closed subset: an optional tag + optional `#id`
/// + optional `.class`. E.g. `h1`, `.price`, `#main`, `div.card`, `a#logo`.
#[derive(Debug, Clone, Default)]
struct Selector {
    tag: Option<String>,
    id: Option<String>,
    class: Option<String>,
}

fn parse_selector(sel: &str) -> Selector {
    let sel = sel.trim();
    let mut out = Selector::default();
    // Split into a leading tag and #id/.class fragments.
    let bytes = sel.as_bytes();
    // leading tag = run until a '.' or '#'
    let mut j = 0;
    while j < bytes.len() && bytes[j] != b'.' && bytes[j] != b'#' {
        j += 1;
    }
    if j > 0 {
        out.tag = Some(sel[..j].to_ascii_lowercase());
    }
    let mut i = j;
    while i < bytes.len() {
        let marker = bytes[i];
        let start = i + 1;
        let mut k = start;
        while k < bytes.len() && bytes[k] != b'.' && bytes[k] != b'#' {
            k += 1;
        }
        let frag = &sel[start..k];
        match marker {
            b'#' => out.id = Some(frag.to_string()),
            b'.' => out.class = Some(frag.to_string()),
            _ => {}
        }
        i = k;
    }
    out
}

fn matches(el: &Element, s: &Selector) -> bool {
    if let Some(t) = &s.tag {
        if &el.tag != t {
            return false;
        }
    }
    if let Some(id) = &s.id {
        if &el.id != id {
            return false;
        }
    }
    if let Some(c) = &s.class {
        if !el.classes.iter().any(|x| x == c) {
            return false;
        }
    }
    true
}

/// Scan HTML into a flat element list. A deterministic reference parser
/// (regex over open-tag..matching-close for a common set of container tags),
/// NOT a spec-compliant DOM — the enterprise/browser tier owns fidelity
///. Nested same-tag elements resolve to the outermost occurrence,
/// which is sufficient for the closed selector subset.
/// The closed tag set the reference extractor scans. Container + inline tags
/// common in article/product markup — sufficient for the closed selector
/// subset; the enterprise/browser tier owns full-DOM fidelity.
const SCANNED_TAGS: &[&str] = &[
    "h1", "h2", "h3", "h4", "h5", "h6", "p", "a", "span", "div", "article", "section", "li", "td",
    "th", "title", "strong", "em", "b", "i",
];

fn scan_elements(html: &str) -> Vec<Element> {
    use regex::Regex;
    // The `regex` crate has no backreferences, so we scan per-tag with a
    // compiled-once regex map (`<tag ...>inner</tag>`, non-greedy). Cached in
    // source order across the tag set; each element records its byte start so
    // the result stays in document order.
    static RES: OnceLock<Vec<(String, Regex)>> = OnceLock::new();
    let res = RES.get_or_init(|| {
        SCANNED_TAGS
            .iter()
            .map(|t| {
                (
                    t.to_string(),
                    Regex::new(&format!(r#"(?is)<{t}\b([^>]*)>(.*?)</{t}>"#))
                        .expect("static per-tag scrape regex is valid"),
                )
            })
            .collect()
    });
    let attr_id = attr_regex("id");
    let attr_class = attr_regex("class");
    let tag_strip = tag_strip_regex();
    let mut out: Vec<(usize, Element)> = Vec::new();
    for (tag, re) in res {
        for cap in re.captures_iter(html) {
            let whole = cap.get(0).unwrap();
            let attrs = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let inner = cap.get(2).map(|m| m.as_str()).unwrap_or("");
            let id = attr_id
                .captures(attrs)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let classes = attr_class
                .captures(attrs)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().split_whitespace().map(|s| s.to_string()).collect())
                .unwrap_or_default();
            // Inner text = strip nested tags, collapse whitespace.
            let text = tag_strip.replace_all(inner, " ");
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            out.push((
                whole.start(),
                Element {
                    tag: tag.clone(),
                    id,
                    classes,
                    text,
                },
            ));
        }
    }
    // Document order.
    out.sort_by_key(|(start, _)| *start);
    out.into_iter().map(|(_, e)| e).collect()
}

fn attr_regex(name: &str) -> regex::Regex {
    regex::Regex::new(&format!(r#"(?i)\b{name}\s*=\s*["']([^"']*)["']"#)).expect("attr regex valid")
}

fn tag_strip_regex() -> regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"(?s)<[^>]*>").expect("tag strip regex valid"))
        .clone()
}

/// Extract every declared FieldSpec from `html`. Each spec is `name=selector`.
/// Returns a JSON object `{ name: text }`. When `adaptive` and a selector
/// misses, a HEURISTIC relocation is attempted (loosen the selector, keeping
/// the tag) — recorded honestly, never presented as a proof.
fn extract_fields(
    html: &str,
    specs: &[String],
    adaptive: bool,
    similarity_floor: f64,
    tenant: &str,
    tool: &str,
    domain: &str,
) -> serde_json::Value {
    let elements = scan_elements(html);
    let mut obj = serde_json::Map::new();
    for spec in specs {
        let (field, selector_str) = match spec.split_once('=') {
            Some((f, s)) => (f.trim(), s.trim()),
            // A malformed spec yields a null — never a panic.
            None => {
                obj.insert(spec.clone(), serde_json::Value::Null);
                continue;
            }
        };
        // v2.56.0 — (1) recall: a previously-learned selector for this
        // (tenant, tool, field, domain) that STILL matches heals a prior drift
        // before we even try the declared selector.
        if let Some(learned) = recall_selector(tenant, tool, field, domain) {
            let lsel = parse_selector(&learned);
            if let Some(el) = elements.iter().find(|el| matches(el, &lsel)) {
                obj.insert(field.to_string(), serde_json::Value::String(el.text.clone()));
                continue;
            }
        }
        // (2) the declared selector, exact.
        let sel = parse_selector(selector_str);
        if let Some(el) = elements.iter().find(|el| matches(el, &sel)) {
            obj.insert(field.to_string(), serde_json::Value::String(el.text.clone()));
            continue;
        }
        // (3) adaptive relocation — and LEARN the selector it recovered, so the
        // next run recalls it directly (v2.56.0, the drift-healing loop).
        let value = if adaptive {
            match relocate(&elements, &sel, similarity_floor) {
                Some((text, learned_selector)) => {
                    learn_selector(tenant, tool, field, domain, &learned_selector);
                    Some(text)
                }
                None => None,
            }
        } else {
            None
        };
        obj.insert(
            field.to_string(),
            value.map(serde_json::Value::String).unwrap_or(serde_json::Value::Null),
        );
    }
    serde_json::Value::Object(obj)
}

/// v2.56.0 — adaptive relocation, scored for real. When the exact
/// selector misses (the target drifted its `id`/`class`), score every candidate
/// element by **structural + textual similarity** to the selector's signature —
/// tag match + class-token Jaccard + id-token overlap — and relocate to the
/// best-scoring element, but ONLY if its similarity clears `similarity_floor`.
/// Below the floor it returns `None` (a typed empty), NEVER a wrong field: the
/// pre-v2.56.0 stub returned a hardcoded `1.0` for any tag-level fallback, silently
/// producing a wrong value. Honesty over a fabricated cell (v2.54.0 the design decision applied
/// to selectors). Deterministic: ties resolve to document order. This is still a
/// HEURISTIC, not a proof; the enterprise tier's durable per-tenant
/// selector memory (v2.56.0) is what turns a good relocation into learned drift.
fn relocate(elements: &[Element], sel: &Selector, similarity_floor: f64) -> Option<(String, String)> {
    // A selector with no distinguishing components cannot be relocated.
    if sel.tag.is_none() && sel.id.is_none() && sel.class.is_none() {
        return None;
    }
    let floor = if similarity_floor.is_nan() {
        1.0
    } else {
        similarity_floor.clamp(0.0, 1.0)
    };
    let mut best: Option<(f64, &Element)> = None;
    for el in elements {
        let score = selector_similarity(sel, el);
        if score + f64::EPSILON < floor {
            continue;
        }
        match best {
            Some((b, _)) if b >= score => {}
            _ => best = Some((score, el)),
        }
    }
    // Return the recovered text + a reconstructed selector that will MATCH this
    // element again — so v2.56.0 can learn it for the next run.
    best.map(|(_, el)| (el.text.clone(), reconstruct_selector(el)))
}

/// Reconstruct a stable selector from a matched element: `tag#id` if it has an
/// id, else `tag.class` on its first class, else the bare `tag`. By construction
/// `matches(el, parse_selector(reconstruct_selector(el)))` holds.
fn reconstruct_selector(el: &Element) -> String {
    if !el.id.is_empty() {
        format!("{}#{}", el.tag, el.id)
    } else if let Some(c) = el.classes.first() {
        format!("{}.{}", el.tag, c)
    } else {
        el.tag.clone()
    }
}

/// Similarity of a candidate element to a drifted selector's signature, in
/// `[0,1]`. Each component the selector SPECIFIES contributes its weight; a
/// component it omits is neutral (never penalises). Tag is the strongest signal;
/// class/id similarity is token-level so `price` still partially matches a
/// renamed `product-price` (Jaccard over `-`/`_`-split tokens). Normalised by the
/// specified weight so a class-only selector scores on class alone.
fn selector_similarity(sel: &Selector, el: &Element) -> f64 {
    const W_TAG: f64 = 0.5;
    const W_CLASS: f64 = 0.35;
    const W_ID: f64 = 0.35;
    let (mut score, mut weight) = (0.0, 0.0);
    if let Some(t) = &sel.tag {
        weight += W_TAG;
        if &el.tag == t {
            score += W_TAG;
        }
    }
    if let Some(c) = &sel.class {
        weight += W_CLASS;
        let a = tokenize(c);
        let mut b = std::collections::HashSet::new();
        for cls in &el.classes {
            b.extend(tokenize(cls));
        }
        score += W_CLASS * jaccard(&a, &b);
    }
    if let Some(id) = &sel.id {
        weight += W_ID;
        score += W_ID * jaccard(&tokenize(id), &tokenize(&el.id));
    }
    if weight == 0.0 {
        0.0
    } else {
        (score / weight).clamp(0.0, 1.0)
    }
}

/// Split an identifier into lowercase alphanumeric tokens (`product-price` →
/// `{product, price}`), so a partial rename still shares tokens.
fn tokenize(s: &str) -> std::collections::HashSet<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// Jaccard similarity of two token sets — `|A∩B| / |A∪B|`, `0` when both empty.
fn jaccard(
    a: &std::collections::HashSet<String>,
    b: &std::collections::HashSet<String>,
) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

// ════════════════════════════════════════════════════════════════════════════
//  ScrapeStreamingTool — concurrent checkpointed crawl (scrape_crawl)
// ════════════════════════════════════════════════════════════════════════════

use async_trait::async_trait;

use crate::tool_trait::{Tool, ToolChunk, ToolContext, ToolFinishReason, ToolStream};

/// v2.52.0 — the streaming crawl provider. A bounded, checkpointed spider:
/// it BFS-walks from `seed`, emitting each fetched [`RawPage`] as a
/// `ToolChunk` AS it arrives, bounded by `max_pages` / `max_depth` /
/// `concurrency`, honoring the per-invocation cancel flag between fetches.
/// Every emitted page is born Untrusted — the taint discipline is
/// identical to the synchronous providers; the compile-time barrier (v2.52.0)
/// enforces the shield before an agent's belief.
///
/// The OSS engine uses the plain-`reqwest` fetcher (or the registered
/// enterprise stealth engine); resumable checkpointing to a store is the
/// enterprise `<storage>` concern (v2.52.0) — OSS keeps the visited set in
/// memory (non-resumable, honestly bounded).
pub struct ScrapeStreamingTool {
    name: String,
    cfg: ScrapeConfig,
    timeout: Duration,
}

impl ScrapeStreamingTool {
    /// Build from a registry [`ToolEntry`]. Falls back to a default config
    /// when the entry carries none (defensive — the checker guarantees a
    /// `scrape_crawl` tool has a `scrape:` block).
    pub fn from_entry(entry: &ToolEntry) -> Self {
        ScrapeStreamingTool {
            name: entry.name.clone(),
            cfg: entry.scrape.clone().unwrap_or_default(),
            timeout: crate::http_tool::parse_timeout_pub(&entry.timeout).unwrap_or(DEFAULT_TIMEOUT),
        }
    }

    /// The effective per-crawl page ceiling: the declared `max_pages`
    /// (clamped to the hard cap), or the hard cap when unbounded (`0`).
    fn page_budget(&self) -> usize {
        let declared = self.cfg.max_pages.max(0) as usize;
        if declared == 0 {
            CRAWL_HARD_PAGE_CAP
        } else {
            declared.min(CRAWL_HARD_PAGE_CAP)
        }
    }
}

/// Extract absolute-ish links from a page body for crawl expansion. When a
/// `follow` selector is declared, only `<a>` hrefs are considered (the OSS
/// engine treats `follow` as "follow anchors"); enterprise fidelity can honor
/// a richer selector. Relative links are joined against `base`.
fn extract_links(html: &str, base: &str) -> Vec<String> {
    use regex::Regex;
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"(?i)<a\b[^>]*\bhref\s*=\s*["']([^"'#]+)["']"#).expect("href regex valid")
    });
    let mut out = Vec::new();
    for cap in re.captures_iter(html) {
        if let Some(m) = cap.get(1) {
            if let Some(abs) = join_url(base, m.as_str()) {
                out.push(abs);
            }
        }
    }
    out
}

/// Minimal URL join (dep-free): resolve `href` against `base`. Handles
/// absolute, root-relative, and same-directory relative links — enough for
/// the reference crawler; the enterprise engine can use a full `url` resolver.
fn join_url(base: &str, href: &str) -> Option<String> {
    if href.starts_with("http://") || href.starts_with("https://") {
        return Some(href.to_string());
    }
    let (scheme, host, path) = split_url(base)?;
    if let Some(rooted) = href.strip_prefix('/') {
        return Some(format!("{scheme}://{host}/{rooted}"));
    }
    // Same-directory relative: replace the last path segment.
    let dir = match path.rfind('/') {
        Some(i) => &path[..=i],
        None => "/",
    };
    Some(format!("{scheme}://{host}{dir}{href}"))
}

#[async_trait]
impl Tool for ScrapeStreamingTool {
    async fn execute(&self, args: String, _ctx: ToolContext) -> ToolResult {
        // Synchronous fallback: a crawl collapsed to a single seed fetch.
        let entry_cfg = self.cfg.clone();
        let timeout = self.timeout;
        let name = self.name.clone();
        let args_owned = args.clone();
        match tokio::task::spawn_blocking(move || {
            let parsed = parse_args(&args_owned);
            let entry = ToolEntry {
                name: name.clone(),
                provider: "scrape_http".to_string(),
                timeout: format!("{}s", timeout.as_secs()),
                runtime: String::new(),
                resource_ref: String::new(),
                substrate: None,
                capacity: None,
                sandbox: None,
                max_results: None,
                output_schema: String::new(),
                effect_row: vec!["network".into(), "web".into()],
                parameters: Vec::new(),
                secret: String::new(),
                secret_partition: String::new(),
                source: crate::tool_registry::ToolSource::Program,
                is_streaming: true,
                scrape: Some(entry_cfg),
            };
            // Seed arg may be under `seed` or `url`.
            let seed = parsed
                .get("seed")
                .and_then(|v| v.as_str())
                .or_else(|| parsed.get("url").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_string();
            run_scrape_http(
                &entry.name,
                &entry.scrape.clone().unwrap_or_default(),
                &entry.timeout,
                &serde_json::json!({ "url": seed }),
            )
            .result
        })
        .await
        {
            Ok(r) => r,
            Err(e) => ToolResult {
                success: false,
                output: format!("scrape_crawl '{}': join failed: {e}", self.name),
                tool_name: self.name.clone(),
            },
        }
    }

    async fn stream(&self, args: String, ctx: ToolContext) -> ToolStream {
        let name = self.name.clone();
        let cfg = self.cfg.clone();
        let timeout = self.timeout;
        let budget = self.page_budget();
        let max_depth = self.cfg.max_depth.max(0) as usize;
        let follow = !self.cfg.follow.is_empty();
        let cancel = ctx.cancel.clone();

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ToolChunk>();

        tokio::spawn(async move {
            let send_term = |reason: ToolFinishReason| {
                let _ = tx.send(ToolChunk::terminator("", reason));
            };

            let seed = {
                let parsed = parse_args(&args);
                parsed
                    .get("seed")
                    .and_then(|v| v.as_str())
                    .or_else(|| parsed.get("url").and_then(|v| v.as_str()))
                    .unwrap_or("")
                    .to_string()
            };
            if seed.is_empty() {
                send_term(ToolFinishReason::Error {
                    message: format!("scrape_crawl '{name}': missing 'seed' argument"),
                });
                return;
            }

            // v2.69.0 (owed) — the crawl is BOUNDED-CONCURRENT: up to
            // `concurrency` fetches in flight at once, the declaration
            // `scrape.concurrency` at last enforced (it was parsed + type-checked
            // + documented as honored, but the loop fetched one page at a time —
            // a documented behavior the code did not have). Clamped to
            // `[1, CRAWL_MAX_CONCURRENCY]`; `1` reproduces the prior sequential
            // crawl byte-for-byte (one in flight, awaited before the next).
            //
            // Correctness: the driver task alone owns `frontier` + `visited` +
            // `succeeded` (single-threaded — the concurrency is in the fetch
            // FUTURES, which return an owned `RawPage` and touch no shared state),
            // so no lock is needed. A URL is marked `visited` when it is LAUNCHED,
            // so two in-flight fetches never target the same URL. The success
            // budget holds with N in flight: a fetch is launched only while
            // `succeeded + in_flight < budget`, so at most `budget` pages are ever
            // emitted. Pages are emitted in ARRIVAL order (whichever fetch
            // finishes first) — the streaming contract is "each page AS it
            // arrives"; the BFS frontier still governs link EXPANSION order.
            let concurrency =
                (cfg.concurrency.max(1) as usize).min(CRAWL_MAX_CONCURRENCY);

            let mut frontier: std::collections::VecDeque<(String, usize)> =
                std::collections::VecDeque::new();
            let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
            frontier.push_back((seed, 0));
            let mut succeeded = 0usize;
            let mut in_flight = futures::stream::FuturesUnordered::new();

            loop {
                if cancel.is_cancelled() {
                    send_term(ToolFinishReason::Cancelled);
                    return;
                }

                // Fill the in-flight set up to `concurrency`, without launching
                // more fetches than could still reach the success budget.
                while in_flight.len() < concurrency && succeeded + in_flight.len() < budget {
                    let Some((url, depth)) = frontier.pop_front() else {
                        break; // frontier drained — let the in-flight set finish
                    };
                    if !visited.insert(url.clone()) {
                        continue; // already crawled — skip without consuming a slot
                    }
                    let req = FetchRequest {
                        url,
                        engine: cfg.effective_engine().to_string(),
                        impersonate: cfg.impersonate.clone(),
                        proxy: cfg.proxy.clone(),
                        respect_robots: cfg.respect_robots,
                        render_wait: cfg.render_wait.clone(),
                        timeout,
                        body_limit: DEFAULT_BODY_LIMIT,
                        // v2.57.0-1 — carry the tenant into each crawl fetch.
                        tenant: cfg.tenant.clone(),
                    };
                    // Blocking fetch on the blocking pool; carry `depth` so the
                    // completion can expand links at the right level.
                    let handle = tokio::task::spawn_blocking(move || fetch_page(&req));
                    in_flight.push(async move { (depth, handle.await) });
                }

                // Nothing running and nothing launchable → the crawl is done.
                if in_flight.is_empty() {
                    break;
                }

                // Await the next completed fetch (arrival order).
                let Some((depth, joined)) =
                    futures::StreamExt::next(&mut in_flight).await
                else {
                    break;
                };
                match joined {
                    Ok(Ok(page)) => {
                        succeeded += 1;
                        // Expand links (bounded by depth) before emitting.
                        if follow && depth < max_depth {
                            for link in extract_links(&page.body, &page.final_url) {
                                if !visited.contains(&link) {
                                    frontier.push_back((link, depth + 1));
                                }
                            }
                        }
                        match serde_json::to_string(&page) {
                            Ok(json) => {
                                if tx.send(ToolChunk::intermediate(json)).is_err() {
                                    return; // consumer dropped
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(ToolChunk::intermediate(
                                    ScrapeError::FetchFailed(format!("encode: {e}")).to_string(),
                                ));
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        // A per-page failure is a typed chunk, not a crawl
                        // abort — the spider keeps going (the design decision typed outcome).
                        let _ = tx.send(ToolChunk::intermediate(e.to_string()));
                    }
                    Err(e) => {
                        let _ = tx.send(ToolChunk::intermediate(format!("crawl join failed: {e}")));
                    }
                }
            }
            send_term(ToolFinishReason::Stop);
        });

        Box::pin(futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|chunk| (chunk, rx))
        }))
    }

    fn is_streaming(&self) -> bool {
        true
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_dom(extract: &[&str], adaptive: bool, floor: f64) -> ScrapeConfig {
        ScrapeConfig {
            extract: extract.iter().map(|s| s.to_string()).collect(),
            adaptive,
            similarity_floor: floor,
            ..Default::default()
        }
    }

    fn dom_entry(name: &str, cfg: ScrapeConfig) -> ToolEntry {
        ToolEntry {
            name: name.to_string(),
            provider: "scrape_dom".to_string(),
            timeout: String::new(),
            runtime: String::new(),
            resource_ref: String::new(),
            substrate: None,
            capacity: None,
            sandbox: None,
            max_results: None,
            output_schema: String::new(),
            effect_row: vec!["web".to_string()],
            parameters: Vec::new(),
            secret: String::new(),
            secret_partition: String::new(),
            source: crate::tool_registry::ToolSource::Program,
            is_streaming: false,
            scrape: Some(cfg),
        }
    }

    #[test]
    fn dom_output_is_born_untrusted() {
        let html = r#"<html><body><h1>Hello</h1><p class="lead">World</p></body></html>"#;
        let entry = dom_entry("Ex", cfg_dom(&["title=h1", "lead=p.lead"], false, 0.0));
        let out = dispatch_scrape_outcome(&entry, &format!("{{\"page\": {:?} }}", html));
        assert!(out.result.success, "output: {}", out.result.output);
        // The load-bearing property: web content is born at ⊥.
        assert_eq!(out.taint, EpistemicTaint::Untrusted);
        let v: serde_json::Value = serde_json::from_str(&out.result.output).unwrap();
        assert_eq!(v["title"], "Hello");
        assert_eq!(v["lead"], "World");
    }

    #[test]
    fn dom_selector_subset_id_class_tag() {
        let html = r#"<div id="main"><span class="price">$9</span><a href="/x">buy</a></div>"#;
        let out = extract_fields(
            html,
            &[
                "p=#main".to_string(), // #id (div) — but selector tag is empty, matches by id
                "price=.price".to_string(),
                "cta=a".to_string(),
            ],
            false,
            0.0,
            "", "", "",
        );
        assert_eq!(out["price"], "$9");
        assert_eq!(out["cta"], "buy");
    }

    #[test]
    fn dom_miss_without_adaptive_is_null() {
        let html = "<h1>Title</h1>";
        let out = extract_fields(html, &["x=.nope".to_string()], false, 0.0, "", "", "");
        assert_eq!(out["x"], serde_json::Value::Null);
    }

    #[test]
    fn dom_adaptive_relocates_by_tag() {
        // Selector `h1.headline` misses (no class), adaptive relocates to <h1>.
        let html = "<h1>Relocated</h1>";
        let out = extract_fields(html, &["t=h1.headline".to_string()], true, 0.5, "", "", "");
        assert_eq!(out["t"], "Relocated");
    }

    #[test]
    fn dom_adaptive_respects_floor() {
        // The class shares no token with the element → similarity 0 < floor → null.
        let html = "<h1>X</h1>";
        let out = extract_fields(html, &["t=.only-class".to_string()], true, 0.5, "", "", "");
        assert_eq!(out["t"], serde_json::Value::Null);
    }

    #[test]
    fn dom_adaptive_relocates_across_class_drift() {
        // v2.56.0 — the target renamed `.price` to `.product-price`; token
        // similarity (price ∈ {product, price}) relocates instead of empty.
        let html = r#"<span class="product-price">$42</span>"#;
        let out = extract_fields(html, &["p=span.price".to_string()], true, 0.6, "", "", "");
        assert_eq!(out["p"], "$42");
    }

    #[test]
    fn dom_adaptive_below_floor_is_null_not_a_wrong_field() {
        // v2.56.0 / the design decision — an unrelated same-tag element must NOT be returned.
        // The pre-v2.56.0 stub returned the FIRST same-tag element at confidence 1.0
        // — a silent wrong field. Real scoring falls below the floor → null.
        let html = r#"<span class="footer-legal">unrelated</span>"#;
        let out = extract_fields(html, &["p=span.price".to_string()], true, 0.75, "", "", "");
        assert_eq!(
            out["p"],
            serde_json::Value::Null,
            "a low-similarity element must not be fabricated as the field"
        );
    }

    #[test]
    fn dom_adaptive_picks_best_scoring_candidate() {
        // The span sharing the `price` token wins over an unrelated same-tag span.
        let html = r#"<span class="nav">Home</span><span class="unit-price">$9</span>"#;
        let out = extract_fields(html, &["p=span.price".to_string()], true, 0.6, "", "", "");
        assert_eq!(out["p"], "$9");
    }

    /// v2.56.0 — the drift-healing loop: a relocation LEARNS the recovered
    /// selector; a later run RECALLS it directly. Serialised because the memory
    /// registry is process-global.
    #[test]
    fn dom_selector_memory_learns_then_recalls_a_drift() {
        use std::collections::HashMap;
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _g = LOCK.lock().unwrap();

        #[derive(Default)]
        struct Mem {
            map: Mutex<HashMap<String, String>>,
        }
        impl SelectorMemory for Mem {
            fn recall(&self, t: &str, tool: &str, f: &str, d: &str) -> Option<String> {
                self.map.lock().unwrap().get(&format!("{t}|{tool}|{f}|{d}")).cloned()
            }
            fn learn(&self, t: &str, tool: &str, f: &str, d: &str, sel: &str) {
                self.map.lock().unwrap().insert(format!("{t}|{tool}|{f}|{d}"), sel.to_string());
            }
        }
        let mem = Arc::new(Mem::default());
        register_selector_memory(mem.clone());

        // Run 1: declared `.price` misses; relocation recovers via `product-price`
        // and LEARNS `span.product-price`.
        let html = r#"<span class="product-price">$42</span>"#;
        let out = extract_fields(
            html,
            &["p=span.price".to_string()],
            true,
            0.6,
            "kivi",
            "Harvest",
            "shop.acme.com",
        );
        assert_eq!(out["p"], "$42");
        assert_eq!(
            mem.recall("kivi", "Harvest", "p", "shop.acme.com").as_deref(),
            Some("span.product-price"),
            "the recovered selector must be learned"
        );

        // Run 2: even with adaptive OFF, the LEARNED selector is recalled first
        // and matches — the pipeline healed the drift without re-relocating.
        let out2 = extract_fields(
            html,
            &["p=span.price".to_string()],
            false,
            1.0,
            "kivi",
            "Harvest",
            "shop.acme.com",
        );
        assert_eq!(out2["p"], "$42", "the learned selector heals the drift on recall");

        // Isolation: a DIFFERENT tenant does not see kivi's learned selector.
        assert!(mem.recall("other", "Harvest", "p", "shop.acme.com").is_none());
        clear_selector_memory();
    }

    /// v2.57.0-1 — the fetch-grain audit seam records for a stamped tenant
    /// and is a NOOP for an unstamped one (an unattributable acquisition is never
    /// witnessed cross-tenant). Serialised — the sink registry is process-global.
    #[test]
    fn scrape_audit_sink_records_only_for_a_stamped_tenant() {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _g = LOCK.lock().unwrap();

        #[derive(Default)]
        struct Sink {
            rows: Mutex<Vec<String>>,
        }
        impl ScrapeAuditSink for Sink {
            fn record(&self, tenant: &str, event: ScrapeAuditEvent<'_>) {
                let row = match event {
                    ScrapeAuditEvent::RobotsDenied { url } => format!("{tenant}|robots|{url}"),
                    ScrapeAuditEvent::Blocked { url, engine, reason } => {
                        format!("{tenant}|blocked|{url}|{engine}|{reason}")
                    }
                };
                self.rows.lock().unwrap().push(row);
            }
        }
        let sink = Arc::new(Sink::default());
        register_scrape_audit_sink(sink.clone());

        // A stamped tenant is witnessed.
        audit_scrape("kivi", ScrapeAuditEvent::RobotsDenied { url: "https://x.com/p" });
        audit_scrape(
            "kivi",
            ScrapeAuditEvent::Blocked { url: "https://x.com/q", engine: "browser", reason: "turnstile" },
        );
        // An UNSTAMPED tenant is a noop (never a cross-tenant row).
        audit_scrape("", ScrapeAuditEvent::RobotsDenied { url: "https://x.com/z" });

        let rows = sink.rows.lock().unwrap().clone();
        assert_eq!(rows, vec![
            "kivi|robots|https://x.com/p".to_string(),
            "kivi|blocked|https://x.com/q|browser|turnstile".to_string(),
        ]);
        clear_scrape_audit_sink();
    }

    #[test]
    fn body_cap_truncates_on_char_boundary() {
        let page = RawPage {
            status: 200,
            final_url: "https://e.x".into(),
            headers: BTreeMap::new(),
            body: "áéíóú".repeat(1000),
            from_cache: false,
            truncated: false,
            engine: "test".into(),
        }
        .capped(10);
        assert!(page.truncated);
        assert!(page.body.len() <= 10);
        // Still valid UTF-8 (did not split a multibyte char).
        assert!(std::str::from_utf8(page.body.as_bytes()).is_ok());
    }

    #[test]
    fn robots_longest_match_wins() {
        let robots = "User-agent: *\nDisallow: /private\nAllow: /private/public\n";
        assert!(!robots_path_allowed(robots, "/private/secret"));
        assert!(robots_path_allowed(robots, "/private/public/x"));
        assert!(robots_path_allowed(robots, "/open"));
    }

    #[test]
    fn robots_no_rules_allows() {
        assert!(robots_path_allowed("", "/anything"));
        assert!(robots_path_allowed("User-agent: Googlebot\nDisallow: /", "/x"));
    }

    #[test]
    fn split_url_extracts_parts() {
        let (s, h, p) = split_url("https://ex.com:8080/a/b?q=1#frag").unwrap();
        assert_eq!(s, "https");
        assert_eq!(h, "ex.com:8080");
        assert_eq!(p, "/a/b?q=1");
        assert!(split_url("ftp://x").is_none());
    }

    #[test]
    fn browser_engine_without_sidecar_is_typed_refusal() {
        let req = FetchRequest {
            url: "https://example.com".into(),
            engine: "browser".into(),
            impersonate: String::new(),
            proxy: String::new(),
            respect_robots: false,
            render_wait: String::new(),
            timeout: Duration::from_secs(2),
            body_limit: DEFAULT_BODY_LIMIT,
            tenant: String::new(),
        };
        // No enterprise fetcher registered in the unit test → OSS default.
        assert!(matches!(default_fetch(&req), Err(ScrapeError::NoBrowserSidecar)));
    }

    #[test]
    fn invalid_url_is_typed_refusal() {
        let req = FetchRequest {
            url: "notaurl".into(),
            engine: "impersonate".into(),
            impersonate: String::new(),
            proxy: String::new(),
            respect_robots: false,
            render_wait: String::new(),
            timeout: Duration::from_secs(2),
            body_limit: DEFAULT_BODY_LIMIT,
            tenant: String::new(),
        };
        assert!(matches!(default_fetch(&req), Err(ScrapeError::InvalidUrl(_))));
    }

    #[test]
    fn missing_url_argument_is_error() {
        let entry = ToolEntry {
            name: "F".into(),
            provider: "scrape_http".into(),
            timeout: String::new(),
            runtime: String::new(),
            resource_ref: String::new(),
            substrate: None,
            capacity: None,
            sandbox: None,
            max_results: None,
            output_schema: String::new(),
            effect_row: vec!["network".into(), "web".into()],
            parameters: Vec::new(),
            secret: String::new(),
            secret_partition: String::new(),
            source: crate::tool_registry::ToolSource::Program,
            is_streaming: false,
            scrape: Some(ScrapeConfig::default()),
        };
        let out = dispatch_scrape_outcome(&entry, "{}");
        assert!(!out.result.success);
        assert!(out.result.output.contains("url"));
    }
}
