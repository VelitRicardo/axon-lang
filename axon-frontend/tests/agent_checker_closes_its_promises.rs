//! The `agent` primitive's compile-time promises, made true — and gated here.
//!
//! The README has said since 2.x that the compiler rejects agents with
//! unbounded budgets and that the type checker validates an agent's return
//! type. An outside reviewer measured 4.0.0 against that: a missing
//! `max_iterations` passed `axon check`; a typo (`max_iteration: 6`) was
//! skipped in silence; `strategy: custom` with steps parsed clean (the body
//! was dropped) and was refused at dispatch; `return:` reached no AST field;
//! a call to an undeclared agent passed `check` and failed at run time. Every
//! one of those is now a compile-time refusal:
//!
//! | rule | what it refuses |
//! | --- | --- |
//! | parse error | an unknown field inside `agent { }` (closed catalogue) |
//! | axon-T1216 | no `max_iterations`, or a non-positive one — ONE predicate with savant's axon-T877 |
//! | axon-T1217 | `strategy: custom` without steps; steps under any other strategy |
//! | axon-T1218 | `<Agent>(…)` naming no declared agent, or a non-agent symbol |
//! | axon-T1219 | `return:` naming a non-type; a calling step whose `output:` disagrees |
//! | axon-T1220 | `max_time:` that is not a duration (`500ms`, `30s`, `2m`, `1h`) |
//!
//! The runtime half (the loop enforces `max_time`, validates `return:`,
//! executes `custom`) is gated in `axon-rs/tests/agent_loop_closes_its_promises.rs`.

use axon_frontend::ast::Declaration;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::{parse_duration_ms, TypeChecker};

fn parse(src: &str) -> Result<axon_frontend::ast::Program, axon_frontend::parser::ParseError> {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    Parser::new(tokens).parse()
}

fn check(src: &str) -> Vec<String> {
    let program = parse(src).expect("parses");
    TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

fn codes(errs: &[String], code: &str) -> Vec<String> {
    errs.iter().filter(|m| m.contains(code)).cloned().collect()
}

const GOOD: &str = r#"
tool WebSearch { provider: http timeout: 10s }
type Report { total: Integer }

agent Researcher {
    goal: "find things"
    tools: [WebSearch]
    strategy: react
    max_iterations: 6
    max_time: 30s
    on_stuck: retry
    return: Report
}

flow F(q: String) -> Report {
    step Research {
        Researcher(q)
        output: Report
    }
}
"#;

#[test]
fn the_readme_shape_is_clean() {
    let errs = check(GOOD);
    assert!(errs.is_empty(), "the canonical agent program must check clean: {errs:#?}");
}

// ── the closed catalogue (parser) ───────────────────────────────────────────

#[test]
fn a_typo_d_field_is_a_parse_error_naming_the_catalogue() {
    let src = GOOD.replace("max_iterations: 6", "max_iteration: 6");
    let err = parse(&src).expect_err("a typo'd field must not be skipped in silence");
    assert!(
        err.message.contains("max_iteration") && err.message.contains("closed catalog"),
        "the error names the field and the catalogue: {}",
        err.message
    );
    assert!(
        err.message.contains("max_iterations"),
        "…and lists the spelling the author meant: {}",
        err.message
    );
}

#[test]
fn return_is_a_real_field_and_the_arrow_position_also_works() {
    let p = parse(GOOD).expect("parses");
    let agent = p.declarations.iter().find_map(|d| match d {
        Declaration::Agent(a) => Some(a),
        _ => None,
    });
    assert_eq!(agent.unwrap().return_type, "Report", "`return:` reaches the AST");

    let arrow = GOOD
        .replace("agent Researcher {", "agent Researcher(q: String) -> Report {")
        .replace("    return: Report\n", "");
    let p = parse(&arrow).expect("the signature position parses");
    let agent = p.declarations.iter().find_map(|d| match d {
        Declaration::Agent(a) => Some(a),
        _ => None,
    });
    assert_eq!(agent.unwrap().return_type, "Report", "`-> T` reaches the same field");
}

#[test]
fn the_custom_body_survives_parsing() {
    let src = r#"
agent OnboardingGuide {
    goal: "onboard"
    strategy: custom
    max_iterations: 12
    on_stuck: retry

    step Greet { ask: "Welcome the user and assess their goals" }
    step Configure { ask: "Recommend workspace configuration" }
    step Train { ask: "Generate personalized tutorial sequence" }
}
"#;
    let p = parse(src).expect("README use case 3 parses");
    let agent = p.declarations.iter().find_map(|d| match d {
        Declaration::Agent(a) => Some(a),
        _ => None,
    }).unwrap();
    let names: Vec<&str> = agent.body.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["Greet", "Configure", "Train"], "the body is the written sequence, in order");
    assert!(check(src).is_empty(), "…and it checks clean");
}

// ── axon-T1216 — the termination bound is required ──────────────────────────

#[test]
fn t1216_a_missing_max_iterations_is_a_check_error() {
    let src = GOOD.replace("    max_iterations: 6\n", "");
    let errs = check(&src);
    let hits = codes(&errs, "axon-T1216");
    assert_eq!(hits.len(), 1, "exactly one T1216: {errs:#?}");
    assert!(hits[0].contains("Researcher") && hits[0].contains("max_iterations"), "{}", hits[0]);
}

#[test]
fn t1216_a_zero_bound_is_refused_too() {
    let src = GOOD.replace("max_iterations: 6", "max_iterations: 0");
    let errs = check(&src);
    assert_eq!(codes(&errs, "axon-T1216").len(), 1, "{errs:#?}");
    assert!(
        !errs.iter().any(|m| m.contains("max_iterations must be >= 1")),
        "the old present-only rule is retired in favour of the one predicate: {errs:#?}"
    );
}

