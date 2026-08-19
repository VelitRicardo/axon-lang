//! §Fase 116.a — axon-agora governed social connectors: the OSS dispatch arm for
//! the `agora_*` tool providers, and the per-platform `SocialConnector` injection
//! seam (the [`crate::scrape_tool::register_scrape_fetcher`] /
//! [`crate::enrichment::register_provider`] shape).
//!
//! **What agora is.** The first official library of axon-lang: governed native
//! connectors for LinkedIn, Facebook Pages, Instagram, and TikTok, so a cognitive
//! agent acts directly inside those networks (read comments/reactions/metrics,
//! moderate, reply, publish) as one step in a multi-tool flow. The protocol layer
//! — the capability×scope matrix, the session-typed publish protocols, the
//! owned-only posture refusals, the consumable quotas — lives in the `axon-agora`
//! crate (`docs/papers/paper_axon_agora.md`); this module is the runtime seam.
//!
//! **Routing.** A surface `tool` declares `provider: agora_<platform>` (the
//! CLOSED §114.b catalog) and names its operation in `runtime:` — the same slug
//! role the field plays for `http` tools. Dispatch maps the provider to a
//! [`Platform`], the `runtime:` slug to an [`Operation`], parses the structured
//! body, and calls the registered connector's **typed** method inside the
//! caller's `spawn_blocking` (the Brief #63 isolation — this module does no
//! spawning of its own).
//!
//! **The honesty law (D104.6, inherited).** OSS ships NO connector. Every
//! platform typed-refuses until the host registers one via
//! [`register_agora_connector`] — never a fabricated comment, metric, or receipt.
//! The enterprise host registers its cores at boot (§116.c–f); tests register
//! in-process connectors that enforce prod invariants
//! (`feedback_mock_must_enforce_prod_invariants`).
//!
//! **Provenance.** Every connector result is born epistemically **Untrusted**
//! (⊥) — a comment read from a social network is attacker-controlled text
//! (§98/T908), and a vendor receipt is a vendor's claim. The taint rides
//! [`AgoraOutcome`] exactly like [`crate::enrichment::EnrichmentOutcome`].

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock, RwLock};

use axon_agora::{
    CallContext, ConnectorError, ModerationAction, Operation, Platform, PublishRequest, SocialConnector,
};

use crate::emcp::EpistemicTaint;
use crate::tool_executor::ToolResult;
use crate::tool_registry::ToolEntry;

// ════════════════════════════════════════════════════════════════════════════
//  The per-platform connector registry (the §98.g / §104.a injection shape)
// ════════════════════════════════════════════════════════════════════════════

fn registry() -> &'static RwLock<BTreeMap<Platform, Arc<dyn SocialConnector>>> {
    static REG: OnceLock<RwLock<BTreeMap<Platform, Arc<dyn SocialConnector>>>> = OnceLock::new();
    REG.get_or_init(|| RwLock::new(BTreeMap::new()))
}

/// Register a platform connector (the host calls this once at boot; §116.c–f
/// cores, or a test connector). Keyed by [`SocialConnector::platform`] —
/// replaces any prior registration for that platform.
pub fn register_agora_connector(connector: Arc<dyn SocialConnector>) {
    registry()
        .write()
        .expect("agora registry poisoned")
        .insert(connector.platform(), connector);
}

/// Clear every registered connector (back to the OSS typed-refusal default).
pub fn clear_agora_connectors() {
    registry().write().expect("agora registry poisoned").clear();
}

/// The active connector for `platform`, if one is registered.
pub fn active_agora_connector(platform: Platform) -> Option<Arc<dyn SocialConnector>> {
    registry()
        .read()
        .expect("agora registry poisoned")
        .get(&platform)
        .cloned()
}

// ════════════════════════════════════════════════════════════════════════════
//  Provenance-tagged outcome (the §104 EnrichmentOutcome shape)
// ════════════════════════════════════════════════════════════════════════════

