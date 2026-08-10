//! §Fase 51.a — Proof generation (the producer / compiler side).
//!
//! Walks an [`IRProgram`] and emits one [`ProofTerm`] per apx /
//! axonendpoint that declares a `compliance:` set. Generation records
//! the DERIVATION (the witness) for every compliance-bearing endpoint;
//! it does NOT decide the verdict — that is the independent checker's
//! job ([`crate::pcc::checker::check_proof`]). This split is deliberate:
//! the producer hands the consumer a derivation, the consumer
//! re-checks it (D51.2). A defective endpoint (phantom class / no
//! shield) still gets a proof term; the checker renders `Refuted` so
//! the defect is surfaced, not hidden.

use crate::ir_nodes::{IRProgram, IRSessionStep};

use super::effects;
use super::proof_term::{
    AggregateSoundnessWitness, CallSoundnessCertificate, CapabilityContainmentWitness,
    AuthorizationCoverageWitness, CapabilityGrantabilityWitness,
    CapabilityIsolationWitness,
    ChannelEgressSoundnessWitness,
    ChannelDeliverySoundnessWitness, ComplianceCoverageWitness, CorsPolicyConsistencyWitness,
    EffectBudgetedWitness,
    EffectRowSoundnessWitness, InterruptibleSessionSoundnessWitness, JsonShapeSoundnessWitness,
    ParkedResidualSoundnessWitness, ProofTerm, PropertyClass,
    CacheSoundnessWitness, ForgeSoundnessWitness, ResourceBoundsWitness, SavantSoundnessWitness,
    DocumentIngestionSoundnessWitness, DocumentProvenanceSoundnessWitness,
    DataspaceSchemaSoundnessWitness, DeliveryProvenanceSoundnessWitness,
    GradientSoundnessWitness, NotificationProvenanceSoundnessWitness,
    QuerySafetySoundnessWitness,
    InferredCeilingSoundnessWitness,
    ScopeCoverageSoundnessWitness, ScrapeProvenanceSoundnessWitness, WardenSoundnessWitness,
    ShieldHaltGuaranteeWitness, TechnicianCommandSafetyWitness, ToolCallSoundnessWitness,
    UpstreamProjectionSoundnessWitness, Witness,
    CALL_INTERRUPT_CAUSES, MAX_RETRIES, VALID_BREACH_POLICIES, VALID_BUDGET_PERIODS,
    VALID_ON_EXHAUSTED,
};

/// Canonical SHA-256 hex digest of the IR artifact. Reuses the
/// `esk::provenance` canonicalizer so the producer + the independent
/// checker compute byte-identical digests. A serialization failure
/// (practically unreachable for the derive-`Serialize` IR) degrades to
/// a fixed sentinel — still consistent between generate + check, so
/// the digest binding stays sound even in the degenerate case.
pub fn artifact_digest(ir: &IRProgram) -> String {
    match serde_json::to_value(ir) {
        Ok(v) => crate::esk::provenance::content_hash(&v),
        Err(_) => "<ir-unserializable>".to_string(),
    }
}

/// Sort + dedup a class list into the canonical form the witness
/// carries (so the checker's re-derivation compares equal).
fn canonical_classes(raw: &[String]) -> Vec<String> {
    let mut v: Vec<String> = raw.to_vec();
    v.sort();
    v.dedup();
    v
}

/// §51.a — derive a [`ComplianceCoverageWitness`] for one endpoint
/// against the program IR. Pure + total. Shared with the checker's
/// re-derivation path so producer + verifier compute identically (the
/// checker calls this to recompute, then compares against the proof's
/// witness — D51.2).
pub fn derive_compliance_coverage_witness(
    endpoint_name: &str,
    declared_compliance: &[String],
    shield_ref: &str,
    ir: &IRProgram,
) -> ComplianceCoverageWitness {
    let required_classes = canonical_classes(declared_compliance);

    // Resolve the shield once; reuse for presence + provided coverage.
    let resolved_shield = if shield_ref.is_empty() {
        None
    } else {
        ir.shields.iter().find(|s| s.name == shield_ref)
    };
    let shield_present = resolved_shield.is_some();
    let provided_classes = resolved_shield
        .map(|s| canonical_classes(&s.compliance))
        .unwrap_or_default();

    let unknown_classes: Vec<String> = required_classes
        .iter()
        .filter(|c| !crate::esk::compliance::is_known(c))
        .cloned()
        .collect();

    // The coverage gap: required classes the shield does NOT provide.
    // Reuses the canonical `compliance::covers` predicate (the producer
    // + the independent checker share this exact derivation).
    let mut uncovered_classes: Vec<String> =
        crate::esk::compliance::covers(provided_classes.iter(), required_classes.iter())
            .into_iter()
            .collect();
    uncovered_classes.sort();

    ComplianceCoverageWitness {
        endpoint_name: endpoint_name.to_string(),
        required_classes,
        shield_ref: shield_ref.to_string(),
        shield_present,
        provided_classes,
        unknown_classes,
        uncovered_classes,
    }
}

/// §51.a — generate compliance-coverage proofs for every apx /
/// axonendpoint in `ir` that declares a non-empty `compliance:` set.
///
/// D51.5 — apx (`axpoint`) and `axonendpoint` share the IR
/// `endpoints` node family, so this one walk covers both. Endpoints
/// with no compliance declaration produce no proof (nothing to
/// certify).
pub fn generate_compliance_coverage_proofs(ir: &IRProgram, axon_version: &str) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut proofs = Vec::new();
    for ep in &ir.endpoints {
        if ep.compliance.is_empty() {
            continue;
        }
        let witness =
            derive_compliance_coverage_witness(&ep.name, &ep.compliance, &ep.shield_ref, ir);
        proofs.push(ProofTerm {
            property: PropertyClass::ComplianceCoverage,
            artifact_digest: digest.clone(),
            witness: Witness::ComplianceCoverage(witness),
            axon_version: axon_version.to_string(),
        });
    }
    proofs
}

/// §89.c — derive an [`AuthorizationCoverageWitness`] for one endpoint. Pure +
/// total. Shared with the checker's re-derivation path (D51.2): both the prover
/// and the verifier compute coverage the SAME way, so the witnesses agree by
/// construction, and a forged `authorized: true` is caught by recomputation.
pub fn derive_authorization_coverage_witness(
    ep: &crate::ir_nodes::IRAxonEndpoint,
) -> AuthorizationCoverageWitness {
    let dispatches = !ep.execute_flow.is_empty();
    let has_requires = !ep.requires_capabilities.is_empty();
    let has_shield = !ep.shield_ref.is_empty();
    let has_compliance = !ep.compliance.is_empty();
    let public = ep.public;
    // The §89.b rule EXACTLY: covered by ≥1 discipline OR the explicit opt-out.
    let authorized = has_requires || has_shield || has_compliance || public;
    AuthorizationCoverageWitness {
        endpoint_name: ep.name.clone(),
        dispatches,
        has_requires,
        has_shield,
        has_compliance,
        public,
        authorized,
    }
}

/// §90.b — derive the whole-program [`CapabilityGrantabilityWitness`] given the
/// grantable authority catalog (RBAC colon perms ∪ reserved dotted caps ∪
/// SA-grantable). Collects the distinct `requires:` scopes across DISPATCHING
/// endpoints, re-projects the catalog through `π`, and records whether the
/// namespace is clean AND every requirement is grantable. The manifest is an
/// INPUT because pure OSS has no authority system — the enterprise supplies its
/// live RBAC catalog at generation time; the independent checker re-derives all
/// of this from the IR + the witness's own `authorities`.
pub fn derive_capability_grantability_witness(
    ir: &IRProgram,
    authorities: &[String],
) -> CapabilityGrantabilityWitness {
    use std::collections::BTreeSet;
    let required: Vec<String> = ir
        .endpoints
        .iter()
        .filter(|e| !e.execute_flow.is_empty())
        .flat_map(|e| e.requires_capabilities.iter().cloned())
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect();
    let mut authorities_sorted: Vec<String> =
        authorities.iter().cloned().collect::<BTreeSet<String>>().into_iter().collect();
    authorities_sorted.sort();

    let grantable = crate::auth_scope::build_grantable_set(authorities_sorted.iter());
    let all_grantable = grantable.is_clean()
        && matches!(
            crate::auth_scope::check_grantable(&required, &grantable.caps),
            crate::auth_scope::GrantabilityVerdict::Grantable
        );

    CapabilityGrantabilityWitness {
        required,
        authorities: authorities_sorted,
        all_grantable,
    }
}

/// §89.c — generate an AuthorizationCoverage proof for every DISPATCHING
/// `axonendpoint` (non-empty `execute:` — a real trust boundary). A
/// non-dispatching endpoint crosses no boundary → no proof (mirrors "no
/// compliance → no compliance proof"). The doctrine `every_boundary_is_guarded`
/// as an independently-verifiable, deploy-gated fact.
pub fn generate_authorization_coverage_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut proofs = Vec::new();
    for ep in &ir.endpoints {
        if ep.execute_flow.is_empty() {
            continue;
        }
        proofs.push(ProofTerm {
            property: PropertyClass::AuthorizationCoverage,
            artifact_digest: digest.clone(),
            witness: Witness::AuthorizationCoverage(derive_authorization_coverage_witness(ep)),
            axon_version: axon_version.to_string(),
        });
    }
    proofs
}

/// §90.b — emit the whole-program [`PropertyClass::CapabilityGrantability`]
/// proof, GIVEN the grantable authority catalog. One proof per program (like
/// `CacheSoundness`). Emitted only when the program has ≥1 dispatching endpoint
/// declaring a `requires:` — a program with no capability requirements has no
/// grantability obligation. The enterprise supplies its live RBAC catalog
/// (`authorities`); a pure-OSS `axon pcc prove` without a catalog does not emit
/// this class (no authority system to check against).
pub fn generate_capability_grantability_proofs(
    ir: &IRProgram,
    authorities: &[String],
    axon_version: &str,
) -> Vec<ProofTerm> {
    let has_requirement = ir
        .endpoints
        .iter()
        .any(|e| !e.execute_flow.is_empty() && !e.requires_capabilities.is_empty());
    if !has_requirement {
        return Vec::new();
    }
    vec![ProofTerm {
        property: PropertyClass::CapabilityGrantability,
        artifact_digest: artifact_digest(ir),
        witness: Witness::CapabilityGrantability(derive_capability_grantability_witness(
            ir,
            authorities,
        )),
        axon_version: axon_version.to_string(),
    }]
}

/// §51.b — derive an [`EffectRowSoundnessWitness`] for one tool's
/// declared effect row. Pure + total. Shared with the checker's
/// re-derivation path (D51.2).
pub fn derive_effect_row_soundness_witness(
    tool_name: &str,
    effect_row: &[String],
    // §Fase 53.d — the extension-declared PROVENANCE members the checker
    // honors, re-derived INDEPENDENTLY from the artifact's own
    // `extensions` by the caller (see `extension_effect_members`). Empty
    // for an artifact with no `extension` declarations (byte-identical
    // pre-§53 behavior).
    extension_effect_members: &std::collections::HashSet<String>,
) -> EffectRowSoundnessWitness {
    let declared_effects = canonical_classes(effect_row);

    let mut unknown_bases = Vec::new();
    let mut missing_qualifier = Vec::new();
    let mut invalid_stream_qualifier = Vec::new();
    let mut has_pure = false;
    let mut has_other = false;

    for entry in &declared_effects {
        // §Fase 53.d / §53.c.2 — a PROVENANCE member is accepted VERBATIM
        // (the full entry). Two sources: an `extension`-declared member,
        // or the built-in `epistemic:<level>` confidence axis. Both carry
        // no runtime capability (invariant #2), so neither is an unknown
        // base nor subject to qualifier enforcement; both count as "other"
        // for purity (a tool declaring `pure` + a provenance effect is
        // still a contradiction).
        if extension_effect_members.contains(entry) || effects::is_epistemic_provenance(entry) {
            has_other = true;
            continue;
        }
        let (base, qualifier) = effects::split_effect(entry);
        if !effects::is_known_base(base) {
            unknown_bases.push(entry.clone());
            // An unknown base is also "other" for purity purposes —
            // a tool with `pure` + a phantom effect is still a
            // contradiction. (has_other set below covers it.)
            has_other = true;
            continue;
        }
        if base == "pure" {
            has_pure = true;
        } else {
            has_other = true;
        }
        if effects::requires_qualifier(base) && qualifier.is_none() {
            missing_qualifier.push(entry.clone());
        }
        if base == "stream" {
            if let Some(q) = qualifier {
                if !effects::is_valid_stream_qualifier(q) {
                    invalid_stream_qualifier.push(entry.clone());
                }
            }
        }
    }

    let purity_violation = has_pure && has_other;

    EffectRowSoundnessWitness {
        tool_name: tool_name.to_string(),
        declared_effects,
        unknown_bases,
        missing_qualifier,
        invalid_stream_qualifier,
        purity_violation,
    }
}

/// §Fase 53.d — the set of extension-declared PROVENANCE effect members
/// the PCC checker honors, re-derived INDEPENDENTLY from the artifact's
/// own `extensions` (soundness invariant #1 — the verifier never trusts
/// an external registry or the producer's compiler; D51.2). Invariant #2
/// is enforced here independently: a member whose base IS a canonical
/// enforceable base is NOT a provenance member (it is not "rescued" by
/// the extension), so it is excluded and falls through to the canonical
/// base/qualifier checks. Both the prover and the checker call this over
/// the SAME `ir`, so the re-derived witnesses agree by construction.
pub fn extension_effect_members(ir: &IRProgram) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    for ext in &ir.extensions {
        if ext.category != "effects" {
            continue;
        }
        for m in &ext.members {
            let (base, _) = effects::split_effect(&m.name);
            if !effects::is_known_base(base) {
                set.insert(m.name.clone());
            }
        }
    }
    set
}

/// §51.b — generate effect-row-soundness proofs for every tool in `ir`
/// that declares a non-empty `effects: <...>` row. Tools with no
/// declared effects produce no proof (nothing to certify).
pub fn generate_effect_row_soundness_proofs(ir: &IRProgram, axon_version: &str) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    // §Fase 53.d — provenance members declared by the artifact's own
    // extensions, re-derived once for the whole program.
    let ext_members = extension_effect_members(ir);
    let mut proofs = Vec::new();
    for tool in &ir.tools {
        if tool.effect_row.is_empty() {
            continue;
        }
        let witness =
            derive_effect_row_soundness_witness(&tool.name, &tool.effect_row, &ext_members);
        proofs.push(ProofTerm {
            property: PropertyClass::EffectRowSoundness,
            artifact_digest: digest.clone(),
            witness: Witness::EffectRowSoundness(witness),
            axon_version: axon_version.to_string(),
        });
    }
    proofs
}

/// §51.c — derive a [`CapabilityIsolationWitness`] for one store's
/// capability gate. Pure + total. Shared with the checker's
/// re-derivation path (D51.2). Grammar validity delegates to the OSS
/// single-source-of-truth `axon_frontend::parser::is_valid_capability_slug`
/// (re-exported as `crate::parser`) — the checker re-derives the FACT
/// (this store's gate slug) and re-runs the canonical validator; it
/// does not trust the witness.
pub fn derive_capability_isolation_witness(
    store_name: &str,
    capability: &str,
) -> CapabilityIsolationWitness {
    let malformed = !capability.is_empty() && !crate::parser::is_valid_capability_slug(capability);
    CapabilityIsolationWitness {
        store_name: store_name.to_string(),
        capability: capability.to_string(),
        malformed,
    }
}

/// §51.c — generate capability-isolation proofs for every `axonstore`
/// in `ir` that declares a non-empty `capability` gate. Stores with no
/// gate produce no proof (nothing to certify — an ungated store is out
/// of scope for the gate-integrity property).
pub fn generate_capability_isolation_proofs(ir: &IRProgram, axon_version: &str) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut proofs = Vec::new();
    for store in &ir.axonstore_specs {
        if store.capability.is_empty() {
            continue;
        }
        let witness = derive_capability_isolation_witness(&store.name, &store.capability);
        proofs.push(ProofTerm {
            property: PropertyClass::CapabilityIsolation,
            artifact_digest: digest.clone(),
            witness: Witness::CapabilityIsolation(witness),
            axon_version: axon_version.to_string(),
        });
    }
    proofs
}

/// §51.d — derive the retry-bound witness for one endpoint. Pure +
/// total. Shared with the checker (D51.2).
pub fn derive_endpoint_retry_witness(endpoint_name: &str, retries: i64) -> ResourceBoundsWitness {
    ResourceBoundsWitness::EndpointRetry {
        endpoint_name: endpoint_name.to_string(),
        retries,
        in_bounds: (0..=MAX_RETRIES).contains(&retries),
    }
}

/// §51.d — derive the credit-positivity witness for one socket's
/// DECLARED credit window. Pure + total. Shared with the checker.
pub fn derive_socket_credit_witness(socket_name: &str, credit: i64) -> ResourceBoundsWitness {
    ResourceBoundsWitness::SocketCredit {
        socket_name: socket_name.to_string(),
        credit,
        positive: credit >= 1,
    }
}

/// §51.d — generate resource-bound proofs: one retry-bound proof per
/// apx/axonendpoint, plus one credit-positivity proof per socket that
/// DECLARES a `backpressure: credit(k)` window. Sockets with an
/// unspecified credit produce no proof (unspecified is a legitimate
/// type state, not a bound to certify). `timeout` is out of scope by
/// design (closed duration enum, bounded at parse).
pub fn generate_resource_bounds_proofs(ir: &IRProgram, axon_version: &str) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut proofs = Vec::new();
    for ep in &ir.endpoints {
        let witness = derive_endpoint_retry_witness(&ep.name, ep.retries);
        proofs.push(ProofTerm {
            property: PropertyClass::ResourceBounds,
            artifact_digest: digest.clone(),
            witness: Witness::ResourceBounds(witness),
            axon_version: axon_version.to_string(),
        });
    }
    for socket in &ir.sockets {
        if let Some(credit) = socket.backpressure_credit {
            let witness = derive_socket_credit_witness(&socket.name, credit);
            proofs.push(ProofTerm {
                property: PropertyClass::ResourceBounds,
                artifact_digest: digest.clone(),
                witness: Witness::ResourceBounds(witness),
                axon_version: axon_version.to_string(),
            });
        }
    }
    proofs
}

/// §51.e — derive a [`ShieldHaltGuaranteeWitness`] for one shield's
/// breach policy. Pure + total. Shared with the checker (D51.2).
///
/// §Fase 77.a (D77.6) — a shield whose enforcement is `sign:` (egress
/// signing) is NOT vacuous: the signature is what it enforces, so
/// `on_breach: halt` has a breach to react to (a delivery it refuses to
/// sign). `vacuous_halt` therefore requires BOTH `scan` and `sign` empty.
pub fn derive_shield_halt_witness(
    shield_name: &str,
    on_breach: &str,
    scan: &[String],
    sign: &str,
) -> ShieldHaltGuaranteeWitness {
    let known_policy = VALID_BREACH_POLICIES.contains(&on_breach);
    let scan_count = scan.len();
    let vacuous_halt = on_breach == "halt" && scan.is_empty() && sign.is_empty();
    ShieldHaltGuaranteeWitness {
        shield_name: shield_name.to_string(),
        on_breach: on_breach.to_string(),
        known_policy,
        scan_count,
        vacuous_halt,
    }
}

