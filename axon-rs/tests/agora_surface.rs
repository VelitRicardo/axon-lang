//! v2.77.0 — the axon-agora connector surface, through the REAL pipeline.
//!
//! No mock of the EMS by the EMS (the v2.76.0 discipline): these tests compile a
//! program that `import agora.*` through `axon_frontend::ems::compile_project`
//! against the crate's SHIPPED module surface (`axon-agora/modules/`), then
//! drive the linked IR into the runtime `ToolRegistry` and dispatch through the
//! registered connector — the exact shape `execute_server_flow` takes in
//! production (registry dispatch inside `spawn_blocking`).

use std::path::PathBuf;
use std::sync::Arc;

use axon::agora_runtime::{clear_agora_connectors, register_agora_connector, AgoraCallContext};
use axon::tool_registry::ToolRegistry;
use axon_agora::{
    required_scopes, Comment, ConnectorError, Metrics, ModerationAction, Operation, Platform,
    PublishReceipt, PublishRequest, Reaction, SocialConnector,
};
use axon_frontend::ems::{compile_project, EmsFailure, EmsOptions, EmsSuccess};

/// The shipped module surface, path-anchored to this crate (works from any CWD).
fn modules_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../axon-agora/modules")
}

fn compile_result(entry_source: &str) -> Result<EmsSuccess, EmsFailure> {
    // A per-test directory (thread + a monotonic salt) so parallel tests never
    // race on the same entry file. (Date/random are fine in a test binary.)
    let dir = std::env::temp_dir().join(format!(
        "v2.77.0-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let entry = dir.join("main.axon");
    std::fs::write(&entry, entry_source).expect("write entry");
    let opts = EmsOptions {
        modules_root: Some(modules_root()),
        use_cache: false,
        cache_dir: None,
    };
    compile_project(&entry, &opts)
}

fn compile(entry_source: &str) -> EmsSuccess {
    match compile_result(entry_source) {
        Ok(s) => s,
        Err(f) => panic!("agora surface failed to compile: {:?}", f),
    }
}

/// A credential granting every scope the shipped surface declares — so a flow
/// that USES the connectors is scope-covered (axon-T956). This is what an
/// adopter writes once, from their granted OAuth scopes.
const FULL_CREDENTIAL: &str = r#"credential AgoraAuth {
  ttl: 1h
  grants: [r_organization_social, w_organization_social, pages_read_engagement, pages_manage_engagement, pages_manage_posts, instagram_business_basic, instagram_business_manage_comments, instagram_business_content_publish, comment.list, video.list]
}
"#;

/// Serialises registry-touching tests (process-global connector registry).
static REG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct FixtureConnector;

impl SocialConnector for FixtureConnector {
    fn platform(&self) -> Platform {
        Platform::LinkedIn
    }
    fn name(&self) -> &'static str {
        "fixture"
    }
    fn read_comments(&self, _x: &AgoraCallContext, target: &str) -> Result<Vec<Comment>, ConnectorError> {
        Ok(vec![Comment {
            id: "c1".into(),
            author: "reader".into(),
            text: format!("about {target}"),
        }])
    }
    fn read_reactions(&self, _x: &AgoraCallContext, _t: &str) -> Result<Vec<Reaction>, ConnectorError> {
        Ok(Vec::new())
    }
    fn read_metrics(&self, _x: &AgoraCallContext, _t: &str) -> Result<Metrics, ConnectorError> {
        Ok(Metrics { impressions: 1, engagements: 1, followers: 1 })
    }
    fn reply(&self, _x: &AgoraCallContext, _c: &str, _t: &str) -> Result<PublishReceipt, ConnectorError> {
        Ok(PublishReceipt { object_id: "r1".into(), url: None })
    }
    fn moderate(
        &self,
        _x: &AgoraCallContext,
        _c: &str,
        _a: ModerationAction,
    ) -> Result<(), ConnectorError> {
        Ok(())
    }
    fn publish(
        &self,
        _x: &AgoraCallContext,
        req: &PublishRequest,
    ) -> Result<PublishReceipt, ConnectorError> {
        Ok(PublishReceipt { object_id: format!("post:{}", req.body), url: None })
    }
    fn edit(
        &self,
        _x: &AgoraCallContext,
        o: &str,
        _r: &PublishRequest,
    ) -> Result<PublishReceipt, ConnectorError> {
        Ok(PublishReceipt { object_id: o.into(), url: None })
    }
    fn delete(&self, _x: &AgoraCallContext, _o: &str) -> Result<(), ConnectorError> {
        Ok(())
    }
}