/// The provenance-tagged outcome. `taint` is ALWAYS [`EpistemicTaint::Untrusted`]
/// — social content and vendor receipts are born ⊥ (§98/T908). The registry
/// integration flattens this to a [`ToolResult`].
#[derive(Debug, Clone)]
pub struct AgoraOutcome {
    pub result: ToolResult,
    pub taint: EpistemicTaint,
}

impl AgoraOutcome {
    fn ok(tool_name: &str, output: String) -> Self {
        AgoraOutcome {
            result: ToolResult { success: true, output, tool_name: tool_name.to_string() },
            taint: EpistemicTaint::Untrusted,
        }
    }
    fn err(tool_name: &str, message: String) -> Self {
        AgoraOutcome {
            result: ToolResult { success: false, output: message, tool_name: tool_name.to_string() },
            taint: EpistemicTaint::Untrusted,
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Dispatch
// ════════════════════════════════════════════════════════════════════════════

/// Dispatch an `agora_*` tool call. `argument` is the structured JSON body from
/// the `use Tool(k = v, …)` keyword form, or the interpolated single argument of
/// the legacy `use Tool on "…"` form (treated as the operation's primary
/// argument). Returns the [`ToolResult`] the registry integrates.
pub fn dispatch_agora(entry: &ToolEntry, argument: &str) -> ToolResult {
    dispatch_agora_outcome(entry, argument).result
}

/// The taint-carrying dispatch (used by IFC + tests).
pub fn dispatch_agora_outcome(entry: &ToolEntry, argument: &str) -> AgoraOutcome {
    // 1. Provider → platform. The §114.b catalog guarantees membership at
    //    compile time; an unmapped provider reaching here is a wiring defect,
    //    refused honestly (never a fallthrough).
    let Some(platform) = Platform::from_provider(&entry.provider) else {
        return AgoraOutcome::err(
            &entry.name,
            format!(
                "tool '{}' provider '{}' is not an agora platform — dispatch reached the \
                 agora arm with a non-agora provider (wiring defect, refused)",
                entry.name, entry.provider
            ),
        );
    };

    // 2. `runtime:` slug → operation.
    let op_slug = entry.runtime.trim();
    let Some(op) = Operation::parse(op_slug) else {
        let valid: Vec<&str> = Operation::ALL.iter().map(|o| o.as_str()).collect();
        return AgoraOutcome::err(
            &entry.name,
            format!(
                "tool '{}' (`provider: {}`) names operation `runtime: {}` which is not an \
                 agora operation. Valid operations: {}. The `runtime:` field carries the \
                 connector operation, the same slug role it plays for `http` tools.",
                entry.name,
                entry.provider,
                if op_slug.is_empty() { "<empty>" } else { op_slug },
                valid.join(" | ")
            ),
        );
    };

    // 3. Structured body (keyword form) or legacy single argument. The reserved
    //    §94.c `axon_secret` field is stripped BEFORE anything else can see it —
    //    a custody value never reaches a connector's typed args, a log line, or
    //    a vendor payload. It rides the per-call CallContext into the core
    //    (§116.c), where it becomes the Authorization header and nothing else.
    let mut body: serde_json::Value = match serde_json::from_str(argument) {
        Ok(v @ serde_json::Value::Object(_)) => v,
        _ => serde_json::Value::Null,
    };
    let secret = match body.as_object_mut() {
        Some(map) => map
            .remove("axon_secret")
            .and_then(|v| v.as_str().map(str::to_string)),
        None => None,
    };
    let call_ctx = CallContext { secret };
    let legacy = if body.is_null() { argument.trim() } else { "" };
    let get = |key: &str| -> Option<String> {
        body.get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| (!legacy.is_empty()).then(|| legacy.to_string()))
    };

    // 4. Connector lookup — the D104.6 honesty: no connector, typed refusal.
    let Some(connector) = active_agora_connector(platform) else {
        return AgoraOutcome::err(
            &entry.name,
            format!(
                "no agora connector is registered for platform '{}' — the runtime refuses \
                 rather than fabricate. The enterprise host registers its connector \
                 cores at boot; a standalone runtime registers one via \
                 axon::agora_runtime::register_agora_connector.",
                platform.as_str()
            ),
        );
    };

    // 5. Op → typed connector method. A missing required argument is a typed
    //    refusal naming the parameter (the fail-closed §94/§95 message shape).
    let missing = |param: &str| {
        AgoraOutcome::err(
            &entry.name,
            format!(
                "tool '{}' operation `{}` requires parameter `{}` and the call did not \
                 bind it — dispatch fails closed",
                entry.name,
                op.as_str(),
                param
            ),
        )
    };

    let encode = |value: serde_json::Value| match serde_json::to_string(&value) {
        Ok(json) => AgoraOutcome::ok(&entry.name, json),
        Err(e) => AgoraOutcome::err(&entry.name, format!("encode: {e}")),
    };
    let connector_err =
        |e: ConnectorError| AgoraOutcome::err(&entry.name, format!("agora {}: {e}", op.as_str()));

    match op {
        Operation::ReadComments => match get("target") {
            Some(target) => match connector.read_comments(&call_ctx, &target) {
                Ok(comments) => encode(to_value(&comments)),
                Err(e) => connector_err(e),
            },
            None => missing("target"),
        },
        Operation::ReadReactions => match get("target") {
            Some(target) => match connector.read_reactions(&call_ctx, &target) {
                Ok(reactions) => encode(to_value(&reactions)),
                Err(e) => connector_err(e),
            },
            None => missing("target"),
        },
        Operation::ReadMetrics => match get("target") {
            Some(target) => match connector.read_metrics(&call_ctx, &target) {
                Ok(metrics) => encode(to_value(&metrics)),
                Err(e) => connector_err(e),
            },
            None => missing("target"),
        },
        Operation::Reply => {
            let Some(comment_id) = get("comment_id") else {
                return missing("comment_id");
            };
            let Some(text) = body.get("text").and_then(|v| v.as_str()) else {
                return missing("text");
            };
            match connector.reply(&call_ctx, &comment_id, text) {
                Ok(receipt) => encode(to_value(&receipt)),
                Err(e) => connector_err(e),
            }
        }
        Operation::Moderate => {
            let Some(comment_id) = get("comment_id") else {
                return missing("comment_id");
            };
            let action_slug = body.get("action").and_then(|v| v.as_str()).unwrap_or("");
            let Some(action) = ModerationAction::parse(action_slug) else {
                return AgoraOutcome::err(
                    &entry.name,
                    format!(
                        "tool '{}' `moderate` action '{}' is not valid — expected `hide` or \
                         `delete`",
                        entry.name, action_slug
                    ),
                );
            };
            match connector.moderate(&call_ctx, &comment_id, action) {
                Ok(()) => encode(serde_json::json!({
                    "moderated": true, "comment_id": comment_id, "action": action_slug
                })),
                Err(e) => connector_err(e),
            }
        }
        Operation::Publish => {
            let req = match publish_request_from(&body, legacy) {
                Ok(r) => r,
                Err(param) => return missing(param),
            };
            match connector.publish(&call_ctx, &req) {
                Ok(receipt) => encode(to_value(&receipt)),
                Err(e) => connector_err(e),
            }
        }
        Operation::Edit => {
            let Some(object_id) = body.get("object_id").and_then(|v| v.as_str()) else {
                return missing("object_id");
            };
            let req = match publish_request_from(&body, "") {
                Ok(r) => r,
                Err(param) => return missing(param),
            };
            match connector.edit(&call_ctx, object_id, &req) {
                Ok(receipt) => encode(to_value(&receipt)),
                Err(e) => connector_err(e),
            }
        }
        Operation::Delete => match get("object_id") {
            Some(object_id) => match connector.delete(&call_ctx, &object_id) {
                Ok(()) => encode(serde_json::json!({ "deleted": true, "object_id": object_id })),
                Err(e) => connector_err(e),
            },
            None => missing("object_id"),
        },
    }
}

/// Serialize any `Serialize` into a `Value` (infallible for our wire types).
fn to_value<T: serde::Serialize>(t: &T) -> serde_json::Value {
    serde_json::to_value(t).unwrap_or(serde_json::Value::Null)
}

/// Build a [`PublishRequest`] from the structured body (`body` + optional
/// `media_urls`), or from the legacy single argument (the whole argument IS the
/// post body). `Err` names the missing parameter.
fn publish_request_from(
    body: &serde_json::Value,
    legacy: &str,
) -> Result<PublishRequest, &'static str> {
    if let Some(text) = body.get("body").and_then(|v| v.as_str()) {
        let media_urls = body
            .get("media_urls")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        return Ok(PublishRequest { body: text.to_string(), media_urls });
    }
    if !legacy.is_empty() {
        return Ok(PublishRequest { body: legacy.to_string(), media_urls: Vec::new() });
    }
    Err("body")
}

// Re-export the wire types so enterprise cores implement the seam via
// `axon::agora_runtime::…` without a separate direct dependency line.
pub use axon_agora::{
    CallContext as AgoraCallContext, Comment as AgoraComment,
    ConnectorError as AgoraConnectorError, FacebookPagesConfig, FacebookPagesConnector,
    Metrics as AgoraMetrics, ModerationAction as AgoraModerationAction,
    Operation as AgoraOperation, Platform as AgoraPlatform,
    PublishReceipt as AgoraPublishReceipt, PublishRequest as AgoraPublishRequest,
    Reaction as AgoraReaction, SocialConnector as AgoraSocialConnector,
};

// ════════════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use axon_agora::{Comment, Metrics, PublishReceipt, Reaction};
    use crate::tool_registry::ToolSource;

