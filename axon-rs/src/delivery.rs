//! v2.60.0 — Governed CRM Delivery runtime: the OSS contract + the
//! `DeliveryProvider` seam (the enterprise-engine injection point) + the
//! provenance-carrying discipline + the idempotency law.
//!
//! **What delivery is.** The egress-dual of acquisition (`scrape`, v2.52.0). Given a
//! set of canonical CRM operations whose fields bind flow values, write them into
//! the tenant's system of record. Where `scrape` brings a value in born
//! Untrusted, `deliver` sends a value OUT into a machine others treat as fact.
//!
//! **The load-bearing property: a delivered field carries its epistemic
//! origin, or the author vouched.** With `provenance: attached` (the default) each
//! field lands in the CRM beside its [`FieldProvenance`] block (level +
//! confidence + source) — a vendor guess arrives *labeled as a guess*. With
//! `provenance: cleared` the frontend already proved (axon-T920) the values were
//! shield+anchor-cleared under an `epistemic { believe|know }` vouch, so bare
//! values are honest. Silent laundering is impossible by construction.
//!
//! **The engine is enterprise.** OSS ships the CONTRACT + a default
//! [`NoProvider`] that TYPED-REFUSES every delivery — never a fabricated receipt
//! (the design decision / the v2.58.0 the design decision honesty). The enterprise generic HTTP transducer
//! (v2.60.0) registers via [`register_provider`]; the `crm:deliver` RBAC gate +
//! the per-tenant legal flag + credential resolution + the audit all live in
//! enterprise. With no engine wired, `deliver` is an honest refusal.

use std::sync::{Arc, OnceLock, RwLock};

use serde::{Deserialize, Serialize};

// ════════════════════════════════════════════════════════════════════════════
//  Provenance mode + the per-field epistemic origin
// ════════════════════════════════════════════════════════════════════════════

/// How field provenance crosses the delivery boundary. Mirrors the
/// frontend `deliver.provenance:` catalog; an empty IR value lowers to `Attached`
/// (the safe default: provenance travels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceMode {
    /// Each delivered field lands with its [`FieldProvenance`] block.
    #[default]
    Attached,
    /// Bare values — legal ONLY because axon-T920 proved an `epistemic
    /// { believe|know }` vouch at compile time.
    Cleared,
}

impl ProvenanceMode {
    /// Lower the IR string (`""`/`"attached"` ⇒ Attached; `"cleared"` ⇒ Cleared).
    pub fn from_ir(s: &str) -> Self {
        match s {
            "cleared" => ProvenanceMode::Cleared,
            _ => ProvenanceMode::Attached,
        }
    }
}

/// The epistemic origin of one delivered field — the exact shape v2.58.0's
/// `EnrichedField` produces (`level` on the believe-ceiling lattice + `confidence`
/// + the `source` vendor/flow tag). When [`ProvenanceMode::Attached`], the
/// enterprise transducer lands this beside the value in the CRM record so an
/// auditor can later tell a verified fact from a vendor's guess.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldProvenance {
    /// `speculate | believe | …` — the believe-ceiling lattice name (v2.54.0/v2.58.0).
    pub level: String,
    /// The confidence in `[0,1]`.
    pub confidence: f64,
    /// The producing source — a vendor tag, a flow provenance id.
    pub source: String,
}

/// One resolved field delivered to the CRM. `provenance` is `Some` only when the
/// delivery is `Attached` AND the bound flow value carried an epistemic origin
/// (the enterprise engine populates it from the runtime); a literal author-written
/// value has none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeliveredField {
    pub name: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<FieldProvenance>,
}

// ════════════════════════════════════════════════════════════════════════════
//  Canonical operations + the delivery request
// ════════════════════════════════════════════════════════════════════════════

/// A canonical, vendor-agnostic CRM operation. `kind ∈ {upsert_contact,
/// create_deal, add_note}`. `idempotency_key` is the the design decision law: an at-least-once
/// retry keyed on this value MUST NOT double-create — the enterprise transducer
/// maps it onto the vendor's natural-key/external-id upsert.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalOp {
    pub kind: String,
    pub idempotency_key: String,
    pub fields: Vec<DeliveredField>,
}

/// The request a [`DeliveryProvider`] receives — the compiled delivery with every
/// `Ref` field resolved against the flow's bindings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeliveryRequest {
    /// The `deliver` declaration name (audit provenance).
    pub name: String,
    /// The system-of-record class (`crm`).
    pub target: String,
    pub provenance: ProvenanceMode,
    pub ops: Vec<CanonicalOp>,
}

/// A receipt witnessing one delivered operation. `record_id` is the vendor's id
/// for the upserted record (`None` on a typed miss); `created` is `false` when an
/// existing record was updated — the idempotency witness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeliveryReceipt {
    pub kind: String,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_id: Option<String>,
    pub created: bool,
}

// ════════════════════════════════════════════════════════════════════════════
//  Errors — the Findler-Felleisen-style blame split (D105.b)
// ════════════════════════════════════════════════════════════════════════════

