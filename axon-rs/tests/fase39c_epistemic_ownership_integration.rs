//! §Fase 39.c — End-to-end integration §-assertions for the
//! epistemic field ownership cycle (39.c.x certainty / 39.c.y
//! provenance / 39.c.z blame).
//!
//! Anchors the v2.0.0 epistemic envelope contract per the plan vivo
//! `docs/fase/fase_39_pure_silicon_cognition.md` §3 + §5:
//!
//!   - §1 — C23 kernel `axon_csys_envelope_validate_degradation` is
//!     the canonical entry for Theorem 5.1 enforcement; the Rust
//!     shim re-exports a public surface; the const `THEOREM_5_1_CEILING`
//!     matches the C kernel's export (drift gate).
//!   - §2 — `FlowEnvelope::seal()` delegates to the C23 kernel
//!     (STATIC grep gate on `wire_envelope.rs`).
//!   - §3 — `ServerExecutionResult` carries `provenance_events` +
//!     `blame_attribution` slots that propagate runtime metadata to
//!     the wire envelope.
//!   - §4 — `from_execution_result` interleaves provenance_events
//!     into provenance_chain with the canonical ordering: flow →
//!     events → steps → backend.
//!   - §5 — Closed-catalog `provenance_event_for` taxonomy: 12
//!     event kinds + `None` for non-participating step types.
//!   - §6 — Closed-catalog blame priority: 5 BlameKind variants
//!     with strictly monotone ordinals; merge_blame respects
//!     priority + stable tie-break.
//!   - §7 — End-to-end converter: a flow with retrieve+shield+
//!     anchor breach produces the expected wire envelope shape.
//!
//! These §-assertions form the **structural enforcement** of the
//! 39.c epistemic ownership contract.

use std::fs;

use axon::execution_result::ServerExecutionResult;
use axon::wire_envelope::{BlameContext, BlameKind, FlowEnvelope};
use axon::wire_envelope_producers::{
    blame_for_anchor_breach, blame_for_backend_soft_fail, blame_for_shield_rejection,
    blame_for_store_breach, blame_for_type_mismatch, blame_priority, merge_blame,
    provenance_event_for,
};

// ── §1 — C23 kernel canonical for Theorem 5.1 ────────────────────────

#[test]
fn fase39c_s1_c23_kernel_canonical_for_theorem_5_1() {
    // The Rust THEOREM_5_1_CEILING const MUST match the C kernel's
    // exported constant. Any divergence indicates the C/Rust pair
    // drifted and is a structural bug.
    let from_c = axon_csys::theorem_5_1_ceiling_from_c();
    assert_eq!(
        from_c,
        axon_csys::THEOREM_5_1_CEILING,
        "§39.c §1 — Rust const ({}) MUST match C kernel export ({}); \
         divergence is a structural bug.",
        axon_csys::THEOREM_5_1_CEILING,
        from_c
    );
    assert_eq!(from_c, 0.99, "§39.c §1 — the canonical ceiling is 0.99");
}

#[test]
fn fase39c_s1_c23_clamp_on_derived_state() {
    // The C23 kernel clamps certainty > 0.99 on derived states.
    let env =
        axon_csys::EpistemicEnvelope::new(1.0, true, axon_csys::EpistemicKind::Derived);
    let clamped = axon_csys::validate_degradation(env);
    assert_eq!(
        clamped.certainty, 0.99,
        "§39.c §1 — C23 kernel clamps derived certainty to 0.99"
    );
}

#[test]
fn fase39c_s1_c23_preserves_clean_certainty() {
    let env =
        axon_csys::EpistemicEnvelope::new(1.0, false, axon_csys::EpistemicKind::Clean);
    let clamped = axon_csys::validate_degradation(env);
    assert_eq!(
        clamped.certainty, 1.0,
        "§39.c §1 — C23 kernel preserves apodictic clean certainty"
    );
}

