//! §Fase 119.b.3 — Vía C: the compiler verifies the stability BAND, not a sign.
//!
//! # What this gate protects
//!
//! README §XV promises compile-time rejection of unstable gains, and until this
//! sub-fase the checker implemented exactly the published sign tests:
//! `Kp > 0 ∧ Ki ≥ 0 ∧ Kd ≥ 0`. The two papers show that to be **necessary but
//! not sufficient**, in opposite directions:
//!
//! - `paper_mandate.md` §3: the Lyapunov argument is conditional on the control
//!   effort **exceeding the drift bound `D`** — too small and the cage is open;
//! - `paper_mathematical_prompt_optimization.md` §6.3: convergence needs
//!   `|Kp+Ki+Kd| < 1/L` — too large and the loop diverges.
//!
//! A sign test admits both failure modes. The band `D < |Kp+Ki+Kd| < 1/L`
//! admits neither.
//!
//! # How a static checker verifies an empirical condition — it doesn't
//!
//! `D` and `L` are measured properties of a backend the compiler cannot see.
//! Nothing here fabricates them. The developer **declares** them —
//! `stability { D: 0.5, L: 0.4 }` — and the compiler verifies the theorem
//! conditionally on the declaration, which then travels in the IR as a proof
//! obligation for dispatch. A mandate that declares nothing gets the sign
//! checks only, and its IR carries `None`: absence is visible, never defaulted.
//!
//! This is the same construction as §51's proof-carrying code and §91's `now:`
//! declared time — hypotheses stated, checked against, and discharged at the
//! boundary that can measure them.

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn check(src: &str) -> Vec<String> {
    let program = parse(src);
    TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

/// A fully declared, admissible mandate: effort 2.4 ∈ (D=0.5, 1/L=2.5).
const ADMISSIBLE: &str = r#"
mandate SECCompliance {
    constraint: "no forward-looking statements without safe harbor language"
    pid { Kp: 2.0, Ki: 0.3, Kd: 0.1 }
    stability { D: 0.5, L: 0.4 }
    epsilon: 0.05
    max_steps: 8
    on_violation: halt
}
"#;

#[test]
fn an_admissible_fully_declared_mandate_compiles_clean() {
    let errors = check(ADMISSIBLE);
    assert!(errors.is_empty(), "expected no errors, got {errors:?}");
}

#[test]
fn gains_that_pass_every_sign_test_are_refused_below_the_declared_drift() {
    // THE test of the sub-fase. Kp, Ki, Kd all positive — every published
    // check passes — and the mandate is still refused, because the developer
    // declared the drift these gains must beat and they cannot.
    let errors = check(
        r#"
mandate Weak {
    constraint: "c"
    pid { Kp: 0.3, Ki: 0.1, Kd: 0.05 }
    stability { D: 0.8, L: 0.5 }
    epsilon: 0.05
    max_steps: 8
    on_violation: halt
}
"#,
    );
    assert_eq!(errors.len(), 1, "exactly one defect: {errors:?}");
    let msg = &errors[0];
    assert!(msg.contains("mandate 'Weak'"), "{msg}");
    assert!(
        msg.contains("0.45") && msg.contains("0.8"),
        "the refusal names the developer's numbers: {msg}"
    );
    assert!(
        msg.contains("paper_mandate"),
        "the refusal cites the precondition it enforces: {msg}"
    );
}

#[test]
fn gains_above_the_declared_lipschitz_ceiling_are_refused() {
    let errors = check(
        r#"
mandate Hot {
    constraint: "c"
    pid { Kp: 2.0, Ki: 0.3, Kd: 0.1 }
    stability { D: 0.1, L: 0.5 }
    epsilon: 0.05
    max_steps: 8
    on_violation: halt
}
"#,
    );
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("oscillate or diverge"),
        "{}",
        errors[0]
    );
}

#[test]
fn a_declared_empty_band_is_refused_as_a_property_of_the_backend() {
    // D = 3.0 ≥ 1/L = 2.0: no gains whatever could satisfy this declaration.
    let errors = check(
        r#"
mandate Uncageable {
    constraint: "c"
    pid { Kp: 1.0 }
    stability { D: 3.0, L: 0.5 }
    epsilon: 0.05
    max_steps: 8
    on_violation: halt
}
"#,
    );
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("band is empty"), "{}", errors[0]);
}

#[test]
fn bounds_without_gains_are_a_premise_with_no_theorem() {
    let errors = check(
        r#"
mandate NoController {
    constraint: "c"
    stability { D: 0.5, L: 0.4 }
    epsilon: 0.05
    max_steps: 8
    on_violation: halt
}
"#,
    );
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("no Kp"), "{}", errors[0]);
}

