//! §Fase 40.b — Shield scanner extension point.
//!
//! # Why this exists
//!
//! Per the OSS / ENTERPRISE / SPLIT charter, OSS axon ships the shield
//! *framework* (the `shield apply` algebraic-effect handler + wire shape)
//! but **no scanners** — the OSS default is an identity passthrough. The
//! vertical scanner *implementations* (HIPAA / legal / AML) are enterprise
//! R&D and live in the BSL `axon-enterprise` workspace.
//!
//! Before Fase 40.b there was no clean way for an external crate to inject
//! a scanner: the apply helper was a hardcoded identity. This module is the
//! **public registration hook** the enterprise vertical crate uses. It is a
//! deliberate language extension point — axon-for-axon: it makes axon a
//! better host language for privileged downstream layers, independent of
//! who registers scanners.
//!
//! # Model
//!
//! A [`ShieldScanner`] is registered under a shield *name* (the same name
//! used in `shield apply <name> to <target>`). At dispatch time the
//! `shield apply` handler looks the name up:
//!
//! - **registered** → run the scanner, which returns a [`ShieldVerdict`]
//!   (`Pass` with possibly-redacted content, or `Reject` with a stable
//!   blame code + adopter-facing reason);
//! - **not registered** → OSS identity passthrough (backwards-compatible;
//!   adopters with no enterprise layer see their data unmodified).
//!
//! # Thread-safety / lifecycle
//!
//! The registry is a process-global behind an `RwLock`. Enterprise
//! registers its scanners once at server boot (mirroring the pre-v2.0.0
//! Python `default_registry`). Registration is `last-wins` per name, so a
//! deployment can override a scanner deterministically.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

/// Context handed to a [`ShieldScanner`] on each invocation.
///
/// Intentionally minimal in 40.b (the field the scanner always needs); the
/// vertical scanners landing in 40.c extend their behaviour through their
/// own state, not by widening this struct, to keep the trait stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShieldScanContext {
    /// The shield name as written in `shield apply <name> ...`.
    pub shield_name: String,
}

impl ShieldScanContext {
    /// Construct a context for `shield_name`.
    pub fn new(shield_name: impl Into<String>) -> Self {
        Self {
            shield_name: shield_name.into(),
        }
    }
}

/// A scanner's verdict on a target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShieldVerdict {
    /// Content is allowed through, possibly transformed/redacted. The
    /// returned `String` is bound as the shield step's output.
    Pass(String),
    /// Content is rejected by policy. `code` is a stable slug for blame
    /// attribution (e.g. `"hipaa.phi_unredacted"`); `reason` is the
    /// adopter-facing message. The dispatcher surfaces this as a
    /// `DispatchError::BackendError { name: "shield:<name>", ... }`.
    Reject {
        /// Stable machine slug for blame attribution / metrics.
        code: String,
        /// Human-readable, adopter-facing rejection reason.
        reason: String,
    },
}

impl ShieldVerdict {
    /// Convenience constructor for a passing verdict.
    pub fn pass(content: impl Into<String>) -> Self {
        Self::Pass(content.into())
    }

    /// Convenience constructor for a rejecting verdict.
    pub fn reject(code: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Reject {
            code: code.into(),
            reason: reason.into(),
        }
    }

    /// True for [`ShieldVerdict::Pass`].
    pub fn is_pass(&self) -> bool {
        matches!(self, Self::Pass(_))
    }
}

/// Implemented by enterprise vertical scanners (HIPAA / legal / AML).
///
/// OSS ships **no** implementations. A scanner is pure-ish from the
/// dispatcher's perspective: given a target string + context it returns a
/// verdict. Scanners must be `Send + Sync` (the registry is shared across
/// the async runtime's worker threads).
pub trait ShieldScanner: Send + Sync {
    /// Scan `target` and return a [`ShieldVerdict`].
    fn scan(&self, target: &str, ctx: &ShieldScanContext) -> ShieldVerdict;
}

