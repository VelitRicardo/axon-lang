//! v2.66.0 — Governed Human Notification: the canonical contract.
//!
//! The third egress dual (`deliver` v2.60.0 = systems of record, `document`
//! v2.62.0 = artifacts, **`notify` = human attention**) — and the most
//! dangerous: a notification interrupts a human and asks for action NOW.
//! Everything here serves the three laws:
//!
//! - **Evidence (T933/the design decision)** — a bound flow value crosses to the human
//! WITH its epistemic label; a v2.63.0 query envelope appends its evidence
//!   line ("computed over N rows, taint: untrusted"). A guess arrives
//!   labeled as a guess, or the compile refused it.
//! - **Recipient custody (T934/the design decision)** — this module NEVER sees a phone
//! number or chat id. The plan carries the v2.48.0 secret-CLASS ref; the
//!   enterprise transducer resolves it at dispatch, tenant-scoped.
//! - **Attention (T935/the design decision)** — the plan carries the declared window;
//!   the durable at-most-once enforcement lives in the ENT ledger
//! (v2.66.0). Fire-on-resolution: an unresolved KEY ref makes
//!   the notification a WITNESSED no-op, never an empty message.
//!
//! The provider port follows the house injection pattern (`mint`/`rotate`/
//! `deliver`): [`NoProvider`] fails CLOSED — a reached notification with
//! no transducer is a structured refusal, never a silent drop and never
//! a fabricated "sent".

use std::sync::{Arc, RwLock};

use crate::ir_nodes::IRNotify;

// ─────────────────────────────────────────────────────────────────────
//  The canonical plan
// ─────────────────────────────────────────────────────────────────────

/// How epistemic labels cross to the human.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvenanceMode {
    /// Every bound value arrives with its label; envelope refs append
    /// their evidence line. The default — the safe boundary.
    Attached,
    /// Labels stripped — legal ONLY under an epistemic vouch (T933
    /// refused everything else at compile; the PCC re-derives it).
    Cleared,
}

impl ProvenanceMode {
    pub fn from_ir(s: &str) -> Self {
        match s {
            "cleared" => ProvenanceMode::Cleared,
            _ => ProvenanceMode::Attached,
        }
    }
}

/// One `${ref}` slot's resolution.
#[derive(Debug, Clone)]
pub struct BoundSlot {
    pub name: String,
    pub value: String,
    /// The label appended under `attached` — the value's epistemic status
    /// plus, for a v2.63.0 envelope, its evidence line. Empty under a
    /// vouched `cleared`.
    pub label: String,
}

/// The canonical, vendor-agnostic notification plan. NOTE what is
/// absent: any recipient value — only the custody CLASS travels.
#[derive(Debug, Clone)]
pub struct NotificationPlan {
    pub name: String,
    /// `sms | whatsapp | telegram` (closed, T934).
    pub channel: String,
    /// The v2.48.0 secret-class ref (resolved by the ENT transducer at
    /// dispatch — never here).
    pub recipient_ref: String,
    /// The rendered message: template with slots substituted and (under
    /// `attached`) labels appended.
    pub message: String,
    pub slots: Vec<BoundSlot>,
    /// The declared attention window (T935) — enforcement is the ENT
    /// ledger's job; carried so the ledger keys on the DECLARED value.
    pub window: String,
    pub provenance: ProvenanceMode,
}

