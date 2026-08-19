//! v2.77.0 — the LinkedIn connector against a recorded-fixture REST server,
//! end-to-end through the REAL pipeline.
//!
//! The fixture enforces the LinkedIn REST invariants (Bearer + `LinkedIn-Version`
//! + `X-Restli-Protocol-Version` headers), replays the Social Metadata /
//! socialActions + organizationalEntityShareStatistics shapes, returns the
//! created post URN in the `x-restli-id` header, and — for one version — replays
//! a SUNSET error so the connector's fail-safe hint is exercised.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axon::agora_runtime::{clear_agora_connectors, register_agora_connector};
use axon::tool_registry::ToolRegistry;
use axon_agora::{
    CallContext, ConnectorError, LinkedInConfig, LinkedInConnector, ModerationAction,
    PublishRequest, SocialConnector,
};
use axon_frontend::ems::{compile_project, EmsOptions};

/// (method, path, linkedin-version header, restli header, auth header).
type Seen = Arc<Mutex<Vec<(String, String, String, String, String)>>>;

fn spawn_li_fixture() -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let seen_srv = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let seen = seen_srv.clone();
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 1024];
                let header_end = loop {
                    match stream.read(&mut tmp) {
                        Ok(0) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                        Err(_) => return,
                    }
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break p;
                    }
                    if buf.len() > 65536 {
                        return;
                    }
                };
                let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let mut lines = head.lines();
                let request_line = lines.next().unwrap_or_default().to_string();
                let (mut version, mut restli, mut auth, mut clen) =
                    (String::new(), String::new(), String::new(), 0usize);
                for line in lines {
                    let low = line.to_ascii_lowercase();
                    let val = |line: &str| line.splitn(2, ':').nth(1).unwrap_or("").trim().to_string();
                    if low.starts_with("linkedin-version:") {
                        version = val(line);
                    } else if low.starts_with("x-restli-protocol-version:") {
                        restli = val(line);
                    } else if low.starts_with("authorization:") {
                        auth = val(line);
                    } else if low.starts_with("content-length:") {
                        clen = val(line).parse().unwrap_or(0);
                    }
                }
                let mut have = buf.len() - (header_end + 4);
                while have < clen {
                    match stream.read(&mut tmp) {
                        Ok(0) => break,
                        Ok(n) => have += n,
                        Err(_) => break,
                    }
                }
                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let path = parts.next().unwrap_or_default().to_string();
                let p = path.split('?').next().unwrap_or(&path).to_string();
                seen.lock().unwrap().push((
                    method.clone(),
                    p.clone(),
                    version.clone(),
                    restli.clone(),
                    auth.clone(),
                ));

                let (status, extra_header, body) = route(&method, &p, &version, &auth);
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra_header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            });
        }
    });
    (format!("http://{addr}"), seen)
}

