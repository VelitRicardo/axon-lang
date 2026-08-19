//! v2.83.0 — the `mandate` control law survives the parser.
//!
//! # The defect this gate exists to prevent from recurring
//!
//! README XV publishes every `mandate` with its gains in a `pid { … }` block
//! and its convergence band as `epsilon:`. The parser accepted neither. It did
//! not *reject* them either — `pid { … }` fell into `skip_braced_block()` and
//! `epsilon:` into `skip_value()`, so on the README's own example, verbatim:
//!
//! ```text
//! $ axon check mandate.axon
//! ✓ mandate.axon  31 tokens · 1 declarations · 0 errors
//! ```
//!
//! …while the IR that came out the other side read
//! `kp: None, ki: None, kd: None, tolerance: None`.
//!
//! **Every gain and the tolerance were discarded, and the compiler reported
//! success.** That is strictly worse than a parse error: a parse error tells the
//! developer their program is not the program they wrote. Silent acceptance
//! tells them it is. `mandate`'s whole guarantee is a controller, `u(t) =
//! Kp·e(t) + Ki·∫e + Kd·de/dt`; with the gains dropped there is no controller,
//! and nothing anywhere would have said so.
//!
//! This is the v2.67.0 defect — an advertised primitive whose cable is cut — living
//! in the parser rather than the runtime, which is why v2.67.0's own audit did not
//! catch it: the declaration parsed, so it looked wired.
//!
//! # What is gated here
//!
//! Both spellings reach the same fields, the README's exact block round-trips
//! with every parameter populated, and — the assertion that would have caught
//! the original defect — **no parameter is silently `None`.**

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn mandate_of(src: &str) -> axon_frontend::ast::MandateDefinition {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    program
        .declarations
        .into_iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Mandate(m) => Some(m),
            _ => None,
        })
        .expect("the program declares a mandate")
}

/// The README XV block, verbatim. If this test is ever edited to make it pass,
/// the README must be edited in the same commit — they are the same promise.
const README_SEC_MANDATE: &str = r#"
mandate SECCompliance {
    constraint: "Output must be a valid SEC 10-K section with
                 GAAP-compliant financial tables, footnote references,
                 and no forward-looking statements without safe harbor language"
    pid { Kp: 2.0, Ki: 0.3, Kd: 0.1 }
    epsilon: 0.05
    max_steps: 8
    on_violation: halt
}
"#;

#[test]
fn the_published_mandate_block_round_trips_with_every_parameter_populated() {
    let m = mandate_of(README_SEC_MANDATE);

    assert_eq!(m.name, "SECCompliance");
    assert!(
        m.constraint.contains("safe harbor language"),
        "the constraint text must survive its line breaks, got {:?}",
        m.constraint
    );

    // The four that used to vanish.
    assert_eq!(m.kp, Some(2.0), "Kp was dropped by the `pid` block");
    assert_eq!(m.ki, Some(0.3), "Ki was dropped by the `pid` block");
    assert_eq!(m.kd, Some(0.1), "Kd was dropped by the `pid` block");
    assert_eq!(
        m.tolerance,
        Some(0.05),
        "`epsilon:` was dropped by skip_value()"
    );

    assert_eq!(m.max_steps, Some(8));
    assert_eq!(m.on_violation, "halt");
}

/// The assertion stated as the invariant rather than as four equalities: a
/// mandate that parses must not come out with a hole in its control law.
///
/// This is the one that generalises. A future field added to `pid { }` and
/// forgotten in the match would be caught here even if nobody thought to write
/// an equality for it.
#[test]
fn no_control_parameter_is_silently_absent_after_a_successful_parse() {
    let m = mandate_of(README_SEC_MANDATE);
    let missing: Vec<&str> = [
        ("Kp", m.kp.is_none()),
        ("Ki", m.ki.is_none()),
        ("Kd", m.kd.is_none()),
        ("epsilon", m.tolerance.is_none()),
        ("max_steps", m.max_steps.is_none()),
    ]
    .into_iter()
    .filter(|(_, absent)| *absent)
    .map(|(name, _)| name)
    .collect();
    assert!(
        missing.is_empty(),
        "the parse reported success while discarding {missing:?}; a mandate \
         with a hole in its control law is not a mandate"
    );
}

/// The flat form predates the `pid` block and is still accepted. Removing it
/// would break every program written against the parser rather than the README.
#[test]
fn the_flat_gain_form_still_parses() {
    let m = mandate_of(
        r#"
mandate Flat {
    constraint: "must contain \"x\""
    kp: 1.0
    ki: 0.2
    kd: 0.05
    tolerance: 0.1
    max_steps: 3
    on_violation: halt
}
"#,
    );
    assert_eq!((m.kp, m.ki, m.kd), (Some(1.0), Some(0.2), Some(0.05)));
    assert_eq!(m.tolerance, Some(0.1));
}

/// `epsilon` and `tolerance` are the same ε — the convergence band of
/// `∃t ≤ N : |e(t)| < ε`. Two spellings, one field, no precedence surprise.
#[test]
fn epsilon_and_tolerance_are_the_same_field() {
    let with_epsilon = mandate_of(
        r#"mandate A { constraint: "c" kp: 1.0 epsilon: 0.25 max_steps: 1 on_violation: halt }"#,
    );
    let with_tolerance = mandate_of(
        r#"mandate A { constraint: "c" kp: 1.0 tolerance: 0.25 max_steps: 1 on_violation: halt }"#,
    );
    assert_eq!(with_epsilon.tolerance, Some(0.25));
    assert_eq!(with_tolerance.tolerance, with_epsilon.tolerance);
}

/// Lower-case gains inside the block, and a trailing comma, both parse. The
/// README uses `Kp`; the flat form has always accepted `kp`; a developer moving
/// a flat declaration into a `pid` block should not be punished for it.
#[test]
fn the_pid_block_accepts_both_casings_and_a_trailing_comma() {
    let m = mandate_of(
        r#"mandate A { constraint: "c" pid { kp: 4.0, ki: 0.5, kd: 0.25, } epsilon: 0.1 max_steps: 2 on_violation: halt }"#,
    );
    assert_eq!((m.kp, m.ki, m.kd), (Some(4.0), Some(0.5), Some(0.25)));
}

/// A `pid` block naming only some gains leaves the others `None` rather than
/// defaulting them to zero. `Ki = 0` and "no integral term declared" are
/// different statements, and only the declaration site knows which was meant;
/// collapsing them here would hand the controller a fabricated gain.
#[test]
fn an_omitted_gain_stays_absent_rather_than_becoming_zero() {
    let m = mandate_of(
        r#"mandate A { constraint: "c" pid { Kp: 2.0 } epsilon: 0.1 max_steps: 2 on_violation: halt }"#,
    );
    assert_eq!(m.kp, Some(2.0));
    assert_eq!(m.ki, None, "an undeclared Ki must not be invented as 0.0");
    assert_eq!(m.kd, None, "an undeclared Kd must not be invented as 0.0");
}
