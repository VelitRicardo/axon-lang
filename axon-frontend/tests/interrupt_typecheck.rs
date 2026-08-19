//! v2.36.0 — the interrupt type-checker: closed-catalog signal, two-exit
//! handler, and duality preserved across the interrupt region (Theorem 1).
//!
//! Negative cases pinned (the paper's falsifiable claims 1–3):
//! - a signal outside the closed `CallInterruptCause` catalog is REJECTED;
//! - a handler that reaches neither `resume` nor `end` is REJECTED (two-exit);
//! - a non-dual pair of interrupt roles is REJECTED (connection law);
//! - a well-formed dual interrupt session type-checks CLEAN.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    let (errs, _warnings) = TypeChecker::new(&prog).check_with_warnings();
    errs.into_iter().map(|e| e.message).collect()
}

/// The canonical clean, dual barge-in.
const DUAL_OK: &str = r#"
session VoiceTurn {
    agent: [
        interrupt { send Token } on CallerSpeech as cause resumable { send Ack, resume }
    ]
    caller: [
        interrupt { receive Token } on CallerSpeech as cause resumable { receive Ack, resume }
    ]
}
"#;

#[test]
fn dual_interrupt_session_type_checks_clean() {
    let errs = errors(DUAL_OK);
    assert!(errs.is_empty(), "expected no type errors, got: {errs:?}");
}

#[test]
fn signal_outside_catalog_is_rejected() {
    let errs = errors(
        r#"
session VoiceTurn {
    agent: [
        interrupt { send Token } on Telepathy as cause resumable { send Ack, resume }
    ]
    caller: [
        interrupt { receive Token } on Telepathy as cause resumable { receive Ack, resume }
    ]
}
"#,
    );
    assert!(
        errs.iter().any(|e| e.contains("CallInterruptCause")),
        "a signal outside the closed catalog must be rejected; got: {errs:?}"
    );
}

#[test]
fn handler_without_exit_is_rejected() {
    // Handler ends in `send Ack` — reaches neither `resume` nor `end`.
    let errs = errors(
        r#"
session VoiceTurn {
    agent: [
        interrupt { send Token } on CallerSpeech as cause resumable { send Ack }
    ]
    caller: [
        interrupt { receive Token } on CallerSpeech as cause resumable { receive Ack }
    ]
}
"#,
    );
    assert!(
        errs.iter().any(|e| e.contains("two-exit") || e.contains("resume` or `end")),
        "a handler with no exit must be rejected; got: {errs:?}"
    );
}

#[test]
fn non_dual_interrupt_is_rejected() {
    // Caller's body receives the WRONG payload (Blob, not Token) → not dual.
    let errs = errors(
        r#"
session VoiceTurn {
    agent: [
        interrupt { send Token } on CallerSpeech as cause resumable { send Ack, resume }
    ]
    caller: [
        interrupt { receive Blob } on CallerSpeech as cause resumable { receive Ack, resume }
    ]
}
"#,
    );
    assert!(
        errs.iter().any(|e| e.contains("duality")),
        "a non-dual interrupt pair must violate the connection law; got: {errs:?}"
    );
}
