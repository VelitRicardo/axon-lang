//! v2.77.0 — the Instagram native core: the session-typed publish protocol,
//! made HTTP-real.
//!
//! Owned-only posture: professional (business/creator) accounts only —
//! consumer publishing is refused at compile (`axon-T958`) and does not exist in
//! the official API (paper section 2.3). Reads (comments, insights) born Untrusted;
//! writes are governed egress.
//!
//! **Publishing is a mandatory typestate** (`axon-T957` made real): the connector
//! drives the whole `create container → poll status → publish` sequence the
//! platform requires (paper section 2.3) inside [`SocialConnector::publish`]. A container
//! that reaches `ERROR`/`EXPIRED`, or never reaches `FINISHED` within the poll
//! budget, is a typed failure — never a half-published post.
//!
//! **Quota is a consumable resource** (v2.28.0 / `axon-W018`): before publishing, the
//! connector reconciles with the platform's authoritative count via
//! `GET /{ig}/content_publishing_limit` and refuses ([`ConnectorError::
//! QuotaExhausted`]) at the 100/24h ceiling — the runtime half of the compile-time
//! budget.
//!
//! **Media deletion is NOT offered by the official API** — `delete` is honestly
//! [`ConnectorError::Unsupported`], never emulated. `edit` likewise (not
//! paper-verified). Carousels (multi-media) are deferred to v2.77.0.

use std::time::Duration;

use crate::connector::{
    CallContext, Comment, ConnectorError, Metrics, ModerationAction, PublishReceipt,
    PublishRequest, Reaction, SocialConnector,
};
use crate::graph;
use crate::platform::Platform;
use crate::quota;

/// Default Graph API base (Instagram rides the same Graph host as Pages).
pub const DEFAULT_GRAPH_BASE: &str = "https://graph.facebook.com";
/// Default Graph API version (pin per deployment — Meta sunsets on a cadence).
pub const DEFAULT_GRAPH_VERSION: &str = "v21.0";

/// Configuration for an [`InstagramConnector`].
#[derive(Clone)]
pub struct InstagramConfig {
    /// The owned Instagram professional account id (`/{ig_user_id}/media`).
    pub ig_user_id: String,
    /// Dev/OSS fallback token; production supplies the per-tenant token per-call
    /// via custody ([`CallContext::secret`]), which takes precedence.
    pub access_token: Option<String>,
    pub base_url: String,
    pub graph_version: String,
    pub timeout: Duration,
    /// Container-status poll budget (images finish near-instantly; video may
    /// take longer). Meta recommends ≤ once/minute for ≤ 5 minutes.
    pub poll_max_attempts: u32,
    pub poll_interval: Duration,
    /// Reconcile the 100/24h publish quota with the platform's authoritative
    /// `content_publishing_limit` before publishing (default true).
    pub reconcile_quota: bool,
}

impl InstagramConfig {
    pub fn new(ig_user_id: impl Into<String>) -> InstagramConfig {
        InstagramConfig {
            ig_user_id: ig_user_id.into(),
            access_token: None,
            base_url: DEFAULT_GRAPH_BASE.to_string(),
            graph_version: DEFAULT_GRAPH_VERSION.to_string(),
            timeout: Duration::from_secs(30),
            poll_max_attempts: 20,
            poll_interval: Duration::from_secs(2),
            reconcile_quota: true,
        }
    }
}

// The v2.48.0 redacting-Debug discipline: the fallback token never reaches a log.
impl std::fmt::Debug for InstagramConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstagramConfig")
            .field("ig_user_id", &self.ig_user_id)
            .field("access_token", &self.access_token.as_ref().map(|_| "<redacted>"))
            .field("base_url", &self.base_url)
            .field("graph_version", &self.graph_version)
            .finish()
    }
}

/// The Instagram connector core (v2.77.0).
pub struct InstagramConnector {
    config: InstagramConfig,
    client: reqwest::blocking::Client,
}

