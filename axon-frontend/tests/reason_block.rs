//! v2.83.0 — **`reason { given: … ask: "…" depth: N }` is REAL.**
//!
//! **What was wrong, measured on 2.85.0.** This is the README's most-published
//! cognitive block: sixteen occurrences, and in every one of them it is the
//! step's ONLY cognition. It reached the model in none of them.
//!
//!   - At FLOW level the braced form did not parse at all — the parser wanted
//!     `reason <target>` and reported `Expected identifier or keyword value,
//!     found LBrace('{')`.
//!   - Inside a `step { }` body — the only position the README ever writes —
//!     it fell into `skip_flow_step_structural`, which discarded it. The step
//!     then lowered to `"ask": ""`, `"given": ""`, and `axon check` said
//!     `0 errors`.
//!
//! `advertised.rs` attested `reason` as `Real { proof: "pure_shape::run_reason" }`
//! and the attestation was TRUE — of an engine no published program could
//! reach. v2.67.0's own phrase for this: *motor real, cable muerto*. v2.67.0 built
//! `KNOWN_DEBT` to stop exactly this and could not have caught it, because the
//! primitive is genuinely implemented; what was missing was the grammar that
//! reaches it.
//!
//! **What this pins.** The block's field set is CLOSED, and closed in the
//! direction v2.83.0 named: a `reason` that silently loses its `ask:` still
//! compiles and deliberates over nothing — the failure is quiet, so the
//! unrecognised field must be loud. One implementation serves both positions
//! (the the design decision doctrine), so the two cannot drift.

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

/// The `reason` node of the first flow's first step, or panic.
fn step_reason(program: &axon_frontend::ast::Program) -> &axon_frontend::ast::ReasonStep {
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Step(st) = s {
                    for op in &st.pix_ops {
                        if let FlowStep::Reason(r) = op {
                            return r;
                        }
                    }
                }
            }
        }
    }
    panic!("no `reason` statement found on any step body");
}

const README_SHAPE: &str = r#"
flow Investigate(alert: String) -> RiskAssessment {
    step Challenge {
        reason {
            given: findings
            ask: "Is this a genuine anomaly or a known false positive?"
            depth: 3
        }
        output: RiskAssessment
    }
}
"#;

// ── section 1 — the block reaches the AST ───────────────────────────────────────────

#[test]
fn f8_1_the_published_block_form_parses_in_a_step_body() {
    let program = parse(README_SHAPE).expect("the README's own shape must parse");
    let r = step_reason(&program);
    assert_eq!(r.given, "findings");
    assert_eq!(
        r.ask,
        "Is this a genuine anomaly or a known false positive?",
        "the `ask:` IS the deliberation — losing it is the whole defect"
    );
    assert_eq!(r.depth, Some(3));
}

#[test]
fn f8_2_the_prompt_survives_to_the_ir() {
    let json = ir_json(README_SHAPE);
    assert!(
        json.contains("genuine anomaly"),
        "the step's only prompt must reach the IR. Before v2.83.0 this program \
         lowered to `\"ask\": \"\"` and compiled clean.\n{json}"
    );
}

#[test]
fn f8_3_given_takes_all_three_published_shapes() {
    // One ref, a comma list, a bracketed list — all three occur in the README.
    for (src_given, expected) in [
        ("Extract.output", "Extract.output"),
        ("Initialize.output, sessions", "Initialize.output, sessions"),
        (
            "[baseline.topology, current.topology]",
            "[baseline.topology, current.topology]",
        ),
    ] {
        let src = format!(
            "flow F(a: String) -> R {{ step S {{ reason {{ given: {src_given} ask: \"q\" }} output: R }} }}"
        );
        let program = parse(&src).unwrap_or_else(|e| panic!("`given: {src_given}` must parse: {e:?}"));
        assert_eq!(step_reason(&program).given, expected, "given: {src_given}");
    }
}

#[test]
fn f8_4_chain_of_thought_enabled_is_the_strategy_the_readme_spells() {
    let src = r#"
flow F(a: String) -> R {
    step S {
        reason { chain_of_thought: enabled given: a ask: "q" depth: 3 }
        output: R
    }
}
"#;
    let program = parse(src).expect("parse");
    assert_eq!(step_reason(&program).strategy, "chain_of_thought");
}

// ── section 2 — the same node at flow level ─────────────────────────────────────────

#[test]
fn f8_5_the_block_form_also_parses_at_flow_level() {
    // Pre-v2.83.0 this was a hard parse error: the two positions were served
    // by different code and only one of them knew the braced form existed.
    let src = r#"
flow F(alert: String) -> R {
    reason {
        given: alert
        ask: "why?"
        depth: 2
    }
}
"#;
    let program = parse(src).expect("`reason { … }` must parse at flow level too");
    let mut found = false;
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Reason(r) = s {
                    assert_eq!(r.ask, "why?");
                    assert_eq!(r.depth, Some(2));
                    found = true;
                }
            }
        }
    }
    assert!(found, "the flow-level `reason` block must reach the AST");
}

