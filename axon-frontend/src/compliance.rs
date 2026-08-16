//! §Fase 123 — **the closed regulatory vocabulary Κ.**
//!
//! # Why this module exists in the FRONTEND
//!
//! The ESK paper (§6.1, *Regulatory Type Theory*) states the rule plainly:
//!
//! > *"κ es un subconjunto del registro canónico Κ = {HIPAA, PCI_DSS, GDPR,
//! > SOX, FINRA, ISO27001, SOC2, FISMA, GxP, CCPA, NIST_800_53}. Cualquier
//! > etiqueta fuera de Κ es **compile-time error** (typos como "HIPPA"
//! > rechazados)."*
//!
//! That promise was true — in Python. The paper cites
//! `TestComplianceCoverage.test_unknown_regulatory_class_rejected`, a test from
//! the retired interpreter, and the rule was lost in the Rust rewrite. Not by a
//! decision: the canonical registry landed in `axon-rs::esk::compliance`, and
//! `axon-frontend` depends on `serde` and nothing else. **The catalog ended up
//! downstream of the type checker that needed it**, so the law could not be
//! written where it belonged, and `compliance:` quietly became a free-string
//! field while `effects:` beside it stayed a closed catalog.
//!
//! Measured 2026-08-14, and recorded in `advertised.rs` before this fase
//! existed: `compliance: [NOT_A_FRAMEWORK]` on an `axonendpoint` compiled
//! clean. The values an adopter writes there are `PCI_DSS`, `SOX`, `HIPAA` —
//! read by a regulated reader as an assertion.
//!
//! # What lives here and what does not
//!
//! Only the **membership question**: which labels exist. That is all a compiler
//! needs, and it is pure data.
//!
//! The rich metadata for each class — title, jurisdiction, sector, description
//! — stays in `axon-rs::esk::compliance`, because it exists to build audit
//! dossiers, which is a runtime concern. That module now derives its keys from
//! [`REGULATORY_CLASSES`](crate::compliance::REGULATORY_CLASSES) and a test
//! pins the two in agreement, so there is one
//! source of truth for *what exists* and one place for *what it means*.
//!
//! This is deliberately NOT a second copy of the catalog. §122.e paid for a law
//! written three times: two copies were updated, the third was missed, and the
//! workspace suite caught it — after the first two attempts had already shipped
//! the drift.

/// The canonical regulatory classes — Κ.
///
/// Reflexive by construction: a class covers itself and nothing else.
/// Cross-framework overlap (does SOC2 imply ISO27001?) is an explicit policy
/// decision a regulator makes, not something a compiler may infer.
///
/// **Adding an entry is a deliberate act.** A new framework here becomes a
/// label every adopter can assert, and the assertion is what a regulated
/// reader trusts — so it belongs in a fase with a paper behind it, not in a
/// convenience commit.
pub const REGULATORY_CLASSES: &[&str] = &[
    "HIPAA",
    "PCI_DSS",
    "GDPR",
    "SOX",
    "FINRA",
    "ISO27001",
    "SOC2",
    "FISMA",
    "GxP",
    "CCPA",
    "NIST_800_53",
    // §Fase 124 (D124.1) — the four LATAM jurisdictions the product serves.
    //
    // They enter Κ on the same footing as every class above: declarable, and a
    // participant in `axon-T957`'s coverage difference. That is the whole
    // criterion, and it is worth stating precisely because the first draft of
    // this decision justified the set by saying the excluded jurisdictions
    // "impose no real restriction" — which does not hold. Ley 25.326
    // (Argentina) and Ley 81 (Panamá) would impose exactly the same restriction
    // as these four.
    //
    // The real criterion is PRODUCT PRIORITY: each of these four has a named
    // vertical in the README and a mechanism the language already provides
    // (NOM-151 → `axonstore` sealing; LFPDPPP/LGPD/Ley 1581 → `shield`
    // redaction). That is a good reason. Writing the other one down would have
    // been a justification that does not survive a reading, which is the
    // species of claim this whole line of work exists to remove.
    "NOM151",
    "LFPDPPP",
    "LGPD",
    "LEY1581",
];

/// Is `label` a member of Κ?
///
/// **Case-SENSITIVE, deliberately.** `hipaa` is not `HIPAA`. The canonical
/// spelling is the one that appears in the regulation, and a compliance
/// annotation that renders differently from the framework it names is a
/// different string to every downstream consumer that groups by it — the audit
/// dossier, the SBOM filter, the evidence packager. Accepting case variants
/// would make `[HIPAA]` and `[hipaa]` two classes that look like one.
pub fn is_known(label: &str) -> bool {
    REGULATORY_CLASSES.contains(&label)
}

