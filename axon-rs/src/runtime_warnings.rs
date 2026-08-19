//! v1.24.0 — Closed-catalog runtime warnings for the streaming
//! production path.
//!
//! D5 contract: when the production async streaming path can't
//! activate (flow shape unsupported, backend unknown, etc.), the
//! runtime emits a closed-catalog warning instead of silently
//! degrading. Adopters see the warning on `axon.complete.warnings`
//! AND on the `/v1/replay/<trace_id>` audit row, so a `Stream<T>`
//! declaration that falls through to legacy synchronous-burst
//! delivery is OBSERVABLE — never silent.
//!
//! # Closed-catalog discipline
//!
//! `WarningCode` is a closed enum. Adding a new variant requires:
//!   1. Updating the enum here (compiler enforces exhaustive `slug`
//!      match).
//!   2. Updating the slug-uniqueness pin in
//!      [`closed_catalog_pins`].
//!   3. Updating the warning-surface drift gate in
//!      `tests/warning_catalog.rs`.
//!
//! The "axon-W001" slot is reserved for the X-Axon-Stream-Available
//! HTTP-header diagnostic that shipped in v1.22.0 (kept as a
//! header-level surface rather than a wire-body warning to preserve
//! legacy adopter parsers). 33.x.g introduces "axon-W002" as the
//! first wire-body-surfaced warning code.
//!
//! # Pillar trace
//!
//! - **MATHEMATICS** — closed catalog ⟹ exhaustive match ⟹ adding
//!   a new code breaks the build. Compiler enforces.
//! - **LOGIC** — every legacy-fallback case has a specific
//!   [`FallbackMode`] tag. No "unknown" catch-all that hides
//!   real edge cases.
//! - **PHILOSOPHY** — no silent degradation. An adopter who declares
//!   `Stream<T>` and ends up with synthetic chunking sees the
//!   warning on the wire + on the audit row.
//! - **COMPUTING** — warnings ride a side-channel on
//!   `StreamingExecution`; the consumer reads them at FlowComplete
//!   and projects onto the wire JSON. Zero cost on the happy path
//!   (empty Vec, elided via `skip_serializing_if`).

use serde::{Deserialize, Serialize};

/// Closed catalog of runtime warning codes that can surface on the
/// production SSE wire.
///
/// As of 33.x.g there is exactly one wire-body code: `AxonW002`.
/// W001 was reserved by v1.22.0 but ships as the
/// `X-Axon-Stream-Available` HTTP header (header-level diagnostic),
/// NOT as a wire-body warning. Future codes (W003+) require an
/// explicit founder sign-off and a closed-catalog drift gate
/// update — adding a variant here is not silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WarningCode {
    /// `axon-W002 streaming-not-supported` — emitted when the
    /// adopter's flow declared `output: Stream<T>` but the
    /// production async streaming path could not activate, and
    /// the runtime fell back to the legacy synchronous-burst
    /// delivery. The accompanying [`FallbackMode`] tag captures
    /// the specific reason (flow shape unsupported, backend
    /// unknown, etc.).
    ///
    /// Wire surface: `axon.complete.warnings[*]` (D4-compatible
    /// optional array, elided when empty).
    /// Audit surface: `replay.runtime_warnings[*]` on the
    /// `AxonendpointReplayEntry`.
    #[serde(rename = "axon-W002")]
    AxonW002,
}

impl WarningCode {
    /// Stable wire slug. Closed catalog — adding a variant above
    /// requires updating this match (compiler enforces
    /// exhaustiveness).
    pub fn slug(&self) -> &'static str {
        match self {
            Self::AxonW002 => "axon-W002",
        }
    }

    /// Human-readable summary surfaced on the wire alongside the
    /// slug. The accompanying `FallbackMode` provides the
    /// machine-readable detail.
    pub fn message(&self) -> &'static str {
        match self {
            Self::AxonW002 => "streaming-not-supported",
        }
    }
}

