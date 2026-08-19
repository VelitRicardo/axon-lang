//! v1.31.0 (D10) — `axon-W003`: the no-`backend:` compile warning.
//!
//! An `axonendpoint` that declares no `backend:` resolves its
//! execution backend at request time down the v1.31.0 precedence
//! ladder. `axon check` now emits an `axon-W003` warning per such
//! endpoint — the adopter learns at compile time that the route
//! relies on ladder resolution, not at the first production 503.
//!
//! Pins:
//!   1. An axonendpoint with no `backend:` raises `axon-W003`.
//!   2. An explicit `backend: <provider>` silences it.
//!   3. An explicit `backend: auto` ALSO silences it — declaring
//!      `auto` is the adopter's deliberate opt-in to ladder
//!      resolution, not an oversight.
//!   4. The warning is one-per-endpoint and names the fixes.
//!   5. W003 is a warning, not an error — `check()` (errors only)
//!      never surfaces it.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::{TypeChecker, W003_CODE};

fn warnings(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    let (_errors, warns) = TypeChecker::new(&prog).check_with_warnings();
    warns.into_iter().map(|w| w.message).collect()
}

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog).check().into_iter().map(|e| e.message).collect()
}

fn w003_count(msgs: &[String]) -> usize {
    msgs.iter().filter(|m| m.contains(W003_CODE)).count()
}

const FLOW: &str = "flow Chat() -> Unit { step S { ask: \"hi\" } }\n";

// ─── section 1 — undeclared backend raises axon-W003 ──────────────────────

#[test]
fn s1_undeclared_backend_raises_w003() {
    let src = format!(
        "{FLOW}axonendpoint E {{ method: POST path: \"/chat\" execute: Chat }}"
    );
    let w = warnings(&src);
    assert_eq!(
        w003_count(&w),
        1,
        "36.k D10: an axonendpoint with no `backend:` must raise one \
         axon-W003. Warnings: {w:?}"
    );
    assert!(
        w.iter().any(|m| m.contains(W003_CODE) && m.contains("'E'")),
        "36.k: the warning must name the endpoint. Warnings: {w:?}"
    );
}

// ─── section 2 — an explicit provider silences W003 ───────────────────────

#[test]
fn s2_declared_provider_silences_w003() {
    let src = format!(
        "{FLOW}axonendpoint E {{ method: POST path: \"/chat\" \
         execute: Chat backend: gemini }}"
    );
    assert_eq!(
        w003_count(&warnings(&src)),
        0,
        "36.k D10: a declared `backend: <provider>` pins the model — \
         no W003"
    );
}

// ─── section 3 — explicit `backend: auto` ALSO silences W003 ──────────────

#[test]
fn s3_explicit_auto_silences_w003() {
    let src = format!(
        "{FLOW}axonendpoint E {{ method: POST path: \"/chat\" \
         execute: Chat backend: auto }}"
    );
    assert_eq!(
        w003_count(&warnings(&src)),
        0,
        "36.k D10: an explicit `backend: auto` is the adopter's \
         deliberate opt-in to ladder resolution — it must NOT warn"
    );
}

// ─── section 4 — one warning per endpoint; names the fixes ────────────────

#[test]
fn s4_one_w003_per_undeclared_endpoint() {
    let src = format!(
        "{FLOW}\
         axonendpoint A {{ method: POST path: \"/a\" execute: Chat }}\n\
         axonendpoint B {{ method: GET  path: \"/b\" execute: Chat }}\n\
         axonendpoint C {{ method: PUT  path: \"/c\" execute: Chat backend: kimi }}"
    );
    let w = warnings(&src);
    assert_eq!(
        w003_count(&w),
        2,
        "36.k: exactly the two undeclared endpoints (A, B) warn; the \
         declared one (C) does not. Warnings: {w:?}"
    );
}

#[test]
fn s5_w003_message_names_the_fixes() {
    let src = format!(
        "{FLOW}axonendpoint E {{ method: POST path: \"/chat\" execute: Chat }}"
    );
    let w = warnings(&src);
    let msg = w.iter().find(|m| m.contains(W003_CODE)).expect("a W003");
    assert!(
        msg.contains("backend:") && msg.contains("auto") && msg.contains("503"),
        "36.k: the warning must name the fixes (declare backend:, \
         backend: auto) and the consequence (503). Got: {msg}"
    );
}

// ─── section 5 — W003 is a warning, never an error ────────────────────────

#[test]
fn s6_w003_is_a_warning_not_an_error() {
    let src = format!(
        "{FLOW}axonendpoint E {{ method: POST path: \"/chat\" execute: Chat }}"
    );
    let errs = errors(&src);
    assert!(
        !errs.iter().any(|m| m.contains(W003_CODE)),
        "36.k D10: W003 is a WARNING — `check()` (errors only) must \
         never surface it. Errors: {errs:?}"
    );
}
