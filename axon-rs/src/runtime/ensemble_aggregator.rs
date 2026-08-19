//! AXON Runtime — EnsembleAggregator (v1.1.0)
//!
//! Direct port of `axon/runtime/ensemble_aggregator.py`.
//!
//! Byzantine quorum aggregator fusing N independent `HandlerOutcome`s
//! into a single consensus outcome. Models Fagin-Halpern-Moses-Vardi
//! common knowledge `Cφ`: holds iff ≥ `quorum` observers independently
//! agree.
//!
//! Aggregation policies:
//!   * `majority`   — modal status wins; ties → "partial".
//!   * `weighted`   — each outcome weighted by its certainty c; argmax wins.
//!   * `byzantine`  — Lamport: drop best- and worst-c outliers, then majority.
//!
//! Certainty fusion:
//!   * `min`        — min(c_i) (conservative default).
//!   * `weighted`   — Σ(c_i²) / Σ(c_i) ("strong voices count more").
//!   * `harmonic`   — harmonic mean with an ε floor.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};

use crate::handlers::base::{HandlerError, HandlerOutcome, LambdaEnvelope, make_envelope};
use crate::ir_nodes::IREnsemble;

const VALID_AGGREGATIONS: &[&str] = &["majority", "weighted", "byzantine"];
const VALID_CERTAINTY_MODES: &[&str] = &["min", "weighted", "harmonic"];

/// Explanatory structure for aggregation outcomes — surfaced by tests and logs.
#[derive(Debug, Clone)]
pub struct EnsembleReport {
    pub aggregation: String,
    pub certainty_mode: String,
    pub contributors: Vec<String>,
    pub status_tally: HashMap<String, i64>,
    pub winning_status: String,
    pub fused_certainty: f64,
}

/// Pure aggregator — stateless, threadsafe.
pub struct EnsembleAggregator {
    ir: IREnsemble,
    quorum: i64,
}

impl EnsembleAggregator {
    pub fn new(ir_ensemble: IREnsemble) -> Result<Self, HandlerError> {
        if !VALID_AGGREGATIONS.contains(&ir_ensemble.aggregation.as_str()) {
            return Err(HandlerError::callee(format!(
                "ensemble '{}' has invalid aggregation '{}'",
                ir_ensemble.name, ir_ensemble.aggregation
            )));
        }
        if !VALID_CERTAINTY_MODES.contains(&ir_ensemble.certainty_mode.as_str()) {
            return Err(HandlerError::callee(format!(
                "ensemble '{}' has invalid certainty_mode '{}'",
                ir_ensemble.name, ir_ensemble.certainty_mode
            )));
        }
        let quorum = ir_ensemble.quorum.unwrap_or_else(|| {
            let n = ir_ensemble.observations.len() as i64;
            (n / 2 + 1).max(1)
        });
        Ok(EnsembleAggregator { ir: ir_ensemble, quorum })
    }

