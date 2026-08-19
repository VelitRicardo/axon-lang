//! v2.67.0 — **the Cognitive-I/O Supervisor: the loop that was never built.**
//!
//! # What was missing
//!
//! v2.67.0 found the nine λ-L-E Cognitive-I/O primitives (`observe` · `reconcile` ·
//! `lease` · `ensemble` · `immune` · `reflex` · `heal` · `resource` · `fabric`)
//! **unreachable**: declared, type-checked, carried into the IR, and consumed by
//! nothing. I first called that a *language-design* problem. It was not.
//!
//! - **The language is complete.** The declarations already reference each other —
//!   `immune.watch: [<observe>]`, `reflex.trigger: <immune>`, `heal.source:
//!   <immune>`, `reconcile.observe: <observe>`, `ensemble.observations:
//!   [<observe>]`. They are not seven orphans; they are **one declarative dataflow
//!   graph, already wired.**
//! - **The kernels are complete**, and were built for exactly this:
//!   `AnomalyDetector::new(ir: IRImmune)`, `EnsembleAggregator::new(ir: IREnsemble)`,
//!   `ReconcileLoop::new(ir, program, handler)`. They take the compiled IR
//!   **directly**.
//!
//! **Nobody ever built the loop that hands them the graph and drives it.**
//! `DaemonSupervisor` does exactly this job — for daemons. This family had no
//! counterpart. That is all this module is.
//!
//! # The graph it drives
//!
//! ```text
//!   manifest ──► observe ──┬──► reconcile      (observe → Jaccard drift → shield → on_drift)
//!                          ├──► ensemble       (Byzantine quorum + Cφ fusion)
//!                          └──► immune ──┬──► reflex   (trigger: <immune>)
//!                                        └──► heal     (source:  <immune>)
//! ```
//!
//! One `tick()` walks it in dependency order: observe → {ensemble, immune} →
//! {reflex, heal}, and ticks each `reconcile` (which drives its own observe through
//! the same [`Handler`]).
//!
//! # The law it inherits
//!
//! An `observe` that cannot be taken **refuses** (v2.67.0). The supervisor does not
//! soften that. A refusal is recorded as a refusal and **does not feed the
//! downstream kernels** — an `immune` must never learn a baseline from an
//! observation nobody actually took, and a `reflex` must never fire (or fail to
//! fire) on a health report synthesised from silence.
//!
//! `lease` is **not** driven here. It cannot work: the CT-2 Anchor Breach the
//! README promises is *breach on post-expiry **use***, and **a flow can never *use*
//! a `resource`** (v2.67.0's islands finding). Wiring `LeaseKernel` before **v2.67.0**
//! gives the resource a use-site would be shipping a primitive whose headline
//! guarantee is structurally impossible.

use std::collections::HashMap;

use crate::handlers::base::{identity_continuation, Handler, HandlerOutcome};
use crate::handlers::live::LiveHandler;
use crate::ir_nodes::IRProgram;
use crate::runtime::ensemble_aggregator::{EnsembleAggregator, EnsembleReport};
use crate::runtime::immune::detector::AnomalyDetector;
use crate::runtime::immune::health_report::HealthReport;
use crate::runtime::immune::heal::{HealDecision, HealKernel};
use crate::runtime::immune::reflex::{ReflexEngine, ReflexOutcome};
use crate::runtime::reconcile_loop::{ReconcileLoop, ReconcileTickReport};

