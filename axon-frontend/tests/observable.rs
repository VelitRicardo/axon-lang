//! v2.4.0 — the `observable` Pauli-sum primitive (paper section 3.2; plan D5).
//!
//! `observable <Name> { qubits, term: cₖ·Pₖ … }` declares a typed Pauli-sum
//! `M = Σ cₖ Pₖ`. Real coefficients × Pauli strings over the closed `{I,X,Y,Z}`
//! alphabet are **Hermitian by construction**, so the checker only enforces the
//! structural well-formedness (closed alphabet, equal term lengths, non-empty
//! sum, declared-width match) with `axon-E0785`. A `quant(observable: <Name>)`
//! header reference must resolve to a declared `observable` (`axon-E0784`).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::IRProgram;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn ir_of(src: &str) -> IRProgram {
    let tokens = Lexer::new(src, "o.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    IRGenerator::new().generate(&program)
}

fn errors_of(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "o.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program).check().into_iter().map(|e| e.message).collect()
}

fn has(errs: &[String], code: &str) -> bool {
    errs.iter().any(|e| e.contains(code))
}

const HAMILTONIAN: &str = "observable EnergyHamiltonian {\n\
                              qubits: 2\n\
                              term: 0.5 * \"ZZ\"\n\
                              term: -1.2 * \"XI\"\n\
                           }\n";

// ── Parse + lower ────────────────────────────────────────────────────────────

#[test]
fn observable_parses_and_lowers() {
    let ir = ir_of(HAMILTONIAN);
    assert_eq!(ir.observables.len(), 1, "the observable declaration lowers to IR");
    let obs = &ir.observables[0];
    assert_eq!(obs.name, "EnergyHamiltonian");
    assert_eq!(obs.qubits, Some(2));
    assert_eq!(obs.terms.len(), 2);
    assert_eq!(obs.terms[0].coefficient, 0.5);
    assert_eq!(obs.terms[0].pauli, "ZZ");
    assert_eq!(obs.terms[1].coefficient, -1.2, "the negative coefficient is preserved");
    assert_eq!(obs.terms[1].pauli, "XI");
}

#[test]
fn well_formed_observable_passes() {
    assert!(!has(&errors_of(HAMILTONIAN), "axon-E0785"), "a valid Pauli-sum must not raise E0785");
}

// ── Pauli-sum validation (E0785) ─────────────────────────────────────────────

#[test]
fn bad_pauli_alphabet_is_rejected() {
    let src = "observable Bad { term: 1.0 * \"ZK\" }\n";
    let errs = errors_of(src);
    assert!(has(&errs, "axon-E0785"), "a non-{{I,X,Y,Z}} char must raise E0785: {errs:?}");
    assert!(errs.iter().any(|e| e.contains("'K'")), "diagnostic names the bad char: {errs:?}");
}

#[test]
fn mismatched_term_lengths_are_rejected() {
    let src = "observable Bad {\n\
                  term: 1.0 * \"ZZ\"\n\
                  term: 0.3 * \"X\"\n\
               }\n";
    assert!(has(&errors_of(src), "axon-E0785"), "unequal Pauli-string lengths must raise E0785");
}

#[test]
fn declared_qubits_mismatch_is_rejected() {
    let src = "observable Bad {\n\
                  qubits: 3\n\
                  term: 1.0 * \"ZZ\"\n\
               }\n";
    assert!(has(&errors_of(src), "axon-E0785"), "qubits != Pauli width must raise E0785");
}

#[test]
fn empty_observable_is_rejected() {
    let src = "observable Empty { qubits: 2 }\n";
    assert!(has(&errors_of(src), "axon-E0785"), "an observable with no terms must raise E0785");
}

// ── quant observable: resolution (E0784) ─────────────────────────────────────

#[test]
fn quant_resolves_declared_observable() {
    let src = format!(
        "{HAMILTONIAN}\n\
         flow F(t: String) -> String {{\n\
            quant(observable: EnergyHamiltonian) {{ let x = t }}\n\
            return t\n\
         }}"
    );
    assert!(!has(&errors_of(&src), "axon-E0784"), "a declared observable resolves cleanly");
}

#[test]
fn quant_undefined_observable_is_rejected() {
    let src = "flow F(t: String) -> String {\n\
                  quant(observable: NoSuchThing) { let x = t }\n\
                  return t\n\
               }";
    let errs = errors_of(src);
    assert!(has(&errs, "axon-E0784"), "an undefined observable must raise E0784: {errs:?}");
    assert!(errs.iter().any(|e| e.contains("NoSuchThing")), "diagnostic names it: {errs:?}");
}

#[test]
fn quant_observable_referencing_a_nonobservable_is_rejected() {
    // A `flow` named the same as the reference is not an observable.
    let src = "flow EnergyHamiltonian() -> String { return \"x\" }\n\
               flow F(t: String) -> String {\n\
                  quant(observable: EnergyHamiltonian) { let x = t }\n\
                  return t\n\
               }";
    let errs = errors_of(src);
    assert!(has(&errs, "axon-E0784"), "referencing a non-observable must raise E0784: {errs:?}");
    assert!(errs.iter().any(|e| e.contains("not an observable")), "diagnostic explains the kind: {errs:?}");
}
