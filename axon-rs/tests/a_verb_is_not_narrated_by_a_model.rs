//! v4.5.0 — the non-streaming executor decides what to do with every kind of
//! node, and the decision has no default.
//!
//! # The bug this suite exists because of
//!
//! `routes_through_dispatcher` used to be a `matches!` listing the verbs that
//! bridge into the flow dispatcher. Every other variant took the `false` branch,
//! including every variant written after it — so the default was **"hand it to
//! the LLM and bind whatever comes back"**.
//!
//! `declassify` landed in exactly that hole. Its dispatcher handler projects the
//! record and reduces what the regime permits, failing closed on anything it
//! cannot reduce honestly. On this path the model was handed
//! *"Declassify HIPAA from rec via SafeHarborShield"* and its prose was bound
//! under the de-identified type. The compiler had already proved that type no
//! longer carries the class, so every κ boundary downstream waved it through —
//! and nothing had removed a single identifier.
//!
//! That is the shape worth naming: **a fabricated result is indistinguishable
//! from a real one at the call site.** A `navigate` that returns nothing is
//! visible; a de-identification that returns a plausible paragraph is not.
//!
//! # Why this reads a fixture from disk
//!
//! Because the ledger rows for `identifier` / `declassify` / `attest` / `census`
//! cite this file, and a row is worth exactly what its gate walks. The obvious
//! candidate was the parity corpus — it drives every fixture through both
//! execution paths — but it drives the sync path with the `stub` backend, and
//! that branch returns before the routing decision is ever evaluated. It is a
//! real gate for what it measures, and it does not measure this. Citing it here
//! would have produced precisely the defect the ledger exists to prevent: a row
//! naming a gate that never walks the path it attests.
//!
//! So this compiles the published fixture off disk and checks the two things
//! that decide whether the verb does what the language says: **where it is
//! routed**, and **what plan it carries there**.

use axon::ir_generator::IRGenerator;
use axon::ir_nodes::{IRFlowNode, IRProgram};
use axon::lexer::Lexer;
use axon::parser::Parser;
use std::path::{Path, PathBuf};

const FIXTURE: &str =
    "tests/fixtures/parity_corpus/medicine/11_safe_harbor_declassification.axon";

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE)
}

fn compile_fixture() -> IRProgram {
    let source = std::fs::read_to_string(fixture_path())
        .unwrap_or_else(|e| panic!("the ledger cites {FIXTURE}, which must exist: {e}"));
    let tokens = Lexer::new(&source, FIXTURE).tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    IRGenerator::new().generate(&program)
}

fn declassify_node(ir: &IRProgram) -> &IRFlowNode {
    ir.flows
        .iter()
        .flat_map(|f| f.steps.iter())
        .find(|n| matches!(n, IRFlowNode::Declassify(_)))
        .expect("the fixture's flow lowers a declassify node")
}

// ── the fix, over the source the ledger points at ───────────────────────────

#[test]
fn the_published_fixture_declassifies_by_computation_not_by_narration() {
    // THE REGRESSION. If this flips back, an adopter's non-streaming endpoint
    // de-identifies patient records by asking a language model to say it did.
    let ir = compile_fixture();
    assert_eq!(
        axon::runner::node_route(declassify_node(&ir)),
        axon::runner::NodeRoute::Dispatcher,
        "a declassification must reach its handler on this path too"
    );
}

#[test]
fn the_plan_the_compiler_resolved_rides_the_node() {
    // Routing is half of it. The handler re-derives nothing, so the node has to
    // CARRY the decision — and an empty plan would project away every field and
    // yield `{}`: silent, total data loss under a type saying the record
    // survived.
    let ir = compile_fixture();
    let IRFlowNode::Declassify(node) = declassify_node(&ir) else {
        unreachable!()
    };
    assert_eq!(
        node.keep,
        vec!["dob".to_string(), "note".to_string()],
        "the destination type's fields, and only those"
    );
    assert_eq!(
        node.generalise,
        vec![("dob".to_string(), "year".to_string())],
        "`year` comes from HIPAA_SAFE_HARBOR, not from the fixture's author"
    );
    assert!(
        !node.keep.contains(&"ssn".to_string()),
        "the destination type has no slot for it, so the projection removes it — \
         that is the suppression, and it cannot be forgotten"
    );
}

