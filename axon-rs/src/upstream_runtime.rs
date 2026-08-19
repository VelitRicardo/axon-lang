//! §Fase 80.d — the `upstream` runtime: dial + auth + reconnect + transcode.
//!
//! An `upstream` (§80.b) is the client dual of `socket`: axon dials OUT to a
//! third-party vendor (streaming STT, TTS, fused realtime speech) over a
//! persistent WebSocket, and the declared `map:` projection (§80.c, T849)
//! transcodes between the axon-facing typed session messages and the
//! vendor's wire frames. **This module is what makes "any vendor" true**: a
//! new provider is a new `upstream` DECLARATION (frame shape + projection),
//! never new Rust code, as long as it speaks JSON-envelope or binary frames
//! over WS.
//!
//! Responsibilities (design doc §7):
//! - **Config resolution** — `resolve:`/`secret:` are per-tenant config keys.
//!   The runtime never sees a URL or credential in the program; a
//!   [`UpstreamConfigResolver`] supplies both. The OSS default is
//!   [`EnvConfigResolver`] (`upstream.deepgram.url` → env
//!   `AXON_UPSTREAM_DEEPGRAM_URL`), mirroring `AXON_TOOL_BASE_URL` (§58.g);
//!   enterprise binds its per-tenant secret custody instead (§80.e).
//! - **Auth handshake** — the closed catalog from §80.a: `header` (secret as
//!   a header value, optional prefix), `query` (secret as a query param),
//!   `signed_url` (the resolved URL is already complete + signed).
//! - **Reconnect** — exponential backoff (doubling from `backoff_ms`,
//!   deterministic ±25% jitter), at most `max_attempts` redials,
//!   `on_exhausted: fail` (fail-closed: the consumer SEES the exhaustion as
//!   an event, never a silent hang).
//! - **Transcoding** — [`project_outbound`] / [`classify_inbound`], pure
//!   functions over the compiled projection. Inbound JSON payloads are open
//!   values the flow navigates totally (§73 `Json`) — the projection is a
//!   routing skeleton, not a codegen system.
//! - **Overflow** — when the VENDOR is the slow side, the outbound queue
//!   applies the declared policy: `drop_oldest` | `pause_upstream` | `fail`.
//! - **Lifecycle witnessing** — every `connected` / `reconnected` /
//!   `exhausted` transition flows through an [`UpstreamLifecycleWitness`]
//!   BEFORE it takes effect; a witness refusal aborts the dial (the §76.c
//!   fail-closed pattern — an upstream that cannot witness its own lifecycle
//!   refuses to dial). The OSS default witness logs via `tracing` and never
//!   refuses; enterprise binds the audit chain (§80.e).
//!
//! **The honest line (D80.4):** duality, credit discipline, and projection
//! totality are compiler-proved up to the wire. This module DEFENDS across
//! the trust boundary (overflow policy, fail-closed reconnect, witnessed
//! lifecycle) and surfaces every frame outside the declared projection as
//! an explicit event — it does not claim to prove the vendor's side sound.

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axon_frontend::ir_nodes::{IRUpstream, IRUpstreamMapRule};
use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex, Notify, OwnedSemaphorePermit, Semaphore};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

// ── Errors ──────────────────────────────────────────────────────────────────

/// Everything that can go wrong on the client leg. Every variant names the
/// upstream so multi-vendor programs stay diagnosable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamError {
    /// `resolve:` key has no value in the bound resolver (config hole).
    MissingConfig { upstream: String, key: String },
    /// `secret:` key has no value in the bound resolver.
    MissingSecret { upstream: String, key: String },
    /// The lifecycle witness refused — fail-closed, the dial does not happen.
    UnwitnessedLifecycle { upstream: String, detail: String },
    /// TCP/TLS/WS handshake failure (one attempt; the reconnect loop may retry).
    Dial { upstream: String, detail: String },
    /// The resolved URL could not be turned into a client request.
    BadUrl { upstream: String, detail: String },
    /// An outbound message with no `send` rule in the projection. The §80.c
    /// checker makes this unrepresentable for compiled programs; the runtime
    /// still refuses (defence in depth for hand-built specs).
    UnmappedOutbound { upstream: String, message: String },
    /// The declared overflow policy was `fail` and the outbound queue is full.
    Overflow { upstream: String },
    /// The connection is gone and the reconnect budget is exhausted.
    Exhausted { upstream: String, attempts: u32 },
    /// The handle is closed (driver task ended).
    Closed { upstream: String },
    /// §Fase 114.u — the lease over this upstream's resource is no longer
    /// held: a dial is a USE of the channel, and a post-expiry use is the
    /// CT-2 Anchor Breach (the §113.d/§114.f law, now on the client leg).
    LeaseBreach { upstream: String, detail: String },
}