#[test]
fn fase39c_s1_c23_defensive_normalisation() {
    // NaN / Inf / negative values coerced to 0.0 — defensive ingress
    // hardening at the FFI boundary.
    let nan =
        axon_csys::EpistemicEnvelope::new(f64::NAN, true, axon_csys::EpistemicKind::Derived);
    assert_eq!(axon_csys::validate_degradation(nan).certainty, 0.0);
    let inf =
        axon_csys::EpistemicEnvelope::new(f64::INFINITY, false, axon_csys::EpistemicKind::Clean);
    assert_eq!(axon_csys::validate_degradation(inf).certainty, 0.0);
    let neg =
        axon_csys::EpistemicEnvelope::new(-1.0, false, axon_csys::EpistemicKind::Clean);
    assert_eq!(axon_csys::validate_degradation(neg).certainty, 0.0);
}

// ── §2 — FlowEnvelope::seal delegates to C23 ─────────────────────────

#[test]
fn fase39c_s2_seal_delegates_to_c23_kernel() {
    // Grep gate on wire_envelope.rs — the seal() invariant MUST
    // call into axon_csys::validate_degradation. Any future PR
    // that reverts to the Rust-fallback breaks this assertion.
    let src = fs::read_to_string("src/wire_envelope.rs")
        .expect("read wire_envelope.rs");
    assert!(
        src.contains("axon_csys::validate_degradation"),
        "§39.c §2 — FlowEnvelope::seal MUST delegate to the C23 \
         kernel via axon_csys::validate_degradation. The Rust-side \
         fallback algebra is the v1.x predecessor and is structurally \
         retired in 39.c.x."
    );
    assert!(
        src.contains("axon_csys::EpistemicEnvelope::new"),
        "§39.c §2 — FlowEnvelope::seal MUST build the canonical \
         EpistemicEnvelope wrapper before calling the C23 kernel."
    );
}

// ── §3 — ServerExecutionResult carries 39.c.y + 39.c.z fields ─────

// §Fase 118.b.2 — these two gates follow their SUBJECT, not its old address.
// `ServerExecutionResult` moved from `axon_server.rs` to `execution_result.rs`
// (a leaf module with no dependencies) because the flow dispatcher and the wire
// envelope — the core execution path — could not name their own output type
// without dragging in 29,734 lines of `axum` router. The §39.c contract is
// unchanged; only the file that must contain the fields moved.
//
// Worth noting what this catch cost and what it proves: a source-text gate is
// exactly the kind of test that goes quietly wrong after a refactor, because it
// asserts about a FILE rather than about a TYPE. It did its job — it failed loudly
// the moment the field left the file it was watching — but the reason it can be
// updated safely here is that the field genuinely still exists, and the
// `fase39b`/`fase39c` compile-time uses of `ServerExecutionResult` (repointed to
// `axon::execution_result::…`) are what actually prove it.

#[test]
fn fase39c_s3_server_execution_result_carries_provenance_events() {
    let src = fs::read_to_string("src/execution_result.rs")
        .expect("read execution_result.rs");
    assert!(
        src.contains("pub provenance_events: Vec<String>"),
        "§39.c §3 — `ServerExecutionResult.provenance_events: Vec<String>` \
         MUST be present so the runtime's provenance walk surfaces on the wire."
    );
}

#[test]
fn fase39c_s3_server_execution_result_carries_blame_attribution() {
    let src = fs::read_to_string("src/execution_result.rs")
        .expect("read execution_result.rs");
    assert!(
        src.contains(
            "pub blame_attribution: Option<crate::wire_envelope::BlameContext>"
        ),
        "§39.c §3 — `ServerExecutionResult.blame_attribution` MUST be \
         present so the runtime's blame surfacing reaches the wire."
    );
}

// ── §4 — Provenance chain ordering: flow → events → steps → backend ──

