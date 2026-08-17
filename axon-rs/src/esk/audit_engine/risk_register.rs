//! AXON Audit Evidence Engine — RiskRegister
//!
//! Direct port of `axon/runtime/esk/audit_engine/risk_register.py`.
//!
//! Derives an ISO 27001 / NIST 800-53-shaped risk register from a
//! compiled `IRProgram`. Each row describes:
//!
//!   * The threat                 (what goes wrong)
//!   * The asset(s) impacted      (which IRProgram element)
//!   * The applicable control(s)  (framework controls that mitigate it)
//!   * The likelihood + impact     (qualitative L/M/H defaults)
//!   * The treatment              (accept / mitigate / transfer / avoid)
//!   * The AXON primitive implementing the treatment
//!
//! Output is JSON-serialisable so it can be consumed by a GRC platform
//! (ServiceNow, ZenGRC, Hyperproof) or attached directly to an ISO 27001
//! Stage 1 submission.

#![allow(dead_code)]

use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::ir_nodes::IRProgram;

// ═══════════════════════════════════════════════════════════════════
//  Qualitative scales
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Likelihood {
    Low,
    Medium,
    High,
}

impl Likelihood {
    pub fn as_str(&self) -> &'static str {
        match self {
            Likelihood::Low => "low",
            Likelihood::Medium => "medium",
            Likelihood::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Impact {
    Low,
    Medium,
    High,
    Critical,
}

impl Impact {
    pub fn as_str(&self) -> &'static str {
        match self {
            Impact::Low => "low",
            Impact::Medium => "medium",
            Impact::High => "high",
            Impact::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Treatment {
    Accept,
    Mitigate,
    Transfer,
    Avoid,
}

impl Treatment {
    pub fn as_str(&self) -> &'static str {
        match self {
            Treatment::Accept => "accept",
            Treatment::Mitigate => "mitigate",
            Treatment::Transfer => "transfer",
            Treatment::Avoid => "avoid",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Risk row
// ═══════════════════════════════════════════════════════════════════

/// A single row in the risk register.
#[derive(Debug, Clone)]
pub struct Risk {
    pub risk_id: String,
    pub threat: String,
    pub asset: String,
    pub likelihood: String,
    pub impact: String,
    pub applicable_controls: Vec<String>,
    pub treatment: String,
    pub axon_primitive: String,
    /// 1-9: likelihood_ordinal × impact_ordinal (impact saturates at 3).
    pub residual_score: i64,
}

impl Risk {
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("risk_id".into(), self.risk_id.clone().into());
        m.insert("threat".into(), self.threat.clone().into());
        m.insert("asset".into(), self.asset.clone().into());
        m.insert("likelihood".into(), self.likelihood.clone().into());
        m.insert("impact".into(), self.impact.clone().into());
        m.insert(
            "applicable_controls".into(),
            Value::Array(
                self.applicable_controls
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
        m.insert("treatment".into(), self.treatment.clone().into());
        m.insert("axon_primitive".into(), self.axon_primitive.clone().into());
        m.insert("residual_score".into(), self.residual_score.into());
        Value::Object(m)
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Canonical threat catalog — ISO 27005-informed
// ═══════════════════════════════════════════════════════════════════

struct Template {
    threat: &'static str,
    likelihood: Likelihood,
    impact: Impact,
    controls: &'static [&'static str],
    treatment: Treatment,
    primitive: &'static str,
    feature_gate: Option<&'static str>,
}

fn template_threats() -> Vec<Template> {
    vec![
        Template {
            threat: "Regulated data crosses an uncovered boundary (HIPAA/PCI violation)",
            likelihood: Likelihood::High,
            impact: Impact::Critical,
            controls: &["CC6.6", "FDP_ACC.1", "A.5.36"],
            treatment: Treatment::Mitigate,
            primitive: "Compile-time Compliance (RTT)",
            // §124.c — gated on the coverage rule HOLDING, not on a label
            // being present: this row asserts the risk is MITIGATED, and
            // presence of an annotation mitigates nothing.
            feature_gate: Some("compliance_coverage_holds"),
        },
        Template {
            threat: "Prompt injection subverts intended flow behavior",
            likelihood: Likelihood::High,
            impact: Impact::High,
            controls: &["CC7.1", "CC7.3", "A.8.7"],
            treatment: Treatment::Mitigate,
            primitive: "immune + reflex + EID",
            feature_gate: Some("has_immune"),
        },
        Template {
            threat: "Cryptographic keys or PII leak through logs / traces",
            likelihood: Likelihood::Medium,
            impact: Impact::Critical,
            controls: &["CC6.7", "A.8.12", "FPT_ITC.1"],
            treatment: Treatment::Mitigate,
            primitive: "Secret[T] no-materialize",
            feature_gate: None, // always applicable
        },
        Template {
            threat: "Audit records tampered after-the-fact",
            likelihood: Likelihood::Medium,
            impact: Impact::High,
            controls: &["CC6.8", "A.5.28", "FAU_STG.1"],
            treatment: Treatment::Mitigate,
            primitive: "ProvenanceChain Merkle + HMAC/Ed25519",
            feature_gate: None,
        },
        Template {
            threat: "Resource aliased across manifests (double-provision / split-brain)",
            likelihood: Likelihood::Medium,
            impact: Impact::High,
            controls: &["CC6.1", "FDP_IFC.1", "A.8.2"],
            treatment: Treatment::Mitigate,
            primitive: "Linear Logic + Separation Logic compile-time check",
            feature_gate: Some("has_manifest"),
        },
        Template {
            threat: "Post-quantum break of classical signatures (Shor-capable adversary)",
            likelihood: Likelihood::Low,
            impact: Impact::Critical,
            controls: &["CC6.8", "A.8.24", "FCS_COP.1"],
            treatment: Treatment::Mitigate,
            primitive: "DilithiumSigner + HybridSigner (FIPS 204)",
            feature_gate: None,
        },
        Template {
            threat: "Unbounded AI agency (heal applied twice, reconcile never halts)",
            likelihood: Likelihood::Medium,
            impact: Impact::High,
            controls: &["CC5.1", "FRU_RSA.1"],
            treatment: Treatment::Mitigate,
            primitive: "heal.max_patches + reconcile.max_retries bounds",
            feature_gate: Some("has_heal"),
        },
        Template {
            threat: "Statistical extraction attack (model theft) over many queries",
            likelihood: Likelihood::Medium,
            impact: Impact::High,
            controls: &["P4.1", "FPR_PSE.1", "A.8.12"],
            treatment: Treatment::Mitigate,
            primitive: "PrivacyBudget ε-limit (Differential Privacy)",
            feature_gate: None,
        },
        Template {
            threat: "Network partition observed as false-positive 'healthy'",
            likelihood: Likelihood::Medium,
            impact: Impact::Medium,
            controls: &["CC7.2", "A.5.30"],
            treatment: Treatment::Mitigate,
            primitive: "NetworkPartitionError (CT-3, Decision D4)",
            feature_gate: None,
        },
        Template {
            threat: "Session deadlock between endpoint↔daemon↔resource",
            likelihood: Likelihood::Low,
            impact: Impact::High,
            controls: &["CC4.2", "A.8.27"],
            treatment: Treatment::Mitigate,
            primitive: "π-calculus Honda-liveness compile-time check",
            feature_gate: Some("has_topology"),
        },
        Template {
            threat: "Lease token used post-expiration (stale capability)",
            likelihood: Likelihood::Medium,
            impact: Impact::High,
            controls: &["CC6.3", "A.8.2", "A.8.10"],
            treatment: Treatment::Mitigate,
            primitive: "LeaseKernel τ-decay + Anchor Breach (CT-2)",
            feature_gate: Some("has_lease"),
        },
        Template {
            threat: "Supply-chain compromise (malicious dependency)",
            likelihood: Likelihood::Low,
            impact: Impact::Critical,
            controls: &["CC8.1", "A.8.32", "A.5.33"],
            treatment: Treatment::Mitigate,
            primitive: "Deterministic SBOM + in-toto SLSA v1 attestation",
            feature_gate: None,
        },
    ]
}

// ═══════════════════════════════════════════════════════════════════
//  Scoring helpers
// ═══════════════════════════════════════════════════════════════════

fn score(label: &str) -> i64 {
    match label {
        // impact saturates at 3 to keep residual_score ∈ [1, 9]
        "low" => 1,
        "medium" => 2,
        "high" => 3,
        "critical" => 3,
        _ => 0,
    }
}

fn residual(likelihood: &str, impact: &str) -> i64 {
    score(likelihood) * score(impact)
}

// ═══════════════════════════════════════════════════════════════════
//  Feature detection (duplicated from gap_analyzer to stay self-contained)
// ═══════════════════════════════════════════════════════════════════

fn program_features(program: &IRProgram) -> HashSet<String> {
    let mut features: HashSet<String> = HashSet::new();
    if !program.shields.is_empty()      { features.insert("has_shield".into()); }
    if !program.resources.is_empty()    { features.insert("has_resource".into()); }
    if !program.manifests.is_empty()    { features.insert("has_manifest".into()); }
    if !program.observations.is_empty() { features.insert("has_observe".into()); }
    if !program.immunes.is_empty()      { features.insert("has_immune".into()); }
    if !program.reflexes.is_empty()     { features.insert("has_reflex".into()); }
    if !program.heals.is_empty()        { features.insert("has_heal".into()); }
    if !program.reconciles.is_empty()   { features.insert("has_reconcile".into()); }
    if !program.leases.is_empty()       { features.insert("has_lease".into()); }
    if !program.ensembles.is_empty()    { features.insert("has_ensemble".into()); }
    if !program.topologies.is_empty()   { features.insert("has_topology".into()); }
    if !program.endpoints.is_empty()    { features.insert("has_endpoint".into()); }

    // §124.c — was `has_compliance_annotation`, granted on the mere PRESENCE
    // of a label anywhere. The rows that lean on this feature assert
    // mitigation, so the feature now means the coverage rule HOLDS — every
    // regulated boundary names a shield whose κ covers the data's κ. The
    // rule lives once, in `coverage.rs` (§120.e: a rule duplicated drifts).
    if super::coverage::compliance_coverage_holds(program) {
        features.insert("compliance_coverage_holds".into());
    }
    features
}

// ═══════════════════════════════════════════════════════════════════
//  Public API
// ═══════════════════════════════════════════════════════════════════

/// Build the risk register for a compiled program.
///
/// Only risks whose `feature_gate` is `None` or present in the program's
/// features are included — irrelevant risks are pruned.
pub fn generate_risk_register(program: &IRProgram) -> Vec<Risk> {
    let features = program_features(program);
    let mut rows: Vec<Risk> = Vec::new();
    let mut counter: usize = 0;
    for tpl in template_threats() {
        if let Some(gate) = tpl.feature_gate {
            if !features.contains(gate) {
                continue;
            }
        }
        counter += 1;
        let likelihood = tpl.likelihood.as_str().to_string();
        let impact = tpl.impact.as_str().to_string();
        let residual_score = residual(&likelihood, &impact);

        // §Fase 111 F8 — a risk may only be reported as MITIGATED by a
        // primitive that actually runs. The Cognitive-I/O kernels
        // (`heal`/`reconcile`/`lease`/`immune`/`reflex`/`ensemble`/`observe`/
        // `resource`) have no dispatch path, so a template claiming
        // `treatment: Mitigate` on the strength of one is telling an auditor
        // that a control is in place when nothing enforces it.
        //
        // Such a risk is not mitigated — it is ACCEPTED, silently. We now say
        // so, and we say who is at fault: the gap is AXON's, not the adopter's.
        let treated_by_unenforced = tpl
            .feature_gate
            .is_some_and(|g| !super::runtime_wiring::feature_is_enforced(g));

        let (treatment, axon_primitive) = if treated_by_unenforced
            && tpl.treatment == Treatment::Mitigate
        {
            (
                Treatment::Accept.as_str().to_string(),
                format!(
                    "{} — NOT ENFORCED: the primitive is declared but has no runtime \
                     dispatch path, so this risk is accepted, not mitigated (§111 F8)",
                    tpl.primitive
                ),
            )
        } else {
            (tpl.treatment.as_str().to_string(), tpl.primitive.to_string())
        };

        rows.push(Risk {
            risk_id: format!("AXON-RISK-{:03}", counter),
            threat: tpl.threat.into(),
            asset: "program_state".into(),
            likelihood,
            impact,
            applicable_controls: tpl.controls.iter().map(|s| (*s).to_string()).collect(),
            treatment,
            axon_primitive,
            residual_score,
        });
    }
    rows
}

pub fn risk_register_to_value(risks: &[Risk]) -> Value {
    let mut m = Map::new();
    m.insert("schema".into(), "axon.esk.risk_register.v1".into());
    m.insert("iso_reference".into(), "ISO/IEC 27005:2022".into());
    m.insert("total_risks".into(), (risks.len() as i64).into());
    m.insert(
        "risks".into(),
        Value::Array(risks.iter().map(|r| r.to_value()).collect()),
    );
    Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir_generator::IRGenerator;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn compile(source: &str) -> IRProgram {
        let tokens = Lexer::new(source, "t").tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        IRGenerator::new().generate(&program)
    }

    fn full_program() -> IRProgram {
        compile(r#"
            type R compliance [HIPAA] { x: String }
            flow F(r: R) -> R { step S { ask: "x" output: R } }
            shield G {
                scan: [prompt_injection]
                on_breach: halt
                severity: high
                compliance: [HIPAA]
            }
            axonendpoint E {
                method: POST path: "/p" body: R execute: F output: R
                shield: G
                compliance: [HIPAA]
            }
            resource Db { kind: postgres lifetime: linear }
            fabric Vpc { provider: aws }
            manifest M { resources: [Db] fabric: Vpc compliance: [HIPAA] }
            observe O from M { sources: [prom] quorum: 1 }
            reconcile Rec { observe: O }
            lease L { resource: Db duration: 30m }
            immune I { watch: [O] scope: tenant }
            reflex Rf { trigger: I on_level: doubt action: quarantine scope: tenant }
            heal H { source: I scope: tenant }
        "#)
    }

    #[test]
    fn full_program_emits_multiple_risks() {
        let risks = generate_risk_register(&full_program());
        assert!(risks.len() >= 8, "expected >= 8 risks, got {}", risks.len());
    }

    #[test]
    fn minimal_program_prunes_feature_gated_risks() {
        let minimal = compile("type Basic { note: String }");
        let full = full_program();
        let m = generate_risk_register(&minimal);
        let f = generate_risk_register(&full);
        assert!(
            m.len() < f.len(),
            "minimal ({}) should prune more than full ({})",
            m.len(),
            f.len()
        );
    }

    /// 🎯 §124.c — the regulated-boundary row asserts MITIGATION, and it
    /// appears exactly when the coverage rule holds. Pre-§124.c, all three
    /// programs below scored identically: any label anywhere granted the gate.
    #[test]
    fn the_regulated_boundary_risk_requires_coverage_not_presence() {
        let boundary_row =
            |ir: &IRProgram| generate_risk_register(ir).iter().any(|r| r.threat.contains("uncovered boundary"));

        let label_only = compile("type Phi compliance [HIPAA] { x: String }");
        assert!(
            !boundary_row(&label_only),
            "a κ label nothing carries mitigates nothing — this row asserting \
             'Mitigate' over it was the presence defect"
        );

        let uncovered = compile(
            r#"
            type Phi compliance [HIPAA, PCI_DSS] { x: String }
            shield Weak { scan: [pii_leak] compliance: [HIPAA] }
            channel PhiFeed { message: Phi shield: Weak }
            "#,
        );
        assert!(
            !boundary_row(&uncovered),
            "an UNCOVERED regulated boundary is the risk UNMITIGATED — the row \
             may not claim otherwise"
        );

        let covered = compile(
            r#"
            type Phi compliance [HIPAA] { x: String }
            shield Gate { scan: [pii_leak] compliance: [HIPAA] }
            channel PhiFeed { message: Phi shield: Gate }
            "#,
        );
        assert!(boundary_row(&covered), "coverage holding is what earns the row");
    }

    #[test]
    fn ids_are_sequential_and_unique() {
        let risks = generate_risk_register(&full_program());
        for (i, r) in risks.iter().enumerate() {
            let expected = format!("AXON-RISK-{:03}", i + 1);
            assert_eq!(r.risk_id, expected);
        }
    }

    #[test]
    fn residual_score_in_range() {
        let risks = generate_risk_register(&full_program());
        for r in &risks {
            assert!(
                (1..=9).contains(&r.residual_score),
                "score {} out of [1, 9] for {}",
                r.residual_score,
                r.risk_id
            );
        }
    }

    #[test]
    fn residual_score_saturates_impact_at_three() {
        // critical (→3) × high (→3) == 9, NOT 12.
        assert_eq!(residual("high", "critical"), 9);
        assert_eq!(residual("low", "critical"), 3);
        assert_eq!(residual("medium", "medium"), 4);
    }

    #[test]
    fn to_value_shape_has_schema_marker() {
        let risks = generate_risk_register(&full_program());
        let payload = risk_register_to_value(&risks);
        assert_eq!(payload["schema"], "axon.esk.risk_register.v1");
        assert_eq!(payload["total_risks"], risks.len() as i64);
        assert_eq!(
            payload["risks"].as_array().map(|a| a.len()),
            Some(risks.len())
        );
    }

    #[test]
    fn to_value_deterministic_on_equal_input() {
        let ir = full_program();
        let a = risk_register_to_value(&generate_risk_register(&ir));
        let b = risk_register_to_value(&generate_risk_register(&ir));
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }
}
