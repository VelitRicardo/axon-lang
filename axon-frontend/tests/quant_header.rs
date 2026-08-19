//! v2.4.0 — semantic validation of the `quant` block header
//! (encoding-scheme attribute typing + closed-set checks + the D2 depth
//! trade-off note). Errors are `axon-E0784`; the depth note is `axon-W005`.
//!
//! Scope note: the Pauli-sum `observable:` *declaration* + resolution
//! (v2.4.0) and the typed continuous-carrier grammar + norm invariant
//! (v2.4.0) are separate commits; this pins the header discipline.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors_of(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "q.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program).check().into_iter().map(|e| e.message).collect()
}

fn warnings_of(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "q.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    let (_errors, warnings) = TypeChecker::new(&program).check_with_warnings();
    warnings.into_iter().map(|e| e.message).collect()
}

fn has(errs: &[String], code: &str) -> bool {
    errs.iter().any(|e| e.contains(code))
}

// ── Negative cases — E0784 ───────────────────────────────────────────────────

#[test]
fn unknown_encoding_scheme_is_rejected() {
    let src = "flow F(t: String) -> String {\n\
                  quant(encoding: hologram) { let x = t }\n\
                  return t\n\
               }";
    let errs = errors_of(src);
    assert!(has(&errs, "axon-E0784"), "unknown encoding must raise E0784: {errs:?}");
    assert!(errs.iter().any(|e| e.contains("hologram")), "diagnostic names the bad scheme: {errs:?}");
}

#[test]
fn unknown_backend_is_rejected() {
    let src = "flow F(t: String) -> String {\n\
                  quant(backend: dwave) { let x = t }\n\
                  return t\n\
               }";
    let errs = errors_of(src);
    assert!(has(&errs, "axon-E0784"), "unknown backend must raise E0784: {errs:?}");
    assert!(errs.iter().any(|e| e.contains("dwave")), "diagnostic names the bad backend: {errs:?}");
}

#[test]
fn zero_qubits_is_rejected() {
    let src = "flow F(t: String) -> String {\n\
                  quant(qubits: 0) { let x = t }\n\
                  return t\n\
               }";
    assert!(has(&errors_of(src), "axon-E0784"), "qubits < 1 must raise E0784");
}

#[test]
fn zero_depth_is_rejected() {
    let src = "flow F(t: String) -> String {\n\
                  quant(depth: 0) { let x = t }\n\
                  return t\n\
               }";
    assert!(has(&errors_of(src), "axon-E0784"), "depth < 1 must raise E0784");
}

#[test]
fn nonpositive_bandwidth_is_rejected() {
    let src = "flow F(t: String) -> String {\n\
                  quant(bandwidth: 0) { let x = t }\n\
                  return t\n\
               }";
    assert!(has(&errors_of(src), "axon-E0784"), "bandwidth <= 0 must raise E0784");
}

// ── Positive cases + the D2 note ─────────────────────────────────────────────

#[test]
fn valid_amplitude_header_passes_with_depth_note() {
    let src = "flow F(t: String) -> String {\n\
                  quant(encoding: amplitude, qubits: 10, depth: 4, bandwidth: 0.5, backend: quant_sim) { let x = t }\n\
                  return t\n\
               }";
    assert!(!has(&errors_of(src), "axon-E0784"), "a valid header must not raise E0784");
    let warns = warnings_of(src);
    assert!(
        warns.iter().any(|w| w.contains("axon-W005") && w.contains("amplitude") && w.contains("O(d)")),
        "amplitude must surface the O(d) state-prep depth trade-off note (D2): {warns:?}"
    );
}

#[test]
fn valid_angle_header_passes_with_its_note() {
    let src = "flow F(t: String) -> String {\n\
                  quant(encoding: angle) { let x = t }\n\
                  return t\n\
               }";
    assert!(!has(&errors_of(src), "axon-E0784"));
    let warns = warnings_of(src);
    assert!(
        warns.iter().any(|w| w.contains("axon-W005") && w.contains("angle") && w.contains("d=n")),
        "angle must surface the d=n feature-limit trade-off note (D2): {warns:?}"
    );
}

#[test]
fn bare_quant_has_no_header_error_and_no_note() {
    // No encoding specified ⇒ no E0784, and no W005 note (default is silent).
    let src = "flow F(t: String) -> String {\n\
                  quant { let x = t }\n\
                  return t\n\
               }";
    assert!(!has(&errors_of(src), "axon-E0784"), "bare quant has a valid default header");
    assert!(!has(&warnings_of(src), "axon-W005"), "no encoding specified ⇒ no depth note");
}
