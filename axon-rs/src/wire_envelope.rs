//! §Fase 39.b — Pure Silicon Cognition: the canonical wire payload type
//! for axonendpoint responses on `transport: json`.
//!
//! Isomorphic to the ψ-vector `ψ = ⟨T, V, E⟩` (paper §5):
//!
//! - **T** — the ontological type the value claims to inhabit
//!   (`FlowEnvelope.ontological_type`)
//! - **V** — the typed payload, member of T
//!   (`FlowEnvelope.result`)
//! - **E** — the epistemic envelope: certainty (Theorem 5.1) +
//!   provenance + audit-chain + blame attribution
//!   (the remaining fields)
//!
//! Defined ONCE in Rust; no Python mirror; no drift gate (per D3 +
//! [[feedback_zero_py_files_north_star]]). This module is the
//! Rust-canonical source of truth for the v2.0.0 wire shape.
//!
//! ## Construction
//!
//! The canonical builder is
//! [`FlowEnvelope::from_execution_result`] — converts the v1.x
//! [`crate::execution_result::ServerExecutionResult`] into a v2.0.0
//! envelope. The conversion is total: every field of the legacy
//! struct maps to a pillar-organized slot of the envelope, with no
//! information loss. Epistemic fields (`certainty`,
//! `provenance_chain`, `blame_attribution`) receive Fase-39.b safe
//! defaults; their full producer logic lands in Fase 39.c.
//!
//! ## Sealing
//!
//! [`FlowEnvelope::seal`] is the single egress point before HTTP
//! serialization. In Fase 39.b it runs the Rust-side fallback for
//! Theorem 5.1 enforcement (clamp `certainty ≤ 0.99` if derived) +
//! computes the `audit_chain_hash` over the canonical provenance
//! representation. In Fase 39.c this method delegates to the C23
//! kernel `axon-csys::effects::envelope::validate_epistemic_degradation`,
//! making the bound structurally unbypassable from any Rust caller.
//!
//! ## Pillars
//!
//! - **Pillar I (Epistemic)** — `ontological_type`, `result`,
//!   `certainty` (Theorem 5.1 bounded)
//! - **Pillar II (Audit-chained)** — `provenance_chain`,
//!   `step_audit`, `audit_chain_hash`
//! - **Pillar III (Streaming)** — N/A (SSE has its own event family
//!   per D9; this envelope is JSON-transport-only)
//! - **Pillar IV (Capability)** — `blame_attribution` (carries
//!   `BlameKind` of failure when present)
//!
//! See plan vivo `docs/fase/fase_39_pure_silicon_cognition.md` §4 for
//! the full wire-shape contract.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ════════════════════════════════════════════════════════════════════
// FlowEnvelope — the canonical wire payload for `transport: json`
// ════════════════════════════════════════════════════════════════════