impl fmt::Display for UpstreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpstreamError::MissingConfig { upstream, key } => {
                write!(f, "upstream '{upstream}': no value for resolve key '{key}' (set the per-tenant config or the AXON_* env fallback)")
            }
            UpstreamError::MissingSecret { upstream, key } => {
                write!(f, "upstream '{upstream}': no value for secret key '{key}'")
            }
            UpstreamError::UnwitnessedLifecycle { upstream, detail } => {
                write!(f, "upstream '{upstream}': lifecycle witness refused — refusing to dial (fail-closed): {detail}")
            }
            UpstreamError::Dial { upstream, detail } => write!(f, "upstream '{upstream}': dial failed: {detail}"),
            UpstreamError::BadUrl { upstream, detail } => write!(f, "upstream '{upstream}': bad resolved URL: {detail}"),
            UpstreamError::UnmappedOutbound { upstream, message } => {
                write!(f, "upstream '{upstream}': message '{message}' has no `send` projection rule")
            }
            UpstreamError::Overflow { upstream } => {
                write!(f, "upstream '{upstream}': outbound queue full and overflow policy is `fail`")
            }
            UpstreamError::Exhausted { upstream, attempts } => {
                write!(f, "upstream '{upstream}': reconnect budget exhausted after {attempts} attempts (on_exhausted: fail)")
            }
            UpstreamError::Closed { upstream } => write!(f, "upstream '{upstream}': connection closed"),
            UpstreamError::LeaseBreach { upstream, detail } => {
                write!(f, "upstream '{upstream}': {detail}")
            }
        }
    }
}

impl std::error::Error for UpstreamError {}

// ── Config resolution (the §58.g "config, not code" seam) ──────────────────

/// Supplies the two per-tenant values the program deliberately cannot name:
/// the vendor URL (`resolve:`) and the credential (`secret:`). Enterprise
/// implements this over its secret custody (§75/§80.e); OSS defaults to env.
pub trait UpstreamConfigResolver: Send + Sync {
    fn resolve(&self, key: &str) -> Option<String>;
    fn reveal_secret(&self, key: &str) -> Option<String>;
}

/// `upstream.deepgram.url` → env `AXON_UPSTREAM_DEEPGRAM_URL` — the same
/// env-fallback convention as `AXON_TOOL_BASE_URL` (§58.g), generalised:
/// `AXON_` + uppercase(key) with `.`/`-` → `_`.
pub struct EnvConfigResolver;

/// Pure key→env-var mapping (unit-tested; the resolver is just this + read).
pub fn env_var_for_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 5);
    out.push_str("AXON_");
    for c in key.chars() {
        match c {
            '.' | '-' => out.push('_'),
            c => out.push(c.to_ascii_uppercase()),
        }
    }
    out
}

impl UpstreamConfigResolver for EnvConfigResolver {
    fn resolve(&self, key: &str) -> Option<String> {
        std::env::var(env_var_for_key(key)).ok().filter(|v| !v.is_empty())
    }
    fn reveal_secret(&self, key: &str) -> Option<String> {
        std::env::var(env_var_for_key(key)).ok().filter(|v| !v.is_empty())
    }
}

// ── Lifecycle witnessing (the §76.c fail-closed pattern) ───────────────────

/// One lifecycle transition, witnessed BEFORE it takes effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamLifecycle {
    /// First successful dial of this handle.
    Connected { attempt: u32 },
    /// A successful re-dial after a drop.
    Reconnected { attempt: u32 },
    /// The reconnect budget ran out — the upstream is giving up (fail-closed).
    Exhausted { attempts: u32 },
}

/// One boxed witness future — the §76.c fail-closed pattern is inherently
/// asynchronous for a real audit backend (the durable append must SUCCEED
/// before the lifecycle transition proceeds), so the trait speaks futures
/// without forcing an `async-trait` dependency on implementors.
pub type WitnessFuture<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>>;

/// The witness seam enterprise binds to its audit chain (§80.e): a `Connected`
/// / `Reconnected` refusal ABORTS the dial (an upstream that cannot witness
/// its own lifecycle refuses to dial); an `Exhausted` refusal cannot un-exhaust
/// the budget — it is logged and the exhaustion proceeds (the failure is
/// already the terminal state; suppressing it would trade one unwitnessed
/// event for a silent hang).
pub trait UpstreamLifecycleWitness: Send + Sync {
    fn witness<'a>(&'a self, upstream: &'a str, event: &'a UpstreamLifecycle) -> WitnessFuture<'a>;
}

/// OSS default: log via `tracing`, never refuse. Enterprise replaces this
/// with the fail-closed audit-backed witness.
pub struct TracingLifecycleWitness;

impl UpstreamLifecycleWitness for TracingLifecycleWitness {
    fn witness<'a>(&'a self, upstream: &'a str, event: &'a UpstreamLifecycle) -> WitnessFuture<'a> {
        Box::pin(async move {
            tracing::info!(upstream, ?event, "upstream lifecycle");
            Ok(())
        })
    }
}

// ── Pure transcoding (the §80.a contract, both directions) ─────────────────

/// An outbound payload from flow code: JSON for `as json` rules, raw bytes
/// for `as binary` rules (audio).
#[derive(Debug, Clone, PartialEq)]
pub enum OutboundPayload {
    Json(serde_json::Value),
    Bytes(Vec<u8>),
}

