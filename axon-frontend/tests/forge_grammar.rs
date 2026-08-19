//! v2.41.0 — grammar + AST + IR + type-checker for the REAL `forge`
//! Directed Creative Synthesis primitive (replacing the pre-v2.41.0 no-op stub).
//! See `docs/cycle/forge_creative_synthesis.md` (axon-enterprise repo).
//!
//! Pinned properties:
//! 1. A full `forge Name(seed: "...") -> Type { mode, novelty, depth, branches,
//!    constraints }` parses into a populated `ForgeBlock` (not `{ loc }`).
//! 2. It lowers to `IRForgeBlock` with the metadata (the README's long-claimed
//!    "structured IR metadata"); defaults are elided.
//! 3. **IR-SHA invariance**: a program with no `forge` has no forge metadata.
//! 4. A well-formed forge → zero diagnostics.
//! 5. **axon-T868** unknown mode · **T869** novelty out of [0,1] · **T870**
//!    depth/branches < 1 · **T871** constraints not an anchor / anchor w/o
//!    confidence_floor · **T872** empty seed.
//! 6. The old empty `forge { }` stub is now a compile error (missing seed/type).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::{ParseError, Parser};
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}
fn try_parse(src: &str) -> Result<axon_frontend::ast::Program, ParseError> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse()
}
fn check_errors(src: &str) -> Vec<String> {
    let prog = parse(src);
    TypeChecker::new(&prog).check().iter().map(|e| e.message.clone()).collect()
}
fn ir_json(src: &str) -> String {
    let prog = parse(src);
    serde_json::to_string(&IRGenerator::new().generate(&prog)).expect("serialize IR")
}
fn first_forge(prog: &axon_frontend::ast::Program) -> &axon_frontend::ast::ForgeBlock {
    fn find<'a>(steps: &'a [axon_frontend::ast::FlowStep]) -> Option<&'a axon_frontend::ast::ForgeBlock> {
        for s in steps {
            if let axon_frontend::ast::FlowStep::Forge(f) = s {
                return Some(f);
            }
        }
        None
    }
    prog.declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Flow(f) => find(&f.body),
            _ => None,
        })
        .expect("no forge block")
}

const ANCHOR: &str = "anchor GoldenRatio { require: aesthetic_harmony confidence_floor: 0.70 }\n";

fn well_formed() -> String {
    format!(
        "{ANCHOR}\
         flow CreateVisualConcept(brief: String) -> Visual {{\n\
         \x20\x20forge Artwork(seed: \"aurora borealis over ancient ruins\") -> Visual {{\n\
         \x20\x20\x20\x20mode: transformational\n\
         \x20\x20\x20\x20novelty: 0.85\n\
         \x20\x20\x20\x20constraints: GoldenRatio\n\
         \x20\x20\x20\x20depth: 4\n\
         \x20\x20\x20\x20branches: 7\n\
         \x20\x20}}\n\
         }}\n"
    )
}

#[test]
fn forge_parses_into_populated_ast() {
    let prog = parse(&well_formed());
    let f = first_forge(&prog);
    assert_eq!(f.name, "Artwork");
    assert_eq!(f.seed, "aurora borealis over ancient ruins");
    assert_eq!(f.output_type, "Visual");
    assert_eq!(f.mode, "transformational");
    assert_eq!(f.novelty, 0.85);
    assert_eq!(f.depth, 4);
    assert_eq!(f.branches, 7);
    assert_eq!(f.constraints_ref, "GoldenRatio");
}

#[test]
fn well_formed_forge_has_no_errors() {
    let errs = check_errors(&well_formed());
    assert!(errs.is_empty(), "expected zero errors, got: {errs:#?}");
}

#[test]
fn forge_lowers_to_ir_metadata() {
    let json = ir_json(&well_formed());
    assert!(json.contains("\"seed\":\"aurora borealis over ancient ruins\""), "{json}");
    assert!(json.contains("\"mode\":\"transformational\""), "{json}");
    assert!(json.contains("\"novelty\":0.85"), "{json}");
    assert!(json.contains("\"depth\":4"), "{json}");
    assert!(json.contains("\"branches\":7"), "{json}");
    assert!(json.contains("\"constraints_ref\":\"GoldenRatio\""), "{json}");
}

#[test]
fn ir_sha_invariance_no_forge_metadata() {
    let src = "flow F() -> Unit { step S { ask: \"hi\" } }\n";
    let json = ir_json(src);
    assert!(!json.contains("\"seed\""), "seed leaked: {json}");
    assert!(!json.contains("\"novelty\""), "novelty leaked: {json}");
}

#[test]
fn t868_unknown_mode() {
    let src = format!(
        "{ANCHOR}flow F() -> V {{ forge A(seed: \"x\") -> V {{ mode: chaotic constraints: GoldenRatio }} }}\n"
    );
    assert!(check_errors(&src).iter().any(|m| m.contains("axon-T868")), "expected T868");
}

#[test]
fn t869_novelty_out_of_range() {
    let src = format!(
        "{ANCHOR}flow F() -> V {{ forge A(seed: \"x\") -> V {{ mode: exploratory novelty: 1.5 constraints: GoldenRatio }} }}\n"
    );
    assert!(check_errors(&src).iter().any(|m| m.contains("axon-T869")), "expected T869");
}

#[test]
fn t870_depth_branches_below_one() {
    let src = format!(
        "{ANCHOR}flow F() -> V {{ forge A(seed: \"x\") -> V {{ mode: exploratory depth: 0 branches: 0 }} }}\n"
    );
    let errs = check_errors(&src);
    assert_eq!(errs.iter().filter(|m| m.contains("axon-T870")).count(), 2, "expected two T870 (depth + branches)");
}

#[test]
fn t871_constraints_not_an_anchor() {
    // References a flow, not an anchor.
    let src = "flow Other() -> Unit { step S { ask: \"x\" } }\n\
               flow F() -> V { forge A(seed: \"x\") -> V { mode: exploratory constraints: Other } }\n";
    assert!(check_errors(src).iter().any(|m| m.contains("axon-T871")), "expected T871");
}

#[test]
fn t871_anchor_without_confidence_floor() {
    let src = "anchor Weak { require: something }\n\
               flow F() -> V { forge A(seed: \"x\") -> V { mode: exploratory constraints: Weak } }\n";
    assert!(
        check_errors(src).iter().any(|m| m.contains("axon-T871") && m.contains("confidence_floor")),
        "expected T871 for missing confidence_floor"
    );
}

#[test]
fn t872_empty_seed() {
    let src = format!(
        "{ANCHOR}flow F() -> V {{ forge A(seed: \"\") -> V {{ mode: exploratory constraints: GoldenRatio }} }}\n"
    );
    assert!(check_errors(&src).iter().any(|m| m.contains("axon-T872")), "expected T872");
}

#[test]
fn old_empty_forge_stub_is_now_a_compile_error() {
    // The pre-v2.41.0 no-op `forge { }` (no seed, no -> Type) no longer parses.
    let src = "flow F() -> Unit { forge { } }\n";
    assert!(try_parse(src).is_err(), "empty forge stub must now be a parse error");
}

#[test]
fn mode_defaults_to_exploratory_when_omitted() {
    // Omitting mode is allowed (defaults to exploratory) — no T868.
    let src = format!(
        "{ANCHOR}flow F() -> V {{ forge A(seed: \"x\") -> V {{ novelty: 0.5 constraints: GoldenRatio }} }}\n"
    );
    let errs = check_errors(&src);
    assert!(errs.iter().all(|m| !m.contains("axon-T868")), "omitted mode must not error: {errs:#?}");
}
