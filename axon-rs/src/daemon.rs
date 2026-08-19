//! v2.4.0 — the single-node `daemon` runtime: cron scheduling + the
//! handler-body executor.
//!
//! v2.4.0 parsed + validated `listen "cron:<expr>"` listeners; v2.4.0 made the
//! handler body executable flow-steps (incl. `run <Flow>`). This module is the
//! engine that makes a `daemon` actually FIRE on a single node:
//!
//!   1. [`next_fire_after`] — given a validated [`CronSchedule`] (its fields
//! already expanded to value sets by v2.4.0) and an instant, the next
//!      wall-clock minute that matches. Pure + exhaustively tested; the timer
//! and the v2.4.0 enterprise HA scheduler both compute fire times with it.
//!   2. [`cron_listeners`] — extract a daemon's cron listeners (schedule + body)
//!      from its IR.
//!   3. [`run_invocations`] — the `run <Flow>` steps a handler body invokes.
//!   4. [`execute_listener_body`] — run those invocations through the proven
//!      [`crate::runner::execute_server_flow`] (whose registry path runs a flow
//!      by name with default persona/context — no top-level `run` required).
//!   5. [`run_daemon`] — the async driver: per cron listener, sleep to the next
//!      fire and execute the body, until cancelled. Single-node (the OSS
//! privilege); the v2.4.0 enterprise layer adds multi-tenant mount + HA
//!      fire-once-across-replicas on top of this same scheduling math.
//!
//! **dom/dow semantics (honest v1):** matching is AND across all five fields.
//! POSIX cron's special-case OR between day-of-month and day-of-week (when BOTH
//! are restricted) is a deferred refinement — it only differs when neither is
//! `*`, and the expanded `CronSchedule` does not retain the wildcard-vs-full
//! distinction needed to detect that case. Every schedule with a `*` in dom or
//! dow (the overwhelming majority) is identical under AND and OR.

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};

use axon_frontend::cron::{cron_expr, CronSchedule};
use axon_frontend::ir_nodes::{IRDaemon, IRFlowNode, IRListenStep, IRRun, IRWindow};

use crate::window::{decide, WindowAction};

/// A clock — injected so the scheduler is testable with a fixed time.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

/// The production clock (wall-clock UTC).
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// The next instant strictly after `after` that matches `schedule`, at
/// minute granularity. `None` if no match within ~366 days (an impossible
/// schedule, e.g. Feb 30). Deterministic + side-effect free.
pub fn next_fire_after(schedule: &CronSchedule, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    // Start at the next whole minute strictly after `after`.
    let mut t = (after + Duration::minutes(1))
        .with_second(0)
        .and_then(|t| t.with_nanosecond(0))?;
    let limit = after + Duration::days(366);
    while t <= limit {
        if matches_minute(schedule, t) {
            return Some(t);
        }
        t += Duration::minutes(1);
    }
    None
}

/// The next `n` fire times from `from` (each strictly after the previous).
/// Stops early if the schedule has no further match within the horizon.
pub fn next_fires(schedule: &CronSchedule, from: DateTime<Utc>, n: usize) -> Vec<DateTime<Utc>> {
    let mut out = Vec::with_capacity(n);
    let mut cursor = from;
    for _ in 0..n {
        match next_fire_after(schedule, cursor) {
            Some(t) => {
                out.push(t);
                cursor = t;
            }
            None => break,
        }
    }
    out
}

/// Does this wall-clock minute match every field of the schedule? (AND
/// semantics — see the module note on dom/dow.)
fn matches_minute(s: &CronSchedule, t: DateTime<Utc>) -> bool {
    let dow = t.weekday().num_days_from_sunday(); // 0 = Sunday … 6 = Saturday
    s.minute.contains(&t.minute())
        && s.hour.contains(&t.hour())
        && s.day_of_month.contains(&t.day())
        && s.month.contains(&t.month())
        && s.day_of_week.contains(&dow)
}

/// One cron-scheduled listener of a daemon: its validated schedule + a borrow
/// of its handler body (the flow-steps to run on each fire).
pub struct CronListener<'a> {
    /// The parsed cron schedule (channel was `"cron:<expr>"`).
    pub schedule: CronSchedule,
    /// The handler body — flow-steps executed per fire.
    pub body: &'a [IRFlowNode],
    /// The listener's source channel string (for diagnostics/audit).
    pub channel: String,
}

/// Extract a daemon's CRON listeners (channel `"cron:<expr>"` that parses).
/// Event (non-cron) listeners are ignored here — they fire on the event bus,
/// not the timer. A malformed cron is skipped (it was already `axon-E0789` at
/// type-check; defensively skipped at runtime rather than panicking).
pub fn cron_listeners(daemon: &IRDaemon) -> Vec<CronListener<'_>> {
    daemon
        .listeners
        .iter()
        .filter_map(|l: &IRListenStep| {
            let expr = cron_expr(&l.channel)?;
            let schedule = CronSchedule::parse(expr).ok()?;
            Some(CronListener {
                schedule,
                body: &l.body,
                channel: l.channel.clone(),
            })
        })
        .collect()
}

/// v2.31.0 — one EVENT (non-cron) listener of a daemon: the channel /
/// topic it subscribes to, its event alias, and its handler body. The
/// complement of [`CronListener`].
pub struct EventListener<'a> {
    /// The channel name (a typed `channel`) or topic string it listens on.
    pub channel: String,
    /// The `listen … as <alias>` binding name (the event payload is bound
    /// here for the body).
    pub alias: String,
    /// The handler body — flow-steps run per delivered event.
    pub body: &'a [IRFlowNode],
}

/// v2.31.0 — extract a daemon's EVENT listeners: every `listen` whose
/// channel is NOT a `cron:<expr>` schedule (a typed `channel` or a topic
/// string). The exact complement of [`cron_listeners`] — those fire on the
/// timer, these fire on the event bus (the producer's `emit`). Before v2.31.0
/// these were silently dropped (the v2.4.0 `axon-W009` honesty boundary);
/// v2.31.0 delivers to them.
pub fn event_listeners(daemon: &IRDaemon) -> Vec<EventListener<'_>> {
    daemon
        .listeners
        .iter()
        .filter(|l: &&IRListenStep| cron_expr(&l.channel).is_none())
        .map(|l| EventListener {
            channel: l.channel.clone(),
            alias: l.event_alias.clone(),
            body: &l.body,
        })
        .collect()
}

/// v2.27.0 — the `window` primitive a daemon binds via `window:`, resolved
/// against the program's window declarations. `None` when the daemon has no
/// temporal guard (the common case — behaviour is byte-identical to pre-v2.27.0).
/// A dangling `window_ref` (rejected by `axon-T825` at compile time) resolves
/// to `None` here, so the daemon fires unguarded rather than panicking.
pub fn bound_window<'a>(
    ir: &'a axon_frontend::ir_nodes::IRProgram,
    daemon: &IRDaemon,
) -> Option<&'a IRWindow> {
    if daemon.window_ref.is_empty() {
        return None;
    }
    ir.windows.iter().find(|w| w.name == daemon.window_ref)
}

