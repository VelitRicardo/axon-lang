//! v2.4.0 — the `requires:` capability scope on `daemon`.
//!
//! A cron-SCHEDULED daemon is a standing autonomous privilege: it fires +
//! invokes flows on its own, with no request principal behind it. It MUST
//! declare its capability scope (`requires: [cap, …]`, the closed slug grammar)
//! so the enterprise supervisor (v2.4.0) can mint a least-privilege per-run
//! principal. A cron daemon with no `requires:` → `axon-E0791`. Event-only
//! daemons are exempt (the pre-v2.4.0 cycle-16 surface).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::IRProgram;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors_of(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "d.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

fn ir_of(src: &str) -> IRProgram {
    let tokens = Lexer::new(src, "d.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    IRGenerator::new().generate(&program)
}

fn has(errs: &[String], needle: &str) -> bool {
    errs.iter().any(|e| e.contains(needle))
}

const SCHEDULED_WITH_REQUIRES: &str = "flow HibernateSession() -> Unit {\n\
     step S { ask: \"x\" output: Unit }\n\
   }\n\
   daemon SessionCleaner {\n\
     goal: \"clean\"\n\
     requires: [flow.execute, memory.write]\n\
     listen \"cron:*/5 * * * *\" as tick { run HibernateSession() }\n\
   }";

#[test]
fn scheduled_daemon_with_requires_is_clean_and_lowers_the_scope() {
    assert!(
        errors_of(SCHEDULED_WITH_REQUIRES).is_empty(),
        "a cron daemon WITH requires must type-check clean: {:?}",
        errors_of(SCHEDULED_WITH_REQUIRES)
    );
    let ir = ir_of(SCHEDULED_WITH_REQUIRES);
    let daemon = ir.daemons.iter().find(|d| d.name == "SessionCleaner").unwrap();
    assert_eq!(
        daemon.requires_capabilities,
        vec!["flow.execute".to_string(), "memory.write".to_string()],
        "requires: must lower to the IR daemon's capability scope"
    );
}

#[test]
fn scheduled_daemon_without_requires_raises_e0791() {
    let src = "daemon SessionCleaner {\n\
                 goal: \"clean\"\n\
                 listen \"cron:*/5 * * * *\" as tick {\n\
                    step S { ask: \"x\" output: Unit }\n\
                 }\n\
               }";
    assert!(
        has(&errors_of(src), "axon-E0791"),
        "a cron daemon with NO requires must raise E0791: {:?}",
        errors_of(src)
    );
}

#[test]
fn event_only_daemon_without_requires_is_exempt() {
    // No cron listener → no standing schedule → requires not forced (cycle-16
    // compat). A typed-channel listener; no E0791.
    let src = "channel UserEvents {}\n\
               daemon Reactor {\n\
                 goal: \"react\"\n\
                 listen UserEvents as e {\n\
                    step S { ask: \"x\" output: Unit }\n\
                 }\n\
               }";
    assert!(
        !has(&errors_of(src), "axon-E0791"),
        "an event-only daemon is exempt from the requires gate: {:?}",
        errors_of(src)
    );
}

#[test]
fn invalid_capability_slug_in_requires_is_a_parse_error() {
    // `Flow.Execute` (uppercase) violates the closed slug grammar.
    let src = "daemon D {\n\
                 goal: \"x\"\n\
                 requires: [Flow.Execute]\n\
                 listen \"cron:0 0 * * *\" as t { step S { ask: \"x\" output: Unit } }\n\
               }";
    let tokens = Lexer::new(src, "d.axon").tokenize().expect("lex");
    let parsed = Parser::new(tokens).parse();
    assert!(
        parsed.is_err(),
        "an invalid capability slug in requires must be a parse error"
    );
}