/// One classified inbound frame: the session message the vendor frame maps
/// to, plus its payload (open `Json` for json rules — §73 total navigation —
/// or raw bytes for the binary rule).
#[derive(Debug, Clone, PartialEq)]
pub enum InboundPayload {
    Json(serde_json::Value),
    Bytes(Vec<u8>),
}

/// What the consumer receives from the handle. Frames outside the declared
/// projection are EXPLICIT events, never silently dropped (D80.4: we defend
/// and witness; we do not pretend the vendor is proved sound).
#[derive(Debug, Clone, PartialEq)]
pub enum UpstreamEvent {
    /// A classified inbound message.
    Message { message: String, payload: InboundPayload },
    /// An inbound frame no `receive` rule classifies — outside the declared
    /// projection. Narrowing the projection to only what you consume is
    /// legitimate (vendor housekeeping frames land here by choice); a
    /// malformed vendor stream or a stale declaration also lands here —
    /// either way it is explicit and countable, never silent.
    Unmapped { detail: String },
    /// The connection dropped and was re-established (attempt = redial count).
    Reconnected { attempt: u32 },
    /// Terminal: the reconnect budget is exhausted (`on_exhausted: fail`).
    Exhausted { attempts: u32 },
}

/// Project one outbound message to its wire frame per its `send` rule.
///
/// - `as binary` → one binary frame (raw passthrough; a JSON payload handed
///   to a binary rule is serialized to its UTF-8 bytes — the declaration
///   said "bytes on the wire", so bytes it is).
/// - `as json` (no tag) → the payload JSON VERBATIM — the flow builds the
///   vendor's exact wire shape (ElevenLabs `{"text": …}`, Gemini
///   `{"realtimeInput": …}`); the projection adds nothing.
/// - `as json tag "X"` → the payload object with `"type": "X"` injected
///   (the Deepgram-control / OpenAI-Realtime envelope family). A non-object
///   payload is wrapped as `{"type": "X", "payload": <value>}` — the tag
///   must ride SOMEWHERE, and a bare scalar has no keys to merge into.
pub fn project_outbound(rule: &IRUpstreamMapRule, payload: &OutboundPayload) -> Message {
    match rule.framing.as_str() {
        "binary" => match payload {
            // §Fase 117.c — tungstenite 0.26+ carries frame payloads as
            // `bytes::Bytes` / `Utf8Bytes` rather than `Vec<u8>` / `String`,
            // so the wire types convert at this boundary. Purely
            // representational: the same bytes go out.
            OutboundPayload::Bytes(b) => Message::Binary(b.clone().into()),
            OutboundPayload::Json(v) => Message::Binary(v.to_string().into_bytes().into()),
        },
        _ => {
            let body = match payload {
                OutboundPayload::Json(v) => v.clone(),
                OutboundPayload::Bytes(b) => serde_json::Value::String(String::from_utf8_lossy(b).into_owned()),
            };
            let out = match &rule.tag {
                None => body,
                Some(tag) => match body {
                    serde_json::Value::Object(mut m) => {
                        m.insert("type".to_string(), serde_json::Value::String(tag.clone()));
                        serde_json::Value::Object(m)
                    }
                    other => serde_json::json!({ "type": tag, "payload": other }),
                },
            };
            Message::Text(out.to_string().into())
        }
    }
}

/// Classify one inbound wire frame against the `receive` rules.
///
/// - A binary frame matches the (unique, §80.c-enforced) `receive … as
///   binary` rule.
/// - A text frame is parsed as JSON and matched in TWO passes: equality
///   discriminators first (`when "f" = "v"`, defaulting to `"type" =
///   <MessageName>` when no `when` was written), then field-PRESENCE
///   discriminators (`when "f"` — vendors like Gemini Live mark frame
///   kinds by which key exists). Specific-before-broad keeps dispatch
///   deterministic when both shapes target the same field. The WHOLE
///   vendor body is the payload — the flow navigates it as §73 `Json`.
///
/// `None` ⇒ no rule matches — a frame OUTSIDE the declared projection,
/// surfaced by the driver as [`UpstreamEvent::Unmapped`] (narrowing the
/// projection to only what you consume is legitimate; a malformed vendor
/// stream also lands here — either way it is explicit, never silent).
pub fn classify_inbound(rules: &[IRUpstreamMapRule], frame: &Message) -> Option<(String, InboundPayload)> {
    match frame {
        Message::Binary(b) => rules
            .iter()
            .find(|r| r.direction == "receive" && r.framing == "binary")
            .map(|r| (r.message.clone(), InboundPayload::Bytes(b.to_vec()))),
        Message::Text(t) => {
            let body: serde_json::Value = serde_json::from_str(t).ok()?;
            let json_rules = || rules.iter().filter(|r| r.direction == "receive" && r.framing == "json");
            // Pass 1 — equality discriminators (specific).
            for r in json_rules() {
                let (field, expected) = match (&r.when_field, &r.when_value) {
                    (None, _) => ("type", r.message.as_str()),
                    (Some(f), Some(v)) => (f.as_str(), v.as_str()),
                    (Some(_), None) => continue, // presence rule — pass 2
                };
                if body.get(field).and_then(|v| v.as_str()) == Some(expected) {
                    return Some((r.message.clone(), InboundPayload::Json(body)));
                }
            }
            // Pass 2 — presence discriminators (broad).
            for r in json_rules() {
                if let (Some(f), None) = (&r.when_field, &r.when_value) {
                    if body.get(f).is_some() {
                        return Some((r.message.clone(), InboundPayload::Json(body)));
                    }
                }
            }
            None
        }
        _ => None, // ping/pong/close are transport, not protocol.
    }
}