/// §51.e — generate shield-halt-guarantee proofs for every shield in
/// `ir` that declares a non-empty `on_breach` policy. Shields with no
/// breach policy declared produce no proof (no guarantee to certify).
pub fn generate_shield_halt_guarantee_proofs(ir: &IRProgram, axon_version: &str) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut proofs = Vec::new();
    for shield in &ir.shields {
        if shield.on_breach.is_empty() {
            continue;
        }
        let witness = derive_shield_halt_witness(
            &shield.name,
            &shield.on_breach,
            &shield.scan,
            &shield.sign,
        );
        proofs.push(ProofTerm {
            property: PropertyClass::ShieldHaltGuarantee,
            artifact_digest: digest.clone(),
            witness: Witness::ShieldHaltGuarantee(witness),
            axon_version: axon_version.to_string(),
        });
    }
    proofs
}

/// §51.x — recursively collect every store name a flow's steps reach
/// (Retrieve / Persist / Mutate / Purge), descending into BOTH
/// conditional branches + the for-in loop body. A SOUND
/// over-approximation: every statically-reachable store op is counted
/// (we do not know which branch fires at runtime, so both count), so a
/// containment proof never misses a reachable gate. Total + bounded.
///
/// ## §51.x.3 — no-silent-gap invariant (compiler-enforced)
///
/// The match below is **exhaustive — there is NO `_` wildcard arm**.
/// Every [`IRFlowNode`](crate::ir_nodes::IRFlowNode) variant is
/// classified deliberately into exactly one of three buckets:
///
/// - **store op** — names a capability-gated `axonstore_specs` entry
///   (the four CRUD verbs Retrieve/Persist/Mutate/Purge are, by
///   construction, the ONLY axonstore-touching nodes);
/// - **nesting** — carries a nested `Vec<IRFlowNode>` body to recurse
///   into (ONLY `Conditional` then/else + `ForIn` body);
/// - **leaf** — carries neither an axonstore reference nor a nested
///   body, so it contributes nothing to the reachable-gate set.
///
/// Because there is no wildcard, **adding a new `IRFlowNode` variant
/// breaks compilation here** until a maintainer classifies it — the
/// reachability walk can never silently miss a future node that adds a
/// store reference or a nested body. (A `cargo test` source gate also
/// pins the absence of a wildcard so a refactor cannot reintroduce one.)
///
/// Notes on the leaf classification:
/// - `Remember` / `Recall` reference the cognitive **memory** subsystem
///   (`memory_target` / `memory_source`), a DIFFERENT subsystem from the
///   capability-gated axonstore; if memory ever becomes capability-gated
///   that is a NEW property class, not this walk.
/// - `Par` / `Deliberate` / `Consensus` / `Forge` / `Stream` / `Transact`
///   are payload-free in the IR (no nested `IRFlowNode` body).
/// - If a flow-invocation node (cf. the top-level [`IRRun`](crate::ir_nodes::IRRun),
///   which today lives ONLY at program level and is unreachable from a
///   flow body) ever enters `IRFlowNode`, this is where transitive
///   cross-flow reachability must be REOPENED (§51.x.3).
fn collect_store_accesses(steps: &[crate::ir_nodes::IRFlowNode], out: &mut Vec<String>) {
    use crate::ir_nodes::IRFlowNode as N;
    for step in steps {
        match step {
            // §Fase 109 — a grad touches no store.
            N::Grad(_) => {}
            // ── store ops — the only axonstore-touching nodes ──
            N::Retrieve(s) => out.push(s.store_name.clone()),
            N::Persist(s) => out.push(s.store_name.clone()),
            N::Mutate(s) => out.push(s.store_name.clone()),
            N::Purge(s) => out.push(s.store_name.clone()),
            // ── nesting — the only nodes with a nested body ──
            N::Conditional(c) => {
                collect_store_accesses(&c.then_body, out);
                collect_store_accesses(&c.else_body, out);
            }
            N::ForIn(f) => collect_store_accesses(&f.body, out),
            // §Fase 51.a — `quant` carries a nested flow body; descend so any
            // store op reachable inside it is still soundness-checked.
            N::Quant(q) => collect_store_accesses(&q.body, out),
            // §Fase 88.a — `warden` carries a nested flow body; descend so any
            // store op reachable inside it is still soundness-checked.
            N::Warden(w) => collect_store_accesses(&w.body, out),
            // §Fase 51.d.2 — `yield` is a leaf (measurement value, no store op).
            N::Yield(_) => {}
            // §Fase 92.b — `mint` is a leaf effect (no store op by
            // construction — axon-T896 forbids its binding from entering one).
            N::Mint(_) => {}
            // §Fase 94.b — `rotate` READS its secrets store (the class
            // metadata view) to enumerate matching keys; its capability
            // gate must be honored like any other store access.
            N::Rotate(s) => out.push(s.store_ref.clone()),
            // §Fase 52.a — a `listen` handler body can contain store ops (a
            // daemon cleaner that `persist`s); descend so they're soundness-
            // checked, like the quant body above.
            N::Listen(l) => collect_store_accesses(&l.body, out),
            // §Fase 52.c — `run <Flow>` invokes a flow by NAME (no nested body),
            // so it touches no store DIRECTLY. Transitive store access through
            // the invoked flow is the cross-flow reachability this fn's header
            // flagged — deferred to the PCC DaemonSoundness follow-up; a leaf here.
            N::Run(_) => {}
            // ── leaves — no axonstore ref, no nested body. Listed
            // EXPLICITLY (no `_` wildcard) so a future variant forces a
            // deliberate classification at compile time (§51.x.3). ──
            N::Step(_)
            | N::Probe(_)
            | N::Reason(_)
            | N::Validate(_)
            | N::Refine(_)
            | N::Weave(_)
            | N::UseTool(_)
            | N::Remember(_)
            | N::Recall(_)
            | N::Let(_)
            | N::Return(_)
            | N::Break(_)
            | N::Continue(_)
            | N::LambdaDataApply(_)
            | N::Par(_)
            | N::Hibernate(_)
            | N::Deliberate(_)
            | N::Consensus(_)
            | N::Forge(_)
            | N::Focus(_)
            | N::Associate(_)
            | N::Aggregate(_)
            | N::Explore(_)
            | N::Ingest(_)
            | N::ShieldApply(_)
            | N::Stream(_)
            | N::Navigate(_)
            | N::Drill(_)
            | N::Trail(_)
            | N::Corroborate(_)
            | N::OtsApply(_)
            | N::MandateApply(_)
            | N::ComputeApply(_)
            // §Fase 119.m.3 — an agent call touches no store and dispatches no
            // keyword-arg tool DIRECTLY; the tools its loop reaches go through
            // un_use_tool, which is where those obligations are already
            // collected. If an agent ever gains a direct store surface, this is
            // the arm that must reopen.
            | N::AgentCall(_)
            | N::DaemonStep(_)
            | N::Emit(_)
            | N::Publish(_)
            | N::Discover(_)
            | N::Transact(_) => {}
        }
    }
}

/// §51.x — derive a [`CapabilityContainmentWitness`] for one endpoint
/// against the program IR. Pure + total. Shared with the checker
/// (D51.2). Resolves `execute_flow` in `ir.flows`, walks its reachable
/// store ops, resolves each store's capability gate, and computes the
/// uncovered set (`reached_gates \ declared_requires`).
pub fn derive_capability_containment_witness(
    endpoint_name: &str,
    execute_flow: &str,
    declared_requires_raw: &[String],
    ir: &IRProgram,
) -> CapabilityContainmentWitness {
    let declared_requires = canonical_classes(declared_requires_raw);

    let flow = ir.flows.iter().find(|f| f.name == execute_flow);
    let flow_resolved = flow.is_some();

    // Reachable store names (sound over-approximation).
    let mut reached_stores: Vec<String> = Vec::new();
    if let Some(f) = flow {
        collect_store_accesses(&f.steps, &mut reached_stores);
    }

    // Resolve each reached store to its capability gate (non-empty only).
    let mut reached_gates: Vec<String> = reached_stores
        .iter()
        .filter_map(|name| {
            ir.axonstore_specs
                .iter()
                .find(|s| &s.name == name)
                .map(|s| s.capability.clone())
        })
        .filter(|cap| !cap.is_empty())
        .collect();
    reached_gates.sort();
    reached_gates.dedup();

    // uncovered = reached_gates \ declared_requires (the gates the flow
    // reaches that the endpoint does not declare). Reuses the canonical
    // `covers` predicate (required \ provided).
    let mut uncovered_gates: Vec<String> =
        crate::esk::compliance::covers(declared_requires.iter(), reached_gates.iter())
            .into_iter()
            .collect();
    uncovered_gates.sort();

    CapabilityContainmentWitness {
        endpoint_name: endpoint_name.to_string(),
        execute_flow: execute_flow.to_string(),
        flow_resolved,
        declared_requires,
        reached_gates,
        uncovered_gates,
    }
}

/// §51.x — generate capability-containment proofs. One proof per
/// apx/axonendpoint where the property is non-trivial: the endpoint
/// declares `requires:` OR its flow reaches at least one gated store.
/// (An endpoint with no requires that reaches no gated store has
/// nothing to certify.) The "reaches a gated store with no requires"
/// case IS generated — that is the capability-leak finding.
pub fn generate_capability_containment_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut proofs = Vec::new();
    for ep in &ir.endpoints {
        let witness = derive_capability_containment_witness(
            &ep.name,
            &ep.execute_flow,
            &ep.requires_capabilities,
            ir,
        );
        // Skip trivial subjects: no declared requires AND no reached
        // gates (nothing to certify).
        if witness.declared_requires.is_empty() && witness.reached_gates.is_empty() {
            continue;
        }
        proofs.push(ProofTerm {
            property: PropertyClass::CapabilityContainment,
            artifact_digest: digest.clone(),
            witness: Witness::CapabilityContainment(witness),
            axon_version: axon_version.to_string(),
        });
    }
    proofs
}

// ── §Fase 58.i — ToolCallSoundness ───────────────────────────────────

/// §58.i — mirror of the §58.d `infer_arg_literal_type` (a type-checker
/// private fn). PCC re-states the spec INDEPENDENTLY (D51.2 — the
/// verifier never trusts the compiler): only an UNAMBIGUOUS literal is
/// typed. A bare identifier (`x` — the frontend stored the value as a
/// bare string, so the literal `"x"` and the reference `x` are
/// indistinguishable) and a `${…}` interpolation are runtime-resolved →
/// `None` (skipped, so no false positives). Cross-stack spec pin: a
/// drift gate keeps this in lockstep with the frontend rule.
fn infer_arg_literal_type(value: &str) -> Option<&'static str> {
    if value == "true" || value == "false" {
        return Some("Bool");
    }
    if value.contains('.') && value.parse::<f64>().is_ok() {
        return Some("Float");
    }
    let digits = value.strip_prefix('-').unwrap_or(value);
    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
        return Some("Int");
    }
    None
}

/// §58.i — mirror of the §58.d `tool_arg_types_align`. `Any` accepts
/// anything; an `Int` coerces into a `Float` parameter; otherwise the
/// declared base type (stripping the `?` optional marker + any
/// `<generic>`) must equal the inferred literal type.
fn tool_arg_types_align(value_ty: &str, decl_ty: &str) -> bool {
    let base = decl_ty.trim_end_matches('?').split('<').next().unwrap_or(decl_ty);
    base == "Any" || base == value_ty || (base == "Float" && value_ty == "Int")
}

/// §58.i — collect, in deterministic walk order, every NAMED-args
/// `use <Tool>(k = v, …)` call in a flow's steps, recursing into
/// conditional branches + for-in bodies (a `use` cannot nest in a step
/// body — §54.a — but the IR model permits it inside flow-level control
/// flow, so the walk descends there). The legacy `use <Tool> on <arg>`
/// form has empty `named_args` and is excluded (schema-less, D5).
///
/// Like [`collect_store_accesses`], the match is EXHAUSTIVE — no `_`
/// wildcard — so a future `IRFlowNode` variant carrying a nested body
/// breaks compilation here until a maintainer classifies it (a wildcard
/// could let a nested `use` call silently escape soundness checking). A
/// source gate pins the absence of a wildcard.
fn collect_named_use_tool_calls<'a>(
    steps: &'a [crate::ir_nodes::IRFlowNode],
    out: &mut Vec<&'a crate::ir_nodes::IRUseToolStep>,
) {
    use crate::ir_nodes::IRFlowNode as N;
    for step in steps {
        match step {
            // §Fase 109 — a grad dispatches no tool.
            N::Grad(_) => {}
            // ── target — a structured (keyword-arg) tool dispatch ──
            N::UseTool(u) => {
                if !u.named_args.is_empty() {
                    out.push(u);
                }
            }
            // ── nesting — the only nodes with a nested IRFlowNode body ──
            N::Conditional(c) => {
                collect_named_use_tool_calls(&c.then_body, out);
                collect_named_use_tool_calls(&c.else_body, out);
            }
            N::ForIn(f) => collect_named_use_tool_calls(&f.body, out),
            // §Fase 51.a — `quant` carries a nested flow body; descend so a
            // structured `use` inside it cannot escape soundness checking.
            N::Quant(q) => collect_named_use_tool_calls(&q.body, out),
            // §Fase 88.a — `warden` carries a nested flow body; descend so a
            // structured `use` inside it cannot escape soundness checking.
            N::Warden(w) => collect_named_use_tool_calls(&w.body, out),
            // §Fase 51.d.2 — `yield` is a leaf (no nested body, no `use`).
            N::Yield(_) => {}
            // §Fase 92.b — `mint` is a leaf (no nested body, no `use`).
            N::Mint(_) => {}
            // §Fase 94.b — `rotate` is a leaf here: it references a tool,
            // but NOT as a structured `use` call — the exchange rides the
            // reserved `axon_rotation` envelope, not the tool's declared
            // `parameters:` schema, so §58.i schema-soundness does not
            // apply to it (the §94.e SecretCustodySoundness class owns it).
            N::Rotate(_) => {}
            // §Fase 52.a — a `listen` handler body can contain a `use <Tool>`;
            // descend so it is soundness-checked.
            N::Listen(l) => collect_named_use_tool_calls(&l.body, out),
            // §Fase 52.c — `run <Flow>` invokes a flow by name (no nested body).
            N::Run(_) => {}
            // ── leaves — no nested body. Listed EXPLICITLY (no `_`
            // wildcard) so a future nesting variant forces a deliberate
            // classification at compile time. ──
            N::Step(_)
            | N::Probe(_)
            | N::Reason(_)
            | N::Validate(_)
            | N::Refine(_)
            | N::Weave(_)
            | N::Remember(_)
            | N::Recall(_)
            | N::Let(_)
            | N::Return(_)
            | N::Break(_)
            | N::Continue(_)
            | N::LambdaDataApply(_)
            | N::Par(_)
            | N::Hibernate(_)
            | N::Deliberate(_)
            | N::Consensus(_)
            | N::Forge(_)
            | N::Focus(_)
            | N::Associate(_)
            | N::Aggregate(_)
            | N::Explore(_)
            | N::Ingest(_)
            | N::ShieldApply(_)
            | N::Stream(_)
            | N::Navigate(_)
            | N::Drill(_)
            | N::Trail(_)
            | N::Corroborate(_)
            | N::OtsApply(_)
            | N::MandateApply(_)
            | N::ComputeApply(_)
            // §Fase 119.m.3 — an agent call touches no store and dispatches no
            // keyword-arg tool DIRECTLY; the tools its loop reaches go through
            // un_use_tool, which is where those obligations are already
            // collected. If an agent ever gains a direct store surface, this is
            // the arm that must reopen.
            | N::AgentCall(_)
            | N::DaemonStep(_)
            | N::Emit(_)
            | N::Publish(_)
            | N::Discover(_)
            | N::Retrieve(_)
            | N::Persist(_)
            | N::Mutate(_)
            | N::Purge(_)
            | N::Transact(_) => {}
        }
    }
}

/// §58.i — sort + dedup a name list into the canonical witness form.
fn canonical_names(raw: &[String]) -> Vec<String> {
    let mut v = raw.to_vec();
    v.sort();
    v.dedup();
    v
}

/// §58.i — derive a [`ToolCallSoundnessWitness`] for the `use Tool(k=v)`
/// call at `call_index` (deterministic walk order) in flow `flow_name`.
/// Pure + total. Shared with the checker's re-derivation path (D51.2) —
/// the checker re-walks the SAME digest-bound IR, so producer + verifier
/// compute identically. `None` when the flow is absent or the index is
/// out of range (the checker renders that as a "call site not present"
/// refutation).
pub fn derive_tool_call_soundness_witness(
    flow_name: &str,
    call_index: usize,
    ir: &IRProgram,
) -> Option<ToolCallSoundnessWitness> {
    let flow = ir.flows.iter().find(|f| f.name == flow_name)?;
    let mut calls = Vec::new();
    collect_named_use_tool_calls(&flow.steps, &mut calls);
    let call = calls.get(call_index)?;

    let tool_name = call.tool_name.clone();
    let arg_pairs: Vec<(String, String)> = call
        .named_args
        .iter()
        .map(|a| (a.name.clone(), a.value.clone()))
        .collect();
    let arg_names: Vec<String> = arg_pairs.iter().map(|(n, _)| n.clone()).collect();

    // The called tool's declared schema (name, type, optional). Empty if
    // the tool is undeclared or schema-less.
    let params: Vec<(String, String, bool)> = ir
        .tools
        .iter()
        .find(|t| t.name == tool_name)
        .map(|t| {
            t.parameters
                .iter()
                .map(|p| (p.name.clone(), p.type_name.clone(), p.optional))
                .collect()
        })
        .unwrap_or_default();
    let schema_present = !params.is_empty();
    let declared_params = canonical_names(
        &params.iter().map(|(n, _, _)| n.clone()).collect::<Vec<_>>(),
    );

    // Duplicates: an arg name supplied more than once.
    let mut seen = std::collections::HashSet::new();
    let mut dup = std::collections::HashSet::new();
    for name in &arg_names {
        if !seen.insert(name.clone()) {
            dup.insert(name.clone());
        }
    }
    let duplicate_args = canonical_names(&dup.into_iter().collect::<Vec<_>>());

    // Unknown: an arg naming no declared parameter.
    let unknown_args = canonical_names(
        &arg_names
            .iter()
            .filter(|n| !params.iter().any(|(p, _, _)| &p == n))
            .cloned()
            .collect::<Vec<_>>(),
    );

    // Missing required: a non-optional param not supplied.
    let mut missing_required: Vec<String> = params
        .iter()
        .filter(|(p, _, optional)| !optional && !arg_names.iter().any(|n| n == p))
        .map(|(p, _, _)| p.clone())
        .collect();
    missing_required.sort();
    missing_required.dedup();

    // Literal type mismatches (conservative — decidable literals only).
    let mut type_mismatches: Vec<String> = Vec::new();
    for (name, value) in &arg_pairs {
        if let Some((_, decl_ty, _)) = params.iter().find(|(p, _, _)| p == name) {
            if let Some(val_ty) = infer_arg_literal_type(value) {
                if !tool_arg_types_align(val_ty, decl_ty) {
                    type_mismatches.push(format!("{name}:{decl_ty}:{val_ty}"));
                }
            }
        }
    }
    type_mismatches.sort();
    type_mismatches.dedup();

    Some(ToolCallSoundnessWitness {
        flow_name: flow_name.to_string(),
        call_index,
        tool_name,
        arg_names,
        declared_params,
        schema_present,
        unknown_args,
        duplicate_args,
        missing_required,
        type_mismatches,
    })
}