#[test]
fn the_fixture_signs_for_what_the_compiler_could_not_decide() {
    // The same file has to exercise `attest`, or its ledger row cites a gate
    // that never sees it. And the citation must be RESOLVED at lowering — an
    // evidence reader should not need to know the regulation to print it.
    let ir = compile_fixture();
    let a = ir
        .attestations
        .first()
        .expect("the fixture carries a determination");
    assert_eq!(a.for_type, "DeidentifiedNote");
    assert_eq!(a.basis, "safe_harbor");
    assert_eq!(a.citation, "45 CFR 164.514(b)(2)");
    assert_eq!(a.residual.len(), 2, "Safe Harbor leaves exactly two open");
    assert!(!a.by.trim().is_empty() && !a.on.trim().is_empty());
}

#[test]
fn the_fixture_types_its_identifier_fields() {
    // The detail that decides whether any of this protects anything. A kind is
    // a property of the TYPE; `dob: Text` would carry none, nothing would be
    // reduced, and no diagnostic would fire. A published example showing the
    // untyped shape teaches the version that does not work.
    // Read from the AST, not the IR: a kind is a COMPILE-TIME fact. The plan
    // is resolved at lowering, so `IRType` carries no `identifier` — the
    // runtime never needs to know, and does not.
    let source = std::fs::read_to_string(fixture_path()).expect("readable");
    let tokens = Lexer::new(&source, FIXTURE).tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    let kinds: Vec<&str> = program
        .declarations
        .iter()
        .filter_map(|d| match d {
            axon::ast::Declaration::Type(t) if !t.identifier.is_empty() => {
                Some(t.identifier.as_str())
            }
            _ => None,
        })
        .collect();
    assert!(
        kinds.contains(&"date") && kinds.contains(&"ssn"),
        "the fixture must declare its identifier kinds, or it demonstrates the \
         shape that silently protects nothing: {kinds:?}"
    );
}

// ── and the census of what is still narrated ────────────────────────────────

/// Every variant routed to `Narrated`, read out of the route table.
///
/// Walks the arms rather than slicing between two markers: an earlier version
/// cut at the first `=> Narrated,` and silently depended on the arms' ORDER, so
/// moving a variant changed the count for the wrong reason.
fn narrated_variants() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runner.rs");
    let src = std::fs::read_to_string(&path)
        .expect("runner.rs is readable")
        .replace("\r\n", "\n");

    let table = src
        .split_once("pub fn node_route(")
        .expect("the route table is where this gate thinks it is")
        .1;
    let table = table.split_once("\n}\n").expect("the function ends").0;

    // Split on the verdicts. Chunk `i` holds the variants belonging to the
    // verdict that OPENS chunk `i + 1`.
    let chunks: Vec<&str> = table.split("=> ").collect();
    let mut found: Vec<String> = Vec::new();
    for (i, chunk) in chunks.iter().enumerate().skip(1) {
        if !chunk.starts_with("Narrated,") {
            continue;
        }
        let owner = chunks[i - 1];
        // Drop the previous arm's verdict word so its variants are not counted.
        let owner = match owner.find(',') {
            Some(c) if i > 1 => &owner[c + 1..],
            _ => owner,
        };
        for (at, _) in owner.match_indices("N::") {
            found.push(
                owner[at + 3..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric())
                    .collect(),
            );
        }
    }
    found.sort();
    found.dedup();
    found
}

#[test]
fn the_verbs_still_narrated_on_this_path_are_declared_not_defaulted() {
    // The point is not that this number is zero — it is not, and pretending
    // otherwise would be the same dishonesty in a nicer shape. The point is
    // that it is a NUMBER. Before the table was exhaustive these verbs were not
    // a list anyone could find; they were whatever nobody had added to a
    // `matches!`, which reads from the outside exactly like "nothing".
    //
    // Each has a dispatcher handler wired for `transport: sse` (v1.26.0) and was
    // never bridged into this non-streaming executor. Moving one across is a
    // one-line change plus its behavioural test — and this count going DOWN is
    // the shape of that work landing.
    let narrated = narrated_variants();
    assert_eq!(
        narrated.len(),
        23,
        "the narrated census changed. If a verb was BRIDGED, lower this number \
         in the same commit that adds its behavioural test. If a NEW verb was \
         routed here, say why a model may narrate it — the default that used to \
         answer this question silently is what shipped the declassify bug.\n\
         current: {narrated:#?}"
    );
}

#[test]
fn the_verb_that_retires_a_regulatory_class_is_not_among_them() {
    // Stated where it can be read: `declassify` must never reappear on the
    // narrated side. It is the one verb whose output is a value the program has
    // declared *no longer regulated* — so a plausible sentence accepted in its
    // place disables the boundary laws rather than merely returning a wrong
    // answer.
    assert!(
        !narrated_variants().contains(&"Declassify".to_string()),
        "a de-identification performed by a language model is not one"
    );
}
