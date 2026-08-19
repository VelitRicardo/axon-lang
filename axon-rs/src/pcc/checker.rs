//! v2.4.0 — the INDEPENDENT proof checker (the consumer side).
//!
//! This is the trust boundary of Proof-Carrying Code. A consumer who
//! does NOT trust the axon compiler runs [`check_proof`] against the
//! artifact + the proof term and gets an unforgeable verdict. The
//! checker is small, total, and never-panics on arbitrary proof input
//! — it is the only piece a verifier must audit.
//!
//! ## the design decision — independence (the soundness contract)
//!
//! The checker RE-DERIVES the property from the artifact and verifies
//! the witness against that re-derivation. It NEVER accepts a witness
//! field on faith. A forged witness — one claiming `shield_present:
//! true` for an endpoint whose `shield_ref` is empty, or omitting a
//! phantom class from `unknown_classes` — is rejected because the
//! checker recomputes those facts from the IR and compares. The
//! adversarial tests (`forged_*_rejected`) are first-class gates.
//!
//! ## the design decision — digest binding
//!
//! Before checking any property the checker recomputes the artifact
//! digest and rejects a mismatch ([`CheckOutcome::DigestMismatch`]).
//! A proof minted for program A cannot be replayed against program B.

use crate::ir_nodes::IRProgram;

use super::generate::{
    artifact_digest, derive_aggregate_soundness_witnesses,
    derive_capability_containment_witness, derive_capability_isolation_witness,
    derive_channel_egress_witness, derive_compliance_coverage_witness,
    derive_channel_delivery_soundness_witness,
    derive_effect_budgeted_witness, derive_effect_row_soundness_witness,
    derive_endpoint_retry_witness, derive_interruptible_session_witness,
    derive_json_shape_soundness_witness, derive_parked_residual_witness,
    derive_shield_halt_witness, derive_socket_credit_witness, derive_tool_call_soundness_witness,
    derive_upstream_projection_witness,
};
use super::proof_term::{
    CallSoundnessCertificate, ProofTerm, PropertyClass, ResourceBoundsWitness, Witness,
    CALL_INTERRUPT_CAUSES, VALID_SIGN_ALGORITHMS,
};

/// The closed verdict catalog. Total over every proof shape — no
/// default / panic path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    /// The property holds for this artifact and the witness matches the
    /// checker's independent re-derivation.
    Verified,
    /// The witness matches the artifact but the property does NOT hold
    /// (an honest defect — e.g. compliance declared with no resolvable
    /// shield, or a phantom regulatory class), OR the witness DISAGREES
    /// with the artifact (a forged proof). `reason` distinguishes.
    Refuted { reason: String },
    /// The proof's `artifact_digest` does not match the supplied
    /// artifact — the proof is about a different program.
    DigestMismatch,
    /// The proof's property/witness pairing is not one this checker
    /// version understands (forward-compat for v2.4.0-e classes).
    UnknownProperty,
}

/// v2.4.0 — verify a [`ProofTerm`] against the artifact it claims to be
/// about. Independent of the producer. Total + never-panics.
pub fn check_proof(proof: &ProofTerm, ir: &IRProgram) -> CheckOutcome {
    // (1) the design decision — digest binding. Recompute; reject a mismatch before
    // looking at the witness at all.
    if proof.artifact_digest != artifact_digest(ir) {
        return CheckOutcome::DigestMismatch;
    }
    // (2) dispatch on the (property, witness) pairing. A mismatched
    // pairing (e.g. property says ComplianceCoverage but the witness is
    // an EffectRowSoundness witness) is a malformed proof — neither
    // property is actually witnessed — so it is UnknownProperty, not a
    // silent accept.
    match (&proof.property, &proof.witness) {
        (PropertyClass::ComplianceCoverage, Witness::ComplianceCoverage(w)) => {
            check_compliance_coverage(w, ir)
        }
        (PropertyClass::EffectRowSoundness, Witness::EffectRowSoundness(w)) => {
            check_effect_row_soundness(w, ir)
        }
        (PropertyClass::CapabilityIsolation, Witness::CapabilityIsolation(w)) => {
            check_capability_isolation(w, ir)
        }
        (PropertyClass::ResourceBounds, Witness::ResourceBounds(w)) => {
            check_resource_bounds(w, ir)
        }
        (PropertyClass::ShieldHaltGuarantee, Witness::ShieldHaltGuarantee(w)) => {
            check_shield_halt_guarantee(w, ir)
        }
        (PropertyClass::CapabilityContainment, Witness::CapabilityContainment(w)) => {
            check_capability_containment(w, ir)
        }
        (PropertyClass::ToolCallSoundness, Witness::ToolCallSoundness(w)) => {
            check_tool_call_soundness(w, ir)
        }
        (PropertyClass::EffectBudgeted, Witness::EffectBudgeted(w)) => {
            check_effect_budgeted(w, ir)
        }
        (PropertyClass::JsonShapeSoundness, Witness::JsonShapeSoundness(w)) => {
            check_json_shape_soundness(w, ir)
        }
        (PropertyClass::ChannelDeliverySoundness, Witness::ChannelDeliverySoundness(w)) => {
            check_channel_delivery_soundness(w, ir)
        }
        (PropertyClass::AggregateSoundness, Witness::AggregateSoundness(w)) => {
            check_aggregate_soundness(w, ir)
        }
        (PropertyClass::ChannelEgressSoundness, Witness::ChannelEgressSoundness(w)) => {
            check_channel_egress_soundness(w, ir)
        }
        (
            PropertyClass::InterruptibleSessionSoundness,
            Witness::InterruptibleSessionSoundness(w),
        ) => check_interruptible_session_soundness(w, ir),
        (PropertyClass::ParkedResidualSoundness, Witness::ParkedResidualSoundness(w)) => {
            check_parked_residual_soundness(w, ir)
        }
        (PropertyClass::UpstreamProjectionSoundness, Witness::UpstreamProjectionSoundness(w)) => {
            check_upstream_projection_soundness(w, ir)
        }
        (PropertyClass::CorsPolicyConsistency, Witness::CorsPolicyConsistency(w)) => {
            check_cors_policy_consistency(w, ir)
        }
        (PropertyClass::TechnicianCommandSafety, Witness::TechnicianCommandSafety(w)) => {
            check_technician_command_safety(w, ir)
        }
        (PropertyClass::CacheSoundness, Witness::CacheSoundness(w)) => {
            check_cache_soundness(w, ir)
        }
        (PropertyClass::ForgeSoundness, Witness::ForgeSoundness(w)) => {
            check_forge_soundness(w, ir)
        }
        (PropertyClass::SavantSoundness, Witness::SavantSoundness(w)) => {
            check_savant_soundness(w, ir)
        }
        (PropertyClass::WardenSoundness, Witness::WardenSoundness(w)) => {
            check_warden_soundness(w, ir)
        }
        (PropertyClass::AuthorizationCoverage, Witness::AuthorizationCoverage(w)) => {
            check_authorization_coverage(w, ir)
        }
        (PropertyClass::CapabilityGrantability, Witness::CapabilityGrantability(w)) => {
            check_capability_grantability(w, ir)
        }
        (PropertyClass::TemporalContextSoundness, Witness::TemporalContextSoundness(w)) => {
            check_temporal_context_soundness(w, ir)
        }
        (PropertyClass::CredentialAttenuation, Witness::CredentialAttenuation(w)) => {
            check_credential_attenuation(w, ir)
        }
        (PropertyClass::SecretCustodySoundness, Witness::SecretCustodySoundness(w)) => {
            check_secret_custody_soundness(w, ir)
        }
        (PropertyClass::ScrapeProvenanceSoundness, Witness::ScrapeProvenanceSoundness(w)) => {
            check_scrape_provenance_soundness(w, ir)
        }
        (PropertyClass::ScopeCoverageSoundness, Witness::ScopeCoverageSoundness(w)) => {
            check_scope_coverage_soundness(w, ir)
        }
        (PropertyClass::DocumentProvenanceSoundness, Witness::DocumentProvenanceSoundness(w)) => {
            check_document_provenance_soundness(w, ir)
        }
        (PropertyClass::DeliveryProvenanceSoundness, Witness::DeliveryProvenanceSoundness(w)) => {
            check_delivery_provenance_soundness(w, ir)
        }
        (PropertyClass::DataspaceSchemaSoundness, Witness::DataspaceSchemaSoundness(w)) => {
            check_dataspace_schema_soundness(w, ir)
        }
        (PropertyClass::GradientSoundness, Witness::GradientSoundness(w)) => {
            check_gradient_soundness(w, ir)
        }
        (PropertyClass::NotificationProvenanceSoundness, Witness::NotificationProvenanceSoundness(w)) => {
            check_notification_provenance_soundness(w, ir)
        }
        (PropertyClass::QuerySafetySoundness, Witness::QuerySafetySoundness(w)) => {
            check_query_safety_soundness(w, ir)
        }
        (PropertyClass::DocumentIngestionSoundness, Witness::DocumentIngestionSoundness(w)) => {
            check_document_ingestion_soundness(w, ir)
        }
        (PropertyClass::InferredCeilingSoundness, Witness::InferredCeilingSoundness(w)) => {
            check_inferred_ceiling_soundness(w, ir)
        }
        _ => CheckOutcome::UnknownProperty,
    }
}

