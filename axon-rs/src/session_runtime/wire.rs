//! Wire format for the v2.3.0 session-typed WebSocket dialogue.
//!
//! Every frame is one **text** WebSocket message carrying a JSON envelope
//! whose `kind` discriminator names one of the five operational actions of
//! the v2.3.0 algebra: `send`, `recv` (reserved — see note), `select`,
//! `branch`, `end`, plus an out-of-band `error` carrier for protocol-error
//! close-frame reasons. The format is **closed** — anything else is a
//! `MalformedFrame` (no silent toleration; the type checker has already
//! ruled out the schema, so any new shape on the wire is by definition
//! out-of-spec).
//!
//! ### Why a single direction word
//!
//! `send` and `recv` are *peer-relative*: the **sender** always tags its
//! frame `kind: "send"`. From the **receiver's** perspective the frame is
//! a `recv` step in *its* type, but the wire tag is symmetric — both peers
//! agree on the **sender's** view. This matches RFC 6455 message direction
//! and removes ambiguity when both peers share log infrastructure.
//!
//! `select` is *senderly* (the chooser); `branch` is *receiverly* (the
//! offerer). At the wire level the chooser emits `kind: "select"`; the
//! offerer never emits a label-bearing frame (its arms are silent).
//!
//! All frames are validated by the receiver against the session-type
//! cursor; the wire format is intentionally minimal — it carries just
//! enough to advance the state machine, not the full schema (which lives
//! statically in `axon-frontend::session`).

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use super::error::ProtocolError;

/// Stable protocol-version tag — bumped if and only if the envelope shape
/// changes incompatibly. Senders MUST emit, receivers MUST validate.
pub const AXON_WIRE_VERSION: u8 = 1;

/// One frame on the wire — exactly one operational step.
///
/// `#[serde(tag = "kind", rename_all = "lowercase")]` is intentional: the
/// `kind` discriminator is the closed catalog `send | select | end |
/// error`. Anything else is malformed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Frame {
    /// `!A.S` advance: the sender produces a value of type `A` (named by
    /// the canonical payload string; the JSON `data` carries the value).
    /// On the receiver side this triggers a `try_recv(A)` on its cursor.
    Send {
        /// Canonical payload type name — must equal the cursor's
        /// `Payload` for the step. Validated at the receiver.
        #[serde(rename = "payload_type")]
        payload_type: String,
        /// The value carried by the step. Opaque at this layer (no
        /// schema enforcement beyond presence + JSON well-formedness);
        /// payload-shape validation is a future cycle (e.g. typed-data
        /// integration with `axonstore`).
        data: JsonValue,
    },
    /// `⊕{ℓᵢ:Sᵢ}` advance: the chooser names a labelled branch. On the
    /// receiver side this triggers a `try_offer(ℓ)` on its cursor (whose
    /// type at this point is `&{ℓᵢ:Sᵢ}`).
    Select {
        /// The chosen label — must be a key in the cursor's arms.
        label: String,
    },
    /// `end` — the dialogue terminates. Both halves transition to `End`.
    /// No further frames are accepted from either side; the carrier
    /// (WebSocket) closes cleanly with code `1000 normal closure`.
    End,
    /// Out-of-band protocol error — emitted by either side just before
    /// closing the carrier with code `1002 protocol error`. Carries a
    /// short machine-readable code + a human detail message so the peer
    /// can diagnose the divergence without re-running its analysis.
    Error {
        /// Short stable identifier — see [`ProtocolError::code`].
        code: String,
        /// Free-form human-readable detail.
        detail: String,
    },
}

