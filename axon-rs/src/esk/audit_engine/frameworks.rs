//! AXON Audit Evidence Engine — Framework Catalog
//!
//! Direct port of `axon/runtime/esk/audit_engine/frameworks.py`.
//!
//! Structured catalog of the controls each external audit framework tests,
//! and which AXON primitive / module / generated artifact provides evidence
//! for each one.

#![allow(dead_code)]

use serde::Serialize;

/// Canonical identifiers for supported external audit frameworks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum FrameworkId {
    Soc2TypeII,
    Iso27001,
    Fips140_3,
    CcEal4Plus,
}

impl FrameworkId {
    pub fn as_str(&self) -> &'static str {
        match self {
            FrameworkId::Soc2TypeII => "soc2_type_ii",
            FrameworkId::Iso27001 => "iso_27001",
            FrameworkId::Fips140_3 => "fips_140_3",
            FrameworkId::CcEal4Plus => "cc_eal4_plus",
        }
    }
}

/// How the evidence for a control is produced at audit time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum EvidenceKind {
    CompileTime,
    RuntimeInvariant,
    AutomatedArtifact,
    TestSuite,
    ManualPolicy,
    ExternalOperational,
}

impl EvidenceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EvidenceKind::CompileTime => "compile_time",
            EvidenceKind::RuntimeInvariant => "runtime_invariant",
            EvidenceKind::AutomatedArtifact => "automated_artifact",
            EvidenceKind::TestSuite => "test_suite",
            EvidenceKind::ManualPolicy => "manual_policy",
            EvidenceKind::ExternalOperational => "external_operational",
        }
    }
}

/// A single control within an audit framework.
#[derive(Debug, Clone)]
pub struct Control {
    pub framework: FrameworkId,
    pub control_id: &'static str,
    pub title: &'static str,
    pub axon_primitive: &'static str,
    pub evidence_kind: EvidenceKind,
    pub evidence_locator: &'static str,
}

impl Control {
    pub fn key(&self) -> String {
        format!("{}:{}", self.framework.as_str(), self.control_id)
    }
}

macro_rules! ctl {
    ($fw:ident, $id:expr, $title:expr, $prim:expr, $kind:ident, $loc:expr) => {
        Control {
            framework: FrameworkId::$fw,
            control_id: $id,
            title: $title,
            axon_primitive: $prim,
            evidence_kind: EvidenceKind::$kind,
            evidence_locator: $loc,
        }
    };
}

