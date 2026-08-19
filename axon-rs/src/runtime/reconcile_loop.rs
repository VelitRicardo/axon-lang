//! AXON Runtime — ReconcileLoop (v1.1.0)
//!
//! Direct port of `axon/runtime/reconcile_loop.py`.
//!
//! Free-energy-minimizing control loop per tick:
//!   1. OBSERVE — `handler.observe(obs, manifest)` → HandlerOutcome.
//!   2. MEASURE — symmetric Jaccard distance on resource-name sets (drift).
//!   3. GATE    — skip unless certainty > threshold AND drift > tolerance AND shield approves.
//!   4. ACT     — apply `on_drift ∈ {provision, alert, refine}`.
//!   5. BOUND   — cap at `max_retries`.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use crate::handlers::base::{
    Continuation, Handler, HandlerError, HandlerOutcome, identity_continuation, make_envelope,
};
use crate::ir_nodes::{
    IRFabric, IRManifest, IRObserve, IRProgram, IRReconcile, IRResource,
};

// ═══════════════════════════════════════════════════════════════════
//  REPORTS — one per tick
// ═══════════════════════════════════════════════════════════════════

/// One iteration of the control loop — fully serialisable and immutable.
#[derive(Debug, Clone)]
pub struct ReconcileTickReport {
    pub reconcile_name: String,
    pub observation: Option<HandlerOutcome>,
    pub action: TickAction,
    pub drift: f64,
    pub certainty: f64,
    pub shield_approved: bool,
    pub retries_remaining: i64,
    pub outcome: Option<HandlerOutcome>,
    pub note: String,
}

/// Per-tick action outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickAction {
    Provision,
    Alert,
    Refine,
    Noop,
}

impl TickAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            TickAction::Provision => "provision",
            TickAction::Alert => "alert",
            TickAction::Refine => "refine",
            TickAction::Noop => "noop",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
//  SHIELD ADAPTER — governance gate
// ═══════════════════════════════════════════════════════════════════

/// Shield adapter: `(reconcile_name, observation, drift) → approved`.
///
/// Real shields plug into this contract once runtime wiring for
/// `ShieldApplyNode` lands (v1.1.0+). Fake adapters simulate deny/approve
/// in tests.
pub type ShieldApprove = Box<dyn Fn(&str, &HandlerOutcome, f64) -> bool>;

pub fn allow_all_shield() -> ShieldApprove {
    Box::new(|_name, _obs, _drift| true)
}

pub fn deny_all_shield() -> ShieldApprove {
    Box::new(|_name, _obs, _drift| false)
}

// ═══════════════════════════════════════════════════════════════════
//  DRIFT METRIC — free-energy proxy
// ═══════════════════════════════════════════════════════════════════

/// Symmetric Jaccard distance on resource-name sets.
///
/// `|A △ B| / |A ∪ B|` — zero when belief and evidence agree, approaches
/// 1.0 as they diverge. A principled D_KL proxy for unstructured resource
/// inventories without priors.
pub fn jaccard_drift(expected: &[String], observed: &[String]) -> f64 {
    let a: HashSet<&String> = expected.iter().collect();
    let b: HashSet<&String> = observed.iter().collect();
    let union_len = a.union(&b).count();
    if union_len == 0 {
        return 0.0;
    }
    let sym_diff_len = a.symmetric_difference(&b).count();
    (sym_diff_len as f64) / (union_len as f64)
}

// ═══════════════════════════════════════════════════════════════════
//  RECONCILE LOOP
// ═══════════════════════════════════════════════════════════════════

/// Executes `tick()` / `run()` against a Handler to close the belief-evidence gap.
pub struct ReconcileLoop<'p, H: Handler> {
    ir: IRReconcile,
    handler: H,
    shield: ShieldApprove,
    threshold: f64,
    tolerance: f64,
    retries_left: i64,
    ticks: Vec<ReconcileTickReport>,
    observe: IRObserve,
    manifest: IRManifest,
    resources: HashMap<String, IRResource>,
    fabrics: HashMap<String, IRFabric>,
    _phantom: std::marker::PhantomData<&'p ()>,
}

