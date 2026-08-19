//! §Fase 116.e — the LinkedIn native core: the richest governance posture.
//!
//! Owned-only posture (D116.3) is a **construction invariant** here: a
//! [`LinkedInConnector`] is built for an ORGANIZATION URN, and
//! [`LinkedInConnector::new`] REFUSES a member (`urn:li:person:`) author — the
//! paper's central finding (member-level automation is prohibited, API ToS
//! §3.1(26); `r_member_social` is closed) made a runtime invariant, not just a
//! compile-time refusal (`axon-T958`). The connector cannot post as a member; it
//! never had the path.
//!
//! **Transport is NOT Graph.** LinkedIn's REST API takes `Authorization: Bearer`
//! plus a `LinkedIn-Version` header (`YYYYMM`) and `X-Restli-Protocol-Version:
//! 2.0.0`, and returns a `{"message","serviceErrorCode","status"}` error
//! envelope. The version is CONFIG, not a constant — LinkedIn sunsets Marketing
//! versions on a rolling cadence (paper §2.1), so a deployment pins the version
//! it was reviewed against, and a sunset version surfaces a CLEAR typed error
//! (fail-safe), never a silent break.
//!
//! **Reads use the Social Metadata / socialActions surface** (comments +
//! reactions, the reaction types beyond likes) — the replacement for the legacy
//! `socialActions` endpoint the paper flagged (§2.1). Analytics come from
//! `organizationalEntityShareStatistics`.
//!
//! Unlike Facebook/Instagram, post **edit and delete ARE on the paper-verified
//! surface** (§2.1: "creating, updating, and deleting organization posts").

use std::time::Duration;

use crate::connector::{
    CallContext, Comment, ConnectorError, Metrics, ModerationAction, PublishReceipt,
    PublishRequest, Reaction, SocialConnector,
};
use crate::platform::Platform;
use crate::posture::PostureRefusal;

pub const DEFAULT_API_BASE: &str = "https://api.linkedin.com";
/// A recent Marketing version (`YYYYMM`). Pin per deployment — LinkedIn sunsets
/// versions on a cadence, and a sunset version fails safe (see module docs).
pub const DEFAULT_API_VERSION: &str = "202506";

/// Configuration for a [`LinkedInConnector`].
#[derive(Clone)]
pub struct LinkedInConfig {
    /// The owned ORGANIZATION URN the connector acts as (`urn:li:organization:123`).
    pub author_org_urn: String,
    pub access_token: Option<String>,
    pub base_url: String,
    /// The `LinkedIn-Version` header value (`YYYYMM`).
    pub api_version: String,
    pub timeout: Duration,
}

impl LinkedInConfig {
    pub fn new(author_org_urn: impl Into<String>) -> LinkedInConfig {
        LinkedInConfig {
            author_org_urn: author_org_urn.into(),
            access_token: None,
            base_url: DEFAULT_API_BASE.to_string(),
            api_version: DEFAULT_API_VERSION.to_string(),
            timeout: Duration::from_secs(30),
        }
    }
}

impl std::fmt::Debug for LinkedInConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkedInConfig")
            .field("author_org_urn", &self.author_org_urn)
            .field("access_token", &self.access_token.as_ref().map(|_| "<redacted>"))
            .field("base_url", &self.base_url)
            .field("api_version", &self.api_version)
            .finish()
    }
}

/// The LinkedIn connector core (§116.e).
pub struct LinkedInConnector {
    config: LinkedInConfig,
    client: reqwest::blocking::Client,
}