#[test]
fn f8_6_the_positional_form_still_parses() {
    // Back-compat: `reason <target>` predates the block and must keep working.
    let src = "flow F(a: String) -> R { reason claim }";
    let program = parse(src).expect("parse");
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Reason(r) = s {
                    assert_eq!(r.target, "claim");
                    assert!(r.ask.is_empty());
                    return;
                }
            }
        }
    }
    panic!("the positional form must still produce a Reason node");
}

// ── section 3 — the catalog is CLOSED ───────────────────────────────────────────────

#[test]
fn f8_7_an_unknown_field_is_refused_and_named() {
    // v2.83.0's rule: ask which direction the silence fails in. A skipped
    // `ask:` (here misspelled `sak:`) leaves a deliberation with no question —
    // weaker, not louder. So it must be refused, and the refusal must name the
    // key so the author can see their own typo.
    let src = r#"
flow F(a: String) -> R {
    step S {
        reason { given: a sak: "typo" depth: 3 }
        output: R
    }
}
"#;
    let err = parse(src).expect_err("an unrecognised `reason` field must be REFUSED");
    assert!(
        err.message.contains("sak"),
        "the refusal must name the offending key. Got: {}",
        err.message
    );
    assert!(
        err.message.contains("ask"),
        "the refusal must list what IS accepted, or the author cannot act on \
         it. Got: {}",
        err.message
    );
}

#[test]
fn f8_8_a_non_integer_depth_is_refused() {
    let src = r#"
flow F(a: String) -> R {
    step S {
        reason { given: a ask: "q" depth: "three" }
        output: R
    }
}
"#;
    let err = parse(src).expect_err("`depth:` must be an integer");
    assert!(
        err.message.contains("depth"),
        "got: {}",
        err.message
    );
}

// ── section 3b — the field dual ─────────────────────────────────────────────────────

/// `reason: "…"` written as a step FIELD. v2.83.0 gave `navigate` the same dual
/// (field vs statement, told apart by the token after the keyword) for the same
/// reason: both spellings were already in use and one of them did nothing.
///
/// This form was swallowed whole — key AND value — by
/// `skip_flow_step_structural`. Thirty of this repository's OWN type-checker
/// fixtures wrote it, so thirty fixtures declared a step whose cognition was
/// discarded before the checker ever saw it. Reading it as a `reason` whose
/// `ask:` is that value invents nothing: it is the block form with one field.
#[test]
fn f8_10_the_field_form_is_a_one_line_deliberation() {
    let src = r#"flow F(a: String) -> R { step S { reason: "is this ok?" output: R } }"#;
    let program = parse(src).expect("the field form must still parse");
    let r = step_reason(&program);
    assert_eq!(
        r.ask, "is this ok?",
        "`reason: \"…\"` is the one-field spelling of the block — the value is \
         the question, not scenery"
    );
}

#[test]
fn f8_11_the_field_form_survives_to_the_ir() {
    let json = ir_json(r#"flow F(a: String) -> R { step S { reason: "is this ok?" output: R } }"#);
    assert!(
        json.contains("is this ok?"),
        "before v2.83.0 this value was discarded at PARSE time and never \
         reached the IR.\n{json}"
    );
}

#[test]
fn f8_12_the_field_form_also_takes_a_reference() {
    let src = "flow F(a: String) -> R { step S { reason: Extract.output output: R } }";
    let program = parse(src).expect("parse");
    assert_eq!(step_reason(&program).target, "Extract.output");
}

/// A bare `reason` on its own line inside a step body must not swallow the
/// step's NEXT field key as its target. Without the `Colon` lookahead the step
/// failed on a stray `:` two tokens past the real problem — and a diagnostic
/// that points at the wrong token is not an improvement on the silent drop it
/// replaced.
#[test]
fn f8_13_a_bare_reason_does_not_eat_the_next_field() {
    let src = "flow F(a: String) -> R { step S { reason\n output: R } }";
    let program = parse(src).expect("parse");
    let r = step_reason(&program);
    assert!(
        r.target.is_empty(),
        "a bare `reason` has no target — `output` is the STEP's field. Got: {}",
        r.target
    );
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Step(st) = s {
                    assert_eq!(st.output_type, "R", "the step must keep its own `output:`");
                }
            }
        }
    }
}

// ── section 4 — no IR drift for programs that do not use it ─────────────────────────

#[test]
fn f8_9_a_program_without_the_block_form_has_byte_identical_ir() {
    // The v2.22.0/v2.46.0 discipline: new IR fields are elided when empty, so a
    // pre-v2.83.0 program's IR JSON — and therefore its IR-SHA — is unchanged.
    let json = ir_json("flow F(a: String) -> R { reason claim }");
    for key in ["\"given\"", "\"ask\"", "\"depth\""] {
        assert!(
            !json.contains(key),
            "{key} must be ELIDED for a program that does not use the block \
             form, or every existing IR-SHA moves.\n{json}"
        );
    }
}
