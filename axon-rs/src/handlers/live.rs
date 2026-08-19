//! v2.67.0 — **the `Handler` that actually looks at something.**
//!
//! # What this replaces
//!
//! Until now the tree contained exactly one `Handler`: [`super::dry_run::DryRunHandler`].
//! Its `observe` ends like this:
//!
//! ```ignore
//! make_envelope(1.0, &self.name, "observed", None)
//! ```
//!
//! **Certainty `1.0`. Always. Without going anywhere.** It records what it *would*
//! have looked at and returns a perfect-confidence envelope. It has **zero
//! production instantiations** (v2.67.0 F14) — which, it turns out, was the only thing
//! protecting anyone: a supervisor wired to the only available `Handler` would have
//! reported **perfect health for every system, forever**, without opening a single
//! socket.
//!
//! An `observe` is the primitive whose *entire purpose* is telling you the
//! difference between "I checked and it's fine" and "I have no idea". The one
//! implementation of it could not tell those apart, and always said the first.
//!
//! # The law
//!
//! **An observation that cannot be taken REFUSES. It never returns a default
//! envelope.** Concretely, every one of these is a refusal, not a reading:
//!
//! | | |
//! |---|---|
//! | a source resolves to no registered adapter | it is **unknown**, not healthy |
//! | fewer sources answered than `quorum:` | a **partition**, honoured per `on_partition:` |
//! | aggregate certainty `< certainty_floor:` | the epistemic gate the adopter declared |
//!
//! Certainty aggregates as the **minimum** across the answering sources — the
//! conservative rule, and the language's own default (`ensemble.certainty_mode`
//! defaults to `min`). A quorum of sources is only as trustworthy as its least
//! trustworthy member.
//!
//! `provision` — materialising declared resources — **refuses**. It is the
//! infrastructure half, and it belongs to v2.67.0 (the cycle that makes `resource`
//! govern anything at all). Pretending to provision would be the same lie in the
//! other direction.

use std::collections::HashMap;

use serde_json::{Map, Value};

use super::base::{
    make_envelope, Continuation, Handler, HandlerError, HandlerOutcome,
};
use crate::ir_nodes::{IRFabric, IRManifest, IRObserve, IRResource};
use crate::source_registry::{lookup_source_adapter, SourceError};

/// The real handler: it resolves every declared source through the
/// [`crate::source_registry`], probes each one, and refuses when it cannot.
pub struct LiveHandler {
    name: String,
    /// The program's declared resources, so a source naming one can be probed by
    /// the built-in `ResourceProbeAdapter` family.
    resources: HashMap<String, IRResource>,
}

impl LiveHandler {
    pub fn new(resources: HashMap<String, IRResource>) -> Self {
        LiveHandler {
            name: "live".to_string(),
            resources,
        }
    }

    /// Parse a declared duration (`"5s"`, `"250ms"`, `"2m"`). An unparseable
    /// timeout is a refusal — we do not silently substitute a default, because a
    /// timeout is precisely the bound the adopter set on how long they are willing
    /// to be uncertain.
    fn parse_timeout(raw: &str) -> Result<std::time::Duration, HandlerError> {
        let s = raw.trim();
        if s.is_empty() {
            return Err(HandlerError::caller(
                "observe declares no `timeout:` — refusing rather than choosing one for you",
            ));
        }
        let (num, unit) = s.split_at(
            s.find(|c: char| c.is_alphabetic())
                .ok_or_else(|| HandlerError::caller(format!("malformed timeout '{s}'")))?,
        );
        let n: u64 = num
            .parse()
            .map_err(|_| HandlerError::caller(format!("malformed timeout '{s}'")))?;
        let d = match unit {
            "ms" => std::time::Duration::from_millis(n),
            "s" => std::time::Duration::from_secs(n),
            "m" => std::time::Duration::from_secs(n * 60),
            "h" => std::time::Duration::from_secs(n * 3600),
            other => {
                return Err(HandlerError::caller(format!(
                    "unknown timeout unit '{other}' in '{s}' — one of: ms | s | m | h"
                )))
            }
        };
        Ok(d)
    }
}

impl Handler for LiveHandler {
    fn name(&self) -> &str {
        &self.name
    }