impl<'p, H: Handler> ReconcileLoop<'p, H> {
    pub fn new(
        ir_reconcile: IRReconcile,
        program: &IRProgram,
        handler: H,
    ) -> Result<Self, HandlerError> {
        Self::with_shield(ir_reconcile, program, handler, allow_all_shield())
    }

    pub fn with_shield(
        ir_reconcile: IRReconcile,
        program: &IRProgram,
        handler: H,
        shield: ShieldApprove,
    ) -> Result<Self, HandlerError> {
        let observe = program
            .observations
            .iter()
            .find(|o| o.name == ir_reconcile.observe_ref)
            .cloned()
            .ok_or_else(|| {
                HandlerError::caller(format!(
                    "reconcile '{}' references unknown observe '{}'",
                    ir_reconcile.name, ir_reconcile.observe_ref
                ))
            })?;
        let manifest_name = observe.target.clone();
        let manifest = program
            .manifests
            .iter()
            .find(|m| m.name == manifest_name)
            .cloned()
            .ok_or_else(|| {
                HandlerError::caller(format!(
                    "reconcile '{}': observe '{}' targets unknown manifest '{}'",
                    ir_reconcile.name, ir_reconcile.observe_ref, manifest_name
                ))
            })?;
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
        let threshold = ir_reconcile.threshold.unwrap_or(0.85);
        let tolerance = ir_reconcile.tolerance.unwrap_or(0.10);
        let retries_left = ir_reconcile.max_retries;
        Ok(ReconcileLoop {
            ir: ir_reconcile,
            handler,
            shield,
            threshold,
            tolerance,
            retries_left,
            ticks: Vec::new(),
            observe,
            manifest,
            resources,
            fabrics,
            _phantom: std::marker::PhantomData,
        })
    }

    /// One control-loop tick. Returns the report capturing all gating
    /// decisions and any emitted HandlerOutcome.
    pub fn tick(&mut self) -> Result<ReconcileTickReport, HandlerError> {
        if self.retries_left <= 0 {
            let report = ReconcileTickReport {
                reconcile_name: self.ir.name.clone(),
                observation: None,
                action: TickAction::Noop,
                drift: 0.0,
                certainty: 0.0,
                shield_approved: false,
                retries_remaining: self.retries_left,
                outcome: None,
                note: "max_retries exhausted".into(),
            };
            self.ticks.push(report.clone());
            return Ok(report);
        }

        let mut pass: Continuation<'_> = identity_continuation();
        let observation = self.handler.observe(&self.observe, &self.manifest, &mut pass)?;

        // v2.67.0 — **the belief-vs-evidence gap, restored.**
        //
        // This block used to read:
        //
        // ```ignore
        // // If the handler's observation payload includes a `resources_observed`
        // // string array, use it as evidence; OTHERWISE DEFAULT TO BELIEF.
        // _ => self.manifest.resources.clone(),
        // ```
        //
        // When the evidence was missing, the "observed" state became **the
        // manifest's own declaration** — so
        //
        // ```text
        //   drift = jaccard(manifest.resources, manifest.resources) = 0.0
        // ```
        //
        // **Drift was structurally always zero.** A reconciliation loop whose entire
        // purpose is closing the gap between belief and evidence *closed it by
        // assuming there was no gap* — it compared the belief against itself. And
        // the only handler that existed filled `resources_observed` from
        // `manifest.resources` too, which is also the belief, so **both paths gave
        // zero and `reconcile` could never detect anything.**
        //
        // It is the same defect as `DryRunHandler`'s `c: 1.0`, in a third place:
        // *when the evidence is missing, substitute the belief and report agreement.*
        //
        // Now: **no evidence ⇒ REFUSE.** We cannot compute a drift we did not
        // measure, and reporting `0.0` for it is not a conservative default — it is
        // a claim that the world matches your intent, made without looking.
        let Some(serde_json::Value::Array(arr)) = observation.data.get("resources_observed") else {
            return Err(HandlerError::callee(format!(
                "reconcile '{}': the observation carries no `resources_observed` evidence, so the \
                 drift between the manifest's DESIRED shape and the world's ACTUAL shape cannot be \
                 computed. Refusing — reporting zero drift here would be a claim that reality \
                 matches your intent, made without looking",
                self.ir.name
            )));
        };
        let observed: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        let drift = jaccard_drift(&self.manifest.resources, &observed);
        let certainty = observation.envelope.c;

        if certainty < self.threshold {
            return Ok(self.record(
                Some(observation),
                TickAction::Noop,
                drift,
                certainty,
                false,
                format!(
                    "certainty {certainty:.2} below threshold {:.2}",
                    self.threshold
                ),
            ));
        }
        if drift <= self.tolerance {
            return Ok(self.record(
                Some(observation),
                TickAction::Noop,
                drift,
                certainty,
                true,
                format!("drift {drift:.3} within tolerance {:.3}", self.tolerance),
            ));
        }
        let approved = (self.shield)(&self.ir.name, &observation, drift);
        if !approved {
            return Ok(self.record(
                Some(observation),
                TickAction::Noop,
                drift,
                certainty,
                false,
                "shield denied corrective action".into(),
            ));
        }

        let outcome = self.apply_action(&observation, drift, certainty)?;
        self.retries_left -= 1;
        let action = match self.ir.on_drift.as_str() {
            "provision" => TickAction::Provision,
            "alert" => TickAction::Alert,
            _ => TickAction::Refine,
        };
        let report = ReconcileTickReport {
            reconcile_name: self.ir.name.clone(),
            observation: Some(observation),
            action,
            drift,
            certainty,
            shield_approved: true,
            retries_remaining: self.retries_left,
            outcome: Some(outcome),
            note: format!(
                "drift {drift:.3} > tolerance {:.3}; applied {}",
                self.tolerance,
                action.as_str()
            ),
        };
        self.ticks.push(report.clone());
        Ok(report)
    }

