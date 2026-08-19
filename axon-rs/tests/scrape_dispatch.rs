//! v2.52.0 — end-to-end runtime wiring for Native Web Acquisition: a scrape
//! program compiles, `register_from_ir` populates the `ToolEntry.scrape`
//! config, and `ToolRegistry::dispatch` routes a `scrape_dom` call through the
//! deterministic extractor — producing a born-Untrusted result.

use axon::ir_generator::IRGenerator;
use axon::lexer::Lexer;
use axon::parser::Parser;
use axon::scrape_tool::{self, RawPage};
use axon::tool_registry::ToolRegistry;

fn ir(src: &str) -> axon::ir_nodes::IRProgram {
    let tokens = Lexer::new(src, "<t>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    IRGenerator::new().generate(&prog)
}

const DOM_PROGRAM: &str = r#"
tool ExtractHeadline {
    provider: scrape_dom
    parameters: { page: RawPage }
    output_type: Json
    effects: <web>
    scrape: { extract: ["title=h1", "lead=p.lead"] }
}
"#;

#[test]
fn register_from_ir_populates_scrape_config() {
    let program = ir(DOM_PROGRAM);
    let mut reg = ToolRegistry::new();
    reg.register_from_ir(&program.tools);
    let entry = reg.get("ExtractHeadline").expect("tool registered");
    assert_eq!(entry.provider, "scrape_dom");
    let cfg = entry.scrape.as_ref().expect("scrape config resolved");
    assert_eq!(cfg.extract, vec!["title=h1", "lead=p.lead"]);
    // respect_robots defaults to TRUE (the design decision, default-secure).
    assert!(cfg.respect_robots);
}

#[test]
fn dispatch_routes_scrape_dom_and_extracts() {
    let program = ir(DOM_PROGRAM);
    let mut reg = ToolRegistry::new();
    reg.register_from_ir(&program.tools);

    let html = r#"<html><body><h1>Breaking</h1><p class="lead">Details here</p></body></html>"#;
    let page = RawPage {
        status: 200,
        final_url: "https://ex.com".into(),
        headers: Default::default(),
        body: html.into(),
        from_cache: false,
        truncated: false,
        engine: "test".into(),
    };
    let arg = serde_json::json!({ "page": serde_json::to_string(&page).unwrap() }).to_string();

    let result = reg.dispatch("ExtractHeadline", &arg).expect("dispatched locally");
    assert!(result.success, "output: {}", result.output);
    let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
    assert_eq!(v["title"], "Breaking");
    assert_eq!(v["lead"], "Details here");
}

#[test]
fn scrape_dom_output_is_born_untrusted() {
    let program = ir(DOM_PROGRAM);
    let mut reg = ToolRegistry::new();
    reg.register_from_ir(&program.tools);
    let entry = reg.get("ExtractHeadline").unwrap();

    let arg = serde_json::json!({ "page": "<h1>X</h1>" }).to_string();
    let outcome = scrape_tool::dispatch_scrape_outcome(entry, &arg);
    // the design decision — the load-bearing property.
    assert_eq!(outcome.taint, axon::emcp::EpistemicTaint::Untrusted);
}
