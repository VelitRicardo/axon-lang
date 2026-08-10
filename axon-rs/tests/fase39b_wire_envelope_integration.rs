//! §Fase 39.b — End-to-end integration §-assertions for the
//! `FlowEnvelope<T>` wire envelope.
//!
//! These tests anchor the v2.0.0 wire contract per the plan vivo
//! `docs/fase/fase_39_pure_silicon_cognition.md`:
//!
//!  - §1 STATIC grep gate: every public-facing `serde_json::to_value`
//!    of a `ServerExecutionResult` MUST be gone from the response
//!    path (only legitimate non-wire callers stay).
//!  - §2 the `axon-rs/src/wire_envelope.rs` module exists with the
//!    canonical FlowEnvelope/StepAuditTrail/ExecutionMetrics/
//!    BlameContext/BlameKind surface declared.
//!  - §3 `ExecuteRequest` carries `declared_output_type` (the field
//!    that propagates the endpoint declaration to the wire wrapper).
//!  - §4 `extract_inner_ontological_type` round-trips correctly for
//!    every form the adopter can write.
//!  - §5 wire-shape contract: a sealed `FlowEnvelope` JSON object
//!    has every canonical field of the ψ-vector ⟨T, V, E⟩ and the
//!    Theorem 5.1 bound is honored.
//!  - §6 audit_chain_hash deterministic + tamper-evident.
//!  - §7 D5 unwrap policy: validation runs against the inner result
//!    slot, not the envelope outer shape.
//!
//! These §-assertions form the **structural enforcement** of the
//! Fase 39 wire contract — any future PR that regresses one of them
//! (e.g. unwraps the FlowEnvelope back to flat JSON) turns this test
//! RED before merge.

use std::fs;

use axon::wire_envelope::{
    extract_inner_ontological_type, BlameContext, BlameKind, ExecutionMetrics,
    FlowEnvelope, StepAuditTrail,
};

// ── §1 — STATIC grep: response path no longer emits flat ServerExecutionResult ──

#[test]
fn fase39b_s1_no_flat_server_execution_result_in_response_path() {
    // The legacy v1.x flat envelope shape — `Json(serde_json::to_value(&exec_result))`
    // — MUST be gone from the success path. The replacement is
    // `Json(serde_json::to_value(&envelope))` where `envelope` is a
    // sealed `FlowEnvelope`.
    let src = fs::read_to_string("src/axon_server.rs")
        .expect("read axon_server.rs");
    assert!(
        !src.contains("Json(serde_json::to_value(&exec_result)"),
        "§Fase 39.b §1 — the legacy flat-envelope wire path \
         `Json(serde_json::to_value(&exec_result))` MUST be replaced \
         with the FlowEnvelope wrapper. Any reintroduction of the \
         flat shape breaks the v2.0.0 wire contract."
    );
    assert!(
        src.contains("FlowEnvelope::from_execution_result"),
        "§Fase 39.b §1 — `execute_handler` MUST call \
         `FlowEnvelope::from_execution_result` on the success path. \
         The wrapper is the structural enforcement of D2 (mandatory \
         wire shape)."
    );
    assert!(
        src.contains(".seal()"),
        "§Fase 39.b §1 — the `.seal()` invariant MUST be applied \
         before HTTP serialization. Seal enforces Theorem 5.1 + \
         computes the audit_chain_hash; bypassing it breaks the \
         epistemic contract."
    );
}

// ── §2 — Surface declarations present ───────────────────────────────

#[test]
fn fase39b_s2_wire_envelope_surface_present() {
    let src = fs::read_to_string("src/wire_envelope.rs")
        .expect("read wire_envelope.rs");
    assert!(
        src.contains("pub struct FlowEnvelope"),
        "§39.b §2 — `pub struct FlowEnvelope` MUST be declared"
    );
    assert!(
        src.contains("pub struct StepAuditTrail"),
        "§39.b §2 — `pub struct StepAuditTrail` MUST be declared"
    );
    assert!(
        src.contains("pub struct ExecutionMetrics"),
        "§39.b §2 — `pub struct ExecutionMetrics` MUST be declared"
    );
    assert!(
        src.contains("pub struct BlameContext"),
        "§39.b §2 — `pub struct BlameContext` MUST be declared"
    );
    assert!(
        src.contains("pub enum BlameKind"),
        "§39.b §2 — `pub enum BlameKind` (closed catalog) MUST be \
         declared"
    );
    assert!(
        src.contains("pub fn extract_inner_ontological_type"),
        "§39.b §2 — `extract_inner_ontological_type` helper MUST be \
         declared (used by the wire wrappers)"
    );
}