/// What one pass over the declared graph actually did.
///
/// Every field is a **record of what happened**, not a summary of what was
/// intended. `observation_refusals` is first-class and never empty-by-omission: a
/// supervisor that quietly dropped its refusals would recreate, at a higher level,
/// exactly the `DryRunHandler` defect v2.67.0 removed.
#[derive(Debug, Default)]
pub struct SupervisorTick {
    /// `observe` name → the outcome, for the observations that were actually taken.
    pub observations: HashMap<String, HandlerOutcome>,
    /// `observe` name → why it could not be taken. **An entry here is a system we
    /// could not see — not a system that is fine.**
    pub observation_refusals: HashMap<String, String>,
    /// `ensemble` name → its Byzantine-quorum report.
    pub ensembles: HashMap<String, EnsembleReport>,
    /// `immune` name → the health report its KL-divergence sensor produced.
    ///
    /// **Absent while the immune is still LEARNING its baseline** — see `learning`.
    /// There is no such thing as an anomaly before there is a baseline to be
    /// anomalous with respect to.
    pub health: HashMap<String, HealthReport>,
    /// `immune` name → `(samples_learned, window)` while its baseline is still
    /// being estimated.
    ///
    /// First-class, like `observation_refusals`: **"I am still learning" is a fact
    /// the operator must be able to see.** An immune system that silently reported
    /// nothing during its learning phase would be indistinguishable from one that
    /// is watching and finding nothing wrong — and those are very different
    /// statements about your infrastructure.
    pub learning: HashMap<String, (usize, usize)>,
    /// Reflexes that fired (HMAC-signed traces, idempotency-gated).
    pub reflexes: Vec<ReflexOutcome>,
    /// Heal decisions (audit_only / human_in_loop / adversarial).
    pub heals: Vec<HealDecision>,
    /// `reconcile` name → its tick report (drift, shield verdict, action taken).
    pub reconciles: HashMap<String, ReconcileTickReport>,
}

impl SupervisorTick {
    /// An auditor-legible summary of what this pass actually did.
    ///
    /// `refusals` is emitted **even when empty**, deliberately: a report that only
    /// showed you what it managed to see would be the `DryRunHandler` defect in
    /// report form. The count of things we could **not** observe is a first-class
    /// fact about the health of the system, not an omission.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "observations": self
                .observations
                .iter()
                .map(|(k, o)| {
                    (
                        k.clone(),
                        serde_json::json!({ "status": o.status, "certainty": o.envelope.c }),
                    )
                })
                .collect::<serde_json::Map<_, _>>(),
            // A system we could not see is NOT a system that is fine.
            "refusals": self
                .observation_refusals
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect::<serde_json::Map<_, _>>(),
            "health": self
                .health
                .iter()
                .map(|(k, h)| {
                    (
                        k.clone(),
                        serde_json::json!({
                            "kl_divergence": h.kl_divergence,
                            "free_energy": h.free_energy,
                            "classification": h.classification,
                            "certainty": h.envelope.c,
                        }),
                    )
                })
                .collect::<serde_json::Map<_, _>>(),
            // "I am still learning" ≠ "I am watching and all is well".
            "learning": self
                .learning
                .iter()
                .map(|(k, (have, want))| {
                    (
                        k.clone(),
                        serde_json::json!({ "samples": have, "window": want }),
                    )
                })
                .collect::<serde_json::Map<_, _>>(),
            // `fired` is the truth; the rest were EVALUATED and declined (below the
            // declared on_level, or idempotency-skipped for a signature already
            // acted on). Counting evaluations as firings would overstate what the
            // immune system actually did — the same class of overclaim v2.67.0 spent
            // itself removing.
            "reflexes_fired": self.reflexes.iter().filter(|r| r.fired).count(),
            "reflexes": self
                .reflexes
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "reflex": r.reflex_name,
                        "action": r.action,
                        "fired": r.fired,
                        "reason": r.reason,
                        "latency_us": r.latency_us,
                        // The HMAC-signed trace: a reflex's firing is attestable,
                        // not merely logged.
                        "signed_trace": r.signed_trace,
                    })
                })
                .collect::<Vec<_>>(),
            "heal_decisions": self
                .heals
                .iter()
                .map(|d| {
                    serde_json::json!({
                        "outcome": format!("{:?}", d.outcome),
                        "reason": d.reason,
                    })
                })
                .collect::<Vec<_>>(),
            "ensembles": self.ensembles.len(),
            "reconciles": self
                .reconciles
                .iter()
                .map(|(k, r)| {
                    (
                        k.clone(),
                        serde_json::json!({
                            "action": r.action.as_str(),
                            "drift": r.drift,
                            "certainty": r.certainty,
                            "shield_approved": r.shield_approved,
                        }),
                    )
                })
                .collect::<serde_json::Map<_, _>>(),
        })
    }
}

/// The supervisor. Built from a compiled program, driven by `tick()`.
pub struct CognitiveIoSupervisor {
    program: IRProgram,
    handler: LiveHandler,
    ensembles: HashMap<String, EnsembleAggregator>,
    detectors: HashMap<String, AnomalyDetector>,
    reflexes: ReflexEngine,
    heals: HealKernel,
}

