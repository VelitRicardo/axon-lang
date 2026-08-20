//! v2.63.0 — the typed `dataspace` declaration: grammar + AST + the
//! schema law (`axon-T928`) + IR emission (un-skipped `dataspace_specs`).
//! See `the design plan` (axon-enterprise).
//!
//! Pinned properties:
//! 1. `dataspace X { column a: Int … }` parses into typed `DataspaceColumn`s;
//!    aliases (`int`, `text`, …) are accepted and canonicalized at IR time.
//! 2. **axon-T928** — the schema law: empty schema / duplicate column /
//!    unknown type are refused; all violations accumulate in one compile.
//! 3. The body grammar is CLOSED: a non-`column` entry is a parse error
//! (the section 1 disease — a silently-discarded body — cannot recur).
//! 4. `dataspace_specs` is SERIALIZED into the IR JSON with canonical
//! type names — the v2.63.0 ground-truth fix (the runtime can now see a
//!    declared dataspace).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn check_errors(src: &str) -> Vec<String> {
    let prog = parse(src);
    TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

fn ir_json(src: &str) -> String {
    let prog = parse(src);
    let ir = IRGenerator::new().generate(&prog);
    serde_json::to_string(&ir).expect("serialize IR")
}

const GOOD: &str = r#"
dataspace Leads {
    column email:     Text
    column score:     Float
    column visits:    Int
    column active:    Bool
    column first_at:  Timestamp
    column raw:       Json
}
"#;

// ── 1. Grammar + AST ─────────────────────────────────────────────────────────

#[test]
fn parses_typed_columns_into_ast() {
    let prog = parse(GOOD);
    let ds = prog
        .declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Dataspace(n) => Some(n),
            _ => None,
        })
        .expect("dataspace declaration present");
    assert_eq!(ds.name, "Leads");
    assert_eq!(ds.columns.len(), 6);
    assert_eq!(ds.columns[0].name, "email");
    assert_eq!(ds.columns[0].declared_type, "Text");
    assert_eq!(ds.columns[4].declared_type, "Timestamp");
}

#[test]
fn lowercase_aliases_parse_and_check_clean() {
    // The v1.31.0 from_token convention: common aliases resolve.
    let src = r#"
dataspace Metrics {
    column name:  text
    column n:     integer
    column ratio: double
    column ok:    boolean
}
"#;
    let errs = check_errors(src);
    assert!(
        !errs.iter().any(|e| e.contains("axon-T928")),
        "aliases must resolve against the closed catalog: {errs:?}"
    );
}

#[test]
fn non_column_body_entry_is_a_parse_error() {
    // The grammar is CLOSED — the pre-108.b behaviour (any body silently
    // discarded by skip_braced_block) must be impossible to reintroduce.
    let src = r#"
dataspace X {
    retention: 7
}
"#;
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let err = Parser::new(tokens).parse().expect_err("must refuse");
    assert!(
        err.message.contains("column"),
        "the error must teach the grammar: {}",
        err.message
    );
}

// ── 2. axon-T928 — the schema law ────────────────────────────────────────────

#[test]
fn t928_refuses_an_empty_schema() {
    for src in ["dataspace Empty { }", "dataspace Bare"] {
        let errs = check_errors(src);
        assert!(
            errs.iter()
                .any(|e| e.contains("axon-T928") && e.contains("no columns")),
            "a dataspace IS its schema — `{src}` must refuse: {errs:?}"
        );
    }
}

#[test]
fn t928_refuses_a_duplicate_column() {
    let src = r#"
dataspace Dup {
    column email: Text
    column email: Int
}
"#;
    let errs = check_errors(src);
    assert!(
        errs.iter()
            .any(|e| e.contains("axon-T928") && e.contains("more than once")),
        "one name, one buffer: {errs:?}"
    );
}

#[test]
fn t928_refuses_an_unknown_type_with_a_suggestion() {
    let src = r#"
dataspace Bad {
    column score: Flaot
}
"#;
    let errs = check_errors(src);
    let t928 = errs
        .iter()
        .find(|e| e.contains("axon-T928") && e.contains("unknown type"))
        .expect("unknown type must refuse");
    assert!(
        t928.contains("Float"),
        "the closed catalog (and ideally the smart-suggest hint) must name Float: {t928}"
    );
}

