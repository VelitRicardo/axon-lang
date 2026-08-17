//! AXON Audit Evidence Engine — GapAnalyzer
//!
//! Direct port of `axon/runtime/esk/audit_engine/gap_analyzer.py`.
//!
//! Runs the framework catalog against a compiled `IRProgram` and returns
//! a deterministic gap analysis categorising each control as
//! `ready` / `pending_code` / `pending_external`.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::ir_nodes::IRProgram;

use super::frameworks::{Control, EvidenceKind, FrameworkId, controls_for};

/// Per-control verdict.
#[derive(Debug, Clone, Serialize)]
pub struct ControlAssessment {
    pub control_id: String,
    pub title: String,
    pub axon_primitive: String,
    pub evidence_kind: String,
    pub evidence_locator: String,
    pub status: String,
    pub rationale: String,
}

/// Full gap analysis for one framework.
#[derive(Debug, Clone)]
pub struct GapAnalysis {
    pub framework: String,
    pub total_controls: usize,
    pub ready: usize,
    pub pending_code: usize,
    pub pending_external: usize,
    pub assessments: Vec<ControlAssessment>,
    pub missing_features: Vec<String>,
    /// §Fase 111 F8 — controls this catalog claims are held by a
    /// `RuntimeInvariant` whose kernel is orphaned (no production caller) or
    /// absent (the cited symbol does not exist). **These are OUR defect, not
    /// the adopter's.** They can never be `ready` until the kernel is wired.
    pub unbacked_runtime_claims: Vec<String>,
    /// §Fase 111 F8 — features the program legitimately DECLARES and that
    /// AXON does not enforce. **This is the sentence an adopter most needs to
    /// read**: you wrote `lease`, and we do not run it.
    pub declared_but_unenforced: Vec<String>,
}

