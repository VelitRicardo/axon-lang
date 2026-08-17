//! AXON Audit Evidence Engine — Runtime Wiring Truth Table (§Fase 111, F8)
//!
//! # Why this exists
//!
//! The framework catalog ([`super::frameworks`]) classifies many controls as
//! [`EvidenceKind::RuntimeInvariant`] — meaning: *"this control is satisfied
//! because a kernel enforces it while the program runs."* The gap analyzer
//! then marked such a control `ready` as soon as the program **declared** the
//! corresponding primitive.
//!
//! §Fase 111 found that claim to be **unsound**. Three distinct lies were
//! being told, and the audit engine could not tell them apart:
//!
//! 1. **[`Wiring::Wired`]** — the kernel exists AND a production path calls
//!    it. The invariant genuinely holds at runtime. (Example:
//!    `ProvenanceChain` / `HmacSigner`, reached from
//!    `store::audit_chain`, which every `persist` / `mutate` / `purge`
//!    appends to.)
//!
//! 2. **[`Wiring::Orphaned`]** — the kernel EXISTS, is well-written, is
//!    unit-tested … and **has no production caller**. `axon-rs/src/runtime/*`
//!    is an island whose only callers are its own `#[cfg(test)]` blocks.
//!    Declaring `lease`/`heal`/`reconcile` in a program does **not** cause
//!    `LeaseKernel`/`HealKernel`/`ReconcileLoop` to run — nothing dispatches
//!    them. The math is real; the wire is not.
//!
//! 3. **[`Wiring::Absent`]** — the cited symbol **does not exist in any
//!    language**. The locators are Python module paths
//!    (`axon.runtime.esk.PrivacyBudget`, `tests/test_phase6_runtime.py`)
//!    inherited from a pre-Rust port; `axon/` today holds only `__init__.py`
//!    and `_bootstrap.py`. For these, the only place the name appears in the
//!    entire tree is the evidence string in `frameworks.rs` itself.
//!
//! # The invariant this module establishes
//!
//! **Only [`Wiring::Wired`] evidence may make a `RuntimeInvariant` control
//! `ready`.** An orphaned or absent kernel is not evidence of anything; a
//! compliance posture built on it is a certificate we cannot back in front of
//! an auditor. Everything else fails CLOSED — see
//! [`super::gap_analyzer::assess_control`].
//!
//! # The gate that keeps it honest
//!
//! [`WIRING_TABLE`] is a **closed** table and `wiring_of` has **no default
//! arm**: an unknown locator returns `None`, and the test
//! `every_runtime_invariant_locator_is_classified` fails the build. A new
//! `RuntimeInvariant` control therefore cannot be added without a human
//! stating, on the record, whether its kernel is actually wired. That test is
//! the durable half of this fix — the code below is only the snapshot.
//!
//! When a §111.x sub-fase wires one of the orphaned kernels into the executor,
//! flip its row to `Wired` **in the same PR** and the controls it backs become
//! `ready` automatically. That is the intended, and only, way to raise the
//! score.

#![allow(dead_code)]

/// Whether the runtime evidence a control cites is actually reachable from a
/// production code path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wiring {
    /// The kernel exists and a production path calls it. The cited Rust
    /// symbol is carried so the evidence package can point at real code.
    Wired(&'static str),
    /// The kernel exists but has NO production caller (callers are its own
    /// tests). Carries the module so a §111.x sub-fase knows what to wire.
    Orphaned(&'static str),
    /// The cited symbol does not exist anywhere in the tree — a dangling
    /// anchor inherited from the deleted Python runtime.
    Absent,
}

impl Wiring {
    /// The ONLY predicate that may promote a `RuntimeInvariant` control to
    /// `ready`. Deliberately total and deliberately strict.
    pub fn is_enforced(self) -> bool {
        matches!(self, Wiring::Wired(_))
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Wiring::Wired(_) => "wired",
            Wiring::Orphaned(_) => "orphaned",
            Wiring::Absent => "absent",
        }
    }

    /// Auditor-facing explanation of why this evidence does (not) count.
    pub fn rationale(self, locator: &str) -> String {
        match self {
            Wiring::Wired(sym) => format!("enforced at runtime by `{sym}`"),
            Wiring::Orphaned(module) => format!(
                "NOT ENFORCED — `{locator}` exists in `{module}` but has no production caller; \
                 declaring the primitive does not cause it to run (§111 F8)"
            ),
            Wiring::Absent => format!(
                "NOT ENFORCED — `{locator}` does not exist in the codebase; \
                 dangling evidence anchor inherited from the removed Python runtime (§111 F8)"
            ),
        }
    }
}