impl CognitiveIoSupervisor {
    /// Instantiate the declared graph.
    ///
    /// This is the constructor that never existed. Everything it needs was already
    /// in the IR; nothing had ever read it.
    pub fn from_ir(program: &IRProgram) -> Result<Self, String> {
        let resources = program
            .resources
            .iter()
            .map(|r| (r.name.clone(), r.clone()))
            .collect();

        let mut ensembles = HashMap::new();
        for e in &program.ensembles {
            let agg = EnsembleAggregator::new(e.clone())
                .map_err(|err| format!("ensemble '{}': {}", e.name, err.message))?;
            ensembles.insert(e.name.clone(), agg);
        }

        let detectors = program
            .immunes
            .iter()
            .map(|i| (i.name.clone(), AnomalyDetector::new(i.clone())))
            .collect();

        let mut reflexes = ReflexEngine::new();
        for r in &program.reflexes {
            reflexes
                .register(r.clone())
                .map_err(|err| format!("reflex '{}': {}", r.name, err.message))?;
        }

        let mut heals = HealKernel::new();
        for h in &program.heals {
            heals.register(h.clone());
        }

        Ok(CognitiveIoSupervisor {
            program: program.clone(),
            handler: LiveHandler::new(resources),
            ensembles,
            detectors,
            reflexes,
            heals,
        })
    }

    /// Is there anything to supervise? A program with no Cognitive-I/O declarations
    /// gets no supervisor at all (and pays nothing for it).
    pub fn is_empty(&self) -> bool {
        self.program.observations.is_empty()
            && self.program.ensembles.is_empty()
            && self.program.immunes.is_empty()
            && self.program.reconciles.is_empty()
    }

