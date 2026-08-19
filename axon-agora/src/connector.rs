//! The uniform connector seam.
//!
//! Each platform's native core implements [`SocialConnector`]; the runtime reaches it through
//! the `agora_*` provider dispatch arm (`axon-rs::agora_runtime`), inside `spawn_blocking` —
//! so the trait is **synchronous and dyn-safe**, exactly the `ScrapeFetcher` (§98.g) /
//! `EnrichmentProvider` (§104.a) injection shape. A connector op therefore runs in production
//! through `execute_server_flow` with the governance a `tool` already carries: secret injection
//! without revelation (§94.c), the linear budget (§72), and lease + capacity (§114). D116.6:
//! the trait is uniform, so new platforms are additive without touching the surface.

use crate::platform::Platform;
use crate::posture::PostureRefusal;
use crate::protocol::ProtocolViolation;

/// A comment read from a platform. Born `Untrusted` at the boundary (§98/T908): its text is
/// attacker-controlled and must not launder into a trusted instruction.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Comment {
    pub id: String,
    pub author: String,
    pub text: String,
}

/// A reaction (like, celebrate, …) read from a platform.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Reaction {
    pub kind: String,
    pub count: u64,
}

/// Engagement metrics for an owned asset.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Metrics {
    pub impressions: u64,
    pub engagements: u64,
    pub followers: u64,
}

/// A request to publish content to an owned asset.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PublishRequest {
    pub body: String,
    #[serde(default)]
    pub media_urls: Vec<String>,
}

/// The receipt of a governed egress write (§105/§114): the platform's id for the created object
/// and its public URL if the platform returns one.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PublishReceipt {
    pub object_id: String,
    #[serde(default)]
    pub url: Option<String>,
}

/// How to moderate a comment on owned content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModerationAction {
    Hide,
    Delete,
}

impl ModerationAction {
    /// Parse the wire form (`"hide"` / `"delete"`).
    pub fn parse(s: &str) -> Option<ModerationAction> {
        match s {
            "hide" => Some(ModerationAction::Hide),
            "delete" => Some(ModerationAction::Delete),
            _ => None,
        }
    }
}

/// A connector failure. Governance refusals ([`ConnectorError::Refused`],
/// [`ConnectorError::QuotaExhausted`], [`ConnectorError::Protocol`],
/// [`ConnectorError::Unsupported`]) are distinct from platform and transport errors so the
/// runtime can route them differently (a refusal is never retried; a refusal is never
/// fabricated around).
#[derive(Debug, Clone)]
pub enum ConnectorError {
    /// The platform rejected the request.
    Platform { status: u16, message: String },
    /// The operation is refused by the owned-only posture (axon-T958).
    Refused(PostureRefusal),
    /// The platform's official API has no such operation (e.g. Instagram media
    /// deletion) — honestly unsupported, never emulated.
    Unsupported { platform: Platform, reason: String },
    /// No credential is available for this call — neither the §94.c custody
    /// injection (`CallContext.secret`) nor the connector's configured token.
    /// Fail-closed: never an unauthenticated vendor call (the lambda_tools
    /// posture).
    MissingCredential { platform: Platform },
    /// The publish quota is exhausted (§72 / axon-W018).
    QuotaExhausted,
    /// A protocol-order violation (axon-T957).
    Protocol(ProtocolViolation),
    /// Transport / IO failure.
    Transport(String),
}

impl std::fmt::Display for ConnectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectorError::Platform { status, message } => {
                write!(f, "platform rejected the request ({status}): {message}")
            }
            ConnectorError::Refused(r) => {
                write!(f, "{} {} Fix: {} [{}]", r.code, r.reason, r.fix, r.source)
            }
            ConnectorError::Unsupported { platform, reason } => write!(
                f,
                "the {} official API has no such operation: {reason} — axon-agora never \
                 emulates a missing platform capability",
                platform.as_str()
            ),
            ConnectorError::MissingCredential { platform } => write!(
                f,
                "no access token available for {} — neither custody injection (axon_secret, \
                 secret custody) nor connector config supplies one; the call fails closed (never an \
                 unauthenticated vendor call)",
                platform.as_str()
            ),
            ConnectorError::QuotaExhausted => {
                write!(f, "publish quota exhausted (the budget is spent for this window)")
            }
            ConnectorError::Protocol(v) => write!(
                f,
                "protocol violation on {} at step {}: expected '{}', got '{}' (axon-T957)",
                v.platform.as_str(),
                v.position,
                v.expected,
                v.got
            ),
            ConnectorError::Transport(e) => write!(f, "transport failure: {e}"),
        }
    }
}

