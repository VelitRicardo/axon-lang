//! v2.77.0 — the TikTok native core: READ/ANALYTICS-FIRST.
//!
//! TikTok has the hardest zero-input regime of the four platforms (paper section 2.4):
//! unaudited clients are SELF_ONLY, public posting requires an audit, and the
//! guidelines require EXPRESS PER-POST USER CONSENT before content is even
//! transmitted — so fully unattended public posting is not permitted at the
//! POLICY level, regardless of technical capability. This connector therefore
//! **does not publish**: [`SocialConnector::publish`] returns the crate's own
//! posture refusal ([`crate::posture`]), the single source of truth for the
//! `axon-T958` law. Reads (comments, video metrics) are real; the other write
//! ops are honestly [`ConnectorError::Unsupported`] under the read-first surface.
//!
//! **Transport is neither Graph nor LinkedIn.** TikTok's Open API returns a
//! nested envelope `{"data":…,"error":{"code":"ok"|…,"message":…,"log_id":…}}`
//! where a 200 with `error.code != "ok"` is STILL a failure — the connector
//! checks the envelope, not just the HTTP status. Tokens are the rotating kind
//! ([`crate::oauth::RefreshMechanism::RotatingRefreshGrant`]); the v2.77.0 engine
//! keeps them alive.

use std::time::Duration;

use crate::connector::{
    CallContext, Comment, ConnectorError, Metrics, ModerationAction, PublishReceipt,
    PublishRequest, Reaction, SocialConnector,
};
use crate::platform::{Operation, Platform};
use crate::posture::{posture_check, AppAudit, Attendance, TargetKind};

pub const DEFAULT_API_BASE: &str = "https://open.tiktokapis.com";

/// Configuration for a [`TikTokConnector`].
#[derive(Clone)]
pub struct TikTokConfig {
    pub access_token: Option<String>,
    pub base_url: String,
    pub timeout: Duration,
}

impl TikTokConfig {
    pub fn new() -> TikTokConfig {
        TikTokConfig {
            access_token: None,
            base_url: DEFAULT_API_BASE.to_string(),
            timeout: Duration::from_secs(30),
        }
    }
}

impl Default for TikTokConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for TikTokConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TikTokConfig")
            .field("access_token", &self.access_token.as_ref().map(|_| "<redacted>"))
            .field("base_url", &self.base_url)
            .finish()
    }
}

/// The TikTok connector core (v2.77.0), read/analytics-first.
pub struct TikTokConnector {
    config: TikTokConfig,
    client: reqwest::blocking::Client,
}

impl TikTokConnector {
    pub fn new(config: TikTokConfig) -> Result<TikTokConnector, ConnectorError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| ConnectorError::Transport(format!("client build: {e}")))?;
        Ok(TikTokConnector { config, client })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.config.base_url.trim_end_matches('/'), path.trim_start_matches('/'))
    }

    fn token<'a>(&'a self, ctx: &'a CallContext) -> Result<&'a str, ConnectorError> {
        ctx.secret
            .as_deref()
            .or(self.config.access_token.as_deref())
            .ok_or(ConnectorError::MissingCredential { platform: Platform::TikTok })
    }

    /// Send + check BOTH the HTTP status and the nested `error.code` (a 200 with
    /// `error.code != "ok"` is still a failure — the TikTok envelope discipline).
    fn execute(
        &self,
        req: reqwest::blocking::RequestBuilder,
        token: &str,
    ) -> Result<serde_json::Value, ConnectorError> {
        let resp = req
            .bearer_auth(token)
            .send()
            .map_err(|e| ConnectorError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let body: serde_json::Value = resp
            .json()
            .map_err(|e| ConnectorError::Transport(format!("non-JSON response: {e}")))?;
        let err_code = body
            .pointer("/error/code")
            .and_then(|v| v.as_str())
            .unwrap_or("ok");
        if status >= 400 || (err_code != "ok" && !err_code.is_empty()) {
            let message = body
                .pointer("/error/message")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            return Err(ConnectorError::Platform {
                status: if status >= 400 { status } else { 400 },
                message: format!("{err_code}: {message}"),
            });
        }
        Ok(body)
    }
}

fn str_at(v: &serde_json::Value, pointer: &str) -> String {
    v.pointer(pointer).and_then(|x| x.as_str()).unwrap_or_default().to_string()
}

fn u64_at(v: &serde_json::Value, pointer: &str) -> u64 {
    v.pointer(pointer).and_then(|x| x.as_u64()).unwrap_or(0)
}

impl SocialConnector for TikTokConnector {
    fn platform(&self) -> Platform {
        Platform::TikTok
    }