// ── Auth + backoff (pure, unit-tested) ──────────────────────────────────────

/// Build the dial request per the declared auth handshake. Pure so the three
/// catalog shapes are testable without a socket.
pub fn build_dial_request(
    upstream: &str,
    url: &str,
    auth_kind: &str,
    auth_name: Option<&str>,
    auth_prefix: Option<&str>,
    secret: &str,
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, UpstreamError> {
    let final_url = if auth_kind == "query" {
        let param = auth_name.unwrap_or("token");
        let sep = if url.contains('?') { '&' } else { '?' };
        format!("{url}{sep}{param}={secret}")
    } else {
        url.to_string()
    };
    let mut req = final_url.into_client_request().map_err(|e| UpstreamError::BadUrl {
        upstream: upstream.to_string(),
        detail: e.to_string(),
    })?;
    if auth_kind == "header" {
        let name = auth_name.unwrap_or("Authorization");
        let value = format!("{}{}", auth_prefix.unwrap_or(""), secret);
        let header_name: tokio_tungstenite::tungstenite::http::header::HeaderName =
            name.parse().map_err(|_| UpstreamError::BadUrl {
                upstream: upstream.to_string(),
                detail: format!("invalid auth header name '{name}'"),
            })?;
        req.headers_mut().insert(
            header_name,
            HeaderValue::from_str(&value).map_err(|_| UpstreamError::BadUrl {
                upstream: upstream.to_string(),
                detail: "auth secret is not a valid header value".to_string(),
            })?,
        );
    }
    Ok(req)
}

/// The redial delay before attempt `n` (1-based): `backoff_ms · 2^(n-1)`,
/// capped at 30 s, with a DETERMINISTIC ±25% jitter derived from the attempt
/// number (reproducible in tests; still de-synchronises a fleet because each
/// process adds its connection epoch downstream).
pub fn backoff_delay(backoff_ms: i64, attempt: u32) -> Duration {
    let base = (backoff_ms.max(1) as u64).saturating_mul(1u64 << attempt.saturating_sub(1).min(20));
    let capped = base.min(30_000);
    // xorshift-style scramble of the attempt for a stable pseudo-jitter.
    let mut x = attempt as u64 ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    let jitter_pct = (x % 51) as i64 - 25; // −25 … +25
    let jittered = capped as i64 + (capped as i64 * jitter_pct) / 100;
    Duration::from_millis(jittered.max(1) as u64)
}

// ── The overflow queue (vendor is the slow side) ────────────────────────────

/// Bounded outbound queue applying the declared `overflow:` policy. The
/// §80.c checker admits `drop_oldest` / `pause_upstream` / `fail`; `fail` is
/// also the undeclared default (design doc §1: no silently-lossy audio
/// unless the adopter opts in).
struct OverflowQueue {
    inner: Mutex<VecDeque<Message>>,
    notify: Notify,
    capacity: usize,
    policy: String,
    closed: AtomicBool,
}

impl OverflowQueue {
    fn new(capacity: usize, policy: String) -> Self {
        OverflowQueue {
            inner: Mutex::new(VecDeque::with_capacity(capacity)),
            notify: Notify::new(),
            capacity,
            policy,
            closed: AtomicBool::new(false),
        }
    }

    /// Enqueue per policy. `drop_oldest` evicts the front (and reports how
    /// many were shed); `pause_upstream` awaits room (true producer
    /// backpressure); `fail` errors immediately.
    async fn push(&self, upstream: &str, msg: Message) -> Result<usize, UpstreamError> {
        loop {
            if self.closed.load(Ordering::Acquire) {
                return Err(UpstreamError::Closed { upstream: upstream.to_string() });
            }
            let mut q = self.inner.lock().await;
            if q.len() < self.capacity {
                q.push_back(msg);
                drop(q);
                self.notify.notify_waiters();
                return Ok(0);
            }
            match self.policy.as_str() {
                "drop_oldest" => {
                    let mut shed = 0usize;
                    while q.len() >= self.capacity {
                        q.pop_front();
                        shed += 1;
                    }
                    q.push_back(msg);
                    drop(q);
                    self.notify.notify_waiters();
                    return Ok(shed);
                }
                "pause_upstream" => {
                    drop(q);
                    self.notify.notified().await;
                    // loop — re-check capacity under the lock.
                }
                _ => return Err(UpstreamError::Overflow { upstream: upstream.to_string() }),
            }
        }
    }

    async fn pop(&self) -> Option<Message> {
        loop {
            {
                let mut q = self.inner.lock().await;
                if let Some(m) = q.pop_front() {
                    drop(q);
                    self.notify.notify_waiters();
                    return Some(m);
                }
            }
            if self.closed.load(Ordering::Acquire) {
                return None;
            }
            self.notify.notified().await;
        }
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }
}

// ── The handle + driver ──────────────────────────────────────────────────────

/// Default outbound-queue capacity when the declaration carries no
/// `backpressure: credit(n)` — matches the reference scaffold's credit(64)
/// order of magnitude without inventing a new constant per call site.
const DEFAULT_QUEUE_CAPACITY: usize = 64;

/// A live (or reconnecting) upstream connection. `send` projects + enqueues
/// per the overflow policy; `events` yields classified inbound messages,
/// unmapped-frame events, reconnections, and the terminal exhaustion.
pub struct UpstreamHandle {
    name: String,
    rules: Arc<Vec<IRUpstreamMapRule>>,
    queue: Arc<OverflowQueue>,
    events: mpsc::Receiver<UpstreamEvent>,
    driver: tokio::task::JoinHandle<()>,
    /// §Fase 114.u — the instance permit under the resource's `capacity`
    /// bound. Held for the LIFE of the handle (reconnects re-dial the same
    /// instance, they do not mint a new one); dropping the handle releases
    /// the slot. `None` for un-resourced upstreams — unbounded, the
    /// pre-114.u behaviour.
    _instance_permit: Option<OwnedSemaphorePermit>,
}

impl fmt::Debug for UpstreamHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpstreamHandle")
            .field("name", &self.name)
            .field("rules", &self.rules.len())
            .finish_non_exhaustive()
    }
}