/// v2.4.0 — the result of checking one proof inside a bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofCheck {
    /// Position of the proof in `ProofBundle::proofs`.
    pub index: usize,
    /// The property class the proof claims.
    pub property: PropertyClass,
    /// The witness subject (endpoint / tool / store / shield name).
    pub subject: String,
    /// The independent checker's verdict.
    pub outcome: CheckOutcome,
}

/// v2.4.0 — the deployability report for a whole [`ProofBundle`].
///
/// Aggregates the per-proof [`check_proof`] outcomes so a consumer (the
/// enterprise deploy gate, the `axon pcc verify` CLI) has ONE trusted
/// predicate for "is this bundle deployable." The policy — what counts
/// as deployable — lives HERE, on the checker side, never in consumer
/// glue: a bundle is deployable iff every proof `Verified`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleReport {
    /// One entry per proof, in bundle order.
    pub results: Vec<ProofCheck>,
}

impl BundleReport {
    /// True iff EVERY proof verified. Fail-closed: an empty bundle is
    /// vacuously verified (nothing to refute), and any non-`Verified`
    /// outcome (`Refuted` / `DigestMismatch` / `UnknownProperty`) makes
    /// the whole bundle non-deployable.
    pub fn all_verified(&self) -> bool {
        self.results
            .iter()
            .all(|r| r.outcome == CheckOutcome::Verified)
    }

    /// Every check that did NOT verify — the reasons a deploy gate
    /// surfaces when it rejects.
    pub fn refutations(&self) -> Vec<&ProofCheck> {
        self.results
            .iter()
            .filter(|r| r.outcome != CheckOutcome::Verified)
            .collect()
    }
}

/// v2.4.0 — check every proof in a bundle against the artifact,
/// independently (each proof re-derived via [`check_proof`]). The
/// bundle's own `artifact_digest` is advisory only — the authoritative
/// binding is the per-proof digest [`check_proof`] re-verifies, so a
/// bundle whose proofs are about a different artifact reports
/// `DigestMismatch` per proof (not a silent pass).
pub fn check_bundle(bundle: &super::proof_term::ProofBundle, ir: &IRProgram) -> BundleReport {
    let results = bundle
        .proofs
        .iter()
        .enumerate()
        .map(|(index, proof)| ProofCheck {
            index,
            property: proof.property.clone(),
            subject: proof.witness.subject_name().to_string(),
            outcome: check_proof(proof, ir),
        })
        .collect();
    BundleReport { results }
}

