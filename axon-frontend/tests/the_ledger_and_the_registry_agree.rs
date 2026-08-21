//! v4.4.0 — the third leg: what the compiler ADVERTISES and what it LISTS.
//!
//! Two tables describe the same language from different angles.
//! `advertised.rs` says *what the runtime does with each thing we claim*;
//! `primitive_registry.rs` says *what the catalogue contains*. Each already has
//! its own gates, and the corpus is closed against the registry in both
//! directions. The relation BETWEEN the two tables had none.
//!
//! What that cost, measured: `send`, `receive`, `select`, `branch` and
//! `compliance` were **Attested** in the ledger — cited against a real fixture,
//! run by a real gate — and had no registry row. So `axon.primitives` never
//! listed them. Five constructs the compiler delivered, that no agent asking
//! for the catalogue could discover, for cycles, with every suite green. Both
//! tables were internally consistent. Neither could see the gap.
//!
//! # Why this is not simply "the two sets are equal"
//!
//! Because `ADVERTISED` is not a table of primitives. It is a table of
//! **advertised things**, and some rows are not language constructs at all:
//! `path rewrite` and `PASETO` are capabilities; `effects` is a field on a tool;
//! `backpressure: credit(k)` is a field on a socket. A naive equality law fails
//! on those and gets relaxed into uselessness within a cycle.
//!
//! So the exceptions are enumerated with their KIND, and the enumeration is the
//! assertion. A new ledger row that is not a primitive has to be classified
//! here, which means someone has to look at it and decide — which is the only
//! step that would have caught the five.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// Why a ledger row has no registry entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotAPrimitive {
    /// A FIELD on some declaration, advertised in its own right because the
    /// README badges it. `effects`, `backpressure: credit(k)`.
    Field,
    /// An `@attribute`, which the registry keys differently.
    Attribute,
    /// A capability or an implementation choice — not language surface at all.
    Capability,
    /// The README's grammar spelling of a primitive that IS in the registry
    /// under its bare name. The badge gate requires one ledger row per badge,
    /// so `send T` exists beside `send`.
    ///
    /// **This kind is a known redundancy**, not a design: it means the Attested
    /// census counts four session verbs as eight rows. Recorded rather than
    /// quietly deduplicated, because collapsing it means changing what the
    /// README badge gate reads, and that is a decision rather than a cleanup.
    BadgeSpelling,
}

/// Ledger rows that are deliberately not registry primitives.
///
/// Closed. A row that is neither a registry primitive nor listed here fails the
/// build, and the person adding it decides which it is.
const NOT_PRIMITIVES: &[(&str, NotAPrimitive)] = &[
    ("effects", NotAPrimitive::Field),
    ("backpressure: credit(k)", NotAPrimitive::Field),
    ("reconnect: cognitive_state", NotAPrimitive::Field),
    ("@contract_tool", NotAPrimitive::Attribute),
    ("@csp_tool", NotAPrimitive::Attribute),
    ("shell", NotAPrimitive::Capability),
    ("path rewrite", NotAPrimitive::Capability),
    ("PASETO", NotAPrimitive::Capability),
    ("send T", NotAPrimitive::BadgeSpelling),
    ("receive T", NotAPrimitive::BadgeSpelling),
    ("select {ℓᵢ:…}", NotAPrimitive::BadgeSpelling),
    ("branch {ℓᵢ:…}", NotAPrimitive::BadgeSpelling),
];

/// Registry primitives with no ledger row — the open decision, named.
///
/// `extension` and `witness` are listed in the catalogue and documented, and
/// the ledger says nothing about what their runtime does. Whether they join the
/// advertised set or are marked internal is a product decision; until it is
/// made, they are named HERE so that a THIRD one cannot join them quietly.
/// v4.4.0 — **EMPTY, and that is the point.**
///
/// `extension` and `witness` were the two entries, held here while the
/// decision was open. Both now carry rows: each is writable, enforced by a
/// checker over a closed catalogue, and lowered into the IR, and `extension`
/// decides what the Proof-Carrying Code prover accepts and whether an
/// enterprise deploy is allowed.
///
/// The list stays, empty, because the shape is what protects the property: a
/// catalogued primitive with nothing saying what its runtime does fails the
/// build unless someone types the name here and says why.
const NO_LEDGER_ROW: &[&str] = &[];

fn src(file: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src").join(file);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{} must be readable: {e}", p.display()))
}

