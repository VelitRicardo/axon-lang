//! v4.2.0 — κ is a property of the VALUE, not of a declaration's first field.
//!
//! The finding this suite exists for, measured on 4.1.0: **five of the six
//! regulated-vertical scaffolds this repository ships compiled with every
//! single `shield:` line deleted.** `banking`, `fintech`, `healthcare`,
//! `medic_research`, `pharmatech` — the exact files an enterprise adopter
//! starts from. Their compliance coverage did nothing and nothing said so.
//!
//! One cause, and it is structural rather than careless. All five declare a
//! regulated type and then WRAP it:
//!
//! ```text
//! type PatientRecord compliance [HIPAA] { … }
//! type PatientSummaryRequest { rec: PatientRecord }   // ← κ laundered here
//!
//! axonendpoint PatientSummary {
//!     body:   PatientSummaryRequest
//!     shield: ClinicalShield          // decorative: T957 saw no κ to cover
//! }
//! ```
//!
//! `axon-T957` computed the boundary's κ by peeling transparent constructors
//! and reading `t.compliance` — the DECLARATION — while what crosses is the
//! VALUE. `peel_type_constructors`' own doc comment promised that wrapping a
//! type "in an envelope or a list does not launder its regulatory classes",
//! and the commonest wrapper in any program, a struct field, did.
//!
//! The same class of defect sat in `axon-E042`, one field over: it read the
//! FABRIC's region while `manifest.md` documents the manifest's own `region:`
//! as an override — so a GDPR bundle overridden to `us-east-1` compiled clean.
//! A check that reads the neighbouring field is not conservative. It is a
//! guarantee that fails open while looking green.
//!
//! Both halves are asserted here, and every assertion is written as a pair:
//! the shape that must be REFUSED, and the shape that must still PASS. A
//! coverage law that fires on everything protects nothing at a different cost.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn check(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

fn t957(src: &str) -> Vec<String> {
    check(src)
        .into_iter()
        .filter(|m| m.contains("axon-T957"))
        .collect()
}

fn e042(src: &str) -> Vec<String> {
    check(src)
        .into_iter()
        .filter(|m| m.contains("axon-E042"))
        .collect()
}

/// An endpoint carrying `body_type`, with `shield` (empty for none).
fn endpoint(types: &str, shield_decl: &str, shield_ref: &str, body: &str) -> String {
    format!(
        "{types}\n{shield_decl}\n\
         flow F(v: {body}) -> String {{ step S {{ given: v  ask: \"x\"  output: String }} }}\n\
         axonendpoint E {{ method: post  path: \"/a\"  body: {body}  execute: F  \
         output: String {shield_ref} }}\n"
    )
}

const GUARD: &str = "shield Guard { scan: [pii_leak]  on_breach: raise  compliance: [HIPAA] }";

// ── 1. a wrapper does not launder a class ───────────────────────────────────

#[test]
fn a_regulated_type_one_field_deep_is_still_regulated() {
    let src = endpoint(
        "type P compliance [HIPAA] { ssn: String }\ntype W { p: P }",
        "",
        "",
        "W",
    );
    let hits = t957(&src);
    assert_eq!(
        hits.len(),
        1,
        "a HIPAA type wrapped in ONE struct field must still be seen at the boundary — \
         this is the exact shape five shipped scaffolds used to launder κ.\ngot: {hits:#?}"
    );
    assert!(
        hits[0].contains("HIPAA"),
        "the diagnostic must name the class the reader has to cover: {}",
        hits[0]
    );
}

#[test]
fn nesting_depth_does_not_dilute_a_class() {
    // Three levels. Depth is not a defence: an adopter reaches it by writing
    // an ordinary request/response envelope over an ordinary domain type.
    let src = endpoint(
        "type P compliance [HIPAA] { ssn: String }\n\
         type A { p: P }\ntype B { a: A }\ntype C { b: B }",
        "",
        "",
        "C",
    );
    assert_eq!(t957(&src).len(), 1, "κ must survive three levels of wrapping");
}

#[test]
fn a_collection_of_wrappers_carries_the_wrapped_class() {
    // `List<W>` where W wraps a regulated type — the constructor peel and the
    // field walk have to compose, not take turns.
    let src = endpoint(
        "type P compliance [HIPAA] { ssn: String }\n\
         type W { p: P }\ntype Batch { items: List<W> }",
        "",
        "",
        "Batch",
    );
    assert_eq!(
        t957(&src).len(),
        1,
        "a List of wrappers carries the wrapped type's κ"
    );
}

#[test]
fn the_output_side_of_the_boundary_counts_too() {
    // Data leaving is data crossing. A rule that only reads `body:` guards the
    // half of the boundary that carries the least regulated data.
    let src = "type P compliance [HIPAA] { ssn: String }\n\
               type W { p: P }\n\
               flow F(s: String) -> W { step S { given: s  ask: \"x\"  output: W } }\n\
               axonendpoint E { method: post  path: \"/a\"  body: String  execute: F  output: W }\n";
    assert_eq!(t957(src).len(), 1, "the `output:` side carries κ as well");
}