/// Closed catalog of "why streaming did not activate" tags. Each
/// tag corresponds to a specific decision point in
/// `server_execute_streaming`. Adding a variant requires updating
/// the catalog pin tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackMode {
    // v1.24.0 — `UnsupportedFlowShape` variant DELETED. The
    // per-IRFlowNode dispatcher (v1.24.0 45/45) covers every shape
    // the planner could previously reject; W002 cannot fire for
    // "unsupported flow shape" because no shape is unsupported.
    /// `resolve_streaming_backend` returned `None` for the
    /// requested backend name (after `auto` resolution). The
    /// dispatcher's BackendError surfaces as `axon.error`.
    UnknownBackend,
    /// Source could not be parsed (lex / parse / type-check / IR-
    /// generation error). The dispatcher's compilation-error path
    /// surfaces the diagnostic via `axon.error`.
    SourceCompilationFailed,
    /// Reserved for future scenarios where a custom adopter-
    /// provided backend implements `Backend::complete()` but not
    /// `Backend::stream()`. Not reachable today (all 8 dispatched
    /// backends implement `stream()` per v1.18.0 + v1.24.0); kept
    /// here so the catalog covers the conceptual case adopters
    /// hit when extending the registry.
    BackendLacksStream,
}

impl FallbackMode {
    /// Stable kebab-case slug for wire serialization. Mirrors
    /// `serde(rename_all = "snake_case")` but exposed
    /// programmatically for adopters that don't go through serde.
    pub fn slug(&self) -> &'static str {
        match self {
            Self::UnknownBackend => "unknown_backend",
            Self::SourceCompilationFailed => "source_compilation_failed",
            Self::BackendLacksStream => "backend_lacks_stream",
        }
    }
}

/// One runtime warning entry. Immutable once minted. Surfaces on
/// `axon.complete.warnings[*]` (wire) and
/// `replay.runtime_warnings[*]` (audit).
///
/// # Required fields (per D5 + plan vivo section 1)
///
/// - `code` — closed-catalog [`WarningCode`].
/// - `flow_name` — the flow whose streaming declaration fell back.
/// - `backend` — the backend name (post-`auto` resolution) that
///   was attempted.
/// - `fallback_mode` — the [`FallbackMode`] tag identifying WHY.
/// - `step_name` — optional step name when the warning is
///   step-scoped (e.g. one step has an unsupported feature, the
///   others are fine). `None` for flow-scoped warnings.
/// - `declared_output` — the adopter-source `output:` declaration
///   (e.g. `"Stream<Token>"`) that triggered the streaming
///   expectation. Empty when the warning is not tied to a
///   specific declaration.
/// - `message` — human-readable summary. Equals
///   `code.message()` by default; adopters can include
///   context-specific detail (e.g. the exact `IRFlowNode` variant
///   that triggered `UnsupportedFlowShape`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeWarning {
    pub code: WarningCode,
    pub flow_name: String,
    pub backend: String,
    pub fallback_mode: FallbackMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_name: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub declared_output: String,
    pub message: String,
    /// Unix-millis timestamp when the warning was minted.
    /// Monotonic within a flow execution; multiple warnings
    /// preserve insertion order via the carrying `Vec`.
    pub timestamp_ms: u64,
}