// ── §3 — ExecuteRequest carries declared_output_type ───────────────

#[test]
fn fase39b_s3_execute_request_carries_declared_output_type() {
    let src = fs::read_to_string("src/axon_server.rs")
        .expect("read axon_server.rs");
    assert!(
        src.contains("pub declared_output_type: String"),
        "§39.b §3 — `ExecuteRequest.declared_output_type` MUST be \
         present so the dynamic-route dispatcher can propagate the \
         endpoint's `output:` declaration through to the wire wrapper."
    );
}

// ── §4 — extract_inner_ontological_type round-trips correctly ──────

#[test]
fn fase39b_s4_extract_inner_unwraps_envelope_singular() {
    assert_eq!(
        extract_inner_ontological_type("FlowEnvelope<TenantRecord>"),
        "TenantRecord",
        "§39.b §4 — singular T must unwrap cleanly"
    );
}

#[test]
fn fase39b_s4_extract_inner_unwraps_envelope_list() {
    assert_eq!(
        extract_inner_ontological_type("FlowEnvelope<List<TenantRecord>>"),
        "List<TenantRecord>",
        "§39.b §4 — List<T> inside FlowEnvelope unwraps preserving \
         the nested generic"
    );
}

#[test]
fn fase39b_s4_extract_inner_preserves_bare_type_legacy() {
    // Pre-39.e legacy compat: bare type is preserved verbatim. After
    // 39.e the compiler error axon-E039 rejects bare declarations on
    // transport: json, but the helper itself stays permissive.
    assert_eq!(
        extract_inner_ontological_type("TenantRecord"),
        "TenantRecord"
    );
    assert_eq!(extract_inner_ontological_type("List<X>"), "List<X>");
}

#[test]
fn fase39b_s4_extract_inner_empty_defaults_to_any() {
    assert_eq!(extract_inner_ontological_type(""), "Any");
    assert_eq!(extract_inner_ontological_type("   "), "Any");
}

// ── §5 — Wire shape contract: ψ-vector serialization ────────────────

#[test]
fn fase39b_s5_sealed_envelope_carries_full_psi_vector() {
    let env = FlowEnvelope {
        ontological_type: "TenantRecord".to_string(),
        result: serde_json::json!({"id": 1, "name": "alice"}),
        certainty: 1.0,
        provenance_chain: vec!["flow:GetTenant".to_string()],
        step_audit: StepAuditTrail::default(),
        audit_chain_hash: String::new(),
        blame_attribution: None,
        epistemic_envelopes: Vec::new(),
        execution_metrics: ExecutionMetrics::default(),
        trace_id: "test-trace".to_string(),
        error: None,
        temporal_context: None,
    }
    .seal();
    let json = serde_json::to_value(&env).expect("serialize");
    let obj = json.as_object().expect("object");
    // ψ = ⟨T, V, E⟩ — every component present.
    for required in [
        "ontological_type",
        "result",
        "certainty",
        "provenance_chain",
        "step_audit",
        "audit_chain_hash",
        "blame_attribution",
        "execution_metrics",
        "trace_id",
    ] {
        assert!(
            obj.contains_key(required),
            "§39.b §5 — sealed FlowEnvelope MUST contain field \
             `{required}` (the ψ-vector contract)"
        );
    }
}

#[test]
fn fase39b_s5_theorem_5_1_bounds_derived_certainty() {
    // §Theorem 5.1 — derived states have c ≤ 0.99 in silicon.
    // The 39.b algebra: derived ⇔ anchor_breaches > 0 || errors > 0
    // (consistent between `from_execution_result` and `seal()`).
    let mut env = FlowEnvelope {
        ontological_type: "Any".to_string(),
        result: serde_json::Value::Null,
        certainty: 1.0,
        provenance_chain: vec![
            "flow:Derived".to_string(),
            "step:Reason".to_string(),
        ],
        step_audit: StepAuditTrail {
            anchor_breaches: 1, // triggers derived per 39.b algebra
            ..StepAuditTrail::default()
        },
        audit_chain_hash: String::new(),
        blame_attribution: None,
        epistemic_envelopes: Vec::new(),
        execution_metrics: ExecutionMetrics::default(),
        trace_id: "x".to_string(),
        error: None,
        temporal_context: None,
    };
    env.certainty = 0.999_999; // attempt to escape the bound
    let sealed = env.seal();
    assert!(
        sealed.certainty <= 0.99,
        "§39.b §5 — Theorem 5.1: derived states (anchor_breaches > 0 \
         OR errors > 0) MUST have certainty clamped to ≤ 0.99 by \
         seal(). Got: {}",
        sealed.certainty
    );
}

