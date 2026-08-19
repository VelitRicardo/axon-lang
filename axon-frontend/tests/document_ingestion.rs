//! v2.54.0 — the parse/infer provenance split in the type system.
//! See `docs/cycle/document_ingestion_surgical_edit.md`.
//!
//! Pinned properties:
//! 1. `ingest:parsed` / `ingest:inferred` are valid provenance annotations
//! (NOT effect bases — the design decision); an unknown class is `axon-T1000`.
//! 2. **axon-T1001** — the Inferred ceiling: `ingest:inferred` + `epistemic:know`
//!    is a compile error (a model's belief about pixels can never be `know`).
//! 3. **axon-T908** — the ingestion barrier: a flow feeding ingested content to
//! an agent's beliefs with no shield fails (inherits v2.52.0's barrier); with a
//!    shield it passes.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn check_errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<t>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

// ── 1. ingest:<class> provenance annotation ─────────────────────────────────

#[test]
fn ingest_parsed_is_valid() {
    let src = r#"tool DocReader { provider: native effects: <io, ingest:parsed> parameters: { path: String } }"#;
    let errs = check_errors(src);
    assert!(!errs.iter().any(|e| e.contains("axon-T1000") || e.contains("Unknown effect")), "{errs:?}");
}

#[test]
fn ingest_inferred_is_valid_class() {
    let src = r#"tool Ocr { provider: native effects: <io, ingest:inferred, epistemic:believe> parameters: { img: String } }"#;
    let errs = check_errors(src);
    assert!(!errs.iter().any(|e| e.contains("axon-T1000")), "{errs:?}");
}

#[test]
fn t1000_unknown_ingest_class() {
    let src = r#"tool Bad { provider: native effects: <io, ingest:guessed> parameters: { x: String } }"#;
    let errs = check_errors(src);
    assert!(errs.iter().any(|e| e.contains("axon-T1000")), "{errs:?}");
}

// ── 2. axon-T1001 — the Inferred ceiling ────────────────────────────────────

#[test]
fn t1001_inferred_cannot_be_know() {
    let src = r#"tool Ocr { provider: native effects: <io, ingest:inferred, epistemic:know> parameters: { img: String } }"#;
    let errs = check_errors(src);
    assert!(
        errs.iter().any(|e| e.contains("axon-T1001")),
        "an inferred read can never be `know`: {errs:?}"
    );
}

#[test]
fn inferred_may_be_believe() {
    let src = r#"tool Ocr { provider: native effects: <io, ingest:inferred, epistemic:believe> parameters: { img: String } }"#;
    let errs = check_errors(src);
    assert!(!errs.iter().any(|e| e.contains("axon-T1001")), "{errs:?}");
}

#[test]
fn parsed_may_be_know() {
    // Parsed content is re-derivable from bytes → a shield may elevate it to know.
    let src = r#"tool DocReader { provider: native effects: <io, ingest:parsed, epistemic:know> parameters: { path: String } }"#;
    let errs = check_errors(src);
    assert!(!errs.iter().any(|e| e.contains("axon-T1001")), "{errs:?}");
}

// ── 3. axon-T908 — the ingestion barrier ────────────────────────────────────

const SCAFFOLD: &str = r#"
type Doc { text: String }
type Summary { text: String }
tool DocReader { provider: native  parameters: { path: String }  output_type: Doc  effects: <io, ingest:parsed> }
persona Analyst { domain: ["docs"] }
shield DocShield { scan: [prompt_injection, pii_leak] on_breach: quarantine severity: high }
"#;

#[test]
fn t908_unshielded_ingested_to_agent_is_refused() {
    let src = format!(
        "{SCAFFOLD}
flow Read() -> Summary {{
    use DocReader(path = \"in.docx\")
    step Summarize {{ given: Read  ask: \"Summarize the document\"  output: Summary }}
}}
"
    );
    let errs = check_errors(&src);
    assert!(
        errs.iter().any(|e| e.contains("axon-T908")),
        "ingested content must be shielded before an agent: {errs:?}"
    );
}

#[test]
fn t908_shielded_ingested_passes() {
    let src = format!(
        "{SCAFFOLD}
flow Read() -> Summary {{
    use DocReader(path = \"in.docx\")
    shield DocShield on doc -> Doc
    step Summarize {{ given: Read  ask: \"Summarize the document\"  output: Summary }}
}}
"
    );
    let errs = check_errors(&src);
    assert!(!errs.iter().any(|e| e.contains("axon-T908")), "a shielded ingest flow passes: {errs:?}");
}