/// v2.4.0 — re-derive the compliance-coverage witness from the artifact
/// and verify it against the proof's witness, then render the verdict.
///
/// The order matters for soundness: we recompute the witness FIRST
/// (independent of the proof's claims), reject any disagreement as a
/// forgery, and only THEN decide Verified vs Refuted on the
/// re-derived facts.
fn check_compliance_coverage(
    claimed: &super::proof_term::ComplianceCoverageWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    // Locate the endpoint named in the witness. Absent ⟹ the proof is
    // about an endpoint not in this artifact (forged / stale).
    let ep = match ir.endpoints.iter().find(|e| e.name == claimed.endpoint_name) {
        Some(e) => e,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "endpoint '{}' not present in artifact",
                    claimed.endpoint_name
                ),
            }
        }
    };

    // Re-derive independently — do NOT trust the witness.
    let actual = derive_compliance_coverage_witness(
        &ep.name,
        &ep.compliance,
        &ep.shield_ref,
        ir,
    );

    // Forgery detection: every witness field must equal the checker's
    // re-derivation. Any disagreement ⟹ the proof lies about the
    // artifact.
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }

    // Verdict on the re-derived (trusted) facts.
    if !actual.shield_present {
        return CheckOutcome::Refuted {
            reason: format!(
                "endpoint '{}' declares compliance {:?} but has no resolvable shield (shield_ref={:?})",
                actual.endpoint_name, actual.required_classes, actual.shield_ref
            ),
        };
    }
    if !actual.unknown_classes.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "endpoint '{}' declares unknown regulatory class(es) {:?} (not in the closed registry)",
                actual.endpoint_name, actual.unknown_classes
            ),
        };
    }
    if !actual.uncovered_classes.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "endpoint '{}' requires regulatory class(es) {:?} that its shield '{}' does not provide (shield provides {:?})",
                actual.endpoint_name,
                actual.uncovered_classes,
                actual.shield_ref,
                actual.provided_classes
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.4.0 — re-derive the effect-row witness from the named tool and
/// verify it against the proof, then render the verdict. Independent
/// of the producer: the checker re-reads `tool.effect_row` from
/// the artifact and recomputes every fact; a forged witness is rejected
/// because the recomputation disagrees.
fn check_effect_row_soundness(
    claimed: &super::proof_term::EffectRowSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let tool = match ir.tools.iter().find(|t| t.name == claimed.tool_name) {
        Some(t) => t,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "tool '{}' not present in artifact",
                    claimed.tool_name
                ),
            }
        }
    };

    // Re-derive independently — do NOT trust the witness. v2.5.0:
    // the extension provenance members are re-derived from the SAME
    // artifact (`ir`), so an extension-declared base verifies, while a
    // base no extension declares still refutes. Soundness invariant #1:
    // the verifier reads the artifact's own `extensions`, never an
    // external registry.
    let ext_members = super::generate::extension_effect_members(ir);
    let actual =
        derive_effect_row_soundness_witness(&tool.name, &tool.effect_row, &ext_members);

    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }

    // Verdict on the re-derived facts.
    if !actual.unknown_bases.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "tool '{}' declares effect entr(ies) with unknown base(s) {:?} (not in the closed effect catalog)",
                actual.tool_name, actual.unknown_bases
            ),
        };
    }
    if !actual.missing_qualifier.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "tool '{}' declares qualifier-requiring effect(s) {:?} without a qualifier (bare stream/trust is unenforceable)",
                actual.tool_name, actual.missing_qualifier
            ),
        };
    }
    if !actual.invalid_stream_qualifier.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "tool '{}' declares stream effect(s) {:?} with an invalid backpressure policy qualifier",
                actual.tool_name, actual.invalid_stream_qualifier
            ),
        };
    }
    if actual.purity_violation {
        return CheckOutcome::Refuted {
            reason: format!(
                "tool '{}' declares `pure` alongside other effects {:?} — a pure tool cannot be effectful",
                actual.tool_name, actual.declared_effects
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.4.0 — re-derive the capability-gate witness from the named store
/// and verify it against the proof, then render the verdict.
/// Independent of the producer: the checker re-reads
/// `store.capability` from the artifact and re-runs the v1.23.0 grammar
/// validator; a forged witness (claiming `malformed: false` for a
/// broken gate) is rejected because the recomputation disagrees.
fn check_capability_isolation(
    claimed: &super::proof_term::CapabilityIsolationWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let store = match ir
        .axonstore_specs
        .iter()
        .find(|s| s.name == claimed.store_name)
    {
        Some(s) => s,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "axonstore '{}' not present in artifact",
                    claimed.store_name
                ),
            }
        }
    };

    // Re-derive independently — do NOT trust the witness.
    let actual = derive_capability_isolation_witness(&store.name, &store.capability);

    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }

    if actual.malformed {
        return CheckOutcome::Refuted {
            reason: format!(
                "axonstore '{}' declares a malformed capability gate {:?} (not a valid scope: ^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)*$)",
                actual.store_name, actual.capability
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.4.0 — re-derive the resource-bound witness from the named subject
/// (endpoint or socket) and verify it against the proof, then render
/// the verdict. Independent of the producer: the checker
/// re-reads `retries` / `backpressure_credit` from the artifact and
/// recomputes the bound; a forged witness is rejected because the
/// recomputation disagrees.
fn check_resource_bounds(
    claimed: &ResourceBoundsWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    match claimed {
        ResourceBoundsWitness::EndpointRetry { endpoint_name, .. } => {
            let ep = match ir.endpoints.iter().find(|e| e.name == *endpoint_name) {
                Some(e) => e,
                None => {
                    return CheckOutcome::Refuted {
                        reason: format!(
                            "endpoint '{}' not present in artifact",
                            endpoint_name
                        ),
                    }
                }
            };
            let actual = derive_endpoint_retry_witness(&ep.name, ep.retries);
            if actual != *claimed {
                return CheckOutcome::Refuted {
                    reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                        .to_string(),
                };
            }
            if let ResourceBoundsWitness::EndpointRetry { retries, in_bounds, .. } = &actual {
                if !in_bounds {
                    return CheckOutcome::Refuted {
                        reason: format!(
                            "endpoint '{}' declares retries={} outside the bound [0, {}] (negative is nonsensical; above the ceiling is a retry storm)",
                            ep.name,
                            retries,
                            super::proof_term::MAX_RETRIES
                        ),
                    };
                }
            }
            CheckOutcome::Verified
        }
        ResourceBoundsWitness::SocketCredit { socket_name, .. } => {
            let socket = match ir.sockets.iter().find(|s| s.name == *socket_name) {
                Some(s) => s,
                None => {
                    return CheckOutcome::Refuted {
                        reason: format!(
                            "socket '{}' not present in artifact",
                            socket_name
                        ),
                    }
                }
            };
            // The witness claims a DECLARED credit. If the artifact's
            // socket has no declared credit, the witness disagrees
            // (forged / stale).
            let credit = match socket.backpressure_credit {
                Some(k) => k,
                None => {
                    return CheckOutcome::Refuted {
                        reason: format!(
                            "socket '{}' has no declared backpressure credit in the artifact, but the witness claims one (forged or stale proof)",
                            socket_name
                        ),
                    }
                }
            };
            let actual = derive_socket_credit_witness(&socket.name, credit);
            if actual != *claimed {
                return CheckOutcome::Refuted {
                    reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                        .to_string(),
                };
            }
            if let ResourceBoundsWitness::SocketCredit { credit, positive, .. } = &actual {
                if !positive {
                    return CheckOutcome::Refuted {
                        reason: format!(
                            "socket '{}' declares backpressure credit({}) — a window < 1 deadlocks the typed-resource gate",
                            socket.name, credit
                        ),
                    };
                }
            }
            CheckOutcome::Verified
        }
    }
}

/// v2.4.0 — re-derive the shield breach-policy witness from the named
/// shield and verify it against the proof, then render the verdict.
/// Independent of the producer: the checker re-reads
/// `on_breach` + `scan` from the artifact and recomputes both facts; a
/// forged witness (claiming `vacuous_halt: false` for a halt shield
/// that scans nothing) is rejected because the recomputation disagrees.
fn check_shield_halt_guarantee(
    claimed: &super::proof_term::ShieldHaltGuaranteeWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let shield = match ir.shields.iter().find(|s| s.name == claimed.shield_name) {
        Some(s) => s,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "shield '{}' not present in artifact",
                    claimed.shield_name
                ),
            }
        }
    };

    // Re-derive independently — do NOT trust the witness. v2.34.0: `sign`
    // participates (a sign-only egress shield is a non-vacuous halt, the design decision).
    let actual = derive_shield_halt_witness(
        &shield.name,
        &shield.on_breach,
        &shield.scan,
        &shield.sign,
    );

    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }

    if !actual.known_policy {
        return CheckOutcome::Refuted {
            reason: format!(
                "shield '{}' declares an unknown on_breach policy {:?} (not in the closed breach-policy catalog)",
                actual.shield_name, actual.on_breach
            ),
        };
    }
    if actual.vacuous_halt {
        return CheckOutcome::Refuted {
            reason: format!(
                "shield '{}' declares `on_breach: halt` but neither `scan:` nor `sign:` — the halt can never fire (nothing enforced ⟹ no breach ⟹ no halt): a vacuous guarantee",
                actual.shield_name
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.4.0 — re-derive the capability-containment witness from the named
/// endpoint and verify it against the proof, then render the verdict.
/// Independent of the producer: the checker re-resolves the
/// `execute_flow`, re-walks its reachable store ops, re-resolves each
/// gate, and recomputes the uncovered set; a forged witness (e.g.
/// hiding an uncovered gate) is rejected because the recomputation
/// disagrees.
fn check_capability_containment(
    claimed: &super::proof_term::CapabilityContainmentWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let ep = match ir.endpoints.iter().find(|e| e.name == claimed.endpoint_name) {
        Some(e) => e,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "endpoint '{}' not present in artifact",
                    claimed.endpoint_name
                ),
            }
        }
    };

    // Re-derive independently — do NOT trust the witness.
    let actual = derive_capability_containment_witness(
        &ep.name,
        &ep.execute_flow,
        &ep.requires_capabilities,
        ir,
    );

    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }

    // Verdict. An unresolvable execute_flow cannot be analyzed for
    // store reachability — refuse to certify containment (sound: a
    // missing flow could reach gated stores we cannot see).
    if !actual.flow_resolved {
        return CheckOutcome::Refuted {
            reason: format!(
                "endpoint '{}' executes flow '{}' which is not present in the artifact — cannot certify capability containment",
                actual.endpoint_name, actual.execute_flow
            ),
        };
    }
    if !actual.uncovered_gates.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "endpoint '{}' reaches store(s) gated by capabilit(ies) {:?} that its declared `requires:` {:?} does not cover — a capability leak (a request satisfying the declared requires could reach a store it is not authorized for)",
                actual.endpoint_name, actual.uncovered_gates, actual.declared_requires
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.8.0 — re-derive the tool-call-soundness witness from the named flow
/// + call site and verify it against the proof, then render the verdict.
/// Independent of the producer: the checker re-walks the SAME
/// digest-bound IR, locates the call at `call_index`, re-reads the called
/// tool's `parameters:` schema, and recomputes every fact; a forged
/// witness (e.g. hiding an unknown argument or a type mismatch) is
/// rejected because the recomputation disagrees.
fn check_tool_call_soundness(
    claimed: &super::proof_term::ToolCallSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    // Re-derive independently — do NOT trust the witness. `None` ⟹ the
    // flow or the call site named by the witness is absent from this
    // artifact (forged / stale proof).
    let actual = match derive_tool_call_soundness_witness(
        &claimed.flow_name,
        claimed.call_index,
        ir,
    ) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "flow '{}' has no structured `use` call at index {} in this artifact",
                    claimed.flow_name, claimed.call_index
                ),
            }
        }
    };

    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }

    // Verdict on the re-derived (trusted) facts.
    if !actual.schema_present {
        // A schema-less / undeclared tool carries no arg contract — there
        // is nothing to certify, so a proof for it is vacuously verified
        // (generation never emits one; this stays total for a hand-built
        // witness).
        return CheckOutcome::Verified;
    }
    if !actual.unknown_args.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "call to tool '{}' in flow '{}' passes argument(s) {:?} the tool does not declare (declared parameters: {:?})",
                actual.tool_name, actual.flow_name, actual.unknown_args, actual.declared_params
            ),
        };
    }
    if !actual.duplicate_args.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "call to tool '{}' in flow '{}' supplies duplicate argument(s) {:?}",
                actual.tool_name, actual.flow_name, actual.duplicate_args
            ),
        };
    }
    if !actual.missing_required.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "call to tool '{}' in flow '{}' omits required argument(s) {:?}",
                actual.tool_name, actual.flow_name, actual.missing_required
            ),
        };
    }
    if !actual.type_mismatches.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "call to tool '{}' in flow '{}' has literal-argument type mismatch(es) {:?} (each `arg:expected:got`)",
                actual.tool_name, actual.flow_name, actual.type_mismatches
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.28.0 — check an [`EffectBudgetedWitness`]. Re-derive the budget facts
/// independently from the artifact; refute on disagreement (forgery) or on any
/// budget defect (an unresolved effect, a non-positive limit, an invalid period,
/// or an out-of-catalog `on_exhausted`).
fn check_effect_budgeted(
    claimed: &super::proof_term::EffectBudgetedWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    // Re-derive — do NOT trust the witness. `None` ⟹ the daemon named by the
    // witness is absent, or it carries no budget (forged / stale proof).
    let actual = match derive_effect_budgeted_witness(&claimed.daemon_name, ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "daemon '{}' has no `budget {{ }}` in this artifact",
                    claimed.daemon_name
                ),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    // Verdict on the re-derived (trusted) facts.
    if !actual.unresolved_effects.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "daemon '{}' budget targets undefined tool(s) {:?} — no matching `tool` declaration",
                actual.daemon_name, actual.unresolved_effects
            ),
        };
    }
    if !actual.nonpositive_limits.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "daemon '{}' budget has non-positive quota limit(s) {:?} (each `effect:kind`)",
                actual.daemon_name, actual.nonpositive_limits
            ),
        };
    }
    if !actual.invalid_periods.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "daemon '{}' budget has out-of-catalog period(s) {:?} (each `effect:period`; valid: {:?})",
                actual.daemon_name, actual.invalid_periods, super::proof_term::VALID_BUDGET_PERIODS
            ),
        };
    }
    if !actual.on_exhausted_valid {
        return CheckOutcome::Refuted {
            reason: format!(
                "daemon '{}' budget has an out-of-catalog `on_exhausted` policy '{}' (valid: {:?})",
                actual.daemon_name, actual.on_exhausted, super::proof_term::VALID_ON_EXHAUSTED
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.26.0 — check a [`JsonShapeSoundnessWitness`]. Re-derive the store's
/// lens shapes independently from the artifact; refute on disagreement
/// (forgery) or on any shape that does not resolve to a declared struct
/// `type` (the `axon-T840` invariant, now independently verified).
fn check_json_shape_soundness(
    claimed: &super::proof_term::JsonShapeSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    // Re-derive — do NOT trust the witness. `None` ⟹ the store named by the
    // witness is absent, or carries no inline `Json<T>` lens column (a
    // forged / stale proof).
    let actual = match derive_json_shape_soundness_witness(&claimed.store_name, ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "axonstore '{}' declares no inline `Json<T>` lens column in this artifact",
                    claimed.store_name
                ),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    // Verdict on the re-derived (trusted) facts: every lens shape must
    // resolve to a declared struct `type`.
    if !actual.unresolved_shapes.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axonstore '{}' declares `Json<T>` lens column(s) {:?} (each `column:Shape`) \
                 whose shape is not a declared `type` — the lens cannot be field-checked, so \
                 navigation soundness is unprovable. Declare the struct, or use open `Json`.",
                actual.store_name, actual.unresolved_shapes
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.31.0 — check a [`ChannelDeliverySoundnessWitness`]. Re-derive the
/// channel's producer/consumer facts from the artifact; refute on
/// disagreement (forgery) or when a consumed channel has NO producer (a
/// `listen`er that can never fire — nothing `emit`s to it).
fn check_channel_delivery_soundness(
    claimed: &super::proof_term::ChannelDeliverySoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match derive_channel_delivery_soundness_witness(&claimed.channel_name, ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "channel '{}' has no `daemon` `listen`er in this artifact (no delivery \
                     contract to certify)",
                    claimed.channel_name
                ),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    // Verdict: a consumed channel with no producer never fires.
    if !actual.has_producer {
        return CheckOutcome::Refuted {
            reason: format!(
                "channel '{}' has a `daemon` `listen`er but NO producer — nothing `emit`s to \
                 it, so the listener can never fire. Add an `emit {}(…)` in a flow, or remove \
                 the listener (the Kivi brief #39 defect, now machine-checked).",
                actual.channel_name, actual.channel_name
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.33.0 — check an [`AggregateSoundnessWitness`]. Re-derive ALL aggregate
/// sites from the artifact (through the SAME runtime clause parser the
/// engines execute) and require the claimed witness to be one of them —
/// a witness about a site the artifact does not contain is a forgery.
/// Then the verdict: any recorded violation refutes (the rendered SQL
/// would not aggregate exactly the declared columns through the closed
/// catalog).
fn check_aggregate_soundness(
    claimed: &super::proof_term::AggregateSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let derived = derive_aggregate_soundness_witnesses(ir);
    if !derived.iter().any(|w| w == claimed) {
        return CheckOutcome::Refuted {
            reason: format!(
                "no aggregate-retrieve site in this artifact matches the witness \
                 (flow '{}', store '{}', aggregate '{}') — forged or stale proof",
                claimed.flow_name, claimed.store_name, claimed.aggregate
            ),
        };
    }
    if !claimed.violations.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "aggregate-retrieve on store '{}' in flow '{}' is UNSOUND: {}",
                claimed.store_name,
                claimed.flow_name,
                claimed.violations.join("; ")
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.34.0 — check a [`ChannelEgressSoundnessWitness`]. Re-derive the
/// channel's egress facts from the artifact (publish sites resolved
/// against the DECLARED shields — never the IR's pre-resolved stamps);
/// refute on disagreement (a forged `egress_sign` handle), an algorithm
/// outside the closed signing catalog, or a non-durable egress channel
/// (the design decision — a webhook promise backed by an ephemeral buffer dies
/// unwitnessed with the process).
fn check_channel_egress_soundness(
    claimed: &super::proof_term::ChannelEgressSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match derive_channel_egress_witness(&claimed.channel_name, ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "channel '{}' carries no egress contract in this artifact (no declared \
                     `egress_sign`, no signing publish site) — forged or stale proof",
                    claimed.channel_name
                ),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    // Verdict 1: the handle's marking must be exactly what the program
    // derives — a stamp with no deriving publish site (or a site the
    // stamp hides) is a forged egress surface.
    if actual.declared_egress_sign != actual.derived_sign {
        return CheckOutcome::Refuted {
            reason: format!(
                "channel '{}' egress marking '{}' disagrees with the program's publish \
                 sites (derived '{}') — the egress surface is not derivable from the \
                 source (forged handle or stale lowering)",
                actual.channel_name, actual.declared_egress_sign, actual.derived_sign
            ),
        };
    }
    // Verdict 2: closed signing catalog.
    if !VALID_SIGN_ALGORITHMS.contains(&actual.derived_sign.as_str()) {
        return CheckOutcome::Refuted {
            reason: format!(
                "channel '{}' declares egress signing '{}' — not in the closed signing \
                 catalog {:?}",
                actual.channel_name, actual.derived_sign, VALID_SIGN_ALGORITHMS
            ),
        };
    }
    // Verdict 3: durable egress — signed external delivery must
    // inherit the v2.31.0 outbox's at-least-once.
    if !actual.durable {
        return CheckOutcome::Refuted {
            reason: format!(
                "channel '{}' is egress-published under shield '{}' (`sign: {}`) but its \
                 persistence is '{}' — signed egress requires `persistence: \
                 persistent_axonstore` (axon-T848)",
                actual.channel_name, actual.shield_ref, actual.derived_sign, actual.persistence
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.38.0 — independent checker for [`PropertyClass::CorsPolicyConsistency`].
/// Re-derives the whole-program witness and rejects on ANY of: a forged/
/// stale witness, an unresolved `cors:` reference (T856), a wildcard+
/// credentials violation (T853), or a cross-method path conflict (T857).
fn check_cors_policy_consistency(
    claimed: &super::proof_term::CorsPolicyConsistencyWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match super::generate::derive_cors_policy_consistency_witness(ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: "no cors contract exists in this artifact (no `cors` declarations, \
                         no `cors:` references) — forged or stale proof"
                    .to_string(),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.all_references_resolve {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T856 an `axonendpoint.cors:` reference does not resolve to a declared \
                 `cors` — declared: {:?}, referenced: {:?}",
                actual.declared_cors_names, actual.endpoint_cors_refs
            ),
        };
    }
    if !actual.wildcard_credential_violations.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T853 cors declaration(s) combine an any-origin `allow_origins` with \
                 `allow_credentials: true` (CORS spec violation): {:?}",
                actual.wildcard_credential_violations
            ),
        };
    }
    if !actual.cross_method_conflicts.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T857 axonendpoints sharing a path disagree on `cors:` \
                 (endpoint_a, endpoint_b) pairs: {:?}",
                actual.cross_method_conflicts
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.46.0 — independent checker for [`PropertyClass::CredentialAttenuation`].
/// Re-derives the whole-program witness (contracts + recursive mint walk)
/// and rejects on ANY of: a forged/stale witness, a mint of an undeclared
/// contract (`axon-T895`, re-derived), or an ill-formed contract — empty/
/// invalid grants (`axon-T893`) or a TTL outside the (0, 24h] ephemeral
/// ceiling (`axon-T894`). The dynamic attenuation law
/// (`grants ⊆ capabilities(minter)`) is enforced fail-closed at mint time
/// (v2.46.0) — data-dependent, so it is deliberately NOT claimed here.
fn check_credential_attenuation(
    claimed: &super::proof_term::CredentialAttenuationWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match super::generate::derive_credential_attenuation_witness(ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: "no credential contract exists in this artifact (no `credential` \
                         declarations, no `mint` sites) — forged or stale proof"
                    .to_string(),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.unresolved_mints.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T895 `mint` reference(s) do not resolve to a declared `credential`: {:?}",
                actual.unresolved_mints
            ),
        };
    }
    if !actual.invalid_contracts.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T893/axon-T894 ill-formed credential contract(s) (empty/invalid \
                 grants, or TTL outside the (0, 24h] ephemeral ceiling): {:?}",
                actual.invalid_contracts
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.48.0 — independent checker for [`PropertyClass::SecretCustodySoundness`].
/// Re-derives the whole-program witness (secrets stores + recursive rotate
/// walk + write-verb walk) and rejects on ANY of: a forged/stale witness, a
/// rotate of an undeclared secrets store (`axon-T898`) or an undeclared tool
/// (`axon-T899`), a missing/shape-invalid `class:` (`axon-T900`), a write
/// verb against a secrets store (`axon-T897`), or an ill-formed
/// `secret_partition:` (`axon-T903`, v2.49.0 — the injection-key class-containment
/// law). The dynamic halves of
/// `rotation_without_revelation` (CAS commit, reveal-only-into-the-exchange)
/// are enforced fail-closed by the v2.48.0 dispatcher + custody port — by
/// construction of the wire, no term evaluates to a value — data-dependent,
/// so deliberately NOT claimed here.
fn check_secret_custody_soundness(
    claimed: &super::proof_term::SecretCustodySoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match super::generate::derive_secret_custody_witness(ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: "no custody contract exists in this artifact (no `backend: \
                         secrets` store, no `rotate` sites) — forged or stale proof"
                    .to_string(),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.unresolved_stores.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T898 `rotate` target(s) do not resolve to a declared `backend: \
                 secrets` store: {:?}",
                actual.unresolved_stores
            ),
        };
    }
    if !actual.unresolved_tools.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T899 `rotate` tool(s) do not resolve to a declared `tool`: {:?}",
                actual.unresolved_tools
            ),
        };
    }
    if !actual.invalid_classes.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T900 secrets store(s) with a missing or shape-invalid `class:`: {:?}",
                actual.invalid_classes
            ),
        };
    }
    if !actual.write_violations.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T897 write verb(s) against a secrets store (custody is written \
                 only by the seed API and the mediated rotate commit): {:?}",
                actual.write_violations
            ),
        };
    }
    if !actual.partition_violations.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T903 tool(s) with an ill-formed `secret_partition:` — a partition \
                 must name a required `String` parameter of the SAME tool, alongside a \
                 `secret:`, and never on a technician tool. The \
                 `selection_without_revelation` class-containment guarantee (the resolved \
                 dispatch key never leaves the tool's class) rests on this: {:?}",
                actual.partition_violations
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.46.0 — independent checker for [`PropertyClass::TemporalContextSoundness`].
/// Re-derives the whole-program witness (contexts + recursive step walk) and
/// rejects on ANY of: a forged/stale witness, a zone violating the IANA shape
/// law (`axon-T892`, re-derived), or a shape-valid zone unknown to this
/// build's tz database (the failure the zero-dep frontend cannot catch —
/// v2.46.0 fails closed at runtime; this proof catches it before deploy).
fn check_temporal_context_soundness(
    claimed: &super::proof_term::TemporalContextSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match super::generate::derive_temporal_context_soundness_witness(ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: "no temporal contract exists in this artifact (no `now:` \
                         declarations on any step or context) — forged or stale proof"
                    .to_string(),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.format_violations.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T892 declared `now:` zone(s) violate the IANA shape law \
                 (expected \"Area/Location\" or \"UTC\"): {:?}",
                actual.format_violations
            ),
        };
    }
    if !actual.unknown_zones.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "declared `now:` zone(s) pass the shape law but are NOT in this build's \
                 IANA tz database (tzdb {}) — the run would fail closed at the first \
                 `now:`-bearing step: {:?}",
                crate::window::tz_db_version(),
                actual.unknown_zones
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.39.0 — independent checker for [`PropertyClass::TechnicianCommandSafety`].
/// Re-derives the per-tool witness and rejects on ANY of: a forged/stale
/// witness, a missing argv template on a bash tool (T858), an unbound or
/// partial argv placeholder (T859), or a destructive command with no reachable
/// confirm/deny branch (T860). This proof travels with the artifact to the
/// deploy gate — the last line before an IR reaches a real machine.
fn check_technician_command_safety(
    claimed: &super::proof_term::TechnicianCommandSafetyWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let tool = match ir.tools.iter().find(|t| t.name == claimed.tool_name) {
        Some(t) => t,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "no technician tool '{}' exists in this artifact — forged or stale proof",
                    claimed.tool_name
                ),
            }
        }
    };
    let actual = match super::generate::derive_technician_command_safety_witness(tool, ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "tool '{}' is not a technician tool (no `target:`) — forged or stale proof",
                    claimed.tool_name
                ),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.argv_present {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T858 technician tool '{}' binds `target:` on `provider: bash` but declares \
                 no `argv:` template",
                actual.tool_name
            ),
        };
    }
    if !actual.unbound_placeholders.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T859 technician tool '{}' has argv placeholder(s) not bound to a declared \
                 parameter: {:?}",
                actual.tool_name, actual.unbound_placeholders
            ),
        };
    }
    if !actual.partial_tokens.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T859 technician tool '{}' has partial/fused argv token(s) (a `${{param}}` \
                 must be a whole argv element): {:?}",
                actual.tool_name, actual.partial_tokens
            ),
        };
    }
    if !actual.confirm_branch_reachable {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T860 technician tool '{}' is `risk: destructive` but its bound session '{}' \
                 offers no reachable `branch{{ approved / denied }}` confirmation",
                actual.tool_name, actual.session_name
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.41.0 — independent checker for [`PropertyClass::ForgeSoundness`]. Finds the
/// forge block by (flow, name), re-derives its witness, and rejects a forged/
/// stale proof or any structural violation (T868–T872).
fn check_forge_soundness(
    claimed: &super::proof_term::ForgeSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    // Locate the forge block the witness claims.
    let mut found = None;
    for flow in &ir.flows {
        if flow.name != claimed.flow_name {
            continue;
        }
        let mut forges = Vec::new();
        super::generate::__collect_forge_for_check(&flow.steps, &mut forges);
        if let Some(f) = forges.into_iter().find(|f| f.name == claimed.forge_name) {
            found = Some(f);
            break;
        }
    }
    let forge = match found {
        Some(f) => f,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "no forge '{}' in flow '{}' — forged or stale proof",
                    claimed.forge_name, claimed.flow_name
                ),
            }
        }
    };
    let actual = super::generate::derive_forge_soundness_witness(&claimed.flow_name, forge, ir);
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.mode_ok {
        return CheckOutcome::Refuted {
            reason: format!("axon-T868 forge '{}' has an unknown creativity mode", actual.forge_name),
        };
    }
    if !actual.novelty_in_range {
        return CheckOutcome::Refuted {
            reason: format!("axon-T869 forge '{}' novelty is outside [0,1]", actual.forge_name),
        };
    }
    if !actual.bounds_ok {
        return CheckOutcome::Refuted {
            reason: format!("axon-T870 forge '{}' depth/branches < 1", actual.forge_name),
        };
    }
    if !actual.seed_and_type_present {
        return CheckOutcome::Refuted {
            reason: format!("axon-T872 forge '{}' missing seed or return type", actual.forge_name),
        };
    }
    if !actual.constraints_ok {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T871 forge '{}' `constraints:` does not resolve to an anchor with a \
                 confidence_floor",
                actual.forge_name
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.42.0 — independent checker for [`PropertyClass::SavantSoundness`]. Finds the
/// savant by name, re-derives its witness, and rejects a forged/stale proof or
/// any governance violation (T873/T874/T875/T876/T877).
fn check_savant_soundness(
    claimed: &super::proof_term::SavantSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let savant = match ir.savants.iter().find(|s| s.name == claimed.savant_name) {
        Some(s) => s,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "no savant '{}' in program — forged or stale proof",
                    claimed.savant_name
                ),
            }
        }
    };
    let actual = super::generate::derive_savant_soundness_witness(savant, ir);
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.domain_present {
        return CheckOutcome::Refuted {
            reason: format!("axon-T873 savant '{}' has no `domain:`", actual.savant_name),
        };
    }
    if !actual.mandate_ok {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T874 savant '{}' has no well-formed `mandate` (objective + output)",
                actual.savant_name
            ),
        };
    }
    if !actual.budget_bounded {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T877 savant '{}' declares no positive `budget.max_iterations` — an \
                 unbounded autonomous loop is fail-open",
                actual.savant_name
            ),
        };
    }
    if !actual.cognition_ok {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T876 savant '{}' has an out-of-catalog cognition parameter",
                actual.savant_name
            ),
        };
    }
    if !actual.memory_ref_ok {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T875 savant '{}' `memory.backend` does not resolve to a memory/corpus",
                actual.savant_name
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.43.0 — independent checker for [`PropertyClass::WardenSoundness`]. Finds the
/// warden block by (flow, target, scope), re-derives its witness, and rejects a
/// forged/stale proof or any authorization violation (T887/T884/T885/T886).
fn check_warden_soundness(
    claimed: &super::proof_term::WardenSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    // Locate the warden block the witness claims.
    let mut found = None;
    for flow in &ir.flows {
        if flow.name != claimed.flow_name {
            continue;
        }
        let mut wardens = Vec::new();
        super::generate::__collect_warden_for_check(&flow.steps, &mut wardens);
        if let Some(w) = wardens
            .into_iter()
            .find(|w| w.target == claimed.warden_target && w.scope_ref == claimed.scope_ref)
        {
            found = Some(w);
            break;
        }
    }
    let warden = match found {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "no warden '{}' within '{}' in flow '{}' — forged or stale proof",
                    claimed.warden_target, claimed.scope_ref, claimed.flow_name
                ),
            }
        }
    };
    let actual = super::generate::derive_warden_soundness_witness(&claimed.flow_name, warden, ir);
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.scope_resolves {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T887 warden '{}' `within {}` does not resolve to a declared scope",
                actual.warden_target, actual.scope_ref
            ),
        };
    }
    if !actual.targets_nonempty {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T884 scope '{}' has an empty `targets` allowlist",
                actual.scope_ref
            ),
        };
    }
    if !actual.depth_ok {
        return CheckOutcome::Refuted {
            reason: format!("axon-T885 scope '{}' has an out-of-catalog depth", actual.scope_ref),
        };
    }
    if !actual.approver_present {
        return CheckOutcome::Refuted {
            reason: format!("axon-T886 scope '{}' names no approver", actual.scope_ref),
        };
    }
    CheckOutcome::Verified
}

