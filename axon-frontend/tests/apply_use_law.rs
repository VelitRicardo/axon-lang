//! v2.9.0 — the `use` / `apply:` law + the honest compiler.
//!
//! `apply: <Tool>` is COGNITIVE DELEGATION (the step runs as an LLM call;
//! the model decides whether to invoke the tool). `use <Tool>(k = v, …)`
//! is the one DETERMINISTIC, typed, real-dispatch surface. v2.9.0 makes the
//! compiler indicate this honestly:
//!
//! - `apply: <Tool>` on a tool that declares a `parameters:` schema emits
//!   `axon-W004` (a WARNING, not an error) redirecting to `use(k=v)` — it
//!   never fakes determinism.
//! - This SUPERSEDES the v2.8.0 splat type-check: the old hard errors
//!   (`missing required` / `type mismatch`) validated a deterministic
//!   contract `apply:` never runs (the "illusion of control") — they are
//!   gone. The real CT-2 validation lives on `use <Tool>(k = v, …)`
//! (v2.8.0, untouched), as the regression tests below confirm.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn check(src: &str) -> (Vec<String>, Vec<String>) {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    let (errors, warnings) = TypeChecker::new(&prog).check_with_warnings();
    (
        errors.into_iter().map(|e| e.message).collect(),
        warnings.into_iter().map(|w| w.message).collect(),
    )
}

fn has(v: &[String], needle: &str) -> bool {
    v.iter().any(|e| e.contains(needle))
}

// A schema-bearing tool (two required params + one optional) + its types.
const TOOL: &str = r#"
tool CrmRadar {
    provider: http
    parameters: { company: String, max_results: Int, active: Bool? }
    output_type: CrmReport
}
type CrmReport { summary: String }
type LeadRequest { company: String, max_results: Int }
"#;

#[test]
fn apply_schema_tool_emits_w004_guidance_not_error() {
    let src = format!(
        "{TOOL}
flow Scan(req: LeadRequest) -> CrmReport {{
    step Render {{ given: req apply: CrmRadar ask: \"go\" output: CrmReport }}
}}"
    );
    let (errors, warnings) = check(&src);
    assert!(
        errors.is_empty(),
        "v2.9.0 degrades the v2.8.0 phantom errors — `apply:` must NOT error: {errors:?}"
    );
    assert!(has(&warnings, "axon-W004"), "expected the W004 guidance: {warnings:?}");
    assert!(has(&warnings, "COGNITIVE"), "W004 must name the cognitive nature: {warnings:?}");
    assert!(
        has(&warnings, "use CrmRadar("),
        "W004 must redirect to the deterministic `use CrmRadar(...)` form: {warnings:?}"
    );
    assert!(
        has(&warnings, "axon://logic/dispatch_vs_cognition"),
        "W004 must point at the canonical doctrine: {warnings:?}"
    );
}

#[test]
fn w004_lists_the_schema_params_for_paste_actionable_conversion() {
    let src = format!(
        "{TOOL}
flow Scan(req: LeadRequest) -> CrmReport {{
    step Render {{ given: req apply: CrmRadar ask: \"go\" output: CrmReport }}
}}"
    );
    let (_e, warnings) = check(&src);
    // The call hint names every declared parameter.
    for p in ["company =", "max_results =", "active ="] {
        assert!(has(&warnings, p), "W004 call hint must list '{p}': {warnings:?}");
    }
}

#[test]
fn apply_schema_tool_warns_even_without_given() {
    // The warning is about the OPERATION (cognitive delegation of a
    // schema-bearing tool), independent of `given:`.
    let src = format!(
        "{TOOL}
flow Scan() -> CrmReport {{
    step Render {{ apply: CrmRadar ask: \"go\" output: CrmReport }}
}}"
    );
    let (errors, warnings) = check(&src);
    assert!(errors.is_empty(), "got: {errors:?}");
    assert!(has(&warnings, "axon-W004"), "got: {warnings:?}");
}

#[test]
fn schema_less_tool_apply_is_legitimate_no_warning() {
    // A tool with NO `parameters:` applied cognitively is a legitimate
    // step backend (D7) — no warning.
    let src = "
tool Plain { provider: http }
type W { a: String }
flow F(w: W) -> Any {
    step S { given: w apply: Plain ask: \"go\" output: Any }
}";
    let (errors, warnings) = check(src);
    assert!(errors.is_empty(), "got: {errors:?}");
    assert!(!has(&warnings, "axon-W004"), "schema-less apply must NOT warn: {warnings:?}");
}

#[test]
fn flow_apply_is_composition_no_warning() {
    // `apply: <Flow>` is composition, not a tool — no W004.
    let src = "
type In { x: String }
type Out { y: String }
flow Helper(i: In) -> Out { step S { ask: \"go\" output: Out } }
flow Main(i: In) -> Out {
    step Compose { given: i apply: Helper ask: \"go\" output: Out }
}";
    let (_e, warnings) = check(src);
    assert!(!has(&warnings, "axon-W004"), "a flow apply must NOT warn: {warnings:?}");
}

// ── v2.8.0 degradation: the former hard errors are now gone ──────────

#[test]
fn former_missing_required_no_longer_errors() {
    // v2.8.0 errored when the `given:` struct omitted a required param.
    // v2.9.0: `apply:` is cognitive — that contract never ran — so NO error
    // (only the W004 guidance).
    let src = format!(
        "{TOOL}
type Partial {{ company: String }}
flow Scan(req: Partial) -> CrmReport {{
    step Render {{ given: req apply: CrmRadar ask: \"go\" output: CrmReport }}
}}"
    );
    let (errors, warnings) = check(&src);
    assert!(
        !has(&errors, "does not supply required") && !has(&errors, "Missing required"),
        "the v2.8.0 phantom missing-required error must be GONE: {errors:?}"
    );
    assert!(has(&warnings, "axon-W004"), "got: {warnings:?}");
}

#[test]
fn former_type_mismatch_no_longer_errors() {
    let src = format!(
        "{TOOL}
type Bad {{ company: String, max_results: String }}
flow Scan(req: Bad) -> CrmReport {{
    step Render {{ given: req apply: CrmRadar ask: \"go\" output: CrmReport }}
}}"
    );
    let (errors, _w) = check(&src);
    assert!(
        !has(&errors, "type mismatch") && !has(&errors, "expects"),
        "the v2.8.0 phantom type-mismatch error must be GONE: {errors:?}"
    );
}

// ── the deterministic surface keeps its hard CT-2 validation (v2.8.0) ──

#[test]
fn use_keyword_form_still_hard_errors_ct2() {
    // `use <Tool>(k=v)` is the deterministic surface — it STILL errors on
    // an unknown arg + missing required (v2.8.0 intact). This is where real
    // caller-blame lives.
    let src = format!(
        "{TOOL}
flow F(q: String) -> Any {{ use CrmRadar(bogus = 1) }}"
    );
    let (errors, _w) = check(&src);
    assert!(has(&errors, "has no parameter 'bogus'"), "got: {errors:?}");
    assert!(
        has(&errors, "Missing required argument 'company'"),
        "the deterministic surface must still demand required params: {errors:?}"
    );
}
