//! AXON Runtime — Typed Channels (Fase 13.f.2 — Rust runtime parity).
//!
//! Direct port of `axon/runtime/channels/typed.py` (Fase 13.d). Provides
//! the runtime layer for `Channel<τ, q, ℓ, π>` — first-class affine
//! resources with π-calculus mobility (paper_mobile_channels.md).
//!
//! Layered alongside the existing daemon-supervisor `EventBus`
//! (broadcast semantics for lifecycle events) — this module owns its
//! own per-channel transport with FIFO single-consumer semantics
//! matching the Python reference exactly. The two layers do not share
//! underlying queues; both can coexist in a single process.
//!
//! Public surface:
//! - [`TypedChannelHandle`]   — runtime materialisation of an `IRChannel`
//! - [`Capability`]           — opaque token returned by `publish`,
//!                              consumed by `discover` (Publish-Ext)
//! - [`TypedChannelRegistry`] — name → handle map; bootstraps from
//!                              `axon_frontend::ir_nodes::IRProgram`
//! - [`TypedEventBus`]        — `emit` / `publish` / `discover`
//!                              orchestrator with schema validation,
//!                              QoS, capability gating
//!
//! Errors mirror compile-time diagnostics (Fase 13.b type checker) so a
//! misconfigured runtime cannot silently diverge from the static
//! guarantees.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::{mpsc, Mutex as AsyncMutex};

use axon_frontend::ir_nodes::{IRChannel, IRProgram};

// ═══════════════════════════════════════════════════════════════════
//  ERRORS — runtime-side mirrors of compile-time guarantees
// ═══════════════════════════════════════════════════════════════════

/// Runtime errors raised by the typed-channel layer.
///
/// Each variant mirrors a compile-time diagnostic so a program that
/// would have been rejected by the Fase 13.b type checker is also
/// rejected here as defence-in-depth (relevant for cross-process
/// publish/discover where the receiver cannot rerun static analysis).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypedChannelError {
    /// Channel name not in the registry.
    ChannelNotFound {
        name: String,
        registered: Vec<String>,
    },
    /// Payload type does not match the channel schema.
    ///
    /// The compile-time check (Fase 13.b `_check_emit`) catches this
    /// for statically-known programs; this runtime check is
    /// defence-in-depth.
    SchemaMismatch(String),
    /// `publish` lacks a shield (D8) or `discover` targets an
    /// unpublishable channel / forged capability.
    CapabilityGate(String),
    /// An affine/linear handle was used after consumption.
    ///
    /// Affine: at most one consumption (use OK; drop OK; reuse rejected).
    /// Linear: exactly one consumption (use required; reuse rejected).
    /// Persistent (`!Channel`): unrestricted reuse (no enforcement).
    LifetimeViolation { name: String, count: u32 },
    /// Underlying transport failure (closed channel, dropped sender).
    Transport(String),
}

impl fmt::Display for TypedChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypedChannelError::ChannelNotFound { name, registered } => write!(
                f,
                "channel '{name}' not in TypedChannelRegistry (registered: {registered:?})"
            ),
            TypedChannelError::SchemaMismatch(msg) => write!(f, "{msg}"),
            TypedChannelError::CapabilityGate(msg) => write!(f, "{msg}"),
            TypedChannelError::LifetimeViolation { name, count } => write!(
                f,
                "channel '{name}' is linear but has been consumed {count} times (linear ⇒ exactly once)"
            ),
            TypedChannelError::Transport(msg) => write!(f, "transport: {msg}"),
        }
    }
}

impl std::error::Error for TypedChannelError {}

/// `Result` alias used throughout the typed-channel layer.
pub type Result<T> = std::result::Result<T, TypedChannelError>;

// ═══════════════════════════════════════════════════════════════════
//  CAPABILITY — Publish-Ext token (paper §4.3)
// ═══════════════════════════════════════════════════════════════════

/// Opaque, single-extrusion-hop witness of a published channel.
///
/// A `publish c within σ` reduction returns one of these. The bearer
/// can `discover` the underlying handle through the bus. The runtime
/// attenuates the bearer's certainty envelope by `delta_pub` per hop
/// so published-then-republished handles strictly lose certainty on
/// every traversal — paper §6.2 ("no certainty laundering").
///
/// Capabilities are immutable (no setters); per-hop bookkeeping
/// happens in the bus when the capability is created (`publish`) and
/// consumed (`discover`).
#[derive(Debug, Clone, PartialEq)]
pub struct Capability {
    /// uuid4 — opaque to consumers.
    pub capability_id: String,
    /// The `IRChannel.name` being exposed.
    pub channel_name: String,
    /// σ-shield that mediated extrusion.
    pub shield_ref: String,
    /// Certainty penalty per hop (paper §3.4 lower bound: 0.05).
    pub delta_pub: f64,
    /// Wall-clock seconds since the Unix epoch.
    pub issued_at: f64,
}

// ═══════════════════════════════════════════════════════════════════
//  HANDLE — runtime materialisation of an IRChannel
// ═══════════════════════════════════════════════════════════════════

/// A live, typed channel handle — wraps the static schema (message
/// type, QoS, lifetime, persistence, shield ref) for runtime
/// enforcement.
///
/// The handle's `consumed_count` lets the bus enforce lifetime rules:
/// - linear     → must reach exactly 1 over the lifetime of the handle
/// - affine     → may stay at 0 (drop) but never exceed 1 per holder
/// - persistent → unbounded
///
/// At Fase 13.f.2 scope, the bus tracks consumption counters at the
/// handle level. Per-binding tracking (when `discover` yields fresh
/// aliases) follows the Python reference and is deferred to a
/// future sub-phase aligned with cross-process replay tokens.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedChannelHandle {
    pub name: String,
    /// Surface spelling — `Order` | `Channel<Order>` | …
    pub message: String,
    pub qos: String,
    pub lifetime: String,
    pub persistence: String,
    pub shield_ref: String,
    /// Incremented per emit/publish/discover.
    pub consumed_count: u32,
}

