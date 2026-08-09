//! §Fase 33.y.b — Per-IRFlowNode async dispatcher skeleton.
//!
//! This module ships the **structural foundation** of the Fase 33.y
//! universal-algebraic-streaming cycle: a closed-catalog, compiler-
//! enforced exhaustive dispatch table over the 45-variant
//! [`crate::ir_nodes::IRFlowNode`] enum. Each arm in
//! [`dispatch_node`]'s match is named explicitly; adding a 46th
//! IRFlowNode variant fails the Rust build at this match until the
//! corresponding dispatcher arm is added (D1 totality invariant).
//!
//! # What 33.y.b ships
//!
//! - [`DispatchCtx`] — the shared per-flow context every per-variant
//!   handler reads + writes. Carries the FlowExecutionEvent producer,
//!   the cancellation flag, the side-channels for enforcement summary
//!   / step audit / runtime warnings, the orchestration `branch_path`
//!   (for D6 per-step replay binding under Par/ForIn/Conditional
//!   nesting), and the per-step counter.
//! - [`NodeOutcome`] — closed catalog of dispatcher outcomes. In
//!   33.y.b only the transitional [`NodeOutcome::LegacyShimHandled`]
//!   variant exists; subsequent sub-fases 33.y.c–j add real outcomes
//!   (`Completed`, `Break`, `LoopContinue`, `Return`, etc.) and
//!   33.y.l removes `LegacyShimHandled` once every variant has its
//!   real handler.
//! - [`DispatchError`] — closed catalog of dispatch errors. Five
//!   variants today: BackendError / UpstreamCancelled /
//!   LegacyShimFailed / MissingDependency / ChannelClosed.
//! - [`ShimReason`] — per-IRFlowNode-variant tag for the 33.y.b
//!   transitional shim. The drift gate
//!   [`tests::shim_reason_covers_full_ir_flow_node_catalog`]
//!   asserts the ShimReason set has 1-to-1 cardinality with the
//!   IRFlowNode set.
//! - [`dispatch_node`] — the dispatcher entry point. Exhaustive
//!   match over 45 arms; each arm delegates to [`legacy_shim`] which
//!   returns `Ok(NodeOutcome::LegacyShimHandled)`. **No node is
//!   actually executed in 33.y.b.** The module is standalone — not
//!   wired into `server_execute_streaming` — so production behavior
//!   is byte-identical with v1.25.0 (D4).
//!
//! # What 33.y.b does NOT ship
//!
//! - Real per-variant async logic (lands per sub-fase 33.y.c–j).
//! - Integration with `server_execute_streaming` (lands incrementally
//!   per sub-fase as each variant comes online).
//! - Wire-format extensions (per-step `wire_status`, `branch_path`
//!   field on `StepAuditRecord`, `axon-W003 partial-streaming-
//!   activation` warning — all land in 33.y.c–l).
//!
//! # D-letter anchors
//!
//! - **D1** — Per-IRFlowNode async dispatch is total. The exhaustive
//!   match below is the compiler-enforced totality witness.
//! - **D4** — Wire byte-compat preserved. No production code path
//!   calls `dispatch_node` in 33.y.b; the module exists to lock the
//!   shape that subsequent sub-fases extend.
//! - **D7** — Production-grade per-variant handler discipline. The
//!   shim is INTENTIONALLY a no-op transition — not an
//!   `unimplemented!()` panic, not a `todo!()`, not a `_ =>` catch-all.
//!   Each arm is named; each shim invocation tags its IR variant
//!   precisely. The shim is structural plumbing, not a stub.

use crate::cancel_token::CancellationFlag;
use crate::flow_execution_event::FlowExecutionEvent;
use crate::ir_nodes::IRFlowNode;
use crate::stream_effect::BackpressurePolicy;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// §Fase 33.y.c — Pure-shape variant handlers (Step / Probe / Reason /
/// Validate / Refine / Weave). All 6 variants reduce to "produce a
/// single LLM response from a prompt + cognitive framing"; the module
/// houses the shared async core (`run_pure_shape`) + 6 thin
/// per-variant entry points that build the variant's framing.
/// §Fase 114.a — the budget gate, in ONE place, on EVERY tool path.
///
/// Until §114 it was inlined in `pure_shape` and reachable only by a streaming
/// tool, inside a daemon, on the enterprise supervisor. The canonical
/// `use Tool(…)` path — the one `advertised.rs` cites as PROOF that `tool` is
/// Real — had no budget at all.
pub mod budget_gate;
/// §Fase 119.m.1 — the `agent` executor: `strategy:` as a CONTROL policy.
///
/// `agent` was advertised, type-checked against two closed catalogs, lowered
/// to `IRAgent` with eleven fields — and executed by nothing. This module is
/// the loop and its termination proof, composed from machinery that already
/// runs (`run_pure_shape`, `run_use_tool`, `run_forge`, the §72 budget gate,
/// the Theorem 5.1 ceiling) rather than reimplemented beside it.
pub mod agent_loop;
pub mod pure_shape;

/// §Fase 33.y.d — Orchestration variant handlers (Let / Conditional /
/// ForIn / Break / Continue / Return). 6 variants — control-flow
/// constructs that compose child handlers via recursive
/// `dispatch_node` calls + sentinel-driven loop semantics +
/// `branch_path` segments threading the orchestration tree.
pub mod orchestration;

/// §Fase 33.y.e — Parallel variant handler (`Par`) + public helper
/// [`parallel::run_branches_concurrently`] for concurrent dispatch
/// via `futures::future::join_all` with per-branch DispatchCtx
/// clones + post-join step_counter merge + Return-sentinel
/// propagation. `IRParallelBlock` is payload-free in v1.25.0; the
/// handler emits the `step_type: "par"` wire shape with zero
/// token events. Future IR extensions wire branches into the
/// public helper.
pub mod parallel;

/// §Fase 33.y.e — Stream variant handler (`Stream`) + public bridge
/// [`effects_bridge::bridge_effect_stream_yield`] integrating the
/// Fase 23 algebraic-effects runtime: scans the instruction block
/// for `perform Stream.Yield x` (statically + via trace), runs the
/// `EffectRuntime`, and emits one `axon.token` per Yield with the
/// resolved value. `IRStreamBlock` is payload-free in v1.25.0; the
/// handler emits the `step_type: "stream"` wire shape with zero
/// token events. Future IR extensions wire instruction blocks into
/// the public bridge.
pub mod effects_bridge;

/// §Fase 33.y.f — Cognitive primitives (Fase 11 neuro-symbolic).
/// 10 variants: `Remember` / `Recall` are PEM-bound (write-through
/// + read-back via the optional [`DispatchCtx::pem_backend`]);
/// `Forge` is payload-free wire shape (canonical
/// `step_type: "forge"`); `Focus` / `Associate` / `Aggregate` /
/// `Explore` / `Ingest` / `Navigate` / `Corroborate` reuse the
/// pure-shape async core ([`pure_shape::run_pure_shape`]) with
/// each variant's cognitive framing addendum reflected in the
/// system prompt.
pub mod cognitive;

/// §Fase 33.y.g — Algebraic-effect handler nodes.
/// 6 variants: `ShieldApply` / `OtsApply` / `MandateApply` — apply
/// a named capability to a target with structured output binding;
/// `ComputeApply` — invoke a compute capability with positional
/// arguments; `Listen` — wait on a Fase 13 typed channel for an
/// event; `DaemonStep` — invoke a Fase 16 daemon supervisor by
/// reference. Each handler emits wire shape with the canonical
/// `step_type` slug + public `apply_*` helpers that enterprise
/// integrations override (per the OSS/ENTERPRISE/SPLIT charter
/// — the shield/OTS/mandate scanner registries live in
/// `axon_enterprise.shield`).
pub mod algebraic_handlers;

/// §Fase 33.y.h — Wire-integration handler nodes (π-calc +
/// persistence + multi-agent deliberation). 10 variants:
/// **Emit / Publish / Discover** (Fase 13 typed channels — π-calc
/// output prefix + capability extrusion + dual discovery);
/// **Persist / Retrieve / Mutate / Purge / Transact** (persistence
/// primitives — snapshot / load / update / delete / transactional
/// block); **Deliberate / Consensus** (multi-agent payload-free
/// blocks). Each ships wire shape + public helper that enterprise
/// integrations (Postgres / Redis / MQ / typed-channel runtime)
/// override.
pub mod wire_integrations;

/// §Fase 33.y.i — PIX variants (paper §6 hidden-state primitives).
/// 3 variants: **Hibernate** (CPS-style event-await with timeout
/// — Fase 11.e + Fase 16 supervisor); **Drill** (PIX subtree
/// navigation); **Trail** (breadcrumb walk over a prior
/// navigation). OSS reference impl uses `__pix_*` /
/// `__hibernating_*` namespaced let_bindings keys; enterprise R&D
/// (axon_enterprise.cognitive_states + .supervisor) wires real
/// continuation-passing semantics + PIX state machines.
pub mod pix;

/// §Fase 33.y.j — Lambda + UseTool (the final 2 variants).
/// **LambdaDataApply** — Fase 15 ΛD apply (the sync runner walks
/// a CPS dispatcher mapping lambda data structures to expressions;
/// 33.y.j ships the OSS wire shape + helper). **UseTool** —
/// mid-step tool invocation (Fase 22 backend tools; the
/// `ChatRequest.tools` cross-cutting plumb-through lands in
/// 33.y.k D8). Completes the 45-variant total coverage.
pub mod lambda_tools;

/// §Fase 34.g — Unified stream handler (4-disjunction convergence).
/// Pre-34.g the four streaming-effect disjunctions (LLM-side
/// `output: Stream<T>`, `apply: <stream-tool>`, `use_tool` syntax,
/// `perform Stream.Yield`) had divergent drain paths — disjunct (a)
/// enforced `BackpressurePolicy` at chunk granularity while (b)/(d)
/// only captured the policy slug in audit without enforcement. This
/// module ships [`unified_stream::unified_stream_handler`] — the
/// single drain loop that ALL `Stream<ToolChunk>`-producing
/// disjunctions route through; the handler integrates a
/// [`crate::stream_runtime::Stream<ToolChunk>`] policy primitive +
/// returns a [`unified_stream::ToolStreamSummary`] with real
/// `chunks_dropped`/`chunks_degraded` counters. Also ships the
/// [`unified_stream::chat_chunk_to_tool_chunk`] type-bridge for
/// disjunct (a) symmetry tests + [`unified_stream::unified_stream_from_chunks`]
/// adapter for disjunct (d)'s static-scan output.
pub mod unified_stream;

// ────────────────────────────────────────────────────────────────────
//  DispatchCtx — shared per-flow async surface
// ────────────────────────────────────────────────────────────────────

