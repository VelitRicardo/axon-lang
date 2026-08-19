//! v2.81.0 — `axon-T957` **RegulatedBoundaryCoverage**: the ESK
//! κ-coverage law, scoped at last to the `axonendpoint` —
//! `docs/cycle/the_public_artifact.md`, axon-enterprise repo.
//!
//! **What was wrong.** The README has promised since v1.x that removing the
//! `shield` from a regulated endpoint makes `axon check` refuse — *"That
//! failure is a **type error**, not a lint warning."* It was not. v2.67.0 F16
//! diagnosed the gap and `advertised.rs` carried the confession: exactly one
//! genuine κ-coverage rule existed, scoped to `component`, while the endpoint
//! rule (`axon-T890`) was a PRESENCE check — `!compliance.is_empty()`. The
//! promise could not be violated, and therefore could not be kept: vacuous,
//! not weak. The same shape `lease` had before v2.67.0 gave it a subject.
//!
//! Measured on 2026-08-03 against the published 2.80.0 binary: the README's
//! own 30-second demo emitted the SAME diagnostics with and without its
//! `shield` line. This suite is the regression wall for that.
//!
//! Pinned properties:
//! 1. A regulated endpoint with a covering shield produces **no T957**.
//! 2. A regulated endpoint with **no** `shield:` is REJECTED, and the message
//!    names the uncovered κ.
//! 3. A regulated endpoint whose shield covers only PART of the κ is
//!    REJECTED, and the message names exactly the MISSING classes.
//! 4. The endpoint's own `compliance:` list does **not** satisfy the law —
//! a label is not a control. This is the v2.67.0 presence check staying dead.
//! 5. κ survives wrapping: `FlowEnvelope<T>`, `List<T>`, `Stream<T>` and
//!    nestings thereof do not launder a type's regulatory classes.
//! 6. κ is read from **both** sides of the boundary — `body:` (what arrives)
//!    as well as `output:` (what leaves).
//! 7. An UNREGULATED endpoint is untouched (κ = ∅ ⇒ the law is silent), and
//!    an endpoint that dispatches nothing crosses no boundary.
//! 8. The law is NOT suppressed by the v2.0.0 `axon-E039` wire gate — an
//!    unguarded regulated boundary is a governance defect independent of
//!    wire shape.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn check_errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

fn t957(src: &str) -> Vec<String> {
    check_errors(src)
        .into_iter()
        .filter(|m| m.contains("axon-T957"))
        .collect()
}

/// The canonical covered form: shield's `compliance:` ⊇ the type's κ.
const COVERED: &str = r#"
    type PatientRecord compliance [HIPAA, GDPR] { ssn: String }
    shield PHIShield {
        scan: [pii_leak] on_breach: halt severity: critical
        compliance: [HIPAA, GDPR]
    }
    axonendpoint Api {
        method: POST path: "/p" body: PatientRecord
        execute: F output: FlowEnvelope<PatientRecord> shield: PHIShield
        compliance: [HIPAA, GDPR]
    }
    flow F(r: PatientRecord) -> PatientRecord {
        step R { ask: "summarize" output: PatientRecord }
    }
"#;

#[test]
fn covered_regulated_endpoint_is_clean() {
    // Property 1 — the law is satisfiable. If this ever fails the rule has
    // become a blanket refusal, which is as useless as no rule at all.
    assert!(
        t957(COVERED).is_empty(),
        "a shield covering the full kappa must produce no T957; got: {:?}",
        t957(COVERED)
    );
}

#[test]
fn missing_shield_is_a_type_error() {
    // Property 2 — THE README's claim, made true. This is the exact edit the
    // README instructs the reader to perform ("Remove the `shield` line").
    let src = COVERED
        .replace("shield: PHIShield", "")
        .replace(
            "shield PHIShield {\n        scan: [pii_leak] on_breach: halt severity: critical\n        compliance: [HIPAA, GDPR]\n    }",
            "",
        );
    let errs = t957(&src);
    assert_eq!(
        errs.len(),
        1,
        "removing the shield must raise exactly one T957; got: {errs:?}"
    );
    let m = &errs[0];
    assert!(m.contains("declares no `shield:`"), "message: {m}");
    assert!(m.contains("GDPR") && m.contains("HIPAA"), "must name the uncovered kappa: {m}");
    assert!(m.contains("ESK coverage rule"), "must cite the rule it enforces: {m}");
}

#[test]
fn partial_shield_names_exactly_the_missing_classes() {
    // Property 3 — a real set difference, not a presence check. The shield
    // covers HIPAA but not GDPR; only GDPR may be reported.
    let src = COVERED.replace(
        "compliance: [HIPAA, GDPR]\n    }\n    axonendpoint",
        "compliance: [HIPAA]\n    }\n    axonendpoint",
    );
    let errs = t957(&src);
    assert_eq!(errs.len(), 1, "expected one T957; got: {errs:?}");
    let m = &errs[0];
    assert!(m.contains("GDPR"), "must name the missing class: {m}");
    assert!(
        !m.contains("{HIPAA, GDPR}") && !m.contains("kappa = {HIPAA}"),
        "must report only the DIFFERENCE, not the whole kappa: {m}"
    );
}

