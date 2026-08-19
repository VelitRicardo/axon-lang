//! v2.33.0 — the `retrieve` aggregate surface (frontend half).
//!
//! `aggregate:` (the CLOSED catalog `count` / `sum(col)` / `avg(col)` /
//! `min(col)` / `max(col)`) + `group_by:` parse into the AST, lower into
//! the IR (elided when empty — zero IR-SHA drift for existing programs),
//! and are PROVEN at `axon check`: grammar (axon-T843), schema-backed
//! column + numeric-family facts (axon-T844), structural combinations
//! (axon-T845). The runtime mirror (`filter::parse_aggregate_clause` +
//! the structural SQL renderers) is pinned in `axon-rs`
//! (`store::filter` tests); the engines share ONE SQL path
//! (`row_stream::stream_retrieve`).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn compile(src: &str) -> (axon_frontend::ast::Program, Vec<String>) {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    let errs = TypeChecker::new(&prog)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect();
    (prog, errs)
}

/// A store with a numeric + a text column, and one flow whose retrieve
/// carries the given aggregate surface.
fn src_with(aggregate: &str, group_by: &str, bounds: &str) -> String {
    format!(
        r#"
        axonstore Tenants {{
            backend: postgresql
            connection: "env:DB"
            schema {{
                id:       Uuid primary_key
                tokens:   Int
                industry: Text
            }}
        }}

        flow PlatformStats() -> Unit {{
            retrieve Tenants {{
                where: "tokens > 0"
                {aggregate}
                {group_by}
                {bounds}
                as: stats
            }}
        }}
    "#
    )
}

#[test]
fn aggregate_count_group_by_parses_and_type_checks_clean() {
    let (prog, errs) = compile(&src_with(
        r#"aggregate: "count""#,
        r#"group_by: "industry""#,
        "",
    ));
    assert!(
        !errs.iter().any(|m| m.contains("axon-T84")),
        "a well-formed aggregate must be clean: {errs:?}"
    );
    // The AST carries the raw clauses.
    let flow = prog
        .declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Flow(f) if f.name == "PlatformStats" => Some(f),
            _ => None,
        })
        .expect("PlatformStats flow");
    let retrieve = flow
        .body
        .iter()
        .find_map(|s| match s {
            axon_frontend::ast::FlowStep::Retrieve(r) => Some(r),
            _ => None,
        })
        .expect("retrieve step");
    assert_eq!(retrieve.aggregate, "count");
    assert_eq!(retrieve.group_by, "industry");
    // …and the IR carries them too.
    let ir = IRGenerator::new().generate(&prog);
    let json = serde_json::to_string(&ir).expect("IR serializes");
    assert!(json.contains("\"aggregate\":\"count\""));
    assert!(json.contains("\"group_by\":\"industry\""));
}

#[test]
fn sum_over_numeric_column_is_clean_but_text_is_t844() {
    let (_, errs) = compile(&src_with(r#"aggregate: "sum(tokens)""#, "", ""));
    assert!(
        !errs.iter().any(|m| m.contains("axon-T84")),
        "sum over Int must be clean: {errs:?}"
    );
    let (_, errs) = compile(&src_with(r#"aggregate: "sum(industry)""#, "", ""));
    assert!(
        errs.iter().any(|m| m.contains("axon-T844")),
        "sum over Text must surface axon-T844: {errs:?}"
    );
}

#[test]
fn unknown_function_and_unknown_columns_are_proven() {
    let (_, errs) = compile(&src_with(r#"aggregate: "median(tokens)""#, "", ""));
    assert!(
        errs.iter().any(|m| m.contains("axon-T843")),
        "an off-catalog function must surface axon-T843: {errs:?}"
    );
    let (_, errs) = compile(&src_with(r#"aggregate: "sum(tokns)""#, "", ""));
    assert!(
        errs.iter().any(|m| m.contains("axon-T844") && m.contains("tokns")),
        "an unknown aggregate column must surface axon-T844: {errs:?}"
    );
    let (_, errs) = compile(&src_with(r#"aggregate: "count""#, r#"group_by: "industri""#, ""));
    assert!(
        errs.iter().any(|m| m.contains("axon-T844")),
        "an unknown group column must surface axon-T844: {errs:?}"
    );
}

#[test]
fn structural_combinations_are_t845() {
    // group_by without aggregate.
    let (_, errs) = compile(&src_with("", r#"group_by: "industry""#, ""));
    assert!(
        errs.iter().any(|m| m.contains("axon-T845")),
        "group_by without aggregate must surface axon-T845: {errs:?}"
    );
    // aggregate with limit (v1 closed scope).
    let (_, errs) = compile(&src_with(r#"aggregate: "count""#, "", "limit: 10"));
    assert!(
        errs.iter().any(|m| m.contains("axon-T845")),
        "aggregate+limit must surface axon-T845: {errs:?}"
    );
}

/// v2.33.0 — the COMPILE-GATED corpus example that backs the
/// `ADOPTER_PLATFORM.md` platform-admin snippet ([[feedback-published-
/// grammar-must-compile]]): a platform-owner `axonendpoint` that DECLARES
/// the cross-tenant capability (`requires: [store.platform_read]`, v2.33.0)
/// and runs a global AGGREGATE (`count` grouped by a column, v2.33.0). If
/// this test ever fails, the published guide's snippet no longer compiles
/// — the doc and the grammar have drifted.
#[test]
fn adopter_platform_md_snippet_compiles_clean() {
    let src = r#"
        axonstore Tenants {
            backend: postgresql
            connection: "env:DB"
            schema {
                id:       Uuid primary_key
                status:   Text
                industry: Text
            }
        }

        flow TenantsByIndustry() -> Unit {
            retrieve Tenants {
                where: "status = 'active'"
                aggregate: "count"
                group_by: "industry"
                as: by_industry
            }
        }

        axonendpoint PlatformTenantStats {
            method: GET
            path: "/admin/analytics/by-industry"
            execute: TenantsByIndustry
            output_type: Text
            requires: [store.platform_read]
        }
    "#;
    let (_, errs) = compile(src);
    assert!(
        errs.iter().all(|m| !m.contains("axon-T") && !m.contains("axon-E")),
        "the ADOPTER_PLATFORM.md platform-admin snippet must compile clean: {errs:?}"
    );
}

/// D5 sagrado — a program with NO aggregate surface serializes an IR with
/// NO `aggregate`/`group_by` keys: byte-identical to the pre-v2.33.0 IR, so
/// every existing deployment's `ir_sha256` is untouched.
#[test]
fn plain_retrieve_ir_json_carries_no_aggregate_keys() {
    let (prog, errs) = compile(&src_with("", "", ""));
    assert!(
        !errs.iter().any(|m| m.contains("axon-T84")),
        "the plain retrieve must be clean: {errs:?}"
    );
    let ir = IRGenerator::new().generate(&prog);
    let json = serde_json::to_string(&ir).expect("IR serializes");
    assert!(
        !json.contains("\"aggregate\"") && !json.contains("\"group_by\""),
        "an aggregate-less retrieve must not perturb the IR bytes"
    );
}