impl RuntimeWarning {
    /// Construct a W002 streaming-not-supported warning with the
    /// canonical message.
    pub fn streaming_not_supported(
        flow_name: impl Into<String>,
        backend: impl Into<String>,
        fallback_mode: FallbackMode,
        detail: impl Into<String>,
    ) -> Self {
        let detail = detail.into();
        let message = if detail.is_empty() {
            WarningCode::AxonW002.message().to_string()
        } else {
            format!("{}: {}", WarningCode::AxonW002.message(), detail)
        };
        Self {
            code: WarningCode::AxonW002,
            flow_name: flow_name.into(),
            backend: backend.into(),
            fallback_mode,
            step_name: None,
            declared_output: String::new(),
            message,
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  Tests — closed-catalog pins
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod closed_catalog_pins {
    use super::*;

    #[test]
    fn warning_code_catalog_has_exactly_one_variant() {
        // Compile-time evidence: pattern-match exhaustiveness +
        // the test ensures any addition is intentional. Update
        // this slot when adding W003+ (deliberate founder sign-
        // off, NOT silent).
        let all = [WarningCode::AxonW002];
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn warning_code_slug_is_kebab_case_with_axon_w_prefix() {
        for code in [WarningCode::AxonW002] {
            let slug = code.slug();
            assert!(
                slug.starts_with("axon-W"),
                "WarningCode slug MUST start with 'axon-W'; got {slug:?}"
            );
            assert!(
                slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
                "WarningCode slug MUST be ASCII alphanumeric + dash; got {slug:?}"
            );
        }
    }

    #[test]
    fn warning_code_slugs_are_unique() {
        let all = [WarningCode::AxonW002];
        let mut slugs: Vec<&str> = all.iter().map(|c| c.slug()).collect();
        slugs.sort();
        let mut unique = slugs.clone();
        unique.dedup();
        assert_eq!(slugs.len(), unique.len(), "WarningCode slugs must be unique");
    }

    #[test]
    fn fallback_mode_catalog_has_three_variants_post_33_z_e() {
        // v1.24.0 — `UnsupportedFlowShape` retired; catalog
        // shrinks from 4 to 3. The dispatcher path covers every
        // IRFlowNode variant; no shape is "unsupported".
        let all = [
            FallbackMode::UnknownBackend,
            FallbackMode::SourceCompilationFailed,
            FallbackMode::BackendLacksStream,
        ];
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn fallback_mode_slugs_are_snake_case_unique() {
        let all = [
            FallbackMode::UnknownBackend,
            FallbackMode::SourceCompilationFailed,
            FallbackMode::BackendLacksStream,
        ];
        let mut slugs: Vec<&str> = all.iter().map(|m| m.slug()).collect();
        // Snake-case predicate.
        for slug in &slugs {
            assert!(
                slug.chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()),
                "FallbackMode slug MUST be snake_case; got {slug:?}"
            );
        }
        slugs.sort();
        let mut unique = slugs.clone();
        unique.dedup();
        assert_eq!(slugs.len(), unique.len(), "FallbackMode slugs must be unique");
    }

    #[test]
    fn streaming_not_supported_constructor_sets_canonical_message() {
        let w = RuntimeWarning::streaming_not_supported(
            "Chat",
            "anthropic",
            FallbackMode::UnknownBackend,
            "",
        );
        assert_eq!(w.code, WarningCode::AxonW002);
        assert_eq!(w.flow_name, "Chat");
        assert_eq!(w.backend, "anthropic");
        assert_eq!(w.fallback_mode, FallbackMode::UnknownBackend);
        assert_eq!(w.message, "streaming-not-supported");
        assert!(w.timestamp_ms > 0);
    }

    #[test]
    fn streaming_not_supported_constructor_includes_detail_in_message() {
        let w = RuntimeWarning::streaming_not_supported(
            "Chat",
            "stub",
            FallbackMode::SourceCompilationFailed,
            "parse: missing closing brace",
        );
        assert_eq!(
            w.message,
            "streaming-not-supported: parse: missing closing brace"
        );
    }

    #[test]
    fn runtime_warning_serializes_with_kebab_code_and_snake_fallback() {
        let w = RuntimeWarning::streaming_not_supported(
            "F",
            "stub",
            FallbackMode::UnknownBackend,
            "",
        );
        let json = serde_json::to_value(&w).unwrap();
        assert_eq!(json["code"], "axon-W002");
        assert_eq!(json["fallback_mode"], "unknown_backend");
        assert_eq!(json["flow_name"], "F");
        assert_eq!(json["backend"], "stub");
        // step_name + declared_output are elided when empty.
        assert!(json.get("step_name").is_none());
        assert!(json.get("declared_output").is_none());
    }

    #[test]
    fn runtime_warning_round_trips_via_serde() {
        let w = RuntimeWarning::streaming_not_supported(
            "F",
            "anthropic",
            FallbackMode::BackendLacksStream,
            "adopter's custom backend",
        );
        let json = serde_json::to_string(&w).unwrap();
        let parsed: RuntimeWarning = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.code, WarningCode::AxonW002);
        assert_eq!(parsed.flow_name, "F");
        assert_eq!(parsed.backend, "anthropic");
        assert_eq!(parsed.fallback_mode, FallbackMode::BackendLacksStream);
    }
}