/// The classes in `declared` that are NOT in Κ, in declaration order.
///
/// Returns the offenders rather than a bool so a diagnostic can name what the
/// author actually wrote — `axon-T1214` quotes the typo back, which is the
/// difference between "invalid compliance class" and "`HIPPA` is not a
/// regulatory class; did you mean `HIPAA`?".
pub fn unknown_classes<'a>(declared: &'a [String]) -> Vec<&'a str> {
    declared
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !is_known(s))
        .collect()
}

/// The member of Κ within edit distance 1 of `label`, if exactly one exists.
///
/// A typo in a regulatory class is the failure this catalog exists to catch,
/// and the two that matter — `HIPPA` for `HIPAA`, `PCI-DSS` for `PCI_DSS` —
/// are both one edit away. Suggesting is cheap; suggesting the WRONG framework
/// is not, so an ambiguous match suggests nothing.
pub fn nearest_class(label: &str) -> Option<&'static str> {
    let mut hit: Option<&'static str> = None;
    for candidate in REGULATORY_CLASSES {
        if edit_distance_at_most_1(label, candidate) {
            if hit.is_some() {
                return None; // ambiguous — say nothing rather than guess
            }
            hit = Some(candidate);
        }
    }
    hit
}

/// True when `a` and `b` differ by at most one insertion, deletion or
/// substitution. Bounded on purpose — a full Levenshtein over an 11-entry
/// catalog would suggest `SOX` for `SOC2`.
fn edit_distance_at_most_1(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a == b {
        return true;
    }
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    if long.len() - short.len() > 1 {
        return false;
    }
    let mut i = 0;
    let mut j = 0;
    let mut edited = false;
    while i < long.len() && j < short.len() {
        if long[i] == short[j] {
            i += 1;
            j += 1;
            continue;
        }
        if edited {
            return false;
        }
        edited = true;
        if long.len() == short.len() {
            i += 1;
            j += 1;
        } else {
            i += 1;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_is_the_fifteen_the_paper_names() {
        assert_eq!(
            REGULATORY_CLASSES.len(),
            15,
            "Κ is the canonical registry from the ESK paper §6.1, extended in §124 with the \
             four LATAM jurisdictions. Changing its size is a decision about what an adopter \
             may assert, not a refactor — and the paper's own Κ is pinned against this list \
             by `fase124_the_paper_matches_the_compiler`."
        );
        for class in [
            "HIPAA", "PCI_DSS", "GDPR", "SOX", "FINRA", "ISO27001", "SOC2", "FISMA", "GxP",
            "CCPA", "NIST_800_53", "NOM151", "LFPDPPP", "LGPD", "LEY1581",
        ] {
            assert!(is_known(class), "{class} must be in Κ");
        }
    }

    #[test]
    fn membership_is_case_sensitive() {
        assert!(is_known("HIPAA"));
        assert!(!is_known("hipaa"), "case variants are different strings to every consumer that groups by this label");
        assert!(!is_known("Hipaa"));
    }

    #[test]
    fn a_typo_is_not_a_class_and_gets_a_suggestion() {
        assert!(!is_known("HIPPA"));
        assert_eq!(nearest_class("HIPPA"), Some("HIPAA"));
        assert_eq!(nearest_class("PCI-DSS"), Some("PCI_DSS"));
    }

    #[test]
    fn a_word_that_is_not_a_framework_suggests_nothing() {
        assert!(!is_known("NOT_A_FRAMEWORK"));
        assert_eq!(
            nearest_class("NOT_A_FRAMEWORK"),
            None,
            "a suggestion must be a near miss, never the closest of eleven unrelated names"
        );
    }

    #[test]
    fn unknown_classes_reports_offenders_in_order() {
        let declared = vec!["HIPAA".to_string(), "HIPPA".to_string(), "SOC2".to_string(), "NOPE".to_string()];
        assert_eq!(unknown_classes(&declared), vec!["HIPPA", "NOPE"]);
    }

    #[test]
    fn every_class_is_known_and_suggests_itself() {
        for class in REGULATORY_CLASSES {
            assert!(is_known(class));
            assert_eq!(
                nearest_class(class),
                Some(*class),
                "a valid class must resolve to itself — if two members of Κ are one edit apart, \
                 `nearest_class` goes ambiguous and the diagnostics silently stop suggesting"
            );
        }
    }
}
