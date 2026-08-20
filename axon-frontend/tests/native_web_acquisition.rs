//! v2.52.0 — grammar + AST + IR + type-checker for Native Web
//! Acquisition (`scrape_http` / `scrape_dom` / `scrape_crawl` providers + the
//! closed-catalog `scrape: { … }` block + the `web` effect base).
//! See `the design plan` (axon-enterprise).
//!
//! Pinned properties:
//! 1. A full scrape `tool` parses into `ToolDefinition.scrape` (ScrapeSpec).
//! 2. It lowers to `IRToolSpec.scrape`; a non-scrape tool ELIDES the key
//! (IR-SHA invariance — byte-identical to pre-v2.52.0 IR).
//! 3. The `web` effect base is accepted in an `effects: <…>` row.
//! 4. **axon-T904** — effect honesty: a scrape provider missing `web`; a
//!    `scrape_dom` dishonestly declaring `network`; an `adaptive:` DOM tool
//!    missing `storage`.
//! 5. **axon-T905** — a `scrape:` block on a non-scrape tool; a bad engine;
//!    a crawl-only field on a `scrape_http` tool.
//! 6. **axon-T906** — a malformed `extract` FieldSpec.
//! 7. **axon-T907** — `similarity_floor` out of [0,1].
//! 8. **axon-T908** — the CONTENT-INJECTION BARRIER: a flow that scrapes and
//!    reasons with no shield is refused; a shielded flow passes.
//! 9. **the design decision** — an unknown field in a `scrape:` block is a hard parse error.

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::{ParseError, Parser};
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn try_parse(src: &str) -> Result<axon_frontend::ast::Program, ParseError> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse()
}

fn check_errors(src: &str) -> Vec<String> {
    let prog = parse(src);
    TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

fn first_tool(prog: &axon_frontend::ast::Program) -> &axon_frontend::ast::ToolDefinition {
    prog.declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Tool(t) => Some(t),
            _ => None,
        })
        .expect("no tool declaration")
}

fn ir_json(src: &str) -> String {
    let prog = parse(src);
    let ir = IRGenerator::new().generate(&prog);
    serde_json::to_string(&ir).expect("serialize IR")
}

// ── 1. Grammar + AST ─────────────────────────────────────────────────────────

const HTTP_TOOL: &str = r#"
tool FetchNews {
    provider: scrape_http
    parameters: { url: String }
    output_type: RawPage
    effects: <network, web>
    timeout: 30s
    scrape: {
        engine: impersonate
        impersonate: chrome
        respect_robots: true
    }
}
"#;

#[test]
fn parses_scrape_http_into_ast() {
    let prog = parse(HTTP_TOOL);
    let tool = first_tool(&prog);
    assert_eq!(tool.provider, "scrape_http");
    let s = tool.scrape.as_ref().expect("scrape block");
    assert_eq!(s.engine.as_deref(), Some("impersonate"));
    assert_eq!(s.impersonate.as_deref(), Some("chrome"));
    assert_eq!(s.respect_robots, Some(true));
}

#[test]
fn parses_scrape_dom_extract_specs() {
    let src = r#"
tool Extract {
    provider: scrape_dom
    parameters: { page: RawPage }
    output_type: Json
    effects: <web, storage>
    scrape: {
        extract: ["title=h1", "price=.amount"]
        adaptive: true
        similarity_floor: 0.7
    }
}
"#;
    let prog = parse(src);
    let s = first_tool(&prog).scrape.as_ref().unwrap();
    assert_eq!(s.extract, vec!["title=h1", "price=.amount"]);
    assert_eq!(s.adaptive, Some(true));
    assert_eq!(s.similarity_floor, Some(0.7));
}