impl LinkedInConnector {
    /// Build a connector for an ORGANIZATION. **Refuses a member URN** — the
    /// owned-only posture made a construction invariant (the paper's member-
    /// automation prohibition; `axon-T958` runtime twin).
    pub fn new(config: LinkedInConfig) -> Result<LinkedInConnector, ConnectorError> {
        if config.author_org_urn.starts_with("urn:li:person:") {
            return Err(ConnectorError::Refused(PostureRefusal {
                code: "axon-T958",
                reason: "LinkedIn member-level automation is prohibited (API ToS section 3.1(26); \
                         r_member_social is a closed permission) — a connector cannot act as a \
                         member.",
                fix: "Build the connector for an organization URN (urn:li:organization:<id>) \
                      under an approved Community Management use case.",
                source: "paper_axon_agora.md section 2.1 [L-TOS]",
            }));
        }
        if !config.author_org_urn.starts_with("urn:li:organization:") {
            return Err(ConnectorError::Refused(PostureRefusal {
                code: "axon-T958",
                reason: "the LinkedIn connector acts only as an owned organization.",
                fix: "Set author_org_urn to urn:li:organization:<id>.",
                source: "paper_axon_agora.md section 2.1",
            }));
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| ConnectorError::Transport(format!("client build: {e}")))?;
        Ok(LinkedInConnector { config, client })
    }

    fn token<'a>(&'a self, ctx: &'a CallContext) -> Result<&'a str, ConnectorError> {
        ctx.secret
            .as_deref()
            .or(self.config.access_token.as_deref())
            .ok_or(ConnectorError::MissingCredential { platform: Platform::LinkedIn })
    }

    fn rest_url(&self, path: &str) -> String {
        format!("{}/rest/{}", self.config.base_url.trim_end_matches('/'), path.trim_start_matches('/'))
    }

    /// Add the LinkedIn REST headers (version + restli protocol + Bearer).
    fn with_headers(
        &self,
        req: reqwest::blocking::RequestBuilder,
        token: &str,
    ) -> reqwest::blocking::RequestBuilder {
        req.bearer_auth(token)
            .header("LinkedIn-Version", &self.config.api_version)
            .header("X-Restli-Protocol-Version", "2.0.0")
    }

    /// Send + map LinkedIn's error envelope. A version-sunset error is surfaced
    /// with an explicit upgrade hint (fail-safe), never swallowed.
    fn json(
        &self,
        req: reqwest::blocking::RequestBuilder,
        token: &str,
    ) -> Result<serde_json::Value, ConnectorError> {
        let resp = self
            .with_headers(req, token)
            .send()
            .map_err(|e| ConnectorError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let body: serde_json::Value = resp
            .json()
            .map_err(|e| ConnectorError::Transport(format!("non-JSON response: {e}")))?;
        if status >= 400 {
            return Err(self.map_error(status, &body));
        }
        Ok(body)
    }

    /// Map a LinkedIn error body to a typed error, with a sunset-version hint.
    fn map_error(&self, status: u16, body: &serde_json::Value) -> ConnectorError {
        let mut message = body
            .pointer("/message")
            .and_then(|v| v.as_str())
            .unwrap_or("(no LinkedIn error message)")
            .to_string();
        // Fail-safe on a sunset Marketing version: name the pinned version + the fix.
        let low = message.to_ascii_lowercase();
        if status == 426 || low.contains("version") && (low.contains("sunset") || low.contains("deprecat") || low.contains("invalid")) {
            message = format!(
                "{message} — the pinned LinkedIn-Version '{}' appears sunset/invalid; upgrade \
                 the connector's api_version to a supported YYYYMM (LinkedIn sunsets Marketing \
                 versions on a cadence, paper section 2.1).",
                self.config.api_version
            );
        }
        ConnectorError::Platform { status, message }
    }

    fn encode_urn(urn: &str) -> String {
        urn.replace(':', "%3A")
    }
}

fn str_at(v: &serde_json::Value, pointer: &str) -> String {
    v.pointer(pointer).and_then(|x| x.as_str()).unwrap_or_default().to_string()
}

impl SocialConnector for LinkedInConnector {
    fn platform(&self) -> Platform {
        Platform::LinkedIn
    }

    fn name(&self) -> &'static str {
        "linkedin-rest"
    }

