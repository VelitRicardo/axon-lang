//! v2.4.0 — typed continuous-carrier grammar + the norm-invariant
//! type-carrier discipline.
//!
//! Adds the bracket type form (`SymbolicPtr[Tensor[Float32]]`,
//! `DensityMatrix[1024]`) and the typed `let x: T = …` annotation. Inside a
//! `quant` block the Continuous Type Invariant now inspects the annotation:
//! `DensityMatrix[D]` must have D = 2ⁿ (`axon-E0786`), and a discrete
//! conversational type (`String`/`Text`) is rejected (`axon-E0782`) — the
//! static half of the norm invariant (the encoder input is type-guaranteed a
//! continuous carrier; the numeric ‖x‖₂=1 assertion is the v2.4.0 runtime's job).

use axon_frontend::ast::{Declaration, FlowStep};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors_of(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program).check().into_iter().map(|e| e.message).collect()
}

fn has(errs: &[String], code: &str) -> bool {
    errs.iter().any(|e| e.contains(code))
}

/// Parse helper: return the type annotation of the first `let` in flow `F`'s
/// first quant block.
fn first_quant_let_type(src: &str) -> Option<(String, String)> {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    for decl in &program.declarations {
        if let Declaration::Flow(f) = decl {
            for step in &f.body {
                if let FlowStep::Quant(q) = step {
                    for s in &q.body {
                        if let FlowStep::Let(l) = s {
                            if let Some(ty) = &l.type_annotation {
                                return Some((ty.name.clone(), ty.generic_param.clone()));
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

// ── Bracket grammar parses ───────────────────────────────────────────────────

#[test]
fn nested_bracket_type_parses() {
    let src = "flow F(audio: String) -> String {\n\
                  quant {\n\
                     let surrogate: SymbolicPtr[Tensor[Float32]] = audio\n\
                  }\n\
                  return audio\n\
               }";
    let (name, param) = first_quant_let_type(src).expect("typed let annotation");
    assert_eq!(name, "SymbolicPtr");
    assert_eq!(param, "Tensor[Float32]", "nested bracket type round-trips into generic_param");
}

#[test]
fn density_matrix_dimension_parses() {
    let src = "flow F(audio: String) -> String {\n\
                  quant {\n\
                     let rho: DensityMatrix[1024] = audio\n\
                  }\n\
                  return audio\n\
               }";
    let (name, param) = first_quant_let_type(src).expect("typed let annotation");
    assert_eq!(name, "DensityMatrix");
    assert_eq!(param, "1024");
}

// ── DensityMatrix[D] power-of-two (E0786) ────────────────────────────────────

#[test]
fn power_of_two_density_matrix_passes() {
    let src = "flow F(audio: String) -> String {\n\
                  quant {\n\
                     let rho: DensityMatrix[1024] = audio\n\
                  }\n\
                  return audio\n\
               }";
    assert!(!has(&errors_of(src), "axon-E0786"), "D=1024=2^10 is a valid Hilbert dimension");
}

#[test]
fn non_power_of_two_density_matrix_is_rejected() {
    let src = "flow F(audio: String) -> String {\n\
                  quant {\n\
                     let rho: DensityMatrix[1000] = audio\n\
                  }\n\
                  return audio\n\
               }";
    let errs = errors_of(src);
    assert!(has(&errs, "axon-E0786"), "D=1000 is not 2^n → E0786: {errs:?}");
    assert!(errs.iter().any(|e| e.contains("1000")), "diagnostic names the bad dimension: {errs:?}");
}

// ── Typed discrete leak (E0782) ──────────────────────────────────────────────

#[test]
fn string_typed_let_inside_quant_is_rejected() {
    let src = "flow F(audio: String) -> String {\n\
                  quant {\n\
                     let leak: String = audio\n\
                  }\n\
                  return audio\n\
               }";
    assert!(has(&errors_of(src), "axon-E0782"), "a String-typed let inside quant must raise E0782");
}

#[test]
fn continuous_typed_let_inside_quant_passes() {
    let src = "flow F(audio: String) -> String {\n\
                  quant(encoding: amplitude) {\n\
                     let surrogate: SymbolicPtr[Tensor[Float32]] = audio\n\
                     let rho: DensityMatrix[256] = surrogate\n\
                  }\n\
                  return audio\n\
               }";
    let errs = errors_of(src);
    assert!(!has(&errs, "axon-E0782"), "continuous-typed lets are admitted: {errs:?}");
    assert!(!has(&errs, "axon-E0786"), "D=256=2^8 is valid: {errs:?}");
}

#[test]
fn typed_let_outside_quant_does_not_trip_invariant() {
    // The invariant is scoped to quant; a String-typed let elsewhere is legal.
    let src = "flow F(audio: String) -> String {\n\
                  let greeting: String = audio\n\
                  return greeting\n\
               }";
    assert!(!has(&errors_of(src), "axon-E0782"), "typed String let outside quant is fine");
}