/// §58.i — generate tool-call-soundness proofs: one proof per structured
/// `use <Tool>(k = v, …)` call whose called tool declares a NON-EMPTY
/// `parameters:` schema. A call to a schema-less tool, an undeclared
/// tool, or the legacy `on <arg>` form carries no contract → no proof
/// (nothing to certify — mirrors "no effects → no effect-row proof").
pub fn generate_tool_call_soundness_proofs(ir: &IRProgram, axon_version: &str) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut proofs = Vec::new();
    for flow in &ir.flows {
        let mut calls = Vec::new();
        collect_named_use_tool_calls(&flow.steps, &mut calls);
        for (call_index, _) in calls.iter().enumerate() {
            // Derive from (flow, index) so producer + checker share the
            // exact path. `None` is unreachable here (we just walked the
            // same flow), but stay total.
            let Some(witness) = derive_tool_call_soundness_witness(&flow.name, call_index, ir)
            else {
                continue;
            };
            // Nothing to certify for a schema-less / undeclared tool.
            if !witness.schema_present {
                continue;
            }
            proofs.push(ProofTerm {
                property: PropertyClass::ToolCallSoundness,
                artifact_digest: digest.clone(),
                witness: Witness::ToolCallSoundness(witness),
                axon_version: axon_version.to_string(),
            });
        }
    }
    proofs
}

// ── §Fase 72.f — EffectBudgeted ──────────────────────────────────────

/// §72.f — re-derive an [`EffectBudgetedWitness`] for the daemon named
/// `daemon_name`. `None` if no such daemon exists, or it has no `budget`
/// (nothing to certify). RE-DERIVES every fact from the artifact — the same
/// computation the checker runs — so producer + checker agree by construction.
pub fn derive_effect_budgeted_witness(
    daemon_name: &str,
    ir: &IRProgram,
) -> Option<EffectBudgetedWitness> {
    let daemon = ir.daemons.iter().find(|d| d.name == daemon_name)?;
    let budget = daemon.budget.as_ref()?;

    // The program's declared tools — the resolution surface.
    let declared_tools = canonical_names(&ir.tools.iter().map(|t| t.name.clone()).collect::<Vec<_>>());
    let declared: std::collections::HashSet<&str> =
        declared_tools.iter().map(String::as_str).collect();

    let mut unresolved_effects = Vec::new();
    let mut nonpositive_limits = Vec::new();
    let mut invalid_periods = Vec::new();
    for q in &budget.quotas {
        if !declared.contains(q.effect.as_str()) {
            unresolved_effects.push(q.effect.clone());
        }
        if q.limit <= 0 {
            nonpositive_limits.push(format!("{}:{}", q.effect, q.kind));
        }
        if !VALID_BUDGET_PERIODS.contains(&q.period.as_str()) {
            invalid_periods.push(format!("{}:{}", q.effect, q.period));
        }
    }

    Some(EffectBudgetedWitness {
        daemon_name: daemon_name.to_string(),
        quota_count: budget.quotas.len(),
        declared_tools,
        unresolved_effects: canonical_names(&unresolved_effects),
        nonpositive_limits: {
            let mut v = nonpositive_limits;
            v.sort();
            v
        },
        invalid_periods: {
            let mut v = invalid_periods;
            v.sort();
            v
        },
        on_exhausted: budget.on_exhausted.clone(),
        on_exhausted_valid: VALID_ON_EXHAUSTED.contains(&budget.on_exhausted.as_str()),
    })
}

/// §72.f — generate effect-budgeted proofs: one per daemon that declares a
/// `budget { … }`. A daemon with no budget carries no contract → no proof
/// (mirrors "no effects → no effect-row proof").
pub fn generate_effect_budgeted_proofs(ir: &IRProgram, axon_version: &str) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut proofs = Vec::new();
    for daemon in &ir.daemons {
        if daemon.budget.is_none() {
            continue;
        }
        let Some(witness) = derive_effect_budgeted_witness(&daemon.name, ir) else {
            continue;
        };
        proofs.push(ProofTerm {
            property: PropertyClass::EffectBudgeted,
            artifact_digest: digest.clone(),
            witness: Witness::EffectBudgeted(witness),
            axon_version: axon_version.to_string(),
        });
    }
    proofs
}

// ── §Fase 73.g — JsonShapeSoundness ──────────────────────────────────

/// §73.g — derive a [`JsonShapeSoundnessWitness`] for one store's column
/// shape lenses against the program IR. Pure + total. Shared with the
/// checker's re-derivation path so producer + verifier compute identically
/// (D51.2). `None` when the store is absent OR declares no inline
/// `Json<T>` lens column — no contract → no proof. (A `manifest_ref` /
/// `env_var` schema form carries no INLINE lens shape to certify; only the
/// inline form's declared shapes are in the artifact.)
pub fn derive_json_shape_soundness_witness(
    store_name: &str,
    ir: &IRProgram,
) -> Option<JsonShapeSoundnessWitness> {
    let store = ir.axonstore_specs.iter().find(|s| s.name == store_name)?;
    let columns = match store.column_schema.as_ref()? {
        crate::ir_nodes::IRStoreColumnSchema::Inline { columns } => columns,
        _ => return None,
    };
    // The lens sites: columns carrying a `Json<T>` shape, sorted by
    // (column, shape) for a canonical witness.
    let mut lens: Vec<(String, String)> = columns
        .iter()
        .filter_map(|c| c.json_shape.as_ref().map(|s| (c.name.clone(), s.clone())))
        .collect();
    if lens.is_empty() {
        return None;
    }
    lens.sort();

    let declared_types =
        canonical_names(&ir.types.iter().map(|t| t.name.clone()).collect::<Vec<_>>());
    let declared: std::collections::HashSet<&str> =
        declared_types.iter().map(String::as_str).collect();

    let lens_columns: Vec<String> = lens.iter().map(|(c, s)| format!("{c}:{s}")).collect();
    // `lens` is already sorted, so the filtered subset stays sorted.
    let unresolved_shapes: Vec<String> = lens
        .iter()
        .filter(|(_, s)| !declared.contains(s.as_str()))
        .map(|(c, s)| format!("{c}:{s}"))
        .collect();

    Some(JsonShapeSoundnessWitness {
        store_name: store_name.to_string(),
        declared_types,
        lens_columns,
        unresolved_shapes,
    })
}

/// §73.g — generate JsonShapeSoundness proofs: one per `axonstore` that
/// declares ≥1 inline `Json<T>` lens column. A store with no shape lens
/// carries no contract → no proof.
pub fn generate_json_shape_soundness_proofs(ir: &IRProgram, axon_version: &str) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut proofs = Vec::new();
    for store in &ir.axonstore_specs {
        let Some(witness) = derive_json_shape_soundness_witness(&store.name, ir) else {
            continue;
        };
        proofs.push(ProofTerm {
            property: PropertyClass::JsonShapeSoundness,
            artifact_digest: digest.clone(),
            witness: Witness::JsonShapeSoundness(witness),
            axon_version: axon_version.to_string(),
        });
    }
    proofs
}

// ── §Fase 74.g — ChannelDeliverySoundness ────────────────────────────

/// §74.g — every channel `emit`ted to (a PRODUCER) anywhere in the program:
/// walk all flow bodies + every daemon listener body, recursing into nested
/// `if` / `for` bodies. Shared by producer + checker (D51.2).
fn collect_emitted_channels(ir: &IRProgram) -> std::collections::HashSet<String> {
    fn walk(steps: &[crate::ir_nodes::IRFlowNode], out: &mut std::collections::HashSet<String>) {
        use crate::ir_nodes::IRFlowNode as N;
        for step in steps {
            match step {
                N::Emit(e) => {
                    out.insert(e.channel_ref.clone());
                }
                N::Conditional(c) => {
                    walk(&c.then_body, out);
                    walk(&c.else_body, out);
                }
                N::ForIn(f) => walk(&f.body, out),
                // A daemon listener body can itself `emit` (a relay).
                N::Listen(l) => walk(&l.body, out),
                _ => {}
            }
        }
    }
    let mut out = std::collections::HashSet::new();
    for flow in &ir.flows {
        walk(&flow.steps, &mut out);
    }
    for daemon in &ir.daemons {
        for listener in &daemon.listeners {
            walk(&listener.body, &mut out);
        }
    }
    out
}

/// §74.g — every channel a `daemon` `listen`s on (a non-cron event
/// listener — the consumers). Cron listeners are timer-driven, not channel
/// consumers.
fn collect_listened_channels(ir: &IRProgram) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for daemon in &ir.daemons {
        for l in &daemon.listeners {
            if axon_frontend::cron::cron_expr(&l.channel).is_none() {
                out.insert(l.channel.clone());
            }
        }
    }
    out
}

/// §74.g — derive a [`ChannelDeliverySoundnessWitness`] for one channel.
/// Pure + total. `None` when the channel is absent OR has NO consumer (a
/// daemon `listen`er) — a channel with no listener carries no delivery
/// contract, so no proof is generated (mirrors "no effects → no
/// effect-row proof").
pub fn derive_channel_delivery_soundness_witness(
    channel_name: &str,
    ir: &IRProgram,
) -> Option<ChannelDeliverySoundnessWitness> {
    let ch = ir.channels.iter().find(|c| c.name == channel_name)?;
    let consumers = collect_listened_channels(ir);
    if !consumers.contains(channel_name) {
        return None;
    }
    let producers = collect_emitted_channels(ir);
    Some(ChannelDeliverySoundnessWitness {
        channel_name: channel_name.to_string(),
        persistence: ch.persistence.clone(),
        qos: ch.qos.clone(),
        has_producer: producers.contains(channel_name),
        has_consumer: true,
    })
}

/// §74.g — generate ChannelDeliverySoundness proofs: one per `channel`
/// that a daemon `listen`s on.
pub fn generate_channel_delivery_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut proofs = Vec::new();
    for ch in &ir.channels {
        let Some(witness) = derive_channel_delivery_soundness_witness(&ch.name, ir) else {
            continue;
        };
        proofs.push(ProofTerm {
            property: PropertyClass::ChannelDeliverySoundness,
            artifact_digest: digest.clone(),
            witness: Witness::ChannelDeliverySoundness(witness),
            axon_version: axon_version.to_string(),
        });
    }
    proofs
}

// ── §Fase 76.e — AggregateSoundness ──────────────────────────────────

/// §76.e — every aggregate-`retrieve` site in the program: walk all flow
/// bodies + every daemon listener body, recursing into nested `if` /
/// `for` bodies. A site is any retrieve with a non-empty `aggregate:` OR
/// `group_by:` (the latter alone is a T845 violation the witness records).
/// Shared by producer + checker (D51.2). Sorted + deduped so the
/// derivation is canonical.
pub fn derive_aggregate_soundness_witnesses(
    ir: &IRProgram,
) -> Vec<AggregateSoundnessWitness> {
    fn witness_for(
        flow_name: &str,
        s: &crate::ir_nodes::IRRetrieveStep,
    ) -> Option<AggregateSoundnessWitness> {
        if s.aggregate.trim().is_empty() && s.group_by.trim().is_empty() {
            return None;
        }
        let mut w = AggregateSoundnessWitness {
            flow_name: flow_name.to_string(),
            store_name: s.store_name.clone(),
            aggregate: s.aggregate.clone(),
            group_by: s.group_by.clone(),
            order_by: s.order_by.clone(),
            limit_expr: s.limit_expr.clone(),
            function: String::new(),
            column: String::new(),
            group_columns: Vec::new(),
            violations: Vec::new(),
        };
        // The SAME closed-catalog parser the engines run (D51.2 — the
        // checker re-derives through it too, so the proof certifies the
        // exact runtime semantics, not a reimplementation).
        match crate::store::filter::parse_aggregate_clause(
            &s.aggregate,
            &s.group_by,
            &s.order_by,
            &s.limit_expr,
        ) {
            Ok(Some((spec, groups))) => {
                w.function = spec.func.label().to_string();
                w.column = spec.column.unwrap_or_default();
                w.group_columns = groups;
            }
            // Unreachable for a selected site (empty-both is filtered
            // above) — kept total.
            Ok(None) => {}
            Err(e) => w.violations.push(e.to_string()),
        }
        Some(w)
    }
    fn walk(
        flow_name: &str,
        steps: &[crate::ir_nodes::IRFlowNode],
        out: &mut Vec<AggregateSoundnessWitness>,
    ) {
        use crate::ir_nodes::IRFlowNode as N;
        for step in steps {
            match step {
                N::Retrieve(s) => {
                    if let Some(w) = witness_for(flow_name, s) {
                        out.push(w);
                    }
                }
                N::Conditional(c) => {
                    walk(flow_name, &c.then_body, out);
                    walk(flow_name, &c.else_body, out);
                }
                N::ForIn(f) => walk(flow_name, &f.body, out),
                N::Listen(l) => walk(flow_name, &l.body, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for flow in &ir.flows {
        walk(&flow.name, &flow.steps, &mut out);
    }
    for daemon in &ir.daemons {
        let subject = format!("daemon:{}", daemon.name);
        for listener in &daemon.listeners {
            walk(&subject, &listener.body, &mut out);
        }
    }
    out.sort_by(|a, b| {
        (&a.flow_name, &a.store_name, &a.aggregate, &a.group_by)
            .cmp(&(&b.flow_name, &b.store_name, &b.aggregate, &b.group_by))
    });
    out.dedup();
    out
}

/// §76.e — generate AggregateSoundness proofs: one per aggregate-retrieve
/// site. A malformed site still generates its (refutable) proof — the
/// §52 deploy gate then rejects the bundle, fail-closed.
pub fn generate_aggregate_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    derive_aggregate_soundness_witnesses(ir)
        .into_iter()
        .map(|witness| ProofTerm {
            property: PropertyClass::AggregateSoundness,
            artifact_digest: digest.clone(),
            witness: Witness::AggregateSoundness(witness),
            axon_version: axon_version.to_string(),
        })
        .collect()
}

// ── §Fase 77.b — ChannelEgressSoundness ──────────────────────────────

/// §77.b — the first publish site (flows in declaration order, then daemon
/// listeners) targeting `channel_name` whose shield declares `sign:`.
/// Returns `(sign, shield_ref)`. The derivation NEVER trusts the IR's
/// pre-resolved `IRPublish.sign` stamp — it re-resolves the named shield
/// against the artifact's shield list (D51.2: a forged stamp is caught
/// because the checker recomputes). Mirrors the walk order of the
/// frontend's `mark_egress_channels` (first site wins) so producer and
/// lowering agree.
fn derive_first_signing_publish(
    channel_name: &str,
    ir: &IRProgram,
) -> Option<(String, String)> {
    fn walk(
        steps: &[crate::ir_nodes::IRFlowNode],
        channel_name: &str,
        ir: &IRProgram,
        found: &mut Option<(String, String)>,
    ) {
        use crate::ir_nodes::IRFlowNode as N;
        for step in steps {
            if found.is_some() {
                return;
            }
            match step {
                N::Publish(p) if p.channel_ref == channel_name => {
                    let sign = ir
                        .shields
                        .iter()
                        .find(|s| s.name == p.shield_ref)
                        .map(|s| s.sign.clone())
                        .unwrap_or_default();
                    if !sign.is_empty() {
                        *found = Some((sign, p.shield_ref.clone()));
                    }
                }
                N::Conditional(c) => {
                    walk(&c.then_body, channel_name, ir, found);
                    walk(&c.else_body, channel_name, ir, found);
                }
                N::ForIn(f) => walk(&f.body, channel_name, ir, found),
                N::Par(p) => {
                    for branch in &p.branches {
                        walk(branch, channel_name, ir, found);
                    }
                }
                N::Listen(l) => walk(&l.body, channel_name, ir, found),
                N::Quant(q) => walk(&q.body, channel_name, ir, found),
                _ => {}
            }
        }
    }
    let mut found = None;
    for flow in &ir.flows {
        walk(&flow.steps, channel_name, ir, &mut found);
        if found.is_some() {
            return found;
        }
    }
    for daemon in &ir.daemons {
        for listener in &daemon.listeners {
            walk(&listener.body, channel_name, ir, &mut found);
            if found.is_some() {
                return found;
            }
        }
    }
    found
}

/// §77.b — derive a [`ChannelEgressSoundnessWitness`] for one channel.
/// Pure + total. `None` when the channel carries NO egress contract:
/// neither a declared `egress_sign` on its handle nor a derivable signing
/// publish site (mirrors "no effects → no effect-row proof").
pub fn derive_channel_egress_witness(
    channel_name: &str,
    ir: &IRProgram,
) -> Option<ChannelEgressSoundnessWitness> {
    let ch = ir.channels.iter().find(|c| c.name == channel_name)?;
    let (derived_sign, shield_ref) =
        derive_first_signing_publish(channel_name, ir).unwrap_or_default();
    if ch.egress_sign.is_empty() && derived_sign.is_empty() {
        return None;
    }
    Some(ChannelEgressSoundnessWitness {
        channel_name: channel_name.to_string(),
        declared_egress_sign: ch.egress_sign.clone(),
        derived_sign,
        shield_ref,
        persistence: ch.persistence.clone(),
        durable: ch.persistence == "persistent_axonstore",
    })
}

/// §77.b — generate ChannelEgressSoundness proofs: one per channel with an
/// egress contract (declared or derivable). An unsound site still generates
/// its (refutable) proof — the §52 deploy gate then rejects the bundle,
/// fail-closed.
pub fn generate_channel_egress_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut proofs = Vec::new();
    for ch in &ir.channels {
        let Some(witness) = derive_channel_egress_witness(&ch.name, ir) else {
            continue;
        };
        proofs.push(ProofTerm {
            property: PropertyClass::ChannelEgressSoundness,
            artifact_digest: digest.clone(),
            witness: Witness::ChannelEgressSoundness(witness),
            axon_version: axon_version.to_string(),
        });
    }
    proofs
}

// ── §Fase 79.c — InterruptibleSessionSoundness ───────────────────────────────

/// §79.c — does an IR handler step-sequence reach a two-exit terminal (`resume`
/// or `end`) on every path (D79.11a)? The checker's own re-derivation of the
/// §79.c type-checker's `handler_reaches_exit` (D51.2 — PCC states the spec
/// independently). A terminal choice is well-formed iff every arm reaches one.
fn ir_handler_reaches_exit(steps: &[IRSessionStep]) -> bool {
    match steps.last() {
        Some(s) if s.op == "resume" || s.op == "end" => true,
        Some(s) if s.op == "select" || s.op == "branch" => {
            !s.branches.is_empty() && s.branches.iter().all(|b| ir_handler_reaches_exit(&b.steps))
        }
        _ => false,
    }
}

/// §79.c — the first `interrupt` step (by pre-order walk) in `steps` whose
/// `on <Signal>` matches `signal`. Descends into nested arms (v1 forbids nested
/// interrupts, but the walk is total either way).
fn find_interrupt_step<'a>(steps: &'a [IRSessionStep], signal: &str) -> Option<&'a IRSessionStep> {
    for s in steps {
        if s.op == "interrupt" && s.message_type == signal {
            return Some(s);
        }
        for b in &s.branches {
            if let Some(found) = find_interrupt_step(&b.steps, signal) {
                return Some(found);
            }
        }
    }
    None
}