/// Per-flow dispatcher context. Carries the producer-side wire
/// surface (`tx` for FlowExecutionEvent), cancel-in-body propagation
/// (`cancel`), the audit/enforcement/warning side-channels (read by
/// the SSE handler at `axon.complete` time), and the orchestration
/// `branch_path` for D6 per-step replay binding.
///
/// # `branch_path` semantics
///
/// Empty at flow root. Parent handlers push a segment when descending
/// into a child:
/// - `par[0]`, `par[1]`, `par[2]` for the n-th branch of a Par block.
/// - `for_in[0]`, `for_in[1]` for the n-th iteration of a ForIn loop.
/// - `conditional.then`, `conditional.else` for the chosen branch
///   of an `if`.
/// - Children inside a branch concat: `par[0].step[0]` for the first
///   child step of the first Par branch.
///
/// The path is observable in `StepAuditRecord.branch_path` (extended
/// in 33.y.f when the audit row writer gains the field) so regulators
/// replaying a flow on appeal see the full execution tree, not just a
/// flattened step sequence.
///
/// # `step_counter`
///
/// Monotonic per-flow counter. Each Step (or pure-shape variant
/// promoted to Step) increments. Surface fed into `step_audit` so
/// the row index is correct under nested orchestration.
/// §Fase 67.c — per-run store row counts, observable on
/// `ServerRunnerMetrics`. Surfaces "how much work did this run touch"
/// (closing brief #34 Q3: a daemon run reporting `completed, duration 0`
/// stops being indistinguishable from "found no rows"). Each store op
/// handler folds its `n` into the shared (par-branch-merged) counter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoreRowCounts {
    pub retrieved: u64,
    pub persisted: u64,
    pub mutated: u64,
    pub purged: u64,
}

/// §Fase 67.c — which counter a store op increments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreRowKind {
    Retrieved,
    Persisted,
    Mutated,
    Purged,
}

/// §Fase 111.d — the active `quant` block's resolved parameters, held for the
/// duration of its body so a nested `yield` knows what Hilbert space it is
/// measuring in and with which observable.
///
/// This exists because `quant` and `yield` are two halves of one operation:
/// `quant { … }` declares the space (encoding + observable + register width) and
/// `yield <carrier>` performs the collapse `E = ⟨ψ|M|ψ⟩`. Before §111.d neither
/// half did anything, so the split cost nothing; now that both are real, the
/// frame is what connects them.
#[derive(Clone, Debug)]
pub struct QuantFrame {
    /// How the classical carrier is projected into the state (`amplitude` |
    /// `angle`).
    pub encoding: crate::quant::EncodingScheme,
    /// The resolved Pauli-sum `M`. Hermitian by construction.
    pub observable: crate::quant::PauliSum,
    /// The observable's declared name — carried so a measurement result can say
    /// what it measured, not just what it computed.
    pub observable_name: String,
}

