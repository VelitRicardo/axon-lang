//! v2.83.0 — `retrieve from <Store> where "…"` as a step-body statement.
//!
//! **What was wrong.** README axonstore writes the store read INSIDE the step
//! that consumes it, in a braceless form with `from` and with `where` taking
//! its argument directly — no colon:
//!
//! ```text
//! step Find {
//!     retrieve from Users where "email = '{email}'"
//!     output: UserRecord
//! }
//! ```
//!
//! Neither half parsed. The statement had no step-body position, and the
//! flow-level parser required `retrieve <Store> { where: "…" }` — so the only
//! published `retrieve` failed on its own first line.
//!
//! **Why this one is different from f.7–f.9.** Those three found the engine
//! missing or the wire cut. Here the engine is among the most-exercised paths
//! in the system (v1.30.0–v1.31.0 typed store, `wire_integrations::run_retrieve`, the
//! `*_pg_integration` suites). Only the POSITION was missing — which is what
//! the the design decision doctrine is for: one concept, two positions, one implementation.
//!
//! The elevation ordering matters here more than anywhere: a `retrieve` binds
//! rows the step's `ask:` then reads. Running it after the generation would
//! hand the model an empty binding, which is v2.67.0's defect.

use axon_frontend::ast::{Declaration, FlowStep};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn parse(src: &str) -> Result<axon_frontend::ast::Program, axon_frontend::parser::ParseError> {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    Parser::new(tokens).parse()
}

fn step_retrieve(program: &axon_frontend::ast::Program) -> &axon_frontend::ast::RetrieveStep {
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Step(st) = s {
                    for op in &st.pix_ops {
                        if let FlowStep::Retrieve(r) = op {
                            return r;
                        }
                    }
                }
            }
        }
    }
    panic!("no `retrieve` statement on any step body");
}

#[test]
fn f11_1_the_published_step_statement_parses() {
    let src = r#"
flow LookupUser(email: String) -> UserRecord {
    step Find {
        retrieve from Users where "email = '{email}'"
        output: UserRecord
    }
}
"#;
    let program = parse(src).expect("the README's own shape must parse");
    let r = step_retrieve(&program);
    assert_eq!(r.store_name, "Users");
    assert_eq!(
        r.where_expr, "email = '{email}'",
        "`where` takes its argument with NO colon in the published form"
    );
}

#[test]
fn f11_2_the_step_keeps_its_own_output() {
    let src = r#"
flow F(email: String) -> UserRecord {
    step Find {
        retrieve from Users where "email = '{email}'"
        output: UserRecord
    }
}
"#;
    let program = parse(src).expect("parse");
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Step(st) = s {
                    assert_eq!(
                        st.output_type, "UserRecord",
                        "the braceless clauses must not swallow the step's fields"
                    );
                    return;
                }
            }
        }
    }
    panic!("no step");
}

#[test]
fn f11_3_the_as_alias_clause_parses_braceless() {
    let src = r#"
flow F(email: String) -> R {
    step Find {
        retrieve from Users where "email = '{email}'" as record
        output: R
    }
}
"#;
    let program = parse(src).expect("parse");
    assert_eq!(step_retrieve(&program).alias, "record");
}

#[test]
fn f11_4_from_is_optional() {
    let src = r#"flow F(a: String) -> R { retrieve Users where "id = 1" }"#;
    let program = parse(src).expect("parse");
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Retrieve(r) = s {
                    assert_eq!(r.store_name, "Users");
                    assert_eq!(r.where_expr, "id = 1");
                    return;
                }
            }
        }
    }
    panic!("no Retrieve node");
}

#[test]
fn f11_5_the_braced_form_still_parses() {
    // Back-compat: the only form that parsed before v2.83.0.
    let src = r#"flow F(a: String) -> R { retrieve Users { where: "id = 1" as: rec limit: 5 } }"#;
    let program = parse(src).expect("parse");
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Retrieve(r) = s {
                    assert_eq!(r.store_name, "Users");
                    assert_eq!(r.where_expr, "id = 1");
                    assert_eq!(r.alias, "rec");
                    assert_eq!(r.limit_expr, "5");
                    return;
                }
            }
        }
    }
    panic!("no Retrieve node");
}