#[test]
fn t1216_and_t877_are_one_predicate() {
    // savant's budget rule and agent's rule emit the SAME wording after their
    // own code+subject prefix — the proof that neither can drift.
    let agent_src = GOOD.replace("    max_iterations: 6\n", "");
    let agent_msg = codes(&check(&agent_src), "axon-T1216").remove(0);
    let savant_src = "savant S { domain: \"d\" mandate m { objective: \"o\", output: T } }";
    let savant_errs = check(savant_src);
    let savant_msg = codes(&savant_errs, "axon-T877");
    assert!(!savant_msg.is_empty(), "savant without a budget is T877: {savant_errs:#?}");
    let tail = |m: &str| m.split(" — ").nth(1).unwrap_or("").to_string();
    assert_eq!(
        tail(&agent_msg),
        tail(&savant_msg[0]),
        "same predicate, same sentence:\n  {agent_msg}\n  {}",
        savant_msg[0]
    );
}

// ── axon-T1217 — custom and its body ────────────────────────────────────────

#[test]
fn t1217_custom_without_steps_is_refused() {
    let src = GOOD.replace("strategy: react", "strategy: custom");
    let errs = check(&src);
    let hits = codes(&errs, "axon-T1217");
    assert_eq!(hits.len(), 1, "{errs:#?}");
    assert!(hits[0].contains("custom") && hits[0].contains("step"), "{}", hits[0]);
}

#[test]
fn t1217_steps_under_react_are_refused_not_silently_ignored() {
    let src = GOOD.replace(
        "    return: Report\n}",
        "    return: Report\n    step Greet { ask: \"hi\" }\n}",
    );
    let errs = check(&src);
    let hits = codes(&errs, "axon-T1217");
    assert_eq!(hits.len(), 1, "{errs:#?}");
    assert!(hits[0].contains("react"), "names the strategy that would ignore them: {}", hits[0]);
}

// ── axon-T1218 — the call resolves to a declared agent ──────────────────────

#[test]
fn t1218_a_call_to_an_undeclared_agent_is_a_check_error() {
    let src = GOOD.replace("        Researcher(q)", "        NoExiste(q)");
    let errs = check(&src);
    let hits = codes(&errs, "axon-T1218");
    assert_eq!(hits.len(), 1, "{errs:#?}");
    assert!(hits[0].contains("NoExiste") && hits[0].contains("not declared"), "{}", hits[0]);
}

#[test]
fn t1218_a_call_to_a_non_agent_symbol_is_a_check_error() {
    let src = GOOD.replace("        Researcher(q)", "        WebSearch(q)");
    let errs = check(&src);
    let hits = codes(&errs, "axon-T1218");
    assert_eq!(hits.len(), 1, "{errs:#?}");
    assert!(hits[0].contains("calls a tool, not an agent"), "{}", hits[0]);
}

// ── axon-T1219 — return type ────────────────────────────────────────────────

#[test]
fn t1219_return_naming_a_non_type_is_refused() {
    let src = GOOD.replace("return: Report", "return: WebSearch");
    let errs = check(&src);
    let hits = codes(&errs, "axon-T1219");
    assert!(!hits.is_empty(), "{errs:#?}");
    assert!(hits[0].contains("is a tool, not a type"), "{}", hits[0]);
}

#[test]
fn t1219_the_calling_step_must_agree_with_the_agent_s_return() {
    let src = GOOD.replace("        output: Report", "        output: String");
    let errs = check(&src);
    let hits = codes(&errs, "axon-T1219");
    assert_eq!(hits.len(), 1, "{errs:#?}");
    assert!(
        hits[0].contains("output: String") && hits[0].contains("`return:` is `Report`"),
        "{}",
        hits[0]
    );
}

#[test]
fn t1219_an_undeclared_return_name_is_soft_typed_like_every_other_type_reference() {
    // The house discipline: an unknown type name may be a builtin or an
    // imported type. Declared non-types and call-site disagreement are the
    // refusals; a bare unknown name is not.
    let src = GOOD.replace("return: Report", "return: Informe").replace("output: Report", "output: Informe");
    let errs = check(&src);
    assert!(codes(&errs, "axon-T1219").is_empty(), "{errs:#?}");
}

// ── axon-T1220 — max_time is a duration ─────────────────────────────────────

#[test]
fn t1220_a_non_duration_max_time_is_refused() {
    let src = GOOD.replace("max_time: 30s", "max_time: soon");
    let errs = check(&src);
    let hits = codes(&errs, "axon-T1220");
    assert_eq!(hits.len(), 1, "{errs:#?}");
    assert!(hits[0].contains("soon") && hits[0].contains("30s"), "{}", hits[0]);
}

#[test]
fn the_duration_grammar_is_one_definition() {
    assert_eq!(parse_duration_ms("500ms"), Some(500));
    assert_eq!(parse_duration_ms("30s"), Some(30_000));
    assert_eq!(parse_duration_ms("2m"), Some(120_000));
    assert_eq!(parse_duration_ms("1h"), Some(3_600_000));
    assert_eq!(parse_duration_ms("0ms"), Some(0));
    assert_eq!(parse_duration_ms("soon"), None);
    assert_eq!(parse_duration_ms("30"), None, "a bare number has no unit");
    assert_eq!(parse_duration_ms("30 s"), None, "no whitespace inside the literal");
    assert_eq!(parse_duration_ms(""), None);
}
