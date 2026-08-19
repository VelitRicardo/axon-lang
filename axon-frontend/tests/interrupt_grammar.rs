//! v2.36.0 — grammar + AST + IR for the `interrupt`/`resume` session step.
//!
//! Four properties, each pinned (the semantics are fixed by the 79.a paper;
//! type-checking — duality under both exits, closed-catalog signal, credit
//! symmetry, DBM bound — is 79.c and is NOT exercised here):
//!
//! 1. `interrupt { <body> } on <Signal> as <sig> resumable { <handler> }` parses
//!    into a `SessionStep { op: "interrupt", … }` with the `body`/`handler` arms,
//!    the signal cause, the binder, and `resumable = true`.
//! 2. The `resume` step parses inside a handler.
//! 3. It lowers to `IRSessionStep` preserving all of the above.
//! 4. **IR-SHA invariance**: a session that does NOT use `interrupt` serializes
//! to JSON with no `binder`/`resumable` keys — byte-identical to pre-v2.36.0 IR
//! (the v2.33.0/v2.34.0 additive-only discipline, section 8.3 gate).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::IRSessionStep;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

/// The canonical barge-in shape: an agent utterance (`send Token` loop) that may
/// be interrupted by caller speech, whose handler acknowledges then resumes.
const BARGE_IN: &str = r#"
session VoiceTurn {
    agent: [
        interrupt {
            send Token,
            send Token
        } on CallerSpeech as cause resumable {
            send Ack,
            resume
        }
    ]
    caller: [
        receive Token
    ]
}
"#;

fn first_interrupt_step(prog: &axon_frontend::ast::Program) -> &axon_frontend::ast::SessionStep {
    let session = prog
        .declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Session(s) => Some(s),
            _ => None,
        })
        .expect("no session declaration");
    let agent = session
        .roles
        .iter()
        .find(|r| r.name == "agent")
        .expect("no agent role");
    agent
        .steps
        .iter()
        .find(|s| s.op == "interrupt")
        .expect("no interrupt step in agent role")
}

#[test]
fn interrupt_step_parses_into_ast() {
    let prog = parse(BARGE_IN);
    let step = first_interrupt_step(&prog);

    assert_eq!(step.op, "interrupt");
    assert_eq!(step.message_type, "CallerSpeech", "the `on <Signal>` cause");
    assert_eq!(step.binder, "cause", "the `as <sig>` binder");
    assert!(step.resumable, "the resumable handler was declared");

    // Two labelled arms: body + handler.
    assert_eq!(step.branches.len(), 2);
    let body = step.branches.iter().find(|b| b.label == "body").expect("body arm");
    assert!(
        step.branches.iter().any(|b| b.label == "handler"),
        "handler arm present"
    );
    assert_eq!(body.steps.len(), 2, "two `send Token` steps in the body");
    assert!(body.steps.iter().all(|s| s.op == "send"));
}

#[test]
fn resume_step_parses_in_handler() {
    let prog = parse(BARGE_IN);
    let step = first_interrupt_step(&prog);
    let handler = step.branches.iter().find(|b| b.label == "handler").unwrap();
    assert_eq!(handler.steps.len(), 2, "send Ack, resume");
    assert_eq!(
        handler.steps.last().unwrap().op,
        "resume",
        "handler's normal exit is `resume`"
    );
}

#[test]
fn interrupt_step_lowers_to_ir() {
    let prog = parse(BARGE_IN);
    let ir = IRGenerator::new().generate(&prog);
    let session = ir.sessions.first().expect("no session in IR");
    let agent = session.roles.iter().find(|r| r.name == "agent").unwrap();
    let step: &IRSessionStep = agent
        .steps
        .iter()
        .find(|s| s.op == "interrupt")
        .expect("no interrupt step in IR");

    assert_eq!(step.message_type, "CallerSpeech");
    assert_eq!(step.binder, "cause");
    assert!(step.resumable);
    assert_eq!(step.branches.len(), 2);

    // The interrupt step's IR JSON DOES carry the new fields.
    let json = serde_json::to_string(step).expect("serialize");
    assert!(json.contains("\"binder\":\"cause\""), "got: {json}");
    assert!(json.contains("\"resumable\":true"), "got: {json}");
}

#[test]
fn non_interrupt_session_has_no_ir_drift() {
    // A plain session with no interrupt: the serialized steps must NOT mention
    // `binder` or `resumable` — byte-identical to pre-v2.36.0 IR.
    let prog = parse(
        r#"
session Ping {
    a: [ send Msg, receive Msg, end ]
    b: [ receive Msg, send Msg, end ]
}
"#,
    );
    let ir = IRGenerator::new().generate(&prog);
    let json = serde_json::to_string(&ir.sessions).expect("serialize");
    assert!(
        !json.contains("binder"),
        "no interrupt ⇒ no `binder` key in IR JSON (IR-SHA stability): {json}"
    );
    assert!(
        !json.contains("resumable"),
        "no interrupt ⇒ no `resumable` key in IR JSON (IR-SHA stability): {json}"
    );
}
