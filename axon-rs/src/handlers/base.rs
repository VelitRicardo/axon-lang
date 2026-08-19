//! AXON Runtime — Handler Base Interface
//! =======================================
//! Direct port of `axon/runtime/handlers/base.py`.
//!
//! Free-Monad interpreter for the I/O Cognitivo Intention Tree. A Handler
//! receives pure intentions (`IRManifest`, `IRObserve`) from an
//! `IRIntentionTree` (v1.1.0) and produces concrete outcomes wrapped in
//! the Lambda Data envelope E = ⟨c, τ, ρ, δ⟩.
//!
//! Design anchors (docs/plan_io_cognitivo.md):
//!   * D1 — Free Monads + Handlers (CPS).
//!   * D4 — Partition ⇒ CT-3 infrastructure error, NEVER `doubt`.
//!   * D5 — Curry-Howard λ-L-E; each outcome is a constructive proof witness.

#![allow(dead_code)]

use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use chrono::Utc;
use serde::Serialize;
use serde_json::Value;

use crate::ir_nodes::{
    IRFabric, IRIntentionOperation, IRIntentionTree, IRManifest, IRObserve, IRProgram,
    IRResource,
};

// ═══════════════════════════════════════════════════════════════════
//  ΛD ENVELOPE — Lambda Data epistemic vector
// ═══════════════════════════════════════════════════════════════════

/// Accepted δ (derivation) kinds.
pub const VALID_DERIVATIONS: &[&str] = &["axiomatic", "observed", "inferred", "mutated"];

/// E = ⟨c, τ, ρ, δ⟩ — epistemic envelope wrapping every handler output.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LambdaEnvelope {
    /// Certainty in [0.0, 1.0]. 1.0 = `know`, 0.0 = `void` (⊥).
    pub c: f64,
    /// Temporal frame — ISO-8601 UTC timestamp of the observation.
    pub tau: String,
    /// Provenance — handler id + optional cryptographic signature (v1.2.0).
    pub rho: String,
    /// Derivation kind: axiomatic | observed | inferred | mutated.
    pub delta: String,
}

impl LambdaEnvelope {
    /// Construct an envelope, validating c and delta. Panics on violation —
    /// Python raises ValueError; in Rust these are invariant breaches that
    /// indicate a handler-layer bug (CT-1), so panicking here is correct.
    pub fn new(c: f64, tau: String, rho: String, delta: String) -> Self {
        assert!(
            (0.0..=1.0).contains(&c),
            "LambdaEnvelope.c must be in [0.0, 1.0]; got {c}"
        );
        assert!(
            VALID_DERIVATIONS.contains(&delta.as_str()),
            "LambdaEnvelope.delta must be one of {VALID_DERIVATIONS:?}; got '{delta}'"
        );
        LambdaEnvelope { c, tau, rho, delta }
    }

    /// Return a copy with certainty reduced (used when a lease expires → D2).
    pub fn decayed(&self, to_certainty: f64) -> Self {
        LambdaEnvelope::new(to_certainty, self.tau.clone(), self.rho.clone(), self.delta.clone())
    }
}

/// Current UTC timestamp as ISO-8601 for ΛD τ frames.
pub fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

/// Construct a ΛD envelope with either the supplied τ or the current time.
pub fn make_envelope(c: f64, rho: &str, delta: &str, tau: Option<String>) -> LambdaEnvelope {
    LambdaEnvelope::new(
        c,
        tau.unwrap_or_else(now_iso),
        rho.to_string(),
        delta.to_string(),
    )
}

// ═══════════════════════════════════════════════════════════════════
//  BLAME CALCULUS — Findler-Felleisen CT-1/CT-2/CT-3 error taxonomy
// ═══════════════════════════════════════════════════════════════════

/// CT-1: the handler/runtime itself is broken (bug on Axon side).
pub const BLAME_CALLEE: &str = "CT-1";

/// CT-2: the Axon program made an invalid request (anchor breach, expired
/// lease, invalid manifest).
pub const BLAME_CALLER: &str = "CT-2";