#[derive(Clone)]
pub struct DispatchCtx {
    pub flow_name: String,
    pub backend_name: String,
    /// §Fase 65.C — the per-tenant API key resolved from the tenant secrets
    /// manager (the same key the non-streaming runner receives as
    /// `api_key_override`). When `Some`, the LLM handlers resolve the backend
    /// via [`crate::backends::resolve_streaming_backend_with_key`] so the call
    /// uses THIS tenant's key, not the process env var. `None` (the
    /// `DispatchCtx::new` default) ⇒ env-key behavior, unchanged.
    pub api_key: Option<String>,
    /// §Fase 24.g.2 (Kivi brief #37) — optional per-tenant LLM endpoint
    /// override (base URL + chat path) threaded from the enterprise
    /// `llm.<backend>.base_url` / `.chat_path` secret. When `Some`, the LLM
    /// handlers resolve the backend via
    /// [`crate::backends::resolve_streaming_backend_with_key_and_endpoint`] so
    /// the call hits THIS tenant's endpoint (e.g. z.ai's `/api/paas/v4`)
    /// instead of the provider default. `None` ⇒ env/default, unchanged.
    pub llm_base_url: Option<String>,
    /// §Fase 24.g.2 — companion to `llm_base_url`: the chat-completions path
    /// (e.g. `/api/paas/v4/chat/completions` for z.ai, vs the `/v1/...` default).
    pub llm_chat_path: Option<String>,
    /// §Fase 65.C.2 — the flow's conversation history, accumulated across LLM
    /// steps so each step sees the prior turns. Before this the dispatcher's
    /// LLM path was STATELESS single-shot — every streaming/SSE step lost the
    /// prior steps' Q&A, unlike the non-streaming runner (which threads its
    /// `ConversationHistory` through `execute_step_with_retry`). `Arc<Mutex>`
    /// so it persists across nodes and is shared by par-branches — one
    /// conversation per flow, matching the runner's per-unit history.
    pub conversation: std::sync::Arc<std::sync::Mutex<crate::conversation::ConversationHistory>>,
    /// §Fase 65.C.2 — char budget for `conversation`; the oldest turn pairs are
    /// dropped before each LLM call when exceeded (the runner's `ContextWindow`
    /// discipline). Default = `ContextWindow::new().max_chars`; 0 = unlimited.
    pub context_budget: usize,
    /// §Fase 65.C.3 — the flow's resolved anchors, checked against each LLM
    /// step's output. Before this the dispatcher's (streaming/SSE) path NEVER
    /// enforced anchors — declared `require:` constraints were silently ignored
    /// on SSE. Now a breach is surfaced in the step audit record. The
    /// regenerate-on-breach RETRY stays on the non-streaming runner until §65.D
    /// (retry-while-streaming is fraught — tokens already on the wire). Empty
    /// (the `DispatchCtx::new` default) ⇒ no anchor checking, unchanged.
    pub anchors: std::sync::Arc<Vec<crate::ir_nodes::IRAnchor>>,
    pub system_prompt: String,
    /// §Fase 91.b — the frame-level declared cognitive timezone: the program's
    /// first `context` declaration's `now:` (the same first-context convention
    /// `flow_plan::compose_system_prompt_public` uses for the frame itself).
    /// A step's own `now:` overrides it. `None` (the `new` default) ⇒ only
    /// step-level declarations inject.
    pub default_now_tz: Option<String>,
    /// §Fase 91.b — shared per-run temporal state: ONE lazily-captured instant
    /// + the zones actually rendered. `Arc<Mutex>` so `par` branches share the
    /// capture and the collector reads the final state after the walk (the
    /// §67.c side-channel discipline; the lock is never held across an
    /// `.await`).
    pub temporal: std::sync::Arc<std::sync::Mutex<crate::temporal_context::TemporalState>>,
    /// §Fase 92.c — the ephemeral-credential minter port behind the `mint`
    /// flow verb. `None` (the `new` default) ⇒ a reached `mint` fails CLOSED
    /// with `MissingDependency` — never a silent stub. The enterprise
    /// executor injects its PASETO minter (§92.g); tests use
    /// `credential_minter::InMemoryMinter`.
    pub credential_minter: Option<std::sync::Arc<dyn crate::credential_minter::CredentialMinter>>,
    /// §Fase 92.c — the compiled `credential` contracts by name (from
    /// `ir.credentials`), set by the plan builders like `mdn_corpora`.
    /// Empty for a credential-less program.
    pub credentials:
        std::sync::Arc<std::collections::HashMap<String, crate::ir_nodes::IRCredential>>,
    /// §Fase 94.d — the secret-custody port behind the `backend: secrets`
    /// metadata store, the `rotate` verb, and the `tool { secret: }`
    /// dispatch injection (`rotation_without_revelation`). `None` (the
    /// `new` default) ⇒ every one of those surfaces fails CLOSED with
    /// `MissingDependency` — never a silent stub, never an LLM
    /// fallthrough. The enterprise executor injects its envelope-encrypted
    /// Postgres custody (§94.h); tests use `secret_custody::InMemoryCustody`.
    pub secret_custody: Option<std::sync::Arc<dyn crate::secret_custody::SecretCustody>>,
    /// §Fase 108.b — the deterministic columnar engine port behind the five
    /// data-plane verbs (`ingest` §108.c; `focus`/`associate`/`aggregate`/
    /// `explore` §108.d). `None` (the `new` default) ⇒ each verb fails
    /// CLOSED with `MissingDependency { name: "dataspace_engine" }` — never
    /// an LLM narration (the §108.a honesty floor). The deploy hook builds
    /// it from the compiled `ir.dataspace_specs`.
    pub dataspace_engine: Option<crate::dataspace_engine::SharedDataspaceEngine>,
    /// §Fase 111.c — the adversarial-analysis port behind the `warden` block.
    ///
    /// Before §111 there was NO port here, which is why `warden` could not have
    /// worked even in principle: `run_warden` recorded the scope name in a
    /// let-binding nobody read, returned an empty string, and **silently
    /// discarded the block's body** — while `StepComplete` went out on the wire
    /// saying it had finished. The engine existed the whole time
    /// (`crate::warden::{WardenBackend, ReferenceStaticWarden, Vulnerability,
    /// verify}`) and no code path could reach it; enterprise's `saas-warden`
    /// was mounted only on the `POST /api/v1/warden/{scope}` HTTP route. The
    /// math was a library nobody wired to the keyword (§111 F12).
    ///
    /// `None` (the `new` default) ⇒ the block fails CLOSED with
    /// `MissingDependency { name: "warden_backend" }`. An unanalysed target must
    /// never return "no findings" — an empty result and a refusal look identical
    /// to a reader, and only one of them is honest.
    pub warden_backend: Option<std::sync::Arc<dyn crate::warden::WardenBackend + Send + Sync>>,
    /// §Fase 111.c — the compiled `scope` declarations, so `warden(<t>) within
    /// <Scope>` can resolve its authorization envelope at dispatch time. Empty
    /// (the `new` default) ⇒ every `warden` fails CLOSED: a scope that cannot be
    /// resolved authorises nothing (the §88 fail-closed posture — the `within`
    /// clause is mandatory precisely so an analysis can never run unscoped).
    pub scopes: Arc<Vec<crate::ir_nodes::IRScope>>,
    /// §Fase 111.d — the Hilbert-space port behind the `quant` block and its
    /// `yield` measurement point.
    ///
    /// Same shape as `warden` (§111 F12): `run_quant` was a **pure no-op** — it
    /// inserted `__quant_backend` (a key nothing read), returned an empty
    /// output, and **silently skipped every step inside `quant { … }`**, even
    /// though `ir_generator` had faithfully lowered them. `run_yield` stored the
    /// carrier expression verbatim into `__quant_yield` and collapsed no
    /// amplitudes. Meanwhile `crate::quant` (`ReferenceSimulator`, `PauliSum`,
    /// `StateVector`) was real, non-trivial, fuzz-tested code that **no dispatch
    /// path could reach** — `DispatchCtx` had no port, so enterprise's
    /// `saas-quant` `Q32Simulator` was reachable only from `POST
    /// /api/v1/quant/{name}`.
    ///
    /// `None` (the `new` default) ⇒ `quant` fails CLOSED with
    /// `MissingDependency { name: "quant_backend" }`.
    pub quant_backend: Option<std::sync::Arc<dyn crate::quant::QuantBackend + Send + Sync>>,
    /// §Fase 111.d — the compiled `observable` declarations (the Pauli-sum
    /// catalog). A `quant` block measures `E = ⟨ψ|M|ψ⟩`; with no resolvable `M`
    /// there is nothing to measure, so an unresolvable observable fails CLOSED
    /// rather than returning a number that means nothing.
    pub observables: Arc<Vec<crate::ir_nodes::IRObservable>>,
    /// §Fase 111.d — the active `quant` frame, pushed by `run_quant` for the
    /// duration of its body so a nested `yield` knows which Hilbert space it is
    /// collapsing into. `None` outside a `quant` block ⇒ a bare `yield` fails
    /// CLOSED (a measurement with no state to measure is not a weak result, it
    /// is a category error).
    pub quant_frame: Option<QuantFrame>,
    /// §Fase 111.f — the compiled `compute` declarations: named PURE FUNCTIONS
    /// over the §70 expression language.
    ///
    /// Before §111 this catalog did not need to exist, because `IRCompute`
    /// carried only `name` + `shield_ref` — no parameters, no body. The handler
    /// therefore bound the literal string `"compute:Name(args)"`, and a
    /// downstream step consumed that TEXT where it expected a number, while the
    /// README promised "native Fast-Path execution bypassing the LLM" with an
    /// O(n) guarantee (§111 F10).
    ///
    /// Empty ⇒ `compute … on …` fails CLOSED (axon-T941 catches it at compile
    /// time; this is the runtime's belt).
    pub compute_specs: Arc<Vec<crate::ir_nodes::IRCompute>>,
    /// §Fase 119.b — the compiled `mandate` declarations, so a `mandate X on Y`
    /// (flow-level node or step-scoped guard) can build its enforcement loop at
    /// dispatch. Empty ⇒ every mandate application fails CLOSED — a mandate
    /// that cannot be resolved enforces nothing, so nothing is released
    /// (the §111.f compute doctrine, applied to the cage).
    pub mandate_specs: Arc<Vec<crate::ir_nodes::IRMandate>>,
    /// §Fase 119.c — the compiled `lambda` (ΛD) declarations, so a
    /// `lambda X on y` can construct ψ = ⟨T, V, E⟩ at dispatch instead of
    /// binding the placeholder string "lambda:<name>(<target>)". Empty ⇒
    /// fails CLOSED.
    pub lambda_data_specs: Arc<Vec<crate::ir_nodes::IRLambdaData>>,
    /// §Fase 119.c — the compiled `ots` declarations, resolved against the
    /// name-keyed `ots_registry`. Empty ⇒ fails CLOSED.
    pub ots_specs: Arc<Vec<crate::ir_nodes::IROts>>,
    pub cancel: CancellationFlag,
    pub tx: mpsc::UnboundedSender<FlowExecutionEvent>,
    pub enforcement_summaries: Arc<
        Mutex<HashMap<String, crate::execution_result::EnforcementSummaryWire>>,
    >,
    pub step_audit_records: Arc<
        Mutex<Vec<crate::axonendpoint_replay::StepAuditRecord>>,
    >,
    pub runtime_warnings: Arc<
        Mutex<Vec<crate::runtime_warnings::RuntimeWarning>>,
    >,
    /// §Fase 67.c — shared (par-branch-merged) per-run store row counts.
    /// A plain `std::sync::Mutex` (the update is instant — lock, add,
    /// drop — never held across an `.await`). Like the audit side-
    /// channels, the collector injects its own Arc via
    /// [`DispatchCtx::with_external_side_channels`] and reads it after
    /// the walk; par-branches share it (the Arc survives `ctx.clone()`,
    /// unlike `pinned_conns` which is replaced per branch).
    pub store_row_counts: std::sync::Arc<std::sync::Mutex<StoreRowCounts>>,
    pub branch_path: Vec<String>,
    pub step_counter: usize,
    /// §Fase 33.y.f — Optional PEM async surface for cognitive
    /// primitives (Remember / Recall etc.). When `Some(backend)`,
    /// `run_remember` write-through persists to PEM and `run_recall`
    /// restores from PEM as a write-back cache layered over
    /// `let_bindings`. When `None`, both handlers degrade to
    /// `let_bindings`-only (in-memory) — the canonical adopter
    /// path for tests + adopters that don't opt into persistent
    /// cognitive state. Arc-cloned per branch for concurrent
    /// dispatch (Fase 33.y.e parity).
    pub pem_backend: Option<std::sync::Arc<dyn crate::pem::PersistenceBackend>>,
    /// §Fase 33.y.f — Session anchor for PEM persistence. Defaults
    /// to `flow_name` in [`DispatchCtx::new`]; adopters override
    /// for multi-session flows.
    pub session_id: String,
    /// §Fase 33.y.f — Tenant routing tag for PEM persistence.
    /// Defaults to empty in [`DispatchCtx::new`]; multi-tenant
    /// adopters set this before dispatch.
    pub tenant_id: String,
    /// §Fase 33.y.d — Let-binding scope. Map from binding name to its
    /// resolved value. `run_let` inserts; `run_conditional` reads to
    /// evaluate the condition; `run_for_in` inserts the iteration
    /// variable per iter. Bindings persist through the flow's
    /// lifetime — sub-scoping is NOT introduced in 33.y.d (the
    /// sync runner's let semantics are flow-scoped + monotonic,
    /// matching this discipline for D10 parity). The `HashMap` is
    /// cheap to clone for branch isolation when sub-fases 33.y.e
    /// introduce parallel branches with private scopes (Par block).
    pub let_bindings: std::collections::HashMap<String, String>,
    /// §Fase 33.y.c — Per-node declared `<stream:<policy>>` resolved
    /// by the caller BEFORE invoking `dispatch_node`. The pure-shape
    /// handlers read + consume this field (set back to `None` on
    /// entry) so each handler observes the policy intended for ITS
    /// node, never the previous node's residue. When `None`, the
    /// handler skips `StreamPolicyEnforcer` wrapping + emits chunks
    /// directly to the wire.
    ///
    /// Subsequent sub-fases 33.y.d-l adopt the same pattern for
    /// orchestration handlers (`Par` / `ForIn`) when child nodes
    /// declare effects.
    pub pending_effect_policy: Option<BackpressurePolicy>,
    /// §Fase 34.d (v1.29.0) — Tool registry surface for the
    /// streaming-tool dispatcher branch. When `Some(registry)`,
    /// `pure_shape::run_step` resolves `step.apply_ref` against
    /// the registry; if the entry's `is_streaming` flag is true,
    /// the step bypasses `Backend::stream()` entirely + invokes
    /// `tool.stream(args, ctx)` via the
    /// [`crate::tool_dispatch_bridge::resolve_streaming_tool`]
    /// factory. When `None` (D9 backwards-compat), the legacy
    /// LLM-side path is taken regardless of source-declared
    /// `effects: <stream:<policy>>` — adopters who haven't wired
    /// the registry yet see no behavior change. Arc-shared for
    /// concurrent dispatch (Fase 33.y.e parity).
    pub tool_registry: Option<std::sync::Arc<crate::tool_registry::ToolRegistry>>,
    /// §Fase 63.B — the MDN corpus graphs built from `corpus { relations: … }`
    /// declarations (those that carry typed edges). When a `navigate <ref>`
    /// names a key here, the handler runs real MDN graph navigation
    /// (`mdn::navigate_corpus` / signed EPR) instead of PIX tree navigation.
    /// `Arc`-shared so branch clones (Par) are cheap.
    pub mdn_corpora: Option<std::sync::Arc<std::collections::HashMap<String, crate::mdn::Corpus>>>,
    /// §Fase 63.C — the names of corpora declared `adaptive: true` (the memory
    /// endofunctor is enabled for navigations over them).
    pub mdn_adaptive: std::sync::Arc<std::collections::HashSet<String>>,
    /// §Fase 63.C — per-corpus interaction history (mutable, accumulates across a
    /// flow's navigations). A navigation over an adaptive corpus applies the
    /// memory endofunctor (`mdn_memory::apply_memory`) using this history, then
    /// records its own trajectory. `Arc<Mutex<…>>` so branch clones share it.
    pub mdn_histories:
        std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, crate::mdn_memory::History>>>,
    /// §Fase 64.B — the DYNAMIC, store-sourced MDN corpora (`corpus N from
    /// axonstore { … }`): a map from corpus name to its store-mapping spec. A
    /// navigation over one of these does NOT use the pre-built `mdn_corpora`;
    /// instead the runtime reads the mapped stores tenant-scoped and builds a
    /// fresh `mdn::Corpus` from the LIVE rows (the graph grows as the stores
    /// grow). `Arc`-shared so branch clones (Par) are cheap.
    pub mdn_store_sources:
        std::sync::Arc<std::collections::HashMap<String, crate::ir_nodes::IRCorpusStoreSource>>,
    /// §Fase 35.f (v1.30.0) — axonstore registry for SQL-vs-KV
    /// dispatch. When `Some(registry)`, `run_persist` / `run_retrieve`
    /// / `run_mutate` / `run_purge` resolve `store_name` against it: a
    /// `postgresql`-backed store routes through `PostgresStoreBackend`,
    /// every other (and every undeclared) store takes the byte-
    /// identical key-value path. When `None` (the `DispatchCtx::new`
    /// default), every store op is key-value — the pre-35 behavior,
    /// unchanged (D3). Arc-shared so concurrent branches share one
    /// per-DSN pool cache.
    pub store_registry: Option<std::sync::Arc<crate::store::registry::StoreRegistry>>,
    /// §Fase 35.j (v1.30.0) — Pillar IV: the capability slugs the
    /// current request carries (the JWT bearer's `capabilities`
    /// claim). When `Some`, the store handlers re-check a
    /// capability-gated store against this set before any access —
    /// defense-in-depth behind the type-checker's compile-time
    /// guarantee. When `None` (the `DispatchCtx::new` default), there
    /// is no capability context at this layer and the runtime
    /// re-check is a no-op: the compile-time check + the endpoint's
    /// Fase 32.g `requires:` gate stand.
    pub held_capabilities: Option<Vec<String>>,
    /// §Fase 35.h (v1.30.0) — Pillar II: the flow's tamper-evident
    /// HMAC-Merkle mutation chain. Every `persist`/`mutate`/`purge`
    /// appends a delta. Shared (`Arc`) across concurrent branches so a
    /// `Par` block's mutations land in one chain; the Merkle head is a
    /// verifiable fingerprint of the flow's complete mutation history.
    pub audit_chain:
        std::sync::Arc<std::sync::Mutex<crate::store::audit_chain::StoreAuditChain>>,
    /// §Fase 37.x.j (D2) — Per-flow pinned Postgres connections.
    /// Populated at stream start by `run_streaming_via_dispatcher`:
    /// the IR is walked, every postgresql-backed `axonstore` referenced
    /// by the flow body has ONE `PoolConnection<Postgres>` acquired,
    /// and the map holds them by axonstore name for the flow's
    /// lifetime. The map drops at the end of the streaming task,
    /// returning every conn to the pool via the `after_release
    /// DEALLOCATE ALL` hook (Fase 38.x.a D2 composing with 37.x.j D1).
    ///
    /// Wire-integration store handlers consult this map per op:
    /// `take` the pin out → run the SQL via `StoreConn::Pinned(&mut pin)`
    /// → `insert` the pin back. The take/return discipline preserves
    /// the Arc<Mutex<>> sharing pattern across cloned (par-branched)
    /// contexts while keeping individual ops borrow-checker friendly.
    ///
    /// Empty map ≡ no pinning held (legacy path) → handlers fall back
    /// to `StoreConn::Pool(backend.pool())`. This is the case for
    /// callers that haven't eager-acquired (non-streaming RPC paths,
    /// CLI tests, etc.) — D5 byte-identical backwards-compat.
    ///
    /// Per D6.b (sub-fase 37.x.j.6): `par {}` branches that share this
    /// Arc serialize on its mutex. The D6.a default (per-branch
    /// sub-pin) replaces this Arc with a fresh empty map at par-branch
    /// clone time so branches do NOT serialize on the parent's pins.
    pub pinned_conns: std::sync::Arc<
        std::sync::Mutex<
            std::collections::HashMap<
                String,
                crate::pinned_conn::PinnedConn,
            >,
        >,
    >,
    /// §Fase 72.c — the active `budget { … }` linear-effect gate, when the flow
    /// is being run by a budgeted `daemon`. Before a tool effect is emitted, the
    /// dispatcher consults this gate; an exhausted quota under `on_exhausted:
    /// block` fails the step (`EffectQuotaExhausted`). `None` for a flow run with
    /// no budget (every non-daemon path, and budgetless daemons) — the tool
    /// dispatch is then unconditional, byte-identical to pre-§72. Shared
    /// (`Arc<Mutex>`) so the cumulative bucket state spans the daemon's flows +
    /// (cloned) par branches within a tick.
    pub budget: Option<
        std::sync::Arc<std::sync::Mutex<crate::runtime::budget_kernel::BudgetGate>>,
    >,
    /// §Fase 114.e — the per-channel concurrency semaphores (`resource.capacity`),
    /// held across requests so `capacity: 8` bounds simultaneous in-flight calls.
    /// `None` (the `DispatchCtx::new` default) ⇒ no channel is bounded, byte-
    /// identical to pre-§114. Attached on the server path via
    /// [`DispatchCtx::with_channel_semaphores`].
    pub channel_semaphores: Option<std::sync::Arc<crate::channel_semaphore::ChannelSemaphores>>,
    /// §Fase 114.f — the leases over tool-held resources, held across requests. A
    /// post-expiry vendor call is a CT-2 Anchor Breach. `None` ⇒ no lease governs a
    /// tool's channel in this program.
    pub tool_leases: Option<std::sync::Arc<crate::resource_lease::ResourceLeaseGuard>>,
    /// §Fase 74.a — the shared typed-channel event bus a flow's `emit`
    /// routes to (the producer side of durable event delivery). `None`
    /// (the `DispatchCtx::new` default — HTTP / CLI / test paths) ⇒ `emit`
    /// falls back to the legacy per-flow in-memory buffer, byte-identical
    /// to pre-§74. When `Some` (the daemon-supervisor path attaches it via
    /// [`DispatchCtx::with_event_bus`]), `emit Channel(payload)` delivers
    /// to the bus → a `daemon`'s `listen Channel` receives it. The bus is
    /// shared (`Arc`) so producer flows + consumer listeners in one runtime
    /// reach the same transport.
    pub event_bus: Option<std::sync::Arc<crate::runtime::channels::TypedEventBus>>,
    /// §Fase 74.c — the durable event outbox a flow's `emit` to a
    /// `persistent_axonstore` channel appends to (so the event survives the
    /// consumer being down — and, on the §74.f Postgres outbox, a crash).
    /// `None` (the `DispatchCtx::new` default) ⇒ no durable outbox; `emit`
    /// uses the ephemeral bus / legacy buffer (pre-§74.c). When `Some` (the
    /// supervisor path attaches it via [`DispatchCtx::with_event_outbox`]),
    /// a `persistent_axonstore` channel's `emit` is appended to the outbox.
    pub event_outbox: Option<std::sync::Arc<dyn crate::event_outbox::EventOutbox>>,
    /// §Fase 74.e — optional replay/audit-chain sink. When attached, a
    /// flow's `emit` records an `emit:<channel>` `ReplayToken` (the
    /// producer's Chan-Output reduction in the §11.c chain). `None` (the
    /// `DispatchCtx::new` default) ⇒ no recording (pre-§74.e).
    pub replay_log: Option<std::sync::Arc<dyn crate::replay_token::ReplayLog>>,
}

