//! v1.31.0 (D2) — the `axonendpoint backend:` declaration.
//!
//! v1.31.0 makes the execution backend a declared, compiled,
//! type-checked property of the program. 36.d lands the front of
//! that contract: an optional `backend:` field on the `axonendpoint`
//! declaration, validated against the closed catalog
//! `CANONICAL_PROVIDERS ∪ {auto, stub}`.
//!
//! Pins:
//!   1. A declared `backend:` is captured into the AST.
//!   2. Every one of the nine catalog entries parses cleanly.
//!   3. An omitted `backend:` leaves the field empty (D9 — not
//! declared ≡ resolve down the v1.31.0 D1 ladder).
//!   4. `backend: stub` parses — the no-op is reachable by an
//!      EXPLICIT declaration (D5 forbids a SILENT stub, not an opt-in).
//!   5. `backend: auto` parses — transparent, equivalent to omitting.
//!   6. An unknown backend is a parse error with a smart-suggest hint.
//!   7. The type-checker re-rejects an unknown backend (an AST built
//!      outside the parser still fails `axon check`).
//!   8. The type-checker accepts a valid declared backend + an empty
//!      one (no false-positive compile error).
//!   9. The closed catalog is exactly the nine documented entries.

use axon_frontend::ast::{Declaration, Program};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::{ParseError, Parser, AXONENDPOINT_BACKEND_VALUES};
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> Result<Program, ParseError> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse()
}

fn endpoint<'a>(prog: &'a Program, name: &str) -> &'a axon_frontend::ast::AxonEndpointDefinition {
    for decl in &prog.declarations {
        if let Declaration::AxonEndpoint(ae) = decl {
            if ae.name == name {
                return ae;
            }
        }
    }
    panic!("axonendpoint {name} not found");
}

/// A minimal, type-correct program: one flow + one endpoint that
/// references it. `backend:` is interpolated so each test can vary it.
fn program_with_backend(backend_field: &str) -> String {
    format!(
        "flow Chat() -> Unit {{ step S {{ ask: \"hi\" }} }}\n\
         axonendpoint E {{ method: POST path: \"/chat\" execute: Chat {backend_field} }}"
    )
}

// ─── section 1 — a declared backend is captured ───────────────────────────

#[test]
fn s1_declared_backend_is_captured() {
    let prog = parse(&program_with_backend("backend: anthropic")).expect("parse");
    assert_eq!(
        endpoint(&prog, "E").backend,
        "anthropic",
        "36.d D2: a declared `backend:` must be captured into the AST"
    );
}

// ─── section 2 — every catalog entry parses ───────────────────────────────

#[test]
fn s2_every_catalog_entry_parses() {
    for &b in AXONENDPOINT_BACKEND_VALUES {
        let prog = parse(&program_with_backend(&format!("backend: {b}")))
            .unwrap_or_else(|e| panic!("36.d: `backend: {b}` must parse — got {}", e.message));
        assert_eq!(
            endpoint(&prog, "E").backend,
            b,
            "36.d: catalog entry `{b}` must round-trip into the AST"
        );
    }
}

// ─── section 3 — omitted backend leaves the field empty (D9) ──────────────

#[test]
fn s3_omitted_backend_is_empty_d9() {
    let prog = parse(&program_with_backend("")).expect("parse");
    assert_eq!(
        endpoint(&prog, "E").backend,
        "",
        "36.d D9: an omitted `backend:` leaves the field empty — the \
         endpoint resolves down the v1.31.0 D1 ladder, no regression"
    );
}

// ─── section 4 — `backend: stub` is an explicit, legal opt-in (D5) ────────

#[test]
fn s4_backend_stub_parses_explicit_optin_d5() {
    let prog = parse(&program_with_backend("backend: stub")).expect("parse");
    assert_eq!(
        endpoint(&prog, "E").backend,
        "stub",
        "36.d D5: `stub` is reachable by an EXPLICIT declaration — D5 \
         forbids a SILENT degradation to stub, not a written opt-in"
    );
}