fn route(method: &str, p: &str, version: &str, auth: &str) -> (&'static str, String, String) {
    if auth != "Bearer test-token" {
        return (
            "401 Unauthorized",
            String::new(),
            r#"{"message":"Invalid access token","serviceErrorCode":65600,"status":401}"#.to_string(),
        );
    }
    // Fail-safe: version 200001 is "sunset".
    if version == "200001" {
        return (
            "426 Upgrade Required",
            String::new(),
            r#"{"message":"API version is sunset","serviceErrorCode":100,"status":426}"#.to_string(),
        );
    }
    let share = "urn%3Ali%3Ashare%3A123";
    match (method, p) {
        ("GET", pp) if pp == format!("/rest/socialActions/{share}/comments") => (
            "200 OK",
            String::new(),
            r#"{"elements":[{"id":"c1","actor":"urn:li:person:x","message":{"text":"nice"}}]}"#.to_string(),
        ),
        ("GET", pp) if pp == format!("/rest/socialActions/{share}/reactions") => (
            "200 OK",
            String::new(),
            r#"{"elements":[{"reactionType":"LIKE"},{"reactionType":"LIKE"},{"reactionType":"PRAISE"}]}"#.to_string(),
        ),
        ("GET", "/rest/organizationalEntityShareStatistics") => (
            "200 OK",
            String::new(),
            r#"{"elements":[{"totalShareStatistics":{"impressionCount":500,"engagement":40,"uniqueImpressionsCount":300}}]}"#.to_string(),
        ),
        ("POST", "/rest/posts") => (
            "201 Created",
            "x-restli-id: urn:li:share:NEW999\r\n".to_string(),
            "{}".to_string(),
        ),
        ("POST", "/rest/posts/urn%3Ali%3Ashare%3ANEW999") => ("200 OK", String::new(), "{}".to_string()),
        ("DELETE", "/rest/posts/urn%3Ali%3Ashare%3ANEW999") => ("200 OK", String::new(), "{}".to_string()),
        ("POST", pp) if pp == format!("/rest/socialActions/{share}/comments") => {
            ("200 OK", String::new(), r#"{"id":"urn:li:comment:reply1"}"#.to_string())
        }
        _ => (
            "404 Not Found",
            String::new(),
            format!(r#"{{"message":"unknown {method} {p}","serviceErrorCode":0,"status":404}}"#),
        ),
    }
}

fn connector(base: &str, version: &str) -> LinkedInConnector {
    let mut cfg = LinkedInConfig::new("urn:li:organization:99");
    cfg.base_url = base.to_string();
    cfg.access_token = Some("test-token".into());
    cfg.api_version = version.to_string();
    cfg.timeout = Duration::from_secs(5);
    LinkedInConnector::new(cfg).unwrap()
}

#[test]
fn reads_use_the_social_metadata_surface_with_the_version_headers() {
    let (base, seen) = spawn_li_fixture();
    let c = connector(&base, "202506");
    let comments = c.read_comments(&CallContext::none(), "urn:li:share:123").expect("comments");
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text, "nice");

    let reactions = c.read_reactions(&CallContext::none(), "urn:li:share:123").expect("reactions");
    // Reaction types BEYOND likes are summarized (Social Metadata, section 2.1).
    assert!(reactions.iter().any(|r| r.kind == "LIKE" && r.count == 2));
    assert!(reactions.iter().any(|r| r.kind == "PRAISE" && r.count == 1));

    let m = c.read_metrics(&CallContext::none(), "").expect("stats");
    assert_eq!((m.impressions, m.engagements, m.followers), (500, 40, 300));

    // Every request carried the LinkedIn REST headers.
    let log = seen.lock().unwrap();
    assert!(log.iter().all(|(_, _, v, r, _)| v == "202506" && r == "2.0.0"));
}

#[test]
fn publish_edit_delete_the_full_owned_org_lifecycle() {
    let (base, _seen) = spawn_li_fixture();
    let c = connector(&base, "202506");
    let ctx = CallContext::none();
    // publish reads the created URN from the x-restli-id header.
    let receipt = c
        .publish(&ctx, &PublishRequest { body: "org update".into(), media_urls: vec![] })
        .expect("publish");
    assert_eq!(receipt.object_id, "urn:li:share:NEW999");
    // edit + delete ARE on the paper-verified LinkedIn surface (section 2.1).
    c.edit(&ctx, "urn:li:share:NEW999", &PublishRequest { body: "edited".into(), media_urls: vec![] })
        .expect("edit");
    c.delete(&ctx, "urn:li:share:NEW999").expect("delete");
}

#[test]
fn a_sunset_marketing_version_fails_safe_with_an_upgrade_hint() {
    let (base, _seen) = spawn_li_fixture();
    let c = connector(&base, "200001"); // the fixture treats this as sunset
    let err = c.read_comments(&CallContext::none(), "urn:li:share:123").unwrap_err();
    match err {
        ConnectorError::Platform { status, message } => {
            assert_eq!(status, 426);
            assert!(message.contains("sunset"), "got: {message}");
            assert!(message.contains("200001"), "must name the pinned version: {message}");
            assert!(message.contains("upgrade"), "must name the fix: {message}");
        }
        other => panic!("expected Platform 426 with upgrade hint, got: {other}"),
    }
}

#[test]
fn hide_is_unsupported_at_the_connector() {
    let (base, _seen) = spawn_li_fixture();
    let c = connector(&base, "202506");
    assert!(matches!(
        c.moderate(&CallContext::none(), "urn:li:comment:1", ModerationAction::Hide),
        Err(ConnectorError::Unsupported { .. })
    ));
}

#[test]
fn ems_to_registry_to_linkedin_read_e2e() {
    static REG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = REG_LOCK.lock().unwrap();
    let (base, _seen) = spawn_li_fixture();

    let dir = std::env::temp_dir().join(format!("v2.77.0-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("main.axon");
    std::fs::write(
        &entry,
        r#"import agora.linkedin.{ linkedin_read_comments }

credential OrgAuth { ttl: 1h grants: [r_organization_social] }

type R { text: String }

flow ReadLi(post: String) -> R {
  use linkedin_read_comments(target = "${post}")
  step S { ask: "digest" output: R }
}
"#,
    )
    .unwrap();
    let opts = EmsOptions {
        modules_root: Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../axon-agora/modules")),
        use_cache: false,
        cache_dir: None,
    };
    let success = compile_project(&entry, &opts).expect("compile li flow");

    let mut registry = ToolRegistry::new();
    registry.register_from_ir(&success.ir.tools);
    clear_agora_connectors();
    register_agora_connector(Arc::new(connector(&base, "202506")));

    let out = registry
        .dispatch(
            "linkedin_read_comments",
            r#"{"target":"urn:li:share:123","axon_secret":"test-token"}"#,
        )
        .expect("dispatched locally");
    assert!(out.success, "got: {}", out.output);
    assert!(out.output.contains("nice"), "got: {}", out.output);
    clear_agora_connectors();
}