impl TypedChannelHandle {
    /// Construct a handle with Fase 13 defaults
    /// (`qos=at_least_once`, `lifetime=affine`, `persistence=ephemeral`,
    /// no shield).
    pub fn new(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            message: message.into(),
            qos: "at_least_once".to_string(),
            lifetime: "affine".to_string(),
            persistence: "ephemeral".to_string(),
            shield_ref: String::new(),
            consumed_count: 0,
        }
    }

    /// D8 — a channel is publishable iff it declared a shield gate.
    pub fn is_publishable(&self) -> bool {
        !self.shield_ref.is_empty()
    }

    /// Second-order: this channel transports another channel handle.
    pub fn carries_channel(&self) -> bool {
        self.message.starts_with("Channel<") && self.message.ends_with('>')
    }

    /// Unwrap one level of `Channel<…>` to find the carried type.
    ///
    /// Returns the leaf type for plain channels, or the immediately
    /// nested type for second-order channels. For triple-nesting
    /// `Channel<Channel<T>>`, returns `Channel<T>` (one unwrap).
    pub fn inner_message_type(&self) -> &str {
        if !self.carries_channel() {
            return &self.message;
        }
        &self.message["Channel<".len()..self.message.len() - 1]
    }

    /// Build a runtime handle from a lowered `IRChannel` (post-13.c).
    pub fn from_ir(ir: &IRChannel) -> Self {
        Self {
            name: ir.name.clone(),
            message: ir.message.clone(),
            qos: ir.qos.clone(),
            lifetime: ir.lifetime.clone(),
            persistence: ir.persistence.clone(),
            shield_ref: ir.shield_ref.clone(),
            consumed_count: 0,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
//  PAYLOAD + EVENT
// ═══════════════════════════════════════════════════════════════════

/// A typed payload — either a scalar value (JSON) or a channel handle
/// (mobility). The discriminator replaces Python's
/// `payload_is_handle: bool` keyword argument with a sum type that the
/// type system enforces.
#[derive(Debug, Clone)]
pub enum TypedPayload {
    Scalar(serde_json::Value),
    Handle(TypedChannelHandle),
}

impl TypedPayload {
    /// Convenience constructor for scalar payloads.
    pub fn scalar<V: Into<serde_json::Value>>(v: V) -> Self {
        TypedPayload::Scalar(v.into())
    }

    /// Convenience constructor for mobility (channel-as-value) payloads.
    pub fn handle(h: TypedChannelHandle) -> Self {
        TypedPayload::Handle(h)
    }

    pub fn is_handle(&self) -> bool {
        matches!(self, TypedPayload::Handle(_))
    }
}

/// One event flowing through a typed channel.
#[derive(Debug, Clone)]
pub struct TypedEvent {
    pub channel: String,
    pub payload: TypedPayload,
    pub event_id: String,
    pub timestamp_secs: f64,
}

// ═══════════════════════════════════════════════════════════════════
//  REGISTRY — name → handle map
// ═══════════════════════════════════════════════════════════════════

/// Authoritative map of channel name → [`TypedChannelHandle`].
///
/// Bootstraps from an `IRProgram` (post-13.c) so the registry is a
/// faithful runtime projection of the compiler's view. Hand-rolled
/// registration is also supported for tests and embedded runtimes.
#[derive(Debug, Default)]
pub struct TypedChannelRegistry {
    handles: HashMap<String, TypedChannelHandle>,
}

impl TypedChannelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a handle to the registry. Re-registering with the same name
    /// overwrites — useful for hot-reloads in dev workflows.
    pub fn register(&mut self, handle: TypedChannelHandle) {
        self.handles.insert(handle.name.clone(), handle);
    }

    /// Instantiate a handle from an `IRChannel` and register it.
    pub fn register_from_ir(&mut self, ir: &IRChannel) -> TypedChannelHandle {
        let handle = TypedChannelHandle::from_ir(ir);
        self.handles.insert(handle.name.clone(), handle.clone());
        handle
    }

    pub fn get(&self, name: &str) -> Result<&TypedChannelHandle> {
        self.handles
            .get(name)
            .ok_or_else(|| TypedChannelError::ChannelNotFound {
                name: name.to_string(),
                registered: self.names(),
            })
    }

    fn get_mut(&mut self, name: &str) -> Result<&mut TypedChannelHandle> {
        let registered = self.names();
        self.handles
            .get_mut(name)
            .ok_or_else(|| TypedChannelError::ChannelNotFound {
                name: name.to_string(),
                registered,
            })
    }

    pub fn has(&self, name: &str) -> bool {
        self.handles.contains_key(name)
    }

    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.handles.keys().cloned().collect();
        v.sort();
        v
    }

    pub fn len(&self) -> usize {
        self.handles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }
}

// ═══════════════════════════════════════════════════════════════════
//  SHIELD CHECKER PROTOCOL — minimal interface to ESK
// ═══════════════════════════════════════════════════════════════════

/// Predicate `(shield_name, handle) → covers?`.
///
/// Default impl returns `true` (no enforcement). Production callers
/// inject an ESK-aware checker that consults the actual
/// `ShieldDefinition` and looks up `κ(message_type)` for the given
/// handle. Delegating κ-extraction to the predicate keeps the typed-
/// channel layer agnostic of the ESK module (which holds
/// `TypeDefinition.compliance` metadata).
pub type ShieldComplianceFn = Arc<dyn Fn(&str, &TypedChannelHandle) -> bool + Send + Sync>;

fn default_compliance_check() -> ShieldComplianceFn {
    Arc::new(|_, _| true)
}

/// §Fase 124.c — derive the compliance predicate from the SAME
/// `IRProgram` the bus is built from: κ(payload type) must be covered
/// by κ(shield) before `publish` extrudes a capability (D8).
///
/// This is the runtime enforcement of the rule `axon-T1215` decides at
/// check time. Both exist deliberately: the checker refuses the
/// declaration before the program ever runs, and THIS predicate is the
/// fail-closed backstop for IR that never went through the checker
/// (hand-assembled programs, older compiled artifacts). Before §124.c,
/// `from_ir_program` handed every caller the permissive default — the
/// two production doors (`daemon.rs`, `runner.rs`) built buses where a
/// shield covering NOTHING satisfied a channel carrying PHI. That is
/// the presence-vs-exercise defect §122 spent a fase on, in the one
/// seam that extrudes capabilities to outside parties.
///
/// Semantics, fail-closed on the right side:
/// - payload type unknown to the IR, or κ(payload) empty → pass
///   (nothing regulated is carried; the shield-identity gate in
///   `publish` still applies).
/// - κ(payload) non-empty and the shield is unknown to the IR → REFUSE
///   (an unresolvable control covers nothing).
/// - otherwise → κ(payload) ⊆ κ(shield).
fn compliance_check_from_ir(ir: &IRProgram) -> ShieldComplianceFn {
    let type_kappa: HashMap<String, Vec<String>> = ir
        .types
        .iter()
        .map(|t| (t.name.clone(), t.compliance.clone()))
        .collect();
    let shield_kappa: HashMap<String, Vec<String>> = ir
        .shields
        .iter()
        .map(|s| (s.name.clone(), s.compliance.clone()))
        .collect();
    Arc::new(move |shield, handle| {
        // Same peel the checker applies for T1215 — one definition,
        // `axon_frontend::compliance`, so the two ends of this rule
        // cannot drift on what wraps a payload without changing its κ.
        let leaf = axon_frontend::compliance::peel_channel_payload(&handle.message);
        let required = match type_kappa.get(leaf) {
            Some(k) if !k.is_empty() => k,
            _ => return true,
        };
        match shield_kappa.get(shield) {
            Some(provided) => required.iter().all(|k| provided.contains(k)),
            None => false,
        }
    })
}

