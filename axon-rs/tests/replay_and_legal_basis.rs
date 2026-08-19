//! Integration tests for v1.4.0 — Deterministic Replay +
//! Legal-Basis Typed Effects.

use axon::lexer::Lexer;
use axon::parser::Parser;
use axon::type_checker::{TypeChecker, TypeError};

fn type_check(src: &str) -> Vec<TypeError> {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex ok");
    let program = Parser::new(tokens).parse().expect("parse ok");
    TypeChecker::new(&program).check()
}

fn any_error_mentions(errs: &[TypeError], needle: &str) -> bool {
    errs.iter().any(|e| e.message.contains(needle))
}

// ── Tool-level legal-basis qualifier checks ─────────────────────────

#[test]
fn legal_effect_without_qualifier_is_rejected() {
    let src = r#"
        tool process_health_record {
          provider: stub
          timeout: 10s
          effects: <sensitive:health_data, legal>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "Effect 'legal'")
            && any_error_mentions(&errs, "basis qualifier"),
        "expected legal-missing-qualifier error, got {:?}",
        errs
    );
}

#[test]
fn legal_effect_with_unknown_basis_is_rejected() {
    let src = r#"
        tool process_health_record {
          provider: stub
          timeout: 10s
          effects: <sensitive:health_data, legal:MADE_UP.Act42>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "Unknown legal basis"),
        "expected unknown-basis error, got {:?}",
        errs
    );
}

#[test]
fn sensitive_without_category_qualifier_is_rejected() {
    let src = r#"
        tool process_payment {
          provider: stub
          timeout: 10s
          effects: <sensitive, legal:PCI_DSS.v4_Req3>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "Effect 'sensitive'")
            && any_error_mentions(&errs, "jurisdiction qualifier"),
        "expected sensitive-without-category error, got {:?}",
        errs
    );
}

#[test]
fn sensitive_without_legal_basis_is_rejected() {
    let src = r#"
        tool process_health_record {
          provider: stub
          timeout: 10s
          effects: <sensitive:health_data>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "no 'legal:<basis>' effect"),
        "expected sensitive-without-legal error, got {:?}",
        errs
    );
}

#[test]
fn sensitive_with_matching_legal_basis_passes() {
    let src = r#"
        tool process_health_record {
          provider: stub
          timeout: 10s
          effects: <sensitive:health_data, legal:HIPAA.164_502>
        }
    "#;
    let errs = type_check(src);
    assert!(
        !any_error_mentions(&errs, "no 'legal:<basis>' effect")
            && !any_error_mentions(&errs, "Unknown legal basis"),
        "expected clean compile, got {:?}",
        errs
    );
}

#[test]
fn legal_without_sensitive_is_tolerated() {
    // A tool can declare a legal basis without processing regulated
    // data — some tools carry broad authorisations. Not an error.
    let src = r#"
        tool audit_log_writer {
          provider: stub
          timeout: 10s
          effects: <legal:SOX.404, io>
        }
    "#;
    let errs = type_check(src);
    assert!(
        !any_error_mentions(&errs, "no 'legal:<basis>' effect")
            && !any_error_mentions(&errs, "Unknown"),
        "lone legal: should pass, got {:?}",
        errs
    );
}

#[test]
fn all_legal_basis_slugs_compile() {
    for basis in [
        "GDPR.Art6.Consent",
        "GDPR.Art6.Contract",
        "GDPR.Art6.LegalObligation",
        "GDPR.Art6.VitalInterests",
        "GDPR.Art6.PublicTask",
        "GDPR.Art6.LegitimateInterests",
        "GDPR.Art9.ExplicitConsent",
        "GDPR.Art9.Employment",
        "GDPR.Art9.VitalInterests",
        "GDPR.Art9.NotForProfit",
        "GDPR.Art9.PublicData",
        "GDPR.Art9.LegalClaims",
        "GDPR.Art9.SubstantialPublicInterest",
        "GDPR.Art9.HealthcareProvision",
        "GDPR.Art9.PublicHealth",
        "GDPR.Art9.ArchivingResearch",
        "CCPA.1798_100",
        "SOX.404",
        "HIPAA.164_502",
        "GLBA.501b",
        "PCI_DSS.v4_Req3",
    ] {
        let src = format!(
            r#"
                tool t {{
                  provider: stub
                  timeout: 10s
                  effects: <legal:{basis}>
                }}
            "#
        );
        let errs = type_check(&src);
        assert!(
            !any_error_mentions(&errs, "Unknown legal basis"),
            "basis {basis} rejected: {:?}",
            errs
        );
    }
}

// ── Catalogue coverage regression ────────────────────────────────────

#[test]
fn legal_basis_catalog_is_case_sensitive() {
    let src = r#"
        tool t {
          provider: stub
          timeout: 10s
          effects: <sensitive:phi, legal:hipaa.164_502>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "Unknown legal basis"),
        "case-variant 'hipaa.*' must be rejected"
    );
}
