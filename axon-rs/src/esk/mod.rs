//! AXON Runtime — Epistemic Security Kernel (v1.2.0).
//!
//! Direct port of `axon/runtime/esk/`. The ESK sits at the sealed boundary
//! of the runtime and gives every artefact a cryptographic, regulatory,
//! and epistemic identity that external audits (SOC 2 / ISO 27001 / FIPS /
//! CC EAL 4+) can verify without access to Axon internals.
//!
//! Sub-modules:
//! * `compliance` — canonical κ registry (v1.2.0).
//! * `provenance` — HMAC / Merkle-chained signed envelopes (v1.2.0).
//! * `attestation` — SBOM + ComplianceDossier + in-toto Statement (v1.2.0).
//!   * `audit_engine` — gap analysis, risk register, evidence packager.

pub mod attestation;
pub mod audit_engine;
pub mod compliance;
/// v4.0.0 — the hybrid evidence signer, `Ed25519(H) ‖ ML-DSA-65(H)`.
/// Gated on `csys-native`: its cryptography is the C module boundary, and
/// v2.83.0 made the C toolchain opt-in. Without the feature, the HMAC-SHA256
/// baseline in [`provenance`] remains the signer.
#[cfg(feature = "csys-native")]
pub mod hybrid_signer;
pub mod provenance;