#[test]
fn fase39c_s4_provenance_chain_canonical_ordering() {
    let exec = ServerExecutionResult {
        success: true,
        flow_name: "FetchTenants".to_string(),
        source_file: "tenants.axon".to_string(),
        backend: "stub".to_string(),
        steps_executed: 2,
        latency_ms: 50,
        tokens_input: 0,
        tokens_output: 0,
        anchor_checks: 0,
        anchor_breaches: 0,
        errors: 0,
        step_names: vec!["Plan".to_string(), "Decide".to_string()],
        step_results: vec!["[]".to_string(), "[]".to_string()],
        trace_id: 0,
        effect_policies: Vec::new(),
        enforcement_summaries: Vec::new(),
        runtime_warnings: Vec::new(),
        provenance_events: vec![
            "retrieve:tenants".to_string(),
            "shield:Hipaa".to_string(),
        ],
        blame_attribution: None,
        epistemic_envelopes: Vec::new(),
        error: None,
        temporal_context: None,
    };
    let env = FlowEnvelope::from_execution_result(exec, "List<Any>".to_string());
    assert_eq!(
        env.provenance_chain,
        vec![
            "flow:FetchTenants",
            "retrieve:tenants",
            "shield:Hipaa",
            "step:Plan",
            "step:Decide",
            "backend:stub",
        ],
        "§39.c §4 — provenance chain ordering MUST be: \
         flow → semantic events → canonical steps → backend"
    );
}

// ── §5 — Closed-catalog provenance taxonomy ─────────────────────────

#[test]
fn fase39c_s5_provenance_taxonomy_closed_catalog() {
    // The closed taxonomy at v2.0.0: 12 semantic kinds + None for
    // non-participating step types. Any future addition requires
    // an explicit plan-vivo sub-fase.
    let cases = [
        ("retrieve", "tenants", Some("retrieve:tenants")),
        ("persist", "audit_log", Some("persist:audit_log")),
        ("mutate", "tx", Some("mutate:tx")),
        ("purge", "expired", Some("purge:expired")),
        ("shield_apply", "Hipaa", Some("shield:Hipaa")),
        ("ots_apply", "resample", Some("ots:resample")),
        ("mandate_apply", "Gdpr", Some("mandate:Gdpr")),
        ("compute_apply", "gpu", Some("compute:gpu")),
        ("lambda_data_apply", "psi", Some("lambda_apply:psi")),
        ("use_tool", "search", Some("tool:search")),
        ("remember", "Persist", Some("memory:remember@Persist")),
        ("recall", "Lookup", Some("memory:recall@Lookup")),
        // Non-participating kinds:
        ("step", "Triage", None),
        ("reason", "Analyze", None),
        ("validate", "Check", None),
        ("refine", "Improve", None),
        ("weave", "Combine", None),
        ("let_binding", "x", None),
        ("return", "", None),
        ("break", "", None),
        ("continue", "", None),
        // Unknown future kinds default to None (closed catalog
        // discipline — future fases must extend explicitly).
        ("future_kind", "x", None),
    ];
    for (stype, sname, expected) in cases {
        let got = provenance_event_for(stype, sname);
        match expected {
            Some(want) => assert_eq!(
                got.as_deref(),
                Some(want),
                "§39.c §5 — step_type={stype:?} name={sname:?} \
                 must emit {want:?}"
            ),
            None => assert_eq!(
                got, None,
                "§39.c §5 — step_type={stype:?} must NOT emit a provenance entry"
            ),
        }
    }
}

// ── §6 — Closed-catalog blame priority ──────────────────────────────

#[test]
fn fase39c_s6_blame_priority_is_strictly_monotone() {
    // The closed catalog of 5 BlameKind variants has strict
    // monotonic priority ordinals — no two variants tie. This is
    // the structural enforcement that prevents drift when future
    // sub-fases extend the catalog (each new variant MUST insert
    // at a unique priority).
    let p_anchor = blame_priority(&BlameKind::AnchorBreach);
    let p_shield = blame_priority(&BlameKind::ShieldRejection);
    let p_store = blame_priority(&BlameKind::StoreBreach);
    let p_backend = blame_priority(&BlameKind::BackendSoftFail);
    let p_typemis = blame_priority(&BlameKind::TypeMismatch);
    assert!(p_anchor < p_shield);
    assert!(p_shield < p_store);
    assert!(p_store < p_backend);
    assert!(p_backend < p_typemis);
    // Anchor highest, type mismatch lowest.
    assert_eq!(p_anchor, 0);
    assert_eq!(p_typemis, 4);
}