#[test]
fn fase39b_s5_blame_kind_serializes_snake_case() {
    let blame = BlameContext {
        kind: BlameKind::AnchorBreach,
        party: None,
        location: "step:Triage".to_string(),
        message: "confidence below threshold".to_string(),
        d_letter: Some("39.b".to_string()),
    };
    let json = serde_json::to_value(&blame).expect("to_value");
    assert_eq!(
        json["kind"], "anchor_breach",
        "§39.b §5 — BlameKind MUST serialize as snake_case (wire \
         shape contract — adopters parse the slug directly)"
    );
    // Check every variant for parity.
    let cases = [
        (BlameKind::AnchorBreach, "anchor_breach"),
        (BlameKind::ShieldRejection, "shield_rejection"),
        (BlameKind::BackendSoftFail, "backend_soft_fail"),
        (BlameKind::StoreBreach, "store_breach"),
        (BlameKind::TypeMismatch, "type_mismatch"),
    ];
    for (kind, slug) in cases {
        let b = BlameContext {
            kind: kind.clone(),
            // §Fase 119.m.2 — this case pins the KIND's wire slug; leaving the
            // party absent is also what proves it stays elided, keeping §39's
            // D11 wire byte-identical for a consumer that knows only `kind`.
            party: None,
            location: String::new(),
            message: String::new(),
            d_letter: None,
        };
        let j = serde_json::to_value(&b).expect("to_value");
        assert_eq!(
            j["kind"], slug,
            "§39.b §5 — BlameKind::{kind:?} MUST serialize as `{slug}`"
        );
    }
}

// ── §6 — Audit chain hash determinism + tamper-evidence ────────────

#[test]
fn fase39b_s6_audit_chain_hash_deterministic_on_identical_input() {
    let env_a = FlowEnvelope {
        ontological_type: "TenantRecord".to_string(),
        result: serde_json::json!({"id": 7}),
        certainty: 1.0,
        provenance_chain: vec!["flow:F".to_string(), "step:S".to_string()],
        step_audit: StepAuditTrail {
            step_names: vec!["S".to_string()],
            step_results: vec![serde_json::json!({"id": 7})],
            anchor_checks: 0,
            anchor_breaches: 0,
            errors: 0,
            steps_executed: 1,
            enforcement_summaries: Vec::new(),
            effect_policies: Vec::new(),
            runtime_warnings: Vec::new(),
        },
        audit_chain_hash: String::new(),
        blame_attribution: None,
        epistemic_envelopes: Vec::new(),
        execution_metrics: ExecutionMetrics::default(),
        trace_id: "t".to_string(),
        error: None,
        temporal_context: None,
    }
    .seal();
    let env_b_inputs = FlowEnvelope {
        ontological_type: "TenantRecord".to_string(),
        result: serde_json::json!({"id": 7}),
        certainty: 1.0,
        provenance_chain: vec!["flow:F".to_string(), "step:S".to_string()],
        step_audit: StepAuditTrail {
            step_names: vec!["S".to_string()],
            step_results: vec![serde_json::json!({"id": 7})],
            anchor_checks: 0,
            anchor_breaches: 0,
            errors: 0,
            steps_executed: 1,
            enforcement_summaries: Vec::new(),
            effect_policies: Vec::new(),
            runtime_warnings: Vec::new(),
        },
        audit_chain_hash: String::new(),
        blame_attribution: None,
        epistemic_envelopes: Vec::new(),
        execution_metrics: ExecutionMetrics::default(),
        trace_id: "t".to_string(),
        error: None,
        temporal_context: None,
    }
    .seal();
    assert_eq!(
        env_a.audit_chain_hash, env_b_inputs.audit_chain_hash,
        "§39.b §6 — identical (provenance_chain, step_audit) MUST \
         produce identical audit_chain_hash"
    );
    assert_eq!(
        env_a.audit_chain_hash.len(),
        64,
        "§39.b §6 — SHA-256 hex digest is exactly 64 chars"
    );
}

#[test]
fn fase39b_s6_audit_chain_hash_changes_on_provenance_tamper() {
    let base = || FlowEnvelope {
        ontological_type: "T".to_string(),
        result: serde_json::Value::Null,
        certainty: 1.0,
        provenance_chain: vec!["flow:F".to_string()],
        step_audit: StepAuditTrail::default(),
        audit_chain_hash: String::new(),
        blame_attribution: None,
        epistemic_envelopes: Vec::new(),
        execution_metrics: ExecutionMetrics::default(),
        trace_id: "t".to_string(),
        error: None,
        temporal_context: None,
    };
    let clean = base().seal();
    let mut tampered_input = base();
    tampered_input.provenance_chain.push("step:Injected".to_string());
    let tampered = tampered_input.seal();
    assert_ne!(
        clean.audit_chain_hash, tampered.audit_chain_hash,
        "§39.b §6 — any change to provenance_chain MUST change the \
         audit_chain_hash (Pillar II tamper-evidence)"
    );
}