    /// Walk the declared graph once.
    ///
    /// Dependency order: **observe → {ensemble, immune} → {reflex, heal}**, then
    /// each `reconcile` (which drives its own observe through the same handler).
    pub fn tick(&mut self) -> SupervisorTick {
        let mut out = SupervisorTick::default();

        // ── 1. observe ────────────────────────────────────────────────────
        //
        // A refusal is recorded AS a refusal. It does not become an observation
        // with low confidence, and it does not feed anything downstream. The
        // difference between "I looked and it's fine" and "I could not look" is
        // the entire reason this primitive exists (v2.67.0).
        let manifests: HashMap<&str, _> = self
            .program
            .manifests
            .iter()
            .map(|m| (m.name.as_str(), m))
            .collect();

        for obs in &self.program.observations {
            let Some(manifest) = manifests.get(obs.target.as_str()) else {
                out.observation_refusals.insert(
                    obs.name.clone(),
                    format!(
                        "observe '{}' targets manifest '{}', which is not declared",
                        obs.name, obs.target
                    ),
                );
                continue;
            };
            let mut cont = identity_continuation();
            match self.handler.observe(obs, manifest, &mut cont) {
                Ok(outcome) => {
                    out.observations.insert(obs.name.clone(), outcome);
                }
                Err(e) => {
                    out.observation_refusals
                        .insert(obs.name.clone(), e.message.clone());
                }
            }
        }

        // ── 2. ensemble — Byzantine quorum over the observations it names ──
        for e in &self.program.ensembles {
            let Some(agg) = self.ensembles.get(&e.name) else {
                continue;
            };
            // Only observations we ACTUALLY took are eligible. A refused observe is
            // absent, not "failed with low confidence" — which is what lets the
            // aggregator's own quorum gate do its job honestly.
            let inputs: Vec<HandlerOutcome> = e
                .observations
                .iter()
                .filter_map(|name| out.observations.get(name).cloned())
                .collect();
            if let Ok((_outcome, report)) = agg.aggregate(&inputs) {
                out.ensembles.insert(e.name.clone(), report);
            }
        }

        // ── 3. immune — the KL-divergence sensor over what it watches ──────
        //
        // ## The baseline lifecycle (v2.67.0)
        //
        // `immune.baseline: learned` is the language's default, and `window:` is —
        // in the AST's own words — "samples used to estimate baseline". **The
        // baseline is LEARNED.** Until it is, there is nothing to deviate *from*.
        //
        // A detector classifying against an EMPTY baseline sees every symbol as
        // novel, reports a high KL divergence, and fires every reflex — on a
        // perfectly healthy system, on the very first tick.
        //
        // > A monitor that cries wolf from the first tick is as useless as one
        // > that never cries.
        //
        // So the supervisor runs each immune through two phases:
        //
        //   LEARNING   (baseline.size() < window) — samples TRAIN the baseline.
        //                                           No health report is emitted,
        //                                           and therefore NO reflex and NO
        //                                           heal can fire. There is nothing
        //                                           to be anomalous with respect to.
        //   WATCHING   (baseline is full)         — samples are classified against
        //                                           it (per-window: `classify_batch`
        //                                           resets `current` each tick, so a
        //                                           tick's verdict is about THAT
        //                                           tick).
        for imm in &self.program.immunes {
            let Some(detector) = self.detectors.get_mut(&imm.name) else {
                continue;
            };
            // The sample is the signature of what the watched observations
            // reported. An immune watching an observation we could not take gets
            // NOTHING — it must never learn a baseline from silence, and it must
            // never classify against one.
            let samples: Vec<String> = imm
                .watch
                .iter()
                .filter_map(|w| out.observations.get(w))
                .map(|o| format!("{}:{}", o.status, o.envelope.c))
                .collect();
            if samples.is_empty() {
                continue;
            }

            let window = imm.window.max(1) as usize;
            if detector.baseline.size() < window {
                detector.train(samples);
                out.learning.insert(
                    imm.name.clone(),
                    (detector.baseline.size(), window),
                );
                // Deliberately NO health entry: a reflex fired during the learning
                // phase is a false positive BY CONSTRUCTION.
                continue;
            }

            let report = detector.classify_batch(samples);
            out.health.insert(imm.name.clone(), report);
        }

        // ── 4. reflex + heal — the motor responses, on a REAL health report ──
        for report in out.health.values() {
            out.reflexes.extend(self.reflexes.dispatch(report));
            out.heals.extend(self.heals.tick(report));
        }

        // ── 5. reconcile — drives its own observe through the same handler ──
        for rec in &self.program.reconciles {
            let mut loop_ = match ReconcileLoop::new(
                rec.clone(),
                &self.program,
                LiveHandler::new(
                    self.program
                        .resources
                        .iter()
                        .map(|r| (r.name.clone(), r.clone()))
                        .collect(),
                ),
            ) {
                Ok(l) => l,
                Err(_) => continue,
            };
            if let Ok(report) = loop_.tick() {
                out.reconciles.insert(rec.name.clone(), report);
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir_generator::IRGenerator;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::source_registry::{register_source_adapter, SourceAdapter, SourceError, SourceReading};
    use std::sync::Arc;

    fn compile(src: &str) -> IRProgram {
        let tokens = Lexer::new(src, "t").tokenize().expect("lex");
        let prog = Parser::new(tokens).parse().expect("parse");
        IRGenerator::new().generate(&prog)
    }

    struct Fixed(String, f64);
    impl SourceAdapter for Fixed {
        fn name(&self) -> &str {
            &self.0
        }
        fn probe(
            &self,
            _r: Option<&crate::ir_nodes::IRResource>,
            _t: std::time::Duration,
        ) -> Result<SourceReading, SourceError> {
            Ok(SourceReading::new(self.1, serde_json::Map::new()))
        }
    }

    const PROGRAM: &str = r#"
resource Db { kind: postgres  endpoint: db.main  lifetime: affine }
fabric  Vpc { provider: aws }
manifest Infra { resources: [Db]  fabric: Vpc }
observe Health from Infra { sources: [sv_probe]  quorum: 1  timeout: 1s  on_partition: fail }
immune  Sentinel { watch: [Health]  scope: tenant  window: 8 }
reflex  Quarantine { trigger: Sentinel  on_level: doubt  action: quarantine  scope: tenant  sla: 1ms }
heal    Repair { source: Sentinel  on_level: doubt  mode: audit_only  scope: tenant }
"#;

    /// **The line that never existed.** The supervisor instantiates the declared
    /// graph from the IR — every kernel takes the compiled declarations directly,
    /// and nothing had ever handed them to it.
    #[test]
    fn the_supervisor_instantiates_the_declared_graph() {
        let ir = compile(PROGRAM);
        let sup = CognitiveIoSupervisor::from_ir(&ir).expect("the graph must instantiate");
        assert!(!sup.is_empty(), "a program with observe+immune has a graph to drive");
        assert_eq!(sup.detectors.len(), 1, "one immune ⇒ one AnomalyDetector");
    }

    /// A program with no Cognitive-I/O declarations gets no supervisor work.
    #[test]
    fn a_program_with_no_cognitive_io_has_an_empty_graph() {
        let ir = compile("flow F() -> Unit { let a = \"x\" }");
        let sup = CognitiveIoSupervisor::from_ir(&ir).unwrap();
        assert!(sup.is_empty());
    }

    /// **The flagship.** A tick walks observe → immune → reflex/heal, and the health
    /// report is produced from an observation that was *actually taken*.
    ///
    /// v2.67.0 — the first ticks LEARN. `immune.baseline: learned` is the language's
    /// default and `window:` is, in the AST's own words, "samples used to estimate
    /// baseline". Until the baseline exists there is nothing to be anomalous with
    /// respect to, so no health report is emitted and no reflex can fire — a reflex
    /// against an untrained baseline is a false positive BY CONSTRUCTION.
    #[test]
    fn a_tick_drives_observe_through_immune_to_the_motor_responses() {
        register_source_adapter("sv_probe", Arc::new(Fixed("sv_probe".into(), 0.95)));

        let ir = compile(PROGRAM);
        let mut sup = CognitiveIoSupervisor::from_ir(&ir).unwrap();

        // `window: 8` ⇒ the first eight ticks train the baseline.
        for _ in 0..8 {
            let t = sup.tick();
            assert!(
                t.learning.contains_key("Sentinel"),
                "while the baseline is being estimated the immune must say so — silence here                  would be indistinguishable from 'watching and finding nothing wrong'"
            );
            assert!(t.health.is_empty(), "no anomaly exists before a baseline does");
            assert!(t.reflexes.is_empty());
        }

        // Now it is watching.
        let tick = sup.tick();

        assert!(
            tick.observations.contains_key("Health"),
            "the observation must be taken; refusals: {:?}",
            tick.observation_refusals
        );
        assert_eq!(
            tick.observations["Health"].envelope.c, 0.95,
            "the envelope carries what the source ACTUALLY reported"
        );
        assert!(
            tick.health.contains_key("Sentinel"),
            "with a learned baseline the immune must now classify"
        );
        // The KL sensor ran on a real sample — the report is derived, not defaulted.
        assert_eq!(tick.health["Sentinel"].immune_name, "Sentinel");
        assert_eq!(
            tick.health["Sentinel"].classification, "know",
            "an unchanged world must classify as `know` — a healthy system that trips its own              immune system is a monitor nobody will keep listening to"
        );
    }

    /// **The law the supervisor must not soften.** An `observe` that REFUSES is
    /// recorded as a refusal — and it feeds **nothing**. An immune must never learn
    /// a baseline from an observation nobody took, and a reflex must never fire (or
    /// fail to fire) on a health report synthesised from silence.
    ///
    /// This is the v2.67.0 defect one level up: a supervisor that quietly dropped its
    /// refusals would recreate `DryRunHandler`'s `c: 1.0` in a new place.
    #[test]
    fn a_refused_observation_feeds_nothing_downstream() {
        // `sv_ghost` is never registered ⇒ the observe refuses (deny-by-default).
        let ir = compile(
            r#"
resource Db2 { kind: postgres  endpoint: db.main }
manifest Infra { resources: [Db2] }
observe Blind from Infra { sources: [sv_ghost]  quorum: 1  timeout: 1s  on_partition: fail }
immune  Sentinel2 { watch: [Blind]  scope: tenant  window: 8 }
reflex  React { trigger: Sentinel2  on_level: doubt  action: quarantine  scope: tenant  sla: 1ms }
"#,
        );
        let mut sup = CognitiveIoSupervisor::from_ir(&ir).unwrap();
        let tick = sup.tick();

        assert!(
            tick.observations.is_empty(),
            "an unregistered source must yield NO observation"
        );
        assert!(
            tick.observation_refusals.contains_key("Blind"),
            "the refusal must be recorded, first-class — a system we could not see is not a \
             system that is fine"
        );
        assert!(
            tick.health.is_empty(),
            "the immune must NOT produce a health report from an observation nobody took — \
             learning a baseline from silence is how a monitor becomes a liar"
        );
        assert!(
            tick.reflexes.is_empty(),
            "no reflex may fire on a health report that was never produced"
        );
    }
}
