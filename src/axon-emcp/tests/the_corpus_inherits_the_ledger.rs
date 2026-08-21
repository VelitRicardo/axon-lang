//! v0.53.0 — what the ℰMCP tells an agent about a primitive must include what
//! its runtime actually does.
//!
//! The finding: the corpus documented `savant` and `synth` in full — grammar,
//! fields, runtime behaviour, worked examples — and never said that **nothing
//! in production calls either of them**. A human reading a confident reference
//! page might still check. An agent does not check: it writes the primitive
//! into an adopter's program, the program compiles, and the behaviour the page
//! described never happens.
//!
//! Three cycles of work went into making the compiler's own ledger honest about
//! that. This suite is what carries the honesty across to the agent.
//!
//! **Derived, not copied.** The status is read from
//! `axon_frontend::advertised::status_of` at load time, so there is no second
//! table to drift. The obvious alternative — a `status:` line in each
//! document's frontmatter — is a copy of the ledger, and a corpus doc claiming
//! `attested` over a row the ledger calls `Unwired` would launder the debt
//! through the one surface an agent trusts.

use axon_emcp::knowledge::{Catalog, RuntimeReality};

fn catalog() -> Catalog {
    Catalog::load_embedded().expect("the embedded corpus must load")
}

#[test]
fn every_documented_primitive_carries_its_ledger_row() {
    let cat = catalog();
    let mut without_row = Vec::new();
    for p in cat.primitives() {
        if p.reality.is_none() {
            without_row.push(p.name.clone());
        }
    }

    // `extension` and `witness` are documented and have no row — a known
    // finding, held open as a decision rather than a silent gap. They are
    // named here so that the LIST is the assertion: a new undocumented-status
    // primitive joins this failure instead of slipping in beside them.
    without_row.sort();
    assert_eq!(
        without_row,
        vec!["extension".to_string(), "witness".to_string()],
        "the set of primitives with NO ledger row changed. Every documented primitive \
         should have a row in `advertised.rs` saying what its runtime does; the two \
         listed here are a known open question. If you added a primitive, add its row. \
         If the question was resolved, update this assertion."
    );
}

#[test]
fn the_ledger_status_is_read_from_the_compiler_not_restated() {
    // The point of deriving: the two agree BY CONSTRUCTION. This asserts the
    // wiring is live — that `reality` is actually populated from `status_of`
    // and not left at its default for everything.
    let cat = catalog();
    for p in cat.primitives() {
        let expected = axon_frontend::advertised::status_of(&p.name);
        assert_eq!(
            p.reality.is_some(),
            expected.is_some(),
            "`{}`: the corpus and the compiler ledger disagree about whether a row \
             exists. They cannot disagree unless the derivation broke.",
            p.name
        );
    }
}

#[test]
fn the_population_is_not_uniformly_deliverable() {
    // A field that is `true` for everything says nothing, and would pass every
    // other test in this file. The corpus documents primitives that do NOT
    // deliver — that is the whole reason the field exists — so at least one
    // must come back non-deliverable.
    let cat = catalog();
    let undeliverable: Vec<&str> = cat
        .primitives()
        .filter(|p| p.reality.map(|r| !r.is_deliverable()).unwrap_or(false))
        .map(|p| p.name.as_str())
        .collect();

    assert!(
        !undeliverable.is_empty(),
        "no documented primitive comes back non-deliverable. Either the ledger has no \
         Unwired/NotImplemented rows left (in which case this test should be retired \
         deliberately), or the derivation is returning a status that flatters every row."
    );
}

#[test]
fn a_state_that_should_warn_produces_a_warning() {
    // The states an adopter cannot be allowed to meet silently.
    for r in [
        RuntimeReality::Unwired,
        RuntimeReality::NotImplemented,
        RuntimeReality::Partial,
        RuntimeReality::Unaudited,
    ] {
        assert!(
            !r.warning().is_empty(),
            "{r:?} must carry a warning: an agent that reads only the body writes the \
             primitive into an adopter's program."
        );
    }

    // And the states where a warning would be noise. A caveat on everything is
    // a caveat on nothing.
    for r in [RuntimeReality::Attested, RuntimeReality::FailsClosed] {
        assert!(
            r.warning().is_empty(),
            "{r:?} must NOT warn — it is either proven by a gate or refuses honestly."
        );
    }
}

#[test]
fn only_the_states_that_do_not_deliver_are_marked_undeliverable() {
    // `Partial` and `Unaudited` still DO the thing, with a caveat; `FailsClosed`
    // refuses honestly, which is a behaviour an adopter can build on. Only
    // Unwired and NotImplemented are indistinguishable to an adopter from a
    // primitive that does not exist.
    assert!(!RuntimeReality::Unwired.is_deliverable());
    assert!(!RuntimeReality::NotImplemented.is_deliverable());
    assert!(RuntimeReality::Attested.is_deliverable());
    assert!(RuntimeReality::Partial.is_deliverable());
    assert!(RuntimeReality::Unaudited.is_deliverable());
    assert!(RuntimeReality::FailsClosed.is_deliverable());
}

#[test]
fn every_slug_is_distinct_and_snake_case() {
    // The slug is what an agent branches on. Two states sharing one spelling
    // would collapse a distinction the ledger spent three cycles drawing.
    let slugs: Vec<&str> = [
        RuntimeReality::Attested,
        RuntimeReality::Partial,
        RuntimeReality::Unwired,
        RuntimeReality::NotImplemented,
        RuntimeReality::FailsClosed,
        RuntimeReality::Unaudited,
    ]
    .iter()
    .map(|r| r.slug())
    .collect();

    let mut sorted = slugs.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), slugs.len(), "two states share a slug: {slugs:?}");

    for s in &slugs {
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "slug `{s}` is not snake_case"
        );
    }
}
