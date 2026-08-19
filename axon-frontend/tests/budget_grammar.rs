//! v2.28.0 — the `budget { … }` linear-effect rate-limit block on `daemon`:
//! grammar → AST → IR + the closed-catalog type checks (`axon-T830`–`T834`).

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

const TOOL: &str = "tool TelnyxCall { provider: http timeout: 5s }";
const FLOW: &str = "flow SendBatch() -> Unit { step S { ask: \"x\" output: Unit } }";

fn program(daemon_body: &str) -> String {
    format!(
        "{TOOL}\n{FLOW}\n\
         daemon OutboundScheduler {{\n\
           requires: [flow.execute]\n\
           {daemon_body}\n\
           listen \"cron:*/5 * * * *\" as t {{ run SendBatch() }}\n\
         }}"
    )
}

// ── Grammar → IR ────────────────────────────────────────────────────────────

#[test]
fn budget_lowers_to_ir() {
    let d = first_daemon(&program(
        "budget {\n\
           rate: 8 per hour on Tool(TelnyxCall)\n\
           max: 50 per day on Tool(TelnyxCall)\n\
           on_exhausted: defer\n\
         }",
    ));
    let b = d.budget.expect("budget lowered");
    assert_eq!(b.on_exhausted, "defer");
    assert_eq!(b.quotas.len(), 2);
    assert_eq!(b.quotas[0].kind, "rate");
    assert_eq!(b.quotas[0].limit, 8);
    assert_eq!(b.quotas[0].period, "hour");
    assert_eq!(b.quotas[0].effect, "TelnyxCall");
    assert_eq!(b.quotas[1].kind, "max");
    assert_eq!(b.quotas[1].limit, 50);
    assert_eq!(b.quotas[1].period, "day");
}

#[test]
fn omitted_on_exhausted_lowers_to_block() {
    let d = first_daemon(&program(
        "budget { rate: 4 per minute on Tool(TelnyxCall) }",
    ));
    // The fail-closed default: never over-emit.
    assert_eq!(d.budget.unwrap().on_exhausted, "block");
}

#[test]
fn budgetless_daemon_has_none_and_omits_json() {
    let d = first_daemon(&format!(
        "{FLOW}\n\
         daemon Plain {{\n\
           requires: [flow.execute]\n\
           listen \"cron:*/5 * * * *\" as t {{ run SendBatch() }}\n\
         }}"
    ));
    assert!(d.budget.is_none());
    let json = serde_json::to_string(&d).expect("serialize");
    assert!(!json.contains("budget"), "a budgetless daemon must not serialize a budget key");
}

// ── Type-check (axon-T830–T834) ─────────────────────────────────────────────

#[test]
fn valid_budget_type_checks_clean() {
    let errs = errors(&program(
        "budget {\n\
           rate: 8 per hour on Tool(TelnyxCall)\n\
           max: 50 per day on Tool(TelnyxCall)\n\
           on_exhausted: shed\n\
         }",
    ));
    assert!(errs.is_empty(), "expected clean, got {errs:?}");
}

#[test]
fn undefined_tool_is_t830() {
    let errs = errors(&program(
        "budget { rate: 8 per hour on Tool(NoSuchTool) }",
    ));
    assert!(
        errs.iter().any(|e| e.contains("axon-T830") && e.contains("NoSuchTool")),
        "{errs:?}"
    );
}

#[test]
fn non_positive_limit_is_t831() {
    let errs = errors(&program("budget { rate: 0 per hour on Tool(TelnyxCall) }"));
    assert!(errs.iter().any(|e| e.contains("axon-T831")), "{errs:?}");
}

#[test]
fn unknown_period_is_t832() {
    let errs = errors(&program("budget { rate: 8 per fortnight on Tool(TelnyxCall) }"));
    assert!(errs.iter().any(|e| e.contains("axon-T832")), "{errs:?}");
}

#[test]
fn unknown_on_exhausted_is_t833() {
    let errs = errors(&program(
        "budget { rate: 8 per hour on Tool(TelnyxCall) on_exhausted: explode }",
    ));
    assert!(errs.iter().any(|e| e.contains("axon-T833")), "{errs:?}");
}

#[test]
fn empty_budget_is_t834() {
    let errs = errors(&program("budget { on_exhausted: block }"));
    assert!(errs.iter().any(|e| e.contains("axon-T834")), "{errs:?}");
}
