//! v2.0.0 — Wire envelope producer helpers.
//!
//! Pure helpers that transform runtime execution metadata into the
//! semantic taxonomy of the wire envelope's epistemic fields:
//!
//!   - [`provenance_event_for`] — closed taxonomy of provenance
//!     event slugs. Given a `step_type` + `step_name` from the IR
//!     walk, emits the canonical `kind:identifier[@step]` slug for
//!     the `provenance_chain`.
//!   - [`derive_blame_from_report`] — pulls the first surfaced
//!     blame attribution from a flow's `RunReport`. Closed
//!     catalog: AnchorBreach → ShieldRejection → StoreBreach →
//!     BackendSoftFail → TypeMismatch.
//!   - [`collect_provenance_events`] — walk-and-emit helper that
//!     iterates an opaque `ExecutionUnit` slice via the same
//!     accessor pattern the runner uses internally; intentionally
//!     declared in this crate so 39.c.y can pull execution_units
//!     internals via `pub(crate)`.
//!
//! ## Why these live separately from runner.rs
//!
//! Pillar split: `runner.rs` is the execution machinery; this
//! module is the SEMANTIC TAXONOMY that turns the execution trace
//! into the wire-shape epistemic fields. Keeping the taxonomy in
//! one module makes it easy to grep + audit + test in isolation.
//!
//! ## Closed catalog of provenance event slugs
//!
//! The slugs follow a `kind:identifier` shape with an optional
//! `@step_name` suffix when the event is associated with a
//! specific step rather than a global flow position. The closed
//! catalog at v2.0.0:
//!
//!   | kind            | identifier source       | example                           |
//!   |-----------------|-------------------------|-----------------------------------|
//!   | `flow:`         | flow_name               | `flow:FetchTenants`               |
//!   | `step:`         | CompiledStep.step_name  | `step:Triage`                     |
//!   | `retrieve:`     | store_name              | `retrieve:tenants`                |
//!   | `persist:`      | store_name              | `persist:patient_records`         |
//!   | `mutate:`       | store_name              | `mutate:transactions`             |
//!   | `purge:`        | store_name              | `purge:audit_log`                 |
//!   | `shield:`       | shield_name@step_name   | `shield:Hipaa@Triage`             |
//!   | `ots:`          | ots_name@step_name      | `ots:audio_resample@Transcribe`   |
//!   | `mandate:`      | mandate_name@step_name  | `mandate:GdprArt6@Review`         |
//!   | `compute:`      | compute_name@step_name  | `compute:gpu_batch@Render`        |
//!   | `lambda_apply:` | lambda_name@step_name   | `lambda_apply:psi_builder@Forge`  |
//!   | `tool:`         | tool_name@step_name     | `tool:web_search@Discovery`       |
//!   | `memory:`       | kind(rem/rec)@step_name | `memory:remember@Persist`         |
//!   | `backend:`      | backend slug            | `backend:anthropic`               |
//!
//! The taxonomy is intentionally a closed catalog at v2.0.0; new
//! event kinds require an explicit plan-vivo step to extend
//! the table. This prevents drift between producers + consumers.

use crate::wire_envelope::{BlameContext, BlameKind, BlameParty};

// ════════════════════════════════════════════════════════════════════
// Provenance event taxonomy
// ════════════════════════════════════════════════════════════════════

