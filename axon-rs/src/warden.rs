//! v2.43.0 — the `WardenBackend` port + the OSS reference static analyzer.
//!
//! This is the OSS half of the `warden` primitive's RUNTIME. It defines:
//!   - [`Vulnerability`] — the ATTESTED finding type. A finding is NOT LLM prose;
//! it is a paraconsistent **contradiction** (paper section 5.3) between a system's
//!     declared contract and its observed behaviour, carrying a re-checkable
//!     [`Witness`] (the concrete input + trace + violated contract). A finding
//!     without a witness is not a finding — [`verify`] rejects it.
//!   - [`WardenBackend`] — the **port** (charter split R1). Enterprise mounts the
//! real LLM-abduction engine (v2.43.0) behind this trait; OSS ships only the
//!     bounded, deterministic [`ReferenceStaticWarden`] below.
//!   - [`ReferenceStaticWarden`] — a real (if minimal) static analyzer over an
//!     operator-provided, in-scope text artifact. It runs a closed set of
//!     deterministic pattern checks (unsafe calls, hard-coded secrets, SQL
//!     concatenation), each emitting a `Vulnerability` whose witness is the exact
//! offending line. **Precision over recall** (paper section 5.3): it reports only
//!     what it can attest.
//!
//! **Authorization is enforced here, not assumed** (paper section 5.2): [`analyze`]
//! refuses evidence whose target is not in the scope's allowlist
//! ([`WardenError::TargetNotAuthorized`]) and refuses any depth above
//! `static_artifact` in OSS ([`WardenError::DepthNotSupported`] — the invasive
//! depths are enterprise-only, v2.43.0). No unscoped, un-authorized analysis path
//! exists. No advantage is claimed (v2.23.0): the reference is exact pattern
//! matching; the value is the attested-witness discipline + the governance.

/// The re-checkable proof a [`Vulnerability`] carries — the paraconsistent
/// contradiction made concrete (paper section 5.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Witness {
    /// The concrete input / offending construct that triggers the finding.
    pub input: String,
    /// The observed trace (here: the source location + line).
    pub trace: String,
    /// The declared contract the observed behaviour violates.
    pub contract_violated: String,
}

impl Witness {
    /// A witness is well-formed iff it actually attests something — a non-empty
    /// input AND a stated contract violation. The load-bearing check behind
    /// [`verify`].
    pub fn is_attested(&self) -> bool {
        !self.input.trim().is_empty() && !self.contract_violated.trim().is_empty()
    }
}

/// An attested security finding.
#[derive(Debug, Clone, PartialEq)]
pub struct Vulnerability {
    /// The finding class (a closed taxonomy slug, e.g. `unsafe_call`).
    pub class: String,
    /// The analysed resource this finding is about.
    pub target: String,
    /// `low | medium | high | critical`.
    pub severity: String,
    /// Analyst confidence in `[0, 1]`.
    pub confidence: f64,
    /// The re-checkable proof.
    pub witness: Witness,
}

/// The evidence artifact under analysis — operator-provided, in-scope.
#[derive(Debug, Clone)]
pub struct Evidence {
    /// The resource id this artifact represents (must be in the scope allowlist).
    pub target: String,
    /// The artifact bytes. The reference analyzer treats them as UTF-8 text
    /// (source / config); non-text kinds are the enterprise engine's domain.
    pub content: Vec<u8>,
}

/// The resolved authorization scope (from a compiled `scope` declaration).
#[derive(Debug, Clone)]
pub struct AnalysisScope {
    pub targets: Vec<String>,
    pub depth: String,
    pub approver: String,
}

/// A structured warden failure — never a silent empty result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WardenError {
    /// The evidence's target is not in the scope's allowlist (fail-closed).
    TargetNotAuthorized { target: String },
    /// The requested depth is above what this backend supports (OSS = only
    /// `static_artifact`; the invasive depths are enterprise, v2.43.0).
    DepthNotSupported { depth: String },
    /// The scope carries no approver — an unapproved scope authorises nothing.
    Unapproved,
}

impl std::fmt::Display for WardenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WardenError::TargetNotAuthorized { target } => write!(
                f,
                "warden: target '{target}' is not in the authorization scope's allowlist"
            ),
            WardenError::DepthNotSupported { depth } => write!(
                f,
                "warden: analysis depth '{depth}' is not supported by this backend (the OSS \
                 reference supports only 'static_artifact'; invasive depths are enterprise-only)"
            ),
            WardenError::Unapproved => {
                write!(f, "warden: the authorization scope names no approver")
            }
        }
    }
}

impl std::error::Error for WardenError {}

