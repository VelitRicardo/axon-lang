//! v1.4.0 — OTS (Ontological Tool Synthesis) compile-time slug catalogs.
//!
//! These constants are consumed by `type_checker` to validate that
//! `ots:transform:*` and `ots:backend:*` effect slugs carry only
//! qualifiers from the closed catalog. The runtime side of OTS —
//! pipeline execution, transformer registry, native transcoders,
//! ffmpeg subprocess fallback — lives in the `axon` runtime crate
//! at `axon_rs::ots` and re-exports these constants for backward
//! compatibility.
//!
//! Keeping the catalog here (not in the runtime) means any tool that
//! analyses AXON source (LSP, linters, analyzers) can validate OTS
//! usage without linking the full runtime.

/// Effect slug `ots:transform:<from>:<to>` declares a tool as
/// performing a kind conversion. The checker verifies a path exists.
pub const OTS_TRANSFORM_EFFECT_SLUG: &str = "ots:transform";

/// Effect slug `ots:backend:<kind>` declares HOW the conversion
/// happens. Closed qualifier set — new backends require a patch.
pub const OTS_BACKEND_EFFECT_SLUG: &str = "ots:backend";

/// Catalogue of valid `ots:backend:<qualifier>` qualifiers.
///
/// v2.4.0 — extended with the two `quant` cognitive-primitive backends so
/// `ots:backend:quant_sim` / `ots:backend:qpu_native` are first-class,
/// type-checked algebraic-effect slugs (alongside the OTS transcoder backends
/// `native` / `ffmpeg`).
pub const OTS_BACKEND_CATALOG: &[&str] = &["native", "ffmpeg", "quant_sim", "qpu_native"];

/// v2.4.0 — the closed set of backends a `quant` block may select via
/// `quant(backend: …)`. A strict SUBSET of [`OTS_BACKEND_CATALOG`]: `native` /
/// `ffmpeg` are OTS transcoder backends, NOT quantum backends, so they are
/// rejected in a `quant` header. `quant_sim` = the host CPU/GPU simulator;
/// `qpu_native` = a physical QPU coprocessor (D1/D9).
pub const QUANT_BACKEND_CATALOG: &[&str] = &["quant_sim", "qpu_native"];

/// v2.4.0 — the canonical algebraic-effect slug a `quant` block performs:
/// `ots:backend:<backend>` (e.g. `ots:backend:quant_sim`). This is the effect
/// injected into the closed `ots:backend` catalogue and propagated to a flow's
/// performed-effect set (`type_checker::flow_quant_effects`).
pub fn quant_effect_slug(backend: &str) -> String {
    format!("{OTS_BACKEND_EFFECT_SLUG}:{backend}")
}
