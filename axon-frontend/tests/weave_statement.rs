//! v2.83.0 — **`weave [a, b] format: T include: […]` is REAL.**
//!
//! **What was wrong.** Fourteen README examples close with this statement. The
//! parser accepted only the BRACED field form (`weave { sources: […] … }`),
//! which no published block writes; the bracket form fell into
//! `skip_flow_step_structural`.
//!
//! That skipper did worse than drop the node. It stops at the first `output`
//! KEYWORD it meets, so `weave [A.output, B.output]` left the parser **mid
//! list** — the step loop then saw `output` as its own field, demanded a colon,
//! and reported `Expected Colon, found Comma` pointing at a comma that was
//! never the problem. A silent drop that also mislocated its own error, which
//! is how README block 15's ledger row came to be filed under the wrong cause.
//!
//! **What this pins.**
//! - the three published surfaces parse into ONE node;
//!   - `output:` is NOT in the braceless catalog — it belongs to the enclosing
//!     step, and sharing the name is what made the terminator ambiguous in the
//!     first place;
//!   - `include:` reaches the AST and the IR, because a field decided by
//! nothing is the v2.67.0 defect this cycle exists to end;
//!   - sources are DOTTED, since `weave [A.output, B.output]` is the only shape
//!     the README ever writes.

use axon_frontend::ast::{Declaration, FlowStep};
use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn parse(src: &str) -> Result<axon_frontend::ast::Program, axon_frontend::parser::ParseError> {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    Parser::new(tokens).parse()
}

fn ir_json(src: &str) -> String {
    let program = parse(src).expect("parse");
    serde_json::to_string_pretty(&IRGenerator::new().generate(&program)).expect("json")
}

/// The `weave` node on the first step body found, or panic.
fn step_weave(program: &axon_frontend::ast::Program) -> &axon_frontend::ast::WeaveStep {
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Step(st) = s {
                    for op in &st.pix_ops {
                        if let FlowStep::Weave(w) = op {
                            return w;
                        }
                    }
                }
            }
        }
    }
    panic!("no `weave` statement found on any step body");
}

/// README block 1's closing step, verbatim.
const README_SHAPE: &str = r#"
flow AnalyzeContract(doc: Document) -> StructuredReport {
    step Report {
        weave [Extract.output, Check.output]
        format: StructuredReport
        include: [summary, risks, recommendations]
    }
}
"#;

// ── section 1 — the published statement reaches the AST ─────────────────────────────

#[test]
fn f9_1_the_published_step_statement_parses() {
    let program = parse(README_SHAPE).expect("the README's own shape must parse");
    let w = step_weave(&program);
    assert_eq!(
        w.sources,
        vec!["Extract.output".to_string(), "Check.output".to_string()],
        "the sources are DOTTED references to prior steps' outputs"
    );
    assert_eq!(w.format_type, "StructuredReport");
    assert_eq!(
        w.include,
        vec![
            "summary".to_string(),
            "risks".to_string(),
            "recommendations".to_string()
        ],
        "`include:` had no slot at any position before v2.83.0"
    );
}

#[test]
fn f9_2_it_survives_to_the_ir() {
    let json = ir_json(README_SHAPE);
    for needle in ["Extract.output", "recommendations"] {
        assert!(
            json.contains(needle),
            "`{needle}` must reach the IR — before v2.83.0 the whole statement \
             was discarded at PARSE time.\n{json}"
        );
    }
}

// ── section 2 — the terminator bug, from the side that caused it ────────────────────

/// The regression that named this step: `output:` after a weave belongs to
/// the STEP. The old skipper stopped ON that keyword mid-list and the step then
/// failed on the comma. Both halves are asserted — the weave keeps its whole
/// list, and the step keeps its own output type.
#[test]
fn f9_3_a_following_step_output_is_the_steps_not_the_weaves() {
    let src = r#"
flow DeepResearch(question: String) -> ResearchReport {
    step Synthesize {
        weave [Web.output, DB.output]
        output: ResearchReport
    }
}
"#;
    let program = parse(src).expect("this is README block 15's closing step");
    let w = step_weave(&program);
    assert_eq!(
        w.sources.len(),
        2,
        "the source list must survive whole — the old skipper stopped at the \
         `output` inside `Web.output` and left the parser mid-list. Got: {:?}",
        w.sources
    );
    assert!(
        w.format_type.is_empty(),
        "`output:` is the STEP's field; weave must not have eaten it"
    );
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Step(st) = s {
                    assert_eq!(
                        st.output_type, "ResearchReport",
                        "the step keeps its own `output:`"
                    );
                }
            }
        }
    }
}

// ── section 3 — the flow-level surface, same node ───────────────────────────────────

#[test]
fn f9_4_the_flow_level_into_form_parses() {
    // README blocks 3 and 4: `weave [A, B] into Report { format: T }`.
    let src = r#"
flow AnalyzeRisk(contract: Document) -> StructuredReport {
    par {
        step Financial { ask: "a" output: RiskScore }
        step Regulatory { ask: "b" output: ComplianceReport }
    }
    weave [Financial, Regulatory] into Report { format: StructuredReport }
}
"#;
    let program = parse(src).expect("`weave [...] into X { … }` must parse at flow level");
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Weave(w) = s {
                    assert_eq!(w.sources, vec!["Financial".to_string(), "Regulatory".to_string()]);
                    assert_eq!(w.target, "Report");
                    assert_eq!(w.format_type, "StructuredReport");
                    return;
                }
            }
        }
    }
    panic!("the flow-level weave must reach the AST");
}

#[test]
fn f9_5_the_braced_field_form_still_parses() {
    // Pre-v2.83.0 back-compat: the only form the parser used to accept.
    let src = r#"
flow F(a: String) -> R {
    weave { sources: [A, B] target: Out format: R style: formal }
}
"#;
    let program = parse(src).expect("parse");
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Weave(w) = s {
                    assert_eq!(w.sources.len(), 2);
                    assert_eq!(w.target, "Out");
                    assert_eq!(w.style, "formal");
                    return;
                }
            }
        }
    }
    panic!("the braced form must keep working");
}

/// `weave Baz` — the bare positional subject every other statement in the
/// language takes (`probe X`, `reason X`, `validate X`), read as a one-element
/// source list. Not a special case: the uniform rule.
///
/// It is also a compatibility surface. v2.7.0's suite pins this shape, because
/// before v2.83.0 it disappeared into `skip_flow_step_structural` — parsing it
/// keeps every program that wrote it compiling, and now it means something.
#[test]
fn f9_7_a_bare_positional_subject_is_a_single_source() {
    let src = r#"flow F(a: String) -> R { step S { ask: "x" weave Baz output: R } }"#;
    let program = parse(src).expect("parse");
    let w = step_weave(&program);
    assert_eq!(w.sources, vec!["Baz".to_string()]);
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Step(st) = s {
                    assert_eq!(
                        st.output_type, "R",
                        "the bare form must not swallow the step's next field"
                    );
                }
            }
        }
    }
}

// ── section 4 — no IR drift for programs that do not use it ─────────────────────────

#[test]
fn f9_6_a_weave_without_include_has_byte_identical_ir() {
    let json = ir_json("flow F(a: String) -> R { weave { sources: [A] target: Out } }");
    assert!(
        !json.contains("\"include\""),
        "`include` must be ELIDED when empty, or every pre-v2.83.0 IR-SHA \
         moves.\n{json}"
    );
}