impl InstagramConnector {
    pub fn new(config: InstagramConfig) -> Result<InstagramConnector, ConnectorError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| ConnectorError::Transport(format!("client build: {e}")))?;
        Ok(InstagramConnector { config, client })
    }

    fn url(&self, path: &str) -> String {
        graph::url(&self.config.base_url, &self.config.graph_version, path)
    }

    fn token<'a>(&'a self, ctx: &'a CallContext) -> Result<&'a str, ConnectorError> {
        ctx.secret
            .as_deref()
            .or(self.config.access_token.as_deref())
            .ok_or(ConnectorError::MissingCredential { platform: Platform::Instagram })
    }

    /// Reconcile with the platform's authoritative publish count; refuse at the
    /// 100/24h ceiling (paper section 2.3). Skipped when `reconcile_quota` is false.
    fn check_quota(&self, token: &str) -> Result<(), ConnectorError> {
        if !self.config.reconcile_quota {
            return Ok(());
        }
        let ceiling = quota::publish_quota(Platform::Instagram).map(|q| q.limit).unwrap_or(100);
        let body = graph::execute(
            self.client
                .get(self.url(&format!("{}/content_publishing_limit", self.config.ig_user_id)))
                .query(&[("fields", "quota_usage")]),
            token,
        )?;
        let usage = body
            .pointer("/data/0/quota_usage")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if usage >= u64::from(ceiling) {
            return Err(ConnectorError::QuotaExhausted);
        }
        Ok(())
    }

    /// Poll the container's `status_code` until `FINISHED`; map `ERROR`/`EXPIRED`
    /// and a poll-budget timeout to typed failures (never a half-published post).
    fn poll_until_finished(
        &self,
        container_id: &str,
        token: &str,
    ) -> Result<(), ConnectorError> {
        for attempt in 0..self.config.poll_max_attempts {
            let body = graph::execute(
                self.client
                    .get(self.url(container_id))
                    .query(&[("fields", "status_code")]),
                token,
            )?;
            match graph::str_at(&body, "/status_code").as_str() {
                "FINISHED" | "PUBLISHED" => return Ok(()),
                "ERROR" => {
                    return Err(ConnectorError::Platform {
                        status: 422,
                        message: format!("Instagram media container {container_id} status ERROR"),
                    })
                }
                "EXPIRED" => {
                    return Err(ConnectorError::Platform {
                        status: 410,
                        message: format!(
                            "Instagram media container {container_id} EXPIRED (unpublished > 24h)"
                        ),
                    })
                }
                // IN_PROGRESS / anything else: wait and retry (unless last).
                _ => {
                    if attempt + 1 < self.config.poll_max_attempts {
                        std::thread::sleep(self.config.poll_interval);
                    }
                }
            }
        }
        Err(ConnectorError::Platform {
            status: 504,
            message: format!(
                "Instagram media container {container_id} did not reach FINISHED within {} polls",
                self.config.poll_max_attempts
            ),
        })
    }
}

impl SocialConnector for InstagramConnector {
    fn platform(&self) -> Platform {
        Platform::Instagram
    }

