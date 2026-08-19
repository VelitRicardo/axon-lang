//! v2.77.0 — the Facebook Pages native core: the first REAL connector.
//!
//! Owned-only posture: every operation acts on a Page the tenant owns,
//! through the official Pages API surface the paper verified (section 2.2): publish
//! (`pages_manage_posts`), comment moderation + deletion
//! (`pages_manage_engagement`), reads + insights (`pages_read_engagement`).
//! Post EDITING was not paper-verified and is therefore [`ConnectorError::
//! Unsupported`] — never emulated (the v2.67.0 posture).
//!
//! **Credentials.** The token is resolved PER CALL: the v2.48.0 custody injection
//! ([`CallContext::secret`]) takes precedence; the connector's configured token
//! is the dev/OSS fallback; neither ⇒ [`ConnectorError::MissingCredential`]
//! (fail-closed, never an unauthenticated vendor call). The token travels as an
//! `Authorization: Bearer` header — never in a URL, a log, or a payload.
//!
//! **Transport.** Blocking `reqwest` — the runtime already isolates dispatch in
//! `spawn_blocking` (Brief #63), and a sync core keeps the trait dyn-safe.
//! `base_url` + `graph_version` are configuration: tests point them at a
//! recorded-fixture server; production points at Meta (whose versioned-release
//! cadence means the pinned default WILL sunset — configure per deployment).

use std::time::Duration;

use crate::connector::{
    CallContext, Comment, ConnectorError, Metrics, ModerationAction, PublishReceipt,
    PublishRequest, Reaction, SocialConnector,
};
use crate::graph;
use crate::platform::Platform;

/// Default Graph API base. Overridable for fixture servers + regional gateways.
pub const DEFAULT_GRAPH_BASE: &str = "https://graph.facebook.com";
/// Default Graph API version. Meta sunsets versions on a rolling cadence —
/// deployments should pin the version they were reviewed against.
pub const DEFAULT_GRAPH_VERSION: &str = "v21.0";

/// Configuration for a [`FacebookPagesConnector`].
#[derive(Clone)]
pub struct FacebookPagesConfig {
    /// The owned Page this connector publishes to (`/{page_id}/feed`).
    pub page_id: String,
    /// Dev/OSS fallback token. In the enterprise runtime the per-tenant token
    /// arrives per-call via custody ([`CallContext::secret`]) and takes
    /// precedence; leave `None` there.
    pub access_token: Option<String>,
    /// Graph API base URL (no trailing slash). Tests: the fixture server.
    pub base_url: String,
    /// Graph API version segment (`v21.0`).
    pub graph_version: String,
    /// Per-request timeout.
    pub timeout: Duration,
}

impl FacebookPagesConfig {
    pub fn new(page_id: impl Into<String>) -> FacebookPagesConfig {
        FacebookPagesConfig {
            page_id: page_id.into(),
            access_token: None,
            base_url: DEFAULT_GRAPH_BASE.to_string(),
            graph_version: DEFAULT_GRAPH_VERSION.to_string(),
            timeout: Duration::from_secs(30),
        }
    }
}

// The v2.48.0 redacting-Debug discipline: the fallback token never reaches a log.
impl std::fmt::Debug for FacebookPagesConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FacebookPagesConfig")
            .field("page_id", &self.page_id)
            .field("access_token", &self.access_token.as_ref().map(|_| "<redacted>"))
            .field("base_url", &self.base_url)
            .field("graph_version", &self.graph_version)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// The Facebook Pages connector core (v2.77.0).
pub struct FacebookPagesConnector {
    config: FacebookPagesConfig,
    client: reqwest::blocking::Client,
}

impl FacebookPagesConnector {
    pub fn new(config: FacebookPagesConfig) -> Result<FacebookPagesConnector, ConnectorError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| ConnectorError::Transport(format!("client build: {e}")))?;
        Ok(FacebookPagesConnector { config, client })
    }

    fn url(&self, path: &str) -> String {
        graph::url(&self.config.base_url, &self.config.graph_version, path)
    }

    /// Per-call token: custody injection first (v2.48.0), configured fallback
    /// second, fail-closed third.
    fn token<'a>(&'a self, ctx: &'a CallContext) -> Result<&'a str, ConnectorError> {
        ctx.secret
            .as_deref()
            .or(self.config.access_token.as_deref())
            .ok_or(ConnectorError::MissingCredential { platform: Platform::FacebookPages })
    }

    fn execute(
        &self,
        req: reqwest::blocking::RequestBuilder,
        token: &str,
    ) -> Result<serde_json::Value, ConnectorError> {
        graph::execute(req, token)
    }
}

impl SocialConnector for FacebookPagesConnector {
    fn platform(&self) -> Platform {
        Platform::FacebookPages
    }

