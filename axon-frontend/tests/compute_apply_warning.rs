//! v2.22.0 — `axon-W006`: `apply: <Compute>` is a model-selection no-op.
//!
//! The brief-#36 adopter wrote `compute Big { model: "moonshot-v1-32k" }` +
//! `step S { apply: Big }` to pin a larger model — and was silently ignored (the
//! `model:` is dropped at lowering, `apply:` picks no model). The honest compiler
//! must say so and point at `requires_context:` (the v2.9.0 doctrine: a silent no-op
//! becomes guidance). A step that uses `requires_context:` instead must NOT warn.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn warnings(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    let (_errors, warns) = TypeChecker::new(&prog).check_with_warnings();
    warns.into_iter().map(|w| w.message).collect()
}

const APPLY_COMPUTE: &str = r#"
compute Big { model: "moonshot-v1-32k" }

flow Summarize() -> String {
    step S {
        given: history
        ask: "summarize ${history}"
        apply: Big
        output: String
    }
    return "ok"
}
"#;

const REQUIRES_CONTEXT: &str = r#"
flow Summarize() -> String {
    step S {
        given: history
        ask: "summarize ${history}"
        requires_context: 32000
        output: String
    }
    return "ok"
}
"#;

#[test]
fn apply_compute_emits_w006_pointing_at_requires_context() {
    let warns = warnings(APPLY_COMPUTE);
    let w006: Vec<&String> = warns.iter().filter(|w| w.contains("axon-W006")).collect();
    assert_eq!(w006.len(), 1, "exactly one W006 for the apply: <Compute>; got {warns:?}");
    let msg = w006[0];
    assert!(msg.contains("requires_context"), "must point at the faithful surface: {msg}");
    assert!(msg.contains("does NOT select an LLM model"), "must state the no-op: {msg}");
}

#[test]
fn requires_context_step_does_not_warn_w006() {
    let warns = warnings(REQUIRES_CONTEXT);
    assert!(
        !warns.iter().any(|w| w.contains("axon-W006")),
        "a step declaring requires_context: is the faithful form — no W006. got {warns:?}"
    );
}