impl DispatchCtx {
    /// Construct a fresh context for a new flow. Subsequent sub-fases
    /// extend this with builder methods as the surface grows (PEM /
    /// ReplayToken / CognitiveState plumbing in 33.y.f, tool registry
    /// in 33.y.k, etc.).
    pub fn new(
        flow_name: impl Into<String>,
        backend_name: impl Into<String>,
        system_prompt: impl Into<String>,
        cancel: CancellationFlag,
        tx: mpsc::UnboundedSender<FlowExecutionEvent>,
    ) -> Self {
        let flow_name = flow_name.into();
        let session_id = flow_name.clone();
        Self {
            flow_name,
            backend_name: backend_name.into(),
            api_key: None,
            llm_base_url: None,
            llm_chat_path: None,
            conversation: std::sync::Arc::new(std::sync::Mutex::new(
                crate::conversation::ConversationHistory::new(),
            )),
            context_budget: crate::conversation::ContextWindow::new().max_chars,
            anchors: std::sync::Arc::new(Vec::new()),
            system_prompt: system_prompt.into(),
            default_now_tz: None,
            temporal: std::sync::Arc::new(std::sync::Mutex::new(
                crate::temporal_context::TemporalState::default(),
            )),
            credential_minter: None,
            credentials: std::sync::Arc::new(std::collections::HashMap::new()),
            // §Fase 94.d — no custody by default: rotate / secrets-retrieve /
            // secret-bearing tool dispatch all fail CLOSED until one is attached.
            secret_custody: None,
            // §Fase 108.b — no engine by default: the five data-plane verbs
            // fail CLOSED until the deploy hook attaches one.
            dataspace_engine: None,
            // §Fase 111.c — no warden backend and no scope catalog by default:
            // a `warden` block fails CLOSED until BOTH are attached. Analysis
            // requires an engine to run it AND an authorization envelope to run
            // it within; neither is optional.
            warden_backend: None,
            scopes: Arc::new(Vec::new()),
            // §Fase 111.d — no simulator and no observable catalog by default:
            // a `quant` block fails CLOSED until both are attached. Measuring
            // requires a Hilbert space to measure IN and an observable to
            // measure WITH; neither is optional.
            quant_backend: None,
            observables: Arc::new(Vec::new()),
            quant_frame: None,
            // §Fase 111.f — no compute catalog by default: an apply fails CLOSED.
            compute_specs: Arc::new(Vec::new()),
            // §Fase 119.b — no mandate catalog by default: an apply fails CLOSED.
            mandate_specs: Arc::new(Vec::new()),
            // §Fase 119.c — no ΛD / ots catalogs by default: fail CLOSED.
            lambda_data_specs: Arc::new(Vec::new()),
            ots_specs: Arc::new(Vec::new()),
            cancel,
            tx,
            enforcement_summaries: Arc::new(Mutex::new(HashMap::new())),
            step_audit_records: Arc::new(Mutex::new(Vec::new())),
            runtime_warnings: Arc::new(Mutex::new(Vec::new())),
            store_row_counts: std::sync::Arc::new(std::sync::Mutex::new(
                StoreRowCounts::default(),
            )),
            branch_path: Vec::new(),
            step_counter: 0,
            pem_backend: None,
            session_id,
            tenant_id: String::new(),
            let_bindings: std::collections::HashMap::new(),
            pending_effect_policy: None,
            tool_registry: None,
            mdn_corpora: None,
            mdn_adaptive: std::sync::Arc::new(std::collections::HashSet::new()),
            mdn_histories: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            mdn_store_sources: std::sync::Arc::new(std::collections::HashMap::new()),
            store_registry: None,
            held_capabilities: None,
            audit_chain: std::sync::Arc::new(std::sync::Mutex::new(
                crate::store::audit_chain::StoreAuditChain::new(),
            )),
            // §Fase 37.x.j (D2) — empty pin map by default; populated
            // by `run_streaming_via_dispatcher` via `with_pinned_conns`.
            pinned_conns: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            // §Fase 72.c — no budget by default (unbudgeted dispatch). The daemon
            // path attaches one via `with_budget`.
            budget: None,
            channel_semaphores: None,
            tool_leases: None,
            // §Fase 74.a — no event bus by default; `emit` uses the legacy
            // per-flow buffer. The daemon supervisor attaches the shared bus
            // via `with_event_bus` so `emit` delivers to `listen`ers.
            event_bus: None,
            // §Fase 74.c — no durable outbox by default.
            event_outbox: None,
            // §Fase 74.e — no replay sink by default.
            replay_log: None,
        }
    }

    /// §Fase 94.d — Builder: attach the secret-custody port so the
    /// `backend: secrets` metadata store, the `rotate` verb, and
    /// `tool { secret: }` dispatch injection have a live custody behind
    /// them. Without this, all three fail CLOSED (`MissingDependency`).
    pub fn with_secret_custody(
        mut self,
        custody: std::sync::Arc<dyn crate::secret_custody::SecretCustody>,
    ) -> Self {
        self.secret_custody = Some(custody);
        self
    }

