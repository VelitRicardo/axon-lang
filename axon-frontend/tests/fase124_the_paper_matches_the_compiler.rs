//! §Fase 124 (D124.5) — **the public paper may not outrun the compiler.**
//!
//! # Why a test over a document
//!
//! `docs/papers/paper_compliance.md` is the public justification for the
//! compliance work. It is read by regulated buyers and by auditors, and the
//! rule the founder ratified for it is absolute:
//!
//! > *cero afirmaciones falsas o desconectadas del código. Si el compilador no
//! > lo demuestra o el runtime no lo ejecuta, el paper se ajusta o el código se
//! > implementa antes de publicar.*
//!
//! A rule stated in a plan is honoured as often as someone rereads the plan —
//! §119.i.2 established that, and §122 spent an entire fase on the general
//! case: a claim nothing checks drifts from the thing it claims. The paper's
//! own §2.1 already drifted once, listing a Κ that shared six of eleven members
//! with the compiler's and adding two audit *frameworks* as if they were data
//! classes.
//!
//! So the load-bearing claim gets a gate. This is the §122 doctrine turned on
//! the document that describes §122.
//!
//! # What this pins, and what it deliberately does not
//!
//! It pins **Κ**: the closed regulatory vocabulary, which is the paper's
//! central formal object and the one an adopter can act on. Prose about
//! architecture is not mechanically checkable and is not checked here.
//!
//! It does NOT pin the paper's roadmap sections. A document is allowed to
//! describe what is coming — it is not allowed to describe it in the present
//! tense, and the sections that do so are marked as pending in the text itself.

/// The paper, embedded at compile time so a change to it rebuilds this test.
const PAPER: &str = include_str!("../../docs/papers/paper_compliance.md");

/// Extract the members of the `\mathrm{K} = { … }` set from the paper's LaTeX.
///
/// Deliberately tolerant of the LaTeX escaping (`PCI\_DSS`) and of whitespace,
/// and deliberately strict about finding the line at all: if the definition is
/// renamed or restructured, this fails loudly rather than silently comparing an
/// empty set to an empty set — the vacuous-pass shape §122.b found in a
/// declaration that "attested" nine rows and exercised none.
fn kappa_in_the_paper() -> Vec<String> {
    let line = PAPER
        .lines()
        .find(|l| l.contains("\\mathrm{K} = "))
        .expect(
            "the paper must still define Κ as `\\mathrm{K} = { … }` — if that definition moved \
             or was renamed, this gate needs its anchor updated, not deleting",
        );
    let open = line.find('{').expect("the set literal must open");
    let close = line.rfind('}').expect("the set literal must close");
    line[open + 1..close]
        .split(',')
        .filter_map(|raw| {
            // `\text{PCI\_DSS}` → `PCI_DSS`
            let start = raw.find("\\text{")? + "\\text{".len();
            let rest = &raw[start..];
            let end = rest.find('}')?;
            Some(rest[..end].replace("\\_", "_").trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// 🎯 The paper's Κ IS the compiler's Κ — same members, same order.
///
/// Order matters here for a reason beyond tidiness: a reader comparing the
/// paper against `REGULATORY_CLASSES` should be able to do it line by line. A
/// set that matches only after sorting invites the reader to trust a
/// correspondence they did not actually check.
#[test]
fn the_papers_regulatory_vocabulary_is_the_compilers() {
    let paper: Vec<String> = kappa_in_the_paper();
    let compiler: Vec<String> = axon_frontend::compliance::REGULATORY_CLASSES
        .iter()
        .map(|s| s.to_string())
        .collect();

    assert!(
        !paper.is_empty(),
        "no classes were parsed out of the paper's Κ definition — the anchor matched but the \
         extraction did not, which is worse than a plain mismatch because it would pass as \
         soon as the compiler's list were also empty"
    );

    assert_eq!(
        paper, compiler,
        "\n\nThe paper's Κ and the compiler's REGULATORY_CLASSES have diverged.\n\n\
         D124.5: the paper is not published while it claims something the code does not do. \
         Either add the class to `axon-frontend/src/compliance.rs` (and give it its metadata \
         row in `esk::compliance`, which `registry_matches_the_frontend_vocabulary` will \
         demand), or correct the paper.\n\n\
         Note that a regulatory label is not a documentation detail — it is an assertion a \
         regulated reader trusts, and the two catalogues this project keeps are deliberately \
         separate: κ classifies WHAT DATA IS; a FrameworkId (SOC2 / ISO27001 / FIPS 140-3 / \
         CC EAL4+) is WHAT THE IMPLEMENTATION IS AUDITED AGAINST. `FIPS` and `CC_EAL4+` \
         belong to the second and must never enter Κ (D124.4).\n"
    );
}

/// The paper must not reassert, in the present tense, the two things §124 has
/// not built yet.
///
/// This is narrower than it looks: it forbids exactly the phrasings that were
/// removed, not the topics. The paper is expected to DISCUSS post-quantum
/// signing and the LATAM vocabulary — it is expected to mark them as pending
/// while they are, which the surrounding text does.
#[test]
fn the_paper_does_not_claim_the_unbuilt_as_delivered() {
    // The PQC scheme is designed and feature-gated; `FIPS.KAT_ED25519` carries
    // the literal locator "PENDING".
    assert!(
        PAPER.contains("diseñado y no construido"),
        "§4.2 must keep stating that the hybrid post-quantum signature is designed and not \
         built. `esk/provenance.rs` says `future §Fase 6.3 work`; a cryptographic guarantee \
         asserted in a document aimed at regulated buyers is the single most refutable claim \
         a paper can carry, and the easiest for an assessor to check."
    );
    assert!(
        !PAPER.contains("Inmutabilidad Post-Cuántica"),
        "the Conclusions must not list post-quantum immutability as a delivered guarantee \
         until Ed25519 + ML-DSA-65 are implemented and their NIST CAVP known-answer vectors \
         run in the suite (§124-B)."
    );

    // §111 retracted `taint:` with its own diagnostic.
    assert!(
        PAPER.contains("axon-T936"),
        "§2.3 must keep naming the retraction: `taint:` is a DEAD field refused by \
         axon-T936, so the trust lattice may be described as a discipline but not as a \
         mechanism that annotation drives."
    );
}