/// CT-3: the physical world cannot answer (partition, quota, missing creds).
pub const BLAME_INFRASTRUCTURE: &str = "CT-3";

/// Every handler-emitted error — always carries a blame tag.
#[derive(Debug)]
pub struct HandlerError {
    pub message: String,
    pub blame: &'static str,
    pub kind: HandlerErrorKind,
    pub cause: Option<Box<dyn Error + Send + Sync + 'static>>,
}

/// The kind discriminant for a `HandlerError`. Lets callers match on a
/// specific failure mode without losing the message/cause chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerErrorKind {
    /// CT-1 — the handler implementation is broken. Always a bug.
    Callee,
    /// CT-2 — the Axon program made an invalid request.
    Caller,
    /// CT-3 — the physical world cannot answer.
    Infrastructure,
    /// Subtype of CT-3 — D4 partition = ⊥ void, NEVER `doubt`.
    NetworkPartition,
    /// Subtype of CT-2 — D2 τ expired = Anchor Breach.
    LeaseExpired,
    /// Subtype of CT-3 — backing SDK/binary missing.
    HandlerUnavailable,
}

impl HandlerError {
    pub fn callee(msg: impl Into<String>) -> Self {
        Self { message: msg.into(), blame: BLAME_CALLEE, kind: HandlerErrorKind::Callee, cause: None }
    }

    pub fn caller(msg: impl Into<String>) -> Self {
        Self { message: msg.into(), blame: BLAME_CALLER, kind: HandlerErrorKind::Caller, cause: None }
    }

    pub fn infrastructure(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            blame: BLAME_INFRASTRUCTURE,
            kind: HandlerErrorKind::Infrastructure,
            cause: None,
        }
    }

    pub fn network_partition(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            blame: BLAME_INFRASTRUCTURE,
            kind: HandlerErrorKind::NetworkPartition,
            cause: None,
        }
    }

    pub fn lease_expired(msg: impl Into<String>) -> Self {
        Self { message: msg.into(), blame: BLAME_CALLER, kind: HandlerErrorKind::LeaseExpired, cause: None }
    }

    pub fn handler_unavailable(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            blame: BLAME_INFRASTRUCTURE,
            kind: HandlerErrorKind::HandlerUnavailable,
            cause: None,
        }
    }

    pub fn with_cause(mut self, cause: impl Error + Send + Sync + 'static) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }
}

impl fmt::Display for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.blame, self.message)
    }
}

impl Error for HandlerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause.as_deref().map(|b| b as &(dyn Error + 'static))
    }
}

// ═══════════════════════════════════════════════════════════════════
//  HANDLER OUTCOME — the CPS return type
// ═══════════════════════════════════════════════════════════════════

/// Accepted outcome statuses.
pub const VALID_OUTCOME_STATUSES: &[&str] = &["ok", "partial", "failed"];

/// The result of β-reducing one Intention Tree node through a Handler.
///
/// Immutable so it can be shared across continuations safely.
#[derive(Debug, Clone, Serialize)]
pub struct HandlerOutcome {
    pub operation: String,
    pub target: String,
    pub status: String,
    pub envelope: LambdaEnvelope,
    pub data: serde_json::Map<String, Value>,
    pub handler: String,
}

impl HandlerOutcome {
    pub fn new(
        operation: impl Into<String>,
        target: impl Into<String>,
        status: impl Into<String>,
        envelope: LambdaEnvelope,
        handler: impl Into<String>,
    ) -> Self {
        let status = status.into();
        assert!(
            VALID_OUTCOME_STATUSES.contains(&status.as_str()),
            "HandlerOutcome.status must be one of {VALID_OUTCOME_STATUSES:?}; got '{status}'"
        );
        HandlerOutcome {
            operation: operation.into(),
            target: target.into(),
            status,
            envelope,
            data: serde_json::Map::new(),
            handler: handler.into(),
        }
    }

    pub fn with_data(mut self, data: serde_json::Map<String, Value>) -> Self {
        self.data = data;
        self
    }
}

// ═══════════════════════════════════════════════════════════════════
//  HANDLER INTERFACE — the abstract Free-Monad interpreter
// ═══════════════════════════════════════════════════════════════════

