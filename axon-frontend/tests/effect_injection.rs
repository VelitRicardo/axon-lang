//! v2.4.0 — algebraic-effect injection + flow-level propagation (D9).
//!
//! `ots:backend:quant_sim` / `ots:backend:qpu_native` are now first-class,
//! type-checked effect slugs (injected into the closed `ots:backend`
//! catalogue): a tool may declare them. A `quant` block selects its backend
//! from the strict subset `{quant_sim, qpu_native}` (axon-E0784). And
//! `flow_quant_effects` projects the `ots:backend:<backend>` slugs a flow
//! performs — the flow-level effect row for the quant primitive.
//!
//! NOT covered here: the `yield` measurement point + the one-shot delimited
//! continuation over the v1.16.0–v1.18.0 runtime → v2.4.0.

use axon_frontend::ast::Declaration;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::{flow_quant_effects, TypeChecker};

fn errors_of(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "e.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program).check().into_iter().map(|e| e.message).collect()
}

fn has(errs: &[String], code: &str) -> bool {
    errs.iter().any(|e| e.contains(code))
}

fn quant_effects(src: &str, flow: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "e.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    let f = program
        .declarations
        .iter()
        .find_map(|d| match d {
            Declaration::Flow(f) if f.name == flow => Some(f),
            _ => None,
        })
        .expect("flow");
    flow_quant_effects(f)
}

// ── Effect injection: ots:backend:quant_sim is a valid tool effect ───────────

#[test]
fn quant_sim_is_a_valid_tool_effect_slug() {
    // Before v2.4.0, `ots:backend:quant_sim` was an unknown OTS backend.
    let src = "tool Simulate {\n\
                  effects: <ots:backend:quant_sim>\n\
               }";
    assert!(
        !errors_of(src).iter().any(|e| e.contains("Unknown OTS backend")),
        "ots:backend:quant_sim must be a valid effect slug now: {:?}",
        errors_of(src)
    );
}

#[test]
fn qpu_native_is_a_valid_tool_effect_slug() {
    let src = "tool RunOnQpu {\n\
                  effects: <ots:backend:qpu_native>\n\
               }";
    assert!(
        !errors_of(src).iter().any(|e| e.contains("Unknown OTS backend")),
        "ots:backend:qpu_native must be a valid effect slug now"
    );
}

#[test]
fn unknown_ots_backend_still_rejected() {
    let src = "tool Bad {\n\
                  effects: <ots:backend:dwave>\n\
               }";
    assert!(
        errors_of(src).iter().any(|e| e.contains("Unknown OTS backend")),
        "an unknown OTS backend is still rejected (the catalogue stays closed)"
    );
}

// ── quant header selects from the quant subset ───────────────────────────────

#[test]
fn quant_rejects_a_non_quantum_backend() {
    // `native` is a valid ots:backend qualifier but NOT a quantum backend.
    let src = "flow F(t: String) -> String {\n\
                  quant(backend: native) { let x = t }\n\
                  return t\n\
               }";
    assert!(has(&errors_of(src), "axon-E0784"), "a quant block may not select the `native` OTS backend");
}

// ── Flow-level effect-row projection ─────────────────────────────────────────

#[test]
fn flow_with_quant_carries_the_effect() {
    let src = "flow F(t: String) -> String {\n\
                  quant(backend: quant_sim) { let x = t }\n\
                  return t\n\
               }";
    assert_eq!(
        quant_effects(src, "F"),
        vec!["ots:backend:quant_sim".to_string()],
        "a flow containing a quant block carries its ots:backend effect"
    );
}

#[test]
fn bare_quant_defaults_to_quant_sim_effect() {
    let src = "flow F(t: String) -> String {\n\
                  quant { let x = t }\n\
                  return t\n\
               }";
    assert_eq!(quant_effects(src, "F"), vec!["ots:backend:quant_sim".to_string()]);
}

#[test]
fn flow_without_quant_has_no_quant_effect() {
    let src = "flow F(t: String) -> String { return t }";
    assert!(quant_effects(src, "F").is_empty(), "no quant block ⇒ no quant effect in the row");
}

#[test]
fn nested_quant_effects_collected_and_deduped() {
    // qpu_native in a for-body + a quant_sim at top level; dedup within kind.
    let src = "flow F(items: String, t: String) -> String {\n\
                  quant(backend: quant_sim) { let a = t }\n\
                  for e in items {\n\
                     quant(backend: qpu_native) { let b = t }\n\
                  }\n\
                  quant(backend: quant_sim) { let c = t }\n\
                  return t\n\
               }";
    let effects = quant_effects(src, "F");
    assert_eq!(
        effects,
        vec!["ots:backend:quant_sim".to_string(), "ots:backend:qpu_native".to_string()],
        "effects are collected across nesting, source-ordered, and deduped: {effects:?}"
    );
}