    /// §Fase 108.b — Builder: attach the deterministic columnar engine so
    /// the five data-plane verbs have live declared stores behind them.
    /// Without this, all five fail CLOSED (`MissingDependency`, the
    /// §108.a honesty floor). Built at deploy from `ir.dataspace_specs`
    /// via [`crate::dataspace_engine::DataspaceEngine::from_ir`].
    pub fn with_dataspace_engine(
        mut self,
        engine: crate::dataspace_engine::SharedDataspaceEngine,
    ) -> Self {
        self.dataspace_engine = Some(engine);
        self
    }

    /// §Fase 111.c — Builder: attach the adversarial-analysis engine AND the
    /// compiled scope catalog, so a `warden` block can actually run.
    ///
    /// Both, together, on purpose. An engine without a scope catalog would be an
    /// analysis with no authorization envelope to run within — and the `within
    /// <Scope>` clause is mandatory in the grammar (§88) *precisely* so that
    /// cannot happen. Taking them as one argument pair makes the unscoped
    /// configuration unrepresentable, rather than merely discouraged.
    ///
    /// OSS mounts [`crate::warden::ReferenceStaticWarden`] (deterministic static
    /// checks; `depth: static_artifact` only). Enterprise mounts its abduction
    /// engine behind the same trait. Without this, `warden` fails CLOSED.
    pub fn with_warden(
        mut self,
        backend: std::sync::Arc<dyn crate::warden::WardenBackend + Send + Sync>,
        scopes: Arc<Vec<crate::ir_nodes::IRScope>>,
    ) -> Self {
        self.warden_backend = Some(backend);
        self.scopes = scopes;
        self
    }

    /// §Fase 111.d — Builder: attach the Hilbert-space simulator AND the
    /// compiled observable catalog, so a `quant` block can actually measure.
    ///
    /// Both together, for the same reason as [`Self::with_warden`]: a simulator
    /// with no observable catalog is a state you cannot ask a question of.
    ///
    /// OSS mounts [`crate::quant::ReferenceSimulator`] (dense statevector, capped
    /// at `OSS_QUBIT_CAP` qubits — a register above the cap fails closed with
    /// `axon-E0783`, never a silent truncation). Enterprise mounts its
    /// `Q32Simulator` behind the same trait.
    /// §Fase 111.f — Builder: attach the compiled `compute` declarations, so
    /// `compute <Name> on …` evaluates its declared expression natively instead
    /// of binding a placeholder string.
    pub fn with_computes(mut self, specs: Arc<Vec<crate::ir_nodes::IRCompute>>) -> Self {
        self.compute_specs = specs;
        self
    }

    /// §Fase 119.b — Builder: attach the compiled `mandate` declarations, so
    /// `mandate <Name> on …` builds its enforcement loop at dispatch instead
    /// of discarding the name on the first line.
    pub fn with_mandates(mut self, specs: Arc<Vec<crate::ir_nodes::IRMandate>>) -> Self {
        self.mandate_specs = specs;
        self
    }

    /// §Fase 119.c — Builder: attach the compiled ΛD declarations, so
    /// `lambda <Name> on …` constructs ψ instead of a placeholder string.
    pub fn with_lambdas(mut self, specs: Arc<Vec<crate::ir_nodes::IRLambdaData>>) -> Self {
        self.lambda_data_specs = specs;
        self
    }

    /// §Fase 119.c — Builder: attach the compiled `ots` declarations.
    pub fn with_ots(mut self, specs: Arc<Vec<crate::ir_nodes::IROts>>) -> Self {
        self.ots_specs = specs;
        self
    }

    pub fn with_quant(
        mut self,
        backend: std::sync::Arc<dyn crate::quant::QuantBackend + Send + Sync>,
        observables: Arc<Vec<crate::ir_nodes::IRObservable>>,
    ) -> Self {
        self.quant_backend = Some(backend);
        self.observables = observables;
        self
    }

    /// §Fase 74.a — Builder: attach the shared typed-channel event bus so a
    /// flow's `emit Channel(payload)` delivers to it (the producer side of
    /// durable event delivery). Without this, `emit` buffers locally
    /// (pre-§74 behaviour).
    pub fn with_event_bus(
        mut self,
        bus: std::sync::Arc<crate::runtime::channels::TypedEventBus>,
    ) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// §Fase 74.c — Builder: attach the durable event outbox so a flow's
    /// `emit` to a `persistent_axonstore` channel is APPENDED to it
    /// (durable intent), instead of the ephemeral bus. Pairs with
    /// `with_event_bus` (the bus is the channel registry that resolves a
    /// channel's `persistence`).
    pub fn with_event_outbox(
        mut self,
        outbox: std::sync::Arc<dyn crate::event_outbox::EventOutbox>,
    ) -> Self {
        self.event_outbox = Some(outbox);
        self
    }

    /// §Fase 74.e — Builder: attach the replay/audit-chain sink so a flow's
    /// `emit` records an `emit:<channel>` ReplayToken (and the daemon
    /// receive-loop records `deliver:<channel>` tokens).
    pub fn with_replay_log(
        mut self,
        log: std::sync::Arc<dyn crate::replay_token::ReplayLog>,
    ) -> Self {
        self.replay_log = Some(log);
        self
    }

    /// §Fase 72.c — Builder: attach the active `budget { … }` gate (the daemon
    /// path). Shared so cumulative bucket state spans the daemon's flows + par
    /// branches within a tick.
    pub fn with_budget(
        mut self,
        budget: std::sync::Arc<std::sync::Mutex<crate::runtime::budget_kernel::BudgetGate>>,
    ) -> Self {
        self.budget = Some(budget);
        self
    }

    /// §Fase 114.e — attach the cross-request channel semaphores.
    pub fn with_channel_semaphores(
        mut self,
        sems: std::sync::Arc<crate::channel_semaphore::ChannelSemaphores>,
    ) -> Self {
        self.channel_semaphores = Some(sems);
        self
    }

    /// §Fase 114.f — attach the cross-request tool-lease guard.
    pub fn with_tool_leases(
        mut self,
        leases: std::sync::Arc<crate::resource_lease::ResourceLeaseGuard>,
    ) -> Self {
        self.tool_leases = Some(leases);
        self
    }

    /// §Fase 37.x.j (D2) — Builder: attach an Arc-shared pinned
    /// connection map populated by the caller. `run_streaming_via_dispatcher`
    /// uses this to install the eagerly-acquired flow-scoped pins
    /// BEFORE the dispatcher walks any node. Returns `self` so the
    /// builder pattern chains with `with_store_registry`, `with_pem`,
    /// `with_tool_registry`, `with_held_capabilities`.
    pub fn with_pinned_conns(
        mut self,
        conns: std::sync::Arc<
            std::sync::Mutex<
                std::collections::HashMap<
                    String,
                    crate::pinned_conn::PinnedConn,
                >,
            >,
        >,
    ) -> Self {
        self.pinned_conns = conns;
        self
    }

    /// §Fase 65.C — Builder: pin the per-tenant API key the dispatcher's LLM
    /// handlers use to resolve the backend (instead of the process env var).
    /// Returns `self` so builders chain. `None` leaves env-key behavior.
    pub fn with_api_key(mut self, api_key: Option<String>) -> Self {
        self.api_key = api_key;
        self
    }

    /// §Fase 24.g.2 (Kivi brief #37) — Builder: pin a per-tenant LLM endpoint
    /// override (base URL + chat path) the dispatcher threads into the backend
    /// factory. Either may be `None` (then env/default applies for that part).
    pub fn with_llm_endpoint(
        mut self,
        base_url: Option<String>,
        chat_path: Option<String>,
    ) -> Self {
        self.llm_base_url = base_url;
        self.llm_chat_path = chat_path;
        self
    }

    /// §Fase 65.C.2 — Builder: set the conversation char budget (0 = unlimited).
    /// Returns `self` so builders chain.
    pub fn with_context_budget(mut self, max_chars: usize) -> Self {
        self.context_budget = max_chars;
        self
    }

    /// §Fase 65.C.3 — Builder: install the flow's resolved anchors so each LLM
    /// step's output is checked against them (breaches surfaced in the step
    /// audit). Returns `self` so builders chain.
    pub fn with_anchors(mut self, anchors: std::sync::Arc<Vec<crate::ir_nodes::IRAnchor>>) -> Self {
        self.anchors = anchors;
        self
    }

    /// §Fase 35.f — Builder: attach the `axonstore` registry so the
    /// wire-integration store handlers route postgresql-backed stores
    /// to SQL. Without it, every store op stays key-value (D3).
    /// Returns `self` so builders chain.
    pub fn with_store_registry(
        mut self,
        registry: std::sync::Arc<crate::store::registry::StoreRegistry>,
    ) -> Self {
        self.store_registry = Some(registry);
        self
    }

    /// §Fase 35.j — Builder: attach the request's held capability
    /// slugs so the store handlers re-check capability-gated stores
    /// (Pillar IV). Returns `self` so builders chain.
    pub fn with_held_capabilities(mut self, capabilities: Vec<String>) -> Self {
        self.held_capabilities = Some(capabilities);
        self
    }

    /// §Fase 34.d — Builder: attach a tool registry so the
    /// dispatcher's streaming-tool branch can resolve `apply_ref`
    /// against it. Returns `self` so builders chain.
    pub fn with_tool_registry(
        mut self,
        registry: std::sync::Arc<crate::tool_registry::ToolRegistry>,
    ) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    /// §Fase 63.B — Builder: attach the MDN corpus graphs (built from the IR's
    /// `corpus { relations: … }` declarations) so `navigate <corpus>` runs real
    /// graph navigation. Returns `self` so builders chain.
    pub fn with_mdn_corpora(
        mut self,
        corpora: std::sync::Arc<std::collections::HashMap<String, crate::mdn::Corpus>>,
    ) -> Self {
        self.mdn_corpora = Some(corpora);
        self
    }

    /// §Fase 63.C — Builder: mark which corpora are `adaptive` (memory-enabled).
    pub fn with_mdn_adaptive(
        mut self,
        adaptive: std::sync::Arc<std::collections::HashSet<String>>,
    ) -> Self {
        self.mdn_adaptive = adaptive;
        self
    }

    /// §Fase 64.B — Builder: register the DYNAMIC, store-sourced MDN corpora
    /// (`corpus N from axonstore { … }`) by name → store-mapping spec.
    pub fn with_mdn_store_sources(
        mut self,
        sources: std::sync::Arc<
            std::collections::HashMap<String, crate::ir_nodes::IRCorpusStoreSource>,
        >,
    ) -> Self {
        self.mdn_store_sources = sources;
        self
    }

    /// Builder: attach a PEM persistence backend. Returns `self` so
    /// callers can chain `DispatchCtx::new(...).with_pem(backend)`.
    pub fn with_pem(
        mut self,
        backend: std::sync::Arc<dyn crate::pem::PersistenceBackend>,
    ) -> Self {
        self.pem_backend = Some(backend);
        self
    }

