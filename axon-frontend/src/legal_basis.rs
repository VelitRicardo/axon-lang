//! Legal-basis catalogue — v1.4.0.
//!
//! A sensitive effect (one that reads or writes regulated data:
//! personal data under GDPR, ePHI under HIPAA, payment card data
//! under PCI-DSS, etc.) cannot be invoked without a compile-time
//! declaration of which legal basis authorises the processing.
//!
//! The catalogue is **closed**: every variant represents a specific
//! article / section of a real regulation, and adding one requires a
//! compiler patch + legal review — the same strict posture as the
//! trust-proof catalogue in [`crate::refinement`], and for the same
//! reason: regulators need an enumerable, reviewable list.
//!
//! # Coverage today
//!
//! - **GDPR Art. 6** — six lawful bases for processing personal data:
//!   consent, contract, legal obligation, vital interests, public
//!   task, legitimate interests.
//! - **GDPR Art. 9** — ten derogations for special-category data
//!   (health, ethnicity, biometrics, …).
//! - **CCPA section 1798.100** — right-to-know acknowledgement.
//! - **SOX section 404** — internal-controls attestation (financial
//!   reporting).
//! - **HIPAA section 164.502** — permitted uses + disclosures of protected
//!   health information (PHI).
//! - **GLBA section 501(b)** — safeguards rule (financial non-public
//!   personal information).
//! - **PCI-DSS v4.0 req 3** — stored cardholder data protection.
//!
//! # Slug format
//!
//! Slugs use `Regulation.Section[.Clause]` with dots as separators.
//! The slugs are what adopters write in their source:
//!
//! ```text
//! effects: <sensitive:health_data, legal:HIPAA.164_502>
//! ```

use std::fmt;

/// Canonical identifier of a single legal basis. Closed enum —
/// mirror of `axon.compiler.legal_basis.LegalBasis` in Python.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LegalBasis {
    // ── GDPR Art. 6 — lawful bases for processing personal data ──
    GdprArt6Consent,
    GdprArt6Contract,
    GdprArt6LegalObligation,
    GdprArt6VitalInterests,
    GdprArt6PublicTask,
    GdprArt6LegitimateInterests,

    // ── GDPR Art. 9 — derogations for special-category data ──
    GdprArt9ExplicitConsent,
    GdprArt9Employment,
    GdprArt9VitalInterests,
    GdprArt9NotForProfit,
    GdprArt9PublicData,
    GdprArt9LegalClaims,
    GdprArt9SubstantialPublicInterest,
    GdprArt9HealthcareProvision,
    GdprArt9PublicHealth,
    GdprArt9ArchivingResearch,

    // ── CCPA ──
    CcpaSec1798_100,

    // ── SOX ──
    SoxSec404,

    // ── HIPAA ──
    HipaaSec164_502,

    // ── GLBA ──
    GlbaSec501b,

    // ── PCI-DSS ──
    PciDssV4Req3,
}

impl LegalBasis {
    /// Every variant — used by checker diagnostics and parity
    /// harnesses. The order is stable so cross-language tests match.
    pub const ALL: &'static [LegalBasis] = &[
        LegalBasis::GdprArt6Consent,
        LegalBasis::GdprArt6Contract,
        LegalBasis::GdprArt6LegalObligation,
        LegalBasis::GdprArt6VitalInterests,
        LegalBasis::GdprArt6PublicTask,
        LegalBasis::GdprArt6LegitimateInterests,
        LegalBasis::GdprArt9ExplicitConsent,
        LegalBasis::GdprArt9Employment,
        LegalBasis::GdprArt9VitalInterests,
        LegalBasis::GdprArt9NotForProfit,
        LegalBasis::GdprArt9PublicData,
        LegalBasis::GdprArt9LegalClaims,
        LegalBasis::GdprArt9SubstantialPublicInterest,
        LegalBasis::GdprArt9HealthcareProvision,
        LegalBasis::GdprArt9PublicHealth,
        LegalBasis::GdprArt9ArchivingResearch,
        LegalBasis::CcpaSec1798_100,
        LegalBasis::SoxSec404,
        LegalBasis::HipaaSec164_502,
        LegalBasis::GlbaSec501b,
        LegalBasis::PciDssV4Req3,
    ];

    /// Stable slug written in source code.
    pub fn slug(self) -> &'static str {
        match self {
            LegalBasis::GdprArt6Consent => "GDPR.Art6.Consent",
            LegalBasis::GdprArt6Contract => "GDPR.Art6.Contract",
            LegalBasis::GdprArt6LegalObligation => "GDPR.Art6.LegalObligation",
            LegalBasis::GdprArt6VitalInterests => "GDPR.Art6.VitalInterests",
            LegalBasis::GdprArt6PublicTask => "GDPR.Art6.PublicTask",
            LegalBasis::GdprArt6LegitimateInterests => "GDPR.Art6.LegitimateInterests",
            LegalBasis::GdprArt9ExplicitConsent => "GDPR.Art9.ExplicitConsent",
            LegalBasis::GdprArt9Employment => "GDPR.Art9.Employment",
            LegalBasis::GdprArt9VitalInterests => "GDPR.Art9.VitalInterests",
            LegalBasis::GdprArt9NotForProfit => "GDPR.Art9.NotForProfit",
            LegalBasis::GdprArt9PublicData => "GDPR.Art9.PublicData",
            LegalBasis::GdprArt9LegalClaims => "GDPR.Art9.LegalClaims",
            LegalBasis::GdprArt9SubstantialPublicInterest => "GDPR.Art9.SubstantialPublicInterest",
            LegalBasis::GdprArt9HealthcareProvision => "GDPR.Art9.HealthcareProvision",
            LegalBasis::GdprArt9PublicHealth => "GDPR.Art9.PublicHealth",
            LegalBasis::GdprArt9ArchivingResearch => "GDPR.Art9.ArchivingResearch",
            LegalBasis::CcpaSec1798_100 => "CCPA.1798_100",
            LegalBasis::SoxSec404 => "SOX.404",
            LegalBasis::HipaaSec164_502 => "HIPAA.164_502",
            LegalBasis::GlbaSec501b => "GLBA.501b",
            LegalBasis::PciDssV4Req3 => "PCI_DSS.v4_Req3",
        }
    }

    /// Resolve a slug back to a variant. `None` for anything not in
    /// the catalogue — the checker uses this to emit a targeted
    /// "unknown legal basis" error.
    pub fn from_slug(slug: &str) -> Option<LegalBasis> {
        Self::ALL.iter().copied().find(|b| b.slug() == slug)
    }

    /// Regulation family — for grouping dashboards and reports.
    pub fn regulation(self) -> Regulation {
        use LegalBasis::*;
        match self {
            GdprArt6Consent
            | GdprArt6Contract
            | GdprArt6LegalObligation
            | GdprArt6VitalInterests
            | GdprArt6PublicTask
            | GdprArt6LegitimateInterests
            | GdprArt9ExplicitConsent
            | GdprArt9Employment
            | GdprArt9VitalInterests
            | GdprArt9NotForProfit
            | GdprArt9PublicData
            | GdprArt9LegalClaims
            | GdprArt9SubstantialPublicInterest
            | GdprArt9HealthcareProvision
            | GdprArt9PublicHealth
            | GdprArt9ArchivingResearch => Regulation::Gdpr,
            CcpaSec1798_100 => Regulation::Ccpa,
            SoxSec404 => Regulation::Sox,
            HipaaSec164_502 => Regulation::Hipaa,
            GlbaSec501b => Regulation::Glba,
            PciDssV4Req3 => Regulation::PciDss,
        }
    }
}