// ────────────────────────────────────────────────────────────────────────
//  Process-global registry
// ────────────────────────────────────────────────────────────────────────

static REGISTRY: LazyLock<RwLock<HashMap<String, Arc<dyn ShieldScanner>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Register `scanner` under `shield_name`. Returns the previously
/// registered scanner for that name, if any (last-wins). Safe to call from
/// any thread; intended to run once per name at startup.
pub fn register_shield_scanner(
    shield_name: impl Into<String>,
    scanner: Arc<dyn ShieldScanner>,
) -> Option<Arc<dyn ShieldScanner>> {
    REGISTRY
        .write()
        .expect("shield registry RwLock poisoned")
        .insert(shield_name.into(), scanner)
}

/// Look up the scanner registered under `shield_name`, if any.
pub fn lookup_shield_scanner(shield_name: &str) -> Option<Arc<dyn ShieldScanner>> {
    REGISTRY
        .read()
        .expect("shield registry RwLock poisoned")
        .get(shield_name)
        .cloned()
}

/// §Fase 122.a — what a shield site must do, decided ONCE for every site.
///
/// There are two shield enforcement sites (`run_shield_apply` and `run_emit`'s
/// σ-gate) and they must never disagree about when a missing scanner is
/// tolerable. §120 is the reason this is a projection and not a rule: two
/// public entry points into one subsystem, each with its own copy of the
/// decision, drifted in silence for entire fases. Here the decision has one
/// home and the sites only obey it.
pub enum ScanDisposition {
    /// A scanner is registered — run it.
    Scanner(Arc<dyn ShieldScanner>),
    /// No scanner, and the shield DECLARES no `scan:`. Identity is honest:
    /// a filter that filters nothing leaves the data untouched and claims
    /// nothing about it (§119.c's argument, kept deliberately).
    IdentityIsHonest,
    /// No scanner, but the shield DECLARES a `scan:` list. Refuse.
    ///
    /// A declared scan is not a request to filter — it is an ASSERTION about
    /// the content ("nothing past this point carries an injection"), which the
    /// PCC proves over and the ESK maps to ISO 27001 A.8.23. Passing the value
    /// through and binding it under a name that claims the property is the
    /// §111 F12 shape `warden` already refuses: a clean-looking result for
    /// something never examined. It is also what makes §98.d's web-taint
    /// barrier real — `<web>` content is born Untrusted and `prompt_injection`
    /// is what it must pass; an identity there tracks the taint at compile
    /// time and evaporates it at runtime.
    UnhonouredScan {
        /// The adopter-facing refusal, naming the scans nobody can run.
        message: String,
    },
}

/// §Fase 122.a — resolve what a shield site must do for `shield_name`, given
/// the `scan:` list the artifact carries for it (stamped at lowering).
pub fn resolve_scan(shield_name: &str, declared_scan: &[String]) -> ScanDisposition {
    if let Some(scanner) = lookup_shield_scanner(shield_name) {
        return ScanDisposition::Scanner(scanner);
    }
    if declared_scan.is_empty() {
        return ScanDisposition::IdentityIsHonest;
    }
    ScanDisposition::UnhonouredScan {
        message: format!(
            "shield `{shield_name}` declares `scan: [{}]` and NO scanner is registered under that \
             name, so nothing examined this content. Passing it through would bind a value under a \
             name asserting a property that was never checked — the one result a content barrier \
             must never fabricate (the posture `warden` already takes for an unread \
             target). This build ships no scanners for `{shield_name}`: register one via \
             `axon::shield_registry::register_shield_scanner` (the enterprise vertical crates \
             register the HIPAA / legal / AML / prompt-injection scanners), or remove the `scan:` \
             declaration if the shield is only meant to filter — a shield with no `scan:` still \
             passes content through untouched, honestly.",
            declared_scan.join(", ")
        ),
    }
}