// ── §7 — D5 unwrap policy declarative check ─────────────────────────
//
// §Fase 39.d UPDATE — the §s7 contract evolved. Pre-39.d the D5 gate
// MANUALLY called `extract_inner_ontological_type` + pulled `result`
// out of the parsed body. Post-39.d, `validate_body` is the CANONICAL
// entry that handles FlowEnvelope unwrapping internally — the gate
// just passes the raw declared type. The assertions below reflect
// this evolution: the gate must NOT manually unwrap (39.d retired
// that), and validate_body must be the call site.

#[test]
fn fase39b_s7_d5_gate_delegates_to_canonical_validate_body() {
    // §Fase 39.d — the D5 gate's wire-shape handling MUST live inside
    // `validate_body` (the canonical entry), not in the gate. The gate
    // should call `validate_body` with the raw declared type and the
    // raw response body — validate_body handles the FlowEnvelope unwrap
    // + nested generic parsing internally.
    let src = fs::read_to_string("src/axon_server.rs")
        .expect("read axon_server.rs");
    assert!(
        src.contains("crate::route_schema::validate_body(&parsed, &route.output_type, &type_table)"),
        "§39.b §7 (39.d update) — the D5 gate MUST call \
         `crate::route_schema::validate_body(&parsed, &route.output_type, &type_table)` \
         directly. Pre-39.d the gate manually unwrapped FlowEnvelope; \
         post-39.d that knowledge lives in validate_body."
    );
}

#[test]
fn fase39b_s7_d5_gate_no_longer_manually_extracts_inner_t() {
    // §Fase 39.d — the manual extract pattern that 39.b shipped is
    // RETIRED. validate_body now does the FlowEnvelope unwrap. Any
    // reintroduction of the manual extract in the gate breaks this
    // assertion.
    let src = fs::read_to_string("src/axon_server.rs")
        .expect("read axon_server.rs");
    // The active line `let inner_t = extract_inner_ontological_type(...)`
    // should be GONE. (extract_inner_ontological_type is still
    // exported as a helper — adopters / future code can use it —
    // but the D5 gate must not perform the manual unwrap itself.)
    assert!(
        !src.contains("let inner_t =\n        crate::wire_envelope::extract_inner_ontological_type"),
        "§39.b §7 (39.d update) — the 39.b manual `let inner_t = \
         extract_inner_ontological_type(...)` pattern in the D5 gate \
         MUST stay retired. validate_body handles the unwrap. \
         Reintroducing the manual extract regresses the \
         convergence dividend that 39.d shipped."
    );
}

// ── §8 — End-to-end converter from ServerExecutionResult ────────────

#[test]
fn fase39b_s8_from_execution_result_e2e() {
    // This anchors the public converter surface — when an adopter or
    // an enterprise integration consumes `FlowEnvelope::from_execution_result`
    // directly (e.g. to mock a wire shape in tests), the result MUST
    // be the canonical sealed envelope.
    let exec = axon::execution_result::ServerExecutionResult {
        success: true,
        flow_name: "FetchTenants".to_string(),
        source_file: "tenants.axon".to_string(),
        backend: "stub".to_string(),
        steps_executed: 1,
        latency_ms: 42,
        tokens_input: 0,
        tokens_output: 0,
        anchor_checks: 0,
        anchor_breaches: 0,
        errors: 0,
        step_names: vec!["RetrieveAll".to_string()],
        step_results: vec![r#"[{"id":1}]"#.to_string()],
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
    let env = FlowEnvelope::from_execution_result(
        exec,
        "List<TenantRecord>".to_string(),
    )
    .seal();
    assert_eq!(env.ontological_type, "List<TenantRecord>");
    // result is the parsed last step
    let arr = env.result.as_array().expect("result is array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], 1);
    // provenance includes flow + step + backend
    assert_eq!(
        env.provenance_chain,
        vec!["flow:FetchTenants", "step:RetrieveAll", "backend:stub"]
    );
    // audit_chain_hash is populated post-seal
    assert_eq!(env.audit_chain_hash.len(), 64);
    // clean path → certainty 1.0
    assert_eq!(env.certainty, 1.0);
}
