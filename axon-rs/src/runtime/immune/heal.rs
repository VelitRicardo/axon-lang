//! AXON Runtime — HealKernel (v1.1.0, paper_immune_v2.md section 6-7)
//!
//! Direct port of `axon/runtime/immune/heal.py`.
//!
//! Linear-Logic one-shot patch kernel. Each patch has type
//! `P: !Synthesized ⊸ Applied ⊸ Collapsed` (paper section 6.2) — each transition
//! consumes its predecessor, yielding four hard guarantees:
//!   1. Single application (Synthesized consumed at Apply).
//!   2. Forced collapse (Applied MUST transition to Collapsed).
//!   3. No revival post-collapse.
//!   4. Full audit (every transition emits a signed trace).
//!
//! Compliance modes (paper section 7):
//!   * `audit_only`    — synthesised but never applied.
//!   * `human_in_loop` — synthesised; waits for explicit approve/reject.
//!   * `adversarial`   — applied autonomously with post-hoc review.

#![allow(dead_code)]

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::handlers::base::{HandlerError, LambdaEnvelope, make_envelope};
use crate::ir_nodes::IRHeal;

use super::health_report::{HealthReport, level_at_least};

/// Patch lifecycle state (Linear Logic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchState {
    Synthesized,
    Applied,
    Collapsed,
    Rejected,
}

impl PatchState {
    pub fn as_str(&self) -> &'static str {
        match self {
            PatchState::Synthesized => "synthesized",
            PatchState::Applied => "applied",
            PatchState::Collapsed => "collapsed",
            PatchState::Rejected => "rejected",
        }
    }
}

/// A proof-carrying patch under Linear Logic.
#[derive(Debug, Clone)]
pub struct Patch {
    pub patch_id: String,
    pub heal_name: String,
    pub source_immune: String,
    pub target_signature: String,
    pub payload: serde_json::Map<String, serde_json::Value>,
    pub state: PatchState,
    pub created_at: DateTime<Utc>,
    pub envelope: LambdaEnvelope,
    pub approvals: Vec<String>,
}

impl Patch {
    pub fn with_state(&self, state: PatchState, approver: &str) -> Patch {
        let mut approvals = self.approvals.clone();
        if !approver.is_empty() {
            approvals.push(approver.into());
        }
        Patch {
            patch_id: self.patch_id.clone(),
            heal_name: self.heal_name.clone(),
            source_immune: self.source_immune.clone(),
            target_signature: self.target_signature.clone(),
            payload: self.payload.clone(),
            state,
            created_at: self.created_at,
            envelope: self.envelope.clone(),
            approvals,
        }
    }
}

/// Synthesis / application / governance hooks.
pub type SynthesizeFn =
    Box<dyn Fn(&IRHeal, &HealthReport) -> serde_json::Map<String, serde_json::Value> + Send>;
pub type ApplyFn =
    Box<dyn Fn(&Patch) -> serde_json::Map<String, serde_json::Value> + Send>;
pub type ShieldApproveFn = Box<dyn Fn(&IRHeal, &Patch) -> bool + Send>;
pub type Clock = Box<dyn Fn() -> DateTime<Utc> + Send>;

/// Default deterministic placeholder patch — records KL profile for review.
pub fn default_synthesize(
) -> SynthesizeFn {
    Box::new(|_ir: &IRHeal, report: &HealthReport| {
        let mut m = serde_json::Map::new();
        m.insert("classification".into(), report.classification.clone().into());
        m.insert("kl_divergence".into(), report.kl_divergence.into());
        m.insert(
            "observation".into(),
            serde_json::Value::Array(
                report
                    .observation_window
                    .iter()
                    .cloned()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
        m.insert(
            "note".into(),
            "synthesized placeholder — override default_synthesize for real patches".into(),
        );
        m
    })
}

/// No-op apply — production deployments wire in real patch logic.
pub fn default_apply() -> ApplyFn {
    Box::new(|patch: &Patch| {
        let mut m = serde_json::Map::new();
        m.insert("applied_patch_id".into(), patch.patch_id.clone().into());
        m
    })
}

pub fn default_shield_approve() -> ShieldApproveFn {
    Box::new(|_ir: &IRHeal, _p: &Patch| true)
}

fn default_clock() -> Clock {
    Box::new(Utc::now)
}

/// Return type of `HealKernel::tick` and approve/reject — explains what happened.
#[derive(Debug, Clone)]
pub struct HealDecision {
    pub outcome: HealOutcome,
    pub patch: Option<Patch>,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealOutcome {
    Synthesized,
    Applied,
    RolledBack,
    Denied,
    Rejected,
    Skipped,
}

impl HealOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            HealOutcome::Synthesized => "synthesized",
            HealOutcome::Applied => "applied",
            HealOutcome::RolledBack => "rolled_back",
            HealOutcome::Denied => "denied",
            HealOutcome::Rejected => "rejected",
            HealOutcome::Skipped => "skipped",
        }
    }
}