    /// Builder: set the session id (defaults to flow_name).
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = session_id.into();
        self
    }

    /// Builder: set the tenant id (defaults to empty).
    pub fn with_tenant_id(mut self, tenant_id: impl Into<String>) -> Self {
        self.tenant_id = tenant_id.into();
        self
    }

    /// Builder-style setter for the pending effect policy. Returns
    /// `self` so callers can chain `ctx.with_effect_policy(policy)`
    /// before invoking `dispatch_node`. Handlers read + clear the
    /// field via [`Self::take_pending_effect_policy`].
    pub fn with_effect_policy(mut self, policy: BackpressurePolicy) -> Self {
        self.pending_effect_policy = Some(policy);
        self
    }

    /// §Fase 33.z.c — Builder: inject external Arc-backed side-channels
    /// so the dispatcher's per-variant handlers populate the SAME
    /// Mutexes that `server_execute_streaming` reads from for the SSE
    /// wire's `enforcement_summary`, `step_audit`, and `runtime_warnings`
    /// fields.
    ///
    /// Without this builder, `DispatchCtx::new` creates FRESH Arcs that
    /// the dispatcher populates but the production hot path can't read.
    /// That gap broke `axon.complete.enforcement_summary` wire emission
    /// on the canonical Step shape when the dispatcher graft (33.z.b)
    /// activated — the 33.x.d production-path tests detected the
    /// regression at the `assert_eq!(generate_summary["chunks_pushed"], 1)`
    /// line because the side-channel the wire reads from stayed empty
    /// while the dispatcher's fresh Arc carried the counters.
    ///
    /// Used exclusively by `streaming_via_dispatcher::run_streaming_via_dispatcher`
    /// to thread the side-channels the SSE handler constructs into the
    /// dispatcher. Downstream-crate consumers driving `dispatch_node`
    /// directly continue to use `DispatchCtx::new` + the fresh internal
    /// Arcs.
    pub fn with_external_side_channels(
        mut self,
        enforcement_summaries: std::sync::Arc<
            tokio::sync::Mutex<
                std::collections::HashMap<String, crate::execution_result::EnforcementSummaryWire>,
            >,
        >,
        step_audit_records: std::sync::Arc<
            tokio::sync::Mutex<Vec<crate::axonendpoint_replay::StepAuditRecord>>,
        >,
        runtime_warnings: std::sync::Arc<
            tokio::sync::Mutex<Vec<crate::runtime_warnings::RuntimeWarning>>,
        >,
        // §Fase 67.c — the collector injects its own row-count Arc so it
        // can read the totals after the dispatcher walk completes.
        store_row_counts: std::sync::Arc<std::sync::Mutex<StoreRowCounts>>,
    ) -> Self {
        self.enforcement_summaries = enforcement_summaries;
        self.step_audit_records = step_audit_records;
        self.runtime_warnings = runtime_warnings;
        self.store_row_counts = store_row_counts;
        self
    }

    /// §Fase 67.c — fold a store op's row count into the shared per-run
    /// totals. The guard is dropped at the end of the statement (never
    /// held across an `.await`), so the `std::sync::Mutex` is safe inside
    /// the async store handlers.
    pub fn record_store_rows(&self, kind: StoreRowKind, n: u64) {
        let mut c = self.store_row_counts.lock().unwrap();
        match kind {
            StoreRowKind::Retrieved => c.retrieved += n,
            StoreRowKind::Persisted => c.persisted += n,
            StoreRowKind::Mutated => c.mutated += n,
            StoreRowKind::Purged => c.purged += n,
        }
    }

    /// Read + clear the pending effect policy. Returns `None` when no
    /// policy was set by the caller. The take-semantics (vs. peek)
    /// prevents a stale policy from a previous node leaking into the
    /// next handler's invocation if the caller forgets to clear.
    pub fn take_pending_effect_policy(&mut self) -> Option<BackpressurePolicy> {
        self.pending_effect_policy.take()
    }

    /// Render the current `branch_path` as a wire-stable string. Empty
    /// path returns `""` (flow root); single segment `"par[0]"`; multi
    /// `"par[0].step[1]"`. The format is byte-stable across calls.
    pub fn branch_path_string(&self) -> String {
        self.branch_path.join(".")
    }
}

// ────────────────────────────────────────────────────────────────────
//  NodeOutcome — closed catalog of dispatcher outcomes
// ────────────────────────────────────────────────────────────────────

/// Closed catalog of dispatcher outcomes. 33.y.b ships only the
/// transitional [`LegacyShimHandled`] variant; subsequent sub-fases
/// 33.y.c–j add real outcomes:
///
/// - `Completed { output, tokens_emitted }` — handler ran to
///   completion; output captured + tokens forwarded on the wire.
/// - `Break` — sentinel from an in-loop `break`. The For-In handler
///   short-circuits remaining iterations + propagates up.
/// - `LoopContinue` — sentinel from `continue`. Skips to next
///   iteration.
/// - `Return { value }` — sentinel from `return`. Flow loop
///   terminates.
///
/// 33.y.l removes [`LegacyShimHandled`] once every variant has its
/// real handler.
///
/// # Why a closed catalog vs `Result<String, _>`
///
/// Sentinel values (Break / LoopContinue / Return) need to flow up
/// through nested handler stacks WITHOUT being mistaken for content.
/// A `Result<String, _>` would force the caller to encode sentinels
/// in-band, which is unsound under serde + adopter-observable output.
/// The closed enum is the only sound algebraic representation.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum NodeOutcome {
    /// §Fase 33.y.c+ — Handler ran to completion. `output` is the
    /// concatenated chunk content captured during streaming;
    /// `tokens_emitted` is the count of non-empty `StepToken` events
    /// fanned to the wire (post-policy enforcement for steps with a
    /// declared `<stream:<policy>>`). The `step_index` is the value
    /// of `ctx.step_counter` at the moment the handler started (i.e.
    /// the index the handler reserved for itself before
    /// incrementing); orchestration handlers in 33.y.d–e use this
    /// to surface per-branch index trails on `branch_path`.
    Completed {
        output: String,
        tokens_emitted: u64,
        step_index: usize,
    },
    /// §Fase 33.y.d sentinel — emitted by the `Break` handler. The
    /// enclosing `ForIn` handler observes this outcome from its
    /// child dispatch + terminates the loop (skips remaining
    /// iterations). Parser scope check guarantees `Break` only
    /// appears inside a `ForIn` body, so non-loop ancestors that
    /// observe this outcome MUST propagate it upward unchanged.
    Break,
    /// §Fase 33.y.d sentinel — emitted by the `Continue` handler.
    /// The enclosing `ForIn` handler observes this + skips to the
    /// next iteration. Same propagation discipline as
    /// [`NodeOutcome::Break`].
    LoopContinue,
    /// §Fase 33.y.d sentinel — emitted by the `Return` handler.
    /// Terminates the flow loop with the carried `value` as the
    /// final flow output. Parents propagate unchanged until the
    /// flow-loop level observes it.
    Return { value: String },
    /// §Fase 119.d — the hibernate handler observed its suspension point.
    /// The WALK LOOP (which alone knows the node's position in the body)
    /// parks the continuation and HALTS the run. Nested constructs that see
    /// this outcome refuse — suspending inside a branch has no defined
    /// continuation shape yet, and guessing is not honesty.
    Hibernated {
        event_name: String,
        timeout: String,
        step_index: usize,
    },
}

// ────────────────────────────────────────────────────────────────────
//  DispatchError — closed catalog of dispatcher errors
// ────────────────────────────────────────────────────────────────────

/// Closed catalog of dispatcher errors. Adopter-reachable error
/// surfaces are NAMED (D7 mandate: zero `unwrap()` / zero
/// `unimplemented!()` / zero `_ =>` catch-all). Each variant carries
/// adopter-actionable structured data.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum DispatchError {
    /// `Backend::stream()` failed on a per-variant handler that
    /// invoked it. Carries the backend name + the upstream error
    /// message so the SSE handler can surface a structured
    /// `axon.error` event.
    BackendError { name: String, message: String },

    /// Cancellation flag fired mid-dispatch (client disconnected or
    /// upstream `tokio::select!` raced the cancel branch). Caller
    /// MUST treat this as a clean exit (no `axon.error` event
    /// surfaced — the consumer is already gone).
    UpstreamCancelled,

    /// A per-variant handler needed a dependency that wasn't
    /// available on the DispatchCtx (e.g., PEM async surface for a
    /// `Remember`/`Recall` handler before 33.y.f wires it in). The
    /// `name` field tags which dependency.
    MissingDependency { name: &'static str },

    /// The mpsc sender returned `Err(_)` — consumer dropped. Caller
    /// MUST treat this as a clean exit (same posture as
    /// `UpstreamCancelled`).
    ChannelClosed,

    /// §Fase 72.c — a budgeted effect (`budget { … on Tool(X) }`) was blocked
    /// because its rate/max quota is exhausted and the daemon's `on_exhausted`
    /// policy is `block` (fail-closed). `effect` is the tool name; `retry_at_ms`
    /// is when a token next frees up (operator diagnostics — the call did NOT
    /// emit). The typed `axon-E08xx EffectQuotaExhausted` surface.
    EffectQuotaExhausted { effect: String, retry_at_ms: i64 },

    /// §Fase 72.d — a budgeted effect was DEFERRED (`on_exhausted: defer`): the
    /// quota is exhausted, so the daemon's tick should re-run when a token frees
    /// up at `retry_at_ms`. Distinct from `EffectQuotaExhausted` so the supervisor
    /// can DISTINGUISH "reschedule me" from "I failed": the enterprise supervisor
    /// records a coalesced deferred tick (the §71.d defer-ledger) targeting
    /// `retry_at_ms`; the OSS single-process driver degrades to a logged retry on
    /// the next cron tick (the §71.c degradation). The current flow does NOT
    /// complete this tick.
    EffectDeferred { effect: String, retry_at_ms: i64 },
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BackendError { name, message } => {
                write!(f, "backend '{name}' stream() failed: {message}")
            }
            Self::UpstreamCancelled => write!(f, "upstream cancelled mid-dispatch"),
            Self::MissingDependency { name } => {
                write!(f, "dispatcher missing dependency: {name}")
            }
            Self::ChannelClosed => write!(f, "channel closed (consumer dropped)"),
            Self::EffectQuotaExhausted { effect, retry_at_ms } => write!(
                f,
                "axon-E0810 EffectQuotaExhausted: budget for Tool({effect}) is exhausted \
                 (on_exhausted: block); next token at {retry_at_ms}ms — the call was not emitted"
            ),
            Self::EffectDeferred { effect, retry_at_ms } => write!(
                f,
                "axon-E0811 EffectDeferred: budget for Tool({effect}) is exhausted \
                 (on_exhausted: defer); the tick reschedules to {retry_at_ms}ms — \
                 the call was not emitted this run"
            ),
        }
    }
}

