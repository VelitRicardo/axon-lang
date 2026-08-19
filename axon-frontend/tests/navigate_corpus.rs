//! v2.13.0 — `navigate <corpus> { query:, from:, budget:, output: }`: the MDN
//! corpus-graph navigation surface. The navigate target may be a pix (PIX tree)
//! or a corpus (MDN graph); `from:`/`budget:` lower to the IR.

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::IRFlowNode;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog).check().into_iter().map(|e| e.message).collect()
}

const PROG: &str = r#"
corpus Sessions {
    documents: [a, b, c]
    relations: [ cite(b, a, 0.9) ]
}
flow Recall(q: String) -> String {
    navigate Sessions {
        query: "${q}"
        from: a
        budget: 5
        output: hits
    }
    return hits
}
"#;

#[test]
fn navigate_over_a_corpus_type_checks() {
    let e = errors(PROG);
    assert!(e.is_empty(), "navigate over an MDN corpus must type-check: {e:?}");
}

#[test]
fn navigate_seed_and_budget_lower_to_the_ir() {
    let tokens = Lexer::new(PROG, "t.axon").tokenize().unwrap();
    let prog = Parser::new(tokens).parse().unwrap();
    let ir = IRGenerator::new().generate(&prog);
    let flow = ir.flows.iter().find(|f| f.name == "Recall").unwrap();
    let nav = flow
        .steps
        .iter()
        .find_map(|n| match n {
            IRFlowNode::Navigate(s) => Some(s),
            _ => None,
        })
        .expect("a navigate node");
    assert_eq!(nav.pix_ref, "Sessions");
    assert_eq!(nav.seed, "a");
    assert_eq!(nav.budget, Some(5));
}

#[test]
fn navigate_over_undefined_ref_is_rejected() {
    let src = r#"flow F() -> String { navigate Nope { query: "x" } return "" }"#;
    assert!(
        errors(src).iter().any(|m| m.contains("pix or corpus")),
        "an undefined navigate target must error"
    );
}

#[test]
fn navigate_over_a_non_pix_non_corpus_is_rejected() {
    // `Persona` is neither a pix nor a corpus.
    let src = r#"
persona P { tone: precise }
flow F() -> String { navigate P { query: "x" } return "" }
"#;
    assert!(
        errors(src).iter().any(|m| m.contains("not a pix or corpus")),
        "navigating a non-pix/non-corpus must error"
    );
}