/// Linear-Logic one-shot patch kernel.
pub struct HealKernel {
    synthesize: SynthesizeFn,
    apply: ApplyFn,
    shield_approve: ShieldApproveFn,
    clock: Clock,
    patches: HashMap<String, Patch>,
    heals: HashMap<String, IRHeal>,
    counts: HashMap<String, i64>,
}

impl HealKernel {
    pub fn new() -> Self {
        HealKernel {
            synthesize: default_synthesize(),
            apply: default_apply(),
            shield_approve: default_shield_approve(),
            clock: default_clock(),
            patches: HashMap::new(),
            heals: HashMap::new(),
            counts: HashMap::new(),
        }
    }

    pub fn with_hooks(
        synthesize: SynthesizeFn,
        apply: ApplyFn,
        shield_approve: ShieldApproveFn,
        clock: Clock,
    ) -> Self {
        HealKernel {
            synthesize,
            apply,
            shield_approve,
            clock,
            patches: HashMap::new(),
            heals: HashMap::new(),
            counts: HashMap::new(),
        }
    }

    pub fn register(&mut self, heal: IRHeal) {
        self.counts.entry(heal.name.clone()).or_insert(0);
        self.heals.insert(heal.name.clone(), heal);
    }

    /// Evaluate every registered heal against the report and advance the FSM.
    pub fn tick(&mut self, report: &HealthReport) -> Vec<HealDecision> {
        let mut decisions = Vec::new();
        let candidates: Vec<IRHeal> = self
            .heals
            .values()
            .filter(|h| h.source == report.immune_name)
            .cloned()
            .collect();
        for heal in candidates {
            decisions.push(self.step(&heal, report));
        }
        decisions
    }

    /// Explicit human-approval path — consumes Synthesized → Applied → Collapsed.
    pub fn approve(
        &mut self,
        patch_id: &str,
        approver: &str,
    ) -> Result<HealDecision, HandlerError> {
        let patch = self
            .patches
            .get(patch_id)
            .cloned()
            .ok_or_else(|| HandlerError::caller(format!("unknown patch '{patch_id}'")))?;
        if patch.state != PatchState::Synthesized {
            return Err(HandlerError::caller(format!(
                "patch '{patch_id}' in state '{}' cannot be approved",
                patch.state.as_str()
            )));
        }
        if !self.heals.contains_key(&patch.heal_name) {
            return Err(HandlerError::callee(format!(
                "heal '{}' missing at approve time",
                patch.heal_name
            )));
        }
        let applied = patch.with_state(PatchState::Applied, approver);
        self.patches.insert(patch_id.into(), applied.clone());
        let result = (self.apply)(&applied);
        let collapsed = applied.with_state(PatchState::Collapsed, "");
        self.patches.insert(patch_id.into(), collapsed.clone());
        let result_json = serde_json::Value::Object(result);
        Ok(HealDecision {
            outcome: HealOutcome::Applied,
            patch: Some(collapsed),
            reason: format!("approved by '{approver}'; apply returned {result_json}"),
        })
    }

    /// Reject a synthesised patch — collapses straight to the Rejected terminal.
    pub fn reject(
        &mut self,
        patch_id: &str,
        approver: &str,
    ) -> Result<HealDecision, HandlerError> {
        let patch = self
            .patches
            .get(patch_id)
            .cloned()
            .ok_or_else(|| HandlerError::caller(format!("unknown patch '{patch_id}'")))?;
        match patch.state {
            PatchState::Collapsed | PatchState::Applied | PatchState::Rejected => {
                return Err(HandlerError::caller(format!(
                    "patch '{patch_id}' already finalized in state '{}'",
                    patch.state.as_str()
                )));
            }
            _ => {}
        }
        let who = if approver.is_empty() { "reviewer" } else { approver };
        let rejected = patch.with_state(PatchState::Rejected, who);
        self.patches.insert(patch_id.into(), rejected.clone());
        Ok(HealDecision {
            outcome: HealOutcome::RolledBack,
            patch: Some(rejected),
            reason: format!(
                "rejected by '{who}'; Linear token collapses to rejected terminal"
            ),
        })
    }

