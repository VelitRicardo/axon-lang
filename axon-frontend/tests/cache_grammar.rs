//! v2.40.0 — grammar + AST + IR + type-checker for the native `cache`
//! primitive (`cache <Name> { … }` + `tool.cache:` / `retrieve.cache:`).
//! See `docs/cycle/native_cache_primitive.md` (axon-enterprise repo).
//!
//! Pinned properties:
//! 1. A full `cache` parses into `CacheDefinition` (every field).
//! 2. It lowers to `IRCache`; absent optionals are ELIDED from the JSON.
//! 3. **IR-SHA invariance**: a program with no `cache` serialises with no
//!    `caches` key and no `cache` field on tool/retrieve — byte-identical to
//! pre-v2.40.0 IR.
//! 4. A well-formed cache program → zero diagnostics.
//! 5. **axon-T863** — more than one `cache { default: true }`.
//! 6. **axon-T864** — a `tool.cache:` / `retrieve.cache:` / `invalidate_on:`
//!    reference to an undeclared symbol.
//! 7. **axon-T865** — a non-pure cache (widened, or used by a non-pure
//!    tool/retrieve) with no finite `ttl:`.
//! 8. **axon-T866** — unknown `backend:`. **axon-T867** — unknown effect in
//!    `apply_to_effects:`.
//! 9. **axon-W013** — a widened `default: true` cache names each non-pure tool
//!    it auto-covers.
//! 10. **the design decision** — an unknown field in a `cache { }` block is a hard parse
//!     error. **`cache: none`** opts a pure tool out without erroring.

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

fn check_warnings(src: &str) -> Vec<String> {
    let prog = parse(src);
    let (_errs, warns) = TypeChecker::new(&prog).check_with_warnings();
    warns.iter().map(|w| w.message.clone()).collect()
}

fn first_cache(prog: &axon_frontend::ast::Program) -> &axon_frontend::ast::CacheDefinition {
    prog.declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Cache(c) => Some(c),
            _ => None,
        })
        .expect("no cache declaration")
}

fn ir_json(src: &str) -> String {
    let prog = parse(src);
    let ir = IRGenerator::new().generate(&prog);
    serde_json::to_string(&ir).expect("serialize IR")
}

const FLOW: &str = "flow Chat() -> Unit { step S { ask: \"hi\" } }\n";

/// A well-formed cache program: a pure tool auto-covered by a pure default
/// cache (no ttl needed — provably deterministic), plus a widened, ttl-bounded,
/// invalidation-wired cache referenced by a non-pure tool.
fn well_formed() -> String {
    format!(
        "{FLOW}\
         type WeatherEvent {{ city: String }}\n\
         channel WeatherUpdated {{ message: WeatherEvent }}\n\
         tool Fingerprint {{ provider: http effects: <pure> parameters: {{ input: String }} }}\n\
         tool Weather {{ provider: http effects: <network> parameters: {{ city: String }} cache: WeatherCache }}\n\
         cache DefaultPure {{ default: true }}\n\
         cache WeatherCache {{ backend: redis ttl: 5m apply_to_effects: [pure, network] invalidate_on: [WeatherUpdated] }}\n"
    )
}

#[test]
fn cache_parses_into_ast() {
    let src = format!(
        "{FLOW}cache C {{ backend: redis ttl: 30s key: [city] default: true apply_to_effects: [pure] }}\n"
    );
    let prog = parse(&src);
    let c = first_cache(&prog);
    assert_eq!(c.backend, "redis");
    assert_eq!(c.ttl.as_deref(), Some("30s"));
    assert_eq!(c.key_params, vec!["city"]);
    assert!(c.default_policy);
    assert_eq!(c.apply_to_effects, vec!["pure"]);
}

#[test]
fn well_formed_program_has_no_errors() {
    let errs = check_errors(&well_formed());
    assert!(errs.is_empty(), "expected zero errors, got: {errs:#?}");
}

#[test]
fn cache_lowers_into_ir() {
    let json = ir_json(&well_formed());
    assert!(json.contains("\"caches\""), "json: {json}");
    assert!(json.contains("\"name\":\"WeatherCache\""), "json: {json}");
    assert!(json.contains("\"ttl\":\"5m\""), "json: {json}");
    // The non-pure tool carries its cache reference.
    assert!(json.contains("\"cache\":\"WeatherCache\""), "json: {json}");
}

#[test]
fn ir_sha_invariance_no_cache_fields_elided() {
    // A program with no cache must serialise with NONE of the new keys.
    let src = format!("{FLOW}tool T {{ provider: http parameters: {{ x: String }} }}\n");
    let json = ir_json(&src);
    assert!(!json.contains("\"caches\""), "caches leaked: {json}");
    assert!(!json.contains("\"cache\""), "cache leaked: {json}");
}

