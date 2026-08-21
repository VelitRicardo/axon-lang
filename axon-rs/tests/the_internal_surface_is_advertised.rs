//! v4.4.0 — `extension` and `witness`: two constructs an adopter can write and
//! the ledger said nothing about.
//!
//! Both were listed in the primitive catalogue with no row in `advertised.rs`,
//! and the proposal on the table was to mark them internal — `pub(crate)`,
//! undocumented, out of the promise.
//!
//! **The measurement refused that.** Neither is an IR-only construct:
//!
//! - both have a lexer token, a parser production, and a checker enforcing a
//!   CLOSED catalogue (`metric:` ∈ {geometric_difference,
//!   kernel_target_alignment, ranking_lift, outcome_lift}; `category:` ∈
//!   {effects, scan}) with their own diagnostics;
//! - both are lowered into the IR;
//! - `extension` is read by the Proof-Carrying Code **prover** and **checker**
//!   through `pcc::generate::extension_effect_members`, and by
//!   `shield_registry::check_extension_scan_coverage`, which the enterprise
//!   deploy path calls to REFUSE a flow whose scan coverage is wrong;
//! - `witness` is lowered to `IRWitness` and its metric/baseline vocabulary is
//!   shared with `advantage_witness`, which `quant_witness` and
//!   `retrieval_witness` consume.
//!
//! Hiding a writable, enforced construct is the v2.87.0 blind spot in its worst
//! form. The ledger's own words: *"an unadvertised subsystem is invisible to
//! every gate in this file."* The algebraic-effects engine survived 590 lines
//! and 49 green tests unnoticed for exactly that reason — nothing was
//! advertised, so there was nothing to classify. A construct an adopter CAN
//! write and the compiler DOES enforce, with no row saying what its runtime
//! delivers, is the same hole with the polarity flipped.
//!
//! So both get a row, and this file is the gate those rows cite. Under Law 4 a
//! citation must name a `.axon` fixture that exists and a gate that READS it —
//! which is what makes the row mean something rather than merely parse.

use std::path::{Path, PathBuf};

fn fixture(name: &str) -> String {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/surface_census")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{} must exist: {e}", p.display()))
}

/// Source → IR through the same pipeline the CLI uses. Anything less would test
/// a program this compiler cannot actually be handed.
fn compile(src: &str, label: &str) -> axon::ir_nodes::IRProgram {
    let tokens = axon_frontend::lexer::Lexer::new(src, label)
        .tokenize()
        .unwrap_or_else(|e| panic!("{label} must lex: {e:?}"));
    let program = axon_frontend::parser::Parser::new(tokens)
        .parse()
        .unwrap_or_else(|e| panic!("{label} must parse: {e:?}"));
    let errors = axon_frontend::type_checker::TypeChecker::new(&program).check();
    assert!(
        errors.is_empty(),
        "{label} must type-check clean, got: {:#?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    axon_frontend::ir_generator::IRGenerator::new().generate(&program)
}

// ── extension ───────────────────────────────────────────────────────────────

#[test]
fn a_declared_extension_reaches_the_proof_carrying_code_prover() {
    let src = fixture("extension_effects.axon");
    let ir = compile(&src, "extension_effects.axon");

    assert_eq!(ir.extensions.len(), 1, "the extension must reach the IR");

    // THE PROPERTY. `billing:charge` is not in the canonical effect catalogue.
    // Without the extension the PCC prover treats it as an unknown base and
    // refutes; with it, the member is rescued and the proof is over the program
    // the adopter actually wrote. Both halves of PCC call this same function
    // over the same IR, which is why their witnesses agree by construction.
    let members = axon::pcc::generate::extension_effect_members(&ir);
    assert!(
        members.contains("billing:charge"),
        "the PCC prover does not see the declared extension member — this is the \
         whole reason `extension` is not decoration. got: {members:?}"
    );
}

#[test]
fn a_shield_claiming_an_extension_category_no_scanner_implements_is_refused() {
    // The second consumer, and the sharper one: the enterprise deploy path calls
    // this and turns a failure into `flow.phantom_guardrail`.
    //
    // The violation is precise, and I got it wrong on the first pass by assuming
    // instead of reading: an extension alone is NOT a violation. It takes an
    // extension declaring a scan category, a shield claiming to scan it, and no
    // registered scanner implementing it. That is a guardrail that exists on
    // paper — the adopter believes content is being checked, which is exactly
    // the belief that stops them checking it themselves.
    let ir = compile(&fixture("phantom_guardrail.axon"), "phantom_guardrail.axon");

    let verdict = axon::shield_registry::check_extension_scan_coverage(&ir);
    assert!(
        verdict.is_err(),
        "a shield claiming an extension category that no scanner implements must be \
         refused — this is the deploy gate the enterprise server relies on"
    );
    let blame = verdict.unwrap_err();
    assert!(
        blame.contains("prompt_leak"),
        "the refusal must name the uncovered category, or the operator cannot act \
         on it: {blame}"
    );

    // The pair. A shield scanning a CANONICAL category, with no extension in
    // sight, is not a phantom guardrail — and a gate that fired on this would be
    // noise, which is how a real one gets switched off.
    let plain = compile(&fixture("canonical_scan.axon"), "canonical_scan.axon");
    assert!(
        axon::shield_registry::check_extension_scan_coverage(&plain).is_ok(),
        "a shield scanning a canonical category must pass"
    );
}

// ── witness ─────────────────────────────────────────────────────────────────

#[test]
fn a_declared_witness_carries_its_four_facts_into_the_ir() {
    let src = fixture("witness_advantage.axon");
    let ir = compile(&src, "witness_advantage.axon");

    assert_eq!(ir.witnesses.len(), 1, "the witness must reach the IR");
    let w = &ir.witnesses[0];

    // An advantage claim is worth something only if it names what it beat, by
    // how much, and on what data. All four have to survive lowering, or the
    // declaration is a comment.
    assert_eq!(w.name, "RetrievalAdvantage");
    assert_eq!(w.baseline, "cosine", "the baseline must survive lowering");
    assert_eq!(w.metric, "ranking_lift", "the metric must survive lowering");
    assert!(
        (w.threshold - 0.15).abs() < f64::EPSILON,
        "the threshold must survive lowering, got {}",
        w.threshold
    );
    assert!(!w.data.is_empty(), "the evidence set must survive lowering");
}

#[test]
fn a_witness_without_a_baseline_or_a_metric_is_refused() {
    // The property that makes `witness` more than a field: "it is better" cannot
    // be written. `against:` and `metric:` are required, and `metric:` comes
    // from a closed catalogue — so an advantage with nothing to compare against,
    // or measured by a word someone invented, does not compile.
    let src = "witness Vague {\n    claim: \"it is faster\"\n}\n";
    let tokens = axon_frontend::lexer::Lexer::new(src, "<vague>")
        .tokenize()
        .expect("lex");
    let program = axon_frontend::parser::Parser::new(tokens).parse().expect("parse");
    let errors = axon_frontend::type_checker::TypeChecker::new(&program).check();

    let messages: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
    assert!(
        messages.iter().any(|m| m.contains("against")),
        "a witness with no baseline must be refused: {messages:#?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("metric")),
        "a witness with no metric must be refused: {messages:#?}"
    );
}