#[test]
fn parses_scrape_crawl_bounds() {
    let src = r#"
tool Crawl {
    provider: scrape_crawl
    parameters: { seed: String }
    output_type: RawPage
    effects: <network, web, stream:drop_oldest>
    scrape: {
        follow: "a.article"
        max_depth: 3
        max_pages: 100
        concurrency: 4
        engine: impersonate
    }
}
"#;
    let prog = parse(src);
    let s = first_tool(&prog).scrape.as_ref().unwrap();
    assert_eq!(s.follow, "a.article");
    assert_eq!(s.max_depth, Some(3));
    assert_eq!(s.max_pages, Some(100));
    assert_eq!(s.concurrency, Some(4));
}

// ── 2. IR lowering + IR-SHA invariance ───────────────────────────────────────

#[test]
fn scrape_lowers_to_ir() {
    let json = ir_json(HTTP_TOOL);
    assert!(json.contains("\"scrape\""), "scrape block present in IR");
    assert!(json.contains("scrape_spec"));
    assert!(json.contains("impersonate"));
}

#[test]
fn non_scrape_tool_elides_scrape_key() {
    // IR-SHA invariance: a plain tool serialises with no `scrape` key.
    let src = r#"
tool Plain {
    provider: http
    parameters: { q: String }
    effects: <network>
    runtime: "https://api.example.com"
}
"#;
    let json = ir_json(src);
    assert!(!json.contains("\"scrape\""), "no scrape key for a non-scrape tool");
}

// ── 3. `web` effect base accepted ────────────────────────────────────────────

#[test]
fn web_effect_base_is_valid() {
    // A scrape tool declaring `web` produces no "unknown effect" diagnostic.
    let errs = check_errors(HTTP_TOOL);
    assert!(
        !errs.iter().any(|e| e.contains("Unknown effect")),
        "web must be a known effect base: {errs:?}"
    );
}

// ── 4. axon-T904 — effect honesty ────────────────────────────────────────────

#[test]
fn t904_scrape_http_must_declare_web() {
    let src = r#"
tool Bad {
    provider: scrape_http
    parameters: { url: String }
    effects: <network>
    scrape: { engine: impersonate }
}
"#;
    let errs = check_errors(src);
    assert!(
        errs.iter().any(|e| e.contains("axon-T904") && e.contains("web")),
        "expected T904 (missing web): {errs:?}"
    );
}

#[test]
fn t904_scrape_dom_must_not_declare_network() {
    let src = r#"
tool Bad {
    provider: scrape_dom
    parameters: { page: RawPage }
    effects: <web, network>
    scrape: { extract: ["t=h1"] }
}
"#;
    let errs = check_errors(src);
    assert!(
        errs.iter().any(|e| e.contains("axon-T904") && e.contains("network")),
        "expected T904 (dom must not declare network): {errs:?}"
    );
}

#[test]
fn t904_adaptive_dom_needs_storage() {
    let src = r#"
tool Bad {
    provider: scrape_dom
    parameters: { page: RawPage }
    effects: <web>
    scrape: { extract: ["t=h1"] adaptive: true }
}
"#;
    let errs = check_errors(src);
    assert!(
        errs.iter().any(|e| e.contains("axon-T904") && e.contains("storage")),
        "expected T904 (adaptive needs storage): {errs:?}"
    );
}

// ── 5. axon-T905 — engine catalog + provider↔field applicability ─────────────

#[test]
fn t905_scrape_block_on_non_scrape_tool() {
    let src = r#"
tool Bad {
    provider: http
    effects: <network>
    runtime: "https://x"
    scrape: { engine: impersonate }
}
"#;
    let errs = check_errors(src);
    assert!(errs.iter().any(|e| e.contains("axon-T905")), "{errs:?}");
}

#[test]
fn t905_bad_engine() {
    let src = r#"
tool Bad {
    provider: scrape_http
    parameters: { url: String }
    effects: <network, web>
    scrape: { engine: teleport }
}
"#;
    let errs = check_errors(src);
    assert!(
        errs.iter().any(|e| e.contains("axon-T905") && e.contains("engine")),
        "{errs:?}"
    );
}

#[test]
fn t905_crawl_field_on_http_tool() {
    let src = r#"
tool Bad {
    provider: scrape_http
    parameters: { url: String }
    effects: <network, web>
    scrape: { engine: impersonate max_pages: 10 }
}
"#;
    let errs = check_errors(src);
    assert!(
        errs.iter().any(|e| e.contains("axon-T905") && e.contains("max_pages")),
        "{errs:?}"
    );
}