impl UpstreamHandle {
    /// Project `message` through its `send` rule and enqueue it. Errors:
    /// unmapped message (defence in depth under T849), queue overflow under
    /// `fail`, or a closed connection.
    pub async fn send(&self, message: &str, payload: OutboundPayload) -> Result<(), UpstreamError> {
        let rule = self
            .rules
            .iter()
            .find(|r| r.direction == "send" && r.message == message)
            .ok_or_else(|| UpstreamError::UnmappedOutbound {
                upstream: self.name.clone(),
                message: message.to_string(),
            })?;
        let frame = project_outbound(rule, &payload);
        let shed = self.queue.push(&self.name, frame).await?;
        if shed > 0 {
            tracing::warn!(upstream = %self.name, shed, "overflow drop_oldest shed outbound frames");
        }
        Ok(())
    }

    /// Receive the next event. `None` after the terminal event when the
    /// driver has ended.
    pub async fn recv(&mut self) -> Option<UpstreamEvent> {
        self.events.recv().await
    }

    /// Close the outbound side and stop the driver.
    pub fn close(&self) {
        self.queue.close();
        self.driver.abort();
    }
}

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

// ── §Fase 114.u — the connection-INSTANCE bound ─────────────────────────────
//
// `upstream X { resource: R }` + `resource R { capacity: N }` ⇒ at most N
// concurrently-dialed instances of X in this process. Frames are already
// flow-controlled by `backpressure_credit`; capacity bounds CONNECTIONS —
// making it frames would state one fact twice (founder-ratified, §114.u).
//
// Same documented limit as §114.e: the semaphore is in-memory / per-process.
// Across horizontally-scaled replicas `capacity: N` is a per-process bound; a
// true global bound needs a distributed semaphore (future fase).
static INSTANCE_BOUNDS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, (i64, Arc<Semaphore>)>>,
> = std::sync::OnceLock::new();

/// The per-upstream instance semaphore under `capacity`. Keyed by upstream
/// name; a REDEPLOY that changes the capacity replaces the semaphore (new
/// dials ride the new bound; outstanding permits release into the old one,
/// which `Arc` keeps alive until they drop).
fn instance_semaphore(upstream: &str, capacity: i64) -> Arc<Semaphore> {
    let map = INSTANCE_BOUNDS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut map = map.lock().unwrap_or_else(|p| p.into_inner());
    match map.get(upstream) {
        Some((cap, sem)) if *cap == capacity => Arc::clone(sem),
        _ => {
            let sem = Arc::new(Semaphore::new(usize::try_from(capacity).unwrap_or(0).max(0)));
            map.insert(upstream.to_string(), (capacity, Arc::clone(&sem)));
            sem
        }
    }
}

