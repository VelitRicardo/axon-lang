//! v4.0.0 — **the public paper may not outrun the compiler.**
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
//! v2.83.0 established that, and v2.89.0 spent an entire cycle on the general
//! case: a claim nothing checks drifts from the thing it claims. The paper's
//! own section 2.1 already drifted once, listing a Κ that shared six of eleven members
//! with the compiler's and adding two audit *frameworks* as if they were data
//! classes.
//!
//! So the load-bearing claim gets a gate. This is the v2.89.0 doctrine turned on
//! the document that describes v2.89.0.
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
/// empty set to an empty set — the vacuous-pass shape v2.89.0 found in a
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
         the design decision: the paper is not published while it claims something the code does not do. \
         Either add the class to `axon-frontend/src/compliance.rs` (and give it its metadata \
         row in `esk::compliance`, which `registry_matches_the_frontend_vocabulary` will \
         demand), or correct the paper.\n\n\
         Note that a regulatory label is not a documentation detail — it is an assertion a \
         regulated reader trusts, and the two catalogues this project keeps are deliberately \
         separate: κ classifies WHAT DATA IS; a FrameworkId (SOC2 / ISO27001 / FIPS 140-3 / \
         CC EAL4+) is WHAT THE IMPLEMENTATION IS AUDITED AGAINST. `FIPS` and `CC_EAL4+` \
         belong to the second and must never enter Κ.\n"
    );
}

/// The paper must not claim, in the present tense, what v4.0.0 has not built.
///
/// # The history of this test IS its argument
///
/// Its first version required the paper to say the hybrid signature was
/// *"diseñado y no construido"* — because it was. v4.0.0 then built it:
/// vendored kernels, KATs, cross-implementation interop, and the
/// `evidence-package` wiring. Removing the "not built" phrase made THIS TEST
/// FAIL, which is the gate working — and this edit updates it in the same
/// change that made the claim true, which is the only honest order.
///
/// What the paper may now assert: the hybrid scheme, implemented and verified,
/// producing a detached signature over the package bytes. What it still may
/// NOT assert: non-repudiation (per-package keys prove integrity, not durable
/// identity — custody is named pending work) and CAVP validation.
#[test]
fn the_paper_does_not_claim_the_unbuilt_as_delivered() {
    // The delivered scope, stated precisely — both anchors must survive.
    assert!(
        PAPER.contains("integridad del paquete desde su empaquetado"),
        "section 4.2 must state the guarantee the signature actually delivers: package integrity \
         since packaging."
    );
    assert!(
        PAPER.contains("ni, por tanto, no repudio"),
        "section 4.2 must keep the explicit disclaimer: per-package keys do not constitute durable \
         signer identity, and therefore not non-repudiation. NOM-151's retention argument \
         leans on non-repudiation, which makes THIS the overclaim a regulated reader would \
         act on."
    );
    assert!(
        !PAPER.contains("Inmutabilidad Post-Cuántica"),
        "the Conclusions must not resurrect the old blanket heading — the delivered claim \
         is the hybrid signature with its scope stated, not 'immutability'."
    );

    // v2.67.0 retracted `taint:` with its own diagnostic.
    assert!(
        PAPER.contains("axon-T936"),
        "section 2.3 must keep naming the retraction: `taint:` is a DEAD field refused by \
         axon-T936, so the trust lattice may be described as a discipline but not as a \
         mechanism that annotation drives."
    );

    // v4.0.0 — FIPS 140-3 conformance is not a property you write.
    //
    // The paper claimed the C23 structures were "validadas por el programa CAVP
    // del NIST". There is no CAVP certificate. Validation requires an
    // accredited laboratory under CMVP and a per-algorithm CAVP certificate;
    // implementing the algorithm correctly is necessary and nowhere near
    // sufficient.
    //
    // The project's own build system already had the honest phrasing —
    // `build.rs`: "algorithmically compliant; not formally validated" — and the
    // audit engine already models FIPS as a SUBMISSION: `FIPS.BOUNDARY` and
    // `FIPS.FSM` carry `ManualPolicy` evidence pointing at a submission
    // template. The paper was the only artifact claiming otherwise.
    assert!(
        PAPER.contains("no se encuentran formalmente validadas"),
        "section 3.2 must keep the FIPS qualification: the cryptographic implementations are \
         algorithmically conformant and NOT formally validated. Asserting CAVP validation \
         without the certificates is the one claim in this paper an assessor can refute in \
         a single search of the NIST public list."
    );
    assert!(
        !PAPER.contains("validadas por el programa CAVP"),
        "the paper must not reassert CAVP validation of the C23 structures. If certificates \
         are ever obtained, cite them by number — that is what makes the claim checkable."
    );
}