/// The closed truth table: every `RuntimeInvariant` evidence locator in the
/// framework catalog, and whether it is really wired.
///
/// Verified 2026-07-13 by walking every caller of each symbol across
/// `axon-rs/src/` and excluding `#[cfg(test)]` blocks.
const WIRING_TABLE: &[(&str, Wiring)] = &[
    // ── WIRED — real kernels on a real production path ───────────────────
    //
    // The HMAC-Merkle provenance chain is the one member of the ESK family
    // that made it into the executor: `store::audit_chain` wraps it and every
    // `persist` / `mutate` / `purge` appends a signed entry.
    ("ProvenanceChain", Wiring::Wired("esk::provenance::ProvenanceChain (via store::audit_chain)")),
    ("ProvenanceChain.append", Wiring::Wired("esk::provenance::ProvenanceChain::append (via store::audit_chain)")),
    ("axon.runtime.esk.ProvenanceChain", Wiring::Wired("esk::provenance::ProvenanceChain (via store::audit_chain)")),
    ("axon.runtime.esk.provenance", Wiring::Wired("esk::provenance (via store::audit_chain)")),
    // The locator is a stale Python path, but the kernel it names was really
    // ported and really runs — so the CONTROL is backed. The catalog's locator
    // string still wants correcting (§111 F6 doc sweep); the wiring does not.
    ("provenance.py", Wiring::Wired("esk::provenance (via store::audit_chain)")),
    ("HmacSigner", Wiring::Wired("esk::provenance::HmacSigner (via store::audit_chain)")),
    ("HmacSigner.random", Wiring::Wired("esk::provenance::HmacSigner::random (via store::audit_chain)")),
    ("HmacSigner.verify", Wiring::Wired("esk::provenance::HmacSigner::verify (via store::audit_chain)")),

    // ── WIRED by §Fase 112 — the loop that was never built ────────────────
    //
    // §111 F9 called these "real engine, dead wire": genuinely implemented,
    // unit-tested, and reachable by nobody. I diagnosed that as a LANGUAGE
    // problem. It was not — the declarations already formed a dataflow graph
    // (`immune.watch: [<observe>]`, `reflex.trigger: <immune>`, …) and the
    // kernels took the compiled IR **directly**. What was missing was the
    // supervisor that hands them the graph and drives it.
    //
    // §112 built it (`cognitive_io_supervisor`), and every row below now cites a
    // gate proving it runs through the REAL deploy path. Two of them were also
    // WRONG, and only running them could tell you: the KL sensor scored an unseen
    // symbol against the baseline's MINIMUM probability (so a perfectly stable
    // baseline could never register an anomaly), and `ReconcileLoop` defaulted its
    // "observed" state to the manifest's own declaration (so drift was structurally
    // always zero). An orphaned kernel is not idle code — it is code whose
    // correctness was never tested against reality, and it rots.
    ("AnomalyDetector", Wiring::Wired("runtime::immune::detector (via cognitive_io_supervisor, §112.d)")),
    ("axon.runtime.immune.AnomalyDetector", Wiring::Wired("runtime::immune::detector (via cognitive_io_supervisor, §112.d)")),
    ("AnomalyDetector + ReflexEngine", Wiring::Wired("runtime::immune::{detector,reflex} (via cognitive_io_supervisor, §112.d)")),
    ("axon.runtime.immune + esk.eid", Wiring::Wired("runtime::immune (via cognitive_io_supervisor, §112.d)")),
    ("HealKernel", Wiring::Wired("runtime::immune::heal (via cognitive_io_supervisor, §112.d)")),
    ("axon.runtime.immune.HealKernel", Wiring::Wired("runtime::immune::heal (via cognitive_io_supervisor, §112.d)")),
    ("HealDefinition.mode", Wiring::Wired("runtime::immune::heal (via cognitive_io_supervisor, §112.d)")),
    // §113.d — the lease has a USE-SITE: `StoreRegistry::charge_lease` runs on every
    // `resolve()` of a leased store, so a store operation IS a use of the resource and
    // the post-expiry one is the CT-2 Anchor Breach.
    ("LeaseKernel", Wiring::Wired("store::registry::charge_lease")),
    ("axon.runtime.lease_kernel.LeaseKernel", Wiring::Wired("store::registry::charge_lease")),
    ("ReconcileLoop", Wiring::Wired("runtime::reconcile_loop (via cognitive_io_supervisor, §112.e)")),
    ("axon.runtime.reconcile_loop.ReconcileLoop", Wiring::Wired("runtime::reconcile_loop (via cognitive_io_supervisor, §112.e)")),
    ("axon.runtime.ensemble_aggregator", Wiring::Wired("runtime::ensemble_aggregator (via cognitive_io_supervisor, §112.c)")),

    // ── ABSENT — the cited symbol does not exist in any language ──────────
    //
    // These names occur NOWHERE in the tree except as the evidence strings in
    // `frameworks.rs`. They are Python paths from a runtime that was deleted
    // in the Rust port. Citing them to an auditor is citing nothing.
    ("ReflexEngine", Wiring::Absent),
    ("axon.runtime.immune.ReflexEngine", Wiring::Absent),
    ("EpistemicIntrusionDetector.observe", Wiring::Absent),
    ("axon.runtime.esk.EpistemicIntrusionDetector", Wiring::Absent),
    ("axon.runtime.esk.eid.IntrusionEvent", Wiring::Absent),
    ("axon.runtime.esk.PrivacyBudget", Wiring::Absent),
    ("axon.runtime.esk.privacy", Wiring::Absent),
    ("gaussian_noise + PrivacyBudget", Wiring::Absent),
    ("laplace_noise", Wiring::Absent),
    ("laplace_noise / gaussian_noise", Wiring::Absent),
    ("Secret", Wiring::Absent),
    ("Secret[T] invariant", Wiring::Absent),
    ("Secret.audit_trail", Wiring::Absent),
    ("SecretAccess", Wiring::Absent),
    ("axon.runtime.esk.Secret", Wiring::Absent),
    ("ShieldDefinition + Secret", Wiring::Absent),
    ("LeaseKernel + Secret.audit_trail", Wiring::Absent),
    ("tests/test_phase6_runtime.py::TestSecret", Wiring::Absent),
    ("LambdaEnvelope.tau", Wiring::Absent),
    ("NetworkPartitionError", Wiring::Absent),
    ("axon.runtime.handlers", Wiring::Absent),
    ("axon.runtime.handlers.base", Wiring::Absent),
];