/// The full shipped surface — all four platform modules — resolves, links, and
/// type-checks clean through the real EMS (with the scopes granted). 6 modules:
/// entry + 4 platforms + the shared `agora.types` (the diamond, linked once).
#[test]
fn the_full_shipped_surface_compiles_clean_through_the_ems() {
    let success = compile(&format!(
        r#"import agora.linkedin.{{ linkedin_read_comments, linkedin_read_reactions, linkedin_page_analytics, linkedin_reply, linkedin_publish_post, linkedin_edit_post, linkedin_delete_post }}
import agora.facebook.{{ facebook_read_comments, facebook_page_insights, facebook_reply, facebook_moderate, facebook_publish_post, facebook_delete_post }}
import agora.instagram.{{ instagram_read_comments, instagram_insights, instagram_reply, instagram_moderate, instagram_publish_media }}
import agora.tiktok.{{ tiktok_read_comments, tiktok_video_metrics }}

{FULL_CREDENTIAL}
type Digest {{ text: String }}

flow SurfaceDigest(target: String) -> Digest {{
  use tiktok_video_metrics(target = "${{target}}")
  step Summarize {{
    ask: "Digest the metrics"
    output: Digest
  }}
}}
"#
    ));
    assert_eq!(success.module_count, 6, "entry + 4 platforms + agora.types");
    let tool = |name: &str| {
        success
            .ir
            .tools
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("linked IR is missing tool '{name}'"))
    };
    assert_eq!(tool("linkedin_read_comments").provider, "agora_linkedin");
    assert_eq!(tool("linkedin_read_comments").runtime, "read_comments");
    assert_eq!(tool("facebook_publish_post").provider, "agora_facebook");
    assert_eq!(tool("instagram_publish_media").runtime, "publish");
    assert_eq!(tool("tiktok_video_metrics").provider, "agora_tiktok");
    // v2.77.0 — every op declares its per-tenant custody key (v2.48.0). At
    // production dispatch this resolves the tenant's token into `axon_secret`,
    // which agora_runtime threads into the connector's CallContext.
    assert_eq!(tool("facebook_read_comments").secret, "agora.facebook.token");
    assert_eq!(tool("instagram_publish_media").secret, "agora.instagram.token");
    assert_eq!(tool("linkedin_publish_post").secret, "agora.linkedin.token");
    assert_eq!(tool("tiktok_video_metrics").secret, "agora.tiktok.token");
    // the design decision: TikTok is read-first — no publish op on its surface until v2.77.0.
    assert!(
        !success
            .ir
            .tools
            .iter()
            .any(|t| t.provider == "agora_tiktok" && t.runtime == "publish"),
        "the TikTok surface must not expose publish before v2.77.0"
    );
}

/// **The library-tier anti-drift gate**: every shipped `.axon` tool's
/// `requires:` scopes are EXACTLY the crate's `scope::required_scopes(platform,
/// op)`. The crate matrix is the single authority; the surface cannot drift
/// from it. Checked through the linked IR — the real compiled artifact.
#[test]
fn surface_requires_equals_the_crate_scope_matrix() {
    let success = compile(&format!(
        r#"import agora.linkedin.{{ linkedin_publish_post, linkedin_read_comments }}
import agora.facebook.{{ facebook_publish_post, facebook_moderate }}
import agora.instagram.{{ instagram_publish_media }}
import agora.tiktok.{{ tiktok_video_metrics }}

{FULL_CREDENTIAL}
type U {{ x: String }}
flow F() -> U {{ step S {{ ask: "x" output: U }} }}
"#
    ));
    let requires_of = |name: &str| -> Vec<String> {
        let mut r = success
            .ir
            .tools
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("missing '{name}'"))
            .requires
            .clone();
        r.sort();
        r
    };
    let matrix = |p: Platform, op: Operation| -> Vec<String> {
        let mut v: Vec<String> = required_scopes(p, op).iter().map(|s| s.to_string()).collect();
        v.sort();
        v
    };
    assert_eq!(requires_of("linkedin_publish_post"), matrix(Platform::LinkedIn, Operation::Publish));
    assert_eq!(requires_of("linkedin_read_comments"), matrix(Platform::LinkedIn, Operation::ReadComments));
    assert_eq!(requires_of("facebook_publish_post"), matrix(Platform::FacebookPages, Operation::Publish));
    assert_eq!(requires_of("facebook_moderate"), matrix(Platform::FacebookPages, Operation::Moderate));
    assert_eq!(requires_of("instagram_publish_media"), matrix(Platform::Instagram, Operation::Publish));
    assert_eq!(requires_of("tiktok_video_metrics"), matrix(Platform::TikTok, Operation::ReadMetrics));
}

