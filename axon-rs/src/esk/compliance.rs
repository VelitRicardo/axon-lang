//! AXON Runtime — Regulatory Compliance Registry (§Fase 6.1)
//!
//! Direct port of `axon/runtime/esk/compliance.py`.
//!
//! Canonical vocabulary for the ESK κ (regulatory class) annotation.
//! Every class corresponds to a real-world regulation; typos are
//! compile-time errors (§6.1 enforcement).

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use serde::Serialize;

/// Metadata for one regulatory framework.
#[derive(Debug, Clone, Serialize)]
pub struct RegulatoryClass {
    pub name: String,
    pub title: String,
    pub jurisdiction: String,
    pub sector: String,
    pub description: String,
}

impl RegulatoryClass {
    /// Reflexive coverage — a class covers itself only. Cross-framework
    /// overlap is an explicit policy decision, not implicit.
    pub fn covers_class(&self, other: &RegulatoryClass) -> bool {
        self.name == other.name
    }

    fn new(
        name: &str,
        title: &str,
        jurisdiction: &str,
        sector: &str,
        description: &str,
    ) -> Self {
        RegulatoryClass {
            name: name.into(),
            title: title.into(),
            jurisdiction: jurisdiction.into(),
            sector: sector.into(),
            description: description.into(),
        }
    }
}

/// Build the canonical registry — must match Python `REGISTRY` exactly.
pub fn registry() -> HashMap<String, RegulatoryClass> {
    let entries = [
        RegulatoryClass::new(
            "HIPAA",
            "Health Insurance Portability and Accountability Act",
            "US",
            "healthcare",
            "PHI confidentiality, integrity, and availability for US healthcare providers.",
        ),
        RegulatoryClass::new(
            "PCI_DSS",
            "Payment Card Industry Data Security Standard",
            "Global",
            "financial",
            "Cardholder data protection for merchants and payment processors.",
        ),
        RegulatoryClass::new(
            "GDPR",
            "General Data Protection Regulation",
            "EU",
            "cross-sector",
            "Personal data protection for EU residents, with right-to-erasure.",
        ),
        RegulatoryClass::new(
            "SOX",
            "Sarbanes-Oxley Act",
            "US",
            "financial",
            "Financial reporting integrity for public companies.",
        ),
        RegulatoryClass::new(
            "FINRA",
            "Financial Industry Regulatory Authority",
            "US",
            "financial",
            "Broker-dealer oversight — communications, record retention, surveillance.",
        ),
        RegulatoryClass::new(
            "ISO27001",
            "ISO/IEC 27001",
            "Global",
            "cross-sector",
            "Information security management system certification.",
        ),
        RegulatoryClass::new(
            "SOC2",
            "SOC 2 Type II",
            "Global",
            "cross-sector",
            "Trust Services Criteria — security, availability, confidentiality.",
        ),
        RegulatoryClass::new(
            "FISMA",
            "Federal Information Security Management Act",
            "US",
            "government",
            "US federal government information security baseline.",
        ),
        RegulatoryClass::new(
            "GxP",
            "Good x Practice",
            "Global",
            "pharma",
            "Quality guidelines for pharma / clinical / manufacturing (GLP/GMP/GCP).",
        ),
        RegulatoryClass::new(
            "CCPA",
            "California Consumer Privacy Act",
            "US-CA",
            "cross-sector",
            "Consumer data rights for California residents.",
        ),
        RegulatoryClass::new(
            "NIST_800_53",
            "NIST SP 800-53",
            "US",
            "government",
            "Security and privacy controls catalog for US federal systems.",
        ),
    ];
    entries.into_iter().map(|c| (c.name.clone(), c)).collect()
}