#[test]
fn endpoint_compliance_label_does_not_cover() {
    // Property 4 — the v2.67.0 F16 presence check stays dead. The endpoint
    // declares `compliance: [HIPAA, GDPR]` (which satisfies T890) but names
    // no shield: the boundary is labelled, not controlled.
    let src = r#"
        type PatientRecord compliance [HIPAA, GDPR] { ssn: String }
        axonendpoint Api {
            method: POST path: "/p" body: PatientRecord
            execute: F output: FlowEnvelope<PatientRecord>
            compliance: [HIPAA, GDPR]
        }
        flow F(r: PatientRecord) -> PatientRecord {
            step R { ask: "summarize" output: PatientRecord }
        }
    "#;
    let errs = t957(src);
    assert_eq!(
        errs.len(),
        1,
        "an endpoint `compliance:` label must NOT satisfy the coverage law; got: {errs:?}"
    );
    assert!(
        errs[0].contains("label") && errs[0].contains("control"),
        "the diagnostic must explain why the label is not coverage: {}",
        errs[0]
    );
}

#[test]
fn kappa_survives_envelope_list_and_stream_wrapping() {
    // Property 5 — wrapping is not laundering. Each wrapper, and a nesting,
    // must still be caught with no shield present.
    for wrapper in [
        "FlowEnvelope<PatientRecord>",
        "List<PatientRecord>",
        "Stream<PatientRecord>",
        "FlowEnvelope<List<PatientRecord>>",
    ] {
        let src = format!(
            r#"
            type PatientRecord compliance [HIPAA] {{ ssn: String }}
            axonendpoint Api {{
                method: POST path: "/p" body: PatientRecord
                execute: F output: {wrapper}
                compliance: [HIPAA]
            }}
            flow F(r: PatientRecord) -> PatientRecord {{
                step R {{ ask: "summarize" output: PatientRecord }}
            }}
        "#
        );
        assert_eq!(
            t957(&src).len(),
            1,
            "kappa must survive `{wrapper}`; got: {:?}",
            t957(&src)
        );
    }
}

#[test]
fn body_side_of_the_boundary_counts() {
    // Property 6 — regulated data ARRIVING is still regulated data crossing
    // the boundary. Here `output:` is an unregulated type; only `body:` is κ.
    let src = r#"
        type PatientRecord compliance [HIPAA] { ssn: String }
        type Ack { ok: Bool }
        axonendpoint Api {
            method: POST path: "/p" body: PatientRecord
            execute: F output: FlowEnvelope<Ack>
            compliance: [HIPAA]
        }
        flow F(r: PatientRecord) -> Ack {
            step R { ask: "summarize" output: Ack }
        }
    "#;
    let errs = t957(src);
    assert_eq!(errs.len(), 1, "the body side must count; got: {errs:?}");
    assert!(errs[0].contains("HIPAA"), "message: {}", errs[0]);
}

#[test]
fn unregulated_endpoint_is_untouched() {
    // Property 7 — κ = ∅ ⇒ silence. The law must not tax every endpoint in
    // the language; `public: true` keeps T890 quiet so only T957 is in play.
    let src = r#"
        type Ack { ok: Bool }
        axonendpoint Api {
            method: POST path: "/p" body: Ack
            execute: F output: FlowEnvelope<Ack>
            public: true
        }
        flow F(r: Ack) -> Ack {
            step R { ask: "echo" output: Ack }
        }
    "#;
    assert!(
        t957(src).is_empty(),
        "an unregulated endpoint must raise no T957; got: {:?}",
        t957(src)
    );
}

#[test]
fn fires_independently_of_the_e039_wire_gate() {
    // Property 8 — the v2.0.0 gate suppresses the CARDINALITY gate, never
    // this one. A bare `output: T` on a json transport trips E039; the
    // unguarded regulated boundary must still be reported, because the two
    // are different defects. This is the precise shape that made the README
    // demo look "fixed" while the governance hole stayed open.
    let src = r#"
        type PatientRecord compliance [HIPAA, GDPR] { ssn: String }
        axonendpoint Api {
            method: POST path: "/p" body: PatientRecord
            execute: F output: PatientRecord
            compliance: [HIPAA, GDPR]
        }
        flow F(r: PatientRecord) -> PatientRecord {
            step R { ask: "summarize" output: PatientRecord }
        }
    "#;
    let all = check_errors(src);
    assert!(
        all.iter().any(|m| m.contains("axon-E039")),
        "precondition: E039 must fire on this program; got: {all:?}"
    );
    assert!(
        all.iter().any(|m| m.contains("axon-T957")),
        "T957 must NOT be suppressed by E039; got: {all:?}"
    );
}