/// Classify one `RuntimeInvariant` evidence locator.
///
/// Returns `None` for an unknown locator — **deliberately not a default
/// arm**. The build gate below turns that `None` into a test failure, so a
/// new control cannot smuggle in an unclassified runtime claim.
pub fn wiring_of(locator: &str) -> Option<Wiring> {
    WIRING_TABLE
        .iter()
        .find(|(l, _)| *l == locator)
        .map(|(_, w)| *w)
}

/// Fail-CLOSED accessor used by the gap analyzer: an unknown locator is
/// treated as [`Wiring::Absent`], never as enforced. The build gate makes the
/// unknown case unreachable in practice; this keeps the runtime path honest
/// even if it ever were.
pub fn wiring_or_absent(locator: &str) -> Wiring {
    wiring_of(locator).unwrap_or(Wiring::Absent)
}

/// The program features whose enforcement depends on an orphaned/absent
/// kernel. A program that DECLARES these does not thereby ENFORCE them, so
/// they must not satisfy a control requirement.
///
/// This is the feature-level projection of the table above: every one of
/// these maps to a Cognitive-I/O primitive with no `IRFlowNode` dispatch arm
/// (§111 F14). Wiring any of them into the executor means deleting its row
/// here in the same PR.
const UNENFORCED_FEATURES: &[&str] = &[
    // §112 removed SIX (`has_observe`, `has_reconcile`, `has_ensemble`, `has_immune`,
    // `has_reflex`, `has_heal`) — driven by the `CognitiveIoSupervisor`, each with a
    // gate proving it runs through the REAL deploy path.
    //
    // §113 removed the last TWO — `has_lease` and `has_resource`.
    //
    //   `has_resource`: a `resource` now governs what runs. An `axonstore
    //   { resource: Db }` DERIVES its DSN and its POOL SIZE from it (`capacity: 20`
    //   ⇒ twenty connections; before §113 that field was read by ZERO lines of code
    //   while every pool sat at a hardcoded 10), and `lifetime` decides how many
    //   holders may name it (axon-T945).
    //
    //   `has_lease`: the CT-2 Anchor Breach fires. The kernel was never broken — it
    //   had no SUBJECT. A flow could not USE a resource, so a guarantee about
    //   post-expiry USE was vacuous: unviolatable, and therefore unkeepable. §113
    //   made the store operation the use.
    //
    // THIS LIST IS NOW EMPTY, AND THAT IS THE POINT OF F8.
    //
    // §111 found this engine UNSOUND in the worst possible direction: declaring a
    // DEAD primitive RAISED your SOC2 score. §111 made it stop lying — every one of
    // these features bought exactly nothing. §112 and §113 let it start saying YES,
    // and every yes now cites a kernel on a production path.
    //
    // An empty list is not a licence. A NEW feature whose kernel does not run belongs
    // here, immediately, in the same PR that declares it.
];