/// True when at least one scanner is registered. Cheap O(1)-ish guard so
/// the dispatcher can skip the lookup entirely in the common OSS case (no
/// enterprise layer present).
pub fn has_registered_scanners() -> bool {
    !REGISTRY
        .read()
        .expect("shield registry RwLock poisoned")
        .is_empty()
}

/// All registered shield names, sorted (for discovery endpoints + audit
/// diagnostics). Deterministic ordering so wire/log output is stable.
pub fn registered_shield_names() -> Vec<String> {
    let mut names: Vec<String> = REGISTRY
        .read()
        .expect("shield registry RwLock poisoned")
        .keys()
        .cloned()
        .collect();
    names.sort();
    names
}

/// Remove the scanner registered under `shield_name`, returning it if
/// present. Mainly for deployments that hot-swap scanners + for tests.
pub fn unregister_shield_scanner(shield_name: &str) -> Option<Arc<dyn ShieldScanner>> {
    REGISTRY
        .write()
        .expect("shield registry RwLock poisoned")
        .remove(shield_name)
}

/// Clear the entire registry. Test-support + clean-shutdown helper.
#[doc(hidden)]
pub fn clear_shield_registry() {
    REGISTRY
        .write()
        .expect("shield registry RwLock poisoned")
        .clear();
}

// ────────────────────────────────────────────────────────────────────────
//  §Fase 53.e — NO PHANTOM GUARDRAILS (founder refinement C)
// ────────────────────────────────────────────────────────────────────────

/// Every `(shield_name, category)` where a shield declares an
/// EXTENSION-introduced scan category (one declared via an
/// `extension { category: scan }` block) but has **no registered
/// scanner** — i.e. a guardrail the operator believes is active that is
/// actually a silent no-op.
///
/// Canonical scan categories are intentionally NOT gated: they carry a
/// documented framework meaning, and the OSS identity passthrough (no
/// scanner) is the backwards-compatible default. Only adopter-introduced
/// extension categories — which have NO default semantics — require an
/// explicit scanner; serving one unscanned is a false sense of security.
pub fn unscanned_extension_scan_categories(
    ir: &crate::ir_nodes::IRProgram,
) -> Vec<(String, String)> {
    let mut ext_cats: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for ext in &ir.extensions {
        if ext.category == "scan" {
            for m in &ext.members {
                ext_cats.insert(m.name.as_str());
            }
        }
    }
    if ext_cats.is_empty() {
        return Vec::new();
    }
    let mut violations = Vec::new();
    for shield in &ir.shields {
        // A registered scanner owns the shield: it is responsible for the
        // declared categories. Only a shield with NO scanner can leave an
        // extension category as a ghost guardrail.
        if lookup_shield_scanner(&shield.name).is_some() {
            continue;
        }
        for cat in &shield.scan {
            if ext_cats.contains(cat.as_str()) {
                violations.push((shield.name.clone(), cat.clone()));
            }
        }
    }
    violations
}

/// §Fase 53.e — the boot gate. `Ok(())` when every extension scan
/// category used by a shield has a registered scanner; `Err(blame)` (a
/// Server-Blame message) otherwise. The boot sequence MUST treat `Err`
/// as FATAL — refuse to serve rather than present a ghost guardrail
/// (founder refinement C: no silent no-op, fail loud).
pub fn check_extension_scan_coverage(ir: &crate::ir_nodes::IRProgram) -> Result<(), String> {
    let violations = unscanned_extension_scan_categories(ir);
    if violations.is_empty() {
        return Ok(());
    }
    let detail = violations
        .iter()
        .map(|(s, c)| format!("shield '{s}' → scan category '{c}'"))
        .collect::<Vec<_>>()
        .join("; ");
    Err(format!(
        "refusing to boot — extension scan categor(ies) declared but \
         UNSCANNED (no scanner registered): {detail}. An `extension` scan category \
         has no default meaning; serving it as a silent no-op would be a phantom \
         guardrail. Register a scanner for the shield(s) or remove the category."
    ))
}