/// The `run <Flow>` invocations in a handler body, in order. v1 scheduled
/// handlers ORCHESTRATE flows (the logic lives in the flows they run); this is
/// the set of flows a tick dispatches.
pub fn run_invocations(body: &[IRFlowNode]) -> Vec<&IRRun> {
    body.iter()
        .filter_map(|n| match n {
            IRFlowNode::Run(r) => Some(r),
            _ => None,
        })
        .collect()
}

/// Execute a handler body once (one tick): dispatch each `run <Flow>`
/// invocation through [`crate::runner::execute_server_flow`]. Returns the
/// per-invocation results (`Ok` metrics or an error string), in order.
///
/// `ir` is the full program (the flow registry `execute_server_flow` resolves
/// the invoked flow against). Synchronous: a tick runs its flows to completion
/// before the next sleep.
pub fn execute_listener_body(
    ir: &axon_frontend::ir_nodes::IRProgram,
    body: &[IRFlowNode],
    backend: &str,
    source_file: &str,
    // v2.28.0 — the daemon's linear-effect budget gate (shared across ticks so
    // bucket/window state is cumulative). `None` for a budgetless daemon.
    budget: Option<std::sync::Arc<std::sync::Mutex<crate::runtime::budget_kernel::BudgetGate>>>,
) -> Vec<(String, Result<crate::runner::ServerRunnerMetrics, String>)> {
    let empty = std::collections::HashMap::new();
    run_invocations(body)
        .into_iter()
        .map(|run| {
            let result = crate::runner::execute_server_flow(
                ir,
                &run.flow_name,
                backend,
                // v2.49.0 — the OSS in-process daemon runs under the
                // request/default tenant scope (self-hosted is single-tenant;
                // the ENT multi-tenant path is the supervisor).
                //
                // v2.89.0 — the old wording, *"bridge the task-local to the
                // executor's explicit tenant"*, described something that cannot
                // happen here. This read is inside `run_daemon`'s
                // `tokio::spawn`, and a spawned task inherits no task-local, so
                // there is nothing to bridge: the value is always the
                // `"default"` fallback.
                //
                // The BEHAVIOUR is correct and deliberate — a self-hosted OSS
                // daemon is single-tenant, and `"default"` is that tenant. Only
                // the comment was wrong, and a comment that describes a bridge
                // where there is none is how the next reader concludes this
                // path is tenant-aware. It is not; the multi-tenant story is
                // the enterprise supervisor, which re-enters `scope_tenant` per
                // tenant inside its own spawned driver.
                &crate::tenant_context::current_tenant_id(),
                source_file,
                None,
                None,
                &empty,
                &empty,
                None,
                // v1.18.0 — OSS daemon: env/default LLM endpoint
                // (per-tenant override is the enterprise supervisor's path).
                None,
                None,
                // v2.28.0 — the daemon's effect budget gate.
                budget.clone(),
                // v2.69.0 — the daemon path carries no channel semaphores (the
                // HTTP vendor-concurrency case is the server path's; a daemon's tool
                // calls are single-threaded per tick). Owed if daemons ever fan out.
                None,
                // v2.69.0 — likewise no tool leases on the daemon path.
                None,
                // v2.31.0 — the OSS single-node daemon keeps `emit` in-process;
                // the durable per-tenant outbox is the enterprise supervisor path.
                None,
                None, // v2.46.0 — no minter on the OSS daemon path (mint fails closed)
                None, // v2.48.0 — no custody on the OSS daemon path (rotate/secrets fail closed)
                        None, // v2.63.0 — no engine on the OSS daemon path yet (data-plane verbs fail closed — honest refusal; wiring is deferred v2.63.0 surface)
                        None, // v2.56.0 scrape_overrides
);
            (run.flow_name.clone(), result)
        })
        .collect()
}

/// v2.31.0 — deliver one emitted event to every daemon `listen`er on
/// `channel`. For each matching listener body's `run <Flow>` invocation,
/// run the flow with the event `payload` bound as its REQUEST BODY (so the
/// consumer flow's declared parameters bind from the event's same-named
/// fields — the v1.32.0 Request Binding Contract). Returns the per-(daemon,
/// flow) results in declaration order.
///
/// This is the π-calculus `(Comm)` reduction made executable — the
/// CONSUMER side of durable event delivery. v2.31.0 establishes the contract
/// in-process; at-least-once + the durable outbox + the running supervisor
/// loop are v2.31.0. A channel with no matching listener delivers to
/// nobody (an empty result — not an error; a fan-out of zero is valid).
#[allow(clippy::type_complexity)]
pub fn deliver_typed_event(
    ir: &axon_frontend::ir_nodes::IRProgram,
    channel: &str,
    payload: &serde_json::Value,
    backend: &str,
    source_file: &str,
    budget: Option<std::sync::Arc<std::sync::Mutex<crate::runtime::budget_kernel::BudgetGate>>>,
) -> Vec<(String, String, Result<crate::runner::ServerRunnerMetrics, String>)> {
    let empty = std::collections::HashMap::new();
    let mut out = Vec::new();
    for daemon in &ir.daemons {
        for listener in event_listeners(daemon) {
            if listener.channel != channel {
                continue;
            }
            for run in run_invocations(listener.body) {
                let result = crate::runner::execute_server_flow(
                    ir,
                    &run.flow_name,
                    backend,
                    // v2.49.0 — bridge the daemon's tenant scope explicitly.
                    &crate::tenant_context::current_tenant_id(),
                    source_file,
                    None,
                    // v2.31.0 — the event payload binds to the consumer flow's
                    // parameters (the v1.32.0 Request Binding Contract).
                    Some(payload),
                    &empty,
                    &empty,
                    None,
                    None,
                    None,
                    budget.clone(),
                    // v2.69.0 — daemon path: no channel semaphores (server path's concern).
                    None, // v2.69.0 — daemon path: no tool leases (server path's concern).
                    None,
                    // v2.31.0 — OSS single-node delivery stays in-process.
                    None,
                    None, // v2.46.0 — no minter on the OSS daemon path (mint fails closed)
                    None, // v2.48.0 — no custody on the OSS daemon path (rotate/secrets fail closed)
                                None, // v2.63.0 — no engine on the OSS daemon path yet (data-plane verbs fail closed — honest refusal; wiring is deferred v2.63.0 surface)
                                None, // v2.56.0 scrape_overrides
);
                out.push((daemon.name.clone(), run.flow_name.clone(), result));
            }
        }
    }
    out
}

/// v2.31.0 — the bounded retry ceiling for at-least-once delivery. A
/// listener body that keeps failing past this many attempts is DEAD-
/// LETTERED rather than redelivered forever (no infinite-redelivery storm
/// — the `delivery_is_a_kept_promise` doctrine keeps delivery TOTAL).
pub const MAX_DELIVERY_ATTEMPTS: u32 = 5;