/// §Fase 123 — membership now DELEGATES to the frontend's Κ.
///
/// The registry below still owns what each class MEANS — title, jurisdiction,
/// sector, description — because that metadata exists to build audit dossiers,
/// which is a runtime concern. But *which labels exist* is a compile-time
/// question, and it had ended up here, downstream of the type checker that
/// needed it: `axon-frontend` depends on `serde` and nothing else, so the
/// catalog was unreachable from the only place a compile-time law can live.
/// That is the whole reason `compliance:` was a free-string field while
/// `effects:` beside it was closed.
///
/// So the vocabulary moved up to [`axon_frontend::compliance`] and this
/// function reads it. One source of truth for membership, one for meaning, and
/// `registry_matches_the_frontend_vocabulary` below proves they describe the
/// same eleven classes — rather than two lists that agree until one is edited.
pub fn is_known(label: &str) -> bool {
    axon_frontend::compliance::is_known(label)
}

pub fn get_class(label: &str) -> Option<RegulatoryClass> {
    registry().get(label).cloned()
}

/// Return the MISSING classes — i.e. `required \ provided`.
pub fn covers<I, J, S>(shield_compliance: I, required: J) -> HashSet<String>
where
    I: IntoIterator<Item = S>,
    J: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let provided: HashSet<String> = shield_compliance
        .into_iter()
        .map(|s| s.as_ref().to_string())
        .collect();
    let needed: HashSet<String> = required
        .into_iter()
        .map(|s| s.as_ref().to_string())
        .collect();
    needed.difference(&provided).cloned().collect()
}

/// Return the set of sectors the labels span.
pub fn classify_sector<I, S>(labels: I) -> HashSet<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let reg = registry();
    labels
        .into_iter()
        .filter_map(|l| reg.get(l.as_ref()).map(|c| c.sector.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §Fase 123 — the two halves describe the SAME eleven classes.
    ///
    /// Membership lives in the frontend (a compiler needs it); meaning lives
    /// here (a dossier needs it). That split is only safe while the sets are
    /// identical — a class the frontend accepts but this registry cannot
    /// describe would produce a dossier silently missing an entry, and a class
    /// described here but rejected upstream could never be written by any
    /// program.
    ///
    /// §122.e is why this test exists rather than a comment: a compatibility
    /// window written in three places had two of them updated and the third
    /// missed, and only the suite caught it. Two places is better than three,
    /// and two places compared is better than two places trusted.
    #[test]
    fn registry_matches_the_frontend_vocabulary() {
        let mut described: Vec<String> = registry().keys().cloned().collect();
        described.sort();
        let mut vocabulary: Vec<String> = axon_frontend::compliance::REGULATORY_CLASSES
            .iter()
            .map(|s| s.to_string())
            .collect();
        vocabulary.sort();
        assert_eq!(
            described, vocabulary,
            "the ESK registry (what each class MEANS) and the frontend vocabulary (which \
             classes EXIST) have diverged. Adding a regulatory framework means adding it to \
             BOTH — the compiler must accept the label and the dossier must be able to \
             describe it."
        );
    }

    #[test]
    fn registry_has_all_expected_classes() {
        let reg = registry();
        for name in [
            "HIPAA", "PCI_DSS", "GDPR", "SOX", "FINRA", "ISO27001", "SOC2",
            "FISMA", "GxP", "CCPA", "NIST_800_53",
        ] {
            assert!(reg.contains_key(name), "missing {name}");
        }
    }

    #[test]
    fn is_known_accepts_canonical_names_only() {
        assert!(is_known("HIPAA"));
        assert!(!is_known("hipaa")); // case-sensitive
        assert!(!is_known("INVENTED"));
    }

    #[test]
    fn covers_returns_missing_classes() {
        let missing = covers(["HIPAA", "SOC2"], ["HIPAA", "GDPR", "SOC2"]);
        assert_eq!(missing.len(), 1);
        assert!(missing.contains("GDPR"));
    }

    #[test]
    fn classify_sector_aggregates() {
        let sectors = classify_sector(["HIPAA", "PCI_DSS", "GxP"]);
        assert!(sectors.contains("healthcare"));
        assert!(sectors.contains("financial"));
        assert!(sectors.contains("pharma"));
    }

    #[test]
    fn reflexive_covers_class() {
        let hipaa = get_class("HIPAA").unwrap();
        assert!(hipaa.covers_class(&hipaa));
        let gdpr = get_class("GDPR").unwrap();
        assert!(!hipaa.covers_class(&gdpr));
    }
}
