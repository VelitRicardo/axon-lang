//! §Fase 119.e — `fabric` governs: the compile-time half.
//!
//! §111's finding: *"`provider`/`region`/`zones` are consumed by NOTHING at
//! runtime … Still governs nothing that runs."* `fabric` has no paper; its
//! specification is its knowledge doc, which draws the line this sub-fase
//! works inside:
//!
//! > **Not infrastructure-as-code.** AXON fabric declarations do NOT provision
//! > infrastructure — they *describe* what the runtime expects to find. The
//! > fabric makes expectations **typed + auditable**.
//!
//! So making it real is not "make it provision". It is: a declaration that
//! cannot be true is refused (`axon-E041`), an obligation the substrate cannot
//! satisfy is refused (`axon-E042` — the doc's own GDPR example), and what
//! runs carries `(provider, region)` (gated on the runtime side, in axon-rs).

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn check(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

// ── axon-E041 — a substrate that does not exist ─────────────────────

#[test]
fn the_docs_own_mismatch_example_is_refused() {
    // fabric.md: "Provider mismatches (`provider: aws  region: \"eastus\"`)
    // are rejected with a structured `axon-E041 region/provider mismatch`."
    let errors = check(
        r#"
fabric Wrong {
    provider: aws
    region: "eastus"
    zones: 3
}
"#,
    );
    let e = errors
        .iter()
        .find(|m| m.contains("axon-E041"))
        .unwrap_or_else(|| panic!("expected axon-E041, got {errors:?}"));
    assert!(e.contains("eastus") && e.contains("aws"), "{e}");
    assert!(
        e.contains("us-east-1"),
        "the diagnostic shows what a valid region for this provider looks like: {e}"
    );
}

#[test]
fn a_matching_provider_and_region_compiles_clean() {
    for (provider, region) in [
        ("aws", "us-east-1"),
        ("gcp", "europe-west1"),
        ("azure", "westeurope"),
    ] {
        let errors = check(&format!(
            "fabric Ok {{ provider: {provider} region: \"{region}\" zones: 3 }}"
        ));
        assert!(
            errors.is_empty(),
            "{provider}/{region} should compile: {errors:?}"
        );
    }
}

#[test]
fn an_unvalidated_provider_is_accepted_because_the_catalog_is_open() {
    // The knowledge doc: "Open catalogue at the parser level — the runtime
    // decides which providers are deployable." Claiming to know every cloud's
    // topology would be a fabricated rule.
    let errors = check(r#"fabric OnPrem { provider: onprem region: "rack-7" }"#);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn an_absent_region_is_fine_because_region_is_optional() {
    let errors = check("fabric Bare { provider: aws zones: 1 }");
    assert!(errors.is_empty(), "{errors:?}");
}

// ── axon-E042 — an obligation the substrate cannot satisfy ──────────

const GDPR_ON_US: &str = r#"
fabric UsCloud {
    provider: aws
    region: "us-east-1"
}

resource Db {
    kind: postgres
    endpoint: "env:DB_URL"
    within: UsCloud
}

manifest Prod {
    resources: [Db]
    fabric: UsCloud
    compliance: [gdpr]
}
"#;

#[test]
fn the_docs_own_gdpr_example_is_refused() {
    // fabric.md: "a GDPR-tagged manifest deployed to a non-EU region is
    // rejected." This is where `region` stops being a label: it decides
    // whether a declared obligation can hold.
    let errors = check(GDPR_ON_US);
    let e = errors
        .iter()
        .find(|m| m.contains("axon-E042"))
        .unwrap_or_else(|| panic!("expected axon-E042, got {errors:?}"));
    assert!(e.contains("gdpr"), "{e}");
    assert!(
        e.contains("out of the jurisdiction"),
        "the refusal says what would actually happen: {e}"
    );
}

#[test]
fn the_same_manifest_in_an_eu_region_compiles_clean() {
    let errors = check(&GDPR_ON_US.replace("us-east-1", "eu-west-1"));
    assert!(
        !errors.iter().any(|m| m.contains("axon-E042")),
        "{errors:?}"
    );
}

#[test]
fn an_undeterminable_jurisdiction_is_refused_never_waved_through() {
    // THE decisive one. A substrate this compiler cannot place is a substrate
    // whose GDPR obligation was never SHOWN to hold — and the entire value of
    // a typed expectation is that it was shown. "We cannot tell" must not
    // read as "it is fine".
    let errors = check(&GDPR_ON_US.replace("provider: aws", "provider: onprem"));
    let e = errors
        .iter()
        .find(|m| m.contains("axon-E042"))
        .unwrap_or_else(|| panic!("expected axon-E042, got {errors:?}"));
    assert!(e.contains("cannot be determined"), "{e}");
}

#[test]
fn tags_with_no_geographic_obligation_are_not_invented_into_one() {
    // Inventing a geography for soc2 would be the fabricated catalog this
    // codebase has caught five times.
    let errors = check(&GDPR_ON_US.replace("compliance: [gdpr]", "compliance: [soc2]"));
    assert!(
        !errors.iter().any(|m| m.contains("axon-E042")),
        "{errors:?}"
    );
}

#[test]
fn a_manifest_with_no_fabric_is_not_judged_on_a_substrate_it_never_named() {
    let errors = check(
        r#"
resource Db { kind: postgres endpoint: "env:DB_URL" }

manifest Prod {
    resources: [Db]
    compliance: [gdpr]
}
"#,
    );
    assert!(
        !errors.iter().any(|m| m.contains("axon-E042")),
        "{errors:?}"
    );
}

#[test]
fn declaration_order_does_not_decide_whether_the_check_runs() {
    // The manifest is declared BEFORE the fabric it references. If the check
    // read accumulated state instead of the program, this would silently pass
    // — a governance rule that depends on source order governs nothing.
    let reordered = r#"
manifest Prod {
    resources: [Db]
    fabric: UsCloud
    compliance: [gdpr]
}

resource Db {
    kind: postgres
    endpoint: "env:DB_URL"
    within: UsCloud
}

fabric UsCloud {
    provider: aws
    region: "us-east-1"
}
"#;
    let errors = check(reordered);
    assert!(
        errors.iter().any(|m| m.contains("axon-E042")),
        "the check must fire regardless of declaration order: {errors:?}"
    );
}