/// Dial an upstream from its compiled declaration. Resolves `resolve:` +
/// `secret:` through the bound resolver, witnesses `Connected` (fail-closed),
/// then spawns the driver (pump + reconnect loop) and returns the handle.
///
/// The FIRST dial is awaited here so a config hole / refused witness / dead
/// vendor surfaces as an immediate `Err` — a voice agent that cannot reach
/// its STT vendor must fail its deploy health-check, not its first caller.
pub async fn dial_upstream(
    spec: &IRUpstream,
    resolver: &dyn UpstreamConfigResolver,
    witness: Arc<dyn UpstreamLifecycleWitness>,
    lease: Option<&crate::resource_lease::ResourceLeaseGuard>,
) -> Result<UpstreamHandle, UpstreamError> {
    let name = spec.name.clone();

    // §Fase 114.u — a DIAL is a USE of the channel's resource. When a lease
    // guard governs it, charge BEFORE anything else: a post-expiry dial is
    // the CT-2 Anchor Breach, fail-closed (the §113.d/§114.f law on the
    // client leg). `None` for every caller without lease governance — the
    // pre-114.u behaviour, and the same signature convention as §114.e's
    // channel semaphores.
    if let Some(guard) = lease {
        if !spec.resource_ref.is_empty() {
            if let Err(breach) = guard.charge(&spec.resource_ref) {
                return Err(UpstreamError::LeaseBreach {
                    upstream: name.clone(),
                    detail: breach.to_string(),
                });
            }
        }
    }

    let url = resolver.resolve(&spec.resolve).ok_or_else(|| UpstreamError::MissingConfig {
        upstream: name.clone(),
        key: spec.resolve.clone(),
    })?;
    let secret = if spec.auth_kind == "signed_url" {
        String::new() // the URL already carries its signature (§80.a).
    } else {
        resolver.reveal_secret(&spec.secret).ok_or_else(|| UpstreamError::MissingSecret {
            upstream: name.clone(),
            key: spec.secret.clone(),
        })?
    };

    // §Fase 114.u — acquire the connection-INSTANCE permit under the
    // resource's `capacity`. The N+1th dial WAITS for a slot (the §114.e
    // held-across-requests semantics: a bound that queues, not one that
    // lies by refusing). Config holes fail BEFORE this point so a
    // misconfigured upstream never queues.
    let instance_permit = match spec.capacity {
        Some(n) if !spec.resource_ref.is_empty() && n > 0 => Some(
            instance_semaphore(&name, n)
                .acquire_owned()
                .await
                .map_err(|_| UpstreamError::Closed { upstream: name.clone() })?,
        ),
        _ => None,
    };

    // Fail-closed witness BEFORE the first dial: the durable audit append
    // must succeed or the connection never happens.
    witness
        .witness(&name, &UpstreamLifecycle::Connected { attempt: 0 })
        .await
        .map_err(|detail| UpstreamError::UnwitnessedLifecycle { upstream: name.clone(), detail })?;

    let request = build_dial_request(
        &name,
        &url,
        &spec.auth_kind,
        spec.auth_name.as_deref(),
        spec.auth_prefix.as_deref(),
        &secret,
    )?;
    let (ws, _resp) = connect_async(request).await.map_err(|e| UpstreamError::Dial {
        upstream: name.clone(),
        detail: e.to_string(),
    })?;

    let rules = Arc::new(spec.map.clone());
    let capacity = spec
        .backpressure_credit
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_QUEUE_CAPACITY);
    let policy = spec.overflow.clone().unwrap_or_else(|| "fail".to_string());
    let queue = Arc::new(OverflowQueue::new(capacity, policy));
    let (event_tx, event_rx) = mpsc::channel::<UpstreamEvent>(capacity.max(16));

    let driver = tokio::spawn(drive_upstream(
        name.clone(),
        ws,
        Arc::clone(&rules),
        Arc::clone(&queue),
        event_tx,
        DialParams {
            url,
            secret,
            auth_kind: spec.auth_kind.clone(),
            auth_name: spec.auth_name.clone(),
            auth_prefix: spec.auth_prefix.clone(),
            backoff_ms: spec.reconnect.as_ref().map(|r| r.backoff_ms).unwrap_or(500),
            max_attempts: spec.reconnect.as_ref().map(|r| r.max_attempts).unwrap_or(0),
        },
        witness,
    ));

    Ok(UpstreamHandle { name, rules, queue, events: event_rx, driver, _instance_permit: instance_permit })
}

/// Everything the reconnect loop needs to re-dial without re-resolving
/// config (the resolver belongs to the dial moment; a rotated secret is
/// picked up on the NEXT `dial_upstream`, not silently mid-life).
struct DialParams {
    url: String,
    secret: String,
    auth_kind: String,
    auth_name: Option<String>,
    auth_prefix: Option<String>,
    backoff_ms: i64,
    max_attempts: i64,
}