// ═══════════════════════════════════════════════════════════════════
//  PER-CHANNEL TRANSPORT — single-consumer FIFO
// ═══════════════════════════════════════════════════════════════════

/// Per-channel single-consumer FIFO queue (parallel to Python's
/// `InMemoryChannel(asyncio.Queue)`).
///
/// Sender and receiver are split so the bus owns both — `emit()`
/// pushes via the sender, `receive()` awaits via the receiver. The
/// receiver is held behind a `tokio::sync::Mutex` so concurrent
/// callers serialise into the queue (single-consumer FIFO matches
/// `at_least_once` / `queue` semantics from the paper).
struct ChannelTransport {
    tx: mpsc::UnboundedSender<TypedEvent>,
    rx: AsyncMutex<mpsc::UnboundedReceiver<TypedEvent>>,
    closed: AtomicBool,
}

impl ChannelTransport {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            tx,
            rx: AsyncMutex::new(rx),
            closed: AtomicBool::new(false),
        }
    }

    fn send(&self, event: TypedEvent) -> Result<()> {
        if self.closed.load(Ordering::Acquire) {
            return Err(TypedChannelError::Transport(
                "channel is closed".to_string(),
            ));
        }
        self.tx
            .send(event)
            .map_err(|e| TypedChannelError::Transport(format!("send failed: {e}")))
    }

    async fn recv(&self) -> Result<TypedEvent> {
        let mut rx = self.rx.lock().await;
        rx.recv().await.ok_or_else(|| {
            TypedChannelError::Transport("channel sender dropped".to_string())
        })
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }
}

// ═══════════════════════════════════════════════════════════════════
//  TYPED EVENT BUS — emit / publish / discover orchestrator
// ═══════════════════════════════════════════════════════════════════

/// Schema-aware, capability-gated event bus.
///
/// Owns its own per-channel transport (FIFO mpsc queues for
/// `at_least_once`/`queue`/`at_most_once`/`exactly_once`, multi-
/// subscriber for `broadcast`). Coexists with the daemon-supervisor
/// `EventBus` (`crate::event_bus`) without sharing transport — both
/// can run in the same process for different concerns.
///
/// Usage:
///
/// ```no_run
/// # use axon::runtime::channels::{TypedEventBus, TypedChannelHandle, TypedPayload};
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let bus = TypedEventBus::new();
/// bus.register({
///     let mut h = TypedChannelHandle::new("OrdersCreated", "Order");
///     h.shield_ref = "PublicBroker".into();
///     h
/// });
/// bus.emit("OrdersCreated", TypedPayload::scalar(serde_json::json!({"id": 1}))).await?;
/// let cap = bus.publish("OrdersCreated", "PublicBroker").await?;
/// let _handle = bus.discover(&cap).await?;
/// # Ok(())
/// # }
/// ```
pub struct TypedEventBus {
    registry: Mutex<TypedChannelRegistry>,
    transports: Mutex<HashMap<String, Arc<ChannelTransport>>>,
    broadcast_subs: Mutex<HashMap<String, Vec<mpsc::UnboundedSender<TypedEvent>>>>,
    capabilities: Mutex<HashMap<String, Capability>>,
    delivered_ids: Mutex<HashMap<String, HashSet<String>>>,
    compliance_check: ShieldComplianceFn,
}

impl Default for TypedEventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl TypedEventBus {
    /// Empty bus with the permissive default compliance check.
    pub fn new() -> Self {
        Self::with_compliance_check(default_compliance_check())
    }

    /// Empty bus with a caller-supplied compliance predicate.
    pub fn with_compliance_check(check: ShieldComplianceFn) -> Self {
        Self {
            registry: Mutex::new(TypedChannelRegistry::new()),
            transports: Mutex::new(HashMap::new()),
            broadcast_subs: Mutex::new(HashMap::new()),
            capabilities: Mutex::new(HashMap::new()),
            delivered_ids: Mutex::new(HashMap::new()),
            compliance_check: check,
        }
    }

    /// Bootstrap a bus from a fully-lowered `IRProgram` (post-13.c).
    /// Every `IRChannel` becomes a registered runtime handle.
    ///
    /// §Fase 124.c — the compliance predicate is derived from the SAME
    /// IR (see [`compliance_check_from_ir`]): `publish` refuses a
    /// shield whose κ does not cover the channel's payload κ. Callers
    /// needing a different policy inject it via
    /// [`from_ir_program_with`](Self::from_ir_program_with).
    pub fn from_ir_program(ir: &IRProgram) -> Self {
        Self::from_ir_program_with(ir, compliance_check_from_ir(ir))
    }

    /// Same as [`from_ir_program`](Self::from_ir_program) but with a
    /// caller-supplied compliance predicate.
    pub fn from_ir_program_with(ir: &IRProgram, check: ShieldComplianceFn) -> Self {
        let bus = Self::with_compliance_check(check);
        {
            let mut reg = bus.registry.lock().unwrap();
            for ch in &ir.channels {
                reg.register_from_ir(ch);
            }
        }
        bus
    }

    pub fn register(&self, handle: TypedChannelHandle) {
        self.registry.lock().unwrap().register(handle);
    }

    pub fn register_from_ir(&self, ir: &IRChannel) -> TypedChannelHandle {
        self.registry.lock().unwrap().register_from_ir(ir)
    }

    /// Snapshot of the handle for a channel.
    pub fn get_handle(&self, name: &str) -> Result<TypedChannelHandle> {
        self.registry.lock().unwrap().get(name).cloned()
    }

    pub fn channel_names(&self) -> Vec<String> {
        self.registry.lock().unwrap().names()
    }

    // ── EMIT (Chan-Output / Chan-Mobility, paper §3.1, §3.2) ──────

    /// Emit a value (or a channel handle for mobility) on a typed
    /// channel.
    ///
    /// Schema enforcement mirrors Fase 13.b's `_check_emit`:
    /// - second-order channel + scalar payload → [`TypedChannelError::SchemaMismatch`]
    /// - first-order channel + handle payload  → [`TypedChannelError::SchemaMismatch`]
    /// - second-order schema mismatch          → [`TypedChannelError::SchemaMismatch`]
    pub async fn emit(&self, channel: &str, payload: TypedPayload) -> Result<()> {
        let handle = self.get_handle(channel)?;
        Self::check_emit_schema(&handle, &payload)?;

        let event = TypedEvent {
            channel: channel.to_string(),
            payload,
            event_id: gen_uuid(),
            timestamp_secs: now_secs(),
        };

        self.dispatch(&handle, event)?;
        self.consume(channel)?;
        Ok(())
    }