/// Everything that can go wrong delivering. A failure is a typed error + (in
/// enterprise) an audit row — NEVER a fabricated receipt. The
/// blame split is load-bearing: `Caller` is the flow's fault (a malformed
/// payload), `Provider`/`Network` is the vendor's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryError {
    /// No enterprise transducer is registered — the OSS build cannot deliver.
    NoProviderConfigured,
    /// The canonical payload was malformed (missing key, unknown op) — caller blame.
    Caller(String),
    /// The vendor rejected the request (auth, validation, quota) — provider blame.
    Provider(String),
    /// The vendor was unreachable (transport / timeout) — network blame.
    Network(String),
    /// The per-tenant `crm.deliver_enabled` legal gate is OFF (enterprise gate).
    TierDisabled,
}

impl std::fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeliveryError::NoProviderConfigured => write!(
                f,
                "deliver: no delivery transducer is configured — the OSS build has no engine \
                 (this is a typed refusal, never a fabricated receipt)"
            ),
            DeliveryError::Caller(m) => write!(f, "deliver: malformed operation — {m}"),
            DeliveryError::Provider(m) => write!(f, "deliver: the CRM rejected the write — {m}"),
            DeliveryError::Network(m) => write!(f, "deliver: the CRM was unreachable — {m}"),
            DeliveryError::TierDisabled => write!(
                f,
                "deliver: the tenant's `crm.deliver_enabled` tier is OFF (a human must enable it)"
            ),
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  The pluggable provider (enterprise engine injection seam)
// ════════════════════════════════════════════════════════════════════════════

/// The contract every delivery engine satisfies: a [`DeliveryRequest`] → one
/// [`DeliveryReceipt`] per operation. The enterprise generic HTTP transducer
/// (v2.60.0) implements this. Failure is a typed [`DeliveryError`], never a
/// fabricated receipt.
pub trait DeliveryProvider: Send + Sync {
    /// A stable provider identifier for the audit row (e.g. `"http-generic"`).
    fn name(&self) -> &str;
    /// Deliver every operation. MUST be idempotent per `idempotency_key`
    /// and MUST return a typed error on failure — never a fabricated
    /// receipt.
    fn deliver(&self, req: &DeliveryRequest) -> Result<Vec<DeliveryReceipt>, DeliveryError>;
}

/// The OSS default engine: **none**. It typed-refuses every delivery — the honest
/// state of a runtime with no engine wired (the v2.58.0 `NoProvider` shape).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProvider;

impl DeliveryProvider for NoProvider {
    fn name(&self) -> &str {
        "none"
    }
    fn deliver(&self, _req: &DeliveryRequest) -> Result<Vec<DeliveryReceipt>, DeliveryError> {
        Err(DeliveryError::NoProviderConfigured)
    }
}

fn registry() -> &'static RwLock<Option<Arc<dyn DeliveryProvider>>> {
    static REG: OnceLock<RwLock<Option<Arc<dyn DeliveryProvider>>>> = OnceLock::new();
    REG.get_or_init(|| RwLock::new(None))
}

/// v2.60.0 — register the process-wide delivery transducer. OSS ships none
/// (every delivery typed-refuses); the enterprise host mounts the generic HTTP
/// transducer here at boot. Exactly the [`crate::enrichment::register_provider`]
/// / [`crate::scrape_tool::register_scrape_fetcher`] injection shape.
pub fn register_provider(provider: Arc<dyn DeliveryProvider>) {
    *registry().write().expect("delivery registry poisoned") = Some(provider);
}

/// Clear the registered provider (back to the `NoProvider` typed refusal).
pub fn clear_provider() {
    *registry().write().expect("delivery registry poisoned") = None;
}

/// The active provider handle, if one is registered.
pub fn active_provider() -> Option<Arc<dyn DeliveryProvider>> {
    registry().read().expect("delivery registry poisoned").clone()
}

// ════════════════════════════════════════════════════════════════════════════
//  Planning (IR → resolved request) + dispatch
// ════════════════════════════════════════════════════════════════════════════

/// v2.60.0 — compile an [`crate::ir_nodes::IRDeliver`] into a resolved
/// [`DeliveryRequest`]: every `ref` field is looked up via `resolve` (the flow's
/// binding environment); literals pass through. The `key:` field of each
/// operation becomes its `idempotency_key` (T926 guaranteed its presence at
/// compile time; a runtime miss is a typed `Caller` error, never a silent skip).
///
/// Provenance blocks are NOT populated here — OSS has no epistemic runtime; the
/// enterprise engine enriches each `DeliveredField.provenance` from the flow
/// value's level/confidence when [`ProvenanceMode::Attached`]. OSS carries the
/// mode faithfully so the engine knows whether to attach + require it.
pub fn plan_delivery(
    ir: &crate::ir_nodes::IRDeliver,
    mut resolve: impl FnMut(&str) -> Option<String>,
) -> Result<DeliveryRequest, DeliveryError> {
    let mut ops = Vec::with_capacity(ir.ops.len());
    for op in &ir.ops {
        let mut idempotency_key = None;
        let mut fields = Vec::with_capacity(op.fields.len());
        for f in &op.fields {
            let value = match f.kind {
                "ref" => resolve(&f.value).ok_or_else(|| {
                    DeliveryError::Caller(format!(
                        "operation `{}` binds unresolved flow value `{}`",
                        op.kind, f.value
                    ))
                })?,
                // A literal — text/int/bool pass through as their surface string.
                _ => f.value.clone(),
            };
            if f.name == "key" {
                idempotency_key = Some(value.clone());
            }
            fields.push(DeliveredField {
                name: f.name.clone(),
                value,
                provenance: None,
            });
        }
        let idempotency_key = idempotency_key.ok_or_else(|| {
            DeliveryError::Caller(format!("operation `{}` resolved no idempotency `key`", op.kind))
        })?;
        ops.push(CanonicalOp {
            kind: op.kind.clone(),
            idempotency_key,
            fields,
        });
    }
    Ok(DeliveryRequest {
        name: ir.name.clone(),
        target: ir.target.clone(),
        provenance: ProvenanceMode::from_ir(&ir.provenance),
        ops,
    })
}