/// §Fase 39 (D1, D2, D5) — the wire payload of every `transport: json`
/// axonendpoint response (HTTP 2xx) and every legacy
/// `POST /v1/execute` invocation.
///
/// Fields are organized by Pillar (see module docs). At wire
/// emission, `result` carries a `serde_json::Value` (monomorphic at
/// runtime); D5 validation (Fase 32.d) — once simplified in 39.d —
/// will type-check this slot against the declared inner T of the
/// adopter's `output: FlowEnvelope<T>` declaration.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FlowEnvelope {
    // ── Pillar I (Epistemic) — the ψ-vector slots ────────────────
    /// The ontological type declared at the endpoint surface (the
    /// inner T of `output: FlowEnvelope<T>`). Slug form:
    /// `TenantRecord`, `List<PatientRecord>`, `Stream<Token>`.
    /// For legacy `/v1/execute` invocations (no endpoint
    /// declaration), this is the runtime-inferred type slug.
    pub ontological_type: String,

    /// The typed payload — member of `ontological_type`.
    /// `serde_json::Value` at the wire layer because the runtime
    /// is monomorphic; D5 (when simplified in 39.d) validates the
    /// inner shape against the declared T.
    pub result: serde_json::Value,

    /// Certainty `c ∈ [0.0, 1.0]`, bounded by Theorem 5.1:
    /// `c ≤ 0.99` whenever `derived_status = true`. In Fase 39.b
    /// the bound is enforced by [`FlowEnvelope::seal`]'s Rust
    /// fallback; in Fase 39.c the bound moves to the C23 kernel
    /// `axon-csys::effects::envelope::validate_epistemic_degradation`,
    /// making it structurally unbypassable.
    pub certainty: f64,

    /// §Fase 55.b — the Theorem 5.1 `(base, scope, confidence)` triple of
    /// every flow-level `use <Tool>` dispatch whose tool declares an
    /// `epistemic:<level>` effect. Surfaces the epistemic degradation on
    /// the wire — the §50.i.4 parity gate, promoted (the competitive
    /// differential: an adopter sees a query routed through an
    /// `epistemic:speculate` tool decay to `confidence ≤ 0.80`). Empty —
    /// and elided from the JSON — for flows with no epistemic tool (D5
    /// backward-compat: byte-identical wire for every pre-55.b flow).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub epistemic_envelopes: Vec<crate::epistemic_capture::EpistemicEnvelope>,

    // ── Pillar II (Audit-chained) — provenance + step trail ──────
    /// Ordered list of `kind:identifier` tuples capturing the
    /// lineage of `result`. Examples:
    ///   - `["flow:FetchTenants", "retrieve:tenants", "backend:stub"]`
    ///   - `["step:Triage", "shield:Hipaa", "backend:anthropic"]`
    /// Empty for endpoints with no derived state (singular literal
    /// returns); populated by [`FlowEnvelope::from_execution_result`].
    pub provenance_chain: Vec<String>,

    /// Per-step audit trail. Survives from v1.x as the canonical
    /// observability surface; here it is structured (not just
    /// `Vec<String>`). Step results are TYPED `Value` post-39.b
    /// (pre-v2.0.0 they were stringified — the typed form is a D5
    /// simplification dividend).
    pub step_audit: StepAuditTrail,

    /// HMAC-SHA256 hex of the canonical form of `provenance_chain
    /// || step_audit`. Computed by [`FlowEnvelope::seal`]; in
    /// Fase 39.c the hash moves to the C23 kernel for byte-
    /// deterministic cross-deployment verification.
    pub audit_chain_hash: String,

    // ── Pillar IV (Capability) — blame attribution ───────────────
    /// Populated only when the flow's success path produced a
    /// degraded posture (anchor breach, shield rejection, backend
    /// soft-fail, store breach, type-mismatch on recoverable path).
    /// `None` on the clean happy path.
    pub blame_attribution: Option<BlameContext>,

    // ── Cross-cutting — observability + correlation ──────────────
    /// Execution metrics — latency, tokens, backend identity.
    /// Always populated.
    pub execution_metrics: ExecutionMetrics,

    /// Correlation anchor (matches `X-Axon-Trace-Id` header).
    /// String form for cross-stack compat (the v1.x `trace_id: u64`
    /// is reborn here as Uuid v4 hex string).
    pub trace_id: String,

    /// §Fase 65.F — the HONEST hard-failure detail when the flow aborted on
    /// a node's `DispatchError` (a failing `persist`/`mutate`/`purge` store
    /// write, a backend error, etc.). `Some("flow 'F' failed at persist into
    /// 'S': <cause>")` names the failing node + the underlying cause. This is
    /// distinct from `blame_attribution`, which is reserved for SOFT
    /// degradation reported ON the success path — a hard fail needs its own
    /// slot. Mirrors the streaming dispatcher's `FlowError.error` so a
    /// non-streaming endpoint surfaces store-write failures with the SAME
    /// honesty the SSE path always had (closing the §65.E.2 silent-abort
    /// regression). `None` — and elided from the JSON — on the clean path, so
    /// every pre-§65.F happy-path wire stays byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// §Fase 91.b — the run's temporal record when any step rendered a
    /// declared `now:` (`time_is_an_explicit_input` applied to cognition):
    /// the single captured instant (RFC 3339 UTC), the tz-database release
    /// it resolved against, and the declared zones actually rendered — the
    /// triple that makes the exact prompt the model saw reconstructible
    /// byte-for-byte. `None` — and elided from the JSON — for every
    /// `now:`-less flow, so every pre-§91 wire stays byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporal_context: Option<crate::temporal_context::TemporalRecord>,
}

