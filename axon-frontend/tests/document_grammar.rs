//! v2.53.0 — grammar + AST + IR + structure checker + the
//! assertion-laundering barrier for Native Document Synthesis (`document`).
//! See `the design plan` (axon-enterprise).
//!
//! Pinned properties:
//! 1. A `document` parses into `DocumentDefinition` with a block tree.
//! 2. It lowers to `IRProgram.documents`; a document-less program elides it.
//! 3. **axon-T916** — the barrier: an unattributed flow-value in an assertive
//!    slot fails; with `attribute:` it passes; wrapped in `epistemic{believe}`
//!    it passes; a literal always passes.
//! 4. **axon-T910/T911/T912/T913/T914/T917** — structure laws.

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn check_errors(src: &str) -> Vec<String> {
    let prog = parse(src);
    TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

fn ir_json(src: &str) -> String {
    let prog = parse(src);
    let ir = IRGenerator::new().generate(&prog);
    serde_json::to_string(&ir).expect("serialize IR")
}

const GOOD: &str = r#"
document quarterly_report {
    target:     docx
    provenance: embedded
    effects:    <io, storage>
    section {
        heading: "Q3 Results"
        para  { text: "All figures are audited." }
        para  { text: revenue_summary  attribute: analyst_agent }
        table { columns: ["Region", "Revenue"]  rows: region_rows  attribute: finance_system }
        chart { kind: bar  series: region_rows  range: "B2:B9"  attribute: finance_system }
    }
}
"#;

// ── 1. Grammar + AST ─────────────────────────────────────────────────────────

#[test]
fn parses_document_into_ast() {
    let prog = parse(GOOD);
    let doc = prog
        .declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Document(x) => Some(x),
            _ => None,
        })
        .expect("document declaration");
    assert_eq!(doc.target, "docx");
    assert_eq!(doc.provenance, "embedded");
    assert_eq!(doc.blocks.len(), 1);
    let section = &doc.blocks[0];
    assert_eq!(section.kind, "section");
    // heading field + 2 para + 1 table + 1 chart child
    assert_eq!(section.children.len(), 4);
}

#[test]
fn good_document_type_checks_clean() {
    let errs = check_errors(GOOD);
    assert!(errs.iter().all(|e| !e.contains("axon-T9")), "unexpected T9xx: {errs:?}");
}

// ── 2. IR lowering ───────────────────────────────────────────────────────────

#[test]
fn lowers_to_ir_documents() {
    let json = ir_json(GOOD);
    assert!(json.contains("\"documents\""));
    assert!(json.contains("quarterly_report"));
    assert!(json.contains("\"kind\":\"chart\""));
}

#[test]
fn document_less_program_elides_field() {
    let json = ir_json("flow F() -> Unit { step S { ask: \"hi\" } }\n");
    assert!(!json.contains("\"documents\""));
}

// ── 3. axon-T916 — the assertion-laundering barrier (the flagship) ──────────

#[test]
fn t916_unattributed_flow_value_in_assertive_slot_is_refused() {
    let src = r#"
document d {
    target: docx
    section { para { text: model_guess } }
}
"#;
    let errs = check_errors(src);
    assert!(
        errs.iter().any(|e| e.contains("axon-T916")),
        "expected the assertion-laundering barrier: {errs:?}"
    );
}

#[test]
fn t916_attribute_satisfies_the_barrier() {
    let src = r#"
document d {
    target: docx
    section { para { text: model_guess  attribute: research_agent } }
}
"#;
    let errs = check_errors(src);
    assert!(!errs.iter().any(|e| e.contains("axon-T916")), "{errs:?}");
}

#[test]
fn t916_epistemic_believe_wrapper_satisfies_the_barrier() {
    let src = r#"
know {
    document d {
        target: docx
        section { para { text: audited_total } }
    }
}
"#;
    let errs = check_errors(src);
    assert!(
        !errs.iter().any(|e| e.contains("axon-T916")),
        "an epistemic-know wrapper vouches ≥ believe: {errs:?}"
    );
}

#[test]
fn t916_literal_text_always_passes() {
    let src = r#"
document d {
    target: docx
    section { para { text: "hand-written prose" } }
}
"#;
    let errs = check_errors(src);
    assert!(!errs.iter().any(|e| e.contains("axon-T916")), "{errs:?}");
}

// ── 4. Structure laws ────────────────────────────────────────────────────────

#[test]
fn t910_bad_target() {
    let errs = check_errors("document d { target: pdf  section { para { text: \"x\" } } }\n");
    assert!(errs.iter().any(|e| e.contains("axon-T910")), "{errs:?}");
}

#[test]
fn t912_slide_in_docx_is_refused() {
    let errs = check_errors("document d { target: docx  slide { layout: \"Title\" } }\n");
    assert!(errs.iter().any(|e| e.contains("axon-T912")), "{errs:?}");
}

#[test]
fn t917_bad_chart_kind() {
    let src = r#"
document d {
    target: docx
    section { chart { kind: smartart  series: s  attribute: a } }
}
"#;
    let errs = check_errors(src);
    assert!(errs.iter().any(|e| e.contains("axon-T917")), "{errs:?}");
}

#[test]
fn t914_unknown_field() {
    let src = r#"
document d {
    target: docx
    section { para { text: "x"  colour: "red" } }
}
"#;
    let errs = check_errors(src);
    assert!(errs.iter().any(|e| e.contains("axon-T914")), "{errs:?}");
}

#[test]
fn t913_sensitive_without_legal() {
    let src = r#"
document d {
    target: docx
    effects: <io, sensitive:health>
    section { para { text: "x" } }
}
"#;
    let errs = check_errors(src);
    assert!(errs.iter().any(|e| e.contains("axon-T913")), "{errs:?}");
}