/// A CPS continuation: receives an outcome, returns (possibly transformed) outcome.
pub type Continuation<'a> = Box<dyn FnMut(HandlerOutcome) -> HandlerOutcome + 'a>;

/// Default continuation that passes outcomes through unchanged.
pub fn identity_continuation<'a>() -> Continuation<'a> {
    Box::new(|o| o)
}

/// Abstract interpreter of the Intention Tree (Free Monad F_Σ(X)).
///
/// Concrete implementors provide `provision` + `observe`; the default
/// `interpret` walks an `IRIntentionTree` and drives CPS evaluation
/// deterministically in declaration order.
pub trait Handler {
    /// Unique handler identifier (used by HandlerRegistry and provenance ρ).
    fn name(&self) -> &str;

    /// Return `true` iff this handler can interpret the given IR operation.
    fn supports(&self, op: &IRIntentionOperation) -> bool {
        matches!(op, IRIntentionOperation::Manifest(_) | IRIntentionOperation::Observe(_))
    }

    /// Materialize the resources listed in the manifest.
    fn provision(
        &mut self,
        manifest: &IRManifest,
        resources: &HashMap<String, IRResource>,
        fabrics: &HashMap<String, IRFabric>,
        continuation: &mut Continuation<'_>,
    ) -> Result<HandlerOutcome, HandlerError>;

    /// Take a quorum-gated snapshot of the manifest's real state.
    fn observe(
        &mut self,
        obs: &IRObserve,
        manifest: &IRManifest,
        continuation: &mut Continuation<'_>,
    ) -> Result<HandlerOutcome, HandlerError>;

    /// Release handler-level resources. MUST be idempotent.
    fn close(&mut self) {}

    /// β-reduce F_Σ(X) → X by walking the tree in declaration order.
    fn interpret(
        &mut self,
        tree: &IRIntentionTree,
        resources: &HashMap<String, IRResource>,
        fabrics: &HashMap<String, IRFabric>,
        manifests: &HashMap<String, IRManifest>,
    ) -> Result<Vec<HandlerOutcome>, HandlerError> {
        let mut outcomes: Vec<HandlerOutcome> = Vec::with_capacity(tree.operations.len());
        let mut pass_through: Continuation<'_> = identity_continuation();
        for op in &tree.operations {
            let outcome = match op {
                IRIntentionOperation::Manifest(m) => {
                    self.provision(m, resources, fabrics, &mut pass_through)?
                }
                IRIntentionOperation::Observe(o) => {
                    let target = manifests.get(&o.target).ok_or_else(|| {
                        HandlerError::caller(format!(
                            "observe '{}' targets unknown manifest '{}' — \
                             did you forget a declaration?",
                            o.name, o.target
                        ))
                    })?;
                    self.observe(o, target, &mut pass_through)?
                }
            };
            outcomes.push(outcome);
        }
        Ok(outcomes)
    }

    /// Convenience: extract tree + tables from an `IRProgram` and interpret.
    fn interpret_program(&mut self, program: &IRProgram) -> Result<Vec<HandlerOutcome>, HandlerError> {
        let Some(tree) = program.intention_tree.as_ref() else {
            return Ok(Vec::new());
        };
        let resources: HashMap<String, IRResource> = program
            .resources
            .iter()
            .map(|r| (r.name.clone(), r.clone()))
            .collect();
        let fabrics: HashMap<String, IRFabric> = program
            .fabrics
            .iter()
            .map(|f| (f.name.clone(), f.clone()))
            .collect();
        let manifests: HashMap<String, IRManifest> = program
            .manifests
            .iter()
            .map(|m| (m.name.clone(), m.clone()))
            .collect();
        self.interpret(tree, &resources, &fabrics, &manifests)
    }
}

// ═══════════════════════════════════════════════════════════════════
//  HANDLER REGISTRY — plugin registration & dispatch
// ═══════════════════════════════════════════════════════════════════