#[test]
fn without_a_stability_block_the_sign_checks_still_apply_and_nothing_more() {
    // Backwards compatibility is a theorem about the judgment, not a hope:
    // every mandate written before §119.b.3 declares no bounds and must keep
    // compiling exactly as before.
    let errors = check(
        r#"
mandate Legacy {
    constraint: "c"
    pid { Kp: 2.0, Ki: 0.3, Kd: 0.1 }
    epsilon: 0.05
    max_steps: 8
    on_violation: halt
}
"#,
    );
    assert!(errors.is_empty(), "{errors:?}");

    // And the signs still bite without bounds.
    let errors = check(r#"mandate Bad { constraint: "c" pid { Kp: -1.0 } }"#);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("no restoring force"), "{}", errors[0]);
}

#[test]
fn epsilon_above_one_is_refused_because_e_cannot_exceed_one() {
    // e = 1 − CSR ∈ [0, 1]. ε > 1 means even total violation converges — a
    // mandate that could never fail, which is the vacuous guarantee this
    // entire fase exists to hunt.
    let errors = check(
        r#"
mandate Vacuous {
    constraint: "c"
    pid { Kp: 2.0 }
    epsilon: 1.5
    max_steps: 8
    on_violation: halt
}
"#,
    );
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("(0, 1]"), "{}", errors[0]);
}

#[test]
fn a_negative_declared_bound_is_a_category_error_not_a_loose_bound() {
    let errors = check(
        r#"
mandate Nonsense {
    constraint: "c"
    pid { Kp: 2.0 }
    stability { D: -0.5, L: 0.4 }
    epsilon: 0.05
    max_steps: 8
    on_violation: halt
}
"#,
    );
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("sup|drift(t)|"),
        "the refusal explains WHY a negative D is impossible: {}",
        errors[0]
    );
}

#[test]
fn an_empty_stability_block_is_a_parse_error_not_a_silent_no_op() {
    // The §119.b.1 lesson applied forward: a block the parser walks past
    // silently is a specification discarded with a green checkmark. This one
    // refuses at parse, where the block is still visible.
    let tokens = Lexer::new(
        r#"mandate M { constraint: "c" pid { Kp: 1.0 } stability { } }"#,
        "<test>",
    )
    .tokenize()
    .expect("lex");
    let err = Parser::new(tokens).parse().expect_err("must not parse");
    assert!(
        err.message.contains("neither `D` nor `L`"),
        "{}",
        err.message
    );
}

#[test]
fn the_declared_hypotheses_travel_in_the_ir_as_proof_obligations() {
    let program = parse(ADMISSIBLE);
    let errors = TypeChecker::new(&program).check();
    assert!(errors.is_empty(), "{errors:?}");
    let ir = IRGenerator::new().generate(&program);
    let m = ir
        .mandate_specs
        .iter()
        .find(|m| m.name == "SECCompliance")
        .expect("the mandate reaches the IR");
    assert_eq!(
        (m.drift_bound, m.lipschitz),
        (Some(0.5), Some(0.4)),
        "dispatch cannot discharge an obligation the IR does not carry"
    );
    assert_eq!((m.kp, m.ki, m.kd), (Some(2.0), Some(0.3), Some(0.1)));
}

#[test]
fn an_undeclared_band_reaches_the_ir_as_none_never_as_a_default() {
    // The runtime must be able to SEE that nothing was statically promised.
    // A fabricated default here would convert "unverified" into "verified
    // against numbers nobody measured" — the vacuous gate, one layer down.
    let program = parse(
        r#"
mandate Legacy {
    constraint: "c"
    pid { Kp: 2.0 }
    epsilon: 0.05
    max_steps: 8
    on_violation: halt
}
"#,
    );
    let ir = IRGenerator::new().generate(&program);
    let m = &ir.mandate_specs[0];
    assert_eq!((m.drift_bound, m.lipschitz), (None, None));
}

#[test]
fn the_flat_bound_spellings_parse_too() {
    // `drift_bound:` / `lipschitz:` inside the block, for the developer who
    // finds single capital letters opaque. Same fields, no precedence games.
    let program = parse(
        r#"
mandate M {
    constraint: "c"
    pid { Kp: 2.0 }
    stability { drift_bound: 0.5, lipschitz: 0.4 }
    epsilon: 0.05
    max_steps: 8
    on_violation: halt
}
"#,
    );
    let ir = IRGenerator::new().generate(&program);
    assert_eq!(
        (ir.mandate_specs[0].drift_bound, ir.mandate_specs[0].lipschitz),
        (Some(0.5), Some(0.4))
    );
}