impl fmt::Display for LegalBasis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Regulation {
    Gdpr,
    Ccpa,
    Sox,
    Hipaa,
    Glba,
    PciDss,
}

impl Regulation {
    pub fn slug(self) -> &'static str {
        match self {
            Regulation::Gdpr => "GDPR",
            Regulation::Ccpa => "CCPA",
            Regulation::Sox => "SOX",
            Regulation::Hipaa => "HIPAA",
            Regulation::Glba => "GLBA",
            Regulation::PciDss => "PCI_DSS",
        }
    }
}

/// Slug catalogue used by the checker's "unknown basis" diagnostic.
pub const LEGAL_BASIS_CATALOG: &[&str] = &[
    "GDPR.Art6.Consent",
    "GDPR.Art6.Contract",
    "GDPR.Art6.LegalObligation",
    "GDPR.Art6.VitalInterests",
    "GDPR.Art6.PublicTask",
    "GDPR.Art6.LegitimateInterests",
    "GDPR.Art9.ExplicitConsent",
    "GDPR.Art9.Employment",
    "GDPR.Art9.VitalInterests",
    "GDPR.Art9.NotForProfit",
    "GDPR.Art9.PublicData",
    "GDPR.Art9.LegalClaims",
    "GDPR.Art9.SubstantialPublicInterest",
    "GDPR.Art9.HealthcareProvision",
    "GDPR.Art9.PublicHealth",
    "GDPR.Art9.ArchivingResearch",
    "CCPA.1798_100",
    "SOX.404",
    "HIPAA.164_502",
    "GLBA.501b",
    "PCI_DSS.v4_Req3",
];

// ── Effect slugs used in source-level `effects: <...>` rows ─────────

/// Base effect slug for "this effect touches regulated data". A
/// tool declaring `sensitive:<category>` commits to carrying a
/// `legal:<basis>` alongside.
pub const SENSITIVE_EFFECT_SLUG: &str = "sensitive";

/// Base effect slug for "this call is authorised under <basis>".
pub const LEGAL_EFFECT_SLUG: &str = "legal";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_roundtrip_covers_closed_catalog() {
        for basis in LegalBasis::ALL {
            let slug = basis.slug();
            assert_eq!(Some(*basis), LegalBasis::from_slug(slug));
            assert!(LEGAL_BASIS_CATALOG.contains(&slug));
        }
        assert_eq!(
            LegalBasis::ALL.len(),
            LEGAL_BASIS_CATALOG.len(),
            "catalogue drift — Rust enum + const slice must match"
        );
    }

    #[test]
    fn unknown_slug_rejected() {
        assert!(LegalBasis::from_slug("GDPR.Art99.Made_Up").is_none());
        assert!(LegalBasis::from_slug("HIPAA").is_none()); // too short
        assert!(LegalBasis::from_slug("").is_none());
    }

    #[test]
    fn regulation_family_covers_every_variant() {
        use std::collections::HashSet;
        let mut seen: HashSet<Regulation> = HashSet::new();
        for b in LegalBasis::ALL {
            seen.insert(b.regulation());
        }
        // Every regulation must have at least one basis.
        for reg in [
            Regulation::Gdpr,
            Regulation::Ccpa,
            Regulation::Sox,
            Regulation::Hipaa,
            Regulation::Glba,
            Regulation::PciDss,
        ] {
            assert!(seen.contains(&reg), "missing regulation {reg:?}");
        }
    }

    #[test]
    fn display_format_matches_slug() {
        assert_eq!(format!("{}", LegalBasis::HipaaSec164_502), "HIPAA.164_502");
    }
}
