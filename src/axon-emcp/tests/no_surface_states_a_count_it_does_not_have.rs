//! v0.53.0 — a number written by hand on a public surface is a claim, and this
//! crate's were wrong by more than 2×.
//!
//! `server.json` — the entry the official MCP registry serves — announced
//! **45 primitives, 33 templates, 17 examples** against a corpus that held 99,
//! 34 and 30. The README said the same. So did the description sent to the
//! agent itself, which is the part that stings: the channel through which a
//! coding agent learns AXON was mis-describing its own contents to the agent.
//!
//! Nobody wrote a false number. Someone wrote a true one, and then the corpus
//! grew — twelve times, across two years — and nothing made the sentence move.
//! That is the whole failure mode: **a count and the thing it counts, in two
//! places, with only discipline holding them together.**
//!
//! The fix has two halves, and the first is the better one:
//!
//! 1. **Stop counting where the count is not needed.** The tool descriptions
//!    used to say "any of the 45 primitives in the registry"; they now say "any
//!    primitive in the registry". A sentence that states no number cannot state
//!    a wrong one, and the agent can call `axon.primitives` if it wants the
//!    figure. Two literals deleted rather than maintained.
//! 2. **Gate the ones the format requires.** `server.json` is consumed by a
//!    registry that wants a human-readable description, and a README is prose.
//!    Those literals stay — and this file compares each of them against the
//!    embedded corpus, so the sentence cannot outlive the truth by a single
//!    build.

use axon_emcp::knowledge::Catalog;
use std::path::PathBuf;

fn crate_file(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{} must be readable: {e}", p.display()))
}

/// Every `<number> <noun>` claim on a surface, for the nouns this crate counts.
///
/// Deliberately regex-free and shape-specific: it looks for the exact phrasing
/// these surfaces use, so a rewording that drops a claim removes it from the
/// check rather than silently passing a claim the check no longer understands.
fn stated(text: &str, noun: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, _) in text.match_indices(noun) {
        // walk back over the space and the digits immediately before the noun
        let head = &text[..i];
        let head = head.strip_suffix(' ').unwrap_or(head);
        let digits: String = head
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if !digits.is_empty() {
            out.push(digits.parse().expect("digits parse"));
        }
    }
    out
}

fn check(surface: &str, text: &str, noun: &str, actual: usize) {
    let claims = stated(text, noun);
    for c in &claims {
        assert_eq!(
            *c, actual,
            "{surface} claims {c} {noun}; the embedded corpus holds {actual}. A count and \
             the thing it counts live in two places here — this assertion is the only thing \
             holding them together. Update the sentence, or delete the number: a surface \
             that states no count cannot state a wrong one."
        );
    }
}

#[test]
fn the_registry_entry_states_the_corpus_it_actually_ships() {
    let cat = Catalog::load_embedded().expect("embedded corpus must load");
    let json = crate_file("server.json");

    // If the description stops making claims entirely that is a WIN, not a
    // failure — but it must be a deliberate one, so the shape is pinned.
    assert!(
        json.contains("primitives"),
        "server.json no longer describes the corpus at all. If the counts were removed on \
         purpose, retire this test in the same change."
    );

    check("server.json", &json, "primitives", cat.primitive_count());
    check("server.json", &json, "templates", cat.template_count());
    check("server.json", &json, "examples", cat.example_count());
}

#[test]
fn the_readme_states_the_corpus_it_actually_ships() {
    let cat = Catalog::load_embedded().expect("embedded corpus must load");
    let md = crate_file("README.md");

    check("README.md", &md, "primitive docs", cat.primitive_count());
    check("README.md", &md, "templates", cat.template_count());
    check("README.md", &md, "idiomatic examples", cat.example_count());
}

#[test]
fn the_text_sent_to_the_agent_states_no_count_at_all() {
    // The surface that matters most, and the one where a wrong number does the
    // most damage: an agent reads this to decide what it can ask for. It used
    // to say "any of the 45 primitives in the registry" against a registry of
    // 99. Now it counts nothing.
    let src = crate_file("src/tools.rs");
    let descriptions: String = axon_emcp::tools::list()
        .iter()
        .map(|t| t.to_string())
        .collect();

    for noun in ["primitives", "templates", "examples"] {
        let in_desc = stated(&descriptions, noun);
        assert!(
            in_desc.is_empty(),
            "a tool description sent to the agent states {in_desc:?} {noun}. Descriptions \
             must not carry counts: they are static strings and the corpus is not, so the \
             number is wrong the moment the corpus grows. Say `any primitive in the \
             registry` and let the agent call axon.primitives."
        );
    }

    // The doc comments in the same file are read by whoever maintains it next,
    // and a stale number there becomes a stale number in the description on the
    // next edit.
    assert!(
        !src.contains("45 primitives"),
        "src/tools.rs still carries the stale `45 primitives` figure in prose"
    );
}

#[test]
fn the_counter_reads_a_number_and_not_a_coincidence() {
    // This helper is what the three laws above stand on. If it silently found
    // nothing, every one of them would pass while checking nothing at all.
    assert_eq!(stated("exposes 99 primitives, 34 templates", "primitives"), vec![99]);
    assert_eq!(stated("exposes 99 primitives, 34 templates", "templates"), vec![34]);
    assert_eq!(stated("7 primitive docs + 8 templates", "primitive docs"), vec![7]);

    // no number in front → no claim
    assert!(stated("any primitive in the registry", "primitive").is_empty());
    assert!(stated("the primitives are listed", "primitives").is_empty());

    // several claims on one surface are all returned, not just the first
    assert_eq!(stated("3 examples here and 4 examples there", "examples"), vec![3, 4]);
}