/// v2.89.0 — **the deploy gate.** Which declared scopes this backend cannot
/// honour.
///
/// # Why this exists at DEPLOY and not only at dispatch
///
/// v2.43.0 already refuses a depth above the backend's ceiling, and refuses it
/// LOUDLY — [`WardenError::DepthNotSupported`], never a silent downgrade. That
/// was never the defect. The defect is WHEN.
///
/// `can_analyze_depth` was called only from inside [`WardenBackend::analyze`],
/// so a flow whose seventh step is `warden(…) within LiveScope` ran steps one
/// through six — with their emits, writes, deliveries and notifications — and
/// only then discovered that the audit it was built around cannot run on this
/// deployment. The program was refused after it had already acted.
///
/// Everything needed to answer this is present at deploy: the compiled `scope`
/// catalog and the mounted backend arrive at the same call site. v2.89.0 opened on
/// exactly this charge — *"the compiler accepts `depth: live_network`, the OSS
/// runtime rejects it"* — and the founder's ruling is that the deploy
/// gate is the SOURCE OF TRUTH, not a rung below a compile-time check: a
/// compile-time check is only as honest as the manifest it reads, so if the
/// build declares a capability and mounts a backend without it, the lie merely
/// moves earlier.
///
/// Returns one adopter-facing diagnostic per unsupported scope. Empty ⇒ every
/// declared scope is within reach of the mounted engine.
pub fn unsupported_scope_depths(
    backend: &dyn WardenBackend,
    scopes: &[crate::ir_nodes::IRScope],
) -> Vec<String> {
    scopes
        .iter()
        .filter(|s| !backend.can_analyze_depth(&s.depth))
        .map(|s| {
            format!(
                "scope `{}` declares `depth: {}`, which the mounted warden backend cannot \
                 analyse. Refusing at DEPLOY rather than mid-flow: a `warden` block that fails \
                 on its seventh step has already let the six before it emit, persist and \
                 deliver. The OSS reference supports `static_artifact` only; the invasive \
                 depths (`memory_dump`, `live_network`) require the enterprise engine. \
                 Either lower the scope's ceiling or deploy against a backend that reaches it.",
                s.name, s.depth
            )
        })
        .collect()
}

/// The paraconsistent finding-validator (paper section 5.3): a `Vulnerability` is valid
/// iff its witness attests something. An un-witnessed finding is noise and does
/// not cross the type boundary — the runtime rejects it and (in a flow) retries
/// via `immune`.
pub fn verify(v: &Vulnerability) -> bool {
    v.witness.is_attested()
        && !v.class.trim().is_empty()
        && (0.0..=1.0).contains(&v.confidence)
}

/// The warden analysis port (charter split R1). Enterprise mounts the LLM
/// abduction engine (v2.43.0) behind this trait, witness-gated (v2.23.0).
pub trait WardenBackend {
    /// Whether this backend can analyse at the given depth.
    fn can_analyze_depth(&self, depth: &str) -> bool;
    /// Analyse authorised, in-scope evidence, returning attested findings.
    /// Fails closed on any authorization breach.
    fn analyze(
        &self,
        evidence: &Evidence,
        scope: &AnalysisScope,
    ) -> Result<Vec<Vulnerability>, WardenError>;
}

/// One deterministic static check: a pattern + how to describe a match.
struct StaticCheck {
    needle: &'static str,
    class: &'static str,
    severity: &'static str,
    contract: &'static str,
}

/// The closed catalog of reference static checks. Real, well-known signals; the
/// enterprise engine (v2.43.0) does the open-ended abductive analysis.
const STATIC_CHECKS: &[StaticCheck] = &[
    StaticCheck {
        needle: "strcpy(",
        class: "unsafe_call",
        severity: "high",
        contract: "no unbounded string copy (strcpy has no length bound → buffer overflow)",
    },
    StaticCheck {
        needle: "gets(",
        class: "unsafe_call",
        severity: "critical",
        contract: "no unbounded stdin read (gets cannot be used safely)",
    },
    StaticCheck {
        needle: "system(",
        class: "command_injection_risk",
        severity: "high",
        contract: "no shell invocation on unsanitised input (command injection surface)",
    },
    StaticCheck {
        needle: "password =",
        class: "hardcoded_secret",
        severity: "critical",
        contract: "secrets are not hard-coded in source (must come from a secret store)",
    },
];

/// The OSS reference static analyzer: deterministic, bounded, attested.
pub struct ReferenceStaticWarden;

impl WardenBackend for ReferenceStaticWarden {
    fn can_analyze_depth(&self, depth: &str) -> bool {
        // Deny-by-default: OSS analyses only operator-provided static artifacts.
        depth.is_empty() || depth == "static_artifact"
    }

