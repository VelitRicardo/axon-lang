//! v2.22.0 — `axon-T809`: a `requires_context:` no model could ever satisfy is
//! a compile-time error (caught at `axon check`, not at deploy or 2am in a tick).

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog).check().into_iter().map(|e| e.message).collect()
}

fn flow_with(requires: &str) -> String {
    format!(
        r#"
flow F() -> String {{
    step S {{ given: h  ask: "x"  requires_context: {requires}  output: String }}
    return "ok"
}}
"#
    )
}

#[test]
fn requirement_above_the_largest_window_is_t809() {
    // 2,000,000 > the largest canonical window (gemini 1,048,576) → impossible.
    let errs = errors(&flow_with("2000000"));
    let t809: Vec<&String> = errs.iter().filter(|e| e.contains("axon-T809")).collect();
    assert_eq!(t809.len(), 1, "an impossible requirement must be T809; got {errs:?}");
    assert!(t809[0].contains("exceeds"), "names the ceiling: {}", t809[0]);
}

#[test]
fn zero_requirement_is_t809() {
    let errs = errors(&flow_with("0"));
    assert!(
        errs.iter().any(|e| e.contains("axon-T809") && e.contains("positive")),
        "a zero requirement must be T809; got {errs:?}"
    );
}

#[test]
fn a_satisfiable_requirement_is_clean() {
    // 32000 fits within several backends' windows → no T809.
    let errs = errors(&flow_with("32000"));
    assert!(
        !errs.iter().any(|e| e.contains("axon-T809")),
        "a satisfiable requirement must type-check clean; got {errs:?}"
    );
}
