//! v4.0.0 — **the closed regulatory vocabulary Κ.**
//!
//! # Why this module exists in the FRONTEND
//!
//! The ESK paper (section 6.1, *Regulatory Type Theory*) states the rule plainly:
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
//! Measured 2026-08-14, and recorded in `advertised.rs` before this cycle
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
//! This is deliberately NOT a second copy of the catalog. v2.89.0 paid for a law
//! written three times: two copies were updated, the third was missed, and the
//! workspace suite caught it — after the first two attempts had already shipped
//! the drift.

use std::collections::BTreeSet;

/// The canonical regulatory classes — Κ.
///
/// Reflexive by construction: a class covers itself and nothing else.
/// Cross-framework overlap (does SOC2 imply ISO27001?) is an explicit policy
/// decision a regulator makes, not something a compiler may infer.
///
/// **Adding an entry is a deliberate act.** A new framework here becomes a
/// label every adopter can assert, and the assertion is what a regulated
/// reader trusts — so it belongs in a cycle with a paper behind it, not in a
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
    // v4.0.0 — the four LATAM jurisdictions the product serves.
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

/// Peel the type constructors that are transparent to κ:
/// `FlowEnvelope<T>` / `List<T>` / `Stream<T>` carry the κ of `T`, and `?`
/// (optionality) is orthogonal to what the data IS.
///
/// v4.0.0 — hoisted here from `type_checker`'s private helper because a
/// THIRD consumer arrived (the audit engine's coverage rule, joining T957 and
/// the typed-bus predicate) and three private copies of "what wraps a type
/// without changing its κ" is how the copies drift — v2.89.0 paid for exactly
/// that with a law written three times.
pub fn peel_type_constructors(type_ref: &str) -> &str {
    let mut t = type_ref.trim();
    t = t.strip_suffix('?').unwrap_or(t).trim();
    loop {
        let peeled = ["FlowEnvelope<", "List<", "Stream<"].iter().find_map(|ctor| {
            t.strip_prefix(*ctor)
                .and_then(|rest| rest.strip_suffix('>'))
                .map(|inner| inner.trim())
        });
        match peeled {
            Some(inner) => t = inner.strip_suffix('?').unwrap_or(inner).trim(),
            None => return t,
        }
    }
}

/// How a given exit's κ is covered — or why it cannot be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// κ is read from DECLARED TYPES at this exit and covered by a named
    /// `shield:`. `kappa_from` names the fields the rule reads; `code` is the
    /// diagnostic that refuses an uncovered crossing.
    Shielded {
        kappa_from: &'static str,
        code: &'static str,
    },
    /// This exit has NO static κ. Not an oversight — a property of how it binds
    /// its data, recorded so that "no rule here" can never be confused with
    /// "nobody looked here". `governed_by` is the diagnostic that governs it on
    /// the epistemic axis instead, or `None` where nothing does.
    NoStaticKappa {
        governed_by: Option<&'static str>,
        why: &'static str,
    },
}

/// One way data leaves an AXON program.
#[derive(Debug, Clone, Copy)]
pub struct Egress {
    pub primitive: &'static str,
    pub coverage: Coverage,
}