impl Frame {
    /// The fixed runtime tag of this frame's kind, used in
    /// [`ProtocolError::UnexpectedFrame`] for cursor / frame mismatch
    /// diagnostics. Stable in both the wire (tag value) and the runtime
    /// (matching the closed catalog).
    pub fn kind_tag(&self) -> &'static str {
        match self {
            Frame::Send { .. } => "send",
            Frame::Select { .. } => "select",
            Frame::End => "end",
            Frame::Error { .. } => "error",
        }
    }

    /// Serialise to a JSON string, prefixed with the wire-version tag in
    /// the outer envelope: every frame on the wire is one line of
    /// `{"v":1,"kind":"…",…}` — the version is the **first** key so a
    /// linewise log scan can reject pre-handshake junk without parsing.
    ///
    /// We splice the version into the head of the serialised inner object
    /// rather than building a `serde_json::Map`: the default `Map` sorts
    /// keys alphabetically (BTreeMap-backed without `preserve_order`),
    /// which would land `"kind"` before `"v"`. The splice is total — the
    /// inner serialisation is always a JSON object for this enum (every
    /// variant has a `kind` tag), so the leading `{` is guaranteed.
    pub fn to_wire(&self) -> String {
        let inner = serde_json::to_string(self).expect("Frame ⇒ JSON is total");
        debug_assert!(inner.starts_with('{') && inner.ends_with('}'));
        if inner == "{}" {
            // Defensive — no Frame variant produces this, but be total.
            return format!("{{\"v\":{AXON_WIRE_VERSION}}}");
        }
        // `{"kind":"…",…}` → `{"v":1,"kind":"…",…}`
        format!("{{\"v\":{AXON_WIRE_VERSION},{}", &inner[1..])
    }

    /// Parse a wire string into a [`Frame`]. Validates the version tag and
    /// the closed `kind` catalog; returns [`ProtocolError::MalformedFrame`]
    /// on any divergence (including pre-1.0 shapes, unknown `kind`, missing
    /// required fields).
    pub fn from_wire(s: &str) -> Result<Frame, ProtocolError> {
        let raw: JsonValue = serde_json::from_str(s)
            .map_err(|e| ProtocolError::MalformedFrame(format!("invalid JSON: {e}")))?;
        let obj = raw
            .as_object()
            .ok_or_else(|| ProtocolError::MalformedFrame("envelope is not a JSON object".into()))?;
        let v = obj
            .get("v")
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| ProtocolError::MalformedFrame("missing wire-version `v`".into()))?;
        if v != AXON_WIRE_VERSION as u64 {
            return Err(ProtocolError::MalformedFrame(format!(
                "unsupported wire version {v} (this runtime speaks v{AXON_WIRE_VERSION})"
            )));
        }
        // `serde(tag = "kind")` does the rest — we strip the version so the
        // deserialiser sees the inner shape exactly.
        let mut inner = obj.clone();
        inner.remove("v");
        serde_json::from_value::<Frame>(JsonValue::Object(inner)).map_err(|e| {
            ProtocolError::MalformedFrame(format!(
                "unknown frame kind or missing field: {e}"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn send_frame_round_trips_through_wire() {
        let f = Frame::Send {
            payload_type: "Msg".into(),
            data: json!({"text": "hello"}),
        };
        let wire = f.to_wire();
        // Version is the first key (deterministic order).
        assert!(wire.starts_with("{\"v\":1,"));
        let parsed = Frame::from_wire(&wire).expect("parse");
        assert_eq!(parsed, f);
    }

    #[test]
    fn select_frame_round_trips() {
        let f = Frame::Select { label: "ask".into() };
        assert_eq!(Frame::from_wire(&f.to_wire()).unwrap(), f);
    }

    #[test]
    fn end_frame_is_a_bare_kind_marker() {
        let wire = Frame::End.to_wire();
        assert_eq!(wire, "{\"v\":1,\"kind\":\"end\"}");
        assert_eq!(Frame::from_wire(&wire).unwrap(), Frame::End);
    }

    #[test]
    fn error_frame_round_trips_with_code_and_detail() {
        let f = Frame::Error {
            code: "credit_exhausted".into(),
            detail: "n=0 on send Msg".into(),
        };
        assert_eq!(Frame::from_wire(&f.to_wire()).unwrap(), f);
    }

    #[test]
    fn missing_version_is_malformed() {
        let s = r#"{"kind":"end"}"#;
        match Frame::from_wire(s) {
            Err(ProtocolError::MalformedFrame(m)) => assert!(m.contains("wire-version")),
            other => panic!("expected MalformedFrame, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_version_is_malformed() {
        let s = r#"{"v":99,"kind":"end"}"#;
        match Frame::from_wire(s) {
            Err(ProtocolError::MalformedFrame(m)) => assert!(m.contains("unsupported wire version")),
            other => panic!("expected version-mismatch malformed, got {other:?}"),
        }
    }

    #[test]
    fn unknown_kind_is_malformed() {
        let s = r#"{"v":1,"kind":"yeet","payload_type":"X"}"#;
        assert!(matches!(Frame::from_wire(s), Err(ProtocolError::MalformedFrame(_))));
    }

    #[test]
    fn invalid_json_is_malformed() {
        let s = r#"{"v":1,"kind":"send""#; // truncated
        assert!(matches!(Frame::from_wire(s), Err(ProtocolError::MalformedFrame(_))));
    }

    #[test]
    fn non_object_envelope_is_malformed() {
        let s = "[1,2,3]";
        assert!(matches!(Frame::from_wire(s), Err(ProtocolError::MalformedFrame(_))));
    }

    #[test]
    fn kind_tag_matches_wire_tag() {
        let cases = [
            (Frame::Send { payload_type: "T".into(), data: json!(null) }, "send"),
            (Frame::Select { label: "a".into() }, "select"),
            (Frame::End, "end"),
            (Frame::Error { code: "c".into(), detail: "d".into() }, "error"),
        ];
        for (f, tag) in cases {
            assert_eq!(f.kind_tag(), tag);
        }
    }
}
