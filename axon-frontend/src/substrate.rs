//! v2.83.0 — the `fabric` substrate judgment: provider ↔ region ↔
//! jurisdiction, shared verbatim by the compiler and the runtime.
//!
//! # What `fabric` is, and what it is not
//!
//! From its knowledge doc (`src/axon-emcp/knowledge/primitives/fabric.md`) —
//! `fabric` has no paper; it was born from the implementation, and the doc is
//! its specification:
//!
//! > **Not infrastructure-as-code.** AXON fabric declarations do NOT provision
//! > infrastructure — they *describe* what the runtime expects to find.
//! > Provisioning happens upstream (Terraform, Pulumi, manual ops). The fabric
//! > makes expectations **typed + auditable**.
//!
//! That sentence is the whole scope of this step, and it is why v2.83.0 is
//! not "make fabric provision things". v2.67.0's finding was that
//! `provider`/`region`/`zones` were *consumed by nothing*: the fabric was a
//! label. Making it real means making those three fields **decide something**
//! — which, for a primitive whose job is typed expectation, means
//!
//! 1. a declaration that cannot be true is REFUSED at compile time
//!    (`axon-E041`: an Azure region under `provider: aws` names a substrate
//!    that does not exist), and
//! 2. a compliance obligation that the substrate cannot satisfy is REFUSED
//!    (`axon-E042`: the doc's own example — *"a GDPR-tagged manifest deployed
//!    to a non-EU region is rejected"*), and
//! 3. what actually runs carries `(provider, region)` so cross-jurisdiction
//!    data movement is auditable — the doc's "compliance propagation".
//!
//! # Why the catalog is closed for the providers it knows, and open otherwise
//!
//! The doc says the provider list is *"open at the parser level — the runtime
//! decides which providers are deployable"*. So this module does **not** reject
//! unknown providers. It validates the region only for providers whose region
//! shape is a matter of public fact, and for anything else
//! [`ProviderShape::Unvalidated`] says so out loud rather than inventing a
//! rule. A check that pretended to know every cloud's topology would be the
//! fabricated catalog this codebase has now caught five times.

use std::fmt;

/// What is known about a provider's region shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderShape {
    /// The provider's region naming is a matter of public fact and is
    /// validated. Carries a human description of the shape for diagnostics.
    Validated { example: &'static str },
    /// The provider is accepted (the catalog is open per the spec) but its
    /// region shape is not something this compiler claims to know. **No region
    /// check runs** — and callers can see that it did not.
    Unvalidated,
}

/// A jurisdiction a region sits in, for compliance cross-validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jurisdiction {
    /// The European Union / EEA — the jurisdiction GDPR binds.
    Eu,
    /// Outside the EU/EEA.
    NonEu,
    /// Not determinable from the declaration (unvalidated provider, or a
    /// region this catalog does not map). **Never silently treated as
    /// compliant**: see [`compliance_violation`].
    Unknown,
}

/// The providers whose region shape is public fact. Each entry is
/// `(provider slug, region-prefix set, example)`.
///
/// The prefixes are geographic stems, not full region lists: a new AWS region
/// appears every few months and a hard list would reject valid deployments
/// within a quarter of shipping. The stems are stable, and the check they
/// support is the one the doc names — catching an *Azure* region under AWS,
/// not policing the exact digit.
const PROVIDER_REGION_STEMS: &[(&str, &[&str], &str)] = &[
    // AWS: `<geo>-<direction>-<n>` — us-east-1, eu-west-2, sa-east-1.
    (
        "aws",
        &["us-", "eu-", "ap-", "sa-", "ca-", "me-", "af-", "il-", "cn-", "us-gov-"],
        "us-east-1",
    ),
    // GCP: `<geo>-<direction><n>` — us-central1, europe-west1, southamerica-east1.
    (
        "gcp",
        &[
            "us-", "europe-", "asia-", "australia-", "southamerica-",
            "northamerica-", "me-", "africa-",
        ],
        "us-central1",
    ),
    // Azure: one word — eastus, westeurope, brazilsouth. Distinguished from
    // AWS/GCP precisely by carrying NO hyphen before the first digit.
    ("azure", &[], "eastus"),
];

