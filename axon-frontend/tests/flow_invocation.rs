//! v2.4.0 (layer 1) — `run <Flow>(args)` as a flow-step: invoke a declared
//! flow from inside a body, notably a `daemon`'s `listen` handler (brief #32 Q3).
//!
//! Pre-v2.4.0, `run` was a TOP-LEVEL statement only — `parse_flow_step` rejected
//! it inside a body ("Unexpected token in flow body: 'run'"). v2.4.0 routes `run`
//! to the existing `parse_run`, lowers it to `IRFlowNode::Run(IRRun)`, and
//! type-checks it (the invoked flow must be declared). The actual recursive
//! flow dispatch under the daemon's identity is the v2.4.0 daemon executor; this
//! file pins the LANGUAGE surface (parse + lower + check).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::{IRFlowNode, IRProgram};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn ir_of(src: &str) -> IRProgram {
    let tokens = Lexer::new(src, "r.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    IRGenerator::new().generate(&program)
}

fn errors_of(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "r.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

// The cron daemon declares `requires:` (v2.4.0 `axon-E0791` — a scheduled
// privilege must be explicit); these checks isolate the `run`-in-handler path.
const CLEANER: &str = "flow HibernateSession() -> Unit {\n\
     step Hibernate { ask: \"hibernate idle sessions\" output: Unit }\n\
   }\n\
   daemon SessionCleaner {\n\
     goal: \"hibernate idle sessions\"\n\
     requires: [flow.execute]\n\
     listen \"cron:*/5 * * * *\" as tick {\n\
        run HibernateSession()\n\
     }\n\
   }";

#[test]
fn run_in_a_daemon_handler_lowers_to_a_run_node() {
    let ir = ir_of(CLEANER);
    let daemon = ir
        .daemons
        .iter()
        .find(|d| d.name == "SessionCleaner")
        .expect("daemon");
    let body = &daemon.listeners[0].body;
    let run = body
        .iter()
        .find_map(|n| match n {
            IRFlowNode::Run(r) => Some(r),
            _ => None,
        })
        .expect("the handler body must lower `run` to an IRFlowNode::Run");
    assert_eq!(run.flow_name, "HibernateSession");
}

#[test]
fn run_in_a_handler_is_type_checked_clean_when_the_flow_exists() {
    // The whole point: the adopter's exact shape type-checks with 0 errors.
    assert!(
        errors_of(CLEANER).is_empty(),
        "a daemon cron handler invoking a declared flow must be clean: {:?}",
        errors_of(CLEANER)
    );
}

#[test]
fn run_an_undefined_flow_in_a_handler_errors() {
    // Proof the handler body's `run` is checked: an undefined flow is rejected.
    let src = CLEANER.replace("run HibernateSession()", "run NoSuchFlow()");
    let errs = errors_of(&src);
    assert!(
        errs.iter().any(|e| e.contains("Undefined flow 'NoSuchFlow'")),
        "an undefined flow in a handler must be caught: {errs:?}"
    );
}

#[test]
fn run_works_as_a_plain_flow_step_too() {
    // `run` is now a flow-step anywhere a body is parsed, not just in daemons.
    let src = "flow Sub() -> Unit { step S { ask: \"x\" output: Unit } }\n\
               flow Caller() -> Unit {\n\
                  run Sub()\n\
               }";
    let ir = ir_of(src);
    let caller = ir.flows.iter().find(|f| f.name == "Caller").expect("Caller");
    assert!(
        caller
            .steps
            .iter()
            .any(|n| matches!(n, IRFlowNode::Run(r) if r.flow_name == "Sub")),
        "`run Sub()` must lower to a Run node in the flow body"
    );
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}
