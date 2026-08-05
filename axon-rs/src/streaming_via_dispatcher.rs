//! §Fase 33.z.b — Streaming-via-dispatcher graft skeleton.
//!
//! This module ships the alternative production-path producer that
//! routes adopter traffic through `flow_dispatcher::dispatch_node`
//! end-to-end. It is the v1.27.0 lift of the 33.y structurally-
//! complete dispatcher (45/45 IRFlowNode variants with compiler-
//! enforced exhaustive match) from the test surface to the
//! production SSE wire.
//!
//! # Why a new module, not a `server_execute_streaming` rewrite?
//!
//! The 33.z.b sub-fase ships the producer behind a feature flag
//! (`AXON_STREAMING_VIA_DISPATCHER`, default OFF) so v1.27.0-alpha
//! adopters who DON'T opt in see byte-identical v1.26.0 wire
//! behavior (D4 safety net during the migration). When the flag is
//! ON, `server_execute_streaming` invokes
//! [`run_streaming_via_dispatcher`] instead of the v1.26.0 paths
//! (`run_streaming_async_path` for canonical Step + `run_streaming_legacy_path`
//! for everything else). Sub-fase 33.z.c flips the flag default
//! from OFF to ON for v1.27.0 stable; sub-fase 33.z.e deletes the
//! flag + the legacy path entirely.
//!
//! Mirrors the proven Fase 33.x.h opt-in BPE chunking pattern —
//! land a new hot path behind a flag, validate with adopters,
//! flip default, then retire the legacy path.
//!
//! # Architecture
//!
//! The producer:
//!
//! 1. Emits `FlowExecutionEvent::FlowStart` on the same mpsc channel
//!    the SSE consumer reads from (path-agnostic — the consumer
//!    doesn't know whether events came from the legacy producer or
//!    this one).
//! 2. Compiles `source` → AST → IR via the unified pipeline
//!    (`crate::flow_plan::compile_source_to_ir`). Parse / type-check
//!    / IR-generation errors emit `FlowExecutionEvent::FlowError`
//!    and the producer exits cleanly.
//! 3. Resolves the requested `flow_name` from the IR's flow list.
//!    Missing flow → structured `FlowError` + clean exit.
//! 4. Constructs a [`DispatchCtx`] with the inbound `tx` so every
//!    `dispatch_node` call emits events on the SSE consumer's
//!    channel directly — no intermediate buffering.
//! 5. Walks `flow.body` in order. For each top-level `IRFlowNode`,
//!    calls `flow_dispatcher::dispatch_node(&node, &mut ctx).await`.
//!    The dispatcher's per-variant handler emits StepStart /
//!    StepToken / StepComplete / ToolCall events on `ctx.tx`.
//! 6. Honors sentinel outcomes from the dispatcher: `Return { value }`
//!    breaks the loop (a `return foo` statement short-circuits the
//!    flow); `Break` and `LoopContinue` are loop-only sentinels and
//!    are ignored at the top level (they only propagate inside
//!    `ForIn` bodies, which the dispatcher's orchestration handlers
//!    handle internally).
//! 7. Emits `FlowExecutionEvent::FlowComplete` with the final
//!    success flag + accumulated token counts + audit summary.
//!
//! # Cancel discipline (D3)
//!
//! The producer threads the same `CancellationFlag` from
//! `server_execute_streaming` into the `DispatchCtx`. Every
//! per-variant handler checks the flag at its entry (33.y D3
//! invariant enforced by the drift gate). A client disconnect
//! during a deeply-nested orchestration shape (e.g.,
//! `for x in xs { par { step A {...} step B {...} } }`) aborts
//! ALL concurrent branches within p95 ≤100ms wall-clock budget
//! preserved from 33.x.e.
//!
//! # Wire byte-compat (D4)
//!
//! Canonical `step S { ask: "..." output: Stream<Token> }` + stub
//! backend continues to emit exactly 1 axon.token "(stub)" + 1
//! axon.complete — identical to the v1.26.0 `run_streaming_async_path`
//! wire body. This is enforced by the 33.z.b integration test pack:
//! every shape that worked in v1.26.0 keeps working byte-equal
//! with the flag ON.
//!
//! # What this module does NOT do
//!
//! - Per-step replay binding extension (D6 branch_path) — that's
//!   33.z.c scope; the dispatcher's `StepAuditRecord` shape with
//!   `branch_path` already exists from 33.y, but the SSE handler
//!   side of the replay row capture is in 33.z.c.
//! - `axon.tool_call` SSE event wire emission — also 33.z.c; this
//!   module emits `FlowExecutionEvent::ToolCall` (33.y.k) but the
//!   SSE consumer still silently consumes (the 33.y.k baseline).
//! - 50-flow sync↔async parity corpus — 33.z.d.
//! - Legacy path retirement — 33.z.e.

use crate::cancel_token::CancellationFlag;
use crate::flow_dispatcher::{dispatch_node, DispatchCtx, NodeOutcome};
use crate::flow_execution_event::{now_ms, FlowExecutionEvent};
use tokio::sync::mpsc::UnboundedSender;

