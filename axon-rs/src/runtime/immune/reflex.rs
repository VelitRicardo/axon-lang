//! AXON Runtime — ReflexEngine (v1.1.0, paper_immune_v2.md section 4.2)
//!
//! Direct port of `axon/runtime/immune/reflex.py`.
//!
//! Deterministic, O(1), LLM-free motor responses. Every activation is:
//!   * Idempotent on `(reflex_name, signature)`.
//!   * HMAC-signed for an auditable trace.
//!   * Never an LLM call; no long-running I/O.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::handlers::base::{HandlerError, LambdaEnvelope, make_envelope};
use crate::ir_nodes::IRReflex;

use super::health_report::{HealthReport, level_at_least};

type HmacSha256 = Hmac<Sha256>;

const KNOWN_ACTIONS: &[&str] = &[
    "drop", "revoke", "emit", "redact", "quarantine", "terminate", "alert",
];

/// Result of one reflex firing — fully auditable, no side-channels.
#[derive(Debug, Clone)]
pub struct ReflexOutcome {
    pub reflex_name: String,
    pub action: String,
    pub fired: bool,
    pub reason: String,
    pub target_signature: String,
    pub latency_us: f64,
    pub envelope: LambdaEnvelope,
    pub signed_trace: String,
}

fn sign(message: &str, secret: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC key of any length");
    mac.update(message.as_bytes());
    let hex = format!("{:x}", mac.finalize().into_bytes());
    hex[..32.min(hex.len())].to_string()
}

/// Registry-dispatching engine that consumes HealthReports and fires
/// matching `reflex` declarations when their epistemic threshold is met.
pub struct ReflexEngine {
    reflexes: HashMap<String, IRReflex>,
    fired: HashSet<(String, String)>,
    trace_secret: Vec<u8>,
}

impl ReflexEngine {
    pub fn new() -> Self {
        let mut h = Sha256::new();
        h.update(b"axon-reflex-engine-default-secret");
        ReflexEngine::with_secret(h.finalize().to_vec())
    }

    pub fn with_secret(trace_secret: Vec<u8>) -> Self {
        ReflexEngine {
            reflexes: HashMap::new(),
            fired: HashSet::new(),
            trace_secret,
        }
    }

    pub fn register(&mut self, reflex: IRReflex) -> Result<(), HandlerError> {
        if !KNOWN_ACTIONS.contains(&reflex.action.as_str()) {
            return Err(HandlerError::callee(format!(
                "reflex '{}' declares unknown action '{}'. Engine knows: {}",
                reflex.name,
                reflex.action,
                KNOWN_ACTIONS.join(", ")
            )));
        }
        self.reflexes.insert(reflex.name.clone(), reflex);
        Ok(())
    }

    /// Fire every registered reflex whose trigger == report.immune_name AND
    /// whose on_level is reached or exceeded by the report.
    pub fn dispatch(&mut self, report: &HealthReport) -> Vec<ReflexOutcome> {
        let mut outs = Vec::new();
        // Clone IRReflex candidates to avoid aliasing `self` during mutation.
        let candidates: Vec<IRReflex> = self
            .reflexes
            .values()
            .filter(|r| r.trigger == report.immune_name)
            .cloned()
            .collect();
        for reflex in candidates {
            outs.push(self.maybe_fire(&reflex, report));
        }
        outs
    }

    /// Reset idempotency set — used by tests.
    pub fn clear_idempotency(&mut self) {
        self.fired.clear();
    }

    fn maybe_fire(&mut self, reflex: &IRReflex, report: &HealthReport) -> ReflexOutcome {
        let start = Instant::now();
        if !level_at_least(&report.classification, &reflex.on_level) {
            return self.noop(
                reflex,
                report,
                start,
                format!(
                    "level '{}' below threshold '{}'",
                    report.classification, reflex.on_level
                ),
            );
        }
        let key_sig = if report.anomaly_signature.is_empty() {
            report.immune_name.clone()
        } else {
            report.anomaly_signature.clone()
        };
        let key = (reflex.name.clone(), key_sig);
        if self.fired.contains(&key) {
            return self.noop(
                reflex,
                report,
                start,
                "idempotent skip (already fired for this signature)".into(),
            );
        }
        self.fired.insert(key);
        // Default handlers are pure no-ops — deployments wire real hooks
        // via a separate adapter layer (see Python `register_action_hook`).
        let latency_us = start.elapsed().as_secs_f64() * 1e6;
        let payload = format!(
            "{}|{}|{}|{}|{:.6}",
            reflex.name,
            reflex.action,
            report.anomaly_signature,
            report.classification,
            report.kl_divergence,
        );
        ReflexOutcome {
            reflex_name: reflex.name.clone(),
            action: reflex.action.clone(),
            fired: true,
            reason: format!(
                "level '{}' ≥ threshold '{}'",
                report.classification, reflex.on_level
            ),
            target_signature: report.anomaly_signature.clone(),
            latency_us,
            envelope: make_envelope(
                report.envelope.c,
                &format!("reflex:{}", reflex.name),
                "observed",
                None,
            ),
            signed_trace: sign(&payload, &self.trace_secret),
        }
    }