/// v2.0.0 — emit the canonical provenance slug for a runtime
/// step type + name. Returns `None` when the step type doesn't
/// participate in the provenance chain (e.g. `step`, `reason`,
/// `validate` — these contribute via `step:` entries built
/// separately from `step_names`). The returned slug is one of the
/// closed-catalog forms documented in the module-level docs.
///
/// Pure function; deterministic.
pub fn provenance_event_for(step_type: &str, step_name: &str) -> Option<String> {
    match step_type {
        // Pillar II — store operations (the canonical case the
        // founder cited for adopter audit trails).
        "retrieve" => Some(format!("retrieve:{}", step_name)),
        "persist" => Some(format!("persist:{}", step_name)),
        "mutate" => Some(format!("mutate:{}", step_name)),
        "purge" => Some(format!("purge:{}", step_name)),
        // Pillar I — shield invocation (HIPAA / legal privilege /
        // AML scanners + judges).
        "shield_apply" => Some(format!("shield:{}", step_name)),
        // Algebraic apply primitives — each carries its own slug
        // for adopter auditors to trace.
        "ots_apply" => Some(format!("ots:{}", step_name)),
        "mandate_apply" => Some(format!("mandate:{}", step_name)),
        "compute_apply" => Some(format!("compute:{}", step_name)),
        "lambda_data_apply" => Some(format!("lambda_apply:{}", step_name)),
        // Tools + memory — observable but distinct from store ops.
        "use_tool" => Some(format!("tool:{}", step_name)),
        "remember" => Some(format!("memory:remember@{}", step_name)),
        "recall" => Some(format!("memory:recall@{}", step_name)),
        // `step` / `reason` / `validate` / `refine` / `weave` /
        // `let_binding` / `return` / `break` / `continue` and
        // every other internal step type contributes to the chain
        // via the `step:` entries built from `step_names`, NOT via
        // this taxonomy. Return None.
        _ => None,
    }
}

// ════════════════════════════════════════════════════════════════════
// Blame attribution producers
// ════════════════════════════════════════════════════════════════════

/// v2.0.0 — close-catalog blame producer for an anchor
/// breach observed at runtime. The `severity` discriminator
/// follows the same convention as `IRAnchor.severity` ("warn",
/// "error", "critical"); only non-"error" severities surface as
/// degraded posture (an "error" severity hard-fails the flow and
/// is caught by the existing error path).
pub fn blame_for_anchor_breach(
    step_name: &str,
    anchor_name: &str,
    severity: &str,
    confidence: f64,
) -> BlameContext {
    BlameContext {
        kind: BlameKind::AnchorBreach,
        // v2.83.0 — the paper assigns this one BY NAME: negative blame,
        // "la activación de un fallo por anchor_breach, el cual ocurre cuando el
        // sub-agente intenta modificar recursos fuera de su ámbito de contención
        // concedido" (paper_agent.md, Eje 2).
        party: Some(BlameParty::Server),
        location: format!("step:{}", step_name),
        message: format!(
            "anchor '{}' breached (severity={}, confidence={:.2}) — \
             flow proceeded on degraded posture",
            anchor_name, severity, confidence
        ),
        d_letter: Some("39.c.z".to_string()),
    }
}

/// v2.0.0 — blame producer for a shield scanner rejection
/// observed at runtime, where the flow chose to proceed (degraded
/// posture). When the shield's `on_violation` is "block", the
/// flow hard-fails and this producer is NOT called; this is only
/// for the "warn"/"continue" cases.
pub fn blame_for_shield_rejection(
    shield_name: &str,
    step_name: &str,
    pattern: &str,
) -> BlameContext {
    BlameContext {
        kind: BlameKind::ShieldRejection,
        // v2.83.0 — not named in the paper, but it follows from the
        // definition rather than from taste: the shield IS the contract guard,
        // and the content it flagged was produced by the invoked party. That is
        // negative blame — a postcondition violated by the callee.
        party: Some(BlameParty::Server),
        location: format!("step:{}", step_name),
        message: format!(
            "shield '{}' flagged pattern '{}' — flow proceeded on \
             degraded posture",
            shield_name, pattern
        ),
        d_letter: Some("39.c.z".to_string()),
    }
}

