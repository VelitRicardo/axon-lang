//! v2.5.0 — `extension` IR lowering + deterministic order.
//!
//! Pins:
//!   1. An `extension` declaration lowers into `IRProgram.extensions`
//!      (node_type, name, category, members + metadata preserved).
//!   2. Multiple extensions are sorted alphabetically by `name` at the
//!      end of IR generation (founder refinement B) — declaration order
//! does NOT perturb the IR vector, so the v2.5.0 proof-bundle hash
//!      is stable across multi-file declaration order.
//!   3. Member order within an extension is preserved (single-file,
//!      already deterministic).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn gen_ir(src: &str) -> axon_frontend::ir_nodes::IRProgram {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    IRGenerator::new().generate(&program)
}

// ─── section 1 — lowering captures the declaration ────────────────────────

#[test]
fn extension_lowers_into_ir() {
    let src = r#"
extension epistemic_axis {
  category: effects
  members: [
    "epistemic:believe" : { semantics: "trusted CRM", default_confidence: 0.95 }
  ]
}
"#;
    let ir = gen_ir(src);
    assert_eq!(ir.extensions.len(), 1);
    let ext = &ir.extensions[0];
    assert_eq!(ext.node_type, "extension");
    assert_eq!(ext.name, "epistemic_axis");
    assert_eq!(ext.category, "effects");
    assert_eq!(ext.members.len(), 1);
    assert_eq!(ext.members[0].name, "epistemic:believe");
    assert_eq!(ext.members[0].semantics.as_deref(), Some("trusted CRM"));
    assert_eq!(ext.members[0].default_confidence, Some(0.95));
}

// ─── section 2 — deterministic order (refinement B) ───────────────────────

#[test]
fn extensions_are_sorted_by_name_regardless_of_declaration_order() {
    // Declared zebra → alpha → mango; the IR must come back alpha →
    // mango → zebra so the proof-bundle hash is order-independent.
    let src = r#"
extension zebra  { category: scan members: [ "z" ] }
extension alpha  { category: scan members: [ "a" ] }
extension mango  { category: scan members: [ "m" ] }
"#;
    let ir = gen_ir(src);
    let names: Vec<&str> = ir.extensions.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "mango", "zebra"]);
}

// ─── section 3 — member order preserved within an extension ───────────────

#[test]
fn member_order_is_preserved() {
    let src = r#"
extension axis {
  category: effects
  members: [ "epistemic:speculate", "epistemic:believe", "epistemic:know" ]
}
"#;
    let ir = gen_ir(src);
    let members: Vec<&str> = ir.extensions[0]
        .members
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(
        members,
        vec!["epistemic:speculate", "epistemic:believe", "epistemic:know"]
    );
}
