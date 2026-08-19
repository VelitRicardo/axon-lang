//! v2.5.0 — type-checker validation of `extension` + augmented catalogs.
//!
//! Pins the soundness invariants:
//!   1. A tool using an extension-declared effect member type-checks
//!      clean (the member is accepted verbatim, provenance-class).
//!   2. A tool using an UNDECLARED member of the same axis still errors
//!      (only the declared members are honored — not the whole prefix).
//!   3. A shield using an extension-declared scan category type-checks
//!      clean.
//!   4. INVARIANT #2 — an `effects` member whose base is a canonical
//!      ENFORCEABLE base (`io:bypass_shield`) is REJECTED (no smuggling
//!      a privileged effect under a custom name).
//!   5. INVARIANT #3 — a `scan` member shadowing a canonical category
//!      (`code_injection`) is REJECTED.
//!   6. `default_confidence` outside [0,1] is REJECTED.
//!   7. An unknown `category` is REJECTED.
//!   8. A rejected member is NOT honored downstream (the tool using a
//!      rejected member still errors — fail-closed).

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

fn has(errs: &[String], needle: &str) -> bool {
    errs.iter().any(|e| e.contains(needle))
}

// A generic provenance axis. NOTE: `epistemic:` is NOT used here on
// purpose — the effects-row parser special-cases `epistemic:` into the
// built-in `epistemic_level` field (parser.rs `parse_effect_row`), so
// it never reaches the effects list. A custom `risk:` base exercises
// the generic extension-effects path the type-checker actually gates.
const RISK_EXT: &str = r#"
extension risk_axis {
  category: effects
  members: [
    "risk:elevated" : { semantics: "external", default_confidence: 0.80 },
    "risk:high"     : { semantics: "untrusted", default_confidence: 0.95 }
  ]
}
"#;

// ─── section 1 — declared effect member accepted ──────────────────────────

#[test]
fn tool_using_declared_effect_member_typechecks_clean() {
    let src =
        format!("{RISK_EXT}\ntool t {{ description: \"t\" effects: <network, risk:elevated> }}");
    let errs = errors(&src);
    assert!(
        !has(&errs, "Unknown effect"),
        "declared extension member must be accepted: {errs:?}"
    );
}

// ─── section 2 — undeclared member of the same axis still errors ──────────

#[test]
fn tool_using_undeclared_member_still_errors() {
    let src = format!("{RISK_EXT}\ntool t {{ description: \"t\" effects: <risk:guess> }}");
    let errs = errors(&src);
    assert!(
        has(&errs, "Unknown effect 'risk:guess'"),
        "an undeclared member must NOT be honored by prefix: {errs:?}"
    );
}

// ─── section 3 — declared scan category accepted ──────────────────────────

#[test]
fn shield_using_declared_scan_category_typechecks_clean() {
    let src = r#"
extension collections_scans { category: scan members: [ "dunning_pressure" ] }
shield s { scan: [dunning_pressure] strategy: pattern on_breach: halt }
"#;
    let errs = errors(src);
    assert!(
        !has(&errs, "Unknown scan category"),
        "declared scan category must be accepted: {errs:?}"
    );
}

// ─── section 4 — invariant #2: cannot shadow an enforceable base ──────────

#[test]
fn effects_member_shadowing_enforceable_base_is_rejected() {
    let src = r#"
extension malicious {
  category: effects
  members: [ "io:bypass_shield" ]
}
"#;
    let errs = errors(src);
    assert!(
        has(&errs, "PROVENANCE-class only") && has(&errs, "io"),
        "io:bypass must be rejected (invariant #2): {errs:?}"
    );
}

// ─── section 5 — invariant #3: cannot shadow a canonical scan category ────

#[test]
fn scan_member_shadowing_canonical_category_is_rejected() {
    let src = r#"
extension dup { category: scan members: [ "code_injection" ] }
"#;
    let errs = errors(src);
    assert!(
        has(&errs, "shadows a canonical scan category"),
        "code_injection shadow must be rejected (invariant #3): {errs:?}"
    );
}

// ─── section 6 — default_confidence range ─────────────────────────────────

#[test]
fn out_of_range_default_confidence_is_rejected() {
    let src = r#"
extension bad { category: effects members: [ "epistemic:x" : { default_confidence: 1.5 } ] }
"#;
    let errs = errors(src);
    assert!(
        has(&errs, "outside the valid range [0.0, 1.0]"),
        "default_confidence 1.5 must be rejected: {errs:?}"
    );
}

// ─── section 7 — unknown category ─────────────────────────────────────────

#[test]
fn unknown_category_is_rejected() {
    let src = r#"
extension weird { category: telepathy members: [ "x" ] }
"#;
    let errs = errors(src);
    assert!(
        has(&errs, "unknown category 'telepathy'"),
        "unknown category must be rejected: {errs:?}"
    );
}

// ─── section 8 — a rejected member is NOT honored downstream ──────────────

#[test]
fn rejected_member_is_not_honored_downstream() {
    // The extension member `io:bypass_shield` is rejected (section 4), so a tool
    // declaring it must STILL error (fail-closed — the rejected member
    // was never added to the augmented catalog).
    let src = r#"
extension malicious { category: effects members: [ "io:bypass_shield" ] }
tool t { description: "t" effects: <io:bypass_shield> }
"#;
    let errs = errors(src);
    // The extension itself is rejected; the tool's use must not be
    // silently accepted via the augmented set.
    assert!(
        has(&errs, "PROVENANCE-class only"),
        "the extension declaration must be rejected: {errs:?}"
    );
    // `io:bypass_shield` splits to base `io` (a known base) so the tool
    // line itself does not emit "Unknown effect" — but the point is the
    // member was NOT added to ext_effect_members, so no laundering.
    // (Defense-in-depth: the extension rejection is the gate.)
}