#[test]
fn fase39c_s6_merge_blame_respects_priority_chain() {
    // Build all 5 producer types and verify the priority order
    // wins through chained merges.
    let blames = [
        blame_for_type_mismatch("f", "I", "S"),
        blame_for_backend_soft_fail("be", "r", None),
        blame_for_store_breach("st", "seg", None),
        blame_for_shield_rejection("sh", "S", "p"),
        blame_for_anchor_breach("S", "A", "warn", 0.5),
    ];
    let mut acc: Option<BlameContext> = None;
    for b in &blames {
        acc = merge_blame(acc, Some(b.clone()));
    }
    assert_eq!(
        acc.unwrap().kind,
        BlameKind::AnchorBreach,
        "§39.c §6 — chained merges MUST end with the highest-priority \
         (anchor breach) blame"
    );
}

// ── §7 — End-to-end converter integration ───────────────────────────

#[test]
fn fase39c_s7_e2e_converter_with_anchor_breach() {
    let exec = ServerExecutionResult {
        success: true,
        flow_name: "ClinicalTriage".to_string(),
        source_file: "clinical.axon".to_string(),
        backend: "anthropic".to_string(),
        steps_executed: 3,
        latency_ms: 250,
        tokens_input: 100,
        tokens_output: 50,
        anchor_checks: 2,
        anchor_breaches: 1,
        errors: 0,
        step_names: vec![
            "Triage".to_string(),
            "Decide".to_string(),
            "AuditLog".to_string(),
        ],
        step_results: vec![
            r#"{"severity":"high"}"#.to_string(),
            r#"{"action":"escalate"}"#.to_string(),
            "ok".to_string(),
        ],
        trace_id: 0,
        effect_policies: Vec::new(),
        enforcement_summaries: Vec::new(),
        runtime_warnings: Vec::new(),
        provenance_events: vec![
            "retrieve:patient_history".to_string(),
            "shield:Hipaa".to_string(),
            "persist:audit_log".to_string(),
        ],
        blame_attribution: Some(blame_for_anchor_breach(
            "Triage",
            "ConfidenceFloor",
            "warn",
            0.42,
        )),
        epistemic_envelopes: Vec::new(),
        error: None,
        temporal_context: None,
    };
    let env = FlowEnvelope::from_execution_result(
        exec,
        "FlowEnvelope<TriageDecision>".to_string(),
    )
    .seal();

    // Wire shape — every component present.
    assert_eq!(env.ontological_type, "FlowEnvelope<TriageDecision>");
    // Derived state (anchor_breaches > 0) → certainty clamped by C23.
    assert!(
        env.certainty <= 0.99,
        "§39.c §7 — derived state MUST be clamped by C23 kernel; got {}",
        env.certainty
    );
    // Provenance chain has the canonical ordering.
    assert_eq!(env.provenance_chain[0], "flow:ClinicalTriage");
    assert!(
        env.provenance_chain.contains(&"retrieve:patient_history".to_string())
    );
    assert!(env.provenance_chain.contains(&"shield:Hipaa".to_string()));
    assert!(env.provenance_chain.contains(&"persist:audit_log".to_string()));
    assert!(env.provenance_chain.contains(&"step:Triage".to_string()));
    assert!(env.provenance_chain.contains(&"step:Decide".to_string()));
    assert!(env.provenance_chain.contains(&"step:AuditLog".to_string()));
    assert_eq!(
        env.provenance_chain.last().unwrap(),
        "backend:anthropic"
    );
    // Blame attribution carried through.
    let blame = env.blame_attribution.expect("blame surfaces");
    assert_eq!(blame.kind, BlameKind::AnchorBreach);
    assert_eq!(blame.location, "step:Triage");
    assert!(blame.message.contains("ConfidenceFloor"));
    // audit_chain_hash populated.
    assert_eq!(env.audit_chain_hash.len(), 64);
    // Last typed step result is the "result" slot — JSON-parsed.
    assert_eq!(env.result, serde_json::Value::String("ok".to_string()));
}

