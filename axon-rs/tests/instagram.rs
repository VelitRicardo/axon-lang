//! v2.77.0 — the Instagram connector: the session-typed container publish
//! protocol (create → poll → publish), quota reconciliation, and the honest
//! refusals, against a recorded-fixture Graph server, end-to-end through the
//! REAL pipeline.
//!
//! The fixture enforces the prod invariant (Bearer required) and scripts the
//! container `status_code` sequence so a test can exercise the IN_PROGRESS →
//! FINISHED poll loop, the ERROR failure, and the quota ceiling — the typestate
//! the paper mandates (section 2.3), made real.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axon::agora_runtime::{clear_agora_connectors, register_agora_connector};
use axon::tool_registry::ToolRegistry;
use axon_agora::{
    CallContext, ConnectorError, InstagramConfig, InstagramConnector, PublishRequest,
    SocialConnector,
};
use axon_frontend::ems::{compile_project, EmsOptions};

type Seen = Arc<Mutex<Vec<(String, String)>>>;

/// A Graph fixture for Instagram. `quota_usage` is what
/// `content_publishing_limit` reports; `statuses` is the container status_code
/// sequence (each container GET pops the front; the last repeats).
fn spawn_ig_fixture(quota_usage: u64, statuses: &[&str]) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let seen_srv = seen.clone();
    let script: Arc<Mutex<VecDeque<String>>> =
        Arc::new(Mutex::new(statuses.iter().map(|s| s.to_string()).collect()));

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let (seen, script) = (seen_srv.clone(), script.clone());
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
                let mut auth = String::new();
                let mut content_length = 0usize;
                for line in lines {
                    let low = line.to_ascii_lowercase();
                    if let Some(v) = low.strip_prefix("authorization:") {
                        auth = line[line.len() - v.trim_start().len()..].trim().to_string();
                    }
                    if let Some(v) = low.strip_prefix("content-length:") {
                        content_length = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut have = buf.len() - (header_end + 4);
                while have < content_length {
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
                seen.lock().unwrap().push((method.clone(), p.clone()));

                let (status, body) = if auth != "Bearer test-token" {
                    (
                        "401 Unauthorized",
                        r#"{"error":{"message":"Invalid OAuth access token.","code":190}}"#.to_string(),
                    )
                } else {
                    ig_route(&method, &p, quota_usage, &script)
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            });
        }
    });
    (format!("http://{addr}"), seen)
}