/// The witnessed outcome of a send attempt.
#[derive(Debug, Clone)]
pub struct NotificationReceipt {
    pub name: String,
    pub channel: String,
    /// Vendor-assigned id when available (an accepted API call is NOT a
    /// read message — the honest section 8 perimeter).
    pub vendor_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationError {
    /// No transducer mounted — fail CLOSED (the v2.63.0 posture).
    NoProvider,
    /// The KEY ref did not resolve — fire-on-resolution: a
    /// witnessed no-op, distinct from an error.
    RefUnresolved { slot: String },
    /// Transducer/vendor failure (network, auth, vendor 4xx/5xx).
    Transducer { message: String },
    /// The plan violates the contract (stale/hand-edited IR reached
    /// dispatch — the compile laws did not run).
    InvalidPlan { message: String },
}

impl std::fmt::Display for NotificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotificationError::NoProvider => write!(
                f,
                "no notification transducer mounted — a reached `notify` fails CLOSED \
                 (never a silent drop, never a fabricated send)"
            ),
            NotificationError::RefUnresolved { slot } => {
                write!(f, "notification slot `${{{slot}}}` did not resolve — witnessed no-op")
            }
            NotificationError::Transducer { message } => write!(f, "transducer: {message}"),
            NotificationError::InvalidPlan { message } => write!(f, "invalid plan: {message}"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// The provider port (the v2.46.0/v2.48.0/v2.60.0 injection shape)
// ─────────────────────────────────────────────────────────────────────

/// The channel transducer. The ENTERPRISE side implements it (generic
/// SMS / WhatsApp Cloud / Telegram Bot) and resolves the recipient from
/// custody INSIDE `send` — the OSS side never holds a recipient value.
pub trait NotificationProvider: Send + Sync {
    fn send(
        &self,
        tenant_id: &str,
        plan: &NotificationPlan,
    ) -> Result<NotificationReceipt, NotificationError>;
}

/// The fail-closed default.
pub struct NoProvider;

impl NotificationProvider for NoProvider {
    fn send(
        &self,
        _tenant_id: &str,
        _plan: &NotificationPlan,
    ) -> Result<NotificationReceipt, NotificationError> {
        Err(NotificationError::NoProvider)
    }
}

static PROVIDER: RwLock<Option<Arc<dyn NotificationProvider>>> = RwLock::new(None);

pub fn register_provider(provider: Arc<dyn NotificationProvider>) {
    *PROVIDER.write().expect("notification provider lock poisoned") = Some(provider);
}

pub fn clear_provider() {
    *PROVIDER.write().expect("notification provider lock poisoned") = None;
}

pub fn active_provider() -> Option<Arc<dyn NotificationProvider>> {
    PROVIDER
        .read()
        .expect("notification provider lock poisoned")
        .clone()
}

// ─────────────────────────────────────────────────────────────────────
//  Planning — fire-on-resolution + the evidence labels
// ─────────────────────────────────────────────────────────────────────

/// Extract the `${ref}` slot names from a template, in order.
pub fn template_slots(template: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(i) = rest.find("${") {
        if let Some(j) = rest[i + 2..].find('}') {
            out.push(rest[i + 2..i + 2 + j].to_string());
            rest = &rest[i + 2 + j + 1..];
        } else {
            break;
        }
    }
    out
}

/// The evidence label for a resolved value (the design decision, the "conscious"
/// half): if the value parses as a v2.63.0 query envelope, its evidence
/// line; otherwise the generic untrusted-provenance label. Empty under
/// a vouched `cleared` plan (the caller decides).
pub fn evidence_label(value: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(value) {
        if let (Some(taint), Some(stats)) = (v.get("taint"), v.get("stats")) {
            let scanned = stats.get("rows_scanned").and_then(|x| x.as_u64()).unwrap_or(0);
            let matched = stats.get("rows_matched").and_then(|x| x.as_u64()).unwrap_or(0);
            return format!(
                " [computed: {matched}/{scanned} rows, taint: {}]",
                taint.as_str().unwrap_or("unknown")
            );
        }
    }
    " [unverified flow value]".to_string()
}

/// Build the canonical plan from the compiled declaration + the run's
/// bindings. **Fire-on-resolution:** if ANY template slot does
/// not resolve, the notification is a witnessed no-op
/// ([`NotificationError::RefUnresolved`]) — the flow's own logic (an
/// `if` around producing the binding) IS the trigger.
pub fn plan_notification(
    ir: &IRNotify,
    bindings: &std::collections::HashMap<String, String>,
) -> Result<NotificationPlan, NotificationError> {
    if ir.template.is_empty() || ir.to_secret.is_empty() || ir.window.is_empty() {
        return Err(NotificationError::InvalidPlan {
            message: format!(
                "notify '{}' carries an empty template/recipient-ref/window — the \
                 compile-time axon-T934/T935 checks did not run over this IR \
                 (stale or hand-edited artifact)",
                ir.name
            ),
        });
    }
    let provenance = ProvenanceMode::from_ir(&ir.provenance);
    // A cleared plan is legal only under a vouch — re-check what T933
    // proved (the runtime never trusts the artifact blindly).
    if provenance == ProvenanceMode::Cleared
        && !ir.template.is_empty()
        && template_slots(&ir.template)
            .iter()
            .any(|s| bindings.contains_key(s))
        && !matches!(ir.epistemic_mode.as_str(), "believe" | "know")
    {
        return Err(NotificationError::InvalidPlan {
            message: format!(
                "notify '{}' clears provenance on bound flow values without an epistemic \
                 vouch — axon-T933 did not run over this IR (stale or hand-edited artifact)",
                ir.name
            ),
        });
    }
    let mut slots = Vec::new();
    let mut message = ir.template.clone();
    for slot in template_slots(&ir.template) {
        let value = crate::exec_context::resolve_dotted_var(bindings, &slot)
            .ok_or(NotificationError::RefUnresolved { slot: slot.clone() })?;
        let label = if provenance == ProvenanceMode::Attached {
            evidence_label(&value)
        } else {
            String::new()
        };
        message = message.replace(&format!("${{{slot}}}"), &format!("{value}{label}"));
        slots.push(BoundSlot {
            name: slot,
            value,
            label,
        });
    }
    Ok(NotificationPlan {
        name: ir.name.clone(),
        channel: ir.channel.clone(),
        recipient_ref: ir.to_secret.clone(),
        message,
        slots,
        window: ir.window.clone(),
        provenance,
    })
}

/// Dispatch through the mounted transducer — fail CLOSED without one.
pub fn run_notification(
    tenant_id: &str,
    plan: &NotificationPlan,
) -> Result<NotificationReceipt, NotificationError> {
    match active_provider() {
        Some(p) => p.send(tenant_id, plan),
        None => Err(NotificationError::NoProvider),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ir(template: &str, provenance: &str, epistemic: &str) -> IRNotify {
        IRNotify {
            node_type: "notify",
            source_line: 1,
            source_column: 1,
            name: "N".into(),
            channel: "sms".into(),
            to_secret: "ops.oncall_phone".into(),
            template: template.into(),
            window: "4h".into(),
            provenance: provenance.into(),
            effects: vec!["web".into()],
            epistemic_mode: epistemic.into(),
        }
    }

    fn binds(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn attached_plan_appends_evidence_labels() {
        // A v2.63.0 envelope ref gets its evidence line — the notification
        // that explains itself.
        let envelope = r#"{"rows":[{"sum_monto":12400.0}],"taint":"untrusted","stats":{"rows_scanned":1204,"rows_matched":7,"batches_total":3,"batches_pruned":2}}"#;
        let plan = plan_notification(
            &ir("Ventas 7d: ${resumen}", "attached", ""),
            &binds(&[("resumen", envelope)]),
        )
        .unwrap();
        assert!(plan.message.contains("[computed: 7/1204 rows, taint: untrusted]"), "{}", plan.message);
        // A plain flow value gets the generic label — never unlabeled.
        let plan = plan_notification(
            &ir("estado: ${estado}", "attached", ""),
            &binds(&[("estado", "critico")]),
        )
        .unwrap();
        assert!(plan.message.contains("critico [unverified flow value]"), "{}", plan.message);
    }

    #[test]
    fn fire_on_resolution_unresolved_ref_is_a_witnessed_noop() {
        // the design decision — the flow didn't bind `alerta` (its condition didn't
        // hold): the notification is a NO-OP, distinct from an error.
        let err = plan_notification(&ir("x: ${alerta}", "attached", ""), &binds(&[]))
            .unwrap_err();
        assert_eq!(err, NotificationError::RefUnresolved { slot: "alerta".into() });
    }

    #[test]
    fn cleared_without_vouch_is_refused_at_plan_time_too() {
        // Defense in depth: a hand-edited IR that cleared provenance is
        // refused HERE even before the PCC would have refused the deploy.
        let err = plan_notification(
            &ir("x: ${v}", "cleared", ""),
            &binds(&[("v", "guess")]),
        )
        .unwrap_err();
        assert!(matches!(err, NotificationError::InvalidPlan { .. }), "{err:?}");
        // Vouched cleared passes, labels absent.
        let plan = plan_notification(
            &ir("x: ${v}", "cleared", "believe"),
            &binds(&[("v", "verified")]),
        )
        .unwrap();
        assert_eq!(plan.message, "x: verified");
    }

    #[test]
    fn no_provider_fails_closed() {
        clear_provider();
        let plan = plan_notification(
            &ir("hola", "attached", ""),
            &binds(&[]),
        )
        .unwrap();
        let err = run_notification("t1", &plan).unwrap_err();
        assert_eq!(err, NotificationError::NoProvider);
    }

    #[test]
    fn the_plan_never_carries_a_recipient_value() {
        let plan = plan_notification(&ir("hola", "attached", ""), &binds(&[])).unwrap();
        assert_eq!(plan.recipient_ref, "ops.oncall_phone", "the CLASS, not a number");
    }

    #[test]
    fn template_slot_extraction() {
        assert_eq!(
            template_slots("a ${x} b ${y.z} c"),
            vec!["x".to_string(), "y.z".to_string()]
        );
        assert!(template_slots("no slots").is_empty());
    }
}