    /// Tick until quiescence (two consecutive noops), budget exhaustion,
    /// or the supplied `max_ticks` (default = `max_retries + 2`).
    pub fn run(&mut self, max_ticks: Option<u32>) -> Result<Vec<ReconcileTickReport>, HandlerError> {
        let limit = max_ticks.unwrap_or((self.ir.max_retries + 2) as u32);
        let mut results = Vec::new();
        let mut consecutive_noops = 0;
        for _ in 0..limit {
            let report = self.tick()?;
            let is_noop = report.action == TickAction::Noop;
            let exhausted = report.note.contains("exhausted");
            results.push(report);
            if is_noop {
                consecutive_noops += 1;
                if consecutive_noops >= 2 || exhausted {
                    break;
                }
            } else {
                consecutive_noops = 0;
            }
        }
        Ok(results)
    }

    pub fn history(&self) -> &[ReconcileTickReport] {
        &self.ticks
    }

    fn apply_action(
        &mut self,
        observation: &HandlerOutcome,
        drift: f64,
        certainty: f64,
    ) -> Result<HandlerOutcome, HandlerError> {
        match self.ir.on_drift.as_str() {
            "provision" => {
                let mut pass = identity_continuation();
                self.handler
                    .provision(&self.manifest, &self.resources, &self.fabrics, &mut pass)
            }
            "alert" => {
                let mut data = serde_json::Map::new();
                data.insert("reconcile".into(), self.ir.name.clone().into());
                data.insert("drift".into(), serde_json::Value::from(drift));
                data.insert(
                    "source_observation".into(),
                    observation.target.clone().into(),
                );
                Ok(HandlerOutcome::new(
                    "alert",
                    self.ir.name.clone(),
                    "ok",
                    make_envelope(certainty, "reconcile", "inferred", None),
                    format!("reconcile:{}", self.ir.name),
                )
                .with_data(data))
            }
            _ => {
                // refine — belief-revision placeholder (v1.1.0+).
                let mut data = serde_json::Map::new();
                data.insert("reconcile".into(), self.ir.name.clone().into());
                data.insert("drift".into(), serde_json::Value::from(drift));
                data.insert(
                    "note".into(),
                    "belief revision reserved for psyche integration".into(),
                );
                Ok(HandlerOutcome::new(
                    "refine",
                    self.ir.name.clone(),
                    "partial",
                    make_envelope(certainty, "reconcile", "inferred", None),
                    format!("reconcile:{}", self.ir.name),
                )
                .with_data(data))
            }
        }
    }