/// §Fase 33.z.b — Drive a flow's execution through the
/// `flow_dispatcher::dispatch_node` hot path end-to-end, emitting
/// FlowExecutionEvents on the inbound mpsc channel.
///
/// Signature mirrors [`run_streaming_legacy_path`] for drop-in
/// replacement under the `AXON_STREAMING_VIA_DISPATCHER` flag.
///
/// # Arguments
///
/// - `source` — the `.axon` source text (deploy-time captured).
/// - `source_file` — for diagnostic locality in error messages.
/// - `flow_name` — the flow to dispatch (resolved from IR).
/// - `backend` — the LLM backend name (passed through to DispatchCtx).
/// - `cancel` — the cancellation flag shared with the SSE handler.
/// - `tx` — the FlowExecutionEvent mpsc channel the SSE consumer
///   reads from.
/// - `request_body` — §Fase 37.b: the parsed HTTP request body. The
///   flow's declared parameters bind from its same-named fields (the
///   Request Binding Contract) and seed `DispatchCtx.let_bindings`
///   before the flow walk.
///
/// # Cancel safety
///
/// Every send wraps a `cancel.is_cancelled()` check + a tx-closed
/// check (matches the 33.x.f cancel-safety helper pattern in the
/// legacy path). On cancel OR consumer-drop, the producer exits
/// early without emitting further events. The dispatcher's per-
/// variant handlers carry the same discipline internally.
pub async fn run_streaming_via_dispatcher(
    source: String,
    source_file: String,
    flow_name: String,
    backend: String,
    cancel: CancellationFlag,
    tx: UnboundedSender<FlowExecutionEvent>,
    // §Fase 33.z.c — External side-channels threaded from
    // `server_execute_streaming` so the dispatcher's per-variant
    // handlers populate the SAME Mutexes the SSE wire's
    // `enforcement_summary`, `step_audit`, and `runtime_warnings`
    // fields read from. Without these, the dispatcher's fresh internal
    // Arcs (created by `DispatchCtx::new`) would carry the counters
    // but never reach the wire — adopter loses D4 byte-compat with
    // the v1.25.0 33.x.d/f production-path surface.
    enforcement_summaries: std::sync::Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<
                String,
                crate::execution_result::EnforcementSummaryWire,
            >,
        >,
    >,
    step_audit_records: std::sync::Arc<
        tokio::sync::Mutex<Vec<crate::axonendpoint_replay::StepAuditRecord>>,
    >,
    runtime_warnings: std::sync::Arc<
        tokio::sync::Mutex<Vec<crate::runtime_warnings::RuntimeWarning>>,
    >,
    // §Fase 91.b — external temporal side-channel (the same discipline as
    // the three above): the dispatcher's per-step `now:` renders populate
    // THIS shared state, so the SSE `axon.complete` envelope can carry the
    // run's temporal record (`captured_utc` + `tzdb_version` + zones) at
    // §55.c parity with the sync `FlowEnvelope.temporal_context`.
    temporal_state: std::sync::Arc<
        std::sync::Mutex<crate::temporal_context::TemporalState>,
    >,
    // §Fase 35.j (Pillar IV) — the capability slugs the request
    // carries (the JWT bearer's `capabilities` claim). `Some` activates
    // the store handlers' runtime capability re-check; `None` defers to
    // the type-checker's compile-time guarantee.
    held_capabilities: Option<Vec<String>>,
    // §Fase 37.b (D1) — the parsed HTTP request body. The flow's
    // declared parameters bind from its same-named fields and seed
    // `DispatchCtx.let_bindings` before the flow walk, so a `${param}`
    // interpolates in `where:` clauses, step `ask:` prompts, and
    // `persist`/`mutate` field blocks. `None` for a request with no
    // body, or for a non-dynamic-route caller (e.g. a `/v1/execute/sse`
    // direct hit) — the flow then runs with only its own `let` / step
    // bindings (D5 backwards-compat).
    request_body: Option<serde_json::Value>,
    // §Fase 37.y (D3) — URL path captures from the dynamic-route
    // dispatcher (empty for non-dynamic-route callers). Threaded to
    // `bind_request` alongside body + query.
    request_path: std::collections::HashMap<String, String>,
    // §Fase 37.y (D3) — URL query string parsed name → value.
    request_query: std::collections::HashMap<String, String>,
    // §Fase 58.g (D7) — optional per-tenant / per-server tool base URL.
    // When `Some`, every URL-dispatched program tool with a RELATIVE
    // `runtime` is resolved against it before the dispatcher walks any
    // node (`{base}/{slug}`); absolute runtimes stay verbatim (D5).
    // `None` → no resolution (the pre-§58.g behavior).
    tool_base_url: Option<String>,
    // §Fase 65.C — the per-tenant API key (resolved from the tenant secrets
    // manager by the SSE handler). When `Some`, the dispatcher's LLM handlers
    // use THIS tenant's key instead of the process env var — closing the gap
    // where the streaming path could only ever use the env key. `None` ⇒
    // env-key behavior, unchanged.
    api_key: Option<String>,
    // §Fase 114 — the per-deployment channel guards (`resource.capacity`
    // semaphores + the `lease` decay guard), resolved from `ServerState` by
    // `server_execute_streaming`. Threaded onto the DispatchCtx so the SSE tool
    // path (`pure_shape`) charges the lease + holds a concurrency permit for a
    // resourced tool — parity with the sync `execute_server_flow` path. `None`
    // (no `capacity`-bounded resource / no `lease`) ⇒ unbounded, as before.
    channel_semaphores: Option<
        std::sync::Arc<crate::channel_semaphore::ChannelSemaphores>,
    >,
    tool_leases: Option<std::sync::Arc<crate::resource_lease::ResourceLeaseGuard>>,
) {
    // Cancel-safety helper — mirrors the legacy path's `emit` closure.
    // Returns `Err(())` when the producer should exit early (cancel
    // fired OR consumer dropped the receiver).
    let emit = |event: FlowExecutionEvent| -> Result<(), ()> {
        if cancel.is_cancelled() {
            return Err(());
        }
        tx.send(event).map_err(|_| ())
    };

    let exec_start = std::time::Instant::now();

    // §1 — FlowStart fires BEFORE we attempt compilation so any
    // consumer can bind side-effect state (audit row, observability
    // span) immediately.
    if emit(FlowExecutionEvent::FlowStart {
        flow_name: flow_name.clone(),
        backend: backend.clone(),
        timestamp_ms: now_ms(),
    })
    .is_err()
    {
        return;
    }

    // §2 — Compile source → IR via the unified pipeline. Parse /
    // type-check / IR-generation errors emit FlowError + exit.
    let (ast_program, ir) = match crate::flow_plan::compile_source_to_ir(&source, &source_file) {
        Ok(pair) => pair,
        Err(plan_error) => {
            // §Fase 37.e (D6) — honest failure: the diagnostic reaches
            // the `axon.error` wire event (every dialect) AND a
            // structured server log line. A stream that dies says why.
            let detail = format!("compilation failed: {plan_error:?}");
            tracing::error!(
                flow = %flow_name,
                source_file = %source_file,
                detail = %detail,
                "axon streaming flow failed — source did not compile"
            );
            let _ = emit(FlowExecutionEvent::FlowError {
                flow_name: flow_name.clone(),
                error: detail,
                timestamp_ms: now_ms(),
            });
            // §Fase 36.x.c (D1) — exactly one terminator: a
            // compilation failure emits ONLY `FlowError` (the failure
            // terminator). No trailing `FlowComplete`.
            return;
        }
    };

    // §2.5 — Build the axonstore registry (Fase 35.f). The D2 closed-
    // catalog gate runs here: an unknown `backend:` fails fast with a
    // named FlowError before any node dispatches.
    let store_registry = match crate::store::registry::StoreRegistry::build(
        &ir.axonstore_specs,
    ) {
        Ok(r) => std::sync::Arc::new(r),
        Err(reg_error) => {
            // §Fase 37.e (D6) — honest failure: name the cause.
            let detail = format!("axonstore registry: {reg_error}");
            tracing::error!(
                flow = %flow_name,
                detail = %detail,
                "axon streaming flow failed — axonstore registry build"
            );
            let _ = emit(FlowExecutionEvent::FlowError {
                flow_name: flow_name.clone(),
                error: detail,
                timestamp_ms: now_ms(),
            });
            // §Fase 36.x.c (D1) — exactly one terminator: a registry
            // build failure emits ONLY `FlowError`.
            return;
        }
    };

    // §Fase 37.x.j (D2) — Eager acquire ONE PoolConnection per
    // postgresql-backed axonstore the resolved flow body references.
    // Held in an Arc<Mutex<HashMap>> shared with `DispatchCtx` so
    // every wire-integration store handler routes its SQL through the
    // pinned conn. Conns drop when this function returns + the
    // streaming task ends → `after_release(DEALLOCATE ALL)` (Fase
    // 38.x.a D2) wipes any prepared statements before the pool reuses
    // them.
    //
    // The discovery walk is currently a permissive over-acquire: it
    // scans every flow in the IR for postgresql-backed store ops and
    // acquires one pin per unique store_name. This is a deliberate
    // honest-deferral for sub-fase 37.x.j.5 — a precise walk that
    // visits ONLY the resolved flow's body lands in sub-fase 37.x.j.6
    // alongside the par-block isolation. Over-acquire is safe (pins
    // are released on drop) but holds the pool slightly longer than
    // strictly necessary; acceptable trade for shipping the adopter-
    // blocking fix on the 37.x.j.5 timeline.
    let pinned_conns: std::sync::Arc<
        std::sync::Mutex<
            std::collections::HashMap<
                String,
                crate::pinned_conn::PinnedConn,
            >,
        >,
    > = std::sync::Arc::new(std::sync::Mutex::new(
        std::collections::HashMap::new(),
    ));
    {
        let mut needed: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for f in &ir.flows {
            for node in &f.steps {
                // §Fase 37.x.j.5 — match the four SQL store-op variants
                // of `IRFlowNode`. Discovery is permissive: an in_memory
                // store filters out below via `backend_kind`.
                let store_ref: Option<&str> = match node {
                    crate::ir_nodes::IRFlowNode::Persist(p) => Some(p.store_name.as_str()),
                    crate::ir_nodes::IRFlowNode::Retrieve(r) => Some(r.store_name.as_str()),
                    crate::ir_nodes::IRFlowNode::Mutate(m) => Some(m.store_name.as_str()),
                    crate::ir_nodes::IRFlowNode::Purge(p) => Some(p.store_name.as_str()),
                    _ => None,
                };
                if let Some(store_name) = store_ref {
                    if store_registry.backend_kind(store_name)
                        == Some(crate::store::registry::StoreBackendKind::Postgresql)
                    {
                        needed.insert(store_name.to_string());
                    }
                }
            }
        }
        // §Fase 96.a — the filter yields nothing under a session/direct pooler,
        // so the streaming path's eager pin is skipped (store ops release across
        // cognition; no misleading warn). Doctrine `connections_release_across_cognition`.
        for store_name in needed
            .iter()
            .filter(|_| crate::store::postgres_backend::connection_pinning_enabled())
        {
            match store_registry.resolve(store_name) {
                Ok(crate::store::registry::StoreHandle::Postgres(backend)) => {
                    match backend.acquire_pin().await {
                        Ok(conn) => {
                            // §Fase 37.x.j (D4) — emit acquire event.
                            crate::store::pin_observability::emit_pin_acquire(
                                store_name,
                                &flow_name,
                                "", // streaming dispatcher's trace_id is reserved further down
                                "eager",
                                None,
                            );
                            pinned_conns
                                .lock()
                                .unwrap()
                                .insert(store_name.clone(), conn);
                        }
                        Err(e) => {
                            tracing::warn!(
                                target: "axon::store::pin",
                                store_name = %store_name,
                                error = %e,
                                d_letter = "37.x.j.D2",
                                "streaming flow failed to acquire pin; \
                                 falling back to per-step pool acquisition \
                                 (legacy path). Adopter under transaction-\
                                 mode pooler may observe the unnamed-\
                                 prepared-statement race for this store."
                            );
                        }
                    }
                }
                Ok(_) => {} // filtered above; defensive no-op
                Err(e) => {
                    tracing::warn!(
                        target: "axon::store::pin",
                        store_name = %store_name,
                        error = %e,
                        d_letter = "37.x.j.D2",
                        "streaming flow failed to resolve axonstore for \
                         pin acquisition; falling back to per-step pool \
                         acquisition (legacy path)."
                    );
                }
            }
        }
    }

    // §3 — Resolve the requested flow from the IR's flow list.
    // The frontend's IR generator preserves source declaration order
    // so a multi-flow program can dispatch any of them by name.
    let flow = match ir.flows.iter().find(|f| f.name == flow_name) {
        Some(f) => f.clone(),
        None => {
            let available: Vec<String> = ir.flows.iter().map(|f| f.name.clone()).collect();
            // §Fase 37.e (D6) — honest failure: name what was sought
            // and what was available.
            let detail = format!(
                "flow '{}' not found in compiled IR; available: {:?}",
                flow_name, available
            );
            tracing::error!(
                flow = %flow_name,
                detail = %detail,
                "axon streaming flow failed — flow not found"
            );
            let _ = emit(FlowExecutionEvent::FlowError {
                flow_name: flow_name.clone(),
                error: detail,
                timestamp_ms: now_ms(),
            });
            // §Fase 36.x.c (D1) — exactly one terminator: a
            // flow-not-found failure emits ONLY `FlowError`.
            return;
        }
    };

    // §4 — System prompt composition. Mirror of the v1.25.0 33.x.b
    // discipline: persona + context + anchor instructions via the
    // public composer; `backend_tag: None` because the wire's
    // `axon.complete.backend` field already carries the backend name.
    let system_prompt = crate::flow_plan::compose_system_prompt_public(&flow, &ir, None);

    // §Fase 36.i (D4) — Build the tool registry from the compiled
    // IR's tool declarations. Pre-36.i `DispatchCtx::with_tool_registry`
    // had ZERO production callers: `run_streaming_via_dispatcher`
    // attached only the store registry + side-channels, so
    // `ctx.tool_registry` stayed `None` and the dispatcher's
    // streaming-tool branch in `run_step` (`pure_shape.rs`) — gated on
    // `ctx.tool_registry == Some` — could never fire. A
    // `step S { apply: <tool> }` whose `tool` declared
    // `provider: <p>, effects: <stream:<policy>>` silently fell
    // through to the plain-LLM path: the declared provider AND the
    // declared stream effect were ignored at execution.
    //
    // Wiring the registry here makes the dead builder LIVE: a step
    // applying a tool the registry flags `is_streaming` now reaches
    // `run_step_streaming_tool` and executes against the TOOL's
    // declared `provider:` (the per-step backend, overriding the
    // flow-level backend for that step) with the `<stream:<policy>>`
    // effect honored end-to-end. `register_from_ir` auto-derives the
    // `is_streaming` flag from each tool's `effect_row` (Fase 34.c).
    let tool_registry = {
        let mut reg = crate::tool_registry::ToolRegistry::new();
        reg.register_from_ir(&ir.tools);
        // §Fase 58.g (D7) — resolve relative tool runtimes against the
        // caller-supplied per-tenant / per-server base URL. This `reg`
        // is request-scoped (fresh per stream) → no cross-tenant
        // leakage (§58 D10).
        if let Some(base) = tool_base_url.as_deref() {
            reg.resolve_relative_endpoints(base);
        }
        // §Fase 114.d — the STREAMING path must derive a tool's channel from its
        // `resource` too, or a governed tool would resolve its endpoint on the
        // synchronous path (`execute_server_flow`) and NOT on the streaming one —
        // `capacity: 20` real on a POST, a phantom on an SSE. The exact
        // "real-on-one-path, dead-on-the-other" defect §111 exists to end, so it
        // is wired in BOTH places.
        let _refused = reg.resolve_from_resources(
            &ir.resources,
            &crate::resource_resolver::EnvResourceResolver,
        );
        std::sync::Arc::new(reg)
    };

    // §Fase 63.B — build the MDN corpus graphs from `corpus { relations: … }`
    // declarations (request-scoped, like the tool registry). A `navigate
    // <corpus>` over one of these runs real graph navigation.
    let mdn_corpora = {
        let mut map: std::collections::HashMap<String, crate::mdn::Corpus> =
            std::collections::HashMap::new();
        for c in &ir.corpus_specs {
            if c.relations.is_empty() {
                continue;
            }
            let rels: Vec<(String, String, String, f64)> = c
                .relations
                .iter()
                .map(|r| (r.etype.clone(), r.from.clone(), r.to.clone(), r.weight))
                .collect();
            if let Ok(corpus) = crate::mdn::Corpus::from_declaration(&c.documents, &rels) {
                map.insert(c.name.clone(), corpus);
            }
        }
        std::sync::Arc::new(map)
    };

    // §Fase 63.C — the corpora declared `adaptive: true` (memory-enabled).
    // §Fase 64.B — a store-sourced corpus ALSO counts as a graph (its edges
    // live as rows), so adaptive over it is meaningful (the §64.C write-back).
    let mdn_adaptive = {
        let mut set: std::collections::HashSet<String> = std::collections::HashSet::new();
        for c in &ir.corpus_specs {
            if c.adaptive && (!c.relations.is_empty() || c.store_source.is_some()) {
                set.insert(c.name.clone());
            }
        }
        std::sync::Arc::new(set)
    };

    // §Fase 64.B — the DYNAMIC, store-sourced MDN corpora (`corpus N from
    // axonstore { … }`): a navigation over one of these builds a fresh
    // `mdn::Corpus` from the LIVE store rows at navigate-time (tenant-scoped).
    let mdn_store_sources = {
        let mut map: std::collections::HashMap<String, crate::ir_nodes::IRCorpusStoreSource> =
            std::collections::HashMap::new();
        for c in &ir.corpus_specs {
            if let Some(src) = &c.store_source {
                map.insert(c.name.clone(), src.clone());
            }
        }
        std::sync::Arc::new(map)
    };

    // §5 — Construct DispatchCtx. The mpsc tx is the SAME channel
    // the SSE consumer reads from; the dispatcher's events flow
    // directly to the wire with no intermediate buffering.
    let mut ctx = DispatchCtx::new(
        flow_name.clone(),
        backend.clone(),
        system_prompt,
        cancel.clone(),
        tx.clone(),
    )
    .with_external_side_channels(
        enforcement_summaries,
        step_audit_records,
        runtime_warnings,
        // §Fase 67.c — the SSE path does not surface aggregate row counts on
        // the wire (per-step counts already ride the result envelopes), so a
        // throwaway counter satisfies the shared-channel contract.
        std::sync::Arc::new(std::sync::Mutex::new(
            crate::flow_dispatcher::StoreRowCounts::default(),
        )),
    )
    .with_store_registry(store_registry)
    // §Fase 65.C — the per-tenant API key so LLM steps use this tenant's key.
    .with_api_key(api_key)
    // §Fase 65.C.3 — the flow's anchors so each LLM step's output is checked
    // (declared `require:` constraints were silently ignored on SSE before).
    .with_anchors(std::sync::Arc::new(ir.anchors.clone()))
    // §Fase 36.i (D4) — the tool registry, now LIVE on the production
    // SSE path. Activates the dispatcher's streaming-tool branch.
    .with_tool_registry(tool_registry)
    // §Fase 63.B — the MDN corpus graphs, so `navigate <corpus>` runs real
    // signed-EPR / ε-informative graph navigation.
    .with_mdn_corpora(mdn_corpora)
    // §Fase 63.C — mark the adaptive corpora so navigations apply the memory
    // endofunctor + learn.
    .with_mdn_adaptive(mdn_adaptive)
    // §Fase 64.B — register the dynamic store-sourced corpora so `navigate`
    // builds the graph from live store rows at request-time (tenant-scoped).
    .with_mdn_store_sources(mdn_store_sources)
    // §Fase 37.x.j (D2) — install the eager-acquired flow-scoped pin
    // map. The wire-integration store handlers (`run_persist`,
    // `run_retrieve`, `run_mutate`, `run_purge`) consult this map
    // per op via take/dispatch/return discipline so every store op
    // against the same axonstore for this flow lifetime routes
    // through the SAME physical Postgres backend connection.
    .with_pinned_conns(pinned_conns);
    // §Fase 35.j — thread the request's held capabilities into the
    // dispatcher so the store handlers can re-check gated stores.
    ctx.held_capabilities = held_capabilities;
    // §Fase 114 — the per-deployment channel guards, so the SSE tool path
    // (`pure_shape`) enforces `resource.capacity` (a concurrency permit) and
    // breaches a post-expiry `lease` — at parity with the sync path. Before
    // this the SSE ctx carried `None` and a `capacity: N` tool invoked from a
    // streaming endpoint ran unbounded (the "real-on-sync, inert-on-SSE" gap).
    if let Some(sems) = channel_semaphores {
        ctx = ctx.with_channel_semaphores(sems);
    }
    if let Some(leases) = tool_leases {
        ctx = ctx.with_tool_leases(leases);
    }
    // §Fase 91.b — the frame-level declared cognitive timezone (the program's
    // first `context` declaration's `now:` — the same first-context convention
    // `compose_system_prompt_public` uses). A step's own `now:` overrides it.
    // The caller's shared temporal state replaces the ctx's fresh internal Arc
    // (the §33.z.c external-side-channel discipline) so the record reaches the
    // SSE completion envelope.
    ctx.default_now_tz = ir.contexts.first().and_then(|c| c.now_tz.clone());
    ctx.temporal = temporal_state;
    // §Fase 92.c — the compiled `credential` contracts, so a `mint` resolves
    // its ttl/grants at dispatch. (The minter PORT is injected by the caller
    // that owns one — the enterprise executor; absent => mint fails closed.)
    ctx.credentials = std::sync::Arc::new(
        ir.credentials
            .iter()
            .map(|c| (c.name.clone(), c.clone()))
            .collect(),
    );

    // §Fase 37.b (D1, D4) — The Request Binding Contract. Seed the
    // flow's declared parameters from the parsed request body BEFORE
    // the flow walk, so `${param}` resolves everywhere downstream — a
    // `retrieve` / `mutate` / `purge` `where:` clause, a `step` `ask:`
    // prompt, a `persist` / `mutate` field block.
    //
    // §Fase 37.y (D3) — extended to THREE binding sources: URL path
    // captures + URL query string + parsed JSON body. The compile-time
    // D3 + D4 check (`axon-T901`) guarantees every flow parameter
    // resolves to EXACTLY ONE source — collisions are compile errors
    // — so the runtime merge order (path > query > body) is
    // semantically irrelevant; it's just the lookup precedence.
    //
    // `bind_request` binds ONLY declared parameters (D4): an
    // unrelated path capture / query value / body field is never
    // silently injected. A flow-body `let` that shadows a parameter
    // name overwrites it later in the walk — the author's explicit
    // `let` wins, by construction.
    for (name, value) in crate::request_binding::bind_request(
        &flow,
        &request_path,
        &request_query,
        request_body.as_ref(),
    ) {
        ctx.let_bindings.insert(name, value);
    }

    // §6 — Walk the flow body. For each top-level IRFlowNode, call
    // dispatch_node and honor the outcome semantics:
    //
    //   Completed { .. }      → continue to the next node
    //   Break                 → ignored at top level (loop sentinel
    //                           only propagates inside ForIn bodies)
    //   LoopContinue          → ignored at top level (same)
    //   Return { value }      → short-circuit the flow loop (a `return`
    //                           statement explicitly terminates execution)
    //   Err(UpstreamCancelled)→ exit cleanly (cancel was observed
    //                           inside a handler at its entry guard)
    //   Err(other)            → emit FlowError + exit
    //
    // Sentinels Break / LoopContinue are scoped to orchestration
    // bodies (ForIn / Conditional) — they're handled INTERNALLY by
    // those handlers and don't surface here. If they DO surface at
    // top level it's a structural bug; we treat them as no-ops so
    // the flow continues (defensive — never panic on the production
    // hot path per D7).
    let mut total_tokens_output: u64 = 0;
    let mut steps_executed: usize = 0;
    let mut flow_success = true;
    // §Fase 36.x.c (D1) — `true` once a `FlowError` terminator has
    // been emitted from the dispatch loop. Gates the §7 `FlowComplete`
    // so the producer NEVER emits a second terminator after
    // `FlowError` — the wire carries exactly one (`FlowComplete` XOR
    // `FlowError`). Distinct from `flow_success`: a flow can complete
    // with `success:false` (a clean non-success) WITHOUT erroring, and
    // that case still gets its single `FlowComplete` terminator.
    let mut flow_errored = false;

    // §Fase 33.z.c — Look up the AST flow that corresponds to the
    // IR flow. The AST is needed by `resolve_stream_effect_for_step`
    // to walk the tool-effects table. For canonical Step shapes
    // declaring `<stream:<policy>>` effects on a tool referenced via
    // `apply: tool_name`, this is the same resolution the v1.25.0
    // 33.x.d production path does in `build_streaming_plan`.
    //
    // Resolution happens at the TOP LEVEL only. Nested-step policies
    // (steps inside Conditional / ForIn / Par bodies) inherit `None`
    // — a deeper resolution that walks AST through orchestration is
    // 33.z.d scope (50-flow parity corpus). For the canonical Step
    // shape that the d2_post_33_x_d test exercises, top-level
    // resolution is sufficient.
    let ast_flow = ast_program
        .declarations
        .iter()
        .find_map(|d| match d {
            crate::ast::Declaration::Flow(f) if f.name == flow_name => Some(f),
            _ => None,
        });

    for node in &flow.steps {
        if cancel.is_cancelled() {
            break;
        }
        // §Fase 33.z.c — Per-step effect-policy resolution. Set
        // `ctx.pending_effect_policy` before dispatch; the pure_shape
        // handler reads + clears via `take_pending_effect_policy()` +
        // wraps the chunk stream with `StreamPolicyEnforcer`.
        if let (Some(ast_f), crate::ir_nodes::IRFlowNode::Step(ir_step)) = (ast_flow, node) {
            if !ir_step.name.is_empty() {
                ctx.pending_effect_policy =
                    crate::stream_effect_dispatcher::resolve_stream_effect_for_step(
                        &ir_step.name,
                        ast_f,
                        &ast_program,
                    );
            }
        }
        let outcome = dispatch_node(node, &mut ctx).await;
        match outcome {
            Ok(NodeOutcome::Completed {
                tokens_emitted, ..
            }) => {
                total_tokens_output += tokens_emitted;
                // step_index from the handler reflects the node's
                // position in the FLOW (not the body — orchestration
                // handlers internally manage their own counters).
                steps_executed += 1;
            }
            Ok(NodeOutcome::Break) | Ok(NodeOutcome::LoopContinue) => {
                // Defensive: shouldn't reach top level. Treat as no-op.
            }
            Ok(NodeOutcome::Return { .. }) => {
                // Explicit `return` in flow body — short-circuit.
                steps_executed += 1;
                break;
            }
            Err(crate::flow_dispatcher::DispatchError::UpstreamCancelled) => {
                // Cancel fired during dispatch; exit cleanly without
                // emitting FlowError (cancel is a graceful shutdown,
                // not a flow failure per D3 contract from 33.x.e).
                break;
            }
            Err(e) => {
                flow_success = false;
                // §Fase 36.x.c (D1) — `FlowError` is the terminator
                // for this flow; §7 must NOT also emit `FlowComplete`.
                flow_errored = true;
                // §Fase 37.e (D6) — honest failure: name the FAILING
                // NODE. Step + the four store ops carry a meaningful
                // name; any other variant is named by its flow
                // position. The diagnostic reaches the `axon.error`
                // wire event AND a structured server log.
                use crate::ir_nodes::IRFlowNode;
                let node_label = match node {
                    IRFlowNode::Step(s) if !s.name.is_empty() => {
                        format!("step '{}'", s.name)
                    }
                    IRFlowNode::Retrieve(r) => {
                        format!("retrieve from '{}'", r.store_name)
                    }
                    IRFlowNode::Persist(p) => {
                        format!("persist into '{}'", p.store_name)
                    }
                    IRFlowNode::Mutate(m) => format!("mutate '{}'", m.store_name),
                    IRFlowNode::Purge(p) => format!("purge '{}'", p.store_name),
                    _ => format!("node #{}", steps_executed + 1),
                };
                let detail =
                    format!("flow '{flow_name}' failed at {node_label}: {e:?}");
                tracing::error!(
                    flow = %flow_name,
                    node = %node_label,
                    detail = %detail,
                    "axon streaming flow failed — node dispatch error"
                );
                let _ = emit(FlowExecutionEvent::FlowError {
                    flow_name: flow_name.clone(),
                    error: detail,
                    timestamp_ms: now_ms(),
                });
                break;
            }
        }
    }

    // §7 — FlowComplete with accumulated counters. tokens_input is
    // 0 because the dispatcher doesn't currently track input tokens
    // separately (the per-step audit records do, but the FlowComplete
    // input field stays 0 until 33.z.c extends the surface).
    //
    // §Fase 36.x.c (D1) — GATED on `!flow_errored`. When the dispatch
    // loop already terminated the stream with `FlowError`, emitting
    // `FlowComplete` here would put a SECOND terminator on the wire —
    // the malformed double-terminator the 36.x.a anchor pinned. The
    // wire carries exactly one terminator: `FlowComplete` XOR
    // `FlowError`.
    if !flow_errored {
        let _ = emit(FlowExecutionEvent::FlowComplete {
            flow_name,
            backend,
            success: flow_success,
            steps_executed,
            tokens_input: 0,
            tokens_output: total_tokens_output,
            latency_ms: exec_start.elapsed().as_millis() as u64,
            timestamp_ms: now_ms(),
        });
    }

    // Drop ctx (and its tx clone) so the consumer's `recv().await`
    // returns None when this producer task completes — the original
    // tx in the caller is dropped separately by the spawn site.
    drop(ctx);
}

