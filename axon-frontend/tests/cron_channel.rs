//! v2.4.0 — the cron-channel surface: `listen "cron:<expr>" as t { … }`.
//!
//! A `listen` whose channel is `"cron:<expr>"` is a first-class TIME trigger
//! (not a legacy string topic). The type-checker validates the 5-field cron
//! expression (so a schedule that type-checks is one the v2.4.0 TimerSource can
//! fire), rejects a malformed schedule (`axon-E0789`), and rejects a scheduled
//! trigger with no handler body (`axon-E0792`). It must NOT emit the cycle-13
//! string-topic deprecation warning for a cron channel.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn check(src: &str) -> (Vec<String>, Vec<String>) {
    let tokens = Lexer::new(src, "c.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    let (errs, warns) = TypeChecker::new(&program).check_with_warnings();
    (
        errs.into_iter().map(|e| e.message).collect(),
        warns.into_iter().map(|w| w.message).collect(),
    )
}

fn has(v: &[String], needle: &str) -> bool {
    v.iter().any(|s| s.contains(needle))
}

// NOTE: a cron-scheduled daemon MUST declare `requires:` (v2.4.0 `axon-E0791`
// — a standing scheduled privilege must be explicit). This fixture carries it so
// the cron-channel checks below isolate the cron grammar, not the scope gate.
const CRON_DAEMON: &str = "daemon SessionCleaner {\n\
     goal: \"hibernate idle sessions\"\n\
     requires: [flow.execute]\n\
     listen \"cron:*/5 * * * *\" as tick {\n\
        step Hibernate { ask: \"hibernate idle sessions\" output: Unit }\n\
     }\n\
   }";

#[test]
fn valid_cron_daemon_is_clean_and_not_deprecated() {
    let (errs, warns) = check(CRON_DAEMON);
    assert!(errs.is_empty(), "a valid cron daemon must type-check clean: {errs:?}");
    assert!(
        !has(&warns, "deprecated"),
        "a cron channel is a time trigger, not a legacy string topic — no deprecation warning: {warns:?}"
    );
}

#[test]
fn malformed_cron_raises_e0789() {
    // minute 99 is out of range (0–59).
    let src = CRON_DAEMON.replace("cron:*/5 * * * *", "cron:99 * * * *");
    let (errs, _) = check(&src);
    assert!(has(&errs, "axon-E0789"), "a malformed cron must raise E0789: {errs:?}");
}

#[test]
fn cron_with_wrong_field_count_raises_e0789() {
    // Four fields, not five.
    let src = CRON_DAEMON.replace("cron:*/5 * * * *", "cron:*/5 * * *");
    let (errs, _) = check(&src);
    assert!(has(&errs, "axon-E0789"), "wrong field count must raise E0789: {errs:?}");
}

#[test]
fn cron_listener_without_a_body_raises_e0792() {
    // A scheduled trigger with no handler is a no-op. (E0792 since the v2.23.0
    // witness well-formedness check now owns E0790 — one code, one meaning.)
    let src = "daemon SessionCleaner {\n\
                 goal: \"x\"\n\
                 listen \"cron:*/5 * * * *\" as tick\n\
               }";
    let (errs, _) = check(src);
    assert!(
        has(&errs, "axon-E0792"),
        "a cron listener with no body must raise E0792: {errs:?}"
    );
    assert!(
        !has(&errs, "axon-E0790"),
        "must NOT collide with witness E0790: {errs:?}"
    );
}

#[test]
fn non_cron_string_topic_warns_never_fires() {
    // v2.4.0 — the cron branch must not suppress the honesty warning for
    // an ordinary (non-cron) string topic: a daemon fires only on `cron:`,
    // so a topic listener NEVER fires → `axon-W009` (replaces the pre-52.g
    // "deprecated, migrate to typed channel" warning, which pointed at a
    // form that also never fires).
    let src = "daemon D {\n\
                 goal: \"x\"\n\
                 listen \"ticks\" as e {\n\
                    step S { ask: \"do\" output: Unit }\n\
                 }\n\
               }";
    let (_, warns) = check(src);
    assert!(
        has(&warns, "axon-W009"),
        "a non-cron string topic gets the v2.4.0 never-fires warning: {warns:?}"
    );
}