    fn record(
        &mut self,
        observation: Option<HandlerOutcome>,
        action: TickAction,
        drift: f64,
        certainty: f64,
        shield_approved: bool,
        note: String,
    ) -> ReconcileTickReport {
        let report = ReconcileTickReport {
            reconcile_name: self.ir.name.clone(),
            observation,
            action,
            drift,
            certainty,
            shield_approved,
            retries_remaining: self.retries_left,
            outcome: None,
            note,
        };
        self.ticks.push(report.clone());
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::base::{LambdaEnvelope, make_envelope};

    // ── Scripted handler: returns fixed observations and records provisions ──

    struct ScriptedHandler {
        observation_certainty: f64,
        observed_resources: Vec<String>,
        provisions: u32,
    }

    impl Handler for ScriptedHandler {
        fn name(&self) -> &str { "scripted" }

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
                make_envelope(1.0, "scripted", "observed", Some("T".into())),
                "scripted",
            ))
        }

        fn observe(
            &mut self,
            obs: &IRObserve,
            _manifest: &IRManifest,
            _cont: &mut Continuation<'_>,
        ) -> Result<HandlerOutcome, HandlerError> {
            let env = LambdaEnvelope::new(
                self.observation_certainty,
                "T".into(),
                "scripted".into(),
                "observed".into(),
            );
            let mut data = serde_json::Map::new();
            data.insert(
                "resources_observed".into(),
                serde_json::Value::Array(
                    self.observed_resources
                        .iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
                ),
            );
            Ok(HandlerOutcome::new("observe", obs.name.clone(), "ok", env, "scripted")
                .with_data(data))
        }
    }

    fn program_with_resources(resources: &[&str]) -> IRProgram {
        use crate::ir_generator::IRGenerator;
        use crate::lexer::Lexer;
        use crate::parser::Parser;
        // Use the compile pipeline to build a realistic IRProgram, then
        // manipulate the compiled manifest / observe in place for the tests.
        let base = format!(
            r#"
            resource Db {{ kind: postgres lifetime: linear }}
            fabric Vpc {{ provider: aws region: "us-east-1" zones: 1 }}
            manifest Prod {{ resources: [{}] fabric: Vpc }}
            observe Health from Prod {{ sources: [prom] quorum: 1 }}
            reconcile R {{
                observe: Health
                threshold: 0.5
                tolerance: 0.1
                on_drift: provision
                max_retries: 3
            }}"#,
            resources.join(", ")
        );
        // The .axon fixture doesn't actually declare every resource in the
        // `resources:` list as a top-level `resource X {...}` — the type
        // checker runs in its own pass and would reject undefined refs.
        // We bypass that here by directly mutating the parsed IR manifest.
        let real = r#"
            resource Db { kind: postgres lifetime: linear }
            fabric Vpc { provider: aws region: "us-east-1" zones: 1 }
            manifest Prod { resources: [Db] fabric: Vpc }
            observe Health from Prod { sources: [prom] quorum: 1 }
            reconcile R {
                observe: Health
                threshold: 0.5
                tolerance: 0.1
                on_drift: provision
                max_retries: 3
            }
        "#;
        let _ = base;
        let tokens = Lexer::new(real, "r").tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut ir = IRGenerator::new().generate(&program);
        // Replace the manifest's resources list with the test's intended one.
        ir.manifests[0].resources = resources.iter().map(|s| s.to_string()).collect();
        ir
    }

    #[test]
    fn jaccard_drift_edges() {
        assert_eq!(jaccard_drift(&[], &[]), 0.0);
        assert_eq!(
            jaccard_drift(&["a".into(), "b".into()], &["a".into(), "b".into()]),
            0.0
        );
        // Disjoint: |sym_diff| = 4, |union| = 4 → 1.0.
        assert_eq!(
            jaccard_drift(&["a".into(), "b".into()], &["c".into(), "d".into()]),
            1.0
        );
        // {a,b} vs {b,c}: sym=2, union=3 → 2/3
        let d = jaccard_drift(&["a".into(), "b".into()], &["b".into(), "c".into()]);
        assert!((d - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn tick_noops_when_certainty_below_threshold() {
        let ir = program_with_resources(&["Db"]);
        let reconcile = ir.reconciles[0].clone();
        let handler = ScriptedHandler {
            observation_certainty: 0.3, // below threshold 0.5
            observed_resources: vec!["Db".into()],
            provisions: 0,
        };
        let mut loop_ = ReconcileLoop::new(reconcile, &ir, handler).unwrap();
        let r = loop_.tick().unwrap();
        assert_eq!(r.action, TickAction::Noop);
        assert!(r.note.contains("below threshold"));
    }

    #[test]
    fn tick_noops_when_drift_within_tolerance() {
        let ir = program_with_resources(&["Db"]);
        let reconcile = ir.reconciles[0].clone();
        let handler = ScriptedHandler {
            observation_certainty: 1.0,
            observed_resources: vec!["Db".into()], // zero drift
            provisions: 0,
        };
        let mut loop_ = ReconcileLoop::new(reconcile, &ir, handler).unwrap();
        let r = loop_.tick().unwrap();
        assert_eq!(r.action, TickAction::Noop);
        assert!(r.note.contains("within tolerance"));
    }

    #[test]
    fn tick_triggers_provision_on_drift_above_tolerance() {
        let ir = program_with_resources(&["Db"]);
        let reconcile = ir.reconciles[0].clone();
        let handler = ScriptedHandler {
            observation_certainty: 1.0,
            observed_resources: vec![], // drift = 1.0
            provisions: 0,
        };
        let mut loop_ = ReconcileLoop::new(reconcile, &ir, handler).unwrap();
        let r = loop_.tick().unwrap();
        assert_eq!(r.action, TickAction::Provision);
        assert_eq!(r.shield_approved, true);
        assert!(r.outcome.is_some());
    }

    #[test]
    fn tick_noops_when_shield_denies() {
        let ir = program_with_resources(&["Db"]);
        let reconcile = ir.reconciles[0].clone();
        let handler = ScriptedHandler {
            observation_certainty: 1.0,
            observed_resources: vec![],
            provisions: 0,
        };
        let mut loop_ =
            ReconcileLoop::with_shield(reconcile, &ir, handler, deny_all_shield()).unwrap();
        let r = loop_.tick().unwrap();
        assert_eq!(r.action, TickAction::Noop);
        assert!(r.note.contains("shield denied"));
    }

    #[test]
    fn run_respects_max_retries_budget() {
        let ir = program_with_resources(&["Db"]);
        let reconcile = ir.reconciles[0].clone();
        let max = reconcile.max_retries;
        let handler = ScriptedHandler {
            observation_certainty: 1.0,
            observed_resources: vec![], // drift = 1.0 every tick
            provisions: 0,
        };
        let mut loop_ = ReconcileLoop::new(reconcile, &ir, handler).unwrap();
        let reports = loop_.run(Some(20)).unwrap();
        let provisions = reports
            .iter()
            .filter(|r| r.action == TickAction::Provision)
            .count();
        // Budget caps provisions at max_retries.
        assert_eq!(provisions as i64, max);
    }

    #[test]
    fn reconcile_with_undefined_observe_is_caller_error() {
        let ir = program_with_resources(&["Db"]);
        let mut bad = ir.reconciles[0].clone();
        bad.observe_ref = "Ghost".into();
        let handler = ScriptedHandler {
            observation_certainty: 1.0,
            observed_resources: vec![],
            provisions: 0,
        };
        match ReconcileLoop::new(bad, &ir, handler) {
            Err(e) => assert_eq!(e.blame, "CT-2"),
            Ok(_) => panic!("undefined observe must fail to construct the loop"),
        }
    }
}