/// §79.c — every distinct interrupt signal declared in `steps` (canonical:
/// sorted + deduped), so one region ↦ one proof even if the same cause recurs.
fn interrupt_signals(steps: &[IRSessionStep]) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(steps: &[IRSessionStep], out: &mut Vec<String>) {
        for s in steps {
            if s.op == "interrupt" {
                out.push(s.message_type.clone());
            }
            for b in &s.branches {
                walk(&b.steps, out);
            }
        }
    }
    walk(steps, &mut out);
    out.sort();
    out.dedup();
    out
}

/// §79.c — re-derive the InterruptibleSessionSoundness witness for the interrupt
/// region located by `(session, role, signal)`. `None` if no such region exists
/// in the artifact (a forged / stale proof then refutes).
pub fn derive_interruptible_session_witness(
    session_name: &str,
    role_name: &str,
    signal: &str,
    ir: &IRProgram,
) -> Option<InterruptibleSessionSoundnessWitness> {
    let session = ir.sessions.iter().find(|s| s.name == session_name)?;
    let role = session.roles.iter().find(|r| r.name == role_name)?;
    let step = find_interrupt_step(&role.steps, signal)?;
    let has_body = step.branches.iter().any(|b| b.label == "body");
    let handler = step.branches.iter().find(|b| b.label == "handler");
    Some(InterruptibleSessionSoundnessWitness {
        session_name: session_name.to_string(),
        role_name: role_name.to_string(),
        signal: step.message_type.clone(),
        signal_in_catalog: CALL_INTERRUPT_CAUSES.contains(&step.message_type.as_str()),
        has_body,
        has_handler: handler.is_some(),
        handler_reaches_exit: handler
            .map(|h| ir_handler_reaches_exit(&h.steps))
            .unwrap_or(false),
    })
}

/// §79.c — generate InterruptibleSessionSoundness proofs: one per interrupt
/// region declared in any session role. An unsound region (bogus signal,
/// missing arm, handler with no exit) still gets its refutable proof — the §52
/// deploy gate then rejects the bundle fail-closed.
pub fn generate_interruptible_session_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut proofs = Vec::new();
    for session in &ir.sessions {
        for role in &session.roles {
            for signal in interrupt_signals(&role.steps) {
                let Some(witness) =
                    derive_interruptible_session_witness(&session.name, &role.name, &signal, ir)
                else {
                    continue;
                };
                proofs.push(ProofTerm {
                    property: PropertyClass::InterruptibleSessionSoundness,
                    artifact_digest: digest.clone(),
                    witness: Witness::InterruptibleSessionSoundness(witness),
                    axon_version: axon_version.to_string(),
                });
            }
        }
    }
    proofs
}

// ── §Fase 79.f — ParkedResidualSoundness + CallSoundnessCertificate ──────────

/// §79.f — re-derive the ParkedResidualSoundness witness for `socket_name`.
/// `None` if the socket does not carry an interruptible session (no residual is
/// ever parked at rest → no obligation → no proof).
pub fn derive_parked_residual_witness(
    socket_name: &str,
    ir: &IRProgram,
) -> Option<ParkedResidualSoundnessWitness> {
    let socket = ir.sockets.iter().find(|s| s.name == socket_name)?;
    let session = ir.sessions.iter().find(|s| s.name == socket.protocol);
    let session_has_interrupt = session
        .map(|s| {
            s.roles
                .iter()
                .any(|r| !interrupt_signals(&r.steps).is_empty())
        })
        .unwrap_or(false);
    if !session_has_interrupt {
        return None;
    }
    Some(ParkedResidualSoundnessWitness {
        socket_name: socket_name.to_string(),
        session_name: socket.protocol.clone(),
        session_has_interrupt,
        reconnect_cognitive_state: socket.reconnect,
        legal_basis_declared: socket
            .legal_basis
            .as_ref()
            .map(|b| !b.is_empty())
            .unwrap_or(false),
    })
}

/// §79.f — one ParkedResidualSoundness proof per socket carrying an
/// interruptible session. A socket that parks a residual without `reconnect:
/// cognitive_state` or without a `legal_basis` still gets its refutable proof.
pub fn generate_parked_residual_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut proofs = Vec::new();
    for socket in &ir.sockets {
        let Some(witness) = derive_parked_residual_witness(&socket.name, ir) else {
            continue;
        };
        proofs.push(ProofTerm {
            property: PropertyClass::ParkedResidualSoundness,
            artifact_digest: digest.clone(),
            witness: Witness::ParkedResidualSoundness(witness),
            axon_version: axon_version.to_string(),
        });
    }
    proofs
}

/// §80 — collect the distinct message types a compiled session role
/// sends/receives, walking select/branch arms and §79 interrupt body+handler
/// (a message only exchanged inside a handler still crosses the wire).
/// Order-preserving first-occurrence dedup — the checker compares these
/// vectors verbatim, so derivation order must be deterministic.
fn collect_ir_role_messages(
    steps: &[axon_frontend::ir_nodes::IRSessionStep],
    sends: &mut Vec<String>,
    receives: &mut Vec<String>,
) {
    for step in steps {
        match step.op.as_str() {
            "send" => {
                if !sends.contains(&step.message_type) {
                    sends.push(step.message_type.clone());
                }
            }
            "receive" => {
                if !receives.contains(&step.message_type) {
                    receives.push(step.message_type.clone());
                }
            }
            "select" | "branch" | "interrupt" => {
                for b in &step.branches {
                    collect_ir_role_messages(&b.steps, sends, receives);
                }
            }
            _ => {}
        }
    }
}

/// §80 — the T850 charset law, re-derived: a config key is lowercase
/// `[a-z0-9][a-z0-9_.-]*` (the compile-time mirror of the enterprise
/// `SecretKeyPolicy`). A URL (`:`/`/`) or a typical credential literal
/// (uppercase) cannot satisfy it.
fn is_policy_shaped_config_key(key: &str) -> bool {
    let mut chars = key.chars();
    let head_ok = chars.next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    head_ok && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'))
}

/// §80 — derive the [`UpstreamProjectionSoundnessWitness`] for one upstream
/// from the IR alone (pure; shared by the generator and, by re-derivation,
/// the checker — D51.2). `None` if the upstream does not exist.
pub fn derive_upstream_projection_witness(
    upstream_name: &str,
    ir: &IRProgram,
) -> Option<UpstreamProjectionSoundnessWitness> {
    let up = ir.upstreams.iter().find(|u| u.name == upstream_name)?;

    // Required sets — from the bound role (empty if the binding is broken;
    // a broken binding also fails totality below, so the proof refutes).
    let mut required_sends = Vec::new();
    let mut required_receives = Vec::new();
    let bound_role = ir
        .sessions
        .iter()
        .find(|s| s.name == up.protocol)
        .and_then(|s| s.roles.iter().find(|r| r.name == up.role));
    if let Some(role) = bound_role {
        collect_ir_role_messages(&role.steps, &mut required_sends, &mut required_receives);
    }

    // Covered sets — from the map rules (dedup, source order).
    let mut covered_sends = Vec::new();
    let mut covered_receives = Vec::new();
    for r in &up.map {
        let bucket = if r.direction == "send" { &mut covered_sends } else { &mut covered_receives };
        if !bucket.contains(&r.message) {
            bucket.push(r.message.clone());
        }
    }

    // Totality + unambiguity — the T849 law re-derived from the artifact.
    let mut total = bound_role.is_some();
    for (dir, required) in [("send", &required_sends), ("receive", &required_receives)] {
        for msg in required.iter() {
            let n = up.map.iter().filter(|r| r.direction == dir && &r.message == msg).count();
            if n != 1 {
                total = false;
            }
        }
    }
    for r in &up.map {
        let known = if r.direction == "send" {
            required_sends.contains(&r.message)
        } else {
            required_receives.contains(&r.message)
        };
        if !known {
            total = false;
        }
    }
    // Discriminator keys mirror the §80.c checker exactly: no `when` ⇒
    // equality ("type", Some(<MessageName>)); `when "f" = "v"` ⇒ equality
    // (f, Some(v)); `when "f"` ⇒ PRESENCE (f, None). Equality dispatches
    // before presence at runtime, so only identical keys are ambiguous.
    let receive_json: Vec<(String, Option<String>)> = up
        .map
        .iter()
        .filter(|r| r.direction == "receive" && r.framing == "json")
        .map(|r| match (&r.when_field, &r.when_value) {
            (None, _) => ("type".to_string(), Some(r.message.clone())),
            (Some(f), Some(v)) => (f.clone(), Some(v.clone())),
            (Some(f), None) => (f.clone(), None),
        })
        .collect();
    for (i, a) in receive_json.iter().enumerate() {
        if receive_json.iter().skip(i + 1).any(|b| a == b) {
            total = false;
        }
    }
    if up.map.iter().filter(|r| r.direction == "receive" && r.framing == "binary").count() > 1 {
        total = false;
    }

    Some(UpstreamProjectionSoundnessWitness {
        upstream_name: up.name.clone(),
        session_name: up.protocol.clone(),
        role_name: up.role.clone(),
        required_sends,
        required_receives,
        covered_sends,
        covered_receives,
        projection_total: total,
        config_keys_valid: is_policy_shaped_config_key(&up.resolve) && is_policy_shaped_config_key(&up.secret),
    })
}

/// §80 — one UpstreamProjectionSoundness proof per `upstream`. An upstream
/// with a partial projection or literal-shaped config still gets its
/// refutable proof — the checker refutes it, the deploy gate blocks it.
pub fn generate_upstream_projection_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut proofs = Vec::new();
    for up in &ir.upstreams {
        let Some(witness) = derive_upstream_projection_witness(&up.name, ir) else {
            continue;
        };
        proofs.push(ProofTerm {
            property: PropertyClass::UpstreamProjectionSoundness,
            artifact_digest: digest.clone(),
            witness: Witness::UpstreamProjectionSoundness(witness),
            axon_version: axon_version.to_string(),
        });
    }
    proofs
}

/// §79.f — the unified **`CallSoundnessCertificate`**: for one socket bundle,
/// compose the interruptible-session soundness of its session, the parked-
/// residual soundness of the socket, and the socket's resource (credit) bound
/// into ONE deploy-time artifact — the "can this call ever misbehave" question
/// asked once, before a single call happens. Composition is NOT a mere
/// conjunction (D79.8): `ParkedResidualSoundness` is the genuinely-new member
/// interruption introduces (the data-at-rest surface, paper §7).
///
/// The enterprise serving layer (§79.f ENT) extends the bundle with the
/// flow-scoped shield + budget proofs it can resolve; this OSS core composes
/// the socket-derivable members. `None` if the socket does not exist.
pub fn generate_call_soundness_certificate(
    socket_name: &str,
    ir: &IRProgram,
    axon_version: &str,
) -> Option<CallSoundnessCertificate> {
    let socket = ir.sockets.iter().find(|s| s.name == socket_name)?;
    let session_name = socket.protocol.clone();
    let mut proofs = Vec::new();

    // (1) InterruptibleSessionSoundness — the socket's session.
    proofs.extend(
        generate_interruptible_session_soundness_proofs(ir, axon_version)
            .into_iter()
            .filter(|p| {
                matches!(&p.witness,
                    Witness::InterruptibleSessionSoundness(w) if w.session_name == session_name)
            }),
    );
    // (2) ParkedResidualSoundness — the socket's data-at-rest surface (new).
    proofs.extend(
        generate_parked_residual_soundness_proofs(ir, axon_version)
            .into_iter()
            .filter(|p| {
                matches!(&p.witness,
                    Witness::ParkedResidualSoundness(w) if w.socket_name == socket_name)
            }),
    );
    // (3) ResourceBounds — the socket's credit-window bound.
    proofs.extend(
        generate_resource_bounds_proofs(ir, axon_version)
            .into_iter()
            .filter(|p| p.witness.subject_name() == socket_name),
    );

    Some(CallSoundnessCertificate {
        socket_name: socket_name.to_string(),
        session_name,
        artifact_digest: artifact_digest(ir),
        axon_version: axon_version.to_string(),
        proofs,
    })
}

// ── §Fase 83.c — CorsPolicyConsistency ───────────────────────────────────────

/// §83.c — re-derive the whole-program CORS witness from `ir.cors_policies`
/// + `ir.endpoints`. "No contract → no proof" (the standing convention): a
/// program with no `cors` declarations AND no `cors_ref` anywhere returns
/// `None` — there is nothing to certify.
pub fn derive_cors_policy_consistency_witness(ir: &IRProgram) -> Option<CorsPolicyConsistencyWitness> {
    if ir.cors_policies.is_empty() && ir.endpoints.iter().all(|e| e.cors_ref.is_empty()) {
        return None;
    }
    let declared_cors_names: Vec<String> =
        ir.cors_policies.iter().map(|c| c.name.clone()).collect();
    let endpoint_cors_refs: Vec<(String, String)> = ir
        .endpoints
        .iter()
        .filter(|e| !e.cors_ref.is_empty())
        .map(|e| (e.name.clone(), e.cors_ref.clone()))
        .collect();
    let all_references_resolve = endpoint_cors_refs
        .iter()
        .all(|(_, r)| declared_cors_names.contains(r));
    let wildcard_credential_violations: Vec<String> = ir
        .cors_policies
        .iter()
        .filter(|c| c.allow_credentials && c.allow_origins.iter().any(|o| o == "*"))
        .map(|c| c.name.clone())
        .collect();
    // Cross-method path consistency: same shape as the §83.c type-checker
    // pass (axon-T857), re-derived from the compiled IR instead of the AST.
    let mut by_path: std::collections::HashMap<&str, (&str, &str)> = std::collections::HashMap::new();
    let mut cross_method_conflicts: Vec<(String, String)> = Vec::new();
    for ep in &ir.endpoints {
        if ep.path.is_empty() {
            continue;
        }
        match by_path.get(ep.path.as_str()) {
            None => {
                by_path.insert(ep.path.as_str(), (ep.name.as_str(), ep.cors_ref.as_str()));
            }
            Some((first_name, first_ref)) => {
                if *first_ref != ep.cors_ref {
                    cross_method_conflicts.push((first_name.to_string(), ep.name.clone()));
                }
            }
        }
    }
    Some(CorsPolicyConsistencyWitness {
        declared_cors_names,
        endpoint_cors_refs,
        all_references_resolve,
        wildcard_credential_violations,
        cross_method_conflicts,
    })
}

/// §83.c — generate the (at most one) CorsPolicyConsistency proof for `ir`.
/// Program-wide, so this is a Vec of length 0 or 1, unlike most generators.
pub fn generate_cors_policy_consistency_proofs(ir: &IRProgram, axon_version: &str) -> Vec<ProofTerm> {
    match derive_cors_policy_consistency_witness(ir) {
        Some(witness) => vec![ProofTerm {
            property: PropertyClass::CorsPolicyConsistency,
            artifact_digest: artifact_digest(ir),
            witness: Witness::CorsPolicyConsistency(witness),
            axon_version: axon_version.to_string(),
        }],
        None => Vec::new(),
    }
}

// ── §Fase 91.c — TemporalContextSoundness ────────────────────────────────────

/// §91.c — recursively collect every step-level `now:` declaration from a
/// flow-IR body, recursing through the container variants that carry nested
/// bodies (Conditional / ForIn / Par / Listen / Warden / Quant).
fn collect_step_now_declarations(
    nodes: &[axon_frontend::ir_nodes::IRFlowNode],
    out: &mut Vec<(String, String, String)>,
) {
    use axon_frontend::ir_nodes::IRFlowNode;
    for node in nodes {
        match node {
            IRFlowNode::Step(s) => {
                if let Some(tz) = &s.now_tz {
                    out.push(("step".to_string(), s.name.clone(), tz.clone()));
                }
            }
            IRFlowNode::Conditional(c) => {
                collect_step_now_declarations(&c.then_body, out);
                collect_step_now_declarations(&c.else_body, out);
            }
            IRFlowNode::ForIn(f) => collect_step_now_declarations(&f.body, out),
            IRFlowNode::Par(p) => {
                for branch in &p.branches {
                    collect_step_now_declarations(branch, out);
                }
            }
            IRFlowNode::Listen(l) => collect_step_now_declarations(&l.body, out),
            IRFlowNode::Warden(w) => collect_step_now_declarations(&w.body, out),
            IRFlowNode::Quant(q) => collect_step_now_declarations(&q.body, out),
            _ => {}
        }
    }
}

/// §91.c — the IANA SHAPE law (`axon-T892`, the §91.a frontend check
/// re-stated by the checker per D51.2): `"UTC"`, or `Area/Location` with no
/// leading/trailing slash.
fn now_zone_shape_ok(zone: &str) -> bool {
    let t = zone.trim();
    t == "UTC" || (t.contains('/') && !t.starts_with('/') && !t.ends_with('/'))
}

/// §91.c — re-derive the whole-program temporal-context witness. `None` when
/// the program declares no `now:` anywhere — no temporal contract → no proof
/// (the standing convention).
pub fn derive_temporal_context_soundness_witness(
    ir: &IRProgram,
) -> Option<super::proof_term::TemporalContextSoundnessWitness> {
    let mut declarations: Vec<(String, String, String)> = Vec::new();
    for c in &ir.contexts {
        if let Some(tz) = &c.now_tz {
            declarations.push(("context".to_string(), c.name.clone(), tz.clone()));
        }
    }
    for flow in &ir.flows {
        collect_step_now_declarations(&flow.steps, &mut declarations);
    }
    if declarations.is_empty() {
        return None;
    }
    let mut format_violations: Vec<String> = Vec::new();
    let mut unknown_zones: Vec<String> = Vec::new();
    for (_, _, zone) in &declarations {
        if !now_zone_shape_ok(zone) {
            if !format_violations.contains(zone) {
                format_violations.push(zone.clone());
            }
        } else if crate::window::parse_tz(zone).is_none() {
            // Shape-valid but not in this build's tz database — the failure
            // the zero-dep frontend CANNOT catch (§71.a split); §91.b fails
            // closed at runtime, this proof catches it at verify/deploy.
            if !unknown_zones.contains(zone) {
                unknown_zones.push(zone.clone());
            }
        }
    }
    Some(super::proof_term::TemporalContextSoundnessWitness {
        declarations,
        format_violations,
        unknown_zones,
    })
}