impl std::error::Error for DispatchError {}

// ────────────────────────────────────────────────────────────────────
//  §Fase 33.y.l — ShimReason enum + legacy_shim function retired
// ────────────────────────────────────────────────────────────────────
//
// After 33.y.j reached 45/45 IRFlowNode graduation, the
// transitional `ShimReason` enum + `legacy_shim` function + the
// `NodeOutcome::LegacyShimHandled` variant became structurally
// unreachable from `dispatch_node`'s exhaustive match. 33.y.l
// retires the entire shim infrastructure in this lockstep cleanup:
//
//   - ShimReason enum                   — DELETED
//   - ShimReason::ALL constant          — DELETED
//   - ShimReason::slug() method         — DELETED
//   - legacy_shim() async helper        — DELETED
//   - NodeOutcome::LegacyShimHandled    — DELETED variant
//
// Drift-gate slug catalog now uses `flow_plan::ir_flow_node_kind`
// directly (the same byte-stable surface that was duplicated in
// ShimReason::slug — single source of truth).
//
// The dispatcher's 45-arm exhaustive match is unchanged: every IR
// variant routes to its real async handler module (pure_shape /
// orchestration / parallel / effects_bridge / cognitive /
// algebraic_handlers / wire_integrations / pix / lambda_tools).
//
// Search the codebase: `grep -E "unimplemented|todo!|legacy_shim"
// axon-rs/src/flow_dispatcher/*.rs` returns ZERO matches post-33.y.l
// (verified by the `fase33y_l_parity_gate.rs::d7_no_legacy_markers`
// drift-gate test).
//
// Build-time guarantee: `legacy_shim` is gone → compiler enforces
// that NO future arm in dispatch_node can fall back to a stub. The
// catalog totality contract D1 is sealed.
// ────────────────────────────────────────────────────────────────────
//  dispatch_node — the exhaustive entry point
// ────────────────────────────────────────────────────────────────────

/// Dispatch a single IRFlowNode through the per-variant async
/// handler stack. Total over the 45-variant closed catalog
/// (compiler-enforced exhaustive match).
///
/// # 45/45 graduation FINAL (33.y.j)
///
/// As of Fase 33.y.j, every IRFlowNode variant has a NAMED async
/// handler. There are NO `_ =>` catch-all arms, NO `legacy_shim`
/// calls, NO `unimplemented!()` markers. Adding a 46th IRFlowNode
/// variant fails the Rust build here until a real per-variant
/// async handler is wired in.
///
/// Each arm's handler module:
///
/// - **pure_shape** (33.y.c) — Step / Probe / Reason / Validate /
///   Refine / Weave
/// - **orchestration** (33.y.d) — Let / Conditional / ForIn /
///   Break / Continue / Return
/// - **parallel** (33.y.e) — Par
/// - **effects_bridge** (33.y.e + D9) — Stream
/// - **cognitive** (33.y.f) — Remember / Recall / Forge / Focus /
///   Associate / Aggregate / Explore / Ingest / Navigate /
///   Corroborate
/// - **algebraic_handlers** (33.y.g) — ShieldApply / OtsApply /
///   MandateApply / ComputeApply / Listen / DaemonStep
/// - **wire_integrations** (33.y.h) — Emit / Publish / Discover /
///   Persist / Retrieve / Mutate / Purge / Transact / Deliberate /
///   Consensus
/// - **pix** (33.y.i) — Hibernate / Drill / Trail
/// - **lambda_tools** (33.y.j) — LambdaDataApply / UseTool
///
/// # Cancellation
///
/// Every per-variant handler checks `ctx.cancel.is_cancelled()`
/// at entry and at every `.await` boundary per the Fase 33.x.e
/// `cancel_aware` discipline. Cancel propagation is uniform
/// across the entire 45-variant catalog.
pub async fn dispatch_node(
    node: &IRFlowNode,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    // Exhaustive match — compiler enforces every variant has a
    // named arm. Adding a 46th IRFlowNode variant fails the build
    // here until the new arm is added. ZERO `_ =>` catch-all.
    match node {
        // §Fase 33.y.c — pure-shape variants graduated to real
        // async handlers. Each delegates to its labeled
        // `pure_shape::run_*` entry which wraps the shared
        // `pure_shape::run_pure_shape` async core. The shim is
        // retired for these 6 variants; subsequent sub-fases retire
        // it for the remaining 39 variants per the topological
        // schedule in `docs/fase/fase_33y_algebraic_streaming_dispatcher.md`.
        IRFlowNode::Step(step) => pure_shape::run_step(step, ctx).await,
        IRFlowNode::Probe(probe) => pure_shape::run_probe(probe, ctx).await,
        IRFlowNode::Reason(reason) => pure_shape::run_reason(reason, ctx).await,
        IRFlowNode::Validate(validate) => pure_shape::run_validate(validate, ctx).await,
        IRFlowNode::Refine(refine) => pure_shape::run_refine(refine, ctx).await,
        IRFlowNode::Weave(weave) => pure_shape::run_weave(weave, ctx).await,
        // §Fase 33.y.j — UseTool graduated.
        IRFlowNode::UseTool(node) => lambda_tools::run_use_tool(node, ctx).await,
        // §Fase 33.y.f — cognitive primitives PEM-bound.
        IRFlowNode::Remember(node) => cognitive::run_remember(node, ctx).await,
        IRFlowNode::Recall(node) => cognitive::run_recall(node, ctx).await,
        // §Fase 33.y.d — orchestration variants graduated to real
        // async handlers. Each composes child handlers via recursive
        // `dispatch_node` calls + threads sentinel outcomes (Break /
        // LoopContinue / Return) up through orchestration parents.
        IRFlowNode::Conditional(cond) => orchestration::run_conditional(cond, ctx).await,
        IRFlowNode::ForIn(for_in) => orchestration::run_for_in(for_in, ctx).await,
        IRFlowNode::Let(let_bind) => orchestration::run_let(let_bind, ctx).await,
        IRFlowNode::Return(ret) => orchestration::run_return(ret, ctx).await,
        IRFlowNode::Break(brk) => orchestration::run_break(brk, ctx).await,
        IRFlowNode::Continue(cont) => orchestration::run_continue(cont, ctx).await,
        // §Fase 33.y.j — LambdaDataApply graduated.
        IRFlowNode::LambdaDataApply(node) => lambda_tools::run_lambda_data_apply(node, ctx).await,
        // §Fase 33.y.e — Par graduated to real async handler. The
        // payload-free `IRParallelBlock` emits the canonical
        // `step_type: "par"` wire shape; future IR extensions
        // delegate to `parallel::run_branches_concurrently`.
        IRFlowNode::Par(par) => parallel::run_par(par, ctx).await,
        // §Fase 33.y.i — PIX variants graduated.
        IRFlowNode::Hibernate(node) => pix::run_hibernate(node, ctx).await,
        // §Fase 33.y.h — multi-agent deliberation blocks.
        IRFlowNode::Deliberate(node) => wire_integrations::run_deliberate(node, ctx).await,
        IRFlowNode::Consensus(node) => wire_integrations::run_consensus(node, ctx).await,
        // §Fase 33.y.f — Forge payload-free wire shape.
        IRFlowNode::Forge(node) => cognitive::run_forge(node, ctx).await,
        // §Fase 33.y.f — cognitive framing handlers reuse pure_shape.
        // §Fase 109.b — the proof-carrying derivative: evaluate the
        // compile-time-derived expressions at the current bindings.
        IRFlowNode::Grad(node) => orchestration::run_grad(node, ctx).await,
        IRFlowNode::Focus(node) => cognitive::run_focus(node, ctx).await,
        IRFlowNode::Associate(node) => cognitive::run_associate(node, ctx).await,
        IRFlowNode::Aggregate(node) => cognitive::run_aggregate(node, ctx).await,
        IRFlowNode::Explore(node) => cognitive::run_explore(node, ctx).await,
        IRFlowNode::Ingest(node) => cognitive::run_ingest(node, ctx).await,
        // §Fase 33.y.g — algebraic-effect handler nodes graduated.
        IRFlowNode::ShieldApply(node) => algebraic_handlers::run_shield_apply(node, ctx).await,
        // §Fase 33.y.e — Stream graduated to real async handler.
        // The payload-free `IRStreamBlock` emits the canonical
        // `step_type: "stream"` wire shape; future IR extensions
        // delegate to `effects_bridge::bridge_effect_stream_yield`.
        IRFlowNode::Stream(stream) => effects_bridge::run_stream(stream, ctx).await,
        IRFlowNode::Navigate(node) => cognitive::run_navigate(node, ctx).await,
        IRFlowNode::Drill(node) => pix::run_drill(node, ctx).await,
        IRFlowNode::Trail(node) => pix::run_trail(node, ctx).await,
        IRFlowNode::Corroborate(node) => cognitive::run_corroborate(node, ctx).await,
        IRFlowNode::OtsApply(node) => algebraic_handlers::run_ots_apply(node, ctx).await,
        IRFlowNode::MandateApply(node) => algebraic_handlers::run_mandate_apply(node, ctx).await,
        IRFlowNode::ComputeApply(node) => algebraic_handlers::run_compute_apply(node, ctx).await,
        IRFlowNode::Listen(node) => algebraic_handlers::run_listen(node, ctx).await,
        IRFlowNode::DaemonStep(node) => algebraic_handlers::run_daemon_step(node, ctx).await,
        // §Fase 92.c — ephemeral-credential minting (attenuated, fail-closed).
        IRFlowNode::Mint(node) => wire_integrations::run_mint(node, ctx).await,
        // §Fase 94.b — mediated secret renewal (fail-closed without the
        // custody port; an LLM fallthrough would HALLUCINATE a rotation).
        IRFlowNode::Rotate(node) => wire_integrations::run_rotate(node, ctx).await,
        // §Fase 33.y.h — π-calc typed channels (Fase 13).
        IRFlowNode::Emit(node) => wire_integrations::run_emit(node, ctx).await,
        IRFlowNode::Publish(node) => wire_integrations::run_publish(node, ctx).await,
        IRFlowNode::Discover(node) => wire_integrations::run_discover(node, ctx).await,
        // §Fase 33.y.h — persistence primitives.
        IRFlowNode::Persist(node) => wire_integrations::run_persist(node, ctx).await,
        IRFlowNode::Retrieve(node) => wire_integrations::run_retrieve(node, ctx).await,
        IRFlowNode::Mutate(node) => wire_integrations::run_mutate(node, ctx).await,
        IRFlowNode::Purge(node) => wire_integrations::run_purge(node, ctx).await,
        IRFlowNode::Transact(node) => wire_integrations::run_transact(node, ctx).await,
        // §Fase 51.a — the `quant` cognitive block. SURFACE only in this
        // sub-fase: the OSS dispatcher recognizes it and emits the canonical
        // `step_type: "quant"` wire shape but does NOT execute the Hilbert-space
        // body — real evaluation requires the `QuantBackend` port + reference
        // simulator (§51.e) and the effect injection + `yield` measurement
        // (§51.d), and is hardware-accelerated only in the enterprise backend.
        IRFlowNode::Quant(node) => wire_integrations::run_quant(node, ctx).await,
        // §Fase 88.a — the `warden` adversarial-analysis block. SURFACE only:
        // emits the canonical `step_type: "warden"` wire shape but does NOT run
        // the analysis — the real engine (`WardenBackend` port + reference, §88.d;
        // enterprise abduction, §88.f) + the authorization gate land later.
        IRFlowNode::Warden(node) => wire_integrations::run_warden(node, ctx).await,
        // §Fase 51.d.2 — the `yield` measurement point. SURFACE only: emits the
        // canonical `step_type: "yield"` wire shape. The actual amplitude
        // collapse + one-shot delimited continuation is the §51.e reference
        // simulator / enterprise QuIDD-QPU backend.
        IRFlowNode::Yield(node) => wire_integrations::run_yield(node, ctx).await,
        // §Fase 52.c — `run <Flow>(args)` flow-step. SURFACE here (binds the
        // invocation outcome); the real recursive flow dispatch under the
        // daemon's identity is the §52.c daemon executor.
        IRFlowNode::Run(node) => algebraic_handlers::run_run(node, ctx).await,
    }
}

