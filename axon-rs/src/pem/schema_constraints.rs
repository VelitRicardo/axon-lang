//! v2.88.0 — **a declared `type` lowered into a constraint set**, so
//! `validate X against: T` produces a CSR instead of prose.
//!
//! # The problem this solves
//!
//! `validate <target> against: <Schema>` is published in four README blocks.
//! Measured before this landed, `run_validate` asked the model for *"a
//! structured verdict (pass/fail) with the reasoning that supports it"* and
//! returned the answer as **opaque text**. The prompt requested a structure and
//! the runtime threw it away — v2.67.0's shape inside a primitive attested `Real`.
//!
//! And three of those four blocks follow the validation with
//! `if confidence < 0.8 -> refine(max_attempts: 2)`. There was no confidence.
//!
//! # The mapping, and why each half of it is forced
//!
//! A `type` is a claim about STRUCTURE. `pem::semantic_validator` already
//! computes the ratio this needs — `CSR = |{c ∈ C : r ⊨ c}| / |C|`, shipped in
//! v2.83.0 and driving `mandate` — so the only question is which `C` a type
//! denotes. The answer:
//!
//! ```text
//!   C_T  =  { ParsesAsJson }  ∪  { JsonField{f} : f ∈ fields(T), ¬optional(f) }
//! ```
//!
//! **Structure is not evidenced by the presence of text.** The tempting
//! lowering — one `Contains { needle: "\"amount\"" }` per field — is a substring
//! test over prose. It passes on *"I could not determine the amount"*. So each
//! field becomes a [`Predicate::JsonField`]: parse, require an object, look up
//! the key. That predicate is the one member v2.88.0 added to the closed catalog,
//! deliberately, because none of the six that existed can express a structural
//! obligation without lying about what it measured.
//!
//! **Optional fields are not obligations.** `TypeExpr.optional` already exists
//! and survives into `IRTypeField`, so "non-optional" is read off the
//! declaration rather than assumed.
//!
//! **`ParsesAsJson` is in the set even though it is arithmetically redundant** —
//! a non-JSON response fails every `JsonField` too, so CSR is 0 either way. It
//! earns its place in the FEEDBACK: without it a prose answer yields N copies of
//! *"not valid JSON, so field … cannot be present"*, and `refine` would be
//! handed N restatements of one root cause. With it, the root cause is named
//! once and the field obligations read as themselves.
//!
//! # What this deliberately does NOT do
//!
//! It does not check field TYPES. `amount: Float` lowers to *"the object must
//! carry a non-null `amount`"*, not *"…whose value is a number"*. Type
//! conformance of a JSON value against a declared column type is
//! `store_column_proof`'s job (`axon-T802`), and duplicating that classification
//! here would make a second catalog of one concept — the v2.83.0 defect. When
//! `validate` needs it, it should CALL that engine, not re-derive it.

use crate::ir_nodes::IRType;
use crate::pem::semantic_validator::{Clause, Constraint, ConstraintSet, Predicate, ValidatorError};

/// Lower a declared `type` into the constraint set `validate … against:` scores
/// a response with.
///
/// Fails with [`ValidatorError::NoCheckableConstraints`] only if the set comes
/// out empty, which it cannot today — `ParsesAsJson` is unconditional. The
/// `Result` is kept so the signature does not have to change when a schema form
/// arrives that CAN lower to nothing.
pub fn constraints_for_type(ty: &IRType) -> Result<ConstraintSet, ValidatorError> {
    let mut clauses = Vec::new();

    clauses.push(Clause::Checkable(Constraint {
        id: "c1".to_string(),
        source: format!("type {} — the response must be structured", ty.name),
        predicate: Predicate::ParsesAsJson,
    }));

    for f in ty.fields.iter().filter(|f| !f.optional) {
        let id = format!("c{}", clauses.len() + 1);
        clauses.push(Clause::Checkable(Constraint {
            id,
            source: format!("type {} — field `{}: {}`", ty.name, f.name, f.type_name),
            predicate: Predicate::JsonField {
                field: f.name.clone(),
            },
        }));
    }

    ConstraintSet::new(clauses)
}