// ────────────────────────────────────────────────────────────────────
//  Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel_token::CancellationFlag;
    use tokio::sync::mpsc;

    /// Canonical Step + stub backend → 1 axon.token "(stub)" + 1
    /// axon.complete. Wire byte-compat with v1.26.0 `run_streaming_async_path`.
    #[tokio::test]
    async fn canonical_step_emits_single_stub_token() {
        let src = "flow Chat() -> Unit {\n\
                       step Generate { ask: \"hi\" output: Stream<Token> }\n\
                   }\n\
                   axonendpoint E { method: POST path: \"/c\" execute: Chat transport: sse public: true }";
        let (tx, mut rx) = mpsc::unbounded_channel();
        let cancel = CancellationFlag::new();
        let enforcement = std::sync::Arc::new(tokio::sync::Mutex::new(
            std::collections::HashMap::new(),
        ));
        let audit = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let warnings = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        run_streaming_via_dispatcher(
            src.to_string(),
            "test.axon".to_string(),
            "Chat".to_string(),
            "stub".to_string(),
            cancel,
            tx,
            enforcement,
            audit,
            warnings,
            std::sync::Arc::new(std::sync::Mutex::new(
                crate::temporal_context::TemporalState::default(),
            )), // §Fase 91.b — temporal side-channel (test: fresh)
            None,
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None, // §Fase 58.g — tool_base_url
            None, // §Fase 65.C — api_key (tests use the env/stub key)
            None, // §Fase 114 — channel_semaphores (test: ungoverned)
            None, // §Fase 114 — tool_leases (test: ungoverned)
        )
        .await;

        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }

        let token_count = events
            .iter()
            .filter(|e| matches!(e, FlowExecutionEvent::StepToken { .. }))
            .count();
        let complete_count = events
            .iter()
            .filter(|e| matches!(e, FlowExecutionEvent::FlowComplete { .. }))
            .count();
        let flow_start_count = events
            .iter()
            .filter(|e| matches!(e, FlowExecutionEvent::FlowStart { .. }))
            .count();
        assert_eq!(flow_start_count, 1, "exactly 1 FlowStart");
        assert_eq!(token_count, 1, "stub backend emits exactly 1 token");
        assert_eq!(complete_count, 1, "exactly 1 FlowComplete");
    }

    /// §Fase 36.x.c (D1) — a compilation error emits EXACTLY ONE
    /// terminator: `FlowError`, no StepToken events, and NO redundant
    /// post-error `FlowComplete`. A stream that ends twice is a lie
    /// about where it ended.
    #[tokio::test]
    async fn compilation_error_emits_exactly_one_flow_error() {
        let src = "not valid axon source ::: <<< broken";
        let (tx, mut rx) = mpsc::unbounded_channel();
        let cancel = CancellationFlag::new();
        let enforcement = std::sync::Arc::new(tokio::sync::Mutex::new(
            std::collections::HashMap::new(),
        ));
        let audit = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let warnings = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        run_streaming_via_dispatcher(
            src.to_string(),
            "broken.axon".to_string(),
            "Whatever".to_string(),
            "stub".to_string(),
            cancel,
            tx,
            enforcement,
            audit,
            warnings,
            std::sync::Arc::new(std::sync::Mutex::new(
                crate::temporal_context::TemporalState::default(),
            )), // §Fase 91.b — temporal side-channel (test: fresh)
            None,
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None, // §Fase 58.g — tool_base_url
            None, // §Fase 65.C — api_key (tests use the env/stub key)
            None, // §Fase 114 — channel_semaphores (test: ungoverned)
            None, // §Fase 114 — tool_leases (test: ungoverned)
        )
        .await;

        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }

        let error_count = events
            .iter()
            .filter(|e| matches!(e, FlowExecutionEvent::FlowError { .. }))
            .count();
        let token_count = events
            .iter()
            .filter(|e| matches!(e, FlowExecutionEvent::StepToken { .. }))
            .count();
        let complete_count = events
            .iter()
            .filter(|e| matches!(e, FlowExecutionEvent::FlowComplete { .. }))
            .count();
        assert_eq!(error_count, 1, "compilation error emits exactly one FlowError");
        assert_eq!(token_count, 0, "no tokens on compilation failure");
        assert_eq!(
            complete_count, 0,
            "no FlowComplete after a FlowError — exactly one terminator (36.x.c/D1)"
        );
        assert!(
            matches!(events.last(), Some(FlowExecutionEvent::FlowError { .. })),
            "the FlowError is the final event — nothing follows the terminator"
        );
    }

    /// §Fase 36.x.c (D1) — a missing flow name emits a single
    /// structured `FlowError` terminator, no `FlowComplete`.
    #[tokio::test]
    async fn missing_flow_name_emits_flow_error() {
        let src = "flow Chat() -> Unit {\n\
                       step Generate { ask: \"hi\" output: Stream<Token> }\n\
                   }\n\
                   axonendpoint E { method: POST path: \"/c\" execute: Chat transport: sse public: true }";
        let (tx, mut rx) = mpsc::unbounded_channel();
        let cancel = CancellationFlag::new();
        let enforcement = std::sync::Arc::new(tokio::sync::Mutex::new(
            std::collections::HashMap::new(),
        ));
        let audit = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let warnings = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        run_streaming_via_dispatcher(
            src.to_string(),
            "test.axon".to_string(),
            "NonExistent".to_string(),
            "stub".to_string(),
            cancel,
            tx,
            enforcement,
            audit,
            warnings,
            std::sync::Arc::new(std::sync::Mutex::new(
                crate::temporal_context::TemporalState::default(),
            )), // §Fase 91.b — temporal side-channel (test: fresh)
            None,
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None, // §Fase 58.g — tool_base_url
            None, // §Fase 65.C — api_key (tests use the env/stub key)
            None, // §Fase 114 — channel_semaphores (test: ungoverned)
            None, // §Fase 114 — tool_leases (test: ungoverned)
        )
        .await;

        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }

        let error_found = events.iter().any(|e| {
            matches!(e, FlowExecutionEvent::FlowError { error, .. } if error.contains("not found"))
        });
        assert!(error_found, "missing flow surfaces structured 'not found' error");
    }

    /// Pre-cancel → producer exits cleanly without StepToken / FlowComplete.
    #[tokio::test]
    async fn pre_cancel_short_circuits() {
        let src = "flow Chat() -> Unit {\n\
                       step Generate { ask: \"hi\" output: Stream<Token> }\n\
                   }\n\
                   axonendpoint E { method: POST path: \"/c\" execute: Chat transport: sse public: true }";
        let (tx, mut rx) = mpsc::unbounded_channel();
        let cancel = CancellationFlag::new();
        cancel.cancel(); // pre-cancel
        let enforcement = std::sync::Arc::new(tokio::sync::Mutex::new(
            std::collections::HashMap::new(),
        ));
        let audit = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let warnings = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        run_streaming_via_dispatcher(
            src.to_string(),
            "test.axon".to_string(),
            "Chat".to_string(),
            "stub".to_string(),
            cancel,
            tx,
            enforcement,
            audit,
            warnings,
            std::sync::Arc::new(std::sync::Mutex::new(
                crate::temporal_context::TemporalState::default(),
            )), // §Fase 91.b — temporal side-channel (test: fresh)
            None,
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None, // §Fase 58.g — tool_base_url
            None, // §Fase 65.C — api_key (tests use the env/stub key)
            None, // §Fase 114 — channel_semaphores (test: ungoverned)
            None, // §Fase 114 — tool_leases (test: ungoverned)
        )
        .await;

        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }
        // Cancel fired before FlowStart could be emitted → channel
        // close drains zero events. The producer's `emit` helper
        // checks `cancel.is_cancelled()` first → returns Err(()) →
        // producer exits without emitting.
        assert_eq!(events.len(), 0, "pre-cancel → zero events emitted");
    }
}
