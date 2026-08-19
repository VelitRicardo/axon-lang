//! v2.65.0 — `grad` over the closed `Expr`: grammar + the two laws
//! (axon-T931 differentiability / axon-T932 resolution) + the IR artifact
//! (the SIMPLIFIED derivative rides `IRGradStep.derivatives`).
//! See `docs/cycle/grad_over_expr.md` (axon-enterprise).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn check_errors(src: &str) -> Vec<String> {
    let prog = parse(src);
    TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

fn ir_json(src: &str) -> String {
    let prog = parse(src);
    let ir = IRGenerator::new().generate(&prog);
    serde_json::to_string(&ir).expect("serialize IR")
}

const GOOD: &str = r#"
flow Score(x: Float, y: Float) -> Text {
    let total = 3.0 * x + y * y
    grad total wrt [x, y] as g
    return g
}
"#;

// ── 1. Grammar ───────────────────────────────────────────────────────────────

#[test]
fn parses_grad_with_multi_wrt_and_as() {
    let prog = parse(GOOD);
    let flow = prog
        .declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Flow(f) => Some(f),
            _ => None,
        })
        .expect("flow");
    let grad = flow
        .body
        .iter()
        .find_map(|s| match s {
            axon_frontend::ast::FlowStep::Grad(g) => Some(g),
            _ => None,
        })
        .expect("grad step");
    assert_eq!(grad.target, "total");
    assert_eq!(grad.wrt, vec!["x", "y"]);
    assert_eq!(grad.output, "g");
}

#[test]
fn single_wrt_and_default_output_parse() {
    let src = "flow F(x: Float) -> Text {\n    let e = x * x\n    grad e wrt x\n}\n";
    let errs = check_errors(src);
    assert!(
        !errs.iter().any(|e| e.contains("axon-T93")),
        "the minimal form checks clean: {errs:?}"
    );
    let json = ir_json(src);
    assert!(json.contains("\"output\":\"d_e\""), "default binding: {json}");
}

// ── 2. axon-T932 — resolution ────────────────────────────────────────────────

#[test]
fn t932_refuses_no_wrt_and_unknown_target() {
    let src = "flow F(x: Float) -> Text {\n    let e = x * x\n    grad e\n}\n";
    let errs = check_errors(src);
    assert!(
        errs.iter().any(|e| e.contains("axon-T932") && e.contains("no `wrt`")),
        "{errs:?}"
    );
    let src = "flow F(x: Float) -> Text {\n    grad ghost wrt x\n}\n";
    let errs = check_errors(src);
    assert!(
        errs.iter()
            .any(|e| e.contains("axon-T932") && e.contains("not a PRIOR rich `let`")),
        "{errs:?}"
    );
}

#[test]
fn t932_grad_before_its_let_is_refused() {
    // "Prior" means PRIOR: the derivative of an expression not yet bound
    // is a forward reference, refused.
    let src = "flow F(x: Float) -> Text {\n    grad e wrt x\n    let e = x * x\n}\n";
    let errs = check_errors(src);
    assert!(
        errs.iter().any(|e| e.contains("axon-T932")),
        "forward grad must refuse: {errs:?}"
    );
}

#[test]
fn t932_literal_let_is_not_differentiable_material() {
    // `let s = "hello"` is a literal, not a rich expression — no AST to
    // differentiate.
    let src = "flow F(x: Float) -> Text {\n    let s = \"hello\"\n    grad s wrt x\n}\n";
    let errs = check_errors(src);
    assert!(errs.iter().any(|e| e.contains("axon-T932")), "{errs:?}");
}

// ── 3. axon-T931 — differentiability ─────────────────────────────────────────

#[test]
fn t931_names_the_construct_and_position() {
    let src = "flow F(x: Float, s: Text) -> Text {\n    let e = x + s.length()\n    grad e wrt x\n}\n";
    let errs = check_errors(src);
    let t931 = errs
        .iter()
        .find(|e| e.contains("axon-T931"))
        .unwrap_or_else(|| panic!("must refuse: {errs:?}"));
    assert!(t931.contains("builtin length"), "names the construct: {t931}");
    assert!(t931.contains("position"), "names the position: {t931}");
    assert!(
        t931.contains("does not fabricate"),
        "teaches the doctrine (no silent zeros): {t931}"
    );
}

#[test]
fn t931_refuses_mod_and_comparisons() {
    for (expr, construct) in [("x % 2", "mod"), ("x > 1.0", "comparison")] {
        let src = format!(
            "flow F(x: Float) -> Text {{\n    let e = {expr}\n    grad e wrt x\n}}\n"
        );
        let errs = check_errors(&src);
        assert!(
            errs.iter()
                .any(|e| e.contains("axon-T931") && e.contains(construct)),
            "`{expr}` → {construct}: {errs:?}"
        );
    }
}

// ── 4. The IR artifact — the derivative IS IR ────────────────────────────────

#[test]
fn ir_carries_original_and_simplified_derivatives() {
    let json = ir_json(GOOD);
    // ∂(3x + y²)/∂x = 3 (fully folded by the simplifier);
    // ∂/∂y = y + y (product rule, 1· stripped).
    assert!(json.contains("\"node_type\":\"grad\""), "{json}");
    assert!(json.contains("\"target\":\"total\""));
    assert!(json.contains("\"original\""), "the differentiated expr rides along");
    assert!(
        json.contains("\"derivatives\""),
        "the simplified gradient vector rides along: {json}"
    );
    // The x-derivative folded to the literal 3.0 — proof the simplifier
    // ran (an unsimplified product rule would carry `0.0 * x` noise).
    assert!(
        json.contains("3.0") && !json.contains("0.0 *"),
        "simplified: {json}"
    );
}