/// The pump: outbound queue → wire, wire → classified events; on drop, the
/// reconnect loop (backoff + witnessed redial) until the budget exhausts.
async fn drive_upstream(
    name: String,
    mut ws: WsStream,
    rules: Arc<Vec<IRUpstreamMapRule>>,
    queue: Arc<OverflowQueue>,
    events: mpsc::Sender<UpstreamEvent>,
    params: DialParams,
    witness: Arc<dyn UpstreamLifecycleWitness>,
) {
    loop {
        // ── Pump this connection until it drops. ──
        let dropped = pump_connection(&name, &mut ws, &rules, &queue, &events).await;
        if !dropped {
            // Handle closed us — clean exit, nothing to reconnect.
            return;
        }
        // ── Reconnect loop. ──
        let mut attempt: u32 = 0;
        let reconnected = loop {
            if attempt as i64 >= params.max_attempts {
                // Budget exhausted — fail-closed. Witness refusal at this
                // point cannot un-exhaust the budget; log + proceed.
                let ev = UpstreamLifecycle::Exhausted { attempts: attempt };
                if let Err(e) = witness.witness(&name, &ev).await {
                    tracing::error!(upstream = %name, error = %e, "exhaustion could not be witnessed");
                }
                let _ = events.send(UpstreamEvent::Exhausted { attempts: attempt }).await;
                queue.close();
                return;
            }
            attempt += 1;
            tokio::time::sleep(backoff_delay(params.backoff_ms, attempt)).await;
            let ev = UpstreamLifecycle::Reconnected { attempt };
            if let Err(detail) = witness.witness(&name, &ev).await {
                // Fail-closed: an unwitnessable reconnect is not attempted.
                tracing::error!(upstream = %name, %detail, "reconnect refused by witness (fail-closed)");
                continue;
            }
            let request = match build_dial_request(
                &name,
                &params.url,
                &params.auth_kind,
                params.auth_name.as_deref(),
                params.auth_prefix.as_deref(),
                &params.secret,
            ) {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(upstream = %name, error = %e, "re-dial request build failed");
                    continue;
                }
            };
            match connect_async(request).await {
                Ok((new_ws, _)) => break Some((new_ws, attempt)),
                Err(e) => {
                    tracing::warn!(upstream = %name, attempt, error = %e, "re-dial failed");
                    continue;
                }
            }
        };
        match reconnected {
            Some((new_ws, attempt)) => {
                ws = new_ws;
                let _ = events.send(UpstreamEvent::Reconnected { attempt }).await;
            }
            None => return,
        }
    }
}

