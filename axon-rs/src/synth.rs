//! v2.42.0 — the `SynthBackend` port + the OSS deny-by-default reference.
//!
//! `synth` (v2.42.0) declares the safety envelope under which a `savant` may write
//! and run a tool at runtime. This module is the RUNTIME port for that: the
//! enterprise engine (v2.42.0) mounts a Coder/Reviewer (`par`) → `wasm32-wasi` →
//! Extism/gVisor zero-trust executor behind [`SynthBackend`]. The OSS crate
//! ships ONLY [`DenyByDefaultSynth`], which **never executes** — running
//! untrusted synthesised code needs isolation OSS cannot provide.
//!
//! This is the runtime half of the v2.42.0 compile-time `sandbox: wasm`
//! deny-by-default gate (`axon-T882`): the checker stops an adopter *declaring*
//! an unsandboxed policy; this backend stops the OSS runtime *executing* one.

/// A request to synthesise (and, on the enterprise backend, run) a tool.
#[derive(Debug, Clone)]
pub struct SynthRequest {
    /// The `synth` policy this request runs under.
    pub policy_name: String,
    /// The risk class (`low|medium|high|critical`) from the policy.
    pub risk: String,
    /// The target source language (`rust|c|python`).
    pub language: String,
    /// The isolation tier (`wasm`). Anything else is refused fail-closed.
    pub sandbox: String,
    /// The capability the savant needs — what the tool must do.
    pub purpose: String,
}

/// What a successful synthesis+execution yields (enterprise backend only).
#[derive(Debug, Clone)]
pub struct SynthOutcome {
    /// A content-addressed id of the synthesised, reviewed tool.
    pub tool_id: String,
    /// The tool's stdout, ingested by the savant as empirical evidence.
    pub stdout: String,
}

/// A structured synthesis failure — never a silent no-op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SynthError {
    /// The backend refuses to execute synthesised code (the OSS default).
    ExecutionRefused { reason: String },
    /// The requested sandbox is not `wasm` (fail-closed).
    SandboxUnavailable { requested: String },
    /// The Coder/Reviewer consensus rejected the generated tool.
    ReviewRejected { reason: String },
}

impl std::fmt::Display for SynthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SynthError::ExecutionRefused { reason } => write!(f, "synth execution refused: {reason}"),
            SynthError::SandboxUnavailable { requested } => {
                write!(f, "synth sandbox '{requested}' unavailable — only 'wasm' is permitted")
            }
            SynthError::ReviewRejected { reason } => write!(f, "synth review rejected: {reason}"),
        }
    }
}

impl std::error::Error for SynthError {}

/// The dynamic tool-synthesis port (charter split R1). Enterprise mounts the
/// Extism/gVisor executor (v2.42.0) behind this trait.
pub trait SynthBackend {
    /// Whether this backend can execute synthesised code at all. The OSS
    /// reference returns `false`.
    fn can_execute(&self) -> bool;
    /// Synthesise + run a tool under the policy. The OSS reference always fails
    /// closed with [`SynthError::ExecutionRefused`].
    fn synthesize_and_run(&self, req: &SynthRequest) -> Result<SynthOutcome, SynthError>;
}

/// The OSS reference: it refuses to execute synthesised code, unconditionally.
/// Deploying dynamic tool synthesis requires the enterprise flavour (v2.42.0).
pub struct DenyByDefaultSynth;

impl SynthBackend for DenyByDefaultSynth {
    fn can_execute(&self) -> bool {
        false
    }

    fn synthesize_and_run(&self, req: &SynthRequest) -> Result<SynthOutcome, SynthError> {
        // Fail closed even before considering the request: OSS never runs
        // untrusted synthesised code. The sandbox check is defensive belt-and-
        // braces so a future OSS extension cannot accidentally weaken it.
        if req.sandbox != "wasm" {
            return Err(SynthError::SandboxUnavailable {
                requested: req.sandbox.clone(),
            });
        }
        Err(SynthError::ExecutionRefused {
            reason: format!(
                "the OSS SynthBackend never executes synthesised code (policy '{}'); the enterprise \
                 Extism/gVisor zero-trust executor is required",
                req.policy_name
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(sandbox: &str) -> SynthRequest {
        SynthRequest {
            policy_name: "Toolsmith".into(),
            risk: "medium".into(),
            language: "rust".into(),
            sandbox: sandbox.into(),
            purpose: "parse a dataset".into(),
        }
    }

    #[test]
    fn oss_backend_never_executes() {
        let b = DenyByDefaultSynth;
        assert!(!b.can_execute());
        assert!(matches!(
            b.synthesize_and_run(&req("wasm")),
            Err(SynthError::ExecutionRefused { .. })
        ));
    }

    #[test]
    fn non_wasm_sandbox_is_refused_first() {
        let b = DenyByDefaultSynth;
        assert!(matches!(
            b.synthesize_and_run(&req("none")),
            Err(SynthError::SandboxUnavailable { .. })
        ));
    }
}