/// Keyed registry of available handlers. Used by the CLI/runtime to look
/// up a handler by name — the single dispatch point so that one `.axon`
/// program can run under multiple handlers without source changes.
pub struct HandlerRegistry {
    handlers: HashMap<String, Box<dyn Handler + Send>>,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        HandlerRegistry { handlers: HashMap::new() }
    }

    pub fn register(
        &mut self,
        handler: Box<dyn Handler + Send>,
        replace: bool,
    ) -> Result<(), HandlerError> {
        let name = handler.name().to_string();
        if self.handlers.contains_key(&name) && !replace {
            return Err(HandlerError::callee(format!(
                "handler '{name}' already registered; pass replace=true to override"
            )));
        }
        self.handlers.insert(name, handler);
        Ok(())
    }

    pub fn unregister(&mut self, name: &str) {
        if let Some(mut handler) = self.handlers.remove(name) {
            handler.close();
        }
    }

    pub fn get(&mut self, name: &str) -> Result<&mut (dyn Handler + Send), HandlerError> {
        let available = self.names().join(", ");
        match self.handlers.get_mut(name) {
            Some(h) => Ok(h.as_mut()),
            None => Err(HandlerError::caller(format!(
                "no handler registered with name '{name}'. Available: {}",
                if available.is_empty() { "(none)" } else { &available }
            ))),
        }
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.handlers.keys().cloned().collect();
        names.sort();
        names
    }

    pub fn contains(&self, name: &str) -> bool {
        self.handlers.contains_key(name)
    }

    pub fn close_all(&mut self) {
        for (_, mut handler) in self.handlers.drain() {
            handler.close();
        }
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for HandlerRegistry {
    fn drop(&mut self) {
        self.close_all();
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests mirror `tests/test_handlers_base.py` scope.
    use super::*;

    #[test]
    fn envelope_validates_certainty_range() {
        let e = LambdaEnvelope::new(0.5, "t".into(), "r".into(), "observed".into());
        assert_eq!(e.c, 0.5);
    }

    #[test]
    #[should_panic(expected = "must be in [0.0, 1.0]")]
    fn envelope_rejects_c_out_of_range() {
        LambdaEnvelope::new(1.1, "t".into(), "r".into(), "observed".into());
    }

    #[test]
    #[should_panic(expected = "delta must be one of")]
    fn envelope_rejects_invalid_delta() {
        LambdaEnvelope::new(1.0, "t".into(), "r".into(), "imagined".into());
    }

    #[test]
    fn envelope_decayed_preserves_tau_rho_delta() {
        let e = LambdaEnvelope::new(1.0, "T".into(), "R".into(), "observed".into());
        let d = e.decayed(0.0);
        assert_eq!(d.c, 0.0);
        assert_eq!(d.tau, "T");
        assert_eq!(d.rho, "R");
        assert_eq!(d.delta, "observed");
    }

    #[test]
    fn make_envelope_uses_supplied_or_current_tau() {
        let fixed = make_envelope(1.0, "h", "observed", Some("FIXED".into()));
        assert_eq!(fixed.tau, "FIXED");
        let fresh = make_envelope(1.0, "h", "observed", None);
        assert!(!fresh.tau.is_empty());
    }

    #[test]
    fn handler_error_display_includes_blame_tag() {
        let err = HandlerError::caller("oops");
        assert_eq!(format!("{err}"), "[CT-2] oops");
    }

    #[test]
    fn network_partition_is_ct3() {
        let e = HandlerError::network_partition("partition");
        assert_eq!(e.blame, BLAME_INFRASTRUCTURE);
        assert_eq!(e.kind, HandlerErrorKind::NetworkPartition);
    }

    #[test]
    fn lease_expired_is_ct2() {
        let e = HandlerError::lease_expired("expired");
        assert_eq!(e.blame, BLAME_CALLER);
        assert_eq!(e.kind, HandlerErrorKind::LeaseExpired);
    }

    #[test]
    fn outcome_rejects_invalid_status() {
        let env = LambdaEnvelope::new(1.0, "t".into(), "h".into(), "observed".into());
        let result = std::panic::catch_unwind(|| {
            HandlerOutcome::new("provision", "M", "weird", env, "h")
        });
        assert!(result.is_err());
    }

    struct DummyHandler {
        name: String,
        provisions: u32,
        observes: u32,
    }

    impl Handler for DummyHandler {
        fn name(&self) -> &str { &self.name }

        fn provision(
            &mut self,
            manifest: &IRManifest,
            _resources: &HashMap<String, IRResource>,
            _fabrics: &HashMap<String, IRFabric>,
            _cont: &mut Continuation<'_>,
        ) -> Result<HandlerOutcome, HandlerError> {
            self.provisions += 1;
            Ok(HandlerOutcome::new(
                "provision",
                manifest.name.clone(),
                "ok",
                make_envelope(1.0, &self.name, "observed", Some("T".into())),
                &self.name,
            ))
        }

        fn observe(
            &mut self,
            obs: &IRObserve,
            _manifest: &IRManifest,
            _cont: &mut Continuation<'_>,
        ) -> Result<HandlerOutcome, HandlerError> {
            self.observes += 1;
            Ok(HandlerOutcome::new(
                "observe",
                obs.name.clone(),
                "ok",
                make_envelope(0.94, &self.name, "observed", Some("T".into())),
                &self.name,
            ))
        }
    }

    fn fixture_program() -> IRProgram {
        use crate::ir_generator::IRGenerator;
        use crate::lexer::Lexer;
        use crate::parser::Parser;

        let source = r#"
            resource Db { kind: postgres lifetime: linear }
            fabric Vpc { provider: aws region: "us-east-1" zones: 1 }
            manifest Prod { resources: [Db] fabric: Vpc }
            observe Health from Prod { sources: [prom] quorum: 1 }
        "#;
        let tokens = Lexer::new(source, "h").tokenize().expect("lex ok");
        let program = Parser::new(tokens).parse().expect("parse ok");
        IRGenerator::new().generate(&program)
    }

    #[test]
    fn dummy_handler_interprets_intention_tree_in_order() {
        let program = fixture_program();
        assert!(program.intention_tree.is_some());
        let mut handler = DummyHandler { name: "dummy".into(), provisions: 0, observes: 0 };
        let outcomes = handler.interpret_program(&program).expect("interpret ok");
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].operation, "provision");
        assert_eq!(outcomes[1].operation, "observe");
        assert_eq!(handler.provisions, 1);
        assert_eq!(handler.observes, 1);
    }

    #[test]
    fn registry_register_then_get() {
        let mut reg = HandlerRegistry::new();
        reg.register(
            Box::new(DummyHandler { name: "dummy".into(), provisions: 0, observes: 0 }),
            false,
        )
        .expect("register ok");
        assert!(reg.contains("dummy"));
        assert_eq!(reg.names(), vec!["dummy".to_string()]);
        let h = reg.get("dummy").expect("get ok");
        assert_eq!(h.name(), "dummy");
    }

    #[test]
    fn registry_refuses_duplicate_without_replace() {
        let mut reg = HandlerRegistry::new();
        reg.register(
            Box::new(DummyHandler { name: "dup".into(), provisions: 0, observes: 0 }),
            false,
        )
        .unwrap();
        let err = reg
            .register(
                Box::new(DummyHandler { name: "dup".into(), provisions: 0, observes: 0 }),
                false,
            )
            .unwrap_err();
        assert_eq!(err.kind, HandlerErrorKind::Callee);
    }

    #[test]
    fn registry_allows_replace_when_flagged() {
        let mut reg = HandlerRegistry::new();
        reg.register(
            Box::new(DummyHandler { name: "r".into(), provisions: 0, observes: 0 }),
            false,
        )
        .unwrap();
        reg.register(
            Box::new(DummyHandler { name: "r".into(), provisions: 0, observes: 0 }),
            true,
        )
        .expect("replace ok");
    }

    #[test]
    fn registry_get_unknown_is_caller_blame() {
        let mut reg = HandlerRegistry::new();
        match reg.get("ghost") {
            Err(e) => assert_eq!(e.kind, HandlerErrorKind::Caller),
            Ok(_) => panic!("registry.get on unknown name must error"),
        }
    }
}
