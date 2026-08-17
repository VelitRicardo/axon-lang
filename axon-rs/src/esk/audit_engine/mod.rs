//! AXON Runtime — Audit Evidence Engine (§ESK Fase 7.x, external audits).
//!
//! Direct port of `axon/runtime/esk/audit_engine/`. Closes the gap between
//! Axon's runtime primitives and external audits (SOC 2 Type II / ISO
//! 27001:2022 / FIPS 140-3 / CC EAL 4+) by automating every engineering
//! step that can precede an accredited lab or CPA engagement.

pub mod control_statements;
pub mod coverage;
#[cfg(feature = "documents")]
pub mod evidence_packager;
pub mod frameworks;
pub mod gap_analyzer;
pub mod risk_register;
pub mod runtime_wiring;

pub use control_statements::{
    ControlImplementationStatement, generate_control_statements, statements_to_value,
};
#[cfg(feature = "documents")]
pub use evidence_packager::{EvidencePackage, build_evidence_package};
pub use frameworks::{Control, EvidenceKind, FrameworkId, all_frameworks, control_count, controls_for};
pub use gap_analyzer::{ControlAssessment, GapAnalysis, analyze_all, analyze_gaps};
pub use runtime_wiring::{Wiring, feature_is_enforced, unenforced_features, wiring_of, wiring_or_absent};
pub use risk_register::{
    Impact, Likelihood, Risk, Treatment, generate_risk_register, risk_register_to_value,
};