/// §91.c — generate the (at most one) TemporalContextSoundness proof for
/// `ir`. Program-wide, so this is a Vec of length 0 or 1.
pub fn generate_temporal_context_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    match derive_temporal_context_soundness_witness(ir) {
        Some(witness) => vec![ProofTerm {
            property: PropertyClass::TemporalContextSoundness,
            artifact_digest: artifact_digest(ir),
            witness: Witness::TemporalContextSoundness(witness),
            axon_version: axon_version.to_string(),
        }],
        None => Vec::new(),
    }
}

// ── §Fase 92.d — CredentialAttenuation ───────────────────────────────────────

/// §92.d — recursively collect every `mint` site from a flow-IR body
/// (recursing through the container variants, the §91.c walk shape).
fn collect_mint_sites(
    flow_name: &str,
    nodes: &[axon_frontend::ir_nodes::IRFlowNode],
    out: &mut Vec<(String, String, String)>,
) {
    use axon_frontend::ir_nodes::IRFlowNode;
    for node in nodes {
        match node {
            IRFlowNode::Mint(m) => out.push((
                flow_name.to_string(),
                m.credential_ref.clone(),
                m.binding.clone(),
            )),
            IRFlowNode::Conditional(c) => {
                collect_mint_sites(flow_name, &c.then_body, out);
                collect_mint_sites(flow_name, &c.else_body, out);
            }
            IRFlowNode::ForIn(f) => collect_mint_sites(flow_name, &f.body, out),
            IRFlowNode::Par(p) => {
                for branch in &p.branches {
                    collect_mint_sites(flow_name, branch, out);
                }
            }
            IRFlowNode::Listen(l) => collect_mint_sites(flow_name, &l.body, out),
            IRFlowNode::Warden(w) => collect_mint_sites(flow_name, &w.body, out),
            IRFlowNode::Quant(q) => collect_mint_sites(flow_name, &q.body, out),
            _ => {}
        }
    }
}

/// §92.d — the compile-time contract laws, re-stated by the checker
/// (D51.2): non-empty valid-slug grants (`axon-T893`) + TTL ∈ (0, 24h]
/// seconds (`axon-T894`). Mirror of the frontend's
/// `is_valid_capability_slug` shape law.
fn credential_contract_ok(c: &axon_frontend::ir_nodes::IRCredential) -> bool {
    let slug_ok = |s: &str| {
        !s.is_empty()
            && s.split('.').all(|seg| {
                let mut ch = seg.chars();
                matches!(ch.next(), Some('a'..='z'))
                    && ch.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            })
    };
    !c.grants.is_empty()
        && c.grants.iter().all(|g| slug_ok(g))
        && c.ttl_secs > 0
        && c.ttl_secs <= 86_400
}

/// §92.d — re-derive the whole-program credential-attenuation witness.
/// `None` when the program declares no `credential` and no `mint` — no
/// contract → no proof (the standing convention).
pub fn derive_credential_attenuation_witness(
    ir: &IRProgram,
) -> Option<super::proof_term::CredentialAttenuationWitness> {
    let contracts: Vec<(String, u64, Vec<String>)> = ir
        .credentials
        .iter()
        .map(|c| (c.name.clone(), c.ttl_secs, c.grants.clone()))
        .collect();
    let mut mints: Vec<(String, String, String)> = Vec::new();
    for flow in &ir.flows {
        collect_mint_sites(&flow.name, &flow.steps, &mut mints);
    }
    if contracts.is_empty() && mints.is_empty() {
        return None;
    }
    let declared: std::collections::HashSet<&str> =
        ir.credentials.iter().map(|c| c.name.as_str()).collect();
    let mut unresolved_mints: Vec<String> = Vec::new();
    for (_, cred_ref, _) in &mints {
        if !declared.contains(cred_ref.as_str()) && !unresolved_mints.contains(cred_ref) {
            unresolved_mints.push(cred_ref.clone());
        }
    }
    let invalid_contracts: Vec<String> = ir
        .credentials
        .iter()
        .filter(|c| !credential_contract_ok(c))
        .map(|c| c.name.clone())
        .collect();
    Some(super::proof_term::CredentialAttenuationWitness {
        contracts,
        mints,
        unresolved_mints,
        invalid_contracts,
    })
}

/// §92.d — generate the (at most one) CredentialAttenuation proof for
/// `ir`. Program-wide, so this is a Vec of length 0 or 1.
pub fn generate_credential_attenuation_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    match derive_credential_attenuation_witness(ir) {
        Some(witness) => vec![ProofTerm {
            property: PropertyClass::CredentialAttenuation,
            artifact_digest: artifact_digest(ir),
            witness: Witness::CredentialAttenuation(witness),
            axon_version: axon_version.to_string(),
        }],
        None => Vec::new(),
    }
}

// ── §Fase 94.e — SecretCustodySoundness ──────────────────────────────────────

/// §94.e — recursively collect every `rotate` site from a flow-IR body
/// (the §92.d walk shape).
fn collect_rotate_sites(
    flow_name: &str,
    nodes: &[axon_frontend::ir_nodes::IRFlowNode],
    out: &mut Vec<(String, String, String, String)>,
) {
    use axon_frontend::ir_nodes::IRFlowNode;
    for node in nodes {
        match node {
            IRFlowNode::Rotate(r) => out.push((
                flow_name.to_string(),
                r.store_ref.clone(),
                r.tool_ref.clone(),
                r.binding.clone(),
            )),
            IRFlowNode::Conditional(c) => {
                collect_rotate_sites(flow_name, &c.then_body, out);
                collect_rotate_sites(flow_name, &c.else_body, out);
            }
            IRFlowNode::ForIn(f) => collect_rotate_sites(flow_name, &f.body, out),
            IRFlowNode::Par(p) => {
                for branch in &p.branches {
                    collect_rotate_sites(flow_name, branch, out);
                }
            }
            IRFlowNode::Listen(l) => collect_rotate_sites(flow_name, &l.body, out),
            IRFlowNode::Warden(w) => collect_rotate_sites(flow_name, &w.body, out),
            IRFlowNode::Quant(q) => collect_rotate_sites(flow_name, &q.body, out),
            _ => {}
        }
    }
}

/// §94.e — recursively collect every WRITE verb against a store in
/// `secrets_stores` (`axon-T897`, re-derived).
fn collect_secrets_write_violations(
    flow_name: &str,
    nodes: &[axon_frontend::ir_nodes::IRFlowNode],
    secrets_stores: &std::collections::HashSet<&str>,
    out: &mut Vec<(String, String, String)>,
) {
    use axon_frontend::ir_nodes::IRFlowNode;
    let push = |verb: &str, store: &str, out: &mut Vec<(String, String, String)>| {
        if secrets_stores.contains(store) {
            out.push((flow_name.to_string(), verb.to_string(), store.to_string()));
        }
    };
    for node in nodes {
        match node {
            IRFlowNode::Persist(s) => push("persist", &s.store_name, out),
            IRFlowNode::Mutate(s) => push("mutate", &s.store_name, out),
            IRFlowNode::Purge(s) => push("purge", &s.store_name, out),
            IRFlowNode::Conditional(c) => {
                collect_secrets_write_violations(flow_name, &c.then_body, secrets_stores, out);
                collect_secrets_write_violations(flow_name, &c.else_body, secrets_stores, out);
            }
            IRFlowNode::ForIn(f) => {
                collect_secrets_write_violations(flow_name, &f.body, secrets_stores, out)
            }
            IRFlowNode::Par(p) => {
                for branch in &p.branches {
                    collect_secrets_write_violations(flow_name, branch, secrets_stores, out);
                }
            }
            IRFlowNode::Listen(l) => {
                collect_secrets_write_violations(flow_name, &l.body, secrets_stores, out)
            }
            IRFlowNode::Warden(w) => {
                collect_secrets_write_violations(flow_name, &w.body, secrets_stores, out)
            }
            IRFlowNode::Quant(q) => {
                collect_secrets_write_violations(flow_name, &q.body, secrets_stores, out)
            }
            _ => {}
        }
    }
}

/// §94.e — the `class:` shape law, re-stated by the checker (D51.2):
/// non-empty dotted lowercase slug (`axon-T900`'s charset, the §92.d
/// `slug_ok` mirror).
fn secrets_class_ok(class: &str) -> bool {
    !class.is_empty()
        && class.split('.').all(|seg| {
            let mut ch = seg.chars();
            matches!(ch.next(), Some('a'..='z'))
                && ch.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
}

/// §94.e — re-derive the whole-program secret-custody witness. `None`
/// when the program declares no `backend: secrets` store and no `rotate`
/// — no contract → no proof (the standing convention).
pub fn derive_secret_custody_witness(
    ir: &IRProgram,
) -> Option<super::proof_term::SecretCustodySoundnessWitness> {
    let stores: Vec<(String, String)> = ir
        .axonstore_specs
        .iter()
        .filter(|s| s.backend == "secrets")
        .map(|s| (s.name.clone(), s.class.clone()))
        .collect();
    let mut rotates: Vec<(String, String, String, String)> = Vec::new();
    for flow in &ir.flows {
        collect_rotate_sites(&flow.name, &flow.steps, &mut rotates);
    }
    if stores.is_empty() && rotates.is_empty() {
        return None;
    }

    let secrets_names: std::collections::HashSet<&str> =
        stores.iter().map(|(n, _)| n.as_str()).collect();
    let declared_tools: std::collections::HashSet<&str> =
        ir.tools.iter().map(|t| t.name.as_str()).collect();

    let mut unresolved_stores: Vec<String> = Vec::new();
    let mut unresolved_tools: Vec<String> = Vec::new();
    for (_, store_ref, tool_ref, _) in &rotates {
        if !secrets_names.contains(store_ref.as_str()) && !unresolved_stores.contains(store_ref)
        {
            unresolved_stores.push(store_ref.clone());
        }
        if !declared_tools.contains(tool_ref.as_str()) && !unresolved_tools.contains(tool_ref) {
            unresolved_tools.push(tool_ref.clone());
        }
    }
    let invalid_classes: Vec<String> = stores
        .iter()
        .filter(|(_, class)| !secrets_class_ok(class))
        .map(|(n, _)| n.clone())
        .collect();
    let mut write_violations: Vec<(String, String, String)> = Vec::new();
    for flow in &ir.flows {
        collect_secrets_write_violations(
            &flow.name,
            &flow.steps,
            &secrets_names,
            &mut write_violations,
        );
    }
    // §Fase 95.a — re-derive the `secret_partition:` laws (`axon-T903`)
    // from the IR's tool specs. The `selection_without_revelation`
    // containment guarantee holds ONLY if every partitioned tool's
    // discriminator is a required `String` parameter it declares — this
    // is the deploy-gate's independent check of that (a hand-edited IR
    // that points a partition at a ghost/non-string/absent parameter, or
    // sets one with no `secret:`, or on a technician tool, is REFUTED).
    let partition_violations: Vec<String> = ir
        .tools
        .iter()
        .filter(|t| !t.secret_partition.is_empty())
        .filter(|t| {
            let no_secret = t.secret.is_empty();
            let technician = t.target.is_some();
            let param_ok = t
                .parameters
                .iter()
                .any(|p| p.name == t.secret_partition && p.type_name == "String" && !p.optional);
            no_secret || technician || !param_ok
        })
        .map(|t| t.name.clone())
        .collect();
    Some(super::proof_term::SecretCustodySoundnessWitness {
        stores,
        rotates,
        unresolved_stores,
        unresolved_tools,
        invalid_classes,
        write_violations,
        partition_violations,
    })
}

/// §94.e — generate the (at most one) SecretCustodySoundness proof for
/// `ir`. Program-wide, so this is a Vec of length 0 or 1.
pub fn generate_secret_custody_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    match derive_secret_custody_witness(ir) {
        Some(witness) => vec![ProofTerm {
            property: PropertyClass::SecretCustodySoundness,
            artifact_digest: artifact_digest(ir),
            witness: Witness::SecretCustodySoundness(witness),
            axon_version: axon_version.to_string(),
        }],
        None => Vec::new(),
    }
}

// ── §Fase 84.c — TechnicianCommandSafety ─────────────────────────────────────

/// §84.c — does this IR session-step sequence contain a reachable
/// `branch{ approved / denied }`? Mirror of the frontend
/// `type_checker::session_has_confirm_branch`, re-derived on the compiled IR.
fn ir_session_has_confirm_branch(steps: &[axon_frontend::ir_nodes::IRSessionStep]) -> bool {
    for step in steps {
        if step.op == "branch" {
            let has_approved = step
                .branches
                .iter()
                .any(|b| b.label == axon_frontend::technician::CONFIRM_APPROVED_LABEL);
            let has_denied = step
                .branches
                .iter()
                .any(|b| b.label == axon_frontend::technician::CONFIRM_DENIED_LABEL);
            if has_approved && has_denied {
                return true;
            }
        }
        for b in &step.branches {
            if ir_session_has_confirm_branch(&b.steps) {
                return true;
            }
        }
    }
    false
}

/// §84.c — re-derive the technician-safety witness for one `target:`-bound
/// tool. `None` for a tool that is not a technician tool (no `target:`) — "no
/// contract → no proof" (the standing convention).
pub fn derive_technician_command_safety_witness(
    tool: &axon_frontend::ir_nodes::IRToolSpec,
    ir: &IRProgram,
) -> Option<TechnicianCommandSafetyWitness> {
    let target_socket = tool.target.clone()?;
    let session_name = ir
        .sockets
        .iter()
        .find(|s| s.name == target_socket)
        .map(|s| s.protocol.clone())
        .unwrap_or_default();
    let risk = tool.risk.clone().unwrap_or_default();

    let param_names: std::collections::HashSet<&str> =
        tool.parameters.iter().map(|p| p.name.as_str()).collect();
    let mut unbound_placeholders = Vec::new();
    let mut partial_tokens = Vec::new();
    for tok in &tool.argv {
        match axon_frontend::technician::classify_argv_token(tok) {
            axon_frontend::technician::ArgvToken::Placeholder(name) => {
                if !param_names.contains(name.as_str()) {
                    unbound_placeholders.push(name);
                }
            }
            axon_frontend::technician::ArgvToken::Partial(t) => partial_tokens.push(t),
            axon_frontend::technician::ArgvToken::Literal(_) => {}
        }
    }

    // T858 is scoped to `provider: bash`; a non-bash target tool is not
    // required to carry an argv template, so `argv_present` is vacuously true.
    let argv_present = tool.provider != "bash" || !tool.argv.is_empty();

    let confirm_branch_reachable =
        if risk == axon_frontend::technician::RISK_DESTRUCTIVE {
            ir.sessions
                .iter()
                .find(|s| s.name == session_name)
                .map(|s| s.roles.iter().any(|r| ir_session_has_confirm_branch(&r.steps)))
                .unwrap_or(false)
        } else {
            true
        };

    Some(TechnicianCommandSafetyWitness {
        tool_name: tool.name.clone(),
        target_socket,
        session_name,
        risk,
        argv: tool.argv.clone(),
        argv_present,
        unbound_placeholders,
        partial_tokens,
        confirm_branch_reachable,
    })
}

/// §84.c — one TechnicianCommandSafety proof per `target:`-bound tool.
pub fn generate_technician_command_safety_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut proofs = Vec::new();
    for tool in &ir.tools {
        let Some(witness) = derive_technician_command_safety_witness(tool, ir) else {
            continue;
        };
        proofs.push(ProofTerm {
            property: PropertyClass::TechnicianCommandSafety,
            artifact_digest: digest.clone(),
            witness: Witness::TechnicianCommandSafety(witness),
            axon_version: axon_version.to_string(),
        });
    }
    proofs
}

// ── §Fase 85.c — CacheSoundness ──────────────────────────────────────────────

/// §85.c — re-derive the whole-program cache witness from `ir.caches` +
/// `ir.tools` + `ir.channels`. "No contract → no proof": a program with no
/// `cache` declaration and no `tool.cache:` reference returns `None`.
pub fn derive_cache_soundness_witness(ir: &IRProgram) -> Option<CacheSoundnessWitness> {
    let uses_cache = !ir.caches.is_empty()
        || ir
            .tools
            .iter()
            .any(|t| !t.cache.is_empty() && t.cache != "none");
    if !uses_cache {
        return None;
    }

    let cache_names: Vec<String> = ir.caches.iter().map(|c| c.name.clone()).collect();
    let default_count = ir.caches.iter().filter(|c| c.default_policy).count();

    let base = |e: &str| e.split_once(':').map(|(b, _)| b).unwrap_or(e).to_string();
    let widened_without_ttl: Vec<String> = ir
        .caches
        .iter()
        .filter(|c| {
            let pure_only = c.apply_to_effects.iter().all(|e| base(e) == "pure");
            !pure_only && c.ttl.is_none()
        })
        .map(|c| c.name.clone())
        .collect();

    let channel_names: std::collections::HashSet<&str> =
        ir.channels.iter().map(|c| c.name.as_str()).collect();
    let cache_name_set: std::collections::HashSet<&str> =
        cache_names.iter().map(|s| s.as_str()).collect();
    let mut unresolved_refs = Vec::new();
    for c in &ir.caches {
        for ch in &c.invalidate_on {
            if !channel_names.contains(ch.as_str()) {
                unresolved_refs.push(format!("cache '{}' invalidate_on '{}'", c.name, ch));
            }
        }
    }
    for t in &ir.tools {
        if !t.cache.is_empty() && t.cache != "none" && !cache_name_set.contains(t.cache.as_str()) {
            unresolved_refs.push(format!("tool '{}' cache '{}'", t.name, t.cache));
        }
    }

    Some(CacheSoundnessWitness {
        cache_names,
        default_count,
        widened_without_ttl,
        unresolved_refs,
    })
}

/// §85.c — generate the (at most one) CacheSoundness proof for `ir`.
pub fn generate_cache_soundness_proofs(ir: &IRProgram, axon_version: &str) -> Vec<ProofTerm> {
    match derive_cache_soundness_witness(ir) {
        Some(witness) => vec![ProofTerm {
            property: PropertyClass::CacheSoundness,
            artifact_digest: artifact_digest(ir),
            witness: Witness::CacheSoundness(witness),
            axon_version: axon_version.to_string(),
        }],
        None => Vec::new(),
    }
}

// ── §Fase 98.d — ScrapeProvenanceSoundness ───────────────────────────────────

/// §98.d — the closed web-acquisition provider set (mirror of
/// `type_checker::VALID_SCRAPE_PROVIDERS`). The checker's own statement of
/// "what a scrape tool is" (D51.2).
const SCRAPE_PROVIDERS: &[&str] = &["scrape_http", "scrape_dom", "scrape_crawl", "scrape_enrich"];

