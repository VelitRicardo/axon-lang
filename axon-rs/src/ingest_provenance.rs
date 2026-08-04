//! §Fase 118.b — the provenance class of an ingested value, in a leaf module.
//!
//! **Why this module exists.** [`IngestProvenance`] is an EPISTEMIC concept —
//! *was this derived from bytes, or believed from pixels?* — and it carries the
//! hard ceiling that keeps a model's guess from ever reaching `know`. It is not
//! an OOXML concept. It lived in `ooxml_read.rs` only because §Fase 100.d is
//! where it was first needed.
//!
//! That placement made `extraction.rs` — the §101 IDP-E extraction contract,
//! which has nothing to do with Office file formats — depend on the OOXML
//! reader, and therefore on `zip`, purely to name an enum with two variants.
//! Gating the OOXML surface behind the `documents` feature surfaced it
//! immediately: the §101 extraction contract stopped compiling for want of a
//! type it uses to say "this fact is Inferred".
//!
//! This is the second instance of the same shape in §118 (the first was
//! `AXON_VERSION` living inside the flow executor, §118.a.1): a general concept
//! parked in the specific module that first needed it, quietly making everything
//! downstream depend on that module's dependencies. Worth looking for a third.
//!
//! Like `version.rs`, this module must never acquire a dependency.
//! `ooxml_read` re-exports the type so every existing call site keeps resolving.

/// §Fase 100.d — the provenance class of an ingested value, carried into the
/// type system. **Parsed**: derived deterministically from the bytes; faithful;
/// born `Untrusted` but elevatable by a shield. **Inferred**: produced by a
/// model from pixels (OCR / vision); a belief about an image, with a hard
/// ceiling of `believe` — never `know`. §100 constructs ONLY `Parsed`; the
/// `Inferred` variant exists so §101's producers land into a lattice that
/// already refuses to over-trust them (D100.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestProvenance {
    /// A fact about the file (re-derivable). Elevatable by a shield.
    Parsed,
    /// A belief about pixels (OCR/vision). Ceiling of `believe`. NO producer in
    /// §100 — inhabited only from §101.
    Inferred,
}

impl IngestProvenance {
    pub fn as_str(self) -> &'static str {
        match self {
            IngestProvenance::Parsed => "parsed",
            IngestProvenance::Inferred => "inferred",
        }
    }
    /// The hard epistemic ceiling of this class. `Parsed` may reach `know` (via
    /// a shield); `Inferred` is capped at `believe` — no shield, no `know` block
    /// may raise it (D100.1).
    pub fn epistemic_ceiling(self) -> &'static str {
        match self {
            IngestProvenance::Parsed => "know",
            IngestProvenance::Inferred => "believe",
        }
    }
}