// ── 6/7. axon-T906 / T907 — extract + similarity_floor ───────────────────────

#[test]
fn t906_malformed_extract_spec() {
    let src = r#"
tool Bad {
    provider: scrape_dom
    parameters: { page: RawPage }
    effects: <web>
    scrape: { extract: ["justaname"] }
}
"#;
    let errs = check_errors(src);
    assert!(errs.iter().any(|e| e.contains("axon-T906")), "{errs:?}");
}

#[test]
fn t907_similarity_floor_out_of_range() {
    let src = r#"
tool Bad {
    provider: scrape_dom
    parameters: { page: RawPage }
    effects: <web, storage>
    scrape: { extract: ["t=h1"] adaptive: true similarity_floor: 1.7 }
}
"#;
    let errs = check_errors(src);
    assert!(errs.iter().any(|e| e.contains("axon-T907")), "{errs:?}");
}

// ── 8. axon-T908 — the CONTENT-INJECTION BARRIER (the flagship) ──────────────

const BARRIER_SCAFFOLD: &str = r#"
type RawPage { status: Int, body: String }
type Summary { text: String }
type Clean { body: String }

tool FetchNews {
    provider: scrape_http
    parameters: { url: String }
    output_type: RawPage
    effects: <network, web>
    scrape: { engine: impersonate }
}

persona Analyst { domain: ["news"] }

shield NewsShield { scan: [prompt_injection] on_breach: quarantine severity: high }
"#;

#[test]
fn t908_unshielded_scrape_to_agent_is_refused() {
    // A flow that scrapes (web-tainted) and reasons with a persona, no shield.
    let src = format!(
        "{BARRIER_SCAFFOLD}
flow Digest() -> Summary {{
    use FetchNews(url = \"https://ex.com/news\")
    step Summarize {{ given: Digest  ask: \"Summarize the fetched page\"  output: Summary }}
}}
"
    );
    let errs = check_errors(&src);
    assert!(
        errs.iter().any(|e| e.contains("axon-T908")),
        "expected the content-injection barrier to fire: {errs:?}"
    );
}

#[test]
fn t908_shielded_scrape_to_agent_passes() {
    // The SAME flow with a `shield` step before the reasoning step: no T908.
    let src = format!(
        "{BARRIER_SCAFFOLD}
flow Digest() -> Summary {{
    use FetchNews(url = \"https://ex.com/news\")
    shield NewsShield on page -> Clean
    step Summarize {{ given: Digest  ask: \"Summarize the fetched page\"  output: Summary }}
}}
"
    );
    let errs = check_errors(&src);
    assert!(
        !errs.iter().any(|e| e.contains("axon-T908")),
        "a shielded flow must pass the barrier: {errs:?}"
    );
}

#[test]
fn t908_scrape_without_reasoning_carries_no_obligation() {
    // A flow that scrapes but never reasons (no persona step) → no barrier.
    let src = format!(
        "{BARRIER_SCAFFOLD}
flow Fetch() -> RawPage {{
    use FetchNews(url = \"https://ex.com/news\")
}}
"
    );
    let errs = check_errors(&src);
    assert!(
        !errs.iter().any(|e| e.contains("axon-T908")),
        "no reasoning step ⇒ no obligation: {errs:?}"
    );
}

// ── 9. the design decision — unknown scrape field is a hard parse error ─────────────────────

#[test]
fn d98_2_unknown_scrape_field_is_parse_error() {
    let src = r#"
tool Bad {
    provider: scrape_http
    parameters: { url: String }
    effects: <network, web>
    scrape: { engine: impersonate stealth_mode: true }
}
"#;
    let err = try_parse(src).unwrap_err();
    assert!(
        err.message.contains("scrape") && err.message.contains("stealth_mode"),
        "expected a closed-catalog parse error: {}",
        err.message
    );
}