    fn name(&self) -> &'static str {
        "tiktok-open"
    }

    /// `POST /v2/video/comment/list/` — comments on a video (born Untrusted).
    fn read_comments(
        &self,
        ctx: &CallContext,
        target: &str,
    ) -> Result<Vec<Comment>, ConnectorError> {
        let token = self.token(ctx)?;
        let body = self.execute(
            self.client
                .post(self.url("v2/video/comment/list/"))
                .query(&[("fields", "id,text,username")])
                .json(&serde_json::json!({ "video_id": target, "max_count": 50 })),
            token,
        )?;
        let comments = body
            .pointer("/data/comments")
            .and_then(|v| v.as_array())
            .map(|rows| {
                rows.iter()
                    .map(|r| Comment {
                        id: str_at(r, "/id"),
                        author: str_at(r, "/username"),
                        text: str_at(r, "/text"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(comments)
    }

    /// TikTok exposes like counts, not typed reactions: the video's `like_count`.
    fn read_reactions(
        &self,
        ctx: &CallContext,
        target: &str,
    ) -> Result<Vec<Reaction>, ConnectorError> {
        let m = self.read_metrics(ctx, target)?;
        Ok(vec![Reaction { kind: "like".to_string(), count: m.engagements }])
    }

    /// `POST /v2/video/query/` — public video stats mapped onto [`Metrics`]
    /// (video-level: no follower count — that field is account-level, left 0).
    fn read_metrics(&self, ctx: &CallContext, target: &str) -> Result<Metrics, ConnectorError> {
        let token = self.token(ctx)?;
        let body = self.execute(
            self.client
                .post(self.url("v2/video/query/"))
                .query(&[("fields", "id,view_count,like_count,comment_count,share_count")])
                .json(&serde_json::json!({ "filters": { "video_ids": [target] } })),
            token,
        )?;
        let v = body.pointer("/data/videos/0").cloned().unwrap_or(serde_json::Value::Null);
        Ok(Metrics {
            impressions: u64_at(&v, "/view_count"),
            engagements: u64_at(&v, "/like_count")
                + u64_at(&v, "/comment_count")
                + u64_at(&v, "/share_count"),
            followers: 0,
        })
    }

    fn reply(
        &self,
        _ctx: &CallContext,
        _comment_id: &str,
        _text: &str,
    ) -> Result<PublishReceipt, ConnectorError> {
        Err(ConnectorError::Unsupported {
            platform: Platform::TikTok,
            reason: "TikTok is read/analytics-first — commenting is not on the surface"
                .to_string(),
        })
    }

    fn moderate(
        &self,
        _ctx: &CallContext,
        _comment_id: &str,
        _action: ModerationAction,
    ) -> Result<(), ConnectorError> {
        Err(ConnectorError::Unsupported {
            platform: Platform::TikTok,
            reason: "TikTok is read/analytics-first — moderation is not on the surface"
                .to_string(),
        })
    }

    /// **Publishing is REFUSED.** TikTok requires express per-post user consent
    /// before content is transmitted; fully unattended public posting is not
    /// permitted (paper section 2.4). The refusal is the crate's own `posture_check`
    /// — the single source of truth for `axon-T958`.
    fn publish(
        &self,
        _ctx: &CallContext,
        _req: &PublishRequest,
    ) -> Result<PublishReceipt, ConnectorError> {
        // The connector runs unattended by nature; even an audited app cannot
        // publish publicly without per-post consent.
        match posture_check(
            Platform::TikTok,
            Operation::Publish,
            TargetKind::OwnedProfessionalAccount,
            AppAudit::Audited,
            Attendance::Unattended,
        ) {
            Err(refusal) => Err(ConnectorError::Refused(refusal)),
            // Unreachable: TikTok + Publish + Unattended always refuses. If the
            // posture ever permitted it, the connector still has no publish path.
            Ok(()) => Err(ConnectorError::Unsupported {
                platform: Platform::TikTok,
                reason: "TikTok publishing arrives only with the per-post consent machinery"
                    .to_string(),
            }),
        }
    }

    fn edit(
        &self,
        _ctx: &CallContext,
        _object_id: &str,
        _req: &PublishRequest,
    ) -> Result<PublishReceipt, ConnectorError> {
        Err(ConnectorError::Unsupported {
            platform: Platform::TikTok,
            reason: "TikTok is read/analytics-first".to_string(),
        })
    }

    fn delete(&self, _ctx: &CallContext, _object_id: &str) -> Result<(), ConnectorError> {
        Err(ConnectorError::Unsupported {
            platform: Platform::TikTok,
            reason: "TikTok is read/analytics-first".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connector(base: &str, token: Option<&str>) -> TikTokConnector {
        let mut cfg = TikTokConfig::new();
        cfg.base_url = base.to_string();
        cfg.access_token = token.map(str::to_string);
        TikTokConnector::new(cfg).unwrap()
    }

    #[test]
    fn publish_is_refused_with_the_consent_posture() {
        let c = connector("http://127.0.0.1:9", Some("t"));
        match c.publish(&CallContext::none(), &PublishRequest { body: "x".into(), media_urls: vec![] }) {
            Err(ConnectorError::Refused(r)) => {
                assert_eq!(r.code, "axon-T958");
                assert!(r.reason.contains("consent"), "the consent blocker: {}", r.reason);
            }
            other => panic!("TikTok publish must be Refused, got: {other:?}"),
        }
    }

    #[test]
    fn write_ops_are_read_first_unsupported() {
        let c = connector("http://127.0.0.1:9", Some("t"));
        let ctx = CallContext { secret: Some("t".into()) };
        assert!(matches!(c.reply(&ctx, "c", "t"), Err(ConnectorError::Unsupported { .. })));
        assert!(matches!(
            c.moderate(&ctx, "c", ModerationAction::Delete),
            Err(ConnectorError::Unsupported { .. })
        ));
        assert!(matches!(c.delete(&ctx, "v"), Err(ConnectorError::Unsupported { .. })));
    }

    #[test]
    fn no_credential_fails_closed() {
        let c = connector("http://127.0.0.1:9", None);
        assert!(matches!(
            c.read_metrics(&CallContext::none(), "v1"),
            Err(ConnectorError::MissingCredential { .. })
        ));
    }

    #[test]
    fn config_debug_redacts_the_token() {
        let mut cfg = TikTokConfig::new();
        cfg.access_token = Some("tok-SENSITIVE".into());
        assert!(!format!("{cfg:?}").contains("tok-SENSITIVE"));
    }
}