/// **The closed catalogue of exits.**
///
/// A compliance guarantee is worth exactly what its list of exits is worth. A
/// list somebody maintains by hand develops a hole the first time primitive
/// number eight arrives, and the hole is invisible: every existing rule still
/// passes. So this catalogue is closed, it is gated
/// (`tests/the_egress_catalogue_is_closed.rs`), and every entry says how that
/// exit is covered **or why it cannot be**.
///
/// Recording the uncoverable ones is the point. An exit missing from this list
/// is indistinguishable from an exit nobody examined; an exit present with
/// `NoStaticKappa` is a stated limit an auditor can read and price.
///
/// # What was measured to build it (v4.3.0)
///
/// κ originates in exactly one place: a `type` declaration's `compliance:`
/// list. So an exit has a static κ precisely when it binds a DECLARED TYPE.
/// Three do. Four do not, for two different reasons, and both reasons are
/// structural rather than incidental:
///
/// - `document` / `deliver` / `notify` bind **bare value references**
///   (`DocScalar::Ref`) and have no typed binding site in the grammar —
///   `render` is a runtime concept, not a declaration. This is also exactly why
///   their epistemic barriers work: "is this value attributed?" is answerable
///   about a bare reference, and "what classes does it carry?" is not.
/// - `axonstore` binds a **closed catalogue of primitive SQL column types**. A
///   regulated value is decomposed into columns before it lands, and κ does not
///   survive that decomposition — the same intra-expression limit this cycle
///   names in its own trade-off list, met at the storage boundary.
pub const EGRESS_PRIMITIVES: &[Egress] = &[
    Egress {
        primitive: "axonendpoint",
        coverage: Coverage::Shielded {
            kappa_from: "body: and output:",
            code: "axon-T957",
        },
    },
    Egress {
        primitive: "channel",
        coverage: Coverage::Shielded {
            kappa_from: "message:",
            code: "axon-T1215",
        },
    },
    // v4.3.0 — the widest exit, and the last one to get a rule. A whole
    // regulated record passed to a tool with `effects: <network, web>`
    // compiled clean until this entry existed.
    Egress {
        primitive: "tool",
        coverage: Coverage::Shielded {
            kappa_from: "parameters: and output_type:",
            code: "axon-T1221",
        },
    },
    Egress {
        primitive: "document",
        coverage: Coverage::Shielded {
            kappa_from: "payload:",
            code: "axon-T1222",
        },
    },
    Egress {
        primitive: "deliver",
        coverage: Coverage::Shielded {
            kappa_from: "payload:",
            code: "axon-T1223",
        },
    },
    Egress {
        primitive: "notify",
        coverage: Coverage::Shielded {
            kappa_from: "payload:",
            code: "axon-T1224",
        },
    },
    Egress {
        primitive: "axonstore",
        coverage: Coverage::NoStaticKappa {
            governed_by: None,
            why: "a store schema is a closed catalogue of primitive SQL column types, so a \
                  regulated value is decomposed into columns before it lands and carries no \
                  declared type to read a class from",
        },
    },
];

/// The exits whose κ this compiler can compute and refuse on.
pub fn shielded_exits() -> impl Iterator<Item = &'static Egress> {
    EGRESS_PRIMITIVES
        .iter()
        .filter(|e| matches!(e.coverage, Coverage::Shielded { .. }))
}