    fn name(&self) -> &'static str {
        "facebook-graph"
    }

    /// `GET /{target}/comments` — comments on a Page post (born Untrusted).
    fn read_comments(
        &self,
        ctx: &CallContext,
        target: &str,
    ) -> Result<Vec<Comment>, ConnectorError> {
        let token = self.token(ctx)?;
        let body = self.execute(
            self.client
                .get(self.url(&format!("{target}/comments")))
                .query(&[("fields", "id,from{name},message")]),
            token,
        )?;
        let comments = body
            .pointer("/data")
            .and_then(|v| v.as_array())
            .map(|rows| {
                rows.iter()
                    .map(|r| Comment {
                        id: graph::str_at(r, "/id"),
                        author: graph::str_at(r, "/from/name"),
                        text: graph::str_at(r, "/message"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(comments)
    }

    /// Reactions ride the Graph `/{target}/reactions` edge with a summary per
    /// type; v1 surfaces the total. (The agora surface does not yet export a
    /// facebook read_reactions tool; this keeps the trait honest for direct
    /// core users.)
    fn read_reactions(
        &self,
        ctx: &CallContext,
        target: &str,
    ) -> Result<Vec<Reaction>, ConnectorError> {
        let token = self.token(ctx)?;
        let body = self.execute(
            self.client
                .get(self.url(&format!("{target}/reactions")))
                .query(&[("summary", "total_count")]),
            token,
        )?;
        let total = body
            .pointer("/summary/total_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        Ok(vec![Reaction { kind: "total".to_string(), count: total }])
    }

    /// `GET /{target}/insights` — Page-level metrics (impressions, post
    /// engagements, fans), mapped onto the uniform [`Metrics`] wire shape.
    fn read_metrics(&self, ctx: &CallContext, target: &str) -> Result<Metrics, ConnectorError> {
        let token = self.token(ctx)?;
        let body = self.execute(
            self.client
                .get(self.url(&format!("{target}/insights")))
                .query(&[("metric", "page_impressions,page_post_engagements,page_fans")]),
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
            impressions: metric("page_impressions"),
            engagements: metric("page_post_engagements"),
            followers: metric("page_fans"),
        })
    }

    /// `POST /{comment_id}/comments` — reply to a comment.
    fn reply(
        &self,
        ctx: &CallContext,
        comment_id: &str,
        text: &str,
    ) -> Result<PublishReceipt, ConnectorError> {
        let token = self.token(ctx)?;
        let body = self.execute(
            self.client
                .post(self.url(&format!("{comment_id}/comments")))
                .form(&[("message", text)]),
            token,
        )?;
        Ok(PublishReceipt { object_id: graph::str_at(&body, "/id"), url: None })
    }

    /// Hide: `POST /{comment_id}` with `is_hidden=true`. Delete:
    /// `DELETE /{comment_id}`. Both under `pages_manage_engagement`.
    fn moderate(
        &self,
        ctx: &CallContext,
        comment_id: &str,
        action: ModerationAction,
    ) -> Result<(), ConnectorError> {
        let token = self.token(ctx)?;
        match action {
            ModerationAction::Hide => {
                self.execute(
                    self.client
                        .post(self.url(comment_id))
                        .form(&[("is_hidden", "true")]),
                    token,
                )?;
            }
            ModerationAction::Delete => {
                self.execute(self.client.delete(self.url(comment_id)), token)?;
            }
        }
        Ok(())
    }

    /// Text → `POST /{page_id}/feed`; single photo → `POST /{page_id}/photos`
    /// with `url` + `caption` (published). **Multi-photo (v2.77.0)** = the
    /// two-step `attached_media` flow the paper (section 2.2) verified: each image is
    /// first uploaded as an UNPUBLISHED photo container
    /// (`POST /{page_id}/photos` with `published=false` → `{id}`), then a single
    /// `POST /{page_id}/feed` attaches all containers via
    /// `attached_media[i]={"media_fbid":<id>}`. One post carries the album — no
    /// silent degradation to a first-photo-only post.
    fn publish(
        &self,
        ctx: &CallContext,
        req: &PublishRequest,
    ) -> Result<PublishReceipt, ConnectorError> {
        let token = self.token(ctx)?;
        let page = &self.config.page_id;
        let body = match req.media_urls.len() {
            0 => self.execute(
                self.client
                    .post(self.url(&format!("{page}/feed")))
                    .form(&[("message", req.body.as_str())]),
                token,
            )?,
            1 => self.execute(
                self.client.post(self.url(&format!("{page}/photos"))).form(&[
                    ("url", req.media_urls[0].as_str()),
                    ("caption", req.body.as_str()),
                ]),
                token,
            )?,
            _ => {
                // v2.77.0 — step 1: upload each image as an UNPUBLISHED photo
                // container, collecting its fbid. A container with no returned id
                // is a hard failure (never attach a phantom media to the post).
                let mut fbids: Vec<String> = Vec::with_capacity(req.media_urls.len());
                for media in &req.media_urls {
                    let container = self.execute(
                        self.client.post(self.url(&format!("{page}/photos"))).form(&[
                            ("url", media.as_str()),
                            ("published", "false"),
                        ]),
                        token,
                    )?;
                    let fbid = graph::str_at(&container, "/id");
                    if fbid.is_empty() {
                        return Err(ConnectorError::Platform {
                            status: 502,
                            message: "unpublished photo upload returned no media id".to_string(),
                        });
                    }
                    fbids.push(fbid);
                }
                // Step 2: one feed post attaching every container. `attached_media[i]`
                // takes a JSON object as its value (built with serde so the fbid is
                // escaped, never string-concatenated).
                let mut form: Vec<(String, String)> = Vec::with_capacity(fbids.len() + 1);
                form.push(("message".to_string(), req.body.clone()));
                for (i, fbid) in fbids.iter().enumerate() {
                    form.push((
                        format!("attached_media[{i}]"),
                        serde_json::json!({ "media_fbid": fbid }).to_string(),
                    ));
                }
                self.execute(
                    self.client.post(self.url(&format!("{page}/feed"))).form(&form),
                    token,
                )?
            }
        };
        // /photos returns {id, post_id}; /feed returns {id}. The POST id is the
        // receipt; prefer post_id when present (the feed-visible object).
        let object_id = body
            .pointer("/post_id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| graph::str_at(&body, "/id"));
        Ok(PublishReceipt { object_id, url: None })
    }

    /// Post editing was NOT paper-verified for the Pages surface (section 2.2) — the
    /// op is honestly unsupported until verified, never emulated.
    fn edit(
        &self,
        _ctx: &CallContext,
        _object_id: &str,
        _req: &PublishRequest,
    ) -> Result<PublishReceipt, ConnectorError> {
        Err(ConnectorError::Unsupported {
            platform: Platform::FacebookPages,
            reason: "post editing is not on the paper-verified Pages surface (paper section 2.2)".to_string(),
        })
    }

    /// `DELETE /{object_id}` — delete a Page post (`pages_manage_engagement`).
    fn delete(&self, ctx: &CallContext, object_id: &str) -> Result<(), ConnectorError> {
        let token = self.token(ctx)?;
        self.execute(self.client.delete(self.url(object_id)), token)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_building_joins_base_version_and_path() {
        let mut cfg = FacebookPagesConfig::new("page1");
        cfg.base_url = "http://127.0.0.1:9/".to_string();
        let c = FacebookPagesConnector::new(cfg).unwrap();
        assert_eq!(c.url("page1/feed"), "http://127.0.0.1:9/v21.0/page1/feed");
        assert_eq!(c.url("/x"), "http://127.0.0.1:9/v21.0/x");
    }

    #[test]
    fn token_precedence_is_custody_then_config_then_fail_closed() {
        let mut cfg = FacebookPagesConfig::new("p");
        cfg.access_token = Some("config-token".into());
        let c = FacebookPagesConnector::new(cfg).unwrap();
        let custody = CallContext { secret: Some("custody-token".into()) };
        assert_eq!(c.token(&custody).unwrap(), "custody-token");
        assert_eq!(c.token(&CallContext::none()).unwrap(), "config-token");

        let bare = FacebookPagesConnector::new(FacebookPagesConfig::new("p")).unwrap();
        assert!(matches!(
            bare.token(&CallContext::none()),
            Err(ConnectorError::MissingCredential { .. })
        ));
    }

    #[test]
    fn config_debug_redacts_the_token() {
        let mut cfg = FacebookPagesConfig::new("p");
        cfg.access_token = Some("tok-SENSITIVE".into());
        let dbg = format!("{cfg:?}");
        assert!(!dbg.contains("tok-SENSITIVE"));
        assert!(dbg.contains("<redacted>"));
        let ctx = CallContext { secret: Some("tok-SENSITIVE".into()) };
        let dbg = format!("{ctx:?}");
        assert!(!dbg.contains("tok-SENSITIVE"));
    }

    #[test]
    fn edit_is_honestly_unsupported() {
        let c = FacebookPagesConnector::new(FacebookPagesConfig::new("p")).unwrap();
        let ctx = CallContext { secret: Some("t".into()) };
        let req = PublishRequest { body: "x".into(), media_urls: vec![] };
        assert!(matches!(
            c.edit(&ctx, "post1", &req),
            Err(ConnectorError::Unsupported { .. })
        ));
    }

    // v2.77.0 — the multi-photo `attached_media` flow is HTTP-driven (two-step:
    // unpublished containers → feed attach), so its coverage lives in the
    // recorded-fixture integration test (`facebook.rs`) where a
    // Graph-shaped server observes the two POST /photos + one POST /feed with
    // `attached_media`. There is nothing network-free left to assert here.
}