impl GapAnalysis {
    pub fn readiness_percent(&self) -> f64 {
        if self.total_controls == 0 {
            return 100.0;
        }
        100.0 * self.ready as f64 / self.total_controls as f64
    }

    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("schema".into(), "axon.esk.audit_gap_analysis.v1".into());
        m.insert("framework".into(), self.framework.clone().into());
        m.insert("total_controls".into(), (self.total_controls as i64).into());
        m.insert("ready".into(), (self.ready as i64).into());
        m.insert("pending_code".into(), (self.pending_code as i64).into());
        m.insert("pending_external".into(), (self.pending_external as i64).into());
        // Python uses `round(x, 2)`; serde_json will serialise f64 faithfully.
        let pct = (self.readiness_percent() * 100.0).round() / 100.0;
        m.insert("readiness_percent".into(), Value::from(pct));
        m.insert(
            "missing_features".into(),
            Value::Array(
                self.missing_features.iter().cloned().map(Value::String).collect(),
            ),
        );
        // §111 F8 — both lists ride the wire so a consumer (CLI, evidence
        // package, adopter dashboard) cannot render a readiness percentage
        // without also being able to render what we do not back.
        m.insert(
            "unbacked_runtime_claims".into(),
            Value::Array(
                self.unbacked_runtime_claims.iter().cloned().map(Value::String).collect(),
            ),
        );
        m.insert(
            "declared_but_unenforced".into(),
            Value::Array(
                self.declared_but_unenforced.iter().cloned().map(Value::String).collect(),
            ),
        );
        m.insert(
            "assessments".into(),
            Value::Array(
                self.assessments
                    .iter()
                    .map(|a| {
                        let mut am = Map::new();
                        am.insert("control_id".into(), a.control_id.clone().into());
                        am.insert("title".into(), a.title.clone().into());
                        am.insert("axon_primitive".into(), a.axon_primitive.clone().into());
                        am.insert("evidence_kind".into(), a.evidence_kind.clone().into());
                        am.insert("evidence_locator".into(), a.evidence_locator.clone().into());
                        am.insert("status".into(), a.status.clone().into());
                        am.insert("rationale".into(), a.rationale.clone().into());
                        Value::Object(am)
                    })
                    .collect(),
            ),
        );
        Value::Object(m)
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Feature detection
// ═══════════════════════════════════════════════════════════════════

/// Features the program **declares in source**.
///
/// §Fase 111 F8 — declaring is not enforcing. This set answers "what is
/// written in the program", NOT "what actually runs". Control requirements are
/// checked against [`enforced_features`]; this set exists only so the analyzer
/// can tell an auditor the difference between *you did not declare it* and
/// *you declared it and AXON does not enforce it*.
fn declared_features(program: &IRProgram) -> HashSet<String> {
    let mut features: HashSet<String> = HashSet::new();
    if !program.shields.is_empty()      { features.insert("has_shield".into()); }
    if !program.resources.is_empty()    { features.insert("has_resource".into()); }
    if !program.manifests.is_empty()    { features.insert("has_manifest".into()); }
    if !program.observations.is_empty() { features.insert("has_observe".into()); }
    if !program.immunes.is_empty()      { features.insert("has_immune".into()); }
    if !program.reflexes.is_empty()     { features.insert("has_reflex".into()); }
    if !program.heals.is_empty()        { features.insert("has_heal".into()); }
    if !program.reconciles.is_empty()   { features.insert("has_reconcile".into()); }
    if !program.leases.is_empty()       { features.insert("has_lease".into()); }
    if !program.ensembles.is_empty()    { features.insert("has_ensemble".into()); }
    if !program.topologies.is_empty()   { features.insert("has_topology".into()); }
    if !program.endpoints.is_empty()    { features.insert("has_endpoint".into()); }

    // §124.c — was `has_compliance_annotation`, granted on the mere PRESENCE
    // of a label anywhere; C1.1 / P1.1 / P6.1 / A.5.34 counted as satisfied
    // by a κ-annotated type flowing through a shield that covered nothing.
    // The feature now means the coverage rule HOLDS: every regulated
    // boundary names a shield whose κ covers the data's κ. One definition,
    // `coverage.rs`, shared with the risk register (§120.e).
    if super::coverage::compliance_coverage_holds(program) {
        features.insert("compliance_coverage_holds".into());
    }
    features
}

/// Features the program declares AND that AXON actually enforces at runtime.
///
/// §Fase 111 F8. The Cognitive-I/O family (`observe`/`reconcile`/`lease`/
/// `ensemble`/`immune`/`reflex`/`heal`/`resource`) parses, type-checks and
/// reaches the IR — and then nothing dispatches it (there is no `IRFlowNode`
/// arm for any of them, and no runtime path reads
/// `IRProgram.{observations,leases,heals,…}`). A control may only be satisfied
/// by a feature that runs, so those are filtered out here.
///
/// Wiring one of them into the executor = delete its row from
/// `runtime_wiring::UNENFORCED_FEATURES` in the same PR, and the controls it
/// backs go `ready` on their own. That is the only sanctioned way to raise a
/// readiness score.
fn enforced_features(declared: &HashSet<String>) -> HashSet<String> {
    declared
        .iter()
        .filter(|f| super::runtime_wiring::feature_is_enforced(f))
        .cloned()
        .collect()
}

// ═══════════════════════════════════════════════════════════════════
//  Rules
// ═══════════════════════════════════════════════════════════════════

fn feature_requirements() -> HashMap<&'static str, HashSet<&'static str>> {
    let mut m: HashMap<&'static str, HashSet<&'static str>> = HashMap::new();
    // SOC 2
    m.insert("CC3.2",  HashSet::from(["has_immune"]));
    m.insert("CC3.3",  HashSet::from(["has_reflex"]));
    m.insert("CC6.3",  HashSet::from(["has_lease"]));
    m.insert("CC6.6",  HashSet::from(["has_shield", "has_endpoint"]));
    m.insert("CC6.8",  HashSet::new());
    m.insert("CC7.1",  HashSet::from(["has_immune"]));
    m.insert("CC7.2",  HashSet::from(["has_immune"]));
    m.insert("CC7.3",  HashSet::from(["has_immune"]));
    m.insert("CC7.4",  HashSet::from(["has_reflex", "has_heal"]));
    m.insert("CC7.5",  HashSet::from(["has_reconcile"]));
    m.insert("C1.1",   HashSet::from(["compliance_coverage_holds"]));
    m.insert("PI1.4",  HashSet::from(["has_ensemble"]));
    m.insert("P1.1",   HashSet::from(["compliance_coverage_holds"]));
    m.insert("P6.1",   HashSet::from(["has_shield", "compliance_coverage_holds"]));
    // ISO 27001
    m.insert("A.5.2",  HashSet::from(["has_heal"]));
    m.insert("A.5.7",  HashSet::from(["has_immune"]));
    m.insert("A.5.23", HashSet::new());
    m.insert("A.5.24", HashSet::from(["has_immune", "has_reflex", "has_heal"]));
    m.insert("A.5.30", HashSet::from(["has_reconcile"]));
    m.insert("A.5.34", HashSet::from(["compliance_coverage_holds"]));
    m.insert("A.8.2",  HashSet::from(["has_lease"]));
    m.insert("A.8.7",  HashSet::from(["has_immune", "has_reflex"]));
    m.insert("A.8.8",  HashSet::from(["has_heal"]));
    m.insert("A.8.13", HashSet::from(["has_resource"]));
    m
}

const PENDING_KEYWORD: &str = "PENDING";

fn is_external_kind(kind: EvidenceKind) -> bool {
    matches!(
        kind,
        EvidenceKind::ExternalOperational | EvidenceKind::ManualPolicy
    )
}

/// Assess one control against the program.
///
/// # §Fase 111 F8 — the unsoundness this function used to carry
///
/// Before §111 the final arm read `("ready", "enforced by {primitive}")` as
/// soon as the program *declared* the required primitive. That made the audit
/// engine assert a `RuntimeInvariant` — "a kernel enforces this while the
/// program runs" — on the strength of a declaration alone. For the whole
/// Cognitive-I/O family the kernel has **no production caller**, and for a
/// third of the catalog the cited symbol **does not exist at all** (dangling
/// Python anchors from the removed `axon/runtime/` tree).
///
/// The consequence was concrete and serious: writing `lease Foo { … }` and
/// never running it — never being *able* to run it — marked SOC2 CC6.3
/// `ready`, which `control_statements` then renders to an auditor as
/// **"implemented"**. A customer could pass a compliance gate we generated and
/// that we cannot back.
///
/// Now: a `RuntimeInvariant` control is `ready` only if
/// [`runtime_wiring::Wiring::is_enforced`] holds for its evidence locator.
/// Everything else fails CLOSED, and says why in the rationale — including the
/// case that matters most to a user, *"you declared it; we do not enforce it"*,
/// which is a different (and far more actionable) sentence than *"you did not
/// declare it"*.
fn assess_control(
    control: &Control,
    declared: &HashSet<String>,
    enforced: &HashSet<String>,
) -> ControlAssessment {
    let locator = control.evidence_locator;
    let is_pending = locator.contains(PENDING_KEYWORD);
    let reqs = feature_requirements();
    let required: HashSet<&str> = reqs.get(control.control_id).cloned().unwrap_or_default();

    // Split the unmet requirements into the two honest categories.
    let mut not_declared: Vec<&str> = required
        .iter()
        .filter(|f| !declared.contains(**f))
        .copied()
        .collect();
    let mut declared_but_unenforced: Vec<&str> = required
        .iter()
        .filter(|f| declared.contains(**f) && !enforced.contains(**f))
        .copied()
        .collect();
    not_declared.sort();
    declared_but_unenforced.sort();

    // §111 F8 — a runtime claim is only as good as its wire.
    let wiring = super::runtime_wiring::wiring_or_absent(locator);
    let runtime_claim_unbacked =
        control.evidence_kind == EvidenceKind::RuntimeInvariant && !wiring.is_enforced();

    let (status, rationale) = if is_pending && is_external_kind(control.evidence_kind) {
        (
            "pending_external",
            format!("requires external engagement (accredited lab / CPA) — {locator}"),
        )
    } else if is_pending {
        (
            "pending_code",
            format!("evidence artifact not yet produced — {locator}"),
        )
    } else if runtime_claim_unbacked {
        // The load-bearing arm. Never `ready`, whatever the program declares.
        ("pending_code", wiring.rationale(locator))
    } else if !declared_but_unenforced.is_empty() {
        (
            "pending_code",
            format!(
                "program declares {} but AXON does not enforce {} at runtime — \
                 the primitive has no dispatch path (§111 F14); declaring it is not evidence",
                declared_but_unenforced.join(", "),
                if declared_but_unenforced.len() == 1 { "it" } else { "them" },
            ),
        )
    } else if !not_declared.is_empty() {
        (
            "pending_code",
            format!(
                "program does not declare required primitive(s): {}",
                not_declared.join(", ")
            ),
        )
    } else if control.evidence_kind == EvidenceKind::ExternalOperational {
        ("ready", format!("operational artifact: {locator}"))
    } else if control.evidence_kind == EvidenceKind::RuntimeInvariant {
        ("ready", wiring.rationale(locator))
    } else {
        ("ready", format!("enforced by {}", control.axon_primitive))
    };

    ControlAssessment {
        control_id: control.control_id.into(),
        title: control.title.into(),
        axon_primitive: control.axon_primitive.into(),
        evidence_kind: control.evidence_kind.as_str().into(),
        evidence_locator: locator.into(),
        status: status.into(),
        rationale,
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Public API
// ═══════════════════════════════════════════════════════════════════

pub fn analyze_gaps(program: &IRProgram, framework: FrameworkId) -> GapAnalysis {
    let declared = declared_features(program);
    let enforced = enforced_features(&declared);
    let controls = controls_for(framework);
    let mut analysis = GapAnalysis {
        framework: framework.as_str().into(),
        total_controls: controls.len(),
        ready: 0,
        pending_code: 0,
        pending_external: 0,
        assessments: Vec::new(),
        missing_features: Vec::new(),
        unbacked_runtime_claims: Vec::new(),
        declared_but_unenforced: Vec::new(),
    };
    let reqs = feature_requirements();

    // §111 F8 — surface the defect LOUDLY. A control that silently drops out
    // of `ready` is an improvement over a false `ready`, but it still leaves
    // the reader guessing. These two lists say the quiet part out loud:
    // which of OUR runtime claims are unbacked, and which of THEIR
    // declarations we do not honour.
    let mut declared_unenforced: Vec<String> = declared
        .iter()
        .filter(|f| !enforced.contains(*f))
        .cloned()
        .collect();
    declared_unenforced.sort();
    analysis.declared_but_unenforced = declared_unenforced;

    for c in &controls {
        if c.evidence_kind == EvidenceKind::RuntimeInvariant
            && !super::runtime_wiring::wiring_or_absent(c.evidence_locator).is_enforced()
        {
            analysis.unbacked_runtime_claims.push(format!(
                "{} ({})",
                c.control_id, c.evidence_locator
            ));
        }

        let a = assess_control(c, &declared, &enforced);
        match a.status.as_str() {
            "ready" => analysis.ready += 1,
            "pending_code" => {
                analysis.pending_code += 1;
                if let Some(req) = reqs.get(c.control_id) {
                    for feat in req {
                        if !enforced.contains(*feat)
                            && !analysis.missing_features.contains(&feat.to_string())
                        {
                            analysis.missing_features.push(feat.to_string());
                        }
                    }
                }
            }
            "pending_external" => analysis.pending_external += 1,
            _ => {}
        }
        analysis.assessments.push(a);
    }
    analysis
}

pub fn analyze_all(program: &IRProgram) -> BTreeMap<String, GapAnalysis> {
    let mut m = BTreeMap::new();
    for f in super::frameworks::all_frameworks() {
        m.insert(f.as_str().into(), analyze_gaps(program, f));
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir_generator::IRGenerator;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn compile(source: &str) -> IRProgram {
        let tokens = Lexer::new(source, "t").tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        IRGenerator::new().generate(&program)
    }

    #[test]
    fn empty_program_counts_sum_to_total_controls() {
        let ir = compile("type X { field: String }");
        let gap = analyze_gaps(&ir, FrameworkId::Soc2TypeII);
        assert_eq!(
            gap.ready + gap.pending_code + gap.pending_external,
            gap.total_controls
        );
        // At least the feature-gated controls must be pending_code on an
        // empty program (no immune / heal / reflex / ...).
        assert!(gap.pending_code > 0);
    }

    /// **The F8 loop, closing.** This test has now been the record of three
    /// different truths, and that history is the point.
    ///
    /// - **Before §111** it asserted this program made **≥25 SOC2 controls
    ///   `ready`** — and *that assertion was the bug*, written down and pinned by a
    ///   passing test. None of the stack ran. A compliance posture built on it was
    ///   a certificate we could not back.
    /// - **§111 (F8)** inverted it: declaring an **unenforced** primitive must never
    ///   buy a `ready`. The engine stopped lying.
    /// - **§112** built the supervisor that actually drives six of them. So now the
    ///   engine can also **say yes** — *for exactly the six that run, and not one
    ///   more.*
    ///
    /// That last step is the whole reason F8 was worth doing. A compliance engine
    /// that can only ever say "no" is honest and useless; one that says "yes" for
    /// things that do not run is worse than useless. **This asserts it says yes for
    /// precisely what it can back.**
    #[test]
    fn the_cognitive_io_stack_buys_credit_for_exactly_what_runs() {
        let stack = r#"
            fabric Vpc { provider: aws }
            resource Db { kind: postgres endpoint: db.main within: Vpc lifetime: affine capacity: 20 }
            axonstore Users { backend: postgresql resource: Db }
            manifest M { resources: [Db] fabric: Vpc compliance: [HIPAA] }
            observe O from M { sources: [prom] quorum: 1 }
            observe O2 from M { sources: [cw] quorum: 1 }
            reconcile Rec { observe: O }
            lease L { resource: Db duration: 30m }
            ensemble En { observations: [O, O2] quorum: 2 }
            immune I { watch: [O] scope: tenant }
            reflex Rf { trigger: I on_level: doubt action: quarantine scope: tenant }
            heal H { source: I scope: tenant }
        "#;
        let base = "type R compliance [HIPAA] { x: String }
                    flow F(r: R) -> R { step S { ask: \"x\" output: R } }
";

        let without = analyze_gaps(&compile(base), FrameworkId::Soc2TypeII);
        let with = analyze_gaps(&compile(&format!("{base}{stack}")), FrameworkId::Soc2TypeII);

        // §112 — the six primitives the CognitiveIoSupervisor drives are ENFORCED, so
        // declaring them legitimately raises the posture. The engine can say yes.
        assert!(
            with.ready > without.ready,
            "the six Cognitive-I/O primitives §112 wired (observe/ensemble/immune/reflex/heal/             reconcile) are driven by the supervisor through the real deploy path. Declaring them              MUST now raise the score — an engine that can only say no is honest and useless.              ready went {} -> {}",
            without.ready,
            with.ready
        );

        // §113 — and now `lease` and `resource` buy credit too, because they finally
        // govern something. An `axonstore { resource: Db }` derives its DSN and its
        // POOL SIZE from the resource, and `lease`'s CT-2 Anchor Breach fires on the
        // store operation, which IS the use of the resource.
        //
        // The §111 version of this block asserted the exact OPPOSITE, and was right
        // to: back then the guarantee was structurally impossible, not merely
        // unwired — and an impossible guarantee cannot be KEPT, not merely broken.
        //
        // NOTHING declared here is unenforced any more. That sentence is the whole
        // arc of §111 → §112 → §113.
        assert!(
            with.declared_but_unenforced.is_empty(),
            "as of §113 every primitive in this stack is on a production path; the analysis              must report NOTHING as declared-but-unenforced. Got: {:?}",
            with.declared_but_unenforced
        );
        for f in ["has_lease", "has_resource", "has_immune", "has_heal", "has_reconcile", "has_ensemble", "has_observe"] {
            assert!(
                !with.declared_but_unenforced.contains(&f.to_string()),
                "`{f}` runs on a production path (§112 supervisor / §113 resource+lease) — it                  must NOT be reported as unenforced"
            );
        }

        // And no control may still be `ready` on an unbacked runtime invariant.
        for a in &with.assessments {
            if a.evidence_kind == "runtime_invariant"
                && !crate::esk::audit_engine::runtime_wiring::wiring_or_absent(&a.evidence_locator)
                    .is_enforced()
            {
                assert_ne!(
                    a.status, "ready",
                    "control {} is `ready` on an unbacked runtime invariant (`{}`)",
                    a.control_id, a.evidence_locator
                );
            }
        }
    }

    /// 🎯 §124.c — C1.1 (confidentiality commitments) is satisfied by the
    /// coverage rule HOLDING, not by a label being present. Pre-§124.c both
    /// programs below scored identically on every control that named
    /// `has_compliance_annotation`.
    #[test]
    fn c11_requires_coverage_not_presence() {
        let status_of = |ir: &IRProgram| {
            analyze_gaps(ir, FrameworkId::Soc2TypeII)
                .assessments
                .iter()
                .find(|a| a.control_id == "C1.1")
                .expect("SOC2 catalog carries C1.1")
                .status
                .clone()
        };

        let label_only = compile("type Phi compliance [HIPAA] { x: String }");
        assert_ne!(
            status_of(&label_only),
            "ready",
            "a κ label that crosses no boundary satisfies no confidentiality control"
        );

        let covered = compile(
            r#"
            type Phi compliance [HIPAA] { x: String }
            shield Gate { scan: [pii_leak] compliance: [HIPAA] }
            channel PhiFeed { message: Phi shield: Gate }
            "#,
        );
        assert_eq!(
            status_of(&covered),
            "ready",
            "coverage holding at every regulated boundary is what `ready` means here"
        );
    }

    /// The other half of fail-closed: a control whose runtime kernel IS wired
    /// must still be able to reach `ready`. A fix that made everything pending
    /// would be honest and useless.
    #[test]
    fn a_wired_runtime_invariant_can_still_be_ready() {
        let ir = compile(
            r#"
            type R { x: String }
            flow F(r: R) -> R { step S { ask: "x" output: R } }
        "#,
        );
        let gap = analyze_gaps(&ir, FrameworkId::Soc2TypeII);
        let wired_ready = gap.assessments.iter().any(|a| {
            a.evidence_kind == "runtime_invariant"
                && a.status == "ready"
                && crate::esk::audit_engine::runtime_wiring::wiring_or_absent(&a.evidence_locator)
                    .is_enforced()
        });
        assert!(
            wired_ready,
            "no wired RuntimeInvariant reached `ready` — the ProvenanceChain / HmacSigner \
             audit chain IS on the production path and must still count"
        );
    }

    #[test]
    fn fips_has_pending_external_for_lab_engagement() {
        let ir = compile("type T { x: String }");
        let gap = analyze_gaps(&ir, FrameworkId::Fips140_3);
        assert!(
            gap.pending_external > 0,
            "FIPS must carry ≥1 pending_external entry"
        );
    }

    #[test]
    fn analyze_all_covers_every_framework() {
        let ir = compile("type T { x: String }");
        let all = analyze_all(&ir);
        assert_eq!(all.len(), 4);
        for fw in super::super::frameworks::all_frameworks() {
            assert!(all.contains_key(fw.as_str()));
        }
    }

    #[test]
    fn readiness_percent_sane_bounds() {
        let ir = compile("type T { x: String }");
        let gap = analyze_gaps(&ir, FrameworkId::Iso27001);
        let pct = gap.readiness_percent();
        assert!((0.0..=100.0).contains(&pct));
    }

    #[test]
    fn gap_analysis_deterministic() {
        let ir = compile(r#"
            type T compliance [GDPR] { x: String }
            resource R { kind: postgres lifetime: linear }
        "#);
        let a = analyze_gaps(&ir, FrameworkId::Iso27001).to_value();
        let b = analyze_gaps(&ir, FrameworkId::Iso27001).to_value();
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }
}
