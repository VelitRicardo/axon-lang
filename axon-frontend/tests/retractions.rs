//! v2.67.0 (Tier 2) — the retractions: `logic` · `apx` · `taint` · `transact` · `corroborate`.
//!
//! v2.67.0's diagnostic walked every primitive the public README advertises and
//! asked one question of each: *does the runtime do what the description
//! promises?* These five failed in the most specific way — they did **nothing**,
//! while the compiler accepted them and the README sold them.
//!
//! The v2.63.0 precedent applies (`dataspace` was a no-op; `ingest` hallucinated):
//! **fail CLOSED first.** A primitive that silently does nothing is worse than
//! an absent one, because the adopter stops looking for the thing they think
//! they already have. So each of these now REFUSES at compile time, and each
//! diagnostic says what was actually happening and what to do instead.
//!
//! Pins:
//! 1. `logic` is no longer a reserved keyword — it is an ordinary identifier
//!    again (it had zero parser productions for four years while reserving the
//!    word).
//! 2. `import … with apx { … }` is a hard parse error (it was parsed and
//!    silently DISCARDED via `skip_braced_block`).
//! 3. **axon-T936** — `shield { taint: … }` is refused (a DEAD field: carried
//!    into the IR, never read by the runtime).
//! 4. **axon-T937** — `corroborate` is refused (it interpolated the reference's
//!    NAME into an LLM prompt and asked the model to report "agreement
//!    strength" between sources it never fetched).
//! 5. **axon-T938** — `transact` is refused (it opened no transaction; the
//!    block's body was never even lowered into the IR).
//! 6. Nothing else regressed: a normal `shield` / `persist` program still
//!    compiles clean.

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

fn parse_err(src: &str) -> String {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    match Parser::new(tokens).parse() {
        Ok(_) => panic!("expected a parse error, but the program parsed clean"),
        Err(e) => e.message,
    }
}

fn has(errs: &[String], code: &str) -> bool {
    errs.iter().any(|e| e.contains(code))
}

// ── 1. `logic` — the dead keyword is released ───────────────────────────────

/// `logic` was a reserved keyword with NO parser production, no type-checker
/// arm and no IR node — dead in the entire frontend, while the README sold it
/// as primitive #50 ("arithmetic DSL for pure deterministic transforms").
/// `primitive_registry` had already deleted its entry in v1.2.0.
///
/// Retracting it does more than stop the lie: it gives the word back. An
/// adopter could not name a binding `logic`.
#[test]
fn logic_is_an_ordinary_identifier_again() {
    let errs = errors(
        r#"
        type T { logic: String }
        flow F(logic: String) -> String {
            let logic_result = logic
            return logic_result
        }
    "#,
    );
    assert!(
        errs.is_empty(),
        "`logic` must be a plain identifier now that the dead keyword is gone; got {errs:?}"
    );
}

// ── 2. `apx` — parsed and discarded, now refused ────────────────────────────

/// The ONLY handling `apx` ever had was `skip_braced_block()`: the policy was
/// consumed and thrown on the floor. It never reached the AST, let alone the
/// IR. There is no APX crate, no MEC/PCC dependency verification, no EPR
/// ranking, no quarantine, no compliance gate — the README advertised all five.
///
/// This is the worst shape for a supply-chain promise specifically: believing
/// your dependencies are being verified is exactly what stops you from
/// verifying them yourself.
#[test]
fn apx_policy_block_is_refused() {
    let msg = parse_err(
        r#"
        import std.data with apx {
            verify: mec
            quarantine: strict
        }
        type T { x: String }
    "#,
    );
    assert!(
        msg.contains("RETRACTED") && msg.contains("apx"),
        "the apx refusal must name itself as a retraction; got: {msg}"
    );
    assert!(
        msg.contains("DISCARDED") || msg.contains("verified nothing"),
        "the diagnostic must tell the truth about what it USED to do — silently discard the \
         policy — or an adopter will just assume a syntax change; got: {msg}"
    );
}

/// The retraction is surgical: a plain `import` is untouched.
#[test]
fn plain_import_still_parses() {
    let errs = errors(
        r#"
        import std.data
        type T { x: String }
    "#,
    );
    assert!(errs.is_empty(), "a plain import must be unaffected; got {errs:?}");
}

// ── 3. axon-T936 — `shield { taint: }` is a dead field ──────────────────────

