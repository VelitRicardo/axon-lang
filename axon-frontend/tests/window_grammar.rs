//! v2.27.0 — the `window` temporal execution-window primitive: grammar →
//! AST → IR + the closed-catalog type checks (`axon-T820`–`T824`).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::IRWindow;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn first_window(src: &str) -> IRWindow {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    let ir = IRGenerator::new().generate(&prog);
    ir.windows.first().cloned().expect("a window in the IR")
}

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog).check().into_iter().map(|e| e.message).collect()
}

const BUSINESS_HOURS: &str = "window BusinessHours {\n\
     timezone: \"America/Bogota\"\n\
     allow: [ { days: Mon..Fri, hours: 9..18 } ]\n\
     on_outside: defer\n\
   }";

// ── Grammar → IR ────────────────────────────────────────────────────────────

#[test]
fn window_lowers_to_ir() {
    let w = first_window(BUSINESS_HOURS);
    assert_eq!(w.name, "BusinessHours");
    assert_eq!(w.timezone, "America/Bogota");
    assert_eq!(w.on_outside, "defer");
    assert_eq!(w.allow.len(), 1);
    let s = &w.allow[0];
    assert_eq!(s.day_start, "Mon");
    assert_eq!(s.day_end, "Fri");
    assert_eq!(s.hour_start, 9);
    assert_eq!(s.hour_end, 18);
}

#[test]
fn multiple_spans_and_exclude_and_default_policy() {
    let src = "window Ops {\n\
        timezone: \"UTC\"\n\
        allow: [ { days: Mon..Fri, hours: 9..17 }, { days: Sat..Sat, hours: 10..14 } ]\n\
        exclude: [ \"2026-12-25\", \"2026-01-01\" ]\n\
      }";
    let w = first_window(src);
    assert_eq!(w.allow.len(), 2);
    // v2.27.0 — excluded holidays are ISO date literals (part of the program).
    assert_eq!(w.exclude, vec!["2026-12-25".to_string(), "2026-01-01".to_string()]);
    // an omitted on_outside lowers to the safe default `defer`.
    assert_eq!(w.on_outside, "defer");
}

#[test]
fn business_hours_type_checks_clean() {
    assert!(errors(BUSINESS_HOURS).is_empty(), "{:?}", errors(BUSINESS_HOURS));
}

// ── Closed-catalog type errors ──────────────────────────────────────────────

fn has(errs: &[String], code: &str) -> bool {
    errs.iter().any(|e| e.contains(code))
}

#[test]
fn invalid_timezone_is_t820() {
    let src = "window W { timezone: \"Bogota\"  allow: [ { days: Mon..Fri, hours: 9..18 } ] }";
    assert!(has(&errors(src), "axon-T820"), "{:?}", errors(src));
}

#[test]
fn empty_allow_is_t821() {
    let src = "window W { timezone: \"UTC\"  allow: [] }";
    assert!(has(&errors(src), "axon-T821"), "{:?}", errors(src));
}

#[test]
fn invalid_day_is_t822() {
    let src = "window W { timezone: \"UTC\"  allow: [ { days: Funday..Fri, hours: 9..18 } ] }";
    assert!(has(&errors(src), "axon-T822"), "{:?}", errors(src));
}

#[test]
fn out_of_range_hour_is_t823() {
    let src = "window W { timezone: \"UTC\"  allow: [ { days: Mon..Fri, hours: 9..25 } ] }";
    assert!(has(&errors(src), "axon-T823"), "{:?}", errors(src));
}

#[test]
fn unknown_on_outside_is_t824() {
    let src = "window W { timezone: \"UTC\"  allow: [ { days: Mon..Fri, hours: 9..18 } ]  on_outside: maybe }";
    assert!(has(&errors(src), "axon-T824"), "{:?}", errors(src));
}

// ── v2.27.0 — exclude holiday dates ──────────────────────────────────────

#[test]
fn valid_exclude_dates_type_check_clean() {
    let src = "window W { timezone: \"UTC\"  allow: [ { days: Mon..Fri, hours: 9..18 } ]  \
               exclude: [ \"2026-12-25\", \"2026-01-01\", \"2028-02-29\" ] }";
    assert!(errors(src).is_empty(), "{:?}", errors(src));
}

#[test]
fn malformed_exclude_date_is_t826() {
    // Wrong shape (not YYYY-MM-DD).
    let src = "window W { timezone: \"UTC\"  allow: [ { days: Mon..Fri, hours: 9..18 } ]  \
               exclude: [ \"Dec 25\" ] }";
    assert!(has(&errors(src), "axon-T826"), "{:?}", errors(src));
}

#[test]
fn impossible_calendar_date_is_t826() {
    // Real shape, impossible day — Feb 30 and a non-leap Feb 29.
    for bad in ["2026-02-30", "2026-02-29", "2026-13-01", "2026-00-10", "2026-04-31"] {
        let src = format!(
            "window W {{ timezone: \"UTC\"  allow: [ {{ days: Mon..Fri, hours: 9..18 }} ]  \
             exclude: [ \"{bad}\" ] }}"
        );
        assert!(has(&errors(&src), "axon-T826"), "{bad} should be T826: {:?}", errors(&src));
    }
}

#[test]
fn exclude_lowers_to_ir_dates() {
    let src = "window W { timezone: \"UTC\"  allow: [ { days: Mon..Fri, hours: 9..18 } ]  \
               exclude: [ \"2026-12-25\" ] }";
    let w = first_window(src);
    assert_eq!(w.exclude, vec!["2026-12-25".to_string()]);
}