/// Every κ class a value of this type carries — **including the classes its
/// fields carry**.
///
/// This is the answer to "what regulated data is in here?", and it is the only
/// definition of that question. [`peel_type_constructors`] answers a smaller
/// one — which wrappers are transparent — and answering the big question with
/// the small one is the defect this function exists to close.
///
/// # Why a walk and not a field read
///
/// The κ of a type used to be `t.compliance`, read after peeling constructors.
/// That reads the DECLARATION, and what crosses a boundary is the VALUE. The
/// two differ the moment anyone writes the most ordinary thing in the language:
///
/// ```text
/// type PatientRecord compliance [HIPAA] { … }
/// type PatientSummaryRequest { rec: PatientRecord }   // ← κ laundered
///
/// axonendpoint PatientSummary {
///     body:   PatientSummaryRequest
///     shield: ClinicalShield          // decorative — T957 saw no κ to cover
/// }
/// ```
///
/// Five of the six regulated-vertical scaffolds this repository ships were
/// written exactly that way, and every one of them compiled with **all** of its
/// `shield:` lines deleted. `peel_type_constructors`' own doc comment says that
/// wrapping a type "in an envelope or a list does not launder its regulatory
/// classes" — and the commonest wrapper of all, a struct field, did.
///
/// # What it walks
///
/// - the transparent constructors, via [`peel_type_constructors`];
/// - **any other generic spelling** `Name<Inner>` — `Inner` is walked whether or
///   not `Name` is a constructor this module recognises. An unknown wrapper must
///   not be a hiding place, which is the same law one level up;
/// - every field of every declared type it reaches, transitively.
///
/// # Totality
///
/// Total by construction. An unresolvable name contributes nothing (the house
/// soft-type discipline: builtins and imported names are not declared here), and
/// a cycle — `type Node { next: Node }` — terminates on the visited set rather
/// than recursing forever.
pub fn transitive_kappa(program: &crate::ast::Program, type_ref: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut queue: Vec<String> = vec![type_ref.to_string()];

    while let Some(spelling) = queue.pop() {
        let base = peel_type_constructors(&spelling);
        if base.is_empty() {
            continue;
        }

        // A generic this module does not recognise still has an argument, and
        // that argument is data. Walk it before resolving the head.
        if let Some(open) = base.find('<') {
            if let Some(inner) = base.strip_suffix('>').map(|s| &s[open + 1..]) {
                for arg in inner.split(',') {
                    let arg = arg.trim();
                    if !arg.is_empty() && !visited.contains(arg) {
                        queue.push(arg.to_string());
                    }
                }
            }
        }

        if !visited.insert(base.to_string()) {
            continue;
        }

        let Some(decl) = program.declarations.iter().find_map(|d| match d {
            crate::ast::Declaration::Type(t) if t.name == base => Some(t),
            _ => None,
        }) else {
            continue;
        };

        found.extend(decl.compliance.iter().cloned());

        for field in &decl.fields {
            let spelling = if field.type_expr.generic_param.is_empty() {
                field.type_expr.name.clone()
            } else {
                format!("{}<{}>", field.type_expr.name, field.type_expr.generic_param)
            };
            if !spelling.is_empty() {
                queue.push(spelling);
            }
        }
    }

    found
}

/// **What a piece of data IS** — independent of any regulation.
///
/// κ says which regime a value falls under. It does not say whether a field is
/// a social-security number or a clinical note, and without that distinction
/// every claim of de-identification is a promise: the compiler can check that a
/// class was declared, and nothing at all about what was actually removed.
///
/// This catalogue is **transversal**, deliberately. A social-security number is
/// an identifier under HIPAA, under GDPR and under Ley 1581; what changes
/// between regimes is what each one requires you to DO with it. So the kinds
/// describe the data once, and each regime brings its own rule table over them
/// ([`HIPAA_SAFE_HARBOR`] is the first). Writing seventeen classes per Κ member
/// would be the same duplication this project has spent cycles removing — and
/// the LATAM jurisdictions reuse this list as-is.
///
/// Closed, like Κ. An identifier kind outside it is a compile error rather than
/// a string the compiler cannot reason about: *a free string field breeds an
/// imaginary catalogue*, recorded four times in this repository.
pub const IDENTIFIER_KINDS: &[&str] = &[
    "name",
    "geography",
    "date",
    "age",
    "phone",
    "fax",
    "email",
    "ssn",
    "medical_record_number",
    "health_plan_number",
    "account_number",
    "license_number",
    "vehicle_identifier",
    "device_identifier",
    "url",
    "ip_address",
    "biometric",
    "face_photo",
];

/// Is `kind` a member of the identifier catalogue?
pub fn is_known_identifier(kind: &str) -> bool {
    IDENTIFIER_KINDS.contains(&kind)
}

/// The nearest catalogue member to a misspelling, for the diagnostic.
pub fn nearest_identifier(kind: &str) -> Option<&'static str> {
    let lower = kind.to_ascii_lowercase();
    IDENTIFIER_KINDS
        .iter()
        .copied()
        .find(|k| k.eq_ignore_ascii_case(&lower) || k.starts_with(lower.as_str()))
}