    /// v2.67.0 — **refused.** Materialising a declared `resource` is the
    /// infrastructure half of the λ-L-E block, and today a `resource` governs
    /// nothing that runs (v2.67.0's islands finding: `resource.endpoint` and
    /// `axonstore.connection` are the same fact declared twice, and the discipline
    /// hangs off the copy nothing runs on). **v2.67.0** makes `resource` the single
    /// source of truth; provisioning belongs there.
    ///
    /// Refusing is the honest move. A `provision` that reported success without
    /// creating anything would be the `DryRunHandler`'s `c: 1.0` wearing a
    /// different hat.
    fn provision(
        &mut self,
        manifest: &IRManifest,
        _resources: &HashMap<String, IRResource>,
        _fabrics: &HashMap<String, IRFabric>,
        _continuation: &mut Continuation<'_>,
    ) -> Result<HandlerOutcome, HandlerError> {
        Err(HandlerError::caller(format!(
            "`provision` of manifest '{}' is not implemented. A `resource` does not yet \
             govern anything that runs — `resource.endpoint` and `axonstore.connection` are the \
             same fact declared twice, and nothing links them. The resource catalog makes \
             `resource` the single source of truth; provisioning lands there. Refusing rather \
             than reporting a success that created nothing.",
            manifest.name
        )))
    }

