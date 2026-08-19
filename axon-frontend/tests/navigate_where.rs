//! v2.17.0 (Q2) — the `navigate { … where: "<filter>" … }` column-scope key
//! parses + lowers to `IRNavigateStep.where_expr`.
//!
//! Kivi multiplexes N end-clients in ONE axon-tenant via a `tenant_id` column;
//! RLS scopes by axon-tenant, so a `navigate` over a `corpus from axonstore`
//! without a column filter recovers cross-client rows. This adds an optional
//! `where:` (same shape as `retrieve … where`) that the runtime pushes to the
//! SELECT sourcing the corpus rows. This test pins the FRONTEND surface: parse
//! + IR lowering + back-compat (no `where:` ⇒ empty, serde-elided).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::IRFlowNode;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

const STORES: &str = r#"
axonstore LtmSummaries {
    backend: postgresql
    connection: "env:DB"
    schema { id: Uuid primary_key  summary: Text not_null  tenant_id: Uuid }
}
axonstore LtmEdges {
    backend: postgresql
    connection: "env:DB"
    schema { from_id: Uuid  to_id: Uuid  etype: Text  weight: Float  tenant_id: Uuid }
}
corpus LtmGraph from axonstore {
    documents: LtmSummaries( id, summary )
    relations: LtmEdges( from_id, to_id, etype, weight )
    adaptive: true
}
"#;

fn ir_of(src: &str) -> axon_frontend::ir_nodes::IRProgram {
    let toks = Lexer::new(src, "t.axon").tokenize().expect("lex");
    let prog = Parser::new(toks).parse().expect("parse");
    IRGenerator::new().generate(&prog)
}

fn navigate_of(ir: &axon_frontend::ir_nodes::IRProgram, flow: &str) -> axon_frontend::ir_nodes::IRNavigateStep {
    ir.flows
        .iter()
        .find(|f| f.name == flow)
        .expect("flow")
        .steps
        .iter()
        .find_map(|n| match n {
            IRFlowNode::Navigate(s) => Some(s.clone()),
            _ => None,
        })
        .expect("navigate node")
}

#[test]
fn navigate_where_parses_and_lowers_to_ir() {
    let src = format!(
        "{STORES}\n\
         flow Recall(user_input: String, tenant_id: String) -> String {{\n\
            navigate LtmGraph {{ query: \"${{user_input}}\"  budget: 5  where: \"tenant_id == '${{tenant_id}}'\"  output: hits }}\n\
            return hits\n\
         }}"
    );
    let ir = ir_of(&src);
    let nav = navigate_of(&ir, "Recall");
    assert_eq!(
        nav.where_expr, "tenant_id == '${tenant_id}'",
        "the navigate `where:` filter must lower to IRNavigateStep.where_expr verbatim"
    );
    assert_eq!(nav.budget, Some(5), "other keys still parse alongside where:");
    assert_eq!(nav.output_name, "hits");
}

#[test]
fn navigate_without_where_is_empty_and_back_compatible() {
    let src = format!(
        "{STORES}\n\
         flow Recall(user_input: String) -> String {{\n\
            navigate LtmGraph {{ query: \"${{user_input}}\"  budget: 5  output: hits }}\n\
            return hits\n\
         }}"
    );
    let ir = ir_of(&src);
    let nav = navigate_of(&ir, "Recall");
    assert!(
        nav.where_expr.is_empty(),
        "no `where:` ⇒ empty (RLS-only scope, the v2.14.0 default)"
    );
    // Serde elision: a no-where navigate serialises without the field (the IR
    // golden / wire stays byte-identical for every pre-v2.17.0 corpus flow).
    let json = serde_json::to_string(&nav).expect("serialize");
    assert!(
        !json.contains("where_expr"),
        "empty where_expr must be serde-skipped: {json}"
    );
}