/// Is this program feature backed by a kernel that actually runs?
///
/// `has_shield`, `has_endpoint`, `has_topology`, `has_manifest` and
/// `compliance_coverage_holds` are compile-time or genuinely-wired surfaces
/// and remain enforceable; the Cognitive-I/O family does not.
pub fn feature_is_enforced(feature: &str) -> bool {
    !UNENFORCED_FEATURES.contains(&feature)
}

/// Every feature that a program may declare but that AXON does not enforce.
/// Surfaced in the gap analysis so the defect is loud rather than merely
/// absent from the `ready` count.
pub fn unenforced_features() -> &'static [&'static str] {
    UNENFORCED_FEATURES
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::frameworks::{EvidenceKind, all_frameworks, controls_for};

    /// **The durable half of the §111 F8 fix.**
    ///
    /// Every `RuntimeInvariant` control in the catalog must have an explicit
    /// row in [`WIRING_TABLE`]. Adding a control that claims a runtime
    /// invariant without stating whether its kernel is wired is a BUILD
    /// FAILURE — which is exactly how `LeaseKernel` should have been caught
    /// years ago.
    #[test]
    fn every_runtime_invariant_locator_is_classified() {
        let mut unclassified: Vec<String> = Vec::new();
        for fw in all_frameworks() {
            for c in controls_for(fw) {
                if c.evidence_kind == EvidenceKind::RuntimeInvariant
                    && wiring_of(c.evidence_locator).is_none()
                {
                    unclassified.push(format!(
                        "{}:{} → `{}`",
                        fw.as_str(),
                        c.control_id,
                        c.evidence_locator
                    ));
                }
            }
        }
        assert!(
            unclassified.is_empty(),
            "RuntimeInvariant control(s) cite evidence with no entry in WIRING_TABLE.\n\
             You must state, on the record, whether the kernel is Wired / Orphaned / Absent.\n\
             An unclassified runtime claim is how §111 F8 happened.\n{}",
            unclassified.join("\n")
        );
    }

    /// A `Wired` row must name a real Rust symbol, not a Python path. The
    /// dangling-anchor class of defect (`axon.runtime.*`,
    /// `tests/test_phase6_runtime.py`) is what made the catalog unfalsifiable.
    #[test]
    fn wired_rows_point_at_rust_symbols_not_python_paths() {
        for (locator, wiring) in WIRING_TABLE {
            if let Wiring::Wired(sym) = wiring {
                assert!(
                    !sym.starts_with("axon.runtime") && !sym.contains(".py"),
                    "locator `{locator}` is marked Wired but its symbol `{sym}` is a Python \
                     path — a Wired row must cite the real Rust symbol that runs"
                );
            }
        }
    }

    /// Only `Wired` counts. Guards against a future refactor quietly making
    /// `is_enforced` permissive.
    #[test]
    fn only_wired_is_enforced() {
        assert!(Wiring::Wired("x").is_enforced());
        assert!(!Wiring::Orphaned("runtime::lease_kernel").is_enforced());
        assert!(!Wiring::Absent.is_enforced());
    }

    /// The §111 headline, **paid**: declaring `lease` is now genuine evidence.
    ///
    /// §111 wrote this test to pin the LIE. SOC2 CC6.3 cited `LeaseKernel`, which
    /// had zero production callers — so declaring a `lease` RAISED an adopter's
    /// compliance score while enforcing precisely nothing. That was F8: the engine
    /// was unsound in the worst possible direction.
    ///
    /// The §111 assertion ended with an instruction to whoever eventually fixed it:
    /// *"If this now passes, wire it AND flip its WIRING_TABLE row in the same
    /// PR."* §113.d did exactly that, and this is the same test, inverted.
    #[test]
    fn the_soc2_cc63_lease_claim_is_now_genuinely_enforced() {
        let w = wiring_or_absent("axon.runtime.lease_kernel.LeaseKernel");
        assert!(
            w.is_enforced(),
            "§113.d — `LeaseKernel` is on a production path: `StoreRegistry::charge_lease` runs \
             on every resolve of a leased store, so a store operation IS a use of the resource \
             and the post-expiry one is the CT-2 Anchor Breach. SOC2 CC6.3 may now cite it and \
             MEAN it."
        );
        assert_eq!(w.as_str(), "wired");
    }

    /// An unknown locator must fail CLOSED, never default to enforced.
    #[test]
    fn unknown_locator_fails_closed() {
        assert!(wiring_of("totally::made::up").is_none());
        assert!(!wiring_or_absent("totally::made::up").is_enforced());
    }

    /// The Cognitive-I/O family must not satisfy control requirements while
    /// it has no dispatch arm.
    /// §Fase 112 — six of these are now ENFORCED, and this test is the record of it.
    ///
    /// The `CognitiveIoSupervisor` drives them through the real deploy path, each
    /// with a gate that proves it runs. This is the F8 loop closing: §111 made the
    /// compliance engine stop lying; §112 is what lets it also say YES.
    #[test]
    fn the_cognitive_io_features_112_wired_are_now_enforced() {
        for f in [
            "has_observe",
            "has_ensemble",
            "has_immune",
            "has_reflex",
            "has_heal",
            "has_reconcile",
        ] {
            assert!(
                feature_is_enforced(f),
                "`{f}` is driven by the CognitiveIoSupervisor (§112) and must now count"
            );
        }
    }

    /// …and since §113, so do `lease` and `resource`.
    ///
    /// The §111 version of this test was named `..._until_113` and asserted the
    /// **negation** of every line below. It was a promise with a date on it.
    #[test]
    fn lease_and_resource_are_enforced_since_113() {
        for f in ["has_lease", "has_resource"] {
            assert!(
                feature_is_enforced(f),
                "§113 — `{f}` is enforced. A `resource` governs what runs (an `axonstore` \
                 derives its DSN and its POOL SIZE from it), and `lease`'s CT-2 Anchor Breach \
                 has a moment to fire in: the store operation IS the use of the resource. \
                 Before §113 that guarantee was structurally IMPOSSIBLE, not merely unwired — \
                 and an impossible guarantee is one that cannot be KEPT, not merely one that \
                 cannot be broken."
            );
        }
        // …and the genuinely-wired surfaces still count, as they always did.
        // §124.c renamed `has_compliance_annotation` → `compliance_coverage_holds`
        // when its meaning changed from presence to the coverage rule holding —
        // enforced at check time (T957/T1215) and at the typed bus's `publish`.
        for f in ["has_shield", "has_endpoint", "compliance_coverage_holds"] {
            assert!(feature_is_enforced(f), "`{f}` is wired and must still count");
        }
    }

    /// **The debt ledger is empty — and an empty ledger is not a licence.**
    ///
    /// §111 opened this list with eight entries, and found the compliance engine
    /// scoring adopters on primitives that did nothing: **declaring a dead
    /// primitive RAISED your SOC2 score.** §112 paid six of them; §113 paid the
    /// last two.
    ///
    /// This test exists so the emptiness stays *earned*. A new feature whose kernel
    /// does not run belongs on this list immediately, in the same PR that declares
    /// it. If someone skips that step, the gates above turn it into a red build
    /// instead of a quiet lie inside a compliance report — which is the entire
    /// value of F8.
    #[test]
    fn the_unenforced_ledger_is_empty_and_that_emptiness_is_earned() {
        assert!(
            unenforced_features().is_empty(),
            "the ledger is expected empty as of §113. Adding an entry back is CORRECT and \
             honest when a kernel does not run — but say so out loud in the fase that adds \
             it, because it means the compliance engine can no longer say yes for that \
             feature. That admission is the point."
        );
    }
}