// ────────────────────────────────────────────────────────────────────
//  Unit tests — drift gate + smoke
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel_token::CancellationFlag;

    #[test]
    fn budget_dispatch_errors_carry_their_typed_codes() {
        // §Fase 72.c/d — the two budget-exhaustion surfaces have distinct codes
        // so an operator (and the enterprise supervisor) can tell a hard block
        // from a reschedule.
        let blocked = DispatchError::EffectQuotaExhausted {
            effect: "TelnyxCall".into(),
            retry_at_ms: 1_700_000_000_000,
        };
        let msg = blocked.to_string();
        assert!(msg.contains("axon-E0810"), "{msg}");
        assert!(msg.contains("EffectQuotaExhausted") && msg.contains("Tool(TelnyxCall)"), "{msg}");
        assert!(msg.contains("not emitted"), "{msg}");

        let deferred = DispatchError::EffectDeferred {
            effect: "TelnyxCall".into(),
            retry_at_ms: 1_700_000_000_000,
        };
        let msg = deferred.to_string();
        assert!(msg.contains("axon-E0811"), "{msg}");
        assert!(msg.contains("EffectDeferred") && msg.contains("reschedules"), "{msg}");
    }

    /// §Fase 33.y.l drift-gate update — the historical
    /// `shim_reason_cardinality_45_variants` /
    /// `shim_reason_slugs_are_unique` /
    /// `shim_reason_slugs_are_well_formed` /
    /// `legacy_shim_returns_handled_on_happy_path` /
    /// `legacy_shim_returns_cancel_when_flag_set` /
    /// `shim_reason_slug_matches_ir_flow_node_kind` tests are
    /// RETIRED here. The replacement coverage lives in:
    ///
    ///   - `tests/fase33y_b_dispatcher_skeleton.rs` — IR-variant
    ///     catalog cardinality + slug uniqueness via
    ///     `flow_plan::ir_flow_node_kind` directly (single source
    ///     of truth, no more `ShimReason::slug` duplication).
    ///   - `tests/fase33y_l_parity_gate.rs` — D7 build-time grep
    ///     invariant: zero `unimplemented!` / `todo!` / `legacy_shim`
    ///     symbols in `flow_dispatcher/*.rs`.

    /// 33.y.b branch_path: empty at flow root.
    #[test]
    fn dispatch_ctx_branch_path_empty_at_root() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let ctx = DispatchCtx::new(
            "F",
            "stub",
            "",
            CancellationFlag::new(),
            tx,
        );
        assert!(ctx.branch_path.is_empty());
        assert_eq!(ctx.branch_path_string(), "");
        assert_eq!(ctx.step_counter, 0);
    }

    /// §Fase 65.C — `with_api_key` carries the per-tenant key into the ctx;
    /// the default is `None` (env-key behavior).
    #[test]
    fn dispatch_ctx_with_api_key_carries_per_tenant_key() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let ctx = DispatchCtx::new("F", "kimi", "", CancellationFlag::new(), tx);
        assert_eq!(ctx.api_key, None, "default is env-key (None)");

        let (tx2, _rx2) = mpsc::unbounded_channel();
        let ctx2 = DispatchCtx::new("F", "kimi", "", CancellationFlag::new(), tx2)
            .with_api_key(Some("sk-tenant-42".to_string()));
        assert_eq!(ctx2.api_key.as_deref(), Some("sk-tenant-42"));
    }

    /// 33.y.b branch_path: multi-segment join is wire-stable.
    #[test]
    fn dispatch_ctx_branch_path_joins_segments() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new(
            "F",
            "stub",
            "",
            CancellationFlag::new(),
            tx,
        );
        ctx.branch_path.push("par[0]".to_string());
        ctx.branch_path.push("step[1]".to_string());
        assert_eq!(ctx.branch_path_string(), "par[0].step[1]");
    }

    /// 33.y.b DispatchError Display surface produces actionable
    /// messages for every variant.
    #[test]
    fn dispatch_error_display_surface() {
        let cases: Vec<(DispatchError, &str)> = vec![
            (
                DispatchError::BackendError {
                    name: "anthropic".to_string(),
                    message: "rate limited".to_string(),
                },
                "backend 'anthropic' stream() failed: rate limited",
            ),
            (DispatchError::UpstreamCancelled, "upstream cancelled mid-dispatch"),
            (
                DispatchError::MissingDependency { name: "pem_async" },
                "dispatcher missing dependency: pem_async",
            ),
            (DispatchError::ChannelClosed, "channel closed (consumer dropped)"),
        ];
        for (err, expected) in cases {
            assert_eq!(format!("{err}"), expected);
        }
    }

    /// 33.y.c smoke: dispatch_node dispatches Step through the
    /// graduated pure-shape handler (not the shim) and returns
    /// `NodeOutcome::Completed` with the stub backend's canonical
    /// "(stub)" 1-token output.
    #[tokio::test]
    async fn dispatch_node_step_routes_to_pure_shape_handler() {
        use crate::ir_nodes::*;

        let step = IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
            name: "Generate".to_string(),
            persona_ref: String::new(),
            given: String::new(),
            ask: "hi".to_string(),
            use_tool: None,
            probe: None,
            reason: None,
            weave: None,
            output_type: String::new(),
            confidence_floor: None,
            navigate_ref: String::new(),
            apply_ref: String::new(),
            pix_ops: Vec::new(),
            guards: Vec::new(),
            requires_context: None,            now_tz: None,            body: Vec::new(),
        };
        let node = IRFlowNode::Step(step);

        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new(
            "F",
            "stub",
            "",
            CancellationFlag::new(),
            tx,
        );

        let outcome = dispatch_node(&node, &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Completed {
                output,
                tokens_emitted,
                step_index,
            } => {
                assert_eq!(output, "(stub)");
                assert_eq!(tokens_emitted, 1);
                assert_eq!(step_index, 0);
            }
            other => panic!("post-33.y.c: Step routes to pure_shape handler returning Completed; got {other:?}"),
        }
    }
}

/// §Fase 119.d — re-enter a parked continuation: rebuild a ctx from the
/// seed, bind the wake payload under the event name, walk the remaining
/// top-level nodes, and record the terminal outcome in the parking lot
/// (the original client's stream ended at the halt, so the lot is where
/// the result lands).
pub fn resume_parked_flow(
    parked: crate::hibernation::ParkedFlow,
    wake_payload: String,
) -> futures::future::BoxFuture<'static, ()> {
    // The explicit BoxFuture return type cuts the Send-inference cycle:
    // dispatch_node → run_emit → spawn(resume_parked_flow) → dispatch_node.
    // With a nominal (Send) future type here, run_emit's Send proof no longer
    // recurses into this body.
    Box::pin(resume_parked_flow_inner(parked, wake_payload))
}

async fn resume_parked_flow_inner(
    parked: crate::hibernation::ParkedFlow,
    wake_payload: String,
) {
    let id = parked.continuation_id.clone();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    // Drain wire events: no client is attached to a resumed run.
    tokio::spawn(async move { while rx.recv().await.is_some() {} });

    let mut ctx = DispatchCtx::new(
        &parked.flow_name,
        &parked.backend_name,
        &parked.system_prompt,
        crate::cancel_token::CancellationFlag::new(),
        tx,
    );
    ctx.tenant_id = parked.tenant_id.clone();
    ctx.session_id = parked.session_id.clone();
    ctx.step_counter = parked.step_counter;
    ctx.let_bindings = parked.let_bindings.clone();
    ctx.let_bindings
        .insert(parked.event_name.clone(), wake_payload);
    ctx = ctx
        .with_computes(parked.compute_specs.clone())
        .with_mandates(parked.mandate_specs.clone())
        .with_lambdas(parked.lambda_data_specs.clone())
        .with_ots(parked.ots_specs.clone());

    let mut last_output = String::new();
    for node in &parked.remaining_nodes {
        match dispatch_node(node, &mut ctx).await {
            Ok(NodeOutcome::Completed { output, .. }) => last_output = output,
            Ok(NodeOutcome::Return { value }) => {
                last_output = value;
                break;
            }
            Ok(NodeOutcome::Hibernated { .. }) => {
                // A second hibernate in the continuation: not yet defined —
                // record honestly rather than parking a park.
                crate::hibernation::parking_lot().record_outcome(
                    &id,
                    crate::hibernation::ResumedOutcome::Failed {
                        error: "a resumed continuation hibernated again; chained \
                                hibernation is not yet defined"
                            .to_string(),
                    },
                );
                return;
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(continuation = id.as_str(), error = ?e,
                    "resumed continuation failed");
                crate::hibernation::parking_lot().record_outcome(
                    &id,
                    crate::hibernation::ResumedOutcome::Failed {
                        error: format!("{e:?}"),
                    },
                );
                return;
            }
        }
    }
    crate::hibernation::parking_lot().record_outcome(
        &id,
        crate::hibernation::ResumedOutcome::Completed { output: last_output },
    );
}