    fn check_emit_schema(handle: &TypedChannelHandle, payload: &TypedPayload) -> Result<()> {
        match payload {
            TypedPayload::Handle(inner) => {
                if !handle.carries_channel() {
                    return Err(TypedChannelError::SchemaMismatch(format!(
                        "emit on '{}' (message: {}) received a channel handle, but the channel is not second-order — expected scalar payload",
                        handle.name, handle.message,
                    )));
                }
                let expected_inner = handle.inner_message_type();
                if inner.message != expected_inner {
                    return Err(TypedChannelError::SchemaMismatch(format!(
                        "emit on '{}' expects Channel<{}> but received handle for '{}' (second-order schema mismatch, paper §3.2)",
                        handle.name, expected_inner, inner.message,
                    )));
                }
                Ok(())
            }
            TypedPayload::Scalar(_) => {
                if handle.carries_channel() {
                    return Err(TypedChannelError::SchemaMismatch(format!(
                        "emit on '{}' (message: {}) requires a channel handle but received scalar — pass TypedPayload::Handle(handle) for mobility",
                        handle.name, handle.message,
                    )));
                }
                Ok(())
            }
        }
    }

    fn dispatch(&self, handle: &TypedChannelHandle, event: TypedEvent) -> Result<()> {
        match handle.qos.as_str() {
            "broadcast" => {
                let subs = self.broadcast_subs.lock().unwrap();
                if let Some(queues) = subs.get(&handle.name) {
                    for queue in queues {
                        // Best-effort fan-out — a dropped subscriber
                        // queue must not poison the publish path.
                        let _ = queue.send(event.clone());
                    }
                }
                Ok(())
            }
            "at_most_once" => {
                let transport = self.transport_for(&handle.name);
                // Best-effort: ignore put failures (drop silently per AMO).
                let _ = transport.send(event);
                Ok(())
            }
            "exactly_once" => {
                {
                    let mut delivered = self.delivered_ids.lock().unwrap();
                    let seen = delivered.entry(handle.name.clone()).or_default();
                    if seen.contains(&event.event_id) {
                        return Ok(());
                    }
                    seen.insert(event.event_id.clone());
                }
                let transport = self.transport_for(&handle.name);
                transport.send(event)
            }
            _ => {
                // at_least_once (default) and queue both delegate to
                // the single-consumer FIFO transport. Difference is
                // per-handle, not per-event, and surfaces in
                // subscribe semantics for `queue` (single-consumer).
                let transport = self.transport_for(&handle.name);
                transport.send(event)
            }
        }
    }

    fn transport_for(&self, channel: &str) -> Arc<ChannelTransport> {
        let mut transports = self.transports.lock().unwrap();
        transports
            .entry(channel.to_string())
            .or_insert_with(|| Arc::new(ChannelTransport::new()))
            .clone()
    }

    fn consume(&self, channel: &str) -> Result<()> {
        let mut reg = self.registry.lock().unwrap();
        let handle = reg.get_mut(channel)?;
        handle.consumed_count += 1;
        if handle.lifetime == "linear" && handle.consumed_count > 1 {
            return Err(TypedChannelError::LifetimeViolation {
                name: handle.name.clone(),
                count: handle.consumed_count,
            });
        }
        // affine and persistent impose no upper bound on emits per
        // handle definition; per-binding affinity is tracked
        // separately (deferred — parity with Python 13.d note).
        Ok(())
    }

    // ── PUBLISH (Publish-Ext, paper §4.3) ─────────────────────────

    /// Extrude a channel handle through a shield, returning a
    /// [`Capability`] that downstream callers can `discover`.
    ///
    /// Compile-time D8 already requires `within Shield`; the runtime
    /// also rejects empty/missing shields so an embedded program
    /// cannot bypass the gate by clearing the field.
    ///
    /// The compliance predicate (injected via constructor) verifies
    /// that `shield.compliance ⊇ κ(channel.message_type)`. Default is
    /// permissive; production hooks an ESK-aware checker.
    pub async fn publish(&self, channel: &str, shield: &str) -> Result<Capability> {
        if shield.is_empty() {
            return Err(TypedChannelError::CapabilityGate(format!(
                "publish '{channel}' requires a non-empty shield (D8 — capability extrusion is shield-mediated)"
            )));
        }
        let handle = self.get_handle(channel)?;
        if !handle.is_publishable() {
            return Err(TypedChannelError::CapabilityGate(format!(
                "channel '{channel}' is not publishable: its definition declares no shield_ref (D8)"
            )));
        }
        if shield != handle.shield_ref {
            return Err(TypedChannelError::CapabilityGate(format!(
                "publish '{channel}' requires shield '{}' (declared on the channel) but received '{shield}'",
                handle.shield_ref
            )));
        }
        if !(self.compliance_check)(shield, &handle) {
            return Err(TypedChannelError::CapabilityGate(format!(
                "shield '{shield}' does not cover compliance required by channel '{channel}'"
            )));
        }

        let cap = Capability {
            capability_id: gen_uuid(),
            channel_name: channel.to_string(),
            shield_ref: shield.to_string(),
            delta_pub: 0.05,
            issued_at: now_secs(),
        };
        self.capabilities
            .lock()
            .unwrap()
            .insert(cap.capability_id.clone(), cap.clone());
        Ok(cap)
    }

    // ── DISCOVER (paper §3.4 dual) ────────────────────────────────

    /// Consume a [`Capability`] and return the underlying handle.
    /// One-shot: subsequent calls with the same capability are
    /// rejected.
    pub async fn discover(&self, capability: &Capability) -> Result<TypedChannelHandle> {
        let removed = {
            let mut caps = self.capabilities.lock().unwrap();
            caps.remove(&capability.capability_id)
        };
        if removed.is_none() {
            return Err(TypedChannelError::CapabilityGate(format!(
                "capability '{}' has been revoked or was never issued by this bus",
                capability.capability_id,
            )));
        }
        self.get_handle(&capability.channel_name)
    }

    // ── SUBSCRIBE — broadcast and queue ──────────────────────────