    /// Serialises the registry-touching tests — the connector registry is
    /// process-global (the §101/§104 `REG_LOCK` discipline).
    static REG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn entry(provider: &str, runtime: &str) -> ToolEntry {
        ToolEntry {
            name: "AgoraTool".into(),
            provider: provider.into(),
            timeout: String::new(),
            runtime: runtime.into(),
            resource_ref: String::new(),
            substrate: None,
            capacity: None,
            sandbox: None,
            max_results: None,
            output_schema: String::new(),
            effect_row: vec!["network".into()],
            parameters: Vec::new(),
            secret: String::new(),
            secret_partition: String::new(),
            source: ToolSource::Program,
            is_streaming: false,
            scrape: None,
        }
    }

    /// A test connector that enforces the prod invariants a real core carries
    /// (`feedback_mock_must_enforce_prod_invariants`): non-empty targets, and
    /// Instagram media deletion honestly Unsupported.
    struct TestConnector {
        platform: Platform,
    }

    impl SocialConnector for TestConnector {
        fn platform(&self) -> Platform {
            self.platform
        }
        fn name(&self) -> &'static str {
            "test-connector"
        }
        fn read_comments(&self, _ctx: &CallContext, target: &str) -> Result<Vec<Comment>, ConnectorError> {
            assert!(!target.is_empty(), "prod invariant: target is never empty");
            Ok(vec![Comment {
                id: "c1".into(),
                author: "alice".into(),
                text: format!("comment on {target}"),
            }])
        }
        fn read_reactions(&self, _ctx: &CallContext, _target: &str) -> Result<Vec<Reaction>, ConnectorError> {
            Ok(vec![Reaction { kind: "like".into(), count: 7 }])
        }
        fn read_metrics(&self, _ctx: &CallContext, _target: &str) -> Result<Metrics, ConnectorError> {
            Ok(Metrics { impressions: 100, engagements: 10, followers: 5 })
        }
        fn reply(&self, _ctx: &CallContext, comment_id: &str, text: &str) -> Result<PublishReceipt, ConnectorError> {
            Ok(PublishReceipt { object_id: format!("reply-to-{comment_id}-{text}"), url: None })
        }
        fn moderate(
            &self,
            _ctx: &CallContext,
            _comment_id: &str,
            _action: ModerationAction,
        ) -> Result<(), ConnectorError> {
            Ok(())
        }
        fn publish(&self, _ctx: &CallContext, req: &PublishRequest) -> Result<PublishReceipt, ConnectorError> {
            assert!(
                !req.body.contains("axon_secret"),
                "prod invariant: a custody value never reaches the connector payload"
            );
            Ok(PublishReceipt { object_id: "post-1".into(), url: Some("https://x/1".into()) })
        }
        fn edit(
            &self,
            _ctx: &CallContext,
            object_id: &str,
            _req: &PublishRequest,
        ) -> Result<PublishReceipt, ConnectorError> {
            Ok(PublishReceipt { object_id: object_id.into(), url: None })
        }
        fn delete(&self, _ctx: &CallContext, _object_id: &str) -> Result<(), ConnectorError> {
            if self.platform == Platform::Instagram {
                return Err(ConnectorError::Unsupported {
                    platform: Platform::Instagram,
                    reason: "media deletion is not offered by the official API".into(),
                });
            }
            Ok(())
        }
    }

    #[test]
    fn unregistered_platform_is_a_typed_refusal_never_a_fabrication() {
        let _g = REG_LOCK.lock().unwrap();
        clear_agora_connectors();
        for p in Platform::ALL {
            let out =
                dispatch_agora(&entry(p.provider(), "read_comments"), r#"{"target":"post"}"#);
            assert!(!out.success);
            assert!(
                out.output.contains("no agora connector is registered")
                    && out.output.contains(p.as_str()),
                "refusal must name the platform. Got: {}",
                out.output
            );
        }
    }

    #[test]
    fn registered_connector_dispatches_through_typed_methods() {
        let _g = REG_LOCK.lock().unwrap();
        clear_agora_connectors();
        register_agora_connector(Arc::new(TestConnector { platform: Platform::FacebookPages }));

        // Keyword form.
        let out = dispatch_agora(
            &entry("agora_facebook", "read_comments"),
            r#"{"target":"page-post-9"}"#,
        );
        assert!(out.success, "got: {}", out.output);
        assert!(out.output.contains("comment on page-post-9"));

        // Legacy `on "…"` form: the argument is the primary arg.
        let out = dispatch_agora(&entry("agora_facebook", "read_metrics"), "page-post-9");
        assert!(out.success);
        assert!(out.output.contains("\"impressions\":100"));

        // Publish with a media list.
        let out = dispatch_agora(
            &entry("agora_facebook", "publish"),
            r#"{"body":"hello world","media_urls":["https://img/1.png"]}"#,
        );
        assert!(out.success);
        assert!(out.output.contains("post-1"));
        clear_agora_connectors();
    }

    #[test]
    fn axon_secret_is_stripped_before_the_connector_sees_anything() {
        let _g = REG_LOCK.lock().unwrap();
        clear_agora_connectors();
        register_agora_connector(Arc::new(TestConnector { platform: Platform::FacebookPages }));
        // The §94.c injected field must never reach the connector or the output.
        let out = dispatch_agora(
            &entry("agora_facebook", "publish"),
            r#"{"body":"hi","axon_secret":"tok-SENSITIVE"}"#,
        );
        assert!(out.success);
        assert!(
            !out.output.contains("tok-SENSITIVE"),
            "custody value leaked into the outcome: {}",
            out.output
        );
        clear_agora_connectors();
    }

    #[test]
    fn unknown_operation_is_refused_naming_the_valid_set() {
        let _g = REG_LOCK.lock().unwrap();
        let out = dispatch_agora(&entry("agora_linkedin", "post"), "{}");
        assert!(!out.success);
        assert!(out.output.contains("not an agora operation"));
        assert!(out.output.contains("read_comments | read_reactions"));
        let out = dispatch_agora(&entry("agora_linkedin", ""), "{}");
        assert!(!out.success);
        assert!(out.output.contains("<empty>"));
    }

    #[test]
    fn missing_required_parameter_fails_closed_naming_it() {
        let _g = REG_LOCK.lock().unwrap();
        clear_agora_connectors();
        register_agora_connector(Arc::new(TestConnector { platform: Platform::LinkedIn }));
        let out = dispatch_agora(&entry("agora_linkedin", "reply"), r#"{"text":"hi"}"#);
        assert!(!out.success);
        assert!(out.output.contains("`comment_id`"));
        let out = dispatch_agora(&entry("agora_linkedin", "publish"), "{}");
        assert!(!out.success);
        assert!(out.output.contains("`body`"));
        clear_agora_connectors();
    }

    #[test]
    fn unsupported_platform_capability_is_honestly_refused() {
        let _g = REG_LOCK.lock().unwrap();
        clear_agora_connectors();
        register_agora_connector(Arc::new(TestConnector { platform: Platform::Instagram }));
        let out =
            dispatch_agora(&entry("agora_instagram", "delete"), r#"{"object_id":"m1"}"#);
        assert!(!out.success);
        assert!(out.output.contains("never"), "got: {}", out.output);
        clear_agora_connectors();
    }

    #[test]
    fn moderate_requires_a_valid_action() {
        let _g = REG_LOCK.lock().unwrap();
        clear_agora_connectors();
        register_agora_connector(Arc::new(TestConnector { platform: Platform::FacebookPages }));
        let out = dispatch_agora(
            &entry("agora_facebook", "moderate"),
            r#"{"comment_id":"c1","action":"ban"}"#,
        );
        assert!(!out.success);
        assert!(out.output.contains("expected `hide` or `delete`"));
        let out = dispatch_agora(
            &entry("agora_facebook", "moderate"),
            r#"{"comment_id":"c1","action":"hide"}"#,
        );
        assert!(out.success);
        clear_agora_connectors();
    }

    #[test]
    fn every_outcome_is_born_untrusted() {
        let _g = REG_LOCK.lock().unwrap();
        clear_agora_connectors();
        register_agora_connector(Arc::new(TestConnector { platform: Platform::TikTok }));
        let ok = dispatch_agora_outcome(
            &entry("agora_tiktok", "read_metrics"),
            r#"{"target":"v1"}"#,
        );
        assert!(matches!(ok.taint, EpistemicTaint::Untrusted));
        clear_agora_connectors();
        let refused = dispatch_agora_outcome(
            &entry("agora_tiktok", "read_metrics"),
            r#"{"target":"v1"}"#,
        );
        assert!(matches!(refused.taint, EpistemicTaint::Untrusted));
    }

    /// **The anti-drift gate**: the frontend's §114.b closed catalog and this
    /// module's dispatch arm name EXACTLY the same agora providers, and every
    /// [`Platform`] has its catalog row. A platform added to `axon-agora`
    /// without its catalog entry — or a catalog entry without a platform —
    /// fails here, not in production.
    #[test]
    fn catalog_and_platforms_agree_exactly() {
        let catalog_agora: Vec<&&str> = axon_frontend::type_checker::VALID_TOOL_PROVIDERS
            .iter()
            .filter(|p| p.starts_with("agora_"))
            .collect();
        assert_eq!(catalog_agora.len(), Platform::ALL.len());
        for p in Platform::ALL {
            assert!(
                catalog_agora.contains(&&p.provider()),
                "platform '{}' has no VALID_TOOL_PROVIDERS row",
                p.as_str()
            );
        }
        for c in catalog_agora {
            assert!(
                Platform::from_provider(c).is_some(),
                "catalog provider '{c}' maps to no Platform — a dead catalog entry"
            );
        }
    }
}