/// EU/EEA region stems, per provider naming. Factual, not policy: these are
/// the geographic stems each provider uses for its European regions.
const EU_REGION_STEMS: &[&str] = &[
    // AWS + GCP style.
    "eu-", "europe-",
    // Azure style (single word).
    "westeurope", "northeurope", "germanywestcentral", "germanynorth",
    "francecentral", "francesouth", "norwayeast", "norwaywest",
    "switzerlandnorth", "switzerlandwest", "swedencentral", "polandcentral",
    "italynorth", "spaincentral",
];

/// Look up a provider's shape. Unknown providers are [`ProviderShape::Unvalidated`]
/// — accepted, not validated, and visibly so.
pub fn provider_shape(provider: &str) -> ProviderShape {
    let p = provider.trim().to_ascii_lowercase();
    match PROVIDER_REGION_STEMS.iter().find(|(slug, _, _)| *slug == p) {
        Some((_, _, example)) => ProviderShape::Validated { example },
        None => ProviderShape::Unvalidated,
    }
}

/// Whether `region` is a plausible region for `provider`.
///
/// Returns `None` when no judgment is made (unvalidated provider, or an empty
/// region — `region:` is optional per the grammar). `Some(true/false)` is a
/// judgment the caller may act on.
pub fn region_matches_provider(provider: &str, region: &str) -> Option<bool> {
    let p = provider.trim().to_ascii_lowercase();
    let r = region.trim().to_ascii_lowercase();
    if r.is_empty() {
        return None;
    }
    let (_, stems, _) = PROVIDER_REGION_STEMS.iter().find(|(slug, _, _)| *slug == p)?;
    if p == "azure" {
        // Azure regions are a single lowercase word: no hyphens. That is
        // exactly what separates `eastus` from `us-east-1` — the doc's own
        // mismatch example, in one character class.
        return Some(!r.contains('-') && r.chars().all(|c| c.is_ascii_alphanumeric()));
    }
    Some(stems.iter().any(|s| r.starts_with(s)))
}

/// The jurisdiction a `(provider, region)` sits in.
pub fn jurisdiction_of(provider: &str, region: &str) -> Jurisdiction {
    let r = region.trim().to_ascii_lowercase();
    if r.is_empty() || provider_shape(provider) == ProviderShape::Unvalidated {
        return Jurisdiction::Unknown;
    }
    if EU_REGION_STEMS.iter().any(|s| r.starts_with(s)) {
        Jurisdiction::Eu
    } else {
        Jurisdiction::NonEu
    }
}

/// Compliance tags that bind a jurisdiction, and the jurisdiction each binds.
///
/// Closed by construction: a tag is listed here only when the geographic
/// obligation is a matter of law, not of opinion. Tags outside this list carry
/// no jurisdictional constraint and are not checked here — which is honest,
/// because inventing a geography for `soc2` would be a fabricated rule.
/// v4.0.0 — **every tag here must be a member of Κ**, or the rule is dead.
///
/// These names are matched case-insensitively against a `manifest`'s
/// `compliance:` list. Since v4.0.0 that list is a closed catalogue
/// (`axon-T1214`), so a tag absent from Κ can never appear in any program that
/// compiles — the jurisdiction constraint keyed on it is unreachable, and
/// unreachable-but-present is indistinguishable from enforced when reading the
/// source.
///
/// `eu_ai_act` was exactly that: listed here, absent from Κ, refused by the
/// type checker on the only declaration that feeds this function. Removed in
/// v4.0.0 rather than promoted, because the design decision fixed Κ's growth at the four LATAM
/// jurisdictions and the EU AI Act's coverage in AXON is a RUNTIME one (`trail`,
/// `mandate`, `reason`) rather than a data-classification one. Adding a κ class
/// to make a dead rule live would have been the tail wagging the dog.
///
/// `every_jurisdiction_tag_is_a_real_regulatory_class` is what keeps this
/// honest; entries are lowercase by this module's own convention and Κ is
/// canonical-uppercase, so it compares case-insensitively.
const JURISDICTION_BOUND_TAGS: &[(&str, Jurisdiction)] = &[("gdpr", Jurisdiction::Eu)];

