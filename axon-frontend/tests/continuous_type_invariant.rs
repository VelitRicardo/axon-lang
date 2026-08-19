//! v2.4.0 — the Continuous Type Invariant over a `quant` block body
//! (paper section 4.2, refined per plan D8; `axon-E0782`).
//!
//! Inside a `quant { … }` block the Hilbert-space scope admits continuous
//! carriers + discrete *classical control* (integer indices, the closed enum of
//! measurement bases) but REJECTS conversational / unstructured discrete values
//! that collapse the continuous gradient: String literals, `.to_string` textual
//! conversions, and free-text `ask:` prompts. This test pins the negative cases
//! (each rejected with `axon-E0782`) + the positive cases (continuous bodies +
//! String usage OUTSIDE quant stay clean) + recursion into nested blocks.
//!
//! NOT covered here: the norm invariant ‖x‖₂=1 (a typed-carrier property → v2.4.0).

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn check_errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "q.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

fn has_e0782(errors: &[String]) -> bool {
    errors.iter().any(|e| e.contains("axon-E0782"))
}

// ── Negative cases — the invariant fires ────────────────────────────────────

#[test]
fn string_literal_inside_quant_is_rejected() {
    // The paper's E0782 example shape: a String leak inside the quantum scope.
    let src = "flow F(audio_tensor: String) -> String {\n\
                  quant {\n\
                     let surrogate = audio_tensor\n\
                     let text_leak = \"prediccion_latente\"\n\
                  }\n\
                  return surrogate\n\
               }";
    let errors = check_errors(src);
    assert!(has_e0782(&errors), "a String literal in a quant body must raise axon-E0782: {errors:?}");
    assert!(
        errors.iter().any(|e| e.contains("text_leak") && e.contains("String")),
        "the diagnostic should name the offending binding + the non-continuous type: {errors:?}"
    );
}

#[test]
fn to_string_conversion_inside_quant_is_rejected() {
    let src = "flow F(audio_tensor: String) -> String {\n\
                  quant {\n\
                     let surrogate = audio_tensor\n\
                     let leak = surrogate.to_string\n\
                  }\n\
                  return surrogate\n\
               }";
    let errors = check_errors(src);
    assert!(has_e0782(&errors), "a `.to_string` textual conversion must raise axon-E0782: {errors:?}");
    assert!(
        errors.iter().any(|e| e.contains("to_string")),
        "the diagnostic should name the textual conversion: {errors:?}"
    );
}

#[test]
fn free_text_ask_step_inside_quant_is_rejected() {
    let src = "flow F(audio_tensor: String) -> String {\n\
                  quant {\n\
                     step Analyze { ask: \"describe the latent\" }\n\
                  }\n\
                  return audio_tensor\n\
               }";
    let errors = check_errors(src);
    assert!(has_e0782(&errors), "a free-text `ask:` step inside quant must raise axon-E0782: {errors:?}");
    assert!(
        errors.iter().any(|e| e.contains("ask")),
        "the diagnostic should call out the free-text ask prompt: {errors:?}"
    );
}

#[test]
fn leak_nested_in_a_for_inside_quant_is_still_caught() {
    // Recursion: a leak one nesting level down must not escape the invariant.
    let src = "flow F(items: String) -> String {\n\
                  quant {\n\
                     for e in items {\n\
                        let leak = \"boom\"\n\
                     }\n\
                  }\n\
                  return items\n\
               }";
    let errors = check_errors(src);
    assert!(has_e0782(&errors), "a String leak nested in a for-in inside quant must be caught: {errors:?}");
}

// ── Positive cases — the invariant admits continuous control ─────────────────

#[test]
fn continuous_quant_body_passes() {
    // References to prior continuous carriers + a numeric index are admitted.
    let src = "flow F(audio_tensor: String) -> String {\n\
                  quant(qubits: 10) {\n\
                     let surrogate = audio_tensor\n\
                     let depth = 4\n\
                     probe surrogate\n\
                  }\n\
                  return surrogate\n\
               }";
    let errors = check_errors(src);
    assert!(!has_e0782(&errors), "a continuous quant body must NOT raise axon-E0782: {errors:?}");
}

#[test]
fn string_literal_outside_quant_is_fine() {
    // The invariant is scoped to the quant block — String usage elsewhere is legal.
    let src = "flow F(audio_tensor: String) -> String {\n\
                  let greeting = \"hello\"\n\
                  quant {\n\
                     let surrogate = audio_tensor\n\
                  }\n\
                  return greeting\n\
               }";
    let errors = check_errors(src);
    assert!(!has_e0782(&errors), "a String literal OUTSIDE quant must not trip the invariant: {errors:?}");
}

#[test]
fn numeric_header_attributes_are_not_string_leaks() {
    // qubits/depth/bandwidth live in the header, not the body — never E0782.
    let src = "flow F(t: String) -> String {\n\
                  quant(encoding: amplitude, qubits: 10, depth: 4, bandwidth: 0.5) {\n\
                     let x = t\n\
                  }\n\
                  return t\n\
               }";
    let errors = check_errors(src);
    assert!(!has_e0782(&errors), "header attributes are not body String leaks: {errors:?}");
}
