//! v2.26.0 — compile-time const-folding of `if` conditions. A condition that
//! folds to a constant statically decides the branch → `axon-W008` (dead-branch
//! warning). A condition with any reference / field / index / call is dynamic
//! and never warns.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn warnings(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    let (_errs, warns) = TypeChecker::new(&prog).check_with_warnings();
    warns.into_iter().map(|w| w.message).collect()
}

fn flow(params: &str, cond: &str) -> String {
    format!("flow F({params}) -> String {{\n  if {cond} {{ probe x }}\n  return \"ok\"\n}}")
}

fn has_w008(src: &str) -> bool {
    warnings(src).iter().any(|w| w.contains("axon-W008"))
}

// ── Constant conditions → W008 ──────────────────────────────────────────────

#[test]
fn literal_true_warns_dead_else() {
    let w = warnings(&flow("", "true"));
    assert!(
        w.iter().any(|m| m.contains("axon-W008") && m.contains("always true")),
        "`if true` must warn dead else: {w:?}"
    );
}

#[test]
fn literal_false_warns_dead_then() {
    let w = warnings(&flow("", "false"));
    assert!(
        w.iter().any(|m| m.contains("axon-W008") && m.contains("always false")),
        "`if false` must warn dead then: {w:?}"
    );
}

#[test]
fn constant_comparison_folds() {
    assert!(has_w008(&flow("", "5 > 3")), "`5 > 3` folds to true");
    assert!(has_w008(&flow("", "2 + 2 == 4")), "`2 + 2 == 4` folds to true");
    assert!(has_w008(&flow("", "1 == 2 or 3 < 1")), "boolean fold");
}

#[test]
fn constant_not_folds() {
    assert!(has_w008(&flow("", "not false")), "`not false` folds to true");
}

// ── Dynamic conditions → no W008 ────────────────────────────────────────────

#[test]
fn reference_condition_does_not_fold() {
    assert!(!has_w008(&flow("count: Int", "count > 3 and true")), "has a ref → dynamic");
}

#[test]
fn builtin_condition_does_not_fold() {
    assert!(!has_w008(&flow("", "recent.length >= 5")), "has a call → dynamic");
}

#[test]
fn flow_md_throttle_snippet_compiles_clean() {
    // published-grammar-must-compile — the flow.md v2.26.0 doc example.
    let src = "flow Throttle(recent: List<Call>, limit: Int) -> String {\n\
               \x20 if recent.length >= limit {\n\
               \x20   return \"throttled\"\n\
               \x20 }\n\
               \x20 return \"ok\"\n\
               }";
    let errs = errors_of(src);
    assert!(errs.is_empty(), "doc snippet must compile clean: {errs:?}");
}

fn errors_of(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog).check().into_iter().map(|e| e.message).collect()
}
