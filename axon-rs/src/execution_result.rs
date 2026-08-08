//! §Fase 118.b.2 — the flow-execution RESULT, in a module that reaches nothing.
//!
//! **THE THIRD INSTANCE OF THE SMELL, and the largest so far.** `AXON_VERSION`
//! (§118.a.1) lived in the flow executor; `IngestProvenance` (§118.b.1) lived in
//! the OOXML reader; `ServerExecutionResult` and `EnforcementSummaryWire` lived
//! in `axon_server.rs` — 29,734 lines of `axum` router. Same shape every time: a
//! general concept parked in the specific module that first needed it, silently
//! chaining everything downstream to that module's dependencies.
//!
//! What it chained here was not a leaf. `ServerExecutionResult` is the input of
//! [`crate::wire_envelope::FlowEnvelope::from_execution_result`] (§39.b), and
//! `EnforcementSummaryWire` is threaded through `flow_dispatcher`,
//! `flow_dispatcher::pure_shape` and `streaming_via_dispatcher` — **the core
//! execution path**. So `axon run`, which opens no socket, could not compile
//! without the HTTP server, for two structs that contain no HTTP: eight counters,
//! a policy slug, and an aggregation of step names and token totals.
//!
//! Neither type is server-specific. `server_execute` was simply the first caller
//! to need a place to put the answer. The name `ServerExecutionResult` is kept
//! verbatim — it is crate-public since §39.b and named by
//! `tests/fase39b_wire_envelope_integration.rs` and
//! `tests/fase39c_epistemic_ownership_integration.rs` — and `axon_server`
//! re-exports both, so every existing call site (including `axon-enterprise`,
//! which consumes `axon::axon_server::ServerExecutionResult`) keeps resolving.
//!
//! **This module must never acquire a dependency.** Its whole value is that the
//! execution path can name its own result type without linking a web framework.

use serde::Serialize;

/// Server-side execution result.
///
/// §Fase 39.b — promoted from `struct` to `pub struct` (and all fields
/// to `pub`) so the new `crate::wire_envelope::FlowEnvelope` module
/// can consume it as the converter input. Pre-39.b this type was
/// internal to `axon_server`; v2.0.0 elevates it to a crate-public
/// shape because it is the canonical input of the wire envelope
/// builder. It is intentionally NOT part of the JSON wire (the
/// FlowEnvelope is); it remains a runtime-internal aggregation step.
#[derive(Debug, Clone, Serialize)]
pub struct ServerExecutionResult {
    pub success: bool,
    pub flow_name: String,
    pub source_file: String,
    pub backend: String,
    pub steps_executed: usize,
    pub latency_ms: u64,
    pub tokens_input: u64,
    pub tokens_output: u64,
    pub anchor_checks: usize,
    pub anchor_breaches: usize,
    pub errors: usize,
    pub step_names: Vec<String>,
    pub step_results: Vec<String>,
    pub trace_id: u64,
    /// §Fase 33.e — Per-step stream-effect policies declared in the
    /// source. Each entry is `(step_name, policy_slug)` where slug is
    /// one of the closed catalog `{drop_oldest, degrade_quality,
    /// pause_upstream, fail}`. Empty when no step in the flow declares
    /// a `<stream:<policy>>` effect. Surfaced on the SSE
    /// `axon.complete` wire envelope so adopters can observe the
    /// policy is bound to runtime.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effect_policies: Vec<(String, String)>,

    /// §Fase 33.x.d — Per-step `EnforcementSummary` from the
    /// `StreamPolicyEnforcer` runs. Empty in two cases:
    ///   1. Legacy synchronous path (deleted in 33.z.e) —
    ///      the enforcer is not run; the wire stays byte-identical
    ///      with v1.24.0 (D4 byte-compat).
    ///   2. Async streaming path where no step in the flow has a
    ///      declared `<stream:<policy>>` effect — the enforcer is
    ///      not constructed (no policy to enforce); D2 contract.
    /// Surfaced on the SSE `axon.complete` wire envelope so adopters
    /// can observe whether the declared policy actually fired in
    /// production (a `drop_oldest` policy that never fires under
    /// sustained load is a configuration smell).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enforcement_summaries: Vec<(String, EnforcementSummaryWire)>,

    /// §Fase 33.x.g — Closed-catalog runtime warnings. Populated
    /// only when `server_execute_streaming` falls back to the
    /// legacy synchronous path; carries one `axon-W002
    /// streaming-not-supported` warning with the specific
    /// `FallbackMode` tag identifying WHY. Empty on the happy
    /// (async-streaming-active) path = D4 byte-compat preserved
    /// (wire field elided when empty).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_warnings: Vec<crate::runtime_warnings::RuntimeWarning>,

