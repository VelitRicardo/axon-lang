//! v2.83.0 — the name-keyed `ots` transformer registry.
//!
//! # Why this exists
//!
//! v2.67.0 F18: *"`apply_ots_to_target` is literally `target.to_string()`; no
//! ots_registry exists anywhere, so the documented 'enterprise override' has
//! no hook to override."* An `ots` whose application is the identity function
//! is the same defect as the mandate that transformed nothing: a primitive
//! that promises transformation, applies nothing, and can never fail.
//!
//! This registry is that hook, made real — the exact shape of
//! [`crate::shield_registry`] (v2.0.0), which is the proven pattern for
//! "OSS framework + registered implementation": a name-keyed table of
//! transformers, consulted at dispatch, with a verdict type that can REFUSE.
//!
//! # The one deliberate difference from `shield_registry`
//!
//! An UNREGISTERED shield passes content through unmodified — that is
//! shield's documented contract (no scanner ⇒ data unmodified), and it is
//! honest there because a shield is a *filter*: absence of filtering leaves
//! the data exactly as true as it was.
//!
//! An unregistered `ots` REFUSES (`run_ots_apply` fails the flow). An `ots`
//! is a *transformation*: its declaration promises that the output IS the
//! transformed input, and passing the input through unchanged under a
//! transformation's name fabricates a result — the v2.67.0 F18 lie, which this
//! cycle exists to end. Absence of a transformer is not a weaker transform;
//! it is the inability to honor the declaration, and it is said out loud.
//!
//! # What registers here
//!
//! - The in-tree media pipeline (`crate::ots::TransformerRegistry`, v2.4.0-era)
//!   remains the buffer-typed engine for audio/voice paths; a name-keyed
//!   entry here may delegate into it.
//! - Enterprise verticals register their transformers exactly as they
//!   register shield scanners.
//! - Tests register deterministic transformers to prove the dispatch path —
//!   which is the point: the hook is real because a test can watch it fire,
//!   and watch its absence refuse.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Context handed to a transformer alongside the content.
#[derive(Debug, Clone)]
pub struct OtsTransformContext {
    /// The `ots` declaration's name (`ThreatPatcher`, …).
    pub ots_name: String,
    /// The declared teleology, verbatim — the intent the transform serves.
    pub teleology: String,
    /// The declared homotopy search mode (closed catalog:
    /// deep | shallow | speculative).
    pub homotopy_search: String,
    /// The declared loss function, verbatim.
    pub loss_function: String,
}

/// What a transformer decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OtsVerdict {
    /// The transformation succeeded; `content` is the transformed output.
    Transformed(String),
    /// The transformer could not honor the declaration for this input.
    /// Surfaces as a flow error naming the ots — never a silent passthrough.
    Refused { code: String, reason: String },
}

/// A registered `ots` transformer.
pub trait OtsTransformer: Send + Sync {
    /// Transform `content` under the declaration in `ctx`.
    fn transform(&self, content: &str, ctx: &OtsTransformContext) -> OtsVerdict;
}

static REGISTRY: RwLock<Option<HashMap<String, Arc<dyn OtsTransformer>>>> = RwLock::new(None);

/// Register a transformer for an `ots` declaration name. Later registrations
/// for the same name replace earlier ones (the shield-registry discipline).
pub fn register_ots_transformer(ots_name: impl Into<String>, t: Arc<dyn OtsTransformer>) {
    let mut guard = REGISTRY.write().expect("ots registry poisoned");
    guard
        .get_or_insert_with(HashMap::new)
        .insert(ots_name.into(), t);
}

/// Look up the transformer registered for `ots_name`.
pub fn lookup_ots_transformer(ots_name: &str) -> Option<Arc<dyn OtsTransformer>> {
    let guard = REGISTRY.read().expect("ots registry poisoned");
    guard.as_ref().and_then(|m| m.get(ots_name).cloned())
}

/// Remove a registration (test hygiene).
pub fn unregister_ots_transformer(ots_name: &str) -> Option<Arc<dyn OtsTransformer>> {
    let mut guard = REGISTRY.write().expect("ots registry poisoned");
    guard.as_mut().and_then(|m| m.remove(ots_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Upper;
    impl OtsTransformer for Upper {
        fn transform(&self, content: &str, _ctx: &OtsTransformContext) -> OtsVerdict {
            OtsVerdict::Transformed(content.to_uppercase())
        }
    }

    #[test]
    fn register_lookup_roundtrip_and_replacement() {
        register_ots_transformer("ots-reg-test", Arc::new(Upper));
        let t = lookup_ots_transformer("ots-reg-test").expect("registered");
        let ctx = OtsTransformContext {
            ots_name: "ots-reg-test".into(),
            teleology: String::new(),
            homotopy_search: String::new(),
            loss_function: String::new(),
        };
        assert_eq!(t.transform("abc", &ctx), OtsVerdict::Transformed("ABC".into()));
        unregister_ots_transformer("ots-reg-test");
        assert!(lookup_ots_transformer("ots-reg-test").is_none());
    }

    #[test]
    fn unregistered_name_is_none_never_a_default() {
        assert!(lookup_ots_transformer("no-such-ots").is_none());
    }
}