/// v2.0.0 — blame producer for a store mutation chain
/// verification failure where the flow proceeded with a prior-
/// state read. The location identifies which store + which
/// chain segment failed verification.
/// v2.83.0 — `party` is a PARAMETER here, and that is the point.
///
/// A broken mutation chain has two possible authors: the invoked party wrote a
/// malformed segment (`Server`), or the storage layer corrupted one
/// (`Environment`). The KIND cannot tell them apart; the call site can.
///
/// This is the evidence that the two axes are genuinely orthogonal. If every
/// `BlameKind` determined a `BlameParty`, `party` would not be a second axis —
/// it would be a function of the first, and shipping it as a field would be
/// storing a derivation. Two of the five kinds do not determine it, so it is a
/// real axis, and the honest default for those two is `None` rather than a
/// guess. Attributing responsibility to the wrong component is worse than
/// attributing it to nobody.
pub fn blame_for_store_breach(
    store_name: &str,
    chain_segment: &str,
    party: Option<BlameParty>,
) -> BlameContext {
    BlameContext {
        kind: BlameKind::StoreBreach,
        party,
        location: format!("store:{}", store_name),
        message: format!(
            "mutation chain verification failed at segment '{}' — \
             flow proceeded with prior-state read",
            chain_segment
        ),
        d_letter: Some("39.c.z".to_string()),
    }
}

/// v2.0.0 — blame producer for a backend soft-fail
/// (truncated response, partial completion, downgraded
/// throughput). The backend_name + reason identifies the source.
/// v2.83.0 — `party` is a PARAMETER, for the same reason as
/// [`blame_for_store_breach`]. A truncated or non-conforming completion is the
/// backend violating a postcondition (`Server`); a soft rate-limit or a
/// throughput downgrade is infrastructure (`Environment`). One kind, two
/// parties — so the kind does not decide it, and inventing one here would
/// fabricate an attribution the runtime never established.
pub fn blame_for_backend_soft_fail(
    backend_name: &str,
    reason: &str,
    party: Option<BlameParty>,
) -> BlameContext {
    BlameContext {
        kind: BlameKind::BackendSoftFail,
        party,
        location: format!("backend:{}", backend_name),
        message: format!("backend '{}' soft-fail: {}", backend_name, reason),
        d_letter: Some("39.c.z".to_string()),
    }
}

/// v2.0.0 — blame producer for a recoverable D5 type
/// mismatch (e.g. missing optional field with a defaulted value).
/// Distinct from a hard D5 rejection (which is a 4xx/5xx, not a
/// degraded 200).
pub fn blame_for_type_mismatch(
    field_path: &str,
    expected: &str,
    got: &str,
) -> BlameContext {
    BlameContext {
        kind: BlameKind::TypeMismatch,
        // v2.83.0 — the paper's negative blame, named: "el retorno de
        // tipos no conformes" (paper_agent.md, Eje 2).
        party: Some(BlameParty::Server),
        location: format!("field:{}", field_path),
        message: format!(
            "recoverable type mismatch at '{}' (expected {}, got {})",
            field_path, expected, got
        ),
        d_letter: Some("39.c.z".to_string()),
    }
}

// ════════════════════════════════════════════════════════════════════
// Closed-catalog priority: which blame wins when multiple surface
// ════════════════════════════════════════════════════════════════════

/// Stable ordinal for [`BlameKind`] priority. Lower ordinal = higher
/// priority (surfaces first in the wire envelope's
/// `blame_attribution` slot). Closed-catalog discipline: every
/// variant has a fixed priority; no ties.
///
/// Rationale: anchor breaches are the most epistemically severe
/// (the flow's own evidentiary contract was violated); type
/// mismatches are the most recoverable (the data shape was
/// salvaged). The other three sit in the middle.
pub fn blame_priority(kind: &BlameKind) -> u8 {
    match kind {
        BlameKind::AnchorBreach => 0,
        BlameKind::ShieldRejection => 1,
        BlameKind::StoreBreach => 2,
        BlameKind::BackendSoftFail => 3,
        BlameKind::TypeMismatch => 4,
    }
}