#[test]
fn fase39c_s7_e2e_clean_path_no_blame() {
    let exec = ServerExecutionResult {
        success: true,
        flow_name: "Healthcheck".to_string(),
        source_file: "health.axon".to_string(),
        backend: "stub".to_string(),
        steps_executed: 1,
        latency_ms: 5,
        tokens_input: 0,
        tokens_output: 0,
        anchor_checks: 0,
        anchor_breaches: 0,
        errors: 0,
        step_names: vec!["Ping".to_string()],
        step_results: vec![r#""ok""#.to_string()],
        trace_id: 0,
        effect_policies: Vec::new(),
        enforcement_summaries: Vec::new(),
        runtime_warnings: Vec::new(),
        provenance_events: Vec::new(),
        blame_attribution: None,
        epistemic_envelopes: Vec::new(),
        error: None,
        temporal_context: None,
    };
    let env = FlowEnvelope::from_execution_result(exec, "Any".to_string()).seal();
    assert_eq!(env.certainty, 1.0, "§39.c §7 — clean path → 1.0");
    assert!(env.blame_attribution.is_none(), "§39.c §7 — clean path → no blame");
}

// ── §8 — wire_envelope_producers module surface present ──

#[test]
fn fase39c_s8_producers_module_surface_complete() {
    let src = fs::read_to_string("src/wire_envelope_producers.rs")
        .expect("read wire_envelope_producers.rs");
    let required_producers = [
        "blame_for_anchor_breach",
        "blame_for_shield_rejection",
        "blame_for_store_breach",
        "blame_for_backend_soft_fail",
        "blame_for_type_mismatch",
    ];
    for producer in required_producers {
        assert!(
            src.contains(&format!("pub fn {producer}")),
            "§39.c §8 — `pub fn {producer}` MUST be declared (closed \
             catalog of 5 BlameKind producers)"
        );
    }
    assert!(
        src.contains("pub fn merge_blame"),
        "§39.c §8 — `merge_blame` MUST be declared (priority-aware coalesce)"
    );
    assert!(
        src.contains("pub fn blame_priority"),
        "§39.c §8 — `blame_priority` MUST be declared (closed-catalog priority)"
    );
    assert!(
        src.contains("pub fn provenance_event_for"),
        "§39.c §8 — `provenance_event_for` MUST be declared (taxonomy entry)"
    );
    assert!(
        src.contains("pub fn collect_provenance_events_from"),
        "§39.c §8 — `collect_provenance_events_from` MUST be declared (walk helper)"
    );
    assert!(
        src.contains("pub fn derive_blame_from_report"),
        "§39.c §8 — `derive_blame_from_report` MUST be declared (report walk)"
    );
}

// ── §9 — C23 kernel source files present ──

#[test]
fn fase39c_s9_c23_kernel_source_files_present() {
    assert!(
        std::path::Path::new("../axon-csys/c-src/effects/envelope.c").exists(),
        "§39.c §9 — C23 kernel implementation MUST exist at \
         axon-csys/c-src/effects/envelope.c"
    );
    assert!(
        std::path::Path::new("../axon-csys/c-src/effects/envelope.h").exists(),
        "§39.c §9 — C23 kernel header MUST exist at \
         axon-csys/c-src/effects/envelope.h"
    );
    assert!(
        std::path::Path::new("../axon-csys/src/envelope.rs").exists(),
        "§39.c §9 — Rust shim MUST exist at axon-csys/src/envelope.rs"
    );
}