    /// Register a fresh queue for a `qos: broadcast` channel; returns
    /// a receiver so the consumer can `recv().await` per event.
    /// Multiple subscribers each receive every emitted event.
    pub fn subscribe_broadcast(
        &self,
        channel: &str,
    ) -> Result<mpsc::UnboundedReceiver<TypedEvent>> {
        let handle = self.get_handle(channel)?;
        if handle.qos != "broadcast" {
            return Err(TypedChannelError::SchemaMismatch(format!(
                "subscribe_broadcast called on '{channel}' but its qos is {}, not broadcast",
                handle.qos,
            )));
        }
        let (tx, rx) = mpsc::unbounded_channel();
        self.broadcast_subs
            .lock()
            .unwrap()
            .entry(channel.to_string())
            .or_default()
            .push(tx);
        Ok(rx)
    }

    /// Receive the next event on a non-broadcast channel.
    ///
    /// For `qos: broadcast`, use [`subscribe_broadcast`](Self::subscribe_broadcast)
    /// instead — the bus does not maintain a default queue for
    /// broadcast since each subscriber needs its own.
    pub async fn receive(&self, channel: &str) -> Result<TypedEvent> {
        let handle = self.get_handle(channel)?;
        if handle.qos == "broadcast" {
            return Err(TypedChannelError::SchemaMismatch(format!(
                "channel '{channel}' has qos=broadcast — call subscribe_broadcast() to get a per-subscriber queue"
            )));
        }
        let transport = self.transport_for(channel);
        transport.recv().await
    }

    // ── INTROSPECTION + CLEANUP ──────────────────────────────────

    /// Count of live (not yet discovered) capabilities.
    pub fn issued_capabilities(&self) -> usize {
        self.capabilities.lock().unwrap().len()
    }