/// v2.44.0 — independent checker for [`PropertyClass::AuthorizationCoverage`].
/// Re-derives the endpoint's coverage from the IR alone and rejects on: a
/// forged/stale witness (disagrees with re-derivation), an endpoint the
/// artifact does not contain, or an UNAUTHORIZED boundary (dispatches but is
/// neither covered nor `public: true` — the audit's Modo 1, `axon-T890`).
fn check_authorization_coverage(
    claimed: &super::proof_term::AuthorizationCoverageWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let ep = match ir.endpoints.iter().find(|e| e.name == claimed.endpoint_name) {
        Some(e) => e,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "no axonendpoint '{}' in the artifact — forged or stale proof",
                    claimed.endpoint_name
                ),
            }
        }
    };
    let actual = super::generate::derive_authorization_coverage_witness(ep);
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.authorized {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T890 axonendpoint '{}' dispatches a flow but declares no authorization \
                 coverage (no `requires:`/`shield:`/`compliance:`) and is not `public: true` \
                 — an unguarded boundary (doctrine `every_boundary_is_guarded`)",
                actual.endpoint_name
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.45.0 — independent checker for [`PropertyClass::CapabilityGrantability`]
/// (doctrine `every_requirement_is_grantable`). Re-derives the whole-program
/// `requires:` set from the IR (rejecting a forged witness), RE-PROJECTS the
/// witness's authority catalog through `π` (rejecting a fractured namespace or
/// an unprojectable authority — never trusting a pre-computed grantable list),
/// and rejects a DEAD requirement (`required ⊄ π(catalog)`, `axon-T891`) — the
/// dual of the v2.44.0 Modo-2 dead permission.
fn check_capability_grantability(
    claimed: &super::proof_term::CapabilityGrantabilityWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    use std::collections::BTreeSet;
    // (1) Re-derive `required` from the IR — a forged witness that dropped a
    // dead requirement (to fake `all_grantable`) disagrees here.
    let required: Vec<String> = ir
        .endpoints
        .iter()
        .filter(|e| !e.execute_flow.is_empty())
        .flat_map(|e| e.requires_capabilities.iter().cloned())
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect();
    if required != claimed.required {
        return CheckOutcome::Refuted {
            reason: "requires-set disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    // (2) Re-project the catalog. A fracture (two authorities → one capability)
    // or an unprojectable authority makes the manifest itself unsound — reject
    // fail-closed rather than certify against a broken grantable set.
    let grantable = crate::auth_scope::build_grantable_set(claimed.authorities.iter());
    if !grantable.collisions.is_empty() {
        let (a, b, cap) = &grantable.collisions[0];
        return CheckOutcome::Refuted {
            reason: format!(
                "fractured capability namespace: authorities '{a}' and '{b}' both project to \
                 '{cap}' — the grantable set is ambiguous (doctrine \
                 `every_requirement_is_grantable`)"
            ),
        };
    }
    if !grantable.unprojectable.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "authority '{}' does not project to a valid capability slug — malformed catalog",
                grantable.unprojectable[0]
            ),
        };
    }
    // (3) The law: requires ⊆ π(catalog). A dead requirement is axon-T891.
    match crate::auth_scope::check_grantable(&required, &grantable.caps) {
        crate::auth_scope::GrantabilityVerdict::Grantable => {}
        crate::auth_scope::GrantabilityVerdict::Ungrantable { ungrantable, .. } => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "axon-T891 `requires: [{}]` is not grantable — no authority in the catalog \
                     grants it (a dead boundary, declarable but never satisfiable; doctrine \
                     `every_requirement_is_grantable`)",
                    ungrantable.join(", ")
                ),
            };
        }
    }
    // (4) The witness's own verdict must agree with the re-derivation.
    if !claimed.all_grantable {
        return CheckOutcome::Refuted {
            reason: "witness claims all_grantable=false but re-derivation found every requirement \
                     grantable (forged or stale proof)"
                .to_string(),
        };
    }
    CheckOutcome::Verified
}