#[test]
fn t928_violations_accumulate_in_one_compile() {
    // Duplicate AND unknown type in one declaration: both surface.
    let src = r#"
dataspace Multi {
    column a: Text
    column a: Text
    column b: Decimal
}
"#;
    let errs: Vec<String> = check_errors(src)
        .into_iter()
        .filter(|e| e.contains("axon-T928"))
        .collect();
    assert!(
        errs.len() >= 2,
        "all schema errors accumulate (parser keeps types raw): {errs:?}"
    );
}

// ── 3. IR emission — the un-skip ─────────────────────────────────────────────

#[test]
fn ir_json_carries_dataspace_specs_with_canonical_types() {
    let json = ir_json(GOOD);
    assert!(
        json.contains("\"dataspace_specs\""),
        "dataspace_specs must be SERIALIZED (the v2.63.0 ground-truth fix)"
    );
    assert!(json.contains("\"Leads\""));
    assert!(
        json.contains("\"column_type\":\"Timestamp\""),
        "canonical type names in the IR: {json}"
    );
}

// ── 4. axon-T929 — the ingest law (v2.63.0) ───────────────────────────────────

const DS: &str = "dataspace Leads { column email: Text\n column score: Float }\n";

#[test]
fn t929_governed_ingest_checks_clean() {
    let src = format!(
        "{DS}flow Load(raw_leads: Text) -> Text {{\n    ingest raw_leads into Leads {{ format: csv, limits {{ max_bytes: 1048576, max_rows: 10000 }} }}\n}}\n"
    );
    let errs = check_errors(&src);
    assert!(
        !errs.iter().any(|e| e.contains("axon-T929")),
        "the governed form must check clean: {errs:?}"
    );
}

#[test]
fn t929_refuses_an_ingest_without_a_dataspace() {
    let src = "flow Load(raw: Text) -> Text {\n    ingest raw\n}\n";
    let errs = check_errors(src);
    assert!(
        errs.iter()
            .any(|e| e.contains("axon-T929") && e.contains("names no dataspace")),
        "{errs:?}"
    );
}

#[test]
fn t929_refuses_an_undeclared_or_wrong_kind_target() {
    let src = "flow Load(raw: Text) -> Text {\n    ingest raw into Ghost { format: csv }\n}\n";
    let errs = check_errors(src);
    assert!(
        errs.iter()
            .any(|e| e.contains("axon-T929") && e.contains("not declared")),
        "{errs:?}"
    );
}

#[test]
fn t929_refuses_a_missing_or_unknown_format() {
    let src = format!("{DS}flow Load(raw: Text) -> Text {{\n    ingest raw into Leads\n}}\n");
    let errs = check_errors(&src);
    assert!(
        errs.iter()
            .any(|e| e.contains("axon-T929") && e.contains("no `format:`")),
        "{errs:?}"
    );
    let src = format!(
        "{DS}flow Load(raw: Text) -> Text {{\n    ingest raw into Leads {{ format: parquet }}\n}}\n"
    );
    let errs = check_errors(&src);
    assert!(
        errs.iter()
            .any(|e| e.contains("axon-T929") && e.contains("parquet")),
        "{errs:?}"
    );
}

#[test]
fn t929_refuses_a_zero_limit() {
    let src = format!(
        "{DS}flow Load(raw: Text) -> Text {{\n    ingest raw into Leads {{ format: csv, limits {{ max_bytes: 0 }} }}\n}}\n"
    );
    let errs = check_errors(&src);
    assert!(
        errs.iter()
            .any(|e| e.contains("axon-T929") && e.contains("zero limit")),
        "{errs:?}"
    );
}

// ── 5. axon-T927 × the design decision — an ingest is a declared write ────────────────────

#[test]
fn t927_refuses_a_query_endpoint_whose_flow_ingests() {
    // the design decision — an ingest appends to server-resident state: a safe method
    // cannot reach it. A QUERY may focus/aggregate; it may NOT ingest.
    let src = format!(
        "{DS}flow Load(raw: Text) -> Text {{\n    ingest raw into Leads {{ format: csv }}\n}}\n\
         axonendpoint LoadLeads {{\n    method: QUERY\n    path: \"/leads/load\"\n    execute: Load\n    backend: stub\n    requires: [flow.execute]\n}}\n"
    );
    let errs = check_errors(&src);
    assert!(
        errs.iter()
            .any(|e| e.contains("axon-T927") && e.contains("ingest")),
        "an ingest behind a safe method must refuse: {errs:?}"
    );
}