    fn analyze(
        &self,
        evidence: &Evidence,
        scope: &AnalysisScope,
    ) -> Result<Vec<Vulnerability>, WardenError> {
        // 1. Authorization: the scope must be approved.
        if scope.approver.trim().is_empty() {
            return Err(WardenError::Unapproved);
        }
        // 2. Depth: deny-by-default for anything above static_artifact.
        if !self.can_analyze_depth(&scope.depth) {
            return Err(WardenError::DepthNotSupported {
                depth: scope.depth.clone(),
            });
        }
        // 3. Allowlist: the evidence target MUST be in the scope's allowlist
        // (the runtime enforcement the frontend v2.43.0 deferred, fail-closed).
        if !scope.targets.iter().any(|t| t == &evidence.target) {
            return Err(WardenError::TargetNotAuthorized {
                target: evidence.target.clone(),
            });
        }

        // 4. Deterministic static analysis. Each match → an attested finding.
        let text = String::from_utf8_lossy(&evidence.content);
        let mut findings = Vec::new();
        for (lineno, line) in text.lines().enumerate() {
            for check in STATIC_CHECKS {
                if line.contains(check.needle) {
                    let v = Vulnerability {
                        class: check.class.to_string(),
                        target: evidence.target.clone(),
                        severity: check.severity.to_string(),
                        confidence: 0.9,
                        witness: Witness {
                            input: check.needle.to_string(),
                            trace: format!("line {}: {}", lineno + 1, line.trim()),
                            contract_violated: check.contract.to_string(),
                        },
                    };
                    // Precision over recall: only emit a witnessed finding.
                    if verify(&v) {
                        findings.push(v);
                    }
                }
            }
        }
        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> AnalysisScope {
        AnalysisScope {
            targets: vec!["svc://payments".to_string()],
            depth: "static_artifact".to_string(),
            approver: "security.lead".to_string(),
        }
    }

    fn evidence(src: &str) -> Evidence {
        Evidence {
            target: "svc://payments".to_string(),
            content: src.as_bytes().to_vec(),
        }
    }

    #[test]
    fn detects_unsafe_call_with_a_witness() {
        let w = ReferenceStaticWarden;
        let ev = evidence("int f(char* s) {\n  strcpy(buf, s);\n  return 0;\n}");
        let found = w.analyze(&ev, &scope()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].class, "unsafe_call");
        assert_eq!(found[0].severity, "high");
        assert!(found[0].witness.is_attested());
        assert!(found[0].witness.trace.contains("line 2"));
        assert!(verify(&found[0]));
    }

    #[test]
    fn detects_hardcoded_secret() {
        let w = ReferenceStaticWarden;
        let ev = evidence("const config = {\n  password = \"hunter2\"\n}");
        let found = w.analyze(&ev, &scope()).unwrap();
        assert!(found.iter().any(|v| v.class == "hardcoded_secret"));
    }

    #[test]
    fn clean_artifact_yields_no_findings() {
        let w = ReferenceStaticWarden;
        let ev = evidence("fn safe() -> i32 {\n  let x = 1;\n  x + 1\n}");
        assert!(w.analyze(&ev, &scope()).unwrap().is_empty());
    }

    #[test]
    fn verify_rejects_a_witnessless_finding() {
        let bogus = Vulnerability {
            class: "made_up".to_string(),
            target: "x".to_string(),
            severity: "high".to_string(),
            confidence: 0.99,
            witness: Witness {
                input: String::new(), // no attestation
                trace: String::new(),
                contract_violated: String::new(),
            },
        };
        assert!(!verify(&bogus), "an un-witnessed finding is not a finding");
    }

    #[test]
    fn deny_by_default_refuses_invasive_depth() {
        let w = ReferenceStaticWarden;
        assert!(!w.can_analyze_depth("memory_dump"));
        assert!(!w.can_analyze_depth("live_network"));
        let mut s = scope();
        s.depth = "live_network".to_string();
        assert!(matches!(
            w.analyze(&evidence("x"), &s),
            Err(WardenError::DepthNotSupported { .. })
        ));
    }

    #[test]
    fn refuses_evidence_outside_the_allowlist() {
        let w = ReferenceStaticWarden;
        let ev = Evidence {
            target: "svc://not-authorized".to_string(),
            content: b"strcpy(a,b);".to_vec(),
        };
        assert!(matches!(
            w.analyze(&ev, &scope()),
            Err(WardenError::TargetNotAuthorized { .. })
        ));
    }

    #[test]
    fn refuses_an_unapproved_scope() {
        let w = ReferenceStaticWarden;
        let mut s = scope();
        s.approver = String::new();
        assert!(matches!(
            w.analyze(&evidence("strcpy(a,b);"), &s),
            Err(WardenError::Unapproved)
        ));
    }
}
