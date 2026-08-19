//! v2.4.0 — Proof-Carrying Code (PCC).
//!
//! From **attestation** to **machine-checkable proof**. The
//! [`crate::esk::attestation`] surface (SBOM, in-toto/SLSA provenance,
//! ComplianceDossier) emits builder-signed CLAIMS a consumer trusts.
//! PCC emits a portable [`ProofTerm`] a consumer INDEPENDENTLY verifies
//! ([`check_proof`]) — without trusting the axon compiler that produced
//! it (Necula 1997: ship code + proof; the consumer runs a small,
//! trusted, producer-independent checker).
//!
//! v2.4.0 ships the kernel + the first property class
//! ([`PropertyClass::ComplianceCoverage`]): every regulatory class an
//! apx / axonendpoint declares is known + backed by a resolvable
//! shield. v2.4.0-e generalize to effect-row soundness, capability
//! isolation, resource bounds, and shield-halt guarantees — the
//! proof-term language + checker dispatch are designed to extend
//! (the design decision — "universal" is the architecture, shipped one class at a
//! time).
//!
//! ## The loop
//!
//! ```text
//!   producer:  generate_compliance_coverage_proofs(ir, version)  ->  [ProofTerm]
//!                  (records the derivation; does NOT decide the verdict)
//!   consumer:  check_proof(&proof, ir)  ->  CheckOutcome
//!                  (re-derives from the artifact; renders Verified / Refuted;
//!                   rejects a forged witness or a digest mismatch)
//! ```
//!
//! The split is the point: the producer hands over a derivation, the
//! consumer re-checks it. Trust lives in the (small, auditable)
//! checker, not the (large, complex) compiler.

pub mod checker;
pub mod effects;
pub mod generate;
pub mod proof_term;