/// **axon-T956**: a `use` of an agora tool whose scope the program's granted
/// set does not cover is refused — through the REAL EMS pipeline, naming the
/// missing scope and the fix.
#[test]
fn using_a_connector_without_its_scope_is_refused() {
    let failure = match compile_result(
        r#"import agora.linkedin.{ linkedin_publish_post }

type U { x: String }
flow F(body: String) -> U {
  use linkedin_publish_post(body = "${body}")
  step S { ask: "x" output: U }
}
"#,
    ) {
        Ok(_) => panic!("a publish with no granting credential must fail T956"),
        Err(f) => f,
    };
    let msg = format!("{failure:?}");
    assert!(msg.contains("axon-T956"), "expected T956, got: {msg}");
    assert!(msg.contains("w_organization_social"), "must name the missing scope: {msg}");
    assert!(msg.contains("credential"), "must name the fix: {msg}");
    assert!(msg.contains("granted"), "must name the granted-set concept: {msg}");
}

/// A `credential` granting the scope covers the same `use` — the coverage law
/// is satisfiable, not just refusing.
#[test]
fn granting_the_scope_covers_the_use() {
    let success = compile(
        r#"import agora.linkedin.{ linkedin_publish_post }

credential OrgAuth { ttl: 1h grants: [w_organization_social] }

type U { x: String }
flow F(body: String) -> U {
  use linkedin_publish_post(body = "${body}")
  step S { ask: "x" output: U }
}
"#,
    );
    assert!(success
        .ir
        .tools
        .iter()
        .any(|t| t.name == "linkedin_publish_post" && t.requires == ["w_organization_social"]));
}

/// The paper section 1.1 flagship shape: two platforms in one flow, the shared types
/// module imported by both (the v2.76.0 diamond — linked once, no collision).
#[test]
fn two_platforms_in_one_flow_link_as_a_diamond() {
    let success = compile(&format!(
        r#"import agora.linkedin.{{ linkedin_read_comments, linkedin_publish_post }}
import agora.facebook.{{ facebook_page_insights }}

{FULL_CREDENTIAL}
type EngagementBrief {{ summary: String }}

flow ManageEngagement(post: String, page: String) -> EngagementBrief {{
  use linkedin_read_comments(target = "${{post}}")
  use facebook_page_insights(target = "${{page}}")
  step Analyze {{
    ask: "Summarize sentiment and metrics"
    output: EngagementBrief
  }}
}}
"#
    ));
    assert_eq!(success.module_count, 4, "entry + linkedin + facebook + types(once)");
}

/// EMS → ToolRegistry → dispatch: the linked IR's tool specs register into the
/// runtime registry, and a registered connector serves the call — the exact
/// production shape (`register_from_ir` + registry dispatch). No connector ⇒
/// the typed the design decision refusal, also through the real registry.
#[test]
fn linked_ir_registers_and_dispatches_through_the_connector() {
    let _g = REG_LOCK.lock().unwrap();
    let success = compile(
        r#"import agora.linkedin.{ linkedin_read_comments }

credential OrgAuth { ttl: 1h grants: [r_organization_social] }

type Digest { text: String }

flow F(target: String) -> Digest {
  use linkedin_read_comments(target = "${target}")
  step S { ask: "digest" output: Digest }
}
"#,
    );
    let mut registry = ToolRegistry::new();
    registry.register_from_ir(&success.ir.tools);

    // Unregistered: typed refusal through the REAL registry dispatch arm.
    clear_agora_connectors();
    let refused = registry
        .dispatch("linkedin_read_comments", r#"{"target":"post-1"}"#)
        .expect("agora providers dispatch locally, never LLM-fallthrough");
    assert!(!refused.success);
    assert!(refused.output.contains("no agora connector is registered"));

    // Registered: the connector serves the call, born-Untrusted content rides.
    register_agora_connector(Arc::new(FixtureConnector));
    let served = registry
        .dispatch("linkedin_read_comments", r#"{"target":"post-1"}"#)
        .expect("dispatched locally");
    assert!(served.success, "got: {}", served.output);
    assert!(served.output.contains("about post-1"));
    clear_agora_connectors();
}