    /// Take a **real** quorum-gated snapshot.
    fn observe(
        &mut self,
        obs: &IRObserve,
        manifest: &IRManifest,
        _cont: &mut Continuation<'_>,
    ) -> Result<HandlerOutcome, HandlerError> {
        if obs.sources.is_empty() {
            return Err(HandlerError::caller(format!(
                "observe '{}' declares no `sources:` — there is nothing to look at, and an \
                 observation of nothing is not an observation of health",
                obs.name
            )));
        }

        let timeout = Self::parse_timeout(&obs.timeout)?;
        let quorum = obs.quorum.unwrap_or(obs.sources.len() as i64).max(1) as usize;

        let mut readings: Vec<(String, f64)> = Vec::new();
        let mut failures: Vec<String> = Vec::new();
        let mut per_source = Map::new();

        for source in &obs.sources {
            // 1. Resolve. An unregistered source is UNKNOWN — the load-bearing
            //    refusal. We do not skip it, and we do not count it.
            let Some(adapter) = lookup_source_adapter(source) else {
                return Err(HandlerError::caller(
                    SourceError::Unregistered {
                        source: source.clone(),
                    }
                    .to_string(),
                ));
            };

            // 2. Probe. The built-in family passes the declared `resource` when the
            //    source names one.
            let resource = self.resources.get(source);
            match adapter.probe(resource, timeout) {
                Ok(reading) => {
                    per_source.insert(
                        source.clone(),
                        serde_json::json!({
                            "answered": true,
                            "certainty": reading.certainty,
                            "data": Value::Object(reading.data),
                        }),
                    );
                    readings.push((source.clone(), reading.certainty));
                }
                Err(e) => {
                    // A source that did not answer does NOT count toward quorum.
                    // It is recorded, honestly, as having failed.
                    per_source.insert(
                        source.clone(),
                        serde_json::json!({ "answered": false, "error": e.to_string() }),
                    );
                    failures.push(format!("{e}"));
                }
            }
        }

        // 3. Quorum. Fewer answers than declared ⇒ a PARTITION. This is the CT-3
        //    case the language already names, and `on_partition:` already declares
        //    the intent — we honour it rather than degrading to a low-confidence
        //    "observation" of a system we could not see.
        if readings.len() < quorum {
            let detail = format!(
                "observe '{}' reached {} of {} source(s), below its declared quorum of {} \
                 [{}]",
                obs.name,
                readings.len(),
                obs.sources.len(),
                quorum,
                failures.join("; ")
            );
            return match obs.on_partition.as_str() {
                // D4: a partition is ⊥ (void) — NEVER downgraded to `doubt`.
                "fail" | "" => Err(HandlerError::network_partition(detail)),
                "shield_quarantine" => Err(HandlerError::network_partition(format!(
                    "{detail} — `on_partition: shield_quarantine`"
                ))),
                other => Err(HandlerError::caller(format!(
                    "observe '{}' declares `on_partition: {other}` — one of: fail | \
                     shield_quarantine",
                    obs.name
                ))),
            };
        }

        // 4. Aggregate certainty: the MINIMUM across the answering sources. A
        //    quorum is only as trustworthy as its least trustworthy member — and
        //    `min` is the language's own default (`ensemble.certainty_mode`).
        let certainty = readings
            .iter()
            .map(|(_, c)| *c)
            .fold(f64::INFINITY, f64::min);

        // 5. The epistemic gate the adopter declared.
        if let Some(floor) = obs.certainty_floor {
            if certainty < floor {
                return Err(HandlerError::callee(format!(
                    "observe '{}' produced certainty {certainty:.3}, below its declared \
                     `certainty_floor: {floor}` — refusing. An observation you do not trust is \
                     not an observation you may act on",
                    obs.name
                )));
            }
        }

        // v2.67.0 — **the EVIDENCE.** These are the manifest's declared resources
        // that we actually REACHED — established, not assumed.
        //
        // `reconcile` computes its Jaccard drift as
        // `jaccard(manifest.resources, resources_observed)`: the *desired* shape
        // against the *actual* one. It is the whole belief-vs-evidence gap the
        // primitive exists to close.
        //
        // Before this, no handler reported it honestly — `DryRunHandler` filled it
        // from `manifest.resources` (which is the BELIEF), and `ReconcileLoop`'s
        // fallback when it was absent was, in its own comment, to "default to
        // belief". Either path gave `jaccard(belief, belief) = 0`: **drift was
        // structurally always zero, and reconcile could never detect anything.**
        let resources_observed: Vec<Value> = readings
            .iter()
            .filter(|(name, _)| self.resources.contains_key(name))
            .filter(|(name, _)| manifest.resources.contains(name))
            .map(|(name, _)| Value::String(name.clone()))
            .collect();

        let mut data = Map::new();
        data.insert("observe".into(), obs.name.clone().into());
        data.insert("manifest".into(), manifest.name.clone().into());
        data.insert("quorum".into(), (quorum as i64).into());
        data.insert("answered".into(), (readings.len() as i64).into());
        data.insert("of".into(), (obs.sources.len() as i64).into());
        data.insert("sources".into(), Value::Object(per_source));
        data.insert(
            "resources_observed".into(),
            Value::Array(resources_observed),
        );

        let status = if failures.is_empty() { "ok" } else { "partial" };

        Ok(HandlerOutcome::new(
            "observe",
            obs.name.clone(),
            status,
            // ρ = this handler, δ = observed, c = what we actually established.
            make_envelope(certainty, &self.name, "observed", None),
            self.name.clone(),
        )
        .with_data(data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::base::{identity_continuation, HandlerErrorKind};
    use crate::source_registry::{
        register_source_adapter, ResourceProbeAdapter, SourceAdapter, SourceReading,
    };
    use std::sync::Arc;

    fn observe(name: &str, sources: Vec<&str>, quorum: Option<i64>, floor: Option<f64>) -> IRObserve {
        IRObserve {
            node_type: "observe",
            source_line: 0,
            source_column: 0,
            name: name.into(),
            target: "Infra".into(),
            sources: sources.into_iter().map(String::from).collect(),
            quorum,
            timeout: "1s".into(),
            on_partition: "fail".into(),
            certainty_floor: floor,
        }
    }

    fn manifest() -> IRManifest {
        IRManifest {
            node_type: "manifest",
            source_line: 0,
            source_column: 0,
            name: "Infra".into(),
            resources: vec!["Db".into()],
            fabric_ref: String::new(),
            region: String::new(),
            zones: None,
            compliance: Vec::new(),
        }
    }

    fn resource(name: &str, endpoint: &str) -> IRResource {
        IRResource {
            node_type: "resource",
            source_line: 0,
            source_column: 0,
            name: name.into(),
            kind: "postgres".into(),
            endpoint: endpoint.into(),
            capacity: None,
            lifetime: "affine".into(),
            certainty_floor: None,
            shield_ref: String::new(),
            within: String::new(),
        }
    }

    /// A stub adapter that always answers with a given certainty — for exercising
    /// quorum and the certainty floor without real sockets.
    struct Fixed(String, f64);
    impl SourceAdapter for Fixed {
        fn name(&self) -> &str {
            &self.0
        }
        fn probe(
            &self,
            _r: Option<&IRResource>,
            _t: std::time::Duration,
        ) -> Result<SourceReading, SourceError> {
            Ok(SourceReading::new(self.1, Map::new()))
        }
    }
    struct Down(String);
    impl SourceAdapter for Down {
        fn name(&self) -> &str {
            &self.0
        }
        fn probe(
            &self,
            _r: Option<&IRResource>,
            _t: std::time::Duration,
        ) -> Result<SourceReading, SourceError> {
            Err(SourceError::Unreachable {
                source: self.0.clone(),
                detail: "down".into(),
            })
        }
    }

    fn handler_with(res: Vec<IRResource>) -> LiveHandler {
        let map = res.into_iter().map(|r| (r.name.clone(), r)).collect();
        LiveHandler::new(map)
    }

    // ── The flagship: it actually connects ─────────────────────────────────

    /// **The line that separates v2.67.0 from everything before it.** The handler
    /// opens a real socket to a real listener and reports certainty because it
    /// *established* reachability — not because it assumed it.
    #[test]
    fn observe_reaches_a_real_endpoint_and_reports_what_it_established() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        // v2.67.0 — the resource names a config KEY (`axon-T944`); the resolver
        // turns it into the address. The probe still opens a real socket to a
        // real listener: what is observed is still what was actually reached.
        register_source_adapter(
            "LiveDb",
            Arc::new(ResourceProbeAdapter::with_resolver(
                "LiveDb",
                Arc::new(
                    crate::resource_resolver::MapResourceResolver::new()
                        .with("livedb.main", &format!("postgres://{addr}/app")),
                ),
            )),
        );

        let mut h = handler_with(vec![resource("LiveDb", "livedb.main")]);
        let out = h
            .observe(&observe("Health", vec!["LiveDb"], None, None), &manifest(), &mut identity_continuation())
            .expect("a reachable resource must be observable");

        assert_eq!(out.status, "ok");
        assert_eq!(out.envelope.c, 1.0);
        assert_eq!(out.data["answered"], 1);
    }

    // ── The refusals — each one a thing DryRunHandler got wrong ─────────────

    /// **The load-bearing refusal.** An unregistered source is UNKNOWN. The
    /// `DryRunHandler` would have returned `c: 1.0` here — a clean bill of health
    /// for a system nobody can even name a way to reach.
    #[test]
    fn an_unregistered_source_refuses() {
        let mut h = handler_with(vec![]);
        let err = h
            .observe(&observe("Health", vec!["prometheus"], None, None), &manifest(), &mut identity_continuation())
            .expect_err("an unregistered source must refuse");
        let msg = format!("{}", err.message);
        assert!(msg.contains("UNKNOWN, not healthy"), "got {msg}");
    }

    /// Fewer answers than the declared quorum is a PARTITION — CT-3, void, never
    /// downgraded to a low-confidence observation.
    #[test]
    fn below_quorum_is_a_partition_not_a_weak_observation() {
        register_source_adapter("q_up", Arc::new(Fixed("q_up".into(), 1.0)));
        register_source_adapter("q_down", Arc::new(Down("q_down".into())));

        let mut h = handler_with(vec![]);
        let err = h
            .observe(&observe("Health", vec!["q_up", "q_down"], Some(2), None), &manifest(), &mut identity_continuation())
            .expect_err("1 of 2 with quorum 2 must be a partition");

        assert_eq!(
            err.kind,
            HandlerErrorKind::NetworkPartition,
            "a partition is ⊥ (void) — it must NEVER degrade into an observation with low c"
        );
    }

    /// Certainty aggregates as the MINIMUM: a quorum is only as trustworthy as its
    /// least trustworthy member.
    #[test]
    fn certainty_is_the_minimum_across_answering_sources() {
        register_source_adapter("m_hi", Arc::new(Fixed("m_hi".into(), 0.9)));
        register_source_adapter("m_lo", Arc::new(Fixed("m_lo".into(), 0.4)));

        let mut h = handler_with(vec![]);
        let out = h
            .observe(&observe("Health", vec!["m_hi", "m_lo"], Some(2), None), &manifest(), &mut identity_continuation())
            .expect("both answered");
        assert_eq!(out.envelope.c, 0.4);
    }

    /// The epistemic gate the adopter declared. An observation you do not trust is
    /// not one you may act on.
    #[test]
    fn certainty_below_the_declared_floor_refuses() {
        register_source_adapter("f_weak", Arc::new(Fixed("f_weak".into(), 0.5)));

        let mut h = handler_with(vec![]);
        let err = h
            .observe(&observe("Health", vec!["f_weak"], None, Some(0.8)), &manifest(), &mut identity_continuation())
            .expect_err("c=0.5 under floor 0.8 must refuse");
        assert!(format!("{}", err.message).contains("certainty_floor"));
    }

    /// A partial answer that still meets quorum is honestly labelled `partial` —
    /// not `ok`. The status tells the truth about what was reached.
    #[test]
    fn a_partial_quorum_is_labelled_partial_not_ok() {
        register_source_adapter("p_up", Arc::new(Fixed("p_up".into(), 1.0)));
        register_source_adapter("p_down", Arc::new(Down("p_down".into())));

        let mut h = handler_with(vec![]);
        let out = h
            .observe(&observe("Health", vec!["p_up", "p_down"], Some(1), None), &manifest(), &mut identity_continuation())
            .expect("quorum 1 of 2 is met");
        assert_eq!(out.status, "partial", "one source is down — say so");
        assert_eq!(out.data["answered"], 1);
        assert_eq!(out.data["of"], 2);
    }

    /// `provision` refuses rather than reporting a success that created nothing.
    #[test]
    fn provision_refuses_and_points_at_113() {
        let mut h = handler_with(vec![]);
        let err = h
            .provision(
                &manifest(),
                &HashMap::new(),
                &HashMap::new(),
                &mut identity_continuation(),
            )
            .expect_err("provision must refuse");
        assert!(format!("{}", err.message).contains("single source of truth"));
    }
}