#[test]
fn t863_multiple_defaults() {
    let src = format!(
        "{FLOW}cache A {{ default: true }}\ncache B {{ default: true }}\n"
    );
    assert!(
        check_errors(&src).iter().any(|m| m.contains("axon-T863")),
        "expected T863 for two default caches"
    );
}

#[test]
fn t864_undefined_references() {
    // tool.cache references a nonexistent cache.
    let tool_ref = format!(
        "{FLOW}tool T {{ provider: http effects: <pure> parameters: {{ x: String }} cache: Ghost }}\n"
    );
    assert!(
        check_errors(&tool_ref).iter().any(|m| m.contains("axon-T864")),
        "expected T864 for undefined tool cache"
    );
    // invalidate_on references a nonexistent channel.
    let inv_ref = format!(
        "{FLOW}cache C {{ ttl: 10s apply_to_effects: [network] invalidate_on: [NoSuchChannel] }}\n"
    );
    assert!(
        check_errors(&inv_ref).iter().any(|m| m.contains("axon-T864")),
        "expected T864 for undefined invalidate_on channel"
    );
}

#[test]
fn t865_non_pure_needs_ttl() {
    // (a) a widened cache DECLARATION without ttl.
    let decl = format!("{FLOW}cache C {{ apply_to_effects: [pure, network] }}\n");
    assert!(
        check_errors(&decl).iter().any(|m| m.contains("axon-T865")),
        "expected T865 for widened cache without ttl"
    );
    // (b) a non-pure tool memoised by a ttl-less cache.
    let usage = format!(
        "{FLOW}cache C {{ apply_to_effects: [pure] }}\n\
         tool T {{ provider: http effects: <network> parameters: {{ x: String }} cache: C }}\n"
    );
    assert!(
        check_errors(&usage).iter().any(|m| m.contains("axon-T865")),
        "expected T865 for non-pure tool via ttl-less cache"
    );
}

#[test]
fn pure_tool_may_use_ttl_less_cache() {
    // A provably-pure tool referencing a ttl-less cache is SOUND (no T865).
    let src = format!(
        "{FLOW}cache C {{ }}\n\
         tool T {{ provider: http effects: <pure> parameters: {{ x: String }} cache: C }}\n"
    );
    let errs = check_errors(&src);
    assert!(
        errs.iter().all(|m| !m.contains("axon-T865")),
        "pure tool + ttl-less cache must be sound, got: {errs:#?}"
    );
}

#[test]
fn t866_bad_backend() {
    let src = format!("{FLOW}cache C {{ backend: memcached }}\n");
    assert!(
        check_errors(&src).iter().any(|m| m.contains("axon-T866")),
        "expected T866 for unknown backend"
    );
}

#[test]
fn t867_bad_apply_to_effects() {
    let src = format!("{FLOW}cache C {{ ttl: 10s apply_to_effects: [pure, banana] }}\n");
    assert!(
        check_errors(&src).iter().any(|m| m.contains("axon-T867")),
        "expected T867 for unknown effect"
    );
}

#[test]
fn w013_widened_default_names_nonpure_tools() {
    let src = format!(
        "{FLOW}\
         cache Wide {{ default: true ttl: 30s apply_to_effects: [pure, network] }}\n\
         tool Fetch {{ provider: http effects: <network> parameters: {{ url: String }} }}\n"
    );
    let warns = check_warnings(&src);
    assert!(
        warns.iter().any(|m| m.contains("axon-W013") && m.contains("Fetch")),
        "expected W013 naming Fetch, got: {warns:#?}"
    );
}

#[test]
fn cache_none_opts_out_without_error() {
    // A pure tool with `cache: none` opts out of the default; no T864/T865.
    let src = format!(
        "{FLOW}cache DefaultPure {{ default: true }}\n\
         tool T {{ provider: http effects: <pure> parameters: {{ x: String }} cache: none }}\n"
    );
    let errs = check_errors(&src);
    assert!(
        errs.iter().all(|m| !m.contains("axon-T864") && !m.contains("axon-T865")),
        "cache: none must not error, got: {errs:#?}"
    );
}

#[test]
fn d83_7_unknown_cache_field_is_parse_error() {
    let src = format!("{FLOW}cache C {{ ttl: 10s bogus: 3 }}\n");
    let err = try_parse(&src).expect_err("expected hard parse error for unknown cache field");
    assert!(
        err.message.contains("unknown cache field") && err.message.contains("bogus"),
        "unexpected error: {}",
        err.message
    );
}