// ────────────────────────────────────────────────────────────────────────
//  §Fase 114.w — the `on_breach:` catalog, HONORED
// ────────────────────────────────────────────────────────────────────────
//
// The shield doc has promised five policies since Fase 20:
//
// | `halt`               | stop; surface a typed error                    |
// | `quarantine`         | route the candidate to the `quarantine:` sink |
// | `deflect`            | emit the `deflect_message:` instead           |
// | `sanitize_and_retry` | apply `redact:` + re-scan, ≤ `max_retries:`   |
// | `escalate`           | hand off to the escalation queue              |
//
// …and the runtime ALWAYS halted. §114.w gives each policy its documented
// meaning. Two seams stay registry-backed, mirroring the scanner model:
// the quarantine SINKS (named — enterprise mounts the DLQ under the audit
// hash chain) and the ESCALATION queue (one per process). A policy whose
// destination is not mounted HALTS with a diagnostic naming the hole —
// fail-closed, never a phantom guardrail (the §53.e doctrine).

/// A breach destination: where `quarantine` routes and `escalate` hands off.
/// OSS ships **no** implementations (charter: framework here, verticals in
/// enterprise — the DLQ-under-hash-chain sink is enterprise R&D).
pub trait BreachSink: Send + Sync {
    /// Persist the rejected candidate for later handling. An `Err` aborts
    /// into halt — a quarantine that cannot record must not pretend it did.
    ///
    /// `tenant_id` is the acquiring tenant (mirrors
    /// `axon::scrape_tool::ScrapeAuditSink::record`'s `tenant: &str` — a
    /// process-global hook still needs the caller's tenant so a
    /// multi-tenant sink can partition storage and RLS-scope its audit
    /// witness). A sink over regulated content that cannot see WHOSE
    /// content it is quarantining cannot honestly claim per-tenant
    /// recovery or compliance segregation.
    fn route(
        &self,
        tenant_id: &str,
        shield_name: &str,
        code: &str,
        reason: &str,
        candidate: &str,
    ) -> Result<(), String>;
}

static BREACH_SINKS: LazyLock<RwLock<HashMap<String, Arc<dyn BreachSink>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static ESCALATION_QUEUE: LazyLock<RwLock<Option<Arc<dyn BreachSink>>>> =
    LazyLock::new(|| RwLock::new(None));

/// Register a quarantine sink under its declared name (`shield … {
/// quarantine: <name> }`). Last-wins, like the scanner registry.
pub fn register_breach_sink(
    sink_name: impl Into<String>,
    sink: Arc<dyn BreachSink>,
) -> Option<Arc<dyn BreachSink>> {
    BREACH_SINKS
        .write()
        .expect("breach sink registry RwLock poisoned")
        .insert(sink_name.into(), sink)
}

/// Look up a quarantine sink by name.
pub fn lookup_breach_sink(sink_name: &str) -> Option<Arc<dyn BreachSink>> {
    BREACH_SINKS
        .read()
        .expect("breach sink registry RwLock poisoned")
        .get(sink_name)
        .cloned()
}

/// Remove a quarantine sink (hot-swap + tests).
pub fn unregister_breach_sink(sink_name: &str) -> Option<Arc<dyn BreachSink>> {
    BREACH_SINKS
        .write()
        .expect("breach sink registry RwLock poisoned")
        .remove(sink_name)
}

/// Install the process escalation queue (`on_breach: escalate` hands off
/// here). `None` uninstalls (tests / shutdown).
pub fn set_escalation_queue(queue: Option<Arc<dyn BreachSink>>) {
    *ESCALATION_QUEUE
        .write()
        .expect("escalation queue RwLock poisoned") = queue;
}

fn escalation_queue() -> Option<Arc<dyn BreachSink>> {
    ESCALATION_QUEUE
        .read()
        .expect("escalation queue RwLock poisoned")
        .clone()
}