/// SOC 2 Type II — Common Criteria + C + PI + P subset.
fn soc2_controls() -> Vec<Control> {
    vec![
        ctl!(Soc2TypeII, "CC1.1", "Commitment to integrity", "Zero-shortcuts policy + signed commits", ManualPolicy, "CODE_OF_CONDUCT.md (repo-external)"),
        ctl!(Soc2TypeII, "CC2.1", "Quality information", "axon check compile-time compliance", CompileTime, "axon.compiler.type_checker._check_regulatory_compliance"),
        ctl!(Soc2TypeII, "CC3.1", "Objectives specification", "manifest.compliance declares system's κ", CompileTime, "axon.compiler.ast_nodes.ManifestDefinition"),
        ctl!(Soc2TypeII, "CC3.2", "Risk identification", "immune + KL-divergence baseline", RuntimeInvariant, "axon.runtime.immune.AnomalyDetector"),
        ctl!(Soc2TypeII, "CC3.3", "Fraud potential", "reflex action quarantine/terminate", RuntimeInvariant, "axon.runtime.immune.ReflexEngine"),
        ctl!(Soc2TypeII, "CC4.1", "Control activities", "ESK 6 subsystems", TestSuite, "tests/test_phase6_*.py"),
        ctl!(Soc2TypeII, "CC4.2", "Technology controls", "shield + type checker + Phase 4 pass", CompileTime, "axon.compiler.type_checker"),
        ctl!(Soc2TypeII, "CC5.1", "Policies deployment", "axonendpoint.shield binding", CompileTime, "axon.compiler.ast_nodes.AxonEndpointDefinition"),
        ctl!(Soc2TypeII, "CC6.1", "Logical access controls", "Secret[T] with audit trail", RuntimeInvariant, "axon.runtime.esk.Secret"),
        ctl!(Soc2TypeII, "CC6.2", "Credential provisioning", "Handler-level credential + CT-3 classification", RuntimeInvariant, "axon.runtime.handlers.base"),
        ctl!(Soc2TypeII, "CC6.3", "Access modification/removal", "lease τ-decay", RuntimeInvariant, "axon.runtime.lease_kernel.LeaseKernel"),
        ctl!(Soc2TypeII, "CC6.6", "Boundary access controls", "axonendpoint.shield mandatory", CompileTime, "_check_regulatory_compliance"),
        ctl!(Soc2TypeII, "CC6.7", "Information transmission restriction", "Secret[T] no-materialize", RuntimeInvariant, "tests/test_phase6_runtime.py::TestSecret"),
        ctl!(Soc2TypeII, "CC6.8", "Unauthorized change prevention", "ProvenanceChain Merkle", RuntimeInvariant, "axon.runtime.esk.ProvenanceChain"),
        ctl!(Soc2TypeII, "CC7.1", "Event detection", "immune + EID", RuntimeInvariant, "axon.runtime.esk.EpistemicIntrusionDetector"),
        ctl!(Soc2TypeII, "CC7.2", "Anomaly monitoring", "KL-based AnomalyDetector", RuntimeInvariant, "axon.runtime.immune.AnomalyDetector"),
        ctl!(Soc2TypeII, "CC7.3", "Incident evaluation", "EID severity mapping", RuntimeInvariant, "axon.runtime.esk.eid.IntrusionEvent"),
        ctl!(Soc2TypeII, "CC7.4", "Incident response", "reflex + heal linear", RuntimeInvariant, "axon.runtime.immune.HealKernel"),
        ctl!(Soc2TypeII, "CC7.5", "Recovery", "reconcile Active Inference", RuntimeInvariant, "axon.runtime.reconcile_loop.ReconcileLoop"),
        ctl!(Soc2TypeII, "CC8.1", "Change authorization", "SBOM deterministic content_hash", AutomatedArtifact, "axon sbom CLI"),
        ctl!(Soc2TypeII, "CC9.1", "Risk mitigation activities", "PrivacyBudget ε-tracker", RuntimeInvariant, "axon.runtime.esk.PrivacyBudget"),
        ctl!(Soc2TypeII, "CC9.2", "Vendor risk management", "SBOM dependencies list", AutomatedArtifact, "axon sbom CLI"),
        ctl!(Soc2TypeII, "C1.1", "Confidential information identification", "type κ annotation", CompileTime, "TypeDefinition.compliance"),
        ctl!(Soc2TypeII, "C1.2", "Confidential information disposal", "lease τ-decay + Secret audit", RuntimeInvariant, "LeaseKernel + Secret.audit_trail"),
        ctl!(Soc2TypeII, "PI1.1", "Processing objectives", "axon check passes ⟺ objectives met", CompileTime, "axon check exit 0"),
        ctl!(Soc2TypeII, "PI1.4", "Output completeness", "Byzantine ensemble quorum", RuntimeInvariant, "axon.runtime.ensemble_aggregator"),
        ctl!(Soc2TypeII, "PI1.5", "Information retention", "ProvenanceChain append-only", RuntimeInvariant, "ProvenanceChain.append"),
        ctl!(Soc2TypeII, "P1.1", "Notice to data subjects", "dossier classes_covered", AutomatedArtifact, "axon dossier CLI"),
        ctl!(Soc2TypeII, "P4.1", "Collection limitation", "Differential Privacy mechanisms", RuntimeInvariant, "axon.runtime.esk.privacy"),
        ctl!(Soc2TypeII, "P5.1", "Data subject access", "Secret audit + ProvenanceChain", RuntimeInvariant, "Secret.audit_trail"),
        ctl!(Soc2TypeII, "P6.1", "Disclosure restriction", "shield<GDPR> mandatory gate", CompileTime, "_check_regulatory_compliance"),
    ]
}

