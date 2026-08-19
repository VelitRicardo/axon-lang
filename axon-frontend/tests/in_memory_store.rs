//! v1.31.0 (D2) — `in_memory` is a first-class declarable
//! `axonstore` backend.
//!
//! The runtime `StoreRegistry::classify_backend` already maps
//! `"in_memory"` → `StoreHandle::InMemory`. The only gap was the
//! frontend: `VALID_STORE_BACKENDS` omitted `in_memory`, so a
//! source-declared in-memory store was a compile error — and the
//! canonical agent flow could not run or be tested without a live
//! Postgres. 36.x.b closes it.
//!
//! Pins:
//!   1. `axonstore X { backend: in_memory }` type-checks clean.
//!   2. …with NO `connection:` — it is optional for `in_memory`.
//!   3. The three SQL backends still type-check (no regression).
//!   4. An unknown backend is still rejected.
//!   5. The full agent flow — `in_memory` store + retrieve + step +
//!      persist + a streaming `axonendpoint` — compiles with zero
//!      errors.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

fn backend_errors(src: &str) -> Vec<String> {
    errors(src)
        .into_iter()
        .filter(|m| m.to_lowercase().contains("backend"))
        .collect()
}

// ─── section 1 — `in_memory` type-checks clean ────────────────────────────

#[test]
fn s1_in_memory_store_type_checks_clean() {
    let errs = backend_errors("axonstore mem { backend: in_memory connection: \"\" }");
    assert!(
        errs.is_empty(),
        "36.x.b D2: `backend: in_memory` must type-check — it is a \
         first-class declarable axonstore backend. Got: {errs:?}"
    );
}

// ─── section 2 — `connection:` is optional for `in_memory` ────────────────

#[test]
fn s2_in_memory_store_needs_no_connection() {
    let errs = errors("axonstore mem { backend: in_memory }");
    assert!(
        errs.is_empty(),
        "36.x.b D2: an `in_memory` store needs no `connection:` — the \
         declaration must type-check with zero errors. Got: {errs:?}"
    );
}

// ─── section 3 — the backends that EXIST type-check; the ones that don't, don't ──────

/// The backends the runtime can actually build still type-check. No regression.
///
/// (`secrets` carries its own `axon-T900` obligation — a class-less secrets store
/// would enumerate the tenant's ENTIRE secret namespace, so the `class:` is
/// mandatory (v2.48.0). That is a *different* law, and a correct one; the fixture
/// declares it.)
#[test]
fn s3_the_implemented_backends_are_still_valid() {
    for decl in [
        "axonstore s { backend: postgresql }",
        "axonstore s { backend: in_memory }",
        "axonstore s { backend: secrets  class: crm }",
    ] {
        let errs = backend_errors(decl);
        assert!(
            errs.is_empty(),
            "`{decl}` names a backend `classify_backend` implements — it must remain valid. \
             Got: {errs:?}"
        );
    }
}

/// **v2.67.0 — this test used to assert the OPPOSITE, and it was wrong.**
///
/// It was called `s3_sql_backends_still_valid` and it demanded that `mysql` and
/// `sqlite` type-check clean. They did. **And then they died at DEPLOY** with
/// `UnknownBackend`, because `classify_backend` implements three backends and the
/// grammar advertised five.
///
/// The type-checker knew. Its own comment, right above the catalog, said so:
///
/// > *"`mysql` / `sqlite` remain type-check-valid but runtime-absent (a documented
/// > future cycle)"*
///
/// **A gap that has been written down stops looking like a gap** — and this test
/// was the thing holding it in place. It is the same shape as the nine tests v2.67.0
/// found asserting `compute`'s placeholder: *a suite can pin a lie as firmly as a
/// truth, and the build stays green either way.*
///
/// Nothing that worked stops working. A program declaring `mysql` was **already
/// broken**; it now fails while the adopter is still holding the code, instead of
/// while they are holding an incident.
#[test]
fn s3b_a_backend_with_no_implementation_is_refused_at_compile_not_at_deploy() {
    for b in ["mysql", "sqlite"] {
        let errs = backend_errors(&format!("axonstore s {{ backend: {b} connection: \"x\" }}"));
        assert!(
            !errs.is_empty(),
            "`backend: {b}` has NO runtime implementation (`classify_backend` returns None), so \
             it must be refused by the COMPILER. Accepting it here is what let it reach deploy \
             and fail there for years. If you are re-adding it to the grammar, write the backend \
             in the same PR — a catalog is a promise, and a promise costs an implementation."
        );
    }
}

// ─── section 4 — an unknown backend is still rejected ─────────────────────

#[test]
fn s4_unknown_store_backend_still_rejected() {
    let errs = backend_errors("axonstore s { backend: redis connection: \"x\" }");
    assert!(
        errs.iter().any(|m| m.contains("redis")),
        "36.x.b: an unknown store backend is still a compile error. \
         Got: {errs:?}"
    );
}

// ─── section 5 — the full agent flow compiles ─────────────────────────────

#[test]
fn s5_agent_flow_with_in_memory_store_compiles_clean() {
    // retrieve context → deliberate (step) → persist result, against
    // an in-memory store, behind a streaming axonendpoint — the
    // canonical agent shape, now declarable with zero infrastructure.
    let src = "axonstore mem { backend: in_memory }\n\
        flow AgentFlow() -> Unit {\n\
            retrieve mem { where: \"kind = 'history'\" as: history }\n\
            step Deliberate { ask: \"answer\" output: Stream<Token> }\n\
            persist into mem { kind: \"reply\" content: \"done\" }\n\
        }\n\
        axonendpoint AgentE { method: POST path: \"/agent\" \
        execute: AgentFlow backend: stub transport: sse public: true }";
    let errs = errors(src);
    assert!(
        errs.is_empty(),
        "36.x.b D2: the canonical agent flow (retrieve → step → \
         persist) against an `in_memory` store must compile with zero \
         errors. Got: {errs:?}"
    );
}
