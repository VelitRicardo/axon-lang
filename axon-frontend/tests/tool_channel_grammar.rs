//! v2.69.0 — **the third island: a `tool`'s channel is named, not pinned.**
//!
//! v2.67.0's census found `resource`/`axonstore` was one island and `tool.runtime`
//! a **third** — the one that ships every day. An absolute
//! `runtime: "https://api.vendor.com"` is used *verbatim* as the endpoint
//! (`tool_registry.rs`), opening a real production connection with **no lifetime,
//! no capacity, no shield, no certainty_floor** — and `check_tool` never looked at
//! the field.
//!
//! A production URL, in source, on the one primitive whose neighbours the language
//! already forbids URLs on: `axon-T850` (`upstream.resolve`), `axon-T902`
//! (`tool.secret`).
//!
//! v2.69.0 gives the channel a name. `tool { resource: R }` — the address,
//! concurrency and lifecycle come from the resource; `runtime:` names the **path
//! within** the channel. And the absolute URL is refused.

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

/// The governed shape: the tool names a resource whose endpoint is a config key.
const GOVERNED: &str = r#"
resource SearchApi { kind: https  endpoint: vendor.search.base  capacity: 8  lifetime: persistent }
tool Search { provider: http  resource: SearchApi  runtime: search }
"#;

#[test]
fn a_tool_on_a_resource_compiles() {
    accepts(GOVERNED);
}

// ── axon-T949 — the absolute runtime URL is refused ──────────────────────────

/// **The third island, closed.** A production URL in a `tool` used to compile
/// clean and open an ungoverned connection. Now it is refused where the adopter
/// is still holding the code.
#[test]
fn t949_an_absolute_runtime_url_is_refused_it_was_the_third_island() {
    refutes(
        r#"tool Search { provider: http  runtime: "https://api.vendor.com" }"#,
        "axon-T949",
    );
    refutes(
        r#"tool Search { provider: http  runtime: "http://10.0.0.1/search" }"#,
        "axon-T949",
    );
}

/// **The legacy slug form still works.** `runtime: search` is joined onto a
/// per-tenant base URL at dispatch — already *config, not code*. It is ungoverned
/// (no resource), so it is not eligible for the channel `shield`/`lease`, but it
/// compiles: the migration is soft, exactly as v2.67.0's was for `axonstore`.
#[test]
fn a_slug_runtime_still_compiles_the_migration_is_soft() {
    accepts(r#"tool Search { provider: http  runtime: search }"#);
}

/// And a tool with no runtime at all is unchanged (LLM-routed or provider-only).
#[test]
fn a_tool_with_no_runtime_is_unchanged() {
    accepts(r#"tool Summarize { max_results: 1 }"#);
}

// ── axon-T950 — the resource reference resolves ──────────────────────────────

#[test]
fn t950_a_tool_on_a_phantom_resource_is_refused() {
    refutes(
        r#"tool Search { provider: http  resource: NoSuchApi }"#,
        "axon-T950",
    );
}

#[test]
fn t950_a_tool_whose_resource_is_not_a_resource_is_refused() {
    refutes(
        r#"
shield G { scan: [prompt_injection]  on_breach: halt  severity: high }
tool Search { provider: http  resource: G }
"#,
        "axon-T950",
    );
}

/// The IR carries the reference, so the runtime can derive the channel from it
/// (the derivation itself is v2.69.0 — this only pins that the fact survives
/// lowering, skip-if-empty so a pre-v2.69.0 tool has zero IR-SHA drift).
#[test]
fn the_resource_reference_reaches_the_ir() {
    use axon_frontend::ir_generator::IRGenerator;
    let tokens = Lexer::new(GOVERNED, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    let ir = IRGenerator::new().generate(&prog);
    let tool = ir.tools.iter().find(|t| t.name == "Search").expect("tool in IR");
    assert_eq!(tool.resource_ref, "SearchApi");

    // A tool with no resource emits no `resource_ref` key (IR-SHA stability).
    let plain = "tool Plain { provider: http  runtime: search }";
    let t2 = Lexer::new(plain, "<t>").tokenize().unwrap();
    let p2 = Parser::new(t2).parse().unwrap();
    let ir2 = IRGenerator::new().generate(&p2);
    let json = serde_json::to_string(&ir2.tools[0]).unwrap();
    assert!(
        !json.contains("resource_ref"),
        "a resource-less tool must not emit the key: {json}"
    );
}