#[test]
fn ir_ingest_carries_format_and_limits() {
    let src = format!(
        "{DS}flow Load(raw: Text) -> Text {{\n    ingest raw into Leads {{ format: json, limits {{ max_bytes: 2048, max_rows: 10 }} }}\n}}\n"
    );
    let json = ir_json(&src);
    assert!(json.contains("\"format\":\"json\""), "{json}");
    assert!(json.contains("\"max_bytes\":2048"), "{json}");
    assert!(json.contains("\"max_rows\":10"), "{json}");
}

// ── 6. axon-T930 — the query-verb law (v2.63.0) ───────────────────────────────

#[test]
fn t930_governed_query_verbs_check_clean() {
    let src = format!(
        "{DS}dataspace Accounts {{ column email: Text\n column plan: Text }}\n\
         flow Q() -> Text {{\n\
             focus Leads {{ where: \"score >= 0.5\", select: [email], as: hot }}\n\
             aggregate Leads {{ group_by: [email], compute: [count, avg(score)], as: by_email }}\n\
             associate Leads Accounts using email\n\
             explore Leads {{ as: shape }}\n\
         }}\n"
    );
    let errs = check_errors(&src);
    assert!(
        !errs.iter().any(|e| e.contains("axon-T930")),
        "the governed forms must check clean: {errs:?}"
    );
}

#[test]
fn t930_refuses_a_query_verb_on_a_non_dataspace() {
    let src = "flow Q() -> Text {\n    focus Nothing\n    explore Ghost\n}\n";
    let errs: Vec<String> = check_errors(src)
        .into_iter()
        .filter(|e| e.contains("axon-T930"))
        .collect();
    assert!(errs.len() >= 2, "both verbs refused: {errs:?}");
    assert!(errs[0].contains("not declared"), "{errs:?}");
}

#[test]
fn t930_refuses_ghost_columns_and_unknown_aggregates() {
    let src = format!(
        "{DS}flow Q() -> Text {{\n\
             focus Leads {{ select: [ghost] }}\n\
             aggregate Leads {{ group_by: [nope], compute: [median(score)] }}\n\
         }}\n"
    );
    let errs: Vec<String> = check_errors(&src)
        .into_iter()
        .filter(|e| e.contains("axon-T930"))
        .collect();
    assert!(errs.iter().any(|e| e.contains("`ghost`")), "{errs:?}");
    assert!(errs.iter().any(|e| e.contains("`nope`")), "{errs:?}");
    assert!(errs.iter().any(|e| e.contains("median")), "{errs:?}");
}

#[test]
fn t930_refuses_sum_over_text_and_key_type_mismatch() {
    let src = format!(
        "{DS}dataspace Other {{ column email: Int }}\n\
         flow Q() -> Text {{\n\
             aggregate Leads {{ compute: [sum(email)] }}\n\
             associate Leads Other using email\n\
         }}\n"
    );
    let errs: Vec<String> = check_errors(&src)
        .into_iter()
        .filter(|e| e.contains("axon-T930"))
        .collect();
    assert!(
        errs.iter().any(|e| e.contains("Int/Float")),
        "sum(Text) refused: {errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("refusal, not coercion")),
        "Text↔Int join key refused: {errs:?}"
    );
}

#[test]
fn ir_focus_and_aggregate_carry_the_query_surface() {
    let src = format!(
        "{DS}flow Q() -> Text {{\n\
             focus Leads {{ where: \"score >= 0.5\", select: [email], as: hot }}\n\
             aggregate Leads {{ group_by: [email], compute: [count], where: \"score > 0\", as: agg }}\n\
         }}\n"
    );
    let json = ir_json(&src);
    assert!(json.contains("\"where_expr\":\"score >= 0.5\""), "{json}");
    assert!(json.contains("\"select\":[\"email\"]"), "{json}");
    assert!(json.contains("\"output\":\"hot\""), "{json}");
    assert!(json.contains("\"compute\":[\"count\"]"), "{json}");
}

#[test]
fn ir_canonicalizes_aliases() {
    let src = r#"
dataspace M {
    column n: integer
}
"#;
    let json = ir_json(src);
    assert!(
        json.contains("\"column_type\":\"Int\""),
        "alias `integer` canonicalizes to `Int` at IR generation: {json}"
    );
}