    pub fn patches(&self) -> Vec<Patch> {
        self.patches.values().cloned().collect()
    }

    pub fn patches_by_state(&self, state: PatchState) -> Vec<Patch> {
        self.patches.values().filter(|p| p.state == state).cloned().collect()
    }

    fn step(&mut self, heal: &IRHeal, report: &HealthReport) -> HealDecision {
        if !level_at_least(&report.classification, &heal.on_level) {
            return HealDecision {
                outcome: HealOutcome::Skipped,
                patch: None,
                reason: format!(
                    "report level '{}' below heal threshold '{}'",
                    report.classification, heal.on_level
                ),
            };
        }
        let already = *self.counts.get(&heal.name).unwrap_or(&0);
        if already >= heal.max_patches {
            return HealDecision {
                outcome: HealOutcome::Skipped,
                patch: None,
                reason: format!(
                    "heal '{}' reached max_patches={}",
                    heal.name, heal.max_patches
                ),
            };
        }

        let payload = (self.synthesize)(heal, report);
        let patch = Patch {
            patch_id: format!("patch-{}", &Uuid::new_v4().simple().to_string()[..12]),
            heal_name: heal.name.clone(),
            source_immune: heal.source.clone(),
            target_signature: report.anomaly_signature.clone(),
            payload,
            state: PatchState::Synthesized,
            created_at: (self.clock)(),
            envelope: make_envelope(
                report.envelope.c,
                &format!("heal:{}", heal.name),
                "inferred",
                None,
            ),
            approvals: Vec::new(),
        };
        self.patches.insert(patch.patch_id.clone(), patch.clone());
        *self.counts.entry(heal.name.clone()).or_insert(0) += 1;

        match heal.mode.as_str() {
            "audit_only" => {
                // Synthesised → Collapsed without ever passing through Applied.
                let collapsed = patch.with_state(PatchState::Collapsed, "");
                self.patches.insert(collapsed.patch_id.clone(), collapsed.clone());
                HealDecision {
                    outcome: HealOutcome::Synthesized,
                    patch: Some(collapsed),
                    reason: "audit_only mode — synthesized and collapsed without application".into(),
                }
            }
            "adversarial" => {
                if !(self.shield_approve)(heal, &patch) {
                    let rejected = patch.with_state(PatchState::Rejected, "shield");
                    self.patches.insert(rejected.patch_id.clone(), rejected.clone());
                    return HealDecision {
                        outcome: HealOutcome::Denied,
                        patch: Some(rejected),
                        reason: "shield denied adversarial patch".into(),
                    };
                }
                let applied = patch.with_state(PatchState::Applied, "autonomous");
                self.patches.insert(applied.patch_id.clone(), applied.clone());
                let _ = (self.apply)(&applied);
                let collapsed = applied.with_state(PatchState::Collapsed, "");
                self.patches.insert(collapsed.patch_id.clone(), collapsed.clone());
                HealDecision {
                    outcome: HealOutcome::Applied,
                    patch: Some(collapsed),
                    reason: "adversarial mode — autonomous application + collapse".into(),
                }
            }
            _ => HealDecision {
                // human_in_loop (default).
                outcome: HealOutcome::Synthesized,
                patch: Some(patch),
                reason: "human_in_loop — waiting for explicit approval within review SLA".into(),
            },
        }
    }
}

impl Default for HealKernel {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::health_report::make_health_report;

    fn mk_heal(name: &str, source: &str, mode: &str, on_level: &str, max: i64) -> IRHeal {
        IRHeal {
            node_type: "heal",
            source_line: 1,
            source_column: 1,
            name: name.into(),
            source: source.into(),
            on_level: on_level.into(),
            mode: mode.into(),
            scope: "tenant".into(),
            review_sla: "1h".into(),
            shield_ref: String::new(),
            max_patches: max,
        }
    }

    fn mk_report(immune: &str, level: &str, sig: &str) -> HealthReport {
        let kl = match level {
            "know" => 0.1,
            "believe" => 0.45,
            "speculate" => 0.75,
            "doubt" => 0.95,
            _ => 0.0,
        };
        let mut r = make_health_report(
            immune, kl, vec!["Health".into()], sig, 300.0, "exponential", "immune:I",
        );
        r.classification = level.into();
        r
    }