/// What a regime requires be done with an identifier before the data leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deidentify {
    /// The value must be removed. Nothing of it may survive.
    Suppress,
    /// The value may be KEPT in a reduced form. This is the half a masking-only
    /// mechanism cannot express, and its absence is expensive: the regulation
    /// does not ask you to delete the date, it asks for the year. A system that
    /// makes an adopter throw away data the law lets them keep is a system that
    /// gets routed around.
    Generalise(Generalisation),
}

/// The reductions this compiler knows how to require and the runtime knows how
/// to perform. Closed — and short, which is honest: cell suppression,
/// micro-aggregation and swapping are not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Generalisation {
    /// A date reduced to its year.
    Year,
    /// A postal code reduced to its first three digits — admissible only where
    /// the resulting unit exceeds 20,000 people. That population fact is NOT
    /// decided here: it is a fact about the world that changes with the census,
    /// and it is declared per deployment in the `manifest`. The compiler checks
    /// that it was DECLARED, not that it is true.
    Zip3,
    /// Ages above 89 collapsed into a single category.
    AgeCap,
}

/// One regime's requirement for one identifier kind.
pub struct DeidentifyRule {
    pub kind: &'static str,
    pub operation: Deidentify,
    /// The letter this rule carries in the regulation's own enumeration, so the
    /// mapping can be audited against the source text rather than trusted.
    pub citation: &'static str,
}

/// **HIPAA Safe Harbor — 45 CFR 164.514(b)(2).**
///
/// The regulation lists eighteen identifier classes. **Seventeen of them are
/// enumerable, and this table is that enumeration**; the eighteenth —
/// *"any other unique identifying number, characteristic, or code"* — is a
/// judgement, not a class, and it is not here. Neither is the final clause,
/// which requires that the covered entity have no *actual knowledge* that the
/// information could identify an individual: that is a condition on the ENTITY,
/// not on the data, and no type system decides it.
///
/// Those two are the whole residue, and they are attested rather than proved.
/// That is a much smaller gap than "a compiler cannot decide Safe Harbor", and
/// the difference is the point of this table.
///
/// This encoding is not the regulation. The regulation is the source; a mapping
/// error here is a defect in this file, which is why every row cites the item
/// it encodes.
pub const HIPAA_SAFE_HARBOR: &[DeidentifyRule] = &[
    DeidentifyRule { kind: "name", operation: Deidentify::Suppress, citation: "164.514(b)(2)(i)(A)" },
    // Geography and dates are where the regulation ALLOWS you to keep something.
    DeidentifyRule { kind: "geography", operation: Deidentify::Generalise(Generalisation::Zip3), citation: "164.514(b)(2)(i)(B)" },
    DeidentifyRule { kind: "date", operation: Deidentify::Generalise(Generalisation::Year), citation: "164.514(b)(2)(i)(C)" },
    DeidentifyRule { kind: "age", operation: Deidentify::Generalise(Generalisation::AgeCap), citation: "164.514(b)(2)(i)(C)" },
    DeidentifyRule { kind: "phone", operation: Deidentify::Suppress, citation: "164.514(b)(2)(i)(D)" },
    DeidentifyRule { kind: "fax", operation: Deidentify::Suppress, citation: "164.514(b)(2)(i)(E)" },
    DeidentifyRule { kind: "email", operation: Deidentify::Suppress, citation: "164.514(b)(2)(i)(F)" },
    DeidentifyRule { kind: "ssn", operation: Deidentify::Suppress, citation: "164.514(b)(2)(i)(G)" },
    DeidentifyRule { kind: "medical_record_number", operation: Deidentify::Suppress, citation: "164.514(b)(2)(i)(H)" },
    DeidentifyRule { kind: "health_plan_number", operation: Deidentify::Suppress, citation: "164.514(b)(2)(i)(I)" },
    DeidentifyRule { kind: "account_number", operation: Deidentify::Suppress, citation: "164.514(b)(2)(i)(J)" },
    DeidentifyRule { kind: "license_number", operation: Deidentify::Suppress, citation: "164.514(b)(2)(i)(K)" },
    DeidentifyRule { kind: "vehicle_identifier", operation: Deidentify::Suppress, citation: "164.514(b)(2)(i)(L)" },
    DeidentifyRule { kind: "device_identifier", operation: Deidentify::Suppress, citation: "164.514(b)(2)(i)(M)" },
    DeidentifyRule { kind: "url", operation: Deidentify::Suppress, citation: "164.514(b)(2)(i)(N)" },
    DeidentifyRule { kind: "ip_address", operation: Deidentify::Suppress, citation: "164.514(b)(2)(i)(O)" },
    DeidentifyRule { kind: "biometric", operation: Deidentify::Suppress, citation: "164.514(b)(2)(i)(P)" },
    DeidentifyRule { kind: "face_photo", operation: Deidentify::Suppress, citation: "164.514(b)(2)(i)(Q)" },
];