/// v2.31.0 — the outcome of delivering one event to one listener under
/// its channel's `qos`. Total: every delivery terminates in exactly one of
/// these — acked, dead-lettered, or (at_most_once) dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOutcome {
    /// The body ran to completion within the retry budget. `attempts` is
    /// how many tries it took (1 = first try).
    Acked { attempts: u32 },
    /// `at_least_once`: the body failed every attempt up to the ceiling →
    /// dead-lettered (recorded, not silently dropped).
    DeadLettered { attempts: u32, last_error: String },
    /// `at_most_once`: the body failed its single attempt → dropped
    /// (fire-and-forget, no redelivery — the qos the program declared).
    Dropped { error: String },
}

/// v2.31.0 — at-least-once delivery of ONE event to ONE listener body,
/// honoring `qos`. Pure policy (the body executor is injected as `run`):
///
/// - `at_least_once` (default + `exactly_once` + `queue` + `broadcast`):
///   retry a failing body up to `max_attempts`, then [`DeliveryOutcome::DeadLettered`].
/// - `at_most_once`: run once; a failure is [`DeliveryOutcome::Dropped`]
///   (no redelivery — the program asked for best-effort).
///
/// `run` returns `Ok(())` on a successful body run, `Err(reason)` on
/// failure. Side-effect-free + deterministic given `run` — the retry/
/// dead-letter policy is unit-tested without touching the flow runtime.
pub fn deliver_with_retry(
    qos: &str,
    max_attempts: u32,
    mut run: impl FnMut() -> std::result::Result<(), String>,
) -> DeliveryOutcome {
    let cap = max_attempts.max(1);
    if qos == "at_most_once" {
        return match run() {
            Ok(()) => DeliveryOutcome::Acked { attempts: 1 },
            Err(e) => DeliveryOutcome::Dropped { error: e },
        };
    }
    let mut last_error = String::new();
    for attempt in 1..=cap {
        match run() {
            Ok(()) => return DeliveryOutcome::Acked { attempts: attempt },
            Err(e) => last_error = e,
        }
    }
    DeliveryOutcome::DeadLettered { attempts: cap, last_error }
}

/// v2.31.0 — the qos FAN-OUT rule for daemon delivery: how many of the
/// `matching` listeners on a channel actually receive an event.
///
/// - `broadcast` → ALL of them (pub/sub fan-out: every listener gets it).
/// - everything else (`at_least_once` / `at_most_once` / `exactly_once` /
/// `queue`) → SINGLE-CONSUMER: at most ONE listener receives it (the v1.6.0
///   transport spec — these modes are a single-consumer FIFO).
///
/// (Fair round-robin balancing across competing `queue` consumers is a
/// refinement; the load-bearing semantic — `queue`/FIFO ⇒ one, `broadcast`
/// ⇒ all — is what this enforces. `exactly_once`'s no-double-apply-across-a-
/// crash guarantee needs an ATOMIC ack and is v2.31.0; here `exactly_once`
/// is single-consumer + the outbox's offset-keyed `mark_processed` dedup.)
pub fn fan_out_count(qos: &str, matching: usize) -> usize {
    if qos == "broadcast" {
        matching
    } else {
        matching.min(1)
    }
}

/// v2.31.0 — RELIABLE (at-least-once) delivery of one event to every
/// daemon `listen`er on `channel`, honoring the channel's declared `qos`.
/// Per listener, the body (all its `run <Flow>` invocations, each with the
/// event `payload` bound as its v1.32.0 request body) is run with
/// [`deliver_with_retry`]; a body that exhausts its retries is dead-
/// lettered (logged, surfaced in the outcome), never silently lost. This
/// is the Q3 guarantee on the in-process substrate — at-least-once with
/// bounded redelivery. (Survive-a-crash durability is the v2.31.0
/// transactional outbox + v2.31.0 per-tenant store.) Returns the
/// per-(daemon) outcome in declaration order.
#[allow(clippy::type_complexity)]
pub fn deliver_typed_event_reliable(
    ir: &axon_frontend::ir_nodes::IRProgram,
    channel: &str,
    payload: &serde_json::Value,
    backend: &str,
    source_file: &str,
    budget: Option<std::sync::Arc<std::sync::Mutex<crate::runtime::budget_kernel::BudgetGate>>>,
) -> Vec<(String, DeliveryOutcome)> {
    // The qos the program declared on the channel (default at_least_once
    // when the channel is not a declared `channel` — a topic string).
    let qos = ir
        .channels
        .iter()
        .find(|c| c.name == channel)
        .map(|c| c.qos.clone())
        .unwrap_or_else(|| "at_least_once".to_string());

    // v2.31.0 — collect the matching listeners, then apply the qos FAN-OUT:
    // `broadcast` delivers to EVERY listener; every other mode is
    // SINGLE-CONSUMER (one listener), per the v1.6.0 transport spec.
    let mut matching: Vec<(String, EventListener)> = Vec::new();
    for daemon in &ir.daemons {
        for listener in event_listeners(daemon) {
            if listener.channel == channel {
                matching.push((daemon.name.clone(), listener));
            }
        }
    }
    let take = fan_out_count(&qos, matching.len());

    let empty = std::collections::HashMap::new();
    let mut out = Vec::new();
    for (daemon_name, listener) in matching.into_iter().take(take) {
        {
            let outcome = deliver_with_retry(&qos, MAX_DELIVERY_ATTEMPTS, || {
                // The body runs ALL its invocations; a single failure fails
                // the body run (→ a retry of the whole body — re-running a
                // succeeded invocation is the at-least-once cost; idempotent
                // dedup is v2.31.0 `exactly_once`).
                for run in run_invocations(listener.body) {
                    let r = crate::runner::execute_server_flow(
                        ir,
                        &run.flow_name,
                        backend,
                        // v2.49.0 — the daemon's tenant scope.
                        // v2.89.0 — likewise inside `run_event_listeners`'s
                        // `tokio::spawn`: nothing is bridged, this is always
                        // `"default"`, and for a single-tenant OSS daemon that
                        // is the right answer. See the note in
                        // `execute_listener_body`.
                        &crate::tenant_context::current_tenant_id(),
                        source_file,
                        None,
                        Some(payload),
                        &empty,
                        &empty,
                        None,
                        None,
                        None,
                        budget.clone(),
                        // v2.69.0 — daemon path: no channel semaphores (server path's concern).
                        None, // v2.69.0 — daemon path: no tool leases (server path's concern).
                        None,
                        // v2.31.0 — OSS single-node delivery stays in-process.
                        None,
                        None, // v2.46.0 — no minter on the OSS daemon path (mint fails closed)
                    None, // v2.48.0 — no custody on the OSS daemon path (rotate/secrets fail closed)
                                        None, // v2.63.0 — no engine on the OSS daemon path yet (data-plane verbs fail closed — honest refusal; wiring is deferred v2.63.0 surface)
                                        None, // v2.56.0 scrape_overrides
);
                    if let Err(e) = r {
                        return Err(format!("flow '{}': {e}", run.flow_name));
                    }
                }
                Ok(())
            });
            if let DeliveryOutcome::DeadLettered { attempts, last_error } = &outcome {
                eprintln!(
                    "daemon '{daemon_name}' listener on '{channel}' DEAD-LETTERED after \
                     {attempts} attempts: {last_error}"
                );
            }
            out.push((daemon_name, outcome));
        }
    }
    out
}

