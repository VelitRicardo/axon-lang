//! v4.0.0 — **the compliance COVERAGE rule, computed over the IR.**
//!
//! # Why this exists
//!
//! Until this cycle, both the risk register and the gap analyzer granted the
//! program feature `has_compliance_annotation` on `!compliance.is_empty()` —
//! the mere PRESENCE of a label anywhere in the program. On that feature the
//! risk register asserted that "Regulated data crosses an uncovered boundary"
//! was *mitigated*, and the gap analyzer marked C1.1 / P1.1 / P6.1 / A.5.34
//! as satisfied. A program whose PHI-labelled type flowed through a shield
//! covering nothing scored identically to one whose every boundary was
//! covered. That is the presence-vs-exercise defect v2.67.0 condemned and v2.89.0
//! spent a cycle on, sitting inside the engine that WRITES AUDIT DOSSIERS.
//!
//! # The rule
//!
//! The feature the audit rows may lean on is renamed
//! `compliance_coverage_holds`, and it is granted iff:
//!
//! 1. at least one regulated boundary EXISTS — an `axonendpoint` whose
//!    body/output κ is non-empty and that dispatches a flow, or a `channel`
//!    whose payload κ is non-empty; and
//! 2. EVERY such boundary names a shield whose κ covers the data's κ.
//!
//! Labels that cross no boundary (a κ-annotated type nothing carries, an
//! endpoint's own `compliance:` list, a manifest's) grant nothing: they are
//! labels, and the doctrine since T957 is that a κ class is covered when
//! something can ACT on a breach of it. This is the same set-difference the
//! checker enforces as `axon-T957` (endpoints) and `axon-T1215` (channels) —
//! recomputed here because dossiers are built from IR, and IR does not carry
//! a certificate that it ever met the checker.
//!
//! One definition, called by both `risk_register` and `gap_analyzer`: their
//! `program_features` copies are deliberately self-contained for the trivial
//! presence facts, but a RULE duplicated is a rule that drifts (v2.87.0).

use std::collections::HashSet;

use crate::ir_nodes::IRProgram;
use axon_frontend::compliance::{peel_channel_payload, peel_type_constructors};