/// v2.40.0 — independent checker for [`PropertyClass::CacheSoundness`]. Re-derives
/// the whole-program cache witness and rejects on ANY of: a forged/stale
/// witness, more than one `default: true` (T863), a widened cache without a
/// finite ttl (T865), or an unresolved reference (T864).
fn check_cache_soundness(
    claimed: &super::proof_term::CacheSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match super::generate::derive_cache_soundness_witness(ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: "no cache contract exists in this artifact (no `cache` declarations, no \
                         `tool.cache:` references) — forged or stale proof"
                    .to_string(),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if actual.default_count > 1 {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T863 {} caches declare `default: true` (must be ≤ 1)",
                actual.default_count
            ),
        };
    }
    if !actual.widened_without_ttl.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T865 cache(s) widen `apply_to_effects:` beyond [pure] with no `ttl:`: {:?}",
                actual.widened_without_ttl
            ),
        };
    }
    if !actual.unresolved_refs.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T864 unresolved cache/channel reference(s): {:?}",
                actual.unresolved_refs
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.52.0 — verify a [`ScrapeProvenanceSoundnessWitness`]. The checker
/// independently RE-DERIVES the whole-program web-acquisition provenance from
/// `ir.tools` + `ir.flows` + `ir.agents` and rejects on ANY of: a forged/stale
/// witness, a scrape tool missing `web` (T904), a `scrape_dom` dishonestly
/// declaring `network` (T904), or a flow that lets web content reach an agent's
/// beliefs unshielded (T908, the content-injection barrier).
fn check_scrape_provenance_soundness(
    claimed: &super::proof_term::ScrapeProvenanceSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match super::generate::derive_scrape_provenance_soundness_witness(ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: "no web-acquisition contract exists in this artifact (no scrape tool) — \
                         forged or stale proof"
                    .to_string(),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.tools_missing_web.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T904 scrape tool(s) omit the `web` effect (scraped values must be born \
                 Untrusted): {:?}",
                actual.tools_missing_web
            ),
        };
    }
    if !actual.dom_tools_with_network.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T904 scrape_dom tool(s) dishonestly declare `network` (they do no I/O): {:?}",
                actual.dom_tools_with_network
            ),
        };
    }
    if !actual.unshielded_flows.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T908 content-injection barrier: flow(s) feed web content to an agent's \
                 beliefs with no shield: {:?}",
                actual.unshielded_flows
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.77.0 — verify a [`ScopeCoverageSoundnessWitness`]. The checker
/// independently RE-DERIVES the whole-program scope coverage from `ir.tools` +
/// `ir.credentials` + `ir.endpoints` + `ir.daemons` + `ir.flows` and rejects on:
/// a forged/stale witness, or any `use` of a scope-declaring tool whose required
/// scope the program's granted set does not cover (`axon-T956`) — a hand-edited
/// IR that smuggles in an unauthorized operation, or strips the granting
/// credential, is refuted BEFORE it mounts.
fn check_scope_coverage_soundness(
    claimed: &super::proof_term::ScopeCoverageSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match super::generate::derive_scope_coverage_soundness_witness(ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: "no scope-coverage contract exists in this artifact (no tool declares \
                         `requires:`) — forged or stale proof"
                    .to_string(),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.uncovered_uses.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T956 scope coverage: tool use(s) require a scope the program's granted set \
                 does not cover (flow, tool, missing_scope): {:?}",
                actual.uncovered_uses
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.53.0 — verify a [`DocumentProvenanceSoundnessWitness`]. The checker
/// independently RE-DERIVES the whole-program egress provenance from
/// `ir.documents` and rejects on ANY of: a forged/stale witness, a bad
/// `target` (T910), a `sensitive:*` document with no `legal:*` basis (T913), or
/// an assertive slot binding an unattributed flow value (T916, the assertion-
/// laundering barrier).
fn check_document_provenance_soundness(
    claimed: &super::proof_term::DocumentProvenanceSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match super::generate::derive_document_provenance_soundness_witness(ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: "no document contract exists in this artifact (no `document`) — forged or \
                         stale proof"
                    .to_string(),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.bad_targets.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!("axon-T910 document(s) with an invalid target: {:?}", actual.bad_targets),
        };
    }
    if !actual.sensitive_without_legal.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T913 document(s) bind sensitive data with no legal basis: {:?}",
                actual.sensitive_without_legal
            ),
        };
    }
    if !actual.unattributed_slots.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T916 assertion-laundering barrier: unattributed flow value(s) in assertive \
                 slots: {:?}",
                actual.unattributed_slots
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.60.0 — verify a [`DeliveryProvenanceSoundnessWitness`]. The checker
/// independently RE-DERIVES the whole-program egress provenance from
/// `ir.deliveries` and rejects on ANY of: a forged/stale witness, a bad `target`
/// (T921), a `sensitive:*` delivery with no `legal:*` basis (T924), or a
/// `provenance: cleared` delivery binding a flow value outside an epistemic vouch
/// (T920, the provenance-stripping barrier).
fn check_delivery_provenance_soundness(
    claimed: &super::proof_term::DeliveryProvenanceSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match super::generate::derive_delivery_provenance_soundness_witness(ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: "no delivery contract exists in this artifact (no `deliver`) — forged or \
                         stale proof"
                    .to_string(),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.bad_targets.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!("axon-T921 delivery(ies) with an invalid target: {:?}", actual.bad_targets),
        };
    }
    if !actual.sensitive_without_legal.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T924 delivery(ies) bind sensitive data with no legal basis: {:?}",
                actual.sensitive_without_legal
            ),
        };
    }
    if !actual.laundered_deliveries.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T920 provenance-stripping barrier: `provenance: cleared` delivery(ies) \
                 laundering an unshielded flow value into a CRM: {:?}",
                actual.laundered_deliveries
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.66.0 — verify a [`NotificationProvenanceSoundnessWitness`]: re-derive
/// the three laws from the artifact; refute disagreement or any surviving
/// violation. A hand-edited artifact that launders a guess to a human,
/// drops the window, or smuggles a literal recipient cannot deploy.
fn check_notification_provenance_soundness(
    claimed: &super::proof_term::NotificationProvenanceSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match super::generate::derive_notification_provenance_soundness_witness(ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: "no notify declaration exists in this artifact — forged or stale proof"
                    .to_string(),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.bad_structure.is_empty()
        || !actual.bad_windows.is_empty()
        || !actual.laundered_notifications.is_empty()
    {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T933/T934/T935 notification law violated (stale or hand-edited \
                 artifact): structure={:?} windows={:?} laundered={:?}",
                actual.bad_structure, actual.bad_windows, actual.laundered_notifications
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.65.0 — verify a [`GradientSoundnessWitness`]: re-differentiate every
/// grad's original expression + simplify + compare; refute disagreement
/// or any surviving violation. A hand-edited gradient cannot deploy.
fn check_gradient_soundness(
    claimed: &super::proof_term::GradientSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match super::generate::derive_gradient_soundness_witness(ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: "no grad step exists in this artifact — forged or stale proof"
                    .to_string(),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.violations.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T931/axon-T932 gradient law violated (a stored derivative does not \
                 match its re-derivation — hand-edited or stale artifact): {:?}",
                actual.violations
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.63.0 — verify a [`DataspaceSchemaSoundnessWitness`]: re-derive the
/// schema surface + the T928/T930 laws from the artifact; refute on any
/// disagreement or surviving violation.
fn check_dataspace_schema_soundness(
    claimed: &super::proof_term::DataspaceSchemaSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match super::generate::derive_dataspace_schema_soundness_witness(ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: "no dataspace surface exists in this artifact — forged or stale proof"
                    .to_string(),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.violations.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T928/axon-T930 dataspace-schema law violated (the compile-time check \
                 did not run over this IR — stale or hand-edited artifact): {:?}",
                actual.violations
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.62.0 — verify a [`QuerySafetySoundnessWitness`]. The checker independently
/// RE-DERIVES the whole-program QUERY safety from `ir.endpoints` + `ir.flows` +
/// the egress declarations, and rejects on ANY of: a forged/stale witness, a QUERY
/// endpoint whose flow reaches a declared write (`axon-T927` — RFC 10008 section 2 says a
/// QUERY MUST change no state; caches and proxies are entitled to retry it), or a
/// program-level `deliver`/`document` egress (which fires for every flow).
fn check_query_safety_soundness(
    claimed: &super::proof_term::QuerySafetySoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match super::generate::derive_query_safety_soundness_witness(ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: "no QUERY endpoint exists in this artifact — forged or stale proof"
                    .to_string(),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.unsafe_queries.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T927 QUERY-safety: endpoint(s) declare `method: QUERY` but their flow \
                 performs a declared write (RFC 10008 section 2 — a QUERY MUST be safe and idempotent; \
                 caches, proxies and clients may retry it freely): {:?}",
                actual.unsafe_queries
            ),
        };
    }
    if !actual.egress_declarations.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T927 QUERY-safety: this program declares an egress that fires for every \
                 flow, so no QUERY endpoint here can be safe: {:?}",
                actual.egress_declarations
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.54.0 — verify a [`DocumentIngestionSoundnessWitness`]. The checker
/// independently RE-DERIVES the whole-program ingestion provenance from
/// `ir.tools` + `ir.flows` and rejects on ANY of: a forged/stale witness, a tool
/// declaring `ingest:inferred` + `epistemic:know` (T1001, the Inferred ceiling),
/// or a flow feeding ingested content to an agent's beliefs unshielded (T908).
fn check_document_ingestion_soundness(
    claimed: &super::proof_term::DocumentIngestionSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match super::generate::derive_document_ingestion_soundness_witness(ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: "no ingestion contract exists in this artifact (no `ingest:*` tool) — \
                         forged or stale proof"
                    .to_string(),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.inferred_ceiling_violations.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T1001 tool(s) declare `ingest:inferred` + `epistemic:know` (an inferred \
                 read can never be `know`): {:?}",
                actual.inferred_ceiling_violations
            ),
        };
    }
    if !actual.unshielded_flows.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T908 ingestion barrier: flow(s) feed ingested content to an agent's beliefs \
                 with no shield: {:?}",
                actual.unshielded_flows
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.54.0 — verify an [`InferredCeilingSoundnessWitness`]. The checker
/// independently RE-DERIVES the inferred-producer set from `ir.tools` + `ir.flows`
/// and rejects on ANY of: a forged/stale witness, an absent producer set (the
/// v2.54.0 vacuum still holds — no proof was owed), a producer declaring
/// `epistemic:know` (T1001, the ceiling), or a flow feeding an inferred read to
/// an agent's beliefs unshielded (T908).
fn check_inferred_ceiling_soundness(
    claimed: &super::proof_term::InferredCeilingSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match super::generate::derive_inferred_ceiling_soundness_witness(ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: "no `ingest:inferred` producer exists in this artifact — the ingestion vacuum \
                         holds; forged or stale proof"
                    .to_string(),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    if !actual.ceiling_violations.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T1001 `ingest:inferred` producer(s) also declare `epistemic:know` (an \
                 inferred read can never be `know`): {:?}",
                actual.ceiling_violations
            ),
        };
    }
    if !actual.unshielded_flows.is_empty() {
        return CheckOutcome::Refuted {
            reason: format!(
                "axon-T908 barrier: flow(s) feed an inferred read to an agent's beliefs with no \
                 shield: {:?}",
                actual.unshielded_flows
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.36.0 — verify an [`InterruptibleSessionSoundnessWitness`]. The checker
/// re-derives every fact from the IR session steps and then applies the
/// four soundness verdicts of the 79.a paper section 6.2: closed-catalog signal, both
/// arms present, and a two-exit handler.
fn check_interruptible_session_soundness(
    claimed: &super::proof_term::InterruptibleSessionSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match derive_interruptible_session_witness(
        &claimed.session_name,
        &claimed.role_name,
        &claimed.signal,
        ir,
    ) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "no interrupt region with signal '{}' in session '{}' role '{}' — \
                     forged or stale proof",
                    claimed.signal, claimed.session_name, claimed.role_name
                ),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    // Verdict 1: closed CallInterruptCause catalog.
    if !actual.signal_in_catalog {
        return CheckOutcome::Refuted {
            reason: format!(
                "interrupt signal '{}' is not a CallInterruptCause {:?}",
                actual.signal, CALL_INTERRUPT_CAUSES
            ),
        };
    }
    // Verdict 2: both a body and a resumable handler.
    if !actual.has_body || !actual.has_handler {
        return CheckOutcome::Refuted {
            reason: format!(
                "interrupt region in session '{}' role '{}' is missing its {} arm",
                actual.session_name,
                actual.role_name,
                if !actual.has_body { "body" } else { "resumable handler" }
            ),
        };
    }
    // Verdict 3: two-exit handler.
    if !actual.handler_reaches_exit {
        return CheckOutcome::Refuted {
            reason: format!(
                "interrupt handler in session '{}' role '{}' does not reach a two-exit \
                 terminal (`resume` or `end`)",
                actual.session_name, actual.role_name
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.36.0 — verify a [`ParkedResidualSoundnessWitness`]: the socket's parked κ is
/// AAD-bound + recoverable (`reconnect: cognitive_state`) and its at-rest
/// retention is governed (`legal_basis`). Re-derived from the IR.
fn check_parked_residual_soundness(
    claimed: &super::proof_term::ParkedResidualSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match derive_parked_residual_witness(&claimed.socket_name, ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "socket '{}' carries no interruptible session in this artifact (no parked-\
                     residual obligation) — forged or stale proof",
                    claimed.socket_name
                ),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    // Verdict 1: the park must be AAD-bound + recoverable (v2.3.0).
    if !actual.reconnect_cognitive_state {
        return CheckOutcome::Refuted {
            reason: format!(
                "socket '{}' parks an interruptible residual but does not declare `reconnect: \
                 cognitive_state` — the at-rest κ would be an unsealed second store",
                actual.socket_name
            ),
        };
    }
    // Verdict 2: the at-rest retention must be governed.
    if !actual.legal_basis_declared {
        return CheckOutcome::Refuted {
            reason: format!(
                "socket '{}' parks a possibly-PII-bearing residual but declares no `legal_basis` \
                 — the at-rest retention TTL has no legal ceiling",
                actual.socket_name
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.37.0 — verify an [`UpstreamProjectionSoundnessWitness`]: the upstream's
/// `map:` projection is a total, unambiguous cover of the bound role's
/// message set (T849 re-derived) and its `resolve:`/`secret:` values are
/// policy-shaped config keys (T850 re-derived). Everything is recomputed
/// from the IR; a forged `projection_total: true` cannot survive.
fn check_upstream_projection_soundness(
    claimed: &super::proof_term::UpstreamProjectionSoundnessWitness,
    ir: &IRProgram,
) -> CheckOutcome {
    let actual = match derive_upstream_projection_witness(&claimed.upstream_name, ir) {
        Some(w) => w,
        None => {
            return CheckOutcome::Refuted {
                reason: format!(
                    "upstream '{}' does not exist in this artifact — forged or stale proof",
                    claimed.upstream_name
                ),
            }
        }
    };
    if actual != *claimed {
        return CheckOutcome::Refuted {
            reason: "witness disagrees with artifact re-derivation (forged or stale proof)"
                .to_string(),
        };
    }
    // Verdict 1: no message may fall through untranscoded (v2.37.0's core claim).
    if !actual.projection_total {
        return CheckOutcome::Refuted {
            reason: format!(
                "upstream '{}': the `map:` projection is not a total, unambiguous cover of \
                 session '{}' role '{}' — a message would cross the boundary untranscoded (T849)",
                actual.upstream_name, actual.session_name, actual.role_name
            ),
        };
    }
    // Verdict 2: no vendor coordinate may live in source (config, not code).
    if !actual.config_keys_valid {
        return CheckOutcome::Refuted {
            reason: format!(
                "upstream '{}': `resolve:`/`secret:` are not policy-shaped config keys — an \
                 endpoint or credential literal is in the artifact (T850)",
                actual.upstream_name
            ),
        };
    }
    CheckOutcome::Verified
}

/// v2.36.0 — the overall verdict for a [`CallSoundnessCertificate`]: EVERY
/// composed member must independently verify against `ir`. Returns the first
/// non-`Verified` member's outcome (so the caller sees *why* a call is not
/// certified sound), or `Verified` when all hold. An empty certificate (a
/// socket with no interruptible session) is vacuously `Verified` — there is no
/// call-soundness obligation to discharge.
pub fn check_call_soundness_certificate(
    cert: &CallSoundnessCertificate,
    ir: &IRProgram,
) -> CheckOutcome {
    for proof in &cert.proofs {
        let outcome = check_proof(proof, ir);
        if outcome != CheckOutcome::Verified {
            return outcome;
        }
    }
    CheckOutcome::Verified
}
