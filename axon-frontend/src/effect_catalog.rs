//! v2.87.0 — the CLOSED catalog of declared algebraic effects, and the
//! resolution of a bare `perform Op(x)` against it (**the design decision**).
//!
//! # Why this is one module and not two implementations
//!
//! Both the IR generator and the type-checker need the same answer to the same
//! question — *which effect declares this operation?* The generator needs it to
//! FILL `IREffectPerform::effect_name`; the checker needs it to REFUSE with a
//! location when the answer is not exactly one. Deriving it twice is how the
//! two drift, and a drift here is invisible: the checker would pass a program
//! the generator then lowers against a different effect.
//!
//! `feedback_free_string_field_breeds_fake_catalog` is the rule this module
//! exists to obey — *a free string field breeds an imaginary catalog; the fix
//! is a closed catalog keyed by what the runtime reaches.* The runtime reaches
//! `IRProgram::effects`, which is built from `Declaration::Effect`. So that is
//! the key.
//!
//! # The resolution rule
//!
//! `the design plan` section 3.1 publishes the BARE form (`perform Emit(response.token)`)
//! while D9 writes the QUALIFIED one (`perform Effect.Op(...)`), and `IRPerform`
//! carries both names. Under v2.83.0's doctrine — *the published document is the
//! promise, the compiler is what must honour it* — both parse, and the bare
//! form resolves by lookup:
//!
//! | declarers of `Op` | outcome |
//! |---|---|
//! | exactly one | resolved to that effect |
//! | two or more | **compile error naming every candidate**, author must qualify |
//! | none | **compile error** — the operation is not declared anywhere |
//!
//! This is a LOOKUP, not an inference: the answer is a function of the declared
//! set, and the ambiguous case is refused rather than picked. Picking would be
//! the defect the two-effects-one-operation case exists to expose — a
//! `perform Emit(x)` silently routed to the logger instead of the wire.

use std::collections::BTreeMap;

use crate::ast::{Declaration, EffectDefinition, Program};

/// The outcome of resolving an operation name against the declared catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpResolution {
    /// Exactly one declared effect owns this operation.
    Resolved(String),
    /// Two or more declare it. Carries EVERY candidate, sorted, so the
    /// diagnostic can name them all — an error that says "ambiguous" without
    /// saying between what is an error the author cannot act on.
    Ambiguous(Vec<String>),
    /// No declared effect owns it.
    Undeclared,
}

/// The closed catalog: declared effects, and the operations each one owns.
#[derive(Debug, Default, Clone)]
pub struct EffectCatalog {
    /// effect name → its operation names, in declaration order.
    effects: BTreeMap<String, Vec<String>>,
    /// operation name → the effects declaring it. `BTreeMap`/sorted so the
    /// ambiguity diagnostic is DETERMINISTIC — a message whose candidate order
    /// depends on hash iteration is a message that cannot be asserted on.
    by_operation: BTreeMap<String, Vec<String>>,
    /// (effect, operation) → declared parameter count, for the arity check.
    arity: BTreeMap<(String, String), usize>,
}

impl EffectCatalog {
    /// Build the catalog from a parsed program.
    pub fn from_program(program: &Program) -> Self {
        let mut catalog = Self::default();
        for decl in &program.declarations {
            if let Declaration::Effect(eff) = decl {
                catalog.insert(eff);
            }
        }
        for owners in catalog.by_operation.values_mut() {
            owners.sort();
            owners.dedup();
        }
        catalog
    }

    fn insert(&mut self, eff: &EffectDefinition) {
        let ops: Vec<String> = eff.operations.iter().map(|o| o.name.clone()).collect();
        for op in &eff.operations {
            self.by_operation
                .entry(op.name.clone())
                .or_default()
                .push(eff.name.clone());
            self.arity
                .insert((eff.name.clone(), op.name.clone()), op.parameters.len());
        }
        // A program that declares the SAME effect name twice keeps the union of
        // the operations rather than dropping one silently. The duplicate name
        // is itself refused by the type-checker's declaration registry; this
        // only guarantees the catalog never loses an operation while that error
        // is being reported.
        self.effects.entry(eff.name.clone()).or_default().extend(ops);
    }