    /// §Fase 39.c.y — semantic provenance events from the runtime
    /// walk (`retrieve:<store>`, `shield:<name>`, `mutate:<store>`,
    /// etc.). Merged into the `FlowEnvelope.provenance_chain` by
    /// the converter. Empty for flows with no taxonomy-participating
    /// steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provenance_events: Vec<String>,

    /// §Fase 39.c.z — surfaced blame attribution when the flow
    /// proceeded on degraded posture (anchor breach / shield
    /// rejection / store breach / backend soft-fail / type mismatch).
    /// `None` on clean happy path; the converter writes this slot
    /// into the wire envelope's `blame_attribution` field verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blame_attribution: Option<crate::wire_envelope::BlameContext>,

    /// §Fase 55.b — per-tool epistemic envelopes (`base`, `scope`,
    /// `confidence`) for every flow-level `use <Tool>` whose tool declares
    /// an `epistemic:<level>` effect. Propagated from the runner's
    /// IR-derived capture and written into
    /// `FlowEnvelope.epistemic_envelopes` by the converter; the streaming
    /// path derives the identical set via
    /// `resolve_epistemic_envelopes_for_flow`. Empty (and elided from the
    /// wire) for flows that dispatch no epistemic-annotated tool.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub epistemic_envelopes: Vec<crate::epistemic_capture::EpistemicEnvelope>,

    /// §Fase 65.F — the HONEST hard-failure detail when a node's
    /// `DispatchError` aborted the non-streaming flow (a failing
    /// `persist`/`mutate`/`purge` store write, a backend error, etc.):
    /// `Some("flow 'F' failed at persist into 'S': <cause>")`, naming the
    /// failing node + the underlying cause. Byte-parity with the streaming
    /// dispatcher's `FlowError.error` (§37.e/D6). `None` on the clean path;
    /// the converter writes this slot into `FlowEnvelope.error` verbatim and
    /// counts it as one `errors` so the wire envelope's certainty bounds to
    /// the derived ceiling. Closes the §65.E.2 silent-abort regression (a
    /// pre-insert store failure used to present as `success:false` + empty
    /// result + zero diagnostic). Elided from the wire when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// §Fase 91.b — the run's temporal record when any step rendered a
    /// declared `now:` (`captured_utc` + `tzdb_version` + `zones` — the
    /// replayability triple of `time_is_an_explicit_input`, §71/§91). The
    /// converter writes it into `FlowEnvelope.temporal_context` verbatim.
    /// `None` — and elided from the wire — for every `now:`-less flow, so
    /// every pre-§91 envelope stays byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporal_context: Option<crate::temporal_context::TemporalRecord>,
}

/// §Fase 33.x.d — Wire-serializable mirror of
/// [`crate::stream_effect_dispatcher::EnforcementSummary`] published
/// on `axon.complete` per the D2 contract.
///
/// `policy_slug` is the closed-catalog slug of the policy that the
/// enforcer ran (`drop_oldest` / `degrade_quality` / `pause_upstream`
/// / `fail`); `pushed`/`delivered` count chunks the enforcer's input
/// stream produced + the consumer drained respectively. The four
/// `*_hits` / `*_blocks` / `*_overflows` counters surface
/// policy-specific activations so adopters can verify the declared
/// policy actually fired (D2 contract — declaration ⟺ runtime
/// behavior).
///
/// All counters are `u64` so high-throughput long-running flows
/// don't risk overflow. `failed` is set only when the enforcer's
/// internal stream surfaced `BackpressurePolicy::Fail` overflow.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct EnforcementSummaryWire {
    pub policy_slug: String,
    pub chunks_pushed: u64,
    pub chunks_delivered: u64,
    pub drop_oldest_hits: u64,
    pub degrade_quality_hits: u64,
    pub pause_upstream_blocks: u64,
    pub fail_overflows: u64,
    pub failed: bool,
}

impl EnforcementSummaryWire {
    /// Project from the rich internal `EnforcementSummary` (which has
    /// `policy: Option<&'static str>`) into the wire-stable shape.
    pub fn from_summary(
        s: &crate::stream_effect_dispatcher::EnforcementSummary,
    ) -> Self {
        Self {
            policy_slug: s.policy.unwrap_or("").to_string(),
            chunks_pushed: s.chunks_pushed,
            chunks_delivered: s.chunks_delivered,
            drop_oldest_hits: s.drop_oldest_hits,
            degrade_quality_hits: s.degrade_quality_hits,
            pause_upstream_blocks: s.pause_upstream_blocks,
            fail_overflows: s.fail_overflows,
            failed: s.failed,
        }
    }
}
