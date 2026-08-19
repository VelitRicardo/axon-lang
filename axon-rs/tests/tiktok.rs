//! v2.77.0 — the TikTok connector (read/analytics-first) against a
//! recorded-fixture Open API server, end-to-end through the REAL pipeline.
//!
//! The fixture replays the distinctive TikTok envelope
//! `{"data":…,"error":{"code":"ok"|…}}` — including a 200 whose `error.code` is
//! NOT `ok` (a failure the connector must catch by the envelope, not the HTTP
//! status). Publishing is proven refused at the connector (the per-post-consent
//! posture); the surface exports no TikTok publish tool.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axon::agora_runtime::{clear_agora_connectors, register_agora_connector};
use axon::tool_registry::ToolRegistry;
use axon_agora::{
    CallContext, ConnectorError, PublishRequest, SocialConnector, TikTokConfig, TikTokConnector,
};
use axon_frontend::ems::{compile_project, EmsOptions};

type Seen = Arc<Mutex<Vec<(String, String)>>>;

fn spawn_tt_fixture() -> (String, Seen) {
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
                let (mut auth, mut clen) = (String::new(), 0usize);
                for line in lines {
                    let low = line.to_ascii_lowercase();
                    if let Some(v) = low.strip_prefix("authorization:") {
                        auth = line[line.len() - v.trim_start().len()..].trim().to_string();
                    }
                    if let Some(v) = low.strip_prefix("content-length:") {
                        clen = v.trim().parse().unwrap_or(0);
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
                seen.lock().unwrap().push((method.clone(), p.clone()));

                // TikTok's tell: a WRONG token returns HTTP 200 with an error
                // envelope, not a 4xx. The connector must catch it by the envelope.
                let body = if auth != "Bearer test-token" {
                    r#"{"data":{},"error":{"code":"access_token_invalid","message":"The access token is invalid","log_id":"1"}}"#.to_string()
                } else {
                    match p.as_str() {
                        "/v2/video/comment/list/" => {
                            r#"{"data":{"comments":[{"id":"c1","text":"first!","username":"fan"}]},"error":{"code":"ok","message":"","log_id":"2"}}"#.to_string()
                        }
                        "/v2/video/query/" => {
                            r#"{"data":{"videos":[{"id":"v1","view_count":10000,"like_count":800,"comment_count":120,"share_count":45}]},"error":{"code":"ok","message":"","log_id":"3"}}"#.to_string()
                        }
                        _ => r#"{"data":{},"error":{"code":"not_found","message":"unknown route","log_id":"4"}}"#.to_string(),
                    }
                };
                // TikTok returns 200 even for the envelope-error case.
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            });
        }
    });
    (format!("http://{addr}"), seen)
}

fn connector(base: &str, token: Option<&str>) -> TikTokConnector {
    let mut cfg = TikTokConfig::new();
    cfg.base_url = base.to_string();
    cfg.access_token = token.map(str::to_string);
    cfg.timeout = Duration::from_secs(5);
    TikTokConnector::new(cfg).unwrap()
}

#[test]
fn reads_map_the_envelope_shapes() {
    let (base, _seen) = spawn_tt_fixture();
    let c = connector(&base, Some("test-token"));
    let comments = c.read_comments(&CallContext::none(), "v1").expect("comments");
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text, "first!");
    assert_eq!(comments[0].author, "fan");

    let m = c.read_metrics(&CallContext::none(), "v1").expect("metrics");
    assert_eq!(m.impressions, 10000);
    assert_eq!(m.engagements, 800 + 120 + 45);
    assert_eq!(m.followers, 0); // video-level: no follower count
}

#[test]
fn a_200_with_a_non_ok_error_code_is_caught_as_a_failure() {
    // The TikTok discipline: a wrong token returns HTTP 200 with error.code set.
    let (base, _seen) = spawn_tt_fixture();
    let c = connector(&base, Some("wrong-token"));
    let err = c.read_metrics(&CallContext::none(), "v1").unwrap_err();
    match err {
        ConnectorError::Platform { message, .. } => {
            assert!(message.contains("access_token_invalid"), "got: {message}");
        }
        other => panic!("a non-ok envelope must be a failure, got: {other}"),
    }
}

#[test]
fn publishing_is_refused_by_the_consent_posture() {
    let (base, _seen) = spawn_tt_fixture();
    let c = connector(&base, Some("test-token"));
    match c.publish(&CallContext::none(), &PublishRequest { body: "x".into(), media_urls: vec![] }) {
        Err(ConnectorError::Refused(r)) => {
            assert_eq!(r.code, "axon-T958");
            assert!(r.reason.contains("consent"));
        }
        other => panic!("TikTok publish must be Refused, got: {other:?}"),
    }
}

#[test]
fn ems_to_registry_to_tiktok_read_e2e() {
    static REG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = REG_LOCK.lock().unwrap();
    let (base, _seen) = spawn_tt_fixture();

    let dir = std::env::temp_dir().join(format!("v2.77.0-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("main.axon");
    std::fs::write(
        &entry,
        r#"import agora.tiktok.{ tiktok_video_metrics }

credential TtAuth { ttl: 1h grants: [video.list] }

type R { text: String }

flow ReadTt(video: String) -> R {
  use tiktok_video_metrics(target = "${video}")
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
    let success = compile_project(&entry, &opts).expect("compile tt flow");

    let mut registry = ToolRegistry::new();
    registry.register_from_ir(&success.ir.tools);
    clear_agora_connectors();
    register_agora_connector(Arc::new(connector(&base, None))); // token via custody

    let out = registry
        .dispatch(
            "tiktok_video_metrics",
            r#"{"target":"v1","axon_secret":"test-token"}"#,
        )
        .expect("dispatched locally");
    assert!(out.success, "got: {}", out.output);
    assert!(out.output.contains("10000"), "got: {}", out.output);
    clear_agora_connectors();
}