/// ISO 27001:2022 — Annex A subset (organizational + technological).
fn iso_27001_controls() -> Vec<Control> {
    vec![
        ctl!(Iso27001, "A.5.1", "Information security policies", "manifest.compliance declarations", CompileTime, "ManifestDefinition"),
        ctl!(Iso27001, "A.5.2", "Security roles", "heal.mode human_in_loop", RuntimeInvariant, "HealDefinition.mode"),
        ctl!(Iso27001, "A.5.3", "Segregation of duties", "shield.allow_tools / deny_tools", CompileTime, "ShieldDefinition"),
        ctl!(Iso27001, "A.5.7", "Threat intelligence", "immune baseline-learned KL", RuntimeInvariant, "AnomalyDetector"),
        ctl!(Iso27001, "A.5.8", "InfoSec in project management", "CI axon check gate", ExternalOperational, ".github/workflows/*.yml"),
        ctl!(Iso27001, "A.5.23", "Cloud services InfoSec", "Handler protocol abstraction", RuntimeInvariant, "axon.runtime.handlers"),
        ctl!(Iso27001, "A.5.24", "Incident management planning", "immune+reflex+heal+EID", RuntimeInvariant, "axon.runtime.immune + esk.eid"),
        ctl!(Iso27001, "A.5.25", "Event assessment", "EID classify_severity", RuntimeInvariant, "EpistemicIntrusionDetector.observe"),
        ctl!(Iso27001, "A.5.26", "Incident response", "reflex signed_trace", RuntimeInvariant, "ReflexEngine"),
        ctl!(Iso27001, "A.5.27", "Learning from incidents", "ProvenanceChain ledger", AutomatedArtifact, "ProvenanceChain.entries"),
        ctl!(Iso27001, "A.5.28", "Evidence collection", "Merkle chain + HMAC", AutomatedArtifact, "ProvenanceChain.append"),
        ctl!(Iso27001, "A.5.30", "Business continuity ICT", "reconcile self-healing", RuntimeInvariant, "ReconcileLoop"),
        ctl!(Iso27001, "A.5.33", "Records protection", "Immutable ProvenanceChain + SBOM", AutomatedArtifact, "axon sbom"),
        ctl!(Iso27001, "A.5.34", "Privacy / PII protection", "compliance [GDPR, CCPA] + PrivacyBudget + Secret[T]", CompileTime, "TypeDefinition.compliance + PrivacyBudget"),
        ctl!(Iso27001, "A.5.36", "Compliance with policies", "axon check enforces RTT", CompileTime, "_check_regulatory_compliance"),
        ctl!(Iso27001, "A.8.1", "User endpoint devices", "axonendpoint.shield gate", CompileTime, "AxonEndpointDefinition"),
        ctl!(Iso27001, "A.8.2", "Privileged access rights", "lease τ-decay", RuntimeInvariant, "LeaseKernel"),
        ctl!(Iso27001, "A.8.3", "Information access restriction", "shield.allow_tools + Secret[T]", RuntimeInvariant, "ShieldDefinition + Secret"),
        ctl!(Iso27001, "A.8.5", "Secure authentication", "Handler credential flow + CT-3", RuntimeInvariant, "axon.runtime.handlers"),
        ctl!(Iso27001, "A.8.6", "Capacity management", "resource.capacity + manifest.zones", CompileTime, "ResourceDefinition"),
        ctl!(Iso27001, "A.8.7", "Protection against malware", "immune behavioral detection", RuntimeInvariant, "AnomalyDetector + ReflexEngine"),
        ctl!(Iso27001, "A.8.8", "Technical vulnerability management", "heal + human_in_loop + signed", RuntimeInvariant, "HealKernel"),
        ctl!(Iso27001, "A.8.9", "Configuration management", "deterministic SBOM program_hash", AutomatedArtifact, "axon sbom"),
        ctl!(Iso27001, "A.8.10", "Information deletion", "lease on_expire anchor_breach", RuntimeInvariant, "LeaseKernel"),
        ctl!(Iso27001, "A.8.12", "Data leakage prevention", "Secret[T] no-materialize", RuntimeInvariant, "tests/test_phase6_runtime.py::TestSecret"),
        ctl!(Iso27001, "A.8.13", "Information backup", "resource.lifetime persistent + axonstore", CompileTime, "ResourceDefinition"),
        ctl!(Iso27001, "A.8.15", "Logging", "reflex HMAC signed_trace", RuntimeInvariant, "ReflexEngine"),
        ctl!(Iso27001, "A.8.16", "Monitoring", "immune continuous sensing", RuntimeInvariant, "AnomalyDetector"),
        ctl!(Iso27001, "A.8.17", "Clock synchronization", "ISO-8601 UTC timestamps", RuntimeInvariant, "LambdaEnvelope.tau"),
        ctl!(Iso27001, "A.8.20", "Network security", "Handler CT-3 partition classification", RuntimeInvariant, "NetworkPartitionError"),
        ctl!(Iso27001, "A.8.23", "Web filtering", "shield.scan threat categories", CompileTime, "ShieldDefinition.scan"),
        ctl!(Iso27001, "A.8.24", "Use of cryptography", "HmacSigner + Ed25519Signer + DilithiumSigner", RuntimeInvariant, "axon.runtime.esk.provenance"),
        ctl!(Iso27001, "A.8.25", "SDL", "axon check CI gate", ExternalOperational, ".github/workflows"),
        ctl!(Iso27001, "A.8.26", "Application security requirements", "compliance declarations", CompileTime, "TypeDefinition + ShieldDefinition"),
        ctl!(Iso27001, "A.8.27", "Secure architecture", "π-calculus + Linear + Separation Logic compile-time", CompileTime, "type_checker.*"),
        ctl!(Iso27001, "A.8.28", "Secure coding", "Compile-time type errors (Theorem 5.1)", CompileTime, "paper_lambda_lineal_epistemico.md"),
        ctl!(Iso27001, "A.8.29", "Security testing", "pytest 3680 tests", TestSuite, "tests/"),
        ctl!(Iso27001, "A.8.30", "Outsourced development", "Dual-remote strategy + SBOM", AutomatedArtifact, "DEVELOPMENT.md + axon sbom"),
        ctl!(Iso27001, "A.8.31", "Dev/test/prod separation", "fabric.ephemeral", CompileTime, "FabricDefinition"),
        ctl!(Iso27001, "A.8.32", "Change management", "deterministic SBOM diff", AutomatedArtifact, "axon sbom"),
        ctl!(Iso27001, "A.8.33", "Test information", "DP-noise test data", RuntimeInvariant, "laplace_noise / gaussian_noise"),
    ]
}