#[test]
fn a_self_referential_type_terminates() {
    // `type Node { next: Node }` — the walk must end on its visited set. A
    // hang here is not a wrong answer, it is a compiler that never returns.
    let src = endpoint(
        "type P compliance [HIPAA] { ssn: String }\ntype Node { next: Node  p: P }",
        "",
        "",
        "Node",
    );
    assert_eq!(t957(&src).len(), 1, "a cyclic type resolves and still reports κ");
}

// ── 2. the law must be silent where there is nothing to cover ───────────────

#[test]
fn a_wrapper_with_no_regulated_field_is_not_regulated() {
    let src = endpoint("type P { ssn: String }\ntype W { p: P }", "", "", "W");
    assert!(
        t957(&src).is_empty(),
        "no type in this program declares a class — T957 must stay silent. A coverage \
         law that fires on everything is noise, and noise is how a real one gets \
         switched off.\ngot: {:#?}",
        t957(&src)
    );
}

#[test]
fn a_covering_shield_satisfies_the_law() {
    let src = endpoint(
        "type P compliance [HIPAA] { ssn: String }\ntype W { p: P }",
        GUARD,
        "shield: Guard",
        "W",
    );
    assert!(
        t957(&src).is_empty(),
        "a shield covering the nested class must satisfy the rule: {:#?}",
        t957(&src)
    );
}

#[test]
fn partial_coverage_names_exactly_what_is_missing() {
    let src = endpoint(
        "type P compliance [HIPAA, GDPR] { ssn: String }\ntype W { p: P }",
        GUARD, // covers HIPAA only
        "shield: Guard",
        "W",
    );
    let hits = t957(&src);
    assert_eq!(hits.len(), 1, "partial coverage is a violation: {hits:#?}");
    assert!(
        hits[0].contains("GDPR"),
        "the set difference must name GDPR, the uncovered class: {}",
        hits[0]
    );
    assert!(
        !hits[0].contains("{GDPR, HIPAA}"),
        "and must NOT re-list the class the shield already covers — the message is a \
         work list, not a restatement: {}",
        hits[0]
    );
}

// ── 3. the channel dual reads the same κ ────────────────────────────────────

#[test]
fn the_channel_rule_sees_through_a_wrapper_too() {
    // axon-T1215 is T957's dual and shares the definition. If it kept its own
    // copy, this is where the two would drift apart.
    let src = "type P compliance [HIPAA] { ssn: String }\n\
               type W { p: P }\n\
               channel C { message: W  qos: at_least_once }\n";
    let hits: Vec<String> = check(src)
        .into_iter()
        .filter(|m| m.contains("axon-T1215"))
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "a channel whose payload WRAPS a regulated type carries that κ: {hits:#?}"
    );
}

// ── 4. E042 reads the region the deployment uses ────────────────────────────

const FABRIC_EU: &str =
    "fabric F { provider: aws  region: \"eu-west-1\"  zones: 1  ephemeral: false }";
const FABRIC_US: &str =
    "fabric F { provider: aws  region: \"us-east-1\"  zones: 1  ephemeral: false }";

#[test]
fn a_manifest_region_override_out_of_jurisdiction_is_refused() {
    // THE FAIL-OPEN CASE. The fabric is compliant; the override is not; the
    // override is what deploys. This compiled clean before v4.2.0.
    let src = format!(
        "{FABRIC_EU}\nmanifest M {{ fabric: F  region: \"us-east-1\"  zones: 1  \
         compliance: [GDPR] }}\n"
    );
    let hits = e042(&src);
    assert_eq!(
        hits.len(),
        1,
        "a GDPR manifest overriding an EU fabric to a non-EU region must be refused — \
         the override is what the deployment uses: {hits:#?}"
    );
    assert!(
        hits[0].contains("overrides fabric"),
        "and the message must say where the offending region came from, or the reader \
         checks the fabric, finds it compliant, and cannot tell: {}",
        hits[0]
    );
}

#[test]
fn a_manifest_region_override_into_jurisdiction_is_accepted() {
    // The noisy direction. Refusing this would be just as wrong, and worse for
    // adoption: it refuses a deployment that is actually compliant.
    let src = format!(
        "{FABRIC_US}\nmanifest M {{ fabric: F  region: \"eu-west-1\"  zones: 1  \
         compliance: [GDPR] }}\n"
    );
    assert!(
        e042(&src).is_empty(),
        "an override INTO the jurisdiction is compliant and must pass: {:#?}",
        e042(&src)
    );
}

#[test]
fn without_an_override_the_fabric_region_still_decides() {
    let bad = format!("{FABRIC_US}\nmanifest M {{ fabric: F  zones: 1  compliance: [GDPR] }}\n");
    assert_eq!(
        e042(&bad).len(),
        1,
        "with no override the fabric's region is the effective one: {:#?}",
        e042(&bad)
    );
    assert!(
        !e042(&bad)[0].contains("overrides fabric"),
        "and there is no override to attribute: {}",
        e042(&bad)[0]
    );

    let good = format!("{FABRIC_EU}\nmanifest M {{ fabric: F  zones: 1  compliance: [GDPR] }}\n");
    assert!(
        e042(&good).is_empty(),
        "an EU fabric with no override satisfies GDPR: {:#?}",
        e042(&good)
    );
}