    /// `GET /rest/socialActions/{shareUrn}/comments` — Social Metadata comments.
    fn read_comments(
        &self,
        ctx: &CallContext,
        target: &str,
    ) -> Result<Vec<Comment>, ConnectorError> {
        let token = self.token(ctx)?;
        let body = self.json(
            self.client.get(self.rest_url(&format!(
                "socialActions/{}/comments",
                Self::encode_urn(target)
            ))),
            token,
        )?;
        let comments = body
            .pointer("/elements")
            .and_then(|v| v.as_array())
            .map(|rows| {
                rows.iter()
                    .map(|r| Comment {
                        id: str_at(r, "/id"),
                        author: str_at(r, "/actor"),
                        text: str_at(r, "/message/text"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(comments)
    }

    /// `GET /rest/socialActions/{shareUrn}/reactions` — reaction types beyond
    /// likes (the Social Metadata surface, paper §2.1). Summarized per type.
    fn read_reactions(
        &self,
        ctx: &CallContext,
        target: &str,
    ) -> Result<Vec<Reaction>, ConnectorError> {
        let token = self.token(ctx)?;
        let body = self.json(
            self.client.get(self.rest_url(&format!(
                "socialActions/{}/reactions",
                Self::encode_urn(target)
            ))),
            token,
        )?;
        let mut counts: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
        if let Some(rows) = body.pointer("/elements").and_then(|v| v.as_array()) {
            for r in rows {
                let kind = str_at(r, "/reactionType");
                if !kind.is_empty() {
                    *counts.entry(kind).or_insert(0) += 1;
                }
            }
        }
        Ok(counts.into_iter().map(|(kind, count)| Reaction { kind, count }).collect())
    }

    /// `GET /rest/organizationalEntityShareStatistics` — org analytics.
    fn read_metrics(&self, ctx: &CallContext, _target: &str) -> Result<Metrics, ConnectorError> {
        let token = self.token(ctx)?;
        let body = self.json(
            self.client
                .get(self.rest_url("organizationalEntityShareStatistics"))
                .query(&[
                    ("q", "organizationalEntity"),
                    ("organizationalEntity", &self.config.author_org_urn),
                ]),
            token,
        )?;
        let stat = |name: &str| -> u64 {
            body.pointer(&format!("/elements/0/totalShareStatistics/{name}"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
        };
        Ok(Metrics {
            impressions: stat("impressionCount"),
            engagements: stat("engagement"),
            followers: stat("uniqueImpressionsCount"),
        })
    }

    /// `POST /rest/socialActions/{shareUrn}/comments` — comment as the org.
    fn reply(
        &self,
        ctx: &CallContext,
        comment_id: &str,
        text: &str,
    ) -> Result<PublishReceipt, ConnectorError> {
        let token = self.token(ctx)?;
        let payload = serde_json::json!({
            "actor": self.config.author_org_urn,
            "message": { "text": text },
        });
        let body = self.json(
            self.client
                .post(self.rest_url(&format!("socialActions/{}/comments", Self::encode_urn(comment_id))))
                .json(&payload),
            token,
        )?;
        Ok(PublishReceipt { object_id: str_at(&body, "/id"), url: None })
    }

    /// Delete a comment: `DELETE /rest/socialActions/{share}/comments/{id}`.
    /// LinkedIn has no "hide" — [`ModerationAction::Hide`] is honestly Unsupported.
    fn moderate(
        &self,
        ctx: &CallContext,
        comment_id: &str,
        action: ModerationAction,
    ) -> Result<(), ConnectorError> {
        let token = self.token(ctx)?;
        match action {
            ModerationAction::Hide => Err(ConnectorError::Unsupported {
                platform: Platform::LinkedIn,
                reason: "LinkedIn has no hide-comment operation — use delete".to_string(),
            }),
            ModerationAction::Delete => {
                self.json(
                    self.client.delete(self.rest_url(&format!(
                        "comments/{}",
                        Self::encode_urn(comment_id)
                    ))),
                    token,
                )?;
                Ok(())
            }
        }
    }

    /// `POST /rest/posts` — an organization post. The created post URN rides
    /// the `x-restli-id` response header.
    fn publish(
        &self,
        ctx: &CallContext,
        req: &PublishRequest,
    ) -> Result<PublishReceipt, ConnectorError> {
        let token = self.token(ctx)?;
        let payload = serde_json::json!({
            "author": self.config.author_org_urn,
            "commentary": req.body,
            "visibility": "PUBLIC",
            "distribution": { "feedDistribution": "MAIN_FEED", "targetEntities": [], "thirdPartyDistributionChannels": [] },
            "lifecycleState": "PUBLISHED",
            "isReshareDisabledByAuthor": false,
        });
        let resp = self
            .with_headers(self.client.post(self.rest_url("posts")).json(&payload), token)
            .send()
            .map_err(|e| ConnectorError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        // The post URN is returned in the x-restli-id header (before the body).
        let urn = resp
            .headers()
            .get("x-restli-id")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        if status >= 400 {
            let body: serde_json::Value = resp.json().unwrap_or(serde_json::Value::Null);
            return Err(self.map_error(status, &body));
        }
        Ok(PublishReceipt {
            object_id: urn.unwrap_or_default(),
            url: None,
        })
    }

    /// `POST /rest/posts/{urn}` PARTIAL_UPDATE — update the commentary
    /// (paper §2.1: organization posts are updatable).
    fn edit(
        &self,
        ctx: &CallContext,
        object_id: &str,
        req: &PublishRequest,
    ) -> Result<PublishReceipt, ConnectorError> {
        let token = self.token(ctx)?;
        let payload = serde_json::json!({ "patch": { "$set": { "commentary": req.body } } });
        self.json(
            self.client
                .post(self.rest_url(&format!("posts/{}", Self::encode_urn(object_id))))
                .header("X-RestLi-Method", "PARTIAL_UPDATE")
                .json(&payload),
            token,
        )?;
        Ok(PublishReceipt { object_id: object_id.to_string(), url: None })
    }

    /// `DELETE /rest/posts/{urn}` — delete an organization post (paper §2.1).
    fn delete(&self, ctx: &CallContext, object_id: &str) -> Result<(), ConnectorError> {
        let token = self.token(ctx)?;
        self.json(
            self.client.delete(self.rest_url(&format!("posts/{}", Self::encode_urn(object_id)))),
            token,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_urn_author_is_refused_at_construction() {
        let cfg = LinkedInConfig::new("urn:li:person:abc");
        match LinkedInConnector::new(cfg) {
            Err(ConnectorError::Refused(r)) => {
                assert_eq!(r.code, "axon-T958");
                assert!(r.reason.contains("member-level automation"));
            }
            _ => panic!("a member author must be refused"),
        }
    }

    #[test]
    fn a_non_org_urn_is_refused() {
        assert!(matches!(
            LinkedInConnector::new(LinkedInConfig::new("urn:li:share:1")),
            Err(ConnectorError::Refused(_))
        ));
    }

    #[test]
    fn an_org_urn_builds() {
        assert!(LinkedInConnector::new(LinkedInConfig::new("urn:li:organization:99")).is_ok());
    }

    #[test]
    fn hide_is_unsupported_delete_is_the_moderation_path() {
        let c = LinkedInConnector::new(LinkedInConfig::new("urn:li:organization:1")).unwrap();
        let ctx = CallContext { secret: Some("t".into()) };
        assert!(matches!(
            c.moderate(&ctx, "urn:li:comment:1", ModerationAction::Hide),
            Err(ConnectorError::Unsupported { .. })
        ));
    }

    #[test]
    fn no_credential_fails_closed() {
        let mut cfg = LinkedInConfig::new("urn:li:organization:1");
        cfg.base_url = "http://127.0.0.1:9".into();
        let c = LinkedInConnector::new(cfg).unwrap();
        assert!(matches!(
            c.read_comments(&CallContext::none(), "urn:li:share:1"),
            Err(ConnectorError::MissingCredential { .. })
        ));
    }

    #[test]
    fn config_debug_redacts_the_token() {
        let mut cfg = LinkedInConfig::new("urn:li:organization:1");
        cfg.access_token = Some("tok-SENSITIVE".into());
        assert!(!format!("{cfg:?}").contains("tok-SENSITIVE"));
    }

    #[test]
    fn urn_encoding_escapes_colons() {
        assert_eq!(LinkedInConnector::encode_urn("urn:li:share:1"), "urn%3Ali%3Ashare%3A1");
    }
}