/// `taint` was advertised as primitive #45 ("Epistemic trust label for
/// untrusted external data sources"). It was never a primitive — it was a
/// `shield` FIELD, and the field was dead: parsed, copied into the IR by
/// `ir_generator`, and never read back by the runtime. Writing it bought
/// exactly the protection of a comment.
#[test]
fn t936_shield_taint_is_refused() {
    let errs = errors(
        r#"
        shield G {
            scan: [prompt_injection]
            on_breach: halt
            severity: high
            taint: untrusted
        }
    "#,
    );
    assert!(has(&errs, "axon-T936"), "expected axon-T936, got {errs:?}");
    let msg = errs.iter().find(|e| e.contains("axon-T936")).unwrap();
    assert!(
        msg.contains("DEAD"),
        "the adopter must learn the field did nothing, not merely that it is unsupported; got: {msg}"
    );
    assert!(
        msg.contains("T908") || msg.contains("Untrusted"),
        "the epistemic-taint LAW is real and lives in the lattice — the diagnostic must point \
         there, or we retract a promise without saying where it was actually kept; got: {msg}"
    );
}

/// A shield without the dead field still compiles — the gates that DO run
/// (`scan:` / `on_breach:` / `redact:`) are untouched.
#[test]
fn shield_without_taint_compiles_clean() {
    let errs = errors(
        r#"
        shield G {
            scan: [prompt_injection]
            on_breach: halt
            severity: high
            redact: [ssn]
        }
    "#,
    );
    assert!(errs.is_empty(), "a normal shield must still compile; got {errs:?}");
}

// ── 4. axon-T937 — `corroborate` manufactured a warrant ─────────────────────

/// The whole handler was:
///
/// ```text
/// format!("Corroborate navigation result `{}`", node.navigate_ref)
/// ```
///
/// It interpolated the reference's NAME — never its content. It fetched no
/// second source, read nothing, computed no agreement metric… and then told the
/// model: *"Cross-validate independently; surface agreement strength."* So the
/// model invented a corroboration, including how strongly two sources it never
/// saw agreed with each other.
///
/// That is not a missing feature. It is a manufactured warrant, and it is
/// strictly worse than no verification, because a reader trusts it.
#[test]
fn t937_corroborate_is_refused() {
    let errs = errors(
        "flow F() -> Unit {\n\
            corroborate nav\n\
        }",
    );
    assert!(has(&errs, "axon-T937"), "expected axon-T937, got {errs:?}");
    let msg = errs.iter().find(|e| e.contains("axon-T937")).unwrap();
    assert!(
        msg.contains("agreement") || msg.contains("independent"),
        "the diagnostic must name the specific lie — an invented agreement metric over sources \
         never fetched; got: {msg}"
    );
}

// ── 5. axon-T938 — `transact` opened no transaction ─────────────────────────

/// `transact { … }` promised atomicity and delivered a string: the runtime set
/// `__txn_active = "true"` (a key nothing reads), took no lock, opened no
/// transaction and rolled nothing back. `TransactBlock` carries only a source
/// location — the block's BODY was never lowered into the IR at all.
///
/// This is the most dangerous dead primitive of the five, because it is
/// load-bearing exactly when things go wrong: an adopter wraps two `mutate`s in
/// `transact` precisely so a failure between them cannot half-write the store,
/// and got no such guarantee — silently, on the unhappy path they will only
/// meet in production. We shipped it in a knowledge template.
#[test]
fn t938_transact_is_refused() {
    let errs = errors(
        "axonstore mem { backend: in_memory }\n\
        flow F() -> Unit {\n\
            transact {\n\
            }\n\
        }",
    );
    assert!(has(&errs, "axon-T938"), "expected axon-T938, got {errs:?}");
    let msg = errs.iter().find(|e| e.contains("axon-T938")).unwrap();
    assert!(
        msg.contains("idempotent"),
        "refusing atomicity without offering the honest alternative (idempotent writes, so a \
         retry converges instead of corrupting) leaves the adopter stranded; got: {msg}"
    );
}

/// The retraction is surgical: writes OUTSIDE a `transact` are unaffected. They
/// were never any less atomic than writes inside one — which was the whole
/// problem.
#[test]
fn persist_without_transact_compiles_clean() {
    let errs = errors(
        "axonstore mem { backend: in_memory }\n\
        flow F() -> Unit {\n\
            persist into mem { kind: \"audit\" content: \"written\" }\n\
        }",
    );
    assert!(errs.is_empty(), "a plain persist must still compile; got {errs:?}");
}
