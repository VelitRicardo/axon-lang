//! `Stream<T>` — temporal algebraic effect with mandatory backpressure.
//!
//! v1.4.0 — every `Stream<T>` declaration MUST carry a
//! [`BackpressurePolicy`] annotation. The checker rejects any flow
//! that declares a stream-valued parameter or return without one.
//! The rationale is operational: a reactive stream without an
//! explicit contrapressure strategy silently drops or fails under
//! load, and that's exactly the class of incident the type system
//! exists to prevent.
//!
//! The catalogue of policies is **closed** at the compiler level.
//! Adding a new one requires a compiler patch — we don't want an
//! adopter to invent "retry_forever" and starve the rest of the
//! runtime. Custom *composition* of the four primitives is fine and
//! lives outside this module (runtime combinators).

use std::fmt;

// ── Closed catalogue of backpressure policies ────────────────────────

/// The four strategies the runtime knows how to execute when a
/// producer outruns its consumer. See also [`crate::stream_runtime`]
/// for the runtime impls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackpressurePolicy {
    /// Drop the oldest item to make room for a fresh one. Used for
    /// "keep only the most recent" telemetry streams.
    DropOldest,
    /// Apply a pure degradation function (e.g. resample audio to a
    /// lower bitrate) so the downstream consumer still gets every
    /// frame, just lossy. The degrader is declared via
    /// `degrade_quality(resample_to=8000)` syntax.
    DegradeQuality,
    /// Block the producer until the buffer drains. Safe for
    /// request/response flows but MUST NOT be used on real-time
    /// ingest paths (microphones, market data) or the source hangs.
    PauseUpstream,
    /// Raise an error and cancel the stream. Forces callers to deal
    /// with saturation as an explicit failure mode. The default-is-
    /// no-default: a `Stream<T>` without a declared policy never
    /// falls back to `Fail`; it fails to compile.
    Fail,
}

impl BackpressurePolicy {
    /// Every variant. Explicit slice so adding a policy without
    /// updating consumers is a compile error.
    pub const ALL: &'static [BackpressurePolicy] = &[
        BackpressurePolicy::DropOldest,
        BackpressurePolicy::DegradeQuality,
        BackpressurePolicy::PauseUpstream,
        BackpressurePolicy::Fail,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            BackpressurePolicy::DropOldest => "drop_oldest",
            BackpressurePolicy::DegradeQuality => "degrade_quality",
            BackpressurePolicy::PauseUpstream => "pause_upstream",
            BackpressurePolicy::Fail => "fail",
        }
    }

    pub fn from_slug(slug: &str) -> Option<BackpressurePolicy> {
        Self::ALL.iter().copied().find(|p| p.slug() == slug)
    }
}

impl fmt::Display for BackpressurePolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

/// Catalogue lookup for checker diagnostics.
pub const BACKPRESSURE_CATALOG: &[&str] =
    &["drop_oldest", "degrade_quality", "pause_upstream", "fail"];

// ── Stream type constructor ──────────────────────────────────────────

/// Canonical name of the stream type constructor, as it appears in
/// source (`Stream<Bytes>`, `Stream<AudioFrame>`).
pub const STREAM_TYPE_CTOR: &str = "Stream";

/// True when the given type name denotes a stream.
pub fn is_stream_type(name: &str) -> bool {
    name == STREAM_TYPE_CTOR
}

// ── Annotation parsing ───────────────────────────────────────────────

/// A `@backpressure(policy, ...options)` annotation attached to the
/// flow or tool that owns the stream. `options` forwards to the
/// policy runtime (e.g. `buffer_size=128`, `degrade_quality(resample
/// _to=8000)` → `options=[(resample_to, 8000)]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackpressureAnnotation {
    pub policy: BackpressurePolicy,
    pub options: Vec<(String, String)>,
}

/// Parse a backpressure annotation body. Returns `None` if the policy
/// slug is unknown so the checker emits a targeted diagnostic.
pub fn parse_backpressure_annotation(body: &str) -> Option<BackpressureAnnotation> {
    let mut parts = body.split(',').map(|p| p.trim());
    let policy_slug = parts.next()?.trim();
    let policy = BackpressurePolicy::from_slug(policy_slug)?;

    let mut options = Vec::new();
    for raw in parts {
        if raw.is_empty() {
            continue;
        }
        let (k, v) = raw.split_once('=')?;
        options.push((k.trim().to_string(), v.trim().to_string()));
    }
    Some(BackpressureAnnotation { policy, options })
}

// ── Effect surface integration ───────────────────────────────────────

/// Effect slug surfaced in the existing `VALID_EFFECTS` catalogue.
/// A tool declaring `effects: [stream]` signals that it produces or
/// consumes a `Stream<T>` and therefore mandates a backpressure
/// handler on every flow that wires through it.
pub const STREAM_EFFECT_SLUG: &str = "stream";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_roundtrip_covers_closed_catalog() {
        for policy in BackpressurePolicy::ALL {
            let slug = policy.slug();
            assert_eq!(Some(*policy), BackpressurePolicy::from_slug(slug));
            assert!(BACKPRESSURE_CATALOG.contains(&slug));
        }
        assert_eq!(BackpressurePolicy::ALL.len(), BACKPRESSURE_CATALOG.len());
    }

    #[test]
    fn unknown_policy_slug_rejected() {
        assert!(BackpressurePolicy::from_slug("retry_forever").is_none());
        assert!(BackpressurePolicy::from_slug("").is_none());
    }

    #[test]
    fn stream_type_recognised() {
        assert!(is_stream_type("Stream"));
        assert!(!is_stream_type("stream")); // case-sensitive
        assert!(!is_stream_type("Iterator"));
    }

    #[test]
    fn parse_annotation_minimal() {
        let ann = parse_backpressure_annotation("drop_oldest").unwrap();
        assert_eq!(ann.policy, BackpressurePolicy::DropOldest);
        assert!(ann.options.is_empty());
    }

    #[test]
    fn parse_annotation_with_options() {
        let ann = parse_backpressure_annotation("degrade_quality, resample_to=8000, codec=mulaw")
            .unwrap();
        assert_eq!(ann.policy, BackpressurePolicy::DegradeQuality);
        assert_eq!(
            ann.options,
            vec![
                ("resample_to".to_string(), "8000".to_string()),
                ("codec".to_string(), "mulaw".to_string()),
            ]
        );
    }

    #[test]
    fn parse_annotation_rejects_malformed() {
        assert!(parse_backpressure_annotation("drop_oldest, no_equals").is_none());
        assert!(parse_backpressure_annotation("bogus_policy").is_none());
    }

    #[test]
    fn stream_effect_slug_is_stable() {
        assert_eq!(STREAM_EFFECT_SLUG, "stream");
    }
}