fn ig_route(
    method: &str,
    p: &str,
    quota_usage: u64,
    script: &Arc<Mutex<VecDeque<String>>>,
) -> (&'static str, String) {
    match (method, p) {
        ("GET", "/v21.0/ig1/content_publishing_limit") => {
            ("200 OK", format!(r#"{{"data":[{{"quota_usage":{quota_usage}}}]}}"#))
        }
        ("POST", "/v21.0/ig1/media") => ("200 OK", r#"{"id":"container1"}"#.to_string()),
        ("GET", "/v21.0/container1") => {
            let mut q = script.lock().unwrap();
            let status = if q.len() > 1 { q.pop_front().unwrap() } else { q.front().cloned().unwrap_or_else(|| "FINISHED".into()) };
            ("200 OK", format!(r#"{{"status_code":"{status}"}}"#))
        }
        ("POST", "/v21.0/ig1/media_publish") => {
            ("200 OK", r#"{"id":"ig_media_9"}"#.to_string())
        }
        ("GET", "/v21.0/m1/comments") => (
            "200 OK",
            r#"{"data":[{"id":"c1","username":"fan","text":"love this"}]}"#.to_string(),
        ),
        ("POST", "/v21.0/c1/replies") => ("200 OK", r#"{"id":"reply1"}"#.to_string()),
        _ => (
            "404 Not Found",
            format!(r#"{{"error":{{"message":"unknown {method} {p}","code":100}}}}"#),
        ),
    }
}

fn connector(base: &str) -> InstagramConnector {
    let mut cfg = InstagramConfig::new("ig1");
    cfg.base_url = base.to_string();
    cfg.access_token = Some("test-token".into());
    cfg.timeout = Duration::from_secs(5);
    cfg.poll_interval = Duration::from_millis(1);
    cfg.poll_max_attempts = 5;
    InstagramConnector::new(cfg).unwrap()
}

#[test]
fn publish_drives_the_container_protocol_create_poll_publish() {
    // Container finishes on the second poll (IN_PROGRESS → FINISHED).
    let (base, seen) = spawn_ig_fixture(0, &["IN_PROGRESS", "FINISHED"]);
    let c = connector(&base);
    let receipt = c
        .publish(
            &CallContext::none(),
            &PublishRequest { body: "hello ig".into(), media_urls: vec!["https://img/1.jpg".into()] },
        )
        .expect("publish");
    assert_eq!(receipt.object_id, "ig_media_9");

    // The protocol ran in order: limit check → create container → poll(s) → publish.
    let log: Vec<String> = seen.lock().unwrap().iter().map(|(m, p)| format!("{m} {p}")).collect();
    let idx = |needle: &str| log.iter().position(|l| l.contains(needle)).expect(needle);
    assert!(idx("content_publishing_limit") < idx("/ig1/media"));
    assert!(idx("/ig1/media") < idx("/container1"));
    assert!(idx("/container1") < idx("/ig1/media_publish"));
    // Two polls happened (IN_PROGRESS then FINISHED).
    assert_eq!(log.iter().filter(|l| l.contains("/container1")).count(), 2);
}

#[test]
fn a_container_error_is_a_typed_failure_never_a_half_post() {
    let (base, seen) = spawn_ig_fixture(0, &["ERROR"]);
    let c = connector(&base);
    let err = c
        .publish(
            &CallContext::none(),
            &PublishRequest { body: "x".into(), media_urls: vec!["https://img/1.jpg".into()] },
        )
        .unwrap_err();
    match err {
        ConnectorError::Platform { status, message } => {
            assert_eq!(status, 422);
            assert!(message.contains("ERROR"), "got: {message}");
        }
        other => panic!("expected Platform 422, got: {other}"),
    }
    // media_publish was NEVER called — no half-published post.
    assert!(!seen.lock().unwrap().iter().any(|(_, p)| p.contains("media_publish")));
}

#[test]
fn the_quota_ceiling_refuses_before_creating_a_container() {
    // At 100/24h usage, publishing is refused (paper section 2.3 quota).
    let (base, seen) = spawn_ig_fixture(100, &["FINISHED"]);
    let c = connector(&base);
    let err = c
        .publish(
            &CallContext::none(),
            &PublishRequest { body: "x".into(), media_urls: vec!["https://img/1.jpg".into()] },
        )
        .unwrap_err();
    assert!(matches!(err, ConnectorError::QuotaExhausted), "got: {err}");
    // The container was NEVER created — the quota gate is pre-flight.
    assert!(!seen.lock().unwrap().iter().any(|(m, p)| m == "POST" && p == "/v21.0/ig1/media"));
}

#[test]
fn reads_map_the_recorded_shapes() {
    let (base, _seen) = spawn_ig_fixture(0, &["FINISHED"]);
    let c = connector(&base);
    let comments = c.read_comments(&CallContext::none(), "m1").expect("comments");
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].author, "fan");
    assert_eq!(comments[0].text, "love this");
}

/// The production shape: EMS-compile a flow that reads Instagram comments,
/// register the linked IR + the real core, dispatch through the registry — the
/// Instagram connector runs across the exact path `execute_server_flow` takes.
/// (Publishing's container protocol is proven by the direct-core tests above;
/// the List-typed `media_urls` arg binding is a separate grammar concern.)
#[test]
fn ems_to_registry_to_instagram_read_e2e() {
    static REG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = REG_LOCK.lock().unwrap();
    let (base, _seen) = spawn_ig_fixture(0, &["FINISHED"]);

    let dir = std::env::temp_dir().join(format!("v2.77.0-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("main.axon");
    std::fs::write(
        &entry,
        r#"import agora.instagram.{ instagram_read_comments }

credential IgAuth { ttl: 1h grants: [instagram_business_basic] }

type R { text: String }

flow ReadIg(media: String) -> R {
  use instagram_read_comments(target = "${media}")
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
    let success = compile_project(&entry, &opts).expect("compile ig flow");

    let mut registry = ToolRegistry::new();
    registry.register_from_ir(&success.ir.tools);
    clear_agora_connectors();
    register_agora_connector(Arc::new(connector(&base)));

    let out = registry
        .dispatch(
            "instagram_read_comments",
            r#"{"target":"m1","axon_secret":"test-token"}"#,
        )
        .expect("dispatched locally");
    assert!(out.success, "got: {}", out.output);
    assert!(out.output.contains("love this"), "got: {}", out.output);
    clear_agora_connectors();
}