    #[test]
    fn audit_only_synthesises_and_collapses_without_apply() {
        let mut k = HealKernel::new();
        k.register(mk_heal("H", "I", "audit_only", "doubt", 3));
        let decisions = k.tick(&mk_report("I", "doubt", "sig"));
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].outcome, HealOutcome::Synthesized);
        let p = decisions[0].patch.as_ref().unwrap();
        assert_eq!(p.state, PatchState::Collapsed);
    }

    #[test]
    fn human_in_loop_stops_at_synthesized() {
        let mut k = HealKernel::new();
        k.register(mk_heal("H", "I", "human_in_loop", "believe", 3));
        let decisions = k.tick(&mk_report("I", "speculate", "sig"));
        let p = decisions[0].patch.as_ref().unwrap();
        assert_eq!(p.state, PatchState::Synthesized);
        assert_eq!(decisions[0].outcome, HealOutcome::Synthesized);
    }

    #[test]
    fn approve_path_drives_synthesized_to_collapsed() {
        let mut k = HealKernel::new();
        k.register(mk_heal("H", "I", "human_in_loop", "believe", 3));
        let decisions = k.tick(&mk_report("I", "speculate", "sig"));
        let pid = decisions[0].patch.as_ref().unwrap().patch_id.clone();
        let approved = k.approve(&pid, "alice").unwrap();
        assert_eq!(approved.outcome, HealOutcome::Applied);
        let final_patch = approved.patch.unwrap();
        assert_eq!(final_patch.state, PatchState::Collapsed);
        assert!(final_patch.approvals.iter().any(|a| a == "alice"));
    }

    #[test]
    fn reject_path_collapses_to_rejected_terminal() {
        let mut k = HealKernel::new();
        k.register(mk_heal("H", "I", "human_in_loop", "believe", 3));
        let decisions = k.tick(&mk_report("I", "speculate", "sig"));
        let pid = decisions[0].patch.as_ref().unwrap().patch_id.clone();
        let rejected = k.reject(&pid, "bob").unwrap();
        assert_eq!(rejected.outcome, HealOutcome::RolledBack);
        assert_eq!(rejected.patch.unwrap().state, PatchState::Rejected);
    }

    #[test]
    fn adversarial_applies_autonomously_when_shield_approves() {
        let mut k = HealKernel::new();
        k.register(mk_heal("H", "I", "adversarial", "believe", 3));
        let d = k.tick(&mk_report("I", "doubt", "sig"));
        assert_eq!(d[0].outcome, HealOutcome::Applied);
        assert_eq!(d[0].patch.as_ref().unwrap().state, PatchState::Collapsed);
    }

    #[test]
    fn adversarial_denied_when_shield_rejects() {
        let k = HealKernel::with_hooks(
            default_synthesize(),
            default_apply(),
            Box::new(|_ir, _p| false),
            default_clock(),
        );
        let mut k = k;
        k.register(mk_heal("H", "I", "adversarial", "believe", 3));
        let d = k.tick(&mk_report("I", "doubt", "sig"));
        assert_eq!(d[0].outcome, HealOutcome::Denied);
        assert_eq!(d[0].patch.as_ref().unwrap().state, PatchState::Rejected);
    }

    #[test]
    fn max_patches_budget_caps_synthesis() {
        let mut k = HealKernel::new();
        k.register(mk_heal("H", "I", "human_in_loop", "know", 2));
        let r1 = mk_report("I", "doubt", "s1");
        let r2 = mk_report("I", "doubt", "s2");
        let r3 = mk_report("I", "doubt", "s3");
        let _ = k.tick(&r1);
        let _ = k.tick(&r2);
        let third = k.tick(&r3);
        assert_eq!(third[0].outcome, HealOutcome::Skipped);
        assert!(third[0].reason.contains("max_patches"));
    }

    #[test]
    fn skip_below_threshold() {
        let mut k = HealKernel::new();
        k.register(mk_heal("H", "I", "human_in_loop", "doubt", 3));
        let r = mk_report("I", "believe", "s");
        let d = k.tick(&r);
        assert_eq!(d[0].outcome, HealOutcome::Skipped);
    }

    #[test]
    fn approve_rejected_patch_is_caller_error() {
        let mut k = HealKernel::new();
        k.register(mk_heal("H", "I", "human_in_loop", "know", 3));
        let d = k.tick(&mk_report("I", "doubt", "s"));
        let pid = d[0].patch.as_ref().unwrap().patch_id.clone();
        let _ = k.reject(&pid, "bob").unwrap();
        match k.approve(&pid, "alice") {
            Err(e) => assert_eq!(e.blame, "CT-2"),
            Ok(_) => panic!("approving a rejected patch must fail"),
        }
    }
}