/// How many obligations a type denotes — `|C_T|`. Exposed so a diagnostic can
/// say what a CSR was computed over without rebuilding the set.
pub fn obligation_count(ty: &IRType) -> usize {
    1 + ty.fields.iter().filter(|f| !f.optional).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir_nodes::IRTypeField;

    fn field(name: &str, ty: &str, optional: bool) -> IRTypeField {
        IRTypeField {
            node_type: "type_field",
            source_line: 0,
            source_column: 0,
            name: name.to_string(),
            type_name: ty.to_string(),
            generic_param: String::new(),
            optional,
        }
    }

    fn schema(fields: Vec<IRTypeField>) -> IRType {
        IRType {
            node_type: "type",
            source_line: 0,
            source_column: 0,
            name: "ContractSchema".to_string(),
            fields,
            range_min: None,
            range_max: None,
            where_expression: String::new(),
            compliance: Vec::new(),
        }
    }

    /// `|C_T| = 1 + |required fields|`, and optional fields are not obligations.
    #[test]
    fn the_set_is_structure_plus_every_required_field() {
        let t = schema(vec![
            field("parties", "String", false),
            field("obligations", "String", false),
            field("notes", "String", true),
        ]);
        assert_eq!(obligation_count(&t), 3);
        let cs = constraints_for_type(&t).expect("lowers");
        assert_eq!(cs.len(), 3, "1 structural + 2 required (the optional is NOT an obligation)");
    }

    /// A well-formed response satisfies everything — CSR = 1.
    #[test]
    fn a_conforming_json_object_scores_one() {
        let t = schema(vec![field("parties", "String", false)]);
        let cs = constraints_for_type(&t).unwrap();
        let v = cs.evaluate(r#"{"parties": "Acme and Beta"}"#);
        assert_eq!(v.csr, 1.0, "violations: {:?}", v.violated);
        assert!(v.is_satisfied());
    }

    /// **The assertion the whole mapping exists for.** Prose that MENTIONS the
    /// field must not satisfy it. A `Contains` lowering would pass this, and it
    /// is exactly the sentence a failing model produces.
    #[test]
    fn prose_that_mentions_the_field_does_not_satisfy_it() {
        let t = schema(vec![field("amount", "Float", false)]);
        let cs = constraints_for_type(&t).unwrap();
        let v = cs.evaluate("I could not determine the amount from the document.");
        assert_eq!(
            v.csr, 0.0,
            "a substring test would score this 1.0 — structure is NOT evidenced \
             by the presence of text"
        );
    }

    /// A field present but null is ABSENT. Counting it would let a response
    /// claim every field of a schema while carrying none of the values.
    #[test]
    fn a_null_member_does_not_satisfy_its_field() {
        let t = schema(vec![field("amount", "Float", false)]);
        let cs = constraints_for_type(&t).unwrap();
        let v = cs.evaluate(r#"{"amount": null}"#);
        assert_eq!(v.csr, 0.5, "structure held; the field did not: {:?}", v.violated);
    }

    /// Partial conformance is a RATIO, not a boolean — which is the entire
    /// reason `if confidence < 0.8` can mean something.
    #[test]
    fn partial_conformance_lands_between_zero_and_one() {
        let t = schema(vec![
            field("a", "String", false),
            field("b", "String", false),
            field("c", "String", false),
        ]);
        let cs = constraints_for_type(&t).unwrap();
        let v = cs.evaluate(r#"{"a": 1, "b": 2}"#);
        assert!((v.csr - 0.75).abs() < 1e-9, "1 structural + 2 of 3 fields = 3/4, got {}", v.csr);
        assert!((v.error - 0.25).abs() < 1e-9, "e = 1 − CSR");
    }

    /// Valid JSON that is not an OBJECT fails the fields with its own reason —
    /// "not an object" and "no such member" send an author to different places.
    #[test]
    fn a_json_array_names_its_own_failure() {
        let t = schema(vec![field("a", "String", false)]);
        let cs = constraints_for_type(&t).unwrap();
        let v = cs.evaluate(r#"["a"]"#);
        assert_eq!(v.csr, 0.5, "the structural obligation held — it IS valid JSON");
        assert!(
            v.violated[0].reason.contains("not an OBJECT"),
            "got: {}",
            v.violated[0].reason
        );
    }

    /// The root cause is named ONCE. Without `ParsesAsJson` in the set, `refine`
    /// would receive N restatements of one problem.
    #[test]
    fn prose_feedback_names_the_root_cause_once() {
        let t = schema(vec![
            field("a", "String", false),
            field("b", "String", false),
        ]);
        let cs = constraints_for_type(&t).unwrap();
        let fb = cs.evaluate("not json at all").feedback();
        assert_eq!(
            fb.matches("must be valid JSON").count(),
            1,
            "the structural obligation appears exactly once:\n{fb}"
        );
    }

    /// A schema with no required fields measures only structure. Weak, and
    /// honest — refusing it would invent a rule the declaration does not carry.
    #[test]
    fn a_schema_with_no_required_fields_measures_structure_alone() {
        let t = schema(vec![field("maybe", "String", true)]);
        let cs = constraints_for_type(&t).expect("still lowers");
        assert_eq!(cs.len(), 1);
        assert_eq!(cs.evaluate(r#"{}"#).csr, 1.0);
        assert_eq!(cs.evaluate("prose").csr, 0.0);
    }
}
