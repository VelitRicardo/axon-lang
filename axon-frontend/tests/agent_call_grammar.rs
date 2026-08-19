//! v2.83.0 — `<Agent>(arg, …)` is a step-body statement.
//!
//! The last piece that makes v2.83.0's bounded control loop reachable from
//! source. Until this landed, `agent` was: advertised, type-checked against two
//! closed catalogs, lowered to `IRAgent`, given a real executor — and callable
//! from nothing. v2.83.0 was, briefly and by design, the very shape this cycle
//! exists to remove; the difference was that it was declared and scheduled
//! rather than hidden, and this file is the schedule being kept.
//!
//! **Five ledger rows fell here (22 → 18)** — blocks 7, 9, 11, 15 and 56 — and
//! not one of their recorded reasons named the real blocker. Each needed two or
//! three decisions: this grammar; `run <Flow>(…) with <Persona>`, README's
//! spelling of the persona binder on every `run` it publishes; and completing
//! examples that used tools they never declared or providers outside T948's
//! closed catalog.

use axon_frontend::ast::{Declaration, FlowStep};
use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn parse(src: &str) -> Result<axon_frontend::ast::Program, axon_frontend::parser::ParseError> {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    Parser::new(tokens).parse()
}

fn step_call(program: &axon_frontend::ast::Program) -> &axon_frontend::ast::AgentCallStep {
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Step(st) = s {
                    for op in &st.pix_ops {
                        if let FlowStep::AgentCall(c) = op {
                            return c;
                        }
                    }
                }
            }
        }
    }
    panic!("no agent call on any step body");
}

// ── section 1 — the published form ─────────────────────────────────────────────────

#[test]
fn m3_1_a_bare_agent_call_parses_in_a_step_body() {
    let src = r#"
flow CompetitiveIntelligence(sector: String) -> CompetitiveReport {
    step Research {
        MarketResearcher(sector)
        output: CompetitiveReport
    }
}
"#;
    let program = parse(src).expect("the README's own shape must parse");
    let c = step_call(&program);
    assert_eq!(c.agent_name, "MarketResearcher");
    assert_eq!(c.arguments, vec!["sector".to_string()]);
}

#[test]
fn m3_2_arguments_are_dotted_subjects() {
    // README block 9: `TrendAnalyzer(Gather.output)`. Without v2.83.0's
    // subject rule this would have stopped at the dot.
    let src = r#"
flow F(a: String) -> R {
    step Analyze {
        TrendAnalyzer(Gather.output)
        output: R
    }
}
"#;
    let program = parse(src).expect("parse");
    assert_eq!(
        step_call(&program).arguments,
        vec!["Gather.output".to_string()]
    );
}

#[test]
fn m3_3_multiple_and_literal_arguments() {
    let src = r#"
flow F(a: String) -> R {
    step S {
        Planner(a, Prior.output, "USD", 3)
        output: R
    }
}
"#;
    let program = parse(src).expect("parse");
    let c = step_call(&program);
    assert_eq!(
        c.arguments,
        vec![
            "a".to_string(),
            "Prior.output".to_string(),
            // v2.10.0's classification survives: the quotes distinguish a literal
            // from a binding name all the way to dispatch.
            "\"USD\"".to_string(),
            "3".to_string()
        ]
    );
}

#[test]
fn m3_4_a_zero_argument_call_parses() {
    let src = r#"flow F(a: String) -> R { step S { Scout() output: R } }"#;
    let program = parse(src).expect("parse");
    let c = step_call(&program);
    assert_eq!(c.agent_name, "Scout");
    assert!(c.arguments.is_empty());
}

// ── section 2 — it reaches the IR ──────────────────────────────────────────────────

#[test]
fn m3_5_the_call_survives_to_the_ir() {
    let src = r#"
flow F(sector: String) -> R {
    step Research {
        MarketResearcher(sector)
        output: R
    }
}
"#;
    let program = parse(src).expect("parse");
    let json =
        serde_json::to_string_pretty(&IRGenerator::new().generate(&program)).expect("json");
    assert!(
        json.contains("agent_call") && json.contains("MarketResearcher"),
        "the call must reach the IR — that is what `dispatch_step_statement` \
         reads.\n{json}"
    );
}

// ── section 3 — the call must not shadow the step's own fields ────────────────────

/// The arm is told apart from every field arm by the `(`. Those all match on a
/// specific field NAME, so a call can never shadow one — but the step's own
/// fields must keep working alongside a call, which is what this pins.
#[test]
fn m3_6_the_step_keeps_its_fields_around_the_call() {
    let src = r#"
flow F(sector: String) -> R {
    step Research {
        ask: "frame the question"
        MarketResearcher(sector)
        output: CompetitiveReport
        confidence_floor: 0.8
    }
}
"#;
    let program = parse(src).expect("parse");
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Step(st) = s {
                    assert_eq!(st.ask, "frame the question");
                    assert_eq!(st.output_type, "CompetitiveReport");
                    assert_eq!(st.confidence_floor, Some(0.8));
                    assert_eq!(st.pix_ops.len(), 1, "and exactly one agent call");
                    return;
                }
            }
        }
    }
    panic!("no step");
}

// ── section 4 — `run <Flow>(…) with <Persona>` ────────────────────────────────────

/// README binds the persona with `with` on EVERY `run` it publishes; the
/// parser took only `as`. Same position, same meaning — and the two `with`s in
/// the language cannot collide, because the other one sits after a tool name
/// inside a step body, where this modifier list is never consulted.
#[test]
fn m3_7_run_binds_its_persona_with_the_published_spelling() {
    let src = "flow F(a: String) -> R { step S { ask: \"x\" output: R } }\n\
               persona Analyst { domain: [\"x\"] tone: analytical }\n\
               run F(\"electric vehicles\")\n    with Analyst";
    let program = parse(src).expect("`run … with <Persona>` must parse");
    for d in &program.declarations {
        if let Declaration::Run(r) = d {
            assert_eq!(r.persona, "Analyst");
            return;
        }
    }
    panic!("no run statement");
}

#[test]
fn m3_8_the_as_spelling_still_binds_the_persona() {
    let src = "flow F(a: String) -> R { step S { ask: \"x\" output: R } }\n\
               persona Analyst { domain: [\"x\"] tone: analytical }\n\
               run F(\"x\") as Analyst";
    let program = parse(src).expect("parse");
    for d in &program.declarations {
        if let Declaration::Run(r) = d {
            assert_eq!(r.persona, "Analyst", "back-compat: `as` is unchanged");
            return;
        }
    }
    panic!("no run statement");
}