fn effect_base(e: &str) -> &str {
    e.split_once(':').map(|(b, _)| b).unwrap_or(e)
}

/// Recursively accumulate the three content-injection-barrier signals over an
/// `IRFlow`'s step tree (mirror of the frontend `walk_flow_for_injection`).
fn walk_ir_for_injection(
    steps: &[crate::ir_nodes::IRFlowNode],
    is_web_tool: &dyn Fn(&str) -> bool,
    shielded_agents: &std::collections::HashSet<String>,
    web_producer: &mut bool,
    unshielded_belief: &mut bool,
    shield_present: &mut bool,
) {
    use crate::ir_nodes::IRFlowNode;
    for step in steps {
        match step {
            IRFlowNode::Step(s) => {
                for tref in [&s.apply_ref, &s.navigate_ref] {
                    if !tref.is_empty() && is_web_tool(tref) {
                        *web_producer = true;
                    }
                }
                let is_cognitive = !s.ask.is_empty() || !s.persona_ref.is_empty();
                let agent_shielded =
                    !s.persona_ref.is_empty() && shielded_agents.contains(&s.persona_ref);
                if is_cognitive && !agent_shielded {
                    *unshielded_belief = true;
                }
            }
            IRFlowNode::UseTool(u) => {
                if is_web_tool(&u.tool_name) {
                    *web_producer = true;
                }
            }
            IRFlowNode::ShieldApply(_) => {
                *shield_present = true;
            }
            IRFlowNode::Conditional(c) => {
                walk_ir_for_injection(
                    &c.then_body,
                    is_web_tool,
                    shielded_agents,
                    web_producer,
                    unshielded_belief,
                    shield_present,
                );
                walk_ir_for_injection(
                    &c.else_body,
                    is_web_tool,
                    shielded_agents,
                    web_producer,
                    unshielded_belief,
                    shield_present,
                );
            }
            IRFlowNode::ForIn(f) => {
                walk_ir_for_injection(
                    &f.body,
                    is_web_tool,
                    shielded_agents,
                    web_producer,
                    unshielded_belief,
                    shield_present,
                );
            }
            _ => {}
        }
    }
}

/// §98.d — re-derive the whole-program scrape-provenance witness from
/// `ir.tools` + `ir.flows` + `ir.agents`. "No contract → no proof": a program
/// declaring no web-acquisition tool returns `None`.
pub fn derive_scrape_provenance_soundness_witness(
    ir: &IRProgram,
) -> Option<ScrapeProvenanceSoundnessWitness> {
    let scrape_tools: Vec<(String, String)> = ir
        .tools
        .iter()
        .filter(|t| SCRAPE_PROVIDERS.contains(&t.provider.as_str()))
        .map(|t| (t.name.clone(), t.provider.clone()))
        .collect();
    if scrape_tools.is_empty() {
        return None;
    }

    // (T904) effect honesty.
    let mut tools_missing_web = Vec::new();
    let mut dom_tools_with_network = Vec::new();
    for t in ir
        .tools
        .iter()
        .filter(|t| SCRAPE_PROVIDERS.contains(&t.provider.as_str()))
    {
        let has = |b: &str| t.effect_row.iter().any(|e| effect_base(e) == b);
        if !has("web") {
            tools_missing_web.push(t.name.clone());
        }
        if t.provider == "scrape_dom" && has("network") {
            dom_tools_with_network.push(t.name.clone());
        }
    }

    // (T908) content-injection barrier, per flow.
    let web_tool_names: std::collections::HashSet<String> = ir
        .tools
        .iter()
        .filter(|t| t.effect_row.iter().any(|e| effect_base(e) == "web"))
        .map(|t| t.name.clone())
        .collect();
    let is_web_tool = |name: &str| web_tool_names.contains(name);
    let shielded_agents: std::collections::HashSet<String> = ir
        .agents
        .iter()
        .filter(|a| !a.shield_ref.is_empty())
        .map(|a| a.name.clone())
        .collect();

    let mut unshielded_flows = Vec::new();
    for flow in &ir.flows {
        let (mut web_producer, mut unshielded_belief, mut shield_present) = (false, false, false);
        walk_ir_for_injection(
            &flow.steps,
            &is_web_tool,
            &shielded_agents,
            &mut web_producer,
            &mut unshielded_belief,
            &mut shield_present,
        );
        if web_producer && unshielded_belief && !shield_present {
            unshielded_flows.push(flow.name.clone());
        }
    }

    Some(ScrapeProvenanceSoundnessWitness {
        scrape_tools,
        tools_missing_web,
        dom_tools_with_network,
        unshielded_flows,
    })
}

/// §98.d — generate the (at most one) ScrapeProvenanceSoundness proof for `ir`.
pub fn generate_scrape_provenance_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    match derive_scrape_provenance_soundness_witness(ir) {
        Some(witness) => vec![ProofTerm {
            property: PropertyClass::ScrapeProvenanceSoundness,
            artifact_digest: artifact_digest(ir),
            witness: Witness::ScrapeProvenanceSoundness(witness),
            axon_version: axon_version.to_string(),
        }],
        None => Vec::new(),
    }
}

// ── §Fase 116.a (D116.9) — ScopeCoverageSoundness ────────────────────────────

/// §116.a — accumulate every tool-use name over an `IRFlow`'s step tree (mirror
/// of the frontend flow walk; recurses `Conditional`/`ForIn` bodies).
fn collect_tool_uses(steps: &[crate::ir_nodes::IRFlowNode], out: &mut Vec<String>) {
    use crate::ir_nodes::IRFlowNode;
    for step in steps {
        match step {
            IRFlowNode::UseTool(u) => out.push(u.tool_name.clone()),
            IRFlowNode::Conditional(c) => {
                collect_tool_uses(&c.then_body, out);
                collect_tool_uses(&c.else_body, out);
            }
            IRFlowNode::ForIn(f) => collect_tool_uses(&f.body, out),
            _ => {}
        }
    }
}

/// §116.a — re-derive the whole-program scope-coverage witness from `ir.tools` +
/// `ir.credentials` + `ir.endpoints` + `ir.daemons` + `ir.flows`. "No contract →
/// no proof": a program with no tool declaring a `requires:` returns `None`.
/// This is the deploy-time twin of the frontend `check_use_tool_scopes` law
/// (`axon-T956`) — the two granted-set derivations must stay in lockstep.
pub fn derive_scope_coverage_soundness_witness(
    ir: &IRProgram,
) -> Option<ScopeCoverageSoundnessWitness> {
    // The contract: tools that DECLARE a required scope (source order).
    let scoped_tools: Vec<(String, Vec<String>)> = ir
        .tools
        .iter()
        .filter(|t| !t.requires.is_empty())
        .map(|t| {
            let mut r = t.requires.clone();
            r.sort();
            r.dedup();
            (t.name.clone(), r)
        })
        .collect();
    if scoped_tools.is_empty() {
        return None;
    }

    // The GRANTED set (program-wide, the mirror of the frontend `granted_scopes`):
    // ∪ credential.grants ∪ endpoint/daemon requires_capabilities.
    let mut granted_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for c in &ir.credentials {
        granted_set.extend(c.grants.iter().cloned());
    }
    for e in &ir.endpoints {
        granted_set.extend(e.requires_capabilities.iter().cloned());
    }
    for d in &ir.daemons {
        granted_set.extend(d.requires_capabilities.iter().cloned());
    }
    let granted: Vec<String> = granted_set.iter().cloned().collect();

    // Tool → required scopes (from the SAME source-order slice, unsorted so the
    // scan reports in the tool's declared order).
    let requires_of: std::collections::HashMap<&str, &Vec<String>> = ir
        .tools
        .iter()
        .filter(|t| !t.requires.is_empty())
        .map(|t| (t.name.as_str(), &t.requires))
        .collect();

    // Every flow's tool uses: a required scope not in the granted set is an
    // uncovered use (axon-T956).
    let mut uncovered_uses: Vec<(String, String, String)> = Vec::new();
    for flow in &ir.flows {
        let mut uses = Vec::new();
        collect_tool_uses(&flow.steps, &mut uses);
        for tool in &uses {
            if let Some(reqs) = requires_of.get(tool.as_str()) {
                for scope in reqs.iter() {
                    if !granted_set.contains(scope) {
                        uncovered_uses.push((flow.name.clone(), tool.clone(), scope.clone()));
                    }
                }
            }
        }
    }
    // Canonical order (both generate + check call this — stable equality).
    uncovered_uses.sort();
    uncovered_uses.dedup();

    Some(ScopeCoverageSoundnessWitness { scoped_tools, granted, uncovered_uses })
}

/// §116.a — generate the (at most one) ScopeCoverageSoundness proof for `ir`.
pub fn generate_scope_coverage_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    match derive_scope_coverage_soundness_witness(ir) {
        Some(witness) => vec![ProofTerm {
            property: PropertyClass::ScopeCoverageSoundness,
            artifact_digest: artifact_digest(ir),
            witness: Witness::ScopeCoverageSoundness(witness),
            axon_version: axon_version.to_string(),
        }],
        None => Vec::new(),
    }
}

// ── §Fase 99.d — DocumentProvenanceSoundness ─────────────────────────────────

const DOC_TARGETS: &[&str] = &["docx", "pptx", "xlsx"];

/// §99.d — the ASSERTIVE SLOT of a block kind (mirror of the frontend
/// `type_checker::doc_assertive_slot`). Must stay in lockstep — the barrier
/// re-derivation depends on it.
fn doc_assertive_slot(kind: &str) -> Option<&'static str> {
    match kind {
        "para" | "notes" | "footnote" | "heading" => Some("text"),
        "table" => Some("rows"),
        "chart" => Some("series"),
        "formula" => Some("expr"),
        "bullets" => Some("items"),
        "placeholder" => Some("text"),
        "row" => Some("cells"),
        _ => None,
    }
}

fn effect_base_doc(e: &str) -> &str {
    e.split_once(':').map(|(b, _)| b).unwrap_or(e)
}

/// Recurse a document block tree, flagging unattributed assertive-slot flow
/// values (mirror of the frontend barrier). `epistemic_ok` short-circuits.
fn walk_doc_blocks_for_barrier(
    doc_name: &str,
    blocks: &[crate::ir_nodes::IRDocBlock],
    epistemic_ok: bool,
    out: &mut Vec<String>,
) {
    for b in blocks {
        if let Some(slot) = doc_assertive_slot(&b.kind) {
            if let Some(f) = b.fields.iter().find(|f| f.name == slot) {
                if f.kind == "ref" {
                    let attributed = b.fields.iter().any(|x| x.name == "attribute");
                    if !attributed && !epistemic_ok {
                        out.push(format!("{doc_name}.{}.{slot}", b.kind));
                    }
                }
            }
        }
        walk_doc_blocks_for_barrier(doc_name, &b.children, epistemic_ok, out);
    }
}

/// §99.d — re-derive the whole-program document-provenance witness from
/// `ir.documents`. "No contract → no proof": a document-less program → `None`.
pub fn derive_document_provenance_soundness_witness(
    ir: &IRProgram,
) -> Option<DocumentProvenanceSoundnessWitness> {
    if ir.documents.is_empty() {
        return None;
    }
    let documents: Vec<(String, String)> = ir
        .documents
        .iter()
        .map(|d| (d.name.clone(), d.target.clone()))
        .collect();
    let mut bad_targets = Vec::new();
    let mut sensitive_without_legal = Vec::new();
    let mut unattributed_slots = Vec::new();
    for d in &ir.documents {
        if !DOC_TARGETS.contains(&d.target.as_str()) {
            bad_targets.push(d.name.clone());
        }
        let has_sensitive = d.effect_row.iter().any(|e| effect_base_doc(e) == "sensitive");
        let has_legal = d.effect_row.iter().any(|e| effect_base_doc(e) == "legal");
        if has_sensitive && !has_legal {
            sensitive_without_legal.push(d.name.clone());
        }
        let epistemic_ok = matches!(d.epistemic_mode.as_str(), "believe" | "know");
        walk_doc_blocks_for_barrier(&d.name, &d.blocks, epistemic_ok, &mut unattributed_slots);
    }
    Some(DocumentProvenanceSoundnessWitness {
        documents,
        bad_targets,
        sensitive_without_legal,
        unattributed_slots,
    })
}

/// §99.d — generate the (at most one) DocumentProvenanceSoundness proof for `ir`.
pub fn generate_document_provenance_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    match derive_document_provenance_soundness_witness(ir) {
        Some(witness) => vec![ProofTerm {
            property: PropertyClass::DocumentProvenanceSoundness,
            artifact_digest: artifact_digest(ir),
            witness: Witness::DocumentProvenanceSoundness(witness),
            axon_version: axon_version.to_string(),
        }],
        None => Vec::new(),
    }
}

// ── §Fase 105 — DeliveryProvenanceSoundness ──────────────────────────────────

const DELIVER_TARGETS: &[&str] = &["crm"];

/// §105 — re-derive the whole-program delivery-provenance witness from
/// `ir.deliveries`. "No contract → no proof": a delivery-less program → `None`.
/// Mirrors the frontend `check_deliver` laws (T921 target, T924 sensitive⇒legal,
/// T920 the provenance-stripping barrier) — MUST stay in lockstep.
pub fn derive_delivery_provenance_soundness_witness(
    ir: &IRProgram,
) -> Option<DeliveryProvenanceSoundnessWitness> {
    if ir.deliveries.is_empty() {
        return None;
    }
    let deliveries: Vec<(String, String)> = ir
        .deliveries
        .iter()
        .map(|d| (d.name.clone(), d.target.clone()))
        .collect();
    let mut bad_targets = Vec::new();
    let mut sensitive_without_legal = Vec::new();
    let mut laundered_deliveries = Vec::new();
    for d in &ir.deliveries {
        if !DELIVER_TARGETS.contains(&d.target.as_str()) {
            bad_targets.push(d.name.clone());
        }
        let has_sensitive = d.effect_row.iter().any(|e| effect_base_doc(e) == "sensitive");
        let has_legal = d.effect_row.iter().any(|e| effect_base_doc(e) == "legal");
        if has_sensitive && !has_legal {
            sensitive_without_legal.push(d.name.clone());
        }
        // T920 — the provenance-stripping barrier, re-derived: a `cleared`
        // delivery that binds any flow `ref` field, outside an epistemic vouch.
        let epistemic_ok = matches!(d.epistemic_mode.as_str(), "believe" | "know");
        let cleared = d.provenance == "cleared";
        let binds_flow_value = d
            .ops
            .iter()
            .any(|op| op.fields.iter().any(|f| f.kind == "ref"));
        if cleared && binds_flow_value && !epistemic_ok {
            laundered_deliveries.push(d.name.clone());
        }
    }
    Some(DeliveryProvenanceSoundnessWitness {
        deliveries,
        bad_targets,
        sensitive_without_legal,
        laundered_deliveries,
    })
}

/// §105 — generate the (at most one) DeliveryProvenanceSoundness proof for `ir`.
pub fn generate_delivery_provenance_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    match derive_delivery_provenance_soundness_witness(ir) {
        Some(witness) => vec![ProofTerm {
            property: PropertyClass::DeliveryProvenanceSoundness,
            artifact_digest: artifact_digest(ir),
            witness: Witness::DeliveryProvenanceSoundness(witness),
            axon_version: axon_version.to_string(),
        }],
        None => Vec::new(),
    }
}

// ── §Fase 107 — QuerySafetySoundness ─────────────────────────────────────────

/// §107 — the DECLARED WRITE reached by an IR flow body, at ANY nesting depth.
/// Mirror of the frontend `type_checker::first_declared_write` (D107.1) — MUST stay
/// in lockstep, since the PCC class re-derives the same law. Returns the verb name.
fn first_declared_write_ir(steps: &[crate::ir_nodes::IRFlowNode]) -> Option<&'static str> {
    use crate::ir_nodes::IRFlowNode as N;
    for s in steps {
        let hit = match s {
            N::Persist(_) => Some("persist"),
            N::Mutate(_) => Some("mutate"),
            N::Purge(_) => Some("purge"),
            N::Emit(_) => Some("emit"),
            N::Publish(_) => Some("publish"),
            N::Rotate(_) => Some("rotate"),
            N::Mint(_) => Some("mint"),
            N::Transact(_) => Some("transact"),
            // Fase 108.c (D108.4) - `ingest` appends to a server-resident
            // dataspace: state change. Mirrors the frontend's T927 walk -
            // prover and verifier must agree on the write surface.
            N::Ingest(_) => Some("ingest"),
            // Recurse — a nested write is still a write (an unsound proof is no proof).
            N::Conditional(c) => first_declared_write_ir(&c.then_body)
                .or_else(|| first_declared_write_ir(&c.else_body)),
            N::ForIn(f) => first_declared_write_ir(&f.body),
            N::Par(p) => p.branches.iter().find_map(|b| first_declared_write_ir(b)),
            N::Warden(w) => first_declared_write_ir(&w.body),
            _ => None,
        };
        if hit.is_some() {
            return hit;
        }
    }
    None
}

/// §107 — re-derive the whole-program QUERY-safety witness. "No contract → no
/// proof": a program with no `method: QUERY` endpoint → `None`.
pub fn derive_query_safety_soundness_witness(
    ir: &IRProgram,
) -> Option<QuerySafetySoundnessWitness> {
    let queries: Vec<&crate::ir_nodes::IRAxonEndpoint> = ir
        .endpoints
        .iter()
        .filter(|e| e.method.eq_ignore_ascii_case("QUERY"))
        .collect();
    if queries.is_empty() {
        return None;
    }
    let query_endpoints: Vec<(String, String)> = queries
        .iter()
        .map(|e| (e.name.clone(), e.execute_flow.clone()))
        .collect();

    let mut unsafe_queries = Vec::new();
    for e in &queries {
        if let Some(flow) = ir.flows.iter().find(|f| f.name == e.execute_flow) {
            if let Some(verb) = first_declared_write_ir(&flow.steps) {
                unsafe_queries.push(format!("{}:{}", e.name, verb));
            }
        }
    }

    // A program-level egress declaration fires for EVERY flow the executor runs
    // (D105.7-B), so no QUERY endpoint here could be safe.
    let mut egress_declarations: Vec<String> = ir
        .deliveries
        .iter()
        .map(|d| format!("deliver:{}", d.name))
        .collect();
    egress_declarations.extend(ir.documents.iter().map(|d| format!("document:{}", d.name)));

    Some(QuerySafetySoundnessWitness {
        query_endpoints,
        unsafe_queries,
        egress_declarations,
    })
}

/// §107 — generate the (at most one) QuerySafetySoundness proof for `ir`.
pub fn generate_query_safety_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    match derive_query_safety_soundness_witness(ir) {
        Some(witness) => vec![ProofTerm {
            property: PropertyClass::QuerySafetySoundness,
            artifact_digest: artifact_digest(ir),
            witness: Witness::QuerySafetySoundness(witness),
            axon_version: axon_version.to_string(),
        }],
        None => Vec::new(),
    }
}

// ── §Fase 110.b — NotificationProvenanceSoundness ────────────────────────────