    /// Drain caps, close transports, clear broadcast queues. Mirrors
    /// Python `close_all`.
    pub fn close_all(&self) {
        self.capabilities.lock().unwrap().clear();
        self.broadcast_subs.lock().unwrap().clear();
        self.delivered_ids.lock().unwrap().clear();
        let transports = self.transports.lock().unwrap();
        for t in transports.values() {
            t.close();
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
//  helpers
// ═══════════════════════════════════════════════════════════════════

fn gen_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

// ═══════════════════════════════════════════════════════════════════
//  tests — paridad con tests/test_typed_channels.py (Fase 13.d)
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use axon_frontend::ir_nodes::{IRChannel, IRProgram};
    use serde_json::json;

    // ── helpers ───────────────────────────────────────────────────

    fn ir_channel(
        name: &str,
        message: &str,
        qos: &str,
        lifetime: &str,
        persistence: &str,
        shield: &str,
    ) -> IRChannel {
        IRChannel {
            node_type: "IRChannel",
            source_line: 0,
            source_column: 0,
            name: name.to_string(),
            message: message.to_string(),
            qos: qos.to_string(),
            lifetime: lifetime.to_string(),
            persistence: persistence.to_string(),
            shield_ref: shield.to_string(),
            egress_sign: String::new(),
        }
    }

    fn handle(name: &str, message: &str) -> TypedChannelHandle {
        TypedChannelHandle::new(name, message)
    }

    // ── HANDLE ────────────────────────────────────────────────────

    #[test]
    fn handle_defaults_match_d1() {
        let h = handle("Orders", "Order");
        assert_eq!(h.qos, "at_least_once");
        assert_eq!(h.lifetime, "affine");
        assert_eq!(h.persistence, "ephemeral");
        assert_eq!(h.shield_ref, "");
        assert_eq!(h.consumed_count, 0);
    }

    #[test]
    fn handle_is_publishable_iff_shield() {
        let mut h = handle("Orders", "Order");
        assert!(!h.is_publishable());
        h.shield_ref = "PublicBroker".into();
        assert!(h.is_publishable());
    }

    #[test]
    fn handle_carries_channel_second_order() {
        let h = handle("Broker", "Channel<Order>");
        assert!(h.carries_channel());
        let h_first = handle("Orders", "Order");
        assert!(!h_first.carries_channel());
    }

    #[test]
    fn handle_inner_message_type_unwrap() {
        let h_so = handle("Broker", "Channel<Order>");
        assert_eq!(h_so.inner_message_type(), "Order");
        let h_first = handle("Orders", "Order");
        assert_eq!(h_first.inner_message_type(), "Order");
        let h_third = handle("Outer", "Channel<Channel<Order>>");
        assert_eq!(h_third.inner_message_type(), "Channel<Order>");
    }

    #[test]
    fn handle_from_ir_round_trip() {
        let ir = ir_channel(
            "Orders",
            "Order",
            "exactly_once",
            "linear",
            "persistent",
            "PublicBroker",
        );
        let h = TypedChannelHandle::from_ir(&ir);
        assert_eq!(h.name, "Orders");
        assert_eq!(h.message, "Order");
        assert_eq!(h.qos, "exactly_once");
        assert_eq!(h.lifetime, "linear");
        assert_eq!(h.persistence, "persistent");
        assert_eq!(h.shield_ref, "PublicBroker");
        assert_eq!(h.consumed_count, 0);
    }

    // ── REGISTRY ──────────────────────────────────────────────────

    #[test]
    fn registry_register_and_get() {
        let mut reg = TypedChannelRegistry::new();
        reg.register(handle("Orders", "Order"));
        assert!(reg.has("Orders"));
        assert_eq!(reg.get("Orders").unwrap().message, "Order");
    }

    #[test]
    fn registry_unknown_returns_error_with_registered() {
        let mut reg = TypedChannelRegistry::new();
        reg.register(handle("Orders", "Order"));
        let err = reg.get("Missing").unwrap_err();
        match err {
            TypedChannelError::ChannelNotFound { name, registered } => {
                assert_eq!(name, "Missing");
                assert_eq!(registered, vec!["Orders".to_string()]);
            }
            other => panic!("expected ChannelNotFound, got {other:?}"),
        }
    }

    #[test]
    fn registry_overwrite_replaces() {
        let mut reg = TypedChannelRegistry::new();
        reg.register(handle("Orders", "Order"));
        let mut h2 = handle("Orders", "OrderV2");
        h2.qos = "exactly_once".into();
        reg.register(h2);
        let stored = reg.get("Orders").unwrap();
        assert_eq!(stored.message, "OrderV2");
        assert_eq!(stored.qos, "exactly_once");
    }

    #[test]
    fn registry_names_sorted() {
        let mut reg = TypedChannelRegistry::new();
        reg.register(handle("ZetaOrders", "Order"));
        reg.register(handle("Alpha", "Alpha"));
        reg.register(handle("Mu", "Mu"));
        assert_eq!(
            reg.names(),
            vec!["Alpha".to_string(), "Mu".to_string(), "ZetaOrders".to_string()]
        );
    }

    #[test]
    fn registry_register_from_ir_returns_handle() {
        let mut reg = TypedChannelRegistry::new();
        let ir = ir_channel(
            "Orders",
            "Order",
            "at_least_once",
            "affine",
            "ephemeral",
            "Σ",
        );
        let h = reg.register_from_ir(&ir);
        assert_eq!(h.shield_ref, "Σ");
        assert_eq!(reg.get("Orders").unwrap().shield_ref, "Σ");
    }

    // ── BUS BOOTSTRAP ─────────────────────────────────────────────

    fn empty_ir_program() -> IRProgram {
        IRProgram::new()
    }

    #[test]
    fn bus_from_ir_program_registers_channels() {
        let mut ir = empty_ir_program();
        ir.channels.push(ir_channel(
            "Orders",
            "Order",
            "at_least_once",
            "affine",
            "ephemeral",
            "",
        ));
        ir.channels.push(ir_channel(
            "Broker",
            "Channel<Order>",
            "exactly_once",
            "affine",
            "ephemeral",
            "PublicBroker",
        ));
        let bus = TypedEventBus::from_ir_program(&ir);
        let names = bus.channel_names();
        assert_eq!(names, vec!["Broker".to_string(), "Orders".to_string()]);
        assert!(bus.get_handle("Broker").unwrap().is_publishable());
    }

    #[test]
    fn bus_default_compliance_is_permissive() {
        // Default compliance always returns true; publish succeeds
        // even when the underlying ESK metadata would have rejected.
        let bus = TypedEventBus::new();
        let mut h = handle("Orders", "Order");
        h.shield_ref = "Σ".into();
        bus.register(h);
        let cap = futures_executor_block_on(bus.publish("Orders", "Σ")).unwrap();
        assert_eq!(cap.channel_name, "Orders");
    }

    // Tiny block-on shim so non-async tests can trigger async paths.
    fn futures_executor_block_on<F: std::future::Future>(f: F) -> F::Output {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(f)
    }

    // ── EMIT ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn emit_scalar_round_trips() {
        let bus = TypedEventBus::new();
        bus.register(handle("Orders", "Order"));
        bus.emit("Orders", TypedPayload::scalar(json!({"id": 1})))
            .await
            .unwrap();
        let event = bus.receive("Orders").await.unwrap();
        match event.payload {
            TypedPayload::Scalar(v) => assert_eq!(v["id"], 1),
            _ => panic!("expected scalar"),
        }
    }

    #[tokio::test]
    async fn emit_unknown_channel_errors() {
        let bus = TypedEventBus::new();
        let err = bus
            .emit("Nope", TypedPayload::scalar(json!(null)))
            .await
            .unwrap_err();
        assert!(matches!(err, TypedChannelError::ChannelNotFound { .. }));
    }

    #[tokio::test]
    async fn emit_event_has_id_and_timestamp() {
        let bus = TypedEventBus::new();
        bus.register(handle("Orders", "Order"));
        bus.emit("Orders", TypedPayload::scalar(json!(0)))
            .await
            .unwrap();
        let e = bus.receive("Orders").await.unwrap();
        assert!(!e.event_id.is_empty());
        assert!(e.timestamp_secs > 0.0);
    }

    // ── EMIT MOBILITY (second-order) ──────────────────────────────

    #[tokio::test]
    async fn emit_handle_through_second_order() {
        let bus = TypedEventBus::new();
        bus.register(handle("Orders", "Order"));
        bus.register(handle("Broker", "Channel<Order>"));
        let inner = bus.get_handle("Orders").unwrap();
        bus.emit("Broker", TypedPayload::handle(inner))
            .await
            .unwrap();
        let e = bus.receive("Broker").await.unwrap();
        match e.payload {
            TypedPayload::Handle(h) => assert_eq!(h.name, "Orders"),
            _ => panic!("expected handle"),
        }
    }

    #[tokio::test]
    async fn emit_mobility_schema_mismatch_inner() {
        let bus = TypedEventBus::new();
        bus.register(handle("Orders", "Order"));
        bus.register(handle("Wrong", "Different"));
        bus.register(handle("Broker", "Channel<Order>"));
        let wrong = bus.get_handle("Wrong").unwrap();
        let err = bus
            .emit("Broker", TypedPayload::handle(wrong))
            .await
            .unwrap_err();
        match err {
            TypedChannelError::SchemaMismatch(msg) => {
                assert!(msg.contains("Channel<Order>"));
                assert!(msg.contains("Different"));
            }
            other => panic!("expected SchemaMismatch, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn emit_scalar_to_second_order_rejected() {
        let bus = TypedEventBus::new();
        bus.register(handle("Broker", "Channel<Order>"));
        let err = bus
            .emit("Broker", TypedPayload::scalar(json!("oops")))
            .await
            .unwrap_err();
        assert!(matches!(err, TypedChannelError::SchemaMismatch(_)));
    }

    #[tokio::test]
    async fn emit_handle_to_first_order_rejected() {
        let bus = TypedEventBus::new();
        bus.register(handle("Orders", "Order"));
        bus.register(handle("FirstOrder", "Order"));
        let h = bus.get_handle("Orders").unwrap();
        let err = bus
            .emit("FirstOrder", TypedPayload::handle(h))
            .await
            .unwrap_err();
        assert!(matches!(err, TypedChannelError::SchemaMismatch(_)));
    }

    // ── PUBLISH ──────────────────────────────────────────────────

    fn publishable_handle(name: &str, message: &str, shield: &str) -> TypedChannelHandle {
        let mut h = handle(name, message);
        h.shield_ref = shield.into();
        h
    }

    #[tokio::test]
    async fn publish_returns_capability() {
        let bus = TypedEventBus::new();
        bus.register(publishable_handle("Orders", "Order", "Σ"));
        let cap = bus.publish("Orders", "Σ").await.unwrap();
        assert_eq!(cap.channel_name, "Orders");
        assert_eq!(cap.shield_ref, "Σ");
        assert!(!cap.capability_id.is_empty());
        assert_eq!(bus.issued_capabilities(), 1);
    }

    #[tokio::test]
    async fn publish_empty_shield_rejected() {
        let bus = TypedEventBus::new();
        bus.register(publishable_handle("Orders", "Order", "Σ"));
        let err = bus.publish("Orders", "").await.unwrap_err();
        assert!(matches!(err, TypedChannelError::CapabilityGate(_)));
    }

    #[tokio::test]
    async fn publish_unpublishable_rejected() {
        let bus = TypedEventBus::new();
        // No shield_ref → not publishable.
        bus.register(handle("Orders", "Order"));
        let err = bus.publish("Orders", "Σ").await.unwrap_err();
        match err {
            TypedChannelError::CapabilityGate(msg) => {
                assert!(msg.contains("not publishable"));
            }
            other => panic!("expected CapabilityGate, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn publish_wrong_shield_rejected() {
        let bus = TypedEventBus::new();
        bus.register(publishable_handle("Orders", "Order", "Σ"));
        let err = bus.publish("Orders", "Other").await.unwrap_err();
        match err {
            TypedChannelError::CapabilityGate(msg) => {
                assert!(msg.contains("Σ"));
                assert!(msg.contains("Other"));
            }
            other => panic!("expected CapabilityGate, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn publish_unknown_channel_errors() {
        let bus = TypedEventBus::new();
        let err = bus.publish("Missing", "Σ").await.unwrap_err();
        assert!(matches!(err, TypedChannelError::ChannelNotFound { .. }));
    }

    #[tokio::test]
    async fn publish_default_delta_pub_is_paper_lower_bound() {
        let bus = TypedEventBus::new();
        bus.register(publishable_handle("Orders", "Order", "Σ"));
        let cap = bus.publish("Orders", "Σ").await.unwrap();
        assert!((cap.delta_pub - 0.05).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn publish_compliance_predicate_can_veto() {
        let veto: ShieldComplianceFn = Arc::new(|_, _| false);
        let bus = TypedEventBus::with_compliance_check(veto);
        bus.register(publishable_handle("Orders", "Order", "Σ"));
        let err = bus.publish("Orders", "Σ").await.unwrap_err();
        match err {
            TypedChannelError::CapabilityGate(msg) => {
                assert!(msg.contains("does not cover compliance"));
            }
            other => panic!("expected CapabilityGate, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn publish_compliance_predicate_inspects_handle() {
        let inspected: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured = inspected.clone();
        let check: ShieldComplianceFn = Arc::new(move |shield, h| {
            captured.lock().unwrap().push(format!("{shield}/{}", h.name));
            true
        });
        let bus = TypedEventBus::with_compliance_check(check);
        bus.register(publishable_handle("Orders", "Order", "Σ"));
        bus.publish("Orders", "Σ").await.unwrap();
        let calls = inspected.lock().unwrap();
        assert_eq!(*calls, vec!["Σ/Orders".to_string()]);
    }

    // ── DISCOVER ─────────────────────────────────────────────────

    #[tokio::test]
    async fn discover_returns_handle_and_consumes_capability() {
        let bus = TypedEventBus::new();
        bus.register(publishable_handle("Orders", "Order", "Σ"));
        let cap = bus.publish("Orders", "Σ").await.unwrap();
        assert_eq!(bus.issued_capabilities(), 1);
        let found = bus.discover(&cap).await.unwrap();
        assert_eq!(found.name, "Orders");
        assert_eq!(bus.issued_capabilities(), 0);
        // Second discover with the same capability is rejected.
        let err = bus.discover(&cap).await.unwrap_err();
        assert!(matches!(err, TypedChannelError::CapabilityGate(_)));
    }

    #[tokio::test]
    async fn discover_forged_capability_rejected() {
        let bus = TypedEventBus::new();
        bus.register(publishable_handle("Orders", "Order", "Σ"));
        let forged = Capability {
            capability_id: "forged".to_string(),
            channel_name: "Orders".to_string(),
            shield_ref: "Σ".to_string(),
            delta_pub: 0.05,
            issued_at: 0.0,
        };
        let err = bus.discover(&forged).await.unwrap_err();
        assert!(matches!(err, TypedChannelError::CapabilityGate(_)));
    }

    #[tokio::test]
    async fn capability_from_other_bus_rejected() {
        let bus_a = TypedEventBus::new();
        let bus_b = TypedEventBus::new();
        bus_a.register(publishable_handle("Orders", "Order", "Σ"));
        bus_b.register(publishable_handle("Orders", "Order", "Σ"));
        let cap = bus_a.publish("Orders", "Σ").await.unwrap();
        let err = bus_b.discover(&cap).await.unwrap_err();
        assert!(matches!(err, TypedChannelError::CapabilityGate(_)));
    }

    // ── QoS ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn qos_at_least_once_default_delivers() {
        let bus = TypedEventBus::new();
        bus.register(handle("Orders", "Order")); // default at_least_once
        bus.emit("Orders", TypedPayload::scalar(json!({"id": 1})))
            .await
            .unwrap();
        bus.emit("Orders", TypedPayload::scalar(json!({"id": 2})))
            .await
            .unwrap();
        let e1 = bus.receive("Orders").await.unwrap();
        let e2 = bus.receive("Orders").await.unwrap();
        match (&e1.payload, &e2.payload) {
            (TypedPayload::Scalar(v1), TypedPayload::Scalar(v2)) => {
                assert_eq!(v1["id"], 1);
                assert_eq!(v2["id"], 2);
            }
            _ => panic!("expected scalars"),
        }
    }

    #[tokio::test]
    async fn qos_at_most_once_delivers_once_then_drops_silently() {
        let bus = TypedEventBus::new();
        let mut h = handle("Telemetry", "Tick");
        h.qos = "at_most_once".into();
        bus.register(h);
        bus.emit("Telemetry", TypedPayload::scalar(json!(1)))
            .await
            .unwrap();
        // Close transport so the second emit hits the silent-drop path.
        // Direct close: grab the transport and mark closed.
        let transport = bus.transport_for("Telemetry");
        transport.close();
        // Should NOT raise — at_most_once swallows transport errors.
        bus.emit("Telemetry", TypedPayload::scalar(json!(2)))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn qos_exactly_once_dedups_event_ids() {
        let bus = TypedEventBus::new();
        let mut h = handle("EO", "Tick");
        h.qos = "exactly_once".into();
        bus.register(h.clone());

        // First emit — assert event flows. Then synthesise an event
        // with the same event_id and dispatch directly to confirm the
        // dedup set short-circuits the second send.
        bus.emit("EO", TypedPayload::scalar(json!(1)))
            .await
            .unwrap();
        let _e1 = bus.receive("EO").await.unwrap();

        let manual = TypedEvent {
            channel: "EO".to_string(),
            payload: TypedPayload::scalar(json!("dup")),
            event_id: "fixed-id".to_string(),
            timestamp_secs: now_secs(),
        };
        bus.dispatch(&h, manual.clone()).unwrap();
        // Second dispatch with the same event_id is dropped.
        bus.dispatch(&h, manual).unwrap();
        let received = bus.receive("EO").await.unwrap();
        assert_eq!(received.event_id, "fixed-id");
        // No more events queued.
        let try_more =
            tokio::time::timeout(std::time::Duration::from_millis(20), bus.receive("EO")).await;
        assert!(try_more.is_err(), "expected dedup to block second event");
    }

    #[tokio::test]
    async fn qos_broadcast_fan_out_to_subscribers() {
        let bus = TypedEventBus::new();
        let mut h = handle("Bus", "Tick");
        h.qos = "broadcast".into();
        bus.register(h);
        let mut s1 = bus.subscribe_broadcast("Bus").unwrap();
        let mut s2 = bus.subscribe_broadcast("Bus").unwrap();
        bus.emit("Bus", TypedPayload::scalar(json!("hi")))
            .await
            .unwrap();
        let e1 = s1.recv().await.unwrap();
        let e2 = s2.recv().await.unwrap();
        assert_eq!(e1.event_id, e2.event_id);
    }

    #[tokio::test]
    async fn qos_broadcast_subscribe_check_rejects_non_broadcast() {
        let bus = TypedEventBus::new();
        bus.register(handle("Plain", "X"));
        let err = bus.subscribe_broadcast("Plain").unwrap_err();
        assert!(matches!(err, TypedChannelError::SchemaMismatch(_)));
    }

    #[tokio::test]
    async fn qos_broadcast_receive_rejection() {
        let bus = TypedEventBus::new();
        let mut h = handle("Bus", "Tick");
        h.qos = "broadcast".into();
        bus.register(h);
        let err = bus.receive("Bus").await.unwrap_err();
        assert!(matches!(err, TypedChannelError::SchemaMismatch(_)));
    }

    #[tokio::test]
    async fn qos_queue_fifo_ordering() {
        let bus = TypedEventBus::new();
        let mut h = handle("Q", "Job");
        h.qos = "queue".into();
        bus.register(h);
        bus.emit("Q", TypedPayload::scalar(json!(1))).await.unwrap();
        bus.emit("Q", TypedPayload::scalar(json!(2))).await.unwrap();
        bus.emit("Q", TypedPayload::scalar(json!(3))).await.unwrap();
        let mut seen = vec![];
        for _ in 0..3 {
            let e = bus.receive("Q").await.unwrap();
            if let TypedPayload::Scalar(v) = e.payload {
                seen.push(v.as_i64().unwrap());
            }
        }
        assert_eq!(seen, vec![1, 2, 3]);
    }

    // ── LIFETIME ─────────────────────────────────────────────────

    #[tokio::test]
    async fn lifetime_affine_allows_multi_emit() {
        let bus = TypedEventBus::new();
        bus.register(handle("Orders", "Order")); // affine default
        for i in 0..3 {
            bus.emit("Orders", TypedPayload::scalar(json!(i)))
                .await
                .unwrap();
        }
        // affine has no upper bound on emits per handle (per-handle
        // tracking; per-binding deferred — parity with Python 13.d).
    }

    #[tokio::test]
    async fn lifetime_linear_second_emit_violates() {
        let bus = TypedEventBus::new();
        let mut h = handle("Once", "Order");
        h.lifetime = "linear".into();
        bus.register(h);
        bus.emit("Once", TypedPayload::scalar(json!(0)))
            .await
            .unwrap();
        let err = bus
            .emit("Once", TypedPayload::scalar(json!(1)))
            .await
            .unwrap_err();
        match err {
            TypedChannelError::LifetimeViolation { name, count } => {
                assert_eq!(name, "Once");
                assert_eq!(count, 2);
            }
            other => panic!("expected LifetimeViolation, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn lifetime_persistent_unrestricted() {
        let bus = TypedEventBus::new();
        let mut h = handle("Ledger", "Entry");
        h.lifetime = "persistent".into();
        bus.register(h);
        for i in 0..16 {
            bus.emit("Ledger", TypedPayload::scalar(json!(i)))
                .await
                .unwrap();
        }
    }

    // ── PAPER §9 END-TO-END ──────────────────────────────────────

    #[tokio::test]
    async fn paper_section9_e2e_producer_publish_discover_receive() {
        // Models the worked example from paper_mobile_channels.md §9
        // (paper §9): typed producer emits an Order, then publishes
        // its OrdersCreated channel through a shield, then a separate
        // consumer discovers and receives.
        let bus = TypedEventBus::new();
        let mut orders = handle("OrdersCreated", "Order");
        orders.shield_ref = "PublicBroker".into();
        bus.register(orders);

        // Producer emits an order.
        bus.emit(
            "OrdersCreated",
            TypedPayload::scalar(json!({"id": 42, "total": 19.99})),
        )
        .await
        .unwrap();

        // Producer publishes the channel through the shield.
        let cap = bus
            .publish("OrdersCreated", "PublicBroker")
            .await
            .unwrap();

        // Consumer discovers and receives the queued event.
        let handle = bus.discover(&cap).await.unwrap();
        assert_eq!(handle.name, "OrdersCreated");
        let event = bus.receive("OrdersCreated").await.unwrap();
        match event.payload {
            TypedPayload::Scalar(v) => {
                assert_eq!(v["id"], 42);
                assert_eq!(v["total"], 19.99);
            }
            _ => panic!("expected scalar Order payload"),
        }
    }

    // ── ERROR DISPLAY ────────────────────────────────────────────

    #[test]
    fn error_display_includes_useful_context() {
        let err = TypedChannelError::ChannelNotFound {
            name: "X".to_string(),
            registered: vec!["A".to_string(), "B".to_string()],
        };
        let s = format!("{err}");
        assert!(s.contains("'X'"));
        assert!(s.contains("[\"A\", \"B\"]"));

        let err = TypedChannelError::LifetimeViolation {
            name: "Once".to_string(),
            count: 2,
        };
        let s = format!("{err}");
        assert!(s.contains("Once"));
        assert!(s.contains("2"));
    }

    // ── EDGE CASES ───────────────────────────────────────────────

    #[tokio::test]
    async fn capability_ids_are_unique() {
        let bus = TypedEventBus::new();
        bus.register(publishable_handle("Orders", "Order", "Σ"));
        let cap1 = bus.publish("Orders", "Σ").await.unwrap();
        let cap2 = bus.publish("Orders", "Σ").await.unwrap();
        assert_ne!(cap1.capability_id, cap2.capability_id);
        assert_eq!(bus.issued_capabilities(), 2);
    }

    #[tokio::test]
    async fn close_all_drains_state() {
        let bus = TypedEventBus::new();
        bus.register(publishable_handle("Orders", "Order", "Σ"));
        let mut bcast = handle("Bus", "Tick");
        bcast.qos = "broadcast".into();
        bus.register(bcast);
        let _sub = bus.subscribe_broadcast("Bus").unwrap();
        let _cap = bus.publish("Orders", "Σ").await.unwrap();
        assert_eq!(bus.issued_capabilities(), 1);

        bus.close_all();

        assert_eq!(bus.issued_capabilities(), 0);
        // Broadcast subs cleared — next emit fans out to nobody.
        bus.emit("Bus", TypedPayload::scalar(json!("after-close")))
            .await
            .unwrap();
    }
}
