//! v2.10.0 — kwarg value binding: the value_kind classification + the v2.10.0
//! type-checker validation of `"reference"` values (a flow-param or a
//! `Step.output`) against their source type.
//!
//! The runtime resolution (a reference → a binding lookup, like `let`) is
//! covered in `axon-rs` (`exec_context` unit tests + the dispatch tests); here
//! we pin the FRONTEND contract: references are classified and their type is
//! validated against the tool's declared parameter schema, with no false
//! positive for references the checker can't resolve in-scope (a `let`).

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

fn has(v: &[String], needle: &str) -> bool {
    v.iter().any(|e| e.contains(needle))
}

const TOOL: &str = r#"
tool Fetch {
    provider: http
    parameters: { url: String }
    output_type: Doc
}
type Doc { body: String }
"#;

#[test]
fn flow_param_reference_type_aligns_ok() {
    // `url = site` — a bare flow-param reference; site:String → url:String. OK.
    let src = format!(
        "{TOOL}
flow Scan(site: String) -> Doc {{ use Fetch(url = site) }}"
    );
    let e = errors(&src);
    assert!(!has(&e, "Type mismatch"), "param ref of matching type must not error: {e:?}");
    assert!(!has(&e, "has no parameter"), "{e:?}");
}

#[test]
fn flow_param_reference_type_mismatch_errors() {
    // `url = count` — count:Int → url:String. v2.10.0 caller-blame mismatch.
    let src = format!(
        "{TOOL}
flow Scan(count: Int) -> Doc {{ use Fetch(url = count) }}"
    );
    let e = errors(&src);
    assert!(
        has(&e, "Type mismatch for parameter 'url'") && has(&e, "from reference 'count'"),
        "expected a reference type-mismatch naming the source: {e:?}"
    );
}

#[test]
fn step_output_reference_aligns_ok() {
    // `url = ExtractUrl.output` — the prior step's String output → url:String. OK.
    let src = format!(
        "{TOOL}
flow Scan(site: String) -> Doc {{
    step ExtractUrl {{ ask: \"extract the url from ${{site}}\" output: String }}
    use Fetch(url = ExtractUrl.output)
}}"
    );
    let e = errors(&src);
    assert!(!has(&e, "Type mismatch"), "matching step-output ref must not error: {e:?}");
}

#[test]
fn step_output_reference_type_mismatch_errors() {
    // ExtractCount.output is Int → url:String. Mismatch, named to the step.
    let src = format!(
        "{TOOL}
flow Scan(site: String) -> Doc {{
    step ExtractCount {{ ask: \"count something\" output: Int }}
    use Fetch(url = ExtractCount.output)
}}"
    );
    let e = errors(&src);
    assert!(
        has(&e, "Type mismatch for parameter 'url'") && has(&e, "ExtractCount.output"),
        "expected a step-output type-mismatch: {e:?}"
    );
}

#[test]
fn bare_step_name_reference_resolves_like_dotted() {
    // `url = ExtractUrl` (bare step name, no `.output`) resolves to the same
    // source type — a mismatch still fires.
    let src = format!(
        "{TOOL}
flow Scan(site: String) -> Doc {{
    step ExtractCount {{ ask: \"count\" output: Int }}
    use Fetch(url = ExtractCount)
}}"
    );
    let e = errors(&src);
    assert!(has(&e, "Type mismatch for parameter 'url'"), "{e:?}");
}

#[test]
fn unresolvable_reference_is_skipped_no_false_positive() {
    // `url = x` where x is a `let` — the checker does not track let types, so it
    // conservatively skips (no false positive). Must NOT error.
    let src = format!(
        "{TOOL}
flow Scan(site: String) -> Doc {{
    let x = site
    use Fetch(url = x)
}}"
    );
    let e = errors(&src);
    assert!(!has(&e, "Type mismatch"), "an untracked let reference must not error: {e:?}");
    assert!(!has(&e, "has no parameter"), "{e:?}");
}

#[test]
fn literal_path_still_validates_ct2() {
    // v2.8.0 literal validation is untouched: a numeric literal into a String
    // param still errors; an unknown param still errors.
    let src = format!(
        "{TOOL}
flow Scan() -> Doc {{ use Fetch(url = 10) }}"
    );
    let e = errors(&src);
    assert!(has(&e, "Type mismatch for parameter 'url'"), "literal mismatch still errors: {e:?}");
}