    /// Fuse outcomes into a single consensus `HandlerOutcome`.
    pub fn aggregate(
        &self,
        outcomes: &[HandlerOutcome],
    ) -> Result<(HandlerOutcome, EnsembleReport), HandlerError> {
        let mut survivors: Vec<HandlerOutcome> = outcomes
            .iter()
            .filter(|o| o.status != "failed")
            .cloned()
            .collect();
        if survivors.is_empty() {
            survivors = outcomes.to_vec();
        }
        if (survivors.len() as i64) < self.quorum {
            return Err(HandlerError::infrastructure(format!(
                "ensemble '{}' has only {} surviving observations; quorum requires {}. \
                 Decision D4: too many partitions ⇒ CT-3.",
                self.ir.name,
                survivors.len(),
                self.quorum
            )));
        }

        if self.ir.aggregation == "byzantine" {
            survivors = Self::byzantine_filter(&survivors);
        }

        let tally = Self::tally_statuses(&survivors);
        let winning_status = self.select_status(&survivors, &tally);
        let accepting: Vec<HandlerOutcome> = survivors
            .iter()
            .filter(|o| o.status == winning_status)
            .cloned()
            .collect();

        if (accepting.len() as i64) < self.quorum {
            return Err(HandlerError::infrastructure(format!(
                "ensemble '{}' failed Cφ consensus: only {}/{} observers agreed on '{}'. \
                 Status tally: {:?}",
                self.ir.name,
                accepting.len(),
                self.quorum,
                winning_status,
                tally
            )));
        }

        let cs: Vec<f64> = accepting.iter().map(|o| o.envelope.c).collect();
        let fused_c = self.fuse_certainty(&cs)?;
        let envelope = self.fused_envelope(&accepting, fused_c);

        let mut data = serde_json::Map::new();
        data.insert("aggregation".into(), self.ir.aggregation.clone().into());
        data.insert("certainty_mode".into(), self.ir.certainty_mode.clone().into());
        data.insert("quorum".into(), self.quorum.into());
        data.insert(
            "contributors".into(),
            serde_json::Value::Array(
                survivors.iter().map(|o| o.target.clone().into()).collect(),
            ),
        );
        data.insert(
            "accepting".into(),
            serde_json::Value::Array(
                accepting.iter().map(|o| o.target.clone().into()).collect(),
            ),
        );
        // Status tally preserves insertion order via BTreeMap for readable JSON.
        let mut tally_map = serde_json::Map::new();
        for (k, v) in tally.iter() {
            tally_map.insert(k.clone(), serde_json::Value::from(*v));
        }
        data.insert("status_tally".into(), serde_json::Value::Object(tally_map));

        let consensus = HandlerOutcome::new(
            "ensemble",
            self.ir.name.clone(),
            winning_status.clone(),
            envelope,
            format!("ensemble:{}", self.ir.name),
        )
        .with_data(data);

        let report = EnsembleReport {
            aggregation: self.ir.aggregation.clone(),
            certainty_mode: self.ir.certainty_mode.clone(),
            contributors: survivors.iter().map(|o| o.target.clone()).collect(),
            status_tally: tally.into_iter().collect(),
            winning_status,
            fused_certainty: fused_c,
        };
        Ok((consensus, report))
    }

    fn tally_statuses(survivors: &[HandlerOutcome]) -> BTreeMap<String, i64> {
        let mut t: BTreeMap<String, i64> = BTreeMap::new();
        for o in survivors {
            *t.entry(o.status.clone()).or_insert(0) += 1;
        }
        t
    }