pub use checker::{
    check_bundle, check_call_soundness_certificate, check_proof, BundleReport, CheckOutcome,
    ProofCheck,
};
pub use generate::{
    artifact_digest, derive_capability_containment_witness, derive_capability_isolation_witness,
    derive_channel_egress_witness, derive_compliance_coverage_witness,
    derive_effect_row_soundness_witness,
    derive_endpoint_retry_witness, derive_interruptible_session_witness,
    derive_parked_residual_witness,
    derive_shield_halt_witness, derive_socket_credit_witness,
    derive_tool_call_soundness_witness, generate_all_proofs, generate_call_soundness_certificate,
    generate_capability_containment_proofs, generate_capability_isolation_proofs,
    generate_channel_egress_soundness_proofs, generate_compliance_coverage_proofs,
    // v2.44.0 — the every_boundary_is_guarded coverage obligation.
    derive_authorization_coverage_witness, generate_authorization_coverage_proofs,
    // v2.45.0 — the every_requirement_is_grantable obligation (the dual).
    derive_capability_grantability_witness, generate_capability_grantability_proofs,
    generate_effect_row_soundness_proofs,
    generate_interruptible_session_soundness_proofs,
    generate_parked_residual_soundness_proofs,
    generate_resource_bounds_proofs, generate_shield_halt_guarantee_proofs,
    generate_tool_call_soundness_proofs,
    // v2.37.0 — the outbound-vendor projection obligation.
    derive_upstream_projection_witness, generate_upstream_projection_soundness_proofs,
    // v2.38.0 — the whole-program CORS-reference/consistency obligation.
    derive_cors_policy_consistency_witness, generate_cors_policy_consistency_proofs,
    // v2.46.0 — the time_is_an_explicit_input cognitive obligation.
    derive_temporal_context_soundness_witness, generate_temporal_context_soundness_proofs,
    // v2.46.0 — the authority_only_attenuates static obligation.
    derive_credential_attenuation_witness, generate_credential_attenuation_proofs,
    // v2.48.0 — the rotation_without_revelation static obligation.
    derive_secret_custody_witness, generate_secret_custody_proofs,
    // v2.52.0 — the web-acquisition provenance obligation (born-Untrusted
    // + the content-injection barrier).
    derive_scrape_provenance_soundness_witness, generate_scrape_provenance_soundness_proofs,
    // v2.77.0 — the authorization scope-coverage obligation (axon-T956 twin).
    derive_scope_coverage_soundness_witness, generate_scope_coverage_soundness_proofs,
    derive_document_provenance_soundness_witness, generate_document_provenance_soundness_proofs,
    // v2.60.0 — the CRM-delivery provenance obligation (T920 egress barrier).
    derive_delivery_provenance_soundness_witness, generate_delivery_provenance_soundness_proofs,
    // v2.62.0 — the QUERY-safety obligation (T927; RFC 10008 section 2 made a proof).
    derive_dataspace_schema_soundness_witness, derive_gradient_soundness_witness,
    derive_query_safety_soundness_witness,
    generate_dataspace_schema_soundness_proofs, generate_gradient_soundness_proofs,
    generate_notification_provenance_soundness_proofs,
    generate_query_safety_soundness_proofs,
    derive_document_ingestion_soundness_witness, generate_document_ingestion_soundness_proofs,
    derive_inferred_ceiling_soundness_witness, generate_inferred_ceiling_soundness_proofs,
};
pub use proof_term::{
    CallSoundnessCertificate, CapabilityContainmentWitness, CapabilityGrantabilityWitness,
    CapabilityIsolationWitness,
    CredentialAttenuationWitness,
    ChannelEgressSoundnessWitness,
    ComplianceCoverageWitness,
    CorsPolicyConsistencyWitness,
    EffectRowSoundnessWitness, InterruptibleSessionSoundnessWitness, ParkedResidualSoundnessWitness,
    ProofBundle, ProofTerm,
    PropertyClass, ResourceBoundsWitness,
    ScrapeProvenanceSoundnessWitness,
    DocumentProvenanceSoundnessWitness,
    DeliveryProvenanceSoundnessWitness,
    DataspaceSchemaSoundnessWitness, GradientSoundnessWitness,
    NotificationProvenanceSoundnessWitness, QuerySafetySoundnessWitness,
    DocumentIngestionSoundnessWitness,
    InferredCeilingSoundnessWitness,
    SecretCustodySoundnessWitness,
    ShieldHaltGuaranteeWitness, TemporalContextSoundnessWitness, ToolCallSoundnessWitness,
    UpstreamProjectionSoundnessWitness,
    Witness, CALL_INTERRUPT_CAUSES,
    MAX_RETRIES, VALID_BREACH_POLICIES, VALID_SIGN_ALGORITHMS,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir_nodes::{IRAxonEndpoint, IRProgram, IRShield, IRToolSpec};

    const VERSION: &str = "2.4.0-test";

    fn empty_ir() -> IRProgram {
        IRProgram::new()
    }

    fn endpoint(name: &str, compliance: &[&str], shield_ref: &str) -> IRAxonEndpoint {
        IRAxonEndpoint {
            node_type: "endpoint",
            source_line: 1,
            source_column: 1,
            name: name.to_string(),
            method: "POST".to_string(),
            path: format!("/{name}"),
            body_type: String::new(),
            execute_flow: "F".to_string(),
            output_type: String::new(),
            shield_ref: shield_ref.to_string(),
            cors_ref: String::new(),
            retries: 0,
            timeout: String::new(),
            compliance: compliance.iter().map(|s| s.to_string()).collect(),
            path_params: Vec::new(),
            query_params: Vec::new(),
            requires_capabilities: Vec::new(),
            public: false,
        }
    }

    /// Shield providing the given regulatory classes (its `compliance:`
    /// set — the ESK covered-classes field).
    fn shield(name: &str, provides: &[&str]) -> IRShield {
        IRShield {
            node_type: "shield",
            source_line: 1,
            source_column: 1,
            name: name.to_string(),
            scan: vec!["pii_leak".to_string()],
            strategy: String::new(),
            on_breach: "halt".to_string(),
            severity: "high".to_string(),
            quarantine: String::new(),
            max_retries: 0,
            confidence_threshold: 0.0,
            allow_tools: Vec::new(),
            deny_tools: Vec::new(),
            sandbox: false,
            redact: Vec::new(),
            log: String::new(),
            deflect_message: String::new(),
            taint: String::new(),
            compliance: provides.iter().map(|s| s.to_string()).collect(),
            sign: String::new(),
        }
    }

    /// Happy path: an apx declaring known classes + a present shield
    /// generates a proof the independent checker VERIFIES.
    #[test]
    fn covered_endpoint_proof_verifies() {
        let mut ir = empty_ir();
        ir.shields.push(shield("PhiGate", &["HIPAA", "GDPR"]));
        ir.endpoints
            .push(endpoint("ChatEndpoint", &["HIPAA", "GDPR"], "PhiGate"));

        let proofs = generate_compliance_coverage_proofs(&ir, VERSION);
        assert_eq!(
            proofs.len(),
            1,
            "one compliance-bearing endpoint => one proof"
        );
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// Endpoints with no compliance declaration produce no proof
    /// (nothing to certify).
    #[test]
    fn no_compliance_no_proof() {
        let mut ir = empty_ir();
        ir.shields.push(shield("PhiGate", &["HIPAA"]));
        ir.endpoints.push(endpoint("Plain", &[], "PhiGate"));
        assert!(generate_compliance_coverage_proofs(&ir, VERSION).is_empty());
    }

    /// Honest defect: the shield resolves but does NOT provide every
    /// required class (endpoint requires HIPAA+GDPR, shield covers only
    /// HIPAA) → Refuted naming the uncovered gap. This is the core
    /// `covers()` semantic the proof certifies.
    #[test]
    fn shield_missing_required_class_is_refuted() {
        let mut ir = empty_ir();
        ir.shields.push(shield("PartialGate", &["HIPAA"]));
        ir.endpoints
            .push(endpoint("Gap", &["HIPAA", "GDPR"], "PartialGate"));
        let proofs = generate_compliance_coverage_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("does not provide"), "got: {reason}");
                assert!(reason.contains("GDPR"), "got: {reason}");
            }
            other => panic!("expected coverage-gap Refuted, got {other:?}"),
        }
    }

    /// Honest defect: compliance declared but no shield attached →
    /// the checker REFUTES (you cannot claim regulatory coverage with
    /// zero enforcement).
    #[test]
    fn compliance_without_shield_is_refuted() {
        let mut ir = empty_ir();
        ir.endpoints.push(endpoint("NoGuard", &["HIPAA"], "")); // empty shield_ref
        let proofs = generate_compliance_coverage_proofs(&ir, VERSION);
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("no resolvable shield"), "got: {reason}");
            }
            other => panic!("expected Refuted, got {other:?}"),
        }
    }

    /// Honest defect: shield_ref names a shield that isn't in the IR →
    /// dangling reference → Refuted.
    #[test]
    fn dangling_shield_ref_is_refuted() {
        let mut ir = empty_ir();
        ir.endpoints
            .push(endpoint("Dangling", &["HIPAA"], "GhostShield"));
        let proofs = generate_compliance_coverage_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("no resolvable shield"), "got: {reason}");
            }
            other => panic!("expected Refuted, got {other:?}"),
        }
    }

    /// Honest defect: phantom regulatory class (typo'd `HIPPA`) →
    /// Refuted naming the unknown class.
    #[test]
    fn phantom_regulatory_class_is_refuted() {
        let mut ir = empty_ir();
        ir.shields.push(shield("PhiGate", &["HIPAA"]));
        ir.endpoints.push(endpoint("Typo", &["HIPPA"], "PhiGate")); // HIPPA, not HIPAA
        let proofs = generate_compliance_coverage_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("unknown regulatory class"), "got: {reason}");
                assert!(reason.contains("HIPPA"), "got: {reason}");
            }
            other => panic!("expected Refuted, got {other:?}"),
        }
    }

    /// ADVERSARIAL: a forged witness claiming `shield_present:
    /// true` for an endpoint whose shield_ref is empty is REJECTED —
    /// the checker recomputes shield_present from the artifact and
    /// finds the witness lies.
    #[test]
    fn forged_shield_present_rejected() {
        let mut ir = empty_ir();
        ir.endpoints.push(endpoint("Forge", &["HIPAA"], ""));
        let mut proofs = generate_compliance_coverage_proofs(&ir, VERSION);
        // Tamper: flip shield_present to true (the lie).
        if let Witness::ComplianceCoverage(ref mut w) = proofs[0].witness {
            w.shield_present = true;
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("disagrees with artifact"), "got: {reason}");
            }
            other => panic!("expected forged-witness Refuted, got {other:?}"),
        }
    }

    /// ADVERSARIAL: a forged witness hiding a phantom class
    /// (omitting it from `unknown_classes`) is REJECTED — the checker
    /// recomputes the unknown set.
    #[test]
    fn forged_hidden_unknown_class_rejected() {
        let mut ir = empty_ir();
        ir.shields.push(shield("PhiGate", &["HIPAA"]));
        ir.endpoints
            .push(endpoint("Hide", &["NOTACLASS"], "PhiGate"));
        let mut proofs = generate_compliance_coverage_proofs(&ir, VERSION);
        // Tamper: pretend there are no unknown classes.
        if let Witness::ComplianceCoverage(ref mut w) = proofs[0].witness {
            w.unknown_classes.clear();
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("disagrees with artifact"), "got: {reason}");
            }
            other => panic!("expected forged-witness Refuted, got {other:?}"),
        }
    }

    /// ADVERSARIAL: a proof minted for program A checked
    /// against program B (different digest) is REJECTED.
    #[test]
    fn digest_mismatch_rejected() {
        let mut ir_a = empty_ir();
        ir_a.shields.push(shield("PhiGate", &["HIPAA"]));
        ir_a.endpoints
            .push(endpoint("ChatEndpoint", &["HIPAA"], "PhiGate"));
        let proofs = generate_compliance_coverage_proofs(&ir_a, VERSION);

        // Program B: same endpoint + shield, but an extra endpoint
        // changes the IR digest. Built fresh (IRProgram is not Clone).
        let mut ir_b = empty_ir();
        ir_b.shields.push(shield("PhiGate", &["HIPAA"]));
        ir_b.endpoints
            .push(endpoint("ChatEndpoint", &["HIPAA"], "PhiGate"));
        ir_b.endpoints.push(endpoint("Extra", &["GDPR"], "PhiGate"));

        assert_eq!(
            check_proof(&proofs[0], &ir_b),
            CheckOutcome::DigestMismatch,
            "a proof for program A must not verify against program B"
        );
    }

    /// ADVERSARIAL: a proof whose witness names an endpoint absent from
    /// the artifact is Refuted (not a panic). Exercises the
    /// never-panic guarantee on a stale/forged endpoint reference.
    #[test]
    fn proof_for_absent_endpoint_refuted_not_panic() {
        let mut ir = empty_ir();
        ir.shields.push(shield("PhiGate", &["HIPAA"]));
        ir.endpoints.push(endpoint("Real", &["HIPAA"], "PhiGate"));
        let mut proofs = generate_compliance_coverage_proofs(&ir, VERSION);
        // Tamper the endpoint name to one not in the IR, and re-stamp
        // the digest so we get past the digest gate to the endpoint
        // lookup (isolates the absent-endpoint path).
        if let Witness::ComplianceCoverage(ref mut w) = proofs[0].witness {
            w.endpoint_name = "Ghost".to_string();
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("not present in artifact"), "got: {reason}");
            }
            other => panic!("expected absent-endpoint Refuted, got {other:?}"),
        }
    }

    /// The proof term round-trips through JSON (it travels as JSON
    /// alongside the artifact). A round-tripped proof still verifies.
    #[test]
    fn proof_term_json_round_trips_and_verifies() {
        let mut ir = empty_ir();
        ir.shields.push(shield("PhiGate", &["HIPAA"]));
        ir.endpoints
            .push(endpoint("ChatEndpoint", &["HIPAA"], "PhiGate"));
        let proofs = generate_compliance_coverage_proofs(&ir, VERSION);
        let json = serde_json::to_string(&proofs[0]).expect("serialize");
        let restored: ProofTerm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, proofs[0]);
        assert_eq!(check_proof(&restored, &ir), CheckOutcome::Verified);
    }

    /// Witness canonicalization: declared classes in any order +
    /// duplicates produce the same (sorted, deduped) witness, so the
    /// checker's re-derivation compares equal regardless of source
    /// ordering.
    #[test]
    fn class_ordering_and_dupes_canonicalized() {
        let mut ir = empty_ir();
        ir.shields.push(shield("PhiGate", &["HIPAA", "GDPR"]));
        ir.endpoints.push(endpoint(
            "Multi",
            &["GDPR", "HIPAA", "GDPR"], // unsorted + duplicate
            "PhiGate",
        ));
        let proofs = generate_compliance_coverage_proofs(&ir, VERSION);
        if let Witness::ComplianceCoverage(ref w) = proofs[0].witness {
            assert_eq!(
                w.required_classes,
                vec!["GDPR".to_string(), "HIPAA".to_string()]
            );
        } else {
            panic!("expected ComplianceCoverage witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// v2.4.0 property-class slug is stable (wire contract).
    #[test]
    fn property_class_slug_stable() {
        assert_eq!(
            PropertyClass::ComplianceCoverage.slug(),
            "compliance_coverage"
        );
        assert_eq!(
            PropertyClass::EffectRowSoundness.slug(),
            "effect_row_soundness"
        );
        assert_eq!(
            PropertyClass::CapabilityIsolation.slug(),
            "capability_isolation"
        );
        assert_eq!(PropertyClass::ResourceBounds.slug(), "resource_bounds");
        assert_eq!(
            PropertyClass::ShieldHaltGuarantee.slug(),
            "shield_halt_guarantee"
        );
        assert_eq!(
            PropertyClass::CapabilityContainment.slug(),
            "capability_containment"
        );
        assert_eq!(
            PropertyClass::ToolCallSoundness.slug(),
            "tool_call_soundness"
        );
        assert_eq!(PropertyClass::EffectBudgeted.slug(), "effect_budgeted");
        assert_eq!(
            PropertyClass::JsonShapeSoundness.slug(),
            "json_shape_soundness"
        );
        assert_eq!(
            PropertyClass::ChannelDeliverySoundness.slug(),
            "channel_delivery_soundness"
        );
        assert_eq!(
            PropertyClass::AggregateSoundness.slug(),
            "aggregate_soundness"
        );
        assert_eq!(
            PropertyClass::ChannelEgressSoundness.slug(),
            "channel_egress_soundness"
        );
    }

    // ── v2.4.0 — EffectRowSoundness ──────────────────────────────

    /// Tool declaring the given effect-row entries.
    fn tool(name: &str, effects: &[&str]) -> IRToolSpec {
        IRToolSpec {
            node_type: "tool",
            source_line: 1,
            source_column: 1,
            name: name.to_string(),
            provider: "native".to_string(),
            max_results: None,
            filter_expr: String::new(),
            timeout: String::new(),
            runtime: String::new(),
            resource_ref: String::new(),
            sandbox: None,
            input_schema: Vec::new(),
            output_schema: String::new(),
            parameters: Vec::new(),
            output_type: None,
            requires: Vec::new(),
            secret: String::new(),
            secret_partition: String::new(),
            effect_row: effects.iter().map(|s| s.to_string()).collect(),
            target: None,
            risk: None,
            argv: Vec::new(),
            cache: String::new(),
            scrape: None,
        }
    }

    /// Happy path: a tool with well-formed effects verifies.
    #[test]
    fn well_formed_effect_row_verifies() {
        let mut ir = empty_ir();
        ir.tools.push(tool("Fetch", &["network", "storage"]));
        let proofs = generate_effect_row_soundness_proofs(&ir, VERSION);
        assert_eq!(proofs.len(), 1);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// A tool with no declared effects produces no proof.
    #[test]
    fn no_effects_no_proof() {
        let mut ir = empty_ir();
        ir.tools.push(tool("Plain", &[]));
        assert!(generate_effect_row_soundness_proofs(&ir, VERSION).is_empty());
    }

    /// Phantom base effect (typo'd `netwrok`) → Refuted.
    #[test]
    fn phantom_effect_base_refuted() {
        let mut ir = empty_ir();
        ir.tools.push(tool("Typo", &["netwrok"]));
        let proofs = generate_effect_row_soundness_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("unknown base"), "got: {reason}");
                assert!(reason.contains("netwrok"), "got: {reason}");
            }
            other => panic!("expected phantom-effect Refuted, got {other:?}"),
        }
    }

    /// Bare `stream` without a backpressure qualifier → Refuted
    /// (unenforceable).
    #[test]
    fn bare_stream_missing_qualifier_refuted() {
        let mut ir = empty_ir();
        ir.tools.push(tool("Streamer", &["stream"]));
        let proofs = generate_effect_row_soundness_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("without a qualifier"), "got: {reason}");
            }
            other => panic!("expected missing-qualifier Refuted, got {other:?}"),
        }
    }

    /// `stream:<valid policy>` verifies (qualifier from the backpressure
    /// catalog).
    #[test]
    fn valid_stream_qualifier_verifies() {
        let mut ir = empty_ir();
        ir.tools.push(tool("Streamer", &["stream:drop_oldest"]));
        let proofs = generate_effect_row_soundness_proofs(&ir, VERSION);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// `stream:<bogus>` (qualifier not in the backpressure catalog) →
    /// Refuted.
    #[test]
    fn invalid_stream_qualifier_refuted() {
        let mut ir = empty_ir();
        ir.tools.push(tool("Streamer", &["stream:explode"]));
        let proofs = generate_effect_row_soundness_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(
                    reason.contains("invalid backpressure policy"),
                    "got: {reason}"
                );
            }
            other => panic!("expected invalid-qualifier Refuted, got {other:?}"),
        }
    }

    // ── v2.5.0 — extension-declared provenance bases ─────────────

    /// Build an `effects`-category extension with the given members
    /// (no metadata).
    fn effects_ext(name: &str, members: &[&str]) -> crate::ir_nodes::IRExtension {
        crate::ir_nodes::IRExtension {
            node_type: "extension",
            source_line: 1,
            source_column: 1,
            name: name.to_string(),
            category: "effects".to_string(),
            members: members
                .iter()
                .map(|m| crate::ir_nodes::IRExtensionMember {
                    name: m.to_string(),
                    semantics: None,
                    default_confidence: None,
                })
                .collect(),
        }
    }

    /// v2.5.0 — a tool using an extension-declared provenance base
    /// VERIFIES (the proof is self-contained: the verifier re-derives
    /// the provenance set from the artifact's own `extensions`).
    #[test]
    fn extension_declared_provenance_base_verifies() {
        let mut ir = empty_ir();
        ir.extensions
            .push(effects_ext("risk_axis", &["risk:elevated"]));
        ir.tools.push(tool("Fetch", &["network", "risk:elevated"]));
        let proofs = generate_effect_row_soundness_proofs(&ir, VERSION);
        assert_eq!(proofs.len(), 1);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// v2.5.0 — a custom base that NO extension declares still REFUTES
    /// (only the declared members are honored — the catalog is not
    /// silently opened).
    #[test]
    fn undeclared_custom_base_refuted() {
        let mut ir = empty_ir();
        // extension declares `risk:elevated`, NOT `risk:guess`.
        ir.extensions
            .push(effects_ext("risk_axis", &["risk:elevated"]));
        ir.tools.push(tool("Fetch", &["risk:guess"]));
        let proofs = generate_effect_row_soundness_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("unknown base"), "got: {reason}");
                assert!(reason.contains("risk:guess"), "got: {reason}");
            }
            other => panic!("expected undeclared-base Refuted, got {other:?}"),
        }
    }

    /// v2.5.0 (invariant #2, PCC independence) — the checker's provenance
    /// set EXCLUDES a member whose base is a canonical enforceable base.
    /// PCC enforces this itself; it does not trust that the type-checker
    /// ran. (`io:bypass` is excluded; `risk:elevated` is included.)
    #[test]
    fn pcc_provenance_set_excludes_canonical_shadow() {
        let mut ir = empty_ir();
        ir.extensions
            .push(effects_ext("mixed", &["io:bypass_shield", "risk:elevated"]));
        let set = super::generate::extension_effect_members(&ir);
        assert!(
            !set.contains("io:bypass_shield"),
            "a member shadowing the canonical enforceable base `io` must NOT \
             become a provenance member (invariant #2)"
        );
        assert!(
            set.contains("risk:elevated"),
            "a genuinely custom base must be a provenance member"
        );
    }

    // ── v2.5.0 — built-in `epistemic:<level>` provenance axis ──

    /// v2.5.0 — a tool declaring the built-in `epistemic:<level>` axis
    /// VERIFIES (no `extension` needed). This is the Kivi brief #15 case:
    /// pre-v2.5.0 the IR re-injected `epistemic:believe` into the effect
    /// row and PCC refuted it as an unknown base, forcing the strip.
    #[test]
    fn builtin_epistemic_level_verifies() {
        for level in ["believe", "doubt", "know", "speculate"] {
            let mut ir = empty_ir();
            ir.tools
                .push(tool("Fetch", &["network", &format!("epistemic:{level}")]));
            let proofs = generate_effect_row_soundness_proofs(&ir, VERSION);
            assert_eq!(proofs.len(), 1);
            assert_eq!(
                check_proof(&proofs[0], &ir),
                CheckOutcome::Verified,
                "epistemic:{level} must verify (built-in provenance axis)"
            );
        }
    }

    /// v2.5.0 — an `epistemic:<bogus>` level (not in the closed catalog)
    /// still REFUTES (the axis is closed, not a wildcard prefix).
    #[test]
    fn unknown_epistemic_level_refuted() {
        let mut ir = empty_ir();
        ir.tools.push(tool("X", &["epistemic:guess"]));
        let proofs = generate_effect_row_soundness_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("unknown base"), "got: {reason}");
                assert!(reason.contains("epistemic:guess"), "got: {reason}");
            }
            other => panic!("expected unknown-epistemic-level Refuted, got {other:?}"),
        }
    }

    /// `pure` alongside another effect → purity contradiction → Refuted.
    #[test]
    fn purity_violation_refuted() {
        let mut ir = empty_ir();
        ir.tools.push(tool("FakePure", &["pure", "network"]));
        let proofs = generate_effect_row_soundness_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("pure"), "got: {reason}");
                assert!(reason.contains("cannot be effectful"), "got: {reason}");
            }
            other => panic!("expected purity Refuted, got {other:?}"),
        }
    }

    /// `pure` alone verifies.
    #[test]
    fn pure_alone_verifies() {
        let mut ir = empty_ir();
        ir.tools.push(tool("TrulyPure", &["pure"]));
        let proofs = generate_effect_row_soundness_proofs(&ir, VERSION);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// ADVERSARIAL: a forged witness hiding a phantom base
    /// (clearing `unknown_bases`) is rejected — the checker recomputes.
    #[test]
    fn forged_hidden_unknown_base_rejected() {
        let mut ir = empty_ir();
        ir.tools.push(tool("Sneaky", &["netwrok"]));
        let mut proofs = generate_effect_row_soundness_proofs(&ir, VERSION);
        if let Witness::EffectRowSoundness(ref mut w) = proofs[0].witness {
            w.unknown_bases.clear(); // the lie
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("disagrees with artifact"), "got: {reason}");
            }
            other => panic!("expected forged Refuted, got {other:?}"),
        }
    }

    /// ADVERSARIAL: a proof whose `property` and `witness` variants
    /// disagree (ComplianceCoverage property carrying an
    /// EffectRowSoundness witness) is UnknownProperty — neither
    /// property is actually witnessed, so no silent accept.
    #[test]
    fn mismatched_property_witness_is_unknown_property() {
        let mut ir = empty_ir();
        ir.tools.push(tool("Fetch", &["network"]));
        let digest = artifact_digest(&ir);
        let bogus = ProofTerm {
            property: PropertyClass::ComplianceCoverage, // mismatch
            artifact_digest: digest,
            witness: Witness::EffectRowSoundness(derive_effect_row_soundness_witness(
                "Fetch",
                &["network".to_string()],
                &std::collections::HashSet::new(),
            )),
            axon_version: VERSION.to_string(),
        };
        assert_eq!(check_proof(&bogus, &ir), CheckOutcome::UnknownProperty);
    }

    /// Effect-row proof round-trips through JSON and still verifies.
    #[test]
    fn effect_proof_json_round_trips_and_verifies() {
        let mut ir = empty_ir();
        ir.tools.push(tool("Fetch", &["network", "storage"]));
        let proofs = generate_effect_row_soundness_proofs(&ir, VERSION);
        let json = serde_json::to_string(&proofs[0]).expect("serialize");
        let restored: ProofTerm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, proofs[0]);
        assert_eq!(check_proof(&restored, &ir), CheckOutcome::Verified);
    }

    /// Digest binding holds for effect-row proofs too.
    #[test]
    fn effect_proof_digest_mismatch_rejected() {
        let mut ir_a = empty_ir();
        ir_a.tools.push(tool("Fetch", &["network"]));
        let proofs = generate_effect_row_soundness_proofs(&ir_a, VERSION);

        let mut ir_b = empty_ir();
        ir_b.tools.push(tool("Fetch", &["network"]));
        ir_b.tools.push(tool("Other", &["storage"])); // changes digest

        assert_eq!(check_proof(&proofs[0], &ir_b), CheckOutcome::DigestMismatch);
    }

    // ── v2.4.0 — CapabilityIsolation ─────────────────────────────

    /// `axonstore` with the given Pillar IV capability gate slug.
    fn store(name: &str, capability: &str) -> crate::ir_nodes::IRAxonStore {
        crate::ir_nodes::IRAxonStore {
            node_type: "axonstore",
            source_line: 1,
            source_column: 1,
            name: name.to_string(),
            backend: "postgresql".to_string(),
            connection: String::new(),
            confidence_floor: None,
            isolation: String::new(),
            on_breach: String::new(),
            capability: capability.to_string(),
            class: String::new(),
            column_schema: None,
            resource_ref: String::new(),
        }
    }

    /// Happy path: a well-formed v1.23.0 gate slug verifies.
    #[test]
    fn well_formed_gate_verifies() {
        let mut ir = empty_ir();
        ir.axonstore_specs.push(store("Ledger", "legal.read"));
        let proofs = generate_capability_isolation_proofs(&ir, VERSION);
        assert_eq!(proofs.len(), 1);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// A store with no capability gate produces no proof.
    #[test]
    fn no_gate_no_proof() {
        let mut ir = empty_ir();
        ir.axonstore_specs.push(store("Open", ""));
        assert!(generate_capability_isolation_proofs(&ir, VERSION).is_empty());
    }

    /// A malformed gate slug (uppercase violates the v1.23.0 grammar) →
    /// Refuted.
    #[test]
    fn malformed_gate_refuted() {
        let mut ir = empty_ir();
        ir.axonstore_specs.push(store("Broken", "Legal.Read")); // uppercase
        let proofs = generate_capability_isolation_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(
                    reason.contains("malformed capability gate"),
                    "got: {reason}"
                );
                assert!(reason.contains("Legal.Read"), "got: {reason}");
            }
            other => panic!("expected malformed-gate Refuted, got {other:?}"),
        }
    }

    /// Dotted multi-segment scope (`hipaa.phi.read`) is well-formed.
    #[test]
    fn deep_dotted_gate_verifies() {
        let mut ir = empty_ir();
        ir.axonstore_specs.push(store("Phi", "hipaa.phi.read"));
        let proofs = generate_capability_isolation_proofs(&ir, VERSION);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// ADVERSARIAL: a forged witness claiming `malformed:
    /// false` for a broken gate is rejected — the checker re-runs the
    /// grammar validator and finds the witness lies.
    #[test]
    fn forged_malformed_flag_rejected() {
        let mut ir = empty_ir();
        ir.axonstore_specs.push(store("Sneaky", "Bad..Slug"));
        let mut proofs = generate_capability_isolation_proofs(&ir, VERSION);
        if let Witness::CapabilityIsolation(ref mut w) = proofs[0].witness {
            w.malformed = false; // the lie
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("disagrees with artifact"), "got: {reason}");
            }
            other => panic!("expected forged Refuted, got {other:?}"),
        }
    }

    /// Digest binding holds for capability proofs.
    #[test]
    fn capability_proof_digest_mismatch_rejected() {
        let mut ir_a = empty_ir();
        ir_a.axonstore_specs.push(store("Ledger", "legal.read"));
        let proofs = generate_capability_isolation_proofs(&ir_a, VERSION);

        let mut ir_b = empty_ir();
        ir_b.axonstore_specs.push(store("Ledger", "legal.read"));
        ir_b.axonstore_specs.push(store("Extra", "fin.read")); // changes digest

        assert_eq!(check_proof(&proofs[0], &ir_b), CheckOutcome::DigestMismatch);
    }

    /// Capability proof round-trips through JSON and still verifies.
    #[test]
    fn capability_proof_json_round_trips_and_verifies() {
        let mut ir = empty_ir();
        ir.axonstore_specs.push(store("Ledger", "legal.read"));
        let proofs = generate_capability_isolation_proofs(&ir, VERSION);
        let json = serde_json::to_string(&proofs[0]).expect("serialize");
        let restored: ProofTerm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, proofs[0]);
        assert_eq!(check_proof(&restored, &ir), CheckOutcome::Verified);
    }

    // ── v2.4.0 — ResourceBounds ──────────────────────────────────

    /// An endpoint with the given `retries` (reuses the base builder).
    fn endpoint_retries(name: &str, retries: i64) -> IRAxonEndpoint {
        let mut e = endpoint(name, &[], "");
        e.retries = retries;
        e
    }

    /// A socket with the given (optional) backpressure credit window.
    fn socket(name: &str, credit: Option<i64>) -> crate::ir_nodes::IRSocket {
        crate::ir_nodes::IRSocket {
            node_type: "socket",
            source_line: 1,
            source_column: 1,
            name: name.to_string(),
            protocol: "ChatProtocol".to_string(),
            backpressure_credit: credit,
            reconnect: false,
            legal_basis: None,
        }
    }

    /// Retry counts within `[0, MAX_RETRIES]` verify — including both
    /// boundaries (0 and the ceiling).
    #[test]
    fn retry_in_bounds_verifies() {
        let mut ir = empty_ir();
        ir.endpoints.push(endpoint_retries("Mid", 3));
        ir.endpoints.push(endpoint_retries("Zero", 0));
        ir.endpoints.push(endpoint_retries("Ceiling", MAX_RETRIES));
        let proofs = generate_resource_bounds_proofs(&ir, VERSION);
        assert_eq!(proofs.len(), 3);
        for p in &proofs {
            assert_eq!(check_proof(p, &ir), CheckOutcome::Verified);
        }
    }

    /// Negative retries → Refuted (nonsensical).
    #[test]
    fn retry_negative_refuted() {
        let mut ir = empty_ir();
        ir.endpoints.push(endpoint_retries("Neg", -1));
        let proofs = generate_resource_bounds_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("retries=-1"), "got: {reason}");
                assert!(reason.contains("outside the bound"), "got: {reason}");
            }
            other => panic!("expected negative-retry Refuted, got {other:?}"),
        }
    }

    /// Retry count above the ceiling → Refuted (retry storm).
    #[test]
    fn retry_storm_refuted() {
        let mut ir = empty_ir();
        ir.endpoints
            .push(endpoint_retries("Storm", MAX_RETRIES + 1));
        let proofs = generate_resource_bounds_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("retry storm"), "got: {reason}");
            }
            other => panic!("expected retry-storm Refuted, got {other:?}"),
        }
    }

    /// A socket with a positive declared credit verifies.
    #[test]
    fn socket_positive_credit_verifies() {
        let mut ir = empty_ir();
        ir.sockets.push(socket("Chat", Some(8)));
        let proofs = generate_resource_bounds_proofs(&ir, VERSION);
        assert_eq!(proofs.len(), 1);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// A socket declaring credit(0) → Refuted (deadlock per v2.3.0).
    #[test]
    fn socket_zero_credit_refuted() {
        let mut ir = empty_ir();
        ir.sockets.push(socket("Dead", Some(0)));
        let proofs = generate_resource_bounds_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("deadlock"), "got: {reason}");
            }
            other => panic!("expected zero-credit Refuted, got {other:?}"),
        }
    }

    /// A socket with UNSPECIFIED credit produces no proof (legitimate
    /// type state, not a bound to certify).
    #[test]
    fn socket_unspecified_credit_no_proof() {
        let mut ir = empty_ir();
        ir.sockets.push(socket("Unspecified", None));
        // No endpoints either, so the whole proof set is empty.
        assert!(generate_resource_bounds_proofs(&ir, VERSION).is_empty());
    }

    /// ADVERSARIAL: a forged retry witness claiming
    /// `in_bounds: true` for a negative retry count is rejected — the
    /// checker recomputes the bound.
    #[test]
    fn forged_retry_in_bounds_rejected() {
        let mut ir = empty_ir();
        ir.endpoints.push(endpoint_retries("Liar", -5));
        let mut proofs = generate_resource_bounds_proofs(&ir, VERSION);
        if let Witness::ResourceBounds(ResourceBoundsWitness::EndpointRetry {
            ref mut in_bounds,
            ..
        }) = proofs[0].witness
        {
            *in_bounds = true; // the lie
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("disagrees with artifact"), "got: {reason}");
            }
            other => panic!("expected forged Refuted, got {other:?}"),
        }
    }

    /// ADVERSARIAL: a SocketCredit witness for a socket that has NO
    /// declared credit in the artifact is rejected (forged / stale).
    #[test]
    fn forged_socket_credit_for_unspecified_rejected() {
        let mut ir = empty_ir();
        ir.sockets.push(socket("Ghost", None)); // unspecified in artifact
        let digest = artifact_digest(&ir);
        let bogus = ProofTerm {
            property: PropertyClass::ResourceBounds,
            artifact_digest: digest,
            witness: Witness::ResourceBounds(derive_socket_credit_witness("Ghost", 8)),
            axon_version: VERSION.to_string(),
        };
        match check_proof(&bogus, &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(
                    reason.contains("no declared backpressure credit"),
                    "got: {reason}"
                );
            }
            other => panic!("expected forged-socket Refuted, got {other:?}"),
        }
    }

    /// Digest binding holds for resource-bound proofs.
    #[test]
    fn resource_proof_digest_mismatch_rejected() {
        let mut ir_a = empty_ir();
        ir_a.endpoints.push(endpoint_retries("E", 2));
        let proofs = generate_resource_bounds_proofs(&ir_a, VERSION);

        let mut ir_b = empty_ir();
        ir_b.endpoints.push(endpoint_retries("E", 2));
        ir_b.endpoints.push(endpoint_retries("Extra", 1)); // changes digest

        assert_eq!(check_proof(&proofs[0], &ir_b), CheckOutcome::DigestMismatch);
    }

    /// Resource-bound proof round-trips through JSON and still verifies.
    #[test]
    fn resource_proof_json_round_trips_and_verifies() {
        let mut ir = empty_ir();
        ir.sockets.push(socket("Chat", Some(16)));
        let proofs = generate_resource_bounds_proofs(&ir, VERSION);
        let json = serde_json::to_string(&proofs[0]).expect("serialize");
        let restored: ProofTerm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, proofs[0]);
        assert_eq!(check_proof(&restored, &ir), CheckOutcome::Verified);
    }

    // ── v2.4.0 — ShieldHaltGuarantee ─────────────────────────────

    /// Shield with the given `on_breach` policy + `scan` categories.
    fn shield_breach(name: &str, on_breach: &str, scan: &[&str]) -> IRShield {
        let mut s = shield(name, &[]); // reuse base builder (compliance empty)
        s.on_breach = on_breach.to_string();
        s.scan = scan.iter().map(|c| c.to_string()).collect();
        s
    }

    /// Happy path: a halt shield that actually scans verifies.
    #[test]
    fn valid_halt_with_scan_verifies() {
        let mut ir = empty_ir();
        ir.shields
            .push(shield_breach("Guard", "halt", &["pii_leak"]));
        let proofs = generate_shield_halt_guarantee_proofs(&ir, VERSION);
        assert_eq!(proofs.len(), 1);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// A non-halt policy (deflect) with an empty scan verifies — the
    /// vacuous check is halt-specific; deflect makes no halt guarantee.
    #[test]
    fn non_halt_policy_with_empty_scan_verifies() {
        let mut ir = empty_ir();
        ir.shields.push(shield_breach("Soft", "deflect", &[]));
        let proofs = generate_shield_halt_guarantee_proofs(&ir, VERSION);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// v2.34.0 — a sign-only egress shield (`sign: hmac_sha256`,
    /// no `scan:`) is a NON-vacuous halt: the signature is its enforcement
    /// (a breach = a delivery it refuses to sign). The brief-#51 Q3.d case.
    #[test]
    fn sign_only_halt_shield_verifies() {
        let mut ir = empty_ir();
        let mut s = shield_breach("WebhookEgress", "halt", &[]);
        s.sign = "hmac_sha256".to_string();
        ir.shields.push(s);
        let proofs = generate_shield_halt_guarantee_proofs(&ir, VERSION);
        assert_eq!(proofs.len(), 1);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// v2.34.0 — with NEITHER `scan:` nor `sign:`, a halt stays vacuous
    /// and the proof still refutes (the pre-v2.34.0 guarantee is unchanged).
    #[test]
    fn halt_with_neither_scan_nor_sign_still_refutes() {
        let mut ir = empty_ir();
        ir.shields.push(shield_breach("Hollow", "halt", &[]));
        let proofs = generate_shield_halt_guarantee_proofs(&ir, VERSION);
        assert_eq!(proofs.len(), 1);
        assert!(matches!(
            check_proof(&proofs[0], &ir),
            CheckOutcome::Refuted { .. }
        ));
    }

    /// A shield with no declared `on_breach` produces no proof.
    #[test]
    fn no_breach_policy_no_proof() {
        let mut ir = empty_ir();
        ir.shields.push(shield_breach("None", "", &["pii_leak"]));
        assert!(generate_shield_halt_guarantee_proofs(&ir, VERSION).is_empty());
    }

    /// Unknown breach policy (typo'd `hault`, which the PARSER does NOT
    /// reject — it reads `on_breach` as a bare identifier) → Refuted.
    #[test]
    fn unknown_breach_policy_refuted() {
        let mut ir = empty_ir();
        ir.shields
            .push(shield_breach("Typo", "hault", &["pii_leak"]));
        let proofs = generate_shield_halt_guarantee_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("unknown on_breach policy"), "got: {reason}");
                assert!(reason.contains("hault"), "got: {reason}");
            }
            other => panic!("expected unknown-policy Refuted, got {other:?}"),
        }
    }

    /// VACUOUS HALT: a shield declaring `on_breach: halt` with an empty
    /// `scan: []` → Refuted (the halt can never fire — security
    /// theater). This defect is NOT enforced upstream.
    #[test]
    fn vacuous_halt_refuted() {
        let mut ir = empty_ir();
        ir.shields.push(shield_breach("Theater", "halt", &[]));
        let proofs = generate_shield_halt_guarantee_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("vacuous"), "got: {reason}");
                assert!(reason.contains("never fire"), "got: {reason}");
            }
            other => panic!("expected vacuous-halt Refuted, got {other:?}"),
        }
    }

    /// ADVERSARIAL: a forged witness claiming `vacuous_halt:
    /// false` for a halt shield that scans nothing is rejected — the
    /// checker recomputes from the artifact.
    #[test]
    fn forged_vacuous_halt_rejected() {
        let mut ir = empty_ir();
        ir.shields.push(shield_breach("Sneaky", "halt", &[]));
        let mut proofs = generate_shield_halt_guarantee_proofs(&ir, VERSION);
        if let Witness::ShieldHaltGuarantee(ref mut w) = proofs[0].witness {
            w.vacuous_halt = false; // the lie
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("disagrees with artifact"), "got: {reason}");
            }
            other => panic!("expected forged Refuted, got {other:?}"),
        }
    }

    /// Digest binding holds for shield-halt proofs.
    #[test]
    fn shield_halt_digest_mismatch_rejected() {
        let mut ir_a = empty_ir();
        ir_a.shields
            .push(shield_breach("Guard", "halt", &["pii_leak"]));
        let proofs = generate_shield_halt_guarantee_proofs(&ir_a, VERSION);

        let mut ir_b = empty_ir();
        ir_b.shields
            .push(shield_breach("Guard", "halt", &["pii_leak"]));
        ir_b.shields
            .push(shield_breach("Other", "deflect", &["toxicity"])); // changes digest

        assert_eq!(check_proof(&proofs[0], &ir_b), CheckOutcome::DigestMismatch);
    }

    /// Shield-halt proof round-trips through JSON and still verifies.
    #[test]
    fn shield_halt_json_round_trips_and_verifies() {
        let mut ir = empty_ir();
        ir.shields
            .push(shield_breach("Guard", "halt", &["pii_leak", "jailbreak"]));
        let proofs = generate_shield_halt_guarantee_proofs(&ir, VERSION);
        let json = serde_json::to_string(&proofs[0]).expect("serialize");
        let restored: ProofTerm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, proofs[0]);
        assert_eq!(check_proof(&restored, &ir), CheckOutcome::Verified);
    }

    // ── v2.4.0 — CapabilityContainment ───────────────────────────

    /// An endpoint with the given execute_flow + declared requires.
    fn endpoint_requires(name: &str, execute_flow: &str, requires: &[&str]) -> IRAxonEndpoint {
        let mut e = endpoint(name, &[], "");
        e.execute_flow = execute_flow.to_string();
        e.requires_capabilities = requires.iter().map(|s| s.to_string()).collect();
        e
    }

    fn retrieve_step(store_name: &str) -> crate::ir_nodes::IRFlowNode {
        crate::ir_nodes::IRFlowNode::Retrieve(crate::ir_nodes::IRRetrieveStep {
            node_type: "retrieve",
            source_line: 1,
            source_column: 1,
            store_name: store_name.to_string(),
            where_expr: String::new(),
            alias: String::new(),
            order_by: String::new(),
            limit_expr: String::new(),
            aggregate: String::new(),
            group_by: String::new(),
            cache: String::new(),
        })
    }

    /// A flow whose top-level steps each retrieve from the given stores.
    fn flow_reaching(name: &str, store_names: &[&str]) -> crate::ir_nodes::IRFlow {
        crate::ir_nodes::IRFlow {
            node_type: "flow",
            source_line: 1,
            source_column: 1,
            name: name.to_string(),
            parameters: Vec::new(),
            return_type_name: String::new(),
            return_type_generic: String::new(),
            return_type_optional: false,
            steps: store_names.iter().map(|s| retrieve_step(s)).collect(),
            edges: Vec::new(),
            execution_levels: Vec::new(),
        }
    }

    /// A flow that reaches `store_name` ONLY inside a conditional
    /// then-branch (exercises the recursive store walk).
    fn flow_reaching_in_conditional(name: &str, store_name: &str) -> crate::ir_nodes::IRFlow {
        let cond = crate::ir_nodes::IRFlowNode::Conditional(crate::ir_nodes::IRConditional {
            node_type: "conditional",
            source_line: 1,
            source_column: 1,
            condition: "x".to_string(),
            comparison_op: "==".to_string(),
            comparison_value: "1".to_string(),
            then_body: vec![retrieve_step(store_name)],
            else_body: Vec::new(),
            conditions: Vec::new(),
            conjunctor: String::new(),
            cond: None,
        });
        let mut f = flow_reaching(name, &[]);
        f.steps = vec![cond];
        f
    }

    /// Happy path: the flow reaches a gated store, and the endpoint
    /// declares requiring that exact gate → contained → Verified.
    #[test]
    fn covered_containment_verifies() {
        let mut ir = empty_ir();
        ir.axonstore_specs.push(store("Ledger", "data.read"));
        ir.flows.push(flow_reaching("Chat", &["Ledger"]));
        ir.endpoints
            .push(endpoint_requires("ChatEndpoint", "Chat", &["data.read"]));
        let proofs = generate_capability_containment_proofs(&ir, VERSION);
        assert_eq!(proofs.len(), 1);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// CAPABILITY LEAK: the flow reaches a gated store but the endpoint
    /// declares no covering requires → uncovered gate → Refuted.
    #[test]
    fn uncovered_gate_refuted() {
        let mut ir = empty_ir();
        ir.axonstore_specs.push(store("Ledger", "data.read"));
        ir.flows.push(flow_reaching("Chat", &["Ledger"]));
        ir.endpoints.push(endpoint_requires("Leaky", "Chat", &[])); // declares nothing
        let proofs = generate_capability_containment_proofs(&ir, VERSION);
        assert_eq!(
            proofs.len(),
            1,
            "reached-gate-with-no-requires must produce a proof"
        );
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("capability leak"), "got: {reason}");
                assert!(reason.contains("data.read"), "got: {reason}");
            }
            other => panic!("expected leak Refuted, got {other:?}"),
        }
    }

    /// The recursive walk catches a store reached only inside a
    /// conditional branch → still a leak when uncovered.
    #[test]
    fn nested_conditional_reach_is_caught() {
        let mut ir = empty_ir();
        ir.axonstore_specs.push(store("Secret", "admin.read"));
        ir.flows
            .push(flow_reaching_in_conditional("Branchy", "Secret"));
        ir.endpoints
            .push(endpoint_requires("NestedLeak", "Branchy", &[]));
        let proofs = generate_capability_containment_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("admin.read"), "got: {reason}");
            }
            other => panic!("expected nested-reach Refuted, got {other:?}"),
        }
    }

    /// An unresolvable execute_flow → Refuted (cannot certify
    /// containment for a flow not in the artifact).
    #[test]
    fn unresolved_flow_refuted() {
        let mut ir = empty_ir();
        ir.endpoints
            .push(endpoint_requires("Ghost", "NoSuchFlow", &["x.read"]));
        let proofs = generate_capability_containment_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(
                    reason.contains("not present in the artifact"),
                    "got: {reason}"
                );
            }
            other => panic!("expected unresolved-flow Refuted, got {other:?}"),
        }
    }

    /// Declaring MORE requires than reached gates is fine (verified) —
    /// containment is `reached ⊆ declared`, not equality.
    #[test]
    fn over_declared_requires_verifies() {
        let mut ir = empty_ir();
        ir.axonstore_specs.push(store("Ledger", "data.read"));
        ir.flows.push(flow_reaching("Chat", &["Ledger"]));
        ir.endpoints.push(endpoint_requires(
            "Strict",
            "Chat",
            &["data.read", "extra.write"], // extra is harmless
        ));
        let proofs = generate_capability_containment_proofs(&ir, VERSION);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// An endpoint with no requires reaching only UNGATED stores has
    /// nothing to certify → no proof.
    #[test]
    fn trivial_no_requires_no_gates_no_proof() {
        let mut ir = empty_ir();
        ir.axonstore_specs.push(store("Open", "")); // ungated
        ir.flows.push(flow_reaching("Chat", &["Open"]));
        ir.endpoints.push(endpoint_requires("Plain", "Chat", &[]));
        assert!(generate_capability_containment_proofs(&ir, VERSION).is_empty());
    }

    /// ADVERSARIAL: a forged witness hiding an uncovered gate
    /// (clearing `uncovered_gates`) is rejected — the checker
    /// recomputes from the artifact.
    #[test]
    fn forged_hidden_uncovered_rejected() {
        let mut ir = empty_ir();
        ir.axonstore_specs.push(store("Ledger", "data.read"));
        ir.flows.push(flow_reaching("Chat", &["Ledger"]));
        ir.endpoints.push(endpoint_requires("Sneaky", "Chat", &[]));
        let mut proofs = generate_capability_containment_proofs(&ir, VERSION);
        if let Witness::CapabilityContainment(ref mut w) = proofs[0].witness {
            w.uncovered_gates.clear(); // the lie
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("disagrees with artifact"), "got: {reason}");
            }
            other => panic!("expected forged Refuted, got {other:?}"),
        }
    }

    /// Digest binding holds for containment proofs.
    #[test]
    fn containment_digest_mismatch_rejected() {
        let mut ir_a = empty_ir();
        ir_a.axonstore_specs.push(store("Ledger", "data.read"));
        ir_a.flows.push(flow_reaching("Chat", &["Ledger"]));
        ir_a.endpoints
            .push(endpoint_requires("E", "Chat", &["data.read"]));
        let proofs = generate_capability_containment_proofs(&ir_a, VERSION);

        let mut ir_b = empty_ir();
        ir_b.axonstore_specs.push(store("Ledger", "data.read"));
        ir_b.flows.push(flow_reaching("Chat", &["Ledger"]));
        ir_b.endpoints
            .push(endpoint_requires("E", "Chat", &["data.read"]));
        ir_b.endpoints
            .push(endpoint_requires("Extra", "Chat", &["data.read"])); // changes digest

        assert_eq!(check_proof(&proofs[0], &ir_b), CheckOutcome::DigestMismatch);
    }

    /// Containment proof round-trips through JSON and still verifies.
    #[test]
    fn containment_json_round_trips_and_verifies() {
        let mut ir = empty_ir();
        ir.axonstore_specs.push(store("Ledger", "data.read"));
        ir.flows.push(flow_reaching("Chat", &["Ledger"]));
        ir.endpoints
            .push(endpoint_requires("E", "Chat", &["data.read"]));
        let proofs = generate_capability_containment_proofs(&ir, VERSION);
        let json = serde_json::to_string(&proofs[0]).expect("serialize");
        let restored: ProofTerm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, proofs[0]);
        assert_eq!(check_proof(&restored, &ir), CheckOutcome::Verified);
    }

    // ── v2.4.0 — check_bundle deployability aggregate ────────────

    /// Build the full proof bundle for an IR (mirrors the CLI/handler
    /// `generate_all_proofs` → `ProofBundle` path).
    fn bundle_for(ir: &crate::ir_nodes::IRProgram) -> ProofBundle {
        ProofBundle {
            axon_version: VERSION.to_string(),
            artifact_digest: artifact_digest(ir),
            proofs: generate_all_proofs(ir, VERSION),
        }
    }

    /// A clean program (covered compliance + halting shield) → every
    /// proof verifies → `all_verified()` true, no refutations.
    #[test]
    fn bundle_report_all_verified_for_clean_program() {
        let mut ir = empty_ir();
        ir.shields
            .push(shield_breach("Guard", "halt", &["pii_leak"]));
        ir.axonstore_specs.push(store("Ledger", "data.read"));
        ir.flows.push(flow_reaching("Chat", &["Ledger"]));
        ir.endpoints
            .push(endpoint_requires("ChatEndpoint", "Chat", &["data.read"]));

        let report = check_bundle(&bundle_for(&ir), &ir);
        assert!(
            !report.results.is_empty(),
            "clean program still yields proofs"
        );
        assert!(
            report.all_verified(),
            "every proof must verify: {:?}",
            report.refutations()
        );
        assert!(report.refutations().is_empty());
    }

    /// A leaky program (a flow reaches a gated store the endpoint does
    /// NOT declare) → the containment proof refutes → `all_verified()`
    /// false, and the refutation names the leaked capability. This is
    /// the deploy-gate signal v2.4.0 rejects on.
    #[test]
    fn bundle_report_flags_capability_leak() {
        let mut ir = empty_ir();
        ir.axonstore_specs.push(store("Ledger", "data.read"));
        ir.flows.push(flow_reaching("Chat", &["Ledger"]));
        ir.endpoints.push(endpoint_requires("Leaky", "Chat", &[])); // declares nothing

        let report = check_bundle(&bundle_for(&ir), &ir);
        assert!(
            !report.all_verified(),
            "a capability leak must be non-deployable"
        );
        let refs = report.refutations();
        assert!(!refs.is_empty());
        assert!(
            refs.iter()
                .any(|r| r.property == PropertyClass::CapabilityContainment),
            "the leak must surface as a CapabilityContainment refutation"
        );
        assert!(
            refs.iter().any(|r| matches!(&r.outcome, CheckOutcome::Refuted { reason } if reason.contains("data.read"))),
            "the refutation must name the leaked capability"
        );
    }

    /// The aggregate agrees with per-proof `check_proof`, in order — the
    /// report is just a faithful fold, no hidden policy.
    #[test]
    fn bundle_report_matches_per_proof_check() {
        let mut ir = empty_ir();
        ir.shields
            .push(shield_breach("Guard", "halt", &["pii_leak"]));
        ir.endpoints.push(endpoint("E", &["HIPAA"], "Guard"));
        let bundle = bundle_for(&ir);
        let report = check_bundle(&bundle, &ir);
        assert_eq!(report.results.len(), bundle.proofs.len());
        for (r, proof) in report.results.iter().zip(&bundle.proofs) {
            assert_eq!(r.outcome, check_proof(proof, &ir));
            assert_eq!(r.property, proof.property);
            assert_eq!(r.subject, proof.witness.subject_name());
        }
    }

    /// A bundle whose proofs are about a DIFFERENT artifact → every
    /// check is `DigestMismatch` → non-deployable (fail-closed).
    #[test]
    fn bundle_report_rejects_foreign_bundle() {
        let mut ir_a = empty_ir();
        ir_a.endpoints.push(endpoint("E", &["HIPAA"], ""));
        let bundle = bundle_for(&ir_a);

        let mut ir_b = empty_ir();
        ir_b.endpoints.push(endpoint("E", &["HIPAA"], ""));
        ir_b.endpoints.push(endpoint("Other", &["PCI_DSS"], "")); // different digest

        let report = check_bundle(&bundle, &ir_b);
        assert!(!report.all_verified());
        assert!(report
            .results
            .iter()
            .all(|r| r.outcome == CheckOutcome::DigestMismatch));
    }

    // ── v2.4.0 — no-silent-gap reachability invariant ─────────

    /// The reachability walk's match is exhaustive: it must NOT contain
    /// a `_` wildcard arm. This is the maintainable proxy for the
    /// compiler-enforced invariant (a wildcard would let a future
    /// `IRFlowNode` variant carrying a store ref / nested body be
    /// silently missed). A refactor that reintroduces a wildcard fails
    /// HERE — the v2.4.0 no-silent-gap gate.
    #[test]
    fn reachability_walk_has_no_wildcard_arm() {
        const GEN_SRC: &str = include_str!("generate.rs");
        let start = GEN_SRC
            .find("fn collect_store_accesses(")
            .expect("collect_store_accesses present");
        // Bound the slice to the function body (up to the next top-level
        // `pub fn`, which is derive_capability_containment_witness).
        let rest = &GEN_SRC[start..];
        let end = rest
            .find("\npub fn derive_capability_containment_witness")
            .expect("derive_capability_containment_witness follows");
        let body = &rest[..end];
        assert!(
            !body.contains("_ => {}") && !body.contains("_ =>"),
            "collect_store_accesses must stay an EXHAUSTIVE match with no \
             `_` wildcard — a wildcard re-opens the silent-gap risk \
             (a future IRFlowNode variant could carry a store ref / nested \
             body and be missed)"
        );
    }

    /// The walk documents where transitive cross-flow reachability must
    /// be reopened if a flow-invocation node ever enters `IRFlowNode`
    /// (today `IRRun` is top-level only, so the case is vacuous — see
    /// v2.4.0 recon). Pin the sentinel so the note can't be silently
    /// dropped.
    #[test]
    fn reachability_walk_documents_transitive_reopen() {
        const GEN_SRC: &str = include_str!("generate.rs");
        assert!(
            GEN_SRC.contains("REOPENED") || GEN_SRC.contains("REOPEN"),
            "the walk must keep the note on where to reopen transitive \
             cross-flow reachability if a flow-invocation node ever joins \
             IRFlowNode"
        );
    }

    /// Leaf nodes (here: a payload-free `Break`) contribute NOTHING to
    /// the reachable-gate set — only the store op is counted. Confirms
    /// the leaf classification is inert.
    #[test]
    fn leaf_nodes_contribute_no_gates() {
        use crate::ir_nodes::{IRBreakStep, IRFlowNode};
        let mut ir = empty_ir();
        ir.axonstore_specs.push(store("Ledger", "data.read"));
        // A flow whose body interleaves a leaf node with the one store op.
        let mut flow = flow_reaching("Chat", &["Ledger"]);
        flow.steps.push(IRFlowNode::Break(IRBreakStep {
            node_type: "break",
            source_line: 1,
            source_column: 1,
        }));
        ir.flows.push(flow);
        let w = derive_capability_containment_witness("E", "Chat", &["data.read".to_string()], &ir);
        // Only the gated store contributes; the leaf adds nothing.
        assert_eq!(w.reached_gates, vec!["data.read".to_string()]);
        assert!(w.uncovered_gates.is_empty());
    }

    /// An empty bundle is vacuously deployable (nothing to refute).
    #[test]
    fn empty_bundle_is_vacuously_verified() {
        let ir = empty_ir();
        let bundle = ProofBundle {
            axon_version: VERSION.to_string(),
            artifact_digest: artifact_digest(&ir),
            proofs: Vec::new(),
        };
        let report = check_bundle(&bundle, &ir);
        assert!(report.all_verified());
        assert!(report.refutations().is_empty());
    }

    // ── v2.8.0 — ToolCallSoundness ───────────────────────────────

    /// A tool declaring the given `(name, type, optional)` input schema.
    fn tool_with_params(name: &str, params: &[(&str, &str, bool)]) -> IRToolSpec {
        let mut t = tool(name, &[]); // reuse base (effect_row empty)
        t.provider = "http".to_string();
        t.parameters = params
            .iter()
            .map(|(n, ty, opt)| crate::ir_nodes::IRToolParam {
                name: n.to_string(),
                type_name: ty.to_string(),
                optional: *opt,
            })
            .collect();
        t
    }

    /// A flow-level `use <Tool>(k = v, …)` call node.
    fn use_named(tool_name: &str, args: &[(&str, &str)]) -> crate::ir_nodes::IRFlowNode {
        crate::ir_nodes::IRFlowNode::UseTool(crate::ir_nodes::IRUseToolStep {
            node_type: "use_tool",
            source_line: 1,
            source_column: 1,
            tool_name: tool_name.to_string(),
            argument: String::new(),
            named_args: args
                .iter()
                .map(|(n, v)| crate::ir_nodes::IRNamedArg {
                    name: n.to_string(),
                    value: v.to_string(),
                    value_kind: "literal".to_string(),
                })
                .collect(),
        })
    }

    /// The legacy positional `use <Tool> on <arg>` call node (no named args).
    fn use_legacy(tool_name: &str, arg: &str) -> crate::ir_nodes::IRFlowNode {
        crate::ir_nodes::IRFlowNode::UseTool(crate::ir_nodes::IRUseToolStep {
            node_type: "use_tool",
            source_line: 1,
            source_column: 1,
            tool_name: tool_name.to_string(),
            argument: arg.to_string(),
            named_args: Vec::new(),
        })
    }

    /// A flow whose body is the given step nodes.
    fn flow_with_steps(name: &str, steps: Vec<crate::ir_nodes::IRFlowNode>) -> crate::ir_nodes::IRFlow {
        let mut f = flow_reaching(name, &[]);
        f.steps = steps;
        f
    }

    /// The canonical schema for the tests: company:String, max_results:Int,
    /// active:Bool? (active optional).
    fn crm_tool() -> IRToolSpec {
        tool_with_params(
            "CrmRadar",
            &[
                ("company", "String", false),
                ("max_results", "Int", false),
                ("active", "Bool", true),
            ],
        )
    }

    /// Happy path: a structured call satisfying the schema verifies. The
    /// bare-identifier `company` arg is runtime-resolved (skipped); the
    /// Int + Bool literals align; the optional `active` may be omitted.
    #[test]
    fn sound_tool_call_verifies() {
        let mut ir = empty_ir();
        ir.tools.push(crm_tool());
        ir.flows.push(flow_with_steps(
            "Scan",
            vec![use_named("CrmRadar", &[("company", "company"), ("max_results", "5")])],
        ));
        let proofs = generate_tool_call_soundness_proofs(&ir, VERSION);
        assert_eq!(proofs.len(), 1, "one schema-full structured call => one proof");
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// An unknown argument name → Refuted.
    #[test]
    fn unknown_arg_refuted() {
        let mut ir = empty_ir();
        ir.tools.push(crm_tool());
        ir.flows.push(flow_with_steps(
            "Scan",
            vec![use_named(
                "CrmRadar",
                &[("company", "x"), ("max_results", "5"), ("bogus", "1")],
            )],
        ));
        let proofs = generate_tool_call_soundness_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("does not declare"), "got: {reason}");
                assert!(reason.contains("bogus"), "got: {reason}");
            }
            other => panic!("expected unknown-arg Refuted, got {other:?}"),
        }
    }

    /// A duplicate argument → Refuted.
    #[test]
    fn duplicate_arg_refuted() {
        let mut ir = empty_ir();
        ir.tools.push(crm_tool());
        ir.flows.push(flow_with_steps(
            "Scan",
            vec![use_named(
                "CrmRadar",
                &[("company", "a"), ("company", "b"), ("max_results", "5")],
            )],
        ));
        let proofs = generate_tool_call_soundness_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("duplicate"), "got: {reason}");
                assert!(reason.contains("company"), "got: {reason}");
            }
            other => panic!("expected duplicate Refuted, got {other:?}"),
        }
    }

    /// A missing required argument → Refuted.
    #[test]
    fn missing_required_refuted() {
        let mut ir = empty_ir();
        ir.tools.push(crm_tool());
        // omit `max_results` (required); supply only `company`.
        ir.flows.push(flow_with_steps(
            "Scan",
            vec![use_named("CrmRadar", &[("company", "x")])],
        ));
        let proofs = generate_tool_call_soundness_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("omits required"), "got: {reason}");
                assert!(reason.contains("max_results"), "got: {reason}");
            }
            other => panic!("expected missing-required Refuted, got {other:?}"),
        }
    }

    /// An optional param omitted verifies (not "missing required").
    #[test]
    fn optional_param_omitted_verifies() {
        let mut ir = empty_ir();
        ir.tools.push(crm_tool());
        // `active` is optional → omitting it is fine.
        ir.flows.push(flow_with_steps(
            "Scan",
            vec![use_named("CrmRadar", &[("company", "x"), ("max_results", "5")])],
        ));
        let proofs = generate_tool_call_soundness_proofs(&ir, VERSION);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// A literal type mismatch (Int literal into a String param) → Refuted.
    #[test]
    fn literal_type_mismatch_refuted() {
        let mut ir = empty_ir();
        ir.tools.push(crm_tool());
        // company:String given the Int literal `5`.
        ir.flows.push(flow_with_steps(
            "Scan",
            vec![use_named("CrmRadar", &[("company", "5"), ("max_results", "5")])],
        ));
        let proofs = generate_tool_call_soundness_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("type mismatch"), "got: {reason}");
                assert!(reason.contains("company:String:Int"), "got: {reason}");
            }
            other => panic!("expected type-mismatch Refuted, got {other:?}"),
        }
    }

    /// Int coerces into a Float parameter (no mismatch).
    #[test]
    fn int_into_float_param_verifies() {
        let mut ir = empty_ir();
        ir.tools
            .push(tool_with_params("Calc", &[("ratio", "Float", false)]));
        ir.flows.push(flow_with_steps(
            "Run",
            vec![use_named("Calc", &[("ratio", "5")])], // Int literal into Float
        ));
        let proofs = generate_tool_call_soundness_proofs(&ir, VERSION);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    /// A schema-less tool (no `parameters:`) produces NO proof — there is
    /// no arg contract to certify.
    #[test]
    fn schema_less_tool_no_proof() {
        let mut ir = empty_ir();
        ir.tools.push(tool("Plain", &["network"])); // no parameters
        ir.flows.push(flow_with_steps(
            "Scan",
            vec![use_named("Plain", &[("anything", "1")])],
        ));
        assert!(generate_tool_call_soundness_proofs(&ir, VERSION).is_empty());
    }

    /// The legacy `use <Tool> on <arg>` form produces NO proof (schema-
    /// less by construction — D5 back-compat).
    #[test]
    fn legacy_on_form_no_proof() {
        let mut ir = empty_ir();
        ir.tools.push(crm_tool());
        ir.flows
            .push(flow_with_steps("Scan", vec![use_legacy("CrmRadar", "${q}")]));
        assert!(generate_tool_call_soundness_proofs(&ir, VERSION).is_empty());
    }

    /// Two calls to the SAME tool (one sound, one defective) are
    /// distinguished by `call_index`: the sound one verifies, the
    /// defective one refutes.
    #[test]
    fn two_calls_same_tool_distinguished() {
        let mut ir = empty_ir();
        ir.tools.push(crm_tool());
        ir.flows.push(flow_with_steps(
            "Scan",
            vec![
                use_named("CrmRadar", &[("company", "x"), ("max_results", "5")]), // sound
                use_named("CrmRadar", &[("company", "x"), ("bogus", "1")]),       // defective
            ],
        ));
        let proofs = generate_tool_call_soundness_proofs(&ir, VERSION);
        assert_eq!(proofs.len(), 2);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
        match check_proof(&proofs[1], &ir) {
            CheckOutcome::Refuted { reason } => {
                // proofs[1] is the defective call: it omits `max_results`
                // AND passes the unknown `bogus`.
                assert!(
                    reason.contains("bogus") || reason.contains("max_results"),
                    "got: {reason}"
                );
            }
            other => panic!("expected Refuted for the defective call, got {other:?}"),
        }
    }

    /// A defective call nested inside a conditional branch is caught (the
    /// recursive walk descends into then/else bodies).
    #[test]
    fn nested_conditional_call_is_caught() {
        let mut ir = empty_ir();
        ir.tools.push(crm_tool());
        let cond = crate::ir_nodes::IRFlowNode::Conditional(crate::ir_nodes::IRConditional {
            node_type: "conditional",
            source_line: 1,
            source_column: 1,
            condition: "x".to_string(),
            comparison_op: "==".to_string(),
            comparison_value: "1".to_string(),
            then_body: vec![use_named("CrmRadar", &[("company", "x"), ("ghost", "1")])],
            else_body: Vec::new(),
            conditions: Vec::new(),
            conjunctor: String::new(),
            cond: None,
        });
        ir.flows.push(flow_with_steps("Branchy", vec![cond]));
        let proofs = generate_tool_call_soundness_proofs(&ir, VERSION);
        assert_eq!(proofs.len(), 1, "the nested call must be discovered");
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("ghost"), "got: {reason}");
            }
            other => panic!("expected nested-call Refuted, got {other:?}"),
        }
    }

    /// ADVERSARIAL: a forged witness hiding an unknown argument
    /// (clearing `unknown_args`) is rejected — the checker recomputes.
    #[test]
    fn forged_hidden_unknown_arg_rejected() {
        let mut ir = empty_ir();
        ir.tools.push(crm_tool());
        ir.flows.push(flow_with_steps(
            "Scan",
            vec![use_named(
                "CrmRadar",
                &[("company", "x"), ("max_results", "5"), ("sneaky", "1")],
            )],
        ));
        let mut proofs = generate_tool_call_soundness_proofs(&ir, VERSION);
        if let Witness::ToolCallSoundness(ref mut w) = proofs[0].witness {
            w.unknown_args.clear(); // the lie
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("disagrees with artifact"), "got: {reason}");
            }
            other => panic!("expected forged Refuted, got {other:?}"),
        }
    }

    /// ADVERSARIAL: a witness naming a call index absent from the artifact
    /// is Refuted (not a panic).
    #[test]
    fn proof_for_absent_call_index_refuted() {
        let mut ir = empty_ir();
        ir.tools.push(crm_tool());
        ir.flows.push(flow_with_steps(
            "Scan",
            vec![use_named("CrmRadar", &[("company", "x"), ("max_results", "5")])],
        ));
        let mut proofs = generate_tool_call_soundness_proofs(&ir, VERSION);
        if let Witness::ToolCallSoundness(ref mut w) = proofs[0].witness {
            w.call_index = 9; // no such call
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("no structured `use` call at index"), "got: {reason}");
            }
            other => panic!("expected absent-call Refuted, got {other:?}"),
        }
    }

    /// Digest binding holds for tool-call proofs.
    #[test]
    fn tool_call_proof_digest_mismatch_rejected() {
        let mut ir_a = empty_ir();
        ir_a.tools.push(crm_tool());
        ir_a.flows.push(flow_with_steps(
            "Scan",
            vec![use_named("CrmRadar", &[("company", "x"), ("max_results", "5")])],
        ));
        let proofs = generate_tool_call_soundness_proofs(&ir_a, VERSION);

        let mut ir_b = empty_ir();
        ir_b.tools.push(crm_tool());
        ir_b.flows.push(flow_with_steps(
            "Scan",
            vec![use_named("CrmRadar", &[("company", "x"), ("max_results", "5")])],
        ));
        ir_b.tools.push(tool("Extra", &["network"])); // changes digest

        assert_eq!(check_proof(&proofs[0], &ir_b), CheckOutcome::DigestMismatch);
    }

    /// Tool-call proof round-trips through JSON and still verifies.
    #[test]
    fn tool_call_proof_json_round_trips_and_verifies() {
        let mut ir = empty_ir();
        ir.tools.push(crm_tool());
        ir.flows.push(flow_with_steps(
            "Scan",
            vec![use_named(
                "CrmRadar",
                &[("company", "x"), ("max_results", "5"), ("active", "true")],
            )],
        ));
        let proofs = generate_tool_call_soundness_proofs(&ir, VERSION);
        let json = serde_json::to_string(&proofs[0]).expect("serialize");
        let restored: ProofTerm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, proofs[0]);
        assert_eq!(check_proof(&restored, &ir), CheckOutcome::Verified);
    }

    /// `generate_all_proofs` includes the tool-call-soundness class, and a
    /// defective call makes the bundle non-deployable (the v2.4.0 deploy-gate
    /// signal).
    #[test]
    fn all_proofs_includes_tool_call_soundness_and_flags_defect() {
        let mut ir = empty_ir();
        ir.tools.push(crm_tool());
        ir.flows.push(flow_with_steps(
            "Scan",
            vec![use_named("CrmRadar", &[("company", "5"), ("max_results", "5")])], // Int into String
        ));
        let bundle = bundle_for(&ir);
        assert!(
            bundle
                .proofs
                .iter()
                .any(|p| p.property == PropertyClass::ToolCallSoundness),
            "generate_all_proofs must include the tool-call-soundness class"
        );
        let report = check_bundle(&bundle, &ir);
        assert!(!report.all_verified(), "a literal type mismatch is non-deployable");
        assert!(report
            .refutations()
            .iter()
            .any(|r| r.property == PropertyClass::ToolCallSoundness));
    }

    // ── v2.28.0 — EffectBudgeted ──────────────────────────────────────

    /// Parse → IR (no type-check, so a deliberately-defective budget survives to
    /// the checker, which is the independent verifier under test).
    fn ir_from_source(src: &str) -> crate::ir_nodes::IRProgram {
        let toks = axon_frontend::lexer::Lexer::new(src, "<pcc-test>").tokenize().unwrap();
        let prog = axon_frontend::parser::Parser::new(toks).parse().unwrap();
        axon_frontend::ir_generator::IRGenerator::new().generate(&prog)
    }

    const BUDGETED_DAEMON: &str = "tool TelnyxCall { provider: http timeout: 5s }\n\
         flow SendBatch() -> Unit { step S { ask: \"x\" output: Unit } }\n\
         daemon OutboundScheduler {\n\
           requires: [flow.execute]\n\
           budget {\n\
             rate: 8 per hour on Tool(TelnyxCall)\n\
             max: 50 per day on Tool(TelnyxCall)\n\
             on_exhausted: defer\n\
           }\n\
           listen \"cron:*/5 * * * *\" as t { run SendBatch() }\n\
         }";

    #[test]
    fn effect_budgeted_round_trips_and_verifies() {
        let ir = ir_from_source(BUDGETED_DAEMON);
        let proofs = super::generate::generate_effect_budgeted_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one proof per budgeted daemon");
        assert_eq!(proofs[0].property, PropertyClass::EffectBudgeted);
        // The independent checker verifies a well-formed budget.
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    // ── v2.42.0 — SavantSoundness ──────────────────────────────────────────────

    const VALID_SAVANT: &str = "memory ResearchStore { store: persistent }\n\
         savant DeepAnalyst {\n\
           domain: \"quantum error correction\"\n\
           cognition { depth: hyper, entropic_threshold: 0.001, divergence: high }\n\
           memory { backend: ResearchStore, corpus_graph: true }\n\
           budget { max_iterations: 50000 }\n\
           mandate m { objective: \"synthesise papers\", output: Report }\n\
         }";

    #[test]
    fn savant_soundness_round_trips_and_verifies() {
        let ir = ir_from_source(VALID_SAVANT);
        let proofs = super::generate::generate_savant_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one proof per savant");
        assert_eq!(proofs[0].property, PropertyClass::SavantSoundness);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
        assert!(
            generate_all_proofs(&ir, "test")
                .iter()
                .any(|p| p.property == PropertyClass::SavantSoundness),
            "generate_all_proofs must include the savant-soundness class"
        );
    }

    #[test]
    fn savant_soundness_refutes_unbudgeted() {
        // Parses + lowers, but declares no budget → the PCC checker REFUTES it
        // (T877 — an unbounded autonomous loop is fail-open).
        let ir = ir_from_source(
            "savant S { domain: \"d\" mandate m { objective: \"o\", output: T } }",
        );
        let proofs = super::generate::generate_savant_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        assert!(matches!(
            check_proof(&proofs[0], &ir),
            CheckOutcome::Refuted { .. }
        ));
    }

    #[test]
    fn savant_soundness_refutes_undefined_memory() {
        let ir = ir_from_source(
            "savant S { domain: \"d\" memory { backend: Nope } budget { max_iterations: 5 } \
             mandate m { objective: \"o\", output: T } }",
        );
        let proofs = super::generate::generate_savant_soundness_proofs(&ir, "test");
        assert!(matches!(
            check_proof(&proofs[0], &ir),
            CheckOutcome::Refuted { .. }
        ));
    }

    #[test]
    fn savant_soundness_refutes_forged_witness() {
        let ir = ir_from_source(VALID_SAVANT);
        let mut proof = super::generate::generate_savant_soundness_proofs(&ir, "test").remove(0);
        // Tamper: claim a budget bound the artifact does not carry.
        if let Witness::SavantSoundness(ref mut w) = proof.witness {
            w.max_iterations = 999_999;
        }
        assert!(matches!(
            check_proof(&proof, &ir),
            CheckOutcome::Refuted { .. }
        ));
    }

    // ── v2.43.0 — WardenSoundness ──────────────────────────────────────────────

    const VALID_WARDEN: &str = "scope InternalAudit {\n\
           targets: [ \"svc://payments\" ]\n\
           depth: static_artifact\n\
           approver: requires \"security.lead\"\n\
         }\n\
         flow Audit() -> Unit {\n\
           warden(payments) within InternalAudit { step S { ask: \"x\" output: Unit } }\n\
         }";

    #[test]
    fn warden_soundness_round_trips_and_verifies() {
        let ir = ir_from_source(VALID_WARDEN);
        let proofs = super::generate::generate_warden_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one proof per warden block");
        assert_eq!(proofs[0].property, PropertyClass::WardenSoundness);
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
        assert!(
            generate_all_proofs(&ir, "test")
                .iter()
                .any(|p| p.property == PropertyClass::WardenSoundness),
            "generate_all_proofs must include the warden-soundness class"
        );
    }

    #[test]
    fn warden_soundness_refutes_undefined_scope() {
        // Parses + lowers, but `within Ghost` resolves to no declared scope → the
        // PCC checker REFUTES it (T887 — no authorization scope, no analysis).
        let ir = ir_from_source(
            "flow Audit() -> Unit { warden(t) within Ghost { step S { ask: \"x\" output: Unit } } }",
        );
        let proofs = super::generate::generate_warden_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        assert!(matches!(
            check_proof(&proofs[0], &ir),
            CheckOutcome::Refuted { .. }
        ));
    }

    #[test]
    fn warden_soundness_refutes_forged_witness() {
        let ir = ir_from_source(VALID_WARDEN);
        let mut proof = super::generate::generate_warden_soundness_proofs(&ir, "test").remove(0);
        // Tamper: claim the scope resolves to a different name than the artifact.
        if let Witness::WardenSoundness(ref mut w) = proof.witness {
            w.approver_present = false;
        }
        assert!(matches!(
            check_proof(&proof, &ir),
            CheckOutcome::Refuted { .. }
        ));
    }

    #[test]
    fn budgetless_daemon_carries_no_effect_budgeted_proof() {
        let ir = ir_from_source(
            "flow SendBatch() -> Unit { step S { ask: \"x\" output: Unit } }\n\
             daemon Plain {\n\
               requires: [flow.execute]\n\
               listen \"cron:*/5 * * * *\" as t { run SendBatch() }\n\
             }",
        );
        assert!(super::generate::generate_effect_budgeted_proofs(&ir, "test").is_empty());
    }

    #[test]
    fn effect_budgeted_refutes_an_undefined_tool() {
        // A budget targeting an undeclared tool PARSES (the type-checker would
        // T830 it, but we skip that) — the independent PCC checker REFUTES it.
        let ir = ir_from_source(
            "flow SendBatch() -> Unit { step S { ask: \"x\" output: Unit } }\n\
             daemon D {\n\
               requires: [flow.execute]\n\
               budget { rate: 8 per hour on Tool(NoSuchTool) }\n\
               listen \"cron:*/5 * * * *\" as t { run SendBatch() }\n\
             }",
        );
        let proofs = super::generate::generate_effect_budgeted_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("NoSuchTool"), "{reason}"),
            other => panic!("expected Refuted, got {other:?}"),
        }
    }

    #[test]
    fn effect_budgeted_refutes_a_forged_witness() {
        let ir = ir_from_source(BUDGETED_DAEMON);
        let mut proofs = super::generate::generate_effect_budgeted_proofs(&ir, "test");
        // Forge the witness: claim a clean budget had an unresolved effect.
        if let Witness::EffectBudgeted(ref mut w) = proofs[0].witness {
            w.unresolved_effects = vec!["Ghost".to_string()];
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("disagrees"), "{reason}")
            }
            other => panic!("expected Refuted (forgery), got {other:?}"),
        }
    }

    // ── v2.26.0 — JsonShapeSoundness ──────────────────────────────────

    const LENS_STORE: &str = "type UserEvent { name: String age: Int }\n\
         axonstore Events {\n\
           backend: postgresql\n\
           connection: \"env:DB\"\n\
           schema {\n\
             id: Uuid primary_key\n\
             profile: Json<UserEvent>\n\
           }\n\
         }";

    #[test]
    fn json_shape_soundness_round_trips_and_verifies() {
        let ir = ir_from_source(LENS_STORE);
        let proofs = super::generate::generate_json_shape_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one proof per store with a lens column");
        assert_eq!(proofs[0].property, PropertyClass::JsonShapeSoundness);
        // The shape `UserEvent` is a declared struct → the checker verifies.
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn open_json_store_carries_no_json_shape_proof() {
        // A store whose only Json column is OPEN (no `<T>` lens) declares no
        // shape contract → no proof.
        let ir = ir_from_source(
            "axonstore Events {\n\
               backend: postgresql\n\
               connection: \"env:DB\"\n\
               schema { id: Uuid primary_key  payload: Json }\n\
             }",
        );
        assert!(super::generate::generate_json_shape_soundness_proofs(&ir, "test").is_empty());
    }

    #[test]
    fn json_shape_soundness_refutes_an_undeclared_shape() {
        // `Json<Ghost>` with no `type Ghost` PARSES (the type-checker would
        // axon-T840 it, but we skip that) — the independent PCC checker REFUTES.
        let ir = ir_from_source(
            "axonstore Events {\n\
               backend: postgresql\n\
               connection: \"env:DB\"\n\
               schema { id: Uuid primary_key  profile: Json<Ghost> }\n\
             }",
        );
        let proofs = super::generate::generate_json_shape_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("Ghost"), "{reason}")
            }
            other => panic!("expected Refuted, got {other:?}"),
        }
    }

    #[test]
    fn json_shape_soundness_refutes_a_forged_witness() {
        let ir = ir_from_source(LENS_STORE);
        let mut proofs = super::generate::generate_json_shape_soundness_proofs(&ir, "test");
        // Forge: claim a clean store had an unresolved shape.
        if let Witness::JsonShapeSoundness(ref mut w) = proofs[0].witness {
            w.unresolved_shapes = vec!["profile:Ghost".to_string()];
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("disagrees"), "{reason}")
            }
            other => panic!("expected Refuted (forgery), got {other:?}"),
        }
    }

    // ── v2.31.0 — ChannelDeliverySoundness ────────────────────────────

    const PRODUCED_CHANNEL: &str = "type Hib { tenant_id: String }\n\
         channel HibCh { message: Hib  qos: at_least_once  persistence: persistent_axonstore }\n\
         flow Produce(tenant_id: String) -> Unit { emit HibCh(tenant_id) }\n\
         flow Learn(tenant_id: String) -> Unit { probe p }\n\
         daemon IntentLearner {\n\
           requires: [flow.execute]\n\
           listen HibCh as ev { run Learn() }\n\
         }";

    #[test]
    fn channel_delivery_round_trips_and_verifies() {
        let ir = ir_from_source(PRODUCED_CHANNEL);
        let proofs = super::generate::generate_channel_delivery_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one proof per listened channel");
        assert_eq!(proofs[0].property, PropertyClass::ChannelDeliverySoundness);
        // The channel has a producer (`emit HibCh`) → verifies.
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn unlistened_channel_carries_no_delivery_proof() {
        // A channel emitted to but with NO `listen`er → no delivery contract.
        let ir = ir_from_source(
            "type Hib { tenant_id: String }\n\
             channel HibCh { message: Hib }\n\
             flow Produce(tenant_id: String) -> Unit { emit HibCh(tenant_id) }",
        );
        assert!(super::generate::generate_channel_delivery_soundness_proofs(&ir, "test").is_empty());
    }

    #[test]
    fn channel_delivery_refutes_a_listener_with_no_producer() {
        // The Kivi brief #39 defect: a daemon listens on a channel NOTHING
        // emits to → the listener can never fire → REFUTED.
        let ir = ir_from_source(
            "type Hib { tenant_id: String }\n\
             channel HibCh { message: Hib  persistence: persistent_axonstore }\n\
             flow Learn() -> Unit { probe p }\n\
             daemon IntentLearner {\n\
               requires: [flow.execute]\n\
               listen HibCh as ev { run Learn() }\n\
             }",
        );
        let proofs = super::generate::generate_channel_delivery_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("no producer") || reason.contains("never fire"), "{reason}")
            }
            other => panic!("expected Refuted (no producer), got {other:?}"),
        }
    }

    #[test]
    fn channel_delivery_refutes_a_forged_witness() {
        let ir = ir_from_source(PRODUCED_CHANNEL);
        let mut proofs = super::generate::generate_channel_delivery_soundness_proofs(&ir, "test");
        // Forge: claim there was no producer when the artifact has one.
        if let Witness::ChannelDeliverySoundness(ref mut w) = proofs[0].witness {
            w.has_producer = false;
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("disagrees"), "{reason}"),
            other => panic!("expected Refuted (forgery), got {other:?}"),
        }
    }

    // ── v2.33.0 — AggregateSoundness ──────────────────────────────────

    const AGGREGATE_RETRIEVE: &str = "axonstore Tenants {\n\
           backend: postgresql\n\
           connection: \"env:DB\"\n\
         }\n\
         flow PlatformStats() -> Unit {\n\
           retrieve Tenants {\n\
             where: \"tokens > 0\"\n\
             aggregate: \"sum(tokens)\"\n\
             group_by: \"industry\"\n\
             as: stats\n\
           }\n\
         }";

    #[test]
    fn aggregate_soundness_round_trips_and_verifies() {
        let ir = ir_from_source(AGGREGATE_RETRIEVE);
        let proofs = super::generate::generate_aggregate_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one proof per aggregate-retrieve site");
        assert_eq!(proofs[0].property, PropertyClass::AggregateSoundness);
        if let Witness::AggregateSoundness(ref w) = proofs[0].witness {
            assert_eq!(w.function, "sum");
            assert_eq!(w.column, "tokens");
            assert_eq!(w.group_columns, vec!["industry".to_string()]);
            assert!(w.violations.is_empty());
        } else {
            panic!("expected an AggregateSoundness witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn plain_retrieve_carries_no_aggregate_proof() {
        let ir = ir_from_source(
            "axonstore Tenants {\n\
               backend: postgresql\n\
               connection: \"env:DB\"\n\
             }\n\
             flow List() -> Unit {\n\
               retrieve Tenants { where: \"tokens > 0\"  as: rows }\n\
             }",
        );
        assert!(super::generate::generate_aggregate_soundness_proofs(&ir, "test").is_empty());
    }

    #[test]
    fn aggregate_soundness_refutes_an_unsound_clause() {
        // group_by without an aggregate — a T845-class violation the
        // runtime parser records; the proof generates but REFUTES, so the
        // v2.4.0 deploy gate rejects the bundle fail-closed.
        let ir = ir_from_source(
            "axonstore Tenants {\n\
               backend: postgresql\n\
               connection: \"env:DB\"\n\
             }\n\
             flow Bad() -> Unit {\n\
               retrieve Tenants { group_by: \"industry\"  as: rows }\n\
             }",
        );
        let proofs = super::generate::generate_aggregate_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("UNSOUND"), "{reason}")
            }
            other => panic!("expected Refuted (unsound clause), got {other:?}"),
        }
    }

    #[test]
    fn aggregate_soundness_refutes_a_forged_witness() {
        let ir = ir_from_source(AGGREGATE_RETRIEVE);
        let mut proofs = super::generate::generate_aggregate_soundness_proofs(&ir, "test");
        // Forge: claim the aggregate was a different clause than declared.
        if let Witness::AggregateSoundness(ref mut w) = proofs[0].witness {
            w.aggregate = "count".to_string();
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("forged") || reason.contains("no aggregate-retrieve"), "{reason}")
            }
            other => panic!("expected Refuted (forgery), got {other:?}"),
        }
    }

    // ── v2.34.0 — ChannelEgressSoundness ──────────────────────────────

    /// The brief-#51 target program: a durable channel egress-published
    /// under a sign-only shield.
    const EGRESS_CHANNEL: &str = "type SkillResult { task_id: String }\n\
         shield WebhookEgress { sign: hmac_sha256  on_breach: halt }\n\
         channel SkillCompleted { message: SkillResult  qos: at_least_once  \
           lifetime: affine  persistence: persistent_axonstore  shield: WebhookEgress }\n\
         flow CompleteSkill(task_id: String) -> Unit {\n\
           emit SkillCompleted(task_id)\n\
           publish SkillCompleted within WebhookEgress\n\
         }";

    #[test]
    fn channel_egress_round_trips_and_verifies() {
        let ir = ir_from_source(EGRESS_CHANNEL);
        // The lowering stamped the handle (v2.34.0 Phase 1.5)…
        assert_eq!(ir.channels[0].egress_sign, "hmac_sha256");
        // …and the proof re-derives + verifies it.
        let proofs = super::generate::generate_channel_egress_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one proof per egress channel");
        assert_eq!(proofs[0].property, PropertyClass::ChannelEgressSoundness);
        if let Witness::ChannelEgressSoundness(ref w) = proofs[0].witness {
            assert_eq!(w.declared_egress_sign, "hmac_sha256");
            assert_eq!(w.derived_sign, "hmac_sha256");
            assert_eq!(w.shield_ref, "WebhookEgress");
            assert!(w.durable);
        } else {
            panic!("expected a ChannelEgressSoundness witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn non_signing_publish_carries_no_egress_proof() {
        // A publish under a SCANNING shield is pure π-calc capability
        // extrusion — no egress contract, no proof (pre-v2.34.0 semantics).
        let ir = ir_from_source(
            "type T { id: String }\n\
             shield Gate { scan: [pii_leak]  on_breach: halt }\n\
             channel C { message: T  shield: Gate }\n\
             flow F(id: String) -> Unit { publish C within Gate }",
        );
        assert!(ir.channels[0].egress_sign.is_empty());
        assert!(
            super::generate::generate_channel_egress_soundness_proofs(&ir, "test").is_empty()
        );
    }

    #[test]
    fn channel_egress_refutes_an_ephemeral_channel() {
        // Signed egress on a non-durable channel — the promise dies with
        // the process. The compile-time mirror is axon-T848; the
        // proof REFUTES independently so a handcrafted IR cannot sneak by.
        let ir = ir_from_source(
            "type T { id: String }\n\
             shield WebhookEgress { sign: hmac_sha256  on_breach: halt }\n\
             channel C { message: T  shield: WebhookEgress }\n\
             flow F(id: String) -> Unit { publish C within WebhookEgress }",
        );
        let proofs = super::generate::generate_channel_egress_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("persistent_axonstore"), "{reason}")
            }
            other => panic!("expected Refuted (ephemeral egress), got {other:?}"),
        }
    }

    #[test]
    fn channel_egress_refutes_a_forged_handle() {
        // Forge the CHANNEL HANDLE (what the enterprise egress worker
        // reads): stamp an egress marking the program's publish sites do
        // not derive. The checker recomputes from the shields — refuted.
        let mut ir = ir_from_source(
            "type T { id: String }\n\
             shield Gate { scan: [pii_leak]  on_breach: halt }\n\
             channel C { message: T  persistence: persistent_axonstore  shield: Gate }\n\
             flow F(id: String) -> Unit { publish C within Gate }",
        );
        ir.channels[0].egress_sign = "hmac_sha256".to_string();
        let proofs = super::generate::generate_channel_egress_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "the forged handle now carries a contract");
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("disagrees") || reason.contains("derivable"), "{reason}")
            }
            other => panic!("expected Refuted (forged handle), got {other:?}"),
        }
    }

    #[test]
    fn channel_egress_refutes_a_forged_witness() {
        let ir = ir_from_source(EGRESS_CHANNEL);
        let mut proofs = super::generate::generate_channel_egress_soundness_proofs(&ir, "test");
        // Forge: claim the egress was ephemeral-safe by lying about
        // persistence.
        if let Witness::ChannelEgressSoundness(ref mut w) = proofs[0].witness {
            w.persistence = "ephemeral".to_string();
            w.durable = false;
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("disagrees"), "{reason}"),
            other => panic!("expected Refuted (forgery), got {other:?}"),
        }
    }

    // ── v2.38.0 — CorsPolicyConsistency ───────────────────────────────

    const CORS_PROGRAM: &str = "flow Chat() -> Unit { step S { ask: \"hi\" } }\n\
         cors PublicWebCors {\n\
           allow_origins: [\"https://app.example.com\"]\n\
           allow_methods: [GET, POST]\n\
           allow_credentials: false\n\
         }\n\
         axonendpoint E { method: POST path: \"/chat\" execute: Chat cors: PublicWebCors }";

    #[test]
    fn cors_policy_consistency_round_trips_and_verifies() {
        let ir = ir_from_source(CORS_PROGRAM);
        let proofs = super::generate::generate_cors_policy_consistency_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one whole-program proof when a cors contract exists");
        assert_eq!(proofs[0].property, PropertyClass::CorsPolicyConsistency);
        if let Witness::CorsPolicyConsistency(ref w) = proofs[0].witness {
            assert_eq!(w.declared_cors_names, vec!["PublicWebCors".to_string()]);
            assert_eq!(
                w.endpoint_cors_refs,
                vec![("E".to_string(), "PublicWebCors".to_string())]
            );
            assert!(w.all_references_resolve);
            assert!(w.wildcard_credential_violations.is_empty());
            assert!(w.cross_method_conflicts.is_empty());
        } else {
            panic!("expected a CorsPolicyConsistency witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn cors_less_program_carries_no_proof() {
        let ir = ir_from_source("flow Chat() -> Unit { step S { ask: \"hi\" } }\n");
        assert!(
            super::generate::generate_cors_policy_consistency_proofs(&ir, "test").is_empty(),
            "no contract → no proof"
        );
    }

    #[test]
    fn cors_policy_consistency_refutes_a_forged_reference() {
        // Forge: the witness claims a reference the artifact doesn't have.
        let ir = ir_from_source(CORS_PROGRAM);
        let mut proofs = super::generate::generate_cors_policy_consistency_proofs(&ir, "test");
        if let Witness::CorsPolicyConsistency(ref mut w) = proofs[0].witness {
            w.endpoint_cors_refs.push(("Ghost".to_string(), "PublicWebCors".to_string()));
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("disagrees"), "{reason}"),
            other => panic!("expected Refuted (forgery), got {other:?}"),
        }
    }

    #[test]
    fn cors_policy_consistency_refutes_an_unresolved_reference() {
        // Hand-craft an IR (bypassing the checker that would have caught
        // this at compile time, axon-T856) where the endpoint's cors_ref
        // names a declaration that doesn't exist.
        let mut ir = ir_from_source(CORS_PROGRAM);
        ir.endpoints[0].cors_ref = "Ghost".to_string();
        let proofs = super::generate::generate_cors_policy_consistency_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T856"), "{reason}"),
            other => panic!("expected Refuted (unresolved reference), got {other:?}"),
        }
    }

    #[test]
    fn cors_policy_consistency_refutes_wildcard_plus_credentials() {
        // Hand-craft an IR with the forbidden pairing (bypassing axon-T853).
        let mut ir = ir_from_source(CORS_PROGRAM);
        ir.cors_policies[0].allow_origins = vec!["*".to_string()];
        ir.cors_policies[0].allow_credentials = true;
        let proofs = super::generate::generate_cors_policy_consistency_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T853"), "{reason}"),
            other => panic!("expected Refuted (wildcard+credentials), got {other:?}"),
        }
    }

    #[test]
    fn cors_policy_consistency_refutes_a_cross_method_conflict() {
        // Hand-craft an IR with two endpoints on one path disagreeing on
        // cors_ref (bypassing axon-T857).
        let mut ir = ir_from_source(CORS_PROGRAM);
        let mut second = ir.endpoints[0].clone();
        second.name = "E2".to_string();
        second.method = "GET".to_string();
        second.cors_ref = String::new();
        ir.endpoints.push(second);
        let proofs = super::generate::generate_cors_policy_consistency_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T857"), "{reason}"),
            other => panic!("expected Refuted (cross-method conflict), got {other:?}"),
        }
    }

    // ── v2.46.0 — TemporalContextSoundness ────────────────────────────

    const TEMPORAL_PROGRAM: &str = "context Scheduling { now: \"UTC\" }\n\
         flow Plan() -> Unit {\n\
             step Triage { now: \"America/Bogota\" ask: \"slots\" }\n\
             step Confirm { ask: \"confirm\" }\n\
         }\n";

    #[test]
    fn temporal_context_soundness_round_trips_and_verifies() {
        let ir = ir_from_source(TEMPORAL_PROGRAM);
        let proofs =
            super::generate::generate_temporal_context_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one whole-program proof when a temporal contract exists");
        assert_eq!(proofs[0].property, PropertyClass::TemporalContextSoundness);
        if let Witness::TemporalContextSoundness(ref w) = proofs[0].witness {
            assert_eq!(
                w.declarations,
                vec![
                    ("context".to_string(), "Scheduling".to_string(), "UTC".to_string()),
                    ("step".to_string(), "Triage".to_string(), "America/Bogota".to_string()),
                ]
            );
            assert!(w.format_violations.is_empty());
            assert!(w.unknown_zones.is_empty());
        } else {
            panic!("expected a TemporalContextSoundness witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn temporal_less_program_carries_no_proof() {
        let ir = ir_from_source("flow Chat() -> Unit { step S { ask: \"hi\" } }\n");
        assert!(
            super::generate::generate_temporal_context_soundness_proofs(&ir, "test").is_empty(),
            "no temporal contract → no proof"
        );
    }

    #[test]
    fn temporal_context_soundness_refutes_an_unknown_zone() {
        // `Fake/Zone` passes the frontend's SHAPE law (axon-T892 sees a
        // plausible IANA form) — only the tz-database membership check the
        // proof carries can catch it before the runtime fails closed.
        let ir = ir_from_source(
            "flow Plan() -> Unit { step S { now: \"Fake/Zone\" ask: \"hi\" } }\n",
        );
        let proofs =
            super::generate::generate_temporal_context_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("tz database"), "{reason}");
                assert!(reason.contains("Fake/Zone"), "{reason}");
            }
            other => panic!("expected Refuted (unknown zone), got {other:?}"),
        }
    }

    #[test]
    fn temporal_context_soundness_refutes_a_shape_violation() {
        // Hand-craft an IR with a malformed zone (bypassing the compile-time
        // axon-T892 — a hand-edited or version-drifted deployment).
        let mut ir = ir_from_source(TEMPORAL_PROGRAM);
        ir.contexts[0].now_tz = Some("Bogota".to_string());
        let proofs =
            super::generate::generate_temporal_context_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T892"), "{reason}"),
            other => panic!("expected Refuted (shape violation), got {other:?}"),
        }
    }

    #[test]
    fn temporal_context_soundness_refutes_a_forged_witness() {
        // Forge: the witness claims a zone the artifact doesn't declare.
        let ir = ir_from_source(TEMPORAL_PROGRAM);
        let mut proofs =
            super::generate::generate_temporal_context_soundness_proofs(&ir, "test");
        if let Witness::TemporalContextSoundness(ref mut w) = proofs[0].witness {
            w.declarations
                .push(("step".to_string(), "Ghost".to_string(), "UTC".to_string()));
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("disagrees"), "{reason}"),
            other => panic!("expected Refuted (forgery), got {other:?}"),
        }
    }

    #[test]
    fn temporal_context_soundness_rides_generate_all_proofs() {
        let ir = ir_from_source(TEMPORAL_PROGRAM);
        let proofs = super::generate::generate_all_proofs(&ir, "test");
        assert!(
            proofs
                .iter()
                .any(|p| p.property == PropertyClass::TemporalContextSoundness),
            "self-contained → registered in the default generation set"
        );
    }

    // ── v2.46.0 — CredentialAttenuation ───────────────────────────────

    const CREDENTIAL_PROGRAM: &str = "credential WidgetSession {\n\
             ttl: 15m\n\
             grants: [chat.invoke]\n\
         }\n\
         flow Bootstrap() -> Unit {\n\
             mint WidgetSession as tok\n\
             step S { ask: \"hi\" }\n\
         }\n";

    #[test]
    fn credential_attenuation_round_trips_and_verifies() {
        let ir = ir_from_source(CREDENTIAL_PROGRAM);
        let proofs = super::generate::generate_credential_attenuation_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one whole-program proof when a contract exists");
        assert_eq!(proofs[0].property, PropertyClass::CredentialAttenuation);
        if let Witness::CredentialAttenuation(ref w) = proofs[0].witness {
            assert_eq!(
                w.contracts,
                vec![("WidgetSession".to_string(), 900, vec!["chat.invoke".to_string()])]
            );
            assert_eq!(
                w.mints,
                vec![(
                    "Bootstrap".to_string(),
                    "WidgetSession".to_string(),
                    "tok".to_string()
                )]
            );
            assert!(w.unresolved_mints.is_empty());
            assert!(w.invalid_contracts.is_empty());
        } else {
            panic!("expected a CredentialAttenuation witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn credential_less_program_carries_no_attenuation_proof() {
        let ir = ir_from_source("flow Chat() -> Unit { step S { ask: \"hi\" } }\n");
        assert!(
            super::generate::generate_credential_attenuation_proofs(&ir, "test").is_empty(),
            "no contract → no proof"
        );
    }

    #[test]
    fn credential_attenuation_refutes_an_unresolved_mint() {
        // Hand-craft an IR (bypassing compile-time axon-T895) where the mint
        // names a contract that doesn't exist.
        let mut ir = ir_from_source(CREDENTIAL_PROGRAM);
        ir.credentials.clear();
        let proofs = super::generate::generate_credential_attenuation_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T895"), "{reason}"),
            other => panic!("expected Refuted (unresolved mint), got {other:?}"),
        }
    }

    #[test]
    fn credential_attenuation_refutes_an_ill_formed_contract() {
        // Hand-craft an IR with a TTL above the ephemeral ceiling
        // (bypassing compile-time axon-T894).
        let mut ir = ir_from_source(CREDENTIAL_PROGRAM);
        ir.credentials[0].ttl_secs = 7 * 86_400;
        let proofs = super::generate::generate_credential_attenuation_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T894"), "{reason}"),
            other => panic!("expected Refuted (ill-formed contract), got {other:?}"),
        }
    }

    #[test]
    fn credential_attenuation_refutes_a_forged_witness() {
        let ir = ir_from_source(CREDENTIAL_PROGRAM);
        let mut proofs = super::generate::generate_credential_attenuation_proofs(&ir, "test");
        if let Witness::CredentialAttenuation(ref mut w) = proofs[0].witness {
            // Forge: claim a broader grant set than the artifact declares.
            w.contracts[0].2.push("tenant.update".to_string());
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("disagrees"), "{reason}"),
            other => panic!("expected Refuted (forgery), got {other:?}"),
        }
    }

    #[test]
    fn credential_attenuation_rides_generate_all_proofs() {
        let ir = ir_from_source(CREDENTIAL_PROGRAM);
        let proofs = super::generate::generate_all_proofs(&ir, "test");
        assert!(
            proofs
                .iter()
                .any(|p| p.property == PropertyClass::CredentialAttenuation),
            "self-contained → registered in the default generation set"
        );
    }

    // ── v2.48.0 — SecretCustodySoundness ──────────────────────────────

    const CUSTODY_PROGRAM: &str = "axonstore CrmTokens {\n\
             backend: secrets\n\
             class: crm\n\
         }\n\
         tool RefreshCrmToken {\n\
             endpoint: \"/tools/crm/refresh\"\n\
         }\n\
         flow RotateExpiring() -> Unit {\n\
             rotate CrmTokens where \"expires_at < now() + interval '10 minutes'\" with RefreshCrmToken as result\n\
             step S { ask: \"${result}\" }\n\
         }\n";

    #[test]
    fn secret_custody_round_trips_and_verifies() {
        let ir = ir_from_source(CUSTODY_PROGRAM);
        let proofs = super::generate::generate_secret_custody_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one whole-program proof when custody exists");
        assert_eq!(proofs[0].property, PropertyClass::SecretCustodySoundness);
        if let Witness::SecretCustodySoundness(ref w) = proofs[0].witness {
            assert_eq!(
                w.stores,
                vec![("CrmTokens".to_string(), "crm".to_string())]
            );
            assert_eq!(
                w.rotates,
                vec![(
                    "RotateExpiring".to_string(),
                    "CrmTokens".to_string(),
                    "RefreshCrmToken".to_string(),
                    "result".to_string()
                )]
            );
            assert!(w.unresolved_stores.is_empty());
            assert!(w.unresolved_tools.is_empty());
            assert!(w.invalid_classes.is_empty());
            assert!(w.write_violations.is_empty());
        } else {
            panic!("expected a SecretCustodySoundness witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn custody_less_program_carries_no_custody_proof() {
        let ir = ir_from_source("flow Chat() -> Unit { step S { ask: \"hi\" } }\n");
        assert!(
            super::generate::generate_secret_custody_proofs(&ir, "test").is_empty(),
            "no custody → no proof"
        );
    }

    #[test]
    fn secret_custody_refutes_a_ghost_rotation_tool() {
        // Hand-craft an IR (bypassing compile-time axon-T899) where the
        // rotate names a tool that doesn't exist.
        let mut ir = ir_from_source(CUSTODY_PROGRAM);
        ir.tools.clear();
        let proofs = super::generate::generate_secret_custody_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T899"), "{reason}"),
            other => panic!("expected Refuted (ghost tool), got {other:?}"),
        }
    }

    #[test]
    fn secret_custody_refutes_a_class_less_store() {
        // Hand-craft an IR with an empty class (bypassing axon-T900).
        let mut ir = ir_from_source(CUSTODY_PROGRAM);
        for s in &mut ir.axonstore_specs {
            if s.backend == "secrets" {
                s.class = String::new();
            }
        }
        let proofs = super::generate::generate_secret_custody_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T900"), "{reason}"),
            other => panic!("expected Refuted (class-less store), got {other:?}"),
        }
    }

    #[test]
    fn secret_custody_refutes_a_forged_witness() {
        let ir = ir_from_source(CUSTODY_PROGRAM);
        let mut proofs = super::generate::generate_secret_custody_proofs(&ir, "test");
        if let Witness::SecretCustodySoundness(ref mut w) = proofs[0].witness {
            // Forge: claim a broader class than the artifact declares.
            w.stores[0].1 = "llm".to_string();
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("disagrees"), "{reason}"),
            other => panic!("expected Refuted (forgery), got {other:?}"),
        }
    }

    #[test]
    fn secret_custody_rides_generate_all_proofs() {
        let ir = ir_from_source(CUSTODY_PROGRAM);
        let proofs = super::generate::generate_all_proofs(&ir, "test");
        assert!(
            proofs
                .iter()
                .any(|p| p.property == PropertyClass::SecretCustodySoundness),
            "self-contained → registered in the default generation set"
        );
    }

    // ── v2.39.0 — TechnicianCommandSafety ─────────────────────────────

    const TECH_PROGRAM: &str = concat!(
        "type Command { line: String }\n",
        "type CommandResult { stdout: String, stderr: String, exit_code: Int }\n",
        "type DenyReason { detail: String }\n",
        "session TechSafe {\n",
        "  server: [ send Command, receive CommandResult, end ]\n",
        "  client: [ receive Command, send CommandResult, end ]\n",
        "}\n",
        "session TechConfirm {\n",
        "  server: [ send Command, select { approved: [ receive CommandResult, end ], denied: [ receive DenyReason, end ] } ]\n",
        "  client: [ receive Command, branch { approved: [ send CommandResult, end ], denied: [ send DenyReason, end ] } ]\n",
        "}\n",
        "socket TechSafeWS { protocol: TechSafe }\n",
        "socket TechConfirmWS { protocol: TechConfirm }\n",
        "tool Ping { provider: bash target: TechSafeWS risk: safe parameters: { count: Int, host: String } argv: [\"ping\", \"-c\", \"${count}\", \"${host}\"] }\n",
        "tool DeleteFile { provider: bash target: TechConfirmWS risk: destructive parameters: { path: String } argv: [\"rm\", \"${path}\"] }\n",
    );

    #[test]
    fn technician_command_safety_round_trips_and_verifies() {
        let ir = ir_from_source(TECH_PROGRAM);
        let proofs = super::generate::generate_technician_command_safety_proofs(&ir, "test");
        assert_eq!(proofs.len(), 2, "one proof per target-bound tool");
        for p in &proofs {
            assert_eq!(p.property, PropertyClass::TechnicianCommandSafety);
            assert_eq!(check_proof(p, &ir), CheckOutcome::Verified);
        }
        // The destructive tool's witness records its reachable confirm branch.
        let del = proofs
            .iter()
            .find(|p| p.witness.subject_name() == "DeleteFile")
            .expect("DeleteFile proof");
        if let Witness::TechnicianCommandSafety(ref w) = del.witness {
            assert_eq!(w.risk, "destructive");
            assert!(w.confirm_branch_reachable);
            assert!(w.argv_present);
            assert!(w.unbound_placeholders.is_empty());
            assert!(w.partial_tokens.is_empty());
        } else {
            panic!("expected a TechnicianCommandSafety witness");
        }
    }

    #[test]
    fn non_technician_program_carries_no_technician_proof() {
        let ir = ir_from_source("tool WebSearch { provider: http max_results: 5 }\n");
        assert!(
            super::generate::generate_technician_command_safety_proofs(&ir, "test").is_empty(),
            "no target-bound tool → no proof"
        );
    }

    #[test]
    fn technician_command_safety_refutes_a_forged_witness() {
        let ir = ir_from_source(TECH_PROGRAM);
        let mut proofs = super::generate::generate_technician_command_safety_proofs(&ir, "test");
        if let Witness::TechnicianCommandSafety(ref mut w) = proofs[0].witness {
            w.argv.push("--forged".to_string());
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("disagrees"), "{reason}"),
            other => panic!("expected Refuted (forgery), got {other:?}"),
        }
    }

    #[test]
    fn technician_command_safety_refutes_an_unbound_placeholder() {
        // Hand-craft an IR (bypassing axon-T859) where the argv references a
        // placeholder that is not a declared parameter.
        let mut ir = ir_from_source(TECH_PROGRAM);
        let ping = ir.tools.iter_mut().find(|t| t.name == "Ping").unwrap();
        ping.argv = vec!["ping".to_string(), "${ghost}".to_string()];
        let proofs = super::generate::generate_technician_command_safety_proofs(&ir, "test");
        let ping_proof = proofs
            .iter()
            .find(|p| p.witness.subject_name() == "Ping")
            .unwrap();
        match check_proof(ping_proof, &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T859"), "{reason}"),
            other => panic!("expected Refuted (unbound placeholder), got {other:?}"),
        }
    }

    #[test]
    fn technician_command_safety_refutes_destructive_without_branch() {
        // Hand-craft an IR (bypassing axon-T860) where a destructive tool is
        // re-pointed to the confirmation-less socket.
        let mut ir = ir_from_source(TECH_PROGRAM);
        let del = ir.tools.iter_mut().find(|t| t.name == "DeleteFile").unwrap();
        del.target = Some("TechSafeWS".to_string());
        let proofs = super::generate::generate_technician_command_safety_proofs(&ir, "test");
        let del_proof = proofs
            .iter()
            .find(|p| p.witness.subject_name() == "DeleteFile")
            .unwrap();
        match check_proof(del_proof, &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T860"), "{reason}"),
            other => panic!("expected Refuted (destructive w/o branch), got {other:?}"),
        }
    }

    // ── v2.40.0 — CacheSoundness ──────────────────────────────────────

    const CACHE_PROGRAM: &str = concat!(
        "flow Chat() -> Unit { step S { ask: \"hi\" } }\n",
        "type WeatherEvent { city: String }\n",
        "channel WeatherUpdated { message: WeatherEvent }\n",
        "tool Fingerprint { provider: http effects: <pure> parameters: { input: String } }\n",
        "tool Weather { provider: http effects: <network> parameters: { city: String } cache: WeatherCache }\n",
        "cache DefaultPure { default: true }\n",
        "cache WeatherCache { backend: redis ttl: 5m apply_to_effects: [pure, network] invalidate_on: [WeatherUpdated] }\n",
    );

    #[test]
    fn cache_soundness_round_trips_and_verifies() {
        let ir = ir_from_source(CACHE_PROGRAM);
        let proofs = super::generate::generate_cache_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one whole-program cache proof");
        assert_eq!(proofs[0].property, PropertyClass::CacheSoundness);
        if let Witness::CacheSoundness(ref w) = proofs[0].witness {
            assert_eq!(w.default_count, 1);
            assert!(w.widened_without_ttl.is_empty());
            assert!(w.unresolved_refs.is_empty());
        } else {
            panic!("expected a CacheSoundness witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn cache_less_program_carries_no_proof() {
        let ir = ir_from_source("flow Chat() -> Unit { step S { ask: \"hi\" } }\n");
        assert!(
            super::generate::generate_cache_soundness_proofs(&ir, "test").is_empty(),
            "no contract → no proof"
        );
    }

    #[test]
    fn cache_soundness_refutes_two_defaults() {
        let mut ir = ir_from_source(CACHE_PROGRAM);
        if let Some(wc) = ir.caches.iter_mut().find(|c| c.name == "WeatherCache") {
            wc.default_policy = true;
        }
        let proofs = super::generate::generate_cache_soundness_proofs(&ir, "test");
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T863"), "{reason}"),
            other => panic!("expected Refuted (two defaults), got {other:?}"),
        }
    }

    #[test]
    fn cache_soundness_refutes_widened_without_ttl() {
        let mut ir = ir_from_source(CACHE_PROGRAM);
        if let Some(wc) = ir.caches.iter_mut().find(|c| c.name == "WeatherCache") {
            wc.ttl = None;
        }
        let proofs = super::generate::generate_cache_soundness_proofs(&ir, "test");
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T865"), "{reason}"),
            other => panic!("expected Refuted (widened w/o ttl), got {other:?}"),
        }
    }

    #[test]
    fn cache_soundness_refutes_unresolved_reference() {
        let mut ir = ir_from_source(CACHE_PROGRAM);
        if let Some(t) = ir.tools.iter_mut().find(|t| t.name == "Weather") {
            t.cache = "Ghost".to_string();
        }
        let proofs = super::generate::generate_cache_soundness_proofs(&ir, "test");
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T864"), "{reason}"),
            other => panic!("expected Refuted (unresolved ref), got {other:?}"),
        }
    }

    // ── v2.52.0 — ScrapeProvenanceSoundness ───────────────────────────

    const SCRAPE_PROGRAM: &str = concat!(
        "type RawPage { status: Int, body: String }\n",
        "type Summary { text: String }\n",
        "persona Analyst { domain: [\"news\"] }\n",
        "shield NewsShield { scan: [prompt_injection] on_breach: quarantine severity: high }\n",
        "tool FetchNews { provider: scrape_http parameters: { url: String } ",
        "output_type: RawPage effects: <network, web> scrape: { engine: impersonate } }\n",
        "flow Digest() -> Summary {\n",
        "  use FetchNews(url = \"https://ex.com/news\")\n",
        "  shield NewsShield on page -> RawPage\n",
        "  step Summarize { given: Digest ask: \"Summarize\" output: Summary }\n",
        "}\n",
    );

    #[test]
    fn scrape_provenance_round_trips_and_verifies() {
        let ir = ir_from_source(SCRAPE_PROGRAM);
        let proofs = super::generate::generate_scrape_provenance_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one whole-program scrape proof");
        assert_eq!(proofs[0].property, PropertyClass::ScrapeProvenanceSoundness);
        if let Witness::ScrapeProvenanceSoundness(ref w) = proofs[0].witness {
            assert_eq!(w.scrape_tools.len(), 1);
            assert!(w.tools_missing_web.is_empty());
            assert!(w.dom_tools_with_network.is_empty());
            assert!(w.unshielded_flows.is_empty(), "shielded flow → no barrier violation");
        } else {
            panic!("expected a ScrapeProvenanceSoundness witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn scrape_less_program_carries_no_proof() {
        let ir = ir_from_source("flow Chat() -> Unit { step S { ask: \"hi\" } }\n");
        assert!(
            super::generate::generate_scrape_provenance_soundness_proofs(&ir, "test").is_empty(),
            "no scrape tool → no proof"
        );
    }

    #[test]
    fn scrape_provenance_refutes_missing_web() {
        // Tamper the stored IR: strip `web` from the scrape tool's effect row.
        let mut ir = ir_from_source(SCRAPE_PROGRAM);
        if let Some(t) = ir.tools.iter_mut().find(|t| t.name == "FetchNews") {
            t.effect_row.retain(|e| e != "web");
        }
        let proofs = super::generate::generate_scrape_provenance_soundness_proofs(&ir, "test");
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T904"), "{reason}"),
            other => panic!("expected Refuted (missing web), got {other:?}"),
        }
    }

    #[test]
    fn scrape_provenance_refutes_unshielded_flow() {
        // Tamper: drop the shield step, leaving web content → belief unscanned.
        let mut ir = ir_from_source(SCRAPE_PROGRAM);
        for flow in ir.flows.iter_mut() {
            flow.steps
                .retain(|s| !matches!(s, crate::ir_nodes::IRFlowNode::ShieldApply(_)));
        }
        let proofs = super::generate::generate_scrape_provenance_soundness_proofs(&ir, "test");
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T908"), "{reason}"),
            other => panic!("expected Refuted (content-injection barrier), got {other:?}"),
        }
    }

    // ── v2.53.0 — DocumentProvenanceSoundness ─────────────────────────

    const DOC_PROGRAM: &str = concat!(
        "document quarterly_report {\n",
        "  target: docx\n",
        "  provenance: embedded\n",
        "  effects: <io, storage>\n",
        "  section {\n",
        "    heading: \"Q3\"\n",
        "    para { text: \"Audited.\" }\n",
        "    para { text: revenue_summary  attribute: analyst_agent }\n",
        "  }\n",
        "}\n",
    );

    #[test]
    fn document_provenance_round_trips_and_verifies() {
        let ir = ir_from_source(DOC_PROGRAM);
        let proofs = super::generate::generate_document_provenance_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one whole-program document proof");
        assert_eq!(proofs[0].property, PropertyClass::DocumentProvenanceSoundness);
        if let Witness::DocumentProvenanceSoundness(ref w) = proofs[0].witness {
            assert_eq!(w.documents.len(), 1);
            assert!(w.bad_targets.is_empty());
            assert!(w.sensitive_without_legal.is_empty());
            assert!(w.unattributed_slots.is_empty(), "attributed ⇒ no barrier violation");
        } else {
            panic!("expected a DocumentProvenanceSoundness witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn document_less_program_carries_no_proof() {
        let ir = ir_from_source("flow Chat() -> Unit { step S { ask: \"hi\" } }\n");
        assert!(
            super::generate::generate_document_provenance_soundness_proofs(&ir, "test").is_empty(),
            "no document → no proof"
        );
    }

    #[test]
    fn document_provenance_refutes_unattributed_slot() {
        // Tamper the stored IR: strip the `attribute` field, laundering an
        // unattributed flow value into an assertive slot.
        let mut ir = ir_from_source(DOC_PROGRAM);
        for d in ir.documents.iter_mut() {
            for section in d.blocks.iter_mut() {
                for block in section.children.iter_mut() {
                    block.fields.retain(|f| f.name != "attribute");
                }
            }
        }
        let proofs = super::generate::generate_document_provenance_soundness_proofs(&ir, "test");
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T916"), "{reason}"),
            other => panic!("expected Refuted (assertion-laundering barrier), got {other:?}"),
        }
    }

    // ── v2.60.0 — DeliveryProvenanceSoundness ──────────────────────────

    const DELIVER_PROGRAM: &str = concat!(
        "deliver push_lead {\n",
        "  target: crm\n",
        "  provenance: attached\n",
        "  secret: crm_api_key\n",
        "  effects: <web>\n",
        "  upsert_contact { key: resolved_email  email: resolved_email }\n",
        "}\n",
    );

    #[test]
    fn delivery_provenance_round_trips_and_verifies() {
        let ir = ir_from_source(DELIVER_PROGRAM);
        let proofs = super::generate::generate_delivery_provenance_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one whole-program delivery proof");
        assert_eq!(proofs[0].property, PropertyClass::DeliveryProvenanceSoundness);
        if let Witness::DeliveryProvenanceSoundness(ref w) = proofs[0].witness {
            assert_eq!(w.deliveries.len(), 1);
            assert!(w.bad_targets.is_empty());
            assert!(w.sensitive_without_legal.is_empty());
            assert!(w.laundered_deliveries.is_empty(), "attached ⇒ no barrier violation");
        } else {
            panic!("expected a DeliveryProvenanceSoundness witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn delivery_less_program_carries_no_proof() {
        let ir = ir_from_source("flow Chat() -> Unit { step S { ask: \"hi\" } }\n");
        assert!(
            super::generate::generate_delivery_provenance_soundness_proofs(&ir, "test").is_empty(),
            "no deliver → no proof"
        );
    }

    #[test]
    fn delivery_provenance_refutes_laundered_cleared_delivery() {
        // Tamper the stored IR: flip provenance to `cleared`, laundering a flow
        // value into the CRM as a bare fact with no epistemic vouch.
        let mut ir = ir_from_source(DELIVER_PROGRAM);
        for d in ir.deliveries.iter_mut() {
            d.provenance = "cleared".to_string();
        }
        let proofs = super::generate::generate_delivery_provenance_soundness_proofs(&ir, "test");
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T920"), "{reason}"),
            other => panic!("expected Refuted (provenance-stripping barrier), got {other:?}"),
        }
    }

    // ── v2.66.0 — NotificationProvenanceSoundness ────────────────────

    const NOTIFY_PROGRAM: &str = concat!(
        "notify LowSales {\n",
        "  channel: sms\n",
        "  to: secret(ops.oncall_phone)\n",
        "  template: \"Ventas 7d: ${resumen}\"\n",
        "  window: 4h\n",
        "  provenance: attached\n",
        "  effects: <web>\n",
        "}\n",
    );

    #[test]
    fn notification_round_trips_and_verifies() {
        let ir = ir_from_source(NOTIFY_PROGRAM);
        let proofs =
            super::generate::generate_notification_provenance_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        assert_eq!(
            proofs[0].property,
            PropertyClass::NotificationProvenanceSoundness
        );
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn notification_refutes_a_hand_edited_laundering() {
        // Forge: clear the provenance on a slot-binding notify with no vouch.
        let mut ir = ir_from_source(NOTIFY_PROGRAM);
        ir.notifications[0].provenance = "cleared".to_string();
        let proofs =
            super::generate::generate_notification_provenance_soundness_proofs(&ir, "test");
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("laundered"), "{reason}")
            }
            other => panic!("expected Refuted (laundered notification), got {other:?}"),
        }
    }

    #[test]
    fn notification_refutes_a_dropped_window() {
        let mut ir = ir_from_source(NOTIFY_PROGRAM);
        ir.notifications[0].window = String::new();
        let proofs =
            super::generate::generate_notification_provenance_soundness_proofs(&ir, "test");
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("windows"), "{reason}")
            }
            other => panic!("expected Refuted (unbounded interruption), got {other:?}"),
        }
    }

    // ── v2.65.0 — GradientSoundness (the proof-carrying derivative) ──

    const GRAD_PROGRAM: &str = concat!(
        "flow Score(x: Float, y: Float) -> Text {\n",
        "  let total = 3.0 * x + y * y\n",
        "  grad total wrt [x, y] as g\n",
        "  return g\n",
        "}\n",
    );

    #[test]
    fn gradient_round_trips_and_verifies() {
        let ir = ir_from_source(GRAD_PROGRAM);
        let proofs = super::generate::generate_gradient_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one whole-program gradient proof");
        assert_eq!(proofs[0].property, PropertyClass::GradientSoundness);
        if let Witness::GradientSoundness(ref w) = proofs[0].witness {
            assert_eq!(w.grads.len(), 1);
            assert_eq!(w.grads[0].1, "total");
            assert_eq!(w.grads[0].2, "x,y");
            assert!(w.violations.is_empty(), "{:?}", w.violations);
        } else {
            panic!("expected a GradientSoundness witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn gradient_refutes_a_hand_edited_derivative() {
        // Forge: swap a stored derivative for a flattering constant. The
        // checker re-differentiates the original and refutes.
        let mut ir = ir_from_source(GRAD_PROGRAM);
        for flow in &mut ir.flows {
            for step in &mut flow.steps {
                if let crate::ir_nodes::IRFlowNode::Grad(g) = step {
                    g.derivatives[0] = crate::ir_nodes::IRExpr::Lit {
                        lit: crate::ir_nodes::IRExprLit::Float { value: 999.0 },
                    };
                }
            }
        }
        let proofs = super::generate::generate_gradient_soundness_proofs(&ir, "test");
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("derivative-mismatch"), "{reason}")
            }
            other => panic!("expected Refuted (forged gradient), got {other:?}"),
        }
    }

    #[test]
    fn grad_less_program_carries_no_gradient_proof() {
        let ir = ir_from_source("flow Chat() -> Unit { step S { ask: \"hi\" } }\n");
        assert!(
            super::generate::generate_gradient_soundness_proofs(&ir, "test").is_empty(),
            "no grad step → no proof"
        );
    }

    // ── v2.63.0 — DataspaceSchemaSoundness (T928/T930, made a proof) ──

    const DATASPACE_PROGRAM: &str = concat!(
        "dataspace Sales {\n",
        "  column region: Text\n",
        "  column amount: Float\n",
        "}\n",
        "flow Report(raw: Text) -> Text {\n",
        "  ingest raw into Sales { format: csv }\n",
        "  aggregate Sales { group_by: [region], compute: [count, sum(amount)], as: r }\n",
        "  return r\n",
        "}\n",
    );

    #[test]
    fn dataspace_schema_round_trips_and_verifies() {
        let ir = ir_from_source(DATASPACE_PROGRAM);
        let proofs =
            super::generate::generate_dataspace_schema_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one whole-program dataspace-schema proof");
        assert_eq!(proofs[0].property, PropertyClass::DataspaceSchemaSoundness);
        if let Witness::DataspaceSchemaSoundness(ref w) = proofs[0].witness {
            assert_eq!(w.dataspaces.len(), 1);
            assert_eq!(w.dataspaces[0].0, "Sales");
            assert!(w.violations.is_empty(), "{:?}", w.violations);
        } else {
            panic!("expected a DataspaceSchemaSoundness witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn dataspace_schema_refutes_a_hand_edited_ghost_column() {
        // A stale/hand-edited artifact whose aggregate reads a column the
        // schema never declared MUST be refuted at deploy — the T930 law,
        // re-derived. Forge it by mutating the compiled IR.
        let mut ir = ir_from_source(DATASPACE_PROGRAM);
        for flow in &mut ir.flows {
            for step in &mut flow.steps {
                if let crate::ir_nodes::IRFlowNode::Aggregate(a) = step {
                    a.group_by = vec!["ghost_region".to_string()];
                }
            }
        }
        let proofs =
            super::generate::generate_dataspace_schema_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("ghost-column"), "{reason}")
            }
            other => panic!("expected Refuted (ghost column), got {other:?}"),
        }
    }

    #[test]
    fn dataspace_less_program_carries_no_schema_proof() {
        let ir = ir_from_source("flow Chat() -> Unit { step S { ask: \"hi\" } }\n");
        assert!(
            super::generate::generate_dataspace_schema_soundness_proofs(&ir, "test")
                .is_empty(),
            "no dataspace surface → no proof"
        );
    }

    // ── v2.62.0 — QuerySafetySoundness (RFC 10008 section 2, made a proof) ────

    const QUERY_PROGRAM: &str = concat!(
        "axonstore mem { backend: in_memory }\n",
        "flow Search() -> Unit {\n",
        "  retrieve mem { where: \"kind = 'lead'\" as: hits }\n",
        "}\n",
        "axonendpoint E { method: QUERY path: \"/search\" execute: Search backend: stub }\n",
    );

    #[test]
    fn query_safety_round_trips_and_verifies() {
        let ir = ir_from_source(QUERY_PROGRAM);
        let proofs = super::generate::generate_query_safety_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one whole-program QUERY-safety proof");
        assert_eq!(proofs[0].property, PropertyClass::QuerySafetySoundness);
        if let Witness::QuerySafetySoundness(ref w) = proofs[0].witness {
            assert_eq!(w.query_endpoints.len(), 1);
            assert!(w.unsafe_queries.is_empty(), "a read-only QUERY is safe");
            assert!(w.egress_declarations.is_empty());
        } else {
            panic!("expected a QuerySafetySoundness witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn query_less_program_carries_no_proof() {
        let ir = ir_from_source("flow Chat() -> Unit { step S { ask: \"hi\" } }\n");
        assert!(
            super::generate::generate_query_safety_soundness_proofs(&ir, "test").is_empty(),
            "no QUERY endpoint → no proof"
        );
    }

    #[test]
    fn query_safety_refutes_a_write_smuggled_into_a_safe_method() {
        // Tamper the stored IR: append a `persist` to the QUERY's flow — the exact
        // attack the class exists to stop (a write hiding behind a method that
        // caches and proxies are entitled to retry).
        let mut ir = ir_from_source(QUERY_PROGRAM);
        let persist = ir_from_source(
            "axonstore mem { backend: in_memory }\n\
             flow W() -> Unit { persist into mem { kind: \"x\" content: \"y\" } }\n",
        )
        .flows
        .iter()
        .find(|f| f.name == "W")
        .expect("W")
        .steps
        .first()
        .expect("the persist node")
        .clone();
        for f in ir.flows.iter_mut() {
            if f.name == "Search" {
                f.steps.push(persist.clone());
            }
        }
        let proofs = super::generate::generate_query_safety_soundness_proofs(&ir, "test");
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T927"), "{reason}"),
            other => panic!("expected Refuted (QUERY-safety), got {other:?}"),
        }
    }

    // ── v2.54.0 — DocumentIngestionSoundness ─────────────────────────

    const INGEST_PROGRAM: &str = concat!(
        "type Doc { text: String }\n",
        "type Summary { text: String }\n",
        "tool DocReader { provider: native parameters: { path: String } output_type: Doc ",
        "effects: <io, ingest:parsed> }\n",
        "persona Analyst { domain: [\"docs\"] }\n",
        "shield DocShield { scan: [prompt_injection] on_breach: quarantine severity: high }\n",
        "flow Read() -> Summary {\n",
        "  use DocReader(path = \"in.docx\")\n",
        "  shield DocShield on doc -> Doc\n",
        "  step Summarize { given: Read ask: \"Summarize\" output: Summary }\n",
        "}\n",
    );

    #[test]
    fn document_ingestion_round_trips_and_verifies() {
        let ir = ir_from_source(INGEST_PROGRAM);
        let proofs = super::generate::generate_document_ingestion_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one whole-program ingestion proof");
        assert_eq!(proofs[0].property, PropertyClass::DocumentIngestionSoundness);
        if let Witness::DocumentIngestionSoundness(ref w) = proofs[0].witness {
            assert_eq!(w.ingest_tools.len(), 1);
            assert_eq!(w.ingest_tools[0].1, "parsed");
            assert!(w.inferred_ceiling_violations.is_empty());
            assert!(w.unshielded_flows.is_empty(), "shielded flow → no barrier violation");
        } else {
            panic!("expected a DocumentIngestionSoundness witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn ingestion_less_program_carries_no_proof() {
        let ir = ir_from_source("flow Chat() -> Unit { step S { ask: \"hi\" } }\n");
        assert!(
            super::generate::generate_document_ingestion_soundness_proofs(&ir, "test").is_empty(),
            "no ingesting tool → no proof"
        );
    }

    #[test]
    fn no_inferred_producer_ships_in_this_fase() {
        // the design decision — the vacuum: the built-in stdlib catalog has NO tool that
        // declares `ingest:inferred`, so the Inferred ceiling (T1001) is
        // vacuously satisfied until v2.54.0 adds a producer.
        let has_inferred = crate::stdlib::TOOLS.iter().any(|t| {
            // native tools carry no declared ingest class in the catalog; the
            // OSS `DocumentReader` produces Parsed only (asserted in ooxml_read).
            t.name == "DocumentReaderInferred" // no such tool exists
        });
        assert!(!has_inferred, "no built-in Inferred producer may ship");
    }

    #[test]
    fn ingestion_refutes_unshielded_flow() {
        // Tamper: drop the shield, leaving ingested content → belief unscanned.
        let mut ir = ir_from_source(INGEST_PROGRAM);
        for flow in ir.flows.iter_mut() {
            flow.steps
                .retain(|s| !matches!(s, crate::ir_nodes::IRFlowNode::ShieldApply(_)));
        }
        let proofs = super::generate::generate_document_ingestion_soundness_proofs(&ir, "test");
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T908"), "{reason}"),
            other => panic!("expected Refuted (ingestion barrier), got {other:?}"),
        }
    }

    // ── v2.54.0 — InferredCeilingSoundness ───────────────────────────

    // An `ingest:inferred` PRODUCER (an OCR reader) — the state v2.54.0 forbade
    // and v2.54.0 creates. It declares NO `epistemic:know` (ceiling holds)
    // and its output is shielded before reasoning.
    const INFER_PROGRAM: &str = concat!(
        "type Doc { text: String }\n",
        "type Summary { text: String }\n",
        "tool OcrReader { provider: native parameters: { img: String } output_type: Doc ",
        "effects: <io, ingest:inferred> }\n",
        "persona Analyst { domain: [\"docs\"] }\n",
        "shield DocShield { scan: [prompt_injection] on_breach: quarantine severity: high }\n",
        "flow Read() -> Summary {\n",
        "  use OcrReader(img = \"scan.png\")\n",
        "  shield DocShield on doc -> Doc\n",
        "  step Summarize { given: Read ask: \"Summarize\" output: Summary }\n",
        "}\n",
    );

    #[test]
    fn inferred_ceiling_round_trips_and_verifies() {
        let ir = ir_from_source(INFER_PROGRAM);
        let proofs = super::generate::generate_inferred_ceiling_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one whole-program inferred-ceiling proof");
        assert_eq!(proofs[0].property, PropertyClass::InferredCeilingSoundness);
        if let Witness::InferredCeilingSoundness(ref w) = proofs[0].witness {
            assert_eq!(w.inferred_producers, vec!["OcrReader".to_string()]);
            assert!(w.ceiling_violations.is_empty(), "no epistemic:know → ceiling holds");
            assert!(w.unshielded_flows.is_empty(), "shielded flow → no barrier violation");
        } else {
            panic!("expected an InferredCeilingSoundness witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn parsed_only_program_carries_no_inferred_ceiling_proof() {
        // The v2.54.0 vacuum: a program with only `ingest:parsed` tools owes NO
        // inferred-ceiling proof (no producer exists).
        let ir = ir_from_source(INGEST_PROGRAM);
        assert!(
            super::generate::generate_inferred_ceiling_soundness_proofs(&ir, "test").is_empty(),
            "no `ingest:inferred` producer → no proof (the ingestion vacuum holds)"
        );
    }

    #[test]
    fn inferred_ceiling_refutes_epistemic_know() {
        // Tamper: inject `epistemic:know` into the inferred producer's effect row.
        // A re-derivation catches T1001 — an inferred read can never be `know`.
        let mut ir = ir_from_source(INFER_PROGRAM);
        for t in ir.tools.iter_mut() {
            if t.name == "OcrReader" {
                t.effect_row.push("epistemic:know".to_string());
            }
        }
        let proofs = super::generate::generate_inferred_ceiling_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T1001"), "{reason}"),
            other => panic!("expected Refuted (ceiling), got {other:?}"),
        }
    }

    #[test]
    fn inferred_ceiling_refutes_unshielded_flow() {
        // Tamper: drop the shield — an inferred read reaches belief unscanned.
        let mut ir = ir_from_source(INFER_PROGRAM);
        for flow in ir.flows.iter_mut() {
            flow.steps
                .retain(|s| !matches!(s, crate::ir_nodes::IRFlowNode::ShieldApply(_)));
        }
        let proofs = super::generate::generate_inferred_ceiling_soundness_proofs(&ir, "test");
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T908"), "{reason}"),
            other => panic!("expected Refuted (barrier), got {other:?}"),
        }
    }

    // ── v2.41.0 — ForgeSoundness ──────────────────────────────────────

    const FORGE_PROGRAM: &str = concat!(
        "anchor GoldenRatio { require: aesthetic_harmony confidence_floor: 0.70 }\n",
        "flow CreateVisualConcept(brief: String) -> Visual {\n",
        "  forge Artwork(seed: \"aurora borealis over ancient ruins\") -> Visual {\n",
        "    mode: transformational\n",
        "    novelty: 0.85\n",
        "    constraints: GoldenRatio\n",
        "    depth: 4\n",
        "    branches: 7\n",
        "  }\n",
        "}\n",
    );

    #[test]
    fn forge_soundness_round_trips_and_verifies() {
        let ir = ir_from_source(FORGE_PROGRAM);
        let proofs = super::generate::generate_forge_soundness_proofs(&ir, "test");
        assert_eq!(proofs.len(), 1, "one proof per forge block");
        assert_eq!(proofs[0].property, PropertyClass::ForgeSoundness);
        if let Witness::ForgeSoundness(ref w) = proofs[0].witness {
            assert_eq!(w.forge_name, "Artwork");
            assert_eq!(w.novelty_milli, 850);
            assert!(w.mode_ok && w.novelty_in_range && w.bounds_ok);
            assert!(w.seed_and_type_present && w.constraints_ok);
        } else {
            panic!("expected a ForgeSoundness witness");
        }
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn forge_less_program_carries_no_proof() {
        let ir = ir_from_source("flow F() -> Unit { step S { ask: \"hi\" } }\n");
        assert!(
            super::generate::generate_forge_soundness_proofs(&ir, "test").is_empty(),
            "no forge → no proof"
        );
    }

    #[test]
    fn forge_soundness_refutes_a_forged_witness() {
        let ir = ir_from_source(FORGE_PROGRAM);
        let mut proofs = super::generate::generate_forge_soundness_proofs(&ir, "test");
        if let Witness::ForgeSoundness(ref mut w) = proofs[0].witness {
            w.novelty_milli = 1500; // claim a novelty the artifact doesn't have
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("disagrees"), "{reason}"),
            other => panic!("expected Refuted (forgery), got {other:?}"),
        }
    }

    // ── v2.37.0 — UpstreamProjectionSoundness ───────────────────────────

    const STT_UPSTREAM: &str = "session SttDialogue {\n\
             axon:   [ send AudioChunk, receive Transcript, loop ]\n\
             vendor: [ receive AudioChunk, send Transcript, loop ]\n\
         }\n\
         upstream DeepgramSTT {\n\
             transport: websocket\n\
             protocol: SttDialogue\n\
             role: axon\n\
             resolve: upstream.deepgram.url\n\
             secret: upstream.deepgram.api_key\n\
             auth: header(\"Authorization\", \"Token \")\n\
             map: [\n\
                 send AudioChunk as binary,\n\
                 receive Transcript as json when \"type\" = \"Results\",\n\
             ]\n\
             reconnect: { backoff_ms: 500, max_attempts: 5, on_exhausted: fail }\n\
             overflow: drop_oldest\n\
         }";

    #[test]
    fn upstream_projection_soundness_verifies_a_total_projection() {
        let ir = ir_from_source(STT_UPSTREAM);
        let proofs = super::generate::generate_upstream_projection_soundness_proofs(&ir, VERSION);
        assert_eq!(proofs.len(), 1, "one proof per upstream");
        assert_eq!(proofs[0].property.slug(), "upstream_projection_soundness");
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
        // …and it rides the all-proofs bundle (`axon pcc prove`).
        assert!(generate_all_proofs(&ir, VERSION)
            .iter()
            .any(|p| p.property == PropertyClass::UpstreamProjectionSoundness));
    }

    #[test]
    fn upstream_projection_refutes_a_partial_projection() {
        // No receive rule for Transcript — a message would cross the
        // boundary untranscoded. (No type-check ran, so the defect
        // survives to the independent checker — which is the point.)
        let ir = ir_from_source(&STT_UPSTREAM.replace(
            "receive Transcript as json when \"type\" = \"Results\",\n",
            "",
        ));
        let proofs = super::generate::generate_upstream_projection_soundness_proofs(&ir, VERSION);
        assert_eq!(proofs.len(), 1, "the defective upstream still gets its refutable proof");
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("untranscoded") && reason.contains("T849"), "{reason}")
            }
            other => panic!("expected Refuted (partial projection), got {other:?}"),
        }
    }

    #[test]
    fn upstream_projection_refutes_a_literal_shaped_secret() {
        // An uppercase dotted path parses, but production custody would
        // reject it — the T850 law re-derived by the checker.
        let ir = ir_from_source(&STT_UPSTREAM.replace(
            "secret: upstream.deepgram.api_key",
            "secret: Upstream.Deepgram.ApiKey",
        ));
        let proofs = super::generate::generate_upstream_projection_soundness_proofs(&ir, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("T850"), "{reason}"),
            other => panic!("expected Refuted (literal-shaped key), got {other:?}"),
        }
    }

    #[test]
    fn upstream_projection_refutes_a_forged_witness() {
        let ir = ir_from_source(&STT_UPSTREAM.replace(
            "receive Transcript as json when \"type\" = \"Results\",\n",
            "",
        ));
        let mut proofs = super::generate::generate_upstream_projection_soundness_proofs(&ir, VERSION);
        // Forge: claim totality for the partial projection.
        if let Witness::UpstreamProjectionSoundness(ref mut w) = proofs[0].witness {
            w.projection_total = true;
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("disagrees"), "{reason}"),
            other => panic!("expected Refuted (forgery), got {other:?}"),
        }
    }

    // ── v2.45.0 — CapabilityGrantability (every_requirement_is_grantable) ──

    /// A representative RBAC catalog (colon) + the reserved dotted caps —
    /// the grantable authority manifest the enterprise supplies.
    fn catalog() -> Vec<String> {
        [
            "flow:execute", "flow:deploy", "tenant:update", "secret:read", "secret:write",
            "warden:execute", "tech:dispatch", "tech:approve", "daemon:run",
            "store.platform_read", "store.platform_write",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn grantability_verifies_when_requires_projects_from_the_catalog() {
        let mut ir = empty_ir();
        // requires the DOTTED `flow.execute` — grantable because the catalog
        // has the colon `flow:execute`, which π projects to it.
        ir.endpoints
            .push(endpoint_requires("Deploy", "DeployFlow", &["flow.execute"]));
        let proofs = generate_capability_grantability_proofs(&ir, &catalog(), VERSION);
        assert_eq!(proofs.len(), 1, "one whole-program grantability proof");
        assert_eq!(proofs[0].property.slug(), "capability_grantability");
        assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
    }

    #[test]
    fn grantability_refutes_the_briefs_dead_requirement() {
        // Kivi brief #55: `requires: [tenant.write]` — the catalog has
        // `tenant:update`, NOT `tenant:write`. A dead boundary → axon-T891.
        let mut ir = empty_ir();
        ir.endpoints
            .push(endpoint_requires("Write", "WriteFlow", &["tenant.write"]));
        let proofs = generate_capability_grantability_proofs(&ir, &catalog(), VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("axon-T891"), "{reason}");
                assert!(reason.contains("tenant.write"), "{reason}");
            }
            other => panic!("expected Refuted (dead requirement), got {other:?}"),
        }
    }

    #[test]
    fn grantability_refutes_a_fractured_catalog() {
        // If the catalog gained a `store:platform_read` colon perm it would
        // collide with the reserved dotted `store.platform_read` under π.
        let mut ir = empty_ir();
        ir.endpoints
            .push(endpoint_requires("Plat", "PlatFlow", &["store.platform_read"]));
        let mut authorities = catalog();
        authorities.push("store:platform_read".to_string());
        let proofs = generate_capability_grantability_proofs(&ir, &authorities, VERSION);
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => {
                assert!(reason.contains("fractured"), "{reason}");
                assert!(reason.contains("store.platform_read"), "{reason}");
            }
            other => panic!("expected Refuted (fracture), got {other:?}"),
        }
    }

    #[test]
    fn grantability_refutes_a_forged_requires_set() {
        // Forge: drop the dead requirement from the witness to fake
        // all_grantable. The checker re-derives `required` from the IR and
        // catches the disagreement.
        let mut ir = empty_ir();
        ir.endpoints
            .push(endpoint_requires("Write", "WriteFlow", &["tenant.write"]));
        let mut proofs = generate_capability_grantability_proofs(&ir, &catalog(), VERSION);
        if let Witness::CapabilityGrantability(ref mut w) = proofs[0].witness {
            w.required = Vec::new();
            w.all_grantable = true;
        }
        match check_proof(&proofs[0], &ir) {
            CheckOutcome::Refuted { reason } => assert!(reason.contains("disagrees"), "{reason}"),
            other => panic!("expected Refuted (forged requires-set), got {other:?}"),
        }
    }

    #[test]
    fn grantability_emits_no_proof_without_requirements() {
        // A program whose dispatching endpoints declare no `requires:` has
        // no grantability obligation.
        let mut ir = empty_ir();
        ir.endpoints
            .push(endpoint_requires("Public", "PublicFlow", &[]));
        let proofs = generate_capability_grantability_proofs(&ir, &catalog(), VERSION);
        assert!(proofs.is_empty(), "no requires → no grantability proof");
    }
}