/// FIPS 140-3 — Cryptographic Module Validation.
fn fips_140_3_controls() -> Vec<Control> {
    vec![
        ctl!(Fips140_3, "FIPS.ALG_APPROVED", "Only FIPS-approved algorithms used", "HMAC-SHA256 + SHA-256 + Ed25519 + ML-DSA-65", RuntimeInvariant, "axon.runtime.esk.provenance"),
        ctl!(Fips140_3, "FIPS.BOUNDARY", "Cryptographic boundary defined", "ESK-CB: provenance.py + secret.py + attestation.py", ManualPolicy, "documents/compliance/fips_140_3_submission_template.md"),
        ctl!(Fips140_3, "FIPS.FSM", "Finite State Model", "POWER-OFF → INITIALIZED → OPERATIONAL", ManualPolicy, "documents/compliance/fips_140_3_submission_template.md, section 6"),
        ctl!(Fips140_3, "FIPS.KAT_HMAC", "HMAC-SHA256 Known Answer Test", "Test with fixed key/message → known tag", TestSuite, "tests/test_phase6_runtime.py"),
        ctl!(Fips140_3, "FIPS.KAT_SHA", "SHA-256 Known Answer Test", "SHA-256('abc') verification", TestSuite, "tests/test_phase6_runtime.py"),
        // v4.0.0 — this control carried the literal locator "PENDING"
        // since the matrix was written: a control that documented its own
        // absence. The KATs exist now, and they are stronger than the ask —
        // RFC 8032 section 7.1 vectors asserted byte-exactly (seed→pk AND signature,
        // three keypairs) plus byte-identity against an independent
        // implementation, which determinism makes possible for Ed25519.
        ctl!(Fips140_3, "FIPS.KAT_ED25519", "Ed25519 KAT (RFC 8032 vectors)", "RFC 8032 section 7.1 byte-exact + cross-impl byte-identity", TestSuite, "axon-csys/tests/ed25519_kat.rs"),
        ctl!(Fips140_3, "FIPS.PCT", "Pairwise consistency test Ed25519", "generate → sign → verify", TestSuite, "tests/test_phase6_runtime.py"),
        ctl!(Fips140_3, "FIPS.RNG", "Approved RNG source", "Python secrets.token_bytes (OS CSPRNG)", RuntimeInvariant, "HmacSigner.random"),
        ctl!(Fips140_3, "FIPS.CT_COMPARE", "Constant-time comparisons", "hmac.compare_digest for MAC verify", RuntimeInvariant, "HmacSigner.verify"),
        ctl!(Fips140_3, "FIPS.KEY_LIFECYCLE", "Key zeroization on dispose", "Garbage collection + no plaintext in repr", RuntimeInvariant, "Secret[T] invariant"),
        ctl!(Fips140_3, "FIPS.DELIVERY", "Secure delivery procedures", "crates.io publishes + GitHub release signatures", ExternalOperational, "Cargo.toml + release workflow"),
        ctl!(Fips140_3, "FIPS.SELF_TEST", "Pre-operational self-test on load", "Pending: module-load KAT execution", TestSuite, "PENDING"),
        ctl!(Fips140_3, "FIPS.LAB_CAVP", "CAVP algorithm testing", "Business: engage NIST-accredited lab", ExternalOperational, "PENDING (external)"),
        ctl!(Fips140_3, "FIPS.CMVP", "CMVP module validation", "Business: submission to NIST", ExternalOperational, "PENDING (external)"),
    ]
}