// ─── section 5 — `backend: auto` is transparent but legal ─────────────────

#[test]
fn s5_backend_auto_parses_transparent() {
    let prog = parse(&program_with_backend("backend: auto")).expect("parse");
    assert_eq!(
        endpoint(&prog, "E").backend,
        "auto",
        "36.d: `auto` is a legal declaration — transparent, equivalent \
         to omitting `backend:` (resolves down the D1 ladder)"
    );
}

// ─── section 6 — unknown backend is a parse error w/ smart-suggest ────────

#[test]
fn s6_unknown_backend_is_parse_error_with_hint() {
    let err = parse(&program_with_backend("backend: anthropicc"))
        .expect_err("36.d D2: an unknown backend must be a parse error");
    let msg = err.message.to_lowercase();
    assert!(
        msg.contains("invalid backend") && msg.contains("anthropicc"),
        "36.d: the diagnostic must name the offending backend. Got: {}",
        err.message
    );
    assert!(
        msg.contains("anthropic") || msg.contains("expected"),
        "36.d: smart-suggest should point at the nearest catalog entry. \
         Got: {}",
        err.message
    );
}

#[test]
fn s6_unknown_backend_typo_gemniai_suggests_gemini() {
    let err = parse(&program_with_backend("backend: gemniai"))
        .expect_err("36.d: `gemniai` is not in the catalog");
    assert!(
        err.message.to_lowercase().contains("gemini"),
        "36.d: smart-suggest should recover `gemini` from `gemniai`. \
         Got: {}",
        err.message
    );
}

// ─── section 7 — the type-checker re-rejects an unknown backend ───────────

#[test]
fn s7_type_checker_rejects_unknown_backend() {
    // Parse a valid program, then inject an impossible backend the
    // parser would have rejected — emulating an AST built by the LSP
    // or constructed programmatically. `axon check` must still fail.
    let mut prog = parse(&program_with_backend("backend: anthropic")).expect("parse");
    for decl in &mut prog.declarations {
        if let Declaration::AxonEndpoint(ae) = decl {
            ae.backend = "not_a_real_backend".to_string();
        }
    }
    let errors = TypeChecker::new(&prog).check();
    assert!(
        errors.iter().any(|e| {
            let m = e.message.to_lowercase();
            m.contains("unknown backend") && m.contains("not_a_real_backend")
        }),
        "36.d D2: the type-checker must reject an unknown backend as a \
         compile error. Errors: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ─── section 8 — no false-positive on a valid / empty backend ─────────────

#[test]
fn s8_type_checker_accepts_valid_and_empty_backend() {
    for field in ["backend: gemini", "backend: stub", "backend: auto", ""] {
        let prog = parse(&program_with_backend(field)).expect("parse");
        let errors = TypeChecker::new(&prog).check();
        let backend_errs: Vec<_> = errors
            .iter()
            .filter(|e| e.message.to_lowercase().contains("backend"))
            .collect();
        assert!(
            backend_errs.is_empty(),
            "36.d: `{field}` must not raise a backend compile error. \
             Got: {:?}",
            backend_errs.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
    }
}

// ─── section 9 — the closed catalog is exactly the nine entries ───────────

#[test]
fn s9_backend_catalog_is_the_closed_nine() {
    assert_eq!(
        AXONENDPOINT_BACKEND_VALUES.len(),
        9,
        "36.d D2: the catalog is `CANONICAL_PROVIDERS (7) ∪ {{auto, \
         stub}}` = exactly 9. Adding a provider requires a deliberate \
         step + the axon-rs cross-stack drift gate update."
    );
    let expected: std::collections::HashSet<&str> = [
        "anthropic",
        "auto",
        "gemini",
        "glm",
        "kimi",
        "ollama",
        "openai",
        "openrouter",
        "stub",
    ]
    .into_iter()
    .collect();
    let actual: std::collections::HashSet<&str> =
        AXONENDPOINT_BACKEND_VALUES.iter().copied().collect();
    assert_eq!(actual, expected, "36.d: closed-catalog set drift");
}