    /// True iff an effect with this name is declared.
    pub fn declares_effect(&self, effect: &str) -> bool {
        self.effects.contains_key(effect)
    }

    /// True iff `effect` declares `operation`.
    pub fn declares_operation(&self, effect: &str, operation: &str) -> bool {
        self.arity.contains_key(&(effect.to_string(), operation.to_string()))
    }

    /// The declared parameter count of `effect.operation`, if declared.
    pub fn arity_of(&self, effect: &str, operation: &str) -> Option<usize> {
        self.arity
            .get(&(effect.to_string(), operation.to_string()))
            .copied()
    }

    /// Every declared effect name, sorted.
    pub fn effect_names(&self) -> Vec<&str> {
        self.effects.keys().map(|s| s.as_str()).collect()
    }

    /// the design decision — resolve a BARE operation name.
    pub fn resolve_bare(&self, operation: &str) -> OpResolution {
        match self.by_operation.get(operation) {
            None => OpResolution::Undeclared,
            Some(owners) if owners.len() == 1 => OpResolution::Resolved(owners[0].clone()),
            Some(owners) => OpResolution::Ambiguous(owners.clone()),
        }
    }

    /// Resolve a perform/forward site to its effect name, honouring an explicit
    /// qualifier when the source wrote one.
    ///
    /// A QUALIFIED site is taken at its word here — whether the named effect
    /// actually declares the operation is a separate diagnostic
    /// ([`Self::declares_operation`]), reported by the type-checker so the
    /// author gets "SSE has no operation Emitt", not "ambiguous".
    pub fn resolve_site(&self, explicit: Option<&str>, operation: &str) -> OpResolution {
        match explicit {
            Some(effect) => OpResolution::Resolved(effect.to_string()),
            None => self.resolve_bare(operation),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn catalog(src: &str) -> EffectCatalog {
        let tokens = Lexer::new(src, "<catalog-test>").tokenize().expect("lex");
        let program = Parser::new(tokens).parse().expect("parse");
        EffectCatalog::from_program(&program)
    }

    #[test]
    fn one_declarer_resolves() {
        let c = catalog("effect SSE { Emit(token: Token) -> Unit }");
        assert_eq!(
            c.resolve_bare("Emit"),
            OpResolution::Resolved("SSE".to_string())
        );
    }

    /// The case the whole rule exists for. Two effects, one operation name: the
    /// catalog must REFUSE and name both, never pick.
    #[test]
    fn two_declarers_are_ambiguous_and_both_are_named() {
        let c = catalog(
            "effect SSE { Emit(token: Token) -> Unit }\n\
             effect Log { Emit(message: Text) -> Unit }",
        );
        match c.resolve_bare("Emit") {
            OpResolution::Ambiguous(owners) => {
                assert_eq!(owners, vec!["Log".to_string(), "SSE".to_string()]);
            }
            other => panic!("a colliding operation must be ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn undeclared_operation_does_not_resolve() {
        let c = catalog("effect SSE { Emit(token: Token) -> Unit }");
        assert_eq!(c.resolve_bare("Nope"), OpResolution::Undeclared);
    }

    /// A qualified site is never ambiguous — that is the escape hatch the
    /// ambiguity diagnostic points the author at, so it must actually work.
    #[test]
    fn qualifying_escapes_the_ambiguity() {
        let c = catalog(
            "effect SSE { Emit(token: Token) -> Unit }\n\
             effect Log { Emit(message: Text) -> Unit }",
        );
        assert_eq!(
            c.resolve_site(Some("SSE"), "Emit"),
            OpResolution::Resolved("SSE".to_string())
        );
    }

    #[test]
    fn arity_and_membership_come_from_the_declaration() {
        let c = catalog("effect SSE { Emit(token: Token) -> Unit  Done() -> Never }");
        assert!(c.declares_effect("SSE"));
        assert!(!c.declares_effect("Nope"));
        assert!(c.declares_operation("SSE", "Done"));
        assert!(!c.declares_operation("SSE", "Missing"));
        assert_eq!(c.arity_of("SSE", "Emit"), Some(1));
        assert_eq!(c.arity_of("SSE", "Done"), Some(0));
        assert_eq!(c.arity_of("SSE", "Missing"), None);
    }
}