/// Per-call context (§116.c). The credential is PER-CALL, not per-connector-instance:
/// in the enterprise runtime one registered connector serves many tenants, and the §94.c
/// custody injection resolves the per-tenant token at dispatch (`axon_secret`), which the
/// `agora_runtime` dispatch strips out of the body and hands here — the value never rides
/// a vendor payload, a log line, or the flow's bindings.
#[derive(Clone, Default)]
pub struct CallContext {
    /// The custody-revealed credential for THIS call, if the tool declared `secret:`
    /// (§94.c/§116.b). `None` in a dev runtime with no custody — the connector then falls
    /// back to its own configured token, or fails closed. Redacted from `Debug` output.
    pub secret: Option<String>,
}

// The §94 redacting-Debug discipline (the `RevealedSecret` shape): a custody value
// must never reach a log line through a stray `{:?}`.
impl std::fmt::Debug for CallContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallContext")
            .field("secret", &self.secret.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl CallContext {
    /// A context with no custody-revealed credential (dev/OSS default).
    pub fn none() -> CallContext {
        CallContext { secret: None }
    }
}

/// The uniform surface every platform connector implements. Read operations return data born
/// `Untrusted`; write operations are governed egress. Synchronous by design — the runtime wraps
/// dispatch in `spawn_blocking` (the Brief #63 isolation discipline), and a sync trait is
/// dyn-safe for the `Arc<dyn SocialConnector>` registry.
///
/// Implementors live in the native cores (§116.c–f) and register via
/// `axon-rs::agora_runtime::register_agora_connector`. An op the platform's official API does
/// not offer MUST return [`ConnectorError::Unsupported`] — never an emulation.
pub trait SocialConnector: Send + Sync {
    /// Which platform this connector serves.
    fn platform(&self) -> Platform;

    /// A short engine slug for provenance + audit (e.g. `"facebook-graph"`).
    fn name(&self) -> &'static str;

    /// Read comments on an owned asset. Results are born `Untrusted`.
    fn read_comments(&self, ctx: &CallContext, target: &str)
        -> Result<Vec<Comment>, ConnectorError>;

    /// Read reactions on an owned asset. Results are born `Untrusted`.
    fn read_reactions(
        &self,
        ctx: &CallContext,
        target: &str,
    ) -> Result<Vec<Reaction>, ConnectorError>;

    /// Read engagement metrics for an owned asset.
    fn read_metrics(&self, ctx: &CallContext, target: &str) -> Result<Metrics, ConnectorError>;

    /// Reply to a comment (governed egress).
    fn reply(
        &self,
        ctx: &CallContext,
        comment_id: &str,
        text: &str,
    ) -> Result<PublishReceipt, ConnectorError>;

    /// Moderate a comment on owned content (governed egress).
    fn moderate(
        &self,
        ctx: &CallContext,
        comment_id: &str,
        action: ModerationAction,
    ) -> Result<(), ConnectorError>;

    /// Publish content to an owned asset (governed egress; quota-metered; protocol-driven).
    fn publish(&self, ctx: &CallContext, req: &PublishRequest)
        -> Result<PublishReceipt, ConnectorError>;

    /// Edit previously published content, where the platform's official API supports it.
    fn edit(
        &self,
        ctx: &CallContext,
        object_id: &str,
        req: &PublishRequest,
    ) -> Result<PublishReceipt, ConnectorError>;

    /// Delete previously published content (governed egress).
    fn delete(&self, ctx: &CallContext, object_id: &str) -> Result<(), ConnectorError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moderation_action_parses_the_wire_forms_only() {
        assert_eq!(ModerationAction::parse("hide"), Some(ModerationAction::Hide));
        assert_eq!(ModerationAction::parse("delete"), Some(ModerationAction::Delete));
        assert_eq!(ModerationAction::parse("ban"), None);
        assert_eq!(ModerationAction::parse(""), None);
    }

    #[test]
    fn wire_types_roundtrip_through_json() {
        let c = Comment { id: "1".into(), author: "a".into(), text: "t".into() };
        let json = serde_json::to_string(&c).unwrap();
        let back: Comment = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "1");

        let r: PublishRequest = serde_json::from_str(r#"{"body":"hello"}"#).unwrap();
        assert_eq!(r.body, "hello");
        assert!(r.media_urls.is_empty()); // #[serde(default)]
    }

    #[test]
    fn errors_display_with_their_governance_identity() {
        let e = ConnectorError::Refused(crate::posture::PostureRefusal {
            code: "axon-T958",
            reason: "reason.",
            fix: "fix.",
            source: "src",
        });
        assert!(e.to_string().contains("axon-T958"));
        let u = ConnectorError::Unsupported {
            platform: Platform::Instagram,
            reason: "media deletion".into(),
        };
        assert!(u.to_string().contains("never"));
    }
}