/// §110.b — re-derive the whole-program notification witness. "No
/// contract → no proof": a program with no `notify` yields `None`.
pub fn derive_notification_provenance_soundness_witness(
    ir: &IRProgram,
) -> Option<NotificationProvenanceSoundnessWitness> {
    if ir.notifications.is_empty() {
        return None;
    }
    let mut bad_structure = Vec::new();
    let mut bad_windows = Vec::new();
    let mut laundered = Vec::new();
    for n in &ir.notifications {
        // T934 — structure.
        if !matches!(n.channel.as_str(), "sms" | "whatsapp" | "telegram") {
            bad_structure.push(format!("{}:channel:{}", n.name, n.channel));
        }
        if n.template.is_empty() {
            bad_structure.push(format!("{}:empty-template", n.name));
        }
        if n.to_secret.is_empty() {
            bad_structure.push(format!("{}:empty-recipient-ref", n.name));
        }
        if !n.effects.iter().any(|e| e == "web" || e.starts_with("web:")) {
            bad_structure.push(format!("{}:no-web-effect", n.name));
        }
        // T935 — the mandatory window.
        let w = n.window.trim();
        let window_ok = !w.is_empty()
            && w.len() >= 2
            && w[..w.len() - 1].chars().all(|c| c.is_ascii_digit())
            && matches!(w.chars().last(), Some('s') | Some('m') | Some('h') | Some('d'))
            && w[..w.len() - 1].parse::<u64>().map(|x| x > 0).unwrap_or(false);
        if !window_ok {
            bad_windows.push(format!("{}:window:{}", n.name, n.window));
        }
        // T933 — the evidence barrier.
        if n.provenance == "cleared"
            && n.template.contains("${")
            && !matches!(n.epistemic_mode.as_str(), "believe" | "know")
        {
            laundered.push(n.name.clone());
        }
    }
    Some(NotificationProvenanceSoundnessWitness {
        notifications: ir
            .notifications
            .iter()
            .map(|n| (n.name.clone(), n.channel.clone()))
            .collect(),
        bad_structure,
        bad_windows,
        laundered_notifications: laundered,
    })
}

/// §110.b — generate the (at most one) proof.
pub fn generate_notification_provenance_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    match derive_notification_provenance_soundness_witness(ir) {
        Some(witness) => vec![ProofTerm {
            property: PropertyClass::NotificationProvenanceSoundness,
            artifact_digest: artifact_digest(ir),
            witness: Witness::NotificationProvenanceSoundness(witness),
            axon_version: axon_version.to_string(),
        }],
        None => Vec::new(),
    }
}

// ── §Fase 109.b — GradientSoundness ──────────────────────────────────────────

/// §109.b — canonical fingerprint of a derivative list: the serde JSON of
/// the IRExpr vector (deterministic — field order is struct order).
fn derivative_fingerprint(derivatives: &[crate::ir_nodes::IRExpr]) -> String {
    serde_json::to_string(derivatives).unwrap_or_default()
}

/// §109.b — the IR-level re-differentiation. The frontend differentiates
/// `ast::Expr`; at deploy the artifact carries `IRExpr`, so the verifier
/// re-differentiates the IR form directly (the same §5.2 rules + §5.3
/// simplifier, ported over `IRExpr` — one closed grammar, two carriers).
fn ir_diff(e: &crate::ir_nodes::IRExpr, wrt: &str) -> Result<crate::ir_nodes::IRExpr, String> {
    use crate::ir_nodes::{IRExpr as E, IRExprLit as L};
    let zero = || E::Lit { lit: L::Float { value: 0.0 } };
    let one = || E::Lit { lit: L::Float { value: 1.0 } };
    match e {
        E::Lit { lit: L::Int { .. } } | E::Lit { lit: L::Float { .. } } => Ok(zero()),
        E::Lit { .. } => Err("non-numeric literal".to_string()),
        E::Ref { path } => Ok(if path == wrt { one() } else { zero() }),
        // §Fase 119.o — refused, and it MUST match the frontend's
        // `expr_diff::diff_at`, which refuses the same term for the same reason:
        // the `Ref` arm above answers 1 or 0 by NAME, so a reference to a
        // let-bound name inside the body would differentiate to 0 as though it
        // were a constant, and the gradient would be plausible and wrong.
        //
        // The two carriers agreeing is the point of this function existing
        // twice. If the frontend refused and this verifier quietly produced a
        // derivative, a proof-carrying artifact would attest a gradient the
        // compiler declined to compute.
        E::Let { .. } => Err("let binding".to_string()),
        E::Unary { op, operand } if op == "neg" => Ok(E::Unary {
            op: "neg".to_string(),
            operand: Box::new(ir_diff(operand, wrt)?),
        }),
        E::Unary { .. } => Err("logical not".to_string()),
        E::Binary { op, lhs, rhs } => match op.as_str() {
            "add" | "sub" => Ok(E::Binary {
                op: op.clone(),
                lhs: Box::new(ir_diff(lhs, wrt)?),
                rhs: Box::new(ir_diff(rhs, wrt)?),
            }),
            "mul" => Ok(E::Binary {
                op: "add".to_string(),
                lhs: Box::new(E::Binary {
                    op: "mul".to_string(),
                    lhs: Box::new(ir_diff(lhs, wrt)?),
                    rhs: rhs.clone(),
                }),
                rhs: Box::new(E::Binary {
                    op: "mul".to_string(),
                    lhs: lhs.clone(),
                    rhs: Box::new(ir_diff(rhs, wrt)?),
                }),
            }),
            "div" => Ok(E::Binary {
                op: "div".to_string(),
                lhs: Box::new(E::Binary {
                    op: "sub".to_string(),
                    lhs: Box::new(E::Binary {
                        op: "mul".to_string(),
                        lhs: Box::new(ir_diff(lhs, wrt)?),
                        rhs: rhs.clone(),
                    }),
                    rhs: Box::new(E::Binary {
                        op: "mul".to_string(),
                        lhs: lhs.clone(),
                        rhs: Box::new(ir_diff(rhs, wrt)?),
                    }),
                }),
                rhs: Box::new(E::Binary {
                    op: "mul".to_string(),
                    lhs: rhs.clone(),
                    rhs: rhs.clone(),
                }),
            }),
            other => Err(format!("operator {other}")),
        },
        E::Call { builtin, args } if builtin == "as_float" && args.len() == 1 => {
            ir_diff(&args[0], wrt)
        }
        E::Call { builtin, .. } => Err(format!("builtin {builtin}")),
        E::Field { .. } => Err("field access".to_string()),
        E::Index { .. } => Err("index access".to_string()),
    }
}

fn ir_num(e: &crate::ir_nodes::IRExpr) -> Option<f64> {
    use crate::ir_nodes::{IRExpr as E, IRExprLit as L};
    match e {
        E::Lit { lit: L::Float { value } } => Some(*value),
        E::Lit { lit: L::Int { value } } => Some(*value as f64),
        _ => None,
    }
}

fn ir_lit_f(v: f64) -> crate::ir_nodes::IRExpr {
    crate::ir_nodes::IRExpr::Lit {
        lit: crate::ir_nodes::IRExprLit::Float { value: v },
    }
}

/// §109.b — the §5.3 simplifier over `IRExpr` (the SAME rules the
/// frontend runs — D109.4: prover and verifier agree post-simplification).
fn ir_simplify(e: crate::ir_nodes::IRExpr) -> crate::ir_nodes::IRExpr {
    use crate::ir_nodes::IRExpr as E;
    match e {
        E::Unary { op, operand } if op == "neg" => {
            let inner = ir_simplify(*operand);
            match ir_num(&inner) {
                Some(v) => ir_lit_f(-v),
                None => E::Unary { op, operand: Box::new(inner) },
            }
        }
        E::Binary { op, lhs, rhs } => {
            let l = ir_simplify(*lhs);
            let r = ir_simplify(*rhs);
            if let (Some(a), Some(b)) = (ir_num(&l), ir_num(&r)) {
                let folded = match op.as_str() {
                    "add" => Some(a + b),
                    "sub" => Some(a - b),
                    "mul" => Some(a * b),
                    "div" if b != 0.0 => Some(a / b),
                    _ => None,
                };
                if let Some(v) = folded {
                    return ir_lit_f(v);
                }
            }
            let is0 = |x: &E| ir_num(x) == Some(0.0);
            let is1 = |x: &E| ir_num(x) == Some(1.0);
            match op.as_str() {
                "add" if is0(&l) => r,
                "add" if is0(&r) => l,
                "sub" if is0(&r) => l,
                "mul" if is0(&l) || is0(&r) => ir_lit_f(0.0),
                "mul" if is1(&l) => r,
                "mul" if is1(&r) => l,
                "div" if is0(&l) => ir_lit_f(0.0),
                "div" if is1(&r) => l,
                _ => E::Binary { op, lhs: Box::new(l), rhs: Box::new(r) },
            }
        }
        E::Call { builtin, args } => E::Call {
            builtin,
            args: args.into_iter().map(ir_simplify).collect(),
        },
        other => other,
    }
}

fn grad_violations_in(
    flow: &str,
    steps: &[crate::ir_nodes::IRFlowNode],
    grads: &mut Vec<(String, String, String, String)>,
    violations: &mut Vec<String>,
) {
    use crate::ir_nodes::IRFlowNode as N;
    for s in steps {
        match s {
            N::Grad(g) => {
                let fp = derivative_fingerprint(&g.derivatives);
                grads.push((
                    flow.to_string(),
                    g.target.clone(),
                    g.wrt.join(","),
                    fp,
                ));
                let Some(original) = &g.original else {
                    violations.push(format!("{flow}:{}:missing-original", g.target));
                    continue;
                };
                if g.derivatives.len() != g.wrt.len() || g.wrt.is_empty() {
                    violations.push(format!("{flow}:{}:arity", g.target));
                    continue;
                }
                for (var, stored) in g.wrt.iter().zip(&g.derivatives) {
                    match ir_diff(original, var).map(ir_simplify) {
                        Ok(expected) => {
                            if serde_json::to_string(&expected).unwrap_or_default()
                                != serde_json::to_string(stored).unwrap_or_default()
                            {
                                violations.push(format!(
                                    "{flow}:{}:wrt-{var}:derivative-mismatch",
                                    g.target
                                ));
                            }
                        }
                        Err(construct) => violations.push(format!(
                            "{flow}:{}:wrt-{var}:non-differentiable:{construct}",
                            g.target
                        )),
                    }
                }
            }
            N::Conditional(c) => {
                grad_violations_in(flow, &c.then_body, grads, violations);
                grad_violations_in(flow, &c.else_body, grads, violations);
            }
            N::ForIn(f) => grad_violations_in(flow, &f.body, grads, violations),
            N::Par(p) => p
                .branches
                .iter()
                .for_each(|b| grad_violations_in(flow, b, grads, violations)),
            N::Warden(w) => grad_violations_in(flow, &w.body, grads, violations),
            _ => {}
        }
    }
}

/// §109.b — re-derive the whole-program gradient witness. "No contract →
/// no proof": a program with no grad step yields `None`.
pub fn derive_gradient_soundness_witness(
    ir: &IRProgram,
) -> Option<GradientSoundnessWitness> {
    let mut grads = Vec::new();
    let mut violations = Vec::new();
    for flow in &ir.flows {
        grad_violations_in(&flow.name, &flow.steps, &mut grads, &mut violations);
    }
    if grads.is_empty() {
        return None;
    }
    Some(GradientSoundnessWitness { grads, violations })
}

/// §109.b — generate the (at most one) GradientSoundness proof.
pub fn generate_gradient_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    match derive_gradient_soundness_witness(ir) {
        Some(witness) => vec![ProofTerm {
            property: PropertyClass::GradientSoundness,
            artifact_digest: artifact_digest(ir),
            witness: Witness::GradientSoundness(witness),
            axon_version: axon_version.to_string(),
        }],
        None => Vec::new(),
    }
}

// ── §Fase 108.d — DataspaceSchemaSoundness ───────────────────────────────────

/// §108.d — walk a flow body (recursing into every nested block) and
/// re-derive the T930 laws over the IR: query verbs target declared
/// dataspaces; referenced columns exist; aggregates stay in the closed
/// catalog. Mirrors the frontend checker — prover and verifier agree.
fn dataspace_violations_in(
    steps: &[crate::ir_nodes::IRFlowNode],
    specs: &std::collections::HashMap<&str, Vec<&str>>,
    out: &mut Vec<String>,
) {
    use crate::ir_nodes::IRFlowNode as N;
    let has_col = |ds: &str, col: &str| specs.get(ds).map(|c| c.iter().any(|x| *x == col));
    for s in steps {
        match s {
            N::Focus(n) => {
                match specs.get(n.expression.as_str()) {
                    None => out.push(format!("focus:{}:not-a-dataspace", n.expression)),
                    Some(_) => {
                        for col in &n.select {
                            if has_col(&n.expression, col) == Some(false) {
                                out.push(format!("focus:{}:ghost-column:{col}", n.expression));
                            }
                        }
                    }
                }
            }
            N::Aggregate(n) => {
                if !specs.contains_key(n.target.as_str()) {
                    out.push(format!("aggregate:{}:not-a-dataspace", n.target));
                } else {
                    for col in &n.group_by {
                        if has_col(&n.target, col) == Some(false) {
                            out.push(format!("aggregate:{}:ghost-column:{col}", n.target));
                        }
                    }
                    for spec in &n.compute {
                        let (fname, col) = match spec.find('(') {
                            Some(p) if spec.ends_with(')') => {
                                (&spec[..p], Some(spec[p + 1..spec.len() - 1].trim()))
                            }
                            None => (spec.as_str(), None),
                            _ => {
                                out.push(format!("aggregate:{}:malformed:{spec}", n.target));
                                continue;
                            }
                        };
                        if !matches!(fname, "count" | "sum" | "avg" | "min" | "max") {
                            out.push(format!("aggregate:{}:unknown-fn:{fname}", n.target));
                        }
                        if let Some(col) = col {
                            if has_col(&n.target, col) == Some(false) {
                                out.push(format!("aggregate:{}:ghost-column:{col}", n.target));
                            }
                        }
                    }
                }
            }
            N::Associate(n) => {
                for side in [&n.left, &n.right] {
                    if !specs.contains_key(side.as_str()) {
                        out.push(format!("associate:{side}:not-a-dataspace"));
                    } else if !n.using_field.is_empty()
                        && has_col(side, &n.using_field) == Some(false)
                    {
                        out.push(format!("associate:{side}:ghost-column:{}", n.using_field));
                    }
                }
                if n.using_field.is_empty() {
                    out.push(format!("associate:{}:{}:no-key", n.left, n.right));
                }
            }
            N::Explore(n) => {
                if !specs.contains_key(n.target.as_str()) {
                    out.push(format!("explore:{}:not-a-dataspace", n.target));
                }
            }
            N::Ingest(n) => {
                if !specs.contains_key(n.target.as_str()) {
                    out.push(format!("ingest:{}:not-a-dataspace", n.target));
                }
            }
            // Recurse — a nested violation is still a violation.
            N::Conditional(c) => {
                dataspace_violations_in(&c.then_body, specs, out);
                dataspace_violations_in(&c.else_body, specs, out);
            }
            N::ForIn(f) => dataspace_violations_in(&f.body, specs, out),
            N::Par(p) => p
                .branches
                .iter()
                .for_each(|b| dataspace_violations_in(b, specs, out)),
            N::Warden(w) => dataspace_violations_in(&w.body, specs, out),
            _ => {}
        }
    }
}

/// §108.d — re-derive the whole-program dataspace-schema witness. "No
/// contract → no proof": a program with no dataspace AND no data-plane
/// verb yields `None`.
pub fn derive_dataspace_schema_soundness_witness(
    ir: &IRProgram,
) -> Option<DataspaceSchemaSoundnessWitness> {
    let mut violations: Vec<String> = Vec::new();
    let mut specs: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for spec in &ir.dataspace_specs {
        for col in &spec.columns {
            // The closed catalog (D108.1) — an unknown canonical type in
            // a stored artifact is a forged/stale IR.
            if !matches!(
                col.column_type.as_str(),
                "Text" | "Int" | "Float" | "Bool" | "Timestamp" | "Json"
            ) {
                violations.push(format!(
                    "dataspace:{}:unknown-type:{}:{}",
                    spec.name, col.name, col.column_type
                ));
            }
        }
        specs.insert(
            spec.name.as_str(),
            spec.columns.iter().map(|c| c.name.as_str()).collect(),
        );
    }
    let mut touches_plane = !ir.dataspace_specs.is_empty();
    for flow in &ir.flows {
        let before = violations.len();
        dataspace_violations_in(&flow.steps, &specs, &mut violations);
        if violations.len() > before {
            touches_plane = true;
        }
        // A flow may use the verbs correctly too — detect via a cheap walk.
        if !touches_plane {
            let mut probe: Vec<String> = Vec::new();
            let empty: std::collections::HashMap<&str, Vec<&str>> = Default::default();
            dataspace_violations_in(&flow.steps, &empty, &mut probe);
            if !probe.is_empty() {
                touches_plane = true; // verbs exist (they all violate vs. empty specs)
            }
        }
    }
    if !touches_plane {
        return None;
    }
    let dataspaces = ir
        .dataspace_specs
        .iter()
        .map(|d| {
            (
                d.name.clone(),
                d.columns
                    .iter()
                    .map(|c| format!("{}:{}", c.name, c.column_type))
                    .collect::<Vec<_>>()
                    .join(","),
            )
        })
        .collect();
    Some(DataspaceSchemaSoundnessWitness {
        dataspaces,
        violations,
    })
}

/// §108.d — generate the (at most one) DataspaceSchemaSoundness proof.
pub fn generate_dataspace_schema_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    match derive_dataspace_schema_soundness_witness(ir) {
        Some(witness) => vec![ProofTerm {
            property: PropertyClass::DataspaceSchemaSoundness,
            artifact_digest: artifact_digest(ir),
            witness: Witness::DataspaceSchemaSoundness(witness),
            axon_version: axon_version.to_string(),
        }],
        None => Vec::new(),
    }
}

// ── §Fase 100.e — DocumentIngestionSoundness ─────────────────────────────────

