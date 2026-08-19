//! v2.69.0 — **`tool.provider` is a closed catalog.**
//!
//! # The defect
//!
//! `provider:` was a **free string with no membership check** — the same disease
//! `resource.kind` had. A **typo** (`htpp`) compiled clean and reached a runtime
//! fallthrough that silently handed the call to the model, which **fabricated**
//! the output. On the one primitive whose whole purpose is that an action's
//! result is **born with an honest epistemic status**, a typo produced an
//! invented one — the v2.67.0 defect (*when the evidence is missing, substitute the
//! belief and report agreement*) on the tool surface.
//!
//! And, exactly as with `resource.kind`, the free string let the **docs and
//! templates accumulate an imaginary catalog** — `brave`, `local`,
//! `code_interpreter`, `openai`, `pgvector`, `chroma`, a dozen more — none of
//! which the runtime ever dispatched.
//!
//! # The fix, and what it deliberately keeps
//!
//! A **non-empty** provider must be in the catalog. That is what kills the typo,
//! at compile.
//!
//! An **empty** provider stays legitimate: it means **LLM-routed** — the tool IS
//! the model (a `Summarize`, a `Classify`). `axon-T948` validates it by its
//! *absence*, not its presence. (An early version of this step also tried to
//! stop an empty provider resolving to the streaming stub — but that "stub" is
//! how an LLM-routed streaming tool routes to the configured backend, not a
//! fabrication. The census over-simplified; the real fabrication was the typo,
//! and the closed catalog closes it at compile.)

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors_of(src: &str) -> String {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

fn accepts(src: &str) {
    let errs = errors_of(src);
    assert!(errs.is_empty(), "expected a clean program, got:\n{errs}");
}

fn refutes(src: &str, code: &str) {
    let errs = errors_of(src);
    assert!(
        errs.contains(code),
        "expected {code} to REFUTE this program. Diagnostics were:\n{errs}"
    );
}

/// **A typo in `provider:` is refused at COMPILE.**
///
/// `htpp` used to compile clean and reach a runtime fallthrough. Now it fails
/// where the adopter is still holding the code, not where they are holding an
/// incident — and, on the streaming path, not where they are holding a fabricated
/// answer they believed was real.
#[test]
fn t948_a_misspelled_provider_is_refused_it_used_to_reach_a_fabricating_fallthrough() {
    refutes(
        r#"tool Search { provider: htpp  runtime: search }"#,
        "axon-T948",
    );
}

/// Every provider the runtime actually dispatches type-checks. The catalog is
/// exactly the union of the two dispatch tables (+ the `bash` technician path).
#[test]
fn every_dispatchable_provider_is_accepted() {
    // The plain providers: no other law attaches.
    for p in ["native", "stub", "stub_stream", "http", "mcp"] {
        accepts(&format!("tool T {{ provider: {p} }}"));
    }
    // The scrape providers pass T948 (the catalog), but carry their own v2.52.0 laws
    // (T904 demands the `web`/`network` effects). Declared so this test isolates
    // T948 rather than colliding with a different, correct law.
    for p in ["scrape_http", "scrape_dom", "scrape_crawl", "scrape_enrich"] {
        let errs = errors_of(&format!("tool T {{ provider: {p}  effects: <web, network> }}"));
        assert!(
            !errs.contains("axon-T948"),
            "`provider: {p}` is in the catalog and must pass T948. Got: {errs}"
        );
    }
    // v2.77.0 — the axon-agora connector providers. Each has a real dispatch
    // arm (`agora_runtime::dispatch_agora`), honoring the catalog's own law:
    // a value here without an arm behind it re-creates the v2.69.0 defect.
    for p in ["agora_linkedin", "agora_facebook", "agora_instagram", "agora_tiktok"] {
        let errs = errors_of(&format!(
            "tool T {{ provider: {p}  runtime: read_comments  effects: <network> }}"
        ));
        assert!(
            !errs.contains("axon-T948"),
            "`provider: {p}` is in the catalog and must pass T948. Got: {errs}"
        );
    }
}

/// **An LLM-routed tool — no `provider:` at all — stays legitimate.**
///
/// This is the case the closed catalog must NOT break: a tool that is just the
/// model. It is validated by the *absence* of a provider, not by naming one.
#[test]
fn an_llm_routed_tool_with_no_provider_is_still_valid() {
    accepts(r#"tool Summarize { max_results: 1 }"#);
}

/// The one that would be the tempting over-reach: refusing the empty provider too.
/// If this ever started failing, every LLM-routed tool in every program would
/// break, and "the tool is the model" would stop being expressible.
#[test]
fn the_empty_provider_is_not_itself_an_error() {
    let errs = errors_of(r#"tool Ask { }"#);
    assert!(
        !errs.contains("axon-T948"),
        "an empty provider is LLM-routed and legitimate — T948 must not fire on it. Got: {errs}"
    );
}