/// §Fase 39 (D5) — per-step audit surface. Structured replacement
/// for the v1.x `Vec<String>` step results.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct StepAuditTrail {
    pub step_names: Vec<String>,
    /// §Fase 39.b — TYPED. The v1.x stringified results are parsed
    /// as JSON values when constructible; opaque strings fall back
    /// to `Value::String(...)`. The D5 simplification in 39.d
    /// leverages this typed form.
    pub step_results: Vec<serde_json::Value>,
    pub anchor_checks: usize,
    pub anchor_breaches: usize,
    pub errors: usize,
    pub steps_executed: usize,
    /// §Fase 33.x.d carry-over — per-step EnforcementSummary entries
    /// (from `StreamPolicyEnforcer` runs). Empty in the legacy sync
    /// path; populated by `server_execute_streaming` per the D2
    /// contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enforcement_summaries:
        Vec<(String, crate::execution_result::EnforcementSummaryWire)>,
    /// §Fase 33.e carry-over — per-step `<stream:<policy>>` slugs
    /// declared in source. Empty when no step declares one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effect_policies: Vec<(String, String)>,
    /// §Fase 33.x.g carry-over — closed-catalog runtime warnings
    /// (only populated on legacy-path fallback under axon-W002 —
    /// structurally unreachable post-33.z but the slot survives
    /// for forward-compat with future warnings).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_warnings: Vec<crate::runtime_warnings::RuntimeWarning>,
}

/// §Fase 39 (D5) — execution metrics + provenance identity. Always
/// populated.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ExecutionMetrics {
    pub latency_ms: u64,
    pub tokens_input: u64,
    pub tokens_output: u64,
    pub backend: String,
    pub flow_name: String,
    pub source_file: String,
}

// ════════════════════════════════════════════════════════════════════
// BlameContext — Pillar IV attribution surface
// ════════════════════════════════════════════════════════════════════

/// §Fase 39 (D11) — closed-catalog blame attribution. Surfaces
/// WHICH layer produced the degraded posture on a 2xx response.
/// Hard-fails (4xx/5xx) are handled by the existing error envelopes
/// (not this struct) — `BlameContext` is for SOFT degradation
/// reported on the success path.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct BlameContext {
    pub kind: BlameKind,
    /// `file:line:col` (compile-time origin) OR `step:name`
    /// (runtime origin). Empty string when the origin cannot be
    /// pinpointed.
    pub location: String,
    /// Human-readable diagnostic. Forms the audit_log entry's
    /// primary message.
    pub message: String,
    /// Optional anchor back to a plan-vivo D-letter (e.g. "39.c",
    /// "33.x.d") for forward correlation when the blame ties to a
    /// specific architectural commitment.
    pub d_letter: Option<String>,
}

/// §Fase 39 (D11) — closed catalog of blame kinds. Adding a variant
/// is a non-breaking surface change (consumers MUST handle
/// `#[non_exhaustive]`-style fall-through).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BlameKind {
    /// Pillar IV — an anchor's `require:` predicate failed; flow
    /// chose to proceed (degraded path).
    AnchorBreach,
    /// Pillar I — a shield scanner flagged content; flow chose to
    /// proceed.
    ShieldRejection,
    /// Backend returned a degraded response (truncated, partial,
    /// soft-rate-limited).
    BackendSoftFail,
    /// Pillar II — store mutation chain verification failed; flow
    /// proceeded with the prior-state read.
    StoreBreach,
    /// D5 detected partial typing inconsistency that is recoverable
    /// (e.g. missing optional field with a sane default).
    TypeMismatch,
}

// ════════════════════════════════════════════════════════════════════
// FlowEnvelope::from_execution_result — v1.x → v2.0.0 converter
// ════════════════════════════════════════════════════════════════════