    fn select_status(
        &self,
        survivors: &[HandlerOutcome],
        tally: &BTreeMap<String, i64>,
    ) -> String {
        if self.ir.aggregation == "weighted" {
            let mut scores: HashMap<&str, f64> = HashMap::new();
            for o in survivors {
                *scores.entry(o.status.as_str()).or_insert(0.0) += o.envelope.c;
            }
            return scores
                .into_iter()
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(k, _)| k.to_string())
                .unwrap_or_else(|| "failed".into());
        }
        // majority + byzantine share the modal rule. Ties → "partial".
        let mut pairs: Vec<(&String, &i64)> = tally.iter().collect();
        pairs.sort_by(|a, b| b.1.cmp(a.1));
        if pairs.len() >= 2 && pairs[0].1 == pairs[1].1 {
            return "partial".to_string();
        }
        pairs[0].0.clone()
    }

    fn byzantine_filter(survivors: &[HandlerOutcome]) -> Vec<HandlerOutcome> {
        if survivors.len() < 4 {
            return survivors.to_vec();
        }
        let mut sorted: Vec<HandlerOutcome> = survivors.to_vec();
        sorted.sort_by(|a, b| {
            a.envelope
                .c
                .partial_cmp(&b.envelope.c)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        // Drop best + worst outliers (Lamport's one-third rule approximation).
        sorted[1..sorted.len() - 1].to_vec()
    }

    fn fuse_certainty(&self, values: &[f64]) -> Result<f64, HandlerError> {
        if values.is_empty() {
            return Ok(0.0);
        }
        match self.ir.certainty_mode.as_str() {
            "min" => Ok(values.iter().cloned().fold(f64::INFINITY, f64::min)),
            "weighted" => {
                let total: f64 = values.iter().sum();
                if total == 0.0 {
                    return Ok(0.0);
                }
                Ok(values.iter().map(|c| c * c).sum::<f64>() / total)
            }
            "harmonic" => {
                let eps = 1e-9;
                let safe: Vec<f64> = values.iter().map(|c| c.max(eps)).collect();
                Ok(safe.len() as f64 / safe.iter().map(|c| 1.0 / c).sum::<f64>())
            }
            other => Err(HandlerError::callee(format!(
                "unknown certainty_mode '{other}'"
            ))),
        }
    }

    fn fused_envelope(&self, accepting: &[HandlerOutcome], fused_c: f64) -> LambdaEnvelope {
        // Sort handler identifiers so provenance is deterministic.
        let mut handlers: Vec<String> = accepting
            .iter()
            .map(|o| o.handler.clone())
            .filter(|h| !h.is_empty())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        handlers.sort();
        let rho = if handlers.is_empty() {
            format!("ensemble:{}", self.ir.name)
        } else {
            format!("ensemble({})", handlers.join(","))
        };
        make_envelope(fused_c, &rho, "inferred", None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::base::{HandlerErrorKind, make_envelope};

    fn outcome(target: &str, status: &str, c: f64, handler: &str) -> HandlerOutcome {
        let env = make_envelope(c, handler, "observed", Some("T".into()));
        HandlerOutcome::new("observe", target, status, env, handler)
    }

    fn mk_ensemble(aggregation: &str, certainty_mode: &str, quorum: Option<i64>) -> IREnsemble {
        IREnsemble {
            node_type: "ensemble",
            source_line: 1,
            source_column: 1,
            name: "E".into(),
            observations: vec!["O1".into(), "O2".into(), "O3".into()],
            quorum,
            aggregation: aggregation.into(),
            certainty_mode: certainty_mode.into(),
        }
    }

    #[test]
    fn rejects_invalid_aggregation() {
        let mut ir = mk_ensemble("majority", "min", Some(2));
        ir.aggregation = "consensus_by_vibes".into();
        match EnsembleAggregator::new(ir) {
            Err(e) => assert_eq!(e.kind, HandlerErrorKind::Callee),
            Ok(_) => panic!("invalid aggregation must be rejected at construction"),
        }
    }

    #[test]
    fn majority_wins_modal_status() {
        let agg = EnsembleAggregator::new(mk_ensemble("majority", "min", Some(2))).unwrap();
        let outs = vec![
            outcome("O1", "ok", 0.9, "h1"),
            outcome("O2", "ok", 0.8, "h2"),
            outcome("O3", "partial", 0.5, "h3"),
        ];
        let (consensus, report) = agg.aggregate(&outs).unwrap();
        assert_eq!(consensus.status, "ok");
        assert_eq!(report.winning_status, "ok");
        // min fusion → 0.8
        assert!((report.fused_certainty - 0.8).abs() < 1e-9);
    }

    #[test]
    fn majority_tie_resolves_to_partial() {
        let agg = EnsembleAggregator::new(mk_ensemble("majority", "min", Some(2))).unwrap();
        let _outs = vec![
            outcome("O1", "ok", 0.9, "h1"),
            outcome("O2", "ok", 0.8, "h2"),
            outcome("O3", "failed", 0.5, "h3"),
            outcome("O4", "failed", 0.4, "h4"),
        ];
        // `ok` has count 2, `failed` has count 0 after survivor filter drops
        // the two failed. Use a bad scenario: all four survive by not filtering.
        // To force a real tie we need equal non-failed counts.
        let outs = vec![
            outcome("O1", "ok", 0.9, "h1"),
            outcome("O2", "ok", 0.8, "h2"),
            outcome("O3", "partial", 0.6, "h3"),
            outcome("O4", "partial", 0.5, "h4"),
        ];
        let (consensus, report) = agg.aggregate(&outs).unwrap();
        assert_eq!(consensus.status, "partial");
        assert_eq!(report.winning_status, "partial");
    }

    #[test]
    fn weighted_picks_argmax_by_certainty() {
        let agg = EnsembleAggregator::new(mk_ensemble("weighted", "weighted", Some(1))).unwrap();
        // Two partials with low c, one ok with high c — weighted argmax → ok.
        let outs = vec![
            outcome("O1", "partial", 0.2, "h1"),
            outcome("O2", "partial", 0.1, "h2"),
            outcome("O3", "ok", 0.95, "h3"),
        ];
        let (consensus, _) = agg.aggregate(&outs).unwrap();
        assert_eq!(consensus.status, "ok");
    }

    #[test]
    fn below_quorum_raises_ct3() {
        let agg = EnsembleAggregator::new(mk_ensemble("majority", "min", Some(3))).unwrap();
        let outs = vec![
            outcome("O1", "ok", 0.9, "h1"),
            outcome("O2", "failed", 0.2, "h2"),
        ];
        let err = agg.aggregate(&outs).unwrap_err();
        assert_eq!(err.kind, HandlerErrorKind::Infrastructure);
        assert_eq!(err.blame, "CT-3");
    }

    #[test]
    fn byzantine_drops_best_and_worst_before_majority() {
        let agg = EnsembleAggregator::new(mk_ensemble("byzantine", "min", Some(2))).unwrap();
        // 5 survivors: drop best (c=1.0 "failed") + worst (c=0.1 "failed");
        // remaining 3 agree on "ok".
        let outs = vec![
            outcome("O1", "ok", 0.6, "h1"),
            outcome("O2", "ok", 0.7, "h2"),
            outcome("O3", "ok", 0.8, "h3"),
            outcome("O4", "failed", 0.1, "h4"),  // dropped (worst)
            outcome("O5", "failed", 1.0, "h5"),  // dropped (best)
        ];
        let (consensus, report) = agg.aggregate(&outs).unwrap();
        assert_eq!(consensus.status, "ok");
        assert_eq!(report.fused_certainty, 0.6);
    }

    #[test]
    fn certainty_fusion_min_is_conservative() {
        let agg = EnsembleAggregator::new(mk_ensemble("majority", "min", Some(2))).unwrap();
        let outs = vec![
            outcome("O1", "ok", 0.9, "h1"),
            outcome("O2", "ok", 0.7, "h2"),
            outcome("O3", "ok", 0.5, "h3"),
        ];
        let (_, report) = agg.aggregate(&outs).unwrap();
        assert!((report.fused_certainty - 0.5).abs() < 1e-9);
    }

    #[test]
    fn certainty_fusion_weighted_self_weighted_mean() {
        let agg = EnsembleAggregator::new(mk_ensemble("majority", "weighted", Some(2))).unwrap();
        let outs = vec![
            outcome("O1", "ok", 0.8, "h1"),
            outcome("O2", "ok", 0.6, "h2"),
        ];
        // weighted: (0.64 + 0.36) / 1.4 = 1.0 / 1.4 ≈ 0.7142...
        let (_, report) = agg.aggregate(&outs).unwrap();
        let expected = (0.8_f64.powi(2) + 0.6_f64.powi(2)) / (0.8 + 0.6);
        assert!((report.fused_certainty - expected).abs() < 1e-9);
    }

    #[test]
    fn certainty_fusion_harmonic_penalises_outliers() {
        let agg = EnsembleAggregator::new(mk_ensemble("majority", "harmonic", Some(2))).unwrap();
        let outs = vec![
            outcome("O1", "ok", 0.9, "h1"),
            outcome("O2", "ok", 0.9, "h2"),
            outcome("O3", "ok", 0.1, "h3"),
        ];
        let (_, report) = agg.aggregate(&outs).unwrap();
        // Harmonic mean of 0.9, 0.9, 0.1 ≈ 0.2368
        let expected = 3.0 / (1.0 / 0.9 + 1.0 / 0.9 + 1.0 / 0.1);
        assert!((report.fused_certainty - expected).abs() < 1e-6);
    }

    #[test]
    fn failed_status_survivors_excluded() {
        let agg = EnsembleAggregator::new(mk_ensemble("majority", "min", Some(2))).unwrap();
        let outs = vec![
            outcome("O1", "ok", 0.9, "h1"),
            outcome("O2", "ok", 0.8, "h2"),
            outcome("O3", "failed", 0.1, "h3"),
        ];
        let (consensus, report) = agg.aggregate(&outs).unwrap();
        assert_eq!(consensus.status, "ok");
        // failed is filtered out before selection
        assert_eq!(report.contributors.len(), 2);
    }

    #[test]
    fn fused_envelope_provenance_is_deterministic() {
        let agg = EnsembleAggregator::new(mk_ensemble("majority", "min", Some(2))).unwrap();
        let outs = vec![
            outcome("O1", "ok", 0.9, "zeta"),
            outcome("O2", "ok", 0.8, "alpha"),
        ];
        let (consensus, _) = agg.aggregate(&outs).unwrap();
        // Handlers must be sorted alphabetically.
        assert_eq!(consensus.envelope.rho, "ensemble(alpha,zeta)");
    }
}