/// v2.31.0 — the single-node OSS event-delivery driver: per the
/// program's daemon EVENT listeners, spawn a task that loops
/// `bus.receive(channel).await` → reliable (at-least-once) delivery, until
/// cancelled. The complement of [`run_daemon`] (cron). Single-node by
/// construction (it delivers once PER PROCESS); the v2.31.0 enterprise layer
/// adds the durable per-tenant outbox + fire-once-across-replicas on top.
/// `bus` is shared with the producer flows (their `emit` feeds it).
pub async fn run_event_listeners(
    ir: std::sync::Arc<axon_frontend::ir_nodes::IRProgram>,
    bus: std::sync::Arc<crate::runtime::channels::TypedEventBus>,
    backend: String,
    cancel: crate::cancel_token::CancellationFlag,
    // v2.31.0 — optional replay/audit-chain sink: each delivery records a
    // `deliver:<channel>` ReplayToken. `None` → no recording (pre-v2.31.0).
    replay_log: Option<std::sync::Arc<dyn crate::replay_token::ReplayLog>>,
) {
    // The distinct channels any daemon event-listens on (one receive loop
    // per channel — the bus transport is single-consumer FIFO per channel).
    let mut channels: Vec<String> = ir
        .daemons
        .iter()
        .flat_map(|d| event_listeners(d).into_iter().map(|l| l.channel))
        .collect();
    channels.sort();
    channels.dedup();

    let mut tasks = Vec::new();
    for channel in channels {
        let ir = ir.clone();
        let bus = bus.clone();
        let cancel = cancel.clone();
        let backend = backend.clone();
        let replay_log = replay_log.clone();
        tasks.push(tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    received = bus.receive(&channel) => {
                        let Ok(event) = received else { return }; // sender dropped → stop
                        let payload = match event.payload {
                            crate::runtime::channels::TypedPayload::Scalar(v) => v,
                            // Channel mobility (handle payload) delivery is v1.6.0 section 3.3,
                            // deferred (v2.31.0 deferred scope) — skip, don't crash.
                            crate::runtime::channels::TypedPayload::Handle(_) => continue,
                        };
                        // v2.31.0 — reliable delivery + record the deliver token.
                        let _ = deliver_and_record(
                            &ir, &channel, &payload, &backend, "<daemon-event>", None,
                            replay_log.as_deref(),
                        )
                        .await;
                    }
                }
            }
        }));
    }
    for t in tasks {
        let _ = t.await;
    }
}

/// v2.31.0 — mint a `ReplayToken` for a typed-channel event (paper section 5.4:
/// "every Chan-Input reduction emits a ReplayToken" — and, symmetrically,
/// every Chan-Output). `effect_name` is `emit:<channel>` (the producer's
/// output prefix) or `deliver:<channel>` (the consumer's input prefix);
/// `inputs` carries the event payload + the `flow_id` the [`ReplayLog`]
/// indexes by; `outputs` is the delivery outcome (Null for a bare emit).
/// The model slug is the stable deterministic `axon.builtin.channel.v1` —
/// channel delivery is MECHANICAL (dispatch, not cognition), so it replays
/// bit-for-bit. The receipt puts an event in the v1.4.0 audit/replay chain.
pub fn mint_channel_event_token(
    effect_name: &str,
    flow_id: &str,
    payload: &serde_json::Value,
    outputs: serde_json::Value,
) -> crate::replay_token::ReplayToken {
    crate::replay_token::ReplayTokenBuilder::new()
        .effect_name(effect_name)
        .inputs(serde_json::json!({ "flow_id": flow_id, "payload": payload }))
        .outputs(outputs)
        .model_version("axon.builtin.channel.v1")
        .mint()
}

/// v2.31.0 — deliver an event (reliable, at-least-once, v2.31.0) AND
/// record a `deliver:<channel>` [`ReplayToken`] per listener outcome to the
/// attached [`ReplayLog`], so each Chan-Input reduction is in the replay/
/// audit chain. Async (the log sink may be Postgres / the audit chain).
/// Returns the per-(daemon) outcomes. When `log` is `None`, behaves exactly
/// like [`deliver_typed_event_reliable`] (no recording).
pub async fn deliver_and_record(
    ir: &axon_frontend::ir_nodes::IRProgram,
    channel: &str,
    payload: &serde_json::Value,
    backend: &str,
    source_file: &str,
    budget: Option<std::sync::Arc<std::sync::Mutex<crate::runtime::budget_kernel::BudgetGate>>>,
    log: Option<&dyn crate::replay_token::ReplayLog>,
) -> Vec<(String, DeliveryOutcome)> {
    let outcomes =
        deliver_typed_event_reliable(ir, channel, payload, backend, source_file, budget);
    if let Some(log) = log {
        for (daemon_name, outcome) in &outcomes {
            let summary = match outcome {
                DeliveryOutcome::Acked { attempts } => {
                    serde_json::json!({ "outcome": "acked", "attempts": attempts })
                }
                DeliveryOutcome::DeadLettered { attempts, last_error } => serde_json::json!({
                    "outcome": "dead_lettered", "attempts": attempts, "error": last_error
                }),
                DeliveryOutcome::Dropped { error } => {
                    serde_json::json!({ "outcome": "dropped", "error": error })
                }
            };
            let token = mint_channel_event_token(
                &format!("deliver:{channel}"),
                daemon_name,
                payload,
                summary,
            );
            // A token-sink failure must not lose the delivery — log + carry on
            // (the delivery already happened; the receipt is best-effort here,
            // hardened in the v2.31.0 durable audit-chain sink).
            if let Err(e) = log.append(token).await {
                eprintln!("deliver token append failed for '{channel}': {e}");
            }
        }
    }
    outcomes
}

/// v2.31.0 — drain a channel's unprocessed OUTBOX and deliver each
/// event at-least-once (v2.31.0), acking every entry whose delivery reaches
/// a TERMINAL outcome (acked / dead-lettered / dropped) so it is not
/// redelivered. An entry only stays redeliverable while NO drain has been
/// run for it — so a supervisor that was DOWN when the event was emitted
/// picks the backlog up on its next drain (the durable Q3 guarantee, over
/// a persisted log rather than an ephemeral queue). An event with no
/// listener is acked (a fan-out of zero is done — not redelivered to
/// nobody). Returns the per-(offset, daemon) outcomes.
#[allow(clippy::type_complexity)]
pub fn drain_outbox(
    ir: &axon_frontend::ir_nodes::IRProgram,
    outbox: &dyn crate::event_outbox::EventOutbox,
    channel: &str,
    backend: &str,
    source_file: &str,
    budget: Option<std::sync::Arc<std::sync::Mutex<crate::runtime::budget_kernel::BudgetGate>>>,
) -> Vec<(u64, String, DeliveryOutcome)> {
    let mut out = Vec::new();
    for entry in outbox.unprocessed(channel) {
        let outcomes = deliver_typed_event_reliable(
            ir,
            channel,
            &entry.payload,
            backend,
            source_file,
            budget.clone(),
        );
        // Terminal: every attempted delivery (incl. an empty fan-out) acks
        // the entry. Redelivery happens only ACROSS drains, for entries a
        // drain never reached (the consumer/supervisor was down).
        outbox.mark_processed(channel, entry.offset);
        for (daemon, outcome) in outcomes {
            out.push((entry.offset, daemon, outcome));
        }
    }
    out
}