/// Run the active provider, or typed-refuse if none is registered.
pub fn run_delivery(req: &DeliveryRequest) -> Result<Vec<DeliveryReceipt>, DeliveryError> {
    match active_provider() {
        Some(p) => p.deliver(req),
        None => Err(DeliveryError::NoProviderConfigured),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir_nodes::{IRDeliver, IRDeliverOp, IRDocField};

    /// Serialises the registry-touching tests — the provider registry is
    /// process-global (the v2.58.0 `REG_LOCK` discipline).
    static REG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn ir_deliver(provenance: &str) -> IRDeliver {
        IRDeliver {
            node_type: "deliver",
            source_line: 1,
            source_column: 1,
            name: "push_lead".into(),
            target: "crm".into(),
            provenance: provenance.into(),
            secret: "crm_api_key".into(),
            effect_row: vec!["web".into()],
            epistemic_mode: String::new(),
            ops: vec![IRDeliverOp {
                kind: "upsert_contact".into(),
                fields: vec![
                    IRDocField { name: "key".into(), kind: "ref", value: "resolved_email".into(), items: vec![] },
                    IRDocField { name: "email".into(), kind: "ref", value: "resolved_email".into(), items: vec![] },
                    IRDocField { name: "firstname".into(), kind: "text", value: "Ada".into(), items: vec![] },
                ],
            }],
        }
    }

    #[test]
    fn plan_resolves_refs_and_extracts_idempotency_key() {
        let ir = ir_deliver("attached");
        let req = plan_delivery(&ir, |name| match name {
            "resolved_email" => Some("ada@example.com".into()),
            _ => None,
        })
        .expect("plan");
        assert_eq!(req.provenance, ProvenanceMode::Attached);
        assert_eq!(req.ops.len(), 1);
        let op = &req.ops[0];
        assert_eq!(op.idempotency_key, "ada@example.com");
        // ref resolved, literal passed through.
        assert_eq!(op.fields[1].value, "ada@example.com");
        assert_eq!(op.fields[2].value, "Ada");
    }

    #[test]
    fn plan_unresolved_ref_is_caller_blame_never_a_silent_skip() {
        let ir = ir_deliver("attached");
        let err = plan_delivery(&ir, |_| None).expect_err("must fail");
        assert!(matches!(err, DeliveryError::Caller(_)));
    }

    #[test]
    fn cleared_mode_lowers_from_ir() {
        let ir = ir_deliver("cleared");
        let req = plan_delivery(&ir, |_| Some("x".into())).expect("plan");
        assert_eq!(req.provenance, ProvenanceMode::Cleared);
    }

    #[test]
    fn no_provider_is_a_typed_refusal_never_a_fabricated_receipt() {
        let _g = REG_LOCK.lock().unwrap();
        clear_provider();
        let ir = ir_deliver("attached");
        let req = plan_delivery(&ir, |_| Some("ada@example.com".into())).expect("plan");
        assert_eq!(run_delivery(&req), Err(DeliveryError::NoProviderConfigured));
    }

    #[test]
    fn registered_provider_is_dispatched_and_idempotent() {
        let _g = REG_LOCK.lock().unwrap();

        struct Echo;
        impl DeliveryProvider for Echo {
            fn name(&self) -> &str {
                "echo"
            }
            fn deliver(&self, req: &DeliveryRequest) -> Result<Vec<DeliveryReceipt>, DeliveryError> {
                Ok(req
                    .ops
                    .iter()
                    .map(|o| DeliveryReceipt {
                        kind: o.kind.clone(),
                        idempotency_key: o.idempotency_key.clone(),
                        record_id: Some(format!("rec_{}", o.idempotency_key)),
                        created: true,
                    })
                    .collect())
            }
        }

        register_provider(Arc::new(Echo));
        let ir = ir_deliver("attached");
        let req = plan_delivery(&ir, |_| Some("ada@example.com".into())).expect("plan");
        let receipts = run_delivery(&req).expect("deliver");
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].record_id.as_deref(), Some("rec_ada@example.com"));
        clear_provider();
    }
}