/// Does the program carry regulated data across at least one boundary, with
/// EVERY such boundary covered by its shield's κ?
///
/// Fail-closed on the right side: a boundary that names an unknown shield —
/// or none — while carrying non-empty κ returns `false` outright.
pub fn compliance_coverage_holds(program: &IRProgram) -> bool {
    let kappa_of_type = |type_ref: &str| -> HashSet<&str> {
        let base = peel_type_constructors(type_ref);
        program
            .types
            .iter()
            .find(|t| t.name == base)
            .map(|t| t.compliance.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    };
    let shield_covers = |required: &HashSet<&str>, shield_ref: &str| -> bool {
        program
            .shields
            .iter()
            .find(|s| s.name == shield_ref)
            .is_some_and(|s| {
                let provided: HashSet<&str> =
                    s.compliance.iter().map(|x| x.as_str()).collect();
                required.is_subset(&provided)
            })
    };

    let mut any_regulated = false;

    for e in &program.endpoints {
        // Same gating as axon-T957: an endpoint that dispatches nothing
        // crosses no boundary.
        if e.execute_flow.is_empty() {
            continue;
        }
        let mut required = kappa_of_type(&e.body_type);
        required.extend(kappa_of_type(&e.output_type));
        if required.is_empty() {
            continue;
        }
        any_regulated = true;
        if !shield_covers(&required, &e.shield_ref) {
            return false;
        }
    }

    for c in &program.channels {
        let required = kappa_of_type(peel_channel_payload(&c.message));
        if required.is_empty() {
            continue;
        }
        any_regulated = true;
        if !shield_covers(&required, &c.shield_ref) {
            return false;
        }
    }

    any_regulated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir_generator::IRGenerator;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    /// Lexer → Parser → IRGenerator, deliberately WITHOUT the type checker:
    /// this is precisely the "IR that never met `axon-T1215`" population the
    /// runtime rule exists for. The checker would refuse most of the negative
    /// programs below at declaration time.
    fn compile(source: &str) -> IRProgram {
        let tokens = Lexer::new(source, "t").tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        IRGenerator::new().generate(&program)
    }

    /// 🎯 A label that crosses no boundary grants NOTHING — the presence
    /// check this module replaces would have granted the feature here.
    #[test]
    fn an_annotation_nothing_carries_is_a_label_not_coverage() {
        let ir = compile("type Phi compliance [HIPAA] { x: String }");
        assert!(
            !compliance_coverage_holds(&ir),
            "a κ-annotated type that no endpoint or channel carries exercises \
             no coverage rule — scoring it was the presence-not-coverage defect"
        );
    }

    /// 🎯 The exact program the old check scored as mitigated: regulated
    /// payload, shield that covers nothing.
    #[test]
    fn an_uncovered_regulated_channel_denies_the_feature() {
        let ir = compile(
            r#"
            type Phi compliance [HIPAA] { x: String }
            shield Sieve { scan: [pii_leak] }
            channel PhiFeed { message: Phi shield: Sieve }
            "#,
        );
        assert!(!compliance_coverage_holds(&ir));
    }

    #[test]
    fn a_covered_regulated_channel_grants_the_feature() {
        let ir = compile(
            r#"
            type Phi compliance [HIPAA] { x: String }
            shield Sieve { scan: [pii_leak] compliance: [HIPAA, SOC2] }
            channel PhiFeed { message: Channel<Phi> shield: Sieve }
            "#,
        );
        assert!(
            compliance_coverage_holds(&ir),
            "`Channel<Phi>` carries Phi's κ — the peel must see through the wrapper"
        );
    }

    #[test]
    fn one_uncovered_boundary_poisons_an_otherwise_covered_program() {
        let ir = compile(
            r#"
            type Phi compliance [HIPAA] { x: String }
            type Card compliance [PCI_DSS] { x: String }
            shield Sieve { scan: [pii_leak] compliance: [HIPAA] }
            channel PhiFeed { message: Phi shield: Sieve }
            channel CardFeed { message: Card shield: Sieve }
            "#,
        );
        assert!(
            !compliance_coverage_holds(&ir),
            "coverage is a universal claim — one uncovered boundary and the \
             dossier may not assert the risk is mitigated"
        );
    }

    #[test]
    fn an_unknown_or_absent_shield_fails_closed() {
        let no_shield = compile(
            r#"
            type Phi compliance [HIPAA] { x: String }
            channel PhiFeed { message: Phi }
            "#,
        );
        assert!(!compliance_coverage_holds(&no_shield), "no shield at all");

        let ghost = compile(
            r#"
            type Phi compliance [HIPAA] { x: String }
            channel PhiFeed { message: Phi shield: Ghost }
            "#,
        );
        assert!(
            !compliance_coverage_holds(&ghost),
            "a shield the IR cannot resolve covers nothing"
        );
    }

    #[test]
    fn endpoints_participate_with_t957_gating() {
        let covered = compile(
            r#"
            type Phi compliance [HIPAA] { x: String }
            flow F(r: Phi) -> Phi { step S { ask: "x" output: Phi } }
            shield Sieve { scan: [pii_leak] compliance: [HIPAA] }
            axonendpoint E {
                method: POST path: "/p" body: Phi execute: F output: Phi
                shield: Sieve
            }
            "#,
        );
        assert!(compliance_coverage_holds(&covered));

        // An endpoint that dispatches no flow crosses no boundary (T957
        // gating) — and with no other boundary the feature is not granted.
        let inert = compile(
            r#"
            type Phi compliance [HIPAA] { x: String }
            shield Sieve { scan: [pii_leak] compliance: [HIPAA] }
            axonendpoint E {
                method: POST path: "/p" body: Phi output: Phi
                shield: Sieve
            }
            "#,
        );
        assert!(!compliance_coverage_holds(&inert));
    }
}