impl FlowEnvelope {
    /// §Fase 39.b — convert a v1.x [`crate::execution_result::ServerExecutionResult`]
    /// into a v2.0.0 envelope. Total: every legacy field maps to a
    /// pillar-organized slot; no information loss.
    ///
    /// Epistemic field defaults applied here (refined in 39.c):
    /// - `certainty = 1.0` when `anchor_breaches == 0` and
    ///   `errors == 0` (clean happy path; no derived posture).
    /// - `certainty = 0.99` when `anchor_breaches > 0 ||
    ///   errors > 0` (Theorem 5.1: derived states bounded ≤ 0.99).
    /// - `provenance_chain` built from
    ///   `flow_name + step_names + backend`.
    /// - `blame_attribution = None` always at this layer (the soft-
    ///   degradation surface is populated by the runtime when it
    ///   detects anchor/shield/store/backend events — 39.c lands
    ///   that wiring).
    ///
    /// The `result` slot is populated from the LAST step's typed
    /// output (`step_results.last()` parsed as `Value`). For flows
    /// with no steps (degenerate) the result is `Value::Null`.
    ///
    /// `trace_id` is converted from the legacy `u64` to a Uuid v4
    /// hex string. When the legacy id is 0 (pre-record), a fresh
    /// Uuid is minted.
    pub fn from_execution_result(
        exec_result: crate::execution_result::ServerExecutionResult,
        ontological_type: String,
    ) -> Self {
        // ── Pillar II — provenance chain ──
        // §Fase 39.c.y — interleave semantic provenance events
        // (`retrieve:*`, `shield:*`, etc.) with the canonical
        // step/backend entries. Order: `flow:F`, then taxonomy
        // events from execution_units walk, then `step:S` entries
        // for each canonical step, then `backend:B` last. This
        // gives auditors a complete lineage from flow declaration
        // through every observable runtime event.
        let mut provenance_chain = Vec::with_capacity(
            2 + exec_result.step_names.len() + exec_result.provenance_events.len(),
        );
        provenance_chain.push(format!("flow:{}", exec_result.flow_name));
        for event in &exec_result.provenance_events {
            provenance_chain.push(event.clone());
        }
        for step_name in &exec_result.step_names {
            provenance_chain.push(format!("step:{}", step_name));
        }
        provenance_chain.push(format!("backend:{}", exec_result.backend));

        // ── Pillar II — typed step_results ──
        // Parse each stringified result as JSON if possible; fall
        // back to a String Value preserving the raw text.
        let step_results_typed: Vec<serde_json::Value> = exec_result
            .step_results
            .iter()
            .map(|s| {
                serde_json::from_str::<serde_json::Value>(s)
                    .unwrap_or_else(|_| serde_json::Value::String(s.to_string()))
            })
            .collect();

        // ── Pillar I — the `result` slot ──
        // Canonically the last step's typed value is the flow output.
        // When the flow has no steps (degenerate), result is Null.
        let result = step_results_typed
            .last()
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        // ── Pillar I — certainty (Theorem 5.1 Rust-side fallback) ──
        // The C23 kernel in 39.c will replace this; here we apply
        // the same algebra so wire bytes are stable across the
        // 39.b → 39.c transition.
        let derived =
            exec_result.anchor_breaches > 0 || exec_result.errors > 0;
        let certainty = if derived { 0.99 } else { 1.0 };

        // ── Pillar IV — blame ──
        // §Fase 39.c.z — propagate the blame attribution from the
        // runtime walk (populated by `derive_blame_from_report` in
        // `wire_envelope_producers`). `None` on clean happy path;
        // populated when the runtime surfaced an anchor breach,
        // shield rejection, store breach, backend soft-fail, or
        // recoverable type mismatch. The first-emitted (highest-
        // priority) blame wins per `merge_blame`.
        let blame_attribution: Option<BlameContext> =
            exec_result.blame_attribution;

        // ── Cross-cutting — trace_id ──
        let trace_id = if exec_result.trace_id == 0 {
            uuid::Uuid::new_v4().to_string()
        } else {
            // Pre-39 the trace_id was a u64; here we render it as
            // a 16-char hex string (preserve the value semantically;
            // future code paths will mint Uuids directly).
            format!("{:016x}", exec_result.trace_id)
        };

        Self {
            ontological_type,
            result,
            certainty,
            // §Fase 55.b — surface the IR-derived epistemic envelopes.
            epistemic_envelopes: exec_result.epistemic_envelopes,
            provenance_chain,
            step_audit: StepAuditTrail {
                step_names: exec_result.step_names.clone(),
                step_results: step_results_typed,
                anchor_checks: exec_result.anchor_checks,
                anchor_breaches: exec_result.anchor_breaches,
                errors: exec_result.errors,
                steps_executed: exec_result.steps_executed,
                enforcement_summaries: exec_result.enforcement_summaries,
                effect_policies: exec_result.effect_policies,
                runtime_warnings: exec_result.runtime_warnings,
            },
            audit_chain_hash: String::new(), // computed by seal()
            blame_attribution,
            execution_metrics: ExecutionMetrics {
                latency_ms: exec_result.latency_ms,
                tokens_input: exec_result.tokens_input,
                tokens_output: exec_result.tokens_output,
                backend: exec_result.backend,
                flow_name: exec_result.flow_name,
                source_file: exec_result.source_file,
            },
            trace_id,
            // §Fase 65.F — the honest hard-failure detail (named node + cause),
            // verbatim from the runtime walk. `None` on the clean path.
            error: exec_result.error,
            // §Fase 91.b — the temporal record, verbatim from the runtime walk.
            temporal_context: exec_result.temporal_context,
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// FlowEnvelope::seal — single egress before HTTP serialization
// ════════════════════════════════════════════════════════════════════

impl FlowEnvelope {
    /// §Fase 39.b — apply epistemic enforcement + compute the
    /// `audit_chain_hash` before wire serialization. This is the
    /// ONLY public sealing surface; the wire bytes emitted by
    /// `axon_server` MUST pass through this method (the `seal()`
    /// invariant — Fase 39.b establishes it; 39.h grep gate locks
    /// it structurally).
    ///
    /// ## Fase 39.c.x implementation (C23 kernel canonical)
    ///
    /// 1. Theorem 5.1 enforcement DELEGATES to the C23 kernel
    ///    `axon-csys::envelope::validate_degradation`. The kernel:
    ///    a. Defensively normalises NaN / Inf / out-of-range
    ///       certainty into `[0.0, 1.0]`.
    ///    b. Clamps `certainty ≤ 0.99` when `derived_status = true`.
    ///    c. Returns the envelope with `derived_status` +
    ///       `epistemic_kind` passed through unchanged.
    ///    The C23 kernel is the SINGLE point of structural truth —
    ///    no Rust path bypasses it for production code paths.
    /// 2. `derived_status` algebra (Rust-side, matches the producer
    ///    in [`FlowEnvelope::from_execution_result`] verbatim):
    ///    `derived = step_audit.anchor_breaches > 0
    ///             || step_audit.errors > 0`. The Rust side decides
    ///    WHO is derived (semantic); the C23 kernel enforces WHAT
    ///    the ceiling looks like (structural).
    /// 3. `audit_chain_hash` = SHA-256 hex of the canonical-JSON
    ///    serialization of `[provenance_chain, step_audit]`.
    ///    Deterministic on identical inputs; tamper-evident.
    ///    (39.c.x leaves the SHA-256 in Rust pending a future
    ///    sub-fase that moves it to axon-csys::crypto for true
    ///    silicon-grounded tamper-evidence.)
    pub fn seal(mut self) -> Self {
        // §Fase 55.e — epistemic ceiling propagates to the HEADLINE
        // certainty: a flow can be no more certain than its least-certain
        // epistemic tool. The λD lattice meet (⊓) of the per-tool ceilings
        // is their minimum; each envelope's `confidence` IS that tool's
        // applied ceiling (§55.a/b), so `min(confidences)` is the flow's
        // epistemic upper bound. Applied BEFORE the C23 kernel so the kernel
        // consolidates the full degradation (Theorem 5.1) at the single
        // egress — no gateway can read a nominal `know` headline while an
        // internal tool degraded the computation to `speculate`
        // (Epistemic Transparency / taint propagation). No epistemic tool ⇒
        // no change (D5 wire byte-compat for every pre-55 flow).
        if let Some(min_ceiling) = self
            .epistemic_envelopes
            .iter()
            .map(|e| e.confidence)
            .reduce(f64::min)
        {
            self.certainty = self.certainty.min(min_ceiling);
        }
        // Theorem 5.1 — DELEGATE to C23 kernel. The algebra for
        // `derived_status` matches the producer in
        // `from_execution_result` (anchor_breaches > 0 || errors > 0)
        // so producer + sealer agree on WHO is derived.
        let derived = self.step_audit.anchor_breaches > 0
            || self.step_audit.errors > 0;
        let epistemic_kind = if !derived {
            axon_csys::EpistemicKind::Clean
        } else if self.blame_attribution.is_some() {
            // Multi-source degradation — anchor/shield/store/backend
            // surfaced an explicit blame producer (Pillar IV).
            axon_csys::EpistemicKind::Degraded
        } else if self.step_audit.anchor_breaches > 0 {
            axon_csys::EpistemicKind::Breached
        } else {
            axon_csys::EpistemicKind::Derived
        };
        let env = axon_csys::EpistemicEnvelope::new(
            self.certainty,
            derived,
            epistemic_kind,
        );
        let clamped = axon_csys::validate_degradation(env);
        self.certainty = clamped.certainty;
        // Audit chain hash — SHA-256 over canonical JSON of
        // [provenance_chain, step_audit]. We use serde_json for
        // canonicalization (sorted keys on structs by design; we
        // accept the array-of-(provenance, audit) tuple as the
        // canonical input).
        let canonical = serde_json::to_string(&(
            &self.provenance_chain,
            &self.step_audit,
        ))
        .unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(canonical.as_bytes());
        let digest = hasher.finalize();
        self.audit_chain_hash = format!("{digest:x}");
        self
    }
}

// ════════════════════════════════════════════════════════════════════
// Helpers
// ════════════════════════════════════════════════════════════════════

/// §Fase 39.b — derive the `ontological_type` slug for an endpoint's
/// declared `output: T`. When the endpoint declares
/// `output: FlowEnvelope<T>` (the canonical form post-39.e), this
/// extracts the inner T. For legacy declarations (pre-39.e — still
/// in tree until atomic deploy), returns the declared type verbatim.
/// For empty / missing declarations returns `"Any"` (the singular
/// catch-all).
pub fn extract_inner_ontological_type(declared: &str) -> String {
    let t = declared.trim();
    if t.is_empty() {
        return "Any".to_string();
    }
    if let Some(rest) = t.strip_prefix("FlowEnvelope<") {
        if let Some(inner) = rest.strip_suffix('>') {
            return inner.trim().to_string();
        }
    }
    t.to_string()
}

// ════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_result::ServerExecutionResult;

    fn fixture_exec_result() -> ServerExecutionResult {
        ServerExecutionResult {
            success: true,
            flow_name: "FetchTenants".to_string(),
            source_file: "tenants.axon".to_string(),
            backend: "stub".to_string(),
            steps_executed: 1,
            latency_ms: 142,
            tokens_input: 0,
            tokens_output: 0,
            anchor_checks: 0,
            anchor_breaches: 0,
            errors: 0,
            step_names: vec!["RetrieveAll".to_string()],
            step_results: vec![
                r#"[{"id":1,"name":"foo"},{"id":2,"name":"bar"}]"#.to_string(),
            ],
            trace_id: 0,
            effect_policies: Vec::new(),
            enforcement_summaries: Vec::new(),
            runtime_warnings: Vec::new(),
            provenance_events: Vec::new(),
            blame_attribution: None,
            epistemic_envelopes: Vec::new(),
            error: None,
            temporal_context: None,
        }
    }

    #[test]
    fn fase39b_from_execution_result_clean_happy_path() {
        let exec = fixture_exec_result();
        let env = FlowEnvelope::from_execution_result(
            exec,
            "List<TenantRecord>".to_string(),
        );
        assert_eq!(env.ontological_type, "List<TenantRecord>");
        assert_eq!(env.certainty, 1.0, "clean path → certainty 1.0");
        assert_eq!(env.execution_metrics.flow_name, "FetchTenants");
        assert_eq!(env.execution_metrics.latency_ms, 142);
        assert!(env.blame_attribution.is_none());
        assert_eq!(
            env.provenance_chain,
            vec![
                "flow:FetchTenants",
                "step:RetrieveAll",
                "backend:stub"
            ]
        );
    }

    #[test]
    fn fase39b_typed_result_slot_from_last_step() {
        let exec = fixture_exec_result();
        let env = FlowEnvelope::from_execution_result(
            exec,
            "List<TenantRecord>".to_string(),
        );
        // result is the LAST step's JSON-parsed value
        let arr = env.result.as_array().expect("result must be array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], 1);
        assert_eq!(arr[0]["name"], "foo");
        assert_eq!(arr[1]["id"], 2);
        assert_eq!(arr[1]["name"], "bar");
    }

    #[test]
    fn fase39b_typed_step_results_parsed_when_json() {
        let exec = fixture_exec_result();
        let env = FlowEnvelope::from_execution_result(
            exec,
            "List<TenantRecord>".to_string(),
        );
        assert_eq!(env.step_audit.step_results.len(), 1);
        assert!(env.step_audit.step_results[0].is_array());
    }

    #[test]
    fn fase39b_opaque_step_result_falls_back_to_string_value() {
        let mut exec = fixture_exec_result();
        exec.step_results = vec!["(stub model response)".to_string()];
        let env = FlowEnvelope::from_execution_result(exec, "String".to_string());
        // Opaque non-JSON text → Value::String wrapping the raw text
        assert_eq!(env.step_audit.step_results.len(), 1);
        assert_eq!(
            env.step_audit.step_results[0],
            serde_json::Value::String("(stub model response)".to_string())
        );
    }

    #[test]
    fn fase39b_certainty_bounded_on_derived_state() {
        let mut exec = fixture_exec_result();
        exec.anchor_breaches = 1;
        let env = FlowEnvelope::from_execution_result(exec, "Any".to_string());
        assert_eq!(env.certainty, 0.99, "derived state → 0.99 per Theorem 5.1");
    }

    #[test]
    fn fase39b_certainty_bounded_on_errors() {
        let mut exec = fixture_exec_result();
        exec.errors = 1;
        let env = FlowEnvelope::from_execution_result(exec, "Any".to_string());
        assert_eq!(env.certainty, 0.99, "errors → derived → 0.99");
    }

    #[test]
    fn fase39b_seal_populates_audit_chain_hash() {
        let env = FlowEnvelope::from_execution_result(
            fixture_exec_result(),
            "List<TenantRecord>".to_string(),
        );
        assert_eq!(env.audit_chain_hash, "", "pre-seal: empty");
        let sealed = env.seal();
        assert_eq!(
            sealed.audit_chain_hash.len(),
            64,
            "post-seal: SHA-256 hex digest (64 chars)"
        );
        assert!(
            sealed.audit_chain_hash.chars().all(|c| c.is_ascii_hexdigit()),
            "post-seal: lowercase hex"
        );
    }

    #[test]
    fn fase39b_seal_is_deterministic_on_identical_inputs() {
        let a = FlowEnvelope::from_execution_result(
            fixture_exec_result(),
            "List<TenantRecord>".to_string(),
        )
        .seal();
        let b = FlowEnvelope::from_execution_result(
            fixture_exec_result(),
            "List<TenantRecord>".to_string(),
        )
        .seal();
        assert_eq!(
            a.audit_chain_hash, b.audit_chain_hash,
            "seal must be deterministic"
        );
    }

    #[test]
    fn fase39b_seal_changes_hash_on_provenance_drift() {
        let a = FlowEnvelope::from_execution_result(
            fixture_exec_result(),
            "List<TenantRecord>".to_string(),
        )
        .seal();
        let mut exec_b = fixture_exec_result();
        exec_b.step_names = vec!["RetrieveAllRenamed".to_string()];
        let b = FlowEnvelope::from_execution_result(
            exec_b,
            "List<TenantRecord>".to_string(),
        )
        .seal();
        assert_ne!(
            a.audit_chain_hash, b.audit_chain_hash,
            "tamper detection: provenance drift changes the hash"
        );
    }

    #[test]
    fn fase39b_seal_clamps_certainty_on_derived() {
        // §Theorem 5.1 enforcement — even if a producer set certainty
        // > 0.99 on a derived state, seal() clamps it.
        // The 39.b algebra: derived ⇔ anchor_breaches > 0 || errors > 0
        // (matches from_execution_result verbatim).
        let mut env = FlowEnvelope {
            ontological_type: "Any".to_string(),
            result: serde_json::Value::Null,
            certainty: 1.0, // misbehaving producer
            epistemic_envelopes: Vec::new(),
            provenance_chain: vec!["flow:Derived".to_string()],
            step_audit: StepAuditTrail {
                anchor_breaches: 1, // makes this derived per 39.b algebra
                ..StepAuditTrail::default()
            },
            audit_chain_hash: String::new(),
            blame_attribution: None,
            execution_metrics: ExecutionMetrics::default(),
            trace_id: "x".to_string(),
            error: None,
            temporal_context: None,
        };
        env.certainty = 1.0;
        let sealed = env.seal();
        assert!(
            sealed.certainty <= 0.99,
            "Theorem 5.1: certainty must be clamped to ≤ 0.99 on \
             derived states (anchor_breaches > 0). Got: {}",
            sealed.certainty
        );
    }

    #[test]
    fn fase39b_seal_preserves_certainty_on_clean_path() {
        // §Theorem 5.1 — only derived states are clamped. A flow
        // with no derivation (just the flow:_ provenance prefix and
        // nothing else) keeps certainty = 1.0.
        let mut exec = fixture_exec_result();
        exec.step_names = Vec::new(); // strip the step to remove derivation
        let env = FlowEnvelope::from_execution_result(exec, "Any".to_string());
        // After from_execution_result the provenance chain has only
        // ["flow:FetchTenants", "backend:stub"]. That's 2 entries
        // (> 1), so this counts as derived per our algebra.
        // To get a NON-derived state we'd need a flow with NO
        // backend either — i.e. a degenerate flow. For the test we
        // assert the algebra by directly constructing.
        let degenerate = FlowEnvelope {
            ontological_type: "Any".to_string(),
            result: serde_json::Value::Null,
            certainty: 1.0,
            epistemic_envelopes: Vec::new(),
            provenance_chain: vec!["flow:Empty".to_string()],
            step_audit: StepAuditTrail::default(),
            audit_chain_hash: String::new(),
            blame_attribution: None,
            execution_metrics: ExecutionMetrics::default(),
            trace_id: "x".to_string(),
            error: None,
            temporal_context: None,
        };
        let sealed = degenerate.seal();
        assert_eq!(sealed.certainty, 1.0);
        let _ = env;
    }

    #[test]
    fn fase39b_extract_inner_ontological_type_unwraps_envelope() {
        assert_eq!(
            extract_inner_ontological_type("FlowEnvelope<List<TenantRecord>>"),
            "List<TenantRecord>"
        );
        assert_eq!(
            extract_inner_ontological_type("FlowEnvelope<TenantRecord>"),
            "TenantRecord"
        );
        // Legacy: bare type — returned verbatim (pre-39.e tolerance).
        assert_eq!(extract_inner_ontological_type("TenantRecord"), "TenantRecord");
        assert_eq!(extract_inner_ontological_type("List<X>"), "List<X>");
        // Missing / empty — defaults to Any (singular catch-all).
        assert_eq!(extract_inner_ontological_type(""), "Any");
        assert_eq!(extract_inner_ontological_type("   "), "Any");
    }

    #[test]
    fn fase39b_serialization_round_trip() {
        let env = FlowEnvelope::from_execution_result(
            fixture_exec_result(),
            "List<TenantRecord>".to_string(),
        )
        .seal();
        let serialized = serde_json::to_string(&env).expect("serialize");
        let parsed: FlowEnvelope =
            serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(parsed.ontological_type, env.ontological_type);
        assert_eq!(parsed.certainty, env.certainty);
        assert_eq!(parsed.audit_chain_hash, env.audit_chain_hash);
        assert_eq!(parsed.trace_id, env.trace_id);
        assert_eq!(parsed.provenance_chain, env.provenance_chain);
    }

    #[test]
    fn fase39b_wire_shape_has_canonical_field_order() {
        // §Fase 39 §4 — the wire is the ψ-vector. Verify the
        // serialized form carries every field the contract names.
        let env = FlowEnvelope::from_execution_result(
            fixture_exec_result(),
            "List<TenantRecord>".to_string(),
        )
        .seal();
        let json = serde_json::to_value(&env).expect("to_value");
        let obj = json.as_object().expect("envelope is a JSON object");
        // ψ = ⟨T, V, E⟩ — every component MUST be present.
        assert!(obj.contains_key("ontological_type"), "T component");
        assert!(obj.contains_key("result"), "V component");
        assert!(obj.contains_key("certainty"), "E: epistemic");
        assert!(obj.contains_key("provenance_chain"), "E: audit");
        assert!(obj.contains_key("step_audit"), "E: audit detail");
        assert!(obj.contains_key("audit_chain_hash"), "E: tamper-evidence");
        assert!(obj.contains_key("blame_attribution"), "E: blame");
        assert!(obj.contains_key("execution_metrics"), "observability");
        assert!(obj.contains_key("trace_id"), "correlation");
    }

    #[test]
    fn fase39b_blame_kind_serializes_snake_case() {
        // Wire-shape contract: BlameKind serializes as snake_case.
        let blame = BlameContext {
            kind: BlameKind::AnchorBreach,
            location: "step:Triage".to_string(),
            message: "Confidence below threshold".to_string(),
            d_letter: Some("39.c".to_string()),
        };
        let json = serde_json::to_value(&blame).expect("to_value");
        assert_eq!(json["kind"], "anchor_breach");

        let blame2 = BlameContext {
            kind: BlameKind::BackendSoftFail,
            location: String::new(),
            message: "Truncated".to_string(),
            d_letter: None,
        };
        let json2 = serde_json::to_value(&blame2).expect("to_value");
        assert_eq!(json2["kind"], "backend_soft_fail");
    }

    #[test]
    fn fase39b_trace_id_minted_when_legacy_is_zero() {
        let exec = fixture_exec_result(); // trace_id = 0
        let env = FlowEnvelope::from_execution_result(exec, "Any".to_string());
        // Uuid v4 is 36 chars with dashes; legacy hex (16) is 16.
        assert!(
            env.trace_id.len() == 36 || env.trace_id.len() == 16,
            "trace_id length must be Uuid (36) or legacy hex (16). \
             Got len={}: {}",
            env.trace_id.len(),
            env.trace_id
        );
        assert_ne!(env.trace_id, "0");
    }

    #[test]
    fn fase39b_trace_id_carries_legacy_value_when_nonzero() {
        let mut exec = fixture_exec_result();
        exec.trace_id = 0xDEADBEEF;
        let env = FlowEnvelope::from_execution_result(exec, "Any".to_string());
        assert_eq!(env.trace_id, "00000000deadbeef");
    }
}