/// What Safe Harbor requires of this identifier kind, if anything.
pub fn safe_harbor_rule(kind: &str) -> Option<&'static DeidentifyRule> {
    HIPAA_SAFE_HARBOR.iter().find(|r| r.kind == kind)
}

/// Every identifier kind a value of this type carries — **its own and its
/// fields'**, transitively.
///
/// The same walk as [`transitive_kappa`], over the same structure, for the same
/// reason: wrapping a type in a request struct must not hide what is inside it.
/// Keeping them as two functions over one traversal shape is a deliberate
/// trade — they answer different questions and a single function returning both
/// would be read as "the compliance facts", which is exactly the conflation
/// this cycle exists to remove.
pub fn transitive_identifiers(program: &crate::ast::Program, type_ref: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut queue: Vec<String> = vec![type_ref.to_string()];

    while let Some(spelling) = queue.pop() {
        let base = peel_type_constructors(&spelling);
        if base.is_empty() {
            continue;
        }
        if let Some(open) = base.find('<') {
            if let Some(inner) = base.strip_suffix('>').map(|s| &s[open + 1..]) {
                for arg in inner.split(',') {
                    let arg = arg.trim();
                    if !arg.is_empty() && !visited.contains(arg) {
                        queue.push(arg.to_string());
                    }
                }
            }
        }
        if !visited.insert(base.to_string()) {
            continue;
        }
        let Some(decl) = program.declarations.iter().find_map(|d| match d {
            crate::ast::Declaration::Type(t) if t.name == base => Some(t),
            _ => None,
        }) else {
            continue;
        };

        if !decl.identifier.is_empty() {
            found.insert(decl.identifier.clone());
        }
        for field in &decl.fields {
            let spelling = if field.type_expr.generic_param.is_empty() {
                field.type_expr.name.clone()
            } else {
                format!("{}<{}>", field.type_expr.name, field.type_expr.generic_param)
            };
            if !spelling.is_empty() {
                queue.push(spelling);
            }
        }
    }

    found
}

/// Peel a channel `message:` spelling to its payload leaf.
///
/// `Channel<…<T>>` peels to `T` — a second-order channel relays the same
/// payload, so it carries the same κ — then the ordinary constructors peel
/// via [`peel_type_constructors`].
pub fn peel_channel_payload(spelling: &str) -> &str {
    let mut leaf = spelling.trim();
    while let Some(inner) = leaf
        .strip_prefix("Channel<")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        leaf = inner.trim();
    }
    peel_type_constructors(leaf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_is_the_fifteen_the_paper_names() {
        assert_eq!(
            REGULATORY_CLASSES.len(),
            15,
            "Κ is the canonical registry from the ESK paper, extended in v4.0.0 with the \
             four LATAM jurisdictions. Changing its size is a decision about what an adopter \
             may assert, not a refactor — and the paper's own Κ is pinned against this list \
             by the paper-matches-compiler gate."
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