    fn noop(
        &self,
        reflex: &IRReflex,
        report: &HealthReport,
        start: Instant,
        reason: String,
    ) -> ReflexOutcome {
        let latency_us = start.elapsed().as_secs_f64() * 1e6;
        let payload = format!(
            "{}|NOOP|{}|{}",
            reflex.name, report.anomaly_signature, reason
        );
        ReflexOutcome {
            reflex_name: reflex.name.clone(),
            action: reflex.action.clone(),
            fired: false,
            reason,
            target_signature: report.anomaly_signature.clone(),
            latency_us,
            envelope: make_envelope(
                report.envelope.c,
                &format!("reflex:{}", reflex.name),
                "observed",
                None,
            ),
            signed_trace: sign(&payload, &self.trace_secret),
        }
    }
}

impl Default for ReflexEngine {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::health_report::make_health_report;
    use crate::handlers::base::HandlerErrorKind;

    fn mk_reflex(name: &str, trigger: &str, on_level: &str, action: &str) -> IRReflex {
        IRReflex {
            node_type: "reflex",
            source_line: 1,
            source_column: 1,
            name: name.into(),
            trigger: trigger.into(),
            on_level: on_level.into(),
            action: action.into(),
            scope: "tenant".into(),
            sla: "1ms".into(),
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
        // Force classification exactly (KL mapping is tested elsewhere).
        r.classification = level.into();
        r
    }

    #[test]
    fn register_rejects_unknown_action() {
        let mut eng = ReflexEngine::new();
        let bad = mk_reflex("R", "I", "doubt", "yeet");
        let err = eng.register(bad).unwrap_err();
        assert_eq!(err.kind, HandlerErrorKind::Callee);
    }

    #[test]
    fn dispatch_fires_reflex_at_or_above_threshold() {
        let mut eng = ReflexEngine::new();
        eng.register(mk_reflex("Stop", "I", "believe", "quarantine")).unwrap();
        let report = mk_report("I", "speculate", "sig-1");
        let outs = eng.dispatch(&report);
        assert_eq!(outs.len(), 1);
        assert!(outs[0].fired);
        assert!(!outs[0].signed_trace.is_empty());
    }

    #[test]
    fn dispatch_does_not_fire_below_threshold() {
        let mut eng = ReflexEngine::new();
        eng.register(mk_reflex("Stop", "I", "doubt", "quarantine")).unwrap();
        let report = mk_report("I", "believe", "sig-1");
        let outs = eng.dispatch(&report);
        assert_eq!(outs.len(), 1);
        assert!(!outs[0].fired);
        assert!(outs[0].reason.contains("below threshold"));
    }

    #[test]
    fn dispatch_is_idempotent_on_same_signature() {
        let mut eng = ReflexEngine::new();
        eng.register(mk_reflex("Stop", "I", "doubt", "quarantine")).unwrap();
        let report = mk_report("I", "doubt", "sig-x");
        let first = eng.dispatch(&report);
        let second = eng.dispatch(&report);
        assert!(first[0].fired);
        assert!(!second[0].fired);
        assert!(second[0].reason.contains("idempotent"));
    }

    #[test]
    fn dispatch_only_triggers_on_matching_immune_name() {
        let mut eng = ReflexEngine::new();
        eng.register(mk_reflex("R_other", "OtherSensor", "know", "alert")).unwrap();
        let report = mk_report("I", "doubt", "sig-m");
        assert!(eng.dispatch(&report).is_empty());
    }

    #[test]
    fn signed_trace_differs_per_firing_payload() {
        let mut eng = ReflexEngine::new();
        eng.register(mk_reflex("R", "I", "know", "alert")).unwrap();
        let r1 = mk_report("I", "know", "sig-A");
        let r2 = mk_report("I", "know", "sig-B");
        let a = eng.dispatch(&r1).into_iter().next().unwrap();
        let b = eng.dispatch(&r2).into_iter().next().unwrap();
        assert_ne!(a.signed_trace, b.signed_trace);
    }

    #[test]
    fn latency_is_small_and_non_negative() {
        let mut eng = ReflexEngine::new();
        eng.register(mk_reflex("R", "I", "know", "alert")).unwrap();
        let report = mk_report("I", "doubt", "sig-l");
        let out = eng.dispatch(&report).into_iter().next().unwrap();
        assert!(out.latency_us >= 0.0);
        // Paper section 4.2 target is ~few microseconds; allow slack for CI noise.
        assert!(out.latency_us < 5_000.0);
    }
}