/// The row keys of `ADVERTISED`, read from source.
///
/// Read as TEXT rather than through `status_of`, because the question is what
/// the table CONTAINS and a lookup can only answer about a key you already
/// guessed.
fn ledger_rows() -> Vec<String> {
    let text = src("advertised.rs");
    let body = text
        .split("ADVERTISED: &[(&str, RuntimeStatus)] = &[")
        .nth(1)
        .expect("the ADVERTISED table must be found — it was renamed or reshaped")
        .split("\n];")
        .next()
        .expect("the table must terminate");

    let mut out = Vec::new();
    for line in body.lines() {
        // rows sit at exactly one level of indentation inside the array; the
        // deeper strings are field values (`fixture:`, `gate:`, prose).
        let Some(rest) = line.strip_prefix("    (\"") else { continue };
        if let Some(name) = rest.split('"').next() {
            out.push(name.to_string());
        }
    }
    out
}

fn registry_primitives() -> BTreeSet<String> {
    axon_frontend::PRIMITIVE_REGISTRY
        .iter()
        .map(|p| p.name.to_string())
        .collect()
}

#[test]
fn the_ledger_table_is_still_readable() {
    // Every law below stands on this parse. If the table moves and the parse
    // silently returns nothing, all of them pass while checking nothing.
    let rows = ledger_rows();
    assert!(
        rows.len() > 80,
        "only {} ledger rows parsed — ADVERTISED was reshaped and this file is now reading \
         almost nothing: {rows:?}",
        rows.len()
    );
    assert!(
        rows.contains(&"flow".to_string()),
        "the parse did not find `flow`, which is certainly advertised — the row shape changed"
    );
}

#[test]
fn everything_the_compiler_advertises_is_findable_in_the_catalogue() {
    let registry = registry_primitives();
    let classified: BTreeSet<&str> = NOT_PRIMITIVES.iter().map(|(n, _)| *n).collect();

    let unclassified: Vec<String> = ledger_rows()
        .into_iter()
        .filter(|r| !registry.contains(r) && !classified.contains(r.as_str()))
        .collect();

    assert!(
        unclassified.is_empty(),
        "the ledger advertises something the primitive registry does not list, and this file \
         has no classification for it: {unclassified:?}\n\n\
         This is the gap that hid `send`, `receive`, `select`, `branch` and `compliance` — \
         Attested against real fixtures, run by real gates, and absent from \
         `axon.primitives`, so no agent asking for the catalogue could find them.\n\n\
         Decide which it is. If it is a language construct, give it a registry row (and a \
         corpus doc — the coverage gate will ask). If it is a field, an attribute, a \
         capability or a README badge spelling, add it to NOT_PRIMITIVES with that kind."
    );
}

#[test]
fn every_catalogued_primitive_has_a_ledger_row() {
    let rows: BTreeSet<String> = ledger_rows().into_iter().collect();
    let exempt: BTreeSet<&str> = NO_LEDGER_ROW.iter().copied().collect();

    let mut silent: Vec<String> = registry_primitives()
        .into_iter()
        .filter(|p| !rows.contains(p) && !exempt.contains(p.as_str()))
        .collect();
    silent.sort();

    assert!(
        silent.is_empty(),
        "the catalogue lists a primitive the ledger says nothing about: {silent:?}\n\n\
         Every construct an adopter can find must come with a statement of what its runtime \
         actually does — that is the whole point of the ledger. Add the row, or add the name \
         to NO_LEDGER_ROW with the decision that put it there."
    );
}

#[test]
fn the_open_decision_has_not_quietly_grown() {
    // `NO_LEDGER_ROW` is an exemption list, and an exemption list that nobody
    // watches becomes the place things go to be forgotten. Pinning its contents
    // means a third name cannot join the two without someone typing it here.
    let rows: BTreeSet<String> = ledger_rows().into_iter().collect();
    let registry = registry_primitives();

    for name in NO_LEDGER_ROW {
        assert!(
            registry.contains(*name),
            "`{name}` is exempted from needing a ledger row and is not in the registry at \
             all — the exemption outlived the thing it exempted. Remove it."
        );
        assert!(
            !rows.contains(*name),
            "`{name}` now HAS a ledger row. The decision was made; take it out of \
             NO_LEDGER_ROW so the list keeps meaning `still undecided`."
        );
    }
}

#[test]
fn every_exception_carries_a_kind_and_no_kind_is_empty() {
    // A classification everything falls into is not a classification. Each kind
    // must actually be used, or it is a box that exists to hold whatever is
    // inconvenient.
    for kind in [
        NotAPrimitive::Field,
        NotAPrimitive::Attribute,
        NotAPrimitive::Capability,
        NotAPrimitive::BadgeSpelling,
    ] {
        assert!(
            NOT_PRIMITIVES.iter().any(|(_, k)| *k == kind),
            "{kind:?} classifies nothing. Either a row lost its kind, or the kind should go."
        );
    }

    let names: BTreeSet<&str> = NOT_PRIMITIVES.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        names.len(),
        NOT_PRIMITIVES.len(),
        "a row is classified twice in NOT_PRIMITIVES"
    );
}