/// Common Criteria EAL 4+ — subset of SFRs + SARs.
fn cc_eal4_plus_controls() -> Vec<Control> {
    vec![
        // FAU — Security Audit
        ctl!(CcEal4Plus, "FAU_GEN.1", "Audit data generation", "ReflexOutcome + ProvenanceChain", RuntimeInvariant, "ProvenanceChain.append"),
        ctl!(CcEal4Plus, "FAU_GEN.2", "User identity association", "Secret.audit_trail accessor", RuntimeInvariant, "SecretAccess"),
        ctl!(CcEal4Plus, "FAU_SAR.1", "Audit review", "axon dossier / sbom + chain.entries()", AutomatedArtifact, "CLI commands"),
        ctl!(CcEal4Plus, "FAU_STG.1", "Protected audit storage", "Merkle append-only chain", RuntimeInvariant, "ProvenanceChain"),
        // FCO — Communication
        ctl!(CcEal4Plus, "FCO_NRO.1", "Proof of origin", "Ed25519Signer / DilithiumSigner", RuntimeInvariant, "provenance.py"),
        // FCS — Cryptographic Support
        ctl!(CcEal4Plus, "FCS_COP.1", "Cryptographic operation", "HMAC-SHA256 + Ed25519 + Dilithium3", RuntimeInvariant, "provenance.py"),
        ctl!(CcEal4Plus, "FCS_CKM.1", "Key generation", "HmacSigner.random via secrets.token_bytes", RuntimeInvariant, "HmacSigner"),
        // FDP — User Data Protection
        ctl!(CcEal4Plus, "FDP_ACC.1", "Access control (coverage rule)", "κ(shield) ⊇ κ(body)∪κ(output)", CompileTime, "_check_regulatory_compliance"),
        ctl!(CcEal4Plus, "FDP_IFC.1", "Information flow control", "κ propagation through types", CompileTime, "TypeDefinition + type_checker"),
        ctl!(CcEal4Plus, "FDP_IFF.1", "Security attributes", "compliance: [HIPAA,...] annotation", CompileTime, "TypeDefinition"),
        ctl!(CcEal4Plus, "FDP_ITC.2", "Import with attributes", "manifest.compliance at ingest boundary", CompileTime, "ManifestDefinition"),
        // FIA — Identification & Authentication
        ctl!(CcEal4Plus, "FIA_UAU.1", "Authentication timing", "Handler-level credential flow", RuntimeInvariant, "axon.runtime.handlers"),
        // FMT — Security Management
        ctl!(CcEal4Plus, "FMT_MSA.1", "Security attribute management", "Only shield definitions set compliance", CompileTime, "ShieldDefinition.compliance"),
        ctl!(CcEal4Plus, "FMT_SMR.1", "Security roles", "Crypto Officer + User + Reviewer (heal.mode)", RuntimeInvariant, "HealDefinition.mode"),
        // FPR — Privacy
        ctl!(CcEal4Plus, "FPR_PSE.1", "Pseudonymity", "DP Laplace mechanism", RuntimeInvariant, "laplace_noise"),
        ctl!(CcEal4Plus, "FPR_UNL.1", "Unlinkability", "DP Gaussian with composition", RuntimeInvariant, "gaussian_noise + PrivacyBudget"),
        // FPT — Protection of TSF
        ctl!(CcEal4Plus, "FPT_TST.1", "TSF testing", "pytest 3680 tests", TestSuite, "tests/"),
        ctl!(CcEal4Plus, "FPT_ITC.1", "Inter-TSF confidentiality", "Secret[T] no-materialize", RuntimeInvariant, "Secret"),
        // FRU — Resource Utilisation
        ctl!(CcEal4Plus, "FRU_RSA.1", "Maximum quotas", "resource.capacity + heal.max_patches", CompileTime, "ResourceDefinition + HealDefinition"),
        // FTP — Trusted Path/Channels
        ctl!(CcEal4Plus, "FTP_ITC.1", "Inter-TSF trusted channel", "Operator's TLS stack (outside TOE)", ExternalOperational, "PENDING (operator-provided)"),
        // SARs (augmentations for EAL 4+)
        ctl!(CcEal4Plus, "ALC_FLR.2", "Flaw reporting procedures", "GitHub Issues SLA", ExternalOperational, "SECURITY.md (organization-authored)"),
        ctl!(CcEal4Plus, "AVA_VAN.5", "Advanced vulnerability analysis", "External pentest (accredited lab)", ExternalOperational, "PENDING (external)"),
    ]
}