/// Merge two blame attributions per priority. The winner is the
/// HIGHER-PRIORITY (lower ordinal) one. Ties broken by keeping
/// `existing` (stable; first-emitted wins among equals). Used by
/// the runtime walk to coalesce multiple surfaced blames into a
/// single `blame_attribution` slot.
pub fn merge_blame(
    existing: Option<BlameContext>,
    incoming: Option<BlameContext>,
) -> Option<BlameContext> {
    match (existing, incoming) {
        (None, x) | (x, None) => x.or(None),
        (Some(a), Some(b)) => {
            if blame_priority(&b.kind) < blame_priority(&a.kind) {
                Some(b)
            } else {
                Some(a)
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── section 1 — provenance_event_for taxonomy ──

    #[test]
    fn retrieve_emits_retrieve_slug() {
        assert_eq!(
            provenance_event_for("retrieve", "tenants"),
            Some("retrieve:tenants".to_string())
        );
    }

    #[test]
    fn persist_emits_persist_slug() {
        assert_eq!(
            provenance_event_for("persist", "patient_records"),
            Some("persist:patient_records".to_string())
        );
    }

    #[test]
    fn mutate_emits_mutate_slug() {
        assert_eq!(
            provenance_event_for("mutate", "transactions"),
            Some("mutate:transactions".to_string())
        );
    }

    #[test]
    fn purge_emits_purge_slug() {
        assert_eq!(
            provenance_event_for("purge", "audit_log"),
            Some("purge:audit_log".to_string())
        );
    }

    #[test]
    fn shield_apply_emits_shield_slug() {
        assert_eq!(
            provenance_event_for("shield_apply", "HipaaTriage"),
            Some("shield:HipaaTriage".to_string())
        );
    }

    #[test]
    fn ots_apply_emits_ots_slug() {
        assert_eq!(
            provenance_event_for("ots_apply", "audio_resample"),
            Some("ots:audio_resample".to_string())
        );
    }

    #[test]
    fn mandate_apply_emits_mandate_slug() {
        assert_eq!(
            provenance_event_for("mandate_apply", "GdprArt6"),
            Some("mandate:GdprArt6".to_string())
        );
    }

    #[test]
    fn compute_apply_emits_compute_slug() {
        assert_eq!(
            provenance_event_for("compute_apply", "gpu_batch"),
            Some("compute:gpu_batch".to_string())
        );
    }

    #[test]
    fn lambda_apply_emits_lambda_slug() {
        assert_eq!(
            provenance_event_for("lambda_data_apply", "psi_builder"),
            Some("lambda_apply:psi_builder".to_string())
        );
    }

    #[test]
    fn use_tool_emits_tool_slug() {
        assert_eq!(
            provenance_event_for("use_tool", "web_search"),
            Some("tool:web_search".to_string())
        );
    }

    #[test]
    fn remember_emits_memory_slug() {
        assert_eq!(
            provenance_event_for("remember", "Persist"),
            Some("memory:remember@Persist".to_string())
        );
    }

    #[test]
    fn recall_emits_memory_slug() {
        assert_eq!(
            provenance_event_for("recall", "Lookup"),
            Some("memory:recall@Lookup".to_string())
        );
    }

    #[test]
    fn regular_step_returns_none() {
        // Regular `step` / `reason` / `validate` / etc. don't
        // emit semantic provenance entries — they're captured
        // via the `step:` slugs built from `step_names`.
        assert_eq!(provenance_event_for("step", "Triage"), None);
        assert_eq!(provenance_event_for("reason", "Analyze"), None);
        assert_eq!(provenance_event_for("validate", "Check"), None);
        assert_eq!(provenance_event_for("refine", "Improve"), None);
        assert_eq!(provenance_event_for("weave", "Combine"), None);
        assert_eq!(provenance_event_for("let_binding", "x"), None);
        assert_eq!(provenance_event_for("return", "_"), None);
    }

    #[test]
    fn unknown_step_type_returns_none() {
        // Defensive: unknown step types don't emit fabricated
        // entries — they're silent. This keeps the taxonomy
        // closed; future plan-vivos extending it must add the
        // explicit `match` arm.
        assert_eq!(provenance_event_for("future_kind", "X"), None);
        assert_eq!(provenance_event_for("", "Anything"), None);
    }

    // ── section 2 — blame producers ──

    #[test]
    fn anchor_breach_producer() {
        let b = blame_for_anchor_breach(
            "Triage",
            "ConfidenceFloor",
            "warn",
            0.42,
        );
        assert_eq!(b.kind, BlameKind::AnchorBreach);
        assert_eq!(b.location, "step:Triage");
        assert!(b.message.contains("ConfidenceFloor"));
        assert!(b.message.contains("warn"));
        assert!(b.message.contains("0.42"));
        assert_eq!(b.d_letter.as_deref(), Some("39.c.z"));
    }

    #[test]
    fn shield_rejection_producer() {
        let b = blame_for_shield_rejection("Hipaa", "Review", "pii_phone");
        assert_eq!(b.kind, BlameKind::ShieldRejection);
        assert_eq!(b.location, "step:Review");
        assert!(b.message.contains("Hipaa"));
        assert!(b.message.contains("pii_phone"));
    }

    #[test]
    fn store_breach_producer() {
        let b = blame_for_store_breach("transactions", "segment_42", None);
        assert_eq!(b.kind, BlameKind::StoreBreach);
        assert_eq!(b.location, "store:transactions");
        assert!(b.message.contains("segment_42"));
    }

    #[test]
    fn backend_soft_fail_producer() {
        let b = blame_for_backend_soft_fail("anthropic", "truncated_response", None);
        assert_eq!(b.kind, BlameKind::BackendSoftFail);
        assert_eq!(b.location, "backend:anthropic");
        assert!(b.message.contains("truncated_response"));
    }

    #[test]
    fn type_mismatch_producer() {
        let b = blame_for_type_mismatch("user.age", "Integer", "String");
        assert_eq!(b.kind, BlameKind::TypeMismatch);
        assert_eq!(b.location, "field:user.age");
        assert!(b.message.contains("Integer"));
        assert!(b.message.contains("String"));
    }

    // ── section 3 — priority + merge ──

    #[test]
    fn priority_anchor_beats_shield() {
        let anchor = blame_for_anchor_breach("S", "A", "warn", 0.5);
        let shield = blame_for_shield_rejection("Sh", "S", "p");
        let winner = merge_blame(Some(shield.clone()), Some(anchor.clone()));
        assert_eq!(
            winner.unwrap().kind,
            BlameKind::AnchorBreach,
            "anchor breach has higher priority than shield rejection"
        );
    }

    #[test]
    fn priority_shield_beats_store() {
        let shield = blame_for_shield_rejection("Sh", "S", "p");
        let store = blame_for_store_breach("st", "seg", None);
        let winner = merge_blame(Some(store), Some(shield.clone()));
        assert_eq!(winner.unwrap().kind, BlameKind::ShieldRejection);
    }

    #[test]
    fn priority_store_beats_backend() {
        let store = blame_for_store_breach("st", "seg", None);
        let backend = blame_for_backend_soft_fail("be", "r", None);
        let winner = merge_blame(Some(backend), Some(store.clone()));
        assert_eq!(winner.unwrap().kind, BlameKind::StoreBreach);
    }

    #[test]
    fn priority_backend_beats_typemismatch() {
        let backend = blame_for_backend_soft_fail("be", "r", None);
        let mismatch = blame_for_type_mismatch("f", "I", "S");
        let winner = merge_blame(Some(mismatch), Some(backend.clone()));
        assert_eq!(winner.unwrap().kind, BlameKind::BackendSoftFail);
    }

    #[test]
    fn merge_none_preserves_other() {
        let b = blame_for_anchor_breach("S", "A", "warn", 0.5);
        assert_eq!(
            merge_blame(None, Some(b.clone())),
            Some(b.clone())
        );
        assert_eq!(
            merge_blame(Some(b.clone()), None),
            Some(b)
        );
        assert_eq!(merge_blame(None, None), None);
    }

    #[test]
    fn merge_tie_keeps_existing() {
        let a1 = blame_for_anchor_breach("S1", "A1", "warn", 0.5);
        let a2 = blame_for_anchor_breach("S2", "A2", "warn", 0.6);
        let winner = merge_blame(Some(a1.clone()), Some(a2));
        // Same priority → existing wins (stable).
        assert_eq!(winner.unwrap().location, "step:S1");
    }

    #[test]
    fn priority_closed_catalog_total_order() {
        // Verify the priority ordinals form a total order over the
        // 5 BlameKind variants. This is the structural enforcement
        // that prevents drift if a new variant is added: every
        // pair must have a well-defined winner.
        let priorities = [
            blame_priority(&BlameKind::AnchorBreach),
            blame_priority(&BlameKind::ShieldRejection),
            blame_priority(&BlameKind::StoreBreach),
            blame_priority(&BlameKind::BackendSoftFail),
            blame_priority(&BlameKind::TypeMismatch),
        ];
        // Strictly monotone increasing → total order, no ties.
        for w in priorities.windows(2) {
            assert!(
                w[0] < w[1],
                "blame priority MUST be strictly monotone: {} not < {}",
                w[0], w[1]
            );
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// Internal API consumed by runner.rs (pub(crate))
// ════════════════════════════════════════════════════════════════════

/// v2.0.0 — collect provenance events from the runtime's
/// execution plan. Walks the caller-flattened `(step_type, step_name)`
/// slice; for each step whose `step_type` participates in the
/// closed taxonomy, emits the canonical slug. Called from
/// `execute_server_flow` AFTER the report is built.
///
/// The function is intentionally a thin wrapper around
/// [`provenance_event_for`] — the taxonomy logic lives in the
/// pure helper above (testable in isolation); this function
/// handles only the walk + the closed-catalog filtering.
///
/// Implementation note: takes a slice of `(step_type, step_name)`
/// tuples extracted by the caller — the actual `ExecutionUnit`
/// struct is private to runner.rs, so the caller flattens before
/// calling here. This keeps the producer module free of internal
/// runner types.
pub fn collect_provenance_events_from(
    steps: &[(String, String)],
) -> Vec<String> {
    steps
        .iter()
        .filter_map(|(stype, sname)| provenance_event_for(stype, sname))
        .collect()
}

/// v2.0.0 — derive blame attribution from the runtime's
/// `ExecutionReport`. Scans every unit + every step; the first
/// step with `anchor_breaches > 0` surfaces as `BlameKind::AnchorBreach`.
/// Subsequent breaches are still observable on the step audit
/// but do NOT overwrite (first-emitted wins per `merge_blame`'s
/// stable-tie discipline).
///
/// In v2.0.0 this implementation covers the AnchorBreach
/// kind only; the other 4 kinds have ready producer functions
/// (see `blame_for_shield_rejection` / `blame_for_store_breach` /
/// `blame_for_backend_soft_fail` / `blame_for_type_mismatch`)
/// but their wiring depends on richer runtime observability that
/// future steps add. The blame module is 100% robust at the
/// PRODUCER surface; the runtime walks are extended progressively
/// as observability hooks land.
pub fn derive_blame_from_report(
    report: &crate::output::ExecutionReport,
) -> Option<BlameContext> {
    let mut accumulated: Option<BlameContext> = None;
    for unit in &report.units {
        for step in &unit.steps {
            if step.anchor_breaches > 0 {
                // The runtime doesn't currently surface the
                // specific anchor name / severity / confidence
                // through the report (that flows through trace
                // events). Use a structural attribution that
                // names the step + the breach count.
                let blame = BlameContext {
                    kind: BlameKind::AnchorBreach,
                    // v2.83.0 — same attribution as the dispatcher path in
                    // `runner.rs`, and it has to be: `merge_blame`'s
                    // first-emitted-wins discipline means the two engines must
                    // agree on the whole shape, party included, or the reported
                    // responsibility would depend on which engine ran.
                    party: Some(BlameParty::Server),
                    location: format!("step:{}", step.name),
                    message: format!(
                        "{} anchor breach(es) on step '{}' — flow \
                         proceeded on degraded posture",
                        step.anchor_breaches, step.name
                    ),
                    d_letter: Some("39.c.z".to_string()),
                };
                accumulated = merge_blame(accumulated, Some(blame));
            }
        }
    }
    accumulated
}

#[cfg(test)]
mod runner_integration_tests {
    use super::*;
    use crate::output::{
        ExecutionReport, ExecutionSummary, StepReport, UnitReport,
    };
    use crate::plan_export::SchemaHeader;

    fn build_test_report(units: Vec<UnitReport>) -> ExecutionReport {
        ExecutionReport {
            _schema: SchemaHeader::new("axon.report"),
            axon_version: "test".to_string(),
            source_file: "t.axon".to_string(),
            backend: "stub".to_string(),
            mode: "test".to_string(),
            success: true,
            units,
            summary: ExecutionSummary {
                total_units: 0,
                total_steps: 0,
                total_duration_ms: 0,
                avg_step_duration_ms: 0,
                total_input_tokens: 0,
                total_output_tokens: 0,
                total_tokens: 0,
                retried_steps: 0,
            },
        }
    }

    fn build_test_step(name: &str, anchor_breaches: u32) -> StepReport {
        StepReport {
            name: name.to_string(),
            step_type: "step".to_string(),
            result: String::new(),
            duration_ms: 0,
            input_tokens: 0,
            output_tokens: 0,
            anchor_breaches,
            chain_activations: 0,
            was_retried: false,
        }
    }

    fn build_test_unit(steps: Vec<StepReport>) -> UnitReport {
        UnitReport {
            flow_name: "F".to_string(),
            persona_name: String::new(),
            steps,
            duration_ms: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_anchor_breaches: 0,
            total_chain_activations: 0,
        }
    }

    #[test]
    fn clean_report_returns_no_blame() {
        let report = build_test_report(vec![build_test_unit(vec![
            build_test_step("S1", 0),
            build_test_step("S2", 0),
        ])]);
        assert!(derive_blame_from_report(&report).is_none());
    }

    #[test]
    fn anchor_breach_surfaces_blame() {
        let report = build_test_report(vec![build_test_unit(vec![
            build_test_step("Triage", 1),
        ])]);
        let blame = derive_blame_from_report(&report).expect("blame surfaces");
        assert_eq!(blame.kind, BlameKind::AnchorBreach);
        assert_eq!(blame.location, "step:Triage");
    }

    #[test]
    fn first_breach_wins_on_multi() {
        let report = build_test_report(vec![build_test_unit(vec![
            build_test_step("First", 1),
            build_test_step("Second", 1),
        ])]);
        let blame = derive_blame_from_report(&report).expect("blame surfaces");
        assert_eq!(
            blame.location, "step:First",
            "first surfaced breach wins (stable tie-break)"
        );
    }

    #[test]
    fn collect_provenance_walks_taxonomy() {
        let steps = vec![
            ("step".to_string(), "Plan".to_string()),
            ("retrieve".to_string(), "tenants".to_string()),
            ("step".to_string(), "Decide".to_string()),
            ("shield_apply".to_string(), "Hipaa".to_string()),
            ("persist".to_string(), "audit_log".to_string()),
        ];
        let events = collect_provenance_events_from(&steps);
        assert_eq!(
            events,
            vec![
                "retrieve:tenants",
                "shield:Hipaa",
                "persist:audit_log",
            ],
            "only taxonomy-participating step types emit slugs; the \
             regular `step` entries are absent (they're emitted via \
             step_names by the converter)"
        );
    }
}