/// The compliance violation a `(tag, provider, region)` triple represents, if
/// any.
///
/// `Unknown` jurisdiction is **not** a pass: a manifest that claims GDPR on a
/// substrate whose jurisdiction cannot be determined has not shown that the
/// obligation holds, and the whole point of the fabric is that the expectation
/// is typed. It is reported distinctly from an outright violation so the
/// diagnostic can say which one happened.
pub fn compliance_violation(
    tag: &str,
    provider: &str,
    region: &str,
) -> Option<ComplianceViolation> {
    let t = tag.trim().to_ascii_lowercase();
    let (_, required) = JURISDICTION_BOUND_TAGS
        .iter()
        .find(|(name, _)| *name == t)?;
    let actual = jurisdiction_of(provider, region);
    if actual == *required {
        return None;
    }
    Some(ComplianceViolation {
        tag: t,
        required: *required,
        actual,
        provider: provider.to_string(),
        region: region.to_string(),
    })
}

/// A compliance obligation the declared substrate does not satisfy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplianceViolation {
    pub tag: String,
    pub required: Jurisdiction,
    pub actual: Jurisdiction,
    pub provider: String,
    pub region: String,
}

impl fmt::Display for ComplianceViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.actual {
            Jurisdiction::Unknown => write!(
                f,
                "declares compliance '{}', which binds data to the {:?} \
                 jurisdiction, but the substrate's jurisdiction cannot be \
                 determined from provider '{}' region '{}'. An obligation that \
                 cannot be shown to hold is not shown to hold — declare a \
                 region this compiler can place, or drop the tag",
                self.tag, self.required, self.provider, self.region
            ),
            _ => write!(
                f,
                "declares compliance '{}', which binds data to the {:?} \
                 jurisdiction, but provider '{}' region '{}' is {:?}. The \
                 deployment would move regulated data out of the jurisdiction \
                 the tag promises to keep it in",
                self.tag, self.required, self.provider, self.region, self.actual
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// v4.0.0 — a jurisdiction rule must be keyed on a class a program can
    /// legally declare.
    ///
    /// `compliance_violation` is reached from `axon-E042` with the tags of a
    /// `manifest`'s `compliance:` list, and since v4.0.0 that list is closed by
    /// `axon-T1214`. A tag here that is not in Κ therefore describes a rule no
    /// program can ever trigger — present in the source, enforcing nothing, and
    /// reading exactly like a rule that works.
    ///
    /// That is precisely the defect class v2.67.0 named *motor real, cable
    /// muerto*, at the granularity of a single table row. `eu_ai_act` was the
    /// instance; this test is why there will not be another.
    #[test]
    fn every_jurisdiction_tag_is_a_real_regulatory_class() {
        for (tag, _) in JURISDICTION_BOUND_TAGS {
            let matches_kappa = crate::compliance::REGULATORY_CLASSES
                .iter()
                .any(|class| class.eq_ignore_ascii_case(tag));
            assert!(
                matches_kappa,
                "jurisdiction tag `{tag}` is not a member of Κ ({:?}), so no program that \
                 compiles can carry it and this rule can never fire. Either add the class to \
                 `compliance::REGULATORY_CLASSES` — a product decision about what an adopter \
                 may assert — or delete the row.",
                crate::compliance::REGULATORY_CLASSES
            );
        }
    }

    #[test]
    fn the_docs_own_mismatch_example_is_caught() {
        // fabric.md: "Provider mismatches (`provider: aws  region: \"eastus\"`)
        // are rejected with a structured `axon-E041 region/provider mismatch`".
        assert_eq!(region_matches_provider("aws", "eastus"), Some(false));
        assert_eq!(region_matches_provider("azure", "eastus"), Some(true));
    }

    #[test]
    fn each_validated_provider_accepts_its_own_regions() {
        for (provider, region) in [
            ("aws", "us-east-1"),
            ("aws", "eu-west-2"),
            ("aws", "sa-east-1"),
            ("gcp", "us-central1"),
            ("gcp", "europe-west1"),
            ("gcp", "southamerica-east1"),
            ("azure", "eastus"),
            ("azure", "westeurope"),
            ("azure", "brazilsouth"),
        ] {
            assert_eq!(
                region_matches_provider(provider, region),
                Some(true),
                "{provider} should accept {region}"
            );
        }
    }

    #[test]
    fn cross_provider_regions_are_rejected_in_both_directions() {
        assert_eq!(region_matches_provider("azure", "us-east-1"), Some(false));
        assert_eq!(region_matches_provider("aws", "westeurope"), Some(false));
        assert_eq!(region_matches_provider("gcp", "eastus"), Some(false));
    }

    #[test]
    fn an_unvalidated_provider_makes_no_region_judgment_and_says_so() {
        // The spec keeps the provider catalog OPEN. Claiming to know every
        // cloud's topology would be a fabricated rule.
        assert_eq!(provider_shape("onprem"), ProviderShape::Unvalidated);
        assert_eq!(provider_shape("kubernetes"), ProviderShape::Unvalidated);
        assert_eq!(region_matches_provider("onprem", "rack-7"), None);
    }

    #[test]
    fn an_absent_region_makes_no_judgment_because_region_is_optional() {
        assert_eq!(region_matches_provider("aws", ""), None);
        assert_eq!(region_matches_provider("aws", "   "), None);
    }

    #[test]
    fn jurisdiction_places_eu_regions_across_all_three_naming_styles() {
        assert_eq!(jurisdiction_of("aws", "eu-west-1"), Jurisdiction::Eu);
        assert_eq!(jurisdiction_of("gcp", "europe-west4"), Jurisdiction::Eu);
        assert_eq!(jurisdiction_of("azure", "westeurope"), Jurisdiction::Eu);
        assert_eq!(jurisdiction_of("azure", "germanywestcentral"), Jurisdiction::Eu);
    }

    #[test]
    fn jurisdiction_places_non_eu_regions_and_admits_when_it_cannot() {
        assert_eq!(jurisdiction_of("aws", "us-east-1"), Jurisdiction::NonEu);
        assert_eq!(jurisdiction_of("gcp", "asia-northeast1"), Jurisdiction::NonEu);
        assert_eq!(jurisdiction_of("azure", "eastus"), Jurisdiction::NonEu);
        assert_eq!(jurisdiction_of("onprem", "rack-7"), Jurisdiction::Unknown);
        assert_eq!(jurisdiction_of("aws", ""), Jurisdiction::Unknown);
    }

    #[test]
    fn the_docs_own_gdpr_example_is_caught() {
        // fabric.md: "a GDPR-tagged manifest deployed to a non-EU region is
        // rejected".
        let v = compliance_violation("gdpr", "aws", "us-east-1").expect("violation");
        assert_eq!(v.actual, Jurisdiction::NonEu);
        assert_eq!(v.required, Jurisdiction::Eu);
        assert!(v.to_string().contains("out of the jurisdiction"), "{v}");
    }

    #[test]
    fn gdpr_in_an_eu_region_is_clean() {
        assert_eq!(compliance_violation("gdpr", "aws", "eu-west-1"), None);
        assert_eq!(compliance_violation("GDPR", "azure", "westeurope"), None);
    }

    #[test]
    fn an_undeterminable_jurisdiction_is_a_violation_never_a_pass() {
        // THE decisive one: "we cannot tell" must not read as "it's fine".
        let v = compliance_violation("gdpr", "onprem", "rack-7").expect("violation");
        assert_eq!(v.actual, Jurisdiction::Unknown);
        assert!(v.to_string().contains("cannot be determined"), "{v}");
    }

    #[test]
    fn tags_with_no_geographic_obligation_are_not_invented_into_one() {
        // soc2 / hipaa carry no jurisdictional geography in law; pretending
        // they did would be a fabricated rule.
        assert_eq!(compliance_violation("soc2", "aws", "us-east-1"), None);
        assert_eq!(compliance_violation("hipaa", "gcp", "asia-east1"), None);
    }
}