    fn name(&self) -> &'static str {
        "instagram-graph"
    }

    /// `GET /{media_id}/comments` — comments on a media object (born Untrusted).
    fn read_comments(
        &self,
        ctx: &CallContext,
        target: &str,
    ) -> Result<Vec<Comment>, ConnectorError> {
        let token = self.token(ctx)?;
        let body = graph::execute(
            self.client
                .get(self.url(&format!("{target}/comments")))
                .query(&[("fields", "id,username,text")]),
            token,
        )?;
        let comments = body
            .pointer("/data")
            .and_then(|v| v.as_array())
            .map(|rows| {
                rows.iter()
                    .map(|r| Comment {
                        id: graph::str_at(r, "/id"),
                        author: graph::str_at(r, "/username"),
                        text: graph::str_at(r, "/text"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(comments)
    }

    /// Instagram surfaces likes, not typed reactions: `like_count` on a media.
    fn read_reactions(
        &self,
        ctx: &CallContext,
        target: &str,
    ) -> Result<Vec<Reaction>, ConnectorError> {
        let token = self.token(ctx)?;
        let body = graph::execute(
            self.client.get(self.url(target)).query(&[("fields", "like_count")]),
            token,
        )?;
        let count = body.pointer("/like_count").and_then(|v| v.as_u64()).unwrap_or(0);
        Ok(vec![Reaction { kind: "like".to_string(), count }])
    }

    /// `GET /{ig_user_id}/insights` — account metrics mapped onto the uniform
    /// [`Metrics`] wire shape (the projection is lossy by design).
    fn read_metrics(&self, ctx: &CallContext, target: &str) -> Result<Metrics, ConnectorError> {
        let token = self.token(ctx)?;
        let body = graph::execute(
            self.client
                .get(self.url(&format!("{target}/insights")))
                .query(&[("metric", "impressions,reach,follower_count")]),
            token,
        )?;
        let metric = |name: &str| -> u64 {
            body.pointer("/data")
                .and_then(|v| v.as_array())
                .and_then(|rows| {
                    rows.iter()
                        .find(|r| r.pointer("/name").and_then(|v| v.as_str()) == Some(name))
                })
                .and_then(|r| r.pointer("/values/0/value"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
        };
        Ok(Metrics {
            impressions: metric("impressions"),
            engagements: metric("reach"),
            followers: metric("follower_count"),
        })
    }

    /// `POST /{comment_id}/replies` — reply to a comment.
    fn reply(
        &self,
        ctx: &CallContext,
        comment_id: &str,
        text: &str,
    ) -> Result<PublishReceipt, ConnectorError> {
        let token = self.token(ctx)?;
        let body = graph::execute(
            self.client
                .post(self.url(&format!("{comment_id}/replies")))
                .form(&[("message", text)]),
            token,
        )?;
        Ok(PublishReceipt { object_id: graph::str_at(&body, "/id"), url: None })
    }

    /// Hide: `POST /{comment_id}` with `hide=true`. Delete: `DELETE /{comment_id}`.
    fn moderate(
        &self,
        ctx: &CallContext,
        comment_id: &str,
        action: ModerationAction,
    ) -> Result<(), ConnectorError> {
        let token = self.token(ctx)?;
        match action {
            ModerationAction::Hide => {
                graph::execute(
                    self.client.post(self.url(comment_id)).form(&[("hide", "true")]),
                    token,
                )?;
            }
            ModerationAction::Delete => {
                graph::execute(self.client.delete(self.url(comment_id)), token)?;
            }
        }
        Ok(())
    }

    /// The container typestate (paper section 2.3): quota-reconcile → create container
    /// (`/{ig}/media` with `image_url` + `caption`) → poll status to `FINISHED`
    /// → publish (`/{ig}/media_publish` with `creation_id`). A single image; a
    /// media-less request or a carousel is honestly refused (never degraded).
    fn publish(
        &self,
        ctx: &CallContext,
        req: &PublishRequest,
    ) -> Result<PublishReceipt, ConnectorError> {
        let token = self.token(ctx)?;
        let ig = &self.config.ig_user_id;

        let image_url = match req.media_urls.len() {
            0 => {
                return Err(ConnectorError::Unsupported {
                    platform: Platform::Instagram,
                    reason: "Instagram posts require media — a text-only post has no API surface"
                        .to_string(),
                })
            }
            1 => req.media_urls[0].as_str(),
            n => {
                return Err(ConnectorError::Unsupported {
                    platform: Platform::Instagram,
                    reason: format!(
                        "carousel posts ({n} media) need the child-container + carousel flow — \
                         deferred, not silently degraded"
                    ),
                })
            }
        };

        // Runtime quota reconciliation (v2.28.0 / W018) — refuse at the ceiling.
        self.check_quota(token)?;

        // 1. Create the media container.
        let container = graph::execute(
            self.client
                .post(self.url(&format!("{ig}/media")))
                .form(&[("image_url", image_url), ("caption", req.body.as_str())]),
            token,
        )?;
        let container_id = graph::str_at(&container, "/id");
        if container_id.is_empty() {
            return Err(ConnectorError::Platform {
                status: 502,
                message: "Instagram /media returned no container id".to_string(),
            });
        }

        // 2. Poll the container until FINISHED (the typestate barrier).
        self.poll_until_finished(&container_id, token)?;

        // 3. Publish the finished container.
        let published = graph::execute(
            self.client
                .post(self.url(&format!("{ig}/media_publish")))
                .form(&[("creation_id", container_id.as_str())]),
            token,
        )?;
        Ok(PublishReceipt { object_id: graph::str_at(&published, "/id"), url: None })
    }

    /// Media caption editing is not on the paper-verified Instagram surface.
    fn edit(
        &self,
        _ctx: &CallContext,
        _object_id: &str,
        _req: &PublishRequest,
    ) -> Result<PublishReceipt, ConnectorError> {
        Err(ConnectorError::Unsupported {
            platform: Platform::Instagram,
            reason: "Instagram media editing is not on the paper-verified surface (paper section 2.3)"
                .to_string(),
        })
    }

    /// Media deletion is NOT offered by the official Instagram API.
    fn delete(&self, _ctx: &CallContext, _object_id: &str) -> Result<(), ConnectorError> {
        Err(ConnectorError::Unsupported {
            platform: Platform::Instagram,
            reason: "the official Instagram API offers no media deletion".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connector(base_url: &str, token: Option<&str>) -> InstagramConnector {
        let mut cfg = InstagramConfig::new("ig1");
        cfg.base_url = base_url.to_string();
        cfg.access_token = token.map(str::to_string);
        cfg.poll_interval = Duration::from_millis(1);
        InstagramConnector::new(cfg).unwrap()
    }

    #[test]
    fn delete_and_media_less_and_carousel_publish_are_honest_refusals() {
        let c = connector("http://127.0.0.1:9", Some("t"));
        let ctx = CallContext { secret: Some("t".into()) };
        assert!(matches!(
            c.delete(&ctx, "m1"),
            Err(ConnectorError::Unsupported { .. })
        ));
        assert!(matches!(
            c.edit(&ctx, "m1", &PublishRequest { body: "x".into(), media_urls: vec![] }),
            Err(ConnectorError::Unsupported { .. })
        ));
        // media-less publish (no network hop needed — refused before dispatch).
        assert!(matches!(
            c.publish(&ctx, &PublishRequest { body: "x".into(), media_urls: vec![] }),
            Err(ConnectorError::Unsupported { .. })
        ));
        // carousel (>1 media).
        assert!(matches!(
            c.publish(
                &ctx,
                &PublishRequest {
                    body: "x".into(),
                    media_urls: vec!["a".into(), "b".into()]
                }
            ),
            Err(ConnectorError::Unsupported { .. })
        ));
    }

    #[test]
    fn no_credential_fails_closed() {
        let c = connector("http://127.0.0.1:9", None);
        assert!(matches!(
            c.read_comments(&CallContext::none(), "m1"),
            Err(ConnectorError::MissingCredential { .. })
        ));
    }

    #[test]
    fn config_debug_redacts_the_token() {
        let mut cfg = InstagramConfig::new("ig1");
        cfg.access_token = Some("tok-SENSITIVE".into());
        assert!(!format!("{cfg:?}").contains("tok-SENSITIVE"));
    }
}
