//! v2.62.0 — the HTTP QUERY method (RFC 10008) + `axon-T927`, the safety law.
//!
//! RFC 10008 (Proposed Standard, June 2026) adds QUERY: safe + idempotent +
//! cacheable, WITH a request body — closing the "GET has no body / POST is not
//! safe" gap. Its section 2 says a QUERY MUST be processed "in a safe and idempotent
//! manner". Everywhere else that MUST is a convention nobody enforces; here it is
//! a compile-time proof.
//!
//! Pins:
//! 1. `method: QUERY` is in the closed endpoint catalog (and CORS inherits it).
//! 2. A QUERY endpoint over a READ-ONLY flow compiles clean.
//! 3. **axon-T927** — a QUERY endpoint whose flow performs a declared write
//!    (`persist`/`mutate`/`purge`/`emit`/`publish`/`rotate`/`mint`/`transact`) is
//!    REFUSED, and the diagnostic names the offending verb.
//! 4. Soundness: a write NESTED inside `if` / `for` / `par` / `warden` is still
//!    caught — a proof that misses a nested write is not a proof.
//! 5. A program declaring a `deliver` (v2.60.0) / `document` (v2.62.0) egress cannot host
//! a QUERY endpoint (those fire post-run for ANY flow, the design decision-B).
//! 6. The same write under `method: POST` is fine — the law is QUERY-specific.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

fn has_t927(errs: &[String]) -> bool {
    errs.iter().any(|e| e.contains("axon-T927"))
}

// ── 1. QUERY is a first-class method ────────────────────────────────────────

#[test]
fn query_is_in_the_closed_method_catalog() {
    let src = "flow Search() -> Unit { step S { ask: \"find\" } }\n\
        axonendpoint E { method: QUERY path: \"/search\" execute: Search backend: stub }";
    let errs = errors(src);
    assert!(
        !errs.iter().any(|e| e.contains("Unknown HTTP method")),
        "QUERY (RFC 10008) must be a valid axonendpoint method. Got: {errs:?}"
    );
}

#[test]
fn query_is_declarable_in_cors_allow_methods() {
    // axon-T855 — the cors catalog REUSES the endpoint method catalog, so QUERY
    // becomes declarable for free. (The RFC does not safelist QUERY: a browser
    // preflights it, so an adopter MUST list it.)
    let src = "cors Api { allow_origins: [\"https://app.example\"] allow_methods: [QUERY] }\n\
        flow Search() -> Unit { step S { ask: \"find\" } }\n\
        axonendpoint E { method: QUERY path: \"/search\" execute: Search backend: stub cors: Api }";
    let errs = errors(src);
    assert!(
        !errs.iter().any(|e| e.contains("axon-T855")),
        "QUERY must be declarable in cors allow_methods (T855 reuses the catalog). Got: {errs:?}"
    );
}

// ── 2. A read-only QUERY compiles clean ─────────────────────────────────────

#[test]
fn query_over_a_read_only_flow_compiles_clean() {
    // `retrieve` is a READ — the canonical QUERY shape (a complex filter in the
    // body, a store read, a result). This is what QUERY exists for.
    let src = "axonstore mem { backend: in_memory }\n\
        flow Search() -> Unit {\n\
            retrieve mem { where: \"kind = 'lead'\" as: hits }\n\
        }\n\
        axonendpoint E { method: QUERY path: \"/search\" execute: Search backend: stub }";
    let errs = errors(src);
    assert!(
        !has_t927(&errs),
        "a read-only QUERY flow must compile — T927 only refuses WRITES. Got: {errs:?}"
    );
}

// ── 3. axon-T927 — the safety law (the flagship) ────────────────────────────

#[test]
fn t927_query_flow_that_persists_is_refused() {
    let src = "axonstore mem { backend: in_memory }\n\
        flow Search() -> Unit {\n\
            retrieve mem { where: \"kind = 'lead'\" as: hits }\n\
            persist into mem { kind: \"audit\" content: \"searched\" }\n\
        }\n\
        axonendpoint E { method: QUERY path: \"/search\" execute: Search backend: stub }";
    let errs = errors(src);
    assert!(has_t927(&errs), "a QUERY flow that persists must be refused: {errs:?}");
    assert!(
        errs.iter().any(|e| e.contains("persist")),
        "the diagnostic must NAME the offending write verb: {errs:?}"
    );
}

#[test]
fn t927_query_flow_that_emits_is_refused() {
    // A channel `emit` is egress — observable state change, not safe.
    let src = "channel Bus { message: Text }\n\
        flow Search() -> Unit {\n\
            step S { ask: \"find\" }\n\
            emit Bus(S)\n\
        }\n\
        axonendpoint E { method: QUERY path: \"/search\" execute: Search backend: stub }";
    let errs = errors(src);
    assert!(has_t927(&errs), "a QUERY flow that emits must be refused: {errs:?}");
}

// ── 4. Soundness — a NESTED write is still a write ──────────────────────────

#[test]
fn t927_catches_a_write_nested_in_a_conditional() {
    // The proof must not be defeatable by one level of indentation.
    let src = "axonstore mem { backend: in_memory }\n\
        flow Search(hot: Bool) -> Unit {\n\
            retrieve mem { where: \"kind = 'lead'\" as: hits }\n\
            if hot {\n\
                persist into mem { kind: \"audit\" content: \"hot\" }\n\
            }\n\
        }\n\
        axonendpoint E { method: QUERY path: \"/search\" execute: Search backend: stub }";
    let errs = errors(src);
    assert!(
        has_t927(&errs),
        "a write NESTED in an `if` must still be caught — otherwise the proof is worthless: {errs:?}"
    );
}

// ── 5. A program-level egress declaration poisons QUERY (the design decision-B) ──────────

#[test]
fn t927_query_in_a_program_declaring_a_deliver_is_refused() {
    // v2.60.0 `deliver` fires POST-RUN for ANY flow the executor runs, so a QUERY
    // endpoint in this program would write a CRM row — it cannot be safe.
    let src = "deliver PushLead { target: crm  secret: k  effects: <web>\n\
            upsert_contact { key: lead_email  email: lead_email }\n\
        }\n\
        flow Search() -> Unit { step S { ask: \"find\" } }\n\
        axonendpoint E { method: QUERY path: \"/search\" execute: Search backend: stub }";
    let errs = errors(src);
    assert!(
        has_t927(&errs),
        "a QUERY endpoint cannot coexist with a `deliver` (it fires for every flow): {errs:?}"
    );
}

// ── 6. The law is QUERY-specific — POST may write freely ────────────────────

#[test]
fn the_same_write_under_post_is_fine() {
    let src = "axonstore mem { backend: in_memory }\n\
        flow Save() -> Unit {\n\
            persist into mem { kind: \"audit\" content: \"x\" }\n\
        }\n\
        axonendpoint E { method: POST path: \"/save\" execute: Save backend: stub }";
    let errs = errors(src);
    assert!(
        !has_t927(&errs),
        "T927 governs QUERY only — a POST is free to change state: {errs:?}"
    );
}