/// v2.4.0 — the single-node daemon driver. For each cron listener, loops:
/// compute the next fire from the clock, sleep until then, execute the body.
/// Returns when `cancel` is triggered. Each listener is driven on its own
/// spawned task so independent schedules don't block each other.
///
/// Single-node by construction: it fires once PER PROCESS. A multi-replica
/// deploy must NOT run this directly (it would double-fire) — the v2.4.0
/// enterprise supervisor adds the fire-once-across-replicas guard on top of the
/// same [`next_fire_after`] math.
pub async fn run_daemon(
    ir: std::sync::Arc<axon_frontend::ir_nodes::IRProgram>,
    daemon_name: String,
    backend: String,
    clock: std::sync::Arc<dyn Clock>,
    cancel: crate::cancel_token::CancellationFlag,
) {
    // Snapshot the daemon's cron schedules + bodies (owned, so the spawned
    // per-listener tasks don't borrow `ir`'s daemon list). v2.27.0 also
    // snapshots the bound `window` (cloned once — it is the same guard for every
    // listener of the daemon).
    // v2.28.0 also builds the daemon's `budget { … }` gate ONCE (shared
    // `Arc<Mutex>` across every listener + tick, so the rate buckets / max windows
    // accumulate across the daemon's whole lifetime — a daily `max` spans ticks).
    type SharedBudget = Option<std::sync::Arc<std::sync::Mutex<crate::runtime::budget_kernel::BudgetGate>>>;
    let (listeners, window, budget): (
        Vec<(CronSchedule, Vec<IRFlowNode>, String)>,
        Option<IRWindow>,
        SharedBudget,
    ) = {
        let Some(daemon) = ir.daemons.iter().find(|d| d.name == daemon_name) else {
            eprintln!("run_daemon: daemon '{daemon_name}' not in IR — nothing to drive");
            return;
        };
        let window = bound_window(&ir, daemon).cloned();
        let budget = daemon.budget.as_ref().map(|b| {
            std::sync::Arc::new(std::sync::Mutex::new(
                crate::runtime::budget_kernel::BudgetGate::from_ir(b, &daemon_name, clock.now()),
            ))
        });
        let listeners = cron_listeners(daemon)
            .into_iter()
            .map(|l| (l.schedule, l.body.to_vec(), l.channel))
            .collect();
        (listeners, window, budget)
    };

    let mut tasks = Vec::new();
    for (schedule, body, channel) in listeners {
        let ir = ir.clone();
        let clock = clock.clone();
        let cancel = cancel.clone();
        let daemon_name = daemon_name.clone();
        let backend = backend.clone();
        let window = window.clone();
        let budget = budget.clone();
        tasks.push(tokio::spawn(async move {
            loop {
                let now = clock.now();
                let Some(next) = next_fire_after(&schedule, now) else {
                    eprintln!(
                        "daemon '{daemon_name}' listener '{channel}': schedule never \
                         fires within the horizon — stopping this listener"
                    );
                    return;
                };
                let wait = (next - now).to_std().unwrap_or(std::time::Duration::ZERO);
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(wait) => {
                        // v2.27.0 — the temporal guard. `next` is the wall-clock
                        // minute the tick fires at; evaluate the bound window there.
                        if let Some(w) = &window {
                            match decide(next, w) {
                                WindowAction::Fire => {} // inside → fall through and fire.
                                WindowAction::Skip => {
                                    eprintln!(
                                        "daemon '{daemon_name}' tick at {next} is OUTSIDE \
                                         window '{}' (on_outside: skip) — dropped",
                                        w.name
                                    );
                                    continue;
                                }
                                WindowAction::Warn => {
                                    eprintln!(
                                        "daemon '{daemon_name}' tick at {next} is OUTSIDE \
                                         window '{}' (on_outside: warn) — firing anyway",
                                        w.name
                                    );
                                    // fall through and fire.
                                }
                                WindowAction::Defer { open_at } => {
                                    // The OSS single-process supervisor cannot persist a
                                    // defer ledger; it degrades `defer` to a logged skip.
                                    // True coalesced fire-once-when-open is the v2.27.0
                                    // enterprise defer-ledger.
                                    eprintln!(
                                        "daemon '{daemon_name}' tick at {next} is OUTSIDE \
                                         window '{}' (on_outside: defer) — OSS degrades defer to \
                                         skip (next opening {open_at:?}); the enterprise \
                                         defer-ledger fires it once when the window opens",
                                        w.name
                                    );
                                    continue;
                                }
                            }
                        }
                        let results =
                            execute_listener_body(&ir, &body, &backend, "<daemon>", budget.clone());
                        for (flow, res) in results {
                            if let Err(e) = res {
                                eprintln!(
                                    "daemon '{daemon_name}' tick → flow '{flow}' failed: {e}"
                                );
                            }
                        }
                    }
                }
            }
        }));
    }
    for t in tasks {
        let _ = t.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn every_five_minutes_next_fire() {
        let s = CronSchedule::parse("*/5 * * * *").unwrap();
        // 10:02 → next is 10:05.
        assert_eq!(next_fire_after(&s, at(2026, 6, 26, 10, 2)), Some(at(2026, 6, 26, 10, 5)));
        // 10:05 exactly → strictly after → 10:10.
        assert_eq!(next_fire_after(&s, at(2026, 6, 26, 10, 5)), Some(at(2026, 6, 26, 10, 10)));
        // 10:57 → rolls to 11:00.
        assert_eq!(next_fire_after(&s, at(2026, 6, 26, 10, 57)), Some(at(2026, 6, 26, 11, 0)));
    }

    #[test]
    fn daily_at_specific_time_rolls_to_next_day() {
        let s = CronSchedule::parse("30 9 * * *").unwrap(); // 09:30 daily
        assert_eq!(next_fire_after(&s, at(2026, 6, 26, 9, 0)), Some(at(2026, 6, 26, 9, 30)));
        // After today's 09:30 → tomorrow 09:30.
        assert_eq!(next_fire_after(&s, at(2026, 6, 26, 9, 30)), Some(at(2026, 6, 27, 9, 30)));
    }

    #[test]
    fn weekday_business_hours_skips_weekend() {
        // 0 9 * * 1-5 = 09:00 Mon–Fri. 2026-06-26 is a Friday; next Mon is 29th.
        let s = CronSchedule::parse("0 9 * * 1-5").unwrap();
        // Friday 10:00 → next is Monday 09:00 (skips Sat/Sun).
        let fired = next_fire_after(&s, at(2026, 6, 26, 10, 0)).unwrap();
        assert_eq!(fired, at(2026, 6, 29, 9, 0));
        assert_eq!(fired.weekday().num_days_from_sunday(), 1, "Monday");
    }

    #[test]
    fn next_fires_sequence() {
        let s = CronSchedule::parse("*/15 * * * *").unwrap();
        let fires = next_fires(&s, at(2026, 6, 26, 8, 0), 3);
        assert_eq!(
            fires,
            vec![at(2026, 6, 26, 8, 15), at(2026, 6, 26, 8, 30), at(2026, 6, 26, 8, 45)]
        );
    }

    #[test]
    fn impossible_schedule_yields_none() {
        // Feb 30 never occurs.
        let s = CronSchedule::parse("0 0 30 2 *").unwrap();
        assert_eq!(next_fire_after(&s, at(2026, 1, 1, 0, 0)), None);
    }

    fn ir_with_daemon(src: &str) -> axon_frontend::ir_nodes::IRProgram {
        let tokens = axon_frontend::lexer::Lexer::new(src, "d.axon").tokenize().unwrap();
        let program = axon_frontend::parser::Parser::new(tokens).parse().unwrap();
        axon_frontend::ir_generator::IRGenerator::new().generate(&program)
    }

    #[test]
    fn extracts_cron_listeners_and_invocations() {
        let ir = ir_with_daemon(
            "flow HibernateSession() -> Unit { step S { ask: \"x\" output: Unit } }\n\
             daemon Cleaner {\n\
               goal: \"clean\"\n\
               listen \"cron:*/5 * * * *\" as tick { run HibernateSession() }\n\
               listen \"user_events\" as e { run HibernateSession() }\n\
             }",
        );
        let daemon = ir.daemons.iter().find(|d| d.name == "Cleaner").unwrap();
        let crons = cron_listeners(daemon);
        // Only the cron listener is a timer source (the event listener is not).
        assert_eq!(crons.len(), 1);
        assert_eq!(crons[0].channel, "cron:*/5 * * * *");
        let invs = run_invocations(crons[0].body);
        assert_eq!(invs.len(), 1);
        assert_eq!(invs[0].flow_name, "HibernateSession");
    }

    // ── v2.31.0 — event listeners + in-process delivery ───────────

    #[test]
    fn event_listeners_are_the_complement_of_cron() {
        let ir = ir_with_daemon(
            "type Hib { tenant_id: String }\n\
             channel HibCh { message: Hib  qos: at_least_once  persistence: persistent_axonstore }\n\
             flow Learn() -> Unit { probe p }\n\
             daemon Mixed {\n\
               requires: [flow.execute]\n\
               listen \"cron:*/5 * * * *\" as tick { run Learn() }\n\
               listen HibCh as ev { run Learn() }\n\
             }",
        );
        let daemon = ir.daemons.iter().find(|d| d.name == "Mixed").unwrap();
        // cron_listeners sees only the timer; event_listeners only the channel.
        assert_eq!(cron_listeners(daemon).len(), 1);
        let evs = event_listeners(daemon);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].channel, "HibCh");
        assert_eq!(evs[0].alias, "ev");
    }

    #[tokio::test]
    async fn emit_reaches_a_daemon_listener_in_process() {
        // v2.31.0 — the headline: a producer emits on a typed channel, the bus
        // carries it, and the consumer daemon's `listen Channel` body RUNS
        // with the event payload bound to the consumer flow's parameters.
        // This is the flow→daemon delivery the v2.4.0 `axon-W009` said did not
        // exist — now it does (in-process; durable at-least-once is v2.31.0).
        let ir = ir_with_daemon(
            "type Hib { tenant_id: String  session_id: String }\n\
             channel HibCh { message: Hib  qos: at_least_once  persistence: persistent_axonstore }\n\
             flow Learn(tenant_id: String, session_id: String) -> String { return \"learned\" }\n\
             daemon IntentLearner {\n\
               requires: [flow.execute]\n\
               listen HibCh as ev { run Learn() }\n\
             }",
        );
        // Producer side: emit onto the shared bus (built from the IR, so the
        // channel is registered). The run_emit→bus routing is tested in
        // wire_integrations; here we exercise bus + consumer delivery.
        let bus = crate::runtime::channels::TypedEventBus::from_ir_program(&ir);
        let payload = serde_json::json!({ "tenant_id": "acme", "session_id": "s1" });
        bus.emit(
            "HibCh",
            crate::runtime::channels::TypedPayload::Scalar(payload.clone()),
        )
        .await
        .expect("emit to a registered channel succeeds");

        // Consumer side: drain the bus + deliver to the daemon listener.
        let event = bus.receive("HibCh").await.expect("the emitted event is queued");
        let received = match event.payload {
            crate::runtime::channels::TypedPayload::Scalar(v) => v,
            other => panic!("expected scalar payload, got {other:?}"),
        };
        assert_eq!(received["tenant_id"], "acme");

        let results = deliver_typed_event(&ir, "HibCh", &received, "stub", "<test>", None);
        assert_eq!(results.len(), 1, "exactly one listener fired");
        let (daemon, flow, res) = &results[0];
        assert_eq!(daemon, "IntentLearner");
        assert_eq!(flow, "Learn");
        assert!(
            res.is_ok(),
            "the consumer flow ran to completion (err: {:?})",
            res.as_ref().err()
        );
    }

    // ── v2.31.0 — at-least-once delivery (retry / dead-letter / qos) ──

    #[test]
    fn deliver_with_retry_acks_on_a_later_attempt() {
        // v2.31.0 — at_least_once retries; a body that fails twice then
        // succeeds is Acked on the 3rd attempt.
        let mut n = 0;
        let outcome = deliver_with_retry("at_least_once", 5, || {
            n += 1;
            if n < 3 { Err(format!("transient {n}")) } else { Ok(()) }
        });
        assert_eq!(outcome, DeliveryOutcome::Acked { attempts: 3 });
    }

    #[test]
    fn deliver_with_retry_dead_letters_after_the_ceiling() {
        // v2.31.0 — a body that always fails is dead-lettered after exactly
        // MAX_DELIVERY_ATTEMPTS (no infinite redelivery).
        let mut n = 0;
        let outcome = deliver_with_retry("at_least_once", MAX_DELIVERY_ATTEMPTS, || {
            n += 1;
            Err(format!("perma-fail {n}"))
        });
        match outcome {
            DeliveryOutcome::DeadLettered { attempts, last_error } => {
                assert_eq!(attempts, MAX_DELIVERY_ATTEMPTS);
                assert!(last_error.contains("perma-fail"));
            }
            other => panic!("expected DeadLettered, got {other:?}"),
        }
        assert_eq!(n, MAX_DELIVERY_ATTEMPTS, "tried exactly the ceiling, no more");
    }

    #[test]
    fn deliver_with_retry_at_most_once_drops_without_retry() {
        // v2.31.0 — at_most_once runs ONCE; a failure is dropped (the
        // best-effort qos the program declared), never retried.
        let mut n = 0;
        let outcome = deliver_with_retry("at_most_once", 5, || {
            n += 1;
            Err("boom".to_string())
        });
        assert!(matches!(outcome, DeliveryOutcome::Dropped { .. }));
        assert_eq!(n, 1, "at_most_once never retries");
    }

    #[tokio::test]
    async fn reliable_delivery_acks_a_passing_listener() {
        let ir = ir_with_daemon(
            "type Hib { tenant_id: String }\n\
             channel HibCh { message: Hib  qos: at_least_once }\n\
             flow Learn(tenant_id: String) -> String { return \"ok\" }\n\
             daemon D { requires: [flow.execute]  listen HibCh as ev { run Learn() } }",
        );
        let payload = serde_json::json!({ "tenant_id": "acme" });
        let out = deliver_typed_event_reliable(&ir, "HibCh", &payload, "stub", "<test>", None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "D");
        assert!(matches!(out[0].1, DeliveryOutcome::Acked { .. }), "{:?}", out[0].1);
    }

    #[tokio::test]
    async fn reliable_delivery_dead_letters_a_failing_listener() {
        // The body runs an UNDEFINED flow → execute_server_flow errors every
        // attempt → dead-lettered (at_least_once), not silently lost.
        let ir = ir_with_daemon(
            "type Hib { tenant_id: String }\n\
             channel HibCh { message: Hib  qos: at_least_once }\n\
             daemon D { requires: [flow.execute]  listen HibCh as ev { run Ghost() } }",
        );
        let out = deliver_typed_event_reliable(
            &ir, "HibCh", &serde_json::json!({}), "stub", "<test>", None,
        );
        assert_eq!(out.len(), 1);
        match &out[0].1 {
            DeliveryOutcome::DeadLettered { attempts, .. } => {
                assert_eq!(*attempts, MAX_DELIVERY_ATTEMPTS)
            }
            other => panic!("expected DeadLettered, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reliable_delivery_at_most_once_drops_a_failing_listener() {
        // Same failing body, but the channel is at_most_once → one attempt,
        // dropped (no retry storm).
        let ir = ir_with_daemon(
            "type T { x: String }\n\
             channel C { message: T  qos: at_most_once }\n\
             daemon D { requires: [flow.execute]  listen C as ev { run Ghost() } }",
        );
        let out = deliver_typed_event_reliable(&ir, "C", &serde_json::json!({}), "stub", "<test>", None);
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0].1, DeliveryOutcome::Dropped { .. }), "{:?}", out[0].1);
    }

    // ── v2.31.0 — durable outbox: drain + redelivery-after-down ───────

    #[tokio::test]
    async fn outbox_event_redelivers_after_the_consumer_was_down() {
        // The headline v2.31.0 guarantee: a producer appends to the durable
        // outbox while the consumer/supervisor is DOWN (no drain runs). The
        // event WAITS in the persisted log; when a drain runs, it delivers.
        use crate::event_outbox::{EventOutbox, InMemoryEventOutbox};
        let ir = ir_with_daemon(
            "type Hib { tenant_id: String }\n\
             channel HibCh { message: Hib  qos: at_least_once  persistence: persistent_axonstore }\n\
             flow Learn(tenant_id: String) -> String { return \"ok\" }\n\
             daemon D { requires: [flow.execute]  listen HibCh as ev { run Learn() } }",
        );
        let outbox = InMemoryEventOutbox::new();
        // Producer emits while no drain runs (consumer down): the event is
        // durably queued, not lost.
        outbox.append("HibCh", serde_json::json!({ "tenant_id": "acme" }));
        assert_eq!(outbox.unprocessed("HibCh").len(), 1, "queued, awaiting delivery");

        // Consumer comes back → a drain delivers + acks.
        let outcomes = drain_outbox(&ir, &outbox, "HibCh", "stub", "<test>", None);
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(outcomes[0].2, DeliveryOutcome::Acked { .. }), "{:?}", outcomes[0].2);
        assert!(outbox.unprocessed("HibCh").is_empty(), "acked → not redelivered");

        // A second drain is a no-op (the entry was acked — no double-fire).
        assert!(drain_outbox(&ir, &outbox, "HibCh", "stub", "<test>", None).is_empty());
    }

    #[tokio::test]
    async fn outbox_dead_letters_a_failing_listener_then_stops_redelivering() {
        // A failing body (undefined flow) is retried to the ceiling, dead-
        // lettered, AND acked — so the durable log does not redeliver it
        // forever (delivery stays TOTAL).
        use crate::event_outbox::{EventOutbox, InMemoryEventOutbox};
        let ir = ir_with_daemon(
            "type Hib { tenant_id: String }\n\
             channel HibCh { message: Hib  qos: at_least_once  persistence: persistent_axonstore }\n\
             daemon D { requires: [flow.execute]  listen HibCh as ev { run Ghost() } }",
        );
        let outbox = InMemoryEventOutbox::new();
        outbox.append("HibCh", serde_json::json!({}));
        let outcomes = drain_outbox(&ir, &outbox, "HibCh", "stub", "<test>", None);
        assert!(matches!(outcomes[0].2, DeliveryOutcome::DeadLettered { .. }));
        assert!(outbox.unprocessed("HibCh").is_empty(), "dead-lettered entries are acked");
    }

    // ── v2.31.0 — qos fan-out (broadcast = all / else = single-consumer) ──

    #[test]
    fn fan_out_count_broadcast_is_all_else_one() {
        assert_eq!(fan_out_count("broadcast", 3), 3, "broadcast → every listener");
        for single in ["at_least_once", "at_most_once", "exactly_once", "queue"] {
            assert_eq!(fan_out_count(single, 3), 1, "{single} → single-consumer");
        }
        // No listeners → zero either way.
        assert_eq!(fan_out_count("broadcast", 0), 0);
        assert_eq!(fan_out_count("at_least_once", 0), 0);
    }

    #[tokio::test]
    async fn broadcast_fans_out_to_every_listener() {
        let ir = ir_with_daemon(
            "type E { x: String }\n\
             channel BroadcastCh { message: E  qos: broadcast }\n\
             flow A() -> String { return \"a\" }\n\
             flow B() -> String { return \"b\" }\n\
             daemon DA { requires: [flow.execute]  listen BroadcastCh as ev { run A() } }\n\
             daemon DB { requires: [flow.execute]  listen BroadcastCh as ev { run B() } }",
        );
        let out = deliver_typed_event_reliable(
            &ir, "BroadcastCh", &serde_json::json!({}), "stub", "<test>", None,
        );
        assert_eq!(out.len(), 2, "broadcast → BOTH daemons fire: {out:?}");
        assert!(out.iter().all(|(_, o)| matches!(o, DeliveryOutcome::Acked { .. })));
    }

    #[tokio::test]
    async fn single_consumer_qos_delivers_to_one_listener_only() {
        // `at_least_once` is single-consumer: with two daemons listening on
        // the same channel, exactly ONE fires (a work-queue, not a fan-out).
        let ir = ir_with_daemon(
            "type E { x: String }\n\
             channel QCh { message: E  qos: at_least_once }\n\
             flow A() -> String { return \"a\" }\n\
             daemon DA { requires: [flow.execute]  listen QCh as ev { run A() } }\n\
             daemon DB { requires: [flow.execute]  listen QCh as ev { run A() } }",
        );
        let out =
            deliver_typed_event_reliable(&ir, "QCh", &serde_json::json!({}), "stub", "<test>", None);
        assert_eq!(out.len(), 1, "single-consumer → exactly one fired: {out:?}");
    }

    // ── v2.31.0 — ReplayToken integration ─────────────────────────────

    #[test]
    fn channel_event_token_captures_the_causal_receipt() {
        // v2.31.0 — an `emit`/`deliver` mints a ReplayToken: the effect name,
        // the payload (+ flow_id) as inputs, a deterministic channel slug.
        let tok = mint_channel_event_token(
            "deliver:HibCh",
            "IntentLearner",
            &serde_json::json!({ "tenant_id": "acme" }),
            serde_json::json!({ "outcome": "acked", "attempts": 1 }),
        );
        assert_eq!(tok.effect_name, "deliver:HibCh");
        assert_eq!(tok.model_version, "axon.builtin.channel.v1");
        assert_eq!(tok.inputs["flow_id"], "IntentLearner");
        assert_eq!(tok.inputs["payload"]["tenant_id"], "acme");
        assert_eq!(tok.outputs["outcome"], "acked");
        assert!(!tok.token_hash_hex.is_empty(), "the receipt is hashed");
    }

    #[tokio::test]
    async fn deliver_and_record_logs_a_deliver_token_per_outcome() {
        use crate::replay_token::{InMemoryReplayLog, ReplayLog};
        let ir = ir_with_daemon(
            "type Hib { tenant_id: String }\n\
             channel HibCh { message: Hib  qos: at_least_once }\n\
             flow Learn(tenant_id: String) -> String { return \"ok\" }\n\
             daemon D { requires: [flow.execute]  listen HibCh as ev { run Learn() } }",
        );
        let log = InMemoryReplayLog::new();
        let payload = serde_json::json!({ "tenant_id": "acme" });
        let outcomes = deliver_and_record(
            &ir, "HibCh", &payload, "stub", "<test>", None, Some(&log),
        )
        .await;
        assert_eq!(outcomes.len(), 1);
        // The Chan-Input reduction was recorded in the replay chain.
        assert_eq!(log.len(), 1, "one deliver token recorded");
        let tokens = log.tokens_for_flow("D").await.unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].effect_name, "deliver:HibCh");
    }

    #[tokio::test]
    async fn deliver_and_record_without_a_log_is_pure_delivery() {
        let ir = ir_with_daemon(
            "type Hib { tenant_id: String }\n\
             channel HibCh { message: Hib }\n\
             flow Learn(tenant_id: String) -> String { return \"ok\" }\n\
             daemon D { requires: [flow.execute]  listen HibCh as ev { run Learn() } }",
        );
        let out = deliver_and_record(
            &ir, "HibCh", &serde_json::json!({ "tenant_id": "x" }), "stub", "<test>", None, None,
        )
        .await;
        assert_eq!(out.len(), 1, "delivery still happens with no replay sink");
    }

    #[test]
    fn deliver_to_a_channel_with_no_listener_is_an_empty_fan_out() {
        let ir = ir_with_daemon(
            "type Hib { tenant_id: String }\n\
             channel HibCh { message: Hib }\n\
             flow Learn() -> String { return \"ok\" }\n\
             daemon D { requires: [flow.execute]  listen HibCh as ev { run Learn() } }",
        );
        // No listener on `OtherCh` → zero deliveries (valid, not an error).
        let out = deliver_typed_event(&ir, "OtherCh", &serde_json::json!({}), "stub", "<test>", None);
        assert!(out.is_empty());
    }

    #[test]
    fn bound_window_resolves_the_daemon_guard() {
        let ir = ir_with_daemon(
            "flow Send() -> Unit { step S { ask: \"x\" output: Unit } }\n\
             window BusinessHours {\n\
               timezone: \"America/Bogota\"\n\
               allow: [ { days: Mon..Fri, hours: 9..18 } ]\n\
               on_outside: skip\n\
             }\n\
             daemon Scheduler {\n\
               window: BusinessHours\n\
               requires: [flow.execute]\n\
               listen \"cron:*/5 * * * *\" as tick { run Send() }\n\
             }",
        );
        let daemon = ir.daemons.iter().find(|d| d.name == "Scheduler").unwrap();
        assert_eq!(daemon.window_ref, "BusinessHours");
        let w = bound_window(&ir, daemon).expect("window resolves");
        assert_eq!(w.name, "BusinessHours");
        assert_eq!(w.timezone, "America/Bogota");
        assert_eq!(w.on_outside, "skip");

        // A daemon with no `window:` resolves to None (unguarded — pre-v2.27.0).
        let unguarded = ir_with_daemon(
            "flow Send() -> Unit { step S { ask: \"x\" output: Unit } }\n\
             daemon Plain {\n\
               listen \"cron:*/5 * * * *\" as tick { run Send() }\n\
             }",
        );
        let d2 = unguarded.daemons.iter().find(|d| d.name == "Plain").unwrap();
        assert!(bound_window(&unguarded, d2).is_none());
    }

    #[test]
    fn budget_gate_builds_from_a_parsed_daemon_and_enforces() {
        // v2.28.0 — parse → IR → BudgetGate, end to end. A daemon whose budget
        // allows 1 TelnyxCall/hour: the first emission is granted, the second is
        // denied under the (default) `block` policy.
        let ir = ir_with_daemon(
            "tool TelnyxCall { provider: http timeout: 5s }\n\
             flow SendBatch() -> Unit { step S { ask: \"x\" output: Unit } }\n\
             daemon OutboundScheduler {\n\
               requires: [flow.execute]\n\
               budget {\n\
                 max: 1 per hour on Tool(TelnyxCall)\n\
                 on_exhausted: block\n\
               }\n\
               listen \"cron:*/5 * * * *\" as t { run SendBatch() }\n\
             }",
        );
        let daemon = ir.daemons.iter().find(|d| d.name == "OutboundScheduler").unwrap();
        let budget = daemon.budget.as_ref().expect("budget lowered onto the daemon");
        let now: chrono::DateTime<chrono::Utc> = "2026-06-29T00:00:00Z".parse().unwrap();
        let mut gate = crate::runtime::budget_kernel::BudgetGate::from_ir(budget, &daemon.name, now);

        use crate::runtime::budget_kernel::GateDecision;
        assert_eq!(gate.gate("TelnyxCall", now), GateDecision::Allow);
        match gate.gate("TelnyxCall", now) {
            GateDecision::Deny { on_exhausted, .. } => assert_eq!(on_exhausted, "block"),
            other => panic!("expected Deny, got {other:?}"),
        }
        // A tool with no quota is never gated.
        assert_eq!(gate.gate("SomeOtherTool", now), GateDecision::Allow);
    }
}