pub fn controls_for(framework: FrameworkId) -> Vec<Control> {
    match framework {
        FrameworkId::Soc2TypeII => soc2_controls(),
        FrameworkId::Iso27001 => iso_27001_controls(),
        FrameworkId::Fips140_3 => fips_140_3_controls(),
        FrameworkId::CcEal4Plus => cc_eal4_plus_controls(),
    }
}

pub fn all_frameworks() -> Vec<FrameworkId> {
    vec![
        FrameworkId::Soc2TypeII,
        FrameworkId::Iso27001,
        FrameworkId::Fips140_3,
        FrameworkId::CcEal4Plus,
    ]
}

pub fn control_count(framework: FrameworkId) -> usize {
    controls_for(framework).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_counts_match_python() {
        assert_eq!(control_count(FrameworkId::Soc2TypeII), 31);
        assert_eq!(control_count(FrameworkId::Iso27001), 41);
        assert_eq!(control_count(FrameworkId::Fips140_3), 14);
        assert_eq!(control_count(FrameworkId::CcEal4Plus), 22);
    }

    #[test]
    fn control_ids_unique_within_framework() {
        for fw in all_frameworks() {
            let ids: Vec<_> = controls_for(fw).iter().map(|c| c.control_id).collect();
            let mut sorted = ids.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(ids.len(), sorted.len(), "duplicate ids in {:?}", fw);
        }
    }

    #[test]
    fn every_control_has_axon_primitive_and_locator() {
        for fw in all_frameworks() {
            for c in controls_for(fw) {
                assert!(!c.axon_primitive.is_empty(), "{:?} {} missing primitive", fw, c.control_id);
                assert!(!c.evidence_locator.is_empty(), "{:?} {} missing locator", fw, c.control_id);
            }
        }
    }

    #[test]
    fn framework_id_as_str_matches_python() {
        assert_eq!(FrameworkId::Soc2TypeII.as_str(), "soc2_type_ii");
        assert_eq!(FrameworkId::Iso27001.as_str(), "iso_27001");
        assert_eq!(FrameworkId::Fips140_3.as_str(), "fips_140_3");
        assert_eq!(FrameworkId::CcEal4Plus.as_str(), "cc_eal4_plus");
    }
}