/// What the enforcement site does after the policy ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreachDisposition {
    /// Fail closed with this message (halt / routed-then-refused /
    /// exhausted retries / missing destination).
    Halt { message: String },
    /// Continue with this content INSTEAD of the candidate — `deflect`'s
    /// canned reply, or `sanitize_and_retry`'s masked-and-now-passing value.
    Proceed { content: String },
}

/// Recursively mask every field named in `redact` (case-sensitive, the
/// declared spelling) anywhere in the JSON tree.
fn mask_fields(value: &mut serde_json::Value, redact: &[String]) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if redact.iter().any(|r| r == k) {
                    *v = serde_json::Value::String("[REDACTED]".to_string());
                } else {
                    mask_fields(v, redact);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for v in items.iter_mut() {
                mask_fields(v, redact);
            }
        }
        _ => {}
    }
}

/// §Fase 114.w — run the shield's declared `on_breach:` policy over a
/// REJECTED candidate. Called by BOTH enforcement sites (the shield STEP and
/// `run_emit`'s σ-gate) — the policy arrives ON THE NODE (`IRBreachPolicy`,
/// resolved at lowering), so every dispatch path honors it by construction.
///
/// `None` policy ⇒ `halt` (the fail-closed default, byte-identical to the
/// pre-§114.w behaviour).
pub fn apply_on_breach(
    tenant_id: &str,
    shield_name: &str,
    policy: Option<&crate::ir_nodes::IRBreachPolicy>,
    scanner: &Arc<dyn ShieldScanner>,
    candidate: &str,
    code: &str,
    reason: &str,
) -> BreachDisposition {
    let halt = |message: String| BreachDisposition::Halt { message };
    let base = format!("[{code}] {reason}");
    let Some(policy) = policy else {
        return halt(base);
    };

    match policy.on_breach.as_str() {
        // Route the candidate to the named sink, then REFUSE. Quarantine
        // differs from halt by making the candidate RECOVERABLE (the DLQ
        // reading), never by letting it through.
        "quarantine" if policy.quarantine.is_empty() => halt(format!(
            "{base} — `on_breach: quarantine` declares NO sink (axon-W012 warned at \
             compile); halting (fail-closed, the pre-114.w behaviour)"
        )),
        "quarantine" => match lookup_breach_sink(&policy.quarantine) {
            Some(sink) => match sink.route(tenant_id, shield_name, code, reason, candidate) {
                Ok(()) => halt(format!(
                    "{base} — candidate quarantined to sink '{}' (recoverable; the \
                     emission itself is refused)",
                    policy.quarantine
                )),
                Err(e) => halt(format!(
                    "{base} — quarantine sink '{}' FAILED to record the candidate \
                     ({e}); halting (a quarantine that cannot record must not \
                     pretend it did)",
                    policy.quarantine
                )),
            },
            None => halt(format!(
                "{base} — `on_breach: quarantine` names sink '{}', which is NOT \
                 mounted in this process; halting (fail-closed — a phantom \
                 quarantine would be a false sense of recovery)",
                policy.quarantine
            )),
        },
        // Hand off to the escalation queue, then REFUSE (a human decides).
        "escalate" => match escalation_queue() {
            Some(queue) => match queue.route(tenant_id, shield_name, code, reason, candidate) {
                Ok(()) => halt(format!(
                    "{base} — escalated to the escalation queue (a human decides; \
                     the emission itself is refused)"
                )),
                Err(e) => halt(format!(
                    "{base} — escalation queue FAILED to record ({e}); halting"
                )),
            },
            None => halt(format!(
                "{base} — `on_breach: escalate` but NO escalation queue is mounted \
                 in this process; halting (fail-closed)"
            )),
        },
        // Emit the canned safe reply INSTEAD of the candidate — the only
        // policy that proceeds, and it proceeds with DECLARED content, never
        // with any part of the rejected value. axon-T952 guarantees the
        // message is non-empty at compile time.
        "deflect" => BreachDisposition::Proceed {
            content: policy.deflect_message.clone(),
        },
        // Mask the declared `redact:` fields and RE-SCAN, up to
        // `max_retries:`. Only a candidate the scanner now PASSES proceeds;
        // a non-JSON candidate cannot be field-masked and halts (fail-closed
        // beats guessing at a sanitization the adopter never declared).
        "sanitize_and_retry" => {
            let Ok(mut value) = serde_json::from_str::<serde_json::Value>(candidate) else {
                return halt(format!(
                    "{base} — `on_breach: sanitize_and_retry` but the candidate is \
                     not JSON, so the declared `redact:` fields cannot be masked; \
                     halting (fail-closed)"
                ));
            };
            mask_fields(&mut value, &policy.redact);
            let masked = value.to_string();
            let budget = policy.max_retries.max(1);
            let ctx = ShieldScanContext::new(shield_name.to_string());
            for _attempt in 0..budget {
                match scanner.scan(&masked, &ctx) {
                    ShieldVerdict::Pass(content) => {
                        return BreachDisposition::Proceed { content };
                    }
                    ShieldVerdict::Reject { .. } => continue,
                }
            }
            halt(format!(
                "{base} — sanitize_and_retry exhausted {budget} attempt(s): the \
                 scanner still rejects after masking {:?}; halting",
                policy.redact
            ))
        }
        // `halt` and anything unrecognized (the checker forbids the latter,
        // defence in depth here): fail closed.
        _ => halt(base),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // NOTE on test isolation: the registry is a process-global and cargo
    // runs tests in parallel. These tests therefore use UNIQUE shield
    // names (disjoint keys never collide under the RwLock), clean up after
    // themselves with `unregister_shield_scanner`, and never assert on
    // GLOBAL state (emptiness / the full name list) — only on the keys they
    // own. `clear_shield_registry` is deliberately NOT used here (it would
    // nuke a concurrent test's registration).

    struct UppercaseScanner;
    impl ShieldScanner for UppercaseScanner {
        fn scan(&self, target: &str, _ctx: &ShieldScanContext) -> ShieldVerdict {
            ShieldVerdict::pass(target.to_uppercase())
        }
    }

    struct AlwaysReject;
    impl ShieldScanner for AlwaysReject {
        fn scan(&self, _target: &str, ctx: &ShieldScanContext) -> ShieldVerdict {
            ShieldVerdict::reject(
                format!("{}.blocked", ctx.shield_name),
                "policy rejection (test)",
            )
        }
    }

    // ── §Fase 53.e — phantom-guardrail boot gate ───────────────────

    fn ir_from(src: &str) -> crate::ir_nodes::IRProgram {
        let tokens = crate::lexer::Lexer::new(src, "<test>")
            .tokenize()
            .expect("lex");
        let program = crate::parser::Parser::new(tokens).parse().expect("parse");
        crate::ir_generator::IRGenerator::new().generate(&program)
    }

    struct PassScanner;
    impl ShieldScanner for PassScanner {
        fn scan(&self, target: &str, _ctx: &ShieldScanContext) -> ShieldVerdict {
            ShieldVerdict::pass(target.to_string())
        }
    }

    /// A shield using ONLY canonical scan categories (no scanner) is NOT
    /// a violation — the canonical passthrough is the documented default.
    #[test]
    fn canonical_category_without_scanner_is_not_a_violation() {
        let ir = ir_from(
            "shield T53e_canon { scan: [code_injection] strategy: pattern on_breach: halt }",
        );
        assert!(unscanned_extension_scan_categories(&ir).is_empty());
        assert!(check_extension_scan_coverage(&ir).is_ok());
    }

    /// A shield using an EXTENSION scan category with NO registered
    /// scanner is a phantom guardrail → reported + boot refused.
    #[test]
    fn extension_category_without_scanner_is_a_violation() {
        let ir = ir_from(
            "extension t53e_x { category: scan members: [ \"dunning_pressure\" ] }\n\
             shield T53e_ghost { scan: [dunning_pressure] strategy: pattern on_breach: halt }",
        );
        let v = unscanned_extension_scan_categories(&ir);
        assert_eq!(
            v,
            vec![("T53e_ghost".to_string(), "dunning_pressure".to_string())]
        );
        let err = check_extension_scan_coverage(&ir).expect_err("must refuse boot");
        assert!(err.contains("phantom guardrail"), "got: {err}");
        assert!(err.contains("dunning_pressure"), "got: {err}");
    }

    /// Same source, but a scanner registered under the shield name → the
    /// extension category is covered → no violation.
    #[test]
    fn extension_category_with_scanner_is_ok() {
        const SHIELD: &str = "T53e_covered";
        let _prev = register_shield_scanner(SHIELD, Arc::new(PassScanner));
        let ir = ir_from(&format!(
            "extension t53e_y {{ category: scan members: [ \"dunning_pressure\" ] }}\n\
             shield {SHIELD} {{ scan: [dunning_pressure] strategy: pattern on_breach: halt }}"
        ));
        let ok = check_extension_scan_coverage(&ir);
        // Clean up BEFORE asserting so a failure doesn't leak the scanner.
        unregister_shield_scanner(SHIELD);
        assert!(ok.is_ok(), "a registered scanner must cover the category: {ok:?}");
    }

    #[test]
    fn register_lookup_roundtrip() {
        const NAME: &str = "t_reg_roundtrip_upper";
        assert!(lookup_shield_scanner(NAME).is_none());

        let prev = register_shield_scanner(NAME, Arc::new(UppercaseScanner));
        assert!(prev.is_none(), "first registration has no predecessor");
        assert!(has_registered_scanners(), "at least our scanner is present");

        let s = lookup_shield_scanner(NAME).expect("registered");
        let v = s.scan("phi data", &ShieldScanContext::new(NAME));
        assert_eq!(v, ShieldVerdict::Pass("PHI DATA".to_string()));

        unregister_shield_scanner(NAME);
        assert!(lookup_shield_scanner(NAME).is_none());
    }

    #[test]
    fn last_wins_and_unregister() {
        const NAME: &str = "t_reg_last_wins";
        register_shield_scanner(NAME, Arc::new(UppercaseScanner));
        let prev = register_shield_scanner(NAME, Arc::new(AlwaysReject));
        assert!(prev.is_some(), "second registration returns the predecessor");

        let s = lookup_shield_scanner(NAME).unwrap();
        assert!(matches!(
            s.scan("x", &ShieldScanContext::new(NAME)),
            ShieldVerdict::Reject { .. }
        ));

        let removed = unregister_shield_scanner(NAME);
        assert!(removed.is_some());
        assert!(lookup_shield_scanner(NAME).is_none());
    }

    #[test]
    fn registered_names_includes_own_in_sorted_order() {
        // Unique prefix so we can filter out any concurrently-registered
        // scanners and assert only on the keys this test owns.
        let names = ["t_names_zeta", "t_names_alpha", "t_names_mu"];
        for n in names {
            register_shield_scanner(n, Arc::new(UppercaseScanner));
        }
        let mut mine: Vec<String> = registered_shield_names()
            .into_iter()
            .filter(|n| n.starts_with("t_names_"))
            .collect();
        // `registered_shield_names` is documented sorted; filtering
        // preserves order, so `mine` must already be sorted ascending.
        let mut expected = mine.clone();
        expected.sort();
        assert_eq!(mine, expected, "registered names must be returned sorted");
        mine.sort();
        assert_eq!(
            mine,
            vec![
                "t_names_alpha".to_string(),
                "t_names_mu".to_string(),
                "t_names_zeta".to_string()
            ]
        );
        for n in names {
            unregister_shield_scanner(n);
        }
    }

    #[test]
    fn verdict_constructors() {
        assert!(ShieldVerdict::pass("ok").is_pass());
        assert!(!ShieldVerdict::reject("c", "r").is_pass());
    }
}
