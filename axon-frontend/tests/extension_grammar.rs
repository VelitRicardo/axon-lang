//! v2.5.0 — the `extension` declaration grammar.
//!
//! v2.5.0 introduces a first-class, auditable + gateable
//! `extension Name { category: effects|scan, members: [ … ] }`
//! declaration so an adopter can expand a closed catalog (PCC effect
//! bases / shield scan categories) with domain-specific PROVENANCE
//! members WITHOUT touching the canonical catalog. 53.a lands the
//! front of the contract: the lexer keyword + AST + parser. Member
//! semantics validation (category ∈ {effects,scan}, no-shadowing,
//! provenance-class) is v2.5.0; IR lowering is v2.5.0; PCC soundness is
//! v2.5.0.
//!
//! Pins:
//!   1. `category: effects` + members with `{ semantics, default_confidence }`
//!      metadata parse + are captured into the AST.
//!   2. `category: scan` + bare string members parse (metadata absent).
//!   3. A trailing comma in the members list is tolerated.
//!   4. The declaration coexists with other top-level declarations.
//!   5. A member without a metadata block has `None` semantics +
//!      `None` default_confidence.

use axon_frontend::ast::{Declaration, ExtensionDefinition, Program};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::{ParseError, Parser};

fn parse(src: &str) -> Result<Program, ParseError> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse()
}

fn extension<'a>(prog: &'a Program, name: &str) -> &'a ExtensionDefinition {
    for decl in &prog.declarations {
        if let Declaration::Extension(e) = decl {
            if e.name == name {
                return e;
            }
        }
    }
    panic!("extension {name} not found");
}

// ─── section 1 — effects category + metadata captured ─────────────────────

#[test]
fn effects_extension_with_metadata_is_captured() {
    let src = r#"
extension epistemic_axis {
  category: effects
  members: [
    "epistemic:speculate" : { semantics: "external web data", default_confidence: 0.80 },
    "epistemic:believe"   : { semantics: "trusted CRM data",   default_confidence: 0.95 },
    "epistemic:know"       : { semantics: "confirmed action",   default_confidence: 1.0 }
  ]
}
"#;
    let prog = parse(src).expect("parse");
    let ext = extension(&prog, "epistemic_axis");
    assert_eq!(ext.category, "effects");
    assert_eq!(ext.members.len(), 3);

    let believe = ext
        .members
        .iter()
        .find(|m| m.name == "epistemic:believe")
        .expect("believe member present");
    assert_eq!(believe.semantics.as_deref(), Some("trusted CRM data"));
    assert_eq!(believe.default_confidence, Some(0.95));

    let know = &ext.members[2];
    assert_eq!(know.name, "epistemic:know");
    assert_eq!(know.default_confidence, Some(1.0));
}

// ─── section 2 — scan category + bare members ─────────────────────────────

#[test]
fn scan_extension_with_bare_members_parses() {
    let src = r#"
extension collections_scans {
  category: scan
  members: [ "dunning_pressure", "promise_to_pay_coercion" ]
}
"#;
    let prog = parse(src).expect("parse");
    let ext = extension(&prog, "collections_scans");
    assert_eq!(ext.category, "scan");
    assert_eq!(ext.members.len(), 2);
    assert_eq!(ext.members[0].name, "dunning_pressure");
    // section 5 — bare member: no metadata.
    assert!(ext.members[0].semantics.is_none());
    assert!(ext.members[0].default_confidence.is_none());
    assert_eq!(ext.members[1].name, "promise_to_pay_coercion");
}

// ─── section 3 — trailing comma tolerated ─────────────────────────────────

#[test]
fn trailing_comma_in_members_is_tolerated() {
    let src = r#"
extension trailer {
  category: scan
  members: [ "a", "b", ]
}
"#;
    let prog = parse(src).expect("parse");
    let ext = extension(&prog, "trailer");
    assert_eq!(ext.members.len(), 2);
}

// ─── section 4 — coexists with other declarations ─────────────────────────

#[test]
fn extension_coexists_with_other_declarations() {
    let src = r#"
extension epistemic_axis {
  category: effects
  members: [ "epistemic:know" : { default_confidence: 1.0 } ]
}

flow Chat() -> Unit { step S { ask: "hi" } }
"#;
    let prog = parse(src).expect("parse");
    // both declarations present
    let ext = extension(&prog, "epistemic_axis");
    assert_eq!(ext.members.len(), 1);
    assert_eq!(ext.members[0].name, "epistemic:know");
    // a member with only default_confidence (no semantics)
    assert!(ext.members[0].semantics.is_none());
    assert_eq!(ext.members[0].default_confidence, Some(1.0));
    let has_flow = prog
        .declarations
        .iter()
        .any(|d| matches!(d, Declaration::Flow(f) if f.name == "Chat"));
    assert!(has_flow, "flow Chat must coexist with the extension");
}