/// §100.e — re-derive the whole-program document-ingestion witness from
/// `ir.tools` + `ir.flows`. "No contract → no proof": no ingesting tool → None.
pub fn derive_document_ingestion_soundness_witness(
    ir: &IRProgram,
) -> Option<DocumentIngestionSoundnessWitness> {
    // An ingesting tool declares an `ingest:<class>` provenance member.
    let ingest_class = |t: &crate::ir_nodes::IRToolSpec| -> Option<String> {
        t.effect_row.iter().find_map(|e| e.strip_prefix("ingest:").map(|c| c.to_string()))
    };
    let ingest_tools: Vec<(String, String)> = ir
        .tools
        .iter()
        .filter_map(|t| ingest_class(t).map(|c| (t.name.clone(), c)))
        .collect();
    if ingest_tools.is_empty() {
        return None;
    }

    // (T1001) the Inferred ceiling: no tool with `ingest:inferred` +
    // `epistemic:know`.
    let mut inferred_ceiling_violations = Vec::new();
    for t in &ir.tools {
        let has_inferred = t.effect_row.iter().any(|e| e == "ingest:inferred");
        let has_know = t.effect_row.iter().any(|e| e == "epistemic:know");
        if has_inferred && has_know {
            inferred_ceiling_violations.push(t.name.clone());
        }
    }

    // (T908) the ingestion barrier: an ingesting tool's output reaching an
    // agent's beliefs unshielded. Reuse the same flow walk (`ingest:*` tools are
    // ingress producers, exactly like `web` tools).
    let ingest_tool_names: std::collections::HashSet<String> = ir
        .tools
        .iter()
        .filter(|t| ingest_class(t).is_some())
        .map(|t| t.name.clone())
        .collect();
    let is_ingest_tool = |name: &str| ingest_tool_names.contains(name);
    let shielded_agents: std::collections::HashSet<String> = ir
        .agents
        .iter()
        .filter(|a| !a.shield_ref.is_empty())
        .map(|a| a.name.clone())
        .collect();
    let mut unshielded_flows = Vec::new();
    for flow in &ir.flows {
        let (mut producer, mut unshielded_belief, mut shield_present) = (false, false, false);
        walk_ir_for_injection(
            &flow.steps,
            &is_ingest_tool,
            &shielded_agents,
            &mut producer,
            &mut unshielded_belief,
            &mut shield_present,
        );
        if producer && unshielded_belief && !shield_present {
            unshielded_flows.push(flow.name.clone());
        }
    }

    Some(DocumentIngestionSoundnessWitness {
        ingest_tools,
        inferred_ceiling_violations,
        unshielded_flows,
    })
}

/// §100.e — generate the (at most one) DocumentIngestionSoundness proof for `ir`.
pub fn generate_document_ingestion_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    match derive_document_ingestion_soundness_witness(ir) {
        Some(witness) => vec![ProofTerm {
            property: PropertyClass::DocumentIngestionSoundness,
            artifact_digest: artifact_digest(ir),
            witness: Witness::DocumentIngestionSoundness(witness),
            axon_version: axon_version.to_string(),
        }],
        None => Vec::new(),
    }
}

// ── §Fase 101.b — InferredCeilingSoundness ───────────────────────────────────

/// §101.b — re-derive the Inferred-ceiling witness over exactly the
/// `ingest:inferred` PRODUCERS. "No inferred producer → no proof" (the dual of
/// §100's vacuum): the class is inhabited only from §101, so this proof appears
/// only once a producer exists, and then asserts the vacuum's invariants still
/// hold — every producer capped at `believe`, none unshielded into beliefs.
pub fn derive_inferred_ceiling_soundness_witness(
    ir: &IRProgram,
) -> Option<InferredCeilingSoundnessWitness> {
    let is_inferred = |t: &crate::ir_nodes::IRToolSpec| {
        t.effect_row.iter().any(|e| e == "ingest:inferred")
    };
    let inferred_producers: Vec<String> =
        ir.tools.iter().filter(|t| is_inferred(t)).map(|t| t.name.clone()).collect();
    if inferred_producers.is_empty() {
        return None; // the §100 vacuum still holds — nothing to prove
    }

    // (T1001) the ceiling: an inferred producer may not declare `epistemic:know`.
    let ceiling_violations: Vec<String> = ir
        .tools
        .iter()
        .filter(|t| is_inferred(t) && t.effect_row.iter().any(|e| e == "epistemic:know"))
        .map(|t| t.name.clone())
        .collect();

    // (T908) the barrier: an inferred producer's output reaching an agent's
    // beliefs unshielded. Reuse the shared injection walk over exactly the
    // inferred producers.
    let inferred_names: std::collections::HashSet<String> =
        inferred_producers.iter().cloned().collect();
    let is_inferred_tool = |name: &str| inferred_names.contains(name);
    let shielded_agents: std::collections::HashSet<String> = ir
        .agents
        .iter()
        .filter(|a| !a.shield_ref.is_empty())
        .map(|a| a.name.clone())
        .collect();
    let mut unshielded_flows = Vec::new();
    for flow in &ir.flows {
        let (mut producer, mut unshielded_belief, mut shield_present) = (false, false, false);
        walk_ir_for_injection(
            &flow.steps,
            &is_inferred_tool,
            &shielded_agents,
            &mut producer,
            &mut unshielded_belief,
            &mut shield_present,
        );
        if producer && unshielded_belief && !shield_present {
            unshielded_flows.push(flow.name.clone());
        }
    }

    Some(InferredCeilingSoundnessWitness {
        inferred_producers,
        ceiling_violations,
        unshielded_flows,
    })
}

/// §101.b — generate the (at most one) InferredCeilingSoundness proof for `ir`.
pub fn generate_inferred_ceiling_soundness_proofs(
    ir: &IRProgram,
    axon_version: &str,
) -> Vec<ProofTerm> {
    match derive_inferred_ceiling_soundness_witness(ir) {
        Some(witness) => vec![ProofTerm {
            property: PropertyClass::InferredCeilingSoundness,
            artifact_digest: artifact_digest(ir),
            witness: Witness::InferredCeilingSoundness(witness),
            axon_version: axon_version.to_string(),
        }],
        None => Vec::new(),
    }
}

// ── §Fase 86.c — ForgeSoundness ──────────────────────────────────────────────

const FORGE_MODES: &[&str] = &["combinatorial", "exploratory", "transformational"];

/// Recursively collect `(flow_name, &IRForgeBlock)` from a flow's step tree
/// (forge blocks can nest inside `if` / `for` bodies).
fn collect_forge_blocks<'a>(
    steps: &'a [axon_frontend::ir_nodes::IRFlowNode],
    flow_name: &str,
    out: &mut Vec<(String, &'a axon_frontend::ir_nodes::IRForgeBlock)>,
) {
    use axon_frontend::ir_nodes::IRFlowNode;
    for step in steps {
        match step {
            IRFlowNode::Forge(f) => out.push((flow_name.to_string(), f)),
            IRFlowNode::Conditional(c) => {
                collect_forge_blocks(&c.then_body, flow_name, out);
                collect_forge_blocks(&c.else_body, flow_name, out);
            }
            IRFlowNode::ForIn(f) => collect_forge_blocks(&f.body, flow_name, out),
            _ => {}
        }
    }
}

/// §86.c — collect `&IRForgeBlock` from a step tree (checker helper; recurses
/// into `if`/`for` bodies).
pub(crate) fn __collect_forge_for_check<'a>(
    steps: &'a [axon_frontend::ir_nodes::IRFlowNode],
    out: &mut Vec<&'a axon_frontend::ir_nodes::IRForgeBlock>,
) {
    use axon_frontend::ir_nodes::IRFlowNode;
    for step in steps {
        match step {
            IRFlowNode::Forge(f) => out.push(f),
            IRFlowNode::Conditional(c) => {
                __collect_forge_for_check(&c.then_body, out);
                __collect_forge_for_check(&c.else_body, out);
            }
            IRFlowNode::ForIn(f) => __collect_forge_for_check(&f.body, out),
            _ => {}
        }
    }
}

/// §86.c — re-derive one forge block's soundness witness against the IR anchors.
pub fn derive_forge_soundness_witness(
    flow_name: &str,
    forge: &axon_frontend::ir_nodes::IRForgeBlock,
    ir: &IRProgram,
) -> ForgeSoundnessWitness {
    let mode_ok = forge.mode.is_empty() || FORGE_MODES.contains(&forge.mode.as_str());
    let novelty_in_range = forge.novelty >= 0.0 && forge.novelty <= 1.0;
    let bounds_ok = forge.depth >= 1 && forge.branches >= 1;
    let seed_and_type_present =
        !forge.seed.trim().is_empty() && !forge.output_type.trim().is_empty();
    let constraints_ok = forge.constraints_ref.is_empty()
        || ir
            .anchors
            .iter()
            .any(|a| a.name == forge.constraints_ref && a.confidence_floor.is_some());
    ForgeSoundnessWitness {
        forge_name: forge.name.clone(),
        flow_name: flow_name.to_string(),
        mode: forge.mode.clone(),
        novelty_milli: (forge.novelty * 1000.0).round() as i64,
        depth: forge.depth,
        branches: forge.branches,
        constraints_ref: forge.constraints_ref.clone(),
        mode_ok,
        novelty_in_range,
        bounds_ok,
        seed_and_type_present,
        constraints_ok,
    }
}

/// §86.c — one ForgeSoundness proof per `forge` block in the program.
pub fn generate_forge_soundness_proofs(ir: &IRProgram, axon_version: &str) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut forges = Vec::new();
    for flow in &ir.flows {
        collect_forge_blocks(&flow.steps, &flow.name, &mut forges);
    }
    forges
        .into_iter()
        .map(|(flow_name, forge)| ProofTerm {
            property: PropertyClass::ForgeSoundness,
            artifact_digest: digest.clone(),
            witness: Witness::ForgeSoundness(derive_forge_soundness_witness(&flow_name, forge, ir)),
            axon_version: axon_version.to_string(),
        })
        .collect()
}

/// §87.g — the checker's own statement of the `savant` cognition catalogs
/// (mirror of the private `axon_frontend::type_checker::VALID_SAVANT_*`).
const SAVANT_DEPTHS: &[&str] = &["standard", "deep", "hyper"];
const SAVANT_DIVERGENCES: &[&str] = &["low", "med", "high"];

/// §87.g — re-derive one savant's governance-soundness witness against the IR.
/// The single source of truth reused by both the prover and the verifier
/// (`check_savant_soundness`), so a forged/stale proof is caught by
/// re-derivation, exactly as ForgeSoundness/CacheSoundness.
pub fn derive_savant_soundness_witness(
    savant: &axon_frontend::ir_nodes::IRSavant,
    ir: &IRProgram,
) -> SavantSoundnessWitness {
    let domain_present = !savant.domain.trim().is_empty();

    let mandate_ok = !savant.mandates.is_empty()
        && savant
            .mandates
            .iter()
            .all(|m| !m.objective.trim().is_empty() && !m.output_type.trim().is_empty());

    let max_iterations = savant
        .budget
        .as_ref()
        .and_then(|b| b.max_iterations)
        .unwrap_or(0);
    let budget_bounded = max_iterations > 0;

    let cognition_ok = savant.cognition.as_ref().is_none_or(|c| {
        let depth_ok = c.depth.is_empty() || SAVANT_DEPTHS.contains(&c.depth.as_str());
        let div_ok = c.divergence.is_empty() || SAVANT_DIVERGENCES.contains(&c.divergence.as_str());
        let thr_ok = c.entropic_threshold.is_none_or(|t| t > 0.0);
        depth_ok && div_ok && thr_ok
    });

    let memory_ref_ok = savant.memory.as_ref().is_none_or(|m| {
        m.backend.is_empty()
            || ir.memories.iter().any(|x| x.name == m.backend)
            || ir.corpus_specs.iter().any(|x| x.name == m.backend)
    });

    SavantSoundnessWitness {
        savant_name: savant.name.clone(),
        mandate_count: savant.mandates.len() as i64,
        max_iterations,
        domain_present,
        mandate_ok,
        budget_bounded,
        cognition_ok,
        memory_ref_ok,
    }
}

/// §87.g — one SavantSoundness proof per `savant` in the program. `savant` is a
/// top-level declaration, so this iterates `ir.savants` directly (no flow-step
/// walk like forge).
pub fn generate_savant_soundness_proofs(ir: &IRProgram, axon_version: &str) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    ir.savants
        .iter()
        .map(|savant| ProofTerm {
            property: PropertyClass::SavantSoundness,
            artifact_digest: digest.clone(),
            witness: Witness::SavantSoundness(derive_savant_soundness_witness(savant, ir)),
            axon_version: axon_version.to_string(),
        })
        .collect()
}

// ── §Fase 88.e — WardenSoundness ─────────────────────────────────────────────

const SCOPE_DEPTHS: &[&str] = &["static_artifact", "memory_dump", "live_network"];

/// Recursively collect `(flow_name, &IRWarden)` from a flow's step tree (warden
/// blocks can nest inside `if`/`for`/`quant` and other warden bodies).
fn collect_warden_blocks<'a>(
    steps: &'a [axon_frontend::ir_nodes::IRFlowNode],
    flow_name: &str,
    out: &mut Vec<(String, &'a axon_frontend::ir_nodes::IRWarden)>,
) {
    use axon_frontend::ir_nodes::IRFlowNode;
    for step in steps {
        match step {
            IRFlowNode::Warden(w) => {
                out.push((flow_name.to_string(), w));
                collect_warden_blocks(&w.body, flow_name, out);
            }
            IRFlowNode::Conditional(c) => {
                collect_warden_blocks(&c.then_body, flow_name, out);
                collect_warden_blocks(&c.else_body, flow_name, out);
            }
            IRFlowNode::ForIn(f) => collect_warden_blocks(&f.body, flow_name, out),
            IRFlowNode::Quant(q) => collect_warden_blocks(&q.body, flow_name, out),
            _ => {}
        }
    }
}

/// §88.e — collect `&IRWarden` from a step tree (checker helper; recurses into
/// `if`/`for`/`quant`/warden bodies).
pub(crate) fn __collect_warden_for_check<'a>(
    steps: &'a [axon_frontend::ir_nodes::IRFlowNode],
    out: &mut Vec<&'a axon_frontend::ir_nodes::IRWarden>,
) {
    use axon_frontend::ir_nodes::IRFlowNode;
    for step in steps {
        match step {
            IRFlowNode::Warden(w) => {
                out.push(w);
                __collect_warden_for_check(&w.body, out);
            }
            IRFlowNode::Conditional(c) => {
                __collect_warden_for_check(&c.then_body, out);
                __collect_warden_for_check(&c.else_body, out);
            }
            IRFlowNode::ForIn(f) => __collect_warden_for_check(&f.body, out),
            IRFlowNode::Quant(q) => __collect_warden_for_check(&q.body, out),
            _ => {}
        }
    }
}

/// §88.e — re-derive one warden block's authorization-soundness witness against
/// the IR (`scopes`). The single source of truth reused by the prover + the
/// verifier (`check_warden_soundness`), so a forged/stale proof is caught by
/// re-derivation.
pub fn derive_warden_soundness_witness(
    flow_name: &str,
    warden: &axon_frontend::ir_nodes::IRWarden,
    ir: &IRProgram,
) -> WardenSoundnessWitness {
    let scope = ir.scopes.iter().find(|s| s.name == warden.scope_ref);
    WardenSoundnessWitness {
        warden_target: warden.target.clone(),
        flow_name: flow_name.to_string(),
        scope_ref: warden.scope_ref.clone(),
        scope_resolves: scope.is_some(),
        targets_nonempty: scope.is_some_and(|s| !s.targets.is_empty()),
        depth_ok: scope
            .is_some_and(|s| s.depth.is_empty() || SCOPE_DEPTHS.contains(&s.depth.as_str())),
        approver_present: scope.is_some_and(|s| !s.approver.trim().is_empty()),
    }
}

/// §88.e — one WardenSoundness proof per `warden` block in the program.
pub fn generate_warden_soundness_proofs(ir: &IRProgram, axon_version: &str) -> Vec<ProofTerm> {
    let digest = artifact_digest(ir);
    let mut wardens = Vec::new();
    for flow in &ir.flows {
        collect_warden_blocks(&flow.steps, &flow.name, &mut wardens);
    }
    wardens
        .into_iter()
        .map(|(flow_name, warden)| ProofTerm {
            property: PropertyClass::WardenSoundness,
            artifact_digest: digest.clone(),
            witness: Witness::WardenSoundness(derive_warden_soundness_witness(
                &flow_name, warden, ir,
            )),
            axon_version: axon_version.to_string(),
        })
        .collect()
}

/// §51.f — generate proofs across ALL property classes for `ir`. The
/// `axon pcc prove` entry point. Concatenates every per-class
/// generator (compliance / effects / capability-gate / resources /
/// shields / capability-containment / tool-call-soundness /
/// effect-budgeted) — one bundle covering every certifiable property an
/// apx program declares.
pub fn generate_all_proofs(ir: &IRProgram, axon_version: &str) -> Vec<ProofTerm> {
    let mut proofs = Vec::new();
    proofs.extend(generate_compliance_coverage_proofs(ir, axon_version));
    proofs.extend(generate_effect_row_soundness_proofs(ir, axon_version));
    proofs.extend(generate_capability_isolation_proofs(ir, axon_version));
    proofs.extend(generate_resource_bounds_proofs(ir, axon_version));
    proofs.extend(generate_shield_halt_guarantee_proofs(ir, axon_version));
    proofs.extend(generate_capability_containment_proofs(ir, axon_version));
    proofs.extend(generate_tool_call_soundness_proofs(ir, axon_version));
    proofs.extend(generate_effect_budgeted_proofs(ir, axon_version));
    proofs.extend(generate_json_shape_soundness_proofs(ir, axon_version));
    proofs.extend(generate_channel_delivery_soundness_proofs(ir, axon_version));
    proofs.extend(generate_aggregate_soundness_proofs(ir, axon_version));
    proofs.extend(generate_channel_egress_soundness_proofs(ir, axon_version));
    proofs.extend(generate_interruptible_session_soundness_proofs(ir, axon_version));
    proofs.extend(generate_parked_residual_soundness_proofs(ir, axon_version));
    proofs.extend(generate_upstream_projection_soundness_proofs(ir, axon_version));
    proofs.extend(generate_cors_policy_consistency_proofs(ir, axon_version));
    proofs.extend(generate_technician_command_safety_proofs(ir, axon_version));
    proofs.extend(generate_cache_soundness_proofs(ir, axon_version));
    proofs.extend(generate_scrape_provenance_soundness_proofs(ir, axon_version));
    proofs.extend(generate_scope_coverage_soundness_proofs(ir, axon_version));
    proofs.extend(generate_document_provenance_soundness_proofs(ir, axon_version));
    proofs.extend(generate_delivery_provenance_soundness_proofs(ir, axon_version));
    proofs.extend(generate_query_safety_soundness_proofs(ir, axon_version));
    proofs.extend(generate_dataspace_schema_soundness_proofs(ir, axon_version));
    proofs.extend(generate_gradient_soundness_proofs(ir, axon_version));
    proofs.extend(generate_notification_provenance_soundness_proofs(ir, axon_version));
    proofs.extend(generate_document_ingestion_soundness_proofs(ir, axon_version));
    proofs.extend(generate_inferred_ceiling_soundness_proofs(ir, axon_version));
    proofs.extend(generate_forge_soundness_proofs(ir, axon_version));
    proofs.extend(generate_savant_soundness_proofs(ir, axon_version));
    proofs.extend(generate_warden_soundness_proofs(ir, axon_version));
    proofs.extend(generate_authorization_coverage_proofs(ir, axon_version));
    proofs.extend(generate_temporal_context_soundness_proofs(ir, axon_version));
    proofs.extend(generate_credential_attenuation_proofs(ir, axon_version));
    proofs.extend(generate_secret_custody_proofs(ir, axon_version));
    proofs
}
