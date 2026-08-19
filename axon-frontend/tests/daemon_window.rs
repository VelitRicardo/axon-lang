//! v2.27.0 — the daemon `window:` temporal binding: grammar → AST → IR +
//! the `axon-T825` reference check (the bound name must be a declared `window`).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::IRDaemon;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn first_daemon(src: &str) -> IRDaemon {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    let ir = IRGenerator::new().generate(&prog);
    ir.daemons.first().cloned().expect("a daemon in the IR")
}

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog).check().into_iter().map(|e| e.message).collect()
}

const WINDOW: &str = "window BusinessHours {\n\
     timezone: \"America/Bogota\"\n\
     allow: [ { days: Mon..Fri, hours: 9..18 } ]\n\
     on_outside: skip\n\
   }";

const FLOW: &str = "flow Send() -> Unit { step S { ask: \"x\" output: Unit } }";

fn program(daemon: &str) -> String {
    format!("{FLOW}\n{WINDOW}\n{daemon}")
}

// ── Grammar → IR ────────────────────────────────────────────────────────────

#[test]
fn daemon_window_binding_lowers_to_ir() {
    let d = first_daemon(&program(
        "daemon Scheduler {\n\
           window: BusinessHours\n\
           requires: [flow.execute]\n\
           listen \"cron:*/5 * * * *\" as tick { run Send() }\n\
         }",
    ));
    assert_eq!(d.name, "Scheduler");
    assert_eq!(d.window_ref, "BusinessHours");
}

#[test]
fn windowless_daemon_has_empty_window_ref() {
    // Zero-drift: a daemon with no `window:` keeps an empty binding (and the
    // field is skipped from JSON entirely — pre-v2.27.0 byte-identical).
    let d = first_daemon(&format!(
        "{FLOW}\n\
         daemon Plain {{\n\
           requires: [flow.execute]\n\
           listen \"cron:*/5 * * * *\" as tick {{ run Send() }}\n\
         }}"
    ));
    assert_eq!(d.window_ref, "");
    let json = serde_json::to_string(&d).expect("serialize");
    assert!(!json.contains("window_ref"), "empty window_ref must not serialize");
}

// ── Type-check (axon-T825) ──────────────────────────────────────────────────

#[test]
fn bound_window_clean() {
    let errs = errors(&program(
        "daemon Scheduler {\n\
           window: BusinessHours\n\
           requires: [flow.execute]\n\
           listen \"cron:*/5 * * * *\" as tick { run Send() }\n\
         }",
    ));
    assert!(errs.is_empty(), "expected clean, got {errs:?}");
}

#[test]
fn undefined_window_is_t825() {
    let errs = errors(&format!(
        "{FLOW}\n\
         daemon Scheduler {{\n\
           window: NoSuchWindow\n\
           requires: [flow.execute]\n\
           listen \"cron:*/5 * * * *\" as tick {{ run Send() }}\n\
         }}"
    ));
    assert!(
        errs.iter().any(|e| e.contains("axon-T825") && e.contains("NoSuchWindow")),
        "expected axon-T825 for undefined window, got {errs:?}"
    );
}

#[test]
fn window_ref_to_non_window_is_t825() {
    // Binding `window:` to a flow name (not a window) is a kind mismatch.
    let errs = errors(&format!(
        "{FLOW}\n\
         daemon Scheduler {{\n\
           window: Send\n\
           requires: [flow.execute]\n\
           listen \"cron:*/5 * * * *\" as tick {{ run Send() }}\n\
         }}"
    ));
    assert!(
        errs.iter().any(|e| e.contains("axon-T825")),
        "expected axon-T825 for kind mismatch, got {errs:?}"
    );
}