/// Pump one live connection. Returns `true` if the wire dropped (reconnect
/// candidate), `false` if the handle closed us (clean shutdown).
async fn pump_connection(
    name: &str,
    ws: &mut WsStream,
    rules: &Arc<Vec<IRUpstreamMapRule>>,
    queue: &Arc<OverflowQueue>,
    events: &mpsc::Sender<UpstreamEvent>,
) -> bool {
    loop {
        tokio::select! {
            outbound = queue.pop() => {
                match outbound {
                    Some(frame) => {
                        if let Err(e) = ws.send(frame).await {
                            tracing::warn!(upstream = %name, error = %e, "outbound send failed — wire dropped");
                            return true;
                        }
                    }
                    None => {
                        // Queue closed by the handle: drain finished, close the wire.
                        let _ = ws.close(None).await;
                        return false;
                    }
                }
            }
            inbound = ws.next() => {
                match inbound {
                    Some(Ok(frame @ (Message::Text(_) | Message::Binary(_)))) => {
                        match classify_inbound(rules, &frame) {
                            Some((message, payload)) => {
                                let _ = events.send(UpstreamEvent::Message { message, payload }).await;
                            }
                            None => {
                                let detail = match &frame {
                                    Message::Text(t) => format!("unclassifiable text frame: {}", &t[..t.len().min(200)]),
                                    _ => "unclassifiable binary frame (no `receive … as binary` rule)".to_string(),
                                };
                                let _ = events.send(UpstreamEvent::Unmapped { detail }).await;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => return true,
                    Some(Ok(_)) => { /* ping/pong — transport keepalive, tungstenite auto-replies */ }
                    Some(Err(e)) => {
                        tracing::warn!(upstream = %name, error = %e, "inbound stream error — wire dropped");
                        return true;
                    }
                }
            }
        }
    }
}

// ── Unit tests (pure parts) ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(direction: &str, message: &str, framing: &str) -> IRUpstreamMapRule {
        IRUpstreamMapRule {
            node_type: "upstream_map_rule",
            direction: direction.into(),
            message: message.into(),
            framing: framing.into(),
            tag: None,
            when_field: None,
            when_value: None,
        }
    }

    #[test]
    fn env_var_mapping_mirrors_the_tool_convention() {
        assert_eq!(env_var_for_key("upstream.deepgram.url"), "AXON_UPSTREAM_DEEPGRAM_URL");
        assert_eq!(env_var_for_key("upstream.eleven-labs.api_key"), "AXON_UPSTREAM_ELEVEN_LABS_API_KEY");
    }

    #[test]
    fn outbound_binary_is_raw_passthrough() {
        let r = rule("send", "AudioChunk", "binary");
        let m = project_outbound(&r, &OutboundPayload::Bytes(vec![1, 2, 3]));
        assert_eq!(m, Message::Binary(vec![1, 2, 3].into()));
    }

    #[test]
    fn outbound_json_without_tag_is_verbatim() {
        // The ElevenLabs family: the flow builds the vendor's exact shape.
        let r = rule("send", "TextChunk", "json");
        let m = project_outbound(&r, &OutboundPayload::Json(serde_json::json!({"text": "hola "})));
        let Message::Text(t) = m else { panic!("expected text frame") };
        assert_eq!(t, r#"{"text":"hola "}"#, "no envelope, no injected keys");
    }

    #[test]
    fn outbound_json_with_tag_injects_type_into_the_object() {
        // The Deepgram-control / OpenAI-Realtime family.
        let mut r = rule("send", "Configure", "json");
        r.tag = Some("Settings".into());
        let m = project_outbound(&r, &OutboundPayload::Json(serde_json::json!({"model": "nova-3"})));
        let Message::Text(t) = m else { panic!("expected text frame") };
        let v: serde_json::Value = serde_json::from_str(&t).unwrap();
        assert_eq!(v["type"], "Settings", "tag injected at top level");
        assert_eq!(v["model"], "nova-3", "payload keys merged, not nested");
        // A non-object payload still carries the tag, wrapped.
        let m2 = project_outbound(&r, &OutboundPayload::Json(serde_json::json!("scalar")));
        let Message::Text(t2) = m2 else { panic!() };
        let v2: serde_json::Value = serde_json::from_str(&t2).unwrap();
        assert_eq!((v2["type"].as_str(), v2["payload"].as_str()), (Some("Settings"), Some("scalar")));
    }

    #[test]
    fn inbound_presence_discriminator_classifies_after_equality() {
        // The Gemini-Live family: frame kind = which key exists. An eq rule
        // and a presence rule coexist; specific (eq) wins over broad.
        let mut server_content = rule("receive", "ServerContent", "json");
        server_content.when_field = Some("serverContent".into());
        server_content.when_value = None; // presence
        let mut setup_done = rule("receive", "SetupComplete", "json");
        setup_done.when_field = Some("setupComplete".into());
        setup_done.when_value = None; // presence
        let rules = vec![server_content, setup_done];

        let frame = Message::Text(r#"{"serverContent":{"modelTurn":{}}}"#.into());
        assert_eq!(classify_inbound(&rules, &frame).unwrap().0, "ServerContent");
        let frame2 = Message::Text(r#"{"setupComplete":{}}"#.into());
        assert_eq!(classify_inbound(&rules, &frame2).unwrap().0, "SetupComplete");
    }

    #[test]
    fn inbound_json_classifies_on_discriminator_with_default() {
        let mut results = rule("receive", "Transcript", "json");
        results.when_field = Some("type".into());
        results.when_value = Some("Results".into());
        let default_rule = rule("receive", "SpeechStarted", "json"); // default: "type" = "SpeechStarted"
        let rules = vec![results, default_rule];

        let frame = Message::Text(r#"{"type":"Results","channel":{"alternatives":[{"transcript":"hola"}]}}"#.into());
        let (msg, payload) = classify_inbound(&rules, &frame).expect("classified");
        assert_eq!(msg, "Transcript");
        let InboundPayload::Json(v) = payload else { panic!("json payload") };
        assert_eq!(v["channel"]["alternatives"][0]["transcript"], "hola", "whole body is the Json payload");

        let frame2 = Message::Text(r#"{"type":"SpeechStarted"}"#.into());
        assert_eq!(classify_inbound(&rules, &frame2).unwrap().0, "SpeechStarted", "default discriminator");

        let unknown = Message::Text(r#"{"type":"Metadata"}"#.into());
        assert!(classify_inbound(&rules, &unknown).is_none(), "unmatched frame surfaces as Unmapped upstream");
    }

    #[test]
    fn inbound_binary_needs_the_binary_rule() {
        let rules = vec![rule("receive", "AudioOut", "binary")];
        let (msg, payload) = classify_inbound(&rules, &Message::Binary(vec![9].into())).unwrap();
        assert_eq!(msg, "AudioOut");
        assert_eq!(payload, InboundPayload::Bytes(vec![9]));
        assert!(classify_inbound(&[], &Message::Binary(vec![9].into())).is_none());
    }

    #[test]
    fn dial_request_header_auth_carries_prefix() {
        let req = build_dial_request("U", "ws://x.test/v1", "header", Some("Authorization"), Some("Token "), "s3cr3t").unwrap();
        assert_eq!(req.headers().get("Authorization").unwrap(), "Token s3cr3t");
    }

    #[test]
    fn dial_request_query_auth_appends_param() {
        let req = build_dial_request("U", "ws://x.test/v1?model=nova", "query", Some("token"), None, "k").unwrap();
        assert_eq!(req.uri().query(), Some("model=nova&token=k"));
        let req2 = build_dial_request("U", "ws://x.test/v1", "query", Some("key"), None, "k").unwrap();
        assert_eq!(req2.uri().query(), Some("key=k"));
    }

    #[test]
    fn dial_request_signed_url_dials_as_is() {
        let req = build_dial_request("U", "ws://x.test/v1?sig=abc", "signed_url", None, None, "").unwrap();
        assert_eq!(req.uri().query(), Some("sig=abc"));
        assert!(req.headers().get("Authorization").is_none());
    }

    #[test]
    fn backoff_doubles_capped_and_jittered_deterministically() {
        let d1 = backoff_delay(500, 1);
        let d2 = backoff_delay(500, 2);
        let d3 = backoff_delay(500, 3);
        // Within ±25% of 500 / 1000 / 2000.
        assert!((375..=625).contains(&(d1.as_millis() as u64)), "{d1:?}");
        assert!((750..=1250).contains(&(d2.as_millis() as u64)), "{d2:?}");
        assert!((1500..=2500).contains(&(d3.as_millis() as u64)), "{d3:?}");
        // Deterministic: same inputs, same delay.
        assert_eq!(backoff_delay(500, 2), backoff_delay(500, 2));
        // Cap at 30 s (+25% jitter ceiling).
        assert!(backoff_delay(500, 30).as_millis() as u64 <= 37_500);
    }
}
